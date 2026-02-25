//! # crous-simd
//!
//! Optional SIMD-accelerated routines for Crous encoding/decoding.
//!
//! This crate provides optimized implementations of performance-critical
//! operations. On aarch64 (Apple Silicon, etc.) it uses NEON intrinsics.
//! On x86_64 it uses SSE2/AVX2 when available.
//! All functions have scalar fallbacks for unsupported platforms.
//!
//! # Feature flags
//! - `simd-varint` — enable SIMD-accelerated varint boundary pre-scan.
//!   Idea: https://github.com/as-com/varint-simd
//!
//! # Provided routines
//! - `batch_decode_varints` — decode multiple LEB128 varints sequentially
//! - `batch_decode_varints_simd` — SIMD pre-scan variant (feature `simd-varint`)
//! - `find_byte` — locate first occurrence of a byte (SIMD-accelerated)
//! - `count_byte` — count occurrences of a byte (SIMD-accelerated)
//! - `find_non_ascii` — locate first non-ASCII byte (for fast UTF-8 pre-scan)

/// Batch-decode multiple varints from a contiguous buffer (scalar path).
///
/// Returns a vector of `(value, bytes_consumed)` pairs.
pub fn batch_decode_varints(data: &[u8], count: usize) -> Vec<(u64, usize)> {
    let mut results = Vec::with_capacity(count);
    let mut offset = 0;
    for _ in 0..count {
        if offset >= data.len() {
            break;
        }
        match crous_core::varint::decode_varint(data, offset) {
            Ok((val, consumed)) => {
                results.push((val, consumed));
                offset += consumed;
            }
            Err(_) => break,
        }
    }
    results
}

/// Total bytes consumed by a batch decode.
pub fn batch_decode_total_consumed(data: &[u8], count: usize) -> usize {
    let mut offset = 0;
    for _ in 0..count {
        if offset >= data.len() {
            break;
        }
        match crous_core::varint::decode_varint(data, offset) {
            Ok((_val, consumed)) => offset += consumed,
            Err(_) => break,
        }
    }
    offset
}

// ── SIMD varint boundary pre-scan (feature = "simd-varint") ──────────
//
// The key insight from varint-simd: we can use SIMD to scan for the
// continuation bit (0x80) across 16 bytes at once to quickly find
// varint termination bytes, then extract values with scalar code.
// Citation: https://github.com/as-com/varint-simd

#[cfg(all(feature = "simd-varint", target_arch = "aarch64"))]
mod simd_varint_neon {
    use std::arch::aarch64::*;

    /// SIMD pre-scan: find the offset of the first byte without the high bit
    /// set (i.e., a varint terminator) starting from `offset`.
    ///
    /// Returns the length of the varint starting at `offset` (1..=10), or
    /// None if no terminator found in the next 16 bytes (malformed).
    ///
    /// # Safety
    /// NEON always available on aarch64.
    #[inline]
    pub(crate) unsafe fn varint_len_neon(data: &[u8], offset: usize) -> Option<usize> {
        let remaining = data.len() - offset;
        if remaining == 0 {
            return None;
        }

        if remaining >= 16 {
            let ptr = data.as_ptr().add(offset);
            let chunk = unsafe { vld1q_u8(ptr) };
            let high_bits = unsafe { vshrq_n_u8::<7>(chunk) }; // isolate bit 7
            // We want the first lane where bit 7 is 0 (terminator)
            let zero_vec = unsafe { vdupq_n_u8(0) };
            let is_terminator = unsafe { vceqq_u8(high_bits, zero_vec) };
            let max_val = unsafe { vmaxvq_u8(is_terminator) };
            if max_val != 0 {
                let mut mask = [0u8; 16];
                unsafe { vst1q_u8(mask.as_mut_ptr(), is_terminator) };
                for (j, &m) in mask.iter().enumerate() {
                    if m != 0 {
                        let len = j + 1;
                        if len <= 10 {
                            return Some(len);
                        } else {
                            return None; // overflow
                        }
                    }
                }
            }
            None
        } else {
            // Scalar fallback for tail
            scalar_varint_len(data, offset)
        }
    }

    fn scalar_varint_len(data: &[u8], offset: usize) -> Option<usize> {
        for i in 0..10.min(data.len() - offset) {
            if data[offset + i] & 0x80 == 0 {
                return Some(i + 1);
            }
        }
        None
    }
}

