use super::*;
use crate::world::Chunk;
use std::sync::Arc;
use winit::keyboard::KeyCode;

type Trace = (i32, f64, u8, i32, u8, bool, usize, Vec<[u32; 6]>);

#[test]
#[ignore = "requires locally imported Minecraft resources"]
fn door_cactus_survival_contact_hits_at_the_inset_even_when_stationary() {
    let resources = crate::validation::resources();
    let settings = Settings::default();
    for (x, y, mode, hits) in [
        (8.5, 2.9375, Mode::Survival, true), // feet on the recessed top
        (8.0 + 0.0625 - f64::from(0.3f32), 2.0, Mode::Survival, true),
        (8.0 - f64::from(0.3f32), 2.0, Mode::Survival, false),
        (8.5, 2.9375, Mode::Creative, false),
    ] {
        let mut world = world(0, false);
        world.set(IVec3::new(8, 1, 8), 12 << 4);
        world.set(IVec3::new(8, 2, 8), 81 << 4);
        let mut player = Player::new(world.spawn);
        player.set_pose_f64([x, y, 8.5], 0., 0., true);
        let mut sim = Simulation { mode, mob_spawning: false, world_difficulty: Some(2), ..Default::default() };
        for tick in 0..21 {
            sim.tick(&mut world, &mut player, resources, &settings, false, false);
            let damage = if hits { 1 + tick / 10 } else { 0 };
            assert_eq!(world.get(IVec3::new(8, 2, 8)) >> 4, 81);
            assert_eq!(sim.hurt_camera.hurt_time, if hits { 9 - tick % 10 } else { 0 });
            assert_eq!(sim.health, 20. - damage as f32,
                "x={x} y={y} tick={tick} pos={:?} contact={} hurt_ticks={}",
                player.precise_position(), player.touching_cactus(), sim.hurt_ticks);
        }
    }
}

#[test]
#[ignore = "requires locally imported Minecraft resources"]
fn hurt_camera_starts_on_landing_and_resets_after_death_and_respawn() {
    let mut world = world(0, false);
    let mut player = Player::new(Vec3::new(8.5, 12., 8.5));
    let settings = Settings::default();
    let resources = crate::validation::resources();
    let mut sim = Simulation { mob_spawning: false, ..Default::default() };
    for _ in 0..50 {
        sim.tick(&mut world, &mut player, resources, &settings, false, false);
        if player.grounded { break; }
        assert_eq!(sim.hurt_camera.hurt_time, 0);
    }
    assert!(sim.health < sim.max_health && sim.health > 0.);
    for remaining in (1..=9).rev() {
        assert_eq!(sim.hurt_camera.hurt_time, remaining);
        sim.tick(&mut world, &mut player, resources, &settings, false, false);
    }
    assert_eq!(sim.hurt_camera, HurtCamera::default());
    player.set_pose_f64([8.5, 14., 8.5], 0., 0., false);
    sim.health = 1.;
    for _ in 0..50 {
        sim.tick(&mut world, &mut player, resources, &settings, false, false);
        if sim.health == 0. { break; }
    }
    assert_eq!(sim.health, 0.);
    assert_eq!(sim.hurt_camera, HurtCamera { hurt_time: 9, death_time: 1 });
    for tick in 2..=25 {
        sim.hurt_camera.tick(true);
        assert_eq!(sim.hurt_camera.hurt_time, 10u8.saturating_sub(tick));
        assert_eq!(sim.hurt_camera.death_time, tick.min(20));
    }
    sim.respawn(&mut player, &world);
    assert_eq!(sim.hurt_camera, HurtCamera::default());
}

#[test]
fn hurt_camera_accepts_absorbed_hits_but_ignores_immune_hits_and_save_reloads() {
    let mut sim = Simulation { absorption: 4., ..Default::default() };
    sim.damage(2.);
    assert_eq!((sim.health, sim.hurt_camera.hurt_time), (20., 10));
    sim.hurt_camera.tick(false);
    sim.damage(1.);
    assert_eq!((sim.absorption, sim.hurt_camera.hurt_time), (2., 9));
    // S1C updates HP before S06 after a stronger hit during immunity. This
    // reduces HP without a new S19/2 or another client camera impulse.
    sim.damage(6.);
    assert_eq!((sim.health, sim.hurt_camera.hurt_time), (18., 9));
    let loaded: Simulation = serde_json::from_slice(&serde_json::to_vec(&sim).unwrap()).unwrap();
    assert_eq!(loaded.hurt_camera, HurtCamera::default());
}

