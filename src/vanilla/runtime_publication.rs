//! Pure, bounded packing/index preparation; original sorted owner publication.
use super::*;
use rayon::prelude::*;
use std::time::Instant;

const PARALLEL_MINIMUM: usize = 64;
const PREPARED_LIMIT: usize = 32;

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct PublicationTimings {
    pub calls: u64,
    pub parallel_calls: u64,
    pub positions: u64,
    pub packed_columns: u64,
    pub prepared_random_indexes: u64,
    pub prepared_scheduled_indexes: u64,
    pub maximum_prepared_columns: usize,
    /// Owner wall time, including the scoped wait. Worker sums below overlap.
    pub preparation_wall_ms: f64,
    pub packing_worker_ms: f64,
    pub index_worker_ms: f64,
    pub installation_ms: f64,
}
impl PublicationTimings {
    fn merge(&mut self, other: Self) {
        self.calls += other.calls; self.parallel_calls += other.parallel_calls;
        self.positions += other.positions; self.packed_columns += other.packed_columns;
        self.prepared_random_indexes += other.prepared_random_indexes;
        self.prepared_scheduled_indexes += other.prepared_scheduled_indexes;
        self.maximum_prepared_columns = self.maximum_prepared_columns.max(other.maximum_prepared_columns);
        self.preparation_wall_ms += other.preparation_wall_ms;
        self.packing_worker_ms += other.packing_worker_ms;
        self.index_worker_ms += other.index_worker_ms;
        self.installation_ms += other.installation_ms;
    }
}
struct Prepared {
    position: ChunkPos,
    // None means no refresh. Some(None) retains the old missing-light behavior.
    light: Option<Option<Arc<super::super::lighting::ChunkLight>>>,
    indexes: Option<crate::world::PreparedTickIndexes>,
    packing_ms: f64,
    index_ms: f64,
}
fn elapsed(start: Instant) -> f64 { start.elapsed().as_secs_f64() * 1000. }

// Only future Cold columns need a storage-only owner promotion. The disabled
// compression policy means ordinary Dense publications retain their exact Arc
// identity/COW behavior. Unexpected retained Cold aliases detach safely.
fn prepare_block_storage(chunk: &mut Arc<crate::world::Chunk>) {
    if chunk.blocks.is_cold() { Arc::make_mut(chunk).blocks.promote_to_dense(); }
}

