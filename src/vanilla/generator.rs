//! Native pre-population provider and explicitly isolated structure previews.
//! The default provider performs no approximate decoration or Java execution.
use super::{
    biome::BiomeGenerator,
    mineshaft::{self, Mineshaft},
    random::JavaRandom,
    terrain::Terrain,
    village::{Village, VillageWorldAdapter, VillagerSpawn},
};
use crate::world::{Chunk, ChunkPos, World};
use anyhow::{ensure, Result};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex, OnceLock},
};

pub const COVERAGE:&str="Original 1.8.9 DEFAULT biomes, density, surface, caves and ravines. Population/decorations/structures are not applied by this provider.";

pub struct Generator {
    pub seed: i64,
    pub biomes: BiomeGenerator,
    terrain: Terrain,
}
impl Generator {
    pub fn new(seed: i64) -> Self {
        Self {
            seed,
            biomes: BiomeGenerator::new(seed),
            terrain: Terrain::new(seed),
        }
    }
    pub fn configured(seed: i64, settings: super::terrain::Settings, amplified: bool) -> Self {
        // Factory's JSON biome index skips Hell and The End.
        let fixed = match settings.fixed_biome {
            0..=7 => Some(settings.fixed_biome),
            8..=37 => Some(settings.fixed_biome + 2),
            _ => None,
        };
        let biomes =
            BiomeGenerator::configured(seed, settings.biome_size, settings.river_size, fixed);
        let mut terrain = Terrain::new(seed);
        terrain.settings = settings;
        terrain.amplified = amplified;
        Self {
            seed,
            biomes,
            terrain,
        }
    }
    pub fn chunk(&self, pos: ChunkPos) -> Result<Chunk> {
        self.terrain.generate(pos.0, pos.1, &self.biomes)
    }
    pub fn read_chunks(&self, positions: &[ChunkPos]) -> Result<Vec<(ChunkPos, Arc<Chunk>)>> {
        positions
            .par_iter()
            .map(|&pos| self.chunk(pos).map(|c| (pos, Arc::new(c))))
            .collect()
    }
    /// Safe native preview spawn. This deliberately does not claim original
    /// WorldServer spawn parity before its population/loading events are ported.
    pub fn preview_spawn(&self) -> Result<glam::Vec3> {
        let mut random = JavaRandom::new(self.seed);
        let grid = self.biomes.get_biomes_for_generation(-64, -64, 129, 129);
        let mut found = None;
        let mut choices = 0;
        for (i, b) in grid.into_iter().enumerate() {
            if matches!(b, 1 | 4 | 5 | 18 | 19 | 21 | 22)
                && (found.is_none() || random.next_int(choices + 1) == 0)
            {
                found = Some(((i as i32 % 129 - 64) * 4, (i as i32 / 129 - 64) * 4));
                choices += 1;
            }
        }
        let (mut x, mut z) = found.unwrap_or((0, 0));
        let mut last_chunk = None;
        for _ in 0..1000 {
            let pos = (x.div_euclid(16), z.div_euclid(16));
            if last_chunk.as_ref().is_none_or(|(p, _)| *p != pos) {
                last_chunk = Some((pos, self.chunk(pos)?));
            }
            let chunk = &last_chunk.as_ref().unwrap().1;
            let (lx, lz) = (x.rem_euclid(16), z.rem_euclid(16));
            let mut y = 63;
            while y < 255 && chunk.get(lx, y + 1, lz) >> 4 != 0 {
                y += 1;
            }
            if chunk.get(lx, y, lz) >> 4 == 2 {
                return Ok(glam::Vec3::new(
                    x as f32 + 0.5,
                    y as f32 + 1.01,
                    z as f32 + 0.5,
                ));
            }
            x += random.next_int(64) - random.next_int(64);
            z += random.next_int(64) - random.next_int(64);
        }
        let chunk = self.chunk((x.div_euclid(16), z.div_euclid(16)))?;
        let y = (0..256)
            .rev()
            .find(|&y| chunk.get(x.rem_euclid(16), y, z.rem_euclid(16)) != 0)
            .unwrap_or(63);
        Ok(glam::Vec3::new(
            x as f32 + 0.5,
            y as f32 + 1.01,
            z as f32 + 0.5,
        ))
    }
}
static GENERATOR_CACHE: OnceLock<Mutex<Option<Arc<Generator>>>> = OnceLock::new();

