#![no_main]

// The app decodes every RAW under catch_unwind (the loader's `guarded`), so a panic inside
// rawler is a file that fails to open, not a crash; this target does the same, and hunts
// what that cannot contain: running out of memory and hanging. Duckfoot's own parsers are
// held to no panics at all by the tiff target.
libfuzzer_sys::fuzz_target!(
    init: {
        std::panic::set_hook(Box::new(|_| {}));
    },
    |data: &[u8]| {
        let _ = std::panic::catch_unwind(|| photo_engine::decode::fuzzing::raw(data));
    }
);
