//! Private, bounded lossless-storage maintenance. Report-only enabled draft.
//! Hints are submission-time snapshots: movement during an in-flight command can
//! only update the next submission. Never use a stamped hint as current authority.
use super::{Command, Runtime};
use crate::world::ChunkPos;
use std::time::{Duration, Instant};

#[path = "native_session_block_maintenance.rs"]
pub(crate) mod blocks;

const LIGHT_HALO: i64 = 2;
const SCAN_LIMIT: usize = 64;
const ARRAY_BYTES: usize = 16 * 256 * 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Window { min_x: i32, max_x: i32, min_z: i32, max_z: i32 }
impl Window {
    fn around(center: ChunkPos, radius: i32) -> Option<Self> {
        if radius < 0 { return None; }
        Some(Self {
            min_x: center.0.checked_sub(radius)?, max_x: center.0.checked_add(radius)?,
            min_z: center.1.checked_sub(radius)?, max_z: center.1.checked_add(radius)?,
        })
    }
    fn include(&mut self, p: ChunkPos) {
        self.min_x = self.min_x.min(p.0); self.max_x = self.max_x.max(p.0);
        self.min_z = self.min_z.min(p.1); self.max_z = self.max_z.max(p.1);
    }
    fn protects(self, p: ChunkPos) -> bool {
        let (x, z) = (i64::from(p.0), i64::from(p.1));
        x >= i64::from(self.min_x) - LIGHT_HALO && x <= i64::from(self.max_x) + LIGHT_HALO
            && z >= i64::from(self.min_z) - LIGHT_HALO && z <= i64::from(self.max_z) + LIGHT_HALO
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ViewHint {
    desired: Window, committed: Window, first_play_ready: bool, epoch: u64,
}
#[derive(Default)]
pub(crate) struct ViewState {
    desired: Option<Window>, committed: Option<Window>, first_play_ready: bool, epoch: u64,
}
impl ViewState {
    pub(crate) fn desired(&mut self, center: ChunkPos, radius: i32) {
        let next = Window::around(center, radius);
        if self.desired != next { self.desired = next; self.epoch = self.epoch.wrapping_add(1); }
    }
    pub(crate) fn commit(&mut self, center: ChunkPos, radius: i32) {
        let next = Window::around(center, radius);
        if self.committed != next { self.committed = next; self.epoch = self.epoch.wrapping_add(1); }
    }
    pub(crate) fn include_requests(&mut self, positions: &[ChunkPos]) {
        if let Some(window) = &mut self.committed {
            let before = *window;
            for &p in positions { window.include(p); }
            if *window != before { self.epoch = self.epoch.wrapping_add(1); }
        }
    }
    pub(crate) fn first_play_presented(&mut self) -> bool {
        let changed = !self.first_play_ready; self.first_play_ready = true; changed
    }
    pub(crate) fn hint(&self) -> Option<ViewHint> {
        Some(ViewHint { desired: self.desired?, committed: self.committed?,
            first_play_ready: self.first_play_ready, epoch: self.epoch })
    }
}

pub(super) struct Context { view: ViewHint, targets: Vec<ChunkPos> }
impl Context {
    pub(super) fn capture(command: &Command, hint: Option<ViewHint>, enabled: bool,
        fresh_spawn_publication: bool) -> Option<Self> {
        // The disabled production path allocates nothing and never touches Runtime.
        if !enabled || fresh_spawn_publication { return None; }
        let view = hint.filter(|v| v.first_play_ready)?;
        let (positions, ahead): (&[ChunkPos], &[ChunkPos]) = match command {
            Command::Generate(positions) => (positions, &[]),
            Command::GenerateAhead { positions, ahead } => (positions, ahead),
            _ => return None,
        };
        if positions.len() > 256 || ahead.len() > 64 { return None; }
        let mut targets = Vec::with_capacity(positions.len() + ahead.len());
        targets.extend_from_slice(positions); targets.extend_from_slice(ahead);
        Some(Self { view, targets })
    }
    fn protects(&self, p: ChunkPos) -> bool {
        self.view.desired.protects(p) || self.view.committed.protects(p)
            || self.targets.iter().any(|t| (i64::from(t.0) - i64::from(p.0)).abs() <= LIGHT_HALO
                && (i64::from(t.1) - i64::from(p.1)).abs() <= LIGHT_HALO)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Stage { #[default] Sky, Block, Packed }
#[derive(Default, Debug, PartialEq, Eq)]
pub(super) struct Cursor { index: usize, stage: Stage }
impl Cursor {
    fn advance(&mut self, len: usize) { self.index = (self.index + 1) % len; self.stage = Stage::Sky; }
    fn finish_stage(&mut self, len: usize) -> bool {
        match self.stage {
            Stage::Sky => { self.stage = Stage::Block; false }
            Stage::Block => { self.stage = Stage::Packed; false }
            Stage::Packed => { self.advance(len); true }
        }
    }
}
#[derive(Clone, Copy)]
struct Limits {
    scans: usize, encodes: usize, input_bytes: usize, cache_releases: usize,
    packed_attempts: usize, release_bytes: usize, elapsed: Duration,
}
impl Limits {
    fn measured_initial() -> Self {
        // Ninth optimized probe: Simple encode mean .021 ms, max .037 ms per
        // 64 KiB array. These are private initial caps, not a hard time guarantee.
        Self { scans: 64, encodes: 96, input_bytes: 6 * 1024 * 1024,
            cache_releases: 32, packed_attempts: 64, release_bytes: 8 * 1024 * 1024,
            elapsed: Duration::from_millis(2) }
    }
}
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct Metrics {
    /// Independent bounded native block storage; included in the same next-publication report.
    pub block_storage: blocks::Metrics,
    pub passes: u64, pub visited: u64, pub protected: u64, pub dirty_or_missing: u64,
    pub cooldown: u64, pub recent_cold_accesses: u64, pub encode_attempts: u64,
    pub encode_input_bytes: u64, pub compressed_arrays: u64, pub uniform_arrays: u64,
    pub rle_arrays: u64, pub unprofitable_arrays: u64, pub codec_errors: u64,
    pub released_raw_bytes: u64, pub encoded_payload_bytes_created: u64,
    pub cache_releases: u64, pub released_cache_bytes: u64, pub packed_attempts: u64,
    pub packed_references_removed: u64, pub packed_reference_bytes_removed: u64,
    pub packed_payload_bytes_proven_freed: u64, pub packed_shared: u64,
    pub encode_ms: f64, pub max_encode_ms: f64, pub pass_ms: f64, pub max_pass_ms: f64,
    pub elapsed_since_first_pass_ms: f64, pub max_visited: usize, pub max_encode_attempts: usize,
    pub max_encode_input_bytes: usize, pub max_cache_releases: usize, pub max_packed_attempts: usize,
    pub max_release_bytes_budgeted: usize,
}
#[derive(Default, Debug)]
struct Work {
    visited: usize, protected: usize, dirty: usize, cooldown: usize, recent: usize,
    encodes: usize, input_bytes: usize, cache_releases: usize, packed_attempts: usize,
    release_bytes_budgeted: usize, outcome: Outcome,
}
#[derive(Default, Debug)]
struct Outcome {
    compressed: usize, uniform: usize, rle: usize, unprofitable: usize, errors: usize,
    raw_released: usize, encoded_retained: usize, cache_released: usize,
    packed_removed: usize, packed_bytes: usize, packed_freed: usize, packed_shared: usize,
    encode_ms: f64, max_encode_ms: f64,
}
impl Outcome {
    fn merge(&mut self, b: Self) {
        self.compressed += b.compressed; self.uniform += b.uniform; self.rle += b.rle;
        self.unprofitable += b.unprofitable; self.errors += b.errors; self.raw_released += b.raw_released;
        self.encoded_retained += b.encoded_retained; self.cache_released += b.cache_released;
        self.packed_removed += b.packed_removed; self.packed_bytes += b.packed_bytes;
        self.packed_freed += b.packed_freed; self.packed_shared += b.packed_shared;
        self.encode_ms += b.encode_ms; self.max_encode_ms = self.max_encode_ms.max(b.max_encode_ms);
    }
}
impl Metrics {
    fn add(&mut self, work: Work, elapsed: Duration) {
        self.passes += 1; self.visited += work.visited as u64; self.protected += work.protected as u64;
        self.dirty_or_missing += work.dirty as u64; self.cooldown += work.cooldown as u64;
        self.recent_cold_accesses += work.recent as u64; self.encode_attempts += work.encodes as u64;
        self.encode_input_bytes += work.input_bytes as u64; self.cache_releases += work.cache_releases as u64;
        self.packed_attempts += work.packed_attempts as u64;
        self.max_visited = self.max_visited.max(work.visited);
        self.max_encode_attempts = self.max_encode_attempts.max(work.encodes);
        self.max_encode_input_bytes = self.max_encode_input_bytes.max(work.input_bytes);
        self.max_cache_releases = self.max_cache_releases.max(work.cache_releases);
        self.max_packed_attempts = self.max_packed_attempts.max(work.packed_attempts);
        self.max_release_bytes_budgeted = self.max_release_bytes_budgeted.max(work.release_bytes_budgeted);
        let o = work.outcome;
        self.compressed_arrays += o.compressed as u64; self.uniform_arrays += o.uniform as u64;
        self.rle_arrays += o.rle as u64; self.unprofitable_arrays += o.unprofitable as u64;
        self.codec_errors += o.errors as u64; self.released_raw_bytes += o.raw_released as u64;
        self.encoded_payload_bytes_created += o.encoded_retained as u64;
        self.released_cache_bytes += o.cache_released as u64;
        self.packed_references_removed += o.packed_removed as u64;
        self.packed_reference_bytes_removed += o.packed_bytes as u64;
        self.packed_payload_bytes_proven_freed += o.packed_freed as u64;
        self.packed_shared += o.packed_shared as u64;
        self.encode_ms += o.encode_ms; self.max_encode_ms = self.max_encode_ms.max(o.max_encode_ms);
        let ms = elapsed.as_secs_f64() * 1000.; self.pass_ms += ms; self.max_pass_ms = self.max_pass_ms.max(ms);
    }
}

pub(super) struct Policy {
    limits: Option<Limits>, generation: u64, cooldown_generations: u64,
    minimum_saved_bytes: usize, first_pass: Option<Instant>, metrics: Metrics, blocks: blocks::Policy,
}
impl Default for Policy {
    fn default() -> Self { Self { limits: Some(Limits::measured_initial()), generation: 0,
        cooldown_generations: 32, minimum_saved_bytes: 1024, first_pass: None, metrics: Metrics::default(), blocks: blocks::Policy::default() } }
}
impl Policy {
    pub(super) fn enabled(&self) -> bool { self.limits.is_some() }
    pub(super) fn metrics(&self) -> Metrics { self.metrics }
    pub(super) fn after_publication(&mut self, runtime: &mut Runtime, cursor: &mut Cursor, context: Option<Context>) {
        let (Some(limits), Some(context)) = (self.limits, context) else { return; };
        let start = Instant::now();
        let first = *self.first_pass.get_or_insert(start);
        self.generation = self.generation.saturating_add(1);
        let work = pass(&mut RuntimeStore { runtime, generation: self.generation,
            cooldown: self.cooldown_generations, minimum_saved_bytes: self.minimum_saved_bytes }, cursor, &context, limits);
        self.metrics.add(work, start.elapsed());
        // A block pass with a1 ms soft budget follows the existing2 ms
        // light budget. Each may overrun by one codec/drop or scheduling delay.
        // No new command, tick, callback or event.
        self.blocks.after_publication(runtime, &context, self.generation);
        self.metrics.block_storage = self.blocks.metrics();
        self.metrics.elapsed_since_first_pass_ms = first.elapsed().as_secs_f64() * 1000.;
    }
}
pub(super) fn successful(context: Option<Context>, reply: &super::Reply) -> Option<Context> {
    context.filter(|_| matches!(reply, super::Reply::Generated))
}

#[derive(Clone, Copy)]
enum Eligibility { Ready, DirtyOrMissing, Cooling { recent: usize } }
#[derive(Clone, Copy)]
enum Plan { Skip, Encode { input_bytes: usize, release_capacity: usize }, CacheRelease(usize), PackedRelease(usize) }
trait Store {
    fn len(&self) -> usize;
    fn position(&self, index: usize) -> ChunkPos;
    fn eligibility(&mut self, p: ChunkPos) -> Eligibility;
    fn plan(&self, p: ChunkPos, stage: Stage) -> Plan;
    fn execute(&mut self, p: ChunkPos, stage: Stage, plan: Plan) -> Outcome;
}
struct RuntimeStore<'a> { runtime: &'a mut Runtime, generation: u64, cooldown: u64, minimum_saved_bytes: usize }
impl Store for RuntimeStore<'_> {
    fn len(&self) -> usize { self.runtime.provider.loaded_order().len() }
    fn position(&self, index: usize) -> ChunkPos { self.runtime.provider.loaded_order()[index] }
    fn eligibility(&mut self, p: ChunkPos) -> Eligibility {
        let b = &mut self.runtime.backend;
        if !b.world.chunks.contains_key(&p) || b.dirty_chunks.contains(&p) || b.light.render_is_dirty(p) {
            return Eligibility::DirtyOrMissing;
        }
        let Some(light) = b.light.chunks.get_mut(&p) else { return Eligibility::DirtyOrMissing; };
        let mut recent = 0; let mut cooling = false;
        for bytes in [&mut light.sky, &mut light.block] {
            if bytes.take_recent_cold_access() {
                recent += 1;
                bytes.set_next_eligible_generation(self.generation.saturating_add(self.cooldown));
            }
            cooling |= bytes.next_eligible_generation() > self.generation;
        }
        if cooling { Eligibility::Cooling { recent } } else { Eligibility::Ready }
    }
    fn plan(&self, p: ChunkPos, stage: Stage) -> Plan {
        let b = &self.runtime.backend;
        if stage == Stage::Packed {
            return b.world.chunks.get(&p).and_then(|c| c.light.as_ref())
                .map_or(Plan::Skip, |c| Plan::PackedRelease(c.values.capacity()
                    + c.height.capacity() * std::mem::size_of::<i32>()));
        }
        let Some(c) = b.light.chunks.get(&p) else { return Plan::Skip; };
        let bytes = match stage { Stage::Sky => &c.sky, Stage::Block => &c.block, Stage::Packed => unreachable!() };
        if bytes.len() != ARRAY_BYTES { return Plan::Skip; }
        if bytes.is_dense() { Plan::Encode { input_bytes: bytes.len(), release_capacity: bytes.capacity() } }
        else if bytes.is_cold() && bytes.has_read_cache() { Plan::CacheRelease(bytes.capacity()) }
        else { Plan::Skip }
    }
    fn execute(&mut self, p: ChunkPos, stage: Stage, plan: Plan) -> Outcome {
        let mut result = Outcome::default();
        if let Plan::PackedRelease(_) = plan {
            let Some(chunk) = self.runtime.backend.world.chunks.get_mut(&p).and_then(std::sync::Arc::get_mut)
                else { result.packed_shared = 1; return result; };
            if let Some(light) = chunk.light.as_mut() {
                let bytes = light.values.capacity() + light.height.capacity() * std::mem::size_of::<i32>();
                result.packed_removed = 1; result.packed_bytes = bytes;
                // Strong-count alone is insufficient: another Weak can upgrade.
                // Actual get_mut proves both strong and Weak uniqueness here.
                if std::sync::Arc::get_mut(light).is_some() { result.packed_freed = bytes; }
            }
            chunk.light = None;
            return result;
        }
        let Some(light) = self.runtime.backend.light.chunks.get_mut(&p) else { return result; };
        let bytes = match stage { Stage::Sky => &mut light.sky, Stage::Block => &mut light.block, Stage::Packed => unreachable!() };
        match plan {
            Plan::Encode { .. } => {
                let before = bytes.capacity(); let start = Instant::now();
                match bytes.compress_if_saving(self.minimum_saved_bytes) {
                    Ok(true) => {
                        result.compressed = 1; result.raw_released = before.saturating_sub(bytes.capacity());
                        result.encoded_retained = bytes.encoded_bytes();
                        if bytes.encoded_capacity_bytes() == 0 { result.uniform = 1; } else { result.rle = 1; }
                    }
                    Ok(false) => result.unprofitable = 1,
                    Err(_) => result.errors = 1, // keep Dense; maintenance cannot fail gameplay
                }
                result.encode_ms = start.elapsed().as_secs_f64() * 1000.; result.max_encode_ms = result.encode_ms;
            }
            Plan::CacheRelease(_) => result.cache_released = bytes.release_read_cache(),
            Plan::Skip | Plan::PackedRelease(_) => {}
        }
        result
    }
}

fn pass(store: &mut impl Store, cursor: &mut Cursor, context: &Context, limits: Limits) -> Work {
    let mut work = Work::default(); let len = store.len();
    if len == 0 || limits.elapsed.is_zero() || (limits.encodes == 0 && limits.cache_releases == 0 && limits.packed_attempts == 0) {
        return work;
    }
    if cursor.index >= len { cursor.index = 0; cursor.stage = Stage::Sky; }
    let started = Instant::now();
    for _ in 0..limits.scans.min(SCAN_LIMIT).min(len) {
        if started.elapsed() >= limits.elapsed { break; }
        let p = store.position(cursor.index); work.visited += 1;
        if context.protects(p) { work.protected += 1; cursor.advance(len); continue; }
        match store.eligibility(p) {
            Eligibility::DirtyOrMissing => { work.dirty += 1; cursor.advance(len); continue; }
            Eligibility::Cooling { recent } => { work.cooldown += 1; work.recent += recent; cursor.advance(len); continue; }
            Eligibility::Ready => {}
        }
        loop {
            // Cooperative bound: one codec/drop and OS scheduling cannot be preempted.
            if started.elapsed() >= limits.elapsed { return work; }
            let plan = store.plan(p, cursor.stage);
            match plan {
                Plan::Skip => {}
                Plan::Encode { input_bytes, release_capacity } => {
                    if limits.encodes == 0 || input_bytes > limits.input_bytes || release_capacity > limits.release_bytes {
                        if cursor.finish_stage(len) { break; } continue;
                    }
                    if work.encodes >= limits.encodes || input_bytes > limits.input_bytes - work.input_bytes
                        || release_capacity > limits.release_bytes - work.release_bytes_budgeted { return work; }
                    work.encodes += 1; work.input_bytes += input_bytes;
                    // Compression can free Dense storage. Reserve its capacity
                    // too; an unprofitable encode conservatively spends this cap.
                    work.release_bytes_budgeted += release_capacity;
                }
                Plan::CacheRelease(bytes) => {
                    if limits.cache_releases == 0 || bytes > limits.release_bytes { if cursor.finish_stage(len) { break; } continue; }
                    if work.cache_releases >= limits.cache_releases || bytes > limits.release_bytes - work.release_bytes_budgeted { return work; }
                    work.cache_releases += 1; work.release_bytes_budgeted += bytes;
                }
                Plan::PackedRelease(bytes) => {
                    if limits.packed_attempts == 0 || bytes > limits.release_bytes { if cursor.finish_stage(len) { break; } continue; }
                    if work.packed_attempts >= limits.packed_attempts || bytes > limits.release_bytes - work.release_bytes_budgeted { return work; }
                    work.packed_attempts += 1; work.release_bytes_budgeted += bytes;
                }
            }
            if !matches!(plan, Plan::Skip) { work.outcome.merge(store.execute(p, cursor.stage, plan)); }
            // Completion advances even when encoding stayed Dense or an Arc was
            // shared. Budget exhaustion retains the exact Sky/Block/Packed step.
            if cursor.finish_stage(len) { break; }
        }
    }
    work
}

#[cfg(test)]
#[path = "native_session_maintenance_tests.rs"]
mod tests;
