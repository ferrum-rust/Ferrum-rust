//! Original BlockModelRenderer face brightness and ambient-occlusion weights.
use super::material;
use glam::IVec3;
#[path = "ambient_occlusion_data.rs"]
mod data;
pub const DIRECTIONS: [IVec3; 6] = [
    IVec3::NEG_Y,
    IVec3::Y,
    IVec3::NEG_Z,
    IVec3::Z,
    IVec3::NEG_X,
    IVec3::X,
];
pub trait Access {
    fn state(&self, p: IVec3) -> u16;
    fn combined(&self, p: IVec3, minimum: u8) -> u32;
}
impl Access for crate::world::World {
    fn state(&self, p: IVec3) -> u16 {
        self.get(p)
    }
    fn combined(&self, p: IVec3, minimum: u8) -> u32 {
        let uv = self.light_uv(p, minimum);
        (uv[1] as u32) << 16 | uv[0] as u32
    }
}
fn facts(state: u16) -> [u32; 4] {
    data::FACTS
        .get((state >> 4) as usize)
        .copied()
        .unwrap_or(data::FACTS[0])
}
fn shade(world: &impl Access, p: IVec3) -> f32 {
    f32::from_bits(facts(world.state(p))[1])
}
fn translucent(world: &impl Access, p: IVec3) -> bool {
    facts(world.state(p))[0] != 0
}
fn opaque(world: &impl Access, p: IVec3, fancy: bool) -> bool {
    let state = world.state(p);
    if matches!(state >> 4, 18 | 161) {
        !fancy
    } else {
        facts(state)[2] != 0
    }
}
/// Block.getMixedBrightnessForBlock, including the dark-slab fallback.
pub fn mixed(world: &impl Access, p: IVec3) -> u32 {
    let state = world.state(p);
    let value = world.combined(p, material::light_value(state));
    if value == 0 && facts(state)[3] != 0 {
        let below = p - IVec3::Y;
        world.combined(below, material::light_value(world.state(below)))
    } else {
        value
    }
}
pub fn uv(value: u32) -> [u8; 4] {
    [value as u8, (value >> 16) as u8, 0, 0]
}
pub fn vertices(face: usize) -> [[usize; 3]; 4] {
    data::VERTICES[face]
}
fn average(a: u32, b: u32, c: u32, center: u32) -> u32 {
    let fallback = |v| if v == 0 { center } else { v };
    ((fallback(a) + fallback(b) + fallback(c) + center) >> 2) & 0xff00ff
}
#[derive(Debug)]
pub struct Face {
    /// Original baked-quad vertex order, exposed by vertices(face).
    pub shade: [f32; 4],
    pub brightness: [u32; 4],
}
pub fn sample(
    world: &impl Access,
    p: IVec3,
    face: usize,
    bounds: [f32; 6],
    offset: bool,
    partial: bool,
    fancy: bool,
) -> Face {
    let step = DIRECTIONS[face];
    let base = if offset { p + step } else { p };
    let sides = data::SIDES[face].map(|d| DIRECTIONS[d]);
    let positions = sides.map(|d| base + d);
    let light = positions.map(|at| mixed(world, at));
    let colors = positions.map(|at| shade(world, at));
    let pass = positions.map(|at| translucent(world, at + step));
    let pairs = [(0, 2), (0, 3), (1, 2), (1, 3)];
    let corners = pairs.map(|(a, b)| {
        if pass[a] || pass[b] {
            let at = positions[a] + sides[b];
            (shade(world, at), mixed(world, at))
        } else {
            (colors[a], light[a])
        }
    });
    let mut center = mixed(world, p);
    if offset || !opaque(world, p + step, fancy) {
        center = mixed(world, p + step);
    }
    let center_color = shade(world, base);
    let order = [(3, 0, 1), (2, 0, 0), (2, 1, 2), (3, 1, 3)];
    let color = order.map(|(a, b, c)| (colors[a] + colors[b] + corners[c].0 + center_color) * 0.25);
    let brightness = order.map(|(a, b, c)| average(light[a], light[b], corners[c].1, center));
    let mut result = Face {
        shade: [0.0; 4],
        brightness: [0; 4],
    };
    for i in 0..4 {
        let output = data::TRANSLATIONS[face][i];
        if partial && data::PARTIAL[face] {
            let bound = |n: usize| {
                if n < 6 {
                    bounds[n]
                } else {
                    1.0 - bounds[n - 6]
                }
            };
            let indices = data::WEIGHTS[face][i];
            let weights: [f32; 4] =
                std::array::from_fn(|j| bound(indices[j * 2]) * bound(indices[j * 2 + 1]));
            result.shade[output] = color[0] * weights[0]
                + color[1] * weights[1]
                + color[2] * weights[2]
                + color[3] * weights[3];
            let weighted = |shift: u32| {
                let channels = brightness.map(|v| ((v >> shift) & 255u32) as f32);
                (channels[0] * weights[0]
                    + channels[1] * weights[1]
                    + channels[2] * weights[2]
                    + channels[3] * weights[3]) as u32
                    & 255
            };
            result.brightness[output] = weighted(16) << 16 | weighted(0);
        } else {
            result.shade[output] = color[i];
            result.brightness[output] = brightness[i];
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_ambient_occlusion_all_faces_partial_weights_and_dark_slabs_match() {
        struct Grid {
            states: Vec<u16>,
            lights: Vec<u8>,
        }
        fn index(p: IVec3) -> usize {
            ((p.y - 62) * 25 + (p.z + 2) * 5 + p.x + 2) as usize
        }
        impl Access for Grid {
            fn state(&self, p: IVec3) -> u16 {
                self.states[index(p)]
            }
            fn combined(&self, p: IVec3, minimum: u8) -> u32 {
                let value = self.lights[index(p)];
                ((value >> 4) as u32) << 20 | ((value & 15).max(minimum) as u32) << 4
            }
        }
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-ambient-occlusion.json"
        ))
        .unwrap();
        let mut count = 0;
        for world in data["worlds"].as_array().unwrap() {
            let grid = Grid {
                states: world[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n.as_u64().unwrap() as u16)
                    .collect(),
                lights: world[2]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n.as_u64().unwrap() as u8)
                    .collect(),
            };
            for row in world[3].as_array().unwrap() {
                let flags = row[2].as_u64().unwrap();
                let bounds = std::array::from_fn(|i| {
                    f32::from_bits(
                        data["bounds"][row[3].as_u64().unwrap() as usize][i]
                            .as_u64()
                            .unwrap() as u32,
                    )
                });
                let actual = sample(
                    &grid,
                    IVec3::new(0, 64, 0),
                    row[1].as_u64().unwrap() as usize,
                    bounds,
                    flags & 1 != 0,
                    flags & 2 != 0,
                    row[0].as_bool().unwrap(),
                );
                assert_eq!(
                    actual.shade.map(f32::to_bits),
                    std::array::from_fn(|i| row[4][i].as_u64().unwrap() as u32),
                    "shade world {} row {row}",
                    world[0]
                );
                assert_eq!(
                    actual.brightness,
                    std::array::from_fn(|i| row[5][i].as_u64().unwrap() as u32),
                    "light world {} row {row}",
                    world[0]
                );
                count += 1;
            }
        }
        assert_eq!(count, 36288);
    }
}
