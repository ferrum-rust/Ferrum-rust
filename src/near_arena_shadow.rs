//! Diagnostic metadata only: immediate-free virtual placement, never GPU work.
use super::{ChunkPos,Source,MAX_VISIBLE,RANGE_LIMIT};
use std::ops::Range;
const MIB:u64=1024*1024;
const VERTEX_BUDGET:u64=512*MIB;
const INDEX_PAGE:u64=64*MIB;
const INDEX_BUDGET:u64=128*MIB;
const VERTEX_STRIDE:u64=std::mem::size_of::<crate::mesh::Vertex>() as u64;

#[derive(Clone,Copy,Default,Debug)]
pub(in crate::renderer) struct Upload {
    pub vertex_used:u64,pub index_used:u64,pub vertex_reserved:u64,pub index_reserved:u64,
    pub vertex_count:u64,pub index_count:u64,pub width:u8,
}
impl Upload {
    fn valid(self)->bool {
        matches!(self.width,2|4) && self.vertex_count.checked_mul(VERTEX_STRIDE)==Some(self.vertex_used)
            && self.index_count.checked_mul(u64::from(self.width)).is_some_and(|n|n<=self.index_used)
            && self.vertex_used<=self.vertex_reserved && self.index_used<=self.index_reserved
            && self.index_used%4==0 && self.vertex_reserved%VERTEX_STRIDE==0 && self.index_reserved%4==0
    }
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct Allocation {page:u16,start:u64,len:u64}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct Hole {page:u16,start:u64,len:u64}
struct Pool {page_size:u64,max_pages:u16,pages:u16,holes:Vec<Hole>,hole_limit:usize}
impl Pool {
    fn new(page_size:u64,max_pages:u16,columns:usize)->Self {
        let hole_limit=columns+usize::from(max_pages);
        Self{page_size,max_pages,pages:0,holes:Vec::with_capacity(hole_limit),hole_limit}
    }
    fn clear(&mut self){self.pages=0;self.holes.clear();}
    fn allocate(&mut self,len:u64,allow_new:bool)->Option<Allocation> {
        if len==0||len>self.page_size{return None;}
        if let Some(i)=self.holes.iter().position(|h|h.len>=len) {
            let h=self.holes[i];let allocation=Allocation{page:h.page,start:h.start,len};
            if h.len==len {self.holes.remove(i);}else{self.holes[i].start+=len;self.holes[i].len-=len;}
            return Some(allocation);
        }
        if !allow_new||self.pages==self.max_pages{return None;}
        if len!=self.page_size&&self.holes.len()==self.hole_limit{return None;}
        let page=self.pages;self.pages+=1;
        if len!=self.page_size{self.holes.push(Hole{page,start:len,len:self.page_size-len});}
        Some(Allocation{page,start:0,len})
    }
    // Immediate reuse is optimistic. This deliberately models no GPU retirement.
    fn release(&mut self,a:Allocation)->bool {
        let Some(end)=a.start.checked_add(a.len)else{return false;};
        if a.page>=self.pages||a.len==0||end>self.page_size{return false;}
        let mut i=self.holes.partition_point(|h|(h.page,h.start)<(a.page,a.start));
        let mut h=Hole{page:a.page,start:a.start,len:a.len};
        if i>0&&self.holes[i-1].page==a.page {
            let left=self.holes[i-1];let left_end=left.start+left.len;
            if left_end>a.start{return false;}
            if left_end==a.start {h.start=left.start;h.len+=left.len;self.holes.remove(i-1);i-=1;}
        }
        if i<self.holes.len()&&self.holes[i].page==a.page {
            let right=self.holes[i];let end=h.start+h.len;
            if end>right.start{return false;}
            if end==right.start {h.len+=right.len;self.holes.remove(i);}
        }
        if self.holes.len()==self.hole_limit{return false;}
        self.holes.insert(i,h);true
    }
    fn report(&self)->serde_json::Value {
        let empty=self.holes.iter().filter(|h|h.start==0&&h.len==self.page_size).count();
        serde_json::json!({"page_bytes":self.page_size,"committed_pages":self.pages,
            "active_pages":usize::from(self.pages)-empty,"empty_committed_pages":empty,
            "committed_bytes":u64::from(self.pages)*self.page_size,
            "free_bytes":self.holes.iter().map(|h|h.len).sum::<u64>(),
            "largest_free_interval":self.holes.iter().map(|h|h.len).max().unwrap_or(0),
            "free_intervals":self.holes.len(),"free_interval_capacity":self.holes.capacity(),
            "free_interval_limit":self.hole_limit})
    }
}
#[derive(Clone,Copy)]
struct Entry {revision:u64,upload:Upload,vertex:Option<Allocation>,index:Option<Allocation>}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct Binding {vertex_page:u16,index_page:u16,width:u8}
#[derive(Clone,Copy,Debug)]
enum Failure {Unknown,Metadata,Placement,Width,Range}
#[derive(Clone,Copy,Default,Debug,serde::Serialize)]
struct Counts {
    records:u64,calls:u64,mdi_calls:u64,singleton_calls:u64,fallback_records:u64,
    records_in_mdi:u64,max_run:u64,run_histogram:[u64;17],
    unknown_records:u64,invalid_metadata_records:u64,unplaced_records:u64,
    width_mismatch_records:u64,invalid_range_records:u64,
    vertex_page_transitions:u64,index_page_transitions:u64,width_transitions:u64,
}
impl Counts {
    fn run(&mut self,start:usize,end:usize,binding:Option<Binding>,emit:&mut impl FnMut(Range<usize>,Option<Binding>)) {
        let n=(end-start)as u64;if n==0{return;}
        self.calls+=1;self.max_run=self.max_run.max(n);
        self.run_histogram[(63-n.leading_zeros())as usize]+=1;
        if n>1 {self.mdi_calls+=1;self.records_in_mdi+=n;}else{self.singleton_calls+=1;}
        emit(start..end,binding);
    }
    fn failure(&mut self,reason:Failure) {
        self.fallback_records+=1;match reason {
            Failure::Unknown=>self.unknown_records+=1,Failure::Metadata=>self.invalid_metadata_records+=1,
            Failure::Placement=>self.unplaced_records+=1,Failure::Width=>self.width_mismatch_records+=1,
            Failure::Range=>self.invalid_range_records+=1,
        }
    }
}
#[derive(Default,serde::Serialize)]
struct Totals {
    frames:u64,eyes:u64,records:u64,calls:u64,mdi_calls:u64,singleton_calls:u64,
    fallback_records:u64,records_in_mdi:u64,max_run:u64,max_calls_per_eye:u64,
    max_records_per_eye:u64,call_reduction:u64,max_call_reduction_per_eye:u64,
    run_histogram:[u64;17],all_record_argument_bytes:u64,mdi_record_argument_bytes:u64,
    unknown_records:u64,invalid_metadata_records:u64,unplaced_records:u64,
    width_mismatch_records:u64,invalid_range_records:u64,
    vertex_page_transitions:u64,index_page_transitions:u64,width_transitions:u64,
}
impl Totals {
    fn add(&mut self,c:Counts,eyes:usize) {
        let eyes=eyes as u64;self.frames+=1;self.eyes+=eyes;
        self.records+=c.records*eyes;self.calls+=c.calls*eyes;self.mdi_calls+=c.mdi_calls*eyes;
        self.singleton_calls+=c.singleton_calls*eyes;self.fallback_records+=c.fallback_records*eyes;
        self.records_in_mdi+=c.records_in_mdi*eyes;self.max_run=self.max_run.max(c.max_run);
        self.max_calls_per_eye=self.max_calls_per_eye.max(c.calls);
        self.max_records_per_eye=self.max_records_per_eye.max(c.records);
        self.call_reduction+=(c.records-c.calls)*eyes;
        self.max_call_reduction_per_eye=self.max_call_reduction_per_eye.max(c.records-c.calls);
        for (total,at) in self.run_histogram.iter_mut().zip(c.run_histogram){*total+=at*eyes;}
        // Geometry arguments may be reused between eyes; these are not eye-scaled.
        self.all_record_argument_bytes+=20*c.records;self.mdi_record_argument_bytes+=20*c.records_in_mdi;
        self.unknown_records+=c.unknown_records*eyes;self.invalid_metadata_records+=c.invalid_metadata_records*eyes;
        self.unplaced_records+=c.unplaced_records*eyes;self.width_mismatch_records+=c.width_mismatch_records*eyes;
        self.invalid_range_records+=c.invalid_range_records*eyes;
        self.vertex_page_transitions+=c.vertex_page_transitions*eyes;
        self.index_page_transitions+=c.index_page_transitions*eyes;self.width_transitions+=c.width_transitions*eyes;
    }
}
#[derive(Default,serde::Serialize)]
struct Events {
    uploads:u64,removes:u64,clears:u64,resident_limit_rejections:u64,invalid_uploads:u64,
    placement_failures:u64,allocator_faults:u64,vertex_reuses:u64,index_reuses:u64,
    vertex_allocations:u64,index_allocations:u64,width_changes:u64,
    replaced_vertex_payload_bytes:u64,replaced_index_payload_bytes:u64,
    peak_resident:usize,peak_vertex_committed_bytes:u64,peak_index_committed_bytes:u64,
}
struct Model {
    vertices:Pool,narrow:Pool,wide:Pool,index_max_pages:u16,limit:usize,
    entries:ahash::AHashMap<ChunkPos,Entry>,events:Events,all:Totals,direct:Totals,faulted:bool,
}
impl Model {
    fn new(page:u64)->Self {Self::with_limits(page,VERTEX_BUDGET,INDEX_PAGE,INDEX_BUDGET,MAX_VISIBLE)}
    fn with_limits(page:u64,vertex_budget:u64,index_page:u64,index_budget:u64,limit:usize)->Self {
        let v=(vertex_budget/page)as u16;let i=(index_budget/index_page)as u16;
        Self{vertices:Pool::new(page,v,limit),narrow:Pool::new(index_page,i,limit),
            wide:Pool::new(index_page,i,limit),index_max_pages:i,limit,
            entries:ahash::AHashMap::with_capacity(limit),events:Events::default(),
            all:Totals::default(),direct:Totals::default(),faulted:false}
    }
    fn index_pool(&mut self,width:u8)->&mut Pool {if width==2{&mut self.narrow}else{&mut self.wide}}
    fn release(&mut self,entry:Entry) {
        let mut valid=true;
        if let Some(a)=entry.vertex{valid&=self.vertices.release(a);}
        if let Some(a)=entry.index{valid&=self.index_pool(entry.upload.width).release(a);}
        if !valid {self.events.allocator_faults+=1;self.faulted=true;}
    }
    fn clear(&mut self) {
        self.events.clears+=1;self.entries.clear();self.vertices.clear();self.narrow.clear();self.wide.clear();self.faulted=false;
    }
    fn removed(&mut self,p:ChunkPos) {
        self.events.removes+=1;if let Some(old)=self.entries.remove(&p){self.release(old);}
    }
    fn upload(&mut self,p:ChunkPos,revision:u64,upload:Upload) {
        self.events.uploads+=1;
        if !self.entries.contains_key(&p)&&self.entries.len()==self.limit {
            self.events.resident_limit_rejections+=1;return;
        }
        // Replacing an existing payload need not create a map tombstone.
        let old=self.entries.get(&p).copied();
        let mut entry=Entry{revision,upload,vertex:None,index:None};
        if !upload.valid()||self.faulted {
            if !upload.valid(){self.events.invalid_uploads+=1;}
            if let Some(old)=old {self.release(old);}
        }else{
            if let Some(mut old)=old {
                if old.upload.width!=upload.width{self.events.width_changes+=1;}
                if old.vertex.is_some_and(|a|a.len>=upload.vertex_reserved) {
                    entry.vertex=old.vertex.take();self.events.vertex_reuses+=1;
                }else{self.events.replaced_vertex_payload_bytes=self.events.replaced_vertex_payload_bytes.saturating_add(old.upload.vertex_used);}
                if old.upload.width==upload.width&&old.index.is_some_and(|a|a.len>=upload.index_reserved) {
                    entry.index=old.index.take();self.events.index_reuses+=1;
                }else{self.events.replaced_index_payload_bytes=self.events.replaced_index_payload_bytes.saturating_add(old.upload.index_used);}
                self.release(old);
            }
            if !self.faulted {
                if entry.vertex.is_none(){entry.vertex=self.vertices.allocate(upload.vertex_reserved,true);
                    self.events.vertex_allocations+=u64::from(entry.vertex.is_some());}
                if entry.index.is_none(){let allow=self.narrow.pages+self.wide.pages<self.index_max_pages;
                    entry.index=self.index_pool(upload.width).allocate(upload.index_reserved,allow);
                    self.events.index_allocations+=u64::from(entry.index.is_some());}
            }
            if entry.vertex.is_none()||entry.index.is_none()||self.faulted {
                self.events.placement_failures+=1;self.release(entry);entry.vertex=None;entry.index=None;
            }
        }
        self.entries.insert(p,entry);self.events.peak_resident=self.events.peak_resident.max(self.entries.len());
        // No per-event table census: current fallback and payload sums are computed
        // once at report time; committed page peaks are constant-time counters.
        self.events.peak_vertex_committed_bytes=self.events.peak_vertex_committed_bytes.max(
            u64::from(self.vertices.pages)*self.vertices.page_size);
        self.events.peak_index_committed_bytes=self.events.peak_index_committed_bytes.max(
            u64::from(self.narrow.pages+self.wide.pages)*self.narrow.page_size);
    }
    fn binding(&self,p:ChunkPos,revision:Option<u64>,width:u8,r:&Range<u32>)->Result<Binding,Failure> {
        let e=self.entries.get(&p).filter(|e|Some(e.revision)==revision).ok_or(Failure::Unknown)?;
        if !e.upload.valid(){return Err(Failure::Metadata);}
        if e.upload.width!=width{return Err(Failure::Width);}
        let(v,i)=match(e.vertex,e.index){(Some(v),Some(i))if !self.faulted=>(v,i),_=>return Err(Failure::Placement)};
        let stride=u64::from(width);
        if u64::from(r.end)>e.upload.index_count || i.start%stride!=0 || v.start%VERTEX_STRIDE!=0
            || i32::try_from(v.start/VERTEX_STRIDE).is_err()
            || (i.start/stride).checked_add(u64::from(r.end)).is_none_or(|n|n>u64::from(u32::MAX))
            || u64::from(r.end).checked_mul(stride).is_none_or(|n|n>i.len) {return Err(Failure::Range);}
        Ok(Binding{vertex_page:v.page,index_page:i.page,width})
    }
    fn walk(&self,source:&impl Source,identities:&ahash::AHashMap<ChunkPos,u64>,
        mut emit:impl FnMut(Range<usize>,Option<Binding>))->Counts {
        let mut out=Counts::default();let mut run:Option<(usize,Binding)>=None;let mut at=0usize;
        for row in 0..source.len() {
            let(p,_,width)=source.key(row);let revision=identities.get(&p).copied();
            for r in source.ranges(row).iter().filter(|r|!r.is_empty()) {
                out.records+=1;
                match self.binding(p,revision,width,r) {
                    Ok(binding)=>{
                        if let Some((start,previous))=run {
                            if previous!=binding {
                                out.vertex_page_transitions+=u64::from(previous.vertex_page!=binding.vertex_page);
                                out.index_page_transitions+=u64::from(previous.index_page!=binding.index_page);
                                out.width_transitions+=u64::from(previous.width!=binding.width);
                                out.run(start,at,Some(previous),&mut emit);run=Some((at,binding));
                            }
                        }else{run=Some((at,binding));}
                    }
                    Err(reason)=>{
                        if let Some((start,binding))=run.take(){out.run(start,at,Some(binding),&mut emit);}
                        out.failure(reason);out.run(at,at+1,None,&mut emit);
                    }
                }
                at+=1;
            }
        }
        if let Some((start,binding))=run{out.run(start,at,Some(binding),&mut emit);}
        out
    }
    fn report(&self)->serde_json::Value {
        let mut used=[0u64;2];let mut reserved=[0u64;2];let mut placed=[0u64;2];let mut fallback=0;
        let mut saturated=false;
        let mut add=|to:&mut u64,n:u64|{if let Some(v)=to.checked_add(n){*to=v;}else{*to=u64::MAX;saturated=true;}};
        for e in self.entries.values(){add(&mut used[0],e.upload.vertex_used);add(&mut used[1],e.upload.index_used);
            add(&mut reserved[0],e.upload.vertex_reserved);add(&mut reserved[1],e.upload.index_reserved);
            if let(Some(v),Some(i))=(e.vertex,e.index){add(&mut placed[0],v.len);add(&mut placed[1],i.len);}else{fallback+=1;}}
        serde_json::json!({"vertex_page_bytes":self.vertices.page_size,"vertex_budget_bytes":VERTEX_BUDGET,
            "exceeds_current_renderer_default_max_buffer_size":self.vertices.page_size>wgpu::Limits::default().max_buffer_size,
            "index_page_bytes":INDEX_PAGE,"combined_index_budget_bytes":INDEX_BUDGET,
            "vertex_pool":self.vertices.report(),"u16_index_pool":self.narrow.report(),"u32_index_pool":self.wide.report(),
            "resident_columns":self.entries.len(),"resident_capacity":self.entries.capacity(),
            "fallback_columns":fallback,"faulted":self.faulted,"aggregate_byte_overflow":saturated,"actual_used_bytes":used,
            "actual_buffer_reserved_bytes":reserved,"virtual_reserved_bytes":placed,
            "events":self.events,"all_opaque_frames":self.all,"whole_list_bundle_miss_frames":self.direct})
    }
}
pub(super) struct Census {models:[Model;4],pending:Option<([Counts;4],usize)>,
    frames:u64,incomplete_frames:u64,unknown_uploads:u64}
impl Census {
    pub(super) fn new()->Self {Self{models:[64,128,256,512].map(|m|Model::new(m*MIB)),
        pending:None,frames:0,incomplete_frames:0,unknown_uploads:0}}
    pub(super) fn cancel_pending(&mut self){self.pending=None;}
    pub(super) fn upload(&mut self,p:ChunkPos,revision:Option<u64>,upload:Upload) {
        if let Some(revision)=revision{for model in &mut self.models{model.upload(p,revision,upload);}}
        else{self.unknown_uploads+=1;for model in &mut self.models{model.removed(p);}}
    }
    pub(super) fn removed(&mut self,p:ChunkPos){for model in &mut self.models{model.removed(p);}}
    pub(super) fn clear(&mut self){for model in &mut self.models{model.clear();}self.pending=None;}
    pub(super) fn sample(&mut self,source:&impl Source,identities:&ahash::AHashMap<ChunkPos,u64>,eyes:usize) {
        self.pending=None;self.frames+=1;
        if source.len()>MAX_VISIBLE||!(1..=2).contains(&eyes)
            ||(0..source.len()).any(|i|source.ranges(i).len()>RANGE_LIMIT) {
            self.incomplete_frames+=1;return;
        }
        let mut rows=[Counts::default();4];
        for(model,row)in self.models.iter_mut().zip(&mut rows){*row=model.walk(source,identities,|_,_|{});model.all.add(*row,eyes);}
        self.pending=Some((rows,eyes));
    }
    pub(super) fn completed(&mut self,cached:bool) {
        if let Some((rows,eyes))=self.pending.take(){if !cached{for(model,row)in self.models.iter_mut().zip(rows){model.direct.add(row,eyes);}}}
    }
    pub(super) fn report(&self)->serde_json::Value {serde_json::json!({
        "diagnostic_not_baseline":true,"placement":"optimistic_immediate_free_no_retirement_no_compaction",
        "frames":self.frames,"incomplete_frames":self.incomplete_frames,"unknown_uploads":self.unknown_uploads,
        "resident_limit":MAX_VISIBLE,"record_limit_per_eye":MAX_VISIBLE*RANGE_LIMIT,
        "hypothetical_page_note":"The512MiB vertex-page scenario exceeds the current renderer's requested256MiB default max_buffer_size. It is a CPU layout case only; no larger device limit or GPU allocation is requested or validated.",
        "scenarios":self.models.iter().map(Model::report).collect::<Vec<_>>(),
        "note":"CPU metadata only; zero GPU allocations/draws/migrations. Actual retained buffer capacities determine reservations; actual used bytes are separately reported. Vertex pages64/128/256/512MiB share512MiB per scenario; u16/u32 index pools use64MiB pages with128MiB combined. First-fit stable pages, no regrouping or unrelated relocation. Freed ranges are immediately reusable: this is optimistic placement, not safe GPU retirement or real VRAM peak. Empty pages stay committed until clear. Every fallback range splits runs and counts as a direct draw. Incomplete oversize inputs are excluded, not claimed as savings. Counts/calls/transitions are eye-scaled; argument bytes describe one reusable geometry list. Histogram bucket i is floor(log2(run records)); transitions may overlap. whole_list_bundle_miss_frames excludes whole-list terrain bundles only; an independently enabled near_opaque_runs experiment may still bundle that subset, so keep it off for a direct-baseline interpretation. Captured uploads include startup/warmup; frame samples follow the existing render-profile gate. No FPS prediction."})}
}
#[cfg(test)]
#[path="near_arena_shadow_tests.rs"]
mod tests;
