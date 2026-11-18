//! Ignored end-to-end Vulkan oracle for the optional same-frame sort scheduler.
//! Real atlas glass geometry is deliberately repeated into32 visible draw slots
//! to cross the admission threshold. This is correctness, not a performance scene.
use super::*;
use super::super::PreparedChunk;
use super::super::super::{RenderInput, Renderer};
use crate::{gameplay::{Mode as GameMode, Simulation}, mesh::{self, Mesh},
    player::Player, settings::Settings, ui::{Frame as UiFrame, Gui, Screen},
    world::{Chunk, World}};
use glam::IVec3;
use std::{path::PathBuf, sync::Arc};

const DRAW_SLOTS: usize = 32;
type Indices = Vec<(ChunkPos, Vec<u32>)>;

fn glass_world() -> World {
    let mut world = World { chunks: crate::world::ChunkMap::from([((0,0),Arc::new(Chunk::empty()))]),
        spawn: Vec3::ZERO, label: String::new(), save_path: Default::default(),
        edits: Default::default(), imported: false, light_revision: 0 };
    //192 disconnected glass blocks give1152 actual blended faces. Distinct
    // atlas colors/depths make wrong ordering observable instead of fog-only.
    for x in (0..16).step_by(2) { for z in (0..16).step_by(2) { for y in [62,64,66] {
        world.set(IVec3::new(x,y,z),95 << 4 | ((x+z+y) % 15) as u16);
    } } }
    world
}

fn upload_scene(renderer: &mut Renderer, template: &Mesh, variant: usize) {
    for slot in 0..DRAW_SLOTS {
        let offset = Vec3::new(0.,variant as f32*0.2,slot as f32*0.035);
        let mut vertices = template.vertices.clone();
        for vertex in &mut vertices {
            let p = Vec3::from_array(vertex.position)+offset; vertex.position=p.to_array();
            // A stable per-slot tint adds another visible inter-draw difference.
            vertex.color[0] = vertex.color[0].saturating_sub((slot*3+variant*11) as u8);
        }
        let mesh = Mesh { sections:None, vertices, indices:template.indices.clone(),
            min:template.min+offset,max:template.max+offset,visible_faces:template.visible_faces };
        let mut prepared = PreparedChunk::new(mesh);
        assert_eq!(prepared.solid_count,0,"fixture must contain only real blended faces");
        assert!(prepared.order.quads.len()>=1024);
        // Exercise two visibility membership bits without a graph culling the
        // synthetic overlapping draw slots by their map keys.
        for (i,quad) in prepared.order.quads.iter_mut().enumerate() {quad.section=(i&1) as u8;}
        prepared.present=3;
        if slot&1!=0 {prepared.force_wide_indices();}
        let initial_indices=prepared.indices.bytes().to_vec();
        let pos=(slot as i32%8,slot as i32/8);
        renderer.upload(pos,prepared);
        let chunk=renderer.chunks.get_mut(&pos).unwrap();
        assert!(chunk.graph.is_none());
        assert_eq!(chunk.gpu.index_format,if slot&1==0 {wgpu::IndexFormat::Uint16}else{wgpu::IndexFormat::Uint32});
        // Test-only readback usage. Preserve initial payload/capacity/format;
        // upload() already invalidated any cached draw that referenced the old
        // buffer. Production sorting and queue.write_buffer remain unchanged.
        chunk.gpu.indices=renderer.device.create_buffer(&wgpu::BufferDescriptor {
            label:Some("sort oracle readable terrain indices"),size:chunk.gpu.index_capacity,
            usage:wgpu::BufferUsages::INDEX|wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation:false });
        renderer.queue.write_buffer(&chunk.gpu.indices,0,&initial_indices);
    }
}

fn invalidate_only_cached_eye(renderer: &mut Renderer) {
    for chunk in renderer.chunks.values_mut() {
        let old=chunk.order.eye.expect("a previous frame sorted this visible chunk");
        chunk.order.eye=Some(old+Vec3::new(2.,0.,0.));
    }
}

