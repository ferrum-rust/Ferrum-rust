use super::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
struct TestWorld {
    pattern: i32,
    blocks: BTreeMap<[i32; 3], u16>,
    chests: BTreeMap<[i32; 3], Vec<Option<LootStack>>>,
    spawners: BTreeMap<[i32; 3], String>,
    forced: Vec<[i32; 3]>,
}
impl FortressWorld for TestWorld {
    fn get(&self, p: [i32; 3]) -> u16 {
        if let Some(&s) = self.blocks.get(&p) {
            return s;
        }
        let floor = if self.pattern == 0 {
            63
        } else {
            28 + (p[0] * 3 + p[2] * 7).rem_euclid(11)
        };
        let id = if !(0..=255).contains(&p[1]) {
            0
        } else if p[1] == 0 {
            7
        } else if p[1] < floor {
            87
        } else if self.pattern == 2 && p[1] < 45 {
            11
        } else {
            0
        };
        id << 4
    }
    fn set(&mut self, p: [i32; 3], state: u16) {
        if !(0..=255).contains(&p[1]) {
            return;
        }
        self.blocks.insert(p, state);
        if state >> 4 == 54 {
            self.chests.entry(p).or_insert_with(|| vec![None; 27]);
        } else {
            self.chests.remove(&p);
        }
        if state >> 4 == 52 {
            self.spawners.entry(p).or_insert_with(|| "Pig".into());
        } else {
            self.spawners.remove(&p);
        }
    }
    fn chest(&mut self, p: [i32; 3], slots: Vec<Option<LootStack>>) {
        self.chests.insert(p, slots);
    }
    fn spawner(&mut self, p: [i32; 3]) {
        self.spawners.insert(p, "Blaze".into());
    }
    fn force_lava(&mut self, p: [i32; 3], _random: &mut JavaRandom) {
        assert_eq!(self.get(p) >> 4, 10);
        self.forced.push(p);
    }
}
fn bbox(value: &Value) -> BoundingBox {
    let n: Vec<i32> = serde_json::from_value(value.clone()).unwrap();
    BoundingBox::new([n[0], n[1], n[2]], [n[3], n[4], n[5]])
}
fn from_nbt(row: &Value) -> Piece {
    let kind = KINDS
        .into_iter()
        .chain([Kind::End, Kind::Start])
        .find(|k| row["id"] == k.id())
        .unwrap();
    let mut piece = Piece::blank(
        kind,
        bbox(&row["BB"]),
        row["O"].as_u64().unwrap() as u8,
        row["GD"].as_i64().unwrap() as i32,
    );
    piece.fill_seed = row["Seed"].as_i64().unwrap_or(0) as i32;
    piece.chest_pending = row["Chest"].as_bool().unwrap_or(false);
    piece.spawner_made = row["Mob"].as_bool().unwrap_or(false);
    piece
}
#[test]
fn all_blocks_rng_chests_spawners_and_force_callback_arguments_match_original() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-fortress-population.json"
    ))
    .unwrap();
    let mut count = 0;
    for (case, row) in fixture.as_array().unwrap().iter().enumerate() {
        let mut piece = from_nbt(&row["before"]);
        let mut world = TestWorld {
            pattern: row["pattern"].as_i64().unwrap() as i32,
            blocks: BTreeMap::new(),
            chests: BTreeMap::new(),
            spawners: BTreeMap::new(),
            forced: Vec::new(),
        };
        let mut random = JavaRandom::new(row["seed"].as_str().unwrap().parse().unwrap());
        for clip in row["clips"].as_array().unwrap() {
            piece.populate(&mut world, bbox(clip), &mut random);
        }
        assert_eq!(
            random.next_long().to_string(),
            row["next"].as_str().unwrap(),
            "RNG case {case} {:?}",
            piece.kind
        );
        assert_eq!(super::tests::nbt(&piece), row["after"], "flags case {case}");
        let expected = row["blocks"].as_array().unwrap();
        assert_eq!(world.blocks.len(), expected.len(), "count case {case}");
        for block in expected {
            let p = std::array::from_fn(|i| block[i].as_i64().unwrap() as i32);
            assert_eq!(
                world.blocks.get(&p).copied(),
                Some(block[3].as_u64().unwrap() as u16),
                "case {case} {:?} block {p:?}",
                piece.kind
            );
            count += 1;
        }
        let mut chests: Vec<([i32; 3], Vec<Option<LootStack>>)> =
            serde_json::from_value(row["chests"].clone()).unwrap();
        chests.sort_by_key(|v| v.0);
        assert_eq!(
            world.chests.into_iter().collect::<Vec<_>>(),
            chests,
            "chests case {case}"
        );
        let mut spawners: Vec<([i32; 3], String)> =
            serde_json::from_value(row["spawners"].clone()).unwrap();
        spawners.sort_by_key(|v| v.0);
        assert_eq!(
            world.spawners.into_iter().collect::<Vec<_>>(),
            spawners,
            "spawners case {case}"
        );
        assert_eq!(json!(world.forced), row["forced"], "force_lava case {case}");
        let restored: Piece =
            serde_json::from_str(&serde_json::to_string(&piece).unwrap()).unwrap();
        assert_eq!(json!(restored), json!(piece));
    }
    assert_eq!(count, 154779);
}
#[test]
fn entrance_forced_lava_blocks_both_rng_streams_and_schedule_match_real_worldserver() {
    use crate::{
        block_ticks::BlockTicks,
        world::{Chunk, World},
    };
    use glam::{IVec3, Vec3};
    use std::{path::PathBuf, sync::Arc};
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-fortress-lava.json"
    ))
    .unwrap();
    for (case, row) in fixture.as_array().unwrap().iter().enumerate() {
        let seed = row["seed"].as_str().unwrap().parse::<i64>().unwrap();
        let orientation = row["orientation"].as_u64().unwrap() as u8;
        let mut chunks = crate::world::ChunkMap::new();
        for x in -2..=2 {
            for z in -2..=2 {
                let mut chunk = Chunk::empty();
                for bx in 0..16 {
                    for bz in 0..16 {
                        chunk.set(bx, 0, bz, 7 << 4);
                    }
                }
                chunks.insert((x, z), Arc::new(chunk));
            }
        }
        let mut world = World {
            chunks,
            spawn: Vec3::ZERO,
            label: String::new(),
            save_path: PathBuf::new(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        };
        let mut ticks = BlockTicks::default();
        ticks.set_nether(true);
        ticks.set_world_random(&JavaRandom::new(seed ^ 189));
        let mut output = Population::default();
        let mut random = JavaRandom::new(seed);
        let mut piece = Piece::blank(
            Kind::Entrance,
            BoundingBox::new([-3, 40, -5], [9, 53, 7]),
            orientation,
            5,
        );
        piece.populate(
            &mut WorldAdapter {
                world: &mut world,
                output: &mut output,
                ticks: &mut ticks,
            },
            BoundingBox::new([-8, 0, -8], [23, 255, 23]),
            &mut random,
        );
        assert_eq!(
            random.next_long().to_string(),
            row["population_next"].as_str().unwrap(),
            "population RNG case {case}"
        );
        assert_eq!(
            ticks.world_random().next_long().to_string(),
            row["world_next"].as_str().unwrap(),
            "world RNG case {case}"
        );
        let mut actual = Vec::new();
        for x in -8..=23 {
            for y in 1..=64 {
                for z in -8..=23 {
                    let s = world.get(IVec3::new(x, y, z));
                    if s != 0 {
                        actual.push([x, y, z, s as i32]);
                    }
                }
            }
        }
        let expected: Vec<[i32; 4]> = serde_json::from_value(row["blocks"].clone()).unwrap();
        assert_eq!(actual, expected, "blocks case {case}");
        let serialized = serde_json::to_value(&ticks).unwrap();
        let mut clocks: Vec<_> = serialized["queue"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.parse::<i64>().unwrap(), v))
            .collect();
        clocks.sort_by_key(|(k, _)| *k);
        let mut pending = Vec::new();
        for (time, entries) in clocks {
            for e in entries.as_array().unwrap() {
                pending.push([
                    time,
                    e["position"][0].as_i64().unwrap(),
                    e["position"][1].as_i64().unwrap(),
                    e["position"][2].as_i64().unwrap(),
                    e["kind"].as_i64().unwrap(),
                ]);
            }
        }
        assert_eq!(json!(pending), row["pending"], "scheduler case {case}");
    }
}
