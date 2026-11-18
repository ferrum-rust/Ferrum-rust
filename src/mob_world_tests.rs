use super::*;
use crate::nbt::Tag;
use crate::vanilla::{
    drops::DropNbt,
    ray_trace::{Hit, Kind},
};
use std::sync::Arc;

#[test]
fn original_anvil_entities_import_once_and_keep_species_data_after_reload() {
    let mut world = tests::flat_world();
    world.imported = true;
    let chunk = Arc::make_mut(world.chunks.get_mut(&(0, 0)).unwrap());
    for (index, &kind) in MobKind::ALL.iter().enumerate() {
        let tag = Tag::Compound(
            [
                ("id".into(), Tag::String(kind.name().into())),
                (
                    "Pos".into(),
                    Tag::List(vec![Tag::Double(4.5), Tag::Double(1.), Tag::Double(3.5)]),
                ),
                (
                    "Rotation".into(),
                    Tag::List(vec![Tag::Float(37.), Tag::Float(-12.)]),
                ),
                ("HealF".into(), Tag::Float(3.5)),
                ("CustomName".into(), Tag::String(kind.name().into())),
                ("Age".into(), Tag::Int(-600)),
                ("Variant".into(), Tag::Int(1025)),
                ("Elder".into(), Tag::Byte(1)),
                (
                    "ArmorItem".into(),
                    Tag::Compound(
                        [(
                            "id".into(),
                            Tag::String("minecraft:iron_horse_armor".into()),
                        )]
                        .into(),
                    ),
                ),
            ]
            .into(),
        );
        chunk
            .entities
            .push(crate::villagers::read_entity(&tag, (0, 0), index).unwrap());
    }
    let mut sim = Simulation::new(Mode::Creative, 189);
    crate::villagers::sync_import(&world, &mut sim);
    let mut saved: Simulation = serde_json::from_slice(&serde_json::to_vec(&sim).unwrap()).unwrap();
    crate::villagers::sync_import(&world, &mut saved);
    assert_eq!(saved.mobs.len(), 32);
    for mob in &saved.mobs {
        let data = &saved.command_entities.mobs[&mob.id];
        assert_eq!(data.name, mob.kind.name());
        assert_eq!(data.pitch, -12.);
        assert_eq!(mob.health, 3.5);
        assert_eq!(data.extra["Variant"], DropNbt::Int(1025));
        assert!(matches!(data.extra["ArmorItem"], DropNbt::Compound(_)));
        assert_eq!(saved.mob_brains[&mob.id].growing_age, -600);
    }
}

fn top(block: IVec3) -> Hit {
    Hit {
        kind: Kind::Block,
        position: block,
        face: 1,
        point: block.as_dvec3() + glam::DVec3::new(0.5, 1., 0.5),
    }
}

#[test]
fn pumpkin_and_skull_placements_consume_complete_patterns_only() {
    let mut world = tests::flat_world();
    let mut sim = Simulation::new(Mode::Creative, 189);
    let player = Player::new(world.spawn);
    let mut out = Outcome::default();
    let base = IVec3::new(4, 1, 4);
    for (block, kind) in [
        (80, MobKind::SnowGolem),
        (42, MobKind::IronGolem),
        (88, MobKind::Wither),
    ] {
        let arms = base + IVec3::Y;
        world.set(base, block << 4);
        world.set(arms, block << 4);
        if block != 80 {
            for dx in [-1, 1] {
                world.set(arms + IVec3::X * dx, block << 4);
            }
        }
        let old = sim.mobs.len();
        if block == 88 {
            let mut skull = Stack::new(397, 1);
            skull.meta = 1;
            for dx in [-1, 1] {
                sim.place_held_block(
                    &mut world,
                    &player,
                    &skull,
                    top(arms + IVec3::X * dx),
                    &mut out,
                );
            }
            assert_eq!(old, sim.mobs.len());
            sim.place_held_block(&mut world, &player, &skull, top(arms), &mut out);
        } else {
            sim.place_held_block(&mut world, &player, &Stack::new(86, 1), top(arms), &mut out);
        }
        assert_eq!(old + 1, sim.mobs.len(), "{kind:?}");
        let mob = sim.mobs.last().unwrap();
        assert_eq!(mob.kind, kind);
        if kind == MobKind::Wither {
            assert_eq!(
                sim.command_entities.mobs[&mob.id].extra["Invul"],
                DropNbt::Int(220)
            );
        }
        assert_eq!(world.get(base), 0);
    }
}

#[test]
fn tame_wolf_feed_command_and_mooshroom_shearing_use_live_inventory() {
    let world = tests::flat_world();
    let mut sim = Simulation::new(Mode::Survival, 189);
    sim.local_name = "Player".into();
    let player = Player::new(world.spawn);
    let mut out = Outcome::default();
    let wolf = sim.spawn_registered(MobKind::Wolf, world.spawn, 0.);
    sim.inventory.slots[0] = Some(Stack::new(352, 64));
    for _ in 0..64 {
        assert!(sim.interact_pet_or_mooshroom(0, &player, &mut out));
        if species_ai::tamed(sim.command_entities.mobs.get(&wolf)) {
            break;
        }
    }
    assert_eq!(
        sim.command_entities.mobs[&wolf].extra["Sitting"],
        DropNbt::Byte(1)
    );
    sim.inventory.slots[0] = None;
    assert!(sim.interact_pet_or_mooshroom(0, &player, &mut out));
    assert_eq!(
        sim.command_entities.mobs[&wolf].extra["Sitting"],
        DropNbt::Byte(0)
    );
    sim.mobs[0].health = 5.;
    sim.inventory.slots[0] = Some(Stack::new(364, 1));
    assert!(sim.interact_pet_or_mooshroom(0, &player, &mut out));
    assert_eq!(sim.mobs[0].health, 13.);
    assert!(sim.inventory.held().is_none());
    sim.spawn_registered(MobKind::Mooshroom, world.spawn, 0.);
    sim.inventory.slots[0] = Some(Stack::new(281, 1));
    assert!(sim.interact_pet_or_mooshroom(1, &player, &mut out));
    assert_eq!(sim.inventory.held().unwrap().id, 282);
    sim.inventory.slots[0] = Some(Stack::new(359, 1));
    assert!(sim.interact_pet_or_mooshroom(1, &player, &mut out));
    assert_eq!(sim.mobs[1].kind, MobKind::Cow);
    assert_eq!(sim.drops.len(), 5);
}
