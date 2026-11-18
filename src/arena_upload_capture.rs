//! Opt-in arena upload wall attribution. Scalar records only; no GPU ownership.
use std::time::Instant;
use super::upload_capture::Span;
pub(super) const NAMES:[&str;18]=[
    "state_mutex_acquire","cache_invalidation","admission_inclusive","checked_page_inclusive",
    "dedicated_fallback_write_inclusive","buffer_create_call","error_scope_resolution",
    "shared_write_inclusive","vertex_queue_write_call","index_queue_write_call","commit",
    "old_cpu_payload_replace","old_gpu_handle_replace","new_cpu_metadata","input_cleanup",
    "old_token_release","explicit_dedicated_control_inclusive","map_insert",
];
#[derive(Clone,Copy)]
#[repr(usize)]
pub(super) enum Phase {Mutex,Cache,Admission,Page,Fallback,Create,Resolve,SharedWrite,VertexWrite,
    IndexWrite,Commit,CpuReplace,GpuReplace,NewMetadata,Cleanup,Release,Control,MapInsert}
#[derive(Clone,Copy,Default,serde::Serialize)]
pub(super) struct Observation {
    pub sequence:u64,pub position:[i32;2],pub configuration_elapsed_ms:f64,pub total_ms:f64,
    pub phases:[Span;NAMES.len()],pub vacant:bool,pub old_shared:bool,pub new_shared:bool,
    pub old_capacity:[u64;2],pub new_capacity:[u64;2],pub submitted_bytes:[u64;2],pub index_width:u8,
    pub old_cpu_capacity_bytes:[u64;5],pub old_graph_present:bool,
    pub map_len_before:usize,pub map_capacity_before:usize,pub map_capacity_after:usize,
    pub page_attempts:[u64;3],pub page_attempted_bytes:[u64;3],pub buffer_creates:u64,
    pub queue_writes:[u64;2],pub fallback:bool,pub fallback_reason:u8,pub explicit_dedicated:bool,
    pub detailed_buffer_calls_measured:bool,
}
fn add(span:&mut Span,ms:f64){span.calls+=1;span.total_ms+=ms;span.max_ms=span.max_ms.max(ms);}
fn merge(span:&mut Span,other:Span){span.calls+=other.calls;span.total_ms+=other.total_ms;span.max_ms=span.max_ms.max(other.max_ms);}
impl Observation {
    pub(super) fn covered_ms(&self)->f64 {
        [Phase::Mutex,Phase::Cache,Phase::Admission,Phase::Fallback,Phase::SharedWrite,Phase::Commit,
            Phase::CpuReplace,Phase::GpuReplace,Phase::NewMetadata,Phase::Cleanup,Phase::Release,
            Phase::Control,Phase::MapInsert].into_iter().map(|p|self.phases[p as usize].total_ms).sum()
    }
    fn valid(&self)->bool {
        self.total_ms.is_finite()&&self.total_ms>=0.&&self.configuration_elapsed_ms.is_finite()
            &&self.configuration_elapsed_ms>=0.&&self.covered_ms().is_finite()
            &&self.phases.iter().all(|s|s.total_ms.is_finite()&&s.total_ms>=0.&&s.max_ms.is_finite()&&s.max_ms>=0.)
    }
}
pub(super) struct Trace {pub observation:Observation,started:Instant,body_end:Option<Instant>}
impl Trace {
    fn new(p:(i32,i32),configured:Instant)->Self {
        let started=Instant::now();Self{observation:Observation{position:[p.0,p.1],
            configuration_elapsed_ms:started.duration_since(configured).as_secs_f64()*1000.,..Default::default()},started,body_end:None}
    }
    pub(super) fn phase(&mut self,p:Phase,started:Instant){add(&mut self.observation.phases[p as usize],started.elapsed().as_secs_f64()*1000.);}
    // Mark the same callee's last body statement. Its unused PreparedChunk
    // mesh/index arrays still drop implicitly before returned() is called.
    pub(super) fn body_finished(&mut self){self.body_end=Some(Instant::now());}
    pub(super) fn returned(&mut self){if let Some(at)=self.body_end.take(){self.phase(Phase::Cleanup,at);}}
    pub(super) fn finish(mut self)->Observation {self.observation.total_ms=self.started.elapsed().as_secs_f64()*1000.;self.observation}
}
struct State {
    configured:Instant,application_anchor_ms:Option<f64>,observations:u64,invalid:u64,total:Span,
    phases:[Span;NAMES.len()],covered_ms:f64,unattributed_ms:f64,slowest:[Option<Observation>;8],
    page_attempts:[u64;3],page_attempted_bytes:[u64;3],fallbacks:u64,shared:u64,controls:u64,
}
impl State {
    fn new()->Self {Self{configured:Instant::now(),application_anchor_ms:None,observations:0,invalid:0,total:Span::default(),
        phases:[Span::default();NAMES.len()],covered_ms:0.,unattributed_ms:0.,slowest:[None;8],
        page_attempts:[0;3],page_attempted_bytes:[0;3],fallbacks:0,shared:0,controls:0}}
    fn record(&mut self,mut o:Observation) {
        if !o.valid(){self.invalid+=1;return;}
        self.observations+=1;o.sequence=self.observations;add(&mut self.total,o.total_ms);
        for (total,span) in self.phases.iter_mut().zip(o.phases){merge(total,span);}
        let covered=o.covered_ms();self.covered_ms+=covered;self.unattributed_ms+=(o.total_ms-covered).max(0.);
        for i in 0..3 {self.page_attempts[i]+=o.page_attempts[i];self.page_attempted_bytes[i]+=o.page_attempted_bytes[i];}
        self.fallbacks+=u64::from(o.fallback);self.shared+=u64::from(o.new_shared);self.controls+=u64::from(o.explicit_dedicated);
        if let Some(at)=self.slowest.iter().position(|old|old.is_none_or(|old|o.total_ms>old.total_ms)) {
            for i in (at+1..8).rev(){self.slowest[i]=self.slowest[i-1];}self.slowest[at]=Some(o);
        }
    }
}
#[derive(Default)]
pub(super) struct Capture {state:Option<Box<State>>}
impl Capture {
    pub(super) fn new(enabled:bool,origin:Option<Instant>)->Self {
        Self{state:enabled.then(||{let mut state=Box::new(State::new());state.application_anchor_ms=origin
            .and_then(|origin|state.configured.checked_duration_since(origin)).map(|d|d.as_secs_f64()*1000.);state})}
    }
    pub(super) fn start(&self,p:(i32,i32))->Option<Trace>{self.state.as_ref().map(|s|Trace::new(p,s.configured))}
    pub(super) fn record(&mut self,o:Observation){if let Some(s)=&mut self.state{s.record(o);}}
    pub(super) fn report(&self)->serde_json::Value {
        let Some(s)=&self.state else{return serde_json::json!({"enabled":false,"diagnostic_not_baseline":false})};
        serde_json::json!({"enabled":true,"diagnostic_not_baseline":true,"phase_names":NAMES,
            "configuration_application_elapsed_ms":s.application_anchor_ms,"observations":s.observations,
            "invalid_observations":s.invalid,"total":s.total,"phases":s.phases,
            "exclusive_covered_ms":s.covered_ms,"unattributed_ms":s.unattributed_ms,
            "page_attempts_vertex_u16_u32":s.page_attempts,"page_attempted_bytes_vertex_u16_u32":s.page_attempted_bytes,
            "fallbacks":s.fallbacks,"shared_uploads":s.shared,"explicit_dedicated_controls":s.controls,
            "slowest_limit":8,"slowest":s.slowest,
            "note":"Explicit diagnostic of the actual arena upload path; it never suppresses automatic arena selection. Successful calls since configuration include startup/warmup; panic/error exits abort the benchmark and are not recorded. No geometry/GPU resources retained, fixed top8 scalar copies, no per-upload collection allocation. Start App milliseconds = configuration_application_elapsed_ms + configuration_elapsed_ms; end adds total_ms, using the same App.started origin as present begin/end anchors. Page is nested in Admission; Create/Resolve/QueueWrite are nested in Page, Fallback or SharedWrite and excluded from exclusive coverage. Resolve is CPU wall time in existing pop_error_scope futures, not proof of GPU completion, contention or a wait cause. MutexAcquire is lock-call wall time, not measured contention. CpuReplace includes prior CPU assignments/destruction; GpuReplace includes old handle assignment/destruction, not proof of driver free. A shared handle normally aliases a page retained by Arena. InputCleanup is implicit incoming mesh/index destruction after the unchanged callee body plus return/timer overhead. Final unlock/local teardown and uninstrumented work remain in unattributed time. Collector aggregation/top8 copying occurs after the total timestamp and is excluded from these child durations but remains in the existing outer visual_gpu_upload timer. Explicit Dedicated control uses its unchanged update routine as one inclusive span, with exact submitted size/width but detailed_buffer_calls_measured=false: its per-create/per-write counters are unmeasured, not evidence of zero work. Fallback codes:0 none,1 InvalidRequest,2 OwnerLimit,3 PageLimit,4 Fragmented,5 Metadata,6 checked page error; consistency errors still abort. Off path adds only an Option branch; no clocks/state allocation. This does not cover caller-side mesh stamps, all visual-job work, or Renderer::remove."})
    }
}
#[cfg(test)]
#[path="arena_upload_capture_tests.rs"]
mod tests;
