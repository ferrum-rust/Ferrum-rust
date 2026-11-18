//! Isolated collector oracle, not a Renderer/surface or performance benchmark.
//! All off/on submissions precede the one terminal readback wait. Distinct
//! copied markers expose accidental last-frame reuse of a persistent resource.
use super::*;

const FRAMES: usize = 96;
const WIDTH: u32 = 64;
const HEIGHT: u32 = 32;
const ROW: u32 = 256;
const PLANE: u64 = ROW as u64 * HEIGHT as u64;
const INDEX_OFFSET: u64 = PLANE * 2;
const MARKER_OFFSET: u64 = INDEX_OFFSET + 32;
const STRIDE: u64 = 16_640;

struct Workload {
    color: wgpu::Texture,
    depth: wgpu::Texture,
    color_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    uniform: wgpu::Buffer,
    indices: wgpu::Buffer,
    marker: wgpu::Buffer,
    bind: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}
impl Workload {
    fn new(device: &wgpu::Device) -> Self {
        let extent = wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 };
        let texture = |format| device.create_texture(&wgpu::TextureDescriptor {
            label: Some("timestamp oracle reused attachment"), size: extent,
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
            format, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let color = texture(wgpu::TextureFormat::Rgba8Unorm);
        let depth = texture(wgpu::TextureFormat::Depth32Float);
        let color_view = color.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let buffer = |label, size, usage| device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label), size, usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false,
        });
        let uniform = buffer("timestamp oracle changing uniform", 32, wgpu::BufferUsages::UNIFORM);
        let indices = buffer("timestamp oracle changing indices", 24,
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_SRC);
        let marker = buffer("timestamp oracle changing marker", 64, wgpu::BufferUsages::COPY_SRC);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("timestamp oracle layout"), entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false, min_binding_size: None }, count: None,
            }],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("timestamp oracle bind"), layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("timestamp oracle independent shader"),
            source: wgpu::ShaderSource::Wgsl(r#"
struct Uniforms { color: vec4<f32>, transform: vec4<f32> }
@group(0) @binding(0) var<uniform> u: Uniforms;
@vertex fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let p = array<vec2<f32>, 4>(vec2(-0.65, -0.6), vec2(0.65, -0.6),
                               vec2(0.65, 0.6), vec2(-0.65, 0.6));
    return vec4(p[index] + u.transform.xy, u.transform.z, 1.0);
}
@fragment fn fs() -> @location(0) vec4<f32> { return u.color; }
"#.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("timestamp oracle pipeline layout"), bind_group_layouts: &[&layout], push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("timestamp oracle pipeline"), layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs"),
                compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs"),
                compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm, blend: None, write_mask: wgpu::ColorWrites::ALL,
                })] }),
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true, depth_compare: wgpu::CompareFunction::Less,
                stencil: Default::default(), bias: Default::default() }),
            multisample: Default::default(), multiview: None, cache: None,
        });
        Self { color, depth, color_view, depth_view, uniform, indices, marker, bind, pipeline }
    }
    fn write(&self, queue: &wgpu::Queue, frame: usize) {
        let uniform = [
            (40 + frame % 97) as f32 / 255., (80 + frame * 3 % 101) as f32 / 255.,
            (120 + frame * 7 % 89) as f32 / 255., 1.,
            (frame % 7) as f32 * 0.04 - 0.12, (frame % 3) as f32 * 0.04 - 0.04,
            0.2 + (frame % 5) as f32 * 0.1, 0.,
        ];
        queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&uniform));
        queue.write_buffer(&self.indices, 0, bytemuck::cast_slice(&expected_indices(frame)));
        queue.write_buffer(&self.marker, 0, bytemuck::cast_slice(&expected_marker(frame)));
    }
    fn encode(&self, encoder: &mut wgpu::CommandEncoder, readback: &wgpu::Buffer, offset: u64) {
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("timestamp oracle changing draw"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &self.color_view,
                    resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 1./255., g: 2./255., b: 3./255., a: 1. }), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view, depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.), store: wgpu::StoreOp::Store }), stencil_ops: None,
                }), timestamp_writes: None, occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline); pass.set_bind_group(0, &self.bind, &[]);
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..6, 0, 0..1);
        }
        let extent = wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 };
        for (texture, plane, aspect) in [(&self.color, 0, wgpu::TextureAspect::All),
                                         (&self.depth, PLANE, wgpu::TextureAspect::DepthOnly)] {
            encoder.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo { texture, mip_level: 0,
                origin: wgpu::Origin3d::ZERO, aspect }, wgpu::TexelCopyBufferInfo { buffer: readback,
                layout: wgpu::TexelCopyBufferLayout { offset: offset + plane,
                    bytes_per_row: Some(ROW), rows_per_image: Some(HEIGHT) } }, extent);
        }
        encoder.copy_buffer_to_buffer(&self.indices, 0, readback, offset + INDEX_OFFSET, 24);
        encoder.copy_buffer_to_buffer(&self.marker, 0, readback, offset + MARKER_OFFSET, 64);
    }
}
fn expected_indices(frame: usize) -> [u32; 6] {
    if frame % 2 == 0 { [0, 1, 2, 0, 2, 3] } else { [2, 3, 0, 2, 0, 1] }
}
fn expected_marker(frame: usize) -> [u32; 16] {
    std::array::from_fn(|word| (frame as u32).wrapping_mul(0x9e37_79b9)
        ^ (word as u32).wrapping_mul(0x85eb_ca6b) ^ 0xa189_2026)
}

