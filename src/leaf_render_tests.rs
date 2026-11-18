use super::*;
use crate::{
    gameplay::{Mode, Simulation},
    models::{OPAQUE_LEAF, SOLID, UNLIT},
    player::Player,
    renderer::{RenderInput, Renderer},
    settings::Settings,
    ui::{Frame, Gui, Screen},
    world::Chunk,
};
use std::sync::Arc;

fn world() -> World {
    World {
        chunks: [((0,0), Arc::new(Chunk::empty()))].into(), spawn: Vec3::ZERO,
        label: "Leaf support visibility".into(), save_path: Default::default(),
        edits: Default::default(), imported: false, light_revision: 0,
    }
}

#[test]
#[ignore = "uses installed original leaf and piston block models"]
fn real_leaf_models_match_graphics_opacity_and_keep_fancy_model_support_faces() {
    let assets = crate::validation::assets();
    let mut world = world();
    let p = IVec3::new(8,68,8);
    for fancy in [false,true] {
        let settings = Settings { fancy, ..Default::default() };
        for id in [18,161] {
            for meta in 0..16 {
                if id==161 && meta&3>1 { continue; }
                let state = id<<4 | meta;
                // Original leaves.json intentionally omits cullface. This is
                // the real baked path that synthetic cube tests did not cover.
                assert!(assets.block_models.custom_shape(state));
                let faces = assets.block_models.carried(state).unwrap();
                assert_eq!(faces.len(),6);
                assert!(faces.iter().all(|f|f.cull_face.is_none()));
                let mut mesh = crate::models::empty();
                special_block_mesh(&mut mesh,&world,p,state,assets,&settings);
                assert_eq!(mesh.vertices.len(),24);
                for (quad,face) in mesh.vertices.chunks_exact(4).zip(faces) {
                    for (v,expected) in quad.iter().zip(face.vertices) {
                        assert_eq!(v.layer & OPAQUE_LEAF !=0,!fancy,"state {state}, fancy {fancy}");
                        assert_eq!(v.layer & !OPAQUE_LEAF,face.layer);
                        assert_eq!(v.position,(p.as_vec3()+expected.position).to_array());
                        assert_eq!(v.uv,expected.uv);
                    }
                }
                world.set(p+IVec3::Y,state);
                // A normal piston top is a baked cullface, unlike the greedy
                // stone/grass mask. Fancy leaves must not remove that surface.
                let piston = 33<<4 | 1;
                let mut mesh = crate::models::empty();
                special_block_mesh(&mut mesh,&world,p,piston,assets,&settings);
                let top = mesh.vertices.chunks_exact(4).any(|q|
                    q.iter().all(|v|v.position[1]==p.y as f32+1.) &&
                    (Vec3::from_array(q[1].position)-Vec3::from_array(q[0].position))
                        .cross(Vec3::from_array(q[2].position)-Vec3::from_array(q[0].position)).y>0.);
                assert_eq!(top,fancy,"piston support, state {state}, fancy {fancy}");
                world.set(p+IVec3::Y,0);
            }
        }
    }
}

#[test]
#[ignore = "requires installed leaf texture and Vulkan framebuffer readback"]
fn placing_real_leaves_on_ground_never_reveals_the_cave_below() {
    let assets = crate::validation::assets();
    let resources = crate::validation::resources();
    let mut world = world();
    for x in 0..16 { for z in 0..16 {
        world.set(IVec3::new(x,68,z),1<<4);
        world.set(IVec3::new(x,65,z),35<<4|2);
    }}
    world.set(IVec3::new(8,69,8),18<<4|4);
    let marker = assets.texture(35<<4|2,1,true);
    let mut settings = Settings { fancy:false, mipmaps:0, clouds:0, render_distance:2,
        entity_shadows:false, ..Default::default() };
    let mut renderer = pollster::block_on(Renderer::new(None,assets,resources,&settings,None)).unwrap();
    let (width,height) = (320,320);
    renderer.resize(width,height);
    let mut player = Player::new(Vec3::new(8.5,71.,8.5));
    player.set_rotation_degrees(0.,90.);
    let mut sim = Simulation::new(Mode::Creative,189);
    sim.clock=6000;
    let gui = Gui { screen:Screen::Playing,hide_hud:true,..Default::default() };
    for _ in 0..40 {renderer.tick_lighting(&world,&player,&sim,settings.render_distance);}
    let path=std::env::temp_dir().join(format!("ferrum-leaf-ground-{}.png",std::process::id()));
    let mut frames=Vec::new();
    for (fancy,regress) in [(false,false),(true,false),(false,true)] {
        settings.fancy=fancy;
        renderer.apply_settings(&settings);
        let mut mesh=build_with_settings(&world,(0,0),assets,&settings);
        for v in &mut mesh.vertices {
            if v.layer & 65535 == marker {
                // Emissive magenta marks the hidden cave floor. It must never
                // appear through the intact stone support and placed leaves.
                v.layer |= SOLID | UNLIT;
                v.color=[255,0,255,255];
            }
            if regress {v.layer &= !OPAQUE_LEAF;}
        }
        renderer.upload((0,0),mesh);
        renderer.screenshot(&path,&RenderInput {selected_block:None,assets,resources,
            settings:&settings,gui:&gui,ui:&Frame::default(),world:Some(&world),
            player:Some(&player),simulation:Some(&sim),alpha:1.,time:0.}).unwrap();
        frames.push(image::open(&path).unwrap().to_rgb8());
    }
    std::fs::remove_file(path).unwrap();
    if let Some(out)=std::env::var_os("FERRUM_LEAF_CAPTURE") {
        let mut sheet=image::RgbImage::new(width*3,height);
        for (i,frame) in frames.iter().enumerate() {
            image::imageops::replace(&mut sheet,frame,i as i64*i64::from(width),0);
        }
        sheet.save(out).unwrap();
    }
    let revealed:Vec<_>=frames.iter().map(|frame|frame.pixels()
        .filter(|p|p[0]>200 && p[2]>200 && p[1]<70).count()).collect();
    assert_eq!(&revealed[..2],&[0,0],"fast/fancy leaves must both conceal the underground probe");
    assert!(revealed[2]>30,"the old missing-opacity flag must reproduce cave x-ray: {revealed:?}");
    let mode_difference=frames[0].pixels().zip(frames[1].pixels()).filter(|(a,b)|a!=b).count();
    assert!(mode_difference>30,"fast leaves must remain opaque; fancy leaves retain genuine foliage holes");
    eprintln!("Leaf cave probe pixels: fast {}, fancy {}, old broken path {}",revealed[0],revealed[1],revealed[2]);
}
