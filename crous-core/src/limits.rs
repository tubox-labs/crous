//! Configurable resource limits for decoding, to prevent denial-of-service attacks.
//!
//! All limits have sane defaults and can be overridden per-decoder instance.

/// Resource limits for decoder operations.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Maximum nesting depth for objects/arrays (default: 128).
    pub max_nesting_depth: usize,
    /// Maximum size of a single block in bytes (default: 64 MiB).
    pub max_block_size: usize,
    /// Maximum number of items in a single array or object (default: 1M).
    pub max_items: usize,
    /// Maximum total memory allocation for a single decode session (default: 256 MiB).
    pub max_memory: usize,
    /// Maximum string length in bytes (default: 16 MiB).
    pub max_string_length: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_nesting_depth: 128,
            max_block_size: 64 * 1024 * 1024,    // 64 MiB
            max_items: 1_000_000,                // 1M items
            max_memory: 256 * 1024 * 1024,       // 256 MiB
            max_string_length: 16 * 1024 * 1024, // 16 MiB
        }
    }
}

impl Limits {
    /// Restrictive limits suitable for untrusted input.
    pub fn strict() -> Self {
        Self {
            max_nesting_depth: 32,
            max_block_size: 1024 * 1024, // 1 MiB
            max_items: 10_000,
            max_memory: 4 * 1024 * 1024, // 4 MiB
            max_string_length: 65536,    // 64 KiB
        }
    }

    /// No limits — for trusted data only.
    pub fn unlimited() -> Self {
        Self {
            max_nesting_depth: usize::MAX,
            max_block_size: usize::MAX,
            max_items: usize::MAX,
            max_memory: usize::MAX,
            max_string_length: usize::MAX,
        }
    }
}
