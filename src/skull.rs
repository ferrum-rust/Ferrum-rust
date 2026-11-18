//! TileEntitySkullRenderer and LayerCustomHead geometry and skin resolution for 1.8.9 skulls.
use crate::{
    mesh::Mesh,
    model_box::{self, Texture},
    models::{ModelContext, ATLAS},
    player_skins::resolve_skull_skin,
    resources::Resources,
    vanilla::{drops::DropNbt as Tag, item_nbt},
    world::World,
};
use glam::{IVec3, Mat4, Vec3};

fn read_tile(tile: Option<&Tag>) -> (u8, i8, Option<&Tag>) {
    let Some(Tag::Compound(fields)) = tile else {
        return (0, 0, None);
    };
    let skull_type = item_nbt::number(fields.get("SkullType")).clamp(0, 255) as u8;
    let rot = item_nbt::number(fields.get("Rot")) as i8;
    let owner = fields.get("Owner").or_else(|| fields.get("ExtraType"));
    (skull_type, rot, owner)
}

fn placement_matrix(pos: IVec3, state: u16, rot: i8) -> Mat4 {
    let facing = (state & 7) % 6;
    let (offset, yaw_deg) = match facing {
        1 => (
            Vec3::new(0.5, 0.0, 0.5),
            (i32::from(rot) * 360) as f32 / 16.0,
        ),
        2 => (Vec3::new(0.5, 0.25, 0.74), 0.0),
        3 => (Vec3::new(0.5, 0.25, 0.26), 180.0),
        4 => (Vec3::new(0.74, 0.25, 0.5), 270.0),
        _ => (Vec3::new(0.26, 0.25, 0.5), 90.0),
    };
    Mat4::from_translation(pos.as_vec3() + offset)
        * Mat4::from_scale(Vec3::new(-1., -1., 1.))
        * Mat4::from_rotation_y(yaw_deg.to_radians())
        * Mat4::from_scale(Vec3::splat(1. / 16.))
}

fn skull_texture(
    skull_type: u8,
    owner: Option<&Tag>,
    r: &Resources,
    presentation: Option<&crate::net_presentation::State>,
) -> Texture {
    if skull_type == 3 {
        let region = resolve_skull_skin(owner, r, presentation);
        return Texture {
            layer: region.layer | ATLAS,
            origin: region.origin,
            scale: [region.scale[0] / 64., region.scale[1] / 64.],
        };
    }
    let (path, tex_h) = match skull_type {
        1 => ("textures/entity/skeleton/wither_skeleton.png", 32.),
        2 => ("textures/entity/zombie/zombie.png", 64.),
        4 => ("textures/entity/creeper/creeper.png", 32.),
        _ => ("textures/entity/skeleton/skeleton.png", 32.),
    };
    let sprite = r.sprite(path);
    Texture {
        layer: sprite.layer | ATLAS,
        origin: [0., 0.],
        scale: [
            sprite.width as f32 / (64. * 256.),
            sprite.height as f32 / (tex_h * 256.),
        ],
    }
}

fn emit_skull(
    mesh: &mut Mesh,
    transform: Mat4,
    skull_type: u8,
    texture: Texture,
    light: [u8; 4],
    extra_flags: u32,
) {
    if extra_flags == 0 {
        model_box::draw_inflated(
            mesh,
            transform,
            [-4., -8., -4.],
            [8., 8., 8.],
            0.,
            [0., 0.],
            texture,
            light,
            false,
        );
        if matches!(skull_type, 2 | 3) {
            model_box::draw_inflated(
                mesh,
                transform,
                [-4., -8., -4.],
                [8., 8., 8.],
                0.25,
                [32., 0.],
                texture,
                light,
                true,
            );
        }
    } else {
        model_box::draw_inflated_tinted(
            mesh,
            transform,
            [-4., -8., -4.],
            [8., 8., 8.],
            0.,
            [0., 0.],
            texture,
            light,
            false,
            extra_flags,
        );
        if matches!(skull_type, 2 | 3) {
            model_box::draw_inflated_tinted(
                mesh,
                transform,
                [-4., -8., -4.],
                [8., 8., 8.],
                0.25,
                [32., 0.],
                texture,
                light,
                true,
                extra_flags,
            );
        }
    }
}

pub(crate) fn visible(c: &ModelContext<'_>, mesh: &mut Mesh) {
    let cam_cx = (c.eye.x.floor() as i32).div_euclid(16);
    let cam_cz = (c.eye.z.floor() as i32).div_euclid(16);
    for dz in -5..=5 {
        for dx in -5..=5 {
            let (cx, cz) = (cam_cx + dx, cam_cz + dz);
            let Some(chunk) = c.world.chunks.get(&(cx, cz)) else {
                continue;
            };
            let cdx = (cx * 16 + 8) as f32 - c.eye.x;
            let cdz = (cz * 16 + 8) as f32 - c.eye.z;
            if cdx * cdx + cdz * cdz > 76.0 * 76.0 {
                continue;
            }
            let positions = chunk.skull_positions();
            if positions.is_empty() {
                continue;
            }
            for &idx in positions {
                let lx = i32::from(idx & 15);
                let lz = i32::from((idx >> 4) & 15);
                let y = i32::from(idx >> 8);
                let state = chunk.get(lx, y, lz);
                if state >> 4 != 144 {
                    continue;
                }
                let pos = IVec3::new(cx * 16 + lx, y, cz * 16 + lz);
                let at = pos.as_vec3();
                if (at + Vec3::splat(0.5)).distance_squared(c.eye) >= 4096.
                    || !c.frustum.visible(at, at + Vec3::ONE)
                {
                    continue;
                }
                let (skull_type, rot, owner) = read_tile(c.world.tile(pos));
                let mtx = placement_matrix(pos, state, rot);
                let light = c.world.visual_light_uv(pos, 0);
                let tex = skull_texture(skull_type, owner, c.resources, Some(&c.sim.presentation));
                emit_skull(mesh, mtx, skull_type, tex, light, 0);
            }
        }
    }
}

