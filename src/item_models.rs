//! Model-space quads baked by the original ItemModelMesher, before any display transform.
use super::*;
use crate::vanilla::{math, random::JavaRandom};
use serde::Deserialize;
use std::{collections::HashMap, sync::LazyLock};

const ITEM: u32 = NO_MIPMAP;
type RawFace = (String, u32, i16, i8, [i32; 20]);
#[derive(Deserialize)]
struct GroundScale {
    y: f32,
    z: f32,
}
#[derive(Deserialize)]
struct Raw(bool, bool, GroundScale, [[f32; 16]; 3], Vec<RawFace>);
#[derive(Deserialize)]
struct RawCatalog {
    items: HashMap<String, usize>,
    models: Vec<Raw>,
}
enum Texture {
    Block(String),
    Sprite(String),
    Banner,
}
struct Quad {
    texture: usize,
    tint: u32,
    tint_index: i16,
    normal: Vec3,
    vertices: [Vec3; 4],
    uv: [[f32; 2]; 4],
    // Original stitched-atlas coordinates for RenderItem.renderEffect's texture matrix.
    glint_uv: [[u8; 2]; 4],
}
struct Model {
    solid: bool,
    builtin: bool,
    scale: GroundScale,
    transforms: [Mat4; 3],
    quads: Vec<Quad>,
}
struct Catalog {
    items: HashMap<(u16, u16), usize>,
    variants: HashMap<String, usize>,
    models: Vec<Model>,
    textures: Vec<Texture>,
}
static DATA: LazyLock<Catalog> = LazyLock::new(|| {
    let raw: RawCatalog = serde_json::from_reader(flate2::read::GzDecoder::new(
        &include_bytes!("vanilla/world_item_models189.json.gz")[..],
    ))
    .expect("1.8.9 item models");
    let (mut ids, mut textures) = (HashMap::<String, usize>::new(), Vec::new());
    let models = raw
        .models
        .into_iter()
        .map(|Raw(solid, builtin, scale, matrices, faces)| {
            let quads = faces
                .into_iter()
                .map(|(name, tint, tint_index, facing, raw)| {
                    let texture = *ids.entry(name.clone()).or_insert_with(|| {
                        let i = textures.len();
                        textures.push(if let Some(n) = name.strip_prefix("blocks/") {
                            Texture::Block(n.to_owned())
                        } else if name.starts_with("creative/banner_") {
                            Texture::Banner
                        } else {
                            Texture::Sprite(format!("textures/{name}.png"))
                        });
                        i
                    });
                    let vertices = std::array::from_fn(|i| {
                        Vec3::new(
                            raw[i * 5] as f32,
                            raw[i * 5 + 1] as f32,
                            raw[i * 5 + 2] as f32,
                        ) / 65536.
                    });
                    let normal = match facing {
                        0 => Vec3::NEG_Y,
                        1 => Vec3::Y,
                        2 => Vec3::NEG_Z,
                        3 => Vec3::Z,
                        4 => Vec3::NEG_X,
                        5 => Vec3::X,
                        _ => (vertices[1] - vertices[0])
                            .cross(vertices[2] - vertices[0])
                            .normalize_or_zero(),
                    };
                    let uv = std::array::from_fn(|i| {
                        [
                            raw[i * 5 + 3] as f32 / 65536.,
                            raw[i * 5 + 4] as f32 / 65536.,
                        ]
                    });
                    let glint_uv = uv.map(|[u, v]| {
                        [
                            (u.clamp(0., 1.) * 255.).round() as u8,
                            (v.clamp(0., 1.) * 255.).round() as u8,
                        ]
                    });
                    Quad {
                        glint_uv,
                        texture,
                        tint,
                        tint_index,
                        normal,
                        vertices,
                        uv,
                    }
                })
                .collect();
            Model {
                solid,
                builtin,
                scale,
                transforms: matrices.map(|m| Mat4::from_cols_array(&m)),
                quads,
            }
        })
        .collect();
    let (mut items, mut variants) = (HashMap::new(), HashMap::new());
    for (key, i) in raw.items {
        if let Some((id, meta)) = key.split_once(':') {
            items.insert((id.parse().unwrap(), meta.parse().unwrap()), i);
        } else {
            variants.insert(key, i);
        }
    }
    Catalog {
        items,
        variants,
        models,
        textures,
    }
});
/// Bakes only the embedded original model catalog. Active-pack sprite layers
/// and block textures are still resolved by Model::render on each draw.
pub(super) fn warm_geometry() -> usize {
    LazyLock::force(&DATA).models.len()
}
fn model(stack: &Stack, variant: Option<&str>) -> Option<&'static Model> {
    let fallback_meta = if stack.id == 373 && stack.meta & 0x4000 != 0 {
        16385
    } else {
        0
    };
    let i = variant
        .and_then(|s| DATA.variants.get(s))
        .or_else(|| DATA.items.get(&(stack.id, stack.meta)))
        .or_else(|| DATA.items.get(&(stack.id, fallback_meta)))
        .or_else(|| DATA.items.get(&(stack.id, 0)))?;
    Some(&DATA.models[*i])
}
fn tint(stack: &Stack, q: &Quad) -> u32 {
    if q.tint_index == 0 && stack.id == 373 {
        return crate::ui::creative::potion_color_and_glint(stack).0;
    }
    if q.tint_index == 0 && (298..=301).contains(&stack.id) {
        if let Some(color) = stack
            .nbt
            .as_deref()
            .and_then(crate::vanilla::item_nbt::compound)
            .and_then(|t| t.get("display"))
            .and_then(crate::vanilla::item_nbt::compound)
            .and_then(|t| t.get("color"))
            .map(|t| crate::vanilla::item_nbt::number(Some(t)))
        {
            return color as u32;
        }
    }
    q.tint
}
impl Model {
    #[allow(clippy::too_many_arguments)]
    fn render(
        &self,
        mesh: &mut Mesh,
        frame: Mat4,
        normal_rotation: glam::Mat3,
        stack: &Stack,
        asset: &Assets,
        r: &Resources,
        flags: u32,
    ) {
        let matrix = frame
            * Mat4::from_scale(Vec3::splat(0.5))
            * if self.builtin {
                Mat4::from_rotation_y(std::f32::consts::PI)
            } else {
                Mat4::IDENTITY
            }
            * Mat4::from_translation(Vec3::splat(-0.5));
        let normal_matrix = normal_rotation * glam::Mat3::from_mat4(matrix).inverse().transpose();
        // Equal-depth grass/leather/potion overlay quads stay in the source order.
        let reverse = matrix.determinant() < 0.;
        let glows = crate::ui::creative::world_glint(stack);
        let banner = (stack.id == 425).then(|| r.banners.stack(r, stack));
        let skull_skin = (stack.id == 397 && stack.meta == 3).then(|| {
            crate::player_skins::resolve_skull_skin(
                crate::player_skins::item_skull_owner(stack.nbt.as_deref()),
                r,
                None,
            )
        });
        for q in &self.quads {
            let (layer, dimensions, origin) = match &DATA.textures[q.texture] {
                Texture::Block(name) => (
                    *asset.materials.get(name).unwrap_or(&0) | crate::models::NO_MIPMAP,
                    [1.; 2],
                    [0.; 2],
                ),
                Texture::Sprite(name) => {
                    if let Some(region) =
                        skull_skin.filter(|_| name == "textures/entity/steve.png")
                    {
                        (region.layer | ATLAS, region.scale, region.origin)
                    } else {
                        let s = r.sprite(name);
                        (
                            s.layer | ATLAS,
                            [s.width as f32 / 256., s.height as f32 / 256.],
                            [0.; 2],
                        )
                    }
                }
                Texture::Banner => {
                    let banner = banner.expect("banner texture belongs to the banner item");
                    (banner.layer | ATLAS, [0.25; 2], banner.origin)
                }
            };
            let normal = (normal_matrix * q.normal).normalize_or_zero();
            let light = crate::models::standard_item_light(normal);
            let [_, red, green, blue] = tint(stack, q).to_be_bytes();
            // Fixed-function lighting clamps the result per channel, after material tint.
            let color = [red, green, blue].map(|c| (f32::from(c) * light).round() as u8);
            let first = mesh.vertices.len() as u32;
            for j in 0..4 {
                mesh.vertices.push(Vertex {
                    position: matrix.transform_point3(q.vertices[j]).to_array(),
                    uv: [
                        origin[0] + q.uv[j][0] * dimensions[0],
                        origin[1] + q.uv[j][1] * dimensions[1],
                    ],
                    layer: layer | flags | ITEM | if glows { GLINT } else { 0 },
                    color: [color[0], color[1], color[2], 255],
                    light: if glows {
                        [0, 240, q.glint_uv[j][0], q.glint_uv[j][1]]
                    } else {
                        [0, 240, 0, 0]
                    },
                });
            }
            mesh.indices.extend(
                if reverse {
                    [0, 2, 1, 0, 3, 2]
                } else {
                    [0, 1, 2, 0, 2, 3]
                }
                .map(|i| first + i),
            );
            mesh.visible_faces += 1;
        }
    }
}
fn scale(s: f32) -> Mat4 {
    Mat4::from_scale(Vec3::splat(s))
}
fn rx(d: f32) -> Mat4 {
    Mat4::from_rotation_x(d.to_radians())
}
fn ry(d: f32) -> Mat4 {
    Mat4::from_rotation_y(d.to_radians())
}
fn rz(d: f32) -> Mat4 {
    Mat4::from_rotation_z(d.to_radians())
}
fn t(x: f32, y: f32, z: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(x, y, z))
}
pub(crate) fn third_person_item(
    mesh: &mut Mesh,
    arm_frame: Mat4,
    stack: &Stack,
    assets: &Assets,
    resources: &Resources,
    sneaking: bool,
    cast_line: bool,
) {
    let variant = (stack.id == 346 && cast_line).then_some("fishing_rod_cast");
    if let Some(m) = model(stack, variant) {
        let grip = arm_frame * t(-0.0625, 0.4375, 0.0625);
        let special = if matches!(stack.id, 54 | 130 | 146) {
            t(0., 0.1875, -0.3125)
                * rx(20.)
                * ry(45.)
                * Mat4::from_scale(Vec3::new(-0.375, -0.375, 0.375))
        } else {
            Mat4::IDENTITY
        };
        let frame = grip
            * special
            * t(0., if sneaking { 0.203125 } else { 0. }, 0.)
            * scale(2.)
            * m.transforms[1];
        m.render(mesh, frame, glam::Mat3::IDENTITY, stack, assets, resources, 0);
    }
}

