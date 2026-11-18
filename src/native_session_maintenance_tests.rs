use super::*;
use super::super::{Event, NativeSession};
use std::{collections::HashSet, sync::{mpsc, Arc}, path::PathBuf};
fn ready_view(center: ChunkPos, radius: i32) -> ViewState {
    let mut v = ViewState::default(); v.desired(center, radius); v.commit(center, radius);
    assert!(v.first_play_presented()); v
}
fn context() -> Context {
    Context::capture(&Command::Generate(vec![(0, 0)]), ready_view((0, 0), 1).hint(), true, false).unwrap()
}
fn limits(encodes: usize) -> Limits {
    Limits { scans: 64, encodes, input_bytes: encodes * ARRAY_BYTES, cache_releases: 0,
        packed_attempts: 0, release_bytes: encodes * ARRAY_BYTES, elapsed: Duration::from_secs(5) }
}
#[derive(Default)]
struct MemoryStore { order: Vec<ChunkPos>, blocked: HashSet<ChunkPos>, attempted: Vec<(ChunkPos, Stage)>,
    cache: bool }
impl Store for MemoryStore {
    fn len(&self) -> usize { self.order.len() }
    fn position(&self, index: usize) -> ChunkPos { self.order[index] }
    fn eligibility(&mut self, p: ChunkPos) -> Eligibility {
        if self.blocked.contains(&p) { Eligibility::DirtyOrMissing } else { Eligibility::Ready }
    }
    fn plan(&self, _: ChunkPos, stage: Stage) -> Plan {
        if stage == Stage::Packed { Plan::PackedRelease(ARRAY_BYTES + 1024) }
        else if self.cache { Plan::CacheRelease(ARRAY_BYTES) } else { Plan::Encode { input_bytes: ARRAY_BYTES, release_capacity: ARRAY_BYTES } }
    }
    fn execute(&mut self, p: ChunkPos, stage: Stage, plan: Plan) -> Outcome {
        self.attempted.push((p, stage));
        match plan { Plan::Encode { .. } => Outcome { unprofitable: 1, ..Default::default() },
            Plan::CacheRelease(bytes) => Outcome { cache_released: bytes, ..Default::default() },
            Plan::PackedRelease(bytes) => Outcome { packed_removed: 1, packed_bytes: bytes,
                packed_freed: bytes, ..Default::default() }, Plan::Skip => Outcome::default() }
    }
}
#[test]
fn desired_and_committed_protect_busy_moves_distance_changes_and_offscreen_requests() {
    let mut v = ready_view((0, 0), 25);
    v.desired((200, 0), 5);
    let c = Context::capture(&Command::Generate(vec![]), v.hint(), true, false).unwrap();
    assert!(c.protects((27, 0))); // old committed radius plus light halo
    assert!(c.protects((207, 0))); // current desired, before a busy host can trim
    assert!(!c.protects((100, 0))); // separate rectangles, no teleport-sized union
    v.commit((200, 0), 5);
    let c = Context::capture(&Command::Generate(vec![]), v.hint(), true, false).unwrap();
    assert!(!c.protects((0, 0)));
    v.include_requests(&[(215, 10)]);
    assert!(v.hint().unwrap().committed.protects((217, 12)));
    v.commit((200, 0), 5); // real trim resets temporary offscreen expansion
    assert!(!v.hint().unwrap().committed.protects((217, 12)));
    v.desired((i32::MAX, 0), 5);
    assert!(v.hint().is_none()); // invalid bounds disable, never shrink protection
}

