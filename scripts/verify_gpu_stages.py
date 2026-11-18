"""Validate optional GPU stage records independently of renderer bookkeeping.

Call the existing whole-encoder verifier first for lifecycle and exact App anchors.
Timestamp reordering is retained as invalid evidence, never clamped to zero.
"""
import math

from verify_gpu_timestamps import finite, integer


EYE_POINTS = ("world_begin", "near_begin", "near_end", "far_end",
              "sky_dynamic_end", "alpha_end", "world_end")


def expected_points(mode, eyes, postfx, gui):
    assert mode in ("world", "no_world")
    assert integer(eyes) and eyes in ((1, 2) if mode == "world" else (0,))
    assert isinstance(postfx, bool) and isinstance(gui, bool)
    points = [("whole_begin", None)]
    for eye in range(eyes):
        points.extend((name, eye) for name in EYE_POINTS)
    if postfx:
        points.extend((("post_begin", None), ("post_end", None)))
    if gui:
        points.extend((("gui_begin", None), ("gui_end", None)))
    return points + [("whole_end", None)]


def verify_requested_gpu_stages(run, capture, label):
    requested = run.get("gpu_stages") is True or "--benchmark-gpu-stages" in run.get("arguments", [])
    if requested:
        assert isinstance(capture, dict) and capture.get("enabled") is True, (label, "missing base capture")
        assert capture.get("stages", {}).get("enabled") is True, (label, "requested stages missing")


def verify_gpu_stages(capture, label):
    if not capture or not capture.get("stages", {}).get("enabled", False):
        return 0
    assert capture.get("enabled") is True and capture["state"] == "complete", label
    assert capture["failure"] is None, label
    stage = capture["stages"]
    assert stage["enabled"] is True, label
    for key in ("schema_version", "capacity_rows", "query_count", "readback_stride_bytes",
                "readback_bytes", "submitted", "valid_intervals", "invalid_intervals", "quantized_zero_intervals"):
        assert integer(stage[key]), (label, key)
    for key in ("capacity_rows", "query_count", "resolve_bytes", "readback_bytes"):
        assert integer(capture[key]), (label, key)
    assert stage["schema_version"] == 1, label
    assert stage["query_count"] == capture["query_count"] == 20, label
    assert stage["readback_stride_bytes"] == 160, label
    assert stage["storage_shared_with_whole_frame"] is True, label
    assert stage["capacity_rows"] == capture["capacity_rows"], label
    assert stage["submitted"] <= stage["capacity_rows"] <= 131_072, label
    assert stage["readback_bytes"] == stage["capacity_rows"] * 160, label
    assert capture["readback_bytes"] == stage["readback_bytes"], (label, "single shared readback")
    assert capture["resolve_bytes"] == 256, label
    rows = stage["rows"]
    assert isinstance(rows, list) and len(rows) == len(capture["rows"]) == stage["submitted"], label
    period = capture["timestamp_period_ns"]
    assert finite(period) and period > 0, label
    valid = invalid = zeros = 0
    for index, (row, whole) in enumerate(zip(rows, capture["rows"])):
        assert integer(row["row"]) and row["row"] == whole["row"] == index, label
        expected = expected_points(row["mode"], row["eyes"], row["postfx"], row["gui"])
        points = row["points"]
        assert integer(row["used_query_count"]), label
        assert len(points) == row["used_query_count"] == len(expected) <= 20, label
        for query, (point, identity) in enumerate(zip(points, expected)):
            assert integer(point["query_index"]) and point["query_index"] == query, label
            assert (point["label"], point["eye"]) == identity, (label, index, query, "marker order")
            assert point["eye"] is None or integer(point["eye"]), label
            assert integer(point["raw_ticks"]) and point["raw_ticks"] <= 2**64 - 1, label
        assert [points[0]["raw_ticks"], points[-1]["raw_ticks"]] == whole["raw_ticks"], label
        intervals = row["intervals"]
        assert len(intervals) == len(points) - 1, label
        elapsed_sum = 0.0
        row_valid = True
        for query, interval in enumerate(intervals):
            assert integer(interval["from"]) and integer(interval["to"]), label
            assert (interval["from"], interval["to"]) == (query, query + 1), label
            first, last = points[query]["raw_ticks"], points[query + 1]["raw_ticks"]
            elapsed = interval["gpu_ms"]
            reason = interval["invalid_reason"]
            if last < first:
                assert elapsed is None and reason == "reversed_or_unknown_wrap", label
                invalid += 1
                row_valid = False
            else:
                expected_ms = (last - first) * period / 1_000_000
                if not math.isfinite(expected_ms):
                    assert elapsed is None and reason == "nonfinite_gpu_elapsed", label
                    invalid += 1
                    row_valid = False
                else:
                    assert finite(elapsed) and elapsed >= 0 and reason is None, label
                    assert math.isclose(elapsed, expected_ms, rel_tol=1e-12, abs_tol=1e-9), label
                    valid += 1
                    zeros += elapsed == 0
                    elapsed_sum += elapsed
        if row_valid:
            assert whole["invalid_reason"] is None, label
            assert math.isclose(elapsed_sum, whole["gpu_encoder_ms"], rel_tol=1e-12, abs_tol=1e-9), label
    assert stage["valid_intervals"] == valid, label
    assert stage["invalid_intervals"] == invalid, label
    assert stage["quantized_zero_intervals"] == zeros, label
    return len(rows)
