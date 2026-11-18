//! Deterministic Ferrum generation. This is not Java's seed-compatible generator.
use crate::{
    container::{Container, Kind},
    inventory::Stack,
    world::{terrain_height, Chunk, ChunkPos},
};
use glam::{IVec3, Vec3};

pub fn hash(x: i32, z: i32, seed: u64) -> u64 {
    let mut v = (x as u64).wrapping_mul(0x9e3779b97f4a7c15)
        ^ (z as u64).wrapping_mul(0xbf58476d1ce4e5b9)
        ^ seed;
    v = (v ^ (v >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    v = (v ^ (v >> 27)).wrapping_mul(0x94d049bb133111eb);
    v ^ (v >> 31)
}
fn random(state: &mut u64) -> f32 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    (*state >> 40) as f32 / 16777216.0
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Village {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub id: u64,
}
pub fn village(rx: i32, rz: i32, seed: u64) -> Option<Village> {
    let h = hash(rx, rz, seed ^ 0x76696c6c616765);
    let x = rx * 256 + 64 + (h % 128) as i32;
    let z = rz * 256 + 64 + ((h >> 16) % 128) as i32;
    let y = terrain_height(x, z, seed) + 1;
    (y > 53).then_some(Village {
        x,
        y,
        z,
        id: h | (1 << 63),
    })
}
pub fn nearest_village(seed: u64) -> Village {
    (-4..=4)
        .flat_map(|z| (-4..=4).filter_map(move |x| village(x, z, seed)))
        .min_by_key(|v| v.x as i64 * v.x as i64 + v.z as i64 * v.z as i64)
        .expect("Ferrum terrain has land near the origin")
}
pub fn spawn(seed: u64, near_village: bool) -> Vec3 {
    if near_village {
        let v = nearest_village(seed);
        Vec3::new(v.x as f32 + 5.5, v.y as f32, v.z as f32 + 5.5)
    } else {
        Vec3::new(0.5, terrain_height(0, 0, seed) as f32 + 2.0, 0.5)
    }
}
struct Builder<'a> {
    chunk: &'a mut Chunk,
    pos: ChunkPos,
}
impl Builder<'_> {
    fn set(&mut self, x: i32, y: i32, z: i32, state: u16) {
        let (x, z) = (x - self.pos.0 * 16, z - self.pos.1 * 16);
        if (0..16).contains(&x) && (0..16).contains(&z) && (1..256).contains(&y) {
            self.chunk.set(x, y, z, state);
        }
    }
    fn fill(&mut self, a: IVec3, b: IVec3, state: u16) {
        for z in a.z.max(self.pos.1 * 16)..=b.z.min(self.pos.1 * 16 + 15) {
            for x in a.x.max(self.pos.0 * 16)..=b.x.min(self.pos.0 * 16 + 15) {
                for y in a.y.max(1)..=b.y.min(255) {
                    self.set(x, y, z, state);
                }
            }
        }
    }
    fn chest(&mut self, p: IVec3, loot: &[Stack]) {
        self.set(p.x, p.y, p.z, 54 << 4 | 2);
        if p.x.div_euclid(16) == self.pos.0 && p.z.div_euclid(16) == self.pos.1 {
            let mut c = Container::new(Kind::Chest);
            for (s, item) in c.slots.iter_mut().zip(loot) {
                *s = Some(item.clone());
            }
            self.chunk.containers.push((p, c));
        }
    }
    fn foundation(&mut self, x: i32, z: i32, y: i32, seed: u64, top: u16) {
        let h = terrain_height(x, z, seed);
        for by in h.min(y - 2)..y {
            self.set(x, by, z, if by == y - 1 { top } else { 3 << 4 });
        }
        for by in y..=h.max(y) + 10 {
            self.set(x, by, z, 0);
        }
    }
}
pub fn decorate(chunk: &mut Chunk, pos: ChunkPos, seed: u64) {
    let mut b = Builder { chunk, pos };
    // Every neighboring origin is evaluated in the same order, so tunnels join at chunk boundaries.
    let (cx, cz) = (pos.0 * 16, pos.1 * 16);
    for rz in (cz - 88).div_euclid(64)..=(cz + 103).div_euclid(64) {
        for rx in (cx - 88).div_euclid(64)..=(cx + 103).div_euclid(64) {
            let mut rng = hash(rx, rz, seed ^ 0x63617665);
            let mut p = Vec3::new(
                (rx * 64) as f32 + random(&mut rng) * 64.,
                10. + random(&mut rng) * 34.,
                (rz * 64) as f32 + random(&mut rng) * 64.,
            );
            let entrance = hash(rx, rz, seed).is_multiple_of(3)
                && terrain_height(p.x as i32, p.z as i32, seed) > 55;
            if entrance {
                p.y = terrain_height(p.x as i32, p.z as i32, seed) as f32 - 1.;
            }
            let mut yaw = random(&mut rng) * std::f32::consts::TAU;
            let mut pitch = (random(&mut rng) - 0.5) * 0.3;
            for step in 0..68 {
                yaw += (random(&mut rng) - 0.5) * 0.45;
                pitch = (pitch * 0.8 + (random(&mut rng) - 0.5) * 0.18
                    - if entrance { 0.06 } else { 0. })
                .clamp(-0.45, 0.45);
                p += Vec3::new(yaw.cos(), pitch, yaw.sin()) * 1.25;
                let radius = 1.8 + (step as f32 / 68. * std::f32::consts::PI).sin() * 1.7;
                if p.x + radius < cx as f32
                    || p.x - radius > (cx + 15) as f32
                    || p.z + radius < cz as f32
                    || p.z - radius > (cz + 15) as f32
                {
                    continue;
                }
                for z in (p.z - radius).floor() as i32..=(p.z + radius).ceil() as i32 {
                    for x in (p.x - radius).floor() as i32..=(p.x + radius).ceil() as i32 {
                        let h = terrain_height(x, z, seed);
                        for y in
                            (p.y - radius * 0.7).floor() as i32..=(p.y + radius * 0.7).ceil() as i32
                        {
                            let d = Vec3::new(
                                (x as f32 + 0.5 - p.x) / radius,
                                (y as f32 + 0.5 - p.y) / (radius * 0.7),
                                (z as f32 + 0.5 - p.z) / radius,
                            );
                            if y > 1
                                && y < if entrance && step < 20 && h > 53 {
                                    h + 4
                                } else {
                                    h - 4
                                }
                                && d.length_squared() < 1.
                            {
                                b.set(x, y, z, if y < 10 { 11 << 4 } else { 0 });
                            }
                        }
                    }
                }
            }
        }
    }
    // Underground rooms and timber-supported mineshafts, with persistent chest contents.
    for rz in (cz - 48).div_euclid(128)..=(cz + 63).div_euclid(128) {
        for rx in (cx - 48).div_euclid(128)..=(cx + 63).div_euclid(128) {
            let h = hash(rx, rz, seed ^ 0x64756e67656f6e);
            let p = IVec3::new(
                rx * 128 + 32 + (h % 64) as i32,
                18 + ((h >> 8) % 18) as i32,
                rz * 128 + 32 + ((h >> 16) % 64) as i32,
            );
            b.fill(p - IVec3::new(5, 1, 5), p + IVec3::new(5, 4, 5), 48 << 4);
            b.fill(p - IVec3::new(4, 0, 4), p + IVec3::new(4, 3, 4), 0);
            b.chest(
                p + IVec3::new(-3, 0, 3),
                &[
                    Stack::new(265, 3),
                    Stack::new(297, 2),
                    Stack::new(287, 4),
                    Stack::new(289, 2),
                ],
            );
            // The north wall of the room supports this south-facing torch.
            b.set(p.x, p.y + 2, p.z - 4, (50 << 4) | 3);
            for dz in -1..=1 {
                for dx in -32..=32 {
                    for dy in 0..=2 {
                        b.set(p.x + dx, p.y + dy, p.z + dz, 0);
                    }
                    b.set(p.x + dx, p.y - 1, p.z + dz, 5 << 4);
                    if dx.rem_euclid(6) == 0 {
                        b.set(p.x + dx, p.y + 2, p.z + dz, 5 << 4);
                        if dz != 0 {
                            for dy in 0..2 {
                                b.set(p.x + dx, p.y + dy, p.z + dz, 85 << 4);
                            }
                        }
                    }
                }
            }
        }
    }
    for rz in (cz - 40).div_euclid(256)..=(cz + 55).div_euclid(256) {
        for rx in (cx - 40).div_euclid(256)..=(cx + 55).div_euclid(256) {
            if let Some(v) = village(rx, rz, seed) {
                build_village(&mut b, v, seed);
            }
        }
    }
    b.chunk.top = (0..256)
        .rev()
        .find(|&y| (0..16).any(|z| (0..16).any(|x| b.chunk.get(x, y, z) != 0)))
        .map_or(0, |y| y + 1);
}
fn build_village(b: &mut Builder<'_>, v: Village, seed: u64) {
    // Grade a connected village floor, with a gradual transition to the surrounding terrain.
    // Otherwise hillside houses and even residents can be buried by the ungraded neighbor columns.
    for z in (v.z - 40).max(b.pos.1 * 16)..=(v.z + 40).min(b.pos.1 * 16 + 15) {
        for x in (v.x - 40).max(b.pos.0 * 16)..=(v.x + 40).min(b.pos.0 * 16 + 15) {
            let radius = (((x - v.x).pow(2) + (z - v.z).pow(2)) as f32).sqrt();
            if radius > 40. {
                continue;
            }
            let blend = ((radius - 30.) / 10.).clamp(0., 1.);
            let ground = ((v.y - 1) as f32 * (1. - blend)
                + terrain_height(x, z, seed) as f32 * blend)
                .round() as i32;
            b.foundation(x, z, ground + 1, seed, 2 << 4);
        }
    }
    for z in v.z - 29..=v.z + 29 {
        for x in v.x - 29..=v.x + 29 {
            if (x - v.x).abs() <= 2
                || (z - v.z).abs() <= 2
                || ((x - v.x).abs() <= 6 && (z - v.z).abs() <= 6)
            {
                b.foundation(x, z, v.y, seed, 13 << 4);
            }
        }
    }
    for (dx, dz) in [(-12, -12), (12, -12), (-12, 12), (12, 12)] {
        let (x, z) = (v.x + dx, v.z + dz);
        for bz in z - 4..=z + 4 {
            for bx in x - 4..=x + 4 {
                b.foundation(bx, bz, v.y, seed, 4 << 4);
            }
        }
        b.fill(
            IVec3::new(x - 3, v.y, z - 3),
            IVec3::new(x + 3, v.y + 3, z + 3),
            5 << 4,
        );
        b.fill(
            IVec3::new(x - 2, v.y, z - 2),
            IVec3::new(x + 2, v.y + 2, z + 2),
            0,
        );
        for ox in [-3, 3] {
            for oz in [-3, 3] {
                for y in v.y..v.y + 4 {
                    b.set(x + ox, y, z + oz, 17 << 4);
                }
            }
        }
        b.fill(
            IVec3::new(x - 4, v.y + 4, z - 4),
            IVec3::new(x + 4, v.y + 4, z + 4),
            5 << 4,
        );
        for bx in [x - 3, x + 3] {
            b.set(bx, v.y + 1, z, 20 << 4);
        }
        let door_z = z + if dz < 0 { 3 } else { -3 };
        // Door blocks use vanilla lower/upper metadata and can be opened by interaction.
        b.set(x, v.y, door_z, (64 << 4) | if dz < 0 { 1 } else { 3 });
        b.set(x, v.y + 1, door_z, (64 << 4) | 8);
        for dzpath in 4..=10 {
            let bz = z + if dz < 0 { dzpath } else { -dzpath };
            b.foundation(x, bz, v.y, seed, 13 << 4);
        }
        b.set(x - 2, v.y, z - 2, 58 << 4);
        b.set(x + 2, v.y, z - 2, (61 << 4) | 2);
        b.set(x, v.y + 2, z - 2, (50 << 4) | 3);
        if dx < 0 && dz < 0 {
            b.chest(
                IVec3::new(x + 2, v.y, z + 2),
                &[Stack::new(297, 3), Stack::new(260, 2), Stack::new(388, 2)],
            );
        }
    }
    // Irrigated wheat plot.
    for dz in -6..=6 {
        for dx in 20..=28 {
            let (x, z) = (v.x + dx, v.z + dz);
            b.foundation(
                x,
                z,
                v.y,
                seed,
                if dx == 24 {
                    9 << 4
                } else if dx == 20 || dx == 28 || dz == -6 || dz == 6 {
                    17 << 4
                } else {
                    (60 << 4) | 7
                },
            );
            if dx != 24 && dx > 20 && dx < 28 && dz.abs() < 6 {
                b.set(x, v.y, z, (59 << 4) | 7);
            }
        }
    }
    // Central covered well.
    for dz in -2i32..=2 {
        for dx in -2i32..=2 {
            b.set(
                v.x + dx,
                v.y - 1,
                v.z + dz,
                if dx.abs() < 2 && dz.abs() < 2 {
                    9 << 4
                } else {
                    4 << 4
                },
            );
            if dx.abs() == 2 && dz.abs() == 2 {
                for dy in 0..3 {
                    b.set(v.x + dx, v.y + dy, v.z + dz, 85 << 4);
                }
            }
            b.set(v.x + dx, v.y + 3, v.z + dz, 4 << 4);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn village_coordinates_and_spawn_are_stable() {
        for seed in [0, 1, 189, 10000, u64::MAX] {
            let v = nearest_village(seed);
            assert_eq!(
                village(v.x.div_euclid(256), v.z.div_euclid(256), seed),
                Some(v)
            );
            assert_eq!(spawn(seed, true).y, v.y as f32);
        }
    }
    #[test]
    fn underground_features_preserve_bedrock_and_chunk_order() {
        let mut a = Chunk::empty();
        let mut b = Chunk::empty();
        for z in 0..16 {
            for x in 0..16 {
                for y in 0..60 {
                    a.set(x, y, z, if y == 0 { 7 << 4 } else { 1 << 4 });
                }
            }
        }
        b.blocks.clone_from(&a.blocks);
        b.top = a.top;
        decorate(&mut a, (-1, 2), 189);
        decorate(&mut b, (-1, 2), 189);
        assert_eq!(a.blocks, b.blocks);
        assert!((0..16).all(|z| (0..16).all(|x| a.get(x, 0, z) == 7 << 4)));
    }
}
