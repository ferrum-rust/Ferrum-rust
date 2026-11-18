use std::{cell::Cell,sync::{Arc,Mutex,MutexGuard,atomic::{AtomicU64,Ordering}},time::Instant};
use tracing::{span::{Attributes,Id,Record},subscriber::Interest,Dispatch,Event,Metadata,Subscriber};

const TOP:usize=32;
const SLOTS:usize=32;
const DEPTH:usize=8;
const IGNORED:u64=1;
const THRESHOLDS_MS:[f64;4]=[1.,2.,4.,8.];
static NEXT_CAPTURE:AtomicU64=AtomicU64::new(1);
thread_local! {static CONTEXT:Cell<Option<(u64,u64)>>=const {Cell::new(None)};}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Kind {Core=0,Wait=1,Native=2}
fn filter_parts(name:&str,target:&str,file:Option<&str>,line:Option<u32>,span:bool)->Option<Kind> {
    if !span{return None}
    let (kind,suffix,expected_line)=match(name,target) {
        ("Surface::get_current_texture","wgpu_core::present") =>
            (Kind::Core,"wgpu-core-24.0.5/src/present.rs",121),
        ("Device::wait","wgpu_hal::vulkan::device") =>
            (Kind::Wait,"wgpu-hal-24.0.4/src/vulkan/device.rs",2598),
        ("vkAcquireNextImageKHR","wgpu_hal::vulkan::instance") =>
            (Kind::Native,"wgpu-hal-24.0.4/src/vulkan/instance.rs",1044),
        _=>return None,
    };
    // Compare path components without allocating a normalized string. Require
    // the pinned crate/version and complete source suffix, not only basename.
    let file=file?;
    let boundary=file.len()==suffix.len() || file.len()>suffix.len()
        && matches!(file.as_bytes()[file.len()-suffix.len()-1],b'/'|b'\\');
    let matched=boundary && file.bytes().rev().zip(suffix.bytes().rev())
        .take(suffix.len()).all(|(a,b)|if b==b'/' {a==b'/'||a==b'\\'}else{a==b});
    (matched && line==Some(expected_line)).then_some(kind)
}
fn filter(meta:&Metadata<'_>)->Option<Kind> {
    if *meta.level()!=tracing::Level::INFO{return None}
    filter_parts(meta.name(),meta.target(),meta.file(),meta.line(),meta.is_span())
}

