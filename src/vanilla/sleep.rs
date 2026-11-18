//! Surface-world bed placement, sleeping and waking.
use super::{
    hand_placement::{replaceable_for_click, Access},
    lightmap, math,
    respawn::BedSpawn,
    spawning::solid_top,
};
use crate::{
    gameplay::{Outcome, Simulation},
    player::Player,
    world::World,
};
use glam::{DVec3, IVec3, Vec3};
use serde::{Deserialize, Serialize};

const FACING: [IVec3; 4] = [IVec3::Z, IVec3::NEG_X, IVec3::NEG_Z, IVec3::X];
pub(crate) fn other_half(p:IVec3,state:u16)->IVec3 {
    let offset=FACING[(state&3) as usize];
    if state&8==0 {p+offset} else {p-offset}
}
pub fn bed_placement(
    world: &impl Access,
    clicked: IVec3,
    face: u8,
    yaw: f32,
) -> Option<[(IVec3, u16); 2]> {
    if face != 1 {
        return None;
    }
    let replaces = replaceable_for_click(world, clicked);
    let foot = if replaces {
        clicked
    } else {
        clicked + IVec3::Y
    };
    let orientation = ((f64::from(yaw * 4. / 360.) + 0.5).floor() as i32 & 3) as usize;
    let head = foot + FACING[orientation];
    if !(replaces || world.get(foot) == 0)
        || !(replaceable_for_click(world, head) || world.get(head) == 0)
        || !solid_top(world.get(foot - IVec3::Y))
        || !solid_top(world.get(head - IVec3::Y))
    {
        return None;
    }
    let state = (26 << 4) | orientation as u16;
    Some([(foot, state), (head, state | 8)])
}
pub fn daytime(time: i64, rain: f32, thunder: f32) -> bool {
    let angle = lightmap::celestial_angle(time, 1., 0);
    let mut light =
        1. - (1. - (math::cos(angle * std::f32::consts::PI * 2.) * 2. + 0.5)).clamp(0., 1.);
    light = (f64::from(light) * (1. - f64::from(rain * 5.) / 16.)) as f32;
    light = (f64::from(light) * (1. - f64::from(thunder * rain * 5.) / 16.)) as f32;
    (((1. - light) * 11.) as i32) < 4
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Sleep {
    pub head: Option<IVec3>,
    pub timer: i32,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Attempt {
    Asleep,
    Occupied,
    Daytime,
    TooFar,
    Unsafe,
    Unavailable,
    Exploded,
}
impl Attempt {
    pub fn notice(&self) -> Option<&'static str> {
        match self {
            Self::Occupied => Some("tile.bed.occupied"),
            Self::Daytime => Some("tile.bed.noSleep"),
            Self::Unsafe => Some("tile.bed.notSafe"),
            _ => None,
        }
    }
}
pub(crate) fn resting_position(p: IVec3, direction: u16) -> [f64; 3] {
    let offset = match direction & 3 {
        0 => Vec3::new(0.5, 0.6875, 0.9),
        1 => Vec3::new(0.1, 0.6875, 0.5),
        2 => Vec3::new(0.5, 0.6875, 0.1),
        _ => Vec3::new(0.9, 0.6875, 0.5),
    };
    (p.as_vec3() + offset).as_dvec3().to_array()
}
/// RenderPlayer anchors the sideways model near the foot of the bed.
pub(crate) fn body_transform(feet: Vec3, state: u16) -> glam::Mat4 {
    let facing = (state & 3) as usize;
    let origin = feet - FACING[facing].as_vec3() * 1.8;
    let yaw = [90_f32, 0., 270., 180.][facing].to_radians();
    glam::Mat4::from_translation(origin)
        * glam::Mat4::from_rotation_y(yaw)
        * glam::Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2)
        * glam::Mat4::from_rotation_y(270_f32.to_radians())
}
pub(crate) fn wake_position(world: &World, bed: BedSpawn) -> [f64; 3] {
    let exit = bed.exit(world).unwrap_or(bed.point + IVec3::Y);
    (exit.as_vec3() + Vec3::new(0.5, 0.1, 0.5)).as_dvec3().to_array()
}
impl Sleep {
    pub(crate) fn tick_timer(&mut self) {
        if self.head.is_some() {
            self.timer = (self.timer + 1).min(100);
        } else if self.timer > 0 {
            self.timer += 1;
            if self.timer >= 110 { self.timer = 0; }
        }
    }
}
impl Simulation {
    pub fn try_bed(
        &mut self,
        world: &mut World,
        player: &mut Player,
        clicked: IVec3,
    ) -> (Attempt, Option<IVec3>) {
        let settings = crate::settings::Settings {
            difficulty: self.world_difficulty.unwrap_or(2),
            ..crate::settings::Settings::default()
        };
        let mut out = Outcome::default();
        let res = self.try_bed_with_outcome(world, player, clicked, &settings, &mut out);
        if !out.changed.is_empty() {
            self.pending_changed_blocks.extend(out.changed);
        }
        if !out.world_sounds.is_empty() {
            self.pending_world_sounds.extend(out.world_sounds);
        }
        res
    }
    pub(crate) fn try_bed_with_outcome(
        &mut self,
        world: &mut World,
        player: &mut Player,
        clicked: IVec3,
        settings: &crate::settings::Settings,
        out: &mut Outcome,
    ) -> (Attempt, Option<IVec3>) {
        let mut p = clicked;
        let mut state = world.get(p);
        if state >> 4 != 26 {
            return (Attempt::Unavailable, None);
        }
        if state & 8 == 0 {
            p += FACING[(state & 3) as usize];
            state = world.get(p);
            if state >> 4 != 26 {
                return (Attempt::Unavailable, None);
            }
        }
        if self.dimension != 0 || crate::assets::biome_colors::biome_at(world, p) == 8 {
            if world.set(p, 0) {
                out.changed.push(p);
                self.block_ticks.notify(world, p);
            }
            let foot = p - FACING[(state & 3) as usize];
            if world.get(foot) >> 4 == 26 && world.set(foot, 0) {
                out.changed.push(foot);
                self.block_ticks.notify(world, foot);
            }
            let blast_pos = p.as_dvec3() + DVec3::splat(0.5);
            self.explode_with_fire(
                world,
                player,
                settings,
                blast_pos,
                5.0,
                true,
                false,
                true,
                None,
                out,
            );
            return (Attempt::Exploded, Some(p));
        }
        if self.sleep.head == Some(p) && state & 4 != 0 {
            return (Attempt::Occupied, None);
        }
        let cleared = if state & 4 != 0 {
            state &= !4;
            world.set(p, state).then_some(p)
        } else {
            None
        };
        if self.health <= 0. || self.sleep.head.is_some() {
            return (Attempt::Unavailable, cleared);
        }
        let (rain, thunder) = self.weather.strengths(1.);
        if daytime(self.clock as i64, rain, thunder) {
            return (Attempt::Daytime, cleared);
        }
        let delta = (DVec3::from_array(player.precise_position()) - p.as_dvec3()).abs();
        if delta.x > 3. || delta.y > 2. || delta.z > 3. {
            return (Attempt::TooFar, cleared);
        }
        let center = p.as_dvec3();
        let danger_min = center - DVec3::new(8., 5., 8.);
        let danger_max = center + DVec3::new(8., 5., 8.);
        for mob in self
            .mobs
            .iter()
            .filter(|m| m.kind.hostile() && m.health > 0.)
        {
            let (w, h) = mob.kind.adult_size(self.command_entities.mobs.get(&mob.id));
            let scale = if self.is_baby(mob.id) { 0.5 } else { 1. };
            let half = f64::from(w * scale * 0.5);
            let pos = mob.position.as_dvec3();
            if (pos + DVec3::new(half, f64::from(h * scale), half))
                .cmpgt(danger_min)
                .all()
                && (pos - DVec3::new(half, 0., half)).cmplt(danger_max).all()
            {
                return (Attempt::Unsafe, cleared);
            }
        }
        self.sleep = Sleep {
            head: Some(p),
            timer: 0,
        };
        player.sleeping = true;
        player.set_position_f64(resting_position(p, state));
        player.set_velocity_f64([0.; 3]);
        world.set(p, state | 4);
        (Attempt::Asleep, Some(p))
    }
    pub fn wake(
        &mut self,
        world: &mut World,
        player: &mut Player,
        immediate: bool,
        set_spawn: bool,
    ) -> Option<IVec3> {
        let p = self.sleep.head.take()?;
        player.sleeping = false;
        self.sleep.timer = if immediate { 0 } else { 100 };
        let bed = BedSpawn {
            point: p,
            forced: false,
        };
        let state = world.get(p);
        let mut changed = None;
        if state >> 4 == 26 {
            changed = world.set(p, state & !4).then_some(p);
            player.set_position_f64(wake_position(world, bed));
        }
        if set_spawn {
            self.bed_spawn = Some(bed);
        }
        changed
    }
    pub(crate) fn tick_sleep(&mut self, world: &mut World, player: &mut Player, out: &mut Outcome) {
        if self.sleep.head.is_some() && self.sleep.timer >= 100 {
            if self.daylight_cycle {
                let next = self.clock.wrapping_add(24000);
                self.clock = next - next % 24000;
            }
            out.changed.extend(self.wake(world, player, false, true));
            self.weather.clear_after_sleep();
        }
        let (rain, thunder) = self.weather.strengths(1.);
        let day = daytime(self.clock as i64, rain, thunder);
        if self.daylight_cycle {
            self.clock = self.clock.wrapping_add(1);
        }
        self.sleep.tick_timer();
        if let Some(p) = self.sleep.head {
            if world.get(p) >> 4 != 26 {
                out.changed.extend(self.wake(world, player, true, false));
            } else if day {
                out.changed.extend(self.wake(world, player, false, true));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        gameplay::{Mob, MobKind, Mode},
        world::Chunk,
    };
    use serde::Deserialize;
    use std::sync::Arc;
    #[derive(Deserialize)]
    struct Activation {
        i: usize,
        time: u64,
        start: [f64; 3],
        attempt: String,
        sleeping: bool,
        rest: [f64; 3],
        state: i32,
        wake: [f64; 3],
        spawn: Option<[i32; 3]>,
        timer: i32,
    }
    type Placement = (u8, f32, Vec<[i32; 4]>, bool, i32, Vec<[i32; 5]>);
    type CycleTick = (u64, bool, i32, Option<[i32; 3]>, i32);
    type Corpus = (
        Vec<u8>,
        Vec<Activation>,
        Vec<Placement>,
        Vec<Vec<CycleTick>>,
    );
    const HEAD: IVec3 = IVec3::new(15, 72, -16);
    fn grid() -> World {
        World {
            chunks: [(0, -2), (1, -2), (0, -1), (1, -1)]
                .into_iter()
                .map(|cp| (cp, Arc::new(Chunk::empty())))
                .collect(),
            spawn: HEAD.as_vec3(),
            label: "bed probe".into(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: true,
            light_revision: 0,
        }
    }
    fn setup_bed(world: &mut World, dir: usize, occupied: bool) -> IVec3 {
        for x in -2..=2 {
            for z in -2..=2 {
                world.set(HEAD + IVec3::new(x, -1, z), 16);
            }
        }
        let state = (26 << 4) | dir as u16;
        let foot = HEAD - FACING[dir];
        world.set(HEAD, state | 8 | if occupied { 4 } else { 0 });
        world.set(foot, state);
        foot
    }
    #[test]
    fn original_surface_day_night_bed_activation_wake_and_item_placement_match() {
        let (days, nights, placements, cycles): Corpus =
            serde_json::from_str(include_str!("../../tests/fixtures/vanilla189-sleep.json"))
                .unwrap();
        assert_eq!(
            (days.len(), nights.len(), placements.len()),
            (24000, 288, 1728)
        );
        for (tick, day) in days.into_iter().enumerate() {
            assert_eq!(daytime(tick as i64, 0., 0.), day != 0, "day tick {tick}");
        }
        for row in nights {
            let i = row.i;
            let dir = i % 4;
            let scenario = i / 4 % 12;
            let mode = i / 48;
            let mut world = grid();
            let foot = setup_bed(&mut world, dir, mode % 2 != 0);
            if scenario == 9 {
                world.set(HEAD, 0);
            }
            let mut p = Player::new(Vec3::ZERO);
            p.set_pose_f64(row.start, 0., 0., false);
            let mut sim = Simulation::new(Mode::Survival, 189);
            sim.clock = row.time;
            if scenario == 10 {
                sim.health = 0.;
            }
            if matches!(scenario, 7 | 8) {
                let position =
                    HEAD.as_vec3() + Vec3::new(if scenario == 8 { 8.5 } else { 7.5 }, 0., 0.5);
                sim.mobs.push(Mob {
                    id: 1,
                    kind: MobKind::Zombie,
                    position,
                    previous: position,
                    velocity: Vec3::ZERO,
                    yaw: 0.,
                    health: 20.,
                    age: 0,
                    turn_ticks: 0,
                    hurt: 0,
                    fuse: 0,
                    swing_ticks: 0,
                    death_ticks: 0,
                    eat_ticks: 0,
                    attack_ticks: 0,
                    rose_ticks: 0,
                    prev_fuse: 0,
                });
            }
            let click = if mode % 3 == 1 { foot } else { HEAD };
            let (result, _) = sim.try_bed(&mut world, &mut p, click);
            let matches = match row.attempt.as_str() {
                "NONE" | "OTHER_PROBLEM" => result == Attempt::Unavailable,
                "NOT_POSSIBLE_NOW" => result == Attempt::Daytime,
                "TOO_FAR_AWAY" => result == Attempt::TooFar,
                "NOT_SAFE" => result == Attempt::Unsafe,
                "OK" => result == Attempt::Asleep,
                _ => false,
            };
            assert!(matches, "case {i}: {result:?} vs {}", row.attempt);
            assert_eq!(p.sleeping, row.sleeping, "case {i}");
            assert_eq!(p.precise_position(), row.rest, "case {i} rest");
            let state = world.get(HEAD);
            assert_eq!(
                if state >> 4 == 26 {
                    (state & 15) as i32
                } else {
                    -1
                },
                row.state,
                "case {i} state"
            );
            if p.sleeping {
                if scenario == 11 {
                    world.set(HEAD, 0);
                }
                sim.wake(&mut world, &mut p, mode % 2 == 0, mode % 3 != 0);
            }
            assert_eq!(p.precise_position(), row.wake, "case {i} wake");
            assert_eq!(
                sim.bed_spawn.map(|b| b.point.to_array()),
                row.spawn,
                "case {i} new spawn"
            );
            assert_eq!(sim.sleep.timer, row.timer, "case {i} timer");
        }
        let mut world = grid();
        for (i, (face, yaw, cells, ok, count, writes)) in placements.into_iter().enumerate() {
            for [x, y, z, state] in cells {
                world.set(HEAD + IVec3::new(x, y, z), state as u16);
            }
            let parts = bed_placement(&world, HEAD, face, yaw);
            assert_eq!(parts.is_some(), ok, "place {i}");
            assert_eq!(3 - i32::from(parts.is_some()), count, "place {i} count");
            let actual: Vec<_> = parts
                .into_iter()
                .flatten()
                .map(|(p, s)| [p.x, p.y, p.z, i32::from(s), 3])
                .collect();
            assert_eq!(actual, writes, "place {i} writes");
        }
        assert_eq!(cycles.len(), 5);
        for (scenario, ticks) in cycles.into_iter().enumerate() {
            let mut world = grid();
            setup_bed(&mut world, 0, false);
            let mut sim = Simulation::new(Mode::Survival, 189);
            sim.clock = 15000;
            sim.daylight_cycle = scenario != 4;
            let mut p = Player::new(Vec3::new(15.5, 72., -15.5));
            assert_eq!(sim.try_bed(&mut world, &mut p, HEAD).0, Attempt::Asleep);
            assert_eq!(ticks.len(), 112);
            for (t, expected) in ticks.into_iter().enumerate() {
                if t == 18 {
                    match scenario {
                        1 => {
                            sim.wake(&mut world, &mut p, false, true);
                        }
                        2 => {
                            world.set(HEAD, 0);
                        }
                        3 => {
                            sim.clock = 6000;
                        }
                        _ => {}
                    }
                }
                sim.tick_sleep(&mut world, &mut p, &mut Outcome::default());
                let state = world.get(HEAD);
                let actual = (
                    sim.clock,
                    p.sleeping,
                    sim.sleep.timer,
                    sim.bed_spawn.map(|b| b.point.to_array()),
                    if state >> 4 == 26 {
                        i32::from(state & 15)
                    } else {
                        -1
                    },
                );
                assert_eq!(actual, expected, "night {scenario} tick {t}");
            }
        }
    }
}
