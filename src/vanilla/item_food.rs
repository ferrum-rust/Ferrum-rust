//! Original ItemFood callbacks and EntityPlayer's identity-based use countdown.
//! Particle animation consumes entity-local/global visual RNG, separately from
//! the world RNG advanced here by burp pitch and food-effect probability.
use super::{
    food::{food_values, FoodState},
    random::JavaRandom,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
include!("item_food_data.rs");

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Effect {
    pub id: u8,
    pub duration: i32,
    pub amplifier: i32,
    pub ambient: bool,
    pub show_particles: bool,
}
impl Effect {
    pub fn new(id: u8, duration: i32, amplifier: i32) -> Self {
        Self {
            id,
            duration,
            amplifier,
            ambient: false,
            show_particles: true,
        }
    }
    /// PotionEffect.combine retains the original ambient-flag else-if order.
    pub fn combine(&mut self, other: Self) {
        if other.amplifier > self.amplifier {
            self.amplifier = other.amplifier;
            self.duration = other.duration;
        } else if other.amplifier == self.amplifier && self.duration < other.duration {
            self.duration = other.duration;
        } else if !other.ambient && self.ambient {
            self.ambient = false;
        }
        self.show_particles = other.show_particles;
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoodStack {
    pub id: u16,
    pub meta: u16,
    pub count: i32,
}
#[derive(Clone, Debug)]
pub struct Finished {
    pub stack: FoodStack,
    pub burp_pitch: f32,
    pub effects: Vec<Effect>,
}
pub fn can_start(stack: FoodStack, food: &FoodState, immune: bool) -> Result<bool> {
    let value = food_values(stack.id, stack.meta).context("Item is not an original food")?;
    Ok(!immune && (value.always_edible || food.level < 20))
}
/// ItemFood.onItemUseFinish itself does not check player creative immunity.
/// Normal callers enforce can_start; server status9 can invoke completion too.
pub fn finish(
    food: &mut FoodState,
    random: &mut JavaRandom,
    mut stack: FoodStack,
    remote: bool,
) -> Result<Finished> {
    let value = food_values(stack.id, stack.meta).context("Item is not an original food")?;
    stack.count = stack.count.wrapping_sub(1);
    food.add_stats(value.nutrition, value.saturation);
    let burp_pitch = random.next_float() * 0.1 + 0.9;
    let mut effects = Vec::new();
    let mut generic = true;
    if stack.id == 322 {
        if !remote {
            effects.push(Effect::new(22, 2400, 0));
        }
        if stack.meta > 0 {
            generic = false;
            if !remote {
                effects.extend([
                    Effect::new(10, 600, 4),
                    Effect::new(11, 6000, 0),
                    Effect::new(12, 6000, 0),
                ]);
            }
        }
    }
    if matches!(stack.id, 349 | 350) && stack.meta == 3 {
        effects.extend([
            Effect::new(19, 1200, 3),
            Effect::new(17, 300, 2),
            Effect::new(9, 300, 1),
        ]);
    }
    if generic && !remote {
        let (_, id, seconds, amplifier, chance) = EFFECTS
            .iter()
            .find(|row| row.0 == stack.id)
            .context("Missing original food effect registry")?;
        if *id > 0 && random.next_float() < f32::from_bits(*chance) {
            effects.push(Effect::new(*id, seconds.wrapping_mul(20), *amplifier));
        }
    }
    if matches!(stack.id, 282 | 413) {
        stack = FoodStack {
            id: 281,
            meta: 0,
            count: 1,
        };
    }
    Ok(Finished {
        stack,
        burp_pitch,
        effects,
    })
}

/// The identity is the ItemStack object, not its value or inventory slot.
/// Moving that same object to another selected slot continues the action;
/// replacing it with an equal-valued new object cancels it.
#[derive(Clone, Debug, Default)]
pub struct UseState {
    identity: Option<u64>,
    remaining: i32,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct UseTick {
    pub animation: bool,
    pub finished: bool,
    pub cancelled: bool,
}
impl UseState {
    pub fn active(&self) -> bool {
        self.identity.is_some()
    }
    pub fn remaining(&self) -> i32 {
        self.remaining
    }
    pub fn identity(&self) -> Option<u64> {
        self.identity
    }
    pub fn start(&mut self, identity: u64, allowed: bool) {
        if allowed {
            self.start_for(identity, 32);
        }
    }
    pub fn start_for(&mut self, identity: u64, duration: i32) {
        if self.identity != Some(identity) {
            self.identity = Some(identity);
            self.remaining = duration;
        }
    }
    pub fn stop(&mut self) {
        self.identity = None;
        self.remaining = 0;
    }
    pub fn force_finish(&mut self) -> bool {
        let active = self.active();
        self.stop();
        active
    }
    pub fn tick(&mut self, held: Option<u64>, remote: bool) -> UseTick {
        if !self.active() {
            return UseTick::default();
        }
        if held != self.identity {
            self.stop();
            return UseTick {
                cancelled: true,
                ..Default::default()
            };
        }
        let animation = self.remaining <= 25 && self.remaining % 4 == 0;
        self.remaining = self.remaining.wrapping_sub(1);
        let finished = self.remaining == 0 && !remote;
        if finished {
            self.stop();
        }
        UseTick {
            animation,
            finished,
            cancelled: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn stack(stack: Option<FoodStack>) -> Value {
        stack.map_or(Value::Null, |s| json!([s.id, s.meta, s.count]))
    }
    fn food_state(food: FoodState) -> Value {
        json!({"food":food.level,"timer":food.timer,"saturation":food.saturation.to_bits(),"exhaustion":food.exhaustion.to_bits(),"previous":food.previous_level,"health":20f32.to_bits(),"heals":0,"damages":0})
    }
    #[test]
    fn food_finish_effect_requests_and_world_rng_match_original() {
        let data: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-item-food.json"
        ))
        .unwrap();
        for (index, row) in data["finish"].as_array().unwrap().iter().enumerate() {
            let mut food = FoodState {
                level: row["level"].as_i64().unwrap() as i32,
                saturation: 0.,
                ..Default::default()
            };
            let mut random = JavaRandom::new(row["seed"].as_str().unwrap().parse().unwrap());
            let remote = row["remote"].as_bool().unwrap();
            let result = finish(
                &mut food,
                &mut random,
                FoodStack {
                    id: row["id"].as_u64().unwrap() as u16,
                    meta: row["meta"].as_u64().unwrap() as u16,
                    count: 2,
                },
                remote,
            )
            .unwrap();
            assert_eq!(
                stack(Some(result.stack)),
                row["returned"],
                "case{index} returned"
            );
            assert_eq!(food_state(food), row["food"], "case{index} nutrition");
            assert_eq!(
                json!(result
                    .effects
                    .iter()
                    .map(|e| [i32::from(e.id), e.duration, e.amplifier])
                    .collect::<Vec<_>>()),
                row["effects"],
                "case{index} effects"
            );
            assert_eq!(
                json!([result.burp_pitch.to_bits()]),
                row["pitches"],
                "case{index} pitch"
            );
            assert_eq!(
                random.next_long().to_string(),
                row["tail"].as_str().unwrap(),
                "case{index} world RNG"
            );
        }
        assert_eq!(data["finish"].as_array().unwrap().len(), 4960);
    }
    #[test]
    fn use_duration_object_identity_and_server_finish_match_original() {
        let data: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-item-food.json"
        ))
        .unwrap();
        for (index, row) in data["use"].as_array().unwrap().iter().enumerate() {
            let mut food = FoodState {
                level: row["level"].as_i64().unwrap() as i32,
                saturation: 0.,
                ..Default::default()
            };
            let mut random = JavaRandom::new(189);
            let mut use_state = UseState::default();
            let mut held = Some(FoodStack {
                id: row["id"].as_u64().unwrap() as u16,
                meta: row["meta"].as_u64().unwrap() as u16,
                count: 2,
            });
            let mut identity = Some(1);
            let remote = row["remote"].as_bool().unwrap();
            use_state.start(
                1,
                can_start(held.unwrap(), &food, row["immune"].as_bool().unwrap()).unwrap(),
            );
            let mut steps = vec![json!([
                use_state.active(),
                use_state.remaining(),
                stack(held)
            ])];
            let mut animations = Vec::new();
            let mut effects = Vec::new();
            let mut pitches = Vec::new();
            for tick in 1..=40 {
                let mut forced = false;
                if tick == 12 {
                    match row["pattern"].as_u64().unwrap() {
                        1 => identity = Some(2),
                        2 => {}
                        3 => held.as_mut().unwrap().count += 1,
                        4 => use_state.stop(),
                        5 => forced = use_state.force_finish(),
                        _ => {}
                    }
                }
                if forced {
                    animations.push(16);
                    let result = finish(&mut food, &mut random, held.unwrap(), remote).unwrap();
                    held = (result.stack.count != 0).then_some(result.stack);
                    pitches.push(result.burp_pitch.to_bits());
                    effects.extend(result.effects);
                    if held.is_none() {
                        identity = None;
                    }
                }
                let update = use_state.tick(identity, remote);
                if update.animation {
                    animations.push(5);
                }
                if update.finished {
                    animations.push(16);
                    let result = finish(&mut food, &mut random, held.unwrap(), remote).unwrap();
                    held = (result.stack.count != 0).then_some(result.stack);
                    pitches.push(result.burp_pitch.to_bits());
                    effects.extend(result.effects);
                    if held.is_none() {
                        identity = None;
                    }
                }
                steps.push(json!([
                    use_state.active(),
                    use_state.remaining(),
                    stack(held)
                ]));
            }
            assert_eq!(
                json!(steps),
                row["steps"],
                "use{index} item identity/countdown"
            );
            assert_eq!(
                json!(animations),
                row["animations"],
                "use{index} animation timing"
            );
            assert_eq!(food_state(food), row["food"], "use{index} food");
            assert_eq!(
                json!(effects
                    .iter()
                    .map(|e| [i32::from(e.id), e.duration, e.amplifier])
                    .collect::<Vec<_>>()),
                row["effects"],
                "use{index} effects"
            );
            assert_eq!(json!(pitches), row["pitches"], "use{index} pitch");
            assert_eq!(
                random.next_long().to_string(),
                row["tail"].as_str().unwrap(),
                "use{index} RNG"
            );
        }
        assert_eq!(data["use"].as_array().unwrap().len(), 240);
    }
}
