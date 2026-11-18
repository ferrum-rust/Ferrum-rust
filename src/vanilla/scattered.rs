//! Native 1.8.9 desert/jungle pyramids and swamp huts. Structure state and
//! generated tile/entity metadata are retained independently of simulation.
use super::{
    loot::{fill_loot, LootEntry, LootStack},
    material,
    random::JavaRandom,
    structure_common::{rotate_metadata, BoundingBox},
};
use serde::{Deserialize, Serialize};
#[path = "scattered_templates.rs"]
mod templates;
#[cfg(test)]
#[path = "scattered_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeatureKind {
    DesertPyramid,
    JunglePyramid,
    SwampHut,
}
impl FeatureKind {
    fn size(self) -> [i32; 3] {
        match self {
            Self::DesertPyramid => [21, 15, 21],
            Self::JunglePyramid => [12, 10, 15],
            Self::SwampHut => [7, 7, 9],
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::DesertPyramid => "TeDP",
            Self::JunglePyramid => "TeJP",
            Self::SwampHut => "TeSH",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Piece {
    pub kind: FeatureKind,
    pub bounds: BoundingBox,
    pub orientation: u8,
    pub size: [i32; 3],
    pub ground_level: i32,
    /// Desert uses horizontal-index chest flags; jungle uses main/hidden/trap1/trap2.
    pub flags: [bool; 4],
    pub witch_spawned: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scattered {
    pub chunk: [i32; 2],
    /// Original StructureStart bounds do not move when its component aligns.
    pub bounds: BoundingBox,
    pub piece: Option<Piece>,
}
pub fn lattice_candidate(seed: i64, cx: i32, cz: i32) -> bool {
    let rx = if cx < 0 { cx.wrapping_sub(31) } else { cx } / 32;
    let rz = if cz < 0 { cz.wrapping_sub(31) } else { cz } / 32;
    let mut random = JavaRandom::new(
        (rx as i64)
            .wrapping_mul(341873128712)
            .wrapping_add((rz as i64).wrapping_mul(132897987541))
            .wrapping_add(seed)
            .wrapping_add(14357617),
    );
    cx == rx.wrapping_mul(32).wrapping_add(random.next_int(24))
        && cz == rz.wrapping_mul(32).wrapping_add(random.next_int(24))
}
/// `biome_id` is original biome lookup at (cx*16+8,0,cz*16+8); mutations are
/// deliberately excluded, because the original uses biome instance identity.
pub fn candidate(seed: i64, cx: i32, cz: i32, biome_id: u8) -> Option<Scattered> {
    if !lattice_candidate(seed, cx, cz) {
        return None;
    }
    let kind = match biome_id {
        2 | 17 => FeatureKind::DesertPyramid,
        21 | 22 => FeatureKind::JunglePyramid,
        6 => FeatureKind::SwampHut,
        _ => return None,
    };
    let mut random = JavaRandom::new(seed);
    let a = random.next_long();
    let b = random.next_long();
    random.set_seed((cx as i64).wrapping_mul(a) ^ (cz as i64).wrapping_mul(b) ^ seed);
    random.next_i32();
    let piece = Piece::new(kind, &mut random, cx.wrapping_mul(16), cz.wrapping_mul(16));
    Some(Scattered {
        chunk: [cx, cz],
        bounds: piece.bounds,
        piece: Some(piece),
    })
}
pub trait ScatteredWorld {
    fn get(&self, p: [i32; 3]) -> u16;
    fn set(&mut self, p: [i32; 3], state: u16);
    fn container(&mut self, p: [i32; 3], block: u16, slots: Vec<Option<LootStack>>);
    fn witch(&mut self, p: [i32; 3]);
    fn top_solid(&self, x: i32, z: i32) -> i32 {
        for y in (0..=256).rev() {
            let state = self.get([x, y - 1, z]);
            if material::blocks_movement(state) && !material::is_leaves(state) {
                return y;
            }
        }
        -1
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Population {
    pub containers: Vec<GeneratedContainer>,
    pub witches: Vec<[i32; 3]>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeneratedContainer {
    pub position: [i32; 3],
    pub block: u16,
    pub slots: Vec<Option<LootStack>>,
}
pub struct WorldAdapter<'a> {
    pub world: &'a mut crate::world::World,
    pub output: &'a mut Population,
}
impl ScatteredWorld for WorldAdapter<'_> {
    fn get(&self, p: [i32; 3]) -> u16 {
        self.world.get(glam::IVec3::from_array(p))
    }
    fn set(&mut self, p: [i32; 3], state: u16) {
        self.world.set(glam::IVec3::from_array(p), state);
    }
    fn container(&mut self, p: [i32; 3], block: u16, slots: Vec<Option<LootStack>>) {
        self.output.containers.push(GeneratedContainer {
            position: p,
            block,
            slots,
        });
    }
    fn witch(&mut self, p: [i32; 3]) {
        self.output.witches.push(p);
    }
}
impl Scattered {
    pub fn populate(
        &mut self,
        world: &mut impl ScatteredWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) -> bool {
        if !self
            .bounds
            .intersects_xz(clip.min[0], clip.min[2], clip.max[0], clip.max[2])
        {
            return false;
        }
        if let Some(piece) = &mut self.piece {
            if piece.bounds.intersects(&clip) && !piece.populate(world, clip, random) {
                self.piece = None;
            }
        }
        true
    }
}
impl Piece {
    pub fn new(kind: FeatureKind, random: &mut JavaRandom, x: i32, z: i32) -> Self {
        let size = kind.size();
        let orientation = [2, 3, 0, 1][random.next_int(4) as usize];
        let [width, depth] = if orientation & 1 == 0 {
            [size[0], size[2]]
        } else {
            [size[2], size[0]]
        };
        Self {
            kind,
            bounds: BoundingBox::new([x, 64, z], [x + width - 1, 64 + size[1] - 1, z + depth - 1]),
            orientation,
            size,
            ground_level: -1,
            flags: [false; 4],
            witch_spawned: false,
        }
    }
    pub fn populate(
        &mut self,
        world: &mut impl ScatteredWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) -> bool {
        if self.kind != FeatureKind::DesertPyramid && self.ground_level < 0 {
            let (mut sum, mut count) = (0, 0);
            for z in self.bounds.min[2]..=self.bounds.max[2] {
                for x in self.bounds.min[0]..=self.bounds.max[0] {
                    if clip.contains([x, 64, z]) {
                        sum += world.top_solid(x, z).max(64);
                        count += 1;
                    }
                }
            }
            if count == 0 {
                return false;
            }
            self.ground_level = sum / count;
            self.bounds
                .offset(0, self.ground_level - self.bounds.min[1], 0);
        }
        match self.kind {
            FeatureKind::DesertPyramid => self.template_desertpyramid(world, clip, random),
            FeatureKind::JunglePyramid => self.template_junglepyramid(world, clip, random),
            FeatureKind::SwampHut => self.template_swamphut(world, clip, random),
        }
        true
    }
    fn point(&self, x: i32, y: i32, z: i32) -> [i32; 3] {
        self.bounds.local_to_world(x, y, z, self.orientation)
    }
    fn get(&self, world: &impl ScatteredWorld, x: i32, y: i32, z: i32, clip: BoundingBox) -> u16 {
        let p = self.point(x, y, z);
        if clip.contains(p) {
            world.get(p)
        } else {
            0
        }
    }
    fn place(
        &self,
        world: &mut impl ScatteredWorld,
        state: u16,
        x: i32,
        y: i32,
        z: i32,
        clip: BoundingBox,
    ) {
        let p = self.point(x, y, z);
        if clip.contains(p) {
            world.set(p, state);
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn fill(
        &self,
        world: &mut impl ScatteredWorld,
        clip: BoundingBox,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        edge: u16,
        inside: u16,
        existing: bool,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if existing && self.get(world, x, y, z, clip) >> 4 == 0 {
                        continue;
                    }
                    self.place(
                        world,
                        if x == x0 || x == x1 || y == y0 || y == y1 || z == z0 || z == z1 {
                            edge
                        } else {
                            inside
                        },
                        x,
                        y,
                        z,
                        clip,
                    );
                }
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn air(
        &self,
        world: &mut impl ScatteredWorld,
        clip: BoundingBox,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
    ) {
        self.fill(world, clip, x0, y0, z0, x1, y1, z1, 0, 0, false);
    }
    #[allow(clippy::too_many_arguments)]
    fn stones(
        &self,
        world: &mut impl ScatteredWorld,
        clip: BoundingBox,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        existing: bool,
        random: &mut JavaRandom,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if existing && self.get(world, x, y, z, clip) >> 4 == 0 {
                        continue;
                    }
                    let state = if random.next_float() < 0.4 {
                        4 << 4
                    } else {
                        48 << 4
                    };
                    self.place(world, state, x, y, z, clip);
                }
            }
        }
    }
    fn down(
        &self,
        world: &mut impl ScatteredWorld,
        state: u16,
        x: i32,
        y: i32,
        z: i32,
        clip: BoundingBox,
    ) {
        let mut p = self.point(x, y, z);
        if !clip.contains(p) {
            return;
        }
        while matches!(world.get(p) >> 4, 0 | 8..=11) && p[1] > 1 {
            world.set(p, state);
            p[1] -= 1;
        }
    }
    fn spawn_witch(&mut self, world: &mut impl ScatteredWorld, clip: BoundingBox) {
        let p = self.point(2, 2, 5);
        if !self.witch_spawned && clip.contains(p) {
            self.witch_spawned = true;
            world.witch(p);
        }
    }
    fn temple_chest(
        &mut self,
        world: &mut impl ScatteredWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
        index: usize,
        pos: [i32; 3],
    ) {
        if self.flags[index] {
            return;
        }
        let mut entries: Vec<_> = [
            (264, 1, 3, 3),
            (265, 1, 5, 10),
            (266, 2, 7, 15),
            (388, 1, 3, 2),
            (352, 4, 6, 20),
            (367, 3, 7, 16),
            (329, 1, 1, 3),
            (417, 1, 1, 1),
            (418, 1, 1, 1),
            (419, 1, 1, 1),
        ]
        .into_iter()
        .map(|(id, min, max, weight)| LootEntry::new(id, 0, min, max, weight))
        .collect();
        entries.push(LootEntry::book(random));
        let rolls = 2 + random.next_int(5);
        let p = self.point(pos[0], pos[1], pos[2]);
        if !clip.contains(p) || world.get(p) >> 4 == 54 {
            return;
        }
        let facing = chest_facing(world, p);
        world.set(p, 54 << 4 | facing as u16);
        world.container(p, 54, fill_loot(random, &entries, rolls, 27));
        self.flags[index] = true;
    }
    fn trap(
        &mut self,
        world: &mut impl ScatteredWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
        index: usize,
        pos: [i32; 3],
        facing: u8,
    ) {
        if self.flags[index] {
            return;
        }
        let p = self.point(pos[0], pos[1], pos[2]);
        if !clip.contains(p) || world.get(p) >> 4 == 23 {
            return;
        }
        world.set(
            p,
            23 << 4 | rotate_metadata(23, facing, self.orientation) as u16,
        );
        world.container(
            p,
            23,
            fill_loot(random, &[LootEntry::new(262, 0, 2, 7, 30)], 2, 9),
        );
        self.flags[index] = true;
    }
}
fn lever(facing: u8) -> i32 {
    [0, 5, 4, 3, 2, 1][facing as usize]
}
fn chest_facing(world: &impl ScatteredWorld, p: [i32; 3]) -> u8 {
    let offset = |facing: u8| {
        let [dx, dz] = [[0, 1], [-1, 0], [0, -1], [1, 0]][facing as usize];
        [p[0] + dx, p[1], p[2] + dz]
    };
    let mut adjacent = None;
    for direction in [2, 3, 0, 1] {
        let q = offset(direction);
        if world.get(q) >> 4 == 54 {
            return 2;
        }
        if material::full_block(world.get(q)) {
            if adjacent.is_some() {
                adjacent = None;
                break;
            }
            adjacent = Some(direction);
        }
    }
    let mut facing = 2;
    if let Some(direction) = adjacent {
        facing = (direction + 2) & 3;
    } else {
        if material::full_block(world.get(offset(facing))) {
            facing = (facing + 2) & 3;
        }
        if material::full_block(world.get(offset(facing))) {
            facing = (facing + 1) & 3;
        }
        if material::full_block(world.get(offset(facing))) {
            facing = (facing + 2) & 3;
        }
    }
    [3, 4, 2, 5][facing as usize]
}
