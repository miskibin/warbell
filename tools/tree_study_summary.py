"""Summarize real frame/GPU records; never substitute screenshots for timings."""
import json
from pathlib import Path
import re
import statistics

ROOT = Path(__file__).resolve().parents[1]
BASE = ROOT / "target/tree-study"

def summarize():
    runs = []
    for path in sorted(BASE.glob("*/run.json")):
        row = json.loads(path.read_text(encoding="utf-8"))
        if "performance" not in row:
            continue
        log = "\n".join(p.read_text(encoding="utf-8") for p in path.parent.glob("*.log"))
        log = re.sub(r"\x1b\[[0-9;]*m", "", log)
        ready = re.search(r"PERF_READY t=([\d.]+)", log)
        cutoff = max(30, float(ready[1]) + 15) if ready else 30
        elapsed = -1
        samples = []
        rss = []
        passes = {}
        for line in log.splitlines():
            perf = re.search(r"PERF t=\s*([\d.]+).*?rss=\s*([\d.]+)GiB", line)
            if perf:
                elapsed = float(perf[1])
                if elapsed >= cutoff:
                    rss.append(float(perf[2]))
            gpu = re.search(r"GPU Σ=([\d.]+)ms\s+(.*)", line)
            if gpu and elapsed >= cutoff:
                samples.append(float(gpu[1]))
                for name, val in re.findall(r"([^=]+)=([\d.]+)", gpu[2]):
                    passes.setdefault(name.strip(), []).append(float(val))
        row["gpu_sum_ms"] = statistics.mean(samples) if samples else None
        row["gpu_sample_count"] = len(samples)
        row["gpu_pass_mean_ms"] = {k: statistics.mean(v) for k, v in passes.items()}
        row["reported_rss_gib"] = statistics.mean(rss) if rss else None
        runs.append(row)
    (BASE / "summary.json").write_text(json.dumps(runs, indent=2), encoding="utf-8")
    print("label | mean ms | p95 ms | p99 ms | GPU sum ms | RSS GiB")
    for r in runs:
        p = r["performance"]
        print(f'{r["label"]} | {p["mean_ms"]:.3f} | {p["p95_ms"]:.3f} | {p["p99_ms"]:.3f} | {r["gpu_sum_ms"]} | {r["reported_rss_gib"]}')

if __name__ == "__main__":
    summarize()
