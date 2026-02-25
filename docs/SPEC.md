# Crous Binary Format Specification v1.0

## 1. Overview

Crous is a compact, canonical binary serialization format designed as an alternative to JSON.
It provides deterministic encoding, schema evolution support, and both human-readable and
binary representations.

**Design principles:**
1. Safety and correctness over micro-optimizations
2. Deterministic encoding (same data → same bytes, always)
3. Zero-copy decoding when possible
4. Forward/backward compatible schema evolution
5. Streaming and random-access support

## 2. File Layout

```
┌──────────────────────────────────────────┐
│ File Header (8 bytes)                    │
├──────────────────────────────────────────┤
│ Block 0 (Data)                           │
│   block_type(1) | len(varint) |          │
│   comp(1) | checksum(8) | payload(...)   │
├──────────────────────────────────────────┤
│ Block 1 (Data)                           │
├──────────────────────────────────────────┤
│ ...                                      │
├──────────────────────────────────────────┤
│ Block N-1 (Optional: Index)              │
├──────────────────────────────────────────┤
│ Trailer Block (8-byte overall checksum)  │
└──────────────────────────────────────────┘
```

### 2.1 File Header (8 bytes)

| Offset | Size | Field    | Value                    |
|--------|------|----------|--------------------------|
| 0      | 7    | Magic    | ASCII `"CROUSv1"`        |
| 7      | 1    | Flags    | Bit field (see below)    |

**Flags byte:**
- Bit 0: Reserved (0)
- Bit 1: Has index block
- Bit 2: Has schema block
- Bits 3-7: Reserved (0)

### 2.2 Block Header

| Field       | Size    | Description                              |
|-------------|---------|------------------------------------------|
| block_type  | 1 byte  | Block type ID (see §2.3)                 |
| block_len   | varint  | Length of payload in bytes                |
| comp_type   | 1 byte  | Compression algorithm ID                 |
| checksum    | 8 bytes | XXH64 of uncompressed payload (LE)       |
| payload     | N bytes | Block data (N = block_len)               |

### 2.3 Block Types

| ID   | Name       | Description                              |
|------|------------|------------------------------------------|
| 0x01 | Data       | Encoded value data                       |
| 0x02 | Index      | Offset index for random access           |
| 0x03 | Schema     | Embedded schema information              |
| 0x04 | StringDict | String dictionary for deduplication      |
| 0xFF | Trailer    | File-level checksum (last block)         |

### 2.4 Compression Types

| ID   | Name   | Description              |
|------|--------|--------------------------|
| 0x00 | None   | No compression           |
| 0x01 | Zstd   | Zstandard compression    |
| 0x02 | Snappy | Snappy compression       |
| 0x03 | LZ4    | LZ4 frame compression    |

> **Adaptive compression** (feature `lz4`/`zstd`/`snappy`): the encoder
> can sample the first N bytes of a block and select the algorithm that
> achieves the best ratio above a configurable threshold. See
> `AdaptiveSelector` in crous-compression.

### 2.5 Compressed Block Wire Format

When a block is compressed, the on-wire payload is:

```
uncompressed_len(varint) | compressed_data
```

- The `block_len` field in the block header reflects the **compressed** size (including the varint prefix).
- The `checksum` is computed on the **uncompressed** payload, so integrity is verified after decompression.
- If the compressed output is not smaller than the original, the encoder falls back to `comp_type = 0x00` (None).

### 2.6 Decode Paths

- **Zero-copy** (`decode_next()` → `CrousValue<'a>`): Borrows from the input slice. Rejects compressed blocks with a descriptive error.
- **Owned** (`decode_next_owned()` → `Value`): Works transparently with both compressed and uncompressed blocks. Decompresses into an internal buffer when needed.

## 3. Wire Types

Each value is prefixed with a **tag byte**:
- Low 4 bits: wire type
- High 4 bits: flags

| ID   | Wire Type      | Payload                           |
|------|----------------|-----------------------------------|
| 0x00 | Null           | None                              |
| 0x01 | Bool           | 1 byte (0x00=false, 0x01=true)    |
| 0x02 | VarUInt        | LEB128 unsigned integer           |
| 0x03 | VarInt         | ZigZag + LEB128 signed integer    |
| 0x04 | Fixed64        | 8 bytes little-endian (f64)       |
| 0x05 | LenDelimited   | sub-type(1) + len(varint) + data  |
| 0x06 | StartObject    | count(varint) + fields...         |
| 0x07 | EndObject      | None                              |
| 0x08 | StartArray     | count(varint) + items...          |
| 0x09 | EndArray       | None                              |
| 0x0A | Reference      | ref_id(varint)                    |

