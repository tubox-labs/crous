//! Report generation: raw JSON, summary CSV, regression Markdown.

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::metrics::{BenchReport, Measurement, Regression, Severity};

/// Write all output artifacts to the given directory.
pub fn write_all(
    dir: &Path,
    report: &BenchReport,
    regressions: &[Regression],
    baseline: Option<&BenchReport>,
) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;

    // 1. raw.json
    let raw_json = serde_json::to_string_pretty(report).unwrap();
    fs::write(dir.join("raw.json"), &raw_json)?;

    // 2. system_info.json
    let sys_json = serde_json::to_string_pretty(&report.system).unwrap();
    fs::write(dir.join("system_info.json"), &sys_json)?;

    // 3. summary.csv
    write_csv(dir, &report.measurements)?;

    // 4. regression_report.md
    write_regression_md(dir, report, regressions, baseline)?;

    // 5. size_comparison.md
    write_size_comparison(dir, &report.measurements)?;

    Ok(())
}

fn write_csv(dir: &Path, measurements: &[Measurement]) -> std::io::Result<()> {
    let path = dir.join("summary.csv");
    let mut wtr = csv::Writer::from_path(&path)?;

    wtr.write_record([
        "format",
        "dataset",
        "operation",
        "median_ns",
        "mean_ns",
        "p95_ns",
        "p99_ns",
        "min_ns",
        "max_ns",
        "stddev_ns",
        "cv_pct",
        "throughput_mbps",
        "serialized_size",
        "runs",
    ])?;

    for m in measurements {
        wtr.write_record([
            &m.format,
            &m.dataset,
            &m.operation,
            &m.median_ns().to_string(),
            &format!("{:.0}", m.mean_ns()),
            &m.p95_ns().to_string(),
            &m.p99_ns().to_string(),
            &m.min_ns().to_string(),
            &m.max_ns().to_string(),
            &format!("{:.0}", m.stddev_ns()),
            &format!("{:.2}", m.cv() * 100.0),
            &m.throughput_mbps()
                .map(|v| format!("{v:.1}"))
                .unwrap_or_else(|| "-".into()),
            &m.serialized_size
                .map(|s| s.to_string())
                .unwrap_or_else(|| "-".into()),
            &m.durations_ns.len().to_string(),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}

fn write_regression_md(
    dir: &Path,
    report: &BenchReport,
    regressions: &[Regression],
    baseline: Option<&BenchReport>,
) -> std::io::Result<()> {
    let path = dir.join("regression_report.md");
    let mut f = fs::File::create(&path)?;

    writeln!(f, "# Crous Regression Benchmark Report")?;
    writeln!(f)?;
    writeln!(f, "**Version:** `{}`", report.version)?;
    writeln!(f, "**Timestamp:** {}", report.timestamp)?;
    writeln!(f, "**Mode:** {}", report.mode)?;
    writeln!(f, "**Dataset version:** {}", report.dataset_version)?;
    writeln!(f)?;

    // System info.
    writeln!(f, "## System")?;
    writeln!(f)?;
    writeln!(f, "| Property | Value |")?;
    writeln!(f, "|----------|-------|")?;
    writeln!(f, "| OS | {} | ", report.system.os)?;
    writeln!(f, "| Arch | {} |", report.system.arch)?;
    writeln!(f, "| CPU | {} |", report.system.cpu_model)?;
    writeln!(f, "| Cores | {} |", report.system.cpu_cores)?;
    writeln!(f, "| RAM | {} MB |", report.system.ram_mb)?;
    writeln!(f, "| Rust | {} |", report.system.rust_version)?;
    writeln!(f)?;

    // Overall status.
    let failures = regressions.iter().filter(|r| r.severity == Severity::Failure).count();
    let warnings = regressions.iter().filter(|r| r.severity == Severity::Warning).count();

    if failures > 0 {
        writeln!(f, "## ❌ REGRESSION DETECTED")?;
        writeln!(f)?;
        writeln!(f, "**{failures} failure(s), {warnings} warning(s)**")?;
    } else if warnings > 0 {
        writeln!(f, "## ⚠️  WARNINGS")?;
        writeln!(f)?;
        writeln!(f, "**{warnings} warning(s), 0 failures**")?;
    } else {
        writeln!(f, "## ✅ NO REGRESSIONS DETECTED")?;
    }
    writeln!(f)?;

    // Regression details.
    if !regressions.is_empty() {
        writeln!(f, "### Regression Details")?;
        writeln!(f)?;
        writeln!(
            f,
            "| Severity | Format | Dataset | Metric | Baseline | Current | Change | Threshold |"
        )?;
        writeln!(
            f,
            "|----------|--------|---------|--------|----------|---------|--------|-----------|"
        )?;
        for r in regressions {
            let icon = match r.severity {
                Severity::Failure => "❌",
                Severity::Warning => "⚠️",
            };
            writeln!(
                f,
                "| {} | {} | {} | {} | {:.0} | {:.0} | {:+.1}% | {:.0}% |",
                icon,
                r.format,
                r.dataset,
                r.metric,
                r.baseline_value,
                r.current_value,
                r.change_pct,
                r.threshold_pct,
            )?;
        }
        writeln!(f)?;
    }

    // Performance summary table.
    writeln!(f, "## Performance Summary")?;
    writeln!(f)?;
    writeln!(
        f,
        "| Format | Dataset | Op | Median (µs) | p95 (µs) | CV% | MB/s | Size |"
    )?;
    writeln!(
        f,
        "|--------|---------|-----|-------------|----------|-----|------|------|"
    )?;

    for m in &report.measurements {
        let median_us = m.median_ns() as f64 / 1000.0;
        let p95_us = m.p95_ns() as f64 / 1000.0;
        let mbps = m
            .throughput_mbps()
            .map(|v| format!("{v:.1}"))
            .unwrap_or_else(|| "-".into());
        let size = m
            .serialized_size
            .map(format_bytes)
            .unwrap_or_else(|| "-".into());

        writeln!(
            f,
            "| {} | {} | {} | {:.1} | {:.1} | {:.1} | {} | {} |",
            m.format,
            m.dataset,
            m.operation,
            median_us,
            p95_us,
            m.cv() * 100.0,
            mbps,
            size,
        )?;
    }
    writeln!(f)?;

    // Baseline comparison if available.
    if let Some(base) = baseline {
        writeln!(f, "## Baseline Comparison")?;
        writeln!(f)?;
        writeln!(f, "**Baseline version:** `{}`", base.version)?;
        writeln!(f, "**Baseline timestamp:** {}", base.timestamp)?;
    }

    writeln!(f)?;
    writeln!(
        f,
        "---\n*Generated by crous-bench v{} on {}*",
        env!("CARGO_PKG_VERSION"),
        report.timestamp,
    )?;

    Ok(())
}

fn write_size_comparison(dir: &Path, measurements: &[Measurement]) -> std::io::Result<()> {
    let path = dir.join("size_comparison.md");
    let mut f = fs::File::create(&path)?;

    writeln!(f, "# Size Comparison")?;
    writeln!(f)?;
    writeln!(f, "| Dataset | Crous | Crous+Dedup | JSON | MsgPack | CBOR | Crous/JSON |")?;
    writeln!(f, "|---------|-------|-------------|------|---------|------|------------|")?;

    // Collect encode sizes per dataset.
    let datasets: Vec<&str> = measurements
        .iter()
        .filter(|m| m.operation == "encode" && m.format == "crous")
        .map(|m| m.dataset.as_str())
        .collect();

    for ds in &datasets {
        let get_size = |fmt: &str| -> String {
            measurements
                .iter()
                .find(|m| m.dataset == *ds && m.format == fmt && m.operation == "encode")
                .and_then(|m| m.serialized_size)
                .map(format_bytes)
                .unwrap_or_else(|| "-".into())
        };
        let get_size_raw = |fmt: &str| -> Option<usize> {
            measurements
                .iter()
                .find(|m| m.dataset == *ds && m.format == fmt && m.operation == "encode")
                .and_then(|m| m.serialized_size)
        };

        let ratio = match (get_size_raw("crous"), get_size_raw("json")) {
            (Some(c), Some(j)) if j > 0 => format!("{:.2}×", c as f64 / j as f64),
            _ => "-".into(),
        };

        writeln!(
            f,
            "| {} | {} | {} | {} | {} | {} | {} |",
            ds,
            get_size("crous"),
            get_size("crous_dedup"),
            get_size("json"),
            get_size("msgpack"),
            get_size("cbor"),
            ratio,
        )?;
    }

    Ok(())
}

fn format_bytes(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}
