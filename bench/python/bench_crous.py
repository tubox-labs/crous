#!/usr/bin/env python3
"""
Crous Python Benchmark Harness

Measures encode/decode throughput for the Python crous implementation
and compares against stdlib json and msgpack (if available).

Usage:
    python bench_crous.py [--mode ci|full] [--output DIR] [--baseline FILE]
"""

import argparse
import gc
import hashlib
import json
import os
import platform
import statistics
import sys
import time
from pathlib import Path

# Ensure the python package is importable
sys.path.insert(0, str(Path(__file__).resolve().parent.parent.parent / "python"))

import crous  # noqa: E402

# ── Deterministic PRNG (matches Rust xorshift64) ─────────────────────

class Rng:
    """Deterministic xorshift64 PRNG matching the Rust benchmark harness."""

    def __init__(self, seed: int):
        self.state = seed & 0xFFFF_FFFF_FFFF_FFFF

    def next_u64(self) -> int:
        x = self.state
        x ^= (x << 13) & 0xFFFF_FFFF_FFFF_FFFF
        x ^= (x >> 7) & 0xFFFF_FFFF_FFFF_FFFF
        x ^= (x << 17) & 0xFFFF_FFFF_FFFF_FFFF
        self.state = x & 0xFFFF_FFFF_FFFF_FFFF
        return self.state

    def next_range(self, max_val: int) -> int:
        return self.next_u64() % max_val

    def next_bool(self) -> bool:
        return (self.next_u64() & 1) == 1

    def next_string(self, length: int) -> str:
        chars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
        return "".join(chars[self.next_range(len(chars))] for _ in range(length))


# ── Dataset generation ───────────────────────────────────────────────

SEEDS = {
    "small_objects": 0xC005_0001,
    "string_heavy": 0xC005_0002,
    "nested_deep": 0xC005_0003,
    "binary_blobs": 0xC005_0004,
    "mixed_api_events": 0xC005_0005,
    "numeric_heavy": 0xC005_0006,
}


def gen_small_objects(n: int) -> dict:
    rng = Rng(SEEDS["small_objects"])
    items = []
    for _ in range(n):
        items.append({
            "id": rng.next_range(1_000_000),
            "name": rng.next_string(20),
            "active": rng.next_bool(),
            "score": rng.next_range(10000) / 100.0,
        })
    return {"type": "small_objects", "count": n, "items": items}


def gen_string_heavy(n: int) -> dict:
    rng = Rng(SEEDS["string_heavy"])
    pool = [rng.next_string(30 + rng.next_range(70)) for _ in range(50)]
    entries = []
    for _ in range(n):
        if rng.next_range(100) < 60:
            entries.append(pool[rng.next_range(len(pool))])
        else:
            entries.append(rng.next_string(10 + rng.next_range(90)))
    return {"type": "string_heavy", "pool_size": len(pool), "entries": entries}


def gen_nested_deep(rng: Rng, depth: int) -> dict:
    node = {
        "label": rng.next_string(12),
        "value": rng.next_range(1000),
    }
    if depth > 0:
        children_count = 2 + rng.next_range(3)
        node["children"] = [gen_nested_deep(rng, depth - 1) for _ in range(children_count)]
    return node


def gen_nested(depth: int) -> dict:
    rng = Rng(SEEDS["nested_deep"])
    tree = gen_nested_deep(rng, depth)
    # linear chain
    chain = {"data": rng.next_string(20)}
    current = chain
    for _ in range(depth * 5):
        inner = {"data": rng.next_string(20)}
        current["next"] = inner
        current = inner
    return {"type": "nested_deep", "tree": tree, "chain": chain}


def gen_binary_blobs(n: int) -> dict:
    rng = Rng(SEEDS["binary_blobs"])
    records = []
    for _ in range(n):
        size = 60000 + rng.next_range(8000)
        # Python crous uses bytes for binary; generate deterministic bytes
        data = bytes(rng.next_range(256) for _ in range(size))
        records.append({
            "id": rng.next_string(16),
            "mime": "application/octet-stream",
            "data_b64": data.hex(),  # store as hex string since JSON can't do binary
            "size": size,
        })
    return {"type": "binary_blobs", "count": n, "records": records}


