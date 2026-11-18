//! Opt-in consecutive-frame CPU shadow of exact opaque draw runs. Never creates GPU work.
use super::{ChunkMesh,ChunkPos};
use std::{ops::Range,time::Instant};
#[path="near_arena_shadow.rs"]
mod arena;
pub(super) use arena::Upload as ArenaUpload;
const MAX_VISIBLE:usize=4096;
const GROUP:usize=4;
const CACHE:usize=512;
const BUILDS:usize=4;
const RANGE_LIMIT:usize=16;

#[derive(Clone,Copy,Default,PartialEq,Eq,Debug)]
struct DrawRange {start:u32,end:u32}
#[derive(Clone,Copy,Default,PartialEq,Eq,Debug)]
struct Token {
    position:ChunkPos,revision:u64,mask:u16,width:u8,
    ranges:[DrawRange;RANGE_LIMIT],len:u8,
}
impl Token {fn draws(self)->u64 {u64::from(self.len)}}
trait Source {
    fn len(&self)->usize;
    fn key(&self,i:usize)->(ChunkPos,u16,u8);
    fn ranges(&self,i:usize)->&[Range<u32>];
}
struct Meshes<'a,'b>(&'a [(ChunkPos,&'b mut ChunkMesh)]);
impl Source for Meshes<'_,'_> {
    fn len(&self)->usize {self.0.len()}
    fn key(&self,i:usize)->(ChunkPos,u16,u8) {
        let(p,c)=&self.0[i];(*p,c.mask,if c.gpu.index_format==wgpu::IndexFormat::Uint16{2}else{4})
    }
    fn ranges(&self,i:usize)->&[Range<u32>] {&self.0[i].1.solid_ranges}
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Kind {Reused,Built,Direct}
#[derive(Default,Debug)]
struct Coverage {reused_groups:u64,reused_chunks:u64,reused_draws:u64,
    built_groups:u64,built_chunks:u64,built_draws:u64,direct_chunks:u64,direct_draws:u64}
struct Model {
    entries:ahash::AHashMap<ChunkPos,[Token;GROUP]>,
    ring:Vec<ChunkPos>,next:usize,meta:Option<(usize,wgpu::TextureFormat)>,
}
impl Model {
    fn new()->Self {Self {entries:ahash::AHashMap::with_capacity(CACHE),ring:Vec::with_capacity(CACHE),next:0,meta:None}}
    fn clear(&mut self){self.entries.clear();self.ring.clear();self.next=0;self.meta=None;}
    fn matches(&self,tokens:&[Token],at:usize)->bool {
        at+GROUP<=tokens.len() && self.entries.get(&tokens[at].position)
            .is_some_and(|key|key.as_slice()==&tokens[at..at+GROUP])
    }
    fn insert(&mut self,key:[Token;GROUP]) {
        let anchor=key[0].position;
        if !self.entries.contains_key(&anchor) {
            if self.ring.len()==CACHE {
                self.entries.remove(&self.ring[self.next]);self.ring[self.next]=anchor;
                self.next=(self.next+1)%CACHE;
            }else{self.ring.push(anchor);}
        }
        self.entries.insert(anchor,key);
        debug_assert!(self.entries.len()<=CACHE && self.ring.len()<=CACHE);
    }
    fn walk(&mut self,tokens:&[Token],eyes:usize,format:wgpu::TextureFormat,
        mut emit:impl FnMut(Kind,Range<usize>))->Coverage {
        if self.meta!=Some((eyes,format)){self.clear();self.meta=Some((eyes,format));}
        let mut result=Coverage::default();let mut at=0;let mut budget=BUILDS;
        while at<tokens.len() {
            let(kind,end)=if self.matches(tokens,at) {(Kind::Reused,at+GROUP)}
            else {
                // Do not build across an exact cached run one/two/three chunks
                // later. A direct prefix preserves its current encounter order.
                let next=(1..GROUP).find(|n|self.matches(tokens,at+n));
                if let Some(n)=next {(Kind::Direct,at+n)}
                else if budget!=0 && at+GROUP<=tokens.len() {
                    let key=tokens[at..at+GROUP].try_into().unwrap();
                    self.insert(key);budget-=1;(Kind::Built,at+GROUP)
                }else{(Kind::Direct,at+1)}
            };
            let chunks=(end-at)as u64;let draws=tokens[at..end].iter().map(|t|t.draws()).sum::<u64>();
            match kind {
                Kind::Reused=>{result.reused_groups+=1;result.reused_chunks+=chunks;result.reused_draws+=draws;}
                Kind::Built=>{result.built_groups+=1;result.built_chunks+=chunks;result.built_draws+=draws;}
                Kind::Direct=>{result.direct_chunks+=chunks;result.direct_draws+=draws;}
            }
            emit(kind,at..end);at=end;
        }
        result
    }
}
#[derive(Default,serde::Serialize)]
struct Stats {
    eligible_frames:u64,warmup_or_disabled_frames:u64,sample_attempts:u64,complete_samples:u64,
    too_many_visible:u64,unknown_payload_samples:u64,too_many_ranges:u64,
    whole_key_changed:u64,whole_eye_count_changed:u64,whole_length_changed:u64,
    whole_position_order_changed:u64,whole_aligned_mask_changed:u64,
    whole_existing_reusable:u64,whole_gate_wait_samples:u64,whole_rebuilt_samples:u64,whole_cached_samples:u64,
    visible_chunks:u64,nonempty_solid_chunks:u64,nonempty_solid_ranges:u64,
    prospective_repeated_buffer_bind_pairs:u64,actual_direct_repeated_buffer_bind_pairs:u64,
    reused_groups:u64,reused_chunks:u64,reused_draws:u64,
    modeled_build_groups:u64,modeled_build_chunks:u64,modeled_build_draws:u64,
    modeled_direct_chunks:u64,modeled_direct_draws:u64,
    payload_events:u64,remove_events:u64,binding_events:u64,clear_events:u64,
    remembered_upload_invalidations:u64,remembered_remove_invalidations:u64,active_gate_invalidations:u64,
    payload_capacity_rejections:u64,revision_wraps:u64,oversize_event_keys:u64,
    sample_work_ms:f64,max_sample_work_ms:f64,max_sample_frame:u64,
    event_hook_ms:f64,max_event_hook_ms:f64,completion_hook_ms:f64,
    max_visible:usize,max_solid_chunks:usize,max_model_entries:usize,
}
struct Pending {repeated_pairs:u64}
struct State {
    identities:ahash::AHashMap<ChunkPos,u64>,next_revision:u64,tokens:Vec<Token>,
    model:Model,arena:arena::Census,stats:Stats,pending:Option<Pending>,
}
impl State {
    fn new()->Self {Self {identities:ahash::AHashMap::with_capacity(MAX_VISIBLE),next_revision:0,
        tokens:Vec::with_capacity(MAX_VISIBLE),model:Model::new(),arena:arena::Census::new(),stats:Stats::default(),pending:None}}
    fn payload(&mut self,p:ChunkPos) {
        if self.next_revision==u64::MAX {
            self.identities.clear();self.model.clear();self.arena.clear();self.next_revision=0;self.stats.revision_wraps+=1;
        }
        self.next_revision+=1;
        if self.identities.contains_key(&p)||self.identities.len()<MAX_VISIBLE {
            self.identities.insert(p,self.next_revision);
        }else{self.stats.payload_capacity_rejections+=1;}
    }
    fn collect(&mut self,source:&impl Source)->Result<u64,u8> {
        self.tokens.clear();let mut ranges=0u64;
        for i in 0..source.len() {
            let(position,mask,width)=source.key(i);
            let Some(&revision)=self.identities.get(&position)else{return Err(1)};
            let mut token=Token {position,revision,mask,width,..Default::default()};
            if source.ranges(i).len()>RANGE_LIMIT {return Err(2)}
            for r in source.ranges(i).iter().filter(|r|!r.is_empty()) {
                if usize::from(token.len)==RANGE_LIMIT{return Err(2)}
                token.ranges[usize::from(token.len)]=DrawRange{start:r.start,end:r.end};token.len+=1;
            }
            if token.len!=0 {ranges+=token.draws();self.tokens.push(token);}
        }
        Ok(ranges)
    }
    fn sample(&mut self,frame:u64,eyes:usize,format:wgpu::TextureFormat,
        previous:&[(ChunkPos,u16)],previous_eyes:usize,matching_frames:u8,has_bundles:bool,source:&impl Source) {
        let started=Instant::now();self.pending=None;self.stats.sample_attempts+=1;
        // This independent census counts unknown/malformed records as direct
        // fallbacks, even when the older bundle-reuse shadow rejects a sample.
        self.arena.sample(source,&self.identities,eyes);
        if source.len()>MAX_VISIBLE || previous.len()>MAX_VISIBLE {
            self.stats.too_many_visible+=1;self.model.clear();
        }else{
            self.sample_bounded(eyes,format,previous,previous_eyes,matching_frames,has_bundles,source);
        }
        let ms=started.elapsed().as_secs_f64()*1000.;self.stats.sample_work_ms+=ms;
        if ms>self.stats.max_sample_work_ms {self.stats.max_sample_work_ms=ms;self.stats.max_sample_frame=frame;}
    }
    fn sample_bounded(&mut self,eyes:usize,format:wgpu::TextureFormat,previous:&[(ChunkPos,u16)],
        previous_eyes:usize,matching_frames:u8,has_bundles:bool,source:&impl Source) {
        let eye_changed=previous_eyes!=eyes;let len_changed=previous.len()!=source.len();
        let mut position_changed=false;let mut mask_changed=false;
        for (i,&(p,mask)) in previous.iter().enumerate().take(source.len()) {
            let(current,current_mask,_)=source.key(i);
            position_changed|=p!=current;mask_changed|=p==current&&mask!=current_mask;
        }
        let same=!(eye_changed||len_changed||position_changed||mask_changed);
        self.stats.whole_key_changed+=u64::from(!same);
        self.stats.whole_eye_count_changed+=u64::from(eye_changed);
        self.stats.whole_length_changed+=u64::from(len_changed);
        self.stats.whole_position_order_changed+=u64::from(position_changed);
        self.stats.whole_aligned_mask_changed+=u64::from(mask_changed);
        self.stats.whole_existing_reusable+=u64::from(same&&has_bundles);
        self.stats.whole_gate_wait_samples+=u64::from(same&&!has_bundles&&matching_frames<2&&!previous.is_empty());
        let ranges=match self.collect(source) {
            Ok(n)=>n,Err(reason)=>{
                if reason==1{self.stats.unknown_payload_samples+=1;}else{self.stats.too_many_ranges+=1;}
                self.model.clear();return;
            }
        };
        let chunks=self.tokens.len()as u64;
        self.stats.complete_samples+=1;self.stats.visible_chunks+=source.len()as u64;
        self.stats.nonempty_solid_chunks+=chunks;self.stats.nonempty_solid_ranges+=ranges;
        self.stats.prospective_repeated_buffer_bind_pairs+=(ranges-chunks)*eyes as u64;
        self.stats.max_visible=self.stats.max_visible.max(source.len());
        self.stats.max_solid_chunks=self.stats.max_solid_chunks.max(self.tokens.len());
        let coverage=self.model.walk(&self.tokens,eyes,format,|_,_|{});
        self.stats.reused_groups+=coverage.reused_groups;self.stats.reused_chunks+=coverage.reused_chunks;
        self.stats.reused_draws+=coverage.reused_draws;
        self.stats.modeled_build_groups+=coverage.built_groups;self.stats.modeled_build_chunks+=coverage.built_chunks;
        self.stats.modeled_build_draws+=coverage.built_draws;
        self.stats.modeled_direct_chunks+=coverage.direct_chunks;self.stats.modeled_direct_draws+=coverage.direct_draws;
        debug_assert_eq!(coverage.reused_draws+coverage.built_draws+coverage.direct_draws,ranges);
        self.stats.max_model_entries=self.stats.max_model_entries.max(self.model.entries.len());
        self.pending=Some(Pending{repeated_pairs:(ranges-chunks)*eyes as u64});
    }
}
#[derive(Default)]
pub(super) struct Shadow {state:Option<Box<State>>}
impl Shadow {
    pub(super) fn new(enabled:bool)->Self {Self{state:enabled.then(||Box::new(State::new()))}}
    pub(super) fn enabled(&self)->bool {self.state.is_some()}
    pub(super) fn begin_frame(&mut self,enabled:bool)->bool {
        let Some(state)=&mut self.state else{return false};
        state.pending=None;state.arena.cancel_pending();
        if !enabled {state.stats.warmup_or_disabled_frames+=1;state.model.clear();return false;}
        state.stats.eligible_frames+=1;true
    }
    pub(super) fn payload_changed(&mut self,p:ChunkPos,previous:&[(ChunkPos,u16)],active_gate:bool) {
        let Some(state)=&mut self.state else{return};let start=Instant::now();
        state.stats.payload_events+=1;
        let remembered=if previous.len()>MAX_VISIBLE {state.stats.oversize_event_keys+=1;false}
            else{previous.iter().any(|&(q,_)|p==q)};
        state.stats.remembered_upload_invalidations+=u64::from(remembered);
        state.stats.active_gate_invalidations+=u64::from(remembered&&active_gate);
        state.payload(p);Self::event_time(state,start);
    }
    pub(super) fn geometry_uploaded(&mut self,p:ChunkPos,upload:ArenaUpload) {
        let Some(state)=&mut self.state else{return};let start=Instant::now();
        state.arena.upload(p,state.identities.get(&p).copied(),upload);
        Self::event_time(state,start);
    }
    pub(super) fn removed(&mut self,p:ChunkPos,previous:&[(ChunkPos,u16)],active_gate:bool) {
        let Some(state)=&mut self.state else{return};let start=Instant::now();
        state.stats.remove_events+=1;state.identities.remove(&p);state.arena.removed(p);
        let remembered=if previous.len()>MAX_VISIBLE {state.stats.oversize_event_keys+=1;false}
            else{previous.iter().any(|&(q,_)|p==q)};
        state.stats.remembered_remove_invalidations+=u64::from(remembered);
        state.stats.active_gate_invalidations+=u64::from(remembered&&active_gate);
        Self::event_time(state,start);
    }
    pub(super) fn bindings_changed(&mut self) {
        let Some(state)=&mut self.state else{return};let start=Instant::now();
        state.stats.binding_events+=1;state.model.clear();Self::event_time(state,start);
    }
    pub(super) fn cleared(&mut self) {
        let Some(state)=&mut self.state else{return};let start=Instant::now();
        state.stats.clear_events+=1;state.identities.clear();state.tokens.clear();state.model.clear();state.arena.clear();
        state.pending=None;Self::event_time(state,start);
    }
    fn event_time(state:&mut State,start:Instant) {
        let ms=start.elapsed().as_secs_f64()*1000.;state.stats.event_hook_ms+=ms;
        state.stats.max_event_hook_ms=state.stats.max_event_hook_ms.max(ms);
    }
    pub(super) fn sample(&mut self,frame:u64,eyes:usize,format:wgpu::TextureFormat,
        previous:&[(ChunkPos,u16)],previous_eyes:usize,matching_frames:u8,has_bundles:bool,
        visible:&[(ChunkPos,&mut ChunkMesh)]) {
        if let Some(state)=&mut self.state {
            state.sample(frame,eyes,format,previous,previous_eyes,matching_frames,has_bundles,&Meshes(visible));
        }
    }
    pub(super) fn completed(&mut self,rebuilt:bool,cached:bool) {
        let Some(state)=&mut self.state else{return};
        let start=Instant::now();state.arena.completed(cached);
        if let Some(pending)=state.pending.take() {
            state.stats.whole_rebuilt_samples+=u64::from(rebuilt);state.stats.whole_cached_samples+=u64::from(cached);
            if !cached {state.stats.actual_direct_repeated_buffer_bind_pairs+=pending.repeated_pairs;}
        }
        state.stats.completion_hook_ms+=start.elapsed().as_secs_f64()*1000.;
    }
    pub(super) fn report(&self)->serde_json::Value {
        let Some(state)=&self.state else{return serde_json::json!({"enabled":false,"diagnostic_not_baseline":false})};
        serde_json::json!({"enabled":true,"diagnostic_not_baseline":true,"stats":&state.stats,
            "sample_every_enabled_render_frames":1,"max_visible_and_identities":MAX_VISIBLE,
            "group_chunks":GROUP,"model_entries_limit":CACHE,"model_builds_per_sample":BUILDS,
            "range_limit_per_chunk":RANGE_LIMIT,"resident_payload_identities":state.identities.len(),
            "current_model_entries":state.model.entries.len(),"token_capacity":state.tokens.capacity(),
            "model_ring_capacity":state.model.ring.capacity(),"arena_ordered_runs":state.arena.report(),
            "solid_ranges_per_solid_chunk":if state.stats.nonempty_solid_chunks==0{None}else{
                Some(state.stats.nonempty_solid_ranges as f64/state.stats.nonempty_solid_chunks as f64)},
            "prospective_repeated_vertex_plus_index_bind_calls":state.stats.prospective_repeated_buffer_bind_pairs.saturating_mul(2),
            "actual_direct_repeated_vertex_plus_index_bind_calls":state.stats.actual_direct_repeated_buffer_bind_pairs.saturating_mul(2),
            "note":"Opt-in diagnostic CPU work; no GPU algorithm changes. Samples every consecutive enabled render-profile frame; screenshots/warmup are excluded and reset only the bundle-reuse model; virtual arena placement retains actual upload history until world clear. Counts are frame observations, not distinct chunks; failed render attempts are not retrospectively removed. Whole-key reasons compare the actual cache key immediately before prepare on bounded attempts, even if later identity collection fails; reasons overlap, aligned-mask counts only matching positions at the same slot. Coverage and bind counts use complete_samples only. Payload/removal/binding event counters and overhead include startup/warmup. Oversized remembered keys skip event-membership classification; those events are separately counted. Model caches exact consecutive4-chunk opaque runs with complete ranges/mask/payload revision/index width,512-entry FIFO and at most4 modeled builds per sample. Nonempty ranges/chunks count the original opaque draw stream; each repeated pair is one redundant vertex plus one redundant index bind, multiplied by the active eye count; actual-direct counts exclude samples using existing whole-list bundles. Modeled builds become eligible on the next frame; coverage is only a CPU shadow of one FIFO/group policy, not measured GPU reuse or latency. Oversize/unknown inputs fail closed and reset only the shadow. Reported wall overhead covers sample/event/completion bodies, excludes branch/timer/report serialization overhead, and must not be subtracted to claim a clean baseline. No driver/bundle allocation or retained GPU object is modeled. arena_ordered_runs is a separate bounded immediate-free placement census; its event/sample/completion work is included in these diagnostic overhead timers."})
    }
}
#[cfg(test)]
#[path="near_opaque_shadow_tests.rs"]
mod tests;

