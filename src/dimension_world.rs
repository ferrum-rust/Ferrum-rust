//! Multi-dimension (Nether `-1` and End `1`) chunk loading, procedural generation,
//! Fortress monster-spawn queries, Nether `Teleporter`, and End podium/platform builders.

use crate::{
    block_ticks::BlockTicks,
    inventory::Stack,
    vanilla::{
        drops::DropNbt,
        end,
        fortress::{
            self, Fortress, Population as FortressPopulation, WorldAdapter as FortressAdapter,
        },
        lighting::{Kind as LightKind, Lighting},
        loot::LootStack,
        material, middle_pick,
        nether::{self, population::NetherWorld},
        population::{
            decoration::Decorator, FeatureWorld, GeneratedEntity, GeneratedEntityKind,
        },
        random::JavaRandom,
        runtime::stack_from_loot,
        structure_common::BoundingBox,
    },
    world::{Chunk, ChunkMap, ChunkPos, EditJournal, World, HEIGHT},
    Options,
};
use anyhow::Result;
use glam::{DVec3, IVec3, Vec3};
use rayon::prelude::*;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    hash::Hash,
    path::PathBuf,
    sync::{Arc, LazyLock, Mutex},
};

#[derive(Clone, Debug, Default)]
pub struct GeneratedDimensionMeta {
    pub chests: Vec<([i32; 3], Vec<(usize, Stack)>)>,
    pub spawners: Vec<([i32; 3], &'static str)>,
    pub end_crystals: Vec<[f64; 3]>,
    pub ender_dragon: Option<[f64; 3]>,
}

struct BoundedCache<K, V> {
    capacity: usize,
    map: HashMap<K, (V, u64)>,
    order: BTreeMap<u64, K>,
    tick: u64,
}

impl<K: Eq + Hash + Clone, V: Clone> BoundedCache<K, V> {
    fn new(capacity: usize) -> Self {
        let cap = capacity.max(1);
        Self {
            capacity: cap,
            map: HashMap::with_capacity(cap.min(256)),
            order: BTreeMap::new(),
            tick: 0,
        }
    }

    fn get(&mut self, key: &K) -> Option<V> {
        let (val, old_tick) = {
            let (v, t) = self.map.get(key)?;
            (v.clone(), *t)
        };
        self.tick = self.tick.wrapping_add(1);
        let new_tick = self.tick;
        self.order.remove(&old_tick);
        self.order.insert(new_tick, key.clone());
        if let Some((_, slot_tick)) = self.map.get_mut(key) {
            *slot_tick = new_tick;
        }
        Some(val)
    }

    fn insert(&mut self, key: K, value: V) {
        if let Some((_, old_tick)) = self.map.remove(&key) {
            self.order.remove(&old_tick);
        } else {
            while self.map.len() >= self.capacity {
                let Some((_, evicted)) = self.order.pop_first() else {
                    self.map.clear();
                    break;
                };
                self.map.remove(&evicted);
            }
        }
        self.tick = self.tick.wrapping_add(1);
        let new_tick = self.tick;
        self.order.insert(new_tick, key.clone());
        self.map.insert(key, (value, new_tick));
    }

    fn clear(&mut self) {
        self.map.clear();
        self.map.shrink_to_fit();
        self.order.clear();
        self.tick = 0;
    }
}

type RawChunkKey = (i64, i32, i32, i32);
type NetherOriginKey = (i64, bool, i32, i32);
type PopulatedChunkKey = (i64, bool, i32, i32, i32);
type FortressRegionKey = (i64, i32, i32);

#[derive(Clone)]
struct NetherOriginDelta {
    /// Block diffs `(packed_index, state)` for `[(ox, oz), (ox+1, oz), (ox, oz+1), (ox+1, oz+1)]`.
    quadrants: [Vec<(u16, u16)>; 4],
    chests: Vec<([i32; 3], Vec<(usize, Stack)>)>,
    spawners: Vec<([i32; 3], &'static str)>,
}

const CHUNK_CACHE_CAPACITY: usize = 2600;

static EMPTY_CHUNK: LazyLock<Arc<Chunk>> = LazyLock::new(|| Arc::new(Chunk::empty()));
static NETHER_TERRAINS: LazyLock<Mutex<BoundedCache<i64, Arc<nether::Terrain>>>> =
    LazyLock::new(|| Mutex::new(BoundedCache::new(8)));
static END_TERRAINS: LazyLock<Mutex<BoundedCache<i64, Arc<end::Terrain>>>> =
    LazyLock::new(|| Mutex::new(BoundedCache::new(8)));
static RAW_CHUNKS: LazyLock<Mutex<BoundedCache<RawChunkKey, Arc<Chunk>>>> =
    LazyLock::new(|| Mutex::new(BoundedCache::new(CHUNK_CACHE_CAPACITY)));
static NETHER_ORIGINS: LazyLock<Mutex<BoundedCache<NetherOriginKey, Arc<NetherOriginDelta>>>> =
    LazyLock::new(|| Mutex::new(BoundedCache::new(CHUNK_CACHE_CAPACITY)));
static POPULATED_CHUNKS: LazyLock<
    Mutex<BoundedCache<PopulatedChunkKey, (Arc<Chunk>, GeneratedDimensionMeta)>>,
> = LazyLock::new(|| Mutex::new(BoundedCache::new(CHUNK_CACHE_CAPACITY)));
static LIT_CHUNKS: LazyLock<Mutex<BoundedCache<PopulatedChunkKey, Arc<Chunk>>>> =
    LazyLock::new(|| Mutex::new(BoundedCache::new(CHUNK_CACHE_CAPACITY)));
static FORTRESS_REGIONS: LazyLock<Mutex<BoundedCache<FortressRegionKey, Option<Arc<Fortress>>>>> =
    LazyLock::new(|| Mutex::new(BoundedCache::new(512)));

pub fn clear_caches() {
    RAW_CHUNKS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    NETHER_ORIGINS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    POPULATED_CHUNKS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    LIT_CHUNKS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    FORTRESS_REGIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    NETHER_TERRAINS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    END_TERRAINS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    crate::vanilla::generator::clear_cache();
    crate::world_types::clear_cache();
}

const LIGHT_DIRS: [IVec3; 6] = [
    IVec3::NEG_Y,
    IVec3::Y,
    IVec3::NEG_Z,
    IVec3::Z,
    IVec3::NEG_X,
    IVec3::X,
];

fn empty_mini_world() -> World {
    World {
        chunks: ChunkMap::default(),
        spawn: Vec3::ZERO,
        label: String::new(),
        save_path: PathBuf::new(),
        edits: EditJournal::default(),
        imported: false,
        light_revision: 0,
    }
}

fn nether_terrain(seed: i64) -> Arc<nether::Terrain> {
    if let Some(terrain) = NETHER_TERRAINS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&seed)
    {
        return terrain;
    }
    let built = Arc::new(nether::Terrain::new(seed));
    NETHER_TERRAINS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(seed, built.clone());
    built
}

fn end_terrain(seed: i64) -> Arc<end::Terrain> {
    if let Some(terrain) = END_TERRAINS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&seed)
    {
        return terrain;
    }
    let built = Arc::new(end::Terrain::new(seed));
    END_TERRAINS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(seed, built.clone());
    built
}

fn raw_dimension_chunk(seed: i64, dimension: i32, cx: i32, cz: i32) -> Arc<Chunk> {
    let key = (seed, dimension, cx, cz);
    if let Some(chunk) = RAW_CHUNKS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
    {
        return chunk;
    }
    let chunk = match dimension {
        -1 => Arc::new(nether_terrain(seed).generate(cx, cz)),
        1 => Arc::new(end_terrain(seed).generate(cx, cz)),
        _ => Arc::clone(&EMPTY_CHUNK),
    };
    RAW_CHUNKS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, chunk.clone());
    chunk
}

fn fortress_in_region(seed: i64, rx: i32, rz: i32) -> Option<Arc<Fortress>> {
    let key = (seed, rx, rz);
    if let Some(cached) = FORTRESS_REGIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
    {
        return cached;
    }
    let mut probe = JavaRandom::new(0);
    probe.set_seed(((rx ^ (rz << 4)) as i64) ^ seed);
    probe.next_i32();
    let generated = if probe.next_int(3) == 0 {
        let fcx = (rx << 4) + 4 + probe.next_int(8);
        let fcz = (rz << 4) + 4 + probe.next_int(8);
        fortress::candidate(seed, fcx, fcz).map(Arc::new)
    } else {
        None
    };
    FORTRESS_REGIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, generated.clone());
    generated
}

fn origin_random(seed: i64, ox: i32, oz: i32) -> JavaRandom {
    let mut rng = JavaRandom::new(seed);
    let a = (rng.next_long() / 2).wrapping_mul(2).wrapping_add(1);
    let b = (rng.next_long() / 2).wrapping_mul(2).wrapping_add(1);
    rng.set_seed(
        (ox as i64)
            .wrapping_mul(a)
            .wrapping_add((oz as i64).wrapping_mul(b))
            ^ seed,
    );
    rng
}

struct DimensionFeatureAdapter<'a> {
    world: &'a mut World,
    ticks: &'a mut BlockTicks,
    default_biome: u8,
    nether: bool,
    chests: Vec<([i32; 3], Vec<Option<LootStack>>)>,
    spawners: Vec<([i32; 3], &'static str)>,
    entities: Vec<GeneratedEntity>,
}

impl DimensionFeatureAdapter<'_> {
    #[inline]
    fn fast_set(&mut self, p: IVec3, state: u16) {
        if matches!(state >> 4, 8..=13 | 122 | 145) {
            self.ticks.set_nether(self.nether);
            let _ = self.ticks.set_block(self.world, p, state, false);
            return;
        }
        if !(0..HEIGHT).contains(&p.y) {
            return;
        }
        let cpos = (p.x.div_euclid(16), p.z.div_euclid(16));
        let Some(chunk_arc) = self.world.chunks.get_mut(&cpos) else {
            return;
        };
        let lx = p.x.rem_euclid(16);
        let lz = p.z.rem_euclid(16);
        let idx = Chunk::index(lx, p.y, lz);
        let prev = chunk_arc.blocks.value_at(idx);
        if prev == state {
            return;
        }
        let id = state >> 4;
        if chunk_arc.light.is_none()
            && chunk_arc.tiles.is_empty()
            && chunk_arc.hoppers.is_empty()
            && chunk_arc.pistons.is_empty()
            && chunk_arc.random_tick_sections.get().is_none()
            && chunk_arc.scheduled_tick_blocks.get().is_none()
            && chunk_arc.partial_light_sections.get().is_none()
            && chunk_arc.skulls.get().is_none()
            && chunk_arc.signs.get().is_none()
            && !matches!(id, 52 | 154 | 176 | 177)
            && !matches!(prev >> 4, 36 | 52 | 154)
        {
            let chunk = Arc::make_mut(chunk_arc);
            chunk.blocks[idx] = state;
            if state != 0 && p.y + 1 > chunk.top {
                chunk.top = p.y + 1;
            }
        } else {
            self.world.set_raw(p, state);
        }
    }
}

