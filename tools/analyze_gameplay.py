#!/usr/bin/env python3
"""Analyze Warbell schema-1 gameplay JSONL captures using only the Python standard library.

Usage: python3 tools/analyze_gameplay.py capture.jsonl --output report.md --json summary.json
       python3 tools/analyze_gameplay.py after.jsonl --compare before.jsonl

Default selection is focused, ready Playing / no-modal gameplay, after five seconds of
continuous active state. --include-all bypasses both this filter and its warmup. Reads lines
incrementally; retains up to one million frame measurements for exact distribution statistics.
"""

import argparse
import collections
import datetime
import json
import math
from pathlib import Path
import statistics


MAX_FRAMES = 1_000_000


def number(value):
    return isinstance(value, (float, int)) and not isinstance(value, bool) and math.isfinite(value)


def percentile(values, pct):
    """Linear interpolation between adjacent ranks, including endpoints."""
    if not values:
        return None
    ordered = sorted(values)
    rank = (len(ordered) - 1) * pct / 100.0
    lo, hi = math.floor(rank), math.ceil(rank)
    return ordered[lo] + (ordered[hi] - ordered[lo]) * (rank - lo)


def distribution(values):
    if not values:
        return None
    mean = statistics.fmean(values)
    return {
        "count": len(values), "mean_ms": mean,
        "p50_ms": percentile(values, 50), "p95_ms": percentile(values, 95),
        "p99_ms": percentile(values, 99), "max_ms": max(values),
        "average_fps": 1000.0 / mean if mean > 0 else None,
        "percent_over": {str(limit): 100.0 * sum(v > limit for v in values) / len(values)
                         for limit in (33.3, 50, 100)},
    }


def active(row):
    return (row.get("app_state") == "Playing" and row.get("modal") in (None, "None")
            and row.get("focused") is True and row.get("world_ready") is True)


def context_of(row):
    return {key: row.get(key) for key in ("quality", "settings", "window")}


class Selection:
    def __init__(self, warmup, include_all):
        self.warmup, self.include_all = warmup, include_all
        self.start = None
        self.epoch = 0
        self.last_time = None

    def interrupt(self):
        self.start = None
        self.epoch += 1

    def observe(self, row):
        t = row.get("t_s")
        if not number(t):
            self.interrupt()
            return False, "invalid timestamp"
        if self.last_time is not None and t < self.last_time:
            self.interrupt()
        self.last_time = t
        if self.include_all:
            return True, None
        if not active(row):
            self.interrupt()
            return False, "inactive / unfocused / loading"
        if self.start is None:
            self.start = t
        if t - self.start < self.warmup:
            return False, "warmup"
        return True, None


def read_system(capture_path):
    """Read optional launcher sidecars; their process-level measurements cover all states."""
    folder = Path(capture_path).parent
    source = folder / "system.jsonl"
    result = {"available": source.exists(), "process_samples": 0, "cpu_percent": None,
              "rss_mib": None, "stack_intervals": [], "launch_unix_s": None,
              "clock_offset_s": None, "stack_padding_s": 2.0, "malformed_lines": 0}
    try:
        launch = json.loads((folder / "launch.json").read_text())
        stamp = datetime.datetime.fromisoformat(launch["started"])
        if stamp.tzinfo is not None:
            result["launch_unix_s"] = stamp.timestamp()
    except (OSError, ValueError, KeyError, TypeError):
        pass
    cpu, rss, pending = [], [], None
    if source.exists():
        with source.open(encoding="utf-8", errors="replace") as stream:
            for raw in stream:
                if not raw.strip():
                    continue
                try:
                    row = json.loads(raw)
                    if not isinstance(row, dict):
                        raise ValueError("not an object")
                except ValueError:
                    result["malformed_lines"] += 1
                    continue
                if row.get("type") == "process":
                    result["process_samples"] += 1
                    if number(row.get("cpu_percent")):
                        cpu.append(row["cpu_percent"])
                    if number(row.get("rss_kib")):
                        rss.append(row["rss_kib"] / 1024.0)
                elif row.get("type") == "stack_sample_begin" and number(row.get("t_s")):
                    pending = {"start_s": row["t_s"], "end_s": None, "path": row.get("path")}
                    result["stack_intervals"].append(pending)
                elif row.get("type") == "stack_sample_end" and pending is not None and number(row.get("t_s")):
                    pending["end_s"] = row["t_s"]
                    pending["exit_code"] = row.get("exit_code")
                    pending = None
    for key, values in (("cpu_percent", cpu), ("rss_mib", rss)):
        if values:
            result[key] = {"min": min(values), "max": max(values), "mean": statistics.fmean(values)}
    return result


