//! Wire types for the Crous binary format.
//!
//! Each field in a Crous document is prefixed with a tag byte encoding:
//! - Low 4 bits: wire type (the physical encoding of the data)
//! - High 4 bits: flags (reserved, currently used for null/optional markers)
//!
//! Wire types define *how* data is serialized on the wire, independent of
//! the logical schema type. This enables forward-compatible skipping of
//! unknown fields: a decoder that doesn't know a field's schema type can
//! still determine how many bytes to skip.

/// Wire type identifiers (low 4 bits of tag byte).
///
/// Design note: 16 possible wire types (4 bits). We use 11 currently and
/// reserve 5 for future use (e.g., decimal, timestamp, map).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WireType {
    /// Null value: no payload.
    Null = 0x00,
    /// Boolean: 1-byte payload (0x00 = false, 0x01 = true).
    Bool = 0x01,
    /// Unsigned integer: LEB128 varint payload.
    VarUInt = 0x02,
    /// Signed integer: ZigZag + LEB128 varint payload.
    VarInt = 0x03,
    /// 64-bit fixed-width: 8-byte little-endian payload (for f64, fixed i64/u64).
    Fixed64 = 0x04,
    /// Length-delimited: varint length prefix + raw bytes (strings, binary blobs).
    LenDelimited = 0x05,
    /// Start of an object/map: followed by field entries until EndObject.
    StartObject = 0x06,
    /// End of an object/map: no payload.
    EndObject = 0x07,
    /// Start of an array: followed by elements until EndArray.
    StartArray = 0x08,
    /// End of an array: no payload.
    EndArray = 0x09,
    /// Reference to a previously-seen value (dedup): varint reference ID payload.
    Reference = 0x0A,
}

impl WireType {
    /// Parse a wire type from the low 4 bits of a tag byte.
    pub fn from_tag(tag: u8) -> Option<WireType> {
        match tag & 0x0F {
            0x00 => Some(WireType::Null),
            0x01 => Some(WireType::Bool),
            0x02 => Some(WireType::VarUInt),
            0x03 => Some(WireType::VarInt),
            0x04 => Some(WireType::Fixed64),
            0x05 => Some(WireType::LenDelimited),
            0x06 => Some(WireType::StartObject),
            0x07 => Some(WireType::EndObject),
            0x08 => Some(WireType::StartArray),
            0x09 => Some(WireType::EndArray),
            0x0A => Some(WireType::Reference),
            _ => None,
        }
    }

    /// Encode this wire type as a tag byte (flags in high nibble are zero).
    pub fn to_tag(self) -> u8 {
        self as u8
    }

    /// Encode this wire type with flags in the high nibble.
    pub fn to_tag_with_flags(self, flags: u8) -> u8 {
        (flags << 4) | (self as u8)
    }
}

/// Tag byte flags (high 4 bits).
pub mod flags {
    /// No flags set.
    pub const NONE: u8 = 0x00;
    /// Field is a string dictionary reference (optimization hint).
    pub const STRING_DICT_REF: u8 = 0x01;
    /// Field has an inline schema annotation.
    pub const HAS_SCHEMA_ANNOTATION: u8 = 0x02;
}

/// Compression type identifiers for block headers.
/// Citation: https://facebook.github.io/zstd/ and https://github.com/lz4/lz4
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CompressionType {
    None = 0x00,
    Zstd = 0x01,
    Snappy = 0x02,
    /// LZ4 block compression: https://github.com/lz4/lz4
    Lz4 = 0x03,
}

impl CompressionType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x00 => Some(Self::None),
            0x01 => Some(Self::Zstd),
            0x02 => Some(Self::Snappy),
            0x03 => Some(Self::Lz4),
            _ => None,
        }
    }
}

/// Block type identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BlockType {
    /// Data block containing encoded values.
    Data = 0x01,
    /// Index block for random access.
    Index = 0x02,
    /// Schema block embedding type information.
    Schema = 0x03,
    /// String dictionary block for deduplication.
    StringDict = 0x04,
    /// File trailer/footer.
    Trailer = 0xFF,
}

impl BlockType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x01 => Some(Self::Data),
            0x02 => Some(Self::Index),
            0x03 => Some(Self::Schema),
            0x04 => Some(Self::StringDict),
            0xFF => Some(Self::Trailer),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_type_roundtrip() {
        for tag in 0x00..=0x0Au8 {
            let wt = WireType::from_tag(tag).unwrap();
            assert_eq!(wt.to_tag(), tag);
        }
    }

    #[test]
    fn unknown_wire_type() {
        assert!(WireType::from_tag(0x0B).is_none());
        assert!(WireType::from_tag(0x0F).is_none());
    }

    #[test]
    fn tag_with_flags() {
        let tag = WireType::VarUInt.to_tag_with_flags(flags::STRING_DICT_REF);
        assert_eq!(tag, 0x12); // 0x01 << 4 | 0x02
        assert_eq!(WireType::from_tag(tag), Some(WireType::VarUInt));
        assert_eq!(tag >> 4, flags::STRING_DICT_REF);
    }
}