impl FeatureWorld for DimensionFeatureAdapter<'_> {
    #[inline]
    fn get(&self, p: IVec3) -> u16 {
        self.world.get(p)
    }

    #[inline]
    fn set(&mut self, p: IVec3, state: u16) {
        self.fast_set(p, state);
    }

    fn biome(&self, p: IVec3) -> u8 {
        self.world
            .chunks
            .get(&(p.x.div_euclid(16), p.z.div_euclid(16)))
            .map(|c| c.biomes[(p.z.rem_euclid(16) * 16 + p.x.rem_euclid(16)) as usize])
            .unwrap_or(self.default_biome)
    }

    fn light(&self, p: IVec3, sky: bool) -> u8 {
        if sky {
            return 0;
        }
        let mut best = 0u8;
        for dy in -2i32..=2 {
            let qy = p.y + dy;
            if !(0..HEIGHT).contains(&qy) {
                continue;
            }
            for dz in -2i32..=2 {
                for dx in -2i32..=2 {
                    let dist = (dx.abs() + dy.abs() + dz.abs()) as u8;
                    if dist > 2 {
                        continue;
                    }
                    let q = IVec3::new(p.x + dx, qy, p.z + dz);
                    let emitted = material::light_value(self.world.get(q));
                    best = best.max(emitted.saturating_sub(dist));
                }
            }
        }
        best
    }

    fn chest(&mut self, p: IVec3, slots: Vec<Option<LootStack>>) {
        self.chests.push((p.to_array(), slots));
    }

    fn spawner(&mut self, p: IVec3, entity: &'static str) {
        self.spawners.push((p.to_array(), entity));
    }

    fn entity(&mut self, entity: GeneratedEntity) {
        self.entities.push(entity);
    }
}

impl NetherWorld for DimensionFeatureAdapter<'_> {
    fn force_lava(&mut self, p: IVec3, random: &mut JavaRandom) {
        self.ticks.set_nether(true);
        let _ = self.ticks.force_liquid_update(self.world, p, 10, random);
    }
}

fn write_spawner_entity_nbt(chunk: &mut Chunk, world_pos: IVec3, entity: &str) {
    let local = [
        world_pos.x.rem_euclid(16),
        world_pos.y,
        world_pos.z.rem_euclid(16),
    ];
    if !(0..HEIGHT).contains(&local[1]) || chunk.get(local[0], local[1], local[2]) >> 4 != 52 {
        return;
    }
    let existing = chunk.tiles.get(&local).map(Arc::as_ref);
    if let Some(mut tile) = middle_pick::tile_snapshot(52, world_pos, existing, None) {
        if let DropNbt::Compound(tag) = &mut tile {
            tag.insert("EntityId".into(), DropNbt::String(entity.to_string()));
        }
        chunk.tiles.insert(local, Arc::new(tile));
    }
}

fn convert_loot_slots(slots: &[Option<LootStack>]) -> Vec<(usize, Stack)> {
    slots
        .iter()
        .enumerate()
        .filter_map(|(i, slot)| slot.as_ref().map(|loot| (i, stack_from_loot(loot))))
        .collect()
}

fn extract_chunk_diff(raw: &Chunk, modified: &Chunk) -> Vec<(u16, u16)> {
    let max_top = raw.top.max(modified.top).clamp(0, HEIGHT) as usize;
    let end = (max_top * 256).min(65536);
    raw.blocks.with_slice(|raw_s| {
        modified.blocks.with_slice(|mod_s| {
            let mut diffs = Vec::new();
            for i in 0..end {
                if mod_s[i] != raw_s[i] {
                    diffs.push((i as u16, mod_s[i]));
                }
            }
            diffs
        })
    })
}

fn nether_origin_delta(seed: i64, structures: bool, ox: i32, oz: i32) -> Arc<NetherOriginDelta> {
    let key = (seed, structures, ox, oz);
    if let Some(cached) = NETHER_ORIGINS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
    {
        return cached;
    }

    let mut mini_world = empty_mini_world();
    mini_world.chunks.reserve(16);
    let mut raw_quads: [Option<Arc<Chunk>>; 4] = [None, None, None, None];
    for rz in (oz - 1)..=(oz + 2) {
        for rx in (ox - 1)..=(ox + 2) {
            let raw = raw_dimension_chunk(seed, -1, rx, rz);
            if (ox..=ox + 1).contains(&rx) && (oz..=oz + 1).contains(&rz) {
                let qi = ((rz - oz) * 2 + (rx - ox)) as usize;
                raw_quads[qi] = Some(Arc::clone(&raw));
            }
            mini_world.chunks.insert((rx, rz), raw);
        }
    }

    let mut fort_out = FortressPopulation::default();
    let mut pop_rng = origin_random(seed, ox, oz);
    let world_rng = JavaRandom::new(
        seed ^ (ox as i64)
            .wrapping_mul(341_873_128_712)
            .wrapping_add((oz as i64).wrapping_mul(132_897_987_541)),
    );
    let mut ticks = BlockTicks::default();
    ticks.set_nether(true);
    ticks.set_fall_instantly(true);
    ticks.set_world_random(&world_rng);

    if structures {
        let clip = BoundingBox::new(
            [ox * 16 + 8, 0, oz * 16 + 8],
            [ox * 16 + 23, 255, oz * 16 + 23],
        );
        let base_rx = ox >> 4;
        let max_rx = (ox + 1) >> 4;
        let base_rz = oz >> 4;
        let max_rz = (oz + 1) >> 4;
        let mut fort_adapter = FortressAdapter {
            world: &mut mini_world,
            output: &mut fort_out,
            ticks: &mut ticks,
        };
        for rz in (base_rz - 1)..=(max_rz + 1) {
            for rx in (base_rx - 1)..=(max_rx + 1) {
                if let Some(fortress) = fortress_in_region(seed, rx, rz) {
                    if fortress
                        .bounds
                        .intersects_xz(clip.min[0], clip.min[2], clip.max[0], clip.max[2])
                    {
                        let mut fort_copy = (*fortress).clone();
                        fort_copy.populate(&mut fort_adapter, clip, &mut pop_rng);
                    }
                }
            }
        }
    }

    let mut feature_adapter = DimensionFeatureAdapter {
        world: &mut mini_world,
        ticks: &mut ticks,
        default_biome: 8,
        nether: true,
        chests: Vec::new(),
        spawners: Vec::new(),
        entities: Vec::new(),
    };
    nether::population::after_fortress(&mut feature_adapter, &mut pop_rng, ox, oz);
    let feature_chests = feature_adapter.chests;
    let feature_spawners = feature_adapter.spawners;

    let quad_coords = [(ox, oz), (ox + 1, oz), (ox, oz + 1), (ox + 1, oz + 1)];
    let quadrants = std::array::from_fn(|qi| {
        let qpos = quad_coords[qi];
        let after = &mini_world.chunks[&qpos];
        let raw = raw_quads[qi].as_ref().unwrap();
        if Arc::ptr_eq(after, raw) {
            Vec::new()
        } else {
            extract_chunk_diff(raw, after)
        }
    });

    let mut chests = Vec::new();
    for container in &fort_out.chests {
        chests.push((container.position, convert_loot_slots(&container.slots)));
    }
    for (p, slots) in &feature_chests {
        chests.push((*p, convert_loot_slots(slots)));
    }

    let mut spawners = Vec::new();
    for &p in &fort_out.spawners {
        spawners.push((p, "Blaze"));
    }
    spawners.extend(feature_spawners);

    let delta = Arc::new(NetherOriginDelta {
        quadrants,
        chests,
        spawners,
    });
    NETHER_ORIGINS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, Arc::clone(&delta));
    delta
}

fn populate_nether_chunk(
    seed: i64,
    structures: bool,
    cx: i32,
    cz: i32,
) -> (Arc<Chunk>, GeneratedDimensionMeta) {
    let origins = [
        ((cx - 1, cz - 1), 3usize),
        ((cx, cz - 1), 2usize),
        ((cx - 1, cz), 1usize),
        ((cx, cz), 0usize),
    ];
    let deltas = origins.map(|((ox, oz), qi)| (nether_origin_delta(seed, structures, ox, oz), qi));

    let raw_arc = raw_dimension_chunk(seed, -1, cx, cz);
    let any_edits = deltas.iter().any(|(d, qi)| !d.quadrants[*qi].is_empty());
    let any_tiles = deltas.iter().any(|(d, _)| {
        d.chests
            .iter()
            .any(|(p, _)| (p[0].div_euclid(16), p[2].div_euclid(16)) == (cx, cz))
            || d.spawners
                .iter()
                .any(|(p, _)| (p[0].div_euclid(16), p[2].div_euclid(16)) == (cx, cz))
    });

    if !any_edits && !any_tiles {
        return (raw_arc, GeneratedDimensionMeta::default());
    }

    let mut chunk = (*raw_arc).clone();
    {
        let dst: &mut [u16] = &mut chunk.blocks;
        let mut top = chunk.top;
        for (delta, qi) in &deltas {
            for &(idx, state) in &delta.quadrants[*qi] {
                let index = usize::from(idx);
                dst[index] = state;
                if state != 0 {
                    let y = (index >> 8) as i32;
                    if y + 1 > top {
                        top = y + 1;
                    }
                }
            }
        }
        chunk.top = top;
    }

    let mut meta = GeneratedDimensionMeta::default();
    let mut chest_map: BTreeMap<[i32; 3], Vec<(usize, Stack)>> = BTreeMap::new();
    let mut spawner_map: BTreeMap<[i32; 3], &'static str> = BTreeMap::new();

    for (delta, _) in &deltas {
        for (p, slots) in &delta.chests {
            if (p[0].div_euclid(16), p[2].div_euclid(16)) == (cx, cz)
                && (0..HEIGHT).contains(&p[1])
                && chunk.get(p[0].rem_euclid(16), p[1], p[2].rem_euclid(16)) >> 4 == 54
            {
                chest_map.insert(*p, slots.clone());
            }
        }
        for &(p, entity) in &delta.spawners {
            if (p[0].div_euclid(16), p[2].div_euclid(16)) == (cx, cz)
                && (0..HEIGHT).contains(&p[1])
                && chunk.get(p[0].rem_euclid(16), p[1], p[2].rem_euclid(16)) >> 4 == 52
            {
                write_spawner_entity_nbt(&mut chunk, IVec3::from_array(p), entity);
                spawner_map.insert(p, entity);
            }
        }
    }
    meta.chests = chest_map.into_iter().collect();
    meta.spawners = spawner_map.into_iter().collect();

    (Arc::new(chunk), meta)
}

