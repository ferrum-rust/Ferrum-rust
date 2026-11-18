//! Keep section visibility separate from transparent distance ordering. Visible
//! translucent faces maintain their relative order across section boundaries.
use super::{geometry, GpuMesh, Mesh, Vec3};
use crate::mesh::visibility::Graph;
use std::ops::Range;
#[path = "transparency_sort_batch.rs"]
pub(super) mod sort_batch;
#[derive(Clone, Copy)]
pub(super) struct Quad {
    pub(super) center: Vec3,
    pub(super) indices: [u32; 6],
    pub(super) section: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sort { Skipped, Identical, Changed }
/// Only the active key representation owns scratch. A quad list is immutable
/// until partition() resets it, so no per-key width checks are needed.
enum DistanceKeys {
    Packed(Vec<u64>),
    Wide(Vec<(std::cmp::Reverse<u32>, usize)>),
}
impl Default for DistanceKeys {
    fn default() -> Self { Self::Packed(Vec::new()) }
}
impl DistanceKeys {
    fn packed_fits(count: usize) -> bool {
        u32::try_from(count.saturating_sub(1)).is_ok()
    }
    fn configure(&mut self, count: usize, force_wide: bool) {
        let packed = !force_wide && Self::packed_fits(count);
        if packed != matches!(self, Self::Packed(_)) {
            *self = if packed { Self::Packed(Vec::new()) } else { Self::Wide(Vec::new()) };
        }
        self.clear();
    }
    fn clear(&mut self) {
        match self { Self::Packed(keys) => keys.clear(), Self::Wide(keys) => keys.clear() }
    }
    fn reserve(&mut self, count: usize) {
        match self { Self::Packed(keys) => keys.reserve(count), Self::Wide(keys) => keys.reserve(count) }
    }
    #[cfg(test)]
    fn rows(&self) -> Vec<(std::cmp::Reverse<u32>,usize)> {
        match self {
            Self::Packed(keys) => keys.iter().map(|&key|
                (std::cmp::Reverse(!((key >> 32) as u32)), (key as u32) as usize)).collect(),
            Self::Wide(keys) => keys.clone(),
        }
    }
}
fn packed_distance(bits: u32, index: u32) -> u64 {
    (u64::from(!bits) << 32) | u64::from(index)
}
/// Order chunk columns front-to-back by horizontal X-Z column midpoint distance
/// rather than 3D AABB midpoint. Because `gpu.max.y` equals `chunk.top`, a 3D
/// midpoint biases open-air columns (`mid.y = top * 0.5`) against adjacent columns
/// with tall roofs or ceilings, inverting `visible.iter().rev()` back-to-front
/// translucent draws along chunk seams (such as flat slime block pads).
#[inline]
pub(super) fn cmp_column_front_to_back(
    (pa, min_a, max_a): (crate::world::ChunkPos, Vec3, Vec3),
    (pb, min_b, max_b): (crate::world::ChunkPos, Vec3, Vec3),
    eye: Vec3,
) -> std::cmp::Ordering {
    let dxa = (min_a.x + max_a.x) * 0.5 - eye.x;
    let dza = (min_a.z + max_a.z) * 0.5 - eye.z;
    let dxb = (min_b.x + max_b.x) * 0.5 - eye.x;
    let dzb = (min_b.z + max_b.z) * 0.5 - eye.z;
    (dxa * dxa + dza * dza)
        .total_cmp(&(dxb * dxb + dzb * dzb))
        .then_with(|| pa.cmp(&pb))
}
fn finish_distance_order(indices: &mut Vec<u32>, quads: &[Quad],
    order: impl ExactSizeIterator<Item=usize> + Clone) -> Sort {
    if indices.len() == order.len() * 6
        && order.clone().enumerate().all(|(at,i)| indices[at*6..at*6+6] == quads[i].indices)
    { return Sort::Identical; }
    indices.clear();
    indices.extend(order.flat_map(|i| quads[i].indices));
    Sort::Changed
}
#[derive(Default)]
struct Order {
    quads: Vec<Quad>,
    by_distance: DistanceKeys,
    indices: Vec<u32>,
    narrow_indices: Vec<u16>,
    eye: Option<Vec3>,
    mask: u16,
    #[cfg(test)]
    force_wide: bool,
}
impl Order {
    fn partition(&mut self, mesh: &mut Mesh, owners: &[u8], ends: &mut [u32; 16]) -> u32 {
        self.quads.clear();
        self.by_distance.clear();
        self.indices.clear();
        self.narrow_indices.clear();
        self.eye = None;
        let mut opaque: [Vec<u32>; 16] = Default::default();
        for (i, indices) in mesh.indices.as_chunks::<6>().0.iter().enumerate() {
            let section = owners.get(i).copied().unwrap_or(0);
            if mesh.vertices[indices[0] as usize].layer & crate::models::BLEND == 0 {
                opaque[section as usize].extend_from_slice(indices);
                continue;
            }
            let first = *indices.iter().min().unwrap() as usize;
            let verts = &mesh.vertices[first..first + 4];
            let base = Vec3::from_array(verts[0].position).floor();
            let enclosed = verts.iter().all(|v| {
                let d = Vec3::from_array(v.position) - base;
                d.min_element() > 0.01 && d.max_element() < 0.99
            });
            let center = if enclosed {
                base + Vec3::splat(0.5)
            } else {
                verts
                    .iter()
                    .fold(Vec3::ZERO, |p, v| p + Vec3::from_array(v.position))
                    * 0.25
            };
            self.quads.push(Quad {
                center,
                indices: *indices,
                section,
            });
        }
        mesh.indices.clear();
        for (y, indices) in opaque.iter().enumerate() {
            mesh.indices.extend_from_slice(indices);
            ends[y] = mesh.indices.len() as u32;
        }
        let solid = mesh.indices.len() as u32;
        #[cfg(not(test))] let force_wide = false;
        #[cfg(test)] let force_wide = self.force_wide;
        self.by_distance.configure(self.quads.len(), force_wide);
        mesh.indices
            .extend(self.quads.iter().flat_map(|q| q.indices));
        solid
    }
    fn sort(&mut self, eye: Vec3, mask: u16) -> Sort {
        if !self.needs_sort(eye, mask) { return Sort::Skipped; }
        let same_membership = self.mask == mask && self.eye.is_some();
        // Production partition already selected/reserved the representation.
        // This also handles direct test fixtures and first-use width forcing.
        if self.eye.is_none() {
            #[cfg(not(test))] let force_wide = false;
            #[cfg(test)] let force_wide = self.force_wide;
            self.by_distance.configure(self.quads.len(), force_wide);
        }
        self.mask = mask;
        self.eye = Some(eye);
        match &mut self.by_distance {
            DistanceKeys::Packed(keys) => {
                debug_assert!(DistanceKeys::packed_fits(self.quads.len()));
                if same_membership {
                    for key in keys.iter_mut() {
                        let i = *key as u32;
                        *key = packed_distance(self.quads[i as usize].center.distance_squared(eye).to_bits(),i);
                    }
                    if keys.is_sorted() { return Sort::Identical; }
                } else {
                    keys.clear();
                    keys.extend(self.quads.iter().enumerate()
                        .filter(|(_,q)| mask & (1 << q.section) != 0)
                        .map(|(i,q)| packed_distance(q.center.distance_squared(eye).to_bits(),i as u32)));
                }
                keys.sort_unstable();
                finish_distance_order(&mut self.indices,&self.quads,
                    keys.iter().map(|&key| (key as u32) as usize))
            }
            DistanceKeys::Wide(keys) => {
                if same_membership {
                    for (distance,i) in keys.iter_mut() {
                        *distance=std::cmp::Reverse(self.quads[*i].center.distance_squared(eye).to_bits());
                    }
                    if keys.is_sorted() { return Sort::Identical; }
                } else {
                    keys.clear();
                    keys.extend(self.quads.iter().enumerate()
                        .filter(|(_,q)| mask & (1 << q.section) != 0)
                        .map(|(i,q)| (std::cmp::Reverse(q.center.distance_squared(eye).to_bits()),i)));
                }
                keys.sort_unstable();
                finish_distance_order(&mut self.indices,&self.quads,keys.iter().map(|&(_,i)|i))
            }
        }
    }
    fn needs_sort(&self, eye: Vec3, mask: u16) -> bool {
        !self.quads.is_empty()
            && !(self.mask == mask && self.eye.is_some_and(|p| p.distance_squared(eye) <= 1.))
    }
}

/// CPU-only terrain upload data. Build this alongside the chunk mesh so the
/// render thread never partitions all its quads or computes translucent centers.
pub(crate) struct PreparedChunk {
    mesh: Mesh,
    indices: geometry::Indices,
    chests: Vec<glam::IVec3>,
    solid_count: u32,
    ends: [u32; 16],
    graph: Option<Box<Graph>>,
    present: u16,
    order: Order,
}
impl PreparedChunk {
    pub(crate) fn new(mut mesh: Mesh) -> Self {
        let (chests, graph, owners) = if let Some(data) = mesh.sections.take() {
            (data.chests, Some(data.graph), data.owners)
        } else {
            (Vec::new(), None, Vec::new())
        };
        debug_assert!(owners.is_empty() || owners.len() == mesh.quads());
        let present = if owners.is_empty() && mesh.quads() != 0 {
            1
        } else {
            owners.iter().fold(0, |mask, y| mask | (1u16 << y))
        };
        let mut order = Order::default();
        let mut ends = [0; 16];
        let solid_count = order.partition(&mut mesh, &owners, &mut ends);
        let indices = geometry::Indices::new(std::mem::take(&mut mesh.indices));
        // Allocate camera-sort scratch on the preparation worker, not during
        // the first visible frame after uploading a water/glass-heavy column.
        order.by_distance.reserve(order.quads.len());
        order.indices.reserve(order.quads.len()*6);
        if indices.format() == wgpu::IndexFormat::Uint16 {
            order.narrow_indices.reserve(order.quads.len()*6);
        }
        Self {
            mesh,
            indices,
            chests,
            solid_count,
            ends,
            graph,
            present,
            order,
        }
    }
    // Called only by the already enabled CPU shadow diagnostic. Keep private
    // geometry fields encapsulated; this copies scalar sizes, never payloads.
    pub(super) fn arena_upload_metadata(&self)->super::near_opaque_shadow::ArenaUpload {
        super::near_opaque_shadow::ArenaUpload {
            vertex_used:(self.mesh.vertices.len()*std::mem::size_of::<crate::mesh::Vertex>()) as u64,
            index_used:self.indices.bytes().len() as u64,
            vertex_count:self.mesh.vertices.len() as u64,
            index_count:u64::from(self.indices.count()),
            width:if self.indices.format()==wgpu::IndexFormat::Uint16{2}else{4},
            ..Default::default()
        }
    }
    pub(super) fn arena_request(&self,capacities:[u64;2])->super::near_arena::Request {
        let vertex_bytes=(self.mesh.vertices.len()*std::mem::size_of::<crate::mesh::Vertex>()) as u64;
        let index_bytes=self.indices.bytes().len() as u64;
        super::near_arena::Request{vertex_bytes,index_bytes,
            vertex_reserved:geometry::terrain_capacity(capacities[0],vertex_bytes),
            index_reserved:geometry::terrain_capacity(capacities[1],index_bytes),
            vertices:u32::try_from(self.mesh.vertices.len()).unwrap_or(u32::MAX),indices:self.indices.count(),
            width:if self.indices.format()==wgpu::IndexFormat::Uint16{super::near_arena::IndexWidth::U16}else{super::near_arena::IndexWidth::U32},
            max_local_index:self.indices.max_local_index()}
    }
    pub(super) fn write_shared(&self,device:&wgpu::Device,queue:&wgpu::Queue,b:&super::near_arena::Binding<'_,wgpu::Buffer>)
        ->std::result::Result<GpuMesh,String> {
        let vertices=bytemuck::cast_slice(&self.mesh.vertices);let indices=self.indices.bytes();
        super::renderer_arena::checked_writes(device,||{
            if !vertices.is_empty(){queue.write_buffer(b.vertex,b.vertex_range.start,vertices);}
            if !indices.is_empty(){queue.write_buffer(b.index,b.index_range.start,indices);}
        })?;
        Ok(GpuMesh{vertices:b.vertex.clone(),indices:b.index.clone(),count:self.indices.count(),
            min:self.mesh.min,max:self.mesh.max,vertex_capacity:b.vertex_range.end-b.vertex_range.start,
            index_capacity:b.index_range.end-b.index_range.start,index_format:self.indices.format(),
            vertex_offset:b.vertex_range.start,index_offset:b.index_range.start,arena_token:None})
    }
    pub(super) fn write_dedicated_checked(&self,device:&wgpu::Device,queue:&wgpu::Queue,capacities:[u64;2])
        ->std::result::Result<GpuMesh,String> {
        let vb=bytemuck::cast_slice(&self.mesh.vertices);let ib=self.indices.bytes();
        let vertex_capacity=geometry::terrain_capacity(capacities[0],vb.len() as u64);
        let index_capacity=geometry::terrain_capacity(capacities[1],ib.len() as u64);
        let vertices=super::renderer_arena::checked_buffer(device,vertex_capacity,
            wgpu::BufferUsages::VERTEX|wgpu::BufferUsages::COPY_DST,"validated arena fallback vertices")?;
        let indices=super::renderer_arena::checked_buffer(device,index_capacity,
            wgpu::BufferUsages::INDEX|wgpu::BufferUsages::COPY_DST,"validated arena fallback indices")?;
        super::renderer_arena::checked_writes(device,||{
            if !vb.is_empty(){queue.write_buffer(&vertices,0,vb);}
            if !ib.is_empty(){queue.write_buffer(&indices,0,ib);}
        })?;
        Ok(GpuMesh{vertices,indices,count:self.indices.count(),min:self.mesh.min,max:self.mesh.max,
            vertex_capacity,index_capacity,index_format:self.indices.format(),vertex_offset:0,index_offset:0,arena_token:None})
    }
    pub(super) fn write_shared_arena_captured(&self,device:&wgpu::Device,queue:&wgpu::Queue,b:&super::near_arena::Binding<'_,wgpu::Buffer>,trace:&mut super::arena_upload_capture::Trace)
        ->std::result::Result<GpuMesh,String> {
        let vertices=bytemuck::cast_slice(&self.mesh.vertices);let indices=self.indices.bytes();
        super::renderer_arena::upload_traced::checked_writes_captured(device,trace,|trace|{
            if !vertices.is_empty(){let writing=std::time::Instant::now();queue.write_buffer(b.vertex,b.vertex_range.start,vertices);trace.phase(super::arena_upload_capture::Phase::VertexWrite,writing);trace.observation.queue_writes[0]+=1;}
            if !indices.is_empty(){let writing=std::time::Instant::now();queue.write_buffer(b.index,b.index_range.start,indices);trace.phase(super::arena_upload_capture::Phase::IndexWrite,writing);trace.observation.queue_writes[1]+=1;}
        })?;
        Ok(GpuMesh{vertices:b.vertex.clone(),indices:b.index.clone(),count:self.indices.count(),
            min:self.mesh.min,max:self.mesh.max,vertex_capacity:b.vertex_range.end-b.vertex_range.start,
            index_capacity:b.index_range.end-b.index_range.start,index_format:self.indices.format(),
            vertex_offset:b.vertex_range.start,index_offset:b.index_range.start,arena_token:None})
    }
    pub(super) fn write_dedicated_arena_captured(&self,device:&wgpu::Device,queue:&wgpu::Queue,capacities:[u64;2],trace:&mut super::arena_upload_capture::Trace)
        ->std::result::Result<GpuMesh,String> {
        let vb=bytemuck::cast_slice(&self.mesh.vertices);let ib=self.indices.bytes();
        let vertex_capacity=geometry::terrain_capacity(capacities[0],vb.len() as u64);
        let index_capacity=geometry::terrain_capacity(capacities[1],ib.len() as u64);
        let vertices=super::renderer_arena::upload_traced::checked_buffer_captured(device,vertex_capacity,
            wgpu::BufferUsages::VERTEX|wgpu::BufferUsages::COPY_DST,"validated arena fallback vertices",trace)?;
        let indices=super::renderer_arena::upload_traced::checked_buffer_captured(device,index_capacity,
            wgpu::BufferUsages::INDEX|wgpu::BufferUsages::COPY_DST,"validated arena fallback indices",trace)?;
        super::renderer_arena::upload_traced::checked_writes_captured(device,trace,|trace|{
            if !vb.is_empty(){let writing=std::time::Instant::now();queue.write_buffer(&vertices,0,vb);trace.phase(super::arena_upload_capture::Phase::VertexWrite,writing);trace.observation.queue_writes[0]+=1;}
            if !ib.is_empty(){let writing=std::time::Instant::now();queue.write_buffer(&indices,0,ib);trace.phase(super::arena_upload_capture::Phase::IndexWrite,writing);trace.observation.queue_writes[1]+=1;}
        })?;
        Ok(GpuMesh{vertices,indices,count:self.indices.count(),min:self.mesh.min,max:self.mesh.max,
            vertex_capacity,index_capacity,index_format:self.indices.format(),vertex_offset:0,index_offset:0,arena_token:None})
    }
    pub(crate) fn byte_len(&self) -> usize {
        self.mesh.vertices.len() * std::mem::size_of::<crate::mesh::Vertex>()
            + self.indices.bytes().len()
    }
    #[cfg(test)]
    pub(super) fn force_wide_indices(&mut self) {
        self.indices.wide();
    }
}
impl From<Mesh> for PreparedChunk {
    fn from(mesh: Mesh) -> Self {
        Self::new(mesh)
    }
}
pub(super) struct ChunkMesh {
    pub gpu: GpuMesh,
    pub chests: Vec<glam::IVec3>,
    solid_count: u32,
    ends: [u32; 16],
    pub graph: Option<Box<Graph>>,
    pub present: u16,
    pub mask: u16,
    pub solid_ranges: Vec<Range<u32>>,
    order: Order,
}
impl ChunkMesh {
    pub(super) fn arena_capture_old_metadata(&self,trace:&mut super::arena_upload_capture::Trace) {
        trace.observation.old_shared=self.gpu.arena_token.is_some();
        trace.observation.old_capacity=[self.gpu.vertex_capacity,self.gpu.index_capacity];
        trace.observation.old_cpu_capacity_bytes=[
            (self.chests.capacity()*std::mem::size_of::<glam::IVec3>()) as u64,
            (self.order.quads.capacity()*std::mem::size_of::<Quad>()) as u64,
            match &self.order.by_distance {
                DistanceKeys::Packed(v)=>(v.capacity()*std::mem::size_of::<u64>()) as u64,
                DistanceKeys::Wide(v)=>(v.capacity()*std::mem::size_of::<(std::cmp::Reverse<u32>,usize)>()) as u64,
            },(self.order.indices.capacity()*4) as u64,(self.order.narrow_indices.capacity()*2) as u64,
        ];trace.observation.old_graph_present=self.graph.is_some();
    }
    pub(super) fn from_prepared_gpu_arena_captured(prepared:PreparedChunk,gpu:GpuMesh,trace:&mut super::arena_upload_capture::Trace)->Self {
        let started=std::time::Instant::now();
        let PreparedChunk{chests,solid_count,ends,graph,present,order,..}=prepared;
        let result=Self{gpu,chests,solid_count,ends,graph,present,order,mask:0,solid_ranges:Vec::new()};
        trace.phase(super::arena_upload_capture::Phase::NewMetadata,started);trace.body_finished();result
    }
    pub(super) fn install_prepared_gpu_arena_captured(&mut self,prepared:PreparedChunk,gpu:GpuMesh,trace:&mut super::arena_upload_capture::Trace) {
        let PreparedChunk{chests,solid_count,ends,graph,present,order,..}=prepared;
        let cpu=std::time::Instant::now();
        self.chests=chests;self.graph=graph;self.present=present;self.solid_count=solid_count;
        self.ends=ends;self.order=order;self.mask=0;self.solid_ranges.clear();
        trace.phase(super::arena_upload_capture::Phase::CpuReplace,cpu);
        let gpu_time=std::time::Instant::now();self.gpu=gpu;
        trace.phase(super::arena_upload_capture::Phase::GpuReplace,gpu_time);trace.body_finished();
    }

    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            gpu: GpuMesh::new(device),
            chests: Vec::new(),
            solid_count: 0,
            ends: [0; 16],
            graph: None,
            present: 0,
            mask: 0,
            solid_ranges: Vec::new(),
            order: Order::default(),
        }
    }
    pub(super) fn from_prepared_gpu(prepared:PreparedChunk,gpu:GpuMesh)->Self {
        let PreparedChunk{chests,solid_count,ends,graph,present,order,..}=prepared;
        Self{gpu,chests,solid_count,ends,graph,present,order,mask:0,solid_ranges:Vec::new()}
    }
    pub(super) fn install_prepared_gpu(&mut self,prepared:PreparedChunk,gpu:GpuMesh) {
        let PreparedChunk{chests,solid_count,ends,graph,present,order,..}=prepared;
        self.chests=chests;self.graph=graph;self.present=present;self.solid_count=solid_count;
        self.ends=ends;self.order=order;self.mask=0;self.solid_ranges.clear();self.gpu=gpu;
    }
    pub fn update_prepared(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        prepared: PreparedChunk,
    ) {
        let PreparedChunk {
            mesh,
            indices,
            chests,
            solid_count,
            ends,
            graph,
            present,
            order,
        } = prepared;
        self.chests = chests;
        self.graph = graph;
        self.present = present;
        self.solid_count = solid_count;
        self.ends = ends;
        self.order = order;
        self.mask = 0;
        self.solid_ranges.clear();
        self.gpu.update_terrain(device, queue, &mesh, &indices);
    }
    pub(super) fn update_prepared_captured(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        prepared: PreparedChunk,
        trace:&mut super::upload_capture::Trace,
    ) {
        let PreparedChunk {
            mesh,
            indices,
            chests,
            solid_count,
            ends,
            graph,
            present,
            order,
        } = prepared;
        trace.observation.old_cpu_capacity_bytes=[
            (self.chests.capacity()*std::mem::size_of::<glam::IVec3>()) as u64,
            (self.order.quads.capacity()*std::mem::size_of::<Quad>()) as u64,
            match &self.order.by_distance {
                DistanceKeys::Packed(v)=>(v.capacity()*std::mem::size_of::<u64>()) as u64,
                DistanceKeys::Wide(v)=>(v.capacity()*std::mem::size_of::<(std::cmp::Reverse<u32>,usize)>()) as u64,
            },(self.order.indices.capacity()*4) as u64,(self.order.narrow_indices.capacity()*2) as u64,
        ];
        trace.observation.old_graph_present=self.graph.is_some();
        trace.observation.old_index_width=if self.gpu.index_format==wgpu::IndexFormat::Uint16{2}else{4};
        let replacing=std::time::Instant::now();
        self.chests = chests;
        self.graph = graph;
        self.present = present;
        self.solid_count = solid_count;
        self.ends = ends;
        self.order = order;
        self.mask = 0;
        self.solid_ranges.clear();
        trace.phase(super::upload_capture::Phase::CpuReplace,replacing);
        self.gpu.update_terrain_captured(device,queue,&mesh,&indices,trace);
        // Original implicit local cleanup follows this final marker. No moved
        // explicit drops, no retained resource references, no RAII Drop guard.
        trace.body_finished();
    }
    pub fn prepare_ranges(&mut self,mask:u16) {
        if self.mask != mask {
            self.mask = mask;
            self.solid_ranges.clear();
            let mut start = 0;
            for (i, &end) in self.ends.iter().enumerate() {
                if mask & (1 << i) != 0 && start != end {
                    if let Some(previous) = self.solid_ranges.last_mut().filter(|r| r.end == start)
                    {
                        previous.end = end;
                    } else {
                        self.solid_ranges.push(start..end);
                    }
                }
                start = end;
            }
        }
    }
    fn account_sorted(&self,sorted:Sort,profile:&mut super::render_profile::Frame) {
        if sorted != Sort::Skipped {
            if profile.enabled {
                profile.counters.transparency_sorted_chunks += 1;
                profile.counters.transparency_scanned_quads += self.order.quads.len();
                profile.counters.transparency_sorted_quads += self.order.indices.len() / 6;
                if sorted == Sort::Identical {
                    profile.counters.transparency_unchanged_chunks += 1;
                    profile.counters.transparency_upload_bytes_avoided += self.order.indices.len()
                        * if self.gpu.index_format == wgpu::IndexFormat::Uint16 { 2 } else { 4 };
                }
            }
        }
    }
    fn upload_sorted(&mut self,queue:&wgpu::Queue,sorted:Sort,profile:&mut super::render_profile::Frame) {
        if sorted == Sort::Changed && !self.order.indices.is_empty() {
            let upload_started = profile.timer();
            let (bytes, stride) = if self.gpu.index_format == wgpu::IndexFormat::Uint16 {
                geometry::narrow_into(&self.order.indices,&mut self.order.narrow_indices);
                (bytemuck::cast_slice(&self.order.narrow_indices),2)
            } else {
                (bytemuck::cast_slice(&self.order.indices),4)
            };
            queue.write_buffer(
                &self.gpu.indices,
                self.gpu.index_offset + u64::from(self.solid_count) * stride,
                bytes,
            );
            profile.stages_ms.transparency_gpu_writes += super::render_profile::elapsed(upload_started);
            profile.buffer_write(bytes.len(), false);
        }
    }
    fn complete_sorted(&mut self,queue:&wgpu::Queue,sorted:Sort,profile:&mut super::render_profile::Frame) {
        self.account_sorted(sorted,profile);self.upload_sorted(queue,sorted,profile);
    }
    pub fn prepare(&mut self, queue: &wgpu::Queue, eye: Vec3, mask: u16,
        profile: &mut super::render_profile::Frame, position:crate::world::ChunkPos,
        capture:&mut super::transparency_capture::Collector) {
        self.prepare_ranges(mask);
        // Avoid two clock reads per unchanged visible chunk on every frame.
        let needs_sort=(profile.enabled||capture.enabled())&&self.order.needs_sort(eye,mask);
        let ticket=if needs_sort&&capture.enabled() {
            let (keys,key_capacity)=match &self.order.by_distance {
                DistanceKeys::Packed(keys)=>(Some(keys.as_slice()),keys.capacity()),
                DistanceKeys::Wide(_)=>(None,0),
            };
            capture.begin(super::transparency_capture::Before{position,
                previous_eye:self.order.eye.map(|p|p.to_array().map(f32::to_bits)),eye:eye.to_array().map(f32::to_bits),
                old_mask:self.order.mask,mask,index_stride:if self.gpu.index_format==wgpu::IndexFormat::Uint16{2}else{4},solid_count:self.solid_count,
                capacities:[self.order.quads.capacity(),key_capacity,self.order.indices.capacity(),self.order.narrow_indices.capacity()],
                quads:&self.order.quads,keys,indices:&self.order.indices})
        }else{None};
        let sort_started=(profile.enabled&&needs_sort).then(std::time::Instant::now);
        let sorted = self.order.sort(eye, mask);
        if sorted != Sort::Skipped {
            profile.stages_ms.transparency_sort += super::render_profile::elapsed(sort_started);
        }
        self.account_sorted(sorted,profile);
        if let Some(ticket)=ticket {
            let result=match sorted{Sort::Skipped=>0,Sort::Identical=>1,Sort::Changed=>2};
            match &self.order.by_distance {
                DistanceKeys::Packed(keys)=>capture.finish(ticket,result,keys,&self.order.indices),
                DistanceKeys::Wide(_)=>capture.finish(ticket,255,&[],&[]),
            }
        }
        self.upload_sorted(queue,sorted,profile);
    }
    /// Conservative NEW membership bound; old active indices can be smaller
    /// after a mask shrink. No quad scan, geometry copy or allocation.
    pub(super) fn staging_bound(&self,eye:Vec3,mask:u16)->Option<u64> {
        if !self.order.needs_sort(eye,mask){return Some(0)}
        (self.order.quads.len() as u64).checked_mul(6)?.checked_mul(
            if self.gpu.index_format==wgpu::IndexFormat::Uint16{2}else{4})
    }
    fn upload_sorted_staged(&mut self,queue:&wgpu::Queue,sorted:Sort,profile:&mut super::render_profile::Frame,
        staging:&mut super::transparent_staging::Session<'_>,encoder:&mut wgpu::CommandEncoder) {
        if sorted == Sort::Changed && !self.order.indices.is_empty() {
            let upload_started = profile.timer();
            let (bytes, stride) = if self.gpu.index_format == wgpu::IndexFormat::Uint16 {
                geometry::narrow_into(&self.order.indices,&mut self.order.narrow_indices);
                (bytemuck::cast_slice(&self.order.narrow_indices),2)
            } else {
                (bytemuck::cast_slice(&self.order.indices),4)
            };
            let relative=u64::from(self.solid_count)*stride;
            assert!(relative.checked_add(bytes.len() as u64).is_some_and(|end|end<=self.gpu.index_capacity),
                "transparent staging must stay inside its current owner reservation");
            staging.write(encoder,queue,&self.gpu.indices,
                self.gpu.index_offset + relative,bytes)
                .expect("transparent staging write failed closed");
            profile.stages_ms.transparency_gpu_writes += super::render_profile::elapsed(upload_started);
            profile.buffer_write(bytes.len(), false);
        }
    }
    fn complete_sorted_staged(&mut self,queue:&wgpu::Queue,sorted:Sort,profile:&mut super::render_profile::Frame,
        staging:&mut super::transparent_staging::Session<'_>,encoder:&mut wgpu::CommandEncoder) {
        self.account_sorted(sorted,profile);self.upload_sorted_staged(queue,sorted,profile,staging,encoder);
    }
    pub fn prepare_staged(&mut self, queue: &wgpu::Queue, eye: Vec3, mask: u16,
        profile: &mut super::render_profile::Frame, position:crate::world::ChunkPos,
        capture:&mut super::transparency_capture::Collector,staging:&mut super::transparent_staging::Session<'_>,
        encoder:&mut wgpu::CommandEncoder) {
        self.prepare_ranges(mask);
        // Avoid two clock reads per unchanged visible chunk on every frame.
        let needs_sort=(profile.enabled||capture.enabled())&&self.order.needs_sort(eye,mask);
        let ticket=if needs_sort&&capture.enabled() {
            let (keys,key_capacity)=match &self.order.by_distance {
                DistanceKeys::Packed(keys)=>(Some(keys.as_slice()),keys.capacity()),
                DistanceKeys::Wide(_)=>(None,0),
            };
            capture.begin(super::transparency_capture::Before{position,
                previous_eye:self.order.eye.map(|p|p.to_array().map(f32::to_bits)),eye:eye.to_array().map(f32::to_bits),
                old_mask:self.order.mask,mask,index_stride:if self.gpu.index_format==wgpu::IndexFormat::Uint16{2}else{4},solid_count:self.solid_count,
                capacities:[self.order.quads.capacity(),key_capacity,self.order.indices.capacity(),self.order.narrow_indices.capacity()],
                quads:&self.order.quads,keys,indices:&self.order.indices})
        }else{None};
        let sort_started=(profile.enabled&&needs_sort).then(std::time::Instant::now);
        let sorted = self.order.sort(eye, mask);
        if sorted != Sort::Skipped {
            profile.stages_ms.transparency_sort += super::render_profile::elapsed(sort_started);
        }
        self.account_sorted(sorted,profile);
        if let Some(ticket)=ticket {
            let result=match sorted{Sort::Skipped=>0,Sort::Identical=>1,Sort::Changed=>2};
            match &self.order.by_distance {
                DistanceKeys::Packed(keys)=>capture.finish(ticket,result,keys,&self.order.indices),
                DistanceKeys::Wide(_)=>capture.finish(ticket,255,&[],&[]),
            }
        }
        self.upload_sorted_staged(queue,sorted,profile,staging,encoder);
    }
    pub fn translucent_range(&self) -> Range<u32> {
        self.solid_count..self.solid_count + self.order.indices.len() as u32
    }
    pub fn triangles(&self) -> usize {
        (self.solid_ranges.iter().map(|r| r.len()).sum::<usize>() + self.order.indices.len()) / 3
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct OriginalOrder {
        quads: Vec<Quad>,
        by_distance: Vec<(std::cmp::Reverse<u32>,usize)>,
        indices: Vec<u32>,
        eye: Option<Vec3>,
        mask: u16,
    }
    // Independent original algorithm, including its eye threshold and mask.
    fn original_sort(order: &mut OriginalOrder, eye: Vec3, mask: u16) -> bool {
        if order.quads.is_empty()
            || (order.mask == mask && order.eye.is_some_and(|p| p.distance_squared(eye) <= 1.))
        { return false; }
        order.mask = mask;
        order.eye = Some(eye);
        order.by_distance.clear();
        order.by_distance.extend(order.quads.iter().enumerate()
            .filter(|(_, q)| mask & (1 << q.section) != 0)
            .map(|(i, q)| (std::cmp::Reverse(q.center.distance_squared(eye).to_bits()), i)));
        order.by_distance.sort_unstable();
        order.indices.clear();
        order.indices.extend(order.by_distance.iter().flat_map(|&(_, i)| order.quads[i].indices));
        true
    }

    #[test]
    fn identical_sort_suppression_preserves_original_indices_thresholds_and_mask_changes() {
        let make = || Order { quads: (0..144).map(|i| Quad {
            center: Vec3::new((i % 9) as f32 - 4., (i % 7) as f32, (i % 11) as f32),
            indices: [i * 4 + 65534, i * 4 + 65535, i * 4 + 65536,
                i * 4 + 65534, i * 4 + 65536, i * 4 + 65537],
            section: (i % 4) as u8,
        }).collect(), ..Default::default() };
        for force_wide in [false,true] {
        let mut actual=make(); actual.force_wide=force_wide;
        let mut expected=OriginalOrder {quads:actual.quads.clone(),..Default::default()};
        let (mut identical, mut changed, mut skipped) = (0, 0, 0);
        let mut path = vec![(Vec3::ZERO, 15), (Vec3::ZERO, 31), (Vec3::X, 31),
            (Vec3::X * 1.01, 31), (Vec3::ZERO, 0), (Vec3::X * 3., 0),
            (Vec3::ZERO, 1), (Vec3::new(f32::NAN, 0., 0.), 15),
            (Vec3::splat(f32::INFINITY), 15), (Vec3::splat(f32::NEG_INFINITY), 15)];
        for i in 0..4096 {
            path.push((Vec3::new((i % 93) as f32 * 0.25 - 12.,
                (i % 19) as f32, (i % 47) as f32 - 23.), (i % 32) as u16));
        }
        // Long unchanged-membership runs exercise temporal ordering reuse;
        // include stable ordering, reversals, ties and nonfinite key changes.
        for mask in [0, 1, 15, 31] {
            for i in 0..256 {
                path.push((Vec3::new(i as f32 * 2. - 256., 6., 3.), mask));
            }
            for eye in [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NAN),
                Vec3::splat(f32::NEG_INFINITY), Vec3::ZERO] {
                path.push((eye, mask));
            }
        }
        for (eye, mask) in path {
            let previous = expected.indices.clone();
            let ran = original_sort(&mut expected, eye, mask);
            let result = actual.sort(eye, mask);
            assert_eq!(result != Sort::Skipped, ran);
            assert_eq!(actual.indices, expected.indices);
            assert_eq!(actual.by_distance.rows(), expected.by_distance);
            assert_eq!(actual.mask, expected.mask);
            assert_eq!(actual.eye.map(|v| v.to_array().map(f32::to_bits)),
                expected.eye.map(|v| v.to_array().map(f32::to_bits)));
            if ran { assert_eq!(result == Sort::Identical, previous == expected.indices); }
            match result { Sort::Identical => identical += 1,
                Sort::Changed => changed += 1, Sort::Skipped => skipped += 1 }
        }
        assert!(identical > 1 && changed > 1000 && skipped > 0);
        // A new mesh must upload its first nonempty sorted prefix even when the
        // previous mesh at this coordinate had exactly the same index values.
        actual.indices.clear(); actual.eye = None;
        assert_eq!(actual.sort(Vec3::ZERO, 15), Sort::Changed);
        assert_eq!(matches!(actual.by_distance,DistanceKeys::Wide(_)),force_wide);
        }
    }

