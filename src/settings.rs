use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MenuSettings {
    pub realms_notifications: bool,
    pub skin_parts: u8,
    /// Music, records, weather, blocks, hostile, neutral, players and ambient.
    pub sound_levels: [f32; 8],
    pub chat_visibility: u8,
    pub chat_colors: bool,
    pub chat_links: bool,
    pub chat_links_prompt: bool,
    pub chat_opacity: f32,
    pub chat_scale: f32,
    pub chat_height_focused: f32,
    pub chat_height_unfocused: f32,
    pub chat_width: f32,
    pub reduced_debug: bool,
    pub force_unicode: bool,
    pub snooper: bool,
    pub hide_server_address: bool,
    /// User-selected priority order, highest first, as shown by GuiScreenResourcePacks.
    pub resource_packs: Vec<String>,
}
impl Default for MenuSettings {
    fn default() -> Self {
        Self {
            realms_notifications: true, skin_parts: 127, sound_levels: [1.; 8],
            chat_visibility: 0, chat_colors: true, chat_links: true, chat_links_prompt: true,
            chat_opacity: 1., chat_scale: 1., chat_height_focused: 1., chat_height_unfocused: 0.44366196,
            chat_width: 1., reduced_debug: false, force_unicode: false, snooper: true, hide_server_address: false, resource_packs: Vec::new(),
        }
    }
}
impl MenuSettings {
    fn validate(&mut self) {
        self.skin_parts &= 127;
        self.chat_visibility = self.chat_visibility.min(2);
        for value in self.sound_levels.iter_mut().chain([
            &mut self.chat_opacity, &mut self.chat_scale, &mut self.chat_height_focused,
            &mut self.chat_height_unfocused, &mut self.chat_width]) {
            *value = if value.is_finite() { value.clamp(0., 1.) } else { 1. };
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub menu: MenuSettings,
    #[serde(skip)]
    pub locale_unicode: bool,
    pub language: String,
    pub render_distance: u32,
    pub fancy: bool,
    pub smooth_lighting: u8,
    pub max_fps: u32,
    pub anaglyph: bool,
    pub bobbing: bool,
    pub gui_scale: u8,
    pub gamma: f32,
    pub clouds: u8,
    pub particles: u8,
    pub fullscreen: bool,
    pub vsync: bool,
    pub mipmaps: u8,
    pub alternate_blocks: bool,
    pub entity_shadows: bool,
    pub entity_culling: bool,
    pub entity_distance: u32,
    pub chunk_uploads: u32,
    pub greedy_meshing: bool,
    pub fov: f32,
    pub sensitivity: f32,
    pub invert_mouse: bool,
    pub touchscreen: bool,
    pub bindings: crate::keybindings::Bindings,
    pub volume: f32,
    pub difficulty: u8,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            menu: MenuSettings::default(),
            locale_unicode: false,
            language: "ko_kr".into(),
            render_distance: 8,
            fancy: true,
            smooth_lighting: 2,
            max_fps: 0,
            anaglyph: false,
            bobbing: true,
            gui_scale: 0,
            gamma: 0.5,
            clouds: 2,
            particles: 0,
            fullscreen: false,
            vsync: false,
            mipmaps: 4,
            alternate_blocks: true,
            entity_shadows: true,
            entity_culling: true,
            entity_distance: 96,
            chunk_uploads: 4,
            greedy_meshing: true,
            fov: 70.0,
            sensitivity: 0.5,
            invert_mouse: false,
            touchscreen: false,
            bindings: Default::default(),
            volume: 1.,
            difficulty: 2,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    Graphics,
    Distance,
    Lighting,
    Fps,
    Anaglyph,
    Bobbing,
    GuiScale,
    Brightness,
    Clouds,
    Particles,
    Fullscreen,
    Vsync,
    Mipmaps,
    Alternatives,
    Shadows,
    Culling,
    EntityDistance,
    Uploads,
    Greedy,
    Fov,
    Sensitivity,
    Volume,
    Difficulty,
}
pub const VIDEO: [Setting; 19] = [
    Setting::Graphics,
    Setting::Distance,
    Setting::Lighting,
    Setting::Fps,
    Setting::Anaglyph,
    Setting::Bobbing,
    Setting::GuiScale,
    Setting::Brightness,
    Setting::Clouds,
    Setting::Particles,
    Setting::Fullscreen,
    Setting::Vsync,
    Setting::Mipmaps,
    Setting::Alternatives,
    Setting::Shadows,
    Setting::Culling,
    Setting::EntityDistance,
    Setting::Uploads,
    Setting::Greedy,
];
impl Settings {
    /// Distant LOD extends visibility, never the original active gameplay area.
    /// Existing 2..24 settings keep precisely their previous full block range.
    pub fn full_detail_distance(&self) -> u32 { self.render_distance.min(24) }
    pub fn load() -> Result<Self> {
        let p = Path::new("options.json");
        let mut s = if p.is_file() {
            serde_json::from_slice(&std::fs::read(p)?)?
        } else {
            Self::default()
        };
        s.validate();
        Ok(s)
    }
    pub fn validate(&mut self) {
        self.menu.validate();
        self.render_distance = self.render_distance.clamp(2, 128);
        self.smooth_lighting = self.smooth_lighting.min(2);
        self.max_fps = self.max_fps.min(1000);
        self.gui_scale = self.gui_scale.min(4);
        self.gamma = if self.gamma.is_finite() {
            self.gamma.clamp(0.0, 1.0)
        } else {
            0.5
        };
        self.clouds = self.clouds.min(2);
        self.particles = self.particles.min(2);
        self.mipmaps = self.mipmaps.min(4);
        self.entity_distance = self.entity_distance.clamp(16, 192);
        self.chunk_uploads = self.chunk_uploads.clamp(1, 16);
        self.fov = if self.fov.is_finite() {
            self.fov.clamp(30.0, 110.0)
        } else {
            70.0
        };
        self.sensitivity = if self.sensitivity.is_finite() {
            self.sensitivity.clamp(0.0, 1.0)
        } else {
            0.5
        };
        self.volume = if self.volume.is_finite() {
            self.volume.clamp(0.0, 1.0)
        } else {
            1.
        };
        self.difficulty = self.difficulty.min(3);
    }
    pub fn save_to(&self, path:&Path) -> Result<()> {
        crate::world::atomic_json(path, self)
    }
    pub fn scale(&self, width: u32, height: u32) -> f32 {
        let max = if self.gui_scale == 0 {
            4
        } else {
            self.gui_scale as u32
        };
        let mut scale = 1;
        while scale < max && width / (scale + 1) >= 320 && height / (scale + 1) >= 240 {
            scale += 1;
        }
        if scale > 1 && scale % 2 != 0 && (self.menu.force_unicode || self.locale_unicode) { scale -= 1; }
        scale as f32
    }
    pub fn cycle(&mut self, s: Setting, reverse: bool) {
        let dir = if reverse { -1 } else { 1 };
        fn cycle(v: &mut u8, max: u8, d: i32) {
            *v = (*v as i32 + d).rem_euclid(max as i32 + 1) as u8;
        }
        fn listed(v: &mut u32, list: &[u32], d: i32) {
            let i = list.iter().position(|x| x == v).unwrap_or(0);
            *v = list[(i as i32 + d).rem_euclid(list.len() as i32) as usize];
        }
        match s {
            Setting::Graphics => self.fancy = !self.fancy,
            Setting::Distance => listed(
                &mut self.render_distance,
                &[2, 4, 6, 8, 10, 12, 16, 20, 24, 32, 48, 64, 96, 128],
                dir,
            ),
            Setting::Lighting => cycle(&mut self.smooth_lighting, 2, dir),
            Setting::Fps => listed(
                &mut self.max_fps,
                &[0, 30, 60, 90, 120, 144, 165, 240, 360, 500],
                dir,
            ),
            Setting::Anaglyph => self.anaglyph = !self.anaglyph,
            Setting::Bobbing => self.bobbing = !self.bobbing,
            Setting::GuiScale => cycle(&mut self.gui_scale, 4, dir),
            Setting::Brightness => self.gamma = (self.gamma + 0.25 * dir as f32).rem_euclid(1.25),
            Setting::Clouds => cycle(&mut self.clouds, 2, dir),
            Setting::Particles => cycle(&mut self.particles, 2, dir),
            Setting::Fullscreen => self.fullscreen = !self.fullscreen,
            Setting::Vsync => self.vsync = !self.vsync,
            Setting::Mipmaps => cycle(&mut self.mipmaps, 4, dir),
            Setting::Alternatives => self.alternate_blocks = !self.alternate_blocks,
            Setting::Shadows => self.entity_shadows = !self.entity_shadows,
            Setting::Culling => self.entity_culling = !self.entity_culling,
            Setting::EntityDistance => {
                listed(&mut self.entity_distance, &[32, 64, 96, 128, 192], dir)
            }
            Setting::Uploads => listed(&mut self.chunk_uploads, &[1, 2, 4, 8, 16], dir),
            Setting::Greedy => self.greedy_meshing = !self.greedy_meshing,
            Setting::Fov => {
                self.fov = 30.0 + (self.fov - 30.0 + 10.0 * dir as f32).rem_euclid(90.0)
            }
            Setting::Sensitivity => {
                self.sensitivity = 0.1 + (self.sensitivity - 0.1 + 0.1 * dir as f32).rem_euclid(1.0)
            }
            Setting::Volume => self.volume = (self.volume + 0.1 * dir as f32).rem_euclid(1.1),
            Setting::Difficulty => cycle(&mut self.difficulty, 3, dir),
        }
        self.validate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_round_trip_and_limits() {
        let mut s = Settings {
            render_distance: 999,
            fov: f32::NAN,
            ..Default::default()
        };
        s.validate();
        assert_eq!(s.render_distance, 128);
        assert_eq!(s.full_detail_distance(), 24);
        assert_eq!(s.fov, 70.0);
        s.cycle(Setting::Distance, false);
        assert_eq!(s.render_distance, 2);
        let bytes = serde_json::to_vec(&s).unwrap();
        let decoded: Settings = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.language, "ko_kr");
        assert!(decoded.scale(1280, 720) >= 2.0);
    }
}
