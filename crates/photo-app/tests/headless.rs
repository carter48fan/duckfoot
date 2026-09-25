//! The whole app, driven frame by frame with no window and no GPU.
//!
//! wgpu's no-op backend supplies a real `Device` and `Queue` whose work goes nowhere, so
//! everything the app does short of producing pixels runs for real: the loader's threads,
//! decoding, the upload from the decode thread, `Engine::load_uploaded`, and egui laying
//! out every panel. Pixel correctness is `photo-engine`'s `bench` example's job.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Duration, Instant};

use photo_app::compact::Tab;
use photo_app::dam::FolderBookmark;
use photo_app::platform::{Platform, Request, Response};
use photo_app::{Gpu, PhotoApp};

struct Headless {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: RwLock<egui_wgpu::Renderer>,
    ctx: egui::Context,
}

impl Headless {
    fn new() -> Self {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::NOOP;
        desc.backend_options.noop.enable = true;
        let instance = wgpu::Instance::new(desc);
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("the no-op adapter");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("a no-op device");
        let renderer = RwLock::new(egui_wgpu::Renderer::new(
            &device,
            wgpu::TextureFormat::Rgba8Unorm,
            egui_wgpu::RendererOptions::default(),
        ));
        let ctx = egui::Context::default();
        photo_app::theme::apply(&ctx);
        Self {
            device,
            queue,
            renderer,
            ctx,
        }
    }

