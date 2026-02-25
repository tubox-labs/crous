//! Error types for Crous encoding/decoding operations.

use thiserror::Error;

/// All errors that can occur during Crous operations.
#[derive(Debug, Error)]
pub enum CrousError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid magic bytes in file header: expected CROUSv1")]
    InvalidMagic,

    #[error("Unsupported format version: {0}")]
    UnsupportedVersion(u8),

    #[error("Invalid wire type tag: 0x{0:02x}")]
    InvalidWireType(u8),

    #[error("Varint overflow: encoded integer exceeds 64 bits")]
    VarintOverflow,

    #[error("Unexpected end of input at offset {0}")]
    UnexpectedEof(usize),

    #[error("Checksum mismatch: expected 0x{expected:016x}, got 0x{actual:016x}")]
    ChecksumMismatch { expected: u64, actual: u64 },

    #[error("Invalid UTF-8 in string field at offset {0}")]
    InvalidUtf8(usize),

    #[error("Nesting depth {0} exceeds maximum {1}")]
    NestingTooDeep(usize, usize),

    #[error("Block size {0} exceeds maximum {1}")]
    BlockTooLarge(usize, usize),

    #[error("Item count {0} exceeds maximum {1}")]
    TooManyItems(usize, usize),

    #[error("Unknown compression type: {0}")]
    UnknownCompression(u8),

    #[error("Decompression error: {0}")]
    DecompressionError(String),

    #[error("Invalid block type: {0}")]
    InvalidBlockType(u8),

    #[error("Text parse error at line {line}, col {col}: {message}")]
    ParseError {
        line: usize,
        col: usize,
        message: String,
    },

    #[error("Schema mismatch: {0}")]
    SchemaMismatch(String),

    #[error("Memory limit exceeded: requested {0} bytes, limit {1}")]
    MemoryLimitExceeded(usize, usize),

    #[error("Invalid base64 data: {0}")]
    InvalidBase64(String),
}

/// Convenience Result alias.
pub type Result<T> = std::result::Result<T, CrousError>;
