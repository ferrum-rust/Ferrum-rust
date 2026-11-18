//! Experimental bounded pure geometry lookahead. All real Random advancement
//! and every world callback stay on the caller. There is never an owner wait.
use super::{math, ore_bounds::Bounds, FeatureWorld, JavaRandom};
use glam::IVec3;
use std::{ops::Range, panic::{catch_unwind, AssertUnwindSafe},
    sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc}};

const VEINS: usize = 100;
const PACKET_VEINS: usize = 16;
const COORDINATE_LIMIT: usize = 32_768;
// Measured default packets stayed below9,000 positions. Reserve half the hard
// limit initially; exceptional larger packets may still grow to the same cap.
const INITIAL_COORDINATE_CAPACITY: usize = 16_384;
static ACTIVE: AtomicBool = AtomicBool::new(false);
struct Lease { busy: &'static AtomicBool }
impl Drop for Lease { fn drop(&mut self) { self.busy.store(false, Ordering::Release); } }
fn acquire_from(busy: &'static AtomicBool) -> Option<Arc<Lease>> {
    busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .ok().map(|_| Arc::new(Lease { busy }))
}
fn acquire() -> Option<Arc<Lease>> {
    #[cfg(test)]
    if TEST_ACTIVE.with(std::cell::Cell::get) { return None; }
    acquire_from(&ACTIVE)
}

#[cfg(test)]
thread_local! { static TEST_ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum TestMode { Disabled, Ready, Unavailable }
#[cfg(test)]
pub(crate) fn run_for_test(world: &mut impl FeatureWorld, random: &mut JavaRandom,
    origin: IVec3, mode: TestMode) -> Option<Stats> {
    if matches!(mode, TestMode::Disabled) { return None; }
    if TEST_ACTIVE.with(|active| active.replace(true)) { return None; }
    struct Guard;
    impl Drop for Guard { fn drop(&mut self) { TEST_ACTIVE.with(|active| active.set(false)); } }
    let _guard = Guard;
    struct Deterministic(TestMode);
    impl Executor for Deterministic {
        fn submit(&self, start: usize, random: JavaRandom, origin: IVec3,
            recycled: Option<Packet>) -> mpsc::Receiver<Option<Packet>> {
            let (tx, rx) = mpsc::sync_channel(1);
            if matches!(self.0, TestMode::Ready) {
                let _ = tx.try_send(prepare(start, random, origin, recycled));
            }
            rx
        }
    }
    Some(run(world, random, origin, &Deterministic(mode)))
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct Stats {
    pub submitted: usize, pub ready: usize, pub unavailable: usize,
    pub rng_rejections: usize, pub prepared_spheres: usize, pub inline_spheres: usize,
    pub maximum_packet_coordinates: usize,
}
impl Stats {
    pub(crate) fn add(&mut self, next: Self) {
        self.submitted += next.submitted; self.ready += next.ready;
        self.unavailable += next.unavailable; self.rng_rejections += next.rng_rejections;
        self.prepared_spheres += next.prepared_spheres; self.inline_spheres += next.inline_spheres;
        self.maximum_packet_coordinates = self.maximum_packet_coordinates.max(next.maximum_packet_coordinates);
    }
}
#[derive(Clone, Copy)]
struct Spec { ore: u16, size: i32, height: i32, lapis: bool }
fn spec(index: usize) -> Spec {
    let (ore, size, height) = match index {
        0..=9 => (3 << 4, 33, 256), 10..=17 => (13 << 4, 33, 256),
        18..=27 => ((1 << 4) | 3, 33, 80), 28..=37 => ((1 << 4) | 1, 33, 80),
        38..=47 => ((1 << 4) | 5, 33, 80), 48..=67 => (16 << 4, 17, 128),
        68..=87 => (15 << 4, 9, 64), 88..=89 => (14 << 4, 9, 32),
        90..=97 => (73 << 4, 8, 16), 98 => (56 << 4, 8, 16),
        99 => (21 << 4, 7, 16), _ => unreachable!(),
    };
    Spec { ore, size, height, lapis: index == 99 }
}
fn position(random: &mut JavaRandom, origin: IVec3, spec: Spec) -> IVec3 {
    let x = random.next_int(16);
    let y = if spec.lapis { random.next_int(16) + random.next_int(16) }
        else { random.next_int(spec.height) };
    let z = random.next_int(16);
    origin + IVec3::new(x, y, z)
}
struct Path { a: [f64; 3], b: [f64; 3], size: i32, bounds: Bounds }
impl Path {
    fn new(random: &mut JavaRandom, p: IVec3, size: i32) -> Self {
        let angle = random.next_float() * std::f32::consts::PI;
        let a = [
            (p.x.wrapping_add(8) as f32 + math::sin(angle) * size as f32 / 8.) as f64,
            (p.y + random.next_int(3) - 2) as f64,
            (p.z.wrapping_add(8) as f32 + math::cos(angle) * size as f32 / 8.) as f64,
        ];
        let b = [
            (p.x.wrapping_add(8) as f32 - math::sin(angle) * size as f32 / 8.) as f64,
            (p.y + random.next_int(3) - 2) as f64,
            (p.z.wrapping_add(8) as f32 - math::cos(angle) * size as f32 / 8.) as f64,
        ];
        Self { a, b, size, bounds: Bounds::new() }
    }
    fn sphere(&self, step: i32, draw: f64) -> ([f64; 3], f64) {
        let fraction = step as f32 / self.size as f32;
        let center = std::array::from_fn(|i| self.a[i] + (self.b[i] - self.a[i]) * fraction as f64);
        let radius_random = draw * self.size as f64 / 16.;
        let radius = ((math::sin(std::f32::consts::PI * fraction) + 1.) as f64 * radius_random + 1.) / 2.;
        (center, radius)
    }
}
struct Scratch { ys: [f64; 16], zs: [f64; 16], batch: [IVec3; 64] }
impl Default for Scratch {
    fn default() -> Self { Self { ys: [0.; 16], zs: [0.; 16], batch: [IVec3::ZERO; 64] } }
}
// Independent cached arithmetic here keeps the established minable_in intact.
// This experimental adapter only accepts the fixed size<=33 default ore table.
fn sphere(path: &Path, step: i32, draw: f64, scratch: &mut Scratch,
    mut emit: impl FnMut(&[IVec3]) -> bool) -> bool {
    let (center, radius) = path.sphere(step, draw);
    let (low, high) = path.bounds.get(center, radius);
    let y_len = i64::from(high[1]) - i64::from(low[1]) + 1;
    let z_len = i64::from(high[2]) - i64::from(low[2]) + 1;
    assert!((1..=16).contains(&y_len) && (1..=16).contains(&z_len));
    for (offset, value) in scratch.ys[..y_len as usize].iter_mut().enumerate() {
        let dy = ((low[1] + offset as i32) as f64 + 0.5 - center[1]) / radius;
        *value = dy * dy;
    }
    for (offset, value) in scratch.zs[..z_len as usize].iter_mut().enumerate() {
        let dz = ((low[2] + offset as i32) as f64 + 0.5 - center[2]) / radius;
        *value = dz * dz;
    }
    let mut count = 0;
    for x in low[0]..=high[0] {
        let dx = (x as f64 + 0.5 - center[0]) / radius;
        if dx * dx >= 1. { continue; }
        for (iy, &dy) in scratch.ys[..y_len as usize].iter().enumerate() {
            let xy = dx * dx + dy;
            if xy >= 1. { continue; }
            for (iz, &dz) in scratch.zs[..z_len as usize].iter().enumerate() {
                if xy + dz < 1. {
                    scratch.batch[count] = IVec3::new(x, low[1] + iy as i32, low[2] + iz as i32);
                    count += 1;
                    if count == 64 {
                        if !emit(&scratch.batch) { return false; }
                        count = 0;
                    }
                }
            }
        }
    }
    count == 0 || emit(&scratch.batch[..count])
}
struct PreparedSphere { before: u64, positions: Range<usize> }
struct PreparedVein { before: u64, spheres: Range<usize> }
struct Packet {
    start: usize, before: u64, positions: Vec<IVec3>,
    spheres: Vec<PreparedSphere>, veins: Vec<PreparedVein>,
}
impl Default for Packet {
    fn default() -> Self {
        Self { start: 0, before: 0, positions: Vec::with_capacity(INITIAL_COORDINATE_CAPACITY),
            spheres: Vec::with_capacity(PACKET_VEINS * 33), veins: Vec::with_capacity(PACKET_VEINS) }
    }
}
fn prepare(start: usize, random: JavaRandom, origin: IVec3, recycled: Option<Packet>) -> Option<Packet> {
    prepare_bounded(start, random, origin, recycled, COORDINATE_LIMIT)
}
fn prepare_bounded(start: usize, mut random: JavaRandom, origin: IVec3, recycled: Option<Packet>, limit: usize) -> Option<Packet> {
    assert!(limit <= COORDINATE_LIMIT);
    let mut packet = recycled.unwrap_or_default();
    packet.start = start; packet.before = random.state();
    packet.positions.clear(); packet.spheres.clear(); packet.veins.clear();
    let mut scratch = Scratch::default();
    for index in start..(start + PACKET_VEINS).min(VEINS) {
        let before = random.state(); let spec = spec(index);
        let p = position(&mut random, origin, spec); let path = Path::new(&mut random, p, spec.size);
        let begin = packet.spheres.len();
        for step in 0..spec.size {
            let before = random.state(); let draw = random.next_double();
            let begin = packet.positions.len();
            if !sphere(&path, step, draw, &mut scratch, |positions| {
                if positions.len() > limit - packet.positions.len() { return false; }
                packet.positions.extend_from_slice(positions); true
            }) { return None; }
            packet.spheres.push(PreparedSphere { before, positions: begin..packet.positions.len() });
        }
        packet.veins.push(PreparedVein { before, spheres: begin..packet.spheres.len() });
    }
    Some(packet)
}
fn predict_after(start: usize, random: &JavaRandom, origin: IVec3) -> JavaRandom {
    let mut random = random.clone(); // Owned snapshot; never a shared handle.
    for index in start..(start + PACKET_VEINS).min(VEINS) {
        let spec = spec(index); let _ = position(&mut random, origin, spec);
        let _ = random.next_float(); let _ = random.next_int(3); let _ = random.next_int(3);
        for _ in 0..spec.size { let _ = random.next_double(); }
    }
    random
}
trait Executor {
    fn submit(&self, start: usize, random: JavaRandom, origin: IVec3,
        recycled: Option<Packet>) -> mpsc::Receiver<Option<Packet>>;
}
struct Pool<'a> { pool: &'a rayon::ThreadPool, lease: Arc<Lease> }
impl Executor for Pool<'_> {
    fn submit(&self, start: usize, random: JavaRandom, origin: IVec3,
        recycled: Option<Packet>) -> mpsc::Receiver<Option<Packet>> {
        let (tx, rx) = mpsc::sync_channel(1); let lease = Arc::clone(&self.lease);
        self.pool.spawn_fifo(move || {
            let _lease = lease; // In-flight work keeps the global bound after owner cancellation.
            let result = catch_unwind(AssertUnwindSafe(|| prepare(start, random, origin, recycled)))
                .ok().flatten();
            // One result, no worker wait and no retained world/RNG handle.
            let _ = tx.try_send(result);
        });
        rx
    }
}
// None means no work began; caller executes the unchanged baseline.
pub(crate) fn try_run(world: &mut impl FeatureWorld, random: &mut JavaRandom,
    origin: IVec3, pool: &rayon::ThreadPool) -> Option<Stats> {
    if origin.y != 0 || !(-30_000_000..30_000_000).contains(&origin.x)
        || !(-30_000_000..30_000_000).contains(&origin.z) { return None; }
    let executor = Pool { pool, lease: acquire()? };
    // Do the pure one-time initialization before launching work; a descheduled
    // speculative worker must not own a OnceLock that the owner first needs.
    let _ = math::sin(0.); let _ = Bounds::new();
    Some(run(world, random, origin, &executor))
}
fn run(world: &mut impl FeatureWorld, random: &mut JavaRandom, origin: IVec3,
    executor: &impl Executor) -> Stats {
    let mut stats = Stats::default(); let mut active = true;
    let mut pending: Option<mpsc::Receiver<Option<Packet>>> = None;
    let mut recycled = None; let mut scratch = Scratch::default();
    for start in (0..VEINS).step_by(PACKET_VEINS) {
        let mut current = None;
        if start != 0 && active {
            match pending.take().and_then(|rx| rx.try_recv().ok()).flatten() {
                Some(packet) if packet.start == start && packet.before == random.state() => {
                    stats.ready += 1; stats.maximum_packet_coordinates = stats.maximum_packet_coordinates.max(packet.positions.len());
                    current = Some(packet);
                }
                Some(_) => { stats.rng_rejections += 1; active = false; }
                None => { stats.unavailable += 1; active = false; }
            }
        }
        let next = start + PACKET_VEINS;
        if active && next < VEINS {
            pending = Some(executor.submit(next, predict_after(start, random, origin), origin, recycled.take()));
            stats.submitted += 1;
        }
        for index in start..next.min(VEINS) {
            let prepared = current.as_ref().map(|p| &p.veins[index - start]);
            if active && prepared.is_some_and(|p| p.before != random.state()) {
                stats.rng_rejections += 1; active = false;
            }
            let spec = spec(index); let p = position(random, origin, spec);
            let path = Path::new(random, p, spec.size);
            for step in 0..spec.size {
                let candidate = prepared.map(|v| &current.as_ref().unwrap().spheres[v.spheres.start + step as usize]);
                if active && candidate.is_some_and(|s| s.before != random.state()) {
                    stats.rng_rejections += 1; active = false;
                }
                // This real draw remains before all original live callbacks.
                let draw = random.next_double();
                if let Some(candidate) = candidate.filter(|_| active) {
                    for positions in current.as_ref().unwrap().positions[candidate.positions.clone()].chunks(64) {
                        world.replace_ore_line(positions, spec.ore, 1);
                    }
                    stats.prepared_spheres += 1;
                } else {
                    assert!(sphere(&path, step, draw, &mut scratch, |positions| {
                        world.replace_ore_line(positions, spec.ore, 1); true
                    }));
                    stats.inline_spheres += 1;
                }
            }
        }
        if let Some(packet) = current { recycled = Some(packet); }
        if !active { pending = None; }
    }
    stats
}
#[cfg(test)]
#[path = "ore_lookahead_tests.rs"]
mod tests;