    #[test]
    fn worker_preparation_preserves_section_ranges_and_translucent_order() {
        let mut mesh = crate::models::empty();
        for (x, layer) in [
            (10., 0),
            (20., crate::models::BLEND),
            (30., 0),
            (40., crate::models::BLEND),
        ] {
            crate::models::face(&mut mesh, [Vec3::X * x; 4], [0.; 4], layer, [255; 4]);
        }
        let original = mesh.indices.clone();
        let bytes =
            mesh.vertices.len() * std::mem::size_of::<crate::mesh::Vertex>() + original.len() * 2;
        let chest = glam::IVec3::new(1, 20, 3);
        let mut sections =
            crate::mesh::visibility::TerrainSections::new(&crate::world::Chunk::empty(), true);
        sections.owners = vec![1, 0, 0, 2];
        sections.chests.push(chest);
        mesh.sections = Some(sections);
        let mut prepared = std::thread::spawn(move || PreparedChunk::new(mesh))
            .join()
            .unwrap();
        assert_eq!(prepared.byte_len(), bytes);
        assert_eq!(prepared.solid_count, 12);
        assert_eq!(prepared.present, 0b111);
        assert_eq!(prepared.chests, vec![chest]);
        assert!(prepared.graph.is_some());
        assert!(prepared.mesh.sections.is_none());
        assert_eq!(prepared.ends[0], 6);
        assert!(prepared.ends[1..].iter().all(|&end| end == 12));
        assert_eq!(
            prepared.indices.values(),
            [
                &original[12..18],
                &original[..6],
                &original[6..12],
                &original[18..24]
            ]
            .concat()
        );
        assert_eq!(prepared.order.sort(Vec3::ZERO, 0b101), Sort::Changed);
        assert_eq!(
            prepared.order.indices,
            [&original[18..24], &original[6..12]].concat()
        );
        assert_eq!(prepared.order.sort(Vec3::ZERO, 0b001), Sort::Changed);
        assert_eq!(prepared.order.indices, original[6..12]);
    }
    #[test]
    fn blended_faces_sort_behind_opaque_objects_only_when_eye_moves() {
        let mut mesh = crate::models::empty();
        for (x, layer) in [
            (8., crate::models::BLEND),
            (16., 0),
            (24., crate::models::BLEND),
        ] {
            crate::models::face(&mut mesh, [Vec3::X * x; 4], [0.; 4], layer, [255; 4]);
        }
        let original = mesh.indices.clone();
        let mut order = Order::default();
        assert_eq!(order.partition(&mut mesh, &[], &mut [0; 16]), 6);
        assert_eq!(&mesh.indices[..6], &original[6..12]);
        assert_eq!(order.sort(Vec3::ZERO, u16::MAX), Sort::Changed);
        assert_eq!(order.indices[..6], original[12..18]);
        assert_eq!(order.sort(Vec3::X, u16::MAX), Sort::Skipped);
        assert_eq!(order.sort(Vec3::X * 32., u16::MAX), Sort::Changed);
        assert_eq!(order.indices[..6], original[..6]);
    }
    #[test]
    fn mask_changes_rewrite_only_the_visible_sorted_translucent_prefix() {
        let mut mesh = crate::models::empty();
        for x in [12., 8., 4.] {
            crate::models::face(
                &mut mesh,
                [Vec3::X * x; 4],
                [0.; 4],
                crate::models::BLEND,
                [255; 4],
            );
        }
        let original = mesh.indices.clone();
        let mut order = Order::default();
        order.partition(&mut mesh, &[0, 1, 0], &mut [0; 16]);
        assert_eq!(order.sort(Vec3::ZERO, 1), Sort::Changed);
        assert_eq!(&order.indices[..6], &original[..6]);
        assert_eq!(&order.indices[6..], &original[12..]);
        assert_eq!(order.sort(Vec3::ZERO, 2), Sort::Changed);
        assert_eq!(&order.indices, &original[6..12]);
    }
    #[test]
    fn packed_distance_keys_match_tuple_order_for_bit_and_index_boundaries() {
        use std::cmp::Reverse;
        let bits=[0,1,0x007f_ffff,0x0080_0000,0x3f80_0000,0x7f7f_ffff,
            f32::INFINITY.to_bits(),f32::NAN.to_bits(),0x7fff_ffff,0x8000_0000,
            f32::NEG_INFINITY.to_bits(),0xff80_0001,u32::MAX];
        let mut tuples=Vec::new();
        for bits in bits {for index in [0,1,u32::MAX-1,u32::MAX] {
            tuples.push((Reverse(bits),index as usize));
        }}
        let mut state=189u64;
        for _ in 0..2048 {
            state=state.wrapping_mul(6364136223846793005).wrapping_add(1);
            tuples.push((Reverse((state>>32) as u32),(state as u32) as usize));
        }
        let mut packed=tuples.iter().map(|(Reverse(bits),index)|
            packed_distance(*bits,*index as u32)).collect::<Vec<_>>();
        tuples.sort_unstable(); packed.sort_unstable();
        let decoded=DistanceKeys::Packed(packed).rows();
        assert_eq!(decoded,tuples);
        assert!(DistanceKeys::packed_fits(0));
        assert!(DistanceKeys::packed_fits(u32::MAX as usize));
        if let Some(count)=(u32::MAX as usize).checked_add(1) {
            assert!(DistanceKeys::packed_fits(count),"largest valid index is u32::MAX");
            let mut keys=DistanceKeys::default(); keys.configure(count,false);
            assert!(matches!(keys,DistanceKeys::Packed(_)));
            if let Some(too_many)=count.checked_add(1) {
                assert!(!DistanceKeys::packed_fits(too_many));
                keys.configure(too_many,false);
                assert!(matches!(keys,DistanceKeys::Wide(_)),"must not truncate a large index");
            }
        }
    }

