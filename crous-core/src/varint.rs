//! Varint encoding: unsigned LEB128 and signed ZigZag + LEB128.
//!
//! ## Why LEB128?
//! LEB128 is the standard variable-length integer encoding used by protobuf,
//! DWARF, WebAssembly, and many other formats. It encodes small integers in
//! fewer bytes (1 byte for 0–127) while supporting the full u64 range in at
//! most 10 bytes. This makes it ideal for field IDs, lengths, and small counts
//! which dominate typical structured data.
//!
//! ## Why ZigZag for signed integers?
//! ZigZag maps signed integers to unsigned integers so that small-magnitude
//! numbers (both positive and negative) produce small varint encodings.
//! Without ZigZag, -1 would encode as a 10-byte varint (all bits set).
//! With ZigZag: -1 → 1, 1 → 2, -2 → 3, 2 → 4, etc.

use crate::error::{CrousError, Result};

// ---------------------------------------------------------------------------
// ZigZag encoding/decoding
// ---------------------------------------------------------------------------

/// Encode a signed 64-bit integer using ZigZag encoding.
///
/// Maps signed integers to unsigned: 0→0, -1→1, 1→2, -2→3, 2→4, ...
///
/// ```
/// use crous_core::varint::zigzag_encode;
/// assert_eq!(zigzag_encode(0), 0);
/// assert_eq!(zigzag_encode(-1), 1);
/// assert_eq!(zigzag_encode(1), 2);
/// assert_eq!(zigzag_encode(-2), 3);
/// assert_eq!(zigzag_encode(i64::MIN), u64::MAX);
/// ```
#[inline]
pub fn zigzag_encode(n: i64) -> u64 {
    ((n << 1) ^ (n >> 63)) as u64
}

/// Decode a ZigZag-encoded unsigned integer back to signed.
///
/// ```
/// use crous_core::varint::zigzag_decode;
/// assert_eq!(zigzag_decode(0), 0);
/// assert_eq!(zigzag_decode(1), -1);
/// assert_eq!(zigzag_decode(2), 1);
/// assert_eq!(zigzag_decode(3), -2);
/// ```
#[inline]
pub fn zigzag_decode(n: u64) -> i64 {
    ((n >> 1) as i64) ^ (-((n & 1) as i64))
}

// ---------------------------------------------------------------------------
// LEB128 unsigned varint
// ---------------------------------------------------------------------------

/// Encode an unsigned 64-bit integer as LEB128 into `buf`.
/// Returns the number of bytes written (1–10).
///
/// ```
/// use crous_core::varint::encode_varint;
/// let mut buf = [0u8; 10];
/// assert_eq!(encode_varint(0, &mut buf), 1);
/// assert_eq!(buf[0], 0x00);
///
/// assert_eq!(encode_varint(127, &mut buf), 1);
/// assert_eq!(buf[0], 0x7f);
///
/// assert_eq!(encode_varint(128, &mut buf), 2);
/// assert_eq!(&buf[..2], &[0x80, 0x01]);
///
/// assert_eq!(encode_varint(300, &mut buf), 2);
/// assert_eq!(&buf[..2], &[0xac, 0x02]);
/// ```
#[inline]
pub fn encode_varint(mut value: u64, buf: &mut [u8; 10]) -> usize {
    let mut i = 0;
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf[i] = byte;
        i += 1;
        if value == 0 {
            break;
        }
    }
    i
}

/// Encode a varint and append it to a `Vec<u8>`.
#[inline]
pub fn encode_varint_vec(value: u64, out: &mut Vec<u8>) {
    let mut buf = [0u8; 10];
    let n = encode_varint(value, &mut buf);
    out.extend_from_slice(&buf[..n]);
}

