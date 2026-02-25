//! Benchmark runner — executes measurements for each format × dataset × operation.

use std::time::Instant;

use crous_core::{Decoder, Encoder, Value};

use crate::datasets::Dataset;
use crate::metrics::Measurement;

/// Number of warm-up iterations before measuring.
const WARMUP: usize = 3;

/// Run all measurements for a single dataset across all formats.
pub fn run_dataset(dataset: &Dataset, iterations: usize) -> Vec<Measurement> {
    let mut results = Vec::new();

    // ── Crous ────────────────────────────────────────────────────────
    let crous_bytes = crous_encode_once(&dataset.value);
    results.push(bench_crous_encode(dataset, iterations, &crous_bytes));
    results.push(bench_crous_decode(dataset, iterations, &crous_bytes));
    results.push(bench_crous_roundtrip(dataset, iterations));

    // ── Crous + string dedup ─────────────────────────────────────────
    let dedup_bytes = crous_encode_dedup_once(&dataset.value);
    results.push(bench_crous_dedup_encode(dataset, iterations, &dedup_bytes));
    results.push(bench_crous_dedup_decode(dataset, iterations, &dedup_bytes));

    // ── JSON (serde_json) ────────────────────────────────────────────
    let json_val: serde_json::Value = (&dataset.value).into();
    let json_bytes = serde_json::to_vec(&json_val).unwrap();
    results.push(bench_json_encode(
        dataset,
        iterations,
        &json_val,
        &json_bytes,
    ));
    results.push(bench_json_decode(dataset, iterations, &json_bytes));

    // ── MessagePack (rmp-serde) ──────────────────────────────────────
    let msgpack_bytes = rmp_serde::to_vec(&json_val).unwrap();
    results.push(bench_msgpack_encode(
        dataset,
        iterations,
        &json_val,
        &msgpack_bytes,
    ));
    results.push(bench_msgpack_decode(dataset, iterations, &msgpack_bytes));

    // ── CBOR (ciborium) ──────────────────────────────────────────────
    let mut cbor_bytes = Vec::new();
    ciborium::into_writer(&json_val, &mut cbor_bytes).unwrap();
    results.push(bench_cbor_encode(
        dataset,
        iterations,
        &json_val,
        &cbor_bytes,
    ));
    results.push(bench_cbor_decode(dataset, iterations, &cbor_bytes));

    results
}

// ── Crous Helpers ───────────────────────────────────────────────────

fn crous_encode_once(value: &Value) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.encode_value(value).unwrap();
    enc.finish().unwrap()
}

fn crous_encode_dedup_once(value: &Value) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.enable_dedup();
    enc.encode_value(value).unwrap();
    enc.finish().unwrap()
}

fn bench_crous_encode(ds: &Dataset, iters: usize, encoded: &[u8]) -> Measurement {
    let mut m = Measurement::new("crous", ds.name, "encode");
    m.serialized_size = Some(encoded.len());

    // Warm up.
    for _ in 0..WARMUP {
        let mut enc = Encoder::new();
        enc.encode_value(&ds.value).unwrap();
        let _ = enc.finish().unwrap();
    }

    for _ in 0..iters {
        let start = Instant::now();
        let mut enc = Encoder::new();
        enc.encode_value(&ds.value).unwrap();
        let _ = enc.finish().unwrap();
        m.add_duration(start.elapsed());
    }
    m
}

fn bench_crous_decode(ds: &Dataset, iters: usize, encoded: &[u8]) -> Measurement {
    let mut m = Measurement::new("crous", ds.name, "decode");
    m.serialized_size = Some(encoded.len());

    for _ in 0..WARMUP {
        let mut dec = Decoder::new(encoded);
        let _ = dec.decode_next().unwrap();
    }

    for _ in 0..iters {
        let start = Instant::now();
        let mut dec = Decoder::new(encoded);
        let _ = dec.decode_next().unwrap();
        m.add_duration(start.elapsed());
    }
    m
}

fn bench_crous_roundtrip(ds: &Dataset, iters: usize) -> Measurement {
    let mut m = Measurement::new("crous", ds.name, "roundtrip");

    for _ in 0..WARMUP {
        let mut enc = Encoder::new();
        enc.encode_value(&ds.value).unwrap();
        let bytes = enc.finish().unwrap();
        let mut dec = Decoder::new(&bytes);
        let _ = dec.decode_next().unwrap();
    }

    for _ in 0..iters {
        let start = Instant::now();
        let mut enc = Encoder::new();
        enc.encode_value(&ds.value).unwrap();
        let bytes = enc.finish().unwrap();
        let mut dec = Decoder::new(&bytes);
        let _ = dec.decode_next().unwrap();
        m.add_duration(start.elapsed());
    }
    m
}

