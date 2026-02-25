//! # crous-compression
//!
//! Pluggable compression adapters for Crous blocks.
//! Provides a trait for custom compressors and optional built-in
//! support for zstd, lz4, and snappy (behind feature flags).
//!
//! ## Compression libraries
//! - Zstd: https://facebook.github.io/zstd/
//! - LZ4: https://github.com/lz4/lz4 (pure-Rust via lz4_flex)
//! - Snappy: https://github.com/google/snappy

#[cfg(any(feature = "zstd", feature = "snappy", feature = "lz4"))]
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

/// LZ4 block compressor (requires `lz4` feature).
/// Uses lz4_flex for pure-Rust LZ4 compression.
/// Citation: https://github.com/PSeitz/lz4_flex
#[cfg(feature = "lz4")]
pub struct Lz4Compressor;

#[cfg(feature = "lz4")]
impl Compressor for Lz4Compressor {
    fn compression_type(&self) -> CompressionType {
        CompressionType::Lz4
    }

    fn compress(&self, input: &[u8]) -> Result<Vec<u8>> {
        Ok(lz4_flex::compress_prepend_size(input))
    }

    fn decompress(&self, input: &[u8], max_output: usize) -> Result<Vec<u8>> {
        // lz4_flex stores the uncompressed size as a 4-byte LE prefix
        if input.len() < 4 {
            return Err(CrousError::DecompressionError(
                "lz4: input too short for size prefix".into(),
            ));
        }
        let expected_size = u32::from_le_bytes([input[0], input[1], input[2], input[3]]) as usize;
        if expected_size > max_output {
            return Err(CrousError::MemoryLimitExceeded(expected_size, max_output));
        }
        lz4_flex::decompress_size_prepended(input)
            .map_err(|e| CrousError::DecompressionError(format!("lz4 decompress: {e}")))
    }

    fn name(&self) -> &'static str {
        "lz4"
    }
}

/// Adaptive compression selector.
///
/// Samples the first N bytes of input, compresses with each available
/// compressor, and picks the one with the best ratio (if it meets
/// the threshold). Falls back to `NoCompression` if nothing helps.
pub struct AdaptiveSelector {
    /// Minimum compression ratio (compressed/original) to justify compression.
    /// E.g., 0.9 means compression must achieve at least 10% reduction.
    pub ratio_threshold: f64,
    /// Maximum sample size (bytes) for the trial compression.
    pub sample_size: usize,
}

impl Default for AdaptiveSelector {
    fn default() -> Self {
        Self {
            ratio_threshold: 0.90,
            sample_size: 64 * 1024, // 64 KiB
        }
    }
}

impl AdaptiveSelector {
    /// Given a payload, select the best compressor from the registry.
    /// Returns the compression type to use.
    pub fn select(&self, data: &[u8], registry: &CompressorRegistry) -> CompressionType {
        let sample = if data.len() > self.sample_size {
            &data[..self.sample_size]
        } else {
            data
        };

        let mut best_type = CompressionType::None;
        let mut best_ratio = 1.0f64;

        for comp in &registry.compressors {
            if comp.compression_type() == CompressionType::None {
                continue;
            }
            if let Ok(compressed) = comp.compress(sample) {
                let ratio = compressed.len() as f64 / sample.len() as f64;
                if ratio < best_ratio && ratio < self.ratio_threshold {
                    best_ratio = ratio;
                    best_type = comp.compression_type();
                }
            }
        }
        best_type
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
        #[cfg(feature = "lz4")]
        reg.register(Box::new(Lz4Compressor));
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
