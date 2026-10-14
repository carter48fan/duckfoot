//! Which monitor the window is on, and the profile the system has for it (W-14): looked up
//! on a thread of its own whenever the window reaches another monitor, and handed to the
//! app, which uses it while its screen is set to "system".

use std::sync::mpsc::{self, Receiver};

use photo_app::PhotoApp;
use winit::event_loop::EventLoopProxy;
use winit::window::Window;

use crate::UserEvent;

type Found = Result<Option<(String, Vec<u8>)>, String>;

#[derive(Default)]
pub(crate) struct Watch {
    /// The monitor last looked up, so moving within it looks nothing up again.
    last: Option<display_profile::Monitor>,
    pending: Option<Receiver<Found>>,
}

impl Watch {
    /// Look the profile up again if the window is now on a different monitor.
    pub(crate) fn check(&mut self, window: &Window, proxy: &EventLoopProxy<UserEvent>) {
        let monitor = monitor_of(window);
        if self.last.as_ref() == Some(&monitor) {
            return;
        }
        self.last = Some(monitor.clone());
        let (tx, rx) = mpsc::channel();
        let proxy = proxy.clone();
        let spawned = std::thread::Builder::new()
            .name("duckfoot-display-profile".into())
            .spawn(move || {
                let found = display_profile::lookup(&monitor).map(|p| p.map(|p| (p.name, p.icc)));
                let _ = tx.send(found);
                let _ = proxy.send_event(UserEvent::Wake);
            });
        if spawned.is_ok() {
            self.pending = Some(rx);
        }
    }

    /// Hand the app a finished lookup, if one has come back.
    pub(crate) fn poll(&mut self, app: &mut PhotoApp) {
        let Some(rx) = &self.pending else { return };
        if let Ok(found) = rx.try_recv() {
            if let Err(e) = &found {
                log::info!("no system display profile: {e}");
            }
            app.set_system_display(found);
            self.pending = None;
        }
    }
}

fn monitor_of(window: &Window) -> display_profile::Monitor {
    let current = window.current_monitor();
    #[cfg(windows)]
    {
        use winit::platform::windows::MonitorHandleExtWindows;
        match current {
            Some(m) => display_profile::Monitor::Device(m.native_id()),
            None => display_profile::Monitor::Device(r"\\.\DISPLAY1".into()),
        }
    }
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::MonitorHandleExtMacOS;
        match current {
            Some(m) => display_profile::Monitor::Display(m.native_id()),
            None => display_profile::Monitor::Display(display_profile::main_display()),
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        display_profile::Monitor::Output(current.and_then(|m| m.name()))
    }
}
