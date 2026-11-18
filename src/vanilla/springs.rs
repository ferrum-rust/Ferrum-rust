//! BiomeDecorator's spring phase. Call between before/after-springs decoration,
//! using the same population Random and the world's shared Random.
//! This loaded-region adapter does not replace provider reentrant load events.
use super::{population::decoration::Decorator, random::JavaRandom};
use crate::{
    block_ticks::{BlockTicks, TickResult},
    world::World,
};
use glam::IVec3;

pub fn decorate(
    world: &mut World,
    ticks: &mut BlockTicks,
    population_random: &mut JavaRandom,
    world_random: &mut JavaRandom,
    origin: IVec3,
    biome: u8,
) -> TickResult {
    let mut result = TickResult::default();
    if !Decorator::generates_springs(biome) {
        return result;
    }
    for fluid in [8, 10] {
        for _ in 0..if fluid == 8 { 50 } else { 20 } {
            let x = population_random.next_int(16) + 8;
            let z = population_random.next_int(16) + 8;
            let bound = if fluid == 8 {
                population_random.next_int(248) + 8
            } else {
                let inner = population_random.next_int(240) + 8;
                population_random.next_int(inner) + 8
            };
            let y = population_random.next_int(bound);
            let (_, mut changes) = ticks.generate_spring(
                world,
                origin + IVec3::new(x, y, z),
                fluid,
                population_random,
                world_random,
            );
            result.changed.append(&mut changes.changed);
            result.drops.append(&mut changes.drops);
            result.washed.append(&mut changes.washed);
        }
    }
    result
}
