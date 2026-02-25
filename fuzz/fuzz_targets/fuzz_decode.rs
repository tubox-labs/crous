//! Fuzz target for Crous decode functions.
//!
//! Run with: cargo +nightly fuzz run fuzz_decode
//!
//! This target feeds arbitrary bytes to the decoder and ensures it:
//! 1. Does not panic (safe even on malformed input)
//! 2. Does not hang (bounded by length checks)
//! 3. Reports errors gracefully

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Attempt to decode arbitrary bytes — must not panic.
    let mut decoder = crous_core::Decoder::new(data);
    let _ = decoder.decode_all_owned();

    // Also try text parsing — must not panic.
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = crous_core::text::parse(text);
    }
});
