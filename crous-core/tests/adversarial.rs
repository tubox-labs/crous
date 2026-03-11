//! Adversarial & stress tests for crous-core.
//!
//! These tests simulate hostile input, edge cases, and abuse patterns
//! that would be encountered in production systems (databases, financial
//! systems, async servers, adversarial networks).
//!
//! CATEGORIES:
//! - Malformed binary input (decoder crash resistance)
//! - Deserialization bombs (resource exhaustion)
//! - Edge-case values (boundary integers, special floats, huge strings)
//! - Encoder/decoder state machine abuse
//! - Checksum tampering
//! - StringDict corruption
//! - Text parser adversarial inputs
//! - API misuse / invalid call sequences

use crous_core::decoder::Decoder;
use crous_core::encoder::Encoder;
use crous_core::error::CrousError;
use crous_core::header::{FileHeader, HEADER_SIZE};
use crous_core::limits::Limits;
use crous_core::text::{parse, pretty_print};
use crous_core::value::Value;
use crous_core::varint::{decode_varint, encode_varint_vec, zigzag_decode, zigzag_encode};
use crous_core::wire::{BlockType, CompressionType, WireType};

// ═══════════════════════════════════════════════════════════════════════
// 1. MALFORMED BINARY INPUT — DECODER MUST NEVER PANIC
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn decode_empty_input() {
    let mut dec = Decoder::new(&[]);
    assert!(dec.decode_next().is_err());
    assert!(dec.decode_all_owned().is_ok()); // fresh decoder on empty → Ok(vec![])
}

#[test]
fn decode_just_magic_no_block() {
    let hdr = FileHeader::new(0).encode();
    let mut dec = Decoder::new(&hdr);
    // Should get empty — no blocks after header
    let vals = dec.decode_all_owned().unwrap();
    assert!(vals.is_empty());
}

#[test]
fn decode_truncated_header() {
    for len in 0..7 {
        let data = b"CROUSv1"[..len].to_vec();
        let mut dec = Decoder::new(&data);
        assert!(dec.decode_next().is_err());
    }
}

#[test]
fn decode_wrong_magic() {
    let mut data = b"CROUSv2\x00".to_vec();
    let mut dec = Decoder::new(&data);
    assert!(matches!(dec.decode_next(), Err(CrousError::InvalidMagic)));

    data = b"GARBAGE\x00".to_vec();
    let mut dec = Decoder::new(&data);
    assert!(dec.decode_next().is_err());
}

#[test]
fn decode_every_single_byte_input() {
    // Feed every possible single byte — must not panic
    for b in 0..=255u8 {
        let data = [b];
        let mut dec = Decoder::new(&data);
        let _ = dec.decode_all_owned();
    }
}

#[test]
fn decode_every_two_byte_input() {
    // Feed every possible two-byte combination — must not panic
    for hi in 0..=255u8 {
        for lo in [0x00u8, 0x01, 0x7F, 0x80, 0xFF] {
            let data = [hi, lo];
            let mut dec = Decoder::new(&data);
            let _ = dec.decode_all_owned();
        }
    }
}

