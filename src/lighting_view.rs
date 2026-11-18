//! Lighting host for the existing loaded-world play path. Native Runtime owns
//! its own callback-ordered lighting; this adapter reconciles legacy batches.
use crate::{
    vanilla::lighting::{ChunkLight, LightChunk, Lighting},
    world::{Chunk, ChunkPos, World},
};
use glam::IVec3;
use rayon::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub struct LightingView {
    engine: Lighting,
    no_sky_override: Option<bool>,
    seen: crate::world::ChunkMap<Arc<Chunk>>,
    pending: HashSet<ChunkPos>,
    unsettled: HashSet<ChunkPos>,
    last_revision: Option<u64>,
    unpublished: HashMap<ChunkPos, Option<Arc<crate::vanilla::lighting::ChunkLight>>>,
    #[cfg(test)]
    eager_native_adoption: bool,
}

fn same_light(a: &Option<Arc<ChunkLight>>, b: &Option<Arc<ChunkLight>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

fn snapshot_matches(snapshot: &ChunkLight, light: &LightChunk, no_sky: bool) -> bool {
    snapshot.no_sky == no_sky && snapshot.sections == light.sections
        && snapshot.height == light.height && snapshot.values.len() == light.sky.len()
        && snapshot.values.len() == light.block.len()
        && ((light.sky.shares_packed(snapshot, crate::vanilla::lighting::Kind::Sky)
            && light.block.shares_packed(snapshot, crate::vanilla::lighting::Kind::Block))
            || light.sky.with_bytes(|sky| light.block.with_bytes(|block|
                snapshot.values.iter().zip(sky.iter().zip(block))
                    .all(|(&packed, (&sky, &block))| packed == (sky << 4 | block)))))
}

impl LightingView {
    pub fn new(world: &mut World) -> Self {
        let mut value = Self::empty(world);
        value.update(world);
        value
    }
    pub fn new_with_sky(world: &mut World, no_sky: bool) -> Self {
        let mut value = Self::empty(world);
        value.set_no_sky(no_sky);
        value.update(world);
        value
    }
    pub fn set_no_sky(&mut self, no_sky: bool) {
        self.no_sky_override = Some(no_sky);
        if self.engine.no_sky != no_sky {
            self.engine.no_sky = no_sky;
            self.engine.chunks.clear();
            self.seen.clear();
            self.pending.clear();
            self.unsettled.clear();
            self.unpublished.clear();
            self.last_revision = None;
        }
    }
    fn empty(world: &World) -> Self {
        Self {
            engine: Lighting::new(
                &World {
                    chunks: crate::world::ChunkMap::new(),
                    spawn: world.spawn,
                    label: String::new(),
                    save_path: Default::default(),
                    edits: Default::default(),
                    imported: world.imported,
                    light_revision: world.light_revision,
                },
                false,
            ),
            no_sky_override: None,
            seen: crate::world::ChunkMap::new(),
            pending: HashSet::new(),
            unsettled: HashSet::new(),
            last_revision: None,
            unpublished: HashMap::new(),
            #[cfg(test)]
            eager_native_adoption: false,
        }
    }
    fn adopt(&self, snapshot: &Arc<ChunkLight>) -> LightChunk {
        #[cfg(test)]
        if self.eager_native_adoption {
            // Independent scalar reference, including the original literal
            // defaults; do not call the new factory from both oracle sides.
            return LightChunk { height: snapshot.height.clone(),
                minimum: *snapshot.height.iter().min().unwrap_or(&0), sections: snapshot.sections,
                sky: snapshot.values.iter().map(|v| v >> 4).collect(),
                block: snapshot.values.iter().map(|v| v & 15).collect(),
                gaps: vec![false; 256], queued_checks: 4096 };
        }
        snapshot.to_shared_chunk()
    }
    #[cfg(test)]
    fn new_eager_reference(world: &mut World) -> Self {
        let mut value = Self::empty(world);
        value.eager_native_adoption = true;
        value.update(world);
        value
    }
    pub fn update(&mut self, world: &mut World) -> Vec<ChunkPos> {
        if self.last_revision == Some(world.light_revision) {
            return Vec::new();
        }
        if self.no_sky_override.is_none() {
            if let Some(no_sky) = world
                .chunks
                .values()
                .find_map(|c| c.light.as_ref().map(|l| l.no_sky))
            {
                if self.engine.no_sky != no_sky {
                    self.engine.no_sky = no_sky;
                    self.engine.chunks.clear();
                    self.seen.clear();
                    self.pending.clear();
                    self.unsettled.clear();
                    self.unpublished.clear();
                }
            }
        }
        self.engine
            .chunks
            .retain(|p, _| world.chunks.contains_key(p));
        self.pending.retain(|p| world.chunks.contains_key(p));
        self.unsettled.retain(|p| world.chunks.contains_key(p));
        let mut added = false;
        let mut positions = world.chunks.keys().copied().collect::<Vec<_>>();
        positions.sort_unstable();
        let mut adopted = HashSet::new();
        let mut republish = HashSet::new();
        let mut edited = Vec::new();
        for &old_pos in self.seen.keys() {
            if !world.chunks.contains_key(&old_pos) {
                edited.push(old_pos);
            }
        }
        for (pos, original_light) in self.unpublished.drain() {
            let Some(current) = world.chunks.get(&pos) else { continue; };
            let same = same_light(&original_light, &current.light);
            if same {
                // A worker computed this light but could not publish it because
                // a neighbor changed. The main thread still holds the OLD light,
                // not a new native correction. Retain the computed engine and
                // replay changes from the worker's old block inputs normally.
                if let Some(previous) = self.seen.get_mut(&pos) {
                    if !same_light(&previous.light, &current.light) {
                        Arc::make_mut(previous).light = current.light.clone();
                    }
                }
                republish.insert(pos);
            }
        }
        // Native population can update light in an existing chunk without
        // changing any of its blocks (a neighboring spring/tree may suffice).
        // Seed every incoming snapshot before replaying local block changes so
        // those callbacks see the complete current neighborhood. Local edits
        // retain the old snapshot Arc, or clear it when overlaying a publication.
        let incoming = positions.iter().filter_map(|&pos| {
            let current = &world.chunks[&pos];
            let light = current.light.as_ref()?;
            let previous = self.seen.get(&pos).and_then(|c| c.light.as_ref());
            if previous.is_some_and(|old| Arc::ptr_eq(old, light)) {
                return None;
            }
            Some((pos, light))
        }).collect::<Vec<_>>();
        // Adopting immutable native snapshots has no callbacks or world reads.
        // Bound temporary results to32 columns and keep original sorted owner
        // installation before any local lighting callbacks. Small updates stay
        // serial; large initial publications share the existing Rayon pool.
        let parallel = incoming.len() >= 64;
        for batch in incoming.chunks(32) {
            let prepared = if parallel {
                batch.par_iter().map(|(pos, light)| (*pos, self.adopt(light))).collect::<Vec<_>>()
            } else {
                batch.iter().map(|(pos, light)| (*pos, self.adopt(light))).collect::<Vec<_>>()
            };
            for (pos, light) in prepared {
                added |= !self.seen.contains_key(&pos);
                self.engine.chunks.insert(pos, light);
                self.pending.remove(&pos);
                self.unsettled.remove(&pos);
                adopted.insert(pos);
            }
        }
        for &pos in &positions {
            if adopted.contains(&pos) {
                let current = &world.chunks[&pos];
                for &(idx, previous) in &current.light_edits {
                    let i = usize::from(idx);
                    if current.blocks[i] != previous {
                        self.engine.world_block_changed(
                            world,
                            IVec3::new(
                                pos.0 * 16 + (i % 16) as i32,
                                (i / 256) as i32,
                                pos.1 * 16 + (i / 16 % 16) as i32,
                            ),
                            previous,
                        );
                    }
                }
                continue;
            }
            let current = &world.chunks[&pos];
            let Some(old) = self.seen.get(&pos) else {
                added = true;
                if let Some(light) = &current.light {
                    let light = self.adopt(light);
                    self.engine.chunks.insert(pos, light);
                } else {
                    self.engine.add_chunk(world, pos);
                    self.pending.insert(pos);
                    self.unsettled.insert(pos);
                }
                continue;
            };
            if Arc::ptr_eq(current, old) {
                continue;
            }
            if current.light.is_none() {
                // Overlaying a stale native publication may clear its light
                // even when the latest local blocks equal our previous view.
                // Keep the already-reconciled engine snapshot in that case.
                republish.insert(pos);
            }
            // Most edits touch one vertical section. Slice equality skips the
            // other 15 sections with a bulk comparison instead of replaying a
            // branch-heavy 65,536-block scan on every changed chunk.
            let mut block_changed = false;
            for (section, (before, after)) in old.blocks.chunks_exact(4096)
                .zip(current.blocks.chunks_exact(4096)).enumerate() {
                if before == after { continue; }
                block_changed = true;
                for (offset, (&previous, &next)) in before.iter().zip(after).enumerate() {
                    if previous != next {
                        let i = section * 4096 + offset;
                        self.engine.world_block_changed(world, IVec3::new(
                            pos.0 * 16 + (i % 16) as i32, (i / 256) as i32,
                            pos.1 * 16 + (i / 16 % 16) as i32), previous);
                    }
                }
            }
            if block_changed {
                edited.push(pos);
            }
        }
        if added {
            let enclosed = |pos: ChunkPos, world: &World| {
                (-2..=2).all(|dx| {
                    (-2..=2).all(|dz| world.chunks.contains_key(&(pos.0 + dx, pos.1 + dz)))
                })
            };
            let mut pending = self.pending.iter().copied().collect::<Vec<_>>();
            pending.sort_unstable();
            for pos in pending {
                let full = enclosed(pos, world);
                if self
                    .engine
                    .initialize_chunk(world, pos, 63, |p| world.chunks.contains_key(&p))
                {
                    self.pending.remove(&pos);
                    if full {
                        self.unsettled.remove(&pos);
                    }
                }
            }
            let mut unsettled = self
                .unsettled
                .iter()
                .copied()
                .filter(|&pos| !self.pending.contains(&pos) && enclosed(pos, world))
                .collect::<Vec<_>>();
            unsettled.sort_unstable();
            for pos in unsettled {
                if self
                    .engine
                    .initialize_chunk(world, pos, 63, |p| world.chunks.contains_key(&p))
                {
                    self.unsettled.remove(&pos);
                }
            }
        }
        let mut changed = self.engine.take_render_dirty();
        let recomputed = changed.iter().copied().collect::<HashSet<_>>();
        changed.extend(adopted.iter().copied());
        changed.extend(republish.iter().copied());
        changed.sort_unstable();
        changed.dedup();
        for &pos in &changed {
            if let Some(chunk) = world.chunks.get_mut(&pos) {
                // An adopted authoritative snapshot is already the published
                // result unless another callback changed its engine data. Do
                // not clone its 128-KiB block array just to replace equal light.
                if adopted.contains(&pos) && !recomputed.contains(&pos) && !republish.contains(&pos)
                    && chunk.light_edits.is_empty()
                    && chunk.light.as_ref().is_some_and(|light| light.no_sky == self.engine.no_sky) {
                    continue;
                }
                let unchanged = chunk.light_edits.is_empty() && match (&chunk.light, self.engine.chunks.get(&pos)) {
                    (Some(snapshot), Some(light)) => snapshot_matches(snapshot, light, self.engine.no_sky),
                    (None, None) => true,
                    _ => false,
                };
                if unchanged { continue; }
                let chunk = Arc::make_mut(chunk);
                chunk.light = self.engine.render_snapshot(pos);
                chunk.light_edits.clear();
            }
        }
        let mut seed_chunks = changed.clone();
        seed_chunks.extend(edited);
        seed_chunks.sort_unstable();
        seed_chunks.dedup();
        let visual_changed = crate::visual_light::reconcile(world, &seed_chunks, self.engine.no_sky);
        if !visual_changed.is_empty() {
            changed.extend(visual_changed);
            changed.sort_unstable();
            changed.dedup();
        }
        self.seen.clone_from(&world.chunks);
        self.last_revision = Some(world.light_revision);
        changed
    }
    pub(crate) fn retry_publications(&mut self, inputs: &crate::world::ChunkMap<Arc<Chunk>>,
        positions: &HashSet<ChunkPos>) {
        for &p in positions {
            if let Some(input) = inputs.get(&p) { self.unpublished.insert(p, input.light.clone()); }
        }
        if !positions.is_empty() { self.last_revision = None; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_native_unpack_preserves_original_fields_and_arcs_at_dispatch_boundaries() {
        for count in [0, 1, 32, 63, 64, 65, 96] {
            let mut world = World { chunks: Default::default(), spawn: glam::Vec3::ZERO,
                label: String::new(), save_path: Default::default(), edits: Default::default(),
                imported: true, light_revision: 0 };
            for i in 0..count {
                let pos = (i % 12 - 6, i / 12 - 4);
                let snapshot = ChunkLight {
                    values: (0..65536).map(|at| (at * 13 + i as usize * 7) as u8).collect(),
                    height: (0..256).map(|at| (at * 17 + i) % 257).collect(),
                    sections: std::array::from_fn(|y| (y + i as usize) % 3 != 0),
                    no_sky: false,
                };
                let mut chunk = Chunk::empty();
                chunk.light = Some(Arc::new(snapshot));
                world.chunks.insert(pos, Arc::new(chunk));
            }
            let before = world.chunks.clone();
            let seed_calls = crate::vanilla::random::independent_seed_calls_for_thread();
            let view = LightingView::new(&mut world);
            assert_eq!(crate::vanilla::random::independent_seed_calls_for_thread(), seed_calls);
            assert!(view.pending.is_empty());
            assert_eq!(view.engine.chunks.len(), count as usize);
            for (pos, original) in before {
                let packed = original.light.as_ref().unwrap();
                // Original scalar unpack and literal defaults, independent of
                // the parallel preparation and its conversion helper.
                let mut sky = Vec::new(); let mut block = Vec::new();
                for value in &packed.values { sky.push(value >> 4); block.push(value & 15); }
                let reference = LightChunk { height: packed.height.clone(),
                    minimum: *packed.height.iter().min().unwrap_or(&0), sections: packed.sections,
                    sky: sky.into(), block: block.into(), gaps: vec![false; 256], queued_checks: 4096 };
                let adopted = &view.engine.chunks[&pos];
                assert!(adopted.sky.is_shared_packed() && adopted.block.is_shared_packed());
                assert_eq!(adopted.sky.capacity() + adopted.block.capacity(), 0);
                assert_eq!(serde_json::to_value(&view.engine.chunks[&pos]).unwrap(),
                    serde_json::to_value(reference).unwrap());
                assert_eq!(adopted.sky.capacity() + adopted.block.capacity(), 0);
                assert!(Arc::ptr_eq(&world.chunks[&pos], &original));
                assert!(same_light(&world.chunks[&pos].light, &original.light));
                assert!(Arc::ptr_eq(&view.seen[&pos], &original));
            }
        }
    }

    pub(super) fn lit_region() -> (World, LightingView) {
        let mut world = World {
            chunks: crate::world::ChunkMap::new(), spawn: glam::Vec3::ZERO, label: String::new(),
            save_path: Default::default(), edits: Default::default(), imported: false, light_revision: 0,
        };
        for x in -2..=2 { for z in -2..=2 {
            let mut chunk = Chunk::empty();
            for dx in 0..16 { for dz in 0..16 { chunk.set(dx,0,dz,7<<4); } }
            world.chunks.insert((x,z),Arc::new(chunk));
        } }
        let lighting = LightingView::new(&mut world);
        (world, lighting)
    }

    #[test]
    fn authoritative_light_adoption_shares_original_chunk_and_light_allocations() {
        let (mut world, _) = lit_region();
        let native = world.clone();
        let view = LightingView::new(&mut world);
        for (pos, original) in &native.chunks {
            let current = &world.chunks[pos];
            assert!(Arc::ptr_eq(original,current),"adoption copied native blocks at {pos:?}");
            assert!(same_light(&original.light,&current.light),"adoption copied native light at {pos:?}");
            assert!(Arc::ptr_eq(current,&view.seen[pos]));
            assert!(snapshot_matches(current.light.as_ref().unwrap(),&view.engine.chunks[pos],false));
        }
        let blocks:usize = native.chunks.values().map(|c| c.blocks.len()*2).sum();
        let light:usize = native.chunks.values().map(|c| c.light.as_ref().unwrap().values.len()).sum();
        eprintln!("Authoritative adoption shares {} chunk allocations: {blocks} block bytes and {light} light bytes",native.chunks.len());
    }

    #[test]
    fn equal_light_republication_preserves_arcs_and_dirty_notifications() {
        let (mut world, mut view) = lit_region();
        let before = world.clone();
        // A dirty engine can legitimately produce identical render bytes.
        // Consumers still receive dirty notifications, without new allocations.
        view.engine.invalidate_render_cache();
        world.invalidate_lighting();
        assert_eq!(view.update(&mut world).len(),before.chunks.len());
        for (pos, original) in &before.chunks {
            assert!(Arc::ptr_eq(original,&world.chunks[pos]),"equal result cloned {pos:?}");
            assert!(same_light(&original.light,&world.chunks[pos].light));
        }
        assert!(view.update(&mut world).is_empty());
    }

    #[test]
    fn adopted_snapshot_is_replaced_when_a_local_neighbor_changes_its_light() {
        let (mut world, mut view) = lit_region();
        let mut reference = world.clone();
        let mut reference_view = LightingView::new_eager_reference(&mut reference);
        let at = (1,0);
        let mut publication = (*world.chunks[&at]).clone();
        publication.light = Some(Arc::new((**publication.light.as_ref().unwrap()).clone()));
        let publication = Arc::new(publication);
        world.chunks.insert(at,publication.clone());
        world.invalidate_lighting();
        let source = IVec3::new(15,4,8);
        world.set(source,89<<4);
        reference.set(source,89<<4);
        assert!(view.update(&mut world).contains(&at));
        reference_view.update(&mut reference);
        assert_eq!(world.light_uv(source+IVec3::X,0)[0],14*16);
        assert_eq!(publication.light.as_ref().unwrap().get(crate::vanilla::lighting::Kind::Block,source+IVec3::X),0);
        assert!(!Arc::ptr_eq(&publication,&world.chunks[&at]));
        assert!(!same_light(&publication.light,&world.chunks[&at].light));
        for (pos, expected) in &reference.chunks {
            let actual = world.chunks[pos].light.as_ref().unwrap();
            let expected = expected.light.as_ref().unwrap();
            assert_eq!(actual.values,expected.values,"neighbor light differs at {pos:?}");
            assert_eq!(actual.height,expected.height);
            assert_eq!(actual.sections,expected.sections);
        }
    }

    #[test]
    fn native_light_only_publication_survives_a_later_local_edit() {
        use crate::vanilla::lighting::Kind;
        let mut chunk = Chunk::empty();
        for x in 0..16 {
            for z in 0..16 {
                chunk.set(x, 0, z, 7 << 4);
            }
        }
        let mut world = World {
            chunks: crate::world::ChunkMap::from([((0, 0), Arc::new(chunk))]),
            spawn: glam::Vec3::ZERO,
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: true,
            light_revision: 0,
        };
        let mut view = LightingView::new(&mut world);
        let light_only = IVec3::new(1, 4, 1);
        let mut native = view.engine.clone();
        native.set(&world, Kind::Block, light_only, 12);
        let blocks = world.chunks[&(0, 0)].blocks.clone();
        Arc::make_mut(world.chunks.get_mut(&(0, 0)).unwrap()).light =
            native.render_snapshot((0, 0));
        let native_publication = world.chunks[&(0,0)].clone();
        world.invalidate_lighting();
        view.update(&mut world);
        assert!(Arc::ptr_eq(&native_publication,&world.chunks[&(0,0)]));
        assert!(same_light(&native_publication.light,&world.chunks[&(0,0)].light));
        assert_eq!(world.chunks[&(0, 0)].blocks, blocks);
        assert_eq!(world.light_uv(light_only, 0)[0], 12 * 16);

        // Dirty the same chunk beyond the bounded local light-update radius.
        // Re-publication must retain the native value, even though its incoming
        // update contained no block delta from which to reconstruct that light.
        world.set(IVec3::new(14, 20, 14), 1 << 4);
        assert!(view.update(&mut world).contains(&(0, 0)));
        assert_eq!(world.light_uv(light_only, 0)[0], 12 * 16);
        assert!(view.update(&mut world).is_empty());
        // A publication overlaid with already-applied player edits has no new
        // block delta but deliberately discards the stale incoming snapshot.
        Arc::make_mut(world.chunks.get_mut(&(0, 0)).unwrap()).light = None;
        world.invalidate_lighting();
        assert!(view.update(&mut world).contains(&(0, 0)));
        assert_eq!(world.light_uv(light_only, 0)[0], 12 * 16);
    }
    #[test]
    fn placed_and_removed_light_updates_neighbor_mesh_snapshot() {
        let mut world = World {
            chunks: crate::world::ChunkMap::new(),
            spawn: glam::Vec3::ZERO,
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        };
        for x in -2..=2 {
            for z in -2..=2 {
                let mut chunk = Chunk::empty();
                for dx in 0..16 {
                    for dz in 0..16 {
                        chunk.set(dx, 0, dz, 7 << 4);
                    }
                }
                world.chunks.insert((x, z), Arc::new(chunk));
            }
        }
        let mut lighting = LightingView::new(&mut world);
        let old = world.clone();
        let source = IVec3::new(15, 4, 8);
        world.set(source, 89 << 4);
        assert!(lighting.update(&mut world).contains(&(1, 0)));
        assert_eq!(world.light_uv(source + IVec3::X, 0)[0], 14 * 16);
        assert_eq!(old.light_uv(source + IVec3::X, 0)[0], 0);
        let lit = world.clone();
        world.set(source, 0);
        assert!(lighting.update(&mut world).contains(&(1, 0)));
        assert_eq!(world.light_uv(source + IVec3::X, 0)[0], 0);
        assert_eq!(lit.light_uv(source + IVec3::X, 0)[0], 14 * 16);
        assert!(lighting.update(&mut world).is_empty());
        let revision = world.light_revision;
        let unchanged = Arc::clone(&world.chunks[&(0, 0)]);
        assert!(world.set(source, 0));
        assert!(world
            .integrate(vec![((0, 0), unchanged.clone())])
            .is_empty());
        world.unload_chunk((99, 99));
        assert_eq!(world.light_revision, revision);
        assert!(Arc::ptr_eq(&unchanged, &world.chunks[&(0, 0)]));
        assert!(lighting.update(&mut world).is_empty());

        // A move can remove one chunk and load another before the next frame,
        // leaving the map length unchanged. The new light inputs must still be seen.
        world.unload_chunk((-2, -2));
        assert_eq!(
            world.integrate(vec![((3, 0), Arc::new(Chunk::empty()))]),
            [(3, 0)]
        );
        lighting.update(&mut world);
        assert!(lighting.seen.contains_key(&(3, 0)));
        assert!(!lighting.seen.contains_key(&(-2, -2)));
        assert_eq!(lighting.last_revision, Some(world.light_revision));
    }

    #[test]
    fn streamed_unlit_chunks_relight_border_overhang_once_five_by_five_completes() {
        let make_floor = |roof: bool| {
            let mut chunk = Chunk::empty();
            for dx in 0..16 {
                for dz in 0..16 {
                    for y in 0..=63 {
                        chunk.set(dx, y, dz, 1 << 4);
                    }
                }
            }
            if roof {
                for dx in 0..=3 {
                    for dz in 6..=10 {
                        chunk.set(dx, 66, dz, 1 << 4);
                    }
                }
            }
            Arc::new(chunk)
        };
        let mut world = World {
            chunks: Default::default(),
            spawn: glam::Vec3::ZERO,
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: true,
            light_revision: 0,
        };
        for x in -1..=2 {
            for z in -2..=2 {
                world.chunks.insert((x, z), make_floor(x == 0 && z == 0));
            }
        }
        let mut lighting = LightingView::new(&mut world);
        let under = IVec3::new(1, 65, 8);
        assert_eq!(world.light_uv(under, 0)[1], 0);
        assert!(lighting.pending.contains(&(0, 0)));
        assert!(!lighting.pending.contains(&(1, 0)));
        assert!(lighting.unsettled.contains(&(0, 0)));
        assert!(lighting.unsettled.contains(&(1, 0)));

        let mut next_ring = Vec::new();
        for z in -2..=2 {
            next_ring.push(((-2, z), make_floor(false)));
            next_ring.push(((3, z), make_floor(false)));
        }
        world.integrate(next_ring);
        let changed = lighting.update(&mut world);
        assert!(changed.contains(&(0, 0)));
        assert_eq!(world.light_uv(under, 0)[1], 13 * 16);
        assert!(!lighting.pending.contains(&(0, 0)));
        assert!(!lighting.unsettled.contains(&(0, 0)));
        assert!(!lighting.unsettled.contains(&(1, 0)));
    }
}

#[cfg(test)]
#[path = "lighting_view_shared_tests.rs"]
mod shared_tests;
