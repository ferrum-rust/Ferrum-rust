use super::*;
use crate::native_session::{Command, Worker, Update};
use crate::vanilla::runtime::{Access, Hooks};
use crate::vanilla::random::JavaRandom;

fn context() -> Context {
    let mut view = super::super::ViewState::default();
    view.desired((1000, 1000), 25); view.commit((1000, 1000), 25); view.first_play_presented();
    Context::capture(&Command::Generate(vec![]), view.hint(), true, false).unwrap()
}
fn limits(encodes: usize) -> Limits {
    Limits { scans: 64, encodes, input_bytes: encodes * RAW_BYTES,
        cache_releases: 0, release_bytes: encodes * RAW_BYTES, elapsed: Duration::from_secs(5) }
}
#[derive(Default)]
struct Fake { order: Vec<ChunkPos>, attempted: Vec<ChunkPos>, cache: bool }
impl Store for Fake {
    fn len(&self) -> usize { self.order.len() }
    fn position(&self, index: usize) -> ChunkPos { self.order[index] }
    fn admit(&mut self, _: ChunkPos) -> Admission {
        Admission::Ready(if self.cache { Plan::CacheRelease(RAW_BYTES) }
            else { Plan::Encode { release_bytes: RAW_BYTES } })
    }
    fn execute(&mut self, p: ChunkPos, _: Plan) -> Outcome {
        self.attempted.push(p); Outcome { unprofitable: 1, ..Default::default() }
    }
}
#[test]
fn block_sweep_bounds_progress_without_restarting_or_pinning_unprofitable_columns() {
    let mut store = Fake { order: (0..160).map(|x| (x, 0)).collect(), ..Default::default() };
    let mut cursor = 0;
    for expected in 0..160 {
        let result = pass(&mut store, &mut cursor, &context(), limits(1));
        assert_eq!(result.encodes, 1); assert_eq!(result.input_bytes, RAW_BYTES);
        assert_eq!(result.release_bytes_budgeted, RAW_BYTES);
        assert_eq!(store.attempted.last(), Some(&(expected, 0)));
        assert_eq!(cursor, (expected as usize + 1) % 160);
    }
    let mut unlimited = limits(usize::MAX / RAW_BYTES);
    unlimited.scans = usize::MAX;
    let result = pass(&mut store, &mut cursor, &context(), unlimited);
    assert_eq!(result.visited, SCAN_LIMIT); assert_eq!(result.encodes, SCAN_LIMIT);
    let before = cursor;
    let result = pass(&mut store, &mut cursor, &context(), limits(0));
    assert_eq!(result.visited, 0); assert_eq!(cursor, before);
    store.cache = true;
    let budget = Limits { cache_releases: 1, release_bytes: RAW_BYTES, ..limits(0) };
    let result = pass(&mut store, &mut cursor, &context(), budget);
    assert_eq!(result.cache_releases, 1); assert_eq!(result.release_bytes_budgeted, RAW_BYTES);
    assert_eq!(cursor, (before + 1) % 160);
    // Appending a provider record and shrinking a synthetic fixture never loses
    // progress or reads out of bounds. Real provider history is never shrunk.
    store.order.push((160, 0)); cursor = 160;
    pass(&mut store, &mut cursor, &context(), budget); assert_eq!(cursor, 0);
    cursor = 999;
    pass(&mut store, &mut cursor, &context(), budget); assert_eq!(cursor, 1);
}

