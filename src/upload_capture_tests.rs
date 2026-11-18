use super::*;
fn observation(position:i32,ms:f64)->Observation {
    Observation{position:[position,-position],configuration_elapsed_ms:position as f64,
        total_ms:ms,..Default::default()}
}
#[test]
fn disabled_capture_owns_nothing_and_upload_flag_requires_streaming_benchmark() {
    use clap::Parser;
    let mut capture=Capture::default();
    assert!(capture.state.is_none());assert!(capture.start((1,2)).is_none());
    capture.record(observation(1,5.));assert!(capture.take_last().is_none());
    assert_eq!(capture.report(),serde_json::json!({"enabled":false,"diagnostic_not_baseline":false}));
    assert!(!std::mem::needs_drop::<Observation>());
    assert!(!std::mem::needs_drop::<Buffer>());
    let defaults=crate::Options::try_parse_from(["ferrum"]).unwrap();
    assert!(!defaults.benchmark_capture_uploads);
    assert!(crate::Options::try_parse_from(["ferrum","--benchmark-capture-uploads"]).is_err());
    assert!(crate::Options::try_parse_from(["ferrum","--benchmark","--benchmark-capture-uploads"]).is_err());
    assert!(crate::Options::try_parse_from(["ferrum","--benchmark-streaming","--benchmark-capture-uploads"]).is_err());
    let options=crate::Options::try_parse_from(["ferrum","--benchmark","--benchmark-streaming","--benchmark-capture-uploads"]).unwrap();
    assert!(options.benchmark_capture_uploads);
}
#[test]
fn top_eight_matches_independent_stable_sort_and_last_is_consumed_once() {
    let mut capture=Capture::new(true);let address=capture.state.as_deref().unwrap() as *const State;
    let durations=[2.,5.,5.,1.,9.,8.,7.,3.,6.,9.,4.,10.,10.,0.,8.,20.,20.,19.,20.,19.];
    for (i,&ms) in durations.iter().enumerate() {
        let mut o=observation(i as i32,ms);o.sequence=9999;capture.record(o);
        let last=capture.take_last().unwrap();assert_eq!(last.sequence,i as u64+1);
        assert_eq!(last.position,[i as i32,-(i as i32)]);assert!(capture.take_last().is_none());
        let mut expected:Vec<_>=durations[..=i].iter().copied().enumerate().collect();
        expected.sort_by(|a,b|b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));expected.truncate(8);
        let state=capture.state.as_deref().unwrap();
        let actual:Vec<_>=state.slowest.iter().flatten().map(|o|(o.sequence as usize-1,o.total_ms)).collect();
        assert_eq!(actual,expected);assert_eq!(state.slowest.len(),8);
        assert_eq!(state as *const State,address);
        assert_eq!(state.observations,i as u64+1);
    }
    let report=capture.report();assert_eq!(report["slowest_limit"],8);
    assert_eq!(report["observations"],durations.len());
    assert_eq!(report["total"]["total_ms"],durations.iter().sum::<f64>());
    assert_eq!(report["total"]["max_ms"],20.);
    assert_eq!(report["diagnostic_not_baseline"],true);
}
#[test]
fn invalid_observations_cannot_enter_rankings_totals_or_stale_last_record() {
    let mut capture=Capture::new(true);capture.record(observation(1,10.));
    let mut bad=Vec::new();
    for ms in [f64::NAN,f64::INFINITY,f64::NEG_INFINITY,-1.] {bad.push(observation(2,ms));}
    let mut o=observation(3,5.);o.configuration_elapsed_ms=f64::NAN;bad.push(o);
    let mut o=observation(4,5.);o.phases[Phase::Write as usize].total_ms=f64::NAN;bad.push(o);
    let mut o=observation(5,5.);o.phases[Phase::Replace as usize].max_ms=f64::INFINITY;bad.push(o);
    let mut o=observation(6,5.);o.phases[Phase::Entry as usize].total_ms=-1.;bad.push(o);
    for (i,o) in bad.into_iter().enumerate() {
        capture.record(o);assert!(capture.take_last().is_none());
        let state=capture.state.as_ref().unwrap();
        assert_eq!(state.invalid_observations,i as u64+1);assert_eq!(state.observations,1);
        assert_eq!(state.total.total_ms,10.);assert_eq!(state.slowest.iter().flatten().count(),1);
    }
    capture.record(observation(7,0.));let last=capture.take_last().unwrap();
    assert_eq!(last.sequence,2);assert_eq!(last.total_ms,0.);
}
#[test]
fn aggregate_coverage_excludes_nested_construct_and_create_but_keeps_raw_spans() {
    let mut o=observation(1,60.);o.vacant=true;
    for (phase,ms) in [(Phase::Hooks,1.),(Phase::CacheScan,2.),(Phase::BundleDrop,3.),
        (Phase::Entry,10.),(Phase::Initial,4.),(Phase::CpuReplace,5.),
        (Phase::Create,3.),(Phase::Replace,7.),(Phase::Write,11.),(Phase::Cleanup,13.)] {
        o.phases[phase as usize]=Span{calls:1,total_ms:ms,max_ms:ms};
    }
    o.buffers[0]=Buffer{old_capacity:256,new_capacity:4096,submitted_bytes:1024,creates:1,writes:1,
        create_ms:3.,replace_inclusive_ms:7.,write_ms:6.};
    o.buffers[1]=Buffer{old_capacity:256,new_capacity:256,submitted_bytes:12,writes:1,write_ms:5.,..Default::default()};
    assert_eq!(o.exclusive_covered_ms(),52.);
    let mut capture=Capture::new(true);capture.record(o);o.vacant=false;o.total_ms=70.;capture.record(o);
    let report=capture.report();
    assert_eq!(report["exclusive_covered_ms"],104.);assert_eq!(report["unattributed_ms"],26.);
    assert_eq!(report["phases"][Phase::Initial as usize]["total_ms"],8.);
    assert_eq!(report["phases"][Phase::Create as usize]["total_ms"],6.);
    assert_eq!(report["phases"][Phase::Replace as usize]["calls"],2);
    assert_eq!(report["phases"][Phase::Replace as usize]["max_ms"],7.);
    assert_eq!(report["vacant"],1);assert_eq!(report["occupied"],1);
    assert_eq!(report["creates_vertex_index"],serde_json::json!([2,0]));
    assert_eq!(report["writes_vertex_index"],serde_json::json!([2,2]));
    assert_eq!(report["submitted_bytes_vertex_index"],serde_json::json!([2048,24]));
    let mut trace=capture.start((8,-9)).unwrap();trace.body_finished();trace.returned();trace.returned();
    let finished=trace.finish();assert_eq!(finished.position,[8,-9]);
    assert_eq!(finished.phases[Phase::Cleanup as usize].calls,1);
    assert!(finished.total_ms.is_finite()&&finished.total_ms>=0.);
}

#[test]
fn application_origin_anchor_correlates_upload_start_without_extra_clock_reads() {
    let origin=Instant::now();let mut capture=Capture::new(true);
    assert!(capture.report()["configuration_application_elapsed_ms"].is_null());
    capture.application_origin(Some(origin));
    let trace=capture.start((1,2)).unwrap();
    let expected=trace.started.duration_since(origin).as_secs_f64()*1000.;
    let record=trace.finish();
    let anchored=capture.report()["configuration_application_elapsed_ms"].as_f64().unwrap()+record.configuration_elapsed_ms;
    assert!((anchored-expected).abs()<1e-7);
    capture.record(record);assert_eq!(capture.take_last().unwrap().position,[1,2]);
    let mut disabled=Capture::default();disabled.application_origin(Some(origin));assert!(disabled.state.is_none());
}
