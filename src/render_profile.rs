//! Opt-in, bounded CPU diagnostics. No disk I/O or heap allocation on ordinary frames.
use serde::Serialize;
use std::time::Instant;

const LIMIT: usize = 32;

#[derive(Clone, Default, Serialize)]
pub(super) struct Stages {
    // Nested in Frame.pre_acquire_ms, outside post-acquire encode_ms.
    pub pre_acquire_section_visibility: f64,
    pub encoder_create: f64,
    pub model_cpu: f64,
    pub chest_cpu: f64,
    pub dynamic_gpu_writes: f64,
    pub clouds: f64,
    pub section_visibility: f64,
    pub visible_chunks_inclusive: f64,
    pub transparency_sort: f64,
    pub transparency_gpu_writes: f64,
    // Children of visible_chunks_inclusive; owner/join are inside scope.
    pub transparency_batch_admission: f64,
    pub transparency_batch_take: f64,
    pub transparency_batch_scope: f64,
    pub transparency_batch_owner: f64,
    pub transparency_batch_join_tail: f64,
    pub transparency_batch_restore: f64,
    pub visible_order: f64,
    pub block_overlay: f64,
    pub terrain_bundles: f64,
    pub near_opaque_run_prepare: f64,
    // Build is nested in prepare; replay is nested in world_near_opaque.
    pub near_opaque_run_build: f64,
    pub near_opaque_run_replay: f64,
    pub far_bundle_prepare: f64,
    pub camera_uniform_writes: f64,
    pub world_draw_record: f64,
    // Children of world_draw_record; do not add them again in finish().
    pub world_near_opaque: f64,
    pub world_far_opaque: f64,
    pub world_sky_dynamic: f64,
    pub world_translucent: f64,
    pub world_pass_finish: f64,
    pub postfx: f64,
    pub banner_upload: f64,
    pub ui_gpu_writes: f64,
    pub ui_draw_record: f64,
    pub encode_other: f64,
}

impl Stages {
    pub fn finish(&mut self, encode_ms: f64) {
        // Early visibility belongs to Frame.pre_acquire_ms, not encode_ms.
        // The visible-chunk span contains both transparency spans.
        let measured = self.encoder_create + self.model_cpu + self.chest_cpu + self.dynamic_gpu_writes
            + self.clouds + self.section_visibility + self.visible_chunks_inclusive
            + self.visible_order + self.block_overlay + self.terrain_bundles + self.near_opaque_run_prepare + self.far_bundle_prepare
            + self.camera_uniform_writes + self.world_draw_record + self.postfx
            + self.banner_upload + self.ui_gpu_writes + self.ui_draw_record;
        self.encode_other = (encode_ms - measured).max(0.);
    }
}

#[derive(Clone, Default, Serialize)]
pub(super) struct Counters {
    pub loaded_chunks: usize,
    pub visible_chunks: usize,
    pub distant_visible_tiles: usize,
    pub visible_entities: usize,
    pub total_entities: usize,
    pub dynamic_vertices: usize,
    pub dynamic_indices: usize,
    pub tracked_buffer_write_calls: usize,
    pub tracked_buffer_write_bytes: usize,
    pub tracked_buffer_resizes: usize,
    pub transparency_sorted_chunks: usize,
    pub transparency_scanned_quads: usize,
    pub transparency_sorted_quads: usize,
    pub transparency_unchanged_chunks: usize,
    pub transparency_upload_bytes_avoided: usize,
    pub transparency_batch_frames: usize,
    pub transparency_batch_jobs: usize,
    pub transparency_batch_quads: usize,
    pub transparency_batch_worker_jobs: usize,
    pub transparency_batch_small: usize,
    pub transparency_batch_limit: usize,
    pub transparency_batch_unavailable: usize,
    pub terrain_bundle_rebuilds: usize,
    pub terrain_bundle_draws: usize,
    pub near_bundled_eyes: usize,
    pub near_direct_draws: usize,
    pub near_translucent_direct_draws: usize,
    pub near_translucent_mdi_calls: usize,
    pub near_translucent_mdi_records: usize,
    pub far_direct_draws: usize,
    pub far_bundle_rebuild_groups: usize,
    pub far_bundle_recorded_draws: usize,
    pub far_cached_visible_groups: usize,
    pub far_cached_visible_tiles: usize,
    pub far_bundle_runs: usize,
    pub far_bundle_draws: usize,
}