#[test]
#[ignore = "requires locally imported Minecraft resources"]
fn door_cactus_living_mob_contact_uses_hurt_immunity() {
    let mut world = world(0, false);
    world.set(IVec3::new(8, 1, 8), 12 << 4);
    world.set(IVec3::new(8, 2, 8), 81 << 4);
    let position = Vec3::new(8.5, 2.9375, 8.5);
    let mut sim = Simulation { mob_spawning: false, ..Default::default() };
    sim.mobs.push(Mob {
        id: 7, kind: MobKind::Cow, position, previous: position, velocity: Vec3::ZERO,
        yaw: 0., health: 10., age: 0, turn_ticks: 0, hurt: 0, fuse: 0,
        swing_ticks: 0, death_ticks: 0, eat_ticks: 0, attack_ticks: 0, rose_ticks: 0, prev_fuse: 0,
    });
    let mut player = Player::new(Vec3::new(3.5, 1., 3.5));
    for tick in 0..11 {
        // Keep the animal on the plant irrespective of the wandering goal's RNG.
        sim.mobs[0].position = position;
        sim.mobs[0].velocity = Vec3::ZERO;
        let out = sim.tick(&mut world, &mut player, crate::validation::resources(),
            &Settings::default(), false, false);
        let hit = tick == 0 || tick == 10;
        assert_eq!(sim.mobs[0].health, if tick == 10 { 8. } else { 9. });
        assert_eq!(out.world_sounds.iter().any(|s| s.0 == "mob.cow.hurt"), hit);
    }
}

fn world(scenario: i32, track: bool) -> World {
    let mut chunks = crate::world::ChunkMap::new();
    for cz in 0..=if track { 16 } else { 0 } {
        let mut chunk = Chunk::empty();
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..=8 {
                    chunk.set(
                        x,
                        y,
                        z,
                        crate::player::movement_tests::initial(x, y, cz * 16 + z, scenario),
                    );
                }
            }
        }
        chunks.insert((0, cz), Arc::new(chunk));
    }
    World {
        chunks,
        spawn: Vec3::new(8.5, 1., 8.5),
        label: String::new(),
        save_path: Default::default(),
        edits: Default::default(),
        imported: false,
        light_revision: 0,
    }
}

