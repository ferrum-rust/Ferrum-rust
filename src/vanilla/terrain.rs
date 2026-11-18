//! Native 1.8.9 DEFAULT density interpolation and biome surface replacement.
//! Population/structures are separate stages; this module never invokes Java.
use super::{
    biome::{biome, Biome, BiomeGenerator},
    noise::{Octaves, Perlin},
    random::JavaRandom,
};
use crate::world::Chunk;
use anyhow::Result;

/// Fields consumed by 1.8.9 ChunkProviderGenerate before population.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub coordinate_scale: f32,
    pub height_scale: f32,
    pub lower_limit_scale: f32,
    pub upper_limit_scale: f32,
    pub depth_noise_scale_x: f32,
    pub depth_noise_scale_z: f32,
    pub main_noise_scale_x: f32,
    pub main_noise_scale_y: f32,
    pub main_noise_scale_z: f32,
    pub base_size: f32,
    pub stretch_y: f32,
    pub biome_depth_offset: f32,
    pub biome_depth_weight: f32,
    pub biome_scale_offset: f32,
    pub biome_scale_weight: f32,
    pub sea_level: i32,
    pub use_lava_oceans: bool,
    pub use_caves: bool,
    pub use_ravines: bool,
    pub fixed_biome: i32,
    pub biome_size: usize,
    pub river_size: usize,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            coordinate_scale: 684.412,
            height_scale: 684.412,
            lower_limit_scale: 512.,
            upper_limit_scale: 512.,
            depth_noise_scale_x: 200.,
            depth_noise_scale_z: 200.,
            main_noise_scale_x: 80.,
            main_noise_scale_y: 160.,
            main_noise_scale_z: 80.,
            base_size: 8.5,
            stretch_y: 12.,
            biome_depth_offset: 0.,
            biome_depth_weight: 1.,
            biome_scale_offset: 0.,
            biome_scale_weight: 1.,
            sea_level: 63,
            use_lava_oceans: false,
            use_caves: true,
            use_ravines: true,
            fixed_biome: -1,
            biome_size: 4,
            river_size: 4,
        }
    }
}

