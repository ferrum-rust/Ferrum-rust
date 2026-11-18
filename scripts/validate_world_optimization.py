"""Run the native equivalence and renderer oracles against an already built test binary."""
import argparse
import json
import re
import subprocess
from pathlib import Path

ORACLES = {
    "covered-lod-gpu": "renderer::distant_render::covered_gpu_tests::covered_lod_tiles_match_original_generated_strides_holes_revisions_and_stereo",
    "covered-lod-boundaries-gpu": "renderer::distant_render::covered_gpu_tests::covered_lod_unknown_outside_halo_world_edges_and_clear_preserve_original_output",
    "gpu-stages-isolated": "renderer::gpu_timestamps::gpu_tests::gpu_stage_prefix_rows_preserve_queued_work_and_optional_coverage",
    "gpu-stages-renderer": "renderer::renderer_arena::renderer_arena_gpu_tests::gpu_stages_tests::gpu_stage_markers_preserve_actual_renderer_world_stereo_menu_postfx_and_ui",
    "packed-frustum-gpu": "renderer::renderer_arena::renderer_arena_gpu_tests::packed_frustum_gpu_tests::packed_frustum_matches_scalar_renderer_masks_stereo_publication_and_screenshot",
    "pre-acquire-visibility-gpu": "renderer::renderer_arena::renderer_arena_gpu_tests::pre_acquire_visibility_gpu_tests::pre_acquire_visibility_matches_original_carrier_output_and_failed_acquire_history",
    "two-helper-sort-gpu": "renderer::chunk_transparency::sort_batch::gpu_tests::scoped_two_tests::scoped_two_matches_original_pixels_and_gpu_indices_with_two_helpers",
    "alpha-mdi-lifecycle-gpu": "renderer::renderer_arena::renderer_arena_gpu_tests::translucent_mdi_gpu_tests::translucent_mdi_matches_direct_through_real_renderer_lifecycle_and_screenshot",
    "alpha-mdi-fallback-gpu": "renderer::renderer_arena::renderer_arena_gpu_tests::translucent_mdi_gpu_tests::translucent_mdi_record_limit_fallback_and_later_admission_preserve_direct_output",
    "alpha-mdi-order-gpu": "renderer::renderer_arena::renderer_arena_gpu_tests::translucent_mdi_gpu_tests::translucent_mdi_preserves_noncommutative_alpha_order_and_rejects_forward_order_control",
    "index-staging-lifecycle-gpu": "renderer::renderer_arena::renderer_arena_gpu_tests::transparent_staging_tests::transparent_staging_matches_queue_through_masks_stereo_busy_reuse_and_real_screenshot",
    "index-staging-fallback-gpu": "renderer::renderer_arena::renderer_arena_gpu_tests::transparent_staging_tests::transparent_staging_fallback_and_capture_preserve_queue_bytes",
    "index-staging-abandon-gpu": "renderer::renderer_arena::renderer_arena_gpu_tests::transparent_staging_tests::transparent_staging_rejects_raw_encode_and_poisoned_identical_eye_retry",
    "index-staging-batch-gpu": "renderer::renderer_arena::renderer_arena_gpu_tests::transparent_staging_tests::transparent_staging_batch_completion_matches_queue_with_98304_real_sorted_quads",
    "gpu-timestamps": "renderer::gpu_timestamps::gpu_tests::gpu_timestamp_rows_preserve_queued_draws_copies_and_terminal_lifecycle",
    "625-complete": "vanilla::runtime::terrain_work::tests::original_625_prepared_initial_light_preserves_hash_rng_callbacks_and_publication",
    "ore-625": "vanilla::runtime::terrain_work::tests::original_625_ore_lookahead_forced_ready_unavailable_and_pool_match",
    "native-diagnostics-625": "native_session::spawn_publication_tests::native_inert_diagnostics_original625_profile_preserves_boundary_state",
    "transparency-gpu": "renderer::geometry::tests::mixed_index_widths_preserve_textures_translucent_reordering_and_cached_draws",
    "far-gpu": "renderer::far_bundles::tests::grouped_far_cache_is_bounded_and_invalidates_payload_binding_eye_and_renderer_changes",
    "near-runs-gpu": "renderer::near_opaque_runs::gpu_tests::exact_opaque_runs_match_direct_pixels_through_payload_masks_bindings_and_reuse",
    "scoped-sort-gpu": "renderer::chunk_transparency::sort_batch::gpu_tests::all_sort_modes_match_vulkan_pixels_and_indices_with_real_batch_admission",
    "large-sort-gpu": "renderer::chunk_transparency::sort_batch::gpu_tests::large_sort_mode_matches_all_modes_with_actual_98304_plus_quad_admission",
    "wide-sort-gpu": "renderer::chunk_transparency::sort_batch::gpu_tests::wide_sort_mode_matches_original_and_old_limit_fallbacks_above_262144_quads",
    "upload-capture-gpu": "renderer::chunk_transparency::upload_capture_gpu_tests::captured_upload_matches_normal_gpu_output_and_payload_lifecycle",
    "arena-core-gpu": "renderer::near_arena::gpu_tests::native_arena_mdi_preserves_pixels_indices_and_queued_lifecycle",
    "arena-stale-encoder-gpu": "renderer::near_arena::gpu_tests::delayed_encoder_is_not_made_safe_by_queue_order",
    "arena-renderer-gpu": "renderer::renderer_arena::renderer_arena_gpu_tests::renderer_arena_matches_ordinary_and_explicit_dedicated_through_real_submit_lifecycle",
    "arena-failure-gpu": "renderer::renderer_arena::renderer_arena_gpu_tests::renderer_arena_checked_failures_keep_old_geometry_or_fail_closed_after_target",
    "arena-policy-gpu": "renderer::renderer_arena::renderer_arena_gpu_tests::policy_gpu_tests::automatic_arena_policy_preserves_original_pixels_through_upgrade_latch_failure_clear_and_screenshot",
    "arena-policy-controls-gpu": "renderer::renderer_arena::renderer_arena_gpu_tests::policy_gpu_tests::automatic_arena_policy_keeps_unmeasured_distances_and_diagnostic_controls_original",
    "arena-policy-live-diagnostics-gpu": "renderer::renderer_arena::renderer_arena_gpu_tests::policy_gpu_tests::diagnostic_activation_on_live_original_preserves_backing_and_rejects_live_shared",
    "arena-upload-capture-gpu": "renderer::renderer_arena::renderer_arena_gpu_tests::arena_upload_capture_tests::arena_upload_capture_matches_uncaptured_automatic_mdi_and_dedicated_lifecycle",
    "arena-upload-capture-failure-gpu": "renderer::renderer_arena::renderer_arena_gpu_tests::arena_upload_capture_tests::arena_upload_capture_preserves_pre_target_failures_fallback_and_post_target_poison",
    "models-ui": "gameplay::presentation_copy::tests::presentation_projection_matches_full_clone_models_inventory_hud_and_windows",
    "lazy-edit-revisit": "lighting_view::shared_tests::timed_eager_lazy_native_light_edit_and_revisit_probe",
    "spawn-resume": "populated_world::tests::fresh_entry_finishes_original_spawn_then_defers_view_and_survives_immediate_save",
    "sky-depth-gpu": "renderer::sky_gpu_tests::sky_is_not_a_depth_occluder_and_foreground_still_hides_celestial_bodies",
    "celestial-gpu": "celestial::tests::original_sun_moon_phases_stars_and_sunrise_framebuffer_pixels_match",
    "bounded-block-625": "native_session::maintenance::blocks::tests::original_625_block_maintenance_keeps_full_checkpoint_rng_order_and_dense_revisit",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--prefix", type=Path, required=True)
    parser.add_argument("--skip-normal", action="store_true",
                        help="Run only the selected exact ignored oracles.")
    parser.add_argument("--oracles", nargs="*", choices=ORACLES,
                        help="Run the normal suite and these exact oracles; omit for all.")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    args.prefix.parent.mkdir(parents=True, exist_ok=True)
    # A renamed/missing filter must fail before executing anything expensive.
    listing = subprocess.run([str(binary), "--list"], check=True, capture_output=True, text=True).stdout
    selected = ORACLES if args.oracles is None else {name: ORACLES[name] for name in args.oracles}
    for name in selected.values():
        assert name + ": test" in listing, f"Missing exact test: {name}"
    checks = []
    plan = ([] if args.skip_normal else [("tests", None)]) + list(selected.items())
    assert plan, "No validation checks selected"
    for label, test in plan:
        path = args.prefix.with_name(args.prefix.name + "-" + label + ".log")
        command = [str(binary), "--test-threads=2"] if test is None else [
            str(binary), test, "--ignored", "--exact", "--nocapture", "--test-threads=1"]
        print("Running", label, flush=True)
        with path.open("w", encoding="utf-8") as output:
            result = subprocess.run(command, stdout=output, stderr=subprocess.STDOUT)
        log = path.read_text(encoding="utf-8")
        summaries = re.findall(r"^test result:.*$", log, re.MULTILINE)
        assert result.returncode == 0 and len(summaries) == 1, (path, result.returncode, summaries)
        summary = summaries[0]
        match = re.search(r"ok\. (\d+) passed; 0 failed; (\d+) ignored", summary)
        assert match and (int(match[1]) > 0 if test is None else int(match[1]) == 1), (path, summary)
        checks.append({"path": str(path), "result": [summary], "exact_test": test})
        args.prefix.with_name(args.prefix.name + "-test-results.json").write_text(
            json.dumps({"binary": str(binary), "checks": checks}, indent=2) + "\n", encoding="utf-8")
        print(summary, flush=True)


if __name__ == "__main__":
    main()
