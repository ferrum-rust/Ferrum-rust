//! Six original ModelRenderer parts, including the mirrored leg and expanded
//! lower torso. Positions and UVs come from the original model's built quads.
use super::{pose, triangle};
use crate::mesh::{Mesh, Vertex};
use glam::{Mat4, Vec3};
type ModelPart = ([f32; 3], &'static [[[f32; 5]; 4]]);
include!("golem_mesh_data.rs");

#[derive(Clone, Copy)]
pub struct Frame {
    pub position: Vec3,
    pub yaw: f32,
    pub head_yaw: f32,
    pub head_pitch: f32,
    pub swing: f32,
    pub limb_amount: f32,
    pub previous_amount: f32,
    pub partial: f32,
    pub attack: i32,
    pub rose: i32,
}
pub fn draw(mesh: &mut Mesh, frame: Frame, layer: u32, light: [u8; 4], color: [u8; 4]) {
    let amount =
        frame.previous_amount + (frame.limb_amount - frame.previous_amount) * frame.partial;
    let swing = frame.swing - frame.limb_amount * (1. - frame.partial);
    let [head_x, head_y, right_arm, left_arm, right_leg, left_leg] = pose(
        swing,
        amount.min(1.),
        frame.partial,
        frame.head_yaw,
        frame.head_pitch,
        frame.attack,
        frame.rose,
    );
    let sway = if f64::from(frame.limb_amount) >= 0.01 {
        (6.5 * triangle(swing + 6., 13.)).to_radians()
    } else {
        0.
    };
    let base = Mat4::from_translation(frame.position)
        * Mat4::from_rotation_y((180. - frame.yaw).to_radians())
        * Mat4::from_rotation_z(sway)
        * Mat4::from_scale(Vec3::new(-1., -1., 1.))
        * Mat4::from_translation(Vec3::new(0., -1.5078125, 0.))
        * Mat4::from_scale(Vec3::splat(0.0625));
    for ((pivot, quads), (yaw, pitch)) in PARTS.iter().zip([
        (head_y, head_x),
        (0., 0.),
        (0., right_arm),
        (0., left_arm),
        (0., right_leg),
        (0., left_leg),
    ]) {
        let matrix = base
            * Mat4::from_translation(Vec3::from_array(*pivot))
            * Mat4::from_rotation_y(yaw)
            * Mat4::from_rotation_x(pitch);
        for quad in *quads {
            let first = mesh.vertices.len() as u32;
            for [x, y, z, u, v] in quad {
                mesh.vertices.push(Vertex {
                    position: matrix.transform_point3(Vec3::new(*x, *y, *z)).to_array(),
                    uv: [u * 0.5, v * 0.5],
                    layer,
                    color,
                    light,
                });
            }
            mesh.indices.extend([0, 1, 2, 0, 2, 3].map(|n| first + n));
            mesh.visible_faces += 1;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw_living(
    mesh: &mut Mesh,
    frame: Frame,
    roll_z: f32,
    sprite: crate::resources::Sprite,
    flags: u32,
    light: [u8; 4],
    assets: &crate::assets::Assets,
    resources: &crate::resources::Resources,
) {
    let amount =
        frame.previous_amount + (frame.limb_amount - frame.previous_amount) * frame.partial;
    let swing = frame.swing - frame.limb_amount * (1. - frame.partial);
    let [head_x, head_y, right_arm, left_arm, right_leg, left_leg] = pose(
        swing,
        amount.min(1.),
        frame.partial,
        frame.head_yaw,
        frame.head_pitch,
        frame.attack,
        frame.rose,
    );
    let sway = if f64::from(frame.limb_amount) >= 0.01 {
        (6.5 * triangle(swing + 6., 13.)).to_radians()
    } else {
        0.
    };
    let base = Mat4::from_translation(frame.position)
        * Mat4::from_rotation_y((180. - frame.yaw).to_radians())
        * Mat4::from_rotation_z(roll_z + sway)
        * Mat4::from_scale(Vec3::new(-1., -1., 1.))
        * Mat4::from_translation(Vec3::new(0., -1.5078125, 0.))
        * Mat4::from_scale(Vec3::splat(0.0625));
    let uv_scale = [
        sprite.width as f32 / 256.,
        sprite.height as f32 / 256.,
    ];
    let layer = sprite.layer | flags;
    let mut right_arm_matrix = base;
    for (idx, ((pivot, quads), (yaw, pitch))) in PARTS
        .iter()
        .zip([
            (head_y, head_x),
            (0., 0.),
            (0., right_arm),
            (0., left_arm),
            (0., right_leg),
            (0., left_leg),
        ])
        .enumerate()
    {
        let matrix = base
            * Mat4::from_translation(Vec3::from_array(*pivot))
            * Mat4::from_rotation_y(yaw)
            * Mat4::from_rotation_x(pitch);
        if idx == 2 {
            right_arm_matrix = matrix;
        }
        for quad in *quads {
            let pts = quad.map(|[x, y, z, _, _]| matrix.transform_point3(Vec3::new(x, y, z)));
            let normal = (pts[1] - pts[0]).cross(pts[2] - pts[0]).normalize_or_zero();
            let shade = (crate::models::entity_face_shade(normal) * 255.)
                .round()
                .clamp(0., 255.) as u8;
            let first = mesh.vertices.len() as u32;
            for (pos, [_, _, _, u, v]) in pts.into_iter().zip(*quad) {
                mesh.vertices.push(Vertex {
                    position: pos.to_array(),
                    uv: [u * uv_scale[0], v * uv_scale[1]],
                    layer,
                    color: [shade, shade, shade, 255],
                    light,
                });
            }
            mesh.indices.extend([0, 1, 2, 0, 2, 3].map(|n| first + n));
            mesh.visible_faces += 1;
        }
    }
    if frame.rose > 0 {
        let hurt = flags & crate::entity_visual::HURT;
        // LayerIronGolemFlower attaches a 0.5-block Poppy (38:0) to the outstretched right hand.
        let flower_transform = right_arm_matrix
            * Mat4::from_translation(Vec3::new(-11., 24., -3.))
            * Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2)
            * Mat4::from_scale(Vec3::splat(8.))
            * Mat4::from_translation(Vec3::new(-0.5, 0., -0.5));
        if let Some(faces) = assets.block_models.carried(38 << 4) {
            let reverse = flower_transform.determinant() < 0.;
            for f in faces {
                let first = mesh.vertices.len() as u32;
                for v in f.vertices {
                    mesh.vertices.push(Vertex {
                        position: flower_transform.transform_point3(v.position).to_array(),
                        uv: v.uv,
                        layer: f.layer | hurt,
                        color: [f.shade, f.shade, f.shade, 255],
                        light,
                    });
                }
                let order = if reverse {
                    [0, 2, 1, 0, 3, 2]
                } else {
                    [0, 1, 2, 0, 2, 3]
                };
                mesh.indices.extend(order.map(|n| first + n));
                mesh.visible_faces += 1;
            }
        } else {
            let pop = crate::inventory::Stack::new(38, 1);
            let item_start = mesh.vertices.len();
            let arm_world = Mat4::from_translation(frame.position)
                * Mat4::from_rotation_y((180. - frame.yaw).to_radians())
                * Mat4::from_rotation_z(roll_z + sway)
                * Mat4::from_scale(Vec3::new(-1., -1., 1.))
                * Mat4::from_translation(Vec3::new(0., -1.5078125 - 7. / 16., 0.))
                * Mat4::from_rotation_x(right_arm)
                * Mat4::from_translation(Vec3::new(-10. / 16., 18. / 16., 0.));
            crate::models::third_person_item(
                mesh,
                arm_world,
                &pop,
                assets,
                resources,
                false,
                false,
            );
            for v in &mut mesh.vertices[item_start..] {
                v.light[..2].copy_from_slice(&light[..2]);
                v.layer |= hurt;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_model_box_geometry_keeps_all_six_parts_and_original_sprite_uvs() {
        let mut mesh = crate::models::empty();
        draw(
            &mut mesh,
            Frame {
                position: Vec3::ZERO,
                yaw: 180.,
                head_yaw: 0.,
                head_pitch: 0.,
                swing: 0.,
                limb_amount: 0.,
                previous_amount: 0.,
                partial: 0.,
                attack: 0,
                rose: 0,
            },
            77,
            [48, 160, 0, 0],
            [255; 4],
        );
        assert_eq!(
            PARTS.iter().map(|p| p.1.len()).collect::<Vec<_>>(),
            [12, 12, 6, 6, 6, 6]
        );
        assert_eq!(mesh.vertices.len(), 192);
        assert_eq!(mesh.indices.len(), 288);
        assert_eq!(mesh.vertices[0].uv, [0.09375, 0.03125]);
        assert_eq!(mesh.vertices[0].position, [-0.25, 2.6953125, 0.03125]);
        assert!(mesh
            .vertices
            .iter()
            .all(|p| p.layer == 77 && p.light == [48, 160, 0, 0]));
    }
}
