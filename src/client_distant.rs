//! Bounded visual-only distant terrain. Authoritative surfaces are captured
//! before chunk eviction, but these jobs never modify blocks or world RNG.
use super::*;
use crate::distant_terrain::{
    DistantTerrain, Surface, TerrainTile, TilePos, GRID, STEP, TILE_CHUNKS,
};
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{OnceLock, Weak},
};

const IN_FLIGHT: usize = 2;
const MAX_CAPTURED_CHUNKS: usize = 51 * 51 + IN_FLIGHT * 64;
type Samples = [Surface; 16];

fn workers() -> Result<&'static rayon::ThreadPool> {
    static POOL: OnceLock<std::result::Result<rayon::ThreadPool, String>> = OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(IN_FLIGHT)
            .thread_name(|i| format!("distant-surface-{i}"))
            .start_handler(|index| crate::worker_priority::on_start(crate::worker_priority::Pool::Distant, index))
            .build()
            .map_err(|error| error.to_string())
    })
    .as_ref()
    .map_err(|error| anyhow::anyhow!("Create distant terrain workers: {error}"))
}

#[derive(Clone)]
enum CaptureData {
    Chunk(Arc<Chunk>),
    Samples(Samples),
}
#[derive(Clone)]
struct Capture {
    stamp: u64,
    data: CaptureData,
}
struct Work {
    position: TilePos,
    epoch: u64,
    version: u64,
    stride: u8,
    base: Option<Arc<TerrainTile>>,
    uploaded: Option<Arc<TerrainTile>>,
    uploaded_stride: Option<u8>,
    captures: Vec<(ChunkPos, Capture)>,
}
struct Finished {
    tile: Arc<TerrainTile>,
    mesh: Option<Mesh>,
    stamps: Vec<(ChunkPos, u64)>,
}
struct Completed {
    position: TilePos,
    epoch: u64,
    version: u64,
    stride: u8,
    result: Result<Finished>,
    milliseconds: f64,
}

fn guarded_work(position: TilePos, action: impl FnOnce() -> Result<Finished>) -> Result<Finished> {
    catch_unwind(AssertUnwindSafe(action)).unwrap_or_else(|_| {
        Err(anyhow::anyhow!(
            "Distant terrain worker panicked at {position:?}"
        ))
    })
}

pub(super) struct DistantScene {
    source: DistantTerrain,
    assets: Option<Arc<Assets>>,
    center: Option<(ChunkPos, u32)>,
    wanted: HashSet<TilePos>,
    // Last uploaded and last processed surfaces can differ while a newer
    // capture is pending. Comparing against uploaded avoids losing that edit
    // when the following job has no additional surface changes of its own.
    resident: HashMap<TilePos, Arc<TerrainTile>>,
    resident_strides: HashMap<TilePos, u8>,
    cached: HashMap<TilePos, Arc<TerrainTile>>,
    pending: HashMap<TilePos, (u64, u64)>,
    captures: HashMap<ChunkPos, Capture>,
    observed: HashMap<ChunkPos, Weak<Chunk>>,
    observed_revision: Option<u64>,
    versions: HashMap<TilePos, u64>,
    dirty: HashSet<TilePos>,
    full_captures: usize,
    compacted_on_frame: u64,
    next_stamp: u64,
    order: Vec<TilePos>,
    epoch: u64,
    sender: mpsc::Sender<Completed>,
    receiver: mpsc::Receiver<Completed>,
    alive: Arc<()>,
    completed_jobs: u64,
    unchanged_jobs: u64,
    stale_jobs: u64,
    total_job_ms: f64,
    max_job_ms: f64,
    error: Option<String>,
}

