//! Speculative work is limited to immutable density/surface/carver generation
//! and its initial per-column light arrays. Discovery, population, neighbor
//! lighting and publication remain on the owner in original provider order.
use super::*;
use rayon::prelude::*;
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{mpsc, OnceLock},
    time::Instant,
};

const LOOKAHEAD_LIMIT: usize = 64;
pub(super) type Prepared = (crate::world::Chunk, u64, Option<super::super::lighting::PreparedLight>);
pub(super) struct TerrainJob {
    receiver: mpsc::Receiver<Result<Prepared>>,
    // Dropping an owner cancels queued speculative work before it starts.
    // An already running pure calculation can finish without retaining Runtime.
    _alive: Arc<()>,
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct TerrainTimings {
    /// Wall time on the owner spent preparing its requested batch, including
    /// any wait for terrain already running on the bounded worker pool.
    pub prepare_ms: f64,
    /// Subset of owner time blocked on speculative results. Reentrant loads
    /// can wait outside prepare_ms, so this is not an additional phase total.
    pub lookahead_wait_ms: f64,
    pub lookahead_submitted: usize,
    pub lookahead_consumed: usize,
    pub lookahead_peak: usize,
    pub prepared_light_installed: usize,
    pub prepared_light_fallbacks: usize,
}

fn pool() -> Result<&'static rayon::ThreadPool> {
    static POOL: OnceLock<std::result::Result<rayon::ThreadPool, String>> = OnceLock::new();
    POOL.get_or_init(|| {
        let cores = std::thread::available_parallelism().map_or(1, usize::from);
        rayon::ThreadPoolBuilder::new()
            .num_threads(cores.saturating_sub(3).clamp(1, 8))
            .thread_name(|index| format!("native-terrain-{index}"))
            .start_handler(|index| crate::worker_priority::on_start(crate::worker_priority::Pool::NativeTerrain, index))
            .build()
            .map_err(|error| error.to_string())
    })
    .as_ref()
    .map_err(|error| anyhow::anyhow!("Start terrain workers: {error}"))
}

fn generate(terrain: &Terrain, biomes: &BiomeGenerator, pos: ChunkPos, light_mode: Option<bool>) -> Result<Prepared> {
    let mut random = JavaRandom::new(0);
    let chunk = terrain.generate_with_random(pos.0, pos.1, biomes, &mut random)?;
    let light = light_mode.map(|no_sky| super::super::lighting::PreparedLight::new(&chunk, no_sky));
    Ok((chunk, random.state(), light))
}

impl Runtime {
    pub fn terrain_timings(&self) -> TerrainTimings {
        self.backend.terrain_timings
    }

    /// At most 64 prepared/in-flight chunks are retained by lookahead. This
    /// does not provide a chunk, discover a structure or consume world RNG.
    pub fn prefetch_terrain(&mut self, positions: &[ChunkPos]) -> Result<()> {
        let backend = &mut self.backend;
        let light_mode = backend.prepared_light_mode();
        for &pos in positions {
            if backend.terrain_jobs.len() + backend.prepared_terrain.len() >= LOOKAHEAD_LIMIT {
                break;
            }
            if backend.world.chunks.contains_key(&pos)
                || backend.prepared_terrain.contains_key(&pos)
                || backend.terrain_jobs.contains_key(&pos)
            {
                continue;
            }
            let terrain = Arc::clone(&backend.terrain);
            let biomes = Arc::clone(&backend.biomes);
            let (sender, receiver) = mpsc::sync_channel(1);
            let alive = Arc::new(());
            let weak = Arc::downgrade(&alive);
            // FIFO gives the earliest original request priority. Workers own
            // only immutable generator inputs and one result, never Runtime.
            pool()?.spawn_fifo(move || {
                let Some(_alive) = weak.upgrade() else {
                    return;
                };
                let result = catch_unwind(AssertUnwindSafe(|| generate(&terrain, &biomes, pos, light_mode)))
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("Terrain worker panicked at {pos:?}")));
                // Runtime may be dropped with unused speculative work. Its
                // result is then discarded without touching live world state.
                let _ = sender.send(result);
            });
            backend.terrain_jobs.insert(
                pos,
                TerrainJob {
                    receiver,
                    _alive: alive,
                },
            );
            backend.terrain_timings.lookahead_submitted += 1;
            backend.terrain_timings.lookahead_peak = backend
                .terrain_timings
                .lookahead_peak
                .max(backend.terrain_jobs.len() + backend.prepared_terrain.len());
        }
        Ok(())
    }

    pub fn prepare_terrain(&mut self, positions: &[ChunkPos]) -> Result<()> {
        let begin = Instant::now();
        let result = (|| {
            // Receive in caller order. This has no publication side effects;
            // generate() below restores the matching end state only on use.
            for &pos in positions {
                if self.backend.world.chunks.contains_key(&pos)
                    || self.backend.prepared_terrain.contains_key(&pos)
                {
                    continue;
                }
                if let Some(prepared) = self.backend.take_terrain_job(pos)? {
                    self.backend.prepared_terrain.insert(pos, prepared);
                }
            }
            let mut missing = positions
                .iter()
                .copied()
                .filter(|p| {
                    !self.backend.world.chunks.contains_key(p)
                        && !self.backend.prepared_terrain.contains_key(p)
                })
                .collect::<Vec<_>>();
            missing.sort_unstable();
            missing.dedup();
            if !missing.is_empty() {
                let terrain = &self.backend.terrain;
                let biomes = &self.backend.biomes;
                let light_mode = self.backend.prepared_light_mode();
                let prepared = pool()?.install(|| {
                    missing
                        .par_iter()
                        .map(|&pos| Ok((pos, generate(terrain, biomes, pos, light_mode)?)))
                        .collect::<Result<Vec<_>>>()
                })?;
                self.backend.prepared_terrain.extend(prepared);
            }
            Ok(())
        })();
        self.backend.terrain_timings.prepare_ms += begin.elapsed().as_secs_f64() * 1000.;
        result
    }
}

impl RuntimeBackend {
    fn prepared_light_mode(&self) -> Option<bool> {
        #[cfg(test)]
        if self.reference_prepared_light { return None; }
        self.light.preparation_mode()
    }
    fn take_terrain_job(&mut self, pos: ChunkPos) -> Result<Option<Prepared>> {
        let Some(job) = self.terrain_jobs.remove(&pos) else {
            return Ok(None);
        };
        let begin = Instant::now();
        let result = job
            .receiver
            .recv()
            .map_err(|_| anyhow::anyhow!("Terrain worker disconnected at {pos:?}"))?;
        self.terrain_timings.lookahead_wait_ms += begin.elapsed().as_secs_f64() * 1000.;
        self.terrain_timings.lookahead_consumed += 1;
        result.map(Some)
    }

    pub(super) fn take_prepared_terrain(&mut self, pos: ChunkPos) -> Result<Option<Prepared>> {
        if let Some(prepared) = self.prepared_terrain.remove(&pos) {
            Ok(Some(prepared))
        } else {
            // A decorator may request a future chunk before the outer batch
            // reaches it. Consuming its pure result here keeps that reentrant
            // provider call and its RNG publication in the original position.
            self.take_terrain_job(pos)
        }
    }
}

#[cfg(test)]
#[path = "runtime_terrain_tests.rs"]
mod tests;

// Synchronous pure publication preparation reuses the bounded terrain pool.
pub(super) fn publication_pool() -> Result<&'static rayon::ThreadPool> { pool() }
