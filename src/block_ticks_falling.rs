//! Falling entities in the playable local world. The scheduler and renderer
//! share the same double-precision entity state used by the native host.
use super::{BlockTicks, FallingBlock, TickResult};
use crate::vanilla::{
    drops::DropNbt,
    falling::{can_place_gravity, FallingWorld},
    movement::{Aabb, MovementWorld},
    random::JavaRandom,
};
use crate::world::World;
use anyhow::Result;
use glam::{DVec3, IVec3, Vec3};
use serde::{Deserialize, Deserializer};
use std::cell::RefCell;

#[derive(Debug)]
pub struct FallingImpact {
    pub bounds: Aabb,
    pub anvil: bool,
    pub amount: f32,
}
fn independent_random() -> JavaRandom {
    JavaRandom::new(JavaRandom::independent_seed())
}
thread_local! {
    static CLIENT_SOUND_RNG: RefCell<JavaRandom> = RefCell::new(independent_random());
}
fn entity_at(position: DVec3, state: u16) -> FallingBlock {
    let mut rng = independent_random();
    // Entity's initial UUID consumes two longs from its own Random.
    rng.next_long();
    rng.next_long();
    let mut entity = FallingBlock::new(state, position, rng.state());
    entity.hurt_entities = state >> 4 == 145;
    entity
}
pub(super) fn spawn(p: IVec3, state: u16) -> FallingBlock {
    entity_at(p.as_dvec3() + DVec3::new(0.5, 0., 0.5), state)
}

/// Old local saves store only f32 position/velocity and age. Preserve all of
/// those values and reconstruct the 0.98-wide bounding box. New records carry
/// the full original fields, including RNG, sub-block position and web state.
pub(super) fn read_saved<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<FallingBlock>, D::Error> {
    #[derive(Deserialize)]
    struct Legacy {
        state: u16,
        position: Vec3,
        previous: Vec3,
        velocity: Vec3,
        age: u16,
    }
    Vec::<serde_json::Value>::deserialize(deserializer)?
        .into_iter()
        .map(|value| {
            if value.get("bounds").is_some() {
                return serde_json::from_value(value).map_err(serde::de::Error::custom);
            }
            let old: Legacy = serde_json::from_value(value).map_err(serde::de::Error::custom)?;
            let mut entity = entity_at(old.position.as_dvec3(), old.state);
            entity.previous = old.previous.as_dvec3();
            entity.velocity = old.velocity.as_dvec3();
            // The old scheduler already removed the source on spawn, even at age 0.
            entity.age = i32::from(old.age).max(1);
            Ok(entity)
        })
        .collect()
}

struct PlayWorld<'a> {
    world: &'a mut World,
    ticks: &'a mut BlockTicks,
    output: &'a mut TickResult,
    sound_position: DVec3,
}
impl MovementWorld for PlayWorld<'_> {
    fn get(&self, p: IVec3) -> u16 {
        self.world.get(p)
    }
    fn loaded_column(&self, x: i32, z: i32) -> bool {
        self.world.loaded_column(x, z)
    }
    fn block_boxes(&self, p: IVec3, state: u16, out: &mut Vec<Aabb>) {
        self.world.block_boxes(p, state, out);
    }
}
impl FallingWorld for PlayWorld<'_> {
    fn is_remote(&self) -> bool {
        false
    }
    fn do_entity_drops(&self) -> bool {
        self.ticks.state().do_entity_drops
    }
    fn set_block(&mut self, p: IVec3, state: u16, flags: i32) -> Result<bool> {
        let before = self.output.changed.len();
        self.ticks
            .write(self.world, p, state, flags & 1 != 0, self.output);
        Ok(self.output.changed.len() != before)
    }
    fn can_place(&mut self, p: IVec3, state: u16) -> Result<bool> {
        // This scheduler spawns only the three BlockFalling families.
        Ok(can_place_gravity(state, self.world.get(p)).unwrap_or(false))
    }
    fn placed(&mut self, p: IVec3, state: u16, _: Option<&DropNbt>) -> Result<()> {
        if state >> 4 == 145 {
            let pitch = CLIENT_SOUND_RNG.with(|r| r.borrow_mut().next_float() * 0.1 + 0.9);
            self.output.world_sounds.push((
                "random.anvil_land",
                p.as_dvec3() + DVec3::splat(0.5),
                0.3,
                pitch,
            ));
        }
        Ok(())
    }
    fn drop_item(&mut self, position: DVec3, state: u16) -> Result<()> {
        let id = state >> 4;
        let damage = match id {
            12 => state & 15,
            145 => (state & 15) >> 2,
            _ => 0,
        };
        self.output
            .drops
            .push((position.as_vec3(), id << 4 | damage));
        Ok(())
    }
    fn damage_entities(&mut self, bounds: Aabb, anvil: bool, amount: f32) -> Result<()> {
        self.output.impacts.push(FallingImpact {
            bounds,
            anvil,
            amount,
        });
        Ok(())
    }
    fn contact(&mut self, _: IVec3, _: u16, _: &mut FallingBlock) -> Result<()> {
        // The legacy simulation has no portal travel or entity-powered circuit
        // callbacks yet. Web and soul-sand contact are handled by the core.
        Ok(())
    }
    fn raining_at(&self, _: IVec3) -> bool {
        false
    }
    fn sound(&mut self, name: &str, volume: f32, pitch: f32) -> Result<()> {
        if name == "random.fizz" {
            self.output
                .world_sounds
                .push(("random.fizz", self.sound_position, volume, pitch));
        }
        Ok(())
    }
}
pub(super) fn tick(ticks: &mut BlockTicks, world: &mut World, output: &mut TickResult) {
    let entities = std::mem::take(&mut ticks.falling);
    let mut access = PlayWorld {
        world,
        ticks,
        output,
        sound_position: DVec3::ZERO,
    };
    for mut entity in entities {
        let p = entity.position.floor().as_ivec3();
        access.sound_position = entity.position;
        // World.updateEntityWithOptionalForce requires the horizontal +/-32
        // neighborhood for a non-player entity before ticking it.
        let loaded = ((p.x - 32).div_euclid(16)..=(p.x + 32).div_euclid(16)).all(|x| {
            ((p.z - 32).div_euclid(16)..=(p.z + 32).div_euclid(16))
                .all(|z| access.world.chunks.contains_key(&(x, z)))
        });
        if loaded {
            entity
                .tick(&mut access)
                .expect("in-memory falling callbacks are infallible");
        }
        if !entity.dead {
            access.ticks.falling.push(entity);
        }
    }
}
