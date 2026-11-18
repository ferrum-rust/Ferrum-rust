//! Bounded owner-only lossless block storage; never unloads a provider column.
//! This policy runs only after the parent's successful Generated publication.
//! Submission-time hints may be stale during a command: owner publication still
//! promotes Cold blocks before exposing any new client/mesh/LOD Arc.
use super::{Context, Runtime};
use crate::world::ChunkPos;
use std::{sync::Arc, time::{Duration, Instant}};

const STATES: usize = 65536;
const RAW_BYTES: usize = STATES * std::mem::size_of::<u16>();
const SCAN_LIMIT: usize = 64;

#[derive(Clone, Copy)]
struct Limits {
    scans: usize, encodes: usize, input_bytes: usize, cache_releases: usize,
    release_bytes: usize, elapsed: Duration,
}
impl Limits {
    fn initial() -> Self {
        // Actual Arc probe: checkpoint medians .076-.130 ms, max .214 ms
        // per128 KiB encode. Keep conservative count/input and soft time caps.
        Self { scans: SCAN_LIMIT, encodes: 8, input_bytes: 1024 * 1024,
            cache_releases: 4, release_bytes: 1024 * 1024,
            elapsed: Duration::from_millis(1) }
    }
}
#[derive(Default, Clone, Copy, Debug, serde::Serialize)]
pub struct Metrics {
    pub passes: u64, pub visited: u64, pub protected: u64, pub dirty_or_missing: u64,
    pub shared: u64, pub unprepared: u64, pub cooling: u64, pub recent_cold_accesses: u64,
    pub encode_attempts: u64, pub encode_input_bytes: u64, pub compressed_columns: u64,
    pub unprofitable: u64, pub raw_bytes_released: u64, pub encoded_payload_bytes_created: u64,
    pub cache_releases: u64, pub cache_bytes_released: u64,
    pub encode_ms: f64, pub max_encode_ms: f64, pub pass_ms: f64, pub max_pass_ms: f64,
    pub max_visited: usize, pub max_encode_attempts: usize, pub max_encode_input_bytes: usize,
    pub max_cache_releases: usize, pub max_release_bytes_budgeted: usize,
}
#[derive(Default)]
struct Work {
    visited: usize, protected: usize, dirty: usize, shared: usize, unprepared: usize,
    cooling: usize, recent: usize, encodes: usize, input_bytes: usize,
    cache_releases: usize, release_bytes_budgeted: usize, outcome: Outcome,
}
#[derive(Default)]
struct Outcome {
    compressed: usize, unprofitable: usize, raw_released: usize, encoded_created: usize,
    cache_released: usize, shared: usize, encode_ms: f64, max_encode_ms: f64,
}
impl Outcome {
    fn merge(&mut self, other: Self) {
        self.compressed += other.compressed; self.unprofitable += other.unprofitable;
        self.raw_released += other.raw_released; self.encoded_created += other.encoded_created;
        self.cache_released += other.cache_released; self.shared += other.shared;
        self.encode_ms += other.encode_ms; self.max_encode_ms = self.max_encode_ms.max(other.max_encode_ms);
    }
}
impl Metrics {
    fn add(&mut self, work: Work, elapsed: Duration) {
        self.passes += 1; self.visited += work.visited as u64; self.protected += work.protected as u64;
        self.dirty_or_missing += work.dirty as u64; self.shared += (work.shared + work.outcome.shared) as u64;
        self.unprepared += work.unprepared as u64; self.cooling += work.cooling as u64;
        self.recent_cold_accesses += work.recent as u64; self.encode_attempts += work.encodes as u64;
        self.encode_input_bytes += work.input_bytes as u64; self.cache_releases += work.cache_releases as u64;
        self.max_visited = self.max_visited.max(work.visited);
        self.max_encode_attempts = self.max_encode_attempts.max(work.encodes);
        self.max_encode_input_bytes = self.max_encode_input_bytes.max(work.input_bytes);
        self.max_cache_releases = self.max_cache_releases.max(work.cache_releases);
        self.max_release_bytes_budgeted = self.max_release_bytes_budgeted.max(work.release_bytes_budgeted);
        let o = work.outcome;
        self.compressed_columns += o.compressed as u64; self.unprofitable += o.unprofitable as u64;
        self.raw_bytes_released += o.raw_released as u64; self.encoded_payload_bytes_created += o.encoded_created as u64;
        self.cache_bytes_released += o.cache_released as u64;
        self.encode_ms += o.encode_ms; self.max_encode_ms = self.max_encode_ms.max(o.max_encode_ms);
        let ms = elapsed.as_secs_f64() * 1000.; self.pass_ms += ms; self.max_pass_ms = self.max_pass_ms.max(ms);
    }
}

