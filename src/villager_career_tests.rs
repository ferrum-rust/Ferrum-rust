use super::*;
use crate::vanilla::item_nbt;
use serde_json::{json, Value};

fn tail(state: u64) -> String {
    JavaRandom::from_state(state).next_long().to_string()
}
fn encoded(stack: Option<&Stack>, tags: &[Vec<u8>]) -> Value {
    let Some(stack) = stack else {
        return Value::Null;
    };
    let tag = stack.nbt.as_deref().map(|tag| {
        let bytes = item_nbt::encode(tag).unwrap();
        tags.iter()
            .position(|value| value == &bytes)
            .expect("Original fixture contains generated tag")
    });
    let damage = if armor::maximum_damage(stack.id) > 0 {
        stack.damage
    } else {
        stack.meta
    };
    json!([stack.id, damage, stack.count, tag])
}
#[test]
fn every_villager_profession_career_level_offer_and_rng_matches_original() {
    let data: Value = serde_json::from_str(include_str!(
        "../tests/fixtures/vanilla189-villager-careers.json"
    ))
    .unwrap();
    let tags = data["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            let hex = v.as_str().unwrap();
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect::<Vec<_>>();
            item_nbt::encode(&item_nbt::decode(&bytes).unwrap().unwrap()).unwrap()
        })
        .collect::<Vec<_>>();
    for (case, row) in data["careers"].as_array().unwrap().iter().enumerate() {
        let mut trader = Trader::seeded(
            row[0].as_u64().unwrap() as u8,
            row[1].as_str().unwrap().parse().unwrap(),
        );
        for (level, expected) in row[2].as_array().unwrap().iter().enumerate() {
            let previous = trader.offers.len();
            if level == 0 {
                trader.ensure_offers().unwrap();
            } else {
                trader.populate().unwrap();
            }
            let added = trader.offers[previous..]
                .iter()
                .map(|o| {
                    json!([
                        encoded(Some(&o.cost), &tags),
                        encoded(o.second.as_ref(), &tags),
                        encoded(Some(&o.output), &tags),
                        o.uses,
                        o.max_uses,
                        o.reward_exp
                    ])
                })
                .collect::<Vec<_>>();
            assert_eq!(
                json!([
                    trader.career.id,
                    trader.career.level,
                    added,
                    tail(trader.career.random_state)
                ]),
                *expected,
                "career{case}, level{level}"
            );
        }
    }
    assert_eq!(data["careers"].as_array().unwrap().len(), 352);
}
#[test]
fn trade_experience_reputation_reopening_and_restock_match_original() {
    let data: Value = serde_json::from_str(include_str!(
        "../tests/fixtures/vanilla189-villager-restock.json"
    ))
    .unwrap();
    for (case, row) in data.as_array().unwrap().iter().enumerate() {
        let mut trader = Trader::seeded(
            row[0].as_u64().unwrap() as u8,
            row[1].as_str().unwrap().parse().unwrap(),
        );
        let member = row[2].as_bool().unwrap();
        let rewards = row[3].as_bool().unwrap();
        let mut reputation = 0i32;
        for (step, expected) in row[4].as_array().unwrap().iter().enumerate() {
            let command = &expected["op"];
            let arg = command[1].as_i64().unwrap() as usize;
            let mut events = Vec::new();
            match command[0].as_str().unwrap() {
                "init" => {
                    trader.ensure_offers().unwrap();
                    trader.offers[arg].reward_exp = rewards;
                }
                "customer" => trader.customer = (arg != 0).then(|| "FoodOracle".to_owned()),
                "use" => events.push(trader.use_recipe(arg)),
                "disable" => trader.offers[arg].uses = trader.offers[arg].max_uses,
                "tick" => {
                    for _ in 0..arg {
                        events.push(trader.tick_restock(member).unwrap());
                    }
                }
                _ => unreachable!(),
            }
            let mut xp = Vec::new();
            let mut sounds = Vec::new();
            let mut effects = Vec::new();
            let mut statuses = Vec::new();
            for event in events {
                xp.extend(event.experience);
                if let Some((name, volume, pitch)) = event.sound {
                    sounds.push(json!([name, volume.to_bits(), pitch.to_bits()]));
                }
                if event.regeneration {
                    effects.push(json!([10, 200, 0]));
                }
                if let Some(name) = event.reputation_player {
                    assert_eq!(name, "FoodOracle");
                    reputation = (reputation + 1).min(10);
                    statuses.push(14);
                }
            }
            let native = json!({"op":command,"stock":trader.offers.iter().map(|o|[o.uses,o.max_uses]).collect::<Vec<_>>(),"career":trader.career.id,"level":trader.career.level,"wealth":trader.career.wealth,"willing":trader.career.willing,"timer":trader.career.reset_ticks,"init":trader.career.needs_initialization,"buyer":trader.career.last_buyer,"rep":reputation,"sounds":sounds,"effects":effects,"experience":xp,"statuses":statuses,"tail":tail(trader.career.random_state)});
            assert_eq!(native, *expected, "restock case{case}, step{step}");
        }
    }
    assert_eq!(data.as_array().unwrap().len(), 160);
}
