"""Verify optional terminal GPU records against existing CPU present boundaries."""
import math
from itertools import accumulate


def integer(value):
    return isinstance(value, int) and not isinstance(value, bool) and value >= 0


def finite(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def verify_requested_gpu_capture(run, capture, label):
    requested = run.get("gpu_timestamps") is True or "--benchmark-gpu-timestamps" in run.get("arguments", [])
    if requested:
        assert isinstance(capture, dict) and capture.get("enabled") is True, (label, "requested GPU capture missing")


def verify_gpu_timestamps(frames, capture, intervals, label):
    if not capture or not capture.get("enabled", False):
        return 0
    assert capture["state"] == "complete" and capture["failure"] is None, label
    assert capture["diagnostic_not_baseline"] is True, label
    period = capture["timestamp_period_ns"]
    assert finite(period) and period > 0, (label, "period")
    rows = capture["rows"]
    assert isinstance(rows, list), label
    assert integer(capture["capacity_rows"]) and len(rows) <= capture["capacity_rows"], label
    assert capture["submitted"] == len(rows), label
    # Successful window runs reserve only after acquisition. An encoded failure
    # aborts that run, so canceled/late/unanchored rows are not a valid success.
    for name in ("attempted", "overflow_skipped", "canceled_reserved", "late_reserves", "unanchored_rows"):
        assert integer(capture[name]), (label, name)
    assert capture["canceled_reserved"] == capture["late_reserves"] == capture["unanchored_rows"] == 0, label
    assert capture["attempted"] == len(rows) + capture["overflow_skipped"], label
    origin = intervals["first_measured_present_application_ms"]
    assert finite(origin), label
    boundaries = [origin + seconds * 1000 for seconds in accumulate(frames, initial=0.0)]
    sample_indices, values = [], []
    first_anchors = invalid = zeros = 0
    previous_frame = None
    for index, row in enumerate(rows):
        assert integer(row["row"]) and row["row"] == index, (label, index)
        frame = row["renderer_frame_id"]
        assert integer(frame) and (previous_frame is None or frame > previous_frame), (label, "frame order")
        previous_frame = frame
        assert finite(row["application_seconds"]), label  # Context, never an endpoint.
        for name in ("surface_acquire", "encode", "finish_and_submit", "present"):
            value = row["cpu_ms"][name]
            assert finite(value) and value >= 0, (label, name)
        sample = row["sample_index"]
        end = row["end_application_ms"]
        assert finite(end), (label, "missing present stamp")
        if sample is None:
            assert index == 0 and row["begin_application_ms"] is None, (label, "first anchor")
            assert abs(end - origin) < 1e-5, (label, "first anchor time")
            first_anchors += 1
        else:
            assert integer(sample) and sample < len(frames), (label, "sample index")
            begin = row["begin_application_ms"]
            assert finite(begin) and abs(begin - boundaries[sample]) < 1e-5, (label, "begin")
            assert abs(end - boundaries[sample + 1]) < 1e-5, (label, "end")
            sample_indices.append(sample)
        raw = row["raw_ticks"]
        assert isinstance(raw, list) and len(raw) == 2, (label, "raw ticks")
        assert all(integer(v) and v <= 2**64 - 1 for v in raw), label
        elapsed = row["gpu_encoder_ms"]
        if raw[1] < raw[0]:
            assert elapsed is None and row["invalid_reason"] == "reversed_or_unknown_wrap", label
            invalid += 1
        else:
            expected = (raw[1] - raw[0]) * period / 1_000_000
            assert finite(elapsed) and elapsed >= 0 and row["invalid_reason"] is None, label
            assert math.isclose(elapsed, expected, rel_tol=1e-12, abs_tol=1e-9), (label, "GPU elapsed")
            values.append(elapsed)
            zeros += elapsed == 0
    # Overflow can omit only a suffix. Failed acquisitions do not create a CPU
    # present interval and may leave gaps in renderer IDs, but never sample IDs.
    assert sample_indices == list(range(len(sample_indices))), (label, "sample prefix")
    assert len(sample_indices) + capture["overflow_skipped"] == len(frames), (label, "sample coverage")
    assert capture["attempted"] == len(frames) + first_anchors, label
    if capture["overflow_skipped"]:
        assert len(rows) == capture["capacity_rows"], label
    assert capture["valid_gpu_rows"] == len(values) and capture["invalid_gpu_rows"] == invalid, label
    assert capture["quantized_zero_rows"] == zeros, label
    assert math.isclose(capture["gpu_encoder_total_ms"], sum(values), rel_tol=1e-12, abs_tol=1e-9), label
    maximum = capture["gpu_encoder_max_ms"]
    assert maximum == max(values, default=None), label
    return len(sample_indices)
