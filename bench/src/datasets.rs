//! Deterministic dataset generator for regression benchmarks.
//!
//! All datasets are generated from fixed seeds so results are reproducible
//! across machines and commits. Dataset integrity is verified via SHA-256 hashes.

use crous_core::Value;
use sha2::{Digest, Sha256};

/// Simple deterministic PRNG (xorshift64) — no external deps, fixed output.
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 16) as u32
    }

    /// Generate a deterministic ASCII string of given length.
    pub fn next_string(&mut self, len: usize) -> String {
        (0..len)
            .map(|_| {
                let c = (self.next_u32() % 26) as u8 + b'a';
                c as char
            })
            .collect()
    }

    /// Random integer in [0, max).
    pub fn next_range(&mut self, max: u64) -> u64 {
        self.next_u64() % max
    }

    /// Random bytes.
    pub fn next_bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| (self.next_u64() & 0xFF) as u8).collect()
    }
}

/// Dataset version — bump when dataset generation logic changes.
pub const DATASET_VERSION: &str = "1.0.0";

/// A named benchmark dataset with its Crous Value and metadata.
#[derive(Clone)]
pub struct Dataset {
    pub name: &'static str,
    pub description: String,
    pub value: Value,
    pub sha256: String,
}

impl Dataset {
    fn new(name: &'static str, description: impl Into<String>, value: Value) -> Self {
        let description = description.into();
        // Compute hash over the JSON representation for version tracking.
        let json: serde_json::Value = (&value).into();
        let canonical = serde_json::to_string(&json).unwrap();
        let hash = Sha256::digest(canonical.as_bytes());
        Self {
            name,
            description,
            value,
            sha256: format!("{hash:x}"),
        }
    }
}

/// Generate all benchmark datasets. Output is deterministic.
pub fn generate_all() -> Vec<Dataset> {
    vec![
        gen_small_objects(),
        gen_string_heavy(),
        gen_nested_deep(),
        gen_binary_blobs(),
        gen_mixed_api_events(),
        gen_numeric_heavy(),
    ]
}

/// Generate a reduced set for CI fast mode.
pub fn generate_ci_subset() -> Vec<Dataset> {
    vec![
        gen_small_objects_ci(),
        gen_string_heavy_ci(),
        gen_nested_deep(),
        gen_binary_blobs_ci(),
        gen_mixed_api_events_ci(),
        gen_numeric_heavy_ci(),
    ]
}

// ── Dataset: Small Objects ──────────────────────────────────────────

fn gen_small_objects() -> Dataset {
    gen_small_objects_n(100_000)
}

fn gen_small_objects_ci() -> Dataset {
    gen_small_objects_n(1_000)
}

fn gen_small_objects_n(n: usize) -> Dataset {
    let mut rng = Rng::new(0xC005_0001);
    let items: Vec<Value> = (0..n)
        .map(|i| {
            Value::Object(vec![
                ("id".into(), Value::UInt(i as u64)),
                ("name".into(), Value::Str(rng.next_string(8 + (i % 12)))),
                (
                    "email".into(),
                    Value::Str(format!("{}@example.com", rng.next_string(6))),
                ),
                (
                    "active".into(),
                    Value::Bool(rng.next_u32().is_multiple_of(2)),
                ),
                ("score".into(), Value::Float(rng.next_u32() as f64 / 100.0)),
                ("level".into(), Value::UInt(rng.next_range(100))),
            ])
        })
        .collect();
    Dataset::new(
        "small_objects",
        format!("{n} small objects with 6 fields each"),
        Value::Array(items),
    )
}

// ── Dataset: String-Heavy ───────────────────────────────────────────

fn gen_string_heavy() -> Dataset {
    gen_string_heavy_n(10_000)
}

fn gen_string_heavy_ci() -> Dataset {
    gen_string_heavy_n(500)
}

