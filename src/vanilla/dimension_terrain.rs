//! Shared original Nether/End coarse-density interpolation, preserving each add.
use crate::world::Chunk;
pub(super) fn interpolate(
    density: &[f64],
    grid: usize,
    vertical_steps: usize,
    solid: u16,
    fluid: Option<(usize, u16)>,
) -> Chunk {
    let horizontal_steps = 16 / grid;
    let columns = grid + 1;
    let layers = 128 / vertical_steps;
    let points = layers + 1;
    let mut chunk = Chunk::empty();
    let index = |x: usize, y: usize, z: usize| (x * columns + z) * points + y;
    for x in 0..grid {
        for z in 0..grid {
            for y in 0..layers {
                let mut c = [
                    density[index(x, y, z)],
                    density[index(x, y, z + 1)],
                    density[index(x + 1, y, z)],
                    density[index(x + 1, y, z + 1)],
                ];
                let above = [
                    density[index(x, y + 1, z)],
                    density[index(x, y + 1, z + 1)],
                    density[index(x + 1, y + 1, z)],
                    density[index(x + 1, y + 1, z + 1)],
                ];
                let dy: [f64; 4] =
                    std::array::from_fn(|i| (above[i] - c[i]) * (1.0 / vertical_steps as f64));
                for by in 0..vertical_steps {
                    let (mut a, mut b) = (c[0], c[1]);
                    let dx = (c[2] - c[0]) * (1.0 / horizontal_steps as f64);
                    let dx2 = (c[3] - c[1]) * (1.0 / horizontal_steps as f64);
                    for bx in 0..horizontal_steps {
                        let mut v = a;
                        let dz = (b - a) * (1.0 / horizontal_steps as f64);
                        for bz in 0..horizontal_steps {
                            let py = y * vertical_steps + by;
                            let state = if v > 0.0 {
                                solid
                            } else {
                                fluid
                                    .filter(|(height, _)| py < *height)
                                    .map(|(_, state)| state)
                                    .unwrap_or(0)
                            };
                            chunk.set(
                                (x * horizontal_steps + bx) as i32,
                                py as i32,
                                (z * horizontal_steps + bz) as i32,
                                state,
                            );
                            v += dz;
                        }
                        a += dx;
                        b += dx2;
                    }
                    for i in 0..4 {
                        c[i] += dy[i];
                    }
                }
            }
        }
    }
    chunk
}

#[cfg(test)]
mod tests {
    use super::super::{end, nether, random::JavaRandom};
    fn hash(bytes: impl IntoIterator<Item = u8>) -> String {
        bytes
            .into_iter()
            .fold(0xcbf29ce484222325u64, |h, b| {
                (h ^ b as u64).wrapping_mul(0x100000001b3)
            })
            .to_string()
    }
    #[test]
    fn original_nether_end_density_surface_caves_biomes_and_rng() {
        let rows: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-dimension-terrain.json"
        ))
        .unwrap();
        for row in rows.as_array().unwrap() {
            let seed = row["seed"].as_str().unwrap().parse::<i64>().unwrap();
            let x = row["chunk"][0].as_i64().unwrap() as i32;
            let z = row["chunk"][1].as_i64().unwrap() as i32;
            let hell = row["dimension"] == "nether";
            let n = nether::Terrain::new(seed);
            let e = end::Terrain::new(seed);
            let mut random = if hell {
                n.initial_random()
            } else {
                e.initial_random()
            };
            assert_eq!(
                random.next_long().to_string(),
                row["initial"].as_str().unwrap()
            );
            let density = if hell {
                n.density(x, z)
            } else {
                e.density(x, z)
            };
            for (i, (value, expected)) in density
                .iter()
                .zip(row["density"].as_array().unwrap())
                .enumerate()
            {
                assert_eq!(
                    value.to_bits(),
                    u64::from_str_radix(expected.as_str().unwrap(), 16).unwrap(),
                    "{} seed={seed} chunk={x},{z} density[{i}]",
                    row["dimension"]
                );
            }
            let mut base = if hell {
                n.base_chunk(x, z)
            } else {
                super::interpolate(&density, 2, 4, 121 << 4, None)
            };
            assert_eq!(
                hash(base.blocks.iter().flat_map(|b| b.to_le_bytes())),
                row["base"].as_str().unwrap(),
                "base {} {seed} {x},{z}",
                row["dimension"]
            );
            let mut random = JavaRandom::new(
                (x as i64)
                    .wrapping_mul(341873128712)
                    .wrapping_add((z as i64).wrapping_mul(132897987541)),
            );
            if hell {
                n.surface(x, z, &mut base, &mut random)
            } else {
                e.surface(&mut base)
            }
            assert_eq!(
                hash(base.blocks.iter().flat_map(|b| b.to_le_bytes())),
                row["surface"].as_str().unwrap(),
                "surface {} {seed} {x},{z}",
                row["dimension"]
            );
            assert_eq!(
                random.next_long().to_string(),
                row["surface_next"].as_str().unwrap()
            );
            let full = if hell {
                n.generate_with_random(x, z, &mut random)
            } else {
                e.generate_with_random(x, z, &mut random)
            };
            assert_eq!(
                hash(full.blocks.iter().flat_map(|b| b.to_le_bytes())),
                row["blocks"].as_str().unwrap(),
                "full {} {seed} {x},{z}",
                row["dimension"]
            );
            assert_eq!(
                hash(full.biomes.iter().copied()),
                row["biomes"].as_str().unwrap()
            );
            assert_eq!(
                random.next_long().to_string(),
                row["next"].as_str().unwrap()
            );
        }
        assert_eq!(rows.as_array().unwrap().len(), 252);
    }
}
