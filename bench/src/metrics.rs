//! Metrics collection, serialization, and regression detection.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

/// All metrics for a single (format × dataset × operation) measurement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Measurement {
    pub format: String,
    pub dataset: String,
    pub operation: String, // "encode" | "decode" | "roundtrip"
    /// Individual run durations.
    pub durations_ns: Vec<u64>,
    /// Serialized size in bytes (for encode).
    pub serialized_size: Option<usize>,
    /// Peak RSS delta in bytes (if measurable).
    pub peak_rss_bytes: Option<usize>,
}

impl Measurement {
    pub fn new(format: &str, dataset: &str, operation: &str) -> Self {
        Self {
            format: format.into(),
            dataset: dataset.into(),
            operation: operation.into(),
            durations_ns: Vec::new(),
            serialized_size: None,
            peak_rss_bytes: None,
        }
    }

    pub fn add_duration(&mut self, d: Duration) {
        self.durations_ns.push(d.as_nanos() as u64);
    }

    /// Median duration in nanoseconds.
    pub fn median_ns(&self) -> u64 {
        if self.durations_ns.is_empty() {
            return 0;
        }
        let mut sorted = self.durations_ns.clone();
        sorted.sort_unstable();
        sorted[sorted.len() / 2]
    }

    /// p95 duration in nanoseconds.
    pub fn p95_ns(&self) -> u64 {
        if self.durations_ns.is_empty() {
            return 0;
        }
        let mut sorted = self.durations_ns.clone();
        sorted.sort_unstable();
        let idx = ((sorted.len() as f64) * 0.95).ceil() as usize - 1;
        sorted[idx.min(sorted.len() - 1)]
    }

    /// p99 duration in nanoseconds.
    pub fn p99_ns(&self) -> u64 {
        if self.durations_ns.is_empty() {
            return 0;
        }
        let mut sorted = self.durations_ns.clone();
        sorted.sort_unstable();
        let idx = ((sorted.len() as f64) * 0.99).ceil() as usize - 1;
        sorted[idx.min(sorted.len() - 1)]
    }

    pub fn min_ns(&self) -> u64 {
        self.durations_ns.iter().copied().min().unwrap_or(0)
    }

    pub fn max_ns(&self) -> u64 {
        self.durations_ns.iter().copied().max().unwrap_or(0)
    }

    pub fn mean_ns(&self) -> f64 {
        if self.durations_ns.is_empty() {
            return 0.0;
        }
        self.durations_ns.iter().sum::<u64>() as f64 / self.durations_ns.len() as f64
    }

    pub fn stddev_ns(&self) -> f64 {
        if self.durations_ns.len() < 2 {
            return 0.0;
        }
        let mean = self.mean_ns();
        let variance = self.durations_ns.iter()
            .map(|&v| {
                let diff = v as f64 - mean;
                diff * diff
            })
            .sum::<f64>()
            / (self.durations_ns.len() - 1) as f64;
        variance.sqrt()
    }

    /// Coefficient of variation (stddev / mean).
    pub fn cv(&self) -> f64 {
        let mean = self.mean_ns();
        if mean == 0.0 {
            return 0.0;
        }
        self.stddev_ns() / mean
    }

    /// Throughput in MB/s (if serialized_size is known).
    pub fn throughput_mbps(&self) -> Option<f64> {
        let size = self.serialized_size? as f64;
        let median = self.median_ns() as f64;
        if median == 0.0 {
            return None;
        }
        Some(size / median * 1_000.0) // bytes/ns → MB/s
    }
}

/// A complete benchmark report with all measurements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchReport {
    /// Commit hash or version tag.
    pub version: String,
    /// Timestamp (ISO 8601).
    pub timestamp: String,
    /// System information.
    pub system: SystemInfo,
    /// Benchmark mode.
    pub mode: String,
    /// Dataset version.
    pub dataset_version: String,
    /// All measurements.
    pub measurements: Vec<Measurement>,
}

/// System hardware/software info for reproducibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub cpu_model: String,
    pub cpu_cores: usize,
    pub ram_mb: u64,
    pub rust_version: String,
}

impl SystemInfo {
    pub fn collect() -> Self {
        Self {
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            cpu_model: Self::cpu_model(),
            cpu_cores: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
            ram_mb: Self::ram_mb(),
            rust_version: Self::rust_version(),
        }
    }

    fn cpu_model() -> String {
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("sysctl")
                .args(["-n", "machdep.cpu.brand_string"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| "unknown".into())
        }
        #[cfg(target_os = "linux")]
        {
            std::fs::read_to_string("/proc/cpuinfo")
                .ok()
                .and_then(|s| {
                    s.lines()
                        .find(|l| l.starts_with("model name"))
                        .map(|l| l.split(':').nth(1).unwrap_or("").trim().to_string())
                })
                .unwrap_or_else(|| "unknown".into())
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            "unknown".into()
        }
    }

