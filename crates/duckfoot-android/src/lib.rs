//! Android bootstrap. The same shell, app and engine as the desktop; what differs is the
//! entry point, the logger, and where files come from.
//!
//! Storage model, for now: Android hands out `content://` documents rather than paths,
//! and the app keys everything — shelf, thumbnails, sidecars — on a path. So the Java
//! activity (`android/app/src/main/java/com/walker/duckfoot/DuckfootActivity.java`) *imports*
//! what the user picks into the app's private library directory and passes the copies'
//! paths here. Sidecars then work unchanged, beside the copies. Exports go straight to
//! the shared `Pictures/Duckfoot`, which apps may write to without a permission from
//! Android 11 on. See docs/ANDROID_AUDIT.md (B3) for the fuller model this stands in for.
//!
//! Threading: every call into Java happens on the event-loop thread, which is attached
//! to the VM once. The activity's own threads never call into Rust except
//! `nativeWake`, which only posts a winit user event.

#![cfg(target_os = "android")]

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use duckfoot::UserEvent;
use jni::objects::{GlobalRef, JObject, JObjectArray, JString, JValue};
use jni::{JNIEnv, JavaVM, NativeMethod};
use photo_app::dam::FolderBookmark;
use photo_app::platform::{Platform, Request, Response};
use winit::event_loop::EventLoopProxy;
use winit::platform::android::activity::AndroidApp;
use winit::platform::android::EventLoopBuilderExtAndroid;

/// Lets `nativeWake`, called on an activity thread, reach the event loop.
static PROXY: OnceLock<Mutex<EventLoopProxy<UserEvent>>> = OnceLock::new();

/// Codes shared with `DuckfootActivity.requestPick`.
const PICK_OPEN: i32 = 0;
const PICK_ADD: i32 = 1;
const PICK_FOLDER: i32 = 2;

#[no_mangle]
fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("duckfoot"),
    );
    // A panic's message otherwise goes to stderr, which Android discards.
    std::panic::set_hook(Box::new(|info| log::error!("panic: {info}")));

    if let Err(e) = run(app) {
        log::error!("duckfoot stopped: {e:#}");
    }
}

fn run(app: AndroidApp) -> anyhow::Result<()> {
    let event_loop = winit::event_loop::EventLoop::<UserEvent>::with_user_event()
        .with_android_app(app.clone())
        .build()?;
    let _ = PROXY.set(Mutex::new(event_loop.create_proxy()));

    let bridge = Bridge::new(&app)?;
    let library = bridge.dir("libraryDir")?;
    let pictures = bridge.dir("picturesDir")?;
    log::info!(
        "library {}, exports {}",
        library.display(),
        pictures.display()
    );

    duckfoot::run(
        event_loop,
        duckfoot::Options {
            pending_open: None,
            platform: Box::new(move || {
                Box::new(AndroidPlatform {
                    bridge: bridge.clone(),
                    library: library.clone(),
                    pictures: pictures.clone(),
                    answered: Vec::new(),
                })
            }),
        },
    )
}

/// `DuckfootActivity.nativeWake()`: an import finished, so draw a frame to pick it up.
extern "system" fn native_wake(_env: JNIEnv, _this: JObject) {
    if let Some(proxy) = PROXY.get() {
        if let Ok(proxy) = proxy.lock() {
            let _ = proxy.send_event(UserEvent::Wake);
        }
    }
}

/// The activity, and the VM to reach it through.
#[derive(Clone)]
struct Bridge {
    vm: std::sync::Arc<JavaVM>,
    activity: GlobalRef,
}

