//! Fuzz target: compression boundary testing.
//!
//! Encodes structured values with dedup and compression enabled,
//! then corrupts random bytes in the output and verifies the decoder
//! never panics (catches checksum/decompression errors gracefully).
//!
//! Run with: cargo +nightly fuzz run fuzz_compress_corrupt

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

#[derive(Debug, Arbitrary)]
struct CompressCorruptInput {
    /// Which compression to use (0=None, 1=Zstd, 2=Lz4, 3=Snappy).
    compression: u8,
    /// Enable string deduplication.
    dedup: bool,
    /// Values to encode.
    values: Vec<SimpleValue>,
    /// Byte indices to corrupt (mod file length).
    corruptions: Vec<(u16, u8)>,
}

#[derive(Debug, Arbitrary)]
enum SimpleValue {
    Null,
    Bool(bool),
    UInt(u64),
    Int(i64),
    Str(String),
    Bytes(Vec<u8>),
}

impl SimpleValue {
    fn to_value(&self) -> crous_core::Value {
        match self {
            SimpleValue::Null => crous_core::Value::Null,
            SimpleValue::Bool(b) => crous_core::Value::Bool(*b),
            SimpleValue::UInt(n) => crous_core::Value::UInt(*n),
            SimpleValue::Int(n) => crous_core::Value::Int(*n),
            SimpleValue::Str(s) => crous_core::Value::Str(s.clone()),
            SimpleValue::Bytes(b) => crous_core::Value::Bytes(b.clone()),
        }
    }
}

fuzz_target!(|input: CompressCorruptInput| {
    if input.values.is_empty() || input.values.len() > 128 {
        return;
    }

    let compression = match input.compression % 4 {
        0 => crous_core::wire::CompressionType::None,
        1 => crous_core::wire::CompressionType::Zstd,
        2 => crous_core::wire::CompressionType::Lz4,
        3 => crous_core::wire::CompressionType::Snappy,
        _ => unreachable!(),
    };

    // Encode
    let mut enc = crous_core::Encoder::new();
    enc.set_compression(compression);
    if input.dedup {
        enc.enable_dedup();
    }

    for v in &input.values {
        if enc.encode_value(&v.to_value()).is_err() {
            return;
        }
    }

    let mut bytes = match enc.finish() {
        Ok(b) => b,
        Err(_) => return,
    };

    // First verify uncorrupted roundtrip works
    let mut dec = crous_core::Decoder::new(&bytes);
    let originals = match dec.decode_all_owned() {
        Ok(v) => v,
        Err(_) => return, // Encoding edge case
    };
    assert_eq!(originals.len(), input.values.len());

    // Now corrupt bytes
    if !input.corruptions.is_empty() && !bytes.is_empty() {
        for (idx, val) in &input.corruptions {
            let i = (*idx as usize) % bytes.len();
            bytes[i] = *val;
        }

        // Decode corrupted data — MUST NOT PANIC
        let mut dec = crous_core::Decoder::new(&bytes);
        let _ = dec.decode_all_owned();

        // Also try with strict limits
        let strict = crous_core::Limits::strict();
        let mut dec = crous_core::Decoder::with_limits(&bytes, strict);
        let _ = dec.decode_all_owned();
    }
});
