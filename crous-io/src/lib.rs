//! # crous-io
//!
//! Async IO adapters for Crous, including:
//! - Framed stream reader/writer for Tokio
//! - Memory-mapped file reader (feature `mmap`)
//! - Streaming block reader
//! - Bytes-based shared buffer API
//!
//! ## Feature flags
//! - `mmap` — enables `MmapReader` for zero-copy file access.
//!   Citation: https://docs.rs/memmap2 — memmap best practices

use crous_core::block::BlockWriter;
use crous_core::error::{CrousError, Result};
use crous_core::header::{FileHeader, HEADER_SIZE};
use crous_core::wire::BlockType;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Async writer that frames Crous data into blocks over a Tokio stream.
pub struct FramedWriter<W: AsyncWrite + Unpin> {
    writer: W,
    header_written: bool,
    flags: u8,
}

impl<W: AsyncWrite + Unpin> FramedWriter<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            header_written: false,
            flags: 0,
        }
    }

    pub fn with_flags(writer: W, flags: u8) -> Self {
        Self {
            writer,
            header_written: false,
            flags,
        }
    }

    /// Write the file header if not already written.
    async fn ensure_header(&mut self) -> Result<()> {
        if !self.header_written {
            let header = FileHeader::new(self.flags);
            self.writer.write_all(&header.encode()).await?;
            self.header_written = true;
        }
        Ok(())
    }

    /// Write a pre-built block.
    pub async fn write_block(&mut self, block: &[u8]) -> Result<()> {
        self.ensure_header().await?;
        self.writer.write_all(block).await?;
        Ok(())
    }

    /// Write a raw data payload as a framed block.
    pub async fn write_data(&mut self, payload: &[u8]) -> Result<()> {
        self.ensure_header().await?;
        let mut bw = BlockWriter::new(BlockType::Data);
        bw.write(payload);
        let block_bytes = bw.finish();
        self.writer.write_all(&block_bytes).await?;
        Ok(())
    }

    /// Flush the underlying writer.
    pub async fn flush(&mut self) -> Result<()> {
        self.writer.flush().await?;
        Ok(())
    }

    /// Consume the writer and return the inner stream.
    pub fn into_inner(self) -> W {
        self.writer
    }
}

/// Async reader that reads Crous blocks from a Tokio stream.
pub struct FramedReader<R: AsyncRead + Unpin> {
    reader: R,
    header: Option<FileHeader>,
    #[allow(dead_code)]
    buf: Vec<u8>,
}

impl<R: AsyncRead + Unpin> FramedReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            header: None,
            buf: Vec::with_capacity(4096),
        }
    }

    /// Read and parse the file header.
    pub async fn read_header(&mut self) -> Result<&FileHeader> {
        if self.header.is_none() {
            let mut header_buf = [0u8; HEADER_SIZE];
            self.reader.read_exact(&mut header_buf).await?;
            self.header = Some(FileHeader::decode(&header_buf)?);
        }
        Ok(self.header.as_ref().unwrap())
    }

    /// Read the next block's raw bytes. Returns None at EOF.
    pub async fn read_next_block_raw(&mut self) -> Result<Option<Vec<u8>>> {
        if self.header.is_none() {
            self.read_header().await?;
        }

        // Read block type (1 byte).
        let mut type_buf = [0u8; 1];
        match self.reader.read_exact(&mut type_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        }

        if type_buf[0] == BlockType::Trailer as u8 {
            return Ok(None);
        }

        // Read varint length (up to 10 bytes, 1 at a time for streaming).
        let mut len_bytes = Vec::with_capacity(10);
        loop {
            let mut b = [0u8; 1];
            self.reader.read_exact(&mut b).await?;
            len_bytes.push(b[0]);
            if b[0] & 0x80 == 0 {
                break;
            }
            if len_bytes.len() > 10 {
                return Err(CrousError::VarintOverflow);
            }
        }

        let (block_len, _) = crous_core::varint::decode_varint(&len_bytes, 0)?;
        let block_len = block_len as usize;

        // Read compression type (1 byte) + checksum (8 bytes) + payload.
        let remaining = 1 + 8 + block_len;
        let mut payload = vec![0u8; remaining];
        self.reader.read_exact(&mut payload).await?;

        // Reconstruct the full block bytes.
        let mut block = Vec::with_capacity(1 + len_bytes.len() + remaining);
        block.push(type_buf[0]);
        block.extend_from_slice(&len_bytes);
        block.extend_from_slice(&payload);

        Ok(Some(block))
    }
}

