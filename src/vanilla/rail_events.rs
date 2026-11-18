//! Original rail graph and ordinary/powered/activator block callbacks.
//! World owns recursive flags=3 notifications, support, redstone and drops.
use glam::IVec3;
#[path = "rail_power.rs"]
mod power;
pub trait RailWorld {
    fn get(&self, p: IVec3) -> u16;
    fn set(&mut self, p: IVec3, state: u16, flags: u8);
    fn is_powered(&self, p: IVec3) -> bool;
    fn solid_top(&self, p: IVec3) -> bool;
    fn drop_block(&mut self, p: IVec3, state: u16);
    fn notify_neighbors(&mut self, p: IVec3, source: u16);
}
pub fn is_rail(state: u16) -> bool {
    matches!(state >> 4, 27 | 28 | 66 | 157)
}
fn shape(state: u16) -> u16 {
    if state >> 4 == 66 {
        state & 15
    } else {
        state & 7
    }
}
fn shaped(state: u16, shape: u16) -> u16 {
    if state >> 4 == 66 {
        state & !15 | shape
    } else {
        state & !7 | shape
    }
}
struct Rail {
    p: IVec3,
    state: u16,
    straight: bool,
    connections: Vec<IVec3>,
}
impl Rail {
    fn new(p: IVec3, state: u16) -> Self {
        let mut r = Self {
            p,
            state,
            straight: state >> 4 != 66,
            connections: Vec::with_capacity(3),
        };
        r.set_connections(shape(state));
        r
    }
    fn set_connections(&mut self, shape: u16) {
        let (n, s, w, e, u) = (-IVec3::Z, IVec3::Z, -IVec3::X, IVec3::X, IVec3::Y);
        let offsets = match shape {
            0 => [n, s],
            1 => [w, e],
            2 => [w, e + u],
            3 => [w + u, e],
            4 => [n + u, s],
            5 => [n, s + u],
            6 => [e, s],
            7 => [w, s],
            8 => [w, n],
            9 => [e, n],
            _ => [n, s],
        };
        self.connections.clear();
        self.connections.extend(offsets.map(|d| self.p + d));
    }
    fn connects(&self, p: IVec3) -> bool {
        self.connections.iter().any(|q| p.x == q.x && p.z == q.z)
    }
    fn find(world: &impl RailWorld, p: IVec3) -> Option<Self> {
        for q in [p, p + IVec3::Y, p - IVec3::Y] {
            let s = world.get(q);
            if is_rail(s) {
                return Some(Self::new(q, s));
            }
        }
        None
    }
    fn trim(&mut self, world: &impl RailWorld) {
        let mut i = 0;
        while i < self.connections.len() {
            if let Some(r) = Self::find(world, self.connections[i]).filter(|r| r.connects(self.p)) {
                self.connections[i] = r.p;
                i += 1;
            } else {
                self.connections.remove(i);
            }
        }
    }
    fn accepts(&self, other: &Self) -> bool {
        self.connects(other.p) || self.connections.len() != 2
    }
    fn neighbor_accepts(&self, world: &impl RailWorld, p: IVec3) -> bool {
        if let Some(mut r) = Self::find(world, p) {
            r.trim(world);
            r.accepts(self)
        } else {
            false
        }
    }
    fn ascending(&self, world: &impl RailWorld, mut s: Option<u16>) -> u16 {
        if s == Some(0) {
            if is_rail(world.get(self.p - IVec3::Z + IVec3::Y)) {
                s = Some(4);
            }
            if is_rail(world.get(self.p + IVec3::Z + IVec3::Y)) {
                s = Some(5);
            }
        }
        if s == Some(1) {
            if is_rail(world.get(self.p + IVec3::X + IVec3::Y)) {
                s = Some(2);
            }
            if is_rail(world.get(self.p - IVec3::X + IVec3::Y)) {
                s = Some(3);
            }
        }
        s.unwrap_or(0)
    }
    fn connect(&mut self, world: &mut impl RailWorld, other: &Self) {
        self.connections.push(other.p);
        let [n, s, w, e] =
            [-IVec3::Z, IVec3::Z, -IVec3::X, IVec3::X].map(|d| self.connects(self.p + d));
        let mut result = None;
        if n || s {
            result = Some(0);
        }
        if w || e {
            result = Some(1);
        }
        if !self.straight {
            if s && e && !n && !w {
                result = Some(6);
            }
            if s && w && !n && !e {
                result = Some(7);
            }
            if n && w && !s && !e {
                result = Some(8);
            }
            if n && e && !s && !w {
                result = Some(9);
            }
        }
        self.state = shaped(self.state, self.ascending(world, result));
        world.set(self.p, self.state, 3);
    }
    fn update(&mut self, world: &mut impl RailWorld, powered: bool, force: bool) {
        let [n, s, w, e] = [-IVec3::Z, IVec3::Z, -IVec3::X, IVec3::X]
            .map(|d| self.neighbor_accepts(world, self.p + d));
        let mut result = None;
        if (n || s) && !w && !e {
            result = Some(0);
        }
        if (w || e) && !n && !s {
            result = Some(1);
        }
        if !self.straight {
            if s && e && !n && !w {
                result = Some(6);
            }
            if s && w && !n && !e {
                result = Some(7);
            }
            if n && w && !s && !e {
                result = Some(8);
            }
            if n && e && !s && !w {
                result = Some(9);
            }
        }
        if result.is_none() {
            if n || s {
                result = Some(0);
            }
            if w || e {
                result = Some(1);
            }
            if !self.straight {
                if powered {
                    if s && e {
                        result = Some(6);
                    }
                    if w && s {
                        result = Some(7);
                    }
                    if e && n {
                        result = Some(9);
                    }
                    if n && w {
                        result = Some(8);
                    }
                } else {
                    if n && w {
                        result = Some(8);
                    }
                    if e && n {
                        result = Some(9);
                    }
                    if w && s {
                        result = Some(7);
                    }
                    if s && e {
                        result = Some(6);
                    }
                }
            }
        }
        let result = self.ascending(world, result);
        self.set_connections(result);
        self.state = shaped(self.state, result);
        if force || world.get(self.p) != self.state {
            world.set(self.p, self.state, 3);
            for i in 0..self.connections.len() {
                if let Some(mut r) = Self::find(world, self.connections[i]) {
                    r.trim(world);
                    if r.accepts(self) {
                        r.connect(world, self);
                    }
                }
            }
        }
    }
    fn adjacent(&self, world: &impl RailWorld) -> usize {
        [-IVec3::Z, IVec3::X, IVec3::Z, -IVec3::X]
            .into_iter()
            .filter(|d| {
                let p = self.p + *d;
                is_rail(world.get(p))
                    || is_rail(world.get(p + IVec3::Y))
                    || is_rail(world.get(p - IVec3::Y))
            })
            .count()
    }
}
/// Graph update shared by all rail classes. Powered/detector callbacks are
/// deliberately separate; invoking this alone does not implement redstone.
pub fn update_shape(world: &mut impl RailWorld, p: IVec3, state: u16, force: bool) -> u16 {
    let mut rail = Rail::new(p, state);
    let powered = world.is_powered(p);
    rail.update(world, powered, force);
    rail.state
}
pub fn on_added(world: &mut impl RailWorld, p: IVec3, state: u16) -> bool {
    if !matches!(state >> 4, 27 | 28 | 66 | 157) {
        return false;
    }
    let state = update_shape(world, p, state, true);
    if state >> 4 != 66 {
        neighbor_changed(world, p, state, false);
    }
    true
}
pub fn neighbor_changed(
    world: &mut impl RailWorld,
    p: IVec3,
    state: u16,
    source_can_provide_power: bool,
) -> bool {
    if !is_rail(state) {
        return false;
    }
    let s = shape(state);
    let mut invalid = !world.solid_top(p - IVec3::Y);
    let side = match s {
        2 => Some(IVec3::X),
        3 => Some(-IVec3::X),
        4 => Some(-IVec3::Z),
        5 => Some(IVec3::Z),
        _ => None,
    };
    if let Some(d) = side {
        if !world.solid_top(p + d) {
            invalid = true;
        }
    }
    if invalid {
        world.drop_block(p, state);
        world.set(p, 0, 3);
    } else if state >> 4 == 66
        && source_can_provide_power
        && Rail::new(p, state).adjacent(world) == 3
    {
        update_shape(world, p, state, false);
    } else if matches!(state >> 4, 27 | 157) {
        power::neighbor(world, p, state);
    }
    true
}
pub fn on_removed(world: &mut impl RailWorld, p: IVec3, state: u16) -> bool {
    if !is_rail(state) {
        return false;
    }
    if (2..=5).contains(&shape(state)) {
        world.notify_neighbors(p + IVec3::Y, state >> 4);
    }
    if state >> 4 != 66 {
        world.notify_neighbors(p, state >> 4);
        world.notify_neighbors(p - IVec3::Y, state >> 4);
    }
    true
}

#[cfg(test)]
#[path = "rail_events_tests.rs"]
mod tests;