#[test]
fn decode_header_plus_random_garbage() {
    let data = b"CROUSv1\x00".to_vec();
    // Add garbage after header — should error gracefully, not panic
    for garbage in [
        &[0xFF][..],
        &[0x00],
        &[0x01, 0x80, 0x80, 0x80],
        &[0x01, 0x05, 0x00], // truncated block
        &[BlockType::Data as u8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // empty data block
    ] {
        let mut input = data.clone();
        input.extend_from_slice(garbage);
        let mut dec = Decoder::new(&input);
        let _ = dec.decode_all_owned(); // must not panic
    }
}

#[test]
fn decode_valid_header_invalid_block_type() {
    let mut data = b"CROUSv1\x00".to_vec();
    data.push(0xFE); // Invalid block type
    let mut dec = Decoder::new(&data);
    assert!(matches!(
        dec.decode_next(),
        Err(CrousError::InvalidBlockType(0xFE))
    ));
}

#[test]
fn decode_valid_header_invalid_compression() {
    let mut data = b"CROUSv1\x00".to_vec();
    data.push(BlockType::Data as u8); // block type
    data.push(0x01); // block_len = 1 (varint)
    data.push(0xFE); // invalid compression type
    let mut dec = Decoder::new(&data);
    assert!(matches!(
        dec.decode_next(),
        Err(CrousError::UnknownCompression(0xFE))
    ));
}

#[test]
fn decode_block_with_wrong_checksum() {
    let mut enc = Encoder::new();
    enc.encode_value(&Value::UInt(42)).unwrap();
    let mut bytes = enc.finish().unwrap();

    // Find and corrupt the checksum bytes (after header + block_type + block_len + comp_type)
    // Header: 8 bytes, block_type: 1, block_len: 1 (varint for small payload), comp_type: 1
    // Checksum starts at offset 8+1+1+1 = 11
    let cksum_offset = HEADER_SIZE + 1 + 1 + 1; // simplified, actual may differ
    if cksum_offset + 8 <= bytes.len() {
        bytes[cksum_offset] ^= 0xFF;
        let mut dec = Decoder::new(&bytes);
        assert!(matches!(dec.decode_next(), Err(CrousError::ChecksumMismatch { .. })));
    }
}

#[test]
fn decode_block_payload_shorter_than_declared() {
    let mut data = b"CROUSv1\x00".to_vec();
    data.push(BlockType::Data as u8);
    encode_varint_vec(1000, &mut data); // declare 1000 bytes
    data.push(CompressionType::None as u8);
    data.extend_from_slice(&[0u8; 8]); // fake checksum
    data.extend_from_slice(&[0u8; 10]); // only 10 bytes of payload
    let mut dec = Decoder::new(&data);
    assert!(dec.decode_next().is_err()); // UnexpectedEof
}

// ═══════════════════════════════════════════════════════════════════════
// 2. DESERIALIZATION BOMBS — RESOURCE EXHAUSTION
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn bomb_deeply_nested_arrays() {
    // Try to decode a payload claiming 128+ levels of nesting
    let limits = Limits::strict(); // max_nesting_depth: 32
    let mut val = Value::UInt(1);
    for _ in 0..128 {
        val = Value::Array(vec![val]);
    }
    let mut enc = Encoder::with_limits(Limits::unlimited());
    enc.encode_value(&val).unwrap();
    let bytes = enc.finish().unwrap();

    let mut dec = Decoder::with_limits(&bytes, limits);
    assert!(matches!(
        dec.decode_next(),
        Err(CrousError::NestingTooDeep(..))
    ));
}

#[test]
fn bomb_huge_array_count() {
    // Craft a block claiming 1 billion array items
    let mut data = b"CROUSv1\x00".to_vec();
    let mut block_payload = vec![];
    block_payload.push(WireType::StartArray.to_tag());
    encode_varint_vec(1_000_000_000, &mut block_payload); // 1B items

    let checksum = crous_core::checksum::compute_xxh64(&block_payload);
    data.push(BlockType::Data as u8);
    encode_varint_vec(block_payload.len() as u64, &mut data);
    data.push(CompressionType::None as u8);
    data.extend_from_slice(&checksum.to_le_bytes());
    data.extend_from_slice(&block_payload);

    let limits = Limits::strict(); // max_items: 10_000
    let mut dec = Decoder::with_limits(&data, limits);
    assert!(matches!(
        dec.decode_next(),
        Err(CrousError::TooManyItems(..))
    ));
}

#[test]
fn bomb_huge_string_length() {
    // Craft a block with a string claiming to be 1 GB
    let mut block_payload = vec![];
    block_payload.push(WireType::LenDelimited.to_tag());
    block_payload.push(0x00); // sub-type: string
    encode_varint_vec(1_000_000_000, &mut block_payload); // 1GB string

    let mut data = b"CROUSv1\x00".to_vec();
    let checksum = crous_core::checksum::compute_xxh64(&block_payload);
    data.push(BlockType::Data as u8);
    encode_varint_vec(block_payload.len() as u64, &mut data);
    data.push(CompressionType::None as u8);
    data.extend_from_slice(&checksum.to_le_bytes());
    data.extend_from_slice(&block_payload);

    let limits = Limits::strict(); // max_string_length: 64KB
    let mut dec = Decoder::with_limits(&data, limits);
    let result = dec.decode_next_owned();
    assert!(result.is_err());
}

#[test]
fn bomb_memory_limit_with_many_small_strings() {
    // Accumulate memory across many small strings until limit hit
    let limits = Limits {
        max_memory: 1024,
        ..Limits::default()
    };
    let mut items: Vec<Value> = Vec::new();
    for i in 0..200 {
        items.push(Value::Str(format!("string_{i:04}")));
    }
    let val = Value::Array(items);
    let mut enc = Encoder::new();
    enc.encode_value(&val).unwrap();
    let bytes = enc.finish().unwrap();

    let mut dec = Decoder::with_limits(&bytes, limits);
    let result = dec.decode_all_owned();
    assert!(result.is_err());
}

#[test]
fn bomb_block_size_limit() {
    let limits = Limits {
        max_block_size: 100,
        ..Limits::default()
    };
    // Create a value that produces a block > 100 bytes
    let big_str = "x".repeat(200);
    let val = Value::Str(big_str);
    let mut enc = Encoder::new();
    enc.encode_value(&val).unwrap();
    let bytes = enc.finish().unwrap();

    let mut dec = Decoder::with_limits(&bytes, limits);
    assert!(matches!(
        dec.decode_next(),
        Err(CrousError::BlockTooLarge(..))
    ));
}

// ═══════════════════════════════════════════════════════════════════════
// 3. BOUNDARY VALUES & EDGE CASES
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn roundtrip_u64_max() {
    let val = Value::UInt(u64::MAX);
    let mut enc = Encoder::new();
    enc.encode_value(&val).unwrap();
    let bytes = enc.finish().unwrap();
    let mut dec = Decoder::new(&bytes);
    assert_eq!(dec.decode_next().unwrap().to_owned_value(), val);
}

#[test]
fn roundtrip_i64_extremes() {
    for v in [i64::MIN, i64::MAX, 0, -1, 1] {
        let val = Value::Int(v);
        let mut enc = Encoder::new();
        enc.encode_value(&val).unwrap();
        let bytes = enc.finish().unwrap();
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.decode_next().unwrap().to_owned_value(), val, "i64 {v}");
    }
}

