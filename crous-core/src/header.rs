//! File header for the Crous binary format.
//!
//! Layout (8 bytes total):
//! ```text
//! Offset  Size  Description
//! 0       7     ASCII magic "CROUSv1"
//! 7       1     Flags byte
//! ```
//!
//! Flags byte layout:
//! ```text
//! Bit 0:   Reserved (must be 0)
//! Bit 1:   Has index block (1 = yes)
//! Bit 2:   Has schema block (1 = yes)
//! Bit 3-7: Reserved (must be 0)
//! ```

use crate::error::{CrousError, Result};

/// The 7-byte magic identifying a Crous file.
pub const MAGIC: &[u8; 7] = b"CROUSv1";

/// Header size in bytes.
pub const HEADER_SIZE: usize = 8;

/// No flags set.
pub const FLAGS_NONE: u8 = 0x00;
/// Flag: file contains an index block.
pub const FLAGS_HAS_INDEX: u8 = 0x02;
/// Flag: file contains a schema block.
pub const FLAGS_HAS_SCHEMA: u8 = 0x04;

/// Parsed file header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHeader {
    /// Flags byte.
    pub flags: u8,
}

impl FileHeader {
    /// Create a new header with the given flags.
    pub fn new(flags: u8) -> Self {
        Self { flags }
    }

    /// Serialize the header to exactly 8 bytes.
    pub fn encode(&self) -> [u8; HEADER_SIZE] {
        let mut buf = [0u8; HEADER_SIZE];
        buf[..7].copy_from_slice(MAGIC);
        buf[7] = self.flags;
        buf
    }

    /// Parse a header from exactly 8 bytes.
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < HEADER_SIZE {
            return Err(CrousError::UnexpectedEof(data.len()));
        }
        if &data[..7] != MAGIC {
            return Err(CrousError::InvalidMagic);
        }
        Ok(Self { flags: data[7] })
    }

    /// Check if the file has an index block.
    pub fn has_index(&self) -> bool {
        self.flags & FLAGS_HAS_INDEX != 0
    }

    /// Check if the file has a schema block.
    pub fn has_schema(&self) -> bool {
        self.flags & FLAGS_HAS_SCHEMA != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let hdr = FileHeader::new(FLAGS_HAS_INDEX | FLAGS_HAS_SCHEMA);
        let bytes = hdr.encode();
        assert_eq!(&bytes[..7], b"CROUSv1");
        assert_eq!(bytes[7], 0x06);
        let decoded = FileHeader::decode(&bytes).unwrap();
        assert_eq!(decoded, hdr);
        assert!(decoded.has_index());
        assert!(decoded.has_schema());
    }

    #[test]
    fn header_invalid_magic() {
        let bad = b"CROUSv2\x00";
        assert!(matches!(
            FileHeader::decode(bad),
            Err(CrousError::InvalidMagic)
        ));
    }

    #[test]
    fn header_too_short() {
        let short = b"CROUS";
        assert!(FileHeader::decode(short).is_err());
    }
}
