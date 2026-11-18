use super::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
fn hash(mut h: u64, value: u64) -> u64 {
    for b in value.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}
struct TestWorld {
    pattern: i32,
    sea: i32,
    blocks: BTreeMap<[i32; 3], u16>,
    elders: Vec<[i32; 3]>,
}
impl MonumentWorld for TestWorld {
    fn get(&self, p: [i32; 3]) -> u16 {
        if let Some(&s) = self.blocks.get(&p) {
            return s;
        }
        let floor = if self.pattern == 0 {
            35
        } else {
            31 + (p[0] * 3 + p[2] * 7).rem_euclid(13)
        };
        let id = if !(0..=255).contains(&p[1]) {
            0
        } else if p[1] == 0 {
            7
        } else if p[1] < floor {
            1
        } else if p[1] < self.sea {
            9
        } else {
            0
        };
        id << 4
    }
    fn set(&mut self, p: [i32; 3], state: u16) {
        if (0..=255).contains(&p[1]) {
            self.blocks.insert(p, state);
        }
    }
    fn sea_level(&self) -> i32 {
        self.sea
    }
    fn elder(&mut self, p: [i32; 3]) {
        self.elders.push(p);
    }
}
impl TestWorld {
    fn hash(&self) -> u64 {
        let mut h = 14695981039346656037u64;
        for (p, &state) in &self.blocks {
            for n in [p[0], p[1], p[2], state as i32] {
                h = hash(h, n as i64 as u64);
            }
        }
        h
    }
}
#[test]
fn all_blocks_population_rng_elders_and_processed_restart_state_match_original() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-monument-population.json"
    ))
    .unwrap();
    let (mut snapshots, mut total) = (0, 0);
    for (case, row) in fixture.as_array().unwrap().iter().enumerate() {
        let seed = row["seed"].as_str().unwrap().parse::<i64>().unwrap();
        let cx = row["chunk"][0].as_i64().unwrap() as i32;
        let cz = row["chunk"][1].as_i64().unwrap() as i32;
        let mut start = Monument::new(seed, cx, cz);
        let mut world = TestWorld {
            pattern: row["pattern"].as_i64().unwrap() as i32,
            sea: row["sea"].as_i64().unwrap() as i32,
            blocks: BTreeMap::new(),
            elders: Vec::new(),
        };
        let mut random = JavaRandom::new(row["randomSeed"].as_str().unwrap().parse().unwrap());
        for (i, step) in row["steps"].as_array().unwrap().iter().enumerate() {
            let pos: [i32; 2] = serde_json::from_value(step["chunk"].clone()).unwrap();
            let clip = BoundingBox::new(
                [pos[0] * 16 + 8, 0, pos[1] * 16 + 8],
                [pos[0] * 16 + 23, 255, pos[1] * 16 + 23],
            );
            if step["reload"].as_bool().unwrap() {
                start = serde_json::from_str(&serde_json::to_string(&start).unwrap()).unwrap();
                start.reload_as_original();
            }
            let generated = start.populate(&mut world, pos, clip, &mut random);
            assert_eq!(
                generated,
                step["generated"].as_bool().unwrap(),
                "generation case {case} clip {i}"
            );
            assert_eq!(
                random.next_long().to_string(),
                step["next"].as_str().unwrap(),
                "RNG case {case} clip {i}"
            );
            assert_eq!(
                world.blocks.len() as u64,
                step["count"].as_u64().unwrap(),
                "count case {case} clip {i}"
            );
            assert_eq!(
                world.hash().to_string(),
                step["hash"].as_str().unwrap(),
                "blocks case {case} clip {i}"
            );
            assert_eq!(
                json!(world.elders),
                step["elders"],
                "elders case {case} clip {i}"
            );
            snapshots += 1;
        }
        assert_eq!(world.elders.len(), 3);
        total += world.blocks.len();
    }
    assert_eq!(snapshots, 357);
    assert_eq!(total, 2350001);
}
fn bbox(b: BoundingBox) -> [i32; 6] {
    [b.min[0], b.min[1], b.min[2], b.max[0], b.max[1], b.max[2]]
}
#[test]
fn candidate_decisions_world_rng_and_all_room_graph_fields_match_original() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-monument.json"
    ))
    .unwrap();
    for row in fixture["queries"].as_array().unwrap() {
        let seed = row["seed"].as_str().unwrap().parse::<i64>().unwrap();
        let biomes = BiomeGenerator::new(seed);
        let real = row["real"].as_bool().unwrap();
        let mut random = JavaRandom::new(0);
        let mut h = 14695981039346656037u64;
        let mut positives = Vec::new();
        for x in -96..=96 {
            for z in -96..=96 {
                let can = if real {
                    can_spawn(seed, x, z, &biomes, &mut random)
                } else {
                    lattice(seed, x, z, &mut random)
                };
                h = hash(h, can as u64);
                h = hash(h, random.next_long() as u64);
                if can {
                    positives.push([x, z]);
                }
            }
        }
        assert_eq!(
            json!(positives),
            row["candidates"],
            "candidate seed={seed} real={real}"
        );
        assert_eq!(
            h.to_string(),
            row["hash"].as_str().unwrap(),
            "RNG seed={seed} real={real}"
        );
    }
    let mut count = 0;
    for row in fixture["starts"].as_array().unwrap() {
        let seed = row["seed"].as_str().unwrap().parse::<i64>().unwrap();
        let cx = row["chunk"][0].as_i64().unwrap() as i32;
        let cz = row["chunk"][1].as_i64().unwrap() as i32;
        let mut random = JavaRandom::new(0);
        let start = Monument::with_random(seed, cx, cz, &mut random);
        let context = format!("seed={seed} chunk={cx},{cz}");
        assert_eq!(
            random.next_long().to_string(),
            row["next"].as_str().unwrap(),
            "RNG {context}"
        );
        assert_eq!(json!(bbox(start.bounds)), row["bounds"]);
        assert_eq!(json!(start.orientation), row["orientation"]);
        assert_eq!(json!(start.core), row["core"]);
        let mut rooms = start.rooms.iter().flatten().collect::<Vec<_>>();
        rooms.sort_by_key(|r| r.id);
        let rooms=rooms.into_iter().map(|r|json!({"id":r.id,"neighbors":r.neighbors.map(|n|n.map(|i|start.rooms[i].as_ref().unwrap().id)),"open":r.open,"claimed":r.claimed,"source":r.source,"scan":r.scan})).collect::<Vec<_>>();
        assert_eq!(json!(rooms), row["rooms"], "room graph {context}");
        let pieces=start.pieces.iter().map(|p|json!({"kind":p.kind.name(),"bounds":bbox(p.bounds),"room":p.room.map(|i|start.rooms[i].as_ref().unwrap().id),"variant":p.variant})).collect::<Vec<_>>();
        assert_eq!(json!(pieces), row["pieces"], "pieces {context}");
        count += pieces.len();
    }
    assert_eq!(count, 1334);
}
