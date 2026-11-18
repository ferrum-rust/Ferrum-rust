//! Local-asset integration checks. These tests never write to .minecraft.
use crate::{
    assets::Assets,
    gameplay::{self, Mob, MobKind, Mode, Simulation},
    inventory::Stack,
    mesh::Frustum,
    models,
    player::Player,
    resources::Resources,
    settings::Settings,
    ui::{self, Action, Gui, Screen, Stats},
    world::{Chunk, World},
};
use glam::{IVec3, Mat4, Vec3};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, OnceLock},
};
pub(crate) fn resources() -> &'static Resources {
    static R: OnceLock<Resources> = OnceLock::new();
    R.get_or_init(|| Resources::load().unwrap())
}
#[test]
#[ignore = "requires fixtures generated from the owned 1.8.9 bytecode: python scripts/test_vanilla_inventory.py"]
fn vanilla_inventory_matches_original_bytecode() {
    use crate::container::{window_click, Container, Kind};
    use crate::inventory::{ClickAction, Inventory, SlotRef};
    let path = PathBuf::from(".local/reference/fixtures/inventory.json");
    let cases: serde_json::Value = serde_json::from_slice(
        &std::fs::read(path).expect("Run python scripts/test_vanilla_inventory.py first"),
    )
    .unwrap();
    fn read_stack(v: &serde_json::Value) -> Option<Stack> {
        if v.is_null() {
            return None;
        }
        let id = v["id"].as_u64().unwrap() as u16;
        let damage = v["damage"].as_u64().unwrap() as u16;
        Some(Stack {
            nbt: None,
            identity: crate::inventory::Stack::fresh_identity(),
            id,
            count: v["count"].as_u64().unwrap() as u8,
            meta: if crate::inventory::tool(id).0 == crate::inventory::Tool::None {
                damage
            } else {
                0
            },
            damage: if crate::inventory::tool(id).0 != crate::inventory::Tool::None {
                damage
            } else {
                0
            },
        })
    }
    for case in cases.as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let size = case["size"].as_u64().unwrap() as usize;
        let mut inv = Inventory::default();
        let mut container = Container::new(Kind::Chest);
        container.slots.resize(size, None);
        for i in 0..36 {
            inv.slots[i] = read_stack(&case["initial"]["player"][i]);
        }
        for i in 0..size {
            container.slots[i] = read_stack(&case["initial"]["chest"][i]);
        }
        inv.cursor = read_stack(&case["initial"]["cursor"]);
        let mut drag = Vec::new();
        let mut drag_mode = 0;
        for (step_number, step) in case["steps"].as_array().unwrap().iter().enumerate() {
            let raw_slot = step["slot"].as_i64().unwrap();
            let button = step["button"].as_u64().unwrap() as usize;
            let mode = step["mode"].as_u64().unwrap();
            let slot = if raw_slot < 0 {
                SlotRef::Outside
            } else if (raw_slot as usize) < size {
                SlotRef::Container(raw_slot as usize)
            } else {
                let index = raw_slot as usize - size;
                SlotRef::Player(if index < 27 { index + 9 } else { index - 27 })
            };
            let action = match mode {
                0 => Some(ClickAction::Pickup { right: button == 1 }),
                1 => Some(ClickAction::QuickMove),
                2 => Some(ClickAction::Hotbar(button)),
                3 => Some(ClickAction::Clone),
                6 => Some(ClickAction::Collect {
                    reverse: button == 1,
                }),
                5 => match button & 3 {
                    0 => {
                        drag.clear();
                        drag_mode = (button >> 2) as u8;
                        None
                    }
                    1 => {
                        if !drag.contains(&slot) {
                            drag.push(slot);
                        }
                        None
                    }
                    2 => Some(ClickAction::Drag {
                        mode: drag_mode,
                        slots: std::mem::take(&mut drag),
                    }),
                    _ => unreachable!(),
                },
                _ => panic!("Unsupported fixturemode"),
            };
            if let Some(action) = action {
                let drops = window_click(
                    &mut inv,
                    Some(&mut container),
                    slot,
                    action,
                    case["creative"].as_bool().unwrap(),
                    2,
                );
                assert!(drops.is_empty(), "{name}/{step_number}");
            }
            for i in 0..36 {
                assert_eq!(
                    inv.slots[i],
                    read_stack(&step["player"][i]),
                    "{name} step {step_number}, player {i}"
                );
            }
            for i in 0..size {
                assert_eq!(
                    container.slots[i],
                    read_stack(&step["chest"][i]),
                    "{name} step {step_number}, chest {i}"
                );
            }
            assert_eq!(
                inv.cursor,
                read_stack(&step["cursor"]),
                "{name} step {step_number}, cursor"
            );
        }
    }
}
pub(crate) fn assets() -> &'static Assets {
    static A: OnceLock<Assets> = OnceLock::new();
    A.get_or_init(|| {
        Assets::load(&crate::minecraft_dir().join("versions/1.8.9/1.8.9.jar")).unwrap()
    })
}
#[test]
fn scheduled_blocks_match_original_java_world_ticks() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/vanilla189-blocks.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let mut world = flat();
        world.chunks.clear();
        for cz in -2..=2 {
            for cx in -2..=2 {
                let mut chunk = Chunk::empty();
                for z in 0..16 {
                    for x in 0..16 {
                        chunk.set(x, 0, z, 7 << 4);
                    }
                }
                world.chunks.insert((cx, cz), Arc::new(chunk));
            }
        }
        for edit in case["initial"].as_array().unwrap() {
            world.set(
                IVec3::new(
                    edit[0].as_i64().unwrap() as i32,
                    edit[1].as_i64().unwrap() as i32,
                    edit[2].as_i64().unwrap() as i32,
                ),
                edit[3].as_u64().unwrap() as u16,
            );
        }
        let mut ticks = crate::block_ticks::BlockTicks::default();
        // BlockTickOracle.java uses World.setBlockState(..., 3) before taking
        // its "initial" snapshot. Those explicit placements already queued
        // BlockFalling.onBlockAdded at tick 0; loading the snapshot is not a
        // substitute for that event. Recreate the known fixture's pending work.
        for edit in case["initial"].as_array().unwrap() {
            let kind = edit[3].as_u64().unwrap() as u16 >> 4;
            if matches!(kind, 12 | 13 | 145) {
                let p = IVec3::new(edit[0].as_i64().unwrap() as i32,
                    edit[1].as_i64().unwrap() as i32, edit[2].as_i64().unwrap() as i32);
                ticks.schedule_block_update(p, kind, 2, 0);
            }
        }
        let mut elapsed = 0;
        for snapshot in case["snapshots"].as_array().unwrap() {
            let target = snapshot["tick"].as_u64().unwrap();
            while elapsed < target {
                elapsed += 1;
                if case["remove_at"].as_i64().unwrap() == elapsed as i64 {
                    let p = IVec3::new(8, 4, 8);
                    world.set(p, 0);
                    ticks.notify(&world, p);
                }
                ticks.tick(&mut world);
            }
            let mut hash = 0xcbf29ce484222325u64;
            for y in 0..=10 {
                for z in -4..=20 {
                    for x in -4..=20 {
                        let state = world.get(IVec3::new(x, y, z));
                        for byte in state.to_le_bytes() {
                            hash = (hash ^ byte as u64).wrapping_mul(0x100000001b3);
                        }
                    }
                }
            }
            let expected = snapshot["hash"].as_str().unwrap().parse::<u64>().unwrap();
            if hash != expected {
                let expected_blocks: HashMap<_, _> = snapshot["blocks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| {
                        (
                            (
                                e[0].as_i64().unwrap() as i32,
                                e[1].as_i64().unwrap() as i32,
                                e[2].as_i64().unwrap() as i32,
                            ),
                            e[3].as_u64().unwrap() as u16,
                        )
                    })
                    .collect();
                let mut mismatches = Vec::new();
                for y in 1..=10 {
                    for z in -4..=20 {
                        for x in -4..=20 {
                            let actual = world.get(IVec3::new(x, y, z));
                            let expected = *expected_blocks.get(&(x, y, z)).unwrap_or(&0);
                            if actual != expected {
                                mismatches.push(((x, y, z), actual, expected));
                            }
                        }
                    }
                }
                eprintln!(
                    "{name} at {target}: first differences (position, Rust, original): {:?}",
                    &mismatches[..mismatches.len().min(20)]
                );
            }
            assert_eq!(hash, expected, "{name} at original tick {target}");
            assert_eq!(
                ticks.falling.len(),
                snapshot["falling"].as_array().unwrap().len(),
                "{name} falling at tick {target}"
            );
            for (entity, original) in ticks
                .falling
                .iter()
                .zip(snapshot["falling"].as_array().unwrap())
            {
                assert!(
                    entity.position.y.to_bits() == original[1].as_f64().unwrap().to_bits(),
                    "{name} falling position at tick {target}"
                );
            }
        }
    }
}
fn flat() -> World {
    let mut c = Chunk::empty();
    for x in 0..16 {
        for z in 0..16 {
            c.set(x, 0, z, 2 << 4);
        }
    }
    World {
        chunks: crate::world::ChunkMap::from([((0, 0), Arc::new(c))]),
        spawn: Vec3::new(8.5, 1.0, 8.5),
        label: "Test fixture".into(),
        save_path: PathBuf::from(".local/gameplay-test.json"),
        edits: Default::default(),
        imported: false,
        light_revision: 0,
    }
}
#[test]
#[ignore = "requires a Vulkan adapter and the locally owned resources"]
fn vulkan_golden_apple_glint_is_visible_in_hand_and_on_the_ground() {
    use crate::renderer::{RenderInput, Renderer};
    let (a, r) = (assets(), resources());
    let settings = Settings { clouds: 0, render_distance: 4, bobbing: false, ..Default::default() };
    let mut renderer = pollster::block_on(Renderer::new(None, a, r, &settings, None)).unwrap();
    renderer.resize(512, 320);
    let world = flat();
    let mut player = Player::new(world.spawn);
    player.set_rotation_degrees(180., 0.);
    let mut sim = Simulation::new(Mode::Survival, 189);
    sim.clock = 6000;
    let mut apple = Stack::new(322, 1);
    sim.spawn_drop(apple.clone(), world.spawn + Vec3::new(0., 1.35, -2.), Vec3::ZERO);
    sim.drops[0].hover_start = 0.;
    let gui = Gui { screen: Screen::Playing, ..Default::default() };
    let path = std::env::temp_dir().join(format!("ferrum-apple-{}.png", std::process::id()));
    let empty = ui::Frame::default();
    let mut images = vec![];
    for (meta, time) in [(0, 0.4), (1, 0.4), (1, 1.8), (0, 1.8)] {
        apple.meta = meta;
        sim.drops[0].stack.meta = meta;
        sim.inventory.slots[0] = Some(apple.clone());
        for _ in 0..8 { sim.equipped.tick(Some(apple.clone()), None); }
        renderer.screenshot(&path, &RenderInput {
            assets: a, resources: r, settings: &settings, gui: &gui, ui: &empty,
            selected_block: None, world: Some(&world), player: Some(&player), simulation: Some(&sim),
            alpha: 0.5, time,
        }).unwrap();
        images.push(image::open(&path).unwrap().to_rgb8());
    }
    std::fs::remove_file(path).unwrap();
    for (name, (x0, x1, y0, y1)) in [
        ("held", (310, 512, 160, 320)), ("dropped", (220, 293, 112, 202)),
    ] {
        let count = |a: usize, b: usize| {
            (x0..x1).map(|x| (y0..y1).filter(|&y|
                images[a].get_pixel(x,y) != images[b].get_pixel(x,y)).count()).sum::<usize>()
        };
        assert_eq!(count(0, 3), 0, "ordinary {name} apple glowed");
        assert!(count(0, 1) > 12, "{name} enchanted apple has no visible glint");
        assert!(count(1, 2) > 12, "{name} glint did not animate");
    }
    if let Some(dest) = std::env::var_os("FERRUM_APPLE_CAPTURE") {
        let mut strip = image::RgbImage::new(1024, 320);
        image::imageops::replace(&mut strip, &images[0], 0, 0);
        image::imageops::replace(&mut strip, &images[1], 512, 0);
        strip.save(dest).unwrap();
    }
}
#[test]
#[ignore = "requires a Vulkan adapter and the locally owned resources"]
fn vulkan_section_occlusion_matches_full_geometry_and_reopens_after_wall_edit() {
    use crate::renderer::{RenderInput, Renderer};
    let a = assets();
    let r = resources();
    let settings = Settings { clouds: 0, fov: 12.0, ..Default::default() };
    let mut renderer = pollster::block_on(Renderer::new(None,a,r,&settings,None)).unwrap();
    renderer.resize(160,128);
    let mut world=flat();
    for x in [1,2] { world.chunks.insert((x,0),Arc::new(Chunk::empty())); }
    for y in 0..16 {
        for z in 0..16 {
            world.set(IVec3::new(24,y,z),1<<4);
            world.set(IVec3::new(40,y,z),35<<4|14);
        }
    }
    let mut player=Player::new(Vec3::ZERO);
    player.set_pose_f64([8.5,6.38,8.5],-90.,0.,true);
    let mut sim=Simulation::new(Mode::Survival,189);
    sim.clock=6000;
    let gui=Gui {screen:Screen::Playing, hide_hud:true,..Default::default()};
    let frame=ui::build(&gui,&settings,r,a,renderer.resolution(),Some(&sim),&Stats::default());
    let file=std::env::temp_dir().join(format!("ferrum189-section-cull-{}.png",std::process::id()));
    let draw = |renderer: &mut Renderer,world:&World| {
        renderer.screenshot(&file,&RenderInput { selected_block: None, assets:a,resources:r,settings:&settings,
            gui:&gui,ui:&frame,world:Some(world),player:Some(&player),simulation:Some(&sim),alpha:1.,time:4.,
        }).unwrap();
        image::open(&file).unwrap().to_rgb8()
    };
    for x in 0..3 {
        let mut mesh=crate::mesh::build_with_settings(&world,(x,0),a,&settings);
        if x==1 {
            // Keep identical geometry and lighting while permitting the entire
            // frustum through the middle chunk for the reference image.
            mesh.sections.as_mut().unwrap().graph=
                crate::mesh::visibility::TerrainSections::new(&Chunk::empty(),true).graph;
        }
        renderer.upload((x,0),mesh);
    }
    let unoccluded=draw(&mut renderer,&world);
    let unoccluded_triangles=renderer.triangles;
    renderer.upload((1,0),crate::mesh::build_with_settings(&world,(1,0),a,&settings));
    let closed=draw(&mut renderer,&world);
    assert!(closed==unoccluded,"opaque-section traversal changed visible pixels");
    assert!(renderer.triangles<unoccluded_triangles,"hidden back wall was still submitted");
    for y in 7..=9 { for z in 7..=9 {world.set(IVec3::new(24,y,z),0);} }
    renderer.upload((1,0),crate::mesh::build_with_settings(&world,(1,0),a,&settings));
    let opened=draw(&mut renderer,&world);
    assert!(opened!=closed,"opened portal did not reveal the distant wall");
    renderer.remove((2,0));
    let missing=draw(&mut renderer,&world);
    assert!(opened!=missing,"distant wall must really be visible through the opening");
    std::fs::remove_file(file).unwrap();
}
#[test]
#[ignore = "requires a Vulkan adapter and the locally owned resources"]
fn vulkan_water_and_stained_panes_transmit_the_background() {
    use crate::renderer::{RenderInput, Renderer};
    let a = assets();
    let r = resources();
    let s = Settings {
        clouds: 0,
        ..Settings::default()
    };
    let mut renderer = pollster::block_on(Renderer::new(None, a, r, &s, None)).unwrap();
    renderer.resize(256, 160);
    let mut world = flat();
    let mut player = Player::new(Vec3::ZERO);
    player.set_pose_f64([8.5, 62., 11.5], 180., 0., true);
    let mut sim = Simulation::new(Mode::Survival, 189);
    sim.clock = 6000;
    let gui = Gui {
        screen: Screen::Playing,
        hide_hud: true,
        ..Default::default()
    };
    let frame = ui::build(
        &gui,
        &s,
        r,
        a,
        renderer.resolution(),
        Some(&sim),
        &Stats::default(),
    );
    let path =
        std::env::temp_dir().join(format!("ferrum189-blend-test-{}.png", std::process::id()));
    let mut sample = |front, back| {
        for x in 3..=13 {
            for y in 58..=69 {
                world.set(IVec3::new(x, y, 6), back);
                world.set(IVec3::new(x, y, 8), front);
            }
        }
        renderer.upload(
            (0, 0),
            crate::mesh::build_with_settings(&world, (0, 0), a, &s),
        );
        let input = RenderInput {
            selected_block: None,
            assets: a,
            resources: r,
            settings: &s,
            gui: &gui,
            ui: &frame,
            world: Some(&world),
            player: Some(&player),
            simulation: Some(&sim),
            alpha: 1.,
            time: 4.,
        };
        // Each upload invalidates the old ranges, even if the same buffers
        // were reused. The first frame draws directly, the third uses bundles.
        assert!(!renderer.terrain_draws_cached());
        renderer.screenshot(&path, &input).unwrap();
        let direct = image::open(&path).unwrap().to_rgb8();
        renderer.screenshot(&path, &input).unwrap();
        renderer.screenshot(&path, &input).unwrap();
        assert!(renderer.terrain_draws_cached());
        let image = image::open(&path).unwrap().to_rgb8();
        assert!(direct == image, "recorded terrain commands changed rendered pixels");
        if matches!(front >> 4, 9 | 10) && back == 35 << 4 {
            // The camera faces a vertical wall. With multiple liquid blocks
            // above it, the center necessarily samples the flowing sprite.
            let water = front >> 4 == 9;
            let mut colors = std::collections::HashSet::new();
            for y in 60..100 {
                for x in 108..148 {
                    let [r,g,b] = image.get_pixel(x, y).0;
                    if (water && b > r && b > g) || (!water && r > g && g > b) {
                        colors.insert([r,g,b]);
                    }
                }
            }
            assert!(colors.len() >= 4, "missing flowing sprite colors on state {front}");
            if let Some(dir) = std::env::var_os("FERRUM_FLUID_CAPTURE_DIR") {
                image.save(std::path::Path::new(&dir).join(if water { "water.png" } else { "lava.png" })).unwrap();
            }
        }
        let mut sum = 0i32;
        for y in 70..90 {
            for x in 118..138 {
                sum += image
                    .get_pixel(x, y)
                    .0
                    .into_iter()
                    .map(i32::from)
                    .sum::<i32>();
            }
        }
        sum
    };
    // Only the distant wall changes. Opaque stone obscures it, tinted
    // surfaces transmit some light, and air gives the full contrast.
    let mut contrast = |front| sample(front, 35 << 4) - sample(front, (35 << 4) | 15);
    let dry = contrast(0);
    assert!(dry > 40_000, "air contrast {dry}");
    assert_eq!(contrast(1 << 4), 0);
    assert_eq!(contrast(10 << 4), 0);
    for front in [9 << 4, (95 << 4) | 14, (160 << 4) | 14] {
        let wet = contrast(front);
        assert!(
            (1000..dry).contains(&wet),
            "state {front}, transmitted {wet}/{dry}"
        );
    }
    std::fs::remove_file(path).unwrap();
}
#[test]
#[ignore = "requires locally imported Minecraft resources"]
fn survival_mine_pickup_place_eat_die_and_save() {
    let r = resources();
    let s = Settings::default();
    let mut w = flat();
    let mut p = Player::new(w.spawn);
    p.yaw = 0.0;
    p.pitch = 0.0;
    p.grounded = true;
    let mut sim = Simulation::new(Mode::Survival, 189);
    sim.inventory.slots[0] = Some(Stack::new(270, 1));
    let block = IVec3::new(8, 2, 6);
    w.set(block, 16);
    let mut broken = false;
    for _ in 0..30 {
        let out = sim.tick(&mut w, &mut p, r, &s, true, false);
        broken |= out.changed.contains(&block);
    }
    assert!(broken);
    assert_eq!(w.get(block), 0);
    assert_eq!(sim.inventory.slots[0].as_ref().unwrap().damage, 1);
    assert!(sim.drops.iter().any(|d| d.stack.id == 4));
    p.position = Vec3::new(8.5, 1.0, 6.5);
    p.previous = p.position;
    for _ in 0..20 {
        sim.tick(&mut w, &mut p, r, &s, false, false);
    }
    assert!(sim.inventory.slots.iter().flatten().any(|s| s.id == 4));
    assert!(sim.drops.is_empty());
    p.position = Vec3::new(8.5, 1.0, 8.5);
    p.previous = p.position;
    p.pitch = -0.45;
    w.set(IVec3::new(8, 1, 5), 16);
    sim.inventory.selected = 1;
    sim.inventory.slots[1] = Some(Stack::new(4, 3));
    let out = sim.tick(&mut w, &mut p, r, &s, false, true);
    assert_eq!(out.changed.len(), 1);
    assert_eq!(w.get(out.changed[0]), 4 << 4);
    assert_eq!(sim.inventory.slots[1].as_ref().unwrap().count, 2);
    // Release after placing the block: the next press starts a separate use.
    // Continuously holding instead would wait for the original four-tick repeat delay.
    sim.tick(&mut w, &mut p, r, &s, false, false);
    sim.food = 10;
    sim.inventory.slots[1] = Some(Stack::new(260, 2));
    for _ in 0..32 {
        sim.tick(&mut w, &mut p, r, &s, false, true);
    }
    assert_eq!(sim.food, 14);
    assert_eq!(sim.inventory.slots[1].as_ref().unwrap().count, 1);
    sim.health = 1.0;
    sim.damage(20.0);
    assert_eq!(sim.health, 0.0);
    sim.die(&p);
    assert!(sim.inventory.slots.iter().all(Option::is_none));
    assert!(!sim.drops.is_empty());
    let before = sim.drops.iter().map(|d| d.stack.count as u32).sum::<u32>();
    use clap::Parser;
    let id = format!(
        "survival-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let options = crate::Options::parse_from([
        "ferrum189",
        "--generator",
        "1",
        "--render-distance",
        "2",
        "--save-id",
        &id,
    ]);
    w.save_path = PathBuf::from("saves").join(format!("world-{id}.json"));
    w.save(
        &options,
        crate::player_save::PlayerSave::capture(&p, sim.mode),
        &sim,
    )
    .unwrap();
    let restored = World::load(&options).unwrap().simulation.unwrap().restore();
    std::fs::remove_file(&w.save_path).unwrap();
    assert_eq!(restored.health, 0.0);
    assert_eq!(
        restored
            .drops
            .iter()
            .map(|d| d.stack.count as u32)
            .sum::<u32>(),
        before
    );
    assert!(restored.inventory.slots.iter().all(Option::is_none));
    let bed = crate::vanilla::respawn::BedSpawn {
        point: IVec3::new(12, 1, 12),
        forced: true,
    };
    sim.bed_spawn = Some(bed);
    let restored: Simulation = serde_json::from_slice(&serde_json::to_vec(&sim).unwrap()).unwrap();
    assert_eq!(restored.bed_spawn, Some(bed));
    sim.respawn(&mut p, &w);
    assert_eq!(sim.health, 20.0);
    assert_eq!(p.precise_position(), [12.5, f64::from(1.1f32), 12.5]);
    assert_eq!(p.precise_velocity(), [0.; 3]);
    assert_eq!(p.java_rotation(), [0., 0.]);
}
#[test]
#[ignore = "requires locally imported Minecraft resources"]
fn entity_culling_changes_draws_without_removing_ai() {
    let r = resources();
    let a = assets();
    let mut w = flat();
    let mut p = Player::new(w.spawn);
    p.yaw = 0.0;
    p.pitch = 0.0;
    let mut sim = Simulation::new(Mode::Survival, 189);
    let pos = Vec3::new(8.5, 1.0, 3.5);
    sim.mobs.push(Mob {
        id: 1,
        kind: MobKind::Cow,
        position: pos,
        previous: pos,
        velocity: Vec3::ZERO,
        yaw: 0.0,
        health: 10.0,
        age: 0,
        turn_ticks: 60,
        hurt: 0,
        fuse: 0,
        swing_ticks: 0,
        death_ticks: 0,
        eat_ticks: 0,
        attack_ticks: 0,
        rose_ticks: 0,
        prev_fuse: 0,
    });
    for x in 0..16 {
        for y in 1..6 {
            w.set(IVec3::new(x, y, 6), 16);
        }
    }
    let eye = p.eye(1.0);
    let f = Frustum::new(
        Mat4::perspective_rh(70f32.to_radians(), 16.0 / 9.0, 0.05, 256.0)
            * Mat4::look_to_rh(eye, p.direction(), Vec3::Y),
    );
    let render = |settings: &Settings| {
        models::build(models::ModelContext {
            sim: &sim,
            player: &p,
            world: &w,
            assets: a,
            resources: r,
            settings,
            frustum: &f,
            viewport_height: 1080,
            eye,
            alpha: 1.0,
            perspective: 0,
            render_hand: true,
            hitboxes: false,
        })
    };
    let s = Settings::default();
    assert_eq!(render(&s).visible, 0);
    assert_eq!(
        render(&Settings {
            entity_culling: false,
            ..s.clone()
        })
        .visible,
        1
    );
    let age = sim.mobs[0].age;
    sim.tick(&mut w, &mut p, r, &s, false, false);
    assert_eq!(sim.mobs.len(), 1);
    assert_eq!(sim.mobs[0].age, age + 1);
}
#[test]
#[ignore = "requires locally imported Minecraft resources"]
fn ui_geometry_is_reused_and_hidden_hud_draws_no_stale_buttons() {
    let settings = Settings::default();
    let stats = Stats::default();
    let mut frame = ui::Frame::default();
    frame.set_viewport([640, 480], &settings);
    let mut gui = Gui { screen: Screen::Playing, hide_hud: true, ..Default::default() };
    ui::paint(&mut frame, &gui, &settings, resources(), assets(), None, &stats);
    assert_eq!(frame.vertices.capacity(), 0);
    gui.screen = Screen::Main;
    ui::paint(&mut frame, &gui, &settings, resources(), assets(), None, &stats);
    let vertices = frame.vertices.as_ptr();
    let buttons = frame.buttons.as_ptr();
    let expected = bytemuck::cast_slice::<_, u8>(&frame.vertices).to_vec();
    assert!(!expected.is_empty() && !frame.buttons.is_empty());
    gui.screen = Screen::Playing;
    ui::paint(&mut frame, &gui, &settings, resources(), assets(), None, &stats);
    assert!(frame.vertices.is_empty() && frame.buttons.is_empty() && !frame.panorama);
    gui.screen = Screen::Main;
    ui::paint(&mut frame, &gui, &settings, resources(), assets(), None, &stats);
    assert!(frame.panorama);
    assert_eq!(frame.vertices.as_ptr(), vertices);
    assert_eq!(frame.buttons.as_ptr(), buttons);
    assert_eq!(bytemuck::cast_slice::<_, u8>(&frame.vertices), expected);
}
#[test]
#[ignore = "requires locally imported Minecraft resources"]
fn menus_locales_and_graphics_controls_fit_minimum_window() {
    let r = resources();
    let a = assets();
    assert!(r.languages.len() >= 70);
    assert_eq!(r.tr("ko_kr", "menu.singleplayer"), "싱글플레이");
    assert!(r.sound_files > 500);
    let sound = std::fs::read(r.root.join("sounds/random/click.ogg")).unwrap();
    assert!(rodio::Decoder::new(std::io::Cursor::new(sound)).is_ok());
    let mut sim = Simulation::new(Mode::Survival, 189);
    sim.trades.insert(1, crate::villagers::Trader::default());
    let settings = Settings::default();
    for resolution in [[640, 480], [1600, 900]] {
        for screen in [
            Screen::Main,
            Screen::Multiplayer,
            Screen::Realms,
            Screen::Options,
            Screen::Video,
            Screen::Languages,
            Screen::Controls,
            Screen::Create,
            Screen::Inventory,
            Screen::Crafting,
            Screen::Creative,
            Screen::Trading,
            Screen::Pause,
            Screen::Death,
        ] {
            let gui = Gui {
                screen,
                trader: Some(1),
                ..Default::default()
            };
            let frame = ui::build(
                &gui,
                &settings,
                r,
                a,
                resolution,
                Some(&sim),
                &Stats::default(),
            );
            assert!(!frame.vertices.is_empty());
            for b in &frame.buttons {
                assert!(
                    b.rect.x >= 0.0
                        && b.rect.y >= 0.0
                        && b.rect.x + b.rect.w <= frame.size[0] + 0.1
                        && b.rect.y + b.rect.h <= frame.size[1] + 0.1,
                    "{screen:?} at {resolution:?}"
                );
            }
            if screen == Screen::Main {
                assert!(frame.buttons.iter().any(|b|
                    b.enabled && matches!(b.action, Action::Screen(Screen::Multiplayer))
                ));
            }
            if screen == Screen::Multiplayer {
                use crate::ui::multiplayer::Command;
                assert!(frame.buttons.iter().any(|b|
                    b.enabled && matches!(b.action, Action::Server(Command::Direct))
                ));
                assert!(frame.buttons.iter().any(|b|
                    !b.enabled && matches!(b.action, Action::Server(Command::Edit))
                ));
            }
        }
    }
    let mut controls = std::collections::HashSet::new();
    for scroll in 0..=6 {
        let gui = Gui {
            screen: Screen::Video,
            scroll,
            ..Default::default()
        };
        let frame = ui::build(&gui, &settings, r, a, [640, 480], None, &Stats::default());
        for b in frame.buttons {
            if let Action::Setting(s) = b.action {
                controls.insert(format!("{s:?}"));
            }
        }
    }
    assert_eq!(controls.len(), crate::settings::VIDEO.len());
}
#[test]
#[ignore = "requires local original resources"]
fn double_chest_gui_exposes_all_54_slots_without_overlapping_player_storage() {
    let mut container = crate::container::Container::new(crate::container::Kind::Chest);
    container.slots.resize(54, None);
    let gui = Gui {
        screen: Screen::Chest,
        container_view: Some(container),
        ..Default::default()
    };
    let sim = Simulation::default();
    let frame = ui::build(
        &gui,
        &Settings::default(),
        resources(),
        assets(),
        [640, 480],
        Some(&sim),
        &Stats::default(),
    );
    let slots = frame
        .buttons
        .iter()
        .filter(|b| matches!(b.action, Action::ContainerSlot(_) | Action::Slot(_)))
        .collect::<Vec<_>>();
    assert_eq!(slots.len(), 90);
    for (i, a) in slots.iter().enumerate() {
        assert!(a.rect.y >= 0. && a.rect.y + a.rect.h <= frame.size[1]);
        for b in slots.iter().skip(i + 1) {
            assert!(
                a.rect.x + a.rect.w <= b.rect.x
                    || b.rect.x + b.rect.w <= a.rect.x
                    || a.rect.y + a.rect.h <= b.rect.y
                    || b.rect.y + b.rect.h <= a.rect.y,
                "Overlapping slots"
            );
        }
    }
}
#[test]
fn crafting_tools_and_armor_support_mirrored_patterns() {
    use crate::inventory::recipe;
    let pick = [
        Some(Stack::new(4, 1)),
        Some(Stack::new(4, 1)),
        Some(Stack::new(4, 1)),
        None,
        Some(Stack::new(280, 1)),
        None,
        None,
        Some(Stack::new(280, 1)),
        None,
    ];
    assert_eq!(recipe(&pick, 3).unwrap().output.id, 274);
    let mut invalid = pick;
    invalid[3] = Some(Stack::new(3, 1));
    assert!(recipe(&invalid, 3).is_none());
    let mut armor = vec![None; 9];
    for i in [0, 1, 2, 3, 5, 6, 8] {
        armor[i] = Some(Stack::new(265, 1));
    }
    assert_eq!(recipe(&armor, 3).unwrap().output.id, 308);
    assert_eq!(gameplay::experience_level(7), (1, 0.0));
}
#[test]
fn imported_container_is_not_reimported_after_break_and_replace() {
    let mut w = flat();
    w.imported = true;
    let pos = IVec3::new(3, 1, 3);
    w.set(pos, 54 << 4 | 2);
    let mut chest = crate::container::Container::new(crate::container::Kind::Chest);
    chest.slots[0] = Some(Stack::new(264, 5));
    Arc::make_mut(w.chunks.get_mut(&(0, 0)).unwrap())
        .containers
        .push((pos, chest));
    let mut sim = Simulation::default();
    sim.import_containers(&w);
    let key = gameplay::container_key(pos);
    assert_eq!(sim.containers[&key].slots[0], Some(Stack::new(264, 5)));
    sim.containers.remove(&key);
    w.set(pos, 0);
    w.set(pos, 54 << 4 | 2);
    sim.import_containers(&w);
    assert!(!sim.containers.contains_key(&key));
    let bytes = serde_json::to_vec(&sim).unwrap();
    let mut restored: Simulation = serde_json::from_slice(&bytes).unwrap();
    restored.import_containers(&w);
    assert!(!restored.containers.contains_key(&key));
}
#[test]
fn legacy_item_nbt_preserves_metadata_and_tool_damage() {
    use crate::nbt::Tag;
    let stack = |id, damage, count| {
        Tag::Compound(std::collections::BTreeMap::from([
            ("id".into(), Tag::Short(id)),
            ("Damage".into(), Tag::Short(damage)),
            ("Count".into(), Tag::Byte(count)),
        ]))
    };
    assert_eq!(
        crate::inventory::from_nbt(&stack(35, 14, 12)),
        Some(Stack {
            nbt: None,
            identity: crate::inventory::Stack::fresh_identity(),
            id: 35,
            meta: 14,
            count: 12,
            damage: 0
        })
    );
    assert_eq!(
        crate::inventory::from_nbt(&stack(278, 120, 1)),
        Some(Stack {
            nbt: None,
            identity: crate::inventory::Stack::fresh_identity(),
            id: 278,
            meta: 0,
            count: 1,
            damage: 120
        })
    );
    assert!(crate::inventory::from_nbt(&stack(4, 0, 0)).is_none());
}
#[test]
fn nested_options_return_to_pause_and_main_languages_return_to_title() {
    let mut gui = Gui {
        screen: Screen::Pause,
        ..Default::default()
    };
    gui.open(Screen::Options);
    gui.open(Screen::Video);
    gui.open(gui.back());
    assert_eq!(gui.options_parent, Screen::Pause);
    gui.open(Screen::Languages);
    gui.open(gui.back());
    assert_eq!(gui.back(), Screen::Pause);
    gui.open(Screen::Main);
    gui.open(Screen::Languages);
    gui.open(Screen::Options);
    assert_eq!(gui.back(), Screen::Main);
}

