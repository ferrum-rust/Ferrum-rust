use crate::gameplay::{HurtCamera, Simulation};
use glam::Mat4;

#[derive(Clone, Copy)]
pub(super) struct HurtView {
    pub rotation: Mat4,
    fov_divisor: f32,
}

impl HurtView {
    pub fn for_camera(sim: &Simulation, partial: f32) -> Self {
        let (state, dead) = sim.server_scene.camera_hurt.unwrap_or((sim.hurt_camera, sim.health <= 0.));
        Self::sample(state, dead, partial)
    }

    fn sample(state: HurtCamera, dead: bool, partial: f32) -> Self {
        let mut degrees = 0.;
        let mut fov_divisor = 1.;
        if dead {
            let age = f32::from(state.death_time) + partial;
            degrees = 40. - 8000. / (age + 200.);
            fov_divisor = (1. - 500. / (age + 500.)) * 2. + 1.;
        }
        // In the 1.8.9 client, hurt status and animation set attackedAtYaw to zero.
        if state.hurt_time > 0 {
            let phase = (f32::from(state.hurt_time) - partial) / 10.;
            degrees -= 14. * crate::vanilla::math::sin(phase * phase * phase * phase * std::f32::consts::PI);
        }
        Self { rotation: Mat4::from_rotation_z(degrees.to_radians()), fov_divisor }
    }

    pub fn fov(self, degrees: f32) -> f32 {
        degrees / self.fov_divisor
    }

    /// The hand has its own FOV. Rotate in eye space before applying the viewport's aspect ratio.
    pub fn hand_view(self, underwater: bool) -> [f32; 4] {
        let fov = self.fov(if underwater { 60. } else { 70. });
        let cot = 1. / (0.5 * fov.to_radians()).tan();
        let (x, y) = (self.rotation.x_axis, self.rotation.y_axis);
        [x.x * cot, y.x * cot, x.y * cot, y.y * cot]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Frame {
        hurt: u8,
        death: u8,
        health: f32,
        partial: f32,
        fov_scale: f32,
        effect: [f32; 16],
        world_base: [f32; 16],
        world: [f32; 16],
        hand: Option<[f32; 16]>,
    }

    #[test]
    fn original_189_hurt_camera_world_and_hand_transforms() {
        let samples: Vec<Frame> = serde_json::from_reader(flate2::read::GzDecoder::new(
            &include_bytes!("../tests/fixtures/vanilla189-hurt-camera.json.gz")[..],
        )).unwrap();
        assert_eq!(samples.len(), 120);
        for frame in samples {
            let state = HurtCamera { hurt_time: frame.hurt, death_time: frame.death };
            let view = HurtView::sample(state, frame.health <= 0., frame.partial);
            assert!(view.rotation.abs_diff_eq(Mat4::from_cols_array(&frame.effect), 0.000002));
            let actual_world = view.rotation * Mat4::from_cols_array(&frame.world_base);
            assert!(actual_world.abs_diff_eq(Mat4::from_cols_array(&frame.world), 0.000002));
            assert!((view.fov(70.) / 70. - frame.fov_scale).abs() < 0.000001);
            if let Some(hand) = frame.hand {
                let cot = 1. / ((70. * frame.fov_scale).to_radians() * 0.5).tan();
                let expected = [hand[0], hand[4], hand[1], hand[5]];
                for (actual, rotation) in view.hand_view(false).into_iter().zip(expected) {
                    assert!((actual - cot * rotation).abs() < 0.000002);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "hurt_view_gpu_tests.rs"]
mod gpu_tests;
