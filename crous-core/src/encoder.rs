//! Encoder for the Crous binary format.
//!
//! Encodes `Value` instances into the canonical Crous binary representation.
//! The encoder handles:
//! - File header emission
//! - Block framing with checksums
//! - Wire-type-tagged field encoding
//! - Varint/ZigZag integer encoding
//! - Length-delimited strings and bytes

use std::collections::HashMap;

use crate::checksum::compute_xxh64;
use crate::error::{CrousError, Result};
use crate::header::{FLAGS_NONE, FileHeader};
use crate::limits::Limits;
use crate::value::Value;
use crate::varint::encode_varint_vec;
use crate::wire::{BlockType, CompressionType, WireType};

/// Encoder that serializes `Value`s into Crous binary format.
///
/// # Example
/// ```
/// use crous_core::{Encoder, Value};
///
/// let mut enc = Encoder::new();
/// enc.encode_value(&Value::UInt(42)).unwrap();
/// let bytes = enc.finish().unwrap();
/// assert!(bytes.len() > 8); // header + block
/// ```
pub struct Encoder {
    /// The output buffer accumulating the binary output.
    output: Vec<u8>,
    /// Buffer for the current block's payload (before framing).
    block_buf: Vec<u8>,
    /// Current nesting depth (for overflow protection).
    depth: usize,
    /// Resource limits.
    limits: Limits,
    /// Whether the file header has been written.
    header_written: bool,
    /// File header flags.
    flags: u8,
    /// Compression type for blocks.
    compression: CompressionType,
    /// Per-block string dictionary: string → index.
    /// When `dedup_strings` is true, repeated strings are encoded as Reference.
    string_dict: HashMap<String, u32>,
    /// Whether to enable string deduplication.
    dedup_strings: bool,
}

impl Encoder {
    /// Create a new encoder with default settings.
    pub fn new() -> Self {
        Self {
            output: Vec::with_capacity(4096),
            block_buf: Vec::with_capacity(4096),
            depth: 0,
            limits: Limits::default(),
            header_written: false,
            flags: FLAGS_NONE,
            compression: CompressionType::None,
            string_dict: HashMap::new(),
            dedup_strings: false,
        }
    }

    /// Create an encoder with custom limits.
    pub fn with_limits(limits: Limits) -> Self {
        Self {
            limits,
            ..Self::new()
        }
    }

    /// Enable string deduplication. Repeated strings within a block
    /// will be encoded as Reference wire types pointing to the dictionary.
    pub fn enable_dedup(&mut self) {
        self.dedup_strings = true;
    }

    /// Set the compression type for subsequent blocks.
    pub fn set_compression(&mut self, comp: CompressionType) {
        self.compression = comp;
    }

    /// Set the file header flags.
    pub fn set_flags(&mut self, flags: u8) {
        self.flags = flags;
    }

    /// Ensure the file header has been written.
    fn ensure_header(&mut self) {
        if !self.header_written {
            let header = FileHeader::new(self.flags);
            self.output.extend_from_slice(&header.encode());
            self.header_written = true;
        }
    }

    /// Encode a single `Value` into the current block buffer.
    ///
    /// This is the main entry point for encoding. Values are accumulated
    /// in the block buffer; call `finish()` to flush and produce the final bytes.
    pub fn encode_value(&mut self, value: &Value) -> Result<()> {
        self.encode_value_inner(value)
    }

