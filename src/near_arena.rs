//! Bounded persistent near-terrain page allocator for the optional experiment.
//! Default mode remains Dedicated. PageFactory must return only a completely
//! validated GPU page (validation/OOM errors captured before returning Ok).
//! No compaction, migration, mapped writes, eviction or automatic fallback draw.
//!
//! Single-owner/single-Queue contract: before begin_update/release, every old
//! encoder must have been submitted or discarded and all bundles/plans naming
//! the affected old allocation must be invalidated before new writes. Queue
//! staging writes must finish in new-owner order. No delayed writer may retain
//! an obsolete destination. Submitted work need not be host-waited: Queue order
//! protects it; this does NOT authorize reusing storage referenced by a future
//! old encoder. Recording enforces lexical exclusion and fails closed on unwind.
//! A Pending may be abandoned only BEFORE writes; allocation failure and Drop
//! then preserve the old owner exactly. target() marks write-start BEFORE it
//! returns GPU handles. Dropping that Pending poisons the Arena; old tokens are
//! no longer readable/admissible. No GPU bytes are rolled back by this core.
use std::{ops::Range, sync::atomic::{AtomicU64, Ordering}};
const MIB:u64=1024*1024;
const VERTEX_STRIDE:u64=32; // Integration must assert size_of::<Vertex>() == 32.
static NEXT_ARENA:AtomicU64=AtomicU64::new(1);

