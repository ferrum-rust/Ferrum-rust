//! Original Overworld BlockFire support, scheduling and BlockPortal.Size scan.
use super::*;
use crate::vanilla::rail_events::RailWorld;
#[cfg(test)]
#[path = "runtime_fire_tests.rs"]
mod tests;
const SIDES: [IVec3; 6] = [
    IVec3::NEG_Y,
    IVec3::Y,
    IVec3::NEG_Z,
    IVec3::Z,
    IVec3::NEG_X,
    IVec3::X,
];
pub(crate) trait FireView {
    fn get(&self, p: IVec3) -> u16;
    fn set(&mut self, p: IVec3, state: u16, flags: u8);
    fn solid_top(&self, p: IVec3) -> bool;
}
impl<T: RailWorld> FireView for T {
    fn get(&self, p: IVec3) -> u16 {
        RailWorld::get(self, p)
    }
    fn set(&mut self, p: IVec3, state: u16, flags: u8) {
        RailWorld::set(self, p, state, flags);
    }
    fn solid_top(&self, p: IVec3) -> bool {
        RailWorld::solid_top(self, p)
    }
}
pub(crate) trait FireWorld: FireView {
    fn end_dimension(&self) -> bool {
        false
    }
    fn start_fire_tick(&mut self, p: IVec3);
}
impl FireWorld for Access<'_> {
    fn start_fire_tick(&mut self, p: IVec3) {
        self.with(|_, b| {
            let delay = 30 + b.world_random.next_int(10);
            let delay = if b.ticks.immediate() { 1 } else { delay as u64 };
            b.ticks.schedule_block_update(p, 51, delay, 0);
        });
    }
}
impl FireWorld for structures::LoadedStructures<'_> {
    fn start_fire_tick(&mut self, p: IVec3) {
        let delay = 30 + self.random.next_int(10);
        let delay = if self.ticks.immediate() {
            1
        } else {
            delay as u64
        };
        self.ticks.schedule_block_update(p, 51, delay, 0);
    }
}
fn supported(w: &impl FireView, p: IVec3) -> bool {
    w.solid_top(p - IVec3::Y)
        || SIDES
            .iter()
            .any(|d| crate::vanilla::hand_placement::catches_fire(w.get(p + *d)))
}
fn interior(state: u16) -> bool {
    matches!(state >> 4, 0 | 51 | 90)
}
fn distance(w: &impl FireView, p: IVec3, dir: IVec3) -> i32 {
    let mut i = 0;
    while i < 22 {
        let at = p + dir * i;
        if !interior(w.get(at)) || w.get(at - IVec3::Y) >> 4 != 49 {
            break;
        }
        i += 1;
    }
    if w.get(p + dir * i) >> 4 == 49 {
        i
    } else {
        0
    }
}
struct Frame {
    bottom: IVec3,
    dir: IVec3,
    width: i32,
    height: i32,
    existing: i32,
}
fn frame(w: &impl FireView, p: IVec3, axis: u16) -> Option<Frame> {
    let side = if axis == 1 { IVec3::X } else { IVec3::NEG_Z };
    let dir = -side;
    let mut bottom = p;
    while bottom.y > p.y - 21 && bottom.y > 0 && interior(w.get(bottom - IVec3::Y)) {
        bottom.y -= 1;
    }
    let edge = distance(w, bottom, side) - 1;
    if edge < 0 {
        return None;
    }
    bottom += side * edge;
    let width = distance(w, bottom, dir);
    if !(2..=21).contains(&width) {
        return None;
    }
    let mut height = 0;
    let mut existing = 0;
    'height: while height < 21 {
        for i in 0..width {
            let at = bottom + dir * i + IVec3::Y * height;
            let state = w.get(at);
            if !interior(state) {
                break 'height;
            }
            if state >> 4 == 90 {
                existing += 1;
            }
            if i == 0 && w.get(at + side) >> 4 != 49 || i == width - 1 && w.get(at + dir) >> 4 != 49
            {
                break 'height;
            }
        }
        height += 1;
    }
    for i in 0..width {
        if w.get(bottom + dir * i + IVec3::Y * height) >> 4 != 49 {
            return None;
        }
    }
    (height >= 3).then_some(Frame {
        bottom,
        dir,
        width,
        height,
        existing,
    })
}
pub(crate) fn added(world: &mut impl FireWorld, p: IVec3, old: u16, new: u16) {
    if old >> 4 == 51 || new >> 4 != 51 {
        return;
    }
    let allow_portal = !world.end_dimension();
    for axis in [1, 2].into_iter().filter(|_| allow_portal) {
        if let Some(f) = frame(world, p, axis).filter(|f| f.existing == 0) {
            for i in 0..f.width {
                for y in 0..f.height {
                    world.set(f.bottom + f.dir * i + IVec3::Y * y, 90 << 4 | axis, 2);
                }
            }
            return;
        }
    }
    if supported(world, p) {
        world.start_fire_tick(p);
    } else {
        world.set(p, 0, 3);
    }
}
pub(crate) fn changed(world: &mut impl FireView, p: IVec3, state: u16) {
    match state >> 4 {
        51 if !supported(world, p) => world.set(p, 0, 3),
        90 if frame(world, p, state & 3).is_none_or(|f| f.existing < f.width * f.height) => {
            world.set(p, 0, 3);
        }
        _ => {}
    }
}