    #[test]
    fn packed_key_worker_reserve_keeps_gpu_index_widths_and_repartition_upload_semantics() {
        fn mesh(wide:bool)->Mesh {
            let mut mesh=crate::models::empty();
            for x in [8.,24.] {crate::models::face(&mut mesh,[Vec3::X*x;4],[0.;4],crate::models::BLEND,[255;4]);}
            if wide {
                mesh.vertices.resize(65536,mesh.vertices[0]);
                crate::models::face(&mut mesh,[Vec3::X*16.;4],[0.;4],crate::models::BLEND,[255;4]);
            }
            mesh
        }
        for wide_gpu in [false,true] {
            let mut prepared=PreparedChunk::new(mesh(wide_gpu));
            assert_eq!(prepared.indices.format(),if wide_gpu {wgpu::IndexFormat::Uint32}else{wgpu::IndexFormat::Uint16});
            let DistanceKeys::Packed(keys)=&prepared.order.by_distance else {panic!("normal mesh uses packed keys")};
            assert!(keys.capacity()>=prepared.order.quads.len());
            let pointer=keys.as_ptr(); let capacity=keys.capacity();
            assert_eq!(prepared.order.narrow_indices.capacity()==0,wide_gpu);
            assert_eq!(prepared.order.sort(Vec3::ZERO,u16::MAX),Sort::Changed);
            assert_eq!(prepared.order.sort(Vec3::X,u16::MAX),Sort::Skipped);
            let DistanceKeys::Packed(keys)=&prepared.order.by_distance else {panic!("packed")};
            assert_eq!(keys.as_ptr(),pointer); assert_eq!(keys.capacity(),capacity);
            let mut original=OriginalOrder{quads:prepared.order.quads.clone(),..Default::default()};
            original_sort(&mut original,Vec3::ZERO,u16::MAX);
            assert_eq!(prepared.order.indices,original.indices);
        }
        let mut order=Order::default();
        // Repartition must discard old membership/order even for an identical
        // replacement mesh; CPU key width and GPU index width are independent.
        for force_wide in [false,true,false] {
            order.force_wide=force_wide;
            for wide_gpu in [false,true] {
                let mut mesh=mesh(wide_gpu); order.partition(&mut mesh,&[],&mut[0;16]);
                order.by_distance.reserve(order.quads.len());
                let mut reference=OriginalOrder{quads:order.quads.clone(),..Default::default()};
                assert!(order.eye.is_none()&&order.indices.is_empty());
                assert_eq!(order.sort(Vec3::ZERO,u16::MAX),Sort::Changed);
                original_sort(&mut reference,Vec3::ZERO,u16::MAX);
                assert_eq!(order.indices,reference.indices);
                assert_eq!(order.by_distance.rows(),reference.by_distance);
                assert_eq!(matches!(order.by_distance,DistanceKeys::Wide(_)),force_wide);
            }
        }
    }

