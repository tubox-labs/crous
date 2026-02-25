//! Fuzz target: dedup (string deduplication) integrity.
//!
//! Generates structured inputs with repeated strings and verifies
//! that dedup encoding produces correct roundtrip results and that
//! corrupting the StringDict causes graceful errors, not panics.
//!
//! Run with: cargo +nightly fuzz run fuzz_dedup

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct DedupInput {
    /// Pool of strings to choose from (creates duplicates).
    string_pool: Vec<String>,
    /// Values that reference the pool.
    value_indices: Vec<u8>,
    /// Byte indices to corrupt in the encoded output.
    corruptions: Vec<(u16, u8)>,
}

fuzz_target!(|input: DedupInput| {
    if input.string_pool.is_empty() || input.value_indices.is_empty() {
        return;
    }
    if input.value_indices.len() > 256 {
        return;
    }

    // Build values from the string pool (creating intentional duplicates)
    let values: Vec<crous_core::Value> = input
        .value_indices
        .iter()
        .map(|idx| {
            let s = &input.string_pool[(*idx as usize) % input.string_pool.len()];
            crous_core::Value::Str(s.clone())
        })
        .collect();

    // Encode with dedup enabled
    let mut enc = crous_core::Encoder::new();
    enc.enable_dedup();
    for v in &values {
        if enc.encode_value(v).is_err() {
            return;
        }
    }
    let bytes = match enc.finish() {
        Ok(b) => b,
        Err(_) => return,
    };

    // Decode and verify roundtrip
    let mut dec = crous_core::Decoder::new(&bytes);
    match dec.decode_all_owned() {
        Ok(decoded) => {
            assert_eq!(
                decoded.len(),
                values.len(),
                "Dedup roundtrip count mismatch"
            );
            for (orig, dec) in values.iter().zip(decoded.iter()) {
                assert_eq!(orig, dec, "Dedup roundtrip value mismatch");
            }
        }
        Err(e) => {
            panic!("Dedup roundtrip decode failed: {e}");
        }
    }

    // Encode without dedup for comparison
    let mut enc_no_dedup = crous_core::Encoder::new();
    for v in &values {
        if enc_no_dedup.encode_value(v).is_err() {
            return;
        }
    }
    let bytes_no_dedup = match enc_no_dedup.finish() {
        Ok(b) => b,
        Err(_) => return,
    };

    // Verify non-dedup also roundtrips correctly
    let mut dec2 = crous_core::Decoder::new(&bytes_no_dedup);
    match dec2.decode_all_owned() {
        Ok(decoded2) => {
            assert_eq!(decoded2.len(), values.len());
            for (orig, dec) in values.iter().zip(decoded2.iter()) {
                assert_eq!(orig, dec);
            }
        }
        Err(e) => {
            panic!("Non-dedup roundtrip decode failed: {e}");
        }
    }

    // Corrupt the dedup-encoded bytes and verify graceful handling
    if !input.corruptions.is_empty() && !bytes.is_empty() {
        let mut corrupted = bytes.clone();
        for (idx, val) in &input.corruptions {
            let i = (*idx as usize) % corrupted.len();
            corrupted[i] = *val;
        }
        // Must not panic
        let mut dec3 = crous_core::Decoder::new(&corrupted);
        let _ = dec3.decode_all_owned();
    }
});
