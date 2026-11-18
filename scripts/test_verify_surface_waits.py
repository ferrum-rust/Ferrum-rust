import copy
import math
import unittest

from verify_surface_waits import verify_surface_waits, verify_requested_surface_waits


def fixture(count=2, first=True, native_error=True):
    frames = [0.02] * count
    intervals = {"first_measured_present_application_ms": 100.}
    rows = []
    sequence = 20  # Warmup shares the counter; this need not start at1.
    samples = ([None] if first else []) + list(range(count))
    for index, sample in enumerate(samples):
        # Includes zero and exact threshold boundaries, with nonuniform ranking.
        public = [0., 1., 2., 4., 8., 12., 0.5][(index * 3) % 7]
        core, wait, native = public * 0.75, public * 0.25, public * 0.25
        sequence += 1
        rows.append(dict(sequence=sequence, renderer_frame_id=sequence + 100,
            application_seconds_context_label=0.01, outcome="success", scope_counts=[1, 1, 1],
            scope_complete=True, invalid=False, public_acquire_ms=public, core_acquire_ms=core,
            prior_submission_wait_ms=wait, native_acquire_ms=native,
            core_residual_ms=core - wait - native, public_residual_ms=public - core,
            presented=True, sample_index=sample,
            present_begin_application_ms=None if sample is None else 100. + sample * 20.,
            present_end_application_ms=100. if sample is None else 100. + (sample + 1) * 20.))
        if native_error and index == 0:
            sequence += 1
            rows.append(dict(sequence=sequence, renderer_frame_id=sequence + 100,
                application_seconds_context_label=0.01, outcome="native_error", scope_counts=[1, 1, 0],
                scope_complete=False, invalid=False, public_acquire_ms=0.75, core_acquire_ms=0.5,
                prior_submission_wait_ms=0.25, native_acquire_ms=0., core_residual_ms=None,
                public_residual_ms=None, presented=False, sample_index=None,
                present_begin_application_ms=None, present_end_application_ms=None))
    success = count + int(first)
    scope_keys = ["core_acquire_ms", "prior_submission_wait_ms", "native_acquire_ms"]
    complete = [r for r in rows if r["scope_complete"]]
    capture = dict(enabled=True, feature_enabled=True, diagnostic_not_baseline=True,
        status="complete", capture_epoch=17, top_capacity=32, span_capacity=32, stack_capacity=8,
        registered_scope_mask=7, registered_scope_mask_before_first_attempt=7,
        warmup_successful_scope_complete_before_first_attempt=12,
        warmup=dict(attempts=20, successful_acquires=20, native_errors=0, unwinds=0,
            successful_scope_complete_attempts=20, invalid_attempts=0,
            scope_completion_counts=[20, 20, 20], callback_timing_enabled=False),
        attempts=len(rows), successful_acquires=success, native_errors=int(native_error), unwinds=0,
        scope_complete_attempts=len(complete), successful_scope_complete_attempts=success,
        invalid_attempts=0, public_duration_count=len(rows), public_acquire_ms_total=math.fsum(r["public_acquire_ms"] for r in rows),
        scope_completion_counts=[sum(r["scope_counts"][i] for r in rows) for i in range(3)],
        scope_ms_totals=[math.fsum(r[k] for r in rows) for k in scope_keys],
        scope_order=["core_acquire", "prior_submission_wait", "native_acquire"], thresholds_ms=[1, 2, 4, 8],
        scope_at_least_counts=[[sum(r["scope_counts"][i] > 0 and r[k] >= t for r in rows)
            for t in [1, 2, 4, 8]] for i, k in enumerate(scope_keys)],
        public_at_least_counts=[sum(r["public_acquire_ms"] >= t for r in rows) for t in [1, 2, 4, 8]],
        core_residual_ms_total=math.fsum(r["core_residual_ms"] for r in complete),
        public_residual_ms_total=math.fsum(r["public_residual_ms"] for r in complete),
        presented_attempts=success, first_present_without_sample=int(first), unstamped_successful_acquires=0,
        unassociated_presents=int(not first), lifecycle_errors=0, slot_overflow=0, serial_exhaustion=0,
        foreign_context_callbacks=0, callback_contention=0, reentry_attempts=0, live_span_handles=0,
        top=sorted(rows, key=lambda r: -r["public_acquire_ms"])[:32])
    return frames, capture, intervals


