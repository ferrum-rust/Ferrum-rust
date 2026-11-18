//! Iron golem's own combat, status and animation callbacks. The complete
//! living-entity task/navigation tick owns movement and effect dispatch.
#![allow(dead_code)]
use crate::vanilla::random::JavaRandom;
#[path = "golem_actions.rs"]
pub mod actions;
#[path = "golem_model.rs"]
pub mod model;
#[path = "golem_targets.rs"]
pub mod targets;

#[derive(Clone, Debug)]
pub struct Golem {
    pub attack_ticks: i32,
    pub rose_ticks: i32,
    pub player_created: bool,
    pub random: JavaRandom,
}
pub trait Actions {
    fn status(&mut self, id: u8);
    fn attack(&mut self, amount: i32) -> bool;
    fn knock_up(&mut self, amount: f64);
    fn enchantment_callbacks(&mut self);
    fn throw_sound(&mut self);
    fn drop(&mut self, item: u16);
}
impl Golem {
    pub fn new(random: JavaRandom, player_created: bool) -> Self {
        Self {
            attack_ticks: 0,
            rose_ticks: 0,
            player_created,
            random,
        }
    }
    pub fn strike(&mut self, world: &mut impl Actions) -> bool {
        self.attack_ticks = 10;
        world.status(4);
        let accepted = world.attack(7 + self.random.next_int(15));
        if accepted {
            world.knock_up(f64::from(0.4f32));
            world.enchantment_callbacks();
        }
        world.throw_sound();
        accepted
    }
    pub fn drop_loot(&mut self, world: &mut impl Actions) {
        for _ in 0..self.random.next_int(3) {
            world.drop(38);
        }
        for _ in 0..3 + self.random.next_int(3) {
            world.drop(265);
        }
    }
    pub fn hold_rose(&mut self, hold: bool, world: &mut impl Actions) {
        self.rose_ticks = if hold { 400 } else { 0 };
        world.status(11);
    }
    pub fn status(&mut self, id: u8, world: &mut impl Actions) {
        match id {
            4 => {
                self.attack_ticks = 10;
                world.throw_sound();
            }
            11 => self.rose_ticks = 400,
            _ => {}
        }
    }
}
fn triangle(time: f32, period: f32) -> f32 {
    ((time % period - period * 0.5).abs() - period * 0.25) / (period * 0.25)
}
/// Head pitch/yaw, right/left arms, right/left legs. Model coordinates are
/// deliberately used until the entity renderer applies its world transform.
pub fn pose(
    swing: f32,
    amount: f32,
    partial: f32,
    yaw: f32,
    pitch: f32,
    attack: i32,
    rose: i32,
) -> [f32; 6] {
    let legs = triangle(swing, 13.);
    let (right, left) = if attack > 0 {
        let a = -2. + 1.5 * triangle(attack as f32 - partial, 10.);
        (a, a)
    } else if rose > 0 {
        (-0.8 + 0.025 * triangle(rose as f32, 70.), 0.)
    } else {
        ((-0.2 + 1.5 * legs) * amount, (-0.2 - 1.5 * legs) * amount)
    };
    [
        pitch / 57.295776,
        yaw / 57.295776,
        right,
        left,
        1.5 * legs * amount,
        -1.5 * legs * amount,
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::io::Read;
    struct World {
        accepted: bool,
        velocity: f64,
        events: Vec<Value>,
    }
    impl Actions for World {
        fn status(&mut self, id: u8) {
            self.events.push(json!(["status", id]));
        }
        fn attack(&mut self, n: i32) -> bool {
            self.events.push(json!(["hit", "mob", n]));
            self.accepted
        }
        fn knock_up(&mut self, n: f64) {
            self.velocity += n;
        }
        fn enchantment_callbacks(&mut self) {}
        fn throw_sound(&mut self) {
            self.events.push(json!([
                "sound",
                "mob.irongolem.throw",
                1f32.to_bits(),
                1f32.to_bits()
            ]));
        }
        fn drop(&mut self, id: u16) {
            self.events.push(json!(["drop", id, 1, 0]));
        }
    }
    fn fixture() -> Value {
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(
            include_bytes!("../tests/fixtures/vanilla189-iron-golem.json.gz").as_slice(),
        )
        .read_to_end(&mut bytes)
        .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }
    #[test]
    fn iron_golem_strike_drops_rose_states_and_target_rules_match_512_original_cases() {
        let fixture = fixture();
        for row in fixture["combat"].as_array().unwrap() {
            let i = row[1].as_i64().unwrap();
            let mut golem = Golem::new(
                JavaRandom::new(row[0].as_str().unwrap().parse().unwrap()),
                i & 1 != 0,
            );
            let mut w = World {
                accepted: i & 2 != 0,
                velocity: (i % 7) as f64 * 0.18,
                events: Vec::new(),
            };
            let ok = golem.strike(&mut w);
            assert_eq!(
                json!([
                    ok,
                    [format!("{:x}", w.velocity.to_bits())],
                    golem.attack_ticks,
                    golem.random.state().to_string(),
                    w.events
                ]),
                row[2],
                "strike{i}"
            );
            w.events.clear();
            golem.drop_loot(&mut w);
            assert_eq!(
                json!([golem.random.state().to_string(), w.events]),
                row[3],
                "loot{i}"
            );
            w.events.clear();
            golem.hold_rose(i & 8 != 0, &mut w);
            assert_eq!(golem.rose_ticks, row[4].as_i64().unwrap() as i32);
            golem.status(if i % 3 == 0 { 4 } else { 11 }, &mut w);
            assert_eq!(
                json!([golem.rose_ticks, golem.attack_ticks, w.events]),
                json!([row[5], row[6], row[7]])
            );
            assert_eq!(
                json!([
                    targets::can_attack_kind(golem.player_created, targets::Kind::Player),
                    targets::can_attack_kind(golem.player_created, targets::Kind::Creeper),
                    targets::can_attack_kind(golem.player_created, targets::Kind::Hostile),
                    targets::can_attack_kind(golem.player_created, targets::Kind::Ghast)
                ]),
                json!([row[8][0], row[8][1], row[8][2], row[8][4]])
            );
        }
    }
    #[test]
    fn model_iron_golem_head_limbs_attack_and_flower_match_512_original_poses() {
        let fixture = fixture();
        for (i, row) in fixture["models"].as_array().unwrap().iter().enumerate() {
            let f = |k: usize| f32::from_bits(row[0][k].as_u64().unwrap() as u32);
            assert_eq!(
                json!(pose(
                    f(0),
                    f(1),
                    f(2),
                    f(3),
                    f(4),
                    row[0][5].as_i64().unwrap() as i32,
                    row[0][6].as_i64().unwrap() as i32
                )
                .map(f32::to_bits)),
                row[1],
                "pose{i}"
            );
        }
    }
}