impl Bridge {
    fn new(app: &AndroidApp) -> anyhow::Result<Self> {
        // SAFETY: android-activity hands out the process's VM and the live activity; both
        // stay valid for as long as `android_main` runs.
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast())? };
        let activity = {
            let mut env = vm.attach_current_thread_permanently()?;
            let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
            let activity = env.new_global_ref(activity)?;

            // Registered here rather than resolved by name, because NativeActivity
            // dlopen()s this library itself and JNI would not search it.
            let class = env.get_object_class(activity.as_obj())?;
            env.register_native_methods(
                &class,
                &[NativeMethod {
                    name: "nativeWake".into(),
                    sig: "()V".into(),
                    fn_ptr: native_wake as *mut c_void,
                }],
            )?;

            // Tell the activity which files are photos, and that it may now wake us.
            let extensions: Vec<String> = photo_engine::all_image_dialog_extensions()
                .into_iter()
                .map(|e| e.to_lowercase())
                .collect();
            let array = string_array(&mut env, &extensions)?;
            env.call_method(
                activity.as_obj(),
                "onNativeReady",
                "([Ljava/lang/String;)V",
                &[JValue::Object(&array)],
            )?;
            activity
        };
        Ok(Self {
            vm: std::sync::Arc::new(vm),
            activity,
        })
    }

    fn env(&self) -> jni::errors::Result<JNIEnv<'_>> {
        self.vm.attach_current_thread_permanently()
    }

    /// A directory the activity reports through a `String name()` method.
    fn dir(&self, method: &str) -> anyhow::Result<PathBuf> {
        let mut env = self.env()?;
        let path = env.with_local_frame(4, |env| -> jni::errors::Result<String> {
            let s = env
                .call_method(self.activity.as_obj(), method, "()Ljava/lang/String;", &[])?
                .l()?;
            Ok(env.get_string(&JString::from(s))?.into())
        })?;
        let path = PathBuf::from(path);
        std::fs::create_dir_all(&path)?;
        Ok(path)
    }

    fn call_void(&self, method: &str, sig: &str, args: &[JValue]) {
        let outcome = self.env().and_then(|mut env| {
            env.call_method(self.activity.as_obj(), method, sig, args)
                .map(|_| ())
        });
        if let Err(e) = outcome {
            log::error!("DuckfootActivity.{method} failed: {e}");
        }
    }

    /// The next finished import, as `[kind, args…]`, or `None` once the queue is empty.
    fn poll_event(&self) -> Option<Vec<String>> {
        let mut env = self.env().ok()?;
        let event = env.with_local_frame(8, |env| -> jni::errors::Result<Option<Vec<String>>> {
            let array = env
                .call_method(
                    self.activity.as_obj(),
                    "pollEvent",
                    "()[Ljava/lang/String;",
                    &[],
                )?
                .l()?;
            if array.is_null() {
                return Ok(None);
            }
            let array = JObjectArray::from(array);
            let len = env.get_array_length(&array)?;
            let mut items = Vec::with_capacity(len as usize);
            for i in 0..len {
                let item = JString::from(env.get_object_array_element(&array, i)?);
                items.push(env.get_string(&item)?.into());
                env.delete_local_ref(item)?;
            }
            Ok(Some(items))
        });
        match event {
            Ok(event) => event,
            Err(e) => {
                log::error!("DuckfootActivity.pollEvent failed: {e}");
                None
            }
        }
    }
}

fn string_array<'local>(
    env: &mut JNIEnv<'local>,
    items: &[String],
) -> jni::errors::Result<JObjectArray<'local>> {
    let array = env.new_object_array(items.len() as i32, "java/lang/String", JObject::null())?;
    for (i, item) in items.iter().enumerate() {
        let s = env.new_string(item)?;
        env.set_object_array_element(&array, i as i32, &s)?;
        env.delete_local_ref(s)?;
    }
    Ok(array)
}

struct AndroidPlatform {
    bridge: Bridge,
    /// Private: imported originals and their sidecars.
    library: PathBuf,
    /// Shared `Pictures/Duckfoot`: where exports land, visible to the gallery.
    pictures: PathBuf,
    /// Answers that needed no activity.
    answered: Vec<Response>,
}

