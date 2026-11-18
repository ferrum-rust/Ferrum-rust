//! Guarded exact-distance automatic admission plus explicit benchmark controls.
use super::*;
#[path="arena_upload_traced.rs"]
pub(super) mod upload_traced;
use super::near_arena::{Arena,IndexWidth,Limits,Mode,PageFactory,PageKind,Recording};
use super::near_arena_plan::{Command,DrawLimits,Plan,Source,Row,Backing};
use std::sync::Mutex;

#[derive(Clone,Copy,Debug,PartialEq,Eq,clap::ValueEnum)]
pub(crate) enum Choice {Original,Dedicated,Direct,Mdi}
impl Choice {fn mode(self)->Mode{match self{Self::Original|Self::Dedicated=>Mode::Dedicated,Self::Direct=>Mode::ArenaDirect,Self::Mdi=>Mode::ArenaMdi}}}
#[derive(Clone, Copy)]
pub(super) struct Capabilities {requested:Choice,effective:Mode,native_max:Option<u32>,advertised:bool,
    indirect:bool,base_vertex:bool,reason:&'static str}
impl Capabilities {
    pub(super) fn inspect(adapter:&wgpu::Adapter,requested:Choice)->Self {
        let flags=adapter.get_downlevel_capabilities().flags;
        let advertised=adapter.features().contains(wgpu::Features::MULTI_DRAW_INDIRECT);
        let indirect=flags.contains(wgpu::DownlevelFlags::INDIRECT_EXECUTION);
        let base_vertex=flags.contains(wgpu::DownlevelFlags::BASE_VERTEX);
        let native_max=if requested==Choice::Mdi {
            // Read only the SAME selected Vulkan adapter. No raw handle escapes.
            unsafe {adapter.as_hal::<wgpu::hal::api::Vulkan,_,_>(|a|a.map(|a|
                a.shared_instance().raw_instance().get_physical_device_properties(a.raw_physical_device()).limits.max_draw_indirect_count))}
        }else{None};
        let accepted=advertised&&indirect&&base_vertex&&native_max.is_some_and(|n|n>=2);
        let (effective,reason)=if requested==Choice::Mdi&&!accepted{(Mode::ArenaDirect,"MDI capability/native count unavailable; explicit arena-direct fallback")}
            else{(requested.mode(),"requested mode supported")};
        Self{requested,effective,native_max,advertised,indirect,base_vertex,reason}
    }
    pub(super) fn supports_automatic(&self)->bool {self.effective==Mode::ArenaMdi}
    pub(super) fn features(&self)->wgpu::Features {if self.effective==Mode::ArenaMdi{wgpu::Features::MULTI_DRAW_INDIRECT}else{wgpu::Features::empty()}}
}
struct Factory<'a>{device:&'a wgpu::Device,#[cfg(test)] fail_page:bool}
impl PageFactory for Factory<'_> {type Handle=wgpu::Buffer;type Error=String;
    fn create(&mut self,kind:PageKind,bytes:u64)->Result<wgpu::Buffer,String> {
        #[cfg(test)] if std::mem::take(&mut self.fail_page) {
            return checked_buffer(self.device,16,wgpu::BufferUsages::MAP_READ|wgpu::BufferUsages::VERTEX,"injected invalid page descriptor");
        }
        let usage=match kind{PageKind::Vertex=>wgpu::BufferUsages::VERTEX,PageKind::Index(_)=>wgpu::BufferUsages::INDEX};
        checked_buffer(self.device,bytes,usage|wgpu::BufferUsages::COPY_DST,"experimental bounded near page")
    }
}
pub(super) fn checked_buffer(device:&wgpu::Device,bytes:u64,usage:wgpu::BufferUsages,label:&str)->Result<wgpu::Buffer,String> {
    if bytes>device.limits().max_buffer_size{return Err("page exceeds negotiated max_buffer_size".into());}
    device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);device.push_error_scope(wgpu::ErrorFilter::Validation);
    #[cfg(test)] let usage=usage|wgpu::BufferUsages::COPY_SRC;
    let buffer=device.create_buffer(&wgpu::BufferDescriptor{label:Some(label),size:bytes,usage,mapped_at_creation:false});
    let validation=pollster::block_on(device.pop_error_scope());let oom=pollster::block_on(device.pop_error_scope());
    if let Some(error)=validation.or(oom){Err(error.to_string())}else{Ok(buffer)}
}
pub(super) fn checked_writes(device:&wgpu::Device,write:impl FnOnce())->std::result::Result<(),String> {
    device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);device.push_error_scope(wgpu::ErrorFilter::Validation);
    write();
    let validation=pollster::block_on(device.pop_error_scope());let oom=pollster::block_on(device.pop_error_scope());
    if let Some(error)=validation.or(oom){Err(error.to_string())}else{Ok(())}
}
#[derive(Default)]
struct Counts {uploads:u64,shared_uploads:u64,dedicated_control:u64,fallbacks:u64,page_failures:u64,
    frames:u64,records:u64,multi_calls:u64,multi_records:u64,direct_calls:u64,argument_bytes:u64,plan_limit_fallbacks:u64,clear_released_page_handles:u64,clear_released_committed_bytes:u64}
