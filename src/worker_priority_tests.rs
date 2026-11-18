use super::*;
use std::{cell::RefCell, collections::VecDeque};

struct Fake {
    supported: bool,
    queries: VecDeque<Result<i32, u32>>,
    set: Result<(), u32>,
    calls: RefCell<Vec<&'static str>>,
}
impl Fake {
    fn new(queries: impl IntoIterator<Item=Result<i32,u32>>, set: Result<(),u32>) -> Self {
        Self { supported:true, queries:queries.into_iter().collect(), set, calls:RefCell::new(Vec::new()) }
    }
    fn untouched(&self) { assert!(self.calls.borrow().is_empty()); }
}
impl PriorityApi for Fake {
    fn supported(&self)->bool { self.calls.borrow_mut().push("supported"); self.supported }
    fn query(&mut self)->Result<i32,u32> {
        self.calls.borrow_mut().push("query"); self.queries.pop_front().expect("unexpected priority query")
    }
    fn set_below_normal(&mut self)->Result<(),u32> { self.calls.borrow_mut().push("set"); self.set }
}

#[test]
fn priority_attempts_preserve_existing_priority_and_verify_actual_result() {
    let mut unsupported=Fake::new([],Ok(()));unsupported.supported=false;
    assert_eq!(apply(&mut unsupported),Attempt::new(Outcome::Unsupported));
    assert_eq!(*unsupported.calls.borrow(),["supported"]);
    let cases=[
        (vec![Err(0)],Ok(()),Attempt{outcome:Outcome::QueryFailed,before:None,after:None,error:Some(0)},vec!["supported","query"]),
        (vec![Ok(BELOW_NORMAL)],Ok(()),Attempt{outcome:Outcome::AlreadyBelowNormal,before:Some(BELOW_NORMAL),after:None,error:None},vec!["supported","query"]),
        (vec![Ok(-2)],Ok(()),Attempt{outcome:Outcome::SkippedExisting,before:Some(-2),after:None,error:None},vec!["supported","query"]),
        (vec![Ok(2)],Ok(()),Attempt{outcome:Outcome::SkippedExisting,before:Some(2),after:None,error:None},vec!["supported","query"]),
        (vec![Ok(NORMAL)],Err(5),Attempt{outcome:Outcome::SetFailed,before:Some(NORMAL),after:None,error:Some(5)},vec!["supported","query","set"]),
        (vec![Ok(NORMAL),Err(6)],Ok(()),Attempt{outcome:Outcome::VerifyFailed,before:Some(NORMAL),after:None,error:Some(6)},vec!["supported","query","set","query"]),
        (vec![Ok(NORMAL),Ok(NORMAL)],Ok(()),Attempt{outcome:Outcome::VerifyMismatch,before:Some(NORMAL),after:Some(NORMAL),error:None},vec!["supported","query","set","query"]),
        (vec![Ok(NORMAL),Ok(-2)],Ok(()),Attempt{outcome:Outcome::VerifyMismatch,before:Some(NORMAL),after:Some(-2),error:None},vec!["supported","query","set","query"]),
        (vec![Ok(NORMAL),Ok(BELOW_NORMAL)],Ok(()),Attempt{outcome:Outcome::Applied,before:Some(NORMAL),after:Some(BELOW_NORMAL),error:None},vec!["supported","query","set","query"]),
    ];
    for (queries,set,expected,calls) in cases {
        let mut api=Fake::new(queries,set);
        assert_eq!(apply(&mut api),expected);assert_eq!(*api.calls.borrow(),calls);
        assert!(api.queries.is_empty());
    }
}

#[test]
fn priority_selection_and_disabled_start_make_no_platform_calls() {
    let disabled=Diagnostics::new();
    for pool in Pool::ALL {
        let mut api=Fake::new([],Ok(()));
        disabled.start(None,pool,usize::MAX,&mut api);api.untouched();
        let report=disabled.report(None);
        assert_eq!(report["pools"][pool.name()]["status"],"disabled");
        assert_eq!(report["pools"][pool.name()]["observed_starts"],0);
        assert_eq!(report["pools"][pool.name()]["unexpected_or_repeated_start_indices"],0);
    }
    for mode in [Mode::Baseline,Mode::Visual,Mode::All] {
        let diag=Diagnostics::new();
        for pool in Pool::ALL {
            let selected=match mode {Mode::Baseline=>false,Mode::Visual=>pool!=Pool::NativeTerrain,Mode::All=>true};
            assert_eq!(mode.selects(pool),selected);
            let before=diag.report(Some(mode));
            assert_eq!(before["pools"][pool.name()]["status"],if selected {"not_observed"} else {"not_selected"});
            let mut api=Fake::new([Ok(NORMAL),Ok(BELOW_NORMAL)],Ok(()));
            diag.start(Some(mode),pool,0,&mut api);
            let report=diag.report(Some(mode));let data=&report["pools"][pool.name()];
            assert_eq!(data["requested"],selected);assert_eq!(data["observed_starts"],1);
            assert_eq!(data["set_attempts"],usize::from(selected));
            assert_eq!(data["status"],if selected {"verified_for_observed_workers"} else {"not_selected"});
            if !selected {api.untouched();assert_eq!(data["outcomes"]["unselected"],1);}
        }
    }
}

