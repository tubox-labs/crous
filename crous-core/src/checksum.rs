//! Checksum utilities for Crous block integrity.
//!
//! Default: XXH64 — a non-cryptographic hash at near-memcpy speeds (~30 GB/s).
//!
//! Feature `xxh3`: Switches to XXH3-64 for ~2× throughput on modern CPUs.
//! Citation: XXH3 rationale — https://xxhash.com/
//!
//! Feature `compat-crc32`: Uses CRC32 for backward compatibility with legacy data.
//! Citation: crc32fast — https://docs.rs/crc32fast

use xxhash_rust::xxh64::xxh64;

/// Compute an XXH64 checksum of the given data with seed 0.
///
/// ```
/// use crous_core::checksum::compute_xxh64;
/// let hash = compute_xxh64(b"hello world");
/// assert_ne!(hash, 0); // Extremely unlikely to be zero.
/// ```
#[inline]
pub fn compute_xxh64(data: &[u8]) -> u64 {
    xxh64(data, 0)
}

/// Verify that the checksum of `data` matches `expected`.
#[inline]
pub fn verify_xxh64(data: &[u8], expected: u64) -> bool {
    compute_xxh64(data) == expected
}

// ---------------------------------------------------------------------------
// XXH3-64 (feature = "xxh3")
// ---------------------------------------------------------------------------

/// Compute an XXH3-64 checksum of the given data.
/// XXH3 offers ~2× throughput vs XXH64 on modern CPUs with SIMD.
/// Citation: https://xxhash.com/
#[cfg(feature = "xxh3")]
#[inline]
pub fn compute_xxh3(data: &[u8]) -> u64 {
    xxhash_rust::xxh3::xxh3_64(data)
}

/// Verify an XXH3-64 checksum.
#[cfg(feature = "xxh3")]
#[inline]
pub fn verify_xxh3(data: &[u8], expected: u64) -> bool {
    compute_xxh3(data) == expected
}

// ---------------------------------------------------------------------------
// CRC32 (feature = "compat-crc32")
// ---------------------------------------------------------------------------

/// Compute a CRC32 checksum (zero-extended to u64).
/// For backward compatibility with legacy Crous data.
/// Citation: https://docs.rs/crc32fast
#[cfg(feature = "compat-crc32")]
#[inline]
pub fn compute_crc32(data: &[u8]) -> u64 {
    crc32fast::hash(data) as u64
}

/// Verify a CRC32 checksum (stored zero-extended in u64).
#[cfg(feature = "compat-crc32")]
#[inline]
pub fn verify_crc32(data: &[u8], expected: u64) -> bool {
    compute_crc32(data) == expected
}

// ---------------------------------------------------------------------------
// Unified checksum API
// ---------------------------------------------------------------------------

/// Checksum algorithm identifier stored in block/file headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ChecksumAlgo {
    /// XXH64 (default, always available).
    Xxh64 = 0,
    /// XXH3-64 (feature = "xxh3").
    Xxh3 = 1,
    /// CRC32 zero-extended to u64 (feature = "compat-crc32").
    Crc32 = 2,
}

impl ChecksumAlgo {
    /// Compute a checksum using this algorithm.
    #[inline]
    pub fn compute(self, data: &[u8]) -> u64 {
        match self {
            ChecksumAlgo::Xxh64 => compute_xxh64(data),
            #[cfg(feature = "xxh3")]
            ChecksumAlgo::Xxh3 => compute_xxh3(data),
            #[cfg(not(feature = "xxh3"))]
            ChecksumAlgo::Xxh3 => compute_xxh64(data), // fallback
            #[cfg(feature = "compat-crc32")]
            ChecksumAlgo::Crc32 => compute_crc32(data),
            #[cfg(not(feature = "compat-crc32"))]
            ChecksumAlgo::Crc32 => compute_xxh64(data), // fallback
        }
    }

    /// Verify a checksum using this algorithm.
    #[inline]
    pub fn verify(self, data: &[u8], expected: u64) -> bool {
        self.compute(data) == expected
    }

    /// The default checksum algorithm based on enabled features.
    #[inline]
    pub fn default_algo() -> Self {
        #[cfg(feature = "xxh3")]
        {
            ChecksumAlgo::Xxh3
        }
        #[cfg(not(feature = "xxh3"))]
        {
            ChecksumAlgo::Xxh64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_deterministic() {
        let data = b"The quick brown fox jumps over the lazy dog";
        let h1 = compute_xxh64(data);
        let h2 = compute_xxh64(data);
        assert_eq!(h1, h2);
    }

    #[test]
    fn checksum_differs_for_different_data() {
        let h1 = compute_xxh64(b"aaa");
        let h2 = compute_xxh64(b"aab");
        assert_ne!(h1, h2);
    }

    #[test]
    fn verify_works() {
        let data = b"test data";
        let hash = compute_xxh64(data);
        assert!(verify_xxh64(data, hash));
        assert!(!verify_xxh64(data, hash.wrapping_add(1)));
    }

    #[test]
    fn algo_api() {
        let data = b"checksum algo test";
        let algo = ChecksumAlgo::Xxh64;
        let hash = algo.compute(data);
        assert!(algo.verify(data, hash));
        assert!(!algo.verify(data, hash ^ 1));
    }

    #[test]
    fn default_algo_works() {
        let data = b"default algo";
        let algo = ChecksumAlgo::default_algo();
        let hash = algo.compute(data);
        assert!(algo.verify(data, hash));
    }
}
