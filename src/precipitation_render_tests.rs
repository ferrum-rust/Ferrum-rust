use super::*;
use crate::{
    gameplay::{Mode, Simulation},
    player::Player,
    renderer::{RenderInput, Renderer},
    settings::Settings,
    ui::{Frame, Gui, Screen},
    world::{Chunk, World},
};
use std::sync::Arc;

fn world(id: u8) -> World {
    let mut chunk = Chunk::empty();
    chunk.biomes.fill(id);
    for x in 0..16 {
        for z in 0..16 {
            chunk.set(x, 63, z, 2 << 4);
        }
    }
    World {
        chunks: (-1..=1)
            .flat_map(|x| (-1..=1).map(move |z| (x, z)))
            .map(|pos| (pos, Arc::new(chunk.clone())))
            .collect(),
        spawn: Vec3::new(8., 64., 4.),
        label: "Precipitation fixture".into(),
        save_path: Default::default(),
        edits: Default::default(),
        imported: false,
        light_revision: 0,
    }
}

#[test]
#[ignore = "requires installed original rain and snow resources and Vulkan"]
fn original_precipitation_textures_fall_only_in_eligible_exposed_biomes() {
    let assets = crate::validation::assets();
    let resources = crate::validation::resources();
    let settings = Settings {
        clouds: 0,
        render_distance: 3,
        ..Default::default()
    };
    let mut sim = Simulation::new(Mode::Creative, 189);
    sim.clock = 6000;
    sim.total_ticks = 123;
    sim.weather.game_state(7, 1.);
    let mut player = Player::new(Vec3::new(8., 64., 4.));
    player.set_rotation_degrees(0., 15.);
    let mut renderer =
        pollster::block_on(Renderer::new(None, assets, resources, &settings, None)).unwrap();
    let (width, height) = (384, 256);
    renderer.resize(width, height);
    let gui = Gui {
        screen: Screen::Playing,
        hide_hud: true,
        ..Default::default()
    };
    let path =
        std::env::temp_dir().join(format!("ferrum-precipitation-{}.png", std::process::id()));
    let mut sheet = image::RgbImage::new(width * 3, height);
    for (index, (biome_id, texture)) in [
        (12, Some("textures/environment/snow.png")),
        (4, Some("textures/environment/rain.png")),
        (2, None),
    ]
    .into_iter()
    .enumerate()
    {
        let mut world = world(biome_id);
        let view = camera(&player, &world, &settings, 0, 0.5);
        let frustum = Frustum::new(
            Mat4::perspective_rh(70_f32.to_radians(), 1.5, 0.05, 128.) * view.view_at(view.eye),
        );
        let model = |world: &World, sim: &Simulation| {
            let c = ModelContext {
                sim,
                player: &player,
                world,
                assets,
                resources,
                settings: &settings,
                frustum: &frustum,
                viewport_height: height,
                eye: view.eye,
                alpha: 0.5,
                perspective: 0,
                render_hand: false,
                hitboxes: false,
            };
            let mut mesh = empty();
            draw(&mut mesh, &c);
            mesh
        };
        let mesh = model(&world, &sim);
        if let Some(texture) = texture {
            let sprite = resources.sprite(texture);
            assert_ne!(sprite.layer, 0);
            assert!(mesh.vertices.len() > 100);
            assert!(mesh
                .vertices
                .iter()
                .all(|v| v.layer & 65535 == sprite.layer && v.position[1] >= 64.));
            assert!(mesh
                .vertices
                .iter()
                .all(|v| v.position.iter().all(|n| n.is_finite())));
            let mut advanced = sim.clone();
            advanced.total_ticks += 1;
            assert!(mesh
                .vertices
                .iter()
                .zip(model(&world, &advanced).vertices)
                .any(|(a, b)| a.uv != b.uv));
            let mut dry = sim.clone();
            dry.weather.game_state(7, 0.);
            assert!(model(&world, &dry).vertices.is_empty());
            for chunk in world.chunks.values_mut() {
                let chunk = Arc::make_mut(chunk);
                for x in 0..16 {
                    for z in 0..16 {
                        chunk.set(x, 90, z, 1 << 4);
                    }
                }
            }
            assert!(
                model(&world, &sim).vertices.is_empty(),
                "roof must hide precipitation below it"
            );
            world = world_for_capture(biome_id);
        } else {
            assert!(mesh.vertices.is_empty(), "desert must stay dry");
        }
        for &pos in world.chunks.keys() {
            renderer.upload(
                pos,
                crate::mesh::build_with_settings(&world, pos, assets, &settings),
            );
        }
        for _ in 0..40 {
            renderer.tick_lighting(&world, &player, &sim, 3);
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
                    alpha: 0.5,
                    time: 0.,
                },
            )
            .unwrap();
        let pixels = image::open(&path).unwrap().to_rgb8();
        if biome_id == 12 {
            let snow_pixels = pixels
                .enumerate_pixels()
                .filter(|(_, y, p)| *y > height / 2 && p[0] > 150 && p[1] > 150 && p[2] > 150)
                .count();
            assert!(
                snow_pixels > 30,
                "Vulkan must draw visible snow flakes, got {snow_pixels}"
            );
        }
        image::imageops::replace(&mut sheet, &pixels, index as i64 * i64::from(width), 0);
    }
    if let Some(out) = std::env::var_os("FERRUM_PRECIPITATION_CAPTURE") {
        sheet.save(out).unwrap();
    }
    std::fs::remove_file(path).unwrap();
}

fn world_for_capture(id: u8) -> World {
    world(id)
}
