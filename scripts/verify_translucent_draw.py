"""Validate ordered translucent MDI evidence; preparation is not GPU completion."""


def verify_translucent_draw(run, report, label):
    requested = run.get("transparency_draw_override") or None
    arguments = run.get("arguments", [])
    for index, argument in enumerate(arguments):
        if argument == "--benchmark-transparency-draw":
            assert index + 1 < len(arguments), (label, "missing draw mode")
            value = arguments[index + 1]
        elif argument.startswith("--benchmark-transparency-draw="):
            value = argument.partition("=")[2]
        else:
            continue
        assert requested is None or requested == value, (label, "draw mode disagreement")
        requested = value
    if requested is not None:
        assert requested in ("direct", "mdi"), label
        assert isinstance(report, dict) and report.get("mode") == requested, (label, "requested draw mode missing")
    if report is None:
        return 0  # Historical binaries predate this explicit experiment.
    if requested is None:
        assert report.get("mode") == "direct", (label, "ordinary default changed")
    if report.get("mode") == "direct":
        assert report.get("enabled") is False, label
        assert "argument_buffer_bytes" not in report, (label, "default allocated alpha arguments")
        return 0
    assert report.get("mode") == "mdi" and report.get("enabled") is True, label
    assert report["eligible_render_distance"] == 128, label
    assert report["max_records"] == 4096, label
    assert report["argument_buffer_bytes"] == 4096 * 20, label
    assert report["range_capacity"] == 4096, label
    assert isinstance(report["plan_reserved_bytes"], int) and not isinstance(report["plan_reserved_bytes"], bool) and report["plan_reserved_bytes"] > 0, label
    assert isinstance(report["last_plan_has_mdi"], bool), label
    counts, issued = report["counts"], report["issued"]
    for values in (counts, issued):
        assert all(isinstance(v, int) and not isinstance(v, bool) and v >= 0 for v in values.values()), label
    fallbacks = ("distance_fallback_frames", "arena_fallback_frames", "opaque_plan_fallback_frames", "record_limit_fallback_frames")
    assert counts["frames"] == counts["prepared_frames"] + sum(counts[k] for k in fallbacks), label
    assert counts["empty_frames"] <= counts["prepared_frames"], label
    assert counts["planned_records"] == counts["planned_direct_calls"] + counts["planned_multi_records"], label
    nonempty_plans = counts["prepared_frames"] - counts["empty_frames"]
    assert nonempty_plans <= counts["planned_records"] <= nonempty_plans * report["max_records"], label
    assert 2 * counts["planned_multi_calls"] <= counts["planned_multi_records"], label
    assert counts["argument_uploads"] <= nonempty_plans, label
    assert counts["argument_uploads"] <= counts["planned_multi_calls"], label
    assert counts["argument_bytes"] % 20 == 0, label
    assert counts["argument_bytes"] <= counts["argument_uploads"] * report["argument_buffer_bytes"], label
    assert counts["argument_bytes"] <= counts["planned_records"] * 20, label
    assert counts["argument_bytes"] >= counts["planned_multi_records"] * 20, label
    assert issued["logical_records"] == issued["direct_calls"] + issued["multi_records"], label
    assert 2 * issued["multi_calls"] <= issued["multi_records"], label
    assert issued["eyes"] + issued["cached_eyes"] <= counts["frames"] * 2, label
    for actual, planned in (("multi_calls", "planned_multi_calls"), ("multi_records", "planned_multi_records")):
        assert issued[actual] <= counts[planned] * 2, label
    if run.get("render_distance") is not None and run["render_distance"] != 128:
        assert counts["prepared_frames"] == counts["planned_records"] == issued["multi_records"] == 0, label
    # These counters include warmup/screenshot. They describe prepared plans and
    # recorded commands, NOT successful submissions, GPU execution, or the
    # measured frame window. Issued direct counts include fallback draws;
    # cached fallback eyes are counted separately from direct/MDI records.
    return issued["logical_records"]