struct Quiet;
impl Hooks for Quiet {
    fn populate(&self, _: &mut Access<'_>, _: ChunkPos, _: &mut JavaRandom) -> anyhow::Result<()> { Ok(()) }
}
fn runtime() -> Runtime {
    let mut runtime = Runtime::new(189, 1234, false, Arc::new(Quiet));
    runtime.use_overlay_journal(false); runtime.provide((0, 0)).unwrap();
    drop(runtime.take_chunk_updates()); runtime
}
fn store(runtime: &mut Runtime, generation: u64) -> RuntimeStore<'_> {
    RuntimeStore { runtime, generation, cooldown: 32, minimum_saved_bytes: 4096 }
}
fn wire(runtime: &Runtime) -> serde_json::Value {
    serde_json::to_value(runtime.checkpoint().unwrap()).unwrap()
}
fn plan(runtime: &mut Runtime, generation: u64) -> Plan {
    match store(runtime, generation).admit((0, 0)) { Admission::Ready(plan) => plan, _ => panic!("fixture must be admitted") }
}
#[test]
fn native_blocks_require_clean_prepared_unique_outer_owner_and_preserve_complete_snapshot() {
    let mut runtime = runtime(); let p = (0, 0);
    let expected = wire(&runtime);
    let provider = serde_json::to_value(&runtime.provider).unwrap();
    let random = (runtime.backend.provider_random.state(), runtime.backend.world_random.state());
    let ptr = Arc::as_ptr(&runtime.backend.world.chunks[&p]);
    let strong = runtime.backend.world.chunks[&p].clone();
    assert!(matches!(store(&mut runtime, 1).admit(p), Admission::Shared));
    let weak = Arc::downgrade(&strong); drop(strong);
    assert!(matches!(store(&mut runtime, 1).admit(p), Admission::Shared)); drop(weak);
    runtime.backend.dirty_chunks.insert(p);
    assert!(matches!(store(&mut runtime, 1).admit(p), Admission::DirtyOrMissing));
    runtime.backend.dirty_chunks.remove(&p);
    let light = runtime.backend.light.chunks.remove(&p).unwrap();
    assert!(matches!(store(&mut runtime, 1).admit(p), Admission::DirtyOrMissing));
    runtime.backend.light.chunks.insert(p, light);
    Arc::get_mut(runtime.backend.world.chunks.get_mut(&p).unwrap()).unwrap().invalidate_tick_indexes();
    assert!(matches!(store(&mut runtime, 1).admit(p), Admission::Unprepared));
    runtime.backend.world.chunks[&p].prepare_tick_indexes();
    let candidate = plan(&mut runtime, 1);
    let result = store(&mut runtime, 1).execute(p, candidate);
    assert_eq!(result.compressed, 1); assert_eq!(result.raw_released, RAW_BYTES);
    assert_eq!(Arc::as_ptr(&runtime.backend.world.chunks[&p]), ptr);
    assert!(runtime.backend.world.chunks[&p].blocks.is_cold());
    assert_eq!(wire(&runtime), expected);
    assert_eq!(serde_json::to_value(&runtime.provider).unwrap(), provider);
    assert_eq!((runtime.backend.provider_random.state(), runtime.backend.world_random.state()), random);
    assert!(runtime.backend.dirty_chunks.is_empty());
    assert!(runtime.backend.light.diagnostic_render_dirty().is_empty());
    let blocks = &mut Arc::get_mut(runtime.backend.world.chunks.get_mut(&p).unwrap()).unwrap().blocks;
    assert!(!blocks.has_read_cache()); assert!(!blocks.take_recent_cold_access());
}

#[test]
fn protected_windows_and_command_targets_never_admit_block_storage() {
    let mut view = super::super::ViewState::default();
    view.desired((0, 0), 1); view.commit((100, 100), 1); view.first_play_presented();
    let context = Context::capture(&Command::GenerateAhead { positions: vec![(200, 200)], ahead: vec![(300, 300)] },
        view.hint(), true, false).unwrap();
    let mut store = Fake { order: vec![(3, -3), (103, 97), (202, 198), (298, 302), (50, 50)], ..Default::default() };
    let result = pass(&mut store, &mut 0, &context, limits(8));
    assert_eq!(result.protected, 4); assert_eq!(store.attempted, [(50, 50)]);
    assert!(Context::capture(&Command::Generate(vec![]), view.hint(), true, true).is_none());
    assert!(Context::capture(&Command::Snapshot { gameplay: None }, view.hint(), true, false).is_none());
    assert!(Context::capture(&Command::Tick { steps: 1 }, view.hint(), true, false).is_none());
}

