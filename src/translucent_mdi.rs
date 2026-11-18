//! Optional exact-order translucent MDI. Direct is the ordinary/default path.
//! This owns draw-plan scratch and a DISTINCT argument buffer, never geometry.
use super::{ChunkMesh,ChunkPos,Renderer,render_profile,near_arena,near_arena_plan};
use super::near_arena::{IndexWidth,Recording};
use super::near_arena_plan::{Backing,BoundedPlan,Command,DrawLimits,Row,Source};
use std::{cell::Cell,ops::Range};
use serde::Serialize;
pub(super) const MAX_RECORDS:usize=4096;
const ARGUMENT_BYTES:u64=(MAX_RECORDS*near_arena_plan::ARGUMENT_BYTES)as u64;

#[derive(Clone,Copy,Debug,Default,PartialEq,Eq,clap::ValueEnum)]
pub(crate) enum Mode {#[default] Direct,Mdi}

pub(super) struct Info {pub backing:Backing,pub width:IndexWidth,pub range:Range<u32>}
pub(super) trait Rows {fn len(&self)->usize;fn info(&self,index:usize)->Info;}
pub(super) struct VisibleRows<'v,'b>{rows:&'v[(ChunkPos,&'b mut ChunkMesh)]}
impl<'v,'b> VisibleRows<'v,'b>{pub(super) fn new(rows:&'v[(ChunkPos,&'b mut ChunkMesh)])->Self{Self{rows}}}
impl Rows for VisibleRows<'_,'_>{
    fn len(&self)->usize{self.rows.len()}
    fn info(&self,index:usize)->Info{let chunk=&self.rows[index].1;
        Info{backing:chunk.gpu.arena_token.map_or(Backing::Dedicated,Backing::Arena),
            width:if chunk.gpu.index_format==wgpu::IndexFormat::Uint16{IndexWidth::U16}else{IndexWidth::U32},
            range:chunk.translucent_range()}}
}
pub(super) fn reverse_index(len:usize,index:usize)->Option<usize>{len.checked_sub(index.checked_add(1)?)}
pub(super) fn fill_ranges<R:Rows>(rows:&R,ranges:&mut Vec<Range<u32>>,limit:usize)->bool {
    ranges.clear();
    if rows.len()>limit||rows.len()>ranges.capacity(){return false}
    ranges.extend((0..rows.len()).rev().map(|index|rows.info(index).range));true
}
pub(super) struct ReverseSource<'s,'r,R:Rows>{pub rows:&'s R,pub ranges:&'r[Range<u32>]}
impl<R:Rows> Source for ReverseSource<'_,'_,R>{
    fn len(&self)->usize{self.ranges.len()}
    fn row(&self,index:usize)->Row<'_>{
        let original=reverse_index(self.rows.len(),index).expect("reverse source index");
        let info=self.rows.info(original);
        Row{backing:info.backing,width:info.width,ranges:&self.ranges[index..index+1]}
    }
}

#[derive(Default,Serialize)]
struct Counts {frames:u64,prepared_frames:u64,empty_frames:u64,distance_fallback_frames:u64,
    arena_fallback_frames:u64,opaque_plan_fallback_frames:u64,record_limit_fallback_frames:u64,
    planned_records:u64,planned_direct_calls:u64,planned_multi_calls:u64,planned_multi_records:u64,
    argument_uploads:u64,argument_bytes:u64}
#[derive(Clone,Copy,Default,Serialize)]
pub(super) struct Issued {eyes:u64,cached_eyes:u64,logical_records:u64,direct_calls:u64,multi_calls:u64,multi_records:u64}
pub(super) fn direct_eye(issued:&Cell<Issued>,calls:u64){let mut s=issued.get();s.eyes+=1;s.logical_records+=calls;s.direct_calls+=calls;issued.set(s);}
pub(super) fn cached_eye(issued:&Cell<Issued>){let mut s=issued.get();s.cached_eyes+=1;issued.set(s);}

pub(super) struct State {
    plan:BoundedPlan<MAX_RECORDS>,arguments:wgpu::Buffer,ranges:Vec<Range<u32>>,
    counts:Counts,issued:Cell<Issued>,last_plan_has_mdi:bool,
    #[cfg(test)] native_limit:u32,
    #[cfg(test)] record_limit:usize,
}
pub(super) struct Builder<'a>{plan:&'a mut BoundedPlan<MAX_RECORDS>,arguments:&'a wgpu::Buffer,
    counts:&'a mut Counts,issued:&'a Cell<Issued>,last_plan_has_mdi:&'a mut bool,limits:DrawLimits}
