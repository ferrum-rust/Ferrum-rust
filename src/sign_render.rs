//! TileEntitySignRenderer and ModelSign geometry, 4-line text, and crack overlay for 1.8.9 signs.
use crate::{
    mesh::Mesh,
    model_box::{self, Texture},
    models::{ModelContext, ATLAS},
    vanilla::drops::DropNbt,
    world::World,
};
use glam::{IVec3, Mat4, Vec3};

const F3: f32 = 2.0 / 3.0;

fn base_matrix(pos: IVec3, id: u16, meta: u16) -> Mat4 {
    let center = Mat4::from_translation(pos.as_vec3() + Vec3::splat(0.5));
    if id == 63 {
        let angle = -((meta as f32) * 360.0 / 16.0).to_radians();
        center * Mat4::from_rotation_y(angle)
    } else {
        let deg = match meta {
            2 => 180.0_f32,
            4 => 90.0,
            5 => -90.0,
            _ => 0.0,
        };
        center
            * Mat4::from_rotation_y((-deg).to_radians())
            * Mat4::from_translation(Vec3::new(0.0, -0.3125, -0.4375))
    }
}

fn emit_sign_boxes(mesh: &mut Mesh, base: Mat4, id: u16, tex: Texture, light: [u8; 4]) {
    let box_mat = base * Mat4::from_scale(Vec3::new(F3, -F3, -F3) / 16.0);
    model_box::draw(
        mesh,
        box_mat,
        [-12.0, -14.0, -1.0],
        [24.0, 12.0, 2.0],
        [0.0, 0.0],
        tex,
        light,
    );
    if id == 63 {
        model_box::draw(
            mesh,
            box_mat,
            [-1.0, -2.0, -1.0],
            [2.0, 14.0, 2.0],
            [0.0, 14.0],
            tex,
            light,
        );
    }
}

pub fn visible(c: &ModelContext<'_>, opaque: &mut Mesh, translucent: &mut Mesh) {
    let s = c.resources.sprite("textures/entity/sign.png");
    let tex = Texture {
        layer: s.layer | ATLAS,
        origin: [0.0, 0.0],
        scale: [
            s.width as f32 / (64.0 * 256.0),
            s.height as f32 / (32.0 * 256.0),
        ],
    };
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
            let positions = chunk.sign_positions();
            if positions.is_empty() {
                continue;
            }
            for &idx in positions {
                let lx = i32::from(idx & 15);
                let lz = i32::from((idx >> 4) & 15);
                let y = i32::from(idx >> 8);
                let state = chunk.get(lx, y, lz);
                let id = state >> 4;
                if !matches!(id, 63 | 68) {
                    continue;
                }
                let pos = IVec3::new(cx * 16 + lx, y, cz * 16 + lz);
                let at = pos.as_vec3();
                if (at + Vec3::splat(0.5)).distance_squared(c.eye) >= 4096.0
                    || !c
                        .frustum
                        .visible(at - Vec3::splat(0.25), at + Vec3::splat(1.25))
                {
                    continue;
                }
                let meta = state & 15;
                let base = base_matrix(pos, id, meta);
                let light = c.world.visual_light_uv(pos, 0);
                emit_sign_boxes(opaque, base, id, tex, light);

                if let Some(DropNbt::Compound(map)) = c.world.tile(pos) {
                    let mut rows: [Option<String>; 4] = Default::default();
                    let mut any_text = false;
                    for (row, key) in ["Text1", "Text2", "Text3", "Text4"].into_iter().enumerate() {
                        let Some(DropNbt::String(raw)) = map.get(key) else {
                            continue;
                        };
                        if raw.is_empty() || raw == "\"\"" {
                            continue;
                        }
                        let runs = crate::ui::chat::components::parse(raw, |k| {
                            c.resources.tr(&c.settings.language, k)
                        });
                        let formatted = crate::ui::chat::components::formatted(&runs);
                        if !formatted.is_empty() {
                            rows[row] = Some(formatted);
                            any_text = true;
                        }
                    }
                    if any_text {
                        let text_origin =
                            base.transform_point3(Vec3::new(0.0, 0.5 * F3, 0.073 * F3));
                        let right = base.transform_vector3(Vec3::X);
                        let up = base.transform_vector3(Vec3::Y);
                        let normal = base.transform_vector3(Vec3::Z).normalize_or_zero();
                        let scale = 0.015625 * F3;
                        for (row, line) in rows.into_iter().enumerate() {
                            if let Some(text) = line {
                                let y_px = (row as f32) * 10.0 - 20.0;
                                crate::ui::world_sign_line(
                                    translucent,
                                    c.resources,
                                    c.settings,
                                    text_origin,
                                    right,
                                    up,
                                    normal,
                                    scale,
                                    y_px,
                                    &text,
                                    [light[0], light[1]],
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn damage(mesh: &mut Mesh, world: &World, pos: IVec3, layer: u32) {
    let state = world.get(pos);
    let id = state >> 4;
    if !matches!(id, 63 | 68) {
        return;
    }
    let base = base_matrix(pos, id, state & 15);
    let tex = Texture {
        layer,
        origin: [0.25, 0.125],
        scale: [1.0 / 16.0, 1.0 / 16.0],
    };
    emit_sign_boxes(mesh, base, id, tex, world.visual_light_uv(pos, 0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standing_and_wall_signs_emit_expected_boxes_and_front_normals() {
        let tex = Texture {
            layer: 3,
            origin: [0.0, 0.0],
            scale: [1.0 / 64.0, 1.0 / 32.0],
        };
        let mut standing = crate::models::empty();
        emit_sign_boxes(
            &mut standing,
            base_matrix(IVec3::ZERO, 63, 0),
            63,
            tex,
            [0, 240, 0, 0],
        );
        assert_eq!(standing.visible_faces, 12);
        assert_eq!(standing.vertices.len(), 48);

        for (meta, expected_normal) in [
            (2u16, Vec3::NEG_Z),
            (3u16, Vec3::Z),
            (4u16, Vec3::NEG_X),
            (5u16, Vec3::X),
        ] {
            let base = base_matrix(IVec3::new(4, 65, 9), 68, meta);
            let normal = base.transform_vector3(Vec3::Z).normalize_or_zero();
            assert!(
                normal.distance(expected_normal) < 1e-5,
                "wall sign meta={meta}: {normal:?} != {expected_normal:?}"
            );
            let mut wall = crate::models::empty();
            emit_sign_boxes(&mut wall, base, 68, tex, [0, 240, 0, 0]);
            assert_eq!(wall.visible_faces, 6);
            assert_eq!(wall.vertices.len(), 24);
        }
    }
}