    fn gpu(&self) -> Gpu<'_> {
        Gpu {
            device: &self.device,
            queue: &self.queue,
            renderer: &self.renderer,
        }
    }

    /// One frame at 1440×900, the way the desktop shell runs one.
    fn frame(&self, app: &mut PhotoApp, input: egui::RawInput) -> egui::FullOutput {
        self.frame_sized(app, egui::vec2(1440.0, 900.0), input)
    }

    /// One frame on a screen of `size` points.
    fn frame_sized(
        &self,
        app: &mut PhotoApp,
        size: egui::Vec2,
        input: egui::RawInput,
    ) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..input
        };
        let gpu = self.gpu();
        let mut output = self.ctx.run_ui(input, |ui| app.update(ui, &gpu));
        // What the shell does with textures, so none leak or trip egui's checks.
        let mut delta = std::mem::take(&mut output.textures_delta);
        {
            let mut renderer = self.renderer.write().unwrap();
            for (id, deltas) in &delta.set {
                for d in deltas {
                    renderer.update_texture(&self.device, &self.queue, *id, d);
                }
            }
            for id in &delta.free {
                renderer.free_texture(id);
            }
        }
        delta.clear();
        output
    }

    /// Run frames until the app has nothing in flight, or panic after `limit`.
    fn settle(&self, app: &mut PhotoApp, limit: Duration) {
        let started = Instant::now();
        loop {
            self.frame(app, Default::default());
            if !app.is_busy() {
                return;
            }
            assert!(started.elapsed() < limit, "still busy after {limit:?}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("duckfoot-headless-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A small but real DNG — mosaic, colour matrix, embedded preview — via rawler's writer.
fn write_dng(path: &Path, w: usize, h: usize) {
    use rawler::decoders::Camera;
    use rawler::dng::writer::DngWriter;
    use rawler::dng::{CropMode, DngCompression, DngPhotometricConversion, DNG_VERSION_V1_4};
    use rawler::imgop::xyz::Illuminant;
    use rawler::rawimage::{BlackLevel, CFAConfig, RawPhotometricInterpretation, WhiteLevel};
    use rawler::{RawImage, RawImageData, CFA};

    let data = (0..w * h)
        .map(|i| 512 + ((i % w) * 12000 / w) as u16)
        .collect();
    let cfa = CFA::new("RGGB");
    let camera = Camera {
        cfa: cfa.clone(),
        ..Default::default()
    };
    let mut color_matrix = HashMap::new();
    color_matrix.insert(
        Illuminant::D65,
        vec![
            0.7460, -0.2365, -0.0588, -0.5687, 1.3442, 0.2474, -0.0624, 0.1156, 0.6584,
        ],
    );
    let raw = RawImage {
        camera: camera.clone(),
        make: "SONY".into(),
        model: "ILCE-7M4".into(),
        clean_make: "Sony".into(),
        clean_model: "ILCE-7M4".into(),
        width: w,
        height: h,
        cpp: 1,
        bps: 14,
        wb_coeffs: [2.4, 1.0, 1.6, f32::NAN],
        whitelevel: WhiteLevel::new(vec![16383]),
        blacklevel: BlackLevel::new(&[512u16; 4], 2, 2, 1),
        xyz_to_cam: [[0.0; 3]; 4],
        photometric: RawPhotometricInterpretation::Cfa(CFAConfig::new(&cfa, &camera.plane_color)),
        active_area: None,
        crop_area: None,
        blackareas: Vec::new(),
        orientation: rawler::Orientation::Normal,
        data: RawImageData::Integer(data),
        color_matrix,
        dng_tags: HashMap::new(),
        fuji_rotation_width: None,
    };
    let preview = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        (w / 2) as u32,
        (h / 2) as u32,
        image::Rgb([120, 110, 100]),
    ));
    let mut buf = Cursor::new(Vec::new());
    let mut dng = DngWriter::new(&mut buf, DNG_VERSION_V1_4).unwrap();
    let mut sub = dng.subframe(0);
    sub.raw_image(
        &raw,
        CropMode::Best,
        DngCompression::Uncompressed,
        DngPhotometricConversion::Original,
        1,
    )
    .unwrap();
    sub.finalize().unwrap();
    let mut prev = dng.subframe(1);
    prev.preview(&preview, 0.9).unwrap();
    prev.finalize().unwrap();
    dng.load_base_tags(&raw).unwrap();
    dng.close().unwrap();
    std::fs::write(path, buf.into_inner()).unwrap();
}

#[test]
fn raws_and_jpegs_open_through_the_upload_thread() {
    let dir = scratch_dir("open");
    let (raw, jpeg) = (dir.join("a.dng"), dir.join("b.jpg"));
    write_dng(&raw, 640, 426);
    image::RgbImage::from_pixel(600, 400, image::Rgb([200, 100, 50]))
        .save(&jpeg)
        .unwrap();

    let h = Headless::new();
    let mut app = PhotoApp::new(&h.gpu());
    h.frame(&mut app, Default::default());

    // Shelving opens the first photo; the thumbnails build alongside.
    app.add_to_shelf(&h.gpu(), vec![raw.clone(), jpeg.clone()]);
    h.settle(&mut app, Duration::from_secs(30));
    assert_eq!(app.error(), None);
    assert_eq!(app.loaded_path(), Some(raw.as_path()));

    // A JPEG takes the raster upload; a second RAW of the same size would reuse the pooled
    // mosaic, which a return to the first exercises.
    app.open_path(&h.gpu(), jpeg.clone());
    h.settle(&mut app, Duration::from_secs(30));
    assert_eq!(app.error(), None);
    assert_eq!(app.loaded_path(), Some(jpeg.as_path()));

    app.open_path(&h.gpu(), raw.clone());
    h.settle(&mut app, Duration::from_secs(30));
    assert_eq!(app.error(), None);
    assert_eq!(app.loaded_path(), Some(raw.as_path()));

    // A file that is not a RAW fails in place, naming the file once, and leaves the photo
    // that was open on the stage.
    let broken = dir.join("c.dng");
    std::fs::write(&broken, b"II*\0not really a raw file").unwrap();
    app.open_path(&h.gpu(), broken.clone());
    h.settle(&mut app, Duration::from_secs(30));
    let error = app.error().expect("no error shown");
    assert_eq!(error.matches("c.dng").count(), 1, "{error}");
    assert_eq!(app.loaded_path(), Some(raw.as_path()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_large_roll_paints_only_the_tiles_in_view() {
    let dir = scratch_dir("roll");
    let h = Headless::new();
    let mut app = PhotoApp::new(&h.gpu());
    h.frame(&mut app, Default::default());

    let count_shapes = |app: &mut PhotoApp| {
        // Two frames: egui settles some layout on the first after a change.
        h.frame(app, Default::default());
        h.frame(app, Default::default()).shapes.len()
    };
    let empty = count_shapes(&mut app);

    // Files that do not exist: each thumbnail fails fast, and the tiles are placeholders.
    let few: Vec<PathBuf> = (0..20).map(|i| dir.join(format!("{i:05}.arw"))).collect();
    let many: Vec<PathBuf> = (20..2000)
        .map(|i| dir.join(format!("{i:05}.arw")))
        .collect();
    // Nothing loads, so nothing is opened; only the shelf and filmstrip grow.
    app.add_to_shelf(&h.gpu(), few);
    h.settle(&mut app, Duration::from_secs(30));
    let with_20 = count_shapes(&mut app);

    app.add_to_shelf(&h.gpu(), many);
    h.settle(&mut app, Duration::from_secs(60));
    let started = Instant::now();
    let with_2000 = count_shapes(&mut app);
    let frame = started.elapsed() / 2;

    assert_eq!(app.shelf.len(), 2000);
    // A 1440-wide window shows about a dozen tiles. Twenty already overflow it, so
    // another 1980 must add nothing to the filmstrip — only the shelf list, which is
    // a separate panel, can grow.
    let filmstrip_growth = with_2000.saturating_sub(with_20);
    println!(
        "shapes: empty {empty}, 20 photos {with_20}, 2000 photos {with_2000}; frame {frame:?}"
    );
    assert!(
        filmstrip_growth < 2000,
        "2000 photos painted {with_2000} shapes against {with_20} for 20: tiles out of view \
         are being drawn"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A platform the test answers for, recording what the app asked of it.
#[derive(Clone, Default)]
struct FakePlatform(std::sync::Arc<std::sync::Mutex<FakeState>>);

#[derive(Default)]
struct FakeState {
    requests: Vec<Request>,
    answers: Vec<Response>,
    leaves: usize,
}

impl FakePlatform {
    fn answer(&self, response: Response) {
        self.0.lock().unwrap().answers.push(response);
    }
}

impl Platform for FakePlatform {
    fn request(&mut self, request: Request) {
        self.0.lock().unwrap().requests.push(request);
    }

    fn poll(&mut self) -> Vec<Response> {
        std::mem::take(&mut self.0.lock().unwrap().answers)
    }

    fn bookmarks(&self) -> Vec<FolderBookmark> {
        vec![FolderBookmark {
            name: "Library".into(),
            path: PathBuf::from("/library"),
        }]
    }

    fn leave(&mut self) {
        self.0.lock().unwrap().leaves += 1;
    }
}

fn key(key: egui::Key) -> egui::RawInput {
    egui::RawInput {
        events: vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
        ..Default::default()
    }
}

/// The Android shell answers pickers some frames after they were asked, from another
/// activity; the app must act on an answer whenever it arrives.
#[test]
fn platform_answers_and_back_are_handled() {
    let dir = scratch_dir("platform");
    let raw = dir.join("picked.dng");
    write_dng(&raw, 640, 426);

    let h = Headless::new();
    let platform = FakePlatform::default();
    let mut app = PhotoApp::with_platform(&h.gpu(), Box::new(platform.clone()));
    assert_eq!(app.folder_tree.bookmarks[0].name, "Library");
    h.frame(&mut app, Default::default());

    platform.answer(Response::OpenImage(raw.clone()));
    h.settle(&mut app, Duration::from_secs(30));
    assert_eq!(app.error(), None);
    assert_eq!(app.loaded_path(), Some(raw.as_path()));

    platform.answer(Response::BatchOutputDir(dir.join("out")));
    h.frame(&mut app, Default::default());
    assert_eq!(app.batch_output_dir, Some(dir.join("out")));

    // Back closes what is open before it leaves the app.
    app.batch_dialog_open = true;
    h.frame(&mut app, key(egui::Key::BrowserBack));
    assert!(!app.batch_dialog_open);
    assert_eq!(platform.0.lock().unwrap().leaves, 0);
    h.frame(&mut app, key(egui::Key::BrowserBack));
    assert_eq!(platform.0.lock().unwrap().leaves, 1);

    assert!(platform.0.lock().unwrap().requests.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

fn pointer(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        events,
        ..Default::default()
    }
}

/// Press at `from`, move to `to` over a few frames, release.
fn drag(h: &Headless, app: &mut PhotoApp, from: egui::Pos2, to: egui::Pos2) {
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    h.frame(app, pointer(vec![egui::Event::PointerMoved(from)]));
    h.frame(app, pointer(vec![button(from, true)]));
    for i in 1..=8 {
        let p = from + (to - from) * (i as f32 / 8.0);
        h.frame(app, pointer(vec![egui::Event::PointerMoved(p)]));
    }
    h.frame(app, pointer(vec![button(to, false)]));
    h.frame(app, Default::default());
}

#[test]
fn a_crop_is_framed_then_applied_or_cancelled() {
    let dir = scratch_dir("crop");
    let raw = dir.join("a.dng");
    write_dng(&raw, 640, 426);
    let h = Headless::new();
    let mut app = PhotoApp::new(&h.gpu());
    h.frame(&mut app, Default::default());
    app.add_to_shelf(&h.gpu(), vec![raw.clone()]);
    h.settle(&mut app, Duration::from_secs(30));
    assert_eq!(app.error(), None);
    let full = app.stack().geometry.crop;
    assert!(full.is_full_frame());

    // The stage sits between the side panels; a drag across its middle lands on the photo.
    let (from, to) = (egui::pos2(560.0, 330.0), egui::pos2(760.0, 470.0));

    // Esc throws the framing away.
    h.frame(&mut app, key(egui::Key::C));
    assert!(app.is_cropping());
    drag(&h, &mut app, from, to);
    assert!(
        !app.stack().geometry.crop.is_full_frame(),
        "the drag did not frame a crop"
    );
    h.frame(&mut app, key(egui::Key::Escape));
    assert!(!app.is_cropping());
    assert_eq!(app.stack().geometry.crop, full);

    // Enter keeps it, and editing carries on in the cropped view.
    h.frame(&mut app, key(egui::Key::C));
    drag(&h, &mut app, from, to);
    let framed = app.stack().geometry.crop;
    h.frame(&mut app, key(egui::Key::Enter));
    assert!(!app.is_cropping());
    assert_eq!(app.stack().geometry.crop, framed);
    for _ in 0..3 {
        h.frame(&mut app, Default::default());
    }

    // Back in, a drag inside the crop slides it without resizing it.
    h.frame(&mut app, key(egui::Key::C));
    drag(
        &h,
        &mut app,
        egui::pos2(640.0, 400.0),
        egui::pos2(600.0, 380.0),
    );
    let moved = app.stack().geometry.crop;
    assert!((moved.width - framed.width).abs() < 1e-4);
    assert!((moved.height - framed.height).abs() < 1e-4);
    assert!(
        moved.x < framed.x && moved.y < framed.y,
        "{framed:?} -> {moved:?}"
    );
    h.frame(&mut app, key(egui::Key::Enter));

    // Each applied session is a single undo step, however many drags it took.
    let undo = pointer(vec![
        egui::Event::ModifiersChanged(egui::Modifiers::COMMAND),
        egui::Event::Key {
            key: egui::Key::Z,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        },
    ]);
    let release = || pointer(vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE)]);
    h.frame(&mut app, undo.clone());
    h.frame(&mut app, release());
    assert_eq!(app.stack().geometry.crop, framed);
    h.frame(&mut app, undo);
    h.frame(&mut app, release());
    assert_eq!(app.stack().geometry.crop, full);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A phone upright and on its side, in points.
const PORTRAIT: egui::Vec2 = egui::vec2(412.0, 892.0);
const LANDSCAPE: egui::Vec2 = egui::vec2(892.0, 412.0);

/// A tap: press and release at one point, over a few frames, as a finger's comes in.
fn tap(h: &Headless, app: &mut PhotoApp, size: egui::Vec2, at: egui::Pos2) {
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    h.frame_sized(app, size, pointer(vec![egui::Event::PointerMoved(at)]));
    h.frame_sized(app, size, pointer(vec![button(true)]));
    h.frame_sized(app, size, pointer(vec![button(false)]));
    h.frame_sized(app, size, pointer(vec![egui::Event::PointerGone]));
}

/// The centre of tab `i` of five in the bottom bar (portrait) or right rail (landscape).
fn tab_centre(size: egui::Vec2, i: usize) -> egui::Pos2 {
    if size.x > size.y {
        let rail = 68.0;
        let top = 34.0; // under the top bar
        let cell = (size.y - top) / 5.0;
        egui::pos2(size.x - rail / 2.0, top + cell * (i as f32 + 0.5))
    } else {
        let bar = 60.0;
        let cell = size.x / 5.0;
        egui::pos2(cell * (i as f32 + 0.5), size.y - bar / 2.0)
    }
}

/// Phones get the navigation-bar layout either way up; every sheet lays out with a photo
/// open, and the tabs open and close them.
#[test]
fn the_phone_layout_shows_every_sheet_and_tabs_switch_them() {
    let dir = scratch_dir("phone");
    let (a, b) = (dir.join("a.dng"), dir.join("b.dng"));
    write_dng(&a, 640, 426);
    write_dng(&b, 640, 426);

    let h = Headless::new();
    let mut app = PhotoApp::with_platform(&h.gpu(), Box::new(FakePlatform::default()));
    h.frame_sized(&mut app, PORTRAIT, Default::default());
    app.add_to_shelf(&h.gpu(), vec![a.clone(), b.clone()]);
    h.settle(&mut app, Duration::from_secs(30));
    assert_eq!(app.error(), None);

    for size in [PORTRAIT, LANDSCAPE] {
        for tab in Tab::ALL {
            app.compact_tab = Some(tab);
            let out = h.frame_sized(&mut app, size, Default::default());
            assert!(!out.shapes.is_empty(), "{tab:?} at {size:?} drew nothing");
        }
        for (i, tab) in Tab::ALL.into_iter().enumerate() {
            app.compact_tab = None;
            h.frame_sized(&mut app, size, Default::default());
            tap(&h, &mut app, size, tab_centre(size, i));
            assert_eq!(app.compact_tab, Some(tab), "tapping {tab:?} at {size:?}");
            // Tapping the open tab again gives the stage the whole screen.
            tap(&h, &mut app, size, tab_centre(size, i));
            assert_eq!(app.compact_tab, None, "closing {tab:?} at {size:?}");
        }
    }

    // Every module's controls lay out alone in the Edit sheet.
    app.compact_tab = Some(Tab::Edit);
    for id in photo_app::InspectorSectionId::all() {
        app.compact_module = *id;
        h.frame_sized(&mut app, PORTRAIT, Default::default());
    }
    assert_eq!(app.error(), None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a touch screen has instead of keys: back closes the sheet before it leaves, select
/// mode replaces Ctrl-click, and a swipe across the photo steps to the next one.
#[test]
fn back_select_mode_and_swipe_work_without_a_keyboard() {
    let dir = scratch_dir("touch");
    let (a, b) = (dir.join("a.dng"), dir.join("b.dng"));
    write_dng(&a, 640, 426);
    write_dng(&b, 640, 426);

    let h = Headless::new();
    let platform = FakePlatform::default();
    let mut app = PhotoApp::with_platform(&h.gpu(), Box::new(platform.clone()));
    h.frame_sized(&mut app, PORTRAIT, Default::default());
    app.add_to_shelf(&h.gpu(), vec![a.clone(), b.clone()]);
    h.settle(&mut app, Duration::from_secs(30));
    assert_eq!(app.loaded_path(), Some(a.as_path()));

    // Back: select mode first, then the sheet, and only then the app.
    app.select_mode = true;
    app.compact_tab = Some(Tab::Library);
    h.frame_sized(&mut app, PORTRAIT, key(egui::Key::BrowserBack));
    assert!(!app.select_mode);
    h.frame_sized(&mut app, PORTRAIT, key(egui::Key::BrowserBack));
    assert_eq!(app.compact_tab, None);
    assert_eq!(platform.0.lock().unwrap().leaves, 0);
    h.frame_sized(&mut app, PORTRAIT, key(egui::Key::BrowserBack));
    assert_eq!(platform.0.lock().unwrap().leaves, 1);

    // A swipe to the left across the photo, with the whole screen to the stage.
    let (from, to) = (egui::pos2(330.0, 450.0), egui::pos2(90.0, 460.0));
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    h.frame_sized(
        &mut app,
        PORTRAIT,
        pointer(vec![egui::Event::PointerMoved(from)]),
    );
    h.frame_sized(&mut app, PORTRAIT, pointer(vec![button(from, true)]));
    for i in 1..=8 {
        let p = from + (to - from) * (i as f32 / 8.0);
        h.frame_sized(
            &mut app,
            PORTRAIT,
            pointer(vec![egui::Event::PointerMoved(p)]),
        );
    }
    h.frame_sized(&mut app, PORTRAIT, pointer(vec![button(to, false)]));
    h.frame_sized(&mut app, PORTRAIT, Default::default());
    h.settle(&mut app, Duration::from_secs(30));
    assert_eq!(
        app.loaded_path(),
        Some(b.as_path()),
        "the swipe did not step"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