#[derive(Clone, Default, Serialize)]
pub(super) struct Frame {
    #[serde(skip)]
    pub enabled: bool,
    pub frame_id: u64,
    pub application_seconds: f32,
    pub render_wall_ms: f64,
    pub render_thread_cpu_ms: Option<f64>,
    pub encode_thread_cpu_ms: Option<f64>,
    pub pre_acquire_ms: f64,
    pub surface_acquire_ms: f64,
    pub encode_ms: f64,
    pub finish_and_submit_ms: f64,
    pub present_ms: f64,
    pub camera: [f32; 3],
    pub stages_ms: Stages,
    pub models: crate::models::build_profile::Profile,
    pub counters: Counters,
}

impl Frame {
    #[inline]
    pub fn timer(&self) -> Option<Instant> { self.enabled.then(Instant::now) }
    #[inline]
    pub fn thread_cpu(&self) -> Option<u64> { self.enabled.then(thread_cpu_100ns).flatten() }
    pub fn buffer_write(&mut self, bytes: usize, resized: bool) {
        if !self.enabled { return; }
        self.counters.tracked_buffer_resizes += usize::from(resized);
        if bytes != 0 {
            self.counters.tracked_buffer_write_calls += 1;
            self.counters.tracked_buffer_write_bytes += bytes;
        }
    }
}

#[inline]
pub(super) fn elapsed(start: Option<Instant>) -> f64 {
    start.map_or(0., |at| at.elapsed().as_secs_f64() * 1000.)
}

pub(super) fn cpu_elapsed(start: Option<u64>, end: Option<u64>) -> Option<f64> {
    Some(end?.saturating_sub(start?) as f64 / 10_000.)
}

// Exhaustive destructuring prevents a newly added stage/counter from silently
// disappearing from aggregate reports. Only these fixed numeric structs are used.
macro_rules! add_fields {
    ($to:expr, $from:expr, $kind:ident, [$($time:ident),* $(,)?],
        [$($count:ident),* $(,)?], [$($skip:ident),* $(,)?]) => {{
        let $kind { $($time,)* $($count,)* $($skip: _,)* } = $from;
        $($to.$time += *$time;)*
        $($to.$count = $to.$count.saturating_add(*$count);)*
    }};
}

#[derive(Default, Serialize)]
struct OptionalTimeTotal {
    samples: u64,
    total_ms: f64,
}
impl OptionalTimeTotal {
    fn add(&mut self, milliseconds: Option<f64>) {
        if let Some(milliseconds) = milliseconds {
            self.samples += 1;
            self.total_ms += milliseconds;
        }
    }
}