#[test]
fn block_reads_cache_release_and_publication_promotions_keep_cooldown_and_values() {
    let mut runtime = runtime(); let p = (0, 0); let expected = wire(&runtime);
    let candidate = plan(&mut runtime, 1); store(&mut runtime, 1).execute(p, candidate);
    let first = runtime.backend.world.chunks[&p].blocks.value_at(0);
    assert!(matches!(store(&mut runtime, 2).admit(p), Admission::Cooling { recent: 1 }));
    assert!(matches!(store(&mut runtime, 33).admit(p), Admission::Cooling { recent: 0 }));
    let candidate = plan(&mut runtime, 34); assert!(matches!(candidate, Plan::CacheRelease(RAW_BYTES)));
    assert_eq!(store(&mut runtime, 34).execute(p, candidate).cache_released, RAW_BYTES);
    assert!(!runtime.backend.world.chunks[&p].blocks.has_read_cache());
    assert_eq!(wire(&runtime), expected); // bulk serialization leaves no cache/notice
    assert!(matches!(plan(&mut runtime, 35), Plan::Skip));
    // Ordinary publication must decode on the owner before sharing and leave
    // the notice for a future eligible maintenance pass, not recompress at once.
    runtime.backend.dirty_chunks.insert(p);
    let published = runtime.take_chunk_updates();
    assert!(published.iter().all(|(_, c)| c.blocks.is_dense()));
    assert_eq!(published[0].1.blocks.value_at(0), first);
    assert!(matches!(store(&mut runtime, 36).admit(p), Admission::Shared));
    drop(published);
    assert!(matches!(store(&mut runtime, 36).admit(p), Admission::Cooling { recent: 1 }));
    assert!(matches!(store(&mut runtime, 67).admit(p), Admission::Cooling { recent: 0 }));
    assert!(matches!(plan(&mut runtime, 68), Plan::Encode { .. }));
    assert_eq!(wire(&runtime), expected);
}

