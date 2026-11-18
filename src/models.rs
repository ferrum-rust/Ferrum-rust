use crate::{
    assets::Assets,
    gameplay::{MobKind, Simulation},
    inventory::Stack,
    mesh::{Frustum, Mesh, Vertex},
    player::Player,
    resources::Resources,
    settings::Settings,
    world::World,
};
use glam::{Mat4, Vec3};
pub const ATLAS: u32 = 1 << 16;
pub const UNLIT: u32 = 1 << 17;
pub const BLEND: u32 = 1 << 18;
pub const OPAQUE_LEAF: u32 = 1 << 19;
pub const CLOUD: u32 = 1 << 20;
pub const SOLID: u32 = 1 << 21;
pub const SKY: u32 = 1 << 23;
/// Original CUTOUT terrain and item rendering sample the full-resolution sprite.
pub const NO_MIPMAP: u32 = 1 << 24;
/// Creeper fuse flash overlay (`LayerCreeperCharge` / `getColorMultiplier` white flash).
pub const WHITE_FLASH: u32 = 1 << 25;
/// Enchanted armor / item glint overlay (`LayerArmorBase.renderGlint`).
pub const GLINT: u32 = 1 << 27;
/// World-space see-through pass (Render.renderLivingLabel disableDepth).
pub const SEE_THROUGH: u32 = 1 << 29;
/// End portal 16-layer eye-linear starfield (`SOLID | FLOWING`, never simultaneously set elsewhere).
pub const END_PORTAL: u32 = (1 << 21) | (1 << 26);

/// Exact 1.8.9 `EntitySheep.DYE_TO_RGB` table indexed by wool/collar color `0..=15`.
pub(crate) const DYE_TO_RGB: [[u8; 3]; 16] = [
    [255, 255, 255], // 0: White
    [216, 127, 51],  // 1: Orange
    [178, 76, 216],  // 2: Magenta
    [102, 153, 216], // 3: Light Blue
    [229, 229, 51],  // 4: Yellow
    [127, 204, 25],  // 5: Lime
    [242, 127, 165], // 6: Pink
    [76, 76, 76],    // 7: Gray
    [153, 153, 153], // 8: Silver
    [76, 127, 153],  // 9: Cyan
    [127, 63, 178],  // 10: Purple
    [51, 76, 178],   // 11: Blue
    [102, 76, 51],   // 12: Brown
    [102, 127, 51],  // 13: Green
    [153, 51, 51],   // 14: Red
    [25, 25, 25],    // 15: Black
];

pub(crate) fn sheep_wool_rgb(color: u8, jeb: bool, age_ticks: u32, id: u64, alpha: f32) -> [u8; 3] {
    if jeb {
        let step = age_ticks / 25 + id as u32;
        let c1 = DYE_TO_RGB[(step % 16) as usize];
        let c2 = DYE_TO_RGB[((step + 1) % 16) as usize];
        let t = (((age_ticks % 25) as f32 + alpha) / 25.0).clamp(0.0, 1.0);
        std::array::from_fn(|k| {
            (c1[k] as f32 * (1.0 - t) + c2[k] as f32 * t)
                .round()
                .clamp(0.0, 255.0) as u8
        })
    } else {
        DYE_TO_RGB[usize::from(color & 15)]
    }
}

/// Vanilla `RendererLivingEntity.rotateCorpse` 20-tick sideways death fall-over (`0..=PI/2`).
pub(crate) fn death_roll(death_time: u8, alpha: f32) -> f32 {
    if death_time > 0 {
        let f = (((death_time as f32 + alpha - 1.0).max(0.0)) / 20.0 * 1.6)
            .sqrt()
            .min(1.0);
        f * std::f32::consts::FRAC_PI_2
    } else {
        0.0
    }
}
#[path="server_models.rs"]
pub mod server;
#[path="vehicle_model.rs"]
pub(crate) mod vehicle;
#[path="boss_bar.rs"]
pub(crate) mod boss_bar;
#[path = "projectile_models.rs"]
mod projectiles;
#[path = "precipitation_models.rs"]
mod precipitation;
#[path = "spawner_render.rs"]
mod spawner_render;
#[path = "end_portal_render.rs"]
mod end_portal_render;
/// RenderGlobal's camera-relative upper sky grid, facing down toward the player.
pub fn sky_plane() -> Mesh {
    let mut mesh = empty();
    for x in (-384..=384).step_by(64) {
        for z in (-384..=384).step_by(64) {
            let (x, z) = (x as f32, z as f32);
            face(
                &mut mesh,
                [
                    Vec3::new(x, 16.0, z),
                    Vec3::new(x + 64.0, 16.0, z),
                    Vec3::new(x + 64.0, 16.0, z + 64.0),
                    Vec3::new(x, 16.0, z + 64.0),
                ],
                [0.0; 4],
                SKY | SOLID | UNLIT,
                [255; 4],
            );
        }
    }
    mesh
}
/// `RenderGlobal.renderSkyEnd()`: 6-face camera-centered `[-100, 100]^3` cube tiled `16x16` per face.
pub fn end_sky(r: &Resources) -> Mesh {
    let mut mesh = empty();
    let sprite = r.sprite("textures/environment/end_sky.png");
    let u_ext = sprite.width as f32 / 256.0;
    let v_ext = sprite.height as f32 / 256.0;
    let layer = sprite.layer | SKY | UNLIT | ATLAS | NO_MIPMAP;
    let rotations = [
        Mat4::IDENTITY,
        Mat4::from_rotation_x(90.0_f32.to_radians()),
        Mat4::from_rotation_x((-90.0_f32).to_radians()),
        Mat4::from_rotation_x(180.0_f32.to_radians()),
        Mat4::from_rotation_z(90.0_f32.to_radians()),
        Mat4::from_rotation_z((-90.0_f32).to_radians()),
    ];
    let step = 200.0_f32 / 16.0;
    for rot in rotations {
        for gz in 0..16 {
            let z0 = -100.0 + gz as f32 * step;
            let z1 = z0 + step;
            for gx in 0..16 {
                let x0 = -100.0 + gx as f32 * step;
                let x1 = x0 + step;
                face(
                    &mut mesh,
                    [
                        rot.transform_point3(Vec3::new(x0, -100.0, z1)),
                        rot.transform_point3(Vec3::new(x1, -100.0, z1)),
                        rot.transform_point3(Vec3::new(x1, -100.0, z0)),
                        rot.transform_point3(Vec3::new(x0, -100.0, z0)),
                    ],
                    [0.0, 0.0, u_ext, v_ext],
                    layer,
                    [40, 40, 40, 255],
                );
            }
        }
    }
    mesh
}
pub fn empty() -> Mesh {
    Mesh {
        sections: None,
        vertices: Vec::new(),
        indices: Vec::new(),
        min: Vec3::splat(-1e6),
        max: Vec3::splat(1e6),
        visible_faces: 0,
    }
}
pub fn face(mesh: &mut Mesh, p: [Vec3; 4], uv: [f32; 4], layer: u32, color: [u8; 4]) {
    let start = mesh.vertices.len() as u32;
    let tex = [
        [uv[0], uv[1] + uv[3]],
        [uv[0] + uv[2], uv[1] + uv[3]],
        [uv[0] + uv[2], uv[1]],
        [uv[0], uv[1]],
    ];
    for i in 0..4 {
        mesh.vertices.push(Vertex {
            position: p[i].to_array(),
            uv: tex[i],
            layer,
            color,
            light: [0, 240, 0, 0],
        });
    }
    mesh.indices.extend([0, 1, 2, 0, 2, 3].map(|i| start + i));
    mesh.visible_faces += 1;
}
pub(crate) fn standard_item_light(normal: Vec3) -> f32 {
    0.4 + 0.6 * (normal.dot(Vec3::new(0.2, 1., -0.7).normalize()).max(0.)
        + normal.dot(Vec3::new(-0.2, 1., 0.7).normalize()).max(0.))
}
pub(crate) fn entity_face_shade(normal: Vec3) -> f32 {
    standard_item_light(normal).clamp(0.0, 1.0)
}
fn light_since(mesh: &mut Mesh, start: usize, world: &World, position: Vec3) {
    let light = world.visual_light_uv(position.floor().as_ivec3(), 0);
    for vertex in &mut mesh.vertices[start..] {
        vertex.light[..2].copy_from_slice(&light[..2]);
    }
}
fn cuboid(
    mesh: &mut Mesh,
    transform: Mat4,
    min: Vec3,
    size: Vec3,
    uv: [f32; 2],
    layer: u32,
    color: [u8; 4],
) {
    let max = min + size;
    let [w, h, d] = size.to_array().map(|v| v * 16.0);
    let [u, v] = uv;
    let parts = [
        (
            [
                Vec3::new(min.x, min.y, min.z),
                Vec3::new(min.x, min.y, max.z),
                Vec3::new(min.x, max.y, max.z),
                Vec3::new(min.x, max.y, min.z),
            ],
            [u, v + d, d, h],
        ),
        (
            [
                Vec3::new(max.x, min.y, max.z),
                Vec3::new(max.x, min.y, min.z),
                Vec3::new(max.x, max.y, min.z),
                Vec3::new(max.x, max.y, max.z),
            ],
            [u + d + w, v + d, d, h],
        ),
        (
            [
                Vec3::new(max.x, min.y, min.z),
                Vec3::new(min.x, min.y, min.z),
                Vec3::new(min.x, max.y, min.z),
                Vec3::new(max.x, max.y, min.z),
            ],
            [u + d, v + d, w, h],
        ),
        (
            [
                Vec3::new(min.x, min.y, max.z),
                Vec3::new(max.x, min.y, max.z),
                Vec3::new(max.x, max.y, max.z),
                Vec3::new(min.x, max.y, max.z),
            ],
            [u + d + w + d, v + d, w, h],
        ),
        (
            [
                Vec3::new(min.x, max.y, max.z),
                Vec3::new(max.x, max.y, max.z),
                Vec3::new(max.x, max.y, min.z),
                Vec3::new(min.x, max.y, min.z),
            ],
            [u + d, v, w, d],
        ),
        (
            [
                Vec3::new(min.x, min.y, min.z),
                Vec3::new(max.x, min.y, min.z),
                Vec3::new(max.x, min.y, max.z),
                Vec3::new(min.x, min.y, max.z),
            ],
            [u + d + w, v, w, d],
        ),
    ];
    for (p, uv) in parts {
        let pts = p.map(|p| transform.transform_point3(p));
        let shade = if layer & UNLIT != 0 {
            1.0
        } else {
            let normal = (pts[1] - pts[0]).cross(pts[2] - pts[0]).normalize_or_zero();
            entity_face_shade(normal)
        };
        face(
            mesh,
            pts,
            uv.map(|v| v / 256.0),
            layer,
            [
                (color[0] as f32 * shade) as u8,
                (color[1] as f32 * shade) as u8,
                (color[2] as f32 * shade) as u8,
                color[3],
            ],
        );
    }
}
#[allow(clippy::too_many_arguments)] // Cuboid transform, texture and animation are independent inputs.
fn part(
    mesh: &mut Mesh,
    base: Mat4,
    pivot: Vec3,
    size: Vec3,
    angle: f32,
    uv: [f32; 2],
    layer: u32,
    color: [u8; 4],
) {
    cuboid(
        mesh,
        base * Mat4::from_translation(pivot) * Mat4::from_rotation_x(angle),
        Vec3::new(-size.x * 0.5, -size.y, -size.z * 0.5),
        size,
        uv,
        layer,
        color,
    );
}
pub fn biped(
    mesh: &mut Mesh,
    base: Mat4,
    layer: u32,
    walk: f32,
    zombie: bool,
    skeleton: bool,
    color: [u8; 4],
) {
    let thickness = if skeleton { 2.0 / 16.0 } else { 4.0 / 16.0 };
    cuboid(
        mesh,
        base,
        Vec3::new(-0.25, 1.5, -0.25),
        Vec3::splat(0.5),
        [0.0, 0.0],
        layer,
        color,
    );
    cuboid(
        mesh,
        base,
        Vec3::new(-0.25, 0.75, -0.125),
        Vec3::new(0.5, 0.75, 0.25),
        [16.0, 16.0],
        layer,
        color,
    );
    for (side, phase) in [(-1.0, walk), (1.0, -walk)] {
        part(
            mesh,
            base,
            Vec3::new(side * 0.125, 0.75, 0.0),
            Vec3::new(thickness, 0.75, thickness),
            phase,
            [0.0, 16.0],
            layer,
            color,
        );
        part(
            mesh,
            base,
            Vec3::new(side * 0.375, 1.5, 0.0),
            Vec3::new(thickness, 0.75, thickness),
            if zombie { 1.5 } else { -phase },
            [40.0, 16.0],
            layer,
            color,
        );
    }
}

struct ModelPainter<'a> {
    mesh: &'a mut Mesh,
    base: Mat4,
    layer: u32,
    color: [u8; 4],
}
impl ModelPainter<'_> {
    fn part(
        &mut self,
        pivot: [f32; 3],
        min: [f32; 3],
        size: [f32; 3],
        uv: [f32; 2],
        angles: [f32; 3],
        inflate: f32,
    ) {
        let size = Vec3::from_array(size) / 16.0;
        let min = Vec3::new(min[0], -min[1], min[2]) / 16.0 - Vec3::Y * size.y;
        let center = min + size * 0.5;
        let matrix = self.base
            * Mat4::from_translation(Vec3::new(pivot[0], 24.0 - pivot[1], pivot[2]) / 16.0)
            * Mat4::from_rotation_z(-angles[2])
            * Mat4::from_rotation_y(angles[1])
            * Mat4::from_rotation_x(-angles[0])
            * Mat4::from_translation(center)
            * Mat4::from_scale((size + Vec3::splat(inflate / 8.0)) / size)
            * Mat4::from_translation(-center);
        cuboid(self.mesh, matrix, min, size, uv, self.layer, self.color);
    }
}
#[allow(dead_code)]
fn player_pose(model:&mut ModelPainter<'_>,walk:f32,pitch:f32,layers:u8,swing:f32){
    // Base skin and its separately switchable outer regions (64x64 ModelPlayer).
    for (part, pivot, min, size, uv, overlay, angle) in [
        (6,[0.,0.,0.],[-4.,-8.,-4.],[8.,8.,8.],[0.,0.],[32.,0.],-pitch),
        (1,[0.,0.,0.],[-4.,0.,-2.],[8.,12.,4.],[16.,16.],[16.,32.],0.),
        (2,[5.,2.,0.],[-1.,-2.,-2.],[4.,12.,4.],[32.,48.],[48.,48.],walk),
        (3,[-5.,2.,0.],[-3.,-2.,-2.],[4.,12.,4.],[40.,16.],[40.,32.],-walk-swing),
        (4,[1.9,12.,0.],[-2.,0.,-2.],[4.,12.,4.],[16.,48.],[0.,48.],-walk),
        (5,[-1.9,12.,0.],[-2.,0.,-2.],[4.,12.,4.],[0.,16.],[0.,32.],walk),
    ] {
        model.part(pivot,min,size,uv,[angle,0.,0.],0.);
        if layers & (1 << part) != 0 {
            model.part(pivot,min,size,overlay,[angle,0.,0.],if part==6 {0.5} else {0.25});
        }
    }
}
#[path="third_person_model.rs"]
mod third_person_model;
#[path = "inventory_player_model.rs"]
mod inventory_player_model;
pub(crate) use inventory_player_model::inventory_player;
#[derive(Clone, Copy)]
pub(crate) struct BasicMobExtra {
    pub head_angles: [f32; 2], // [pitch_rad (positive = down), delta_yaw_rad]
    pub pig_saddled: bool,
    pub sheep_sheared: bool,
    pub sheep_wool_rgb: [u8; 3],
    pub sheep_head_drop_px: f32,
}

