//! Explicit benchmark trial: one fixed mapped source slab per selected frame.
//! Queue remains the default. No new poll/wait, slab growth, geometry change,
//! destination ownership, or presentation setting. A selected frame is an
//! upload transaction: CPU Order changes cannot survive an abandoned encoder.
use std::sync::{Arc,Mutex,atomic::{AtomicU64,Ordering}};
use serde::Serialize;

pub(super) const SLOTS:usize=4;
pub(super) const SLAB_BYTES:u64=8*1024*1024;
const MAX_EPOCH:u64=u64::MAX/4;

#[derive(Clone,Copy,Debug,Default,PartialEq,Eq,clap::ValueEnum)]
pub(crate) enum Mode { #[default] Queue, Staged }

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(super) enum Error { Poisoned, ActiveFrame, Unplanned, AlreadyPlanned, WrongPhase,
    InvalidWrite, Budget, EpochExhausted }
impl std::fmt::Display for Error {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {write!(f,"transparent staging {self:?}")}
}
impl std::error::Error for Error {}
#[derive(Clone,Copy,Debug,PartialEq,Eq,Serialize)]
enum SlotState { Ready, Reserved, Pending, Failed }
#[derive(Clone,Copy,Debug,PartialEq,Eq,Serialize)]
struct Slot { epoch:u64,state:SlotState }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Fallback { Distance, Empty, Busy, Oversize, Overflow }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Selection { Queue(Fallback), Staged{slot:usize,epoch:u64} }

#[derive(Default,Serialize)]
struct Counts {
    begun_frames:u64,submitted_frames:u64,staged_frames:u64,queue_frames:u64,
    distance_frames:u64,empty_frames:u64,busy_frames:u64,oversize_frames:u64,
    overflow_frames:u64,staged_copies:u64,staged_bytes:u64,aligned_staged_bytes:u64,
    queue_writes:u64,queue_bytes:u64,remap_requests:u64,remap_successes:u64,
    remap_failures:u64,ignored_callbacks:u64,reused_frames:u64,abandoned_frames:u64,
    peak_pending_slots:usize,maximum_preflight_bytes:u64,maximum_frame_bytes:u64,
}

/// Adds one potential write. Full transparent membership is used by the caller,
/// never the previous mask's active index length. Zero contributes no padding.
pub(super) fn add_bound(total:Option<u64>,bytes:Option<u64>)->Option<u64> {
    let total=total?;let bytes=bytes?;
    if bytes==0 {Some(total)} else {total.checked_add(7).map(|n|n&!7)?.checked_add(bytes)}
}

struct Book {
    capacity:u64,slots:[Slot;SLOTS],counts:Counts,active:bool,poisoned:bool,
    #[cfg(test)] hold_recalled:bool,
}
impl Book {
    fn new(capacity:u64)->Self {
        assert!(capacity>0&&capacity<=SLAB_BYTES&&capacity%8==0);
        Self{capacity,slots:[Slot{epoch:0,state:SlotState::Ready};SLOTS],
            counts:Counts::default(),active:false,poisoned:false,
            #[cfg(test)] hold_recalled:false}
    }
    fn begin(&mut self,eligible:bool)->Result<FrameBook<'_>,Error> {
        if self.poisoned{return Err(Error::Poisoned)}
        if self.active{return Err(Error::ActiveFrame)}
        self.active=true;self.counts.begun_frames+=1;
        Ok(FrameBook{book:self,eligible,selection:None,bound:0,cursor:0,
            copies:0,bytes:0,queue_writes:0,queue_bytes:0,sealed:false,acknowledged:false})
    }
    fn complete(&mut self,slot:usize,epoch:u64,success:bool) {
        let Some(current)=self.slots.get_mut(slot) else {self.counts.ignored_callbacks+=1;return};
        if current.state!=SlotState::Pending||current.epoch!=epoch {
            self.counts.ignored_callbacks+=1;return
        }
        current.state=if success{SlotState::Ready}else{SlotState::Failed};
        if success{self.counts.remap_successes+=1}else{self.counts.remap_failures+=1}
    }
}
/// Pure ownership/admission state used by the real GPU Session and CPU oracle.
struct FrameBook<'a> {
    book:&'a mut Book,eligible:bool,selection:Option<Selection>,bound:u64,cursor:u64,
    copies:u64,bytes:u64,queue_writes:u64,queue_bytes:u64,sealed:bool,acknowledged:bool,
}
impl FrameBook<'_> {
    fn fail<T>(&mut self,error:Error)->Result<T,Error>{self.book.poisoned=true;Err(error)}
    fn plan(&mut self,bound:Option<u64>)->Result<Selection,Error> {
        if self.book.poisoned{return Err(Error::Poisoned)}
        if self.selection.is_some(){return self.fail(Error::AlreadyPlanned)}
        let selection=if !self.eligible {Selection::Queue(Fallback::Distance)}
        else if let Some(bound)=bound {
            self.bound=bound;self.book.counts.maximum_preflight_bytes=self.book.counts.maximum_preflight_bytes.max(bound);
            if bound==0 {Selection::Queue(Fallback::Empty)}
            else if bound>self.book.capacity {Selection::Queue(Fallback::Oversize)}
            else {
                let ready=self.book.slots.iter().position(|slot|slot.state==SlotState::Ready&&{
                    #[cfg(test)] {!(self.book.hold_recalled&&slot.epoch>0)}
                    #[cfg(not(test))] {true}
                });
                if let Some(slot)=ready {
                    let state=&mut self.book.slots[slot];
                    let Some(epoch)=state.epoch.checked_add(1).filter(|n|*n<=MAX_EPOCH)
                        else{return self.fail(Error::EpochExhausted)};
                    state.epoch=epoch;state.state=SlotState::Reserved;
                    Selection::Staged{slot,epoch}
                }else{Selection::Queue(Fallback::Busy)}
            }
        }else{Selection::Queue(Fallback::Overflow)};
        // Armed before caller enters any Order::sort, even if all later results
        // are Identical/Skipped. Dropping this selected frame poisons the owner.
        self.selection=Some(selection);Ok(selection)
    }
    fn write(&mut self,bytes:u64)->Result<Option<(usize,u64)>,Error> {
        if self.book.poisoned{return Err(Error::Poisoned)}
        if self.sealed||self.acknowledged{return self.fail(Error::WrongPhase)}
        if bytes==0||bytes%4!=0{return self.fail(Error::InvalidWrite)}
        match self.selection {
            Some(Selection::Queue(_))=>{self.queue_writes+=1;self.queue_bytes+=bytes;Ok(None)},
            Some(Selection::Staged{slot,..})=>{
                let Some(start)=self.cursor.checked_add(7).map(|n|n&!7) else{return self.fail(Error::Budget)};
                let Some(end)=start.checked_add(bytes).filter(|n|*n<=self.bound&&*n<=self.book.capacity)
                    else{return self.fail(Error::Budget)};
                self.cursor=end;self.copies+=1;self.bytes+=bytes;Ok(Some((slot,start)))
            },
            None=>self.fail(Error::Unplanned),
        }
    }
    fn seal(&mut self)->Result<Option<usize>,Error> {
        if self.book.poisoned{return Err(Error::Poisoned)}
        if self.selection.is_none(){return self.fail(Error::Unplanned)}
        if self.sealed||self.acknowledged{return self.fail(Error::WrongPhase)}
        self.sealed=true;
        Ok(match self.selection {Some(Selection::Staged{slot,..}) if self.copies>0=>Some(slot),_=>None})
    }
    fn submitted(&mut self)->Result<Option<(usize,u64)>,Error> {
        if self.book.poisoned{return Err(Error::Poisoned)}
        if !self.sealed||self.acknowledged{return self.fail(Error::WrongPhase)}
        let Some(selection)=self.selection else{return self.fail(Error::Unplanned)};
        self.acknowledged=true;
        let c=&mut self.book.counts;c.submitted_frames+=1;c.queue_writes+=self.queue_writes;c.queue_bytes+=self.queue_bytes;
        c.maximum_frame_bytes=c.maximum_frame_bytes.max(self.bytes+self.queue_bytes);
        let remap=match selection {
            Selection::Queue(reason)=>{c.queue_frames+=1;match reason {
                Fallback::Distance=>c.distance_frames+=1,Fallback::Empty=>c.empty_frames+=1,
                Fallback::Busy=>c.busy_frames+=1,Fallback::Oversize=>c.oversize_frames+=1,
                Fallback::Overflow=>c.overflow_frames+=1};None},
            Selection::Staged{slot,epoch}=>{
                c.staged_frames+=1;c.reused_frames+=u64::from(epoch>1);
                c.staged_copies+=self.copies;c.staged_bytes+=self.bytes;c.aligned_staged_bytes+=self.cursor;
                if self.copies==0 {self.book.slots[slot].state=SlotState::Ready;None}
                else {self.book.slots[slot].state=SlotState::Pending;c.remap_requests+=1;Some((slot,epoch))}
            },
        };
        c.peak_pending_slots=c.peak_pending_slots.max(self.book.slots.iter().filter(|s|s.state==SlotState::Pending).count());
        Ok(remap)
    }
}
impl Drop for FrameBook<'_> {
    fn drop(&mut self){
        if !self.acknowledged {
            self.book.counts.abandoned_frames+=1;
            if matches!(self.selection,Some(Selection::Staged{..})){self.book.poisoned=true;}
        }
        self.book.active=false;
    }
}

