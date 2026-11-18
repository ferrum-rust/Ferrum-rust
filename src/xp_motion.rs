use super::Orb;
use crate::vanilla::{
    collision_shapes, falling,
    item_entity::push_out_of_blocks,
    movement::{self, collision, MovementWorld},
    random::JavaRandom,
};
use anyhow::Result;
use glam::{DVec3, IVec3};
use serde::{Deserialize, Serialize};

pub trait OrbWorld: MovementWorld {
    fn full_collision_cube(&mut self, p: IVec3) -> Result<bool>;
    fn raining_at(&self, p: IVec3) -> bool;
}
impl OrbWorld for crate::world::World {
    fn full_collision_cube(&mut self, p: IVec3) -> Result<bool> {
        Ok(collision_shapes::full_collision_cube(
            p,
            self.get(p),
            |q| self.get(q),
            self.piston_at(p),
        ))
    }
    fn raining_at(&self, _p: IVec3) -> bool {
        false
    }
}
#[derive(Clone, Copy)]
pub struct Target {
    pub position: DVec3,
    pub eye: DVec3,
    pub spectator: bool,
}
pub type Sound = (&'static str, f32, f32);

impl Orb {
    pub fn tick(
        &mut self,
        world: &mut impl OrbWorld,
        target: Option<Target>,
        sounds: &mut Vec<Sound>,
    ) -> Result<()> {
        self.previous = self.position;
        // XPOrb overrides handleWaterMovement with handleMaterialAcceleration
        // on its entire box. Unlike EntityItem it never sets Entity.inWater.
        if let (_, Some(flow)) = movement::water_acceleration(world, self.bounds) {
            self.velocity += flow * 0.014;
        }
        if self.fire > 0 {
            if self.fire % 20 == 0 {
                self.health -= 1;
            }
            self.fire -= 1;
        }
        let lava_box = self.bounds.expand(DVec3::new(
            -f64::from(0.1f32),
            -f64::from(0.4f32),
            -f64::from(0.1f32),
        ));
        if movement::touches_liquid(world, lava_box, 10, 11) {
            self.health -= 4;
            self.fire = self.fire.max(300);
        }
        if self.position.y < -64. {
            self.dead = true;
        }
        if self.delay > 0 {
            self.delay -= 1;
        }
        self.velocity.y -= f64::from(0.03f32);
        let mut random = JavaRandom::from_state(self.random_state);
        if matches!(world.get(self.position.floor().as_ivec3()) >> 4, 10 | 11) {
            self.velocity.y = f64::from(0.2f32);
            self.velocity.x = f64::from((random.next_float() - random.next_float()) * 0.2);
            self.velocity.z = f64::from((random.next_float() - random.next_float()) * 0.2);
            sounds.push(("random.fizz", 0.4, 2. + random.next_float() * 0.4));
        }
        let center = DVec3::new(
            self.position.x,
            (self.bounds.min.y + self.bounds.max.y) / 2.,
            self.position.z,
        );
        push_out_of_blocks(
            world,
            self.bounds,
            center,
            &mut self.velocity,
            &mut random,
            |w, p| w.full_collision_cube(p),
        )?;
        self.attract(target);
        self.move_body(world, &mut random, sounds);
        self.random_state = random.state();
        let friction = if self.grounded {
            let below = IVec3::new(
                self.position.x.floor() as i32,
                self.bounds.min.y.floor() as i32 - 1,
                self.position.z.floor() as i32,
            );
            let slipperiness = match world.get(below) >> 4 {
                79 | 174 => 0.98f32,
                165 => 0.8,
                _ => 0.6,
            };
            slipperiness * 0.98
        } else {
            0.98
        };
        self.velocity.x *= f64::from(friction);
        self.velocity.y *= f64::from(0.98f32);
        self.velocity.z *= f64::from(friction);
        if self.grounded {
            self.velocity.y *= f64::from(-0.9f32);
        }
        self.color = self.color.wrapping_add(1);
        self.age = self.age.wrapping_add(1);
        self.dead |= self.health <= 0 || self.age >= 6000;
        Ok(())
    }
    fn attract(&mut self, target: Option<Target>) {
        if self.target_color < self.color.wrapping_sub(20).wrapping_add(self.id % 100) {
            let distance = target.map(|p| self.position.distance_squared(p.position));
            if !self.tracks_player || distance.is_none_or(|d| d > 64.) {
                self.tracks_player = target
                    .zip(distance)
                    .is_some_and(|(p, d)| !p.spectator && d < 64.);
            }
            self.target_color = self.color;
        }
        let Some(p) = target.filter(|p| !p.spectator) else {
            self.tracks_player = false;
            return;
        };
        if self.tracks_player {
            let d = (p.eye - self.position) / 8.;
            let distance = (d.x * d.x + d.y * d.y + d.z * d.z).sqrt();
            let scale = 1. - distance;
            if scale > 0. {
                self.velocity += d / distance * (scale * scale) * 0.1;
            }
        }
    }
    fn move_body(
        &mut self,
        world: &impl OrbWorld,
        random: &mut JavaRandom,
        sounds: &mut Vec<Sound>,
    ) {
        let mut delta = self.velocity;
        if self.in_web {
            self.in_web = false;
            delta *= DVec3::new(0.25, f64::from(0.05f32), 0.25);
            self.velocity = DVec3::ZERO;
        }
        let result =
            collision::move_body_with_step(world, self.bounds, delta, self.grounded, false, 0.);
        self.bounds = result.bounds;
        self.position = result.bounds.feet();
        self.grounded = result.grounded;
        if result.clipped[0] {
            self.velocity.x = 0.;
        }
        if result.clipped[2] {
            self.velocity.z = 0.;
        }
        if result.clipped[1] {
            let below = (self.position - DVec3::Y * f64::from(0.2f32))
                .floor()
                .as_ivec3();
            if world.get(below) >> 4 == 165 {
                if self.velocity.y < 0. {
                    self.velocity.y = -self.velocity.y;
                }
            } else {
                self.velocity.y = 0.;
            }
        }
        let min = (self.bounds.min + DVec3::splat(0.001)).floor().as_ivec3();
        let max = (self.bounds.max - DVec3::splat(0.001)).floor().as_ivec3();
        if falling::area_loaded(world, min, max) {
            for x in min.x..=max.x {
                for y in min.y..=max.y {
                    for z in min.z..=max.z {
                        match world.get(IVec3::new(x, y, z)) >> 4 {
                            30 => self.in_web = true,
                            88 => {
                                self.velocity.x *= 0.4;
                                self.velocity.z *= 0.4;
                            }
                            81 => self.health -= 1,
                            _ => {}
                        }
                    }
                }
            }
        }
        let wet = world.raining_at(self.position.floor().as_ivec3())
            || world.raining_at((self.position + DVec3::Y * 0.5).floor().as_ivec3());
        if falling::flammable(world, self.bounds.expand(DVec3::splat(-0.001))) {
            self.health -= 1;
            if !wet {
                self.fire = self.fire.wrapping_add(1);
                if self.fire == 0 {
                    self.fire = 160;
                }
            }
        } else if self.fire <= 0 {
            self.fire = -1;
        }
        if wet && self.fire > 0 {
            sounds.push((
                "random.fizz",
                0.7,
                1.6 + (random.next_float() - random.next_float()) * 0.4,
            ));
            self.fire = -1;
        }
    }
    pub fn collect(
        &mut self,
        player: &mut Experience,
        ticks: i32,
        sounds: &mut Vec<Sound>,
    ) -> bool {
        if self.dead || self.delay != 0 || player.cooldown != 0 {
            return false;
        }
        player.cooldown = 2;
        let mut random = JavaRandom::from_state(self.random_state);
        let pitch = 0.5 * ((random.next_float() - random.next_float()) * 0.7 + 1.8);
        self.random_state = random.state();
        sounds.push(("random.orb", 0.1, pitch));
        player.add(self.value, ticks, sounds);
        self.dead = true;
        true
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Experience {
    pub level: i32,
    pub progress: f32,
    pub total: i32,
    pub score: i32,
    pub cooldown: i32,
    pub last_sound: i32,
}
fn cap(level: i32) -> i32 {
    if level >= 30 {
        (level - 30).wrapping_mul(9).wrapping_add(112)
    } else if level >= 15 {
        (level - 15).wrapping_mul(5).wrapping_add(37)
    } else {
        7i32.wrapping_add(level.wrapping_mul(2))
    }
}
impl Experience {
    pub fn add(&mut self, points: i32, ticks: i32, sounds: &mut Vec<Sound>) {
        self.score = self.score.wrapping_add(points);
        let n = points.min(i32::MAX.wrapping_sub(self.total));
        self.progress += n as f32 / cap(self.level) as f32;
        self.total = self.total.wrapping_add(n);
        while self.progress >= 1. {
            self.progress = (self.progress - 1.) * cap(self.level) as f32;
            self.level = self.level.wrapping_add(1);
            if self.level < 0 {
                self.level = 0;
                self.progress = 0.;
                self.total = 0;
            }
            if self.level % 5 == 0 && (self.last_sound as f32) < ticks as f32 - 100. {
                let loudness = if self.level > 30 {
                    1.
                } else {
                    self.level as f32 / 30.
                };
                sounds.push(("random.levelup", loudness * 0.75, 1.));
                self.last_sound = ticks;
            }
            self.progress /= cap(self.level) as f32;
        }
    }
}
