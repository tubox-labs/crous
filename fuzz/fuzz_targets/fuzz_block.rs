//! Fuzz target: block-level parser.
//!
//! Feeds arbitrary bytes to the block parser to verify it never panics
//! or causes unbounded resource consumption on malformed input.
//!
//! Run with: cargo +nightly fuzz run fuzz_block

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Try parsing blocks from arbitrary bytes — must not panic.
    let mut offset = 0;
    while offset < data.len() {
        match crous_core::block::BlockReader::parse(data, offset) {
            Ok((block, consumed)) => {
                // Verify checksum computation doesn't panic.
                let _ = block.verify_checksum();
                offset += consumed;
                if consumed == 0 {
                    break; // Prevent infinite loop on zero-size parse.
                }
            }
            Err(_) => break,
        }
    }

    // Also try decoding with strict limits to ensure DoS resistance.
    let strict = crous_core::Limits::strict();
    let mut decoder = crous_core::Decoder::with_limits(data, strict);
    let _ = decoder.decode_all_owned();
});
