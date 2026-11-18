//! Actual Vulkan world-pass oracle: far sky color must never become an occluder.
//! Requires local1.8.9 assets and a device reproducing the legacy far-depth hazard.
use super::*;
use crate::{gameplay::Mode, ui::Screen};

struct Pixels { color:Vec<[u8;4]>, depth:Vec<u32> }
fn read_world(renderer:&mut Renderer,input:&RenderInput<'_>)->Pixels {
    let [width,height]=renderer.resolution();
    let extent=wgpu::Extent3d{width,height,depth_or_array_layers:1};
    // Test-owned readable depth attachment; the real Renderer::encode supplies
    // every render pass, pipeline, uniform, sky grid, celestial sprite and draw.
    let depth=renderer.device.create_texture(&wgpu::TextureDescriptor {
        label:Some("sky oracle readable depth"),size:extent,mip_level_count:1,sample_count:1,
        dimension:wgpu::TextureDimension::D2,format:DEPTH,
        usage:wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::COPY_SRC,view_formats:&[]});
    renderer.depth=depth.create_view(&Default::default());
    let color=renderer.device.create_texture(&wgpu::TextureDescriptor {
        label:Some("sky oracle color"),size:extent,mip_level_count:1,sample_count:1,
        dimension:wgpu::TextureDimension::D2,format:renderer.config.format,
        usage:wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::COPY_SRC,view_formats:&[]});
    let row=(width*4).div_ceil(256)*256;
    let plane=u64::from(row)*u64::from(height);
    let buffer=renderer.device.create_buffer(&wgpu::BufferDescriptor {
        label:Some("sky oracle color and depth readback"),size:plane*2,
        usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
    let mut encoder=renderer.device.create_command_encoder(&Default::default());
    renderer.encode(&mut encoder,&color.create_view(&Default::default()),input,&mut render_profile::Frame::default());
    for (texture,offset,aspect) in [(&color,0,wgpu::TextureAspect::All),(&depth,plane,wgpu::TextureAspect::DepthOnly)] {
        encoder.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo{texture,mip_level:0,origin:wgpu::Origin3d::ZERO,aspect},
            wgpu::TexelCopyBufferInfo{buffer:&buffer,layout:wgpu::TexelCopyBufferLayout{offset,bytes_per_row:Some(row),rows_per_image:Some(height)}},extent);
    }
    renderer.queue.submit(Some(encoder.finish()));
    let(tx,rx)=std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read,move|r|{let _=tx.send(r);});
    renderer.device.poll(wgpu::Maintain::Wait);rx.recv().unwrap().unwrap();
    let mapped=buffer.slice(..).get_mapped_range();
    let mut pixels=Pixels{color:Vec::new(),depth:Vec::new()};
    for y in 0..height as usize {
        let start=y*row as usize;
        pixels.color.extend(mapped[start..start+width as usize*4].chunks_exact(4)
            .map(|p|<[u8;4]>::try_from(p).unwrap()));
        let start=plane as usize+start;
        pixels.depth.extend(mapped[start..start+width as usize*4].chunks_exact(4)
            .map(|p|u32::from_le_bytes(p.try_into().unwrap())));
    }
    drop(mapped);buffer.unmap();pixels
}

