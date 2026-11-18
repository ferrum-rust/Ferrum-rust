//! Native 1.8.9 abandoned mineshaft layout and population.
use super::{
    loot::{fill_loot, LootEntry, LootStack},
    random::JavaRandom,
    structure_common::{full_block, rotate_metadata, BoundingBox},
};
use crate::world::World;
use glam::IVec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum PieceKind {
    Room {
        entrances: Vec<BoundingBox>,
    },
    Corridor {
        rails: bool,
        spiders: bool,
        spawner_placed: bool,
        sections: i32,
    },
    Cross {
        direction: u8,
        multiple_floors: bool,
    },
    Stairs,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Piece {
    pub bounds: BoundingBox,
    pub direction: Option<u8>,
    pub depth: i32,
    pub kind: PieceKind,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mineshaft {
    pub chunk: [i32; 2],
    pub bounds: BoundingBox,
    pub pieces: Vec<Piece>,
}

/// Generated entity/tile data stays explicit; returning this does not simulate
/// chest minecarts or spawners. Callers must persist it alongside the blocks.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Population {
    pub changed: Vec<[i32; 3]>,
    pub carts: Vec<ChestMinecart>,
    pub spawners: Vec<[i32; 3]>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ChestMinecart {
    pub position: [f64; 3],
    pub slots: Vec<Option<LootStack>>,
}
pub trait MineshaftWorld {
    fn get(&self, p: [i32; 3]) -> u16;
    fn set(&mut self, p: [i32; 3], state: u16);
    fn cart(&mut self, cart: ChestMinecart);
    /// The original tile entity is configured as CaveSpider.
    fn spawner(&mut self, p: [i32; 3]);
}
pub struct WorldAdapter<'a> {
    pub world: &'a mut World,
    pub output: &'a mut Population,
}
impl MineshaftWorld for WorldAdapter<'_> {
    fn get(&self, p: [i32; 3]) -> u16 {
        self.world.get(IVec3::from_array(p))
    }
    fn set(&mut self, p: [i32; 3], state: u16) {
        let pos = IVec3::from_array(p);
        if self.world.get(pos) != state && self.world.set(pos, state) {
            self.output.changed.push(p);
        }
    }
    fn cart(&mut self, cart: ChestMinecart) {
        self.output.carts.push(cart);
    }
    fn spawner(&mut self, p: [i32; 3]) {
        self.output.spawners.push(p);
    }
}

impl Mineshaft {
    /// StructureStart.generateStructure order, including removal of components
    /// intersecting liquid and permanent spider-spawner placement flags.
    pub fn populate(
        &mut self,
        world: &mut World,
        random: &mut JavaRandom,
        clip: BoundingBox,
    ) -> Population {
        let mut output = Population::default();
        self.populate_with(
            &mut WorldAdapter {
                world,
                output: &mut output,
            },
            random,
            clip,
        );
        output
    }
    /// Eventful runtime adapter can lazily supply missing chunks and run the
    /// original block callbacks. Legacy previews keep the `populate` wrapper.
    pub fn populate_with(
        &mut self,
        world: &mut impl MineshaftWorld,
        random: &mut JavaRandom,
        clip: BoundingBox,
    ) {
        self.pieces.retain_mut(|piece| {
            if !piece.bounds.intersects(&clip) {
                return true;
            }
            let mut builder = Builder {
                world,
                clip,
                bounds: piece.bounds,
                direction: piece.direction,
            };
            if builder.has_liquid() {
                return false;
            }
            builder.populate(&mut piece.kind, random);
            true
        });
    }
}

