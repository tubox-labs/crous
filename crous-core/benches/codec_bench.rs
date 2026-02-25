//! Criterion benchmarks for crous-core vs serde_json.
//!
//! Run with: `cargo bench -p crous-core`
//!
//! Compares Crous encode/decode throughput against serde_json for
//! the same payloads, measuring both operations/sec and bytes/sec.

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use crous_core::{Decoder, Encoder, Value};

// ── Payload constructors ────────────────────────────────────────────

/// Build a small object (typical API response).
fn make_small_object() -> Value {
    Value::Object(vec![
        ("id".into(), Value::UInt(12345)),
        ("name".into(), Value::Str("Alice Johnson".into())),
        ("email".into(), Value::Str("alice@example.com".into())),
        ("active".into(), Value::Bool(true)),
        ("score".into(), Value::Float(98.5)),
    ])
}

/// Build a large nested document (100 users with tags).
fn make_large_nested() -> Value {
    let mut users = Vec::new();
    for i in 0..100 {
        users.push(Value::Object(vec![
            ("id".into(), Value::UInt(i)),
            ("name".into(), Value::Str(format!("User {i}"))),
            ("email".into(), Value::Str(format!("user{i}@example.com"))),
            (
                "tags".into(),
                Value::Array(vec![
                    Value::Str("admin".into()),
                    Value::Str("user".into()),
                    Value::Str(format!("group-{}", i % 10)),
                ]),
            ),
            ("score".into(), Value::Float(i as f64 * 1.5)),
        ]));
    }
    Value::Object(vec![
        ("users".into(), Value::Array(users)),
        ("total".into(), Value::UInt(100)),
        ("page".into(), Value::UInt(1)),
    ])
}

/// Build a payload with many small strings (1000 items).
fn make_many_strings() -> Value {
    let items: Vec<Value> = (0..1000)
        .map(|i| Value::Str(format!("item-{i:04}")))
        .collect();
    Value::Array(items)
}

/// Build a binary blob payload (64 KB).
fn make_binary_blob() -> Value {
    let blob = vec![0xABu8; 64 * 1024];
    Value::Object(vec![
        ("type".into(), Value::Str("binary".into())),
        ("data".into(), Value::Bytes(blob)),
    ])
}

/// Build a deeply nested structure (50 levels).
fn make_deep_nesting() -> Value {
    let mut v = Value::UInt(42);
    for i in 0..50 {
        v = Value::Object(vec![(format!("level_{i}"), v)]);
    }
    v
}

/// Build a numeric-heavy payload (10K integers).
fn make_numeric_array() -> Value {
    let items: Vec<Value> = (0..10_000).map(|i| Value::Int(i - 5000)).collect();
    Value::Array(items)
}

// ── Head-to-head: Crous vs JSON ─────────────────────────────────────

fn bench_encode_small(c: &mut Criterion) {
    let obj = make_small_object();
    let json_val: serde_json::Value = (&obj).into();
    let json_str = serde_json::to_string(&json_val).unwrap();

    let mut group = c.benchmark_group("encode_small");
    group.throughput(Throughput::Bytes(json_str.len() as u64));

    group.bench_function("crous", |b| {
        b.iter(|| {
            let mut enc = Encoder::new();
            enc.encode_value(black_box(&obj)).unwrap();
            let _ = enc.finish().unwrap();
        });
    });

    group.bench_function("json", |b| {
        b.iter(|| {
            let _ = serde_json::to_vec(black_box(&json_val)).unwrap();
        });
    });

    group.finish();
}

fn bench_decode_small(c: &mut Criterion) {
    let obj = make_small_object();
    let mut enc = Encoder::new();
    enc.encode_value(&obj).unwrap();
    let crous_bytes = enc.finish().unwrap();

    let json_val: serde_json::Value = (&obj).into();
    let json_bytes = serde_json::to_vec(&json_val).unwrap();

    let mut group = c.benchmark_group("decode_small");

    group.throughput(Throughput::Bytes(crous_bytes.len() as u64));
    group.bench_function("crous", |b| {
        b.iter(|| {
            let mut dec = Decoder::new(black_box(&crous_bytes));
            let _ = dec.decode_next().unwrap();
        });
    });

    group.throughput(Throughput::Bytes(json_bytes.len() as u64));
    group.bench_function("json", |b| {
        b.iter(|| {
            let _: serde_json::Value =
                serde_json::from_slice(black_box(&json_bytes)).unwrap();
        });
    });

    group.finish();
}