pub(super) struct Frame<'a,'s,R:Rows>{
    pub builder:Option<Builder<'a>>,pub source:ReverseSource<'s,'a,R>,pub issued:&'a Cell<Issued>,
}
pub(super) struct DrawPlan<'p,'r,'a,S:Source>{
    prepared:near_arena_plan::Prepared<'p,'r,'a,wgpu::Buffer,S,MAX_RECORDS>,
    arguments:&'p wgpu::Buffer,issued:&'p Cell<Issued>,
}
impl State {
    fn new(device:&wgpu::Device)->anyhow::Result<Self>{
        let plan=BoundedPlan::<MAX_RECORDS>::new().map_err(|e|anyhow::anyhow!("alpha plan allocation: {e:?}"))?;
        let arguments=super::renderer_arena::checked_buffer(device,ARGUMENT_BYTES,
            wgpu::BufferUsages::INDIRECT|wgpu::BufferUsages::COPY_DST,"separate exact-order translucent arguments")
            .map_err(anyhow::Error::msg)?;
        let mut ranges=Vec::new();ranges.try_reserve_exact(MAX_RECORDS)?;
        Ok(Self{plan,arguments,ranges,counts:Counts::default(),issued:Cell::new(Issued::default()),last_plan_has_mdi:false,
            #[cfg(test)] native_limit:u32::MAX,#[cfg(test)] record_limit:MAX_RECORDS})
    }
    pub(super) fn frame<'a,'s,R:Rows>(&'a mut self,rows:&'s R,distance:u32,
        limits:Option<DrawLimits>,opaque_available:bool)->Frame<'a,'s,R>{
        self.counts.frames+=1;self.last_plan_has_mdi=false;self.ranges.clear();
        #[cfg(test)] let record_limit=self.record_limit;
        #[cfg(not(test))] let record_limit=MAX_RECORDS;
        let admitted=if distance!=128 {self.counts.distance_fallback_frames+=1;false}
            else if limits.is_none(){self.counts.arena_fallback_frames+=1;false}
            else if !opaque_available{self.counts.opaque_plan_fallback_frames+=1;false}
            else if !fill_ranges(rows,&mut self.ranges,record_limit){self.counts.record_limit_fallback_frames+=1;false}
            else{true};
        let builder=if admitted {
            let limits=limits.unwrap();
            #[cfg(test)] let limits=DrawLimits::negotiated(limits.multi_draw_enabled(),true,true,Some(limits.max_draws().min(self.native_limit)));
            Some(Builder{plan:&mut self.plan,arguments:&self.arguments,counts:&mut self.counts,
                issued:&self.issued,last_plan_has_mdi:&mut self.last_plan_has_mdi,limits})
        }else{None};
        Frame{builder,source:ReverseSource{rows,ranges:&self.ranges},issued:&self.issued}
    }
    fn report(&self)->serde_json::Value{
        serde_json::json!({"enabled":true,"mode":"mdi","eligible_render_distance":128,
            "max_records":MAX_RECORDS,"argument_buffer_bytes":ARGUMENT_BYTES,
            "range_capacity":self.ranges.capacity(),"plan_reserved_bytes":self.plan.reserved_storage_bytes(),
            "last_plan_has_mdi":self.last_plan_has_mdi,"counts":self.counts,"issued":self.issued.get(),
            "note":"Explicit exact-order alpha draw experiment. Counts cover world preparation/command recording, including warmup/screenshots, not completed submissions or GPU execution. prepared_frames includes empty_frames. Issued eyes excludes cached_eyes; direct/MDI counters are actual per-eye command recording and logical_records=direct_calls+multi_records. Cached fallback records are excluded. Direct is ordinary default. Separate fixed argument storage is written once before either eye; opaque arguments and all geometry/index uploads are unchanged. Only adjacent compatible shared-page records of original reverse visible order are grouped. No page sorting or visual tradeoff."})
    }
}
impl Builder<'_>{
    pub(super) fn build<'p,'r,'a,S:Source>(&'p mut self,recording:&'r Recording<'a,wgpu::Buffer>,
        source:&'r S,queue:&wgpu::Queue)->Option<DrawPlan<'p,'r,'a,S>>{
        match self.plan.build(near_arena::Mode::ArenaMdi,self.limits,recording,source){
            Ok(prepared)=>{
                let s=prepared.summary();self.counts.prepared_frames+=1;self.counts.empty_frames+=u64::from(s.records==0);
                self.counts.planned_records+=s.records as u64;self.counts.planned_direct_calls+=s.direct_calls as u64;
                self.counts.planned_multi_calls+=s.multi_calls as u64;self.counts.planned_multi_records+=s.multi_records as u64;
                *self.last_plan_has_mdi=s.multi_calls>0;
                if s.multi_calls>0 {
                    let bytes=prepared.argument_bytes();assert!(bytes.len() as u64<=ARGUMENT_BYTES);
                    queue.write_buffer(self.arguments,0,bytes);self.counts.argument_uploads+=1;self.counts.argument_bytes+=bytes.len() as u64;
                }
                Some(DrawPlan{prepared,arguments:self.arguments,issued:self.issued})
            },
            Err(near_arena_plan::Error::TooManyRecords)=>{self.counts.record_limit_fallback_frames+=1;None},
            Err(error)=>panic!("unsafe/stale translucent draw plan: {error:?}"),
        }
    }
}
impl<S:Source> DrawPlan<'_,'_,'_,S>{
    pub(super) fn draw<'pass>(&'pass self,pass:&mut wgpu::RenderPass<'pass>,
        visible:&'pass[(ChunkPos,&mut ChunkMesh)],profile:&mut render_profile::Frame){
        let mut issued=self.issued.get();issued.eyes+=1;
        for command in self.prepared.commands(){match command{
            Command::Direct{row,range}=>{
                let original=reverse_index(visible.len(),*row).expect("alpha direct row");
                visible[original].1.gpu.draw_range(pass,range.clone());issued.direct_calls+=1;issued.logical_records+=1;
                if profile.enabled{profile.counters.near_translucent_direct_draws+=1;}
            },
            Command::Multi{row,binding,first_arg,count}=>{
                let b=self.prepared.binding(*row).expect("alpha live token").expect("alpha shared run");
                assert_eq!((binding.arena_id,binding.vertex_page,binding.index_page),(b.arena_id,b.vertex_page,b.index_page));
                pass.set_vertex_buffer(0,b.vertex.slice(..));pass.set_index_buffer(b.index.slice(..),
                    if binding.width==IndexWidth::U16{wgpu::IndexFormat::Uint16}else{wgpu::IndexFormat::Uint32});
                pass.multi_draw_indexed_indirect(self.arguments,u64::from(*first_arg)*20,*count);
                issued.multi_calls+=1;issued.multi_records+=u64::from(*count);issued.logical_records+=u64::from(*count);
                if profile.enabled{profile.counters.near_translucent_mdi_calls+=1;profile.counters.near_translucent_mdi_records+=*count as usize;}
            },
        }}
        self.issued.set(issued);
    }
}