#[derive(Clone,Copy,Debug,Default)]
struct SpanSlot {id:u64,attempt:u64,kind:Option<Kind>,refs:u32,entered:bool,exited:bool,start:u64}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Outcome {Success,Error,Unwind}
impl Outcome {fn name(self)->&'static str {match self {Self::Success=>"success",Self::Error=>"native_error",Self::Unwind=>"unwind"}}}
#[derive(Clone,Copy,Debug)]
struct Observation {
    sequence:u64,frame_id:u64,application_seconds_label:f64,outcome:Outcome,measured:bool,
    counts:[u32;3],ns:[u64;3],invalid:bool,public_ms:Option<f64>,
    presented:bool,sample_index:Option<usize>,begin_ms:Option<f64>,end_ms:Option<f64>,
}
impl Observation {
    fn new(sequence:u64,frame_id:u64,label:f64)->Self {Self {sequence,frame_id,
        application_seconds_label:label,outcome:Outcome::Unwind,measured:true,counts:[0;3],ns:[0;3],invalid:false,
        public_ms:None,presented:false,sample_index:None,begin_ms:None,end_ms:None}}
    fn complete_scopes(&self)->bool {self.counts==[1,1,1] && !self.invalid
        && self.ns[1].checked_add(self.ns[2]).is_some_and(|n|n<=self.ns[0])}
    fn rank(&self)->f64 {self.public_ms.unwrap_or(self.ns[0] as f64/1e6)}
    fn json(&self)->serde_json::Value {
        let core=self.ns[0]as f64/1e6;let wait=self.ns[1]as f64/1e6;let native=self.ns[2]as f64/1e6;
        let scopes=self.complete_scopes();
        serde_json::json!({"sequence":self.sequence,"renderer_frame_id":self.frame_id,
            "application_seconds_context_label":self.application_seconds_label,
            "outcome":self.outcome.name(),"scope_counts":self.counts,"scope_complete":scopes,
            "invalid":self.invalid,"public_acquire_ms":self.public_ms,"core_acquire_ms":core,
            "prior_submission_wait_ms":wait,"native_acquire_ms":native,
            "core_residual_ms":scopes.then_some((core-wait-native).max(0.)),
            "public_residual_ms":if scopes {self.public_ms.map(|v|(v-core).max(0.))}else{None},
            "presented":self.presented,"sample_index":self.sample_index,
            "present_begin_application_ms":self.begin_ms,"present_end_application_ms":self.end_ms})
    }
}
#[derive(Default)]
struct Totals {attempts:u64,success:u64,native_errors:u64,unwinds:u64,scope_complete:u64,successful_scope_complete:u64,
    invalid:u64,public_count:u64,public_ms:f64,ns:[u128;3],core_residual_ns:u128,
    public_residual_ms:f64,presented:u64,first_present:u64,scope_counts:[u64;3],
    scope_at_least:[[u64;4];3],public_at_least:[u64;4]}
struct Book {
    next_attempt:u64,next_span:u64,active:Option<Observation>,await_public:Option<Observation>,
    pending_present:Option<u64>,slots:[SpanSlot;SLOTS],stack:[u64;DEPTH],depth:usize,
    top:[Option<Observation>;TOP],totals:Totals,warmup:Totals,
    lifecycle_errors:u64,slot_overflow:u64,serial_exhaustion:u64,unstamped:u64,unassociated_presents:u64,
}
impl Book {
    fn new()->Self {Self{next_attempt:1,next_span:2,active:None,await_public:None,pending_present:None,
        slots:[SpanSlot::default();SLOTS],stack:[0;DEPTH],depth:0,top:[None;TOP],totals:Totals::default(),warmup:Totals::default(),
        lifecycle_errors:0,slot_overflow:0,serial_exhaustion:0,unstamped:0,unassociated_presents:0}}
    fn bad(&mut self){self.lifecycle_errors+=1;if let Some(row)=&mut self.active{row.invalid=true;}}
    fn clear_pending(&mut self){if self.pending_present.take().is_some(){self.unstamped+=1;}}
    fn begin(&mut self,frame:u64,label:f64)->Option<u64> {
        self.clear_pending();
        if self.active.is_some(){self.bad();return None}
        if let Some(mut previous)=self.await_public.take(){previous.invalid=true;self.commit(previous);}
        if !label.is_finite()||label<0. {self.bad();return None}
        let Some(next)=self.next_attempt.checked_add(1) else{self.serial_exhaustion+=1;return None};
        let id=self.next_attempt;self.next_attempt=next;
        self.active=Some(Observation::new(id,frame,label));self.depth=0;Some(id)
    }
    fn begin_warmup(&mut self,frame:u64,label:f64)->Option<u64> {
        let id=self.begin(frame,label)?;self.active.as_mut().unwrap().measured=false;Some(id)
    }
    fn new_span(&mut self,attempt:u64,kind:Kind)->u64 {
        if self.active.as_ref().map(|r|r.sequence)!=Some(attempt){self.bad();return IGNORED}
        let Some(index)=self.slots.iter().position(|s|s.id==0) else {self.slot_overflow+=1;self.bad();return IGNORED};
        let Some(next)=self.next_span.checked_add(1) else{self.serial_exhaustion+=1;self.bad();return IGNORED};
        let id=self.next_span;self.next_span=next;
        self.slots[index]=SpanSlot{id,attempt,kind:Some(kind),refs:1,..Default::default()};id
    }
    fn can_enter(&mut self,attempt:u64,id:u64)->bool {
        let Some(slot)=self.slots.iter().find(|s|s.id==id) else{self.bad();return false};
        let kind=slot.kind.unwrap();
        let nesting=if kind==Kind::Core {self.depth==0}else{self.depth==1 && self.slots.iter()
            .any(|s|s.id==self.stack[0]&&s.kind==Some(Kind::Core))};
        let sequence=self.active.as_ref().is_some_and(|r|match kind {
            Kind::Core=>r.counts==[0;3],
            Kind::Wait=>r.counts[1]==0&&r.counts[2]==0,
            Kind::Native=>r.counts[1]==1&&r.counts[2]==0,
        });
        if slot.attempt!=attempt||slot.entered||slot.exited||self.depth>=DEPTH||!nesting
            ||!sequence||self.active.as_ref().map(|r|r.sequence)!=Some(attempt) {self.bad();return false}
        true
    }
    fn enter(&mut self,id:u64,now:u64) {
        let slot=self.slots.iter_mut().find(|s|s.id==id).unwrap();
        slot.entered=true;slot.start=now;self.stack[self.depth]=id;self.depth+=1;
    }
    fn can_exit(&mut self,attempt:u64,id:u64)->bool {
        let okay=self.active.as_ref().map(|r|r.sequence)==Some(attempt)&&self.depth>0
            &&self.stack[self.depth-1]==id&&self.slots.iter().any(|s|s.id==id&&s.attempt==attempt&&s.entered&&!s.exited);
        if !okay{self.bad();}okay
    }
    fn exit(&mut self,id:u64,now:u64) {
        let slot=self.slots.iter_mut().find(|s|s.id==id).unwrap();
        slot.exited=true;self.depth-=1;
        let Some(delta)=now.checked_sub(slot.start) else{self.bad();return};
        let kind=slot.kind.unwrap()as usize;
        let row=self.active.as_mut().unwrap();
        row.counts[kind]=row.counts[kind].saturating_add(1);
        if let Some(sum)=row.ns[kind].checked_add(delta){row.ns[kind]=sum;}else{self.bad();}
    }
    fn clone_id(&mut self,id:u64) {
        let Some(slot)=self.slots.iter_mut().find(|s|s.id==id) else{self.bad();return};
        if let Some(refs)=slot.refs.checked_add(1){slot.refs=refs;}else{self.bad();}
    }
    fn close(&mut self,id:u64)->bool {
        let Some(index)=self.slots.iter().position(|s|s.id==id) else{self.bad();return false};
        if self.slots[index].refs>1{self.slots[index].refs-=1;return false}
        if self.slots[index].entered&&!self.slots[index].exited{self.bad();}
        self.slots[index]=SpanSlot::default();true
    }
    fn end(&mut self,attempt:u64,outcome:Outcome) {
        if self.active.as_ref().map(|r|r.sequence)!=Some(attempt){self.bad();return}
        if self.depth!=0 {self.bad();self.depth=0;}
        let mut row=self.active.take().unwrap();row.outcome=outcome;
        if !row.measured||outcome==Outcome::Unwind {self.commit(row);} else {self.await_public=Some(row);}
    }
    fn public_elapsed(&mut self,ms:f64)->anyhow::Result<()> {
        let Some(mut row)=self.await_public.take()else{self.bad();anyhow::bail!("surface wait trace has no completed acquisition")};
        if !ms.is_finite()||ms<0. {row.invalid=true;self.commit(row);anyhow::bail!("invalid public acquisition duration")}
        row.public_ms=Some(ms);
        if ms+1e-9<row.ns[0]as f64/1e6{row.invalid=true;}
        if row.outcome==Outcome::Success{self.pending_present=Some(row.sequence);}
        self.commit(row);Ok(())
    }
    fn commit(&mut self,row:Observation) {
        if !row.measured {
            let total=&mut self.warmup;total.attempts+=1;
            match row.outcome{Outcome::Success=>total.success+=1,Outcome::Error=>total.native_errors+=1,Outcome::Unwind=>total.unwinds+=1}
            total.invalid+=u64::from(row.invalid);
            for(i,&n)in row.counts.iter().enumerate(){total.scope_counts[i]+=u64::from(n);}
            if row.complete_scopes(){total.scope_complete+=1;total.successful_scope_complete+=u64::from(row.outcome==Outcome::Success);}
            return;
        }
        let total=&mut self.totals;total.attempts+=1;
        match row.outcome{Outcome::Success=>total.success+=1,Outcome::Error=>total.native_errors+=1,Outcome::Unwind=>total.unwinds+=1}
        total.invalid+=u64::from(row.invalid);
        for(i,&n)in row.ns.iter().enumerate(){total.ns[i]+=u128::from(n);total.scope_counts[i]+=u64::from(row.counts[i]);
            if row.counts[i]>0{for(j,&threshold)in THRESHOLDS_MS.iter().enumerate(){total.scope_at_least[i][j]+=u64::from(n as f64/1e6>=threshold);}}}
        if let Some(ms)=row.public_ms{total.public_count+=1;total.public_ms+=ms;
            for(j,&threshold)in THRESHOLDS_MS.iter().enumerate(){total.public_at_least[j]+=u64::from(ms>=threshold);}}
        if row.complete_scopes(){total.scope_complete+=1;total.successful_scope_complete+=u64::from(row.outcome==Outcome::Success);
            total.core_residual_ns+=u128::from(row.ns[0]-row.ns[1]-row.ns[2]);
            if let Some(ms)=row.public_ms{total.public_residual_ms+=(ms-row.ns[0]as f64/1e6).max(0.);}}
        if let Some(pos)=self.top.iter().position(|old|old.is_none_or(|old|row.rank()>old.rank())) {
            for i in(pos+1..TOP).rev(){self.top[i]=self.top[i-1];}self.top[pos]=Some(row);
        }
    }
    fn presented(&mut self,sample:Option<usize>,begin:Option<f64>,end:f64)->anyhow::Result<()> {
        let Some(sequence)=self.pending_present.take()else{self.unassociated_presents+=1;return Ok(())};
        if !end.is_finite()||end<0.||begin.is_some()!=sample.is_some()
            ||begin.is_some_and(|v|!v.is_finite()||v<0.||v>end) {
            self.bad();anyhow::bail!("invalid surface wait present anchors")
        }
        self.totals.presented+=1;self.totals.first_present+=u64::from(sample.is_none());
        if let Some(row)=self.top.iter_mut().flatten().find(|r|r.sequence==sequence) {
            row.presented=true;row.sample_index=sample;row.begin_ms=begin;row.end_ms=Some(end);
        }
        Ok(())
    }
}

struct Clock {epoch:Instant,#[cfg(test)]fake:Option<Arc<(AtomicU64,AtomicU64)>>}
impl Clock {
    fn new()->Self{Self{epoch:Instant::now(),#[cfg(test)]fake:None}}
    fn now(&self)->u64 {
        #[cfg(test)] if let Some(fake)=&self.fake {fake.1.fetch_add(1,Ordering::Relaxed);return fake.0.load(Ordering::Relaxed)}
        self.epoch.elapsed().as_nanos().min(u128::from(u64::MAX))as u64
    }
}
struct Shared {id:u64,book:Mutex<Book>,clock:Clock,foreign:AtomicU64,contention:AtomicU64,reentry:AtomicU64,
    registered_mask:AtomicU64}
impl Shared {
    fn lock(&self)->Option<MutexGuard<'_,Book>> {match self.book.try_lock(){
        Ok(book)=>Some(book),Err(_)=>{self.contention.fetch_add(1,Ordering::Relaxed);None}}}
    fn attempt(&self)->Option<u64> {
        let value=CONTEXT.with(Cell::get);
        match value{Some((owner,attempt))if owner==self.id=>Some(attempt),_=>{
            self.foreign.fetch_add(1,Ordering::Relaxed);None}}
    }
}
struct Sink {shared:Arc<Shared>}
impl Subscriber for Sink {
    fn enabled(&self,metadata:&Metadata<'_>)->bool{filter(metadata).is_some()}
    fn register_callsite(&self,metadata:&'static Metadata<'static>)->Interest {
        if let Some(kind)=filter(metadata){self.shared.registered_mask.fetch_or(1<<(kind as u8),Ordering::Relaxed);
            Interest::always()}else{Interest::never()}
    }
    fn max_level_hint(&self)->Option<tracing::metadata::LevelFilter>{Some(tracing::metadata::LevelFilter::INFO)}
    fn new_span(&self,attrs:&Attributes<'_>)->Id {
        let id=(||{let kind=filter(attrs.metadata())?;let attempt=self.shared.attempt()?;
            Some(self.shared.lock()?.new_span(attempt,kind))})();Id::from_u64(id.unwrap_or(IGNORED))
    }
    fn record(&self,_span:&Id,_values:&Record<'_>){}
    fn record_follows_from(&self,_span:&Id,_follows:&Id){}
    fn event_enabled(&self,_event:&Event<'_>)->bool{false}
    fn event(&self,_event:&Event<'_>){}
    fn enter(&self,id:&Id) {
        if id.into_u64()==IGNORED{return}let Some(attempt)=self.shared.attempt()else{return};
        if let Some(mut book)=self.shared.lock(){if book.can_enter(attempt,id.into_u64()){
            let now=if book.active.as_ref().is_some_and(|r|r.measured){self.shared.clock.now()}else{0};
            book.enter(id.into_u64(),now);}}
    }
    fn exit(&self,id:&Id) {
        if id.into_u64()==IGNORED{return}let Some(attempt)=self.shared.attempt()else{return};
        if let Some(mut book)=self.shared.lock(){if book.can_exit(attempt,id.into_u64()){
            let now=if book.active.as_ref().is_some_and(|r|r.measured){self.shared.clock.now()}else{0};
            book.exit(id.into_u64(),now);}}
    }
    fn clone_span(&self,id:&Id)->Id {if id.into_u64()!=IGNORED{if let Some(mut book)=self.shared.lock(){book.clone_id(id.into_u64());}}id.clone()}
    fn try_close(&self,id:Id)->bool {if id.into_u64()==IGNORED{return true}
        self.shared.lock().is_some_and(|mut book|book.close(id.into_u64()))}
}

struct AttemptGuard<'a>{shared:&'a Shared,attempt:u64,previous:Option<(u64,u64)>,finished:bool}
impl AttemptGuard<'_> {
    fn complete(mut self,outcome:Outcome){if let Some(mut book)=self.shared.lock(){book.end(self.attempt,outcome);}self.finished=true;}
}
impl Drop for AttemptGuard<'_> {
    fn drop(&mut self){if !self.finished{if let Some(mut book)=self.shared.lock(){book.end(self.attempt,Outcome::Unwind);}}
        CONTEXT.with(|context|context.set(self.previous));}
}

pub(crate) struct Capture {shared:Arc<Shared>,dispatch:Dispatch,registered_before_first_attempt:Option<u64>,
    warmup_complete_before_first_attempt:Option<u64>,warmup_interest_rebuilt:bool,
    warmup_interest_rebuilds_before_first_attempt:Option<u64>}
impl Capture {
    pub(crate) fn new()->anyhow::Result<Self> {
        let id=NEXT_CAPTURE.fetch_update(Ordering::Relaxed,Ordering::Relaxed,|v|v.checked_add(1))
            .map_err(|_|anyhow::anyhow!("surface wait capture identity exhausted"))?;
        let shared=Arc::new(Shared{id,book:Mutex::new(Book::new()),clock:Clock::new(),
            foreign:AtomicU64::new(0),contention:AtomicU64::new(0),reentry:AtomicU64::new(0),registered_mask:AtomicU64::new(0)});
        let dispatch=Dispatch::new(Sink{shared:Arc::clone(&shared)});Ok(Self{shared,dispatch,registered_before_first_attempt:None,
            warmup_complete_before_first_attempt:None,warmup_interest_rebuilt:false,
            warmup_interest_rebuilds_before_first_attempt:None})
    }
    pub(crate) fn clear_pending_present(&mut self){if let Some(mut book)=self.shared.lock(){book.clear_pending();}}
    pub(crate) fn acquire<T,E>(&mut self,frame_id:u64,application_seconds:f64,
        acquire:impl FnOnce()->Result<T,E>)->Result<T,E> {
        self.acquire_inner(frame_id,application_seconds,true,acquire)
    }
    pub(crate) fn warmup_acquire<T,E>(&mut self,frame_id:u64,application_seconds:f64,
        acquire:impl FnOnce()->Result<T,E>)->Result<T,E> {
        self.acquire_inner(frame_id,application_seconds,false,acquire)
    }
    fn acquire_inner<T,E>(&mut self,frame_id:u64,application_seconds:f64,measured:bool,
        acquire:impl FnOnce()->Result<T,E>)->Result<T,E> {
        if CONTEXT.with(Cell::get).is_some(){self.shared.reentry.fetch_add(1,Ordering::Relaxed);return acquire()}
        let attempt=self.shared.lock().and_then(|mut book|if measured {book.begin(frame_id,application_seconds)}
            else {book.begin_warmup(frame_id,application_seconds)});
        let Some(attempt)=attempt else{return acquire()};
        if measured {
            self.warmup_interest_rebuilds_before_first_attempt.get_or_insert(u64::from(self.warmup_interest_rebuilt));
            self.registered_before_first_attempt.get_or_insert_with(||self.shared.registered_mask.load(Ordering::Relaxed));
            if self.warmup_complete_before_first_attempt.is_none(){
                self.warmup_complete_before_first_attempt=self.shared.lock().map(|book|book.warmup.successful_scope_complete);
            }
        }
        tracing::dispatcher::with_default(&self.dispatch,||{
            let previous=CONTEXT.with(|context|context.replace(Some((self.shared.id,attempt))));
            let guard=AttemptGuard{shared:&self.shared,attempt,previous,finished:false};
            if !measured&&!self.warmup_interest_rebuilt {
                // tracing-core's single-Dispatch fast path can cache Never when
                // a callsite first runs outside this scoped Dispatch. Refresh
                // once under the correct dispatcher, before real warmup work.
                // Never refresh during a measured attempt or change the filter.
                tracing::callsite::rebuild_interest_cache();
                self.warmup_interest_rebuilt=true;
            }
            let result=acquire();guard.complete(if result.is_ok(){Outcome::Success}else{Outcome::Error});result
        })
    }
    pub(crate) fn public_elapsed(&mut self,milliseconds:f64)->anyhow::Result<()> {
        self.shared.lock().ok_or_else(||anyhow::anyhow!("surface trace state unavailable"))?.public_elapsed(milliseconds)
    }
    pub(crate) fn presented(&mut self,sample_index:Option<usize>,begin_application_ms:Option<f64>,
        end_application_ms:f64)->anyhow::Result<()> {
        self.shared.lock().ok_or_else(||anyhow::anyhow!("surface trace state unavailable"))?
            .presented(sample_index,begin_application_ms,end_application_ms)
    }
    pub(crate) fn report(&self)->serde_json::Value {
        let Some(book)=self.shared.lock()else{return serde_json::json!({"enabled":true,"feature_enabled":true,
            "diagnostic_not_baseline":true,"status":"state_unavailable"})};
        let t=&book.totals;
        let mut report=serde_json::json!({"enabled":true,"feature_enabled":true,"diagnostic_not_baseline":true,
            "capture_epoch":self.shared.id,
            "warmup_interest_rebuilds":u64::from(self.warmup_interest_rebuilt),
            "warmup_interest_rebuilds_before_first_attempt":self.warmup_interest_rebuilds_before_first_attempt,
            "registered_scope_mask":self.shared.registered_mask.load(Ordering::Relaxed),
            "registered_scope_mask_before_first_attempt":self.registered_before_first_attempt,
            "warmup_successful_scope_complete_before_first_attempt":self.warmup_complete_before_first_attempt,
            "warmup":{"attempts":book.warmup.attempts,"successful_acquires":book.warmup.success,
                "native_errors":book.warmup.native_errors,"unwinds":book.warmup.unwinds,
                "successful_scope_complete_attempts":book.warmup.successful_scope_complete,
                "invalid_attempts":book.warmup.invalid,"scope_completion_counts":book.warmup.scope_counts,
                "callback_timing_enabled":false},
            "status":if book.active.is_some()||book.await_public.is_some(){"incomplete"}else{"complete"},
            "scope":"measured acquisition attempts only; all durations are CPU wall spans",
            "top_capacity":TOP,"span_capacity":SLOTS,"stack_capacity":DEPTH,
            "attempts":t.attempts,"successful_acquires":t.success,"native_errors":t.native_errors,"unwinds":t.unwinds,
            "scope_complete_attempts":t.scope_complete,"invalid_attempts":t.invalid,
            "successful_scope_complete_attempts":t.successful_scope_complete,"scope_completion_counts":t.scope_counts,
            "public_duration_count":t.public_count,"public_acquire_ms_total":t.public_ms,
            "scope_ms_totals":[t.ns[0]as f64/1e6,t.ns[1]as f64/1e6,t.ns[2]as f64/1e6],
            "scope_order":["core_acquire","prior_submission_wait","native_acquire"]});
        // Split the terminal-only object to stay within serde_json's macro
        // recursion budget without changing the crate-wide compiler limit.
        let tail=serde_json::json!({
            "thresholds_ms":THRESHOLDS_MS,"scope_at_least_counts":t.scope_at_least,"public_at_least_counts":t.public_at_least,
            "core_residual_ms_total":t.core_residual_ns as f64/1e6,
            "public_residual_ms_total":t.public_residual_ms,"presented_attempts":t.presented,
            "first_present_without_sample":t.first_present,"unstamped_successful_acquires":book.unstamped,
            "unassociated_presents":book.unassociated_presents,"lifecycle_errors":book.lifecycle_errors,
            "slot_overflow":book.slot_overflow,"serial_exhaustion":book.serial_exhaustion,
            "foreign_context_callbacks":self.shared.foreign.load(Ordering::Relaxed),
            "callback_contention":self.shared.contention.load(Ordering::Relaxed),
            "reentry_attempts":self.shared.reentry.load(Ordering::Relaxed),
            "live_span_handles":book.slots.iter().filter(|s|s.id!=0).count(),
            "top":book.top.iter().flatten().map(Observation::json).collect::<Vec<_>>(),
            "notes":["Exact pinned metadata filter; missing scope coverage is not zero-duration evidence.",
                "Child spans are nested, not additive frame coverage; residuals are unassigned wall time.",
                "No waited submission identity, GPU clock alignment, actual presentation mode or display timing.",
                "Collector storage is fixed; tracing callsite registration may initialize on first use. Pre-attempt mask7 confirms the three selected sites were registered beforehand.",
                "Warmup executes the same scoped lifecycle without callback clock reads; its scalar coverage is excluded from measured aggregates/top/present attachment.",
                "Feature-on capture-off retains tracing interest overhead; feature-off ordinary build has empty profiling macros."]});
        if let serde_json::Value::Object(tail)=tail {report.as_object_mut().unwrap().extend(tail);}
        report
    }
}

#[cfg(test)]
#[path="surface_wait_trace_tests.rs"]
mod tests;