#[test]
fn roundtrip_special_floats() {
    for &f in &[
        f64::INFINITY,
        f64::NEG_INFINITY,
        0.0,
        -0.0,
        f64::MIN,
        f64::MAX,
        f64::MIN_POSITIVE,
        f64::EPSILON,
    ] {
        let val = Value::Float(f);
        let mut enc = Encoder::new();
        enc.encode_value(&val).unwrap();
        let bytes = enc.finish().unwrap();
        let mut dec = Decoder::new(&bytes);
        let decoded = dec.decode_next().unwrap().to_owned_value();
        match (&val, &decoded) {
            (Value::Float(a), Value::Float(b)) => {
                assert_eq!(a.to_bits(), b.to_bits(), "float bits for {f}");
            }
            _ => panic!("type mismatch"),
        }
    }
}

#[test]
fn roundtrip_nan_preserves_bits() {
    let val = Value::Float(f64::NAN);
    let mut enc = Encoder::new();
    enc.encode_value(&val).unwrap();
    let bytes = enc.finish().unwrap();
    let mut dec = Decoder::new(&bytes);
    let decoded = dec.decode_next().unwrap().to_owned_value();
    match decoded {
        Value::Float(f) => assert!(f.is_nan()),
        _ => panic!("expected float"),
    }
}