pub(crate) fn damage(mesh: &mut Mesh, world: &World, pos: IVec3, layer: u32) {
    let state = world.get(pos);
    if state >> 4 != 144 {
        return;
    }
    let (skull_type, rot, _) = read_tile(world.tile(pos));
    let mtx = placement_matrix(pos, state, rot);
    let v_scale = if matches!(skull_type, 2 | 3) {
        1. / 32.
    } else {
        1. / 16.
    };
    let tex = Texture {
        layer,
        origin: [0.25, 0.125],
        scale: [1. / 16., v_scale],
    };
    emit_skull(mesh, mtx, skull_type, tex, [240, 240, 0, 0], 0);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_equipped(
    mesh: &mut Mesh,
    matrix: Mat4,
    skull_type: u8,
    owner: Option<&Tag>,
    r: &Resources,
    presentation: Option<&crate::net_presentation::State>,
    light: [u8; 4],
    hurt: bool,
) {
    let mtx = matrix * Mat4::from_scale(Vec3::splat(1.1875 / 16.));
    let tex = skull_texture(skull_type, owner, r, presentation);
    let extra_flags = if hurt { crate::entity_visual::HURT } else { 0 };
    emit_skull(mesh, mtx, skull_type, tex, light, extra_flags);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models;

    #[test]
    fn wall_and_floor_skull_transforms_face_expected_directions_and_emit_hat_backfaces() {
        let tex = Texture {
            layer: 7,
            origin: [0., 0.],
            scale: [1. / 64., 1. / 64.],
        };
        for (meta, rot, expected_front_normal, expected_z_or_x_back) in [
            (2u16, 0i8, Vec3::NEG_Z, (2usize, 0.99f32)),
            (3u16, 0i8, Vec3::Z, (2usize, 0.01f32)),
            (4u16, 0i8, Vec3::NEG_X, (0usize, 0.99f32)),
            (5u16, 0i8, Vec3::X, (0usize, 0.01f32)),
            (1u16, 0i8, Vec3::NEG_Z, (1usize, 0.0f32)),
            (1u16, 4i8, Vec3::X, (1usize, 0.0f32)),
            (1u16, 8i8, Vec3::Z, (1usize, 0.0f32)),
            (1u16, 12i8, Vec3::NEG_X, (1usize, 0.0f32)),
        ] {
            let mut mesh = models::empty();
            let mtx = placement_matrix(IVec3::ZERO, (144 << 4) | meta, rot);
            emit_skull(&mut mesh, mtx, 0, tex, [240, 240, 0, 0], 0);
            assert_eq!(mesh.quads(), 6);
            // Front face in model_box is face index 4 (vertices 16..20).
            let v0 = Vec3::from_array(mesh.vertices[16].position);
            let v1 = Vec3::from_array(mesh.vertices[17].position);
            let v2 = Vec3::from_array(mesh.vertices[18].position);
            let normal = (v1 - v0).cross(v2 - v0).normalize();
            assert!(
                normal.distance(expected_front_normal) < 1e-5,
                "meta={meta} rot={rot}: got {normal:?}, expected {expected_front_normal:?}"
            );
            let (axis, back_coord) = expected_z_or_x_back;
            if meta >= 2 {
                // Back face is face index 5 (vertices 20..24).
                let back_v = mesh.vertices[20].position[axis];
                assert!(
                    (back_v - back_coord).abs() < 1e-5,
                    "meta={meta}: back={back_v} vs {back_coord}"
                );
            } else {
                let min_y = mesh
                    .vertices
                    .iter()
                    .map(|v| v.position[1])
                    .fold(f32::INFINITY, f32::min);
                let max_y = mesh
                    .vertices
                    .iter()
                    .map(|v| v.position[1])
                    .fold(f32::NEG_INFINITY, f32::max);
                assert!((min_y - 0.0).abs() < 1e-5 && (max_y - 0.5).abs() < 1e-5);
            }
        }

        // Player skull (type 3) emits 6 inner + 12 double-sided outer hat quads = 18 quads.
        let mut player_mesh = models::empty();
        let head_bone = crate::player::third_person::root(Vec3::ZERO, 0.0);
        emit_skull(
            &mut player_mesh,
            head_bone * Mat4::from_scale(Vec3::splat(1.1875 / 16.)),
            3,
            tex,
            [0, 240, 0, 0],
            crate::entity_visual::HURT,
        );
        assert_eq!(player_mesh.quads(), 18);
        assert!(player_mesh
            .vertices
            .iter()
            .all(|v| v.layer & crate::entity_visual::HURT != 0));
        // At yaw = 0 (facing +Z South), equipped skull front face points +Z.
        let fv0 = Vec3::from_array(player_mesh.vertices[16].position);
        let fv1 = Vec3::from_array(player_mesh.vertices[17].position);
        let fv2 = Vec3::from_array(player_mesh.vertices[18].position);
        let fnormal = (fv1 - fv0).cross(fv2 - fv0).normalize();
        assert!(fnormal.distance(Vec3::Z) < 1e-5, "equipped normal: {fnormal:?}");
    }
}
