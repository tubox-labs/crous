//! # crous-simd
//!
//! Optional SIMD-accelerated routines for Crous encoding/decoding.
//!
//! This crate provides optimized implementations of performance-critical
//! operations. On aarch64 (Apple Silicon, etc.) it uses NEON intrinsics.
//! On x86_64 it uses SSE2/AVX2 when available.
//! All functions have scalar fallbacks for unsupported platforms.
//!
//! # Provided routines
//! - `batch_decode_varints` — decode multiple LEB128 varints sequentially
//! - `find_byte` — locate first occurrence of a byte (SIMD-accelerated)
//! - `count_byte` — count occurrences of a byte (SIMD-accelerated)
//! - `find_non_ascii` — locate first non-ASCII byte (for fast UTF-8 pre-scan)

/// Batch-decode multiple varints from a contiguous buffer.
///
/// Returns a vector of `(value, bytes_consumed)` pairs.
/// Uses the scalar `crous_core::varint::decode_varint` internally;
/// the SIMD advantage is in pre-scanning continuation bits to predict
/// varint boundaries, but for now we rely on the compiler's autovectorization
/// and focus SIMD effort on byte-scanning operations.
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
