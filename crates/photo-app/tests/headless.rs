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
    let row = sheet_top + 8.0 + 24.0;
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
        egui::pos2(48.0, sheet_top + 8.0 + 24.0),
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
