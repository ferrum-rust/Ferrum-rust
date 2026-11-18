//! Procedural 1.8.9 `ModelHorse` (`128x128` texture sheet) with front-leg
//! rearing/pawing motion, stride knee articulation, foal/donkey/mule scaling,
//! coat markings, saddle/bridle/stirrups/reins, side chests, and horse armor.
use crate::{
    mesh::{Mesh, Vertex},
    models::ATLAS,
    resources::{Resources, Sprite},
};
use glam::{Mat4, Vec3};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct HorseAnim {
    pub prev_rearing: f32,
    pub rearing: f32,
    pub prev_head_lean: f32,
    pub head_lean: f32,
    pub prev_mouth: f32,
    pub mouth: f32,
    pub tail_ticks: u8,
    pub stand_ticks: u8,
    pub run_crazy_ticks: u8,
    pub gallop_ticks: u32,
}

#[derive(Clone, Copy)]
pub(crate) struct HorseDrawInput<'a> {
    pub feet: Vec3,
    pub body_yaw_rad: f32,
    pub head_yaw_deg: Option<f32>,
    pub pitch_deg: f32,
    pub limb_swing: f32,
    pub limb_amount: f32,
    pub age_in_ticks: f32,
    pub alpha: f32,
    pub roll_z: f32,
    pub subtype: u8, // 0=horse, 1=donkey, 2=mule, 3=zombie, 4=skeleton
    pub baby: bool,
    pub saddled: bool,
    pub chested: bool,
    pub ridden: bool,
    pub base_texture: &'a str,
    pub markings_texture: Option<&'a str>,
    pub horse_armor: u8, // 0=none, 1=iron, 2=gold, 3=diamond
    pub anim: HorseAnim,
    pub light: [u8; 4],
    pub hurt_flag: u32,
}

#[derive(Clone, Copy)]
struct Sheet {
    layer: u32,
    scale: [f32; 2],
}

impl Sheet {
    fn from_sprite(sprite: Sprite, extra_flags: u32) -> Self {
        Self {
            layer: sprite.layer | ATLAS | extra_flags,
            scale: [
                sprite.width as f32 / (128.0 * 256.0),
                sprite.height as f32 / (128.0 * 256.0),
            ],
        }
    }
}