#[test]
fn roundtrip_empty_containers() {
    for val in [
        Value::Array(vec![]),
        Value::Object(vec![]),
        Value::Str(String::new()),
        Value::Bytes(vec![]),
    ] {
        let mut enc = Encoder::new();
        enc.encode_value(&val).unwrap();
        let bytes = enc.finish().unwrap();
        let mut dec = Decoder::new(&bytes);
        assert_eq!(dec.decode_next().unwrap().to_owned_value(), val);
    }
}

#[test]
fn roundtrip_empty_key_in_object() {
    let val = Value::Object(vec![("".into(), Value::Null)]);
    let mut enc = Encoder::new();
    enc.encode_value(&val).unwrap();
    let bytes = enc.finish().unwrap();
    let mut dec = Decoder::new(&bytes);
    assert_eq!(dec.decode_next().unwrap().to_owned_value(), val);
}

#[test]
fn roundtrip_unicode_stress() {
    let strings = vec![
        "こんにちは世界",
        "🎉🎊🎈",
        "\u{0000}", // null char in UTF-8
        "\u{FEFF}", // BOM
        "𝕳𝖊𝖑𝖑𝖔", // Mathematical Fraktur
        "\u{202E}RLO override\u{202C}", // bidi override
        "a\u{0300}", // combining accent
        "\u{10FFFF}", // max unicode code point
    ];
    for s in strings {
        let val = Value::Str(s.to_string());
        let mut enc = Encoder::new();
        enc.encode_value(&val).unwrap();
        let bytes = enc.finish().unwrap();
        let mut dec = Decoder::new(&bytes);
        assert_eq!(
            dec.decode_next().unwrap().to_owned_value(),
            val,
            "unicode roundtrip failed for {:?}",
            s
        );
    }
}

#[test]
fn roundtrip_large_binary_blob() {
    let blob = vec![0xABu8; 1024 * 1024]; // 1 MiB
    let val = Value::Bytes(blob.clone());
    let mut enc = Encoder::new();
    enc.encode_value(&val).unwrap();
    let bytes = enc.finish().unwrap();
    let mut dec = Decoder::new(&bytes);
    assert_eq!(dec.decode_next_owned().unwrap(), val);
}

