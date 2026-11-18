//! Native 1.8.9 MapGenBase / MapGenCaves / MapGenRavine.
//! Blocks use the project's y-z-x layout and legacy id<<4|metadata states.
use super::{math, random::JavaRandom};
use std::f32::consts::{FRAC_PI_2, PI};

struct Terrain<'a> {
    blocks: &'a mut [u16],
    top: &'a [u16; 256],
    x: i32,
    z: i32,
}
impl Terrain<'_> {
    fn index(x: i32, y: i32, z: i32) -> usize {
        ((y * 16 + z) * 16 + x) as usize
    }
    fn get(&self, x: i32, y: i32, z: i32) -> u16 {
        self.blocks[Self::index(x, y, z)]
    }
    fn set(&mut self, x: i32, y: i32, z: i32, state: u16) {
        self.blocks[Self::index(x, y, z)] = state;
    }
}
/// Apply caves after biome surface blocks and before ravines/structures.
pub fn caves(seed: i64, cx: i32, cz: i32, blocks: &mut [u16], biome_top: &[u16; 256]) {
    generate(seed, cx, cz, blocks, biome_top, false);
}
pub fn ravines(seed: i64, cx: i32, cz: i32, blocks: &mut [u16], biome_top: &[u16; 256]) {
    generate(seed, cx, cz, blocks, biome_top, true);
}
fn generate(seed: i64, cx: i32, cz: i32, blocks: &mut [u16], top: &[u16; 256], ravine: bool) {
    assert_eq!(blocks.len(), 65536);
    let mut terrain = Terrain {
        blocks,
        top,
        x: cx,
        z: cz,
    };
    let mut random = JavaRandom::new(seed);
    let x_seed = random.next_long();
    let z_seed = random.next_long();
    for x in cx - 8..=cx + 8 {
        for z in cz - 8..=cz + 8 {
            random
                .set_seed((x as i64).wrapping_mul(x_seed) ^ (z as i64).wrapping_mul(z_seed) ^ seed);
            if ravine {
                ravine_origin(&mut terrain, &mut random, x, z);
            } else {
                cave_origin(&mut terrain, &mut random, x, z);
            }
        }
    }
}
fn cave_origin(terrain: &mut Terrain<'_>, random: &mut JavaRandom, x: i32, z: i32) {
    let bound = random.next_int(15) + 1;
    let bound = random.next_int(bound) + 1;
    let mut count = random.next_int(bound);
    if random.next_int(7) != 0 {
        count = 0;
    }
    for _ in 0..count {
        let px = (x * 16 + random.next_int(16)) as f64;
        let bound = random.next_int(120) + 8;
        let py = random.next_int(bound) as f64;
        let pz = (z * 16 + random.next_int(16)) as f64;
        let mut tunnels = 1;
        if random.next_int(4) == 0 {
            let seed = random.next_long();
            let width = 1. + random.next_float() * 6.;
            cave_tunnel(terrain, seed, [px, py, pz], width, 0., 0., -1, -1, 0.5);
            tunnels += random.next_int(4);
        }
        for _ in 0..tunnels {
            let yaw = random.next_float() * PI * 2.;
            let pitch = (random.next_float() - 0.5) * 2. / 8.;
            let mut width = random.next_float() * 2. + random.next_float();
            if random.next_int(10) == 0 {
                width *= random.next_float() * random.next_float() * 3. + 1.;
            }
            cave_tunnel(
                terrain,
                random.next_long(),
                [px, py, pz],
                width,
                yaw,
                pitch,
                0,
                0,
                1.,
            );
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn cave_tunnel(
    terrain: &mut Terrain<'_>,
    seed: i64,
    mut p: [f64; 3],
    width: f32,
    mut yaw: f32,
    mut pitch: f32,
    mut start: i32,
    mut end: i32,
    vertical: f64,
) {
    let center = [(terrain.x * 16 + 8) as f64, (terrain.z * 16 + 8) as f64];
    let mut yaw_change = 0.;
    let mut pitch_change = 0.;
    let mut random = JavaRandom::new(seed);
    if end <= 0 {
        end = 112 - random.next_int(28);
    }
    let single = start == -1;
    if single {
        start = end / 2;
    }
    let branch = random.next_int(end / 2) + end / 4;
    let gentle = random.next_int(6) == 0;
    while start < end {
        let radius = 1.5 + (math::sin(start as f32 * PI / end as f32) * width) as f64;
        let height = radius * vertical;
        let flat = math::cos(pitch);
        let up = math::sin(pitch);
        p[0] += (math::cos(yaw) * flat) as f64;
        p[1] += up as f64;
        p[2] += (math::sin(yaw) * flat) as f64;
        pitch *= if gentle { 0.92 } else { 0.7 };
        pitch += pitch_change * 0.1;
        yaw += yaw_change * 0.1;
        pitch_change *= 0.9;
        yaw_change *= 0.75;
        pitch_change += (random.next_float() - random.next_float()) * random.next_float() * 2.;
        yaw_change += (random.next_float() - random.next_float()) * random.next_float() * 4.;
        if !single && start == branch && width > 1. && end > 0 {
            let seed = random.next_long();
            let branch_width = random.next_float() * 0.5 + 0.5;
            cave_tunnel(
                terrain,
                seed,
                p,
                branch_width,
                yaw - FRAC_PI_2,
                pitch / 3.,
                start,
                end,
                1.,
            );
            let seed = random.next_long();
            let branch_width = random.next_float() * 0.5 + 0.5;
            cave_tunnel(
                terrain,
                seed,
                p,
                branch_width,
                yaw + FRAC_PI_2,
                pitch / 3.,
                start,
                end,
                1.,
            );
            return;
        }
        if single || random.next_int(4) != 0 {
            let dx = p[0] - center[0];
            let dz = p[2] - center[1];
            let remaining = (end - start) as f64;
            let reach = (width + 2. + 16.) as f64;
            if dx * dx + dz * dz - remaining * remaining > reach * reach {
                return;
            }
            if near_chunk(p, center, radius) && carve(terrain, p, radius, height, None) && single {
                break;
            }
        }
        start += 1;
    }
}
fn ravine_origin(terrain: &mut Terrain<'_>, random: &mut JavaRandom, x: i32, z: i32) {
    if random.next_int(50) != 0 {
        return;
    }
    let px = (x * 16 + random.next_int(16)) as f64;
    let bound = random.next_int(40) + 8;
    let py = (random.next_int(bound) + 20) as f64;
    let pz = (z * 16 + random.next_int(16)) as f64;
    let yaw = random.next_float() * PI * 2.;
    let pitch = (random.next_float() - 0.5) * 2. / 8.;
    let width = (random.next_float() * 2. + random.next_float()) * 2.;
    ravine_tunnel(terrain, random.next_long(), [px, py, pz], width, yaw, pitch);
}
fn ravine_tunnel(
    terrain: &mut Terrain<'_>,
    seed: i64,
    mut p: [f64; 3],
    width: f32,
    mut yaw: f32,
    mut pitch: f32,
) {
    let mut random = JavaRandom::new(seed);
    let center = [(terrain.x * 16 + 8) as f64, (terrain.z * 16 + 8) as f64];
    let end = 112 - random.next_int(28);
    let mut yaw_change = 0.;
    let mut pitch_change = 0.;
    let mut shape = [0.; 256];
    let mut factor = 1.;
    for (i, value) in shape.iter_mut().enumerate() {
        if i == 0 || random.next_int(3) == 0 {
            factor = 1. + random.next_float() * random.next_float();
        }
        *value = factor * factor;
    }
    for step in 0..end {
        let mut radius = 1.5 + (math::sin(step as f32 * PI / end as f32) * width) as f64;
        let mut height = radius * 3.;
        radius *= random.next_float() as f64 * 0.25 + 0.75;
        height *= random.next_float() as f64 * 0.25 + 0.75;
        let flat = math::cos(pitch);
        p[0] += (math::cos(yaw) * flat) as f64;
        p[1] += math::sin(pitch) as f64;
        p[2] += (math::sin(yaw) * flat) as f64;
        pitch *= 0.7;
        pitch += pitch_change * 0.05;
        yaw += yaw_change * 0.05;
        pitch_change *= 0.8;
        yaw_change *= 0.5;
        pitch_change += (random.next_float() - random.next_float()) * random.next_float() * 2.;
        yaw_change += (random.next_float() - random.next_float()) * random.next_float() * 4.;
        if random.next_int(4) != 0 {
            let dx = p[0] - center[0];
            let dz = p[2] - center[1];
            let remaining = (end - step) as f64;
            let reach = (width + 2. + 16.) as f64;
            if dx * dx + dz * dz - remaining * remaining > reach * reach {
                return;
            }
            if near_chunk(p, center, radius) {
                carve(terrain, p, radius, height, Some(&shape));
            }
        }
    }
}
fn near_chunk(p: [f64; 3], center: [f64; 2], radius: f64) -> bool {
    !(p[0] < center[0] - 16. - radius * 2.
        || p[2] < center[1] - 16. - radius * 2.
        || p[0] > center[0] + 16. + radius * 2.
        || p[2] > center[1] + 16. + radius * 2.)
}
fn carve(
    terrain: &mut Terrain<'_>,
    p: [f64; 3],
    radius: f64,
    height: f64,
    ravine: Option<&[f32; 256]>,
) -> bool {
    let min_x = ((p[0] - radius).floor() as i32 - terrain.x * 16 - 1).max(0);
    let max_x = ((p[0] + radius).floor() as i32 - terrain.x * 16 + 1).min(16);
    let min_y = ((p[1] - height).floor() as i32 - 1).max(1);
    let max_y = ((p[1] + height).floor() as i32 + 1).min(248);
    let min_z = ((p[2] - radius).floor() as i32 - terrain.z * 16 - 1).max(0);
    let max_z = ((p[2] + radius).floor() as i32 - terrain.z * 16 + 1).min(16);
    for x in min_x..max_x {
        for z in min_z..max_z {
            let mut y = max_y + 1;
            while y >= min_y - 1 {
                if (0..256).contains(&y) && matches!(terrain.get(x, y, z) >> 4, 8 | 9) {
                    return false;
                }
                if y != min_y - 1 && x != min_x && x != max_x - 1 && z != min_z && z != max_z - 1 {
                    y = min_y;
                }
                y -= 1;
            }
        }
    }
    for x in min_x..max_x {
        let dx = ((x + terrain.x * 16) as f64 + 0.5 - p[0]) / radius;
        for z in min_z..max_z {
            let dz = ((z + terrain.z * 16) as f64 + 0.5 - p[2]) / radius;
            let mut surface = false;
            if dx * dx + dz * dz >= 1. {
                continue;
            }
            for y in (min_y + 1..=max_y).rev() {
                let dy = ((y - 1) as f64 + 0.5 - p[1]) / height;
                let inside = if let Some(shape) = ravine {
                    (dx * dx + dz * dz) * shape[(y - 1) as usize] as f64 + dy * dy / 6. < 1.
                } else {
                    dy > -0.7 && dx * dx + dy * dy + dz * dz < 1.
                };
                if !inside {
                    continue;
                }
                let state = terrain.get(x, y, z);
                let id = state >> 4;
                let above = terrain.get(x, y + 1, z);
                if id == 2 || ravine.is_none() && id == 110 {
                    surface = true;
                }
                let replace = if ravine.is_some() {
                    matches!(id, 1..=3)
                } else {
                    matches!(id, 1..=3 | 24 | 78 | 110 | 159 | 172 | 179)
                        || matches!(id, 12 | 13) && !matches!(above >> 4, 8 | 9)
                };
                if !replace {
                    continue;
                }
                if y - 1 < 10 {
                    terrain.set(x, y, z, if ravine.is_some() { 10 << 4 } else { 11 << 4 });
                    continue;
                }
                terrain.set(x, y, z, 0);
                if ravine.is_none() && above >> 4 == 12 {
                    terrain.set(
                        x,
                        y + 1,
                        z,
                        if above & 15 == 1 { 179 << 4 } else { 24 << 4 },
                    );
                }
                if surface && terrain.get(x, y - 1, z) >> 4 == 3 {
                    let top = terrain.top[(z * 16 + x) as usize];
                    terrain.set(x, y - 1, z, if ravine.is_some() { top } else { top & !15 });
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    fn corpus() -> serde_json::Value {
        serde_json::from_str(include_str!("../../tests/fixtures/vanilla189-carvers.json")).unwrap()
    }
    fn initial(y: usize, pattern: u64) -> u16 {
        if y == 0 {
            return 7 << 4;
        }
        if pattern == 1 {
            if y.is_multiple_of(19) {
                return (12 << 4) | 1;
            }
            if y.is_multiple_of(17) {
                return 13 << 4;
            }
            if y.is_multiple_of(13) {
                return (159 << 4) | 14;
            }
        }
        if pattern == 2 && (58..=66).contains(&y) {
            return 9 << 4;
        }
        1 << 4
    }
    fn fnv(bytes: impl IntoIterator<Item = u8>) -> u64 {
        bytes.into_iter().fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ byte as u64).wrapping_mul(0x100000001b3)
        })
    }
    #[test]
    fn sine_lookup_table_and_truncation_match_original_java_bits() {
        let fixture = corpus();
        let hash = fnv(math::table_bits().flat_map(u32::to_le_bytes));
        assert_eq!(
            format!("{hash:x}"),
            fixture["sine_table_hash"].as_str().unwrap()
        );
        for angle in fixture["angles"].as_array().unwrap() {
            let bits = |index| angle[index].as_str().unwrap().parse::<u32>().unwrap();
            let value = f32::from_bits(bits(0));
            assert_eq!(math::sin(value).to_bits(), bits(1), "sin {value}");
            assert_eq!(math::cos(value).to_bits(), bits(2), "cos {value}");
        }
    }
    #[test]
    fn caves_and_ravines_match_all_original_chunk_state_hashes() {
        let fixture = corpus();
        let mut changed_cases = 0;
        for row in fixture["cases"].as_array().unwrap() {
            let seed = row["seed"].as_str().unwrap().parse().unwrap();
            let cx = row["chunk"][0].as_i64().unwrap() as i32;
            let cz = row["chunk"][1].as_i64().unwrap() as i32;
            let pattern = row["pattern"].as_u64().unwrap();
            let mut blocks = (0..65536)
                .map(|i| initial(i / 256, pattern))
                .collect::<Vec<_>>();
            let name = row["kind"].as_str().unwrap();
            if name == "caves" {
                caves(seed, cx, cz, &mut blocks, &[2 << 4; 256]);
            } else {
                ravines(seed, cx, cz, &mut blocks, &[2 << 4; 256]);
            }
            let hash = fnv(blocks.iter().flat_map(|s| s.to_le_bytes()));
            let changed = blocks
                .iter()
                .enumerate()
                .filter(|&(i, &state)| state != initial(i / 256, pattern))
                .count();
            let air = blocks.iter().filter(|&&s| s == 0).count();
            let lava = blocks
                .iter()
                .filter(|&&s| matches!(s >> 4, 10 | 11))
                .count();
            assert_eq!(
                changed as u64,
                row["changed"].as_u64().unwrap(),
                "changed {name} seed={seed} chunk={cx},{cz} pattern={pattern}"
            );
            assert_eq!(
                air as u64,
                row["air"].as_u64().unwrap(),
                "air {name} seed={seed} chunk={cx},{cz} pattern={pattern}"
            );
            assert_eq!(lava as u64, row["lava"].as_u64().unwrap());
            assert_eq!(
                format!("{hash:x}"),
                row["hash"].as_str().unwrap(),
                "states {name} seed={seed} chunk={cx},{cz} pattern={pattern}"
            );
            changed_cases += usize::from(changed > 0);
        }
        assert_eq!(fixture["cases"].as_array().unwrap().len(), 240);
        assert_eq!(changed_cases, 133);
    }
    #[test]
    fn cave_surface_restoration_uses_biome_default_and_converts_red_sand_above() {
        let mut blocks = vec![1 << 4; 65536];
        let top = [(3 << 4) | 2; 256];
        let mut terrain = Terrain {
            blocks: &mut blocks,
            top: &top,
            x: 0,
            z: 0,
        };
        terrain.set(8, 40, 8, 2 << 4);
        terrain.set(8, 39, 8, 3 << 4);
        terrain.set(8, 41, 8, (12 << 4) | 1);
        assert!(carve(&mut terrain, [8.5, 39.5, 8.5], 0.6, 0.6, None));
        assert_eq!(terrain.get(8, 40, 8), 0);
        assert_eq!(terrain.get(8, 39, 8), 3 << 4); // getBlock().getDefaultState strips podzol metadata.
        assert_eq!(terrain.get(8, 41, 8), 179 << 4);
    }
}