impl Default for BasicMobExtra {
    fn default() -> Self {
        Self {
            head_angles: [0.0, 0.0],
            pig_saddled: false,
            sheep_sheared: false,
            sheep_wool_rgb: [255, 255, 255],
            sheep_head_drop_px: 0.0,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct BipedMobInput<'a> {
    pub species: u8, // 51=Skeleton, 53=Giant, 54=Zombie, 57=PigZombie
    pub subtype: u8, // 51: 1=WitherSkeleton; 54: 1=ZombieVillager
    pub baby: bool,
    pub entity_scale: f32,
    pub feet: Vec3,
    pub body_yaw_java_rad: f32,
    pub head_yaw_java_deg: Option<f32>,
    pub pitch_deg: f32,
    pub limb_swing: f32,
    pub limb_amount: f32,
    pub age: f32,
    pub alpha: f32,
    pub swing_ticks: u8,
    pub roll_z: f32,
    pub hurt: bool,
    pub invisible: bool,
    pub light: [u8; 4],
    pub texture: &'a str,
    pub held: Option<&'a Stack>,
    pub armor: [Option<&'a Stack>; 4], // [boots, leggings, chestplate, helmet]
}

#[allow(clippy::too_many_arguments)]
fn biped_box(
    mesh: &mut Mesh,
    matrix: Mat4,
    min: [f32; 3],
    size: [f32; 3],
    inflate: f32,
    uv: [f32; 2],
    mirror: bool,
    sprite: crate::resources::Sprite,
    tex_size: [f32; 2],
    tint: [u8; 3],
    extra_flags: u32,
    light: [u8; 4],
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
    .map(|a| matrix.transform_point3(Vec3::from_array(a) / 16.0));
    let faces = if mirror {
        [
            ([5, 1, 2, 6], [u + d, v + d, u, v + d + h]),
            ([0, 4, 7, 3], [u + d + w + d, v + d, u + d + w, v + d + h]),
            ([5, 4, 0, 1], [u + d + w, v, u + d, v + d]),
            ([2, 3, 7, 6], [u + d + w + w, v + d, u + d + w, v]),
            ([1, 0, 3, 2], [u + d + w, v + d, u + d, v + d + h]),
            (
                [4, 5, 6, 7],
                [u + d + w + d + w, v + d, u + d + w + d, v + d + h],
            ),
        ]
    } else {
        [
            ([5, 1, 2, 6], [u + d + w, v + d, u + d + w + d, v + d + h]),
            ([0, 4, 7, 3], [u, v + d, u + d, v + d + h]),
            ([5, 4, 0, 1], [u + d, v, u + d + w, v + d]),
            ([2, 3, 7, 6], [u + d + w, v + d, u + d + w + w, v]),
            ([1, 0, 3, 2], [u + d, v + d, u + d + w, v + d + h]),
            (
                [4, 5, 6, 7],
                [u + d + w + d, v + d, u + d + w + d + w, v + d + h],
            ),
        ]
    };
    let uv_scale = [
        sprite.width as f32 / (tex_size[0] * 256.0),
        sprite.height as f32 / (tex_size[1] * 256.0),
    ];
    let layer = sprite.layer | ATLAS | extra_flags;
    for (indices, [u1, v1, u2, v2]) in faces {
        let p0 = pts[indices[0]];
        let p1 = pts[indices[1]];
        let p2 = pts[indices[2]];
        let normal = (p1 - p0).cross(p2 - p0).normalize_or_zero();
        let shade = entity_face_shade(normal);
        let [r, g, b] = tint.map(|ch| (f32::from(ch) * shade).round().clamp(0.0, 255.0) as u8);
        let first = mesh.vertices.len() as u32;
        for (i, [s, t]) in indices
            .into_iter()
            .zip([[u2, v1], [u1, v1], [u1, v2], [u2, v2]])
        {
            mesh.vertices.push(Vertex {
                position: pts[i].to_array(),
                uv: [s * uv_scale[0], t * uv_scale[1]],
                layer,
                color: [r, g, b, 255],
                light,
            });
        }
        mesh.indices.extend([0, 1, 2, 0, 2, 3].map(|k| first + k));
        mesh.visible_faces += 1;
    }
}

pub(crate) fn draw_biped_mob(mesh: &mut Mesh, c: &ModelContext<'_>, input: BipedMobInput<'_>) {
    use crate::player::third_person::Bone;
    use std::f32::consts::{FRAC_PI_2, PI};

    let skeleton = input.species == 51;
    let zombie_villager = input.species == 54 && input.subtype == 1;
    let zombie_arms = matches!(input.species, 53 | 54 | 57);
    let bow_aim = skeleton
        && input.subtype == 0
        && input.held.is_some_and(|s| s.id == 261);

    let body_deg = input.body_yaw_java_rad.to_degrees();
    let net_head_yaw_deg = input
        .head_yaw_java_deg
        .map(|hy| ((hy - body_deg + 180.0).rem_euclid(360.0) - 180.0).clamp(-75.0, 75.0))
        .unwrap_or(0.0);
    let head_rx = input.pitch_deg.clamp(-80.0, 80.0) / 57.295776;
    let head_ry = net_head_yaw_deg / 57.295776;

    let phase = input.limb_swing * 0.6662;
    let amp = input.limb_amount;
    let swing = if input.swing_ticks > 0 {
        ((6.0 - (input.swing_ticks as f32 - input.alpha)) / 6.0).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let head = Bone {
        pivot: Vec3::ZERO,
        rotation: Vec3::new(head_rx, head_ry, 0.0),
    };
    let mut body = Bone::default();
    let mut right = Bone {
        pivot: Vec3::new(-5.0, 2.0, 0.0),
        rotation: Vec3::X * (crate::vanilla::math::cos(phase + PI) * amp),
    };
    let mut left = Bone {
        pivot: Vec3::new(5.0, 2.0, 0.0),
        rotation: Vec3::X * (crate::vanilla::math::cos(phase) * amp),
    };
    let leg_x = if skeleton { 2.0 } else { 1.9 };
    let rleg = Bone {
        pivot: Vec3::new(-leg_x, 12.0, 0.1),
        rotation: Vec3::X * (crate::vanilla::math::cos(phase) * 1.4 * amp),
    };
    let lleg = Bone {
        pivot: Vec3::new(leg_x, 12.0, 0.1),
        rotation: Vec3::X * (crate::vanilla::math::cos(phase + PI) * 1.4 * amp),
    };

    let roll = crate::vanilla::math::cos(input.age * 0.09) * 0.05 + 0.05;
    let idle = crate::vanilla::math::sin(input.age * 0.067) * 0.05;

    if zombie_arms {
        // ModelZombie.setRotationAngles: arms reach forward -90 deg + breathing + melee thrust
        let f = crate::vanilla::math::sin(swing * PI);
        let f1 = crate::vanilla::math::sin((1.0 - (1.0 - swing) * (1.0 - swing)) * PI);
        let thrust = -(f * 1.2 - f1 * 0.4);
        right.rotation = Vec3::new(-FRAC_PI_2 + thrust + idle, -(0.1 - f * 0.6), roll);
        left.rotation = Vec3::new(-FRAC_PI_2 + thrust - idle, 0.1 - f * 0.6, -roll);
    } else if bow_aim {
        // ModelSkeleton.setRotationAngles: two-arm bow aim tracking target pitch/yaw + shot recoil
        let f = crate::vanilla::math::sin(swing * PI);
        let f1 = crate::vanilla::math::sin((1.0 - (1.0 - swing) * (1.0 - swing)) * PI);
        let recoil = -(f * 1.2 - f1 * 0.4);
        right.rotation = Vec3::new(
            -FRAC_PI_2 + head.rotation.x + recoil + idle,
            -(0.1 - f * 0.6) + head.rotation.y,
            roll,
        );
        left.rotation = Vec3::new(
            -FRAC_PI_2 + head.rotation.x + recoil - idle,
            (0.1 - f * 0.6) + head.rotation.y + 0.4,
            -roll,
        );
    } else {
        // ModelBiped melee pose (Wither Skeleton / sword-wielding Skeleton)
        if input.held.is_some() {
            right.rotation.x = right.rotation.x * 0.5 - 0.31415927;
        }
        if swing > 0.0 {
            body.rotation.y = crate::vanilla::math::sin(swing.sqrt() * PI * 2.0) * 0.2;
            right.pivot.z = crate::vanilla::math::sin(body.rotation.y) * 5.0;
            right.pivot.x = -crate::vanilla::math::cos(body.rotation.y) * 5.0;
            left.pivot.z = -right.pivot.z;
            left.pivot.x = -right.pivot.x;
            right.rotation.y += body.rotation.y;
            left.rotation.y += body.rotation.y;
            left.rotation.x += body.rotation.y;
            let t = 1.0 - swing;
            let t2 = t * t;
            let f = crate::vanilla::math::sin((1.0 - t2 * t2) * PI);
            let g = crate::vanilla::math::sin(swing * PI) * -(head.rotation.x - 0.7) * 0.75;
            right.rotation.x -= f * 1.2 + g;
            right.rotation.y += body.rotation.y * 2.0;
            right.rotation.z += crate::vanilla::math::sin(swing * PI) * -0.4;
        }
        right.rotation.z += roll;
        left.rotation.z -= roll;
        right.rotation.x += idle;
        left.rotation.x -= idle;
    }

    let root = Mat4::from_translation(input.feet)
        * Mat4::from_rotation_y(PI - input.body_yaw_java_rad)
        * Mat4::from_rotation_z(input.roll_z)
        * Mat4::from_scale(Vec3::new(
            -input.entity_scale,
            -input.entity_scale,
            input.entity_scale,
        ))
        * Mat4::from_translation(Vec3::new(0.0, -1.5078125, 0.0));

    let (head_root, body_root) = if input.baby {
        (
            root * Mat4::from_translation(Vec3::new(0.0, 12.0 / 16.0, 0.0))
                * Mat4::from_scale(Vec3::splat(0.75)),
            root * Mat4::from_translation(Vec3::new(0.0, 12.0 / 16.0, 0.0))
                * Mat4::from_scale(Vec3::splat(0.5)),
        )
    } else {
        (root, root)
    };

    let hurt_flag = if input.hurt {
        crate::entity_visual::HURT
    } else {
        0
    };
    let sprite = c.resources.sprite(input.texture);
    let tex_h = if sprite.height >= sprite.width {
        64.0
    } else {
        32.0
    };
    let tex_size = [64.0, tex_h];

    let head_mat = head_root * head.transform();
    let body_mat = body_root * body.transform();
    let r_arm_mat = body_root * right.transform();
    let l_arm_mat = body_root * left.transform();
    let r_leg_mat = body_root * rleg.transform();
    let l_leg_mat = body_root * lleg.transform();

    if !input.invisible {
        if zombie_villager {
            biped_box(
                mesh,
                head_mat,
                [-4.0, -10.0, -4.0],
                [8.0, 10.0, 8.0],
                0.0,
                [0.0, 32.0],
                false,
                sprite,
                tex_size,
                [255; 3],
                hurt_flag,
                input.light,
            );
            biped_box(
                mesh,
                head_mat,
                [-1.0, -3.0, -6.0],
                [2.0, 4.0, 2.0],
                0.0,
                [24.0, 32.0],
                false,
                sprite,
                tex_size,
                [255; 3],
                hurt_flag,
                input.light,
            );
        } else {
            biped_box(
                mesh,
                head_mat,
                [-4.0, -8.0, -4.0],
                [8.0, 8.0, 8.0],
                0.0,
                [0.0, 0.0],
                false,
                sprite,
                tex_size,
                [255; 3],
                hurt_flag,
                input.light,
            );
            biped_box(
                mesh,
                head_mat,
                [-4.0, -8.0, -4.0],
                [8.0, 8.0, 8.0],
                0.5,
                [32.0, 0.0],
                false,
                sprite,
                tex_size,
                [255; 3],
                hurt_flag,
                input.light,
            );
        }

        biped_box(
            mesh,
            body_mat,
            [-4.0, 0.0, -2.0],
            [8.0, 12.0, 4.0],
            0.0,
            [16.0, 16.0],
            false,
            sprite,
            tex_size,
            [255; 3],
            hurt_flag,
            input.light,
        );

        if skeleton {
            biped_box(
                mesh,
                r_arm_mat,
                [-1.0, -2.0, -1.0],
                [2.0, 12.0, 2.0],
                0.0,
                [40.0, 16.0],
                false,
                sprite,
                tex_size,
                [255; 3],
                hurt_flag,
                input.light,
            );
            biped_box(
                mesh,
                l_arm_mat,
                [-1.0, -2.0, -1.0],
                [2.0, 12.0, 2.0],
                0.0,
                [40.0, 16.0],
                true,
                sprite,
                tex_size,
                [255; 3],
                hurt_flag,
                input.light,
            );
            biped_box(
                mesh,
                r_leg_mat,
                [-1.0, 0.0, -1.0],
                [2.0, 12.0, 2.0],
                0.0,
                [0.0, 16.0],
                false,
                sprite,
                tex_size,
                [255; 3],
                hurt_flag,
                input.light,
            );
            biped_box(
                mesh,
                l_leg_mat,
                [-1.0, 0.0, -1.0],
                [2.0, 12.0, 2.0],
                0.0,
                [0.0, 16.0],
                true,
                sprite,
                tex_size,
                [255; 3],
                hurt_flag,
                input.light,
            );
        } else {
            biped_box(
                mesh,
                r_arm_mat,
                [-3.0, -2.0, -2.0],
                [4.0, 12.0, 4.0],
                0.0,
                [40.0, 16.0],
                false,
                sprite,
                tex_size,
                [255; 3],
                hurt_flag,
                input.light,
            );
            biped_box(
                mesh,
                l_arm_mat,
                [-1.0, -2.0, -2.0],
                [4.0, 12.0, 4.0],
                0.0,
                [40.0, 16.0],
                true,
                sprite,
                tex_size,
                [255; 3],
                hurt_flag,
                input.light,
            );
            biped_box(
                mesh,
                r_leg_mat,
                [-2.0, 0.0, -2.0],
                [4.0, 12.0, 4.0],
                0.0,
                [0.0, 16.0],
                false,
                sprite,
                tex_size,
                [255; 3],
                hurt_flag,
                input.light,
            );
            biped_box(
                mesh,
                l_leg_mat,
                [-2.0, 0.0, -2.0],
                [4.0, 12.0, 4.0],
                0.0,
                [0.0, 16.0],
                true,
                sprite,
                tex_size,
                [255; 3],
                hurt_flag,
                input.light,
            );
        }
    }

    // LayerBipedArmor: armor[0]=boots, armor[1]=leggings, armor[2]=chestplate, armor[3]=helmet
    for (slot_idx, armor_opt) in input.armor.into_iter().enumerate() {
        let Some(armor) = armor_opt.filter(|s| (298..=317).contains(&s.id)) else {
            continue;
        };
        let mat_name =
            ["leather", "chainmail", "iron", "diamond", "gold"][usize::from(armor.id - 298) / 4];
        let is_legs = slot_idx == 1;
        let layer_num = if is_legs { 2 } else { 1 };
        let inflate = if is_legs { 0.5 } else { 1.0 };
        let armor_sprite = c
            .resources
            .sprite(&format!("textures/models/armor/{mat_name}_layer_{layer_num}.png"));
        let tint = if mat_name == "leather" {
            use crate::vanilla::item_nbt::{compound, number};
            let rgb = armor
                .nbt
                .as_deref()
                .and_then(compound)
                .and_then(|v| v.get("display"))
                .and_then(compound)
                .and_then(|v| v.get("color"))
                .map_or(10511680, |v| number(Some(v)) as u32);
            let [_, r, g, b] = rgb.to_be_bytes();
            [r, g, b]
        } else {
            [255; 3]
        };
        let draw_armor_pass = |mesh: &mut Mesh, sp: crate::resources::Sprite, col: [u8; 3]| match slot_idx {
            3 => {
                let y_off = if zombie_villager { -2.0 } else { 0.0 };
                biped_box(
                    mesh,
                    head_mat,
                    [-4.0, -8.0 + y_off, -4.0],
                    [8.0, 8.0, 8.0],
                    inflate,
                    [0.0, 0.0],
                    false,
                    sp,
                    [64.0, 32.0],
                    col,
                    hurt_flag,
                    input.light,
                );
                biped_box(
                    mesh,
                    head_mat,
                    [-4.0, -8.0 + y_off, -4.0],
                    [8.0, 8.0, 8.0],
                    inflate + 0.5,
                    [32.0, 0.0],
                    false,
                    sp,
                    [64.0, 32.0],
                    col,
                    hurt_flag,
                    input.light,
                );
            }
            2 => {
                biped_box(
                    mesh,
                    body_mat,
                    [-4.0, 0.0, -2.0],
                    [8.0, 12.0, 4.0],
                    inflate,
                    [16.0, 16.0],
                    false,
                    sp,
                    [64.0, 32.0],
                    col,
                    hurt_flag,
                    input.light,
                );
                biped_box(
                    mesh,
                    r_arm_mat,
                    [-3.0, -2.0, -2.0],
                    [4.0, 12.0, 4.0],
                    inflate,
                    [40.0, 16.0],
                    false,
                    sp,
                    [64.0, 32.0],
                    col,
                    hurt_flag,
                    input.light,
                );
                biped_box(
                    mesh,
                    l_arm_mat,
                    [-1.0, -2.0, -2.0],
                    [4.0, 12.0, 4.0],
                    inflate,
                    [40.0, 16.0],
                    true,
                    sp,
                    [64.0, 32.0],
                    col,
                    hurt_flag,
                    input.light,
                );
            }
            1 => {
                biped_box(
                    mesh,
                    body_mat,
                    [-4.0, 0.0, -2.0],
                    [8.0, 12.0, 4.0],
                    inflate,
                    [16.0, 16.0],
                    false,
                    sp,
                    [64.0, 32.0],
                    col,
                    hurt_flag,
                    input.light,
                );
                biped_box(
                    mesh,
                    r_leg_mat,
                    [-2.0, 0.0, -2.0],
                    [4.0, 12.0, 4.0],
                    inflate,
                    [0.0, 16.0],
                    false,
                    sp,
                    [64.0, 32.0],
                    col,
                    hurt_flag,
                    input.light,
                );
                biped_box(
                    mesh,
                    l_leg_mat,
                    [-2.0, 0.0, -2.0],
                    [4.0, 12.0, 4.0],
                    inflate,
                    [0.0, 16.0],
                    true,
                    sp,
                    [64.0, 32.0],
                    col,
                    hurt_flag,
                    input.light,
                );
            }
            0 => {
                biped_box(
                    mesh,
                    r_leg_mat,
                    [-2.0, 0.0, -2.0],
                    [4.0, 12.0, 4.0],
                    inflate,
                    [0.0, 16.0],
                    false,
                    sp,
                    [64.0, 32.0],
                    col,
                    hurt_flag,
                    input.light,
                );
                biped_box(
                    mesh,
                    l_leg_mat,
                    [-2.0, 0.0, -2.0],
                    [4.0, 12.0, 4.0],
                    inflate,
                    [0.0, 16.0],
                    true,
                    sp,
                    [64.0, 32.0],
                    col,
                    hurt_flag,
                    input.light,
                );
            }
            _ => {}
        };
        let armor_start = mesh.vertices.len();
        draw_armor_pass(mesh, armor_sprite, tint);
        if mat_name == "leather" {
            let overlay = c.resources.sprite(&format!(
                "textures/models/armor/leather_layer_{layer_num}_overlay.png"
            ));
            draw_armor_pass(mesh, overlay, [255; 3]);
        }
        if crate::ui::creative::world_glint(armor) {
            for v in &mut mesh.vertices[armor_start..] {
                v.layer |= GLINT;
                v.light[2..4].copy_from_slice(&[
                    ((v.uv[0] * 16.0).fract() * 255.0) as u8,
                    ((v.uv[1] * 16.0).fract() * 255.0) as u8,
                ]);
            }
        }
    }

    // LayerCustomHead: mob wearing a skull (397) or block (e.g. Pumpkin 86 / Jack o'Lantern 91)
    if let Some(head_item) = input.armor[3] {
        if head_item.id == 397 {
            let skull_start = mesh.vertices.len();
            crate::skull::draw_equipped(
                mesh,
                head_mat,
                head_item.meta as u8,
                crate::player_skins::item_skull_owner(head_item.nbt.as_deref()),
                c.resources,
                Some(&c.sim.presentation),
                input.light,
                input.hurt,
            );
            for v in &mut mesh.vertices[skull_start..] {
                v.light[..2].copy_from_slice(&input.light[..2]);
            }
        } else if head_item.id < 256 {
            let state = (head_item.id << 4) | (head_item.meta & 15);
            if let Some(faces) = c.assets.block_models.carried(state) {
                let block_mat = head_mat
                    * Mat4::from_translation(Vec3::new(0.0, -0.25, 0.0))
                    * Mat4::from_rotation_y(PI)
                    * Mat4::from_scale(Vec3::new(0.625, -0.625, -0.625))
                    * Mat4::from_translation(Vec3::new(-0.5, -0.5, -0.5));
                let reverse = block_mat.determinant() < 0.0;
                for f in faces {
                    let first = mesh.vertices.len() as u32;
                    for v in f.vertices {
                        mesh.vertices.push(Vertex {
                            position: block_mat.transform_point3(v.position).to_array(),
                            uv: v.uv,
                            layer: f.layer | hurt_flag,
                            color: [f.shade, f.shade, f.shade, 255],
                            light: input.light,
                        });
                    }
                    let order = if reverse {
                        [0, 2, 1, 0, 3, 2]
                    } else {
                        [0, 1, 2, 0, 2, 3]
                    };
                    mesh.indices.extend(order.map(|k| first + k));
                    mesh.visible_faces += 1;
                }
            }
        }
    }

    // LayerHeldItem: attach third_person_item directly to the animated right arm matrix
    if let Some(item) = input.held {
        let mut item_arm = right;
        if skeleton {
            // ModelSkeleton.postRenderArm centers held items in the 2px skeleton hand
            item_arm.pivot.x += 1.0;
        }
        let item_start = mesh.vertices.len();
        third_person_item(
            mesh,
            body_root * item_arm.transform(),
            item,
            c.assets,
            c.resources,
            false,
            false,
        );
        for v in &mut mesh.vertices[item_start..] {
            v.light[..2].copy_from_slice(&input.light[..2]);
            v.layer |= hurt_flag;
        }
    }
}

fn draw_creeper_charge(
    mesh: &mut Mesh,
    base: Mat4,
    stride: (f32, f32),
    head_angles: [f32; 2],
    age_ticks: f32,
    resources: &Resources,
) {
    let sprite = resources.sprite("textures/entity/creeper/creeper_armor.png");
    let (phase, amount) = stride;
    let walk = crate::vanilla::math::cos(phase * 0.6662) * 1.4 * amount;
    let layer = sprite.layer | ATLAS | BLEND | UNLIT;
    let uv_scale = [
        sprite.width as f32 / (64.0 * 256.0),
        sprite.height as f32 / (32.0 * 256.0),
    ];
    let head = [head_angles[0], -head_angles[1], 0.0];
    let zero = [0.0; 3];
    let parts: [([f32; 3], [f32; 3], [f32; 3], [f32; 2], [f32; 3]); 6] = [
        ([0.0, 6.0, 0.0], [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0.0, 0.0], head),
        ([0.0, 6.0, 0.0], [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [16.0, 16.0], zero),
        ([-2.0, 18.0, 4.0], [-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [0.0, 16.0], [walk, 0.0, 0.0]),
        ([2.0, 18.0, 4.0], [-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [0.0, 16.0], [-walk, 0.0, 0.0]),
        ([-2.0, 18.0, -4.0], [-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [0.0, 16.0], [-walk, 0.0, 0.0]),
        ([2.0, 18.0, -4.0], [-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [0.0, 16.0], [walk, 0.0, 0.0]),
    ];
    let inflate = 2.0f32;
    for (pivot, min_px, size_px, uv, angles) in parts {
        let size = Vec3::from_array(size_px) / 16.0;
        let min = Vec3::new(min_px[0], -min_px[1], min_px[2]) / 16.0 - Vec3::Y * size.y;
        let center = min + size * 0.5;
        let matrix = base
            * Mat4::from_translation(Vec3::new(pivot[0], 24.0 - pivot[1], pivot[2]) / 16.0)
            * Mat4::from_rotation_z(-angles[2])
            * Mat4::from_rotation_y(angles[1])
            * Mat4::from_rotation_x(-angles[0])
            * Mat4::from_translation(center)
            * Mat4::from_scale((size + Vec3::splat(inflate / 8.0)) / size)
            * Mat4::from_translation(-center);
        let max = min + size;
        let [w, h, d] = size_px;
        let [u, v] = uv;
        let faces = [
            (
                [
                    Vec3::new(min.x, min.y, min.z),
                    Vec3::new(min.x, min.y, max.z),
                    Vec3::new(min.x, max.y, max.z),
                    Vec3::new(min.x, max.y, min.z),
                ],
                [u, v + d, d, h],
            ),
            (
                [
                    Vec3::new(max.x, min.y, max.z),
                    Vec3::new(max.x, min.y, min.z),
                    Vec3::new(max.x, max.y, min.z),
                    Vec3::new(max.x, max.y, max.z),
                ],
                [u + d + w, v + d, d, h],
            ),
            (
                [
                    Vec3::new(max.x, min.y, min.z),
                    Vec3::new(min.x, min.y, min.z),
                    Vec3::new(min.x, max.y, min.z),
                    Vec3::new(max.x, max.y, min.z),
                ],
                [u + d, v + d, w, h],
            ),
            (
                [
                    Vec3::new(min.x, min.y, max.z),
                    Vec3::new(max.x, min.y, max.z),
                    Vec3::new(max.x, max.y, max.z),
                    Vec3::new(min.x, max.y, max.z),
                ],
                [u + d + w + d, v + d, w, h],
            ),
            (
                [
                    Vec3::new(min.x, max.y, max.z),
                    Vec3::new(max.x, max.y, max.z),
                    Vec3::new(max.x, max.y, min.z),
                    Vec3::new(min.x, max.y, min.z),
                ],
                [u + d, v, w, d],
            ),
            (
                [
                    Vec3::new(min.x, min.y, min.z),
                    Vec3::new(max.x, min.y, min.z),
                    Vec3::new(max.x, min.y, max.z),
                    Vec3::new(min.x, min.y, max.z),
                ],
                [u + d + w, v, w, d],
            ),
        ];
        for (p, [fu, fv, fw, fh]) in faces {
            let pts = p.map(|pt| matrix.transform_point3(pt));
            let u0 = (fu + age_ticks * 0.64).rem_euclid((64.0 - fw).max(1.0));
            let v0 = (fv + age_ticks * 0.32).rem_euclid((32.0 - fh).max(1.0));
            let rect = [
                u0 * uv_scale[0],
                v0 * uv_scale[1],
                fw * uv_scale[0],
                fh * uv_scale[1],
            ];
            face(mesh, pts, rect, layer, [128, 128, 128, 190]);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn mob_model(
    mesh: &mut Mesh,
    kind: MobKind,
    base: Mat4,
    layer: u32,
    stride: (f32, f32),
    color: [u8; 4],
    resources: &Resources,
    extra: BasicMobExtra,
) {
    let (phase, amount) = stride;
    let walk = crate::vanilla::math::cos(phase * 0.6662) * 1.4 * amount;
    let mut p = ModelPainter {
        mesh,
        base,
        layer,
        color,
    };
    let zero = [0.0; 3];
    let head = [extra.head_angles[0], -extra.head_angles[1], 0.0];
    match kind {
        MobKind::Villager => {
            p.part(
                [0., 0., 0.],
                [-4., -10., -4.],
                [8., 10., 8.],
                [0., 0.],
                head,
                0.,
            );
            p.part(
                [0., 0., 0.],
                [-1., -3., -6.],
                [2., 4., 2.],
                [24., 0.],
                head,
                0.,
            );
            p.part(
                [0., 0., 0.],
                [-4., 0., -3.],
                [8., 12., 6.],
                [16., 20.],
                zero,
                0.,
            );
            p.part(
                [0., 0., 0.],
                [-4., 0., -3.],
                [8., 18., 6.],
                [0., 38.],
                zero,
                0.5,
            );
            p.part(
                [0., 3., -1.],
                [-8., -2., -2.],
                [4., 8., 4.],
                [44., 22.],
                [-0.75, 0., 0.],
                0.,
            );
            p.part(
                [0., 3., -1.],
                [4., -2., -2.],
                [4., 8., 4.],
                [44., 22.],
                [-0.75, 0., 0.],
                0.,
            );
            p.part(
                [0., 3., -1.],
                [-4., 2., -2.],
                [8., 4., 4.],
                [40., 38.],
                [-0.75, 0., 0.],
                0.,
            );
            for (x, phase) in [(-2., walk * 0.5), (2., -walk * 0.5)] {
                p.part(
                    [x, 12., 0.],
                    [-2., 0., -2.],
                    [4., 12., 4.],
                    [0., 22.],
                    [phase, 0., 0.],
                    0.,
                );
            }
        }
        MobKind::Zombie | MobKind::Skeleton => biped(
            p.mesh,
            base,
            layer,
            walk,
            kind == MobKind::Zombie,
            kind == MobKind::Skeleton,
            color,
        ),
        MobKind::Creeper => {
            p.part(
                [0.0, 6.0, 0.0],
                [-4.0, -8.0, -4.0],
                [8.0, 8.0, 8.0],
                [0.0, 0.0],
                head,
                0.0,
            );
            p.part(
                [0.0, 6.0, 0.0],
                [-4.0, 0.0, -2.0],
                [8.0, 12.0, 4.0],
                [16.0, 16.0],
                zero,
                0.0,
            );
            for (x, z, phase) in [
                (-2.0, 4.0, walk),
                (2.0, 4.0, -walk),
                (-2.0, -4.0, -walk),
                (2.0, -4.0, walk),
            ] {
                p.part(
                    [x, 18.0, z],
                    [-2.0, 0.0, -2.0],
                    [4.0, 6.0, 4.0],
                    [0.0, 16.0],
                    [phase, 0.0, 0.0],
                    0.0,
                );
            }
        }
        MobKind::Chicken => {
            p.part(
                [0.0, 15.0, -4.0],
                [-2.0, -6.0, -2.0],
                [4.0, 6.0, 3.0],
                [0.0, 0.0],
                head,
                0.0,
            );
            p.part(
                [0.0, 15.0, -4.0],
                [-2.0, -4.0, -4.0],
                [4.0, 2.0, 2.0],
                [14.0, 0.0],
                head,
                0.0,
            );
            p.part(
                [0.0, 15.0, -4.0],
                [-1.0, -2.0, -3.0],
                [2.0, 2.0, 2.0],
                [14.0, 4.0],
                head,
                0.0,
            );
            p.part(
                [0.0, 16.0, 0.0],
                [-3.0, -4.0, -3.0],
                [6.0, 8.0, 6.0],
                [0.0, 9.0],
                [std::f32::consts::FRAC_PI_2, 0.0, 0.0],
                0.0,
            );
            for (x, phase) in [(-2.0, walk), (1.0, -walk)] {
                p.part(
                    [x, 19.0, 1.0],
                    [-1.0, 0.0, -3.0],
                    [3.0, 5.0, 3.0],
                    [26.0, 0.0],
                    [phase, 0.0, 0.0],
                    0.0,
                );
            }
            for x in [-4.0, 3.0] {
                p.part(
                    [x, 13.0, 0.0],
                    [0.0, 0.0, -3.0],
                    [1.0, 4.0, 6.0],
                    [24.0, 13.0],
                    zero,
                    0.0,
                );
            }
        }
        MobKind::Spider => {
            p.part(
                [0.0, 15.0, -3.0],
                [-4.0, -4.0, -8.0],
                [8.0, 8.0, 8.0],
                [32.0, 4.0],
                head,
                0.0,
            );
            p.part(
                [0.0, 15.0, 0.0],
                [-3.0, -3.0, -3.0],
                [6.0, 6.0, 6.0],
                [0.0, 0.0],
                zero,
                0.0,
            );
            p.part(
                [0.0, 15.0, 9.0],
                [-5.0, -4.0, -6.0],
                [10.0, 8.0, 12.0],
                [0.0, 12.0],
                zero,
                0.0,
            );
            for i in 0..4 {
                for side in [-1.0, 1.0] {
                    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, FRAC_PI_8, PI};
                    let offset = [0., PI, FRAC_PI_2, 3. * FRAC_PI_2][i];
                    let spread = [2., 1., -1., -2.][i] * FRAC_PI_8;
                    let pitch = [1., 0.74, 0.74, 1.][i] * FRAC_PI_4;
                    let yaw_delta =
                        crate::vanilla::math::cos(phase * 0.6662 * 2. + offset) * 0.4 * amount;
                    let lift =
                        crate::vanilla::math::sin(phase * 0.6662 + offset).abs() * 0.4 * amount;
                    p.part(
                        [side * 4.0, 15.0, 2.0 - i as f32],
                        [if side < 0.0 { -15.0 } else { -1.0 }, -1.0, -1.0],
                        [16.0, 2.0, 2.0],
                        [18.0, 0.0],
                        [0.0, side * (yaw_delta - spread), side * (pitch - lift)],
                        0.0,
                    );
                }
            }
        }
        MobKind::Cow | MobKind::Pig | MobKind::Sheep => {
            let cow = kind == MobKind::Cow;
            let sheep = kind == MobKind::Sheep;
            let pig = kind == MobKind::Pig;
            let leg = if pig { 6.0 } else { 12.0 };
            let headpivot = if cow {
                [0.0, 4.0, -8.0]
            } else if sheep {
                [0.0, 6.0 + extra.sheep_head_drop_px, -8.0]
            } else {
                [0.0, 12.0, -6.0]
            };
            let (headmin, headsize) = if cow {
                ([-4.0, -4.0, -6.0], [8.0, 8.0, 6.0])
            } else if sheep {
                ([-3.0, -4.0, -6.0], [6.0, 6.0, 8.0])
            } else {
                ([-4.0, -4.0, -8.0], [8.0, 8.0, 8.0])
            };
            p.part(headpivot, headmin, headsize, [0.0, 0.0], head, 0.0);
            if cow {
                for x in [-5.0, 4.0] {
                    p.part(
                        headpivot,
                        [x, -5.0, -4.0],
                        [1.0, 3.0, 1.0],
                        [22.0, 0.0],
                        head,
                        0.0,
                    );
                }
            } else if !sheep {
                p.part(
                    headpivot,
                    [-2.0, 0.0, -9.0],
                    [4.0, 3.0, 1.0],
                    [16.0, 16.0],
                    head,
                    0.0,
                );
            }
            let bodypivot = [0.0, 17.0 - leg, 2.0];
            let bodymin = if cow {
                [-6.0, -10.0, -7.0]
            } else if sheep {
                [-4.0, -10.0, -7.0]
            } else {
                [-5.0, -10.0, -7.0]
            };
            let bodysize = if cow {
                [12.0, 18.0, 10.0]
            } else if sheep {
                [8.0, 16.0, 6.0]
            } else {
                [10.0, 16.0, 8.0]
            };
            let bodyuv = if cow { [18.0, 4.0] } else { [28.0, 8.0] };
            p.part(
                bodypivot,
                bodymin,
                bodysize,
                bodyuv,
                [std::f32::consts::FRAC_PI_2, 0.0, 0.0],
                0.0,
            );
            if cow {
                p.part(
                    bodypivot,
                    [-2.0, 2.0, -8.0],
                    [4.0, 6.0, 1.0],
                    [52.0, 0.0],
                    [std::f32::consts::FRAC_PI_2, 0.0, 0.0],
                    0.0,
                );
            }
            let legs = [
                (-3.0, 7.0, walk),
                (3.0, 7.0, -walk),
                (-3.0, -5.0, -walk),
                (3.0, -5.0, walk),
            ];
            for (x, z, phase) in legs {
                let x = if cow { x / 3.0 * 4.0 } else { x };
                p.part(
                    [x, 24.0 - leg, z],
                    [-2.0, 0.0, -2.0],
                    [4.0, leg, 4.0],
                    [0.0, 16.0],
                    [phase, 0.0, 0.0],
                    0.0,
                );
            }
            if pig && extra.pig_saddled {
                p.layer = resources
                    .sprite("textures/entity/pig/pig_saddle.png")
                    .layer
                    | ATLAS;
                p.part(
                    bodypivot,
                    bodymin,
                    bodysize,
                    bodyuv,
                    [std::f32::consts::FRAC_PI_2, 0.0, 0.0],
                    0.5,
                );
            }
            if sheep && !extra.sheep_sheared {
                p.layer = resources
                    .sprite("textures/entity/sheep/sheep_fur.png")
                    .layer
                    | ATLAS;
                p.color = [
                    extra.sheep_wool_rgb[0],
                    extra.sheep_wool_rgb[1],
                    extra.sheep_wool_rgb[2],
                    color[3],
                ];
                p.part(
                    headpivot,
                    [-3.0, -4.0, -4.0],
                    [6.0, 6.0, 6.0],
                    [0.0, 0.0],
                    head,
                    0.6,
                );
                p.part(
                    bodypivot,
                    [-4.0, -10.0, -7.0],
                    [8.0, 16.0, 6.0],
                    [28.0, 8.0],
                    [std::f32::consts::FRAC_PI_2, 0.0, 0.0],
                    1.75,
                );
                for (x, z, phase) in legs {
                    p.part(
                        [x, 12., z],
                        [-2., 0., -2.],
                        [4., 6., 4.],
                        [0., 16.],
                        [phase, 0., 0.],
                        0.5,
                    );
                }
            }
        }
        _ => unreachable!("species rendered by its imported 1.8.9 mesh"),
    }
}
fn block_cube(mesh: &mut Mesh, base: Mat4, state: u16, size: f32, assets: &Assets) {
    for axis in 0..3 {
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        for positive in [false, true] {
            let mut p = Vec3::splat(-size / 2.0);
            if positive {
                p[axis] += size;
            }
            let mut du = Vec3::ZERO;
            du[u] = size;
            let mut dv = Vec3::ZERO;
            dv[v] = size;
            let points = if positive {
                [p, p + du, p + du + dv, p + dv]
            } else {
                [p + du, p, p + dv, p + du + dv]
            };
            let shade = match (axis, positive) {
                (1, true) => 255,
                (1, false) => 127,
                (2, _) => 204,
                _ => 153,
            };
            face(
                mesh,
                points.map(|p| base.transform_point3(p)),
                [0.0, 0.0, 1.0, 1.0],
                assets.texture(state, axis, positive),
                [shade, shade, shade, 255],
            );
        }
    }
}
#[path = "item_models.rs"]
mod item_models;
pub(crate) use item_models::third_person_item;

#[derive(serde::Serialize)]
pub(crate) struct RenderGeometryWarmup {
    pub item_models: usize,
    pub item_ms: f64,
    pub player_models: usize,
    pub player_ms: f64,
    pub vehicle_models: usize,
    pub vehicle_ms: f64,
}
/// Prepare embedded immutable geometry only. Resource-pack textures, skins,
/// per-entity state and animation remain resolved by the normal draw paths.
pub(crate) fn warm_render_geometry() -> RenderGeometryWarmup {
    let started = std::time::Instant::now();
    let item_models = item_models::warm_geometry();
    let item_ms = started.elapsed().as_secs_f64() * 1000.;
    let started = std::time::Instant::now();
    let player_models = third_person_model::warm_geometry();
    let player_ms = started.elapsed().as_secs_f64() * 1000.;
    let started = std::time::Instant::now();
    let vehicle_models = vehicle::warm_geometry();
    let vehicle_ms = started.elapsed().as_secs_f64() * 1000.;
    RenderGeometryWarmup { item_models, item_ms, player_models, player_ms, vehicle_models, vehicle_ms }
}
#[derive(Clone, Copy)]
pub struct Viewpoint {
    pub eye: Vec3,
    pub direction: Vec3,
    pub right: Vec3,
}
impl Viewpoint {
    pub fn view_at(self, eye: Vec3) -> Mat4 {
        Mat4::look_to_rh(eye, self.direction, self.right.cross(self.direction))
    }
}
pub fn camera(
    player: &Player,
    world: &World,
    settings: &Settings,
    perspective: u8,
    alpha: f32,
) -> Viewpoint {
    let mut eye = player.eye(alpha);
    if player.sleeping {
        let state=world.get(glam::DVec3::from_array(player.precise_position()).floor().as_ivec3());
        let facing=if state>>4==26 {(state&3) as usize} else {0};
        let direction = [Vec3::NEG_Z,Vec3::X,Vec3::Z,Vec3::NEG_X][facing];
        return Viewpoint { eye: eye + Vec3::Y * 0.7, direction, right: direction.cross(Vec3::Y) };
    }
    // EntityRenderer rotates by yaw and pitch. Yaw still defines screen-right at either pole.
    let [yaw, pitch] = player.java_rotation();
    let (sy, cy) = yaw.to_radians().sin_cos();
    let (sp, cp) = pitch.to_radians().sin_cos();
    let facing = if perspective == 2 { -1. } else { 1. };
    let direction = Vec3::new(-sy * cp, -sp, cy * cp) * facing;
    let right = Vec3::new(-cy, 0., -sy) * facing;
    if perspective == 0 {
        if settings.bobbing && player.grounded {
            let speed = Vec3::new(player.velocity.x, 0.0, player.velocity.z)
                .length()
                .min(0.25);
            eye.y += (player.position.x * 3.0 + player.position.z * 3.0).sin() * speed * 0.12;
        }
        // EntityRenderer.orientCamera translates the first-person view 0.1 along GL -Z.
        eye -= direction * 0.1;
    } else {
        use crate::vanilla::{math,ray_trace};
        let ry=yaw/180.*std::f32::consts::PI;
        let rp=(pitch+if perspective==2{180.}else{0.})/180.*std::f32::consts::PI;
        let offset=Vec3::new(-math::sin(ry)*math::cos(rp),-math::sin(rp),math::cos(ry)*math::cos(rp)).as_dvec3()*4.;
        let origin=player.precise_eye(alpha);
        let opts=ray_trace::Options{stop_on_liquid:false,ignore_without_box:false,return_last_miss:false};
        let mut distance=4.0f64;
        for i in 0..8{
            let bump=glam::DVec3::from_array([0,1,2].map(|bit|f64::from((((i>>bit)&1)*2-1) as f32*0.1)));
            // The original adds the Z corner offset to target X as well.
            if let Some(hit)=ray_trace::trace(world,origin+bump,origin-offset+bump+glam::DVec3::X*bump.z,opts){
                distance=distance.min((hit.point-origin).length() as f32 as f64);
            }
        }
        eye=(origin-direction.as_dvec3()*distance).as_vec3();
    }
    Viewpoint { eye, direction, right }
}
pub struct Dynamic {
    pub opaque: Mesh,
    pub translucent: Mesh,
    pub nametags: Mesh,
    pub visible: usize,
    pub total: usize,
    pub culled: usize,
}
impl Default for Dynamic {
    fn default() -> Self {
        Self {
            opaque: empty(),
            translucent: empty(),
            nametags: empty(),
            visible: 0,
            total: 0,
            culled: 0,
        }
    }
}
pub struct ModelContext<'a> {
    pub sim: &'a Simulation,
    pub player: &'a Player,
    pub world: &'a World,
    pub assets: &'a Assets,
    pub resources: &'a Resources,
    pub settings: &'a Settings,
    pub frustum: &'a Frustum,
    pub viewport_height: u32,
    pub eye: Vec3,
    pub alpha: f32,
    pub perspective: u8,
    pub render_hand: bool,
    pub hitboxes: bool,
}
impl ModelContext<'_> {
    pub(crate) fn fire(&self, mesh: &mut Mesh, start: usize, pos: Vec3, width: f32, height: f32) {
        for v in &mut mesh.vertices[start..] {
            v.light = [240, 240, 0, 0];
        }
        let yaw = self.player.java_rotation()[0].to_radians() + if self.perspective == 2 { std::f32::consts::PI } else { 0. };
        crate::entity_visual::world_fire(mesh, self.assets, pos, yaw, width, height);
    }
}
pub(crate) fn occluded_bounds(world: &World, eye: Vec3, min: Vec3, max: Vec3) -> bool {
    let eps = ((max - min) * 0.15)
        .clamp(Vec3::splat(0.01), Vec3::splat(0.05))
        .min((max - min) * 0.45);
    let a = min + eps;
    let b = max - eps;
    let mid = (min + max) * 0.5;
    [
        mid,
        Vec3::new(mid.x, b.y, mid.z),
        Vec3::new(a.x, b.y, a.z),
        Vec3::new(b.x, b.y, a.z),
        Vec3::new(a.x, b.y, b.z),
        Vec3::new(b.x, b.y, b.z),
        Vec3::new(a.x, a.y, a.z),
        Vec3::new(b.x, a.y, a.z),
        Vec3::new(a.x, a.y, b.z),
        Vec3::new(b.x, a.y, b.z),
    ]
    .into_iter()
    .all(|p| world.visual_occluded(eye, p))
}
fn orb_model(mesh: &mut Mesh, c: &ModelContext<'_>, orb: &crate::gameplay::xp_orb::Orb, position: Vec3) {
    let index = orb.texture_index();
    let uv = [
        (index % 4) as f32 * 16. / 256., (index / 4) as f32 * 16. / 256.,
        16. / 256., 16. / 256.,
    ];
    let yaw = -c.player.yaw + if c.perspective == 2 { std::f32::consts::PI } else { 0. };
    let rotation = Mat4::from_rotation_y(yaw) * Mat4::from_rotation_x(c.player.pitch);
    let points = [
        Vec3::new(-0.5, -0.25, 0.), Vec3::new(0.5, -0.25, 0.),
        Vec3::new(0.5, 0.75, 0.), Vec3::new(-0.5, 0.75, 0.),
    ].map(|v| position + rotation.transform_vector3(v * 0.3));
    let start = mesh.vertices.len();
    face(mesh, points, uv, ATLAS | c.resources.sprite("textures/entity/experience_orb.png").layer, orb.color(c.alpha));
    let sample = (orb.position + glam::DVec3::Y * f64::from(0.5f32 * 0.85)).floor().as_ivec3();
    let mut light = c.world.visual_light_uv(sample, 0);
    light[0] = light[0].saturating_add(120).min(240);
    for vertex in &mut mesh.vertices[start..] { vertex.light = light; }
}
#[path = "model_build_profile.rs"]
pub(crate) mod build_profile;
#[cfg(test)]
#[path = "model_distance_tests.rs"]
mod distance_tests;

pub fn build(c: ModelContext<'_>) -> Dynamic {
    build_impl::<true>(c, &mut build_profile::Profile::default())
}
pub(crate) fn build_profiled(c: ModelContext<'_>, profile: &mut build_profile::Profile) -> Dynamic {
    build_impl::<true>(c, profile)
}
// Preserve the old center arithmetic at float boundaries. Reject only when
// neither age-dependent center could pass the existing distance test.
fn basic_distance_culled(position: Vec3, adult_width: f32, adult_height: f32,
    eye: Vec3, limit_squared: f32) -> bool {
    let outside = |scale| {
        let (width, height) = (adult_width * scale, adult_height * scale);
        let min = position - Vec3::new(width / 2., 0., width / 2.);
        let max = position + Vec3::new(width / 2., height, width / 2.);
        ((min + max) / 2.).distance_squared(eye) > limit_squared
    };
    outside(1.) && outside(0.5)
}
fn build_impl<const EARLY_DISTANCE: bool>(c: ModelContext<'_>, profile: &mut build_profile::Profile) -> Dynamic {
    let _ = c.shadow_env();
    let mut d = Dynamic {
        opaque: empty(),
        translucent: empty(),
        nametags: empty(),
        visible: 0,
        total: c.sim.drops.len() + c.sim.xp_orbs.len(),
        culled: 0,
    };
    if profile.enabled {
        profile.world_columns = c.world.chunks.len();
        profile.mobs = c.sim.mobs.len() + c.sim.dying_mobs.len();
        profile.drops = c.sim.drops.len();
        profile.orbs = c.sim.xp_orbs.len();
        profile.particles = c.sim.particles.len();
        profile.arrows = c.sim.arrows.len();
        profile.mob_shots = c.sim.mob_shots.len();
        profile.falling = c.sim.block_ticks.falling.len();
        profile.pickup_effects = c.sim.pickup_fx.0.len();
    }
    let started = profile.timer();
    crate::banner::render::visible(&c, &mut d.opaque);
    crate::skull::visible(&c, &mut d.opaque);
    crate::sign_render::visible(&c, &mut d.opaque, &mut d.translucent);
    spawner_render::render(&mut d, &c);
    end_portal_render::render(&mut d, &c);
    crate::mesh::moving_pistons(&c, &mut d.opaque, &mut d.translucent);
    crate::mesh::primed_tnt(&c, &mut d.opaque);
    for t in &c.sim.primed_tnt {
        let feet_pos = t.previous.lerp(t.position, f64::from(c.alpha)).as_vec3();
        let center = feet_pos + Vec3::Y * 0.5;
        let remaining = t.fuse as f32 - c.alpha + 1.;
        let scale = 1. + 0.3 * (1. - remaining / 10.).clamp(0., 1.).powi(4);
        let half = Vec3::splat(0.5 * scale);
        if c.frustum.visible(center - half, center + half) {
            shadow_radius(&mut d.translucent, &c, feet_pos, 0.5, 1.0, false);
        }
    }
    profile.block_entities_ms += build_profile::elapsed(started);
    let started = profile.timer();
    let limit_squared = (c.settings.entity_distance * c.settings.entity_distance) as f32;
    for m in c.sim.mobs.iter().chain(&c.sim.dying_mobs) {
        let native_undead = matches!(m.kind, MobKind::Skeleton | MobKind::Zombie)
            && c.sim.command_entities.mobs.contains_key(&m.id);
        if native_undead || !matches!(m.kind, MobKind::Cow | MobKind::Pig | MobKind::Sheep | MobKind::Chicken |
            MobKind::Zombie | MobKind::Skeleton | MobKind::Creeper | MobKind::Spider | MobKind::Villager) {
            if profile.enabled { profile.native_mobs += 1; }
            // draw_at uses interpolated feet for distance, independent of NBT
            // size, baby state, pose or species bounds. Keep its strict test.
            if EARLY_DISTANCE
                && m.kind != MobKind::EnderDragon
                && m.previous.lerp(m.position, c.alpha).distance_squared(c.eye) > limit_squared
            {
                d.total += 1;
                d.culled += 1;
                if profile.enabled { profile.native_early_distance += 1; }
                continue;
            }
            crate::remote_mobs::draw_native(&mut d, &c, m);
            continue;
        }
        d.total += 1;
        let position = m.previous.lerp(m.position, c.alpha);
        let (adult_width, adult_height) = m.kind.size();
        if EARLY_DISTANCE && basic_distance_culled(position, adult_width, adult_height, c.eye, limit_squared) {
            d.culled += 1;
            if profile.enabled { profile.basic_early_distance += 1; }
            continue;
        }
        let basic = c.sim.server_scene.basic_mobs.get(&m.id);
        let data = c.sim.command_entities.mobs.get(&m.id);
        let child = basic.map_or_else(|| c.sim.is_baby(m.id), |b| b.baby);
        let scale = if child { 0.5 } else { 1. };
        let (width, height) = (adult_width * scale, adult_height * scale);
        let min = position - Vec3::new(width / 2.0, 0.0, width / 2.0);
        let max = position + Vec3::new(width / 2.0, height, width / 2.0);
        let center = (min + max) / 2.0;
        let cull_min = min - Vec3::new(0.35 * scale, 0.0, 0.35 * scale);
        let cull_max = max + Vec3::new(0.35 * scale, 0.35 * scale, 0.35 * scale);
        if center.distance_squared(c.eye)
            > (c.settings.entity_distance * c.settings.entity_distance) as f32
            || !c.frustum.visible(cull_min, cull_max)
        {
            d.culled += 1;
            continue;
        }
        let occluded =
            c.settings.entity_culling && occluded_bounds(c.world, c.eye, cull_min, cull_max);
        if occluded {
            d.culled += 1;
            continue;
        }
        d.visible += 1;
        let brain = c.sim.mob_brains.get(&m.id);
        let (yaw, phase, amplitude) = brain.map(|b| b.gait).unwrap_or_default().sample(m.yaw, c.alpha, child);
        let death_time = basic.map_or(m.death_ticks, |b| b.death_time);
        let swing_ticks = basic.map_or(m.swing_ticks, |b| b.swing_ticks);
        let eat_ticks = basic.map_or(m.eat_ticks, |b| b.eat_ticks);
        let roll_z = death_roll(death_time, c.alpha);
        let hurt = m.hurt > 0 || m.health <= 0. || death_time > 0;
        let age_ticks = m.age as f32 + c.alpha;

        let body_java_deg = yaw.to_degrees() - 180.0;
        let (head_yaw_java_deg, pitch_deg) = if let Some(b) = basic {
            (b.head_yaw, b.pitch)
        } else {
            let mut sp_head = None;
            let mut sp_pitch = data.map_or(0.0, |d| d.pitch);
            let dist_sq = position.distance_squared(c.player.position);
            let active = brain.is_some_and(|b| b.tempted || b.anger_ticks > 0 || b.attack_step > 0)
                || c.sim.trading_with == Some(m.id)
                || (m.kind.hostile() && dist_sq < 256.0);
            let watch_weight = if active {
                1.0
            } else if dist_sq < 36.0 {
                ((36.0 - dist_sq) / 12.0).clamp(0.0, 1.0)
            } else {
                0.0
            };
            if watch_weight > 0.0 && death_time == 0 {
                let look = c.player.eye(c.alpha) - (position + Vec3::Y * (height * 0.85));
                let horiz = look.x.hypot(look.z);
                if horiz > 1e-3 {
                    let target_yaw = (-look.x).atan2(look.z).to_degrees();
                    let diff = (target_yaw - body_java_deg + 180.0).rem_euclid(360.0) - 180.0;
                    if m.kind.hostile() || diff.abs() <= 75.0 {
                        let fade = if m.kind.hostile() {
                            watch_weight
                        } else {
                            watch_weight * ((75.0 - diff.abs()) / 20.0).clamp(0.0, 1.0)
                        };
                        sp_head = Some(body_java_deg + diff * fade);
                        if sp_pitch == 0.0 {
                            sp_pitch = ((-look.y).atan2(horiz).to_degrees() * fade).clamp(-40.0, 40.0);
                        }
                    }
                }
            }
            (sp_head, sp_pitch)
        };

        let light = c
            .world
            .visual_light_uv((position + Vec3::Y * (height * 0.85)).floor().as_ivec3(), 0);
        let invisible = data.is_some_and(|d| d.effects.get(14).is_some())
            || basic.is_some_and(|b| b.invisible);
        let start = d.opaque.vertices.len();

        if matches!(m.kind, MobKind::Zombie | MobKind::Skeleton) {
            let wither = m.kind == MobKind::Skeleton && basic.is_some_and(|b| b.skeleton_wither);
            let zvill = m.kind == MobKind::Zombie && basic.is_some_and(|b| b.zombie_villager);
            let subtype = u8::from(wither || zvill);
            let entity_scale = if wither { 1.2 } else { 1.0 };
            let biped_tex = if wither {
                "textures/entity/skeleton/wither_skeleton.png"
            } else if zvill {
                "textures/entity/zombie/zombie_villager.png"
            } else {
                m.kind.texture()
            };
            let sp_default_bow = Stack::new(261, 1);
            let held_ref = if let Some(b) = basic {
                b.held.as_ref()
            } else if m.kind == MobKind::Skeleton {
                Some(&sp_default_bow)
            } else {
                None
            };
            let armor_refs = if let Some(b) = basic {
                [
                    b.armor[0].as_ref(),
                    b.armor[1].as_ref(),
                    b.armor[2].as_ref(),
                    b.armor[3].as_ref(),
                ]
            } else {
                [None; 4]
            };
            draw_biped_mob(
                &mut d.opaque,
                &c,
                BipedMobInput {
                    species: m.kind.vanilla_id(),
                    subtype,
                    baby: child,
                    entity_scale,
                    feet: position,
                    body_yaw_java_rad: body_java_deg.to_radians(),
                    head_yaw_java_deg,
                    pitch_deg,
                    limb_swing: phase,
                    limb_amount: amplitude,
                    age: age_ticks,
                    alpha: c.alpha,
                    swing_ticks,
                    roll_z,
                    hurt,
                    invisible,
                    light,
                    texture: biped_tex,
                    held: held_ref,
                    armor: armor_refs,
                },
            );
        } else if !invisible {
            let delta_yaw_rad = head_yaw_java_deg
                .map(|hy| ((hy - body_java_deg + 180.0).rem_euclid(360.0) - 180.0).clamp(-75.0, 75.0))
                .unwrap_or(0.0)
                .to_radians();
            let mut head_pitch_rad = pitch_deg.clamp(-60.0, 60.0).to_radians();

            let mut extra = BasicMobExtra {
                head_angles: [head_pitch_rad, delta_yaw_rad],
                ..Default::default()
            };

            let mut creeper_flash = false;
            let mut creeper_powered = false;
            let mut base = Mat4::from_translation(position)
                * Mat4::from_rotation_y(-yaw)
                * Mat4::from_rotation_z(roll_z)
                * Mat4::from_scale(Vec3::splat(scale));

            match m.kind {
                MobKind::Pig => {
                    extra.pig_saddled = basic.map_or_else(
                        || {
                            crate::vanilla::item_nbt::number(
                                data.and_then(|d| d.extra.get("Saddle")),
                            ) != 0
                        },
                        |b| b.pig_saddled,
                    );
                }
                MobKind::Sheep => {
                    let (sheared, color) = if let Some(b) = basic {
                        (b.sheep_sheared, b.sheep_color & 15)
                    } else {
                        let sh = crate::vanilla::item_nbt::number(
                            data.and_then(|d| d.extra.get("Sheared")),
                        ) != 0;
                        let col = (crate::vanilla::item_nbt::number(
                            data.and_then(|d| d.extra.get("Color")),
                        ) & 15) as u8;
                        (sh, col)
                    };
                    let jeb = data.is_some_and(|d| d.name == "jeb_")
                        || c.sim
                            .server_scene
                            .labels
                            .iter()
                            .any(|l| l.id == m.id as i32 && l.text == "jeb_");
                    extra.sheep_sheared = sheared;
                    extra.sheep_wool_rgb =
                        sheep_wool_rgb(color, jeb, u32::from(m.age), m.id, c.alpha);
                    if eat_ticks > 0 {
                        let rem = (eat_ticks as f32 - c.alpha).max(0.0);
                        let drop_frac = if eat_ticks >= 4 && eat_ticks <= 36 {
                            1.0
                        } else if eat_ticks < 4 {
                            (rem / 4.0).clamp(0.0, 1.0)
                        } else {
                            ((40.0 - rem) / 4.0).clamp(0.0, 1.0)
                        };
                        extra.sheep_head_drop_px = drop_frac * 9.0;
                        head_pitch_rad = if eat_ticks >= 4 && eat_ticks <= 36 {
                            let f = ((eat_ticks - 4) as f32 - c.alpha) / 32.0;
                            std::f32::consts::PI / 5.0
                                + 0.2199115 * crate::vanilla::math::sin(f * 28.7)
                        } else {
                            (std::f32::consts::PI / 5.0) * drop_frac
                        };
                        extra.head_angles[0] = head_pitch_rad;
                    }
                }
                MobKind::Creeper => {
                    let lerp_fuse = if let Some(b) = basic {
                        if b.creeper_swell > 0 || b.prev_creeper_swell > 0 {
                            b.prev_creeper_swell as f32
                                + (b.creeper_swell as f32 - b.prev_creeper_swell as f32) * c.alpha
                        } else {
                            0.0
                        }
                    } else if m.fuse > 0 || m.prev_fuse > 0 {
                        m.prev_fuse as f32 + (m.fuse as f32 - m.prev_fuse as f32) * c.alpha
                    } else {
                        0.0
                    };
                    if lerp_fuse > 0.0 {
                        let f = (lerp_fuse / 28.0).clamp(0.0, 1.0);
                        let f1 = 1.0 + crate::vanilla::math::sin(f * 100.0) * f * 0.01;
                        let f3 = f * f * f;
                        let xz_scale = (1.0 + f3 * 0.4) * f1;
                        let y_scale = (1.0 + f3 * 0.1) / f1;
                        base *= Mat4::from_scale(Vec3::new(xz_scale, y_scale, xz_scale));
                        if ((lerp_fuse * 10.0) as i32) % 2 == 0 {
                            creeper_flash = true;
                        }
                    }
                    creeper_powered = basic.map_or_else(
                        || {
                            crate::vanilla::item_nbt::number(
                                data.and_then(|d| d.extra.get("powered")),
                            ) != 0
                        },
                        |b| b.creeper_powered,
                    );
                }
                _ => {}
            }

            let texture = if m.kind == MobKind::Villager {
                let prof = basic.map_or_else(
                    || c.sim.trades.get(&m.id).map_or(0, |t| t.profession),
                    |b| b.villager_profession,
                );
                match prof {
                    1 => "textures/entity/villager/librarian.png",
                    2 => "textures/entity/villager/priest.png",
                    3 => "textures/entity/villager/smith.png",
                    4 => "textures/entity/villager/butcher.png",
                    _ => m.kind.texture(),
                }
            } else {
                m.kind.texture()
            };
            let layer = c.resources.sprite(texture).layer | ATLAS;
            mob_model(
                &mut d.opaque,
                m.kind,
                base,
                layer,
                (phase, amplitude),
                [255; 4],
                c.resources,
                extra,
            );
            if hurt {
                crate::entity_visual::mark_hurt(&mut d.opaque, start);
            }
            if creeper_flash {
                for v in &mut d.opaque.vertices[start..] {
                    v.layer |= WHITE_FLASH;
                    v.color[3] = 200;
                }
            }
            for v in &mut d.opaque.vertices[start..] {
                v.light[..2].copy_from_slice(&light[..2]);
            }
            if creeper_powered {
                draw_creeper_charge(
                    &mut d.translucent,
                    base,
                    (phase, amplitude),
                    extra.head_angles,
                    age_ticks,
                    c.resources,
                );
            }
        }
        if brain.is_some_and(|b| b.burning()) || c.sim.server_scene.burning.contains(&(m.id as i32)) {
            c.fire(&mut d.opaque, start, position, width, height);
        }
        let shadow_size = match m.kind {
            MobKind::Cow | MobKind::Pig | MobKind::Sheep => 0.7,
            MobKind::Chicken => 0.3,
            MobKind::Spider => 1.0,
            _ => 0.5,
        } * if child { 0.5 } else { 1.0 };
        if !invisible {
            shadow_radius(&mut d.translucent, &c, position, shadow_size, 1.0, false);
        }
        if c.hitboxes {
            wirebox(&mut d.opaque, min, max);
        }
    }
    profile.mobs_ms += build_profile::elapsed(started);
    let started = profile.timer();
    for item in &c.sim.drops {
        let pos = item.previous.lerp(item.position, f64::from(c.alpha)).as_vec3();
        if pos.distance_squared(c.eye)
            > (c.settings.entity_distance * c.settings.entity_distance) as f32
            || !c
                .frustum
                .visible(pos - Vec3::splat(0.3), pos + Vec3::splat(0.3))
            || (c.settings.entity_culling && c.world.visual_occluded(c.eye, pos + Vec3::Y * 0.2))
        {
            d.culled += 1;
            continue;
        }
        d.visible += 1;
        let start = d.opaque.vertices.len();
        item_models::dropped(&mut d.opaque, item, c.alpha, c.assets, c.resources);
        light_since(&mut d.opaque, start, c.world, pos + Vec3::Y * 0.2125);
        if item.fire > 0 || c.sim.server_scene.burning.contains(&(item.id as i32)) {
            c.fire(&mut d.opaque, start, pos, 0.25, 0.25);
        }
        shadow_radius(&mut d.translucent, &c, pos, 0.15, 0.75, false);
    }
    for orb in &c.sim.xp_orbs {
        let position = orb.render_position(c.alpha);
        let center = position + Vec3::Y * 0.25;
        if position.distance_squared(c.eye) > (c.settings.entity_distance * c.settings.entity_distance) as f32
            || !c.frustum.visible(center - Vec3::splat(0.25), center + Vec3::splat(0.25))
            || c.settings.entity_culling && c.world.visual_occluded(c.eye, center)
        {
            d.culled += 1;
            continue;
        }
        d.visible += 1;
        orb_model(&mut d.opaque, &c, orb, position);
        shadow_radius(&mut d.translucent, &c, position, 0.15, 0.75, false);
        if c.hitboxes { wirebox(&mut d.opaque, orb.bounds.min.as_vec3(), orb.bounds.max.as_vec3()); }
    }
    profile.drops_orbs_ms += build_profile::elapsed(started);
    let started = profile.timer();
    for effect in &c.sim.pickup_fx.0 {
        use crate::gameplay::item_pickup_fx::Visual;
        let pos = effect.position(c.sim, c.player, c.alpha).as_vec3();
        match &effect.visual {
            Visual::Item(item) => {
                let start = d.opaque.vertices.len();
                item_models::dropped_at(&mut d.opaque, item, pos, c.alpha, c.assets, c.resources);
                // EntityPickupFX is 0.2 blocks tall; brightness samples its stationary eye position.
                let sample = effect.source() + glam::DVec3::Y * f64::from(0.2f32 * 0.85);
                let light = c.world.visual_light_uv(sample.floor().as_ivec3(), 0);
                for vertex in &mut d.opaque.vertices[start..] { vertex.light[..2].copy_from_slice(&light[..2]); }
            }
            Visual::Orb(orb) => orb_model(&mut d.opaque, &c, orb, pos),
        }
    }
    projectiles::draw(&mut d, &c);
    for crystal in &c.sim.end_crystals {
        d.total += 1;
        let pos = crystal.position.as_vec3();
        let beam_target = c
            .sim
            .mobs
            .iter()
            .find(|m| {
                m.kind == MobKind::EnderDragon
                    && m.health > 0.0
                    && (c
                        .sim
                        .mob_brains
                        .get(&m.id)
                        .is_some_and(|b| b.dragon_healing_crystal == Some(crystal.id))
                        || crystal.beam_target.is_some_and(|t| {
                            t.distance_squared(m.position + Vec3::Y * 2.0) < 0.25
                        }))
            })
            .map(|m| m.previous.lerp(m.position, c.alpha) + Vec3::Y * 2.0)
            .or(crystal.beam_target);
        let min = pos - Vec3::new(1.0, 0.0, 1.0);
        let max = pos + Vec3::new(1.0, 2.0, 1.0);
        let cull_min = beam_target.map_or(min, |t| min.min(t - Vec3::splat(0.75)));
        let cull_max = beam_target.map_or(max, |t| max.max(t + Vec3::splat(0.75)));
        let dist_limit = if beam_target.is_some() {
            (c.settings.entity_distance.max(256) as f32).powi(2)
        } else {
            (c.settings.entity_distance as f32).powi(2)
        };
        if pos.distance_squared(c.eye) > dist_limit || !c.frustum.visible(cull_min, cull_max) {
            d.culled += 1;
            continue;
        }
        d.visible += 1;
        draw_end_crystal(&mut d, &c, crystal.id, pos, crystal.bottom, beam_target);
        if c.hitboxes {
            wirebox(&mut d.opaque, min, max);
        }
    }
    for shot in &c.sim.mob_shots {
        use crate::gameplay::mob_shots::Payload;
        let pos=shot.previous.lerp(shot.position,c.alpha);
        let (path,side)=match shot.payload {
            Payload::Fireball=>("textures/items/fireball.png",2.),
            Payload::SmallFireball=>("textures/items/fireball.png",0.5),
            Payload::Snowball=>("textures/items/snowball.png",0.5),
            Payload::Splash(_)=>("textures/items/potion_bottle_splash.png",0.5),
            Payload::Skull=>("textures/entity/wither/wither.png",0.5),
        };
        if !c.frustum.visible(pos-Vec3::splat(side),pos+Vec3::splat(side)) {continue;}
        let sprite=c.resources.sprite(path);
        let start=d.opaque.vertices.len();
        if shot.payload==Payload::Skull {
            cuboid(&mut d.opaque,Mat4::from_translation(pos-Vec3::splat(side/2.)),Vec3::ZERO,Vec3::splat(side),
                [0.,0.],sprite.layer|ATLAS,[255;4]);
            for v in &mut d.opaque.vertices[start..] { v.light=[240,240,0,0]; }
        } else {
            let pitch=if c.perspective==2{-c.player.pitch}else{c.player.pitch};
            let rot=Mat4::from_rotation_y(-c.player.yaw+if c.perspective==2{std::f32::consts::PI}else{0.})*Mat4::from_rotation_x(pitch);
            let is_fireball=matches!(shot.payload,Payload::Fireball|Payload::SmallFireball);
            let (y0,y1)=if is_fireball{(-0.25,0.75)}else{(-0.5,0.5)};
            let q=[Vec3::new(-0.5,y0,0.),Vec3::new(0.5,y0,0.),Vec3::new(0.5,y1,0.),Vec3::new(-0.5,y1,0.)].map(|v|pos+rot.transform_vector3(v*side));
            face(&mut d.opaque,q,[0.,0.,sprite.width as f32/256.,sprite.height as f32/256.],sprite.layer|ATLAS,[255;4]);
            if is_fireball {
                let hit=if shot.payload==Payload::Fireball{1.}else{0.3125};
                c.fire(&mut d.opaque,start,pos,hit,hit);
            } else {
                light_since(&mut d.opaque,start,c.world,pos);
            }
        }
    }
    for arrow in &c.sim.arrows {
        let pos = arrow.previous.lerp(arrow.position, c.alpha);
        let start = d.opaque.vertices.len();
        cuboid(
            &mut d.opaque,
            Mat4::from_translation(pos),
            Vec3::new(-0.015, -0.015, -0.25),
            Vec3::new(0.03, 0.03, 0.5),
            [0.0; 2],
            SOLID,
            [150, 105, 60, 255],
        );
        light_since(&mut d.opaque, start, c.world, pos);
    }
    for falling in &c.sim.block_ticks.falling {
        let feet_pos = falling.previous.lerp(falling.position, c.alpha as f64).as_vec3();
        let center = feet_pos + Vec3::Y * 0.5;
        if !c
            .frustum
            .visible(center - Vec3::splat(0.5), center + Vec3::splat(0.5))
        {
            continue;
        }
        draw_falling_block(&mut d.opaque, c.world, c.assets, feet_pos, falling.state);
        shadow_radius(&mut d.translucent, &c, feet_pos, 0.5, 1.0, false);
    }
    profile.other_entities_ms += build_profile::elapsed(started);
    let started = profile.timer();
    server::draw(&mut d,&c);
    c.sim.build_local_vehicles(&mut d, &c);
    profile.server_ms += build_profile::elapsed(started);
    let started = profile.timer();
    if (c.perspective != 0 || c.player.sleeping) && c.sim.server_scene.camera.is_none() {
        let start = d.opaque.vertices.len();
        let pos = c.player.previous.lerp(c.player.position, c.alpha);
        third_person_model::draw(&mut d.opaque,&c,pos);
        light_since(&mut d.opaque, start, c.world, pos + Vec3::Y * 1.62);
        if c.sim.player_burning() && !c.player.is_spectator() && c.sim.server_scene.camera.is_none() {
            c.fire(&mut d.opaque, start, pos, 0.6, 1.8);
        }
        if c.perspective != 0
            && !c.player.sleeping
            && !c.player.is_spectator()
            && c.sim.effects.get(14).is_none()
        {
            shadow_radius(&mut d.translucent, &c, pos, 0.5, 1.0, false);
        }
    } else if c.render_hand && !c.player.is_spectator() {
        let start = d.opaque.vertices.len();
        if let Some(map)=c.sim.server_scene.map.as_ref().filter(|m|c.sim.inventory.held().is_some_and(|i|i.id==358&&i32::from(i.meta)==m.id)){
            server::held_map(&mut d.opaque,&c,map);
        }else if let Some(item) = c.sim.equipped.stack.as_ref() {
            item_models::first_item(&mut d.opaque,&c,item);
        } else {
            let slim = c.player.third_person.slim;
            let skin = c.sim.presentation.local_skin_region(c.resources, slim);
            crate::first_person::empty_hand_skin_shaded(
                &mut d.opaque,
                skin,
                slim,
                c.sim.hand_swing.sample(c.alpha),
                c.sim.equipped.lowered(c.alpha),
                c.settings.menu.skin_parts & (1 << 3) != 0,
                c.player.yaw,
                c.player.pitch,
            );
        }
        light_since(&mut d.opaque, start, c.world, c.eye);
    }
    profile.player_ms += build_profile::elapsed(started);
    let started = profile.timer();
    let yaw = -c.player.yaw + if c.perspective == 2 { std::f32::consts::PI } else { 0. };
    let rotation = Mat4::from_rotation_y(yaw) * Mat4::from_rotation_x(c.player.pitch);
    let right = rotation.transform_vector3(Vec3::X);
    let up = rotation.transform_vector3(Vec3::Y);
    for particle in &c.sim.particles {
        if let Some(base_phase) = particle.is_mob_appearance() {
            let phase = (base_phase + c.alpha / 30.0).clamp(0.0, 1.0);
            draw_elder_guardian_curse(&mut d.translucent, &c, phase);
            continue;
        }
        let (p, size, mut color, block, mut uv) = particle.appearance(c.alpha);
        if color[3] == 0 {
            continue;
        }
        let surface = particle.surface_texture();
        let layer = if let Some((path, _)) = surface {
            let sprite = c.resources.sprite(path);
            for (v, d) in
                uv.iter_mut()
                    .zip([sprite.width, sprite.height, sprite.width, sprite.height])
            {
                *v *= d as f32 / 256.;
            }
            ATLAS | sprite.layer
        } else if let Some((item, meta)) = particle.item_texture() {
            if item < 256 && item != 166 {
                c.assets.particle_texture(item << 4 | (meta & 15))
            } else {
                let sprite = if item == 166 {
                    c.resources.sprite("textures/items/barrier.png")
                } else {
                    c.resources.item_sprite(item, meta)
                };
                for (value, dimension) in
                    uv.iter_mut()
                        .zip([sprite.width, sprite.height, sprite.width, sprite.height])
                {
                    *value *= dimension as f32 / 256.;
                }
                ATLAS | sprite.layer
            }
        } else if let Some((state, origin)) = particle.digging_block() {
            let rgb = if state >> 4 == 2 {
                0xFFFFFF
            } else if state >> 4 == 140 {
                let tile = c.world.tile(origin);
                let pot_tinted = crate::vanilla::middle_pick::block(state, 0, tile)
                    .is_some_and(|i| i.id == 31 && i.damage == 2)
                    || (tile.is_none() && (state & 15) == 11)
                    || c.assets
                        .block_models
                        .flower_pot(state, tile)
                        .is_some_and(|faces| faces.iter().any(|f| f.tinted));
                if pot_tinted {
                    c.assets.biome_colors.block(c.world, origin, state)
                } else {
                    0xFFFFFF
                }
            } else {
                c.assets.biome_colors.block(c.world, origin, state)
            };
            if rgb != 0xFFFFFF {
                color[0] = (u32::from(color[0]) * ((rgb >> 16) & 255) / 255) as u8;
                color[1] = (u32::from(color[1]) * ((rgb >> 8) & 255) / 255) as u8;
                color[2] = (u32::from(color[2]) * (rgb & 255) / 255) as u8;
            }
            if state >> 4 == 166 {
                let sprite = c.resources.sprite("textures/items/barrier.png");
                for (value, dimension) in
                    uv.iter_mut()
                        .zip([sprite.width, sprite.height, sprite.width, sprite.height])
                {
                    *value *= dimension as f32 / 256.;
                }
                ATLAS | sprite.layer
            } else {
                c.assets.particle_texture(state)
            }
        } else {
            block.map_or_else(
                || ATLAS | c.resources.sprite("textures/particle/particles.png").layer,
                |state| c.assets.particle_texture(state),
            )
        };
        if surface.is_some_and(|(_, flat)| flat) {
            let (sky_loss, brightness) = c.shadow_env();
            let wl = c.world.visual_light_uv(p.floor().as_ivec3(), 0);
            let lvl = (wl[0] / 16)
                .max((wl[1] / 16).saturating_sub(sky_loss))
                .min(15) as usize;
            let br = brightness[lvl];
            for ch in &mut color[..3] {
                *ch = (f32::from(*ch) * br).round().clamp(0.0, 255.0) as u8;
            }
            let (horizontal, vertical) = (Vec3::X, -Vec3::Z);
            let center = p + Vec3::Y * 0.002;
            let corners = [
                -horizontal - vertical,
                horizontal - vertical,
                horizontal + vertical,
                -horizontal + vertical,
            ]
            .map(|corner| center + corner * size);
            let start = d.translucent.vertices.len();
            face(&mut d.translucent, corners, uv, layer | BLEND | UNLIT, color);
            for vertex in &mut d.translucent.vertices[start..] {
                vertex.light = [240, 240, 0, 0];
            }
        } else {
            let world_light = c.world.visual_light_uv(p.floor().as_ivec3(), 0);
            let light = particle.light_override(world_light);
            let corners = [
                -right - up,
                right - up,
                right + up,
                -right + up,
            ]
            .map(|corner| p + corner * size);
            let (target_mesh, draw_layer) = if color[3] < 255 {
                (&mut d.translucent, layer | BLEND)
            } else {
                (&mut d.opaque, layer)
            };
            let start = target_mesh.vertices.len();
            face(target_mesh, corners, uv, draw_layer, color);
            for vertex in &mut target_mesh.vertices[start..] {
                vertex.light = light;
            }
        }
    }
    profile.particles_ms += build_profile::elapsed(started);
    let started = profile.timer();
    precipitation::draw(&mut d.translucent, &c);
    profile.weather_ms += build_profile::elapsed(started);
    d
}

pub(crate) fn draw_falling_block(
    mesh: &mut Mesh,
    world: &World,
    assets: &Assets,
    feet_pos: Vec3,
    state: u16,
) {
    let sample_pos = feet_pos + Vec3::Y * 0.5;
    let start = mesh.vertices.len();
    if let Some(faces) = assets.block_models.carried(state) {
        let offset = feet_pos - Vec3::new(0.5, 0.0, 0.5);
        let block_pos = sample_pos.floor().as_ivec3();
        let tint_rgb = assets.biome_colors.block(world, block_pos, state);
        for f in faces {
            let color = if f.tinted && tint_rgb != 0xFFFFFF {
                let s = f32::from(f.shade) / 255.0;
                [
                    (((tint_rgb >> 16) & 255) as f32 * s).round().clamp(0.0, 255.0) as u8,
                    (((tint_rgb >> 8) & 255) as f32 * s).round().clamp(0.0, 255.0) as u8,
                    ((tint_rgb & 255) as f32 * s).round().clamp(0.0, 255.0) as u8,
                    255,
                ]
            } else {
                [f.shade, f.shade, f.shade, 255]
            };
            let first = mesh.vertices.len() as u32;
            for v in f.vertices {
                mesh.vertices.push(Vertex {
                    position: (offset + v.position).to_array(),
                    uv: v.uv,
                    layer: f.layer | NO_MIPMAP,
                    color,
                    light: [0, 240, 0, 0],
                });
            }
            mesh.indices.extend([0, 1, 2, 0, 2, 3].map(|k| first + k));
            mesh.visible_faces += 1;
        }
    } else {
        block_cube(
            mesh,
            Mat4::from_translation(sample_pos),
            state,
            1.0,
            assets,
        );
    }
    light_since(mesh, start, world, sample_pos);
}

pub(crate) fn draw_end_crystal(
    d: &mut Dynamic,
    c: &ModelContext<'_>,
    id: u64,
    pos: Vec3,
    bottom: bool,
    beam_target: Option<Vec3>,
) {
    let sprite = c
        .resources
        .sprite("textures/entity/endercrystal/endercrystal.png");
    let light = c.world.visual_light_uv((pos + Vec3::Y).floor().as_ivec3(), 0);
    let ticks = (c.sim.total_ticks % 240_000) as f32 + c.alpha;
    let phase_offset = (id.wrapping_mul(2654435761) % 100_000) as f32;
    let f = ticks + phase_offset;
    let spin_rad = (f * 3.0).to_radians();
    let s = crate::vanilla::math::sin(f * 0.2) * 0.5 + 0.5;
    let bounce = (s * s + s) * 0.2;

    if bottom {
        let base_mat = Mat4::from_translation(pos)
            * Mat4::from_scale(Vec3::splat(2.0))
            * Mat4::from_translation(Vec3::new(0.0, -0.5, 0.0));
        biped_box(
            &mut d.opaque,
            base_mat,
            [-6.0, 0.0, -6.0],
            [12.0, 4.0, 12.0],
            0.0,
            [0.0, 16.0],
            false,
            sprite,
            [64.0, 32.0],
            [255; 3],
            NO_MIPMAP,
            light,
        );
    }

    let tilt_axis = Vec3::new(
        std::f32::consts::FRAC_1_SQRT_2,
        0.0,
        std::f32::consts::FRAC_1_SQRT_2,
    );
    let tilt = Mat4::from_axis_angle(tilt_axis, 60.0_f32.to_radians());
    let mut m = Mat4::from_translation(pos)
        * Mat4::from_scale(Vec3::splat(2.0))
        * Mat4::from_translation(Vec3::new(0.0, -0.5, 0.0))
        * Mat4::from_rotation_y(spin_rad)
        * Mat4::from_translation(Vec3::new(0.0, 0.8 + bounce, 0.0))
        * tilt;
    biped_box(
        &mut d.opaque,
        m,
        [-4.0, -4.0, -4.0],
        [8.0, 8.0, 8.0],
        0.0,
        [0.0, 0.0],
        false,
        sprite,
        [64.0, 32.0],
        [255; 3],
        NO_MIPMAP,
        light,
    );
    m = m * Mat4::from_scale(Vec3::splat(0.875)) * tilt * Mat4::from_rotation_y(spin_rad);
    biped_box(
        &mut d.opaque,
        m,
        [-4.0, -4.0, -4.0],
        [8.0, 8.0, 8.0],
        0.0,
        [0.0, 0.0],
        false,
        sprite,
        [64.0, 32.0],
        [255; 3],
        NO_MIPMAP,
        light,
    );
    m = m * Mat4::from_scale(Vec3::splat(0.875)) * tilt * Mat4::from_rotation_y(spin_rad);
    biped_box(
        &mut d.opaque,
        m,
        [-4.0, -4.0, -4.0],
        [8.0, 8.0, 8.0],
        0.0,
        [32.0, 0.0],
        false,
        sprite,
        [64.0, 32.0],
        [255; 3],
        NO_MIPMAP,
        light,
    );

    shadow_radius(&mut d.translucent, c, pos, 0.5, 1.0, false);

    if let Some(target) = beam_target {
        let crystal_center = pos + Vec3::Y * (0.6 + 2.0 * bounce);
        draw_end_crystal_beam(&mut d.opaque, c.resources, crystal_center, target, ticks);
    }
}

fn draw_end_crystal_beam(
    mesh: &mut Mesh,
    resources: &Resources,
    from_crystal: Vec3,
    to_target: Vec3,
    ticks: f32,
) {
    let diff = to_target - from_crystal;
    let len = diff.length();
    if len <= 0.05 {
        return;
    }
    let dir = diff / len;
    let arbitrary = if dir.y.abs() < 0.99 { Vec3::Y } else { Vec3::X };
    let axis_u = dir.cross(arbitrary).normalize_or_zero();
    let axis_v = dir.cross(axis_u).normalize_or_zero();

    let sprite = resources.sprite("textures/entity/endercrystal/endercrystal_beam.png");
    let u_max = sprite.width as f32 / 256.0;
    let v_max = sprite.height as f32 / 256.0;
    let layer = sprite.layer | ATLAS | UNLIT | NO_MIPMAP;

    let cycles = len / 32.0;
    let base_v = (-ticks * 0.01).rem_euclid(1.0);
    let end_v = base_v + cycles;

    let ring = std::array::from_fn::<Vec3, 9, _>(|i| {
        let a = (i % 8) as f32 * std::f32::consts::TAU / 8.0;
        let (sa, ca) = a.sin_cos();
        axis_u * sa + axis_v * ca
    });

    let mut emit_segment = |t0: f32, t1: f32, v0_unit: f32, v1_unit: f32| {
        if t1 <= t0 + 1e-5 {
            return;
        }
        let p0 = from_crystal + diff * t0;
        let p1 = from_crystal + diff * t1;
        let r0 = 0.75 * (1.0 - 0.8 * t0);
        let r1 = 0.75 * (1.0 - 0.8 * t1);
        let g0 = (255.0 * (1.0 - t0)).round().clamp(0.0, 255.0) as u8;
        let g1 = (255.0 * (1.0 - t1)).round().clamp(0.0, 255.0) as u8;
        let c0 = [g0, g0, g0, 255];
        let c1 = [g1, g1, g1, 255];
        let v0 = v0_unit * v_max;
        let v1 = v1_unit * v_max;

        for side in 0..8 {
            let u0 = (side as f32 / 8.0) * u_max;
            let u1 = ((side + 1) as f32 / 8.0) * u_max;
            let a = p0 + ring[side] * r0;
            let b = p0 + ring[side + 1] * r0;
            let c = p1 + ring[side + 1] * r1;
            let d = p1 + ring[side] * r1;
            for (verts, uvs, cols) in [
                ([a, b, c, d], [[u0, v0], [u1, v0], [u1, v1], [u0, v1]], [c0, c0, c1, c1]),
                ([b, a, d, c], [[u1, v0], [u0, v0], [u0, v1], [u1, v1]], [c0, c0, c1, c1]),
            ] {
                let first = mesh.vertices.len() as u32;
                for i in 0..4 {
                    mesh.vertices.push(Vertex {
                        position: verts[i].to_array(),
                        uv: uvs[i],
                        layer,
                        color: cols[i],
                        light: [240, 240, 0, 0],
                    });
                }
                mesh.indices.extend([0, 1, 2, 0, 2, 3].map(|k| first + k));
                mesh.visible_faces += 1;
            }
        }
    };

    let mut t0 = 0.0_f32;
    let mut v0 = base_v;
    let mut next_int = 1_u32;
    while (next_int as f32) < end_v && next_int <= 64 {
        let t1 = ((next_int as f32 - base_v) / cycles).clamp(0.0, 1.0);
        emit_segment(t0, t1, v0, 1.0);
        t0 = t1;
        v0 = 0.0;
        next_int += 1;
    }
    let final_v = (end_v - end_v.floor()).clamp(0.0, 1.0);
    emit_segment(t0, 1.0, v0, if final_v <= v0 && t0 < 1.0 { 1.0 } else { final_v });
}

fn draw_elder_guardian_curse(mesh: &mut Mesh, c: &ModelContext<'_>, phase: f32) {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};
    let alpha = ((0.05 + 0.5 * (phase * PI).sin()).clamp(0.0, 1.0) * 255.0) as u8;
    if alpha == 0 {
        return;
    }
    let sprite = c.resources.sprite("textures/entity/guardian_elder.png");
    let [yaw_deg, pitch_deg] = c.player.java_rotation();
    let curse_scale = 0.42553192 * 2.35;
    let base_pos = c.player.previous.lerp(c.player.position, c.alpha) + Vec3::Y * 1.8;
    let body_mat = Mat4::from_translation(base_pos)
        * Mat4::from_rotation_y((180.0 - yaw_deg).to_radians())
        * Mat4::from_rotation_x((60.0 - 150.0 * phase - pitch_deg).to_radians())
        * Mat4::from_translation(Vec3::new(0.0, -0.4, -1.5))
        * Mat4::from_rotation_y(PI)
        * Mat4::from_scale(Vec3::new(-curse_scale, -curse_scale, curse_scale))
        * Mat4::from_translation(Vec3::new(0.0, -1.5078125, 0.0));

    let flags = BLEND | UNLIT;
    let light = [240, 240, 0, 0];
    let tex = [64.0, 64.0];
    let start = mesh.vertices.len();

    // Guardian central body + 4 side/top/bottom plates (`ModelGuardian.java`)
    for (min, size, uv, mirror) in [
        ([-6.0, 10.0, -8.0], [12.0, 12.0, 16.0], [0.0, 0.0], false),
        ([-8.0, 10.0, -6.0], [2.0, 12.0, 12.0], [0.0, 28.0], false),
        ([6.0, 10.0, -6.0], [2.0, 12.0, 12.0], [0.0, 28.0], true),
        ([-6.0, 8.0, -6.0], [12.0, 2.0, 12.0], [16.0, 40.0], false),
        ([-6.0, 22.0, -6.0], [12.0, 2.0, 12.0], [16.0, 40.0], true),
    ] {
        biped_box(mesh, body_mat, min, size, 0.0, uv, mirror, sprite, tex, [255; 3], flags, light);
    }

    // Guardian eye (`guardianEye`)
    let eye_mat = body_mat * Mat4::from_translation(Vec3::new(0.0, 0.0, -8.25 / 16.0));
    biped_box(
        mesh,
        eye_mat,
        [-1.0, 15.0, 0.0],
        [2.0, 2.0, 1.0],
        0.0,
        [8.0, 0.0],
        false,
        sprite,
        tex,
        [255; 3],
        flags,
        light,
    );

    // 12 radiating spines (`guardianSpines[0..12]`)
    let spine_pivots: [[f32; 3]; 12] = [
        [0.0, 8.0, -7.0],
        [-7.0, 8.0, 0.0],
        [0.0, 8.0, 7.0],
        [7.0, 8.0, 0.0],
        [0.0, 24.0, -7.0],
        [-7.0, 24.0, 0.0],
        [0.0, 24.0, 7.0],
        [7.0, 24.0, 0.0],
        [7.0, 16.0, 7.0],
        [7.0, 16.0, -7.0],
        [-7.0, 16.0, -7.0],
        [-7.0, 16.0, 7.0],
    ];
    let spine_rot_y: [f32; 12] = [
        0.0,
        0.5 * PI,
        PI,
        1.5 * PI,
        0.0,
        0.5 * PI,
        PI,
        1.5 * PI,
        0.25 * PI,
        0.75 * PI,
        1.25 * PI,
        1.75 * PI,
    ];
    let spine_rot_x: [f32; 12] = [
        FRAC_PI_4,
        FRAC_PI_4,
        FRAC_PI_4,
        FRAC_PI_4,
        -FRAC_PI_4,
        -FRAC_PI_4,
        -FRAC_PI_4,
        -FRAC_PI_4,
        FRAC_PI_2,
        FRAC_PI_2,
        FRAC_PI_2,
        FRAC_PI_2,
    ];
    for i in 0..12 {
        let spine_mat = body_mat
            * Mat4::from_translation(Vec3::from_array(spine_pivots[i]) / 16.0)
            * Mat4::from_rotation_y(spine_rot_y[i])
            * Mat4::from_rotation_x(spine_rot_x[i]);
        biped_box(
            mesh,
            spine_mat,
            [-1.0, -4.5, -1.0],
            [2.0, 9.0, 2.0],
            0.0,
            [0.0, 0.0],
            false,
            sprite,
            tex,
            [255; 3],
            flags,
            light,
        );
    }

    // 3 tail segments + tail fin (`guardianTail[0..3]`)
    for (min, size, uv) in [
        ([-2.0, 14.0, 7.0], [4.0, 4.0, 8.0], [40.0, 0.0]),
        ([-1.5, 14.5, 15.0], [3.0, 3.0, 7.0], [0.0, 54.0]),
        ([-1.0, 15.0, 22.0], [2.0, 2.0, 6.0], [41.0, 32.0]),
        ([-0.5, 10.5, 25.0], [1.0, 9.0, 9.0], [25.0, 19.0]),
    ] {
        biped_box(mesh, body_mat, min, size, 0.0, uv, false, sprite, tex, [255; 3], flags, light);
    }

    for v in &mut mesh.vertices[start..] {
        v.color = [255, 255, 255, alpha];
    }
}
#[derive(Clone, Copy)]
struct ShadowEnvCache {
    sim_ptr: usize,
    clock: u64,
    dimension: i32,
    rain_bits: u32,
    thunder_bits: u32,
    sky_loss: u8,
    brightness: [f32; 16],
}

impl ModelContext<'_> {
    fn shadow_env(&self) -> (u8, [f32; 16]) {
        use std::cell::Cell;
        thread_local! {
            static CACHE: Cell<Option<ShadowEnvCache>> = const { Cell::new(None) };
        }
        let sim_ptr = std::ptr::from_ref(self.sim) as usize;
        let (rain, thunder) = self.sim.weather.strengths(1.);
        let (rain_bits, thunder_bits) = (rain.to_bits(), thunder.to_bits());
        if let Some(hit) = CACHE.with(Cell::get).filter(|hit| {
            hit.sim_ptr == sim_ptr
                && hit.clock == self.sim.clock
                && hit.dimension == self.sim.dimension
                && hit.rain_bits == rain_bits
                && hit.thunder_bits == thunder_bits
        }) {
            return (hit.sky_loss, hit.brightness);
        }
        let sky_loss = crate::vanilla::atmosphere::skylight_subtracted(
            self.sim.clock as i64,
            rain,
            rain * thunder,
        );
        let brightness = crate::vanilla::lightmap::brightness_table(self.sim.dimension);
        CACHE.with(|cell| {
            cell.set(Some(ShadowEnvCache {
                sim_ptr,
                clock: self.sim.clock,
                dimension: self.sim.dimension,
                rain_bits,
                thunder_bits,
                sky_loss,
                brightness,
            }));
        });
        (sky_loss, brightness)
    }
}

pub(crate) fn shadow_radius(
    mesh: &mut Mesh,
    c: &ModelContext<'_>,
    pos: Vec3,
    radius: f32,
    opaque: f32,
    minecart: bool,
) {
    if !c.settings.entity_shadows || radius <= 0.0 {
        return;
    }
    let dist_sq = pos.distance_squared(c.eye);
    let shadow_alpha = (1.0 - dist_sq / 256.0) * opaque;
    if shadow_alpha <= 0.0 {
        return;
    }
    let (sky_loss, brightness_table) = c.shadow_env();
    let has_sky = c.sim.dimension == 0;
    let y_ref = pos.y + if minecart { 0.0625 } else { 0.0 };
    let sprite = c.resources.sprite("textures/misc/shadow.png");
    let u_scale = sprite.width as f32 / 256.0;
    let v_scale = sprite.height as f32 / 256.0;
    let layer = ATLAS | BLEND | UNLIT | sprite.layer;
    let sample_light = |p: glam::IVec3| -> u8 {
        let base = c.world.game_light(p, sky_loss, true, has_sky);
        let uv = c.world.visual_light_uv(p, 0);
        let block_nibble = uv[0] >> 4;
        let sky_nibble = if has_sky {
            (uv[1] >> 4).saturating_sub(sky_loss)
        } else {
            0
        };
        base.max(block_nibble).max(sky_nibble).min(15)
    };
    let bx0 = (pos.x - radius).floor() as i32;
    let bx1 = (pos.x + radius).floor() as i32;
    let bz0 = (pos.z - radius).floor() as i32;
    let bz1 = (pos.z + radius).floor() as i32;
    let by0 = (y_ref - radius - 0.5).floor() as i32;
    let by1 = y_ref.floor() as i32;
    for bx in bx0..=bx1 {
        let x0 = (bx as f32).max(pos.x - radius);
        let x1 = (bx as f32 + 1.0).min(pos.x + radius);
        if x0 >= x1 {
            continue;
        }
        let u0 = ((pos.x - x0) / (2.0 * radius) + 0.5) * u_scale;
        let u1 = ((pos.x - x1) / (2.0 * radius) + 0.5) * u_scale;
        for bz in bz0..=bz1 {
            let z0 = (bz as f32).max(pos.z - radius);
            let z1 = (bz as f32 + 1.0).min(pos.z + radius);
            if z0 >= z1 {
                continue;
            }
            let v0 = ((pos.z - z0) / (2.0 * radius) + 0.5) * v_scale;
            let v1 = ((pos.z - z1) / (2.0 * radius) + 0.5) * v_scale;
            let mut emitted_heights = [i32::MIN; 8];
            let mut emitted_len = 0usize;
            for by in by0..=by1 {
                let cur_pos = glam::IVec3::new(bx, by, bz);
                let cur = c.world.get(cur_pos);
                let cur_bottom_slab = matches!(cur >> 4, 44 | 126 | 182) && (cur & 8) == 0;
                let below = c.world.get(glam::IVec3::new(bx, by - 1, bz));
                let mut candidates = [(0.0f32, 0u8, false); 2];
                if !crate::vanilla::material::full_cube(cur)
                    && !cur_bottom_slab
                    && below >> 4 != 0
                    && below >> 4 != 166
                    && (crate::vanilla::material::full_cube(below)
                        || (matches!(below >> 4, 44 | 126 | 182) && (below & 8) != 0)
                        || below >> 4 == 60)
                {
                    let top_y = if below >> 4 == 60 {
                        by as f32 - 0.0625
                    } else {
                        by as f32
                    };
                    candidates[0] = (top_y, sample_light(cur_pos), true);
                }
                if cur_bottom_slab {
                    let above_pos = glam::IVec3::new(bx, by + 1, bz);
                    if !crate::vanilla::material::full_cube(c.world.get(above_pos)) {
                        let light = sample_light(cur_pos).max(sample_light(above_pos));
                        candidates[1] = (by as f32 + 0.5, light, true);
                    }
                }
                for (top_y, light, valid) in candidates {
                    if !valid || y_ref < top_y - 0.05 || y_ref - top_y > radius + 0.5 || light <= 3
                    {
                        continue;
                    }
                    let key = (top_y * 16.0).round() as i32;
                    if emitted_heights[..emitted_len].contains(&key) {
                        continue;
                    }
                    if let Some(slot) = emitted_heights.get_mut(emitted_len) {
                        *slot = key;
                        emitted_len += 1;
                    }
                    let alpha = ((shadow_alpha - (y_ref - top_y) * 0.5)
                        * 0.5
                        * brightness_table[light as usize])
                        .clamp(0.0, 1.0);
                    if alpha <= 0.0 {
                        continue;
                    }
                    let y_plane = top_y + 0.015625;
                    let color = [255, 255, 255, (alpha * 255.0).round() as u8];
                    let start = mesh.vertices.len() as u32;
                    for (position, uv) in [
                        (Vec3::new(x0, y_plane, z1), [u0, v1]),
                        (Vec3::new(x1, y_plane, z1), [u1, v1]),
                        (Vec3::new(x1, y_plane, z0), [u1, v0]),
                        (Vec3::new(x0, y_plane, z0), [u0, v0]),
                    ] {
                        mesh.vertices.push(Vertex {
                            position: position.to_array(),
                            uv,
                            layer,
                            color,
                            light: [0, 240, 0, 0],
                        });
                    }
                    mesh.indices.extend([0, 1, 2, 0, 2, 3].map(|i| start + i));
                    mesh.visible_faces += 1;
                }
            }
        }
    }
}
pub(crate) fn wirebox(mesh: &mut Mesh, min: Vec3, max: Vec3) {
    for axis in 0..3 {
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        for a in [false, true] {
            for b in [false, true] {
                let mut start = min;
                start[u] = if a { max[u] } else { min[u] };
                start[v] = if b { max[v] } else { min[v] };
                let mut size = Vec3::splat(0.012);
                size[axis] = max[axis] - min[axis];
                cuboid(
                    mesh,
                    Mat4::IDENTITY,
                    start,
                    size,
                    [0.0; 2],
                    SOLID | UNLIT,
                    [255; 4],
                );
            }
        }
    }
}
pub fn clouds(eye: Vec3, mode: u8, r: &Resources, far: f32) -> Mesh {
    let mut mesh = empty();
    if mode == 0 {
        return mesh;
    }
    let sprite = r.sprite("textures/environment/clouds.png");
    let center = (eye / 12.0).floor().as_ivec3();
    // RenderGlobal's cloud grid has a fixed extent, independent of distant
    // terrain. Preserve the existing <=24-chunk cloud appearance while keeping
    // the visual LOD extension from multiplying cloud geometry on this thread.
    let radius = (far.min(24.*16.) / 12.0).ceil() as i32 + 2;
    let occupied = |x: i32, z: i32| {
        let ix = x.rem_euclid(sprite.width as i32) as usize;
        let iz = z.rem_euclid(sprite.height as i32) as usize;
        r.pixels[(sprite.layer as usize * 256 * 256 + iz * 256 + ix) * 4 + 3] > 127
    };
    for z in center.z - radius..=center.z + radius {
        for x in center.x - radius..=center.x + radius {
            if !occupied(x, z) {
                continue;
            }
            let min = Vec3::new(x as f32 * 12.0, 128.0, z as f32 * 12.0);
            if mode == 1 {
                let points = [
                    min,
                    min + Vec3::X * 12.0,
                    min + Vec3::new(12.0, 0.0, 12.0),
                    min + Vec3::Z * 12.0,
                ];
                face(
                    &mut mesh,
                    points,
                    [0.0; 4],
                    SOLID | CLOUD,
                    [230, 230, 230, 255],
                );
                face(
                    &mut mesh,
                    [points[1], points[0], points[3], points[2]],
                    [0.0; 4],
                    SOLID | CLOUD,
                    [255; 4],
                );
            } else {
                cuboid(
                    &mut mesh,
                    Mat4::IDENTITY,
                    min,
                    Vec3::new(12.0, 4.0, 12.0),
                    [0.0; 2],
                    SOLID | CLOUD,
                    [255; 4],
                );
            }
        }
    }
    mesh
}

#[cfg(test)]
mod cloud_distance_tests {
    #[test]
    fn distant_terrain_radius_does_not_expand_existing_fast_or_fancy_cloud_geometry() {
        let resources=crate::validation::resources();
        for mode in [0,1,2] {
            let near=super::clouds(glam::Vec3::new(-27.,91.,43.),mode,resources,24.*16.);
            let distant=super::clouds(glam::Vec3::new(-27.,91.,43.),mode,resources,128.*16.);
            assert_eq!(near.indices,distant.indices);
            assert_eq!(bytemuck::cast_slice::<_,u8>(&near.vertices),
                bytemuck::cast_slice::<_,u8>(&distant.vertices));
            if mode==0 {assert!(near.vertices.is_empty());}
            else {assert!(!near.vertices.is_empty());}
        }
    }
}

#[cfg(test)]
mod occlusion_culling_tests {
    use super::occluded_bounds;
    use crate::{
        remote_mobs::{visual_cull_bounds, Appearance},
        world::{Chunk, ChunkMap, World},
    };
    use glam::{IVec3, Vec3};
    use std::sync::Arc;

    fn test_world() -> World {
        World {
            chunks: ChunkMap::from([((0, 0), Arc::new(Chunk::empty()))]),
            spawn: Vec3::ZERO,
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        }
    }

    fn rabbit_boxes(feet: Vec3) -> ((Vec3, Vec3), (Vec3, Vec3)) {
        let lo = feet - Vec3::new(0.3, 0.0, 0.3);
        let hi = feet + Vec3::new(0.3, 0.7, 0.3);
        let cull = visual_cull_bounds(101, Appearance::default(), lo, hi);
        ((lo, hi), cull)
    }

    #[test]
    fn rabbit_is_never_culled_by_snow_layers_farmland_cactus_slabs_or_end_portal_frames() {
        let eye = Vec3::new(8.5, 65.62, 5.5);

        // 1. Grounded Rabbit standing inside 1-layer snow (78:0) across surrounding cells
        let mut world = test_world();
        for z in 6..=10 {
            for x in 7..=9 {
                world.set(IVec3::new(x, 63, z), 1 << 4);
                world.set(IVec3::new(x, 64, z), 78 << 4);
            }
        }
        let ((lo, hi), (cull_lo, cull_hi)) = rabbit_boxes(Vec3::new(8.5, 64.0, 8.5));
        assert!(!occluded_bounds(&world, eye, lo, hi));
        assert!(!occluded_bounds(&world, eye, cull_lo, cull_hi));

        // 2. Rabbit on farmland (60) and behind a farmland step
        let mut world = test_world();
        world.set(IVec3::new(8, 63, 8), 60 << 4);
        for x in 7..=9 {
            world.set(IVec3::new(x, 64, 7), 60 << 4);
            world.set(IVec3::new(x, 65, 7), 60 << 4);
        }
        let ((lo, hi), (cull_lo, cull_hi)) = rabbit_boxes(Vec3::new(8.5, 63.9375, 8.5));
        assert!(!occluded_bounds(&world, eye, lo, hi));
        assert!(!occluded_bounds(&world, eye, cull_lo, cull_hi));

        // 3. Rabbit next to / behind cactus (81)
        let mut world = test_world();
        for y in 64..=66 {
            for x in 7..=9 {
                world.set(IVec3::new(x, y, 7), 81 << 4);
            }
        }
        world.set(IVec3::new(7, 64, 8), 81 << 4);
        let ((lo, hi), (cull_lo, cull_hi)) = rabbit_boxes(Vec3::new(8.5, 64.0, 8.5));
        assert!(!occluded_bounds(&world, eye, lo, hi));
        assert!(!occluded_bounds(&world, eye, cull_lo, cull_hi));

        // 4. Rabbit on a bottom slab (44:0) and behind bottom slabs
        let mut world = test_world();
        world.set(IVec3::new(8, 64, 8), 44 << 4);
        for y in 64..=66 {
            for x in 7..=9 {
                world.set(IVec3::new(x, y, 7), 44 << 4);
            }
        }
        let ((lo, hi), (cull_lo, cull_hi)) = rabbit_boxes(Vec3::new(8.5, 64.5, 8.5));
        assert!(!occluded_bounds(&world, eye, lo, hi));
        assert!(!occluded_bounds(&world, eye, cull_lo, cull_hi));

        // 5. Rabbit behind End Portal Frames (120:0 and 120:4 with Eye of Ender)
        for state in [120 << 4, (120 << 4) | 4] {
            let mut world = test_world();
            for y in 64..=66 {
                for x in 7..=9 {
                    world.set(IVec3::new(x, y, 7), state);
                }
            }
            let ((lo, hi), (cull_lo, cull_hi)) = rabbit_boxes(Vec3::new(8.5, 64.0, 8.5));
            assert!(!occluded_bounds(&world, eye, lo, hi));
            assert!(!occluded_bounds(&world, eye, cull_lo, cull_hi));
        }
    }

    #[test]
    fn rabbit_behind_one_block_stone_ledge_stays_visible_while_three_by_three_wall_culls() {
        let eye = Vec3::new(8.5, 65.62, 5.5);

        // 1. Rabbit behind a 1-block solid stone ledge where ears (y = 0.7..1.07) peek over
        let mut ledge_world = test_world();
        for x in 7..=9 {
            ledge_world.set(IVec3::new(x, 64, 7), 1 << 4);
        }
        let (_, (cull_lo, cull_hi)) = rabbit_boxes(Vec3::new(8.5, 64.0, 8.5));
        assert!(!occluded_bounds(&ledge_world, eye, cull_lo, cull_hi));

        // 2. Rabbit behind a 1-block solid stone ledge + back overhang where ONLY front-top
        //    corners (min.z, max.y) are visible from the camera
        let mut front_top_world = test_world();
        for x in 7..=9 {
            front_top_world.set(IVec3::new(x, 64, 7), 1 << 4);
            front_top_world.set(IVec3::new(x, 65, 8), 1 << 4);
        }
        let (_, (front_cull_lo, front_cull_hi)) = rabbit_boxes(Vec3::new(8.5, 64.0, 8.2));
        assert!(!occluded_bounds(
            &front_top_world,
            eye,
            front_cull_lo,
            front_cull_hi,
        ));

        // 3. Rabbit completely sealed behind a 3x3 solid stone wall IS properly culled
        let mut wall_world = test_world();
        for y in 64..=66 {
            for x in 7..=9 {
                wall_world.set(IVec3::new(x, y, 7), 1 << 4);
            }
        }
        assert!(occluded_bounds(&wall_world, eye, cull_lo, cull_hi));
    }
}