impl AndroidPlatform {
    fn pick(&self, kind: i32) {
        self.bridge
            .call_void("requestPick", "(I)V", &[JValue::Int(kind)]);
    }

    fn exports_dir(&mut self) -> Option<PathBuf> {
        match std::fs::create_dir_all(&self.pictures) {
            Ok(()) => Some(self.pictures.clone()),
            Err(e) => {
                self.answered.push(Response::Status(format!(
                    "cannot write to {}: {e}",
                    self.pictures.display()
                )));
                None
            }
        }
    }
}

impl Platform for AndroidPlatform {
    fn request(&mut self, request: Request) {
        match request {
            Request::OpenImage { .. } => self.pick(PICK_OPEN),
            Request::AddImages { .. } => self.pick(PICK_ADD),
            Request::BrowseFolder { .. } => self.pick(PICK_FOLDER),
            // No save dialog: exports go where the gallery finds them.
            Request::BatchOutputDir { .. } => {
                if let Some(dir) = self.exports_dir() {
                    self.answered.push(Response::BatchOutputDir(dir));
                }
            }
            Request::ExportImage { name, .. } => {
                if let Some(dir) = self.exports_dir() {
                    let path = unused_name(&dir, &name);
                    self.answered.push(Response::ExportImage(path));
                }
            }
            // Beside the exports, where a file manager or the share sheet finds it.
            Request::ExportPreset { name } => {
                if let Some(dir) = self.exports_dir() {
                    let path = unused_name(&dir, &name);
                    self.answered.push(Response::ExportPreset(path));
                }
            }
            Request::ImportPreset { .. } => self.answered.push(Response::Status(
                "Importing presets needs a file picker this build does not have yet".into(),
            )),
        }
    }

    fn poll(&mut self) -> Vec<Response> {
        let mut out = std::mem::take(&mut self.answered);
        while let Some(event) = self.bridge.poll_event() {
            let mut args = event.into_iter();
            let kind = args.next().unwrap_or_default();
            let args: Vec<String> = args.collect();
            let mut paths = args.iter().map(PathBuf::from);
            match kind.as_str() {
                "open" => out.extend(paths.next().map(Response::OpenImage)),
                "add" if !args.is_empty() => out.push(Response::AddImages(paths.collect())),
                "folder" => out.extend(paths.next().map(Response::BrowseFolder)),
                // Progress and problems alike go to the status bar.
                "status" => out.extend(args.into_iter().next().map(Response::Status)),
                other => log::warn!("unknown event from the activity: {other}"),
            }
        }
        out
    }

    fn bookmarks(&self) -> Vec<FolderBookmark> {
        vec![FolderBookmark {
            name: "Library".into(),
            path: self.library.clone(),
        }]
    }

    fn default_output_dir(&self) -> Option<PathBuf> {
        Some(self.pictures.clone())
    }

    fn settings_dir(&self) -> Option<PathBuf> {
        // Private to the app, like the library it sits beside.
        self.library.parent().map(|p| p.join("settings"))
    }

    fn leave(&mut self) {
        self.bridge.call_void("leave", "()V", &[]);
    }

    fn export_completed(&mut self, path: &Path) {
        if let Some(path_str) = path.to_str() {
            if let Ok(env) = self.bridge.env() {
                if let Ok(jpath) = env.new_string(path_str) {
                    self.bridge.call_void(
                        "scanMediaFile",
                        "(Ljava/lang/String;)V",
                        &[JValue::Object(&jpath)],
                    );
                }
            }
        }
    }
}

/// `name` in `dir`, or `stem (2).ext`, `stem (3).ext`… if that is taken. Exporting the
/// same photo twice must not silently replace the first file in someone's gallery.
fn unused_name(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let name = Path::new(name);
    let stem = name
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("export");
    let ext = name.extension().and_then(|s| s.to_str()).unwrap_or("png");
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}).{ext}")))
        .find(|p| !p.exists())
        .expect("an unbounded range always finds a free name")
}
