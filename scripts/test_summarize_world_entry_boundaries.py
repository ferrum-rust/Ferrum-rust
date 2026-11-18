"""Synthetic summary checks. Run explicitly; no benchmark files or executables needed."""
import copy
import unittest

from summarize_world_entry import verify_intervals


class FrameBoundarySummaryTests(unittest.TestCase):
    def report(self):
        return {"first_measured_present_application_ms": 3000., "intervals": [{
            "sample_index": 0, "wall_ms": 12.,
            "begin_application_ms": 3000., "end_application_ms": 3012.,
            "frame_boundaries": {"first_frame_start_application_ms": 3002.,
                "last_frame_work_end_application_ms": 3008., "before_first_frame_ms": 2.,
                "first_to_last_work_span_ms": 6., "after_last_frame_work_ms": 4.},
        }]}

    def test_exact_and_failed_attempt_enclosing_span_preserve_anchor_count(self):
        self.assertEqual(verify_intervals([.012], self.report(), "single"), 1)
        report = self.report(); interval = report["intervals"][0]
        interval.update(wall_ms=20., end_application_ms=3020.,
                        frame_work={"calls": 2, "simulation_streaming_ms": 3., "gui_ms": 2., "render_ms": 5.})
        interval["frame_boundaries"].update(first_frame_start_application_ms=3001.,
            last_frame_work_end_application_ms=3016., before_first_frame_ms=1.,
            first_to_last_work_span_ms=15., after_last_frame_work_ms=4.)
        self.assertEqual(verify_intervals([.020], report, "failed attempts"), 1)
        # The inclusive middle15ms need not equal the summed10ms frame work.

    def test_legacy_missing_origin_and_checked_nulls_remain_unknown(self):
        for absent in (True, False):
            report = self.report()
            if absent:
                del report["intervals"][0]["frame_boundaries"]
            else:
                report["intervals"][0]["frame_boundaries"] = None
            self.assertEqual(verify_intervals([.012], report, "legacy"), 1)
            del report["first_measured_present_application_ms"]
            self.assertEqual(verify_intervals([.012], report, "legacy no anchor"), 0)
        report = self.report(); interval = report["intervals"][0]
        report["first_measured_present_application_ms"] = None
        interval["begin_application_ms"] = interval["end_application_ms"] = None
        interval["frame_boundaries"]["first_frame_start_application_ms"] = None
        interval["frame_boundaries"]["last_frame_work_end_application_ms"] = None
        self.assertEqual(verify_intervals([.012], report, "relative only"), 0)
        report = self.report(); bounds = report["intervals"][0]["frame_boundaries"]
        bounds.update(first_frame_start_application_ms=2999., last_frame_work_end_application_ms=3013.,
                      before_first_frame_ms=None, first_to_last_work_span_ms=14., after_last_frame_work_ms=None)
        before = copy.deepcopy(report)
        self.assertEqual(verify_intervals([.012], report, "checked invalid order"), 1)
        self.assertEqual(report, before, "validation must not normalize nulls or mutate evidence")

    def test_inconsistent_finite_partitions_and_endpoint_relations_fail(self):
        cases = [("before_first_frame_ms", 2.1), ("first_to_last_work_span_ms", 6.1),
                 ("after_last_frame_work_ms", 4.1), ("first_frame_start_application_ms", 3002.1),
                 ("last_frame_work_end_application_ms", 3008.1)]
        for key, value in cases:
            with self.subTest(key=key):
                report = self.report(); report["intervals"][0]["frame_boundaries"][key] = value
                with self.assertRaises(AssertionError):
                    verify_intervals([.012], report, key)
        for value in (float("nan"), float("inf"), -1., True, "2"):
            with self.subTest(value=value):
                report = self.report(); report["intervals"][0]["frame_boundaries"]["before_first_frame_ms"] = value
                with self.assertRaises(AssertionError):
                    verify_intervals([.012], report, "bad scalar")

    def test_prefix_sums_and_absolute_tolerance_are_independent_of_context(self):
        report = self.report(); interval = report["intervals"][0]
        interval.update(sample_index=2, wall_ms=17., begin_application_ms=3017.,
                        end_application_ms=3034., application_seconds=-100.)
        interval["frame_boundaries"].update(first_frame_start_application_ms=3018.,
            last_frame_work_end_application_ms=3032., before_first_frame_ms=1.,
            first_to_last_work_span_ms=14., after_last_frame_work_ms=2.)
        self.assertEqual(verify_intervals([.012, .005, .017], report, "prefix"), 1)
        interval["frame_boundaries"]["first_frame_start_application_ms"] += .000005
        self.assertEqual(verify_intervals([.012, .005, .017], report, "rounding"), 1)
        interval["frame_boundaries"]["first_frame_start_application_ms"] += .00002
        with self.assertRaises(AssertionError):
            verify_intervals([.012, .005, .017], report, "outside tolerance")


if __name__ == "__main__":
    unittest.main()
