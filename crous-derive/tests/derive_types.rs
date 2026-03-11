//! Integration tests for `#[derive(Crous)]` with all supported field types.

use crous_core::{Crous, CrousBytes, Value};
use crous_derive::{Crous, CrousSchema};

// ─── Structs using all newly-supported types ─────────────────────────────────

#[derive(Debug, PartialEq, Crous, CrousSchema)]
struct Person {
    #[crous(id = 1)]
    name: String,
    #[crous(id = 2)]
    age: u8,
    #[crous(id = 3)]
    tags: Vec<String>,
}

#[derive(Debug, PartialEq, Crous)]
struct AllIntegers {
    #[crous(id = 1)]
    a: u8,
    #[crous(id = 2)]
    b: u16,
    #[crous(id = 3)]
    c: u32,
    #[crous(id = 4)]
    d: u64,
    #[crous(id = 5)]
    e: i8,
    #[crous(id = 6)]
    f: i16,
    #[crous(id = 7)]
    g: i32,
    #[crous(id = 8)]
    h: i64,
}

#[derive(Debug, PartialEq, Crous)]
struct WithFloats {
    #[crous(id = 1)]
    single: f32,
    #[crous(id = 2)]
    double: f64,
}

#[derive(Debug, PartialEq, Crous)]
struct WithBytes {
    #[crous(id = 1)]
    label: String,
    #[crous(id = 2)]
    raw: CrousBytes,
    #[crous(id = 3)]
    byte_array: Vec<u8>,
}

#[derive(Debug, PartialEq, Crous)]
struct WithOptionals {
    #[crous(id = 1)]
    required: u32,
    #[crous(id = 2)]
    maybe_name: Option<String>,
    #[crous(id = 3)]
    maybe_count: Option<u16>,
}

#[derive(Debug, PartialEq, Crous)]
struct WithBool {
    #[crous(id = 1)]
    flag: bool,
    #[crous(id = 2)]
    items: Vec<bool>,
}

#[derive(Debug, PartialEq, Crous)]
struct WithVecU8 {
    #[crous(id = 1)]
    data: Vec<u8>,
    #[crous(id = 2)]
    name: String,
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn person_with_u8_age() {
    let p = Person {
        name: "Alice".into(),
        age: 30,
        tags: vec!["admin".into(), "dev".into()],
    };

    let val = p.to_crous_value();
    let back = Person::from_crous_value(&val).unwrap();
    assert_eq!(p, back);
}

#[test]
fn person_binary_roundtrip() {
    let p = Person {
        name: "Bob".into(),
        age: 255, // max u8
        tags: vec![],
    };

    let bytes = p.to_crous_bytes().unwrap();
    let back = Person::from_crous_bytes(&bytes).unwrap();
    assert_eq!(p, back);
}

#[test]
fn person_schema_info() {
    let info = Person::schema_info();
    assert_eq!(info.len(), 3);
    assert_eq!(info[0], ("name", 1));
    assert_eq!(info[1], ("age", 2));
    assert_eq!(info[2], ("tags", 3));
}

#[test]
fn all_integers_roundtrip() {
    let v = AllIntegers {
        a: 255,
        b: 65535,
        c: u32::MAX,
        d: u64::MAX,
        e: -128,
        f: -32768,
        g: i32::MIN,
        h: i64::MIN,
    };

    let val = v.to_crous_value();
    let back = AllIntegers::from_crous_value(&val).unwrap();
    assert_eq!(v, back);
}

#[test]
fn all_integers_binary_roundtrip() {
    let v = AllIntegers {
        a: 0,
        b: 1000,
        c: 100_000,
        d: 1_000_000,
        e: -1,
        f: 100,
        g: -100_000,
        h: 0,
    };

    let bytes = v.to_crous_bytes().unwrap();
    let back = AllIntegers::from_crous_bytes(&bytes).unwrap();
    assert_eq!(v, back);
}

#[test]
fn with_floats_roundtrip() {
    let v = WithFloats {
        single: 1.5,
        double: 99.99,
    };

    let val = v.to_crous_value();
    let back = WithFloats::from_crous_value(&val).unwrap();
    // f32 loses precision through f64
    assert!((back.single - 1.5).abs() < 1e-6);
    assert!((back.double - 99.99).abs() < 1e-10);
}

#[test]
fn with_crous_bytes_roundtrip() {
    let v = WithBytes {
        label: "payload".into(),
        raw: CrousBytes(vec![0xDE, 0xAD, 0xBE, 0xEF]),
        byte_array: vec![1, 2, 3, 4, 5],
    };

    let val = v.to_crous_value();

    // CrousBytes → Bytes, Vec<u8> → Array
    if let Value::Object(entries) = &val {
        let raw_val = entries.iter().find(|(k, _)| k == "raw").unwrap();
        assert!(matches!(raw_val.1, Value::Bytes(_)));

        let arr_val = entries.iter().find(|(k, _)| k == "byte_array").unwrap();
        assert!(matches!(arr_val.1, Value::Array(_)));
    } else {
        panic!("expected Object");
    }

    let back = WithBytes::from_crous_value(&val).unwrap();
    assert_eq!(v, back);
}

#[test]
fn with_bytes_binary_roundtrip() {
    let v = WithBytes {
        label: "test".into(),
        raw: CrousBytes(vec![0xFF; 100]),
        byte_array: vec![10, 20, 30],
    };

    let bytes = v.to_crous_bytes().unwrap();
    let back = WithBytes::from_crous_bytes(&bytes).unwrap();
    assert_eq!(v, back);
}

#[test]
fn with_optionals_present() {
    let v = WithOptionals {
        required: 42,
        maybe_name: Some("hello".into()),
        maybe_count: Some(100),
    };

    let bytes = v.to_crous_bytes().unwrap();
    let back = WithOptionals::from_crous_bytes(&bytes).unwrap();
    assert_eq!(v, back);
}

#[test]
fn with_optionals_absent() {
    let v = WithOptionals {
        required: 0,
        maybe_name: None,
        maybe_count: None,
    };

    let bytes = v.to_crous_bytes().unwrap();
    let back = WithOptionals::from_crous_bytes(&bytes).unwrap();
    assert_eq!(v, back);
}

#[test]
fn with_bool_roundtrip() {
    let v = WithBool {
        flag: true,
        items: vec![true, false, true, false],
    };

    let bytes = v.to_crous_bytes().unwrap();
    let back = WithBool::from_crous_bytes(&bytes).unwrap();
    assert_eq!(v, back);
}

#[test]
fn vec_u8_field_binary_roundtrip() {
    let v = WithVecU8 {
        data: vec![0, 127, 255, 1, 42],
        name: "binary-array".into(),
    };

    let bytes = v.to_crous_bytes().unwrap();
    let back = WithVecU8::from_crous_bytes(&bytes).unwrap();
    assert_eq!(v, back);
}

#[test]
fn fingerprint_is_stable() {
    // Schema fingerprint should be deterministic.
    let fp1 = Person::schema_fingerprint();
    let fp2 = Person::schema_fingerprint();
    assert_eq!(fp1, fp2);
    assert_ne!(fp1, 0);

    // Different structs should have different fingerprints.
    let fp3 = AllIntegers::schema_fingerprint();
    assert_ne!(fp1, fp3);
}
