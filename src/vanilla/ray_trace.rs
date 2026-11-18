//! Original 1.8.9 Block.collisionRayTrace and World.rayTraceBlocks.
//! Block selection uses local double coordinates and float-derived bounds.
//! Stairs test six octants in their original order, independently of stair
//! movement collision corners. Liquid picking and returning a final miss are
//! explicit options because the original player and item paths differ.
use super::collision_shapes;
pub use super::collision_shapes::BlockView as Access;
use glam::{DVec3, IVec3};
include!("ray_trace_data.rs");
#[cfg(test)]
#[path = "ray_trace_tests.rs"]
mod tests;
const FACES: [IVec3; 6] = [
    IVec3::NEG_Y,
    IVec3::Y,
    IVec3::NEG_Z,
    IVec3::Z,
    IVec3::NEG_X,
    IVec3::X,
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Miss,
    Block,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub kind: Kind,
    pub position: IVec3,
    pub face: u8,
    pub point: DVec3,
}
impl Hit {
    pub fn adjacent(self) -> IVec3 {
        self.position + FACES[usize::from(self.face)]
    }
    pub fn cursor(self) -> [f32; 3] {
        (self.point - self.position.as_dvec3()).as_vec3().to_array()
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub stop_on_liquid: bool,
    pub ignore_without_box: bool,
    pub return_last_miss: bool,
}
impl Options {
    pub const PICK: Self = Self {
        stop_on_liquid: false,
        ignore_without_box: false,
        return_last_miss: true,
    };
    pub fn item(stop_on_liquid: bool) -> Self {
        Self {
            stop_on_liquid,
            ignore_without_box: !stop_on_liquid,
            return_last_miss: false,
        }
    }
}
fn clean(v: DVec3) -> DVec3 {
    DVec3::from_array(v.to_array().map(|v| if v == 0. { 0. } else { v }))
}
fn square_distance(a: DVec3, b: DVec3) -> f64 {
    let v = a - b;
    v.x * v.x + v.y * v.y + v.z * v.z
}
fn java_floor(v: f64) -> i32 {
    let i = v as i32;
    if v < i as f64 {
        i.wrapping_sub(1)
    } else {
        i
    }
}
fn cell(v: DVec3) -> IVec3 {
    IVec3::from_array(v.to_array().map(java_floor))
}
fn is_stair(state: u16) -> bool {
    matches!(
        state >> 4,
        53 | 67 | 108 | 109 | 114 | 128 | 134..=136 | 156 | 163 | 164 | 180
    )
}
fn skipped(state: u16) -> [u8; 2] {
    let horizontal = [3, 1, 0, 2][usize::from(state & 3)];
    [
        [4, 5],
        [5, 7],
        [6, 7],
        [4, 6],
        [0, 1],
        [1, 3],
        [2, 3],
        [0, 2],
    ][horizontal + if state & 4 != 0 { 4 } else { 0 }]
}
fn octant(pass: u8) -> [f64; 6] {
    let x = f64::from(pass & 1) * 0.5;
    let y = f64::from((pass >> 2) & 1) * 0.5;
    let z = f64::from((pass >> 1) & 1) * 0.5;
    [x, y, z, x + 0.5, y + 0.5, z + 0.5]
}
fn local(b: super::movement::Aabb, p: IVec3) -> [f64; 6] {
    let a = b.min - p.as_dvec3();
    let z = b.max - p.as_dvec3();
    [a.x, a.y, a.z, z.x, z.y, z.z]
}
pub(super) fn selection_bounds(w: &impl Access, p: IVec3, state: u16) -> [f64; 6] {
    let id = state >> 4;
    let get = |q| w.get(q);
    match id {
        54 | 146 => return local(collision_shapes::chest(p, state, &get), p),
        64 | 71 | 193..=197 => return local(collision_shapes::door(p, state, &get), p),
        85 | 101 | 102 | 113 | 139 | 160 | 188..=192 => {
            let [n, e, s, v] = [IVec3::NEG_Z, IVec3::X, IVec3::Z, IVec3::NEG_X]
                .map(|d| collision_shapes::connected(state, get(p + d)));
            let (lo, hi) = if id == 139 {
                (0.25, 0.75)
            } else if matches!(id, 101 | 102 | 160) {
                (0.4375, 0.5625)
            } else {
                (0.375, 0.625)
            };
            let isolated = matches!(id, 101 | 102 | 160) && !n && !e && !s && !v;
            let mut b = [
                if v || isolated { 0. } else { lo },
                0.,
                if n || isolated { 0. } else { lo },
                if e || isolated { 1. } else { hi },
                1.,
                if s || isolated { 1. } else { hi },
            ];
            if id == 139 && n && s && !e && !v {
                b[0] = 0.3125;
                b[3] = 0.6875;
                b[4] = 0.8125;
            } else if id == 139 && e && v && !n && !s {
                b[2] = 0.3125;
                b[5] = 0.6875;
                b[4] = 0.8125;
            }
            return b;
        }
        106 if state & 15 == 0 => {
            let up = get(p + IVec3::Y);
            if super::material::full_cube(up) && super::material::blocks_movement(up) {
                return [0., 0.9375, 0., 1., 1., 1.];
            }
        }
        _ if is_stair(state) => {
            return octant(
                (0..8)
                    .rev()
                    .find(|pass| !skipped(state).contains(pass))
                    .unwrap(),
            )
        }
        _ => {}
    }
    BOXES[usize::from(BOX.get(state as usize).copied().unwrap_or(BOX[0]))]
}
/// The box returned by Block.getSelectedBoundingBox after the crosshair ray hit.
/// Most blocks use the bounds prepared during the pick. Stairs delegate to
/// their full model block, while cake and cactus compute a world-space box.
pub(crate) fn outline_bounds(w: &impl Access, p: IVec3, state: u16) -> super::movement::Aabb {
    let id = state >> 4;
    if matches!(id, 81 | 92) {
        if let Some(mut b) = collision_shapes::single_box_for(p, state, |q| w.get(q), None) {
            if id == 81 {
                b.max.y = f64::from(p.y) + 1.;
            }
            return b;
        }
    }
    let coords = if is_stair(state) {
        [0., 0., 0., 1., 1., 1.]
    } else {
        selection_bounds(w, p, state)
    };
    // AxisAlignedBB orders the endpoints. An unsupported vine can leave
    // inverted bounds for the ray test but returns a normalized outline.
    let b = collision_shapes::box_at(p, coords);
    super::movement::Aabb {
        min: b.min.min(b.max),
        max: b.min.max(b.max),
    }
}
pub(crate) fn intercept(b: [f64; 6], a: DVec3, end: DVec3) -> Option<(u8, DVec3)> {
    let delta = end - a;
    let mut best: Option<(f64, u8, DVec3)> = None;
    for (face, axis, index) in [
        (4, 0, 0),
        (5, 0, 3),
        (0, 1, 1),
        (1, 1, 4),
        (2, 2, 2),
        (3, 2, 5),
    ] {
        if delta[axis] * delta[axis] < 1.0e-7f32 as f64 {
            continue;
        }
        let t = (b[index] - a[axis]) / delta[axis];
        if !(0.0..=1.0).contains(&t) {
            continue;
        }
        let point = clean(a + delta * t);
        if (0..3).any(|i| i != axis && !(point[i] >= b[i] && point[i] <= b[i + 3])) {
            continue;
        }
        let distance = square_distance(a, point);
        if best.is_none_or(|v| distance < v.0) {
            best = Some((distance, face, point));
        }
    }
    best.map(|(_, face, point)| (face, point))
}
fn rectangle(p: IVec3, b: [f64; 6], from: DVec3, to: DVec3) -> Option<Hit> {
    let a = clean(from - p.as_dvec3());
    let end = clean(to - p.as_dvec3());
    intercept(b, a, end).map(|(face, point)| Hit {
        kind: Kind::Block,
        position: p,
        face,
        point: clean(point + p.as_dvec3()),
    })
}
pub fn block(w: &impl Access, p: IVec3, state: u16, from: DVec3, to: DVec3) -> Option<Hit> {
    if state >> 4 == 36 {
        return None;
    }
    if is_stair(state) {
        let mut best = None;
        let mut farthest = 0.;
        let absent = skipped(state);
        for pass in 0..8 {
            if absent.contains(&pass) {
                continue;
            }
            if let Some(hit) = rectangle(p, octant(pass), from, to) {
                let d = square_distance(hit.point, to);
                if d > farthest {
                    farthest = d;
                    best = Some(hit);
                }
            }
        }
        return best;
    }
    rectangle(p, selection_bounds(w, p, state), from, to)
}
fn has_box(w: &impl Access, p: IVec3, state: u16, flags: u8) -> bool {
    if state >> 4 == 36 {
        return collision_shapes::single_box_for(p, state, |q| w.get(q), w.piston(p)).is_some();
    }
    flags & 4 != 0
}
pub fn trace(w: &impl Access, from: DVec3, to: DVec3, options: Options) -> Option<Hit> {
    let mut start = clean(from);
    let end = clean(to);
    if start.is_nan() || end.is_nan() {
        return None;
    }
    let target = cell(end);
    let mut p = cell(start);
    let state = w.get(p);
    let pick_bit = if options.stop_on_liquid { 2 } else { 1 };
    let flags = FLAGS.get(state as usize).copied().unwrap_or(0);
    if (!options.ignore_without_box || has_box(w, p, state, flags)) && flags & pick_bit != 0 {
        if let Some(hit) = block(w, p, state, start, end) {
            return Some(hit);
        }
    }
    let mut miss = None;
    for _ in 0..=200 {
        if start.is_nan() {
            return None;
        }
        if p == target {
            break;
        }
        let delta = end - start;
        let mut planes = [999.; 3];
        let mut times = [999.; 3];
        for axis in 0..3 {
            if p[axis] != target[axis] {
                planes[axis] = p[axis] as f64 + if target[axis] > p[axis] { 1. } else { 0. };
                let t = (planes[axis] - start[axis]) / delta[axis];
                times[axis] = if t == 0. { -1.0e-4 } else { t };
            }
        }
        let axis = if times[0] < times[1] && times[0] < times[2] {
            0
        } else if times[1] < times[2] {
            1
        } else {
            2
        };
        let face = if target[axis] > p[axis] {
            [4, 0, 2][axis]
        } else {
            [5, 1, 3][axis]
        };
        start += delta * times[axis];
        start[axis] = planes[axis];
        start = clean(start);
        p = cell(start);
        if matches!(face, 5 | 1 | 3) {
            p[axis] = p[axis].wrapping_sub(1);
        }
        let state = w.get(p);
        let flags = FLAGS.get(state as usize).copied().unwrap_or(0);
        if options.ignore_without_box && !has_box(w, p, state, flags) {
            continue;
        }
        if flags & pick_bit != 0 {
            if let Some(hit) = block(w, p, state, start, end) {
                return Some(hit);
            }
        } else {
            miss = Some(Hit {
                kind: Kind::Miss,
                position: p,
                face,
                point: start,
            });
        }
    }
    if options.return_last_miss {
        miss
    } else {
        None
    }
}
