import copy
import unittest
from verify_transparent_upload import verify_transparent_upload


def fixture():
    names = "begun_frames submitted_frames staged_frames queue_frames distance_frames empty_frames busy_frames oversize_frames overflow_frames staged_copies staged_bytes aligned_staged_bytes queue_writes queue_bytes remap_requests remap_successes remap_failures ignored_callbacks reused_frames abandoned_frames peak_pending_slots maximum_preflight_bytes maximum_frame_bytes".split()
    counts = dict.fromkeys(names, 0)
    counts.update(begun_frames=8, submitted_frames=8, staged_frames=4, queue_frames=4,
                  empty_frames=1, busy_frames=2, oversize_frames=1, staged_copies=6,
                  staged_bytes=192, aligned_staged_bytes=200, queue_writes=4,
                  queue_bytes=12*1024*1024, remap_requests=3, remap_successes=1,
                  remap_failures=1, reused_frames=1, peak_pending_slots=3,
                  maximum_preflight_bytes=12*1024*1024, maximum_frame_bytes=12*1024*1024)
    return dict(enabled=True, mode="staged", poisoned=False, frame_active=False,
                eligible_render_distance=128, slab_count=4, slab_bytes=8*1024*1024,
                fixed_source_payload_bytes=32*1024*1024, maximum_slabs_per_frame=1,
                slots=[dict(state="Ready",epoch=2),dict(state="Pending",epoch=1),
                       dict(state="Failed",epoch=1),dict(state="Ready",epoch=0)],
                mapped_ready_slots=2, pending_slots=1, failed_slots=1,
                unconsumed_callback_slots=1, counts=counts)


class TransparentUploadVerification(unittest.TestCase):
    def verify(self, report, distance=128):
        return verify_transparent_upload(dict(transparency_upload_override="staged", render_distance=distance), report, "fixture")

    def test_fixed_capacity_and_pending_callback_snapshot(self):
        self.assertEqual(self.verify(fixture()), 8)
        # The Queue fallback may exceed slab capacity; its byte count is not a
        # claim that staged copies exceeded their independently checked cap.
        self.assertGreater(fixture()["counts"]["maximum_frame_bytes"], fixture()["slab_bytes"])

    def test_request_and_ordinary_default_are_enforced(self):
        verify_transparent_upload({}, None, "legacy")
        verify_transparent_upload({}, dict(enabled=False, mode="queue"), "default")
        for report in [None, dict(enabled=False, mode="queue")]:
            with self.assertRaises(AssertionError): self.verify(report)
        with self.assertRaises(AssertionError): verify_transparent_upload({}, fixture(), "default changed")
        run = dict(arguments=["--benchmark-transparency-upload", "staged"], render_distance=128)
        self.assertEqual(verify_transparent_upload(run, fixture(), "argument"), 8)
        run["arguments"] = ["--benchmark-transparency-upload=staged"]
        self.assertEqual(verify_transparent_upload(run, fixture(), "equals argument"), 8)
        with self.assertRaises(AssertionError):
            verify_transparent_upload(run, dict(enabled=False, mode="queue"), "ignored equals argument")

    def test_poison_partial_submission_capacity_and_callback_mismatch_fail(self):
        for mutate in [lambda r:r.update(poisoned=True), lambda r:r.update(frame_active=True),
                       lambda r:r["counts"].update(abandoned_frames=1),
                       lambda r:r["counts"].update(submitted_frames=7),
                       lambda r:r["counts"].update(queue_frames=5),
                       lambda r:r["counts"].update(aligned_staged_bytes=33*1024*1024),
                       lambda r:r["counts"].update(remap_requests=4),
                       lambda r:r["counts"].update(reused_frames=2),
                       lambda r:r["slots"][0].update(epoch=20),
                       lambda r:r["slots"][0].update(state="Reserved"),
                       lambda r:r.update(unconsumed_callback_slots=2),
                       lambda r:r.update(fixed_source_payload_bytes=64*1024*1024)]:
            report=copy.deepcopy(fixture()); mutate(report)
            with self.assertRaises(AssertionError): self.verify(report)

    def test_wrong_distance_cannot_claim_staged_admission(self):
        with self.assertRaises(AssertionError): self.verify(fixture(), 16)


if __name__ == "__main__": unittest.main()
