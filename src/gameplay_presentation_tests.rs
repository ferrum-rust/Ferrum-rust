use super::super::*;
use crate::{
    container::{Container, Kind},
    entity_visual::Gait,
    vanilla::{drops::DropNbt, random::independent_seed_calls_for_thread},
    world::{Chunk, ChunkMap},
};
use glam::DVec3;
use std::sync::Arc;

fn fixture() -> (World, Player, Simulation) {
    let mut chunk = Chunk::empty();
    for x in 0..16 { for z in 0..16 { chunk.set(x, 0, z, 1 << 4); } }
    let world = World {
        chunks: ChunkMap::from([((0, 0), Arc::new(chunk))]),
        spawn: Vec3::new(8.5, 1., 8.5), label: "presentation projection".into(),
        save_path: Default::default(), edits: Default::default(), imported: false, light_revision: 0,
    };
    let mut player = Player::new(world.spawn);
    player.set_pose_f64([8.5, 1., 8.5], 0., 0., true);
    let mut sim = Simulation::default();
    sim.mode = Mode::Survival;
    sim.local_name = "MirrorFixture".into();
    sim.clock = 349;
    sim.total_ticks = 470;
    sim.player_ticks = 93;
    sim.random_tick_lcg = -9981;
    sim.rng = 991;
    sim.player_random_state = 771;
    sim.enchantment_random_state = 331;
    sim.voice_random = [17, 19, 23, 29];
    sim.particle_random = [31, 37, 41];
    sim.fire_ticks = 50;
    sim.fire_flag = true;
    sim.hurt_camera.hurt();
    sim.health = 13.;
    sim.food = 11;
    sim.absorption = 4.;
    sim.effects.add_client(Effect::new(1, 400, 2)).unwrap();
    sim.effects.add_client(Effect::new(10, 120, 1)).unwrap();
    let named = Stack::new(276, 1).with_nbt(DropNbt::Compound(
        std::collections::BTreeMap::from([
            ("display".into(), DropNbt::Compound(std::collections::BTreeMap::from([
                ("Name".into(), DropNbt::String("Mirror sword".into())),
            ]))),
            ("Unbreakable".into(), DropNbt::Byte(1)),
        ])));
    sim.inventory.slots[0] = Some(named.clone());
    sim.inventory.slots[37] = Some(Stack::new(311, 1));
    sim.inventory.cursor = Some(Stack::new(264, 3));
    sim.inventory.crafting[0] = Some(Stack::new(5, 2));
    let mut chest = Container::new(Kind::Chest);
    chest.slots[0] = Some(named.clone());
    sim.containers.insert(container_key(IVec3::new(3, 1, 3)), chest);
    sim.world_commands.rules.insert("keepInventory".into(), "true".into());
    sim.statistics.insert("stat.jump".into(), 123);
    sim.commands.player_mode = Some(0);
    for (id, kind, position) in [
        (11, MobKind::Sheep, Vec3::new(6.5, 1., 11.5)),
        (12, MobKind::Zombie, Vec3::new(8.5, 1., 11.5)),
        (13, MobKind::Zombie, Vec3::new(8.5, 1., 11.5)),
        (14, MobKind::Villager, Vec3::new(10.5, 1., 11.5)),
    ] {
        sim.mobs.push(Mob { id, kind, position, previous: position - Vec3::X * 0.075,
            velocity: Vec3::new(0.1, 0., 0.05), yaw: 0.3, health: 12., age: 55,
            turn_ticks: 7, hurt: 5, fuse: 0, swing_ticks: 0, death_ticks: 0, eat_ticks: 0,
            attack_ticks: 0, rose_ticks: 0, prev_fuse: 0 });
        let mut brain = MobBrain {
            gait: Gait { previous_yaw: Some(0.15), previous_amount: 0.2, amount: 0.7, swing: 2.3 },
            fire_ticks: 18, fire_flag: true, growing_age: if id == 11 { -1200 } else { 0 },
            entity_random: Some(9981 + id), path_timer: 9, grounded: true,
            movement: Some(Box::new(ai::locomotion::Locomotion::new(position.as_dvec3(), 0.6, 1.8, 0.3, true))),
            ..Default::default()
        };
        assert!(brain.navigator.set(&world, ai::path::Walker {
            position: position.as_dvec3(), width: 0.6, height: 1.8,
            in_water: false, in_lava: false, max_fall: 3,
        }, false, Some(vec![IVec3::new(8, 1, 11), IVec3::new(8, 1, 12)])));
        sim.mob_brains.insert(id, brain);
        sim.command_entities.mobs.insert(id, command_entity_state::MobData {
            name: format!("Named {id}"), persistent: true, pitch: 17.,
            equipment: std::collections::BTreeMap::from([(0, Some(named.clone()))]),
            extra: std::collections::BTreeMap::from([("Silent".into(), DropNbt::Byte(0))]),
            ..Default::default()
        });
    }
    sim.trades.insert(14, crate::villagers::Trader::default());
    sim.trading_with = Some(14);
    // Reverse insertion order relative to Vec order for the two overlapping mobs.
    for (id, sequence) in [(11, 3), (12, 2), (13, 1), (14, 4)] {
        sim.pick_sections.insert(id, ([0, 0, 0], sequence, sim.total_ticks));
    }
    sim.pick_sequence = 4;
    sim.block_ticks.set_clock(789);
    sim.block_ticks.schedule_block_update(IVec3::new(4, 2, 4), 8, 100_000, -1);
    sim.block_ticks.falling.push(crate::block_ticks::FallingBlock::new(12 << 4,
        DVec3::new(10.5, 2., 10.5), 7878));
    sim.imported_container_chunks.insert((12, -4));
    sim.imported_entity_chunks.insert((-2, 18));
    sim.pending_golem_spawns.push(IVec3::new(3, 1, 4));
    sim.explosion_clouds.push((DVec3::ONE, 7));
    sim.powered_notes.insert(IVec3::new(4, 1, 4));
    sim.note_events.push((IVec3::new(4, 1, 4), 2, 7));
    (world, player, sim)
}

