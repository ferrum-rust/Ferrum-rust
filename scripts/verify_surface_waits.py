"""Verify optional public surface-acquire wall spans; no GPU/submission identity.

Full counters cover every measured attempt. Only the largest32 observations are
retained: omitted durations cannot be reconstructed, so their bounds remain bounds.
"""
import math
from itertools import accumulate

EPS = 1e-5  # milliseconds, also the existing exact-present anchor tolerance
THRESHOLDS = [1, 2, 4, 8]
SCOPE_ORDER = ["core_acquire", "prior_submission_wait", "native_acquire"]


def integer(value):
    return isinstance(value, int) and not isinstance(value, bool) and 0 <= value <= 2**64 - 1


def finite(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value) and value >= 0


def close(a, b, absolute=EPS):
    return math.isclose(a, b, rel_tol=1e-10, abs_tol=absolute)


def verify_requested_surface_waits(run, capture, label):
    flag = "--benchmark-surface-waits"
    args = run.get("arguments", [])
    assert isinstance(args, list) and all(isinstance(a, str) for a in args), label
    cli = any(a == flag or a.startswith(flag + "=") for a in args)
    metadata = run.get("surface_waits")
    assert metadata is None or isinstance(metadata, bool), (label, "surface_waits metadata")
    if cli and metadata is not None:
        assert metadata is True, (label, "surface wait CLI/metadata disagreement")
    requested = metadata is True or cli
    if requested:
        assert isinstance(capture, dict) and capture.get("enabled") is True, (label, "requested surface capture missing")
    if isinstance(capture, dict) and capture.get("enabled") is True:
        assert requested, (label, "surface capture was not explicitly requested")