#[test]
fn priority_diagnostics_preserve_each_failure_and_never_report_pending_as_applied() {
    let diag=Diagnostics::new();let pool=Pool::NativeTerrain;let mode=Some(Mode::All);
    let attempts=[
        Attempt{outcome:Outcome::Applied,before:Some(0),after:Some(-1),error:None},
        Attempt{outcome:Outcome::AlreadyBelowNormal,before:Some(-1),after:None,error:None},
        Attempt{outcome:Outcome::QueryFailed,before:None,after:None,error:Some(0)},
        Attempt{outcome:Outcome::SetFailed,before:Some(0),after:None,error:Some(5)},
        Attempt{outcome:Outcome::VerifyFailed,before:Some(0),after:None,error:Some(87)},
        Attempt{outcome:Outcome::VerifyMismatch,before:Some(0),after:Some(0),error:None},
        Attempt{outcome:Outcome::SkippedExisting,before:Some(-2),after:None,error:None},
        Attempt::new(Outcome::Unsupported),
    ];
    for (index,attempt) in attempts.into_iter().enumerate() {
        let worker=&diag.pools[pool.index()].workers[index];
        worker.state.store(PENDING,Ordering::Release);
        // Even written-but-unpublished fields must not leak as a verified row.
        worker.before.store(0,Ordering::Relaxed);worker.after.store(-1,Ordering::Relaxed);
        let pending=diag.report(mode);let data=&pending["pools"][pool.name()];
        assert_eq!(data["status"],"startup_pending");assert_eq!(data["pending"],1);
        let row=&data["workers"][index];assert_eq!(row["status"],"startup_pending");
        assert!(row.get("before").is_none());assert!(row.get("after").is_none());
        worker.finish(attempt);assert_eq!(worker.read(),Some(Ok(attempt)));
    }
    let report=diag.report(mode);let data=&report["pools"][pool.name()];
    assert_eq!(data["status"],"partially_applied_or_unapplied");assert_eq!(data["observed_starts"],8);
    assert_eq!(data["pending"],0);assert_eq!(data["set_attempts"],4);
    for (index,attempt) in attempts.into_iter().enumerate() {
        let row=&data["workers"][index];assert_eq!(row["index"],index);assert_eq!(row["status"],attempt.outcome.name());
        assert_eq!(row["before"],serde_json::json!(attempt.before));assert_eq!(row["after"],serde_json::json!(attempt.after));
        assert_eq!(row["win32_error"],serde_json::json!(attempt.error));assert_eq!(data["outcomes"][attempt.outcome.name()],1);
    }
}

#[test]
fn priority_duplicate_indices_are_reported_without_applying_again() {
    let diag=Diagnostics::new();let mode=Some(Mode::All);let pool=Pool::VisualMesh;
    let mut first=Fake::new([Ok(NORMAL),Ok(BELOW_NORMAL)],Ok(()));diag.start(mode,pool,0,&mut first);
    let original=diag.pools[pool.index()].workers[0].read();
    for index in [0,WORKERS,usize::MAX] {
        let mut rejected=Fake::new([],Ok(()));diag.start(mode,pool,index,&mut rejected);rejected.untouched();
    }
    // A concurrent/reentrant start while the index is pending also cannot apply twice.
    diag.pools[pool.index()].workers[1].state.store(PENDING,Ordering::Release);
    let mut rejected=Fake::new([],Ok(()));diag.start(mode,pool,1,&mut rejected);rejected.untouched();
    let report=diag.report(mode);let data=&report["pools"][pool.name()];
    assert_eq!(data["status"],"unexpected_start_indices");
    assert_eq!(data["unexpected_or_repeated_start_indices"],4);assert_eq!(data["set_attempts"],1);
    assert_eq!(data["observed_starts"],2);assert_eq!(data["pending"],1);
    assert_eq!(diag.pools[pool.index()].workers[0].read(),original);
    assert_eq!(report["pools"][Pool::Distant.name()]["status"],"not_observed");
}

#[test]
fn priority_configuration_is_immutable_and_cli_is_benchmark_only() {
    use clap::Parser;
    let config=OnceLock::new();assert!(config.get().is_none());
    assert!(configure_into(&config,Mode::Visual).is_ok());assert!(configure_into(&config,Mode::Visual).is_ok());
    assert!(configure_into(&config,Mode::Baseline).is_err());assert!(configure_into(&config,Mode::All).is_err());
    assert_eq!(config.get(),Some(&Mode::Visual));
    assert_eq!(crate::Options::try_parse_from(["ferrum"]).unwrap().benchmark_worker_priority,None);
    for (text,mode) in [("baseline",Mode::Baseline),("visual",Mode::Visual),("all",Mode::All)] {
        assert!(crate::Options::try_parse_from(["ferrum","--benchmark-worker-priority",text]).is_err());
        assert_eq!(crate::Options::try_parse_from(["ferrum","--benchmark","--benchmark-worker-priority",text]).unwrap().benchmark_worker_priority,Some(mode));
    }
    assert!(crate::Options::try_parse_from(["ferrum","--benchmark","--benchmark-worker-priority","unknown"]).is_err());
}
