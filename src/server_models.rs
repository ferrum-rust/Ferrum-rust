//! Dynamic entities and the active map surface received from a protocol-47 server.
use super::*;
use std::collections::HashMap;
#[derive(Clone, Debug, Default)]
pub struct Actor {
    pub uuid: Option<[u8; 16]>,
    pub gait: crate::entity_visual::Gait,
    pub anim: crate::player::third_person::Animation,
    pub swing_anim: crate::first_person::Swing,
    pub previous: Vec3,
    pub feet: Vec3,
    pub previous_yaw: f32,
    pub yaw: f32,
    pub previous_pitch: f32,
    pub pitch: f32,
    pub skin: u8,
    pub alex: bool,
    pub held: Option<Stack>,
    pub armor: [Option<Stack>; 4],
    pub sneaking: bool,
    pub invisible: bool,
    pub using_item: bool,
    pub fishing_cast: bool,
    pub hurt: bool,
    pub bed: Option<glam::IVec3>,
    pub swing: f32,
    pub name: Option<String>,
}
#[derive(Clone, Debug)]
pub struct RemoteProjectile {
    pub id: i32,
    pub kind: u8,
    pub data: i32,
    pub is_local_owner: bool,
    pub invulnerable: bool,
    pub previous: Vec3,
    pub position: Vec3,
    pub previous_yaw: f32,
    pub yaw: f32,
    pub previous_pitch: f32,
    pub pitch: f32,
}
#[derive(Clone, Debug)]
pub struct RemoteLabel {
    pub id: i32,
    pub previous: Vec3,
    pub position: Vec3,
    pub y_offset: f32,
    pub text: String,
    pub sneaking: bool,
}
#[derive(Clone, Debug, Default)]
pub struct BasicMobVisual {
    pub baby: bool,
    pub invisible: bool,
    pub villager_profession: u8,
    pub sheep_color: u8,
    pub sheep_sheared: bool,
    pub pig_saddled: bool,
    pub skeleton_wither: bool,
    pub zombie_villager: bool,
    pub creeper_state: i8,
    pub creeper_powered: bool,
    pub creeper_ignited: bool,
    pub creeper_swell: u8,
    pub prev_creeper_swell: u8,
    pub swing_ticks: u8,
    pub eat_ticks: u8,
    pub death_time: u8,
    pub head_yaw: Option<f32>,
    pub pitch: f32,
    pub held: Option<crate::inventory::Stack>,
    pub armor: [Option<crate::inventory::Stack>; 4],
}
#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub burning: std::collections::HashSet<i32>,
    pub chest_lids: crate::chest_render::Lids,
    pub bosses: Vec<crate::models::boss_bar::Candidate>,
    pub vehicles: HashMap<i32,super::vehicle::Actor>,
    pub mobs:HashMap<i32,crate::remote_mobs::Actor>,
    pub basic_mobs: HashMap<u64, BasicMobVisual>,
    pub actors: HashMap<i32, Actor>,
    pub projectiles: Vec<RemoteProjectile>,
    pub labels: Vec<RemoteLabel>,
    pub local_fishing_cast: bool,
    pub hide_hud: bool,
    pub paintings: HashMap<i32, (String, glam::IVec3, u8)>,
    pub cracks: Vec<(glam::IVec3, u8)>,
    pub lightning: Vec<(Vec3, u8)>,
    pub camera: Option<i32>,
    /// S43 can select a different entity, whose damage animation is independent of the player.
    pub camera_hurt: Option<(crate::gameplay::HurtCamera, bool)>,
    pub map: Option<MapSurface>,
}
#[derive(Clone, Debug)]
pub struct MapSurface {
    pub id: i32,
    pub revision: u64,
    pub rgba: std::sync::Arc<Vec<u8>>,
    pub icons: Vec<[u8; 3]>,
}
pub fn pixels(colors: &[u8]) -> Vec<u8> {
    const BASE: [u32; 36] = [
        0, 8368696, 16247203, 0xc7c7c7, 0xff0000, 0xa0a0ff, 0xa7a7a7, 31744, 0xffffff, 10791096,
        9923917, 0x707070, 0x4040ff, 9402184, 0xfffcf5, 14188339, 11685080, 6724056, 0xe5e533,
        8375321, 15892389, 0x4c4c4c, 0x999999, 5013401, 8339378, 3361970, 6704179, 6717235,
        0x993333, 0x191919, 16445005, 6085589, 4882687, 55610, 8476209, 0x700200,
    ];
    let mut out = Vec::with_capacity(colors.len() * 4);
    for (i, &b) in colors.iter().enumerate() {
        let index = usize::from(b / 4);
        if index == 0 || index >= BASE.len() {
            out.extend([0, 0, 0, 16 + 8 * ((i + i / 128) & 1) as u8]);
            continue;
        }
        let shade = [180, 220, 255, 135][usize::from(b & 3)];
        let word = BASE[index];
        for shift in [16, 8, 0] {
            out.push((((word >> shift) & 255) * shade / 255) as u8);
        }
        out.push(255);
    }
    out
}
fn art(name: &str) -> [u8; 4] {
    match name {
        "Aztec" => [16, 16, 16, 0],
        "Alban" => [16, 16, 32, 0],
        "Aztec2" => [16, 16, 48, 0],
        "Bomb" => [16, 16, 64, 0],
        "Plant" => [16, 16, 80, 0],
        "Wasteland" => [16, 16, 96, 0],
        "Pool" => [32, 16, 0, 32],
        "Courbet" => [32, 16, 32, 32],
        "Sea" => [32, 16, 64, 32],
        "Sunset" => [32, 16, 96, 32],
        "Creebet" => [32, 16, 128, 32],
        "Wanderer" => [16, 32, 0, 64],
        "Graham" => [16, 32, 16, 64],
        "Match" => [32, 32, 0, 128],
        "Bust" => [32, 32, 32, 128],
        "Stage" => [32, 32, 64, 128],
        "Void" => [32, 32, 96, 128],
        "SkullAndRoses" => [32, 32, 128, 128],
        "Wither" => [32, 32, 160, 128],
        "Fighters" => [64, 32, 0, 96],
        "Pointer" => [64, 64, 0, 192],
        "Pigscene" => [64, 64, 64, 192],
        "BurningSkull" => [64, 64, 128, 192],
        "Skeleton" => [64, 48, 192, 64],
        "DonkeyKong" => [64, 48, 192, 112],
        _ => [16, 16, 0, 0],
    }
}
fn nametag_visible(pres: &crate::net_presentation::State, target: &str, viewer: &str) -> bool {
    let Some(team) = pres.team(target) else {
        return true;
    };
    match team.visibility.as_str() {
        "never" => false,
        "hideForOtherTeams" => pres.memberships.get(viewer) == pres.memberships.get(target),
        "hideForOwnTeam" => pres.memberships.get(viewer) != pres.memberships.get(target),
        _ => true,
    }
}