/// Read a complete Crous file from memory-mapped or in-memory bytes.
///
/// This is the simplest API for reading a complete file.
pub fn read_file_bytes(data: &[u8]) -> Result<Vec<crous_core::Value>> {
    let mut decoder = crous_core::Decoder::new(data);
    decoder.decode_all_owned()
}

/// Write values to an in-memory buffer as a complete Crous file.
pub fn write_values_to_bytes(values: &[crous_core::Value]) -> Result<Vec<u8>> {
    let mut encoder = crous_core::Encoder::new();
    for v in values {
        encoder.encode_value(v)?;
    }
    encoder.finish()
}

// ---------------------------------------------------------------------------
// Bytes-based shared buffer API
// ---------------------------------------------------------------------------

/// Read a complete Crous file from a `bytes::Bytes` buffer.
///
/// The `Bytes` reference-counted buffer avoids copies when sharing
/// between threads or network layers.
/// Citation: https://docs.rs/bytes
pub fn read_from_shared(data: bytes::Bytes) -> Result<Vec<crous_core::Value>> {
    let mut decoder = crous_core::Decoder::new(&data);
    decoder.decode_all_owned()
}

/// Write values into a `bytes::Bytes` shared buffer.
pub fn write_to_shared(values: &[crous_core::Value]) -> Result<bytes::Bytes> {
    let vec = write_values_to_bytes(values)?;
    Ok(bytes::Bytes::from(vec))
}

// ---------------------------------------------------------------------------
// Memory-mapped file reader (feature = "mmap")
// ---------------------------------------------------------------------------

/// Zero-copy memory-mapped file reader for Crous files.
///
/// Maps a file into the process address space and provides direct
/// zero-copy access to the underlying bytes. The `Decoder` can
/// borrow `CrousValue<'a>` directly from the mapped memory.
///
/// # Safety considerations
/// The file must not be modified while the mapping is live.
/// `MmapReader` uses a read-only mapping which will cause SIGBUS
/// if the file is truncated. For untrusted files, prefer `read_file_bytes`.
///
/// Citation: memmap best practices — https://docs.rs/memmap2
#[cfg(feature = "mmap")]
pub struct MmapReader {
    _mmap: memmap2::Mmap,
}

#[cfg(feature = "mmap")]
impl MmapReader {
    /// Open a Crous file for zero-copy reading.
    ///
    /// ```rust,ignore
    /// let reader = MmapReader::open("data.crous")?;
    /// let values = reader.decode_all()?;
    /// ```
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        // SAFETY: We require the file not to be modified while mapped.
        let mmap = unsafe { memmap2::Mmap::map(&file)? };
        Ok(Self { _mmap: mmap })
    }

    /// Get a reference to the mapped bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self._mmap
    }

    /// Get the file size.
    pub fn len(&self) -> usize {
        self._mmap.len()
    }

    /// Check if the mapping is empty.
    pub fn is_empty(&self) -> bool {
        self._mmap.is_empty()
    }

    /// Create a decoder over the mapped memory.
    ///
    /// The returned decoder borrows from the mapping, enabling zero-copy
    /// `CrousValue<'_>` decoding with no additional allocation for strings/bytes.
    pub fn decoder(&self) -> crous_core::Decoder<'_> {
        crous_core::Decoder::new(&self._mmap)
    }

    /// Create a decoder with custom limits.
    pub fn decoder_with_limits(&self, limits: crous_core::Limits) -> crous_core::Decoder<'_> {
        crous_core::Decoder::with_limits(&self._mmap, limits)
    }

    /// Convenience: decode all values as owned Values.
    pub fn decode_all(&self) -> Result<Vec<crous_core::Value>> {
        let mut dec = self.decoder();
        dec.decode_all_owned()
    }

    /// Convenience: decode all values as zero-copy CrousValues.
    pub fn decode_all_borrowed(&self) -> Result<Vec<crous_core::CrousValue<'_>>> {
        let mut dec = self.decoder();
        dec.decode_all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crous_core::Value;

    #[tokio::test]
    async fn framed_writer_basic() {
        let mut buf = Vec::new();
        {
            let mut writer = FramedWriter::new(&mut buf);
            writer.write_data(b"hello").await.unwrap();
            writer.flush().await.unwrap();
        }
        // Should start with magic.
        assert_eq!(&buf[..7], b"CROUSv1");
    }

    #[test]
    fn read_write_bytes() {
        let values = vec![Value::Str("hello".into()), Value::UInt(42)];
        let bytes = write_values_to_bytes(&values).unwrap();
        let decoded = read_file_bytes(&bytes).unwrap();
        assert_eq!(decoded, values);
    }
}