    fn encode_value_inner(&mut self, value: &Value) -> Result<()> {
        match value {
            Value::Null => {
                self.block_buf.push(WireType::Null.to_tag());
            }
            Value::Bool(b) => {
                self.block_buf.push(WireType::Bool.to_tag());
                self.block_buf.push(if *b { 0x01 } else { 0x00 });
            }
            Value::UInt(n) => {
                self.block_buf.push(WireType::VarUInt.to_tag());
                encode_varint_vec(*n, &mut self.block_buf);
            }
            Value::Int(n) => {
                self.block_buf.push(WireType::VarInt.to_tag());
                crate::varint::encode_signed_varint_vec(*n, &mut self.block_buf);
            }
            Value::Float(f) => {
                self.block_buf.push(WireType::Fixed64.to_tag());
                self.block_buf.extend_from_slice(&f.to_le_bytes());
            }
            Value::Str(s) => {
                if self.dedup_strings {
                    if let Some(&idx) = self.string_dict.get(s.as_str()) {
                        // Emit a Reference to the dictionary entry.
                        self.block_buf.push(WireType::Reference.to_tag());
                        encode_varint_vec(idx as u64, &mut self.block_buf);
                        return Ok(());
                    }
                    // First occurrence: record in dictionary.
                    let idx = self.string_dict.len() as u32;
                    self.string_dict.insert(s.clone(), idx);
                }
                self.block_buf.push(WireType::LenDelimited.to_tag());
                // Sub-type marker: 0x00 = UTF-8 string
                self.block_buf.push(0x00);
                encode_varint_vec(s.len() as u64, &mut self.block_buf);
                self.block_buf.extend_from_slice(s.as_bytes());
            }
            Value::Bytes(b) => {
                self.block_buf.push(WireType::LenDelimited.to_tag());
                // Sub-type marker: 0x01 = raw binary
                self.block_buf.push(0x01);
                encode_varint_vec(b.len() as u64, &mut self.block_buf);
                self.block_buf.extend_from_slice(b);
            }
            Value::Array(items) => {
                if self.depth >= self.limits.max_nesting_depth {
                    return Err(CrousError::NestingTooDeep(
                        self.depth,
                        self.limits.max_nesting_depth,
                    ));
                }
                if items.len() > self.limits.max_items {
                    return Err(CrousError::TooManyItems(items.len(), self.limits.max_items));
                }
                self.block_buf.push(WireType::StartArray.to_tag());
                // Encode item count as a varint for fast skipping.
                encode_varint_vec(items.len() as u64, &mut self.block_buf);
                self.depth += 1;
                for item in items {
                    self.encode_value_inner(item)?;
                }
                self.depth -= 1;
                self.block_buf.push(WireType::EndArray.to_tag());
            }
            Value::Object(entries) => {
                if self.depth >= self.limits.max_nesting_depth {
                    return Err(CrousError::NestingTooDeep(
                        self.depth,
                        self.limits.max_nesting_depth,
                    ));
                }
                if entries.len() > self.limits.max_items {
                    return Err(CrousError::TooManyItems(
                        entries.len(),
                        self.limits.max_items,
                    ));
                }
                self.block_buf.push(WireType::StartObject.to_tag());
                // Encode entry count for fast skipping.
                encode_varint_vec(entries.len() as u64, &mut self.block_buf);
                self.depth += 1;
                for (key, val) in entries {
                    // Encode key as a length-delimited string inline.
                    encode_varint_vec(key.len() as u64, &mut self.block_buf);
                    self.block_buf.extend_from_slice(key.as_bytes());
                    // Encode value.
                    self.encode_value_inner(val)?;
                }
                self.depth -= 1;
                self.block_buf.push(WireType::EndObject.to_tag());
            }
        }
        Ok(())
    }

    /// Flush the current block buffer into a framed block and append to output.
    /// Returns the number of bytes in the flushed block.
    pub fn flush_block(&mut self) -> Result<usize> {
        if self.block_buf.is_empty() {
            return Ok(0);
        }

        self.ensure_header();

        let payload = &self.block_buf;
        let checksum = compute_xxh64(payload);

        // Block header:
        //   block_type (1B) | block_len (varint) | comp_type (1B) | checksum (8B) | payload
        let block_type = BlockType::Data as u8;
        let comp_type = self.compression as u8;
        let payload_len = payload.len();

        self.output.push(block_type);
        encode_varint_vec(payload_len as u64, &mut self.output);
        self.output.push(comp_type);
        self.output.extend_from_slice(&checksum.to_le_bytes());
        self.output.extend_from_slice(payload);

        let block_size = 1 + 1 + 8 + payload_len; // approximate (varint may be >1 byte)
        self.block_buf.clear();
        self.string_dict.clear(); // Reset per-block dictionary.
        Ok(block_size)
    }

    /// Finish encoding: flush remaining data and return the complete binary output.
    ///
    /// The output includes: file header + data blocks + file trailer checksum.
    pub fn finish(mut self) -> Result<Vec<u8>> {
        self.flush_block()?;

        // Write file trailer: XXH64 checksum over everything written so far.
        let overall_checksum = compute_xxh64(&self.output);
        // Trailer block: type=0xFF, length=8, no compression, checksum of checksum, payload=checksum
        self.output.push(BlockType::Trailer as u8);
        encode_varint_vec(8, &mut self.output);
        self.output.push(CompressionType::None as u8);
        let trailer_checksum = compute_xxh64(&overall_checksum.to_le_bytes());
        self.output
            .extend_from_slice(&trailer_checksum.to_le_bytes());
        self.output
            .extend_from_slice(&overall_checksum.to_le_bytes());

        Ok(self.output)
    }

    /// Get the current size of the output buffer (including unflushed block data).
    pub fn current_size(&self) -> usize {
        self.output.len() + self.block_buf.len()
    }

