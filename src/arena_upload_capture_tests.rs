use super::*;
fn observation(position:i32,total_ms:f64)->Observation {
    Observation { position:[position,-position], configuration_elapsed_ms:position.max(0) as f64,
        total_ms, ..Default::default() }
}
#[test]
fn disabled_arena_capture_keeps_no_state_and_cli_is_an_independent_streaming_diagnostic() {
    use clap::Parser;
    let mut capture=Capture::new(false,None);
    assert!(capture.state.is_none());assert!(capture.start((7,-2)).is_none());
    capture.record(observation(3,7.));assert!(capture.state.is_none());
    assert_eq!(capture.report(),serde_json::json!({"enabled":false,"diagnostic_not_baseline":false}));
    assert!(!std::mem::needs_drop::<Observation>());
    assert!(!std::mem::needs_drop::<Span>());
    let defaults=crate::Options::try_parse_from(["ferrum"]).unwrap();
    assert!(!defaults.benchmark_capture_arena_uploads);
    for args in [vec!["ferrum","--benchmark-capture-arena-uploads"],
        vec!["ferrum","--benchmark","--benchmark-capture-arena-uploads"],
        vec!["ferrum","--benchmark-streaming","--benchmark-capture-arena-uploads"]] {
        assert!(crate::Options::try_parse_from(args).is_err());
    }
    let base=["ferrum","--benchmark","--benchmark-streaming","--benchmark-capture-arena-uploads"];
    let automatic=crate::Options::try_parse_from(base).unwrap();
    assert!(automatic.benchmark_capture_arena_uploads);
    assert!(automatic.benchmark_near_arena.is_none(),"diagnosis must not rewrite automatic selection");
    for mode in ["direct","mdi"] {
        let mut args=base.to_vec();args.extend(["--benchmark-near-arena",mode]);
        let parsed=crate::Options::try_parse_from(args).unwrap();
        assert!(parsed.benchmark_capture_arena_uploads);assert!(parsed.benchmark_near_arena.is_some());
    }
    for conflict in ["--benchmark-capture-uploads","--benchmark-capture-near-opaque",
        "--benchmark-near-opaque-runs","--benchmark-capture-transparency"] {
        let mut args=base.to_vec();args.push(conflict);
        assert!(crate::Options::try_parse_from(args).is_err(),"conflict {conflict}");
    }
}
#[test]
fn arena_top_eight_matches_independent_stable_sort_without_growing_retained_state() {
    let mut capture=Capture::new(true,None);
    let address=capture.state.as_deref().unwrap() as *const State;
    let durations=[0.,5.,5.,2.,9.,8.,3.,6.,9.,4.,10.,10.,1.,8.,20.,20.,19.,20.,19.,0.];
    for (i,&ms) in durations.iter().enumerate() {
        let mut o=observation(i as i32,ms);o.sequence=u64::MAX;capture.record(o);
        let mut expected:Vec<_>=durations[..=i].iter().copied().enumerate().collect();
        expected.sort_by(|a,b|b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));expected.truncate(8);
        let state=capture.state.as_deref().unwrap();
        let actual:Vec<_>=state.slowest.iter().flatten().map(|o|(o.sequence as usize-1,o.total_ms)).collect();
        assert_eq!(actual,expected);assert_eq!(state.slowest.len(),8);
        assert_eq!(state as *const State,address);assert_eq!(state.observations,i as u64+1);
        for o in state.slowest.iter().flatten() {assert_eq!(o.position,[o.sequence as i32-1,1-o.sequence as i32]);}
    }
    let report=capture.report();assert_eq!(report["slowest_limit"],8);
    assert_eq!(report["observations"],durations.len());
    assert_eq!(report["total"]["total_ms"],durations.iter().sum::<f64>());
    assert_eq!(report["total"]["max_ms"],20.);assert_eq!(report["diagnostic_not_baseline"],true);
}
#[test]
fn invalid_arena_records_cannot_poison_rankings_or_aggregate_coverage() {
    let mut capture=Capture::new(true,None);capture.record(observation(1,7.));
    let mut invalid=Vec::new();
    for total in [f64::NAN,f64::INFINITY,f64::NEG_INFINITY,-1.] {invalid.push(observation(2,total));}
    for at in 0..NAMES.len() {
        let mut o=observation(3,8.);o.phases[at].total_ms=f64::NAN;invalid.push(o);
        let mut o=observation(3,8.);o.phases[at].max_ms=f64::INFINITY;invalid.push(o);
    }
    let mut o=observation(4,8.);o.configuration_elapsed_ms=-1.;invalid.push(o);
    let mut o=observation(4,8.);o.configuration_elapsed_ms=f64::NAN;invalid.push(o);
    let mut o=observation(4,8.);
    o.phases[Phase::Mutex as usize].total_ms=f64::MAX;
    o.phases[Phase::Cache as usize].total_ms=f64::MAX;
    assert!(!o.covered_ms().is_finite());invalid.push(o);
    for (i,o) in invalid.into_iter().enumerate() {
        capture.record(o);let s=capture.state.as_ref().unwrap();
        assert_eq!(s.invalid,i as u64+1);assert_eq!(s.observations,1);
        assert_eq!(s.total.total_ms,7.);assert_eq!(s.slowest.iter().flatten().count(),1);
        assert_eq!(s.covered_ms,0.);
    }
    capture.record(observation(5,0.));assert_eq!(capture.state.as_ref().unwrap().observations,2);
}
#[test]
fn arena_coverage_excludes_nested_page_calls_but_keeps_raw_phase_and_page_totals() {
    let mut o=observation(1,120.);o.new_shared=true;
    for (p,ms) in [(Phase::Mutex,1.),(Phase::Cache,2.),(Phase::Admission,3.),(Phase::Fallback,4.),
        (Phase::SharedWrite,5.),(Phase::Commit,6.),(Phase::CpuReplace,7.),(Phase::GpuReplace,8.),
        (Phase::NewMetadata,9.),(Phase::Cleanup,10.),(Phase::Release,11.),(Phase::Control,12.),(Phase::MapInsert,13.),
        (Phase::Page,50.),(Phase::Create,30.),(Phase::Resolve,10.),(Phase::VertexWrite,4.),(Phase::IndexWrite,2.)] {
        o.phases[p as usize]=Span{calls:1,total_ms:ms,max_ms:ms};
    }
    assert_eq!(o.covered_ms(),91.);
    o.page_attempts=[1,2,3];o.page_attempted_bytes=[256,128,64];
    let mut capture=Capture::new(true,None);capture.record(o);
    o.total_ms=140.;o.new_shared=false;o.explicit_dedicated=true;o.fallback=true;capture.record(o);
    let report=capture.report();assert_eq!(report["exclusive_covered_ms"],182.);
    assert_eq!(report["unattributed_ms"],78.);assert_eq!(report["shared_uploads"],1);
    assert_eq!(report["fallbacks"],1);assert_eq!(report["explicit_dedicated_controls"],1);
    assert_eq!(report["page_attempts_vertex_u16_u32"],serde_json::json!([2,4,6]));
    assert_eq!(report["page_attempted_bytes_vertex_u16_u32"],serde_json::json!([512,256,128]));
    assert_eq!(report["phases"][Phase::Page as usize]["total_ms"],100.);
    assert_eq!(report["phases"][Phase::Admission as usize]["total_ms"],6.);
    assert_eq!(report["phases"][Phase::Create as usize]["max_ms"],30.);
}
#[test]
fn arena_anchor_and_implicit_cleanup_have_exact_monotonic_boundaries() {
    let origin=Instant::now();let capture=Capture::new(true,Some(origin));
    let mut trace=capture.start((-7,12)).unwrap();
    let actual_start=trace.started.duration_since(origin).as_secs_f64()*1000.;
    trace.returned();assert_eq!(trace.observation.phases[Phase::Cleanup as usize].calls,0);
    trace.body_finished();trace.returned();trace.returned();
    assert_eq!(trace.observation.phases[Phase::Cleanup as usize].calls,1);
    let before_finish=Instant::now().duration_since(origin).as_secs_f64()*1000.;
    let o=trace.finish();let after_finish=Instant::now().duration_since(origin).as_secs_f64()*1000.;
    let start=capture.report()["configuration_application_elapsed_ms"].as_f64().unwrap()+o.configuration_elapsed_ms;
    assert!((start-actual_start).abs()<1e-7);let end=start+o.total_ms;
    assert!(end+1e-7>=before_finish&&end<=after_finish+1e-7);
    assert_eq!(o.position,[-7,12]);assert!(o.total_ms>=o.phases[Phase::Cleanup as usize].total_ms);
    let no_origin=Capture::new(true,None);assert!(no_origin.report()["configuration_application_elapsed_ms"].is_null());
    let future=Instant::now()+std::time::Duration::from_secs(60);
    let bad_origin=Capture::new(true,Some(future));assert!(bad_origin.report()["configuration_application_elapsed_ms"].is_null());
}
