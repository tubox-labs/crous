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

Within a data block, repeated strings can be stored in a dictionary table at the start of the block. Subsequent occurrences reference the dictionary by index.

**Algorithm:**
1. First occurrence: encode string normally (LenDelimited + string data).
2. Track strings in a hash map (string → u32 index).
3. Subsequent occurrences: encode as Reference wire type with dictionary index.

**Delta + prefix compression** (planned): Sorted dictionary entries share common prefixes. Store prefix length + suffix for compact representation.

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
