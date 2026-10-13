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
const PICK_PRESET: i32 = 3;

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

    // Once the app has stopped (its edits flushed on the way out), the process goes too:
    // a second android_main in the same process came up to a black screen (H12), so the
    // next launch is a clean cold start instead.
    let exit_bridge = bridge.clone();
    duckfoot::run(
        event_loop,
        duckfoot::Options {
            pending_open: Vec::new(),
            platform: Box::new(move || {
                Box::new(AndroidPlatform {
                    bridge: bridge.clone(),
                    library: library.clone(),
                    pictures: pictures.clone(),
                    answered: Vec::new(),
                })
            }),
        },
    )?;
    exit_bridge.call_void("die", "()V", &[]);
    Ok(())
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

            env.with_local_frame(8, |env| -> jni::errors::Result<()> {
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
                let array = string_array(env, &extensions)?;
                env.call_method(
                    activity.as_obj(),
                    "onNativeReady",
                    "([Ljava/lang/String;)V",
                    &[JValue::Object(&array)],
                )?;
                Ok(())
            })?;
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
        let path = self
            .with_env(method, |env| {
                let s = env
                    .call_method(self.activity.as_obj(), method, "()Ljava/lang/String;", &[])?
                    .l()?;
                Ok(String::from(env.get_string(&JString::from(s))?))
            })
            .ok_or_else(|| anyhow::anyhow!("DuckfootActivity.{method} gave no directory"))?;
        let path = PathBuf::from(path);
        std::fs::create_dir_all(&path)?;
        Ok(path)
    }

    /// Runs `f` in a local frame of its own, so every local reference it makes is freed
    /// when it returns: this thread is attached for good and never returns to Java, so
    /// nothing else would free them, and the table they fill holds 512 (M-20). A Java
    /// exception left pending makes every later JNI call fail: it is reported to logcat
    /// and cleared. `None` if the call failed.
    fn with_env<T>(
        &self,
        what: &str,
        f: impl FnOnce(&mut JNIEnv) -> jni::errors::Result<T>,
    ) -> Option<T> {
        let outcome = self.env().and_then(|mut env| {
            let r = env.with_local_frame(16, f);
            if r.is_err() && env.exception_check().unwrap_or(false) {
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
            r
        });
        match outcome {
            Ok(v) => Some(v),
            Err(e) => {
                log::error!("DuckfootActivity.{what} failed: {e}");
                None
            }
        }
    }

    fn call_void(&self, method: &str, sig: &str, args: &[JValue]) {
        self.with_env(method, |env| {
            env.call_method(self.activity.as_obj(), method, sig, args)
                .map(|_| ())
        });
    }

    /// `void method(String)`, the string made inside the call's frame.
    fn call_with_string(&self, method: &str, arg: &str) {
        self.with_env(method, |env| {
            let s = env.new_string(arg)?;
            env.call_method(
                self.activity.as_obj(),
                method,
                "(Ljava/lang/String;)V",
                &[JValue::Object(&s)],
            )
            .map(|_| ())
        });
    }

    /// The next finished import, as `[kind, args…]`, or `None` once the queue is empty.
    fn poll_event(&self) -> Option<Vec<String>> {
        self.with_env("pollEvent", |env| {
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
        })
        .flatten()
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
            Request::Share { path } => {
                if let Some(path) = path.to_str() {
                    self.bridge.call_with_string("shareFile", path);
                }
            }
            Request::ImportPreset { .. } => self.pick(PICK_PRESET),
            // The phone's screen is drawn as sRGB; there is no profile to choose. Nor is
            // there a proof to print through: the proof chips are the desktop's.
            // A look's .cube is picked on the desktop; on the phone a stack that names one
            // still applies it.
            Request::DisplayProfile { .. }
            | Request::PrinterProfile { .. }
            | Request::LookFile { .. } => {}
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
                "preset" => out.extend(paths.next().map(Response::ImportPreset)),
                "trim" => out.push(Response::LowMemory),
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

    fn library_dir(&self) -> Option<PathBuf> {
        Some(self.library.clone())
    }

    fn settings_dir(&self) -> Option<PathBuf> {
        // Private to the app, like the library it sits beside.
        self.library.parent().map(|p| p.join("settings"))
    }

    fn leave(&mut self) {
        self.bridge.call_void("leave", "()V", &[]);
    }

    fn can_share(&self) -> bool {
        true
    }

    fn exclude_gestures(&mut self, rects: &[[i32; 4]]) {
        let flat: Vec<i32> = rects.iter().flatten().copied().collect();
        self.bridge.with_env("setGestureExclusion", |env| {
            let array = env.new_int_array(flat.len() as i32)?;
            env.set_int_array_region(&array, 0, &flat)?;
            env.call_method(
                self.bridge.activity.as_obj(),
                "setGestureExclusion",
                "([I)V",
                &[JValue::Object(&array)],
            )
            .map(|_| ())
        });
    }

    fn export_completed(&mut self, path: &Path) {
        if let Some(path) = path.to_str() {
            self.bridge.call_with_string("scanMediaFile", path);
        }
    }
}

/// `name` in `dir`, or `stem (2).ext`, `stem (3).ext`… if that is taken. Exporting the
/// same photo twice must not silently replace the first file in someone's gallery.
fn unused_name(dir: &Path, name: &str) -> PathBuf {
    // Claimed by creating it, not by checking it is absent: two exports at once (a single
    // and a batch) cannot both be handed the same name (L-23). The export writes over the
    // empty file.
    let claim = |p: &Path| {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(p)
            .is_ok()
    };
    let first = dir.join(name);
    if claim(&first) {
        return first;
    }
    let name = Path::new(name);
    let stem = name
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("export");
    let ext = name.extension().and_then(|s| s.to_str()).unwrap_or("png");
    (2..10_000)
        .map(|n| dir.join(format!("{stem} ({n}).{ext}")))
        .find(|p| claim(p))
        .unwrap_or(first)
}