fn horse_box(
    mesh: &mut Mesh,
    transform: Mat4,
    min: [f32; 3],
    size: [f32; 3],
    inflate: f32,
    uv: [f32; 2],
    sheet: Sheet,
    light: [u8; 4],
    double_sided: bool,
) {
    let [w, h, d] = size;
    let [u, v] = uv;
    let [x0, y0, z0] = [min[0] - inflate, min[1] - inflate, min[2] - inflate];
    let [x1, y1, z1] = [
        min[0] + w + inflate,
        min[1] + h + inflate,
        min[2] + d + inflate,
    ];
    let pts = [
        [x0, y0, z0],
        [x1, y0, z0],
        [x1, y1, z0],
        [x0, y1, z0],
        [x0, y0, z1],
        [x1, y0, z1],
        [x1, y1, z1],
        [x0, y1, z1],
    ]
    .map(|a| transform.transform_point3(Vec3::from_array(a)));
    let faces = [
        ([5, 1, 2, 6], [u + d + w, v + d, u + d + w + d, v + d + h]),
        ([0, 4, 7, 3], [u, v + d, u + d, v + d + h]),
        ([5, 4, 0, 1], [u + d, v, u + d + w, v + d]),
        ([2, 3, 7, 6], [u + d + w, v + d, u + d + w + w, v]),
        ([1, 0, 3, 2], [u + d, v + d, u + d + w, v + d + h]),
        (
            [4, 5, 6, 7],
            [u + d + w + d, v + d, u + d + w + d + w, v + d + h],
        ),
    ];
    for (face_idx, (indices, [u1, v1, u2, v2])) in faces.into_iter().enumerate() {
        // Zero-width planes (horse reins) only emit their +/-X faces.
        if w == 0.0 && inflate == 0.0 && face_idx >= 2 {
            continue;
        }
        let p0 = pts[indices[0]];
        let p1 = pts[indices[1]];
        let p2 = pts[indices[2]];
        let normal = (p1 - p0).cross(p2 - p0).normalize_or_zero();
        let shade = (crate::models::entity_face_shade(normal) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8;
        let corners: [([f32; 3], [f32; 2]); 4] = std::array::from_fn(|k| {
            let [s, t] = [[u2, v1], [u1, v1], [u1, v2], [u2, v2]][k];
            (pts[indices[k]].to_array(), [s * sheet.scale[0], t * sheet.scale[1]])
        });
        let first = mesh.vertices.len() as u32;
        for (position, uv) in corners {
            mesh.vertices.push(Vertex {
                position,
                uv,
                layer: sheet.layer,
                color: [shade, shade, shade, 255],
                light,
            });
        }
        mesh.indices.extend([0, 1, 2, 0, 2, 3].map(|k| first + k));
        mesh.visible_faces += 1;
        if double_sided {
            let back_shade = (crate::models::entity_face_shade(-normal) * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8;
            let back = mesh.vertices.len() as u32;
            for (position, uv) in corners {
                mesh.vertices.push(Vertex {
                    position,
                    uv,
                    layer: sheet.layer,
                    color: [back_shade, back_shade, back_shade, 255],
                    light,
                });
            }
            mesh.indices.extend([0, 2, 1, 0, 3, 2].map(|k| back + k));
            mesh.visible_faces += 1;
        }
    }
}

#[derive(Clone, Copy)]
struct HorsePose {
    body_mat: Mat4,
    tail_base_mat: Mat4,
    tail_mid_mat: Mat4,
    tail_tip_mat: Mat4,
    bl_leg_mat: Mat4,
    bl_shin_mat: Mat4,
    br_leg_mat: Mat4,
    br_shin_mat: Mat4,
    fl_leg_mat: Mat4,
    fl_shin_mat: Mat4,
    fr_leg_mat: Mat4,
    fr_shin_mat: Mat4,
    head_mat: Mat4,
    neck_rx: f32,
    mouth: f32,
}

fn compute_pose(input: &HorseDrawInput<'_>) -> HorsePose {
    let type_scale = match input.subtype {
        1 => 0.87,
        2 => 0.92,
        _ => 1.0,
    };
    let r = (input.anim.prev_rearing + (input.anim.rearing - input.anim.prev_rearing) * input.alpha)
        .clamp(0.0, 1.0);
    let lean = (input.anim.prev_head_lean
        + (input.anim.head_lean - input.anim.prev_head_lean) * input.alpha)
        .clamp(0.0, 1.0);
    let mouth = (input.anim.prev_mouth + (input.anim.mouth - input.anim.prev_mouth) * input.alpha)
        .clamp(0.0, 1.0);
    let anti_r = 1.0 - r;

    let body_yaw_deg = input.body_yaw_rad.to_degrees();
    let net_head_yaw_rad = input
        .head_yaw_deg
        .map(|hy| ((hy - body_yaw_deg + 180.0).rem_euclid(360.0) - 180.0).clamp(-20.0, 20.0))
        .unwrap_or(0.0)
        .to_radians();
    let mut head_pitch_rad = input.pitch_deg.clamp(-45.0, 45.0) / 57.295776;
    if input.limb_amount > 0.2 {
        head_pitch_rad += (input.limb_swing * 0.6662).cos() * 0.12 * input.limb_amount;
    }

    let root = Mat4::from_translation(input.feet)
        * Mat4::from_rotation_y(PI - input.body_yaw_rad)
        * Mat4::from_rotation_z(input.roll_z)
        * Mat4::from_scale(Vec3::new(-type_scale, -type_scale, type_scale))
        * Mat4::from_translation(Vec3::new(0.0, -1.5078125, 0.0))
        * Mat4::from_scale(Vec3::splat(0.0625));

    // ModelHorse.setLivingAnimations motion math
    let body_rx = -r * FRAC_PI_4;
    let walk_l = (input.limb_swing * 0.6662).cos();
    let walk_r = (input.limb_swing * 0.6662 + PI).cos();
    let walk_front_l = walk_l * 1.4 * input.limb_amount;
    let walk_front_r = walk_r * 1.4 * input.limb_amount;
    let walk_back_l = walk_r * 0.8 * input.limb_amount;
    let walk_back_r = walk_l * 0.8 * input.limb_amount;

    // Front-leg rearing & alternating pawing forward (`말이 발 앞으로 하는모션`)
    let paw = (input.age_in_ticks * 0.6 + PI).cos();
    let rear_fl_leg = -1.0471976 + paw * 0.45;
    let rear_fr_leg = -1.0471976 - paw * 0.45;
    let fl_leg_rx = rear_fl_leg * r + walk_front_l * anti_r;
    let fr_leg_rx = rear_fr_leg * r + walk_front_r * anti_r;

    // Natural stride knee bend when lifting forward + 45-deg forward knee bend while rearing/pawing
    let stride_knee_l = (-walk_l).max(0.0) * 0.85 * input.limb_amount;
    let stride_knee_r = (-walk_r).max(0.0) * 0.85 * input.limb_amount;
    let rear_fl_shin = rear_fl_leg + FRAC_PI_4 * (1.0 - paw * 0.25);
    let rear_fr_shin = rear_fr_leg + FRAC_PI_4 * (1.0 + paw * 0.25);
    let fl_shin_rx = rear_fl_shin * r + (walk_front_l + stride_knee_l) * anti_r;
    let fr_shin_rx = rear_fr_shin * r + (walk_front_r + stride_knee_r) * anti_r;

    // Back legs brace at r * 0.087 while blending out walk swing
    let bl_leg_rx = 0.08726646 * r + walk_back_l * anti_r;
    let br_leg_rx = 0.08726646 * r + walk_back_r * anti_r;
    let stride_hock_l = walk_r.max(0.0) * 0.5 * input.limb_amount;
    let stride_hock_r = walk_l.max(0.0) * 0.5 * input.limb_amount;
    let bl_shin_rx = 0.08726646 * r + (walk_back_l - stride_hock_l) * anti_r;
    let br_shin_rx = 0.08726646 * r + (walk_back_r - stride_hock_r) * anti_r;

    // Shoulder pivot lifts smoothly with the -45 deg rearing chest; shins track distal thigh knees
    let fl_shoulder_y = 9.0 * anti_r + (-2.0) * r;
    let fl_shoulder_z = -8.0 * anti_r + (-2.0) * r;
    // Note: sin(PI/2 + rx) == cos(rx) and cos(PI/2 - rx) == sin(rx)
    let fl_shin_y = fl_shoulder_y + (FRAC_PI_2 + fl_leg_rx).sin() * 7.0;
    let fl_shin_z = fl_shoulder_z + (FRAC_PI_2 - fl_leg_rx).cos() * 7.0;
    let fr_shin_y = fl_shoulder_y + (FRAC_PI_2 + fr_leg_rx).sin() * 7.0;
    let fr_shin_z = fl_shoulder_z + (FRAC_PI_2 - fr_leg_rx).cos() * 7.0;

    let bl_shin_y = 9.0 + (FRAC_PI_2 + bl_leg_rx).sin() * 7.0;
    let bl_shin_z = 11.0 + (FRAC_PI_2 - bl_leg_rx).cos() * 7.0;
    let br_shin_y = 9.0 + (FRAC_PI_2 + br_leg_rx).sin() * 7.0;
    let br_shin_z = 11.0 + (FRAC_PI_2 - br_leg_rx).cos() * 7.0;

    // Neck/head pivot smoothly moves from (0, 4, -10) to (0, -6, -4) during rearing (or down when grazing)
    let rest_blend = (1.0 - r.max(lean)).max(0.0);
    let neck_y = 4.0 * rest_blend + (-6.0) * r + 11.0 * (lean * anti_r);
    let neck_z = -10.0 * anti_r + (-4.0) * r;
    let neck_rx = (0.5235988 + head_pitch_rad) * rest_blend
        + (0.2617994 + head_pitch_rad) * r
        + 2.1816616 * (lean * anti_r);
    let neck_ry = net_head_yaw_rad * (1.0 - 0.5 * r);

    // Tail pose & swish
    let tail_pivot = Vec3::new(0.0, 3.0 * anti_r + 11.0 * r, 14.0 * anti_r + 12.0 * r);
    let walk_tail = if input.limb_amount > 0.01 {
        -1.0471976 + (input.limb_swing * 0.6662).cos() * 0.4 * input.limb_amount
    } else {
        -1.3089969
    };
    let tail_rx = walk_tail * anti_r + (-0.2617994) * r;
    let tail_ry = if input.anim.tail_ticks > 0 {
        (input.age_in_ticks * 0.7).cos() * 0.7
    } else {
        0.0
    };

    if input.baby {
        // ModelHorse foal proportions: 0.5x body, longer 0.7x legs touching feet, slightly larger 0.625x head
        let body_root = root * Mat4::from_translation(Vec3::new(0.0, 9.0, 0.0)) * Mat4::from_scale(Vec3::splat(0.5));
        let leg_scale = Vec3::new(0.5, 0.7, 0.5);
        let leg_root = root * Mat4::from_translation(Vec3::new(0.0, 24.125 * (1.0 - 0.7), 0.0));
        let mk_leg = |x: f32, y: f32, z: f32, rx: f32| {
            leg_root
                * Mat4::from_translation(Vec3::new(x * 0.5, y * 0.7, z * 0.5))
                * Mat4::from_rotation_x(rx)
                * Mat4::from_scale(leg_scale)
        };
        let head_mat = root
            * Mat4::from_translation(Vec3::new(0.0, 9.0 + neck_y * 0.5, neck_z * 0.5))
            * Mat4::from_rotation_y(neck_ry)
            * Mat4::from_rotation_x(neck_rx)
            * Mat4::from_scale(Vec3::splat(0.625));
        let tail_root = body_root
            * Mat4::from_translation(tail_pivot)
            * Mat4::from_rotation_y(tail_ry);
        HorsePose {
            body_mat: body_root
                * Mat4::from_translation(Vec3::new(0.0, 11.0, 9.0))
                * Mat4::from_rotation_x(body_rx),
            tail_base_mat: tail_root * Mat4::from_rotation_x(tail_rx),
            tail_mid_mat: tail_root * Mat4::from_rotation_x(tail_rx),
            tail_tip_mat: tail_root * Mat4::from_rotation_x(tail_rx - 0.2617994),
            bl_leg_mat: mk_leg(4.0, 9.0, 11.0, bl_leg_rx),
            bl_shin_mat: mk_leg(4.0, bl_shin_y, bl_shin_z, bl_shin_rx),
            br_leg_mat: mk_leg(-4.0, 9.0, 11.0, br_leg_rx),
            br_shin_mat: mk_leg(-4.0, br_shin_y, br_shin_z, br_shin_rx),
            fl_leg_mat: mk_leg(4.0, fl_shoulder_y, fl_shoulder_z, fl_leg_rx),
            fl_shin_mat: mk_leg(4.0, fl_shin_y, fl_shin_z, fl_shin_rx),
            fr_leg_mat: mk_leg(-4.0, fl_shoulder_y, fl_shoulder_z, fr_leg_rx),
            fr_shin_mat: mk_leg(-4.0, fr_shin_y, fr_shin_z, fr_shin_rx),
            head_mat,
            neck_rx,
            mouth,
        }
    } else {
        let mk_part = |x: f32, y: f32, z: f32, rx: f32| {
            root * Mat4::from_translation(Vec3::new(x, y, z)) * Mat4::from_rotation_x(rx)
        };
        let tail_root = root
            * Mat4::from_translation(tail_pivot)
            * Mat4::from_rotation_y(tail_ry);
        HorsePose {
            body_mat: mk_part(0.0, 11.0, 9.0, body_rx),
            tail_base_mat: tail_root * Mat4::from_rotation_x(tail_rx),
            tail_mid_mat: tail_root * Mat4::from_rotation_x(tail_rx),
            tail_tip_mat: tail_root * Mat4::from_rotation_x(tail_rx - 0.2617994),
            bl_leg_mat: mk_part(4.0, 9.0, 11.0, bl_leg_rx),
            bl_shin_mat: mk_part(4.0, bl_shin_y, bl_shin_z, bl_shin_rx),
            br_leg_mat: mk_part(-4.0, 9.0, 11.0, br_leg_rx),
            br_shin_mat: mk_part(-4.0, br_shin_y, br_shin_z, br_shin_rx),
            fl_leg_mat: mk_part(4.0, fl_shoulder_y, fl_shoulder_z, fl_leg_rx),
            fl_shin_mat: mk_part(4.0, fl_shin_y, fl_shin_z, fl_shin_rx),
            fr_leg_mat: mk_part(-4.0, fl_shoulder_y, fl_shoulder_z, fr_leg_rx),
            fr_shin_mat: mk_part(-4.0, fr_shin_y, fr_shin_z, fr_shin_rx),
            head_mat: root
                * Mat4::from_translation(Vec3::new(0.0, neck_y, neck_z))
                * Mat4::from_rotation_y(neck_ry)
                * Mat4::from_rotation_x(neck_rx),
            neck_rx,
            mouth,
        }
    }
}

fn draw_anatomy(
    mesh: &mut Mesh,
    pose: &HorsePose,
    subtype: u8,
    inflate: f32,
    sheet: Sheet,
    light: [u8; 4],
) {
    // Torso & 3-segment tail
    horse_box(mesh, pose.body_mat, [-5.0, -8.0, -19.0], [10.0, 10.0, 24.0], inflate, [0.0, 34.0], sheet, light, false);
    horse_box(mesh, pose.tail_base_mat, [-1.0, -1.0, 0.0], [2.0, 2.0, 3.0], inflate, [44.0, 0.0], sheet, light, false);
    horse_box(mesh, pose.tail_mid_mat, [-1.5, -2.0, 3.0], [3.0, 4.0, 7.0], inflate, [38.0, 7.0], sheet, light, false);
    horse_box(mesh, pose.tail_tip_mat, [-1.5, -4.5, 9.0], [3.0, 4.0, 7.0], inflate, [24.0, 3.0], sheet, light, false);

    // Back legs, shins & hooves
    horse_box(mesh, pose.bl_leg_mat, [-2.5, -2.0, -2.5], [4.0, 9.0, 5.0], inflate, [78.0, 29.0], sheet, light, false);
    horse_box(mesh, pose.bl_shin_mat, [-2.0, 0.0, -1.5], [3.0, 5.0, 3.0], inflate, [78.0, 43.0], sheet, light, false);
    horse_box(mesh, pose.bl_shin_mat, [-2.5, 5.1, -2.0], [4.0, 3.0, 4.0], inflate, [78.0, 51.0], sheet, light, false);

    horse_box(mesh, pose.br_leg_mat, [-1.5, -2.0, -2.5], [4.0, 9.0, 5.0], inflate, [96.0, 29.0], sheet, light, false);
    horse_box(mesh, pose.br_shin_mat, [-1.0, 0.0, -1.5], [3.0, 5.0, 3.0], inflate, [96.0, 43.0], sheet, light, false);
    horse_box(mesh, pose.br_shin_mat, [-1.5, 5.1, -2.0], [4.0, 3.0, 4.0], inflate, [96.0, 51.0], sheet, light, false);

    // Front legs, shins & hooves
    horse_box(mesh, pose.fl_leg_mat, [-1.9, -1.0, -2.1], [3.0, 8.0, 4.0], inflate, [44.0, 29.0], sheet, light, false);
    horse_box(mesh, pose.fl_shin_mat, [-1.9, 0.0, -1.6], [3.0, 5.0, 3.0], inflate, [44.0, 41.0], sheet, light, false);
    horse_box(mesh, pose.fl_shin_mat, [-2.4, 5.1, -2.1], [4.0, 3.0, 4.0], inflate, [44.0, 51.0], sheet, light, false);

    horse_box(mesh, pose.fr_leg_mat, [-1.1, -1.0, -2.1], [3.0, 8.0, 4.0], inflate, [60.0, 29.0], sheet, light, false);
    horse_box(mesh, pose.fr_shin_mat, [-1.1, 0.0, -1.6], [3.0, 5.0, 3.0], inflate, [60.0, 41.0], sheet, light, false);
    horse_box(mesh, pose.fr_shin_mat, [-1.6, 5.1, -2.1], [4.0, 3.0, 4.0], inflate, [60.0, 51.0], sheet, light, false);

    // Neck, mane, head, animated upper/lower mouth & ears
    horse_box(mesh, pose.head_mat, [-2.05, -9.8, -2.0], [4.0, 14.0, 8.0], inflate, [0.0, 12.0], sheet, light, false);
    horse_box(mesh, pose.head_mat, [-1.0, -11.5, 5.0], [2.0, 16.0, 4.0], inflate, [58.0, 0.0], sheet, light, false);
    horse_box(mesh, pose.head_mat, [-2.5, -10.0, -1.5], [5.0, 5.0, 7.0], inflate, [0.0, 0.0], sheet, light, false);

    let upper_mouth = pose.head_mat * Mat4::from_rotation_x(-0.09424778 * pose.mouth);
    let lower_mouth = pose.head_mat * Mat4::from_rotation_x(0.15707964 * pose.mouth);
    horse_box(mesh, upper_mouth, [-2.0, -10.0, -7.0], [4.0, 3.0, 6.0], inflate, [24.0, 18.0], sheet, light, false);
    horse_box(mesh, lower_mouth, [-2.0, -7.0, -6.5], [4.0, 2.0, 5.0], inflate, [24.0, 27.0], sheet, light, false);

    if matches!(subtype, 1 | 2) {
        let left_ear = pose.head_mat * Mat4::from_rotation_z(0.2617994);
        let right_ear = pose.head_mat * Mat4::from_rotation_z(-0.2617994);
        horse_box(mesh, left_ear, [-2.0, -16.0, 4.0], [2.0, 7.0, 1.0], inflate, [0.0, 12.0], sheet, light, false);
        horse_box(mesh, right_ear, [0.0, -16.0, 4.0], [2.0, 7.0, 1.0], inflate, [0.0, 12.0], sheet, light, false);
    } else {
        horse_box(mesh, pose.head_mat, [0.45, -12.0, 4.0], [2.0, 3.0, 1.0], inflate, [0.0, 0.0], sheet, light, false);
        horse_box(mesh, pose.head_mat, [-2.45, -12.0, 4.0], [2.0, 3.0, 1.0], inflate, [0.0, 0.0], sheet, light, false);
    }
}

fn draw_equipment(
    mesh: &mut Mesh,
    pose: &HorsePose,
    input: &HorseDrawInput<'_>,
    base_sheet: Sheet,
) {
    let light = input.light;
    if !input.baby && input.saddled {
        // Saddle at rest (0, 2, 2) == offset (0, -9, -7) relative to body pivot (0, 11, 9)
        let saddle_mat = pose.body_mat * Mat4::from_translation(Vec3::new(0.0, -9.0, -7.0));
        horse_box(mesh, saddle_mat, [-5.0, 0.0, -3.0], [10.0, 1.0, 8.0], 0.0, [80.0, 0.0], base_sheet, light, false);
        horse_box(mesh, saddle_mat, [-1.5, -1.0, -3.0], [3.0, 1.0, 2.0], 0.0, [106.0, 9.0], base_sheet, light, false);
        horse_box(mesh, saddle_mat, [-4.0, -1.0, 3.0], [8.0, 1.0, 2.0], 0.0, [80.0, 9.0], base_sheet, light, false);

        // Left & right stirrup straps & irons at rest (+/-5, 3, 2) == (+/-5, -8, -7) relative to body
        let stirrup_rx = if input.ridden { -1.0471976 } else { 0.0 };
        let left_stirrup = pose.body_mat
            * Mat4::from_translation(Vec3::new(5.0, -8.0, -7.0))
            * Mat4::from_rotation_x(stirrup_rx);
        let right_stirrup = pose.body_mat
            * Mat4::from_translation(Vec3::new(-5.0, -8.0, -7.0))
            * Mat4::from_rotation_x(stirrup_rx);
        horse_box(mesh, left_stirrup, [-0.5, 0.0, -0.5], [1.0, 6.0, 1.0], 0.0, [70.0, 0.0], base_sheet, light, false);
        horse_box(mesh, left_stirrup, [-0.5, 6.0, -1.0], [1.0, 2.0, 2.0], 0.0, [74.0, 0.0], base_sheet, light, false);
        horse_box(mesh, right_stirrup, [-0.5, 0.0, -0.5], [1.0, 6.0, 1.0], 0.0, [80.0, 0.0], base_sheet, light, false);
        horse_box(mesh, right_stirrup, [-0.5, 6.0, -1.0], [1.0, 2.0, 2.0], 0.0, [74.0, 4.0], base_sheet, light, false);

        // Bridle & metal bits on head
        horse_box(mesh, pose.head_mat, [-2.5, -10.1, -7.0], [5.0, 5.0, 12.0], 0.2, [80.0, 12.0], base_sheet, light, false);
        horse_box(mesh, pose.head_mat, [1.5, -8.0, -4.0], [1.0, 2.0, 2.0], 0.0, [74.0, 13.0], base_sheet, light, false);
        horse_box(mesh, pose.head_mat, [-2.5, -8.0, -4.0], [1.0, 2.0, 2.0], 0.0, [74.0, 13.0], base_sheet, light, false);

        if input.ridden {
            let rein_mat = pose.head_mat * Mat4::from_rotation_x(-pose.neck_rx);
            horse_box(mesh, rein_mat, [2.6, -6.0, 0.0], [0.0, 3.0, 16.0], 0.0, [44.0, 10.0], base_sheet, light, true);
            horse_box(mesh, rein_mat, [-2.6, -6.0, 0.0], [0.0, 3.0, 16.0], 0.0, [44.0, 5.0], base_sheet, light, true);
        }
    }

    if !input.baby && matches!(input.subtype, 1 | 2) && input.chested {
        // Rest (-7.5, 3, 10) and (4.5, 3, 10) == (-7.5, -8, 1) and (4.5, -8, 1) relative to body (0, 11, 9)
        let left_chest = pose.body_mat
            * Mat4::from_translation(Vec3::new(-7.5, -8.0, 1.0))
            * Mat4::from_rotation_y(FRAC_PI_2);
        let right_chest = pose.body_mat
            * Mat4::from_translation(Vec3::new(4.5, -8.0, 1.0))
            * Mat4::from_rotation_y(FRAC_PI_2);
        horse_box(mesh, left_chest, [-3.0, 0.0, 0.0], [8.0, 8.0, 3.0], 0.0, [0.0, 34.0], base_sheet, light, false);
        horse_box(mesh, right_chest, [-3.0, 0.0, 0.0], [8.0, 8.0, 3.0], 0.0, [0.0, 47.0], base_sheet, light, false);
    }
}

pub(crate) fn armor_texture(horse_armor: u8) -> Option<&'static str> {
    match horse_armor {
        1 => Some("textures/entity/horse/armor/horse_armor_iron.png"),
        2 => Some("textures/entity/horse/armor/horse_armor_gold.png"),
        3 => Some("textures/entity/horse/armor/horse_armor_diamond.png"),
        _ => None,
    }
}

pub(crate) fn draw_horse(mesh: &mut Mesh, resources: &Resources, input: HorseDrawInput<'_>) {
    let pose = compute_pose(&input);
    let base_sheet = Sheet::from_sprite(resources.sprite(input.base_texture), input.hurt_flag);
    draw_anatomy(mesh, &pose, input.subtype, 0.0, base_sheet, input.light);

    if input.subtype == 0 {
        if let Some(markings_path) = input.markings_texture {
            let markings_sheet =
                Sheet::from_sprite(resources.sprite(markings_path), input.hurt_flag);
            draw_anatomy(mesh, &pose, input.subtype, 0.01, markings_sheet, input.light);
        }
        if let Some(armor_path) = armor_texture(input.horse_armor) {
            let armor_sheet = Sheet::from_sprite(resources.sprite(armor_path), input.hurt_flag);
            draw_anatomy(mesh, &pose, input.subtype, 0.08, armor_sheet, input.light);
        }
    }

    draw_equipment(mesh, &pose, &input, base_sheet);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_sheet(layer: u32) -> Sheet {
        Sheet {
            layer: layer | ATLAS,
            scale: [1.0 / 256.0, 1.0 / 256.0],
        }
    }

    fn base_input() -> HorseDrawInput<'static> {
        HorseDrawInput {
            feet: Vec3::ZERO,
            body_yaw_rad: 0.0,
            head_yaw_deg: None,
            pitch_deg: 0.0,
            limb_swing: 0.0,
            limb_amount: 0.0,
            age_in_ticks: 0.0,
            alpha: 1.0,
            roll_z: 0.0,
            subtype: 0,
            baby: false,
            saddled: false,
            chested: false,
            ridden: false,
            base_texture: "textures/entity/horse/horse_white.png",
            markings_texture: None,
            horse_armor: 0,
            anim: HorseAnim::default(),
            light: [0, 240, 0, 0],
            hurt_flag: 0,
        }
    }

    #[test]
    fn procedural_horse_rig_lifts_front_hooves_paws_forward_and_shades_faces() {
        let rest_input = base_input();
        let rest_pose = compute_pose(&rest_input);
        let mut rest_mesh = crate::models::empty();
        draw_anatomy(&mut rest_mesh, &rest_pose, 0, 0.0, test_sheet(1), [0, 240, 0, 0]);
        // 23 anatomy boxes * 6 faces = 138 faces
        assert_eq!(rest_mesh.visible_faces, 138);
        // Directional shading produces multiple distinct RGB shade values instead of flat 255
        let distinct_shades: std::collections::BTreeSet<u8> =
            rest_mesh.vertices.iter().map(|v| v.color[0]).collect();
        assert!(distinct_shades.len() >= 3);

        let rest_fl_hoof = rest_pose.fl_shin_mat.transform_point3(Vec3::new(0.0, 8.1, 0.0));
        assert!(rest_fl_hoof.y.abs() < 0.02, "rest hoof must stand on feet.y=0: {}", rest_fl_hoof.y);

        let mut rear_a = base_input();
        rear_a.anim.prev_rearing = 1.0;
        rear_a.anim.rearing = 1.0;
        rear_a.age_in_ticks = 0.0;
        let pose_a = compute_pose(&rear_a);
        let hoof_l_a = pose_a.fl_shin_mat.transform_point3(Vec3::new(0.0, 8.1, 0.0));
        let hoof_r_a = pose_a.fr_shin_mat.transform_point3(Vec3::new(0.0, 8.1, 0.0));
        assert!(hoof_l_a.y > 0.7 && hoof_r_a.y > 0.7, "rearing front hooves must lift high into the air");
        assert!((hoof_l_a.y - hoof_r_a.y).abs() > 0.1 || (hoof_l_a.z - hoof_r_a.z).abs() > 0.1,
            "left and right front legs must paw out of phase");

        // Equipment adds saddle, stirrups, bridle, reins and donkey chests
        let mut eq = base_input();
        eq.subtype = 1;
        eq.saddled = true;
        eq.ridden = true;
        eq.chested = true;
        let eq_pose = compute_pose(&eq);
        let mut eq_mesh = crate::models::empty();
        draw_anatomy(&mut eq_mesh, &eq_pose, 1, 0.0, test_sheet(1), [0, 240, 0, 0]);
        draw_equipment(&mut eq_mesh, &eq_pose, &eq, test_sheet(1));
        assert!(eq_mesh.visible_faces > rest_mesh.visible_faces + 60);
    }
}
