//! Original ItemBlock target selection, placement requirements and state
//! selection for the ordinary 1.8.9 item registry. ItemSlab and ItemSnow
//! have an extra combine path before invoking the base ItemBlock method.
use super::{collision_shapes, material, movement::Aabb, spawning::solid_top};
use anyhow::{ensure, Result};
use glam::{DVec3, IVec3};
include!("hand_placement_data.rs");
#[cfg(test)]
#[path = "hand_placement_tests.rs"]
mod tests;
const SIDES: [IVec3; 6] = [
    IVec3::NEG_Y,
    IVec3::Y,
    IVec3::NEG_Z,
    IVec3::Z,
    IVec3::NEG_X,
    IVec3::X,
];
const HORIZONTAL: [IVec3; 4] = [IVec3::NEG_Z, IVec3::X, IVec3::Z, IVec3::NEG_X];
pub trait Access: collision_shapes::BlockView {
    fn light(&self, p: IVec3) -> u8;
    fn loaded(&self, _p: IVec3) -> bool {
        true
    }
}
impl Access for crate::world::World {
    fn loaded(&self, p: IVec3) -> bool {
        self.chunks
            .contains_key(&(p.x.div_euclid(16), p.z.div_euclid(16)))
    }
    fn light(&self, p: IVec3) -> u8 {
        self.game_light(p, 0, false, true)
    }
}
pub fn supported(item: u16) -> bool {
    GENERIC.get(item as usize) == Some(&true) || matches!(item, 44 | 78 | 126 | 182 | 331 | 356 | 404) || door_block(item).is_some()
}
pub fn door_block(item: u16) -> Option<u16> {
    match item {
        324 => Some(64),
        330 => Some(71),
        427..=431 => Some(item - 234),
        _ => None,
    }
}
pub fn door_item(block: u16) -> Option<u16> {
    match block {
        64 => Some(324),
        71 => Some(330),
        193..=197 => Some(block + 234),
        _ => None,
    }
}
pub(crate) fn catches_fire(state:u16)->bool{CATCH_FIRE.get((state>>4)as usize)==Some(&1)}
/// SoundEventAccessorComposite owns a client-local RNG per event. Selecting
/// an original sample must not advance the world generation or entity RNG.
pub(crate) fn sample_clip(files:&'static [&'static str])->&'static str {
    if files.len() == 1 {
        return files[0];
    }
    use std::{
        cell::RefCell,
        collections::HashMap,
        time::{SystemTime, UNIX_EPOCH},
    };
    thread_local! {static EVENTS:RefCell<HashMap<usize,super::random::JavaRandom>>=RefCell::new(HashMap::new());}
    let index = EVENTS.with(|events| {
        events
            .borrow_mut()
            .entry(files.as_ptr() as usize)
            .or_insert_with(|| {
                let seed = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as i64;
                super::random::JavaRandom::new(seed)
            })
            .next_int(files.len() as i32) as usize
    });
    files[index]
}
static GLASS_BREAK_CLIPS: &[&str] = &["dig.glass"];

pub fn place_sound(state:u16)->(&'static str,f32,f32) {
    let (event,volume,pitch)=PLACE_SOUNDS[(state>>4) as usize];
    (PLACE_CLIPS[event][0],volume,pitch)
}