fn read_and_check_gpu_indices(renderer: &Renderer) -> Indices {
    let mut positions=renderer.chunks.keys().copied().collect::<Vec<_>>();positions.sort_unstable();
    let mut rows=Vec::new();let mut total=0u64;
    for p in positions {
        let chunk=&renderer.chunks[&p];assert_eq!(chunk.solid_count,0);
        let stride=if chunk.gpu.index_format==wgpu::IndexFormat::Uint16 {2}else{4};
        let size=(chunk.order.indices.len()*stride) as u64;
        assert!(size>0 && size%4==0);
        rows.push((p,total,size,stride));total+=size;
    }
    let buffer=renderer.device.create_buffer(&wgpu::BufferDescriptor {
        label:Some("sort oracle mapped index readback"),size:total,
        usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false });
    let mut encoder=renderer.device.create_command_encoder(&Default::default());
    for &(p,offset,size,_) in &rows {
        encoder.copy_buffer_to_buffer(&renderer.chunks[&p].gpu.indices,0,&buffer,offset,size);
    }
    renderer.queue.submit(Some(encoder.finish()));
    let(tx,rx)=std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read,move|result|{let _=tx.send(result);});
    renderer.device.poll(wgpu::Maintain::Wait);rx.recv().unwrap().unwrap();
    let mapped=buffer.slice(..).get_mapped_range();
    let mut result=Vec::new();
    for (p,offset,size,stride) in rows {
        let bytes=&mapped[offset as usize..(offset+size) as usize];
        let indices=if stride==2 {
            bytes.chunks_exact(2).map(|v|u32::from(u16::from_le_bytes([v[0],v[1]]))).collect::<Vec<_>>()
        }else{
            bytes.chunks_exact(4).map(|v|u32::from_le_bytes([v[0],v[1],v[2],v[3]])).collect::<Vec<_>>()
        };
        assert_eq!(indices,renderer.chunks[&p].order.indices,"GPU upload differs at{p:?}");
        result.push((p,indices));
    }
    drop(mapped);buffer.unmap();result
}

struct TempImage(PathBuf);
impl Drop for TempImage {fn drop(&mut self){let _=std::fs::remove_file(&self.0);}}