fn cpu_context(frame: usize) -> CpuTimes {
    // Synthetic identity metadata, not CPU timing measured by this fixture.
    CpuTimes { acquire_ms: 0.01 + frame as f64 * 0.001,
        encode_ms: 0.02, submit_ms: 0.03, present_ms: 0.04 }
}

#[test]
#[ignore = "actual Vulkan timestamp collector and queued readback oracle"]
fn gpu_timestamp_rows_preserve_queued_draws_copies_and_terminal_lifecycle() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN, ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None, force_fallback_adapter: false,
    })).expect("Vulkan adapter is required for this ignored oracle");
    let features = required_features(&adapter).expect("actual timestamp/nonblocking support is required");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("isolated timestamp oracle"), required_features: features,
        required_limits: wgpu::Limits::default(), memory_hints: wgpu::MemoryHints::Performance,
    }, None)).unwrap();
    let mut capture = Capture::new(&device, &queue).unwrap();
    let work = Workload::new(&device);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("timestamp oracle both arms append-only pixels indices markers"),
        size: 2 * FRAMES as u64 * STRIDE,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut last_ticket = None;
    // Same Device/features/workload in both arms. No map, poll, wait, or host
    // readback occurs in this loop, including between the off and on arms.
    for arm in 0..2 {
        for frame in 0..FRAMES {
            work.write(&queue, frame);
            let mut encoder = device.create_command_encoder(&Default::default());
            let ticket = if arm == 1 {
                capture.clear_pending_present();
                Some(capture.reserve(1000 + frame as u64, (990 + frame * 10) as f32 / 1000.).unwrap())
            } else { None };
            if let Some(ticket) = &ticket { capture.encode_start(&mut encoder, ticket).unwrap(); }
            let slot = arm * FRAMES + frame;
            work.encode(&mut encoder, &readback, slot as u64 * STRIDE);
            if let Some(ticket) = &ticket { capture.encode_end(&mut encoder, ticket).unwrap(); }
            queue.submit(Some(encoder.finish()));
            if let Some(ticket) = ticket {
                capture.submitted(ticket, cpu_context(frame)).unwrap();
                let end_ms = (1000 + frame * 10) as f64;
                capture.presented(frame.checked_sub(1), (frame > 0).then_some(end_ms - 10.), end_ms).unwrap();
                last_ticket = Some(ticket);
            }
        }
    }
    let pending = capture.report();
    assert_eq!(pending["state"], "recording");
    assert_eq!(pending["submitted"], FRAMES);
    assert!(pending["rows"].is_null(), "raw rows must not be exposed before terminal readback");
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    readback.slice(..).map_async(wgpu::MapMode::Read, move |result| { let _ = tx.send(result); });
    // The collector's sole terminal Wait services both pending maps. No second
    // explicit Wait is needed to inspect the independent workload readback.
    capture.finish(&device).unwrap();
    rx.recv().unwrap().unwrap();
    let bytes = readback.slice(..).get_mapped_range();
    let mut distinct_images = std::collections::HashSet::new();
    for frame in 0..FRAMES {
        let off = frame as u64 * STRIDE;
        let on = (FRAMES + frame) as u64 * STRIDE;
        let range = |base: u64, start: u64, size: usize| {
            let at = (base + start) as usize;
            &bytes[at..at + size]
        };
        assert_eq!(range(off, 0, (PLANE*2) as usize), range(on, 0, (PLANE*2) as usize),
            "RGBA/depth differ at frame{frame}");
        let expected = expected_indices(frame);
        assert_eq!(range(off, INDEX_OFFSET, 24), bytemuck::cast_slice::<u32,u8>(&expected));
        assert_eq!(range(on, INDEX_OFFSET, 24), bytemuck::cast_slice::<u32,u8>(&expected));
        let expected = expected_marker(frame);
        assert_eq!(range(off, MARKER_OFFSET, 64), bytemuck::cast_slice::<u32,u8>(&expected));
        assert_eq!(range(on, MARKER_OFFSET, 64), bytemuck::cast_slice::<u32,u8>(&expected));
        let center = u64::from(HEIGHT/2)*u64::from(ROW) + u64::from(WIDTH/2)*4;
        let pixel: [u8;4] = range(on, center, 4).try_into().unwrap();
        assert_ne!(pixel, [1,2,3,255], "changing geometry must actually draw at center");
        distinct_images.insert(pixel);
        let depth = f32::from_le_bytes(range(on, PLANE + center, 4).try_into().unwrap());
        assert!(depth.is_finite() && depth > 0. && depth < 1.);
    }
    assert_eq!(distinct_images.len(), FRAMES, "different frame payloads must survive queued resource reuse");
    drop(bytes); readback.unmap();

    let report = capture.report();
    assert_eq!(report["state"], "complete");
    assert_eq!(report["submitted"], FRAMES);
    assert_eq!(report["valid_gpu_rows"], FRAMES);
    assert_eq!(report["invalid_gpu_rows"], 0);
    assert_eq!(report["unanchored_rows"], 0);
    assert_eq!(report["overflow_skipped"], 0);
    assert_eq!(report["query_count"], 2);
    assert_eq!(report["resolve_bytes"], 256);
    let rows = report["rows"].as_array().unwrap();
    assert_eq!(rows.len(), FRAMES);
    let period = f64::from(queue.get_timestamp_period());
    let mut unique_pairs = std::collections::HashSet::new();
    for (frame, row) in rows.iter().enumerate() {
        assert_eq!(row["row"], frame);
        assert_eq!(row["renderer_frame_id"], 1000 + frame);
        assert_eq!(row["application_seconds"].as_f64().unwrap(),
            f64::from((990 + frame * 10) as f32 / 1000.));
        assert_eq!(row["sample_index"].as_u64(), frame.checked_sub(1).map(|n| n as u64));
        assert_eq!(row["begin_application_ms"].as_f64(), (frame > 0).then_some((990 + frame * 10) as f64));
        assert_eq!(row["end_application_ms"].as_f64(), Some((1000 + frame * 10) as f64));
        let cpu = cpu_context(frame);
        assert_eq!(row["cpu_ms"]["surface_acquire"], cpu.acquire_ms);
        assert_eq!(row["cpu_ms"]["encode"], cpu.encode_ms);
        assert_eq!(row["cpu_ms"]["finish_and_submit"], cpu.submit_ms);
        assert_eq!(row["cpu_ms"]["present"], cpu.present_ms);
        let raw = [row["raw_ticks"][0].as_u64().unwrap(), row["raw_ticks"][1].as_u64().unwrap()];
        let delta = raw[1].checked_sub(raw[0]).expect("GPU pair must not reverse or wrap in this short oracle");
        let expected_ms = delta as f64 * period / 1_000_000.;
        assert_eq!(row["gpu_encoder_ms"].as_f64().unwrap(), expected_ms);
        assert!(row["invalid_reason"].is_null());
        unique_pairs.insert(raw);
    }
    // Quantized zero duration is valid. No elapsed-time floor, GPU/App clock
    // alignment or strict adjacent-pair ordering is assumed: wgpu documents
    // backend-dependent timestamp/command reordering. Across96 submissions the
    // raw counter must nevertheless advance, not repeat only the final pair.
    assert!(unique_pairs.len() > 1, "append-only readback must retain advancing query results");
    let first = rows.first().unwrap()["raw_ticks"][0].as_u64().unwrap();
    let last = rows.last().unwrap()["raw_ticks"][0].as_u64().unwrap();
    assert!(last > first, "short oracle must observe timestamp advancement across the queue");
    capture.finish(&device).unwrap();
    assert_eq!(capture.report(), report, "second finish is an idempotent terminal operation");
    assert!(capture.reserve(9999, 99.).is_none());
    let late = capture.report();
    assert_eq!(late["state"], "complete");
    assert_eq!(late["late_reserves"], 1);
    assert_eq!(late["rows"], report["rows"]);
    let mut encoder = device.create_command_encoder(&Default::default());
    assert!(capture.encode_start(&mut encoder, &last_ticket.unwrap()).is_err(),
        "terminal ticket cannot record into reused GPU query storage");
    // Deliberate invalid client use is rejected without corrupting the already
    // complete immutable report or replaying any GPU work.
    assert!(capture.submitted(last_ticket.unwrap(), CpuTimes::default()).is_err());
    assert!(capture.presented(Some(FRAMES), Some(2000.), 2010.).is_err());
    assert_eq!(capture.report()["state"], "complete");
    assert_eq!(capture.report()["rows"], report["rows"]);
    capture.finish(&device).unwrap();
    println!("timestamp oracle: {FRAMES} captured submissions, exact off/on RGBA/depth/index/marker copies; one terminal wait");
}