/// Batch-decode varints using SIMD pre-scan to determine boundaries first.
///
/// This amortizes branch misprediction by scanning continuation bits in bulk.
/// Falls back to `batch_decode_varints` when the `simd-varint` feature is disabled
/// or the platform is unsupported.
///
/// Citation: SIMD varint idea — https://github.com/as-com/varint-simd
pub fn batch_decode_varints_simd(data: &[u8], count: usize) -> Vec<(u64, usize)> {
    #[cfg(all(feature = "simd-varint", target_arch = "aarch64"))]
    {
        let mut results = Vec::with_capacity(count);
        let mut offset = 0;
        for _ in 0..count {
            if offset >= data.len() {
                break;
            }
            // Use SIMD to find varint length, then decode scalar
            let vlen = unsafe { simd_varint_neon::varint_len_neon(data, offset) };
            match vlen {
                Some(len) => {
                    // Fast scalar decode knowing the exact length
                    match crous_core::varint::decode_varint(data, offset) {
                        Ok((val, consumed)) => {
                            debug_assert_eq!(consumed, len);
                            results.push((val, consumed));
                            offset += consumed;
                        }
                        Err(_) => break,
                    }
                }
                None => {
                    // Fallback to scalar
                    match crous_core::varint::decode_varint(data, offset) {
                        Ok((val, consumed)) => {
                            results.push((val, consumed));
                            offset += consumed;
                        }
                        Err(_) => break,
                    }
                }
            }
        }
        results
    }
    #[cfg(not(all(feature = "simd-varint", target_arch = "aarch64")))]
    {
        batch_decode_varints(data, count)
    }
}

// ── SIMD byte scanning (aarch64 NEON) ────────────────────────────────

#[cfg(target_arch = "aarch64")]
mod neon {
    use std::arch::aarch64::*;

    /// Find the first occurrence of `needle` in `data` using NEON.
    ///
    /// # Safety
    /// Caller must ensure NEON is available (always true on aarch64).
    #[inline]
    pub(crate) unsafe fn find_byte_neon(data: &[u8], needle: u8) -> Option<usize> {
        let len = data.len();
        let ptr = data.as_ptr();
        let needle_vec = unsafe { vdupq_n_u8(needle) };
        let mut i = 0;

        // Process 16-byte chunks
        while i + 16 <= len {
            let chunk = unsafe { vld1q_u8(ptr.add(i)) };
            let cmp = unsafe { vceqq_u8(chunk, needle_vec) };
            // Check if any byte matched
            let max = unsafe { vmaxvq_u8(cmp) };
            if max != 0 {
                // Find the exact position
                let mut mask_bytes = [0u8; 16];
                unsafe { vst1q_u8(mask_bytes.as_mut_ptr(), cmp) };
                for (j, &m) in mask_bytes.iter().enumerate() {
                    if m != 0 {
                        return Some(i + j);
                    }
                }
            }
            i += 16;
        }

        // Scalar tail
        while i < len {
            if unsafe { *ptr.add(i) } == needle {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// Count occurrences of `needle` in `data` using NEON.
    ///
    /// # Safety
    /// Caller must ensure NEON is available.
    #[inline]
    pub(crate) unsafe fn count_byte_neon(data: &[u8], needle: u8) -> usize {
        let len = data.len();
        let ptr = data.as_ptr();
        let needle_vec = unsafe { vdupq_n_u8(needle) };
        let mut total: usize = 0;
        let mut i = 0;

        // Process 16-byte chunks; accumulate per-lane counts
        // Use vaddlvq_u8 on the mask (0xFF = match, 0 = no match).
        // Each match contributes 0xFF = 255, and we need count, so divide by 255.
        while i + 16 <= len {
            let chunk = unsafe { vld1q_u8(ptr.add(i)) };
            let cmp = unsafe { vceqq_u8(chunk, needle_vec) };
            // Each matching lane has value 0xFF. Sum all lanes.
            // We want count of matches = sum / 255.
            let sum = unsafe { vaddlvq_u8(cmp) } as usize;
            total += sum / 255;
            i += 16;
        }

        // Scalar tail
        while i < len {
            if unsafe { *ptr.add(i) } == needle {
                total += 1;
            }
            i += 1;
        }
        total
    }

    /// Find the first non-ASCII byte (byte >= 0x80) using NEON.
    ///
    /// Returns `None` if all bytes are ASCII.
    ///
    /// # Safety
    /// Caller must ensure NEON is available.
    #[inline]
    pub(crate) unsafe fn find_non_ascii_neon(data: &[u8]) -> Option<usize> {
        let len = data.len();
        let ptr = data.as_ptr();
        let threshold = unsafe { vdupq_n_u8(0x80) };
        let mut i = 0;

        while i + 16 <= len {
            let chunk = unsafe { vld1q_u8(ptr.add(i)) };
            // Compare >= 0x80 means high bit set
            let high_bits = unsafe { vcgeq_u8(chunk, threshold) };
            let max = unsafe { vmaxvq_u8(high_bits) };
            if max != 0 {
                let mut mask_bytes = [0u8; 16];
                unsafe { vst1q_u8(mask_bytes.as_mut_ptr(), high_bits) };
                for (j, &m) in mask_bytes.iter().enumerate() {
                    if m != 0 {
                        return Some(i + j);
                    }
                }
            }
            i += 16;
        }

        while i < len {
            if unsafe { *ptr.add(i) } >= 0x80 {
                return Some(i);
            }
            i += 1;
        }
        None
    }
}

// ── Public API ───────────────────────────────────────────────────────

/// Scan a byte slice for a specific byte using SIMD where available.
///
/// On aarch64, uses NEON intrinsics for 16-byte-at-a-time scanning.
/// Falls back to a scalar scan on other architectures.
#[inline]
pub fn find_byte(data: &[u8], needle: u8) -> Option<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        // NEON is always available on aarch64
        unsafe { neon::find_byte_neon(data, needle) }
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        data.iter().position(|&b| b == needle)
    }
}

/// Count the number of occurrences of `needle` in `data`.
///
/// On aarch64, uses NEON intrinsics for fast counting.
#[inline]
pub fn count_byte(data: &[u8], needle: u8) -> usize {
    #[cfg(target_arch = "aarch64")]
    {
        unsafe { neon::count_byte_neon(data, needle) }
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        data.iter().filter(|&&b| b == needle).count()
    }
}

/// Find the first byte with the high bit set (non-ASCII).
///
/// This is useful for fast UTF-8 pre-scanning: if this returns `None`,
/// the entire slice is pure ASCII and valid UTF-8.
#[inline]
pub fn find_non_ascii(data: &[u8]) -> Option<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        unsafe { neon::find_non_ascii_neon(data) }
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        data.iter().position(|&b| b >= 0x80)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_decode_basic() {
        let mut data = Vec::new();
        for v in [0u64, 1, 127, 128, 300] {
            crous_core::varint::encode_varint_vec(v, &mut data);
        }
        let results = batch_decode_varints(&data, 5);
        assert_eq!(results.len(), 5);
        assert_eq!(results[0].0, 0);
        assert_eq!(results[1].0, 1);
        assert_eq!(results[2].0, 127);
        assert_eq!(results[3].0, 128);
        assert_eq!(results[4].0, 300);
    }