def analyze(path, warmup=5.0, include_all=False):
    selection = Selection(warmup, include_all)
    system = read_system(path)
    frames, contexts, markers = [], {}, []
    phase_values, context_values = collections.defaultdict(list), collections.defaultdict(list)
    cpu = collections.defaultdict(lambda: {"calls": 0, "total_ms": 0.0, "intervals": 0})
    render = collections.defaultdict(list)
    counts = collections.defaultdict(list)
    excluded = collections.Counter()
    current_context = "unavailable"
    previous_sample = None
    malformed, last_valid_line, seen_frames = [], 0, 0
    session, end, stop = None, None, None
    cpu_intervals, cpu_seconds = 0, 0.0
    reset_intervals, stale_render, truncated = 0, 0, False
    first_t, last_t = None, None
    with Path(path).open(encoding="utf-8", errors="replace") as source:
        for line_no, raw in enumerate(source, 1):
            if not raw.strip():
                continue
            try:
                row = json.loads(raw)
                if not isinstance(row, dict):
                    raise ValueError("record must be an object")
            except (ValueError, json.JSONDecodeError):
                malformed.append(line_no)
                selection.interrupt()
                previous_sample = None
                continue
            last_valid_line = line_no
            kind = row.get("type")
            if kind == "session":
                if row.get("schema") != 1:
                    raise ValueError("Unsupported capture schema: %r" % row.get("schema"))
                if session is not None:
                    raise ValueError("Multiple sessions in one capture are not supported")
                session = row
                if number(row.get("unix_start_s")) and system["launch_unix_s"] is not None:
                    system["clock_offset_s"] = row["unix_start_s"] - system["launch_unix_s"]
            elif kind == "end":
                end = row
            elif kind == "stop":
                stop = row
            elif kind == "marker":
                markers.append({"t_s": row.get("t_s"), "label": row.get("label")})
            elif kind in ("frame", "sample"):
                eligible, reason = selection.observe(row)
                t = row.get("t_s")
                if not include_all and number(t) and system["clock_offset_s"] is not None:
                    sidecar_t = t + system["clock_offset_s"]
                    padding = system["stack_padding_s"]
                    if any(interval["start_s"] - padding <= sidecar_t
                           and (interval["end_s"] is None or sidecar_t <= interval["end_s"] + padding)
                           for interval in system["stack_intervals"]):
                        eligible, reason = False, "native stack sampling (approximate padded interval)"
                        selection.interrupt()
                if number(t):
                    first_t = t if first_t is None else min(t, first_t)
                    last_t = t if last_t is None else max(t, last_t)
                if kind == "frame":
                    seen_frames += 1
                    if seen_frames > MAX_FRAMES:
                        truncated = True
                        break
                    ms = row.get("frame_ms")
                    if not number(ms) or ms <= 0:
                        excluded["invalid frame interval"] += 1
                        selection.interrupt()
                        continue
                    if not eligible:
                        excluded[reason] += 1
                        continue
                    frame = {key: row.get(key) for key in
                             ("frame", "t_s", "frame_ms", "app_ms", "recorder_ms", "phase", "hero")}
                    frame["context"] = current_context
                    frames.append(frame)
                    phase_values[str(row.get("phase"))].append(ms)
                    context_values[current_context].append(ms)
                else:
                    context = context_of(row)
                    context_key = json.dumps(context, sort_keys=True, separators=(",", ":"))
                    current_context = context_key
                    if eligible:
                        contexts[context_key] = context
                        for name, value in (row.get("counts") or {}).items():
                            if name == "entities" and (session or {}).get("entity_count_semantics") != "live_archetype_rows":
                                name = "allocated_entity_indices"
                            if number(value):
                                counts[name].append(value)
                        for datum in row.get("render") or []:
                            age, value, name = datum.get("age_ms"), datum.get("value"), datum.get("path")
                            if not isinstance(name, str) or not any(s in name for s in ("elapsed_cpu", "elapsed_gpu")):
                                continue
                            if not number(age) or not 0 <= age <= 500 or not number(value) or value < 0:
                                stale_render += 1
                                continue
                            render[name].append(value)
                    # Adjacent samples, endpoints selected, and no intervening inactivity or
                    # malformed/invalid frame. Never subtract across a pause or warmup transition.
                    if (eligible and previous_sample is not None and previous_sample[1]
                            and previous_sample[2] == selection.epoch):
                        prev = previous_sample[0]
                        dt = t - prev["t_s"]
                        if dt > 0:
                            before = {entry.get("name"): entry for entry in prev.get("cpu", [])}
                            used = False
                            for entry in row.get("cpu", []):
                                name = entry.get("name")
                                old = before.get(name)
                                if not old:
                                    continue
                                values = [entry.get("calls"), entry.get("total_ms"), old.get("calls"), old.get("total_ms")]
                                if not all(number(v) for v in values):
                                    continue
                                calls, elapsed = values[0] - values[2], values[1] - values[3]
                                if calls < 0 or elapsed < 0:
                                    reset_intervals += 1
                                    continue
                                item = cpu[name]
                                item["calls"] += calls
                                item["total_ms"] += elapsed
                                item["intervals"] += 1
                                used = True
                            if used:
                                cpu_intervals += 1
                                cpu_seconds += dt
                    previous_sample = (row, eligible, selection.epoch)
    for item in cpu.values():
        item["mean_call_ms"] = item["total_ms"] / item["calls"] if item["calls"] else None
    malformed_tail = bool(malformed and malformed[-1] > last_valid_line)
    warnings = []
    if session is None:
        warnings.append("Session header missing; schema provenance unavailable.")
    elif session.get("entity_count_semantics") != "live_archetype_rows":
        warnings.append("This older capture recorded allocated entity indices, not live entities. That field has been relabeled; the true live total is unavailable.")
    if end is None:
        warnings.append("No end record: capture is incomplete or still being written.")
    if malformed:
        warnings.append("Skipped malformed JSON records at lines " + ", ".join(map(str, malformed[:20])) + ".")
    if malformed_tail:
        warnings.append("Malformed crash tail detected; preceding complete records were retained.")
    if truncated:
        warnings.append("One-million-frame analysis limit reached; report covers only the prefix.")
    dropped = end.get("dropped_records") if end else None
    if dropped:
        warnings.append(f"Recorder dropped {dropped} records; gaps may bias timing distributions and CPU intervals.")
    if reset_intervals:
        warnings.append(f"Skipped {reset_intervals} CPU counter intervals with decreasing counters.")
    if system["stack_intervals"]:
        if include_all:
            message = "Native stack sampling adds overhead; --include-all retains potentially affected frames."
        elif system["clock_offset_s"] is None:
            message = "Native stack sampling adds overhead. Clock alignment is unavailable; affected frames could not be excluded reliably."
        else:
            message = "Native stack sampling adds overhead. Approximate aligned intervals, padded by 2s on each side, are excluded and restart warmup. Clock alignment is approximate, not exact correlation."
        warnings.append(message)
    if system["malformed_lines"]:
        warnings.append(f"Sidecar has {system['malformed_lines']} malformed/incomplete rows; complete process records were retained.")
    return {
        "capture": str(Path(path)), "session": session, "end": end, "stop": stop,
        "selection": {"include_all": include_all, "warmup_s": warmup, "excluded_frames": dict(excluded)},
        "observed_span_s": (last_t - first_t) if first_t is not None else None,
        "selected_frames": len(frames), "seen_frames": min(seen_frames, MAX_FRAMES),
        "frame_intervals": distribution([f["frame_ms"] for f in frames]),
        "app_time": distribution([f["app_ms"] for f in frames if number(f["app_ms"]) and f["app_ms"] >= 0]),
        "recorder_time": distribution([f["recorder_ms"] for f in frames if number(f["recorder_ms"]) and f["recorder_ms"] >= 0]),
        "phases": {key: distribution(values) for key, values in phase_values.items()},
        "contexts": [{"id": key, "context": contexts.get(key), "frames": distribution(values)}
                     for key, values in context_values.items()],
        "sampled_contexts": list(contexts.values()),
        "count_ranges": {key: {"min": min(values), "max": max(values)} for key, values in counts.items()},
        "worst_frames": sorted(frames, key=lambda f: f["frame_ms"], reverse=True)[:12],
        "cpu": sorted((dict(name=name, **item) for name, item in cpu.items()), key=lambda x: x["total_ms"], reverse=True),
        "cpu_selected_intervals": cpu_intervals, "cpu_interval_span_s": cpu_seconds,
        "render": {name: distribution(values) for name, values in render.items()},
        "stale_or_invalid_render_samples": stale_render, "markers": markers,
        "incomplete": end is None or malformed_tail or truncated,
        "dropped_records": dropped, "warnings": warnings, "system": system,
    }


