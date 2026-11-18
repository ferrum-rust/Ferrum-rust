//! Ground navigation and persisted animal lifecycle state.
use super::MobKind;
use crate::world::World;
use glam::Vec3;
use serde::{Deserialize, Serialize};
#[path = "mob_destination.rs"]
pub mod destination;
#[path = "mob_goals.rs"]
pub mod goals;
#[path = "mob_locomotion.rs"]
pub mod locomotion;
#[path = "mob_move.rs"]
pub mod motion;
// Golem movement and target selection must join the living-entity scheduler
// before generated village spawn requests are consumed by the playable world.
#[path = "iron_golem.rs"]
pub mod golem;
#[path = "mob_path.rs"]
pub mod path;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MobBrain {
    #[serde(skip)]
    pub gait: crate::entity_visual::Gait,
    pub fire_ticks: i32,
    pub anger_ticks: u16,
    pub stare_ticks: u8,
    pub player_hit_ticks: u8,
    pub hover_target: Option<Vec3>,
    pub dragon_yaw_velocity: f32,
    pub dragon_chasing: bool,
    pub dragon_slowed: bool,
    pub dragon_death_ticks: u16,
    pub dragon_healing_crystal: Option<u64>,
    #[serde(skip)]
    pub dragon_wing_phase: f32,
    #[serde(skip)]
    pub dragon_prev_wing_phase: f32,
    #[serde(skip)]
    pub dragon_flight: Option<Box<crate::remote::dragon::Flight>>,
    pub enderman_screaming: bool,
    #[serde(skip)]
    pub enderman_unseen_ticks: u16,
    #[serde(skip)]
    pub enderman_teleport_delay: u16,
    pub air_ticks: i16,
    pub jump_timer: u8,
    pub attack_step: u8,
    #[serde(skip)]
    pub(crate) fire_flag: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub movement: Option<Box<locomotion::Locomotion>>,
    pub swimming: bool,
    pub swim_ticks: i32,
    pub entity_random: Option<u64>,
    /// Negative until adulthood, positive during the breeding cooldown (20 Hz).
    pub growing_age: i32,
    pub love: u16,
    pub panic: u16,
    pub panic_from: Vec3,
    pub courtship: u8,
    pub mate: Option<u64>,
    pub egg_timer: u16,
    /// Raw incoming damage for the stronger-hit difference during immunity.
    #[serde(skip)]
    pub last_damage: f32,
    #[serde(skip)]
    pub navigator: path::navigation::Navigator,
    #[serde(skip)]
    pub path_target: Option<Vec3>,
    #[serde(skip)]
    pub path_timer: u8,
    #[serde(skip)]
    pub wander_target: Option<Vec3>,
    #[serde(skip)]
    pub flee_target: Option<Vec3>,
    #[serde(skip)]
    pub tempted: bool,
    #[serde(skip)]
    pub tempt_delay: u8,
    #[serde(skip)]
    pub attack_timer: u8,
    #[serde(skip)]
    pub target_mob: Option<u64>,
    #[serde(skip)]
    pub merchant_goals: Option<crate::villagers::goals::MerchantGoals>,
    #[serde(skip)]
    pub grounded: bool,
    #[serde(skip)]
    pub velocity_changed: bool,
}

