//! Minecraft 1.8.9 VisGraph and the six-direction RenderGlobal traversal.
use super::Frustum;
use crate::world::{Chunk, ChunkPos};
use glam::{IVec3, Mat4, Vec3};
use std::collections::{HashMap, VecDeque};

const ALL: u64 = (1 << 36) - 1;
const DIRS: [IVec3; 6] = [
    IVec3::NEG_Y,
    IVec3::Y,
    IVec3::NEG_Z,
    IVec3::Z,
    IVec3::NEG_X,
    IVec3::X,
];

pub(crate) struct Section {
    occupied: [u64; 64],
    connections: u64,
}
fn occupied(bits: &[u64; 64], i: usize) -> bool {
    bits[i / 64] & (1 << (i % 64)) != 0
}
fn exits(i: usize) -> u8 {
    let x = i & 15;
    let z = (i >> 4) & 15;
    let y = i >> 8;
    u8::from(y == 0)
        | (u8::from(y == 15) << 1)
        | (u8::from(z == 0) << 2)
        | (u8::from(z == 15) << 3)
        | (u8::from(x == 0) << 4)
        | (u8::from(x == 15) << 5)
}
fn flood(bits: &mut [u64; 64], start: usize, stack: &mut Vec<usize>) -> u8 {
    stack.clear();
    stack.push(start);
    bits[start / 64] |= 1 << (start % 64);
    let mut reached = 0;
    while let Some(i) = stack.pop() {
        let edge = exits(i);
        reached |= edge;
        for (face, step) in [-256, 256, -16, 16, -1, 1].into_iter().enumerate() {
            if edge & (1 << face) != 0 {
                continue;
            }
            let next = (i as i32 + step) as usize;
            if !occupied(bits, next) {
                bits[next / 64] |= 1 << (next % 64);
                stack.push(next);
            }
        }
    }
    reached
}
impl Section {
    fn new(bits: [u64; 64]) -> Self {
        let count: u32 = bits.iter().map(|n| n.count_ones()).sum();
        let mut connections = if count < 256 { ALL } else { 0 };
        if (256..4096).contains(&count) {
            let mut marks = bits;
            let mut stack = Vec::new();
            for i in 0..4096 {
                if exits(i) != 0 && !occupied(&marks, i) {
                    let open = flood(&mut marks, i, &mut stack);
                    for face in 0..6 {
                        if open & (1 << face) != 0 {
                            connections |= u64::from(open) << (face * 6);
                        }
                    }
                }
            }
        }
        Self {
            occupied: bits,
            connections,
        }
    }
    #[cfg(test)]
    fn camera_exits(&self, local: usize) -> u8 {
        if self.occupied.iter().all(|n| *n == 0) {
            return 63;
        }
        let mut marks = self.occupied;
        flood(&mut marks, local, &mut Vec::new())
    }
}
fn camera_exits(bits: &[u64; 64], local: usize) -> u8 {
    if bits.iter().all(|n| *n == 0) {
        return 63;
    }
    let mut marks = *bits;
    flood(&mut marks, local, &mut Vec::new())
}
/// One column's immutable visibility data. Traversal reads the compact links;
/// only the camera section needs its occupied voxel bits. Payload stays 8320B.
pub(crate) struct Graph {
    connections: [u64; 16],
    occupied: [[u64; 64]; 16],
}
struct SectionRef<'a> { occupied: &'a [u64; 64] }
impl SectionRef<'_> {
    fn camera_exits(&self, local: usize) -> u8 { camera_exits(self.occupied, local) }
}
impl Graph {
    fn get(&self, y: usize) -> Option<SectionRef<'_>> {
        self.occupied.get(y).map(|occupied| SectionRef { occupied })
    }
    fn new(chunk: &Chunk, fancy: bool) -> Self {
        let mut graph = Self { connections: [0; 16], occupied: [[0; 64]; 16] };
        for y in 0..16 {
            let mut bits = [0; 64];
            if (y * 16) < chunk.top as usize {
                for (i, &state) in chunk.blocks[y * 4096..(y + 1) * 4096].iter().enumerate() {
                    let opaque = if matches!(state >> 4, 18 | 161) {
                        !fancy
                    } else {
                        crate::vanilla::material::opaque_cube(state)
                    };
                    if opaque { bits[i / 64] |= 1 << (i % 64); }
                }
            }
            let section = Section::new(bits);
            graph.connections[y] = section.connections;
            graph.occupied[y] = section.occupied;
        }
        graph
    }
}
pub(crate) struct TerrainSections {
    pub graph: Box<Graph>,
    /// Original block that emitted each quad, including model overhang.
    pub owners: Vec<u8>,
    pub chests: Vec<IVec3>,
}
impl TerrainSections {
    pub(crate) fn new(chunk: &Chunk, fancy: bool) -> Self {
        Self {
            graph: Box::new(Graph::new(chunk, fancy)),
            owners: Vec::new(),
            chests: Vec::new(),
        }
    }
}

