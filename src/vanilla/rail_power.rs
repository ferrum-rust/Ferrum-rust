//! Original powered/activator rail propagation, bounded to eight rail edges.
use super::{shape, RailWorld};
use glam::IVec3;
fn connected(
    world: &impl RailWorld,
    p: IVec3,
    id: u16,
    direction: bool,
    depth: i32,
    axis: u16,
) -> bool {
    let state = world.get(p);
    if state >> 4 != id {
        return false;
    }
    let next_shape = shape(state);
    if axis == 1 && matches!(next_shape, 0 | 4 | 5) || axis == 0 && matches!(next_shape, 1..=3) {
        return false;
    }
    state & 8 != 0 && (world.is_powered(p) || scan(world, p, state, direction, depth + 1))
}
fn scan(world: &impl RailWorld, p: IVec3, state: u16, direction: bool, depth: i32) -> bool {
    if depth >= 8 {
        return false;
    }
    let mut q = p;
    let mut lower = true;
    let mut axis = shape(state);
    match axis {
        0 => q.z += if direction { 1 } else { -1 },
        1 => q.x += if direction { -1 } else { 1 },
        2 => {
            if direction {
                q.x -= 1;
            } else {
                q.x += 1;
                q.y += 1;
                lower = false;
            }
            axis = 1;
        }
        3 => {
            if direction {
                q.x -= 1;
                q.y += 1;
                lower = false;
            } else {
                q.x += 1;
            }
            axis = 1;
        }
        4 => {
            if direction {
                q.z += 1;
            } else {
                q.z -= 1;
                q.y += 1;
                lower = false;
            }
            axis = 0;
        }
        5 => {
            if direction {
                q.z += 1;
                q.y += 1;
                lower = false;
            } else {
                q.z -= 1;
            }
            axis = 0;
        }
        _ => {}
    }
    connected(world, q, state >> 4, direction, depth, axis)
        || lower && connected(world, q - IVec3::Y, state >> 4, direction, depth, axis)
}
pub(super) fn neighbor(world: &mut impl RailWorld, p: IVec3, state: u16) {
    let powered =
        world.is_powered(p) || scan(world, p, state, true, 0) || scan(world, p, state, false, 0);
    if powered != (state & 8 != 0) {
        world.set(p, if powered { state | 8 } else { state & !8 }, 3);
        world.notify_neighbors(p - IVec3::Y, state >> 4);
        if (2..=5).contains(&shape(state)) {
            world.notify_neighbors(p + IVec3::Y, state >> 4);
        }
    }
}