include!("runtime_fire_data.rs");

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct FireEnvironment {
    pub do_fire_tick: bool,
    /// Vanilla EnumDifficulty ID, 0 (peaceful) through 3 (hard).
    pub difficulty: u8,
    /// World.isRaining() after its rain-strength threshold.
    pub raining: bool,
}

impl Default for FireEnvironment {
    fn default() -> Self {
        Self {
            do_fire_tick: true,
            difficulty: 2,
            raining: false,
        }
    }
}

/// The original TNT constructor has run, including its Math.random draw. The
/// session can checkpoint this exact boundary before the entity/explosion phase.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PendingTnt {
    pub id: i32,
    pub position: glam::DVec3,
    pub velocity: glam::DVec3,
    pub fuse: u8,
    pub entity_random: u64,
    pub insertion_index: usize,
}

pub(super) fn raining_at(world: &Access<'_>, p: IVec3) -> bool {
    use crate::vanilla::plant_events::PlantWorld;
    if !world.with(|_, b| b.fire_environment.raining)
        || !PlantWorld::can_see_sky(world, p)
        || FeatureWorld::precipitation_height(world, p.x, p.z) > p.y
    {
        return false;
    }
    let id = FeatureWorld::biome(world, p);
    rain_in_biome(id, p)
}
pub(crate) fn rain_in_biome(id: u8, p: IVec3) -> bool {
    let biome = crate::vanilla::biome::biome(id);
    !biome.snowy && crate::vanilla::population::temperature(id, p) > 0.15 && CAN_RAIN[id as usize]
}
pub(crate) fn biome_can_rain(id: u8) -> bool { CAN_RAIN[id as usize] }

fn fuel(state: u16) -> (u8, u8) {
    FUEL.get(usize::from(state >> 4))
        .copied()
        .unwrap_or_default()
}

pub(crate) trait FireEffects: FireWorld {
    fn fire_biome(&self, p: IVec3) -> u8;
    fn fire_raining_at(&self, p: IVec3) -> bool;
    fn fire_prime_tnt(&mut self, p: IVec3);
}
impl FireEffects for Access<'_> {
    fn fire_biome(&self, p: IVec3) -> u8 {
        FeatureWorld::biome(self, p)
    }
    fn fire_raining_at(&self, p: IVec3) -> bool {
        self.with(|_, b| b.hooks.clone()).raining_at(self, p)
    }
    fn fire_prime_tnt(&mut self, p: IVec3) {
        prime_tnt(self, p);
    }
}
fn exposed_to_rain(world: &impl FireEffects, p: IVec3) -> bool {
    [IVec3::ZERO, IVec3::NEG_X, IVec3::X, IVec3::NEG_Z, IVec3::Z]
        .into_iter()
        .any(|offset| world.fire_raining_at(p + offset))
}

