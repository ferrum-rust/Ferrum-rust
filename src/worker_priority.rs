//! Benchmark-only CPU priority for three existing private worker pools.
//! No process, I/O, affinity, pool-size, or foreground-thread changes.
use std::sync::{OnceLock, atomic::{AtomicI32, AtomicU8, AtomicU32, AtomicU64, Ordering}};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Mode { #[default] Baseline, Visual, All }
impl Mode {
    fn name(self) -> &'static str { match self { Self::Baseline=>"baseline", Self::Visual=>"visual", Self::All=>"all" } }
    fn selects(self, pool: Pool) -> bool { match self {
        Self::Baseline=>false, Self::Visual=>pool!=Pool::NativeTerrain, Self::All=>true,
    } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pool { VisualMesh, Distant, NativeTerrain }
impl Pool {
    const ALL: [Self;3] = [Self::VisualMesh,Self::Distant,Self::NativeTerrain];
    fn index(self) -> usize { match self { Self::VisualMesh=>0,Self::Distant=>1,Self::NativeTerrain=>2 } }
    fn name(self) -> &'static str { match self { Self::VisualMesh=>"visual_mesh",Self::Distant=>"distant_surface",Self::NativeTerrain=>"native_terrain" } }
}
const NORMAL: i32=0;
const BELOW_NORMAL: i32=-1;
const ERROR_RETURN: i32=i32::MAX;
const WORKERS: usize=8; // Existing pools are capped at 2, 2, and 8 workers.
const UNOBSERVED: u8=0;
const PENDING: u8=1;
static MODE: OnceLock<Mode> = OnceLock::new();
static DIAGNOSTICS: Diagnostics = Diagnostics::new();

// Persistent OnceLock pools must never be repurposed between arms in one process.
fn configure_into(config: &OnceLock<Mode>, mode: Mode) -> Result<(), &'static str> {
    let actual=*config.get_or_init(||mode);
    if actual==mode { Ok(()) } else { Err("Worker-priority arm cannot change within a process; run each arm in a fresh process") }
}
pub(crate) fn configure(mode: Mode) -> Result<(), &'static str> { configure_into(&MODE,mode) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Outcome { Unselected=2, Unsupported, QueryFailed, AlreadyBelowNormal, SkippedExisting, SetFailed, VerifyFailed, Applied, VerifyMismatch }
impl Outcome {
    const ALL:[Self;9]=[Self::Unselected,Self::Unsupported,Self::QueryFailed,Self::AlreadyBelowNormal,
        Self::SkippedExisting,Self::SetFailed,Self::VerifyFailed,Self::Applied,Self::VerifyMismatch];
    fn name(self)->&'static str { match self {
        Self::Unselected=>"unselected",Self::Unsupported=>"unsupported",Self::QueryFailed=>"query_failed",
        Self::AlreadyBelowNormal=>"already_below_normal",Self::SkippedExisting=>"skipped_existing_priority",
        Self::SetFailed=>"set_failed",Self::VerifyFailed=>"verification_query_failed",Self::Applied=>"verified_applied",
        Self::VerifyMismatch=>"verification_mismatch",
    } }
    fn attempted(self)->bool { matches!(self,Self::SetFailed|Self::VerifyFailed|Self::Applied|Self::VerifyMismatch) }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Attempt { outcome:Outcome,before:Option<i32>,after:Option<i32>,error:Option<u32> }
impl Attempt {
    fn new(outcome:Outcome)->Self { Self {outcome,before:None,after:None,error:None} }
}
trait PriorityApi {
    fn supported(&self)->bool;
    fn query(&mut self)->Result<i32,u32>;
    fn set_below_normal(&mut self)->Result<(),u32>;
}
fn apply(api:&mut impl PriorityApi)->Attempt {
    if !api.supported() { return Attempt::new(Outcome::Unsupported); }
    let before=match api.query() { Ok(value)=>value,Err(error)=>return Attempt {error:Some(error),..Attempt::new(Outcome::QueryFailed)} };
    if before==BELOW_NORMAL { return Attempt {before:Some(before),..Attempt::new(Outcome::AlreadyBelowNormal)}; }
    // Never raise a worker deliberately started at a different priority.
    if before!=NORMAL { return Attempt {before:Some(before),..Attempt::new(Outcome::SkippedExisting)}; }
    if let Err(error)=api.set_below_normal() { return Attempt {before:Some(before),error:Some(error),..Attempt::new(Outcome::SetFailed)}; }
    match api.query() {
        Err(error)=>Attempt {before:Some(before),error:Some(error),..Attempt::new(Outcome::VerifyFailed)},
        Ok(after)=>Attempt {before:Some(before),after:Some(after),error:None,
            outcome:if after==BELOW_NORMAL {Outcome::Applied} else {Outcome::VerifyMismatch}},
    }
}

// Fields are written once, then published by state.store(Release). The report
// reads state with Acquire before fields, so partial startup cannot masquerade
// as a verified result. No worker strings, locks, logging, or heap allocations.
struct Worker {
    state:AtomicU8,before:AtomicI32,after:AtomicI32,error:AtomicU32,
}
impl Worker {
    const fn new()->Self { Self {state:AtomicU8::new(UNOBSERVED),before:AtomicI32::new(ERROR_RETURN),
        after:AtomicI32::new(ERROR_RETURN),error:AtomicU32::new(0)} }
    fn finish(&self,attempt:Attempt) {
        self.before.store(attempt.before.unwrap_or(ERROR_RETURN),Ordering::Relaxed);
        self.after.store(attempt.after.unwrap_or(ERROR_RETURN),Ordering::Relaxed);
        self.error.store(attempt.error.unwrap_or(0),Ordering::Relaxed);
        self.state.store(attempt.outcome as u8,Ordering::Release);
    }
    fn read(&self)->Option<Result<Attempt,()>> {
        let state=self.state.load(Ordering::Acquire);
        if state==UNOBSERVED {return None;}
        let Some(outcome)=Outcome::ALL.into_iter().find(|v| *v as u8==state) else {return Some(Err(()));};
        let value=|field:&AtomicI32| {let v=field.load(Ordering::Relaxed);(v!=ERROR_RETURN).then_some(v)};
        Some(Ok(Attempt {outcome,before:value(&self.before),after:value(&self.after),
            error:matches!(outcome,Outcome::QueryFailed|Outcome::SetFailed|Outcome::VerifyFailed)
                .then(||self.error.load(Ordering::Relaxed))}))
    }
}
struct PoolDiagnostics { workers:[Worker;WORKERS], unexpected:AtomicU64 }
impl PoolDiagnostics {
    const fn new()->Self {Self {workers:[const {Worker::new()};WORKERS],unexpected:AtomicU64::new(0)}}
}
struct Diagnostics {pools:[PoolDiagnostics;3]}
impl Diagnostics {
    const fn new()->Self { Self {pools:[const {PoolDiagnostics::new()};3]} }
    fn start(&self,mode:Option<Mode>,pool:Pool,index:usize,api:&mut impl PriorityApi) {
        let Some(mode)=mode else {return;}; // Ordinary game/tests: no counters or OS calls.
        let data=&self.pools[pool.index()];
        let Some(worker)=data.workers.get(index) else {data.unexpected.fetch_add(1,Ordering::Relaxed);return;};
        if worker.state.compare_exchange(UNOBSERVED,PENDING,Ordering::AcqRel,Ordering::Acquire).is_err() {
            data.unexpected.fetch_add(1,Ordering::Relaxed);return;
        }
        worker.finish(if mode.selects(pool) {apply(api)} else {Attempt::new(Outcome::Unselected)});
    }
    fn report(&self,mode:Option<Mode>)->serde_json::Value {
        let mut pools=serde_json::Map::new();
        for pool in Pool::ALL {
            let data=&self.pools[pool.index()];
            let selected=mode.is_some_and(|mode|mode.selects(pool));
            let mut counts=serde_json::Map::new();
            for outcome in Outcome::ALL {counts.insert(outcome.name().into(),0u64.into());}
            let mut workers=Vec::new();
            let mut pending=0usize;let mut verified=0usize;let mut attempts=0usize;
            for (index,worker) in data.workers.iter().enumerate() {
                let Some(result)=worker.read() else {continue;};
                match result {
                    Err(())=>{pending+=1;workers.push(serde_json::json!({"index":index,"status":"startup_pending"}));},
                    Ok(attempt)=>{
                        let count=counts.get_mut(attempt.outcome.name()).unwrap();
                        *count=(count.as_u64().unwrap()+1).into();
                        verified+=usize::from(matches!(attempt.outcome,Outcome::Applied|Outcome::AlreadyBelowNormal));
                        attempts+=usize::from(attempt.outcome.attempted());
                        workers.push(serde_json::json!({"index":index,"status":attempt.outcome.name(),
                            "before":attempt.before,"after":attempt.after,"win32_error":attempt.error}));
                    }
                }
            }
            let unexpected=data.unexpected.load(Ordering::Relaxed);
            let status=if mode.is_none() {"disabled"} else if unexpected!=0 {"unexpected_start_indices"}
                else if !selected {"not_selected"} else if workers.is_empty() {"not_observed"}
                else if pending!=0 {"startup_pending"} else if verified==workers.len() {"verified_for_observed_workers"}
                else {"partially_applied_or_unapplied"};
            pools.insert(pool.name().into(),serde_json::json!({"requested":selected,"status":status,
                "observed_starts":workers.len(),"pending":pending,"set_attempts":attempts,
                "unexpected_or_repeated_start_indices":unexpected,"outcomes":counts,"workers":workers}));
        }
        serde_json::json!({"requested_mode":mode.map(Mode::name),"platform_supported":cfg!(windows),
            "target_thread_priority":BELOW_NORMAL,"pools":pools,
            "scope":"Only existing visual-mesh and distant-surface workers; all additionally selects native-terrain workers. Main, native owner, local tick and transparency helper are untouched.",
            "coverage":"Observed startup callbacks only; unused pools may be absent. Results verify priority at startup, not throughout the run. Persistent pools require a fresh process per arm.",
            "mutations":"CPU thread priority only; no process priority, affinity, I/O priority, core count, or graphics setting changes."})
    }
}

pub(crate) fn on_start(pool:Pool,index:usize) {DIAGNOSTICS.start(MODE.get().copied(),pool,index,&mut SystemApi);}
pub(crate) fn report()->serde_json::Value {
    let mut result=DIAGNOSTICS.report(MODE.get().copied());
    result["process_priority_class_at_report"]=process_priority_class();
    result
}
struct SystemApi;
#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    #[link(name="kernel32")]
    unsafe extern "system" {
        pub(super) fn GetCurrentThread()->*mut c_void;
        pub(super) fn GetThreadPriority(thread:*mut c_void)->i32;
        pub(super) fn SetThreadPriority(thread:*mut c_void,priority:i32)->i32;
        pub(super) fn GetLastError()->u32;
        pub(super) fn GetCurrentProcess()->*mut c_void;
        pub(super) fn GetPriorityClass(process:*mut c_void)->u32;
    }
}
impl PriorityApi for SystemApi {
    fn supported(&self)->bool {cfg!(windows)}
    fn query(&mut self)->Result<i32,u32> {
        #[cfg(windows)] unsafe {
            // Pseudo-handle borrowed only on its own thread; never CloseHandle.
            let value=windows::GetThreadPriority(windows::GetCurrentThread());
            if value==ERROR_RETURN {Err(windows::GetLastError())} else {Ok(value)}
        }
        #[cfg(not(windows))] {Err(50)}
    }
    fn set_below_normal(&mut self)->Result<(),u32> {
        #[cfg(windows)] unsafe {
            if windows::SetThreadPriority(windows::GetCurrentThread(),BELOW_NORMAL)==0 {Err(windows::GetLastError())} else {Ok(())}
        }
        #[cfg(not(windows))] {Err(50)}
    }
}
fn process_priority_class()->serde_json::Value {
    #[cfg(windows)] unsafe {
        let value=windows::GetPriorityClass(windows::GetCurrentProcess());
        if value==0 {let error=windows::GetLastError();serde_json::json!({"value":null,"win32_error":error})}
        else {serde_json::json!({"value":value,"win32_error":null})}
    }
    #[cfg(not(windows))] {serde_json::json!({"value":null,"unsupported":true})}
}

#[cfg(test)]
#[path="worker_priority_tests.rs"]
mod tests;
