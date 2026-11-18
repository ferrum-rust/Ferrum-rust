"""Compact benchmark evidence; verify slow intervals against the recorded samples."""
import argparse
import json
from verify_gpu_timestamps import verify_gpu_timestamps, verify_requested_gpu_capture
from verify_gpu_stages import verify_gpu_stages, verify_requested_gpu_stages
from verify_surface_waits import verify_surface_waits, verify_requested_surface_waits
from verify_transparent_upload import verify_transparent_upload
from verify_translucent_draw import verify_translucent_draw
import math
from itertools import accumulate
from pathlib import Path


def read(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def verify_frame_boundaries(interval, label):
    """Validate available chronology only; null/missing values remain unknown."""
    frame = interval.get("frame_boundaries")
    if frame is None:
        return
    assert isinstance(frame, dict), (label, "frame_boundaries", frame)
    keys = ("first_frame_start_application_ms", "last_frame_work_end_application_ms",
            "before_first_frame_ms", "first_to_last_work_span_ms", "after_last_frame_work_ms")
    values = [frame.get(key) for key in keys]
    for key, value in zip(keys, values):
        if value is not None:
            assert isinstance(value, (int, float)) and not isinstance(value, bool), (label, key, value)
            assert math.isfinite(value) and value >= 0, (label, key, value)
    first, last, before, span, after = values

    def equal_if_available(actual, expected, relation):
        if actual is not None and expected is not None:
            assert math.isfinite(expected) and abs(actual - expected) < 1e-5, (
                label, "frame_boundaries", relation, actual, expected)

    def difference(end, start):
        return None if end is None or start is None else end - start

    # Checked-duration nulls can represent invalid ordering. Do not clamp them
    # to zero or infer that any of these spans is additional covered CPU work.
    if all(value is not None for value in (before, span, after)):
        equal_if_available(before + span + after, interval["wall_ms"], "chronology partitions wall")
    equal_if_available(before, difference(first, interval.get("begin_application_ms")), "before first frame")
    equal_if_available(span, difference(last, first), "first to last work")
    equal_if_available(after, difference(interval.get("end_application_ms"), last), "after last work")



def verify_intervals(frames, report, label):
    intervals = report.get("intervals", [])
    anchor = report.get("first_measured_present_application_ms")
    boundaries = None
    if anchor is not None:
        assert math.isfinite(anchor), label
        boundaries = [anchor + seconds * 1000 for seconds in accumulate(frames, initial=0.0)]
    for interval in intervals:
        index = interval["sample_index"]
        assert 0 <= index < len(frames), (label, index)
        actual = frames[index] * 1000
        assert abs(actual - interval["wall_ms"]) < 1e-6, (label, actual, interval)
        if boundaries is not None:
            for key, expected in (("begin_application_ms", boundaries[index]),
                                  ("end_application_ms", boundaries[index + 1])):
                value = interval.get(key)
                assert value is not None and abs(value - expected) < 1e-5, (label, key, expected, value)
        verify_frame_boundaries(interval, label)
    return len(intervals) if boundaries is not None else 0


def summarize(directory):
    entry = read(directory / "world-entry.json")
    run = read(directory / "run.json")
    bench = read(directory / "benchmark.json")
    frames = bench["frame_seconds"]
    interval_report = bench.get("slow_present_intervals", {})
    intervals = interval_report.get("intervals", [])
    verified_anchors = verify_intervals(frames, interval_report, directory)
    verify_requested_gpu_capture(run, bench.get("gpu_timestamps"), directory)
    verified_gpu_anchors = verify_gpu_timestamps(frames, bench.get("gpu_timestamps"), interval_report, directory)
    verify_requested_gpu_stages(run, bench.get("gpu_timestamps"), directory)
    verify_gpu_stages(bench.get("gpu_timestamps"), directory)
    verify_requested_surface_waits(run, bench.get("surface_wait_trace"), directory)
    verified_surface_anchors = verify_surface_waits(frames, bench.get("surface_wait_trace"), interval_report, directory)
    verified_upload_submissions = verify_transparent_upload(run, bench.get("transparent_index_upload"), directory)
    verified_alpha_records = verify_translucent_draw(run, bench.get("translucent_draw"), directory)
    assert run["exit_code"] == 0, directory
    assert entry["nearby_columns_ready"] and entry["entry_mesh_uploads_remaining"] == 0, directory
    keys = ("frames", "measured_seconds", "average_fps", "one_percent_low_fps",
            "p50_frame_ms", "p95_frame_ms", "p99_frame_ms", "loaded_chunks",
            "retained_native_chunks", "entities", "visible_chunks", "distant_terrain",
            "distant_gpu_tiles_visible_allocated_bytes", "chunk_phase_window",
            "chunk_phase_calls_total_ms_max_ms", "native_publication_work",
            "presentation_copy_samples_total_count_max_count", "native_light_maintenance",
            "terrain_gpu_allocations", "streaming_flight_route",
            "transparency_sort_batch", "worker_thread_priority", "near_opaque_shadow", "near_opaque_runs", "near_arena", "arena_upload_capture", "slow_frame_capture", "surface_configuration", "gpu_timestamps", "transparent_index_upload", "translucent_draw", "surface_wait_trace", "pre_acquire_visibility", "packed_frustum")
    performance = {key: bench[key] for key in keys if key in bench}
    if "gpu_timestamps" in performance:
        # Raw rows remain in benchmark.json; validate them, then keep this
        # comparison artifact compact rather than duplicating the capture.
        performance["gpu_timestamps"] = {
            key: value for key, value in performance["gpu_timestamps"].items() if key != "rows"
        }
        stages = performance["gpu_timestamps"].get("stages")
        if isinstance(stages, dict):
            performance["gpu_timestamps"]["stages"] = {
                key: value for key, value in stages.items() if key != "rows"
            }
    if "surface_wait_trace" in performance:
        # Full counters remain; the bounded raw observations stay in benchmark.json.
        performance["surface_wait_trace"] = {
            key: value for key, value in performance["surface_wait_trace"].items() if key != "top"
        }
    result = {"directory": str(directory), "run": run,
              "entry_ms": entry["request_to_first_play_ms"],
              "entry_cpu_ms": entry["request_to_data_and_cpu_mesh_ms"],
              "entry_startup": entry["startup_phases"],
              "performance": performance,
              "max_frame_ms": max(frames, default=0) * 1000,
              "verified_interval_indices": len(intervals),
              "verified_exact_interval_anchors": verified_anchors,
              "verified_gpu_timestamp_anchors": verified_gpu_anchors,
              "verified_surface_wait_retained_sample_anchors": verified_surface_anchors,
              "verified_transparent_upload_submissions_including_warmup": verified_upload_submissions,
              "verified_translucent_recorded_records_including_warmup": verified_alpha_records,
              "first_measured_present_application_ms": interval_report.get("first_measured_present_application_ms"),
              "interval_frame_boundary_note": interval_report.get("frame_boundary_note"),
              "interval_chunk_phase_order": bench.get("slow_present_intervals", {}).get("chunk_phase_order", []),
              "interval_chunk_phase_scope": bench.get("slow_present_intervals", {}).get("chunk_phase_scope"),
              "worst_intervals": intervals[:3],
              "worst_render_frames": bench.get("slow_render_frames", {}).get("frames", [])[:3]}
    render_report = bench.get("slow_render_frames", {})
    aggregate = render_report.get("all_measured_frames")
    if aggregate is not None:
        assert aggregate["count"] == render_report["measured_frames"], directory
        result["all_render_frames"] = aggregate
    memory_path = directory / "memory.jsonl"
    if memory_path.exists():
        result["memory_samples_including_possible_teardown"] = [
            json.loads(line) for line in memory_path.read_text(encoding="utf-8-sig").splitlines() if line.strip()
        ]
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directories", nargs="+", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    results = [summarize(path) for path in args.directories]
    args.output.write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
    for row in results:
        perf = row["performance"]
        print(f'{row["directory"]}: entry {row["entry_ms"]:.1f} ms, '
              f'avg {perf["average_fps"]:.1f}, low {perf["one_percent_low_fps"]:.1f}, '
              f'p99 {perf["p99_frame_ms"]:.2f} ms, max {row["max_frame_ms"]:.2f} ms; '
              f'{row["verified_interval_indices"]} interval indices verified')
