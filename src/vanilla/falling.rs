//! EntityFallingBlock's double-precision tick and ordered world callbacks.
//! World mutation, block-specific placement, item construction and entity damage
//! stay on the authoritative world owner through the explicit callback boundary.
use super::{
    drops::DropNbt,
    movement::{collision, Aabb, MovementWorld},
    random::JavaRandom,
};
use anyhow::Result;
use glam::{DVec3, IVec3};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub trait FallingWorld: MovementWorld {
    fn is_remote(&self) -> bool;
    fn do_entity_drops(&self) -> bool;
    fn set_block(&mut self, p: IVec3, state: u16, flags: i32) -> Result<bool>;
    /// World.canBlockBePlaced(block, position, true, UP, null, null).
    fn can_place(&mut self, p: IVec3, state: u16) -> Result<bool>;
    /// BlockFalling.onEndFalling followed by optional tile-data merge. The merge
    /// copies every supplied tag except x/y/z, then calls tile.markDirty().
    fn placed(&mut self, p: IVec3, state: u16, tile: Option<&DropNbt>) -> Result<()>;
    /// Entity.entityDropItem(new ItemStack(block, 1, damageDropped(state)), 0).
    fn drop_item(&mut self, position: DVec3, state: u16) -> Result<()>;
    /// Original list order; each intersecting entity receives the same damage.
    fn damage_entities(&mut self, bounds: Aabb, anvil: bool, amount: f32) -> Result<()>;
    /// Stateful contact blocks (portals, tripwires, pressure plates, detector
    /// rails) belong to the world. Web and soul-sand motion are handled below.
    fn contact(&mut self, p: IVec3, state: u16, entity: &mut Falling) -> Result<()>;
    fn raining_at(&self, p: IVec3) -> bool;
    fn sound(&mut self, name: &str, volume: f32, pitch: f32) -> Result<()>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Falling {
    pub state: u16,
    pub position: DVec3,
    pub previous: DVec3,
    pub velocity: DVec3,
    pub bounds: Aabb,
    pub age: i32,
    pub should_drop_item: bool,
    pub cannot_place: bool,
    pub hurt_entities: bool,
    pub hurt_max: i32,
    pub hurt_amount: f32,
    #[serde(default, with = "super::item_nbt::optional_arc")]
    pub tile: Option<Arc<DropNbt>>,
    pub grounded: bool,
    pub collided_horizontal: bool,
    pub collided_vertical: bool,
    pub dead: bool,
    pub in_web: bool,
    pub in_water: bool,
    pub no_clip: bool,
    pub fall_distance: f32,
    pub fire: i32,
    pub fire_resistance: i32,
    pub immune_to_fire: bool,
    pub random_state: u64,
}
impl Falling {
    /// The caller supplies the entity's independent Random state, after entity
    /// construction/UUID draws. It is not the world's shared Random stream.
    pub fn new(state: u16, position: DVec3, random_state: u64) -> Self {
        let half = (0.98f32 / 2.) as f64;
        Self {
            state,
            position,
            previous: position,
            velocity: DVec3::ZERO,
            bounds: Aabb {
                min: position - DVec3::new(half, 0., half),
                max: position + DVec3::new(half, 0.98f32 as f64, half),
            },
            age: 0,
            should_drop_item: true,
            cannot_place: false,
            hurt_entities: false,
            hurt_max: 40,
            hurt_amount: 2.,
            tile: None,
            grounded: false,
            collided_horizontal: false,
            collided_vertical: false,
            dead: false,
            in_web: false,
            in_water: false,
            no_clip: false,
            fall_distance: 0.,
            fire: 0,
            fire_resistance: 1,
            immune_to_fire: false,
            random_state,
        }
    }
    pub fn tick(&mut self, world: &mut impl FallingWorld) -> Result<()> {
        let block = self.state >> 4;
        if block == 0 {
            self.dead = true;
            return Ok(());
        }
        self.previous = self.position;
        let first = self.age == 0;
        self.age = self.age.wrapping_add(1);
        if first {
            let p = self.position.floor().as_ivec3();
            if world.get(p) >> 4 == block {
                world.set_block(p, 0, 3)?;
            } else if !world.is_remote() {
                self.dead = true;
                return Ok(());
            }
        }
        self.velocity.y -= 0.04f32 as f64;
        self.move_entity(world)?;
        self.velocity *= 0.98f32 as f64;
        if !world.is_remote() {
            let p = self.position.floor().as_ivec3();
            if self.grounded {
                self.velocity.x *= 0.7f32 as f64;
                self.velocity.z *= 0.7f32 as f64;
                self.velocity.y *= -0.5;
                if world.get(p) >> 4 != 36 {
                    self.dead = true;
                    if !self.cannot_place {
                        if world.can_place(p, self.state)?
                            && !can_fall_into(world.get(p - IVec3::Y))
                            && world.set_block(p, self.state, 3)?
                        {
                            world.placed(p, self.state, self.tile.as_deref())?;
                        } else if self.should_drop_item && world.do_entity_drops() {
                            world.drop_item(self.position, self.state)?;
                        }
                    }
                }
            } else if self.age > 100 && (p.y < 1 || p.y > 256) || self.age > 600 {
                if self.should_drop_item && world.do_entity_drops() {
                    world.drop_item(self.position, self.state)?;
                }
                self.dead = true;
            }
        }
        Ok(())
    }
    pub fn fall(&mut self, world: &mut impl FallingWorld, distance: f32) -> Result<()> {
        if !self.hurt_entities {
            return Ok(());
        }
        let n = (distance - 1.).ceil() as i32;
        if n <= 0 {
            return Ok(());
        }
        let anvil = self.state >> 4 == 145;
        let amount = ((n as f32 * self.hurt_amount).floor() as i32).min(self.hurt_max);
        world.damage_entities(self.bounds, anvil, amount as f32)?;
        if anvil {
            let mut random = JavaRandom::from_state(self.random_state);
            let damage = (random.next_float() as f64) < 0.05f32 as f64 + n as f64 * 0.05;
            self.random_state = random.state();
            if damage {
                let level = ((self.state & 15) >> 2) + 1;
                if level > 2 {
                    self.cannot_place = true;
                } else {
                    self.state = (self.state & !12) | (level << 2);
                }
            }
        }
        Ok(())
    }
    fn move_entity(&mut self, world: &mut impl FallingWorld) -> Result<()> {
        let mut delta = self.velocity;
        if self.no_clip {
            self.bounds = self.bounds.offset(delta);
            self.position = self.bounds.feet();
            return Ok(());
        }
        if self.in_web {
            self.in_web = false;
            delta.x *= 0.25;
            delta.y *= 0.05f32 as f64;
            delta.z *= 0.25;
            self.velocity = DVec3::ZERO;
        }
        let movement =
            collision::move_body_with_step(world, self.bounds, delta, self.grounded, false, 0.);
        self.bounds = movement.bounds;
        self.position = self.bounds.feet();
        self.grounded = movement.grounded;
        self.collided_horizontal = movement.clipped[0] || movement.clipped[2];
        self.collided_vertical = movement.clipped[1];
        let under = IVec3::new(
            self.position.x.floor() as i32,
            (self.position.y - 0.2f32 as f64).floor() as i32,
            self.position.z.floor() as i32,
        );
        let mut block = world.get(under);
        if block >> 4 == 0 {
            let below = world.get(under - IVec3::Y);
            if matches!(below >> 4, 85 | 107 | 113 | 139 | 183..=192) {
                block = below;
            }
        }
        if self.grounded {
            if self.fall_distance > 0. {
                // Slime/hay change the fall multiplier, which FallingBlock's
                // override ignores. Farmland only changes for living entities.
                self.fall(world, self.fall_distance)?;
                self.fall_distance = 0.;
            }
        } else if movement.delta.y < 0. {
            self.fall_distance = (self.fall_distance as f64 - movement.delta.y) as f32;
        }
        if movement.clipped[0] {
            self.velocity.x = 0.;
        }
        if movement.clipped[2] {
            self.velocity.z = 0.;
        }
        if movement.clipped[1] {
            if block >> 4 == 165 {
                if self.velocity.y < 0. {
                    self.velocity.y = -self.velocity.y;
                }
            } else {
                self.velocity.y = 0.;
            }
        }
        let min = (self.bounds.min + DVec3::splat(0.001)).floor().as_ivec3();
        let max = (self.bounds.max - DVec3::splat(0.001)).floor().as_ivec3();
        if area_loaded(world, min, max) {
            for x in min.x..=max.x {
                for y in min.y..=max.y {
                    for z in min.z..=max.z {
                        let p = IVec3::new(x, y, z);
                        let state = world.get(p);
                        match state >> 4 {
                            30 => {
                                self.in_web = true;
                                self.fall_distance = 0.;
                            }
                            88 => {
                                self.velocity.x *= 0.4;
                                self.velocity.z *= 0.4;
                            }
                            _ => {}
                        }
                        world.contact(p, state, self)?;
                    }
                }
            }
        }
        let wet = self.in_water
            || world.raining_at(self.position.floor().as_ivec3())
            || world.raining_at(
                (self.position + DVec3::Y * 0.98f32 as f64)
                    .floor()
                    .as_ivec3(),
            );
        if flammable(world, self.bounds.expand(DVec3::splat(-0.001))) {
            // EntityFallingBlock inherits Entity.attackEntityFrom (false), so
            // fire contact cannot change health. Its ignition counter still runs.
            if !wet {
                self.fire = self.fire.wrapping_add(1);
                if self.fire == 0 {
                    self.fire = 160;
                }
            }
        } else if self.fire <= 0 {
            self.fire = -self.fire_resistance;
        }
        if wet && self.fire > 0 {
            let mut random = JavaRandom::from_state(self.random_state);
            let pitch = 1.6f32 + (random.next_float() - random.next_float()) * 0.4;
            self.random_state = random.state();
            world.sound("random.fizz", 0.7, pitch)?;
            self.fire = -self.fire_resistance;
        }
        Ok(())
    }
}
pub fn can_fall_into(state: u16) -> bool {
    matches!(state >> 4, 0 | 8..=11 | 51)
}
/// Falling blocks ignore entity collision when placing. Sand, gravel and the
/// anvil inherit Block.canReplace. Anvils additionally replace circuits.
pub fn can_place_gravity(state: u16, target: u16) -> Option<bool> {
    match state >> 4 {
        12 | 13 | 122 | 145 => Some(
            super::material::replaceable(target)
                || state >> 4 == 145 && super::material::is_circuits(target),
        ),
        _ => None,
    }
}
pub(crate) fn area_loaded(world: &impl MovementWorld, min: IVec3, max: IVec3) -> bool {
    max.y >= 0
        && min.y < 256
        && (min.x.div_euclid(16)..=max.x.div_euclid(16)).all(|x| {
            (min.z.div_euclid(16)..=max.z.div_euclid(16))
                .all(|z| world.loaded_column(x * 16, z * 16))
        })
}
pub(crate) fn flammable(world: &impl MovementWorld, bounds: Aabb) -> bool {
    let min = bounds.min.floor().as_ivec3();
    let max = (bounds.max + DVec3::ONE).floor().as_ivec3();
    area_loaded(world, min, max)
        && (min.x..max.x).any(|x| {
            (min.y..max.y).any(|y| {
                (min.z..max.z).any(|z| matches!(world.get(IVec3::new(x, y, z)) >> 4, 10 | 11 | 51))
            })
        })
}

#[cfg(test)]
#[path = "falling_tests.rs"]
mod tests;