// Separate isolated stage oracle. Existing whole-pair oracle above remains unchanged.
fn stage_oracle_pass(work: &Workload, encoder: &mut wgpu::CommandEncoder,
    mut capture: Option<&mut Capture>, eye: Option<u8>) {
    if let (Some(capture),Some(eye))=(capture.as_deref_mut(),eye) {
        capture.mark_encoder(encoder,Label::WorldBegin(eye)).unwrap();
    }
    {
        let mut pass=encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label:Some("stage oracle identical draw"),
            color_attachments:&[Some(wgpu::RenderPassColorAttachment { view:&work.color_view,
                resolve_target:None,ops:wgpu::Operations { load:wgpu::LoadOp::Clear(wgpu::Color {
                    r:1./255.,g:2./255.,b:3./255.,a:1. }),store:wgpu::StoreOp::Store } })],
            depth_stencil_attachment:Some(wgpu::RenderPassDepthStencilAttachment {
                view:&work.depth_view,depth_ops:Some(wgpu::Operations {
                    load:wgpu::LoadOp::Clear(1.),store:wgpu::StoreOp::Store }),stencil_ops:None,
            }),timestamp_writes:None,occlusion_query_set:None,
        });
        if let (Some(capture),Some(eye))=(capture.as_deref_mut(),eye) {
            capture.mark_pass(&mut pass,Label::NearBegin(eye)).unwrap();
        }
        pass.set_pipeline(&work.pipeline);pass.set_bind_group(0,&work.bind,&[]);
        pass.set_index_buffer(work.indices.slice(..),wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..6,0,0..1);
        if let (Some(capture),Some(eye))=(capture.as_deref_mut(),eye) {
            for label in [Label::NearEnd(eye),Label::FarEnd(eye),Label::SkyDynamicEnd(eye),Label::AlphaEnd(eye)] {
                capture.mark_pass(&mut pass,label).unwrap();
            }
        }
    }
    if let (Some(capture),Some(eye))=(capture.as_deref_mut(),eye) {
        capture.mark_encoder(encoder,Label::WorldEnd(eye)).unwrap();
    }
}