fn bench_encode_large(c: &mut Criterion) {
    let obj = make_large_nested();
    let json_val: serde_json::Value = (&obj).into();
    let json_str = serde_json::to_string(&json_val).unwrap();

    let mut group = c.benchmark_group("encode_large_nested");
    group.throughput(Throughput::Bytes(json_str.len() as u64));

    group.bench_function("crous", |b| {
        b.iter(|| {
            let mut enc = Encoder::new();
            enc.encode_value(black_box(&obj)).unwrap();
            let _ = enc.finish().unwrap();
        });
    });

    group.bench_function("json", |b| {
        b.iter(|| {
            let _ = serde_json::to_vec(black_box(&json_val)).unwrap();
        });
    });

    group.finish();
}

fn bench_decode_large(c: &mut Criterion) {
    let obj = make_large_nested();
    let mut enc = Encoder::new();
    enc.encode_value(&obj).unwrap();
    let crous_bytes = enc.finish().unwrap();

    let json_val: serde_json::Value = (&obj).into();
    let json_bytes = serde_json::to_vec(&json_val).unwrap();

    let mut group = c.benchmark_group("decode_large_nested");

    group.throughput(Throughput::Bytes(crous_bytes.len() as u64));
    group.bench_function("crous", |b| {
        b.iter(|| {
            let mut dec = Decoder::new(black_box(&crous_bytes));
            let _ = dec.decode_next().unwrap();
        });
    });

    group.throughput(Throughput::Bytes(json_bytes.len() as u64));
    group.bench_function("json", |b| {
        b.iter(|| {
            let _: serde_json::Value =
                serde_json::from_slice(black_box(&json_bytes)).unwrap();
        });
    });

    group.finish();
}

fn bench_strings(c: &mut Criterion) {
    let obj = make_many_strings();
    let mut enc = Encoder::new();
    enc.encode_value(&obj).unwrap();
    let crous_bytes = enc.finish().unwrap();

    let json_val: serde_json::Value = (&obj).into();
    let json_bytes = serde_json::to_vec(&json_val).unwrap();

    let mut group = c.benchmark_group("many_strings");

    group.throughput(Throughput::Bytes(crous_bytes.len() as u64));
    group.bench_function("crous_encode", |b| {
        b.iter(|| {
            let mut enc = Encoder::new();
            enc.encode_value(black_box(&obj)).unwrap();
            let _ = enc.finish().unwrap();
        });
    });
    group.bench_function("crous_decode", |b| {
        b.iter(|| {
            let mut dec = Decoder::new(black_box(&crous_bytes));
            let _ = dec.decode_next().unwrap();
        });
    });

    group.throughput(Throughput::Bytes(json_bytes.len() as u64));
    group.bench_function("json_encode", |b| {
        b.iter(|| {
            let _ = serde_json::to_vec(black_box(&json_val)).unwrap();
        });
    });
    group.bench_function("json_decode", |b| {
        b.iter(|| {
            let _: serde_json::Value =
                serde_json::from_slice(black_box(&json_bytes)).unwrap();
        });
    });

    group.finish();
}

fn bench_binary_blob(c: &mut Criterion) {
    let obj = make_binary_blob();
    let mut enc = Encoder::new();
    enc.encode_value(&obj).unwrap();
    let bytes = enc.finish().unwrap();

    let mut group = c.benchmark_group("binary_blob");
    group.throughput(Throughput::Bytes(bytes.len() as u64));

    group.bench_function("crous_encode", |b| {
        b.iter(|| {
            let mut enc = Encoder::new();
            enc.encode_value(black_box(&obj)).unwrap();
            let _ = enc.finish().unwrap();
        });
    });
    group.bench_function("crous_decode", |b| {
        b.iter(|| {
            let mut dec = Decoder::new(black_box(&bytes));
            let _ = dec.decode_next().unwrap();
        });
    });

    group.finish();
}

