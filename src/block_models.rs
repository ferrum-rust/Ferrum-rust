//! Bake the active 1.8.9 resource-pack block variants and their face materials.
use crate::vanilla::ambient_occlusion::{vertices, DIRECTIONS};
use anyhow::{ensure, Context, Result};
use glam::Vec3;
use serde_json::Value;
use std::collections::HashMap;

const FACES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];
const FLOWER_POT_CONTENTS: [&str; 22] = [
    "empty",
    "rose",
    "dandelion",
    "oak_sapling",
    "spruce_sapling",
    "birch_sapling",
    "jungle_sapling",
    "acacia_sapling",
    "dark_oak_sapling",
    "mushroom_red",
    "mushroom_brown",
    "cactus",
    "dead_bush",
    "fern",
    "blue_orchid",
    "allium",
    "houstonia",
    "red_tulip",
    "orange_tulip",
    "white_tulip",
    "pink_tulip",
    "oxeye_daisy",
];
#[cfg(test)]
#[path = "fire_model_tests.rs"]
mod fire_tests;
#[derive(Clone, Copy, Default)]
pub struct ModelVertex {
    pub position: Vec3,
    pub uv: [f32; 2],
}
pub struct BakedFace {
    pub vertices: [ModelVertex; 4],
    pub facing: usize,
    pub neighbor_light: bool,
    pub layer: u32,
    pub shade: u8,
    pub tinted: bool,
    pub cull_face: Option<usize>,
}
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct CubeFace {
    pub layer: u32,
    // UV at the lower corner and the changes per block along the two greedy mesh axes.
    pub origin: [i8; 2],
    pub du: [i8; 2],
    pub dv: [i8; 2],
}
#[derive(Default)]
pub struct BlockModels {
    blocks: HashMap<u16, Vec<BakedFace>>,
    wire: Vec<Vec<BakedFace>>,
    tripwires: Vec<Vec<BakedFace>>,
    wall_gates: HashMap<u16, Vec<BakedFace>>,
    locked_repeaters: HashMap<u16, Vec<BakedFace>>,
    alternatives: HashMap<u16, (u32, Vec<(u32, [CubeFace; 6])>)>,
    model_alternatives: HashMap<u16, (u32, Vec<(u32, Vec<BakedFace>)>)>,
    short_heads: HashMap<u16, Vec<BakedFace>>,
    cubes: Vec<Option<[CubeFace; 6]>>,
    particles: Vec<Option<u32>>,
    custom_shapes: Vec<bool>,
    moving: HashMap<u16, Vec<BakedFace>>,
    panes: HashMap<(u16, u8), Vec<BakedFace>>,
    doors: HashMap<(u16, u16, bool, bool), Vec<BakedFace>>,
    attached_stems: HashMap<(u16, usize), Vec<BakedFace>>,
    fire: HashMap<u8, Vec<BakedFace>>,
    flower_pots: Vec<Vec<BakedFace>>,
    vine_up: [Vec<BakedFace>; 16],
}
fn numbers<const N: usize>(value: &Value) -> Result<[f32; N]> {
    let items = value
        .as_array()
        .context("Expected model coordinate array")?;
    ensure!(items.len() == N, "Wrong model coordinate count");
    let mut result = [0.; N];
    for (dest, item) in result.iter_mut().zip(items) {
        *dest = item.as_f64().context("Invalid model coordinate")? as f32;
    }
    Ok(result)
}
fn rotate(point: Vec3, origin: Vec3, axis: &str, degrees: f32) -> Result<Vec3> {
    let radians = degrees * (std::f32::consts::PI / 180.);
    let sin = (radians as f64).sin() as f32;
    let cos = (radians as f64).cos() as f32;
    // LWJGL builds the diagonal as (1-cos)+cos in f32. At a 90-degree
    // turn it can round below one even on the axis of rotation.
    let diagonal = (1.0 - cos) + cos;
    let p = point - origin;
    let v = match axis {
        "x" => Vec3::new(
            p.x * diagonal,
            p.y * cos + p.z * (-sin),
            p.y * sin + p.z * cos,
        ),
        "y" => Vec3::new(
            p.x * cos + p.z * sin,
            p.y * diagonal,
            p.x * (-sin) + p.z * cos,
        ),
        "z" => Vec3::new(
            p.x * cos + p.y * (-sin),
            p.x * sin + p.y * cos,
            p.z * diagonal,
        ),
        _ => anyhow::bail!("Unknown model rotation axis {axis}"),
    };
    Ok(v + origin)
}
fn bake_variant(
    models: &HashMap<String, Value>,
    materials: &HashMap<String, u32>,
    variant: &Value,
) -> Result<Vec<BakedFace>> {
    let variant = variant
        .as_array()
        .and_then(|v| v.first())
        .unwrap_or(variant);
    let resource = variant["model"]
        .as_str()
        .context("Missing variant model")?
        .trim_start_matches("minecraft:");
    let name = format!("block/{resource}");
    let textures = super::model_textures(models, &name, 0);
    let mut geometry = models
        .get(&name)
        .with_context(|| format!("Missing {name}"))?;
    for _ in 0..16 {
        if geometry.get("elements").is_some() {
            break;
        }
        let parent = geometry["parent"]
            .as_str()
            .context("Missing model parent")?
            .trim_start_matches("minecraft:");
        geometry = models
            .get(parent)
            .with_context(|| format!("Missing parent {parent}"))?;
    }
    let x = (variant["x"].as_i64().unwrap_or(0) / 90).rem_euclid(4) as usize;
    let y = (variant["y"].as_i64().unwrap_or(0) / 90).rem_euclid(4) as usize;
    // ModelRotation multiplies Y and X matrices first. Sequential pivot rotations
    // round a 45-degree lever handle differently, changing its baked face direction.
    let trig = |q: usize| {
        let a = (-(q as f32) * 90.).to_radians() as f64;
        (a.sin() as f32, a.cos() as f32)
    };
    let (sx, cx) = trig(x);
    let (sy, cy) = trig(y);
    let dx = (1. - cx) + cx;
    let dy = (1. - cy) + cy;
    let rotation = [
        Vec3::new(cy * dx, sy * sx, sy * cx),
        Vec3::new(0., dy * cx, dy * (-sx)),
        Vec3::new((-sy) * dx, cy * sx, cy * cx),
    ];
    let turn_x = |mut side: usize| {
        for _ in 0..x {
            side = [3, 2, 0, 1, 4, 5][side];
        }
        side
    };
    let facing_after = |side: usize| {
        let mut side = turn_x(side);
        for _ in 0..y {
            side = [0, 1, 5, 4, 2, 3][side];
        }
        side
    };
    let mut quads = Vec::new();
    for element in geometry["elements"]
        .as_array()
        .context("Missing model elements")?
    {
        let low = Vec3::from_array(numbers(&element["from"])?) / 16.;
        let high = Vec3::from_array(numbers(&element["to"])?) / 16.;
        let bounds = [low.y, high.y, low.z, high.z, low.x, high.x];
        let transform = element
            .get("rotation")
            .map(|r| -> Result<_> {
                Ok((
                    Vec3::from_array(numbers(&r["origin"])?) / 16.,
                    r["axis"].as_str().context("Missing model axis")?,
                    r["angle"].as_f64().context("Missing model angle")? as f32,
                    r["rescale"].as_bool().unwrap_or(false),
                ))
            })
            .transpose()?;
        for (side, key) in FACES.iter().enumerate() {
            let Some(face) = element["faces"].get(*key) else {
                continue;
            };
            let key = face["texture"]
                .as_str()
                .context("Missing face texture")?
                .trim_start_matches('#');
            let layer = super::resolve_texture(&textures, key, materials)
                .or_else(|| {
                    materials
                        .get(
                            key.trim_start_matches("minecraft:")
                                .trim_start_matches("blocks/"),
                        )
                        .copied()
                })
                .unwrap_or(0);
            let rect = if let Some(uv) = face.get("uv") {
                numbers(uv)?
            } else {
                // BlockPart.getFaceUvs uses model-space (0..16) coordinates.
                let a = low * 16.;
                let b = high * 16.;
                match side {
                    0 | 1 => [a.x, a.z, b.x, b.z],
                    2 | 3 => [a.x, 16. - b.y, b.x, 16. - a.y],
                    _ => [a.z, 16. - b.y, b.z, 16. - a.y],
                }
            };
            let uv_steps = (face["rotation"].as_u64().unwrap_or(0) / 90) as usize;
            let mut data = [ModelVertex::default(); 4];
            for (i, corner) in vertices(side).into_iter().enumerate() {
                let mut position = Vec3::from_array(corner.map(|v| bounds[v]));
                if let Some((origin, axis, angle, rescale)) = transform {
                    if rescale {
                        let radians = if angle.abs() == 22.5 {
                            f64::from(0.3926991_f32)
                        } else {
                            0.7853981852531433_f64
                        };
                        let factor = (1.0_f32 / radians.cos() as f32 - 1.) + 1.;
                        let mut scale = Vec3::splat(factor);
                        scale[match axis {
                            "x" => 0,
                            "y" => 1,
                            _ => 2,
                        }] = 1.;
                        // FaceBakery.rotateScale adds the origin only after scaling.
                        position = rotate(position - origin, Vec3::ZERO, axis, angle)? * scale + origin;
                    } else {
                        position = rotate(position, origin, axis, angle)?;
                    }
                }
                if x != 0 || y != 0 {
                    let p = position - Vec3::splat(0.5);
                    position =
                        Vec3::from_array(rotation.map(|r| r.x * p.x + r.y * p.y + r.z * p.z))
                            + Vec3::splat(0.5);
                }
                let uv_index = (i + uv_steps) % 4;
                let uv = [
                    rect[if uv_index < 2 { 0 } else { 2 }] / 16.,
                    rect[if uv_index == 0 || uv_index == 3 { 1 } else { 3 }] / 16.,
                ];
                let slot =
                    (i + if side >= 4 { x } else { 0 } + if turn_x(side) <= 1 { y } else { 0 }) % 4;
                data[slot] = ModelVertex { position, uv };
            }
            let normal =
                (data[2].position - data[1].position).cross(data[0].position - data[1].position);
            let normal = normal / normal.length();
            let mut facing = 1;
            let mut best = 0.;
            for (i, d) in DIRECTIONS.iter().enumerate() {
                let value = normal.dot(d.as_vec3());
                if value > best {
                    best = value;
                    facing = i;
                }
            }
            if variant["uvlock"].as_bool() == Some(true) {
                for i in 0..4 {
                    let [a, b, c] = data[i].position.to_array().map(|v| {
                        if !(-0.1..1.1).contains(&v) {
                            v - v.floor()
                        } else {
                            v
                        }
                    });
                    data[(i + 4 - uv_steps) % 4].uv = match facing {
                        0 => [a, 1. - c],
                        1 => [a, c],
                        2 => [1. - a, 1. - b],
                        3 => [a, 1. - b],
                        4 => [c, 1. - b],
                        _ => [1. - c, 1. - b],
                    };
                }
            }
            if transform.is_none() {
                align_face(&mut data, facing);
            }
            let axis = [1, 1, 2, 2, 0, 0][facing];
            let value = data[0].position[axis];
            let neighbor_light = data.iter().all(|v| v.position[axis] == value)
                && if facing % 2 == 0 {
                    value < 0.0001
                } else {
                    value > 0.9999
                };
            quads.push(BakedFace {
                vertices: data,
                tinted: face["tintindex"].as_i64().is_some_and(|v| v >= 0),
                facing,
                neighbor_light,
                layer,
                cull_face: face["cullface"]
                    .as_str()
                    .map(|name| {
                        let side = FACES
                            .iter()
                            .position(|&face| face == name)
                            .context("Invalid model cullface")?;
                        Ok::<usize, anyhow::Error>(facing_after(side))
                    })
                    .transpose()?,
                shade: if element["shade"].as_bool() == Some(false) {
                    255
                } else {
                    [127, 255, 204, 204, 153, 153][facing_after(side)]
                },
            });
        }
    }
    Ok(quads)
}
fn missing_cube() -> Vec<BakedFace> {
    (0..6)
        .map(|side| BakedFace {
            vertices: std::array::from_fn(|i| ModelVertex {
                position: Vec3::from_array(vertices(side)[i].map(|d| (d % 2) as f32)),
                uv: [[0., 0.], [0., 1.], [1., 1.], [1., 0.]][i],
            }),
            facing: side,
            neighbor_light: true,
            layer: 0,
            tinted: false,
            shade: [127, 255, 204, 204, 153, 153][side],
            cull_face: Some(side),
        })
        .collect()
}
fn align_face(data: &mut [ModelVertex; 4], facing: usize) {
    let src = *data;
    let min = src
        .iter()
        .fold(Vec3::splat(f32::MAX), |a, v| a.min(v.position));
    let max = src
        .iter()
        .fold(Vec3::splat(f32::MIN), |a, v| a.max(v.position));
    let bounds = [min.y, max.y, min.z, max.z, min.x, max.x];
    for (i, corner) in vertices(facing).into_iter().enumerate() {
        let p = Vec3::from_array(corner.map(|n| bounds[n]));
        data[i].position = p;
        for v in src {
            if (v.position - p).abs().max_element() < 0.00001 {
                data[i].uv = v.uv;
            }
        }
    }
}
fn cube_faces(quads: &[BakedFace]) -> Option<[CubeFace; 6]> {
    if quads.len() != 6 {
        return None;
    }
    let mut result = [CubeFace::default(); 6];
    for q in quads {
        if q.cull_face != Some(q.facing) || !q.neighbor_light {
            return None;
        }
        let axis = [1, 1, 2, 2, 0, 0][q.facing];
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let mut uv = [[0i8; 2]; 4];
        for pt in q.vertices {
            if pt
                .position
                .to_array()
                .iter()
                .any(|n| (*n - n.round()).abs() > 0.0001)
                || pt.uv.iter().any(|n| (*n - n.round()).abs() > 0.0001)
            {
                return None;
            }
            let key = usize::from(pt.position[u] > 0.5) + 2 * usize::from(pt.position[v] > 0.5);
            uv[key] = pt.uv.map(|c| c.round() as i8);
        }
        result[q.facing] = CubeFace {
            layer: q.layer,
            origin: uv[0],
            du: std::array::from_fn(|i| uv[1][i] - uv[0][i]),
            dv: std::array::from_fn(|i| uv[2][i] - uv[0][i]),
        };
    }
    Some(result)
}
impl BlockModels {
    pub fn load(
        models: &HashMap<String, Value>,
        states: &HashMap<String, Value>,
        materials: &HashMap<String, u32>,
    ) -> Result<Self> {
        let mut result = Self::default();
        let registry: Vec<(u16, String, String)> =
            serde_json::from_reader(flate2::read::GzDecoder::new(
                &include_bytes!("vanilla/block-model-states189.json.gz")[..],
            ))?;
        result.cubes.resize(4096, None);
        result.particles.resize(4096, None);
        result.custom_shapes.resize(4096, false);
        for (state, name, key) in registry {
            let id = state >> 4;
            let raw_variant = &states.get(&name).unwrap_or(&Value::Null)["variants"][&key];
            let variant = raw_variant
                .as_array()
                .and_then(|a| a.first())
                .unwrap_or(raw_variant);
            if let Some(resource) = variant["model"].as_str() {
                let model_name = format!("block/{}", resource.trim_start_matches("minecraft:"));
                let chain = super::model_textures(models, &model_name, 0);
                result.particles[state as usize] =
                    super::resolve_texture(&chain, "particle", materials);
            }
            if crate::vanilla::hand_placement::door_item(id).is_some() {
                if state & 15 == 0 {
                    let variants = &states.get(&name).unwrap_or(&Value::Null)["variants"];
                    for lower in 0..8 {
                        let facing = ["east", "south", "west", "north"][usize::from(lower & 3)];
                        for upper in [false, true] {
                            for right in [false, true] {
                                let half = if upper { "upper" } else { "lower" };
                                let hinge = if right { "right" } else { "left" };
                                let key = format!("facing={facing},half={half},hinge={hinge},open={}", lower & 4 != 0);
                                let faces = bake_variant(models, materials, &variants[key])
                                    .unwrap_or_else(|_| missing_cube());
                                result.doors.insert((id, lower, upper, right), faces);
                            }
                        }
                    }
                }
                continue;
            }
            if Self::static_model(id) {
                result.blocks.insert(
                    state,
                    bake_variant(models, materials, variant).unwrap_or_else(|_| missing_cube()),
                );
                if let Some(items) = raw_variant.as_array().filter(|items| items.len() > 1) {
                    let mut list = Vec::with_capacity(items.len());
                    let mut total = 0u32;
                    for entry in items {
                        let weight = entry["weight"].as_u64().unwrap_or(1).max(1) as u32;
                        let baked = bake_variant(models, materials, entry)
                            .unwrap_or_else(|_| missing_cube());
                        total += weight;
                        list.push((weight, baked));
                    }
                    list.sort_by_key(|(weight, _)| std::cmp::Reverse(*weight));
                    result.model_alternatives.insert(state, (total, list));
                }
                if matches!(id, 107 | 183..=187) {
                    let in_wall = &states.get(&name).unwrap_or(&Value::Null)["variants"]
                        [&key.replace("in_wall=false", "in_wall=true")];
                    result.wall_gates.insert(
                        state,
                        bake_variant(models, materials, in_wall).unwrap_or_else(|_| missing_cube()),
                    );
                }
                if matches!(id, 93 | 94) {
                    let locked =
                        &states[&name]["variants"][key.replace("locked=false", "locked=true")];
                    result.locked_repeaters.insert(
                        state,
                        bake_variant(models, materials, locked).unwrap_or_else(|_| missing_cube()),
                    );
                }
                if id == 34 {
                    let short = &states.get(&name).unwrap_or(&Value::Null)["variants"]
                        [key.replace("short=false", "short=true")];
                    result.short_heads.insert(
                        state,
                        bake_variant(models, materials, short).unwrap_or_else(|_| missing_cube()),
                    );
                }
            } else if let Ok(mut faces) = bake_variant(models, materials, variant) {
                if id == 165 {
                    for q in &mut faces {
                        if q.neighbor_light {
                            q.cull_face = Some(q.facing);
                        }
                    }
                    faces.sort_by_key(|q| q.neighbor_light);
                }
                // Greedy meshing can repeat a full cube's original face texture/UVs unchanged.
                if id != 2 && !super::partial_model(state) && !super::crossed(state) {
                    result.cubes[state as usize] = cube_faces(&faces);
                    result.custom_shapes[state as usize] = result.cubes[state as usize].is_none()
                        && crate::vanilla::material::full_cube(state);
                }
                if let (Some(first), Some(options)) = (
                    result.cubes[state as usize],
                    states[&name]["variants"][&key]
                        .as_array()
                        .filter(|v| v.len() > 1),
                ) {
                    let mut alternatives = Vec::new();
                    for (index, opt) in options.iter().enumerate() {
                        let variant = if index == 0 {
                            Some(first)
                        } else {
                            bake_variant(models, materials, opt)
                                .ok()
                                .and_then(|f| cube_faces(&f))
                        };
                        if let Some(variant) = variant {
                            alternatives.push((
                                opt["weight"].as_u64().unwrap_or(1).clamp(1, 65535) as u32,
                                variant,
                            ));
                        }
                    }
                    alternatives.sort_by_key(|(weight, _)| std::cmp::Reverse(*weight));
                    result.cubes[state as usize] = Some(alternatives[0].1);
                    let total = alternatives.iter().map(|(w, _)| *w).sum();
                    result.alternatives.insert(state, (total, alternatives));
                }
                result.moving.insert(state, faces);
            }
        }
        for (id, name) in [(104, "pumpkin_stem"), (105, "melon_stem")] {
            let variants = &states.get(name).unwrap_or(&Value::Null)["variants"];
            for (face, direction) in ["north", "east", "south", "west"].into_iter().enumerate() {
                let key = format!("facing={direction}");
                result.attached_stems.insert(
                    (id, face),
                    bake_variant(models, materials, &variants[key]).unwrap_or_else(|_| missing_cube()),
                );
            }
        }
        let wire = &states.get("redstone_wire").unwrap_or(&Value::Null)["variants"];
        let attachments = ["none", "side", "up"];
        for index in 0..81 {
            let key = format!(
                "east={},north={},south={},west={}",
                attachments[index % 3],
                attachments[index / 3 % 3],
                attachments[index / 9 % 3],
                attachments[index / 27]
            );
            result.wire.push(
                bake_variant(models, materials, &wire[&key]).unwrap_or_else(|_| missing_cube()),
            );
        }
        let mut panes = vec![
            (101 << 4, "iron_bars".to_owned()),
            (102 << 4, "glass_pane".to_owned()),
        ];
        for (meta, dye) in [
            "white",
            "orange",
            "magenta",
            "light_blue",
            "yellow",
            "lime",
            "pink",
            "gray",
            "silver",
            "cyan",
            "purple",
            "blue",
            "brown",
            "green",
            "red",
            "black",
        ]
        .iter()
        .enumerate()
        {
            panes.push((160 << 4 | meta as u16, format!("{dye}_stained_glass_pane")));
        }
        for (state, name) in panes {
            let variants = &states.get(&name).unwrap_or(&Value::Null)["variants"];
            for mask in 0..16 {
                let key = format!(
                    "east={},north={},south={},west={}",
                    mask & 2 != 0,
                    mask & 1 != 0,
                    mask & 4 != 0,
                    mask & 8 != 0
                );
                result.panes.insert(
                    (state, mask),
                    bake_variant(models, materials, &variants[key])
                        .unwrap_or_else(|_| missing_cube()),
                );
            }
        }
        let fire = &states.get("fire").unwrap_or(&Value::Null)["variants"];
        for mask in 0..16u8 {
            for upper in 0..3u8 {
                for alt in [false, true] {
                    for flip in [false, true] {
                        let key = format!(
                            "alt={alt},east={},flip={flip},north={},south={},upper={upper},west={}",
                            mask & 2 != 0,
                            mask & 1 != 0,
                            mask & 4 != 0,
                            mask & 8 != 0,
                        );
                        let index = mask | upper << 4 | u8::from(alt) << 6 | u8::from(flip) << 7;
                        result.fire.insert(
                            index,
                            bake_variant(models, materials, &fire[key])
                                .unwrap_or_else(|_| missing_cube()),
                        );
                    }
                }
            }
        }
        let pot_variants = &states.get("flower_pot").unwrap_or(&Value::Null)["variants"];
        for contents in FLOWER_POT_CONTENTS {
            let key = format!("contents={contents}");
            result.flower_pots.push(
                bake_variant(models, materials, &pot_variants[&key])
                    .unwrap_or_else(|_| missing_cube()),
            );
        }
        let vine_variants = &states.get("vine").unwrap_or(&Value::Null)["variants"];
        for meta in 0..16u16 {
            let key = format!(
                "east={},north={},south={},up=true,west={}",
                meta & 8 != 0,
                meta & 4 != 0,
                meta & 1 != 0,
                meta & 2 != 0,
            );
            result.vine_up[meta as usize] = bake_variant(models, materials, &vine_variants[&key])
                .unwrap_or_else(|_| missing_cube());
        }
        let tripwire = &states.get("tripwire").unwrap_or(&Value::Null)["variants"];
        result.tripwires.reserve(64);
        for index in 0..64usize {
            let attached = (index & 32) != 0;
            let suspended = (index & 16) != 0;
            let north = (index & 1) != 0;
            let east = (index & 2) != 0;
            let south = (index & 4) != 0;
            let west = (index & 8) != 0;
            let key = format!(
                "attached={attached},east={east},north={north},south={south},suspended={suspended},west={west}"
            );
            result.tripwires.push(
                bake_variant(models, materials, &tripwire[&key]).unwrap_or_default(),
            );
        }
        for state in 0..4096u16 {
            if result.particles[state as usize].is_none() {
                let norm = Self::normalize_state(state);
                if norm != state {
                    result.particles[state as usize] = result.particles[norm as usize];
                }
            }
        }
        Ok(result)
    }
    pub fn fire(&self, world: &crate::world::World, p: glam::IVec3) -> Option<&[BakedFace]> {
        use crate::vanilla::{hand_placement::catches_fire, spawning::solid_top};
        use glam::IVec3;
        let below = world.get(p - IVec3::Y);
        let index = if solid_top(below) || catches_fire(below) {
            0
        } else {
            let alt = (p.x + p.y + p.z) & 1 != 0;
            // Java integer division truncates toward zero, including negative coordinates.
            let flip = (p.x / 2 + p.y / 2 + p.z / 2) & 1 != 0;
            let upper = if catches_fire(world.get(p + IVec3::Y)) {
                if alt { 1 } else { 2 }
            } else {
                0
            };
            let mask = [IVec3::NEG_Z, IVec3::X, IVec3::Z, IVec3::NEG_X]
                .into_iter()
                .enumerate()
                .fold(0, |mask, (i, dir)| {
                    mask | u8::from(catches_fire(world.get(p + dir))) << i
                });
            mask | upper << 4 | u8::from(alt) << 6 | u8::from(flip) << 7
        };
        self.fire.get(&index).map(Vec::as_slice)
    }
    pub fn attached_stem(&self, state: u16, face: usize) -> Option<&[BakedFace]> {
        self.attached_stems.get(&(state >> 4, face)).map(Vec::as_slice)
    }
    pub fn door(&self, state: u16, neighbor: u16) -> Option<&[BakedFace]> {
        // BlockDoor.getActualState obtains facing/open below and hinge above.
        // An orphaned upper half retains the default north/closed facing.
        let upper = state & 8 != 0;
        let matching_half = state >> 4 == neighbor >> 4 && (state ^ neighbor) & 8 != 0;
        let (lower, right) = if upper {
            (if matching_half { neighbor & 7 } else { 3 }, state & 1 != 0)
        } else {
            (state & 7, matching_half && neighbor & 1 != 0)
        };
        self.doors.get(&(state >> 4, lower, upper, right)).map(Vec::as_slice)
    }
    pub fn pane(&self, state: u16, mask: u8) -> Option<&[BakedFace]> {
        let state = if state >> 4 == 160 {
            state
        } else {
            state & !15
        };
        self.panes.get(&(state, mask)).map(Vec::as_slice)
    }
    pub fn tripwire(&self, state: u16, mask: u8) -> &[BakedFace] {
        let attached = (state & 4) != 0;
        let suspended = (state & 2) != 0;
        let index =
            (usize::from(attached) << 5) | (usize::from(suspended) << 4) | usize::from(mask & 15);
        self.tripwires.get(index).map_or(&[], Vec::as_slice)
    }
    pub fn fence_gate(&self, state: u16, in_wall: bool) -> Option<&[BakedFace]> {
        if in_wall {
            self.wall_gates.get(&state).map(Vec::as_slice)
        } else {
            self.get(state)
        }
    }
    pub fn particle(&self, state: u16) -> Option<u32> {
        self.particles.get(state as usize).copied().flatten()
    }
    pub fn static_model(id: u16) -> bool {
        matches!(
            id,
            26 | 29
                | 30
                | 33
                | 34
                | 50
                | 51
                | 64
                | 65
                | 69
                | 70
                | 71
                | 72
                | 75
                | 76
                | 77
                | 81
                | 90
                | 92
                | 93
                | 94
                | 96
                | 104
                | 105
                | 106
                | 107
                | 111
                | 115
                | 116
                | 117
                | 118
                | 120
                | 122
                | 127
                | 131
                | 132
                | 138
                | 140
                | 143
                | 145
                | 147
                | 148
                | 149
                | 150
                | 151
                | 154
                | 167
                | 178
                | 183..=187
                | 193..=197
        )
    }
    pub fn custom_shape(&self, state: u16) -> bool {
        let state = if state >> 4 == 165 { 165 << 4 } else { state };
        self.custom_shapes
            .get(state as usize)
            .copied()
            .unwrap_or(false)
    }
    pub fn vine(&self, state: u16, up: bool) -> Option<&[BakedFace]> {
        if up {
            Some(&self.vine_up[usize::from(state & 15)])
        } else {
            self.get(state)
        }
    }
    pub fn carried(&self, state: u16) -> Option<&[BakedFace]> {
        let state = if state >> 4 == 165 { 165 << 4 } else { state };
        self.get(state)
            .or_else(|| self.moving.get(&state).map(Vec::as_slice))
    }
    pub fn cube(&self, state: u16, side: usize) -> Option<CubeFace> {
        self.cubes
            .get(state as usize)?
            .as_ref()
            .map(|faces| faces[side])
    }
    pub fn cube_at(
        &self,
        state: u16,
        side: usize,
        p: glam::IVec3,
        alternate: bool,
    ) -> Option<CubeFace> {
        if alternate {
            if let Some((total, variants)) = self.alternatives.get(&state) {
                let k = i64::from(p.x.wrapping_mul(3129871))
                    ^ (i64::from(p.z) * 116129781)
                    ^ i64::from(p.y);
                let random = k
                    .wrapping_mul(k)
                    .wrapping_mul(42317861)
                    .wrapping_add(k.wrapping_mul(11));
                let mut index = ((random as i32) >> 16).unsigned_abs() % total;
                for (weight, faces) in variants {
                    if index < *weight {
                        return Some(faces[side]);
                    }
                    index -= *weight;
                }
            }
        }
        self.cube(state, side)
    }
    pub fn repeater(&self, state: u16, locked: bool) -> Option<&[BakedFace]> {
        if locked {
            self.locked_repeaters.get(&state).map(Vec::as_slice)
        } else {
            self.get(state)
        }
    }
    pub fn wire(&self, east: usize, north: usize, south: usize, west: usize) -> &[BakedFace] {
        &self.wire[east + 3 * north + 9 * south + 27 * west]
    }
    pub fn head(&self, state: u16, short: bool) -> Option<&[BakedFace]> {
        let state = if state >> 4 == 34 && (state & 7) > 5 {
            (state & !7) | 1
        } else {
            state
        };
        if short {
            self.short_heads.get(&state).map(Vec::as_slice)
        } else {
            self.get(state)
        }
    }
    pub fn flower_pot(
        &self,
        state: u16,
        tile: Option<&crate::vanilla::drops::DropNbt>,
    ) -> Option<&[BakedFace]> {
        let index = if let Some(item) =
            crate::vanilla::middle_pick::block(state, 0, tile).filter(|i| i.id != 390)
        {
            match (item.id, item.damage) {
                (37, 0) => 2,
                (38, 0) => 1,
                (38, d @ 1..=8) => 13 + usize::from(d),
                (6, d @ 0..=5) => 3 + usize::from(d),
                (40, _) => 9,
                (39, _) => 10,
                (81, _) => 11,
                (32, _) => 12,
                (31, 2) => 13,
                _ => 0,
            }
        } else {
            match state & 15 {
                1 => 1,
                2 => 2,
                3 => 3,
                4 => 4,
                5 => 5,
                6 => 6,
                7 => 9,
                8 => 10,
                9 => 11,
                10 => 12,
                11 => 13,
                12 => 7,
                13 => 8,
                _ => 0,
            }
        };
        self.flower_pots
            .get(index)
            .map(Vec::as_slice)
            .or_else(|| self.get(state))
    }
    fn normalize_state(state: u16) -> u16 {
        match state >> 4 {
            26 => state & !4,
            29 | 33 | 34 if (state & 7) > 5 => (state & !7) | 1,
            30 => 30 << 4,
            50 | 75 | 76 if !(1..=5).contains(&(state & 15)) => (state & !15) | 5,
            52 => 52 << 4,
            90 => (90 << 4) | if (state & 3) == 2 { 2 } else { 1 },
            111 => 111 << 4,
            140 => 140 << 4,
            145 if state & 15 >= 12 => (145 << 4) | 8 | (state & 3),
            165 => 165 << 4,
            _ => state,
        }
    }
    pub fn get(&self, state: u16) -> Option<&[BakedFace]> {
        if crate::vanilla::hand_placement::door_item(state >> 4).is_some() {
            return self.door(state, 0);
        }
        let state = Self::normalize_state(state);
        self.blocks.get(&state).map(Vec::as_slice)
    }
    pub fn get_at(&self, state: u16, p: glam::IVec3, alternate: bool) -> Option<&[BakedFace]> {
        if crate::vanilla::hand_placement::door_item(state >> 4).is_some() {
            return self.door(state, 0);
        }
        let norm_state = Self::normalize_state(state);
        if alternate {
            if let Some((total, variants)) = self.model_alternatives.get(&norm_state) {
                let k = i64::from(p.x.wrapping_mul(3129871))
                    ^ (i64::from(p.z) * 116129781)
                    ^ i64::from(p.y);
                let random = k
                    .wrapping_mul(k)
                    .wrapping_mul(42317861)
                    .wrapping_add(k.wrapping_mul(11));
                let mut index = ((random as i32) >> 16).unsigned_abs() % *total;
                for (weight, faces) in variants {
                    if index < *weight {
                        return Some(faces.as_slice());
                    }
                    index -= *weight;
                }
            }
        }
        self.blocks.get(&norm_state).map(Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "uses installed original block atlas"]
    fn infested_stone_variants_emit_original_faces_and_hide_adjacent_block_faces() {
        use crate::world::{Chunk, World};
        use glam::IVec3;
        use std::sync::Arc;
        let a = crate::validation::assets();
        let mut w = World {
            chunks: [((0, 0), Arc::new(Chunk::empty()))].into(),
            spawn: Vec3::ZERO,
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        };
        let p = IVec3::new(6, 64, 6);
        for (meta, &host) in [16, 64, 98 << 4, 98 << 4 | 1, 98 << 4 | 2, 98 << 4 | 3]
            .iter()
            .enumerate()
        {
            let egg = 97 << 4 | meta as u16;
            w.set(p, egg);
            for alternate in [false, true] {
                let settings = crate::settings::Settings {
                    greedy_meshing: false,
                    alternate_blocks: alternate,
                    ..Default::default()
                };
                w.set(p + IVec3::X, 0);
                let m = crate::mesh::build_with_settings(&w, (0, 0), a, &settings);
                assert_eq!(m.quads(), 6);
                let original = a.block_models.carried(egg).unwrap();
                for face in original {
                    let expected_layer = a.block_models.cube(host, face.facing).unwrap().layer;
                    assert_eq!(face.layer, expected_layer);
                    let dir = DIRECTIONS[face.facing].as_vec3();
                    let plane = if face.facing % 2 == 1 { 1. } else { 0. };
                    let mesh_face = m
                        .vertices
                        .chunks_exact(4)
                        .find(|quad| {
                            quad.iter().all(|v| {
                                ((Vec3::from_array(v.position) - p.as_vec3()).dot(dir.abs())
                                    - plane)
                                    .abs()
                                    < 1e-5
                            })
                        })
                        .unwrap();
                    for v in face.vertices {
                        let vertex = mesh_face
                            .iter()
                            .find(|a| {
                                Vec3::from_array(a.position)
                                    .abs_diff_eq(p.as_vec3() + v.position, 1e-5)
                            })
                            .unwrap();
                        assert_eq!(
                            (vertex.layer, vertex.uv),
                            (face.layer, v.uv),
                            "{egg} face {}",
                            face.facing
                        );
                    }
                }
                w.set(p + IVec3::X, host);
                let m = crate::mesh::build_with_settings(&w, (0, 0), a, &settings);
                assert_eq!(
                    m.quads(),
                    10,
                    "the interface must be culled from both sides"
                );
            }
        }
    }
    #[test]
    #[ignore = "uses installed original glass textures"]
    fn placed_pane_rebuilds_cross_arm_and_isolated_shape_across_chunk_edge() {
        use crate::world::{Chunk, World};
        use glam::IVec3;
        use std::sync::Arc;
        let assets = crate::validation::assets();
        let mut w = World {
            chunks: [
                ((0, 0), Arc::new(Chunk::empty())),
                ((1, 0), Arc::new(Chunk::empty())),
            ]
            .into(),
            spawn: Vec3::ZERO,
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        };
        let center = IVec3::new(15, 64, 8);
        let settings = crate::settings::Settings::default();
        for state in [102 << 4, 160 << 4 | 14] {
            w.set(center, state);
            let expected = |mask: u8, cull: Option<usize>| {
                assets
                    .block_models
                    .pane(state, mask)
                    .unwrap()
                    .iter()
                    .filter(|q| cull.is_none() || q.cull_face != cull)
                    .flat_map(|q| {
                        q.vertices
                            .map(|v| ((v.position + center.as_vec3()).to_array(), v.uv, q.layer))
                    })
                    .collect::<Vec<_>>()
            };
            for (neighbor, mask, cull) in [
                (0, 0, None),
                (state, 2, Some(5)),
                (1 << 4, 2, Some(5)),
                (50 << 4 | 5, 0, None),
                (0, 0, None),
            ] {
                w.set(center + IVec3::X, neighbor);
                let mesh = crate::mesh::build_with_settings(&w, (0, 0), assets, &settings);
                let actual = mesh
                    .vertices
                    .iter()
                    .map(|v| (v.position, v.uv, v.layer & !crate::models::BLEND))
                    .collect::<Vec<_>>();
                assert_eq!(
                    actual,
                    expected(mask, cull),
                    "center {state}, neighbor {neighbor}"
                );
            }
        }
    }
    #[test]
    #[ignore = "uses installed original models"]
    fn circuit_variants_match_original_face_bakery() {
        type Face = (String, usize, usize, u8, i32, [[f32; 5]; 4]);
        let cases: Vec<(String, String, Vec<Face>)> =
            serde_json::from_reader(flate2::read::GzDecoder::new(
                &include_bytes!("../tests/fixtures/vanilla189-circuit-models.json.gz")[..],
            ))
            .unwrap();
        let registry: Vec<(u16, String, String)> =
            serde_json::from_reader(flate2::read::GzDecoder::new(
                &include_bytes!("vanilla/block-model-states189.json.gz")[..],
            ))
            .unwrap();
        let assets = crate::validation::assets();
        assert_eq!(cases.len(), 247);
        for (name, key, reference) in cases {
            let model = &assets.block_models;
            let faces = if name == "redstone_wire" {
                let a: Vec<usize> = key
                    .split(',')
                    .map(|v| match v.split('=').nth(1).unwrap() {
                        "side" => 1,
                        "up" => 2,
                        _ => 0,
                    })
                    .collect();
                model.wire(a[0], a[1], a[2], a[3])
            } else {
                let canonical = key
                    .replace("short=true", "short=false")
                    .replace("locked=true", "locked=false");
                let state = registry
                    .iter()
                    .find(|(_, n, k)| n == &name && k == &canonical)
                    .unwrap()
                    .0;
                if name == "piston_head" {
                    model.head(state, key.contains("short=true")).unwrap()
                } else {
                    if matches!(state >> 4, 93 | 94) {
                        model.repeater(state, key.contains("locked=true")).unwrap()
                    } else {
                        model.carried(state).unwrap()
                    }
                }
            };
            assert_eq!(faces.len(), reference.len(), "{name} {key}");
            for (q, (texture, facing, cull, shade, tint, points)) in faces.iter().zip(reference) {
                assert_eq!(
                    (
                        q.layer,
                        q.facing,
                        q.cull_face.unwrap_or(6),
                        q.shade,
                        q.tinted
                    ),
                    (assets.materials[&texture], facing, cull, shade, tint >= 0),
                    "{name} {key} {texture}"
                );
                for (point, expected) in q.vertices.iter().zip(points) {
                    let observed = [
                        point.position.x,
                        point.position.y,
                        point.position.z,
                        point.uv[0],
                        point.uv[1],
                    ];
                    assert!(
                        observed
                            .iter()
                            .zip(expected)
                            .all(|(a, b)| (a - b).abs() < 0.000002),
                        "{name} {key} {texture} {observed:?} vs {expected:?}"
                    );
                }
            }
        }
    }
    #[test]
    #[ignore = "uses installed original models"]
    fn world_cube_faces_use_state_rotation_color_and_original_atlas_layers() {
        let assets = crate::validation::assets();
        let layer = |state, axis, positive| assets.texture(state, axis, positive);
        for (state, axis, positive, name) in [
            (95 << 4 | 14, 0, false, "glass_red"),
            (159 << 4 | 11, 1, true, "hardened_clay_stained_blue"),
            (19 << 4 | 1, 1, true, "sponge_wet"),
            (17 << 4 | 4, 0, true, "log_oak_top"),
            (162 << 4 | 9, 2, true, "log_big_oak_top"),
            (170 << 4 | 4, 0, false, "hay_block_top"),
            (86 << 4 | 2, 2, false, "pumpkin_face_off"),
            (168 << 4 | 1, 2, true, "prismarine_bricks"),
        ] {
            assert_eq!(
                layer(state, axis, positive),
                assets.materials[name],
                "{state} {name}"
            );
        }
        assert!(assets.block_models.cubes.iter().flatten().count() > 175);
    }
    #[test]
    #[ignore = "requires the locally owned 1.8.9 resource JAR"]
    fn original_connected_bars_and_panes_match_bakery() {
        let rows: Vec<(u16, u8, usize, String)> =
            serde_json::from_str(include_str!("../tests/fixtures/vanilla189-pane-model.json"))
                .unwrap();
        let jar = crate::minecraft_dir().join("versions/1.8.9/1.8.9.jar");
        let assets = super::super::Assets::load(&jar).unwrap();
        let names: HashMap<_, _> = assets
            .materials
            .iter()
            .map(|(k, v)| (*v, k.as_str()))
            .collect();
        assert_eq!(rows.len(), 288);
        let mut total = 0;
        for (state, mask, count, expected) in rows {
            let quads = assets.block_models.pane(state, mask).unwrap();
            assert_eq!(quads.len(), count);
            let mut hash = 0xcbf29ce484222325u64;
            let mut add = |word: u32| {
                hash = (hash ^ u64::from(word)).wrapping_mul(0x100000001b3);
            };
            for q in quads {
                for b in names[&q.layer].bytes() {
                    add(u32::from(b));
                }
                add(q.facing as u32);
                add(q.cull_face.unwrap_or(6) as u32);
                add(u32::from(q.shade));
                for v in q.vertices {
                    for word in v.position.to_array().into_iter().chain(v.uv) {
                        add(word.to_bits());
                    }
                }
            }
            assert_eq!(
                hash.to_string(),
                expected,
                "state {state} connections {mask}"
            );
            total += count;
        }
        assert_eq!(total, 2800);
    }
    #[test]
    #[ignore = "requires the locally owned 1.8.9 resource JAR"]
    fn original_facebakery_torch_model_90_quads_match_positions_uv_and_layers() {
        let jar = crate::minecraft_dir().join("versions/1.8.9/1.8.9.jar");
        let assets = super::super::Assets::load(&jar).unwrap();
        let rows: Vec<Value> = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-torch-model.json"
        ))
        .unwrap();
        let mut total = 0;
        for row in rows {
            let state = row[0].as_u64().unwrap() as u16;
            let expected = row[2].as_array().unwrap();
            let quads = assets.block_models.get(state).unwrap();
            assert_eq!(quads.len(), expected.len());
            let texture = row[1].as_str().unwrap().strip_prefix("blocks/").unwrap();
            for (i, (quad, data)) in quads.iter().zip(expected).enumerate() {
                assert_eq!(quad.layer, assets.materials[texture]);
                assert_eq!(quad.shade, data[3].as_u64().unwrap() as u8);
                assert_eq!(quad.facing, data[1].as_u64().unwrap() as usize);
                for (j, (vertex, raw)) in quad
                    .vertices
                    .iter()
                    .zip(data[2].as_array().unwrap())
                    .enumerate()
                {
                    let actual = vertex
                        .position
                        .to_array()
                        .into_iter()
                        .chain(vertex.uv)
                        .map(f32::to_bits)
                        .collect::<Vec<_>>();
                    let want = raw
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_u64().unwrap() as u32)
                        .collect::<Vec<_>>();
                    assert_eq!(actual, want, "state {state} quad {i} vertex {j}");
                }
                total += 1;
            }
        }
        assert_eq!(total, 90);
    }
    #[test]
    #[ignore = "requires the locally owned 1.8.9 resource JAR"]
    fn original_bed_head_foot_and_occupied_model_vertices_match() {
        type Quad = (String, usize, [[u32; 5]; 4], u32);
        let rows: Vec<(u16, Vec<Quad>)> =
            serde_json::from_str(include_str!("../tests/fixtures/vanilla189-bed-model.json"))
                .unwrap();
        let jar = crate::minecraft_dir().join("versions/1.8.9/1.8.9.jar");
        let assets = super::super::Assets::load(&jar).unwrap();
        let mut count = 0;
        for (state, expected) in rows {
            let quads = assets.block_models.get(state).unwrap();
            assert_eq!(quads.len(), expected.len());
            for (q, (texture, side, vertices, color)) in quads.iter().zip(expected) {
                assert_eq!(q.facing, side);
                assert_eq!(q.shade, color as u8);
                assert_eq!(
                    q.layer,
                    assets.materials[texture.strip_prefix("blocks/").unwrap()]
                );
                for (v, r) in q.vertices.iter().zip(vertices) {
                    assert_eq!(
                        [v.position.x, v.position.y, v.position.z, v.uv[0], v.uv[1]]
                            .map(f32::to_bits),
                        r,
                        "bed {state} face {side}"
                    );
                }
                count += 1;
            }
        }
        assert_eq!(count, 60);
    }
}
