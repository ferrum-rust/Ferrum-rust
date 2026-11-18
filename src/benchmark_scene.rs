use crate::settings::Settings;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};

/// Shared by the local Java reference runner and the native window benchmark.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub name: String,
    pub seed: u64,
    pub player_feet: [f64; 3],
    pub yaw_degrees: f32,
    pub pitch_degrees: f32,
    pub fov_degrees: f32,
    pub time: u64,
    pub resolution: [u32; 2],
    pub render_distance: u32,
    pub fancy: bool,
    pub smooth_lighting: u8,
    pub mipmap_levels: u8,
    pub gamma: f32,
    pub clouds: bool,
    pub hud: bool,
    pub vsync: bool,
    pub warmup_seconds: f32,
    pub measure_seconds: f32,
}
impl Scene {
    pub fn load(path: &str) -> Result<Self> {
        let bytes =
            std::fs::read(path).with_context(|| format!("Opening benchmark scene {path}"))?;
        let scene: Self = serde_json::from_slice(&bytes)?;
        ensure!(
            !scene.name.is_empty()
                && crate::player_save::valid_position(scene.player_feet)
                && (-90.0..=90.0).contains(&scene.pitch_degrees)
                && scene.yaw_degrees.is_finite()
                && (30.0..=110.0).contains(&scene.fov_degrees),
            "Invalid benchmark camera"
        );
        ensure!(
            (640..=8192).contains(&scene.resolution[0])
                && (480..=8192).contains(&scene.resolution[1])
                && (2..=24).contains(&scene.render_distance)
                && scene.smooth_lighting <= 2
                && scene.mipmap_levels <= 4
                && (0.0..=1.0).contains(&scene.gamma),
            "Invalid benchmark graphics settings"
        );
        ensure!(
            scene.warmup_seconds.is_finite()
                && scene.warmup_seconds >= 0.0
                && scene.measure_seconds.is_finite()
                && scene.measure_seconds > 0.0,
            "Invalid benchmark duration"
        );
        Ok(scene)
    }
    pub fn settings(&self) -> Settings {
        Settings {
            render_distance: self.render_distance,
            fancy: self.fancy,
            smooth_lighting: self.smooth_lighting,
            fov: self.fov_degrees,
            mipmaps: self.mipmap_levels,
            gamma: self.gamma,
            clouds: if self.clouds { 2 } else { 0 },
            vsync: self.vsync,
            difficulty: 0,
            bobbing: false,
            particles: 2,
            alternate_blocks: true,
            ..Settings::default()
        }
    }
}
