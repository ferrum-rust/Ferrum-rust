//! Original fortress templates with explicit tile and immediate-fluid callbacks.
#![allow(clippy::too_many_arguments)]
use super::*;
pub trait FortressWorld {
    fn get(&self, p: [i32; 3]) -> u16;
    fn set(&mut self, p: [i32; 3], state: u16);
    fn chest(&mut self, p: [i32; 3], slots: Vec<Option<LootStack>>);
    fn spawner(&mut self, p: [i32; 3]);
    /// Source is already placed. Execute World.forceBlockUpdateTick with the
    /// supplied population RNG; recursive updates use the world's own RNG.
    fn force_lava(&mut self, p: [i32; 3], random: &mut JavaRandom);
}
#[derive(Default)]
pub struct Population {
    pub chests: Vec<super::super::scattered::GeneratedContainer>,
    pub spawners: Vec<[i32; 3]>,
    pub ticks: crate::block_ticks::TickResult,
}
pub struct WorldAdapter<'a> {
    pub world: &'a mut crate::world::World,
    pub output: &'a mut Population,
    pub ticks: &'a mut crate::block_ticks::BlockTicks,
}
impl FortressWorld for WorldAdapter<'_> {
    fn get(&self, p: [i32; 3]) -> u16 {
        self.world.get(glam::IVec3::from_array(p))
    }
    fn set(&mut self, p: [i32; 3], state: u16) {
        let pos = glam::IVec3::from_array(p);
        if matches!(state >> 4, 8..=13 | 122 | 145) {
            self.ticks.set_nether(true);
            let result = self.ticks.set_block(self.world, pos, state, false);
            self.output.ticks.changed.extend(result.changed);
            self.output.ticks.drops.extend(result.drops);
            self.output.ticks.washed.extend(result.washed);
            return;
        }
        if !(0..crate::world::HEIGHT).contains(&pos.y) {
            return;
        }
        let cpos = (pos.x.div_euclid(16), pos.z.div_euclid(16));
        let Some(chunk_arc) = self.world.chunks.get_mut(&cpos) else {
            return;
        };
        let lx = pos.x.rem_euclid(16);
        let lz = pos.z.rem_euclid(16);
        let idx = crate::world::Chunk::index(lx, pos.y, lz);
        if chunk_arc.blocks.value_at(idx) == state {
            return;
        }
        let id = state >> 4;
        let prev = chunk_arc.blocks.value_at(idx);
        if chunk_arc.light.is_none()
            && chunk_arc.tiles.is_empty()
            && chunk_arc.hoppers.is_empty()
            && chunk_arc.pistons.is_empty()
            && chunk_arc.random_tick_sections.get().is_none()
            && chunk_arc.scheduled_tick_blocks.get().is_none()
            && chunk_arc.partial_light_sections.get().is_none()
            && chunk_arc.skulls.get().is_none()
            && chunk_arc.signs.get().is_none()
            && !matches!(id, 52 | 154 | 176 | 177)
            && !matches!(prev >> 4, 36 | 52 | 154)
        {
            let chunk = std::sync::Arc::make_mut(chunk_arc);
            chunk.blocks[idx] = state;
            if state != 0 && pos.y + 1 > chunk.top {
                chunk.top = pos.y + 1;
            }
            self.world.light_revision = self.world.light_revision.wrapping_add(1);
        } else {
            self.world.set_raw(pos, state);
        }
        self.output.ticks.changed.push(pos);
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
    fn force_lava(&mut self, p: [i32; 3], random: &mut JavaRandom) {
        self.ticks.set_nether(true);
        let result =
            self.ticks
                .force_liquid_update(self.world, glam::IVec3::from_array(p), 10, random);
        self.output.ticks.changed.extend(result.changed);
        self.output.ticks.drops.extend(result.drops);
        self.output.ticks.washed.extend(result.washed);
    }
}
impl Fortress {
    pub fn populate(
        &mut self,
        world: &mut impl FortressWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) -> bool {
        if !self
            .bounds
            .intersects_xz(clip.min[0], clip.min[2], clip.max[0], clip.max[2])
        {
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
impl Piece {
    pub fn populate(
        &mut self,
        world: &mut impl FortressWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        match self.kind {
            Kind::Straight => self.template_straight(world, clip, random),
            Kind::Crossing3 | Kind::Start => self.template_crossing3(world, clip, random),
            Kind::Crossing => self.template_crossing(world, clip, random),
            Kind::Stairs => self.template_stairs(world, clip, random),
            Kind::Throne => self.template_throne(world, clip, random),
            Kind::Entrance => self.template_entrance(world, clip, random),
            Kind::Corridor5 => self.template_corridor5(world, clip, random),
            Kind::Crossing2 => self.template_crossing2(world, clip, random),
            Kind::Corridor2 => self.template_corridor2(world, clip, random),
            Kind::Corridor => self.template_corridor(world, clip, random),
            Kind::Corridor3 => self.template_corridor3(world, clip, random),
            Kind::Corridor4 => self.template_corridor4(world, clip, random),
            Kind::NetherStalkRoom => self.template_netherstalkroom(world, clip, random),
            Kind::End => self.template_end(world, clip, random),
        }
    }
    pub(super) fn point(&self, x: i32, y: i32, z: i32) -> [i32; 3] {
        self.bounds.local_to_world(x, y, z, self.orientation)
    }
    pub(super) fn get(
        &self,
        world: &impl FortressWorld,
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
        world: &mut impl FortressWorld,
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
        world: &mut impl FortressWorld,
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
    pub(super) fn down(
        &self,
        world: &mut impl FortressWorld,
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
        while p[1] > 1 && matches!(world.get(p) >> 4, 0 | 8..=11) {
            world.set(p, state);
            p[1] -= 1;
        }
    }
    pub(super) fn chest(
        &mut self,
        world: &mut impl FortressWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
        pos: [i32; 3],
    ) {
        let p = self.point(pos[0], pos[1], pos[2]);
        if !self.chest_pending || !clip.contains(p) {
            return;
        }
        self.chest_pending = false;
        let entries: Vec<_> = [
            (264, 1, 3, 5),
            (265, 1, 5, 5),
            (266, 1, 3, 15),
            (283, 1, 1, 5),
            (315, 1, 1, 5),
            (259, 1, 1, 5),
            (372, 3, 7, 5),
            (329, 1, 1, 10),
            (418, 1, 1, 8),
            (417, 1, 1, 5),
            (419, 1, 1, 3),
            (49, 2, 4, 2),
        ]
        .into_iter()
        .map(|(id, min, max, weight)| LootEntry::new(id, 0, min, max, weight))
        .collect();
        let rolls = 2 + random.next_int(4);
        if world.get(p) >> 4 == 54 {
            return;
        }
        world.set(p, 54 << 4 | chest_facing(world, p) as u16);
        world.chest(p, fill_loot(random, &entries, rolls, 27));
    }
    pub(super) fn spawner(&mut self, world: &mut impl FortressWorld, clip: BoundingBox) {
        let p = self.point(3, 5, 5);
        if !self.spawner_made && clip.contains(p) {
            self.spawner_made = true;
            world.set(p, 52 << 4);
            world.spawner(p);
        }
    }
    pub(super) fn force_lava(
        &self,
        world: &mut impl FortressWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        let p = self.point(6, 5, 6);
        if clip.contains(p) {
            world.force_lava(p, random);
        }
    }
}
fn chest_facing(world: &impl FortressWorld, p: [i32; 3]) -> u8 {
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
