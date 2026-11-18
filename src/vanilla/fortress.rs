//! Original 1.8.9 Nether fortress candidate and component construction.
use super::{random::JavaRandom, structure_common::BoundingBox};
use serde::{Deserialize, Serialize};
#[path = "fortress_population.rs"]
mod population;
#[path = "fortress_templates.rs"]
mod templates;
use super::{
    loot::{fill_loot, LootEntry, LootStack},
    material,
    structure_common::rotate_metadata,
};
#[allow(unused_imports)] // Public adapters for the forthcoming Nether provider.
pub use population::{FortressWorld, Population, WorldAdapter};
#[cfg(test)]
#[path = "fortress_population_tests.rs"]
mod population_tests;
#[cfg(test)]
#[path = "fortress_tests.rs"]
mod tests;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Straight,
    Crossing3,
    Crossing,
    Stairs,
    Throne,
    Entrance,
    Corridor5,
    Crossing2,
    Corridor2,
    Corridor,
    Corridor3,
    Corridor4,
    NetherStalkRoom,
    End,
    Start,
}
impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Straight => "NeBS",
            Self::Crossing3 => "NeBCr",
            Self::Crossing => "NeRC",
            Self::Stairs => "NeSR",
            Self::Throne => "NeMT",
            Self::Entrance => "NeCE",
            Self::Corridor5 => "NeSC",
            Self::Crossing2 => "NeSCSC",
            Self::Corridor2 => "NeSCRT",
            Self::Corridor => "NeSCLT",
            Self::Corridor3 => "NeCCS",
            Self::Corridor4 => "NeCTB",
            Self::NetherStalkRoom => "NeCSR",
            Self::End => "NeBEF",
            Self::Start => "NeStart",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Piece {
    pub kind: Kind,
    pub bounds: BoundingBox,
    pub orientation: u8,
    pub depth: i32,
    pub chest_pending: bool,
    pub spawner_made: bool,
    pub fill_seed: i32,
}
impl Piece {
    fn blank(kind: Kind, bounds: BoundingBox, orientation: u8, depth: i32) -> Self {
        Self {
            kind,
            bounds,
            orientation,
            depth,
            chest_pending: false,
            spawner_made: false,
            fill_seed: 0,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fortress {
    pub chunk: [i32; 2],
    pub bounds: BoundingBox,
    pub pieces: Vec<Piece>,
}
/// MapGenNetherBridge resets its own RNG, independently of World.rand. These
/// explicit early returns are the original bytecode's conditional draws.
fn candidate_random(seed: i64, cx: i32, cz: i32, random: &mut JavaRandom) -> bool {
    let rx = cx >> 4;
    let rz = cz >> 4;
    random.set_seed((rx ^ (rz << 4)) as i64 ^ seed);
    random.next_i32();
    if random.next_int(3) != 0 {
        return false;
    }
    if cx != (rx << 4) + 4 + random.next_int(8) {
        return false;
    }
    cz == (rz << 4) + 4 + random.next_int(8)
}
pub fn candidate(seed: i64, cx: i32, cz: i32) -> Option<Fortress> {
    let mut random = JavaRandom::new(0);
    candidate_random(seed, cx, cz, &mut random).then(|| Fortress::with_random(&mut random, cx, cz))
}
const KINDS: [Kind; 13] = [
    Kind::Straight,
    Kind::Crossing3,
    Kind::Crossing,
    Kind::Stairs,
    Kind::Throne,
    Kind::Entrance,
    Kind::Corridor5,
    Kind::Crossing2,
    Kind::Corridor2,
    Kind::Corridor,
    Kind::Corridor3,
    Kind::Corridor4,
    Kind::NetherStalkRoom,
];
const WEIGHTS: [i32; 13] = [30, 10, 10, 10, 5, 5, 25, 15, 5, 5, 10, 7, 5];
const LIMITS: [i32; 13] = [0, 4, 4, 3, 2, 1, 0, 5, 10, 10, 3, 2, 2];
const REPEAT: [bool; 13] = [
    true, false, false, false, false, false, true, false, false, false, true, false, false,
];
struct Builder {
    pieces: Vec<Piece>,
    pending: Vec<usize>,
    counts: [i32; 13],
    active: [bool; 13],
    last: Option<usize>,
}
impl Builder {
    fn new(random: &mut JavaRandom, cx: i32, cz: i32) -> Self {
        let x = (cx << 4) + 2;
        let z = (cz << 4) + 2;
        let start = Piece::blank(
            Kind::Start,
            BoundingBox::new([x, 64, z], [x + 18, 73, z + 18]),
            [2, 3, 0, 1][random.next_int(4) as usize],
            0,
        );
        Self {
            pieces: vec![start],
            pending: Vec::new(),
            counts: [0; 13],
            active: [true; 13],
            last: None,
        }
    }
    fn create(
        &self,
        kind: Kind,
        p: [i32; 3],
        orientation: u8,
        depth: i32,
        random: &mut JavaRandom,
    ) -> Option<Piece> {
        let (offset, size) = match kind {
            Kind::Straight => ([-1, -3, 0], [5, 10, 19]),
            Kind::Crossing3 => ([-8, -3, 0], [19, 10, 19]),
            Kind::Crossing => ([-2, 0, 0], [7, 9, 7]),
            Kind::Stairs => ([-2, 0, 0], [7, 11, 7]),
            Kind::Throne => ([-2, 0, 0], [7, 8, 9]),
            Kind::Entrance | Kind::NetherStalkRoom => ([-5, -3, 0], [13, 14, 13]),
            Kind::Corridor3 => ([-1, -7, 0], [5, 14, 10]),
            Kind::Corridor4 => ([-3, 0, 0], [9, 7, 9]),
            Kind::End => ([-1, -3, 0], [5, 10, 8]),
            Kind::Corridor5 | Kind::Corridor | Kind::Corridor2 | Kind::Crossing2 => {
                ([-1, 0, 0], [5, 7, 5])
            }
            Kind::Start => unreachable!(),
        };
        let bounds = BoundingBox::oriented(p[0], p[1], p[2], offset, size, orientation);
        if bounds.min[1] <= 10
            || self
                .pieces
                .iter()
                .any(|piece| piece.bounds.intersects(&bounds))
        {
            return None;
        }
        let mut piece = Piece::blank(kind, bounds, orientation, depth);
        if matches!(kind, Kind::Corridor | Kind::Corridor2) {
            piece.chest_pending = random.next_int(3) == 0;
        }
        if kind == Kind::End {
            piece.fill_seed = random.next_i32();
        }
        Some(piece)
    }
    fn choose(
        &mut self,
        p: [i32; 3],
        orientation: u8,
        depth: i32,
        secondary: bool,
        random: &mut JavaRandom,
    ) -> Option<Piece> {
        let range = if secondary { 6..13 } else { 0..6 };
        let (mut total, mut more) = (0, false);
        for i in range.clone() {
            if self.active[i] {
                total += WEIGHTS[i];
                if LIMITS[i] > 0 && self.counts[i] < LIMITS[i] {
                    more = true;
                }
            }
        }
        if more && total > 0 && depth <= 30 {
            'attempt: for _ in 0..5 {
                let mut weight = random.next_int(total);
                for i in range.clone() {
                    if !self.active[i] {
                        continue;
                    }
                    weight -= WEIGHTS[i];
                    if weight >= 0 {
                        continue;
                    }
                    if (LIMITS[i] != 0 && self.counts[i] >= LIMITS[i])
                        || (self.last == Some(i) && !REPEAT[i])
                    {
                        continue 'attempt;
                    }
                    if let Some(piece) = self.create(KINDS[i], p, orientation, depth, random) {
                        self.counts[i] += 1;
                        self.last = Some(i);
                        if LIMITS[i] != 0 && self.counts[i] >= LIMITS[i] {
                            self.active[i] = false;
                        }
                        return Some(piece);
                    }
                }
            }
        }
        self.create(Kind::End, p, orientation, depth, random)
    }
    fn connect(
        &mut self,
        index: usize,
        side: u8,
        a: i32,
        b: i32,
        secondary: bool,
        random: &mut JavaRandom,
    ) {
        let piece = &self.pieces[index];
        let orientation = piece.orientation;
        let min = piece.bounds.min;
        let max = piece.bounds.max;
        let depth = piece.depth;
        let (p, facing) = match (side, orientation) {
            (0, 2) => ([min[0] + a, min[1] + b, min[2] - 1], orientation),
            (0, 0) => ([min[0] + a, min[1] + b, max[2] + 1], orientation),
            (0, 1) => ([min[0] - 1, min[1] + b, min[2] + a], orientation),
            (0, 3) => ([max[0] + 1, min[1] + b, min[2] + a], orientation),
            (1, 0 | 2) => ([min[0] - 1, min[1] + a, min[2] + b], 1),
            (1, 1 | 3) => ([min[0] + b, min[1] + a, min[2] - 1], 2),
            (2, 0 | 2) => ([max[0] + 1, min[1] + a, min[2] + b], 3),
            (2, 1 | 3) => ([min[0] + b, min[1] + a, max[2] + 1], 0),
            _ => unreachable!(),
        };
        let start = self.pieces[0].bounds.min;
        if (p[0] - start[0]).abs() > 112 || (p[2] - start[2]).abs() > 112 {
            let _ = self.create(Kind::End, p, facing, depth, random);
            return;
        }
        if let Some(piece) = self.choose(p, facing, depth + 1, secondary, random) {
            self.pending.push(self.pieces.len());
            self.pieces.push(piece);
        }
    }
    fn build(&mut self, index: usize, random: &mut JavaRandom) {
        let piece = self.pieces[index].clone();
        match piece.kind {
            Kind::Start | Kind::Crossing3 => {
                self.connect(index, 0, 8, 3, false, random);
                self.connect(index, 1, 3, 8, false, random);
                self.connect(index, 2, 3, 8, false, random);
            }
            Kind::Straight => self.connect(index, 0, 1, 3, false, random),
            Kind::Crossing => {
                self.connect(index, 0, 2, 0, false, random);
                self.connect(index, 1, 0, 2, false, random);
                self.connect(index, 2, 0, 2, false, random);
            }
            Kind::Stairs => self.connect(index, 2, 6, 2, false, random),
            Kind::Entrance => self.connect(index, 0, 5, 3, true, random),
            Kind::Corridor5 | Kind::Corridor3 => self.connect(index, 0, 1, 0, true, random),
            Kind::Corridor => self.connect(index, 1, 0, 1, true, random),
            Kind::Corridor2 => self.connect(index, 2, 0, 1, true, random),
            Kind::Crossing2 => {
                self.connect(index, 0, 1, 0, true, random);
                self.connect(index, 1, 0, 1, true, random);
                self.connect(index, 2, 0, 1, true, random);
            }
            Kind::Corridor4 => {
                let b = if matches!(piece.orientation, 1 | 2) {
                    5
                } else {
                    1
                };
                let secondary = random.next_int(8) > 0;
                self.connect(index, 1, 0, b, secondary, random);
                let secondary = random.next_int(8) > 0;
                self.connect(index, 2, 0, b, secondary, random);
            }
            Kind::NetherStalkRoom => {
                self.connect(index, 0, 5, 3, true, random);
                self.connect(index, 0, 5, 11, true, random);
            }
            Kind::Throne | Kind::End => {}
        }
    }
}
impl Fortress {
    fn with_random(random: &mut JavaRandom, cx: i32, cz: i32) -> Self {
        let mut builder = Builder::new(random, cx, cz);
        builder.build(0, random);
        while !builder.pending.is_empty() {
            let chosen = random.next_int(builder.pending.len() as i32) as usize;
            let index = builder.pending.remove(chosen);
            builder.build(index, random);
        }
        let mut bounds = BoundingBox::empty();
        for piece in &builder.pieces {
            bounds.expand(&piece.bounds);
        }
        let range = 23 - (bounds.max[1] - bounds.min[1] + 1);
        let y = if range > 1 {
            48 + random.next_int(range)
        } else {
            48
        };
        let dy = y - bounds.min[1];
        bounds.offset(0, dy, 0);
        for piece in &mut builder.pieces {
            piece.bounds.offset(0, dy, 0);
        }
        Self {
            chunk: [cx, cz],
            bounds,
            pieces: builder.pieces,
        }
    }
}