#[derive(Default, Serialize)]
struct FrameTotals {
    render_wall_ms: f64,
    pre_acquire_ms: f64,
    surface_acquire_ms: f64,
    encode_ms: f64,
    finish_and_submit_ms: f64,
    present_ms: f64,
    render_thread_cpu: OptionalTimeTotal,
    encode_thread_cpu: OptionalTimeTotal,
    stages_ms: Stages,
    models: crate::models::build_profile::Profile,
    counters: Counters,
}
impl FrameTotals {
    fn add(&mut self, frame: &Frame) {
        self.render_wall_ms += frame.render_wall_ms;
        self.pre_acquire_ms += frame.pre_acquire_ms;
        self.surface_acquire_ms += frame.surface_acquire_ms;
        self.encode_ms += frame.encode_ms;
        self.finish_and_submit_ms += frame.finish_and_submit_ms;
        self.present_ms += frame.present_ms;
        self.render_thread_cpu.add(frame.render_thread_cpu_ms);
        self.encode_thread_cpu.add(frame.encode_thread_cpu_ms);
        add_fields!(self.stages_ms, &frame.stages_ms, Stages, [
            pre_acquire_section_visibility, encoder_create, model_cpu, chest_cpu, dynamic_gpu_writes, clouds, section_visibility,
            visible_chunks_inclusive, transparency_sort, transparency_gpu_writes, visible_order,
            transparency_batch_admission, transparency_batch_take, transparency_batch_scope, transparency_batch_owner, transparency_batch_join_tail, transparency_batch_restore,
            block_overlay, terrain_bundles, near_opaque_run_prepare, near_opaque_run_build, near_opaque_run_replay, far_bundle_prepare, camera_uniform_writes,
            world_draw_record, world_near_opaque, world_far_opaque, world_sky_dynamic,
            world_translucent, world_pass_finish, postfx, banner_upload, ui_gpu_writes,
            ui_draw_record, encode_other
        ], [], []);
        add_fields!(self.counters, &frame.counters, Counters, [], [
            loaded_chunks, visible_chunks, distant_visible_tiles, visible_entities, total_entities,
            dynamic_vertices, dynamic_indices, tracked_buffer_write_calls,
            tracked_buffer_write_bytes, tracked_buffer_resizes, transparency_sorted_chunks,
            transparency_scanned_quads, transparency_sorted_quads, transparency_unchanged_chunks,
            transparency_upload_bytes_avoided, terrain_bundle_rebuilds, terrain_bundle_draws,
            transparency_batch_frames, transparency_batch_jobs, transparency_batch_quads, transparency_batch_worker_jobs, transparency_batch_small, transparency_batch_limit, transparency_batch_unavailable,
            near_bundled_eyes, near_direct_draws, near_translucent_direct_draws, near_translucent_mdi_calls, near_translucent_mdi_records, far_direct_draws,
            far_bundle_rebuild_groups, far_bundle_recorded_draws, far_cached_visible_groups,
            far_cached_visible_tiles, far_bundle_runs, far_bundle_draws
        ], []);
        use crate::models::build_profile::Profile as ModelProfile;
        add_fields!(self.models, &frame.models, ModelProfile, [
            block_entities_ms, mobs_ms, drops_orbs_ms, other_entities_ms, server_ms, player_ms,
            particles_ms, weather_ms
        ], [
            world_columns, mobs, native_mobs, native_early_distance, basic_early_distance, drops,
            orbs, particles, arrows, mob_shots, falling, pickup_effects
        ], [enabled]);
    }
}

#[derive(Default)]
pub(super) struct Recorder {
    threshold: crate::benchmark_profile::SlowFrameMs,
    pub enabled: bool,
    next_frame: u64,
    measured_frames: u64,
    slow_frames: u64,
    totals: FrameTotals,
    retained: Vec<Frame>,
}

