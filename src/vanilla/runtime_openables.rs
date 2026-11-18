//! Original support and neighbor callbacks for doors, trapdoors and fence gates.
use crate::vanilla::{material, rail_events::RailWorld};
use glam::IVec3;

pub(super) fn changed(
    world: &mut impl RailWorld,
    p: IVec3,
    state: u16,
    source: u16,
) -> Option<(IVec3, i32)> {
    let id = state >> 4;
    let power_source = material::can_provide_power(source << 4);
    match id {
        26 => {
            let facing = [IVec3::Z, IVec3::NEG_X, IVec3::NEG_Z, IVec3::X][(state & 3) as usize];
            if state & 8 != 0 {
                if world.get(p - facing) >> 4 != 26 {
                    world.set(p, 0, 3);
                }
            } else if world.get(p + facing) >> 4 != 26 {
                world.set(p, 0, 3);
                world.drop_block(p, state);
            }
        }
        64 | 71 | 193..=197 => {
            if state & 8 != 0 {
                let below = p - IVec3::Y;
                let lower = world.get(below);
                if lower >> 4 != id {
                    world.set(p, 0, 3);
                } else if source != id {
                    return changed(world, below, lower, source);
                }
                return None;
            }
            let above = p + IVec3::Y;
            let upper = world.get(above);
            let mut removed = false;
            if upper >> 4 != id {
                world.set(p, 0, 3);
                removed = true;
            }
            if !world.solid_top(p - IVec3::Y) {
                world.set(p, 0, 3);
                removed = true;
                if upper >> 4 == id {
                    world.set(above, 0, 3);
                }
            }
            if removed {
                world.drop_block(p, state);
                return None;
            }
            let powered = world.is_powered(p) || world.is_powered(above);
            if (powered || power_source) && source != id && powered != (upper & 2 != 0) {
                world.set(above, (upper & !2) | (u16::from(powered) << 1), 2);
                if powered != (state & 4 != 0) {
                    world.set(p, state ^ 4, 2);
                    return Some((p, if powered { 1003 } else { 1006 }));
                }
            }
        }
        96 | 167 => {
            let support =
                p + [IVec3::Z, IVec3::NEG_Z, IVec3::X, IVec3::NEG_X][(state & 3) as usize];
            let neighbor = world.get(support);
            let valid = material::opaque(neighbor) && material::full_cube(neighbor)
                || matches!(neighbor>>4,43|44|53|67|89|108|109|114|125|126|128|134..=136|156|163|164|180..=182);
            if !valid {
                world.set(p, 0, 3);
                world.drop_block(p, state);
                return None;
            }
            let powered = world.is_powered(p);
            if (powered || power_source) && powered != (state & 4 != 0) {
                world.set(p, state ^ 4, 2);
                return Some((p, if powered { 1003 } else { 1006 }));
            }
        }
        107 | 183..=187 => {
            let powered = world.is_powered(p);
            if powered || power_source {
                let open = state & 4 != 0;
                let remembered = state & 8 != 0;
                if powered && !open && !remembered {
                    world.set(p, state | 12, 2);
                    return Some((p, 1003));
                }
                if !powered && open && remembered {
                    world.set(p, state & !12, 2);
                    return Some((p, 1006));
                }
                if powered != remembered {
                    world.set(p, state ^ 8, 2);
                }
            }
        }
        _ => {}
    }
    None
}
