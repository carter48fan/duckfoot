//! Desktop bootstrap: a logger, the command line, native dialogs. The shell itself is in
//! `lib.rs`, shared with the `duckfoot-android` crate.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    duckfoot::run(
        duckfoot::event_loop()?,
        duckfoot::Options {
            pending_open: std::env::args().nth(1).map(Into::into),
            platform: Box::new(photo_app::platform::default_platform),
        },
    )
}
