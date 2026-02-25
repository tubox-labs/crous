//! Property-based tests for Crous encoding/decoding roundtrip.

use crous_core::text::{parse, pretty_print};
use crous_core::{Decoder, Encoder, Value};
use proptest::prelude::*;

/// Strategy to generate random Crous Values with bounded depth.
fn arb_value(max_depth: u32) -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<u64>().prop_map(Value::UInt),
        any::<i64>().prop_map(Value::Int),
        // Use finite floats only to avoid NaN comparison issues.
        (-1e15f64..1e15f64).prop_map(Value::Float),
        "[a-zA-Z0-9 ]{0,50}".prop_map(Value::Str),
        proptest::collection::vec(any::<u8>(), 0..32).prop_map(Value::Bytes),
    ];

    leaf.prop_recursive(
        max_depth, // max depth
        64,        // max nodes
        8,         // items per collection
        move |inner| {
            prop_oneof![
                proptest::collection::vec(inner.clone(), 0..8).prop_map(Value::Array),
                proptest::collection::vec(
                    ("[a-zA-Z_][a-zA-Z0-9_]{0,10}".prop_map(|s| s), inner),
                    0..8
                )
                .prop_map(Value::Object),
            ]
        },
    )
}

proptest! {
    #[test]
    fn binary_roundtrip(value in arb_value(4)) {
        let mut enc = Encoder::new();
        enc.encode_value(&value).unwrap();
        let bytes = enc.finish().unwrap();

        let mut dec = Decoder::new(&bytes);
        let decoded = dec.decode_next().unwrap().to_owned_value();

        // Float NaN handling: skip NaN comparisons
        if !contains_nan(&value) {
            prop_assert_eq!(&decoded, &value);
        }
    }

    #[test]
    fn text_roundtrip(value in arb_value(3)) {
        // Skip values with bytes (base64 roundtrip tested separately)
        if contains_bytes(&value) {
            return Ok(());
        }
        if contains_nan(&value) {
            return Ok(());
        }

        let text = pretty_print(&value, 4);
        let re_parsed = parse(&text).unwrap();
        // Unsigned integers may be re-parsed as UInt, signed as Int.
        // This is expected behavior. We verify structural equivalence.
        prop_assert!(structural_eq(&value, &re_parsed),
            "Text roundtrip mismatch:\nOriginal: {value:?}\nText: {text}\nReparsed: {re_parsed:?}");
    }
}

fn contains_nan(v: &Value) -> bool {
    match v {
        Value::Float(f) => f.is_nan(),
        Value::Array(items) => items.iter().any(contains_nan),
        Value::Object(entries) => entries.iter().any(|(_, v)| contains_nan(v)),
        _ => false,
    }
}

fn contains_bytes(v: &Value) -> bool {
    match v {
        Value::Bytes(_) => true,
        Value::Array(items) => items.iter().any(contains_bytes),
        Value::Object(entries) => entries.iter().any(|(_, v)| contains_bytes(v)),
        _ => false,
    }
}

/// Structural equality that treats Int/UInt as equivalent for same numeric value.
fn structural_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::UInt(x), Value::UInt(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::UInt(x), Value::Int(y)) => *x as i64 == *y,
        (Value::Int(x), Value::UInt(y)) => *x == *y as i64,
        (Value::Float(x), Value::Float(y)) => (x - y).abs() < 1e-10 || (x.is_nan() && y.is_nan()),
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Bytes(x), Value::Bytes(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| structural_eq(a, b))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y.iter())
                    .all(|((ka, va), (kb, vb))| ka == kb && structural_eq(va, vb))
        }
        _ => false,
    }
}
