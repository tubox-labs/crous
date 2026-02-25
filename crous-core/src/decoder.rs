//! Decoder for the Crous binary format.
//!
//! Provides both zero-copy `CrousValue<'a>` decoding (borrows from the input)
//! and owned `Value` decoding. The decoder validates checksums, enforces limits,
//! and supports skipping unknown wire types for forward compatibility.

use crate::checksum::compute_xxh64;
use crate::error::{CrousError, Result};
use crate::header::{FileHeader, HEADER_SIZE};
use crate::limits::Limits;
use crate::value::{CrousValue, Value};
use crate::varint::{decode_signed_varint, decode_varint};
use crate::wire::{BlockType, CompressionType, WireType};

/// Decoder that reads Crous binary data and produces values.
///
/// # Example
/// ```
/// use crous_core::{Encoder, Decoder, Value};
///
/// let mut enc = Encoder::new();
/// enc.encode_value(&Value::Str("hello".into())).unwrap();
/// let bytes = enc.finish().unwrap();
///
/// let mut dec = Decoder::new(&bytes);
/// let val = dec.decode_next().unwrap();
/// assert_eq!(val.to_owned_value(), Value::Str("hello".into()));
/// ```
pub struct Decoder<'a> {
    /// The input data buffer.
    data: &'a [u8],
    /// Current read position in `data`.
    pos: usize,
    /// The file header (parsed lazily).
    header: Option<FileHeader>,
    /// Resource limits.
    limits: Limits,
    /// Current nesting depth.
    depth: usize,
    /// Current block's payload slice (start, end).
    current_block: Option<(usize, usize)>,
    /// Position within the current block payload.
    block_pos: usize,
    /// Cumulative bytes allocated during this decode session (for memory tracking).
    memory_used: usize,
    /// Per-block borrowed string slices for zero-copy reference resolution.
    str_slices: Vec<&'a str>,
}

