//! Fuzz target: Limits enforcement.
//!
//! Generates structured inputs that specifically target limit boundaries:
//! deep nesting, many items, large strings, large blocks.
//! Verifies the decoder gracefully rejects them without panicking
//! or allocating unbounded memory.
//!
//! Run with: cargo +nightly fuzz run fuzz_limits

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct LimitsInput {
    /// Controls max nesting depth for the Limits (1-255).
    max_depth: u8,
    /// Controls max items (1-10000).
    max_items: u16,
    /// Controls max string length (1-65535).
    max_string: u16,
    /// Actual nesting depth to attempt.
    nesting: u8,
    /// Number of items in array.
    num_items: u16,
    /// String content.
    string: String,
    /// Whether to use zero-copy decode path too.
    try_zero_copy: bool,
}

fuzz_target!(|input: LimitsInput| {
    let max_depth = (input.max_depth as usize).max(1);
    let max_items = (input.max_items as usize).max(1);
    let max_string = (input.max_string as usize).max(1);

    let limits = crous_core::Limits {
        max_nesting_depth: max_depth,
        max_block_size: 1024 * 1024, // 1 MiB
        max_items,
        max_memory: 4 * 1024 * 1024, // 4 MiB
        max_string_length: max_string,
    };

    // Build a value that might violate the limits
    let mut value = crous_core::Value::Null;

    // Nest arrays
    let nesting = input.nesting as usize;
    for _ in 0..nesting {
        value = crous_core::Value::Array(vec![value]);
    }

    // Or build a large array
    let num_items = input.num_items as usize;
    if num_items > 0 && nesting == 0 {
        let items: Vec<crous_core::Value> = (0..num_items.min(5000))
            .map(|i| crous_core::Value::UInt(i as u64))
            .collect();
        value = crous_core::Value::Array(items);
    }

    // Or use a potentially-long string
    if !input.string.is_empty() && nesting == 0 && num_items == 0 {
        value = crous_core::Value::Str(input.string.clone());
    }

    // Encode with default (unlimited-ish) limits
    let mut enc = crous_core::Encoder::new();
    if enc.encode_value(&value).is_err() {
        return; // Encoder has its own limits
    }
    let bytes = match enc.finish() {
        Ok(b) => b,
        Err(_) => return,
    };

    // Decode with the fuzz-controlled limits — must not panic
    let mut dec = crous_core::Decoder::with_limits(&bytes, limits.clone());
    let result_owned = dec.decode_all_owned();
    // It's fine if it returns an error — the point is no panic/OOM.

    if input.try_zero_copy {
        let mut dec = crous_core::Decoder::with_limits(&bytes, limits);
        let _ = dec.decode_all();
    }

    // If decoding succeeded, verify roundtrip consistency
    if let Ok(values) = result_owned {
        if values.len() == 1 {
            // Re-encode and compare
            let mut enc2 = crous_core::Encoder::new();
            if enc2.encode_value(&values[0]).is_ok() {
                if let Ok(bytes2) = enc2.finish() {
                    assert_eq!(bytes, bytes2, "Non-deterministic encoding");
                }
            }
        }
    }
});
