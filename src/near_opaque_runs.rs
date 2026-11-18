//! Opt-in benchmark experiment: exact consecutive opaque runs, unchanged draw order.
//! No terrain, visibility, transparency, graphics setting or simulation changes.
use super::{terrain_bundles::Eye, ChunkMesh, ChunkPos, DEPTH};
use std::{cell::Cell,collections::VecDeque,time::Instant};
const MAX_VISIBLE:usize=4096;
const GROUP:usize=4;
const CACHE:usize=512;
const BUILDS:usize=4;
const RANGE_LIMIT:usize=16;

#[derive(Clone,Copy,Default,Debug,PartialEq,Eq)]
struct DrawRange {start:u32,end:u32}
#[derive(Clone,Copy,Default,Debug,PartialEq,Eq)]
struct Key {
    position:ChunkPos,revision:u64,mask:u16,width:u8,
    ranges:[DrawRange;RANGE_LIMIT],len:u8,
}
impl Key {fn draws(&self)->u64{u64::from(self.len)}}
fn key(position:ChunkPos,revision:u64,mask:u16,width:u8,ranges:&[std::ops::Range<u32>],count:u32)->Option<Key>{
    if ranges.len()>RANGE_LIMIT{return None;}
    let mut key=Key{position,revision,mask,width,..Default::default()};
    for range in ranges.iter().filter(|range|!range.is_empty()){
        if range.end>count{return None;}
        key.ranges[usize::from(key.len)]=DrawRange{start:range.start,end:range.end};key.len+=1;
    }
    Some(key)
}
fn admits(visible:usize,eyes:usize)->bool{visible<=MAX_VISIBLE&&(1..=2).contains(&eyes)}
#[derive(Clone,Copy,Debug)]
struct Row {key:Key,visible_index:usize}
struct Entry<V> {key:[Key;GROUP],value:V}
struct Cache<V> {entries:ahash::AHashMap<ChunkPos,Entry<V>>,fifo:VecDeque<ChunkPos>}
impl<V> Cache<V> {
    fn new()->Self {Self {entries:ahash::AHashMap::with_capacity(CACHE),fifo:VecDeque::with_capacity(CACHE)}}
    fn clear(&mut self){self.entries.clear();self.fifo.clear();}
    fn matches(&self,rows:&[Row],at:usize)->bool {
        at+GROUP<=rows.len() && self.entries.get(&rows[at].key.position)
            .is_some_and(|entry|entry.key.iter().zip(&rows[at..at+GROUP]).all(|(a,b)|a==&b.key))
    }
    fn insert(&mut self,key:[Key;GROUP],value:V)->bool {
        let anchor=key[0].position;let mut evicted=false;
        if !self.entries.contains_key(&anchor) {
            if self.entries.len()==CACHE {
                let oldest=self.fifo.pop_front().unwrap();self.entries.remove(&oldest);evicted=true;
            }
            self.fifo.push_back(anchor);
        }
        self.entries.insert(anchor,Entry{key,value});
        debug_assert_eq!(self.entries.len(),self.fifo.len());debug_assert!(self.entries.len()<=CACHE);
        evicted
    }
    fn invalidate(&mut self,position:ChunkPos)->usize {
        let before=self.entries.len();
        self.entries.retain(|_,entry|!entry.key.iter().any(|key|key.position==position));
        let removed=before-self.entries.len();
        if removed!=0 {let entries=&self.entries;self.fifo.retain(|anchor|entries.contains_key(anchor));}
        removed
    }
}
#[derive(Clone)]
struct Bundles([Option<wgpu::RenderBundle>;2]);
enum Command<V> {
    Direct(usize),
    Cached {value:V,draws:u64,built:bool},
}
#[derive(Default,Debug)]
struct PlanCounts {built:u64,reused:u64,direct_chunks:u64,evicted:u64}
fn plan<V:Clone>(cache:&mut Cache<V>,rows:&[Row],commands:&mut Vec<Command<V>>,
    mut create:impl FnMut(&[Row])->V)->PlanCounts {
    commands.clear();let mut at=0;let mut budget=BUILDS;let mut result=PlanCounts::default();
    while at<rows.len() {
        if cache.matches(rows,at) {
            let entry=&cache.entries[&rows[at].key.position];
            // Own handles until this frame has been encoded: a later FIFO
            // replacement cannot make an earlier planned command disappear.
            commands.push(Command::Cached {value:entry.value.clone(),draws:entry.key.iter().map(Key::draws).sum(),built:false});
            result.reused+=1;at+=GROUP;continue;
        }
        if let Some(prefix)=(1..GROUP).find(|n|cache.matches(rows,at+n)) {
            for row in &rows[at..at+prefix]{commands.push(Command::Direct(row.visible_index));}
            result.direct_chunks+=prefix as u64;at+=prefix;continue;
        }
        if budget!=0 && at+GROUP<=rows.len() {
            let input=&rows[at..at+GROUP];let key=std::array::from_fn(|i|input[i].key);
            let value=create(input);let draws=key.iter().map(Key::draws).sum();
            result.evicted+=u64::from(cache.insert(key,value.clone()));
            commands.push(Command::Cached{value,draws,built:true});
            result.built+=1;budget-=1;at+=GROUP;
        }else{
            commands.push(Command::Direct(rows[at].visible_index));result.direct_chunks+=1;at+=1;
        }
    }
    debug_assert!(commands.len()<=MAX_VISIBLE);result
}
#[derive(Default,serde::Serialize)]
struct Measured {
    frames:u64,admitted_frames:u64,whole_cached_frames:u64,limit_fallbacks:u64,unknown_payload_fallbacks:u64,
    built_groups:u64,reused_groups:u64,direct_chunks:u64,fifo_evictions:u64,
    prepare_ms:f64,max_prepare_ms:f64,build_ms:f64,max_build_ms:f64,
    max_visible:usize,max_opaque_chunks:usize,max_commands:usize,max_entries:usize,
}
#[derive(Clone,Copy,Default,serde::Serialize)]
struct DrawCounts {eyes:u64,bundle_calls:u64,built_bundle_calls:u64,reused_bundle_calls:u64,bundled_draws:u64,direct_draws:u64,replay_ms:f64,max_replay_ms:f64}
impl DrawCounts {
    fn add(&mut self,r:Self){self.eyes+=r.eyes;self.bundle_calls+=r.bundle_calls;self.built_bundle_calls+=r.built_bundle_calls;
        self.reused_bundle_calls+=r.reused_bundle_calls;self.bundled_draws+=r.bundled_draws;self.direct_draws+=r.direct_draws;self.replay_ms+=r.replay_ms;self.max_replay_ms=self.max_replay_ms.max(r.max_replay_ms);}
}
#[derive(Default,serde::Serialize)]
struct Events {uploads:u64,removes:u64,bindings:u64,clears:u64,invalidated_entries:u64,
    identity_capacity_rejections:u64,revision_wraps:u64,hook_ms:f64,max_hook_ms:f64}