fn populate_end_chunk(seed: i64, cx: i32, cz: i32) -> (Arc<Chunk>, GeneratedDimensionMeta) {
    let mut mini_world = empty_mini_world();
    mini_world.chunks.reserve(25);
    for rz in (cz - 2)..=(cz + 2) {
        for rx in (cx - 2)..=(cx + 2) {
            mini_world
                .chunks
                .insert((rx, rz), raw_dimension_chunk(seed, 1, rx, rz));
        }
    }

    let mut meta = GeneratedDimensionMeta::default();
    let origins = [(cx - 1, cz - 1), (cx, cz - 1), (cx - 1, cz), (cx, cz)];

    for (ox, oz) in origins {
        let mut pop_rng = origin_random(seed, ox, oz);
        let world_rng = JavaRandom::new(
            seed ^ (ox as i64)
                .wrapping_mul(341_873_128_712)
                .wrapping_add((oz as i64).wrapping_mul(132_897_987_541)),
        );
        let mut ticks = BlockTicks::default();
        ticks.set_fall_instantly(true);
        ticks.set_world_random(&world_rng);
        let mut decorator = Decorator::default();
        let mut adapter = DimensionFeatureAdapter {
            world: &mut mini_world,
            ticks: &mut ticks,
            default_biome: 9,
            nether: false,
            chests: Vec::new(),
            spawners: Vec::new(),
            entities: Vec::new(),
        };
        let _ = end::populate(&mut adapter, &mut pop_rng, &mut decorator, ox, oz);

        for entity in adapter.entities {
            match entity.kind {
                GeneratedEntityKind::EnderCrystal => {
                    let ex = (entity.position[0].floor() as i32).div_euclid(16);
                    let ez = (entity.position[2].floor() as i32).div_euclid(16);
                    if (ex, ez) == (cx, cz)
                        && !meta.end_crystals.iter().any(|c| {
                            (c[0] - entity.position[0]).abs() < 1e-4
                                && (c[1] - entity.position[1]).abs() < 1e-4
                                && (c[2] - entity.position[2]).abs() < 1e-4
                        })
                    {
                        meta.end_crystals.push(entity.position);
                    }
                }
                GeneratedEntityKind::EnderDragon => {
                    if (cx, cz) == (0, 0) && (ox, oz) == (0, 0) {
                        meta.ender_dragon = Some(entity.position);
                    }
                }
            }
        }
    }

    let chunk = mini_world.chunks[&(cx, cz)].clone();
    (chunk, meta)
}

fn populated_dimension_chunk(
    seed: i64,
    structures: bool,
    dimension: i32,
    pos: ChunkPos,
) -> (Arc<Chunk>, GeneratedDimensionMeta) {
    let key = (seed, structures, dimension, pos.0, pos.1);
    if let Some(cached) = POPULATED_CHUNKS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
    {
        return cached;
    }
    let built = match dimension {
        -1 => populate_nether_chunk(seed, structures, pos.0, pos.1),
        1 => populate_end_chunk(seed, pos.0, pos.1),
        _ => (Arc::clone(&EMPTY_CHUNK), GeneratedDimensionMeta::default()),
    };
    POPULATED_CHUNKS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, built.clone());
    built
}

fn apply_edits_to_chunk(
    chunk: &mut Chunk,
    pos: ChunkPos,
    edits: &BTreeMap<[i32; 3], u16>,
) -> bool {
    if edits.is_empty() {
        return false;
    }
    let min_x = pos.0 * 16;
    let max_x = min_x + 15;
    let min_z = pos.1 * 16;
    let max_z = min_z + 15;
    let mut changed = false;
    for (&[x, y, z], &state) in edits.range([min_x, 0, i32::MIN]..=[max_x, HEIGHT - 1, i32::MAX]) {
        if !(0..HEIGHT).contains(&y) || !(min_z..=max_z).contains(&z) {
            continue;
        }
        chunk.set(x.rem_euclid(16), y, z.rem_euclid(16), state);
        changed = true;
    }
    changed
}

fn has_edits_in_region(
    edits: &BTreeMap<[i32; 3], u16>,
    min_cx: i32,
    max_cx: i32,
    min_cz: i32,
    max_cz: i32,
) -> bool {
    if edits.is_empty() {
        return false;
    }
    let min_x = min_cx * 16;
    let max_x = max_cx * 16 + 15;
    let min_z = min_cz * 16;
    let max_z = max_cz * 16 + 15;
    edits
        .range([min_x, 0, i32::MIN]..=[max_x, HEIGHT - 1, i32::MAX])
        .any(|(&[_, y, z], _)| (0..HEIGHT).contains(&y) && (min_z..=max_z).contains(&z))
}

fn compute_no_sky_chunk_light(
    mini_world: &World,
    center: ChunkPos,
) -> Option<Arc<crate::vanilla::lighting::ChunkLight>> {
    // Initialize an empty no-sky lighting state and add ONLY the 3x3 active chunks
    // (the outer 5x5 border stays in `mini_world.chunks` solely so `area_loaded(..., 17)` succeeds).
    let mut lighting = Lighting::new(&empty_mini_world(), true);
    let mut grid: [Option<&Chunk>; 9] = [None; 9];
    for dz in -1..=1 {
        for dx in -1..=1 {
            let npos = (center.0 + dx, center.1 + dz);
            if let Some(chunk) = mini_world.chunks.get(&npos) {
                lighting.add_chunk(mini_world, npos);
                grid[((dz + 1) * 3 + (dx + 1)) as usize] = Some(chunk.as_ref());
            }
        }
    }

    let neighbor_state = |wx: i32, wy: i32, wz: i32| -> u16 {
        if !(0..HEIGHT).contains(&wy) {
            return 0;
        }
        let dcx = wx.div_euclid(16) - center.0;
        let dcz = wz.div_euclid(16) - center.1;
        if (-1..=1).contains(&dcx) && (-1..=1).contains(&dcz) {
            if let Some(c) = grid[((dcz + 1) * 3 + (dcx + 1)) as usize] {
                return c.get(wx.rem_euclid(16), wy, wz.rem_euclid(16));
            }
        }
        0
    };

    let min_x = center.0 * 16 - 14;
    let max_x = center.0 * 16 + 29;
    let min_z = center.1 * 16 - 14;
    let max_z = center.1 * 16 + 29;
    let mut enclosed_center_emitters = Vec::new();

    for dz in -1..=1 {
        for dx in -1..=1 {
            let Some(chunk) = grid[((dz + 1) * 3 + (dx + 1)) as usize] else {
                continue;
            };
            let cx = center.0 + dx;
            let cz = center.1 + dz;
            let top = chunk.top.clamp(0, HEIGHT);
            if top <= 0 {
                continue;
            }
            let lx_min = (min_x - cx * 16).clamp(0, 15);
            let lx_max = (max_x - cx * 16).clamp(0, 15);
            let lz_min = (min_z - cz * 16).clamp(0, 15);
            let lz_max = (max_z - cz * 16).clamp(0, 15);
            if lx_min > lx_max || lz_min > lz_max {
                continue;
            }
            let in_center = dx == 0 && dz == 0;
            let end = (top as usize * 256).min(65536);

            // Fast linear scan over chunk.blocks slice, only touching light queues for true emitters.
            let mut emitters: Vec<(IVec3, u8, bool)> = Vec::new();
            chunk.blocks.with_slice(|blocks| {
                for (idx, &state) in blocks[..end].iter().enumerate() {
                    if state == 0 {
                        continue;
                    }
                    let emitted = material::light_value(state);
                    if emitted == 0 {
                        continue;
                    }
                    let lx = (idx & 15) as i32;
                    let lz = ((idx >> 4) & 15) as i32;
                    if lx < lx_min || lx > lx_max || lz < lz_min || lz > lz_max {
                        continue;
                    }
                    let y = (idx >> 8) as i32;
                    let p = IVec3::new(cx * 16 + lx, y, cz * 16 + lz);
                    let exposed = LIGHT_DIRS.iter().any(|&d| {
                        let q = p + d;
                        if !(0..HEIGHT).contains(&q.y) {
                            return false;
                        }
                        let ns = neighbor_state(q.x, q.y, q.z);
                        material::light_opacity(ns) < 15 && material::light_value(ns) < emitted
                    });
                    if exposed || in_center {
                        emitters.push((p, emitted, exposed));
                    }
                }
            });

            for (p, emitted, exposed) in emitters {
                if exposed {
                    if lighting.get(LightKind::Block, p) < emitted {
                        lighting.check(mini_world, LightKind::Block, p);
                    }
                } else if in_center {
                    enclosed_center_emitters.push((p, emitted));
                }
            }
        }
    }

    for (p, emitted) in enclosed_center_emitters {
        if lighting.get(LightKind::Block, p) < emitted {
            lighting.set(mini_world, LightKind::Block, p, emitted);
        }
    }

    lighting.render_snapshot(center)
}

fn clean_procedural_lit_chunk(
    seed: i64,
    structures: bool,
    dimension: i32,
    pos: ChunkPos,
) -> Arc<Chunk> {
    let key = (seed, structures, dimension, pos.0, pos.1);
    if let Some(cached) = LIT_CHUNKS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
    {
        return cached;
    }

    let mut mini_world = empty_mini_world();
    mini_world.chunks.reserve(25);
    for dz in -2..=2 {
        for dx in -2..=2 {
            let npos = (pos.0 + dx, pos.1 + dz);
            if dx.abs() <= 1 && dz.abs() <= 1 {
                let (chunk_arc, _) = populated_dimension_chunk(seed, structures, dimension, npos);
                mini_world.chunks.insert(npos, chunk_arc);
            } else {
                mini_world.chunks.insert(npos, Arc::clone(&EMPTY_CHUNK));
            }
        }
    }

    let light = compute_no_sky_chunk_light(&mini_world, pos);
    let mut center_chunk = (*mini_world.chunks[&pos]).clone();
    center_chunk.light = light;
    center_chunk.visual_light = None;
    center_chunk.light_edits.clear();
    center_chunk.prepare_tick_indexes();
    let built = Arc::new(center_chunk);

    LIT_CHUNKS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, Arc::clone(&built));
    built
}

fn procedural_lit_chunk(
    dimension: i32,
    options: &Options,
    edits: &BTreeMap<[i32; 3], u16>,
    pos: ChunkPos,
) -> Arc<Chunk> {
    let seed = options.seed as i64;
    let structures = options.world_rules.structures;
    // Always ensure the clean procedural lit chunk is cached in `LIT_CHUNKS` first.
    let clean = clean_procedural_lit_chunk(seed, structures, dimension, pos);
    if !has_edits_in_region(edits, pos.0 - 1, pos.0 + 1, pos.1 - 1, pos.1 + 1) {
        return clean;
    }

    let mut mini_world = empty_mini_world();
    mini_world.chunks.reserve(25);
    for dz in -2..=2 {
        for dx in -2..=2 {
            let npos = (pos.0 + dx, pos.1 + dz);
            if dx.abs() <= 1 && dz.abs() <= 1 {
                let (mut chunk_arc, _) =
                    populated_dimension_chunk(seed, structures, dimension, npos);
                if has_edits_in_region(edits, npos.0, npos.0, npos.1, npos.1) {
                    let chunk_mut = Arc::make_mut(&mut chunk_arc);
                    apply_edits_to_chunk(chunk_mut, npos, edits);
                }
                mini_world.chunks.insert(npos, chunk_arc);
            } else {
                mini_world.chunks.insert(npos, Arc::clone(&EMPTY_CHUNK));
            }
        }
    }

    let light = compute_no_sky_chunk_light(&mini_world, pos);
    let mut center_chunk = (*mini_world.chunks[&pos]).clone();
    center_chunk.light = light;
    center_chunk.visual_light = None;
    center_chunk.light_edits.clear();
    center_chunk.prepare_tick_indexes();
    Arc::new(center_chunk)
}

fn load_or_generate_neighbor(
    dimension: i32,
    options: &Options,
    edits: &BTreeMap<[i32; 3], u16>,
    pos: ChunkPos,
) -> Result<Arc<Chunk>> {
    if let Some(root) = &options.world {
        if let Some(mut disk_chunk) = World::load_dimension_chunk(root, dimension, pos)? {
            apply_edits_to_chunk(&mut disk_chunk, pos, edits);
            return Ok(Arc::new(disk_chunk));
        }
    }
    let (mut chunk_arc, _) = populated_dimension_chunk(
        options.seed as i64,
        options.world_rules.structures,
        dimension,
        pos,
    );
    if has_edits_in_region(edits, pos.0, pos.0, pos.1, pos.1) {
        apply_edits_to_chunk(Arc::make_mut(&mut chunk_arc), pos, edits);
    }
    Ok(chunk_arc)
}