pub fn break_sound(state: u16) -> (&'static str, f32, f32) {
    let (event, volume, pitch) = PLACE_SOUNDS[((state >> 4) & 255) as usize];
    let clips = match event {
        8 => PLACE_CLIPS[5],
        9 => GLASS_BREAK_CLIPS,
        _ => PLACE_CLIPS[event],
    };
    (clips[0], volume, pitch)
}
pub(crate) fn replaceable_for_click(w: &impl Access, p: IVec3) -> bool {
    let state = w.get(p);
    if state >> 4 == 175 && state & 8 != 0 {
        let lower = w.get(p - IVec3::Y);
        if lower >> 4 == 175 && lower & 8 == 0 {
            return matches!(lower & 7, 2 | 3);
        }
    }
    TARGET_REPLACEABLE
        .get((state >> 4) as usize)
        .is_some_and(|mask| mask & (1 << (state & 15)) != 0)
}
fn normal_cube(w: &impl Access, p: IVec3) -> bool {
    if !(0..256).contains(&p.y)
        || !(-30_000_000..30_000_000).contains(&p.x)
        || !(-30_000_000..30_000_000).contains(&p.z)
        || !w.loaded(p)
    {
        return true;
    }
    let state = w.get(p);
    material::opaque(state) && material::full_cube(state)
}
fn fence(state: u16) -> bool {
    matches!(state >> 4, 85 | 113 | 188..=192)
}
fn stairs(state: u16) -> bool {
    matches!(
        state >> 4,
        53 | 67 | 108 | 109 | 114 | 128 | 134..=136 | 156 | 163 | 164 | 180
    )
}
fn torch_face(w: &impl Access, p: IVec3, side: u8) -> bool {
    if side == 0 {
        return false;
    }
    let q = p - SIDES[usize::from(side)];
    let support = w.get(q);
    if side == 1 {
        solid_top(support) || fence(support) || matches!(support >> 4, 20 | 95 | 139)
    } else {
        normal_cube(w, q)
    }
}
fn can_chest(w: &impl Access, p: IVec3, id: u16) -> bool {
    let mut neighbors = 0;
    for d in HORIZONTAL {
        let q = p + d;
        if w.get(q) >> 4 == id {
            neighbors += 1;
            if HORIZONTAL.iter().any(|dir| w.get(q + *dir) >> 4 == id) {
                return false;
            }
        }
    }
    neighbors <= 1
}
pub fn can_replace(w: &impl Access, p: IVec3, id: u16, side: u8) -> bool {
    let old = w.get(p);
    if id == 145 && material::is_circuits(old) {
        return true;
    }
    if !material::replaceable(old) || side >= 6 {
        return false;
    }
    let below = w.get(p - IVec3::Y);
    let floor = below >> 4;
    let attached = w.get(p - SIDES[usize::from(side)]);
    match id {
        6 | 31 | 37 | 38 => matches!(floor, 2 | 3 | 60),
        32 => matches!(floor, 3 | 12 | 159 | 172),
        39 | 40 => {
            (0..256).contains(&p.y)
                && material::full_block(below)
                && (floor == 110 || below == 3 << 4 | 2 || w.light(p) < 13)
        }
        59 | 104 | 105 | 141 | 142 => floor == 60,
        115 => floor == 88,
        111 => floor == 9,
        175 => matches!(floor, 2 | 3 | 60) && w.get(p + IVec3::Y) >> 4 == 0,
        27 | 28 | 66 | 157 | 86 | 91 | 93 | 94 | 140 | 149 | 150 => solid_top(below),
        70 | 72 | 147 | 148 => solid_top(below) || fence(below),
        50 | 75 | 76 => (1..6).any(|s| torch_face(w, p, s)),
        55 => solid_top(below) || floor == 89,
        54 | 146 => can_chest(w, p, id),
        64 | 71 | 193..=197 => {
            p.y < 255 && solid_top(below) && material::replaceable(w.get(p + IVec3::Y))
        }
        65 => HORIZONTAL
            .iter()
            .any(|d| material::normal_cube(w.get(p + *d))),
        69 | 77 | 143 => {
            if side == 1 {
                solid_top(attached)
            } else {
                material::normal_cube(attached)
            }
        }
        78 => {
            !matches!(floor, 79 | 174)
                && (material::is_leaves(below)
                    || floor == 78 && below & 15 >= 6
                    || material::opaque_cube(below) && material::blocks_movement(below))
        }
        81 => {
            matches!(floor, 12 | 81)
                && !HORIZONTAL.iter().any(|d| material::is_solid(w.get(p + *d)))
        }
        83 => {
            floor == 83
                || matches!(floor, 2 | 3 | 12)
                    && HORIZONTAL
                        .iter()
                        .any(|d| matches!(w.get(p + *d - IVec3::Y) >> 4, 8 | 9))
        }
        92 | 107 | 183..=187 => material::is_solid(below),
        96 | 167 => {
            side >= 2
                && (material::opaque(attached) && material::full_cube(attached)
                    || attached >> 4 == 89
                    || matches!(attached >> 4, 43 | 44 | 125 | 126 | 181 | 182)
                    || stairs(attached))
        }
        106 => {
            side != 0 && {
                let s = if side == 1 {
                    w.get(p + IVec3::Y)
                } else {
                    attached
                };
                material::full_cube(s) && material::blocks_movement(s)
            }
        }
        131 => side >= 2 && material::normal_cube(attached),
        171 => floor != 0,
        34 | 36 => false,
        51 => {
            solid_top(below)
                || SIDES
                    .iter()
                    .any(|d| CATCH_FIRE.get((w.get(p + *d) >> 4) as usize) == Some(&1))
        }
        63 | 68 | 176 | 177 => !HORIZONTAL.iter().any(|d| w.get(p + *d) >> 4 == 81),
        _ => true,
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Input {
    pub item: u16,
    pub meta: u16,
    pub count: u8,
    pub clicked: IVec3,
    pub side: u8,
    pub hit_y: f32,
    pub yaw: f32,
    pub player: DVec3,
    pub eye_height: f32,
}
impl Input {
    pub fn from_hit(
        player: &crate::player::Player,
        item: &crate::inventory::Stack,
        hit: super::ray_trace::Hit,
    ) -> Self {
        Self {
            item: item.id,
            meta: item.meta,
            count: item.count,
            clicked: hit.position,
            side: hit.face,
            hit_y: hit.cursor()[1],
            yaw: player.java_rotation()[0],
            player: DVec3::from_array(player.precise_position()),
            eye_height: if player.sneaking() {
                1.62f32 - 0.08f32
            } else {
                1.62f32
            },
        }
    }
}
#[derive(Default, Debug)]
pub struct Action {
    pub accepted: bool,
    pub consume: bool,
    pub writes: Vec<(IVec3, u16)>,
}
fn writable(w: &impl Access, p: IVec3) -> bool {
    (0..256).contains(&p.y)
        && (-30_000_000..30_000_000).contains(&p.x)
        && (-30_000_000..30_000_000).contains(&p.z)
        && w.loaded(p)
}
fn partly(w: &impl Access, p: IVec3, state: u16, blocked: bool) -> Action {
    let consume = !blocked && writable(w, p) && w.get(p) != state;
    Action {
        accepted: true,
        consume,
        writes: if consume {
            vec![(p, state)]
        } else {
            Vec::new()
        },
    }
}
fn outcome_state(w: &impl Access, p: IVec3, i: &Input) -> Result<i32> {
    ensure!(i.meta < 16, "Unsupported original item metadata {}", i.meta);
    let horizontal = (((i.yaw * 4. / 360.) as f64 + 0.5).floor() as i32 & 3) as usize;
    let close =
        (i.player.x as f32 - p.x as f32).abs() < 2. && (i.player.z as f32 - p.z as f32).abs() < 2.;
    let eyes = i.player.y + i.eye_height as f64 - p.y as f64;
    let vertical = if close && eyes > 2. {
        1
    } else if close && eyes < 0. {
        2
    } else {
        0
    };
    let mut side = i.side;
    if matches!(i.item, 50 | 75 | 76) && !torch_face(w, p, side) {
        side = [2, 5, 3, 4]
            .into_iter()
            .find(|s| torch_face(w, p, *s))
            .unwrap_or(1);
    }
    if i.item == 65 && (side < 2 || !material::normal_cube(w.get(p - SIDES[usize::from(side)]))) {
        side = [2, 5, 3, 4]
            .into_iter()
            .find(|s| material::normal_cube(w.get(p - SIDES[usize::from(*s)])))
            .unwrap_or(2);
    }
    let profile = PROFILE[i.item as usize][i.meta as usize] as usize;
    let mut index =
        (((side as usize * 4 + horizontal) * 3 + vertical) * 2) + usize::from(i.hit_y > 0.5);
    for &(length, state) in STATE_RUNS[profile] {
        if index < length as usize {
            ensure!(
                state >= 0,
                "Invalid original placed state for item {} metadata {}",
                i.item,
                i.meta
            );
            return Ok(state);
        }
        index -= length as usize;
    }
    unreachable!("original placement profile is complete")
}
pub fn plan(w: &impl Access, i: Input, blocked: impl Fn(Aabb) -> bool) -> Result<Action> {
    if i.count == 0 || i.side >= 6 || !supported(i.item) {
        return Ok(Action::default());
    }
    let dir = SIDES[i.side as usize];
    let clicked = w.get(i.clicked);
    let mut clicked_pos = i.clicked;
    if let Some(id) = door_block(i.item) {
        let lower = if replaceable_for_click(w, i.clicked) { i.clicked } else { i.clicked + dir };
        if i.side != 1 || !writable(w, lower) || !can_replace(w, lower, id, i.side) {
            return Ok(Action::default());
        }
        // ItemDoor faces the player's quadrant. Lower metadata encodes its clockwise rotation.
        let facing = (f64::from(i.yaw) / 90.0 + 0.5).floor() as i32 & 3;
        let clockwise = [IVec3::NEG_X, IVec3::NEG_Z, IVec3::X, IVec3::Z][facing as usize];
        let neighbors = |offset| [w.get(lower + offset), w.get(lower + offset + IVec3::Y)];
        let left = neighbors(-clockwise);
        let right = neighbors(clockwise);
        let solids = |pair: [u16; 2]| pair.into_iter().filter(|s| material::normal_cube(*s)).count();
        let door = |pair: [u16; 2]| pair.into_iter().any(|s| s >> 4 == id);
        let right_hinge = door(left) && !door(right) || solids(right) > solids(left);
        return Ok(Action {
            accepted: true,
            consume: true,
            writes: vec![
                (lower, (id << 4) | ((facing as u16 + 1) & 3)),
                (lower + IVec3::Y, (id << 4) | 8 | u16::from(right_hinge)),
            ],
        });
    }
    if matches!(i.item,356|404){
        let target=if replaceable_for_click(w,i.clicked){i.clicked}else{i.clicked+dir};
        let id=if i.item==356{93}else{149};
        if !writable(w,target) || !can_replace(w,target,id,i.side)
            || collision_shapes::placement_box(w,target,id)?.is_some_and(&blocked) {return Ok(Action::default());}
        let yaw=((f64::from(i.yaw*4./360.)+0.5).floor() as i32+2)&3;
        return Ok(partly(w,target,(id<<4)|yaw as u16,false));
    }
    if i.item == 331 {
        let target = if replaceable_for_click(w, i.clicked) { i.clicked } else { i.clicked + dir };
        let old_id = w.get(target) >> 4;
        if !writable(w,target) || !can_replace(w,target,old_id,i.side)
            || collision_shapes::placement_box(w,target,old_id)?.is_some_and(&blocked)
            || !can_replace(w,target,55,i.side) {
            return Ok(Action::default());
        }
        return Ok(partly(w,target,55<<4,false));
    }
    if matches!(i.item, 44 | 126 | 182) {
        // Variant enum mapping is the same as the slab's ordinary bottom-half
        // onBlockPlaced result with this ItemStack metadata.
        let mut sample = i;
        sample.side = 1;
        sample.hit_y = 0.;
        let variant = outcome_state(w, i.clicked, &sample)? as u16 & 7;
        let adjacent = i.clicked + dir;
        let next = w.get(adjacent);
        let same = |s: u16| s >> 4 == i.item && s & 7 == variant;
        let merge = if same(clicked)
            && (i.side == 1 && clicked & 8 == 0 || i.side == 0 && clicked & 8 != 0)
        {
            Some(i.clicked)
        } else if same(next) {
            Some(adjacent)
        } else {
            None
        };
        if let Some(pos) = merge {
            let bounds = Aabb {
                min: pos.as_dvec3(),
                max: pos.as_dvec3() + DVec3::ONE,
            };
            return Ok(partly(
                w,
                pos,
                ((i.item - 1) << 4) | variant,
                blocked(bounds),
            ));
        }
    }
    if i.item == 78 {
        if !(i.side == 1 && clicked >> 4 == 78 || replaceable_for_click(w, i.clicked)) {
            clicked_pos += dir;
        }
        let old = w.get(clicked_pos);
        if old >> 4 == 78 && old & 7 < 7 {
            let state = old + 1;
            let min = clicked_pos.as_dvec3();
            let bounds = Aabb {
                min,
                max: min + DVec3::new(1., (state & 7) as f64 / 8., 1.),
            };
            if !blocked(bounds) && writable(w, clicked_pos) {
                return Ok(partly(w, clicked_pos, state, false));
            }
        }
    }
    let p = if replaceable_for_click(w, clicked_pos) {
        clicked_pos
    } else {
        clicked_pos + dir
    };
    let shape = collision_shapes::placement_box(w, p, i.item)?;
    if shape.is_some_and(&blocked) || !can_replace(w, p, i.item, i.side) {
        return Ok(Action::default());
    }
    let placed = outcome_state(w, p, &i)?;
    let mut state = (placed & 4095) as u16;
    let mut paired = None;
    if matches!(i.item, 54 | 146) {
        // Chest.onBlockPlacedBy only rotates a paired chest when its new
        // facing is perpendicular to the joined edge. Otherwise the initial
        // onBlockPlaced facing remains until the regular neighbor callback.
        if let Some(d) = HORIZONTAL
            .iter()
            .copied()
            .find(|d| w.get(p + *d) >> 4 == i.item)
        {
            let facing = (state & 7) as usize;
            if SIDES[facing].dot(d) == 0 {
                paired = Some((p + d, state));
            } else {
                state = (state & !7)
                    | match facing {
                        2 => 3,
                        3 => 2,
                        4 => 5,
                        _ => 4,
                    };
            }
        }
    }
    let mut action = partly(w, p, state, false);
    if action.consume {
        if let Some((q, s)) = paired {
            if w.get(q) != s {
                action.writes.push((q, s));
            }
        }
        let extra = (placed >> 12) - 1;
        if extra >= 0 && writable(w, p + IVec3::Y) && w.get(p + IVec3::Y) != extra as u16 {
            action.writes.push((p + IVec3::Y, extra as u16));
        }
    }
    Ok(action)
}