#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub enum Mode { #[default] Dedicated, ArenaDirect, ArenaMdi }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum IndexWidth { U16, U32 }
impl IndexWidth { pub fn bytes(self)->u64 {match self {Self::U16=>2,Self::U32=>4}} }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum PageKind { Vertex, Index(IndexWidth) }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Limits { pub vertex_page:u64,pub index_page:u64,pub owners:usize }
impl Default for Limits {fn default()->Self{Self{vertex_page:256*MIB,index_page:64*MIB,owners:4096}}}
impl Limits {
    fn valid(self)->bool {self.owners>0&&self.owners<=4096&&self.vertex_page>0&&self.vertex_page<=256*MIB
        &&self.index_page>0&&self.index_page<=64*MIB&&self.vertex_page%VERTEX_STRIDE==0&&self.index_page%4==0}
    fn bytes(self,kind:PageKind)->u64 {match kind {PageKind::Vertex=>self.vertex_page,PageKind::Index(_)=>self.index_page}}
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Request {
    pub vertex_bytes:u64,pub index_bytes:u64,pub vertex_reserved:u64,pub index_reserved:u64,
    pub vertices:u32,pub indices:u32,pub width:IndexWidth,
    /// Maximum of ALL logical index elements, computed once from exact prepared
    /// index bytes, excluding alignment padding. None iff indices == 0. Caller
    /// must not guess from capacity or an opaque subset. Invalid geometry keeps
    /// the original dedicated robustness path instead of reading adjacent owners.
    pub max_local_index:Option<u32>,
}
impl Request {
    fn valid(self,limits:Limits)->bool {
        let logical_indices=u64::from(self.indices)*self.width.bytes();
        (match (self.indices,self.max_local_index) {
            (0,None)=>true,
            (0,Some(_))|(_,None)=>false,
            (_,Some(max))=>max<self.vertices&&(self.width!=IndexWidth::U16||max<=u16::MAX as u32),
        }) && self.vertex_bytes==u64::from(self.vertices)*VERTEX_STRIDE
            &&self.index_bytes==((logical_indices+3)/4)*4
            &&self.vertex_reserved>0&&self.index_reserved>0
            &&self.vertex_bytes<=self.vertex_reserved&&self.index_bytes<=self.index_reserved
            &&self.vertex_reserved<=limits.vertex_page&&self.index_reserved<=limits.index_page
            &&self.vertex_reserved%VERTEX_STRIDE==0&&self.index_reserved%4==0
    }
}
/// Tokens are copyable references, never implicit owning/dropping handles.
/// Arena identity plus a nonwrapping generation rejects stale/foreign tokens.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Token { arena:u64,slot:u16,generation:u64 }
impl Token {pub fn identity(self)->[u64;3]{[self.arena,u64::from(self.slot),self.generation]}}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Reject { Dedicated, InvalidLimits, InvalidRequest, Metadata, OwnerLimit, PageLimit,
    Fragmented, StaleToken, GenerationExhausted, RecordingUnfinished, Poisoned, CorruptState }
#[derive(Debug,PartialEq,Eq)]
pub enum Failure<E> { Admission(Reject), Page(E) }
impl<E> From<Reject> for Failure<E> {fn from(value:Reject)->Self{Self::Admission(value)}}
/// GPU-specific creation/error scopes belong in a future adapter implementation.
/// A failed create may drop its temporary resource, but must not mutate old pages.
pub trait PageFactory {type Handle;type Error;fn create(&mut self,kind:PageKind,bytes:u64)->Result<Self::Handle,Self::Error>;}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct Span {page:u8,offset:u64,len:u64}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct Hole {offset:u64,len:u64}
struct Page<H> {kind:PageKind,bytes:u64,handle:H,holes:Vec<Hole>,hole_limit:usize}
impl<H> Page<H> {
    fn take(&mut self,len:u64)->Option<Span> {
        let i=self.holes.iter().position(|h|h.len>=len)?;let h=self.holes[i];
        if h.len==len {self.holes.remove(i);}else{self.holes[i].offset+=len;self.holes[i].len-=len;}
        Some(Span{page:0,offset:h.offset,len})
    }
    fn can_release(&self,a:Span)->bool {
        let Some(end)=a.offset.checked_add(a.len)else{return false;};
        if a.len==0||end>self.bytes{return false;}
        let i=self.holes.partition_point(|h|h.offset<a.offset);
        let left=if i>0 {Some(self.holes[i-1])}else{None};let right=self.holes.get(i).copied();
        if left.is_some_and(|h|h.offset+h.len>a.offset)||right.is_some_and(|h|end>h.offset){return false;}
        self.holes.len()<self.hole_limit||left.is_some_and(|h|h.offset+h.len==a.offset)||right.is_some_and(|h|end==h.offset)
    }
    fn release(&mut self,a:Span)->bool {
        // Validate both neighboring overlaps BEFORE changing either interval.
        if !self.can_release(a){return false;}
        let mut i=self.holes.partition_point(|h|h.offset<a.offset);let mut h=Hole{offset:a.offset,len:a.len};
        if i>0&&self.holes[i-1].offset+self.holes[i-1].len==h.offset {
            i-=1;h.offset=self.holes[i].offset;h.len+=self.holes[i].len;self.holes.remove(i);
        }
        if i<self.holes.len()&&h.offset+h.len==self.holes[i].offset {h.len+=self.holes[i].len;self.holes.remove(i);}
        self.holes.insert(i,h);true
    }
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct Record {generation:u64,request:Request,vertex:Span,index:Span}
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub struct Stats {pub owners:usize,pub vertex_pages:usize,pub index_pages:usize,pub committed_bytes:u64,
    pub reserved_bytes:u64,pub used_bytes:u64,pub free_bytes:u64,pub free_intervals:usize,pub metadata_limit:usize}
/// A direct slice uses local indices/base_vertex=0. An MDI binding uses complete
/// pages and Draw's checked absolute first_index/base_vertex. Never mix them.
pub struct Binding<'a,H> {pub arena_id:u64,pub allocation_generation:u64,pub vertex:&'a H,pub index:&'a H,pub vertex_page:u8,pub index_page:u8,
    pub vertex_range:Range<u64>,pub index_range:Range<u64>,pub request:Request}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Draw {pub index_count:u32,pub first_index:u32,pub base_vertex:i32}
impl<H> Binding<'_,H> {
    pub fn draw(&self,range:Range<u32>)->Result<Draw,Reject> {
        if range.start>range.end||range.end>self.request.indices{return Err(Reject::InvalidRequest);}
        let first=self.index_range.start/self.request.width.bytes()+u64::from(range.start);
        Ok(Draw{index_count:range.end-range.start,first_index:u32::try_from(first).map_err(|_|Reject::InvalidRequest)?,
            base_vertex:i32::try_from(self.vertex_range.start/VERTEX_STRIDE).map_err(|_|Reject::InvalidRequest)?})
    }
}
pub struct Arena<H> {id:u64,mode:Mode,limits:Limits,pages:Vec<Page<H>>,slots:Vec<Option<Record>>,
    next_generation:u64,recording_unfinished:bool,poisoned:bool}
