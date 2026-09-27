#![no_main]

// A preset file from someone else: refused or made safe, never a panic.
libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = photo_app::user_presets::UserPreset::from_json(data);
});