impl MobBrain {
    /// Keep every scalar, including transient gait, age, fire and RNG. Owned
    /// movement/routes/goals are never consulted by the presentation mirror.
    pub(crate) fn presentation_clone(&self) -> Self {
        Self {
            gait: self.gait.clone(),
            fire_ticks: self.fire_ticks.clone(),
            anger_ticks: self.anger_ticks.clone(),
            stare_ticks: self.stare_ticks.clone(),
            player_hit_ticks: self.player_hit_ticks.clone(),
            hover_target: self.hover_target.clone(),
            dragon_yaw_velocity: self.dragon_yaw_velocity.clone(),
            dragon_chasing: self.dragon_chasing.clone(),
            dragon_slowed: self.dragon_slowed.clone(),
            dragon_death_ticks: self.dragon_death_ticks,
            dragon_healing_crystal: self.dragon_healing_crystal,
            dragon_wing_phase: self.dragon_wing_phase,
            dragon_prev_wing_phase: self.dragon_prev_wing_phase,
            dragon_flight: self.dragon_flight.clone(),
            enderman_screaming: self.enderman_screaming,
            enderman_unseen_ticks: self.enderman_unseen_ticks,
            enderman_teleport_delay: self.enderman_teleport_delay,
            air_ticks: self.air_ticks.clone(),
            jump_timer: self.jump_timer.clone(),
            attack_step: self.attack_step.clone(),
            fire_flag: self.fire_flag.clone(),
            movement: None,
            swimming: self.swimming.clone(),
            swim_ticks: self.swim_ticks.clone(),
            entity_random: self.entity_random.clone(),
            growing_age: self.growing_age.clone(),
            love: self.love.clone(),
            panic: self.panic.clone(),
            panic_from: self.panic_from.clone(),
            courtship: self.courtship.clone(),
            mate: self.mate.clone(),
            egg_timer: self.egg_timer.clone(),
            last_damage: self.last_damage.clone(),
            navigator: Default::default(),
            path_target: self.path_target.clone(),
            path_timer: self.path_timer.clone(),
            wander_target: self.wander_target.clone(),
            flee_target: self.flee_target.clone(),
            tempted: self.tempted.clone(),
            tempt_delay: self.tempt_delay.clone(),
            attack_timer: self.attack_timer.clone(),
            target_mob: self.target_mob,
            merchant_goals: None,
            grounded: self.grounded.clone(),
            velocity_changed: self.velocity_changed.clone(),
        }
    }

    pub(crate) fn burning(&self) -> bool { self.fire_flag }
    pub fn tick(&mut self) {
        if !self.enderman_screaming {
            self.anger_ticks = self.anger_ticks.saturating_sub(1);
        }
        self.player_hit_ticks = self.player_hit_ticks.saturating_sub(1);
        self.jump_timer = self.jump_timer.saturating_sub(1);
        self.growing_age -= self.growing_age.signum();
        self.love = if self.growing_age != 0 {
            0
        } else {
            self.love.saturating_sub(1)
        };
        self.tick_revenge();
        self.path_timer = self.path_timer.saturating_sub(1);
        self.tempt_delay = self.tempt_delay.saturating_sub(1);
        self.attack_timer = self.attack_timer.saturating_sub(1);
    }

    pub(super) fn tick_revenge(&mut self) {
        self.panic = self.panic.saturating_sub(1);
    }

    pub fn hurt(&mut self, attacker: Vec3) {
        self.panic = 101;
        self.panic_from = attacker;
        self.love = 0;
        self.courtship = 0;
        if self.flee_target.is_none() {
            self.path_timer = 0;
            self.path_target = None;
            self.wander_target = None;
        }
    }
    fn sample(
        &mut self,
        world: &World,
        mob: &super::Mob,
        clock: u64,
        search: destination::Search,
        merchant: Option<&mut u64>,
    ) -> Option<Vec3> {
        use crate::vanilla::{lightmap, random::JavaRandom};
        let state = merchant.unwrap_or_else(|| {
            self.entity_random.get_or_insert_with(|| {
                let mut r = JavaRandom::new(JavaRandom::independent_seed());
                r.next_long();
                r.next_long();
                r.state()
            })
        });
        let mut random = JavaRandom::from_state(*state);
        let sky_loss = sky_reduction(clock);
        let brightness = lightmap::brightness_table(0);
        let selected = destination::choose(&mut random, search, |p| {
            if mob.kind == MobKind::Villager {
                return 0.;
            }
            if !mob.kind.hostile() && world.get(p - glam::IVec3::Y) >> 4 == 2 {
                return 10.;
            }
            let light = world.light_uv(p, 0);
            let block = light[0] / 16;
            let sky = (light[1] / 16).saturating_sub(sky_loss);
            let weight = brightness[usize::from(block.max(sky))] - 0.5;
            if mob.kind.hostile() {
                -weight
            } else {
                weight
            }
        });
        *state = random.state();
        selected.map(|p| p.as_vec3())
    }
    pub fn flee(
        &mut self,
        world: &World,
        mob: &super::Mob,
        clock: u64,
        threat: Option<Vec3>,
        home: Option<(glam::IVec3, f32)>,
        random: Option<&mut u64>,
    ) -> Option<Vec3> {
        if let Some(goal) = self.flee_target {
            if !self.navigator.no_path() {
                return Some(goal);
            }
            self.flee_target = None;
        }
        if !mob.age.wrapping_sub(1).is_multiple_of(3) {
            return None;
        }
        let (horizontal, vertical, direction) = if mob.kind == MobKind::Villager {
            (16, 7, destination::Direction::Away(threat?.as_dvec3()))
        } else {
            if self.panic == 0 && self.fire_ticks <= 0 {
                return None;
            }
            (5, 4, destination::Direction::Any)
        };
        let result = self.sample(
            world,
            mob,
            clock,
            destination::Search {
                position: mob.position.as_dvec3(),
                horizontal,
                vertical,
                home,
                direction,
            },
            random,
        );
        self.flee_target = result.filter(|p| {
            threat.is_none_or(|other| {
                p.distance_squared(other) >= mob.position.distance_squared(other)
            })
        });
        self.flee_target
    }
    pub fn toward_home(
        &mut self,
        world: &World,
        mob: &super::Mob,
        clock: u64,
        home: (glam::IVec3, f32),
        random: Option<&mut u64>,
    ) -> Option<Vec3> {
        let (center, radius) = home;
        if mob
            .position
            .floor()
            .as_dvec3()
            .distance_squared(center.as_dvec3())
            < f64::from(radius * radius)
        {
            return None;
        }
        if self.path_target.is_some() && !self.navigator.no_path() {
            return self.path_target;
        }
        if !mob.age.wrapping_sub(1).is_multiple_of(3) {
            return None;
        }
        self.sample(
            world,
            mob,
            clock,
            destination::Search {
                position: mob.position.as_dvec3(),
                horizontal: 16,
                vertical: 7,
                home: Some(home),
                direction: destination::Direction::Towards(center.as_dvec3()),
            },
            random,
        )
    }

