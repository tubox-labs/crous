//! # crous-python
//!
//! PyO3 native extension that wraps `crous-core` encoder/decoder for Python.
//!
//! Provides:
//! - `encode(obj)` → `bytes`: Encode a Python dict/list/primitive to Crous binary.
//! - `decode(data)` → Python object: Decode Crous binary bytes to Python values.
//! - `Encoder` class: Incremental encoder with dedup/compression support.
//! - `Decoder` class: Incremental decoder with owned-value output.

use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyDict, PyFloat, PyInt, PyList, PyString};

use crous_core::wire::CompressionType;
use crous_core::{Decoder as CoreDecoder, Encoder as CoreEncoder, Value};

use std::fs;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Python ↔ Value conversion helpers
// ---------------------------------------------------------------------------

/// Convert a Python object to a Crous `Value`.
fn py_to_value(obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    if obj.is_none() {
        return Ok(Value::Null);
    }
    // Bool must be checked before int (Python bool is a subclass of int).
    if obj.is_instance_of::<PyBool>() {
        let b: bool = obj.extract()?;
        return Ok(Value::Bool(b));
    }
    if obj.is_instance_of::<PyInt>() {
        let n: i64 = obj.extract()?;
        return if n >= 0 {
            Ok(Value::UInt(n as u64))
        } else {
            Ok(Value::Int(n))
        };
    }
    if obj.is_instance_of::<PyFloat>() {
        let f: f64 = obj.extract()?;
        return Ok(Value::Float(f));
    }
    if obj.is_instance_of::<PyString>() {
        let s: String = obj.extract()?;
        return Ok(Value::Str(s));
    }
    if obj.is_instance_of::<PyBytes>() {
        let b: Vec<u8> = obj.extract()?;
        return Ok(Value::Bytes(b));
    }
    if obj.is_instance_of::<PyDict>() {
        let d = obj.cast_exact::<PyDict>()?;
        let mut entries = Vec::with_capacity(d.len());
        for (k, v) in d.iter() {
            let key: String = k
                .extract()
                .map_err(|_| PyTypeError::new_err("dict keys must be strings"))?;
            let val = py_to_value(&v)?;
            entries.push((key, val));
        }
        return Ok(Value::Object(entries));
    }
    if obj.is_instance_of::<PyList>() {
        let l = obj.cast_exact::<PyList>()?;
        let mut items = Vec::with_capacity(l.len());
        for item in l.iter() {
            items.push(py_to_value(&item)?);
        }
        return Ok(Value::Array(items));
    }
    Err(PyTypeError::new_err(format!(
        "cannot convert {} to Crous value",
        obj.get_type().name()?
    )))
}

/// Convert a Crous `Value` to a Python object.
fn value_to_py<'py>(py: Python<'py>, value: &Value) -> PyResult<Bound<'py, PyAny>> {
    match value {
        Value::Null => Ok(py.None().into_bound(py)),
        Value::Bool(b) => Ok(b.into_pyobject(py)?.to_owned().into_any()),
        Value::UInt(n) => Ok(n.into_pyobject(py)?.into_any()),
        Value::Int(n) => Ok(n.into_pyobject(py)?.into_any()),
        Value::Float(f) => Ok(f.into_pyobject(py)?.into_any()),
        Value::Str(s) => Ok(s.into_pyobject(py)?.into_any()),
        Value::Bytes(b) => Ok(PyBytes::new(py, b).into_any()),
        Value::Array(items) => {
            let list = PyList::empty(py);
            for item in items {
                list.append(value_to_py(py, item)?)?;
            }
            Ok(list.into_any())
        }
        Value::Object(entries) => {
            let dict = PyDict::new(py);
            for (key, val) in entries {
                dict.set_item(key, value_to_py(py, val)?)?;
            }
            Ok(dict.into_any())
        }
    }
}

// ---------------------------------------------------------------------------
// Module-level functions
// ---------------------------------------------------------------------------

/// Encode a Python object (dict, list, str, int, float, bytes, bool, None)
/// into Crous binary format, returned as `bytes`.
#[pyfunction]
fn encode<'py>(py: Python<'py>, obj: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyBytes>> {
    let value = py_to_value(obj)?;
    let mut encoder = CoreEncoder::new();
    encoder
        .encode_value(&value)
        .map_err(|e| PyRuntimeError::new_err(format!("encode error: {e}")))?;
    let bytes = encoder
        .finish()
        .map_err(|e| PyRuntimeError::new_err(format!("finish error: {e}")))?;
    Ok(PyBytes::new(py, &bytes))
}

