use super::*;
use sha1::{Digest, Sha1};
use std::io::Write;

// Generation still runs the real VanillaPopulation callbacks. Only independent
// entity constructor/Math seeds are controlled in the paired full-world oracle;
// otherwise equal drop requests intentionally create different item visuals.
pub(super) struct PublicationOracleHooks;
fn constructor_state(id: u64, family: u64) -> u64 {
    let mut random = crate::vanilla::random::JavaRandom::new(
        (id.wrapping_mul(0x9e3779b97f4a7c15) ^ family) as i64);
    random.next_long();
    random.next_long();
    random.state()
}
impl crate::vanilla::runtime::Hooks for PublicationOracleHooks {
    fn supports_inert_ore_writes(&self) -> bool {
        crate::vanilla::runtime::Hooks::supports_inert_ore_writes(&VanillaPopulation)
    }
    fn populate(&self, access: &mut crate::vanilla::runtime::Access<'_>, pos: ChunkPos,
        random: &mut crate::vanilla::random::JavaRandom) -> Result<()> {
        crate::vanilla::runtime::Hooks::populate(&VanillaPopulation, access, pos, random)
    }
    fn repopulate(&self, access: &mut crate::vanilla::runtime::Access<'_>, pos: ChunkPos) -> Result<bool> {
        crate::vanilla::runtime::Hooks::repopulate(&VanillaPopulation, access, pos)
    }
    fn initialize_light(&self, access: &mut crate::vanilla::runtime::Access<'_>, pos: ChunkPos) -> Result<bool> {
        crate::vanilla::runtime::Hooks::initialize_light(&VanillaPopulation, access, pos)
    }
    fn block_added(&self, access: &mut crate::vanilla::runtime::Access<'_>, p: IVec3,
        old: u16, new: u16) -> Result<()> {
        crate::vanilla::runtime::Hooks::block_added(&VanillaPopulation, access, p, old, new)
    }
    fn player_random_state(&self) -> u64 { constructor_state(0, 189) }
    fn item_random_state(&self, id: u64) -> u64 { constructor_state(id, 190) }
    fn orb_random_state(&self, id: i32) -> u64 { constructor_state(id as u64, 191) }
    fn falling_random_state(&self, id: u64) -> u64 { constructor_state(id, 192) }
}

fn pair() -> (Worker, Worker) {
    let config = Config::new(189, "spawn-publication", std::env::temp_dir());
    let (mut normal, _) = Worker::open(config.clone(), PathBuf::new(), Some(1234)).unwrap();
    let (mut deferred, _) = Worker::open(config, PathBuf::new(), Some(1234)).unwrap();
    deferred.runtime.backend.entities = normal.runtime.backend.entities.clone();
    normal.inventory_epoch = 1;
    deferred.inventory_epoch = 1;
    (normal, deferred)
}

pub(super) fn oracle_pair() -> (Worker, Worker) {
    let (mut normal, mut deferred) = pair();
    for worker in [&mut normal, &mut deferred] {
        let mut runtime = Runtime::new(worker.config.seed, 1234, worker.config.map_features,
            Arc::new(PublicationOracleHooks));
        runtime.use_overlay_journal(false);
        runtime.backend.world.label = worker.runtime.backend.world.label.clone();
        runtime.backend.world.save_path = worker.runtime.backend.world.save_path.clone();
        runtime.backend.entities.math_state = Some(constructor_state(0, 193));
        runtime.backend.entities.inventory_random = Some((constructor_state(0, 194), None));
        worker.runtime = runtime;
    }
    (normal, deferred)
}

fn request(worker: &mut Worker, command: Command) -> (Reply, Update) {
    validate_command(&command).unwrap();
    let deferred = matches!(&command, Command::LoadSpawnBatchDeferred { .. });
    let (reply, _) = worker.execute(command).unwrap();
    let update = worker.update_after_reply(&reply, deferred);
    (reply, update)
}

