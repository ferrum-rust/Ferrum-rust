use super::*;

#[test]
fn original_map_allocation_centers_pixels_shading_and_markers_match() {
    let data: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/vanilla189-maps.json")).unwrap();
    for row in data["cases"].as_array().unwrap() {
        let scenario = row["scenario"].as_i64().unwrap() as i32;
        let position: [f64; 3] = serde_json::from_value(row["position"].clone()).unwrap();
        let dimension = if row["nether"].as_bool().unwrap() {
            -1
        } else {
            0
        };
        let mut map = Map::new(position, 0, dimension);
        assert_eq!(serde_json::json!(map.center), row["center"]);
        let mut world = super::super::tests::flat_world();
        world.chunks.clear();
        for cx in map.center[0] / 16 - 5..map.center[0] / 16 + 5 {
            for cz in map.center[1] / 16 - 5..map.center[1] / 16 + 5 {
                let mut chunk = crate::world::Chunk::empty();
                for x in 0..16 {
                    for z in 0..16 {
                        let (wx, wz) = (cx * 16 + x, cz * 16 + z);
                        let height = if scenario == 1 {
                            64 + (wx.div_euclid(8) + wz.div_euclid(8)).rem_euclid(5)
                        } else {
                            64
                        };
                        for y in 0..=height {
                            let id = if scenario == 2 && y >= 60 {
                                9
                            } else if y == height {
                                if scenario == 1 && wx.rem_euclid(16) < 4 {
                                    12
                                } else {
                                    2
                                }
                            } else {
                                1
                            };
                            chunk.set(x, y, z, id << 4);
                        }
                    }
                }
                world.chunks.insert((cx, cz), std::sync::Arc::new(chunk));
            }
        }
        for (tick, expected) in row["hashes"].as_array().unwrap().iter().enumerate() {
            map.explore(&world, position, dimension);
            let hash = map.colors.iter().fold(0xcbf29ce484222325u64, |hash, &b| {
                (hash ^ u64::from(b)).wrapping_mul(0x100000001b3)
            });
            assert_eq!(
                format!("{hash:x}"),
                expected.as_str().unwrap(),
                "scenario{scenario} dimension{dimension} pos{position:?} tick{tick}"
            );
        }
        let mut player = Player::new(Vec3::ZERO);
        player.set_pose_f64(position, 37.25, 0., false);
        let expected = row["icons"]
            .as_array()
            .unwrap()
            .iter()
            .map(|icon| {
                [
                    (icon[0].as_i64().unwrap() as u8) << 4 | (icon[3].as_i64().unwrap() as u8 & 15),
                    icon[1].as_i64().unwrap() as u8,
                    icon[2].as_i64().unwrap() as u8,
                ]
            })
            .collect::<Vec<_>>();
        assert_eq!(map.marker(&player, dimension, 6000), expected);
    }
}

#[test]
#[ignore = "uses installed resources for normal held-item input"]
fn right_click_empty_map_explores_displays_and_preserves_unique_maps_after_reload() {
    let resources = crate::validation::resources();
    for mode in [Mode::Survival, Mode::Creative] {
        let mut sim = Simulation::new(mode, 189);
        sim.mob_spawning = false;
        sim.inventory.slots[0] = Some(Stack::new(395, 1));
        let mut world = super::super::tests::flat_world();
        let mut player = Player::new(Vec3::new(8.5, 1., 8.5));
        sim.tick(
            &mut world,
            &mut player,
            resources,
            &Settings::default(),
            false,
            true,
        );
        assert_eq!(sim.inventory.held(), Some(Stack::new(358, 1)));
        assert_eq!(sim.maps.data.len(), 1);
        let map = &sim.maps.data[&0];
        assert_eq!((map.center, map.scale, map.dimension), ([0, 0], 0, 0));
        assert!(map.colors.iter().any(|&color| color != 0));
        assert!(sim
            .server_scene
            .map
            .as_ref()
            .is_some_and(|shown| shown.id == 0 && shown.rgba.len() == 65536));
        let pixels = map.colors.clone();
        let mut sim: Simulation =
            serde_json::from_slice(&serde_json::to_vec(&sim).unwrap()).unwrap();
        assert_eq!(sim.maps.data[&0].colors, pixels);
        sim.tick(
            &mut world,
            &mut player,
            resources,
            &Settings::default(),
            false,
            false,
        );
        assert!(sim.server_scene.map.is_some());
        sim.inventory.slots[0] = Some(Stack::new(395, 2));
        for _ in 0..4 {
            sim.tick(
                &mut world,
                &mut player,
                resources,
                &Settings::default(),
                false,
                false,
            );
        }
        sim.tick(
            &mut world,
            &mut player,
            resources,
            &Settings::default(),
            false,
            true,
        );
        assert_eq!(
            sim.inventory.held().unwrap().count,
            if mode == Mode::Creative { 2 } else { 1 }
        );
        assert!(sim
            .inventory
            .slots
            .iter()
            .flatten()
            .any(|s| s.id == 358 && s.meta == 1));
        assert_eq!(sim.maps.data.len(), 2);
        assert!(sim.server_scene.map.is_none());
    }
}

