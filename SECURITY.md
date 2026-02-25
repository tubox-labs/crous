# Security Policy

## Threat Model

Crous is designed to safely handle **untrusted input**. The decoder is built to resist adversarial documents including:

### Attack Vectors & Mitigations

| Attack | Mitigation |
|--------|-----------|
| **Oversized length fields** | All varint-decoded lengths are bounds-checked against configurable `Limits`. Default max block size: 64 MiB, max string: 16 MiB. |
| **Integer overflow** | LEB128 decoder rejects varints > 10 bytes. ZigZag decoding uses wrapping arithmetic (no UB). |
| **Deep nesting** | Configurable `max_nesting_depth` (default: 128, strict: 32). Exceeded depth returns `NestingTooDeep` error. |
| **Memory exhaustion** | Per-session `max_memory` limit (default: 256 MiB). Array/object pre-allocation capped at 1024 elements. |
| **Item count bomb** | `max_items` limit (default: 1M) prevents allocation of enormous arrays/objects from small input. |
| **Recursive references** | Reference wire type currently decoded as integer ID. Full resolution will validate against a bounded reference table. |
| **Malformed varints** | Decoder rejects truncated varints (`UnexpectedEof`) and overlong encodings (`VarintOverflow`). |
| **Invalid UTF-8** | All string fields are validated with `std::str::from_utf8`. Invalid sequences produce `InvalidUtf8` error. |
| **Checksum bypass** | Per-block XXH64 checksums are verified before payload processing. Corrupted blocks are rejected. |
| **Compression bombs** | Decompression output is bounded by `max_block_size`. Snappy provides pre-decompression length check. |

### Resource Limits (Configurable)

```rust
use crous_core::Limits;

// For untrusted network input:
let limits = Limits::strict();
// max_nesting_depth: 32
// max_block_size: 1 MiB
// max_items: 10,000
// max_memory: 4 MiB
// max_string_length: 64 KiB
```

### Safe Rust Policy

- The core encoder/decoder (`crous-core`) uses **100% safe Rust**.
- The FFI crate (`crous-ffi`) uses `unsafe` at the C boundary only, with documented safety contracts.
- No `unsafe` in parsing, varint decoding, or checksum computation.

### Fuzzing

Fuzzing targets cover:
- `Decoder::decode_all_owned()` — arbitrary binary input
- `text::parse()` — arbitrary text input
- Varint decoding — malformed varint sequences
- Block framing — truncated/corrupted blocks

Run fuzzing:
```bash
cd fuzz
cargo +nightly fuzz run fuzz_decode -- -max_total_time=3600
```

### Reporting Vulnerabilities

If you discover a security vulnerability, please report it privately via GitHub Security Advisories or email to [security@crous-format.org].

Do NOT open a public issue for security vulnerabilities.

## Security Audit Checklist

Before each release:
- [ ] Run `cargo audit` — no known vulnerabilities
- [ ] Run fuzzing for ≥1 hour with no crashes
- [ ] Review any new `unsafe` blocks
- [ ] Verify all limits are enforced in tests
- [ ] Check for panics in error paths (should return `Result`)