def gen_mixed_api_events(n: int) -> dict:
    rng = Rng(SEEDS["mixed_api_events"])
    event_types = ["PushEvent", "PullRequestEvent", "IssueCommentEvent",
                   "WatchEvent", "CreateEvent"]
    events = []
    for _ in range(n):
        etype = event_types[rng.next_range(len(event_types))]
        repo = f"org-{rng.next_range(100)}/repo-{rng.next_range(500)}"
        body_len = 50 + rng.next_range(200)
        evt = {
            "id": rng.next_range(10_000_000),
            "type": etype,
            "actor": rng.next_string(15),
            "repo": repo,
            "payload": {
                "action": "created",
                "number": rng.next_range(10_000),
                "title": f"Fix {rng.next_string(12)} in {repo}",
                "body": rng.next_string(body_len),
                "labels": [rng.next_string(8) for _ in range(rng.next_range(4))],
            },
            "public": True,
            "created_at": f"2026-02-{1 + rng.next_range(28):02d}T"
                          f"{rng.next_range(24):02d}:{rng.next_range(60):02d}:"
                          f"{rng.next_range(60):02d}Z",
        }
        events.append(evt)
    return {"type": "mixed_api_events", "count": n, "events": events}


def gen_numeric_heavy(n: int) -> dict:
    rng = Rng(SEEDS["numeric_heavy"])
    records = []
    for _ in range(n):
        records.append({
            "i8": rng.next_range(256) - 128,
            "i16": rng.next_range(65536) - 32768,
            "i32": rng.next_range(0xFFFF_FFFF) - 0x7FFF_FFFF,
            "u64": rng.next_u64(),
            "f32": rng.next_range(1_000_000) / 1000.0,
            "f64": rng.next_u64() / (2**64),
            "flag": rng.next_bool(),
        })
    return {"type": "numeric_heavy", "count": n, "records": records}


# ── Dataset registry ────────────────────────────────────────────────

def generate_datasets(mode: str) -> list[tuple[str, dict]]:
    """Generate all datasets; ci mode uses smaller sizes."""
    if mode == "ci":
        return [
            ("small_objects", gen_small_objects(1_000)),
            ("string_heavy", gen_string_heavy(500)),
            ("nested_deep", gen_nested(10)),
            ("binary_blobs", gen_binary_blobs(10)),
            ("mixed_api_events", gen_mixed_api_events(200)),
            ("numeric_heavy", gen_numeric_heavy(2_000)),
        ]
    else:
        return [
            ("small_objects", gen_small_objects(100_000)),
            ("string_heavy", gen_string_heavy(10_000)),
            ("nested_deep", gen_nested(10)),
            ("binary_blobs", gen_binary_blobs(100)),
            ("mixed_api_events", gen_mixed_api_events(5_000)),
            ("numeric_heavy", gen_numeric_heavy(50_000)),
        ]


# ── Measurement ─────────────────────────────────────────────────────

def measure(func, iterations: int, warmup: int = 3) -> list[float]:
    """Time func() over iterations, return list of durations in nanoseconds."""
    # Warmup
    for _ in range(warmup):
        func()

    gc.disable()
    try:
        durations = []
        for _ in range(iterations):
            start = time.perf_counter_ns()
            func()
            end = time.perf_counter_ns()
            durations.append(float(end - start))
        return durations
    finally:
        gc.enable()