    #[test]
    fn slime_columns_keep_back_to_front_chunk_and_inner_core_order_across_uneven_column_heights() {
        fn push_slime_block(mesh: &mut Mesh, base: Vec3, expose_north: bool, expose_south: bool) {
            let blend = crate::models::BLEND;
            let [x0, y0, z0] = base.to_array();
            // Exposed outer [0..1] shell faces (adjacent slime faces culled across block/chunk seams).
            crate::models::face(
                mesh,
                [
                    Vec3::new(x0, y0 + 1., z0),
                    Vec3::new(x0, y0 + 1., z0 + 1.),
                    Vec3::new(x0 + 1., y0 + 1., z0 + 1.),
                    Vec3::new(x0 + 1., y0 + 1., z0),
                ],
                [0.; 4],
                blend,
                [255; 4],
            );
            if expose_north {
                crate::models::face(
                    mesh,
                    [
                        Vec3::new(x0 + 1., y0, z0),
                        Vec3::new(x0, y0, z0),
                        Vec3::new(x0, y0 + 1., z0),
                        Vec3::new(x0 + 1., y0 + 1., z0),
                    ],
                    [0.; 4],
                    blend,
                    [204; 4],
                );
            }
            if expose_south {
                crate::models::face(
                    mesh,
                    [
                        Vec3::new(x0, y0, z0 + 1.),
                        Vec3::new(x0 + 1., y0, z0 + 1.),
                        Vec3::new(x0 + 1., y0 + 1., z0 + 1.),
                        Vec3::new(x0, y0 + 1., z0 + 1.),
                    ],
                    [0.; 4],
                    blend,
                    [204; 4],
                );
            }
            // Uncullable inner [3/16..13/16] slime core (all 6 faces enclosed inside [0, 1]^3).
            let (a, b) = (3. / 16., 13. / 16.);
            for corners in [
                [Vec3::new(a, b, a), Vec3::new(a, b, b), Vec3::new(b, b, b), Vec3::new(b, b, a)],
                [Vec3::new(a, a, b), Vec3::new(a, a, a), Vec3::new(b, a, a), Vec3::new(b, a, b)],
                [Vec3::new(b, a, a), Vec3::new(a, a, a), Vec3::new(a, b, a), Vec3::new(b, b, a)],
                [Vec3::new(a, a, b), Vec3::new(b, a, b), Vec3::new(b, b, b), Vec3::new(a, b, b)],
                [Vec3::new(a, a, a), Vec3::new(a, a, b), Vec3::new(a, b, b), Vec3::new(a, b, a)],
                [Vec3::new(b, a, b), Vec3::new(b, a, a), Vec3::new(b, b, a), Vec3::new(b, b, b)],
            ] {
                crate::models::face(mesh, corners.map(|c| base + c), [0.; 4], blend, [153; 4]);
            }
        }

        let eye = Vec3::new(8.5, 84.62, 20.5);
        // Far chunk (0, 0) has a tall roof/barrier ceiling (top = 140), while near chunk (0, 1)
        // is open-air (top = 83). A 3D midpoint sort previously put (0, 0) ahead of (0, 1),
        // causing visible.iter().rev() to draw (0, 0)'s z=15 inner cores on top of (0, 1)'s z=16 top face.
        for top_far in [1., 83., 140., 256.] {
            for top_near in [1., 83., 140., 256.] {
                let mut columns = [
                    ((0, 0), Vec3::new(0., 0., 0.), Vec3::new(16., top_far, 16.)),
                    ((0, 1), Vec3::new(0., 0., 16.), Vec3::new(16., top_near, 32.)),
                ];
                columns.sort_unstable_by(|a, b| cmp_column_front_to_back(*a, *b, eye));
                assert_eq!(columns[0].0, (0, 1), "near column must precede far column front-to-back");
                assert_eq!(columns[1].0, (0, 0), "far column must draw first in visible.iter().rev()");
            }
        }

        let mut far_mesh = crate::models::empty();
        far_mesh.min = Vec3::new(0., 0., 0.);
        far_mesh.max = Vec3::new(16., 140., 16.);
        push_slime_block(&mut far_mesh, Vec3::new(8., 82., 14.), true, false);
        push_slime_block(&mut far_mesh, Vec3::new(8., 82., 15.), false, false);

        let mut near_mesh = crate::models::empty();
        near_mesh.min = Vec3::new(0., 0., 16.);
        near_mesh.max = Vec3::new(16., 83., 32.);
        push_slime_block(&mut near_mesh, Vec3::new(8., 82., 16.), false, false);
        push_slime_block(&mut near_mesh, Vec3::new(8., 82., 17.), false, true);

        let mut far_prepared = PreparedChunk::new(far_mesh);
        let mut near_prepared = PreparedChunk::new(near_mesh);
        assert_eq!(far_prepared.order.sort(eye, u16::MAX), Sort::Changed);
        assert_eq!(near_prepared.order.sort(eye, u16::MAX), Sort::Changed);

        for prepared in [&far_prepared, &near_prepared] {
            let vertices = &prepared.mesh.vertices;
            let quad_order: Vec<(i32, bool, bool)> = prepared
                .order
                .indices
                .as_chunks::<6>()
                .0
                .iter()
                .map(|idx| {
                    let first = *idx.iter().min().unwrap() as usize;
                    let verts = &vertices[first..first + 4];
                    let base = Vec3::from_array(verts[0].position).floor();
                    let enclosed = verts.iter().all(|v| {
                        let d = Vec3::from_array(v.position) - base;
                        d.min_element() > 0.01 && d.max_element() < 0.99
                    });
                    let p0 = Vec3::from_array(verts[0].position);
                    let p1 = Vec3::from_array(verts[1].position);
                    let p2 = Vec3::from_array(verts[2].position);
                    let centroid = (p0 + p1 + p2 + Vec3::from_array(verts[3].position)) * 0.25;
                    let front_facing = (p1 - p0).cross(p2 - p0).dot(eye - centroid) > 0.;
                    (base.z as i32, enclosed, front_facing)
                })
                .collect();
            for z in [14, 15, 16, 17] {
                let core_first = quad_order.iter().position(|&(qz, enc, _)| qz == z && enc);
                let core_last = quad_order.iter().rposition(|&(qz, enc, _)| qz == z && enc);
                if let (Some(first_core), Some(last_core)) = (core_first, core_last) {
                    for (pos, &(_, _, front)) in quad_order
                        .iter()
                        .enumerate()
                        .filter(|(_, &(qz, enc, _))| qz == z && !enc)
                    {
                        if front {
                            assert!(
                                last_core < pos,
                                "slime z={z} inner core must draw before front-facing outer shell: {quad_order:?}"
                            );
                        } else {
                            assert!(
                                pos < first_core,
                                "slime z={z} back-facing outer shell must sort behind inner core: {quad_order:?}"
                            );
                        }
                    }
                }
            }
            if let (Some(far_core), Some(near_shell)) = (
                quad_order.iter().rposition(|&(qz, enc, _)| qz == 14 && enc),
                quad_order.iter().position(|&(qz, enc, front)| qz == 15 && !enc && front),
            ) {
                assert!(
                    far_core < near_shell,
                    "z=14 inner core must draw before z=15 front-facing outer shell: {quad_order:?}"
                );
            }
        }
    }

}

#[cfg(test)]
#[path = "transparency_capture_replay.rs"]
mod capture_replay;

#[cfg(test)]
#[path="upload_capture_gpu_tests.rs"]
mod upload_capture_gpu_tests;