impl Renderer {
    pub(crate) fn configure_translucent_mdi(&mut self,mode:Mode)->anyhow::Result<()> {
        if mode==Mode::Direct {anyhow::ensure!(self.translucent_mdi.is_none(),"translucent experiment mode is immutable");return Ok(())}
        anyhow::ensure!(self.chunks.is_empty(),"configure translucent MDI before world geometry");
        anyhow::ensure!(self.gpu_timestamps.is_none()&&self.transparent_staging.is_none(),"translucent MDI conflicts with GPU timestamp/staging trial");
        if self.translucent_mdi.is_none(){self.translucent_mdi=Some(State::new(&self.device)?);}
        Ok(())
    }
    pub(crate) fn translucent_mdi_report(&self)->serde_json::Value {
        self.translucent_mdi.as_ref().map_or_else(||serde_json::json!({"enabled":false,"mode":"direct"}),State::report)
    }
    #[cfg(test)]
    pub(super) fn translucent_mdi_test_limits(&mut self,native_max:u32,record_cap:usize){
        assert!(native_max>=2&&record_cap>0&&record_cap<=MAX_RECORDS);
        let state=self.translucent_mdi.as_mut().expect("configured alpha MDI");assert_eq!(state.counts.frames,0);
        state.native_limit=native_max;state.record_limit=record_cap;
    }
}

#[cfg(test)]
#[path="translucent_mdi_tests.rs"]
mod tests;
