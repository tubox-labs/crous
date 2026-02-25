//! Stress, concurrency, and soak tests for crous-core and crous-io.
//!
//! These tests validate the crate under conditions mimicking:
//! - Concurrent encode/decode from multiple threads
//! - Sustained high-throughput operations
//! - Large payload handling
//! - Async IO interleaving
//! - Send/Sync safety
//!
//! Run with: cargo test -p crous-core --test stress -- --ignored
//! (some tests are slow and marked #[ignore])

use crous_core::decoder::Decoder;
use crous_core::encoder::Encoder;
use crous_core::limits::Limits;
use crous_core::value::Value;

// ═══════════════════════════════════════════════════════════════════════
// COMPILE-TIME ASSERTIONS: Send + Sync
// ═══════════════════════════════════════════════════════════════════════

fn assert_send<T: Send>() {}
fn assert_sync<T: Sync>() {}

#[test]
fn encoder_is_send() {
    assert_send::<Encoder>();
}

// Decoder borrows data so it's !Send across threads, but we verify
// that the owned Value and CrousValue types are fine:
#[test]
fn value_is_send_sync() {
    assert_send::<Value>();
    assert_sync::<Value>();
}

#[test]
fn limits_is_send_sync() {
    assert_send::<Limits>();
    assert_sync::<Limits>();
}