#[cfg(test)]
#[derive(Default)]
struct Faults{page:bool,dedicated:bool,shared_write:bool}
pub(super) struct State {capabilities:Capabilities,arena:Arena<wgpu::Buffer>,plan:Plan,arguments:wgpu::Buffer,
    limits:DrawLimits,arena_limits:Limits,counts:Counts,last_fallback:Option<String>,#[cfg(test)] faults:Faults}
pub(super) type Shared=Arc<Mutex<State>>;
impl State {
    pub(super) fn new(device:&wgpu::Device,capabilities:Capabilities)->Result<Shared> {
        anyhow::ensure!(device.limits().max_buffer_size>=256*1024*1024,"experimental vertex page exceeds negotiated device limit");
        let limits=DrawLimits::negotiated(device.features().contains(wgpu::Features::MULTI_DRAW_INDIRECT),
            capabilities.indirect,capabilities.base_vertex,capabilities.native_max);
        let arguments=checked_buffer(device,(near_arena_plan::MAX_RECORDS*near_arena_plan::ARGUMENT_BYTES)as u64,
            wgpu::BufferUsages::INDIRECT|wgpu::BufferUsages::COPY_DST,"bounded same-frame opaque arguments").map_err(anyhow::Error::msg)?;
        Ok(Arc::new(Mutex::new(Self{arena:Arena::new(capabilities.effective,Limits::default()).map_err(|e|anyhow::anyhow!("arena: {e:?}"))?,
            plan:Plan::new().map_err(|e|anyhow::anyhow!("arena plan: {e:?}"))?,arguments,limits,arena_limits:Limits::default(),capabilities,counts:Counts::default(),last_fallback:None,#[cfg(test)] faults:Faults::default()})))
    }
    pub(super) fn frame(&mut self)->Session<'_> {
        Session{recording:self.arena.begin_recording().expect("near arena recording state must be healthy"),
            plan:&mut self.plan,arguments:&self.arguments,limits:self.limits,mode:self.capabilities.effective,counts:&mut self.counts}
    }
}
pub(super) struct Session<'a>{recording:Recording<'a,wgpu::Buffer>,plan:&'a mut Plan,arguments:&'a wgpu::Buffer,
    limits:DrawLimits,mode:Mode,counts:&'a mut Counts}
impl Session<'_>{pub(super) fn submitted(self){self.recording.submitted();}}
pub(super) struct Visible<'a,'b>{rows:&'a[(ChunkPos,&'b mut ChunkMesh)]}
impl<'a,'b> Visible<'a,'b>{pub(super) fn new(rows:&'a[(ChunkPos,&'b mut ChunkMesh)])->Self{Self{rows}}}
impl Source for Visible<'_,'_>{fn len(&self)->usize{self.rows.len()}fn row(&self,i:usize)->Row<'_>{let chunk=&self.rows[i].1;
    Row{backing:chunk.gpu.arena_token.map_or(Backing::Dedicated,Backing::Arena),
        width:if chunk.gpu.index_format==wgpu::IndexFormat::Uint16{IndexWidth::U16}else{IndexWidth::U32},ranges:&chunk.solid_ranges}}}