#[test]
fn presentation_projection_preserves_visible_state_and_rng_without_owner_work() {
    let (_, _, owner) = fixture();
    let before = serde_json::to_value(&owner).unwrap();
    let ordinary = owner.clone();
    let calls = independent_seed_calls_for_thread();
    let mut mirror = owner.presentation_clone();
    assert_eq!(independent_seed_calls_for_thread(), calls, "projection must not call constructors that consume independent seeds");
    assert_eq!(serde_json::to_value(&owner).unwrap(), before);
    assert_eq!(serde_json::to_value(&ordinary).unwrap(), before, "ordinary Clone retains save/owner semantics");
    let mut expected = before.clone();
    let mut actual = serde_json::to_value(&mirror).unwrap();
    for key in ["mob_brains", "block_ticks", "villager_homes", "pending_golem_spawns", "redstone",
        "imported_container_chunks", "imported_entity_chunks"] {
        expected.as_object_mut().unwrap().remove(key);
        actual.as_object_mut().unwrap().remove(key);
    }
    assert_eq!(actual, expected, "all other persisted fields match the full clone");
    assert_eq!((mirror.random_tick_lcg, mirror.player_ticks, mirror.voice_random, mirror.particle_random),
        (owner.random_tick_lcg, owner.player_ticks, owner.voice_random, owner.particle_random));
    assert_eq!((mirror.pick_sequence, &mirror.pick_sections), (owner.pick_sequence, &owner.pick_sections));
    assert_eq!(mirror.fire_flag, owner.fire_flag);
    for (&id, brain) in &owner.mob_brains {
        let copied = &mirror.mob_brains[&id];
        assert!(brain.movement.is_some() && !brain.navigator.no_path());
        assert!(copied.movement.is_none() && copied.navigator.no_path());
        assert!(ordinary.mob_brains[&id].movement.is_some() && !ordinary.mob_brains[&id].navigator.no_path());
        assert_eq!(copied.gait.sample(0.8, 0.35, id == 11), brain.gait.sample(0.8, 0.35, id == 11));
        assert_eq!((copied.growing_age, copied.fire_flag, copied.entity_random),
            (brain.growing_age, brain.fire_flag, brain.entity_random));
        let mut expected = serde_json::to_value(brain).unwrap();
        expected.as_object_mut().unwrap().remove("movement");
        let mut actual = serde_json::to_value(copied).unwrap();
        actual.as_object_mut().unwrap().remove("movement");
        assert_eq!(actual, expected, "all serialized brain scalars survive");
    }
    assert!(mirror.pending_golem_spawns.is_empty() && mirror.imported_container_chunks.is_empty()
        && mirror.imported_entity_chunks.is_empty() && mirror.explosion_clouds.is_empty()
        && mirror.powered_notes.is_empty() && mirror.note_events.is_empty());
    assert!(serde_json::to_value(&mirror.block_ticks).unwrap()["queue"].as_object().unwrap().is_empty());
    assert!(!before["block_ticks"]["queue"].as_object().unwrap().is_empty());
    mirror.inventory.slots[0].as_mut().unwrap().count = 7;
    mirror.command_entities.mobs.get_mut(&12).unwrap().name = "changed mirror".into();
    mirror.block_ticks.set_clock(1);
    assert_eq!(serde_json::to_value(&owner).unwrap(), before, "the rendering copy cannot alter authoritative saves");
}

