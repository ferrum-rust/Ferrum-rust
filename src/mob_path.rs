//! 1.8.9 PathFinder and WalkNodeProcessor. The point table uses the original
//! integer key (including its coordinate aliases), and the heap keeps Java's
//! right-child tie behavior.
use crate::vanilla::movement::MovementWorld;
use glam::{DVec3, IVec3};
use std::collections::HashMap;
include!("mob_path_data.rs");
#[path = "mob_navigation.rs"]
pub mod navigation;
#[path = "mob_path_world.rs"]
mod world_view;
#[cfg(test)]
pub use world_view::find_world;
pub(super) use world_view::ChunkWindow;
#[path = "mob_path_table.rs"]
mod point_table;
#[cfg(test)]
#[path = "mob_path_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
pub struct Walker {
    pub position: DVec3,
    pub width: f32,
    pub height: f32,
    pub in_water: bool,
    pub in_lava: bool,
    pub max_fall: i32,
}
#[derive(Clone, Copy)]
pub struct Options {
    pub enter_doors: bool,
    pub break_doors: bool,
    pub avoid_water: bool,
    pub swim: bool,
    pub range: f32,
}
#[derive(Clone, Copy)]
pub enum Target {
    Entity(DVec3),
    Block(IVec3),
}
fn path_hash(p: IVec3) -> i32 {
    (p.y & 255)
        | ((p.x & 32767) << 8)
        | ((p.z & 32767) << 24)
        | if p.x < 0 { i32::MIN } else { 0 }
        | if p.z < 0 { 32768 } else { 0 }
}
fn squared(from: IVec3, to: IVec3) -> f32 {
    let x = to.x.wrapping_sub(from.x) as f32;
    let y = to.y.wrapping_sub(from.y) as f32;
    let z = to.z.wrapping_sub(from.z) as f32;
    x * x + y * y + z * z
}
#[derive(Clone)]
struct Node {
    position: IVec3,
    distance: f32,
    total: f32,
    parent: Option<usize>,
    visited: bool,
    heap: Option<usize>,
}
#[derive(Default)]
struct Open {
    nodes: Vec<Node>,
    by_hash: point_table::PointTable,
    heap: Vec<usize>,
}
impl Open {
    fn point(&mut self, p: IVec3) -> usize {
        *self.by_hash.entry(path_hash(p)).or_insert_with(|| {
            let index = self.nodes.len();
            self.nodes.push(Node {
                position: p,
                distance: 0.,
                total: 0.,
                parent: None,
                visited: false,
                heap: None,
            });
            index
        })
    }
    fn assign(&mut self, slot: usize, node: usize) {
        self.heap[slot] = node;
        self.nodes[node].heap = Some(slot);
    }
    fn up(&mut self, mut slot: usize, node: usize) {
        while slot > 0 {
            let parent = (slot - 1) / 2;
            let other = self.heap[parent];
            if self.nodes[node].total >= self.nodes[other].total {
                break;
            }
            self.assign(slot, other);
            slot = parent;
        }
        self.assign(slot, node);
    }
    fn down(&mut self, mut slot: usize, node: usize) {
        loop {
            let left = 1 + slot * 2;
            if left >= self.heap.len() {
                break;
            }
            let right = left + 1;
            let child = if right >= self.heap.len()
                || self.nodes[self.heap[left]].total < self.nodes[self.heap[right]].total
            {
                left
            } else {
                right
            };
            let other = self.heap[child];
            if self.nodes[other].total >= self.nodes[node].total {
                break;
            }
            self.assign(slot, other);
            slot = child;
        }
        self.assign(slot, node);
    }
    fn priority(&mut self, node: usize, total: f32) {
        let previous = self.nodes[node].total;
        self.nodes[node].total = total;
        if let Some(slot) = self.nodes[node].heap {
            if total < previous {
                self.up(slot, node)
            } else {
                self.down(slot, node)
            }
        } else {
            let slot = self.heap.len();
            self.heap.push(node);
            self.up(slot, node);
        }
    }
    fn pop(&mut self) -> Option<usize> {
        let root = *self.heap.first()?;
        let last = self.heap.pop().unwrap();
        if !self.heap.is_empty() {
            self.down(0, last);
        }
        self.nodes[root].heap = None;
        Some(root)
    }
    fn path(&self, mut end: usize) -> Vec<IVec3> {
        let mut route = vec![self.nodes[end].position];
        while let Some(previous) = self.nodes[end].parent {
            end = previous;
            route.push(self.nodes[end].position);
        }
        route.reverse();
        route
    }
}
fn wood_door(state: u16) -> bool {
    matches!(state >> 4, 64 | 193..=197)
}
fn rail(state: u16) -> bool {
    matches!(state >> 4, 27 | 28 | 66 | 157)
}
fn passable(world: &impl MovementWorld, p: IVec3, state: u16) -> bool {
    if matches!(state >> 4, 64 | 71 | 193..=197) {
        let bottom = if state & 8 != 0 {
            world.get(p - IVec3::Y)
        } else {
            state
        };
        return bottom & 4 != 0;
    }
    let raw = usize::from(state);
    PASSABLE
        .get(raw / 64)
        .is_some_and(|bits| bits & (1u64 << (raw % 64)) != 0)
}
struct Processor<'a, W, const CACHED: bool> {
    world: &'a W,
    walker: Walker,
    options: Options,
    size: IVec3,
    on_rail: bool,
    // A search sees one immutable world and one set of movement options. Use
    // exact coordinates here: PathPoint's Java hash deliberately aliases some
    // positions, but those positions do not have the same block clearance.
    offsets: HashMap<IVec3, i32>,
}
impl<W: MovementWorld, const CACHED: bool> Processor<'_, W, CACHED> {
    fn offset(&mut self, p: IVec3) -> i32 {
        if CACHED {
            if let Some(&kind) = self.offsets.get(&p) {
                return kind;
            }
        }
        let kind = self.classify(p);
        if CACHED {
            self.offsets.insert(p, kind);
        }
        kind
    }
    fn classify(&self, p: IVec3) -> i32 {
        let mut special = false;
        for x in p.x..p.x + self.size.x {
            for y in p.y..p.y + self.size.y {
                for z in p.z..p.z + self.size.z {
                    let at = IVec3::new(x, y, z);
                    let state = self.world.get(at);
                    let id = state >> 4;
                    if id == 0 {
                        continue;
                    }
                    match id {
                        96 | 167 => special = true,
                        8 | 9 if self.options.avoid_water => return -1,
                        8 | 9 => special = true,
                        _ if !self.options.enter_doors && wood_door(state) => return 0,
                        _ => {}
                    }
                    if rail(state) {
                        if self.on_rail {
                            continue;
                        }
                        return -3;
                    }
                    if passable(self.world, at, state)
                        || self.options.break_doors && wood_door(state)
                    {
                        continue;
                    }
                    match id {
                        85 | 107 | 113 | 139 | 183..=192 => return -3,
                        96 | 167 => return -4,
                        10 | 11 if self.walker.in_lava => continue,
                        10 | 11 => return -2,
                        _ => return 0,
                    }
                }
            }
        }
        if special {
            2
        } else {
            1
        }
    }
    fn safe(&mut self, open: &mut Open, mut p: IVec3, step: i32) -> Option<usize> {
        let kind = self.offset(p);
        if kind == 2 {
            return Some(open.point(p));
        }
        if kind != 1 {
            if step == 0 || matches!(kind, -3 | -4) || self.offset(p + IVec3::Y * step) != 1 {
                return None;
            }
            p.y += step;
        }
        let mut node = open.point(p);
        let mut distance = 0;
        let mut below = 0;
        while p.y > 0 {
            below = self.offset(p - IVec3::Y);
            if self.options.avoid_water && below == -1 {
                return None;
            }
            if below != 1 {
                break;
            }
            if distance >= self.walker.max_fall {
                return None;
            }
            distance += 1;
            p.y -= 1;
            if p.y <= 0 {
                return None;
            }
            node = open.point(p);
        }
        (below != -2).then_some(node)
    }
}
#[cfg(test)]
pub fn find(
    world: &impl MovementWorld,
    walker: Walker,
    options: Options,
    target: Target,
) -> Option<Vec<IVec3>> {
    find_inner::<true>(world, walker, options, target)
}
fn find_inner<const CACHED: bool>(
    world: &impl MovementWorld,
    walker: Walker,
    mut options: Options,
    target: Target,
) -> Option<Vec<IVec3>> {
    let half = f64::from(walker.width * 0.5);
    let feet = walker.position;
    let mut y;
    if options.swim && walker.in_water {
        y = feet.y as i32;
        let mut p = feet.floor().as_ivec3();
        p.y = y;
        while matches!(world.get(p) >> 4, 8 | 9) {
            y += 1;
            p.y = y;
        }
        options.avoid_water = false;
    } else {
        y = (feet.y + 0.5).floor() as i32;
    }
    let start = IVec3::new(
        (feet.x - half).floor() as i32,
        y,
        (feet.z - half).floor() as i32,
    );
    let target = match target {
        Target::Entity(pos) => pos,
        Target::Block(pos) => (pos.as_vec3() + glam::Vec3::splat(0.5)).as_dvec3(),
    };
    let goal = (target - DVec3::new(half, 0., half)).floor().as_ivec3();
    let width = (walker.width + 1.).floor() as i32;
    let at = walker.position.floor().as_ivec3();
    let mut processor = Processor::<_, CACHED> {
        world,
        walker,
        options,
        size: IVec3::new(width, (walker.height + 1.).floor() as i32, width),
        on_rail: rail(world.get(at)) || rail(world.get(at - IVec3::Y)),
        offsets: HashMap::new(),
    };
    let mut open = Open::default();
    let start = open.point(start);
    let goal = open.point(goal);
    let goal_pos = open.nodes[goal].position;
    open.priority(start, squared(open.nodes[start].position, goal_pos));
    let mut closest = start;
    while let Some(current) = open.pop() {
        if current == goal {
            return Some(open.path(goal));
        }
        let p = open.nodes[current].position;
        if squared(p, goal_pos) < squared(open.nodes[closest].position, goal_pos) {
            closest = current;
        }
        open.nodes[current].visited = true;
        let step = i32::from(processor.offset(p + IVec3::Y) == 1);
        let neighbors = [IVec3::Z, IVec3::NEG_X, IVec3::X, IVec3::NEG_Z]
            .map(|d| processor.safe(&mut open, p + d, step));
        for next in neighbors.into_iter().flatten() {
            let n = &open.nodes[next];
            let heuristic = squared(n.position, goal_pos);
            if n.visited || heuristic.sqrt() >= options.range {
                continue;
            }
            let distance = open.nodes[current].distance + squared(p, n.position);
            if distance >= options.range * 2. || n.heap.is_some() && distance >= n.distance {
                continue;
            }
            let n = &mut open.nodes[next];
            n.parent = Some(current);
            n.distance = distance;
            open.priority(next, distance + heuristic);
        }
    }
    (closest != start).then(|| open.path(closest))
}
