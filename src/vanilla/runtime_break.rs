//! BlockLeaves/BlockLog.breakBlock runs after the raw replacement and before
//! Chunk relighting. Flag-4 neighbor leaf metadata writes do not notify neighbors.
use super::*;
pub(super) fn leaves(
    world: &mut impl BlockAccess,
    ticks: &mut BlockTicks,
    p: IVec3,
    old: u16,
    new: u16,
) {
    if old >> 4 == new >> 4 {
        return;
    }
    let radius = match old >> 4 {
        18 | 161 => 1,
        17 | 162 => 4,
        _ => return,
    };
    let area = radius + 1;
    if p.y + area < 0
        || p.y - area >= 256
        || !((p.x - area).div_euclid(16)..=(p.x + area).div_euclid(16)).all(|x| {
            ((p.z - area).div_euclid(16)..=(p.z + area).div_euclid(16))
                .all(|z| world.chunk_loaded((x, z)))
        })
    {
        return;
    }
    let mut mark = |at: IVec3| {
        let state = world.get(at);
        if super::super::material::is_leaves(state) && state & 8 == 0 {
            ticks.set_block(world, at, state | 8, false);
        }
    };
    if radius == 1 {
        for x in -radius..=radius {
            for y in -radius..=radius {
                for z in -radius..=radius {
                    mark(p + IVec3::new(x, y, z));
                }
            }
        }
    } else {
        // BlockPos.getAllInBox increments X first, then Y, then Z.
        for z in -radius..=radius {
            for y in -radius..=radius {
                for x in -radius..=radius {
                    mark(p + IVec3::new(x, y, z));
                }
            }
        }
    }
}