impl<H> Arena<H> {
    pub fn new(mode:Mode,limits:Limits)->Result<Self,Reject> {
        if !limits.valid(){return Err(Reject::InvalidLimits);}
        let id=NEXT_ARENA.fetch_update(Ordering::Relaxed,Ordering::Relaxed,|n|n.checked_add(1))
            .map_err(|_|Reject::GenerationExhausted)?;
        let mut slots=Vec::new();slots.try_reserve_exact(limits.owners).map_err(|_|Reject::Metadata)?;
        slots.resize(limits.owners,None);let mut pages=Vec::new();pages.try_reserve_exact(4).map_err(|_|Reject::Metadata)?;
        Ok(Self{id,mode,limits,pages,slots,next_generation:1,recording_unfinished:false,poisoned:false})
    }
    pub fn mode(&self)->Mode {self.mode}
    fn record(&self,token:Token)->Result<Record,Reject> {
        if self.poisoned{return Err(Reject::Poisoned);}
        if token.arena!=self.id{return Err(Reject::StaleToken);}
        self.slots.get(usize::from(token.slot)).and_then(|r|*r)
            .filter(|r|r.generation==token.generation).ok_or(Reject::StaleToken)
    }
    fn open(&self)->Result<(),Reject> {if self.poisoned{return Err(Reject::Poisoned);}
        if self.recording_unfinished{Err(Reject::RecordingUnfinished)}else{Ok(())}}
    fn allocate<F:PageFactory<Handle=H>>(&mut self,kind:PageKind,len:u64,factory:&mut F)->Result<Span,Failure<F::Error>> {
        for (i,page) in self.pages.iter_mut().enumerate().filter(|(_,p)|p.kind==kind) {
            if let Some(mut span)=page.take(len){span.page=i as u8;return Ok(span);}
        }
        let count=self.pages.iter().filter(|p|match(kind,p.kind){
            (PageKind::Vertex,PageKind::Vertex)|(PageKind::Index(_),PageKind::Index(_))=>true,_=>false}).count();
        if count==2{return Err(Reject::PageLimit.into());}
        let hole_limit=self.limits.owners+3;
        let mut holes=Vec::new();holes.try_reserve_exact(hole_limit).map_err(|_|Failure::Admission(Reject::Metadata))?;
        let bytes=self.limits.bytes(kind);
        let handle=factory.create(kind,bytes).map_err(Failure::Page)?;
        let page=self.pages.len() as u8;
        if len<bytes{holes.push(Hole{offset:len,len:bytes-len});}
        self.pages.push(Page{kind,bytes,handle,holes,hole_limit});
        Ok(Span{page,offset:0,len})
    }
    fn free(&mut self,span:Span)->bool {self.pages.get_mut(usize::from(span.page)).is_some_and(|p|p.release(span))}
    fn reusable(&self,span:Span,kind:PageKind,len:u64)->bool {
        span.len>=len&&self.pages[usize::from(span.page)].kind==kind
    }
    fn view(&self,record:Record)->Binding<'_,H> {
        Binding{arena_id:self.id,allocation_generation:record.generation,vertex:&self.pages[usize::from(record.vertex.page)].handle,index:&self.pages[usize::from(record.index.page)].handle,
            vertex_page:record.vertex.page,index_page:record.index.page,
            vertex_range:record.vertex.offset..record.vertex.offset+record.vertex.len,
            index_range:record.index.offset..record.index.offset+record.index.len,request:record.request}
    }
    pub fn binding(&self,token:Token)->Result<Binding<'_,H>,Reject> {Ok(self.view(self.record(token)?))}
    /// Successful admission is the last fallible step before caller invalidates
    /// old caches and queues destination writes. Existing old owner is retained.
    pub fn begin_update<F:PageFactory<Handle=H>>(&mut self,old:Option<Token>,request:Request,factory:&mut F)
        ->Result<Pending<'_,H>,Failure<F::Error>> {
        self.open()?;if self.mode==Mode::Dedicated{return Err(Reject::Dedicated.into());}
        if !request.valid(self.limits){return Err(Reject::InvalidRequest.into());}
        let prior=old.map(|token|self.record(token)).transpose()?;
        let slot=if let Some(token)=old {usize::from(token.slot)}else{
            self.slots.iter().position(Option::is_none).ok_or(Reject::OwnerLimit)?};
        let generation=self.next_generation;
        self.next_generation=self.next_generation.checked_add(1).ok_or(Reject::GenerationExhausted)?;
        let reuse_v=prior.is_some_and(|r|self.reusable(r.vertex,PageKind::Vertex,request.vertex_reserved));
        let reuse_i=prior.is_some_and(|r|self.reusable(r.index,PageKind::Index(request.width),request.index_reserved));
        let vertex=if reuse_v {prior.unwrap().vertex}else{self.allocate(PageKind::Vertex,request.vertex_reserved,factory)?};
        // Arm rollback BEFORE the second factory call, including unwind. The
        // private index placeholder is never exposed: replace it before returning
        // Pending, and new_index stays false until a real index range exists.
        let mut pending=Pending{arena:self,slot,prior,record:Record{generation,request,vertex,index:vertex},
            new_vertex:!reuse_v,new_index:false,committed:false,writes_started:false};
        if reuse_i {pending.record.index=prior.unwrap().index;}else{
            pending.record.index=pending.arena.allocate(PageKind::Index(request.width),request.index_reserved,factory)?;
            pending.new_index=true;
        }
        Ok(pending)
    }
    pub fn release(&mut self,token:Token)->Result<(),Reject> {
        self.open()?;let record=self.record(token)?;
        // Both releases validated before mutation; valid owned spans can never overlap.
        if !self.pages[usize::from(record.vertex.page)].can_release(record.vertex)
            ||!self.pages[usize::from(record.index.page)].can_release(record.index){self.poisoned=true;return Err(Reject::CorruptState);}
        let a=self.free(record.vertex);let b=self.free(record.index);debug_assert!(a&&b);
        self.slots[usize::from(token.slot)]=None;Ok(())
    }
    pub fn begin_recording(&mut self)->Result<Recording<'_,H>,Reject> {
        self.open()?;self.recording_unfinished=true;Ok(Recording{arena:self,finished:false})
    }
    /// Diagnostic only; O(owner_limit + free intervals), not a per-frame API.
    pub fn stats(&self)->Stats {
        let mut s=Stats{metadata_limit:self.limits.owners,..Default::default()};
        for page in &self.pages {match page.kind {PageKind::Vertex=>s.vertex_pages+=1,PageKind::Index(_)=>s.index_pages+=1}
            s.committed_bytes+=page.bytes;s.free_intervals+=page.holes.len();s.free_bytes+=page.holes.iter().map(|h|h.len).sum::<u64>();}
        for record in self.slots.iter().flatten(){s.owners+=1;s.reserved_bytes+=record.vertex.len+record.index.len;
            s.used_bytes+=record.request.vertex_bytes+record.request.index_bytes;}
        s
    }
}
pub struct Pending<'a,H> {arena:&'a mut Arena<H>,slot:usize,prior:Option<Record>,record:Record,new_vertex:bool,new_index:bool,committed:bool,writes_started:bool}
impl<H> Pending<'_,H> {
    /// Marks the no-rollback boundary before exposing writable page handles.
    /// If anything panics/returns early after this call, Drop poisons all tokens
    /// rather than pretending overwritten GPU contents still belong to old.
    pub fn target(&mut self)->Binding<'_,H> {self.writes_started=true;self.arena.view(self.record)}
    pub fn previous(&self)->Option<Binding<'_,H>> {self.prior.map(|record|self.arena.view(record))}
    /// No fallible allocation or GPU call here. Caller already queued exact
    /// destination bytes and will immediately publish the new draw metadata.
    pub fn commit(mut self)->Token {
        if let Some(old)=self.prior {
            if self.new_vertex {assert!(self.arena.free(old.vertex),"owned vertex range invariant");}
            if self.new_index {assert!(self.arena.free(old.index),"owned index range invariant");}
        }
        self.arena.slots[self.slot]=Some(self.record);self.committed=true;
        Token{arena:self.arena.id,slot:self.slot as u16,generation:self.record.generation}
    }
}
impl<H> Drop for Pending<'_,H> {
    fn drop(&mut self) {if !self.committed {
        if self.writes_started{self.arena.poisoned=true;}
        if self.new_vertex&&!self.arena.free(self.record.vertex){self.arena.poisoned=true;}
        if self.new_index&&!self.arena.free(self.record.index){self.arena.poisoned=true;}
    }}
}
pub struct Recording<'a,H> {arena:&'a mut Arena<H>,finished:bool}
impl<H> Recording<'_,H> {
    pub fn binding(&self,token:Token)->Result<Binding<'_,H>,Reject> {self.arena.binding(token)}
    /// Must be called only after the actual encoder was submitted to the SAME
    /// Queue. Queue completion is intentionally not required by this contract.
    pub fn submitted(mut self){self.arena.recording_unfinished=false;self.finished=true;}
    /// Must be called only after dropping the entire unsubmitted encoder.
    pub fn discarded(mut self){self.arena.recording_unfinished=false;self.finished=true;}
}
impl<H> Drop for Recording<'_,H> {fn drop(&mut self){if !self.finished{self.arena.poisoned=true;}}}
#[cfg(test)]
#[path="near_arena_tests.rs"]
mod tests;

#[cfg(test)]
#[path="near_arena_gpu_tests.rs"]
mod gpu_tests;