pub fn read_dimension_chunks(
    dimension: i32,
    options: &Options,
    edits: &BTreeMap<[i32; 3], u16>,
    positions: &[ChunkPos],
) -> Result<Vec<(ChunkPos, Arc<Chunk>)>> {
    positions
        .par_iter()
        .map(|&pos| -> Result<(ChunkPos, Arc<Chunk>)> {
            if let Some(root) = &options.world {
                if let Some(mut chunk) = World::load_dimension_chunk(root, dimension, pos)? {
                    let edited = apply_edits_to_chunk(&mut chunk, pos, edits);
                    if edited || chunk.light.is_none() {
                        let mut mini_world = empty_mini_world();
                        mini_world.chunks.reserve(25);
                        for dz in -2..=2 {
                            for dx in -2..=2 {
                                let npos = (pos.0 + dx, pos.1 + dz);
                                if (dx, dz) == (0, 0) {
                                    mini_world.chunks.insert(npos, Arc::new(chunk.clone()));
                                } else if dx.abs() <= 1 && dz.abs() <= 1 {
                                    mini_world.chunks.insert(
                                        npos,
                                        load_or_generate_neighbor(dimension, options, edits, npos)?,
                                    );
                                } else {
                                    mini_world.chunks.insert(npos, Arc::clone(&EMPTY_CHUNK));
                                }
                            }
                        }
                        chunk.light = compute_no_sky_chunk_light(&mini_world, pos);
                        chunk.visual_light = None;
                        chunk.light_edits.clear();
                    }
                    chunk.prepare_tick_indexes();
                    return Ok((pos, Arc::new(chunk)));
                }
            }
            Ok((pos, procedural_lit_chunk(dimension, options, edits, pos)))
        })
        .collect()
}

pub fn chunk_population_meta(
    dimension: i32,
    options: &Options,
    pos: ChunkPos,
) -> GeneratedDimensionMeta {
    if !matches!(dimension, -1 | 1) {
        return GeneratedDimensionMeta::default();
    }
    if let Some(root) = &options.world {
        if matches!(World::load_dimension_chunk(root, dimension, pos), Ok(Some(_))) {
            return GeneratedDimensionMeta::default();
        }
    }
    populated_dimension_chunk(
        options.seed as i64,
        options.world_rules.structures,
        dimension,
        pos,
    )
    .1
}

pub fn fortress_monster_spawn(world: &World, seed: i64, pos: IVec3) -> bool {
    if !(0..HEIGHT).contains(&pos.y) {
        return false;
    }
    let p = pos.to_array();
    let center_rx = pos.x.div_euclid(16) >> 4;
    let center_rz = pos.z.div_euclid(16) >> 4;
    for drz in -1..=1 {
        for drx in -1..=1 {
            let Some(fortress) = fortress_in_region(seed, center_rx + drx, center_rz + drz) else {
                continue;
            };
            if !fortress.bounds.contains(p) {
                continue;
            }
            if fortress.pieces.iter().any(|piece| piece.bounds.contains(p)) {
                return true;
            }
            if pos.y > 0 && world.get(pos - IVec3::Y) >> 4 == 112 {
                return true;
            }
        }
    }
    false
}

// ============================================================================
// 1.8.9 `Teleporter` & `BlockPortal.func_181089_f` (`PatternHelper`)
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum HorizFacing {
    #[default]
    North, // -Z (opposite = South, rotY +X = East, ccw -X = West)
    South, // +Z
    West,  // -X
    East,  // +X
}