    /// Get access to the raw block buffer (for testing/inspection).
    pub fn block_buffer(&self) -> &[u8] {
        &self.block_buf
    }
}

impl Default for Encoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_null() {
        let mut enc = Encoder::new();
        enc.encode_value(&Value::Null).unwrap();
        assert_eq!(enc.block_buffer(), &[0x00]); // WireType::Null
    }

    #[test]
    fn encode_bool() {
        let mut enc = Encoder::new();
        enc.encode_value(&Value::Bool(true)).unwrap();
        assert_eq!(enc.block_buffer(), &[0x01, 0x01]);
        enc.block_buf.clear();
        enc.encode_value(&Value::Bool(false)).unwrap();
        assert_eq!(enc.block_buffer(), &[0x01, 0x00]);
    }

    #[test]
    fn encode_uint_small() {
        let mut enc = Encoder::new();
        enc.encode_value(&Value::UInt(42)).unwrap();
        assert_eq!(enc.block_buffer(), &[0x02, 42]); // WireType::VarUInt, 42
    }

    #[test]
    fn encode_uint_large() {
        let mut enc = Encoder::new();
        enc.encode_value(&Value::UInt(300)).unwrap();
        assert_eq!(enc.block_buffer(), &[0x02, 0xac, 0x02]);
    }

    #[test]
    fn encode_int_negative() {
        let mut enc = Encoder::new();
        enc.encode_value(&Value::Int(-1)).unwrap();
        // ZigZag(-1) = 1, LEB128(1) = 0x01
        assert_eq!(enc.block_buffer(), &[0x03, 0x01]);
    }

    #[test]
    fn encode_float() {
        let mut enc = Encoder::new();
        enc.encode_value(&Value::Float(3.125)).unwrap();
        let mut expected = vec![0x04];
        expected.extend_from_slice(&3.125f64.to_le_bytes());
        assert_eq!(enc.block_buffer(), &expected);
    }

    #[test]
    fn encode_string() {
        let mut enc = Encoder::new();
        enc.encode_value(&Value::Str("hello".into())).unwrap();
        // WireType::LenDelimited (0x05) + sub-type 0x00 + length 5 + "hello"
        let mut expected = vec![0x05, 0x00, 5];
        expected.extend_from_slice(b"hello");
        assert_eq!(enc.block_buffer(), &expected);
    }

    #[test]
    fn encode_bytes() {
        let mut enc = Encoder::new();
        enc.encode_value(&Value::Bytes(vec![0xDE, 0xAD])).unwrap();
        // WireType::LenDelimited (0x05) + sub-type 0x01 + length 2 + bytes
        assert_eq!(enc.block_buffer(), &[0x05, 0x01, 2, 0xDE, 0xAD]);
    }

    #[test]
    fn encode_array() {
        let mut enc = Encoder::new();
        let arr = Value::Array(vec![Value::UInt(1), Value::UInt(2)]);
        enc.encode_value(&arr).unwrap();
        // StartArray(0x08) + count(2) + UInt(1) + UInt(2) + EndArray(0x09)
        assert_eq!(
            enc.block_buffer(),
            &[0x08, 0x02, 0x02, 0x01, 0x02, 0x02, 0x09]
        );
    }

    #[test]
    fn encode_object() {
        let mut enc = Encoder::new();
        let obj = Value::Object(vec![("x".into(), Value::UInt(10))]);
        enc.encode_value(&obj).unwrap();
        // StartObject(0x06) + count(1) + key_len(1) + "x" + UInt(10) + EndObject(0x07)
        assert_eq!(
            enc.block_buffer(),
            &[0x06, 0x01, 0x01, b'x', 0x02, 0x0a, 0x07]
        );
    }

    #[test]
    fn finish_produces_valid_file() {
        let mut enc = Encoder::new();
        enc.encode_value(&Value::Null).unwrap();
        let bytes = enc.finish().unwrap();
        // Must start with magic
        assert_eq!(&bytes[..7], b"CROUSv1");
        // Trailer block is 19 bytes: type(1) + varint(1) + comp(1) + checksum(8) + payload(8)
        assert_eq!(bytes[bytes.len() - 19], BlockType::Trailer as u8);
    }

    #[test]
    fn nesting_depth_limit() {
        let mut enc = Encoder::with_limits(Limits {
            max_nesting_depth: 2,
            ..Limits::default()
        });
        // Nest 3 levels deep — should fail
        let val = Value::Array(vec![Value::Array(vec![Value::Array(vec![])])]);
        assert!(enc.encode_value(&val).is_err());
    }
}
