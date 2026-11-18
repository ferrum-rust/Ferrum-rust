//! EntityRenderer.renderRainSnow: original column geometry, textures and drift.
use super::*;
use crate::vanilla::{biome::biome, spawning::EntityRandom, weather};

pub(super) const WEATHER: u32 = 1 << 30;
#[cfg(test)]
#[path = "precipitation_render_tests.rs"]
mod tests;

pub(super) fn draw(mesh: &mut Mesh, c: &ModelContext<'_>) {
    let rain = c.sim.weather.strengths(c.alpha).0;
    if rain <= 0. || c.sim.dimension != 0 {
        return;
    }
    let feet = c.player.position;
    let origin = feet.floor().as_ivec3();
    let interpolated = c.player.previous.lerp(feet, c.alpha);
    let radius = if c.settings.fancy { 10 } else { 5 };
    let tick = c.sim.total_ticks as i32;
    let time = tick as f32 + c.alpha;
    for z in origin.z - radius..=origin.z + radius {
        for x in origin.x - radius..=origin.x + radius {
            if !c.world.chunks.contains_key(&(x >> 4, z >> 4)) {
                continue;
            }
            let id = crate::assets::biome_colors::biome_at(c.world, glam::IVec3::new(x, 0, z));
            if !biome(id).snowy && !crate::vanilla::runtime::biome_can_rain(id) {
                continue;
            }
            let top = weather::precipitation_height(c.world, x, z);
            let bottom = (origin.y - radius).max(top);
            let upper = (origin.y + radius).max(top);
            if bottom == upper {
                continue;
            }
            let dx = (x - origin.x) as f32;
            let dz = (z - origin.z) as f32;
            let distance = (dx * dx + dz * dz).sqrt();
            if distance == 0. {
                continue;
            } // Original center column has NaN-width and emits no fragments.
            let width = Vec3::new(-dz, 0., dx) / distance * 0.5;
            let a = x
                .wrapping_mul(x)
                .wrapping_mul(3121)
                .wrapping_add(x.wrapping_mul(45_238_971));
            let b = z
                .wrapping_mul(z)
                .wrapping_mul(418_711)
                .wrapping_add(z.wrapping_mul(13_761));
            let mut random = EntityRandom::new(i64::from(a ^ b));
            let snow = weather::snow_at(id, glam::IVec3::new(x, bottom, z));
            let (u, v) = if snow {
                let fall = ((tick & 511) as f32 + c.alpha) / 512.;
                let u = random.random.next_double()
                    + f64::from(time) * 0.01 * f64::from(random.next_gaussian() as f32);
                let v = random.random.next_double()
                    + f64::from(time * random.next_gaussian() as f32) * 0.001;
                (u as f32, fall + v as f32)
            } else {
                (
                    0.,
                    (((tick.wrapping_add(a).wrapping_add(b) & 31) as f64 + f64::from(c.alpha))
                        / 32.
                        * (3. + random.random.next_double())) as f32,
                )
            };
            let radial = Vec3::new(x as f32 + 0.5 - feet.x, 0., z as f32 + 0.5 - feet.z).length()
                / radius as f32;
            let alpha = ((1. - radial * radial) * if snow { 0.3 } else { 0.5 } + 0.5) * rain;
            let sprite = c.resources.sprite(if snow {
                "textures/environment/snow.png"
            } else {
                "textures/environment/rain.png"
            });
            let low = Vec3::new(x as f32 + 0.5, bottom as f32, z as f32 + 0.5);
            let high = Vec3::new(low.x, upper as f32, low.z);
            let quad = [low - width, low + width, high + width, high - width];
            let uv = [
                [u, bottom as f32 * 0.25 + v],
                [u + 1., bottom as f32 * 0.25 + v],
                [u + 1., upper as f32 * 0.25 + v],
                [u, upper as f32 * 0.25 + v],
            ];
            let mut light = c.world.visual_light_uv(
                glam::IVec3::new(x, top.max(interpolated.y.floor() as i32), z),
                0,
            );
            if snow {
                light[0] = ((u16::from(light[0]) * 3 + 240) / 4) as u8;
                light[1] = ((u16::from(light[1]) * 3 + 240) / 4) as u8;
            }
            let start = mesh.vertices.len() as u32;
            for (position, uv) in quad.into_iter().zip(uv) {
                mesh.vertices.push(Vertex {
                    position: position.to_array(),
                    uv,
                    layer: sprite.layer | ATLAS | BLEND | WEATHER,
                    color: [255, 255, 255, (alpha.clamp(0., 1.) * 255.) as u8],
                    light,
                });
            }
            // Vanilla disables face culling for weather columns.
            mesh.indices
                .extend([0, 1, 2, 0, 2, 3, 2, 1, 0, 3, 2, 0].map(|i| start + i));
            mesh.visible_faces += 2;
        }
    }
}
