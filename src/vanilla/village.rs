//! Original 1.8.9 village candidate selection and component layout.
//! Population templates are a separate stage; layout parity does not imply
//! populated block/loot/entity parity.
use super::{biome::BiomeGenerator, random::JavaRandom, structure_common::BoundingBox};
use serde::{Deserialize, Serialize};
#[path = "village_population.rs"]
mod population;
#[allow(unused_imports)]
pub use population::{VillageWorld, VillageWorldAdapter, VillagerSpawn};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PieceKind {
    Start,
    Path,
    Torch,
    House4Garden,
    Church,
    House1,
    WoodHut,
    Hall,
    Field1,
    Field2,
    House2,
    House3,
}
impl PieceKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Start => "ViStart",
            Self::Path => "ViSR",
            Self::Torch => "ViL",
            Self::House4Garden => "ViSH",
            Self::Church => "ViST",
            Self::House1 => "ViBH",
            Self::WoodHut => "ViSmH",
            Self::Hall => "ViPH",
            Self::Field1 => "ViDF",
            Self::Field2 => "ViF",
            Self::House2 => "ViS",
            Self::House3 => "ViTRH",
        }
    }
    fn size(self) -> [i32; 3] {
        match self {
            Self::Torch => [3, 4, 2],
            Self::House4Garden => [5, 6, 5],
            Self::Church => [5, 12, 9],
            Self::House1 => [9, 9, 6],
            Self::WoodHut => [4, 6, 5],
            Self::Hall => [9, 7, 11],
            Self::Field1 => [13, 4, 9],
            Self::Field2 => [7, 4, 9],
            Self::House2 => [10, 6, 7],
            Self::House3 => [9, 7, 12],
            _ => panic!("Variable village piece size"),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Piece {
    pub kind: PieceKind,
    pub bounds: BoundingBox,
    pub orientation: u8,
    pub depth: i32,
    pub ground_level: i32,
    pub villagers_spawned: i32,
    pub desert: bool,
    pub crops: [u16; 4],
    pub roof_accessible: bool,
    pub tall_house: bool,
    pub table_position: i32,
    pub chest_made: bool,
    pub path_length: i32,
}
impl Piece {
    fn new(
        kind: PieceKind,
        bounds: BoundingBox,
        orientation: u8,
        depth: i32,
        desert: bool,
    ) -> Self {
        Self {
            kind,
            bounds,
            orientation,
            depth,
            ground_level: -1,
            villagers_spawned: 0,
            desert,
            crops: [0; 4],
            roof_accessible: false,
            tall_house: false,
            table_position: 0,
            chest_made: false,
            path_length: 0,
        }
    }
    pub fn nbt_json(&self) -> serde_json::Value {
        let mut value = serde_json::json!({"id":self.kind.id(),"BB":[self.bounds.min[0],self.bounds.min[1],self.bounds.min[2],self.bounds.max[0],self.bounds.max[1],self.bounds.max[2]],"O":self.orientation,"GD":self.depth,"HPos":self.ground_level,"VCount":self.villagers_spawned,"Desert":self.desert as u8});
        let map = value.as_object_mut().unwrap();
        match self.kind {
            PieceKind::Field1 | PieceKind::Field2 => {
                for (i, key) in ["CA", "CB", "CC", "CD"]
                    .iter()
                    .enumerate()
                    .take(if self.kind == PieceKind::Field1 { 4 } else { 2 })
                {
                    map.insert((*key).into(), self.crops[i].into());
                }
            }
            PieceKind::House4Garden => {
                map.insert("Terrace".into(), (self.roof_accessible as u8).into());
            }
            PieceKind::WoodHut => {
                map.insert("C".into(), (self.tall_house as u8).into());
                map.insert("T".into(), self.table_position.into());
            }
            PieceKind::House2 => {
                map.insert("Chest".into(), (self.chest_made as u8).into());
            }
            PieceKind::Path => {
                map.insert("Length".into(), self.path_length.into());
            }
            _ => {}
        }
        value
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Village {
    pub chunk: [i32; 2],
    pub bounds: BoundingBox,
    pub valid: bool,
    pub pieces: Vec<Piece>,
}
impl Village {
    pub fn candidate(seed: i64, grid_x: i32, grid_z: i32) -> (i32, i32) {
        let salt = (grid_x as i64)
            .wrapping_mul(341873128712)
            .wrapping_add((grid_z as i64).wrapping_mul(132897987541))
            .wrapping_add(seed)
            .wrapping_add(10387312);
        let mut r = JavaRandom::new(salt);
        (
            grid_x.wrapping_mul(32).wrapping_add(r.next_int(24)),
            grid_z.wrapping_mul(32).wrapping_add(r.next_int(24)),
        )
    }
    pub fn can_spawn(seed: i64, x: i32, z: i32, biomes: &BiomeGenerator) -> bool {
        let grid_x = if x < 0 { x.wrapping_sub(31) } else { x } / 32;
        let grid_z = if z < 0 { z.wrapping_sub(31) } else { z } / 32;
        Self::candidate(seed, grid_x, grid_z) == (x, z)
            && biomes.are_biomes_viable(
                x.wrapping_mul(16).wrapping_add(8),
                z.wrapping_mul(16).wrapping_add(8),
                0,
                &[1, 2, 35],
            )
    }
    pub fn layout(seed: i64, x: i32, z: i32, biomes: &BiomeGenerator) -> Self {
        let mut r = JavaRandom::new(seed);
        let a = r.next_long();
        let b = r.next_long();
        r.set_seed((x as i64).wrapping_mul(a) ^ (z as i64).wrapping_mul(b) ^ seed);
        r.next_i32();
        Self::layout_with_random(x, z, biomes, &mut r)
    }
    pub fn layout_with_random(x: i32, z: i32, biomes: &BiomeGenerator, r: &mut JavaRandom) -> Self {
        let mut weights = Vec::new();
        for (kind, weight, min, max) in [
            (PieceKind::House4Garden, 4, 2, 4),
            (PieceKind::Church, 20, 0, 1),
            (PieceKind::House1, 20, 0, 2),
            (PieceKind::WoodHut, 3, 2, 5),
            (PieceKind::Hall, 15, 0, 2),
            (PieceKind::Field1, 3, 1, 4),
            (PieceKind::Field2, 3, 2, 4),
            (PieceKind::House2, 15, 0, 1),
            (PieceKind::House3, 8, 0, 3),
        ] {
            let limit = min + r.next_int(max - min + 1);
            if limit > 0 {
                weights.push(Weight {
                    kind,
                    weight,
                    limit,
                    spawned: 0,
                });
            }
        }
        let orientation = [2, 3, 0, 1][r.next_int(4) as usize];
        let start_x = (x << 4) + 2;
        let start_z = (z << 4) + 2;
        let desert = matches!(biomes.get_biomes(start_x, start_z, 1, 1)[0], 2 | 17);
        let bounds = BoundingBox::new([start_x, 64, start_z], [start_x + 5, 78, start_z + 5]);
        let start = Piece::new(PieceKind::Start, bounds, orientation, 0, desert);
        let mut builder = Builder {
            village: Self {
                chunk: [x, z],
                bounds,
                valid: false,
                pieces: vec![start],
            },
            weights,
            last: None,
            roads: Vec::new(),
            houses: Vec::new(),
            biomes,
        };
        builder.road(
            r,
            [bounds.min[0] - 1, bounds.max[1] - 4, bounds.min[2] + 1],
            1,
            0,
        );
        builder.road(
            r,
            [bounds.max[0] + 1, bounds.max[1] - 4, bounds.min[2] + 1],
            3,
            0,
        );
        builder.road(
            r,
            [bounds.min[0] + 1, bounds.max[1] - 4, bounds.min[2] - 1],
            2,
            0,
        );
        builder.road(
            r,
            [bounds.min[0] + 1, bounds.max[1] - 4, bounds.max[2] + 1],
            0,
            0,
        );
        while !builder.roads.is_empty() || !builder.houses.is_empty() {
            if !builder.roads.is_empty() {
                let i = r.next_int(builder.roads.len() as i32) as usize;
                let id = builder.roads.remove(i);
                builder.build_road(r, id);
            } else {
                let i = r.next_int(builder.houses.len() as i32) as usize;
                builder.houses.remove(i);
            }
        }
        let mut bounds = BoundingBox::empty();
        for p in &builder.village.pieces {
            bounds.expand(&p.bounds);
        }
        builder.village.bounds = bounds;
        builder.village.valid = builder
            .village
            .pieces
            .iter()
            .filter(|p| p.kind != PieceKind::Path)
            .count()
            > 2;
        builder.village
    }
    pub fn nbt_json(&self) -> serde_json::Value {
        serde_json::json!({"id":"Village","ChunkX":self.chunk[0],"ChunkZ":self.chunk[1],"BB":[self.bounds.min[0],self.bounds.min[1],self.bounds.min[2],self.bounds.max[0],self.bounds.max[1],self.bounds.max[2]],"Valid":self.valid as u8,"Children":self.pieces.iter().map(Piece::nbt_json).collect::<Vec<_>>()})
    }
}
struct Weight {
    kind: PieceKind,
    weight: i32,
    limit: i32,
    spawned: i32,
}
struct Builder<'a> {
    village: Village,
    weights: Vec<Weight>,
    last: Option<PieceKind>,
    roads: Vec<usize>,
    houses: Vec<usize>,
    biomes: &'a BiomeGenerator,
}
impl Builder<'_> {
    fn intersects(&self, b: &BoundingBox) -> bool {
        self.village.pieces.iter().any(|p| p.bounds.intersects(b))
    }
    fn in_range(&self, p: [i32; 3]) -> bool {
        (p[0] - self.village.pieces[0].bounds.min[0]).abs() <= 112
            && (p[2] - self.village.pieces[0].bounds.min[2]).abs() <= 112
    }
    fn viable(&self, b: &BoundingBox) -> bool {
        let x = (b.min[0] + b.max[0]) / 2;
        let z = (b.min[2] + b.max[2]) / 2;
        let size = (b.max[0] - b.min[0]).max(b.max[2] - b.min[2]);
        self.biomes
            .are_biomes_viable(x, z, size / 2 + 4, &[1, 2, 35])
    }
    fn create(
        &self,
        kind: PieceKind,
        r: &mut JavaRandom,
        p: [i32; 3],
        facing: u8,
        depth: i32,
    ) -> Option<Piece> {
        let bounds = BoundingBox::oriented(p[0], p[1], p[2], [0; 3], kind.size(), facing);
        if (kind != PieceKind::Torch && kind != PieceKind::House4Garden && bounds.min[1] <= 10)
            || self.intersects(&bounds)
        {
            return None;
        }
        let mut piece = Piece::new(kind, bounds, facing, depth, self.village.pieces[0].desert);
        match kind {
            PieceKind::Field1 | PieceKind::Field2 => {
                for crop in
                    piece
                        .crops
                        .iter_mut()
                        .take(if kind == PieceKind::Field1 { 4 } else { 2 })
                {
                    *crop = match r.next_int(5) {
                        0 => 141,
                        1 => 142,
                        _ => 59,
                    };
                }
            }
            PieceKind::House4Garden => piece.roof_accessible = r.next_boolean(),
            PieceKind::WoodHut => {
                piece.tall_house = r.next_boolean();
                piece.table_position = r.next_int(3);
            }
            _ => {}
        }
        Some(piece)
    }
    fn choose(&mut self, r: &mut JavaRandom, p: [i32; 3], facing: u8, depth: i32) -> Option<Piece> {
        let total: i32 = self.weights.iter().map(|w| w.weight).sum();
        if total <= 0
            || !self
                .weights
                .iter()
                .any(|w| w.limit > 0 && w.spawned < w.limit)
        {
            return None;
        }
        'attempt: for _ in 0..5 {
            let mut choice = r.next_int(total);
            for index in 0..self.weights.len() {
                let weight = &self.weights[index];
                choice -= weight.weight;
                if choice >= 0 {
                    continue;
                }
                if weight.spawned >= weight.limit
                    || (self.last == Some(weight.kind) && self.weights.len() > 1)
                {
                    continue 'attempt;
                }
                if let Some(piece) = self.create(weight.kind, r, p, facing, depth) {
                    let weight = &mut self.weights[index];
                    weight.spawned += 1;
                    self.last = Some(weight.kind);
                    if weight.spawned >= weight.limit {
                        self.weights.remove(index);
                    }
                    return Some(piece);
                }
            }
        }
        self.create(PieceKind::Torch, r, p, facing, depth)
    }
    fn house(&mut self, r: &mut JavaRandom, p: [i32; 3], facing: u8, depth: i32) -> Option<usize> {
        if depth > 50 || !self.in_range(p) {
            return None;
        }
        let piece = self.choose(r, p, facing, depth + 1)?;
        if !self.viable(&piece.bounds) {
            return None;
        }
        let index = self.village.pieces.len();
        self.village.pieces.push(piece);
        self.houses.push(index);
        Some(index)
    }
    fn road(&mut self, r: &mut JavaRandom, p: [i32; 3], facing: u8, depth: i32) -> Option<usize> {
        if depth > 3 || !self.in_range(p) {
            return None;
        }
        let mut length = 7 * (3 + r.next_int(3));
        let bounds = loop {
            if length < 7 {
                return None;
            }
            let b = BoundingBox::oriented(p[0], p[1], p[2], [0; 3], [3, 3, length], facing);
            if !self.intersects(&b) {
                break b;
            }
            length -= 7;
        };
        if bounds.min[1] <= 10 {
            return None;
        }
        let mut piece = Piece::new(
            PieceKind::Path,
            bounds,
            facing,
            depth,
            self.village.pieces[0].desert,
        );
        piece.path_length = bounds.size()[0].max(bounds.size()[2]);
        if !self.viable(&bounds) {
            return None;
        }
        let index = self.village.pieces.len();
        self.village.pieces.push(piece);
        self.roads.push(index);
        Some(index)
    }
    fn build_road(&mut self, r: &mut JavaRandom, index: usize) {
        let piece = self.village.pieces[index].clone();
        let b = piece.bounds;
        let facing = piece.orientation;
        let mut placed = false;
        for side in 0..2 {
            let mut offset = r.next_int(5);
            while offset < piece.path_length - 8 {
                let (p, direction) = match (side, facing) {
                    (0, 0 | 2) => ([b.min[0] - 1, b.min[1], b.min[2] + offset], 1),
                    (1, 0 | 2) => ([b.max[0] + 1, b.min[1], b.min[2] + offset], 3),
                    (0, _) => ([b.min[0] + offset, b.min[1], b.min[2] - 1], 2),
                    _ => ([b.min[0] + offset, b.min[1], b.max[2] + 1], 0),
                };
                if let Some(id) = self.house(r, p, direction, piece.depth) {
                    let size = self.village.pieces[id].bounds.size();
                    offset += size[0].max(size[2]);
                    placed = true;
                }
                offset += 2 + r.next_int(5);
            }
        }
        if placed && r.next_int(3) > 0 {
            let (p, direction) = match facing {
                2 => ([b.min[0] - 1, b.min[1], b.min[2]], 1),
                0 => ([b.min[0] - 1, b.min[1], b.max[2] - 2], 1),
                3 => ([b.max[0] - 2, b.min[1], b.min[2] - 1], 2),
                _ => ([b.min[0], b.min[1], b.min[2] - 1], 2),
            };
            self.road(r, p, direction, piece.depth);
        }
        if placed && r.next_int(3) > 0 {
            let (p, direction) = match facing {
                2 => ([b.max[0] + 1, b.min[1], b.min[2]], 3),
                0 => ([b.max[0] + 1, b.min[1], b.max[2] - 2], 3),
                3 => ([b.max[0] - 2, b.min[1], b.max[2] + 1], 0),
                _ => ([b.min[0], b.min[1], b.max[2] + 1], 0),
            };
            self.road(r, p, direction, piece.depth);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_village_layout_nbt_and_rng_match_exactly() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-villages.json"
        ))
        .unwrap();
        let mut pieces = 0;
        for row in corpus["cases"].as_array().unwrap() {
            let seed: i64 = row["seed"].as_str().unwrap().parse().unwrap();
            let x = row["x"].as_i64().unwrap() as i32;
            let z = row["z"].as_i64().unwrap() as i32;
            let biomes = BiomeGenerator::new(seed);
            assert_eq!(
                Village::can_spawn(seed, x, z, &biomes),
                row["candidate"].as_bool().unwrap()
            );
            let mut random = JavaRandom::new(seed);
            let a = random.next_long();
            let b = random.next_long();
            random.set_seed((x as i64).wrapping_mul(a) ^ (z as i64).wrapping_mul(b) ^ seed);
            random.next_i32();
            let village = Village::layout_with_random(x, z, &biomes, &mut random);
            assert_eq!(
                village.nbt_json(),
                row["nbt"],
                "seed={seed} village=({x},{z})"
            );
            assert_eq!(
                random.next_long().to_string(),
                row["next_long"].as_str().unwrap(),
                "RNG continuation seed={seed} chunk=({x},{z})"
            );
            pieces += village.pieces.len();
        }
        assert_eq!(pieces, 2271);
    }
}
