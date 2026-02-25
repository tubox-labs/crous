# Design Risks & Tradeoffs

## Format Design Tradeoffs

### TLV (Tag-Length-Value) vs Columnar

**Choice: TLV with element counts.**

- **Pro**: Natural fit for streaming and tree-structured data. Easy to skip unknown fields.
- **Pro**: Simple implementation, well-understood.
- **Con**: Not optimal for analytical workloads (column scans). For analytics, consider Apache Arrow.
- **Mitigation**: Index blocks enable random access within a file. String dictionaries provide some columnar benefits.

### StartObject/EndObject markers vs Length-Prefixed Objects

**Choice: Both — count prefix + end markers.**

We encode element counts at the start of objects/arrays (for fast skipping) AND end markers (for streaming validation). This uses ~2 extra bytes per container but provides:
- Forward skip without recursive descent (use count to skip elements).
- Streaming validation (end markers confirm structure).
- Resilience to truncation (missing end marker detected).

### Per-Block vs Whole-File Compression

**Choice: Per-block.**

- **Pro**: Random access preserved. Can decode any block independently.
- **Pro**: Mixed compression strategies possible (e.g., zstd for text, none for binary blobs).
- **Con**: Slightly lower compression ratio than whole-file (no cross-block dictionary).
- **Mitigation**: Block sizes can be large (up to 64 MiB default) to amortize overhead.

### XXH64 vs CRC32 vs SHA-256

**Choice: XXH64 for per-block, XXH64 for file trailer.**

- XXH64: ~30 GB/s throughput, 64-bit hash, excellent collision resistance for non-cryptographic use.
- CRC32: Weaker collision properties, hardware accelerated but XXH64 is already faster in software.
- SHA-256: Cryptographic strength unnecessary for data integrity (we're not preventing tampering, just detecting corruption).

### String Dictionary: Per-Block vs Global

**Choice: Per-block string dictionary.**

- **Pro**: Each block is self-contained (streamable, seekable).
- **Pro**: Dictionary overhead amortized over typical block sizes.
- **Con**: Repeated strings across blocks are not deduplicated.
- **Future**: Optional global dictionary block (BlockType::StringDict) for files where cross-block dedup matters.

## Implementation Risks

### Schema Evolution Complexity

**Risk**: Complex schema changes (renaming fields, changing types) may lead to subtle data loss.

**Mitigation**: Field IDs are stable. Unknown fields are skipped, not rejected. Type changes require explicit migration. The `CrousSchema` derive provides `schema_info()` for programmatic validation.

### Zero-Copy Safety

**Risk**: `CrousValue<'a>` borrows from the input buffer. If the buffer is deallocated while `CrousValue` references exist, UB would occur.

**Mitigation**: Lifetime parameter `'a` prevents use-after-free at compile time. This is standard Rust borrow semantics — no `unsafe` involved.

### SIMD Portability

**Risk**: SIMD code may not compile or may perform poorly on non-x86 architectures.

**Mitigation**: SIMD is behind the `crous-simd` feature flag and is entirely optional. All operations have scalar fallbacks.

### Endianness

**Risk**: The format uses little-endian on wire. Big-endian hosts must byte-swap.

**Mitigation**: All multi-byte integers use `to_le_bytes()`/`from_le_bytes()`, which Rust handles correctly on all platforms. Varints (LEB128) are endian-independent by definition.

## Performance Risks

### Varint Decode Throughput

**Risk**: LEB128 decoding is branch-heavy and may bottleneck on large varint arrays.

**Mitigation**: Most field IDs and lengths are small (<128), fitting in 1 byte (no branch misprediction). SIMD batch decoding is planned for bulk varint workloads.

### Allocation Pressure

**Risk**: Decoding into owned `Value` types allocates many small strings/vectors.

**Mitigation**: Zero-copy `CrousValue<'a>` avoids allocation for string-heavy reads. Schema-bound decode (via `#[derive(Crous)]`) can decode directly into user structs.

## Prioritized Optimization Roadmap

1. **String dictionary** — implement per-block string table for repeated key dedup.
2. **SIMD varint** — batch decode using PEXT/PDEP on x86_64.
3. **Arena allocation** — pool allocations for `Value` trees.
4. **Mmap decoder** — memory-mapped file support in `crous-io`.
5. **Columnar mode** — optional columnar block layout for analytics workloads.

## Hot Functions to Profile

- `Encoder::encode_value_inner` — recursive encoding, measure per-field overhead.
- `Decoder::decode_value_at` — recursive decoding, tag dispatch overhead.
- `varint::decode_varint` — called per field, must be branch-optimal.
- `compute_xxh64` — called per block, should be negligible.
- `String::from_utf8` / `str::from_utf8` — UTF-8 validation on every string.
