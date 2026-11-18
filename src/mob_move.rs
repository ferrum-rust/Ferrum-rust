//! EntityMoveHelper's yaw limit, requested movement speed and jump trigger.
use glam::DVec3;
include!("mob_angle_data.rs");

pub fn atan2(mut y: f64, mut x: f64) -> f64 {
    let length = x * x + y * y;
    if length.is_nan() {
        return f64::NAN;
    }
    let neg_y = y < 0.;
    let neg_x = x < 0.;
    if neg_y {
        y = -y;
    }
    if neg_x {
        x = -x;
    }
    let swap = y > x;
    if swap {
        std::mem::swap(&mut x, &mut y);
    }
    let mut inv = f64::from_bits(
        (6_910_469_410_427_058_090i64.wrapping_sub((length.to_bits() as i64) >> 1)) as u64,
    );
    inv *= 1.5 - length * 0.5 * inv * inv;
    x *= inv;
    y *= inv;
    let bias = f64::from_bits(4_805_340_802_404_319_232);
    let biased = bias + y;
    let index = biased.to_bits() as u32 as usize;
    let residual = y * f64::from_bits(COS[index]) - x * (biased - bias);
    let mut angle = f64::from_bits(ASIN[index]) + (6. + residual * residual) * residual * (1. / 6.);
    if swap {
        angle = std::f64::consts::FRAC_PI_2 - angle;
    }
    if neg_x {
        angle = std::f64::consts::PI - angle;
    }
    if neg_y {
        angle = -angle;
    }
    angle
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MoveControl {
    pub yaw: f32,
    pub ai_speed: f32,
    pub forward: f32,
    pub jump: bool,
}
impl MoveControl {
    pub fn new(yaw: f32) -> Self {
        Self {
            yaw,
            ai_speed: 0.,
            forward: 0.,
            jump: false,
        }
    }
    pub fn update(&mut self, position: DVec3, target: Option<(DVec3, f64)>, base_speed: f64) {
        self.forward = 0.;
        self.jump = false;
        let Some((to, speed)) = target else {
            return;
        };
        let dx = to.x - position.x;
        let dy = to.y - f64::from((position.y + 0.5).floor() as i32);
        let dz = to.z - position.z;
        if dx * dx + dy * dy + dz * dz < 2.500000277905201E-7 {
            return;
        }
        let desired = (atan2(dz, dx) * 180. / f64::from(std::f32::consts::PI)) as f32 - 90.;
        let mut delta = (desired - self.yaw) % 360.;
        if delta >= 180. {
            delta -= 360.;
        }
        if delta < -180. {
            delta += 360.;
        }
        self.yaw += delta.clamp(-30., 30.);
        if self.yaw < 0. {
            self.yaw += 360.;
        } else if self.yaw > 360. {
            self.yaw -= 360.;
        }
        self.ai_speed = (speed * base_speed) as f32;
        self.forward = self.ai_speed;
        self.jump = dy > 0. && dx * dx + dz * dz < 1.;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::io::Read;
    #[test]
    fn mob_move_helper_yaw_attribute_jump_and_atan2_match_original_bytecode() {
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(
            include_bytes!("../tests/fixtures/vanilla189-mob-move.json.gz").as_slice(),
        )
        .read_to_end(&mut bytes)
        .unwrap();
        let data: Value = serde_json::from_slice(&bytes).unwrap();
        for (i, row) in data["angles"].as_array().unwrap().iter().enumerate() {
            let f = |j: usize| {
                f64::from_bits(u64::from_str_radix(row[j].as_str().unwrap(), 16).unwrap())
            };
            assert_eq!(atan2(f(0), f(1)).to_bits(), f(2).to_bits(), "angle{i}");
        }
        let mut count = 0;
        for (i, row) in data["cases"].as_array().unwrap().iter().enumerate() {
            let mut control = MoveControl::new(f32::from_bits(row[1].as_u64().unwrap() as u32));
            for (t, frame) in row[2].as_array().unwrap().iter().enumerate() {
                let pos = serde_json::from_value(frame[0].clone()).unwrap();
                let target = frame[1].as_bool().unwrap().then(|| {
                    (
                        serde_json::from_value(frame[2].clone()).unwrap(),
                        frame[3].as_f64().unwrap(),
                    )
                });
                control.update(pos, target, row[0].as_f64().unwrap());
                assert_eq!(
                    json!([
                        control.yaw.to_bits(),
                        control.ai_speed.to_bits(),
                        control.forward.to_bits(),
                        control.jump
                    ]),
                    json!([frame[4], frame[5], frame[6], frame[7]]),
                    "move {i}, tick {t}"
                );
                count += 1;
            }
        }
        assert_eq!(count, 4096);
    }
}
