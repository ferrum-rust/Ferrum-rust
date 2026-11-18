//! Original 1.8.9 Nether density and surface/bedrock generation.
use super::{noise::Octaves, random::JavaRandom};
use crate::world::Chunk;
#[path = "nether_caves.rs"]
mod caves;
#[path = "nether_population.rs"]
pub mod population;
pub struct Terrain {
    seed: i64,
    lower: Octaves,
    upper: Octaves,
    main: Octaves,
    sand: Octaves,
    exclusive: Octaves,
    initial: u64,
}
impl Terrain {
    pub fn new(seed: i64) -> Self {
        let mut random = JavaRandom::new(seed);
        let lower = Octaves::new(&mut random, 16);
        let upper = Octaves::new(&mut random, 16);
        let main = Octaves::new(&mut random, 8);
        let sand = Octaves::new(&mut random, 4);
        let exclusive = Octaves::new(&mut random, 4);
        let _unused6 = Octaves::new(&mut random, 10);
        let _unused7 = Octaves::new(&mut random, 16);
        Self {
            seed,
            lower,
            upper,
            main,
            sand,
            exclusive,
            initial: random.state(),
        }
    }
    pub fn initial_random(&self) -> JavaRandom {
        JavaRandom::from_state(self.initial)
    }
    pub fn density(&self, cx: i32, cz: i32) -> Vec<f64> {
        let origin = [cx.wrapping_mul(4), 0, cz.wrapping_mul(4)];
        let d = 684.412;
        let d2 = 2053.236;
        let main = self
            .main
            .generate(origin, [5, 17, 5], [d / 80., d2 / 60., d / 80.]);
        let lower = self.lower.generate(origin, [5, 17, 5], [d, d2, d]);
        let upper = self.upper.generate(origin, [5, 17, 5], [d, d2, d]);
        // The original provider always uses 17 layers. Preserve Java Math.cos
        // results rather than depending on platform CRT cosine rounding. Java 8u202 and 8u501 agree here.
        // cos(y * PI * 6 / 17) * 2 - max(4 - min(y,16-y),0)^3 * 10.
        const VERTICAL: [u64; 17] = [
            0xc083f00000000000,
            0xc070d1bc82ea6bbe,
            0xc0544d2321bb57c7,
            0xc027ee90800ff5de,
            0xbfe183b1c61f0d05,
            0x3ff7a5f6075d4880,
            0x3ffdd6d000370992,
            0x3fc79ee63259b760,
            0xbffb34fa910ea3b7,
            0xbffb34fa910ea3ba,
            0x3fc79ee63259b6ec,
            0x3ffdd6d00037098d,
            0x3ff7a5f6075d4889,
            0xc025183b1c61f0d1,
            0xc0547dd21001febc,
            0xc070f348c86ed5f2,
            0xc083f8de417535df,
        ];
        let vertical = VERTICAL.map(f64::from_bits);
        let mut density = vec![0.; 425];
        let mut index = 0;
        for _x in 0..5 {
            for _z in 0..5 {
                for (y, &height) in vertical.iter().enumerate() {
                    let a = lower[index] / 512.;
                    let b = upper[index] / 512.;
                    let t = (main[index] / 10. + 1.) / 2.;
                    let mut v = if t < 0. {
                        a
                    } else if t > 1. {
                        b
                    } else {
                        a + (b - a) * t
                    };
                    v -= height;
                    if y > 13 {
                        let fade = ((y - 13) as f32 / 3.) as f64;
                        v = v * (1. - fade) + -10. * fade;
                    }
                    density[index] = v;
                    index += 1;
                }
            }
        }
        density
    }
    pub fn base_chunk(&self, cx: i32, cz: i32) -> Chunk {
        super::dimension_terrain::interpolate(
            &self.density(cx, cz),
            4,
            8,
            87 << 4,
            Some((32, 11 << 4)),
        )
    }
    pub fn surface(&self, cx: i32, cz: i32, chunk: &mut Chunk, random: &mut JavaRandom) {
        let scale = 0.03125;
        let sand = self
            .sand
            .generate([cx * 16, cz * 16, 0], [16, 16, 1], [scale, scale, 1.]);
        let gravel = self
            .sand
            .generate([cx * 16, 109, cz * 16], [16, 1, 16], [scale, 1., scale]);
        let exclusive =
            self.exclusive
                .generate([cx * 16, cz * 16, 0], [16, 16, 1], [scale * 2.; 3]);
        for z in 0..16 {
            for x in 0..16 {
                let i = (z + x * 16) as usize;
                let soul = sand[i] + random.next_double() * 0.2 > 0.;
                let gravel = gravel[i] + random.next_double() * 0.2 > 0.;
                let thickness = (exclusive[i] / 3. + 3. + random.next_double() * 0.25) as i32;
                let mut remaining = -1;
                let (mut top, mut filler) = (87 << 4, 87 << 4);
                for y in (0..128).rev() {
                    if y >= 127 - random.next_int(5) || y <= random.next_int(5) {
                        chunk.set(x, y, z, 7 << 4);
                        continue;
                    }
                    let id = chunk.get(x, y, z) >> 4;
                    if id == 0 {
                        remaining = -1;
                        continue;
                    }
                    if id != 87 {
                        continue;
                    }
                    if remaining == -1 {
                        if thickness <= 0 {
                            top = 0;
                            filler = 87 << 4;
                        } else if (60..=65).contains(&y) {
                            top = 87 << 4;
                            filler = 87 << 4;
                            if gravel {
                                top = 13 << 4;
                            }
                            if soul {
                                top = 88 << 4;
                                filler = 88 << 4;
                            }
                        }
                        if y < 64 && top == 0 {
                            top = 11 << 4;
                        }
                        remaining = thickness;
                        chunk.set(x, y, z, if y >= 63 { top } else { filler });
                    } else if remaining > 0 {
                        remaining -= 1;
                        chunk.set(x, y, z, filler);
                    }
                }
            }
        }
    }
    pub fn generate_with_random(&self, cx: i32, cz: i32, random: &mut JavaRandom) -> Chunk {
        random.set_seed(
            (cx as i64)
                .wrapping_mul(341873128712)
                .wrapping_add((cz as i64).wrapping_mul(132897987541)),
        );
        let mut chunk = self.base_chunk(cx, cz);
        self.surface(cx, cz, &mut chunk, random);
        caves::generate(self.seed, cx, cz, &mut chunk.blocks);
        chunk.biomes.fill(8);
        chunk
    }
    pub fn generate(&self, cx: i32, cz: i32) -> Chunk {
        self.generate_with_random(cx, cz, &mut self.initial_random())
    }
}
