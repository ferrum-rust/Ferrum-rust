//! Biome decoration up to the original immediate spring updates.
//! This is an explicit partial phase, not a replacement for full population.
use super::{
    vegetation::{self, BigMushroom, BigTree},
    FeatureWorld,
};
use crate::vanilla::{noise::Perlin, random::JavaRandom};
use anyhow::{ensure, Result};
use glam::IVec3;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Decorator {
    big_oaks: BTreeMap<u8, BigTree>,
    giant_mushrooms: BTreeMap<u8, BigMushroom>,
    plains_counts: BTreeMap<u8, (i32, i32)>,
}
fn grass_noise(x: f64, z: f64) -> f64 {
    static NOISE: std::sync::OnceLock<Perlin> = std::sync::OnceLock::new();
    NOISE
        .get_or_init(|| Perlin::new(&mut JavaRandom::new(2345), 1))
        .sample(x, z)
}
fn xz(random: &mut JavaRandom, origin: IVec3) -> IVec3 {
    origin + IVec3::new(random.next_int(16) + 8, 0, random.next_int(16) + 8)
}
fn random_height(
    world: &impl FeatureWorld,
    random: &mut JavaRandom,
    origin: IVec3,
    double: bool,
) -> Option<IVec3> {
    let mut p = xz(random, origin);
    let h = world.height(p.x, p.z);
    let bound = if double { h * 2 } else { h + 32 };
    if bound <= 0 {
        return None;
    }
    p.y = random.next_int(bound);
    Some(p)
}
fn flower(random: &mut JavaRandom, id: u8, p: IVec3) -> u16 {
    if id == 6 {
        return (38 << 4) | 1;
    }
    if matches!(id, 1 | 129) {
        if grass_noise(p.x as f64 / 200., p.z as f64 / 200.) < -0.8 {
            return (38 << 4) | [5, 4, 7, 6][random.next_int(4) as usize];
        }
        if random.next_int(3) > 0 {
            return (38 << 4) | [0, 3, 8][random.next_int(3) as usize];
        }
        return 37 << 4;
    }
    if id == 132 {
        let value = ((1. + grass_noise(p.x as f64 / 48., p.z as f64 / 48.)) / 2.).clamp(0., 0.9999);
        let index = (value * 10.) as usize;
        return [
            37 << 4,
            38 << 4,
            38 << 4,
            (38 << 4) | 2,
            (38 << 4) | 3,
            (38 << 4) | 4,
            (38 << 4) | 5,
            (38 << 4) | 6,
            (38 << 4) | 7,
            (38 << 4) | 8,
        ][index];
    }
    if random.next_int(3) > 0 {
        37 << 4
    } else {
        38 << 4
    }
}
impl Decorator {
    pub fn supports(id: u8) -> bool {
        matches!(id,0..=39|129..=134|140|149|151|155..=158|160..=167)
    }
    pub fn generates_springs(id: u8) -> bool {
        id != 9
    }
    /// BiomeGenMutated delegates to the base decorator, except concrete mutation
    /// subclasses which construct/use their own decorator.
    pub fn decorator_owner(id: u8) -> u8 {
        match id {
            130 => 2,
            133 => 5,
            134 => 6,
            149 => 21,
            151 => 23,
            155 => 27,
            156 => 28,
            157 => 29,
            158 => 30,
            _ => id,
        }
    }
    /// Runs subclass work before BiomeDecorator's Already decorating guard.
    /// Call this before acquiring the decorator-owner guard; a recursive prefix
    /// can run (and change its settings) before the original detects reentry.
    pub fn decorate_prefix(
        &mut self,
        world: &mut impl FeatureWorld,
        random: &mut JavaRandom,
        origin: IVec3,
        id: u8,
    ) -> Result<()> {
        ensure!(
            Self::supports(id),
            "Native decoration not implemented for biome {id}"
        );
        let id = if id == 157 { 29 } else { id };
        let forest = matches!(id, 4 | 18 | 27 | 28 | 29 | 132);
        let plains = matches!(id, 1 | 129);
        let mega = matches!(id, 32 | 33 | 160 | 161);
        if plains {
            if grass_noise((origin.x + 8) as f64 / 200., (origin.z + 8) as f64 / 200.) < -0.8 {
                self.plains_counts.insert(id, (15, 5));
                world.set_plains_counts(id, 15, 5);
            } else {
                self.plains_counts.insert(id, (4, 10));
                world.set_plains_counts(id, 4, 10);
                world.set_double_plant_type(2);
                for _ in 0..7 {
                    let p = random_height(world, random, origin, false)
                        .expect("Original plains requires positive height+32");
                    vegetation::double_plant(world, random, p, 2);
                }
            }
            if id == 129 {
                world.set_double_plant_type(0);
                for _ in 0..10 {
                    let p = random_height(world, random, origin, false)
                        .expect("Original plains requires positive height+32");
                    vegetation::double_plant(world, random, p, 0);
                }
            }
        }
        if id == 29 {
            for x in 0..4 {
                for z in 0..4 {
                    let mut p = origin
                        + IVec3::new(
                            x * 4 + 9 + random.next_int(3),
                            0,
                            z * 4 + 9 + random.next_int(3),
                        );
                    p.y = world.height(p.x, p.z);
                    if random.next_int(20) == 0 {
                        BigMushroom::default().generate(world, random, p);
                    } else if random.next_int(3) > 0 {
                        vegetation::dark_oak(world, random, p);
                    } else if random.next_int(5) == 0 {
                        vegetation::birch(world, random, p, false);
                    } else {
                        vegetation::tree(world, random, p, 4, 17 << 4, 18 << 4, false);
                    }
                }
            }
        }
        if forest {
            let count = random.next_int(5) - 3 + if id == 132 { 2 } else { 0 };
            for _ in 0..count {
                let plant = [1, 4, 5][random.next_int(3) as usize];
                world.set_double_plant_type(plant);
                for _ in 0..5 {
                    let p = random_height(world, random, origin, false)
                        .expect("Original forest requires positive height+32");
                    if vegetation::double_plant(world, random, p, plant) {
                        break;
                    }
                }
            }
        }
        if mega {
            let count = random.next_int(3);
            for _ in 0..count {
                let mut p = xz(random, origin);
                p.y = world.height(p.x, p.z);
                vegetation::block_blob(world, random, p);
            }
        }
        if matches!(id, 35 | 36 | 5 | 19 | 30 | 31 | 32 | 33 | 160 | 161) {
            let meta = if matches!(id, 35 | 36) { 2 } else { 3 };
            world.set_double_plant_type(meta);
            for _ in 0..7 {
                let p = random_height(world, random, origin, false).unwrap();
                vegetation::double_plant(world, random, p, meta);
            }
        }
        if id == 140 {
            for _ in 0..3 {
                let mut p = xz(random, origin);
                p.y = world.height(p.x, p.z);
                vegetation::ice_spike(world, random, p);
            }
            for _ in 0..2 {
                let mut p = xz(random, origin);
                p.y = world.height(p.x, p.z);
                vegetation::ice_path(world, random, p);
            }
        }
        Ok(())
    }
    /// Convenience wrapper for isolated, non-reentrant feature validation.
    pub fn decorate_before_springs(
        &mut self,
        world: &mut impl FeatureWorld,
        random: &mut JavaRandom,
        origin: IVec3,
        id: u8,
    ) -> Result<()> {
        self.decorate_prefix(world, random, origin, id)?;
        self.decorate_body_before_springs(world, random, origin, id)
    }
    /// Returns an error before consuming Random or editing blocks for unsupported
    /// biomes. The successful result still REQUIRES spring, spawning and snow
    /// stages before marking a chunk fully populated.
    pub fn decorate_body_before_springs(
        &mut self,
        world: &mut impl FeatureWorld,
        random: &mut JavaRandom,
        origin: IVec3,
        id: u8,
    ) -> Result<()> {
        ensure!(
            Self::supports(id),
            "Native decoration not implemented for biome {id}"
        );
        if id == 9 {
            world.generate_default_ores(random, origin);
            if random.next_int(5) == 0 {
                let mut p = xz(random, origin);
                p.y = world.top_solid(p.x, p.z);
                vegetation::end_spike(world, random, p);
            }
            if origin.x == 0 && origin.z == 0 {
                world.entity(super::GeneratedEntity {
                    kind: super::GeneratedEntityKind::EnderDragon,
                    position: [0., 128., 0.],
                    yaw: random.next_float() * 360.,
                });
            }
            return Ok(());
        }
        let id = if id == 157 { 29 } else { id };
        let forest = matches!(id, 4 | 18 | 27 | 28 | 29 | 132);
        let plains = matches!(id, 1 | 129);
        let desert = matches!(id, 2 | 17 | 130);
        let taiga = matches!(id, 5 | 19 | 30 | 31 | 32 | 33 | 133 | 158 | 160 | 161);
        let mega = matches!(id, 32 | 33 | 160 | 161);
        let jungle = matches!(id, 21..=23 | 149 | 151);
        let hills = matches!(id, 3 | 20 | 34 | 131 | 162);
        let savanna = matches!(id, 35 | 36 | 163 | 164);
        let mesa = matches!(id,37..=39|165..=167);
        let swamp = matches!(id, 6 | 134);
        let mushroom_island = matches!(id, 14 | 15);
        let (mut trees, mut flowers, mut grass) = if id == 132 {
            (6, 100, 1)
        } else if id == 29 {
            (-999, 2, 2)
        } else if forest || matches!(id, 155 | 156) {
            (10, 2, 2)
        } else if plains {
            (-999, 4, 10)
        } else if desert || matches!(id, 16 | 25 | 26) {
            (-999, 2, 1)
        } else if swamp {
            (2, 1, 5)
        } else if mushroom_island {
            (-100, -100, -100)
        } else if mega {
            (10, 2, 7)
        } else if jungle {
            (if matches!(id, 23 | 151) { 2 } else { 50 }, 4, 25)
        } else if taiga {
            (10, 2, 1)
        } else if matches!(id, 20 | 34) {
            (3, 2, 1)
        } else if matches!(id, 35 | 36) {
            (1, 4, 20)
        } else if matches!(id, 163 | 164) {
            (2, 2, 5)
        } else if mesa {
            (if matches!(id, 38 | 166) { 5 } else { -999 }, 0, 1)
        } else {
            (0, 2, 1)
        };
        let dead_bushes = if swamp || mega {
            1
        } else if desert {
            2
        } else if mesa {
            20
        } else {
            0
        };
        let mushrooms = if mega {
            3
        } else if swamp {
            8
        } else if taiga || mushroom_island {
            1
        } else {
            0
        };
        let reeds = if swamp {
            10
        } else if desert {
            50
        } else if mesa {
            3
        } else {
            0
        };
        let cacti = if desert {
            10
        } else if mesa {
            5
        } else {
            0
        };
        if plains {
            (flowers, grass) =
                world.plains_counts(id, self.plains_counts.get(&id).copied().unwrap_or((4, 10)));
        }
        world.generate_default_ores(random, origin);
        for (count, state, size, clay) in [
            (if swamp { 0 } else { 3 }, 12 << 4, 7, false),
            (1, 82 << 4, 4, true),
            (if swamp { 0 } else { 1 }, 13 << 4, 6, false),
        ] {
            for _ in 0..count {
                let mut p = xz(random, origin);
                p.y = world.top_solid(p.x, p.z);
                vegetation::sediment(world, random, p, state, size, clay);
            }
        }
        if random.next_int(10) == 0 {
            trees += 1;
        }
        for _ in 0..trees {
            let mut p = xz(random, origin);
            let mut jungle_height = 4;
            // Selection precedes the height query in the original decorator.
            let species = if matches!(id, 155 | 156) {
                if random.next_boolean() {
                    2
                } else {
                    1
                }
            } else if forest {
                if matches!(id, 27 | 28) || random.next_int(5) == 0 {
                    1
                } else {
                    0
                }
            } else if swamp {
                7
            } else if mega && random.next_int(3) == 0 {
                if matches!(id, 160 | 161) || random.next_int(13) == 0 {
                    9
                } else {
                    8
                }
            } else if taiga {
                if random.next_int(3) == 0 {
                    4
                } else {
                    5
                }
            } else if jungle {
                if random.next_int(10) == 0 {
                    3
                } else if random.next_int(2) == 0 {
                    11
                } else if !matches!(id, 23 | 151) && random.next_int(3) == 0 {
                    12
                } else {
                    jungle_height = 4 + random.next_int(7);
                    10
                }
            } else if savanna {
                if random.next_int(5) > 0 {
                    6
                } else {
                    0
                }
            } else if mesa {
                0
            } else if matches!(id, 12 | 13 | 140) || hills && random.next_int(3) > 0 {
                5
            } else if random.next_int(10) == 0 {
                3
            } else {
                0
            };
            p.y = world.height(p.x, p.z);
            match species {
                1 => {
                    vegetation::birch(world, random, p, false);
                }
                2 => {
                    vegetation::birch(world, random, p, true);
                }
                3 => {
                    let generator_id = match id {
                        149 => 21,
                        151 => 23,
                        _ => id,
                    };
                    let tree = self.big_oaks.entry(generator_id).or_default();
                    tree.prepare();
                    tree.generate(world, random, p);
                }
                4 => {
                    vegetation::pine(world, random, p);
                }
                5 => {
                    vegetation::spruce(world, random, p);
                }
                6 => {
                    vegetation::acacia(world, random, p);
                }
                7 => {
                    vegetation::swamp_tree(world, random, p);
                }
                8 | 9 => {
                    if vegetation::mega_pine(world, random, p, species == 9) {
                        vegetation::mega_pine_soil(world, random, p);
                    }
                }
                10 => {
                    vegetation::tree(
                        world,
                        random,
                        p,
                        jungle_height,
                        (17 << 4) | 3,
                        (18 << 4) | 3,
                        true,
                    );
                }
                11 => {
                    vegetation::shrub(world, random, p);
                }
                12 => {
                    vegetation::mega_jungle(world, random, p);
                }
                _ => {
                    vegetation::tree(world, random, p, 4, 17 << 4, 18 << 4, false);
                }
            }
        }
        if mushroom_island {
            let mut p = xz(random, origin);
            p.y = world.height(p.x, p.z);
            self.giant_mushrooms
                .entry(id)
                .or_default()
                .generate(world, random, p);
        }
        for _ in 0..flowers {
            if let Some(p) = random_height(world, random, origin, false) {
                let state = flower(random, id, p);
                vegetation::flowers(world, random, p, state);
            }
        }
        for _ in 0..grass {
            if let Some(p) = random_height(world, random, origin, true) {
                let meta = if matches!(id, 5 | 19 | 30 | 31 | 32 | 33 | 160 | 161)
                    && random.next_int(5) > 0
                    || matches!(id, 21..=23) && random.next_int(4) == 0
                {
                    2
                } else {
                    1
                };
                vegetation::tall_grass(world, random, p, meta);
            }
        }
        for _ in 0..dead_bushes {
            if let Some(p) = random_height(world, random, origin, true) {
                vegetation::dead_bush(world, random, p);
            }
        }
        if swamp {
            for _ in 0..4 {
                if let Some(mut p) = random_height(world, random, origin, true) {
                    while p.y > 0 && world.get(p - IVec3::Y) == 0 {
                        p.y -= 1;
                    }
                    vegetation::waterlily(world, random, p);
                }
            }
        }
        for _ in 0..mushrooms {
            if random.next_int(4) == 0 {
                let mut p = xz(random, origin);
                p.y = world.height(p.x, p.z);
                vegetation::mushroom(world, random, p, 39 << 4);
            }
            if random.next_int(8) == 0 {
                if let Some(p) = random_height(world, random, origin, true) {
                    vegetation::mushroom(world, random, p, 40 << 4);
                }
            }
        }
        for (chance, state) in [(4, 39 << 4), (8, 40 << 4)] {
            if random.next_int(chance) == 0 {
                if let Some(p) = random_height(world, random, origin, true) {
                    vegetation::mushroom(world, random, p, state);
                }
            }
        }
        for _ in 0..reeds + 10 {
            if let Some(p) = random_height(world, random, origin, true) {
                vegetation::reeds(world, random, p);
            }
        }
        if random.next_int(32) == 0 {
            if let Some(p) = random_height(world, random, origin, true) {
                vegetation::pumpkin(world, random, p);
            }
        }
        for _ in 0..cacti {
            if let Some(p) = random_height(world, random, origin, true) {
                vegetation::cactus(world, random, p);
            }
        }
        Ok(())
    }