#[test]
fn map_creation_with_full_inventory_drops_a_map_and_retains_old_pixels() {
    let mut sim = Simulation::new(Mode::Survival, 189);
    for slot in &mut sim.inventory.slots[..36] {
        *slot = Some(Stack::new(1, 64));
    }
    sim.inventory.slots[0] = Some(Stack::new(395, 2));
    let player = Player::new(Vec3::new(-64.1, 70., 129.9));
    sim.use_empty_map(&player, &sim.inventory.held().unwrap());
    assert_eq!(sim.inventory.held().unwrap().count, 1);
    assert_eq!(sim.drops.len(), 1);
    assert_eq!(sim.drops[0].stack.id, 358);
    assert_eq!(sim.drops[0].pickup_delay, 40);
    assert_eq!(sim.maps.data[&0].center, [-128, 128]);
}

#[test]
#[ignore = "requires installed resources and Vulkan framebuffer readback"]
fn used_empty_map_renders_explored_pixels_in_the_players_hands() {
    use crate::{
        renderer::{RenderInput, Renderer},
        ui::{Frame, Gui, Screen},
    };
    let assets = crate::validation::assets();
    let resources = crate::validation::resources();
    let mut world = super::super::tests::flat_world();
    world.chunks.clear();
    for cx in -5..=4 {
        for cz in -5..=4 {
            let mut chunk = crate::world::Chunk::empty();
            for x in 0..16 {
                for z in 0..16 {
                    let wx = cx * 16 + x;
                    chunk.set(
                        x,
                        0,
                        z,
                        if (-3..=3).contains(&wx) {
                            9 << 4
                        } else if (-5..=5).contains(&wx) {
                            12 << 4
                        } else {
                            2 << 4
                        },
                    );
                }
            }
            world.chunks.insert((cx, cz), std::sync::Arc::new(chunk));
        }
    }
    let settings = Settings {
        render_distance: 3,
        clouds: 0,
        ..Default::default()
    };
    let mut sim = Simulation::new(Mode::Creative, 189);
    sim.clock = 6000;
    sim.mob_spawning = false;
    sim.inventory.slots[0] = Some(Stack::new(395, 1));
    let mut player = Player::new(Vec3::new(8.5, 1., 8.5));
    player.set_rotation_degrees(0., 30.);
    for tick in 0..16 {
        sim.tick(
            &mut world,
            &mut player,
            resources,
            &settings,
            false,
            tick == 0,
        );
    }
    assert_eq!(sim.inventory.held().unwrap().id, 358);
    assert!(
        sim.maps.data[&0]
            .colors
            .iter()
            .filter(|&&color| color >= 4)
            .count()
            > 15000
    );
    let mut renderer =
        pollster::block_on(Renderer::new(None, assets, resources, &settings, None)).unwrap();
    renderer.resize(640, 480);
    for &p in world.chunks.keys() {
        renderer.upload(
            p,
            crate::mesh::build_with_settings(&world, p, assets, &settings),
        );
    }
    for _ in 0..40 {
        renderer.tick_lighting(&world, &player, &sim, 3);
    }
    let gui = Gui {
        screen: Screen::Playing,
        ..Default::default()
    };
    let path = std::env::temp_dir().join(format!("ferrum-used-map-{}.png", std::process::id()));
    let mut capture = |sim: &Simulation| {
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
                    simulation: Some(sim),
                    alpha: 1.,
                    time: 0.,
                },
            )
            .unwrap();
        image::open(&path).unwrap().to_rgb8()
    };
    let mut blank = sim.clone();
    blank.server_scene.map = None;
    let before = capture(&blank);
    let after = capture(&sim);
    let changed = before
        .pixels()
        .zip(after.pixels())
        .filter(|(a, b)| a != b)
        .count();
    assert!(changed > 5000, "filled map changed only {changed} pixels");
    if let Some(path) = std::env::var_os("FERRUM_MAP_CAPTURE") {
        after.save(path).unwrap();
    }
    std::fs::remove_file(path).unwrap();
}