    pub fn feed(&mut self) -> bool {
        if self.growing_age < 0 {
            // EntityAnimal feeds 10% of the remaining whole seconds of growth.
            let seconds = (-self.growing_age / 20) as f32;
            self.growing_age = (self.growing_age + (seconds * 0.1) as i32 * 20).min(0);
            true
        } else if self.growing_age == 0 && self.love == 0 {
            self.love = 600;
            true
        } else {
            false
        }
    }

    pub fn steer(
        &mut self,
        world: &World,
        mob: &super::Mob,
        target: Vec3,
        navigation: Navigation,
    ) -> Option<Vec3> {
        let (walker, options, destination) = navigation_input(world, mob, target, navigation);
        let changed = self
            .path_target
            .is_none_or(|p| p.distance_squared(target) > 4.);
        let search = (self.path_timer == 0 || changed)
            && (self.grounded || options.swim && (walker.in_water || walker.in_lava));
        if !search && self.navigator.no_path() {
            // An exhausted path still advances the navigator's tick counter,
            // but needs no chunk prefetch or allocation.
            return self.navigator.tick(world, walker, self.grounded, options.swim, 1.)
                .map(|v| v.as_vec3());
        }
        let view = path::ChunkWindow::new(world, walker, options, destination);
        if search {
            let route = view.find(walker, options, destination);
            if route.is_some() || !navigation.entity_destination {
                self.navigator.set(&view, walker, options.swim, route);
                if navigation.avoid_sun {
                    self.navigator.avoid_sun(|p| sky(world, p), walker);
                }
            }
            self.path_timer = 20;
            self.path_target = Some(target);
        }
        self.navigator
            .tick(&view, walker, self.grounded, options.swim, 1.)
            .map(|v| v.as_vec3())
    }
}

pub fn breeding_food(kind: MobKind, item: u16) -> bool {
    match kind {
        MobKind::Cow | MobKind::Mooshroom | MobKind::Sheep => item == 296,
        MobKind::Rabbit => matches!(item, 391 | 396 | 37),
        MobKind::Ocelot => item == 349,
        MobKind::Wolf => matches!(item,319|320|363|364|365|366|367|411|412|423|424),
        MobKind::Horse => matches!(item,322|396),
        MobKind::Pig => item == 391,
        MobKind::Chicken => matches!(item, 295 | 361 | 362),
        _ => false,
    }
}

pub fn tempting_food(kind: MobKind, item: u16) -> bool {
    // Chicken EntityAITempt uses wheat seeds; breeding also accepts melon/pumpkin seeds.
    if matches!(kind, MobKind::Wolf | MobKind::Horse) { false } else if kind == MobKind::Pig {
        matches!(item, 391 | 398)
    } else if kind == MobKind::Chicken {
        item == 295
    } else {
        breeding_food(kind, item)
    }
}

