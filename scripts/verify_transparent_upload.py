"""Validate the optional bounded index-upload trial without mixing timing windows."""


def verify_transparent_upload(run, report, label):
    requested = run.get("transparency_upload_override") or None
    arguments = run.get("arguments", [])
    for index, argument in enumerate(arguments):
        if argument == "--benchmark-transparency-upload":
            assert index + 1 < len(arguments), (label, "missing upload mode")
            value = arguments[index + 1]
        elif argument.startswith("--benchmark-transparency-upload="):
            value = argument.partition("=")[2]
        else:
            continue
        if requested is not None:
            assert requested == value, (label, "upload mode disagreement")
        requested = value
    if requested is not None:
        assert requested in ("queue", "staged"), label
        assert isinstance(report, dict) and report.get("mode") == requested, (label, "requested upload mode missing")
    if report is None:
        return 0  # Older binaries have no staging implementation/report.
    if requested is None:
        assert report.get("mode") == "queue", (label, "ordinary default changed")
    if report.get("mode") == "queue":
        assert report.get("enabled") is False, label
        assert "fixed_source_payload_bytes" not in report, (label, "queue mode allocated staging")
        return 0
    assert report.get("mode") == "staged" and report.get("enabled") is True, label
    assert report["poisoned"] is False and report["frame_active"] is False, label
    assert report["eligible_render_distance"] == 128, label
    assert report["slab_count"] == 4 and report["slab_bytes"] == 8 * 1024 * 1024, label
    assert report["fixed_source_payload_bytes"] == 32 * 1024 * 1024, label
    assert report["maximum_slabs_per_frame"] == 1, label
    counts = report["counts"]
    assert all(isinstance(v, int) and not isinstance(v, bool) and v >= 0 for v in counts.values()), label
    assert counts["abandoned_frames"] == 0, (label, "unsubmitted frame")
    assert counts["begun_frames"] == counts["submitted_frames"], label
    assert counts["submitted_frames"] == counts["staged_frames"] + counts["queue_frames"], label
    reasons = ("distance_frames", "empty_frames", "busy_frames", "oversize_frames", "overflow_frames")
    assert counts["queue_frames"] == sum(counts[name] for name in reasons), label
    assert counts["staged_copies"] * 4 <= counts["staged_bytes"], label
    assert counts["staged_bytes"] <= counts["aligned_staged_bytes"] <= counts["staged_frames"] * report["slab_bytes"], label
    assert counts["queue_writes"] * 4 <= counts["queue_bytes"], label
    assert counts["staged_bytes"] % 4 == counts["queue_bytes"] % 4 == 0, label
    assert counts["remap_requests"] <= counts["staged_frames"], label
    assert counts["remap_requests"] <= counts["staged_copies"], label
    assert counts["reused_frames"] <= counts["staged_frames"], label  # Reservation reuse, not completed-copy proof.
    slots = report["slots"]
    assert len(slots) == 4, label
    assert all(s["state"] in ("Ready", "Pending", "Failed") for s in slots), (label, "reserved slot after submission")
    for slot in slots:
        assert isinstance(slot["epoch"], int) and not isinstance(slot["epoch"], bool) and 0 <= slot["epoch"] <= (2**64 - 1) // 4, label
        assert slot["state"] == "Ready" or slot["epoch"] > 0, label
    assert sum(s["epoch"] for s in slots) == counts["staged_frames"], (label, "reservation epochs")
    assert sum(max(s["epoch"] - 1, 0) for s in slots) == counts["reused_frames"], (label, "reservation reuse")
    ready = sum(s["state"] == "Ready" for s in slots)
    pending = sum(s["state"] == "Pending" for s in slots)
    failed = sum(s["state"] == "Failed" for s in slots)
    assert report["mapped_ready_slots"] == ready and report["pending_slots"] == pending and report["failed_slots"] == failed, label
    assert counts["remap_failures"] == failed, label
    assert counts["remap_requests"] == counts["remap_successes"] + failed + pending, label
    assert 0 <= report["unconsumed_callback_slots"] <= pending <= counts["peak_pending_slots"] <= 4, label
    assert counts["ignored_callbacks"] == 0, (label, "unexpected callback identity")
    if run.get("render_distance") is not None and run["render_distance"] != 128:
        assert counts["staged_frames"] == counts["staged_copies"] == counts["staged_bytes"] == 0, label
    # These totals include startup, warmup and the final screenshot. They must
    # not be equated to the measured present-interval or render-profile count.
    return counts["submitted_frames"]