pub(super) struct Policy {
    limits: Option<Limits>, cursor: usize, cooldown_generations: u64,
    minimum_saved_bytes: usize, metrics: Metrics,
}
impl Default for Policy {
    fn default() -> Self {
        Self { limits: Some(Limits::initial()), cursor: 0, cooldown_generations: 32,
            minimum_saved_bytes: 4096, metrics: Metrics::default() }
    }
}
impl Policy {
    pub(super) fn metrics(&self) -> Metrics { self.metrics }
    #[cfg(test)]
    pub(super) fn prepare_atomic_save_oracle(&mut self) {
        self.limits = Some(Limits { elapsed: Duration::from_secs(5), ..Limits::initial() });
        // This fixture verifies storage/save isolation. Separate normal tests
        // verify the unchanged production32-boundary read/promotion cooldown.
        self.cooldown_generations = 0;
    }
    pub(super) fn after_publication(&mut self, runtime: &mut Runtime, context: &Context, generation: u64) {
        let Some(limits) = self.limits else { return; };
        let start = Instant::now();
        let work = pass(&mut RuntimeStore { runtime, generation, cooldown: self.cooldown_generations,
            minimum_saved_bytes: self.minimum_saved_bytes }, &mut self.cursor, context, limits);
        self.metrics.add(work, start.elapsed());
    }
}

#[derive(Clone, Copy)]
enum Plan { Encode { release_bytes: usize }, CacheRelease(usize), Skip }
enum Admission { Ready(Plan), DirtyOrMissing, Shared, Unprepared, Cooling { recent: usize } }
trait Store {
    fn len(&self) -> usize;
    fn position(&self, index: usize) -> ChunkPos;
    fn admit(&mut self, position: ChunkPos) -> Admission;
    fn execute(&mut self, position: ChunkPos, plan: Plan) -> Outcome;
}
struct RuntimeStore<'a> { runtime: &'a mut Runtime, generation: u64, cooldown: u64, minimum_saved_bytes: usize }
impl Store for RuntimeStore<'_> {
    fn len(&self) -> usize { self.runtime.provider.loaded_order().len() }
    fn position(&self, index: usize) -> ChunkPos { self.runtime.provider.loaded_order()[index] }
    fn admit(&mut self, p: ChunkPos) -> Admission {
        let b = &mut self.runtime.backend;
        if b.dirty_chunks.contains(&p) || b.light.render_is_dirty(p) || !b.light.chunks.contains_key(&p) {
            return Admission::DirtyOrMissing;
        }
        let Some(owner) = b.world.chunks.get_mut(&p) else { return Admission::DirtyOrMissing; };
        // The only admission authority: rejects both strong and Weak aliases.
        // Never make_mut here; compression must not allocate a duplicate Chunk.
        let Some(chunk) = Arc::get_mut(owner) else { return Admission::Shared; };
        if chunk.blocks.len() != STATES || chunk.random_tick_sections.get().is_none()
            || chunk.scheduled_tick_blocks.get().is_none() { return Admission::Unprepared; }
        let blocks = &mut chunk.blocks;
        let recent = usize::from(blocks.take_recent_cold_access());
        if recent != 0 { blocks.set_next_eligible_generation(self.generation.saturating_add(self.cooldown)); }
        if blocks.next_eligible_generation() > self.generation { return Admission::Cooling { recent }; }
        Admission::Ready(if blocks.is_dense() {
            Plan::Encode { release_bytes: blocks.capacity().saturating_mul(std::mem::size_of::<u16>()) }
        } else if blocks.has_read_cache() {
            Plan::CacheRelease(blocks.capacity().saturating_mul(std::mem::size_of::<u16>()))
        } else { Plan::Skip })
    }
    fn execute(&mut self, p: ChunkPos, plan: Plan) -> Outcome {
        let mut result = Outcome::default();
        // No callback/RNG or mutable owner operation intervenes after admit.
        // Still reacquire the exact exclusive proof instead of trusting counts.
        let Some(chunk) = self.runtime.backend.world.chunks.get_mut(&p).and_then(Arc::get_mut)
            else { result.shared = 1; return result; };
        let blocks = &mut chunk.blocks;
        match plan {
            Plan::Encode { .. } => {
                let before = blocks.capacity() * std::mem::size_of::<u16>();
                let started = Instant::now();
                if blocks.compress_if_saving(self.minimum_saved_bytes) {
                    result.compressed = 1;
                    result.raw_released = before.saturating_sub(blocks.capacity() * std::mem::size_of::<u16>());
                    result.encoded_created = blocks.encoded_payload_bytes();
                } else { result.unprofitable = 1; }
                result.encode_ms = started.elapsed().as_secs_f64() * 1000.; result.max_encode_ms = result.encode_ms;
                // Avoid repeatedly encoding unchanged incompressible storage.
                blocks.set_next_eligible_generation(self.generation.saturating_add(self.cooldown));
            }
            Plan::CacheRelease(_) => { result.cache_released = blocks.release_read_cache(); }
            Plan::Skip => {}
        }
        result
    }
}

