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

/// The app with no platform: tests must not write settings or a session marker into the
/// real user's configuration.
fn quiet_app(h: &Headless) -> PhotoApp {
    PhotoApp::with_platform(&h.gpu(), Box::new(photo_app::platform::NoPlatform))
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
    let mut app = quiet_app(&h);
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

    // A file that is not a RAW fails in place, naming the file once. The controls went to
    // it when it was chosen, and stay there with nothing to act on; nothing is written.
    let broken = dir.join("c.dng");
    std::fs::write(&broken, b"II*\0not really a raw file").unwrap();
    app.open_path(&h.gpu(), broken.clone());
    h.settle(&mut app, Duration::from_secs(30));
    let error = app.error().expect("no error shown");
    assert_eq!(error.matches("c.dng").count(), 1, "{error}");
    assert_eq!(app.loaded_path(), Some(broken.as_path()));
    assert!(!app.is_photo_ready());
    app.edit(|s| s.exposure.params.ev = 1.0);
    app.flush();
    assert!(
        photo_engine::find_sidecar(&broken).is_none(),
        "a photo that never decoded is not written to"
    );
    // And the photo that did open is one step away.
    app.open_path(&h.gpu(), raw.clone());
    h.settle(&mut app, Duration::from_secs(30));
    assert_eq!(app.error(), None);
    assert!(app.is_photo_ready());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_large_roll_paints_only_the_tiles_in_view() {
    let dir = scratch_dir("roll");
    let h = Headless::new();
    let mut app = quiet_app(&h);
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

    app.compact_tab = Some(Tab::Library);
    platform.answer(Response::OpenImage(raw.clone()));
    h.settle(&mut app, Duration::from_secs(30));
    assert_eq!(app.error(), None);
    assert_eq!(app.loaded_path(), Some(raw.as_path()));
    // A photo opened from the picker is there to be edited: the phone goes to Edit.
    assert_eq!(app.compact_tab, Some(Tab::Edit));

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
    let mut app = quiet_app(&h);
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

/// The centre of tab `i` of Tab::ALL.len() in the bottom bar (portrait) or right rail (landscape).
fn tab_centre(size: egui::Vec2, i: usize) -> egui::Pos2 {
    let n = Tab::ALL.len() as f32;
    if size.x > size.y {
        let rail = 56.0;
        let top = 32.0; // under the top bar
        let cell = (size.y - top) / n;
        egui::pos2(size.x - rail / 2.0, top + cell * (i as f32 + 0.5))
    } else {
        let bar = 48.0;
        let cell = size.x / n;
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

/// Upright, the sheet is only as tall as what it holds, so the photo keeps most of the
/// screen; tapping the chosen module in the Edit strip folds its controls away, and
/// tapping it again brings them back.
#[test]
fn the_edit_sheet_stays_low_and_folds_to_its_strip() {
    let dir = scratch_dir("low_sheet");
    let a = dir.join("a.dng");
    write_dng(&a, 640, 426);

    let h = Headless::new();
    let mut app = PhotoApp::with_platform(&h.gpu(), Box::new(FakePlatform::default()));
    h.frame_sized(&mut app, PORTRAIT, Default::default());
    app.add_to_shelf(&h.gpu(), vec![a.clone()]);
    h.settle(&mut app, Duration::from_secs(30));
    assert_eq!(
        app.compact_tab,
        Some(Tab::Edit),
        "the first photo opens to Edit"
    );

    app.compact_module = photo_app::InspectorSectionId::Exposure;
    // The sheet takes its content's height a frame after the content changes.
    for _ in 0..3 {
        h.frame_sized(&mut app, PORTRAIT, Default::default());
    }
    let open = app.stage_size().y;
    assert!(
        open > PORTRAIT.y * 0.5,
        "the photo should keep over half the screen, got {open}"
    );

    app.compact_controls_open = false;
    for _ in 0..3 {
        h.frame_sized(&mut app, PORTRAIT, Default::default());
    }
    let folded = app.stage_size().y;
    assert!(
        folded > open + 100.0 && folded > PORTRAIT.y * 0.75,
        "folded to its strip, the sheet should give the photo the room back: {open} → {folded}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_touch_slider_tap_and_drag() {
    let ctx = egui::Context::default();
    photo_app::theme::apply(&ctx);
    photo_app::widgets::set_touch(&ctx, true);

    let mut value = 0.0f32;
    // Frame 1: render a rail from -2.0 to 2.0, width 200.0, default 0.0
    let input1 = egui::RawInput {
        time: Some(0.0),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(400.0, 300.0),
        )),
        ..Default::default()
    };
    let mut out1 = ctx.run_ui(input1, |ui| {
        photo_app::widgets::rail(ui, &mut value, -2.0..=2.0, 200.0, 0.0);
    });
    out1.textures_delta.clear();

    // Frame 2: tap at x=150 on the rail (right of centre x=100): a tap on touch is as
    // often a scroll being stopped as a value being chosen, so it leaves the value alone.
    let tap_pos = egui::pos2(150.0, 19.0);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    let input2 = egui::RawInput {
        time: Some(0.05),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(400.0, 300.0),
        )),
        events: vec![
            egui::Event::PointerMoved(tap_pos),
            button(tap_pos, true),
            button(tap_pos, false),
        ],
        ..Default::default()
    };
    let mut out2 = ctx.run_ui(input2, |ui| {
        photo_app::widgets::rail(ui, &mut value, -2.0..=2.0, 200.0, 0.0);
    });
    out2.textures_delta.clear();
    assert_eq!(
        value, 0.0,
        "a tap must not move a touch slider, got {value}"
    );

    // Double click to reset: after 1 second, two clicks within 0.15s
    let input_d1 = egui::RawInput {
        time: Some(2.0),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(400.0, 300.0),
        )),
        events: vec![
            egui::Event::PointerMoved(tap_pos),
            button(tap_pos, true),
            button(tap_pos, false),
        ],
        ..Default::default()
    };
    let mut out_d1 = ctx.run_ui(input_d1, |ui| {
        photo_app::widgets::rail(ui, &mut value, -2.0..=2.0, 200.0, 0.0);
    });
    out_d1.textures_delta.clear();

    let input_d2 = egui::RawInput {
        time: Some(2.1),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(400.0, 300.0),
        )),
        events: vec![button(tap_pos, true), button(tap_pos, false)],
        ..Default::default()
    };
    let mut out_d2 = ctx.run_ui(input_d2, |ui| {
        photo_app::widgets::rail(ui, &mut value, -2.0..=2.0, 200.0, 0.0);
    });
    out_d2.textures_delta.clear();
    assert_eq!(value, 0.0, "double-click should reset to default 0.0");

    // Vertical swipe across the slider (user trying to scroll drawer):
    // Start at (100, 19), swipe vertically down to (100, 70)
    let p_start = egui::pos2(100.0, 19.0);
    let mut out_v1 = ctx.run_ui(
        egui::RawInput {
            time: Some(3.0),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            events: vec![egui::Event::PointerMoved(p_start), button(p_start, true)],
            ..Default::default()
        },
        |ui| {
            photo_app::widgets::rail(ui, &mut value, -2.0..=2.0, 200.0, 0.0);
        },
    );
    out_v1.textures_delta.clear();

    let p_down = egui::pos2(100.0, 70.0);
    let mut out_v2 = ctx.run_ui(
        egui::RawInput {
            time: Some(3.05),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            events: vec![egui::Event::PointerMoved(p_down), button(p_down, false)],
            ..Default::default()
        },
        |ui| {
            photo_app::widgets::rail(ui, &mut value, -2.0..=2.0, 200.0, 0.0);
        },
    );
    out_v2.textures_delta.clear();
    assert_eq!(
        value, 0.0,
        "vertical scroll drag should NOT change slider value"
    );

    // Horizontal drag across the slider:
    // Start at (100, 19), drag horizontally to (180, 19)
    let mut out_h1 = ctx.run_ui(
        egui::RawInput {
            time: Some(4.0),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            events: vec![egui::Event::PointerMoved(p_start), button(p_start, true)],
            ..Default::default()
        },
        |ui| {
            photo_app::widgets::rail(ui, &mut value, -2.0..=2.0, 200.0, 0.0);
        },
    );
    out_h1.textures_delta.clear();

    let p_right = egui::pos2(180.0, 19.0);
    let mut out_h2 = ctx.run_ui(
        egui::RawInput {
            time: Some(4.05),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            events: vec![egui::Event::PointerMoved(p_right)],
            ..Default::default()
        },
        |ui| {
            photo_app::widgets::rail(ui, &mut value, -2.0..=2.0, 200.0, 0.0);
        },
    );
    out_h2.textures_delta.clear();
    assert!(
        value > 1.0,
        "horizontal drag should slide value, got {value}"
    );
    // Relative: 80 px of a 4-unit rail about 200 px wide is about +1.6 from 0, not the
    // absolute position under the finger (+1.6 too here, so check it moved by the drag).
    assert!(value < 2.0, "a drag moved it past its travel: {value}");

    let mut out_h3 = ctx.run_ui(
        egui::RawInput {
            time: Some(4.10),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            events: vec![button(p_right, false)],
            ..Default::default()
        },
        |ui| {
            photo_app::widgets::rail(ui, &mut value, -2.0..=2.0, 200.0, 0.0);
        },
    );
    out_h3.textures_delta.clear();
}