def fmt(value, digits=2):
    return "unavailable" if not number(value) else f"{value:.{digits}f}"


def safe(value):
    return str(value).replace("|", "\\|").replace("\n", " ")


def summary_line(stats):
    if stats is None:
        return "Unavailable (no selected measurements)."
    return (f"{stats['count']} measurements; mean {fmt(stats['mean_ms'])} ms; "
            f"p50 / p95 / p99 {fmt(stats['p50_ms'])} / {fmt(stats['p95_ms'])} / {fmt(stats['p99_ms'])} ms.")


def report(result, comparison=None):
    lines = ["# Warbell gameplay capture", "", f"Capture: `{safe(result['capture'])}`", ""]
    if result["stop"]:
        lines.append(f"Recorder stop reason: `{safe(result['stop'].get('reason'))}`.")
    lines.extend(f"- {message}" for message in result["warnings"])
    sel = result["selection"]
    lines += ["", "## Frame delivery", "",
              ("All valid frames included; state filter and warmup disabled." if sel["include_all"] else
               f"Focused, world-ready Playing / no-modal frames only, after {sel['warmup_s']:g}s of continuous active state."),
              f"Selected {result['selected_frames']} of {result['seen_frames']} frame rows.",
              summary_line(result["frame_intervals"])]
    stats = result["frame_intervals"]
    if stats:
        lines += [f"Average FPS: **{fmt(stats['average_fps'])}**, calculated as 1000 / mean frame interval.",
                  "Frames over 33.3 / 50 / 100 ms: " + " / ".join(fmt(stats["percent_over"][str(v)]) + "%" for v in (33.3, 50, 100)) + "."]
    lines += [f"Excluded frame rows: `{json.dumps(sel['excluded_frames'], sort_keys=True)}`.", "",
              "Measured app duration: " + summary_line(result["app_time"]),
              "Measured recorder duration: " + summary_line(result["recorder_time"]),
              "App duration and total frame interval are different measurements. Rendering may be pipelined; "
              "their difference is not a measured GPU cost or proof of a bottleneck.", "", "## Gameplay context", ""]
    for phase, values in result["phases"].items():
        lines.append(f"- Phase `{safe(phase)}`: {summary_line(values)}")
    for i, entry in enumerate(result["contexts"], 1):
        lines.append(f"- Settings context {i}: {summary_line(entry['frames'])} `{safe(json.dumps(entry['context'], sort_keys=True))}`")
    lines += ["Settings context is the latest preceding sample; exact change timing between samples is unknown.",
              "Observed selected-sample population ranges: `" + safe(json.dumps(result["count_ranges"], sort_keys=True)) + "`.",
              "", "## Selected CPU intervals", "",
              f"{result['cpu_selected_intervals']} contiguous selected sample intervals covering {fmt(result['cpu_interval_span_s'])}s.",
              "These are **inclusive** function timings derived from cumulative-counter deltas. Nested callers include callees; "
              "do not sum rows. Counter maxima are cumulative and cannot identify a selected interval's maximum, so they are omitted."]
    if result["cpu"]:
        lines += ["", "| Function | Calls | Total ms | Mean ms/call |", "|---|---:|---:|---:|"]
        lines.extend(f"| {safe(item['name'])} | {item['calls']} | {fmt(item['total_ms'])} | {fmt(item['mean_call_ms'], 4)} |" for item in result["cpu"])
    else:
        lines.append("CPU function timing unavailable for the selected intervals.")
    lines += ["", "## Render diagnostics", "", "Only raw elapsed_cpu / elapsed_gpu diagnostics aged 0–500 ms are included. "
              "These are sampled latest values, not one observation per rendered frame; do not sum nested passes."]
    if not any("elapsed_gpu" in name for name in result["render"]):
        lines.append("GPU timing unavailable; this does not mean zero GPU time.")
    if result["render"]:
        lines += ["", "| Diagnostic path | Samples | Mean ms | p95 ms |", "|---|---:|---:|---:|"]
        for name, values in sorted(result["render"].items()):
            lines.append(f"| {safe(name)} | {values['count']} | {fmt(values['mean_ms'])} | {fmt(values['p95_ms'])} |")
    lines += [f"Stale/invalid elapsed diagnostic samples omitted: {result['stale_or_invalid_render_samples']}.",
              "", "## Worst selected frames", "", "| Time s | Frame | Interval ms | App ms | Phase | Hero x,z |", "|---:|---:|---:|---:|---|---|"]
    for frame in result["worst_frames"]:
        lines.append(f"| {fmt(frame['t_s'])} | {frame['frame']} | {fmt(frame['frame_ms'])} | {fmt(frame['app_ms'])} | {safe(frame['phase'])} | {safe(frame['hero'])} |")
    if result["markers"]:
        lines += ["", "Capture markers:"]
        lines.extend(f"- {fmt(m['t_s'])}s: {safe(m['label'])}" for m in result["markers"])
    system = result["system"]
    lines += ["", "## Process sidecar", ""]
    if system["available"]:
        lines += [f"{system['process_samples']} process samples across all recorded states (not restricted to selected gameplay).",
                  "Process CPU percent min / mean / max: " + " / ".join(fmt((system["cpu_percent"] or {}).get(k)) for k in ("min", "mean", "max")) + ".",
                  "Resident memory MiB min / mean / max: " + " / ".join(fmt((system["rss_mib"] or {}).get(k)) for k in ("min", "mean", "max")) + ".",
                  "Process CPU percent can exceed 100% across cores; it is not GPU utilization or a precise per-frame measurement."]
        if system["stack_intervals"]:
            lines += ["Native stack sampling intervals on the launcher's clock:"]
            for interval in system["stack_intervals"]:
                lines.append(f"- {fmt(interval['start_s'])}s to {fmt(interval['end_s'])}s: `{safe(interval['path'])}`.")
            lines.append(f"Approximate capture-to-launch clock offset: {fmt(system['clock_offset_s'])}s. "
                         "An absent end timestamp is treated as still sampling through the capture tail.")
    else:
        lines.append("Process CPU/RSS sidecar unavailable.")
    if comparison is not None:
        lines += ["", "## Comparison", "", f"Reference capture: `{safe(comparison['capture'])}`.",
                  "Both distributions use the same selection options. This is observational, not a controlled benchmark; "
                  "camera, route, combat, population and settings may differ.", "",
                  "| Capture | Selected frames | Average FPS | p95 ms | p99 ms |", "|---|---:|---:|---:|---:|"]
        for label, capture in (("Current", result), ("Reference", comparison)):
            values = capture["frame_intervals"] or {}
            lines.append(f"| {label} | {capture['selected_frames']} | {fmt(values.get('average_fps'))} | {fmt(values.get('p95_ms'))} | {fmt(values.get('p99_ms'))} |")
        same = (sorted(json.dumps(c, sort_keys=True) for c in result["sampled_contexts"])
                == sorted(json.dumps(c, sort_keys=True) for c in comparison["sampled_contexts"]))
        lines += [f"Settings/window/quality context sets: {'match' if same else 'DIFFER'}.",
                  "Scene population ranges (current / reference): `" + safe(json.dumps(result["count_ranges"], sort_keys=True))
                  + "` / `" + safe(json.dumps(comparison["count_ranges"], sort_keys=True)) + "`.",
                  "Phase frame counts (current / reference): `" + safe(json.dumps({p: v['count'] for p, v in result["phases"].items()}))
                  + "` / `" + safe(json.dumps({p: v['count'] for p, v in comparison["phases"].items()})) + "`."]
        lines.extend("Reference warning: " + message for message in comparison["warnings"])
    return "\n".join(lines).strip() + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("capture")
    parser.add_argument("--output", type=Path, help="Markdown output path (default stdout)")
    parser.add_argument("--json", dest="json_output", type=Path, help="Machine-readable summary path")
    parser.add_argument("--warmup", type=float, default=5.0)
    parser.add_argument("--include-all", action="store_true")
    parser.add_argument("--compare", help="Reference capture analyzed with the same selection")
    args = parser.parse_args()
    if not math.isfinite(args.warmup) or args.warmup < 0:
        parser.error("--warmup must be finite and nonnegative")
    try:
        result = analyze(args.capture, args.warmup, args.include_all)
        comparison = analyze(args.compare, args.warmup, args.include_all) if args.compare else None
        markdown = report(result, comparison)
        if args.output:
            args.output.write_text(markdown, encoding="utf-8")
        else:
            print(markdown, end="")
        if args.json_output:
            payload = dict(result, comparison=comparison) if comparison else result
            args.json_output.write_text(json.dumps(payload, indent=2, allow_nan=False) + "\n", encoding="utf-8")
    except (OSError, ValueError) as error:
        parser.exit(1, f"error: {error}\n")


if __name__ == "__main__":
    main()