fn bench_crous_dedup_encode(ds: &Dataset, iters: usize, encoded: &[u8]) -> Measurement {
    let mut m = Measurement::new("crous_dedup", ds.name, "encode");
    m.serialized_size = Some(encoded.len());

    for _ in 0..WARMUP {
        let mut enc = Encoder::new();
        enc.enable_dedup();
        enc.encode_value(&ds.value).unwrap();
        let _ = enc.finish().unwrap();
    }

    for _ in 0..iters {
        let start = Instant::now();
        let mut enc = Encoder::new();
        enc.enable_dedup();
        enc.encode_value(&ds.value).unwrap();
        let _ = enc.finish().unwrap();
        m.add_duration(start.elapsed());
    }
    m
}

fn bench_crous_dedup_decode(ds: &Dataset, iters: usize, encoded: &[u8]) -> Measurement {
    let mut m = Measurement::new("crous_dedup", ds.name, "decode");
    m.serialized_size = Some(encoded.len());

    for _ in 0..WARMUP {
        let mut dec = Decoder::new(encoded);
        let _ = dec.decode_next().unwrap();
    }

    for _ in 0..iters {
        let start = Instant::now();
        let mut dec = Decoder::new(encoded);
        let _ = dec.decode_next().unwrap();
        m.add_duration(start.elapsed());
    }
    m
}

// ── JSON ────────────────────────────────────────────────────────────

fn bench_json_encode(
    ds: &Dataset,
    iters: usize,
    json_val: &serde_json::Value,
    encoded: &[u8],
) -> Measurement {
    let mut m = Measurement::new("json", ds.name, "encode");
    m.serialized_size = Some(encoded.len());

    for _ in 0..WARMUP {
        let _ = serde_json::to_vec(json_val).unwrap();
    }

    for _ in 0..iters {
        let start = Instant::now();
        let _ = serde_json::to_vec(json_val).unwrap();
        m.add_duration(start.elapsed());
    }
    m
}

fn bench_json_decode(ds: &Dataset, iters: usize, encoded: &[u8]) -> Measurement {
    let mut m = Measurement::new("json", ds.name, "decode");
    m.serialized_size = Some(encoded.len());

    for _ in 0..WARMUP {
        let _: serde_json::Value = serde_json::from_slice(encoded).unwrap();
    }

    for _ in 0..iters {
        let start = Instant::now();
        let _: serde_json::Value = serde_json::from_slice(encoded).unwrap();
        m.add_duration(start.elapsed());
    }
    m
}

// ── MessagePack ─────────────────────────────────────────────────────

fn bench_msgpack_encode(
    ds: &Dataset,
    iters: usize,
    json_val: &serde_json::Value,
    encoded: &[u8],
) -> Measurement {
    let mut m = Measurement::new("msgpack", ds.name, "encode");
    m.serialized_size = Some(encoded.len());

    for _ in 0..WARMUP {
        let _ = rmp_serde::to_vec(json_val).unwrap();
    }

    for _ in 0..iters {
        let start = Instant::now();
        let _ = rmp_serde::to_vec(json_val).unwrap();
        m.add_duration(start.elapsed());
    }
    m
}

fn bench_msgpack_decode(ds: &Dataset, iters: usize, encoded: &[u8]) -> Measurement {
    let mut m = Measurement::new("msgpack", ds.name, "decode");
    m.serialized_size = Some(encoded.len());

    for _ in 0..WARMUP {
        let _: serde_json::Value = rmp_serde::from_slice(encoded).unwrap();
    }

    for _ in 0..iters {
        let start = Instant::now();
        let _: serde_json::Value = rmp_serde::from_slice(encoded).unwrap();
        m.add_duration(start.elapsed());
    }
    m
}

// ── CBOR ────────────────────────────────────────────────────────────

fn bench_cbor_encode(
    ds: &Dataset,
    iters: usize,
    json_val: &serde_json::Value,
    encoded: &[u8],
) -> Measurement {
    let mut m = Measurement::new("cbor", ds.name, "encode");
    m.serialized_size = Some(encoded.len());

    for _ in 0..WARMUP {
        let mut buf = Vec::new();
        ciborium::into_writer(json_val, &mut buf).unwrap();
    }

    for _ in 0..iters {
        let start = Instant::now();
        let mut buf = Vec::new();
        ciborium::into_writer(json_val, &mut buf).unwrap();
        m.add_duration(start.elapsed());
    }
    m
}

fn bench_cbor_decode(ds: &Dataset, iters: usize, encoded: &[u8]) -> Measurement {
    let mut m = Measurement::new("cbor", ds.name, "decode");
    m.serialized_size = Some(encoded.len());

    for _ in 0..WARMUP {
        let _: serde_json::Value = ciborium::from_reader(encoded).unwrap();
    }

    for _ in 0..iters {
        let start = Instant::now();
        let _: serde_json::Value = ciborium::from_reader(encoded).unwrap();
        m.add_duration(start.elapsed());
    }
    m
}