    fn ram_mb() -> u64 {
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("sysctl")
                .args(["-n", "hw.memsize"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .and_then(|s| s.trim().parse::<u64>().ok())
                .map(|b| b / (1024 * 1024))
                .unwrap_or(0)
        }
        #[cfg(target_os = "linux")]
        {
            std::fs::read_to_string("/proc/meminfo")
                .ok()
                .and_then(|s| {
                    s.lines()
                        .find(|l| l.starts_with("MemTotal:"))
                        .and_then(|l| {
                            l.split_whitespace().nth(1)?.parse::<u64>().ok()
                        })
                })
                .map(|kb| kb / 1024)
                .unwrap_or(0)
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            0
        }
    }

    fn rust_version() -> String {
        std::process::Command::new("rustc")
            .arg("--version")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".into())
    }
}

// ── Regression Detection ────────────────────────────────────────────

/// Thresholds for regression detection.
#[derive(Debug, Clone)]
pub struct RegressionThresholds {
    /// Maximum allowed throughput decrease (0.05 = 5%).
    pub max_throughput_decrease: f64,
    /// Maximum allowed size increase (0.05 = 5%).
    pub max_size_increase: f64,
    /// Maximum allowed p95 latency increase (0.10 = 10%).
    pub max_p95_increase: f64,
    /// Maximum allowed memory increase (0.10 = 10%).
    pub max_memory_increase: f64,
    /// Maximum acceptable CV before flagging unstable results.
    pub max_cv: f64,
}

impl Default for RegressionThresholds {
    fn default() -> Self {
        Self {
            max_throughput_decrease: 0.05,
            max_size_increase: 0.05,
            max_p95_increase: 0.10,
            max_memory_increase: 0.10,
            max_cv: 0.05,
        }
    }
}

/// A single regression finding.
#[derive(Debug, Clone, Serialize)]
pub struct Regression {
    pub format: String,
    pub dataset: String,
    pub metric: String,
    pub baseline_value: f64,
    pub current_value: f64,
    pub change_pct: f64,
    pub threshold_pct: f64,
    pub severity: Severity,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Failure,
}

/// Compare two reports and detect regressions.
pub fn detect_regressions(
    baseline: &BenchReport,
    current: &BenchReport,
    thresholds: &RegressionThresholds,
) -> Vec<Regression> {
    let mut regressions = Vec::new();

    // Index baseline by (format, dataset, operation).
    let mut baseline_map: BTreeMap<(String, String, String), &Measurement> = BTreeMap::new();
    for m in &baseline.measurements {
        baseline_map.insert(
            (m.format.clone(), m.dataset.clone(), m.operation.clone()),
            m,
        );
    }

    for cur in &current.measurements {
        let key = (cur.format.clone(), cur.dataset.clone(), cur.operation.clone());
        let Some(base) = baseline_map.get(&key) else {
            continue; // New measurement, no baseline to compare.
        };

        // 1. Throughput regression (median time increased → throughput decreased).
        let base_median = base.median_ns() as f64;
        let cur_median = cur.median_ns() as f64;
        if base_median > 0.0 && cur_median > 0.0 {
            let change = (cur_median - base_median) / base_median;
            if change > thresholds.max_throughput_decrease {
                regressions.push(Regression {
                    format: cur.format.clone(),
                    dataset: cur.dataset.clone(),
                    metric: format!("{}_median_ns", cur.operation),
                    baseline_value: base_median,
                    current_value: cur_median,
                    change_pct: change * 100.0,
                    threshold_pct: thresholds.max_throughput_decrease * 100.0,
                    severity: if change > thresholds.max_throughput_decrease * 2.0 {
                        Severity::Failure
                    } else {
                        Severity::Warning
                    },
                });
            }
        }

        // 2. Size regression.
        if let (Some(base_size), Some(cur_size)) =
            (base.serialized_size, cur.serialized_size)
            && base_size > 0
        {
            let change = (cur_size as f64 - base_size as f64) / base_size as f64;
            if change > thresholds.max_size_increase {
                regressions.push(Regression {
                    format: cur.format.clone(),
                    dataset: cur.dataset.clone(),
                    metric: "serialized_size".into(),
                    baseline_value: base_size as f64,
                    current_value: cur_size as f64,
                    change_pct: change * 100.0,
                    threshold_pct: thresholds.max_size_increase * 100.0,
                    severity: Severity::Failure,
                });
            }
        }

        // 3. p95 latency regression.
        let base_p95 = base.p95_ns() as f64;
        let cur_p95 = cur.p95_ns() as f64;
        if base_p95 > 0.0 && cur_p95 > 0.0 {
            let change = (cur_p95 - base_p95) / base_p95;
            if change > thresholds.max_p95_increase {
                regressions.push(Regression {
                    format: cur.format.clone(),
                    dataset: cur.dataset.clone(),
                    metric: format!("{}_p95_ns", cur.operation),
                    baseline_value: base_p95,
                    current_value: cur_p95,
                    change_pct: change * 100.0,
                    threshold_pct: thresholds.max_p95_increase * 100.0,
                    severity: if change > thresholds.max_p95_increase * 2.0 {
                        Severity::Failure
                    } else {
                        Severity::Warning
                    },
                });
            }
        }

        // 4. Memory regression.
        if let (Some(base_mem), Some(cur_mem)) =
            (base.peak_rss_bytes, cur.peak_rss_bytes)
            && base_mem > 0
        {
            let change = (cur_mem as f64 - base_mem as f64) / base_mem as f64;
            if change > thresholds.max_memory_increase {
                regressions.push(Regression {
                    format: cur.format.clone(),
                    dataset: cur.dataset.clone(),
                    metric: "peak_rss_bytes".into(),
                    baseline_value: base_mem as f64,
                    current_value: cur_mem as f64,
                    change_pct: change * 100.0,
                    threshold_pct: thresholds.max_memory_increase * 100.0,
                    severity: Severity::Failure,
                });
            }
        }

        // 5. Stability warning.
        if cur.cv() > thresholds.max_cv {
            regressions.push(Regression {
                format: cur.format.clone(),
                dataset: cur.dataset.clone(),
                metric: format!("{}_stability_cv", cur.operation),
                baseline_value: base.cv() * 100.0,
                current_value: cur.cv() * 100.0,
                change_pct: cur.cv() * 100.0,
                threshold_pct: thresholds.max_cv * 100.0,
                severity: Severity::Warning,
            });
        }
    }

    regressions
}
