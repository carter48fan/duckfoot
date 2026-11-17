#![no_main]

// A DNG's opcode lists, parsed and applied to a mosaic.
libfuzzer_sys::fuzz_target!(|data: &[u8]| photo_engine::decode::fuzzing::opcodes(data));