#[test]
fn presentation_projection_keeps_exact_entity_pick_order_and_falling_candidates() {
    let (world, mut player, mut owner) = fixture();
    let ordinary = owner.clone();
    let mirror = owner.presentation_clone();
    let hit = ordinary.pick_at(&world, &player, 1.).unwrap();
    assert_eq!(hit.entity(), Some(LocalTarget::Mob(13)), "overlap fixture must exercise insertion order");
    for alpha in [0., 0.25, 0.75, 1.] {
        assert_eq!(format!("{:?}", mirror.pick_at(&world, &player, alpha)),
            format!("{:?}", ordinary.pick_at(&world, &player, alpha)));
    }
    owner.mobs.clear();
    owner.block_ticks.falling.clear();
    owner.block_ticks.falling.push(crate::block_ticks::FallingBlock::new(12 << 4,
        DVec3::new(8.5, 2., 11.5), 9988));
    player.set_pose_f64([8.5, 1., 8.5], 0., 0., true);
    let expected = owner.pick(&world, &player).unwrap();
    assert_eq!(expected.entity(), Some(LocalTarget::Falling(0)));
    assert_eq!(format!("{:?}", owner.presentation_clone().pick(&world, &player)),
        format!("{:?}", owner.clone().pick(&world, &player)));
}

