use super::*;
use crate::vanilla::drops::DropNbt;
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn bits(value: &Value) -> f32 {
    f32::from_bits(value.as_u64().unwrap() as u32)
}
fn tagged_item(id: u16, damage: u16, unbreakable: bool, levels: &Value) -> Stack {
    let enchantments = levels
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            DropNbt::Compound(BTreeMap::from([
                (
                    "id".to_string(),
                    DropNbt::Short(v[0].as_i64().unwrap() as i16),
                ),
                (
                    "lvl".to_string(),
                    DropNbt::Short(v[1].as_i64().unwrap() as i16),
                ),
            ]))
        })
        .collect();
    let mut stack = Stack::new(id, 1).with_nbt(DropNbt::Compound(BTreeMap::from([
        ("Unbreakable".into(), DropNbt::Byte(i8::from(unbreakable))),
        (
            "ench".into(),
            DropNbt::List {
                element: 10,
                values: enchantments,
            },
        ),
        ("custom".into(), DropNbt::String("retain me".into())),
    ])));
    stack.damage = damage;
    stack
}
fn simulation(row: &Value) -> Simulation {
    let seed = row["seed"].as_str().unwrap().parse::<i64>().unwrap();
    let mut sim = Simulation {
        mode: if row["creative"] == true {
            Mode::Creative
        } else {
            Mode::Survival
        },
        absorption: bits(&row["initial_absorption"]),
        player_random_state: JavaRandom::new(seed).state(),
        enchantment_random_state: JavaRandom::new(seed ^ 0x55555).state(),
        ..Default::default()
    };
    sim.block_ticks
        .set_world_random(&JavaRandom::new(seed ^ 0x123456789abcdef));
    for (i, v) in row["initial"].as_array().unwrap().iter().enumerate() {
        if !v.is_null() {
            sim.inventory.slots[36 + i] = Some(tagged_item(
                v[0].as_u64().unwrap() as u16,
                v[1].as_u64().unwrap() as u16,
                row["unbreakable"] == true,
                &v[2],
            ));
        }
    }
    let amp = row["resistance"].as_i64().unwrap() as i32;
    if amp != -99 {
        sim.add_food_effect(Effect::new(11, 100, amp));
    }
    if row["blocking"] == true {
        let sword = Stack::new(267, 1);
        sim.item_use.start_for(sword.identity, 72000);
        sim.inventory.slots[0] = Some(sword);
    }
    sim
}
fn assert_result(sim: &Simulation, row: &Value, case: usize) {
    let armor = sim.inventory.slots[36..40]
        .iter()
        .map(|s| {
            s.as_ref().map_or(Value::Null, |s| {
                assert!(s.nbt.is_some(), "case{case} tag was discarded");
                json!([s.id, s.count, s.damage])
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(json!(armor), row["armor"], "case{case} armor");
    assert_eq!(
        sim.health.to_bits() as u64,
        row["health"].as_u64().unwrap(),
        "case{case} health"
    );
    assert_eq!(
        sim.absorption.to_bits() as u64,
        row["absorption"].as_u64().unwrap(),
        "case{case} absorption"
    );
    assert_eq!(
        sim.exhaustion.to_bits() as u64,
        row["exhaustion"].as_u64().unwrap(),
        "case{case} exhaustion"
    );
    assert_eq!(
        JavaRandom::from_state(sim.player_random_state)
            .next_long()
            .to_string(),
        row["entity_tail"].as_str().unwrap(),
        "case{case} entity RNG"
    );
    assert_eq!(
        sim.block_ticks.world_random().next_long().to_string(),
        row["world_tail"].as_str().unwrap(),
        "case{case} world RNG"
    );
    assert_eq!(
        JavaRandom::from_state(sim.enchantment_random_state)
            .next_long()
            .to_string(),
        row["enchantment_tail"].as_str().unwrap(),
        "case{case} enchantment RNG"
    );
    let sounds = sim
        .damage_sounds
        .iter()
        .map(|(name, volume, pitch)| {
            let event = match *name {
                "random.break" | "game.player.die" | "game.player.hurt" => *name,
                _ => panic!("Unexpected damage sound {name}"),
            };
            json!([event, volume.to_bits(), pitch.to_bits()])
        })
        .collect::<Vec<_>>();
    assert_eq!(json!(sounds), row["sounds"], "case{case} sounds");
}
#[test]
fn connected_equipped_armor_blocking_potions_and_rng_match_original_damage_entity() {
    let data: Value =
        serde_json::from_str(include_str!("../tests/fixtures/vanilla189-armor.json")).unwrap();
    for (case, row) in data["defense"].as_array().unwrap().iter().enumerate() {
        let mut sim = simulation(row);
        let id = row["source"].as_u64().unwrap();
        let source = HitSource {
            fire: id == 1,
            fall: id == 2,
            explosion: id == 3,
            projectile: id == 4,
            creative_harm: id == 7,
            unblockable: matches!(id, 2 | 5 | 6 | 7),
            absolute: id == 6,
            ..Default::default()
        };
        sim.damage_entity(
            bits(&row["amount"]),
            source,
            sim.effects.get(11).map(|e| e.amplifier),
        );
        assert_result(&sim, row, case);
    }
}
#[test]
fn falling_object_helmet_wear_before_hit_window_matches_original_attack() {
    let data: Value = serde_json::from_str(include_str!(
        "../tests/fixtures/vanilla189-armor-impacts.json"
    ))
    .unwrap();
    for (case, row) in data.as_array().unwrap().iter().enumerate() {
        let mut sim = simulation(row);
        sim.hurt_ticks = row["initial_timer"].as_u64().unwrap() as u8;
        sim.last_damage = bits(&row["initial_last"]);
        let accepted = sim.hurt_from(
            bits(&row["amount"]),
            HitSource {
                falling_object: true,
                ..Default::default()
            },
        );
        assert_eq!(
            accepted,
            row["accepted"].as_bool().unwrap(),
            "case{case} accepted"
        );
        assert_eq!(
            u64::from(sim.hurt_ticks),
            row["timer"].as_u64().unwrap(),
            "case{case} timer"
        );
        assert_eq!(
            u64::from(sim.last_damage.to_bits()),
            row["last"].as_u64().unwrap(),
            "case{case} last damage"
        );
        assert_result(&sim, row, case);
    }
    assert_eq!(data.as_array().unwrap().len(), 960);
}
#[test]
fn falling_impact_uses_strict_bounds_and_stronger_hit_difference() {
    let player = Player::new(Vec3::new(0., 64., 0.));
    let mob = |id, kind, x| Mob {
        id,
        kind,
        position: Vec3::new(x, 64., 0.),
        previous: Vec3::new(x, 64., 0.),
        velocity: Vec3::ZERO,
        yaw: 0.,
        health: kind.health(),
        age: 0,
        turn_ticks: 0,
        hurt: 0,
        fuse: 0,
        swing_ticks: 0,
        death_ticks: 0,
        eat_ticks: 0,
        attack_ticks: 0,
        rose_ticks: 0,
        prev_fuse: 0,
    };
    let mut sim = Simulation {
        mobs: vec![mob(1, MobKind::Zombie, 0.), mob(2, MobKind::Cow, 0.75)],
        ..Default::default()
    };
    let mut impact = crate::block_ticks::FallingImpact {
        bounds: player.precise_bounds(),
        anvil: true,
        amount: 10.,
    };
    sim.falling_impact(&impact, &player);
    assert_eq!(sim.health, 10.);
    assert_eq!(sim.mobs[0].health, 20. - 10. * 23. / 25.);
    assert_eq!(
        sim.mobs[1].health, 10.,
        "Merely touching a face must not hit"
    );
    impact.amount = 4.;
    sim.falling_impact(&impact, &player);
    assert_eq!(sim.health, 10.);
    impact.amount = 12.;
    sim.falling_impact(&impact, &player);
    assert_eq!(sim.health, 8.);
    assert_eq!(sim.mobs[0].health, 20. - 10. * 23. / 25. - 2. * 23. / 25.);
}
#[test]
fn motion_effects_and_depth_strider_use_original_armor_order() {
    let mut sim = Simulation::default();
    let mut player = Player::new(Vec3::ZERO);
    for (id, amp) in [(1, 2), (2, 1), (8, 3), (15, 0)] {
        sim.add_food_effect(Effect::new(id, 100, amp));
    }
    sim.inventory.slots[36] = Some(tagged_item(313, 0, false, &json!([[8, 1], [8, 7]])));
    sim.inventory.slots[39] = Some(tagged_item(310, 0, false, &json!([[8, 3]])));
    sim.update_motion_effects(&mut player);
    assert_eq!(
        (
            player.speed_boost,
            player.slowness,
            player.jump_boost,
            player.blinded,
            player.depth_strider
        ),
        (Some(2), Some(1), Some(3), true, 3)
    );
}
