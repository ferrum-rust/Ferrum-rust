//! Actual upload diagnostic path versus unchanged normal upload, same renderer.
use super::*;
use super::super::*;
use crate::{gameplay::Mode,ui::Screen};
#[derive(Debug,PartialEq,Eq)]
struct Pixels {color:Vec<[u8;4]>,depth:Vec<u32>}
fn read(renderer:&mut Renderer,input:&RenderInput<'_>)->Pixels {
    let [width,height]=renderer.resolution();
    let extent=wgpu::Extent3d{width,height,depth_or_array_layers:1};
    let texture=|format,label|renderer.device.create_texture(&wgpu::TextureDescriptor {
        label:Some(label),size:extent,mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,
        format,usage:wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::COPY_SRC,view_formats:&[]});
    let depth=texture(DEPTH,"opaque-run oracle depth");
    let color=texture(renderer.config.format,"opaque-run oracle color");
    renderer.depth=depth.create_view(&Default::default());
    let row=(width*4).div_ceil(256)*256;let plane=u64::from(row)*u64::from(height);
    let buffer=renderer.device.create_buffer(&wgpu::BufferDescriptor {
        label:Some("opaque-run exact color/depth readback"),size:plane*2,
        usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
    let mut encoder=renderer.device.create_command_encoder(&Default::default());
    let mut profile=render_profile::Frame{enabled:true,..Default::default()};
    renderer.encode(&mut encoder,&color.create_view(&Default::default()),input,&mut profile);
    for (texture,offset,aspect) in [(&color,0,wgpu::TextureAspect::All),(&depth,plane,wgpu::TextureAspect::DepthOnly)] {
        encoder.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo{texture,mip_level:0,origin:wgpu::Origin3d::ZERO,aspect},
            wgpu::TexelCopyBufferInfo{buffer:&buffer,layout:wgpu::TexelCopyBufferLayout{offset,bytes_per_row:Some(row),rows_per_image:Some(height)}},extent);
    }
    renderer.queue.submit(Some(encoder.finish()));
    let(tx,rx)=std::sync::mpsc::channel();buffer.slice(..).map_async(wgpu::MapMode::Read,move|r|{let _=tx.send(r);});
    renderer.device.poll(wgpu::Maintain::Wait);rx.recv().unwrap().unwrap();
    let mapped=buffer.slice(..).get_mapped_range();let mut result=Pixels{color:Vec::new(),depth:Vec::new()};
    for y in 0..height as usize {
        let start=y*row as usize;
        result.color.extend(mapped[start..start+width as usize*4].chunks_exact(4).map(|v|<[u8;4]>::try_from(v).unwrap()));
        let start=plane as usize+start;
        result.depth.extend(mapped[start..start+width as usize*4].chunks_exact(4).map(|v|u32::from_le_bytes(v.try_into().unwrap())));
    }
    drop(mapped);buffer.unmap();result
}

fn panel(kind:usize)->Mesh {
    let mut mesh=models::empty();
    if kind==0{return mesh;}
    let cells=if kind==1 {1}else{48};
    for i in 0..cells {
        let (x,y,w,h)=if cells==1 {(-6.,61.,12.,8.)}else{
            (-7.+(i%8) as f32*1.8,59.+(i/8) as f32*1.8,1.65,1.65)};
        let layer=models::SOLID|models::UNLIT;
        models::face(&mut mesh,[Vec3::new(x,y,8.),Vec3::new(x+w,y,8.),Vec3::new(x+w,y+h,8.),Vec3::new(x,y+h,8.)],
            [0.,0.,1.,1.],layer,[40+(i%8) as u8*24,55+(i/8) as u8*28,if kind==3 {210}else{80},255]);
    }
    if cells>1 {
        models::face(&mut mesh,[Vec3::new(-4.,62.,9.),Vec3::new(4.,62.,9.),Vec3::new(4.,67.,9.),Vec3::new(-4.,67.,9.)],
            [0.,0.,1.,1.],models::SOLID|models::UNLIT|models::BLEND,[40,180,240,100]);
    }
    if kind==4 {
        mesh.vertices.splice(0..0,vec![mesh.vertices[0];65536]);
        for index in &mut mesh.indices {*index+=65536;}
    }
    mesh.min=Vec3::new(-7.,59.,8.);mesh.max=Vec3::new(7.5,70.,9.);mesh
}
fn metadata(chunk:&ChunkMesh)->serde_json::Value {
    let keys=chunk.order.by_distance.rows();
    let quads:Vec<_>=chunk.order.quads.iter().map(|q|(q.center.to_array().map(f32::to_bits),q.indices,q.section)).collect();
    serde_json::json!({"capacity":[chunk.gpu.vertex_capacity,chunk.gpu.index_capacity],"count":chunk.gpu.count,
        "width":if chunk.gpu.index_format==wgpu::IndexFormat::Uint16{2}else{4},
        "bounds":[chunk.gpu.min.to_array().map(f32::to_bits),chunk.gpu.max.to_array().map(f32::to_bits)],
        "solid":chunk.solid_count,"ends":chunk.ends,"present":chunk.present,"mask":chunk.mask,
        "ranges":chunk.solid_ranges,"chests":chunk.chests.iter().map(|p|p.to_array()).collect::<Vec<_>>(),
        "graph":chunk.graph.is_some(),"quads":quads,"keys":keys,"indices":chunk.order.indices,
        "narrow":chunk.order.narrow_indices,"eye":chunk.order.eye.map(|p|p.to_array().map(f32::to_bits))})
}
#[test]
#[ignore="requires original resources and Vulkan; upload capture must preserve exact real pixels/depth/cache behavior"]
fn captured_upload_matches_normal_gpu_output_and_payload_lifecycle() {
    let assets=crate::validation::assets();let resources=crate::validation::resources();
    let settings=Settings{render_distance:16,clouds:0,entity_shadows:false,..Default::default()};
    let world=World{chunks:Default::default(),spawn:Vec3::ZERO,label:String::new(),save_path:Default::default(),
        edits:Default::default(),imported:false,light_revision:0};
    let mut player=Player::new(Vec3::new(0.,65.,55.));player.set_rotation_degrees(180.,0.);
    let mut sim=Simulation::new(Mode::Creative,189);sim.clock=6000;
    let gui=Gui{screen:Screen::Playing,hide_hud:true,..Default::default()};let ui=Frame::default();
    let input=RenderInput{selected_block:None,assets,resources,settings:&settings,gui:&gui,ui:&ui,
        world:Some(&world),player:Some(&player),simulation:Some(&sim),alpha:1.,time:0.};
    let mut renderer=pollster::block_on(Renderer::new(None,assets,resources,&settings,None)).unwrap();
    renderer.resize(640,360);renderer.configure_near_opaque_shadow(true);
    let mut expected:Vec<(Pixels,serde_json::Value)>=Vec::new();
    let mut observations=Vec::new();
    for enabled in [false,true] {
        renderer.clear();renderer.configure_upload_capture(enabled,None);
        renderer.torch_flicker=Default::default();
        renderer.visual_random=crate::vanilla::random::JavaRandom::new(189);
        for _ in 0..40 {renderer.tick_lighting(&world,&player,&sim,settings.render_distance);}
        for (step,kind) in [0,1,2,3,4,1,0,2].into_iter().enumerate() {
            let previous_caps=renderer.chunks.get(&(0,0)).map(|c|(c.gpu.vertex_capacity,c.gpu.index_capacity));
            let was_cached=renderer.terrain_draws_cached();
            let prepared=PreparedChunk::new(panel(kind));
            let expected_bytes=[(prepared.mesh.vertices.len()*std::mem::size_of::<crate::mesh::Vertex>()) as u64,
                prepared.indices.bytes().len() as u64];
            renderer.upload_prepared((0,0),prepared);
            assert!(!renderer.terrain_draws_cached(),"remembered visible payload replacement invalidates real bundles");
            if enabled {
                let o=renderer.take_upload_observation().unwrap();
                assert_eq!(o.vacant,step==0);assert_eq!(o.position,[0,0]);
                assert_eq!(o.phases[upload_capture::Phase::Initial as usize].calls,u64::from(step==0));
                assert_eq!(o.phases[upload_capture::Phase::Cleanup as usize].calls,1);
                assert_eq!(o.buffers.map(|b|b.submitted_bytes),expected_bytes);
                assert_eq!(o.buffers.map(|b|b.writes),expected_bytes.map(|bytes|u64::from(bytes!=0)));
                if let Some(caps)=previous_caps {assert_eq!(o.buffers.map(|b|b.old_capacity),[caps.0,caps.1]);}
                if was_cached {assert!(o.cache_position_matched);assert_eq!(o.released_bundle_handles,2);}
                observations.push(o);
            }else{assert!(renderer.take_upload_observation().is_none());}
            let mut pixels=read(&mut renderer,&input);
            for _ in 0..3 {let again=read(&mut renderer,&input);assert_eq!(again,pixels,"stable original bundle reuse");pixels=again;}
            if kind!=0 {assert!(renderer.terrain_draws_cached(),"fixture must actually cache both terrain bundles");}
            let value=metadata(&renderer.chunks[&(0,0)]);
            if !enabled {expected.push((pixels,value));}else{
                assert_eq!(pixels,expected[step].0,"captured upload changed real color/depth at step{step}");
                assert_eq!(value,expected[step].1,"captured upload changed geometry/order/capacity at step{step}");
            }
        }
        // Missing-key insertion and replacement must not evict unrelated real
        // bundles. Include a retained Graph and chest vector in old payload.
        assert!(renderer.terrain_draws_cached());
        let mut offscreen=panel(2);for v in &mut offscreen.vertices{v.position[0]+=10000.;}
        offscreen.min.x+=10000.;offscreen.max.x+=10000.;
        let mut prepared=PreparedChunk::new(offscreen);
        prepared.graph=Some(crate::mesh::visibility::TerrainSections::new(&crate::world::Chunk::empty(),true).graph);
        prepared.chests.push(glam::IVec3::new(10000,64,8));
        renderer.upload((600,0),prepared);assert!(renderer.terrain_draws_cached());
        let inserted=renderer.take_upload_observation();
        renderer.upload((600,0),models::empty());assert!(renderer.terrain_draws_cached());
        let replaced=renderer.take_upload_observation();
        if enabled {
            assert!(inserted.unwrap().vacant);
            let o=replaced.unwrap();assert!(o.old_graph_present&&o.old_cpu_capacity_bytes[0]>0);
            assert!(!o.cache_position_matched);assert_eq!(o.released_bundle_handles,0);
            let report=renderer.upload_capture_report();assert_eq!(report["observations"],10);
            assert_eq!(report["invalid_observations"],0);assert_eq!(report["slowest"].as_array().unwrap().len(),8);
        }
        renderer.remove((600,0));
        assert_eq!(read(&mut renderer,&input),expected[7].0);
    }
    assert!(expected[0].0.color.iter().zip(&expected[2].0.color).filter(|(a,b)|a!=b).count()>1000,
        "the original uploaded geometry must visibly affect the frame");
    assert_ne!(expected[2].0.color,expected[3].0.color,"same-capacity byte replacement must be observable");
    assert!(observations.iter().any(|o|o.new_index_width==4&&o.old_index_width==2));
    assert!(observations.iter().any(|o|o.old_index_width==4&&o.new_index_width==2));
    assert!(observations.iter().any(|o|o.buffers[0].new_capacity>o.buffers[0].old_capacity));
    assert!(observations.iter().any(|o|o.buffers[0].new_capacity<o.buffers[0].old_capacity));
}