#[derive(Clone, Copy)]
pub struct Navigation {
    pub size: (f32, f32),
    pub attack_target: bool,
    pub entity_destination: bool,
    pub difficulty: u8,
    pub avoid_sun: bool,
}
fn navigation_input(
    world: &World,
    mob: &super::Mob,
    destination: Vec3,
    nav: Navigation,
) -> (path::Walker, path::Options, path::Target) {
    use crate::vanilla::movement::{self, Aabb};
    use glam::DVec3;
    let (width, height) = nav.size;
    let position = mob.position.as_dvec3();
    let radius = f64::from(width * 0.5);
    let body = Aabb {
        min: position - DVec3::new(radius, 0., radius),
        max: position + DVec3::new(radius, f64::from(height), radius),
    };
    let (in_water, _) = movement::water_acceleration(
        world,
        body.expand(DVec3::new(0., -f64::from(0.4f32), 0.))
            .expand(DVec3::splat(-0.001)),
    );
    let in_lava = movement::touches_liquid(
        world,
        body.expand(DVec3::new(
            -f64::from(0.1f32),
            -f64::from(0.4f32),
            -f64::from(0.1f32),
        )),
        10,
        11,
    );
    let max_fall = if nav.attack_target {
        ((mob.health - mob.kind.health() * 0.33) as i32 - (3 - i32::from(nav.difficulty)) * 4)
            .max(0)
            + 3
    } else {
        3
    };
    let walker = path::Walker {
        position,
        width,
        height,
        in_water,
        in_lava,
        max_fall,
    };
    let options = path::Options {
        enter_doors: true,
        break_doors: matches!(mob.kind, MobKind::Villager | MobKind::Zombie),
        avoid_water: matches!(
            mob.kind,
            MobKind::Cow | MobKind::Pig | MobKind::Sheep | MobKind::Villager
        ),
        swim: true,
        range: if mob.kind == MobKind::Zombie {
            35.
        } else {
            16.
        },
    };
    let target = if nav.entity_destination {
        path::Target::Entity(destination.as_dvec3())
    } else {
        path::Target::Block(destination.floor().as_ivec3())
    };
    (walker, options, target)
}
fn sky(world: &World, p: glam::IVec3) -> bool {
    let Some(chunk) = world.chunks.get(&(p.x.div_euclid(16), p.z.div_euclid(16))) else {
        return false;
    };
    (p.y.max(0)..chunk.top).all(|y| {
        crate::vanilla::material::light_opacity(chunk.get(
            p.x.rem_euclid(16),
            y,
            p.z.rem_euclid(16),
        )) == 0
    })
}
fn sky_reduction(clock: u64) -> u8 {
    let angle = crate::vanilla::lightmap::celestial_angle(clock as i64, 1., 0);
    let f = (1. - (crate::vanilla::math::cos(angle * std::f32::consts::PI * 2.) * 2. + 0.5))
        .clamp(0., 1.);
    ((1. - (1. - f)) * 11.) as u8
}
pub fn clear_day(clock: u64) -> bool {
    sky_reduction(clock) < 4
}
#[cfg(test)]
pub fn ground_route(
    world: &World,
    mob: &super::Mob,
    destination: Vec3,
    nav: Navigation,
) -> Vec<Vec3> {
    let (walker, options, target) = navigation_input(world, mob, destination, nav);
    let offset = (walker.width + 1.) as i32 as f32 * 0.5;
    path::find_world(world, walker, options, target)
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.as_vec3() + Vec3::new(offset, 0., offset))
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animal_food_and_lifecycle_rules() {
        assert!(breeding_food(MobKind::Cow, 296));
        assert!(!breeding_food(MobKind::Pig, 296));
        assert!(breeding_food(MobKind::Chicken, 361));
        assert!(!tempting_food(MobKind::Chicken, 361));
        let mut b = MobBrain::default();
        assert!(b.feed());
        assert_eq!(b.love, 600);
        assert!(!b.feed());
        b.hurt(Vec3::X);
        assert_eq!(b.love, 0);
        assert_eq!(b.panic, 101);
        for _ in 0..100 { b.tick_revenge(); }
        assert_eq!(b.panic, 1);
        b.tick_revenge();
        assert_eq!(b.panic, 0);
        b.growing_age = -24000;
        assert!(b.feed());
        assert_eq!(b.growing_age, -21600);
        b.growing_age = 6000;
        assert!(!b.feed());
        b.tick();
        assert_eq!(b.growing_age, 5999);
    }
}
