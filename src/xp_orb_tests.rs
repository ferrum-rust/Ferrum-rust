use super::{
    motion::{Experience, OrbWorld, Target},
    *,
};
use crate::vanilla::{collision_shapes, movement::MovementWorld};
use anyhow::Result;
use glam::IVec3;
use serde_json::{json, Value};
use std::io::Read;

fn fixture() -> Value {
    let mut text = String::new();
    flate2::read::GzDecoder::new(
        include_bytes!("../tests/fixtures/vanilla189-xp-orbs.json.gz").as_slice(),
    )
    .read_to_string(&mut text)
    .unwrap();
    serde_json::from_str(&text).unwrap()
}
struct World {
    layout: i32,
    tick: usize,
}
impl MovementWorld for World {
    fn get(&self, p: IVec3) -> u16 {
        if !(0..256).contains(&p.y) || self.layout == 13 {
            return 0;
        }
        if p.y == 0 {
            return (match self.layout {
                1 => 79,
                2 => 174,
                3 => 165,
                4 => 88,
                5 => 44,
                _ => 1,
            }) << 4;
        }
        match self.layout {
            6 if (2..=4).contains(&p.y) => 30 << 4,
            7 if p.y <= 3 => 9 << 4,
            8 if p.y <= 3 => 8 << 4 | p.x.rem_euclid(8) as u16,
            9 if p.y <= 3 => 10 << 4,
            10 | 14 if p.y <= 3 => 51 << 4,
            11 if p.x == 8 && p.z == 8 && (2..=4).contains(&p.y) => 16,
            12 if p.x == 0 && p.y <= 6 => 16,
            15 if p.x == 8 && p.z == 8 && p.y <= 3 => 81 << 4,
            _ => 0,
        }
    }
    fn block_boxes(&self, p: IVec3, state: u16, out: &mut Vec<Aabb>) {
        collision_shapes::emit(p, state, |q| self.get(q), None, |b| out.push(b));
    }
    fn loaded_column(&self, _: i32, _: i32) -> bool {
        true
    }
}
impl OrbWorld for World {
    fn full_collision_cube(&mut self, p: IVec3) -> Result<bool> {
        Ok(collision_shapes::full_collision_cube(
            p,
            self.get(p),
            |q| self.get(q),
            None,
        ))
    }
    fn raining_at(&self, _: IVec3) -> bool {
        self.layout == 10 && self.tick > 4
    }
}
fn bits(values: impl IntoIterator<Item = f64>) -> Vec<String> {
    values
        .into_iter()
        .map(|v| format!("{:x}", v.to_bits()))
        .collect()
}
fn sounds(events: &[(&'static str, f32, f32)]) -> Vec<Value> {
    events
        .iter()
        .map(|(k, v, p)| json!(["sound", k, v.to_bits(), p.to_bits()]))
        .collect()
}
#[test]
fn experience_orb_constructor_and_4165_update_ticks_match_original() {
    let data = fixture();
    let mut frames = 0;
    for c in data["cases"].as_array().unwrap() {
        let layout = c["layout"].as_i64().unwrap() as i32;
        let mode = c["mode"].as_i64().unwrap() as i32;
        let mut w = World { layout, tick: 0 };
        let seed = c["seed"].as_str().unwrap().parse::<i64>().unwrap();
        let mut math = JavaRandom::new(seed ^ 137);
        let pos: DVec3 = serde_json::from_value(c["position"].clone()).unwrap();
        let mut orb = Orb::new(
            c["id"].as_i64().unwrap() as i32,
            pos,
            3 + mode * 2,
            &mut math,
            JavaRandom::new(seed).state(),
        );
        assert_eq!(json!(bits(orb.velocity.to_array())), c["generated"]);
        assert_eq!(json!(orb.yaw.to_bits()), c["yaw"]);
        assert_eq!(json!(math.state().to_string()), c["math"]);
        orb.velocity = serde_json::from_value(c["motion"].clone()).unwrap();
        orb.age = if mode == 3 { 5991 } else { 0 };
        orb.delay = mode + 1;
        orb.fire = if layout == 10 { 60 } else { 0 };
        for (i, expected) in c["frames"].as_array().unwrap().iter().enumerate() {
            w.tick = i + 1;
            let dx = if w.tick <= 12 {
                10.
            } else if mode == 4 {
                8.
            } else if w.tick <= 47 {
                4.2
            } else {
                1.8
            };
            let feet = pos + DVec3::new(dx, -2., 1.);
            let height = if mode == 1 && w.tick >= 50 {
                1.62f32 - 0.08
            } else {
                1.62f32
            };
            let target = Target {
                position: feet,
                eye: feet + DVec3::Y * f64::from(height),
                spectator: mode == 2 && (30..=40).contains(&w.tick),
            };
            let mut events = Vec::new();
            orb.tick(&mut w, Some(target), &mut events).unwrap();
            let actual = json!([
                bits(
                    [
                        orb.position,
                        orb.previous,
                        orb.velocity,
                        orb.bounds.min,
                        orb.bounds.max
                    ]
                    .into_iter()
                    .flat_map(|v| v.to_array())
                ),
                orb.age,
                orb.delay,
                orb.color,
                orb.target_color,
                orb.tracks_player,
                orb.health,
                orb.fire,
                orb.grounded,
                orb.in_web,
                orb.dead,
                orb.random_state.to_string(),
                sounds(&events)
            ]);
            assert_eq!(
                actual, *expected,
                "layout {layout}, mode {mode}, tick {}",
                w.tick
            );
            frames += 1;
        }
    }
    assert_eq!(frames, 4165);
}
#[test]
fn xp_pickup_delay_score_level_sound_and_rng_match_original() {
    let data = fixture();
    for (index, row) in data["pickups"].as_array().unwrap().iter().enumerate() {
        let value = row[0].as_i64().unwrap() as i32;
        let seed = row[1].as_str().unwrap().parse::<i64>().unwrap();
        let before = &row[2];
        let mut player = Experience {
            level: before[0].as_i64().unwrap() as i32,
            total: before[1].as_i64().unwrap() as i32,
            progress: f32::from_bits(before[2].as_u64().unwrap() as u32),
            cooldown: before[3].as_i64().unwrap() as i32,
            ..Default::default()
        };
        let mut orb = Orb::new(
            0,
            DVec3::ZERO,
            value,
            &mut JavaRandom::new(seed),
            JavaRandom::new(seed).state(),
        );
        orb.delay = before[4].as_i64().unwrap() as i32;
        let mut events = Vec::new();
        let collected = orb.collect(&mut player, 145, &mut events);
        let mut calls = sounds(&events);
        if collected {
            calls.insert(1, json!(["pickup", 1]));
        }
        assert_eq!(
            json!([
                player.level,
                player.total,
                player.progress.to_bits(),
                player.score,
                player.cooldown,
                orb.dead,
                orb.random_state.to_string(),
                calls
            ]),
            Value::Array(row.as_array().unwrap()[3..].to_vec()),
            "pickup{index}"
        );
    }
    assert_eq!(data["pickups"].as_array().unwrap().len(), 512);
}
