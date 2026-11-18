//! Instrumented copies used only by explicit arena-upload capture.
use super::*;
use super::super::arena_upload_capture::{Trace,Phase};
use std::time::Instant;

struct CapturedFactory<'a,'t>{device:&'a wgpu::Device,trace:&'t mut Trace,#[cfg(test)] fail_page:bool}
impl PageFactory for CapturedFactory<'_,'_> {type Handle=wgpu::Buffer;type Error=String;
    fn create(&mut self,kind:PageKind,bytes:u64)->Result<wgpu::Buffer,String> {
        let i=match kind{PageKind::Vertex=>0,PageKind::Index(IndexWidth::U16)=>1,PageKind::Index(IndexWidth::U32)=>2};
        self.trace.observation.page_attempts[i]+=1;self.trace.observation.page_attempted_bytes[i]+=bytes;
        let started=Instant::now();
        let result=self.create_inner(kind,bytes);
        self.trace.phase(Phase::Page,started);result
    }
}
impl CapturedFactory<'_,'_> {
    fn create_inner(&mut self,kind:PageKind,bytes:u64)->Result<wgpu::Buffer,String> {
        #[cfg(test)] if std::mem::take(&mut self.fail_page) {
            return checked_buffer_captured(self.device,16,wgpu::BufferUsages::MAP_READ|wgpu::BufferUsages::VERTEX,
                "injected invalid page descriptor",self.trace);
        }
        let usage=match kind{PageKind::Vertex=>wgpu::BufferUsages::VERTEX,PageKind::Index(_)=>wgpu::BufferUsages::INDEX};
        checked_buffer_captured(self.device,bytes,usage|wgpu::BufferUsages::COPY_DST,"experimental bounded near page",self.trace)
    }
}
pub(in crate::renderer) fn checked_buffer_captured(device:&wgpu::Device,bytes:u64,usage:wgpu::BufferUsages,label:&str,trace:&mut Trace)->Result<wgpu::Buffer,String> {
    if bytes>device.limits().max_buffer_size{return Err("page exceeds negotiated max_buffer_size".into());}
    device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);device.push_error_scope(wgpu::ErrorFilter::Validation);
    #[cfg(test)] let usage=usage|wgpu::BufferUsages::COPY_SRC;
    let creating=Instant::now();
    let buffer=device.create_buffer(&wgpu::BufferDescriptor{label:Some(label),size:bytes,usage,mapped_at_creation:false});
    trace.phase(Phase::Create,creating);trace.observation.buffer_creates+=1;
    let resolving=Instant::now();
    let validation=pollster::block_on(device.pop_error_scope());let oom=pollster::block_on(device.pop_error_scope());
    trace.phase(Phase::Resolve,resolving);
    if let Some(error)=validation.or(oom){Err(error.to_string())}else{Ok(buffer)}
}
pub(in crate::renderer) fn checked_writes_captured(device:&wgpu::Device,trace:&mut Trace,write:impl FnOnce(&mut Trace))->Result<(),String> {
    device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);device.push_error_scope(wgpu::ErrorFilter::Validation);
    write(trace);
    let resolving=Instant::now();
    let validation=pollster::block_on(device.pop_error_scope());let oom=pollster::block_on(device.pop_error_scope());
    trace.phase(Phase::Resolve,resolving);
    if let Some(error)=validation.or(oom){Err(error.to_string())}else{Ok(())}
}
impl Renderer {
    pub(super) fn upload_arena_captured(&mut self,p:ChunkPos,prepared:PreparedChunk,mut trace:Trace) {
        trace.observation.map_len_before=self.chunks.len();trace.observation.map_capacity_before=self.chunks.capacity();
        trace.observation.vacant=!self.chunks.contains_key(&p);
        // Scalar lengths only. Also describe explicit Dedicated, whose unchanged
        // upload routine remains a single inclusive control span below.
        let request=prepared.arena_request([256,256]);
        trace.observation.submitted_bytes=[request.vertex_bytes,request.index_bytes];
        trace.observation.index_width=if request.width==IndexWidth::U16{2}else{4};
        if let Some(chunk)=self.chunks.get(&p){chunk.arena_capture_old_metadata(&mut trace);}
        self.upload_arena_captured_body(p,prepared,&mut trace);
        trace.observation.detailed_buffer_calls_measured=!trace.observation.explicit_dedicated;
        // The callee has returned and dropped its MutexGuard, cloned Arc and
        // any remaining locals. No explicit resource drop was relocated here.
        trace.observation.map_capacity_after=self.chunks.capacity();
        if let Some(chunk)=self.chunks.get(&p){trace.observation.new_shared=chunk.gpu.arena_token.is_some();
            trace.observation.new_capacity=[chunk.gpu.vertex_capacity,chunk.gpu.index_capacity];}
        self.arena_upload_capture.record(trace.finish());
    }
}

