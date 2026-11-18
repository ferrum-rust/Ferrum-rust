use super::*;
use crate::vanilla::{
    end,
    loot::LootStack,
    population::{decoration::Decorator, GeneratedEntity},
};
use std::{cell::Cell, collections::BTreeMap};
fn initial(p: IVec3, pattern: i32) -> u16 {
    if !(0..256).contains(&p.y) {
        return 0;
    }
    match pattern {
        0 => {
            if p.y < 128 {
                87 << 4
            } else {
                0
            }
        }
        1 => {
            if p.y <= 31 || p.y >= 96 && p.y < 128 {
                87 << 4
            } else {
                0
            }
        }
        2 => {
            if p.y % 16 < 8 && (p.x + p.z).rem_euclid(7) != 0 {
                87 << 4
            } else {
                0
            }
        }
        3 => match p.y {
            254 => 3 << 4 | 2,
            253 => 110 << 4,
            _ => 0,
        },
        4 if p.y <= 64 => 121 << 4,
        5 if p.y <= 64 && p.x * p.x + p.z * p.z < 10000 => 121 << 4,
        _ => 0,
    }
}
fn int_hash(mut h: u64, n: i32) -> u64 {
    for b in n.to_le_bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    h
}
struct TestWorld {
    pattern: i32,
    reads: Cell<i64>,
    read_hash: Cell<u64>,
    changed: BTreeMap<[i32; 3], u16>,
    writes: i32,
    write_hash: u64,
    forced: Vec<[i32; 4]>,
    entities: Vec<GeneratedEntity>,
}
impl TestWorld {
    fn new(pattern: i32) -> Self {
        Self {
            pattern,
            reads: Cell::new(0),
            read_hash: Cell::new(0xcbf29ce484222325),
            changed: BTreeMap::new(),
            writes: 0,
            write_hash: 0xcbf29ce484222325,
            forced: Vec::new(),
            entities: Vec::new(),
        }
    }
}
impl FeatureWorld for TestWorld {
    fn get(&self, p: IVec3) -> u16 {
        self.reads.set(self.reads.get() + 1);
        let mut h = self.read_hash.get();
        for n in p.to_array() {
            h = int_hash(h, n);
        }
        self.read_hash.set(h);
        self.changed
            .get(&p.to_array())
            .copied()
            .unwrap_or_else(|| initial(p, self.pattern))
    }
    fn set(&mut self, p: IVec3, state: u16) {
        self.writes += 1;
        for n in [p.x, p.y, p.z, state as i32, 2] {
            self.write_hash = int_hash(self.write_hash, n);
        }
        if !(0..256).contains(&p.y) {
            return;
        }
        if state == initial(p, self.pattern) {
            self.changed.remove(&p.to_array());
        } else {
            self.changed.insert(p.to_array(), state);
        }
    }
    fn biome(&self, _p: IVec3) -> u8 {
        if self.pattern >= 4 {
            9
        } else {
            8
        }
    }
    fn light(&self, p: IVec3, sky: bool) -> u8 {
        if !sky && (p.x + p.z).rem_euclid(11) == 0 {
            14
        } else {
            0
        }
    }
    fn chest(&mut self, _p: IVec3, _slots: Vec<Option<LootStack>>) {
        panic!("Unexpected chest");
    }
    fn spawner(&mut self, _p: IVec3, _entity: &'static str) {
        panic!("Unexpected spawner");
    }
    fn entity(&mut self, e: GeneratedEntity) {
        self.entities.push(e);
    }
}
impl NetherWorld for TestWorld {
    fn force_lava(&mut self, p: IVec3, _random: &mut JavaRandom) {
        self.forced.push([p.x, p.y, p.z, 10]);
    }
}
#[test]
fn original_dimension_features_population_order_callbacks_and_entity_metadata() {
    let rows: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-dimension-population.json"
    ))
    .unwrap();
    for row in rows.as_array().unwrap() {
        let kind = row["kind"].as_str().unwrap();
        let seed = row["seed"].as_str().unwrap().parse::<i64>().unwrap();
        let pattern = row["pattern"].as_i64().unwrap() as i32;
        let p = IVec3::from_array(std::array::from_fn(|i| {
            row["origin"][i].as_i64().unwrap() as i32
        }));
        let mut random = JavaRandom::new(seed);
        let mut world = TestWorld::new(pattern);
        let result = match kind {
            "fire" => fire(&mut world, &mut random, p),
            "glow1" | "glow2" => glowstone(&mut world, &mut random, p),
            "brown" | "red" => mushroom(
                &mut world,
                &mut random,
                p,
                if kind == "brown" { 39 << 4 } else { 40 << 4 },
            ),
            "quartz" => population::minable_in(&mut world, &mut random, p, 153 << 4, 14, 87),
            "lava" | "enclosed" => lava(&mut world, &mut random, p, kind == "enclosed"),
            "nether" => {
                assert_eq!(row["fortress"], 1);
                after_fortress(&mut world, &mut random, p.x, p.z);
                true
            }
            "end" => {
                end::populate(&mut world, &mut random, &mut Decorator::default(), p.x, p.z)
                    .unwrap();
                true
            }
            _ => panic!("Unexpected {kind}"),
        };
        let context = format!("{kind} seed={seed} pattern={pattern} pos={p:?}");
        assert_eq!(result, row["result"].as_bool().unwrap(), "{context}");
        assert_eq!(
            world.reads.get(),
            row["reads"].as_i64().unwrap(),
            "read count {context}"
        );
        assert_eq!(
            world.read_hash.get().to_string(),
            row["read_hash"].as_str().unwrap(),
            "read sequence {context}"
        );
        assert_eq!(
            world.writes as i64,
            row["writes"].as_i64().unwrap(),
            "writes {context}"
        );
        assert_eq!(
            world.write_hash.to_string(),
            row["write_hash"].as_str().unwrap(),
            "write sequence {context}"
        );
        let mut hash = 0xcbf29ce484222325;
        for (p, s) in &world.changed {
            for n in [p[0], p[1], p[2], *s as i32] {
                hash = int_hash(hash, n);
            }
        }
        assert_eq!(
            hash.to_string(),
            row["hash"].as_str().unwrap(),
            "final states {context}"
        );
        assert_eq!(
            world.changed.len(),
            row["changed"].as_u64().unwrap() as usize,
            "{context}"
        );
        assert_eq!(
            random.next_long().to_string(),
            row["next"].as_str().unwrap(),
            "RNG {context}"
        );
        assert_eq!(
            serde_json::to_value(world.forced).unwrap(),
            row["forced"],
            "immediate callbacks {context}"
        );
        let entities: Vec<_> = world
            .entities
            .iter()
            .map(|e| serde_json::json!({"kind":e.kind,"position":e.position,"yaw":e.yaw.to_bits()}))
            .collect();
        assert_eq!(
            serde_json::to_value(entities).unwrap(),
            row["entities"],
            "entities {context}"
        );
    }
    assert_eq!(rows.as_array().unwrap().len(), 1225);
}