fn other(update: &Update) -> Value {
    let metadata = &update.metadata;
    let inventory = update.inventory.as_ref().map(|view| serde_json::json!({
        "stamp":view.stamp,"player":view.player,"slots":view.container_slots,"current":view.window_current,
    }));
    serde_json::json!({
        "reset":metadata.reset,"containers":metadata.containers,"removed_containers":metadata.removed_containers,
        "spawners":metadata.spawners,"removed_spawners":metadata.removed_spawners,
        "spawned":metadata.spawned,"drops":metadata.drops,"inventory":inventory,
        "falling":update.falling,"falling_effects":update.falling_effects,"items":update.items,
        "orbs":update.orbs,"experience":update.experience,"entity_effects":update.entity_effects,
        "player_frame":update.player_frame,"world_time":update.world_time,
    })
}

fn first_difference(a: &Value, b: &Value, path: &str) -> Option<String> {
    if a == b { return None; }
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            for (name, value) in a {
                let path = format!("{path}.{name}");
                let Some(actual) = b.get(name) else { return Some(path); };
                if let Some(path) = first_difference(value, actual, &path) { return Some(path); }
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() { return Some(format!("{path}.len ({} vs {})", a.len(), b.len())); }
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                if let Some(path) = first_difference(a, b, &format!("{path}[{index}]")) { return Some(path); }
            }
        }
        _ => {}
    }
    Some(path.into())
}
fn assert_other_equal(a: &Update, b: &Update, stage: usize) {
    let a = other(a);
    let b = other(b);
    if let Some(path) = first_difference(&a, &b, "update") {
        panic!("spawn publication stage {stage}: non-terrain field differs at {path}");
    }
}

struct HashWriter(Sha1);
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}
fn snapshot_digest(runtime: &Runtime) -> String {
    let mut snapshot = runtime.checkpoint().unwrap();
    snapshot.dirty_chunks.sort_unstable();
    snapshot.edits.sort_unstable_by_key(|(p, _)| *p);
    let mut writer = HashWriter(Sha1::new());
    serde_json::to_writer(&mut writer, &snapshot).unwrap();
    format!("{:x}", writer.0.finalize())
}

#[test]
fn deferred_spawn_progress_keeps_metadata_dirty_state_and_normal_commands_publish() {
    let (mut normal, mut deferred) = pair();
    request(&mut normal, Command::PrepareSpawn);
    request(&mut deferred, Command::PrepareSpawn);
    let mut retained = BTreeSet::new();
    for loaded in 1..=2 {
        let (reply_a, _) = normal.execute(Command::LoadSpawnBatch { count: 1 }).unwrap();
        let (reply_b, _) = deferred.execute(Command::LoadSpawnBatchDeferred { count: 1 }).unwrap();
        assert!(matches!(reply_a, Reply::SpawnAreaProgress { loaded: n } if n == loaded));
        assert!(matches!(reply_b, Reply::SpawnAreaProgress { loaded: n } if n == loaded));
        let chunk = normal.runtime.provider.loaded_order().last().copied().unwrap();
        let p = IVec3::new(chunk.0 * 16 + 7, 200, chunk.1 * 16 + 7);
        for worker in [&mut normal, &mut deferred] {
            worker.runtime.access().set_block(p, 54 << 4).unwrap();
            worker.runtime.backend.structure_entities.witches.push(p.to_array());
        }
        let before_dirty = deferred.runtime.backend.dirty_chunks.clone();
        let before_light = deferred.runtime.backend.light.clone().take_render_dirty();
        retained.extend(before_dirty.iter().copied());
        let a = normal.update_after_reply(&reply_a, false);
        let b = deferred.update_after_reply(&reply_b, true);
        assert!(!a.chunks.is_empty());
        assert!(b.chunks.is_empty() && b.remesh.is_empty());
        assert_other_equal(&a, &b, loaded);
        assert_eq!(deferred.runtime.backend.dirty_chunks, before_dirty);
        assert_eq!(deferred.runtime.backend.light.clone().take_render_dirty(), before_light);
        assert_eq!(normal.runtime.provider.loaded_order(), deferred.runtime.provider.loaded_order());
        assert_eq!(normal.runtime.provider.population_order, deferred.runtime.provider.population_order);
        assert_eq!(normal.runtime.backend.provider_random.state(), deferred.runtime.backend.provider_random.state());
        assert_eq!(normal.runtime.backend.world_random.state(), deferred.runtime.backend.world_random.state());
    }
    // A normal interactive request flushes retained terrain immediately; the
    // opt-in must not accidentally turn into a sticky session-wide suppression.
    let p = *retained.first().unwrap();
    let (_, normal_update) = request(&mut deferred, Command::Generate(vec![p]));
    let published = normal_update.chunks.iter().map(|(p, _)| *p).collect::<BTreeSet<_>>();
    assert!(published.is_superset(&retained));
    assert!(deferred.runtime.backend.dirty_chunks.is_empty());
    assert!(deferred.runtime.backend.light.clone().take_render_dirty().is_empty());
    request(&mut normal, Command::Generate(vec![p]));
    assert_eq!(snapshot_digest(&normal.runtime), snapshot_digest(&deferred.runtime));
    assert!(validate_command(&Command::LoadSpawnBatchDeferred { count: 0 }).is_err());
    assert!(validate_command(&Command::LoadSpawnBatchDeferred { count: 257 }).is_err());
}

