//! Synthetic timestamps test chronology independently of wall-clock scheduling.
use super::*;
use std::time::Duration;

fn at(origin: Instant, ms: u64) -> Instant { origin + Duration::from_millis(ms) }
fn close(r: &mut Recorder, origin: Instant, ms: u64) {
    r.present_with_cpu(at(origin, ms), None, -99., [7.; 3], Default::default());
}
fn work(r: &mut Recorder, origin: Instant, stamps: [u64; 4]) {
    r.frame_with_end(at(origin, stamps[0]), at(origin, stamps[1]),
        at(origin, stamps[2]), at(origin, stamps[3]));
}
fn ms(value: Option<f64>) -> f64 { value.expect("expected a valid exact duration") }
fn equal(actual: f64, expected: f64) { assert!((actual-expected).abs() < 1e-9, "{actual} != {expected}"); }

#[test]
fn exact_frame_work_boundaries_partition_chronology_without_adding_coverage() {
    let origin = Instant::now(); let mut r = Recorder::default();
    r.application_origin = Some(origin); close(&mut r, origin, 1000);
    work(&mut r, origin, [1002, 1004, 1005, 1008]);
    // This service lies in the pre-frame gap, yet is already covered work.
    r.current.service_inclusive.user_event.add(1.);
    close(&mut r, origin, 1012);
    let i = &r.retained[0]; let b = i.frame_boundaries.as_ref().unwrap();
    equal(ms(b.first_frame_start_application_ms), 1002.);
    equal(ms(b.last_frame_work_end_application_ms), 1008.);
    assert_eq!((b.before_first_frame_ms, b.first_to_last_work_span_ms, b.after_last_frame_work_ms),
        (Some(2.), Some(6.), Some(4.)));
    equal(ms(b.before_first_frame_ms)+ms(b.first_to_last_work_span_ms)+ms(b.after_last_frame_work_ms), i.wall_ms);
    assert_eq!((i.frame_work.calls, i.frame_work.simulation_streaming_ms,
        i.frame_work.gui_ms, i.frame_work.render_ms), (1, 2., 1., 3.));
    assert_eq!((i.covered_wall_ms, i.outside_covered_wall_ms), (7., 5.));
    assert_ne!(ms(b.before_first_frame_ms)+ms(b.after_last_frame_work_ms), i.outside_covered_wall_ms);
    assert_eq!(i.application_seconds, -99., "context time is not used for exact endpoints");
    assert_eq!(i.camera, [7.; 3]);
}

#[test]
fn failed_present_attempts_keep_first_start_last_work_end_and_inclusive_middle() {
    let origin = Instant::now(); let mut r = Recorder::default();
    r.application_origin = Some(origin); close(&mut r, origin, 1000);
    work(&mut r, origin, [1001, 1002, 1003, 1005]); // failed surface attempt
    r.current.service_inclusive.about_to_wait.add(2.); // inside inter-attempt gap
    work(&mut r, origin, [1010, 1012, 1013, 1016]); // eventual successful attempt
    close(&mut r, origin, 1020);
    let i = &r.retained[0]; let b = i.frame_boundaries.as_ref().unwrap();
    assert_eq!((i.frame_work.calls, i.frame_work.total()), (2, 10.));
    assert_eq!((b.before_first_frame_ms, b.first_to_last_work_span_ms, b.after_last_frame_work_ms),
        (Some(1.), Some(15.), Some(4.)));
    assert_eq!((i.covered_wall_ms, i.outside_covered_wall_ms), (12., 8.));
    assert!(ms(b.first_to_last_work_span_ms) > i.frame_work.total(), "middle contains inter-attempt events/gaps");
    equal(ms(b.first_frame_start_application_ms), 1001.);
    equal(ms(b.last_frame_work_end_application_ms), 1016.);
    // A later synthetic present with no frame must not reuse the old bounds.
    close(&mut r, origin, 1029);
    assert_eq!(r.retained[1].frame_work.calls, 0);
    assert!(r.retained[1].frame_boundaries.is_none());
    assert!(r.first_frame_start.is_none() && r.last_frame_work_end.is_none());
}