#[test]
#[ignore="requires original block resources and Vulkan pixel/index readback; run alone"]
fn all_sort_modes_match_vulkan_pixels_and_indices_with_real_batch_admission() {
    let assets=crate::validation::assets();let resources=crate::validation::resources();
    let world=glass_world();
    let settings=Settings {render_distance:4,clouds:0,entity_shadows:false,..Default::default()};
    let template=mesh::build_with_settings(&world,(0,0),assets,&settings);
    assert!(template.quads()>=1024);
    assert!(template.vertices.iter().all(|v|v.layer&crate::models::BLEND!=0));
    assert!(template.quads()*DRAW_SLOTS>=MIN_QUADS && template.quads()*DRAW_SLOTS<=MAX_QUADS);
    let mut sim=Simulation::new(GameMode::Creative,189);sim.clock=6000;
    let gui=Gui {screen:Screen::Playing,hide_hud:true,..Default::default()};
    let ui=UiFrame::default();
    let path=TempImage(std::env::temp_dir().join(format!("ferrum-scoped-sort-gpu-{}-{}.png",
        std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())));
    let mut reference:Option<Vec<(image::RgbaImage,Indices)>>=None;
    for mode in [Mode::Original,Mode::OwnerBatched,Mode::ScopedOne] {
        let mut renderer=pollster::block_on(Renderer::new(None,assets,resources,&settings,None)).unwrap();
        renderer.resize(320,180);renderer.configure_transparency_sort(mode);
        renderer.transparency_sort.ensure_helper_for_test();
        let mut player=Player::new(Vec3::new(8.,70.,29.));
        player.set_rotation_degrees(180.,20.);
        for _ in 0..40 {renderer.tick_lighting(&world,&player,&sim,settings.render_distance);}
        upload_scene(&mut renderer,&template,0);
        let mut frames:Vec<(image::RgbaImage,Indices)>=Vec::new();
        for phase in 0..7 {
            match phase {
                0|1=>{}, // first sort, then unchanged-eye skipped-sort frame
                2=>invalidate_only_cached_eye(&mut renderer),
                3=>player.set_pose_f64([11.,70.,29.],180.,20.,false),
                4=>{for chunk in renderer.chunks.values_mut(){chunk.present=1;}},
                5=>upload_scene(&mut renderer,&template,1),
                6=>{
                    renderer.configure_transparency_capture(true);
                    renderer.transparency_capture_time(Some(100.));
                    invalidate_only_cached_eye(&mut renderer);
                },
                _=>unreachable!(),
            }
            // Phase2 changes only the cached eligibility eye: actual camera and
            // existing sorted keys are identical, forcing Sort::Identical work.
            let runs=renderer.transparency_sort.successful_runs();
            let input=RenderInput {selected_block:None,assets,resources,settings:&settings,
                gui:&gui,ui:&ui,world:Some(&world),player:Some(&player),simulation:Some(&sim),alpha:1.,time:0.};
            renderer.screenshot(&path.0,&input).unwrap();
            assert_eq!(renderer.visible_chunks,DRAW_SLOTS,"all threshold-forcing slots must be visible");
            let should_batch=mode!=Mode::Original && !matches!(phase,1|6);
            assert_eq!(renderer.transparency_sort.successful_runs()-runs,usize::from(should_batch),
                "real scheduler admission or capture bypass differs in{mode:?} phase{phase}");
            let pixels=image::open(&path.0).unwrap().to_rgba8();
            let indices=read_and_check_gpu_indices(&renderer);
            if matches!(phase,1|2) {
                assert_eq!(pixels,frames[0].0,"unchanged scene must preserve its first pixels");
                assert_eq!(indices,frames[0].1,"same-order and skipped cases must preserve GPU indices");
            }
            if phase==6 {
                assert_eq!(renderer.transparency_capture_report()["cases"],1,
                    "capture bypass must execute an actual begin/finish ticket");
                assert_eq!(pixels,frames[5].0,"capture forces serial without changing pixels");
                assert_eq!(indices,frames[5].1,"capture bypass must preserve the same indices");
            }
            if let Some(reference)=&reference {
                assert_eq!(pixels,reference[phase].0,"Vulkan pixels differ in{mode:?} phase{phase}");
                assert_eq!(indices,reference[phase].1,"uploaded index order differs in{mode:?} phase{phase}");
            }
            frames.push((pixels,indices));
        }
        assert_ne!(frames[0].1,frames[4].1,"mask phase must remove actual GPU membership");
        assert_ne!(frames[0].0,frames[3].0,"moving-eye phase must change visible projection");
        // Reject a fixture that accidentally compares identical empty/fog-only
        // images. Keep the final camera/hand/sky identical and remove terrain.
        let positions=renderer.chunks.keys().copied().collect::<Vec<_>>();
        for p in positions {renderer.remove(p);}
        let input=RenderInput {selected_block:None,assets,resources,settings:&settings,
            gui:&gui,ui:&ui,world:Some(&world),player:Some(&player),simulation:Some(&sim),alpha:1.,time:0.};
        renderer.screenshot(&path.0,&input).unwrap();
        let empty=image::open(&path.0).unwrap().to_rgba8();
        let visible=frames[6].0.pixels().zip(empty.pixels()).filter(|(a,b)|
            (0..3).any(|c|a[c].abs_diff(b[c])>8)).count();
        assert!(visible>500,"glass fixture must visibly render, only{visible} changed pixels");
        eprintln!("Scoped-sort Vulkan oracle{mode:?}:{} quads,{visible} visible pixels; exact narrow/wide GPU uploads",
            template.quads()*DRAW_SLOTS);
        if reference.is_none(){reference=Some(frames);}
    }
}