fn updates_equal(a: &Update, b: &Update) {
    assert_eq!(a.chunks.len(), b.chunks.len()); assert_eq!(a.remesh, b.remesh);
    for ((pa, a), (pb, b)) in a.chunks.iter().zip(&b.chunks) {
        assert_eq!(pa, pb); assert!(a.blocks.is_dense() && b.blocks.is_dense());
        assert_eq!(a.blocks, b.blocks); assert_eq!(a.biomes, b.biomes); assert_eq!(a.top, b.top);
        match (&a.light, &b.light) {
            (Some(a), Some(b)) => { assert_eq!(a.values, b.values); assert_eq!(a.height, b.height); }
            (None, None) => {}, _ => panic!("packed availability differs"),
        }
    }
    assert_eq!(format!("{:?}", a.metadata), format!("{:?}", b.metadata));
    assert_eq!(serde_json::to_value((&a.falling, &a.falling_effects, &a.items, &a.orbs,
        &a.experience, &a.entity_effects, &a.player_frame, a.world_time)).unwrap(),
        serde_json::to_value((&b.falling, &b.falling_effects, &b.items, &b.orbs,
        &b.experience, &b.entity_effects, &b.player_frame, b.world_time)).unwrap());
}
// Exact payload comparison is column-bounded and never populates owner caches.
// Do not use assert_eq! on full arrays: report one differing index/value instead
// of allocating/formatting millions of numbers if a regression is found.
fn exact_payload<T: PartialEq + std::fmt::Debug>(p: ChunkPos, plane: &str, a: &[T], b: &[T]) {
    assert_eq!(a.len(), b.len(), "{p:?} {plane} length");
    if let Some((index, (left, right))) = a.iter().zip(b).enumerate().find(|(_, (x, y))| x != y) {
        panic!("{p:?} {plane}[{index}] differs: {left:?} vs {right:?}");
    }
}
fn exact_native_payloads(a: &Runtime, b: &Runtime) {
    let positions = |runtime: &Runtime| {
        let mut positions = runtime.backend.world.chunks.keys().copied().collect::<Vec<_>>();
        positions.sort_unstable(); positions
    };
    assert_eq!(positions(a), positions(b), "raw column membership");
    let light_positions = |runtime: &Runtime| {
        let mut positions = runtime.backend.light.chunks.keys().copied().collect::<Vec<_>>();
        positions.sort_unstable(); positions
    };
    assert_eq!(light_positions(a), light_positions(b), "light column membership");
    for (p, left) in &a.backend.world.chunks {
        let right = &b.backend.world.chunks[p];
        left.blocks.with_slice(|left| right.blocks.with_slice(|right|
            exact_payload(*p, "blocks", left, right)));
    }
    for (p, left) in &a.backend.light.chunks {
        let right = &b.backend.light.chunks[p];
        left.sky.with_bytes(|left| right.sky.with_bytes(|right|
            exact_payload(*p, "sky", left, right)));
        left.block.with_bytes(|left| right.block.with_bytes(|right|
            exact_payload(*p, "block_light", left, right)));
    }
}
fn checkpoint_without_compared_payloads(runtime: &Runtime) -> serde_json::Value {
    // Keep the real checkpoint constructor/validation and ALL metadata fields.
    // Only previously compared large numeric arrays are replaced in this owned
    // temporary. This does not edit, hydrate, drain or normalize either owner.
    // Build each arm separately so only one temporary raw checkpoint exists.
    let mut snapshot = runtime.checkpoint().unwrap();
    for chunk in &mut snapshot.chunks {
        let owner = &runtime.backend.world.chunks[&chunk.position];
        chunk.blocks.with_slice(|saved| owner.blocks.with_slice(|live|
            exact_payload(chunk.position, "checkpoint blocks clone", saved, live)));
        chunk.blocks = Default::default();
    }
    for (p, light) in &mut snapshot.light.chunks {
        let owner = &runtime.backend.light.chunks[p];
        light.sky.with_bytes(|saved| owner.sky.with_bytes(|live|
            exact_payload(*p, "checkpoint sky clone", saved, live)));
        light.block.with_bytes(|saved| owner.block.with_bytes(|live|
            exact_payload(*p, "checkpoint block-light clone", saved, live)));
        light.sky = Default::default(); light.block = Default::default();
    }
    serde_json::to_value(snapshot).unwrap()
}
fn first_metadata_difference(a: &serde_json::Value, b: &serde_json::Value, path: &str) -> Option<String> {
    use serde_json::Value;
    if a == b { return None; }
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            if a.len() != b.len() { return Some(format!("{path}.keys")); }
            for (name, a) in a {
                let next = format!("{path}.{name}");
                let Some(b) = b.get(name) else { return Some(next); };
                if let Some(next) = first_metadata_difference(a, b, &next) { return Some(next); }
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() { return Some(format!("{path}.len")); }
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                if let Some(next) = first_metadata_difference(a, b, &format!("{path}[{index}]")) { return Some(next); }
            }
        }
        _ => {}
    }
    Some(path.to_owned())
}
fn native_equal(a: &Worker, b: &Worker) {
    exact_native_payloads(&a.runtime, &b.runtime);
    let left = checkpoint_without_compared_payloads(&a.runtime);
    let right = checkpoint_without_compared_payloads(&b.runtime);
    if let Some(path) = first_metadata_difference(&left, &right, "checkpoint") {
        panic!("native non-payload checkpoint differs at {path}");
    }
    assert_eq!(serde_json::to_value(&a.runtime.provider).unwrap(), serde_json::to_value(&b.runtime.provider).unwrap());
    assert_eq!(a.runtime.provider.loaded_order(), b.runtime.provider.loaded_order());
    assert_eq!(a.runtime.backend.dirty_chunks, b.runtime.backend.dirty_chunks);
    assert_eq!(a.runtime.backend.light.diagnostic_render_dirty(), b.runtime.backend.light.diagnostic_render_dirty());
    assert_eq!(a.runtime.backend.events, b.runtime.backend.events);
    assert_eq!((a.runtime.backend.provider_random.state(), a.runtime.backend.world_random.state()),
        (b.runtime.backend.provider_random.state(), b.runtime.backend.world_random.state()));
}

