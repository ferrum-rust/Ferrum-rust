//! Original 1.8.9 ocean-monument room graph and generation state.
use super::{biome::BiomeGenerator, random::JavaRandom, structure_common::BoundingBox};
use serde::{Deserialize, Serialize};
#[path = "monument_population.rs"]
mod population;
#[path = "monument_templates.rs"]
mod templates;
pub use population::{MonumentWorld, Population, WorldAdapter};
#[cfg(test)]
#[path = "monument_tests.rs"]
mod tests;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Building,
    Core,
    DoubleX,
    DoubleXY,
    DoubleY,
    DoubleYZ,
    DoubleZ,
    Entry,
    Penthouse,
    Simple,
    SimpleTop,
    Wing,
}
impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Building => "MonumentBuilding",
            Self::Core => "MonumentCoreRoom",
            Self::DoubleX => "DoubleXRoom",
            Self::DoubleXY => "DoubleXYRoom",
            Self::DoubleY => "DoubleYRoom",
            Self::DoubleYZ => "DoubleYZRoom",
            Self::DoubleZ => "DoubleZRoom",
            Self::Entry => "EntryRoom",
            Self::Penthouse => "Penthouse",
            Self::Simple => "SimpleRoom",
            Self::SimpleTop => "SimpleTopRoom",
            Self::Wing => "WingRoom",
        }
    }
    fn size(self) -> [i32; 3] {
        match self {
            Self::Core => [2, 2, 2],
            Self::DoubleX => [2, 1, 1],
            Self::DoubleXY => [2, 2, 1],
            Self::DoubleY => [1, 2, 1],
            Self::DoubleYZ => [1, 2, 2],
            Self::DoubleZ => [1, 1, 2],
            _ => [1, 1, 1],
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Room {
    pub id: i32,
    pub neighbors: [Option<usize>; 6],
    pub open: [bool; 6],
    pub claimed: bool,
    pub source: bool,
    pub scan: i32,
}
impl Room {
    fn new(id: i32) -> Self {
        Self {
            id,
            neighbors: [None; 6],
            open: [false; 6],
            claimed: false,
            source: false,
            scan: 0,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Piece {
    pub kind: Kind,
    pub bounds: BoundingBox,
    pub orientation: u8,
    pub room: Option<usize>,
    pub variant: i32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Monument {
    pub seed: i64,
    pub chunk: [i32; 2],
    pub bounds: BoundingBox,
    pub orientation: u8,
    pub rooms: Vec<Option<Room>>,
    pub pieces: Vec<Piece>,
    pub core: usize,
    pub processed: Vec<[i32; 2]>,
    pub initialized: bool,
}
/// Sets and advances World.rand exactly as World.setRandomSeed plus the four
/// lattice offset draws. The final coordinate comparison must not short-circuit.
pub fn lattice(seed: i64, cx: i32, cz: i32, world_random: &mut JavaRandom) -> bool {
    let rx = if cx < 0 { cx.wrapping_sub(31) } else { cx } / 32;
    let rz = if cz < 0 { cz.wrapping_sub(31) } else { cz } / 32;
    world_random.set_seed(
        (rx as i64)
            .wrapping_mul(341873128712)
            .wrapping_add((rz as i64).wrapping_mul(132897987541))
            .wrapping_add(seed)
            .wrapping_add(10387313),
    );
    let x = rx
        .wrapping_mul(32)
        .wrapping_add((world_random.next_int(27) + world_random.next_int(27)) / 2);
    let z = rz
        .wrapping_mul(32)
        .wrapping_add((world_random.next_int(27) + world_random.next_int(27)) / 2);
    cx == x && cz == z
}
pub fn can_spawn(
    seed: i64,
    cx: i32,
    cz: i32,
    biomes: &BiomeGenerator,
    world_random: &mut JavaRandom,
) -> bool {
    lattice(seed, cx, cz, world_random)
        && biomes.get_biomes(cx * 16 + 8, cz * 16 + 8, 1, 1)[0] == 24
        && biomes.are_biomes_viable(cx * 16 + 8, cz * 16 + 8, 29, &[0, 24, 7, 10, 11])
}
fn link(rooms: &mut [Option<Room>], a: usize, dir: usize, b: usize) {
    rooms[a].as_mut().unwrap().neighbors[dir] = Some(b);
    rooms[b].as_mut().unwrap().neighbors[dir ^ 1] = Some(a);
}
fn reachable(rooms: &mut [Option<Room>], i: usize, scan: i32) -> bool {
    let room = rooms[i].as_mut().unwrap();
    if room.source {
        return true;
    }
    room.scan = scan;
    for dir in 0..6 {
        let r = rooms[i].as_ref().unwrap();
        if r.open[dir] {
            if let Some(j) = r.neighbors[dir] {
                if rooms[j].as_ref().unwrap().scan != scan && reachable(rooms, j, scan) {
                    return true;
                }
            }
        }
    }
    false
}
impl Piece {
    fn room(kind: Kind, orientation: u8, index: usize, random: &mut JavaRandom) -> Self {
        let [sx, sy, sz] = kind.size();
        let x = index as i32 % 5;
        let z = index as i32 / 5 % 5;
        let y = index as i32 / 25;
        let mut bounds = if orientation & 1 == 0 {
            BoundingBox::new([0, 0, 0], [sx * 8 - 1, sy * 4 - 1, sz * 8 - 1])
        } else {
            BoundingBox::new([0, 0, 0], [sz * 8 - 1, sy * 4 - 1, sx * 8 - 1])
        };
        match orientation {
            2 => bounds.offset(x * 8, y * 4, -(z + sz) * 8 + 1),
            0 => bounds.offset(x * 8, y * 4, z * 8),
            1 => bounds.offset(-(z + sz) * 8 + 1, y * 4, x * 8),
            _ => bounds.offset(z * 8, y * 4, x * 8),
        }
        Self {
            kind,
            bounds,
            orientation,
            room: Some(index),
            variant: if kind == Kind::Simple {
                random.next_int(3)
            } else {
                0
            },
        }
    }
}
impl Monument {
    pub fn new(seed: i64, cx: i32, cz: i32) -> Self {
        let mut random = JavaRandom::new(0);
        Self::with_random(seed, cx, cz, &mut random)
    }
    fn with_random(seed: i64, cx: i32, cz: i32, random: &mut JavaRandom) -> Self {
        random.set_seed(seed);
        let a = random.next_long();
        let b = random.next_long();
        random.set_seed((cx as i64).wrapping_mul(a) ^ (cz as i64).wrapping_mul(b) ^ seed);
        let orientation = [2, 3, 0, 1][random.next_int(4) as usize];
        Self::building(seed, cx, cz, orientation, random)
    }
    fn building(seed: i64, cx: i32, cz: i32, orientation: u8, random: &mut JavaRandom) -> Self {
        let x = cx * 16 - 21;
        let z = cz * 16 - 21;
        let bounds = BoundingBox::new([x, 39, z], [x + 57, 61, z + 57]);
        let mut rooms = vec![None; 78];
        for y in 0..2 {
            for x in 0..5 {
                for z in 0..4 {
                    let i = y * 25 + z * 5 + x;
                    rooms[i] = Some(Room::new(i as i32));
                }
            }
        }
        for x in 1..4 {
            for z in 0..2 {
                let i = 50 + z * 5 + x;
                rooms[i] = Some(Room::new(i as i32));
            }
        }
        for x in 0..5 {
            for z in 0..5 {
                for y in 0..3 {
                    let i = y * 25 + z * 5 + x;
                    if rooms[i].is_none() {
                        continue;
                    }
                    for (dir, [dx, dy, dz]) in [
                        [0, -1, 0],
                        [0, 1, 0],
                        [0, 0, -1],
                        [0, 0, 1],
                        [-1, 0, 0],
                        [1, 0, 0],
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let nx = x as i32 + dx;
                        let ny = y as i32 + dy;
                        let nz = z as i32 + dz;
                        if !(0..5).contains(&nx) || !(0..5).contains(&nz) || !(0..3).contains(&ny) {
                            continue;
                        }
                        let j = (ny * 25 + nz * 5 + nx) as usize;
                        if rooms[j].is_some() {
                            link(&mut rooms, i, if dz != 0 { dir ^ 1 } else { dir }, j);
                        }
                    }
                }
            }
        }
        for (index, id) in [(75, 1003), (76, 1001), (77, 1002)] {
            let mut r = Room::new(id);
            r.claimed = true;
            rooms[index] = Some(r);
        }
        link(&mut rooms, 52, 1, 75);
        link(&mut rooms, 25, 3, 76);
        link(&mut rooms, 29, 3, 77);
        rooms[2].as_mut().unwrap().source = true;
        let core = 10 + random.next_int(4) as usize;
        for dz in 0..2 {
            for dy in 0..2 {
                for dx in 0..2 {
                    rooms[core + dz * 5 + dy * 25 + dx]
                        .as_mut()
                        .unwrap()
                        .claimed = true;
                }
            }
        }
        let mut order = Vec::new();
        for (i, room) in rooms.iter_mut().enumerate().take(75) {
            if let Some(r) = room {
                r.open = r.neighbors.map(|n| n.is_some());
                order.push(i);
            }
        }
        rooms[75].as_mut().unwrap().open =
            rooms[75].as_ref().unwrap().neighbors.map(|n| n.is_some());
        for n in (2..=order.len()).rev() {
            let j = random.next_int(n as i32) as usize;
            order.swap(n - 1, j);
        }
        let mut scan = 1;
        for &i in &order {
            let mut closed = 0;
            for _ in 0..5 {
                if closed >= 2 {
                    break;
                }
                let dir = random.next_int(6) as usize;
                if !rooms[i].as_ref().unwrap().open[dir] {
                    continue;
                }
                let j = rooms[i].as_ref().unwrap().neighbors[dir].unwrap();
                rooms[i].as_mut().unwrap().open[dir] = false;
                rooms[j].as_mut().unwrap().open[dir ^ 1] = false;
                let first = reachable(&mut rooms, i, scan);
                scan += 1;
                let second = if first {
                    let result = reachable(&mut rooms, j, scan);
                    scan += 1;
                    result
                } else {
                    false
                };
                if first && second {
                    closed += 1;
                } else {
                    rooms[i].as_mut().unwrap().open[dir] = true;
                    rooms[j].as_mut().unwrap().open[dir ^ 1] = true;
                }
            }
        }
        order.extend([75, 76, 77]);
        rooms[2].as_mut().unwrap().claimed = true;
        let mut pieces = vec![
            Piece::room(Kind::Entry, orientation, 2, random),
            Piece::room(Kind::Core, orientation, core, random),
        ];
        for &i in &order {
            let r = rooms[i].as_ref().unwrap();
            if r.claimed || r.id >= 75 {
                continue;
            }
            let free = |j: usize, dir: usize| {
                let r = rooms[j].as_ref().unwrap();
                r.open[dir] && !rooms[r.neighbors[dir].unwrap()].as_ref().unwrap().claimed
            };
            let neighbor =
                |j: usize, dir: usize| rooms[j].as_ref().unwrap().neighbors[dir].unwrap();
            let (kind, indices) = if free(i, 5) && free(i, 1) && free(neighbor(i, 5), 1) {
                (
                    Kind::DoubleXY,
                    vec![
                        i,
                        neighbor(i, 5),
                        neighbor(i, 1),
                        neighbor(neighbor(i, 5), 1),
                    ],
                )
            } else if free(i, 2) && free(i, 1) && free(neighbor(i, 2), 1) {
                (
                    Kind::DoubleYZ,
                    vec![
                        i,
                        neighbor(i, 2),
                        neighbor(i, 1),
                        neighbor(neighbor(i, 2), 1),
                    ],
                )
            } else if free(i, 2) {
                (Kind::DoubleZ, vec![i, neighbor(i, 2)])
            } else if free(i, 5) {
                (Kind::DoubleX, vec![i, neighbor(i, 5)])
            } else if free(i, 1) {
                (Kind::DoubleY, vec![i, neighbor(i, 1)])
            } else if [4, 5, 2, 3, 1].into_iter().all(|d| !r.open[d]) {
                (Kind::SimpleTop, vec![i])
            } else {
                (Kind::Simple, vec![i])
            };
            for j in indices {
                rooms[j].as_mut().unwrap().claimed = true;
            }
            pieces.push(Piece::room(kind, orientation, i, random));
        }
        let origin = bounds.local_to_world(9, 0, 22, orientation);
        for piece in &mut pieces {
            piece.bounds.offset(origin[0], origin[1], origin[2]);
        }
        let wing_variant = random.next_i32();
        for (kind, a, b, variant) in [
            (Kind::Wing, [1, 1, 1], [23, 8, 21], wing_variant & 1),
            (
                Kind::Wing,
                [34, 1, 1],
                [56, 8, 21],
                wing_variant.wrapping_add(1) & 1,
            ),
            (Kind::Penthouse, [22, 13, 22], [35, 17, 35], 0),
        ] {
            let a = bounds.local_to_world(a[0], a[1], a[2], orientation);
            let b = bounds.local_to_world(b[0], b[1], b[2], orientation);
            let child_bounds = BoundingBox::new(
                std::array::from_fn(|i| a[i].min(b[i])),
                std::array::from_fn(|i| a[i].max(b[i])),
            );
            pieces.push(Piece {
                kind,
                bounds: child_bounds,
                orientation,
                room: None,
                variant,
            });
        }
        Self {
            seed,
            chunk: [cx, cz],
            bounds,
            orientation,
            rooms,
            pieces,
            core,
            processed: Vec::new(),
            initialized: true,
        }
    }
    /// Vanilla NBT stores only the outer component, then lazily reconstructs
    /// rooms at the next population call using that call's Random instance.
    pub fn reload_as_original(&mut self) {
        self.initialized = false;
    }
}