fn body_direction(player:&Player,time:i64)->Vec3 {
    let a=crate::vanilla::lightmap::celestial_angle(time,1.,0)*std::f32::consts::TAU;
    let orbit=Mat4::from_rotation_y(-std::f32::consts::FRAC_PI_2)*Mat4::from_rotation_x(a);
    let body=orbit.transform_point3(Vec3::Y*if time>=12000 {-100.} else {100.});
    (body-Vec3::Y*player.eye_height()).normalize()
}
fn look_at_body(player:&mut Player,time:i64,offset:f32) {
    let dir=body_direction(player,time);
    let yaw=(-dir.x).atan2(dir.z).to_degrees()+offset;
    let pitch=(-dir.y.asin().to_degrees()+offset*0.45).clamp(-87.,87.);
    player.set_rotation_degrees(yaw,pitch);
}
fn foreground(player:&Player,world:&World,settings:&Settings,time:i64)->Mesh {
    let view=models::camera(player,world,settings,0,1.);
    let ray=body_direction(player,time);
    let center=view.eye+ray*(8./ray.dot(view.direction));
    let right=view.right;let up=right.cross(view.direction);
    // A true opaque terrain draw covers half the celestial disc. Its finite
    // depth must still block both the sky grid and the real sun/moon sprites.
    let points=[center-right*0.10-up*4.,center+right*4.-up*4.,
        center+right*4.+up*4.,center-right*0.10+up*4.];
    let mut mesh=models::empty();
    models::face(&mut mesh,points,[0.;4],models::SOLID|models::UNLIT,[35,95,25,255]);
    models::face(&mut mesh,[points[3],points[2],points[1],points[0]],[0.;4],models::SOLID|models::UNLIT,[35,95,25,255]);
    mesh.min=points.into_iter().fold(Vec3::splat(f32::INFINITY),Vec3::min);
    mesh.max=points.into_iter().fold(Vec3::splat(f32::NEG_INFINITY),Vec3::max);
    mesh
}

