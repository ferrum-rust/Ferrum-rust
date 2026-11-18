//! Original 1.8.9 End island terrain; no later-version outer islands.
use super::{noise::Octaves, random::JavaRandom};
use crate::world::Chunk;
/// End population uses World.rand directly, unlike the provider's own RNG.
/// Call with the world's shared handle; preserve fallInstantly around the call
/// and the original decorator reentry guard in the runtime adapter.
pub fn populate(
    world: &mut impl super::population::FeatureWorld,
    random: &mut JavaRandom,
    decorator: &mut super::population::decoration::Decorator,
    cx: i32,
    cz: i32,
) -> anyhow::Result<()> {
    let origin = glam::IVec3::new(cx.wrapping_mul(16), 0, cz.wrapping_mul(16));
    let biome = world.biome(origin + glam::IVec3::new(16, 0, 16));
    anyhow::ensure!(
        biome == 9,
        "End provider requires original Sky biome, received {biome}"
    );
    decorator.decorate_before_springs(world, random, origin, biome)?;
    decorator.decorate_after_springs(world, random, origin, biome)
}
pub struct Terrain {
    lower: Octaves,
    upper: Octaves,
    main: Octaves,
    initial: u64,
}
impl Terrain {
    pub fn new(seed: i64) -> Self {
        let mut random = JavaRandom::new(seed);
        let lower = Octaves::new(&mut random, 16);
        let upper = Octaves::new(&mut random, 16);
        let main = Octaves::new(&mut random, 8);
        let _unused4 = Octaves::new(&mut random, 10);
        let _unused5 = Octaves::new(&mut random, 16);
        Self {
            lower,
            upper,
            main,
            initial: random.state(),
        }
    }
    pub fn initial_random(&self) -> JavaRandom {
        JavaRandom::from_state(self.initial)
    }
    pub fn density(&self, cx: i32, cz: i32) -> Vec<f64> {
        let origin = [cx.wrapping_mul(2), 0, cz.wrapping_mul(2)];
        let d = 684.412 * 2.;
        let d2 = 684.412;
        let main = self
            .main
            .generate(origin, [3, 33, 3], [d / 80., d2 / 160., d / 80.]);
        let lower = self.lower.generate(origin, [3, 33, 3], [d, d2, d]);
        let upper = self.upper.generate(origin, [3, 33, 3], [d, d2, d]);
        let mut density = vec![0.; 297];
        let mut index = 0;
        for x in 0..3 {
            for z in 0..3 {
                let fx = (x + origin[0]) as f32;
                let fz = (z + origin[2]) as f32;
                let radial =
                    (100.0f32 - ((fx * fx + fz * fz) as f64).sqrt() as f32 * 8.).clamp(-100., 80.);
                for y in 0..33 {
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
                    v -= 8.;
                    v += radial as f64;
                    if y > 14 {
                        let fade = (((y - 14) as f32 / 64.) as f64).clamp(0., 1.);
                        v = v * (1. - fade) + -3000. * fade;
                    }
                    if y < 8 {
                        let fade = ((8 - y) as f32 / 7.) as f64;
                        v = v * (1. - fade) + -30. * fade;
                    }
                    density[index] = v;
                    index += 1;
                }
            }
        }
        density
    }
    /// Original surface replacement only changes stone; native base density
    /// already emits end stone, but callers may verify this pass independently.
    pub fn surface(&self, chunk: &mut Chunk) {
        for x in 0..16 {
            for z in 0..16 {
                let mut remaining = -1;
                for y in (0..128).rev() {
                    match chunk.get(x, y, z) >> 4 {
                        0 => remaining = -1,
                        1 => {
                            if remaining == -1 {
                                remaining = 1;
                                chunk.set(x, y, z, 121 << 4);
                            } else if remaining > 0 {
                                remaining -= 1;
                                chunk.set(x, y, z, 121 << 4);
                            }
                        }
                        _ => {}
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
        let mut chunk =
            super::dimension_terrain::interpolate(&self.density(cx, cz), 2, 4, 121 << 4, None);
        self.surface(&mut chunk);
        chunk.biomes.fill(9);
        chunk
    }
    pub fn generate(&self, cx: i32, cz: i32) -> Chunk {
        self.generate_with_random(cx, cz, &mut self.initial_random())
    }
}