pub fn clear_cache() {
    if let Some(cache) = GENERATOR_CACHE.get() {
        *cache.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// Retain only one seed's immutable noise tables; chunk data itself is owned by
/// World/streaming. Concurrent requests keep their own Arc when the seed changes.
pub fn shared(seed: u64) -> Arc<Generator> {
    let mut guard = GENERATOR_CACHE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(g) = guard.as_ref().filter(|g| g.seed == seed as i64) {
        return g.clone();
    }
    let generator = Arc::new(Generator::new(seed as i64));
    *guard = Some(generator.clone());
    generator
}
pub fn read_chunks(seed: u64, positions: &[ChunkPos]) -> Result<Vec<(ChunkPos, Arc<Chunk>)>> {
    shared(seed).read_chunks(positions)
}
pub fn preview_spawn(seed: u64) -> Result<glam::Vec3> {
    shared(seed).preview_spawn()
}

/// Persistent original component metadata. This is storage for the future
/// ChunkProviderServer event pipeline, not a substitute for that event order.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StructureState {
    pub version: u32,
    pub seed: i64,
    pub villages: Vec<Village>,
    pub mineshafts: Vec<Mineshaft>,
    pub discovered_chunks: Vec<[i32; 2]>,
    pub population_events: Vec<[i32; 2]>,
}
impl StructureState {
    pub fn new(seed: i64) -> Self {
        Self {
            version: 1,
            seed,
            villages: Vec::new(),
            mineshafts: Vec::new(),
            discovered_chunks: Vec::new(),
            population_events: Vec::new(),
        }
    }
    /// Original MapGenBase x-major 17x17 candidate scan. This does not populate
    /// anything, consume decoration RNG, or mark chunks as populated.
    pub fn discover_for_chunk(&mut self, generator: &Generator, chunk: ChunkPos) -> Result<()> {
        ensure!(
            self.version == 1 && self.seed == generator.seed,
            "Structure metadata belongs to a different native generator"
        );
        for x in chunk.0 - 8..=chunk.0 + 8 {
            for z in chunk.1 - 8..=chunk.1 + 8 {
                if self.discovered_chunks.contains(&[x, z]) {
                    continue;
                }
                self.discovered_chunks.push([x, z]);
                if let Some(start) = mineshaft::candidate(self.seed, x, z) {
                    self.mineshafts.push(start);
                }
                if Village::can_spawn(self.seed, x, z, &generator.biomes) {
                    self.villages
                        .push(Village::layout(self.seed, x, z, &generator.biomes));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum PreviewKind {
    Village,
    Mineshaft,
}
/// Nearest viable candidate to the origin within a bounded search. This uses
/// original placement predicates; it never relocates or fabricates a structure.
pub fn nearest_structure_chunk(seed: u64, kind: PreviewKind) -> Result<ChunkPos> {
    let generator = shared(seed);
    match kind {
        PreviewKind::Village => {
            let mut candidates = (-8..=8)
                .flat_map(|x| (-8..=8).map(move |z| Village::candidate(seed as i64, x, z)))
                .collect::<Vec<_>>();
            candidates.sort_by_key(|&(x, z)| (x as i64 * x as i64 + z as i64 * z as i64, x, z));
            for (x, z) in candidates {
                if Village::can_spawn(seed as i64, x, z, &generator.biomes)
                    && Village::layout(seed as i64, x, z, &generator.biomes).valid
                {
                    return Ok((x, z));
                }
            }
        }
        PreviewKind::Mineshaft => {
            let mut candidates = (-128..=128)
                .flat_map(|x| (-128..=128).map(move |z| (x, z)))
                .collect::<Vec<_>>();
            candidates.sort_by_key(|&(x, z)| (x * x + z * z, x, z));
            for (x, z) in candidates {
                if mineshaft::candidate(seed as i64, x, z).is_some() {
                    return Ok((x, z));
                }
            }
        }
    }
    anyhow::bail!("No original {kind:?} candidate found inside the bounded preview search")
}
pub struct StructurePreview {
    pub world: World,
    pub structures: StructureState,
    pub villagers: Vec<VillagerSpawn>,
    pub mineshaft_entities: Vec<mineshaft::Population>,
}
/// Inspect one original structure using a documented x-major population order.
/// This explicitly is NOT the original complete world's population result:
/// other structures, decorations, block callbacks and load order are excluded.
pub fn canonical_structure_preview(
    seed: u64,
    kind: PreviewKind,
    chunk: ChunkPos,
) -> Result<StructurePreview> {
    let generator = shared(seed);
    let mut structures = StructureState::new(seed as i64);
    let bounds = match kind {
        PreviewKind::Village => {
            ensure!(
                Village::can_spawn(seed as i64, chunk.0, chunk.1, &generator.biomes),
                "Chunk is not an original village candidate for this seed"
            );
            let start = Village::layout(seed as i64, chunk.0, chunk.1, &generator.biomes);
            let bounds = start.bounds;
            structures.villages.push(start);
            bounds
        }
        PreviewKind::Mineshaft => {
            let start = mineshaft::candidate(seed as i64, chunk.0, chunk.1).ok_or_else(|| {
                anyhow::anyhow!("Chunk is not an original mineshaft candidate for this seed")
            })?;
            let bounds = start.bounds;
            structures.mineshafts.push(start);
            bounds
        }
    };
    let min = (
        (bounds.min[0] - 8).div_euclid(16),
        (bounds.min[2] - 8).div_euclid(16),
    );
    let max = (
        (bounds.max[0] - 8).div_euclid(16),
        (bounds.max[2] - 8).div_euclid(16),
    );
    let positions = (min.0 - 1..=max.0 + 2)
        .flat_map(|x| (min.1 - 1..=max.1 + 2).map(move |z| (x, z)))
        .collect::<Vec<_>>();
    ensure!(
        positions.len() <= 1024,
        "Structure preview exceeds 1024 chunk safety bound"
    );
    let mut world = World {
        chunks: generator
            .read_chunks(&positions)?
            .into_iter()
            .collect::<crate::world::ChunkMap<_>>(),
        spawn: glam::Vec3::new(
            bounds.min[0] as f32 + 0.5,
            bounds.max[1] as f32 + 3.,
            bounds.min[2] as f32 + 0.5,
        ),
        label: "Canonical isolated native structure preview (not complete vanilla population)"
            .into(),
        save_path: std::path::PathBuf::new(),
        edits: Default::default(),
        imported: false,
        light_revision: 0,
    };
    let mut villagers = Vec::new();
    let mut mineshaft_entities = Vec::new();
    for x in min.0..=max.0 {
        for z in min.1..=max.1 {
            let mut random = population_random(seed as i64, x, z);
            let clip = super::structure_common::BoundingBox::new(
                [x * 16 + 8, 0, z * 16 + 8],
                [x * 16 + 23, 255, z * 16 + 23],
            );
            for start in &mut structures.mineshafts {
                mineshaft_entities.push(start.populate(&mut world, &mut random, clip));
            }
            for start in &mut structures.villages {
                start.populate(
                    &mut VillageWorldAdapter {
                        world: &mut world,
                        villagers: &mut villagers,
                    },
                    x,
                    z,
                    &mut random,
                );
            }
            structures.population_events.push([x, z]);
        }
    }
    // This World is newly allocated for the preview, with no player edits.
    world.edits.clear();
    let center = [
        (bounds.min[0] + bounds.max[0]) / 2,
        (bounds.min[2] + bounds.max[2]) / 2,
    ];
    let surface = (0..256)
        .rev()
        .find(|&y| world.get(glam::IVec3::new(center[0], y, center[1])) != 0)
        .unwrap_or(63);
    let highest_piece = structures
        .villages
        .iter()
        .flat_map(|s| &s.pieces)
        .map(|p| p.bounds.max[1])
        .max()
        .unwrap_or(surface);
    world.spawn = glam::Vec3::new(
        center[0] as f32 + 0.5,
        surface.max(highest_piece) as f32 + 3.,
        center[1] as f32 + 0.5,
    );
    Ok(StructurePreview {
        world,
        structures,
        villagers,
        mineshaft_entities,
    })
}
pub fn population_random(seed: i64, x: i32, z: i32) -> JavaRandom {
    let mut random = JavaRandom::new(seed);
    let a = (random.next_long() / 2).wrapping_mul(2).wrapping_add(1);
    let b = (random.next_long() / 2).wrapping_mul(2).wrapping_add(1);
    random.set_seed(
        (x as i64)
            .wrapping_mul(a)
            .wrapping_add((z as i64).wrapping_mul(b))
            ^ seed,
    );
    random
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn batches_streamed_chunks_and_request_order_are_identical() {
        let g = Generator::new(-1);
        let positions = [(-1, 0), (0, 0), (-1, -1), (1, -1)];
        let batch = g.read_chunks(&positions).unwrap();
        for &pos in positions.iter().rev() {
            let single = g.read_chunks(&[pos]).unwrap();
            let expected = &batch.iter().find(|(p, _)| *p == pos).unwrap().1;
            assert_eq!(single[0].1.blocks, expected.blocks);
            assert_eq!(single[0].1.biomes, expected.biomes);
        }
    }
    #[test]
    fn shared_cache_keeps_signed_seed_bits_and_reuses_tables() {
        let a = shared(u64::MAX);
        let b = shared(u64::MAX);
        assert_eq!(a.seed, -1);
        assert!(Arc::ptr_eq(&a, &b));
    }
}