struct Mailbox {slots:[AtomicU64;SLOTS]}
impl Mailbox {fn new()->Self {Self{slots:std::array::from_fn(|_|AtomicU64::new(0))}}}
pub(super) type Shared=Arc<Mutex<State>>;
pub(super) struct State {book:Book,buffers:[wgpu::Buffer;SLOTS],mailbox:Arc<Mailbox>}
pub(super) struct Session<'a> {
    frame:FrameBook<'a>,buffers:&'a [wgpu::Buffer;SLOTS],mailbox:&'a Arc<Mailbox>,
}
impl State {
    pub(super) fn new(device:&wgpu::Device)->anyhow::Result<Shared>{Self::with_capacity(device,SLAB_BYTES)}
    fn with_capacity(device:&wgpu::Device,capacity:u64)->anyhow::Result<Shared>{
        anyhow::ensure!(capacity>0&&capacity<=SLAB_BYTES&&capacity%8==0,"invalid fixed staging capacity");
        let mut buffers=Vec::with_capacity(SLOTS);
        for _ in 0..SLOTS {
            device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
            device.push_error_scope(wgpu::ErrorFilter::Validation);
            let buffer=device.create_buffer(&wgpu::BufferDescriptor{label:Some("fixed transparent index staging"),
                size:capacity,usage:wgpu::BufferUsages::MAP_WRITE|wgpu::BufferUsages::COPY_SRC,mapped_at_creation:true});
            let validation=pollster::block_on(device.pop_error_scope());
            let oom=pollster::block_on(device.pop_error_scope());
            anyhow::ensure!(validation.is_none()&&oom.is_none(),"fixed staging initialization failed: validation={validation:?}, oom={oom:?}");
            buffers.push(buffer);
        }
        let buffers=buffers.try_into().unwrap_or_else(|_|unreachable!("fixed four buffers"));
        Ok(Arc::new(Mutex::new(Self{book:Book::new(capacity),buffers,mailbox:Arc::new(Mailbox::new())})))
    }
    fn drain(&mut self){for slot in 0..SLOTS {
        let value=self.mailbox.slots[slot].swap(0,Ordering::AcqRel);
        if value!=0 {self.book.complete(slot,value/4,value%4==1);}
    }}
    pub(super) fn frame(&mut self,eligible:bool)->Result<Session<'_>,Error>{
        self.drain();Ok(Session{frame:self.book.begin(eligible)?,buffers:&self.buffers,mailbox:&self.mailbox})
    }
    pub(super) fn report(&self)->serde_json::Value {
        serde_json::json!({"enabled":true,"mode":"staged","eligible_render_distance":128,
            "poisoned":self.book.poisoned,"frame_active":self.book.active,
            "slab_count":SLOTS,"slab_bytes":self.book.capacity,"fixed_source_payload_bytes":self.book.capacity*SLOTS as u64,
            "maximum_slabs_per_frame":1,"reuse_counter_scope":"reservation epoch reuse, including selected frames with no actual copies; completed copy reuse additionally requires remap success and staged copy counters","slots":self.book.slots,
            "mapped_ready_slots":self.book.slots.iter().filter(|s|s.state==SlotState::Ready).count(),
            "pending_slots":self.book.slots.iter().filter(|s|s.state==SlotState::Pending).count(),
            "failed_slots":self.book.slots.iter().filter(|s|s.state==SlotState::Failed).count(),
            "unconsumed_callback_slots":self.mailbox.slots.iter().filter(|m|m.load(Ordering::Acquire)!=0).count(),
            "counts":self.book.counts,
            "note":"Explicit benchmark trial, whole-frame Queue fallback before sorting. Counts cover submitted frames including warmup. Preflight/unmap/remap remain inside existing encode/submit wall scopes; no extra diagnostic clock reads. Four fixed source buffers bound added payload, not driver memory or existing Queue fallback staging. No new poll/wait or per-frame buffer creation. Map callbacks progress via existing submissions. Pending callbacks may already be complete but unconsumed in this final snapshot. Noncoherent unmap may flush the full mapped slab. Staged copies execute inside the frame encoder; original Queue transfers precede it. No GPU timestamp comparison is supported in this initial trial."})
    }
}
impl Session<'_> {
    pub(super) fn plan(&mut self,bound:Option<u64>)->Result<(),Error>{self.frame.plan(bound).map(|_|())}
    pub(super) fn write(&mut self,encoder:&mut wgpu::CommandEncoder,queue:&wgpu::Queue,
        target:&wgpu::Buffer,target_offset:u64,bytes:&[u8])->Result<(),Error>{
        if target_offset%4!=0||target_offset.checked_add(bytes.len() as u64).is_none_or(|end|end>target.size()) {
            return self.frame.fail(Error::InvalidWrite)
        }
        if let Some((slot,offset))=self.frame.write(bytes.len() as u64)? {
            // The view is dropped before command submission/unmap. Exactly the
            // written bytes are copied; alignment holes are never copied.
            {let mut view=self.buffers[slot].slice(offset..offset+bytes.len() as u64).get_mapped_range_mut();view.copy_from_slice(bytes);}
            encoder.copy_buffer_to_buffer(&self.buffers[slot],offset,target,target_offset,bytes.len() as u64);
        }else{queue.write_buffer(target,target_offset,bytes);}
        Ok(())
    }
    pub(super) fn finish(&mut self)->Result<(),Error>{
        if self.frame.selection.is_none(){self.frame.plan(Some(0))?;}
        if let Some(slot)=self.frame.seal()? {self.buffers[slot].unmap();}
        Ok(())
    }
    /// Call ONLY after the encoder was actually submitted on this same Queue.
    /// No callback locks the State mutex held by the submitter.
    pub(super) fn submitted(mut self)->Result<(),Error>{
        if let Some((slot,epoch))=self.frame.submitted()? {
            let mailbox=Arc::clone(self.mailbox);
            self.buffers[slot].slice(..).map_async(wgpu::MapMode::Write,move|result|{
                mailbox.slots[slot].store(epoch*4+if result.is_ok(){1}else{2},Ordering::Release);
            });
        }
        Ok(())
    }
}

