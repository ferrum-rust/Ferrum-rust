//! Explicit benchmark diagnostic. Fixed scalar records; no resource ownership.
use std::time::Instant;
pub(crate) const PHASE_NAMES:[&str;10]=[
    "upload_optional_hooks","upload_cache_key_scan","upload_bundle_release",
    "upload_entry_inclusive","upload_initial_construct","upload_cpu_payload_replace",
    "upload_buffer_create","upload_buffer_replace_inclusive","upload_buffer_write","upload_input_cleanup",
];
const LIMIT:usize=8;
#[derive(Clone,Copy)]
#[repr(usize)]
pub(super) enum Phase {Hooks,CacheScan,BundleDrop,Entry,Initial,CpuReplace,Create,Replace,Write,Cleanup}
#[derive(Clone,Copy,Default,serde::Serialize)]
pub(crate) struct Span {pub calls:u64,pub total_ms:f64,pub max_ms:f64}
impl Span {
    fn add(&mut self,ms:f64){self.calls+=1;self.total_ms+=ms;self.max_ms=self.max_ms.max(ms);}
    fn merge(&mut self,other:Self){self.calls+=other.calls;self.total_ms+=other.total_ms;self.max_ms=self.max_ms.max(other.max_ms);}
}
#[derive(Clone,Copy,Default,serde::Serialize)]
pub(crate) struct Buffer {
    pub old_capacity:u64,pub new_capacity:u64,pub submitted_bytes:u64,
    pub creates:u64,pub writes:u64,pub create_ms:f64,pub replace_inclusive_ms:f64,pub write_ms:f64,
}
#[derive(Clone,Copy,Default,serde::Serialize)]
pub(crate) struct Observation {
    pub sequence:u64,pub position:[i32;2],pub configuration_elapsed_ms:f64,pub total_ms:f64,
    pub phases:[Span;PHASE_NAMES.len()],
    pub vacant:bool,pub map_len_before:usize,pub map_capacity_before:usize,
    pub map_len_after:usize,pub map_capacity_after:usize,
    pub remembered_key_len:usize,pub cached_eyes_before:usize,pub cached_eyes_after:usize,
    pub cache_position_matched:bool,pub released_bundle_handles:usize,
    pub old_cpu_capacity_bytes:[u64;5],pub old_graph_present:bool,
    pub old_index_width:u8,pub new_index_width:u8,
    pub buffers:[Buffer;2],
}
impl Observation {
    pub(crate) fn exclusive_covered_ms(&self)->f64 {
        // Initial is a child of Entry, Create is a child of Replace.
        [Phase::Hooks,Phase::CacheScan,Phase::BundleDrop,Phase::Entry,Phase::CpuReplace,
            Phase::Replace,Phase::Write,Phase::Cleanup].into_iter()
            .map(|p|self.phases[p as usize].total_ms).sum()
    }
    fn valid(&self)->bool {
        self.total_ms.is_finite()&&self.total_ms>=0.&&self.configuration_elapsed_ms.is_finite()&&self.configuration_elapsed_ms>=0.
            &&self.phases.iter().all(|s|s.total_ms.is_finite()&&s.total_ms>=0.&&s.max_ms.is_finite()&&s.max_ms>=0.)
            &&self.exclusive_covered_ms().is_finite()
            &&self.buffers.iter().all(|b|[b.create_ms,b.replace_inclusive_ms,b.write_ms].into_iter().all(|ms|ms.is_finite()&&ms>=0.))
    }
}
pub(super) struct Trace {pub observation:Observation,started:Instant,body_end:Option<Instant>}
impl Trace {
    fn new(position:(i32,i32),configured:Instant)->Self {
        let started=Instant::now();Self{observation:Observation{position:[position.0,position.1],
            configuration_elapsed_ms:started.duration_since(configured).as_secs_f64()*1000.,..Default::default()},
            started,body_end:None}
    }
    pub(super) fn phase(&mut self,phase:Phase,started:Instant)->f64 {
        let ms=started.elapsed().as_secs_f64()*1000.;self.observation.phases[phase as usize].add(ms);ms
    }
    pub(super) fn created(&mut self,buffer:usize,started:Instant) {
        let ms=self.phase(Phase::Create,started);self.observation.buffers[buffer].creates+=1;
        self.observation.buffers[buffer].create_ms+=ms;
    }
    pub(super) fn replaced(&mut self,buffer:usize,started:Instant) {
        let ms=self.phase(Phase::Replace,started);self.observation.buffers[buffer].replace_inclusive_ms+=ms;
    }
    pub(super) fn written(&mut self,buffer:usize,started:Instant) {
        let ms=self.phase(Phase::Write,started);self.observation.buffers[buffer].writes+=1;
        self.observation.buffers[buffer].write_ms+=ms;
    }
    // No custom Drop: the caller observes cleanup AFTER the unchanged callee
    // scope has implicitly destroyed its remaining owned input arrays.
    pub(super) fn body_finished(&mut self){self.body_end=Some(Instant::now());}
    pub(super) fn returned(&mut self){if let Some(started)=self.body_end.take(){self.phase(Phase::Cleanup,started);}}
    pub(super) fn finish(mut self)->Observation {
        self.observation.total_ms=self.started.elapsed().as_secs_f64()*1000.;self.observation
    }
}
struct State {
    configured:Instant,configuration_application_elapsed_ms:Option<f64>,observations:u64,invalid_observations:u64,total:Span,
    phases:[Span;PHASE_NAMES.len()],covered_ms:f64,unattributed_ms:f64,
    slowest:[Option<Observation>;LIMIT],last:Option<Observation>,vacant:u64,
    creates:[u64;2],writes:[u64;2],submitted_bytes:[u64;2],
}
impl State {
    fn new()->Self {Self{configured:Instant::now(),configuration_application_elapsed_ms:None,observations:0,invalid_observations:0,total:Span::default(),
        phases:[Span::default();PHASE_NAMES.len()],covered_ms:0.,unattributed_ms:0.,
        slowest:[None;LIMIT],last:None,vacant:0,creates:[0;2],writes:[0;2],submitted_bytes:[0;2]}}
    fn record(&mut self,mut observation:Observation) {
        if !observation.valid(){self.invalid_observations+=1;self.last=None;return;}
        self.observations+=1;observation.sequence=self.observations;self.total.add(observation.total_ms);
        for (total,at) in self.phases.iter_mut().zip(observation.phases){total.merge(at);}
        let covered=observation.exclusive_covered_ms();self.covered_ms+=covered;
        self.unattributed_ms+=(observation.total_ms-covered).max(0.);self.vacant+=u64::from(observation.vacant);
        for (i,b) in observation.buffers.iter().enumerate(){self.creates[i]+=b.creates;self.writes[i]+=b.writes;self.submitted_bytes[i]+=b.submitted_bytes;}
        if let Some(slot)=self.slowest.iter().position(|old|old.is_none_or(|old|observation.total_ms>old.total_ms)) {
            for i in (slot+1..LIMIT).rev(){self.slowest[i]=self.slowest[i-1];}
            self.slowest[slot]=Some(observation);
        }
        self.last=Some(observation);
    }
}
#[derive(Default)]
pub(super) struct Capture {state:Option<Box<State>>}
impl Capture {
    pub(super) fn new(enabled:bool)->Self{Self{state:enabled.then(||Box::new(State::new()))}}
    pub(super) fn application_origin(&mut self,origin:Option<Instant>) {
        if let Some(s)=&mut self.state {s.configuration_application_elapsed_ms=origin
            .and_then(|origin|s.configured.checked_duration_since(origin)).map(|d|d.as_secs_f64()*1000.);}
    }
    pub(super) fn start(&self,p:(i32,i32))->Option<Trace>{self.state.as_ref().map(|s|Trace::new(p,s.configured))}
    pub(super) fn record(&mut self,observation:Observation){if let Some(state)=&mut self.state{state.record(observation);}}
    pub(super) fn take_last(&mut self)->Option<Observation>{self.state.as_mut().and_then(|s|s.last.take())}
    pub(super) fn report(&self)->serde_json::Value {
        let Some(s)=&self.state else{return serde_json::json!({"enabled":false,"diagnostic_not_baseline":false})};
        serde_json::json!({"enabled":true,"diagnostic_not_baseline":true,"phase_names":PHASE_NAMES,
            "configuration_application_elapsed_ms":s.configuration_application_elapsed_ms,
            "observations":s.observations,"invalid_observations":s.invalid_observations,"total":s.total,
            "phases":s.phases,"exclusive_covered_ms":s.covered_ms,"unattributed_ms":s.unattributed_ms,
            "vacant":s.vacant,"occupied":s.observations-s.vacant,"creates_vertex_index":s.creates,
            "writes_vertex_index":s.writes,"submitted_bytes_vertex_index":s.submitted_bytes,
            "slowest_limit":LIMIT,"slowest":s.slowest,
            "note":"Explicit diagnostic, not baseline. CPU wall timings, not completed GPU time. Includes startup/warmup uploads since renderer configuration. No resources or payloads retained: fixed top8 scalar observations plus one last observation; no per-upload container allocation. PhaseInitialConstruct is nested in EntryInclusive; BufferCreate is nested in BufferReplaceInclusive and excluded from exclusive coverage. BufferReplaceInclusive minus BufferCreate includes old-handle release, assignment and timer overhead, not pure driver free. InputCleanup is the unchanged callee's implicit mesh/index cleanup plus return/timer overhead. Old CPU capacities are bytes for chests/quads/distancekeys/u32indices/u16indices; graph presence is separate. Buffer arrays are vertex,index. Creates count replacement allocations only; each vacant observation additionally constructs two original256-byte buffers inside InitialConstruct. Upload-start application milliseconds equal configuration_application_elapsed_ms plus that observation configuration_elapsed_ms. Correlate with slow_present_intervals begin/end_application_ms or its first_measured_present_application_ms plus frame_seconds prefix sums; application_seconds is only an earlier render-context sample, not an interval endpoint. All exact anchors share App.started. A null anchor means that correlation is unavailable (e.g. headless fixture). Equal-duration top8 ties retain earlier observations. Extra clocks and report aggregation perturb captured runs; do not subtract them to claim a clean baseline."})
    }
}
#[cfg(test)]
#[path="upload_capture_tests.rs"]
mod tests;
