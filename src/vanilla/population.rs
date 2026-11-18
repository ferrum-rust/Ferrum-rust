//! Native population primitives, compared to original 1.8.9 feature bytecode.
//! The caller owns structure order, lighting and population-once persistence.
use super::{
    biome::biome,
    loot::{self, LootEntry, LootStack},
    material, math,
    noise::Perlin,
    random::JavaRandom,
};
use crate::world::World;
use anyhow::{ensure, Result};
use glam::IVec3;
use serde::{Deserialize, Serialize};
include!("population_blocks.rs");
#[path = "decoration.rs"]
pub mod decoration;
#[path = "vegetation.rs"]
pub mod vegetation;

/// WorldGen features read loaded neighbors, never implicit air in unloaded chunks.
/// Lighting is an input because original generation consults actual world light.
pub trait FeatureWorld {
    fn get(&self, p: IVec3) -> u16;
    fn set(&mut self, p: IVec3, state: u16);
    /// The native adapter times this stage as one unit. The default path keeps
    /// the original function, callbacks and shared Random sequence unchanged.
    fn generate_default_ores(&mut self, random: &mut JavaRandom, origin: IVec3)
    where Self: Sized {
        default_ores(self, random, origin);
    }
    /// Replace an ordered ore candidate batch. Each read must observe
    /// preceding writes and their callbacks; positions contain no cached states.
    fn replace_ore_line(&mut self, positions: &[IVec3], ore: u16, replace_id: u16) {
        for &p in positions {
            if self.get(p) >> 4 == replace_id {
                self.set(p, ore);
            }
        }
    }
    fn biome(&self, p: IVec3) -> u8;
    fn light(&self, p: IVec3, sky: bool) -> u8;
    /// World.getLightFromNeighbors, distinct from raw getLightFor used by lakes.
    fn combined_light(&self, p: IVec3) -> u8 {
        self.light(p, true).max(self.light(p, false))
    }
    /// Original BiomeGenBase shares one mutable double-plant generator globally.
    /// Runtime adapters share this state across nested decoration callbacks.
    fn set_plains_counts(&mut self, _id: u8, _flowers: i32, _grass: i32) {}
    fn plains_counts(&self, _id: u8, fallback: (i32, i32)) -> (i32, i32) {
        fallback
    }
    fn set_double_plant_type(&mut self, _meta: u16) {}
    fn double_plant_type(&self, fallback: u16) -> u16 {
        fallback
    }
    fn chest(&mut self, p: IVec3, slots: Vec<Option<LootStack>>);
    fn spawner(&mut self, p: IVec3, entity: &'static str);
    /// Spawn request only; a simulation must construct and tick the entity.
    fn entity(&mut self, entity: GeneratedEntity);
    fn height(&self, x: i32, z: i32) -> i32 {
        for y in (0..256).rev() {
            if LIGHT_OPACITY
                .get((self.get(IVec3::new(x, y, z)) >> 4) as usize)
                .copied()
                .unwrap_or(0)
                > 0
            {
                return y + 1;
            }
        }
        0
    }
    fn top_solid(&self, x: i32, z: i32) -> i32 {
        for y in (0..256).rev() {
            let state = self.get(IVec3::new(x, y, z));
            if material::blocks_movement(state) && !material::is_leaves(state) {
                return y + 1;
            }
        }
        -1
    }
    /// Chunk.getPrecipitationHeight ignores y=0 and uses movement OR liquid,
    /// including leaves. Runtime can override with the original cached lookup.
    fn precipitation_height(&self, x: i32, z: i32) -> i32 {
        for y in (1..256).rev() {
            let state = self.get(IVec3::new(x, y, z));
            if material::blocks_movement(state) || material::is_liquid(state) {
                return y + 1;
            }
        }
        -1
    }
    fn can_freeze(&self, p: IVec3) -> bool {
        // Preserve World callback evaluation order for reentrant chunk loading.
        if temperature(self.biome(p), p) > 0.15
            || !(0..256).contains(&p.y)
            || self.light(p, false) >= 10
        {
            return false;
        }
        let state = self.get(p);
        matches!(state >> 4, 8 | 9) && state & 15 == 0
    }
    fn can_snow(&self, p: IVec3) -> bool {
        if temperature(self.biome(p), p) > 0.15
            || !(0..256).contains(&p.y)
            || self.light(p, false) >= 10
            || self.get(p) >> 4 != 0
        {
            return false;
        }
        let below = self.get(p - IVec3::Y);
        snow_support(below)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum GeneratedEntityKind {
    EnderDragon,
    EnderCrystal,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GeneratedEntity {
    pub kind: GeneratedEntityKind,
    pub position: [f64; 3],
    pub yaw: f32,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Population {
    pub changed: Vec<IVec3>,
    pub chests: Vec<(IVec3, Vec<Option<LootStack>>)>,
    pub spawners: Vec<(IVec3, String)>,
    pub entities: Vec<GeneratedEntity>,
}

/// Adapter with explicit required neighborhood and caller-provided real lighting.
pub struct WorldRegion<'a, L: Fn(&World, IVec3, bool) -> u8> {
    world: &'a mut World,
    light: L,
    min: IVec3,
    max: IVec3,
    pub result: Population,
}
impl<'a, L: Fn(&World, IVec3, bool) -> u8> WorldRegion<'a, L> {
    pub fn new(world: &'a mut World, min: IVec3, max: IVec3, light: L) -> Result<Self> {
        for x in min.x.div_euclid(16)..=max.x.div_euclid(16) {
            for z in min.z.div_euclid(16)..=max.z.div_euclid(16) {
                ensure!(
                    world.chunks.contains_key(&(x, z)),
                    "Population requires loaded neighbor chunk {x},{z}"
                );
            }
        }
        Ok(Self {
            world,
            light,
            min,
            max,
            result: Population::default(),
        })
    }
    fn check(&self, p: IVec3) {
        assert!(
            p.x >= self.min.x && p.x <= self.max.x && p.z >= self.min.z && p.z <= self.max.z,
            "Population access outside preflighted region: {p:?}"
        );
    }
}
impl<L: Fn(&World, IVec3, bool) -> u8> FeatureWorld for WorldRegion<'_, L> {
    fn get(&self, p: IVec3) -> u16 {
        self.check(p);
        self.world.get(p)
    }
    fn set(&mut self, p: IVec3, state: u16) {
        self.check(p);
        if self.world.get(p) != state && self.world.set(p, state) {
            self.result.changed.push(p);
        }
    }
    fn biome(&self, p: IVec3) -> u8 {
        self.check(p);
        self.world.chunks[&(p.x.div_euclid(16), p.z.div_euclid(16))].biomes
            [(p.z.rem_euclid(16) * 16 + p.x.rem_euclid(16)) as usize]
    }
    fn light(&self, p: IVec3, sky: bool) -> u8 {
        (self.light)(self.world, p, sky)
    }
    fn chest(&mut self, p: IVec3, slots: Vec<Option<LootStack>>) {
        self.result.chests.push((p, slots));
    }
    fn spawner(&mut self, p: IVec3, entity: &'static str) {
        self.result.spawners.push((p, entity.into()));
    }
    fn entity(&mut self, entity: GeneratedEntity) {
        self.result.entities.push(entity);
    }
}

pub fn population_random(seed: i64, chunk_x: i32, chunk_z: i32) -> JavaRandom {
    let mut random = JavaRandom::new(seed);
    let x = (random.next_long() / 2).wrapping_mul(2).wrapping_add(1);
    let z = (random.next_long() / 2).wrapping_mul(2).wrapping_add(1);
    random.set_seed(
        (i64::from(chunk_x)
            .wrapping_mul(x)
            .wrapping_add(i64::from(chunk_z).wrapping_mul(z)))
            ^ seed,
    );
    random
}

/// DEFAULT provider stage after mineshaft/village/stronghold/temple/monument
/// generation, before biome decoration. Uses the same Random as those stages.
pub fn lakes_and_dungeons(
    world: &mut impl FeatureWorld,
    random: &mut JavaRandom,
    chunk_x: i32,
    chunk_z: i32,
    village_present: bool,
) {
    let origin = IVec3::new(chunk_x.wrapping_mul(16), 0, chunk_z.wrapping_mul(16));
    let id = world.biome(origin + IVec3::new(16, 0, 16));
    if id != 2 && id != 17 && !village_present && random.next_int(4) == 0 {
        let p = origin
            + IVec3::new(
                random.next_int(16) + 8,
                random.next_int(256),
                random.next_int(16) + 8,
            );
        lake(world, random, p, 9 << 4);
    }
    if !village_present && random.next_int(8) == 0 {
        let x = random.next_int(16) + 8;
        let bound = random.next_int(248) + 8;
        let y = random.next_int(bound);
        let z = random.next_int(16) + 8;
        if y < 63 || random.next_int(10) == 0 {
            lake(world, random, origin + IVec3::new(x, y, z), 11 << 4);
        }
    }
    for _ in 0..8 {
        let p = origin
            + IVec3::new(
                random.next_int(16) + 8,
                random.next_int(256),
                random.next_int(16) + 8,
            );
        dungeon(world, random, p);
    }
}

fn snow_support(below: u16) -> bool {
    !matches!(below >> 4, 79 | 174)
        && (material::is_leaves(below)
            || below >> 4 == 78 && below & 7 >= 6
            || material::opaque_cube(below) && material::blocks_movement(below))
}

/// Original ChunkProviderGenerate.populate tail, after animal spawning. Origin
/// is the unshifted chunk block origin; x-major order and freeze-before-snow
/// matter when block writes update light or trigger neighbor callbacks.
pub fn ice_and_snow(world: &mut impl FeatureWorld, origin: IVec3) {
    for x in 0..16 {
        for z in 0..16 {
            let x = origin.x.wrapping_add(8).wrapping_add(x);
            let z = origin.z.wrapping_add(8).wrapping_add(z);
            let p = IVec3::new(x, world.precipitation_height(x, z), z);
            if world.can_freeze(p - IVec3::Y) {
                world.set(p - IVec3::Y, 79 << 4);
            }
            if world.can_snow(p) {
                world.set(p, 78 << 4);
            }
        }
    }
}

pub(super) fn temperature(id: u8, p: IVec3) -> f32 {
    let base = biome(id).temperature;
    if p.y <= 64 {
        return base;
    }
    static NOISE: std::sync::OnceLock<Perlin> = std::sync::OnceLock::new();
    let noise = NOISE.get_or_init(|| Perlin::new(&mut JavaRandom::new(1234), 1));
    let variation = (noise.sample(p.x as f64 / 8., p.z as f64 / 8.) * 4.) as f32;
    base - (variation + p.y as f32 - 64.) * 0.05 / 30.
}

pub fn minable(
    world: &mut impl FeatureWorld,
    random: &mut JavaRandom,
    p: IVec3,
    ore: u16,
    size: i32,
) -> bool {
    minable_in(world, random, p, ore, size, 1)
}
#[path = "ore_bounds.rs"]
mod ore_bounds;
#[path = "ore_lookahead.rs"]
pub(crate) mod ore_lookahead;

/// Original WorldGenMinable's block predicate, including Nether quartz in netherrack.
pub fn minable_in(
    world: &mut impl FeatureWorld,
    random: &mut JavaRandom,
    p: IVec3,
    ore: u16,
    size: i32,
    replace_id: u16,
) -> bool {
    let angle = random.next_float() * std::f32::consts::PI;
    let a = [
        (p.x.wrapping_add(8) as f32 + math::sin(angle) * size as f32 / 8.) as f64,
        (p.y + random.next_int(3) - 2) as f64,
        (p.z.wrapping_add(8) as f32 + math::cos(angle) * size as f32 / 8.) as f64,
    ];
    let b = [
        (p.x.wrapping_add(8) as f32 - math::sin(angle) * size as f32 / 8.) as f64,
        (p.y + random.next_int(3) - 2) as f64,
        (p.z.wrapping_add(8) as f32 - math::cos(angle) * size as f32 / 8.) as f64,
    ];
    // Only the initialized prefixes are consumed below. Reuse this scratch
    // across spheres instead of clearing 1 KiB for every radius draw; no block
    // state or callback result survives in it between ordered writes.
    let mut y_squared = [0.; 16];
    let mut z_squared = [0.; 16];
    let mut positions = [IVec3::ZERO; 64];
    let bounds = ore_bounds::Bounds::new();
    for step in 0..size {
        let fraction = step as f32 / size as f32;
        let center: [f64; 3] = std::array::from_fn(|i| a[i] + (b[i] - a[i]) * fraction as f64);
        let radius_random = random.next_double() * size as f64 / 16.;
        let radius =
            ((math::sin(std::f32::consts::PI * fraction) + 1.) as f64 * radius_random + 1.) / 2.;
        let (low, high) = bounds.get(center, radius);
        // All vanilla ore sizes fit these small axis caches. Reuse only pure
        // coordinate arithmetic: get/set order and reentrant RNG draws stay
        // exactly inside the original X/Y/Z traversal. Keep each division and
        // square in its original order rather than comparing squared radii.
        let y_len = i64::from(high[1]) - i64::from(low[1]) + 1;
        let z_len = i64::from(high[2]) - i64::from(low[2]) + 1;
        if (1..=16).contains(&y_len) && (1..=16).contains(&z_len) {
            let ys = &mut y_squared[..y_len as usize];
            let zs = &mut z_squared[..z_len as usize];
            for (offset, value) in ys.iter_mut().enumerate() {
                let y = low[1] + offset as i32;
                let dy = (y as f64 + 0.5 - center[1]) / radius;
                *value = dy * dy;
            }
            for (offset, value) in zs.iter_mut().enumerate() {
                let z = low[2] + offset as i32;
                let dz = (z as f64 + 0.5 - center[2]) / radius;
                *value = dz * dz;
            }
            let mut count = 0;
            for x in low[0]..=high[0] {
                let dx = (x as f64 + 0.5 - center[0]) / radius;
                if dx * dx >= 1. { continue; }
                for (offset_y, &dy_squared) in ys.iter().enumerate() {
                    let y = low[1] + offset_y as i32;
                    let xy_squared = dx * dx + dy_squared;
                    if xy_squared >= 1. { continue; }
                    for (offset_z, &dz_squared) in zs.iter().enumerate() {
                        if xy_squared + dz_squared < 1. {
                            positions[count] = IVec3::new(x, y, low[2] + offset_z as i32);
                            count += 1;
                            if count == positions.len() {
                                world.replace_ore_line(&positions, ore, replace_id);
                                count = 0;
                            }
                        }
                    }
                }
            }
            // Only pure coordinates are buffered, in the original X/Y/Z
            // order. A callback may reseed the shared Random, so flush this
            // sphere completely before drawing the next sphere's radius.
            if count != 0 {
                world.replace_ore_line(&positions[..count], ore, replace_id);
            }
            continue;
        }
        // Larger non-vanilla feature sizes use the unchanged bounded traversal
        // instead of allocating scratch proportional to the requested size.
        for x in low[0]..=high[0] {
            let dx = (x as f64 + 0.5 - center[0]) / radius;
            if dx * dx >= 1. {
                continue;
            }
            for y in low[1]..=high[1] {
                let dy = (y as f64 + 0.5 - center[1]) / radius;
                if dx * dx + dy * dy >= 1. {
                    continue;
                }
                for z in low[2]..=high[2] {
                    let dz = (z as f64 + 0.5 - center[2]) / radius;
                    let p = IVec3::new(x, y, z);
                    if dx * dx + dy * dy + dz * dz < 1. && world.get(p) >> 4 == replace_id {
                        world.set(p, ore);
                    }
                }
            }
        }
    }
    true
}

pub fn default_ores(world: &mut impl FeatureWorld, random: &mut JavaRandom, origin: IVec3) {
    for (state, size, count, max) in [
        (3 << 4, 33, 10, 256),
        (13 << 4, 33, 8, 256),
        ((1 << 4) | 3, 33, 10, 80),
        ((1 << 4) | 1, 33, 10, 80),
        ((1 << 4) | 5, 33, 10, 80),
        (16 << 4, 17, 20, 128),
        (15 << 4, 9, 20, 64),
        (14 << 4, 9, 2, 32),
        (73 << 4, 8, 8, 16),
        (56 << 4, 8, 1, 16),
    ] {
        for _ in 0..count {
            let p = origin
                + IVec3::new(
                    random.next_int(16),
                    random.next_int(max),
                    random.next_int(16),
                );
            minable(world, random, p, state, size);
        }
    }
    let p = origin
        + IVec3::new(
            random.next_int(16),
            random.next_int(16) + random.next_int(16),
            random.next_int(16),
        );
    minable(world, random, p, 21 << 4, 7);
}

fn mask_index(x: usize, y: usize, z: usize) -> usize {
    (x * 16 + z) * 8 + y
}
fn boundary(mask: &[bool; 2048], x: usize, y: usize, z: usize) -> bool {
    !mask[mask_index(x, y, z)]
        && (x < 15 && mask[mask_index(x + 1, y, z)]
            || x > 0 && mask[mask_index(x - 1, y, z)]
            || z < 15 && mask[mask_index(x, y, z + 1)]
            || z > 0 && mask[mask_index(x, y, z - 1)]
            || y < 7 && mask[mask_index(x, y + 1, z)]
            || y > 0 && mask[mask_index(x, y - 1, z)])
}
pub fn lake(world: &mut impl FeatureWorld, random: &mut JavaRandom, p: IVec3, liquid: u16) -> bool {
    let mut origin = p - IVec3::new(8, 0, 8);
    while origin.y > 5 && world.get(origin) == 0 {
        origin.y -= 1;
    }
    if origin.y <= 4 {
        return false;
    }
    origin.y -= 4;
    let mut mask = [false; 2048];
    for _ in 0..random.next_int(4) + 4 {
        let sx = random.next_double() * 6. + 3.;
        let sy = random.next_double() * 4. + 2.;
        let sz = random.next_double() * 6. + 3.;
        let cx = random.next_double() * (16. - sx - 2.) + 1. + sx / 2.;
        let cy = random.next_double() * (8. - sy - 4.) + 2. + sy / 2.;
        let cz = random.next_double() * (16. - sz - 2.) + 1. + sz / 2.;
        for x in 1..15 {
            for z in 1..15 {
                for y in 1..7 {
                    let dx = (x as f64 - cx) / (sx / 2.);
                    let dy = (y as f64 - cy) / (sy / 2.);
                    let dz = (z as f64 - cz) / (sz / 2.);
                    if dx * dx + dy * dy + dz * dz < 1. {
                        mask[mask_index(x, y, z)] = true;
                    }
                }
            }
        }
    }
    for x in 0..16 {
        for z in 0..16 {
            for y in 0..8 {
                if !boundary(&mask, x, y, z) {
                    continue;
                }
                let state = world.get(origin + IVec3::new(x as i32, y as i32, z as i32));
                if y >= 4 && liquid_material(state)
                    || y < 4 && !solid(state) && state >> 4 != liquid >> 4
                {
                    return false;
                }
            }
        }
    }
    for x in 0..16 {
        for z in 0..16 {
            for y in 0..8 {
                if mask[mask_index(x, y, z)] {
                    world.set(
                        origin + IVec3::new(x as i32, y as i32, z as i32),
                        if y >= 4 { 0 } else { liquid },
                    );
                }
            }
        }
    }
    for x in 0..16 {
        for z in 0..16 {
            for y in 4..8 {
                let p = origin + IVec3::new(x as i32, y as i32, z as i32);
                if mask[mask_index(x, y, z)]
                    && world.get(p - IVec3::Y) >> 4 == 3
                    && world.light(p, true) > 0
                {
                    let top = if biome(world.biome(p - IVec3::Y)).top >> 4 == 110 {
                        110
                    } else {
                        2
                    };
                    world.set(p - IVec3::Y, top << 4);
                }
            }
        }
    }
    if matches!(liquid >> 4, 10 | 11) {
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..8 {
                    let p = origin + IVec3::new(x as i32, y as i32, z as i32);
                    if boundary(&mask, x, y, z)
                        && (y < 4 || random.next_int(2) != 0)
                        && solid(world.get(p))
                    {
                        world.set(p, 1 << 4);
                    }
                }
            }
        }
    }
    if matches!(liquid >> 4, 8 | 9) {
        for x in 0..16 {
            for z in 0..16 {
                let p = origin + IVec3::new(x, 4, z);
                if world.can_freeze(p) {
                    world.set(p, 79 << 4);
                }
            }
        }
    }
    true
}

fn solid(state: u16) -> bool {
    material::is_solid(state)
}
fn liquid_material(state: u16) -> bool {
    matches!(state >> 4, 8..=11)
}

const HORIZONTAL: [IVec3; 4] = [IVec3::NEG_Z, IVec3::X, IVec3::Z, IVec3::NEG_X];
fn full(state: u16) -> bool {
    // Original registry property (including transparent full cubes) is generated
    // alongside the feature oracle, rather than inferred from render geometry.
    material::full_block(state)
}
fn chest_facing(world: &impl FeatureWorld, p: IVec3) -> u16 {
    let mut wall = None;
    for (i, d) in HORIZONTAL.iter().enumerate() {
        let state = world.get(p + *d);
        if state >> 4 == 54 {
            return (54 << 4) | 2;
        }
        if full(state) {
            if wall.is_some() {
                wall = None;
                break;
            }
            wall = Some(i);
        }
    }
    let mut facing = wall.map_or(0, |i| (i + 2) % 4);
    if wall.is_none() {
        if full(world.get(p + HORIZONTAL[facing])) {
            facing = (facing + 2) % 4;
        }
        if full(world.get(p + HORIZONTAL[facing])) {
            facing = (facing + 1) % 4;
        }
        if full(world.get(p + HORIZONTAL[facing])) {
            facing = (facing + 2) % 4;
        }
    }
    (54 << 4) | [2, 5, 3, 4][facing]
}
pub fn dungeon(world: &mut impl FeatureWorld, random: &mut JavaRandom, origin: IVec3) -> bool {
    let rx = random.next_int(2) + 2;
    let rz = random.next_int(2) + 2;
    let mut openings = 0;
    for x in -rx - 1..=rx + 1 {
        for y in -1..=4 {
            for z in -rz - 1..=rz + 1 {
                let p = origin + IVec3::new(x, y, z);
                if (y == -1 || y == 4) && !solid(world.get(p)) {
                    return false;
                }
                if (x == -rx - 1 || x == rx + 1 || z == -rz - 1 || z == rz + 1)
                    && y == 0
                    && world.get(p) == 0
                    && world.get(p + IVec3::Y) == 0
                {
                    openings += 1;
                }
            }
        }
    }
    if !(1..=5).contains(&openings) {
        return false;
    }
    for x in -rx - 1..=rx + 1 {
        for y in (-1..=3).rev() {
            for z in -rz - 1..=rz + 1 {
                let p = origin + IVec3::new(x, y, z);
                if x == -rx - 1 || x == rx + 1 || y == -1 || z == -rz - 1 || z == rz + 1 {
                    if p.y >= 0 && !solid(world.get(p - IVec3::Y)) {
                        world.set(p, 0);
                    } else if solid(world.get(p)) && world.get(p) >> 4 != 54 {
                        world.set(
                            p,
                            if y == -1 && random.next_int(4) != 0 {
                                48 << 4
                            } else {
                                4 << 4
                            },
                        );
                    }
                } else if world.get(p) >> 4 != 54 {
                    world.set(p, 0);
                }
            }
        }
    }
    for _ in 0..2 {
        for _ in 0..3 {
            let p = origin
                + IVec3::new(
                    random.next_int(rx * 2 + 1) - rx,
                    0,
                    random.next_int(rz * 2 + 1) - rz,
                );
            if world.get(p) != 0
                || HORIZONTAL
                    .iter()
                    .filter(|d| solid(world.get(p + **d)))
                    .count()
                    != 1
            {
                continue;
            }
            world.set(p, chest_facing(world, p));
            let mut entries = [
                (329, 1, 1, 10),
                (265, 1, 4, 10),
                (297, 1, 1, 10),
                (296, 1, 4, 10),
                (289, 1, 4, 10),
                (287, 1, 4, 10),
                (325, 1, 1, 10),
                (322, 1, 1, 1),
                (331, 1, 4, 10),
                (2256, 1, 1, 4),
                (2257, 1, 1, 4),
                (421, 1, 1, 10),
                (418, 1, 1, 2),
                (417, 1, 1, 5),
                (419, 1, 1, 1),
            ]
            .into_iter()
            .map(|(id, min, max, weight)| LootEntry::new(id, 0, min, max, weight))
            .collect::<Vec<_>>();
            entries.push(LootEntry::book(random));
            world.chest(p, loot::fill_loot(random, &entries, 8, 27));
            break;
        }
    }
    world.set(origin, 52 << 4);
    world.spawner(
        origin,
        ["Skeleton", "Zombie", "Zombie", "Spider"][random.next_int(4) as usize],
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    struct FixtureWorld {
        scenario: i32,
        changed: BTreeMap<[i32; 3], u16>,
        chests: BTreeMap<[i32; 3], Vec<Option<LootStack>>>,
        spawners: BTreeMap<[i32; 3], String>,
        entities: Vec<GeneratedEntity>,
    }
    fn initial(p: IVec3, scenario: i32) -> u16 {
        let IVec3 { x, y, z } = p;
        if !(0..256).contains(&y) {
            return 0;
        }
        match scenario {
            0 => 16,
            1 => {
                if (x + y + z).rem_euclid(7) == 0 {
                    19
                } else if (x + y + z).rem_euclid(11) == 0 {
                    48
                } else {
                    16
                }
            }
            2..=5 => {
                if y > 64 {
                    if scenario == 3 && y <= 68 {
                        144
                    } else {
                        0
                    }
                } else if y >= 60 {
                    48
                } else {
                    16
                }
            }
            6 | 7 => {
                if (40..=41).contains(&y) && z == 16 || scenario == 7 && x == 17 && y == 39 {
                    0
                } else {
                    16
                }
            }
            9..=14 => {
                if scenario == 14 && x == 16 && z == 16 && y == 66 {
                    16
                } else if y > 64 {
                    0
                } else if y < 64 {
                    16
                } else if scenario == 10 {
                    12 << 4
                } else if scenario == 11 && x.rem_euclid(4) == 0 {
                    9 << 4
                } else if scenario == 12 {
                    (3 << 4) | 2
                } else if scenario == 13 {
                    60 << 4
                } else {
                    2 << 4
                }
            }
            15 => {
                if y > 64 {
                    0
                } else if y == 64 {
                    80 << 4
                } else if y >= 60 {
                    3 << 4
                } else {
                    16
                }
            }
            18 => {
                if y > 64 {
                    0
                } else if y == 64 {
                    110 << 4
                } else {
                    16
                }
            }
            19 => {
                if y > 64 {
                    0
                } else if y == 64 {
                    9 << 4
                } else if y == 63 {
                    2 << 4
                } else {
                    16
                }
            }
            20 => {
                if x == 17 && (65..128).contains(&y) {
                    16
                } else if y > 64 {
                    0
                } else if y == 64 {
                    2 << 4
                } else {
                    16
                }
            }
            21 | 22 => {
                let top = if scenario == 21 { 64 } else { 244 };
                if y > top {
                    0
                } else if y == top {
                    121 << 4
                } else {
                    16
                }
            }
            _ => 0,
        }
    }
    impl FeatureWorld for FixtureWorld {
        fn get(&self, p: IVec3) -> u16 {
            self.changed
                .get(&p.to_array())
                .copied()
                .unwrap_or_else(|| initial(p, self.scenario))
        }
        fn set(&mut self, p: IVec3, state: u16) {
            if !(0..256).contains(&p.y) {
                return;
            }
            if state == initial(p, self.scenario) {
                self.changed.remove(&p.to_array());
            } else {
                self.changed.insert(p.to_array(), state);
            }
            self.chests.remove(&p.to_array());
            self.spawners.remove(&p.to_array());
        }
        fn biome(&self, _: IVec3) -> u8 {
            match self.scenario {
                4 => 14,
                5 => 12,
                _ => 1,
            }
        }
        fn light(&self, _: IVec3, sky: bool) -> u8 {
            if sky && self.scenario != 4 {
                15
            } else {
                0
            }
        }
        fn chest(&mut self, p: IVec3, slots: Vec<Option<LootStack>>) {
            self.chests.insert(p.to_array(), slots);
        }
        fn spawner(&mut self, p: IVec3, entity: &'static str) {
            self.spawners.insert(p.to_array(), entity.into());
        }
        fn entity(&mut self, entity: GeneratedEntity) {
            self.entities.push(entity);
        }
    }
    fn corpus() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-population.json"
        ))
        .unwrap()
    }
    #[test]
    fn population_rng_reseed_matches_original_signed_overflow() {
        for row in corpus()["reseeds"].as_array().unwrap() {
            let seed = row[0].as_str().unwrap().parse().unwrap();
            let x = row[1][0].as_i64().unwrap() as i32;
            let z = row[1][1].as_i64().unwrap() as i32;
            assert_eq!(
                population_random(seed, x, z).next_long().to_string(),
                row[2].as_str().unwrap()
            );
        }
    }
    #[test]
    fn unsupported_decoration_and_missing_neighbors_fail_before_mutation() {
        let mut fixture = FixtureWorld {
            scenario: 9,
            changed: BTreeMap::new(),
            chests: BTreeMap::new(),
            spawners: BTreeMap::new(),
            entities: Vec::new(),
        };
        let mut random = JavaRandom::new(189);
        let mut expected = random.clone();
        assert!(decoration::Decorator::default()
            .decorate_before_springs(&mut fixture, &mut random, IVec3::ZERO, 255)
            .is_err());
        assert!(fixture.changed.is_empty());
        assert_eq!(random.next_long(), expected.next_long());
        let mut world = World {
            chunks: Default::default(),
            spawn: Default::default(),
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        };
        assert!(WorldRegion::new(
            &mut world,
            IVec3::ZERO,
            IVec3::new(31, 255, 31),
            |_, _, _| 0
        )
        .is_err());
        assert!(world.edits.is_empty());
    }
    #[test]
    fn feature_geometry_loot_and_rng_match_original_bytecode() {
        let corpus = corpus();
        let covered: std::collections::BTreeSet<_> = corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["kind"] == "decorator")
            .map(|row| row["size"].as_u64().unwrap() as u8)
            .collect();
        assert_eq!(
            covered.len(),
            61,
            "All original registered biome decorators need integrated fixtures"
        );
        for id in 0u8..=255 {
            assert_eq!(
                decoration::Decorator::supports(id),
                covered.contains(&id),
                "biome {id} coverage"
            );
        }
        let mut passed = 0;
        for (index, row) in corpus["cases"].as_array().unwrap().iter().enumerate() {
            let seed = row["seed"].as_str().unwrap().parse().unwrap();
            let mut random = JavaRandom::new(seed);
            let mut world = FixtureWorld {
                scenario: row["scenario"].as_i64().unwrap() as i32,
                changed: BTreeMap::new(),
                chests: BTreeMap::new(),
                spawners: BTreeMap::new(),
                entities: Vec::new(),
            };
            let p = IVec3::from_array(std::array::from_fn(|i| {
                row["position"][i].as_i64().unwrap() as i32
            }));
            let state = row["state"].as_u64().unwrap() as u16;
            let kind = row["kind"].as_str().unwrap();
            let success = match kind {
                "ore" => minable(
                    &mut world,
                    &mut random,
                    p,
                    state,
                    row["size"].as_i64().unwrap() as i32,
                ),
                "ores" => {
                    default_ores(&mut world, &mut random, p);
                    true
                }
                "lake" => lake(&mut world, &mut random, p, state),
                "dungeon" => dungeon(&mut world, &mut random, p),
                "decorator" => {
                    let mut decorator = decoration::Decorator::default();
                    decorator
                        .decorate_before_springs(
                            &mut world,
                            &mut random,
                            p,
                            row["size"].as_u64().unwrap() as u8,
                        )
                        .unwrap();
                    decorator
                        .decorate_after_springs(
                            &mut world,
                            &mut random,
                            p,
                            row["size"].as_u64().unwrap() as u8,
                        )
                        .unwrap();
                    true
                }
                "prelude" => {
                    random = population_random(seed, p.x, p.z);
                    lakes_and_dungeons(&mut world, &mut random, p.x, p.z, false);
                    true
                }
                "tree" => vegetation::tree(&mut world, &mut random, p, 4, 17 << 4, 18 << 4, false),
                "birch" => vegetation::birch(&mut world, &mut random, p, false),
                "tall_birch" => vegetation::birch(&mut world, &mut random, p, true),
                "spruce" => vegetation::spruce(&mut world, &mut random, p),
                "pine" => vegetation::pine(&mut world, &mut random, p),
                "acacia" => vegetation::acacia(&mut world, &mut random, p),
                "well" => vegetation::desert_well(&mut world, p),
                "swamp_tree" => vegetation::swamp_tree(&mut world, &mut random, p),
                "end_spike" => vegetation::end_spike(&mut world, &mut random, p),
                "mega_pine" | "mega_spruce" | "mega_pine_decorated" | "mega_spruce_decorated" => {
                    let success = vegetation::mega_pine(
                        &mut world,
                        &mut random,
                        p,
                        kind.starts_with("mega_spruce"),
                    );
                    if success && kind.ends_with("decorated") {
                        vegetation::mega_pine_soil(&mut world, &mut random, p);
                    }
                    success
                }
                "mega_jungle" => vegetation::mega_jungle(&mut world, &mut random, p),
                "blob" => vegetation::block_blob(&mut world, &mut random, p),
                "shrub" => vegetation::shrub(&mut world, &mut random, p),
                "dark_oak" => vegetation::dark_oak(&mut world, &mut random, p),
                "melon" => vegetation::melon(&mut world, &mut random, p),
                "vines" => vegetation::vines(&mut world, &mut random, p),
                "ice_path" => vegetation::ice_path(&mut world, &mut random, p),
                "ice_spike" => vegetation::ice_spike(&mut world, &mut random, p),
                "big_mushroom" | "big_mushroom_sequence" => {
                    let mut mushroom = vegetation::BigMushroom::default();
                    let success = mushroom.generate(&mut world, &mut random, p);
                    if kind == "big_mushroom_sequence" {
                        mushroom.generate(&mut world, &mut random, p + IVec3::X * 32);
                        mushroom.generate(&mut world, &mut random, p + IVec3::X * 64);
                    }
                    success
                }
                "big_oak" | "big_oak_raw" | "big_oak_sequence" => {
                    let mut tree = vegetation::BigTree::default();
                    if kind != "big_oak_raw" {
                        tree.prepare();
                    }
                    let success = tree.generate(&mut world, &mut random, p);
                    if kind == "big_oak_sequence" {
                        tree.generate(&mut world, &mut random, p + IVec3::X * 32);
                        tree.generate(&mut world, &mut random, p + IVec3::X * 64);
                    }
                    success
                }
                "jungle" => vegetation::tree(
                    &mut world,
                    &mut random,
                    p,
                    5,
                    (17 << 4) | 3,
                    (18 << 4) | 3,
                    true,
                ),
                "grass" => vegetation::tall_grass(&mut world, &mut random, p, 1),
                "flower" => vegetation::flowers(&mut world, &mut random, p, 37 << 4),
                "mushroom" => vegetation::mushroom(&mut world, &mut random, p, 39 << 4),
                "dead_bush" => vegetation::dead_bush(&mut world, &mut random, p),
                "reeds" => vegetation::reeds(&mut world, &mut random, p),
                "cactus" => vegetation::cactus(&mut world, &mut random, p),
                "waterlily" => vegetation::waterlily(&mut world, &mut random, p),
                "pumpkin" => vegetation::pumpkin(&mut world, &mut random, p),
                "sand" => vegetation::sediment(&mut world, &mut random, p, 12 << 4, 7, false),
                "clay" => vegetation::sediment(&mut world, &mut random, p, 82 << 4, 4, true),
                _ => panic!("Unknown oracle feature"),
            };
            let label = format!("case{index} {kind} seed{seed} scenario{}", world.scenario);
            assert_eq!(success, row["success"].as_bool().unwrap(), "{label}");
            assert_eq!(
                random.next_long().to_string(),
                row["next_long"].as_str().unwrap(),
                "{label} RNG"
            );
            assert_eq!(
                world.changed.len(),
                row["changed"].as_u64().unwrap() as usize,
                "{label} changed count"
            );
            let mut hash = 0xcbf29ce484222325u64;
            for (p, state) in &world.changed {
                for value in [p[0], p[1], p[2], *state as i32] {
                    for b in value.to_le_bytes() {
                        hash = (hash ^ b as u64).wrapping_mul(0x100000001b3);
                    }
                }
            }
            assert_eq!(
                format!("{hash:x}"),
                row["hash"].as_str().unwrap(),
                "{label} all changed block positions/states"
            );
            assert_eq!(
                serde_json::to_value(world.chests.into_iter().collect::<Vec<_>>()).unwrap(),
                row["chests"],
                "{label} exact chest contents"
            );
            assert_eq!(
                serde_json::to_value(world.spawners.into_iter().collect::<Vec<_>>()).unwrap(),
                row["spawners"],
                "{label} exact spawner types"
            );
            let entities=world.entities.iter().map(|e|serde_json::json!({"kind":e.kind,"position":e.position,"yaw_bits":e.yaw.to_bits()})).collect::<Vec<_>>();
            assert_eq!(
                serde_json::to_value(entities).unwrap(),
                row["entities"],
                "{label} spawn request metadata"
            );
            passed += 1;
        }
        assert_eq!(passed, 1785);
    }
}

#[cfg(test)]
#[path = "snow_tests.rs"]
mod snow_tests;

#[cfg(test)]
#[path = "ore_performance_tests.rs"]
mod ore_performance_tests;
