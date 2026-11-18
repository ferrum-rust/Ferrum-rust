use super::*;
use glam::{Mat4, Vec3};
/// UUID.nameUUIDFromBytes("OfflinePlayer:" + name), then DefaultPlayerSkin.isSlimSkin.
/// Only the low bit of Java UUID.hashCode is needed. Version/variant bits cancel out of this bit.
pub(crate) fn offline_slim(name: &str) -> bool {
    offline_uuid(name).chunks_exact(4).map(|n|n[3]&1).fold(0,|a,b|a^b)!=0
}
pub(crate) fn offline_uuid(name: &str) -> [u8;16] {
    let mut bytes = b"OfflinePlayer:".to_vec();
    bytes.extend_from_slice(name.as_bytes());
    let bits = (bytes.len() as u64) * 8;
    bytes.push(128);
    while bytes.len() % 64 != 56 {
        bytes.push(0)
    }
    bytes.extend_from_slice(&bits.to_le_bytes());
    let mut hash = [0x67452301u32, 0xefcdab89, 0x98badcfe, 0x10325476];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x2441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x4881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];
    for block in bytes.chunks_exact(64) {
        let m: [u32; 16] = std::array::from_fn(|i| {
            u32::from_le_bytes(block[i * 4..i * 4 + 4].try_into().unwrap())
        });
        let [mut a, mut b, mut c, mut d] = hash;
        for i in 0..64 {
            let (f, g, shift) = match i {
                0..16 => ((b & c) | (!b & d), i, [7, 12, 17, 22][i % 4]),
                16..32 => ((b & d) | (c & !d), (5 * i + 1) % 16, [5, 9, 14, 20][i % 4]),
                32..48 => (b ^ c ^ d, (3 * i + 5) % 16, [4, 11, 16, 23][i % 4]),
                _ => (c ^ (b | !d), (7 * i) % 16, [6, 10, 15, 21][i % 4]),
            };
            let next = b.wrapping_add(
                a.wrapping_add(f)
                    .wrapping_add(K[i])
                    .wrapping_add(m[g])
                    .rotate_left(shift),
            );
            (a, b, c, d) = (d, next, b, c);
        }
        for (out, n) in hash.iter_mut().zip([a, b, c, d]) {
            *out = out.wrapping_add(n);
        }
    }
    let mut id=[0u8;16];
    for (out,n) in id.chunks_exact_mut(4).zip(hash){out.copy_from_slice(&n.to_le_bytes());}
    id[6]=(id[6]&15)|0x30;id[8]=(id[8]&63)|0x80;
    id
}

