//! # crous-compression
//!
//! Pluggable compression adapters for Crous blocks.
//! Provides a trait for custom compressors and optional built-in
//! support for zstd and snappy (behind feature flags).

#[cfg(any(feature = "zstd", feature = "snappy"))]
use crous_core::error::CrousError;
use crous_core::error::Result;
use crous_core::wire::CompressionType;

/// Trait for pluggable compression algorithms.
///
/// Implement this trait to add custom compression support to Crous.
///
/// ```rust,ignore
/// struct MyCompressor;
///
/// impl Compressor for MyCompressor {
///     fn compression_type(&self) -> CompressionType { /* ... */ }
///     fn compress(&self, input: &[u8]) -> Result<Vec<u8>> { /* ... */ }
///     fn decompress(&self, input: &[u8], max_output: usize) -> Result<Vec<u8>> { /* ... */ }
/// }
/// ```
pub trait Compressor: Send + Sync {
    /// The compression type identifier for block headers.
    fn compression_type(&self) -> CompressionType;

    /// Compress the input data.
    fn compress(&self, input: &[u8]) -> Result<Vec<u8>>;

    /// Decompress the input data.
    /// `max_output` is the maximum allowed output size (for DoS mitigation).
    fn decompress(&self, input: &[u8], max_output: usize) -> Result<Vec<u8>>;

    /// The human-readable name of this compressor.
    fn name(&self) -> &'static str;
}

/// No-op passthrough compressor (CompressionType::None).
pub struct NoCompression;

impl Compressor for NoCompression {
    fn compression_type(&self) -> CompressionType {
        CompressionType::None
    }

    fn compress(&self, input: &[u8]) -> Result<Vec<u8>> {
        Ok(input.to_vec())
    }

    fn decompress(&self, input: &[u8], _max_output: usize) -> Result<Vec<u8>> {
        Ok(input.to_vec())
    }

    fn name(&self) -> &'static str {
        "none"
    }
}

/// Zstd compressor (requires `zstd` feature).
#[cfg(feature = "zstd")]
pub struct ZstdCompressor {
    /// Compression level (1-22, default 3).
    pub level: i32,
}

#[cfg(feature = "zstd")]
impl Default for ZstdCompressor {
    fn default() -> Self {
        Self { level: 3 }
    }
}

#[cfg(feature = "zstd")]
impl Compressor for ZstdCompressor {
    fn compression_type(&self) -> CompressionType {
        CompressionType::Zstd
    }

    fn compress(&self, input: &[u8]) -> Result<Vec<u8>> {
        zstd::bulk::compress(input, self.level)
            .map_err(|e| CrousError::DecompressionError(format!("zstd compress: {e}")))
    }

    fn decompress(&self, input: &[u8], max_output: usize) -> Result<Vec<u8>> {
        zstd::bulk::decompress(input, max_output)
            .map_err(|e| CrousError::DecompressionError(format!("zstd decompress: {e}")))
    }

    fn name(&self) -> &'static str {
        "zstd"
    }
}

/// Snappy compressor (requires `snappy` feature).
#[cfg(feature = "snappy")]
pub struct SnappyCompressor;

#[cfg(feature = "snappy")]
impl Compressor for SnappyCompressor {
    fn compression_type(&self) -> CompressionType {
        CompressionType::Snappy
    }

    fn compress(&self, input: &[u8]) -> Result<Vec<u8>> {
        let mut encoder = snap::raw::Encoder::new();
        encoder
            .compress_vec(input)
            .map_err(|e| CrousError::DecompressionError(format!("snappy compress: {e}")))
    }

    fn decompress(&self, input: &[u8], max_output: usize) -> Result<Vec<u8>> {
        let decompressed_len = snap::raw::decompress_len(input)
            .map_err(|e| CrousError::DecompressionError(format!("snappy len: {e}")))?;
        if decompressed_len > max_output {
            return Err(CrousError::MemoryLimitExceeded(
                decompressed_len,
                max_output,
            ));
        }
        let mut decoder = snap::raw::Decoder::new();
        decoder
            .decompress_vec(input)
            .map_err(|e| CrousError::DecompressionError(format!("snappy decompress: {e}")))
    }

    fn name(&self) -> &'static str {
        "snappy"
    }
}

/// Registry of available compressors.
pub struct CompressorRegistry {
    compressors: Vec<Box<dyn Compressor>>,
}

impl CompressorRegistry {
    /// Create a new registry with the built-in no-op compressor.
    pub fn new() -> Self {
        Self {
            compressors: vec![Box::new(NoCompression)],
        }
    }

    /// Create a registry with all available built-in compressors.
    pub fn with_defaults() -> Self {
        #[allow(unused_mut)]
        let mut reg = Self::new();
        #[cfg(feature = "zstd")]
        reg.register(Box::new(ZstdCompressor::default()));
        #[cfg(feature = "snappy")]
        reg.register(Box::new(SnappyCompressor));
        reg
    }

    /// Register a custom compressor.
    pub fn register(&mut self, compressor: Box<dyn Compressor>) {
        self.compressors.push(compressor);
    }

    /// Find a compressor by type.
    pub fn find(&self, comp_type: CompressionType) -> Option<&dyn Compressor> {
        self.compressors
            .iter()
            .find(|c| c.compression_type() == comp_type)
            .map(|c| c.as_ref())
    }
}

impl Default for CompressorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_compression_roundtrip() {
        let comp = NoCompression;
        let data = b"hello world, this is a test";
        let compressed = comp.compress(data).unwrap();
        let decompressed = comp.decompress(&compressed, 1024).unwrap();
        assert_eq!(&decompressed, data);
    }

    #[test]
    fn registry_find() {
        let reg = CompressorRegistry::new();
        assert!(reg.find(CompressionType::None).is_some());
        // Without features, zstd/snappy not found.
    }
}
