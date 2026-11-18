import copy
import unittest
from verify_translucent_draw import verify_translucent_draw


def fixture():
    return dict(enabled=True, mode="mdi", eligible_render_distance=128, max_records=4096,
        argument_buffer_bytes=81920, range_capacity=4096, plan_reserved_bytes=262144,
        last_plan_has_mdi=True,
        counts=dict(frames=7, prepared_frames=4, empty_frames=1,
            distance_fallback_frames=1, arena_fallback_frames=1,
            opaque_plan_fallback_frames=0, record_limit_fallback_frames=1,
            planned_records=12, planned_direct_calls=4, planned_multi_calls=3,
            planned_multi_records=8, argument_uploads=2, argument_bytes=200),
        issued=dict(eyes=8, cached_eyes=1, logical_records=30, direct_calls=18,
                    multi_calls=5, multi_records=12))


class TranslucentDrawVerification(unittest.TestCase):
    def verify(self, report, distance=128):
        return verify_translucent_draw(dict(transparency_draw_override="mdi", render_distance=distance), report, "fixture")

    def test_stereo_fallback_and_empty_preparation_have_distinct_counts(self):
        self.assertEqual(self.verify(fixture()), 30)
        self.assertGreater(fixture()["issued"]["direct_calls"], 2 * fixture()["counts"]["planned_direct_calls"])

    def test_default_explicit_mode_and_both_cli_forms_are_checked(self):
        verify_translucent_draw({}, None, "legacy")
        verify_translucent_draw({}, dict(enabled=False, mode="direct"), "default")
        for report in (None, dict(enabled=False, mode="direct")):
            with self.assertRaises(AssertionError): self.verify(report)
        with self.assertRaises(AssertionError): verify_translucent_draw({}, fixture(), "default changed")
        for arguments in (["--benchmark-transparency-draw", "mdi"], ["--benchmark-transparency-draw=mdi"]):
            run = dict(arguments=arguments, render_distance=128)
            self.assertEqual(verify_translucent_draw(run, fixture(), "cli"), 30)
            run["transparency_draw_override"] = "direct"
            with self.assertRaises(AssertionError): verify_translucent_draw(run, fixture(), "disagreement")

    def test_capacity_plan_and_issued_conservation_fail_closed(self):
        mutations = [lambda r:r.update(argument_buffer_bytes=163840),
            lambda r:r.update(plan_reserved_bytes=True),
            lambda r:r["counts"].update(frames=8),
            lambda r:r["counts"].update(empty_frames=5),
            lambda r:r["counts"].update(empty_frames=4),
            lambda r:r["counts"].update(empty_frames=3),
            lambda r:r["counts"].update(planned_records=13),
            lambda r:r["counts"].update(argument_bytes=80),
            lambda r:r["counts"].update(argument_uploads=5),
            lambda r:r["issued"].update(logical_records=29),
            lambda r:r["issued"].update(eyes=14),
            lambda r:r["issued"].update(multi_calls=7),
            lambda r:r["issued"].update(multi_records=True)]
        for mutate in mutations:
            report = copy.deepcopy(fixture()); mutate(report)
            with self.assertRaises(AssertionError): self.verify(report)

    def test_wrong_distance_allows_direct_fallback_but_no_mdi(self):
        with self.assertRaises(AssertionError): self.verify(fixture(), 16)
        report = fixture()
        report["counts"] = dict.fromkeys(report["counts"], 0)
        report["counts"].update(frames=2, distance_fallback_frames=2)
        report["issued"].update(eyes=3, cached_eyes=1, logical_records=20,
                                direct_calls=20, multi_calls=0, multi_records=0)
        report["last_plan_has_mdi"] = False
        self.assertEqual(self.verify(report, 16), 20)


if __name__ == "__main__": unittest.main()