/// Decode Crous binary `bytes` into Python objects.
///
/// Returns a single value if the data contains exactly one top-level value,
/// or a list of values otherwise.
#[pyfunction]
fn decode<'py>(py: Python<'py>, data: &Bound<'py, PyBytes>) -> PyResult<Bound<'py, PyAny>> {
    let buf = data.as_bytes();
    let mut decoder = CoreDecoder::new(buf);
    let values = decoder
        .decode_all_owned()
        .map_err(|e| PyRuntimeError::new_err(format!("decode error: {e}")))?;

    if values.len() == 1 {
        value_to_py(py, &values[0])
    } else {
        let list = PyList::empty(py);
        for v in &values {
            list.append(value_to_py(py, v)?)?;
        }
        Ok(list.into_any())
    }
}

/// Encode a Python object and write the binary output to a file.
///
/// Args:
///     obj: The Python object to encode (dict, list, str, int, float, bytes, bool, None).
///     path: File path to write the encoded binary data to.
///
/// Raises:
///     RuntimeError: If encoding fails.
///     OSError: If writing to the file fails.
#[pyfunction]
fn encode_to_file(obj: &Bound<'_, PyAny>, path: &str) -> PyResult<()> {
    let value = py_to_value(obj)?;
    let mut encoder = CoreEncoder::new();
    encoder
        .encode_value(&value)
        .map_err(|e| PyRuntimeError::new_err(format!("encode error: {e}")))?;
    let bytes = encoder
        .finish()
        .map_err(|e| PyRuntimeError::new_err(format!("finish error: {e}")))?;
    fs::write(PathBuf::from(path), &bytes)?;
    Ok(())
}

/// Read a Crous binary file and decode it into Python objects.
///
/// Args:
///     path: File path to read the binary data from.
///
/// Returns:
///     The decoded Python object (single value or list of values).
///
/// Raises:
///     RuntimeError: If decoding fails.
///     OSError: If reading the file fails.
#[pyfunction]
fn decode_from_file<'py>(py: Python<'py>, path: &str) -> PyResult<Bound<'py, PyAny>> {
    let buf = fs::read(PathBuf::from(path))?;
    let mut decoder = CoreDecoder::new(&buf);
    let values = decoder
        .decode_all_owned()
        .map_err(|e| PyRuntimeError::new_err(format!("decode error: {e}")))?;

    if values.len() == 1 {
        value_to_py(py, &values[0])
    } else {
        let list = PyList::empty(py);
        for v in &values {
            list.append(value_to_py(py, v)?)?;
        }
        Ok(list.into_any())
    }
}

/// Parse Crous human-readable text notation into a Python object.
///
/// Args:
///     text: A string in Crous text format.
///
/// Returns:
///     The parsed Python object.
///
/// Raises:
///     ValueError: If the text cannot be parsed.
#[pyfunction]
fn parse_text<'py>(py: Python<'py>, text: &str) -> PyResult<Bound<'py, PyAny>> {
    let value = crous_core::text::parse(text)
        .map_err(|e| PyValueError::new_err(format!("parse error: {e}")))?;
    value_to_py(py, &value)
}

/// Pretty-print a Python object in Crous human-readable text notation.
///
/// Args:
///     obj: The Python object to format.
///     indent: Number of spaces per indentation level (default: 2).
///
/// Returns:
///     A string in Crous text format.
///
/// Raises:
///     TypeError: If the object cannot be converted to a Crous value.
#[pyfunction]
#[pyo3(signature = (obj, indent=2))]
fn pretty_print(obj: &Bound<'_, PyAny>, indent: usize) -> PyResult<String> {
    let value = py_to_value(obj)?;
    Ok(crous_core::text::pretty_print(&value, indent))
}

// ---------------------------------------------------------------------------
// Encoder class
// ---------------------------------------------------------------------------

/// Incremental Crous encoder.
///
/// Example::
///
///     enc = Encoder()
///     enc.enable_dedup()
///     enc.set_compression("lz4")
///     enc.encode({"key": "value"})
///     data = enc.finish()
#[pyclass]
struct Encoder {
    inner: Option<CoreEncoder>,
}

#[pymethods]
impl Encoder {
    #[new]
    fn new() -> Self {
        Self {
            inner: Some(CoreEncoder::new()),
        }
    }