pub(super) struct DrawPlan<'p,'r,'a,'v,'b>{prepared:near_arena_plan::Prepared<'p,'r,'a,wgpu::Buffer,Visible<'v,'b>>,arguments:&'p wgpu::Buffer}
impl<'a> Session<'a> {
    pub(super) fn translucent_limits(&self)->Option<DrawLimits>{
        (self.mode==Mode::ArenaMdi&&self.limits.multi_draw_enabled()).then_some(self.limits)
    }
    pub(super) fn prepare<'p,'v,'b>(&'p mut self,source:&'p Visible<'v,'b>,queue:&wgpu::Queue)
        ->Option<DrawPlan<'p,'p,'a,'v,'b>> {
        match self.plan.build(self.mode,self.limits,&self.recording,source) {
            Ok(prepared)=>{let s=prepared.summary();self.counts.frames+=1;self.counts.records+=s.records as u64;
                self.counts.multi_calls+=s.multi_calls as u64;self.counts.multi_records+=s.multi_records as u64;
                self.counts.direct_calls+=s.direct_calls as u64;
                if s.multi_calls>0 {queue.write_buffer(self.arguments,0,prepared.argument_bytes());self.counts.argument_bytes+=prepared.argument_bytes().len() as u64;}
                Some(DrawPlan{prepared,arguments:self.arguments})},
            Err(near_arena_plan::Error::TooManyRecords)=>{self.counts.plan_limit_fallbacks+=1;None},
            Err(error)=>panic!("unsafe/stale arena draw plan: {error:?}"),
        }
    }
}
impl<'p,'r,'a,'v,'b> DrawPlan<'p,'r,'a,'v,'b>{
    pub(super) fn recording(&self)->&'r Recording<'a,wgpu::Buffer>{self.prepared.recording()}
    pub(super) fn draw<'pass>(&'pass self,pass:&mut wgpu::RenderPass<'pass>,visible:&'pass[(ChunkPos,&mut ChunkMesh)],profile:&mut render_profile::Frame) {
        for command in self.prepared.commands(){match command{
            Command::Direct{row,range}=>{visible[*row].1.gpu.draw_range(pass,range.clone());
                if profile.enabled{profile.counters.near_direct_draws+=1;}},
            Command::Multi{row,binding,first_arg,count}=>{let b=self.prepared.binding(*row).expect("live draw token").expect("shared multi row");
                assert_eq!((binding.arena_id,binding.vertex_page,binding.index_page),(b.arena_id,b.vertex_page,b.index_page));
                pass.set_vertex_buffer(0,b.vertex.slice(..));pass.set_index_buffer(b.index.slice(..),
                    if binding.width==IndexWidth::U16{wgpu::IndexFormat::Uint16}else{wgpu::IndexFormat::Uint32});
                pass.multi_draw_indexed_indirect(self.arguments,u64::from(*first_arg)*20,*count);
            }
        }}
    }
}