/// A sideways drag at `y` from `x0` to `x1`, as a finger's comes in over a few frames.
fn flick(h: &Headless, app: &mut PhotoApp, y: f32, x0: f32, x1: f32) {
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    let from = egui::pos2(x0, y);
    let to = egui::pos2(x1, y);
    h.frame_sized(
        app,
        PORTRAIT,
        pointer(vec![egui::Event::PointerMoved(from)]),
    );
    h.frame_sized(app, PORTRAIT, pointer(vec![button(from, true)]));
    for i in 1..=8 {
        let p = from + (to - from) * (i as f32 / 8.0);
        h.frame_sized(app, PORTRAIT, pointer(vec![egui::Event::PointerMoved(p)]));
    }
    h.frame_sized(app, PORTRAIT, pointer(vec![button(to, false)]));
    h.frame_sized(app, PORTRAIT, pointer(vec![egui::Event::PointerGone]));
}

/// Swiping moves between modules on the Edit sheet and between tabs on the nav bar; the
/// scope and filmic start hidden on a phone and draw flat once shown; a look from Presets
/// applies and "original" takes it off; the tour closes with back.
#[test]
fn phone_swipes_presets_and_tour() {
    use photo_app::InspectorSectionId as M;
    let dir = scratch_dir("swipes");
    let a = dir.join("a.dng");
    write_dng(&a, 640, 426);

    let h = Headless::new();
    let mut app = PhotoApp::with_platform(&h.gpu(), Box::new(FakePlatform::default()));
    h.frame_sized(&mut app, PORTRAIT, Default::default());
    app.add_to_shelf(&h.gpu(), vec![a.clone()]);
    h.settle(&mut app, Duration::from_secs(30));
    assert_eq!(app.compact_tab, Some(Tab::Edit));
    assert_eq!(
        app.tour_step(),
        None,
        "no settings directory, so no tour on its own"
    );

    // Hidden until asked for: the scope and filmic.
    let shown = |app: &PhotoApp, id| app.compact_sections.iter().any(|s| s.id == id && s.visible);
    assert!(!shown(&app, M::Scope) && !shown(&app, M::Filmic3D));
    assert!(shown(&app, M::Exposure) && shown(&app, M::Tonal));

    // A flick to the left on the exposure module's header (between its name and its
    // summary, where there is no control) moves on to the next module.
    app.compact_module = M::Exposure;
    for _ in 0..3 {
        h.frame_sized(&mut app, PORTRAIT, Default::default());
    }
    let sheet_top = app.sheet_rect().top();
    flick(&h, &mut app, sheet_top + 22.0, 230.0, 90.0);
    assert_eq!(
        app.compact_module,
        M::Tonal,
        "a flick left is the next module"
    );
    for _ in 0..3 {
        h.frame_sized(&mut app, PORTRAIT, Default::default());
    }
    let sheet_top = app.sheet_rect().top();
    flick(&h, &mut app, sheet_top + 22.0, 120.0, 300.0);
    assert_eq!(app.compact_module, M::Exposure, "a flick right goes back");

    // Along the nav bar, a flick left moves one tab on.
    flick(&h, &mut app, PORTRAIT.y - 24.0, 330.0, 150.0);
    assert_eq!(app.compact_tab, Some(Tab::Export));
    flick(&h, &mut app, PORTRAIT.y - 24.0, 100.0, 300.0);
    assert_eq!(app.compact_tab, Some(Tab::Edit));

    // Shown, the scope and filmic lay out (flat) on the phone.
    for s in &mut app.compact_sections {
        s.visible = true;
    }
    for id in [M::Scope, M::Filmic3D] {
        app.compact_module = id;
        h.frame_sized(&mut app, PORTRAIT, Default::default());
        assert_eq!(app.compact_module, id);
    }

    // Presets: the second tile is the first look ("vivid"); the first is "original".
    app.compact_tab = Some(Tab::Presets);
    for _ in 0..3 {
        h.frame_sized(&mut app, PORTRAIT, Default::default());
    }
    let sheet_top = app.sheet_rect().top();
    // Under the looks / snapshots / history chips.
    let row = sheet_top + 8.0 + 34.0 + 24.0;
    let before = app.stack().clone();
    tap(&h, &mut app, PORTRAIT, egui::pos2(12.0 + 78.0 + 36.0, row));
    assert!(app.stack().vibrance.enabled, "vivid turns vibrance on");
    assert!(app.stack().vibrance.params.vibrance > 0.3);
    assert_eq!(
        app.stack().exposure,
        before.exposure,
        "a look leaves exposure alone"
    );
    for _ in 0..3 {
        h.frame_sized(&mut app, PORTRAIT, Default::default());
    }
    let sheet_top = app.sheet_rect().top();
    tap(
        &h,
        &mut app,
        PORTRAIT,
        egui::pos2(48.0, sheet_top + 8.0 + 34.0 + 24.0),
    );
    assert_eq!(app.stack(), &before, "original takes the look back off");

    // The tour: shown on request, closed by back.
    app.start_tour();
    h.frame_sized(&mut app, PORTRAIT, Default::default());
    assert_eq!(app.tour_step(), Some(0));
    h.frame_sized(&mut app, PORTRAIT, key(egui::Key::BrowserBack));
    assert_eq!(app.tour_step(), None);

    assert_eq!(app.error(), None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// U-01: the controls move to a photo the moment it is chosen, showing its own saved
/// edits or defaults; the photo left behind is saved on the way out.
#[test]
fn sliders_reset_to_the_chosen_photo_before_it_decodes() {
    let dir = scratch_dir("u01");
    let (a, b, c) = (dir.join("a.dng"), dir.join("b.dng"), dir.join("c.dng"));
    for p in [&a, &b, &c] {
        write_dng(p, 320, 214);
    }
    // B was edited in an earlier session.
    let mut saved_b = photo_engine::PhotoStack::default();
    saved_b.exposure.params.ev = -0.5;
    photo_engine::save_sidecar(&b, &saved_b).unwrap();

    let h = Headless::new();
    let mut app = quiet_app(&h);
    h.frame(&mut app, Default::default());
    app.add_to_shelf(&h.gpu(), vec![a.clone(), b.clone(), c.clone()]);
    h.settle(&mut app, Duration::from_secs(30));
    assert_eq!(app.loaded_path(), Some(a.as_path()));
    app.edit(|s| s.exposure.params.ev = 1.0);

    // Choosing B: on this very call, before any frame has run B's decode, the controls
    // show B's saved value, and A's edit is on disk.
    app.open_path(&h.gpu(), b.clone());
    assert_eq!(app.loaded_path(), Some(b.as_path()));
    assert!(!app.is_photo_ready(), "B has not decoded yet");
    assert_eq!(app.stack().exposure.params.ev, -0.5);
    let a_disk = photo_engine::load_sidecar(&a)
        .unwrap()
        .expect("A saved on leaving");
    assert_eq!(a_disk.exposure.params.ev, 1.0);

    // C has no edits: everything reads its default at once.
    app.open_path(&h.gpu(), c.clone());
    assert_eq!(app.stack().exposure.params.ev, 0.0);
    assert!(
        photo_engine::find_sidecar(&c).is_none(),
        "passing through a photo writes nothing"
    );
    assert_eq!(
        photo_engine::load_sidecar(&b).unwrap().unwrap(),
        saved_b,
        "B, passed through without an edit, is untouched"
    );

    // An edit made while C decodes is C's, and is on screen once C lands.
    app.edit(|s| s.exposure.params.ev = 0.7);
    h.settle(&mut app, Duration::from_secs(30));
    assert!(app.is_photo_ready());
    assert_eq!(app.loaded_path(), Some(c.as_path()));
    assert_eq!(app.stack().exposure.params.ev, 0.7);
    app.flush();
    assert_eq!(
        photo_engine::load_sidecar(&c)
            .unwrap()
            .unwrap()
            .exposure
            .params
            .ev,
        0.7
    );
    assert_eq!(
        photo_engine::load_sidecar(&a)
            .unwrap()
            .unwrap()
            .exposure
            .params
            .ev,
        1.0,
        "A is not touched by edits made after leaving it"
    );

    // Back to A: its edit comes back with it.
    app.open_path(&h.gpu(), a.clone());
    assert_eq!(app.stack().exposure.params.ev, 1.0);
    h.settle(&mut app, Duration::from_secs(30));
    assert_eq!(app.error(), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn edits_are_saved_a_moment_after_they_stop() {
    let dir = scratch_dir("autosave");
    let a = dir.join("a.dng");
    write_dng(&a, 320, 214);
    let h = Headless::new();
    let mut app = quiet_app(&h);
    h.frame(&mut app, Default::default());
    app.add_to_shelf(&h.gpu(), vec![a.clone()]);
    h.settle(&mut app, Duration::from_secs(30));

    app.edit(|s| s.exposure.params.ev = 0.4);
    h.frame(&mut app, Default::default());
    assert!(
        photo_engine::find_sidecar(&a).is_none(),
        "not while still settling"
    );
    std::thread::sleep(Duration::from_millis(1100));
    h.frame(&mut app, Default::default());
    assert_eq!(
        photo_engine::load_sidecar(&a)
            .unwrap()
            .unwrap()
            .exposure
            .params
            .ev,
        0.4
    );

    // A reset is an edit too: it is saved, not left to come back from the old sidecar.
    app.reset_to_default();
    app.flush();
    assert_eq!(
        photo_engine::load_sidecar(&a)
            .unwrap()
            .unwrap()
            .exposure
            .params
            .ev,
        0.0
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rating_an_unopened_photo_writes_no_stack() {
    let dir = scratch_dir("rating");
    let (a, b) = (dir.join("a.dng"), dir.join("b.dng"));
    write_dng(&a, 320, 214);
    write_dng(&b, 320, 214);
    let h = Headless::new();
    let mut app = quiet_app(&h);
    h.frame(&mut app, Default::default());
    app.add_to_shelf(&h.gpu(), vec![a.clone(), b.clone()]);
    h.settle(&mut app, Duration::from_secs(30));

    app.set_rating_for_path(&b, 4);
    let data = photo_engine::load_sidecar_full(&b).unwrap().unwrap();
    assert_eq!(data.metadata.rating, 4);
    assert_eq!(
        data.stack, None,
        "a stack here would pin the photo's orientation before anything read it"
    );

    // A sidecar this build cannot read is shown as an error and never written over.
    let sidecar = photo_engine::sidecar_path(&a);
    std::fs::write(&sidecar, b"{ damaged").unwrap();
    app.open_path(&h.gpu(), b.clone());
    app.open_path(&h.gpu(), a.clone());
    assert!(app.error().is_some_and(|e| e.contains("a.dng")));
    app.edit(|s| s.exposure.params.ev = 1.0);
    app.set_rating_for_path(&a, 2);
    app.flush();
    assert_eq!(std::fs::read(&sidecar).unwrap(), b"{ damaged");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn paste_to_all_keeps_each_photos_framing_and_can_be_taken_back() {
    let dir = scratch_dir("paste");
    let (a, b) = (dir.join("a.dng"), dir.join("b.dng"));
    write_dng(&a, 320, 214);
    write_dng(&b, 320, 214);
    let h = Headless::new();
    let mut app = quiet_app(&h);
    h.frame(&mut app, Default::default());
    app.add_to_shelf(&h.gpu(), vec![a.clone(), b.clone()]);
    h.settle(&mut app, Duration::from_secs(30));

    app.edit(|s| {
        s.exposure.params.ev = 0.8;
        s.geometry.quarter_turns = 1;
        s.geometry.crop = photo_engine::Crop {
            x: 0.1,
            y: 0.1,
            width: 0.5,
            height: 0.5,
        };
    });
    app.copy_adjustments();
    app.paste_to_all_in_shelf();
    let pasted = photo_engine::load_sidecar(&b).unwrap().unwrap();
    assert_eq!(pasted.exposure.params.ev, 0.8, "the look travels");
    assert_eq!(pasted.geometry.quarter_turns, 0, "the rotation does not");
    assert!(pasted.geometry.crop.is_full_frame(), "nor the crop");

    assert!(app.can_undo_paste_to_all());
    app.undo_paste_to_all();
    assert!(
        photo_engine::find_sidecar(&b).is_none(),
        "B had no sidecar before the paste"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn undo_and_snapshots_stay_with_their_photo() {
    let dir = scratch_dir("perphoto");
    let (a, b) = (dir.join("a.dng"), dir.join("b.dng"));
    write_dng(&a, 320, 214);
    write_dng(&b, 320, 214);
    let h = Headless::new();
    let mut app = quiet_app(&h);
    h.frame(&mut app, Default::default());
    app.add_to_shelf(&h.gpu(), vec![a.clone(), b.clone()]);
    h.settle(&mut app, Duration::from_secs(30));

    app.edit(|s| s.exposure.params.ev = 0.5);
    app.take_snapshot("bright");
    app.open_path(&h.gpu(), b.clone());
    h.settle(&mut app, Duration::from_secs(30));
    assert!(app.snapshots.is_empty(), "A's snapshot is not offered on B");
    assert!(!app.undo(), "B has nothing to undo");

    // Back on A, its snapshot and its undo step are there again.
    app.open_path(&h.gpu(), a.clone());
    h.settle(&mut app, Duration::from_secs(30));
    assert_eq!(app.snapshots.len(), 1);
    assert_eq!(app.snapshots[0].0, "bright");
    assert!(app.undo(), "A's edit can still be undone");
    assert_eq!(app.stack().exposure.params.ev, 0.0);

    // And the snapshot is in A's sidecar, for the next session.
    let data = photo_engine::load_sidecar_full(&a).unwrap().unwrap();
    assert_eq!(data.snapshots.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_drop_of_several_files_shelves_them_all_and_stepping_follows_the_filter() {
    let dir = scratch_dir("drop");
    let paths: Vec<PathBuf> = ["a.dng", "b.dng", "c.dng"]
        .iter()
        .map(|n| dir.join(n))
        .collect();
    for p in &paths {
        write_dng(p, 320, 214);
    }
    let h = Headless::new();
    let mut app = quiet_app(&h);
    h.frame(&mut app, Default::default());
    for p in &paths {
        app.drop_file(p.clone());
    }
    h.settle(&mut app, Duration::from_secs(30));
    assert_eq!(app.shelf.len(), 3, "every dropped file is shelved");
    assert_eq!(app.loaded_path(), Some(paths[0].as_path()));

    // Picks only: a and c. → from a goes to c, skipping the hidden b.
    app.set_flag_for_path(&paths[0], photo_engine::Flag::Pick);
    app.set_flag_for_path(&paths[2], photo_engine::Flag::Pick);
    app.filter.flag = photo_app::dam::FlagFilter::PicksOnly;
    h.frame(&mut app, key(egui::Key::ArrowRight));
    assert_eq!(app.loaded_path(), Some(paths[2].as_path()));
    h.settle(&mut app, Duration::from_secs(30));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn escape_mid_drag_puts_the_stack_back() {
    let dir = scratch_dir("escdrag");
    let a = dir.join("a.dng");
    write_dng(&a, 320, 214);
    let h = Headless::new();
    let mut app = quiet_app(&h);
    h.frame(&mut app, Default::default());
    app.add_to_shelf(&h.gpu(), vec![a.clone()]);
    h.settle(&mut app, Duration::from_secs(30));
    let before = app.stack().clone();

    // A real drag, held on the stage; while it is held the value moves, as a slider's does
    // frame by frame.
    let at = egui::pos2(700.0, 450.0);
    let press = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    h.frame(
        &mut app,
        pointer(vec![egui::Event::PointerMoved(at), press(at, true)]),
    );
    for dx in [10.0, 20.0, 30.0] {
        h.frame(
            &mut app,
            pointer(vec![egui::Event::PointerMoved(at + egui::vec2(dx, 0.0))]),
        );
    }
    assert!(h.ctx.dragged_id().is_some(), "the drag is held");
    app.edit(|s| s.exposure.params.ev = 1.3);
    let mut esc = key(egui::Key::Escape);
    esc.events
        .insert(0, egui::Event::PointerMoved(at + egui::vec2(40.0, 0.0)));
    h.frame(&mut app, esc);
    assert_eq!(
        app.stack(),
        &before,
        "Esc restored the stack from before the drag"
    );
    assert!(!app.undo(), "and the drag's undo step went with it");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn removing_from_the_shelf_can_be_taken_back() {
    let dir = scratch_dir("remove");
    let (a, b, c) = (dir.join("a.dng"), dir.join("b.dng"), dir.join("c.dng"));
    for p in [&a, &b, &c] {
        write_dng(p, 320, 214);
    }
    let h = Headless::new();
    let mut app = quiet_app(&h);
    h.frame(&mut app, Default::default());
    app.add_to_shelf(&h.gpu(), vec![a.clone(), b.clone(), c.clone()]);
    h.settle(&mut app, Duration::from_secs(30));
    app.selection.select_all(&[a.clone(), b.clone(), c.clone()]);

    app.remove_from_shelf(std::slice::from_ref(&b));
    assert_eq!(app.shelf.len(), 2);
    assert!(
        !app.selection.is_selected(&b),
        "a removed photo leaves the selection"
    );
    assert!(b.exists(), "the file itself is never touched");

    app.undo_remove_from_shelf();
    let order: Vec<_> = app.shelf.iter().map(|it| it.path.clone()).collect();
    assert_eq!(order, vec![a, b, c], "back where it was");
    h.settle(&mut app, Duration::from_secs(30));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A platform with somewhere to keep settings, and nothing else.
struct RememberingPlatform(PathBuf);

impl Platform for RememberingPlatform {
    fn request(&mut self, _request: Request) {}
    fn poll(&mut self) -> Vec<Response> {
        Vec::new()
    }
    fn settings_dir(&self) -> Option<PathBuf> {
        Some(self.0.clone())
    }
}

#[test]
fn the_shelf_and_the_open_photo_come_back_on_the_next_launch() {
    let dir = scratch_dir("session");
    let settings = dir.join("settings");
    let (a, b) = (dir.join("a.dng"), dir.join("b.dng"));
    write_dng(&a, 320, 214);
    write_dng(&b, 320, 214);
    let h = Headless::new();
    {
        let mut app =
            PhotoApp::with_platform(&h.gpu(), Box::new(RememberingPlatform(settings.clone())));
        h.frame(&mut app, Default::default());
        app.add_to_shelf(&h.gpu(), vec![a.clone(), b.clone()]);
        h.settle(&mut app, Duration::from_secs(30));
        app.open_path(&h.gpu(), b.clone());
        h.settle(&mut app, Duration::from_secs(30));
        app.set_ui_scale(1.15);
        app.shutdown();
    }
    let mut app = PhotoApp::with_platform(&h.gpu(), Box::new(RememberingPlatform(settings)));
    h.settle(&mut app, Duration::from_secs(30));
    let shelf: Vec<_> = app.shelf.iter().map(|it| it.path.clone()).collect();
    assert_eq!(shelf, vec![a, b.clone()]);
    assert_eq!(
        app.loaded_path(),
        Some(b.as_path()),
        "the photo left open is open"
    );
    assert!(
        (h.ctx.zoom_factor() - 1.15).abs() < 1e-3,
        "the text size came back"
    );
    assert_eq!(
        app.error(),
        None,
        "a clean shutdown leaves no unclean-exit notice"
    );
    h.ctx.set_zoom_factor(1.0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn question_mark_shows_the_shortcuts_and_keys_wait_behind_it() {
    let h = Headless::new();
    let mut app = quiet_app(&h);
    h.frame(&mut app, Default::default());
    let text = |t: &str| egui::RawInput {
        events: vec![egui::Event::Text(t.into())],
        ..Default::default()
    };
    h.frame(&mut app, text("?"));
    assert!(app.shortcuts_open);
    // Behind the sheet a shortcut does nothing but the sheet's own keys.
    let split = app.split_active;
    h.frame(&mut app, key(egui::Key::Backslash));
    assert_eq!(app.split_active, split);
    assert!(app.shortcuts_open);
    h.frame(&mut app, key(egui::Key::Escape));
    assert!(!app.shortcuts_open);
    h.frame(&mut app, key(egui::Key::F1));
    assert!(app.shortcuts_open);
    h.frame(&mut app, text("?"));
    assert!(!app.shortcuts_open);
}

#[test]
fn same_as_previous_carries_the_last_photos_look_but_not_its_framing() {
    let dir = scratch_dir("previous");
    let (a, b) = (dir.join("a.dng"), dir.join("b.dng"));
    write_dng(&a, 320, 214);
    write_dng(&b, 320, 214);
    let h = Headless::new();
    let mut app = quiet_app(&h);
    h.frame(&mut app, Default::default());
    app.add_to_shelf(&h.gpu(), vec![a.clone(), b.clone()]);
    h.settle(&mut app, Duration::from_secs(30));
    assert!(
        !app.can_same_as_previous(),
        "the first photo has no previous"
    );

    app.edit(|s| {
        s.exposure.params.ev = 0.7;
        s.geometry.rotate_right();
    });
    app.open_path(&h.gpu(), b.clone());
    h.settle(&mut app, Duration::from_secs(30));
    let framing = app.stack().geometry;
    app.same_as_previous();
    assert_eq!(app.stack().exposure.params.ev, 0.7);
    assert_eq!(app.stack().geometry, framing, "B keeps its own rotation");
    assert!(app.undo(), "one undo step");
    assert_eq!(app.stack().exposure.params.ev, 0.0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_deleted_snapshot_comes_back_from_the_notice() {
    let dir = scratch_dir("snapdelete");
    let a = dir.join("a.dng");
    write_dng(&a, 320, 214);
    let h = Headless::new();
    let mut app = quiet_app(&h);
    h.frame(&mut app, Default::default());
    app.add_to_shelf(&h.gpu(), vec![a.clone()]);
    h.settle(&mut app, Duration::from_secs(30));
    app.take_snapshot("one");
    app.edit(|s| s.exposure.params.ev = 0.3);
    app.take_snapshot("two");
    app.delete_snapshot(0);
    assert_eq!(app.snapshots.len(), 1);
    assert_eq!(
        photo_engine::load_sidecar_full(&a)
            .unwrap()
            .unwrap()
            .snapshots
            .len(),
        1
    );
    app.undo_delete_snapshot();
    let names: Vec<&str> = app.snapshots.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["one", "two"], "back where it was");
    assert_eq!(
        photo_engine::load_sidecar_full(&a)
            .unwrap()
            .unwrap()
            .snapshots
            .len(),
        2
    );
    app.recall_snapshot(0);
    assert_eq!(app.stack().exposure.params.ev, 0.0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_photo_stays_on_the_shelf_and_the_filter_comes_back() {
    let dir = scratch_dir("missing");
    let settings = dir.join("settings");
    let (a, b) = (dir.join("a.dng"), dir.join("b.dng"));
    write_dng(&a, 320, 214);
    write_dng(&b, 320, 214);
    let h = Headless::new();
    {
        let mut app =
            PhotoApp::with_platform(&h.gpu(), Box::new(RememberingPlatform(settings.clone())));
        h.frame(&mut app, Default::default());
        app.add_to_shelf(&h.gpu(), vec![a.clone(), b.clone()]);
        h.settle(&mut app, Duration::from_secs(30));
        app.filter.sort_descending = true;
        app.shutdown();
    }
    // The drive with b on it is unplugged.
    std::fs::remove_file(&b).unwrap();
    let mut app = PhotoApp::with_platform(&h.gpu(), Box::new(RememberingPlatform(settings)));
    h.settle(&mut app, Duration::from_secs(30));
    assert_eq!(app.shelf.len(), 2, "b is still shelved");
    let missing: Vec<bool> = app.shelf.iter().map(|it| it.missing).collect();
    assert_eq!(missing, [false, true]);
    assert!(app.filter.sort_descending, "the sort came back");
    // Opening it says why instead of failing a decode; stepping goes past it.
    app.open_path(&h.gpu(), b.clone());
    assert_eq!(app.loaded_path(), Some(a.as_path()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_screen_reader_is_told_what_each_control_is() {
    let dir = scratch_dir("accesskit");
    let a = dir.join("a.dng");
    write_dng(&a, 320, 214);
    let h = Headless::new();
    let mut app = quiet_app(&h);
    h.frame(&mut app, Default::default());
    app.add_to_shelf(&h.gpu(), vec![a.clone()]);
    h.settle(&mut app, Duration::from_secs(30));
    h.ctx.enable_accesskit();
    h.frame(&mut app, Default::default());
    let output = h.frame(&mut app, Default::default());
    let update = output
        .platform_output
        .accesskit_update
        .expect("a tree once a reader asked");
    let labels: Vec<(String, Option<f64>)> = update
        .nodes
        .iter()
        .filter_map(|(_, n)| n.label().map(|l| (l.to_string(), n.numeric_value())))
        .collect();
    let has = |want: &str| labels.iter().any(|(l, _)| l == want);
    for want in ["Export", "Open RAW…", "a.dng", "Shortcuts (? or F1)"] {
        assert!(has(want), "no {want:?} among {labels:?}");
    }
    // A slider says its name and its value, not its position along the rail.
    let temp = labels
        .iter()
        .find(|(l, _)| l == "temp (K)")
        .expect("the WB slider");
    assert!(temp.1.is_some_and(|v| v > 1000.0), "{temp:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ctrl_e_opens_the_export_settings_and_they_are_remembered() {
    let dir = scratch_dir("exportsettings");
    let settings = dir.join("settings");
    let a = dir.join("a.dng");
    write_dng(&a, 320, 214);
    let h = Headless::new();
    let ctrl_e = egui::RawInput {
        events: vec![
            egui::Event::ModifiersChanged(egui::Modifiers::COMMAND),
            egui::Event::Key {
                key: egui::Key::E,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            },
        ],
        ..Default::default()
    };
    {
        let mut app =
            PhotoApp::with_platform(&h.gpu(), Box::new(RememberingPlatform(settings.clone())));
        h.frame(&mut app, Default::default());
        app.add_to_shelf(&h.gpu(), vec![a.clone()]);
        h.settle(&mut app, Duration::from_secs(30));
        h.frame(&mut app, ctrl_e);
        assert!(app.export_dialog_open);
        h.frame(
            &mut app,
            pointer(vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE)]),
        );
        h.frame(&mut app, key(egui::Key::Escape));
        assert!(!app.export_dialog_open);
        app.export_settings.format = photo_engine::ExportFormat::Tiff;
        app.export_settings.depth = photo_engine::BitDepth::Sixteen;
        app.export_settings.sizing = photo_engine::Resize::LongEdge(3000);
        app.shutdown();
    }
    let app = PhotoApp::with_platform(&h.gpu(), Box::new(RememberingPlatform(settings)));
    assert_eq!(app.export_settings.format, photo_engine::ExportFormat::Tiff);
    assert_eq!(app.export_settings.depth, photo_engine::BitDepth::Sixteen);
    assert_eq!(
        app.export_settings.sizing,
        photo_engine::Resize::LongEdge(3000)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn paste_carries_only_the_chosen_modules_and_the_choice_is_remembered() {
    let dir = scratch_dir("selectivepaste");
    let settings = dir.join("settings");
    let (a, b) = (dir.join("a.dng"), dir.join("b.dng"));
    write_dng(&a, 320, 214);
    write_dng(&b, 320, 214);
    let h = Headless::new();
    let ctrl_shift_v = egui::RawInput {
        events: vec![
            egui::Event::ModifiersChanged(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT),
            egui::Event::Key {
                key: egui::Key::V,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            },
        ],
        ..Default::default()
    };
    {
        let mut app =
            PhotoApp::with_platform(&h.gpu(), Box::new(RememberingPlatform(settings.clone())));
        h.frame(&mut app, Default::default());
        app.add_to_shelf(&h.gpu(), vec![a.clone(), b.clone()]);
        h.settle(&mut app, Duration::from_secs(30));
        h.frame(&mut app, ctrl_shift_v);
        assert!(app.paste_picker_open);
        h.frame(
            &mut app,
            pointer(vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE)]),
        );
        // Behind the picker, keys wait; Esc closes it.
        h.frame(&mut app, key(egui::Key::ArrowRight));
        assert_eq!(app.loaded_path(), Some(a.as_path()));
        h.frame(&mut app, key(egui::Key::Escape));
        assert!(!app.paste_picker_open);

        app.edit(|s| {
            s.exposure.params.ev = 0.8;
            s.vignette.enabled = true;
            s.vignette.params.amount = -0.5;
            s.geometry.rotate_right();
        });
        app.copy_adjustments();
        app.paste_set
            .set(photo_app::InspectorSectionId::Vignette, false);
        app.paste_set
            .set(photo_app::InspectorSectionId::Geometry, true);
        app.open_path(&h.gpu(), b.clone());
        h.settle(&mut app, Duration::from_secs(30));
        app.paste_adjustments();
        assert_eq!(app.stack().exposure.params.ev, 0.8);
        assert!(!app.stack().vignette.enabled, "the vignette was left out");
        assert_eq!(
            app.stack().geometry.quarter_turns,
            1,
            "the framing was asked for"
        );
        app.shutdown();
    }
    let app = PhotoApp::with_platform(&h.gpu(), Box::new(RememberingPlatform(settings)));
    assert!(!app
        .paste_set
        .carries(photo_app::InspectorSectionId::Vignette));
    assert!(app
        .paste_set
        .carries(photo_app::InspectorSectionId::Geometry));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_saved_preset_applies_elsewhere_survives_a_restart_and_can_go() {
    let dir = scratch_dir("userpresets");
    let settings = dir.join("settings");
    let (a, b) = (dir.join("a.dng"), dir.join("b.dng"));
    write_dng(&a, 320, 214);
    write_dng(&b, 320, 214);
    let h = Headless::new();
    {
        let mut app =
            PhotoApp::with_platform(&h.gpu(), Box::new(RememberingPlatform(settings.clone())));
        h.frame(&mut app, Default::default());
        app.add_to_shelf(&h.gpu(), vec![a.clone(), b.clone()]);
        h.settle(&mut app, Duration::from_secs(30));
        app.edit(|s| {
            s.vibrance.enabled = true;
            s.vibrance.params.vibrance = 0.3;
            s.exposure.params.ev = 1.2;
        });
        app.paste_set
            .set(photo_app::InspectorSectionId::Exposure, false);
        app.save_user_preset("Soft colour");
        assert_eq!(app.user_presets.len(), 1);
        app.open_path(&h.gpu(), b.clone());
        h.settle(&mut app, Duration::from_secs(30));
        app.apply_user_preset(0);
        assert!(app.stack().vibrance.enabled);
        assert_eq!(
            app.stack().exposure.params.ev,
            0.0,
            "exposure was not carried"
        );
        assert!(app.undo(), "one undo step");
        assert!(!app.stack().vibrance.enabled);
        app.shutdown();
    }
    let mut app =
        PhotoApp::with_platform(&h.gpu(), Box::new(RememberingPlatform(settings.clone())));
    h.frame(&mut app, Default::default());
    assert_eq!(app.user_presets.len(), 1);
    assert_eq!(app.user_presets[0].1.name, "Soft colour");
    // A file from elsewhere comes in under its own name.
    let file = dir.join("shared.duckfoot-preset");
    std::fs::write(
        &file,
        photo_app::user_presets::UserPreset::new(
            "Shared",
            Default::default(),
            &photo_engine::PhotoStack::default(),
        )
        .to_json(),
    )
    .unwrap();
    app.import_preset(&file);
    let names: Vec<String> = app
        .user_presets
        .iter()
        .map(|(_, p)| p.name.clone())
        .collect();
    assert_eq!(names, ["Shared", "Soft colour"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_fixed_aspect_holds_through_handles_and_a_drag_outside_straightens() {
    use photo_app::crop_tool::Aspect;
    let dir = scratch_dir("cropaspect");
    let raw = dir.join("a.dng");
    write_dng(&raw, 640, 426);
    let h = Headless::new();
    let mut app = quiet_app(&h);
    h.frame(&mut app, Default::default());
    app.add_to_shelf(&h.gpu(), vec![raw.clone()]);
    h.settle(&mut app, Duration::from_secs(30));

    h.frame(&mut app, key(egui::Key::C));
    app.set_crop_aspect(Aspect::Ratio(1, 1));
    h.frame(&mut app, Default::default());
    let frame = app.stage_frame().expect("the stage drew the frame");
    let square = |c: photo_engine::Crop| (c.width * 640.0) / (c.height * 426.0);
    let c = app.stack().geometry.crop;
    assert!((square(c) - 1.0).abs() < 0.01, "{c:?}");

    // The bottom-right corner, dragged up and in: still square, top-left where it was.
    let corner = frame.min
        + egui::vec2(
            (c.x + c.width) * frame.width(),
            (c.y + c.height) * frame.height(),
        );
    drag(&h, &mut app, corner, corner - egui::vec2(60.0, 20.0));
    let resized = app.stack().geometry.crop;
    assert!((square(resized) - 1.0).abs() < 0.02, "{resized:?}");
    assert!(resized.width < c.width);
    assert!((resized.x - c.x).abs() < 1e-3 && (resized.y - c.y).abs() < 1e-3);

    // X turns the shape; a square turned is a square.
    h.frame(&mut app, key(egui::Key::X));
    assert!((square(app.stack().geometry.crop) - 1.0).abs() < 0.02);
    app.set_crop_aspect(Aspect::Ratio(3, 2));
    h.frame(&mut app, key(egui::Key::X));
    assert_eq!(app.crop_aspect, Aspect::Ratio(2, 3));

    // Outside the crop, a drag turns the photo, as its own undo step.
    let left = egui::pos2(frame.left() + 6.0, frame.center().y);
    drag(&h, &mut app, left, left + egui::vec2(0.0, 60.0));
    let turned = app.stack().geometry.straighten_deg;
    assert!(turned.abs() > 1.0, "straighten {turned}");
    h.frame(&mut app, key(egui::Key::Enter));
    assert!(!app.is_cropping());
    assert!(app.undo(), "the crop session");
    assert!(app.undo(), "the turn");
    assert_eq!(app.stack().geometry.straighten_deg, 0.0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn w_then_a_click_on_the_photo_takes_one_white_balance_pick() {
    let dir = scratch_dir("eyedropper");
    let raw = dir.join("a.dng");
    write_dng(&raw, 320, 214);
    let h = Headless::new();
    let mut app = quiet_app(&h);
    h.frame(&mut app, Default::default());
    app.add_to_shelf(&h.gpu(), vec![raw.clone()]);
    h.settle(&mut app, Duration::from_secs(30));
    h.frame(&mut app, Default::default());
    let before = app.stack().white_balance.clone();

    h.frame(&mut app, key(egui::Key::W));
    assert!(app.picking_wb);
    // A quarter across: the ramp's middle has red near its clip once balanced, which the
    // dropper rightly refuses.
    let frame = app.stage_frame().expect("the stage drew the frame");
    let at = egui::pos2(frame.left() + frame.width() * 0.25, frame.center().y);
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    h.frame(
        &mut app,
        pointer(vec![egui::Event::PointerMoved(at), button(true)]),
    );
    h.frame(&mut app, pointer(vec![button(false)]));
    h.frame(&mut app, Default::default());
    assert!(!app.picking_wb, "one pick, then the dropper is put down");
    // The headless device has no GPU behind it, so the patch reads back empty and the pick
    // is refused (the GPU suite checks the balance it finds); the click was taken as a pick.
    let said = app.status_notification.as_ref().map(|(m, _)| m.as_str());
    assert!(matches!(said, Some(m) if m.contains("balance")), "{said:?}");
    assert_eq!(
        app.stack().white_balance,
        before,
        "a refused pick changes nothing"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every pair of widgets drawn over one another on the page: egui's own record of the
/// last frame, each widget's hit area clipped to what is visible. Floating layers (windows,
/// menus, tooltips, toasts) sit over the page by design and are left out, and a widget
/// wholly inside another (a button in its panel) is containment, not overlap.
fn overlapping_controls(h: &Headless, app: &mut PhotoApp, size: egui::Vec2) -> Vec<String> {
    overlaps_in(h, app, size, false)
}

/// [`overlapping_controls`]; with `same_layer`, only widgets on one layer are compared, so
/// an open dialog is checked against itself and not against the page it floats over.
fn overlaps_in(
    h: &Headless,
    app: &mut PhotoApp,
    size: egui::Vec2,
    same_layer: bool,
) -> Vec<String> {
    h.ctx.enable_accesskit();
    // Settled: the phone layout places its parts by where they were the frame before,
    // and its sheet eases to its height.
    for _ in 0..12 {
        h.frame_sized(app, size, Default::default());
    }
    let output = h.frame_sized(app, size, Default::default());
    // Names from the accessibility tree, for a readable report.
    let names: std::collections::HashMap<u64, String> = output
        .platform_output
        .accesskit_update
        .map(|u| {
            u.nodes
                .iter()
                .filter_map(|(id, n)| n.label().map(|l| (id.0, l.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let name = |id: egui::Id| {
        names
            .get(&id.value())
            .cloned()
            .unwrap_or_else(|| format!("{id:?}"))
    };
    // What is drawn, not where a finger may land: `widgets::allocate_control` gives a
    // touch control a hit area (its id `.with("hit")`) grown past what it paints, which
    // may reach under a neighbour, as touch targets are meant to; egui gives the tap to
    // the nearest. Those are left out and the painted rectangle compared instead.
    let widgets: Vec<(egui::Id, egui::Rect, egui::LayerId)> = h.ctx.viewport(|v| {
        let all: Vec<&egui::WidgetRect> = v
            .prev_pass
            .widgets
            .layers()
            .filter(|(layer, _)| {
                matches!(layer.order, egui::Order::Background | egui::Order::Middle)
            })
            .flat_map(|(_, ws)| ws.iter())
            .collect();
        let hits: std::collections::HashSet<egui::Id> =
            all.iter().map(|w| w.id.with("hit")).collect();
        all.into_iter()
            // Resize grips (the panels', the phone's side sheet's) are thin strips laid
            // along an edge, over what they resize, on purpose.
            .filter(|w| w.interact_rect.width().min(w.interact_rect.height()) > 12.0)
            .filter(|w| w.interact_rect.is_positive() && !hits.contains(&w.id))
            .map(|w| (w.id, w.interact_rect, w.layer_id))
            .collect()
    });
    let mut out = Vec::new();
    for (i, (ida, a, la)) in widgets.iter().enumerate() {
        for (idb, b, lb) in &widgets[i + 1..] {
            if same_layer && la != lb {
                continue;
            }
            let o = a.intersect(*b);
            // Within a point: a frame and the scroll area inside it can differ by the
            // hairline between them.
            let contains =
                |outer: &egui::Rect, inner: &egui::Rect| outer.expand(1.5).contains_rect(*inner);
            let contained = contains(a, b) || contains(b, a);
            // A resizable window's corner grips: small squares centred on its corners.
            let corner_grip = |grip: &egui::Rect, of: &egui::Rect| {
                grip.width() <= 24.0
                    && grip.height() <= 24.0
                    && [
                        of.left_top(),
                        of.right_top(),
                        of.left_bottom(),
                        of.right_bottom(),
                    ]
                    .iter()
                    .any(|c| (grip.center() - *c).length() < 2.0)
            };
            let contained = contained || corner_grip(a, b) || corner_grip(b, a);
            if o.width() > 1.5 && o.height() > 1.5 && !contained {
                out.push(format!(
                    "{:?} {a:?} overlaps {:?} {b:?}",
                    name(*ida),
                    name(*idb)
                ));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn no_controls_overlap_on_any_screen() {
    let dir = scratch_dir("overlap");
    let raw = dir.join("a.dng");
    write_dng(&raw, 640, 426);
    let h = Headless::new();
    let mut app = quiet_app(&h);
    h.frame(&mut app, Default::default());
    app.add_to_shelf(&h.gpu(), vec![raw.clone()]);
    h.settle(&mut app, Duration::from_secs(30));
    let mut found = Vec::new();
    let mut check = |app: &mut PhotoApp, what: &str, size: egui::Vec2| {
        for o in overlapping_controls(&h, app, size) {
            found.push(format!("{what} {size:?}: {o}"));
        }
    };
    let desktops = [
        egui::vec2(1440.0, 900.0),
        egui::vec2(1280.0, 720.0),
        egui::vec2(1024.0, 768.0),
    ];
    for size in desktops {
        check(&mut app, "desktop", size);
    }
    app.looks_open = true;
    app.info_open = true;
    for size in desktops {
        check(&mut app, "looks + info", size);
    }
    app.preset_draft = Some("Evening".into());
    for size in desktops {
        check(&mut app, "looks + naming a preset", size);
    }
    app.preset_draft = None;
    app.looks_open = false;
    app.info_open = false;
    // Every card open at once, scrolled through by the detector's clip.
    let layout = app.inspector_sections.clone();
    for s in &mut app.inspector_sections {
        s.collapsed = false;
        s.visible = true;
    }
    for size in desktops {
        check(&mut app, "every card open", size);
    }
    app.inspector_sections = layout;
    app.begin_crop();
    for size in desktops {
        check(&mut app, "cropping", size);
    }
    h.frame(&mut app, key(egui::Key::Escape));
    // The phone, upright and on its side, with each sheet open.
    for tab in Tab::ALL {
        app.compact_tab = Some(tab);
        for size in [
            egui::vec2(412.0, 892.0),
            egui::vec2(892.0, 412.0),
            egui::vec2(360.0, 740.0),
        ] {
            check(&mut app, tab.label(), size);
        }
    }
    // The Presets sheet naming a preset.
    app.compact_tab = Some(Tab::Presets);
    app.preset_draft = Some("Evening".into());
    for size in [egui::vec2(412.0, 892.0), egui::vec2(360.0, 740.0)] {
        check(&mut app, "presets + naming", size);
    }
    app.preset_draft = None;
    // The Export sheet with the paste picker open.
    app.compact_tab = Some(Tab::Export);
    app.paste_picker_open = true;
    for size in [egui::vec2(412.0, 892.0), egui::vec2(360.0, 740.0)] {
        check(&mut app, "export + paste picker", size);
    }
    app.paste_picker_open = false;
    // The Library sheet with the photo's info card open.
    app.compact_tab = Some(Tab::Library);
    app.info_open = true;
    for size in [egui::vec2(412.0, 892.0), egui::vec2(360.0, 740.0)] {
        check(&mut app, "library + info", size);
    }
    app.info_open = false;
    // Each module on its own, on the narrowest phone.
    app.compact_tab = Some(Tab::Edit);
    for &id in photo_app::InspectorSectionId::all() {
        app.compact_module = id;
        check(&mut app, id.title(), egui::vec2(360.0, 740.0));
    }
    app.compact_tab = None;
    check(&mut app, "phone, no sheet", egui::vec2(412.0, 892.0));

    // Dialogs, each against itself.
    let mut in_dialog = |app: &mut PhotoApp, what: &str, size: egui::Vec2| {
        for o in overlaps_in(&h, app, size, true) {
            found.push(format!("{what} {size:?}: {o}"));
        }
    };
    app.export_dialog_open = true;
    for size in desktops {
        in_dialog(&mut app, "export dialog", size);
    }
    app.export_dialog_open = false;
    app.batch_dialog_open = true;
    for size in desktops {
        in_dialog(&mut app, "batch dialog", size);
    }
    app.batch_dialog_open = false;
    app.shortcuts_open = true;
    for size in desktops {
        in_dialog(&mut app, "shortcuts", size);
    }
    app.shortcuts_open = false;
    app.paste_picker_open = true;
    for size in desktops {
        in_dialog(&mut app, "paste picker", size);
    }
    app.paste_picker_open = false;
    for f in &found {
        eprintln!("{f}");
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(found.is_empty(), "{} overlaps", found.len());
}

#[test]
fn straightening_keeps_the_crop_on_the_photo_unless_told_not_to() {
    use photo_app::crop_tool::covered;
    let dir = scratch_dir("constrain");
    let raw = dir.join("a.dng");
    write_dng(&raw, 640, 426);
    let h = Headless::new();
    let mut app = quiet_app(&h);
    h.frame(&mut app, Default::default());
    app.add_to_shelf(&h.gpu(), vec![raw.clone()]);
    h.settle(&mut app, Duration::from_secs(30));

    app.edit(|s| s.geometry.straighten_deg = 8.0);
    h.frame(&mut app, Default::default());
    let c = app.stack().geometry.crop;
    assert!(!c.is_full_frame(), "the corners would show");
    assert!(covered(c, 8.0, (640.0, 426.0)), "{c:?}");
    // One edit: undo takes back the turn and the crop it caused together.
    assert!(app.undo());
    h.frame(&mut app, Default::default());
    assert!(app.stack().geometry.crop.is_full_frame());
    assert_eq!(app.stack().geometry.straighten_deg, 0.0);

    app.set_constrain_crop(false);
    app.edit(|s| s.geometry.straighten_deg = 8.0);
    h.frame(&mut app, Default::default());
    assert!(
        app.stack().geometry.crop.is_full_frame(),
        "left as it is when off"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