impl Renderer {
    fn upload_arena_captured_body(&mut self,p:ChunkPos,prepared:PreparedChunk,trace:&mut Trace) {
        let shared=self.near_arena.as_ref().expect("explicit arena experiment").clone();
        let mutex=Instant::now();
        let mut state=shared.lock().expect("near arena owner poisoned");
        trace.phase(Phase::Mutex,mutex);state.counts.uploads+=1;
        // Same invalidation ordering in all explicit arms, before any queued
        // destination writes. Experimental arms uniformly bypass near bundles.
        let cache=Instant::now();
        self.near_opaque_runs.payload_changed(p);self.terrain_bundles.invalidate_position(p);self.section_view.invalidate();
        trace.phase(Phase::Cache,cache);
        let old=self.chunks.get(&p).and_then(|chunk|chunk.gpu.arena_token);
        if state.capabilities.effective==Mode::Dedicated {
            state.counts.dedicated_control+=1;trace.observation.explicit_dedicated=true;let control=Instant::now();
            self.chunks.entry(p).or_insert_with(||ChunkMesh::new(&self.device)).update_prepared(&self.device,&self.queue,prepared);trace.phase(Phase::Control,control);return;
        }
        let capacities=self.chunks.get(&p).map_or([256,256],|chunk|[chunk.gpu.vertex_capacity,chunk.gpu.index_capacity]);
        let request=prepared.arena_request(capacities);
        trace.observation.submitted_bytes=[request.vertex_bytes,request.index_bytes];
        trace.observation.index_width=if request.width==IndexWidth::U16{2}else{4};
        #[cfg(test)] let fail_page=std::mem::take(&mut state.faults.page);
        #[cfg(test)] let fail_shared=std::mem::take(&mut state.faults.shared_write);
        #[cfg(test)] let fail_dedicated=std::mem::take(&mut state.faults.dedicated);
        let result={let admission=Instant::now();
            let admitted={let mut factory=CapturedFactory{device:&self.device,trace,#[cfg(test)] fail_page};
                state.arena.begin_update(old,request,&mut factory)};
            trace.phase(Phase::Admission,admission);
            match admitted {
                Ok(mut pending)=>{let mut gpu={let binding=pending.target();
                    #[cfg(test)] if fail_shared {
                        checked_writes(&self.device,||self.queue.write_buffer(binding.vertex,binding.vertex.size(),&[0;4]))
                            .expect("injected shared target failure must poison Pending");
                    }
                    let writing=Instant::now();
                    let result=prepared.write_shared_arena_captured(&self.device,&self.queue,&binding,trace);
                    trace.phase(Phase::SharedWrite,writing);
                    result
                        .unwrap_or_else(|error|panic!("experimental shared upload failed after target: {error}"))};
                    let committing=Instant::now();gpu.arena_token=Some(pending.commit());trace.phase(Phase::Commit,committing);Ok(gpu)},
                Err(error)=>Err(error),
            }};
        match result {
            Ok(gpu)=>{state.counts.shared_uploads+=1;
                if let Some(chunk)=self.chunks.get_mut(&p){chunk.install_prepared_gpu_arena_captured(prepared,gpu,trace);trace.returned();}
                else{let chunk=ChunkMesh::from_prepared_gpu_arena_captured(prepared,gpu,trace);trace.returned();
                    let inserting=Instant::now();self.chunks.insert(p,chunk);trace.phase(Phase::MapInsert,inserting);}
            },
            Err(error)=>{
                // Before-target admission failure leaves old bytes and token
                // intact. Complete standalone replacement BEFORE releasing old.
                if let near_arena::Failure::Admission(reason)=&error {use near_arena::Reject::*;
                    if !matches!(reason,InvalidRequest|OwnerLimit|PageLimit|Fragmented|Metadata) {
                        panic!("experimental arena consistency failure: {error:?}");
                    }
                }
                trace.observation.fallback=true;
                trace.observation.fallback_reason=match &error {near_arena::Failure::Page(_)=>6,
                    near_arena::Failure::Admission(reason)=>match reason {near_arena::Reject::InvalidRequest=>1,
                        near_arena::Reject::OwnerLimit=>2,near_arena::Reject::PageLimit=>3,
                        near_arena::Reject::Fragmented=>4,near_arena::Reject::Metadata=>5,_=>0}};
                state.counts.fallbacks+=1;state.counts.page_failures+=u64::from(matches!(error,near_arena::Failure::Page(_)));
                state.last_fallback=Some(format!("{error:?}"));
                // Validate BOTH replacement buffers and their queued writes
                // before touching the previous valid chunk or releasing token.
                #[cfg(test)] if fail_dedicated {
                    checked_buffer(&self.device,16,wgpu::BufferUsages::MAP_READ|wgpu::BufferUsages::VERTEX,"injected invalid dedicated fallback")
                        .expect("injected dedicated failure retains old shared owner");
                }
                let fallback=Instant::now();
                let result=prepared.write_dedicated_arena_captured(&self.device,&self.queue,capacities,trace);
                trace.phase(Phase::Fallback,fallback);
                let gpu=result
                    .unwrap_or_else(|error|panic!("experimental dedicated fallback failed; old owner retained: {error}"));
                if let Some(chunk)=self.chunks.get_mut(&p){chunk.install_prepared_gpu_arena_captured(prepared,gpu,trace);trace.returned();}
                else{let chunk=ChunkMesh::from_prepared_gpu_arena_captured(prepared,gpu,trace);trace.returned();
                    let inserting=Instant::now();self.chunks.insert(p,chunk);trace.phase(Phase::MapInsert,inserting);}
                if let Some(token)=old{let releasing=Instant::now();state.arena.release(token).expect("old fallback token still owned");trace.phase(Phase::Release,releasing);}
            }
        }
    }
}