    /// Biome subclass suffix. Release the decorator-owner guard after the spring
    /// stage and BEFORE calling this, matching the return from super.decorate.
    pub fn decorate_after_springs(
        &mut self,
        world: &mut impl FeatureWorld,
        random: &mut JavaRandom,
        origin: IVec3,
        id: u8,
    ) -> Result<()> {
        ensure!(
            Self::supports(id),
            "Native decoration not implemented for biome {id}"
        );
        if matches!(id, 2 | 17) && random.next_int(1000) == 0 {
            let mut p = xz(random, origin);
            p.y = world.height(p.x, p.z) + 1;
            vegetation::desert_well(world, p);
        }
        if matches!(id, 3 | 20 | 34 | 131 | 162) {
            let count = 3 + random.next_int(6);
            for _ in 0..count {
                let p = origin
                    + IVec3::new(
                        random.next_int(16),
                        random.next_int(28) + 4,
                        random.next_int(16),
                    );
                if world.get(p) >> 4 == 1 {
                    world.set(p, 129 << 4);
                }
            }
            for _ in 0..7 {
                let p = origin
                    + IVec3::new(
                        random.next_int(16),
                        random.next_int(64),
                        random.next_int(16),
                    );
                super::minable(world, random, p, 97 << 4, 9);
            }
        }
        if matches!(id, 21..=23) {
            let mut p = xz(random, origin);
            p.y = random.next_int(world.height(p.x, p.z) * 2);
            vegetation::melon(world, random, p);
            for _ in 0..50 {
                let mut p = xz(random, origin);
                p.y = 128;
                vegetation::vines(world, random, p);
            }
        }
        Ok(())
    }
}