#[test]
#[ignore="requires original block resources and Vulkan pixel/index readback; run alone"]
fn large_sort_mode_matches_all_modes_with_actual_98304_plus_quad_admission() {
    let assets=crate::validation::assets();let resources=crate::validation::resources();
    let mut world=glass_world();
    // Six extra disconnected layers produce3456 blend quads per slot,
    //110592 total. Leave the existing seven-phase small fixture unchanged.
    for x in (0..16).step_by(2) {for z in (0..16).step_by(2) {for y in (50..=60).step_by(2) {
        world.set(IVec3::new(x,y,z),95<<4|((x+z+y)%15) as u16);
    }}}
    let settings=Settings {render_distance:4,clouds:0,entity_shadows:false,..Default::default()};
    let template=mesh::build_with_settings(&world,(0,0),assets,&settings);
    let total_quads=template.quads()*DRAW_SLOTS;
    assert!(total_quads>=LARGE_MIN_QUADS && total_quads<=MAX_QUADS);
    assert!(template.vertices.iter().all(|v|v.layer&crate::models::BLEND!=0));
    let mut sim=Simulation::new(GameMode::Creative,189);sim.clock=6000;
    let gui=Gui {screen:Screen::Playing,hide_hud:true,..Default::default()};let ui=UiFrame::default();
    let path=TempImage(std::env::temp_dir().join(format!("ferrum-large-sort-gpu-{}-{}.png",
        std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())));
    let mut reference:Option<Vec<(image::RgbaImage,Indices)>>=None;
    for mode in [Mode::Original,Mode::OwnerBatched,Mode::ScopedOne,Mode::ScopedLarge] {
        let mut renderer=pollster::block_on(Renderer::new(None,assets,resources,&settings,None)).unwrap();
        renderer.resize(320,180);renderer.configure_transparency_sort(mode);
        renderer.transparency_sort.ensure_helper_for_test();
        let mut player=Player::new(Vec3::new(8.,70.,29.));player.set_rotation_degrees(180.,20.);
        for _ in 0..40 {renderer.tick_lighting(&world,&player,&sim,settings.render_distance);}
        upload_scene(&mut renderer,&template,0);
        let mut frames=Vec::new();
        for phase in 0..2 {
            if phase==1 {player.set_pose_f64([11.,70.,29.],180.,20.,false);}
            let runs=renderer.transparency_sort.successful_runs();
            let input=RenderInput {selected_block:None,assets,resources,settings:&settings,
                gui:&gui,ui:&ui,world:Some(&world),player:Some(&player),simulation:Some(&sim),alpha:1.,time:0.};
            renderer.screenshot(&path.0,&input).unwrap();
            assert_eq!(renderer.visible_chunks,DRAW_SLOTS);
            assert_eq!(renderer.transparency_sort.successful_runs()-runs,usize::from(mode!=Mode::Original),
                "{mode:?} must really admit the large batch in phase{phase}");
            let pixels=image::open(&path.0).unwrap().to_rgba8();
            let indices=read_and_check_gpu_indices(&renderer);
            if let Some(reference)=&reference {
                assert_eq!(pixels,reference[phase].0,"pixels differ in{mode:?} phase{phase}");
                assert_eq!(indices,reference[phase].1,"GPU index bytes differ in{mode:?} phase{phase}");
            }
            frames.push((pixels,indices));
        }
        assert_ne!(frames[0].0,frames[1].0,"moving eye must change visible projection");
        let positions=renderer.chunks.keys().copied().collect::<Vec<_>>();
        for p in positions {renderer.remove(p);}
        let input=RenderInput {selected_block:None,assets,resources,settings:&settings,
            gui:&gui,ui:&ui,world:Some(&world),player:Some(&player),simulation:Some(&sim),alpha:1.,time:0.};
        renderer.screenshot(&path.0,&input).unwrap();
        let empty=image::open(&path.0).unwrap().to_rgba8();
        let visible=frames[1].0.pixels().zip(empty.pixels()).filter(|(a,b)|
            (0..3).any(|c|a[c].abs_diff(b[c])>8)).count();
        assert!(visible>500,"large fixture must render visible glass, only{visible} pixels");
        eprintln!("Large-sort Vulkan oracle{mode:?}: {total_quads} quads,{visible} visible pixels; exact narrow/wide GPU uploads");
        if reference.is_none(){reference=Some(frames);}
    }
}