impl super::Renderer {
    pub(crate) fn configure_transparent_staging(&mut self,mode:Mode)->anyhow::Result<()> {
        anyhow::ensure!(self.chunks.is_empty(),"configure transparent staging before world geometry");
        if let Some(owner)=&self.transparent_staging {
            anyhow::ensure!(mode==Mode::Staged,"staging mode is immutable for a Renderer");
            anyhow::ensure!(!owner.lock().map_err(|_|anyhow::anyhow!("staging mutex poisoned"))?.book.poisoned,"staging owner poisoned");
            return Ok(())
        }
        if mode==Mode::Staged {
            anyhow::ensure!(self.gpu_timestamps.is_none(),"staging trial conflicts with GPU timestamps: transfer scope changes");
            anyhow::ensure!(self.translucent_mdi.is_none(),"staging trial conflicts with translucent MDI experiment");
            self.transparent_staging=Some(State::new(&self.device)?);
        }
        Ok(())
    }
    pub(crate) fn transparent_staging_report(&self)->serde_json::Value {
        match &self.transparent_staging {
            None=>serde_json::json!({"enabled":false,"mode":"queue"}),
            Some(owner)=>match owner.lock(){Ok(state)=>state.report(),Err(_)=>serde_json::json!({"enabled":true,"mode":"staged","poisoned":true,"mutex_poisoned":true})},
        }
    }
    #[cfg(test)]
    pub(super) fn staging_test_capacity(&mut self,bytes:u64){
        let owner=self.transparent_staging.as_ref().expect("configured staging");
        assert_eq!(owner.lock().unwrap().book.counts.begun_frames,0);
        self.transparent_staging=Some(State::with_capacity(&self.device,bytes).unwrap());
    }
    #[cfg(test)]
    pub(super) fn staging_test_hold_recalled(&self,hold:bool){
        self.transparent_staging.as_ref().unwrap().lock().unwrap().book.hold_recalled=hold;
    }
}

#[cfg(test)]
#[path="transparent_staging_tests.rs"]
mod tests;
