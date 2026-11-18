//! Original-1.8.9 compatibility components. These are independently compared
//! against the user's Java bytecode; they are not a completed world generator.
#![allow(dead_code)]
pub mod biome;
pub mod ambient_occlusion;
pub mod atmosphere;
mod biome_data;
pub mod carvers;
pub mod collision_shapes;
pub mod crafting;
pub mod ray_trace;
pub mod respawn;
pub mod sleep;
pub mod server_teleport;
pub mod drops;
mod dimension_terrain;
pub mod end;
pub mod facing_events;
pub mod falling;
pub mod tnt;
pub mod hand_placement;
pub mod fortress;
pub mod food;
pub mod item_food;
pub mod item_entity;
pub mod entity_pick;
pub mod middle_pick;
pub mod click_controller;
pub mod special_click;
pub mod notes;
pub mod click_border;
pub mod tile_updates;
mod tile_import;
pub mod movement;
pub mod potions;
pub mod armor;
pub mod anvil;
pub(crate) mod item_registry;
pub mod item_nbt;
pub mod generator;
pub mod java_map;
pub mod lighting;
pub mod lightmap;
pub mod loot;
pub mod material;
pub mod mining;
pub mod math;
pub mod mineshaft;
pub mod monument;
pub mod nether;
pub mod noise;
pub mod population;
pub mod plant_events;
pub mod provider;
pub mod random;
pub mod rail_events;
pub mod runtime;
pub mod scattered;
pub mod spawning;
pub mod springs;
pub mod stronghold;
pub mod structure_common;
pub mod terrain;
pub mod village;
pub mod weather;

#[cfg(test)]
mod differential {
    use super::{
        noise::{Improved, Octaves, Perlin, Simplex},
        random::JavaRandom,
    };
    use serde_json::Value;
    fn corpus() -> Value {
        serde_json::from_str(include_str!("../../tests/fixtures/vanilla189-noise.json")).unwrap()
    }
    fn floats<const N: usize>(row: &Value, key: &str) -> [f64; N] {
        std::array::from_fn(|i| row[key][i].as_f64().unwrap())
    }
    fn sizes<const N: usize>(row: &Value) -> [usize; N] {
        std::array::from_fn(|i| row["size"][i].as_u64().unwrap() as usize)
    }
    fn seed(row: &Value) -> i64 {
        row["seed"].as_str().unwrap().parse().unwrap()
    }

    #[test]
    fn java_random_matches_original_signed_integer_and_float_bits() {
        let corpus = corpus();
        let bounds = [1, 2, 3, 7, 16, 31, 256, 257, 1000, 1073741825, i32::MAX];
        for row in corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["kind"] == "random")
        {
            let mut random = JavaRandom::new(seed(row));
            let mut actual = Vec::new();
            for i in 0..64 {
                actual.push(random.next_i32().to_string());
                actual.push(random.next_int(bounds[i % bounds.len()]).to_string());
                actual.push(random.next_long().to_string());
                actual.push(random.next_float().to_bits().to_string());
                actual.push(random.next_double().to_bits().to_string());
                actual.push(random.next_boolean().to_string());
            }
            let expected: Vec<_> = row["sequence"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect();
            assert_eq!(actual, expected, "Java Random seed {}", seed(row));
            let mut bytes = [0; 37];
            random.next_bytes(&mut bytes);
            let expected: Vec<_> = row["bytes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as u8)
                .collect();
            assert_eq!(bytes.as_slice(), expected);
            random.set_seed(seed(row));
            assert_eq!(random.next_i32().to_string(), actual[0]);
        }
    }

    #[test]
    fn all_noise_outputs_match_original_java_f64_bit_for_bit() {
        let corpus = corpus();
        let mut compared = 0;
        for row in corpus["cases"].as_array().unwrap() {
            let kind = row["kind"].as_str().unwrap();
            if kind == "random" {
                continue;
            }
            let mut random = JavaRandom::new(seed(row));
            let actual = match kind {
                "improved" => {
                    let noise = Improved::new(&mut random);
                    let size = sizes::<3>(row);
                    let mut out: Vec<f64> = (0..size.iter().product())
                        .map(|i| i as f64 * 0.03125)
                        .collect();
                    noise.add(
                        &mut out,
                        floats(row, "origin"),
                        size,
                        floats(row, "scale"),
                        row["divisor"].as_f64().unwrap(),
                    );
                    out
                }
                "octaves" => {
                    let noise = Octaves::new(&mut random, row["count"].as_u64().unwrap() as usize);
                    let origin = std::array::from_fn(|i| row["origin"][i].as_i64().unwrap() as i32);
                    noise.generate(origin, sizes(row), floats(row, "scale"))
                }
                "simplex_point" => {
                    let noise = Simplex::new(&mut random);
                    row["points"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|p| noise.sample(p[0].as_f64().unwrap(), p[1].as_f64().unwrap()))
                        .collect()
                }
                "simplex_region" => {
                    let noise = Simplex::new(&mut random);
                    let size = sizes::<2>(row);
                    let mut out = vec![0.; size[0] * size[1]];
                    noise.add(
                        &mut out,
                        floats(row, "origin"),
                        size,
                        floats(row, "scale"),
                        row["amplitude"].as_f64().unwrap(),
                    );
                    out
                }
                "perlin_point" => {
                    let noise = Perlin::new(&mut random, row["count"].as_u64().unwrap() as usize);
                    row["points"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|p| noise.sample(p[0].as_f64().unwrap(), p[1].as_f64().unwrap()))
                        .collect()
                }
                "perlin_region" => {
                    let noise = Perlin::new(&mut random, row["count"].as_u64().unwrap() as usize);
                    noise.generate(
                        floats(row, "origin"),
                        sizes(row),
                        floats(row, "scale"),
                        row["lacunarity"].as_f64().unwrap(),
                        row["persistence"].as_f64().unwrap(),
                    )
                }
                _ => panic!("Unknown fixture kind {kind}"),
            };
            if let Some(expected) = row["next_long"].as_str() {
                assert_eq!(
                    random.next_long().to_string(),
                    expected,
                    "RNG consumption after {kind}"
                );
            }
            let expected = row["bits"].as_array().unwrap();
            assert_eq!(actual.len(), expected.len());
            for (i, (actual, expected)) in actual.iter().zip(expected).enumerate() {
                let bits = u64::from_str_radix(expected.as_str().unwrap(), 16).unwrap();
                assert_eq!(
                    actual.to_bits(),
                    bits,
                    "seed={} {kind} index={i}: native={actual:?}, java={:?}",
                    seed(row),
                    f64::from_bits(bits)
                );
                compared += 1;
            }
        }
        assert_eq!(compared, 18_361);
    }
}
