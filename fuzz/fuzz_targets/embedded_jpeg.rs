#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| photo_engine::decode::fuzzing::embedded_jpeg(data));