    /// Enable string deduplication for subsequent blocks.
    fn enable_dedup(&mut self) -> PyResult<()> {
        self.inner
            .as_mut()
            .ok_or_else(|| PyRuntimeError::new_err("encoder already finished"))?
            .enable_dedup();
        Ok(())
    }

    /// Set compression type: "none", "lz4", "zstd", or "snappy".
    fn set_compression(&mut self, comp: &str) -> PyResult<()> {
        let ct = match comp.to_lowercase().as_str() {
            "none" => CompressionType::None,
            "lz4" => CompressionType::Lz4,
            "zstd" => CompressionType::Zstd,
            "snappy" => CompressionType::Snappy,
            _ => {
                return Err(PyValueError::new_err(format!(
                    "unknown compression: {comp} (expected none, lz4, zstd, snappy)"
                )));
            }
        };
        self.inner
            .as_mut()
            .ok_or_else(|| PyRuntimeError::new_err("encoder already finished"))?
            .set_compression(ct);
        Ok(())
    }

    /// Encode a Python value into the current block.
    fn encode(&mut self, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        let value = py_to_value(obj)?;
        self.inner
            .as_mut()
            .ok_or_else(|| PyRuntimeError::new_err("encoder already finished"))?
            .encode_value(&value)
            .map_err(|e| PyRuntimeError::new_err(format!("encode error: {e}")))?;
        Ok(())
    }

    /// Flush and finalize the encoder, returning the Crous binary output as `bytes`.
    /// The encoder cannot be used after this call.
    fn finish<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let enc = self
            .inner
            .take()
            .ok_or_else(|| PyRuntimeError::new_err("encoder already finished"))?;
        let bytes = enc
            .finish()
            .map_err(|e| PyRuntimeError::new_err(format!("finish error: {e}")))?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Flush and finalize the encoder, writing the output directly to a file.
    /// The encoder cannot be used after this call.
    ///
    /// Args:
    ///     path: File path to write the encoded binary data to.
    ///
    /// Raises:
    ///     RuntimeError: If encoding fails or the encoder was already finished.
    ///     OSError: If writing to the file fails.
    fn finish_to_file(&mut self, path: &str) -> PyResult<()> {
        let enc = self
            .inner
            .take()
            .ok_or_else(|| PyRuntimeError::new_err("encoder already finished"))?;
        let bytes = enc
            .finish()
            .map_err(|e| PyRuntimeError::new_err(format!("finish error: {e}")))?;
        fs::write(PathBuf::from(path), &bytes)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Decoder class
// ---------------------------------------------------------------------------

/// Incremental Crous decoder.
///
/// Example::
///
///     dec = Decoder(data)
///     values = dec.decode_all()
#[pyclass]
struct CrousDecoder {
    /// We store the data so the decoder can borrow from it.
    data: Vec<u8>,
    /// Whether decode_all has been called.
    consumed: bool,
}

#[pymethods]
impl CrousDecoder {
    #[new]
    fn new(data: &Bound<'_, PyBytes>) -> Self {
        Self {
            data: data.as_bytes().to_vec(),
            consumed: false,
        }
    }

    /// Decode all values from the binary data.
    fn decode_all<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        if self.consumed {
            return Err(PyRuntimeError::new_err("decoder already consumed"));
        }
        self.consumed = true;

        let mut decoder = CoreDecoder::new(&self.data);
        let values = decoder
            .decode_all_owned()
            .map_err(|e| PyRuntimeError::new_err(format!("decode error: {e}")))?;

        let list = PyList::empty(py);
        for v in &values {
            list.append(value_to_py(py, v)?)?;
        }
        Ok(list)
    }
}

// ---------------------------------------------------------------------------
// Module definition
// ---------------------------------------------------------------------------

/// crous native extension (Rust-backed via PyO3).
///
/// Provides high-performance encode/decode for the Crous binary format.
#[pymodule]
fn _crous_native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", "1.1.2")?;
    m.add_function(wrap_pyfunction!(encode, m)?)?;
    m.add_function(wrap_pyfunction!(decode, m)?)?;
    m.add_function(wrap_pyfunction!(encode_to_file, m)?)?;
    m.add_function(wrap_pyfunction!(decode_from_file, m)?)?;
    m.add_function(wrap_pyfunction!(parse_text, m)?)?;
    m.add_function(wrap_pyfunction!(pretty_print, m)?)?;
    m.add_class::<Encoder>()?;
    m.add_class::<CrousDecoder>()?;
    Ok(())
}