#[test]
#[ignore="requires local1.8.9 resources and Vulkan color/depth readback; run alone"]
fn sky_is_not_a_depth_occluder_and_foreground_still_hides_celestial_bodies() {
    let assets=crate::validation::assets();let resources=crate::validation::resources();
    let world=World {chunks:Default::default(),spawn:Vec3::new(0.,72.,0.),label:String::new(),
        save_path:Default::default(),edits:Default::default(),imported:false,light_revision:0};
    let mut settings=Settings {render_distance:16,clouds:0,entity_shadows:false,..Default::default()};
    let gui=Gui{screen:Screen::Playing,hide_hud:true,..Default::default()};let ui=Frame::default();
    let mut sim=Simulation::new(Mode::Creative,189);
    let mut player=Player::new(world.spawn);
    let mut renderer=pollster::block_on(Renderer::new_with_arena(None,assets,resources,&settings,None,Some(crate::renderer::NearArenaMode::Original))).unwrap();
    assert!(renderer.postfx.is_none());
    let normal=[renderer.pipeline.clone(),renderer.red.clone(),renderer.cyan.clone()];
    let depthless=[renderer.transparent.clone(),renderer.transparent_red.clone(),renderer.transparent_cyan.clone()];
    let sky_count=renderer.sky.count;
    let mut legacy_depth_pixels=0usize;let mut legacy_color_pixels=0usize;
    let mut bright_pixels=0usize;let mut occluded_bright_pixels=0usize;
    // Odd/even viewport dimensions, shallow/near-zenith sun, moon, both eye
    // masks and near/far projection planes deliberately vary interpolation.
    let cases=[
        (320,180,4,2000,7.,false),(641,359,16,5000,-9.,false),
        (960,540,128,9000,11.,false),(641,359,128,2000,-12.,false),
        (320,180,16,6000,8.,false),(641,359,4,18000,-9.,false),
        (641,359,4,2000,7.,true),(320,180,16,5000,-9.,true),
        (960,540,128,9000,11.,true),(641,359,128,18000,-8.,true),
        (641,359,128,6000,5.,false),(960,540,16,2500,-13.,false),
        // Fullscreen-size rasters reproduce the reported stripes on NVIDIA:
        // the old sky pass writes depths just below1 and hides the sun.
        (1920,1080,128,1000,11.,false),(1920,1001,128,1000,11.,false),
        (1920,1031,128,1000,11.,false),
    ];
    let case_count=cases.len();
    for (case,(width,height,radius,time,offset,anaglyph)) in cases.into_iter().enumerate() {
        renderer.resize(width,height);settings.render_distance=radius;settings.anaglyph=anaglyph;
        sim.clock=time;look_at_body(&mut player,time as i64,offset);
        for _ in 0..30 {renderer.tick_lighting(&world,&player,&sim,radius);}
        let input=RenderInput{selected_block:None,assets,resources,settings:&settings,gui:&gui,ui:&ui,
            world:Some(&world),player:Some(&player),simulation:Some(&sim),alpha:1.,time:0.};
        let fixed=read_world(&mut renderer,&input);
        assert_eq!(renderer.dynamic.count,0,"fixture must not draw player/entities");
        assert_eq!(renderer.cloud.count,0);assert_eq!(renderer.shadows.count,0);
        assert!(fixed.depth.iter().all(|&v|v==1f32.to_bits()),
            "case{case}: actual world sky wrote depth and can hide celestial fragments");
        // Original full-moon phase0 peaks at RGB(217,228,255): unlike
        // the daytime sun, its additive night background does not lift all
        // channels above220. Keep the test on its bright18-texel core.
        let threshold=if time>=12000 {200}else{220};
        let bright=|p:&[u8;4]|p[..3].iter().all(|v|*v>threshold);
        let visible_body=fixed.color.iter().filter(|p|bright(p)).count();
        assert!(visible_body>16,"case{case}: sun/moon must actually be visible, got{visible_body}");
        bright_pixels+=visible_body;
        // Independent reference: both possible sky pipeline choices are
        // depthless, without branching on the production show_sky implementation.
        // This empty scene has no terrain/dynamics whose depth could be changed.
        renderer.pipeline=depthless[0].clone();renderer.red=depthless[1].clone();renderer.cyan=depthless[2].clone();
        let reference=read_world(&mut renderer,&input);
        assert_eq!(fixed.color,reference.color,"case{case}: sky/celestial pixels differ from the independent depthless reference");
        assert_eq!(fixed.depth,reference.depth);
        renderer.pipeline=normal[0].clone();renderer.red=normal[1].clone();renderer.cyan=normal[2].clone();
        // Recreate the old sky depth-write hazard with original per-eye masks.
        // No translucent terrain/dynamics exists, so only the sky draw changes.
        renderer.transparent=normal[0].clone();renderer.transparent_red=normal[1].clone();renderer.transparent_cyan=normal[2].clone();
        let legacy=read_world(&mut renderer,&input);
        legacy_depth_pixels+=legacy.depth.iter().filter(|&&v|v!=1f32.to_bits()).count();
        legacy_color_pixels+=legacy.color.iter().zip(&fixed.color).filter(|(a,b)|a!=b).count();
        renderer.transparent=depthless[0].clone();renderer.transparent_red=depthless[1].clone();renderer.transparent_cyan=depthless[2].clone();
        // Occlusion pixels use a single eye; stereo sky masks were covered above.
        // Removing only sky geometry is an independent depth/foreground oracle:
        // celestial draws and all genuine opaque terrain stay in the real pass.
        if !anaglyph {
            renderer.upload((0,0),foreground(&player,&world,&settings,time as i64));
            let covered=read_world(&mut renderer,&input);
            assert!(renderer.visible_chunks>0);
            renderer.sky.count=0;let no_sky=read_world(&mut renderer,&input);renderer.sky.count=sky_count;
            assert_eq!(covered.depth,no_sky.depth,"case{case}: sky changed existing foreground depth");
            let mut foreground_pixels=0usize;let mut hidden=0usize;
            for i in 0..covered.depth.len() {
                if f32::from_bits(covered.depth[i])<0.9999 {
                    foreground_pixels+=1;
                    assert_eq!(covered.color[i],no_sky.color[i],"case{case}: sky/celestial leaked through foreground at pixel{i}");
                    if bright(&fixed.color[i])&&!bright(&covered.color[i]) {hidden+=1;}
                }else{assert_eq!(covered.depth[i],1f32.to_bits());}
            }
            assert!(foreground_pixels>500,"case{case}: foreground geometry was not meaningfully rendered");
            assert!(hidden>8,"case{case}: foreground must actually cover the celestial disc, got{hidden}");
            occluded_bright_pixels+=hidden;renderer.remove((0,0));
        }
    }
    eprintln!("Sky Vulkan oracle:{case_count} cases,{bright_pixels} bright-body pixels,{occluded_bright_pixels} occluded; legacy{legacy_depth_pixels} polluted depth/{legacy_color_pixels} changed color pixels");
    assert!(legacy_depth_pixels>32,"legacy path did not reproduce the depth hazard on this adapter; oracle is inconclusive");
    assert!(legacy_color_pixels>8,"legacy depth writes must visibly corrupt celestial/sky pixels");
}