#[test]
fn submission_stamps_latest_hint_without_adding_commands_or_request_ids() {
    let (sender, requests) = mpsc::sync_channel(4);
    let (_events, receiver) = mpsc::channel::<Event>();
    let mut session = NativeSession { sender: Some(sender), receiver, next_id: 1,
        terminal: false, closing: false, scheduled_ticks: false, persistent: false,
        worker: None, maintenance_view: None, staged_gameplay: Default::default() };
    let mut v = ready_view((0, 0), 25);
    session.set_maintenance_view(v.hint());
    let queued_before_move = Command::Generate(vec![(12, 12)]);
    v.desired((200, 0), 5); session.set_maintenance_view(v.hint());
    assert!(matches!(requests.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert_eq!(session.next_id, 1);
    assert_eq!(session.submit(queued_before_move).unwrap(), 1);
    let first = requests.try_recv().unwrap();
    assert_eq!(first.maintenance_view, v.hint());
    assert!(matches!(first.command, Command::Generate(ref p) if p == &[(12, 12)]));
    let stamped = first.maintenance_view;
    v.desired((400, 0), 5); session.set_maintenance_view(v.hint());
    assert_ne!(stamped, v.hint()); // an already-running command is a snapshot
    assert_eq!(session.submit(Command::Generate(vec![(13, 12)])).unwrap(), 2);
    assert_eq!(requests.try_recv().unwrap().maintenance_view, v.hint());
    assert!(matches!(requests.try_recv(), Err(mpsc::TryRecvError::Empty)));
}

#[test]
fn fresh_and_first_play_barriers_do_not_require_a_resumed_spawn_cursor() {
    let command = Command::Generate(vec![(0, 0)]);
    let mut v = ViewState::default(); v.desired((0, 0), 25); v.commit((0, 0), 25);
    assert!(Context::capture(&command, v.hint(), true, false).is_none());
    v.first_play_presented();
    assert!(Context::capture(&command, v.hint(), true, true).is_none());
    // A completed resume may leave spawn_load_cursor at zero after skipping the
    // retained original625 barrier; only fresh-publication and first-play gate it.
    assert!(Context::capture(&command, v.hint(), true, false).is_some());
    assert!(Context::capture(&command, v.hint(), false, false).is_none());
    assert!(Context::capture(&Command::LoadSpawnBatchDeferred { count: 25 }, v.hint(), true, false).is_none());
    assert!(Context::capture(&Command::PreparePlayer { adventure: false }, v.hint(), true, false).is_none());
    assert!(Context::capture(&Command::Edits(vec![]), v.hint(), true, false).is_none());
    assert!(Context::capture(&Command::Tick { steps: 1 }, v.hint(), true, false).is_none());
    assert!(Context::capture(&Command::Snapshot { gameplay: None }, v.hint(), true, false).is_none());
    assert!(Context::capture(&Command::Shutdown { gameplay: None }, v.hint(), true, false).is_none());
    assert!(Context::capture(&command, None, true, false).is_none());
    assert!(successful(Some(context()), &super::super::Reply::AwaitingTnt {
        completed_steps: 0, pending: Vec::new() }).is_none());
    assert!(successful(Some(context()), &super::super::Reply::Generated).is_some());
}

#[test]
fn current_targets_ahead_and_two_column_halo_are_protected() {
    let c = Context::capture(&Command::GenerateAhead { positions: vec![(100, 100)],
        ahead: vec![(-100, -100)] }, ready_view((0, 0), 1).hint(), true, false).unwrap();
    assert!(c.protects((102, 98))); assert!(c.protects((-102, -98)));
    assert!(!c.protects((103, 100)));
    assert!(!c.protects((i32::MIN, i32::MAX))); // checked with widened arithmetic
    assert!(Context::capture(&Command::Generate(vec![(0, 0); 257]),
        ready_view((0, 0), 1).hint(), true, false).is_none());
}


#[test]
fn bounded_sweep_preserves_partial_progress_and_unprofitable_encodes_do_not_pin_sky() {
    let mut store = MemoryStore { order: vec![(100, 0), (101, 0)], ..Default::default() };
    let mut cursor = Cursor::default();
    let first = pass(&mut store, &mut cursor, &context(), limits(1));
    assert_eq!(first.release_bytes_budgeted, ARRAY_BYTES);
    assert_eq!(first.encodes, 1); assert_eq!(cursor.index, 0); assert_eq!(cursor.stage, Stage::Block);
    pass(&mut store, &mut cursor, &context(), limits(1));
    assert_eq!(store.attempted, vec![((100, 0), Stage::Sky), ((100, 0), Stage::Block)]);
    assert_eq!(cursor.index, 1); assert_eq!(cursor.stage, Stage::Sky);
    store.order.push((102, 0));
    pass(&mut store, &mut cursor, &context(), limits(4));
    assert_eq!(cursor.index, 0); assert_eq!(store.attempted.len(), 6);
    let before = (cursor.index, cursor.stage, store.attempted.len());
    assert_eq!(pass(&mut store, &mut cursor, &context(), limits(0)).visited, 0);
    assert_eq!(before, (cursor.index, cursor.stage, store.attempted.len()));
}

#[test]
fn scan_and_actual_encode_drop_budgets_are_bounded_independently() {
    let mut store = MemoryStore { order: (0..160).map(|x| (1000 + x, 0)).collect(), ..Default::default() };
    store.blocked.extend(store.order.iter().copied());
    let mut cursor = Cursor::default(); let mut budget = limits(1024); budget.scans = usize::MAX;
    let result = pass(&mut store, &mut cursor, &context(), budget);
    assert_eq!(result.visited, 64); assert_eq!(result.dirty, 64); assert_eq!(cursor.index, 64);
    assert!(store.attempted.is_empty()); store.blocked.clear();
    let result = pass(&mut store, &mut cursor, &context(), budget);
    assert_eq!(result.visited, 64); assert_eq!(result.encodes, 128); assert_eq!(cursor.index, 128);
    let mut aggregate = MemoryStore { order: vec![(100, 0), (101, 0)], ..Default::default() };
    let mut one_free = limits(20); one_free.release_bytes = ARRAY_BYTES;
    let mut partial = Cursor::default();
    let bounded = pass(&mut aggregate, &mut partial, &context(), one_free);
    assert_eq!(bounded.encodes, 1); assert_eq!(bounded.release_bytes_budgeted, ARRAY_BYTES);
    assert_eq!(partial.stage, Stage::Block);
    let mut protected = MemoryStore { order: vec![(0, 0), (1, 1)], ..Default::default() };
    let result = pass(&mut protected, &mut Cursor::default(), &context(), limits(20));
    assert_eq!(result.visited, 2); assert_eq!(result.protected, 2); assert!(protected.attempted.is_empty());
    let mut cache = MemoryStore { order: vec![(100, 0), (101, 0)], cache: true, ..Default::default() };
    let budget = Limits { scans: 64, encodes: 0, input_bytes: 0, cache_releases: 1,
        packed_attempts: 1, release_bytes: 2 * ARRAY_BYTES + 1024, elapsed: Duration::from_secs(5) };
    let mut cursor = Cursor::default();
    let a = pass(&mut cache, &mut cursor, &context(), budget);
    assert_eq!(a.cache_releases, 1); assert_eq!(a.packed_attempts, 0); assert_eq!(cursor.stage, Stage::Block);
    let b = pass(&mut cache, &mut cursor, &context(), budget);
    assert_eq!(b.cache_releases, 1); assert_eq!(b.packed_attempts, 1);
    assert_eq!(b.release_bytes_budgeted, 2 * ARRAY_BYTES + 1024);
    assert_eq!(cursor.index, 1); assert_eq!(cursor.stage, Stage::Sky);
}

fn runtime() -> Runtime {
    let mut runtime = Runtime::for_overworld_verification(189, 1234, false);
    runtime.use_overlay_journal(false); runtime.provide((0, 0)).unwrap();
    drop(runtime.take_chunk_updates()); runtime
}
#[test]
fn actual_packed_release_respects_outer_strong_weak_and_nested_payload_ownership() {
    let mut runtime = runtime(); let p = (0, 0);
    let held = Arc::clone(&runtime.backend.world.chunks[&p]); let weak = Arc::downgrade(&held);
    let plan = RuntimeStore { runtime: &mut runtime, generation: 1, cooldown: 32, minimum_saved_bytes: 1024 }.plan(p, Stage::Packed);
    let execute = |runtime: &mut Runtime| RuntimeStore { runtime, generation: 1,
        cooldown: 32, minimum_saved_bytes: 1024 }.execute(p, Stage::Packed, plan);
    assert_eq!(execute(&mut runtime).packed_shared, 1);
    drop(held); assert_eq!(execute(&mut runtime).packed_shared, 1); drop(weak);
    let payload = runtime.backend.world.chunks[&p].light.as_ref().unwrap().clone();
    let values = payload.values.clone(); let height = payload.height.clone();
    let out = execute(&mut runtime); assert_eq!(out.packed_removed, 1); assert_eq!(out.packed_freed, 0);
    assert!(runtime.backend.world.chunks[&p].light.is_none()); assert_eq!(payload.values, values);
    assert!(runtime.backend.dirty_chunks.is_empty()); assert!(runtime.backend.light.diagnostic_render_dirty().is_empty());
    // Explicit later publication rebuilds the derivative cache without hydration
    // or any maintenance-created dirty marker.
    runtime.backend.dirty_chunks.insert(p); drop(runtime.take_chunk_updates());
    assert_eq!(runtime.backend.world.chunks[&p].light.as_ref().unwrap().values, values);
    assert_eq!(runtime.backend.world.chunks[&p].light.as_ref().unwrap().height, height);
    let out = execute(&mut runtime); assert!(out.packed_freed > 0);
}

#[test]
fn recent_cold_reads_and_mutation_promotions_defer_without_changing_logical_light() {
    let mut runtime = runtime(); let p = (0, 0);
    let c = runtime.backend.light.chunks.get_mut(&p).unwrap();
    assert!(c.sky.compress_if_saving(1024).unwrap()); assert!(c.block.compress_if_saving(1024).unwrap());
    let value = c.sky[0]; c.block[0] = c.block[0]; // ordinary cold access/promotion
    assert!(c.sky.has_read_cache()); assert!(!c.block.is_cold());
    let mut store = RuntimeStore { runtime: &mut runtime, generation: 10, cooldown: 32, minimum_saved_bytes: 1024 };
    assert!(matches!(store.eligibility(p), Eligibility::Cooling { recent: 2 }));
    store.generation = 41; assert!(matches!(store.eligibility(p), Eligibility::Cooling { recent: 0 }));
    store.generation = 42; assert!(matches!(store.eligibility(p), Eligibility::Ready));
    assert_eq!(store.runtime.backend.light.chunks[&p].sky[0], value);
    store.generation = 43; assert!(matches!(store.eligibility(p), Eligibility::Cooling { recent: 1 }));
}

fn oracle_command(step: usize) -> Command {
    use super::super::BlockEdit;
    match step {
        0 => Command::Generate(vec![(0, 0), (1, 0), (0, 1), (1, 1)]),
        1 => Command::Generate(vec![]), // preload is now outside this boundary
        2 => Command::Generate(vec![(0, 0)]), // force derivative-cache re-publication
        3 => Command::Edits(vec![BlockEdit { position: [8, 200, 8], state: 89 << 4,
            expected: None, notify_neighbors: true }]),
        4 => Command::Tick { steps: 2 },
        5 => Command::Generate(vec![(2, 0), (2, 1)]),
        6 => Command::Edits(vec![BlockEdit { position: [8, 200, 8], state: 0,
            expected: Some(89 << 4), notify_neighbors: true }]),
        _ => Command::Generate(vec![(0, 0), (1, 0), (3, 0)]),
    }
}
fn execute_oracle(worker: &mut super::super::Worker, step: usize) -> super::super::Update {
    let command = oracle_command(step); super::super::validate_command(&command).unwrap();
    let context = Context::capture(&command, ready_view((1000, 1000), 25).hint(),
        worker.maintenance_policy.enabled(), worker.fresh_spawn_publication);
    let (reply, stop) = worker.execute(command).unwrap(); assert!(!stop);
    let context = successful(context, &reply);
    let update = worker.update_after_reply(&reply, false);
    // The update has been formed before owner maintenance, as with Completed.
    // Its retained Arcs exercise the shared packed-cache guards.
    worker.maintenance_policy.after_publication(&mut worker.runtime, &mut worker.maintenance_cursor, context);
    worker.generation_timings.maintenance = worker.maintenance_policy.metrics();
    update
}
fn assert_updates_equal(a: &super::super::Update, b: &super::super::Update) {
    assert_eq!(a.chunks.len(), b.chunks.len()); assert_eq!(a.remesh, b.remesh);
    for ((pa, a), (pb, b)) in a.chunks.iter().zip(&b.chunks) {
        assert_eq!(pa, pb); assert_eq!(a.blocks, b.blocks); assert_eq!(a.biomes, b.biomes); assert_eq!(a.top, b.top);
        match (&a.light, &b.light) {
            (Some(a), Some(b)) => { assert_eq!(a.values, b.values); assert_eq!(a.height, b.height); },
            (None, None) => {}, _ => panic!("packed light availability differs"),
        }
    }
    assert_eq!(a.world_time, b.world_time);
    assert_eq!(format!("{:?}", a.metadata), format!("{:?}", b.metadata));
}
fn assert_native_equal(a: &super::super::Worker, b: &super::super::Worker) {
    assert_eq!(serde_json::to_value(a.runtime.checkpoint().unwrap()).unwrap(),
        serde_json::to_value(b.runtime.checkpoint().unwrap()).unwrap());
    assert_eq!(serde_json::to_value(&a.runtime.provider).unwrap(), serde_json::to_value(&b.runtime.provider).unwrap());
    assert_eq!(a.runtime.provider.loaded_order(), b.runtime.provider.loaded_order());
    assert_eq!(a.runtime.backend.provider_random.state(), b.runtime.backend.provider_random.state());
    assert_eq!(a.runtime.backend.world_random.state(), b.runtime.backend.world_random.state());
    assert_eq!(a.runtime.backend.dirty_chunks, b.runtime.backend.dirty_chunks);
    assert_eq!(a.runtime.backend.light.diagnostic_render_dirty(), b.runtime.backend.light.diagnostic_render_dirty());
    assert_eq!(a.runtime.backend.events, b.runtime.backend.events);
}
#[test]
fn maintained_and_disabled_native_commands_atomic_checkpoint_and_resume_are_identical() {
    use super::super::{read_checkpoint, spawn_publication_tests::{oracle_pair, PublicationOracleHooks}};
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};
    let directory = std::env::temp_dir().join(format!("ferrum-cold-light-oracle-{}-{}",
        std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir_all(&directory).unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let (Ok(root), Ok(path)) = (std::fs::canonicalize(std::env::temp_dir()), std::fs::canonicalize(&self.0)) else { return; };
            if path.parent() == Some(root.as_path()) && path.file_name().is_some_and(|name|
                name.to_string_lossy().starts_with("ferrum-cold-light-oracle-")) {
                let _ = std::fs::remove_dir_all(path);
            }
        }
    }
    let _cleanup = Cleanup(directory.clone());
    let path = directory.join("native-world.json.gz");
    // Reuse the established delegating VanillaPopulation oracle: original
    // callbacks, with independent entity constructor/Math seeds controlled.
    let (mut disabled, mut maintained) = oracle_pair();
    disabled.config.enable_scheduled_ticks = true; maintained.config.enable_scheduled_ticks = true;
    disabled.fresh_spawn_publication = false; maintained.fresh_spawn_publication = false;
    disabled.maintenance_policy.limits = None;
    maintained.maintenance_policy.limits = Some(Limits { elapsed: Duration::from_secs(5),
        ..Limits::measured_initial() });
    maintained.maintenance_policy.blocks.prepare_atomic_save_oracle();
    // Only this disposable owner path changes; the logical world's save_path
    // remains identical so the complete checkpoint values compare without masking.
    disabled.path = path.clone(); maintained.path = path.clone();
    for step in 0..8 {
        let left = execute_oracle(&mut disabled, step); let right = execute_oracle(&mut maintained, step);
        assert_updates_equal(&left, &right); drop((left, right));
        assert_native_equal(&disabled, &maintained);
        if step == 1 {
            assert!(maintained.runtime.backend.light.chunks.values().any(|c| c.sky.is_cold() || c.block.is_cold()));
            assert!(disabled.runtime.backend.light.chunks.values().all(|c| !c.sky.is_cold() && !c.block.is_cold()));
        }
        if step == 5 {
            // Drop the ordinary publication Arcs, then exercise the real
            // successful-Generate admission on both arms before the disk save.
            let a = execute_oracle(&mut disabled, 1); let b = execute_oracle(&mut maintained, 1);
            assert_updates_equal(&a, &b); drop((a, b)); assert_native_equal(&disabled, &maintained);
            assert!(maintained.maintenance_policy.blocks.metrics().compressed_columns > 0);
            assert!(maintained.runtime.backend.world.chunks.values().any(|c| c.blocks.is_cold()));
            assert!(disabled.runtime.backend.world.chunks.values().all(|c| c.blocks.is_dense()));
            let storage = |worker: &super::super::Worker| {
                let mut values = worker.runtime.backend.world.chunks.iter().map(|(&p, c)|
                    (p, c.blocks.is_cold(), c.blocks.has_read_cache(), c.blocks.capacity(),
                        c.blocks.next_eligible_generation())).collect::<Vec<_>>();
                values.sort_unstable_by_key(|v| v.0); values
            };
            let cold_storage = storage(&maintained);
            disabled.gameplay = serde_json::json!({"player": [0.5, 76.0, 0.5], "test": "latest save"});
            maintained.gameplay = disabled.gameplay.clone();
            disabled.save().unwrap(); let left = read_checkpoint(fs::File::open(&path).unwrap()).unwrap();
            maintained.save().unwrap(); let right = read_checkpoint(fs::File::open(&path).unwrap()).unwrap();
            assert_eq!(storage(&maintained), cold_storage, "saving must not hydrate or alter live Cold metadata");
            assert_eq!(serde_json::to_value(&left).unwrap(), serde_json::to_value(&right).unwrap());
            disabled.runtime = Runtime::resume(left.runtime, Arc::new(PublicationOracleHooks)).unwrap();
            maintained.runtime = Runtime::resume(right.runtime, Arc::new(PublicationOracleHooks)).unwrap();
            for worker in [&mut disabled, &mut maintained] {
                worker.runtime.use_overlay_journal(false);
                worker.published = Default::default(); worker.maintenance_cursor = Cursor::default();
                worker.maintenance_policy.generation = 0;
            }
            // Match the ordinary Ready publication after reopening before
            // comparing state; resume marks all derivative lighting dirty.
            let a = disabled.update(true); let b = maintained.update(true);
            assert_updates_equal(&a, &b); drop((a, b));
            assert_native_equal(&disabled, &maintained);
        }
    }
    assert!(maintained.maintenance_policy.metrics.compressed_arrays > 0);
    assert_eq!(maintained.maintenance_policy.metrics.codec_errors, 0);
    assert_eq!(disabled.maintenance_policy.metrics.passes, 0);
    drop((disabled, maintained));
}

#[test]
fn actual_vanilla_worker_atomic_cold_save_reopens_identically_to_its_dense_baseline() {
    use super::super::{read_checkpoint, Config, Worker};
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};
    let directory = std::env::temp_dir().join(format!("ferrum-cold-block-reopen-{}-{}",
        std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir_all(&directory).unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let (Ok(root), Ok(path)) = (std::fs::canonicalize(std::env::temp_dir()), std::fs::canonicalize(&self.0)) else { return; };
            if path.parent() == Some(root.as_path()) && path.file_name().is_some_and(|name|
                name.to_string_lossy().starts_with("ferrum-cold-block-reopen-")) {
                let _ = std::fs::remove_dir_all(path);
            }
        }
    }
    let _cleanup = Cleanup(directory.clone()); let path = directory.join("world.json.gz");
    let config = Config::new(189, "cold-block-reopen", directory);
    // Use the real VanillaPopulation identity from construction through both
    // disk reopens. Generate only once: independent entity constructor seeds
    // cannot confound the same owner's Dense-versus-Cold storage comparison.
    let (mut worker, resumed) = Worker::open(config, path.clone(), Some(1234)).unwrap();
    assert!(!resumed && worker._lock.is_none()); worker.fresh_spawn_publication = false;
    worker.maintenance_policy.limits = None;
    let (reply, stop) = worker.execute(Command::Generate(vec![(0, 0)])).unwrap(); assert!(!stop);
    drop(worker.update_after_reply(&reply, false));
    worker.gameplay = serde_json::json!({"player":[0.5,76.0,0.5],"test":"same owner atomic reopen"});
    assert!(worker.runtime.backend.world.chunks.values().all(|c| c.blocks.is_dense()));
    let before = serde_json::to_value(worker.runtime.checkpoint().unwrap()).unwrap();
    let reopen = |worker: &Worker| {
        let (mut reopened, resumed) = Worker::open(worker.config.clone(), path.clone(), None).unwrap();
        assert!(resumed && reopened._lock.is_some() && !reopened.fresh_spawn_publication);
        assert_eq!(reopened.gameplay, worker.gameplay);
        let ready = reopened.update(true);
        assert!(ready.chunks.iter().all(|(_, c)| c.blocks.is_dense()));
        let state = serde_json::to_value(reopened.runtime.checkpoint().unwrap()).unwrap();
        let extra = (reopened.runtime.backend.dirty_chunks.clone(),
            reopened.runtime.backend.light.diagnostic_render_dirty().clone(), reopened.runtime.backend.events.clone());
        (ready, state, extra) // reopened drops the real OS lock before next save
    };
    worker.save().unwrap(); let dense_file = read_checkpoint(fs::File::open(&path).unwrap()).unwrap();
    let (dense_ready, dense_reopened, dense_extra) = reopen(&worker);
    worker.maintenance_policy.limits = Some(Limits { elapsed: Duration::from_secs(5), ..Limits::measured_initial() });
    worker.maintenance_policy.blocks.prepare_atomic_save_oracle();
    drop(execute_oracle(&mut worker, 1)); // actual successful empty Generate boundary
    assert!(worker.maintenance_policy.blocks.metrics().compressed_columns > 0);
    assert!(worker.runtime.backend.world.chunks.values().any(|c|
        c.blocks.is_cold() && !c.blocks.has_read_cache() && c.blocks.capacity() == 0));
    let storage = |worker: &Worker| {
        let mut values = worker.runtime.backend.world.chunks.iter().map(|(&p, c)|
            (p, c.blocks.is_cold(), c.blocks.has_read_cache(), c.blocks.capacity(),
                c.blocks.next_eligible_generation())).collect::<Vec<_>>();
        values.sort_unstable_by_key(|v| v.0); values
    };
    let cold_storage = storage(&worker);
    assert_eq!(serde_json::to_value(worker.runtime.checkpoint().unwrap()).unwrap(), before);
    worker.save().unwrap(); let cold_file = read_checkpoint(fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(storage(&worker), cold_storage, "validation/serialization must not hydrate live Cold storage");
    assert_eq!(serde_json::to_value(&dense_file).unwrap(), serde_json::to_value(&cold_file).unwrap());
    let (cold_ready, cold_reopened, cold_extra) = reopen(&worker);
    assert_updates_equal(&dense_ready, &cold_ready); drop((dense_ready, cold_ready));
    assert_eq!(dense_reopened, cold_reopened); assert_eq!(dense_extra, cold_extra);
    assert_eq!(storage(&worker), cold_storage);
}
