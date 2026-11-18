//! Monument block placement and Elder Guardian spawn metadata.
#![allow(clippy::too_many_arguments)]
use super::*;
pub trait MonumentWorld {
    fn get(&self, p: [i32; 3]) -> u16;
    fn set(&mut self, p: [i32; 3], state: u16);
    fn sea_level(&self) -> i32 {
        63
    }
    /// Original location is (x+.5,y,z+.5), elder=true, health=maxHealth.
    fn elder(&mut self, p: [i32; 3]);
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Population {
    pub elders: Vec<[i32; 3]>,
}
pub struct WorldAdapter<'a> {
    pub world: &'a mut crate::world::World,
    pub output: &'a mut Population,
}
impl MonumentWorld for WorldAdapter<'_> {
    fn get(&self, p: [i32; 3]) -> u16 {
        self.world.get(glam::IVec3::from_array(p))
    }
    fn set(&mut self, p: [i32; 3], state: u16) {
        self.world.set(glam::IVec3::from_array(p), state);
    }
    fn elder(&mut self, p: [i32; 3]) {
        self.output.elders.push(p);
    }
}
impl Monument {
    pub fn populate(
        &mut self,
        world: &mut impl MonumentWorld,
        pos: [i32; 2],
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) -> bool {
        if self.processed.contains(&pos)
            || !self
                .bounds
                .intersects_xz(clip.min[0], clip.min[2], clip.max[0], clip.max[2])
        {
            return false;
        }
        if !self.initialized {
            let processed = std::mem::take(&mut self.processed);
            *self = Self::with_random(self.seed, self.chunk[0], self.chunk[1], random);
            self.processed = processed;
        }
        let building = Piece {
            kind: Kind::Building,
            bounds: self.bounds,
            orientation: self.orientation,
            room: None,
            variant: 0,
        };
        building.populate(world, clip, random, &self.rooms);
        for piece in &self.pieces {
            if piece.bounds.intersects(&clip) {
                piece.populate(world, clip, random, &self.rooms);
            }
        }
        self.processed.push(pos);
        true
    }
}
pub(super) fn room(rooms: &[Option<Room>], index: usize) -> &Room {
    rooms[index].as_ref().unwrap()
}
impl Piece {
    pub fn populate(
        &self,
        world: &mut impl MonumentWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
        rooms: &[Option<Room>],
    ) {
        match self.kind {
            Kind::Building => self.template_monumentbuilding(world, clip, random, rooms),
            Kind::Core => self.template_monumentcoreroom(world, clip, random, rooms),
            Kind::DoubleX => self.template_doublexroom(world, clip, random, rooms),
            Kind::DoubleXY => self.template_doublexyroom(world, clip, random, rooms),
            Kind::DoubleY => self.template_doubleyroom(world, clip, random, rooms),
            Kind::DoubleYZ => self.template_doubleyzroom(world, clip, random, rooms),
            Kind::DoubleZ => self.template_doublezroom(world, clip, random, rooms),
            Kind::Entry => self.template_entryroom(world, clip, random, rooms),
            Kind::Penthouse => self.template_penthouse(world, clip, random, rooms),
            Kind::Simple => self.template_simpleroom(world, clip, random, rooms),
            Kind::SimpleTop => self.template_simpletoproom(world, clip, random, rooms),
            Kind::Wing => self.template_wingroom(world, clip, random, rooms),
        }
    }
    pub(super) fn point(&self, x: i32, y: i32, z: i32) -> [i32; 3] {
        self.bounds.local_to_world(x, y, z, self.orientation)
    }
    pub(super) fn get(
        &self,
        world: &impl MonumentWorld,
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
        world: &mut impl MonumentWorld,
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
        world: &mut impl MonumentWorld,
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
    pub(super) fn water(
        &self,
        world: &mut impl MonumentWorld,
        clip: BoundingBox,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        existing: bool,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if existing && self.get(world, x, y, z, clip) >> 4 == 0 {
                        continue;
                    }
                    let state = if self.bounds.min[1] + y >= world.sea_level() {
                        0
                    } else {
                        9 << 4
                    };
                    self.place(world, state, x, y, z, clip);
                }
            }
        }
    }
    pub(super) fn water_replace(
        &self,
        world: &mut impl MonumentWorld,
        clip: BoundingBox,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        state: u16,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if self.get(world, x, y, z, clip) == 9 << 4 {
                        self.place(world, state, x, y, z, clip);
                    }
                }
            }
        }
    }
    pub(super) fn down(
        &self,
        world: &mut impl MonumentWorld,
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
    pub(super) fn intersects(&self, clip: BoundingBox, x0: i32, z0: i32, x1: i32, z1: i32) -> bool {
        let a = self.point(x0, 0, z0);
        let b = self.point(x1, 0, z1);
        clip.intersects_xz(
            a[0].min(b[0]),
            a[2].min(b[2]),
            a[0].max(b[0]),
            a[2].max(b[2]),
        )
    }
    pub(super) fn elder(
        &self,
        world: &mut impl MonumentWorld,
        clip: BoundingBox,
        x: i32,
        y: i32,
        z: i32,
    ) {
        let p = self.point(x, y, z);
        if clip.contains(p) {
            world.elder(p);
        }
    }
}
