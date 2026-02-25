//! Crous Regression Benchmark Harness
//!
//! Usage:
//!   cargo run -p crous-bench --release -- [OPTIONS]
//!
//! Modes:
//!   --mode ci       Fast CI mode (reduced datasets, 3 runs)   [default]
//!   --mode full     Full benchmark mode (full datasets, 10 runs)
//!
//! Baseline:
//!   --save-baseline        Save results as the new baseline
//!   --baseline <path>      Compare against a specific baseline file
//!
//! Output:
//!   --output <dir>         Output directory [default: bench/results]

use std::path::PathBuf;

use chrono::Utc;
use clap::Parser;

use crous_bench::datasets;
use crous_bench::metrics::{self, BenchReport, RegressionThresholds, Severity, SystemInfo};
use crous_bench::report;
use crous_bench::runner;

#[derive(Parser)]
#[command(name = "crous-bench", about = "Crous regression benchmark harness")]
struct Cli {
    /// Benchmark mode: "ci" (fast) or "full" (deep).
    #[arg(long, default_value = "ci")]
    mode: String,

    /// Save current results as the new baseline.
    #[arg(long)]
    save_baseline: bool,

    /// Path to baseline results JSON for comparison.
    #[arg(long)]
    baseline: Option<PathBuf>,

    /// Output directory for results.
    #[arg(long, default_value = "bench/results")]
    output: PathBuf,

    /// Override the version tag (default: git HEAD or "dev").
    #[arg(long)]
    version: Option<String>,
}

fn main() {
    let cli = Cli::parse();

    let (iterations, mode_label) = match cli.mode.as_str() {
        "full" => (10, "full"),
        _ => (3, "ci"),
    };

    let version = cli.version.unwrap_or_else(|| {
        std::process::Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "dev".into())
    });

    eprintln!("╔══════════════════════════════════════════════════════════╗");
    eprintln!("║  Crous Regression Benchmark — {mode_label} mode                  ║");
    eprintln!("╚══════════════════════════════════════════════════════════╝");
    eprintln!();
    eprintln!("  Version:    {version}");
    eprintln!("  Iterations: {iterations}");
    eprintln!("  Output:     {}", cli.output.display());
    eprintln!();

    // Collect system info.
    let system = SystemInfo::collect();
    eprintln!(
        "  System:     {} {} ({} cores)",
        system.os, system.arch, system.cpu_cores
    );
    eprintln!("  CPU:        {}", system.cpu_model);
    eprintln!("  Rust:       {}", system.rust_version);
    eprintln!();

    // Generate datasets.
    eprintln!("▸ Generating datasets (v{})…", datasets::DATASET_VERSION);
    let dataset_list = if mode_label == "full" {
        datasets::generate_all()
    } else {
        datasets::generate_ci_subset()
    };

    for ds in &dataset_list {
        eprintln!("  • {} — sha256:{:.16}…", ds.name, ds.sha256);
    }
    eprintln!();

    // Run benchmarks.
    let mut all_measurements = Vec::new();
    for ds in &dataset_list {
        eprintln!("▸ Benchmarking: {} ({iterations} iterations)…", ds.name);
        let results = runner::run_dataset(ds, iterations);
        for m in &results {
            let mbps = m
                .throughput_mbps()
                .map(|v| format!("{v:.1} MB/s"))
                .unwrap_or_default();
            let size = m
                .serialized_size
                .map(|s| format!("{s}B"))
                .unwrap_or_default();
            eprintln!(
                "    {:<12} {:<8} median={:.1}µs  cv={:.1}%  {mbps}  {size}",
                m.format,
                m.operation,
                m.median_ns() as f64 / 1000.0,
                m.cv() * 100.0,
            );
        }
        all_measurements.extend(results);
    }
    eprintln!();

    // Build report.
    let report = BenchReport {
        version: version.clone(),
        timestamp: Utc::now().to_rfc3339(),
        system,
        mode: mode_label.into(),
        dataset_version: datasets::DATASET_VERSION.into(),
        measurements: all_measurements,
    };

    // Load baseline if provided.
    let baseline: Option<BenchReport> = cli.baseline.as_ref().and_then(|p| {
        let data = std::fs::read_to_string(p).ok()?;
        serde_json::from_str(&data).ok()
    });

    // Detect regressions.
    let thresholds = RegressionThresholds::default();
    let regressions = if let Some(ref base) = baseline {
        eprintln!("▸ Comparing against baseline: {}", base.version);
        metrics::detect_regressions(base, &report, &thresholds)
    } else {
        Vec::new()
    };

    // Print regression summary.
    let failures = regressions
        .iter()
        .filter(|r| r.severity == Severity::Failure)
        .count();
    let warnings = regressions
        .iter()
        .filter(|r| r.severity == Severity::Warning)
        .count();

    if failures > 0 {
        eprintln!();
        eprintln!("╔══════════════════════════════════════════════════════════╗");
        eprintln!(
            "║  ❌ REGRESSION DETECTED: {failures} failure(s), {warnings} warning(s)       ║"
        );
        eprintln!("╚══════════════════════════════════════════════════════════╝");
        for r in &regressions {
            if r.severity == Severity::Failure {
                eprintln!(
                    "  FAIL: {}/{} — {} {:+.1}% (threshold: {:.0}%)",
                    r.format, r.dataset, r.metric, r.change_pct, r.threshold_pct
                );
            }
        }
    } else if warnings > 0 {
        eprintln!();
        eprintln!("  ⚠️  {warnings} warning(s), no failures");
    } else if baseline.is_some() {
        eprintln!();
        eprintln!("  ✅ No regressions detected");
    }
    eprintln!();

    // Write output artifacts.
    eprintln!("▸ Writing results to {}…", cli.output.display());
    report::write_all(&cli.output, &report, &regressions, baseline.as_ref()).unwrap();

    // Optionally save as baseline.
    if cli.save_baseline {
        let baseline_path = cli.output.join("baseline.json");
        let json = serde_json::to_string_pretty(&report).unwrap();
        std::fs::write(&baseline_path, json).unwrap();
        eprintln!("  ✓ Saved baseline to {}", baseline_path.display());
    }

    eprintln!("▸ Done.");
    eprintln!();

    // Exit with failure code if regressions detected.
    if failures > 0 {
        std::process::exit(1);
    }
}
