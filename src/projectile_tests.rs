use super::*;
use std::sync::Arc;

fn bow() -> bow::BowShot {
    bow::BowShot {
        speed: 3.,
        damage: 2.,
        punch: 0,
        fire_ticks: 0,
        critical: false,
        pickup: 1,
    }
}

#[derive(Deserialize)]
struct Trace {
    kind: String,
    seed: i64,
    player: [f64; 3],
    look: [f32; 2],
    speed: f32,
    water: bool,
    frames: Vec<[f64; 8]>,
}

#[test]
fn installed_arrow_throwable_and_xp_bottle_launch_and_1600_flight_ticks_match() {
    let traces: Vec<Trace> = serde_json::from_reader(flate2::read::GzDecoder::new(
        &include_bytes!("../tests/fixtures/vanilla189-projectile-motion.json.gz")[..],
    ))
    .unwrap();
    let air = Arc::new(crate::world::Chunk::empty());
    let mut water = crate::world::Chunk::empty();
    for x in 0..16 {
        for z in 0..16 {
            for y in 0..128 {
                water.set(x, y, z, 9 << 4);
            }
        }
    }
    let water = Arc::new(water);
    for (case, trace) in traces.into_iter().enumerate() {
        let column = if trace.water { &water } else { &air };
        let mut world = super::super::tests::flat_world();
        world.chunks = (-5..=5)
            .flat_map(|x| (-5..=5).map(move |z| ((x, z), Arc::clone(column))))
            .collect();
        let mut player = Player::new(Vec3::ZERO);
        player.set_pose_f64(trace.player, trace.look[0], trace.look[1], false);
        let mut sim = Simulation::new(Mode::Creative, 189);
        let payload = if trace.kind == "arrow" {
            Payload::Arrow(bow::BowShot {
                speed: trace.speed,
                ..bow()
            })
        } else if trace.kind == "experience_bottle" {
            Payload::ExperienceBottle
        } else {
            Payload::Snowball
        };
        sim.projectiles
            .push(Projectile::launched(&player, payload, 0, trace.seed));
        for (tick, expected) in trace.frames.into_iter().enumerate() {
            if tick > 0 {
                sim.tick_projectiles(&mut world, &mut player, &mut Outcome::default());
            }
            assert_eq!(sim.projectiles.len(), 1, "trace {case} tick {tick}");
            let p = &sim.projectiles[0];
            let actual = [
                p.position.x,
                p.position.y,
                p.position.z,
                p.velocity.x,
                p.velocity.y,
                p.velocity.z,
                p.yaw.into(),
                p.pitch.into(),
            ];
            for component in 0..8 {
                assert!(
                    (actual[component] - expected[component]).abs() < 1e-6,
                    "trace {case} tick {tick} component {component} actual {} expected {}",
                    actual[component],
                    expected[component]
                );
            }
        }
    }
}

#[test]
#[ignore = "uses installed resources for normal XP-bottle right-click ticks"]
fn xp_bottle_right_click_flies_breaks_and_awards_original_world_rng_total() {
    let resources = crate::validation::resources();
    for mode in [Mode::Survival, Mode::Creative] {
        let mut sim = Simulation::new(mode, 189);
        sim.mob_spawning = false;
        sim.inventory.slots[0] = Some(Stack::new(384, 2));
        let mut world = wall();
        let mut player = Player::new(Vec3::new(8.5, 1., 4.5));
        player.set_rotation_degrees(0., 0.);
        sim.tick(
            &mut world,
            &mut player,
            resources,
            &Settings::default(),
            false,
            true,
        );
        assert_eq!(sim.projectiles.len(), 1);
        assert!(matches!(
            sim.projectiles[0].payload,
            Payload::ExperienceBottle
        ));
        assert_eq!(
            sim.inventory.held().unwrap().count,
            if mode == Mode::Creative { 2 } else { 1 }
        );
        assert_eq!(
            sim.statistics["stat.useItem.minecraft.experience_bottle"],
            1
        );
        // Direct flight ticks isolate World.rand's original impact roll from
        // unrelated world/weather ticks and keep the player away from XP.
        let seed = JavaRandom::new(741);
        sim.block_ticks.set_world_random(&seed);
        let mut expected = seed.clone();
        let total = 3 + expected.next_int(5) + expected.next_int(5);
        let mut out = Outcome::default();
        for _ in 0..60 {
            sim.tick_projectiles(&mut world, &mut player, &mut out);
            if sim.projectiles.is_empty() {
                break;
            }
        }
        assert!(sim.projectiles.is_empty());
        assert_eq!(sim.xp_orbs.iter().map(|orb| orb.value).sum::<i32>(), total);
        assert_eq!(sim.block_ticks.world_random().state(), expected.state());
        assert_eq!(sim.health, 20.);
        assert!(out
            .world_sounds
            .iter()
            .any(|sound| sound.0 == "game.potion.smash"));
        assert!(sim
            .particles
            .iter()
            .any(|particle| particle.item_texture() == Some((373, 0))));
        let location = sim.xp_orbs[0].position;
        player.set_position_f64(location.to_array());
        for _ in 0..80 {
            sim.tick(
                &mut world,
                &mut player,
                resources,
                &Settings::default(),
                false,
                false,
            );
        }
        assert_eq!(sim.xp.total, total);
    }
}

