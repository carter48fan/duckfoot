//! The ICC profile the operating system has assigned to a monitor, the one a calibrator
//! installed or the maker's: colord on Linux, ICM on Windows, ColorSync on macOS.
//!
//! Each lookup is a few system calls (on Linux a D-Bus round trip to colord), so callers
//! make it when the window reaches a different monitor, not every frame, and off the UI
//! thread.

/// Which monitor, as the windowing system names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Monitor {
    /// Linux: the output's connector name (`DP-1`, `eDP-1`), matched against colord's
    /// `XRANDR_name`.
    Output(Option<String>),
    /// Windows: the display device name (`\\.\DISPLAY1`).
    Device(String),
    /// macOS: the `CGDirectDisplayID`.
    Display(u32),
}

/// A monitor's profile: its description and its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub icc: Vec<u8>,
}

/// macOS's main display, for a window that is on none yet.
#[cfg(target_os = "macos")]
pub fn main_display() -> u32 {
    objc2_core_graphics::CGMainDisplayID()
}

/// The profile the system has for `monitor`. `Ok(None)` when it has none (or none this
/// code can tell is that monitor's); `Err` when the system could not be asked.
pub fn lookup(monitor: &Monitor) -> Result<Option<Profile>, String> {
    imp::lookup(monitor)
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
mod imp {
    use super::{Monitor, Profile};
    use std::collections::HashMap;
    use zbus::blocking::{Connection, Proxy};
    use zbus::zvariant::OwnedObjectPath;

    const SERVICE: &str = "org.freedesktop.ColorManager";

    pub fn lookup(monitor: &Monitor) -> Result<Option<Profile>, String> {
        let Monitor::Output(output) = monitor else {
            return Ok(None);
        };
        let bus = Connection::system().map_err(|e| format!("no system bus: {e}"))?;
        let manager = Proxy::new(&bus, SERVICE, "/org/freedesktop/ColorManager", SERVICE)
            .map_err(|e| e.to_string())?;
        let devices: Vec<OwnedObjectPath> = match manager.call("GetDevicesByKind", &("display",)) {
            Ok(d) => d,
            // Not installed, or not running and not activatable: nothing to ask.
            Err(zbus::Error::MethodError(name, ..))
                if name.as_str() == "org.freedesktop.DBus.Error.ServiceUnknown" =>
            {
                return Err("colord is not running".into());
            }
            Err(e) => return Err(format!("colord: {e}")),
        };
        // Each display colord knows, with the output it is on and its default profile
        // (the first of its profiles).
        let mut found = Vec::new();
        for path in devices {
            let device = Proxy::new(&bus, SERVICE, path, "org.freedesktop.ColorManager.Device")
                .map_err(|e| e.to_string())?;
            let meta: HashMap<String, String> = device.get_property("Metadata").unwrap_or_default();
            let profiles: Vec<OwnedObjectPath> =
                device.get_property("Profiles").unwrap_or_default();
            let Some(first) = profiles.into_iter().next() else {
                continue;
            };
            let profile = Proxy::new(&bus, SERVICE, first, "org.freedesktop.ColorManager.Profile")
                .map_err(|e| e.to_string())?;
            let file: String = profile.get_property("Filename").unwrap_or_default();
            if file.is_empty() {
                continue;
            }
            let title: String = profile.get_property("Title").unwrap_or_default();
            found.push((meta.get("XRANDR_name").cloned(), file, title));
        }
        // The output's own; failing a name to match, the only one there is. With several
        // and no match, guessing would show one screen's colours by another's profile.
        let chosen = match output {
            Some(name) => found.iter().position(|f| f.0.as_deref() == Some(name)),
            None => None,
        }
        .or((found.len() == 1).then_some(0));
        let Some(i) = chosen else {
            return Ok(None);
        };
        let (_, file, title) = found.swap_remove(i);
        let icc = std::fs::read(&file).map_err(|e| format!("{file}: {e}"))?;
        let name = if title.is_empty() { file } else { title };
        Ok(Some(Profile { name, icc }))
    }
}

#[cfg(windows)]
mod imp {
    use super::{Monitor, Profile};
    use windows_sys::Win32::Graphics::Gdi::{CreateDCW, DeleteDC};
    use windows_sys::Win32::UI::ColorSystem::GetICMProfileW;

    pub fn lookup(monitor: &Monitor) -> Result<Option<Profile>, String> {
        let Monitor::Device(device) = monitor else {
            return Ok(None);
        };
        let wide: Vec<u16> = device.encode_utf16().chain([0]).collect();
        // SAFETY: `wide` is NUL-terminated and outlives the call; the DC is deleted below
        // on every path, and the buffer is sized to what the first call asked for.
        let path = unsafe {
            let dc = CreateDCW(
                std::ptr::null(),
                wide.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
            );
            if dc.is_null() {
                return Err(format!("could not open {device}"));
            }
            let mut len = 0u32;
            GetICMProfileW(dc, &mut len, std::ptr::null_mut());
            let mut buf = vec![0u16; len.max(1) as usize];
            let ok = len > 0 && GetICMProfileW(dc, &mut len, buf.as_mut_ptr()) != 0;
            DeleteDC(dc);
            if !ok {
                return Ok(None);
            }
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            String::from_utf16_lossy(&buf[..end])
        };
        let icc = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
        let name = std::path::Path::new(&path)
            .file_name()
            .map_or(path.clone(), |n| n.to_string_lossy().into_owned());
        Ok(Some(Profile { name, icc }))
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{Monitor, Profile};
    use objc2_core_graphics::{CGColorSpace, CGDisplayCopyColorSpace};

    pub fn lookup(monitor: &Monitor) -> Result<Option<Profile>, String> {
        let Monitor::Display(id) = monitor else {
            return Ok(None);
        };
        // ColorSync's profile for the display, as a colour space; never null for a
        // display that exists (Core Graphics falls back to generic RGB).
        let space = CGDisplayCopyColorSpace(*id);
        let Some(data) = CGColorSpace::icc_data(Some(&space)) else {
            return Ok(None);
        };
        Ok(Some(Profile {
            name: "the display's ColorSync profile".into(),
            icc: data.to_vec(),
        }))
    }
}

#[cfg(not(any(windows, target_os = "macos", all(unix, not(target_os = "android")))))]
mod imp {
    use super::{Monitor, Profile};

    pub fn lookup(_: &Monitor) -> Result<Option<Profile>, String> {
        Ok(None)
    }
}