fn stage_oracle_encode(work: &Workload, encoder: &mut wgpu::CommandEncoder,
    readback: &wgpu::Buffer, offset: u64, mut capture: Option<&mut Capture>,
    eyes: u8, post: bool, gui: bool) {
    if eyes==0 { stage_oracle_pass(work,encoder,None,None); }
    for eye in 0..eyes { stage_oracle_pass(work,encoder,capture.as_deref_mut(),Some(eye)); }
    for (enabled,begin,end) in [(post,Label::PostBegin,Label::PostEnd),(gui,Label::GuiBegin,Label::GuiEnd)] {
        if enabled {
            if let Some(capture)=capture.as_deref_mut() { capture.mark_encoder(encoder,begin).unwrap(); }
            stage_oracle_pass(work,encoder,None,None);
            if let Some(capture)=capture.as_deref_mut() { capture.mark_encoder(encoder,end).unwrap(); }
        }
    }
    let extent=wgpu::Extent3d {width:WIDTH,height:HEIGHT,depth_or_array_layers:1};
    for (texture,plane,aspect) in [(&work.color,0,wgpu::TextureAspect::All),
        (&work.depth,PLANE,wgpu::TextureAspect::DepthOnly)] {
        encoder.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo {texture,mip_level:0,
            origin:wgpu::Origin3d::ZERO,aspect},wgpu::TexelCopyBufferInfo {buffer:readback,
            layout:wgpu::TexelCopyBufferLayout {offset:offset+plane,bytes_per_row:Some(ROW),rows_per_image:Some(HEIGHT)}},extent);
    }
    encoder.copy_buffer_to_buffer(&work.indices,0,readback,offset+INDEX_OFFSET,24);
    encoder.copy_buffer_to_buffer(&work.marker,0,readback,offset+MARKER_OFFSET,64);
}