// ═══════════════════════════════════════════════════════════════════════
// CONCURRENT ROUNDTRIPS
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn concurrent_encode_decode_100_threads() {
    use std::thread;

    let handles: Vec<_> = (0..100)
        .map(|i| {
            thread::spawn(move || {
                let val = Value::Object(vec![
                    ("id".into(), Value::UInt(i)),
                    ("name".into(), Value::Str(format!("thread-{i}"))),
                    ("data".into(), Value::Array(vec![
                        Value::UInt(i * 10),
                        Value::Float(i as f64 * 0.1),
                        Value::Bool(i % 2 == 0),
                    ])),
                ]);

                for _ in 0..100 {
                    let mut enc = Encoder::new();
                    enc.encode_value(&val).unwrap();
                    let bytes = enc.finish().unwrap();

                    let mut dec = Decoder::new(&bytes);
                    let decoded = dec.decode_next_owned().unwrap();
                    assert_eq!(decoded, val, "thread {i} mismatch");
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("thread panicked");
    }
}

#[test]
fn concurrent_dedup_encode_decode() {
    use std::thread;

    let handles: Vec<_> = (0..50)
        .map(|i| {
            thread::spawn(move || {
                let items: Vec<Value> = (0..20)
                    .map(|j| {
                        if j % 3 == 0 {
                            Value::Str(format!("shared-{}", i % 5))
                        } else {
                            Value::Str(format!("unique-{i}-{j}"))
                        }
                    })
                    .collect();
                let val = Value::Array(items.clone());

                let mut enc = Encoder::new();
                enc.enable_dedup();
                enc.encode_value(&val).unwrap();
                let bytes = enc.finish().unwrap();

                let mut dec = Decoder::new(&bytes);
                let decoded = dec.decode_next_owned().unwrap();
                assert_eq!(decoded, val, "dedup thread {i} mismatch");
            })
        })
        .collect();

    for h in handles {
        h.join().expect("dedup thread panicked");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SHARED DATA DECODE (data encoded once, decoded from many threads)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn shared_bytes_decoded_by_many_threads() {
    use std::sync::Arc;
    use std::thread;

    let val = Value::Object(vec![
        ("x".into(), Value::UInt(42)),
        ("y".into(), Value::Str("hello world".into())),
        ("z".into(), Value::Array(vec![Value::Bool(true); 10])),
    ]);

    let mut enc = Encoder::new();
    enc.encode_value(&val).unwrap();
    let bytes = Arc::new(enc.finish().unwrap());
    let val = Arc::new(val);

    let handles: Vec<_> = (0..200)
        .map(|_| {
            let bytes = Arc::clone(&bytes);
            let val = Arc::clone(&val);
            thread::spawn(move || {
                let mut dec = Decoder::new(&bytes);
                let decoded = dec.decode_next_owned().unwrap();
                assert_eq!(decoded, *val);
            })
        })
        .collect();

    for h in handles {
        h.join().expect("shared decode panicked");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SUSTAINED THROUGHPUT
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn sustained_throughput_100k_encodes() {
    let val = Value::Object(vec![
        ("id".into(), Value::UInt(12345)),
        ("msg".into(), Value::Str("test message".into())),
        ("ok".into(), Value::Bool(true)),
    ]);

    for _ in 0..100_000 {
        let mut enc = Encoder::new();
        enc.encode_value(&val).unwrap();
        let bytes = enc.finish().unwrap();
        assert!(bytes.len() > 8);
    }
}

#[test]
fn sustained_throughput_100k_decodes() {
    let val = Value::Object(vec![
        ("id".into(), Value::UInt(12345)),
        ("msg".into(), Value::Str("test message".into())),
        ("ok".into(), Value::Bool(true)),
    ]);

    let mut enc = Encoder::new();
    enc.encode_value(&val).unwrap();
    let bytes = enc.finish().unwrap();

    for _ in 0..100_000 {
        let mut dec = Decoder::new(&bytes);
        let decoded = dec.decode_next_owned().unwrap();
        assert_eq!(decoded, val);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// LARGE PAYLOADS
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn large_array_10k_items() {
    let items: Vec<Value> = (0..10_000)
        .map(|i| Value::Str(format!("item-{i:06}")))
        .collect();
    let val = Value::Array(items);

    let mut enc = Encoder::new();
    enc.encode_value(&val).unwrap();
    let bytes = enc.finish().unwrap();

    let mut dec = Decoder::new(&bytes);
    let decoded = dec.decode_next_owned().unwrap();
    assert_eq!(decoded, val);
}

#[test]
fn large_object_1k_fields() {
    let entries: Vec<(String, Value)> = (0..1000)
        .map(|i| (format!("field_{i:04}"), Value::UInt(i)))
        .collect();
    let val = Value::Object(entries);

    let mut enc = Encoder::new();
    enc.encode_value(&val).unwrap();
    let bytes = enc.finish().unwrap();

    let mut dec = Decoder::new(&bytes);
    let decoded = dec.decode_next_owned().unwrap();
    assert_eq!(decoded, val);
}

#[test]
fn large_string_1mb() {
    let s = "A".repeat(1024 * 1024);
    let val = Value::Str(s);

    let mut enc = Encoder::new();
    enc.encode_value(&val).unwrap();
    let bytes = enc.finish().unwrap();

    let mut dec = Decoder::new(&bytes);
    let decoded = dec.decode_next_owned().unwrap();
    assert_eq!(decoded, val);
}

#[test]
fn large_binary_5mb() {
    let blob = vec![0xCD; 5 * 1024 * 1024];
    let val = Value::Bytes(blob);

    let mut enc = Encoder::new();
    enc.encode_value(&val).unwrap();
    let bytes = enc.finish().unwrap();

    let mut dec = Decoder::new(&bytes);
    let decoded = dec.decode_next_owned().unwrap();
    assert_eq!(decoded, val);
}

// ═══════════════════════════════════════════════════════════════════════
// MIXED WORKLOADS (realistic patterns)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn mixed_payload_sizes() {
    // Simulate a realistic stream: tiny, small, medium, large values
    let values = vec![
        Value::Null,                                           // 1 byte
        Value::Bool(true),                                     // 2 bytes
        Value::UInt(42),                                       // 2 bytes
        Value::Str("short".into()),                            // ~8 bytes
        Value::Str("a".repeat(1000)),                          // ~1 KB
        Value::Bytes(vec![0xAB; 10_000]),                      // 10 KB
        Value::Array(vec![Value::UInt(1); 1000]),               // ~2 KB
        Value::Object(                                         // ~5 KB
            (0..100)
                .map(|i| (format!("k{i}"), Value::Str(format!("v{i}"))))
                .collect(),
        ),
    ];

    let mut enc = Encoder::new();
    for v in &values {
        enc.encode_value(v).unwrap();
    }
    let bytes = enc.finish().unwrap();

    let mut dec = Decoder::new(&bytes);
    let decoded = dec.decode_all_owned().unwrap();
    assert_eq!(decoded.len(), values.len());
    for (original, decoded) in values.iter().zip(decoded.iter()) {
        assert_eq!(decoded, original);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SOAK TEST (long-running, marked #[ignore])
// ═══════════════════════════════════════════════════════════════════════

#[test]
#[ignore] // Run with: cargo test -p crous-core --test stress soak -- --ignored
fn soak_1_million_roundtrips() {
    for i in 0u64..1_000_000 {
        let val = match i % 9 {
            0 => Value::Null,
            1 => Value::Bool(i % 2 == 0),
            2 => Value::UInt(i),
            3 => Value::Int(-(i as i64)),
            4 => Value::Float(i as f64 * 0.001),
            5 => Value::Str(format!("soak-{i}")),
            6 => Value::Bytes(vec![(i & 0xFF) as u8; 4]),
            7 => Value::Array(vec![Value::UInt(i), Value::Null]),
            8 => Value::Object(vec![("k".into(), Value::UInt(i))]),
            _ => unreachable!(),
        };

        let mut enc = Encoder::new();
        enc.encode_value(&val).unwrap();
        let bytes = enc.finish().unwrap();

        let mut dec = Decoder::new(&bytes);
        let decoded = dec.decode_next_owned().unwrap();
        assert_eq!(decoded, val, "soak mismatch at iteration {i}");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// DEDUP STRESS
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn dedup_with_many_unique_strings() {
    // Edge case: dedup enabled but all strings are unique → no savings, but must work
    let items: Vec<Value> = (0..1000)
        .map(|i| Value::Str(format!("unique_string_number_{i:06}")))
        .collect();
    let val = Value::Array(items);

    let mut enc = Encoder::new();
    enc.enable_dedup();
    enc.encode_value(&val).unwrap();
    let bytes = enc.finish().unwrap();

    let mut dec = Decoder::new(&bytes);
    let decoded = dec.decode_next_owned().unwrap();
    assert_eq!(decoded, val);
}

#[test]
fn dedup_with_all_identical_strings() {
    // Edge case: every string is the same
    let items: Vec<Value> = (0..500)
        .map(|_| Value::Str("identical".into()))
        .collect();
    let val = Value::Array(items);

    let mut enc_dedup = Encoder::new();
    enc_dedup.enable_dedup();
    enc_dedup.encode_value(&val).unwrap();
    let bytes_dedup = enc_dedup.finish().unwrap();

    let mut enc_plain = Encoder::new();
    enc_plain.encode_value(&val).unwrap();
    let bytes_plain = enc_plain.finish().unwrap();

    // Dedup should be significantly smaller
    assert!(
        bytes_dedup.len() < bytes_plain.len() / 2,
        "dedup={} plain={}",
        bytes_dedup.len(),
        bytes_plain.len()
    );

    let mut dec = Decoder::new(&bytes_dedup);
    let decoded = dec.decode_next_owned().unwrap();
    assert_eq!(decoded, val);
}

#[test]
fn dedup_empty_strings() {
    // Edge case: dedup with empty strings
    let items: Vec<Value> = (0..10)
        .map(|_| Value::Str(String::new()))
        .collect();
    let val = Value::Array(items);

    let mut enc = Encoder::new();
    enc.enable_dedup();
    enc.encode_value(&val).unwrap();
    let bytes = enc.finish().unwrap();

    let mut dec = Decoder::new(&bytes);
    let decoded = dec.decode_next_owned().unwrap();
    assert_eq!(decoded, val);
}