fn wall() -> World {
    let mut world = super::super::tests::flat_world();
    for x in 0..16 {
        for y in 1..5 {
            world.set(IVec3::new(x, y, 10), 1 << 4);
        }
    }
    world
}

#[test]
#[ignore = "uses installed resources for normal native right-click ticks"]
fn right_click_launches_throwables_and_pearl_teleports_with_fall_damage() {
    let resources = crate::validation::resources();
    for id in [332, 344, 368] {
        let mut world = wall();
        let mut sim = Simulation::new(Mode::Survival, 189);
        sim.mob_spawning = false;
        sim.inventory.slots[0] = Some(Stack::new(id, 2));
        let mut player = Player::new(Vec3::new(8.5, 1., 4.5));
        player.set_rotation_degrees(0., 0.);
        sim.tick(
            &mut world,
            &mut player,
            &resources,
            &Settings::default(),
            false,
            true,
        );
        assert_eq!(sim.inventory.held().unwrap().count, 1, "item {id}");
        assert_eq!(sim.projectiles.len(), 1, "item {id}");
        for _ in 0..12 {
            sim.tick(
                &mut world,
                &mut player,
                &resources,
                &Settings::default(),
                false,
                false,
            );
            if sim.projectiles.is_empty() {
                break;
            }
        }
        assert!(sim.projectiles.is_empty(), "item {id}");
        if id == 368 {
            assert_eq!(sim.health, 15.);
            assert!(player.position.z > 8. && player.position.z < 10.);
            assert_eq!(sim.fall_distance, 0.);
        } else {
            assert_eq!(sim.health, 20.);
        }
    }
}

#[test]
fn stuck_arrow_survives_reload_and_is_retrieved_after_its_shake() {
    let mut world = wall();
    let mut sim = Simulation::new(Mode::Survival, 189);
    let mut player = Player::new(Vec3::new(8.5, 1., 4.5));
    player.set_rotation_degrees(0., 0.);
    sim.launch_bow(&player, bow());
    sim.projectiles[0].velocity = DVec3::Z * 3.;
    for _ in 0..3 {
        sim.tick_projectiles(&mut world, &mut player, &mut Outcome::default());
    }
    let arrow = &sim.projectiles[0];
    assert!(arrow.embedded.is_some());
    // The 0.05-block recoil follows the downward trajectory as it embeds.
    assert!((arrow.position.z - 9.95).abs() < 1e-4);
    let mut sim: Simulation = serde_json::from_slice(&serde_json::to_vec(&sim).unwrap()).unwrap();
    player.set_position_f64([8.5, 1., 9.5]);
    for _ in 0..8 {
        sim.tick_projectiles(&mut world, &mut player, &mut Outcome::default());
    }
    assert!(sim.projectiles.is_empty());
    assert_eq!(
        sim.inventory
            .slots
            .iter()
            .flatten()
            .filter(|s| s.id == 262)
            .map(|s| usize::from(s.count))
            .sum::<usize>(),
        1
    );
}

