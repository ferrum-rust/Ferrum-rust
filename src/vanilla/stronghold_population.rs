//! Stronghold component population; metadata remains available to world storage.
#![allow(clippy::too_many_arguments)]
use super::*;
pub trait StrongholdWorld {
    fn get(&self, p: [i32; 3]) -> u16;
    fn set(&mut self, p: [i32; 3], state: u16);
    fn chest(&mut self, p: [i32; 3], slots: Vec<Option<LootStack>>);
    fn spawner(&mut self, p: [i32; 3]);
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Population {
    pub chests: Vec<super::super::scattered::GeneratedContainer>,
    pub spawners: Vec<[i32; 3]>,
}
pub struct WorldAdapter<'a> {
    pub world: &'a mut crate::world::World,
    pub output: &'a mut Population,
}
impl StrongholdWorld for WorldAdapter<'_> {
    fn get(&self, p: [i32; 3]) -> u16 {
        self.world.get(glam::IVec3::from_array(p))
    }
    fn set(&mut self, p: [i32; 3], state: u16) {
        self.world.set(glam::IVec3::from_array(p), state);
    }
    fn chest(&mut self, p: [i32; 3], slots: Vec<Option<LootStack>>) {
        self.output
            .chests
            .push(super::super::scattered::GeneratedContainer {
                position: p,
                block: 54,
                slots,
            });
    }
    fn spawner(&mut self, p: [i32; 3]) {
        self.output.spawners.push(p);
    }
}
impl Stronghold {
    pub fn populate(
        &mut self,
        world: &mut impl StrongholdWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) -> bool {
        if !self
            .bounds
            .intersects_xz(clip.min[0], clip.min[2], clip.max[0], clip.max[2])
        {
            return false;
        }
        let mut i = 0;
        while i < self.pieces.len() {
            if self.pieces[i].bounds.intersects(&clip)
                && !self.pieces[i].populate(world, clip, random)
            {
                self.pieces.remove(i);
                if i < self.portal {
                    self.portal -= 1;
                }
            } else {
                i += 1;
            }
        }
        true
    }
}
impl Piece {
    pub fn populate(
        &mut self,
        world: &mut impl StrongholdWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) -> bool {
        if self.kind != Kind::PortalRoom && self.liquid(world, clip) {
            return false;
        }
        match self.kind {
            Kind::Straight => self.template_straight(world, clip, random),
            Kind::Prison => self.template_prison(world, clip, random),
            Kind::LeftTurn => self.template_leftturn(world, clip, random),
            Kind::RightTurn => self.template_rightturn(world, clip, random),
            Kind::RoomCrossing => self.template_roomcrossing(world, clip, random),
            Kind::StairsStraight => self.template_stairsstraight(world, clip, random),
            Kind::Stairs | Kind::Start => self.template_stairs(world, clip, random),
            Kind::Crossing => self.template_crossing(world, clip, random),
            Kind::ChestCorridor => self.template_chestcorridor(world, clip, random),
            Kind::Library => self.template_library(world, clip, random),
            Kind::PortalRoom => self.template_portalroom(world, clip, random),
            Kind::Corridor => self.template_corridor(world, clip, random),
        }
        true
    }
    pub(super) fn liquid(&self, world: &impl StrongholdWorld, clip: BoundingBox) -> bool {
        let lo: [i32; 3] = std::array::from_fn(|i| (self.bounds.min[i] - 1).max(clip.min[i]));
        let hi: [i32; 3] = std::array::from_fn(|i| (self.bounds.max[i] + 1).min(clip.max[i]));
        for x in lo[0]..=hi[0] {
            for z in lo[2]..=hi[2] {
                if material::is_liquid(world.get([x, lo[1], z]))
                    || material::is_liquid(world.get([x, hi[1], z]))
                {
                    return true;
                }
            }
        }
        for x in lo[0]..=hi[0] {
            for y in lo[1]..=hi[1] {
                if material::is_liquid(world.get([x, y, lo[2]]))
                    || material::is_liquid(world.get([x, y, hi[2]]))
                {
                    return true;
                }
            }
        }
        for z in lo[2]..=hi[2] {
            for y in lo[1]..=hi[1] {
                if material::is_liquid(world.get([lo[0], y, z]))
                    || material::is_liquid(world.get([hi[0], y, z]))
                {
                    return true;
                }
            }
        }
        false
    }
    pub(super) fn point(&self, x: i32, y: i32, z: i32) -> [i32; 3] {
        self.bounds.local_to_world(x, y, z, self.orientation)
    }
    pub(super) fn get(
        &self,
        world: &impl StrongholdWorld,
        x: i32,
        y: i32,
        z: i32,
        clip: BoundingBox,
    ) -> u16 {
        let p = self.point(x, y, z);
        if clip.contains(p) {
            world.get(p)
        } else {
            0
        }
    }
    pub(super) fn place(
        &self,
        world: &mut impl StrongholdWorld,
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
    pub(super) fn fill(
        &self,
        world: &mut impl StrongholdWorld,
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
    pub(super) fn stones(
        &self,
        world: &mut impl StrongholdWorld,
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
                    let state = if x == x0 || x == x1 || y == y0 || y == y1 || z == z0 || z == z1 {
                        let f = random.next_float();
                        if f < 0.2 {
                            98 << 4 | 2
                        } else if f < 0.5 {
                            98 << 4 | 1
                        } else if f < 0.55 {
                            97 << 4 | 2
                        } else {
                            98 << 4
                        }
                    } else {
                        0
                    };
                    self.place(world, state, x, y, z, clip);
                }
            }
        }
    }
    pub(super) fn random_fill(
        &self,
        world: &mut impl StrongholdWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
        chance: f32,
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
                    if random.next_float() > chance
                        || (existing && self.get(world, x, y, z, clip) >> 4 == 0)
                    {
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
    pub(super) fn random_place(
        &self,
        world: &mut impl StrongholdWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
        chance: f32,
        x: i32,
        y: i32,
        z: i32,
        state: u16,
    ) {
        if random.next_float() < chance {
            self.place(world, state, x, y, z, clip);
        }
    }
    pub(super) fn chest(
        &self,
        world: &mut impl StrongholdWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
        pos: [i32; 3],
        table: u8,
    ) {
        let contents: &[(u16, i32, i32, i32)] = match table {
            0 => &[
                (368, 1, 1, 10),
                (264, 1, 3, 3),
                (265, 1, 5, 10),
                (266, 1, 3, 5),
                (331, 4, 9, 5),
                (297, 1, 3, 15),
                (260, 1, 3, 15),
                (257, 1, 1, 5),
                (267, 1, 1, 5),
                (307, 1, 1, 5),
                (306, 1, 1, 5),
                (308, 1, 1, 5),
                (309, 1, 1, 5),
                (322, 1, 1, 1),
                (329, 1, 1, 1),
                (417, 1, 1, 1),
                (418, 1, 1, 1),
                (419, 1, 1, 1),
            ],
            1 => &[
                (340, 1, 3, 20),
                (339, 2, 7, 20),
                (395, 1, 1, 1),
                (345, 1, 1, 1),
            ],
            _ => &[
                (265, 1, 5, 10),
                (266, 1, 3, 5),
                (331, 4, 9, 5),
                (263, 3, 8, 10),
                (297, 1, 3, 15),
                (260, 1, 3, 15),
                (257, 1, 1, 1),
            ],
        };
        let mut entries: Vec<_> = contents
            .iter()
            .map(|&(id, min, max, weight)| LootEntry::new(id, 0, min, max, weight))
            .collect();
        let mut book = LootEntry::book(random);
        if table == 1 {
            book.min = 1;
            book.max = 5;
            book.weight = 2;
        }
        entries.push(book);
        let rolls = if table == 0 {
            2 + random.next_int(2)
        } else {
            1 + random.next_int(4)
        };
        let p = self.point(pos[0], pos[1], pos[2]);
        if !clip.contains(p) || world.get(p) >> 4 == 54 {
            return;
        }
        world.set(p, 54 << 4 | chest_facing(world, p) as u16);
        world.chest(p, fill_loot(random, &entries, rolls, 27));
    }
    pub(super) fn spawn_silverfish(&mut self, world: &mut impl StrongholdWorld, clip: BoundingBox) {
        let p = self.point(5, 3, 6);
        if !self.spawner_made && clip.contains(p) {
            self.spawner_made = true;
            world.set(p, 52 << 4);
            world.spawner(p);
        }
    }
}
fn chest_facing(world: &impl StrongholdWorld, p: [i32; 3]) -> u8 {
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
