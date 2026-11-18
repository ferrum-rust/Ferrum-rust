//! Tick and render state for EntityLivingBase.updateDistance and ModelBiped.setRotationAngles.
use crate::vanilla::math::{cos, sin};
use glam::{DVec3, Mat4, Vec3};
use std::f32::consts::PI;

pub(super) fn wrap(angle: f32) -> f32 {
    let mut angle = angle % 360.;
    if angle >= 180. {
        angle -= 360.;
    } else if angle < -180. {
        angle += 360.;
    }
    angle
}
#[derive(Clone, Default, Debug)]
pub struct Animation {
    pub slim: bool,
    pub riding: bool,
    pub mount_yaw: Option<[f32; 2]>,
    pub swinging: bool,
    body: f32,
    previous_body: f32,
    head: f32,
    previous_head: f32,
    phase: f32,
    amplitude: f32,
    previous_amplitude: f32,
    age: i32,
}
#[derive(Clone, Copy)]
pub struct Frame {
    pub body: f32,
    pub head: f32,
    pub phase: f32,
    pub amplitude: f32,
    pub age: f32,
}
impl Animation {
    pub fn tick(&mut self, delta: DVec3, yaw: f32) {
        self.age = self.age.wrapping_add(1);
        self.previous_body = self.body;
        self.previous_head = self.head;
        self.head = yaw;
        self.previous_amplitude = self.amplitude;
        let squared = delta.x * delta.x + delta.z * delta.z;
        let speed = (squared.sqrt() as f32 * 4.).min(1.);
        self.amplitude += (speed - self.amplitude) * 0.4;
        self.phase += self.amplitude;
        let target = if self.swinging {
            yaw
        } else if squared as f32 > 0.0025000002 {
            crate::gameplay::ai::motion::atan2(delta.z, delta.x) as f32 * 180. / PI - 90.
        } else {
            self.body
        };
        self.body += wrap(target - self.body) * 0.3;
        let offset = wrap(yaw - self.body).clamp(-75., 75.);
        self.body = yaw - offset;
        if offset * offset > 2500. {
            self.body += offset * 0.2;
        }
        self.previous_body = self.body - wrap(self.body - self.previous_body);
        self.previous_head = self.head - wrap(self.head - self.previous_head);
    }
    pub fn frame(&self, alpha: f32) -> Frame {
        let interp = |a, b| a + wrap(b - a) * alpha;
        let head = interp(self.previous_head, self.head);
        let mut body = interp(self.previous_body, self.body);
        let mut relative = head - body;
        if let Some([last, current]) = self.mount_yaw {
            body = interp(last, current);
            relative = head - body;
            let offset = wrap(relative).clamp(-85., 85.);
            body = head - offset;
            if offset * offset > 2500. {
                body += offset * 0.2;
            }
        }
        Frame {
            body,
            head: relative,
            phase: self.phase - self.amplitude * (1. - alpha),
            amplitude: (self.previous_amplitude
                + (self.amplitude - self.previous_amplitude) * alpha)
                .min(1.),
            age: self.age as f32 + alpha,
        }
    }
}
#[derive(Clone, Copy, Default)]
pub struct Bone {
    pub pivot: Vec3,
    pub rotation: Vec3,
}
impl Bone {
    pub fn transform(self) -> Mat4 {
        Mat4::from_translation(self.pivot / 16.)
            * Mat4::from_rotation_z(self.rotation.z)
            * Mat4::from_rotation_y(self.rotation.y)
            * Mat4::from_rotation_x(self.rotation.x)
    }
}
pub fn root(feet: Vec3, yaw: f32) -> Mat4 {
    Mat4::from_translation(feet)
        * Mat4::from_rotation_y((180. - yaw).to_radians())
        * Mat4::from_scale(Vec3::new(-0.9375, -0.9375, 0.9375))
        * Mat4::from_translation(Vec3::Y * (-1.5078125))
}
impl Frame {
    pub fn bones(
        self,
        pitch: f32,
        swing: f32,
        slim: bool,
        sneak: bool,
        riding: bool,
        held: i32,
        bow: bool,
    ) -> [Bone; 6] {
        let phase = self.phase * 0.6662;
        let amplitude = self.amplitude;
        let mut head = Bone {
            pivot: Vec3::ZERO,
            rotation: Vec3::new(pitch / 57.295776, self.head / 57.295776, 0.),
        };
        let mut body = Bone::default();
        let mut right = Bone {
            pivot: Vec3::new(-5., if slim { 2.5 } else { 2. }, 0.),
            rotation: Vec3::X * (cos(phase + PI) * 2. * amplitude * 0.5),
        };
        let mut left = Bone {
            pivot: Vec3::new(5., right.pivot.y, 0.),
            rotation: Vec3::X * (cos(phase) * 2. * amplitude * 0.5),
        };
        let mut rleg = Bone {
            pivot: Vec3::new(-1.9, 12., 0.1),
            rotation: Vec3::X * (cos(phase) * 1.4 * amplitude),
        };
        let mut lleg = Bone {
            pivot: Vec3::new(1.9, 12., 0.1),
            rotation: Vec3::X * (cos(phase + PI) * 1.4 * amplitude),
        };
        if riding {
            right.rotation.x -= 0.62831855;
            left.rotation.x -= 0.62831855;
            rleg.rotation = Vec3::new(-1.2566371, 0.31415927, 0.);
            lleg.rotation = Vec3::new(-1.2566371, -0.31415927, 0.);
        }
        if held == 1 || held == 3 {
            right.rotation.x = right.rotation.x * 0.5 - 0.31415927 * held as f32;
        }
        if held == 3 {
            right.rotation.y = -0.5235988;
        }
        body.rotation.y = sin(swing.sqrt() * PI * 2.) * 0.2;
        right.pivot.z = sin(body.rotation.y) * 5.;
        right.pivot.x = -cos(body.rotation.y) * 5.;
        left.pivot.z = -right.pivot.z;
        left.pivot.x = -right.pivot.x;
        right.rotation.y += body.rotation.y;
        left.rotation.y = body.rotation.y;
        left.rotation.x += body.rotation.y;
        let t = 1. - swing;
        let t = t * t;
        let f = sin((1. - t * t) * PI);
        let g = sin(swing * PI) * -(head.rotation.x - 0.7) * 0.75;
        right.rotation.x =
            (f64::from(right.rotation.x) - (f64::from(f) * 1.2 + f64::from(g))) as f32;
        right.rotation.y += body.rotation.y * 2.;
        right.rotation.z += sin(swing * PI) * (-0.4);
        if sneak {
            body.rotation.x = 0.5;
            right.rotation.x += 0.4;
            left.rotation.x += 0.4;
            for b in [&mut rleg, &mut lleg] {
                b.pivot.z = 4.;
                b.pivot.y = 9.;
            }
            head.pivot.y = 1.;
        }
        let roll = cos(self.age * 0.09) * 0.05 + 0.05;
        let idle = sin(self.age * 0.067) * 0.05;
        right.rotation.z += roll;
        left.rotation.z -= roll;
        right.rotation.x += idle;
        left.rotation.x -= idle;
        if bow {
            right.rotation = Vec3::new(
                -1.5707964 + head.rotation.x + idle,
                -0.1 + head.rotation.y,
                roll,
            );
            left.rotation = Vec3::new(
                -1.5707964 + head.rotation.x - idle,
                0.1 + head.rotation.y + 0.4,
                -roll,
            );
        }
        [head, body, right, left, rleg, lleg]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_f5_tick_turns_and_132_rendered_model_matrices_and_limb_poses() {
        type Sample = ([f32; 6], [f32; 16], [[f32; 6]; 6]);
        type Tick = (
            i32,
            bool,
            [f64; 2],
            [f32; 3],
            [f32; 4],
            bool,
            i32,
            bool,
            [Sample; 3],
        );
        let rows: Vec<Tick> = serde_json::from_reader(flate2::read::GzDecoder::new(
            &include_bytes!("../tests/fixtures/vanilla189-third-person.json.gz")[..],
        ))
        .unwrap();
        let mut animation = Animation::default();
        for (tick, slim, [dx, dz], [yaw, pitch, swing], state, sneak, held, bow, samples) in rows {
            let near = |got: f32, expected: f32, what: &str| {
                assert!(
                    (got - expected).abs() < 4e-5,
                    "tick {tick}: {what}: {got} != {expected}"
                )
            };
            animation.swinging = swing > 0.;
            animation.tick(DVec3::new(dx, 0., dz), yaw);
            for (x, y) in [
                animation.body,
                animation.head,
                animation.phase,
                animation.amplitude,
            ]
            .into_iter()
            .zip(state)
            {
                near(x, y, "tick state");
            }
            for (a, (input, mat, parts)) in [0., 0.375, 1.].into_iter().zip(samples) {
                let frame = animation.frame(a);
                for (x, y) in [frame.phase, frame.amplitude, frame.age, frame.head, pitch]
                    .into_iter()
                    .zip(input)
                {
                    near(x, y, "RenderPlayer parameters");
                }
                for (x, y) in root(Vec3::ZERO, frame.body)
                    .to_cols_array()
                    .into_iter()
                    .zip(mat)
                {
                    near(x, y, "OpenGL body matrix");
                }
                let bones = frame.bones(pitch, input[5], slim, sneak, false, held, bow);
                for (bone, expected) in bones.into_iter().zip(parts) {
                    for (x, y) in bone
                        .pivot
                        .to_array()
                        .into_iter()
                        .chain(bone.rotation.to_array())
                        .zip(expected)
                    {
                        near(x, y, "rendered ModelPlayer pivot/rotation");
                    }
                }
            }
        }
    }
}
