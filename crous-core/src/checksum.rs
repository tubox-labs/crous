//! Checksum utilities using XXH64.
//!
//! ## Why XXH64?
//! XXH64 is a non-cryptographic hash that runs at near-memcpy speeds
//! (~30 GB/s on modern hardware). It provides excellent collision resistance
//! for data integrity checks while adding negligible overhead to encode/decode.
//! Compared to CRC32 (weaker collision properties) or SHA-256 (much slower),
//! XXH64 is the sweet spot for format-level checksums.

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
pub fn verify_xxh64(data: &[u8], expected: u64) -> bool {
    compute_xxh64(data) == expected
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
}
