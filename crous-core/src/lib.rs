//! # crous-core
//!
//! Core encoder/decoder, block framing, `Value` type, and zero-copy types
//! for the Crous binary format — a compact, canonical binary serializer
//! and human-readable alternative to JSON.
//!
//! ## Quick Start
//!
//! ```rust
//! use crous_core::{Value, Encoder, Decoder};
//!
//! let value = Value::Object(vec![
//!     ("name".into(), Value::Str("Alice".into())),
//!     ("age".into(), Value::UInt(30)),
//! ]);
//!
//! let mut encoder = Encoder::new();
//! encoder.encode_value(&value).unwrap();
//! let bytes = encoder.finish().unwrap();
//!
//! let mut decoder = Decoder::new(&bytes);
//! let decoded = decoder.decode_next().unwrap();
//! ```

pub mod block;
pub mod checksum;
pub mod decoder;
pub mod encoder;
pub mod error;
pub mod header;
pub mod limits;
pub mod text;
pub mod traits;
pub mod value;
pub mod varint;
pub mod wire;

pub use block::{BlockReader, BlockWriter};
pub use checksum::ChecksumAlgo;
#[cfg(feature = "fast-alloc")]
pub use decoder::BumpDecoder;
pub use decoder::Decoder;
pub use encoder::Encoder;
pub use error::{CrousError, Result};
pub use header::{FLAGS_NONE, FileHeader};
pub use limits::Limits;
pub use traits::Crous;
pub use value::{CrousValue, Value};
