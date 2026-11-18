use super::*;
use serde_json::{json, Value};
fn item(id: u16, count: i32, damage: i32, level: i32, unbreakable: bool) -> Item {
    Item {
        id,
        count,
        damage,
        unbreakable,
        enchantments: vec![[34, level as i16]],
    }
}
fn state(item: Option<&Item>) -> Value {
    item.map_or(Value::Null, |s| json!([s.id, s.count, s.damage]))
}
fn events(events: &[Broken]) -> (Value, Value) {
    (
        json!(events
            .iter()
            .map(|e| json!([e.id, e.count, e.damage]))
            .collect::<Vec<_>>()),
        json!(events
            .iter()
            .map(|e| json!(["random.break", 0.8f32.to_bits(), e.pitch.to_bits()]))
            .collect::<Vec<_>>()),
    )
}
#[test]
fn item_durability_armor_and_protection_rng_match_original() {
    let data: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/vanilla189-armor.json")).unwrap();
    for (case, row) in data["damage"].as_array().unwrap().iter().enumerate() {
        let input = &row["input"];
        let seed = input[6].as_str().unwrap().parse::<i64>().unwrap();
        let mut entity = JavaRandom::new(seed);
        let mut world = JavaRandom::new(seed ^ 0x123456789abcdef);
        let mut stack = item(
            input[0].as_u64().unwrap() as u16,
            2,
            input[1].as_i64().unwrap() as i32,
            input[3].as_i64().unwrap() as i32,
            input[4].as_bool().unwrap(),
        );
        let broken = damage_item(
            &mut stack,
            input[2].as_i64().unwrap() as i32,
            input[5].as_bool().unwrap(),
            &mut entity,
            &mut world,
        )
        .into_iter()
        .collect::<Vec<_>>();
        let (breaks, sounds) = events(&broken);
        assert_eq!(state(Some(&stack)), row["stack"], "item case{case}");
        assert_eq!(breaks, row["breaks"], "item case{case} broken");
        assert_eq!(sounds, row["sounds"], "item case{case} sounds");
        assert_eq!(
            entity.next_long().to_string(),
            row["entity_tail"].as_str().unwrap(),
            "item case{case} entity RNG"
        );
        assert_eq!(
            world.next_long().to_string(),
            row["world_tail"].as_str().unwrap(),
            "item case{case} world RNG"
        );
    }
    for (case, row) in data["armor"].as_array().unwrap().iter().enumerate() {
        let seed = row["seed"].as_str().unwrap().parse::<i64>().unwrap();
        let mut entity = JavaRandom::new(seed);
        let mut world = JavaRandom::new(seed ^ 0x123456789abcdef);
        let mut items = std::array::from_fn(|i| {
            Some(item(
                row["initial"][i][0].as_u64().unwrap() as u16,
                1,
                row["initial"][i][1].as_i64().unwrap() as i32,
                row["level"].as_i64().unwrap() as i32,
                row["unbreakable"].as_bool().unwrap(),
            ))
        });
        let broken = damage_armor(
            &mut items,
            f32::from_bits(row["amount"].as_u64().unwrap() as u32),
            row["creative"].as_bool().unwrap(),
            &mut entity,
            &mut world,
        );
        let (breaks, sounds) = events(&broken);
        assert_eq!(
            json!(items.iter().map(|i| state(i.as_ref())).collect::<Vec<_>>()),
            row["armor"],
            "armor case{case}"
        );
        assert_eq!(breaks, row["breaks"], "armor case{case} broken");
        assert_eq!(sounds, row["sounds"], "armor case{case} sounds");
        assert_eq!(
            entity.next_long().to_string(),
            row["entity_tail"].as_str().unwrap(),
            "armor case{case} entity RNG"
        );
        assert_eq!(
            world.next_long().to_string(),
            row["world_tail"].as_str().unwrap(),
            "armor case{case} world RNG"
        );
    }
    for (case, row) in data["protection"].as_array().unwrap().iter().enumerate() {
        let items = row[0]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                s.as_array().map_or(vec![], |a| {
                    a.iter()
                        .map(|e| [e[0].as_i64().unwrap() as i16, e[1].as_i64().unwrap() as i16])
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        let source = row[1].as_u64().unwrap();
        let mut random = JavaRandom::new(row[2].as_str().unwrap().parse().unwrap());
        let result = protection(
            items.iter().map(Vec::as_slice),
            Source {
                creative_harm: source == 7,
                fire: source == 1,
                fall: source == 2,
                explosion: source == 3,
                projectile: source == 4,
                ..Default::default()
            },
            &mut random,
        );
        assert_eq!(
            result as i64,
            row[3].as_i64().unwrap(),
            "protection case{case}"
        );
        assert_eq!(
            random.next_long().to_string(),
            row[4].as_str().unwrap(),
            "protection case{case} RNG"
        );
    }
}
#[test]
fn combined_blocking_armor_break_resistance_protection_and_absorption_match_original() {
    let data: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/vanilla189-armor.json")).unwrap();
    for (case, row) in data["defense"].as_array().unwrap().iter().enumerate() {
        let seed = row["seed"].as_str().unwrap().parse::<i64>().unwrap();
        let mut entity = JavaRandom::new(seed);
        let mut world = JavaRandom::new(seed ^ 0x123456789abcdef);
        let mut enchantment = JavaRandom::new(seed ^ 0x55555);
        let mut items = std::array::from_fn(|i| {
            let v = &row["initial"][i];
            Some(Item {
                id: v[0].as_u64().unwrap() as u16,
                count: 1,
                damage: v[1].as_i64().unwrap() as i32,
                unbreakable: row["unbreakable"].as_bool().unwrap(),
                enchantments: v[2]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| [e[0].as_i64().unwrap() as i16, e[1].as_i64().unwrap() as i16])
                    .collect(),
            })
        });
        let id = row["source"].as_u64().unwrap();
        let source = Source {
            fire: id == 1,
            fall: id == 2,
            explosion: id == 3,
            projectile: id == 4,
            creative_harm: id == 7,
            unblockable: matches!(id, 2 | 5 | 6 | 7),
            absolute: id == 6,
            ..Default::default()
        };
        let amp = row["resistance"].as_i64().unwrap() as i32;
        let context = DefenseContext {
            source,
            creative: row["creative"].as_bool().unwrap(),
            blocking: row["blocking"].as_bool().unwrap(),
            resistance: (amp != -99).then_some(amp),
            absorption: f32::from_bits(row["initial_absorption"].as_u64().unwrap() as u32),
        };
        let result = defend(
            &mut items,
            f32::from_bits(row["amount"].as_u64().unwrap() as u32),
            context,
            &mut entity,
            &mut world,
            &mut enchantment,
        );
        let (breaks, sounds) = events(&result.breaks);
        let exhaustion = if result.health_damage != 0. && !context.creative && !source.unblockable {
            0.3f32
        } else {
            0.
        };
        assert_eq!(
            (20f32 - result.health_damage).clamp(0., 20.).to_bits() as u64,
            row["health"].as_u64().unwrap(),
            "defense{case} health"
        );
        assert_eq!(
            result.absorption.to_bits() as u64,
            row["absorption"].as_u64().unwrap(),
            "defense{case} absorption"
        );
        assert_eq!(
            exhaustion.to_bits() as u64,
            row["exhaustion"].as_u64().unwrap(),
            "defense{case} exhaustion"
        );
        assert_eq!(
            json!(items.iter().map(|i| state(i.as_ref())).collect::<Vec<_>>()),
            row["armor"],
            "defense{case} armor"
        );
        assert_eq!(breaks, row["breaks"], "defense{case} breaks");
        assert_eq!(sounds, row["sounds"], "defense{case} sounds");
        assert_eq!(
            entity.next_long().to_string(),
            row["entity_tail"].as_str().unwrap(),
            "defense{case} entity RNG"
        );
        assert_eq!(
            world.next_long().to_string(),
            row["world_tail"].as_str().unwrap(),
            "defense{case} world RNG"
        );
        assert_eq!(
            enchantment.next_long().to_string(),
            row["enchantment_tail"].as_str().unwrap(),
            "defense{case} enchantment RNG"
        );
    }
    assert_eq!(data["defense"].as_array().unwrap().len(), 2560);
}