pub(super) fn draw(p: &mut Painter<'_>, x: f32, y: f32, size: f32, look: [f32; 2], sim: &Simulation, slim: bool) {
    let yaw = (look[0] / 40.).atan() * 20f32.to_radians();
    let pitch = -(look[1] / 40.).atan() * 20f32.to_radians();
    let slim = sim.presentation.local_slim().unwrap_or(slim);
    let skin = sim.presentation.local_skin_region(p.r, slim);
    let mesh = crate::models::inventory_player(
        p.r,
        p.a,
        skin.layer,
        p.s.menu.skin_parts,
        yaw,
        pitch,
        sim,
        slim,
    );
    let mat = Mat4::from_scale(Vec3::new(size, -size, size) * 0.9375)
        * Mat4::from_rotation_x(pitch)
        * Mat4::from_rotation_y(std::f32::consts::PI - yaw)
        * Mat4::from_scale(Vec3::new(-1., 1., 1.))
        * Mat4::from_translation(Vec3::Y * 0.0078125);
    let mut faces = Vec::new();
    for q in mesh.vertices.chunks_exact(4) {
        let v: [Vec3; 4] =
            std::array::from_fn(|i| mat.transform_point3(Vec3::from_array(q[i].position)));
        let normal = (v[1] - v[0]).cross(v[2] - v[0]).normalize_or_zero();
        if normal.z <= 0. {
            continue;
        }
        let depth = v.iter().map(|p| p.z).sum::<f32>();
        faces.push((depth, v, q, normal));
    }
    faces.sort_by(|a, b| a.0.total_cmp(&b.0));
    let uv_scale = [skin.scale[0] / 0.25, skin.scale[1] / 0.25];
    let glint_sprite = p.r.sprite("textures/misc/enchanted_item_glint.png");
    for (_, v, q, normal) in faces {
        let light = if q[0].layer & crate::models::UNLIT != 0 { 1. } else { entity_lighting(normal) };
        let has_glint = q[0].layer & crate::models::GLINT != 0;
        let base_layer = q[0].layer & 0x8001_ffff;
        let is_skin = (base_layer & 0x1ffff) == skin.layer;
        let start = p.frame.vertices.len();
        p.polygon(
            v.map(|v| [x + v.x, y + v.y]),
            std::array::from_fn(|i| {
                if is_skin {
                    [
                        skin.origin[0] + q[i].uv[0] * uv_scale[0],
                        skin.origin[1] + q[i].uv[1] * uv_scale[1],
                    ]
                } else {
                    q[i].uv
                }
            }),
            base_layer,
            [
                light * f32::from(q[0].color[0]) / 255.,
                light * f32::from(q[0].color[1]) / 255.,
                light * f32::from(q[0].color[2]) / 255.,
                1.,
            ],
        );
        if has_glint {
            for (vertex, j) in p.frame.vertices[start..].iter_mut().zip([0, 1, 2, 0, 2, 3]) {
                let u = f32::from(q[j].light[2]) / 255.0;
                let v = f32::from(q[j].light[3]) / 255.0;
                for (k, angle, phase) in [
                    (0, -50f32, (p.time % 3.) / 3.),
                    (2, 10f32, -(p.time % 4.873) / 4.873),
                ] {
                    let (sin, cos) = angle.to_radians().sin_cos();
                    vertex.glint[k] = 0.25 * (u * cos - v * sin) + phase;
                    vertex.glint[k + 1] = 0.25 * (u * sin + v * cos);
                }
                vertex.layer |= glint_sprite.layer << 17;
            }
        }
    }
}
pub(super) fn entity_lighting(normal: Vec3) -> f32 {
    let lights = Mat4::from_scale(Vec3::new(1., -1., 1.))
        * Mat4::from_rotation_y(135f32.to_radians());
    (0.4 + 0.6 * [Vec3::new(0.2,1.,-0.7),Vec3::new(-0.2,1.,0.7)]
        .iter().map(|v|normal.dot(lights.transform_vector3(v.normalize())).max(0.)).sum::<f32>()).min(1.)
}
pub(super) fn effects(p: &mut Painter<'_>, x: f32, mut y: f32, sim: &Simulation) {
    const INV: &str = "textures/gui/container/inventory.png";
    const EFFECTS: [(&str, i32); 24] = [
        ("", -1),
        ("moveSpeed", 0),
        ("moveSlowdown", 1),
        ("digSpeed", 2),
        ("digSlowDown", 3),
        ("damageBoost", 4),
        ("heal", -1),
        ("harm", -1),
        ("jump", 10),
        ("confusion", 11),
        ("regeneration", 7),
        ("resistance", 14),
        ("fireResistance", 15),
        ("waterBreathing", 16),
        ("invisibility", 8),
        ("blindness", 13),
        ("nightVision", 12),
        ("hunger", 9),
        ("weakness", 5),
        ("poison", 6),
        ("wither", 17),
        ("healthBoost", 18),
        ("absorption", 18),
        ("saturation", -1),
    ];
    let count = sim.effects.ordered().count();
    for effect in sim.effects.ordered() {
        let Some(&(name, icon)) = EFFECTS.get(effect.id as usize) else {
            continue;
        };
        p.image(
            INV,
            Rect {
                x,
                y,
                w: 140.,
                h: 32.,
            },
            [0., 166., 140., 32.],
            WHITE,
        );
        if icon >= 0 {
            p.image(
                INV,
                Rect {
                    x: x + 6.,
                    y: y + 7.,
                    w: 18.,
                    h: 18.,
                },
                [
                    (icon % 8 * 18) as f32,
                    (198 + icon / 8 * 18) as f32,
                    18.,
                    18.,
                ],
                WHITE,
            );
        }
        let name = effect_label(name, effect.amplifier, |key| p.tr(key));
        p.text(x + 28., y + 6., &name, WHITE);
        let secs = effect.duration / 20;
        p.text(
            x + 28.,
            y + 16.,
            &format!("{}:{:02}", secs / 60, secs % 60),
            [127. / 255., 127. / 255., 127. / 255., 1.],
        );
        y += if count > 5 {
            (132 / (count - 1)) as f32
        } else {
            33.
        };
    }
}

fn effect_label(name: &str, amplifier: i32, tr: impl Fn(&str) -> String) -> String {
    // Show every level as requested, including I and the V+ levels omitted by
    // the original inventory. Resource-pack level translations still apply.
    let level = i64::from(amplifier) + 1;
    let key = format!("enchantment.level.{level}");
    let translated = tr(&key);
    let suffix = if translated == key { level.to_string() } else { translated };
    format!("{} {suffix}", tr(&format!("potion.{name}")))
}

#[cfg(test)]
#[test]
fn inventory_effect_labels_include_first_high_and_localized_levels() {
    let tr = |key: &str| match key {
        "potion.regeneration" => "Regeneration".into(),
        "enchantment.level.1" => "I".into(),
        "enchantment.level.2" => "II".into(),
        "enchantment.level.5" => "V".into(),
        "enchantment.level.10" => "X".into(),
        _ => key.to_owned(),
    };
    for (amplifier, suffix) in [(0, "I"), (1, "II"), (4, "V"), (9, "X"), (10, "11"), (255, "256")] {
        assert_eq!(effect_label("regeneration", amplifier, tr), format!("Regeneration {suffix}"));
    }
    assert_eq!(effect_label("regeneration", 4, |key| match key {
        "potion.regeneration" => "재생".into(),
        "enchantment.level.5" => "5단계".into(),
        _ => key.to_owned(),
    }), "재생 5단계");
}

#[cfg(test)]
#[test]
fn offline_default_skin_matches_java_uuid() {
    for (name, slim) in [
        ("Menu189", true),
        ("FerrumPlayer", false),
        ("Notch", false),
        ("Steve", true),
        ("Alex", false),
    ] {
        assert_eq!(offline_slim(name), slim, "{name}");
    }
}