struct Builder<'a, W: MineshaftWorld> {
    world: &'a mut W,
    clip: BoundingBox,
    bounds: BoundingBox,
    direction: Option<u8>,
}
impl<W: MineshaftWorld> Builder<'_, W> {
    fn point(&self, p: [i32; 3]) -> [i32; 3] {
        self.direction
            .map(|d| self.bounds.local_to_world(p[0], p[1], p[2], d))
            .unwrap_or(p)
    }
    fn get(&self, p: [i32; 3]) -> u16 {
        let p = self.point(p);
        if self.clip.contains(p) {
            self.world.get(p)
        } else {
            0
        }
    }
    fn set(&mut self, p: [i32; 3], state: u16) {
        let p = self.point(p);
        if self.clip.contains(p) {
            self.world.set(p, state);
        }
    }
    fn fill(&mut self, min: [i32; 3], max: [i32; 3], state: u16, ignore_air: bool) {
        for y in min[1]..=max[1] {
            for x in min[0]..=max[0] {
                for z in min[2]..=max[2] {
                    if !ignore_air || self.get([x, y, z]) >> 4 != 0 {
                        self.set([x, y, z], state);
                    }
                }
            }
        }
    }
    fn random_fill(
        &mut self,
        random: &mut JavaRandom,
        chance: f32,
        min: [i32; 3],
        max: [i32; 3],
        state: u16,
    ) {
        for y in min[1]..=max[1] {
            for x in min[0]..=max[0] {
                for z in min[2]..=max[2] {
                    if random.next_float() <= chance {
                        self.set([x, y, z], state);
                    }
                }
            }
        }
    }
    fn random_place(&mut self, random: &mut JavaRandom, chance: f32, p: [i32; 3], state: u16) {
        if random.next_float() < chance {
            self.set(p, state);
        }
    }
    fn has_liquid(&self) -> bool {
        let min: [i32; 3] = std::array::from_fn(|a| (self.bounds.min[a] - 1).max(self.clip.min[a]));
        let max: [i32; 3] = std::array::from_fn(|a| (self.bounds.max[a] + 1).min(self.clip.max[a]));
        let liquid = |x, y, z| matches!(self.world.get([x, y, z]) >> 4, 8..=11);
        for x in min[0]..=max[0] {
            for z in min[2]..=max[2] {
                if liquid(x, min[1], z) || liquid(x, max[1], z) {
                    return true;
                }
            }
        }
        for x in min[0]..=max[0] {
            for y in min[1]..=max[1] {
                if liquid(x, y, min[2]) || liquid(x, y, max[2]) {
                    return true;
                }
            }
        }
        for z in min[2]..=max[2] {
            for y in min[1]..=max[1] {
                if liquid(min[0], y, z) || liquid(max[0], y, z) {
                    return true;
                }
            }
        }
        false
    }
    fn chest(&mut self, random: &mut JavaRandom, p: [i32; 3]) {
        let mut entries: Vec<_> = [
            (265, 0, 1, 5, 10),
            (266, 0, 1, 3, 5),
            (331, 0, 4, 9, 5),
            (351, 4, 4, 9, 5),
            (264, 0, 1, 2, 3),
            (263, 0, 3, 8, 10),
            (297, 0, 1, 3, 15),
            (257, 0, 1, 1, 1),
            (66, 0, 4, 8, 1),
            (362, 0, 2, 4, 10),
            (361, 0, 2, 4, 10),
            (329, 0, 1, 1, 3),
            (417, 0, 1, 1, 1),
        ]
        .into_iter()
        .map(|(id, meta, min, max, weight)| LootEntry::new(id, meta, min, max, weight))
        .collect();
        // Java evaluates both arguments even if placement later fails its clip.
        entries.push(LootEntry::book(random));
        let rolls = 3 + random.next_int(4);
        let world_p = self.point(p);
        if self.clip.contains(world_p) && self.get(p) >> 4 == 0 {
            let rail = u8::from(random.next_boolean());
            self.set(
                p,
                66 << 4 | rotate_metadata(66, rail, self.direction.unwrap()) as u16,
            );
            let slots = fill_loot(random, &entries, rolls, 27);
            self.world.cart(ChestMinecart {
                position: world_p.map(|v| (v as f32 + 0.5) as f64),
                slots,
            });
        }
    }
    fn populate(&mut self, kind: &mut PieceKind, random: &mut JavaRandom) {
        let [x, y, z] = self.bounds.min;
        let [xx, yy, zz] = self.bounds.max;
        match kind {
            PieceKind::Stairs => {
                self.fill([0, 5, 0], [2, 7, 1], 0, false);
                self.fill([0, 0, 7], [2, 2, 8], 0, false);
                for i in 0..5 {
                    self.fill(
                        [0, 5 - i - i32::from(i < 4), 2 + i],
                        [2, 7 - i, 2 + i],
                        0,
                        false,
                    );
                }
            }
            PieceKind::Room { entrances } => {
                self.fill([x, y, z], [xx, y, zz], 3 << 4, true);
                self.fill([x, y + 1, z], [xx, (y + 3).min(yy), zz], 0, false);
                for entry in entrances {
                    self.fill(
                        [entry.min[0], entry.max[1] - 2, entry.min[2]],
                        entry.max,
                        0,
                        false,
                    );
                }
                let min_y = y + 4;
                let width = (xx - x + 1) as f32;
                let height = (yy - min_y + 1) as f32;
                let depth = (zz - z + 1) as f32;
                let center_x = x as f32 + width / 2.0;
                let center_z = z as f32 + depth / 2.0;
                for by in min_y..=yy {
                    let fy = (by - min_y) as f32 / height;
                    for bx in x..=xx {
                        let fx = (bx as f32 - center_x) / (width * 0.5);
                        for bz in z..=zz {
                            let fz = (bz as f32 - center_z) / (depth * 0.5);
                            if fx * fx + fy * fy + fz * fz <= 1.05 {
                                self.set([bx, by, bz], 0);
                            }
                        }
                    }
                }
            }
            PieceKind::Cross {
                multiple_floors, ..
            } => {
                if *multiple_floors {
                    self.fill([x + 1, y, z], [xx - 1, y + 2, zz], 0, false);
                    self.fill([x, y, z + 1], [xx, y + 2, zz - 1], 0, false);
                    self.fill([x + 1, yy - 2, z], [xx - 1, yy, zz], 0, false);
                    self.fill([x, yy - 2, z + 1], [xx, yy, zz - 1], 0, false);
                    self.fill([x + 1, y + 3, z + 1], [xx - 1, y + 3, zz - 1], 0, false);
                } else {
                    self.fill([x + 1, y, z], [xx - 1, yy, zz], 0, false);
                    self.fill([x, y, z + 1], [xx, yy, zz - 1], 0, false);
                }
                for bx in [x + 1, xx - 1] {
                    for bz in [z + 1, zz - 1] {
                        self.fill([bx, y, bz], [bx, yy, bz], 5 << 4, false);
                    }
                }
                for bx in x..=xx {
                    for bz in z..=zz {
                        if self.get([bx, y - 1, bz]) >> 4 == 0 {
                            self.set([bx, y - 1, bz], 5 << 4);
                        }
                    }
                }
            }
            PieceKind::Corridor {
                rails,
                spiders,
                spawner_placed,
                sections,
            } => {
                let length = *sections * 5 - 1;
                self.fill([0, 0, 0], [2, 1, length], 0, false);
                self.random_fill(random, 0.8, [0, 2, 0], [2, 2, length], 0);
                if *spiders {
                    self.random_fill(random, 0.6, [0, 0, 0], [2, 1, length], 30 << 4);
                }
                for section in 0..*sections {
                    let z = 2 + section * 5;
                    self.fill([0, 0, z], [0, 1, z], 85 << 4, false);
                    self.fill([2, 0, z], [2, 1, z], 85 << 4, false);
                    if random.next_int(4) == 0 {
                        self.set([0, 2, z], 5 << 4);
                        self.set([2, 2, z], 5 << 4);
                    } else {
                        self.fill([0, 2, z], [2, 2, z], 5 << 4, false);
                    }
                    for (chance, dx, dz) in [
                        (0.1, 0, -1),
                        (0.1, 2, -1),
                        (0.1, 0, 1),
                        (0.1, 2, 1),
                        (0.05, 0, -2),
                        (0.05, 2, -2),
                        (0.05, 0, 2),
                        (0.05, 2, 2),
                    ] {
                        self.random_place(random, chance, [dx, 2, z + dz], 30 << 4);
                    }
                    self.random_place(random, 0.05, [1, 2, z - 1], 50 << 4 | 1);
                    self.random_place(random, 0.05, [1, 2, z + 1], 50 << 4 | 1);
                    if random.next_int(100) == 0 {
                        self.chest(random, [2, 0, z - 1]);
                    }
                    if random.next_int(100) == 0 {
                        self.chest(random, [0, 0, z + 1]);
                    }
                    if *spiders && !*spawner_placed {
                        let p = [1, 0, z - 1 + random.next_int(3)];
                        let wp = self.point(p);
                        if self.clip.contains(wp) {
                            *spawner_placed = true;
                            self.set(p, 52 << 4);
                            self.world.spawner(wp);
                        }
                    }
                }
                for x in 0..=2 {
                    for z in 0..=length {
                        if self.get([x, -1, z]) >> 4 == 0 {
                            self.set([x, -1, z], 5 << 4);
                        }
                    }
                }
                if *rails {
                    let state = 66 << 4 | rotate_metadata(66, 0, self.direction.unwrap()) as u16;
                    for z in 0..=length {
                        let below = self.get([1, -1, z]);
                        if below >> 4 != 0 && full_block(below) {
                            self.random_place(random, 0.7, [1, 0, z], state);
                        }
                    }
                }
            }
        }
    }
}

