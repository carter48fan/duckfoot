#![no_main]

// A .cube look file named by a sidecar or a preset from someone else.
libfuzzer_sys::fuzz_target!(|data: &[u8]| photo_engine::decode::fuzzing::cube(data));
