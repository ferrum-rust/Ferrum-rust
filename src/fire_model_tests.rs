use super::*;
use crate::{
    settings::Settings,
    world::{Chunk, World},
};
use glam::IVec3;
use std::sync::Arc;

fn world() -> World {
    World {
        chunks: [
            ((-1, -1), Arc::new(Chunk::empty())),
            ((0, 0), Arc::new(Chunk::empty())),
        ]
        .into(),
        spawn: Vec3::ZERO,
        label: "Fire model fixture".into(),
        save_path: Default::default(),
        edits: Default::default(),
        imported: false,
        light_revision: 0,
    }
}

#[test]
#[ignore = "uses the installed 1.8.9 fire textures"]
fn all_192_fire_variants_match_original_face_bakery_vertices_and_uvs() {
    type Face = (String, usize, u8, [[u32; 5]; 4]);
    let expected: Vec<(u8, Vec<Face>)> = serde_json::from_reader(flate2::read::GzDecoder::new(
        &include_bytes!("../tests/fixtures/vanilla189-fire-models.json.gz")[..],
    ))
    .unwrap();
    let assets = crate::validation::assets();
    assert_eq!(expected.len(), 192);
    for (state, expected) in expected {
        let quads = &assets.block_models.fire[&state];
        assert_eq!(quads.len(), expected.len(), "fire state {state}");
        for (quad, (texture, side, shade, vertices)) in quads.iter().zip(expected) {
            assert_eq!(quad.layer, assets.materials[&texture]);
            assert_eq!(quad.facing, side);
            assert_eq!(quad.shade, shade);
            for (actual, expected) in quad.vertices.iter().zip(vertices) {
                assert_eq!(
                    [
                        actual.position.x,
                        actual.position.y,
                        actual.position.z,
                        actual.uv[0],
                        actual.uv[1]
                    ]
                    .map(f32::to_bits),
                    expected,
                    "fire state {state} face {side}"
                );
            }
        }
    }
}

#[test]
#[ignore = "uses the installed fire atlas"]
fn placed_fire_uses_floor_side_and_ceiling_models_and_keeps_animation_layers() {
    let assets = crate::validation::assets();
    let mut world = world();
    let settings = Settings::default();
    let p = IVec3::new(6, 64, 6);
    world.set(p - IVec3::Y, 1 << 4);
    let layers = [
        assets.materials["fire_layer_0"],
        assets.materials["fire_layer_1"],
    ];
    for age in 0..16 {
        world.set(p, 51 << 4 | age);
        let mesh = crate::mesh::build_with_settings(&world, (0, 0), assets, &settings);
        let fire: Vec<_> = mesh
            .vertices
            .iter()
            .filter(|v| layers.contains(&(v.layer & 65535)))
            .collect();
        assert_eq!(fire.len(), 32, "eight original floor planes, age {age}");
        assert!(fire.iter().any(|v| v.position[1] > p.y as f32 + 1.));
        assert!(fire
            .iter()
            .all(|v| v.color == [255; 4] && v.uv.iter().all(|u| (0.0..=1.0).contains(u))));
    }
    world.set(p - IVec3::Y, 0);
    world.set(p + IVec3::NEG_Z, 5 << 4);
    assert!(std::ptr::eq(
        assets.block_models.fire(&world, p).unwrap(),
        assets.block_models.fire[&1].as_slice()
    ));
    world.set(p + IVec3::Y, 5 << 4);
    assert!(std::ptr::eq(
        assets.block_models.fire(&world, p).unwrap(),
        assets.block_models.fire[&33].as_slice()
    ));
    world.set(p - IVec3::Y, 5 << 4);
    assert!(std::ptr::eq(
        assets.block_models.fire(&world, p).unwrap(),
        assets.block_models.fire[&0].as_slice()
    ));
    for (position, index) in [
        (IVec3::new(-1, 64, -1), 32),
        (IVec3::new(-2, 65, -1), 160),
        (IVec3::new(-3, 65, -1), 208),
    ] {
        world.set(position + IVec3::Y, 5 << 4);
        assert!(std::ptr::eq(
            assets.block_models.fire(&world, position).unwrap(),
            assets.block_models.fire[&index].as_slice()
        ));
        world.set(position + IVec3::Y, 0);
    }
    // Both original sprite sheets remain in the GPU animation range.
    assert!(layers
        .iter()
        .all(|&layer| layer > 0 && layer <= assets.animations.len() as u32));
}

#[test]
#[ignore = "uses installed original resources and a Vulkan device"]
fn vulkan_renders_placed_floor_and_wall_fire() {
    use crate::{
        gameplay::{Mode, Simulation},
        player::Player,
        renderer::{RenderInput, Renderer},
        ui::{Frame, Gui, Screen},
    };
    let assets = crate::validation::assets();
    let resources = crate::validation::resources();
    let mut world = world();
    for x in 0..16 {
        for z in 0..16 {
            world.set(IVec3::new(x, 63, z), 2 << 4);
        }
    }
    world.set(IVec3::new(10, 65, 9), 5 << 4);
    let settings = Settings {
        clouds: 0,
        render_distance: 2,
        ..Default::default()
    };
    let mut renderer =
        pollster::block_on(Renderer::new(None, assets, resources, &settings, None)).unwrap();
    renderer.resize(640, 400);
    let mut sim = Simulation::new(Mode::Creative, 189);
    sim.clock = 6000;
    let mut player = Player::new(Vec3::new(8., 64.3, 3.));
    player.set_rotation_degrees(0., 8.);
    let gui = Gui {
        screen: Screen::Playing,
        hide_hud: true,
        ..Default::default()
    };
    let path = std::env::temp_dir().join(format!("ferrum-placed-fire-{}.png", std::process::id()));
    let mut baseline = None;
    for lit in [false, true] {
        if lit {
            world.set(IVec3::new(6, 64, 8), 51 << 4);
            world.set(IVec3::new(10, 65, 8), 51 << 4);
        }
        renderer.upload(
            (0, 0),
            crate::mesh::build_with_settings(&world, (0, 0), assets, &settings),
        );
        for _ in 0..40 {
            renderer.tick_lighting(&world, &player, &sim, 2);
        }
        renderer
            .screenshot(
                &path,
                &RenderInput {
                    selected_block: None,
                    assets,
                    resources,
                    settings: &settings,
                    gui: &gui,
                    ui: &Frame::default(),
                    world: Some(&world),
                    player: Some(&player),
                    simulation: Some(&sim),
                    alpha: 1.,
                    time: 0.,
                },
            )
            .unwrap();
        let image = image::open(&path).unwrap().to_rgb8();
        if let Some(baseline) = &baseline {
            let baseline: &image::RgbImage = baseline;
            let fire_pixels = image
                .pixels()
                .zip(baseline.pixels())
                .filter(|(a, b)| a != b && a[0] > 160 && a[1] > 40 && a[2] < 120)
                .count();
            assert!(
                fire_pixels > 200,
                "placed fire drew only {fire_pixels} flame pixels"
            );
            if let Some(out) = std::env::var_os("FERRUM_FIRE_CAPTURE") {
                image.save(out).unwrap();
            }
        } else {
            baseline = Some(image);
        }
    }
    std::fs::remove_file(path).unwrap();
}