impl Recorder {
    pub fn configure_threshold(&mut self, threshold: crate::benchmark_profile::SlowFrameMs) {
        assert_eq!(self.next_frame, 0, "configure benchmark threshold before any frame begins");
        self.threshold = threshold;
    }
    pub fn begin(&mut self, application_seconds: f32) -> Frame {
        self.next_frame += 1;
        Frame { enabled: self.enabled, frame_id: self.next_frame, application_seconds,
            models: crate::models::build_profile::Profile { enabled: self.enabled, ..Default::default() },
            ..Default::default() }
    }
    pub fn finish(&mut self, frame: Frame) {
        if !frame.enabled { return; }
        self.measured_frames += 1;
        self.totals.add(&frame);
        if frame.render_wall_ms < self.threshold.get() { return; }
        self.slow_frames += 1;
        if self.retained.len() < LIMIT {
            if self.retained.capacity() == 0 { self.retained.reserve_exact(LIMIT); }
            self.retained.push(frame);
        } else {
            let (index, shortest) = self.retained.iter().enumerate()
                .min_by(|(_, a), (_, b)| a.render_wall_ms.total_cmp(&b.render_wall_ms)).unwrap();
            if frame.render_wall_ms > shortest.render_wall_ms { self.retained[index] = frame; }
        }
    }
    pub fn report(&self) -> serde_json::Value {
        let mut retained = self.retained.clone();
        retained.sort_unstable_by(|a, b| b.render_wall_ms.total_cmp(&a.render_wall_ms));
        serde_json::json!({
            "measured_frames": self.measured_frames, "slow_frames": self.slow_frames,
            "retained_limit": LIMIT, "threshold_ms": self.threshold.get(), "frames": retained,
            "all_measured_frames": {
                "count": self.measured_frames, "totals": &self.totals,
                "note": "Totals cover every successful enabled render finish, including frames below the slow threshold; warmup/disabled and failed surface acquisitions are excluded. Divide stages/counters by count for per-frame means, including zero/unexecuted stages. Optional CPU totals use their own available-sample counts; valid quantized zeroes count. Gauge counters such as loaded/visible chunks and entities sum frame observations, not distinct objects. Counter totals saturate on overflow. Stages retain the nested-span meaning below and must not be added across parent/child levels."
            },
            "note": "The slowest successful measured render calls, with all stages from the same frame. CPU wall times are not GPU timestamps. pre_acquire_ms is outside encode_ms and includes its nested stages_ms.pre_acquire_section_visibility; post-acquire section_visibility stays inside encode_ms. Early visibility is excluded from encode_other subtraction. visible_chunks_inclusive includes transparency_sort, transparency_gpu_writes and all transparency_batch spans. With scoped sorting, transparency_sort sums overlapping per-job wall times; batch_scope is the main-thread critical-path wall span, with batch_owner and batch_join_tail nested inside it. Do not infer a speedup by comparing the overlapping sum alone; world_draw_record includes its world_* child stages; model_cpu includes the models subphases. Do not add parent and child spans twice. Windows thread CPU times include kernel+user time and may be quantized; null means unavailable. Buffer counters cover dynamic/cloud/overlay geometry, transparency, camera uniforms and UI, not texture or celestial/postfx internal writes. Profiling is disabled outside benchmark measurement; no per-frame disk I/O."
        })
    }
}

#[cfg(windows)]
fn thread_cpu_100ns() -> Option<u64> {
    #[repr(C)]
    #[derive(Default)]
    struct FileTime { low: u32, high: u32 }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThread() -> *mut std::ffi::c_void;
        fn GetThreadTimes(thread: *mut std::ffi::c_void, creation: *mut FileTime,
            exit: *mut FileTime, kernel: *mut FileTime, user: *mut FileTime) -> i32;
    }
    let (mut creation, mut exit, mut kernel, mut user) =
        (FileTime::default(), FileTime::default(), FileTime::default(), FileTime::default());
    // GetCurrentThread returns a borrowed pseudo-handle; it must not be closed.
    let ok = unsafe { GetThreadTimes(GetCurrentThread(), &mut creation, &mut exit, &mut kernel, &mut user) };
    (ok != 0).then(|| (u64::from(kernel.high) << 32 | u64::from(kernel.low))
        + (u64::from(user.high) << 32 | u64::from(user.low)))
}

