//! GPU errors that wgpu reports outside any error scope.
//!
//! wgpu's default for an uncaptured error is to panic, which takes the user's unsaved work
//! with it. The shell installs [`report`] as the device's handler instead; the app picks
//! the message up on its next frame and shows it (DESIGN.md: failures surface in the UI).

use std::sync::Mutex;

static PENDING: Mutex<Option<String>> = Mutex::new(None);

/// Record an error for the UI. Keeps the first until it has been shown; a failure usually
/// arrives as a burst, and the first is the one that explains the rest.
pub fn report(message: impl Into<String>) {
    let message = message.into();
    log::error!("GPU: {message}");
    if let Ok(mut slot) = PENDING.lock() {
        slot.get_or_insert(message);
    }
}

/// The error waiting to be shown, if any.
pub fn take() -> Option<String> {
    PENDING.lock().ok().and_then(|mut slot| slot.take())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_error_of_a_burst_is_kept() {
        report("out of memory");
        report("invalid texture");
        assert_eq!(take().as_deref(), Some("out of memory"));
        assert_eq!(take(), None);
    }
}