/// MapGenBase neighbor seed followed by MapGenStructure's unused nextInt and
/// MapGenMineshaft's chance/distance checks. The remaining RNG builds the start.
pub fn candidate(seed: i64, cx: i32, cz: i32) -> Option<Mineshaft> {
    let mut random = JavaRandom::new(seed);
    let a = random.next_long();
    let b = random.next_long();
    random.set_seed((cx as i64).wrapping_mul(a) ^ (cz as i64).wrapping_mul(b) ^ seed);
    random.next_i32();
    if random.next_double() < 0.004
        && random.next_int(80) < cx.wrapping_abs().max(cz.wrapping_abs())
    {
        Some(Mineshaft::layout(&mut random, cx, cz, 63))
    } else {
        None
    }
}
impl Mineshaft {
    pub fn layout(random: &mut JavaRandom, cx: i32, cz: i32, sea_level: i32) -> Self {
        let x = (cx << 4) + 2;
        let z = (cz << 4) + 2;
        let bounds = BoundingBox::new(
            [x, 50, z],
            [
                x + 7 + random.next_int(6),
                54 + random.next_int(6),
                z + 7 + random.next_int(6),
            ],
        );
        let root = Piece {
            bounds,
            direction: None,
            depth: 0,
            kind: PieceKind::Room {
                entrances: Vec::new(),
            },
        };
        let mut result = Self {
            chunk: [cx, cz],
            bounds,
            pieces: vec![root],
        };
        result.grow(0, random);
        result.bounds = BoundingBox::empty();
        for p in &result.pieces {
            result.bounds.expand(&p.bounds);
        }
        let available = sea_level - 10;
        let mut height = result.bounds.size()[1] + 1;
        if height < available {
            height += random.next_int(available - height);
        }
        let dy = height - result.bounds.max[1];
        result.bounds.offset(0, dy, 0);
        for p in &mut result.pieces {
            p.bounds.offset(0, dy, 0);
            if let PieceKind::Room { entrances } = &mut p.kind {
                for b in entrances {
                    b.offset(0, dy, 0);
                }
            }
        }
        result
    }
    fn intersects(&self, b: &BoundingBox) -> bool {
        self.pieces.iter().any(|p| p.bounds.intersects(b))
    }
    fn attach(
        &mut self,
        random: &mut JavaRandom,
        x: i32,
        y: i32,
        z: i32,
        direction: u8,
        depth: i32,
    ) -> Option<usize> {
        let root = self.pieces[0].bounds;
        if depth > 8 || (x - root.min[0]).abs() > 80 || (z - root.min[2]).abs() > 80 {
            return None;
        }
        let choice = random.next_int(100);
        let piece = if choice >= 80 {
            let mut bounds = BoundingBox::new([x, y, z], [x, y + 2, z]);
            if random.next_int(4) == 0 {
                bounds.max[1] += 4;
            }
            match direction {
                2 => {
                    bounds.min[0] = x - 1;
                    bounds.max[0] = x + 3;
                    bounds.min[2] = z - 4;
                }
                0 => {
                    bounds.min[0] = x - 1;
                    bounds.max[0] = x + 3;
                    bounds.max[2] = z + 4;
                }
                1 => {
                    bounds.min[0] = x - 4;
                    bounds.min[2] = z - 1;
                    bounds.max[2] = z + 3;
                }
                _ => {
                    bounds.max[0] = x + 4;
                    bounds.min[2] = z - 1;
                    bounds.max[2] = z + 3;
                }
            }
            if self.intersects(&bounds) {
                return None;
            }
            Piece {
                bounds,
                direction: None,
                depth: depth + 1,
                kind: PieceKind::Cross {
                    direction,
                    multiple_floors: bounds.size()[1] > 3,
                },
            }
        } else if choice >= 70 {
            let mut bounds = BoundingBox::new([x, y - 5, z], [x, y + 2, z]);
            match direction {
                2 => {
                    bounds.max[0] = x + 2;
                    bounds.min[2] = z - 8;
                }
                0 => {
                    bounds.max[0] = x + 2;
                    bounds.max[2] = z + 8;
                }
                1 => {
                    bounds.min[0] = x - 8;
                    bounds.max[2] = z + 2;
                }
                _ => {
                    bounds.max[0] = x + 8;
                    bounds.max[2] = z + 2;
                }
            }
            if self.intersects(&bounds) {
                return None;
            }
            Piece {
                bounds,
                direction: Some(direction),
                depth: depth + 1,
                kind: PieceKind::Stairs,
            }
        } else {
            let mut sections = random.next_int(3) + 2;
            let mut bounds = BoundingBox::new([x, y, z], [x, y + 2, z]);
            while sections > 0 {
                let length = sections * 5;
                match direction {
                    2 => {
                        bounds.max[0] = x + 2;
                        bounds.min[2] = z - (length - 1);
                    }
                    0 => {
                        bounds.max[0] = x + 2;
                        bounds.max[2] = z + (length - 1);
                    }
                    1 => {
                        bounds.min[0] = x - (length - 1);
                        bounds.max[2] = z + 2;
                    }
                    _ => {
                        bounds.max[0] = x + (length - 1);
                        bounds.max[2] = z + 2;
                    }
                }
                if !self.intersects(&bounds) {
                    break;
                }
                sections -= 1;
            }
            if sections <= 0 {
                return None;
            }
            let rails = random.next_int(3) == 0;
            let spiders = !rails && random.next_int(23) == 0;
            Piece {
                bounds,
                direction: Some(direction),
                depth: depth + 1,
                kind: PieceKind::Corridor {
                    rails,
                    spiders,
                    spawner_placed: false,
                    sections,
                },
            }
        };
        let index = self.pieces.len();
        self.pieces.push(piece);
        self.grow(index, random);
        Some(index)
    }
    fn grow(&mut self, index: usize, random: &mut JavaRandom) {
        let piece = self.pieces[index].clone();
        let b = piece.bounds;
        let [x, y, z] = b.min;
        let [xx, _, zz] = b.max;
        let depth = piece.depth;
        match piece.kind {
            PieceKind::Room { .. } => {
                let vertical = (b.size()[1] - 4).max(1);
                for direction in [2, 0, 1, 3] {
                    let length = b.size()[if direction == 2 || direction == 0 {
                        0
                    } else {
                        2
                    }];
                    let mut along = 0;
                    while along < length {
                        along += random.next_int(length);
                        if along + 3 > length {
                            break;
                        }
                        let y = y + random.next_int(vertical) + 1;
                        let (at_x, at_z) = match direction {
                            2 => (x + along, z - 1),
                            0 => (x + along, zz + 1),
                            1 => (x - 1, z + along),
                            _ => (xx + 1, z + along),
                        };
                        if let Some(child) = self.attach(random, at_x, y, at_z, direction, depth) {
                            let c = self.pieces[child].bounds;
                            let entrance = match direction {
                                2 => BoundingBox::new(
                                    [c.min[0], c.min[1], z],
                                    [c.max[0], c.max[1], z + 1],
                                ),
                                0 => BoundingBox::new(
                                    [c.min[0], c.min[1], zz - 1],
                                    [c.max[0], c.max[1], zz],
                                ),
                                1 => BoundingBox::new(
                                    [x, c.min[1], c.min[2]],
                                    [x + 1, c.max[1], c.max[2]],
                                ),
                                _ => BoundingBox::new(
                                    [xx - 1, c.min[1], c.min[2]],
                                    [xx, c.max[1], c.max[2]],
                                ),
                            };
                            if let PieceKind::Room { entrances } = &mut self.pieces[index].kind {
                                entrances.push(entrance);
                            }
                        }
                        along += 4;
                    }
                }
            }
            PieceKind::Stairs => {
                let direction = piece.direction.unwrap();
                let (at_x, at_z) = match direction {
                    2 => (x, z - 1),
                    0 => (x, zz + 1),
                    1 => (x - 1, z),
                    _ => (xx + 1, z),
                };
                self.attach(random, at_x, y, at_z, direction, depth);
            }
            PieceKind::Cross {
                direction,
                multiple_floors,
            } => {
                let branches: &[u8] = match direction {
                    2 => &[2, 1, 3],
                    0 => &[0, 1, 3],
                    1 => &[2, 0, 1],
                    _ => &[2, 0, 3],
                };
                for &direction in branches {
                    let (at_x, at_z) = cross_exit(b, direction);
                    self.attach(random, at_x, y, at_z, direction, depth);
                }
                if multiple_floors {
                    for direction in [2, 1, 3, 0] {
                        if random.next_boolean() {
                            let (at_x, at_z) = cross_exit(b, direction);
                            self.attach(random, at_x, y + 4, at_z, direction, depth);
                        }
                    }
                }
            }
            PieceKind::Corridor { .. } => {
                let direction = piece.direction.unwrap();
                let choice = random.next_int(4);
                let (at_x, at_z, next) = match direction {
                    2 => {
                        if choice <= 1 {
                            (x, z - 1, 2)
                        } else if choice == 2 {
                            (x - 1, z, 1)
                        } else {
                            (xx + 1, z, 3)
                        }
                    }
                    0 => {
                        if choice <= 1 {
                            (x, zz + 1, 0)
                        } else if choice == 2 {
                            (x - 1, zz - 3, 1)
                        } else {
                            (xx + 1, zz - 3, 3)
                        }
                    }
                    1 => {
                        if choice <= 1 {
                            (x - 1, z, 1)
                        } else if choice == 2 {
                            (x, z - 1, 2)
                        } else {
                            (x, zz + 1, 0)
                        }
                    }
                    _ => {
                        if choice <= 1 {
                            (xx + 1, z, 3)
                        } else if choice == 2 {
                            (xx - 3, z - 1, 2)
                        } else {
                            (xx - 3, zz + 1, 0)
                        }
                    }
                };
                let at_y = y - 1 + random.next_int(3);
                self.attach(random, at_x, at_y, at_z, next, depth);
                if depth >= 8 {
                    return;
                }
                let vertical = direction == 0 || direction == 2;
                let mut along = if vertical { z } else { x } + 3;
                let end = if vertical { zz } else { xx };
                while along + 3 <= end {
                    match random.next_int(5) {
                        0 => {
                            if vertical {
                                self.attach(random, x - 1, y, along, 1, depth + 1);
                            } else {
                                self.attach(random, along, y, z - 1, 2, depth + 1);
                            }
                        }
                        1 => {
                            if vertical {
                                self.attach(random, xx + 1, y, along, 3, depth + 1);
                            } else {
                                self.attach(random, along, y, zz + 1, 0, depth + 1);
                            }
                        }
                        _ => {}
                    }
                    along += 5;
                }
            }
        }
    }
}
fn cross_exit(b: BoundingBox, direction: u8) -> (i32, i32) {
    match direction {
        2 => (b.min[0] + 1, b.min[2] - 1),
        0 => (b.min[0] + 1, b.max[2] + 1),
        1 => (b.min[0] - 1, b.min[2] + 1),
        _ => (b.max[0] + 1, b.min[2] + 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Chunk;
    use std::{collections::HashMap, path::PathBuf, sync::Arc};

    #[test]
    fn structure_chance_and_map_seed_match_original_candidates() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-mineshafts.json"
        ))
        .unwrap();
        let expected: HashMap<(i64, i32, i32), &serde_json::Value> = fixture["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                (
                    (
                        row["seed"].as_str().unwrap().parse().unwrap(),
                        row["chunk"][0].as_i64().unwrap() as i32,
                        row["chunk"][1].as_i64().unwrap() as i32,
                    ),
                    row,
                )
            })
            .collect();
        for seed in [0, 1, -1, 123456789012345, i64::MIN] {
            for x in -80..=80 {
                for z in -80..=80 {
                    let start = candidate(seed, x, z);
                    let expected = expected.get(&(seed, x, z));
                    assert_eq!(
                        start.is_some(),
                        expected.is_some(),
                        "seed {seed} candidate {x},{z}"
                    );
                    if let (Some(start), Some(row)) = (start, expected) {
                        assert_eq!(serde_json::json!(box_array(start.bounds)), row["bounds"]);
                        assert_eq!(start.pieces.len() as u64, row["count"].as_u64().unwrap());
                    }
                }
            }
        }
    }

    #[test]
    fn populated_blocks_carts_spawners_and_rng_match_original_pieces() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-mineshafts.json"
        ))
        .unwrap();
        for row in fixture["populations"].as_array().unwrap() {
            let seed: i64 = row["seed"].as_str().unwrap().parse().unwrap();
            let coord =
                |row: &serde_json::Value, axis: usize| row["chunk"][axis].as_i64().unwrap() as i32;
            let pattern = row["pattern"].as_u64().unwrap();
            let clips = row["clips"].as_array().unwrap();
            let mut world = World {
                chunks: crate::world::ChunkMap::new(),
                spawn: glam::Vec3::ZERO,
                label: String::new(),
                save_path: PathBuf::new(),
                edits: Default::default(),
                imported: false,
                light_revision: 0,
            };
            for cx in clips.iter().map(|r| coord(r, 0)).min().unwrap()
                ..=clips.iter().map(|r| coord(r, 0) + 1).max().unwrap()
            {
                for cz in clips.iter().map(|r| coord(r, 1)).min().unwrap()
                    ..=clips.iter().map(|r| coord(r, 1) + 1).max().unwrap()
                {
                    let mut chunk = Chunk::empty();
                    for y in 0..80 {
                        for z in 0..16 {
                            for x in 0..16 {
                                let wx = cx * 16 + x;
                                let wz = cz * 16 + z;
                                let mut id = if y == 0 { 7 } else { 1 };
                                if pattern == 1
                                    && (20..=60).contains(&y)
                                    && wx.rem_euclid(13) < 4
                                    && wz.rem_euclid(17) < 8
                                {
                                    id = 0;
                                }
                                if pattern == 2
                                    && y > 0
                                    && wx.rem_euclid(31) == 0
                                    && wz.rem_euclid(29) < 5
                                {
                                    id = 9;
                                }
                                chunk.set(x, y, z, id << 4);
                            }
                        }
                    }
                    world.chunks.insert((cx, cz), Arc::new(chunk));
                }
            }
            let mut start =
                Mineshaft::layout(&mut JavaRandom::new(seed), coord(row, 0), coord(row, 1), 63);
            let mut spawners = Vec::new();
            for expected in clips {
                let cx = coord(expected, 0);
                let cz = coord(expected, 1);
                let clip = BoundingBox::new(
                    [cx * 16 + 8, 1, cz * 16 + 8],
                    [cx * 16 + 23, 512, cz * 16 + 23],
                );
                let mut random = JavaRandom::new(
                    seed ^ (cx as i64).wrapping_mul(341873128712)
                        ^ (cz as i64).wrapping_mul(132897987541),
                );
                let result = start.populate(&mut world, &mut random, clip);
                let context = format!("seed {seed} pattern {pattern} clip {cx},{cz}");
                assert_eq!(
                    random.next_long().to_string(),
                    expected["next_long"].as_str().unwrap(),
                    "RNG {context}"
                );
                assert_eq!(
                    serde_json::to_value(&result.carts).unwrap(),
                    expected["carts"],
                    "carts {context}"
                );
                spawners.extend(result.spawners);
                let mut hash = 0xcbf29ce484222325_u64;
                for y in 0..80 {
                    for z in clip.min[2]..=clip.max[2] {
                        for x in clip.min[0]..=clip.max[0] {
                            hash ^= world.get(IVec3::new(x, y, z)) as u64;
                            hash = hash.wrapping_mul(0x100000001b3);
                        }
                    }
                }
                assert_eq!(
                    hash.to_string(),
                    expected["hash"].as_str().unwrap(),
                    "blocks {context}"
                );
            }
            let pieces: Vec<_> = start.pieces.iter().map(piece_json).collect();
            assert_eq!(
                serde_json::json!(pieces),
                row["pieces"],
                "remaining components seed {seed}, pattern {pattern}"
            );
            // A later component could overwrite a generated spawner.
            spawners.retain(|p| world.get(IVec3::from_array(*p)) >> 4 == 52);
            let mut expected: Vec<[i32; 3]> =
                serde_json::from_value(row["spawners"].clone()).unwrap();
            spawners.sort();
            expected.sort();
            assert_eq!(spawners, expected);
        }
    }
    fn box_array(b: BoundingBox) -> [i32; 6] {
        [b.min[0], b.min[1], b.min[2], b.max[0], b.max[1], b.max[2]]
    }
    fn piece_json(p: &Piece) -> serde_json::Value {
        let name = match p.kind {
            PieceKind::Room { .. } => "MSRoom",
            PieceKind::Corridor { .. } => "MSCorridor",
            PieceKind::Cross { .. } => "MSCrossing",
            PieceKind::Stairs => "MSStairs",
        };
        let mut value = serde_json::json!({"id":name,"bounds":box_array(p.bounds),"direction":p.direction.map_or(-1,i32::from),"depth":p.depth});
        match &p.kind {
            PieceKind::Room { entrances } => {
                value["Entrances"] =
                    serde_json::json!(entrances.iter().copied().map(box_array).collect::<Vec<_>>())
            }
            PieceKind::Corridor {
                rails,
                spiders,
                spawner_placed,
                sections,
            } => {
                value["hr"] = (*rails).into();
                value["sc"] = (*spiders).into();
                value["hps"] = (*spawner_placed).into();
                value["Num"] = (*sections).into();
            }
            PieceKind::Cross {
                direction,
                multiple_floors,
            } => {
                value["D"] = (*direction).into();
                value["tf"] = (*multiple_floors).into();
            }
            PieceKind::Stairs => {}
        }
        value
    }
    #[test]
    fn layout_matches_every_original_piece_nbt_and_rng_consumption() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-mineshafts.json"
        ))
        .unwrap();
        let mut count = 0;
        for row in fixture["cases"].as_array().unwrap() {
            let seed = row["seed"].as_str().unwrap().parse().unwrap();
            let mut random = JavaRandom::new(seed);
            let cx = row["chunk"][0].as_i64().unwrap() as i32;
            let cz = row["chunk"][1].as_i64().unwrap() as i32;
            let start = Mineshaft::layout(&mut random, cx, cz, 63);
            assert_eq!(
                serde_json::json!(box_array(start.bounds)),
                row["bounds"],
                "seed{seed} bounds"
            );
            assert_eq!(
                start.pieces.len(),
                row["pieces"].as_array().unwrap().len(),
                "seed{seed} piececount"
            );
            for (i, (piece, expected)) in start
                .pieces
                .iter()
                .zip(row["pieces"].as_array().unwrap())
                .enumerate()
            {
                assert_eq!(
                    piece_json(piece),
                    *expected,
                    "seed{seed} chunk{cx},{cz} piece{i}"
                );
                count += 1;
            }
            assert_eq!(
                random.next_long().to_string(),
                row["next_long"].as_str().unwrap()
            );
        }
        assert_eq!(count, 2145);
    }
}
