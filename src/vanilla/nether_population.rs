//! ChunkProviderHell population after MapGenNetherBridge.generateStructure.
//! Provider Random is deliberately not reseeded here. Set the world's original
//! fallInstantly flag around fortress + this stage; callbacks may reenter it.
use crate::vanilla::{
    population::{self, FeatureWorld},
    random::JavaRandom,
    structure_common::full_block,
};
use glam::IVec3;

pub trait NetherWorld: FeatureWorld {
    /// Original forceBlockUpdateTick after a flags=2 flowing-lava placement.
    /// The implementation must run the immediate update with this Random and
    /// retain its world Random for nested onBlockAdded scheduling callbacks.
    fn force_lava(&mut self, p: IVec3, random: &mut JavaRandom);
}
fn offset(random: &mut JavaRandom) -> IVec3 {
    IVec3::new(
        random.next_int(8) - random.next_int(8),
        random.next_int(4) - random.next_int(4),
        random.next_int(8) - random.next_int(8),
    )
}
pub fn fire(world: &mut impl FeatureWorld, random: &mut JavaRandom, origin: IVec3) -> bool {
    for _ in 0..64 {
        let p = origin + offset(random);
        if world.get(p) == 0 && world.get(p - IVec3::Y) >> 4 == 87 {
            world.set(p, 51 << 4);
        }
    }
    true
}
/// WorldGenGlowStone1 and WorldGenGlowStone2 have identical 1.8.9 bytecode.
pub fn glowstone(world: &mut impl FeatureWorld, random: &mut JavaRandom, origin: IVec3) -> bool {
    if world.get(origin) != 0 || world.get(origin + IVec3::Y) >> 4 != 87 {
        return false;
    }
    world.set(origin, 89 << 4);
    let neighbors = [
        -IVec3::Y,
        IVec3::Y,
        -IVec3::Z,
        IVec3::Z,
        -IVec3::X,
        IVec3::X,
    ];
    for _ in 0..1500 {
        let p = origin
            + IVec3::new(
                random.next_int(8) - random.next_int(8),
                -random.next_int(12),
                random.next_int(8) - random.next_int(8),
            );
        if world.get(p) != 0 {
            continue;
        }
        let mut count = 0;
        for d in neighbors {
            if world.get(p + d) >> 4 == 89 {
                count += 1;
            }
            if count > 1 {
                break;
            }
        }
        if count == 1 {
            world.set(p, 89 << 4);
        }
    }
    true
}
pub fn mushroom(
    world: &mut impl FeatureWorld,
    random: &mut JavaRandom,
    origin: IVec3,
    state: u16,
) -> bool {
    for _ in 0..64 {
        let p = origin + offset(random);
        // GeneratorBushFeature's Nether no-sky guard precedes canBlockStay.
        if world.get(p) != 0 || !(0..255).contains(&p.y) {
            continue;
        }
        let below = world.get(p - IVec3::Y);
        if below >> 4 == 110
            || below == (3 << 4 | 2)
            || world.light(p, false) < 13 && full_block(below)
        {
            world.set(p, state);
        }
    }
    true
}
pub fn lava(
    world: &mut impl NetherWorld,
    random: &mut JavaRandom,
    p: IVec3,
    enclosed: bool,
) -> bool {
    if world.get(p + IVec3::Y) >> 4 != 87 {
        return false;
    }
    if world.get(p) != 0 && world.get(p) >> 4 != 87 {
        return false;
    }
    let neighbors = [-IVec3::X, IVec3::X, -IVec3::Z, IVec3::Z, -IVec3::Y];
    let mut rack = 0;
    for d in neighbors {
        if world.get(p + d) >> 4 == 87 {
            rack += 1;
        }
    }
    let mut air = 0;
    for d in neighbors {
        if world.get(p + d) == 0 {
            air += 1;
        }
    }
    if !enclosed && rack == 4 && air == 1 || rack == 5 {
        world.set(p, 10 << 4);
        world.force_lava(p, random);
    }
    true
}
fn position(
    random: &mut JavaRandom,
    origin: IVec3,
    offset: i32,
    height: i32,
    bottom: i32,
) -> IVec3 {
    origin
        + IVec3::new(
            random.next_int(16) + offset,
            random.next_int(height) + bottom,
            random.next_int(16) + offset,
        )
}
pub fn after_fortress(world: &mut impl NetherWorld, random: &mut JavaRandom, cx: i32, cz: i32) {
    let origin = IVec3::new(cx.wrapping_mul(16), 0, cz.wrapping_mul(16));
    for _ in 0..8 {
        let p = position(random, origin, 8, 120, 4);
        lava(world, random, p, false);
    }
    // These are condition expressions, not counts sampled once before a loop.
    let mut i = 0;
    loop {
        let bound = random.next_int(10) + 1;
        let limit = random.next_int(bound) + 1;
        if i >= limit {
            break;
        }
        let p = position(random, origin, 8, 120, 4);
        fire(world, random, p);
        i += 1;
    }
    let mut i = 0;
    loop {
        let bound = random.next_int(10) + 1;
        let limit = random.next_int(bound);
        if i >= limit {
            break;
        }
        let p = position(random, origin, 8, 120, 4);
        glowstone(world, random, p);
        i += 1;
    }
    for _ in 0..10 {
        let p = position(random, origin, 8, 128, 0);
        glowstone(world, random, p);
    }
    for state in [39 << 4, 40 << 4] {
        if random.next_boolean() {
            let p = position(random, origin, 8, 128, 0);
            mushroom(world, random, p, state);
        }
    }
    for _ in 0..16 {
        let p = position(random, origin, 0, 108, 10);
        population::minable_in(world, random, p, 153 << 4, 14, 87);
    }
    for _ in 0..16 {
        let p = position(random, origin, 0, 108, 10);
        lava(world, random, p, true);
    }
}

#[cfg(test)]
#[path = "dimension_population_tests.rs"]
mod tests;