/// Decode an unsigned LEB128 varint from `data` starting at `offset`.
/// Returns `(value, bytes_consumed)`.
///
/// # Errors
/// - `VarintOverflow` if the varint exceeds 10 bytes (64-bit limit).
/// - `UnexpectedEof` if the data ends before the varint is complete.
///
/// ```
/// use crous_core::varint::decode_varint;
/// let data = [0xac, 0x02];
/// let (val, len) = decode_varint(&data, 0).unwrap();
/// assert_eq!(val, 300);
/// assert_eq!(len, 2);
/// ```
#[inline]
pub fn decode_varint(data: &[u8], offset: usize) -> Result<(u64, usize)> {
    let mut result: u64 = 0;
    let mut shift: u32 = 0;
    let mut i = 0usize;

    loop {
        if offset + i >= data.len() {
            return Err(CrousError::UnexpectedEof(offset + i));
        }
        let byte = data[offset + i];
        i += 1;

        // Check for overflow: LEB128 for u64 is at most 10 bytes,
        // and the 10th byte must have value ≤ 1 (bit 64).
        if i > 10 {
            return Err(CrousError::VarintOverflow);
        }
        if i == 10 && byte > 0x01 {
            return Err(CrousError::VarintOverflow);
        }

        result |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok((result, i));
        }
        shift += 7;
    }
}

/// Convenience: encode a signed i64 as ZigZag + LEB128 into a Vec.
#[inline]
pub fn encode_signed_varint_vec(value: i64, out: &mut Vec<u8>) {
    encode_varint_vec(zigzag_encode(value), out);
}

/// Convenience: decode a ZigZag + LEB128 signed i64 from `data` at `offset`.
#[inline]
pub fn decode_signed_varint(data: &[u8], offset: usize) -> Result<(i64, usize)> {
    let (raw, len) = decode_varint(data, offset)?;
    Ok((zigzag_decode(raw), len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zigzag_roundtrip() {
        for &v in &[0i64, 1, -1, 2, -2, 127, -128, i64::MAX, i64::MIN] {
            assert_eq!(
                zigzag_decode(zigzag_encode(v)),
                v,
                "ZigZag roundtrip failed for {v}"
            );
        }
    }

    #[test]
    fn varint_single_byte() {
        let mut buf = [0u8; 10];
        for v in 0..=127u64 {
            let n = encode_varint(v, &mut buf);
            assert_eq!(
                n, 1,
                "values 0–127 should encode in 1 byte, got {n} for {v}"
            );
            assert_eq!(buf[0], v as u8);
            let (decoded, consumed) = decode_varint(&buf[..n], 0).unwrap();
            assert_eq!(decoded, v);
            assert_eq!(consumed, 1);
        }
    }

    #[test]
    fn varint_multi_byte() {
        let test_cases: &[(u64, &[u8])] = &[
            (128, &[0x80, 0x01]),
            (300, &[0xac, 0x02]),
            (16384, &[0x80, 0x80, 0x01]),
            (
                u64::MAX,
                &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01],
            ),
        ];
        for &(value, expected) in test_cases {
            let mut buf = [0u8; 10];
            let n = encode_varint(value, &mut buf);
            assert_eq!(&buf[..n], expected, "encoding mismatch for {value}");
            let (decoded, consumed) = decode_varint(expected, 0).unwrap();
            assert_eq!(decoded, value, "decode mismatch for {value}");
            assert_eq!(consumed, expected.len());
        }
    }

    #[test]
    fn varint_overflow_detection() {
        // 11 continuation bytes — definitely too long.
        let bad = [0x80u8; 11];
        assert!(decode_varint(&bad, 0).is_err());
    }

    #[test]
    fn varint_unexpected_eof() {
        // Continuation bit set but no more data.
        let truncated = [0x80u8];
        assert!(decode_varint(&truncated, 0).is_err());
    }

    #[test]
    fn signed_varint_roundtrip() {
        for &v in &[0i64, 1, -1, 42, -42, 1000, -1000, i64::MAX, i64::MIN] {
            let mut buf = Vec::new();
            encode_signed_varint_vec(v, &mut buf);
            let (decoded, _) = decode_signed_varint(&buf, 0).unwrap();
            assert_eq!(decoded, v, "signed varint roundtrip failed for {v}");
        }
    }
}