fn first_transform(swing: f32, equip: f32) -> Mat4 {
    let a = std::f32::consts::PI;
    let f = math::sin(swing.sqrt() * a);
    t(0.56, -0.52, -0.71999997)
        * t(0., equip * -0.6, 0.)
        * ry(45.)
        * ry(-20. * math::sin(swing * swing * a))
        * rz(-20. * f)
        * rx(-80. * f)
        * scale(0.4)
}
pub(super) fn first_item(mesh: &mut Mesh, c: &ModelContext<'_>, stack: &Stack) {
    let swing = c.sim.hand_swing.sample(c.alpha);
    let elapsed = c
        .sim
        .equipped
        .use_ticks
        .map(|t| (t as f32 + c.alpha - 1.).max(0.));
    let equip = c.sim.equipped.lowered(c.alpha);
    let root = std::f32::consts::PI * swing.sqrt();
    let mtx = if let Some(f) = elapsed {
        if stack.id == 261 {
            let power = (f / 20.).mul_add(f / 20., 2. * f / 20.) / 3.;
            let amount = power.clamp(0., 1.);
            first_transform(0., equip)
                * rz(-18.)
                * ry(-12.)
                * rx(-8.)
                * t(-0.9, 0.2, 0.)
                * t(
                    0.,
                    if amount > 0.1 {
                        math::sin((f - 0.1) * 1.3) * (amount - 0.1) * 0.01
                    } else {
                        0.
                    },
                    amount * 0.1,
                )
                * Mat4::from_scale(Vec3::new(1., 1., 1. + amount * 0.2))
        } else if matches!(stack.id, 267 | 268 | 272 | 276 | 283) {
            first_transform(0., equip) * t(-0.5, 0.2, 0.) * ry(30.) * rx(-80.) * ry(60.)
        } else {
            let remaining = 32. - f;
            let fraction = remaining / 32.;
            let ease = 1. - fraction.powi(27);
            let bob = if fraction >= 0.8 {
                0.
            } else {
                (math::cos(remaining / 4. * std::f32::consts::PI) * 0.1).abs()
            };
            t(0., bob, 0.)
                * t(ease * 0.6, ease * -0.5, 0.)
                * ry(ease * 90.)
                * rx(ease * 10.)
                * rz(ease * 30.)
                * first_transform(0., equip)
        }
    } else {
        t(
            -0.4 * math::sin(root),
            0.2 * math::sin(root * 2.),
            -0.2 * math::sin(swing * std::f32::consts::PI),
        ) * first_transform(swing, equip)
    };
    let variant = if stack.id == 261 {
        elapsed.and_then(|f| {
            if f >= 18. {
                Some("bow_pulling_2")
            } else if f > 13. {
                Some("bow_pulling_1")
            } else if f > 0. {
                Some("bow_pulling_0")
            } else {
                None
            }
        })
    } else if stack.id == 346
        && (c.sim.fishing.hook.is_some() || c.sim.server_scene.local_fishing_cast)
    {
        Some("fishing_rod_cast")
    } else {
        None
    };
    if let Some(m) = model(stack, variant) {
        let normal_rotation = glam::Mat3::from_rotation_y(-c.player.yaw)
            * glam::Mat3::from_rotation_x(c.player.pitch);
        m.render(
            mesh,
            mtx * scale(2.) * m.transforms[0],
            normal_rotation,
            stack,
            c.assets,
            c.resources,
            crate::first_person::CAMERA_HAND,
        );
    }
}
fn copies(count: u8) -> usize {
    match count {
        49.. => 5,
        33..=48 => 4,
        17..=32 => 3,
        2..=16 => 2,
        _ => 1,
    }
}
pub(super) fn dropped(
    mesh: &mut Mesh,
    item: &crate::gameplay::Drop,
    alpha: f32,
    assets: &Assets,
    r: &Resources,
) {
    let pos = item.previous.lerp(item.position, f64::from(alpha)).as_vec3();
    dropped_at(mesh, item, pos, alpha, assets, r);
}
pub(super) fn dropped_at(
    mesh: &mut Mesh, item: &crate::gameplay::Drop, position: Vec3, alpha: f32, assets: &Assets, r: &Resources,
) {
    let Some(m) = model(&item.stack, None) else {
        return;
    };
    let age = item.age as f32 + alpha;
    let pos = position
        + Vec3::Y * (math::sin(age / 10. + item.hover_start) * 0.1 + 0.1 + 0.25 * m.scale.y);
    let n = copies(item.stack.count);
    let mut frame =
        Mat4::from_translation(pos) * Mat4::from_rotation_y(age / 20. + item.hover_start);
    if !m.solid {
        frame *= t(0., 0., -0.046875 * (n - 1) as f32 * 0.5);
    }
    let mut random = JavaRandom::new(187);
    for i in 0..n {
        let mut model_frame = frame;
        if m.solid {
            if i > 0 {
                let mut jitter = || (random.next_float() * 2. - 1.) * 0.15;
                model_frame *= t(jitter(), jitter(), jitter());
            }
            model_frame *= scale(0.5);
        }
        m.render(
            mesh,
            model_frame * m.transforms[2],
            glam::Mat3::IDENTITY,
            &item.stack,
            assets,
            r,
            0,
        );
        if !m.solid {
            frame *= t(0., 0., 0.046875 * m.scale.z);
        }
    }
}