impl HorizFacing {
    #[inline]
    pub fn opposite(self) -> Self {
        match self {
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }

    #[inline]
    pub fn rotate_y(self) -> Self {
        match self {
            Self::North => Self::East,
            Self::East => Self::South,
            Self::South => Self::West,
            Self::West => Self::North,
        }
    }

    #[inline]
    pub fn rotate_y_ccw(self) -> Self {
        match self {
            Self::North => Self::West,
            Self::West => Self::South,
            Self::South => Self::East,
            Self::East => Self::North,
        }
    }

    #[inline]
    pub fn step(self) -> IVec3 {
        match self {
            Self::North => IVec3::NEG_Z,
            Self::South => IVec3::Z,
            Self::West => IVec3::NEG_X,
            Self::East => IVec3::X,
        }
    }

    /// 1.8.9 `EnumFacing.getHorizontalIndex()`: South=0, West=1, North=2, East=3.
    #[inline]
    pub fn horizontal_index(self) -> i32 {
        match self {
            Self::South => 0,
            Self::West => 1,
            Self::North => 2,
            Self::East => 3,
        }
    }

    #[inline]
    pub fn is_x_axis(self) -> bool {
        matches!(self, Self::West | Self::East)
    }

    #[inline]
    pub fn axis_offset(self) -> i32 {
        match self {
            Self::East | Self::South => 1,
            Self::West | Self::North => -1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NetherPortalEntry {
    pub trigger_pos: glam::DVec3,
    pub yaw: f32,
    pub pitch: f32,
    pub velocity: glam::DVec3,
    pub last_portal_pos: glam::IVec3,
    pub last_portal_vec: glam::DVec3,
    pub teleport_direction: HorizFacing,
}

#[derive(Clone, Copy, Debug)]
pub struct CachedPortalEntry {
    pub pos: glam::IVec3,
    pub last_update_tick: u64,
}

#[derive(Clone, Debug, Default)]
pub struct TeleporterCache {
    pub entries: std::collections::HashMap<i64, CachedPortalEntry>,
}

#[inline]
fn chunk_xz_to_int(x: i32, z: i32) -> i64 {
    (x as i64 & 0xFFFF_FFFF) | ((z as i64 & 0xFFFF_FFFF) << 32)
}

pub fn resolve_nether_portal_target(
    from_dim: i32,
    to_dim: i32,
    entry_feet: glam::DVec3,
    border_min_x: f64,
    border_max_x: f64,
    border_min_z: f64,
    border_max_z: f64,
) -> (glam::DVec3, i32, i32) {
    let scale = match (from_dim, to_dim) {
        (0, -1) => 0.125,
        (-1, 0) => 8.0,
        _ => 1.0,
    };
    let raw_x = entry_feet.x * scale;
    let raw_z = entry_feet.z * scale;
    let clamp_border = |v: f64, min_b: f64, max_b: f64| -> f64 {
        let lo = (min_b + 16.0).min(max_b - 16.0);
        let hi = (max_b - 16.0).max(min_b + 16.0);
        v.clamp(lo, hi)
    };
    let clamped_x = clamp_border(raw_x, border_min_x, border_max_x);
    let clamped_z = clamp_border(raw_z, border_min_z, border_max_z);
    // Exact 1.8.9 `(int) d0` truncation toward zero + MathHelper.clamp_int(-29999872, 29999872).
    let tx = (clamped_x as i32).clamp(-29_999_872, 29_999_872);
    let tz = (clamped_z as i32).clamp(-29_999_872, 29_999_872);
    let target_feet = DVec3::new(tx as f64, entry_feet.y, tz as f64);
    (target_feet, tx, tz)
}

pub fn find_existing_portal_fast(
    to_dim: i32,
    target_feet: glam::DVec3,
    tx: i32,
    tz: i32,
    total_ticks: u64,
    cache: &mut TeleporterCache,
    saved_edits: &std::collections::BTreeMap<[i32; 3], u16>,
    loaded_chunks: Option<&crate::world::ChunkMap>,
    options: &crate::Options,
) -> Option<glam::IVec3> {
    find_existing_portal_inner(
        to_dim,
        target_feet,
        tx,
        tz,
        total_ticks,
        cache,
        saved_edits,
        loaded_chunks,
        options.world.as_deref(),
    )
}

fn find_existing_portal_inner(
    to_dim: i32,
    target_feet: glam::DVec3,
    tx: i32,
    tz: i32,
    total_ticks: u64,
    cache: &mut TeleporterCache,
    saved_edits: &std::collections::BTreeMap<[i32; 3], u16>,
    loaded_chunks: Option<&crate::world::ChunkMap>,
    world_root: Option<&std::path::Path>,
) -> Option<glam::IVec3> {
    let actual_height = if to_dim == -1 { 128 } else { 256 };
    cache
        .entries
        .retain(|_, e| total_ticks.saturating_sub(e.last_update_tick) <= 600);

    let key = chunk_xz_to_int(tx, tz);
    let mut disk_cache: HashMap<ChunkPos, Option<Arc<Chunk>>> = HashMap::new();

    let is_portal_at = |p: IVec3, disk: &mut HashMap<ChunkPos, Option<Arc<Chunk>>>| -> bool {
        if p.y < 0 || p.y >= actual_height {
            return false;
        }
        if let Some(&s) = saved_edits.get(&p.to_array()) {
            return (s >> 4) == 90;
        }
        let cpos = (p.x.div_euclid(16), p.z.div_euclid(16));
        let lx = p.x.rem_euclid(16);
        let lz = p.z.rem_euclid(16);
        if let Some(chunks) = loaded_chunks {
            if let Some(c) = chunks.get(&cpos) {
                return (c.get(lx, p.y, lz) >> 4) == 90;
            }
        }
        if let Some(root) = world_root {
            let entry = disk.entry(cpos).or_insert_with(|| {
                World::load_dimension_chunk(root, to_dim, cpos)
                    .ok()
                    .flatten()
                    .map(Arc::new)
            });
            if let Some(c) = entry {
                return (c.get(lx, p.y, lz) >> 4) == 90;
            }
        }
        false
    };

    if let Some(cached) = cache.entries.get(&key).copied() {
        if is_portal_at(cached.pos, &mut disk_cache) {
            let mut lowest_y = cached.pos.y;
            while lowest_y > 0
                && is_portal_at(IVec3::new(cached.pos.x, lowest_y - 1, cached.pos.z), &mut disk_cache)
            {
                lowest_y -= 1;
            }
            let pos = IVec3::new(cached.pos.x, lowest_y, cached.pos.z);
            if let Some(entry) = cache.entries.get_mut(&key) {
                entry.pos = pos;
                entry.last_update_tick = total_ticks;
            }
            return Some(pos);
        } else {
            cache.entries.remove(&key);
        }
    }

    let min_x = tx - 128;
    let max_x = tx + 128;
    let min_z = tz - 128;
    let max_z = tz + 128;
    let min_cx = min_x.div_euclid(16);
    let max_cx = max_x.div_euclid(16);
    let min_cz = min_z.div_euclid(16);
    let max_cz = max_z.div_euclid(16);

    let mut candidates: Vec<IVec3> = Vec::new();

    if !saved_edits.is_empty() {
        for (&[bx, by, bz], &state) in
            saved_edits.range([min_x, 0, i32::MIN]..=[max_x, actual_height - 1, i32::MAX])
        {
            if (state >> 4) == 90
                && (0..actual_height).contains(&by)
                && (min_z..=max_z).contains(&bz)
            {
                candidates.push(IVec3::new(bx, by, bz));
            }
        }
    }

    for cz in min_cz..=max_cz {
        for cx in min_cx..=max_cx {
            if let Some(chunk) = loaded_chunks.and_then(|m| m.get(&(cx, cz))) {
                for &packed in chunk.portal_positions() {
                    let lx = (packed & 15) as i32;
                    let lz = ((packed >> 4) & 15) as i32;
                    let by = (packed >> 8) as i32;
                    let bx = cx * 16 + lx;
                    let bz = cz * 16 + lz;
                    if by < actual_height
                        && (min_x..=max_x).contains(&bx)
                        && (min_z..=max_z).contains(&bz)
                        && !saved_edits.contains_key(&[bx, by, bz])
                    {
                        candidates.push(IVec3::new(bx, by, bz));
                    }
                }
            } else if let Some(root) = world_root {
                let disk_chunk = disk_cache
                    .entry((cx, cz))
                    .or_insert_with(|| {
                        World::load_dimension_chunk(root, to_dim, (cx, cz))
                            .ok()
                            .flatten()
                            .map(Arc::new)
                    })
                    .clone();
                if let Some(chunk) = disk_chunk {
                    for &packed in chunk.portal_positions() {
                        let lx = (packed & 15) as i32;
                        let lz = ((packed >> 4) & 15) as i32;
                        let by = (packed >> 8) as i32;
                        let bx = cx * 16 + lx;
                        let bz = cz * 16 + lz;
                        if by < actual_height
                            && (min_x..=max_x).contains(&bx)
                            && (min_z..=max_z).contains(&bz)
                            && !saved_edits.contains_key(&[bx, by, bz])
                        {
                            candidates.push(IVec3::new(bx, by, bz));
                        }
                    }
                }
            }
        }
    }

    let mut best: Option<(f64, (i32, i32, std::cmp::Reverse<i32>), IVec3)> = None;
    let mut visited_columns: HashSet<IVec3> = HashSet::new();

    for cand in candidates {
        if !is_portal_at(cand, &mut disk_cache) {
            continue;
        }
        let mut lowest_y = cand.y;
        while lowest_y > 0
            && is_portal_at(IVec3::new(cand.x, lowest_y - 1, cand.z), &mut disk_cache)
        {
            lowest_y -= 1;
        }
        let bottom = IVec3::new(cand.x, lowest_y, cand.z);
        if !visited_columns.insert(bottom) {
            continue;
        }
        let dx = bottom.x as f64 - tx as f64;
        let dy = bottom.y as f64 - target_feet.y;
        let dz = bottom.z as f64 - tz as f64;
        let d2 = dx * dx + dy * dy + dz * dz;
        let tie = (bottom.x, bottom.z, std::cmp::Reverse(bottom.y));
        let better = match best {
            None => true,
            Some((best_d2, best_tie, _)) => d2 < best_d2 || (d2 == best_d2 && tie < best_tie),
        };
        if better {
            best = Some((d2, tie, bottom));
        }
    }

    if let Some((_, _, pos)) = best {
        cache.entries.insert(
            key,
            CachedPortalEntry {
                pos,
                last_update_tick: total_ticks,
            },
        );
        return Some(pos);
    }
    None
}

struct PortalPattern {
    /// Minimum horizontal coordinate (`min_x` if `axis_x`, else `min_z`).
    min_h: i32,
    /// Maximum horizontal coordinate (`max_x` if `axis_x`, else `max_z`).
    max_h: i32,
    /// Fixed plane coordinate (`pos.z` if `axis_x`, else `pos.x`).
    plane_coord: i32,
    min_y: i32,
    height: i32,
    width: i32,
    axis_x: bool,
    /// Chosen unobstructed normal direction (`BlockPattern.PatternHelper.getFinger()`).
    forwards: HorizFacing,
}

fn portal_pattern_helper(get_state: impl Fn(IVec3) -> u16, pos: IVec3) -> PortalPattern {
    let state_at_pos = get_state(pos);
    let axis_x = if (get_state(pos - IVec3::X) >> 4) == 90
        || (get_state(pos + IVec3::X) >> 4) == 90
    {
        true
    } else if (get_state(pos - IVec3::Z) >> 4) == 90 || (get_state(pos + IVec3::Z) >> 4) == 90 {
        false
    } else {
        (state_at_pos & 3) != 2
    };

    let h_step = if axis_x { IVec3::X } else { IVec3::Z };
    let mut min_y = pos.y;
    while min_y > 0 && (get_state(IVec3::new(pos.x, min_y - 1, pos.z)) >> 4) == 90 {
        min_y -= 1;
    }
    let base = IVec3::new(pos.x, min_y, pos.z);
    let mut neg = 0i32;
    while neg < 21 && (get_state(base - h_step * (neg + 1)) >> 4) == 90 {
        neg += 1;
    }
    let mut pos_ext = 0i32;
    while pos_ext < 21 && (get_state(base + h_step * (pos_ext + 1)) >> 4) == 90 {
        pos_ext += 1;
    }
    let width = (neg + pos_ext + 1).max(1);
    let min_corner = base - h_step * neg;

    let mut height = 1i32;
    'height_scan: while height < 21 && min_y + height < HEIGHT {
        for i in 0..width {
            if (get_state(min_corner + h_step * i + IVec3::Y * height) >> 4) != 90 {
                break 'height_scan;
            }
        }
        height += 1;
    }

    // 1.8.9 `BlockPortal.func_181089_f`:
    // For X-axis portal: `rightDir = West (-X)`, `ccw = South (+Z)`, `opposite = North (-Z)`.
    // For Z-axis portal: `rightDir = South (+Z)`, `ccw = East (+X)`, `opposite = West (-X)`.
    let primary = if axis_x {
        HorizFacing::South
    } else {
        HorizFacing::East
    };
    let secondary = primary.opposite();

    let count_open = |side: HorizFacing| -> i32 {
        let step = side.step();
        let rows = height.min(2);
        let mut open = 0;
        for i in 0..width {
            for dy in 0..rows {
                let check = min_corner + h_step * i + IVec3::Y * dy + step;
                let st = get_state(check);
                if st == 0 || (!material::blocks_movement(st) && !material::is_liquid(st)) {
                    open += 1;
                }
            }
        }
        open
    };

    let open_primary = count_open(primary);
    let open_secondary = count_open(secondary);
    let forwards = if open_secondary > open_primary {
        secondary
    } else {
        primary
    };

    let (min_h, max_h, plane_coord) = if axis_x {
        (min_corner.x, min_corner.x + width - 1, pos.z)
    } else {
        (min_corner.z, min_corner.z + width - 1, pos.x)
    };

    PortalPattern {
        min_h,
        max_h,
        plane_coord,
        min_y,
        height,
        width,
        axis_x,
        forwards,
    }
}

fn compute_portal_exit_state(
    get_state: impl Fn(IVec3) -> u16,
    portal_pos: IVec3,
    entry: &NetherPortalEntry,
) -> (DVec3, f32, DVec3) {
    let pat = portal_pattern_helper(&get_state, portal_pos);

    let (f, f1) = if entry.last_portal_vec == DVec3::ZERO {
        // Default centered standing position when caller supplies a zeroed portal vector.
        (0.5, 1.0)
    } else {
        (
            entry.last_portal_vec.x.clamp(0.0, 1.0),
            entry.last_portal_vec.y.clamp(0.0, 1.0),
        )
    };

    let thumb = pat.forwards.rotate_y();
    let flag_neg = thumb.axis_offset() < 0;
    let mut base_h = if flag_neg {
        (pat.max_h + 1) as f64
    } else {
        pat.min_h as f64
    };
    base_h += (1.0 - f) * (pat.width as f64) * (thumb.axis_offset() as f64);

    // Keep 0.6-wide player hitbox clear of left/right obsidian frame pillars.
    let lo_h = pat.min_h as f64 + 0.35;
    let hi_h = (pat.max_h + 1) as f64 - 0.35;
    let exit_h = if lo_h <= hi_h {
        base_h.clamp(lo_h, hi_h)
    } else {
        (pat.min_h as f64 + (pat.max_h + 1) as f64) * 0.5
    };

    // Exact 1.8.9 vertical offset `(top + 1) - f1 * height`, clamped so 1.8-tall player never clips top obsidian.
    let raw_y = (pat.min_y + pat.height) as f64 - f1 * (pat.height as f64);
    let max_safe_y = pat.min_y as f64 + (pat.height as f64 - 1.81).max(0.0);
    let exit_y = raw_y.clamp(pat.min_y as f64, max_safe_y);

    // Plane center `plane_coord + 0.5` keeps [coord+0.2, coord+0.8] clear of both neighbor planes;
    // nudge slightly toward `forwards` if the back face has solid blocks and front face is clear.
    let back_blocked = {
        let back_step = pat.forwards.opposite().step();
        let center_col = if pat.axis_x {
            IVec3::new(exit_h.floor() as i32, pat.min_y, pat.plane_coord)
        } else {
            IVec3::new(pat.plane_coord, pat.min_y, exit_h.floor() as i32)
        };
        material::blocks_movement(get_state(center_col + back_step))
            || material::blocks_movement(get_state(center_col + IVec3::Y + back_step))
    };
    let front_open = {
        let front_step = pat.forwards.step();
        let center_col = if pat.axis_x {
            IVec3::new(exit_h.floor() as i32, pat.min_y, pat.plane_coord)
        } else {
            IVec3::new(pat.plane_coord, pat.min_y, exit_h.floor() as i32)
        };
        !material::blocks_movement(get_state(center_col + front_step))
            && !material::blocks_movement(get_state(center_col + IVec3::Y + front_step))
    };
    let plane_pos = pat.plane_coord as f64
        + 0.5
        + if back_blocked && front_open {
            0.15 * (pat.forwards.axis_offset() as f64)
        } else {
            0.0
        };

    let dest_feet = if pat.axis_x {
        DVec3::new(exit_h, exit_y, plane_pos)
    } else {
        DVec3::new(plane_pos, exit_y, exit_h)
    };

    // Exact 1.8.9 velocity & yaw rotation between `entry.teleport_direction` and `pat.forwards`.
    let (mut f2, mut f3, mut f4, mut f5) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    let finger_opp = pat.forwards.opposite();
    if finger_opp == entry.teleport_direction {
        f2 = 1.0;
        f3 = 1.0;
    } else if finger_opp == entry.teleport_direction.opposite() {
        f2 = -1.0;
        f3 = -1.0;
    } else if finger_opp == entry.teleport_direction.rotate_y() {
        f4 = 1.0;
        f5 = -1.0;
    } else {
        f4 = -1.0;
        f5 = 1.0;
    }
    let exit_vel = DVec3::new(
        entry.velocity.x * f2 + entry.velocity.z * f5,
        entry.velocity.y,
        entry.velocity.x * f4 + entry.velocity.z * f3,
    );
    let delta_deg = (pat.forwards.horizontal_index()
        - entry.teleport_direction.opposite().horizontal_index()) as f32
        * 90.0;
    let exit_yaw = (entry.yaw + delta_deg).rem_euclid(360.0);

    (dest_feet, exit_yaw, exit_vel)
}

fn make_portal_chunks_loaded(world: &World, bx: i32, bz: i32) -> bool {
    let min_cx = (bx - 2).div_euclid(16);
    let max_cx = (bx + 2).div_euclid(16);
    let min_cz = (bz - 2).div_euclid(16);
    let max_cz = (bz + 2).div_euclid(16);
    (min_cx..=max_cx)
        .all(|cx| (min_cz..=max_cz).all(|cz| world.chunks.contains_key(&(cx, cz))))
}

fn check_make_portal_box(
    world: &World,
    bx: i32,
    by: i32,
    bz: i32,
    actual_height: i32,
    rot: i32,
    wide: bool,
) -> bool {
    let mut step_x = rot % 2;
    let mut step_z = 1 - step_x;
    if rot % 4 >= 2 {
        step_x = -step_x;
        step_z = -step_z;
    }
    let norm_range = if wide { -1..=1 } else { 0..=0 };
    for norm in norm_range {
        for along in -1..=2 {
            for dy in -1..=3 {
                let py = by + dy;
                if py < 0 || py >= actual_height {
                    return false;
                }
                let px = bx + along * step_x + norm * step_z;
                let pz = bz + along * step_z - norm * step_x;
                let st = world.get(IVec3::new(px, py, pz));
                if dy < 0 {
                    if !material::is_solid(st) {
                        return false;
                    }
                } else if st != 0 && (material::blocks_movement(st) || material::is_liquid(st)) {
                    return false;
                }
            }
        }
    }
    true
}

pub fn place_or_make_nether_portal(
    world: &mut crate::world::World,
    to_dim: i32,
    entry: &NetherPortalEntry,
    target_feet: glam::DVec3,
    tx: i32,
    tz: i32,
    known_portal: Option<glam::IVec3>,
    total_ticks: u64,
    cache: &mut TeleporterCache,
    rng: &mut crate::vanilla::random::JavaRandom,
) -> (glam::DVec3, f32, glam::DVec3, Vec<glam::IVec3>) {
    let actual_height = if to_dim == -1 { 128 } else { 256 };
    let key = chunk_xz_to_int(tx, tz);

    if let Some(p) = known_portal {
        if (0..actual_height).contains(&p.y) && (world.get(p) >> 4) == 90 {
            cache.entries.insert(
                key,
                CachedPortalEntry {
                    pos: p,
                    last_update_tick: total_ticks,
                },
            );
            let (dest, yaw, vel) = compute_portal_exit_state(|q| world.get(q), p, entry);
            return (dest, yaw, vel, Vec::new());
        }
    }

    // Check loaded `world.chunks` + `world.edits` within +/-128 if `known_portal` was None or stale.
    let saved_from_world: BTreeMap<[i32; 3], u16> = if world.edits.is_empty() {
        BTreeMap::new()
    } else {
        world.edits.iter().map(|(&k, &v)| (k, v)).collect()
    };
    if let Some(found) = find_existing_portal_inner(
        to_dim,
        target_feet,
        tx,
        tz,
        total_ticks,
        cache,
        &saved_from_world,
        Some(&world.chunks),
        None,
    ) {
        let (dest, yaw, vel) = compute_portal_exit_state(|q| world.get(q), found, entry);
        return (dest, yaw, vel, Vec::new());
    }

    // Exact 1.8.9 `Teleporter.makePortal`:
    let rng_base = rng.next_int(4);
    let mut best: Option<(f64, i32, i32, i32, i32)> = None;

    for wide in [true, false] {
        if best.is_some() {
            break;
        }
        let rot_count = if wide { 4 } else { 2 };
        for bx in (tx - 16)..=(tx + 16) {
            let dx = (bx as f64 + 0.5) - target_feet.x;
            for bz in (tz - 16)..=(tz + 16) {
                if !make_portal_chunks_loaded(world, bx, bz) {
                    continue;
                }
                let dz = (bz as f64 + 0.5) - target_feet.z;
                let mut by = actual_height - 1;
                while by >= 0 {
                    let st = world.get(IVec3::new(bx, by, bz));
                    if st == 0 || (!material::blocks_movement(st) && !material::is_liquid(st)) {
                        while by > 0 {
                            let below = world.get(IVec3::new(bx, by - 1, bz));
                            if below == 0
                                || (!material::blocks_movement(below)
                                    && !material::is_liquid(below))
                            {
                                by -= 1;
                            } else {
                                break;
                            }
                        }
                        for k2 in rng_base..(rng_base + rot_count) {
                            let rot = if wide { k2 % 4 } else { k2 % 2 };
                            if check_make_portal_box(world, bx, by, bz, actual_height, rot, wide) {
                                let dy = (by as f64 + 0.5) - target_feet.y;
                                let d2 = dx * dx + dy * dy + dz * dz;
                                if best.is_none_or(|(best_d2, ..)| d2 < best_d2) {
                                    best = Some((d2, bx, by, bz, rot));
                                }
                            }
                        }
                    }
                    by -= 1;
                }
            }
        }
    }

    let mut changed = Vec::new();
    let (bx, by, bz, rot, is_fallback) = if let Some((_, bx, by, bz, rot)) = best {
        (bx, by, bz, rot, false)
    } else {
        let clamped_y = (target_feet.y.floor() as i32).clamp(70, actual_height - 10);
        (tx, clamped_y, tz, rng_base % 2, true)
    };

    let mut step_x = rot % 2;
    let mut step_z = 1 - step_x;
    if rot % 4 >= 2 {
        step_x = -step_x;
        step_z = -step_z;
    }

    if is_fallback {
        // Ensure chunks exist around `(bx, bz)` even if caller provided a small world.
        for cz in (bz - 2).div_euclid(16)..=(bz + 2).div_euclid(16) {
            for cx in (bx - 2).div_euclid(16)..=(bx + 2).div_euclid(16) {
                world
                    .chunks
                    .entry((cx, cz))
                    .or_insert_with(|| Arc::new(Chunk::empty()));
            }
        }
        // 1.8.9 3x2 obsidian platform + 3x4x2 air clearance box.
        for norm in -1..=1 {
            for along in 0..=1 {
                for dy in -1..=2 {
                    let px = bx + along * step_x + norm * step_z;
                    let py = by + dy;
                    let pz = bz + along * step_z - norm * step_x;
                    let state = if dy < 0 { 49 << 4 } else { 0 };
                    set_tracked(world, &mut changed, IVec3::new(px, py, pz), state);
                }
            }
        }
    } else {
        // Clear replaceable plants/snow on immediate step-out cells so exit is tidy.
        for norm in [-1, 1] {
            for along in 0..=1 {
                for dy in 0..=2 {
                    let px = bx + along * step_x + norm * step_z;
                    let py = by + dy;
                    let pz = bz + along * step_z - norm * step_x;
                    let p = IVec3::new(px, py, pz);
                    let cur = world.get(p);
                    if cur != 0 && !material::blocks_movement(cur) && !material::is_liquid(cur) {
                        set_tracked(world, &mut changed, p, 0);
                    }
                }
            }
        }
    }

    let axis_meta: u16 = if step_x != 0 { 1 } else { 2 };
    for along in -1..=2 {
        for dy in -1..=3 {
            let px = bx + along * step_x;
            let py = by + dy;
            let pz = bz + along * step_z;
            let is_frame = along == -1 || along == 2 || dy == -1 || dy == 3;
            let state = if is_frame {
                49 << 4
            } else {
                (90 << 4) | axis_meta
            };
            set_tracked(world, &mut changed, IVec3::new(px, py, pz), state);
        }
    }

    changed.sort_unstable_by_key(|p| (p.x, p.y, p.z));
    changed.dedup();

    let created_pos = IVec3::new(bx, by, bz);
    cache.entries.insert(
        key,
        CachedPortalEntry {
            pos: created_pos,
            last_update_tick: total_ticks,
        },
    );
    let (dest, yaw, vel) = compute_portal_exit_state(|q| world.get(q), created_pos, entry);
    (dest, yaw, vel, changed)
}

// ============================================================================
// Legacy Compatibility Wrappers & End Platform / Dynamic Dragon Exit Podium
// ============================================================================

pub fn scaled_nether_portal_target(from_dim: i32, to_dim: i32, player_feet: DVec3) -> DVec3 {
    let scale = match (from_dim, to_dim) {
        (0, -1) => 1.0 / 8.0,
        (-1, 0) => 8.0,
        _ => 1.0,
    };
    DVec3::new(
        (player_feet.x * scale).clamp(-29_999_872.0, 29_999_872.0),
        player_feet.y.clamp(16.0, 118.0),
        (player_feet.z * scale).clamp(-29_999_872.0, 29_999_872.0),
    )
}

fn set_tracked(world: &mut World, changed: &mut Vec<IVec3>, pos: IVec3, state: u16) {
    if world.set(pos, state) {
        changed.push(pos);
    }
}

fn portal_area_loaded(world: &World, bx: i32, bz: i32) -> bool {
    let min_cx = (bx - 1).div_euclid(16);
    let max_cx = (bx + 2).div_euclid(16);
    let min_cz = (bz - 1).div_euclid(16);
    let max_cz = (bz + 1).div_euclid(16);
    (min_cx..=max_cx)
        .all(|cx| (min_cz..=max_cz).all(|cz| world.chunks.contains_key(&(cx, cz))))
}

fn portal_volume_clear(world: &World, bx: i32, by: i32, bz: i32, wide_z: bool) -> bool {
    let z_range = if wide_z { -1..=1 } else { 0..=0 };
    for dz in z_range {
        for dx in -1..=2 {
            let floor = world.get(IVec3::new(bx + dx, by - 1, bz + dz));
            if !material::is_solid(floor) {
                return false;
            }
            for dy in 0..=3 {
                let above = world.get(IVec3::new(bx + dx, by + dy, bz + dz));
                if material::is_solid(above) || material::is_liquid(above) {
                    return false;
                }
            }
        }
    }
    true
}

pub fn place_in_nether_portal(
    world: &mut World,
    from_dim: i32,
    to_dim: i32,
    player_feet: DVec3,
    yaw: f32,
) -> (DVec3, f32, Vec<IVec3>) {
    let target = scaled_nether_portal_target(from_dim, to_dim, player_feet);
    let tx = target.x.floor() as i32;
    let tz = target.z.floor() as i32;

    let min_cx = (tx - 128).div_euclid(16);
    let max_cx = (tx + 128).div_euclid(16);
    let min_cz = (tz - 128).div_euclid(16);
    let max_cz = (tz + 128).div_euclid(16);
    let mut best_existing: Option<(f64, i32, i32, i32)> = None;

    for cz in min_cz..=max_cz {
        for cx in min_cx..=max_cx {
            let Some(chunk) = world.chunks.get(&(cx, cz)) else {
                continue;
            };
            for &packed in chunk.portal_positions() {
                let lx = (packed & 15) as i32;
                let lz = ((packed >> 4) & 15) as i32;
                let y = (packed >> 8) as i32;
                let bx = cx * 16 + lx;
                let bz = cz * 16 + lz;
                if (bx - tx).abs() > 128 || (bz - tz).abs() > 128 {
                    continue;
                }
                if y > 0 && (chunk.get(lx, y - 1, lz) >> 4) == 90 {
                    continue;
                }
                let lowest_y = y;
                let center = DVec3::new(bx as f64 + 0.5, lowest_y as f64 + 0.5, bz as f64 + 0.5);
                let dist_sq = center.distance_squared(target);
                let better = match best_existing {
                    None => true,
                    Some((best_d, best_y, best_x, best_z)) => {
                        dist_sq < best_d - 1e-9
                            || ((dist_sq - best_d).abs() <= 1e-9
                                && (lowest_y, bx, bz) < (best_y, best_x, best_z))
                    }
                };
                if better {
                    best_existing = Some((dist_sq, lowest_y, bx, bz));
                }
            }
        }
    }

    if let Some((_, lowest_y, bx, bz)) = best_existing {
        return (
            IVec3::new(bx, lowest_y, bz).as_dvec3() + DVec3::new(0.5, 0.0, 0.5),
            yaw,
            Vec::new(),
        );
    }

    let max_y = if to_dim == -1 { 120 } else { 246 };
    let mut chosen: Option<(f64, i32, i32, i32)> = None;

    for wide_z in [true, false] {
        if chosen.is_some() {
            break;
        }
        for bx in (tx - 16)..=(tx + 16) {
            for bz in (tz - 16)..=(tz + 16) {
                if !portal_area_loaded(world, bx, bz) {
                    continue;
                }
                for by in (4..=(max_y - 4)).rev() {
                    if !portal_volume_clear(world, bx, by, bz, wide_z) {
                        continue;
                    }
                    let d = DVec3::new(bx as f64 + 0.5, by as f64 + 0.5, bz as f64 + 0.5)
                        .distance_squared(target);
                    if chosen.is_none_or(|(best_d, _, _, _)| d < best_d) {
                        chosen = Some((d, bx, by, bz));
                    }
                }
            }
        }
    }

    let mut changed = Vec::new();
    let (bx, portal_y, bz, need_platform) = if let Some((_, bx, by, bz)) = chosen {
        (bx, by, bz, false)
    } else {
        let min_clamp_y = if to_dim == -1 { 32 } else { 70 };
        let clamped_y = (target.y.floor() as i32).clamp(min_clamp_y, max_y - 10);
        (tx, clamped_y, tz, true)
    };

    if need_platform {
        for dz in -1..=1 {
            for dx in 0..=1 {
                set_tracked(
                    world,
                    &mut changed,
                    IVec3::new(bx + dx, portal_y - 1, bz + dz),
                    49 << 4,
                );
            }
            if dz != 0 {
                for dx in -1..=1 {
                    for dy in 0..=2 {
                        set_tracked(
                            world,
                            &mut changed,
                            IVec3::new(bx + dx, portal_y + dy, bz + dz),
                            0,
                        );
                    }
                }
            }
        }
    } else {
        for dz in [-1, 1] {
            for dx in 0..=1 {
                for dy in 0..=2 {
                    let p = IVec3::new(bx + dx, portal_y + dy, bz + dz);
                    if world.get(p) != 0 && !material::is_solid(world.get(p)) {
                        set_tracked(world, &mut changed, p, 0);
                    }
                }
            }
        }
    }

    for dy in -1..=3 {
        for dx in -1..=2 {
            let is_frame = dx == -1 || dx == 2 || dy == -1 || dy == 3;
            let state = if is_frame { 49 << 4 } else { (90 << 4) | 1 };
            set_tracked(
                world,
                &mut changed,
                IVec3::new(bx + dx, portal_y + dy, bz),
                state,
            );
        }
    }

    changed.sort_unstable_by_key(|p| (p.x, p.y, p.z));
    changed.dedup();
    let spawn = DVec3::new(bx as f64 + 0.5, portal_y as f64, bz as f64 + 0.5);
    (spawn, yaw, changed)
}

pub fn build_end_obsidian_platform(world: &mut World) -> (DVec3, f32, Vec<IVec3>) {
    let mut changed = Vec::new();
    for x in 98..=102 {
        for z in -2..=2 {
            set_tracked(world, &mut changed, IVec3::new(x, 48, z), 49 << 4);
            for y in 49..=51 {
                set_tracked(world, &mut changed, IVec3::new(x, y, z), 0);
            }
        }
    }
    changed.sort_unstable_by_key(|p| (p.x, p.y, p.z));
    changed.dedup();
    (DVec3::new(100.5, 49.0, 0.5), 90.0, changed)
}

/// Exact 1.8.9 `EntityDragon.generatePortal(BlockPos(posX, 64.0, posZ))`.
pub fn build_end_exit_podium_at(
    world: &mut World,
    dragon_pos: Vec3,
    place_dragon_egg: bool,
) -> Vec<IVec3> {
    let px = dragon_pos.x.floor() as i32;
    let py = 64;
    let pz = dragon_pos.z.floor() as i32;

    for cz in (pz - 4).div_euclid(16)..=(pz + 4).div_euclid(16) {
        for cx in (px - 4).div_euclid(16)..=(px + 4).div_euclid(16) {
            world
                .chunks
                .entry((cx, cz))
                .or_insert_with(|| Arc::new(Chunk::empty()));
        }
    }

    let mut changed = Vec::new();
    for dy in -1..=32 {
        for dx in -4..=4 {
            for dz in -4..=4 {
                let d2 = (dx * dx + dz * dz) as f64;
                if d2 <= 12.25 {
                    let p = IVec3::new(px + dx, py + dy, pz + dz);
                    let state = if dy < 0 {
                        if d2 <= 6.25 {
                            7 << 4
                        } else {
                            121 << 4
                        }
                    } else if dy > 0 {
                        0
                    } else if d2 > 6.25 {
                        7 << 4
                    } else {
                        119 << 4
                    };
                    set_tracked(world, &mut changed, p, state);
                }
            }
        }
    }

    for dy in 0..=3 {
        set_tracked(world, &mut changed, IVec3::new(px, py + dy, pz), 7 << 4);
    }

    set_tracked(
        world,
        &mut changed,
        IVec3::new(px - 1, py + 2, pz),
        (50 << 4) | 2,
    );
    set_tracked(
        world,
        &mut changed,
        IVec3::new(px + 1, py + 2, pz),
        (50 << 4) | 1,
    );
    set_tracked(
        world,
        &mut changed,
        IVec3::new(px, py + 2, pz - 1),
        (50 << 4) | 4,
    );
    set_tracked(
        world,
        &mut changed,
        IVec3::new(px, py + 2, pz + 1),
        (50 << 4) | 3,
    );

    if place_dragon_egg {
        set_tracked(world, &mut changed, IVec3::new(px, py + 4, pz), 122 << 4);
    }

    changed.sort_unstable_by_key(|p| (p.x, p.y, p.z));
    changed.dedup();
    changed
}

pub fn build_end_exit_podium(world: &mut World, place_dragon_egg: bool) -> Vec<IVec3> {
    build_end_exit_podium_at(world, Vec3::new(0.0, 64.0, 0.0), place_dragon_egg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn test_options(seed: u64) -> Options {
        let mut options = Options::parse_from(["ferrum"]);
        options.seed = seed;
        options.world_rules.structures = true;
        options
    }

    #[test]
    fn nether_chunk_generation_populates_blocks_and_no_sky_light() {
        let options = test_options(189);
        let positions = [(0, 0), (1, 0), (0, 1), (1, 1)];
        let chunks = read_dimension_chunks(-1, &options, &BTreeMap::new(), &positions).unwrap();
        assert_eq!(chunks.len(), 4);

        let mut has_netherrack = false;
        let mut has_lava = false;
        let mut has_bedrock_floor = false;
        let mut has_bedrock_ceiling = false;
        let mut has_glowstone = false;
        let mut has_quartz = false;

        for (_, chunk) in &chunks {
            let light = chunk
                .light
                .as_ref()
                .expect("Nether chunk must carry ChunkLight");
            assert!(light.no_sky, "Nether ChunkLight must set no_sky = true");
            for x in 0..16 {
                for z in 0..16 {
                    if chunk.get(x, 0, z) >> 4 == 7 {
                        has_bedrock_floor = true;
                    }
                    if chunk.get(x, 127, z) >> 4 == 7 {
                        has_bedrock_ceiling = true;
                    }
                    for y in 0..128 {
                        match chunk.get(x, y, z) >> 4 {
                            87 => has_netherrack = true,
                            10 | 11 => has_lava = true,
                            89 => has_glowstone = true,
                            153 => has_quartz = true,
                            _ => {}
                        }
                    }
                }
            }
        }

        assert!(has_netherrack, "Nether chunks must contain netherrack (87)");
        assert!(has_lava, "Nether chunks must contain lava (10/11)");
        assert!(
            has_bedrock_floor,
            "Nether chunks must contain bedrock floor at y=0"
        );
        assert!(
            has_bedrock_ceiling,
            "Nether chunks must contain bedrock ceiling at y=127"
        );
        assert!(
            has_glowstone || has_quartz,
            "Populated Nether chunks must contain glowstone (89) or quartz ore (153)"
        );
        assert!(
            has_quartz,
            "Populated Nether chunks must contain quartz ore (153)"
        );
    }

    #[test]
    fn end_origin_chunk_generates_end_stone_no_sky_and_ender_dragon_meta() {
        let options = test_options(189);
        let chunks = read_dimension_chunks(1, &options, &BTreeMap::new(), &[(0, 0)]).unwrap();
        assert_eq!(chunks.len(), 1);
        let (_, chunk) = &chunks[0];
        let light = chunk
            .light
            .as_ref()
            .expect("End chunk must carry ChunkLight");
        assert!(light.no_sky, "End ChunkLight must set no_sky = true");

        let mut has_end_stone = false;
        for y in 0..128 {
            for z in 0..16 {
                for x in 0..16 {
                    if chunk.get(x, y, z) >> 4 == 121 {
                        has_end_stone = true;
                    }
                }
            }
        }
        assert!(
            has_end_stone,
            "End origin chunk (0, 0) must contain End Stone (121)"
        );

        let meta = chunk_population_meta(1, &options, (0, 0));
        assert_eq!(meta.ender_dragon, Some([0.0, 128.0, 0.0]));
    }

    #[test]
    fn nether_portal_teleporter_scales_builds_under_roof_and_reuses_existing_portal() {
        let down = scaled_nether_portal_target(0, -1, DVec3::new(160.0, 80.0, -240.0));
        assert!((down.x - 20.0).abs() < 1e-6);
        assert!((down.z - (-30.0)).abs() < 1e-6);
        let up = scaled_nether_portal_target(-1, 0, down);
        assert!((up.x - 160.0).abs() < 1e-6);
        assert!((up.z - (-240.0)).abs() < 1e-6);

        let mut nether_world = empty_mini_world();
        for cz in -3..=3 {
            for cx in -3..=3 {
                let mut c = Chunk::empty();
                for x in 0..16 {
                    for z in 0..16 {
                        c.set(x, 0, z, 7 << 4);
                        for y in 1..127 {
                            c.set(x, y, z, 87 << 4);
                        }
                        c.set(x, 127, z, 7 << 4);
                    }
                }
                nether_world.chunks.insert((cx, cz), Arc::new(c));
            }
        }

        let (spawn, out_yaw, changed) =
            place_in_nether_portal(&mut nether_world, 0, -1, DVec3::new(80.0, 200.0, 80.0), 45.0);
        assert_eq!(out_yaw, 45.0);
        assert!(!changed.is_empty());
        assert!(
            spawn.y < 122.0,
            "Nether portal spawn must stay below bedrock roof"
        );
        assert!(changed.iter().all(|p| p.y < 122));

        let base = IVec3::new(
            spawn.x.floor() as i32,
            spawn.y.floor() as i32,
            spawn.z.floor() as i32,
        );
        assert_eq!(nether_world.get(base), (90 << 4) | 1);
        assert_eq!(nether_world.get(base + IVec3::X), (90 << 4) | 1);
        assert_eq!(nether_world.get(base - IVec3::Y) >> 4, 49);
        assert_eq!(nether_world.get(base + IVec3::new(0, 3, 0)) >> 4, 49);
        assert_eq!(nether_world.get(base - IVec3::X) >> 4, 49);
        assert_eq!(nether_world.get(base + IVec3::new(2, 0, 0)) >> 4, 49);

        let (reused_spawn, _, reused_changed) =
            place_in_nether_portal(&mut nether_world, 0, -1, DVec3::new(88.0, 72.0, 88.0), 90.0);
        assert!(reused_changed.is_empty());
        assert_eq!(reused_spawn.y, spawn.y);
    }

    #[test]
    fn resolve_target_truncates_toward_zero_and_preserves_unclamped_y() {
        let (target, tx, tz) = resolve_nether_portal_target(
            0,
            -1,
            DVec3::new(-15.0, 210.5, 23.0),
            -30_000_000.0,
            30_000_000.0,
            -30_000_000.0,
            30_000_000.0,
        );
        // -15.0 * 0.125 = -1.875 -> truncates toward zero to -1 (not -2!)
        // 23.0 * 0.125 = 2.875 -> truncates toward zero to 2
        assert_eq!((tx, tz), (-1, 2));
        assert_eq!(target, DVec3::new(-1.0, 210.5, 2.0));

        let (_, bx, bz) = resolve_nether_portal_target(
            -1,
            0,
            DVec3::new(100.0, 65.0, -100.0),
            -500.0,
            500.0,
            -500.0,
            500.0,
        );
        // 800 clamped to 500 - 16 = 484, -800 clamped to -500 + 16 = -484.
        assert_eq!((bx, bz), (484, -484));
    }

    #[test]
    fn find_existing_portal_fast_searches_128_radius_and_caches_and_prunes() {
        let options = test_options(189);
        let mut saved_edits = BTreeMap::new();
        // Place a 2x3 vertical portal column at (110, 70..73, -95) within +/-128 of (0, 0).
        for dy in 0..3 {
            saved_edits.insert([110, 70 + dy, -95], (90 << 4) | 1);
            saved_edits.insert([111, 70 + dy, -95], (90 << 4) | 1);
        }
        let mut cache = TeleporterCache::default();
        let found = find_existing_portal_fast(
            -1,
            DVec3::new(0.0, 72.0, 0.0),
            0,
            0,
            100,
            &mut cache,
            &saved_edits,
            None,
            &options,
        );
        assert_eq!(found, Some(IVec3::new(110, 70, -95)));
        assert_eq!(cache.entries.len(), 1);

        // Breaking the cached portal in `saved_edits` invalidates the cached entry immediately.
        for dy in 0..3 {
            saved_edits.insert([110, 70 + dy, -95], 0);
        }
        let found_after_break = find_existing_portal_fast(
            -1,
            DVec3::new(0.0, 72.0, 0.0),
            0,
            0,
            200,
            &mut cache,
            &saved_edits,
            None,
            &options,
        );
        assert_eq!(found_after_break, Some(IVec3::new(111, 70, -95)));

        // Advance > 600 ticks and search a different key -> stale cache entries are pruned.
        let _ = find_existing_portal_fast(
            -1,
            DVec3::new(500.0, 72.0, 500.0),
            500,
            500,
            900,
            &mut cache,
            &saved_edits,
            None,
            &options,
        );
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn place_or_make_nether_portal_exits_away_from_wall_and_rotates_yaw_and_velocity() {
        let mut world = empty_mini_world();
        for cz in -2..=2 {
            for cx in -2..=2 {
                world.chunks.insert((cx, cz), Arc::new(Chunk::empty()));
            }
        }
        // Build an X-aligned portal at z = 0, x = 0..=1, y = 65..=67 with a solid wall on South (+Z = 1)
        // so the unobstructed side MUST be North (-Z, yaw = 180).
        for dy in -1..=3 {
            for dx in -1..=2 {
                let is_frame = dx == -1 || dx == 2 || dy == -1 || dy == 3;
                world.set(
                    IVec3::new(dx, 65 + dy, 0),
                    if is_frame { 49 << 4 } else { (90 << 4) | 1 },
                );
            }
        }
        for dx in 0..=1 {
            for dy in 0..=1 {
                world.set(IVec3::new(dx, 65 + dy, 1), 1 << 4);
            }
        }
        let entry = NetherPortalEntry {
            trigger_pos: DVec3::new(0.5, 65.0, 0.5),
            yaw: 180.0, // walked North into an entry portal whose open face was South
            pitch: 0.0,
            velocity: DVec3::new(0.0, 0.0, -0.25),
            last_portal_pos: IVec3::new(0, 65, 0),
            last_portal_vec: DVec3::new(0.5, 1.0, 0.0),
            teleport_direction: HorizFacing::South,
        };
        let mut cache = TeleporterCache::default();
        let mut rng = JavaRandom::new(189);
        let (dest, exit_yaw, exit_vel, changed) = place_or_make_nether_portal(
            &mut world,
            0,
            &entry,
            DVec3::new(0.0, 65.0, 0.0),
            0,
            0,
            Some(IVec3::new(0, 65, 0)),
            50,
            &mut cache,
            &mut rng,
        );
        assert!(changed.is_empty());
        assert!((dest.y - 65.0).abs() < 1e-6);
        // Open side is North (-Z): yaw stays 180 (facing North) and velocity stays -Z!
        assert!((exit_yaw - 180.0).abs() < 1e-4);
        assert!(exit_vel.z < -0.2);
    }

    #[test]
    fn end_obsidian_platform_and_exit_podium_place_exact_blocks() {
        let mut world = empty_mini_world();
        for cz in -4..=7 {
            for cx in -4..=7 {
                let mut chunk = Chunk::empty();
                if (-1..=1).contains(&cx) && (-1..=1).contains(&cz) {
                    for x in 0..16 {
                        for z in 0..16 {
                            for y in 50..=70 {
                                chunk.set(x, y, z, 121 << 4);
                            }
                        }
                    }
                }
                world.chunks.insert((cx, cz), Arc::new(chunk));
            }
        }

        let (plat_spawn, plat_yaw, plat_changed) = build_end_obsidian_platform(&mut world);
        assert_eq!(plat_spawn, DVec3::new(100.5, 49.0, 0.5));
        assert_eq!(plat_yaw, 90.0);
        assert!(!plat_changed.is_empty());
        for x in 98..=102 {
            for z in -2..=2 {
                assert_eq!(world.get(IVec3::new(x, 48, z)), 49 << 4);
                for y in 49..=51 {
                    assert_eq!(world.get(IVec3::new(x, y, z)), 0);
                }
            }
        }

        let podium_changed = build_end_exit_podium(&mut world, true);
        assert!(!podium_changed.is_empty());
        let base_y = 64;
        assert_eq!(world.get(IVec3::new(0, base_y, 0)), 7 << 4);
        assert_eq!(world.get(IVec3::new(0, base_y + 3, 0)), 7 << 4);
        assert_eq!(world.get(IVec3::new(1, base_y, 0)), 119 << 4);
        assert_eq!(world.get(IVec3::new(-1, base_y, 0)), 119 << 4);
        assert_eq!(world.get(IVec3::new(0, base_y, 1)), 119 << 4);
        assert_eq!(world.get(IVec3::new(0, base_y, -1)), 119 << 4);
        assert_eq!(world.get(IVec3::new(3, base_y, 0)), 7 << 4);
        assert_eq!(world.get(IVec3::new(3, base_y - 1, 0)), 121 << 4);
        assert_eq!(world.get(IVec3::new(1, base_y + 2, 0)), (50 << 4) | 1);
        assert_eq!(world.get(IVec3::new(-1, base_y + 2, 0)), (50 << 4) | 2);
        assert_eq!(world.get(IVec3::new(0, base_y + 2, 1)), (50 << 4) | 3);
        assert_eq!(world.get(IVec3::new(0, base_y + 2, -1)), (50 << 4) | 4);
        assert_eq!(world.get(IVec3::new(0, base_y + 4, 0)), 122 << 4);

        // Dynamic non-zero dragon death coordinates `(42.7, 85.0, -31.2)` -> `(42, 64, -32)`.
        let dyn_changed =
            build_end_exit_podium_at(&mut world, Vec3::new(42.7, 85.0, -31.2), true);
        assert!(!dyn_changed.is_empty());
        let (px, py, pz) = (42, 64, -32);
        assert_eq!(world.get(IVec3::new(px, py, pz)), 7 << 4);
        assert_eq!(world.get(IVec3::new(px, py + 3, pz)), 7 << 4);
        assert_eq!(world.get(IVec3::new(px + 1, py, pz)), 119 << 4);
        assert_eq!(world.get(IVec3::new(px + 3, py, pz)), 7 << 4);
        assert_eq!(world.get(IVec3::new(px + 3, py - 1, pz)), 121 << 4);
        assert_eq!(world.get(IVec3::new(px - 1, py + 2, pz)), (50 << 4) | 2);
        assert_eq!(world.get(IVec3::new(px + 1, py + 2, pz)), (50 << 4) | 1);
        assert_eq!(world.get(IVec3::new(px, py + 2, pz - 1)), (50 << 4) | 4);
        assert_eq!(world.get(IVec3::new(px, py + 2, pz + 1)), (50 << 4) | 3);
        assert_eq!(world.get(IVec3::new(px, py + 4, pz)), 122 << 4);
    }

    #[test]
    fn chunk_spawner_end_portal_and_nether_portal_indexes_track_live_edits() {
        let mut chunk = Chunk::empty();
        chunk.set(3, 40, 5, 52 << 4);
        chunk.set(7, 62, 9, 119 << 4);
        chunk.set(2, 70, 4, (90 << 4) | 1);
        chunk.prepare_tick_indexes();

        let p_spawner = ((40u16) << 8) | ((5u16) << 4) | 3u16;
        let p_end_portal = ((62u16) << 8) | ((9u16) << 4) | 7u16;
        let p_nether_portal = ((70u16) << 8) | ((4u16) << 4) | 2u16;
        assert_eq!(chunk.spawner_positions(), &[p_spawner]);
        assert_eq!(chunk.end_portal_positions(), &[p_end_portal]);
        assert_eq!(chunk.portal_positions(), &[p_nether_portal]);

        let p_spawner2 = ((41u16) << 8) | ((2u16) << 4) | 1u16;
        let p_end_portal2 = ((62u16) << 8) | ((10u16) << 4) | 8u16;
        let p_nether_portal2 = ((71u16) << 8) | ((4u16) << 4) | 2u16;
        chunk.set(1, 41, 2, 52 << 4);
        chunk.set(8, 62, 10, 119 << 4);
        chunk.set(2, 71, 4, (90 << 4) | 2);
        assert_eq!(chunk.spawner_positions(), &[p_spawner, p_spawner2]);
        assert_eq!(chunk.end_portal_positions(), &[p_end_portal, p_end_portal2]);
        assert_eq!(
            chunk.portal_positions(),
            &[p_nether_portal, p_nether_portal2]
        );

        chunk.set(3, 40, 5, 0);
        chunk.set(7, 62, 9, 0);
        chunk.set(2, 70, 4, 0);
        assert_eq!(chunk.spawner_positions(), &[p_spawner2]);
        assert_eq!(chunk.end_portal_positions(), &[p_end_portal2]);
        assert_eq!(chunk.portal_positions(), &[p_nether_portal2]);

        clear_caches();
    }
}