#[test]
fn nether_lava_matches_real_worldserver_blocks_both_rngs_and_pending_ticks() {
    use crate::{
        block_ticks::BlockTicks,
        world::{Chunk, World},
    };
    use glam::Vec3;
    use std::{path::PathBuf, sync::Arc};
    struct Adapter<'a> {
        world: &'a mut World,
        ticks: &'a mut BlockTicks,
    }
    impl FeatureWorld for Adapter<'_> {
        fn get(&self, p: IVec3) -> u16 {
            self.world.get(p)
        }
        fn set(&mut self, p: IVec3, state: u16) {
            self.ticks.set_block(self.world, p, state, false);
        }
        fn biome(&self, _p: IVec3) -> u8 {
            8
        }
        fn light(&self, _p: IVec3, _sky: bool) -> u8 {
            0
        }
        fn chest(&mut self, _p: IVec3, _slots: Vec<Option<LootStack>>) {
            unreachable!()
        }
        fn spawner(&mut self, _p: IVec3, _entity: &'static str) {
            unreachable!()
        }
        fn entity(&mut self, _entity: GeneratedEntity) {
            unreachable!()
        }
    }
    impl NetherWorld for Adapter<'_> {
        fn force_lava(&mut self, p: IVec3, random: &mut JavaRandom) {
            self.ticks.force_liquid_update(self.world, p, 10, random);
        }
    }
    let rows: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-nether-lava.json"
    ))
    .unwrap();
    for (case, row) in rows.as_array().unwrap().iter().enumerate() {
        let seed = row["seed"].as_str().unwrap().parse::<i64>().unwrap();
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
        for edit in row["initial"].as_array().unwrap() {
            let p = IVec3::from_array(std::array::from_fn(|i| edit[i].as_i64().unwrap() as i32));
            world.set(p, edit[3].as_u64().unwrap() as u16);
        }
        let mut ticks = BlockTicks::default();
        ticks.set_nether(true);
        ticks.set_world_random(&JavaRandom::new(seed ^ 189));
        let mut random = JavaRandom::new(seed);
        let result = lava(
            &mut Adapter {
                world: &mut world,
                ticks: &mut ticks,
            },
            &mut random,
            IVec3::new(8, 31, 8),
            row["enclosed"].as_bool().unwrap(),
        );
        assert_eq!(
            result,
            row["result"].as_bool().unwrap(),
            "result case {case}"
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
        let mut hash = 0xcbf29ce484222325u64;
        for x in -8..=23 {
            for y in 1..=40 {
                for z in -8..=23 {
                    for b in world.get(IVec3::new(x, y, z)).to_le_bytes() {
                        hash = (hash ^ b as u64).wrapping_mul(0x100000001b3);
                    }
                }
            }
        }
        assert_eq!(
            hash.to_string(),
            row["hash"].as_str().unwrap(),
            "blocks case {case}"
        );
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
        assert_eq!(
            serde_json::to_value(pending).unwrap(),
            row["pending"],
            "schedule case {case}"
        );
    }
    assert_eq!(rows.as_array().unwrap().len(), 24);
}