#[derive(Default)]
pub(crate) struct View {
    key: Option<(Mat4, Vec3, Vec3, u8)>,
    // Same bounded column slot already used by traversal. Low16 are seen
    // sections; high16 are accepted sections. Neither changes traversal order.
    reached_origin: ChunkPos,
    reached_side: usize,
    record_order:bool,
    render_order:HashMap<IVec3,u32>,
    frustum_only: bool,
    camera_exits: Option<(IVec3, u8)>,
    pending: VecDeque<(IVec3, Option<u8>, u8)>,
    seen: Vec<u32>,
}
impl View {
    pub fn track_order(&mut self,on:bool){if self.record_order!=on{self.key=None;self.record_order=on;}}
    pub fn order(&self,p:IVec3)->Option<u32>{self.render_order.get(&p).copied()}
    pub fn invalidate(&mut self) {
        self.key = None;
        self.camera_exits = None;
    }
    pub fn update<'a>(
        &mut self,
        matrix: Mat4,
        frustum: &Frustum,
        eye: Vec3,
        direction: Vec3,
        radius: u8,
        graph: impl Fn(ChunkPos) -> Option<&'a Graph>,
    ) {
        let key = (matrix, eye, direction, radius);
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        self.reached_side=0;self.render_order.clear();
        self.frustum_only = true;
        let block = eye.floor().as_ivec3();
        let start = IVec3::new(
            block.x.div_euclid(16),
            block.y.div_euclid(16),
            block.z.div_euclid(16),
        );
        let Some(first) = usize::try_from(start.y)
            .ok()
            .and_then(|y| graph((start.x, start.z))?.get(y))
        else {
            return;
        };
        let local = Chunk::index(
            block.x.rem_euclid(16),
            block.y.rem_euclid(16),
            block.z.rem_euclid(16),
        );
        // Third-person cameras, spectators and corrections may place the eye
        // inside solid geometry. Retain all frustum-visible sections there.
        if occupied(first.occupied, local) {
            return;
        }
        self.frustum_only = false;
        let radius = i32::from(radius);
        let side = (radius * 2 + 1) as usize;
        self.reached_origin = (start.x, start.z);
        self.reached_side = side;
        self.seen.resize(side * side, 0);
        self.seen.fill(0);
        let first_bit = 1u32 << start.y;
        self.seen[radius as usize * (side + 1)] = first_bit | (first_bit << 16);
        if self.record_order{self.render_order.insert(start,0);}
        let mut visible_exits = match self.camera_exits {
            Some((cached, exits)) if cached == block => exits,
            _ => {
                let exits = first.camera_exits(local);
                self.camera_exits = Some((block, exits));
                exits
            }
        };
        if visible_exits.count_ones() == 1 {
            let mut best = f32::MIN_POSITIVE;
            let mut facing = 2;
            for (index, dir) in DIRS.iter().enumerate() {
                let dot = dir.as_vec3().dot(direction);
                if dot > best {
                    best = dot;
                    facing = index;
                }
            }
            visible_exits &= !(1 << (facing ^ 1));
        }
        if visible_exits == 0 {
            return;
        }
        let pending = &mut self.pending;
        pending.clear();
        pending.push_back((start, None, 0));
        let seen = &mut self.seen;
        while let Some((current, incoming, travel)) = pending.pop_front() {
            // Unpublished chunk meshes are traversable until the corresponding
            // opaque mask arrives. Their arrival invalidates the cached view.
            let links =
                graph((current.x, current.z)).map_or(ALL, |g| g.connections[current.y as usize]);
            for (face, delta) in DIRS.iter().enumerate() {
                if travel & (1 << (face ^ 1)) != 0
                    || incoming.is_some_and(|f| links & (1 << ((usize::from(f) ^ 1) * 6 + face)) == 0)
                {
                    continue;
                }
                let next = current + *delta;
                let dx = next.x - start.x;
                let dz = next.z - start.z;
                if !(0..16).contains(&next.y) || dx.abs() > radius || dz.abs() > radius {
                    continue;
                }
                let mark = &mut seen[(dz + radius) as usize * side + (dx + radius) as usize];
                let bit = 1u32 << next.y;
                if *mark & bit != 0 {
                    continue;
                }
                *mark |= bit;
                let min = (next * 16).as_vec3();
                if !frustum.visible(min, min + Vec3::splat(16.)) {
                    continue;
                }
                *mark |= bit << 16;
                if self.record_order{self.render_order.insert(next,self.render_order.len() as u32);}
                pending.push_back((next, Some(face as u8), travel | 1 << face));
            }
        }
    }
    pub fn mask(&self, pos: ChunkPos, frustum: &Frustum, present: u16) -> u16 {
        if self.frustum_only {
            let Some((_, eye, _, radius)) = self.key else {
                return 0;
            };
            let block = eye.floor().as_ivec3();
            if pos.0.abs_diff(block.x.div_euclid(16)) > u32::from(radius)
                || pos.1.abs_diff(block.z.div_euclid(16)) > u32::from(radius)
            {
                return 0;
            }
            let mut mask = 0;
            for y in 0..16 {
                let bit = 1 << y;
                if present & bit == 0 {
                    continue;
                }
                let min = Vec3::new((pos.0 * 16) as f32, (y * 16) as f32, (pos.1 * 16) as f32);
                if frustum.visible(min, min + Vec3::splat(16.)) {
                    mask |= bit;
                }
            }
            mask
        } else {
            let side = self.reached_side;
            if side == 0 { return 0; }
            // A caller can query an arbitrary retained/missing column. Use a
            // wide offset before bounds checks instead of overflowing i32 at
            // distant opposite-sign coordinates; old HashMap misses were zero.
            let radius = (side / 2) as i64;
            let x = i64::from(pos.0) - i64::from(self.reached_origin.0) + radius;
            let z = i64::from(pos.1) - i64::from(self.reached_origin.1) + radius;
            if !(0..side as i64).contains(&x) || !(0..side as i64).contains(&z) { return 0; }
            (self.seen[z as usize * side + x as usize] >> 16) as u16 & present
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn section_graph_and_camera_exits_match_original_visgraph() {
        let rows: Vec<(Vec<i8>, u64, usize, u8)> =
            serde_json::from_reader(flate2::read::GzDecoder::new(
                &include_bytes!("../tests/fixtures/vanilla189-section-visibility.json.gz")[..],
            ))
            .unwrap();
        assert_eq!(rows.len(), 136);
        for (bytes, connections, start, open) in rows {
            let bits = std::array::from_fn(|i| {
                u64::from_le_bytes(std::array::from_fn(|j| bytes[i * 8 + j] as u8))
            });
            let section = Section::new(bits);
            assert_eq!(section.connections, connections);
            assert_eq!(section.camera_exits(start), open);
        }
    }
    #[test]
    fn traversal_rechecks_an_opened_wall_and_keeps_unknown_sections_visible() {
        let mut middle = Chunk::empty();
        for y in 0..16 {
            for z in 0..16 {
                middle.set(8, y, z, 16);
            }
        }
        let wall = TerrainSections::new(&middle, true);
        let open = TerrainSections::new(&Chunk::empty(), true);
        let eye = Vec3::new(8., 8., 8.);
        let direction = Vec3::X;
        let matrix =
            Mat4::perspective_rh(0.1, 1., 0.05, 100.) * Mat4::look_to_rh(eye, direction, Vec3::Y);
        let frustum = Frustum::new(matrix);
        let mut view = View::default();
        view.update(matrix, &frustum, eye, direction, 4, |p| {
            Some(if p == (1, 0) {
                &wall.graph
            } else {
                &open.graph
            })
        });
        assert_eq!(view.mask((1, 0), &frustum, 1), 1);
        assert_eq!(view.mask((2, 0), &frustum, 1), 0);
        view.invalidate();
        view.update(matrix, &frustum, eye, direction, 4, |p| {
            if p == (1, 0) {
                None
            } else {
                Some(&open.graph)
            }
        });
        assert_eq!(view.mask((2, 0), &frustum, 1), 1);
        view.invalidate();
        view.update(matrix, &frustum, eye, direction, 4, |p| {
            if p == (0, 0) {
                None
            } else {
                Some(&wall.graph)
            }
        });
        assert_eq!(view.mask((2, 0), &frustum, 1), 1);
        // A distance reduction still applies when the camera chunk has not
        // been uploaded and the streamer retains old columns temporarily.
        view.update(matrix, &frustum, eye, direction, 1, |_| None);
        assert_eq!(view.mask((2, 0), &frustum, 1), 0);
        // Moving the eye into the separating wall enables the conservative
        // spectator/third-person fallback without requiring a mesh upload.
        let inside = Vec3::new(24.5, 8., 8.);
        let matrix = Mat4::perspective_rh(0.1, 1., 0.05, 100.)
            * Mat4::look_to_rh(inside, direction, Vec3::Y);
        let frustum = Frustum::new(matrix);
        view.update(matrix, &frustum, inside, direction, 4, |p| {
            Some(if p == (1, 0) {
                &wall.graph
            } else {
                &open.graph
            })
        });
        assert_eq!(view.mask((2, 0), &frustum, 1), 1);
    }
}

#[cfg(test)]
#[path = "section_visibility_dense_tests.rs"]
mod dense_tests;

#[cfg(test)]
#[path = "section_graph_layout_tests.rs"]
mod graph_layout_tests;
