//! RenderBoat and RenderMinecart at partialTicks, using the rail under the rendered entity.
use glam::{DVec3, IVec3, Mat4, Vec3};

#[derive(Clone, Copy, Debug, Default)]
pub struct Damage {
    pub ticks: i32,
    pub direction: i32,
    pub amount: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Actor {
    pub cart: bool,
    pub from: DVec3,
    pub to: DVec3,
    pub previous_yaw: f32,
    pub yaw: f32,
    pub previous_pitch: f32,
    pub pitch: f32,
    pub damage: Damage,
    pub display: Option<(u16, i32)>,
}

const RAILS: [[[i32; 3]; 2]; 10] = [
    [[0, 0, -1], [0, 0, 1]],
    [[-1, 0, 0], [1, 0, 0]],
    [[-1, -1, 0], [1, 0, 0]],
    [[-1, 0, 0], [1, -1, 0]],
    [[0, 0, -1], [0, -1, 1]],
    [[0, -1, -1], [0, 0, 1]],
    [[0, 0, 1], [1, 0, 0]],
    [[0, 0, 1], [-1, 0, 0]],
    [[0, 0, -1], [-1, 0, 0]],
    [[0, 0, -1], [1, 0, 0]],
];
fn rail(get: &impl Fn(IVec3) -> u16, at: DVec3) -> Option<(IVec3, [DVec3; 2], usize)> {
    let block = at.floor().as_ivec3();
    for p in [block - IVec3::Y, block] {
        let state = get(p);
        if matches!(state >> 4, 27 | 28 | 66 | 157) {
            let shape = crate::assets::rail_shape(state) as usize;
            return Some((
                p,
                RAILS[shape].map(|a| IVec3::from_array(a).as_dvec3()),
                shape,
            ));
        }
    }
    None
}
fn rail_center(get: &impl Fn(IVec3) -> u16, at: DVec3) -> Option<DVec3> {
    let (p, ends, _) = rail(get, at)?;
    let origin = p.as_dvec3();
    let v = origin + DVec3::new(0.5, 0.0625, 0.5) + ends[0] * 0.5;
    let diff = (ends[1] - ends[0]) * DVec3::new(0.5, 1., 0.5);
    let t = if diff.x == 0. {
        at.z - origin.z
    } else if diff.z == 0. {
        at.x - origin.x
    } else {
        2. * ((at.x - v.x) * diff.x + (at.z - v.z) * diff.z)
    };
    let lift = if diff.y < 0. {
        1.
    } else if diff.y > 0. {
        0.5
    } else {
        0.
    };
    Some(v + diff * t + DVec3::Y * lift)
}
fn rail_offset(get: &impl Fn(IVec3) -> u16, at: DVec3, distance: f64) -> Option<DVec3> {
    let (p, ends, shape) = rail(get, at)?;
    let mut at = at.with_y(f64::from(p.y) + if (2..=5).contains(&shape) { 1. } else { 0. });
    at += (ends[1] - ends[0]).with_y(0.).normalize() * distance;
    for end in ends {
        let delta = at.floor().as_ivec3() - p;
        if end.y != 0. && delta.x == end.x as i32 && delta.z == end.z as i32 {
            at.y += end.y;
            break;
        }
    }
    rail_center(get, at)
}
impl Actor {
    /// Before the final GL scale(-1,-1,1), so cart display tiles use the same translation and wobble.
    pub fn transform(&self, id: i32, alpha: f32, get: &impl Fn(IVec3) -> u16) -> Mat4 {
        let at = self.from.lerp(self.to, f64::from(alpha));
        let mut pos = at.as_vec3();
        let mut yaw = self.previous_yaw + (self.yaw - self.previous_yaw) * alpha;
        let mut pitch = 0.;
        if self.cart {
            let seed = i64::from(id).wrapping_mul(493286711);
            let seed = seed
                .wrapping_mul(seed)
                .wrapping_mul(4392167121)
                .wrapping_add(seed.wrapping_mul(98761));
            let jitter = [16, 20, 24]
                .map(|shift| ((((seed >> shift) & 7i64) as f32 + 0.5) / 8. - 0.5) * 0.004);
            pitch = self.previous_pitch + (self.pitch - self.previous_pitch) * alpha;
            if let Some(center) = rail_center(get, at) {
                let offset = f64::from(0.3f32);
                let a = rail_offset(get, at, offset).unwrap_or(center);
                let b = rail_offset(get, at, -offset).unwrap_or(center);
                pos = DVec3::new(center.x, (a.y + b.y) * 0.5, center.z).as_vec3();
                let delta = b - a;
                let len = f64::from(delta.length() as f32);
                if len != 0. {
                    let dir = if len < 0.0001 {
                        DVec3::ZERO
                    } else {
                        delta / len
                    };
                    yaw = dir.z.atan2(dir.x).to_degrees() as f32;
                    pitch = (dir.y.atan() * 73.) as f32;
                }
            }
            pos.y += 0.375;
            pos += Vec3::from_array(jitter);
        } else {
            pos.y += 0.25;
        }
        let ticks = self.damage.ticks as f32 - alpha;
        let wobble = if ticks > 0. {
            crate::vanilla::math::sin(ticks) * ticks * (self.damage.amount - alpha).max(0.) / 10.
                * self.damage.direction as f32
        } else {
            0.
        };
        Mat4::from_translation(pos)
            * Mat4::from_rotation_y((180. - yaw).to_radians())
            * Mat4::from_rotation_z(-pitch.to_radians())
            * Mat4::from_rotation_x(wobble.to_radians())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_boat_and_cart_modelview_on_rails_and_damage() {
        type Frame = (
            i32,
            i32,
            f32,
            [f64; 3],
            [f64; 3],
            f32,
            f32,
            f32,
            i32,
            f32,
            i32,
            [f32; 16],
        );
        let frames: Vec<Frame> = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-vehicle-render.json"
        ))
        .unwrap();
        for (k, id, a, from, to, yaw, previous_pitch, pitch, ticks, amount, direction, matrix) in
            frames
        {
            let actor = Actor {
                cart: k != -2,
                from: from.into(),
                to: to.into(),
                previous_yaw: yaw,
                yaw,
                previous_pitch,
                pitch,
                damage: Damage {
                    ticks,
                    amount,
                    direction,
                },
                display: None,
            };
            let get = |p| {
                if p == IVec3::new(8, 150, 8) && k >= 0 {
                    (66 << 4) | k as u16
                } else {
                    0
                }
            };
            let model = actor.transform(id, a, &get) * Mat4::from_scale(Vec3::new(-1., -1., 1.));
            for (i, (out, original)) in model.to_cols_array().iter().zip(matrix).enumerate() {
                assert!(
                    (out - original).abs() < 2e-5,
                    "shape {k} id {id} alpha {a} matrix[{i}] {out} != {original}"
                );
            }
        }
    }
}
