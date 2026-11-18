//! Original biome color lookup, altitude temperature and 3x3 color blending.
use crate::{
    vanilla::{
        biome::{biome, get_biome},
        noise::Perlin,
        random::JavaRandom,
    },
    world::World,
};
use glam::IVec3;
use std::sync::OnceLock;

pub struct BiomeColors {
    pub grass: Vec<u32>,
    pub foliage: Vec<u32>,
}
impl Default for BiomeColors {
    fn default() -> Self {
        Self {
            grass: vec![0xffffff; 65536],
            foliage: vec![0xffffff; 65536],
        }
    }
}
fn noise(seed: i64) -> &'static Perlin {
    static TEMPERATURE: OnceLock<Perlin> = OnceLock::new();
    static GRASS: OnceLock<Perlin> = OnceLock::new();
    (if seed == 1234 { &TEMPERATURE } else { &GRASS })
        .get_or_init(|| Perlin::new(&mut JavaRandom::new(seed), 1))
}
fn color_base(id: u8) -> u8 {
    // BiomeGenMutated delegates color to its base, even when its own climate
    // differs (notably Savanna M). Other mutations instantiate new biomes.
    match id {
        130 | 133 | 134 | 149 | 151 | 155 | 156 | 157 | 158 | 163 | 164 => id - 128,
        _ => id,
    }
}
pub fn biome_at(world: &World, p: IVec3) -> u8 {
    world
        .chunks
        .get(&(p.x.div_euclid(16), p.z.div_euclid(16)))
        .map(|c| c.biomes[(p.z.rem_euclid(16) * 16 + p.x.rem_euclid(16)) as usize])
        .unwrap_or(1)
}
pub fn temperature(id: u8, p: IVec3) -> f32 {
    let base = get_biome(i32::from(id))
        .unwrap_or_else(|| biome(1))
        .temperature;
    if p.y <= 64 {
        return base;
    }
    let shift = (noise(1234).sample(f64::from(p.x) / 8.0, f64::from(p.z) / 8.0) * 4.0) as f32;
    base - (shift + p.y as f32 - 64.0) * 0.05 / 30.0
}
impl BiomeColors {
    /// kind: 0 grass, 1 foliage, 2 water multiplier. Palette lookup uses double
    /// arithmetic after Java's f32 temperature/rainfall clamp.
    pub fn color(&self, id: u8, p: IVec3, kind: usize) -> u32 {
        if kind == 2 {
            return if matches!(id, 6 | 134) {
                14745518
            } else {
                0xffffff
            };
        }
        let id = color_base(id);
        let b = get_biome(id as i32).unwrap_or_else(|| biome(1));
        if b.mesa {
            return if kind == 0 { 9470285 } else { 10387789 };
        }
        if id == 6 {
            return if kind == 0
                && noise(2345).sample(p.x as f64 * 0.0225, p.z as f64 * 0.0225) < -0.1
            {
                5011004
            } else {
                6975545
            };
        }
        let t = temperature(id, p).clamp(0.0, 1.0) as f64;
        let rain = b.rainfall.clamp(0.0, 1.0) as f64;
        let x = ((1.0 - t) * 255.0) as usize;
        let y = ((1.0 - rain * t) * 255.0) as usize;
        let palette = if kind == 0 {
            &self.grass
        } else {
            &self.foliage
        };
        let color = palette[(y << 8) | x];
        if kind == 0 && id == 29 {
            ((color & 0xfefefe) + 2634762) >> 1
        } else {
            color
        }
    }
    pub fn average(&self, p: IVec3, kind: usize, biome_at: impl Fn(IVec3) -> u8) -> u32 {
        let mut sum = [0_u32; 3];
        for z in -1..=1 {
            for x in -1..=1 {
                let at = p + IVec3::new(x, 0, z);
                let c = self.color(biome_at(at), at, kind);
                sum[0] += (c >> 16) & 255;
                sum[1] += (c >> 8) & 255;
                sum[2] += c & 255;
            }
        }
        (sum[0] / 9) << 16 | (sum[1] / 9) << 8 | (sum[2] / 9)
    }
    pub fn block(&self, world: &World, p: IVec3, state: u16) -> u32 {
        let id = state >> 4;
        let meta = state & 15;
        let average = |kind| self.average(p, kind, |at| biome_at(world, at));
        match id {
            2 => average(0),
            8 | 9 => average(2),
            18 if meta & 3 == 1 => 0x619961,
            18 if meta & 3 == 2 => 8431445,
            18 | 161 => average(1),
            31 if meta != 0 => self.color(biome_at(world, p), p, 0),
            83 => self.color(biome_at(world, p), p, 0),
            106 => self.color(biome_at(world, p), p, 1),
            111 => 2129968,
            104 | 105 => {
                let age = (meta & 7) as u32;
                (age * 32) << 16 | (255 - age * 8) << 8 | (age * 4)
            }
            140 => average(0),
            175 if matches!(meta & 7, 2 | 3) => average(0),
            _ => 0xffffff,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn negative_chunk_boundary_blends_water_and_leaf_species_are_constant() {
        use crate::world::Chunk;
        use std::{path::PathBuf, sync::Arc};
        let mut left = Chunk::empty();
        left.biomes.fill(6);
        let right = Chunk::empty();
        let world = World {
            chunks: crate::world::ChunkMap::from([((-1, 0), Arc::new(left)), ((0, 0), Arc::new(right))]),
            spawn: glam::Vec3::ZERO,
            label: String::new(),
            save_path: PathBuf::new(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        };
        let colors = BiomeColors::default();
        let p = IVec3::new(-1, 64, 8);
        assert_eq!(biome_at(&world, p), 6);
        assert_eq!(biome_at(&world, p + IVec3::X), 1);
        let swamp = 14745518_u32;
        let channel = |shift: u32| (((swamp >> shift) & 255_u32) * 6 + 255 * 3) / 9;
        let expected = channel(16) << 16 | channel(8) << 8 | channel(0);
        assert_eq!(colors.block(&world, p, 9 << 4), expected);
        assert_eq!(colors.block(&world, p, 18 << 4 | 1), 0x619961);
        assert_eq!(colors.block(&world, p, 18 << 4 | 2), 8431445);
        assert_eq!(colors.block(&world, IVec3::new(4, 64, 8), 9 << 4), 0xffffff);
    }
    #[test]
    fn all_biomes_altitudes_edges_and_mutations_match_original_colors() {
        let mut colors = BiomeColors::default();
        for y in 0..256 {
            for x in 0..256 {
                colors.grass[y * 256 + x] = (((x * 7 + y * 3) & 255) << 16
                    | ((x * 11 + y * 5) & 255) << 8
                    | ((x * 13 + y * 17) & 255)) as u32;
                colors.foliage[y * 256 + x] = (((x * 3 + y * 19) & 255) << 16
                    | ((x * 5 + y * 23) & 255) << 8
                    | ((x * 29 + y * 7) & 255))
                    as u32;
            }
        }
        let biomes: Vec<u8> = (0..=255)
            .filter(|id| get_biome(*id as i32).is_some())
            .collect();
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-biome-colors.json"
        ))
        .unwrap();
        for row in fixture["cases"].as_array().unwrap() {
            let p = IVec3::from_array(serde_json::from_value(row["position"].clone()).unwrap());
            let id = row["biome"].as_i64().unwrap();
            let at = |p: IVec3| {
                if id >= 0 {
                    id as u8
                } else {
                    biomes[(p.x.wrapping_mul(7).wrapping_add(p.z.wrapping_mul(13)))
                        .rem_euclid(biomes.len() as i32) as usize]
                }
            };
            for kind in 0..3 {
                assert_eq!(
                    colors.color(at(p), p, kind) as u64,
                    row["single"][kind].as_u64().unwrap(),
                    "single biome{id} {p} kind{kind}"
                );
                assert_eq!(
                    colors.average(p, kind, at) as u64,
                    row["average"][kind].as_u64().unwrap(),
                    "average biome{id} {p} kind{kind}"
                );
            }
        }
    }
}