impl Renderer {
    /// Call only at the main owner boundary, outside any encode/submit session.
    pub(super) fn reconcile_arena_policy(&mut self) {
        use super::renderer_arena_policy::Selection;
        match self.arena_policy.select(self.near_arena.is_some(), self.chunks.is_empty()) {
            Selection::Hold => {},
            Selection::Original => {
                // Live shared handles must never lose their owner. Policy only
                // chooses this for an inactive owner or after all chunks clear.
                assert!(self.near_arena.is_none() || self.chunks.is_empty());
                self.near_arena = None;
            },
            Selection::Arena(choice) => {
                assert!(self.near_arena.is_none());
                assert!(self.chunks.values().all(|chunk| chunk.gpu.arena_token.is_none()
                    && chunk.gpu.vertex_offset == 0 && chunk.gpu.index_offset == 0));
                let caps = self.arena_capabilities.expect("eligible selection has capabilities");
                // Override is immutable for this Renderer. Automatic admission
                // always uses its pre-negotiated MDI capabilities; settings
                // change eligibility, never the negotiated mode or Device.
                assert_eq!(caps.requested, choice);
                #[cfg(test)]
                let injected = std::mem::take(&mut self.fail_automatic_arena_once);
                #[cfg(not(test))]
                let injected = false;
                let result = if injected {
                    renderer_arena::checked_buffer(&self.device, 16,
                        wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::VERTEX,
                        "injected automatic arena initialization validation failure")
                        .and_then(|_| Err("injected automatic state failure".to_string()))
                        .map_err(anyhow::Error::msg)
                } else { renderer_arena::State::new(&self.device, caps) };
                match result {
                    Ok(state) => {
                        // Publish only after complete checked construction.
                        // No chunks/indices/vertices are copied or replaced.
                        self.terrain_bundles.invalidate();
                        self.near_opaque_runs.cleared();
                        self.near_arena = Some(state);
                    },
                    Err(error) if self.arena_policy.automatic() => {
                        self.arena_policy.initialization_failed(&format!("automatic arena initialization failed: {error}"));
                    },
                    Err(error) => panic!("explicit arena initialization failed: {error}"),
                }
            },
        }
    }
    pub(super) fn suppress_automatic_arena(&mut self, reason: &'static str) {
        if self.arena_policy.automatic() {
            // Resident dedicated meshes are already valid diagnostic input.
            // Only an active shared owner requires clearing those meshes first.
            assert!(self.near_arena.is_none() || self.chunks.is_empty(),
                "clear active arena geometry before enabling original-path diagnostics");
            self.arena_policy.suppress_diagnostic(reason);
            self.reconcile_arena_policy();
        }
    }
    #[cfg(test)]
    pub(super) fn arena_test_fail_automatic_initialization_once(&mut self) {
        assert!(self.near_arena.is_none() && self.arena_policy.automatic());
        self.fail_automatic_arena_once = true;
    }

