//! Value types for schema-less Crous data.
//!
//! Two representations:
//! - `Value`: owned, heap-allocated, suitable for building documents.
//! - `CrousValue<'a>`: zero-copy, borrow-backed, returned by the decoder.

use std::fmt;

/// Owned, schema-less Crous value (analogous to `serde_json::Value`).
///
/// This type owns all its data and can be freely moved, cloned, and serialized.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Null / absent value.
    Null,
    /// Boolean.
    Bool(bool),
    /// Unsigned integer (up to u64).
    UInt(u64),
    /// Signed integer (i64).
    Int(i64),
    /// 64-bit IEEE 754 float.
    Float(f64),
    /// UTF-8 string.
    Str(String),
    /// Raw binary blob.
    Bytes(Vec<u8>),
    /// Ordered array of values.
    Array(Vec<Value>),
    /// Ordered map of key-value pairs (keys are strings).
    /// Uses Vec to preserve insertion order (deterministic encoding).
    Object(Vec<(String, Value)>),
}

impl Value {
    /// Returns the type name as a human-readable string.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::UInt(_) => "uint",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "str",
            Value::Bytes(_) => "bytes",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        }
    }

    /// Returns true if this value is null.
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Try to get as a string reference.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Try to get as u64.
    pub fn as_uint(&self) -> Option<u64> {
        match self {
            Value::UInt(n) => Some(*n),
            _ => None,
        }
    }

    /// Try to get as i64.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }

    /// Try to get as f64.
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(n) => Some(*n),
            _ => None,
        }
    }

    /// Try to get as a boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Try to get as an array.
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    /// Try to get as an object (ordered key-value pairs).
    pub fn as_object(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::UInt(n) => write!(f, "{n}"),
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(n) => write!(f, "{n}"),
            Value::Str(s) => write!(f, "\"{s}\""),
            Value::Bytes(b) => write!(
                f,
                "b64#{}",
                base64::engine::general_purpose::STANDARD.encode(b)
            ),
            Value::Array(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, "]")
            }
            Value::Object(entries) => {
                write!(f, "{{ ")?;
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{k}: {v};")?;
                }
                write!(f, " }}")
            }
        }
    }
}

use base64::Engine;

/// Zero-copy, borrow-backed Crous value returned by the decoder.
///
/// Borrows string and bytes data directly from the input buffer,
/// avoiding allocation for read-heavy workloads.
#[derive(Debug, Clone, PartialEq)]
pub enum CrousValue<'a> {
    Null,
    Bool(bool),
    UInt(u64),
    Int(i64),
    Float(f64),
    /// Borrowed UTF-8 string slice from the input buffer.
    Str(&'a str),
    /// Borrowed byte slice from the input buffer.
    Bytes(&'a [u8]),
    /// Array of zero-copy values.
    Array(Vec<CrousValue<'a>>),
    /// Object with borrowed keys.
    Object(Vec<(&'a str, CrousValue<'a>)>),
}

impl<'a> CrousValue<'a> {
    /// Convert a borrowed CrousValue into an owned Value (copies strings/bytes).
    pub fn to_owned_value(&self) -> Value {
        match self {
            CrousValue::Null => Value::Null,
            CrousValue::Bool(b) => Value::Bool(*b),
            CrousValue::UInt(n) => Value::UInt(*n),
            CrousValue::Int(n) => Value::Int(*n),
            CrousValue::Float(n) => Value::Float(*n),
            CrousValue::Str(s) => Value::Str((*s).to_string()),
            CrousValue::Bytes(b) => Value::Bytes(b.to_vec()),
            CrousValue::Array(items) => {
                Value::Array(items.iter().map(|v| v.to_owned_value()).collect())
            }
            CrousValue::Object(entries) => Value::Object(
                entries
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), v.to_owned_value()))
                    .collect(),
            ),
        }
    }
}

/// Convert from serde_json::Value to crous Value for interop.
impl From<&serde_json::Value> for Value {
    fn from(jv: &serde_json::Value) -> Self {
        match jv {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Bool(*b),
            serde_json::Value::Number(n) => {
                if let Some(u) = n.as_u64() {
                    Value::UInt(u)
                } else if let Some(i) = n.as_i64() {
                    Value::Int(i)
                } else if let Some(f) = n.as_f64() {
                    Value::Float(f)
                } else {
                    Value::Null
                }
            }
            serde_json::Value::String(s) => Value::Str(s.clone()),
            serde_json::Value::Array(arr) => Value::Array(arr.iter().map(Value::from).collect()),
            serde_json::Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| (k.clone(), Value::from(v)))
                    .collect(),
            ),
        }
    }
}

/// Convert from crous Value to serde_json::Value for interop.
impl From<&Value> for serde_json::Value {
    fn from(cv: &Value) -> Self {
        match cv {
            Value::Null => serde_json::Value::Null,
            Value::Bool(b) => serde_json::Value::Bool(*b),
            Value::UInt(n) => serde_json::json!(*n),
            Value::Int(n) => serde_json::json!(*n),
            Value::Float(n) => serde_json::json!(*n),
            Value::Str(s) => serde_json::Value::String(s.clone()),
            Value::Bytes(b) => {
                serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(b))
            }
            Value::Array(items) => {
                serde_json::Value::Array(items.iter().map(serde_json::Value::from).collect())
            }
            Value::Object(entries) => {
                let map: serde_json::Map<String, serde_json::Value> = entries
                    .iter()
                    .map(|(k, v)| (k.clone(), serde_json::Value::from(v)))
                    .collect();
                serde_json::Value::Object(map)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_type_names() {
        assert_eq!(Value::Null.type_name(), "null");
        assert_eq!(Value::Bool(true).type_name(), "bool");
        assert_eq!(Value::UInt(42).type_name(), "uint");
        assert_eq!(Value::Str("hi".into()).type_name(), "str");
    }

    #[test]
    fn crous_value_to_owned() {
        let cv = CrousValue::Object(vec![
            ("name", CrousValue::Str("Alice")),
            ("age", CrousValue::UInt(30)),
        ]);
        let owned = cv.to_owned_value();
        assert_eq!(
            owned,
            Value::Object(vec![
                ("name".into(), Value::Str("Alice".into())),
                ("age".into(), Value::UInt(30)),
            ])
        );
    }

    #[test]
    fn json_roundtrip() {
        let cv = Value::Object(vec![
            ("name".into(), Value::Str("Bob".into())),
            ("score".into(), Value::Float(99.5)),
        ]);
        let jv = serde_json::Value::from(&cv);
        let back = Value::from(&jv);
        // Float comes back as Float since serde_json preserves f64
        match &back {
            Value::Object(entries) => {
                assert_eq!(entries[0].0, "name");
                assert_eq!(entries[1].0, "score");
            }
            _ => panic!("expected object"),
        }
    }
}
