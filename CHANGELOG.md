# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Versioning Rules

- **Major**: File format changes (magic, wire type semantics), breaking API changes.
- **Minor**: New features (new wire types, new block types, new API methods), backward-compatible.
- **Patch**: Bug fixes, performance improvements, documentation.

## [Unreleased]

### Added
- Initial implementation of `crous-core` with encoder/decoder
- LEB128 varint and ZigZag signed integer encoding
- Block framing with per-block XXH64 checksums
- File header with magic "CROUSv1"
- `Value` (owned) and `CrousValue<'a>` (zero-copy) types
- Human-readable text parser and pretty-printer
- `#[derive(Crous)]` and `#[derive(CrousSchema)]` proc-macros
- CLI tool: inspect, pretty, to-json, from-json, encode, bench
- Compression plugin trait with no-op, zstd, snappy adapters
- C FFI bindings with `crous_encode_buffer`, `crous_decode_buffer`, `crous_free`
- Async Tokio adapters (FramedWriter, FramedReader)
- Property-based tests (proptest)
- Criterion benchmarks
- Fuzz target for decode functions
- GitHub Actions CI
- Security documentation and threat model
- Design risks and tradeoffs document

### Added (Audit & Hardening)
- **Decoder memory tracking**: cumulative allocation tracking with configurable `max_memory` limit
- **Unknown-field skipping**: `skip_value_at()` for forward-compatible decoding
- **String deduplication**: encoder `enable_dedup()` emits `Reference` wire types for repeated strings; decoder resolves via zero-copy `str_slices` table
- **NEON SIMD byte scanning** (`crous-simd`): vectorized `find_byte()`, `count_byte()`, `find_non_ascii()` using aarch64 NEON intrinsics with scalar fallbacks
- **Pure Python implementation** (`python/crous/`): full encode/decode with 8 modules, XXH64 hasher, text parser/printer, 54 tests
- **Cross-language interop**: bidirectional Rust↔Python binary format verified
- **Expanded benchmarks**: JSON head-to-head comparison, deep nesting, numeric arrays, size report
- **3 new fuzz targets**: `fuzz_roundtrip` (structured Value), `fuzz_text` (text parser), `fuzz_varint` (varint codec)
- **CI improvements**: MSRV testing (1.85.0), multi-OS matrix, Python test job, cross-language interop job, all 4 fuzz targets

### Fixed
- Python XXH64: corrected `PRIME64_2` constant (`0xC2B2AE3D27D4EB4F`)
- Python XXH64: corrected `PRIME64_4` constant (`0x85EBCA77C2B2AE63`)
- `crous-compression`: conditional `#[cfg]` gate on `CrousError` import to eliminate unused-import warning

## [0.1.0] - TBD

Initial release.

---

## Release Checklist

- [ ] Update version in all `Cargo.toml` files
- [ ] Update this CHANGELOG
- [ ] Run full test suite: `cargo test --workspace --all-features`
- [ ] Run clippy: `cargo clippy --workspace --all-features -- -D warnings`
- [ ] Run Python tests: `cd python && python3 -m pytest tests/ -v`
- [ ] Run benchmarks: `cargo bench -p crous-core`
- [ ] Run all fuzz targets (30s each):
  - `cargo +nightly fuzz run fuzz_decode -- -max_total_time=30`
  - `cargo +nightly fuzz run fuzz_roundtrip -- -max_total_time=30`
  - `cargo +nightly fuzz run fuzz_text -- -max_total_time=30`
  - `cargo +nightly fuzz run fuzz_varint -- -max_total_time=30`
- [ ] Run `cargo audit`
- [ ] Verify cross-language interop (Rust↔Python)
- [ ] Review any new `unsafe` code
- [ ] Tag release: `git tag v0.1.0`
- [ ] Publish: `cargo publish -p crous-core && cargo publish -p crous-derive && ...`
