//! Benchmark-only diagnostics for work between successful window presents.
//! Callback spans are inclusive; only callbacks outside frame() add coverage.
use serde::Serialize;
use std::time::Instant;

const LIMIT: usize = 32;

// Fixed scope: only timers emitted synchronously inside Game::update_chunks.
// Lifetime maxima cannot identify the current invocation and are never sampled.
const CHUNK_PHASE_NAMES: [&str; 32] = [
    "native_publication", "import_publication", "stream_requests_and_imports", "visual_jobs_gpu_upload",
    "native_queue_edits", "native_queue_rebuild", "native_submit", "native_apply_filter",
    "native_metadata_collect", "native_update_release", "native_overlay_scan", "native_replace_and_drop",
    "native_apply_scratch_release", "native_restore_tiles", "native_adopt_containers", "native_adopt_spawns",
    "visual_accept_validate", "visual_accept_lights", "visual_accept_meshes", "visual_snapshot_release",
    "visual_gpu_upload", "visual_snapshot_clone",
    "stream_trim_scan", "stream_lod_capture", "stream_world_unload", "stream_renderer_remove",
    "stream_request_prepare", "stream_request_submit", "stream_import_metadata",
    "visual_light_arc_publish", "visual_light_owner_return", "visual_light_dirty_merge",
];
type ChunkTimings = std::collections::BTreeMap<&'static str, [f64; 3]>;
#[derive(Clone, Copy)]
struct ChunkSample([[f64; 2]; CHUNK_PHASE_NAMES.len()],Option<[[f64;2];10]>);
impl ChunkSample {
    fn capture(timings: &ChunkTimings) -> Self {
        let uploads=timings.contains_key(crate::renderer::upload_capture::PHASE_NAMES[0]).then(||
            std::array::from_fn(|i|timings.get(crate::renderer::upload_capture::PHASE_NAMES[i])
                .map_or([0.;2],|v|[v[0],v[1]])));
        Self(std::array::from_fn(|i| timings.get(CHUNK_PHASE_NAMES[i])
            .map_or([0.; 2], |v| [v[0], v[1]])),uploads)
    }
}
pub(super) struct ChunkStart { at: Instant, before: ChunkSample }

#[derive(Clone, Copy)]
pub(super) enum Origin { Frame, UserEvent, AboutToWait }
#[derive(Clone, Copy)]
pub(super) enum Stage { Accept, InputReplay, TickLighting, Submit, Copy, CatchupPublication }

#[derive(Clone, Default, Serialize)]
pub(super) struct Span {
    calls: u64,
    total_ms: f64,
    max_ms: f64,
}
impl Span {
    fn add(&mut self, ms: f64) {
        self.calls += 1;
        self.total_ms += ms;
        self.max_ms = self.max_ms.max(ms);
    }
}
#[derive(Clone, Default, Serialize)]
struct Services { frame: Span, user_event: Span, about_to_wait: Span }
#[derive(Clone, Default, Serialize)]
struct Stages {
    accept_inclusive: Span,
    input_replay_inclusive: Span,
    tick_lighting: Span,
    submit_inclusive: Span,
    presentation_copy: Span,
    catchup_publication: Span,
}
#[derive(Clone, Default, Serialize)]
struct FrameWork {
    calls: u64,
    simulation_streaming_ms: f64,
    gui_ms: f64,
    render_ms: f64,
}
impl FrameWork {
    fn total(&self) -> f64 { self.simulation_streaming_ms + self.gui_ms + self.render_ms }
}
// Chronological bounds reuse existing clock samples. They are NOT additional
// covered work: external services can lie in either gap, and failed attempts
// leave inter-attempt events inside first_to_last_work_span_ms.
#[derive(Clone, Serialize)]
struct FrameBoundaries {
    first_frame_start_application_ms: Option<f64>,
    last_frame_work_end_application_ms: Option<f64>,
    before_first_frame_ms: Option<f64>,
    first_to_last_work_span_ms: Option<f64>,
    after_last_frame_work_ms: Option<f64>,
}
impl FrameBoundaries {
    fn new(previous: Instant, present: Instant, origin: Option<Instant>,
        first: Instant, last: Instant) -> Self {
        let duration = |end: Instant, start: Instant| end.checked_duration_since(start)
            .map(|d| d.as_secs_f64() * 1000.);
        Self {
            first_frame_start_application_ms: origin.and_then(|at| duration(first, at)),
            last_frame_work_end_application_ms: origin.and_then(|at| duration(last, at)),
            before_first_frame_ms: duration(first, previous),
            first_to_last_work_span_ms: duration(last, first),
            after_last_frame_work_ms: duration(present, last),
        }
    }
}
#[derive(Clone, Default, Serialize)]
struct Interval {
    sample_index: u64,
    // Render context sampled before drawing, not the present boundary.
    application_seconds: f32,
    begin_application_ms: Option<f64>,
    end_application_ms: Option<f64>,
    wall_ms: f64,
    thread_cpu_ms: Option<f64>,
    covered_wall_ms: f64,
    outside_covered_wall_ms: f64,
    camera: [f64; 3],
    frame_work: FrameWork,
    frame_boundaries: Option<FrameBoundaries>,
    chunk_update_inclusive: Span,
    distant_update_inclusive: Span,
    chunk_phase_calls: [u64; CHUNK_PHASE_NAMES.len()],
    chunk_phase_total_ms: [f64; CHUNK_PHASE_NAMES.len()],
    #[serde(skip_serializing_if="Option::is_none")]
    upload_child_calls:Option<[u64;10]>,
    #[serde(skip_serializing_if="Option::is_none")]
    upload_child_total_ms:Option<[f64;10]>,
    service_inclusive: Services,
    service_stages: Stages,
    retired_on_window: u64,
    audio: crate::audio::ProfileStats,
}

