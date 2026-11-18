//! Original MapGenCavesHell: 128-height caves, lava avoidance, and independent map RNG.
use super::super::{math, random::JavaRandom};
use std::f32::consts::{FRAC_PI_2, PI};

struct Terrain<'a> {
    blocks: &'a mut [u16],
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
pub(super) fn generate(seed: i64, cx: i32, cz: i32, blocks: &mut [u16]) {
    let mut terrain = Terrain {
        blocks,
        x: cx,
        z: cz,
    };
    let mut random = JavaRandom::new(seed);
    let xs = random.next_long();
    let zs = random.next_long();
    for x in cx - 8..=cx + 8 {
        for z in cz - 8..=cz + 8 {
            random.set_seed((x as i64).wrapping_mul(xs) ^ (z as i64).wrapping_mul(zs) ^ seed);
            cave_origin(&mut terrain, &mut random, x, z);
        }
    }
}
fn cave_origin(terrain: &mut Terrain<'_>, random: &mut JavaRandom, x: i32, z: i32) {
    let bound = random.next_int(10) + 1;
    let bound = random.next_int(bound) + 1;
    let mut count = random.next_int(bound);
    if random.next_int(5) != 0 {
        count = 0;
    }
    for _ in 0..count {
        let px = (x * 16 + random.next_int(16)) as f64;
        let py = random.next_int(128) as f64;
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
            let width = random.next_float() * 2. + random.next_float();
            cave_tunnel(
                terrain,
                random.next_long(),
                [px, py, pz],
                width * 2.,
                yaw,
                pitch,
                0,
                0,
                0.5,
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
        if !single && start == branch && width > 1. {
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
            if near_chunk(p, center, radius) && carve(terrain, p, radius, height) && single {
                break;
            }
        }
        start += 1;
    }
}
fn near_chunk(p: [f64; 3], center: [f64; 2], radius: f64) -> bool {
    !(p[0] < center[0] - 16. - radius * 2.
        || p[2] < center[1] - 16. - radius * 2.
        || p[0] > center[0] + 16. + radius * 2.
        || p[2] > center[1] + 16. + radius * 2.)
}
fn carve(terrain: &mut Terrain<'_>, p: [f64; 3], radius: f64, height: f64) -> bool {
    let min_x = ((p[0] - radius).floor() as i32 - terrain.x * 16 - 1).max(0);
    let max_x = ((p[0] + radius).floor() as i32 - terrain.x * 16 + 1).min(16);
    let min_y = ((p[1] - height).floor() as i32 - 1).max(1);
    let max_y = ((p[1] + height).floor() as i32 + 1).min(120);
    let min_z = ((p[2] - radius).floor() as i32 - terrain.z * 16 - 1).max(0);
    let max_z = ((p[2] + radius).floor() as i32 - terrain.z * 16 + 1).min(16);
    for x in min_x..max_x {
        for z in min_z..max_z {
            let mut y = max_y + 1;
            while y >= min_y - 1 {
                if (0..128).contains(&y) && matches!(terrain.get(x, y, z) >> 4, 10 | 11) {
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
            for y in (min_y + 1..=max_y).rev() {
                let dy = ((y - 1) as f64 + 0.5 - p[1]) / height;
                if dy > -0.7
                    && dx * dx + dy * dy + dz * dz < 1.
                    && matches!(terrain.get(x, y, z) >> 4, 87 | 2 | 3)
                {
                    terrain.set(x, y, z, 0);
                }
            }
        }
    }
    true
}