### 3.1 LenDelimited Sub-types

| ID   | Sub-type | Description              |
|------|----------|--------------------------|
| 0x00 | String   | UTF-8 encoded string     |
| 0x01 | Bytes    | Raw binary data          |

### 3.2 Object Encoding

```
StartObject(0x06) | count(varint) |
  key_len(varint) key_bytes(UTF-8) value(wire-encoded) |
  key_len(varint) key_bytes(UTF-8) value(wire-encoded) |
  ...
EndObject(0x07)
```

### 3.3 Array Encoding

```
StartArray(0x08) | count(varint) |
  value(wire-encoded) |
  value(wire-encoded) |
  ...
EndArray(0x09)
```

## 4. Integer Encoding

### 4.1 Unsigned: LEB128

Variable-length encoding: 7 bits of data per byte, MSB indicates continuation.

```
Value     Encoded bytes
0         00
127       7F
128       80 01
300       AC 02
16384     80 80 01
```

### 4.2 Signed: ZigZag + LEB128

ZigZag maps signed to unsigned: `0→0, -1→1, 1→2, -2→3, 2→4, ...`
Formula: `encode(n) = (n << 1) ^ (n >> 63)`

## 5. Checksums

### 5.1 Per-Block: XXH64

Every block includes an 8-byte XXH64 hash of its uncompressed payload (seed=0, little-endian).

**Why XXH64**: ~30 GB/s throughput, excellent collision resistance for non-cryptographic integrity checks. Adds < 0.1% overhead to typical workloads.

#### 5.1.1 Alternative checksum algorithms (feature flags)

| Algorithm | Feature flag   | Performance | Notes |
|-----------|---------------|-------------|-------|
| XXH64     | *(default)*   | ~30 GB/s    | Default, best-tested |
| XXH3-64   | `xxh3`        | ~50 GB/s    | Newer, SIMD-optimized |
| CRC32     | `compat-crc32`| ~10 GB/s    | Legacy compatibility |

The `ChecksumAlgo` enum provides a unified API for switching at runtime.
The wire format always stores an 8-byte checksum field; CRC32 values are
zero-extended to 8 bytes for backward compatibility.

### 5.2 File Trailer

The trailer block contains an XXH64 hash of all preceding bytes (header + all blocks). This detects file-level truncation or corruption.

## 6. Endianness

**Canonical wire format: little-endian.**

All multi-byte fixed-width integers (f64, checksums) are stored in little-endian. LEB128 varints are byte-order independent by definition. On big-endian hosts, byte-swap operations are inserted by Rust's `to_le_bytes()`/`from_le_bytes()`.

## 7. Schema Evolution

### 7.1 Field IDs

When using `#[derive(Crous)]`, each field gets a stable integer ID via `#[crous(id = N)]`. Fields are matched by name in schema-less mode and by ID in schema-on-write mode.

### 7.2 Compatible Changes (minor version)
- Adding new optional fields (with new IDs)
- Adding new wire types with defined skip semantics

### 7.3 Incompatible Changes (major version)
- Changing the file header magic
- Changing existing wire type semantics
- Removing the ability to skip unknown fields

### 7.4 Unknown Field Skipping

Decoders MUST be able to skip unknown wire types:
- Null/Bool/End*: fixed size, trivially skipped.
- VarUInt/VarInt: skip varint bytes.
- Fixed64: skip 8 bytes.
- LenDelimited: read length, skip that many bytes.
- StartObject/StartArray: read count, recursively skip children.
- Reference: skip varint ref_id.

## 8. String Dictionary (Per-Block)

Within a data block, repeated strings can be stored in a dictionary table. Subsequent occurrences reference the dictionary by index using the Reference wire type.

**Algorithm (Encoder):**
1. When `enable_dedup()` is called, the encoder maintains a per-block `HashMap<String, u32>`.
2. First occurrence: encode string normally (LenDelimited + string data), record in the map with the next sequential index.
3. Subsequent occurrences: encode as Reference wire type (0x0A) with the dictionary index.
4. On `flush_block()`, if the dictionary is non-empty, emit a **StringDict block** (type 0x04) *before* the data block.
5. The dictionary is cleared between blocks.

### 8.1 StringDict Block Format

The StringDict block (type `0x04`) is emitted immediately before its corresponding data block. It uses standard block framing:

```
block_type(0x04) | block_len(varint) | comp_type(0x00) | checksum(8B) | payload
```

**Payload layout:**

```
entry_count(varint) | entry₀ | entry₁ | ... | entryₙ₋₁
```

Each entry uses **prefix-delta compression** (entries are sorted lexicographically):