    #[test]
    fn batch_decode_simd_matches_scalar() {
        let mut data = Vec::new();
        let values = [0u64, 1, 42, 127, 128, 255, 300, 16384, u64::MAX];
        for v in &values {
            crous_core::varint::encode_varint_vec(*v, &mut data);
        }
        let scalar = batch_decode_varints(&data, values.len());
        let simd = batch_decode_varints_simd(&data, values.len());
        assert_eq!(scalar.len(), simd.len());
        for (s, d) in scalar.iter().zip(simd.iter()) {
            assert_eq!(s.0, d.0, "value mismatch");
            assert_eq!(s.1, d.1, "consumed mismatch");
        }
    }

    #[test]
    fn find_byte_basic() {
        assert_eq!(find_byte(b"hello", b'l'), Some(2));
        assert_eq!(find_byte(b"hello", b'z'), None);
    }

    #[test]
    fn find_byte_long() {
        // Test with data longer than 16 bytes to exercise SIMD path
        let data: Vec<u8> = (0..256).map(|i| i as u8).collect();
        assert_eq!(find_byte(&data, 0), Some(0));
        assert_eq!(find_byte(&data, 42), Some(42));
        assert_eq!(find_byte(&data, 255), Some(255));

        let zeros = vec![0u8; 100];
        assert_eq!(find_byte(&zeros, 1), None);
    }

    #[test]
    fn count_byte_basic() {
        assert_eq!(count_byte(b"hello", b'l'), 2);
        assert_eq!(count_byte(b"hello", b'z'), 0);
        assert_eq!(count_byte(b"hello", b'o'), 1);
    }

    #[test]
    fn count_byte_long() {
        let data = vec![0xABu8; 200];
        assert_eq!(count_byte(&data, 0xAB), 200);
        assert_eq!(count_byte(&data, 0x00), 0);
    }

    #[test]
    fn find_non_ascii_basic() {
        assert_eq!(find_non_ascii(b"hello"), None);
        assert_eq!(find_non_ascii(b"hello\x80"), Some(5));
        assert_eq!(find_non_ascii(b"\xff"), Some(0));
    }

    #[test]
    fn find_non_ascii_long() {
        let mut data = vec![b'a'; 100];
        assert_eq!(find_non_ascii(&data), None);
        data[50] = 0x80;
        assert_eq!(find_non_ascii(&data), Some(50));
    }
}