fn prime_tnt(world: &Access<'_>, p: IVec3) {
    use glam::DVec3;
    world.with(|_, b| {
        let id = b.entities.allocate_entity_id();
        let entity_random = b.hooks.player_random_state();
        let velocity = b.entities.with_math(crate::vanilla::tnt::Primed::launch);
        let position = DVec3::new(
            f64::from(p.x as f32 + 0.5),
            f64::from(p.y),
            f64::from(p.z as f32 + 0.5),
        );
        b.pending_tnt.push(PendingTnt {
            id,
            position,
            velocity,
            fuse: 80,
            entity_random,
            insertion_index: b.entities.order.len() + b.pending_tnt.len(),
        });
        b.falling.effects.push(FallingEffect::Sound {
            name: "game.tnt.primed".into(),
            position,
            volume: 1.,
            pitch: 1.,
        });
    });
}

fn catch_on_fire(
    world: &mut impl FireEffects,
    random: &mut JavaRandom,
    p: IVec3,
    chance: i32,
    age: i32,
) {
    let target = FireView::get(world, p);
    if random.next_int(chance) >= i32::from(fuel(target).1) {
        return;
    }
    if random.next_int(age + 10) < 5 && !world.fire_raining_at(p) {
        let new_age = (age + random.next_int(5) / 4).min(15);
        FireView::set(world, p, 51 << 4 | new_age as u16, 3);
    } else {
        FireView::set(world, p, 0, 3);
    }
    if target >> 4 == 46 {
        world.fire_prime_tnt(p);
    }
}

pub(super) fn update(world: &mut Access<'_>, p: IVec3, state: u16) {
    let conditions = world.with(|_, b| b.fire_environment);
    let mut random = world.world_random();
    update_in(world, p, state, conditions, &mut random);
}
pub(crate) fn update_in(
    world: &mut impl FireEffects,
    p: IVec3,
    state: u16,
    conditions: FireEnvironment,
    random: &mut JavaRandom,
) {
    if !conditions.do_fire_tick {
        return;
    }
    if !supported(world, p) {
        FireView::set(world, p, 0, 3);
        // BlockFire continues its update even after removing itself here.
    }
    let floor = p - IVec3::Y;
    let floor_id = world.get(floor) >> 4;
    let eternal = floor_id == 87 || (world.end_dimension() && floor_id == 7);
    if !eternal && conditions.raining && exposed_to_rain(world, p) {
        FireView::set(world, p, 0, 3);
        return;
    }
    let age = i32::from(state & 15);
    if age < 15 {
        let new_age = age + random.next_int(3) / 2;
        FireView::set(world, p, 51 << 4 | new_age as u16, 4);
    }
    world.start_fire_tick(p);
    if !eternal {
        if !SIDES
            .iter()
            .any(|d| fuel(FireView::get(world, p + *d)).0 > 0)
        {
            if !world.solid_top(floor) || age > 3 {
                FireView::set(world, p, 0, 3);
            }
            return;
        }
        if fuel(FireView::get(world, floor)).0 == 0 && age == 15 && random.next_int(4) == 0 {
            FireView::set(world, p, 0, 3);
            return;
        }
    }
    let humid = crate::vanilla::biome::biome(world.fire_biome(p)).rainfall > 0.85;
    for (offset, chance) in [
        (IVec3::X, 300),
        (IVec3::NEG_X, 300),
        (IVec3::NEG_Y, 250),
        (IVec3::Y, 250),
        (IVec3::NEG_Z, 300),
        (IVec3::Z, 300),
    ] {
        catch_on_fire(
            world,
            random,
            p + offset,
            chance - if humid { 50 } else { 0 },
            age,
        );
    }
    for x in -1..=1 {
        for z in -1..=1 {
            for y in -1..=4 {
                let candidate = p + IVec3::new(x, y, z);
                if candidate == p || FireView::get(world, candidate) >> 4 != 0 {
                    continue;
                }
                let encouragement = SIDES
                    .iter()
                    .map(|d| fuel(FireView::get(world, candidate + *d)).0)
                    .max()
                    .unwrap_or(0);
                if encouragement == 0 {
                    continue;
                }
                let mut chance =
                    (i32::from(encouragement) + 40 + i32::from(conditions.difficulty) * 7)
                        / (age + 30);
                if humid {
                    chance /= 2;
                }
                let bound = y.max(1) * 100;
                if chance > 0
                    && random.next_int(bound) <= chance
                    && !(conditions.raining && exposed_to_rain(world, candidate))
                {
                    let new_age = (age + random.next_int(5) / 4).min(15);
                    FireView::set(world, candidate, 51 << 4 | new_age as u16, 3);
                }
            }
        }
    }
}
