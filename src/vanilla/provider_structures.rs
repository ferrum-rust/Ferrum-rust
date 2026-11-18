//! MapGenStructure live HashMap and serialized Features compound ordering.
use crate::{
    vanilla::{
        biome::BiomeGenerator,
        java_map::{chunk_key, JavaMap},
        mineshaft::{self, Mineshaft},
        monument::{self, Monument},
        random::JavaRandom,
        scattered::{self, Scattered},
        stronghold::{self, Stronghold},
        structure_common::BoundingBox,
        village::{Village, VillageWorldAdapter, VillagerSpawn},
    },
    world::{ChunkPos, World},
};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
pub trait Structure: Clone {
    fn chunk(&self) -> [i32; 2];
    fn reload(&mut self) {}
}
impl Structure for Village {
    fn chunk(&self) -> [i32; 2] {
        self.chunk
    }
}
impl Structure for Mineshaft {
    fn chunk(&self) -> [i32; 2] {
        self.chunk
    }
}
impl Structure for Scattered {
    fn chunk(&self) -> [i32; 2] {
        self.chunk
    }
}
impl Structure for Stronghold {
    fn chunk(&self) -> [i32; 2] {
        self.chunk
    }
}
impl Structure for Monument {
    fn chunk(&self) -> [i32; 2] {
        self.chunk
    }
    fn reload(&mut self) {
        self.reload_as_original();
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StructureStore<T> {
    live: JavaMap<i64, T>,
    /// MapGenStructureData Features has String keys, distinct from live Longs.
    features: JavaMap<String, T>,
}
impl<T> Default for StructureStore<T> {
    fn default() -> Self {
        Self {
            live: JavaMap::default(),
            features: JavaMap::default(),
        }
    }
}
impl<T: Structure> StructureStore<T> {
    pub fn contains(&self, pos: ChunkPos) -> bool {
        self.live.contains_key(&chunk_key(pos.0, pos.1))
    }
    pub fn get(&self, pos: ChunkPos) -> Option<&T> {
        self.live.get(&chunk_key(pos.0, pos.1))
    }
    pub fn entries(&self) -> impl Iterator<Item = (&i64, &T)> {
        self.live.entries()
    }
    pub fn feature_entries(&self) -> impl Iterator<Item = (&String, &T)> {
        self.features.entries()
    }
    pub fn insert(&mut self, start: T) {
        let [x, z] = start.chunk();
        self.live.insert(chunk_key(x, z), start.clone());
        self.features.insert(format!("[{x},{z}]"), start);
    }
    fn persist(&mut self, index: usize) {
        let (_, start) = self.live.entry_mut(index);
        let [x, z] = start.chunk();
        self.features.insert(format!("[{x},{z}]"), start.clone());
    }
    /// NBT writing traverses Features; reading reinserts those String keys into
    /// a new map, and initializeStructureData then builds another Long-key map.
    /// Runtime checkpoints use serde directly; a vanilla-style process restart
    /// must call this explicitly instead of keeping the old live table layout.
    pub fn reload_as_original(&mut self) {
        let serialized = self
            .features
            .entries()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Vec<_>>();
        self.features = JavaMap::default();
        for (k, mut v) in serialized {
            v.reload();
            self.features.insert(k, v);
        }
        self.live = JavaMap::default();
        for (_, v) in self.features.entries() {
            let [x, z] = v.chunk();
            self.live.insert(chunk_key(x, z), v.clone());
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeStructures {
    pub seed: i64,
    pub mineshafts: StructureStore<Mineshaft>,
    pub villages: StructureStore<Village>,
    pub strongholds: StructureStore<Stronghold>,
    stronghold_positions: Option<[[i32; 2]; 3]>,
    pub scattered: StructureStore<Scattered>,
    pub monuments: StructureStore<Monument>,
}
impl NativeStructures {
    pub fn new(seed: i64) -> Self {
        Self {
            seed,
            mineshafts: StructureStore::default(),
            villages: StructureStore::default(),
            strongholds: StructureStore::default(),
            stronghold_positions: None,
            scattered: StructureStore::default(),
            monuments: StructureStore::default(),
        }
    }
    /// Called during both new chunk generation and recreateStructures on disk
    /// load. Each original generator scans its own x-major 17x17 neighborhood.
    pub fn discover(
        &mut self,
        pos: ChunkPos,
        biomes: &BiomeGenerator,
        world_random: &mut JavaRandom,
    ) {
        self.discover_mineshafts(pos);
        self.discover_villages(pos, biomes, world_random);
        self.discover_strongholds(pos, biomes);
        self.discover_scattered(pos, biomes, world_random);
        self.discover_monuments(pos, biomes, world_random);
    }
    pub fn discover_mineshafts(&mut self, pos: ChunkPos) {
        for x in pos.0 - 8..=pos.0 + 8 {
            for z in pos.1 - 8..=pos.1 + 8 {
                if !self.mineshafts.contains((x, z)) {
                    if let Some(start) = mineshaft::candidate(self.seed, x, z) {
                        self.mineshafts.insert(start);
                    }
                }
            }
        }
    }
    pub fn discover_villages(
        &mut self,
        pos: ChunkPos,
        biomes: &BiomeGenerator,
        world_random: &mut JavaRandom,
    ) {
        for x in pos.0 - 8..=pos.0 + 8 {
            for z in pos.1 - 8..=pos.1 + 8 {
                if !self.villages.contains((x, z))
                    && self.lattice(world_random, x, z, 10387312)
                    && Village::can_spawn(self.seed, x, z, biomes)
                {
                    self.villages
                        .insert(Village::layout(self.seed, x, z, biomes));
                }
            }
        }
    }
    pub fn discover_scattered(
        &mut self,
        pos: ChunkPos,
        biomes: &BiomeGenerator,
        world_random: &mut JavaRandom,
    ) {
        for x in pos.0 - 8..=pos.0 + 8 {
            for z in pos.1 - 8..=pos.1 + 8 {
                if !self.scattered.contains((x, z)) && self.lattice(world_random, x, z, 14357617) {
                    let biome = biomes.get_biomes(x * 16 + 8, z * 16 + 8, 1, 1)[0];
                    if let Some(start) = scattered::candidate(self.seed, x, z, biome) {
                        self.scattered.insert(start);
                    }
                }
            }
        }
    }
    pub fn discover_strongholds(&mut self, pos: ChunkPos, biomes: &BiomeGenerator) {
        let positions = *self
            .stronghold_positions
            .get_or_insert_with(|| stronghold::positions(self.seed, biomes));
        for x in pos.0 - 8..=pos.0 + 8 {
            for z in pos.1 - 8..=pos.1 + 8 {
                if !self.strongholds.contains((x, z)) && positions.contains(&[x, z]) {
                    self.strongholds.insert(Stronghold::new(self.seed, x, z));
                }
            }
        }
    }
    pub fn discover_monuments(
        &mut self,
        pos: ChunkPos,
        biomes: &BiomeGenerator,
        world_random: &mut JavaRandom,
    ) {
        for x in pos.0 - 8..=pos.0 + 8 {
            for z in pos.1 - 8..=pos.1 + 8 {
                if !self.monuments.contains((x, z))
                    && monument::can_spawn(self.seed, x, z, biomes, world_random)
                {
                    self.monuments.insert(Monument::new(self.seed, x, z));
                }
            }
        }
    }
    fn lattice(&self, random: &mut JavaRandom, x: i32, z: i32, salt: i64) -> bool {
        let gx = if x < 0 { x.wrapping_sub(31) } else { x } / 32;
        let gz = if z < 0 { z.wrapping_sub(31) } else { z } / 32;
        random.set_seed(
            (gx as i64)
                .wrapping_mul(341873128712)
                .wrapping_add((gz as i64).wrapping_mul(132897987541))
                .wrapping_add(self.seed)
                .wrapping_add(salt),
        );
        // Bytecode draws both offsets before comparing. CFR incorrectly
        // hoists these assignments into a short-circuit return expression.
        let candidate_x = gx.wrapping_mul(32).wrapping_add(random.next_int(24));
        let candidate_z = gz.wrapping_mul(32).wrapping_add(random.next_int(24));
        x == candidate_x && z == candidate_z
    }
    pub fn reload_as_original(&mut self) {
        self.mineshafts.reload_as_original();
        self.villages.reload_as_original();
        self.strongholds.reload_as_original();
        self.stronghold_positions = None;
        self.scattered.reload_as_original();
        self.monuments.reload_as_original();
    }
    fn preflight(world: &World, (x, z): ChunkPos) -> Result<BoundingBox> {
        for cx in x..=x + 1 {
            for cz in z..=z + 1 {
                ensure!(
                    world.chunks.contains_key(&(cx, cz)),
                    "Structure population requires original loaded southeast neighbor {cx},{cz}"
                );
            }
        }
        Ok(BoundingBox::new(
            [x * 16 + 8, 0, z * 16 + 8],
            [x * 16 + 23, 255, z * 16 + 23],
        ))
    }
    /// First structure stage; preserves Java map traversal and shared RNG.
    pub fn populate_mineshafts(
        &mut self,
        world: &mut World,
        pos: ChunkPos,
        random: &mut JavaRandom,
    ) -> Result<mineshaft::Population> {
        let clip = Self::preflight(world, pos)?;
        let mut result = mineshaft::Population::default();
        for i in self.mineshafts.live.indices() {
            let (_, start) = self.mineshafts.live.entry_mut(i);
            if !start
                .bounds
                .intersects_xz(clip.min[0], clip.min[2], clip.max[0], clip.max[2])
            {
                continue;
            }
            let out = start.populate(world, random, clip);
            result.changed.extend(out.changed);
            result.carts.extend(out.carts);
            result.spawners.extend(out.spawners);
            self.mineshafts.persist(i);
        }
        Ok(result)
    }
    /// Second stage. Its bool is the original water-lake suppression flag.
    pub fn populate_villages(
        &mut self,
        world: &mut World,
        pos: ChunkPos,
        random: &mut JavaRandom,
    ) -> Result<(bool, Vec<VillagerSpawn>)> {
        Self::preflight(world, pos)?;
        let mut villages = Vec::new();
        let mut present = false;
        for i in self.villages.live.indices() {
            let (_, start) = self.villages.live.entry_mut(i);
            if start.populate(
                &mut VillageWorldAdapter {
                    world,
                    villagers: &mut villages,
                },
                pos.0,
                pos.1,
                random,
            ) {
                present = true;
                self.villages.persist(i);
            }
        }
        Ok((present, villages))
    }
    /// Third stage. Generated tile events must be reconciled against final
    /// block states, since later clips can overwrite a previous tile block.
    pub fn populate_strongholds(
        &mut self,
        world: &mut World,
        pos: ChunkPos,
        random: &mut JavaRandom,
    ) -> Result<stronghold::Population> {
        let clip = Self::preflight(world, pos)?;
        let mut result = stronghold::Population::default();
        for i in self.strongholds.live.indices() {
            let (_, start) = self.strongholds.live.entry_mut(i);
            if start.populate(
                &mut stronghold::WorldAdapter {
                    world,
                    output: &mut result,
                },
                clip,
                random,
            ) {
                self.strongholds.persist(i);
            }
        }
        Ok(result)
    }
    /// Fourth stage, after strongholds and before monuments.
    pub fn populate_scattered(
        &mut self,
        world: &mut World,
        pos: ChunkPos,
        random: &mut JavaRandom,
    ) -> Result<scattered::Population> {
        let clip = Self::preflight(world, pos)?;
        let mut result = scattered::Population::default();
        for i in self.scattered.live.indices() {
            let (_, start) = self.scattered.live.entry_mut(i);
            if !start
                .bounds
                .intersects_xz(clip.min[0], clip.min[2], clip.max[0], clip.max[2])
            {
                continue;
            }
            start.populate(
                &mut scattered::WorldAdapter {
                    world,
                    output: &mut result,
                },
                clip,
                random,
            );
            self.scattered.persist(i);
        }
        Ok(result)
    }
    /// Fifth structure stage, including the original once-per-clip processed
    /// set and lazy layout reconstruction after an NBT restart.
    pub fn populate_monuments(
        &mut self,
        world: &mut World,
        pos: ChunkPos,
        random: &mut JavaRandom,
    ) -> Result<monument::Population> {
        let clip = Self::preflight(world, pos)?;
        let mut result = monument::Population::default();
        for i in self.monuments.live.indices() {
            let (_, start) = self.monuments.live.entry_mut(i);
            if start.populate(
                &mut monument::WorldAdapter {
                    world,
                    output: &mut result,
                },
                [pos.0, pos.1],
                clip,
                random,
            ) {
                self.monuments.persist(i);
            }
        }
        Ok(result)
    }
    /// All DEFAULT structure stages on an eventful adapter, in the original
    /// generator order. The caller must preserve the original loaded neighbors.
    pub fn populate_with<W>(
        &mut self,
        world: &mut W,
        pos: ChunkPos,
        random: &mut JavaRandom,
    ) -> bool
    where
        W: mineshaft::MineshaftWorld
            + crate::vanilla::village::VillageWorld
            + stronghold::StrongholdWorld
            + scattered::ScatteredWorld
            + monument::MonumentWorld,
    {
        let clip = BoundingBox::new(
            [pos.0 * 16 + 8, 0, pos.1 * 16 + 8],
            [pos.0 * 16 + 23, 255, pos.1 * 16 + 23],
        );
        for i in self.mineshafts.live.indices() {
            let (_, start) = self.mineshafts.live.entry_mut(i);
            if start
                .bounds
                .intersects_xz(clip.min[0], clip.min[2], clip.max[0], clip.max[2])
            {
                start.populate_with(world, random, clip);
                self.mineshafts.persist(i);
            }
        }
        let mut village_present = false;
        for i in self.villages.live.indices() {
            let (_, start) = self.villages.live.entry_mut(i);
            if start.populate(world, pos.0, pos.1, random) {
                village_present = true;
                self.villages.persist(i);
            }
        }
        for i in self.strongholds.live.indices() {
            let (_, start) = self.strongholds.live.entry_mut(i);
            if start.populate(world, clip, random) {
                self.strongholds.persist(i);
            }
        }
        for i in self.scattered.live.indices() {
            let (_, start) = self.scattered.live.entry_mut(i);
            if start
                .bounds
                .intersects_xz(clip.min[0], clip.min[2], clip.max[0], clip.max[2])
            {
                start.populate(world, clip, random);
                self.scattered.persist(i);
            }
        }
        for i in self.monuments.live.indices() {
            let (_, start) = self.monuments.live.entry_mut(i);
            if start.populate(world, [pos.0, pos.1], clip, random) {
                self.monuments.persist(i);
            }
        }
        village_present
    }
    pub fn populate_monuments_with(
        &mut self,
        world: &mut impl monument::MonumentWorld,
        pos: ChunkPos,
        random: &mut JavaRandom,
    ) -> bool {
        let clip = BoundingBox::new(
            [pos.0 * 16 + 8, 0, pos.1 * 16 + 8],
            [pos.0 * 16 + 23, 255, pos.1 * 16 + 23],
        );
        let mut generated = false;
        for i in self.monuments.live.indices() {
            let (_, start) = self.monuments.live.entry_mut(i);
            if start.populate(world, [pos.0, pos.1], clip, random) {
                generated = true;
                self.monuments.persist(i);
            }
        }
        generated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn assert_stage<T: Structure>(
        store: &StructureStore<T>,
        stage: &Value,
        random: &mut JavaRandom,
        label: &str,
    ) {
        assert_eq!(
            json!(store
                .entries()
                .map(|(k, _)| k.to_string())
                .collect::<Vec<_>>()),
            stage["live"],
            "live order {label}"
        );
        assert_eq!(
            json!(store
                .feature_entries()
                .map(|(k, _)| k.clone())
                .collect::<Vec<_>>()),
            stage["features"],
            "NBT Features order {label}"
        );
        assert_eq!(
            random.next_long().to_string(),
            stage["world_next_long"].as_str().unwrap(),
            "shared world RNG {label}"
        );
    }
    #[test]
    fn original_structure_discovery_nbt_restart_and_shared_world_rng_match() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-structure-order.json"
        ))
        .unwrap();
        for row in corpus["cases"].as_array().unwrap() {
            let seed = row["seed"].as_str().unwrap().parse::<i64>().unwrap();
            let biomes = BiomeGenerator::new(seed);
            let mut stores = NativeStructures::new(seed);
            let mut world_random = JavaRandom::new(1234);
            for (i, step) in row["steps"].as_array().unwrap().iter().enumerate() {
                if step["restart"].as_bool().unwrap() {
                    stores.reload_as_original();
                }
                let pos = (
                    step["chunk"][0].as_i64().unwrap() as i32,
                    step["chunk"][1].as_i64().unwrap() as i32,
                );
                let label = format!("seed{seed} step{i} {pos:?}");
                stores.discover_mineshafts(pos);
                assert_stage(
                    &stores.mineshafts,
                    &step["stages"][0],
                    &mut world_random,
                    &label,
                );
                stores.discover_villages(pos, &biomes, &mut world_random);
                assert_stage(
                    &stores.villages,
                    &step["stages"][1],
                    &mut world_random,
                    &label,
                );
                // Stage three uses only its own generator RNG, but its oracle
                // tail probe consumes the shared world stream as usual.
                stores.discover_strongholds(pos, &biomes);
                assert_stage(
                    &stores.strongholds,
                    &step["stages"][2],
                    &mut world_random,
                    &label,
                );
                stores.discover_scattered(pos, &biomes, &mut world_random);
                assert_stage(
                    &stores.scattered,
                    &step["stages"][3],
                    &mut world_random,
                    &label,
                );
                stores.discover_monuments(pos, &biomes, &mut world_random);
                assert_stage(
                    &stores.monuments,
                    &step["stages"][4],
                    &mut world_random,
                    &label,
                );
                stores = serde_json::from_slice(&serde_json::to_vec(&stores).unwrap()).unwrap();
            }
        }
    }
}
