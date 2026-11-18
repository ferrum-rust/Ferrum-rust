use super::*;
use crate::{
    gameplay::ai::golem::{self, targets::Kind},
    vanilla::{movement::Aabb, random::JavaRandom},
};
use serde_json::{json, Value};
use std::io::Read;
struct Harness {
    sample: i32,
    tick: i32,
    center: DVec3,
    actors: Vec<Actor>,
    has_path: bool,
    calls: Vec<Value>,
}
impl World for Harness {
    fn actor(&self, id: u64) -> Option<Actor> {
        self.actors.get(id as usize).copied()
    }
    fn nearest_door(&self, p: IVec3, used: &[IVec3]) -> Option<IVec3> {
        if self.sample % 4 == 0 || p.as_dvec3().distance_squared(DVec3::new(2., 1., 0.)) > 1024. {
            return None;
        }
        (0..5)
            .map(|j| IVec3::new(j * 4 - 6, 1, j % 2 * 2))
            .filter(|d| !used.contains(d))
            .min_by_key(|d| p.as_dvec3().distance_squared(d.as_dvec3()) as i32)
    }
    fn nearest_villager(&self) -> Option<u64> {
        Some(3)
    }
    fn nearest_player(&self, range: f64) -> Option<u64> {
        (self.center.distance_squared(self.actors[1].position) < range * range).then_some(1)
    }
    fn path_entity(&mut self, id: u64) -> Option<Path> {
        let accepted = self.tick % 7 != 2;
        self.calls.push(json!(["entity", id, accepted]));
        accepted.then(|| vec![self.actors[id as usize].position.floor().as_ivec3()])
    }
    fn path_block(&mut self, p: IVec3, break_doors: bool) -> Option<Path> {
        let accepted = (p.x * 7 + p.y + p.z + self.tick).rem_euclid(5) != 1;
        self.calls
            .push(json!(["block", p.x, p.y, p.z, break_doors, accepted]));
        accepted.then(|| vec![p])
    }
    fn set_path(&mut self, path: Option<Path>, speed: f64) {
        self.has_path = path.is_some();
        self.calls.push(json!(["set", self.has_path, speed]));
    }
    fn clear_path(&mut self) {
        self.has_path = false;
        self.calls.push(json!(["clear"]));
    }
    fn no_path(&self) -> bool {
        !self.has_path
    }
    fn entity_eye(&self, id: u64) -> Option<DVec3> {
        self.actor(id)
            .map(|a| a.position + DVec3::Y * f64::from(1.62f32))
    }
    fn look_entity(&mut self, id: u64, yaw: f32, pitch: f32) {
        self.calls.push(json!(["lookEntity", id, yaw, pitch]));
    }
    fn look_position(&mut self, p: DVec3, yaw: f32, pitch: f32) {
        self.calls.push(json!([
            "look",
            p.to_array().map(|v| format!("{:x}", v.to_bits())),
            yaw,
            pitch
        ]));
    }
    fn strike(&mut self, _: u64, core: &mut Golem) {
        core.strike(self);
    }
    fn rose(&mut self, hold: bool, core: &mut Golem) {
        core.hold_rose(hold, self);
    }
}
impl golem::Actions for Harness {
    fn status(&mut self, id: u8) {
        self.calls.push(json!(["status", id]));
    }
    fn attack(&mut self, amount: i32) -> bool {
        self.calls.push(json!(["hit", amount]));
        false
    }
    fn knock_up(&mut self, _: f64) {
        unreachable!()
    }
    fn enchantment_callbacks(&mut self) {
        unreachable!()
    }
    fn throw_sound(&mut self) {
        self.calls.push(json!(["sound", "mob.irongolem.throw"]));
    }
    fn drop(&mut self, _: u16) {
        unreachable!()
    }
}
#[test]
fn all_eight_golem_action_tasks_match_original_in_13728_updates() {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(
        include_bytes!("../tests/fixtures/vanilla189-golem-actions.json.gz").as_slice(),
    )
    .read_to_end(&mut bytes)
    .unwrap();
    let data: Value = serde_json::from_slice(&bytes).unwrap();
    let mut count = 0;
    for row in data.as_array().unwrap() {
        let sample = row[0].as_i64().unwrap() as i32;
        let mut core = Golem::new(
            JavaRandom::new(row[1].as_str().unwrap().parse().unwrap()),
            false,
        );
        let mut goals = ActionGoals::default();
        let mut w = Harness {
            sample,
            tick: 0,
            center: DVec3::ZERO,
            actors: Vec::new(),
            has_path: false,
            calls: Vec::new(),
        };
        for (t, f) in row[2].as_array().unwrap().iter().enumerate() {
            w.tick = t as i32 + 1;
            w.calls.clear();
            let x = f[1].as_f64().unwrap();
            let center = DVec3::new(x, 1., 0.5);
            w.center = center;
            w.actors = [
                (
                    Kind::Hostile,
                    if w.tick % 19 < 7 {
                        x + 1.
                    } else {
                        x + 7. + f64::from(w.tick % 3)
                    },
                    0.5,
                    1.95f32,
                ),
                (Kind::Player, x + 2. + f64::from(w.tick % 11), 0.5, 1.8),
                (Kind::Passive, x + 3., 1.5, 1.8),
                (Kind::Passive, x - 3., -0.5, 1.8),
            ]
            .into_iter()
            .enumerate()
            .map(|(id, (kind, px, z, h))| {
                let pos = DVec3::new(px, 1., z);
                let r = f64::from(0.6f32 * 0.5);
                Actor {
                    id: id as u64,
                    kind,
                    position: pos,
                    bounds: Aabb {
                        min: pos - DVec3::new(r, 0., r),
                        max: pos + DVec3::new(r, f64::from(h), r),
                    },
                    alive: true,
                    invisible: false,
                    immune: false,
                    same_team: false,
                }
            })
            .collect();
            if f[5].as_bool().unwrap() {
                w.has_path = false;
            }
            let target = f[4].as_i64().unwrap();
            let ctx = Context {
                position: center,
                eye: center + DVec3::Y * f64::from(2.9f32 * 0.85),
                width: 1.4,
                age: f[3].as_i64().unwrap() as i32,
                day: f[0].as_bool().unwrap(),
                home: f[2].as_bool().unwrap().then_some((IVec3::Y, 8.)),
                target: (target >= 0).then_some(target as u64),
            };
            goals.tick(ctx, &mut w, &mut core);
            assert_eq!(
                json!([
                    goals.active(),
                    w.has_path,
                    core.attack_ticks,
                    core.rose_ticks,
                    w.calls,
                    core.random.state().to_string()
                ]),
                json!([f[6], f[7], f[8], f[9], f[10], f[11]]),
                "action sample{sample}, tick{}",
                w.tick
            );
            count += 1;
        }
    }
    assert_eq!(count, 13728);
}