#[cfg(not(windows))]
fn thread_cpu_100ns() -> Option<u64> { None }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configurable_threshold_is_inclusive_without_changing_totals_or_bounded_selection() {
        let mut recorder = Recorder::default();
        assert_eq!(recorder.report()["threshold_ms"], 8.);
        recorder.configure_threshold("4".parse().unwrap());
        let mut warmup = recorder.begin(0.); warmup.render_wall_ms = 999.; recorder.finish(warmup);
        recorder.enabled = true;
        let abandoned = recorder.begin(0.); drop(abandoned);
        for duration in [3.999, 4., 4.001] {
            let mut frame = recorder.begin(duration as f32); frame.render_wall_ms = duration;
            frame.stages_ms.model_cpu = duration / 2.; recorder.finish(frame);
        }
        assert_eq!(recorder.report()["measured_frames"], 3);
        assert_eq!(recorder.report()["slow_frames"], 2);
        assert_eq!(recorder.retained[0].render_wall_ms, 4., "equality is admitted");
        assert!((recorder.totals.render_wall_ms - 12.).abs() < 1e-10, "below-threshold frame remains in all-frame totals");
        for duration in 4..84 {
            let mut frame = recorder.begin(duration as f32); frame.render_wall_ms = duration as f64;
            frame.stages_ms.model_cpu = duration as f64 / 2.; recorder.finish(frame);
        }
        let report = recorder.report();
        assert_eq!(report["threshold_ms"], 4.);
        assert_eq!(report["measured_frames"], 83); assert_eq!(report["slow_frames"], 82);
        assert_eq!(recorder.retained.len(), LIMIT);
        assert_eq!(report["frames"][0]["render_wall_ms"], 83.);
        assert_eq!(report["frames"][31]["render_wall_ms"], 52.);
        assert_eq!(report["frames"][31]["stages_ms"]["model_cpu"], 26.);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
            recorder.configure_threshold("8".parse().unwrap()))).is_err(), "cannot silently mix thresholds after frames begin");
        assert_eq!(recorder.report()["threshold_ms"], 4.);
    }
    #[test]
    fn bounded_outliers_keep_correlated_frame_stages_and_ignore_disabled_frames() {
        let mut recorder = Recorder::default();
        let mut ignored = recorder.begin(0.); ignored.render_wall_ms = 999.; recorder.finish(ignored);
        recorder.enabled = true;
        for duration in 0..100 {
            let mut frame = recorder.begin(duration as f32);
            frame.render_wall_ms = duration as f64;
            frame.stages_ms.model_cpu = duration as f64 / 2.;
            recorder.finish(frame);
        }
        let report = recorder.report();
        assert_eq!(report["measured_frames"], 100);
        assert_eq!(report["slow_frames"], 92);
        assert_eq!(report["frames"].as_array().unwrap().len(), LIMIT);
        assert_eq!(report["frames"][0]["render_wall_ms"], 99.);
        assert_eq!(report["frames"][0]["stages_ms"]["model_cpu"], 49.5);
        assert_eq!(report["frames"][31]["render_wall_ms"], 68.);
        assert_eq!(report["all_measured_frames"]["count"], 100);
        assert_eq!(report["all_measured_frames"]["totals"]["render_wall_ms"], 4950.);
        assert_eq!(report["all_measured_frames"]["totals"]["stages_ms"]["model_cpu"], 2475.);
        assert_eq!(cpu_elapsed(Some(10_000), Some(35_000)), Some(2.5));
        assert_eq!(cpu_elapsed(None, Some(35_000)), None);
        let mut stages = Stages { visible_chunks_inclusive: 5., transparency_sort: 3.,
            transparency_gpu_writes: 1., model_cpu: 2., ..Default::default() };
        stages.finish(10.);
        assert_eq!(stages.encode_other, 3., "nested transparency timings must not be counted twice");
    }
    #[test]
    fn early_visibility_is_outside_encode_and_successful_totals_exclude_aborted_attempts() {
        let mut recorder=Recorder::default();recorder.enabled=true;
        let mut failed=recorder.begin(0.);failed.pre_acquire_ms=99.;drop(failed);
        let mut frame=recorder.begin(1.);frame.render_wall_ms=8.;frame.pre_acquire_ms=2.;frame.encode_ms=5.;
        frame.stages_ms.pre_acquire_section_visibility=1.5;frame.stages_ms.model_cpu=1.;
        frame.stages_ms.finish(frame.encode_ms);assert_eq!(frame.stages_ms.encode_other,4.);
        recorder.finish(frame);let report=recorder.report();
        assert_eq!(report["all_measured_frames"]["count"],1);
        let totals=&report["all_measured_frames"]["totals"];
        assert_eq!(totals["pre_acquire_ms"],2.);assert_eq!(totals["encode_ms"],5.);
        assert_eq!(totals["stages_ms"]["pre_acquire_section_visibility"],1.5);
        assert_eq!(report["frames"][0]["pre_acquire_ms"],2.);
    }
    #[test]
    fn aggregate_window_counts_fast_frames_zero_cpu_samples_and_nested_stages_once() {
        let mut recorder = Recorder::default();
        let mut warmup = recorder.begin(0.);
        warmup.render_wall_ms = 999.;
        warmup.counters.tracked_buffer_write_bytes = usize::MAX;
        // Inclusion follows the frame's captured enable flag, not a later toggle.
        recorder.enabled = true;
        recorder.finish(warmup);
        let mut abandoned = recorder.begin(1.); abandoned.render_wall_ms = 999.;
        drop(abandoned); // Surface-acquire failures never call finish().
        let mut fast = recorder.begin(2.);
        fast.render_wall_ms = 4.; fast.surface_acquire_ms = 0.5;
        fast.encode_ms = 3.; fast.finish_and_submit_ms = 0.25; fast.present_ms = 0.25;
        fast.render_thread_cpu_ms = Some(0.); // Available, quantized sample.
        fast.stages_ms = Stages { visible_chunks_inclusive: 2., transparency_sort: 1.,
            transparency_gpu_writes: 0.5, world_draw_record: 0.5, world_near_opaque: 0.25,
            model_cpu: 0.25, ..Default::default() };
        fast.stages_ms.finish(3.);
        fast.models.mobs_ms = 0.25; fast.models.mobs = 4;
        fast.counters.loaded_chunks = 100; fast.counters.tracked_buffer_write_calls = 1;
        fast.counters.tracked_buffer_write_bytes = 4096; fast.counters.transparency_sorted_quads = 12;
        fast.stages_ms.transparency_batch_admission=0.01;
        fast.stages_ms.transparency_batch_take=0.01;
        fast.stages_ms.transparency_batch_scope=0.01;
        fast.stages_ms.transparency_batch_owner=0.01;
        fast.stages_ms.transparency_batch_join_tail=0.01;
        fast.stages_ms.transparency_batch_restore=0.01;
        fast.counters.near_translucent_mdi_calls=2;
        fast.counters.near_translucent_mdi_records=6;
        fast.counters.transparency_batch_frames=2;
        fast.counters.transparency_batch_jobs=2;
        fast.counters.transparency_batch_quads=2;
        fast.counters.transparency_batch_worker_jobs=2;
        fast.counters.transparency_batch_small=2;
        fast.counters.transparency_batch_limit=2;
        fast.counters.transparency_batch_unavailable=2;
        recorder.finish(fast);
        let mut slow = recorder.begin(3.);
        slow.render_wall_ms = 12.; slow.surface_acquire_ms = 1.;
        slow.encode_ms = 10.; slow.finish_and_submit_ms = 0.5; slow.present_ms = 0.5;
        slow.encode_thread_cpu_ms = Some(2.);
        slow.stages_ms = Stages { visible_chunks_inclusive: 5., transparency_sort: 3.,
            transparency_gpu_writes: 1., world_draw_record: 2., world_near_opaque: 1.,
            model_cpu: 0.75, ..Default::default() };
        slow.stages_ms.finish(10.);
        slow.models.mobs_ms = 0.75; slow.models.mobs = 6;
        slow.counters.loaded_chunks = 120; slow.counters.tracked_buffer_write_calls = 2;
        slow.counters.tracked_buffer_write_bytes = 8192; slow.counters.transparency_sorted_quads = 20;
        slow.stages_ms.transparency_batch_admission=0.02;
        slow.stages_ms.transparency_batch_take=0.02;
        slow.stages_ms.transparency_batch_scope=0.02;
        slow.stages_ms.transparency_batch_owner=0.02;
        slow.stages_ms.transparency_batch_join_tail=0.02;
        slow.stages_ms.transparency_batch_restore=0.02;
        slow.counters.near_translucent_mdi_calls=3;
        slow.counters.near_translucent_mdi_records=9;
        slow.counters.transparency_batch_frames=3;
        slow.counters.transparency_batch_jobs=3;
        slow.counters.transparency_batch_quads=3;
        slow.counters.transparency_batch_worker_jobs=3;
        slow.counters.transparency_batch_small=3;
        slow.counters.transparency_batch_limit=3;
        slow.counters.transparency_batch_unavailable=3;
        recorder.finish(slow);
        recorder.enabled = false;
        let mut disabled = recorder.begin(4.); disabled.render_wall_ms = 999.;
        recorder.finish(disabled);
        let report = recorder.report();
        let aggregate = &report["all_measured_frames"];
        assert_eq!(aggregate["count"], 2);
        let totals = &aggregate["totals"];
        for (field,expected) in [("render_wall_ms",16.),("surface_acquire_ms",1.5),("encode_ms",13.),
            ("finish_and_submit_ms",0.75),("present_ms",0.75)] { assert_eq!(totals[field],expected, "{field}"); }
        assert_eq!(totals["render_thread_cpu"], serde_json::json!({"samples":1,"total_ms":0.}));
        assert_eq!(totals["encode_thread_cpu"], serde_json::json!({"samples":1,"total_ms":2.}));
        for (field,expected) in [("visible_chunks_inclusive",7.),("transparency_sort",4.),
            ("transparency_gpu_writes",1.5),("world_draw_record",2.5),("world_near_opaque",1.25),
            ("model_cpu",1.),("encode_other",2.5)] { assert_eq!(totals["stages_ms"][field],expected, "{field}"); }
        for field in ["transparency_batch_admission", "transparency_batch_take", "transparency_batch_scope", "transparency_batch_owner", "transparency_batch_join_tail", "transparency_batch_restore"] {assert!((totals["stages_ms"][field].as_f64().unwrap()-0.03).abs()<1e-12);}
        assert_eq!(totals["counters"]["near_translucent_mdi_calls"],5);
        assert_eq!(totals["counters"]["near_translucent_mdi_records"],15);
        for field in ["transparency_batch_frames", "transparency_batch_jobs", "transparency_batch_quads", "transparency_batch_worker_jobs", "transparency_batch_small", "transparency_batch_limit", "transparency_batch_unavailable"] {assert_eq!(totals["counters"][field],5);}
        assert_eq!(totals["models"]["mobs_ms"], 1.);
        assert_eq!(totals["models"]["mobs"], 10);
        for (field,expected) in [("loaded_chunks",220),("tracked_buffer_write_calls",3),
            ("tracked_buffer_write_bytes",12288),("transparency_sorted_quads",32)] {
            assert_eq!(totals["counters"][field],expected, "{field}");
        }
        assert_eq!(report["slow_frames"], 1);
        assert_eq!(report["frames"].as_array().unwrap().len(), 1);
        assert_eq!(report["frames"][0]["frame_id"], 4);
        assert_eq!(report["frames"][0]["render_wall_ms"], 12.);
    }
    #[test]
    fn opaque_run_nested_spans_are_aggregated_without_double_counting() {
        let mut recorder=Recorder::default();recorder.enabled=true;
        let mut frame=recorder.begin(0.);frame.render_wall_ms=5.;frame.encode_ms=4.;
        frame.stages_ms=Stages{near_opaque_run_prepare:0.5,near_opaque_run_build:0.25,
            world_draw_record:2.,world_near_opaque:0.75,near_opaque_run_replay:0.5,..Default::default()};
        frame.stages_ms.finish(4.);assert_eq!(frame.stages_ms.encode_other,1.5);
        recorder.finish(frame);let report=recorder.report();let stages=&report["all_measured_frames"]["totals"]["stages_ms"];
        assert_eq!(stages["near_opaque_run_prepare"],0.5);assert_eq!(stages["near_opaque_run_build"],0.25);
        assert_eq!(stages["near_opaque_run_replay"],0.5);assert_eq!(stages["encode_other"],1.5);
    }
    #[test]
    #[cfg(windows)]
    fn current_thread_cpu_counter_is_available_and_monotonic() {
        let first = thread_cpu_100ns().expect("GetThreadTimes current pseudo-handle");
        let second = thread_cpu_100ns().expect("GetThreadTimes current pseudo-handle");
        assert!(second >= first);
    }
}
