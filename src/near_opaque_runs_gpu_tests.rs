//! Independent real GPU direct-path oracle for the opt-in opaque run cache.
use super::*;
use super::super::*;
use crate::{gameplay::Mode,ui::Screen};

#[derive(Debug,PartialEq,Eq)]
struct Pixels {color:Vec<[u8;4]>,depth:Vec<u32>}
fn read(renderer:&mut Renderer,input:&RenderInput<'_>)->Pixels {
    renderer.terrain_bundles.invalidate(); // force the existing direct/run choice every time
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
    assert!(!renderer.terrain_draws_cached(),"reference and candidate must bypass whole-list bundles");
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
fn pair(renderer:&mut Renderer,input:&RenderInput<'_>,label:&str)->Pixels {
    // Same device, map encounter order, buffers and bindings. Temporarily park
    // ONLY the experiment so the independent, unchanged direct loop executes.
    let candidate=std::mem::take(&mut renderer.near_opaque_runs);
    let expected=read(renderer,input);renderer.near_opaque_runs=candidate;
    let actual=read(renderer,input);
    assert!(expected.color==actual.color,"{label}: color differs from existing direct path");
    assert!(expected.depth==actual.depth,"{label}: depth differs from existing direct path");actual
}
fn panel(i:i32,variant:u8,wide:bool,texture:u32)->Mesh {
    let mut mesh=models::empty();
    let x=(i%6-3) as f32*5.;let y=57.+(i/6) as f32*3.;let z=8.;
    let mut data=crate::mesh::visibility::TerrainSections::new(&crate::world::Chunk::empty(),true);
    let mut face=|x:f32,y:f32,z:f32,w:f32,h:f32,section:u8,layer:u32,color:[u8;4]| {
        models::face(&mut mesh,[Vec3::new(x,y,z),Vec3::new(x+w,y,z),Vec3::new(x+w,y+h,z),Vec3::new(x,y+h,z)],
            [0.,0.,4.,1.],layer,color);data.owners.push(section);
    };
    for section in 3..6u8 {
        let textured=section==4&&i%3==0;
        let layer=if textured {texture|models::UNLIT}else{models::SOLID|models::UNLIT};
        let color=if textured {[255;4]}else{[50+(i%6) as u8*28,55+(i/6) as u8*35,90+variant*35,255]};
        face(x,y+f32::from(section-3)*0.8,z,4.2,0.65,section,layer,color);
    }
    // Every chunk writes this exact coplanar area with a distinct color. The
    // LessEqual last-writer result makes any reordered opaque run observable.
    face(-1.,70.,10.,2.,1.5,3,models::SOLID|models::UNLIT,[20+i as u8*8,180,35+variant*30,255]);
    // Subsequent transparent drawing requires restored pass bindings after a
    // cached run. Keep its geometry/input exactly the same in both captures.
    face(x+0.8,y+0.25,z+0.2,2.5,1.9,3,models::SOLID|models::UNLIT|models::BLEND,[70,130,230,100]);
    drop(face);
    if variant==2 { // changed count without changing vertex-buffer capacity
        mesh.indices.drain(0..6);data.owners.remove(0);
    }
    if wide {
        let prefix=vec![mesh.vertices[0];65536];mesh.vertices.splice(0..0,prefix);
        for index in &mut mesh.indices {*index+=65536;}
    }
    mesh.min=mesh.vertices.iter().map(|v|Vec3::from_array(v.position)).fold(Vec3::splat(f32::INFINITY),Vec3::min);
    mesh.max=mesh.vertices.iter().map(|v|Vec3::from_array(v.position)).fold(Vec3::splat(f32::NEG_INFINITY),Vec3::max);
    mesh.sections=Some(data);mesh
}
fn upload(renderer:&mut Renderer,i:i32,variant:u8,wide:bool) {
    renderer.upload((i,0),panel(i,variant,wide,crate::validation::assets().texture(1<<4,2,true)));
    // Retain real per-section owner/range preparation, but make test panels
    // independent of world voxel connectivity and synthetic map key positions.
    renderer.chunks.get_mut(&(i,0)).unwrap().graph=None;
}
fn metric(renderer:&Renderer,group:&str,name:&str)->u64 {
    renderer.near_opaque_runs_report()[group][name].as_u64().unwrap()
}
fn contains(renderer:&Renderer,p:ChunkPos)->bool {
    renderer.near_opaque_runs.state.as_ref().unwrap().cache.entries.values()
        .any(|entry|entry.key.iter().any(|key|key.position==p))
}

#[test]
#[ignore="requires original resources and Vulkan; exact ordered run/direct RGBA+depth with real bundle reuse"]
fn exact_opaque_runs_match_direct_pixels_through_payload_masks_bindings_and_reuse() {
    let assets=crate::validation::assets();let resources=crate::validation::resources();
    let mut settings=Settings{render_distance:16,clouds:0,entity_shadows:false,..Default::default()};
    let world=World{chunks:Default::default(),spawn:Vec3::ZERO,label:String::new(),save_path:Default::default(),
        edits:Default::default(),imported:false,light_revision:0};
    let mut player=Player::new(Vec3::new(0.,65.,55.));player.set_rotation_degrees(180.,0.);
    let mut sim=Simulation::new(Mode::Creative,189);sim.clock=6000;
    let gui=Gui{screen:Screen::Playing,hide_hud:true,..Default::default()};let ui=Frame::default();
    let mut renderer=pollster::block_on(Renderer::new(None,assets,resources,&settings,None)).unwrap();
    renderer.resize(640,360);renderer.configure_near_opaque_runs(true);
    for _ in 0..40 {renderer.tick_lighting(&world,&player,&sim,settings.render_distance);}
    for i in 0..24 {upload(&mut renderer,i,0,false);}
    let capture=|renderer:&mut Renderer,settings:&Settings,player:&Player,label:&str| {
        pair(renderer,&RenderInput{selected_block:None,assets,resources,settings,gui:&gui,ui:&ui,
            world:Some(&world),player:Some(player),simulation:Some(&sim),alpha:1.,time:0.},label)
    };
    let initial=capture(&mut renderer,&settings,&player,"first mixed cached/direct frame");
    assert_eq!(renderer.visible_chunks,24,"all six groups must actually reach the draw pass");
    assert_eq!(metric(&renderer,"measured","built_groups"),4,"per-frame real GPU build cap");
    assert!(metric(&renderer,"draw","direct_draws")>0,"remaining two groups use the independent direct fallback");
    assert_eq!(renderer.near_opaque_runs_report()["cache_entries"],4);
    for step in 0..3 {assert_eq!(capture(&mut renderer,&settings,&player,&format!("reuse{step}")),initial);}
    assert!(metric(&renderer,"draw","reused_bundle_calls")>0,"must execute retained actual RenderBundles");
    assert!(metric(&renderer,"draw","bundled_draws")>0);
    assert_eq!(renderer.near_opaque_runs_report()["cache_entries"],6);
    assert!((0..24).all(|i|contains(&renderer,(i,0))));

    // With no sky pass, nothing can accidentally repair bundle-cleared state
    // before real dynamic geometry. Require a visible mob and cached hits.
    let mut nether=sim.clone();nether.dimension=-1;
    let reused_before=metric(&renderer,"draw","reused_bundle_calls");
    let nether_frame=|renderer:&mut Renderer,simulation:&Simulation,label:&str| {
        pair(renderer,&RenderInput{selected_block:None,assets,resources,settings:&settings,gui:&gui,ui:&ui,
            world:Some(&world),player:Some(&player),simulation:Some(simulation),alpha:1.,time:0.},label)
    };
    let no_mob=nether_frame(&mut renderer,&nether,"no sky, cached opaque plus transparency");
    let feet=Vec3::new(0.,64.,22.);
    nether.mobs.push(crate::gameplay::Mob{id:17,kind:crate::gameplay::MobKind::Sheep,position:feet,previous:feet,
        velocity:Vec3::ZERO,yaw:std::f32::consts::FRAC_PI_2,health:8.,age:20,turn_ticks:0,hurt:0,fuse:0,
        swing_ticks:0,death_ticks:0,eat_ticks:0,attack_ticks:0,rose_ticks:0,prev_fuse:0});
    let mob=nether_frame(&mut renderer,&nether,"no sky, cached opaque then visible dynamic mob");
    assert!(renderer.dynamic.count>0&&renderer.visible_entities>0,"real dynamic geometry must be drawn");
    assert!(no_mob.color.iter().zip(&mob.color).filter(|(a,b)|a!=b).count()>20,"dynamic sheep must visibly affect pixels");
    assert_eq!(nether_frame(&mut renderer,&nether,"no-sky dynamic retained reuse"),mob);
    assert!(metric(&renderer,"draw","reused_bundle_calls")>reused_before);

    let capacities={let g=&renderer.chunks[&(0,0)].gpu;(g.vertex_capacity,g.index_capacity)};
    upload(&mut renderer,0,2,false);assert!(!contains(&renderer,(0,0)));
    let g=&renderer.chunks[&(0,0)].gpu;assert_eq!((g.vertex_capacity,g.index_capacity),capacities);
    let changed=capture(&mut renderer,&settings,&player,"same-capacity color/count change");
    assert_ne!(changed.color,initial.color,"payload mutation must change visible pixels");
    let old_capacity=renderer.chunks[&(1,0)].gpu.vertex_capacity;
    upload(&mut renderer,1,1,true);assert!(!contains(&renderer,(1,0)));
    assert_eq!(renderer.chunks[&(1,0)].gpu.index_format,wgpu::IndexFormat::Uint32);
    assert!(renderer.chunks[&(1,0)].gpu.vertex_capacity>old_capacity);
    capture(&mut renderer,&settings,&player,"real wide index and buffer reallocation");
    renderer.remove((2,0));assert!(!contains(&renderer,(2,0)));
    capture(&mut renderer,&settings,&player,"resident removal");
    upload(&mut renderer,2,1,false);assert!(!contains(&renderer,(2,0)));
    capture(&mut renderer,&settings,&player,"same-key ABA reinsert");

    let old_builds=metric(&renderer,"measured","built_groups");
    for chunk in renderer.chunks.values_mut(){chunk.present=(1<<3)|(1<<5);}
    capture(&mut renderer,&settings,&player,"nonadjacent owner masks");
    assert!(renderer.chunks.values().all(|chunk|chunk.mask==((1<<3)|(1<<5))&&chunk.solid_ranges.len()==2));
    assert!(metric(&renderer,"measured","built_groups")>old_builds,"mask/range changes invalidate exact run keys");
    for chunk in renderer.chunks.values_mut(){chunk.present=(1<<3)|(1<<4)|(1<<5);}
    capture(&mut renderer,&settings,&player,"restore complete owner mask");
    renderer.chunks.get_mut(&(3,0)).unwrap().present=0;
    capture(&mut renderer,&settings,&player,"visible sequence drops a middle item");
    assert_eq!(renderer.visible_chunks,23);
    renderer.chunks.get_mut(&(3,0)).unwrap().present=(1<<3)|(1<<4)|(1<<5);
    capture(&mut renderer,&settings,&player,"visible sequence returns");
    for (stereo,mipmaps) in [(true,0),(false,4),(true,4),(false,0)] {
        let old_mipmaps=settings.mipmaps;settings.anaglyph=stereo;settings.mipmaps=mipmaps;
        renderer.apply_settings(&settings);
        if old_mipmaps!=mipmaps {assert_eq!(renderer.near_opaque_runs_report()["cache_entries"],0);}
        capture(&mut renderer,&settings,&player,"eye/bind-group replacement");
        for _ in 0..2 {capture(&mut renderer,&settings,&player,"eye/bind-group reuse");}
        let state=renderer.near_opaque_runs.state.as_ref().unwrap();
        assert!(state.cache.entries.values().all(|entry|entry.value.0[0].is_some()
            &&entry.value.0[1].is_some()==stereo));
    }
    renderer.resize(672,384);capture(&mut renderer,&settings,&player,"resize");
    let visible=capture(&mut renderer,&settings,&player,"visible before empty control");
    for i in 0..24 {renderer.remove((i,0));}
    let empty=capture(&mut renderer,&settings,&player,"empty after removals");
    assert!(visible.color.iter().zip(&empty.color).filter(|(a,b)|a!=b).count()>1000,"avoid equal invisible/fog-only frames");
    assert_eq!(renderer.near_opaque_runs_report()["cache_entries"],0);
    assert_eq!(renderer.near_opaque_runs_report()["identity_count"],0);

    // Resource-pack replacement constructs a new renderer/device resource set.
    let mut replacement=pollster::block_on(Renderer::new(None,assets,resources,&settings,None)).unwrap();
    replacement.resize(640,360);replacement.configure_near_opaque_runs(true);
    assert_eq!(replacement.near_opaque_runs_report()["cache_entries"],0);
    for i in 0..8 {upload(&mut replacement,i,1,false);}
    capture(&mut replacement,&settings,&player,"fresh renderer/bindings");
    assert_eq!(metric(&replacement,"measured","built_groups"),2);
    capture(&mut replacement,&settings,&player,"fresh renderer reuse");
    assert!(metric(&replacement,"draw","reused_bundle_calls")>0);
}
