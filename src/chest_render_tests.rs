use super::*;
use crate::{models, world::Chunk};
use std::sync::Arc;

fn world() -> World {
    World {
        chunks: [
            ((0, 0), Arc::new(Chunk::empty())),
            ((1, 0), Arc::new(Chunk::empty())),
        ]
        .into(),
        spawn: Vec3::new(9., 65., 12.),
        label: "Chest models".into(),
        save_path: Default::default(),
        edits: Default::default(),
        imported: false,
        light_revision: 0,
    }
}

#[test]
fn both_original_models_match_every_vertex_and_uv_in_all_four_directions() {
    let reference: Vec<Vec<[f32; 5]>> = serde_json::from_str(include_str!(
        "../tests/fixtures/vanilla189-chest-models.json"
    ))
    .unwrap();
    let mut world = world();
    let p = IVec3::new(15, 64, 8);
    for facing in 2..6 {
        for large in [false, true] {
            let step = if facing <= 3 { IVec3::X } else { IVec3::Z };
            world.set(p, 54 << 4 | facing);
            if large {
                world.set(p + step, 54 << 4 | facing);
            }
            let chest = Chest::at(&world, p).unwrap();
            if large {
                let other = Chest::at(&world, p + step).unwrap();
                assert_eq!(other.anchor, p);
                assert_eq!(other.second, Some(p + step));
            }
            let mut mesh = models::empty();
            chest.draw(
                &mut mesh,
                0.,
                Texture {
                    layer: 81,
                    origin: [0.; 2],
                    scale: [if large { 1. / 128. } else { 1. / 64. }, 1. / 64.],
                },
                [0; 4],
            );
            let expected = &reference[usize::from(large)];
            assert_eq!(mesh.vertices.len(), 72);
            assert_eq!(expected.len(), mesh.vertices.len());
            let length = if large { 2. } else { 1. };
            for (v, &[x, y, z, u, t]) in mesh.vertices.iter().zip(expected) {
                let point = match facing {
                    2 => Vec3::new(length - x, y, 1. - z),
                    3 => Vec3::new(x, y, z),
                    4 => Vec3::new(1. - z, y, x),
                    _ => Vec3::new(z, y, length - x),
                } + p.as_vec3();
                assert!(Vec3::from_array(v.position).abs_diff_eq(point, 0.00001));
                assert_eq!(v.uv, [u, t]);
                assert_eq!(v.layer, 81);
            }
            world.set(p + step, 0);
        }
    }
    world.set(p, 146 << 4 | 3);
    world.set(p + IVec3::X, 54 << 4 | 3);
    assert!(Chest::at(&world, p).unwrap().second.is_none());
    world.set(p + IVec3::X, 146 << 4 | 3);
    let trapped = Chest::at(&world, p).unwrap();
    assert_eq!(
        trapped.texture(false),
        "textures/entity/chest/trapped_double.png"
    );
    assert_eq!(
        trapped.texture(true),
        "textures/entity/chest/christmas_double.png"
    );
    world.set(p, 130 << 4 | 3);
    world.set(p + IVec3::X, 130 << 4 | 3);
    assert!(Chest::at(&world, p).unwrap().second.is_none());
    assert_eq!(
        Chest::at(&world, p).unwrap().texture(true),
        "textures/entity/chest/ender.png"
    );
}

#[test]
fn opening_either_half_shares_the_hinge_and_closes_over_ten_ticks() {
    let mut world = world();
    let p = IVec3::new(15, 64, 8);
    let q = p + IVec3::X;
    for pos in [p, q] {
        world.set(pos, 54 << 4 | 2);
    }
    let mut lids = Lids::default();
    for _ in 0..5 {
        lids.tick(&world, Some(q));
    }
    assert!((lids.angle(&world, p, 1.) - 0.875).abs() < 1e-6);
    lids.server_users(q, 1);
    for _ in 0..10 {
        lids.tick(&world, None);
    }
    assert_eq!(lids.angle(&world, p, 1.), 1.);
    lids.server_users(q, 0);
    for _ in 0..10 {
        lids.tick(&world, None);
    }
    assert_eq!(lids.angle(&world, q, 1.), 0.);
    lids.tick(&world, None);
    assert!(lids.state.is_empty());
}

#[test]
#[ignore = "requires a Vulkan adapter and installed 1.8.9 resources"]
fn vulkan_single_and_double_chests_join_and_open() {
    use crate::{
        gameplay::{Mode, Simulation},
        player::Player,
        renderer::{RenderInput, Renderer},
        settings::Settings,
        ui::{Frame, Gui, Screen},
    };
    let resources = crate::validation::resources();
    let assets = crate::validation::assets();
    let settings = Settings {
        clouds: 0,
        bobbing: false,
        render_distance: 4,
        ..Default::default()
    };
    let mut renderer =
        pollster::block_on(Renderer::new(None, assets, resources, &settings, None)).unwrap();
    let (width, height) = (680, 370);
    renderer.resize(width, height);
    let mut world = world();
    let chunk = Arc::make_mut(world.chunks.get_mut(&(0, 0)).unwrap());
    for x in 0..16 {
        for z in 0..16 {
            chunk.set(x, 63, z, 2 << 4);
        }
    }
    let single = IVec3::new(10, 64, 8);
    let double = IVec3::new(7, 64, 8);
    for p in [single, double, double + IVec3::X] {
        world.set(p, 54 << 4 | 3);
    }
    let mesh = crate::mesh::build_with_settings(&world, (0, 0), assets, &settings);
    assert_eq!(mesh.sections.as_ref().unwrap().chests.len(), 3);
    renderer.upload((0, 0), mesh);
    let at = glam::DVec3::new(10.1, 64.25, 11.6);
    let mut player = Player::new(at.as_vec3());
    let dir =
        (glam::DVec3::new(9., 64.56, 8.5) - at - glam::DVec3::Y * f64::from(player.eye_height()))
            .normalize();
    player.set_pose_f64(
        at.to_array(),
        (-dir.x).atan2(dir.z).to_degrees() as f32,
        -dir.y.asin().to_degrees() as f32,
        true,
    );
    let mut sim = Simulation::new(Mode::Survival, 189);
    sim.clock = 6000;
    let gui = Gui {
        screen: Screen::Playing,
        hide_hud: true,
        ..Default::default()
    };
    let frame = Frame::default();
    let mut strip = image::RgbImage::new(width * 2, height);
    let temp = std::env::temp_dir().join(format!("ferrum-chests-{}.png", std::process::id()));
    let mut frames = Vec::new();
    for i in 0..2 {
        if i == 1 {
            for _ in 0..10 {
                sim.server_scene.chest_lids.tick(&world, Some(double));
            }
        }
        renderer
            .screenshot(
                &temp,
                &RenderInput {
                    assets,
                    resources,
                    settings: &settings,
                    gui: &gui,
                    ui: &frame,
                    selected_block: None,
                    world: Some(&world),
                    player: Some(&player),
                    simulation: Some(&sim),
                    alpha: 1.,
                    time: 4.,
                },
            )
            .unwrap();
        let image = image::open(&temp).unwrap().to_rgb8();
        image::imageops::replace(&mut strip, &image, i64::from(width) * i, 0);
        frames.push(image);
    }
    let changed = frames[0]
        .pixels()
        .zip(frames[1].pixels())
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        changed > 300 && changed < (width * height / 4) as usize,
        "lid changed {changed} pixels"
    );
    if let Some(path) = std::env::var_os("FERRUM_CHEST_CAPTURE") {
        strip.save(path).unwrap();
    }
    std::fs::remove_file(temp).unwrap();
}