fn gen_string_heavy_n(n: usize) -> Dataset {
    let mut rng = Rng::new(0xC005_0002);

    // Pool of repeated strings (tests dedup effectiveness).
    let pool: Vec<String> = (0..50).map(|_| rng.next_string(20)).collect();

    let items: Vec<Value> = (0..n)
        .map(|i| {
            // 60% chance of picking from pool, 40% unique.
            let val = if rng.next_range(100) < 60 {
                pool[rng.next_range(pool.len() as u64) as usize].clone()
            } else {
                rng.next_string(15 + (i % 25))
            };
            Value::Object(vec![
                ("key".into(), Value::Str(format!("item-{i:06}"))),
                ("value".into(), Value::Str(val)),
                (
                    "tags".into(),
                    Value::Array(vec![
                        Value::Str(pool[rng.next_range(pool.len() as u64) as usize].clone()),
                        Value::Str(pool[rng.next_range(pool.len() as u64) as usize].clone()),
                    ]),
                ),
            ])
        })
        .collect();
    Dataset::new(
        "string_heavy",
        "String-heavy payload with ~60% repeated strings from a pool of 50",
        Value::Array(items),
    )
}

// ── Dataset: Deeply Nested ──────────────────────────────────────────

fn gen_nested_deep() -> Dataset {
    let mut rng = Rng::new(0xC005_0003);

    // Build a tree: 10 levels deep, branching factor 3.
    fn build_tree(rng: &mut Rng, depth: usize, max_depth: usize) -> Value {
        if depth >= max_depth {
            return match rng.next_range(3) {
                0 => Value::UInt(rng.next_range(10000)),
                1 => Value::Str(rng.next_string(10)),
                _ => Value::Bool(rng.next_u32().is_multiple_of(2)),
            };
        }
        let children: Vec<Value> = (0..3)
            .map(|_| build_tree(rng, depth + 1, max_depth))
            .collect();
        Value::Object(vec![
            (format!("d{depth}"), Value::Array(children)),
            ("meta".into(), Value::Str(rng.next_string(8))),
        ])
    }

    // Also add some extreme-depth linear nesting (depth 50).
    let mut linear = Value::UInt(42);
    for i in 0..50 {
        linear = Value::Object(vec![(format!("l{i}"), linear)]);
    }

    let tree = build_tree(&mut rng, 0, 10);
    let combined = Value::Object(vec![("tree".into(), tree), ("linear_50".into(), linear)]);

    Dataset::new(
        "nested_deep",
        "10-level tree (branching=3) + 50-level linear nesting",
        combined,
    )
}

// ── Dataset: Binary Blobs ───────────────────────────────────────────

fn gen_binary_blobs() -> Dataset {
    gen_binary_blobs_n(100, 65536)
}

fn gen_binary_blobs_ci() -> Dataset {
    gen_binary_blobs_n(10, 16384)
}

fn gen_binary_blobs_n(n: usize, blob_size: usize) -> Dataset {
    let mut rng = Rng::new(0xC005_0004);
    let items: Vec<Value> = (0..n)
        .map(|i| {
            let size = blob_size / 2 + rng.next_range(blob_size as u64) as usize;
            Value::Object(vec![
                ("id".into(), Value::UInt(i as u64)),
                ("mime".into(), Value::Str("application/octet-stream".into())),
                ("size".into(), Value::UInt(size as u64)),
                ("data".into(), Value::Bytes(rng.next_bytes(size))),
            ])
        })
        .collect();
    Dataset::new(
        "binary_blobs",
        format!("{n} records with ~{blob_size}B binary payloads"),
        Value::Array(items),
    )
}

// ── Dataset: Mixed API Events ───────────────────────────────────────

fn gen_mixed_api_events() -> Dataset {
    gen_mixed_api_events_n(5_000)
}

fn gen_mixed_api_events_ci() -> Dataset {
    gen_mixed_api_events_n(200)
}