#[test]
#[ignore = "loads installed resources; compares CPU meshes/UI, no GPU or world files"]
fn presentation_projection_matches_full_clone_models_inventory_hud_and_windows() {
    use crate::{mesh::Frustum, models::{self, ModelContext}, ui::{self, Frame, Gui, Screen, Stats}};
    use glam::Mat4;
    let assets = crate::validation::assets();
    let resources = crate::validation::resources();
    let (world, player, owner) = fixture();
    let ordinary = owner.clone();
    let mut spare = owner.presentation_clone().mob_brains;
    for brain in spare.values_mut() {
        brain.growing_age = 190; brain.fire_flag = false; brain.gait.amount = 17.;
    }
    let mirror = owner.presentation_clone_reusing(&mut spare);
    assert!(spare.is_empty());
    let settings = Settings { entity_culling: false, ..Default::default() };
    let eye = player.eye(0.35);
    let frustum = Frustum::new(Mat4::perspective_rh(70f32.to_radians(), 16. / 9., 0.05, 256.)
        * Mat4::look_to_rh(eye, player.direction(), Vec3::Y));
    for perspective in 0..3 {
        let render = |sim| models::build(ModelContext { sim, player: &player, world: &world,
            assets, resources, settings: &settings, frustum: &frustum, viewport_height: 1080,
            eye, alpha: 0.35, perspective, render_hand: true, hitboxes: true });
        let expected = render(&ordinary);
        let actual = render(&mirror);
        assert!(expected.visible > 0 && !expected.opaque.vertices.is_empty(), "fixture must draw entities");
        assert_eq!((actual.visible, actual.total, actual.culled), (expected.visible, expected.total, expected.culled));
        for (a, b) in [(&actual.opaque, &expected.opaque), (&actual.translucent, &expected.translucent)] {
            assert!(bytemuck::cast_slice::<_, u8>(&a.vertices) == bytemuck::cast_slice::<_, u8>(&b.vertices),
                "model vertex bytes differ for perspective {perspective}");
            assert_eq!(a.indices, b.indices);
        }
    }
    for screen in [Screen::Playing, Screen::Inventory, Screen::Creative, Screen::Chest,
        Screen::Trading, Screen::Sleeping, Screen::Chat, Screen::Pause] {
        let mut gui = Gui::default();
        gui.screen = screen;
        gui.cursor = [210., 140.];
        gui.container = Some(IVec3::new(3, 1, 3));
        gui.trader = Some(14);
        gui.debug = true;
        let mut expected = Frame::default();
        let mut actual = Frame::default();
        expected.set_viewport([1920, 1080], &settings);
        actual.set_viewport([1920, 1080], &settings);
        // GuiInventory writes its previous-frame mouse Cell while painting.
        // Reset that input so both copies see the same previous frame; creative
        // indicators reuse the same total_ticks and therefore do not advance.
        let previous_mouse = gui.inventory_mouse.get();
        ui::paint(&mut expected, &gui, &settings, resources, assets, Some(&ordinary), &Stats::default());
        gui.inventory_mouse.set(previous_mouse);
        ui::paint(&mut actual, &gui, &settings, resources, assets, Some(&mirror), &Stats::default());
        assert!(!expected.vertices.is_empty(), "fixture must draw {screen:?}");
        assert!(bytemuck::cast_slice::<_, u8>(&actual.vertices) == bytemuck::cast_slice::<_, u8>(&expected.vertices),
            "UI vertex bytes differ for {screen:?}");
        assert_eq!(actual.buttons.len(), expected.buttons.len());
        for (a, b) in actual.buttons.iter().zip(&expected.buttons) {
            assert_eq!((a.rect.x, a.rect.y, a.rect.w, a.rect.h, a.enabled),
                (b.rect.x, b.rect.y, b.rect.w, b.rect.h, b.enabled));
            assert_eq!(format!("{:?}", a.action), format!("{:?}", b.action));
        }
    }
}

#[test]
fn profiled_presentation_copy_keeps_values_rng_and_exact_collection_counts() {
    let (_, _, owner) = fixture();
    let calls = independent_seed_calls_for_thread();
    let ordinary = owner.presentation_clone();
    let (profiled, profile) = owner.presentation_clone_profiled();
    assert_eq!(independent_seed_calls_for_thread(), calls);
    assert_eq!(serde_json::to_value(&ordinary).unwrap(), serde_json::to_value(&profiled).unwrap());
    let counts = profile.counts().collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(counts["command_mobs"], owner.command_entities.mobs.len());
    assert_eq!(counts["mob_brains"], owner.mob_brains.len());
    assert_eq!(counts["pick_sections"], owner.pick_sections.len());
    assert_eq!(counts["trades"], owner.trades.len());
    assert_eq!(counts["containers"], owner.containers.len());
    assert_eq!(counts["maps"], owner.maps.data.len());
    assert_eq!(profile.timings().count(), 12);
    assert!(profile.timings().all(|(_, ms)| ms.is_finite() && ms >= 0.));
    assert_eq!(ordinary.voice_random, profiled.voice_random);
    assert_eq!(ordinary.particle_random, profiled.particle_random);
}