#[test]
#[ignore = "uses a Vulkan adapter and installed 1.8.9 resources"]
fn six_infested_blocks_match_host_materials_in_the_vulkan_world() {
    use crate::renderer::{RenderInput, Renderer};
    let a=assets();
    let r=resources();
    let settings=Settings{clouds:0,alternate_blocks:false, ..Default::default()};
    let mut renderer=pollster::block_on(Renderer::new(None,a,r,&settings,None)).unwrap();
    renderer.resize(768,384);
    let mut world=flat();
    let mut player=Player::new(Vec3::ZERO);
    player.set_pose_f64([7.,64.5,16.5],180.,0.,true);
    let mut sim=Simulation::new(Mode::Survival,189);
    sim.clock=6000;
    let gui=Gui{screen:Screen::Playing,hide_hud:true,..Default::default()};
    let frame=ui::build(&gui,&settings,r,a,renderer.resolution(),Some(&sim),&Stats::default());
    let ordinary=[1<<4,4<<4,98<<4,98<<4|1,98<<4|2,98<<4|3];
    let path=std::env::temp_dir().join(format!("ferrum-infested-blocks-{}.png",std::process::id()));
    let mut render=|infested:bool| {
        for (i,&host) in ordinary.iter().enumerate() {
            let x=2+2*i as i32;
            world.set(IVec3::new(x,65,8),host);
            world.set(IVec3::new(x,67,8),if infested{97<<4|i as u16}else{host});
        }
        renderer.upload((0,0),crate::mesh::build_with_settings(&world,(0,0),a,&settings));
        let input=RenderInput{selected_block: None,assets:a,resources:r,settings:&settings,gui:&gui,ui:&frame,world:Some(&world),player:Some(&player),simulation:Some(&sim),alpha:1.,time:4.};
        renderer.screenshot(&path,&input).unwrap();
        image::open(&path).unwrap().to_rgb8()
    };
    let regular=render(false);
    let actual=render(true);
    assert!(regular==actual,"The six upper infested blocks must render like their matching ordinary blocks");
    let layers=ordinary.map(|s|a.texture(s,2,true));
    assert!(layers.into_iter().all(|l|l!=0));
    let unique:std::collections::HashSet<_>=layers.into_iter().collect();
    assert_eq!(unique.len(),6);
    if let Some(target)=std::env::var_os("FERRUM_INFESTED_CAPTURE"){std::fs::copy(&path,target).unwrap();}
    std::fs::remove_file(path).unwrap();
}
