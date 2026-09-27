#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| photo_engine::decode::fuzzing::icc(data));
