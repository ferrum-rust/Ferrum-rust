//! Original block collision boxes in emission order. Registry bounds come
//! from original 1.8.9 classes. Neighbor-dependent blocks are evaluated at the
//! queried position. Position-prepared bounds are used for chest/anvil, whose
//! original singleton bounds can otherwise depend on a previous render query.
use super::{material, movement::Aabb};
use anyhow::{ensure, Result};
use glam::{DVec3, IVec3};
use serde::{Deserialize, Serialize};
#[path = "collision_shape_data.rs"]
mod data;
#[cfg(test)]
#[path = "collision_shape_tests.rs"]
mod tests;
const SIDES: [IVec3; 4] = [IVec3::NEG_Z, IVec3::X, IVec3::Z, IVec3::NEG_X];
const FACING: [IVec3; 6] = [
    IVec3::NEG_Y,
    IVec3::Y,
    IVec3::NEG_Z,
    IVec3::Z,
    IVec3::NEG_X,
    IVec3::X,
];

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PistonTile {
    pub carried: u16,
    pub facing: u8,
    pub extending: bool,
    pub progress: f32,
}
pub trait BlockView {
    fn get(&self, p: IVec3) -> u16;
    fn piston(&self, _p: IVec3) -> Option<PistonTile> {
        None
    }
}
impl BlockView for crate::world::World {
    fn get(&self, p: IVec3) -> u16 {
        self.get(p)
    }
    fn piston(&self, p: IVec3) -> Option<PistonTile> {
        self.piston_at(p)
    }
}
/// Old Ferrum placements sometimes wrote item damage as block metadata.
/// Decode those aliases only when loading our local edits. Original section
/// data uses the packed state registry directly, including its unmapped IDs.
pub fn legacy_overlay_state(raw: u16) -> u16 {
    let index = usize::from(raw);
    if data::STATIC_SHAPE.get(index) == Some(&255) {
        if let Ok(decoded) = u16::try_from(data::DECODE_META[index]) {
            if decoded >> 4 == raw >> 4 {
                return decoded;
            }
        }
    }
    raw
}
pub(super) fn carried_state(id: i32, meta: i32) -> Result<u16> {
    ensure!(
        (0..16).contains(&meta),
        "Invalid moving piston block metadata {meta}"
    );
    let state = if (0..256).contains(&id) {
        data::DECODE_META[(id * 16 + meta) as usize]
    } else {
        0
    };
    ensure!(
        state >= 0,
        "Invalid moving piston carried state {id}:{meta}"
    );
    Ok(state as u16)
}
impl PistonTile {
    pub fn from_nbt(tag: &crate::nbt::Tag) -> Result<Self> {
        let int = |name| super::food::nbt_int(tag.get(name));
        let id = int("blockId");
        let meta = int("blockData");
        let carried = carried_state(id, meta)?;
        let facing = (int("facing") % 6).unsigned_abs() as u8;
        let progress = tag
            .get("progress")
            .and_then(crate::nbt::Tag::number)
            .unwrap_or(0.) as f32;
        ensure!(progress.is_finite(), "Non-finite piston progress");
        ensure!(
            carried_shape(carried, facing) >= 0,
            "Original moving piston collision cannot read carried state {id}:{meta}"
        );
        Ok(Self {
            carried,
            facing,
            extending: int("extending") as i8 != 0,
            progress,
        })
    }
}
fn carried_shape(state: u16, face: u8) -> i16 {
    data::CARRIED_INDEX
        .get(state as usize)
        .and_then(|i| data::CARRIED_SHAPES[*i as usize].get(face as usize))
        .copied()
        .unwrap_or(-1)
}
pub(super) fn box_at(p: IVec3, coords: [f64; 6]) -> Aabb {
    let at = p.as_dvec3();
    Aabb {
        min: at + DVec3::new(coords[0], coords[1], coords[2]),
        max: at + DVec3::new(coords[3], coords[4], coords[5]),
    }
}
fn numeric_box(p: IVec3, state: u16, coords: [f64; 6]) -> Aabb {
    let id = state >> 4;
    if matches!(id, 81 | 92) {
        let lo = DVec3::new(
            (p.x as f32 + coords[0] as f32) as f64,
            p.y as f64,
            (p.z as f32 + coords[2] as f32) as f64,
        );
        let hi = DVec3::new(
            ((p.x + 1) as f32 - (1. - coords[3]) as f32) as f64,
            if id == 81 {
                ((p.y + 1) as f32 - 0.0625) as f64
            } else {
                (p.y as f32 + 0.5) as f64
            },
            ((p.z + 1) as f32 - (1. - coords[5]) as f32) as f64,
        );
        return Aabb {
            min: lo.min(hi),
            max: lo.max(hi),
        };
    }
    let mut bounds = box_at(p, coords);
    if matches!(id, 107 | 183..=187) {
        let axis = if state & 1 == 0 { 2 } else { 0 };
        bounds.min[axis] = (p[axis] as f32 + 0.375) as f64;
        bounds.max[axis] = (p[axis] as f32 + 0.625) as f64;
        bounds.max.y = (p.y as f32 + 1.5) as f64;
    }
    bounds
}
fn is_stair(state: u16) -> bool {
    matches!(
        state >> 4,
        53 | 67 | 108 | 109 | 114 | 128 | 134 | 135 | 136 | 156 | 163 | 164 | 180
    )
}
fn stair_direction(state: u16) -> IVec3 {
    [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z][(state & 3) as usize]
}
fn half(coords: &mut [f64; 6], dir: IVec3) {
    let axis = if dir.x != 0 { 0 } else { 2 };
    if dir[axis] > 0 {
        coords[axis] = 0.5;
    } else {
        coords[axis + 3] = 0.5;
    }
}
fn stairs(p: IVec3, state: u16, get: &impl Fn(IVec3) -> u16, mut emit: impl FnMut(Aabb)) {
    let top = state & 4 != 0;
    let base = if top { 0.5 } else { 0. };
    let step = 0.5 - base;
    emit(box_at(p, [0., base, 0., 1., base + 0.5, 1.]));
    let front = stair_direction(state);
    let same = |q| {
        let other = get(q);
        is_stair(other) && other & 7 == state & 7
    };
    let perpendicular = |q| {
        let other = get(q);
        let direction = stair_direction(other);
        (is_stair(other) && other & 4 == state & 4 && front.dot(direction) == 0)
            .then_some(direction)
    };
    let mut upper = [0., step, 0., 1., step + 0.5, 1.];
    half(&mut upper, front);
    if let Some(turn) = perpendicular(p + front).filter(|turn| !same(p - *turn)) {
        half(&mut upper, turn);
        emit(box_at(p, upper));
        return;
    }
    emit(box_at(p, upper));
    if let Some(turn) = perpendicular(p - front).filter(|turn| !same(p + *turn)) {
        let mut corner = [0., step, 0., 1., step + 0.5, 1.];
        half(&mut corner, -front);
        half(&mut corner, turn);
        emit(box_at(p, corner));
    }
}
pub(crate) fn connected(center: u16, other: u16) -> bool {
    let id = center >> 4;
    let next = other >> 4;
    if matches!(id, 101 | 102 | 160) {
        return material::full_block(other) || matches!(next, 20 | 95 | 101 | 102 | 160);
    }
    if next == 166 {
        return false;
    }
    if matches!(next, 107 | 183..=187) {
        return true;
    }
    let same = match id {
        139 => next == 139,
        113 => next == 113,
        _ => matches!(next, 85 | 188..=192),
    };
    same || (material::opaque(other)
        && material::full_cube(other)
        && !matches!(next, 86 | 91 | 103))
}
fn connections(p: IVec3, state: u16, get: &impl Fn(IVec3) -> u16, mut emit: impl FnMut(Aabb)) {
    let [n, e, s, w] = SIDES.map(|dir| connected(state, get(p + dir)));
    let id = state >> 4;
    if id == 139 {
        let (mut xl, mut xh) = (if w { 0. } else { 0.25 }, if e { 1. } else { 0.75 });
        let (mut zl, mut zh) = (if n { 0. } else { 0.25 }, if s { 1. } else { 0.75 });
        if n && s && !w && !e {
            (xl, xh) = (0.3125, 0.6875);
        } else if w && e && !n && !s {
            (zl, zh) = (0.3125, 0.6875);
        }
        emit(box_at(p, [xl, 0., zl, xh, 1.5, zh]));
        return;
    }
    if matches!(id, 101 | 102 | 160) {
        let isolated = !n && !e && !s && !w;
        if w || e || isolated {
            emit(box_at(
                p,
                [
                    if w || isolated { 0. } else { 0.5 },
                    0.,
                    0.4375,
                    if e || isolated { 1. } else { 0.5 },
                    1.,
                    0.5625,
                ],
            ));
        }
        if n || s || isolated {
            emit(box_at(
                p,
                [
                    0.4375,
                    0.,
                    if n || isolated { 0. } else { 0.5 },
                    0.5625,
                    1.,
                    if s || isolated { 1. } else { 0.5 },
                ],
            ));
        }
        return;
    }
    if n || s {
        emit(box_at(
            p,
            [
                0.375,
                0.,
                if n { 0. } else { 0.375 },
                0.625,
                1.5,
                if s { 1. } else { 0.625 },
            ],
        ));
    }
    if w || e || (!n && !s) {
        emit(box_at(
            p,
            [
                if w { 0. } else { 0.375 },
                0.,
                0.375,
                if e { 1. } else { 0.625 },
                1.5,
                0.625,
            ],
        ));
    }
}
pub(super) fn door(p: IVec3, center: u16, get: &impl Fn(IVec3) -> u16) -> Aabb {
    let (lower, upper) = if center & 8 == 0 {
        (center, get(p + IVec3::Y))
    } else {
        (get(p - IVec3::Y), center)
    };
    let mut facing = lower & 3;
    if lower & 4 != 0 {
        facing = (facing + if upper & 1 == 0 { 1 } else { 3 }) & 3;
    }
    let mut coords = [0., 0., 0., 1., 1., 1.];
    match facing {
        0 => coords[3] = 0.1875,
        1 => coords[5] = 0.1875,
        2 => coords[0] = 0.8125,
        _ => coords[2] = 0.8125,
    }
    box_at(p, coords)
}
pub(super) fn chest(p: IVec3, state: u16, get: &impl Fn(IVec3) -> u16) -> Aabb {
    let mut coords = [0.0625, 0., 0.0625, 0.9375, 0.875, 0.9375];
    for (dir, index, edge) in [
        (IVec3::NEG_Z, 2, 0.),
        (IVec3::Z, 5, 1.),
        (IVec3::NEG_X, 0, 0.),
        (IVec3::X, 3, 1.),
    ] {
        if get(p + dir) >> 4 == state >> 4 {
            coords[index] = edge;
            break;
        }
    }
    box_at(p, coords)
}
fn moving(p: IVec3, tile: PistonTile, get: &impl Fn(IVec3) -> u16) -> Option<Aabb> {
    let idx = carried_shape(tile.carried, (get(p) & 7) as u8);
    let coords = data::SHAPES.get(usize::try_from(idx).ok()?)?.first()?;
    let mut bounds = match tile.carried >> 4 {
        64 | 71 | 193..=197 => door(p, get(p), get),
        139 => {
            let mut result = None;
            connections(p, tile.carried, get, |b| result = Some(b));
            result?
        }
        _ => numeric_box(p, tile.carried, *coords),
    };
    let progress = if tile.extending {
        1. - tile.progress
    } else {
        tile.progress
    };
    let direction = FACING[usize::from(tile.facing)];
    for i in 0..3 {
        let amount = (direction[i] as f32 * progress) as f64;
        if direction[i] < 0 {
            bounds.min[i] -= amount;
        } else {
            bounds.max[i] -= amount;
        }
    }
    Some(Aabb {
        min: bounds.min.min(bounds.max),
        max: bounds.min.max(bounds.max),
    })
}
pub fn emit(
    p: IVec3,
    state: u16,
    get: impl Fn(IVec3) -> u16,
    tile: Option<PistonTile>,
    mut output: impl FnMut(Aabb),
) {
    let Some(&shape) = data::STATIC_SHAPE.get(state as usize) else {
        return;
    };
    if shape == 255 {
        return;
    }
    if is_stair(state) {
        stairs(p, state, &get, output);
        return;
    }
    match state >> 4 {
        85 | 101 | 102 | 113 | 139 | 160 | 188..=192 => connections(p, state, &get, output),
        64 | 71 | 193..=197 => output(door(p, state, &get)),
        54 | 146 => output(chest(p, state, &get)),
        36 => {
            if let Some(bounds) = tile.and_then(|t| moving(p, t, &get)) {
                output(bounds);
            }
        }
        _ => {
            for &coords in data::SHAPES[shape as usize] {
                output(numeric_box(p, state, coords));
            }
        }
    }
}

