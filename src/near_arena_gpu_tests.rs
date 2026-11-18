//! Report-only isolated address/order/lifecycle oracle. NOT Renderer integration.
//! No fixture shader or direct reference calls the arena's offset conversion.
use super::*;
use super::super::near_arena_plan as plan;
use crate::mesh::Vertex;
use std::ops::Range;
const WIDTH:u32=320;
const HEIGHT:u32=144;
const ROW:u32=1280;
const PLANE:u64=ROW as u64*HEIGHT as u64;
const LIMITS:Limits=Limits{vertex_page:4*1024*1024,index_page:4096,owners:16};
const SHADER:&str=r#"
struct Eye { shift:vec4<f32> }; @group(0) @binding(0) var<uniform> eye:Eye;
@group(0) @binding(1) var tex:texture_2d<f32>; @group(0) @binding(2) var samp:sampler;
struct Out {@builtin(position) p:vec4<f32>,@location(0) uv:vec2<f32>,@location(1) c:vec4<f32>};
@vertex fn vs(@location(0) p:vec3<f32>,@location(1) uv:vec2<f32>,@location(3) c:vec4<f32>)->Out {
 var o:Out;o.p=vec4<f32>(p.x+eye.shift.x,p.y,p.z,1.);o.uv=uv;o.c=c;return o;
}
@fragment fn fs(i:Out)->@location(0) vec4<f32> {return i.c*textureSample(tex,samp,i.uv);}
"#;
struct Gpu {device:wgpu::Device,queue:wgpu::Queue,opaque:wgpu::RenderPipeline,
    blend:wgpu::RenderPipeline,eyes:[wgpu::BindGroup;2],limits:plan::DrawLimits}
