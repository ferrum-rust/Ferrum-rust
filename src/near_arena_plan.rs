//! Bounded same-frame ordered draw plan with no GPU ownership.
//! Input order is authoritative: never sort by allocation, page or index width.
use super::near_arena::{Binding,IndexWidth,Mode,Recording,Token};
use std::ops::Range;

pub const MAX_RECORDS:usize=65_536;
pub const ARGUMENT_BYTES:usize=20;
const VERTEX_STRIDE:u64=32;
const MAX_VERTEX_PAGE:u64=256*1024*1024;
const MAX_INDEX_PAGE:u64=64*1024*1024;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Backing {Dedicated,Arena(Token)}
pub struct Row<'a> {pub backing:Backing,pub width:IndexWidth,pub ranges:&'a[Range<u32>]}
/// Rows and ranges must be the existing phase encounter order. No
/// callbacks, interior mutation, queue writes or allocation publication here.
pub trait Source {fn len(&self)->usize;fn row(&self,index:usize)->Row<'_>;}

#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub struct DrawLimits {mdi:bool,max_draws:u32}
impl DrawLimits {
    /// Supply the actual requested Device feature, adapter downlevel flags,
    /// and SAME-adapter native Vulkan maxDrawIndirectCount. Missing native
    /// evidence disables multi-draw, never guesses a portable limit.
    pub fn negotiated(device_multi_draw:bool,indirect_execution:bool,base_vertex:bool,native_max:Option<u32>)->Self {
        let max_draws=native_max.unwrap_or(1).min(MAX_RECORDS as u32);
        Self{mdi:device_multi_draw&&indirect_execution&&base_vertex&&max_draws>=2,max_draws}
    }
    pub fn multi_draw_enabled(self)->bool{self.mdi}
    pub fn max_draws(self)->u32{self.max_draws}
}
#[repr(C)]
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq,bytemuck::Pod,bytemuck::Zeroable)]
pub struct Args {pub index_count:u32,pub instance_count:u32,pub first_index:u32,pub base_vertex:i32,pub first_instance:u32}
const _:()=assert!(std::mem::size_of::<Args>()==ARGUMENT_BYTES);
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct PageKey {pub arena_id:u64,pub vertex_page:u8,pub index_page:u8,pub width:IndexWidth}
#[derive(Clone,Debug,PartialEq,Eq)]
pub enum Command {
    /// Existing dedicated backing OR allocation-local slices, original range,
    /// base_vertex=0. This also handles one-record shared-page runs.
    Direct{row:usize,range:Range<u32>},
    /// Bind BOTH complete pages from representative row (never local slices).
    /// first_arg is an Args element index, converted to byte offset with *20.
    Multi{row:usize,binding:PageKey,first_arg:u32,count:u32},
}
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub struct Summary {pub records:usize,pub direct_calls:usize,pub multi_calls:usize,pub multi_records:usize,
    pub argument_records:usize,pub bounded_fallback:bool}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Error {Allocation,TooManyRecords,StaleArena{row:usize},UnsafeArena{row:usize},InvalidRange{row:usize}}
struct Run {row:usize,range:Range<u32>,binding:PageKey,first_arg:u32,count:u32}