fn tile_of(p: ChunkPos) -> TilePos {
    (p.0.div_euclid(TILE_CHUNKS), p.1.div_euclid(TILE_CHUNKS))
}
fn mesh_stride(center: ChunkPos, tile: TilePos) -> u8 {
    // Use the nearest chunk in a tile, keeping every tile that intersects a
    // closer band at that band's finer density. i64 also handles world edges.
    let distance = |camera:i32, tile:i32| {
        let low=i64::from(tile)*i64::from(TILE_CHUNKS);
        let high=low+i64::from(TILE_CHUNKS)-1;
        (low-i64::from(camera)).max(i64::from(camera)-high).max(0)
    };
    match distance(center.0,tile.0).max(distance(center.1,tile.1)) {
        0..=64 => 4,
        65..=96 => 8,
        _ => 16,
    }
}
fn tiles(center: ChunkPos, distance: u32) -> Vec<TilePos> {
    if distance <= 24 {
        return Vec::new();
    }
    let distance = distance.min(128) as i64;
    // Clip the visual request before dispatch. One invalid boundary tile must
    // not turn all otherwise valid distant terrain into a failed scene.
    let low = (
        ((i64::from(center.0) - distance).div_euclid(8)).max(-234375),
        ((i64::from(center.1) - distance).div_euclid(8)).max(-234375),
    );
    let high = (
        ((i64::from(center.0) + distance).div_euclid(8)).min(234374),
        ((i64::from(center.1) + distance).div_euclid(8)).min(234374),
    );
    let mut result = (low.0..=high.0)
        .flat_map(|x| (low.1..=high.1).map(move |z| (x as i32, z as i32)))
        .collect::<Vec<_>>();
    result.sort_unstable_by_key(|&(x, z)| {
        let dx = i64::from(x) * 8 + 4 - i64::from(center.0);
        let dz = i64::from(z) * 8 + 4 - i64::from(center.1);
        (dx * dx + dz * dz, x, z)
    });
    result
}

fn samples(chunk: &Chunk) -> Samples {
    std::array::from_fn(|i| {
        crate::distant_terrain::surface(
            chunk,
            (i % 4) as i32 * STEP + STEP / 2,
            (i / 4) as i32 * STEP + STEP / 2,
        )
    })
}
fn apply_samples(tile: &mut TerrainTile, p: ChunkPos, samples: &Samples) {
    let cx = p.0 - tile.position.0 * TILE_CHUNKS;
    let cz = p.1 - tile.position.1 * TILE_CHUNKS;
    if !(0..TILE_CHUNKS).contains(&cx) || !(0..TILE_CHUNKS).contains(&cz) {
        return;
    }
    for z in 0..4 {
        for x in 0..4 {
            tile.heights[(cz as usize * 4 + z) * GRID + cx as usize * 4 + x] = samples[z * 4 + x];
        }
    }
}

impl Work {
    fn finish(self, mut tile: TerrainTile, build: impl FnOnce(&TerrainTile) -> Mesh) -> Finished {
        debug_assert_eq!(tile.position, self.position);
        let mut stamps = Vec::with_capacity(self.captures.len());
        for (p, capture) in self.captures {
            let surface = match capture.data {
                CaptureData::Chunk(chunk) => samples(&chunk),
                CaptureData::Samples(s) => s,
            };
            apply_samples(&mut tile, p, &surface);
            stamps.push((p, capture.stamp));
        }
        let tile = self
            .base
            .filter(|old| old.as_ref() == &tile)
            .unwrap_or_else(|| Arc::new(tile));
        let unchanged = self.uploaded_stride == Some(self.stride) && self
            .uploaded
            .as_ref()
            .is_some_and(|old| old.as_ref() == tile.as_ref());
        let mesh = (!unchanged).then(|| build(&tile));
        Finished { tile, mesh, stamps }
    }
}

