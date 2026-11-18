//! Original 1.8.9 stronghold candidate search and component graph.
use super::{
    biome::{self, BiomeGenerator},
    random::JavaRandom,
    structure_common::BoundingBox,
};
use serde::{Deserialize, Serialize};
#[path = "stronghold_population.rs"]
mod population;
#[path = "stronghold_templates.rs"]
mod templates;
use super::{
    loot::{fill_loot, LootEntry, LootStack},
    material,
    structure_common::rotate_metadata,
};
pub use population::{Population, StrongholdWorld, WorldAdapter};
#[cfg(test)]
#[path = "stronghold_population_tests.rs"]
mod population_tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Straight,
    Prison,
    LeftTurn,
    RightTurn,
    RoomCrossing,
    StairsStraight,
    Stairs,
    Crossing,
    ChestCorridor,
    Library,
    PortalRoom,
    Start,
    Corridor,
}
impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Straight => "SHS",
            Self::Prison => "SHPH",
            Self::LeftTurn => "SHLT",
            Self::RightTurn => "SHRT",
            Self::RoomCrossing => "SHRC",
            Self::StairsStraight => "SHSSD",
            Self::Stairs => "SHSD",
            Self::Crossing => "SH5C",
            Self::ChestCorridor => "SHCC",
            Self::Library => "SHLi",
            Self::PortalRoom => "SHPR",
            Self::Start => "SHStart",
            Self::Corridor => "SHFC",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Door {
    Opening,
    Wood,
    Grates,
    Iron,
}
impl Door {
    pub fn name(self) -> &'static str {
        match self {
            Self::Opening => "OPENING",
            Self::Wood => "WOOD_DOOR",
            Self::Grates => "GRATES",
            Self::Iron => "IRON_DOOR",
        }
    }
    fn random(random: &mut JavaRandom) -> Self {
        match random.next_int(5) {
            2 => Self::Wood,
            3 => Self::Grates,
            4 => Self::Iron,
            _ => Self::Opening,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Piece {
    pub kind: Kind,
    pub bounds: BoundingBox,
    pub orientation: u8,
    pub depth: i32,
    pub door: Door,
    /// Straight left/right or crossing left-low/high/right-low/high flags.
    pub exits: [bool; 4],
    pub room_type: i32,
    pub large: bool,
    pub source: bool,
    pub steps: i32,
    pub chest_made: bool,
    pub spawner_made: bool,
}
impl Piece {
    fn blank(kind: Kind, bounds: BoundingBox, orientation: u8, depth: i32) -> Self {
        Self {
            kind,
            bounds,
            orientation,
            depth,
            door: Door::Opening,
            exits: [false; 4],
            room_type: 0,
            large: false,
            source: false,
            steps: 0,
            chest_made: false,
            spawner_made: false,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stronghold {
    pub chunk: [i32; 2],
    pub bounds: BoundingBox,
    pub pieces: Vec<Piece>,
    pub portal: usize,
}

/// The default three candidates, including the original biased biome reservoir
/// (its counter increments only when a candidate replaces the selected entry).
pub fn positions(seed: i64, biomes: &BiomeGenerator) -> [[i32; 2]; 3] {
    positions_with(seed, |x, z, random| {
        biomes.find_biome_position(x, z, 112, random, |id| biome::biome(id).base_height > 0.0)
    })
}
fn positions_with(
    seed: i64,
    mut find: impl FnMut(i32, i32, &mut JavaRandom) -> Option<[i32; 2]>,
) -> [[i32; 2]; 3] {
    let mut random = JavaRandom::new(seed);
    let mut angle = random.next_double() * std::f64::consts::PI * 2.0;
    let mut out = [[0; 2]; 3];
    for p in &mut out {
        let distance = (1.25 + random.next_double()) * 32.0;
        // Java Math.round is floor(x+.5), including negative coordinates.
        let mut x = (angle.cos() * distance + 0.5).floor() as i32;
        let mut z = (angle.sin() * distance + 0.5).floor() as i32;
        if let Some(found) = find((x << 4) + 8, (z << 4) + 8, &mut random) {
            x = found[0] >> 4;
            z = found[1] >> 4;
        }
        *p = [x, z];
        angle += std::f64::consts::PI * 2.0 / 3.0;
    }
    out
}
const KINDS: [Kind; 11] = [
    Kind::Straight,
    Kind::Prison,
    Kind::LeftTurn,
    Kind::RightTurn,
    Kind::RoomCrossing,
    Kind::StairsStraight,
    Kind::Stairs,
    Kind::Crossing,
    Kind::ChestCorridor,
    Kind::Library,
    Kind::PortalRoom,
];
const WEIGHTS: [i32; 11] = [40, 5, 20, 20, 10, 5, 5, 5, 5, 10, 20];
const LIMITS: [i32; 11] = [0, 5, 0, 0, 6, 5, 5, 4, 4, 2, 1];
struct Builder {
    pieces: Vec<Piece>,
    pending: Vec<usize>,
    counts: [i32; 11],
    active: [bool; 11],
    last: Option<usize>,
    forced: bool,
    portal: Option<usize>,
}
impl Builder {
    fn new(random: &mut JavaRandom, cx: i32, cz: i32) -> Self {
        let x = cx.wrapping_mul(16).wrapping_add(2);
        let z = cz.wrapping_mul(16).wrapping_add(2);
        let mut start = Piece::blank(
            Kind::Start,
            BoundingBox::new([x, 64, z], [x + 4, 74, z + 4]),
            [2, 3, 0, 1][random.next_int(4) as usize],
            0,
        );
        start.source = true;
        Self {
            pieces: vec![start],
            pending: Vec::new(),
            counts: [0; 11],
            active: [true; 11],
            last: None,
            forced: false,
            portal: None,
        }
    }
    fn available(&self, bounds: BoundingBox) -> bool {
        bounds.min[1] > 10 && !self.pieces.iter().any(|p| p.bounds.intersects(&bounds))
    }
    fn create(
        &self,
        kind: Kind,
        random: &mut JavaRandom,
        p: [i32; 3],
        orientation: u8,
        depth: i32,
    ) -> Option<Piece> {
        let (offset, size) = match kind {
            Kind::Straight => ([-1, -1, 0], [5, 5, 7]),
            Kind::Prison => ([-1, -1, 0], [9, 5, 11]),
            Kind::LeftTurn | Kind::RightTurn => ([-1, -1, 0], [5, 5, 5]),
            Kind::RoomCrossing => ([-4, -1, 0], [11, 7, 11]),
            Kind::StairsStraight => ([-1, -7, 0], [5, 11, 8]),
            Kind::Stairs => ([-1, -7, 0], [5, 11, 5]),
            Kind::Crossing => ([-4, -3, 0], [10, 9, 11]),
            Kind::ChestCorridor => ([-1, -1, 0], [5, 5, 7]),
            Kind::Library => ([-4, -1, 0], [14, 11, 15]),
            Kind::PortalRoom => ([-4, -1, 0], [11, 8, 16]),
            _ => unreachable!(),
        };
        let mut bounds = BoundingBox::oriented(p[0], p[1], p[2], offset, size, orientation);
        if !self.available(bounds) {
            if kind != Kind::Library {
                return None;
            }
            bounds = BoundingBox::oriented(p[0], p[1], p[2], offset, [14, 6, 15], orientation);
            if !self.available(bounds) {
                return None;
            }
        }
        // Original RightTurn inherits a static factory returning LeftTurn.
        let actual = if kind == Kind::RightTurn {
            Kind::LeftTurn
        } else {
            kind
        };
        let mut piece = Piece::blank(actual, bounds, orientation, depth);
        if kind != Kind::PortalRoom {
            piece.door = Door::random(random);
        }
        match kind {
            Kind::Straight => {
                piece.exits[0] = random.next_int(2) == 0;
                piece.exits[1] = random.next_int(2) == 0;
            }
            Kind::Crossing => {
                piece.exits = [
                    random.next_boolean(),
                    random.next_boolean(),
                    random.next_boolean(),
                    random.next_int(3) > 0,
                ];
            }
            Kind::RoomCrossing => piece.room_type = random.next_int(5),
            Kind::Library => piece.large = bounds.max[1] - bounds.min[1] + 1 > 6,
            _ => {}
        }
        Some(piece)
    }
    fn choose(
        &mut self,
        random: &mut JavaRandom,
        p: [i32; 3],
        orientation: u8,
        depth: i32,
    ) -> Option<Piece> {
        let mut total = 0;
        let mut more = false;
        for i in 0..11 {
            if self.active[i] {
                total += WEIGHTS[i];
                if LIMITS[i] > 0 && self.counts[i] < LIMITS[i] {
                    more = true;
                }
            }
        }
        if !more {
            return None;
        }
        if self.forced {
            self.forced = false;
            if let Some(piece) = self.create(Kind::Crossing, random, p, orientation, depth) {
                return Some(piece);
            }
        }
        'attempt: for _ in 0..5 {
            let mut weight = random.next_int(total);
            for i in 0..11 {
                if !self.active[i] {
                    continue;
                }
                weight -= WEIGHTS[i];
                if weight >= 0 {
                    continue;
                }
                if (LIMITS[i] != 0 && self.counts[i] >= LIMITS[i])
                    || (i == 9 && depth <= 4)
                    || (i == 10 && depth <= 5)
                    || self.last == Some(i)
                {
                    continue 'attempt;
                }
                if let Some(piece) = self.create(KINDS[i], random, p, orientation, depth) {
                    self.counts[i] += 1;
                    self.last = Some(i);
                    if LIMITS[i] != 0 && self.counts[i] >= LIMITS[i] {
                        self.active[i] = false;
                    }
                    return Some(piece);
                }
            }
        }
        let bounds = BoundingBox::oriented(p[0], p[1], p[2], [-1, -1, 0], [5, 5, 4], orientation);
        let found = self
            .pieces
            .iter()
            .find(|piece| piece.bounds.intersects(&bounds))?;
        if found.bounds.min[1] == bounds.min[1] {
            for length in (1..=3).rev() {
                let test = BoundingBox::oriented(
                    p[0],
                    p[1],
                    p[2],
                    [-1, -1, 0],
                    [5, 5, length - 1],
                    orientation,
                );
                if !found.bounds.intersects(&test) {
                    let bounds = BoundingBox::oriented(
                        p[0],
                        p[1],
                        p[2],
                        [-1, -1, 0],
                        [5, 5, length],
                        orientation,
                    );
                    if bounds.min[1] > 1 {
                        let mut piece = Piece::blank(Kind::Corridor, bounds, orientation, depth);
                        piece.steps = length;
                        return Some(piece);
                    }
                    return None;
                }
            }
        }
        None
    }
    fn connect(&mut self, index: usize, side: u8, a: i32, b: i32, random: &mut JavaRandom) {
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
        if depth > 50 || (p[0] - start[0]).abs() > 112 || (p[2] - start[2]).abs() > 112 {
            return;
        }
        if let Some(piece) = self.choose(random, p, facing, depth + 1) {
            self.pending.push(self.pieces.len());
            self.pieces.push(piece);
        }
    }
    fn build(&mut self, index: usize, random: &mut JavaRandom) {
        let piece = self.pieces[index].clone();
        match piece.kind {
            Kind::Start | Kind::Stairs => {
                if piece.source {
                    self.forced = true;
                }
                self.connect(index, 0, 1, 1, random);
            }
            Kind::PortalRoom => self.portal = Some(index),
            Kind::Straight => {
                self.connect(index, 0, 1, 1, random);
                if piece.exits[0] {
                    self.connect(index, 1, 1, 2, random);
                }
                if piece.exits[1] {
                    self.connect(index, 2, 1, 2, random);
                }
            }
            Kind::Prison | Kind::StairsStraight | Kind::ChestCorridor => {
                self.connect(index, 0, 1, 1, random)
            }
            Kind::LeftTurn | Kind::RightTurn => {
                let left = matches!(piece.orientation, 2 | 3) ^ (piece.kind == Kind::RightTurn);
                self.connect(index, if left { 1 } else { 2 }, 1, 1, random);
            }
            Kind::RoomCrossing => {
                self.connect(index, 0, 4, 1, random);
                self.connect(index, 1, 1, 4, random);
                self.connect(index, 2, 1, 4, random);
            }
            Kind::Crossing => {
                let (low, high) = if matches!(piece.orientation, 1 | 2) {
                    (5, 3)
                } else {
                    (3, 5)
                };
                self.connect(index, 0, 5, 1, random);
                for (enabled, side, a, b) in [
                    (piece.exits[0], 1, low, 1),
                    (piece.exits[1], 1, high, 7),
                    (piece.exits[2], 2, low, 1),
                    (piece.exits[3], 2, high, 7),
                ] {
                    if enabled {
                        self.connect(index, side, a, b, random);
                    }
                }
            }
            Kind::Library | Kind::Corridor => {}
        }
    }
}
impl Stronghold {
    pub fn new(seed: i64, cx: i32, cz: i32) -> Self {
        let mut random = JavaRandom::new(seed);
        let a = random.next_long();
        let b = random.next_long();
        random.set_seed((cx as i64).wrapping_mul(a) ^ (cz as i64).wrapping_mul(b) ^ seed);
        random.next_i32();
        Self::with_random(&mut random, cx, cz)
    }
    fn with_random(random: &mut JavaRandom, cx: i32, cz: i32) -> Self {
        loop {
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
            let mut top = bounds.max[1] - bounds.min[1] + 2;
            if top < 53 {
                top += random.next_int(53 - top);
            }
            let dy = top - bounds.max[1];
            bounds.offset(0, dy, 0);
            for piece in &mut builder.pieces {
                piece.bounds.offset(0, dy, 0);
            }
            if let Some(portal) = builder.portal {
                return Self {
                    chunk: [cx, cz],
                    bounds,
                    pieces: builder.pieces,
                    portal,
                };
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn bbox(b: BoundingBox) -> [i32; 6] {
        [b.min[0], b.min[1], b.min[2], b.max[0], b.max[1], b.max[2]]
    }
    pub(super) fn nbt(p: &Piece) -> Value {
        let mut row = json!({"id":p.kind.id(),"BB":bbox(p.bounds),"O":p.orientation,"GD":p.depth,"EntryDoor":p.door.name()});
        match p.kind {
            Kind::PortalRoom => row["Mob"] = json!(p.spawner_made),
            Kind::Crossing => {
                for (i, key) in ["leftLow", "leftHigh", "rightLow", "rightHigh"]
                    .iter()
                    .enumerate()
                {
                    row[*key] = json!(p.exits[i]);
                }
            }
            Kind::Library => row["Tall"] = json!(p.large),
            Kind::RoomCrossing => row["Type"] = json!(p.room_type),
            Kind::ChestCorridor => row["Chest"] = json!(p.chest_made),
            Kind::Straight => {
                row["Left"] = json!(p.exits[0]);
                row["Right"] = json!(p.exits[1]);
            }
            Kind::Stairs | Kind::Start => row["Source"] = json!(p.source),
            Kind::Corridor => row["Steps"] = json!(p.steps),
            _ => {}
        }
        row
    }
    #[test]
    fn candidates_biome_reservoir_and_all_graph_nbt_match_original() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-stronghold.json"
        ))
        .unwrap();
        for row in fixture["positions"].as_array().unwrap() {
            let seed = row["seed"].as_str().unwrap().parse::<i64>().unwrap();
            assert_eq!(
                json!(positions(seed, &BiomeGenerator::new(seed))),
                row["positions"],
                "positions seed {seed}"
            );
        }
        let mut count = 0;
        for row in fixture["starts"].as_array().unwrap() {
            let seed = row["seed"].as_str().unwrap().parse::<i64>().unwrap();
            let cx = row["chunk"][0].as_i64().unwrap() as i32;
            let cz = row["chunk"][1].as_i64().unwrap() as i32;
            let mut random = JavaRandom::new(seed);
            let a = random.next_long();
            let b = random.next_long();
            random.set_seed((cx as i64).wrapping_mul(a) ^ (cz as i64).wrapping_mul(b) ^ seed);
            random.next_i32();
            let start = Stronghold::with_random(&mut random, cx, cz);
            let context = format!("seed={seed} chunk={cx},{cz}");
            assert_eq!(json!(bbox(start.bounds)), row["bounds"], "bounds {context}");
            assert_eq!(
                random.next_long().to_string(),
                row["next"].as_str().unwrap(),
                "RNG {context}"
            );
            let expected = row["pieces"].as_array().unwrap();
            assert_eq!(start.pieces.len(), expected.len(), "count {context}");
            for (i, (piece, expected)) in start.pieces.iter().zip(expected).enumerate() {
                assert_eq!(nbt(piece), *expected, "piece {i} {context}");
                count += 1;
            }
            assert_eq!(start.pieces[start.portal].kind, Kind::PortalRoom);
            let restored: Stronghold =
                serde_json::from_str(&serde_json::to_string(&start).unwrap()).unwrap();
            assert_eq!(restored.portal, start.portal);
        }
        assert_eq!(count, 14783);
    }
}