    pub(super) fn upload_arena(&mut self,p:ChunkPos,prepared:PreparedChunk) {
        if let Some(trace)=self.arena_upload_capture.start(p){self.upload_arena_captured(p,prepared,trace);return;}
        let shared=self.near_arena.as_ref().expect("explicit arena experiment").clone();
        let mut state=shared.lock().expect("near arena owner poisoned");state.counts.uploads+=1;
        // Same invalidation ordering in all explicit arms, before any queued
        // destination writes. Experimental arms uniformly bypass near bundles.
        self.near_opaque_runs.payload_changed(p);self.terrain_bundles.invalidate_position(p);self.section_view.invalidate();
        let old=self.chunks.get(&p).and_then(|chunk|chunk.gpu.arena_token);
        if state.capabilities.effective==Mode::Dedicated {
            state.counts.dedicated_control+=1;
            self.chunks.entry(p).or_insert_with(||ChunkMesh::new(&self.device)).update_prepared(&self.device,&self.queue,prepared);return;
        }
        let capacities=self.chunks.get(&p).map_or([256,256],|chunk|[chunk.gpu.vertex_capacity,chunk.gpu.index_capacity]);
        let request=prepared.arena_request(capacities);
        #[cfg(test)] let fail_page=std::mem::take(&mut state.faults.page);
        #[cfg(test)] let fail_shared=std::mem::take(&mut state.faults.shared_write);
        #[cfg(test)] let fail_dedicated=std::mem::take(&mut state.faults.dedicated);
        let result={let mut factory=Factory{device:&self.device,#[cfg(test)] fail_page};
            match state.arena.begin_update(old,request,&mut factory) {
                Ok(mut pending)=>{let mut gpu={let binding=pending.target();
                    #[cfg(test)] if fail_shared {
                        checked_writes(&self.device,||self.queue.write_buffer(binding.vertex,binding.vertex.size(),&[0;4]))
                            .expect("injected shared target failure must poison Pending");
                    }
                    prepared.write_shared(&self.device,&self.queue,&binding)
                        .unwrap_or_else(|error|panic!("experimental shared upload failed after target: {error}"))};
                    gpu.arena_token=Some(pending.commit());Ok(gpu)},
                Err(error)=>Err(error),
            }};
        match result {
            Ok(gpu)=>{state.counts.shared_uploads+=1;
                if let Some(chunk)=self.chunks.get_mut(&p){chunk.install_prepared_gpu(prepared,gpu);}
                else{self.chunks.insert(p,ChunkMesh::from_prepared_gpu(prepared,gpu));}
            },
            Err(error)=>{
                // Before-target admission failure leaves old bytes and token
                // intact. Complete standalone replacement BEFORE releasing old.
                if let near_arena::Failure::Admission(reason)=&error {use near_arena::Reject::*;
                    if !matches!(reason,InvalidRequest|OwnerLimit|PageLimit|Fragmented|Metadata) {
                        panic!("experimental arena consistency failure: {error:?}");
                    }
                }
                state.counts.fallbacks+=1;state.counts.page_failures+=u64::from(matches!(error,near_arena::Failure::Page(_)));
                state.last_fallback=Some(format!("{error:?}"));
                // Validate BOTH replacement buffers and their queued writes
                // before touching the previous valid chunk or releasing token.
                #[cfg(test)] if fail_dedicated {
                    checked_buffer(&self.device,16,wgpu::BufferUsages::MAP_READ|wgpu::BufferUsages::VERTEX,"injected invalid dedicated fallback")
                        .expect("injected dedicated failure retains old shared owner");
                }
                let gpu=prepared.write_dedicated_checked(&self.device,&self.queue,capacities)
                    .unwrap_or_else(|error|panic!("experimental dedicated fallback failed; old owner retained: {error}"));
                if let Some(chunk)=self.chunks.get_mut(&p){chunk.install_prepared_gpu(prepared,gpu);}
                else{self.chunks.insert(p,ChunkMesh::from_prepared_gpu(prepared,gpu));}
                if let Some(token)=old{state.arena.release(token).expect("old fallback token still owned");}
            }
        }
    }
    pub(super) fn remove_arena(&mut self,p:ChunkPos) {
        if let Some(old)=self.chunks.remove(&p){self.near_opaque_runs.removed(p);self.terrain_bundles.invalidate_position(p);self.section_view.invalidate();
            if let Some(token)=old.gpu.arena_token{self.near_arena.as_ref().unwrap().lock().expect("arena poisoned").arena.release(token).expect("removed token live");}
        }
    }
    pub(super) fn clear_arena(&mut self) {
        // Called after every cache and chunk handle has been dropped. Previous
        // submitted GPU commands retain resources; there is no unsubmitted session.
        let mut state=self.near_arena.as_ref().unwrap().lock().expect("arena poisoned");
        let old=state.arena.stats();state.counts.clear_released_page_handles+=(old.vertex_pages+old.index_pages) as u64;
        state.counts.clear_released_committed_bytes+=old.committed_bytes;
        state.arena=Arena::new(state.capabilities.effective,state.arena_limits).expect("validated arena limits");
    }
    #[cfg(test)]
    pub(super) fn arena_test_limits(&mut self,limits:Limits) {
        assert!(self.chunks.values().all(|chunk|chunk.gpu.arena_token.is_none()));let mut state=self.near_arena.as_ref().unwrap().lock().unwrap();
        assert_eq!(state.arena.stats().owners,0);state.arena=Arena::new(state.capabilities.effective,limits).unwrap();state.arena_limits=limits;
    }
    #[cfg(test)]
    pub(super) fn arena_test_health(&self)->(bool,bool) {
        let shared=self.near_arena.as_ref().unwrap();let poisoned=shared.is_poisoned();
        let mut state=shared.lock().unwrap_or_else(|error|error.into_inner());
        let healthy=match state.arena.begin_recording(){Ok(recording)=>{recording.discarded();true},Err(_)=>false};
        (poisoned,healthy)
    }
    #[cfg(test)]
    pub(super) fn arena_test_clear_mutex_poison(&self) {
        assert!(self.arena_test_health().1,"a post-target poisoned Arena cannot be recovered");
        self.near_arena.as_ref().unwrap().clear_poison();
    }
    #[cfg(test)]
    pub(super) fn submit_arena_test_frame(&mut self,mut encoder:wgpu::CommandEncoder,output:&wgpu::TextureView,
        input:&RenderInput<'_>,profile:&mut render_profile::Frame,after:impl FnOnce(&mut wgpu::CommandEncoder,&Renderer)) {
        let prepared_view=self.prepare_pre_acquire_visibility(input,profile);
        self.finish_pre_acquire_visibility(prepared_view.is_some(),Ok(())).expect("test target acquired");
        let gpu_ticket=if profile.enabled {
            self.gpu_timestamps.as_mut().and_then(|capture|capture.reserve(profile.frame_id,input.time))
        }else{None};
        let owner=self.near_arena.as_ref().map(Arc::clone);
        let mut state=owner.as_ref().map(|owner|owner.lock().expect("arena owner poisoned"));
        let mut session=state.as_mut().map(|state|state.frame());
        let staging_owner=self.transparent_staging.as_ref().map(Arc::clone);
        let mut staging_state=staging_owner.as_ref().map(|state|state.lock().expect("staging owner poisoned"));
        let mut staging=staging_state.as_mut().map(|state|state.frame(input.settings.render_distance==128).expect("staging frame rejected"));
        if let (Some(capture),Some(ticket))=(&mut self.gpu_timestamps,gpu_ticket.as_ref()) {
            capture.encode_start(&mut encoder,ticket).unwrap();
        }
        self.encode_inner(&mut encoder,output,input,profile,session.as_mut(),staging.as_mut(),prepared_view);
        if let Some(staging)=staging.as_mut(){staging.finish().expect("staging finish");}
        if let (Some(capture),Some(ticket))=(&mut self.gpu_timestamps,gpu_ticket.as_ref()) {
            capture.encode_end(&mut encoder,ticket).unwrap();
        }
        // Copy/readback encoding only: callback must not re-lock arena state.
        after(&mut encoder,self);self.queue.submit(Some(encoder.finish()));
        if let Some(session)=session.take(){session.submitted();}
        if let Some(staging)=staging.take(){staging.submitted().expect("staging submitted");};
        if let (Some(capture),Some(ticket))=(&mut self.gpu_timestamps,gpu_ticket) {
            capture.submitted(ticket,gpu_timestamps::CpuTimes::default()).unwrap();
        }
    }
    #[cfg(test)]
    pub(super) fn staging_test_abandon_frame(&mut self,mut encoder:wgpu::CommandEncoder,output:&wgpu::TextureView,
        input:&RenderInput<'_>,profile:&mut render_profile::Frame) {
        let owner=self.near_arena.as_ref().map(Arc::clone);
        let mut state=owner.as_ref().map(|owner|owner.lock().expect("arena owner poisoned"));
        let mut session=state.as_mut().map(|state|state.frame());
        let staging_owner=self.transparent_staging.as_ref().expect("configured staging").clone();
        let mut staging_state=staging_owner.lock().expect("staging owner poisoned");
        let mut staging=staging_state.frame(input.settings.render_distance==128).expect("staging frame rejected");
        self.encode_inner(&mut encoder,output,input,profile,session.as_mut(),Some(&mut staging),None);
        staging.finish().expect("staging finish");
        drop(encoder); // Deliberate negative control: no submit/acknowledgement.
    }
    #[cfg(test)]
    pub(super) fn arena_test_faults(&self,page:bool,dedicated:bool,shared_write:bool) {
        let mut state=self.near_arena.as_ref().unwrap().lock().unwrap();state.faults=Faults{page,dedicated,shared_write};
    }
    pub(super) fn arena_unique_pages(&self)->Option<[usize;2]> {
        let shared=self.near_arena.as_ref()?;let state=shared.lock().ok()?;let stats=state.arena.stats();
        Some([stats.vertex_pages,stats.index_pages])
    }
    pub(crate) fn near_arena_report(&self)->serde_json::Value {
        let policy=self.arena_policy.report(self.near_arena.is_some(),!self.chunks.is_empty());
        let Some(shared)=&self.near_arena else{return serde_json::json!({"enabled":false,"ordinary_dedicated":true,"policy":policy})};
        let Ok(state)=shared.lock()else{return serde_json::json!({"enabled":true,"poisoned":true,"status":"failed closed","policy":policy})};
        let stats=state.arena.stats();let mut dedicated_bytes=0u64;let mut dedicated_chunks=0usize;let mut shared_chunks=0usize;
        for chunk in self.chunks.values(){if chunk.gpu.arena_token.is_some(){shared_chunks+=1;}else{
            dedicated_chunks+=1;dedicated_bytes+=chunk.gpu.vertex_capacity+chunk.gpu.index_capacity;}}
        serde_json::json!({"enabled":true,"policy":policy,"requested":format!("{:?}",state.capabilities.requested),"effective":format!("{:?}",state.capabilities.effective),
            "capability_reason":state.capabilities.reason,"advertised_mdi":state.capabilities.advertised,
            "enabled_mdi":state.limits.multi_draw_enabled(),"native_max_draw_indirect_count":state.capabilities.native_max,
            "effective_max_draws":state.limits.max_draws(),"near_cache_control":"active shared-owner path and all explicit arena arms disable near caches; explicit Original retains cached rendering",
            "unique_shared_vertex_pages":stats.vertex_pages,"unique_shared_index_pages":stats.index_pages,
            "unique_shared_committed_bytes":stats.committed_bytes,"shared_reserved_bytes":stats.reserved_bytes,"shared_used_bytes":stats.used_bytes,
            "shared_free_bytes":stats.free_bytes,"free_intervals":stats.free_intervals,
            "clear_released_page_handles":state.counts.clear_released_page_handles,"clear_released_committed_bytes":state.counts.clear_released_committed_bytes,"shared_chunks":shared_chunks,"dedicated_chunks":dedicated_chunks,"dedicated_capacity_bytes":dedicated_bytes,
            "combined_current_geometry_bytes":stats.committed_bytes+dedicated_bytes,"indirect_buffer_bytes":near_arena_plan::MAX_RECORDS*20,
            "uploads":state.counts.uploads,"shared_uploads":state.counts.shared_uploads,"dedicated_control_uploads":state.counts.dedicated_control,
            "fallbacks":state.counts.fallbacks,"page_creation_failures":state.counts.page_failures,"last_fallback":state.last_fallback,
            "planned_frames":state.counts.frames,"opaque_records":state.counts.records,"multi_calls_per_eye_not_scaled":state.counts.multi_calls,
            "multi_records_per_eye_not_scaled":state.counts.multi_records,"direct_calls_per_eye_not_scaled":state.counts.direct_calls,
            "argument_upload_bytes":state.counts.argument_bytes,"plan_limit_fallbacks":state.counts.plan_limit_fallbacks,
            "note":"Policy identifies automatic admission or explicit control; active arena is not the legacy cached dedicated path. One authoritative backing per chunk, no migration; this report covers the opaque plan, while translucent draw mode/counters are reported separately. Shared bytes count each persistent page once, including retained completely empty pages until clear; free bytes include those pages. Clear releases page handles, not proof of completed GPU/driver frees; dedicated fallback and indirect buffer are separate. Current resident counts exclude driver/staging/submitted-resource high-water. OOM/validation checked before page admission; failed post-target sessions poison rather than fake rollback."})
    }
}

#[cfg(test)]
#[path="renderer_arena_tests.rs"]
mod renderer_arena_tests;

#[cfg(test)]
#[path="renderer_arena_gpu_tests.rs"]
mod renderer_arena_gpu_tests;
