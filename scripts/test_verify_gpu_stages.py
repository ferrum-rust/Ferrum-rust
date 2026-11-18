import unittest

from verify_gpu_stages import expected_points, verify_gpu_stages, verify_requested_gpu_stages


def fixture():
    rows, whole = [], []
    valid = zeros = 0
    for index, (mode, eyes, postfx, gui) in enumerate([
        ("world", 1, False, True), ("world", 2, True, True),
        ("no_world", 0, False, True), ("no_world", 0, False, False),
    ]):
        identities = expected_points(mode, eyes, postfx, gui)
        # Deliberately use near-u64MAX ticks to catch float-before-subtraction.
        origin = 2**64 - 1000 + index * 200
        points = [dict(query_index=q, label=name, eye=eye, raw_ticks=origin + q * 7)
                  for q, (name, eye) in enumerate(identities)]
        intervals = [dict(**{"from": q, "to": q + 1}, gpu_ms=0.0000035, invalid_reason=None)
                     for q in range(len(points) - 1)]
        valid += len(intervals)
        rows.append(dict(row=index, mode=mode, eyes=eyes, postfx=postfx, gui=gui,
                         used_query_count=len(points), points=points, intervals=intervals))
        whole.append(dict(row=index, raw_ticks=[points[0]["raw_ticks"], points[-1]["raw_ticks"]],
                          gpu_encoder_ms=(len(points) - 1) * 0.0000035, invalid_reason=None))
    return dict(enabled=True, state="complete", failure=None, timestamp_period_ns=0.5,
                capacity_rows=4, query_count=20, resolve_bytes=256, readback_bytes=640,
                rows=whole, stages=dict(enabled=True, schema_version=1, capacity_rows=4,
                query_count=20, readback_stride_bytes=160, readback_bytes=640, submitted=4,
                storage_shared_with_whole_frame=True,
                valid_intervals=valid, invalid_intervals=0, quantized_zero_intervals=zeros, rows=rows))


class GpuStageVerification(unittest.TestCase):
    def test_mono_stereo_menu_missing_branches_and_precise_integer_deltas(self):
        self.assertEqual(verify_gpu_stages(fixture(), "valid"), 4)

    def test_schema_identity_stale_rows_and_fabricated_durations_are_rejected(self):
        changes = [
            lambda c: c["stages"]["rows"][0]["points"][1].update(eye=1),
            lambda c: c["stages"]["rows"][0]["points"][2].update(query_index=9),
            lambda c: c["stages"]["rows"][0]["points"][3].update(label="far_end"),
            lambda c: c["stages"]["rows"][1].update(eyes=1),
            lambda c: c["stages"]["rows"][2].update(eyes=1),
            lambda c: c["stages"]["rows"][3].update(gui=True),
            lambda c: c["stages"]["rows"][0].update(used_query_count=20),
            lambda c: c["stages"]["rows"][0]["intervals"][0].update(gpu_ms=0.),
            lambda c: c["stages"]["rows"][0]["intervals"][0].update(to=2),
            lambda c: c["stages"]["rows"].pop(),
            lambda c: c.update(readback_bytes=32),
            lambda c: c["stages"].update(valid_intervals=0),
            lambda c: c["stages"].update(schema_version=True),
            lambda c: c["stages"].update(invalid_intervals=False),
            lambda c: c["stages"].update(quantized_zero_intervals=False),
            lambda c: c["stages"].update(capacity_rows=4.0),
            lambda c: c["stages"].update(enabled=1),
            lambda c: c["stages"].update(storage_shared_with_whole_frame=False),
        ]
        for change in changes:
            capture = fixture()
            change(capture)
            with self.assertRaises(AssertionError):
                verify_gpu_stages(capture, "corrupt")

    def test_reversed_stage_is_retained_even_when_whole_encoder_is_valid(self):
        capture = fixture()
        row = capture["stages"]["rows"][0]
        row["points"][2]["raw_ticks"] = row["points"][1]["raw_ticks"] - 1
        row["intervals"][1].update(gpu_ms=None, invalid_reason="reversed_or_unknown_wrap")
        row["intervals"][2]["gpu_ms"] = 15 * 0.5 / 1_000_000
        capture["stages"]["valid_intervals"] -= 1
        capture["stages"]["invalid_intervals"] += 1
        self.assertEqual(verify_gpu_stages(capture, "reordered"), 4)
        row["intervals"][1].update(gpu_ms=0., invalid_reason=None)
        with self.assertRaises(AssertionError):
            verify_gpu_stages(capture, "clamped")

    def test_quantized_zero_is_valid_and_not_an_absent_stage(self):
        capture = fixture()
        row = capture["stages"]["rows"][0]
        row["points"][2]["raw_ticks"] = row["points"][1]["raw_ticks"]
        row["intervals"][1]["gpu_ms"] = 0.
        row["intervals"][2]["gpu_ms"] = 14 * 0.5 / 1_000_000
        capture["stages"]["quantized_zero_intervals"] = 1
        self.assertEqual(verify_gpu_stages(capture, "quantized"), 4)

    def test_requested_diagnostic_cannot_silently_be_disabled(self):
        for run in ({"gpu_stages": True}, {"arguments": ["--benchmark-gpu-stages"]}):
            for capture in (None, {"enabled": False}, {"enabled": True, "stages": {"enabled": False}}):
                with self.assertRaises(AssertionError):
                    verify_requested_gpu_stages(run, capture, "missing")
            verify_requested_gpu_stages(run, fixture(), "present")
        verify_requested_gpu_stages({}, None, "old")
        self.assertEqual(verify_gpu_stages(None, "old"), 0)
        self.assertEqual(verify_gpu_stages({"stages": {"enabled": False}}, "off"), 0)


if __name__ == "__main__":
    unittest.main()
