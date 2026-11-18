//! In protocol 47 a rendered Wither/Dragon sets the one shared BossStatus, even when its skin is invisible.
use glam::{DVec3, IVec3, Vec3};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Boss {
    pub kind: u8,
    pub name: Arc<str>,
    pub health: f32,
    pub max: f32,
}
impl Boss {
    pub fn new(kind: u8) -> Option<Self> {
        let max = match kind {
            63 => 200.,
            64 => 300.,
            _ => return None,
        };
        Some(Self {
            kind,
            name: "".into(),
            health: max,
            max,
        })
    }
    pub fn name_key(&self) -> &'static str {
        if self.kind == 64 {
            "entity.WitherBoss.name"
        } else {
            "entity.EnderDragon.name"
        }
    }
    pub fn range(&self, dist2: f64) -> bool {
        let width = if self.kind == 64 {
            f64::from(0.9f32)
        } else {
            16.
        };
        let height = if self.kind == 64 { 3.5 } else { 8. };
        let radius = (2. * width + height) / 3. * 64.;
        dist2 < radius * radius
    }
    pub fn should_render(&self, dist2: f64, frustum: bool) -> bool {
        self.range(dist2) && (self.kind == 63 || frustum)
    }
    pub fn width(&self) -> i32 {
        (self.health / self.max * 183.) as i32
    }
}
#[derive(Clone, Debug)]
pub struct Candidate {
    pub id: i32,
    pub section: IVec3,
    pub order: u64,
    pub at: DVec3,
    pub boss: Boss,
}
impl Candidate {
    pub fn local(mob: &crate::gameplay::Mob, sim: &crate::gameplay::Simulation) -> Option<Self> {
        let mut boss = Boss::new(mob.kind.vanilla_id())?;
        boss.health = mob.health;
        boss.max = mob.kind.max_health(sim.command_entities.mobs.get(&mob.id));
        if let Some(data) = sim.command_entities.mobs.get(&mob.id) {
            boss.name = data.name.as_str().into();
        }
        let at = mob.position.as_dvec3();
        let [x, z, y] = crate::vanilla::entity_pick::section(at);
        Some(Self { id: mob.id as i32, section: IVec3::new(x,y,z), order: mob.id, at, boss })
    }
    pub fn visible(&self, camera: DVec3, frustum: &crate::mesh::Frustum) -> bool {
        let (w, h) = if self.boss.kind == 64 {
            (0.9f32, 3.5)
        } else {
            (16., 8.)
        };
        let d = Vec3::new(w / 2., 0., w / 2.);
        let at = self.at.as_vec3();
        self.boss.should_render(
            self.at.distance_squared(camera),
            frustum.visible(at - d, at + d + Vec3::Y * h),
        )
    }
}
#[derive(Default)]
pub struct Bar {
    boss: Option<Boss>,
    remaining: u8,
    pub draw: bool,
    pending_tint: bool,
    pub previous_tint: f32,
    pub tint: f32,
}
impl Bar {
    pub fn tick(&mut self) {
        self.previous_tint = self.tint;
        if std::mem::take(&mut self.pending_tint) {
            self.tint = (self.tint + 0.05).min(1.);
        } else if self.tint > 0. {
            self.tint -= 0.0125;
        }
    }
    pub fn fog(&self, mut color: [f32; 3], alpha: f32) -> [f32; 3] {
        if self.tint > 0. {
            let blend = self.previous_tint + (self.tint - self.previous_tint) * alpha;
            for (c, factor) in color.iter_mut().zip([0.7, 0.6, 0.6]) {
                *c = *c * (1. - blend) + *c * factor * blend;
            }
        }
        color
    }
    pub fn frame(&mut self, observed: Option<&Boss>, overlay: bool) {
        if let Some(boss) = observed {
            self.boss = Some(boss.clone());
            self.pending_tint = boss.kind == 64;
            self.remaining = 100;
        }
        self.draw = overlay && self.remaining > 0;
        if self.draw {
            self.remaining -= 1;
        }
    }
    pub fn current(&self) -> Option<&Boss> {
        self.draw.then_some(self.boss.as_ref()).flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires the installed 1.8.9 UI atlases"]
    fn original_boss_quads_through_live_gui_painter() {
        use crate::{gameplay::Simulation, settings::Settings, ui};
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/vanilla189-boss-bar.json"))
                .unwrap();
        let resources = crate::validation::resources();
        let assets = crate::validation::assets();
        let mut settings = Settings::default();
        settings.gui_scale = 2;
        let mut gui = ui::Gui::default();
        gui.screen = ui::Screen::Playing;
        let sim = Simulation::default();
        for row in data["frames"].as_array().unwrap() {
            let mut b = Boss::new(row[0].as_u64().unwrap() as u8).unwrap();
            b.name = "§5Plugin §aBoss".into();
            b.max = 200.;
            b.health = row[1].as_f64().unwrap() as f32;
            gui.boss_bar.frame(Some(&b), true);
            let frame = ui::build(
                &gui,
                &settings,
                resources,
                assets,
                [1600, 900],
                Some(&sim),
                &Default::default(),
            );
            let layer = resources.sprite("textures/gui/icons.png").layer;
            let quads: Vec<_> = frame
                .vertices
                .chunks_exact(6)
                .filter_map(|tri| {
                    if tri[0].layer != layer {
                        return None;
                    }
                    let left = tri
                        .iter()
                        .map(|v| ((v.position[0] + 1.) * frame.size[0] / 2.).round() as i32)
                        .min()?;
                    let right = tri
                        .iter()
                        .map(|v| ((v.position[0] + 1.) * frame.size[0] / 2.).round() as i32)
                        .max()?;
                    let top = tri
                        .iter()
                        .map(|v| ((1. - v.position[1]) * frame.size[1] / 2.).round() as i32)
                        .min()?;
                    let bottom = tri
                        .iter()
                        .map(|v| ((1. - v.position[1]) * frame.size[1] / 2.).round() as i32)
                        .max()?;
                    let u = tri.iter().map(|v| (v.uv[0] * 256.).round() as i32).min()?;
                    let v = tri.iter().map(|v| (v.uv[1] * 256.).round() as i32).min()?;
                    (top == 12).then_some([left, top, u, v, right - left, bottom - top])
                })
                .collect();
            assert_eq!(serde_json::json!(quads), row[4]);
        }
    }
    #[test]
    fn original_invisible_wither_and_dragon_render_status_and_100_gui_frames() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/vanilla189-boss-bar.json"))
                .unwrap();
        for case in v["visibility"].as_array().unwrap() {
            let boss = Boss::new(case[0].as_u64().unwrap() as u8).unwrap();
            let d = case[2].as_f64().unwrap();
            assert_eq!(
                boss.should_render(d * d, case[1].as_bool().unwrap()),
                case[3].as_bool().unwrap()
            );
        }
        for case in v["frames"].as_array().unwrap() {
            let mut boss = Boss::new(case[0].as_u64().unwrap() as u8).unwrap();
            boss.health = case[1].as_f64().unwrap() as f32;
            boss.max = 200.;
            boss.name = "§5Plugin §aBoss".into();
            assert_eq!(format!("{}§r", boss.name), case[2].as_str().unwrap());
            let mut hud = Bar::default();
            let width = v["width"].as_i64().unwrap() as i32;
            let x = width / 2 - 91;
            for frame in 0..102 {
                hud.frame((frame == 0).then_some(&boss), true);
                let mut rows = vec![];
                if let Some(b) = hud.current() {
                    rows.extend([[x, 12, 0, 74, 182, 5]; 2]);
                    if b.width() > 0 {
                        rows.push([x, 12, 0, 79, b.width(), 5]);
                    }
                }
                if frame == 0 {
                    assert_eq!(serde_json::json!(rows), case[4]);
                }
                if frame >= 99 {
                    assert_eq!(serde_json::json!(rows.len()), case[5][frame - 99]);
                }
            }
        }
    }
}