def compute_stats(durations: list[float]) -> dict:
    durations_sorted = sorted(durations)
    n = len(durations_sorted)
    median = durations_sorted[n // 2]
    p95 = durations_sorted[int(n * 0.95)] if n >= 20 else durations_sorted[-1]
    mean = statistics.mean(durations_sorted)
    stdev = statistics.stdev(durations_sorted) if n > 1 else 0.0
    cv = (stdev / mean * 100) if mean > 0 else 0.0
    return {
        "median_ns": median,
        "p95_ns": p95,
        "mean_ns": mean,
        "stddev_ns": stdev,
        "cv_pct": round(cv, 2),
        "count": n,
    }


# ── Benchmark runner ────────────────────────────────────────────────

def run_benchmarks(datasets: list[tuple[str, dict]], iterations: int) -> list[dict]:
    """Run encode/decode benchmarks for crous and json across all datasets."""
    results = []

    # Try importing msgpack
    try:
        import msgpack  # noqa: F401
        has_msgpack = True
    except ImportError:
        has_msgpack = False

    formats = [
        ("crous", crous.encode, crous.decode),
        ("json",
         lambda d: json.dumps(d).encode("utf-8"),
         lambda b: json.loads(b.decode("utf-8"))),
    ]

    if has_msgpack:
        import msgpack as mp
        formats.append((
            "msgpack",
            lambda d: mp.packb(d, use_bin_type=True),
            lambda b: mp.unpackb(b, raw=False),
        ))

    for ds_name, ds_data in datasets:
        print(f"  ▸ {ds_name} ({iterations} iterations)")
        for fmt_name, encode_fn, decode_fn in formats:
            # Encode
            encoded = encode_fn(ds_data)
            size = len(encoded)
            enc_durations = measure(lambda: encode_fn(ds_data), iterations)
            enc_stats = compute_stats(enc_durations)
            throughput = size / (enc_stats["median_ns"] / 1e9) / 1e6 if enc_stats["median_ns"] > 0 else 0

            results.append({
                "format": fmt_name,
                "dataset": ds_name,
                "operation": "encode",
                "serialized_size": size,
                "throughput_mbps": round(throughput, 1),
                **enc_stats,
            })

            # Decode
            dec_durations = measure(lambda: decode_fn(encoded), iterations)
            dec_stats = compute_stats(dec_durations)
            dec_throughput = size / (dec_stats["median_ns"] / 1e9) / 1e6 if dec_stats["median_ns"] > 0 else 0

            results.append({
                "format": fmt_name,
                "dataset": ds_name,
                "operation": "decode",
                "serialized_size": size,
                "throughput_mbps": round(dec_throughput, 1),
                **dec_stats,
            })

            med_us = enc_stats["median_ns"] / 1000
            dec_med_us = dec_stats["median_ns"] / 1000
            print(f"    {fmt_name:12s} encode={med_us:>8.1f}µs  decode={dec_med_us:>8.1f}µs  "
                  f"cv={enc_stats['cv_pct']:.1f}%/{dec_stats['cv_pct']:.1f}%  "
                  f"{throughput:>8.1f}/{dec_throughput:>8.1f} MB/s  {size}B")

    return results


# ── Regression detection ────────────────────────────────────────────

THRESHOLDS = {
    "max_throughput_decrease": 0.05,
    "max_size_increase": 0.05,
    "max_p95_increase": 0.10,
}


def detect_regressions(baseline: list[dict], current: list[dict]) -> list[dict]:
    """Compare current results against baseline, return list of regressions."""
    base_map = {}
    for m in baseline:
        key = (m["format"], m["dataset"], m["operation"])
        base_map[key] = m

    regressions = []
    for m in current:
        key = (m["format"], m["dataset"], m["operation"])
        base = base_map.get(key)
        if not base:
            continue

        label = f"{m['format']}/{m['dataset']}"

        # Median time increase
        if base["median_ns"] > 0:
            time_change = (m["median_ns"] - base["median_ns"]) / base["median_ns"]
            if time_change > THRESHOLDS["max_throughput_decrease"]:
                regressions.append({
                    "label": label,
                    "metric": f"{m['operation']}_median_ns",
                    "change_pct": round(time_change * 100, 1),
                    "threshold_pct": round(THRESHOLDS["max_throughput_decrease"] * 100, 1),
                    "severity": "FAIL" if time_change > 0.10 else "WARN",
                })

        # p95 increase
        if base["p95_ns"] > 0:
            p95_change = (m["p95_ns"] - base["p95_ns"]) / base["p95_ns"]
            if p95_change > THRESHOLDS["max_p95_increase"]:
                regressions.append({
                    "label": label,
                    "metric": f"{m['operation']}_p95_ns",
                    "change_pct": round(p95_change * 100, 1),
                    "threshold_pct": round(THRESHOLDS["max_p95_increase"] * 100, 1),
                    "severity": "FAIL",
                })

        # Size increase
        if base["serialized_size"] > 0:
            size_change = (m["serialized_size"] - base["serialized_size"]) / base["serialized_size"]
            if size_change > THRESHOLDS["max_size_increase"]:
                regressions.append({
                    "label": label,
                    "metric": "serialized_size",
                    "change_pct": round(size_change * 100, 1),
                    "threshold_pct": round(THRESHOLDS["max_size_increase"] * 100, 1),
                    "severity": "FAIL",
                })

    return regressions


# ── Report generation ───────────────────────────────────────────────

def write_report(output_dir: Path, results: list[dict], regressions: list[dict],
                 mode: str, version: str):
    """Write JSON results and markdown report."""
    output_dir.mkdir(parents=True, exist_ok=True)

    # System info
    sys_info = {
        "os": platform.system().lower(),
        "arch": platform.machine(),
        "python": platform.python_version(),
        "cpu": platform.processor() or "unknown",
    }

    report = {
        "version": version,
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
        "mode": mode,
        "system": sys_info,
        "measurements": results,
    }

    # raw.json
    with open(output_dir / "python_raw.json", "w") as f:
        json.dump(report, f, indent=2)

    # Markdown summary
    md_lines = [
        "# Crous Python Benchmark Report\n",
        f"**Version:** `{version}`",
        f"**Mode:** {mode}",
        f"**Python:** {sys_info['python']}",
        f"**OS:** {sys_info['os']} {sys_info['arch']}",
        "",
    ]

    if regressions:
        failures = [r for r in regressions if r["severity"] == "FAIL"]
        warnings = [r for r in regressions if r["severity"] == "WARN"]
        md_lines.append(f"## ❌ {len(failures)} failure(s), {len(warnings)} warning(s)\n")
        md_lines.append("| Label | Metric | Change | Threshold | Severity |")
        md_lines.append("|-------|--------|--------|-----------|----------|")
        for r in regressions:
            md_lines.append(
                f"| {r['label']} | {r['metric']} | "
                f"+{r['change_pct']}% | {r['threshold_pct']}% | {r['severity']} |"
            )
        md_lines.append("")
    else:
        md_lines.append("## ✅ NO REGRESSIONS DETECTED\n")

    md_lines.append("## Performance Summary\n")
    md_lines.append("| Format | Dataset | Op | Median (µs) | p95 (µs) | CV% | MB/s | Size |")
    md_lines.append("|--------|---------|-----|-------------|----------|-----|------|------|")
    for m in results:
        med_us = m["median_ns"] / 1000
        p95_us = m["p95_ns"] / 1000
        size_kb = m["serialized_size"] / 1024
        md_lines.append(
            f"| {m['format']} | {m['dataset']} | {m['operation']} | "
            f"{med_us:.1f} | {p95_us:.1f} | {m['cv_pct']} | "
            f"{m['throughput_mbps']} | {size_kb:.1f} KB |"
        )

    md_lines.append(f"\n---\n*Generated by bench_crous.py on {time.strftime('%Y-%m-%d')}*\n")

    with open(output_dir / "python_report.md", "w") as f:
        f.write("\n".join(md_lines))

    print(f"  ✓ Wrote {output_dir / 'python_raw.json'}")
    print(f"  ✓ Wrote {output_dir / 'python_report.md'}")


# ── CLI ─────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="Crous Python Benchmark")
    parser.add_argument("--mode", choices=["ci", "full"], default="ci",
                        help="ci = fast (3 iterations), full = thorough (50 iterations)")
    parser.add_argument("--output", type=str, default="bench/results/python",
                        help="Output directory")
    parser.add_argument("--baseline", type=str, default=None,
                        help="Path to baseline JSON for regression detection")
    parser.add_argument("--save-baseline", action="store_true",
                        help="Save results as baseline")
    parser.add_argument("--version", type=str, default="dev",
                        help="Version tag for this run")
    args = parser.parse_args()

    iterations = 3 if args.mode == "ci" else 50
    output_dir = Path(args.output)

    print("╔══════════════════════════════════════════════════════════╗")
    print(f"║  Crous Python Benchmark — {args.mode} mode" + " " * (30 - len(args.mode)) + "║")
    print("╚══════════════════════════════════════════════════════════╝")
    print(f"  Python:     {sys.version.split()[0]}")
    print(f"  Iterations: {iterations}")
    print(f"  Output:     {output_dir}")
    print()

    # Generate datasets
    print("▸ Generating datasets…")
    datasets = generate_datasets(args.mode)
    for name, data in datasets:
        h = hashlib.sha256(json.dumps(data, sort_keys=True).encode()).hexdigest()[:16]
        print(f"  • {name} — sha256:{h}…")
    print()

    # Run
    print(f"▸ Running benchmarks ({iterations} iterations)…")
    results = run_benchmarks(datasets, iterations)
    print()

    # Regression check
    regressions = []
    if args.baseline:
        print(f"▸ Comparing against baseline: {args.baseline}")
        with open(args.baseline) as f:
            baseline_data = json.load(f)
        regressions = detect_regressions(baseline_data["measurements"], results)
        if regressions:
            failures = [r for r in regressions if r["severity"] == "FAIL"]
            print(f"  ❌ {len(failures)} failure(s), {len(regressions) - len(failures)} warning(s)")
            for r in regressions:
                print(f"    {r['severity']}: {r['label']} — {r['metric']} "
                      f"+{r['change_pct']}% (threshold: {r['threshold_pct']}%)")
        else:
            print("  ✅ No regressions detected.")
        print()

    # Write reports
    print(f"▸ Writing results to {output_dir}…")
    write_report(output_dir, results, regressions, args.mode, args.version)

    # Save baseline
    if args.save_baseline:
        baseline_path = output_dir / "python_baseline.json"
        report = {
            "version": args.version,
            "timestamp": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
            "mode": args.mode,
            "measurements": results,
        }
        with open(baseline_path, "w") as f:
            json.dump(report, f, indent=2)
        print(f"  ✓ Saved baseline to {baseline_path}")

    print("▸ Done.")

    # Exit with failure if regressions
    if any(r["severity"] == "FAIL" for r in regressions):
        sys.exit(1)


if __name__ == "__main__":
    main()