fn bench_deep_nesting(c: &mut Criterion) {
    let obj = make_deep_nesting();
    let mut enc = Encoder::new();
    enc.encode_value(&obj).unwrap();
    let crous_bytes = enc.finish().unwrap();

    let json_val: serde_json::Value = (&obj).into();
    let json_bytes = serde_json::to_vec(&json_val).unwrap();

    let mut group = c.benchmark_group("deep_nesting");

    group.throughput(Throughput::Bytes(crous_bytes.len() as u64));
    group.bench_function("crous_encode", |b| {
        b.iter(|| {
            let mut enc = Encoder::new();
            enc.encode_value(black_box(&obj)).unwrap();
            let _ = enc.finish().unwrap();
        });
    });
    group.bench_function("crous_decode", |b| {
        b.iter(|| {
            let mut dec = Decoder::new(black_box(&crous_bytes));
            let _ = dec.decode_next().unwrap();
        });
    });

    group.throughput(Throughput::Bytes(json_bytes.len() as u64));
    group.bench_function("json_encode", |b| {
        b.iter(|| {
            let _ = serde_json::to_vec(black_box(&json_val)).unwrap();
        });
    });
    group.bench_function("json_decode", |b| {
        b.iter(|| {
            let _: serde_json::Value =
                serde_json::from_slice(black_box(&json_bytes)).unwrap();
        });
    });

    group.finish();
}

fn bench_numeric_array(c: &mut Criterion) {
    let obj = make_numeric_array();
    let mut enc = Encoder::new();
    enc.encode_value(&obj).unwrap();
    let crous_bytes = enc.finish().unwrap();

    let json_val: serde_json::Value = (&obj).into();
    let json_bytes = serde_json::to_vec(&json_val).unwrap();

    let mut group = c.benchmark_group("numeric_array_10k");

    group.throughput(Throughput::Bytes(crous_bytes.len() as u64));
    group.bench_function("crous_encode", |b| {
        b.iter(|| {
            let mut enc = Encoder::new();
            enc.encode_value(black_box(&obj)).unwrap();
            let _ = enc.finish().unwrap();
        });
    });
    group.bench_function("crous_decode", |b| {
        b.iter(|| {
            let mut dec = Decoder::new(black_box(&crous_bytes));
            let _ = dec.decode_next().unwrap();
        });
    });

    group.throughput(Throughput::Bytes(json_bytes.len() as u64));
    group.bench_function("json_encode", |b| {
        b.iter(|| {
            let _ = serde_json::to_vec(black_box(&json_val)).unwrap();
        });
    });
    group.bench_function("json_decode", |b| {
        b.iter(|| {
            let _: serde_json::Value =
                serde_json::from_slice(black_box(&json_bytes)).unwrap();
        });
    });

    group.finish();
}

fn bench_size_comparison(c: &mut Criterion) {
    // This isn't a time benchmark — it prints size comparisons.
    let payloads: Vec<(&str, Value)> = vec![
        ("small_object", make_small_object()),
        ("large_nested", make_large_nested()),
        ("many_strings", make_many_strings()),
        ("binary_blob", make_binary_blob()),
        ("deep_nesting", make_deep_nesting()),
        ("numeric_10k", make_numeric_array()),
    ];

    let mut group = c.benchmark_group("size_report");
    group.sample_size(10);

    for (name, obj) in &payloads {
        let mut enc = Encoder::new();
        enc.encode_value(obj).unwrap();
        let crous_bytes = enc.finish().unwrap();

        let json_val: serde_json::Value = obj.into();
        let json_bytes = serde_json::to_vec(&json_val).unwrap();

        let ratio = crous_bytes.len() as f64 / json_bytes.len() as f64;
        eprintln!(
            "  {name:20} crous={:6} json={:6}  ratio={:.2}x",
            crous_bytes.len(),
            json_bytes.len(),
            ratio
        );

        group.throughput(Throughput::Bytes(json_bytes.len() as u64));
        group.bench_function(format!("{name}_crous_encode"), |b| {
            b.iter(|| {
                let mut enc = Encoder::new();
                enc.encode_value(black_box(obj)).unwrap();
                let _ = enc.finish().unwrap();
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_encode_small,
    bench_decode_small,
    bench_encode_large,
    bench_decode_large,
    bench_strings,
    bench_binary_blob,
    bench_deep_nesting,
    bench_numeric_array,
    bench_size_comparison,
);
criterion_main!(benches);