#[derive(Default)]
pub(super) struct Recorder {
    threshold: crate::benchmark_profile::SlowFrameMs,
    application_origin: Option<Instant>,
    first_measured_present: Option<Instant>,
    previous_present: Option<Instant>,
    previous_cpu: Option<u64>,
    // Pending only; never serialized as Instants or kept across a successful
    // boundary (including first/warmup and unretained fast intervals).
    first_frame_start: Option<Instant>,
    last_frame_work_end: Option<Instant>,
    current: Interval,
    measured: u64,
    slow: u64,
    retained: Vec<Interval>,
}
impl Recorder {
    pub fn with_threshold(threshold: crate::benchmark_profile::SlowFrameMs) -> Self {
        Self { threshold, ..Default::default() }
    }
    #[inline]
    pub fn enabled(&self) -> bool { self.previous_present.is_some() }
    #[inline]
    pub fn timer(&self) -> Option<Instant> { self.enabled().then(Instant::now) }
    pub fn service(&mut self, origin: Origin, start: Option<Instant>) {
        let Some(start) = start else { return; };
        let span = match origin {
            Origin::Frame => &mut self.current.service_inclusive.frame,
            Origin::UserEvent => &mut self.current.service_inclusive.user_event,
            Origin::AboutToWait => &mut self.current.service_inclusive.about_to_wait,
        };
        span.add(start.elapsed().as_secs_f64() * 1000.);
    }
    pub fn stage(&mut self, stage: Stage, start: Option<Instant>) {
        if let Some(start) = start { self.stage_ms(stage, start.elapsed().as_secs_f64() * 1000.); }
    }
    pub fn stage_ms(&mut self, stage: Stage, ms: f64) {
        if !self.enabled() { return; }
        let span = match stage {
            Stage::Accept => &mut self.current.service_stages.accept_inclusive,
            Stage::InputReplay => &mut self.current.service_stages.input_replay_inclusive,
            Stage::TickLighting => &mut self.current.service_stages.tick_lighting,
            Stage::Submit => &mut self.current.service_stages.submit_inclusive,
            Stage::Copy => &mut self.current.service_stages.presentation_copy,
            Stage::CatchupPublication => &mut self.current.service_stages.catchup_publication,
        };
        span.add(ms);
    }
    pub fn retired_on_window(&mut self, retired: bool) {
        if self.enabled() { self.current.retired_on_window += u64::from(retired); }
    }
    pub fn begin_chunk_update(&self, timings: &ChunkTimings) -> Option<ChunkStart> {
        if !self.enabled() { return None; }
        let before = ChunkSample::capture(timings);
        Some(ChunkStart { at: Instant::now(), before })
    }
    pub fn end_chunk_update(&mut self, started: Option<ChunkStart>, timings: &ChunkTimings) {
        if let Some(started) = started {
            let elapsed_ms = started.at.elapsed().as_secs_f64() * 1000.;
            self.chunk_update_ms(started.before, ChunkSample::capture(timings), elapsed_ms);
        }
    }
    fn chunk_update_ms(&mut self, before: ChunkSample, after: ChunkSample, elapsed_ms: f64) {
        if !self.enabled() { return; }
        self.current.chunk_update_inclusive.add(elapsed_ms);
        for i in 0..CHUNK_PHASE_NAMES.len() {
            self.current.chunk_phase_calls[i] += (after.0[i][0] - before.0[i][0]).max(0.) as u64;
            self.current.chunk_phase_total_ms[i] += (after.0[i][1] - before.0[i][1]).max(0.);
        }
        if let(Some(before),Some(after))=(before.1,after.1) {
            let calls=self.current.upload_child_calls.get_or_insert([0;10]);
            let times=self.current.upload_child_total_ms.get_or_insert([0.;10]);
            for i in 0..10 {calls[i]+=(after[i][0]-before[i][0]).max(0.) as u64;
                times[i]+=(after[i][1]-before[i][1]).max(0.);}
        }
    }
    pub fn end_distant_update(&mut self, started: Option<Instant>) {
        if let Some(started) = started {
            self.current.distant_update_inclusive.add(started.elapsed().as_secs_f64() * 1000.);
        }
    }
    pub fn frame(&mut self, start: Instant, gui: Instant, render: Instant) {
        if !self.enabled() { return; }
        let end = Instant::now();
        self.frame_with_end(start, gui, render, end);
    }
    // The production caller supplies the same existing clock samples; the
    // explicit endpoint also permits deterministic chronology tests.
    fn frame_with_end(&mut self, start: Instant, gui: Instant, render: Instant, end: Instant) {
        self.first_frame_start.get_or_insert(start);
        self.last_frame_work_end = Some(end);
        self.current.frame_work.calls += 1;
        self.current.frame_work.simulation_streaming_ms += (gui - start).as_secs_f64() * 1000.;
        self.current.frame_work.gui_ms += (render - gui).as_secs_f64() * 1000.;
        self.current.frame_work.render_ms += (end - render).as_secs_f64() * 1000.;
    }
    /// The first measured present arms the recorder and discards warm-up work.
    /// Subsequent boundaries are exactly the timestamps used for frame_seconds.
    pub fn present(&mut self, at: Instant, application_origin: Instant, seconds: f32,
        camera: [f64; 3], audio: crate::audio::ProfileStats) {
        self.application_origin.get_or_insert(application_origin);
        self.present_with_cpu(at, thread_cpu_100ns(), seconds, camera, audio);
    }
    fn present_with_cpu(&mut self, at: Instant, cpu: Option<u64>, seconds: f32,
        camera: [f64; 3], audio: crate::audio::ProfileStats) {
        let previous = self.previous_present.replace(at);
        let previous_cpu = std::mem::replace(&mut self.previous_cpu, cpu);
        let mut interval = std::mem::take(&mut self.current);
        let first_frame_start = self.first_frame_start.take();
        let last_frame_work_end = self.last_frame_work_end.take();
        let Some(previous) = previous else {
            self.first_measured_present = Some(at);
            return;
        };
        interval.sample_index = self.measured;
        interval.application_seconds = seconds;
        interval.begin_application_ms = self.application_origin
            .map(|origin| (previous - origin).as_secs_f64() * 1000.);
        interval.end_application_ms = self.application_origin
            .map(|origin| (at - origin).as_secs_f64() * 1000.);
        interval.wall_ms = (at - previous).as_secs_f64() * 1000.;
        interval.thread_cpu_ms = previous_cpu.zip(cpu).map(|(a, b)| b.saturating_sub(a) as f64 / 10_000.);
        // service(frame) and all service/audio subspans are already inside the
        // outer frame or external service spans. Never add them a second time.
        interval.covered_wall_ms = interval.frame_work.total()
            + interval.service_inclusive.user_event.total_ms
            + interval.service_inclusive.about_to_wait.total_ms;
        interval.outside_covered_wall_ms = (interval.wall_ms - interval.covered_wall_ms).max(0.);
        interval.camera = camera;
        interval.audio = audio;
        self.measured += 1;
        if interval.wall_ms < self.threshold.get() { return; }
        interval.frame_boundaries = first_frame_start.zip(last_frame_work_end)
            .map(|(first, last)| FrameBoundaries::new(previous, at, self.application_origin, first, last));
        self.slow += 1;
        if self.retained.len() < LIMIT {
            if self.retained.capacity() == 0 { self.retained.reserve_exact(LIMIT); }
            self.retained.push(interval);
        } else {
            let (index, shortest) = self.retained.iter().enumerate()
                .min_by(|(_, a), (_, b)| a.wall_ms.total_cmp(&b.wall_ms)).unwrap();
            if interval.wall_ms > shortest.wall_ms { self.retained[index] = interval; }
        }
    }
    pub fn report(&self) -> serde_json::Value {
        let mut intervals = self.retained.clone();
        intervals.sort_unstable_by(|a, b| b.wall_ms.total_cmp(&a.wall_ms));
        serde_json::json!({
            "measured_intervals": self.measured, "slow_intervals": self.slow,
            "retained_limit": LIMIT, "threshold_ms": self.threshold.get(), "intervals": intervals,
            "first_measured_present_application_ms": self.application_origin
                .zip(self.first_measured_present).map(|(origin, at)| (at-origin).as_secs_f64()*1000.),
            "clock_note": "begin/end_application_ms and first_measured_present_application_ms use the actual successful-present Instants relative to App.started, without extra clock reads. Add frame_seconds prefix sums to the first-present anchor to recover every measured interval. application_seconds is only the earlier render-context Stats.time sample, not a present endpoint. Null exact timestamps mean no application origin was supplied by a test fixture.",
            "frame_boundary_note": "frame_boundaries reuses App::frame's existing first Instant and Recorder::frame's existing end Instant. last_frame_work_end is the existing clock read on entering Recorder::frame after renderer.render returns, before recorder bookkeeping and later App::frame bookkeeping/screenshots and the successful-present timestamp; it is not App::frame return. For a single valid ordered attempt, before + first_to_last + after partitions present-to-present wall chronology. For multiple failed attempts, first_to_last includes inter-attempt gaps/events and is NOT summed frame_work. These spans overlap existing covered services/work and must never be added to covered_wall_ms; pre/post gaps are not equivalent to outside_covered_wall_ms or proof of OS/title/upload causes. Absolute endpoints use App.started; null absolute values mean missing/invalid origin. A null boundary object means no frame work endpoint was recorded in this interval; individual null durations mean inconsistent timestamp order, never clamped zero. Every successful boundary resets pending endpoints, including warmup arm and fast unretained intervals. No new clock reads or changed present/sample boundaries.",
            "chunk_phase_order": CHUNK_PHASE_NAMES,
            "upload_child_phase_order":crate::renderer::upload_capture::PHASE_NAMES,
            "upload_child_note":"Optional explicit upload diagnostic only. Nested children do not add coverage; InitialConstruct is inside EntryInclusive, BufferCreate inside BufferReplaceInclusive. This covers Renderer upload calls, not all visual-job validation/dispatch/drop work.",
            "chunk_phase_scope": "Streaming benchmark only. Each fixed-array slot is the calls/total-time delta around synchronous Game::update_chunks in this present interval; missing timers are zero. Lifetime maxima are never subtracted. Native, visual and stream subphases are nested within their outer phase, chunk_update_inclusive and frame_work.simulation_streaming_ms; do not add them to coverage. distant_update_inclusive is also inside frame_work. Failed-present attempts accumulate until the next successful boundary. Snapshot bookkeeping is excluded from update wall spans and remains in frame_work.",
            "note": "Same boundaries and zero-based sample_index as frame_seconds, armed after warm-up. Thread CPU is Windows kernel+user time across the entire interval, may be quantized, and is null if unavailable. covered_wall_ms = frame_work + user_event/about_to_wait service totals; other service/audio timings are nested and must not be added. outside_covered_wall_ms includes event-loop scheduling, uninstrumented event callbacks, and profiling/report overhead; it is not proof of OS preemption. frame_work also includes unsuccessful render attempts. No worker execution time is charged as main-thread work. Audio play_ms includes bank/read/decoder/sink work; read_bytes counts resource bytes returned, not physical disk I/O. Disabled outside benchmark intervals; no per-frame disk I/O."
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
    // A borrowed current-thread pseudo-handle must not be closed.
    let ok = unsafe { GetThreadTimes(GetCurrentThread(), &mut creation, &mut exit, &mut kernel, &mut user) };
    (ok != 0).then(|| (u64::from(kernel.high) << 32 | u64::from(kernel.low))
        + (u64::from(user.high) << 32 | u64::from(user.low)))
}
#[cfg(not(windows))]
fn thread_cpu_100ns() -> Option<u64> { None }

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn configured_interval_threshold_preserves_exact_boundaries_and_bounded_correlation() {
        let mut recorder = Recorder::with_threshold("4".parse().unwrap());
        assert!(recorder.timer().is_none()); recorder.stage_ms(Stage::Copy, 999.);
        let origin = Instant::now(); let mut at = origin + Duration::from_secs(3);
        recorder.application_origin = Some(origin);
        recorder.present_with_cpu(at, None, 2.5, [0.; 3], Default::default());
        for microseconds in [3999, 4000, 4001] {
            // Multiple failed/unpresented attempts are still one interval:
            // only the eventual successful-present timestamp ends coverage.
            recorder.current.frame_work.calls = 2;
            recorder.current.frame_work.render_ms = microseconds as f64 / 2000.;
            at += Duration::from_micros(microseconds);
            recorder.present_with_cpu(at, None, 0., [0.; 3], Default::default());
        }
        assert_eq!((recorder.measured, recorder.slow), (3, 2));
        assert_eq!(recorder.retained[0].sample_index, 1);
        assert_eq!(recorder.retained[0].wall_ms, 4.);
        assert!((recorder.retained[0].begin_application_ms.unwrap() - 3003.999).abs() < 1e-9);
        assert!((recorder.retained[0].end_application_ms.unwrap() - 3007.999).abs() < 1e-9);
        assert_eq!(recorder.retained[0].frame_work.calls, 2);
        assert_eq!(recorder.retained[0].service_stages.presentation_copy.calls, 0, "warmup stays excluded");
        for duration in 4..84 {
            at += Duration::from_millis(duration);
            recorder.stage_ms(Stage::Accept, duration as f64 / 2.);
            recorder.present_with_cpu(at, None, duration as f32, [0.; 3], Default::default());
        }
        let report = recorder.report();
        assert_eq!(report["threshold_ms"], 4.);
        assert_eq!(report["first_measured_present_application_ms"], 3000.);
        assert_eq!(report["measured_intervals"], 83); assert_eq!(report["slow_intervals"], 82);
        assert_eq!(recorder.retained.len(), LIMIT);
        assert_eq!(report["intervals"][0]["wall_ms"], 83.);
        assert_eq!(report["intervals"][0]["sample_index"], 82);
        assert_eq!(report["intervals"][31]["wall_ms"], 52.);
        assert_eq!(report["intervals"][31]["service_stages"]["accept_inclusive"]["total_ms"], 26.);
    }
    #[test]
    fn exact_present_clock_is_independent_of_render_context_and_unretained_frames() {
        let mut r = Recorder::default();
        let origin = Instant::now();
        r.present(origin + Duration::from_millis(3000), origin, 2.6, [0.;3], Default::default());
        r.present(origin + Duration::from_millis(3012), origin, 2.8, [0.;3], Default::default());
        // A fast interval is absent from the retained set but remains part of
        // frame_seconds and the absolute timestamp of subsequent intervals.
        r.present(origin + Duration::from_millis(3017), origin, 2.9, [0.;3], Default::default());
        r.present(origin + Duration::from_millis(3034), origin, 3.0, [0.;3], Default::default());
        let report = r.report();
        assert_eq!(report["first_measured_present_application_ms"], 3000.);
        assert_eq!(report["measured_intervals"], 3);
        assert_eq!(report["intervals"].as_array().unwrap().len(), 2);
        let longest = &r.retained[1];
        assert_eq!((longest.sample_index, longest.application_seconds), (2, 3.0));
        assert_eq!(longest.begin_application_ms, Some(3017.));
        assert_eq!(longest.end_application_ms, Some(3034.));
        assert_eq!(longest.wall_ms, 17.);
        assert_eq!(r.retained[0].begin_application_ms, Some(3000.));
        assert_eq!(r.retained[0].end_application_ms, Some(3012.));
        assert_eq!(3000. + [0.012, 0.005, 0.017].iter().sum::<f64>()*1000., 3034.);
    }
    #[test]
    fn interval_boundaries_exclude_warmup_and_preserve_callback_audio_correlation() {
        let mut r = Recorder::default();
        assert!(r.timer().is_none());
        r.stage_ms(Stage::Copy, 999.);
        let start = Instant::now();
        r.present_with_cpu(start, Some(1_000), 1., [0.; 3], Default::default());
        assert_eq!(r.measured, 0);
        r.current.frame_work.simulation_streaming_ms = 2.;
        r.current.frame_work.render_ms = 1.;
        r.current.service_inclusive.frame.add(1.); // nested, not extra coverage
        r.current.service_inclusive.user_event.add(4.);
        r.stage_ms(Stage::Accept, 3.);
        r.stage_ms(Stage::Copy, 0.5);
        let audio = crate::audio::ProfileStats { play_calls: 2, play_ms: 2., ..Default::default() };
        r.present_with_cpu(start + Duration::from_millis(12), Some(51_000), 1.012, [1., 2., 3.], audio);
        let i = &r.retained[0];
        assert_eq!((i.sample_index, i.wall_ms, i.thread_cpu_ms), (0, 12., Some(5.)));
        assert_eq!((i.covered_wall_ms, i.outside_covered_wall_ms), (7., 5.));
        assert_eq!((i.audio.play_calls, i.service_stages.presentation_copy.total_ms), (2, 0.5));
        r.present_with_cpu(start + Duration::from_millis(24), None, 1.024, [0.; 3], Default::default());
        assert_eq!(r.retained[1].covered_wall_ms, 0., "completed interval counters must reset");
        assert_eq!(r.retained[1].audio.play_calls, 0);
        assert_eq!(r.retained[1].thread_cpu_ms, None);
    }
    #[test]
    fn interval_records_are_bounded_and_index_every_measured_present() {
        let mut r = Recorder::default();
        let mut at = Instant::now();
        r.present_with_cpu(at, None, 0., [0.; 3], Default::default());
        for duration in 1..=100 {
            at += Duration::from_millis(duration);
            r.stage_ms(Stage::Accept, duration as f64 / 2.);
            r.present_with_cpu(at, None, duration as f32, [0.; 3], Default::default());
        }
        let report = r.report();
        assert_eq!(report["measured_intervals"], 100);
        assert_eq!(report["slow_intervals"], 93);
        assert_eq!(report["intervals"].as_array().unwrap().len(), LIMIT);
        assert_eq!(report["intervals"][0]["sample_index"], 99);
        assert_eq!(report["intervals"][0]["service_stages"]["accept_inclusive"]["total_ms"], 50.);
        assert_eq!(report["intervals"][31]["wall_ms"], 69.);
    }
}


#[cfg(test)]
mod chunk_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn chunk_deltas_exclude_lifetime_history_and_maxima_with_missing_phases_zero() {
        let before = ChunkTimings::from([("visual_gpu_upload", [500., 2000., 900.])]);
        let after = ChunkTimings::from([
            ("visual_gpu_upload", [503., 2028., 900.]),
            ("native_submit", [1., 0.25, 0.25]),
            ("local_tick_worker", [9., 10000., 9000.]), // outside this scope
        ]);
        let mut r = Recorder::default();
        assert!(r.begin_chunk_update(&before).is_none());
        r.chunk_update_ms(ChunkSample::capture(&before),ChunkSample::capture(&after),99.);
        let start=Instant::now();
        r.present_with_cpu(start,None,0.,[0.;3],Default::default());
        assert_eq!(r.current.chunk_update_inclusive.calls,0,"warmup must not enter measured counters");
        r.chunk_update_ms(ChunkSample::capture(&before),ChunkSample::capture(&after),29.);
        r.present_with_cpu(start+Duration::from_millis(32),None,0.032,[1.;3],Default::default());
        let upload=CHUNK_PHASE_NAMES.iter().position(|name|*name=="visual_gpu_upload").unwrap();
        let submit=CHUNK_PHASE_NAMES.iter().position(|name|*name=="native_submit").unwrap();
        let interval=&r.retained[0];
        assert_eq!((interval.chunk_update_inclusive.calls,interval.chunk_update_inclusive.total_ms),(1,29.));
        assert_eq!((interval.chunk_phase_calls[upload],interval.chunk_phase_total_ms[upload]),(3,28.));
        assert_eq!((interval.chunk_phase_calls[submit],interval.chunk_phase_total_ms[submit]),(1,0.25));
        assert_eq!(interval.chunk_phase_calls.iter().sum::<u64>(),4);
        assert_eq!(interval.chunk_phase_total_ms.iter().sum::<f64>(),28.25);
        assert_eq!(interval.covered_wall_ms,0.,"nested diagnostics must not add frame coverage");
    }

    #[test]
    fn stream_and_light_children_correlate_to_present_without_adding_nested_coverage() {
        let children = ["stream_trim_scan", "stream_lod_capture", "stream_world_unload",
            "stream_renderer_remove", "stream_request_prepare", "stream_request_submit", "stream_import_metadata",
            "visual_light_arc_publish", "visual_light_owner_return", "visual_light_dirty_merge"];
        let mut before = ChunkTimings::from([("stream_requests_and_imports", [9., 90., 9999.])]);
        let mut after = ChunkTimings::from([("stream_requests_and_imports", [10., 100., 9999.])]);
        for name in children {
            before.insert(name, [100., 1000., 9999.]);
            after.insert(name, [102., 1001., 9999.]);
        }
        let mut r=Recorder::default();let start=Instant::now();
        r.present_with_cpu(start,None,0.,[0.;3],Default::default());
        r.chunk_update_ms(ChunkSample::capture(&before),ChunkSample::capture(&after),11.);
        r.present_with_cpu(start+Duration::from_millis(12),None,0.012,[0.;3],Default::default());
        let interval=&r.retained[0];
        for name in children {
            let index=CHUNK_PHASE_NAMES.iter().position(|entry|*entry==name).unwrap();
            assert_eq!((interval.chunk_phase_calls[index],interval.chunk_phase_total_ms[index]),(2,1.));
        }
        let parent=CHUNK_PHASE_NAMES.iter().position(|name|*name=="stream_requests_and_imports").unwrap();
        assert_eq!((interval.chunk_phase_calls[parent],interval.chunk_phase_total_ms[parent]),(1,10.));
        assert_eq!(interval.chunk_update_inclusive.total_ms,11.);
        assert_eq!(interval.covered_wall_ms,0.,"children are not additional frame coverage");
    }

    #[test]
    fn chunk_work_survives_failed_present_attempts_and_resets_at_successful_boundaries() {
        let mut r=Recorder::default(); let start=Instant::now();
        r.present_with_cpu(start,None,0.,[0.;3],Default::default());
        let a=ChunkTimings::new();
        let b=ChunkTimings::from([("native_publication",[1.,4.,4.])]);
        let c=ChunkTimings::from([("native_publication",[2.,7.,4.])]);
        r.chunk_update_ms(ChunkSample::capture(&a),ChunkSample::capture(&b),5.);
        // A timeout/lost surface supplies no successful-present boundary.
        r.chunk_update_ms(ChunkSample::capture(&b),ChunkSample::capture(&c),4.);
        r.current.distant_update_inclusive.add(1.);
        r.present_with_cpu(start+Duration::from_millis(12),None,0.012,[0.;3],Default::default());
        assert_eq!((r.retained[0].chunk_update_inclusive.calls,r.retained[0].chunk_update_inclusive.total_ms),(2,9.));
        assert_eq!((r.retained[0].chunk_phase_calls[0],r.retained[0].chunk_phase_total_ms[0]),(2,7.));
        assert_eq!(r.retained[0].distant_update_inclusive.total_ms,1.);
        r.present_with_cpu(start+Duration::from_millis(24),None,0.024,[0.;3],Default::default());
        assert_eq!(r.retained[1].chunk_update_inclusive.calls,0);
        assert_eq!(r.retained[1].distant_update_inclusive.calls,0);
        assert!(r.retained[1].chunk_phase_calls.iter().all(|v|*v==0));
        assert!(r.retained[1].chunk_phase_total_ms.iter().all(|v|*v==0.));
    }
}

#[cfg(test)]
mod upload_child_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn optional_upload_children_preserve_primary_slots_and_present_boundaries() {
        let names=crate::renderer::upload_capture::PHASE_NAMES;
        assert_eq!(CHUNK_PHASE_NAMES.len(),32);
        assert!(names.iter().all(|name|!CHUNK_PHASE_NAMES.contains(name)));
        let mut before=ChunkTimings::from([("visual_gpu_upload",[40.,200.,9000.])]);
        let mut after=ChunkTimings::from([("visual_gpu_upload",[41.,210.,9000.])]);
        for (i,name) in names.into_iter().enumerate() {
            before.insert(name,[100.,1000.,9000.]);
            after.insert(name,[101.+i as f64,1000.+(i+1) as f64*0.25,9000.]);
        }
        let mut r=Recorder::default();
        assert!(r.begin_chunk_update(&before).is_none());
        r.chunk_update_ms(ChunkSample::capture(&before),ChunkSample::capture(&after),999.);
        let at=Instant::now();
        r.present_with_cpu(at,None,0.,[0.;3],Default::default());
        assert!(r.current.upload_child_calls.is_none());
        assert!(r.current.upload_child_total_ms.is_none());
        // Two attempts with no successful present between them must accumulate.
        r.chunk_update_ms(ChunkSample::capture(&before),ChunkSample::capture(&after),11.);
        r.chunk_update_ms(ChunkSample::capture(&before),ChunkSample::capture(&after),11.);
        r.current.frame_work.simulation_streaming_ms=22.;
        r.current.frame_work.gui_ms=1.;
        r.current.frame_work.render_ms=2.;
        r.present_with_cpu(at+Duration::from_millis(30),None,0.030,[0.;3],Default::default());
        let interval=&r.retained[0];
        assert_eq!(interval.sample_index,0);
        assert_eq!((interval.chunk_update_inclusive.calls,interval.chunk_update_inclusive.total_ms),(2,22.));
        let primary=CHUNK_PHASE_NAMES.iter().position(|name|*name=="visual_gpu_upload").unwrap();
        assert_eq!((interval.chunk_phase_calls[primary],interval.chunk_phase_total_ms[primary]),(2,20.));
        assert_eq!(interval.chunk_phase_calls.iter().sum::<u64>(),2);
        assert_eq!(interval.chunk_phase_total_ms.iter().sum::<f64>(),20.);
        assert_eq!(interval.upload_child_calls.unwrap(),std::array::from_fn(|i|2*(i+1) as u64));
        assert_eq!(interval.upload_child_total_ms.unwrap(),std::array::from_fn(|i|(i+1) as f64*0.5));
        assert_eq!((interval.covered_wall_ms,interval.outside_covered_wall_ms),(25.,5.),
            "nested upload children cannot be added to the original frame coverage");
        let saved=serde_json::to_value(interval).unwrap();
        assert_eq!(saved["upload_child_calls"].as_array().unwrap().len(),10);
        assert_eq!(saved["chunk_phase_calls"].as_array().unwrap().len(),32);
        assert_eq!(r.report()["upload_child_phase_order"],serde_json::json!(names));

