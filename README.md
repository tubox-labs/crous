# Crous

**A compact, canonical binary serializer and human-readable alternative to JSON, written in Rust.**

[![CI](https://github.com/crous-format/crous/actions/workflows/ci.yml/badge.svg)](https://github.com/crous-format/crous/actions)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

## Overview

Crous is a production-grade binary serialization format that provides:

- **Compact binary encoding** — 2×–5× smaller than equivalent JSON
- **Human-readable text notation** — unique syntax with deterministic binary mapping
- **Zero-copy decoding** — borrow directly from input buffers
- **Schema evolution** — stable field IDs, unknown-field skipping
- **Streaming support** — block-framed format with per-block checksums
- **Pluggable compression** — zstd/snappy per-block, with plugin trait
- **Cross-language FFI** — clean C bindings with documented ownership

## Quick Start

```rust
use crous_core::{Encoder, Decoder, Value};

// Encode
let value = Value::Object(vec![
    ("name".into(), Value::Str("Alice".into())),
    ("age".into(), Value::UInt(30)),
    ("active".into(), Value::Bool(true)),
]);

let mut encoder = Encoder::new();
encoder.encode_value(&value).unwrap();
let bytes = encoder.finish().unwrap();

// Decode (zero-copy)
let mut decoder = Decoder::new(&bytes);
let decoded = decoder.decode_next().unwrap();
assert_eq!(decoded.to_owned_value(), value);
```

## Human-Readable Syntax

```crous
{
    name: "Alice";
    age: 30;
    tags: ["admin", "user"];
    config: {
        theme: "dark";
        notifications: true;
    };
    avatar: b64#iVBORw0KGgo=;
}
```

## Workspace Crates

| Crate | Description |
|-------|-------------|
| `crous-core` | Encoder/decoder, block framing, Value types |
| `crous-derive` | `#[derive(Crous)]` proc-macro with stable field IDs |
| `crous-io` | Async Tokio adapters, framed streams |
| `crous-cli` | CLI: inspect, pretty-print, convert |
| `crous-compression` | Pluggable zstd/snappy compression |
| `crous-ffi` | C FFI bindings |
| `crous-simd` | NEON/SSE2 SIMD acceleration (byte scanning) |

## Python Implementation

A pure-Python implementation is included in `python/crous/`, providing
full encode/decode compatibility with the Rust implementation:

```python
import crous

# Encode
data = crous.encode({"name": "Alice", "age": 30, "active": True})

# Decode
obj = crous.decode(data)
print(obj)  # {'name': 'Alice', 'age': 30, 'active': True}

# Human-readable text format
text = crous.pretty_print(crous.Value.from_python(obj))
print(text)
```

Cross-language interop verified: Rust-encoded files decode correctly in
Python and vice versa.

```bash
# Run Python tests
cd python && python -m pytest tests/ -v
```

## CLI Usage

```bash
# Install
cargo install --path crous-cli

# Convert JSON to Crous
crous from-json data.json -o data.crous

# Pretty-print a Crous file
crous pretty data.crous

# Inspect block layout
crous inspect data.crous

# Convert back to JSON
crous to-json data.crous

# Quick benchmark
crous bench data.json -n 10000
```

## Derive Macro

```rust
use crous_derive::{Crous, CrousSchema};

#[derive(Debug, PartialEq, Crous, CrousSchema)]
struct Person {
    #[crous(id = 1)] name: String,
    #[crous(id = 2)] age: u8,
    #[crous(id = 3)] tags: Vec<String>,
}

let alice = Person {
    name: "Alice".into(),
    age: 30,
    tags: vec!["admin".into()],
};

// Encode
let value = alice.to_crous_value();
let mut encoder = Encoder::new();
encoder.encode_value(&value).unwrap();
let bytes = encoder.finish().unwrap();
```

## Binary Format Summary

```
File:    [Header 8B] [Block]* [Trailer Block]
Header:  "CROUSv1" (7B) | flags (1B)
Block:   type(1B) | length(varint) | compression(1B) | checksum(8B) | payload
```

Wire types: Null, Bool, VarUInt (LEB128), VarInt (ZigZag+LEB128), Fixed64, LenDelimited, StartObject, EndObject, StartArray, EndArray, Reference.

## Build & Test

```bash
cargo build --workspace
cargo test --workspace --all-features
cargo clippy --workspace --all-features
cargo bench -p crous-core
```

## Fuzzing

```bash
cargo +nightly fuzz run fuzz_decode     # arbitrary byte decoder
cargo +nightly fuzz run fuzz_roundtrip  # structured Value roundtrip
cargo +nightly fuzz run fuzz_text       # text parser
cargo +nightly fuzz run fuzz_varint     # varint codec
```

## Performance

Crous vs JSON (serde_json) on Apple Silicon (M-series):

| Payload | Crous size | JSON size | Ratio | Decode speed |
|---------|-----------|-----------|-------|--------------|
| Small object | 118 B | 90 B | 1.31× | **4× faster** |
| 100 users nested | 9.8 KB | 10.3 KB | 0.95× | ~2× faster |
| 10K integers | 29.9 KB | 52.8 KB | **0.57×** | ~3× faster |
| 64 KB binary | 65.6 KB | 87.4 KB | **0.75×** | ~10× faster |

Crous decode throughput: **1.2 GiB/s** for small objects.

## License

Licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
