#!/usr/bin/env python3
"""Synthetic regression tests: python3 -m unittest discover -s tools -p test_analyze_gameplay.py."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from analyze_gameplay import analyze, distribution, percentile, report


def frame(t, **overrides):
    row = dict(type="frame", frame=int(t * 60), t_s=t, frame_ms=16.0, app_ms=7.0,
               recorder_ms=0.1, app_state="Playing", modal=None, focused=True,
               world_ready=True, phase="Prep", hero=[0, 0])
    row.update(overrides)
    return row


def sample(t, total, calls, **overrides):
    row = frame(t, type="sample", cpu=[dict(name="GuardCombat", calls=calls, total_ms=total, max_ms=999)],
                render=[], settings={"shadows": False}, quality="Low", window={"width": 1280},
                counts={"animals": 30, "invaders": 5})
    row.update(overrides)
    return row


class AnalyzeTests(unittest.TestCase):
    def capture(self, rows, tail="", ended=True, **options):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "capture.jsonl"
            data = [dict(type="session", schema=1, version="test", platform="test")]
            data.extend(rows)
            if ended:
                data.append(dict(type="end", dropped_records=0, reason="finished"))
            path.write_text("\n".join(json.dumps(row) for row in data) + "\n" + tail)
            return analyze(path, **options)

    def test_percentiles_and_fps_use_frame_interval_distribution(self):
        self.assertEqual(percentile([10, 20, 30, 40], 50), 25)
        self.assertAlmostEqual(percentile([10, 20, 30, 40], 95), 38.5)
        self.assertEqual(percentile([10], 99), 10)
        self.assertIsNone(percentile([], 50))
        stats = distribution([10, 30, 50, 110])
        self.assertEqual(stats["average_fps"], 20)
        self.assertEqual(stats["percent_over"]["50"], 25)

    def test_filter_pause_focus_loading_modal_and_each_reentry_warmup(self):
        rows = [frame(0), frame(4), frame(5), frame(6, app_state="Paused"),
                frame(7), frame(11), frame(12), frame(13, focused=False),
                frame(14), frame(18), frame(19), frame(20, world_ready=False),
                frame(21), frame(25), frame(26), frame(27, modal="Inventory"),
                frame(28), frame(32), frame(33, modal="None")]
        result = self.capture(rows)
        self.assertEqual(result["selected_frames"], 5)
        self.assertEqual({f["t_s"] for f in result["worst_frames"]}, {5, 12, 19, 26, 33})
        self.assertEqual(result["selection"]["excluded_frames"]["inactive / unfocused / loading"], 4)

    def test_include_all_bypasses_warmup_and_state_filter(self):
        result = self.capture([frame(0, focused=False), frame(1, app_state="Paused")], include_all=True)
        self.assertEqual(result["selected_frames"], 2)

    def test_legacy_entity_capacity_is_not_reported_as_live_entities(self):
        result = self.capture([sample(0, 0, 0, counts={"entities": 16384}), frame(0)], warmup=0)
        self.assertNotIn("entities", result["count_ranges"])
        self.assertEqual(result["count_ranges"]["allocated_entity_indices"]["max"], 16384)
        self.assertTrue(any("true live total is unavailable" in w for w in result["warnings"]))

    def test_new_live_entity_semantics_preserves_distinct_counts(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "capture.jsonl"
            rows = [dict(type="session", schema=1, entity_count_semantics="live_archetype_rows"),
                    sample(0, 0, 0, counts={"entities":123,"allocated_entity_indices":16384}), frame(0)]
            path.write_text("\n".join(json.dumps(r) for r in rows))
            result = analyze(path, warmup=0)
            self.assertEqual(result["count_ranges"]["entities"]["max"], 123)
            self.assertEqual(result["count_ranges"]["allocated_entity_indices"]["max"], 16384)

    def test_cpu_uses_deltas_and_never_bridges_inactive_frames(self):
        rows = [frame(0), sample(0, 100, 100), frame(1), sample(1, 120, 110),
                frame(2, focused=False), frame(3), sample(3, 900, 900),
                frame(4), sample(4, 910, 905)]
        result = self.capture(rows, warmup=0)
        metric = result["cpu"][0]
        self.assertEqual(metric["total_ms"], 30)
        self.assertEqual(metric["calls"], 15)
        self.assertEqual(metric["mean_call_ms"], 2)
        self.assertEqual(result["cpu_selected_intervals"], 2)
        self.assertNotIn("max_ms", metric)

    def test_cpu_excludes_intervals_crossing_warmup(self):
        result = self.capture([frame(0), sample(0, 0, 0), frame(4), sample(4, 400, 400),
                               frame(5), sample(5, 500, 500), frame(6), sample(6, 510, 505)])
        self.assertEqual(result["cpu"][0]["total_ms"], 10)
        self.assertEqual(result["cpu"][0]["calls"], 5)

    def test_counter_reset_does_not_make_negative_time(self):
        result = self.capture([sample(0, 100, 100), sample(1, 10, 10)], warmup=0)
        self.assertEqual(result["cpu"], [])
        self.assertTrue(any("decreasing counters" in text for text in result["warnings"]))

    def test_render_staleness_gpu_unavailability_and_no_nested_sums(self):
        result = self.capture([sample(0, 0, 0, render=[
            dict(path="render/elapsed_gpu", value=18, age_ms=501),
            dict(path="render/elapsed_cpu", value=7, age_ms=500),
            dict(path="render/pass/elapsed_cpu", value=3, age_ms=20),
            dict(path="render/not_a_time", value=900, age_ms=1),
        ])], warmup=0)
        self.assertEqual(len(result["render"]), 2)
        self.assertEqual(result["stale_or_invalid_render_samples"], 1)
        self.assertIn("GPU timing unavailable", report(result))

    def test_malformed_crash_tail_retains_valid_frames(self):
        result = self.capture([frame(0), frame(1)], tail='{"type":"frame",', ended=False, warmup=0)
        self.assertEqual(result["selected_frames"], 2)
        self.assertTrue(result["incomplete"])
        self.assertTrue(any("crash tail" in text for text in result["warnings"]))
        self.assertIsNone(result["dropped_records"])

    def test_comparison_shows_settings_difference_and_scene_caution(self):
        first = self.capture([sample(0, 0, 0), frame(1)], warmup=0)
        second = self.capture([sample(0, 0, 0, quality="High"), frame(1, frame_ms=30)], warmup=0)
        text = report(first, second)
        self.assertIn("context sets: DIFFER", text)
        self.assertIn("not a controlled benchmark", text)
        self.assertIn("Scene population ranges", text)

    def test_missing_measurements_are_unavailable_and_json_safe(self):
        result = self.capture([frame(0, frame_ms=float("nan")), frame(1, app_ms=None)], warmup=0)
        self.assertEqual(result["selected_frames"], 1)
        self.assertIsNone(result["app_time"])
        json.dumps(result, allow_nan=False)

    def test_system_sidecar_sampling_exclusion_and_include_all(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp)
            capture = folder / "gameplay.jsonl"
            rows = [dict(type="session", schema=1, unix_start_s=1000)]
            rows += [frame(t) for t in range(0, 31)]
            rows += [dict(type="end", dropped_records=3)]
            capture.write_text("\n".join(map(json.dumps, rows)) + "\n")
            (folder / "launch.json").write_text(json.dumps({"started": "1970-01-01T00:16:40+00:00"}))
            sidecar = [dict(type="process", t_s=1, cpu_percent=250, rss_kib=2048),
                       dict(type="process", t_s=2, cpu_percent=300, rss_kib=4096),
                       dict(type="stack_sample_begin", t_s=10, path="stack.txt"),
                       dict(type="stack_sample_end", t_s=15, exit_code=0)]
            (folder / "system.jsonl").write_text("\n".join(map(json.dumps, sidecar)) + "\n")
            result = analyze(capture)
            # Initial 0..4 warmup; padding excludes 8..17; reentry 18..22 warms up again.
            self.assertEqual(result["selected_frames"], 11)
            self.assertEqual(result["system"]["cpu_percent"]["max"], 300)
            self.assertEqual(result["system"]["rss_mib"]["max"], 4)
            self.assertEqual(result["dropped_records"], 3)
            self.assertTrue(any("approximate" in warning for warning in result["warnings"]))
            self.assertEqual(analyze(capture, include_all=True)["selected_frames"], 31)

    def test_sample_before_matching_frame_keeps_contiguous_cpu_interval(self):
        result = self.capture([sample(0, 100, 10), frame(0), sample(1, 120, 12), frame(1)], warmup=0)
        self.assertEqual(result["cpu"][0]["total_ms"], 20)
        self.assertEqual(result["selected_frames"], 2)
        self.assertIsNotNone(result["contexts"][0]["context"])

    def test_cli_writes_markdown_and_json_with_comparison(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp)
            capture = folder / "capture.jsonl"
            capture.write_text("\n".join(map(json.dumps, [
                dict(type="session", schema=1), frame(0), frame(1),
                dict(type="end", dropped_records=0),
            ])) + "\n")
            markdown, summary = folder / "report.md", folder / "summary.json"
            outcome = subprocess.run([
                sys.executable, str(Path(__file__).with_name("analyze_gameplay.py")), str(capture),
                "--warmup", "0", "--compare", str(capture), "--output", str(markdown), "--json", str(summary),
            ], capture_output=True, text=True)
            self.assertEqual(outcome.returncode, 0, outcome.stderr)
            self.assertIn("## Comparison", markdown.read_text())
            data = json.loads(summary.read_text())
            self.assertEqual(data["selected_frames"], 2)
            self.assertEqual(data["comparison"]["selected_frames"], 2)


if __name__ == "__main__":
    unittest.main()