def verify_surface_waits(frames, capture, intervals, label):
    """Return the number of retained sampled rows with exact present anchors.

    This successful-window gate allows native acquire errors/retries, but never
    turns missing successful scope coverage or invalid observer state into zeros.
    """
    if capture is None:
        return 0
    assert isinstance(capture, dict) and isinstance(capture.get("enabled"), bool), label
    if not capture["enabled"]:
        if "feature_enabled" in capture:
            assert isinstance(capture["feature_enabled"], bool), label
        assert capture.get("diagnostic_not_baseline", False) is False, label
        return 0  # Ordinary feature-off and diagnostic-feature/capture-off are both valid.
    c = capture
    assert c["feature_enabled"] is True and c["diagnostic_not_baseline"] is True, label
    assert c["status"] == "complete", (label, "unfinished observer")
    assert integer(c["capture_epoch"]) and c["capture_epoch"] > 0, label
    assert (c["top_capacity"], c["span_capacity"], c["stack_capacity"]) == (32, 32, 8), label
    assert c["registered_scope_mask"] == c["registered_scope_mask_before_first_attempt"] == 7, (label, "registration not prewarmed")
    # Additive stage39b provenance: old schemas may omit both fields, but a
    # reported repair must have occurred exactly once before measurement. This
    # never substitutes for actual successful warmup/scope coverage below.
    rebuild_keys = ("warmup_interest_rebuilds", "warmup_interest_rebuilds_before_first_attempt")
    if any(k in c for k in rebuild_keys):
        assert all(k in c and integer(c[k]) and c[k] == 1 for k in rebuild_keys), (label, "warmup interest repair chronology")
    counts = ["attempts", "successful_acquires", "native_errors", "unwinds", "scope_complete_attempts",
              "successful_scope_complete_attempts", "invalid_attempts", "public_duration_count", "presented_attempts",
              "first_present_without_sample", "unstamped_successful_acquires", "unassociated_presents",
              "lifecycle_errors", "slot_overflow", "serial_exhaustion", "foreign_context_callbacks",
              "callback_contention", "reentry_attempts", "live_span_handles"]
    assert all(integer(c[k]) for k in counts), (label, "counter types")
    for k in ["unwinds", "invalid_attempts", "unstamped_successful_acquires", "lifecycle_errors", "slot_overflow",
              "serial_exhaustion", "foreign_context_callbacks", "callback_contention", "reentry_attempts", "live_span_handles"]:
        assert c[k] == 0, (label, k, c[k])
    attempts, success, errors = c["attempts"], c["successful_acquires"], c["native_errors"]
    assert attempts == success + errors and attempts > 0, label
    assert c["public_duration_count"] == attempts, (label, "missing original public duration")
    assert c["successful_scope_complete_attempts"] == success, (label, "missing successful scope coverage")
    complete = c["scope_complete_attempts"]
    assert success <= complete <= attempts, label
    assert c["scope_order"] == SCOPE_ORDER and c["thresholds_ms"] == THRESHOLDS, label
    scope_counts, totals = c["scope_completion_counts"], c["scope_ms_totals"]
    assert isinstance(scope_counts, list) and len(scope_counts) == 3 and all(integer(x) for x in scope_counts), label
    assert isinstance(totals, list) and len(totals) == 3 and all(finite(x) for x in totals), label
    # Every original core call exits on Result::Err too; children may stop early.
    assert scope_counts[0] == attempts and scope_counts[2] == complete, label
    assert complete <= scope_counts[1] <= attempts, label
    public_total = c["public_acquire_ms_total"]
    core_residual, public_residual = c["core_residual_ms_total"], c["public_residual_ms_total"]
    assert all(finite(x) for x in [public_total, core_residual, public_residual]), label
    assert totals[0] + EPS >= totals[1] + totals[2] and public_total + EPS >= totals[0], label
    assert core_residual <= totals[0] - totals[1] - totals[2] + EPS, label
    assert public_residual <= public_total - totals[0] + EPS, label
    if complete == attempts:
        assert close(core_residual, totals[0] - totals[1] - totals[2]), label
        assert close(public_residual, public_total - totals[0]), label

    warm = c["warmup"]
    assert isinstance(warm, dict) and warm["callback_timing_enabled"] is False, label
    for k in ["attempts", "successful_acquires", "native_errors", "unwinds", "successful_scope_complete_attempts", "invalid_attempts"]:
        assert integer(warm[k]), (label, "warmup", k)
    assert warm["attempts"] == warm["successful_acquires"] + warm["native_errors"] + warm["unwinds"], label
    assert warm["unwinds"] == warm["invalid_attempts"] == 0, label
    snapshot = c["warmup_successful_scope_complete_before_first_attempt"]
    assert integer(snapshot) and 0 < snapshot <= warm["successful_scope_complete_attempts"] <= warm["successful_acquires"], (label, "no actual successful warmup coverage")
    wc = warm["scope_completion_counts"]
    assert isinstance(wc, list) and len(wc) == 3 and all(integer(x) for x in wc), label
    assert warm["successful_scope_complete_attempts"] <= wc[2] <= wc[1] <= wc[0] <= warm["attempts"], label

    assert isinstance(frames, list) and all(finite(x) and x > 0 for x in frames), label
    origin = intervals["first_measured_present_application_ms"]
    assert finite(origin), label
    boundaries = [origin + x * 1000 for x in accumulate(frames, initial=0.)]
    first = c["first_present_without_sample"]
    assert first in (0, 1), label
    assert c["presented_attempts"] == success == len(frames) + first, (label, "full measured present coverage")
    assert c["unassociated_presents"] == 1 - first, (label, "warmup-crossing first anchor")

    top = c["top"]
    assert isinstance(top, list) and len(top) == min(attempts, 32), label
    observed_scopes = [0, 0, 0]
    observed_ms = [0., 0., 0.]
    observed_thresholds = [[0] * 4 for _ in range(4)]
    observed_public = observed_core_residual = observed_public_residual = 0.
    observed_success = observed_errors = observed_complete = sampled = first_rows = 0
    seen_seq, seen_frame, seen_sample = set(), set(), set()
    previous_rank = None
    for row in top:
        seq, frame = row["sequence"], row["renderer_frame_id"]
        assert integer(seq) and seq > 0 and seq not in seen_seq, (label, "attempt identity")
        assert integer(frame) and frame not in seen_frame, (label, "renderer identity")
        seen_seq.add(seq); seen_frame.add(frame)
        # Sequence includes warmup and is NOT a measured row/sample index.
        assert finite(row["application_seconds_context_label"]), label
        assert row["invalid"] is False and row["outcome"] in ("success", "native_error"), label
        sc = row["scope_counts"]
        assert isinstance(sc, list) and len(sc) == 3 and all(integer(x) and x <= 1 for x in sc), label
        assert sc[0] == 1 and sc[2] <= sc[1], label
        values = [row[k] for k in ("core_acquire_ms", "prior_submission_wait_ms", "native_acquire_ms")]
        public = row["public_acquire_ms"]
        assert all(finite(x) for x in values + [public]), label
        assert values[0] + 1e-9 >= values[1] + values[2] and public + 1e-9 >= values[0], label
        for count, value in zip(sc, values):
            assert count or value == 0, (label, "duration without completed scope")
        is_complete = sc == [1, 1, 1]
        assert row["scope_complete"] is is_complete, label
        if row["outcome"] == "success":
            assert is_complete and row["presented"] is True, label
            observed_success += 1
        else:
            assert row["presented"] is False, label
            observed_errors += 1
        observed_complete += is_complete
        if is_complete:
            assert finite(row["core_residual_ms"]) and close(row["core_residual_ms"], max(0., values[0] - values[1] - values[2]), 1e-9), label
            assert finite(row["public_residual_ms"]) and close(row["public_residual_ms"], max(0., public - values[0]), 1e-9), label
            observed_core_residual += row["core_residual_ms"]
            observed_public_residual += row["public_residual_ms"]
        else:
            assert row["core_residual_ms"] is None and row["public_residual_ms"] is None, label
        if previous_rank is not None:
            last_public, last_seq = previous_rank
            assert public < last_public or (public == last_public and seq > last_seq), (label, "stable top rank")
        previous_rank = public, seq
        sample, begin, end = row["sample_index"], row["present_begin_application_ms"], row["present_end_application_ms"]
        if not row["presented"]:
            assert sample is None and begin is None and end is None, label
        elif sample is None:
            assert first == 1 and begin is None and finite(end) and abs(end - origin) < EPS, label
            first_rows += 1
        else:
            assert integer(sample) and sample < len(frames) and sample not in seen_sample, label
            seen_sample.add(sample); sampled += 1
            assert finite(begin) and abs(begin - boundaries[sample]) < EPS, label
            assert finite(end) and abs(end - boundaries[sample + 1]) < EPS, label
            assert public <= frames[sample] * 1000 + EPS, (label, "acquire exceeds containing present interval")
        observed_public += public
        for k in range(3):
            observed_scopes[k] += sc[k]; observed_ms[k] += values[k]
            for j, threshold in enumerate(THRESHOLDS):
                observed_thresholds[k][j] += bool(sc[k] and values[k] >= threshold)
        for j, threshold in enumerate(THRESHOLDS):
            observed_thresholds[3][j] += public >= threshold
    assert first_rows <= first, label
    ordered = sorted(top, key=lambda r: r["sequence"])
    assert all(a["renderer_frame_id"] < b["renderer_frame_id"] for a, b in zip(ordered, ordered[1:])), label
    successful = [r for r in ordered if r["presented"]]
    sample_order = [-1 if r["sample_index"] is None else r["sample_index"] for r in successful]
    assert all(a < b for a, b in zip(sample_order, sample_order[1:])), label
    missing = attempts - len(top)
    cutoff = top[-1]["public_acquire_ms"]
    for measured, retained in [(success, observed_success), (errors, observed_errors), (complete, observed_complete)]:
        assert retained <= measured <= retained + missing, label
    # Exact if all rows fit; otherwise necessary bounded-subset checks only.
    for measured, retained, count in [(public_total, observed_public, attempts),
            *zip(totals, observed_ms, scope_counts),
            (core_residual, observed_core_residual, complete), (public_residual, observed_public_residual, complete)]:
        assert measured + EPS >= retained and measured <= retained + missing * cutoff + EPS, label
        if missing == 0:
            assert close(measured, retained), (label, "full retained sum")
    matrices = c["scope_at_least_counts"]
    assert isinstance(matrices, list) and len(matrices) == 3, label
    for values, known, count, known_count in zip(matrices + [c["public_at_least_counts"]], observed_thresholds,
                                                scope_counts + [attempts], observed_scopes + [len(top)]):
        assert isinstance(values, list) and len(values) == 4 and all(integer(x) for x in values), label
        assert all(a >= b for a, b in zip(values, values[1:])), label
        assert known_count <= count <= known_count + missing, label
        for j, value in enumerate(values):
            assert known[j] <= value <= known[j] + count - known_count, label
            assert value <= count, label
            if cutoff < THRESHOLDS[j] or missing == 0:
                assert value == known[j], (label, "threshold outside retained tail")
    return sampled