pub struct BoundedPlan<const LIMIT:usize> {commands:Vec<Command>,args:Vec<Args>,summary:Summary}
/// Existing opaque specialization remains at its original 65,536-record bound.
pub type Plan = BoundedPlan<MAX_RECORDS>;
impl<const LIMIT:usize> BoundedPlan<LIMIT> {
    /// Configuration-time allocation only. Maximum65536 records, <=1.25MiB
    /// arguments plus a fixed-capacity command vector; no per-frame growth.
    pub fn new()->Result<Self,Error> {
        assert!(LIMIT>0&&LIMIT<=MAX_RECORDS);
        let mut commands=Vec::new();commands.try_reserve_exact(LIMIT).map_err(|_|Error::Allocation)?;
        let mut args=Vec::new();args.try_reserve_exact(LIMIT).map_err(|_|Error::Allocation)?;
        Ok(Self{commands,args,summary:Summary::default()})
    }
    pub fn reserved_storage_bytes(&self)->usize {
        self.commands.capacity()*std::mem::size_of::<Command>()+self.args.capacity()*std::mem::size_of::<Args>()
    }
    fn clear(&mut self){self.commands.clear();self.args.clear();self.summary=Summary::default();}
    fn flush(&mut self,run:Option<Run>){if let Some(run)=run {
        if run.count==1 {self.summary.direct_calls+=1;self.commands.push(Command::Direct{row:run.row,range:run.range});}
        else {self.summary.multi_calls+=1;self.summary.multi_records+=run.count as usize;
            self.commands.push(Command::Multi{row:run.row,binding:run.binding,first_arg:run.first_arg,count:run.count});}
    }}
    pub fn build<'p,'r,'a,H,S:Source>(&'p mut self,mode:Mode,limits:DrawLimits,
        recording:&'r Recording<'a,H>,source:&'r S)->Result<Prepared<'p,'r,'a,H,S,LIMIT>,Error> {
        self.clear();
        if let Err(error)=self.populate(mode,limits,recording,source) {
            self.clear();self.summary.bounded_fallback=error==Error::TooManyRecords;return Err(error);
        }
        self.summary.argument_records=self.args.len();
        Ok(Prepared{plan:self,recording,source})
    }
    fn populate<H,S:Source>(&mut self,mode:Mode,limits:DrawLimits,recording:&Recording<'_,H>,source:&S)->Result<(),Error> {
        let mdi=mode==Mode::ArenaMdi&&limits.mdi;
        let mut run:Option<Run>=None;let mut examined=0usize;
        if source.len()>LIMIT{return Err(Error::TooManyRecords);}
        for row_index in 0..source.len() {
            let row=source.row(row_index);
            // Resolve once per visible column. A rejected token cannot become
            // an unsafe shared draw or silently disappear from the image.
            let binding=match row.backing {Backing::Dedicated=>None,Backing::Arena(token)=>
                Some(recording.binding(token).map_err(|_|Error::StaleArena{row:row_index})?)};
            if let Some(binding)=&binding {validate(binding,row.width).map_err(|_|Error::UnsafeArena{row:row_index})?;}
            for range in row.ranges {
                if examined==LIMIT{return Err(Error::TooManyRecords);}
                examined+=1;if range.is_empty(){continue;}
                self.summary.records+=1;
                let argument=if let Some(binding)=&binding {
                    let draw=binding.draw(range.clone()).map_err(|_|Error::InvalidRange{row:row_index})?;
                    if u64::from(draw.first_index)+u64::from(draw.index_count)>u64::from(u32::MAX) {
                        return Err(Error::InvalidRange{row:row_index});
                    }
                    Some((PageKey{arena_id:binding.arena_id,vertex_page:binding.vertex_page,
                        index_page:binding.index_page,width:binding.request.width},Args{index_count:draw.index_count,
                        instance_count:1,first_index:draw.first_index,base_vertex:draw.base_vertex,first_instance:0}))
                }else{None};
                if let Some((key,arg))=argument.filter(|_|mdi) {
                    if run.as_ref().is_some_and(|run|run.binding!=key||run.count==limits.max_draws) {
                        self.flush(run.take());
                    }
                    let first_arg=self.args.len() as u32;self.args.push(arg);
                    if let Some(run)=&mut run {run.count+=1;}
                    else{run=Some(Run{row:row_index,range:range.clone(),binding:key,first_arg,count:1});}
                }else{
                    self.flush(run.take());self.summary.direct_calls+=1;
                    self.commands.push(Command::Direct{row:row_index,range:range.clone()});
                }
            }
        }
        self.flush(run);Ok(())
    }
}
fn validate<H>(b:&Binding<'_,H>,width:IndexWidth)->Result<(),()> {
    let r=b.request;
    let vertex_len=b.vertex_range.end.checked_sub(b.vertex_range.start).ok_or(())?;
    let index_len=b.index_range.end.checked_sub(b.index_range.start).ok_or(())?;
    if r.width!=width||b.vertex_range.start%VERTEX_STRIDE!=0||b.index_range.start%width.bytes()!=0
        ||b.index_range.start%4!=0||b.vertex_range.end>MAX_VERTEX_PAGE||b.index_range.end>MAX_INDEX_PAGE
        ||r.vertex_bytes!=u64::from(r.vertices)*VERTEX_STRIDE||r.vertex_bytes>vertex_len
        ||r.index_bytes!=((u64::from(r.indices)*width.bytes()+3)/4)*4||r.index_bytes>index_len
        ||r.vertex_bytes>r.vertex_reserved||r.index_bytes>r.index_reserved
        ||r.vertex_reserved>vertex_len||r.index_reserved>index_len
        ||match (r.indices,r.max_local_index){(0,None)=>false,(0,Some(_))|(_,None)=>true,
            (_,Some(max))=>max>=r.vertices||(width==IndexWidth::U16&&max>u32::from(u16::MAX))}
    {return Err(());}
    let base=b.vertex_range.start/VERTEX_STRIDE;
    if i32::try_from(base).is_err()||r.max_local_index.is_some_and(|max|
        base+u64::from(max)>=b.vertex_range.end/VERTEX_STRIDE){return Err(());}
    Ok(())
}

/// This same-frame borrow prevents source mutation or releasing/reusing an
/// allocation before all plan draws finish. It retains no GPU handle clones.
/// Recording itself must live until the encoder is submitted or discarded.
pub struct Prepared<'p,'r,'a,H,S:Source,const LIMIT:usize=MAX_RECORDS> {plan:&'p BoundedPlan<LIMIT>,recording:&'r Recording<'a,H>,source:&'r S}
impl<'p,'r,'a,H,S:Source,const LIMIT:usize> Prepared<'p,'r,'a,H,S,LIMIT> {
    /// Both opaque and alpha plans borrow this SAME submit-owned recording.
    pub fn recording(&self)->&'r Recording<'a,H>{self.recording}
    pub fn commands(&self)->&[Command]{&self.plan.commands}
    pub fn args(&self)->&[Args]{&self.plan.args}
    pub fn summary(&self)->Summary{self.plan.summary}
    /// Upload this slice ONCE before either eye, only when multi_calls>0.
    /// Singleton slots can remain unused in the bounded argument buffer.
    pub fn argument_bytes(&self)->&[u8]{bytemuck::cast_slice(&self.plan.args)}
    pub fn binding(&self,row:usize)->Result<Option<Binding<'_,H>>,Error> {
        match self.source.row(row).backing {Backing::Dedicated=>Ok(None),Backing::Arena(token)=>
            self.recording.binding(token).map(Some).map_err(|_|Error::StaleArena{row})}
    }
}
#[cfg(test)]
#[path="near_arena_plan_tests.rs"]
mod tests;
