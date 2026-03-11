# Crous — Rust API Documentation

> A compact, canonical binary serializer and human-readable alternative to JSON.

**Version:** 1.1.0 · **Edition:** 2024 · **MSRV:** 1.85.0 · **License:** MIT OR Apache-2.0

---

## Table of Contents

1. [Overview](#overview)
2. [Crate Map](#crate-map)
3. [Quick Start](#quick-start)
4. [Core Types](#core-types)
5. [Encoder](#encoder)
6. [Decoder](#decoder)
7. [Text Format](#text-format)
8. [Derive Macros](#derive-macros)
9. [Compression](#compression)
10. [IO & Streaming](#io--streaming)
11. [SIMD Acceleration](#simd-acceleration)
12. [FFI (C Bindings)](#ffi-c-bindings)
13. [Python Bindings](#python-bindings)
14. [Resource Limits](#resource-limits)
15. [Wire Format Summary](#wire-format-summary)
16. [Feature Flags](#feature-flags)
17. [Performance](#performance)
18. [Safety & Security](#safety--security)

---

## Overview

Crous is a binary serialization format designed for:

- **Compact output** — varint integers, no redundant delimiters, binary-native blobs
- **Canonical encoding** — deterministic byte output for the same logical data
- **Zero-copy decoding** — borrow strings/bytes directly from the input buffer
- **Block framing** — XXH64 checksummed blocks with optional compression
- **String deduplication** — prefix-delta StringDict blocks for repeated keys
- **Human-readable text** — a 1:1 text notation (`{ key: value; }`) for debugging

## Crate Map

| Crate | Purpose |
|-------|---------|
| `crous-core` | Encoder, Decoder, Value types, wire format, text parser |
| `crous-derive` | `#[derive(Crous)]` and `#[derive(CrousSchema)]` proc macros |
| `crous-compression` | Pluggable compression (zstd, lz4, snappy) |
| `crous-io` | Async IO (Tokio framed streams), mmap, Bytes API |
| `crous-simd` | SIMD-accelerated varint decode, byte scanning (NEON/SSE2) |
| `crous-ffi` | C-compatible FFI (`crous_encode_buffer`, `crous_decode_buffer`) |
| `crous-python` | PyO3 native Python extension |

## Quick Start

Add to `Cargo.toml`:

```toml
[dependencies]
crous-core = "1.1"
```

### Encode & Decode

```rust
use crous_core::{Encoder, Decoder, Value};

// Encode
let mut encoder = Encoder::new();
encoder.encode_value(&Value::Object(vec![
    ("name".into(), Value::Str("Alice".into())),
    ("age".into(), Value::UInt(30)),
])).unwrap();
let bytes = encoder.finish().unwrap();

// Decode (zero-copy)
let mut decoder = Decoder::new(&bytes);
let value = decoder.decode_next().unwrap();  // CrousValue<'_>
let owned = value.to_owned_value();          // Value (heap)
```

### With String Dedup

```rust
use crous_core::{Encoder, Decoder, Value};

let mut enc = Encoder::new();
enc.enable_dedup();
enc.encode_value(&Value::Array(vec![
    Value::Str("repeated_key".into()),
    Value::Str("repeated_key".into()),  // stored as Reference
])).unwrap();
let bytes = enc.finish().unwrap();
```

### With Compression (feature = `lz4`)

```rust
use crous_core::{Encoder, Decoder, Value};
use crous_core::wire::CompressionType;

let mut enc = Encoder::new();
enc.set_compression(CompressionType::Lz4);
enc.encode_value(&Value::Str("data...".repeat(100))).unwrap();
let bytes = enc.finish().unwrap();

// Compressed blocks require owned decode
let mut dec = Decoder::new(&bytes);
let values = dec.decode_all_owned().unwrap();
```

---

## Core Types

### `Value` (owned)

```rust
pub enum Value {
    Null,
    Bool(bool),
    UInt(u64),
    Int(i64),
    Float(f64),
    Str(String),
    Bytes(Vec<u8>),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}
```

Helper methods: `type_name()`, `is_null()`, `as_str()`, `as_uint()`, `as_int()`, `as_float()`, `as_bool()`, `as_array()`, `as_object()`.

Implements `Display` for human-readable output and bidirectional conversion with `serde_json::Value` via `From`.

### `CrousValue<'a>` (zero-copy)

```rust
pub enum CrousValue<'a> {
    Null,
    Bool(bool),
    UInt(u64),
    Int(i64),
    Float(f64),
    Str(&'a str),       // borrows from input
    Bytes(&'a [u8]),    // borrows from input
    Array(Vec<CrousValue<'a>>),
    Object(Vec<(&'a str, CrousValue<'a>)>),
}
```

Call `.to_owned_value()` to convert to `Value`.

---

## Encoder

```rust
pub struct Encoder { /* ... */ }
```

### Construction

| Method | Description |
|--------|-------------|
| `Encoder::new()` | Default encoder |
| `Encoder::with_limits(limits)` | Custom resource limits |

### Configuration

| Method | Description |
|--------|-------------|
| `enable_dedup()` | Enable string deduplication (StringDict + Reference) |
| `set_compression(CompressionType)` | Set block compression (`None`, `Lz4`, `Zstd`, `Snappy`) |
| `set_flags(u8)` | Set file header flags |

### Encoding

| Method | Description |
|--------|-------------|
| `encode_value(&Value) → Result<()>` | Encode a single value into the current block |
| `finish() → Result<Vec<u8>>` | Flush the current block, write trailer, return bytes |

### Behavior

- The encoder automatically writes the `CROUSv1` file header on first use.
- `finish()` emits a StringDict block (if dedup enabled and strings were recorded), then a Data block with optional compression, then a Trailer block with a whole-file XXH64 checksum.
- Compression uses a ratio threshold: if the compressed payload is not smaller than the original, it falls back to uncompressed.
- Nesting depth is enforced against `Limits::max_nesting_depth`.

---

## Decoder

```rust
pub struct Decoder<'a> { /* ... */ }
```

### Construction

| Method | Description |
|--------|-------------|
| `Decoder::new(data: &[u8])` | Default decoder |
| `Decoder::with_limits(data, limits)` | Custom resource limits |

### Decoding

| Method | Description |
|--------|-------------|
| `decode_next() → Result<CrousValue<'a>>` | Decode next value (zero-copy). Fails on compressed blocks. |
| `decode_next_owned() → Result<Value>` | Decode next value (owned). Works with compressed blocks. |
| `decode_all() → Result<Vec<CrousValue<'a>>>` | Decode all remaining values (zero-copy). |
| `decode_all_owned() → Result<Vec<Value>>` | Decode all remaining values (owned). |

### Inspection

| Method | Description |
|--------|-------------|
| `position() → usize` | Current byte offset in input |
| `memory_used() → usize` | Cumulative tracked memory allocation |

### Behavior

- The decoder validates the `CROUSv1` magic header on first block read.
- Each block's XXH64 checksum is verified before decoding.
- StringDict blocks are consumed transparently — they populate the string dictionary, then the decoder loops to read the next Data block.
- Compressed blocks are decompressed into an internal buffer for the owned decode path; zero-copy decode rejects them with an error.
- Reference wire types are resolved from the per-block string dictionary (both `str_slices` for zero-copy and `owned_strings` for owned).
- All resource limits (nesting, memory, block size, string length, item count) are enforced.

### `BumpDecoder` (feature = `fast-alloc`)

```rust
pub struct BumpDecoder<'a> { /* ... */ }
```

Wraps `Decoder` with a `bumpalo::Bump` arena for scratch memory. Call `reset_arena()` between blocks to reclaim memory without deallocating backing storage.

---

## Text Format

The Crous text notation provides a 1:1 mapping to the binary format:

```text
{
    name: "Alice";
    age: 30;
    scores: [100, 95, 87];
    active: true;
    avatar: b64#iVBOR...;
}
```

### API

```rust
use crous_core::text::{parse, pretty_print};
use crous_core::Value;

let value = parse(r#"{ name: "Alice"; age: 30; }"#).unwrap();
let text = pretty_print(&value, 4);  // indent=4
```

### Syntax Rules

- Objects: `{ key: value; }` — semicolons are mandatory terminators
- Arrays: `[a, b, c]` — commas as separators
- Strings: `"double-quoted"` with `\n`, `\t`, `\\`, `\"` escapes
- Binary: `b64#<base64-data>;` using standard base64
- Numbers: bare digits for uint, `-` prefix for int, `.` for float
- Keywords: `null`, `true`, `false`
- Type annotations: `42::u32` (optional, skipped during parse)
- Comments: `// line` and `/* block */` (nested)

---

## Derive Macros

```rust
use crous_derive::{Crous, CrousSchema};
use crous_core::CrousBytes;

#[derive(Debug, PartialEq, Crous, CrousSchema)]
struct Person {
    #[crous(id = 1)] name: String,
    #[crous(id = 2)] age: u8,
    #[crous(id = 3)] tags: Vec<String>,
    #[crous(id = 4)] avatar: CrousBytes,
}
```

### `#[derive(Crous)]`

Generates:
- `to_crous_value(&self) → Value` — serializes fields as an Object
- `from_crous_value(&Value) → Result<Self>` — deserializes from Object, skipping unknown fields
- `schema_fingerprint() → u64` — XXH64 of `TypeName:field1=id1,field2=id2,...`
- `type_name() → &'static str`

Convenience methods from the trait:
- `to_crous_bytes() → Result<Vec<u8>>` — encode to binary
- `from_crous_bytes(data) → Result<Self>` — decode from binary
- `from_crous_bytes_with_limits(data, limits) → Result<Self>` — decode with limits

### `#[derive(CrousSchema)]`

Generates:
- `schema_info() → &'static [(&str, u64)]` — field name/ID pairs
- `schema_type_name() → &'static str`

### Blanket Implementations

The `Crous` trait has built-in implementations for all common Rust types:

| Rust type | Crous `Value` | Notes |
|-----------|---------------|-------|
| `bool` | `Bool` | |
| `u8`, `u16`, `u32`, `u64`, `usize` | `UInt` | widened to `u64`, range-checked on decode |
| `u128` | `UInt` | panics on encode if > `u64::MAX` |
| `i8`, `i16`, `i32`, `i64`, `isize` | `Int` | widened to `i64`, range-checked on decode |
| `i128` | `Int` | panics on encode if outside `i64` range |
| `f32`, `f64` | `Float` | `f32` widened to `f64` |
| `String`, `Box<str>` | `Str` | |
| `CrousBytes` | `Bytes` | newtype for raw binary blobs |
| `Vec<u8>` | `Array` of `UInt` | use `CrousBytes` for `Value::Bytes` |
| `Vec<T: Crous>` | `Array` | |
| `Option<T: Crous>` | `T` or `Null` | |
| `Box<T: Crous>` | delegates to `T` | transparent wrapper |
| `HashMap<String, T>` | `Object` | insertion-order not guaranteed |
| `BTreeMap<String, T>` | `Object` | sorted by key |
| `(A,)` … `(A,B,C,D,E,F)` | `Array` | heterogeneous tuples up to 6 elements |
| `()` | `Null` | unit type |

> **`Vec<u8>` vs `CrousBytes`**: `Vec<u8>` encodes as an `Array` of `UInt` values via the generic `Vec<T>` impl. Use `CrousBytes(Vec<u8>)` when you want the compact `Value::Bytes` wire encoding for raw binary data.

Signed integer types (`i8`–`i64`, `isize`) also accept `Value::UInt` on decode when the value fits, providing cross-compatibility with unsigned encoders.

---

## Compression

**Crate:** `crous-compression`

### Trait

```rust
pub trait Compressor: Send + Sync {
    fn compression_type(&self) -> CompressionType;
    fn compress(&self, input: &[u8]) -> Result<Vec<u8>>;
    fn decompress(&self, input: &[u8], max_output: usize) -> Result<Vec<u8>>;
    fn name(&self) -> &'static str;
}
```

### Built-in Compressors

| Type | Feature | Crate | Notes |
|------|---------|-------|-------|
| `NoCompression` | always | — | Passthrough |
| `ZstdCompressor` | `zstd` | zstd 0.13 | Level 1–22, default 3 |
| `Lz4Compressor` | `lz4` | lz4_flex 0.11 | Pure Rust, prepend-size framing |
| `SnappyCompressor` | `snappy` | snap 1 | Google Snappy via snap crate |

### Adaptive Selection

```rust
use crous_compression::{AdaptiveSelector, CompressorRegistry};

let reg = CompressorRegistry::with_defaults();
let selector = AdaptiveSelector::default(); // ratio_threshold: 0.9, sample: 64KB
let best = selector.select(data, &reg);
```

### Registry

```rust
let mut reg = CompressorRegistry::new();
reg.register(Box::new(MyCustomCompressor));
let comp = reg.find(CompressionType::Lz4);
```

---

## IO & Streaming

**Crate:** `crous-io`

### Convenience Functions

```rust
use crous_io::{read_file_bytes, write_values_to_bytes};

let values = read_file_bytes(&data)?;
let bytes = write_values_to_bytes(&values)?;
```

### Shared Buffers (bytes::Bytes)

```rust
use crous_io::{read_from_shared, write_to_shared};

let shared = write_to_shared(&values)?;  // bytes::Bytes
let values = read_from_shared(shared)?;
```

### Async Framed IO (Tokio)

```rust
use crous_io::FramedWriter;

let mut writer = FramedWriter::new(tcp_stream);
writer.write_data(payload).await?;
writer.flush().await?;
```

```rust
use crous_io::FramedReader;

let mut reader = FramedReader::new(tcp_stream);
let _header = reader.read_header().await?;
while let Some(block) = reader.read_next_block_raw().await? {
    // process block bytes
}
```

### Memory-Mapped Files (feature = `mmap`)

```rust
use crous_io::MmapReader;

let reader = MmapReader::open("data.crous")?;
let values = reader.decode_all()?;           // owned
let borrowed = reader.decode_all_borrowed()?; // zero-copy CrousValue<'_>
```

---

## SIMD Acceleration

**Crate:** `crous-simd`

Provides optimized byte-level operations with automatic platform fallbacks.

| Function | Description | SIMD |
|----------|-------------|------|
| `find_byte(data, needle)` | First occurrence of byte | NEON 16B |
| `count_byte(data, needle)` | Count occurrences | NEON 16B |
| `find_non_ascii(data)` | First byte ≥ 0x80 (UTF-8 prescan) | NEON 16B |
| `batch_decode_varints(data, count)` | Decode multiple LEB128 varints | scalar |
| `batch_decode_varints_simd(data, count)` | SIMD-prescan varint boundaries | NEON (feature `simd-varint`) |

All functions fall back to scalar implementations on unsupported architectures.

---

## FFI (C Bindings)

**Crate:** `crous-ffi`

```c
// Encode JSON → Crous binary
int crous_encode_buffer(
    const uint8_t *in_ptr, size_t in_len,
    uint8_t **out_ptr, size_t *out_len
);

// Decode Crous binary → JSON string
int crous_decode_buffer(
    const uint8_t *in_ptr, size_t in_len,
    uint8_t **json_out, size_t *json_len
);

// Free library-allocated memory
void crous_free(uint8_t *ptr, size_t len);
```

Returns `0` on success, `-1` on error. Caller must free output with `crous_free`.

---

## Python Bindings

**Crate:** `crous-python` (PyO3 0.28)

Build with `maturin develop` inside the `crous-python/` directory.

```python
import _crous_native as cn

# Module-level functions
data = cn.encode({"name": "Alice", "age": 30})
obj = cn.decode(data)

# Encoder class
enc = cn.Encoder()
enc.enable_dedup()
enc.set_compression("lz4")
enc.encode({"key": "value"})
data = enc.finish()

# Decoder class
dec = cn.CrousDecoder(data)
values = dec.decode_all()  # returns list
```

---

## Resource Limits

```rust
use crous_core::Limits;
```

| Field | Default | Strict | Unlimited |
|-------|---------|--------|-----------|
| `max_nesting_depth` | 128 | 32 | `usize::MAX` |
| `max_block_size` | 64 MiB | 1 MiB | `usize::MAX` |
| `max_items` | 1,000,000 | 10,000 | `usize::MAX` |
| `max_memory` | 256 MiB | 4 MiB | `usize::MAX` |
| `max_string_length` | 16 MiB | 64 KiB | `usize::MAX` |

```rust
let limits = Limits::strict();          // for untrusted input
let limits = Limits::unlimited();       // for trusted data only
let limits = Limits { max_nesting_depth: 64, ..Limits::default() };
```

---

## Wire Format Summary

### File Layout

```
┌─────────────┐
│ Header (8B) │  "CROUSv1" + flags
├─────────────┤
│ Block 0     │  [StringDict block, if dedup]
├─────────────┤
│ Block 1     │  Data block (values)
├─────────────┤
│ ...         │
├─────────────┤
│ Trailer     │  Type=0xFF, whole-file XXH64
└─────────────┘
```

### Block Layout

```
block_type (1B) | payload_len (varint) | compression (1B) | checksum (8B, XXH64) | payload
```

### Wire Types (low 4 bits of tag byte)

| Tag | Name | Payload |
|-----|------|---------|
| `0x00` | Null | none |
| `0x01` | Bool | 1 byte (0/1) |
| `0x02` | VarUInt | LEB128 |
| `0x03` | VarInt | ZigZag + LEB128 |
| `0x04` | Fixed64 | 8 bytes LE (f64) |
| `0x05` | LenDelimited | subtype(1B) + len(varint) + bytes |
| `0x06` | StartObject | count(varint) + entries + EndObject |
| `0x07` | EndObject | none |
| `0x08` | StartArray | count(varint) + items + EndArray |
| `0x09` | EndArray | none |
| `0x0A` | Reference | varint (string dictionary index) |

### LenDelimited Sub-types

| Byte | Meaning |
|------|---------|
| `0x00` | UTF-8 string |
| `0x01` | Raw binary blob |

---

## Feature Flags

### `crous-core`

| Flag | Effect |
|------|--------|
| `xxh3` | Use XXH3-64 for checksums (~2× throughput on SIMD CPUs) |
| `compat-crc32` | CRC32 checksum support for legacy data |
| `fast-alloc` | `BumpDecoder` with bumpalo arena |
| `zstd` | Zstd block compression |
| `lz4` | LZ4 block compression (pure Rust via lz4_flex) |
| `snappy` | Snappy block compression |

### `crous-io`

| Flag | Effect |
|------|--------|
| `mmap` | `MmapReader` for zero-copy file access |

### `crous-simd`

| Flag | Effect |
|------|--------|
| `simd-varint` | SIMD varint boundary pre-scan (aarch64 NEON) |

---

## Performance

Measured on **Apple M4** (10 cores), Rust 1.92.0, `--release`, 10 iterations per dataset.

### Decode Throughput (MB/s, higher = better)

| Dataset | Crous | JSON | MsgPack | CBOR |
|---------|-------|------|---------|------|
| small_objects | 816 | 280 | 240 | 122 |
| string_heavy | 909 | 348 | 331 | 205 |
| nested_deep | 359 | 180 | 137 | 88 |
| binary_blobs | 24,226 | 10,405 | 26,905 | 13,305 |
| mixed_api_events | 1,111 | 391 | 381 | 216 |
| numeric_heavy | 813 | 393 | 285 | 135 |

### Encode Throughput (MB/s)

| Dataset | Crous | JSON | MsgPack | CBOR |
|---------|-------|------|---------|------|
| small_objects | 983 | 1,072 | 1,100 | 894 |
| string_heavy | 1,602 | 1,669 | 1,761 | 1,518 |
| nested_deep | 771 | 917 | 604 | 513 |
| binary_blobs | 1,825 | 2,871 | 8,437 | 8,265 |
| mixed_api_events | 1,653 | 1,562 | 1,809 | 1,572 |
| numeric_heavy | 1,087 | 1,139 | 1,223 | 992 |

### Serialized Size

| Dataset | Crous | Crous+Dedup | JSON | MsgPack | CBOR | Crous/JSON |
|---------|-------|-------------|------|---------|------|------------|
| small_objects | 8.6 MB | 12.0 MB | 10.5 MB | 7.8 MB | 7.9 MB | 0.82× |
| string_heavy | 1.0 MB | 668 KB | 1.1 MB | 926 KB | 927 KB | 0.96× |
| nested_deep | 1.0 MB | 1.5 MB | 1.2 MB | 835 KB | 835 KB | 0.87× |
| binary_blobs | 6.4 MB | 6.4 MB | 8.5 MB | 8.5 MB | 8.5 MB | 0.75× |
| mixed_api_events | 1.9 MB | 2.8 MB | 2.0 MB | 1.8 MB | 1.8 MB | 0.92× |
| numeric_heavy | 3.7 MB | 3.7 MB | 6.0 MB | 3.5 MB | 3.5 MB | 0.63× |

**Key takeaways:**
- Crous **decode** is **2–5× faster** than JSON and CBOR for structured data
- Crous is **smaller than JSON** across all datasets (0.63–0.96×)
- Binary blobs: Crous stores raw bytes (no base64), achieving 0.75× JSON size
- Numeric data: Crous varints achieve 0.63× JSON size
- Dedup overhead makes output larger for small_objects/nested_deep (dict block overhead exceeds savings)

---

## Safety & Security

- **No `unsafe` in `crous-core`** — all encoding/decoding is safe Rust
- **All `unwrap()` calls** are in test code or preceded by bounds checks on fixed-size slices
- **Resource limits** prevent DoS attacks: nesting depth, memory, block size, string length, item count
- **Checksum verification** on every block (XXH64)
- **Compression DoS mitigation**: `max_output` parameter on all decompressors
- **UTF-8 validation**: all string decoding goes through `std::str::from_utf8`
- **Fuzz targets** available in `fuzz/fuzz_targets/` for decode, roundtrip, text, and varint

---

*Generated from Crous v1.1.0 codebase. See `docs/SPEC.md` for the full wire format specification.*