class SurfaceWaitVerification(unittest.TestCase):
    def verify(self, data):
        return verify_surface_waits(*data, "fixture")

    def reject(self, mutations, data=None):
        original = data or fixture()
        for change in mutations:
            with self.subTest(change=change):
                frames, capture, intervals = copy.deepcopy(original)
                change(capture)
                with self.assertRaises(AssertionError):
                    self.verify((frames, capture, intervals))

    def test_exact_full_records_include_native_error_zero_times_and_sparse_renderer_ids(self):
        data = fixture()
        self.assertEqual(self.verify(data), 2)
        self.assertGreater(min(r["sequence"] for r in data[1]["top"]), data[1]["attempts"])
        failure = next(r for r in data[1]["top"] if r["outcome"] == "native_error")
        self.assertIsNone(failure["core_residual_ms"])
        self.assertEqual(self.verify(fixture(first=False)), 2)

    def test_bounded_top32_does_not_pretend_to_be_a_full_sample_trace(self):
        data = fixture(count=95)
        verified = self.verify(data)
        self.assertLessEqual(verified, 32)
        self.assertLess(verified, len(data[0]))
        self.assertEqual(len(data[1]["top"]), 32)
        self.reject([
            lambda c: c["top"].pop(),
            lambda c: c.update(public_acquire_ms_total=1e9),
            lambda c: c.update(scope_ms_totals=[1e9, 0., 0.]),
            lambda c: c["public_at_least_counts"].__setitem__(3, c["attempts"]),
        ], data)

    def test_corrupt_identity_rank_and_exact_present_anchors_are_rejected(self):
        self.reject([
            lambda c: c["top"][0].update(sequence=c["top"][1]["sequence"]),
            lambda c: c["top"][0].update(renderer_frame_id=c["top"][1]["renderer_frame_id"]),
            lambda c: c["top"].reverse(),
            lambda c: c["top"][0].update(present_begin_application_ms=99.),
            lambda c: c["top"][0].update(present_end_application_ms=999.),
            lambda c: c["top"][0].update(sample_index=99),
            lambda c: c["top"][0].update(presented=False),
            lambda c: c["top"][0].update(application_seconds_context_label=float("nan")),
            lambda c: c["top"][0].update(core_residual_ms=c["top"][0]["core_residual_ms"] + 0.000001),
        ])
        data = fixture(count=95)
        def break_tie(c):
            self.assertEqual(c["top"][0]["public_acquire_ms"], c["top"][1]["public_acquire_ms"])
            c["top"][0], c["top"][1] = c["top"][1], c["top"][0]
        self.reject([break_tie], data)

    def test_full_window_missing_scope_or_counter_corruption_cannot_pass_as_zero(self):
        self.reject([
            lambda c: c.update(attempts=c["attempts"] + 1),
            lambda c: c.update(successful_scope_complete_attempts=0),
            lambda c: c.update(scope_complete_attempts=0),
            lambda c: c.update(public_duration_count=0),
            lambda c: c.update(scope_ms_totals=[0., 0., 0.]),
            lambda c: c.update(scope_completion_counts=[c["attempts"], 0, 0]),
            lambda c: c.update(public_acquire_ms_total=c["public_acquire_ms_total"] + 0.1),
            lambda c: c.update(core_residual_ms_total=0.),
            lambda c: c.update(public_residual_ms_total=0.),
            lambda c: c.update(public_at_least_counts=[99, 99, 99, 99]),
            lambda c: c.update(public_acquire_ms_total=float("inf")),
            lambda c: c.update(attempts=True),
            lambda c: c["top"][0].update(scope_complete=False),
            lambda c: c["top"][0].update(native_acquire_ms=-0.1),
            lambda c: c["top"][0].update(invalid=True),
        ])

    def test_actual_warmup_is_separate_from_registration_and_first_present_arm(self):
        repaired = fixture()
        repaired[1].update(warmup_interest_rebuilds=1, warmup_interest_rebuilds_before_first_attempt=1)
        self.assertEqual(self.verify(repaired), 2)
        self.assertEqual(self.verify(fixture()), 2)  # Older complete schema remains supported.
        for key in ("warmup_interest_rebuilds", "warmup_interest_rebuilds_before_first_attempt"):
            self.reject([lambda c, key=key: c.pop(key)], repaired)
            for bad in (None, False, True, 0, 2, 1.0):
                self.reject([lambda c, key=key, bad=bad: c.update({key: bad})], repaired)
        self.reject([lambda c: c.update(warmup_successful_scope_complete_before_first_attempt=0)], repaired)
        self.reject([
            lambda c: c.update(registered_scope_mask_before_first_attempt=0),
            lambda c: c.update(warmup_successful_scope_complete_before_first_attempt=0),
            lambda c: c.update(warmup_successful_scope_complete_before_first_attempt=None),
            lambda c: c.update(warmup_successful_scope_complete_before_first_attempt=21),
            lambda c: c["warmup"].update(callback_timing_enabled=True),
            lambda c: c["warmup"].update(scope_completion_counts=[20, 20, 0]),
            lambda c: c.update(first_present_without_sample=2),
            lambda c: c.update(unassociated_presents=1),
            lambda c: c.update(presented_attempts=c["presented_attempts"] - 1),
        ])
        self.reject([lambda c: c.update(unassociated_presents=0)], fixture(first=False))

    def test_incomplete_or_failed_observer_is_not_a_successful_capture(self):
        changes = [lambda c: c.update(status="incomplete"), lambda c: c.update(feature_enabled=False)]
        for key in ["unwinds", "invalid_attempts", "unstamped_successful_acquires", "lifecycle_errors", "slot_overflow",
                    "serial_exhaustion", "foreign_context_callbacks", "callback_contention", "reentry_attempts", "live_span_handles"]:
            changes.append(lambda c, key=key: c.update({key: 1}))
        self.reject(changes)
        def failure_cannot_present(c):
            row = next(r for r in c["top"] if r["outcome"] == "native_error")
            row.update(presented=True, sample_index=0, present_begin_application_ms=100., present_end_application_ms=120.)
        self.reject([failure_cannot_present])

    def test_legacy_feature_off_capture_off_and_explicit_request_binding(self):
        self.assertEqual(verify_surface_waits([], None, {}, "legacy"), 0)
        for feature in [False, True]:
            disabled = dict(enabled=False, feature_enabled=feature, diagnostic_not_baseline=False)
            self.assertEqual(verify_surface_waits([], disabled, {}, "off"), 0)
            verify_requested_surface_waits({}, disabled, "off")
        for run in [{"surface_waits": True}, {"arguments": ["--benchmark-surface-waits"]},
                    {"arguments": ["--benchmark-surface-waits=true"]}]:
            for missing in [None, {"enabled": False}]:
                with self.assertRaises(AssertionError):
                    verify_requested_surface_waits(run, missing, "missing")
            verify_requested_surface_waits(run, fixture()[1], "requested")
        for run in [{}, {"surface_waits": False, "arguments": ["--benchmark-surface-waits"]}]:
            with self.assertRaises(AssertionError):
                verify_requested_surface_waits(run, fixture()[1], "unrequested")


if __name__ == "__main__":
    unittest.main()