fn pass(store: &mut impl Store, cursor: &mut usize, context: &Context, limits: Limits) -> Work {
    let mut work = Work::default(); let len = store.len();
    if len == 0 || limits.elapsed.is_zero() || (limits.encodes == 0 && limits.cache_releases == 0) { return work; }
    if *cursor >= len { *cursor = 0; }
    let started = Instant::now();
    for _ in 0..limits.scans.min(SCAN_LIMIT).min(len) {
        if started.elapsed() >= limits.elapsed { break; }
        let p = store.position(*cursor); work.visited += 1;
        // Advance rejected/completed candidates; preserve a budget-blocked
        // candidate so repeated small passes cannot starve it behind the head.
        let mut advance = || *cursor = (*cursor + 1) % len;
        if context.protects(p) { work.protected += 1; advance(); continue; }
        let plan = match store.admit(p) {
            Admission::DirtyOrMissing => { work.dirty += 1; advance(); continue; }
            Admission::Shared => { work.shared += 1; advance(); continue; }
            Admission::Unprepared => { work.unprepared += 1; advance(); continue; }
            Admission::Cooling { recent } => { work.cooling += 1; work.recent += recent; advance(); continue; }
            Admission::Ready(plan) => plan,
        };
        match plan {
            Plan::Encode { release_bytes } => {
                if limits.encodes == 0 || RAW_BYTES > limits.input_bytes || release_bytes > limits.release_bytes {
                    advance(); continue;
                }
                if work.encodes >= limits.encodes || RAW_BYTES > limits.input_bytes - work.input_bytes
                    || release_bytes > limits.release_bytes - work.release_bytes_budgeted { break; }
                work.encodes += 1; work.input_bytes += RAW_BYTES; work.release_bytes_budgeted += release_bytes;
            }
            Plan::CacheRelease(bytes) => {
                if limits.cache_releases == 0 || bytes > limits.release_bytes { advance(); continue; }
                if work.cache_releases >= limits.cache_releases || bytes > limits.release_bytes - work.release_bytes_budgeted { break; }
                work.cache_releases += 1; work.release_bytes_budgeted += bytes;
            }
            Plan::Skip => { advance(); continue; }
        }
        // One codec/drop cannot be preempted. A pass has a soft wall bound plus
        // explicit scan/input/release/count bounds, not a hard real-time promise.
        work.outcome.merge(store.execute(p, plan)); advance();
    }
    work
}

#[cfg(test)]
#[path = "native_session_block_maintenance_tests.rs"]
mod tests;