pub struct Terrain {
    seed: i64,
    initial_random_state: u64,
    lower: Octaves,
    upper: Octaves,
    main: Octaves,
    surface: Perlin,
    depth: Octaves,
    parabolic: [f32; 25],
    temperature: Perlin,
    grass: Perlin,
    mesa: Mesa,
    pub amplified: bool,
    pub settings: Settings,
}
impl Terrain {
    pub fn new(seed: i64) -> Self {
        let mut random = JavaRandom::new(seed);
        let lower = Octaves::new(&mut random, 16);
        let upper = Octaves::new(&mut random, 16);
        let main = Octaves::new(&mut random, 8);
        let surface = Perlin::new(&mut random, 4);
        let _unused_scale = Octaves::new(&mut random, 10);
        let depth = Octaves::new(&mut random, 16);
        let _unused_mob = Octaves::new(&mut random, 8);
        let parabolic = std::array::from_fn(|index| {
            let x = index as i32 % 5 - 2;
            let z = index as i32 / 5 - 2;
            10.0f32 / (((x * x + z * z) as f32 + 0.2f32) as f64).sqrt() as f32
        });
        Self {
            seed,
            initial_random_state: random.state(),
            lower,
            upper,
            main,
            surface,
            depth,
            parabolic,
            temperature: Perlin::new(&mut JavaRandom::new(1234), 1),
            grass: Perlin::new(&mut JavaRandom::new(2345), 1),
            mesa: Mesa::new(seed),
            amplified: false,
            settings: Settings::default(),
        }
    }
    pub fn initial_random(&self) -> JavaRandom {
        JavaRandom::from_state(self.initial_random_state)
    }
    /// Input is WorldChunkManager's 10x10 generation grid at chunk*4-2.
    /// Density output index is (x*5+z)*33+y, exactly the original noise arrays.
    pub fn density(&self, chunk_x: i32, chunk_z: i32, biomes: &[u8]) -> Vec<f64> {
        assert_eq!(biomes.len(), 100);
        let origin = [chunk_x.wrapping_mul(4), 0, chunk_z.wrapping_mul(4)];
        let settings = &self.settings;
        let depth = self.depth.generate_2d(
            origin[0],
            origin[2],
            5,
            5,
            f64::from(settings.depth_noise_scale_x),
            f64::from(settings.depth_noise_scale_z),
        );
        let coordinate = settings.coordinate_scale;
        let height = settings.height_scale;
        let main = self.main.generate(
            origin,
            [5, 33, 5],
            [
                (coordinate / settings.main_noise_scale_x) as f64,
                (height / settings.main_noise_scale_y) as f64,
                (coordinate / settings.main_noise_scale_z) as f64,
            ],
        );
        let lower = self.lower.generate(
            origin,
            [5, 33, 5],
            [coordinate as f64, height as f64, coordinate as f64],
        );
        let upper = self.upper.generate(
            origin,
            [5, 33, 5],
            [coordinate as f64, height as f64, coordinate as f64],
        );
        let mut density = vec![0.; 825];
        for x in 0..5 {
            for z in 0..5 {
                let center = biome(biomes[x + 2 + (z + 2) * 10]);
                let mut scale = 0.0f32;
                let mut base = 0.0f32;
                let mut weights = 0.0f32;
                for dx in -2i32..=2 {
                    for dz in -2i32..=2 {
                        let neighbor = biome(
                            biomes
                                [(x as i32 + dx + 2) as usize + (z as i32 + dz + 2) as usize * 10],
                        );
                        let mut height = settings.biome_depth_offset
                            + neighbor.base_height * settings.biome_depth_weight;
                        let mut variation = settings.biome_scale_offset
                            + neighbor.height_variation * settings.biome_scale_weight;
                        if self.amplified && height > 0. {
                            height = 1. + height * 2.;
                            variation = 1. + variation * 4.;
                        }
                        let mut weight =
                            self.parabolic[(dx + 2 + (dz + 2) * 5) as usize] / (height + 2.);
                        if neighbor.base_height > center.base_height {
                            weight /= 2.;
                        }
                        scale += variation * weight;
                        base += height * weight;
                        weights += weight;
                    }
                }
                scale /= weights;
                base /= weights;
                scale = scale * 0.9f32 + 0.1f32;
                base = (base * 4.0f32 - 1.0f32) / 8.0f32;
                let mut detail = depth[x * 5 + z] / 8000.;
                if detail < 0. {
                    detail = -detail * 0.3;
                }
                detail = detail * 3. - 2.;
                if detail < 0. {
                    detail /= 2.;
                    if detail < -1. {
                        detail = -1.;
                    }
                    detail /= 1.4;
                    detail /= 2.;
                } else {
                    if detail > 1. {
                        detail = 1.;
                    }
                    detail /= 8.;
                }
                let mut offset = base as f64;
                offset += detail * 0.2;
                offset = offset * f64::from(settings.base_size) / 8.;
                let midpoint = f64::from(settings.base_size) + offset * 4.;
                for y in 0..33 {
                    let index = (x * 5 + z) * 33 + y;
                    let mut falloff = (y as f64 - midpoint) * f64::from(settings.stretch_y) * 128.
                        / 256.
                        / scale as f64;
                    if falloff < 0. {
                        falloff *= 4.;
                    }
                    let a = lower[index] / f64::from(settings.lower_limit_scale);
                    let b = upper[index] / f64::from(settings.upper_limit_scale);
                    let t = (main[index] / 10. + 1.) / 2.;
                    let mut value = if t < 0. {
                        a
                    } else if t > 1. {
                        b
                    } else {
                        a + (b - a) * t
                    };
                    value -= falloff;
                    if y > 29 {
                        let t = ((y - 29) as f32 / 3.0f32) as f64;
                        value = value * (1. - t) + -10. * t;
                    }
                    density[index] = value;
                }
            }
        }
        density
    }
    pub fn base_chunk(&self, chunk_x: i32, chunk_z: i32, biomes: &[u8]) -> Chunk {
        let density = self.density(chunk_x, chunk_z, biomes);
        let mut chunk = Chunk::empty();
        // This fresh, unobserved column contains only air and has no tile or
        // tick metadata. The base pass writes only stone/still water/still lava;
        // preserve its original traversal while borrowing Dense storage once.
        let mut top = chunk.top;
        let blocks = chunk.blocks.as_mut_slice();
        for x in 0..4 {
            for z in 0..4 {
                for y in 0..32 {
                    let i00 = (x * 5 + z) * 33 + y;
                    let i01 = (x * 5 + z + 1) * 33 + y;
                    let i10 = ((x + 1) * 5 + z) * 33 + y;
                    let i11 = ((x + 1) * 5 + z + 1) * 33 + y;
                    let mut a = density[i00];
                    let mut b = density[i01];
                    let mut c = density[i10];
                    let mut d = density[i11];
                    let da = (density[i00 + 1] - a) * 0.125;
                    let db = (density[i01 + 1] - b) * 0.125;
                    let dc = (density[i10 + 1] - c) * 0.125;
                    let dd = (density[i11 + 1] - d) * 0.125;
                    for sy in 0..8 {
                        let mut left = a;
                        let mut right = b;
                        let dx = (c - a) * 0.25;
                        let dx_right = (d - b) * 0.25;
                        for sx in 0..4 {
                            let dz = (right - left) * 0.25;
                            let mut value = left - dz;
                            for sz in 0..4 {
                                value += dz;
                                let by = y * 8 + sy;
                                let state = if value > 0. {
                                    1 << 4
                                } else if (by as i32) < self.settings.sea_level {
                                    if self.settings.use_lava_oceans { 11 << 4 } else { 9 << 4 }
                                } else {
                                    0
                                };
                                if state != 0 {
                                    blocks[Chunk::index(
                                        (x * 4 + sx) as i32, by as i32, (z * 4 + sz) as i32,
                                    )] = state;
                                    top = top.max(by as i32 + 1);
                                }
                            }
                            left += dx;
                            right += dx_right;
                        }
                        a += da;
                        b += db;
                        c += dc;
                        d += dd;
                    }
                }
            }
        }
        chunk.top = top;
        chunk
    }
    pub fn generate(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        generator: &BiomeGenerator,
    ) -> Result<Chunk> {
        self.generate_with_random(chunk_x, chunk_z, generator, &mut JavaRandom::new(0))
    }
    /// Uses the actual provider Random object, including its coordinate reseed
    /// and resulting surface-generation state observed by reentrant callers.
    pub fn generate_with_random(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        generator: &BiomeGenerator,
        random: &mut JavaRandom,
    ) -> Result<Chunk> {
        random.set_seed(
            (chunk_x as i64)
                .wrapping_mul(341873128712)
                .wrapping_add((chunk_z as i64).wrapping_mul(132897987541)),
        );
        let generation = generator.get_biomes_for_generation(
            chunk_x.wrapping_mul(4).wrapping_sub(2),
            chunk_z.wrapping_mul(4).wrapping_sub(2),
            10,
            10,
        );
        let biomes =
            generator.get_biomes(chunk_x.wrapping_mul(16), chunk_z.wrapping_mul(16), 16, 16);
        let mut chunk = self.base_chunk(chunk_x, chunk_z, &generation);
        let tops =
            self.replace_surface_tops_with_random(chunk_x, chunk_z, &mut chunk, &biomes, random)?;
        chunk.biomes.copy_from_slice(&biomes);
        if self.settings.use_caves {
            super::carvers::caves(self.seed, chunk_x, chunk_z, &mut chunk.blocks, &tops);
        }
        if self.settings.use_ravines {
            super::carvers::ravines(self.seed, chunk_x, chunk_z, &mut chunk.blocks, &tops);
        }
        Ok(chunk)
    }
    pub fn replace_surface(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        chunk: &mut Chunk,
        biomes: &[u8],
    ) -> Result<()> {
        self.replace_surface_tops(chunk_x, chunk_z, chunk, biomes)
            .map(|_| ())
    }
    pub fn replace_surface_tops(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        chunk: &mut Chunk,
        biomes: &[u8],
    ) -> Result<[u16; 256]> {
        let seed = (chunk_x as i64)
            .wrapping_mul(341873128712)
            .wrapping_add((chunk_z as i64).wrapping_mul(132897987541));
        let mut random = JavaRandom::new(seed);
        self.replace_surface_tops_with_random(chunk_x, chunk_z, chunk, biomes, &mut random)
    }
    /// Caller has already applied ChunkProviderGenerate's coordinate reseed.
    pub fn replace_surface_tops_with_random(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        chunk: &mut Chunk,
        biomes: &[u8],
        random: &mut JavaRandom,
    ) -> Result<[u16; 256]> {
        assert_eq!(biomes.len(), 256);
        let mut current_tops: [u16; 256] =
            std::array::from_fn(|i| super::biome::get_biome(i as i32).map_or(0, |b| b.top));
        let origin = [
            chunk_x.wrapping_mul(16) as f64,
            chunk_z.wrapping_mul(16) as f64,
        ];
        let noise = self
            .surface
            .generate(origin, [16, 16], [0.0625, 0.0625], 1., 0.5);
        for x in 0..16 {
            for z in 0..16 {
                let b = biome(biomes[z + x * 16]);
                let wx = chunk_x.wrapping_mul(16).wrapping_add(x as i32);
                let wz = chunk_z.wrapping_mul(16).wrapping_add(z as i32);
                let dynamic_top = surface_blocks(b, noise[z + x * 16]).0;
                if b.class == "BiomeGenHills" || matches!(b.id, 32 | 33 | 160 | 163 | 164) {
                    current_tops[b.id as usize] = dynamic_top;
                } else if b.id == 161 {
                    // Generic mutated biome delegates terrain to its base object.
                    current_tops[33] = dynamic_top;
                }
                if b.mesa {
                    self.mesa.column(
                        chunk,
                        random,
                        b,
                        (wx, wz),
                        noise[z + x * 16],
                        self.settings.sea_level,
                    );
                } else {
                    self.surface_column(chunk, random, b, wx, wz, noise[z + x * 16]);
                }
            }
        }
        Ok(std::array::from_fn(|i| current_tops[biomes[i] as usize]))
    }
    fn temperature(&self, b: &Biome, x: i32, y: i32, z: i32) -> f32 {
        if y > 64 {
            let noise = (self.temperature.sample(x as f64 / 8., z as f64 / 8.) * 4.) as f32;
            b.temperature - (noise + y as f32 - 64.0f32) * 0.05f32 / 30.0f32
        } else {
            b.temperature
        }
    }
    fn surface_column(
        &self,
        chunk: &mut Chunk,
        random: &mut JavaRandom,
        b: &Biome,
        x: i32,
        z: i32,
        noise: f64,
    ) {
        let (cx, cz) = (z & 15, x & 15); // Vanilla passes transposed local coordinates here.
        let sea_level = self.settings.sea_level;
        let (original_top, original_filler) = surface_blocks(b, noise);
        if b.class == "BiomeGenSwamp" {
            let wet = self.grass.sample(x as f64 * 0.25, z as f64 * 0.25);
            if wet > 0. {
                for y in (0..256).rev() {
                    let state = chunk.get(cx, y, cz);
                    if state == 0 {
                        continue;
                    }
                    if y == 62 && state >> 4 != 9 {
                        chunk.set(cx, y, cz, 9 << 4);
                        if wet < 0.12 {
                            chunk.set(cx, y + 1, cz, 111 << 4);
                        }
                    }
                    break;
                }
            }
        }
        let mut top = original_top;
        let mut filler = original_filler;
        let mut remaining = -1;
        let thickness = (noise / 3. + 3. + random.next_double() * 0.25) as i32;
        for y in (0..256).rev() {
            if y <= random.next_int(5) {
                chunk.set(cx, y, cz, 7 << 4);
                continue;
            }
            let state = chunk.get(cx, y, cz);
            if state == 0 {
                remaining = -1;
                continue;
            }
            if state >> 4 != 1 {
                continue;
            }
            if remaining == -1 {
                if thickness <= 0 {
                    top = 0;
                    filler = 1 << 4;
                } else if (sea_level - 4..=sea_level + 1).contains(&y) {
                    top = original_top;
                    filler = original_filler;
                }
                if y < sea_level && top == 0 {
                    top = if self.temperature(b, x, y, z) < 0.15 {
                        79 << 4
                    } else {
                        9 << 4
                    };
                }
                remaining = thickness;
                if y >= sea_level - 1 {
                    chunk.set(cx, y, cz, top);
                } else if y < sea_level - 7 - thickness {
                    top = 0;
                    filler = 1 << 4;
                    chunk.set(cx, y, cz, 13 << 4);
                } else {
                    chunk.set(cx, y, cz, filler);
                }
            } else if remaining > 0 {
                chunk.set(cx, y, cz, filler);
                remaining -= 1;
                if remaining == 0 && filler >> 4 == 12 {
                    remaining = random.next_int(4) + (y - 63).max(0);
                    filler = if filler & 15 == 1 { 179 << 4 } else { 24 << 4 };
                }
            }
        }
    }
}

