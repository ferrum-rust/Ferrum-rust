//! EntityXPOrb spawn state. Its initial motion uses Math.random, separately
//! from the villager's trade RNG, the orb's Entity.rand and World.rand.
use crate::vanilla::{movement::Aabb, random::JavaRandom};
use glam::DVec3;
use serde::{Deserialize, Serialize};
#[path = "xp_motion.rs"]
pub mod motion;
pub fn split(total: i32) -> i32 {
    [2477, 1237, 617, 307, 149, 73, 37, 17, 7, 3, 1]
        .into_iter()
        .find(|&v| total >= v)
        .unwrap_or(1)
}
#[cfg(test)]
#[path = "xp_orb_tests.rs"]
mod tests;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Orb {
    pub id: i32,
    pub value: i32,
    pub position: DVec3,
    pub previous: DVec3,
    pub velocity: DVec3,
    pub bounds: Aabb,
    pub yaw: f32,
    pub age: i32,
    pub delay: i32,
    pub health: i32,
    #[serde(default)]
    pub dead: bool,
    pub random_state: u64,
    grounded: bool,
    color: i32,
    target_color: i32,
    tracks_player: bool,
    in_web: bool,
    fire: i32,
}
impl Orb {
    pub fn texture_index(&self) -> usize {
        [3, 7, 17, 37, 73, 149, 307, 617, 1237, 2477].partition_point(|&v| self.value >= v)
    }
    pub fn color(&self, partial_tick: f32) -> [u8; 4] {
        let phase = (self.color as f32 + partial_tick) * 0.5;
        let red = (crate::vanilla::math::sin(phase) + 1.) * 0.5 * 255.;
        let blue = (crate::vanilla::math::sin(phase + 4.1887903) + 1.) * 0.1 * 255.;
        [red as u8, 255, blue as u8, 128]
    }
    pub fn render_position(&self, partial_tick: f32) -> glam::Vec3 {
        if self.color == 0 {
            self.position.as_vec3()
        } else {
            self.previous
                .lerp(self.position, f64::from(partial_tick))
                .as_vec3()
        }
    }
    pub fn new(
        id: i32,
        position: DVec3,
        value: i32,
        math: &mut JavaRandom,
        random_state: u64,
    ) -> Self {
        let yaw = (math.next_double() * 360.) as f32;
        let x = (math.next_double() * f64::from(0.2f32) - f64::from(0.1f32)) as f32 * 2.;
        let y = (math.next_double() * 0.2) as f32 * 2.;
        let z = (math.next_double() * f64::from(0.2f32) - f64::from(0.1f32)) as f32 * 2.;
        Self {
            id,
            value,
            position,
            previous: DVec3::ZERO,
            velocity: DVec3::new(f64::from(x), f64::from(y), f64::from(z)),
            bounds: Aabb {
                min: position - DVec3::new(0.25, 0., 0.25),
                max: position + DVec3::new(0.25, 0.5, 0.25),
            },
            yaw,
            age: 0,
            delay: 0,
            health: 5,
            dead: false,
            random_state,
            grounded: false,
            color: 0,
            target_color: 0,
            tracks_player: false,
            in_web: false,
            fire: 0,
        }
    }
}
impl super::Simulation {
    pub fn spawn_xp_total(&mut self, position: DVec3, mut total: i32) {
        while total > 0 {
            let value = split(total);
            total -= value;
            self.spawn_xp(position, value);
        }
    }
    pub fn spawn_xp(&mut self, position: DVec3, value: i32) {
        self.next_id = self.next_id.wrapping_add(1);
        let mut math = JavaRandom::from_state(self.xp_math_random);
        let mut local = JavaRandom::new(JavaRandom::independent_seed());
        local.next_long();
        local.next_long();
        self.xp_orbs.push(Orb::new(
            self.next_id as i32,
            position,
            value,
            &mut math,
            local.state(),
        ));
        self.xp_math_random = math.state();
    }
    pub fn tick_xp(
        &mut self,
        world: &mut crate::world::World,
        player: &crate::player::Player,
        out: &mut super::Outcome,
    ) {
        let target = motion::Target {
            position: DVec3::from_array(player.precise_position()),
            eye: player.precise_eye(1.),
            spectator: self.commands.player_mode == Some(3),
        };
        let pickup = player.precise_bounds().expand(DVec3::new(1., 0.5, 1.));
        let mut sounds = Vec::new();
        for orb in &mut self.xp_orbs {
            if !world.contains(orb.position.as_vec3()) {
                continue;
            }
            if !target.spectator && self.health > 0.
                && pickup.intersects(orb.bounds)
                && orb.collect(&mut self.xp, self.player_ticks, &mut sounds)
            {
                self.pickup_fx.orb(orb, super::item_pickup_fx::Collector::Local);
            } else if let Err(error) = orb.tick(world, Some(target), &mut sounds) {
                eprintln!("Experience orb {} update: {error}", orb.id);
            }
            for (event, volume, pitch) in sounds.drain(..) {
                match event {
                    "random.orb" => out
                        .world_sounds
                        .push(("random.orb", target.position, volume, pitch)),
                    "random.levelup" => out
                        .world_sounds
                        .push(("random.levelup", target.position, volume, pitch)),
                    "random.fizz" => out
                        .world_sounds
                        .push(("random.fizz", orb.position, volume, pitch)),
                    _ => {}
                }
            }
        }
        self.xp_orbs.retain(|orb| !orb.dead);
        self.experience = self.xp.total.max(0) as u32;
    }
}