impl Gpu {
    async fn new()->Self {
        assert_eq!(std::mem::size_of::<Vertex>(),32);
        let instance=wgpu::Instance::new(&wgpu::InstanceDescriptor{backends:wgpu::Backends::VULKAN,..Default::default()});
        let adapter=instance.request_adapter(&wgpu::RequestAdapterOptions{power_preference:wgpu::PowerPreference::HighPerformance,
            compatible_surface:None,force_fallback_adapter:false}).await.expect("Vulkan adapter required");
        assert_eq!(adapter.get_info().backend,wgpu::Backend::Vulkan);
        let flags=adapter.get_downlevel_capabilities().flags;
        assert!(adapter.features().contains(wgpu::Features::MULTI_DRAW_INDIRECT),"native MDI unavailable: no emulation pass");
        assert!(flags.contains(wgpu::DownlevelFlags::INDIRECT_EXECUTION|wgpu::DownlevelFlags::BASE_VERTEX));
        // Read the SAME selected adapter. No native handles escape the callback.
        let native_max=unsafe {adapter.as_hal::<wgpu::hal::api::Vulkan,_,_>(|a|a.map(|a|
            a.shared_instance().raw_instance().get_physical_device_properties(a.raw_physical_device()).limits.max_draw_indirect_count))};
        let (device,queue)=adapter.request_device(&wgpu::DeviceDescriptor{label:Some("isolated arena oracle"),
            required_features:wgpu::Features::MULTI_DRAW_INDIRECT,required_limits:wgpu::Limits::default(),
            memory_hints:wgpu::MemoryHints::Performance},None).await.unwrap();
        let limits=plan::DrawLimits::negotiated(device.features().contains(wgpu::Features::MULTI_DRAW_INDIRECT),
            true,true,native_max.map(|n|n.min(3)));
        assert!(limits.multi_draw_enabled(),"native count must admit real multi-draw");
        let layout=device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor{label:None,entries:&[
            wgpu::BindGroupLayoutEntry{binding:0,visibility:wgpu::ShaderStages::VERTEX,ty:wgpu::BindingType::Buffer{
                ty:wgpu::BufferBindingType::Uniform,has_dynamic_offset:false,min_binding_size:None},count:None},
            wgpu::BindGroupLayoutEntry{binding:1,visibility:wgpu::ShaderStages::FRAGMENT,ty:wgpu::BindingType::Texture{
                sample_type:wgpu::TextureSampleType::Float{filterable:true},view_dimension:wgpu::TextureViewDimension::D2,multisampled:false},count:None},
            wgpu::BindGroupLayoutEntry{binding:2,visibility:wgpu::ShaderStages::FRAGMENT,ty:wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),count:None}]});
        let texture=device.create_texture(&wgpu::TextureDescriptor{label:None,size:wgpu::Extent3d{width:2,height:2,depth_or_array_layers:1},
            mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Rgba8Unorm,
            usage:wgpu::TextureUsages::COPY_DST|wgpu::TextureUsages::TEXTURE_BINDING,view_formats:&[]});
        queue.write_texture(texture.as_image_copy(),&[255,255,255,255,170,230,255,255,255,190,220,255,210,255,180,255],
            wgpu::TexelCopyBufferLayout{offset:0,bytes_per_row:Some(8),rows_per_image:Some(2)},texture.size());
        let view=texture.create_view(&Default::default());let sampler=device.create_sampler(&wgpu::SamplerDescriptor{
            address_mode_u:wgpu::AddressMode::Repeat,address_mode_v:wgpu::AddressMode::Repeat,..Default::default()});
        let eyes=std::array::from_fn(|eye|{
            let uniform=device.create_buffer(&wgpu::BufferDescriptor{label:None,size:16,usage:wgpu::BufferUsages::UNIFORM|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
            queue.write_buffer(&uniform,0,bytemuck::cast_slice(&[if eye==0{-0.08f32}else{0.08},0.,0.,0.]));
            device.create_bind_group(&wgpu::BindGroupDescriptor{label:None,layout:&layout,entries:&[
                wgpu::BindGroupEntry{binding:0,resource:uniform.as_entire_binding()},
                wgpu::BindGroupEntry{binding:1,resource:wgpu::BindingResource::TextureView(&view)},
                wgpu::BindGroupEntry{binding:2,resource:wgpu::BindingResource::Sampler(&sampler)}]})});
        let shader=device.create_shader_module(wgpu::ShaderModuleDescriptor{label:None,source:wgpu::ShaderSource::Wgsl(SHADER.into())});
        let pipeline_layout=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor{label:None,bind_group_layouts:&[&layout],push_constant_ranges:&[]});
        let make=|blend|device.create_render_pipeline(&wgpu::RenderPipelineDescriptor{label:None,layout:Some(&pipeline_layout),
            vertex:wgpu::VertexState{module:&shader,entry_point:Some("vs"),compilation_options:Default::default(),buffers:&[wgpu::VertexBufferLayout{
                array_stride:32,step_mode:wgpu::VertexStepMode::Vertex,attributes:&wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Uint32,3=>Unorm8x4,4=>Unorm8x4]}]},
            fragment:Some(wgpu::FragmentState{module:&shader,entry_point:Some("fs"),compilation_options:Default::default(),targets:&[Some(wgpu::ColorTargetState{
                format:wgpu::TextureFormat::Rgba8Unorm,blend:if blend{Some(wgpu::BlendState::ALPHA_BLENDING)}else{None},write_mask:wgpu::ColorWrites::ALL})]}),
            primitive:wgpu::PrimitiveState{cull_mode:None,..Default::default()},depth_stencil:Some(wgpu::DepthStencilState{
                format:wgpu::TextureFormat::Depth32Float,depth_write_enabled:!blend,depth_compare:wgpu::CompareFunction::LessEqual,
                stencil:Default::default(),bias:Default::default()}),multisample:Default::default(),multiview:None,cache:None});
        let opaque=make(false);let blend=make(true);Self{device,queue,opaque,blend,eyes,limits}
    }
}
struct Factory<'a>{device:&'a wgpu::Device}
impl PageFactory for Factory<'_>{
    type Handle=wgpu::Buffer;type Error=String;
    fn create(&mut self,kind:PageKind,bytes:u64)->Result<wgpu::Buffer,String>{
        self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let buffer=self.device.create_buffer(&wgpu::BufferDescriptor{label:Some("oracle arena page"),size:bytes,mapped_at_creation:false,
            usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::COPY_SRC|match kind{PageKind::Vertex=>wgpu::BufferUsages::VERTEX,PageKind::Index(_)=>wgpu::BufferUsages::INDEX}});
        let validation=pollster::block_on(self.device.pop_error_scope());let oom=pollster::block_on(self.device.pop_error_scope());
        if let Some(error)=validation.or(oom){Err(error.to_string())}else{Ok(buffer)}
    }
}
#[derive(Clone)]
struct Payload{vertices:Vec<Vertex>,indices:Vec<u32>,width:IndexWidth,solid:Vec<Range<u32>>,transparent:Range<u32>}
impl Payload{
    fn bytes(&self)->Vec<u8>{match self.width{
        IndexWidth::U16=>{let mut v:Vec<u16>=self.indices.iter().map(|&i|u16::try_from(i).unwrap()).collect();if v.len()%2!=0{v.push(0);}bytemuck::cast_slice(&v).to_vec()},
        IndexWidth::U32=>bytemuck::cast_slice(&self.indices).to_vec()}}
    fn request(&self)->Request{Request{vertex_bytes:self.vertices.len() as u64*32,index_bytes:self.bytes().len() as u64,
        vertex_reserved:(self.vertices.len() as u64*32).max(32).next_multiple_of(256),index_reserved:(self.bytes().len() as u64).max(4).next_multiple_of(256),
        vertices:self.vertices.len() as u32,indices:self.indices.len() as u32,width:self.width,max_local_index:self.indices.iter().copied().max()}}
}
fn payload(id:usize,wide:bool,variant:u8)->Payload{
    let mut vertices=Vec::new();let mut indices=Vec::new();
    for q in 0..5 {
        let x=-0.84+(id%3) as f32*0.32;let y=-0.72+(id/3) as f32*0.30;
        let z=if q<3{0.65}else{0.25+(q-3) as f32*0.14};
        let color=if q==1{[255,0,255,255]}else{[35+id as u8*23,55+q as u8*31,90+variant*35,if q>=3{100+q as u8*15}else{255}]};
        let base=vertices.len() as u32;
        for (position,uv) in [([x,y,z],[0.,0.]),([x+0.72,y,z],[2.,0.]),([x+0.72,y+0.65,z],[2.,2.]),([x,y+0.65,z],[0.,2.])]{
            vertices.push(Vertex{position,uv,layer:0,color,light:[0;4]});
        }
        indices.extend([0,1,2,0,2,3].map(|i|base+i));
    }
    indices.extend([0,0,0]); // odd U16 index count; final degenerate triangle is invisible.
    if wide{vertices.splice(0..0,vec![vertices[0];65536]);for i in &mut indices{*i+=65536;}}
    Payload{vertices,indices,width:if wide{IndexWidth::U32}else{IndexWidth::U16},solid:vec![0..6,12..18],transparent:18..33}
}
struct Dedicated{vertex:wgpu::Buffer,index:wgpu::Buffer}
impl Dedicated{
    fn new(g:&Gpu,p:&Payload)->Self{
        let make=|size,usage|g.device.create_buffer(&wgpu::BufferDescriptor{label:Some("independent dedicated reference"),size,mapped_at_creation:false,
            usage:usage|wgpu::BufferUsages::COPY_SRC|wgpu::BufferUsages::COPY_DST});
        let vertex=make((p.vertices.len()*32) as u64,wgpu::BufferUsages::VERTEX);let bytes=p.bytes();
        let index=make(bytes.len() as u64,wgpu::BufferUsages::INDEX);
        g.queue.write_buffer(&vertex,0,bytemuck::cast_slice(&p.vertices));g.queue.write_buffer(&index,0,&bytes);Self{vertex,index}
    }
}
struct Item{payload:Payload,dedicated:Dedicated,token:Option<Token>}
struct Scene(Vec<Item>);
impl plan::Source for Scene{
    fn len(&self)->usize{self.0.len()}
    fn row(&self,i:usize)->plan::Row<'_>{let row=&self.0[i];plan::Row{backing:row.token.map_or(plan::Backing::Dedicated,plan::Backing::Arena),width:row.payload.width,ranges:&row.payload.solid}}
}
fn put(g:&Gpu,arena:&mut Arena<wgpu::Buffer>,old:Option<Token>,p:&Payload)->Token{
    let mut factory=Factory{device:&g.device};let mut pending=arena.begin_update(old,p.request(),&mut factory).unwrap();
    {let target=pending.target();g.queue.write_buffer(target.vertex,target.vertex_range.start,bytemuck::cast_slice(&p.vertices));
        g.queue.write_buffer(target.index,target.index_range.start,&p.bytes());}
    pending.commit()
}
fn change(g:&Gpu,arena:&mut Arena<wgpu::Buffer>,scene:&mut Scene,row:usize,p:Payload){
    let old=&scene.0[row];let token=old.token.map(|token|put(g,arena,Some(token),&p));
    scene.0[row]=Item{dedicated:Dedicated::new(g,&p),payload:p,token};
}
fn reorder_transparent(g:&Gpu,arena:&mut Arena<wgpu::Buffer>,scene:&mut Scene,row:usize){
    let item=&mut scene.0[row];let tail=item.payload.indices[18..30].to_vec();
    item.payload.indices[18..24].copy_from_slice(&tail[6..12]);item.payload.indices[24..30].copy_from_slice(&tail[..6]);
    let bytes=item.payload.bytes();let offset=18*item.payload.width.bytes();assert_eq!(offset%4,0);
    // The existing opaque prefix is deliberately NOT rewritten. Exact full
    // readback detects a missing allocation base or wrong index-width stride.
    g.queue.write_buffer(&item.dedicated.index,offset,&bytes[offset as usize..]);
    if let Some(token)=item.token{let b=arena.binding(token).unwrap();
        g.queue.write_buffer(b.index,b.index_range.start+offset,&bytes[offset as usize..]);}
}
fn format(width:IndexWidth)->wgpu::IndexFormat{match width{IndexWidth::U16=>wgpu::IndexFormat::Uint16,IndexWidth::U32=>wgpu::IndexFormat::Uint32}}
struct Targets{color:wgpu::Texture,depth:wgpu::Texture,readback:wgpu::Buffer,lengths:Vec<usize>}
impl Targets{
    fn new(g:&Gpu,scene:&Scene)->Self{
        let texture=|format|g.device.create_texture(&wgpu::TextureDescriptor{label:None,size:wgpu::Extent3d{width:WIDTH,height:HEIGHT,depth_or_array_layers:1},
            mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format,
            usage:wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::COPY_SRC,view_formats:&[]});
        let lengths:Vec<_>=scene.0.iter().map(|i|i.payload.bytes().len()).collect();
        let readback=g.device.create_buffer(&wgpu::BufferDescriptor{label:None,size:2*PLANE+lengths.iter().sum::<usize>()as u64,
            usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
        Self{color:texture(wgpu::TextureFormat::Rgba8Unorm),depth:texture(wgpu::TextureFormat::Depth32Float),readback,lengths}
    }
    fn copy(&self,encoder:&mut wgpu::CommandEncoder,recording:&Recording<'_,wgpu::Buffer>,scene:&Scene,mode:Mode){
        for (texture,offset,aspect) in [(&self.color,0,wgpu::TextureAspect::All),(&self.depth,PLANE,wgpu::TextureAspect::DepthOnly)]{
            encoder.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo{texture,mip_level:0,origin:wgpu::Origin3d::ZERO,aspect},
                wgpu::TexelCopyBufferInfo{buffer:&self.readback,layout:wgpu::TexelCopyBufferLayout{offset,bytes_per_row:Some(ROW),rows_per_image:Some(HEIGHT)}},
                wgpu::Extent3d{width:WIDTH,height:HEIGHT,depth_or_array_layers:1});
        }
        let mut offset=2*PLANE;
        for (item,&len) in scene.0.iter().zip(&self.lengths){
            if let Some(token)=item.token.filter(|_|mode!=Mode::Dedicated){let b=recording.binding(token).unwrap();
                encoder.copy_buffer_to_buffer(b.index,b.index_range.start,&self.readback,offset,len as u64);
            }else{encoder.copy_buffer_to_buffer(&item.dedicated.index,0,&self.readback,offset,len as u64);}
            offset+=len as u64;
        }
    }
    fn finish(self,g:&Gpu)->Image{
        let(tx,rx)=std::sync::mpsc::channel();self.readback.slice(..).map_async(wgpu::MapMode::Read,move|r|{let _=tx.send(r);});
        g.device.poll(wgpu::Maintain::Wait);rx.recv().unwrap().unwrap();let mapped=self.readback.slice(..).get_mapped_range();
        let color=mapped[..PLANE as usize].chunks_exact(4).map(|v|<[u8;4]>::try_from(v).unwrap()).collect();
        let depth=mapped[PLANE as usize..2*PLANE as usize].chunks_exact(4).map(|v|u32::from_le_bytes(v.try_into().unwrap())).collect();
        let mut offset=2*PLANE as usize;let indices=self.lengths.iter().map(|&len|{let bytes=mapped[offset..offset+len].to_vec();offset+=len;bytes}).collect();
        drop(mapped);self.readback.unmap();Image{color,depth,indices}
    }
}
#[derive(Debug,PartialEq,Eq)]
struct Image{color:Vec<[u8;4]>,depth:Vec<u32>,indices:Vec<Vec<u8>>}
fn direct<'a>(pass:&mut wgpu::RenderPass<'a>,recording:&Recording<'_,wgpu::Buffer>,scene:&'a Scene,row:usize,range:Range<u32>,mode:Mode){
    let item=&scene.0[row];
    if let Some(token)=item.token.filter(|_|mode!=Mode::Dedicated){let b=recording.binding(token).unwrap();
        pass.set_vertex_buffer(0,b.vertex.slice(b.vertex_range));pass.set_index_buffer(b.index.slice(b.index_range),format(item.payload.width));
    }else{pass.set_vertex_buffer(0,item.dedicated.vertex.slice(..));pass.set_index_buffer(item.dedicated.index.slice(..),format(item.payload.width));}
    pass.draw_indexed(range,0,0..1);
}
fn encode(g:&Gpu,recording:&Recording<'_,wgpu::Buffer>,scene:&Scene,mode:Mode,targets:&Targets)->(wgpu::CommandBuffer,usize){
    let mut planner=plan::Plan::new().unwrap();let prepared=planner.build(mode,g.limits,recording,scene).unwrap();
    let arguments=g.device.create_buffer(&wgpu::BufferDescriptor{label:None,size:prepared.argument_bytes().len().max(20)as u64,
        usage:wgpu::BufferUsages::INDIRECT|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
    if prepared.summary().multi_calls>0{g.queue.write_buffer(&arguments,0,prepared.argument_bytes());}
    let mut encoder=g.device.create_command_encoder(&Default::default());let color=targets.color.create_view(&Default::default());let depth=targets.depth.create_view(&Default::default());
    for eye in 0..2 {
        let mut pass=encoder.begin_render_pass(&wgpu::RenderPassDescriptor{label:Some("independent arena oracle eye"),color_attachments:&[Some(wgpu::RenderPassColorAttachment{
            view:&color,resolve_target:None,ops:wgpu::Operations{load:if eye==0{wgpu::LoadOp::Clear(wgpu::Color::BLACK)}else{wgpu::LoadOp::Load},store:wgpu::StoreOp::Store}})],
            depth_stencil_attachment:Some(wgpu::RenderPassDepthStencilAttachment{view:&depth,depth_ops:Some(wgpu::Operations{
                load:if eye==0{wgpu::LoadOp::Clear(1.)}else{wgpu::LoadOp::Load},store:wgpu::StoreOp::Store}),stencil_ops:None}),timestamp_writes:None,occlusion_query_set:None});
        pass.set_viewport((eye as u32*WIDTH/2)as f32,0.,(WIDTH/2)as f32,HEIGHT as f32,0.,1.);
        pass.set_scissor_rect(eye as u32*WIDTH/2,0,WIDTH/2,HEIGHT);pass.set_pipeline(&g.opaque);pass.set_bind_group(0,&g.eyes[eye],&[]);
        if mode==Mode::ArenaMdi {
            for command in prepared.commands(){match command{
                plan::Command::Direct{row,range}=>direct(&mut pass,recording,scene,*row,range.clone(),mode),
                plan::Command::Multi{row,binding,first_arg,count}=>{let b=prepared.binding(*row).unwrap().unwrap();
                    assert_eq!((binding.arena_id,binding.vertex_page,binding.index_page),(b.arena_id,b.vertex_page,b.index_page));
                    pass.set_vertex_buffer(0,b.vertex.slice(..));pass.set_index_buffer(b.index.slice(..),format(binding.width));
                    pass.multi_draw_indexed_indirect(&arguments,u64::from(*first_arg)*20,*count);
                }
            }}
        }else{ // Independent original encounter-order reference, no Plan commands.
            for (row,item) in scene.0.iter().enumerate(){for range in &item.payload.solid{direct(&mut pass,recording,scene,row,range.clone(),mode);}}
        }
        pass.set_pipeline(&g.blend);pass.set_bind_group(0,&g.eyes[eye],&[]);
        for row in (0..scene.0.len()).rev(){direct(&mut pass,recording,scene,row,scene.0[row].payload.transparent.clone(),mode);}
    }
    targets.copy(&mut encoder,recording,scene,mode);(encoder.finish(),prepared.summary().multi_calls)
}
fn submit(g:&Gpu,arena:&mut Arena<wgpu::Buffer>,scene:&Scene,mode:Mode)->Targets{
    let targets=Targets::new(g,scene);let recording=arena.begin_recording().unwrap();let(command,multi)=encode(g,&recording,scene,mode,&targets);
    if mode==Mode::ArenaMdi{assert!(multi>=2,"must execute actual native multi-draw and count splits");}
    g.queue.submit(Some(command));recording.submitted();targets
}
fn phase(g:&Gpu,arena:&mut Arena<wgpu::Buffer>,scene:&Scene)->(Vec<Targets>,Vec<Vec<u8>>){
    (vec![submit(g,arena,scene,Mode::Dedicated),submit(g,arena,scene,Mode::ArenaDirect),submit(g,arena,scene,Mode::ArenaMdi)],scene.0.iter().map(|i|i.payload.bytes()).collect())
}

#[test]
#[ignore="requires native Vulkan MDI; isolated allocator/plan correctness, not Renderer integration or a performance probe"]
fn native_arena_mdi_preserves_pixels_indices_and_queued_lifecycle(){
    let g=pollster::block_on(Gpu::new());let mut arena=Arena::new(Mode::ArenaMdi,LIMITS).unwrap();
    // Two small guards reserve both widths and force nonzero offsets. They are
    // intentionally not drawn and stay owned until the fixture ends.
    let mut guards=Vec::new();
    for width in [IndexWidth::U16,IndexWidth::U32]{let mut factory=Factory{device:&g.device};
        guards.push(arena.begin_update(None,Request{vertex_bytes:32,index_bytes:4,vertex_reserved:32,index_reserved:4,
            vertices:1,indices:if width==IndexWidth::U16{2}else{1},width,max_local_index:Some(0)},&mut factory).unwrap().commit());}
    let mut scene=Scene(Vec::new());
    for id in 0..7{let p=payload(id,matches!(id,3|4),0);let token=if id==5{
            let mut oversized=p.request();oversized.vertex_reserved=LIMITS.vertex_page+32;let mut factory=Factory{device:&g.device};
            assert!(arena.begin_update(None,oversized,&mut factory).is_err());
            let before=arena.stats();let mut exhausted=p.request();exhausted.vertex_reserved=LIMITS.vertex_page;
            assert!(matches!(arena.begin_update(None,exhausted,&mut factory),Err(Failure::Admission(Reject::PageLimit))));
            assert_eq!(arena.stats(),before,"exhausted pages preserve all admitted owners");None
        }else{Some(put(&g,&mut arena,None,&p))};scene.0.push(Item{dedicated:Dedicated::new(&g,&p),payload:p,token});}
    let b=arena.binding(scene.0[0].token.unwrap()).unwrap();assert!(b.vertex_range.start>0&&b.index_range.start>0);
    let mut pending=vec![phase(&g,&mut arena,&scene)];
    // Already-submitted draws and copies are not waited before reuse/new writes.
    change(&g,&mut arena,&mut scene,0,payload(0,false,2));
    reorder_transparent(&g,&mut arena,&mut scene,1);reorder_transparent(&g,&mut arena,&mut scene,4);
    pending.push(phase(&g,&mut arena,&scene));
    let before_token=scene.0[0].token.unwrap();let before=arena.binding(before_token).unwrap().vertex_range;
    let mut grown=scene.0[0].payload.clone();grown.vertices.resize(160,grown.vertices[0]);
    change(&g,&mut arena,&mut scene,0,grown);
    let after=arena.binding(scene.0[0].token.unwrap()).unwrap().vertex_range;
    assert!(after.end-after.start>before.end-before.start);assert_ne!(after.start,before.start);
    assert_eq!(arena.binding(before_token).err(),Some(Reject::StaleToken));
    pending.push(phase(&g,&mut arena,&scene));
    // Multiple complete uploads with no intervening render/submit model pending
    // writes left by surface-acquire failures; only the final payload is drawn.
    change(&g,&mut arena,&mut scene,2,payload(2,false,1));change(&g,&mut arena,&mut scene,2,payload(2,false,3));
    let stale=scene.0[6].token.unwrap();let old=arena.binding(stale).unwrap().vertex_range;
    arena.release(stale).unwrap();let replacement=payload(6,false,1);let token=put(&g,&mut arena,None,&replacement);
    assert_ne!(stale.identity(),token.identity());
    assert_eq!(arena.binding(token).unwrap().vertex_range.start,before.start,"cross-owner reuse of the earlier grown allocation");
    assert_ne!(arena.binding(token).unwrap().vertex_range,old);
    assert_eq!(arena.binding(stale).err(),Some(Reject::StaleToken));
    scene.0[6]=Item{dedicated:Dedicated::new(&g,&replacement),payload:replacement,token:Some(token)};
    let recording=arena.begin_recording().unwrap();let mut planner=plan::Plan::new().unwrap();
    struct Stale{token:Token,ranges:[Range<u32>;1]}
    impl plan::Source for Stale{fn len(&self)->usize{1}fn row(&self,_:usize)->plan::Row<'_>{plan::Row{backing:plan::Backing::Arena(self.token),width:IndexWidth::U16,ranges:&self.ranges}}}
    assert!(matches!(planner.build(Mode::ArenaMdi,g.limits,&recording,&Stale{token:stale,ranges:[0..6]}),Err(plan::Error::StaleArena{row:0})));recording.discarded();
    pending.push(phase(&g,&mut arena,&scene));
    // Width changes force separate index pools; replacing a real >65535-index
    // payload with narrow data and back also exercises new allocation generations.
    change(&g,&mut arena,&mut scene,3,payload(3,false,1));pending.push(phase(&g,&mut arena,&scene));
    change(&g,&mut arena,&mut scene,3,payload(3,true,2));pending.push(phase(&g,&mut arena,&scene));
    let stats=arena.stats();assert!(stats.vertex_pages<=2&&stats.index_pages<=2&&stats.owners<=LIMITS.owners);
    assert_eq!(stats.committed_bytes,stats.reserved_bytes+stats.free_bytes);
    let mut originals=Vec::new();
    for (tickets,expected_indices) in pending{let mut images=tickets.into_iter().map(|t|t.finish(&g));let original=images.next().unwrap();
        assert_eq!(original.indices,expected_indices);for candidate in images{assert_eq!(candidate,original);}
        assert!(original.depth.iter().any(|&z|z!=1f32.to_bits()));originals.push(original);}
    assert_ne!(originals[0].color,originals[1].color,"same-capacity/transparent update must be visible");
    assert_eq!(originals[1],originals[2],"unreferenced vertex growth must preserve exact output and indices");
    assert_ne!(originals[1].color,originals[3].color,"queued replacement/reinsert must be visible");
    for image in &originals{assert!(image.color.chunks_exact(WIDTH as usize).any(|row|row[..WIDTH as usize/2]!=row[WIDTH as usize/2..]),"eye bindings must differ");}
    for guard in guards{arena.release(guard).unwrap();}
}

#[test]
#[ignore="requires Vulkan; deliberate stale-unsubmitted-encoder negative control must visibly fail"]
fn delayed_encoder_is_not_made_safe_by_queue_order(){
    let g=pollster::block_on(Gpu::new());let mut arena=Arena::new(Mode::ArenaDirect,LIMITS).unwrap();
    let p=payload(0,false,0);let token=put(&g,&mut arena,None,&p);
    let mut scene=Scene(vec![Item{dedicated:Dedicated::new(&g,&p),payload:p,token:Some(token)}]);
    let expected=submit(&g,&mut arena,&scene,Mode::Dedicated).finish(&g);
    let targets=Targets::new(&g,&scene);let recording=arena.begin_recording().unwrap();
    let (old_command,_)=encode(&g,&recording,&scene,Mode::ArenaDirect,&targets);
    // INTENTIONAL CONTRACT VIOLATION: the encoder was NOT discarded. This is
    // a negative control, never a production recipe or an accepted reuse path.
    recording.discarded();change(&g,&mut arena,&mut scene,0,payload(0,false,3));
    g.queue.submit(Some(old_command));let corrupted=targets.finish(&g);
    assert_ne!(corrupted.color,expected.color,"stale draw must observe the replacement bytes");
    let safe=submit(&g,&mut arena,&scene,Mode::ArenaDirect).finish(&g);assert_eq!(corrupted,safe);
    let recording=arena.begin_recording().unwrap();let targets=Targets::new(&g,&scene);
    let (held,_)=encode(&g,&recording,&scene,Mode::ArenaDirect,&targets);drop(recording);
    assert_eq!(arena.binding(scene.0[0].token.unwrap()).err(),Some(Reject::Poisoned));
    let mut factory=Factory{device:&g.device};assert!(matches!(arena.begin_update(scene.0[0].token,scene.0[0].payload.request(),&mut factory),Err(Failure::Admission(Reject::Poisoned))));
    drop(held);
}