```
original_index(varint) | prefix_len(varint) | suffix_len(varint) | suffix_bytes
```

- `original_index`: The insertion-order index matching Reference wire type IDs.
- `prefix_len`: Number of bytes shared with the previous entry (0 for the first).
- `suffix_len`: Length of the non-shared suffix.
- `suffix_bytes`: The raw suffix bytes.

### 8.2 Decoder Handling

When the decoder encounters a StringDict block:
1. Verify the block checksum.
2. Parse the prefix-delta entries, reconstructing full strings.
3. Populate the per-block string table in insertion order using `original_index`.
4. Proceed to the next block (which should be a Data block).
5. During data block decoding, Reference wire types resolve from the pre-populated table.

The StringDict block is consumed transparently — callers of `decode_next()` / `decode_next_owned()` never see it.

### 8.3 Prefix-Delta Compression

Entries in the StringDict block are sorted lexicographically before encoding. Each entry stores only the suffix that differs from the previous entry:

| Previous      | Current                  | prefix_len | suffix    |
|---------------|--------------------------|------------|-----------|
| (none)        | `config_cache_host`      | 0          | `config_cache_host` |
| `config_cache_host` | `config_cache_port` | 13         | `port`    |
| `config_cache_port` | `config_database_host` | 7       | `database_host` |
| `config_database_host` | `config_database_port` | 16   | `port`    |

This reduces dictionary overhead for datasets with structured/hierarchical key names.

## 9. Reference/Dedup

The Reference wire type (0x0A) encodes a varint index into a per-block reference table. This allows deduplication of:
- Repeated strings (via string dictionary)
- Repeated subtrees (via structural hash → reference table)

Reference IDs are scoped to a single block. Cross-block references are not supported (blocks are self-contained).

## 10. Streaming vs Block Mode

### Streaming Mode
- Blocks are emitted as data arrives.
- No index block.
- Reader processes blocks sequentially.

### Block Mode
- Entire document encoded into one or more data blocks.
- Optional index block at end for random access.
- Optional schema block for self-describing files.

## 11. Security Considerations

See [SECURITY.md](SECURITY.md) for the full threat model.

Key points:
- All lengths are bounds-checked against configurable limits.
- Nesting depth is limited (default: 128).
- Varint decoder rejects overlong encodings.
- UTF-8 validation on all strings.
- Per-block checksums prevent processing corrupted data.
- Decompression output is bounded.
- `skip_value_at()` enforces Limits (nesting + item count) during skip.

## 12. Feature Flags

All optimizations are gated behind Cargo feature flags to keep the default
binary small and compilation fast.

### 12.1 crous-core features

| Flag          | Dependencies  | Description |
|---------------|---------------|-------------|
| `xxh3`        | —             | Use XXH3-64 for checksums (faster SIMD path) |
| `compat-crc32`| `crc32fast`   | CRC32 checksum support for legacy interop |
| `fast-alloc`  | `bumpalo`     | Per-block bump allocator via `BumpDecoder` |

### 12.2 crous-io features

| Flag   | Dependencies | Description |
|--------|-------------|-------------|
| `mmap` | `memmap2`   | Memory-mapped zero-copy file reader (`MmapReader`) |

### 12.3 crous-compression features

| Flag     | Dependencies | Description |
|----------|-------------|-------------|
| `lz4`    | `lz4_flex`  | LZ4 compression support |
| `zstd`   | `zstd`      | Zstandard compression |
| `snappy` | `snap`      | Snappy compression |

### 12.4 crous-simd features

| Flag         | Dependencies | Description |
|--------------|-------------|-------------|
| `simd-varint`| —           | NEON SIMD-accelerated varint boundary pre-scan |

## 13. Text Format

The Crous text format is a deterministic, human-readable notation that maps
1:1 to the binary format. See `docs/TEXT_FORMAT.abnf` for the normative ABNF
grammar (RFC 5234).

Key differences from JSON:
- Object fields terminated by `;` not `,`
- Binary literals: `b64#<base64>;`
- Optional type annotations: `42::u32`
- Comments: `// line` and `/* block */`
- Signed integers use explicit `+`/`-` prefix

## 14. CLI Reference

The `crous` CLI tool (crous-cli) provides the following commands:

| Command    | Description |
|------------|-------------|
| `inspect`  | Show block layout and checksums |
| `pretty`   | Pretty-print in text notation |
| `to-json`  | Convert to JSON |
| `from-json`| Convert JSON to binary |
| `encode`   | Parse text notation, emit binary |
| `decode`   | Decode binary to text notation |
| `validate` | Verify header, checksums, and decode integrity |
| `bench`    | Quick encode/decode performance test |