fn command(step: usize) -> Command {
    use crate::native_session::BlockEdit;
    match step {
        0 => Command::Generate(vec![(0, 0), (1, 0), (0, 1), (1, 1)]),
        1 => Command::Generate(vec![]),
        2 => Command::Generate(vec![(0, 0)]),
        3 => Command::Edits(vec![BlockEdit { position: [8, 200, 8], state: 89 << 4, expected: None, notify_neighbors: true }]),
        4 => Command::Tick { steps: 2 },
        5 => Command::Generate(vec![(2, 0), (2, 1)]),
        6 => Command::Edits(vec![BlockEdit { position: [8, 200, 8], state: 0, expected: Some(89 << 4), notify_neighbors: true }]),
        _ => Command::Generate(vec![(0, 0), (1, 0), (3, 0)]),
    }
}
fn request(worker: &mut Worker, command: Command) -> Update {
    let context = Context::capture(&command, Some(context().view), worker.maintenance_policy.enabled(),
        worker.fresh_spawn_publication);
    let deferred = matches!(&command, Command::LoadSpawnBatchDeferred { .. });
    let (reply, stop) = worker.execute(command).unwrap(); assert!(!stop);
    let update = worker.update_after_reply(&reply, deferred);
    worker.maintenance_policy.after_publication(&mut worker.runtime, &mut worker.maintenance_cursor,
        super::super::successful(context, &reply));
    update
}
#[test]
fn enabled_blocks_match_disabled_commands_checkpoints_resume_and_dense_client_publications() {
    use crate::native_session::spawn_publication_tests::{oracle_pair, PublicationOracleHooks};
    let (mut a, mut b) = oracle_pair();
    for worker in [&mut a, &mut b] {
        worker.config.enable_scheduled_ticks = true; worker.fresh_spawn_publication = false;
        worker.maintenance_policy.blocks.limits = Some(Limits { elapsed: Duration::from_secs(5), ..Limits::initial() });
    }
    a.maintenance_policy.blocks.limits = None;
    for step in 0..8 {
        let left = request(&mut a, command(step)); let right = request(&mut b, command(step));
        updates_equal(&left, &right); drop((left, right)); native_equal(&a, &b);
        if step == 1 {
            assert!(b.runtime.backend.world.chunks.values().any(|c| c.blocks.is_cold()));
            assert!(a.runtime.backend.world.chunks.values().all(|c| c.blocks.is_dense()));
        }
        if step == 5 {
            // In-memory and numeric-wire checkpoints must both preserve full
            // provider/blocks/light/ticks/entities/RNG; neither hydrates owner.
            let sa = a.runtime.checkpoint().unwrap(); let sb = b.runtime.checkpoint().unwrap();
            let encoded = serde_json::to_vec(&sb).unwrap();
            assert_eq!(serde_json::to_value(&sa).unwrap(), serde_json::from_slice::<serde_json::Value>(&encoded).unwrap());
            a.runtime = Runtime::resume(sa, Arc::new(PublicationOracleHooks)).unwrap();
            b.runtime = Runtime::resume(serde_json::from_slice(&encoded).unwrap(), Arc::new(PublicationOracleHooks)).unwrap();
            for worker in [&mut a, &mut b] {
                worker.runtime.use_overlay_journal(false); worker.published = Default::default();
                worker.maintenance_cursor = Default::default(); worker.maintenance_policy.generation = 0;
                worker.maintenance_policy.blocks.cursor = 0;
            }
            let left = a.update(true); let right = b.update(true);
            updates_equal(&left, &right); drop((left, right)); native_equal(&a, &b);
        }
    }
    assert_eq!(a.maintenance_policy.blocks.metrics.compressed_columns, 0);
    assert!(b.maintenance_policy.blocks.metrics.compressed_columns > 0);
}