        // Without the preseed marker, even a similarly named legacy timer is
        // not an enabled capture; completed intervals leave no stale arrays.
        let empty=ChunkTimings::new();
        let partial=ChunkTimings::from([(names[1],[1.,99.,9999.])]);
        assert!(ChunkSample::capture(&partial).1.is_none());
        r.chunk_update_ms(ChunkSample::capture(&empty),ChunkSample::capture(&partial),1.);
        r.present_with_cpu(at+Duration::from_millis(42),None,0.042,[0.;3],Default::default());
        let interval=&r.retained[1];
        assert_eq!(interval.sample_index,1);
        assert!(interval.upload_child_calls.is_none());
        assert!(interval.upload_child_total_ms.is_none());
        assert!(interval.chunk_phase_calls.iter().all(|value|*value==0));
        assert_eq!(interval.covered_wall_ms,0.);
        let saved=serde_json::to_value(interval).unwrap();
        assert!(saved.get("upload_child_calls").is_none());
        assert!(saved.get("upload_child_total_ms").is_none());

        // Decreasing or restarted lifetime counters do not subtract prior
        // work or import lifetime maxima into the current present interval.
        let zero:ChunkTimings=names.into_iter().map(|name|(name,[0.,0.,1e30])).collect();
        r.chunk_update_ms(ChunkSample::capture(&after),ChunkSample::capture(&zero),1.);
        r.present_with_cpu(at+Duration::from_millis(54),None,0.054,[0.;3],Default::default());
        assert_eq!(r.retained[2].upload_child_calls,Some([0;10]));
        assert_eq!(r.retained[2].upload_child_total_ms,Some([0.;10]));
        assert_eq!(r.retained[2].chunk_phase_calls,[0;32]);
    }
}

#[cfg(test)]
#[path="client_interval_boundary_tests.rs"]
mod boundary_tests;