fn gen_mixed_api_events_n(n: usize) -> Dataset {
    let mut rng = Rng::new(0xC005_0005);
    let event_types = [
        "push",
        "pull_request",
        "issue",
        "comment",
        "review",
        "deployment",
        "release",
        "fork",
        "star",
        "watch",
    ];
    let repos = [
        "rust-lang/rust",
        "tokio-rs/tokio",
        "serde-rs/serde",
        "crous-format/crous",
        "hyperium/hyper",
        "actix/actix-web",
        "diesel-rs/diesel",
    ];

    let items: Vec<Value> = (0..n)
        .map(|i| {
            let etype = event_types[rng.next_range(event_types.len() as u64) as usize];
            let repo = repos[rng.next_range(repos.len() as u64) as usize];
            Value::Object(vec![
                ("id".into(), Value::UInt(1_000_000 + i as u64)),
                ("type".into(), Value::Str(etype.into())),
                ("repo".into(), Value::Str(repo.into())),
                (
                    "actor".into(),
                    Value::Object(vec![
                        ("login".into(), Value::Str(rng.next_string(10))),
                        ("id".into(), Value::UInt(rng.next_range(100_000))),
                    ]),
                ),
                (
                    "payload".into(),
                    Value::Object(vec![
                        ("action".into(), Value::Str("created".into())),
                        ("number".into(), Value::UInt(rng.next_range(10_000))),
                        (
                            "title".into(),
                            Value::Str(format!("Fix {} in {}", rng.next_string(12), repo)),
                        ),
                        ("body".into(), {
                            let body_len = 50 + rng.next_range(200) as usize;
                            Value::Str(rng.next_string(body_len))
                        }),
                        (
                            "labels".into(),
                            Value::Array(
                                (0..rng.next_range(4))
                                    .map(|_| Value::Str(rng.next_string(8)))
                                    .collect(),
                            ),
                        ),
                    ]),
                ),
                ("public".into(), Value::Bool(true)),
                (
                    "created_at".into(),
                    Value::Str(format!(
                        "2026-02-{:02}T{:02}:{:02}:{:02}Z",
                        1 + rng.next_range(28),
                        rng.next_range(24),
                        rng.next_range(60),
                        rng.next_range(60)
                    )),
                ),
            ])
        })
        .collect();
    Dataset::new(
        "mixed_api_events",
        "Synthetic GitHub-like API event stream",
        Value::Array(items),
    )
}

// ── Dataset: Numeric Heavy ──────────────────────────────────────────

fn gen_numeric_heavy() -> Dataset {
    gen_numeric_heavy_n(50_000)
}

fn gen_numeric_heavy_ci() -> Dataset {
    gen_numeric_heavy_n(2_000)
}

fn gen_numeric_heavy_n(n: usize) -> Dataset {
    let mut rng = Rng::new(0xC005_0006);
    let items: Vec<Value> = (0..n)
        .map(|_| {
            Value::Object(vec![
                ("i64_val".into(), Value::Int(rng.next_u64() as i64)),
                ("u64_val".into(), Value::UInt(rng.next_u64())),
                (
                    "f64_val".into(),
                    Value::Float(f64::from_bits(rng.next_u64() & 0x7FEFFFFFFFFFFFFF)),
                ),
                ("small".into(), Value::UInt(rng.next_range(256))),
                (
                    "negative".into(),
                    Value::Int(-(rng.next_range(100_000) as i64)),
                ),
            ])
        })
        .collect();
    Dataset::new(
        "numeric_heavy",
        format!("{n} records with mixed integer/float fields"),
        Value::Array(items),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datasets_are_deterministic() {
        let a = generate_all();
        let b = generate_all();
        for (da, db) in a.iter().zip(b.iter()) {
            assert_eq!(
                da.sha256, db.sha256,
                "Dataset '{}' is non-deterministic",
                da.name
            );
        }
    }

    #[test]
    fn ci_subset_is_deterministic() {
        let a = generate_ci_subset();
        let b = generate_ci_subset();
        for (da, db) in a.iter().zip(b.iter()) {
            assert_eq!(
                da.sha256, db.sha256,
                "CI dataset '{}' is non-deterministic",
                da.name
            );
        }
    }
}
