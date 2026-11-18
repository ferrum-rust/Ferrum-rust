import copy
import unittest

from verify_gpu_timestamps import verify_gpu_timestamps, verify_requested_gpu_capture


def fixture():
    rows = []
    for row, sample, begin, end, raw, elapsed, reason in [
        (0, None, None, 100., [5, 5], 0., None),
        (1, 0, 100., 101., [7, 17], 0.00001, None),
        (2, 1, 101., 103., [20, 15], None, "reversed_or_unknown_wrap"),
    ]:
        rows.append(dict(row=row, renderer_frame_id=7 + row * 2, application_seconds=0.09,
                         sample_index=sample, begin_application_ms=begin, end_application_ms=end,
                         cpu_ms=dict(surface_acquire=0.1, encode=0.2, finish_and_submit=0.1, present=0.01),
                         raw_ticks=raw, gpu_encoder_ms=elapsed, invalid_reason=reason))
    return dict(enabled=True, state="complete", failure=None, diagnostic_not_baseline=True,
                timestamp_period_ns=1., capacity_rows=4, submitted=3, attempted=3,
                overflow_skipped=0, canceled_reserved=0, late_reserves=0, unanchored_rows=0,
                valid_gpu_rows=2, invalid_gpu_rows=1, quantized_zero_rows=1,
                gpu_encoder_total_ms=0.00001, gpu_encoder_max_ms=0.00001, rows=rows)


class GpuTimestampVerification(unittest.TestCase):
    def verify(self, capture):
        return verify_gpu_timestamps([0.001, 0.002], capture,
                                     {"first_measured_present_application_ms": 100.}, "fixture")

    def test_exact_cpu_anchors_and_independent_gpu_delta(self):
        self.assertEqual(self.verify(fixture()), 2)

    def test_uncaptured_first_arm_and_overflow_suffix(self):
        capture = fixture()
        capture["rows"].pop(0)
        for index, row in enumerate(capture["rows"]):
            row["row"] = index
        capture.update(submitted=2, attempted=2, valid_gpu_rows=1, quantized_zero_rows=0)
        self.assertEqual(self.verify(capture), 2)
        capture = fixture()
        capture["rows"].pop()
        capture.update(capacity_rows=2, submitted=2, overflow_skipped=1, invalid_gpu_rows=0)
        self.assertEqual(self.verify(capture), 1)

    def test_corrupt_association_and_false_success_are_rejected(self):
        original = fixture()
        for mutate in [
            lambda c: c["rows"][1].update(begin_application_ms=99.),
            lambda c: c["rows"][1].update(sample_index=1),
            lambda c: c["rows"][1].update(renderer_frame_id=7),
            lambda c: c["rows"][1].update(gpu_encoder_ms=10.),
            lambda c: c["rows"][2].update(gpu_encoder_ms=0.),
            lambda c: c.update(state="recording"),
            lambda c: c.update(unanchored_rows=1),
            lambda c: c.update(timestamp_period_ns=float("nan")),
        ]:
            capture = copy.deepcopy(original)
            mutate(capture)
            with self.assertRaises(AssertionError):
                self.verify(capture)

    def test_legacy_and_disabled_do_not_invent_gpu_data(self):
        self.assertEqual(verify_gpu_timestamps([], None, {}, "legacy"), 0)
        self.assertEqual(verify_gpu_timestamps([], {"enabled": False}, {}, "off"), 0)

    def test_explicit_request_cannot_pass_as_legacy_or_disabled(self):
        for run in [{"gpu_timestamps": True}, {"arguments": ["--benchmark-gpu-timestamps"]}]:
            for capture in [None, {"enabled": False}]:
                with self.assertRaises(AssertionError):
                    verify_requested_gpu_capture(run, capture, "missing")
            verify_requested_gpu_capture(run, fixture(), "requested")
        verify_requested_gpu_capture({}, None, "legacy")


if __name__ == "__main__":
    unittest.main()