#[test]
fn arrows_hit_entities_before_walls_and_snowballs_hurt_blazes() {
    for (kind, payload, expected) in [
        (MobKind::Cow, Payload::Arrow(bow()), 4.),
        (MobKind::Blaze, Payload::Snowball, 17.),
        (MobKind::Cow, Payload::ExperienceBottle, 10.),
    ] {
        let mut world = wall();
        let mut sim = Simulation::new(Mode::Survival, 189);
        let mut player = Player::new(Vec3::new(8.5, 1., 4.5));
        player.set_rotation_degrees(0., 0.);
        let id = sim.spawn_registered(kind, Vec3::new(8.5, 1., 8.), 0.);
        let mut projectile = Projectile::launched(&player, payload, 0, 189);
        projectile.velocity = DVec3::Z * 3.;
        sim.projectiles.push(projectile);
        sim.tick_projectiles(&mut world, &mut player, &mut Outcome::default());
        assert_eq!(
            sim.mobs.iter().find(|m| m.id == id).unwrap().health,
            expected
        );
        assert!(sim.projectiles.is_empty());
    }
}

#[test]
fn egg_impact_spawns_original_one_or_four_baby_chickens() {
    for count in [1, 4] {
        let seed = (0..10000)
            .find(|seed| {
                let mut r = JavaRandom::new(*seed);
                r.next_int(8) == 0 && (if r.next_int(32) == 0 { 4 } else { 1 }) == count
            })
            .unwrap();
        let mut world = wall();
        let mut sim = Simulation::new(Mode::Survival, 189);
        let mut player = Player::new(Vec3::new(8.5, 1., 8.5));
        player.set_rotation_degrees(0., 0.);
        let mut projectile = Projectile::launched(&player, Payload::Egg, 0, 189);
        projectile.random = JavaRandom::new(seed).state();
        projectile.velocity = DVec3::Z * 2.;
        sim.projectiles.push(projectile);
        sim.tick_projectiles(&mut world, &mut player, &mut Outcome::default());
        assert_eq!(sim.mobs.len(), count);
        assert!(sim
            .mobs
            .iter()
            .all(|m| m.kind == MobKind::Chicken && sim.is_baby(m.id)));
    }
}

#[test]
fn enderman_avoids_projectile_damage_and_pearl_spawning_respects_game_rule() {
    let mut world = wall();
    let mut sim = Simulation::new(Mode::Survival, 189);
    let mut player = Player::new(Vec3::new(8.5, 1., 4.5));
    player.set_rotation_degrees(0., 0.);
    let enderman = sim.spawn_registered(MobKind::Enderman, Vec3::new(8.5, 1., 8.), 0.);
    sim.mob_brains.entry(enderman).or_default().entity_random = Some(JavaRandom::new(189).state());
    sim.launch_bow(&player, bow());
    sim.projectiles[0].velocity = DVec3::Z * 3.;
    sim.tick_projectiles(&mut world, &mut player, &mut Outcome::default());
    assert_eq!(sim.mobs[0].health, 40.);
    assert_eq!(sim.projectiles.len(), 1);

    let seed = (0..100000)
        .find(|&seed| JavaRandom::new(seed).next_float() < 0.05)
        .unwrap();
    for spawning in [false, true] {
        let mut sim = Simulation::new(Mode::Survival, 189);
        sim.mob_spawning = spawning;
        let old = player.position;
        let mut pearl = Projectile::launched(&player, Payload::Pearl, 0, 189);
        pearl.position += DVec3::X * 2.;
        sim.finish_throw(
            &pearl,
            &mut player,
            &mut JavaRandom::new(seed),
            &mut Outcome::default(),
        );
        assert_eq!(sim.mobs.len(), usize::from(spawning));
        if spawning {
            let mite = &sim.mobs[0];
            assert_eq!(mite.kind, MobKind::Endermite);
            assert_eq!(mite.position, old);
            assert_eq!(
                species_ai::number(sim.command_entities.mobs.get(&mite.id), "PlayerSpawned"),
                1
            );
        }
    }
}
