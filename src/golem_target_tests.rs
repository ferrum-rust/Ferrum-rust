use super::*;
use serde_json::{json, Value};
use std::io::Read;

struct Harness {
    tick: i32,
    actors: Vec<Actor>,
    aggressors: Vec<ActorId>,
    score: i32,
    requested: Vec<[i32; 2]>,
}
impl World for Harness {
    fn actor(&self, id: ActorId) -> Option<Actor> {
        self.actors.get(id as usize).copied()
    }
    fn entities(&self, b: Aabb) -> Vec<ActorId> {
        self.actors
            .iter()
            .filter(|a| a.kind != Kind::Player && b.intersects(a.bounds))
            .map(|a| a.id)
            .collect()
    }
    fn nearest_aggressor(&self) -> Option<ActorId> {
        let center = DVec3::new(0.5, 1., 0.5);
        let mut best = f64::MAX;
        let mut id = None;
        for &actor in &self.aggressors {
            let distance = self.actors[actor as usize]
                .position
                .distance_squared(center);
            if distance <= best {
                best = distance;
                id = Some(actor);
            }
        }
        id
    }
    fn low_reputation_player(&self) -> Option<ActorId> {
        (self.score <= -15).then_some(2)
    }
    fn path_end(&mut self, id: ActorId) -> Option<IVec3> {
        let delta = (id as i32 + self.tick / 11) % 5;
        self.requested.push([id as i32, delta]);
        (delta != 4)
            .then(|| self.actors[id as usize].position.floor().as_ivec3() + IVec3::X * delta)
    }
}
#[test]
fn golem_defend_hurt_and_nearest_target_goal_order_matches_original_9024_ticks() {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(
        include_bytes!("../tests/fixtures/vanilla189-golem-targets.json.gz").as_slice(),
    )
    .read_to_end(&mut bytes)
    .unwrap();
    let data: Value = serde_json::from_slice(&bytes).unwrap();
    let mut count = 0;
    for (sample, row) in data.as_array().unwrap().iter().enumerate() {
        let mut random = JavaRandom::new(row[0].as_str().unwrap().parse().unwrap());
        let made = row[1].as_bool().unwrap();
        let mut targets = Targeting::default();
        let mut revenge = None;
        let mut revenge_timer = 0;
        let mut world = Harness {
            tick: 0,
            actors: Vec::new(),
            aggressors: Vec::new(),
            score: 0,
            requested: Vec::new(),
        };
        let center = DVec3::new(0.5, 1., 0.5);
        let width = f64::from(1.4f32 * 0.5);
        let bounds = Aabb {
            min: center - DVec3::new(width, 0., width),
            max: center + DVec3::new(width, f64::from(2.9f32), width),
        };
        for (i, frame) in row[2].as_array().unwrap().iter().enumerate() {
            world.tick = i as i32 + 1;
            world.requested.clear();
            let a = frame[2].as_i64().unwrap();
            if a >= 0 {
                revenge = Some(a as ActorId);
                revenge_timer = world.tick;
            }
            let a = frame[3].as_i64().unwrap();
            if a >= 0 && !world.aggressors.contains(&(a as ActorId)) {
                world.aggressors.push(a as ActorId);
            }
            world.score = (world.score + frame[4].as_i64().unwrap() as i32).clamp(-30, 10);
            world.actors = frame[6]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(k, p)| {
                    let kind = [
                        Kind::Hostile,
                        Kind::Creeper,
                        Kind::Player,
                        Kind::Hostile,
                        Kind::Passive,
                        Kind::Hostile,
                    ][k];
                    let pos = DVec3::new(
                        p[0].as_f64().unwrap(),
                        p[1].as_f64().unwrap(),
                        p[2].as_f64().unwrap(),
                    );
                    let height = [1.95f32, 1.7, 1.8, 1.95, 1.8, 1.95][k];
                    let r = f64::from(0.6f32 * 0.5);
                    Actor {
                        id: k as u64,
                        position: pos,
                        kind,
                        alive: p[3].as_bool().unwrap(),
                        invisible: p[4].as_bool().unwrap(),
                        immune: k == 2 && frame[5].as_bool().unwrap(),
                        same_team: false,
                        bounds: Aabb {
                            min: pos - DVec3::new(r, 0., r),
                            max: pos + DVec3::new(r, f64::from(height), r),
                        },
                    }
                })
                .collect();
            let ctx = Context {
                position: center,
                bounds,
                home: frame[1].as_bool().unwrap().then_some((IVec3::Y, 8.)),
                member: frame[0].as_bool().unwrap(),
                player_created: made,
                range: 16.,
                revenge,
                revenge_timer,
            };
            targets.tick(ctx, &mut world, &mut random);
            assert_eq!(
                json!([
                    targets.target.map_or(-1, |id| id as i64),
                    world.requested,
                    random.state().to_string()
                ]),
                json!([frame[7], frame[8], frame[9]]),
                "target session{sample}, tick{}",
                world.tick
            );
            count += 1;
        }
    }
    assert_eq!(count, 9024);
}
