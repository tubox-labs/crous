//! Fuzz target: text format parser.
//!
//! Feeds arbitrary strings to the Crous text parser to verify it
//! never panics on malformed input and gracefully returns errors.
//!
//! Run with: cargo +nightly fuzz run fuzz_text

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only feed valid UTF-8 to the text parser.
    if let Ok(text) = std::str::from_utf8(data) {
        // Must not panic on any input.
        if let Ok(value) = crous_core::text::parse(text) {
            // If parsing succeeds, pretty-printing must also succeed.
            let printed = crous_core::text::pretty_print(&value, 2);
            // Re-parsing the pretty-printed output should succeed.
            let reparsed = crous_core::text::parse(&printed)
                .expect("pretty_print output should be parseable");
            // And should produce the same value.
            assert_eq!(value, reparsed, "Text roundtrip mismatch");
        }
    }
});
