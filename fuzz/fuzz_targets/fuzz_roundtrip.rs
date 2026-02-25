//! Fuzz target: encode a Value, then decode it, verify roundtrip.
//!
//! Uses `arbitrary` to generate structured Values (not random bytes),
//! ensuring that encode → decode roundtrip always produces the same value.
//!
//! Run with: cargo +nightly fuzz run fuzz_roundtrip

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

/// A structured input that maps to a Crous `Value`.
#[derive(Debug, Arbitrary)]
enum FuzzValue {
    Null,
    Bool(bool),
    UInt(u64),
    Int(i64),
    Float(f64),
    Str(String),
    Bytes(Vec<u8>),
    Array(Vec<FuzzValue>),
    Object(Vec<(String, FuzzValue)>),
}

impl FuzzValue {
    fn to_value(&self) -> crous_core::Value {
        match self {
            FuzzValue::Null => crous_core::Value::Null,
            FuzzValue::Bool(b) => crous_core::Value::Bool(*b),
            FuzzValue::UInt(n) => crous_core::Value::UInt(*n),
            FuzzValue::Int(n) => crous_core::Value::Int(*n),
            FuzzValue::Float(f) => crous_core::Value::Float(*f),
            FuzzValue::Str(s) => crous_core::Value::Str(s.clone()),
            FuzzValue::Bytes(b) => crous_core::Value::Bytes(b.clone()),
            FuzzValue::Array(arr) => {
                // Limit nesting depth implicitly by limiting array size
                let items: Vec<_> = arr.iter().take(32).map(|v| v.to_value()).collect();
                crous_core::Value::Array(items)
            }
            FuzzValue::Object(entries) => {
                let pairs: Vec<_> = entries
                    .iter()
                    .take(32)
                    .map(|(k, v)| (k.clone(), v.to_value()))
                    .collect();
                crous_core::Value::Object(pairs)
            }
        }
    }
}

fuzz_target!(|input: FuzzValue| {
    let value = input.to_value();

    // Encode
    let mut enc = crous_core::Encoder::new();
    if enc.encode_value(&value).is_err() {
        return; // Nesting too deep, etc. — acceptable
    }
    let bytes = match enc.finish() {
        Ok(b) => b,
        Err(_) => return,
    };

    // Decode
    let mut dec = crous_core::Decoder::new(&bytes);
    let decoded = match dec.decode_next_owned() {
        Ok(v) => v,
        Err(e) => {
            panic!("Roundtrip decode failed: {e}");
        }
    };

    // NaN-aware comparison (NaN != NaN in IEEE 754, but we want roundtrip equality)
    assert!(nan_aware_eq(&value, &decoded), "Roundtrip mismatch:\n  left:  {value:?}\n  right: {decoded:?}");
});

/// Recursively compare two Values, treating NaN == NaN.
fn nan_aware_eq(a: &crous_core::Value, b: &crous_core::Value) -> bool {
    use crous_core::Value;
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::UInt(x), Value::UInt(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => {
            if x.is_nan() && y.is_nan() {
                x.to_bits() == y.to_bits()
            } else {
                x.to_bits() == y.to_bits()
            }
        }
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Bytes(x), Value::Bytes(y)) => x == y,
        (Value::Array(xa), Value::Array(ya)) => {
            xa.len() == ya.len() && xa.iter().zip(ya.iter()).all(|(x, y)| nan_aware_eq(x, y))
        }
        (Value::Object(xo), Value::Object(yo)) => {
            xo.len() == yo.len()
                && xo.iter().zip(yo.iter()).all(|((xk, xv), (yk, yv))| {
                    xk == yk && nan_aware_eq(xv, yv)
                })
        }
        _ => false,
    }
}