#[test]
#[ignore="requires original block resources and Vulkan pixel/index readback; focused >262144 quad admission"]
fn wide_sort_mode_matches_original_and_old_limit_fallbacks_above_262144_quads() {
    let assets=crate::validation::assets();let resources=crate::validation::resources();
    let mut world=glass_world();
    // Twenty-one additional disconnected layers plus the existing three give
    //24*64 real blocks*6 faces =9216 quads per slot,294912 total. Each slot's
    //36864 vertices fit u16; upload_scene additionally forces half to u32.
    // This deliberately exercises the new quad cap with only32 visible jobs.
    for x in (0..16).step_by(2) {for z in (0..16).step_by(2) {for y in (20..=60).step_by(2) {
        world.set(IVec3::new(x,y,z),95<<4|((x+z+y)%15) as u16);
    }}}
    let settings=Settings{render_distance:4,clouds:0,entity_shadows:false,..Default::default()};
    let template=mesh::build_with_settings(&world,(0,0),assets,&settings);
    let total_quads=template.quads()*DRAW_SLOTS;
    assert_eq!(total_quads,294912,"actual disconnected atlas-glass geometry must cross the old quad cap");
    assert!(total_quads>MAX_QUADS && total_quads<=1_048_576);
    assert!(template.vertices.len()<=u16::MAX as usize);
    assert!(template.vertices.iter().all(|v|v.layer&crate::models::BLEND!=0));
    let mut sim=Simulation::new(GameMode::Creative,189);sim.clock=6000;
    let gui=Gui{screen:Screen::Playing,hide_hud:true,..Default::default()};let ui=UiFrame::default();
    let path=TempImage(std::env::temp_dir().join(format!("ferrum-wide-sort-gpu-{}-{}.png",
        std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())));
    let mut reference:Option<Vec<(image::RgbaImage,Indices)>>=None;
    for mode in [Mode::Original,Mode::OwnerBatched,Mode::ScopedOne,Mode::ScopedLarge,Mode::ScopedWide] {
        let mut renderer=pollster::block_on(Renderer::new(None,assets,resources,&settings,None)).unwrap();
        renderer.resize(320,180);renderer.configure_transparency_sort(mode);
        renderer.transparency_sort.ensure_helper_for_test();
        let mut player=Player::new(Vec3::new(8.,70.,29.));player.set_rotation_degrees(180.,20.);
        for _ in 0..40 {renderer.tick_lighting(&world,&player,&sim,settings.render_distance);}
        upload_scene(&mut renderer,&template,0);
        assert_eq!(renderer.chunks.values().filter(|c|c.gpu.index_format==wgpu::IndexFormat::Uint16).count(),DRAW_SLOTS/2);
        assert_eq!(renderer.chunks.values().filter(|c|c.gpu.index_format==wgpu::IndexFormat::Uint32).count(),DRAW_SLOTS/2);
        let mut frames:Vec<(image::RgbaImage,Indices)>=Vec::new();
        for phase in 0..3 {
            if phase==2 {player.set_pose_f64([11.,70.,29.],180.,20.,false);}
            let runs=renderer.transparency_sort.successful_runs();
            let input=RenderInput{selected_block:None,assets,resources,settings:&settings,
                gui:&gui,ui:&ui,world:Some(&world),player:Some(&player),simulation:Some(&sim),alpha:1.,time:0.};
            renderer.screenshot(&path.0,&input).unwrap();
            assert_eq!(renderer.visible_chunks,DRAW_SLOTS,"all threshold-forcing jobs must reach the actual draw pass");
            let should_batch=mode==Mode::ScopedWide&&phase!=1;
            assert_eq!(renderer.transparency_sort.successful_runs()-runs,usize::from(should_batch),
                "{mode:?} phase{phase}: only ScopedWide may admit above the original262144 cap; unchanged eye must skip");
            let pixels=image::open(&path.0).unwrap().to_rgba8();
            // Reads actual active GPU bytes for both u16/u32 and independently
            // checks each buffer against the production CPU Order indices.
            let indices=read_and_check_gpu_indices(&renderer);
            assert_eq!(indices.iter().map(|(_,i)|i.len()/6).sum::<usize>(),total_quads);
            if phase==1 {
                assert!(pixels==frames[0].0,"unchanged-eye pixels differ in{mode:?}");
                assert!(indices==frames[0].1,"unchanged-eye GPU indices differ in{mode:?}");
            }
            if let Some(reference)=&reference {
                assert!(pixels==reference[phase].0,"real pixels differ from Original in{mode:?} phase{phase}");
                assert!(indices==reference[phase].1,"active u16/u32 GPU order differs from Original in{mode:?} phase{phase}");
            }
            frames.push((pixels,indices));
        }
        assert_ne!(frames[0].0,frames[2].0,"moving camera must change the visible projection");
        assert!(frames[0].1!=frames[2].1,"moving camera must also produce a real index-order change");
        assert_eq!(renderer.transparency_sort.successful_runs(),if mode==Mode::ScopedWide{2}else{0});
        let positions=renderer.chunks.keys().copied().collect::<Vec<_>>();
        for p in positions {renderer.remove(p);}
        let input=RenderInput{selected_block:None,assets,resources,settings:&settings,
            gui:&gui,ui:&ui,world:Some(&world),player:Some(&player),simulation:Some(&sim),alpha:1.,time:0.};
        renderer.screenshot(&path.0,&input).unwrap();
        let empty=image::open(&path.0).unwrap().to_rgba8();
        let visible=frames[2].0.pixels().zip(empty.pixels()).filter(|(a,b)|
            (0..3).any(|c|a[c].abs_diff(b[c])>8)).count();
        assert!(visible>500,"large real-glass geometry must visibly render, only{visible} changed pixels");
        eprintln!("Wide-sort Vulkan oracle{mode:?}: {total_quads} real quads in{DRAW_SLOTS} jobs,{visible} visible pixels; exact narrow/wide GPU uploads");
        if reference.is_none(){reference=Some(frames);}
    }
}

#[path="transparency_sort_scoped_two_gpu_tests.rs"]
mod scoped_two_tests;
