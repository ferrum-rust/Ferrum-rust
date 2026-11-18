use super::*;
use crate::vanilla::food::FoodState;
use serde_json::{json, Value};
fn effect(v: &Value) -> Effect {
    Effect {
        id: v[0].as_u64().unwrap() as u8,
        duration: v[1].as_i64().unwrap() as i32,
        amplifier: v[2].as_i64().unwrap() as i32,
        ambient: v[3].as_bool().unwrap(),
        show_particles: v[4].as_bool().unwrap(),
    }
}
fn value(e: Effect) -> Value {
    json!([e.id, e.duration, e.amplifier, e.ambient, e.show_particles])
}
#[test]
fn survival_nbt_numeric_types_and_effect_replacement_match_original() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-survival-nbt.json"
    ))
    .unwrap();
    for (case, row) in cases.as_array().unwrap().iter().enumerate() {
        let hex = row["nbt_hex"].as_str().unwrap();
        let bytes = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect::<Vec<_>>();
        let tag = crate::nbt::read(&bytes).unwrap();
        let mut food = FoodState::default();
        food.read_nbt(&tag);
        assert_eq!(
            food.level as i64,
            row["food"]["food"].as_i64().unwrap(),
            "case{case} foodLevel"
        );
        assert_eq!(
            food.timer as i64,
            row["food"]["timer"].as_i64().unwrap(),
            "case{case} foodTimer"
        );
        assert_eq!(
            food.saturation.to_bits() as u64,
            row["food"]["saturation"].as_u64().unwrap(),
            "case{case} saturation"
        );
        assert_eq!(
            food.exhaustion.to_bits() as u64,
            row["food"]["exhaustion"].as_u64().unwrap(),
            "case{case} exhaustion"
        );
        let mut effects = Effects::default();
        for tag in tag.get("ActiveEffects").unwrap().list().unwrap() {
            if let Some(e) = read_effect(tag) {
                effects.load_effect(e).unwrap();
            }
        }
        assert_eq!(
            json!(effects.ordered().map(value).collect::<Vec<_>>()),
            row["effects"],
            "case{case} imported effects"
        );
    }
}
struct Actor {
    remote: bool,
    immune: bool,
    health: f32,
    absorption: f32,
    food: FoodState,
    events: Vec<Value>,
}
impl Target for Actor {
    fn remote(&self) -> bool {
        self.remote
    }
    fn health(&self) -> f32 {
        self.health
    }
    fn max_health(&self) -> f32 {
        20.
    }
    fn heal(&mut self, amount: f32) {
        self.events.push(json!(["heal", amount.to_bits()]));
        if self.health > 0. {
            self.health = (self.health + amount).clamp(0., 20.);
        }
    }
    fn attack(&mut self, wither: bool, amount: f32) {
        self.events.push(json!([
            if wither { "wither" } else { "magic" },
            amount.to_bits()
        ]));
    }
    fn exhaustion(&mut self, amount: f32) {
        self.events.push(json!(["exhaustion", amount.to_bits()]));
        self.food
            .player_exhaustion(amount, self.immune, self.remote);
    }
    fn saturation(&mut self, amount: i32) {
        self.food.add_stats(amount, 1.);
    }
    fn changed(&mut self, e: Effect, change: Change) {
        self.events.push(match change {
            Change::Added => json!(["added", value(e)]),
            Change::Changed(flag) => json!(["changed", value(e), flag]),
            Change::Removed => json!(["removed", value(e)]),
        });
        if e.id == 22 && !self.remote {
            if matches!(change, Change::Removed | Change::Changed(true)) {
                self.absorption = absorption_attribute(self.absorption, e.amplifier, false);
            }
            if matches!(change, Change::Added | Change::Changed(true)) {
                self.absorption = absorption_attribute(self.absorption, e.amplifier, true);
            }
        }
    }
}
#[test]
fn effect_map_callbacks_and_float_arithmetic_match_original() {
    let data: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/vanilla189-potions.json")).unwrap();
    for (case, row) in data["sequences"].as_array().unwrap().iter().enumerate() {
        let mut target = Actor {
            remote: row["remote"].as_bool().unwrap(),
            immune: row["immune"].as_bool().unwrap(),
            health: f32::from_bits(row["health"].as_u64().unwrap() as u32),
            absorption: 0.,
            food: FoodState::default(),
            events: vec![],
        };
        let mut effects = Effects::default();
        for (step, op) in row["ops"].as_array().unwrap().iter().enumerate() {
            match op[0].as_str().unwrap() {
                "add" => effects.add(effect(&op[1]), &mut target).unwrap(),
                "tick" => {
                    for _ in 0..op[1].as_u64().unwrap() {
                        effects.tick(&mut target)
                    }
                }
                "remove" => effects.remove(op[1].as_u64().unwrap() as u8, &mut target),
                "clear" => effects.clear(&mut target),
                _ => unreachable!(),
            }
            let state = json!([
                effects.ordered().map(value).collect::<Vec<_>>(),
                target.health.to_bits(),
                target.absorption.to_bits(),
                target.food.saturation.to_bits(),
                target.food.level,
                target.food.exhaustion.to_bits()
            ]);
            assert_eq!(
                state, row["states"][step],
                "sequence {case}, step {step}, op {op}"
            );
            // Checkpoint must retain HashMap's expanded capacity after removal.
            effects = serde_json::from_str(&serde_json::to_string(&effects).unwrap()).unwrap();
        }
        assert_eq!(
            json!(target.events),
            row["events"],
            "sequence {case} callback order"
        );
    }
    assert_eq!(data["sequences"].as_array().unwrap().len(), 256);
    for row in data["resistance"].as_array().unwrap() {
        let source = row[1].as_u64().unwrap();
        let got = resistance_damage(
            f32::from_bits(row[2].as_u64().unwrap() as u32),
            Some(row[0].as_i64().unwrap() as i32),
            source == 2,
            source == 3,
        );
        assert_eq!(
            got.to_bits(),
            row[3].as_u64().unwrap() as u32,
            "resistance {row}"
        );
    }
}
