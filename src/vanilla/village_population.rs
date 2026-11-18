//! Native block population for every original default village piece.
use super::*;
use crate::vanilla::{
    loot::{fill_loot, LootEntry, LootStack},
    structure_common::rotate_metadata,
};
#[path = "block_properties.rs"]
mod properties;
#[path = "village_templates.rs"]
mod templates;

/// Accessors deliberately expose tile/entity creation instead of dropping those
/// side effects when writing a structure into a block-only region.
pub trait VillageWorld {
    fn get(&self, pos: [i32; 3]) -> u16;
    fn set(&mut self, pos: [i32; 3], state: u16);
    fn chest(&mut self, pos: [i32; 3], slots: Vec<Option<LootStack>>);
    fn villager(&mut self, pos: [i32; 3], profession: Option<u8>);
    fn top_solid(&self, x: i32, z: i32) -> i32 {
        for y in (0..=256).rev() {
            let flags = properties::FLAGS[(self.get([x, y - 1, z]) >> 4) as usize];
            if flags & 4 != 0 && flags & 8 == 0 {
                return y;
            }
        }
        -1
    }
    fn full_block(&self, pos: [i32; 3]) -> bool {
        properties::FLAGS[(self.get(pos) >> 4) as usize] & 1 != 0
    }
    fn normal_cube(&self, pos: [i32; 3]) -> bool {
        properties::FLAGS[(self.get(pos) >> 4) as usize] & 2 != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    struct TestWorld {
        pattern: i32,
        blocks: BTreeMap<[i32; 3], u16>,
        chests: BTreeMap<[i32; 3], Vec<Option<LootStack>>>,
        villagers: Vec<([i32; 3], Option<u8>)>,
    }
    impl VillageWorld for TestWorld {
        fn get(&self, p: [i32; 3]) -> u16 {
            if let Some(s) = self.blocks.get(&p) {
                return *s;
            }
            let [x, y, z] = p;
            let h = if self.pattern == 0 {
                63
            } else {
                60 + (x * 3 + z * 7).rem_euclid(9)
            };
            (if !(0..256).contains(&y) {
                0
            } else if y == 0 {
                7
            } else if y < h - 3 {
                1
            } else if y < h {
                3
            } else if y == h {
                2
            } else if self.pattern == 2 && y < 66 {
                9
            } else {
                0
            }) << 4
        }
        fn set(&mut self, p: [i32; 3], state: u16) {
            if !(0..256).contains(&p[1]) {
                return;
            }
            self.blocks.insert(p, state);
            if state >> 4 != 54 {
                self.chests.remove(&p);
            }
        }
        fn chest(&mut self, p: [i32; 3], slots: Vec<Option<LootStack>>) {
            self.chests.insert(p, slots);
        }
        fn villager(&mut self, p: [i32; 3], profession: Option<u8>) {
            self.villagers.push((p, profession));
        }
    }
    fn n(v: &Value, key: &str) -> i32 {
        v[key].as_i64().unwrap_or(0) as i32
    }
    fn piece(v: &Value) -> Piece {
        let kind = [
            PieceKind::Start,
            PieceKind::Path,
            PieceKind::Torch,
            PieceKind::House4Garden,
            PieceKind::Church,
            PieceKind::House1,
            PieceKind::WoodHut,
            PieceKind::Hall,
            PieceKind::Field1,
            PieceKind::Field2,
            PieceKind::House2,
            PieceKind::House3,
        ]
        .into_iter()
        .find(|k| k.id() == v["id"].as_str().unwrap())
        .unwrap();
        let bb = v["BB"].as_array().unwrap();
        let mut p = Piece::new(
            kind,
            BoundingBox::new(
                std::array::from_fn(|i| bb[i].as_i64().unwrap() as i32),
                std::array::from_fn(|i| bb[i + 3].as_i64().unwrap() as i32),
            ),
            n(v, "O") as u8,
            n(v, "GD"),
            n(v, "Desert") != 0,
        );
        p.ground_level = n(v, "HPos");
        p.villagers_spawned = n(v, "VCount");
        p.crops = [
            n(v, "CA") as u16,
            n(v, "CB") as u16,
            n(v, "CC") as u16,
            n(v, "CD") as u16,
        ];
        p.roof_accessible = n(v, "Terrace") != 0;
        p.tall_house = n(v, "C") != 0;
        p.table_position = n(v, "T");
        p.path_length = n(v, "Length");
        p.chest_made = n(v, "Chest") != 0;
        p
    }
    #[test]
    fn original_village_population_blocks_chests_and_rng() {
        let corpus: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-village-population.json"
        ))
        .unwrap();
        let mut count = 0;
        for (case, row) in corpus["cases"].as_array().unwrap().iter().enumerate() {
            let mut p = piece(&row["before"]);
            let mut world = TestWorld {
                pattern: n(row, "pattern"),
                blocks: BTreeMap::new(),
                chests: BTreeMap::new(),
                villagers: Vec::new(),
            };
            let mut random = JavaRandom::new(row["seed"].as_str().unwrap().parse().unwrap());
            for clip in row["clips"].as_array().unwrap() {
                p.populate(
                    &mut world,
                    BoundingBox::new(
                        std::array::from_fn(|i| clip[i].as_i64().unwrap() as i32),
                        std::array::from_fn(|i| clip[i + 3].as_i64().unwrap() as i32),
                    ),
                    &mut random,
                );
            }
            assert_eq!(p.nbt_json(), row["after"], "piece metadata case {case}");
            assert_eq!(
                random.next_long().to_string(),
                row["next_long"].as_str().unwrap(),
                "RNG case {case}"
            );
            let expected: BTreeMap<[i32; 3], u16> = row["blocks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| {
                    (
                        [
                            b[0].as_i64().unwrap() as i32,
                            b[1].as_i64().unwrap() as i32,
                            b[2].as_i64().unwrap() as i32,
                        ],
                        b[3].as_u64().unwrap() as u16,
                    )
                })
                .collect();
            for (pos, state) in &expected {
                assert_eq!(
                    world.blocks.get(pos),
                    Some(state),
                    "block case {case} {:?} at {pos:?}",
                    p.kind
                );
            }
            assert_eq!(
                world.blocks.len(),
                expected.len(),
                "block write keys case {case}"
            );
            assert_eq!(
                world.villagers.len(),
                row["villagers"].as_array().unwrap().len(),
                "villager count case {case}"
            );
            for ((pos, profession), expected) in world
                .villagers
                .iter()
                .zip(row["villagers"].as_array().unwrap())
            {
                assert_eq!(
                    json!(pos),
                    json!([expected[0], expected[1], expected[2]]),
                    "villager position case {case}"
                );
                if let Some(profession) = profession {
                    assert_eq!(
                        *profession as i64,
                        expected[3].as_i64().unwrap(),
                        "fixed profession case {case}"
                    );
                }
            }
            let chests:Vec<Value>=world.chests.iter().map(|(pos,slots)|json!({"pos":pos,"items":slots.iter().enumerate().filter_map(|(slot,s)|s.as_ref().map(|s|vec![slot as u16,s.id,s.meta,s.count as u16])).collect::<Vec<_>>()})).collect();
            assert_eq!(json!(chests), row["chests"], "blacksmith loot case {case}");
            count += expected.len();
        }
        println!("288 original piece population cases / {count} states matched, persistent NBT, crop/chest RNG, loot, villager positions/fixed professions");
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VillagerSpawn {
    pub position: [i32; 3],
    /// None leaves the profession selected by onInitialSpawn's World.rand
    /// roll; even a fixed override still consumes that roll first. The native
    /// eventful adapter resolves this field to Some before emitting the spawn.
    pub profession: Option<u8>,
}
pub struct VillageWorldAdapter<'a> {
    pub world: &'a mut crate::world::World,
    /// Caller persists/spawns these through its authoritative entity registry.
    pub villagers: &'a mut Vec<VillagerSpawn>,
}
impl VillageWorld for VillageWorldAdapter<'_> {
    fn get(&self, pos: [i32; 3]) -> u16 {
        self.world.get(glam::IVec3::from_array(pos))
    }
    fn set(&mut self, pos: [i32; 3], state: u16) {
        self.world.set(glam::IVec3::from_array(pos), state);
    }
    fn chest(&mut self, pos: [i32; 3], slots: Vec<Option<LootStack>>) {
        if let Some(chunk) = self
            .world
            .chunks
            .get_mut(&(pos[0].div_euclid(16), pos[2].div_euclid(16)))
        {
            let chunk = std::sync::Arc::make_mut(chunk);
            let at = glam::IVec3::from_array(pos);
            let mut chest = crate::container::Container::new(crate::container::Kind::Chest);
            for (i, s) in slots.into_iter().enumerate() {
                if let Some(s) = s {
                    chest.slots[i] = Some(crate::inventory::Stack {
                        nbt: None,
                        identity: crate::inventory::Stack::fresh_identity(),
                        id: s.id,
                        count: s.count,
                        damage: 0,
                        meta: s.meta,
                    });
                }
            }
            chunk.containers.retain(|(p, _)| *p != at);
            chunk.containers.push((at, chest));
        }
    }
    fn villager(&mut self, pos: [i32; 3], profession: Option<u8>) {
        self.villagers.push(VillagerSpawn {
            position: pos,
            profession,
        });
    }
}
impl Village {
    /// Original MapGenStructure.generateStructure return flag. Persist `self`
    /// afterwards: first intersected clip determines each piece's ground level.
    pub fn populate(
        &mut self,
        world: &mut impl VillageWorld,
        chunk_x: i32,
        chunk_z: i32,
        random: &mut JavaRandom,
    ) -> bool {
        let x = chunk_x.wrapping_mul(16).wrapping_add(8);
        let z = chunk_z.wrapping_mul(16).wrapping_add(8);
        let clip = BoundingBox::new([x, 0, z], [x + 15, 255, z + 15]);
        if !self.valid || !self.bounds.intersects_xz(x, z, x + 15, z + 15) {
            return false;
        }
        for piece in &mut self.pieces {
            if piece.bounds.intersects(&clip) {
                piece.populate(world, clip, random);
            }
        }
        true
    }
}
fn torch(facing: u8) -> u16 {
    50 << 4 | [3, 2, 4, 1][facing as usize]
}
fn offset(mut p: [i32; 3], facing: u8) -> [i32; 3] {
    let [dx, dz] = [[0, 1], [-1, 0], [0, -1], [1, 0]][facing as usize];
    p[0] += dx;
    p[2] += dz;
    p
}
fn up(mut p: [i32; 3]) -> [i32; 3] {
    p[1] += 1;
    p
}
impl Piece {
    pub fn populate(
        &mut self,
        world: &mut impl VillageWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        if self.kind == PieceKind::Path {
            for x in self.bounds.min[0]..=self.bounds.max[0] {
                for z in self.bounds.min[2]..=self.bounds.max[2] {
                    if clip.contains([x, 64, z]) {
                        let y = world.top_solid(x, z) - 1;
                        world.set([x, y, z], self.convert(13 << 4));
                        world.set([x, y - 1, z], self.convert(4 << 4));
                    }
                }
            }
            return;
        }
        if self.ground_level < 0 {
            let mut sum = 0;
            let mut count = 0;
            for z in self.bounds.min[2]..=self.bounds.max[2] {
                for x in self.bounds.min[0]..=self.bounds.max[0] {
                    if clip.contains([x, 64, z]) {
                        sum += world.top_solid(x, z).max(64);
                        count += 1;
                    }
                }
            }
            if count == 0 {
                return;
            }
            self.ground_level = sum / count;
            let h = if self.kind == PieceKind::Start {
                3
            } else {
                self.kind.size()[1] - 1
            };
            self.bounds
                .offset(0, self.ground_level - self.bounds.max[1] + h, 0);
        }
        match self.kind {
            PieceKind::Start => self.template_well(world, clip, random),
            PieceKind::Torch => self.template_torch(world, clip, random),
            PieceKind::Field1 => self.template_field1(world, clip, random),
            PieceKind::Field2 => self.template_field2(world, clip, random),
            PieceKind::House1 => self.template_house1(world, clip, random),
            PieceKind::House2 => self.template_house2(world, clip, random),
            PieceKind::House3 => self.template_house3(world, clip, random),
            PieceKind::House4Garden => self.template_house4garden(world, clip, random),
            PieceKind::Hall => self.template_hall(world, clip, random),
            PieceKind::Church => self.template_church(world, clip, random),
            PieceKind::WoodHut => self.template_woodhut(world, clip, random),
            PieceKind::Path => unreachable!(),
        }
    }
    fn position(&self, x: i32, y: i32, z: i32) -> [i32; 3] {
        self.bounds.local_to_world(x, y, z, self.orientation)
    }
    fn convert(&self, state: u16) -> u16 {
        if !self.desert {
            return state;
        }
        match state >> 4 {
            17 | 162 | 4 | 13 => 24 << 4,
            5 => 24 << 4 | 2,
            53 | 67 => 128 << 4 | (state & 3),
            _ => state,
        }
    }
    fn get(&self, world: &impl VillageWorld, x: i32, y: i32, z: i32, clip: BoundingBox) -> u16 {
        let p = self.position(x, y, z);
        if clip.contains(p) {
            world.get(p)
        } else {
            0
        }
    }
    fn place(
        &self,
        world: &mut impl VillageWorld,
        state: u16,
        x: i32,
        y: i32,
        z: i32,
        clip: BoundingBox,
    ) {
        let p = self.position(x, y, z);
        if clip.contains(p) {
            world.set(p, self.convert(state));
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn fill(
        &self,
        world: &mut impl VillageWorld,
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
    fn clear(&self, world: &mut impl VillageWorld, x: i32, y: i32, z: i32, clip: BoundingBox) {
        let mut p = self.position(x, y, z);
        if !clip.contains(p) {
            return;
        }
        while world.get(p) >> 4 != 0 && p[1] < 255 {
            world.set(p, 0);
            p[1] += 1;
        }
    }
    fn down(
        &self,
        world: &mut impl VillageWorld,
        state: u16,
        x: i32,
        y: i32,
        z: i32,
        clip: BoundingBox,
    ) {
        let mut p = self.position(x, y, z);
        if !clip.contains(p) {
            return;
        }
        while matches!(world.get(p) >> 4, 0 | 8..=11) && p[1] > 1 {
            world.set(p, self.convert(state));
            p[1] -= 1;
        }
    }
    fn door(
        &self,
        world: &mut impl VillageWorld,
        clip: BoundingBox,
        x: i32,
        y: i32,
        z: i32,
        facing: u8,
    ) {
        let p = self.position(x, y, z);
        if !clip.contains(p) {
            return;
        }
        let facing = (facing + 3) & 3;
        let right = offset(p, (facing + 1) & 3);
        let left = offset(p, (facing + 3) & 3);
        let n = world.normal_cube(left) as u8 + world.normal_cube(up(left)) as u8;
        let n2 = world.normal_cube(right) as u8 + world.normal_cube(up(right)) as u8;
        let left_door = world.get(left) >> 4 == 64 || world.get(up(left)) >> 4 == 64;
        let right_door = world.get(right) >> 4 == 64 || world.get(up(right)) >> 4 == 64;
        let hinge = left_door && !right_door || n2 > n;
        world.set(p, 64 << 4 | ((facing + 1) & 3) as u16);
        world.set(up(p), 64 << 4 | 8 | hinge as u16);
    }
    fn spawn(
        &mut self,
        world: &mut impl VillageWorld,
        clip: BoundingBox,
        x: i32,
        y: i32,
        z: i32,
        count: i32,
    ) {
        for i in self.villagers_spawned..count {
            let p = self.position(x + i, y, z);
            if !clip.contains(p) {
                break;
            }
            self.villagers_spawned += 1;
            let profession = match self.kind {
                PieceKind::House1 => Some(1),
                PieceKind::Church => Some(2),
                PieceKind::House2 => Some(3),
                PieceKind::Hall if i == 0 => Some(4),
                _ => None,
            };
            world.villager(p, profession);
        }
    }
    fn blacksmith_chest(
        &mut self,
        world: &mut impl VillageWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        let p = self.position(5, 1, 5);
        if self.chest_made || !clip.contains(p) {
            return;
        }
        self.chest_made = true;
        let rolls = 3 + random.next_int(6);
        if world.get(p) >> 4 == 54 {
            return;
        }
        let mut adjacent = None;
        for dir in [2, 3, 0, 1] {
            let q = offset(p, dir);
            if world.get(q) >> 4 == 54 {
                adjacent = None;
                break;
            }
            if world.full_block(q) {
                if adjacent.is_some() {
                    adjacent = None;
                    break;
                }
                adjacent = Some(dir);
            }
        }
        let mut facing = 2;
        if let Some(dir) = adjacent {
            facing = (dir + 2) & 3;
        } else {
            if world.full_block(offset(p, facing)) {
                facing = (facing + 2) & 3;
            }
            if world.full_block(offset(p, facing)) {
                facing = (facing + 1) & 3;
            }
            if world.full_block(offset(p, facing)) {
                facing = (facing + 2) & 3;
            }
        }
        world.set(p, 54 << 4 | [3, 4, 2, 5][facing as usize]);
        let entries = [
            (264, 1, 3, 3),
            (265, 1, 5, 10),
            (266, 1, 3, 5),
            (297, 1, 3, 15),
            (260, 1, 3, 15),
            (257, 1, 1, 5),
            (267, 1, 1, 5),
            (307, 1, 1, 5),
            (306, 1, 1, 5),
            (308, 1, 1, 5),
            (309, 1, 1, 5),
            (49, 3, 7, 5),
            (6, 3, 7, 5),
            (329, 1, 1, 3),
            (417, 1, 1, 1),
            (418, 1, 1, 1),
            (419, 1, 1, 1),
        ]
        .map(|(id, min, max, weight)| LootEntry::new(id, 0, min, max, weight));
        let slots = fill_loot(random, &entries, rolls, 27);
        world.chest(p, slots);
    }
}