fn visual_light_since(mesh: &mut Mesh, start: usize, world: &World, position: Vec3) {
    let light = world.visual_light_uv(position.floor().as_ivec3(), 0);
    for vertex in &mut mesh.vertices[start..] {
        vertex.light[..2].copy_from_slice(&light[..2]);
    }
}

pub(super) fn draw(d: &mut Dynamic, c: &ModelContext<'_>) {
    struct PendingTag<'a> {
        dist_sq: f32,
        pos: Vec3,
        text: std::borrow::Cow<'a, str>,
        sneaking: bool,
    }
    let s = &c.sim.server_scene;
    for (&id,actor) in &s.vehicles { if s.camera!=Some(id){super::vehicle::draw(d,c,id,actor);} }
    for (&id,actor) in &s.mobs{if s.camera!=Some(id){crate::remote_mobs::draw(d,c,actor,s.burning.contains(&id))}}
    d.total += s.actors.len() + s.paintings.len();
    let cam_yaw = -c.player.yaw + if c.perspective == 2 { std::f32::consts::PI } else { 0. };
    let cam_pitch = if c.perspective == 2 { -c.player.pitch } else { c.player.pitch };
    let cam_rot = Mat4::from_rotation_y(cam_yaw) * Mat4::from_rotation_x(cam_pitch);
    let tag_right = cam_rot.transform_vector3(Vec3::X);
    let tag_up = cam_rot.transform_vector3(Vec3::Y);
    let mut pending_tags: Vec<PendingTag<'_>> = Vec::new();
    for (&id, a) in &s.actors {
        let p = a.previous.lerp(a.feet, c.alpha);
        let dist_sq = p.distance_squared(c.eye);
        if (s.camera == Some(id) && c.perspective == 0 && a.bed.is_none())
            || dist_sq > (c.settings.entity_distance * c.settings.entity_distance) as f32
            || !c
                .frustum
                .visible(p - Vec3::splat(0.45), p + Vec3::new(0.45, 2.6, 0.45))
        {
            d.culled += 1;
            continue;
        }
        d.visible += 1;
        let start = d.opaque.vertices.len();
        let pitch = a.previous_pitch + (a.pitch - a.previous_pitch) * c.alpha;
        let swing = a.swing_anim.sample(c.alpha);
        let skin = a.uuid.map_or_else(
            || c.resources.skins.resolve(c.resources, None, a.anim.slim),
            |uuid| c.sim.presentation.skin_region(c.resources, uuid),
        );
        third_person_model::draw_player(
            &mut d.opaque,
            c,
            third_person_model::PlayerModelInput {
                feet: p,
                anim: &a.anim,
                skin,
                bed: a.bed,
                pitch,
                swing,
                sneak: a.sneaking,
                using_item: a.using_item,
                held: a.held.as_ref(),
                armor: [
                    a.armor[3].as_ref(),
                    a.armor[2].as_ref(),
                    a.armor[1].as_ref(),
                    a.armor[0].as_ref(),
                ],
                skin_parts: a.skin,
                spectator: false,
                invisible: a.invisible,
                hurt: a.hurt,
                fishing_cast: a.fishing_cast,
            },
        );
        visual_light_since(&mut d.opaque, start, c.world, p + Vec3::Y * 1.62);
        if s.burning.contains(&id) {
            for v in &mut d.opaque.vertices[start..] {
                v.light[0] = 240;
                v.light[1] = 240;
            }
            let fire_start = d.translucent.vertices.len();
            c.fire(&mut d.translucent, fire_start, p, 0.6, 1.8);
        }
        if c.settings.entity_shadows && a.bed.is_none() && !a.invisible {
            crate::models::shadow_radius(&mut d.translucent, c, p, 0.5, 1.0, false);
        }
        if c.hitboxes {
            let h = if a.sneaking { 1.65 } else { 1.8 };
            wirebox(
                &mut d.opaque,
                p - Vec3::new(0.3, 0., 0.3),
                p + Vec3::new(0.3, h, 0.3),
            );
        }
        if !s.hide_hud && s.camera != Some(id) && !a.invisible {
            if let Some(raw_name) = a.name.as_deref().filter(|n| !n.is_empty()) {
                let max_tag_dist_sq = if a.sneaking { 32. * 32. } else { 64. * 64. };
                if dist_sq < max_tag_dist_sq
                    && nametag_visible(&c.sim.presentation, raw_name, &c.sim.local_name)
                {
                    let mut label_y = p.y
                        + if a.bed.is_some() {
                            0.7
                        } else if a.sneaking {
                            2.05
                        } else {
                            2.3
                        };
                    if !a.sneaking && dist_sq < 100. {
                        if let Some(obj) = c.sim.presentation.objective(2) {
                            let score = obj.scores.get(raw_name).copied().unwrap_or(0);
                            let below = format!("{score} {}", obj.title);
                            let below_pos = Vec3::new(p.x, label_y, p.z);
                            pending_tags.push(PendingTag {
                                dist_sq: below_pos.distance_squared(c.eye),
                                pos: below_pos,
                                text: std::borrow::Cow::Owned(below),
                                sneaking: false,
                            });
                            label_y += 9.0 * 1.15 * (0.016666668 * 1.6);
                        }
                    }
                    let formatted = c.sim.presentation.formatted_name(raw_name);
                    let tag_pos = Vec3::new(p.x, label_y, p.z);
                    pending_tags.push(PendingTag {
                        dist_sq: tag_pos.distance_squared(c.eye),
                        pos: tag_pos,
                        text: std::borrow::Cow::Owned(formatted),
                        sneaking: a.sneaking,
                    });
                }
            }
        }
    }
    if !s.hide_hud {
        for l in &s.labels {
            if s.camera == Some(l.id) {
                continue;
            }
            let p = l.previous.lerp(l.position, c.alpha);
            let max_tag_dist_sq = if l.sneaking { 32. * 32. } else { 64. * 64. };
            if p.distance_squared(c.eye) >= max_tag_dist_sq {
                continue;
            }
            let tag_pos = Vec3::new(p.x, p.y + l.y_offset, p.z);
            if !c.frustum.visible(
                tag_pos - Vec3::new(4.0, 0.5, 4.0),
                tag_pos + Vec3::new(4.0, 0.8, 4.0),
            ) {
                continue;
            }
            pending_tags.push(PendingTag {
                dist_sq: tag_pos.distance_squared(c.eye),
                pos: tag_pos,
                text: std::borrow::Cow::Borrowed(&l.text),
                sneaking: l.sneaking,
            });
        }
    }
    pending_tags.sort_by(|a, b| b.dist_sq.total_cmp(&a.dist_sq));
    for tag in pending_tags {
        crate::ui::world_nametag(
            &mut d.nametags,
            c.resources,
            c.settings,
            tag.pos,
            tag_right,
            tag_up,
            &tag.text,
            tag.sneaking,
        );
    }
    for (name, p, facing) in s.paintings.values() {
        let [w, h, u, v] = art(name);
        let normal = [Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z, Vec3::X][usize::from(*facing & 3)];
        let right = Vec3::Y.cross(normal);
        let p = p.as_vec3() + Vec3::splat(0.5) - normal * 0.46
            + right * (if w % 32 == 0 { 0.5 } else { 0. })
            + Vec3::Y * (if h % 32 == 0 { 0.5 } else { 0. });
        let x = right * f32::from(w) / 32.;
        let y = Vec3::Y * f32::from(h) / 32.;
        if !c.frustum.visible(p - x.abs() - y, p + x.abs() + y) {
            d.culled += 1;
            continue;
        }
        d.visible += 1;
        let shade = (crate::models::entity_face_shade(normal) * 255.).round() as u8;
        let start = d.opaque.vertices.len();
        face(
            &mut d.opaque,
            [p - x - y, p + x - y, p + x + y, p - x + y],
            [u, v, w, h].map(|v| f32::from(v) / 256.),
            c.resources
                .sprite("textures/painting/paintings_kristoffer_zetterstrand.png")
                .layer
                | ATLAS,
            [shade, shade, shade, 255],
        );
        visual_light_since(&mut d.opaque, start, c.world, p);
    }
    for &(p, age) in &s.lightning {
        let mut rng = crate::vanilla::random::JavaRandom::new(
            i64::from(p.x.to_bits()) ^ i64::from(p.z.to_bits()) ^ i64::from(age / 3),
        );
        let mut a = p;
        for _ in 0..8 {
            let b = a + Vec3::new(
                (rng.next_int(11) - 5) as f32,
                16.,
                (rng.next_int(11) - 5) as f32,
            );
            let edge = Vec3::X * 0.12;
            face(
                &mut d.translucent,
                [a - edge, a + edge, b + edge, b - edge],
                [0.; 4],
                SOLID | UNLIT | BLEND,
                [212, 211, 255, 220],
            );
            a = b;
        }
    }
}
pub(super) fn held_map(mesh: &mut Mesh, c: &ModelContext<'_>, map: &MapSurface) {
    let dir = c.player.direction();
    let right = Vec3::new(c.player.yaw.cos(), 0., c.player.yaw.sin());
    let up = right.cross(dir).normalize_or_zero();
    let center = c.eye + dir * 0.72 - up * 0.24;
    let quad = |p: Vec3, x: Vec3, y: Vec3| [p - x - y, p + x - y, p + x + y, p - x + y];
    face(
        mesh,
        quad(center, right * 0.35, up * 0.35),
        [0., 0., 0.5, 0.5],
        c.resources.sprite("textures/map/map_background.png").layer | ATLAS,
        [255; 4],
    );
    let center = center - dir * 0.001;
    face(
        mesh,
        quad(center, right * 0.326, up * 0.326),
        [0., 0., 0.5, 0.5],
        (c.resources.layers() - 1) | ATLAS,
        [255; 4],
    );
    for &[t, x, y] in &map.icons {
        let p = center - dir * 0.002 + right * (x as i8 as f32 / 128. * 0.326)
            - up * (y as i8 as f32 / 128. * 0.326);
        let a = f32::from(t & 15) * std::f32::consts::TAU / 16.;
        let (sin, cos) = a.sin_cos();
        let icon = t >> 4;
        face(
            mesh,
            quad(
                p,
                (right * cos - up * sin) * 0.015,
                (right * sin + up * cos) * 0.015,
            ),
            [
                f32::from(icon % 4) * 4. / 256.,
                f32::from(icon / 4) * 4. / 256.,
                4. / 256.,
                4. / 256.,
            ],
            c.resources.sprite("textures/map/map_icons.png").layer | ATLAS,
            [255; 4],
        );
    }
}