#[test]
fn deferred_spawn_final_reply_and_resumed_owner_always_publish() {
    let (_, mut worker) = pair();
    request(&mut worker, Command::PrepareSpawn);
    let (_, partial) = request(&mut worker, Command::LoadSpawnBatchDeferred { count: 1 });
    assert!(partial.chunks.is_empty());
    // Exercise the final command boundary without repeating the expensive
    // complete625 test below in every ordinary unit run.
    worker.spawn_load_cursor = Some(624);
    let (reply, final_update) = request(&mut worker, Command::LoadSpawnBatchDeferred { count: 32 });
    assert!(matches!(reply, Reply::SpawnAreaProgress { loaded: 625 }));
    assert!(!final_update.chunks.is_empty());
    assert!(!worker.fresh_spawn_publication);
    assert!(worker.runtime.backend.dirty_chunks.is_empty());
    request(&mut worker, Command::PrepareSpawn);
    worker.runtime.backend.dirty_chunks.insert(worker.runtime.provider.loaded_order()[0]);
    assert!(!request(&mut worker, Command::LoadSpawnBatchDeferred { count: 1 }).1.chunks.is_empty());

    // Persist an interrupted initial load and exercise Worker::open's real
    // resumed path. A resumed session ignores the deferred-publication request.
    let root = std::env::temp_dir().join(format!("ferrum-spawn-publish-{}-{}", std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    let (_, mut worker) = pair();
    request(&mut worker, Command::PrepareSpawn);
    assert!(request(&mut worker, Command::LoadSpawnBatchDeferred { count: 1 }).1.chunks.is_empty());
    assert!(!worker.runtime.backend.dirty_chunks.is_empty());
    let config = Config::new(189, "resume", &root);
    let path = config.checkpoint_path().unwrap();
    worker.config = config.clone();
    worker.path = path.clone();
    worker.runtime.backend.world.save_path = path.clone();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    worker.save().unwrap();
    let (mut resumed, was_resumed) = Worker::open(config, path, None).unwrap();
    assert!(was_resumed && !resumed.fresh_spawn_publication);
    assert!(!resumed.update(true).chunks.is_empty(), "unpublished dirty chunks survive a checkpoint");
    request(&mut resumed, Command::PrepareSpawn);
    resumed.runtime.backend.dirty_chunks.insert(resumed.runtime.provider.loaded_order()[0]);
    assert!(!request(&mut resumed, Command::LoadSpawnBatchDeferred { count: 1 }).1.chunks.is_empty());
    drop(resumed);
    let canonical_root = fs::canonicalize(&root).unwrap();
    let canonical_temp = fs::canonicalize(std::env::temp_dir()).unwrap();
    assert_eq!(canonical_root.parent(), Some(canonical_temp.as_path()));
    assert!(canonical_root.file_name().unwrap().to_string_lossy().starts_with("ferrum-spawn-publish-"));
    fs::remove_dir_all(canonical_root).unwrap();
}

#[test]
#[ignore = "paired original625 publications and full checkpoint oracle; run alone with optimized tests"]
fn complete_spawn_deferred_publication_preserves_original625_and_first_neighborhood() {
    let (mut normal, mut deferred) = oracle_pair();
    normal.runtime.use_reference_publication(true);
    let mut views = [BTreeMap::new(), BTreeMap::new()];
    views[0].extend(request(&mut normal, Command::PrepareSpawn).1.chunks);
    views[1].extend(request(&mut deferred, Command::PrepareSpawn).1.chunks);
    let begin = Instant::now();
    for start in (0..625).step_by(32) {
        let (reply_a, a) = request(&mut normal, Command::LoadSpawnBatch { count: 32 });
        let (reply_b, b) = request(&mut deferred, Command::LoadSpawnBatchDeferred { count: 32 });
        let expected = (start + 32).min(625);
        assert!(matches!(reply_a, Reply::SpawnAreaProgress { loaded } if loaded == expected));
        assert!(matches!(reply_b, Reply::SpawnAreaProgress { loaded } if loaded == expected));
        assert_other_equal(&a, &b, expected);
        assert_eq!(normal.runtime.provider.loaded_order(), deferred.runtime.provider.loaded_order());
        assert_eq!(normal.runtime.provider.population_order, deferred.runtime.provider.population_order);
        assert_eq!(normal.runtime.backend.provider_random.state(), deferred.runtime.backend.provider_random.state());
        assert_eq!(normal.runtime.backend.world_random.state(), deferred.runtime.backend.world_random.state());
        if expected < 625 { assert!(b.chunks.is_empty() && b.remesh.is_empty()); }
        else { assert!(!b.chunks.is_empty()); }
        views[0].extend(a.chunks);
        views[1].extend(b.chunks);
    }
    assert_eq!(snapshot_digest(&normal.runtime), snapshot_digest(&deferred.runtime),
        "all blocks, biomes, lights, provider order, RNG, ticks, tiles and entities must match");
    let before = normal.runtime.runtime_timings();
    let after = deferred.runtime.runtime_timings();
    assert!(after.publication.parallel_calls > 0, "final625 must exercise pure parallel preparation");
    assert!(after.publication.maximum_prepared_columns <= 32);
    assert_eq!((before.access_reads, before.access_writes), (after.access_reads, after.access_writes));
    assert_eq!(views[0].len(), views[1].len());
    for (p, a) in &views[0] {
        let b = &views[1][p];
        assert!(a.blocks == b.blocks, "published blocks differ at {p:?}");
        assert!(a.biomes == b.biomes, "published biomes differ at {p:?}");
        assert_eq!(a.top, b.top, "published top differs at {p:?}");
        assert!(a.light.as_ref().unwrap().values == b.light.as_ref().unwrap().values,
            "published light differs at {p:?}");
        assert_eq!(a.random_tick_mask(), b.random_tick_mask(), "random index differs at {p:?}");
        assert!(a.scheduled_tick_candidates() == b.scheduled_tick_candidates(),
            "scheduled index differs at {p:?}");
    }
    let (reply_a, a) = request(&mut normal, Command::PreparePlayer { adventure: false });
    let (reply_b, b) = request(&mut deferred, Command::PreparePlayer { adventure: false });
    let (Reply::PlayerPrepared { initial: pa }, Reply::PlayerPrepared { initial: pb }) = (reply_a, reply_b)
        else { panic!("both modes must join only after the complete original barrier"); };
    views[0].extend(a.chunks);
    views[1].extend(b.chunks);
    // EntityPlayer constructors intentionally use independent seeds. Each
    // original pose must still have its complete lit3x3 ready for first play.
    for (view, player) in views.iter().zip([pa, pb]) {
        let center = (player.position[0].floor() as i32 >> 4, player.position[2].floor() as i32 >> 4);
        for dx in -1..=1 { for dz in -1..=1 {
            assert!(view.get(&(center.0 + dx, center.1 + dz)).is_some_and(|c| c.light.is_some()));
        } }
    }
    eprintln!("paired625 publication elapsed {:?}, ordinary owner publication {:.3}ms, deferred {:.3}ms",
        begin.elapsed(), normal.generation_timings.publication_ms, deferred.generation_timings.publication_ms);
}


fn inert_diagnostic_capacity(worker: &Worker) -> usize {
    (worker.runtime.backend.output.changed.capacity()
        + worker.runtime.backend.block_effects.changed.capacity()) * std::mem::size_of::<IVec3>()
}

fn inert_effect_fields(worker: &Worker) -> Value {
    let e = &worker.runtime.backend.block_effects;
    serde_json::json!({
        "changed":e.changed,"drops":e.drops,"washed":e.washed,
        "primed_tnt":e.primed_tnt,"note_updates":e.note_updates,
        "impacts":e.impacts.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>(),
        "sounds":e.sounds,"mix_smoke":e.mix_smoke,
    })
}

#[test]
fn omitted_native_inert_positions_preserve_publication_effects_checkpoint_and_resume() {
    let (mut retained, mut omitted) = oracle_pair();
    omitted.runtime.retain_inert_change_diagnostics(false);
    let run = |worker: &mut Worker, step| {
        let command = match step {
            0 => Command::PrepareSpawn,
            1 => Command::LoadSpawnBatchDeferred { count: 64 },
            2 => Command::Generate(vec![]),
            _ => Command::Generate(vec![(0,0),(1,0),(0,1),(1,1)]),
        };
        let deferred = matches!(&command, Command::LoadSpawnBatchDeferred { .. });
        let (reply, _) = worker.execute(command).unwrap();
        let lengths = (worker.runtime.backend.output.changed.len(), worker.runtime.backend.block_effects.changed.len());
        (reply, deferred, lengths)
    };
    for step in 0..4 {
        let (ra, da, lengths_a) = run(&mut retained, step);
        let (rb, db, lengths_b) = run(&mut omitted, step);
        if step == 1 {
            assert!(lengths_a.0 > lengths_b.0 && lengths_a.1 > lengths_b.1,
                "fixture must execute actual inert writes before the deferred publication");
        }
        // Existing non-position effect fields must survive the same publication.
        for worker in [&mut retained, &mut omitted] {
            worker.runtime.backend.block_effects.primed_tnt.push(IVec3::new(1,2,3));
            worker.runtime.backend.block_effects.note_updates.push((IVec3::new(2,3,4),true));
            worker.runtime.backend.block_effects.sounds.push(("random.fizz",0.5,0.8));
            worker.runtime.backend.block_effects.mix_smoke.push(IVec3::new(3,4,5));
        }
        let a = retained.update_after_reply(&ra, da);
        let b = omitted.update_after_reply(&rb, db);
        assert_eq!(format!("{ra:?}"),format!("{rb:?}"));
        assert_other_equal(&a,&b,step);
        assert_eq!(a.remesh,b.remesh);
        assert_eq!(a.chunks.iter().map(|(p,_)|p).collect::<Vec<_>>(),
            b.chunks.iter().map(|(p,_)|p).collect::<Vec<_>>());
        assert_eq!(snapshot_digest(&retained.runtime),snapshot_digest(&omitted.runtime));
        assert_eq!(retained.runtime.backend.world.light_revision,omitted.runtime.backend.world.light_revision);
        assert_eq!(inert_effect_fields(&retained),inert_effect_fields(&omitted));
        drop((a,b));
        if step == 2 {
            let mut left=retained.runtime.checkpoint().unwrap(); left.dirty_chunks.sort_unstable(); left.edits.sort_unstable_by_key(|(p,_)|*p);
            let mut right=omitted.runtime.checkpoint().unwrap(); right.dirty_chunks.sort_unstable(); right.edits.sort_unstable_by_key(|(p,_)|*p);
            let left_bytes=serde_json::to_vec(&left).unwrap();
            let right_bytes=serde_json::to_vec(&right).unwrap();
            assert_eq!(left_bytes,right_bytes);
            retained.runtime=Runtime::resume(serde_json::from_slice(&left_bytes).unwrap(),Arc::new(PublicationOracleHooks)).unwrap();
            omitted.runtime=Runtime::resume(serde_json::from_slice(&right_bytes).unwrap(),Arc::new(PublicationOracleHooks)).unwrap();
            for worker in [&mut retained,&mut omitted] {
                worker.runtime.use_overlay_journal(false); worker.published=Default::default();
            }
            omitted.runtime.retain_inert_change_diagnostics(false);
            let a=retained.update(true); let b=omitted.update(true);
            assert_other_equal(&a,&b,step);
            assert_eq!(snapshot_digest(&retained.runtime),snapshot_digest(&omitted.runtime));
        }
    }
}

#[test]
#[ignore="paired original625 native diagnostic traffic/capacity; run alone with optimized tests"]
fn native_inert_diagnostics_original625_profile_preserves_boundary_state() {
    let mut rows=Vec::new();
    let mut expected=None;
    for retain in [true,false,true,false] {
        let (mut worker, spare)=oracle_pair(); drop(spare);
        worker.runtime.retain_inert_change_diagnostics(retain);
        let begin=std::time::Instant::now();
        request(&mut worker,Command::PrepareSpawn);
        let mut max_capacity_bytes=0;
        let mut position_entries=0;
        for start in (0..625).step_by(32) {
            let (reply,_)=worker.execute(Command::LoadSpawnBatchDeferred {count:32}).unwrap();
            position_entries+=worker.runtime.backend.output.changed.len()
                + worker.runtime.backend.block_effects.changed.len();
            max_capacity_bytes=max_capacity_bytes.max(inert_diagnostic_capacity(&worker));
            drop(worker.update_after_reply(&reply,true));
            assert_eq!(worker.spawn_load_cursor,Some((start+32).min(625)));
        }
        let elapsed_ms=begin.elapsed().as_secs_f64()*1000.;
        let fingerprint=snapshot_digest(&worker.runtime);
        if let Some(expected)=&expected { assert_eq!(expected,&fingerprint); }
        else { expected=Some(fingerprint.clone()); }
        rows.push(serde_json::json!({"retain_inert_change_diagnostics":retain,
            "elapsed_ms":elapsed_ms,"position_entries_before_publication":position_entries,
            "max_retained_position_capacity_bytes":max_capacity_bytes,
            "final_retained_position_capacity_bytes":inert_diagnostic_capacity(&worker),
            "runtime":worker.runtime.runtime_timings(),"fingerprint":fingerprint}));
    }
    std::fs::create_dir_all("reports").unwrap();
    std::fs::write("reports/native-inert-diagnostics-profile.json",serde_json::to_vec_pretty(
        &serde_json::json!({"seed":189,"world_random_seed":1234,"runs":rows,
            "note":"Isolated diagnostic-policy measurement: deterministic PublicationOracleHooks does not opt into ore coordinate lookahead. Command/publication timings exclude full-state digest; capacity counts the two reused position vectors, not RSS. Runs alternate retained/omitted and retain the original625 request order; this is not production entry latency."})).unwrap()).unwrap();
}


#[test]
fn real_worker_open_disables_inert_diagnostics_for_new_and_resumed_native_owners() {
    use crate::vanilla::population::FeatureWorld;
    let directory=std::env::temp_dir().join(format!("ferrum-inert-diagnostic-open-{}-{}",
        std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    fs::create_dir_all(&directory).unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let (Ok(root),Ok(path))=(fs::canonicalize(std::env::temp_dir()),fs::canonicalize(&self.0)) else {return;};
            if path.parent()==Some(root.as_path()) && path.file_name().is_some_and(|v|
                v.to_string_lossy().starts_with("ferrum-inert-diagnostic-open-")) {
                let _=fs::remove_dir_all(path);
            }
        }
    }
    let _cleanup=Cleanup(directory.clone());
    let config=Config::new(189,"inert-diagnostic-open",directory.clone());
    let path=directory.join("native-world.json.gz");
    for expected_resume in [false,true] {
        // None uses the real lock/checkpoint branch, unlike disposable oracle_pair.
        let (mut worker,resumed)=Worker::open(config.clone(),path.clone(),None).unwrap();
        assert_eq!(resumed,expected_resume);
        worker.runtime.provide((0,0)).unwrap();
        let points=[IVec3::new(4,20,4),IVec3::new(4,21,4)];
        for &p in &points {worker.runtime.access().set_block(p,1<<4).unwrap();}
        drop(worker.update(false));
        worker.runtime.access().replace_ore_line(&points,15<<4,1);
        assert!(worker.runtime.backend.output.changed.is_empty());
        assert!(worker.runtime.backend.block_effects.changed.is_empty());
        for &p in &points {assert_eq!(worker.runtime.backend.world.get(p),15<<4);}
        assert!(!worker.runtime.backend.dirty_chunks.is_empty());
        // Eventful/non-ore writes retain their positions even in this owner.
        worker.runtime.access().set_block(points[0],35<<4).unwrap();
        assert!(worker.runtime.backend.output.changed.contains(&points[0]));
        assert!(worker.runtime.backend.block_effects.changed.contains(&points[0]));
        drop(worker.update(false));
        worker.save().unwrap();
        // End this scope's owner/OS lock before opening the durable checkpoint.
    }
}
