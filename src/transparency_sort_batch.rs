//! Bounded same-frame sorting for large visible batches. GPU writes stay on the owner.
use super::{ChunkMesh,Order,Sort,Vec3};
use super::super::render_profile::{self,Frame};
use crate::world::ChunkPos;
use std::time::Instant;

const MAX_JOBS:usize=512;
const MAX_QUADS:usize=262144;
const MIN_JOBS:usize=32;
const MIN_QUADS:usize=32768;
const LARGE_MIN_QUADS:usize=98304;
const WIDE_MAX_JOBS:usize=1024;
const WIDE_MAX_QUADS:usize=1048576;
const ORDINARY_TWO_MIN_CPUS:usize=16;
#[derive(Clone,Copy,Debug,PartialEq,Eq,clap::ValueEnum)]
pub(crate) enum Mode { Original, OwnerBatched, ScopedOne, ScopedLarge, ScopedWide, ScopedTwo }
impl Default for Mode {
    fn default()->Self {
        // Resolve the ordinary policy once per process, including constructor,
        // client configuration and pack reload. Explicit overrides do not use it.
        static ORDINARY:std::sync::OnceLock<Mode>=std::sync::OnceLock::new();
        *ORDINARY.get_or_init(||Self::ordinary_for_parallelism(
            std::thread::available_parallelism().ok().map(usize::from)))
    }
}
impl Mode {
    fn ordinary_for_parallelism(available:Option<usize>)->Self {
        if available.is_some_and(|count|count>=ORDINARY_TWO_MIN_CPUS) {Self::ScopedTwo}else{Self::ScopedWide}
    }
    fn helper_count(self)->usize {if self==Self::ScopedTwo {2}else{usize::from(self.scoped())}}
    fn scoped(self)->bool {matches!(self,Self::ScopedOne|Self::ScopedLarge|Self::ScopedWide|Self::ScopedTwo)}
    fn min_quads(self)->usize {if matches!(self,Self::ScopedLarge|Self::ScopedWide|Self::ScopedTwo) {LARGE_MIN_QUADS}else{MIN_QUADS}}
    fn max_jobs(self)->usize {if matches!(self,Self::ScopedWide|Self::ScopedTwo) {WIDE_MAX_JOBS}else{MAX_JOBS}}
    fn max_quads(self)->usize {if matches!(self,Self::ScopedWide|Self::ScopedTwo) {WIDE_MAX_QUADS}else{MAX_QUADS}}
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Lane { Owner, Worker, WorkerSecond }
#[derive(Clone,Copy,Default)]
struct Fault {
    #[cfg(test)] lane:Option<Lane>,
    #[cfg(test)] slot:usize,
    #[cfg(test)] after:bool,
}
impl Fault {
    #[inline] fn check(self,lane:Lane,slot:usize,after:bool) {
        #[cfg(test)] if self.lane==Some(lane)&&self.slot==slot&&self.after==after {panic!("sort fixture fault");}
        #[cfg(not(test))] let _=(lane,slot,after);
    }
}
struct Job { slot:usize,mask:u16,order:Order,result:Sort,elapsed_ms:f64 }
trait Slots {
    fn len(&self)->usize;
    fn order(&self,slot:usize)->&Order;
    fn mask(&self,slot:usize)->u16;
    fn take(&mut self,slot:usize)->Order;
    fn put(&mut self,slot:usize,order:Order);
}
struct MeshSlots<'a,'b>(&'a mut [(ChunkPos,&'b mut ChunkMesh)]);
impl Slots for MeshSlots<'_,'_> {
    fn len(&self)->usize {self.0.len()}
    fn order(&self,i:usize)->&Order {&self.0[i].1.order}
    fn mask(&self,i:usize)->u16 {self.0[i].1.mask}
    fn take(&mut self,i:usize)->Order {std::mem::take(&mut self.0[i].1.order)}
    fn put(&mut self,i:usize,order:Order) {self.0[i].1.order=order;}
}
// Scope unwinding waits for the helper before dropping this guard. Thus every
// Order returns to its exact slot on success, owner panic or helper panic.
struct Restore<'a,S:Slots> {slots:&'a mut S,jobs:&'a mut Vec<Job>}
impl<S:Slots> Drop for Restore<'_,S> {
    fn drop(&mut self) {
        for job in self.jobs.iter_mut(){self.slots.put(job.slot,std::mem::take(&mut job.order));}
        if std::thread::panicking(){self.jobs.clear();}
    }
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Admission { Ready{jobs:usize,quads:usize}, Small, Limit }
fn admission(slots:&impl Slots,eye:Vec3,mode:Mode)->Admission {
    let (min_quads,max_jobs,max_quads)=(mode.min_quads(),mode.max_jobs(),mode.max_quads());
    let(mut jobs,mut quads)=(0usize,0usize);
    for slot in 0..slots.len() {
        let order=slots.order(slot);
        if order.needs_sort(eye,slots.mask(slot)) {
            jobs+=1;
            let Some(next)=quads.checked_add(order.quads.len()) else{return Admission::Limit;};quads=next;
            if jobs>max_jobs||quads>max_quads{return Admission::Limit;}
        }
    }
    if jobs<MIN_JOBS||quads<min_quads{Admission::Small}else{Admission::Ready{jobs,quads}}
}
fn sort_jobs(jobs:&mut [Job],eye:Vec3,enabled:bool,lane:Lane,fault:Fault) {
    for job in jobs {
        fault.check(lane,job.slot,false);
        let start=enabled.then(Instant::now);
        job.result=job.order.sort(eye,job.mask);
        job.elapsed_ms=render_profile::elapsed(start);
        fault.check(lane,job.slot,true);
    }
}
fn split(jobs:&[Job])->(usize,bool) {
    let total=jobs.iter().map(|job|job.order.quads.len()).sum::<usize>();
    let mut left=0;let mut cut=1;
    for (i,job) in jobs.iter().enumerate().take(jobs.len()-1) {
        left+=job.order.quads.len();cut=i+1;if left>=total/2{break;}
    }
    (cut,left>=total-left) // Larger span stays on the caller thread.
}
// Only ScopedTwo uses this extra scan. The admitted total is <=1,048,576,
// so multiplying a prefix by at most3 cannot overflow. Earliest cut wins ties.
fn balanced_cut(jobs:&[Job],parts:usize)->usize {
    debug_assert!(parts>=2&&jobs.len()>=parts);
    let total=jobs.iter().map(|job|job.order.quads.len()).sum::<usize>();
    let(mut prefix,mut best,mut cut)=(0usize,usize::MAX,1usize);
    for(i,job)in jobs.iter().enumerate().take(jobs.len()-(parts-1)) {
        prefix+=job.order.quads.len();
        let difference=(prefix*parts).abs_diff(total);
        if difference<best {best=difference;cut=i+1;}
    }
    cut
}
fn split_three(jobs:&[Job])->(usize,usize,usize) {
    let first=balanced_cut(jobs,3);
    let second=first+balanced_cut(&jobs[first..],2);
    let weights=[&jobs[..first],&jobs[first..second],&jobs[second..]]
        .map(|span|span.iter().map(|job|job.order.quads.len()).sum::<usize>());
    let mut owner=0;
    for lane in 1..3 {if weights[lane]>weights[owner] {owner=lane;}}
    (first,second,owner)
}
pub(crate) struct Batch {
    mode:Mode,jobs:Vec<Job>,pool:Option<rayon::ThreadPool>,pool_error:Option<String>,
    #[cfg(test)] fault:Fault,
    #[cfg(test)] successful_runs:usize,
}
impl Default for Batch {fn default()->Self{Self::new(Mode::default())}}
impl Batch {
    pub(crate) fn new(mode:Mode)->Self {
        let mut pool=None;let mut pool_error=None;
        if mode.scoped() {
            if std::thread::available_parallelism().map_or(1,usize::from)<4 {
                pool_error=Some("fewer than4 logical CPUs".into());
            }else{
                match rayon::ThreadPoolBuilder::new().num_threads(mode.helper_count()).thread_name(|i|format!("render-sort-{i}")).build() {
                    Ok(value)=>pool=Some(value),Err(error)=>pool_error=Some(error.to_string()),
                }
            }
        }
        Self{mode,jobs:if mode==Mode::Original{Vec::new()}else{Vec::with_capacity(mode.max_jobs())},pool,pool_error,
            #[cfg(test)]fault:Fault::default(),
            #[cfg(test)]successful_runs:0}
    }
    #[cfg(test)]
    pub(crate) fn successful_runs(&self)->usize {self.successful_runs}
    #[cfg(test)]
    pub(crate) fn helper_threads(&self)->usize {self.pool.as_ref().map_or(0,rayon::ThreadPool::current_num_threads)}
    #[cfg(test)]
    pub(crate) fn ensure_helper_for_test(&mut self) {
        if self.mode.scoped()&&self.pool.is_none(){
            self.pool=Some(rayon::ThreadPoolBuilder::new().num_threads(self.mode.helper_count()).build().unwrap());
            self.pool_error=None;
        }
    }
    pub(crate) fn configure(&mut self,mode:Mode) {
        // Renderer construction already selects the ordinary default. Avoid
        // creating a second helper when the client keeps that same mode.
        if self.mode!=mode {*self=Self::new(mode);}
    }
    pub(crate) fn wants_batch(&self,capture_enabled:bool)->bool {
        self.mode!=Mode::Original&&!capture_enabled&&(!self.mode.scoped()||self.pool.is_some())
    }
    pub(crate) fn report(&self)->serde_json::Value {serde_json::json!({
        "mode":match self.mode{Mode::Original=>"original",Mode::OwnerBatched=>"owner-batched",Mode::ScopedOne=>"scoped-one",Mode::ScopedLarge=>"scoped-large",Mode::ScopedWide=>"scoped-wide",Mode::ScopedTwo=>"scoped-two"},
        "helper_available":self.pool.is_some(),"pool_error":self.pool_error,
        "configured_helpers":self.mode.helper_count(),"helper_threads":self.pool.as_ref().map_or(0,rayon::ThreadPool::current_num_threads),
        "ordinary_policy":{"two_minimum_available_logical_cpus":ORDINARY_TWO_MIN_CPUS,
            "below_or_unknown_mode":"scoped-wide","selection":"cached first default resolution per process",
            "explicit_overrides":"preserved","validation":"Measured on one16-logical-CPU system; the availability guard is not validation of every qualifying CPU."},
        "job_capacity":self.jobs.capacity(),"job_capacity_bytes":self.jobs.capacity()*std::mem::size_of::<Job>(),"max_jobs":self.mode.max_jobs(),"max_quads":self.mode.max_quads(),"min_jobs":MIN_JOBS,"min_quads":self.mode.min_quads(),
        "note":"Ordinary selection is ScopedTwo with at least16 available logical CPUs, otherwise ScopedWide (including unknown availability); the first default selection is cached per process. Explicit benchmark overrides are preserved. Capture, fewer than4 logical CPUs or unavailable helper use original serial preparation. Small or oversized batches also fall back to serial sorting. Batched jobs preserve GPU write order. Existing scoped modes each add one private helper; ScopedTwo uses two helpers in one private pool when selected by the guarded ordinary policy or an explicit benchmark override. Main sorts the largest contiguous quad-weighted span and synchronously joins the remaining span(s). worker_jobs sums helper-span jobs. ScopedTwo retains the Wide admission/storage bounds and adds one Rayon internal HeapJob allocation/task wake per admitted frame; no new per-frame geometry/job-vector/scratch allocation is introduced. transparency_sort is an overlapping per-job elapsed sum; batch_scope is critical-path wall time. Bounds are not a latency guarantee. Background thread priorities are unchanged."})}
    pub(crate) fn run(&mut self,visible:&mut [(ChunkPos,&mut ChunkMesh)],eye:Vec3,profile:&mut Frame)->bool {
        self.run_slots(&mut MeshSlots(visible),eye,profile)
    }
    fn run_slots(&mut self,slots:&mut impl Slots,eye:Vec3,profile:&mut Frame)->bool {
        // Earlier successful completion or panic restoration leaves empty
        // placeholders here; no geometry allocation is dropped by clear().
        self.jobs.clear();
        if self.mode==Mode::Original{return false;}
        if self.mode.scoped()&&self.pool.is_none(){
            if profile.enabled{profile.counters.transparency_batch_unavailable+=1;}return false;
        }
        let started=profile.timer();let admitted=admission(slots,eye,self.mode);
        profile.stages_ms.transparency_batch_admission+=render_profile::elapsed(started);
        let Admission::Ready{jobs,quads} = admitted else {
            if profile.enabled {match admitted{Admission::Small=>profile.counters.transparency_batch_small+=1,
                Admission::Limit=>profile.counters.transparency_batch_limit+=1,_=>{}}}return false;
        };
        debug_assert!(self.jobs.capacity()>=self.mode.max_jobs());
        let taken=profile.timer();
        let guard=Restore{slots,jobs:&mut self.jobs};
        for slot in 0..guard.slots.len() {
            let mask=guard.slots.mask(slot);
            if guard.slots.order(slot).needs_sort(eye,mask) {
                let order=guard.slots.take(slot);
                // Admission guarantees this push cannot grow the reused Vec.
                guard.jobs.push(Job{slot,mask,order,result:Sort::Skipped,elapsed_ms:0.});
            }
        }
        debug_assert_eq!(guard.jobs.len(),jobs);
        profile.stages_ms.transparency_batch_take+=render_profile::elapsed(taken);
        #[cfg(test)]let fault=self.fault;
        #[cfg(not(test))]let fault=Fault::default();
        let scope_start=profile.timer();let mut worker_jobs=0;
        if self.mode==Mode::ScopedTwo {
            let(first,second,owner)=split_three(guard.jobs);
            let(left,rest)=guard.jobs.split_at_mut(first);
            let(middle,right)=rest.split_at_mut(second-first);
            let(main,worker,second_worker)=match owner {
                0=>(left,middle,right),1=>(middle,left,right),_=>(right,left,middle),
            };
            worker_jobs=worker.len()+second_worker.len();let enabled=profile.enabled;
            let(owner_ms,owner_finished)=self.pool.as_ref().unwrap().in_place_scope(|scope|{
                scope.spawn(move |_|sort_jobs(worker,eye,enabled,Lane::Worker,fault));
                scope.spawn(move |_|sort_jobs(second_worker,eye,enabled,Lane::WorkerSecond,fault));
                let owner_start=enabled.then(Instant::now);
                sort_jobs(main,eye,enabled,Lane::Owner,fault);
                (render_profile::elapsed(owner_start),enabled.then(Instant::now))
            });
            profile.stages_ms.transparency_batch_owner+=owner_ms;
            profile.stages_ms.transparency_batch_join_tail+=render_profile::elapsed(owner_finished);
        }else if self.mode.scoped() {
            let(cut,main_left)=split(guard.jobs);
            let(left,right)=guard.jobs.split_at_mut(cut);
            let(main,worker)=if main_left{(left,right)}else{(right,left)};
            worker_jobs=worker.len();let enabled=profile.enabled;
            let (owner_ms,owner_finished)=self.pool.as_ref().unwrap().in_place_scope(|scope|{
                scope.spawn(move |_|sort_jobs(worker,eye,enabled,Lane::Worker,fault));
                let owner_start=enabled.then(Instant::now);
                sort_jobs(main,eye,enabled,Lane::Owner,fault);
                (render_profile::elapsed(owner_start),enabled.then(Instant::now))
            });
            profile.stages_ms.transparency_batch_owner+=owner_ms;
            profile.stages_ms.transparency_batch_join_tail+=render_profile::elapsed(owner_finished);
        }else{
            let owner_start=profile.timer();sort_jobs(guard.jobs,eye,profile.enabled,Lane::Owner,fault);
            profile.stages_ms.transparency_batch_owner+=render_profile::elapsed(owner_start);
        }
        profile.stages_ms.transparency_batch_scope+=render_profile::elapsed(scope_start);
        profile.stages_ms.transparency_sort+=guard.jobs.iter().map(|j|j.elapsed_ms).sum::<f64>();
        let restoring=profile.timer();drop(guard);
        profile.stages_ms.transparency_batch_restore+=render_profile::elapsed(restoring);
        if profile.enabled {
            profile.counters.transparency_batch_frames+=1;profile.counters.transparency_batch_jobs+=jobs;
            profile.counters.transparency_batch_quads+=quads;profile.counters.transparency_batch_worker_jobs+=worker_jobs;
        }
        #[cfg(test)]{self.successful_runs+=1;}
        true
    }
    pub(crate) fn complete(&mut self,visible:&mut [(ChunkPos,&mut ChunkMesh)],queue:&wgpu::Queue,profile:&mut Frame) {
        let mut next=self.jobs.iter().peekable();
        for (slot,(_,chunk)) in visible.iter_mut().enumerate() {
            let result=if next.peek().is_some_and(|job|job.slot==slot){next.next().unwrap().result}else{Sort::Skipped};
            chunk.complete_sorted(queue,result,profile);
        }
        debug_assert!(next.next().is_none());self.jobs.clear();
    }
    pub(crate) fn complete_staged(&mut self,visible:&mut [(ChunkPos,&mut ChunkMesh)],queue:&wgpu::Queue,profile:&mut Frame,
        staging:&mut super::super::transparent_staging::Session<'_>,encoder:&mut wgpu::CommandEncoder) {
        let mut next=self.jobs.iter().peekable();
        for (slot,(_,chunk)) in visible.iter_mut().enumerate() {
            let result=if next.peek().is_some_and(|job|job.slot==slot){next.next().unwrap().result}else{Sort::Skipped};
            chunk.complete_sorted_staged(queue,result,profile,staging,encoder);
        }
        debug_assert!(next.next().is_none());self.jobs.clear();
    }
}
#[cfg(test)]
#[path="transparency_sort_batch_tests.rs"]
mod tests;

#[cfg(test)]
#[path="transparency_sort_batch_gpu_tests.rs"]
mod gpu_tests;
