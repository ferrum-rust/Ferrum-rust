//! First-person empty-hand geometry in camera coordinates. The skin is read
//! from the local resource atlas with the original ModelPlayer UV layout.
use crate::{
    mesh::{Mesh, Vertex},
    models::ATLAS,
    vanilla::math,
};
use glam::{Mat4, Vec3};

pub const CAMERA_HAND: u32 = 1 << 22;
#[derive(Clone,Debug)]
pub struct Equipped {
    pub stack:Option<crate::inventory::Stack>,
    previous:f32,
    progress:f32,
    pub use_ticks:Option<u32>,
    previous_bow_zoom: f32,
    bow_zoom: f32,
}
impl Default for Equipped {
    fn default() -> Self {
        Self {
            stack: None,
            previous: 0.,
            progress: 0.,
            use_ticks: None,
            previous_bow_zoom: 1.,
            bow_zoom: 1.,
        }
    }
}
impl Equipped {
    pub fn tick(&mut self,current:Option<crate::inventory::Stack>,used:Option<u32>) {
        let bow_only_target = current.as_ref().filter(|stack| stack.id == 261)
            .and(used).map_or(1., |ticks| {
                let charge = ticks.min(20) as f32 / 20.;
                1. - charge * charge * 0.15
            });
        self.tick_with_fov(current, used, bow_only_target);
    }
    pub fn tick_with_fov(
        &mut self,
        current: Option<crate::inventory::Stack>,
        used: Option<u32>,
        fov_target: f32,
    ) {
        self.previous_bow_zoom = self.bow_zoom;
        self.bow_zoom = (self.bow_zoom + (fov_target - self.bow_zoom) * 0.5).clamp(0.1, 1.5);
        self.previous=self.progress;
        self.use_ticks=used;
        let same=self.stack==current || self.stack.as_ref().zip(current.as_ref()).is_some_and(|(a,b)|a.identity==b.identity);
        let target=if same{1.}else{0.};
        self.progress+=(target-self.progress).clamp(-0.4,0.4);
        if self.progress<0.1 || same {self.stack=current;}
    }
    pub fn lowered(&self,alpha:f32)->f32 {1.-(self.previous+(self.progress-self.previous)*alpha)}
    /// ItemRenderer.resetEquippedProgress after accepted block use.
    pub fn reset_progress(&mut self) { self.progress = 0.; }
    pub fn bow_fov_multiplier(&self, alpha: f32) -> f32 {
        self.previous_bow_zoom + (self.bow_zoom - self.previous_bow_zoom) * alpha
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Swing {
    active: bool,
    tick: i32,
    progress: f32,
    previous: f32,
}
impl Swing {
    pub fn start(&mut self, duration: i32) {
        if !self.active || self.tick >= duration / 2 || self.tick < 0 {
            self.tick = -1;
            self.active = true;
        }
    }
    pub fn advance(&mut self, duration: i32) {
        self.previous = self.progress;
        if self.active {
            self.tick += 1;
            if self.tick >= duration {
                self.tick = 0;
                self.active = false;
            }
        } else {
            self.tick = 0;
        }
        self.progress = self.tick as f32 / duration as f32;
    }
    pub fn in_progress(self)->bool{self.progress>0.}
    pub fn sample(self, alpha: f32) -> f32 {
        let mut d = self.progress - self.previous;
        if d < 0. {
            d += 1.;
        }
        self.previous + d * alpha
    }
}
pub fn swing_duration(effects: &crate::vanilla::potions::Effects) -> i32 {
    if let Some(e) = effects.get(3) {
        return 6i32.wrapping_sub(e.amplifier.wrapping_add(1));
    }
    if let Some(e) = effects.get(4) {
        return 6i32.wrapping_add(e.amplifier.wrapping_add(1).wrapping_mul(2));
    }
    6
}
fn hand_transform(swing: f32, equip: f32) -> Mat4 {
    use std::f32::consts::PI;
    let root = swing.sqrt() * PI;
    let root_sin = math::sin(root);
    Mat4::from_translation(Vec3::new(
        -0.3 * root_sin,
        0.4 * math::sin(root * 2.),
        -0.4 * math::sin(swing * PI),
    )) * Mat4::from_translation(Vec3::new(0.64000005, -0.6, -0.71999997))
        * Mat4::from_translation(Vec3::new(0., equip * -0.6, 0.))
        * Mat4::from_rotation_y(45f32.to_radians())
        * Mat4::from_rotation_y((root_sin * 70.).to_radians())
        * Mat4::from_rotation_z((math::sin(swing * swing * PI) * -20.).to_radians())
        * Mat4::from_translation(Vec3::new(-1., 3.6, 3.5))
        * Mat4::from_rotation_z(120f32.to_radians())
        * Mat4::from_rotation_x(200f32.to_radians())
        * Mat4::from_rotation_y((-135f32).to_radians())
        * Mat4::from_translation(Vec3::X * 5.6)
        * Mat4::from_translation(Vec3::new(-5., 2., 0.) / 16.)
        * Mat4::from_rotation_z(0.1)
        * Mat4::from_scale(Vec3::splat(1. / 16.))
}
#[derive(Clone, Copy)]
struct ModelVertex {
    position: Vec3,
    pixel: [f32; 2],
}
fn arm_faces(sleeve: bool) -> [[ModelVertex; 4]; 6] {
    let expand = if sleeve { 0.25 } else { 0. };
    let lo = Vec3::new(-3., -2., -2.) - Vec3::splat(expand);
    let hi = Vec3::new(1., 10., 2.) + Vec3::splat(expand);
    let x = [lo.x, hi.x, hi.x, lo.x];
    let y = [lo.y, lo.y, hi.y, hi.y];
    let v = if sleeve { 32. } else { 16. };
    // Facing and winding follow ModelBox, including the reversed V on the cap.
    let faces = [
        ([5, 1, 2, 6], [48., v + 4., 52., v + 16.]),
        ([0, 4, 7, 3], [40., v + 4., 44., v + 16.]),
        ([5, 4, 0, 1], [44., v, 48., v + 4.]),
        ([2, 3, 7, 6], [48., v + 4., 52., v]),
        ([1, 0, 3, 2], [44., v + 4., 48., v + 16.]),
        ([4, 5, 6, 7], [52., v + 4., 56., v + 16.]),
    ];
    faces.map(|(indices, [u0, v0, u1, v1])| {
        std::array::from_fn(|j| {
            let i = indices[j];
            ModelVertex {
                position: Vec3::new(x[i % 4], y[i % 4], if i < 4 { lo.z } else { hi.z }),
                pixel: [[u1, v0], [u0, v0], [u0, v1], [u1, v1]][j],
            }
        })
    })
}
pub fn empty_hand(mesh: &mut Mesh, layer: u32, swing: f32, equip: f32, show_sleeve: bool) {
    use std::sync::OnceLock;
    static GEOMETRY: OnceLock<[[[ModelVertex; 4]; 6]; 2]> = OnceLock::new();
    static REST: OnceLock<Mat4> = OnceLock::new();
    let swing = if swing.is_finite() { swing } else { 0. };
    let model = if swing == 0. && equip == 0. {
        *REST.get_or_init(|| hand_transform(0., 0.))
    } else {
        hand_transform(swing, equip)
    };
    let parts = GEOMETRY.get_or_init(|| [arm_faces(false), arm_faces(true)]);
    for part in &parts[..1 + usize::from(show_sleeve)] {
        for quad in part {
            let start = mesh.vertices.len() as u32;
            for v in quad {
                let p = model.transform_point3(v.position);
                mesh.vertices.push(Vertex {
                    position: p.to_array(),
                    uv: v.pixel.map(|n| n / 256.),
                    layer: layer | ATLAS | CAMERA_HAND,
                    color: [255; 4],
                    light: [0, 240, 0, 0],
                });
            }
            mesh.indices.extend([0, 1, 2, 0, 2, 3].map(|i| start + i));
            mesh.visible_faces += 1;
        }
    }
}

fn hand_transform_slim(swing: f32, equip: f32) -> Mat4 {
    use std::f32::consts::PI;
    let root = swing.sqrt() * PI;
    let root_sin = math::sin(root);
    Mat4::from_translation(Vec3::new(
        -0.3 * root_sin,
        0.4 * math::sin(root * 2.),
        -0.4 * math::sin(swing * PI),
    )) * Mat4::from_translation(Vec3::new(0.64000005, -0.6, -0.71999997))
        * Mat4::from_translation(Vec3::new(0., equip * -0.6, 0.))
        * Mat4::from_rotation_y(45f32.to_radians())
        * Mat4::from_rotation_y((root_sin * 70.).to_radians())
        * Mat4::from_rotation_z((math::sin(swing * swing * PI) * -20.).to_radians())
        * Mat4::from_translation(Vec3::new(-1., 3.6, 3.5))
        * Mat4::from_rotation_z(120f32.to_radians())
        * Mat4::from_rotation_x(200f32.to_radians())
        * Mat4::from_rotation_y((-135f32).to_radians())
        * Mat4::from_translation(Vec3::X * 5.6)
        * Mat4::from_translation(Vec3::new(-5., 2.5, 0.) / 16.)
        * Mat4::from_rotation_z(0.1)
        * Mat4::from_scale(Vec3::splat(1. / 16.))
}

fn slim_arm_faces(sleeve: bool) -> [[ModelVertex; 4]; 6] {
    let expand = if sleeve { 0.25 } else { 0. };
    let lo = Vec3::new(-2., -2., -2.) - Vec3::splat(expand);
    let hi = Vec3::new(1., 10., 2.) + Vec3::splat(expand);
    let x = [lo.x, hi.x, hi.x, lo.x];
    let y = [lo.y, lo.y, hi.y, hi.y];
    let v = if sleeve { 32. } else { 16. };
    let faces = [
        ([5, 1, 2, 6], [47., v + 4., 51., v + 16.]),
        ([0, 4, 7, 3], [40., v + 4., 44., v + 16.]),
        ([5, 4, 0, 1], [44., v, 47., v + 4.]),
        ([2, 3, 7, 6], [47., v + 4., 50., v]),
        ([1, 0, 3, 2], [44., v + 4., 47., v + 16.]),
        ([4, 5, 6, 7], [51., v + 4., 54., v + 16.]),
    ];
    faces.map(|(indices, [u0, v0, u1, v1])| {
        std::array::from_fn(|j| {
            let i = indices[j];
            ModelVertex {
                position: Vec3::new(x[i % 4], y[i % 4], if i < 4 { lo.z } else { hi.z }),
                pixel: [[u1, v0], [u0, v0], [u0, v1], [u1, v1]][j],
            }
        })
    })
}

pub fn empty_hand_skin(
    mesh: &mut Mesh,
    skin: crate::player_skins::SkinRegion,
    slim: bool,
    swing: f32,
    equip: f32,
    show_sleeve: bool,
) {
    empty_hand_skin_impl(mesh, skin, slim, swing, equip, show_sleeve, None);
}

#[allow(clippy::too_many_arguments)]
pub fn empty_hand_skin_shaded(
    mesh: &mut Mesh,
    skin: crate::player_skins::SkinRegion,
    slim: bool,
    swing: f32,
    equip: f32,
    show_sleeve: bool,
    yaw: f32,
    pitch: f32,
) {
    empty_hand_skin_impl(mesh, skin, slim, swing, equip, show_sleeve, Some((yaw, pitch)));
}

fn empty_hand_skin_impl(
    mesh: &mut Mesh,
    skin: crate::player_skins::SkinRegion,
    slim: bool,
    swing: f32,
    equip: f32,
    show_sleeve: bool,
    angles: Option<(f32, f32)>,
) {
    use std::sync::OnceLock;
    static CLASSIC_GEOM: OnceLock<[[[ModelVertex; 4]; 6]; 2]> = OnceLock::new();
    static SLIM_GEOM: OnceLock<[[[ModelVertex; 4]; 6]; 2]> = OnceLock::new();
    static CLASSIC_REST: OnceLock<Mat4> = OnceLock::new();
    static SLIM_REST: OnceLock<Mat4> = OnceLock::new();
    let swing = if swing.is_finite() { swing } else { 0. };
    let model = match (slim, swing == 0. && equip == 0.) {
        (false, true) => *CLASSIC_REST.get_or_init(|| hand_transform(0., 0.)),
        (false, false) => hand_transform(swing, equip),
        (true, true) => *SLIM_REST.get_or_init(|| hand_transform_slim(0., 0.)),
        (true, false) => hand_transform_slim(swing, equip),
    };
    let parts = if slim {
        SLIM_GEOM.get_or_init(|| [slim_arm_faces(false), slim_arm_faces(true)])
    } else {
        CLASSIC_GEOM.get_or_init(|| [arm_faces(false), arm_faces(true)])
    };
    let to_world = angles.map(|(yaw, pitch)| {
        glam::Mat3::from_rotation_y(-yaw) * glam::Mat3::from_rotation_x(pitch)
    });
    for part in &parts[..1 + usize::from(show_sleeve)] {
        for quad in part {
            let pts = quad.map(|v| model.transform_point3(v.position));
            let color = if let Some(to_world) = to_world {
                let cam_normal = (pts[1] - pts[0]).cross(pts[2] - pts[0]);
                let normal = (to_world * cam_normal).normalize_or_zero();
                let shade = (crate::models::entity_face_shade(normal) * 255.).round() as u8;
                [shade, shade, shade, 255]
            } else {
                [255; 4]
            };
            let start = mesh.vertices.len() as u32;
            for (v, p) in quad.iter().zip(pts) {
                mesh.vertices.push(Vertex {
                    position: p.to_array(),
                    uv: skin.pixel_uv(v.pixel[0], v.pixel[1]),
                    layer: skin.layer | ATLAS | CAMERA_HAND,
                    color,
                    light: [0, 240, 0, 0],
                });
            }
            mesh.indices.extend([0, 1, 2, 0, 2, 3].map(|i| start + i));
            mesh.visible_faces += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    type Tick = (bool, bool, i32, u32, u32, u32, u32);
    type Sequence = (i32, i32, Vec<Tick>);
    type ReferenceArm = Vec<[[f32; 5]; 4]>;
    #[test]
    fn camera_space_arm_matches_the_original_itemrenderer_modelview() {
        let rows: Vec<(f32, f32, [f32; 16])> = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-arm-transform.json"
        ))
        .unwrap();
        assert_eq!(rows.len(), 28);
        for (equip, swing, expected) in rows {
            let actual = hand_transform(swing, equip);
            for (a, b) in actual.to_cols_array().into_iter().zip(expected) {
                assert!(
                    (a - b).abs() < 0.000004,
                    "equip {equip} swing {swing}: {a} vs {b}"
                );
            }
        }
        let at_rest = hand_transform(0., 0.);
        assert!(arm_faces(false)
            .iter()
            .flatten()
            .all(|v| at_rest.transform_point3(v.position).z < 0.));
    }
    #[test]
    fn original_right_arm_sleeve_and_swing_timing_match() {
        let (models, sequences): (Vec<ReferenceArm>, Vec<Sequence>) = serde_json::from_str(
            include_str!("../tests/fixtures/vanilla189-first-person-arm.json"),
        )
        .unwrap();
        for (sleeve, expected) in models.iter().enumerate() {
            assert_eq!(expected.len(), 6);
            for (quad, reference) in arm_faces(sleeve == 1).iter().zip(expected) {
                for (v, r) in quad.iter().zip(reference) {
                    assert_eq!(
                        [
                            v.position.x,
                            v.position.y,
                            v.position.z,
                            v.pixel[0],
                            v.pixel[1]
                        ],
                        *r
                    );
                }
            }
        }
        assert_eq!(models.len(), 2);
        let bits = |v: f32| if v.is_nan() { 0x7fc00000 } else { v.to_bits() };
        let mut total = 0;
        for (haste, fatigue, ticks) in sequences {
            let mut swing = Swing::default();
            let mut effects = crate::vanilla::potions::Effects::default();
            for (id, amp) in [(3, haste), (4, fatigue)] {
                if amp >= 0 {
                    effects
                        .load_effect(crate::vanilla::item_food::Effect::new(id, 80, amp))
                        .unwrap();
                }
            }
            let duration = swing_duration(&effects);
            for (click, active, tick, progress, quarter, half, end) in ticks {
                if click {
                    swing.start(duration);
                }
                swing.advance(duration);
                assert_eq!(
                    (swing.active, swing.tick, bits(swing.progress)),
                    (active, tick, progress)
                );
                assert_eq!(
                    [0.25, 0.5, 1.].map(|t| bits(swing.sample(t))),
                    [quarter, half, end]
                );
                total += 1;
            }
        }
        assert_eq!(total, 1792);
    }
    #[test]
    fn empty_hand_skin_offsets_atlas_slot_and_supports_slim_arm() {
        let mut ref_mesh = crate::models::empty();
        empty_hand(&mut ref_mesh, 12, 0.25, 0.5, true);
        let mut skin_mesh = crate::models::empty();
        let default_region = crate::player_skins::SkinRegion {
            layer: 12,
            origin: [0., 0.],
            scale: [0.25, 0.25],
        };
        empty_hand_skin(&mut skin_mesh, default_region, false, 0.25, 0.5, true);
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&ref_mesh.vertices),
            bytemuck::cast_slice::<_, u8>(&skin_mesh.vertices)
        );

        let mut slim_mesh = crate::models::empty();
        let slot_region = crate::player_skins::SkinRegion {
            layer: 700,
            origin: [0.5, 0.75],
            scale: [0.25, 0.25],
        };
        empty_hand_skin(&mut slim_mesh, slot_region, true, 0., 0., true);
        assert_eq!(slim_mesh.vertices.len(), 48);
        assert!(slim_mesh.vertices.iter().all(|v| {
            v.layer == (700 | ATLAS | CAMERA_HAND)
                && (0.5..=0.75).contains(&v.uv[0])
                && (0.75..=1.0).contains(&v.uv[1])
        }));
    }
}
