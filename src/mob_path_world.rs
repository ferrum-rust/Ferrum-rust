//! PathNavigate's per-search ChunkCache: borrow chunk columns once instead of
//! hashing the world's chunk map for every WalkNodeProcessor block probe.
use super::{Options, Target, Walker};
use crate::{vanilla::movement::{Aabb, MovementWorld}, world::{Chunk, World, HEIGHT}};
use glam::IVec3;

pub(crate) struct ChunkWindow<'a> {
    world: &'a World,
    x: i32,
    z: i32,
    width: usize,
    depth: usize,
    chunks: Vec<Option<&'a Chunk>>,
}

impl<'a> ChunkWindow<'a> {
    pub(crate) fn new(world: &'a World, walker: Walker, options: Options, target: Target) -> Self {
        // Original PathNavigate uses range + 8 for a block target and range +
        // 16 for an entity target. The bound limits allocation for unusual
        // attributes; probes outside it still use the ordinary World lookup.
        let extra = match target { Target::Block(_) => 8., Target::Entity(_) => 16. };
        let radius = (options.range + extra).clamp(0., 256.) as i32;
        let center = walker.position.floor().as_ivec3();
        let x = center.x.saturating_sub(radius) >> 4;
        let z = center.z.saturating_sub(radius) >> 4;
        let end_x = center.x.saturating_add(radius) >> 4;
        let end_z = center.z.saturating_add(radius) >> 4;
        let width = (end_x - x + 1) as usize;
        let depth = (end_z - z + 1) as usize;
        let mut chunks = Vec::with_capacity(width * depth);
        for cx in x..=end_x {
            for cz in z..=end_z {
                chunks.push(world.chunks.get(&(cx, cz)).map(|chunk| chunk.as_ref()));
            }
        }
        Self { world, x, z, width, depth, chunks }
    }

    pub(crate) fn find(&self, walker: Walker, options: Options, target: Target) -> Option<Vec<IVec3>> {
        // Dense columns make repeated probes cheaper than a second hash table
        // for clearance results, including one-column animal bodies.
        super::find_inner::<false>(self, walker, options, target)
    }

    #[inline]
    fn slot(&self, x: i32, z: i32) -> Option<usize> {
        // Chunk coordinates only span 28 signed bits, so subtracting either
        // endpoint is safe even at the extremes of the block coordinate type.
        let dx = (x >> 4) - self.x;
        let dz = (z >> 4) - self.z;
        ((dx as usize) < self.width && (dz as usize) < self.depth)
            .then(|| dx as usize * self.depth + dz as usize)
    }
}

impl MovementWorld for ChunkWindow<'_> {
    #[inline]
    fn get(&self, p: IVec3) -> u16 {
        if !(0..HEIGHT).contains(&p.y) { return 0; }
        if let Some(slot) = self.slot(p.x, p.z) {
            self.chunks[slot].map_or(0, |chunk| chunk.get(p.x & 15, p.y, p.z & 15))
        } else {
            // Unlike Java ChunkCache's air outside its range, keep the current
            // pathfinder's complete-world semantics unchanged by this cache.
            self.world.get(p)
        }
    }
    fn block_boxes(&self, p: IVec3, state: u16, out: &mut Vec<Aabb>) {
        self.world.block_boxes(p, state, out);
    }
    fn loaded_column(&self, x: i32, z: i32) -> bool {
        self.slot(x, z).map_or_else(
            || self.world.loaded_column(x, z),
            |slot| self.chunks[slot].is_some(),
        )
    }
}

#[cfg(test)]
pub fn find_world(world: &World, walker: Walker, options: Options, target: Target) -> Option<Vec<IVec3>> {
    let view = ChunkWindow::new(world, walker, options, target);
    view.find(walker, options, target)
}