struct State {
    identities:ahash::AHashMap<ChunkPos,u64>,next_revision:u64,binding_epoch:u64,
    meta:Option<(usize,wgpu::TextureFormat,u64)>,cache:Cache<Bundles>,rows:Vec<Row>,commands:Vec<Command<Bundles>>,
    active:bool,measured:Measured,draw:Cell<DrawCounts>,events:Events,
}
impl State {
    fn new()->Self {Self {identities:ahash::AHashMap::with_capacity(MAX_VISIBLE),next_revision:0,binding_epoch:0,
        meta:None,cache:Cache::new(),rows:Vec::with_capacity(MAX_VISIBLE),commands:Vec::with_capacity(MAX_VISIBLE),
        active:false,measured:Measured::default(),draw:Cell::new(DrawCounts::default()),events:Events::default()}}
    fn discard_plan(&mut self){self.commands.clear();self.rows.clear();self.active=false;}
    fn clear_cache(&mut self){self.discard_plan();self.cache.clear();self.meta=None;}
    fn event_time(&mut self,start:Instant){let ms=start.elapsed().as_secs_f64()*1000.;
        self.events.hook_ms+=ms;self.events.max_hook_ms=self.events.max_hook_ms.max(ms);}
    fn payload(&mut self,position:ChunkPos,remove:bool){
        let start=Instant::now();self.discard_plan();
        self.events.invalidated_entries+=self.cache.invalidate(position) as u64;
        if remove {self.events.removes+=1;self.identities.remove(&position);}
        else{
            self.events.uploads+=1;
            if self.next_revision==u64::MAX {
                self.identities.clear();self.clear_cache();self.next_revision=0;self.events.revision_wraps+=1;
            }
            self.next_revision+=1;
            if self.identities.contains_key(&position)||self.identities.len()<MAX_VISIBLE {
                self.identities.insert(position,self.next_revision);
            }else{self.events.identity_capacity_rejections+=1;}
        }
        self.event_time(start);
    }
    fn collect(&mut self,chunks:&[(ChunkPos,&mut ChunkMesh)])->Result<(),bool>{
        self.rows.clear();
        for (visible_index,(position,chunk)) in chunks.iter().enumerate(){
            let Some(&revision)=self.identities.get(position) else{return Err(true)};
            let width=if chunk.gpu.index_format==wgpu::IndexFormat::Uint16{2}else{4};
            let Some(key)=key(*position,revision,chunk.mask,width,&chunk.solid_ranges,chunk.gpu.count) else{return Err(false)};
            if key.len!=0{self.rows.push(Row{key,visible_index});}
        }
        Ok(())
    }
    fn prepare(&mut self,device:&wgpu::Device,format:wgpu::TextureFormat,eyes:&[Eye<'_>],
        chunks:&[(ChunkPos,&mut ChunkMesh)],whole_cached:bool,measured:bool)->(f64,f64){
        let start=measured.then(Instant::now);let mut build_ms=0.;self.discard_plan();
        if measured{self.measured.frames+=1;}
        let meta=(eyes.len(),format,self.binding_epoch);
        if self.meta!=Some(meta){self.cache.clear();self.meta=Some(meta);}
        if whole_cached {
            if measured{self.measured.whole_cached_frames+=1;}
        }else if !admits(chunks.len(),eyes.len()) {
            self.cache.clear();if measured{self.measured.limit_fallbacks+=1;}
        }else if let Err(unknown)=self.collect(chunks){
            self.clear_cache();
            if measured{if unknown{self.measured.unknown_payload_fallbacks+=1;}else{self.measured.limit_fallbacks+=1;}}
        }else{
            let counts=plan(&mut self.cache,&self.rows,&mut self.commands,|rows|{
                let build_start=measured.then(Instant::now);
                let value=build(device,format,eyes,chunks,rows);
                if let Some(start)=build_start{build_ms+=start.elapsed().as_secs_f64()*1000.;}value
            });
            self.active=true;
            if measured{
                let m=&mut self.measured;m.admitted_frames+=1;m.built_groups+=counts.built;m.reused_groups+=counts.reused;
                m.direct_chunks+=counts.direct_chunks;m.fifo_evictions+=counts.evicted;m.build_ms+=build_ms;
                m.max_build_ms=m.max_build_ms.max(build_ms);m.max_visible=m.max_visible.max(chunks.len());
                m.max_opaque_chunks=m.max_opaque_chunks.max(self.rows.len());m.max_commands=m.max_commands.max(self.commands.len());
                m.max_entries=m.max_entries.max(self.cache.entries.len());
            }
        }
        let prepare_ms=start.map_or(0.,|start|start.elapsed().as_secs_f64()*1000.);
        if measured{self.measured.prepare_ms+=prepare_ms;self.measured.max_prepare_ms=self.measured.max_prepare_ms.max(prepare_ms);}
        (prepare_ms,build_ms)
    }
}
fn build(device:&wgpu::Device,format:wgpu::TextureFormat,eyes:&[Eye<'_>],
    chunks:&[(ChunkPos,&mut ChunkMesh)],rows:&[Row])->Bundles {
    let formats=[Some(format)];
    let descriptor=wgpu::RenderBundleEncoderDescriptor {label:Some("exact opaque run"),color_formats:&formats,
        depth_stencil:Some(wgpu::RenderBundleDepthStencil{format:DEPTH,depth_read_only:false,stencil_read_only:true}),
        sample_count:1,multiview:None};
    Bundles(std::array::from_fn(|i|{
        let eye=eyes.get(i)?;let mut encoder=device.create_render_bundle_encoder(&descriptor);
        encoder.set_pipeline(eye.opaque);encoder.set_bind_group(0,eye.bind,&[]);
        for row in rows{
            let chunk=&chunks[row.visible_index].1;
            for range in &row.key.ranges[..usize::from(row.key.len)]{
                encoder.set_vertex_buffer(0,chunk.gpu.vertex_slice());
                encoder.set_index_buffer(chunk.gpu.index_slice(),chunk.gpu.index_format);
                encoder.draw_indexed(range.start..range.end,0,0..1);
            }
        }
        Some(encoder.finish(&wgpu::RenderBundleDescriptor{label:Some("exact opaque run")}))
    }))
}
#[derive(Default)]
pub(super) struct Runs {state:Option<Box<State>>}
impl Runs {
    pub(super) fn new(enabled:bool)->Self{Self{state:enabled.then(||Box::new(State::new()))}}
    pub(super) fn payload_changed(&mut self,position:ChunkPos){if let Some(state)=&mut self.state{state.payload(position,false);}}
    pub(super) fn removed(&mut self,position:ChunkPos){if let Some(state)=&mut self.state{state.payload(position,true);}}
    pub(super) fn bindings_changed(&mut self){if let Some(state)=&mut self.state{
        let start=Instant::now();state.events.bindings+=1;state.clear_cache();state.binding_epoch=state.binding_epoch.wrapping_add(1);state.event_time(start);
    }}
    pub(super) fn cleared(&mut self){if let Some(state)=&mut self.state{
        let start=Instant::now();state.events.clears+=1;state.clear_cache();state.identities.clear();state.event_time(start);
    }}
    pub(super) fn prepare(&mut self,device:&wgpu::Device,format:wgpu::TextureFormat,eyes:&[Eye<'_>],
        chunks:&[(ChunkPos,&mut ChunkMesh)],whole_cached:bool,profile:&mut super::render_profile::Frame){
        if let Some(state)=&mut self.state{let (prepare,build)=state.prepare(device,format,eyes,chunks,whole_cached,profile.enabled);
            profile.stages_ms.near_opaque_run_prepare+=prepare;profile.stages_ms.near_opaque_run_build+=build;}
    }
    pub(super) fn draw<'a>(&'a self,pass:&mut wgpu::RenderPass<'a>,chunks:&'a[(ChunkPos,&mut ChunkMesh)],
        eye:&Eye<'a>,index:usize,profile:&mut super::render_profile::Frame)->bool {
        let Some(state)=&self.state else{return false};
        if !state.active{return false;}
        let replay_start=profile.timer();
        let mut counts=DrawCounts{eyes:1,..Default::default()};let mut restore=false;
        pass.set_pipeline(eye.opaque);pass.set_bind_group(0,eye.bind,&[]);
        for command in &state.commands {match command{
            Command::Cached{value,draws,built}=>{
                let bundle=value.0[index].as_ref().expect("prepared eye count matches draw");
                pass.execute_bundles([bundle]);restore=true;
                counts.bundle_calls+=1;counts.bundled_draws+=draws;
                if *built{counts.built_bundle_calls+=1;}else{counts.reused_bundle_calls+=1;}
            }
            Command::Direct(visible_index)=>{
                if restore{pass.set_pipeline(eye.opaque);pass.set_bind_group(0,eye.bind,&[]);restore=false;}
                let chunk=&chunks[*visible_index].1;
                for range in &chunk.solid_ranges{
                    counts.direct_draws+=u64::from(!range.is_empty());chunk.gpu.draw_range(pass,range.clone());
                }
            }
        }}
        // execute_bundles clears pass state. Preserve the exact caller contract
        // before distant terrain, sky, dynamics, overlays or transparency.
        if restore{pass.set_pipeline(eye.opaque);pass.set_bind_group(0,eye.bind,&[]);}
        if profile.enabled{
            counts.replay_ms=super::render_profile::elapsed(replay_start);counts.max_replay_ms=counts.replay_ms;
            profile.stages_ms.near_opaque_run_replay+=counts.replay_ms;
            profile.counters.near_direct_draws+=counts.direct_draws as usize;
            let mut total=state.draw.get();total.add(counts);state.draw.set(total);
        }
        true
    }
    pub(super) fn report(&self)->serde_json::Value{
        let Some(state)=&self.state else{return serde_json::json!({"enabled":false})};
        serde_json::json!({"enabled":true,"benchmark_only":true,"measured":&state.measured,"draw":state.draw.get(),
            "events":&state.events,"cache_entries":state.cache.entries.len(),"identity_count":state.identities.len(),
            "group_chunks":GROUP,"max_cache_entries":CACHE,"max_builds_per_frame":BUILDS,
            "max_visible_and_identities":MAX_VISIBLE,"max_ranges_per_chunk":RANGE_LIMIT,
            "row_capacity":state.rows.capacity(),"command_capacity":state.commands.capacity(),"fifo_capacity":state.cache.fifo.capacity(),
            "scope":"Opaque only; original whole-list bundles retain priority. Original ordered chunk/range draws, shaders, graphics and transparency are unchanged. Ordinary default is disabled.",
            "timing":"Measured prepare includes key collection, cache lookup, bounded bundle builds and prior-plan release. build_ms is nested. Event hooks include startup/warmup. Replay includes all cached and direct commands in the admitted path and is nested within world_near_opaque/world_draw_record; GPU backend replay at pass end remains separately in world_pass_finish. Do not sum these parent/child spans. These are real GPU command-cache arms, not shadow estimates.",
            "retention":"At most512 cache groups, each references4 currently resident payloads per eye. Upload/removal invalidates all containing groups and prior-plan handles immediately; no historical mesh retention. Current-frame plan clones bundle handles so up to4 FIFO-evicted groups remain alive through encoding. No extra vertex/index buffers are allocated."})
    }
}
#[cfg(test)]
#[path="near_opaque_runs_tests.rs"]
mod tests;
#[cfg(test)]
#[path="near_opaque_runs_gpu_tests.rs"]
mod gpu_tests;