#[test]
#[ignore = "requires locally imported Minecraft resources"]
fn original_falls_and_sprint_hunger_through_the_singleplayer_tick() {
    let cases: Vec<Trace> = serde_json::from_str(include_str!(
        "../tests/fixtures/vanilla189-survival-movement.json"
    ))
    .unwrap();
    let resources = crate::validation::resources();
    let mut differences = Vec::new();
    for (index, (scenario, height, flags, boost, difficulty, creative, ticks, rows)) in
        cases.into_iter().enumerate()
    {
        let mut world = world(scenario, flags & 1 != 0);
        let mut player = Player::new(world.spawn);
        player.set_pose_f64(
            [
                8.5,
                height,
                if matches!(scenario, 7 | 8) { 7.5 } else { 8.5 },
            ],
            0.,
            0.,
            height == 1.,
        );
        for (mask, key) in [
            (1, KeyCode::KeyW),
            (16, KeyCode::Space),
            (32, KeyCode::ShiftRight),
            (64, KeyCode::ControlLeft),
        ] {
            if flags & mask != 0 {
                player.keys.insert(key);
            }
        }
        let mut sim = Simulation {
            mode: if creative {
                Mode::Creative
            } else {
                Mode::Survival
            },
            world_difficulty: Some(difficulty),
            mob_spawning: false,
            ..Default::default()
        };
        if boost >= 0 {
            sim.add_food_effect(Effect::new(8, 1200, boost));
        }
        // The global preference deliberately disagrees with the world's difficulty.
        let settings = Settings {
            difficulty: (difficulty + 1) % 4,
            ..Default::default()
        };
        let mut rows = rows.into_iter().peekable();
        for tick in 0..ticks {
            sim.tick(&mut world, &mut player, resources, &settings, false, false);
            if tick == 8 && index == 6 {
                let pose = crate::player_save::PlayerSave::capture(&player, sim.mode);
                let saved = serde_json::to_vec(&sim).unwrap();
                sim = serde_json::from_slice::<Simulation>(&saved)
                    .unwrap()
                    .restore();
                player = Player::new(world.spawn);
                pose.apply(&mut player);
            }
            if let Some(expected) = rows.next_if(|r| r[0] == tick as u32) {
                let actual = [
                    tick as u32,
                    sim.fall_distance.to_bits(),
                    sim.health.to_bits(),
                    sim.food as u32,
                    sim.saturation.to_bits(),
                    sim.exhaustion.to_bits(),
                ];
                if actual != expected {
                    differences.push(format!(
                        "case {index} surface {scenario} tick {tick}: {actual:?} != {expected:?}"
                    ));
                    break;
                }
            }
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
#[ignore = "requires locally imported Minecraft resources"]
fn original_starvation_regeneration_and_locked_difficulty_in_live_worlds() {
    let data: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/vanilla189-food.json")).unwrap();
    let resources = crate::validation::resources();
    let rows = data["ticks"].as_array().unwrap();
    let mut used = [0; 4];
    for row in rows {
        if row["remote"] == true || row["immune"] == true {
            continue;
        }
        let difficulty = row["difficulty"].as_u64().unwrap() as u8;
        let i = difficulty as usize;
        let (initial, result) = (&row["initial"], &row["result"]);
        if initial["health"] == result["health"] || used[i] >= 8 {
            continue;
        }
        let f = |v: &serde_json::Value| f32::from_bits(v.as_u64().unwrap() as u32);
        let mut sim = Simulation {
            mob_spawning: false,
            world_difficulty: Some(if difficulty == 3 { 0 } else { difficulty }),
            hardcore: difficulty == 3,
            difficulty_locked: true,
            natural_regeneration: row["regeneration"].as_bool().unwrap(),
            food: initial["food"].as_i64().unwrap() as i32,
            food_timer: initial["timer"].as_i64().unwrap() as i32,
            health: f(&initial["health"]),
            max_health: f(&row["maximum"]),
            saturation: f(&initial["saturation"]),
            exhaustion: f(&initial["exhaustion"]),
            player_ticks: row["start"].as_i64().unwrap() as i32 - 1,
            ..Simulation::default()
        };
        let mut world = world(0, false);
        let mut player = Player::new(world.spawn);
        player.grounded = true;
        let settings = Settings {
            difficulty: (difficulty + 2) % 4,
            ..Default::default()
        };
        for _ in 0..row["ticks"].as_u64().unwrap() {
            sim.tick(&mut world, &mut player, resources, &settings, false, false);
        }
        assert_eq!(
            [
                sim.health.to_bits(),
                sim.food as u32,
                sim.food_timer as u32,
                sim.saturation.to_bits(),
                sim.exhaustion.to_bits()
            ],
            [
                f(&result["health"]).to_bits(),
                result["food"].as_i64().unwrap() as u32,
                result["timer"].as_i64().unwrap() as u32,
                f(&result["saturation"]).to_bits(),
                f(&result["exhaustion"]).to_bits()
            ],
            "difficulty {difficulty} seed data {row}"
        );
        used[i] += 1;
    }
    assert_eq!(used, [8; 4]);
}

#[test]
#[ignore = "requires locally imported Minecraft resources"]
fn original_zombie_and_spider_hits_on_all_world_difficulties() {
    let cases: Vec<(u8, u8, f32, f32)> = serde_json::from_str(include_str!(
        "../tests/fixtures/vanilla189-survival-melee.json"
    ))
    .unwrap();
    for (difficulty, id, _, health) in cases {
        let mut world = world(0, false);
        let mut player = Player::new(world.spawn);
        let kind = if id == 54 {
            MobKind::Zombie
        } else {
            MobKind::Spider
        };
        let mut sim = Simulation {
            world_difficulty: Some(difficulty),
            mob_spawning: false,
            ..Default::default()
        };
        for n in 1..=2 {
            let position = world.spawn + Vec3::X * (n as f32 - 1.5);
            sim.mobs.push(Mob {
                id: n,
                kind,
                position,
                previous: position,
                velocity: Vec3::ZERO,
                yaw: 0.,
                health: kind.health(),
                age: 0,
                turn_ticks: 100,
                hurt: 0,
                fuse: 0,
                swing_ticks: 0,
                death_ticks: 0,
                eat_ticks: 0,
                attack_ticks: 0,
                rose_ticks: 0,
                prev_fuse: 0,
            });
        }
        let settings = Settings {
            difficulty: (difficulty + 1) % 4,
            ..Default::default()
        };
        sim.tick(
            &mut world,
            &mut player,
            crate::validation::resources(),
            &settings,
            false,
            false,
        );
        assert_eq!(sim.health, health, "difficulty {difficulty}, mob ID {id}");
        assert_eq!(sim.mobs.is_empty(), difficulty == 0);
    }
}