#[test]
#[ignore = "paired original625 + off-view compression + revisit; run alone in optimized tests"]
fn original_625_block_maintenance_keeps_full_checkpoint_rng_order_and_dense_revisit() {
    use crate::native_session::spawn_publication_tests::oracle_pair;
    let (mut a, mut b) = oracle_pair();
    a.maintenance_policy.blocks.limits = None;
    b.maintenance_policy.blocks.limits = Some(Limits { elapsed: Duration::from_secs(5), ..Limits::initial() });
    for worker in [&mut a, &mut b] {
        let (_, stop) = worker.execute(Command::PrepareSpawn).unwrap(); assert!(!stop);
        drop(worker.update(false));
        while worker.spawn_load_cursor != Some(625) {
            drop(request(worker, Command::LoadSpawnBatchDeferred { count: 25 }));
        }
        assert_eq!(worker.maintenance_policy.blocks.metrics.passes, 0);
    }
    native_equal(&a, &b);
    for _ in 0..128 {
        drop(request(&mut a, Command::Generate(vec![])));
        drop(request(&mut b, Command::Generate(vec![])));
    }
    assert!(b.runtime.backend.world.chunks.values().any(|c| c.blocks.is_cold()));
    native_equal(&a, &b);
    let positions = a.runtime.provider.loaded_order().to_vec();
    for batch in positions.chunks(32) {
        let left = request(&mut a, Command::Generate(batch.to_vec()));
        let right = request(&mut b, Command::Generate(batch.to_vec()));
        updates_equal(&left, &right); drop((left, right)); native_equal(&a, &b);
    }
}

#[test]
fn bounded_native_comparison_is_exact_for_payloads_and_metadata_without_owner_hydration() {
    use crate::native_session::spawn_publication_tests::oracle_pair;
    let (mut a, mut b) = oracle_pair();
    for worker in [&mut a, &mut b] {
        let (_, stop) = worker.execute(Command::Generate(vec![(0, 0)])).unwrap(); assert!(!stop);
        drop(worker.update(false));
    }
    let p = (0, 0);
    assert!(Arc::get_mut(b.runtime.backend.world.chunks.get_mut(&p).unwrap()).unwrap()
        .blocks.compress_if_saving(4096));
    let light = b.runtime.backend.light.chunks.get_mut(&p).unwrap();
    assert!(light.sky.compress_if_saving(1024).unwrap());
    assert!(light.block.compress_if_saving(1024).unwrap());
    native_equal(&a, &b);
    let blocks = &mut Arc::get_mut(b.runtime.backend.world.chunks.get_mut(&p).unwrap()).unwrap().blocks;
    assert!(blocks.is_cold() && !blocks.has_read_cache() && !blocks.take_recent_cold_access());
    let light = b.runtime.backend.light.chunks.get_mut(&p).unwrap();
    for bytes in [&mut light.sky, &mut light.block] {
        assert!(bytes.is_cold() && !bytes.has_read_cache() && !bytes.take_recent_cold_access());
    }
    let fails = |a: &Worker, b: &Worker| {
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| native_equal(a, b))).is_err());
    };
    let old = b.runtime.backend.world.chunks[&p].blocks.with_slice(|values| values[15]);
    Arc::get_mut(b.runtime.backend.world.chunks.get_mut(&p).unwrap()).unwrap().blocks[15] = old ^ 0xf000;
    fails(&a, &b);
    Arc::get_mut(b.runtime.backend.world.chunks.get_mut(&p).unwrap()).unwrap().blocks[15] = old;
    for sky in [false, true] {
        let light = b.runtime.backend.light.chunks.get_mut(&p).unwrap();
        let values = if sky { &mut light.sky } else { &mut light.block };
        let old = values.with_bytes(|values| values[19]); values[19] = old ^ 1;
        fails(&a, &b);
        let light = b.runtime.backend.light.chunks.get_mut(&p).unwrap();
        let values = if sky { &mut light.sky } else { &mut light.block }; values[19] = old;
    }
    // These fields remain in the full checkpoint comparison rather than being
    // accidentally erased along with the already-compared large arrays.
    b.runtime.backend.light.chunks.get_mut(&p).unwrap().gaps[0] ^= true;
    fails(&a, &b); b.runtime.backend.light.chunks.get_mut(&p).unwrap().gaps[0] ^= true;
    Arc::get_mut(b.runtime.backend.world.chunks.get_mut(&p).unwrap()).unwrap().biomes[3] ^= 1;
    fails(&a, &b); Arc::get_mut(b.runtime.backend.world.chunks.get_mut(&p).unwrap()).unwrap().biomes[3] ^= 1;
    b.runtime.backend.world_random.next_int(17); fails(&a, &b);
    b.runtime.backend.world_random = a.runtime.backend.world_random.clone();
    native_equal(&a, &b);
}