fn surface_blocks(b: &Biome, noise: f64) -> (u16, u16) {
    let (mut original_top, mut original_filler) = (b.top, b.filler);
    if b.class == "BiomeGenHills" {
        original_top = 2 << 4;
        original_filler = 3 << 4;
        if (noise < -1. || noise > 2.) && b.id >= 128 {
            original_top = 13 << 4;
            original_filler = 13 << 4;
        } else if noise > 1. && b.id != 34 {
            original_top = 1 << 4;
            original_filler = 1 << 4;
        }
    } else if matches!(b.id, 32 | 33 | 160 | 161) {
        original_top = if noise > 1.75 {
            (3 << 4) | 1
        } else if noise > -0.95 {
            (3 << 4) | 2
        } else {
            2 << 4
        };
        original_filler = 3 << 4;
    } else if matches!(b.id, 163 | 164) {
        original_top = 2 << 4;
        original_filler = 3 << 4;
        if noise > 1.75 {
            original_top = 1 << 4;
            original_filler = 1 << 4;
        } else if noise > -0.5 {
            original_top = (3 << 4) | 1;
        }
    }
    (original_top, original_filler)
}

struct Mesa {
    bands: [u16; 64],
    band_noise: Perlin,
    pillar: Perlin,
    roof: Perlin,
}
impl Mesa {
    fn new(seed: i64) -> Self {
        let mut random = JavaRandom::new(seed);
        let band_noise = Perlin::new(&mut random, 1);
        let mut bands = [172 << 4; 64];
        let mut i = 0;
        while i < 64 {
            i += random.next_int(5) as usize + 1;
            if i < 64 {
                bands[i] = (159 << 4) | 1;
            }
            i += 1;
        }
        for (color, min_length) in [(4, 1), (12, 2), (14, 1)] {
            let count = random.next_int(4) + 2;
            for _ in 0..count {
                let length = random.next_int(3) + min_length;
                let start = random.next_int(64);
                for y in start..(start + length).min(64) {
                    bands[y as usize] = (159 << 4) | color;
                }
            }
        }
        let count = random.next_int(3) + 3;
        let mut y = 0;
        for _ in 0..count {
            y += random.next_int(16) + 4;
            if y < 64 {
                bands[y as usize] = 159 << 4;
                if y > 1 && random.next_boolean() {
                    bands[y as usize - 1] = (159 << 4) | 8;
                }
                if y < 63 && random.next_boolean() {
                    bands[y as usize + 1] = (159 << 4) | 8;
                }
            }
        }
        // In 1.8.9 these two noises initialize from the cached seed field before
        // that field is set to the world's seed; a fresh biome starts at zero.
        let mut pillar_random = JavaRandom::new(0);
        let pillar = Perlin::new(&mut pillar_random, 4);
        let roof = Perlin::new(&mut pillar_random, 1);
        Self {
            bands,
            band_noise,
            pillar,
            roof,
        }
    }
    fn band(&self, x: i32, y: i32) -> u16 {
        // The original intentionally/accidentally samples X twice, not X/Z.
        let shift =
            (self.band_noise.sample(x as f64 / 512., x as f64 / 512.) * 2. + 0.5).floor() as i32;
        self.bands[((y + shift + 64) % 64) as usize]
    }
    fn column(
        &self,
        chunk: &mut Chunk,
        random: &mut JavaRandom,
        b: &Biome,
        position: (i32, i32),
        noise: f64,
        sea_level: i32,
    ) {
        let (x, z) = position;
        let mut pillar_height = 0.;
        if b.id == 165 {
            let px = (x & !15) + (z & 15);
            let pz = (z & !15) + (x & 15);
            let value = noise
                .abs()
                .min(self.pillar.sample(px as f64 * 0.25, pz as f64 * 0.25));
            if value > 0. {
                pillar_height = value * value * 2.5;
                let roof = self
                    .roof
                    .sample(px as f64 * 0.001953125, pz as f64 * 0.001953125)
                    .abs();
                let limit = (roof * 50.).ceil() + 14.;
                if pillar_height > limit {
                    pillar_height = limit;
                }
                pillar_height += 64.;
            }
        }
        let (cx, cz) = (z & 15, x & 15);
        let thickness = (noise / 3. + 3. + random.next_double() * 0.25) as i32;
        let barren = (noise / 3. * std::f64::consts::PI).cos() > 0.;
        let forest = matches!(b.id, 38 | 166);
        let mut filler = b.filler;
        let mut remaining = -1;
        let mut under_sand = false;
        for y in (0..256).rev() {
            if chunk.get(cx, y, cz) == 0 && y < (pillar_height as i32) {
                chunk.set(cx, y, cz, 1 << 4);
            }
            if y <= random.next_int(5) {
                chunk.set(cx, y, cz, 7 << 4);
                continue;
            }
            let current = chunk.get(cx, y, cz);
            if current == 0 {
                remaining = -1;
                continue;
            }
            if current >> 4 != 1 {
                continue;
            }
            if remaining == -1 {
                under_sand = false;
                if thickness <= 0 {
                    filler = 1 << 4;
                } else if (sea_level - 4..=sea_level + 1).contains(&y) {
                    filler = b.filler;
                }
                remaining = thickness + (y - sea_level).max(0);
                if y >= sea_level - 1 {
                    let surface = if forest && y > 86 + thickness * 2 {
                        if barren {
                            (3 << 4) | 1
                        } else {
                            2 << 4
                        }
                    } else if y > sea_level + 3 + thickness {
                        if !(64..=127).contains(&y) {
                            (159 << 4) | 1
                        } else if barren {
                            172 << 4
                        } else {
                            self.band(x, y)
                        }
                    } else {
                        under_sand = true;
                        b.top
                    };
                    chunk.set(cx, y, cz, surface);
                } else {
                    chunk.set(
                        cx,
                        y,
                        cz,
                        if filler >> 4 == 159 {
                            (159 << 4) | 1
                        } else {
                            filler
                        },
                    );
                }
            } else if remaining > 0 {
                remaining -= 1;
                chunk.set(
                    cx,
                    y,
                    cz,
                    if under_sand {
                        (159 << 4) | 1
                    } else {
                        self.band(x, y)
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn corpus() -> serde_json::Value {
        serde_json::from_str(include_str!("../../tests/fixtures/vanilla189-terrain.json")).unwrap()
    }
    fn ids(row: &serde_json::Value, key: &str) -> Vec<u8> {
        row[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u8)
            .collect()
    }
    fn hash(chunk: &Chunk) -> String {
        let hash = chunk
            .blocks
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .fold(0xcbf29ce484222325u64, |h, b| {
                (h ^ b as u64).wrapping_mul(0x100000001b3)
            });
        format!("{hash:x}")
    }
    #[test]
    fn density_and_base_blocks_match_original_chunk_provider() {
        for row in corpus()["cases"].as_array().unwrap() {
            let terrain = Terrain::new(row["seed"].as_str().unwrap().parse().unwrap());
            assert_eq!(
                terrain.initial_random().next_long().to_string(),
                row["initial_next_long"].as_str().unwrap(),
                "provider constructor RNG {}",
                row["name"]
            );
            let (x, z) = (
                row["x"].as_i64().unwrap() as i32,
                row["z"].as_i64().unwrap() as i32,
            );
            let generation = ids(row, "generation");
            let density = terrain.density(x, z, &generation);
            for (i, (value, expected)) in density
                .iter()
                .zip(row["density"].as_array().unwrap())
                .enumerate()
            {
                assert_eq!(
                    format!("{:016x}", value.to_bits()),
                    expected.as_str().unwrap(),
                    "density {} sample {i}",
                    row["name"]
                );
            }
            assert_eq!(
                hash(&terrain.base_chunk(x, z, &generation)),
                row["base_fnv"].as_str().unwrap(),
                "base {}",
                row["name"]
            );
        }
    }
    #[test]
    fn supported_surfaces_match_original_biome_bytecode() {
        let mut tested = 0;
        for row in corpus()["cases"].as_array().unwrap() {
            let terrain = Terrain::new(row["seed"].as_str().unwrap().parse().unwrap());
            let (x, z) = (
                row["x"].as_i64().unwrap() as i32,
                row["z"].as_i64().unwrap() as i32,
            );
            let biomes = ids(row, "biomes");
            let mut chunk = terrain.base_chunk(x, z, &ids(row, "generation"));
            let mut random = JavaRandom::new(
                (x as i64)
                    .wrapping_mul(341873128712)
                    .wrapping_add((z as i64).wrapping_mul(132897987541)),
            );
            let mut observer = random.share();
            let tops = terrain
                .replace_surface_tops_with_random(x, z, &mut chunk, &biomes, &mut random)
                .unwrap();
            assert_eq!(
                observer.next_long().to_string(),
                row["next_long"].as_str().unwrap(),
                "provider RNG after surface {}",
                row["name"]
            );
            assert_eq!(
                hash(&chunk),
                row["surface_fnv"].as_str().unwrap(),
                "surface {}",
                row["name"]
            );
            let expected_tops: Vec<u16> = row["carver_tops"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u16)
                .collect();
            assert_eq!(
                tops.as_slice(),
                expected_tops.as_slice(),
                "carver biome tops {}",
                row["name"]
            );
            super::super::carvers::caves(terrain.seed, x, z, &mut chunk.blocks, &tops);
            super::super::carvers::ravines(terrain.seed, x, z, &mut chunk.blocks, &tops);
            assert_eq!(
                hash(&chunk),
                row["carved_fnv"].as_str().unwrap(),
                "carved {}",
                row["name"]
            );
            tested += 1;
        }
        assert_eq!(tested, 76);
    }
    #[test]
    fn native_biome_density_surface_and_carvers_pipeline_matches_original() {
        for row in corpus()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["fixed"].as_i64() == Some(-1))
        {
            let seed = row["seed"].as_str().unwrap().parse().unwrap();
            let (x, z) = (
                row["x"].as_i64().unwrap() as i32,
                row["z"].as_i64().unwrap() as i32,
            );
            let chunk = Terrain::new(seed)
                .generate(x, z, &BiomeGenerator::new(seed))
                .unwrap();
            assert_eq!(
                chunk.biomes.as_slice(),
                ids(row, "biomes").as_slice(),
                "biomes {}",
                row["name"]
            );
            assert_eq!(
                hash(&chunk),
                row["carved_fnv"].as_str().unwrap(),
                "pipeline {}",
                row["name"]
            );
        }
    }
}

#[cfg(test)]
#[path = "terrain_base_fill_tests.rs"]
mod base_fill_tests;
