use super::{collection::VillageWorld, *};
use crate::vanilla::{material, movement::Aabb, random::JavaRandom};
use glam::DVec3;
use serde_json::{json, Value};
use std::{collections::HashMap, io::Read};

struct World {
    blocks: HashMap<IVec3, u16>,
    residents: Vec<Aabb>,
}
impl VillageWorld for World {
    fn block(&self, p: IVec3) -> u16 {
        self.blocks.get(&p).copied().unwrap_or(0)
    }
    fn sky(&self, p: IVec3) -> bool {
        (p.y..=8).all(|y| material::light_opacity(self.block(IVec3::new(p.x, y, p.z))) == 0)
    }
    fn residents(&self, b: Aabb) -> i32 {
        self.residents.iter().filter(|r| r.intersects(b)).count() as i32
    }
    fn golems(&self, _: Aabb) -> i32 {
        0
    }
    fn spawn_golem(&mut self, _: IVec3) {
        panic!("Sparse original fixtures have fewer than 10 residents");
    }
}
fn snapshot(h: &Homes, query: IVec3, random: &JavaRandom) -> Value {
    let live = h.villages.iter().filter(|v| v.active).collect::<Vec<_>>();
    let villages = live
        .iter()
        .map(|v| {
            let doors = v
                .doors
                .iter()
                .map(|d| {
                    json!([
                        d.position.x,
                        d.position.y,
                        d.position.z,
                        d.inside.x * 2,
                        d.inside.z * 2,
                        d.timestamp
                    ])
                })
                .collect::<Vec<_>>();
            json!([
                v.center.x,
                v.center.y,
                v.center.z,
                v.radius,
                v.residents,
                v.reputation.get("Player").copied().unwrap_or(0),
                doors
            ])
        })
        .collect::<Vec<_>>();
    let index = h
        .nearest(query)
        .and_then(|i| live.iter().position(|v| std::ptr::eq(*v, &h.villages[i])))
        .map_or(-1, |n| n as i64);
    json!([villages, index, random.state().to_string()])
}
fn pos(v: &Value) -> IVec3 {
    serde_json::from_value(v.clone()).unwrap()
}
fn edit(w: &mut World, v: &Value) {
    w.blocks.insert(
        IVec3::new(
            v[0].as_i64().unwrap() as i32,
            v[1].as_i64().unwrap() as i32,
            v[2].as_i64().unwrap() as i32,
        ),
        v[3].as_u64().unwrap() as u16,
    );
}
#[test]
fn village_doors_homes_expiry_and_world_rng_match_29280_original_ticks() {
    let mut text = String::new();
    flate2::read::GzDecoder::new(
        include_bytes!("../tests/fixtures/vanilla189-village-homes.json.gz").as_slice(),
    )
    .read_to_string(&mut text)
    .unwrap();
    let cases: Value = serde_json::from_str(&text).unwrap();
    let mut checks = 0;
    for row in cases.as_array().unwrap() {
        let sample = row[0].as_i64().unwrap();
        let mut random = JavaRandom::new(row[1].as_str().unwrap().parse().unwrap());
        let center = pos(&row[2]);
        let query = pos(&row[3]);
        let mut w = World {
            blocks: Default::default(),
            residents: (0..3)
                .map(|i| {
                    let p = DVec3::new(f64::from(center.x + i), 1., f64::from(center.z));
                    Aabb {
                        min: p - DVec3::new(f64::from(0.3f32), 0., f64::from(0.3f32)),
                        max: p + DVec3::new(
                            f64::from(0.3f32),
                            f64::from(1.8f32),
                            f64::from(0.3f32),
                        ),
                    }
                })
                .collect(),
        };
        for b in row[4].as_array().unwrap() {
            edit(&mut w, b);
        }
        let mut h = Homes::default();
        let mut next = row[5].as_array().unwrap().iter().peekable();
        for t in 1..=1220 {
            let step = next.peek().filter(|s| s[0] == t).copied();
            if let Some(step) = step {
                for p in step[1].as_array().unwrap() {
                    h.enqueue(pos(p));
                }
                for e in step[2].as_array().unwrap() {
                    edit(&mut w, e);
                }
                next.next();
            }
            if t == 22 {
                if let Some(i) = h.villages.iter().position(|v| v.active) {
                    h.reward(i, "Player");
                }
            }
            if t == 30 && sample % 4 == 1 {
                w.residents.clear();
            }
            h.tick(&mut w, &mut random);
            if let Some(expected) = step.map(|s| &s[3]).filter(|v| !v.is_null()) {
                assert_eq!(
                    snapshot(&h, query, &random),
                    *expected,
                    "sample {sample}, tick {t}"
                );
                checks += 1;
            }
        }
    }
    assert_eq!(checks, 864);
}
#[test]
fn iron_golem_rare_spawn_eligibility_positions_and_random_match_original() {
    struct GolemWorld {
        layout: i32,
        born: Vec<IVec3>,
    }
    impl VillageWorld for GolemWorld {
        fn block(&self, p: IVec3) -> u16 {
            if (-3..=3).contains(&p.x) && (-1..=1).contains(&p.z) && (1..=2).contains(&p.y) {
                return 64 << 4 | if p.y == 1 { 0 } else { 8 };
            }
            if p.y == 0 && self.layout != 4 {
                return if self.layout == 2 { 44 << 4 | 8 } else { 16 };
            }
            if self.layout == 1 && p.x < 0 && (1..=4).contains(&p.y) {
                return 16;
            }
            0
        }
        fn sky(&self, _: IVec3) -> bool {
            true
        }
        fn residents(&self, _: Aabb) -> i32 {
            unreachable!()
        }
        fn golems(&self, _: Aabb) -> i32 {
            unreachable!()
        }
        fn spawn_golem(&mut self, p: IVec3) {
            self.born.push(p);
        }
    }
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(
        include_bytes!("../tests/fixtures/vanilla189-village-golem.json.gz").as_slice(),
    )
    .read_to_end(&mut bytes)
    .unwrap();
    let data: Value = serde_json::from_slice(&bytes).unwrap();
    let mut count = 0;
    for (i, row) in data.as_array().unwrap().iter().enumerate() {
        let mut world = GolemWorld {
            layout: row[0].as_i64().unwrap() as i32,
            born: Vec::new(),
        };
        let mut random = JavaRandom::new(row[4].as_str().unwrap().parse().unwrap());
        let village = Village {
            center: IVec3::Y,
            radius: 32,
            residents: row[1].as_i64().unwrap() as i32,
            golems: row[2].as_i64().unwrap() as i32,
            doors: (-3..=3)
                .flat_map(|x| {
                    (-1..=1).map(move |z| Door {
                        position: IVec3::new(x, 1, z),
                        inside: IVec3::X,
                        timestamp: 1,
                        restriction: 0,
                    })
                })
                .take(row[3].as_u64().unwrap() as usize)
                .collect(),
            ..Default::default()
        };
        let mut homes = Homes {
            villages: vec![village],
            ..Default::default()
        };
        homes.tick(&mut world, &mut random);
        let expected = row[5]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                serde_json::from_value::<DVec3>(p.clone())
                    .unwrap()
                    .as_ivec3()
            })
            .collect::<Vec<_>>();
        assert_eq!(world.born, expected, "golem case{i}");
        assert_eq!(
            random.state().to_string(),
            row[6].as_str().unwrap(),
            "golem RNG case{i}"
        );
        count += world.born.len();
    }
    assert_eq!(count, 64);
}
