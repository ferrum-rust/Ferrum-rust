//! Cache bounded groups in the original resident-map traversal order.
use super::{geometry, render_profile, terrain_bundles::Eye, GpuMesh, DEPTH};
use crate::{mesh::Mesh, distant_terrain::TilePos};
const GROUP_SIZE: usize = 16;
const BUILDS_PER_FRAME: usize = 4;
pub(super) fn slot(resident_index: usize) -> usize { resident_index / GROUP_SIZE }

pub(super) struct Tile { pub(super) gpu: GpuMesh, pub(super) revision: u64,
    pub(super) coverage_proof: Option<super::far_coverage::Proof> }
impl Tile {
    pub(super) fn new(device: &wgpu::Device) -> Self { Self { gpu: GpuMesh::new(device), revision: 0, coverage_proof: None } }
    pub(super) fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue,
        mesh: &Mesh, indices: &geometry::Indices, revision: u64) {
        self.revision = revision;
        self.gpu.update_terrain(device, queue, mesh, indices);
    }
}
pub(super) struct Visible<'a> {
    pub(super) group: usize,
    pub(super) position: TilePos,
    pub(super) revision: u64,
    pub(super) gpu: &'a GpuMesh,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Key {
    bindings: u64, eyes: usize, format: wgpu::TextureFormat,
    tiles: Vec<(TilePos,u64)>,
}
struct Cached { key: Key, eyes: Vec<wgpu::RenderBundle> }
#[derive(Default)]
pub(super) struct Groups { slots: Vec<Option<Cached>>, next_payload: u64 }
impl Groups {
    /// Called only when the final benchmark report is assembled.
    pub(super) fn allocation_report(&self) -> serde_json::Value {
        let (mut groups, mut bundles, mut tiles) = (0usize, 0usize, 0usize);
        for cached in self.slots.iter().flatten() {
            groups += 1;
            bundles += cached.eyes.len();
            tiles += cached.key.tiles.len();
        }
        serde_json::json!({
            "slot_count": self.slots.len(),
            "slot_capacity": self.slots.capacity(),
            "cached_groups": groups,
            "cached_eye_bundles": bundles,
            "cached_tile_references": tiles,
            "tiles_per_group_limit": GROUP_SIZE,
        })
    }
    fn invalidate_position(&mut self, position: TilePos) {
        for cached in &mut self.slots {
            if cached.as_ref().is_some_and(|c| c.key.tiles.iter().any(|(p,_)|*p==position)) { *cached=None; }
        }
    }
    pub(super) fn payload_changed(&mut self, position: TilePos) -> u64 {
        self.invalidate_position(position);
        self.next_payload = self.next_payload.wrapping_add(1);
        // Remove/reinsert cannot reuse an old tile revision. Even counter wrap
        // clears every prior payload identity before the revision is reused.
        if self.next_payload==0 { self.slots.clear(); }
        self.next_payload
    }
    pub(super) fn removed(&mut self, position: TilePos, resident: usize) {
        self.invalidate_position(position);
        self.slots.truncate(resident.div_ceil(GROUP_SIZE));
    }
    pub(super) fn invalidate_bindings(&mut self) { self.slots.clear(); }

    pub(super) fn prepare(&mut self, device:&wgpu::Device, format:wgpu::TextureFormat,
        eyes:&[Eye<'_>], bindings:u64, resident:usize, visible:&[Visible<'_>], enabled:bool,
        profile:&mut render_profile::Frame) {
        let started=profile.timer();
        self.slots.resize_with(resident.div_ceil(GROUP_SIZE), ||None);
        let mut cursor=0;
        let mut budget=BUILDS_PER_FRAME;
        let mut active_groups=0;
        let mut active_tiles=0;
        for (group,cached) in self.slots.iter_mut().enumerate() {
            let start=cursor;
            while cursor<visible.len() && visible[cursor].group==group { cursor+=1; }
            let tiles=&visible[start..cursor];
            if tiles.is_empty() { *cached=None; continue; }
            let matches=cached.as_ref().is_some_and(|c| c.key.bindings==bindings
                && c.key.eyes==eyes.len() && c.key.format==format && c.key.tiles.len()==tiles.len()
                && c.key.tiles.iter().copied().eq(tiles.iter().map(|t|(t.position,t.revision))));
            if !matches { *cached=None; }
            if enabled && cached.is_none() && budget!=0 {
                budget-=1;
                let formats=[Some(format)];
                let descriptor=wgpu::RenderBundleEncoderDescriptor {
                    label:Some("distant ordered group"),color_formats:&formats,
                    depth_stencil:Some(wgpu::RenderBundleDepthStencil {format:DEPTH,
                        depth_read_only:false,stencil_read_only:true}),sample_count:1,multiview:None,
                };
                let bundles=eyes.iter().map(|eye| {
                    let mut encoder=device.create_render_bundle_encoder(&descriptor);
                    encoder.set_pipeline(eye.opaque); encoder.set_bind_group(0,eye.bind,&[]);
                    for tile in tiles {
                        encoder.set_vertex_buffer(0,tile.gpu.vertices.slice(..));
                        encoder.set_index_buffer(tile.gpu.indices.slice(..),tile.gpu.index_format);
                        encoder.draw_indexed(0..tile.gpu.count,0,0..1);
                    }
                    encoder.finish(&wgpu::RenderBundleDescriptor {label:Some("distant ordered group")})
                }).collect();
                *cached=Some(Cached { key:Key {bindings,eyes:eyes.len(),format,
                    tiles:tiles.iter().map(|t|(t.position,t.revision)).collect()},eyes:bundles });
                if profile.enabled {
                    profile.counters.far_bundle_rebuild_groups+=1;
                    profile.counters.far_bundle_recorded_draws+=tiles.len()*eyes.len();
                }
            }
            if enabled && cached.is_some() { active_groups+=1; active_tiles+=tiles.len(); }
        }
        debug_assert_eq!(cursor,visible.len());
        if profile.enabled {
            profile.counters.far_cached_visible_groups=active_groups;
            profile.counters.far_cached_visible_tiles=active_tiles;
        }
        profile.stages_ms.far_bundle_prepare+=render_profile::elapsed(started);
    }
    pub(super) fn draw<'a>(&'a self, pass:&mut wgpu::RenderPass<'a>, visible:&'a [Visible<'a>],
        eye:&Eye<'a>, index:usize, enabled:bool, profile:&mut render_profile::Frame) {
        let mut start=0;
        let mut direct_bound=false;
        while start<visible.len() {
            let group=visible[start].group;
            let mut end=start+1;
            while end<visible.len() && visible[end].group==group { end+=1; }
            if let Some(cached)=self.slots[group].as_ref().filter(|_|enabled) {
                pass.execute_bundles([&cached.eyes[index]]);
                direct_bound=false;
                if profile.enabled { profile.counters.far_bundle_runs+=1;
                    profile.counters.far_bundle_draws+=end-start; }
            } else {
                if !direct_bound { pass.set_pipeline(eye.opaque); pass.set_bind_group(0,eye.bind,&[]); direct_bound=true; }
                for tile in &visible[start..end] { tile.gpu.draw(pass); }
                if profile.enabled { profile.counters.far_direct_draws+=end-start; }
            }
            start=end;
        }
        // A far bundle resets render state even if the near cache was absent.
        pass.set_pipeline(eye.opaque); pass.set_bind_group(0,eye.bind,&[]);
    }
}
#[cfg(test)]
#[path="far_group_bundle_tests.rs"]
mod tests;