pub(super) fn take(runtime: &mut Runtime) -> Vec<(ChunkPos, Arc<crate::world::Chunk>)> {
    #[cfg(test)]
    if runtime.backend.reference_publication { return reference(runtime); }
    let light_dirty = runtime.backend.light.take_render_dirty().into_iter().collect::<HashSet<_>>();
    runtime.backend.dirty_chunks.extend(light_dirty.iter().copied());
    let mut positions = runtime.backend.dirty_chunks.drain().collect::<Vec<_>>();
    positions.sort_unstable();
    let mut timings = PublicationTimings { calls: 1, positions: positions.len() as u64, ..Default::default() };
    // Failure to create the already-existing pool keeps the original serial
    // path. No detached jobs, runtime captures, callbacks or missing updates.
    let pool = (positions.len() >= PARALLEL_MINIMUM).then(terrain_work::publication_pool)
        .and_then(std::result::Result::ok);
    let mut output = Vec::with_capacity(positions.len());
    if let Some(pool) = pool {
        timings.parallel_calls = 1;
        for batch in positions.chunks(PREPARED_LIMIT) {
            let promotion_started = Instant::now();
            // Promote on the owner before immutable worker preparation. This
            // avoids hydrating a retained old Cold Arc while scanning indexes.
            for position in batch {
                if let Some(chunk) = runtime.backend.world.chunks.get_mut(position) {
                    prepare_block_storage(chunk);
                }
            }
            timings.installation_ms += elapsed(promotion_started);
            let started = Instant::now();
            let chunks = &runtime.backend.world.chunks;
            let light = &runtime.backend.light;
            let mut prepared: Vec<_> = pool.install(|| batch.par_iter().map(|&position| {
                let refresh = light_dirty.contains(&position)
                    || chunks.get(&position).is_some_and(|c| c.light.is_none());
                let started = Instant::now();
                let packed = refresh.then(|| light.render_snapshot(position));
                let packing_ms = elapsed(started);
                let started = Instant::now();
                let indexes = chunks.get(&position).map(|chunk| chunk.prepare_tick_indexes_detached());
                Prepared { position, light: packed, indexes, packing_ms, index_ms: elapsed(started) }
            }).collect());
            timings.preparation_wall_ms += elapsed(started);
            timings.maximum_prepared_columns = timings.maximum_prepared_columns.max(prepared.len());
            let started = Instant::now();
            for prepared in &mut prepared {
                timings.packing_worker_ms += prepared.packing_ms;
                timings.index_worker_ms += prepared.index_ms;
                timings.packed_columns += u64::from(prepared.light.as_ref().is_some_and(Option::is_some));
                if let Some(indexes) = &prepared.indexes {
                    timings.prepared_random_indexes += u64::from(indexes.random.is_some());
                    timings.prepared_scheduled_indexes += u64::from(indexes.scheduled.is_some());
                }
                if let Some(chunk) = runtime.backend.world.chunks.get_mut(&prepared.position) {
                    if let Some(light) = prepared.light.take() { Arc::make_mut(chunk).light = light; }
                }
            }
            let chunks = &runtime.backend.world.chunks;
            pool.install(|| batch.par_iter().for_each(|&position| {
                if let Some(chunk) = chunks.get(&position) {
                    chunk.skull_positions();
                    chunk.sign_positions();
                    chunk.spawner_positions();
                    chunk.end_portal_positions();
                    chunk.portal_positions();
                }
            }));
            for prepared in prepared {
                if let Some(chunk) = runtime.backend.world.chunks.get(&prepared.position) {
                    chunk.install_prepared_tick_indexes(prepared.indexes.expect("prepared existing chunk"));
                    output.push((prepared.position, Arc::clone(chunk)));
                }
            }
            timings.installation_ms += elapsed(started);
        }
    } else {
        // Keep the original small-batch COW/cache/Arc order. Measure individual
        // kernels separately so dispatch benefit can be judged, not assumed.
        for position in positions {
            let refresh = light_dirty.contains(&position)
                || runtime.backend.world.chunks.get(&position).is_some_and(|c| c.light.is_none());
            let started = Instant::now();
            let light = refresh.then(|| runtime.backend.light.render_snapshot(position));
            let packing_ms = elapsed(started);
            timings.packing_worker_ms += packing_ms;
            timings.preparation_wall_ms += packing_ms;
            timings.packed_columns += u64::from(light.as_ref().is_some_and(Option::is_some));
            if let Some(chunk) = runtime.backend.world.chunks.get_mut(&position) {
                let started = Instant::now();
                if let Some(light) = light { Arc::make_mut(chunk).light = light; }
                prepare_block_storage(chunk);
                timings.installation_ms += elapsed(started);
                let started = Instant::now();
                timings.prepared_random_indexes += u64::from(chunk.random_tick_sections.get().is_none());
                timings.prepared_scheduled_indexes += u64::from(chunk.scheduled_tick_blocks.get().is_none());
                chunk.prepare_tick_indexes();
                let index_ms = elapsed(started);
                timings.index_worker_ms += index_ms;
                timings.preparation_wall_ms += index_ms;
                let started = Instant::now();
                output.push((position, Arc::clone(chunk)));
                timings.installation_ms += elapsed(started);
            }
        }
    }
    runtime.backend.profile.publication.merge(timings);
    output
}

// ORIGINAL_REFERENCE_BODY is supplied verbatim from runtime.rs by the draft
// generator, with only self.backend renamed to runtime.backend.
#[cfg(test)]
fn reference(runtime: &mut Runtime) -> Vec<(ChunkPos, Arc<crate::world::Chunk>)> {
        let light_dirty = runtime.backend.light.take_render_dirty()
            .into_iter().collect::<HashSet<_>>();
        runtime.backend.dirty_chunks.extend(light_dirty.iter().copied());
        let mut positions = runtime.backend.dirty_chunks.drain().collect::<Vec<_>>();
        positions.sort_unstable();
        positions
            .into_iter()
            .filter_map(|p| {
                // Explicit requests also republish unchanged retained chunks.
                // Keep both Arcs in that case: replacing an identical light
                // snapshot would copy 64 KiB of light and, while the renderer
                // holds its Arc, 128 KiB of otherwise unchanged block states.
                let refresh_light = light_dirty.contains(&p)
                    || runtime.backend.world.chunks.get(&p).is_some_and(|c| c.light.is_none());
                let light = refresh_light.then(|| runtime.backend.light.render_snapshot(p));
                runtime.backend.world.chunks.get_mut(&p).map(|chunk| {
                    if let Some(light) = light {
                        Arc::make_mut(chunk).light = light;
                    }
                    prepare_block_storage(chunk);
                    chunk.prepare_tick_indexes();
                    (p, Arc::clone(chunk))
                })
            })
            .collect()
    }

#[cfg(test)]
#[path = "runtime_publication_tests.rs"]
mod tests;