pub(super) fn projectile(mesh: &mut Mesh, frame: Mat4, stack: &Stack, assets: &Assets, resources: &Resources) {
    if let Some(m) = model(stack, None) {
        m.render(
            mesh,
            frame * scale(0.5) * m.transforms[2],
            glam::Mat3::IDENTITY,
            stack,
            assets,
            resources,
            0,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_infested_items_use_their_host_stone_models_in_hand_and_on_the_ground() {
        for (meta, (id, damage)) in [(1, 0), (4, 0), (98, 0), (98, 1), (98, 2), (98, 3)].into_iter().enumerate() {
            let mut egg = Stack::new(97, 1);
            egg.meta = meta as u16;
            let mut host = Stack::new(id, 1);
            host.meta = damage;
            assert!(std::ptr::eq(model(&egg, None).unwrap(), model(&host, None).unwrap()), "monster egg {meta}");
        }
    }

    #[test]
    fn held_and_dropped_item_models_match_original_transforms_stack_copies_and_faces() {
        type First = (f32, f32, [f32; 16]);
        let (frames, counts): (Vec<First>, Vec<usize>) = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-world-item-transforms.json"
        ))
        .unwrap();
        for (swing, equip, expected) in frames {
            for (value, want) in first_transform(swing, equip)
                .to_cols_array()
                .iter()
                .zip(expected)
            {
                assert!(
                    (value - want).abs() < 0.000004,
                    "held swing={swing} equip={equip}: {value} vs {want}"
                );
            }
        }
        for (n, count) in counts.into_iter().enumerate() {
            assert_eq!(copies(n as u8), count);
        }
        let grass = model(&Stack::new(2, 1), None).unwrap();
        let face_data: Vec<_> = grass
            .quads
            .iter()
            .map(|q| {
                (
                    match &DATA.textures[q.texture] {
                        Texture::Block(s) => s.as_str(),
                        _ => panic!("block atlas expected"),
                    },
                    q.normal,
                    q.tint,
                )
            })
            .collect();
        assert_eq!(face_data[0], ("dirt", Vec3::NEG_Y, 0xffffff));
        assert_eq!(face_data[1], ("grass_top", Vec3::Y, 0x7cbd6b));
        for (pair, n) in
            face_data[2..]
                .chunks_exact(2)
                .zip([Vec3::NEG_Z, Vec3::Z, Vec3::NEG_X, Vec3::X])
        {
            assert_eq!(
                pair,
                [
                    ("grass_side", n, 0xffffff),
                    ("grass_side_overlay", n, 0x7cbd6b)
                ]
            );
        }
        let stairs = model(&Stack::new(53, 1), None).unwrap();
        assert!(stairs
            .quads
            .iter()
            .flat_map(|q| q.vertices)
            .any(|p| p.y == 0.5));
        let furnace = model(&Stack::new(61, 1), None).unwrap();
        assert!(furnace.quads.iter().any(
            |q| matches!(&DATA.textures[q.texture],Texture::Block(n) if n=="furnace_front_off")
                && q.normal == Vec3::NEG_Z
        ));
        let pickaxe = model(&Stack::new(257, 1), None).unwrap();
        assert!(!pickaxe.solid && pickaxe.quads.len() > 6);
    }
}