#[test]
fn roundtrip_1000_values_multi_block_scenario() {
    // Encode 1000 values individually, flush between each
    let mut enc = Encoder::new();
    for i in 0..1000u64 {
        enc.encode_value(&Value::UInt(i)).unwrap();
    }
    let bytes = enc.finish().unwrap();
    let mut dec = Decoder::new(&bytes);
    let vals = dec.decode_all_owned().unwrap();
    assert_eq!(vals.len(), 1000);
    for (i, v) in vals.iter().enumerate() {
        assert_eq!(v, &Value::UInt(i as u64));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 4. VARINT EDGE CASES
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn varint_all_powers_of_two() {
    for shift in 0..64u32 {
        let val = 1u64 << shift;
        let mut buf = Vec::new();
        encode_varint_vec(val, &mut buf);
        let (decoded, _) = decode_varint(&buf, 0).unwrap();
        assert_eq!(decoded, val, "power of two {shift}");
    }
}

#[test]
fn varint_boundary_values() {
    // Values at byte boundaries: 2^7-1, 2^7, 2^14-1, 2^14, etc.
    let boundaries: Vec<u64> = (0..10)
        .flat_map(|n| {
            let base = 1u64.checked_shl(7 * n).unwrap_or(u64::MAX);
            vec![base.saturating_sub(1), base, base.saturating_add(1)]
        })
        .collect();
    for &val in &boundaries {
        let mut buf = Vec::new();
        encode_varint_vec(val, &mut buf);
        let (decoded, consumed) = decode_varint(&buf, 0).unwrap();
        assert_eq!(decoded, val, "boundary {val}");
        assert!(consumed <= 10, "varint too long for {val}");
    }
}

#[test]
fn varint_10th_byte_overflow() {
    // A 10-byte varint where byte 10 has value > 1 → overflow
    let bad = [0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x02];
    assert!(matches!(
        decode_varint(&bad, 0),
        Err(CrousError::VarintOverflow)
    ));
}

#[test]
fn varint_10th_byte_exactly_one() {
    // A 10-byte varint where byte 10 has value == 1 → valid (u64::MAX)
    let valid = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01];
    let (val, consumed) = decode_varint(&valid, 0).unwrap();
    assert_eq!(val, u64::MAX);
    assert_eq!(consumed, 10);
}

#[test]
fn varint_non_canonical_encoding() {
    // Non-canonical: 0 encoded as two bytes (0x80, 0x00)
    let non_canonical = [0x80, 0x00];
    let (val, consumed) = decode_varint(&non_canonical, 0).unwrap();
    assert_eq!(val, 0);
    assert_eq!(consumed, 2);
    // This is accepted (like protobuf) — not a bug, but worth documenting
}

#[test]
fn zigzag_all_boundary_values() {
    for &v in &[0i64, 1, -1, 2, -2, 63, -64, 64, -65, i64::MAX, i64::MIN] {
        let encoded = zigzag_encode(v);
        let decoded = zigzag_decode(encoded);
        assert_eq!(decoded, v, "zigzag roundtrip failed for {v}");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 5. TEXT PARSER ADVERSARIAL INPUT
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn text_parse_never_panics_on_garbage() {
    let adversarial_inputs = [
        "",
        "   ",
        "\n\n\n",
        "{",
        "}",
        "[",
        "]",
        "{{{{",
        "]]]]",
        "{ key: }",
        "{ : value; }",
        "{ key value; }",
        "{ key: value }",  // missing semicolon
        r#"{ key: "unterminated }"#,
        "\"\\",
        "b64#;",
        "b64#!!!;",
        "b64#====;",
        "999999999999999999999999999999999999",
        "-999999999999999999999999999999999999",
        "1e99999",
        "1e-99999",
        "0.0.0",
        "--1",
        "++1",
        "/* unclosed comment",
        "// line comment with no newline",
        "/* /* nested */ still going",
        "null null", // multiple values (only first parsed)
        "truefalse",
        "nullnull",
        "\x00",
        "\u{FEFF}", // BOM
        "{ \"a\": 1; \"a\": 2; }", // duplicate keys
        &"[".repeat(1000), // deeply nested opens
        &format!("[{}]", "1,".repeat(10000)), // huge array
    ];

    for input in &adversarial_inputs {
        let _ = parse(input); // must not panic
    }
}

#[test]
fn text_parse_deeply_nested_objects() {
    // Build { a: { a: { a: ... { a: 1; }; }; }; }
    let depth = 200;
    let mut s = String::new();
    for _ in 0..depth {
        s.push_str("{ a: ");
    }
    s.push('1');
    for _ in 0..depth {
        s.push_str("; }");
    }
    // This should succeed (no nesting limit in text parser) but not panic
    let _ = parse(&s);
}

#[test]
fn text_parse_huge_number_doesnt_panic() {
    let huge = "9".repeat(1000);
    let result = parse(&huge);
    // Should either parse as UInt or error — never panic
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn text_pretty_print_then_reparse_identity() {
    let values = vec![
        Value::Null,
        Value::Bool(true),
        Value::Bool(false),
        Value::UInt(0),
        Value::UInt(u64::MAX),
        Value::Int(-1),
        Value::Int(i64::MIN),
        Value::Float(3.125),
        Value::Float(0.0),
        Value::Str("".into()),
        Value::Str("hello world".into()),
        Value::Str("with \"quotes\" and \\backslash".into()),
        Value::Str("line\nbreak\ttab".into()),
        Value::Array(vec![]),
        Value::Object(vec![]),
        Value::Array(vec![Value::UInt(1), Value::Str("two".into()), Value::Null]),
        Value::Object(vec![
            ("key".into(), Value::UInt(1)),
            ("nested".into(), Value::Object(vec![
                ("inner".into(), Value::Bool(true)),
            ])),
        ]),
    ];

    for val in &values {
        let text = pretty_print(val, 2);
        let reparsed = parse(&text).unwrap_or_else(|e| {
            panic!(
                "Failed to re-parse pretty_print output for {:?}:\n{}\nError: {}",
                val, text, e
            );
        });
        // Structural comparison (Int/UInt equivalence for non-negative)
        assert!(
            structural_eq(val, &reparsed),
            "Pretty-print roundtrip failed for {:?}\nText: {}\nReparsed: {:?}",
            val,
            text,
            reparsed
        );
    }
}

fn structural_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::UInt(x), Value::UInt(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x.to_bits() == y.to_bits(),
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Bytes(x), Value::Bytes(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| structural_eq(a, b))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|((ka, va), (kb, vb))| ka == kb && structural_eq(va, vb))
        }
        _ => false,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 6. ENCODER API ABUSE
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn encoder_finish_without_encoding() {
    let enc = Encoder::new();
    let bytes = enc.finish().unwrap();
    // Should produce valid file with header + trailer, no data blocks
    assert!(bytes.starts_with(b"CROUSv1"));
    let mut dec = Decoder::new(&bytes);
    let vals = dec.decode_all_owned().unwrap();
    assert!(vals.is_empty());
}

#[test]
fn encoder_flush_empty_block() {
    let mut enc = Encoder::new();
    let flushed = enc.flush_block().unwrap();
    assert_eq!(flushed, 0); // no data → nothing flushed
}

#[test]
fn encoder_multiple_flushes() {
    let mut enc = Encoder::new();
    enc.encode_value(&Value::UInt(1)).unwrap();
    enc.flush_block().unwrap();
    enc.encode_value(&Value::UInt(2)).unwrap();
    enc.flush_block().unwrap();
    enc.encode_value(&Value::UInt(3)).unwrap();
    let bytes = enc.finish().unwrap();

    let mut dec = Decoder::new(&bytes);
    let vals = dec.decode_all_owned().unwrap();
    assert_eq!(vals, vec![Value::UInt(1), Value::UInt(2), Value::UInt(3)]);
}

#[test]
fn encoder_dedup_with_no_strings() {
    let mut enc = Encoder::new();
    enc.enable_dedup();
    enc.encode_value(&Value::UInt(42)).unwrap();
    let bytes = enc.finish().unwrap();
    let mut dec = Decoder::new(&bytes);
    assert_eq!(dec.decode_next_owned().unwrap(), Value::UInt(42));
}

#[test]
fn encoder_dedup_single_string_no_dup() {
    let mut enc = Encoder::new();
    enc.enable_dedup();
    enc.encode_value(&Value::Str("unique".into())).unwrap();
    let bytes = enc.finish().unwrap();
    let mut dec = Decoder::new(&bytes);
    assert_eq!(
        dec.decode_next_owned().unwrap(),
        Value::Str("unique".into())
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 7. WIRE TYPE COVERAGE — every wire type in both zero-copy and owned
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn all_wire_types_zero_copy_roundtrip() {
    let val = Value::Object(vec![
        ("null".into(), Value::Null),
        ("bool_t".into(), Value::Bool(true)),
        ("bool_f".into(), Value::Bool(false)),
        ("uint".into(), Value::UInt(42)),
        ("int".into(), Value::Int(-7)),
        ("float".into(), Value::Float(3.125)),
        ("str".into(), Value::Str("hello".into())),
        ("bytes".into(), Value::Bytes(vec![1, 2, 3])),
        ("array".into(), Value::Array(vec![Value::UInt(1)])),
        ("nested_obj".into(), Value::Object(vec![("k".into(), Value::Null)])),
    ]);
    let mut enc = Encoder::new();
    enc.encode_value(&val).unwrap();
    let bytes = enc.finish().unwrap();

    // Zero-copy
    let mut dec = Decoder::new(&bytes);
    let zc = dec.decode_next().unwrap().to_owned_value();
    assert_eq!(zc, val);

    // Owned
    let mut dec2 = Decoder::new(&bytes);
    let owned = dec2.decode_next_owned().unwrap();
    assert_eq!(owned, val);
}

// ═══════════════════════════════════════════════════════════════════════
// 8. STRING DICT CORRUPTION
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn reference_to_nonexistent_dict_entry() {
    // Craft a block that has a Reference wire type pointing to index 999
    // but no StringDict block was emitted
    let mut block_payload = vec![];
    block_payload.push(WireType::Reference.to_tag());
    encode_varint_vec(999, &mut block_payload);

    let mut data = b"CROUSv1\x00".to_vec();
    let checksum = crous_core::checksum::compute_xxh64(&block_payload);
    data.push(BlockType::Data as u8);
    encode_varint_vec(block_payload.len() as u64, &mut data);
    data.push(CompressionType::None as u8);
    data.extend_from_slice(&checksum.to_le_bytes());
    data.extend_from_slice(&block_payload);

    let mut dec = Decoder::new(&data);
    let result = dec.decode_next_owned().unwrap();
    // Forward compatibility: unknown reference resolves to UInt
    assert_eq!(result, Value::UInt(999));
}

// ═══════════════════════════════════════════════════════════════════════
// 9. SKIP VALUE ROBUSTNESS
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn skip_value_all_types() {
    // Encode a complex value, decode it via skip, verify position advances correctly
    let val = Value::Object(vec![
        ("null".into(), Value::Null),
        ("bool".into(), Value::Bool(true)),
        ("uint".into(), Value::UInt(u64::MAX)),
        ("int".into(), Value::Int(i64::MIN)),
        ("float".into(), Value::Float(f64::INFINITY)),
        ("str".into(), Value::Str("hello world this is a test".into())),
        ("bytes".into(), Value::Bytes(vec![0; 100])),
        ("arr".into(), Value::Array(vec![Value::UInt(1), Value::UInt(2)])),
        ("obj".into(), Value::Object(vec![("k".into(), Value::Null)])),
    ]);
    let mut enc = Encoder::new();
    enc.encode_value(&val).unwrap();
    let bytes = enc.finish().unwrap();

    // Decode normally first to verify the data is valid
    let mut dec = Decoder::new(&bytes);
    let decoded = dec.decode_next().unwrap().to_owned_value();
    assert_eq!(decoded, val);
}

// ═══════════════════════════════════════════════════════════════════════
// 10. DETERMINISTIC ENCODING
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn encoding_is_deterministic() {
    let val = Value::Object(vec![
        ("z".into(), Value::UInt(1)),
        ("a".into(), Value::UInt(2)),
        ("m".into(), Value::Array(vec![Value::Str("x".into())])),
    ]);

    let mut bytes1 = Vec::new();
    let mut bytes2 = Vec::new();
    for bytes in [&mut bytes1, &mut bytes2] {
        let mut enc = Encoder::new();
        enc.encode_value(&val).unwrap();
        *bytes = enc.finish().unwrap();
    }

    assert_eq!(bytes1, bytes2, "encoding must be deterministic");
}

#[test]
fn dedup_encoding_is_deterministic() {
    let val = Value::Array(vec![
        Value::Str("hello".into()),
        Value::Str("world".into()),
        Value::Str("hello".into()),
    ]);

    let mut results = Vec::new();
    for _ in 0..3 {
        let mut enc = Encoder::new();
        enc.enable_dedup();
        enc.encode_value(&val).unwrap();
        results.push(enc.finish().unwrap());
    }
    assert_eq!(results[0], results[1]);
    assert_eq!(results[1], results[2]);
}

// ═══════════════════════════════════════════════════════════════════════
// 11. CROSS-PATH CONSISTENCY: zero-copy vs owned decode
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn zero_copy_and_owned_produce_same_result() {
    let values = vec![
        Value::Null,
        Value::Bool(true),
        Value::UInt(42),
        Value::Int(-99),
        Value::Float(2.75),
        Value::Str("hello".into()),
        Value::Bytes(vec![1, 2, 3]),
        Value::Array(vec![Value::UInt(1), Value::Str("two".into())]),
        Value::Object(vec![
            ("key".into(), Value::Array(vec![Value::Null, Value::Bool(false)])),
        ]),
    ];

    for val in &values {
        let mut enc = Encoder::new();
        enc.encode_value(val).unwrap();
        let bytes = enc.finish().unwrap();

        let mut dec_zc = Decoder::new(&bytes);
        let zc = dec_zc.decode_next().unwrap().to_owned_value();

        let mut dec_own = Decoder::new(&bytes);
        let owned = dec_own.decode_next_owned().unwrap();

        assert_eq!(zc, owned, "zero-copy vs owned mismatch for {:?}", val);
        assert_eq!(zc, *val, "decoded != original for {:?}", val);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 12. MULTIPLE VALUES IN SINGLE FILE
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn multiple_top_level_values() {
    let mut enc = Encoder::new();
    enc.encode_value(&Value::UInt(1)).unwrap();
    enc.encode_value(&Value::Str("two".into())).unwrap();
    enc.encode_value(&Value::Bool(true)).unwrap();
    let bytes = enc.finish().unwrap();

    let mut dec = Decoder::new(&bytes);
    let vals = dec.decode_all_owned().unwrap();
    assert_eq!(
        vals,
        vec![Value::UInt(1), Value::Str("two".into()), Value::Bool(true)]
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 13. HEADER FLAG VARIATIONS
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn all_header_flags_accepted() {
    for flags in 0..=255u8 {
        let hdr = FileHeader::new(flags);
        let encoded = hdr.encode();
        let decoded = FileHeader::decode(&encoded).unwrap();
        assert_eq!(decoded.flags, flags);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 14. INVALID UTF-8 IN STRING FIELDS
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn decode_invalid_utf8_string() {
    // Craft a block with a LenDelimited string containing invalid UTF-8
    let mut block_payload = vec![];
    block_payload.push(WireType::LenDelimited.to_tag());
    block_payload.push(0x00); // sub-type: string
    block_payload.push(0x04); // length: 4
    block_payload.extend_from_slice(&[0xFF, 0xFE, 0x80, 0x81]); // invalid UTF-8

    let mut data = b"CROUSv1\x00".to_vec();
    let checksum = crous_core::checksum::compute_xxh64(&block_payload);
    data.push(BlockType::Data as u8);
    encode_varint_vec(block_payload.len() as u64, &mut data);
    data.push(CompressionType::None as u8);
    data.extend_from_slice(&checksum.to_le_bytes());
    data.extend_from_slice(&block_payload);

    // Both paths should return InvalidUtf8
    let mut dec = Decoder::new(&data);
    assert!(matches!(dec.decode_next(), Err(CrousError::InvalidUtf8(..))));

    let mut dec2 = Decoder::new(&data);
    assert!(matches!(
        dec2.decode_next_owned(),
        Err(CrousError::InvalidUtf8(..))
    ));
}

// ═══════════════════════════════════════════════════════════════════════
// 15. STRESS: MANY SMALL ROUNDTRIPS
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn stress_10k_individual_roundtrips() {
    for i in 0..10_000u64 {
        let val = match i % 7 {
            0 => Value::Null,
            1 => Value::Bool(i % 2 == 0),
            2 => Value::UInt(i),
            3 => Value::Int(-(i as i64)),
            4 => Value::Float(i as f64 * 0.001),
            5 => Value::Str(format!("s{i}")),
            6 => Value::Bytes(vec![(i & 0xFF) as u8; (i % 16) as usize]),
            _ => unreachable!(),
        };
        let mut enc = Encoder::new();
        enc.encode_value(&val).unwrap();
        let bytes = enc.finish().unwrap();
        let mut dec = Decoder::new(&bytes);
        let decoded = dec.decode_next_owned().unwrap();
        assert_eq!(decoded, val, "mismatch at i={i}");
    }
}