#[test]
#[ignore = "actual Vulkan stage timestamp prefix, queued byte equality and optional coverage oracle"]
fn gpu_stage_prefix_rows_preserve_queued_work_and_optional_coverage() {
    let instance=wgpu::Instance::new(&wgpu::InstanceDescriptor {backends:wgpu::Backends::VULKAN,..Default::default()});
    let adapter=pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference:wgpu::PowerPreference::HighPerformance,compatible_surface:None,force_fallback_adapter:false,
    })).expect("Vulkan adapter required");
    let features=required_features_stages(&adapter).expect("stage queries and nonblocking resolve required");
    let (device,queue)=pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label:Some("stage prefix oracle"),required_features:features,required_limits:wgpu::Limits::default(),
        memory_hints:wgpu::MemoryHints::Performance,
    },None)).unwrap();
    let mut capture=Capture::new_stages(&device,&queue).unwrap();
    let work=Workload::new(&device);
    let readback=device.create_buffer(&wgpu::BufferDescriptor {label:Some("stage oracle both arms append-only output"),
        size:2*FRAMES as u64*STRIDE,usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
    // Alternating maximal and minimal shapes intentionally reuses a long prefix
    // then resolves only two slots. All suffix content must remain unreported.
    let shapes=[(2u8,true,true,20usize),(0,false,false,2),(1,false,false,9),
        (0,false,true,4),(1,true,false,11),(2,false,true,18)];
    let stage_pointer=capture.stages.as_ref().unwrap() as *const _;
    for arm in 0..2 { for frame in 0..FRAMES {
        work.write(&queue,frame);
        let mut encoder=device.create_command_encoder(&Default::default());
        let ticket=if arm==1 {Some(capture.reserve(2000+frame as u64,frame as f32/10.).unwrap())}else{None};
        if let Some(ticket)=ticket.as_ref() {capture.encode_start(&mut encoder,ticket).unwrap();}
        let (eyes,post,gui,_)=shapes[frame%shapes.len()];
        stage_oracle_encode(&work,&mut encoder,&readback,(arm*FRAMES+frame) as u64*STRIDE,
            (arm==1).then_some(&mut capture),eyes,post,gui);
        if let Some(ticket)=ticket.as_ref() {capture.encode_end(&mut encoder,ticket).unwrap();}
        queue.submit(Some(encoder.finish()));
        if let Some(ticket)=ticket {
            capture.submitted(ticket,cpu_context(frame)).unwrap();
            let end=1000.+frame as f64*10.;
            capture.presented(frame.checked_sub(1),(frame>0).then_some(end-10.),end).unwrap();
        }
    }}
    assert_eq!(capture.stages.as_ref().unwrap() as *const _,stage_pointer);
    let before=capture.report();assert_eq!(before["state"],"recording");
    assert!(before["rows"].is_null());assert!(before["stages"]["rows"].is_null());
    assert_eq!(before["submitted"],FRAMES);assert_eq!(before["stages"]["submitted"],FRAMES);
    let (tx,rx)=std::sync::mpsc::sync_channel(1);
    readback.slice(..).map_async(wgpu::MapMode::Read,move |result|{let _=tx.send(result);});
    // No earlier map/poll/wait, including between arms. The collector's one
    // terminal Wait services this independent output map as well.
    capture.finish(&device).unwrap();rx.recv().unwrap().unwrap();
    let mapped=readback.slice(..).get_mapped_range();
    let mut centers=std::collections::HashSet::new();
    for frame in 0..FRAMES {
        let off=frame*STRIDE as usize;let on=(FRAMES+frame)*STRIDE as usize;
        assert_eq!(&mapped[off..off+PLANE as usize*2],&mapped[on..on+PLANE as usize*2],"stage off/on color/depth frame{frame}");
        for base in [off,on] {
            assert_eq!(&mapped[base+INDEX_OFFSET as usize..base+INDEX_OFFSET as usize+24],
                bytemuck::cast_slice::<u32,u8>(&expected_indices(frame)));
            assert_eq!(&mapped[base+MARKER_OFFSET as usize..base+MARKER_OFFSET as usize+64],
                bytemuck::cast_slice::<u32,u8>(&expected_marker(frame)));
        }
        let center=on+(HEIGHT/2*ROW+WIDTH/2*4) as usize;
        let pixel:[u8;4]=mapped[center..center+4].try_into().unwrap();assert_ne!(pixel,[1,2,3,255]);centers.insert(pixel);
        let z=f32::from_le_bytes(mapped[center+PLANE as usize..center+PLANE as usize+4].try_into().unwrap());
        assert!(z>0.&&z<1.&&z.is_finite());
    }
    assert_eq!(centers.len(),FRAMES);drop(mapped);readback.unmap();
    let report=capture.report();assert_eq!(report["state"],"complete");
    assert_eq!(report["query_count"],20);assert_eq!(report["readback_bytes"],20*1024*1024);
    assert_eq!(report["resolve_bytes"],256);assert_eq!(report["unanchored_rows"],0);
    assert_eq!(report["overflow_skipped"],0);assert_eq!(report["canceled_reserved"],0);
    let rows=report["rows"].as_array().unwrap();let stages=&report["stages"];
    let stage_rows=stages["rows"].as_array().unwrap();assert_eq!(stage_rows.len(),FRAMES);
    assert_eq!(stages["storage_shared_with_whole_frame"],true);
    let mut valid=0usize;let mut invalid=0usize;let mut zeros=0usize;
    let mut starts=std::collections::HashSet::new();
    for frame in 0..FRAMES {
        let row=&rows[frame];let stage=&stage_rows[frame];let (eyes,post,gui,used)=shapes[frame%shapes.len()];
        assert_eq!(row["renderer_frame_id"],2000+frame);
        assert_eq!(row["sample_index"].as_u64(),frame.checked_sub(1).map(|n|n as u64));
        assert_eq!(row["end_application_ms"],1000.+frame as f64*10.);
        assert_eq!(stage["row"],frame);assert_eq!(stage["eyes"],eyes);assert_eq!(stage["postfx"],post);assert_eq!(stage["gui"],gui);
        assert_eq!(stage["mode"],if eyes==0 {"no_world"}else{"world"});assert_eq!(stage["used_query_count"],used);
        let points=stage["points"].as_array().unwrap();assert_eq!(points.len(),used);
        assert_eq!(points[0]["label"],"whole_begin");assert_eq!(points[used-1]["label"],"whole_end");
        assert_eq!(row["raw_ticks"][0],points[0]["raw_ticks"]);assert_eq!(row["raw_ticks"][1],points[used-1]["raw_ticks"]);
        starts.insert(points[0]["raw_ticks"].as_u64().unwrap());
        let names=["world_begin","near_begin","near_end","far_end","sky_dynamic_end","alpha_end","world_end"];
        for eye in 0..eyes as usize {for (n,name) in names.iter().enumerate(){
            assert_eq!(points[1+7*eye+n]["label"],*name);assert_eq!(points[1+7*eye+n]["eye"],eye);
        }}
        let mut at=1+7*eyes as usize;
        if post {assert_eq!(points[at]["label"],"post_begin");assert_eq!(points[at+1]["label"],"post_end");at+=2;}
        if gui {assert_eq!(points[at]["label"],"gui_begin");assert_eq!(points[at+1]["label"],"gui_end");}
        for (index,point) in points.iter().enumerate(){assert_eq!(point["query_index"],index);}
        let intervals=stage["intervals"].as_array().unwrap();assert_eq!(intervals.len(),used-1);
        for (i,interval) in intervals.iter().enumerate() {
            assert_eq!(interval["from"],i);assert_eq!(interval["to"],i+1);
            let a=points[i]["raw_ticks"].as_u64().unwrap();let b=points[i+1]["raw_ticks"].as_u64().unwrap();
            if let Some(delta)=b.checked_sub(a) {
                let ms=delta as f64*f64::from(queue.get_timestamp_period())/1_000_000.;
                assert_eq!(interval["gpu_ms"],ms);assert!(interval["invalid_reason"].is_null());valid+=1;zeros+=usize::from(ms==0.);
            }else{assert!(interval["gpu_ms"].is_null());assert_eq!(interval["invalid_reason"],"reversed_or_unknown_wrap");invalid+=1;}
        }
    }
    assert!(starts.len()>1,"short queued oracle must preserve advancing frame values");
    assert_eq!(stages["valid_intervals"],valid);assert_eq!(stages["invalid_intervals"],invalid);
    assert_eq!(stages["quantized_zero_intervals"],zeros);
    // No strict within-frame ordering, elapsed floor, sum==CPU, or adjacent-frame
    // ordering assertion: driver/backend reordering and quantization are exposed.
    capture.finish(&device).unwrap();assert_eq!(capture.report(),report);
    let mut encoder=device.create_command_encoder(&Default::default());
    capture.mark_encoder(&mut encoder,Label::GuiBegin).unwrap();
    assert_eq!(capture.report(),report,"unmeasured screenshot/raw caller cannot append a completed capture");
    println!("stage timestamp oracle: {FRAMES} queued stage rows, alternating2..20 slots, exact output; one terminal wait");
}