/// Single box requested by World.canBlockBePlaced before replacing the cell.
/// Compound movement boxes are a different original Block API. For blocks
/// whose singleton bounds survive an earlier render/selection operation, this
/// query uses the registry's initial bounds. The oracle records those cases.
pub fn placement_box(world: &impl BlockView, p: IVec3, block_id: u16) -> Result<Option<Aabb>> {
    let tile = if block_id == 36 {
        world.piston(p)
    } else {
        None
    };
    placement_for(p, block_id, |q| world.get(q), tile)
}
pub fn placement_for(
    p: IVec3,
    block_id: u16,
    get: impl Fn(IVec3) -> u16,
    tile: Option<PistonTile>,
) -> Result<Option<Aabb>> {
    let existing = get(p);
    let class = data::EXISTING_CLASS
        .get(existing as usize)
        .copied()
        .unwrap_or(data::EXISTING_CLASS[0]);
    let idx = data::PLACEMENT_SHAPE
        .get(block_id as usize)
        .map(|row| row[usize::from(class)])
        .unwrap_or(-1);
    ensure!(idx>=0,"Original single collision query for block {block_id} cannot read properties from state {existing}");
    let state = data::DEFAULT_STATE[block_id as usize];
    let result = match block_id {
        64 | 71 | 193..=197 => Some(door(p, existing, &get)),
        139 => {
            let mut result = None;
            connections(p, state, &get, |bounds| result = Some(bounds));
            result
        }
        36 => tile.and_then(|tile| moving(p, tile, &get)),
        _ => data::SHAPES[idx as usize]
            .first()
            .map(|&coords| numeric_box(p, state, coords)),
    };
    Ok(result)
}

/// Existing state's single collision box, as used by World.isBlockFullCube
/// and ray queries with ignoreBlockWithoutBoundingBox set. Uses the initial
/// singleton profile for bounds left over from previous render queries.
pub fn single_box_for(
    p: IVec3,
    state: u16,
    get: impl Fn(IVec3) -> u16,
    tile: Option<PistonTile>,
) -> Option<Aabb> {
    match state >> 4 {
        64 | 71 | 193..=197 => Some(door(p, state, &get)),
        139 => {
            let mut found = None;
            connections(p, state, &get, |b| found = Some(b));
            found
        }
        36 => tile.and_then(|t| moving(p, t, &get)),
        _ => data::SINGLE_SHAPE
            .get(state as usize)
            .and_then(|&i| data::SHAPES[usize::from(i)].first())
            .map(|&coords| numeric_box(p, state, coords)),
    }
}
pub fn full_collision_cube(
    p: IVec3,
    state: u16,
    get: impl Fn(IVec3) -> u16,
    tile: Option<PistonTile>,
) -> bool {
    single_box_for(p, state, get, tile).is_some_and(|b| {
        let d = b.max - b.min;
        (d.x + d.y + d.z) / 3. >= 1.
    })
}
