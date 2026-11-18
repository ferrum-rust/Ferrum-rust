//! Real Renderer marker oracle: unchanged draws, bindings, backing and queued
//! output snapshots. No surface/display timing claim from this headless test.
use super::*;

fn hud(size:[u32;2],enabled:bool)->Frame {
    let mut ui=Frame {size:size.map(|v|v as f32),..Default::default()};
    if enabled {
        for position in [[-0.9,0.9],[-0.9,0.6],[-0.3,0.6],[-0.9,0.9],[-0.3,0.6],[-0.3,0.9]] {
            ui.vertices.push(crate::ui::UiVertex {position,uv:[-1.,-1.],
                color:[0.1,0.8,0.2,0.75],layer:0,glint:[0.;4]});
        }
    }
    ui
}

fn stage_capture(renderer:&mut Renderer,input:&RenderInput<'_>,frame:usize)->Ticket {
    let [width,height]=renderer.resolution();
    let extent=wgpu::Extent3d {width,height,depth_or_array_layers:1};
    let texture=|format,label|renderer.device.create_texture(&wgpu::TextureDescriptor {
        label:Some(label),size:extent,mip_level_count:1,sample_count:1,
        dimension:wgpu::TextureDimension::D2,format,
        usage:wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::COPY_SRC,view_formats:&[]});
    let color=texture(renderer.config.format,"GPU stages real Renderer color");
    let depth=texture(DEPTH,"GPU stages real Renderer readable depth");
    renderer.depth=depth.create_view(&Default::default());
    let row=(width*4).div_ceil(256)*256;
    let plane=u64::from(row)*u64::from(height);
    let mut keys=renderer.chunks.keys().copied().collect::<Vec<_>>();keys.sort_unstable();
    let bytes=keys.iter().map(|key| {
        let g=&renderer.chunks[key].gpu;
        (u64::from(g.count)*if g.index_format==wgpu::IndexFormat::Uint16 {2}else{4}).div_ceil(4)*4
    }).sum::<u64>();
    let buffer=renderer.device.create_buffer(&wgpu::BufferDescriptor {
        label:Some("GPU stages queued output snapshot"),size:plane*2+bytes,
        usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
    let encoder=renderer.device.create_command_encoder(&Default::default());
    let mut profile=render_profile::Frame {enabled:true,frame_id:frame as u64,..Default::default()};
    let mut geometry=Vec::new();
    renderer.submit_arena_test_frame(encoder,&color.create_view(&Default::default()),input,&mut profile,|encoder,renderer| {
        for (texture,offset,aspect) in [(&color,0,wgpu::TextureAspect::All),(&depth,plane,wgpu::TextureAspect::DepthOnly)] {
            encoder.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo {texture,mip_level:0,
                origin:wgpu::Origin3d::ZERO,aspect},wgpu::TexelCopyBufferInfo {buffer:&buffer,
                layout:wgpu::TexelCopyBufferLayout {offset,bytes_per_row:Some(row),rows_per_image:Some(height)}},extent);
        }
        let mut offset=plane*2;
        for key in keys {
            let chunk=&renderer.chunks[&key];let gpu=&chunk.gpu;
            let width=if gpu.index_format==wgpu::IndexFormat::Uint16 {2}else{4};
            let n=(u64::from(gpu.count)*u64::from(width)).div_ceil(4)*4;
            if n!=0 {encoder.copy_buffer_to_buffer(&gpu.indices,gpu.index_offset,&buffer,offset,n);}
            geometry.push((Geometry {position:key,count:gpu.count,width,
                bounds:[gpu.min.to_array().map(f32::to_bits),gpu.max.to_array().map(f32::to_bits)],
                present:chunk.present,mask:chunk.mask,solid:chunk.solid_ranges.clone(),
                transparent:chunk.translucent_range(),bytes:Vec::new()},offset));
            offset+=n;
        }
    });
    // Synthetic test anchors only; production App still supplies its actual
    // successful-present Instants, with no new clock read in this diagnostic.
    renderer.gpu_timestamps_presented(frame.checked_sub(1),
        frame.checked_sub(1).map(|v|v as f64*10.),frame as f64*10.).unwrap();
    Ticket {buffer,width,height,row,plane,geometry}
}

#[test]
#[ignore="requires original resources and Vulkan in-pass timestamp/native MDI support"]
fn gpu_stage_markers_preserve_actual_renderer_world_stereo_menu_postfx_and_ui() {
    let world=world();let player=player();
    let mut sim=Simulation::new(GameMode::Creative,189);sim.clock=6000;
    for choice in [Some(Choice::Original),None] {
        let mut expected:Option<Vec<Image>>=None;
        let mut expected_screenshot=None;
        for stages in [false,true] {
            let mut settings=settings();settings.render_distance=128;
            // Both reference/candidate Devices negotiate the SAME feature set;
            // only the collector's actual in-pass marker path differs.
            let mut renderer=pollster::block_on(Renderer::new_with_gpu_stage_diagnostics(None,
                crate::validation::assets(),crate::validation::resources(),&settings,None,choice,
                false,true,true)).unwrap();
            renderer.resize(320,180);
            if !stages {renderer.gpu_timestamps=Some(gpu_timestamps::Capture::new(&renderer.device,&renderer.queue).unwrap());}
            assert!(renderer.device.features().contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES));
            assert_eq!(renderer.near_arena_report()["policy"]["automatic"],choice.is_none());
            if choice.is_none() {assert_eq!(renderer.near_arena_report()["effective"],"ArenaMdi");}
            else {assert!(renderer.near_arena.is_none());}
            renderer.torch_flicker=Default::default();
            renderer.visual_random=crate::vanilla::random::JavaRandom::new(189);
            for _ in 0..40 {renderer.tick_lighting(&world,&player,&sim,settings.render_distance);}
            // Load a real original post-processing graph before any submission;
            // no device error-scope readback/wait occurs amid queued frames.
            renderer.load_shader(crate::validation::resources(),0).unwrap();
            for id in 0..4 {upload(&mut renderer,id,0,id==3);}
            if choice.is_none() {
                assert!(renderer.chunks.values().any(|c|c.gpu.arena_token.is_some()&&c.gpu.index_offset!=0));
            }
            let mut tickets=Vec::new();
            // (world tuple present, world option alone, stereo, postfx, GUI)
            let shapes=[(true,true,false,false,false),(true,true,true,false,true),
                (true,true,true,true,true),(true,true,false,true,false),
                (false,false,false,false,false),(false,false,false,false,true),
                (false,true,false,true,true),(true,true,false,false,true),
                (true,true,false,false,false)];
            for (frame,(world_pass,world_option,stereo,postfx,gui_enabled)) in shapes.into_iter().enumerate() {
                settings.anaglyph=stereo;renderer.apply_settings(&settings);
                renderer.postfx.as_mut().unwrap().enabled=postfx;
                if frame==2 {renderer.resize(322,182);}
                if frame==3 {upload(&mut renderer,1,1,true);}
                if frame==8 {for id in 0..4 {renderer.remove((id,0));}}
                let gui=Gui {screen:if world_pass {Screen::Playing}else{Screen::Main},hide_hud:!gui_enabled,..Default::default()};
                let ui=hud(renderer.resolution(),gui_enabled);
                let input=RenderInput {selected_block:None,assets:crate::validation::assets(),
                    resources:crate::validation::resources(),settings:&settings,gui:&gui,ui:&ui,
                    world:world_option.then_some(&world),player:world_pass.then_some(&player),
                    simulation:world_pass.then_some(&sim),alpha:1.,time:frame as f32/100.};
                tickets.push(stage_capture(&mut renderer,&input,frame));
            }
            assert_eq!(renderer.gpu_timestamps_report()["submitted"],9);
            // Actual raw encode and screenshot have no pending surface ticket.
            // They must not append markers/rows or reuse old optional slots.
            for id in 0..4 {upload(&mut renderer,id,0,id==3);}
            let gui=Gui {screen:Screen::Playing,hide_hud:true,..Default::default()};
            let ui=hud(renderer.resolution(),false);
            let input=RenderInput {selected_block:None,assets:crate::validation::assets(),
                resources:crate::validation::resources(),settings:&settings,gui:&gui,ui:&ui,
                world:Some(&world),player:Some(&player),simulation:Some(&sim),alpha:1.,time:0.};
            let before=renderer.gpu_timestamps_report();
            if choice.is_some() {
                let output=renderer.device.create_texture(&wgpu::TextureDescriptor {label:None,
                    size:wgpu::Extent3d {width:322,height:182,depth_or_array_layers:1},mip_level_count:1,sample_count:1,
                    dimension:wgpu::TextureDimension::D2,format:renderer.config.format,
                    usage:wgpu::TextureUsages::RENDER_ATTACHMENT,view_formats:&[]});
                let mut encoder=renderer.device.create_command_encoder(&Default::default());
                renderer.encode(&mut encoder,&output.create_view(&Default::default()),&input,&mut Default::default());
                renderer.queue.submit(Some(encoder.finish()));
            }
            let path=std::env::temp_dir().join(format!("ferrum-gpu-stages-{}-{}-{stages}.png",
                std::process::id(),if choice.is_none(){"automatic"}else{"original"}));
            renderer.screenshot(&path,&input).unwrap();
            let screenshot=std::fs::read(&path).unwrap();std::fs::remove_file(path).unwrap();
            assert_eq!(renderer.gpu_timestamps_report(),before,"non-ticket paths cannot record");
            if let Some(expected)=&expected_screenshot {assert_eq!(&screenshot,expected);}
            else {expected_screenshot=Some(screenshot);}
            // All nine changing frames/copies were submitted before the first
            // host wait (screenshot); snapshots own separate readback storage.
            let images=tickets.into_iter().map(|ticket|ticket.finish(&renderer.device)).collect::<Vec<_>>();
            assert!(images[0].depth.iter().any(|&v|f32::from_bits(v)<1.),"actual world geometry must write depth");
            assert_ne!(images[4].color,images[5].color,"GUI must draw visible pixels on the menu");
            if let Some(expected)=&expected {assert_eq!(&images,expected);}else{expected=Some(images);}
            renderer.finish_gpu_timestamps().unwrap();
            let report=renderer.gpu_timestamps_report();
            assert_eq!(report["state"],"complete");assert_eq!(report["submitted"],9);
            assert_eq!(report["invalid_gpu_rows"],0);
            if stages {
                let rows=report["stages"]["rows"].as_array().unwrap();assert_eq!(rows.len(),9);
                for (index,row) in rows.iter().enumerate() {
                    assert_eq!(row["used_query_count"],[9,18,20,11,2,4,6,11,9][index]);
                    assert_eq!(row["eyes"],[1,2,2,1,0,0,0,1,1][index]);
                    assert_eq!(row["postfx"],matches!(index,2|3|6));
                    assert_eq!(row["gui"],matches!(index,1|2|5|6|7));
                    let points=row["points"].as_array().unwrap();
                    assert_eq!(points.len(),row["used_query_count"].as_u64().unwrap() as usize);
                    let raw=report["rows"][index]["raw_ticks"].as_array().unwrap();
                    assert_eq!(&points.first().unwrap()["raw_ticks"],&raw[0]);
                    assert_eq!(&points.last().unwrap()["raw_ticks"],&raw[1]);
                    for (query,point) in points.iter().enumerate() {assert_eq!(point["query_index"],query);}
                }
            }else{
                assert_eq!(report["query_count"],2);
                assert_eq!(report["readback_bytes"],2*1024*1024);
            }
        }
    }
}