#[test]
fn disabled_warmup_and_first_present_discard_pending_boundaries() {
    let origin = Instant::now(); let mut r = Recorder::default();
    // Public disabled path returns before its existing Instant::now read.
    r.frame(at(origin, 1), at(origin, 2), at(origin, 3));
    assert_eq!(r.current.frame_work.calls, 0);
    assert!(r.first_frame_start.is_none() && r.last_frame_work_end.is_none());
    // Exercise arm/reset defensively even if stale work was supplied by a fixture.
    r.first_frame_start = Some(at(origin, 1));
    r.last_frame_work_end = Some(at(origin, 4));
    r.current.frame_work.calls = 1;
    r.application_origin = Some(origin); close(&mut r, origin, 1000);
    assert_eq!((r.measured, r.slow, r.current.frame_work.calls), (0, 0, 0));
    assert!(r.first_frame_start.is_none() && r.last_frame_work_end.is_none());
    assert_eq!(r.report()["first_measured_present_application_ms"], 1000.);
    close(&mut r, origin, 1010);
    assert!(r.retained[0].frame_boundaries.is_none());
}

#[test]
fn fast_intervals_reset_bounds_and_retained_prefix_sums_stay_exact_and_bounded() {
    let origin = Instant::now(); let mut r = Recorder::default();
    r.application_origin = Some(origin); close(&mut r, origin, 3000);
    let durations: Vec<u64> = [12, 5, 17].into_iter().chain(9..=48).collect();
    let mut expected = Vec::new(); let mut previous = 3000;
    for &duration in &durations {
        let next = previous + duration;
        work(&mut r, origin, [previous+1, previous+2, previous+3, next-1]);
        close(&mut r, origin, next);
        expected.push((previous, next)); previous = next;
        assert!(r.first_frame_start.is_none() && r.last_frame_work_end.is_none(),
            "reset also applies to the unretained5ms interval");
    }
    assert_eq!((r.measured, r.slow, r.retained.len()), (43, 42, LIMIT));
    assert_eq!(r.report()["retained_limit"], 32);
    assert_eq!(r.report()["threshold_ms"], 8.);
    for i in &r.retained {
        let (begin, end) = expected[i.sample_index as usize];
        let b = i.frame_boundaries.as_ref().unwrap();
        equal(ms(i.begin_application_ms), begin as f64); equal(ms(i.end_application_ms), end as f64);
        equal(i.wall_ms, durations[i.sample_index as usize] as f64);
        equal(ms(b.first_frame_start_application_ms), (begin+1) as f64);
        equal(ms(b.last_frame_work_end_application_ms), (end-1) as f64);
        equal(ms(b.before_first_frame_ms)+ms(b.first_to_last_work_span_ms)+ms(b.after_last_frame_work_ms), i.wall_ms);
        equal(3000. + durations[..i.sample_index as usize].iter().sum::<u64>() as f64, begin as f64);
    }
    let saved = r.report();
    assert_eq!(saved["intervals"][0]["wall_ms"], 48.);
    assert_eq!(saved["intervals"][0]["sample_index"], 42);
    assert_eq!(saved["intervals"][31]["wall_ms"], 17.);
}

#[test]
fn missing_origin_and_inconsistent_order_are_explicit_nulls_not_clamped_times() {
    let origin = Instant::now(); let mut r = Recorder::default();
    close(&mut r, origin, 1000); work(&mut r, origin, [1001, 1002, 1003, 1007]); close(&mut r, origin, 1010);
    let b = r.retained[0].frame_boundaries.as_ref().unwrap();
    assert!(b.first_frame_start_application_ms.is_none() && b.last_frame_work_end_application_ms.is_none());
    assert_eq!((b.before_first_frame_ms, b.first_to_last_work_span_ms, b.after_last_frame_work_ms),
        (Some(1.), Some(6.), Some(3.)));
    let invalid = FrameBoundaries::new(at(origin, 1010), at(origin, 1020), Some(at(origin, 1030)),
        at(origin, 1009), at(origin, 1021));
    assert!(invalid.first_frame_start_application_ms.is_none() && invalid.last_frame_work_end_application_ms.is_none());
    assert_eq!((invalid.before_first_frame_ms, invalid.first_to_last_work_span_ms, invalid.after_last_frame_work_ms),
        (None, Some(12.), None));
    let reversed = FrameBoundaries::new(at(origin, 1000), at(origin, 1020), Some(origin),
        at(origin, 1012), at(origin, 1011));
    assert!(reversed.first_to_last_work_span_ms.is_none());
    assert!(serde_json::to_value(invalid).unwrap()["before_first_frame_ms"].is_null());
}
