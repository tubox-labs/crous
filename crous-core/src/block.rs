//! Block-level framing for the Crous binary format.
//!
//! Blocks are the fundamental container for data in a Crous file.
//! Each block has:
//! - A type byte identifying the block kind (data, index, schema, etc.)
//! - A varint-encoded length
//! - A compression type byte
//! - An 8-byte XXH64 checksum of the (uncompressed) payload
//! - The payload bytes
//!
//! This module provides `BlockWriter` for building blocks incrementally
//! and `BlockReader` for reading them from a byte slice.

use crate::checksum::compute_xxh64;
use crate::error::{CrousError, Result};
use crate::varint::{decode_varint, encode_varint_vec};
use crate::wire::{BlockType, CompressionType};

/// Builder for a single block's payload, with framing support.
pub struct BlockWriter {
    block_type: BlockType,
    compression: CompressionType,
    payload: Vec<u8>,
}

impl BlockWriter {
    /// Create a new block writer of the given type.
    pub fn new(block_type: BlockType) -> Self {
        Self {
            block_type,
            compression: CompressionType::None,
            payload: Vec::with_capacity(4096),
        }
    }

    /// Set the compression type for this block.
    pub fn set_compression(&mut self, comp: CompressionType) {
        self.compression = comp;
    }

    /// Write raw bytes into the block payload.
    pub fn write(&mut self, data: &[u8]) {
        self.payload.extend_from_slice(data);
    }

    /// Get a mutable reference to the payload buffer.
    pub fn payload_mut(&mut self) -> &mut Vec<u8> {
        &mut self.payload
    }

    /// Get the current payload size.
    pub fn payload_len(&self) -> usize {
        self.payload.len()
    }

    /// Finalize and serialize this block into a byte vector.
    ///
    /// Layout: `block_type(1)` | `block_len(varint)` | `comp_type(1)` | `checksum(8)` | `payload`
    pub fn finish(self) -> Vec<u8> {
        let checksum = compute_xxh64(&self.payload);
        let mut out = Vec::with_capacity(1 + 10 + 1 + 8 + self.payload.len());

        out.push(self.block_type as u8);
        encode_varint_vec(self.payload.len() as u64, &mut out);
        out.push(self.compression as u8);
        out.extend_from_slice(&checksum.to_le_bytes());
        out.extend_from_slice(&self.payload);

        out
    }
}

/// A parsed block read from binary data.
#[derive(Debug)]
pub struct BlockReader<'a> {
    /// The block type.
    pub block_type: BlockType,
    /// The compression type.
    pub compression: CompressionType,
    /// The expected checksum.
    pub checksum: u64,
    /// The payload bytes (borrowed from input).
    pub payload: &'a [u8],
}

impl<'a> BlockReader<'a> {
    /// Parse a block from `data` starting at `offset`.
    /// Returns `(BlockReader, bytes_consumed)`.
    pub fn parse(data: &'a [u8], offset: usize) -> Result<(Self, usize)> {
        let mut pos = offset;

        if pos >= data.len() {
            return Err(CrousError::UnexpectedEof(pos));
        }

        let block_type_byte = data[pos];
        pos += 1;
        let block_type = BlockType::from_byte(block_type_byte)
            .ok_or(CrousError::InvalidBlockType(block_type_byte))?;

        let (block_len, varint_bytes) = decode_varint(data, pos)?;
        pos += varint_bytes;
        let block_len = block_len as usize;

        if pos >= data.len() {
            return Err(CrousError::UnexpectedEof(pos));
        }
        let comp_byte = data[pos];
        pos += 1;
        let compression = CompressionType::from_byte(comp_byte)
            .ok_or(CrousError::UnknownCompression(comp_byte))?;

        if pos + 8 > data.len() {
            return Err(CrousError::UnexpectedEof(pos));
        }
        let checksum = u64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
        pos += 8;

        if pos + block_len > data.len() {
            return Err(CrousError::UnexpectedEof(pos + block_len));
        }
        let payload = &data[pos..pos + block_len];
        pos += block_len;

        Ok((
            Self {
                block_type,
                compression,
                checksum,
                payload,
            },
            pos - offset,
        ))
    }

    /// Verify the block's checksum matches its payload.
    pub fn verify_checksum(&self) -> bool {
        compute_xxh64(self.payload) == self.checksum
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_roundtrip() {
        let mut writer = BlockWriter::new(BlockType::Data);
        writer.write(b"hello world");
        let bytes = writer.finish();

        let (reader, consumed) = BlockReader::parse(&bytes, 0).unwrap();
        assert_eq!(consumed, bytes.len());
        assert_eq!(reader.block_type, BlockType::Data);
        assert_eq!(reader.compression, CompressionType::None);
        assert_eq!(reader.payload, b"hello world");
        assert!(reader.verify_checksum());
    }

    #[test]
    fn block_checksum_failure() {
        let mut writer = BlockWriter::new(BlockType::Data);
        writer.write(b"test data");
        let mut bytes = writer.finish();

        // Corrupt last byte of payload.
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;

        let (reader, _) = BlockReader::parse(&bytes, 0).unwrap();
        assert!(!reader.verify_checksum());
    }

    #[test]
    fn block_types() {
        for bt in [
            BlockType::Data,
            BlockType::Index,
            BlockType::Schema,
            BlockType::StringDict,
        ] {
            let writer = BlockWriter::new(bt);
            let bytes = writer.finish();
            let (reader, _) = BlockReader::parse(&bytes, 0).unwrap();
            assert_eq!(reader.block_type, bt);
        }
    }
}