#[test]
fn brain_projection_reuses_nodes_with_exact_keys_and_refreshes_visible_state() {
    let (_, _, owner) = fixture();
    let expected = owner.presentation_clone();
    let owner_wire = serde_json::to_value(&owner).unwrap();
    let mut spare = owner.presentation_clone().mob_brains;
    let addresses = spare.iter().map(|(&id, brain)| (id, std::ptr::from_ref(brain))).collect::<Vec<_>>();
    for brain in spare.values_mut() {
        brain.growing_age = 123; brain.fire_flag = false; brain.entity_random = Some(1);
        brain.gait.amount = 9.; brain.path_timer = 0;
    }
    let calls = independent_seed_calls_for_thread();
    let (mirror, profile) = owner.presentation_clone_profiled_reusing(&mut spare);
    assert!(spare.is_empty());
    assert_eq!(independent_seed_calls_for_thread(), calls);
    assert_eq!(profile.counts().find(|(name, _)| *name == "mob_brains_reused").unwrap().1, 1);
    assert_eq!(addresses, mirror.mob_brains.iter().map(|(&id, b)| (id, std::ptr::from_ref(b))).collect::<Vec<_>>());
    assert_eq!(serde_json::to_value(&mirror).unwrap(), serde_json::to_value(&expected).unwrap());
    for (&id, brain) in &mirror.mob_brains {
        assert_eq!(brain.gait.sample(0.8, 0.35, id == 11), owner.mob_brains[&id].gait.sample(0.8, 0.35, id == 11));
        assert_eq!(brain.fire_flag, owner.mob_brains[&id].fire_flag);
        assert!(brain.movement.is_none() && brain.navigator.no_path());
    }
    assert_eq!(serde_json::to_value(&owner).unwrap(), owner_wire);
}

#[test]
fn brain_projection_key_mismatch_keeps_old_spare_for_worker_retirement() {
    let (_, _, owner) = fixture();
    let mut spare = owner.presentation_clone().mob_brains;
    let brain = spare.remove(&11).unwrap(); spare.insert(99, brain);
    let old_wire = serde_json::to_value(&spare).unwrap();
    let old_addresses = spare.iter().map(|(&id, brain)| (id, std::ptr::from_ref(brain))).collect::<Vec<_>>();
    let (mut retired_mirror, profile) = owner.presentation_clone_profiled_reusing(&mut spare);
    assert_eq!(profile.counts().find(|(name, _)| *name == "mob_brains_reused").unwrap().1, 0);
    assert_eq!(serde_json::to_value(&spare).unwrap(), old_wire);
    assert_eq!(old_addresses, spare.iter().map(|(&id, b)| (id, std::ptr::from_ref(b))).collect::<Vec<_>>());
    assert_eq!(serde_json::to_value(&retired_mirror).unwrap(), serde_json::to_value(owner.presentation_clone()).unwrap());
    // Exact O(1) acceptance handoff: retire old spare; retain current mirror map.
    std::mem::swap(&mut retired_mirror.mob_brains, &mut spare);
    assert_eq!(serde_json::to_value(&retired_mirror.mob_brains).unwrap(), old_wire);
    assert_eq!(spare.keys().collect::<Vec<_>>(), owner.mob_brains.keys().collect::<Vec<_>>());
    let (_, profile) = owner.presentation_clone_profiled_reusing(&mut spare);
    assert_eq!(profile.counts().find(|(name, _)| *name == "mob_brains_reused").unwrap().1, 1);
}

#[test]
fn brain_projection_repeated_handoffs_handle_add_remove_empty_and_owner_mutation() {
    let (_, _, mut owner) = fixture();
    let mut spare = Default::default();
    let calls = independent_seed_calls_for_thread();
    for step in 0..32u64 {
        if step == 7 { owner.mob_brains.remove(&11); }
        if step == 13 { owner.mob_brains.insert(501, MobBrain::default()); }
        if step == 20 { owner.mob_brains.clear(); }
        if step == 23 { owner.mob_brains.insert(99, MobBrain::default()); }
        for brain in owner.mob_brains.values_mut() {
            brain.growing_age = -(step as i32); brain.entity_random = Some(step);
            brain.fire_flag = step % 2 == 0; brain.gait.swing = step as f32 * 0.125;
        }
        let expected = owner.presentation_clone();
        let mut mirror = owner.presentation_clone_reusing(&mut spare);
        assert_eq!(serde_json::to_value(&mirror).unwrap(), serde_json::to_value(&expected).unwrap());
        for (&id, brain) in &mirror.mob_brains {
            assert_eq!(brain.gait.sample(1., 0.4, true), owner.mob_brains[&id].gait.sample(1., 0.4, true));
            assert_eq!(brain.fire_flag, owner.mob_brains[&id].fire_flag);
        }
        std::mem::swap(&mut mirror.mob_brains, &mut spare);
        // Production sends the remaining mirror to the existing retirement queue.
        drop(mirror);
    }
    assert_eq!(independent_seed_calls_for_thread(), calls);
}