impl DistantScene {
    pub(super) fn new(source: DistantTerrain) -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            source,
            assets: None,
            center: None,
            wanted: HashSet::new(),
            resident: HashMap::new(),
            resident_strides: HashMap::new(),
            cached: HashMap::new(),
            pending: HashMap::new(),
            captures: HashMap::new(),
            observed: HashMap::new(),
            observed_revision: None,
            versions: HashMap::new(),
            dirty: HashSet::new(),
            full_captures: 0,
            compacted_on_frame: 0,
            next_stamp: 0,
            order: Vec::new(),
            epoch: 0,
            sender,
            receiver,
            alive: Arc::new(()),
            completed_jobs: 0,
            unchanged_jobs: 0,
            stale_jobs: 0,
            total_job_ms: 0.,
            max_job_ms: 0.,
            error: None,
        }
    }

    /// Called immediately before World::unload_chunk. Usually only retains the
    /// latest immutable Arc; extraction and meshing stay on the two workers.
    pub(super) fn capture_chunk(&mut self, world: &World, p: ChunkPos) {
        if self.wanted.is_empty() {
            return;
        }
        let tile = tile_of(p);
        if !self.wanted.contains(&tile) {
            return;
        }
        let Some(chunk) = world.chunks.get(&p) else {
            return;
        };
        if self
            .observed
            .get(&p)
            .is_some_and(|old| old.as_ptr() == Arc::as_ptr(chunk))
        {
            return;
        }
        self.observed.insert(p, Arc::downgrade(chunk));
        self.next_stamp = self.next_stamp.wrapping_add(1);
        let stamp = self.next_stamp;
        let replacing_full = self
            .captures
            .get(&p)
            .is_some_and(|old| matches!(old.data, CaptureData::Chunk(_)));
        let data = if self.full_captures < MAX_CAPTURED_CHUNKS || replacing_full {
            if !replacing_full {
                self.full_captures += 1;
            }
            CaptureData::Chunk(chunk.clone())
        } else {
            // Exceptional fast traversal must not pin tens of thousands of
            // full chunks. Compact this single column set, not the whole world.
            self.compacted_on_frame += 1;
            CaptureData::Samples(samples(chunk))
        };
        self.captures.insert(p, Capture { stamp, data });
        self.versions.insert(tile, stamp);
        self.dirty.insert(tile);
    }

    pub(super) fn observe(&mut self, world: &World) {
        if self.wanted.is_empty() {
            return;
        }
        if self.observed_revision == Some(world.light_revision) {
            return;
        }
        self.observed_revision = Some(world.light_revision);
        self.observed
            .retain(|p, _| world.chunks.contains_key(p) && self.wanted.contains(&tile_of(*p)));
        for &p in world.chunks.keys() {
            self.capture_chunk(world, p);
        }
    }

    fn reset_pack(&mut self) -> Vec<TilePos> {
        self.epoch = self.epoch.wrapping_add(1);
        self.error = None;
        self.dirty.extend(self.cached.keys().copied());
        self.resident_strides.clear();
        self.resident.drain().map(|(p, _)| p).collect()
    }

    fn set_view(&mut self, center: ChunkPos, radius: u32) -> Vec<TilePos> {
        if self.center == Some((center, radius)) {
            return Vec::new();
        }
        self.center = Some((center, radius));
        self.order = tiles(center, radius);
        self.wanted = self.order.iter().copied().collect();
        let removed = self
            .resident
            .keys()
            .filter(|p| !self.wanted.contains(p))
            .copied()
            .collect::<Vec<_>>();
        self.resident.retain(|p, _| self.wanted.contains(p));
        self.resident_strides.retain(|p, _| self.wanted.contains(p));
        self.cached.retain(|p, _| self.wanted.contains(p));
        self.captures
            .retain(|p, _| self.wanted.contains(&tile_of(*p)));
        self.observed
            .retain(|p, _| self.wanted.contains(&tile_of(*p)));
        self.versions.retain(|p, _| self.wanted.contains(p));
        self.dirty.retain(|p| self.wanted.contains(p));
        for (&p,&stride) in &self.resident_strides {
            if stride != mesh_stride(center,p) { self.dirty.insert(p); }
        }
        self.full_captures = self
            .captures
            .values()
            .filter(|c| matches!(c.data, CaptureData::Chunk(_)))
            .count();
        self.observed_revision = None;
        removed
    }

    fn suspend(&mut self, center: ChunkPos, radius: u32) -> Vec<TilePos> {
        let removed = self.set_view(center, radius);
        self.assets = None;
        self.error = None;
        self.observed_revision = None;
        // At most the two former jobs can be outstanding. Discard their
        // results without reintroducing captures or scanning the near world.
        for _ in 0..IN_FLIGHT {
            let Ok(done) = self.receiver.try_recv() else {
                break;
            };
            self.completed(done);
        }
        removed
    }

    fn work(&self, position: TilePos) -> Work {
        let captures = (0..TILE_CHUNKS)
            .flat_map(|x| {
                (0..TILE_CHUNKS)
                    .map(move |z| (position.0 * TILE_CHUNKS + x, position.1 * TILE_CHUNKS + z))
            })
            .filter_map(|p| self.captures.get(&p).map(|c| (p, c.clone())))
            .collect();
        Work {
            position,
            epoch: self.epoch,
            version: self.versions.get(&position).copied().unwrap_or(0),
            stride: mesh_stride(self.center.expect("Active distant view").0,position),
            base: self.cached.get(&position).cloned(),
            uploaded: self.resident.get(&position).cloned(),
            uploaded_stride: self.resident_strides.get(&position).copied(),
            captures,
        }
    }

    fn completed(&mut self, done: Completed) -> Option<(TilePos, Mesh)> {
        if self.pending.get(&done.position) != Some(&(done.epoch, done.version)) {
            return None;
        }
        self.pending.remove(&done.position);
        self.completed_jobs += 1;
        self.total_job_ms += done.milliseconds;
        self.max_job_ms = self.max_job_ms.max(done.milliseconds);
        if !self.wanted.contains(&done.position) {
            return None;
        }
        match done.result {
            Ok(finished) => {
                // The processed result is still useful after a newer capture.
                // Retain it as a base, but acknowledge only exact old stamps.
                self.cached.insert(done.position, finished.tile.clone());
                for (p, stamp) in finished.stamps {
                    if self
                        .captures
                        .get(&p)
                        .is_some_and(|capture| capture.stamp == stamp)
                    {
                        if let Some(Capture {
                            data: CaptureData::Chunk(_),
                            ..
                        }) = self.captures.remove(&p)
                        {
                            self.full_captures -= 1;
                        }
                    }
                }
                if done.epoch != self.epoch
                    || done.version != self.versions.get(&done.position).copied().unwrap_or(0)
                    || done.stride != mesh_stride(self.center.expect("Active distant view").0,done.position)
                {
                    self.stale_jobs += 1;
                    self.dirty.insert(done.position);
                    return None;
                }
                self.dirty.remove(&done.position);
                self.resident.insert(done.position, finished.tile);
                self.resident_strides.insert(done.position,done.stride);
                if let Some(mesh) = finished.mesh {
                    Some((done.position, mesh))
                } else {
                    self.unchanged_jobs += 1;
                    None
                }
            }
            Err(error) => {
                if done.epoch == self.epoch {
                    self.error = Some(format!("Distant terrain {:?}: {error:#}", done.position));
                }
                None
            }
        }
    }

    /// Keep one job making progress in each class when both are available.
    /// The existing order is nearest-first within each class, and old epochs or
    /// teleported-away jobs still occupy the same global two-job budget.
    fn next_position(&self) -> Option<TilePos> {
        if self.error.is_some() || self.pending.len() >= IN_FLIGHT
            || (self.dirty.is_empty() && self.resident.len() == self.wanted.len()) {
            return None;
        }
        let missing_in_flight = self.pending.iter().any(|(p, (epoch, _))|
            *epoch == self.epoch && self.wanted.contains(p) && !self.resident.contains_key(p));
        let candidate = |missing| self.order.iter().copied().find(|p|
            !self.pending.contains_key(p) && if missing {
                !self.resident.contains_key(p)
            } else {
                self.resident.contains_key(p) && self.dirty.contains(p)
            });
        candidate(!missing_in_flight).or_else(|| candidate(missing_in_flight))
    }

    fn schedule(&mut self, assets: &Arc<Assets>) -> Result<()> {
        // No new worker or upload budget: reserve progress for missing tiles
        // without starving edits/stride refreshes when either class stays busy.
        while let Some(p) = self.next_position() {
            let work = self.work(p);
            let source = self.source.clone();
            let assets = assets.clone();
            let sender = self.sender.clone();
            let epoch = work.epoch;
            let version = work.version;
            let stride = work.stride;
            let alive = Arc::downgrade(&self.alive);
            let pool = workers()?;
            self.pending.insert(p, (epoch, version));
            pool.spawn(move || {
                let Some(_alive) = alive.upgrade() else {
                    return;
                };
                let started = Instant::now();
                let result = guarded_work(p, || {
                    let tile = if let Some(base) = &work.base {
                        base.as_ref().clone()
                    } else if work.captures.len() == 64 {
                        TerrainTile {
                            position: p,
                            heights: vec![Default::default(); GRID * GRID],
                        }
                    } else {
                        source.tile(p)?
                    };
                    Ok(work.finish(tile, |tile| {
                        crate::renderer::distant_render::build_with_stride(tile, &assets, stride)
                    }))
                });
                let _ = sender.send(Completed {
                    position: p,
                    epoch,
                    version,
                    stride,
                    result,
                    milliseconds: started.elapsed().as_secs_f64() * 1000.,
                });
            });
        }
        Ok(())
    }

    pub(super) fn notify_renderer_cleared(&mut self) {
        self.resident_strides.clear();
        self.resident.clear();
        self.observed_revision = None;
    }

    pub(super) fn pause_non_overworld(&mut self, renderer: &mut Renderer) {
        self.resident_strides.clear();
        for p in self.resident.drain().map(|(p, _)| p) {
            renderer.remove_distant(p);
        }
        self.observed_revision = None;
        while let Ok(done) = self.receiver.try_recv() {
            if let Some((position, _mesh)) = self.completed(done) {
                self.resident.remove(&position);
                self.resident_strides.remove(&position);
            }
        }
    }

    pub(super) fn update(
        &mut self,
        world: &World,
        center: ChunkPos,
        radius: u32,
        assets: &Arc<Assets>,
        renderer: &mut Renderer,
    ) -> Result<()> {
        if radius <= 24 {
            for p in self.suspend(center, radius) {
                renderer.remove_distant(p);
            }
            return Ok(());
        }
        if self
            .assets
            .as_ref()
            .is_none_or(|old| !Arc::ptr_eq(old, assets))
        {
            for p in self.reset_pack() {
                renderer.remove_distant(p);
            }
            self.assets = Some(assets.clone());
        }
        for p in self.set_view(center, radius) {
            renderer.remove_distant(p);
        }
        self.observe(world);
        let detail = radius.min(24) as i32;
        renderer.set_distant_detail_bounds(
            (center.0 - detail, center.1 - detail),
            (center.0 + detail + 1, center.1 + detail + 1),
        );
        // At most one upload/frame and two outstanding/result jobs total.
        if let Ok(done) = self.receiver.try_recv() {
            if let Some((position, mesh)) = self.completed(done) {
                renderer.upload_distant(position, mesh);
            }
        }
        self.schedule(assets)
    }

    pub(super) fn ready(&self) -> bool {
        self.center.is_some()
            && self.error.is_none()
            && self.dirty.is_empty()
            && self.resident.len() == self.wanted.len()
    }
    pub(super) fn progress(&self) -> (usize, usize) { (self.resident.len(), self.wanted.len()) }
    pub(super) fn report(&self) -> serde_json::Value {
        serde_json::json!({"requested_tiles":self.wanted.len(),"resident_tiles":self.resident.len(),
            "cached_surface_tiles":self.cached.len(),"captured_chunks":self.captures.len(),
            "retained_full_chunk_captures":self.full_captures,"max_full_chunk_captures":MAX_CAPTURED_CHUNKS,
            "frame_compaction_fallbacks":self.compacted_on_frame,"dirty_tiles":self.dirty.len(),
            "unchanged_surface_jobs":self.unchanged_jobs,"rejected_stale_jobs":self.stale_jobs,
            "in_flight_jobs":self.pending.len(),"max_in_flight_jobs":IN_FLIGHT,
            "ready":self.ready(),"completed_jobs":self.completed_jobs,
            "total_background_job_ms":self.total_job_ms,"max_background_job_ms":self.max_job_ms,
            "error":self.error,"tile_chunks":TILE_CHUNKS,"surface_sample_blocks":STEP,
            "mesh_stride_blocks_tiles":([4u8,8,16].map(|stride| (stride,self.resident_strides.values().filter(|&&s|s==stride).count()))),
            "note":"Distant surfaces are visual LOD, not authoritative populated world chunks."})
    }
}

impl Game {
    pub(super) fn update_distant(
        &mut self,
        renderer: &mut Renderer,
        assets: &Arc<Assets>,
        settings: &Settings,
    ) -> Result<()> {
        let [x, _, z] = self.player.precise_position();
        let center = (
            (x.floor() as i32).div_euclid(16),
            (z.floor() as i32).div_euclid(16),
        );
        if let Some(scene) = &mut self.distant {
            if self.sim.dimension != 0 {
                scene.pause_non_overworld(renderer);
                return Ok(());
            }
            scene.update(
                &self.world,
                center,
                settings.render_distance,
                assets,
                renderer,
            )?;
            if self.options.benchmark_distant_ready {
                if let Some(error) = &scene.error {
                    anyhow::bail!("{error}");
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "client_distant_tests.rs"]
mod tests;
