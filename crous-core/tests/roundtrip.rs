//! Integration tests: textual → binary → textual roundtrip for 5 nontrivial examples.
//!
//! Each test parses a Crous text document, encodes it to binary, decodes it back,
//! and verifies the result matches. We also print hex and byte sizes for reference.

use crous_core::text::{parse, pretty_print};
use crous_core::{Decoder, Encoder, Value};

/// Helper: full roundtrip text → binary → text and verify determinism.
fn full_roundtrip(input: &str) -> (Value, Vec<u8>, String) {
    // Parse text → Value
    let val = parse(input).expect("parse failed");

    // Encode Value → binary
    let mut enc = Encoder::new();
    enc.encode_value(&val).expect("encode failed");
    let binary = enc.finish().expect("finish failed");

    // Decode binary → Value
    let mut dec = Decoder::new(&binary);
    let decoded = dec.decode_next().expect("decode failed").to_owned_value();
    assert_eq!(val, decoded, "roundtrip mismatch");

    // Pretty-print back to text
    let text_out = pretty_print(&decoded, 4);

    // Re-parse pretty-printed text and verify identity
    let re_parsed = parse(&text_out).expect("re-parse failed");
    assert_eq!(val, re_parsed, "text roundtrip mismatch");

    (val, binary, text_out)
}

fn hex_string(data: &[u8]) -> String {
    data.iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

// ─── Example 1: Simple person record ────────────────────────────────────────

#[test]
fn example_1_person() {
    let input = r#"{
        name: "Alice";
        age: 30;
        active: true;
    }"#;

    let json_equiv = r#"{"name":"Alice","age":30,"active":true}"#;

    let (_val, binary, text) = full_roundtrip(input);

    println!("=== Example 1: Person ===");
    println!("Crous text:\n{text}");
    println!("Binary ({} bytes): {}", binary.len(), hex_string(&binary));
    println!("JSON equiv ({} bytes): {json_equiv}", json_equiv.len());
    println!(
        "Ratio: {:.1}%\n",
        binary.len() as f64 / json_equiv.len() as f64 * 100.0
    );

    // Note: for very small documents, the 8-byte header + block framing + checksums
    // make binary larger. Binary wins on larger/nested documents.
}

// ─── Example 2: Nested users with arrays ────────────────────────────────────

#[test]
fn example_2_nested_users() {
    let input = r#"{
        users: [
            {
                name: "Bob";
                scores: [100, 95, 87];
            },
            {
                name: "Carol";
                scores: [88, 92, 79];
            }
        ];
        count: 2;
    }"#;

    let json_equiv = r#"{"users":[{"name":"Bob","scores":[100,95,87]},{"name":"Carol","scores":[88,92,79]}],"count":2}"#;

    let (_, binary, text) = full_roundtrip(input);

    println!("=== Example 2: Nested Users ===");
    println!("Crous text:\n{text}");
    println!("Binary ({} bytes): {}", binary.len(), hex_string(&binary));
    println!("JSON equiv ({} bytes)", json_equiv.len());
    println!(
        "Ratio: {:.1}%\n",
        binary.len() as f64 / json_equiv.len() as f64 * 100.0
    );
}

// ─── Example 3: Mixed types with binary data ────────────────────────────────

#[test]
fn example_3_binary_data() {
    let input = r#"{
        id: 42;
        label: "sensor-alpha";
        reading: 23.456;
        raw: b64#AQIDBA==;
        enabled: false;
    }"#;

    let json_equiv =
        r#"{"id":42,"label":"sensor-alpha","reading":23.456,"raw":"AQIDBA==","enabled":false}"#;

    let (_, binary, text) = full_roundtrip(input);

    println!("=== Example 3: Binary Data ===");
    println!("Crous text:\n{text}");
    println!("Binary ({} bytes): {}", binary.len(), hex_string(&binary));
    println!("JSON equiv ({} bytes)", json_equiv.len());
    println!(
        "Ratio: {:.1}%\n",
        binary.len() as f64 / json_equiv.len() as f64 * 100.0
    );
}

// ─── Example 4: Deeply nested config ────────────────────────────────────────

#[test]
fn example_4_deep_config() {
    let input = r#"{
        server: {
            host: "0.0.0.0";
            port: 8080;
            tls: {
                enabled: true;
                cert: "/etc/ssl/cert.pem";
                key: "/etc/ssl/key.pem";
            };
        };
        database: {
            url: "postgres://localhost/mydb";
            pool_size: 10;
        };
    }"#;

    let json_equiv = r#"{"server":{"host":"0.0.0.0","port":8080,"tls":{"enabled":true,"cert":"/etc/ssl/cert.pem","key":"/etc/ssl/key.pem"}},"database":{"url":"postgres://localhost/mydb","pool_size":10}}"#;

    let (_, binary, text) = full_roundtrip(input);

    println!("=== Example 4: Deep Config ===");
    println!("Crous text:\n{text}");
    println!("Binary ({} bytes): {}", binary.len(), hex_string(&binary));
    println!("JSON equiv ({} bytes)", json_equiv.len());
    println!(
        "Ratio: {:.1}%\n",
        binary.len() as f64 / json_equiv.len() as f64 * 100.0
    );
}

// ─── Example 5: Array of mixed values with negatives ────────────────────────

#[test]
fn example_5_mixed_array() {
    let input = r#"{
        measurements: [
            -40,
            0,
            23,
            100,
            3.14,
            -273.15,
            null,
            true,
            "overflow"
        ];
        unit: "celsius";
        timestamp: 1708876800;
    }"#;

    let json_equiv = r#"{"measurements":[-40,0,23,100,3.14,-273.15,null,true,"overflow"],"unit":"celsius","timestamp":1708876800}"#;

    let (_, binary, text) = full_roundtrip(input);

    println!("=== Example 5: Mixed Array ===");
    println!("Crous text:\n{text}");
    println!("Binary ({} bytes): {}", binary.len(), hex_string(&binary));
    println!("JSON equiv ({} bytes)", json_equiv.len());
    println!(
        "Ratio: {:.1}%\n",
        binary.len() as f64 / json_equiv.len() as f64 * 100.0
    );
}
