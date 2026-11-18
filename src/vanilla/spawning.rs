//! Original world-generation CREATURE placement and the two shared RNG streams.
//! Entity-local construction/initialization remains a separate simulation stage.
use super::{material, random::JavaRandom};
#[path = "spawning_math.rs"]
mod strict;
use anyhow::{bail, ensure, Result};
use glam::IVec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AnimalKind {
    Sheep,
    Rabbit,
    Pig,
    Chicken,
    Cow,
    Wolf,
    #[serde(rename = "EntityHorse")]
    Horse,
    #[serde(rename = "MushroomCow")]
    Mooshroom,
}
#[derive(Clone, Copy, Debug)]
pub struct SpawnEntry {
    pub kind: AnimalKind,
    pub weight: i32,
    pub min: i32,
    pub max: i32,
}
include!("spawning_data.rs");

#[derive(Clone, Copy, Debug)]
pub struct SpawnArea {
    pub x: i32,
    pub z: i32,
    pub width: i32,
    pub depth: i32,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AnimalSpawn {
    pub kind: AnimalKind,
    pub position: [f64; 3],
    pub yaw: f32,
    /// Preserve group boundaries for later horse/rabbit entity-local initialization.
    pub group: u32,
    /// Already consumed from world.rand; do not roll it again during initialization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheep_color: Option<u8>,
}

/// Separate from population's feature adapter to support eventful provider loads.
/// A backend must load missing chunks or return an error, never implicit air.
pub trait SpawnWorld {
    fn get(&mut self, p: IVec3) -> Result<u16>;
    fn top_solid_or_liquid(&mut self, x: i32, z: i32) -> Result<IVec3>;
    fn inside_border(&self, p: IVec3) -> bool;
}

pub fn spawn_list(biome: u8) -> Option<(f32, &'static [SpawnEntry])> {
    SPAWN_LISTS
        .iter()
        .find(|(id, _, _)| *id == biome)
        .map(|(_, chance, list)| (*chance, *list))
}
/// World.doesBlockHaveSolidTopSurface, including its unusual snow-layer check.
pub fn solid_top(state: u16) -> bool {
    if material::opaque(state) && material::full_cube(state) {
        return true;
    }
    match state >> 4 {
        53 | 67 | 108 | 109 | 114 | 128 | 134..=136 | 156 | 163 | 164 | 180 => state & 4 != 0,
        44 | 126 | 182 => state & 8 != 0,
        154 => true,
        78 => state & 7 == 6,
        _ => false,
    }
}
pub fn can_spawn(world: &mut impl SpawnWorld, p: IVec3, in_water: bool) -> Result<bool> {
    if !world.inside_border(p) {
        return Ok(false);
    }
    let state = world.get(p)?;
    if in_water {
        return Ok(material::is_liquid(state)
            && material::is_liquid(world.get(p - IVec3::Y)?)
            && !material::normal_cube(world.get(p + IVec3::Y)?));
    }
    if !solid_top(world.get(p - IVec3::Y)?) {
        return Ok(false);
    }
    let below = world.get(p - IVec3::Y)?;
    Ok(!matches!(below >> 4, 7 | 166)
        && !material::normal_cube(state)
        && !material::is_liquid(state)
        && !material::normal_cube(world.get(p + IVec3::Y)?))
}
pub fn random_sheep_color(world_random: &mut JavaRandom) -> u8 {
    let roll = world_random.next_int(100);
    if roll < 5 {
        15
    } else if roll < 10 {
        7
    } else if roll < 15 {
        8
    } else if roll < 18 {
        12
    } else if world_random.next_int(500) == 0 {
        6
    } else {
        0
    }
}

/// Exact placement phase of SpawnerAnimals.performWorldGenSpawning.
/// Species and sheep fleece use world_random; the supplied population Random
/// controls all group/position/yaw attempts. Original Z retry bound uses width.
pub fn perform_worldgen_spawning(
    world: &mut impl SpawnWorld,
    biome: u8,
    area: SpawnArea,
    population_random: &mut JavaRandom,
    world_random: &mut JavaRandom,
) -> Result<Vec<AnimalSpawn>> {
    ensure!(
        area.width > 0 && area.depth > 0,
        "Spawn region dimensions must be positive"
    );
    let Some((chance, list)) = spawn_list(biome) else {
        bail!("Unknown original biome {biome}")
    };
    let mut result = Vec::new();
    if list.is_empty() {
        return Ok(result);
    }
    let weight: i32 = list.iter().map(|entry| entry.weight).sum();
    let mut group = 0;
    while population_random.next_float() < chance {
        let mut choice = world_random.next_int(weight);
        let entry = list
            .iter()
            .find(|entry| {
                choice -= entry.weight;
                choice < 0
            })
            .unwrap();
        let count = entry.min + population_random.next_int(1 + entry.max - entry.min);
        let mut x = area.x.wrapping_add(population_random.next_int(area.width));
        let mut z = area.z.wrapping_add(population_random.next_int(area.depth));
        let anchor_x = x;
        let anchor_z = z;
        for _ in 0..count {
            for _ in 0..4 {
                let p = world.top_solid_or_liquid(x, z)?;
                let success = can_spawn(world, p, false)?;
                if success {
                    let yaw = population_random.next_float() * 360.;
                    let sheep_color = if entry.kind == AnimalKind::Sheep {
                        Some(random_sheep_color(world_random))
                    } else {
                        None
                    };
                    result.push(AnimalSpawn {
                        kind: entry.kind,
                        position: [(x as f32 + 0.5) as f64, p.y as f64, (z as f32 + 0.5) as f64],
                        yaw,
                        group,
                        sheep_color,
                    });
                }
                x = x.wrapping_add(population_random.next_int(5) - population_random.next_int(5));
                z = z.wrapping_add(population_random.next_int(5) - population_random.next_int(5));
                while x < area.x
                    || x >= area.x.wrapping_add(area.width)
                    || z < area.z
                    || z >= area.z.wrapping_add(area.width)
                {
                    x = anchor_x.wrapping_add(
                        population_random.next_int(5) - population_random.next_int(5),
                    );
                    z = anchor_z.wrapping_add(
                        population_random.next_int(5) - population_random.next_int(5),
                    );
                }
                if success {
                    break;
                }
            }
        }
        group += 1;
    }
    Ok(result)
}

/// Entity-local Random starts independently of both worldgen streams. A runtime
/// supplies the post-constructor state; Java's constructor seeds from time and
/// consumes UUID/AI-constructor draws, so seed alone cannot reproduce it.
#[derive(Debug, Clone)]
pub struct EntityRandom {
    pub random: JavaRandom,
    gaussian: Option<f64>,
}
impl EntityRandom {
    pub fn new(seed: i64) -> Self {
        Self {
            random: JavaRandom::new(seed),
            gaussian: None,
        }
    }
    pub fn from_state(state: u64, gaussian: Option<f64>) -> Self {
        Self {
            random: JavaRandom::from_state(state),
            gaussian,
        }
    }
    pub fn state(&self) -> (u64, Option<f64>) {
        (self.random.state(), self.gaussian)
    }
    pub fn next_gaussian(&mut self) -> f64 {
        if let Some(value) = self.gaussian.take() {
            return value;
        }
        loop {
            let a = 2. * self.random.next_double() - 1.;
            let b = 2. * self.random.next_double() - 1.;
            let s = a * a + b * b;
            if s >= 1. || s == 0. {
                continue;
            }
            let multiplier = (-2. * strict::log(s) / s).sqrt();
            self.gaussian = Some(b * multiplier);
            return a * multiplier;
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum AnimalGroup {
    Rabbit(u8),
    Horse { kind: u8, variant: u16 },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct HorseTraits {
    pub kind: u8,
    pub variant: u16,
    pub max_health: f64,
    pub movement_speed: f64,
    pub jump_strength: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AnimalTraits {
    pub growing_age: i32,
    pub follow_range_bonus: f64,
    pub rabbit_type: Option<u8>,
    pub horse: Option<HorseTraits>,
}
/// EntityLiving.onInitialSpawn and CREATURE subclasses, given the entity-local
/// RNG at the original callback boundary. Sheep fleece was consumed earlier by
/// perform_worldgen_spawning from world.rand and is already in AnimalSpawn.
pub fn initialize_animal(
    kind: AnimalKind,
    random: &mut EntityRandom,
    group: &mut Option<AnimalGroup>,
) -> AnimalTraits {
    let mut result = AnimalTraits {
        growing_age: 0,
        follow_range_bonus: random.next_gaussian() * 0.05,
        rabbit_type: None,
        horse: None,
    };
    let random = &mut random.random;
    match kind {
        AnimalKind::Rabbit => {
            let mut kind = random.next_int(6) as u8;
            if let Some(AnimalGroup::Rabbit(shared)) = group {
                kind = *shared;
                result.growing_age = -24000;
            } else {
                *group = Some(AnimalGroup::Rabbit(kind));
            }
            result.rabbit_type = Some(kind);
        }
        AnimalKind::Horse => {
            let (kind, variant) = if let Some(AnimalGroup::Horse { kind, variant }) = group {
                (*kind, (*variant & 255) | ((random.next_int(5) as u16) << 8))
            } else {
                let pair = if random.next_int(10) == 0 {
                    (1, 0)
                } else {
                    (
                        0,
                        random.next_int(7) as u16 | ((random.next_int(5) as u16) << 8),
                    )
                };
                *group = Some(AnimalGroup::Horse {
                    kind: pair.0,
                    variant: pair.1,
                });
                pair
            };
            if random.next_int(5) == 0 {
                result.growing_age = -24000;
            }
            let (max_health, movement_speed) = if matches!(kind, 3 | 4) {
                (15., 0.2f32 as f64)
            } else {
                let health = (15f32 + random.next_int(8) as f32 + random.next_int(9) as f32) as f64;
                let speed = if kind == 0 {
                    (0.45f32 as f64
                        + random.next_double() * 0.3
                        + random.next_double() * 0.3
                        + random.next_double() * 0.3)
                        * 0.25
                } else {
                    0.175f32 as f64
                };
                (health, speed)
            };
            let jump_strength = if matches!(kind, 1 | 2) {
                0.5
            } else {
                0.4f32 as f64
                    + random.next_double() * 0.2
                    + random.next_double() * 0.2
                    + random.next_double() * 0.2
            };
            result.horse = Some(HorseTraits {
                kind,
                variant,
                max_health,
                movement_speed,
                jump_strength,
            });
        }
        _ => {}
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    struct FixtureWorld {
        scenario: i32,
    }
    impl SpawnWorld for FixtureWorld {
        fn get(&mut self, p: IVec3) -> Result<u16> {
            let IVec3 { x, y, z } = p;
            Ok(if !(0..256).contains(&y) || self.scenario == 7 {
                0
            } else if y < 63 {
                16
            } else if y == 63 {
                if self.scenario == 2 {
                    7 << 4
                } else if self.scenario == 4 {
                    85 << 4
                } else {
                    2 << 4
                }
            } else if y == 64
                && (self.scenario == 1 || self.scenario == 3 && (x + z).rem_euclid(3) == 0)
            {
                9 << 4
            } else if y == 64 && self.scenario == 3 && (x + z).rem_euclid(5) == 0 {
                18 << 4
            } else {
                0
            })
        }
        fn top_solid_or_liquid(&mut self, x: i32, z: i32) -> Result<IVec3> {
            for y in (0..256).rev() {
                let state = self.get(IVec3::new(x, y, z))?;
                if material::blocks_movement(state) && !material::is_leaves(state) {
                    return Ok(IVec3::new(x, y + 1, z));
                }
            }
            Ok(IVec3::new(x, -1, z))
        }
        fn inside_border(&self, p: IVec3) -> bool {
            let (min, max) = if self.scenario == 6 {
                (12, 20)
            } else {
                (-29999984, 29999984)
            };
            p.x.wrapping_add(1) > min && p.x < max && p.z.wrapping_add(1) > min && p.z < max
        }
    }
    #[test]
    fn worldgen_animals_and_both_shared_rngs_match_original_bytecode() {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-spawning.json"
        ))
        .unwrap();
        let mut count = 0;
        for (index, row) in data["cases"].as_array().unwrap().iter().enumerate() {
            let mut population_random =
                JavaRandom::new(row["population_seed"].as_str().unwrap().parse().unwrap());
            let mut world_random =
                JavaRandom::new(row["world_seed"].as_str().unwrap().parse().unwrap());
            let mut world = FixtureWorld {
                scenario: row["scenario"].as_i64().unwrap() as i32,
            };
            let a = &row["area"];
            let area = SpawnArea {
                x: a[0].as_i64().unwrap() as i32,
                z: a[1].as_i64().unwrap() as i32,
                width: a[2].as_i64().unwrap() as i32,
                depth: a[3].as_i64().unwrap() as i32,
            };
            let events = perform_worldgen_spawning(
                &mut world,
                row["biome"].as_u64().unwrap() as u8,
                area,
                &mut population_random,
                &mut world_random,
            )
            .unwrap();
            let actual:Vec<_>=events.iter().map(|e|{
                let mut value=serde_json::json!({"kind":e.kind,"position":e.position,"yaw_bits":e.yaw.to_bits(),"group":e.group});
                if let Some(color)=e.sheep_color{value["sheep_color"]=serde_json::json!(color);}value
            }).collect();
            assert_eq!(
                serde_json::json!(actual),
                row["events"],
                "case{index} spawn order/kind/location/yaw/group/wool"
            );
            assert_eq!(
                population_random.next_long().to_string(),
                row["population_tail"].as_str().unwrap(),
                "case{index} population RNG"
            );
            assert_eq!(
                world_random.next_long().to_string(),
                row["world_tail"].as_str().unwrap(),
                "case{index} world RNG"
            );
            count += events.len();
        }
        assert_eq!(data["cases"].as_array().unwrap().len(), 5124);
        assert_eq!(count, 3546);
    }
    #[test]
    fn entity_local_initialization_matches_original_callback() {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-spawning.json"
        ))
        .unwrap();
        for row in data["initializations"].as_array().unwrap() {
            let kind: AnimalKind = serde_json::from_value(row["kind"].clone()).unwrap();
            let mut world_random =
                JavaRandom::new(row["world_seed"].as_str().unwrap().parse().unwrap());
            let mut group = None;
            for event in row["events"].as_array().unwrap() {
                let seed = event["seed"].as_str().unwrap().parse().unwrap();
                let mut random = EntityRandom::new(seed);
                let traits = initialize_animal(kind, &mut random, &mut group);
                let mut actual = serde_json::json!({"seed":seed.to_string(),"age":traits.growing_age,"follow_bits":traits.follow_range_bonus.to_bits().to_string()});
                if kind == AnimalKind::Sheep {
                    actual["sheep_color"] =
                        serde_json::json!(random_sheep_color(&mut world_random));
                }
                if let Some(rabbit) = traits.rabbit_type {
                    actual["rabbit_type"] = serde_json::json!(rabbit);
                }
                if let Some(horse) = traits.horse {
                    for (key, value) in [
                        ("horse_type", serde_json::json!(horse.kind)),
                        ("horse_variant", serde_json::json!(horse.variant)),
                        (
                            "health_bits",
                            serde_json::json!(horse.max_health.to_bits().to_string()),
                        ),
                        (
                            "speed_bits",
                            serde_json::json!(horse.movement_speed.to_bits().to_string()),
                        ),
                        (
                            "jump_bits",
                            serde_json::json!(horse.jump_strength.to_bits().to_string()),
                        ),
                    ] {
                        actual[key] = value;
                    }
                }
                actual["cached_gaussian_bits"] =
                    serde_json::json!(random.next_gaussian().to_bits().to_string());
                actual["tail"] = serde_json::json!(random.random.next_long().to_string());
                assert_eq!(&actual, event, "{kind:?}, entity-local seed{seed}");
            }
            assert_eq!(
                world_random.next_long().to_string(),
                row["world_tail"].as_str().unwrap()
            );
        }
        assert_eq!(data["initializations"].as_array().unwrap().len(), 512);
    }
    #[test]
    fn every_original_block_state_spawn_predicate_matches() {
        struct PredicateWorld([u16; 3]);
        impl SpawnWorld for PredicateWorld {
            fn get(&mut self, p: IVec3) -> Result<u16> {
                Ok(self.0[(p.y - 63) as usize])
            }
            fn top_solid_or_liquid(&mut self, _: i32, _: i32) -> Result<IVec3> {
                unreachable!()
            }
            fn inside_border(&self, _: IVec3) -> bool {
                true
            }
        }
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-spawning.json"
        ))
        .unwrap();
        let mut checks = 0;
        for row in data["predicates"].as_array().unwrap() {
            let state = row[0].as_u64().unwrap() as u16;
            assert_eq!(
                solid_top(state),
                row[1].as_bool().unwrap(),
                "solid top {state}"
            );
            for check in row[2].as_array().unwrap() {
                let mut world = PredicateWorld(std::array::from_fn(|i| {
                    check[0][i].as_u64().unwrap() as u16
                }));
                for (i, in_water) in [false, true].into_iter().enumerate() {
                    assert_eq!(
                        can_spawn(&mut world, IVec3::new(0, 64, 0), in_water).unwrap(),
                        check[i + 1].as_bool().unwrap(),
                        "state{state} {:?} in_water={in_water}",
                        world.0
                    );
                    checks += 1;
                }
            }
        }
        assert!(checks > 10000);
    }
}