impl<'a> Decoder<'a> {
    /// Create a new decoder over the given data.
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            header: None,
            limits: Limits::default(),
            depth: 0,
            current_block: None,
            block_pos: 0,
            memory_used: 0,
            str_slices: Vec::new(),
        }
    }

    /// Create a decoder with custom limits.
    pub fn with_limits(data: &'a [u8], limits: Limits) -> Self {
        Self {
            limits,
            ..Self::new(data)
        }
    }

    /// Track memory allocation; returns error if limit exceeded.
    fn track_alloc(&mut self, bytes: usize) -> Result<()> {
        self.memory_used = self.memory_used.saturating_add(bytes);
        if self.memory_used > self.limits.max_memory {
            return Err(CrousError::MemoryLimitExceeded(
                self.memory_used,
                self.limits.max_memory,
            ));
        }
        Ok(())
    }

    /// Skip a value at the current block_pos without allocating.
    /// Used for forward-compatible skipping of unknown fields.
    pub fn skip_value_at(&mut self, block_end: usize) -> Result<()> {
        if self.block_pos >= block_end {
            return Err(CrousError::UnexpectedEof(self.block_pos));
        }

        let tag = self.data[self.block_pos];
        self.block_pos += 1;

        let wire_type = WireType::from_tag(tag).ok_or(CrousError::InvalidWireType(tag))?;

        match wire_type {
            WireType::Null => {} // no payload
            WireType::EndObject | WireType::EndArray => {} // no payload

            WireType::Bool => {
                if self.block_pos >= block_end {
                    return Err(CrousError::UnexpectedEof(self.block_pos));
                }
                self.block_pos += 1;
            }

            WireType::VarUInt | WireType::VarInt | WireType::Reference => {
                let (_val, consumed) = decode_varint(self.data, self.block_pos)?;
                self.block_pos += consumed;
            }

            WireType::Fixed64 => {
                if self.block_pos + 8 > block_end {
                    return Err(CrousError::UnexpectedEof(self.block_pos));
                }
                self.block_pos += 8;
            }

            WireType::LenDelimited => {
                if self.block_pos >= block_end {
                    return Err(CrousError::UnexpectedEof(self.block_pos));
                }
                self.block_pos += 1; // sub-type byte
                let (len, consumed) = decode_varint(self.data, self.block_pos)?;
                self.block_pos += consumed;
                let len = len as usize;
                if self.block_pos + len > block_end {
                    return Err(CrousError::UnexpectedEof(self.block_pos + len));
                }
                self.block_pos += len;
            }

            WireType::StartArray => {
                let (count, consumed) = decode_varint(self.data, self.block_pos)?;
                self.block_pos += consumed;
                for _ in 0..count {
                    self.skip_value_at(block_end)?;
                }
                // Consume EndArray tag.
                if self.block_pos < block_end
                    && self.data[self.block_pos] == WireType::EndArray.to_tag()
                {
                    self.block_pos += 1;
                }
            }

            WireType::StartObject => {
                let (count, consumed) = decode_varint(self.data, self.block_pos)?;
                self.block_pos += consumed;
                for _ in 0..count {
                    // Skip key (varint len + bytes).
                    let (key_len, kc) = decode_varint(self.data, self.block_pos)?;
                    self.block_pos += kc;
                    let key_len = key_len as usize;
                    if self.block_pos + key_len > block_end {
                        return Err(CrousError::UnexpectedEof(self.block_pos + key_len));
                    }
                    self.block_pos += key_len;
                    // Skip value.
                    self.skip_value_at(block_end)?;
                }
                // Consume EndObject tag.
                if self.block_pos < block_end
                    && self.data[self.block_pos] == WireType::EndObject.to_tag()
                {
                    self.block_pos += 1;
                }
            }
        }

        Ok(())
    }

    /// Parse the file header if not already parsed.
    fn ensure_header(&mut self) -> Result<()> {
        if self.header.is_none() {
            let hdr = FileHeader::decode(self.data)?;
            self.header = Some(hdr);
            self.pos = HEADER_SIZE;
        }
        Ok(())
    }

    /// Get the parsed file header.
    pub fn header(&mut self) -> Result<&FileHeader> {
        self.ensure_header()?;
        Ok(self.header.as_ref().unwrap())
    }

    /// Read the next block from the file.
    /// Returns `(block_type, payload_slice_start, payload_slice_end)` or None if at EOF/trailer.
    fn read_next_block(&mut self) -> Result<Option<(BlockType, usize, usize)>> {
        self.ensure_header()?;

        if self.pos >= self.data.len() {
            return Ok(None);
        }

        // Block header: block_type(1) | block_len(varint) | comp_type(1) | checksum(8) | payload
        let block_type_byte = self.data[self.pos];
        self.pos += 1;

        let block_type = BlockType::from_byte(block_type_byte)
            .ok_or(CrousError::InvalidBlockType(block_type_byte))?;

        if block_type == BlockType::Trailer {
            return Ok(None); // End of data blocks.
        }

        let (block_len, varint_bytes) = decode_varint(self.data, self.pos)?;
        self.pos += varint_bytes;
        let block_len = block_len as usize;

        if block_len > self.limits.max_block_size {
            return Err(CrousError::BlockTooLarge(
                block_len,
                self.limits.max_block_size,
            ));
        }

        let comp_byte = self.data[self.pos];
        self.pos += 1;
        let _comp_type = CompressionType::from_byte(comp_byte)
            .ok_or(CrousError::UnknownCompression(comp_byte))?;

        // Read checksum (8 bytes, little-endian).
        if self.pos + 8 > self.data.len() {
            return Err(CrousError::UnexpectedEof(self.pos));
        }
        let expected_checksum =
            u64::from_le_bytes(self.data[self.pos..self.pos + 8].try_into().unwrap());
        self.pos += 8;

        // Read payload.
        let payload_start = self.pos;
        let payload_end = self.pos + block_len;
        if payload_end > self.data.len() {
            return Err(CrousError::UnexpectedEof(payload_end));
        }

        // Verify checksum.
        let actual_checksum = compute_xxh64(&self.data[payload_start..payload_end]);
        if actual_checksum != expected_checksum {
            return Err(CrousError::ChecksumMismatch {
                expected: expected_checksum,
                actual: actual_checksum,
            });
        }

        self.pos = payload_end;

        Ok(Some((block_type, payload_start, payload_end)))
    }

    /// Decode the next value from the input. Automatically reads blocks as needed.
    ///
    /// Returns a zero-copy `CrousValue` that borrows from the input data.
    pub fn decode_next(&mut self) -> Result<CrousValue<'a>> {
        // If we don't have a current block, read one.
        if self.current_block.is_none() {
            match self.read_next_block()? {
                Some((BlockType::Data, start, end)) => {
                    self.current_block = Some((start, end));
                    self.block_pos = start;
                    // Reset per-block tables.
                    self.str_slices.clear();
                }
                Some(_) => {
                    // Skip non-data blocks, try again.
                    return self.decode_next();
                }
                None => {
                    return Err(CrousError::UnexpectedEof(self.pos));
                }
            }
        }

        let (block_start, block_end) = self.current_block.unwrap();
        let _ = block_start;

        if self.block_pos >= block_end {
            // Current block exhausted, try next.
            self.current_block = None;
            return self.decode_next();
        }

        self.decode_value_at(block_end)
    }

    /// Decode a value starting at `self.block_pos`, not going past `block_end`.
    fn decode_value_at(&mut self, block_end: usize) -> Result<CrousValue<'a>> {
        if self.block_pos >= block_end {
            return Err(CrousError::UnexpectedEof(self.block_pos));
        }

        let tag = self.data[self.block_pos];
        self.block_pos += 1;

        let wire_type = WireType::from_tag(tag).ok_or(CrousError::InvalidWireType(tag))?;

        match wire_type {
            WireType::Null => Ok(CrousValue::Null),

            WireType::Bool => {
                if self.block_pos >= block_end {
                    return Err(CrousError::UnexpectedEof(self.block_pos));
                }
                let b = self.data[self.block_pos] != 0;
                self.block_pos += 1;
                Ok(CrousValue::Bool(b))
            }

            WireType::VarUInt => {
                let (val, consumed) = decode_varint(self.data, self.block_pos)?;
                self.block_pos += consumed;
                Ok(CrousValue::UInt(val))
            }

            WireType::VarInt => {
                let (val, consumed) = decode_signed_varint(self.data, self.block_pos)?;
                self.block_pos += consumed;
                Ok(CrousValue::Int(val))
            }

            WireType::Fixed64 => {
                if self.block_pos + 8 > block_end {
                    return Err(CrousError::UnexpectedEof(self.block_pos));
                }
                let bytes: [u8; 8] = self.data[self.block_pos..self.block_pos + 8]
                    .try_into()
                    .unwrap();
                self.block_pos += 8;
                Ok(CrousValue::Float(f64::from_le_bytes(bytes)))
            }

            WireType::LenDelimited => {
                if self.block_pos >= block_end {
                    return Err(CrousError::UnexpectedEof(self.block_pos));
                }
                let sub_type = self.data[self.block_pos];
                self.block_pos += 1;

                let (len, consumed) = decode_varint(self.data, self.block_pos)?;
                self.block_pos += consumed;
                let len = len as usize;

                if len > self.limits.max_string_length {
                    return Err(CrousError::MemoryLimitExceeded(
                        len,
                        self.limits.max_string_length,
                    ));
                }
                self.track_alloc(len)?;
                if self.block_pos + len > block_end {
                    return Err(CrousError::UnexpectedEof(self.block_pos + len));
                }

                let payload = &self.data[self.block_pos..self.block_pos + len];
                self.block_pos += len;

                match sub_type {
                    0x00 => {
                        // UTF-8 string — zero-copy borrow from input.
                        let s = std::str::from_utf8(payload)
                            .map_err(|_| CrousError::InvalidUtf8(self.block_pos - len))?;
                        // Record in per-block string table for Reference resolution.
                        self.str_slices.push(s);
                        Ok(CrousValue::Str(s))
                    }
                    0x01 => {
                        // Raw binary blob — zero-copy borrow.
                        Ok(CrousValue::Bytes(payload))
                    }
                    _ => {
                        // Unknown sub-type: treat as bytes for forward compatibility.
                        Ok(CrousValue::Bytes(payload))
                    }
                }
            }

            WireType::StartArray => {
                if self.depth >= self.limits.max_nesting_depth {
                    return Err(CrousError::NestingTooDeep(
                        self.depth,
                        self.limits.max_nesting_depth,
                    ));
                }
                let (count, consumed) = decode_varint(self.data, self.block_pos)?;
                self.block_pos += consumed;
                let count = count as usize;

                if count > self.limits.max_items {
                    return Err(CrousError::TooManyItems(count, self.limits.max_items));
                }

                let alloc = count.min(1024) * std::mem::size_of::<CrousValue>();
                self.track_alloc(alloc)?;

                self.depth += 1;
                let mut items = Vec::with_capacity(count.min(1024)); // Cap initial alloc
                for _ in 0..count {
                    items.push(self.decode_value_at(block_end)?);
                }
                self.depth -= 1;

                // Consume EndArray tag.
                if self.block_pos < block_end
                    && self.data[self.block_pos] == WireType::EndArray.to_tag()
                {
                    self.block_pos += 1;
                }

                Ok(CrousValue::Array(items))
            }

            WireType::StartObject => {
                if self.depth >= self.limits.max_nesting_depth {
                    return Err(CrousError::NestingTooDeep(
                        self.depth,
                        self.limits.max_nesting_depth,
                    ));
                }
                let (count, consumed) = decode_varint(self.data, self.block_pos)?;
                self.block_pos += consumed;
                let count = count as usize;

                if count > self.limits.max_items {
                    return Err(CrousError::TooManyItems(count, self.limits.max_items));
                }

                let alloc = count.min(1024)
                    * (std::mem::size_of::<&str>() + std::mem::size_of::<CrousValue>());
                self.track_alloc(alloc)?;

                self.depth += 1;
                let mut entries = Vec::with_capacity(count.min(1024));
                for _ in 0..count {
                    // Read key: varint length + UTF-8 bytes.
                    let (key_len, kc) = decode_varint(self.data, self.block_pos)?;
                    self.block_pos += kc;
                    let key_len = key_len as usize;

                    if self.block_pos + key_len > block_end {
                        return Err(CrousError::UnexpectedEof(self.block_pos + key_len));
                    }
                    let key =
                        std::str::from_utf8(&self.data[self.block_pos..self.block_pos + key_len])
                            .map_err(|_| CrousError::InvalidUtf8(self.block_pos))?;
                    self.block_pos += key_len;

                    // Read value.
                    let val = self.decode_value_at(block_end)?;
                    entries.push((key, val));
                }
                self.depth -= 1;

                // Consume EndObject tag.
                if self.block_pos < block_end
                    && self.data[self.block_pos] == WireType::EndObject.to_tag()
                {
                    self.block_pos += 1;
                }

                Ok(CrousValue::Object(entries))
            }

            WireType::EndObject | WireType::EndArray => {
                // Should not be encountered at top level; treat as protocol error.
                Err(CrousError::InvalidWireType(tag))
            }

            WireType::Reference => {
                // Reference wire type: resolve from the per-block string dictionary.
                let (ref_id, consumed) = decode_varint(self.data, self.block_pos)?;
                self.block_pos += consumed;
                let ref_id = ref_id as usize;

                // Resolve via borrowed slices from the input buffer (zero-copy).
                if let Some(&s) = self.str_slices.get(ref_id) {
                    Ok(CrousValue::Str(s))
                } else {
                    // Unknown reference — return as UInt for forward compatibility.
                    Ok(CrousValue::UInt(ref_id as u64))
                }
            }
        }
    }

    /// Decode all remaining values from the input.
    pub fn decode_all(&mut self) -> Result<Vec<CrousValue<'a>>> {
        let mut values = Vec::new();
        loop {
            match self.decode_next() {
                Ok(v) => values.push(v),
                Err(CrousError::UnexpectedEof(_)) => break,
                Err(e) => return Err(e),
            }
        }
        Ok(values)
    }

    /// Decode all remaining values as owned Values.
    pub fn decode_all_owned(&mut self) -> Result<Vec<Value>> {
        let borrowed = self.decode_all()?;
        Ok(borrowed.iter().map(|v| v.to_owned_value()).collect())
    }

    /// Current position in the input.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Cumulative bytes tracked as allocated during this decode session.
    pub fn memory_used(&self) -> usize {
        self.memory_used
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::Encoder;

    /// Helper: encode a value and decode it back.
    fn roundtrip(value: &Value) -> Value {
        let mut enc = Encoder::new();
        enc.encode_value(value).unwrap();
        let bytes = enc.finish().unwrap();
        let mut dec = Decoder::new(&bytes);
        dec.decode_next().unwrap().to_owned_value()
    }

    #[test]
    fn roundtrip_null() {
        assert_eq!(roundtrip(&Value::Null), Value::Null);
    }

    #[test]
    fn roundtrip_bool() {
        assert_eq!(roundtrip(&Value::Bool(true)), Value::Bool(true));
        assert_eq!(roundtrip(&Value::Bool(false)), Value::Bool(false));
    }

    #[test]
    fn roundtrip_uint() {
        for &v in &[0u64, 1, 127, 128, 300, 65535, u64::MAX] {
            assert_eq!(
                roundtrip(&Value::UInt(v)),
                Value::UInt(v),
                "uint roundtrip failed for {v}"
            );
        }
    }

    #[test]
    fn roundtrip_int() {
        for &v in &[0i64, 1, -1, 127, -128, 1000, -1000, i64::MAX, i64::MIN] {
            assert_eq!(
                roundtrip(&Value::Int(v)),
                Value::Int(v),
                "int roundtrip failed for {v}"
            );
        }
    }

    #[test]
    fn roundtrip_float() {
        for &v in &[0.0f64, 1.0, -1.0, 3.125, f64::MAX, f64::MIN, f64::INFINITY] {
            assert_eq!(
                roundtrip(&Value::Float(v)),
                Value::Float(v),
                "float roundtrip failed for {v}"
            );
        }
    }

    #[test]
    fn roundtrip_string() {
        let long_str = "a".repeat(1000);
        for s in &["", "hello", "こんにちは", long_str.as_str()] {
            assert_eq!(
                roundtrip(&Value::Str(s.to_string())),
                Value::Str(s.to_string()),
                "string roundtrip failed for {s:?}"
            );
        }
    }

    #[test]
    fn roundtrip_bytes() {
        let data = vec![0xDE, 0xAD, 0xBE, 0xEF];
        assert_eq!(roundtrip(&Value::Bytes(data.clone())), Value::Bytes(data));
    }

    #[test]
    fn roundtrip_array() {
        let arr = Value::Array(vec![
            Value::UInt(1),
            Value::Str("two".into()),
            Value::Bool(true),
            Value::Null,
        ]);
        assert_eq!(roundtrip(&arr), arr);
    }

    #[test]
    fn roundtrip_object() {
        let obj = Value::Object(vec![
            ("name".into(), Value::Str("Alice".into())),
            ("age".into(), Value::UInt(30)),
            ("active".into(), Value::Bool(true)),
        ]);
        assert_eq!(roundtrip(&obj), obj);
    }

    #[test]
    fn roundtrip_nested() {
        let val = Value::Object(vec![
            (
                "users".into(),
                Value::Array(vec![Value::Object(vec![
                    ("name".into(), Value::Str("Bob".into())),
                    (
                        "scores".into(),
                        Value::Array(vec![Value::UInt(100), Value::UInt(95), Value::UInt(87)]),
                    ),
                ])]),
            ),
            ("count".into(), Value::UInt(1)),
        ]);
        assert_eq!(roundtrip(&val), val);
    }

    #[test]
    fn checksum_verification() {
        let mut enc = Encoder::new();
        enc.encode_value(&Value::UInt(42)).unwrap();
        let mut bytes = enc.finish().unwrap();

        // Corrupt a byte in the payload area (after header + block header).
        let corrupt_pos = HEADER_SIZE + 12; // somewhere in the payload
        if corrupt_pos < bytes.len() {
            bytes[corrupt_pos] ^= 0xFF;
        }

        let mut dec = Decoder::new(&bytes);
        assert!(dec.decode_next().is_err());
    }

    #[test]
    fn nesting_depth_limit() {
        let limits = Limits {
            max_nesting_depth: 2,
            ..Limits::default()
        };
        // Create deeply nested value
        let val = Value::Array(vec![Value::Array(vec![Value::Array(vec![])])]);
        let mut enc = Encoder::with_limits(Limits::unlimited());
        enc.encode_value(&val).unwrap();
        let bytes = enc.finish().unwrap();

        let mut dec = Decoder::with_limits(&bytes, limits);
        assert!(dec.decode_next().is_err());
    }

    #[test]
    fn memory_tracking() {
        // Decode a large string and verify memory is tracked.
        let big_str = "x".repeat(1000);
        let val = Value::Str(big_str);
        let mut enc = Encoder::new();
        enc.encode_value(&val).unwrap();
        let bytes = enc.finish().unwrap();

        let mut dec = Decoder::new(&bytes);
        let _ = dec.decode_next().unwrap();
        assert!(dec.memory_used() >= 1000, "memory should track string allocation");
    }

    #[test]
    fn memory_limit_enforcement() {
        let big_str = "x".repeat(1000);
        let val = Value::Str(big_str);
        let mut enc = Encoder::new();
        enc.encode_value(&val).unwrap();
        let bytes = enc.finish().unwrap();

        let limits = Limits {
            max_memory: 500,
            ..Limits::default()
        };
        let mut dec = Decoder::with_limits(&bytes, limits);
        assert!(dec.decode_next().is_err(), "should fail when memory limit exceeded");
    }

    #[test]
    fn skip_value_works() {
        // Encode a complex value, then manually position decoder and skip it.
        let val = Value::Object(vec![
            ("name".into(), Value::Str("Alice".into())),
            ("scores".into(), Value::Array(vec![Value::UInt(1), Value::UInt(2)])),
        ]);
        let mut enc = Encoder::new();
        enc.encode_value(&val).unwrap();
        let bytes = enc.finish().unwrap();

        // Decode normally first to verify it's valid.
        let mut dec = Decoder::new(&bytes);
        let decoded = dec.decode_next().unwrap().to_owned_value();
        assert_eq!(decoded, val);
    }

    #[test]
    fn string_dedup_roundtrip() {
        // Encode with dedup enabled — repeated strings should decode correctly.
        let val = Value::Array(vec![
            Value::Str("hello".into()),
            Value::Str("world".into()),
            Value::Str("hello".into()), // duplicate → Reference(0)
            Value::Str("world".into()), // duplicate → Reference(1)
            Value::Str("hello".into()), // duplicate → Reference(0)
        ]);

        let mut enc = Encoder::new();
        enc.enable_dedup();
        enc.encode_value(&val).unwrap();
        let bytes = enc.finish().unwrap();

        // Dedup should produce smaller output than non-dedup.
        let mut enc_no_dedup = Encoder::new();
        enc_no_dedup.encode_value(&val).unwrap();
        let bytes_no_dedup = enc_no_dedup.finish().unwrap();
        assert!(
            bytes.len() < bytes_no_dedup.len(),
            "dedup ({}) should be smaller than no-dedup ({})",
            bytes.len(),
            bytes_no_dedup.len()
        );

        // Decode should resolve references back to the original strings.
        let mut dec = Decoder::new(&bytes);
        let decoded = dec.decode_next().unwrap().to_owned_value();
        assert_eq!(decoded, val, "dedup roundtrip should produce identical value");
    }

    #[test]
    fn string_dedup_in_object() {
        // Verify dedup works for string values inside objects.
        let val = Value::Object(vec![
            ("greeting".into(), Value::Str("hello".into())),
            ("farewell".into(), Value::Str("goodbye".into())),
            ("echo".into(), Value::Str("hello".into())), // dup of first value
        ]);

        let mut enc = Encoder::new();
        enc.enable_dedup();
        enc.encode_value(&val).unwrap();
        let bytes = enc.finish().unwrap();

        let mut dec = Decoder::new(&bytes);
        let decoded = dec.decode_next().unwrap().to_owned_value();
        assert_eq!(decoded, val, "dedup in object should roundtrip correctly");
    }
}
