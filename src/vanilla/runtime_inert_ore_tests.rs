use super::*;

#[test]
fn inert_ore_whitelist_keeps_opaque_nonemitting_lighting_contract() {
    use crate::vanilla::material::{light_opacity, light_value};
    for source in [1, 87] {
        for metadata in 0..16 {
            let old = source << 4 | metadata;
            assert_eq!((light_opacity(old), light_value(old)), (255, 0));
        }
        for target in 0..=u16::MAX {
            if inert_replacement(target, source) {
                assert_eq!((light_opacity(target), light_value(target)), (255, 0),
                    "inert ore target {target} violates the lighting fast-path contract");
            }
        }
    }
}

struct InertHooks;
impl Hooks for InertHooks {
    fn supports_inert_ore_writes(&self) -> bool { true }
    fn populate(&self, _: &mut Access<'_>, _: ChunkPos, _: &mut JavaRandom) -> Result<()> { Ok(()) }
    fn player_random_state(&self) -> u64 { JavaRandom::new(189).state() }
}

// Both variants resume the same checkpoint. Keep the entire entity allocator
// and its RNG in the comparison, including inventory spills from stale tiles.
struct Captured {
    snapshot: Vec<u8>,
    reads: u64,
    writes: u64,
    light_revision: u64,
    indexes: Vec<(ChunkPos, u16, Vec<(u16, u16)>)>,
}
fn capture(runtime: &Runtime) -> Captured {
    let mut snapshot = runtime.checkpoint().unwrap();
    snapshot.dirty_chunks.sort_unstable();
    snapshot.edits.sort_unstable_by_key(|(p, _)| *p);
    let indexes = runtime.provider.loaded_order().iter().map(|p| {
        let c = &runtime.backend.world.chunks[p];
        (*p, c.random_tick_mask(), c.scheduled_tick_candidates().to_vec())
    }).collect::<Vec<_>>();
    Captured {
        snapshot: serde_json::to_vec(&snapshot).unwrap(),
        reads: runtime.backend.profile.access_reads,
        writes: runtime.backend.profile.access_writes,
        light_revision: runtime.backend.world.light_revision,
        indexes,
    }
}

fn same(expected: &Captured, actual: &Captured, stage: usize) {
    // Store compact bytes while running both variants; only expand a failed
    // snapshot for a field-level diagnostic instead of retaining huge Values.
    if expected.snapshot != actual.snapshot {
        let a: serde_json::Value = serde_json::from_slice(&expected.snapshot).unwrap();
        let b: serde_json::Value = serde_json::from_slice(&actual.snapshot).unwrap();
        for (name, value) in a.as_object().unwrap() {
            assert!(value == &b[name], "inert ore stage {stage}: snapshot {name} differs");
        }
    }
    assert_eq!(expected.reads, actual.reads, "stage {stage}: logical reads");
    assert_eq!(expected.writes, actual.writes, "stage {stage}: writes");
    assert_eq!(expected.light_revision, actual.light_revision, "stage {stage}: light revision");
    assert_eq!(expected.indexes, actual.indexes, "stage {stage}: prepared indexes");
}

fn source(runtime: &mut Runtime, points: &[IVec3], state: u16) {
    for &p in points { runtime.access().set_block(p, state).unwrap(); }
}

#[test]
fn inert_ore_preserves_each_write_lighting_indexes_cow_precipitation_and_resume() {
    for no_sky in [false, true] {
        let mut initial = Runtime::new(189, 1234, false, Arc::new(InertHooks));
        for p in [(-1, -1), (0, 0), (0, 1)] { initial.provide(p).unwrap(); }
        let points = vec![IVec3::new(-1, 0, -1), IVec3::new(-1, 20, -1),
            IVec3::new(4, 20, 14), IVec3::new(4, 20, 15), IVec3::new(4, 20, 16),
            IVec3::new(4, 255, 0)];
        source(&mut initial, &points, 16);
        initial.backend.light = Lighting::new(&initial.backend.world, no_sky);
        let checkpoint = initial.checkpoint().unwrap();
        for journal in [false, true] {
            let mut runs = Vec::new();
            for reference in [true, false] {
                let mut runtime = Runtime::resume(checkpoint.clone(), Arc::new(InertHooks)).unwrap();
                runtime.backend.reference_inert_ore = reference;
                runtime.use_overlay_journal(journal);
                for c in runtime.backend.world.chunks.values() { c.prepare_tick_indexes(); }
                let original = runtime.backend.world.chunks[&(0, 0)].clone();
                let old_blocks = original.blocks.clone();
                let mut states = Vec::new();
                let before_empty = capture(&runtime);
                runtime.access().replace_ore_line(&[], 17, 1);
                same(&before_empty, &capture(&runtime), 0);
                // Every exact allowed target is exercised with repeated
                // candidates and ordered intermediate checkpoint comparisons.
                for ore in [17, 18, 19, 20, 21, 22, 48, 224, 240, 256, 336, 896, 1168, 2064, 2448, 16] {
                    source(&mut runtime, &points, 16);
                    for &p in &points {
                        let col = (p.z.rem_euclid(16) * 16 + p.x.rem_euclid(16)) as usize;
                        runtime.backend.precipitation.get_mut(&cp(p)).unwrap()[col] = p.y + 1;
                        runtime.provider.record_mut(cp(p)).unwrap().modified = false;
                        runtime.access().replace_ore_line(&[p, p], ore, 1);
                        assert_eq!(runtime.backend.world.get(p), ore);
                        assert_eq!(runtime.backend.precipitation[&cp(p)][col], -999);
                        if ore != 16 { assert!(runtime.provider.record(cp(p)).unwrap().modified); }
                    }
                    states.push(capture(&runtime));
                }
                assert_eq!(original.blocks, old_blocks, "retained worker chunk changed through COW");
                assert!(!Arc::ptr_eq(&original, &runtime.backend.world.chunks[&(0, 0)]));
                source(&mut runtime, &points, (87 << 4) | 3);
                runtime.access().replace_ore_line(&points, 153 << 4, 87);
                states.push(capture(&runtime));
                if !reference { assert!(runtime.backend.inert_ore_writes > 0); }
                else { assert_eq!(runtime.backend.inert_ore_writes, 0); }
                let mut resumed = Runtime::resume(runtime.checkpoint().unwrap(), Arc::new(InertHooks)).unwrap();
                resumed.backend.reference_inert_ore = reference;
                resumed.use_overlay_journal(journal);
                assert_eq!(resumed.backend.inert_ore_writes, 0, "optimization counters are transient");
                source(&mut resumed, &points, 16);
                resumed.access().replace_ore_line(&points, 17, 1);
                states.push(capture(&resumed));
                runs.push(states);
            }
            for (stage, (expected, actual)) in runs[0].iter().zip(&runs[1]).enumerate() {
                same(expected, actual, stage);
            }
        }
    }
}

#[test]
fn inert_ore_falls_back_for_stale_tiles_and_preserves_spill_rng_and_effect_order() {
    use crate::{inventory::Stack, vanilla::drops::{DropNbt, DropTile}};
    let mut initial = Runtime::new(189, 1234, false, Arc::new(InertHooks));
    initial.provide((0, 0)).unwrap();
    let points = (0..6).map(|z| IVec3::new(4, 20, z)).collect::<Vec<_>>();
    source(&mut initial, &points, 16);
    initial.backend.entities.inventory_random = Some((JavaRandom::new(54321).state(), None));
    initial.backend.entities.math_state = Some(JavaRandom::new(6789).state());
    let checkpoint = initial.checkpoint().unwrap();
    let mut runs = Vec::new();
    for reference in [true, false] {
        let mut runtime = Runtime::resume(checkpoint.clone(), Arc::new(InertHooks)).unwrap();
        runtime.backend.reference_inert_ore = reference;
        let mut chest = LiveContainer::empty(points[1].to_array(), 54, 1, 27);
        chest.slots[0] = Some(Stack::new(264, 49));
        runtime.backend.tile_revision = 1;
        runtime.backend.live_containers.insert(points[1].to_array(), chest);
        runtime.backend.live_spawners.insert(points[2].to_array(), "Pig".into());
        runtime.backend.drop_tiles.insert(points[3].to_array(), DropTile::Piston { state: 16 });
        let local = [points[4].x, points[4].y, points[4].z];
        Arc::make_mut(runtime.backend.world.chunks.get_mut(&(0, 0)).unwrap()).tiles.insert(
            local, Arc::new(DropNbt::Compound(BTreeMap::from([("id".into(), DropNbt::String("Chest".into()))]))));
        let retained = runtime.backend.world.chunks[&(0, 0)].clone();
        let before_effects = runtime.backend.block_effects.changed.len();
        runtime.access().replace_ore_line(&points, 16 << 4, 1);
        assert_eq!(&runtime.backend.block_effects.changed[before_effects..], points.as_slice());
        assert!(runtime.backend.live_containers.is_empty() && runtime.backend.live_spawners.is_empty());
        assert!(runtime.backend.drop_tiles.is_empty());
        assert!(runtime.backend.world.chunks[&(0, 0)].tiles.is_empty());
        assert!(retained.tiles.contains_key(&local), "guarded fallback must also retain COW metadata");
        assert!(!runtime.backend.drops.is_empty(), "stale inventory must still spill its contents");
        assert_eq!(runtime.backend.entities.live.values().map(|e| u32::from(e.stack.count)).sum::<u32>(), 49);
        assert_eq!(runtime.backend.inert_ore_writes, if reference { 0 } else { 2 },
            "only the two metadata-free cells may bypass callbacks");
        runs.push(capture(&runtime));
    }
    same(&runs[0], &runs[1], 0);
}

struct ReentrantInertHooks;
impl Hooks for ReentrantInertHooks {
    fn supports_inert_ore_writes(&self) -> bool { true }
    fn populate(&self, access: &mut Access<'_>, pos: ChunkPos, random: &mut JavaRandom) -> Result<()> {
        let before = random.next_int(1_000);
        if pos == (0, 0) {
            access.provide((2, 1))?;
            source_access(access, IVec3::new(4, 20, 1), 16);
        }
        let after = random.next_int(1_000);
        let world = access.world_random().next_int(1_000);
        access.with(|_, b| b.event(&format!("inert-populate:{before}:{after}:{world}"), pos));
        Ok(())
    }
}
fn source_access(access: &mut Access<'_>, p: IVec3, state: u16) {
    access.set_block(p, state).unwrap();
}

#[test]
fn inert_ore_preserves_missing_load_population_reentry_invalid_and_disabled_coordinates() {
    let mut initial = Runtime::new(189, 1234, false, Arc::new(ReentrantInertHooks));
    for pos in [(0, 0), (0, 1), (1, 0)] { initial.provide(pos).unwrap(); }
    source(&mut initial, &[IVec3::new(4, 20, 0)], 16);
    source(&mut initial, &[IVec3::new(4, 20, 1)], 4 << 4);
    initial.prepare_terrain(&[(1, 1)]).unwrap();
    let checkpoint = initial.checkpoint().unwrap();
    let mut runs = Vec::new();
    for reference in [true, false] {
        let mut runtime = Runtime::resume(checkpoint.clone(), Arc::new(ReentrantInertHooks)).unwrap();
        runtime.backend.reference_inert_ore = reference;
        let line = [IVec3::new(4, 20, 0), IVec3::new(16, 20, 16), IVec3::new(4, 20, 1),
            IVec3::new(4, -1, 0), IVec3::new(4, 256, 0), IVec3::new(-30_000_001, 20, 0),
            IVec3::new(30_000_000, 20, 0)];
        runtime.access().replace_ore_line(&line, 14 << 4, 1);
        assert_eq!(runtime.backend.world.get(IVec3::new(4, 20, 1)), 14 << 4);
        assert!(runtime.provider.population_order.contains(&(0, 0)));
        assert!(runtime.provider.contains((2, 1)));
        let mut stages = vec![capture(&runtime)];
        runtime.prepare_terrain(&[(6, 6)]).unwrap();
        runtime.provider.chunk_load_override = false;
        let before = runtime.provider.loaded_order().to_vec();
        runtime.access().replace_ore_line(&[IVec3::new(96, 20, 96)], 17, 1);
        assert_eq!(runtime.provider.loaded_order(), before.as_slice());
        assert!(!runtime.provider.contains((6, 6)));
        stages.push(capture(&runtime));
        runs.push(stages);
    }
    for (stage, (expected, actual)) in runs[0].iter().zip(&runs[1]).enumerate() {
        same(expected, actual, stage);
    }
}

struct EventfulHooks;
impl Hooks for EventfulHooks {
    fn populate(&self, _: &mut Access<'_>, _: ChunkPos, _: &mut JavaRandom) -> Result<()> { Ok(()) }
    fn block_added(&self, access: &mut Access<'_>, p: IVec3, old: u16, new: u16) -> Result<()> {
        let n = access.world_random().next_int(1_000);
        access.with(|_, b| b.event(&format!("custom-added:{old}:{new}:{n}"), (p.x, p.z)));
        if p == IVec3::new(4, 20, 0) && new == 16 << 4 {
            access.set_block(IVec3::new(4, 20, 1), 4 << 4)?;
        }
        Ok(())
    }
}

#[test]
fn unopted_hooks_and_eventful_target_states_keep_the_original_adapter() {
    let mut runtime = Runtime::new(189, 1234, false, Arc::new(EventfulHooks));
    runtime.provide((0, 0)).unwrap();
    let points = [IVec3::new(4, 20, 0), IVec3::new(4, 20, 1)];
    source(&mut runtime, &points, 16);
    let before = runtime.backend.events.len();
    runtime.access().replace_ore_line(&points, 16 << 4, 1);
    assert_eq!(runtime.backend.world.get(points[1]), 4 << 4,
        "a custom callback must change the next predicate before it is read");
    assert!(runtime.backend.events.len() > before);
    assert_eq!(runtime.backend.inert_ore_writes, 0);
    // Opt in here so capability rejection cannot mask a whitelist regression.
    let mut runtime = Runtime::new(189, 1234, false, Arc::new(InertHooks));
    runtime.provide((0, 0)).unwrap();
    source(&mut runtime, &points, 16);
    let snapshot = capture(&runtime);
    for (target, source) in [(13 << 4, 1), (74 << 4, 1), (89 << 4, 1), (97 << 4, 1), (16 << 4, 97)] {
        assert!(!runtime.access().replace_inert_ore_line(&points, target, source));
    }
    same(&snapshot, &capture(&runtime), 0);
}



#[test]
fn native_inert_diagnostic_policy_preserves_default_resume_order_and_eventful_fallbacks() {
    let mut initial = Runtime::new(189, 1234, false, Arc::new(InertHooks));
    initial.provide((0, 0)).unwrap();
    let points = [IVec3::new(4, 20, 4), IVec3::new(4, 21, 4), IVec3::new(4, 22, 4)];
    source(&mut initial, &points, 1 << 4);
    let checkpoint = initial.checkpoint().unwrap();
    // Normal scoped-column, existing per-cell inert fallback, and eventful
    // adapter fallback. Only the first two may omit inert diagnostic positions.
    for mode in 0..3 {
        let mut expected = None;
        for retain in [true, false] {
            let mut runtime = Runtime::resume(checkpoint.clone(), Arc::new(InertHooks)).unwrap();
            assert!(runtime.backend.record_inert_change_diagnostics);
            runtime.backend.reference_inert_column = mode == 1;
            runtime.backend.reference_inert_ore = mode == 2;
            runtime.backend.output.changed.clear();
            runtime.backend.block_effects.changed.clear();
            runtime.retain_inert_change_diagnostics(retain);
            runtime.access().replace_ore_line(&points, 15 << 4, 1);
            let expected_positions = if retain || mode == 2 { points.as_slice() } else { &[] };
            assert_eq!(runtime.backend.output.changed, expected_positions, "mode {mode}, retain {retain}");
            assert_eq!(runtime.backend.block_effects.changed, expected_positions);
            assert_eq!(runtime.backend.inert_column_writes > 0, mode == 0);
            assert_eq!(runtime.backend.inert_ore_writes > 0, mode != 2);
            let saved = runtime.checkpoint().unwrap();
            let restored = Runtime::resume(saved, Arc::new(InertHooks)).unwrap();
            assert!(restored.backend.record_inert_change_diagnostics,
                "an execution policy must not persist in an ordinary Runtime snapshot");
            assert_eq!(restored.backend.output.changed, expected_positions);
            assert_eq!(restored.backend.block_effects.changed, expected_positions);
            // These are exactly the two lists cleared by NativeSession before
            // another command can request a checkpoint. Compare everything else.
            runtime.backend.output.changed.clear();
            runtime.backend.block_effects.changed.clear();
            let actual = capture(&runtime);
            if let Some(expected) = &expected { same(expected, &actual, mode); }
            else { expected = Some(actual); }
        }
    }
}


#[test]
fn inert_ore_scanner_cold_reads_and_changed_fallback_match_original_adapter() {
    use crate::inventory::Stack;
    let pos=(-1,-1);
    let points=(0..6).map(|i|IVec3::new(-4,20,-12+i)).collect::<Vec<_>>();
    let mut initial=Runtime::new(189,1234,false,Arc::new(InertHooks));
    initial.provide(pos).unwrap();
    source(&mut initial,&points,16);
    source(&mut initial,&[points[1],points[4]],48);
    initial.backend.entities.inventory_random=Some((JavaRandom::new(54321).state(),None));
    initial.backend.entities.math_state=Some(JavaRandom::new(6789).state());
    initial.backend.light=Lighting::new(&initial.backend.world,false);
    let checkpoint=initial.checkpoint().unwrap();
    for cold in [false,true] {
        let mut runs=Vec::new();
        for reference in [true,false] {
            let mut runtime=Runtime::resume(checkpoint.clone(),Arc::new(InertHooks)).unwrap();
            runtime.backend.reference_inert_ore=reference;
            {
                let chunk=Arc::get_mut(runtime.backend.world.chunks.get_mut(&pos).unwrap()).unwrap();
                chunk.prepare_tick_indexes();
                if cold {assert!(chunk.blocks.compress_if_saving(1024));}
                assert!(!chunk.blocks.has_read_cache());
                assert!(!chunk.blocks.take_recent_cold_access());
            }
            let mut stages=Vec::new();
            let original_ptr=Arc::as_ptr(&runtime.backend.world.chunks[&pos]);
            // Empty and invalid candidates must never speculatively hydrate the
            // otherwise cold column. The original validity/read counters remain.
            runtime.access().replace_ore_line(&[],16,1);
            runtime.access().replace_ore_line(&[IVec3::new(-4,-1,-12),IVec3::new(-4,256,-12)],16,1);
            {
                let chunk=Arc::get_mut(runtime.backend.world.chunks.get_mut(&pos).unwrap()).unwrap();
                assert_eq!(chunk.blocks.is_cold(),cold);
                assert!(!chunk.blocks.has_read_cache());assert!(!chunk.blocks.take_recent_cold_access());
            }
            stages.push(capture(&runtime));
            let reads=runtime.backend.profile.access_reads;
            let writes=runtime.backend.profile.access_writes;
            runtime.backend.precipitation.get_mut(&pos).unwrap()[(points[0].z.rem_euclid(16)*16+points[0].x.rem_euclid(16)) as usize]=21;
            runtime.access().replace_ore_line(&[points[0],points[1],points[0],points[1]],16,1);
            assert_eq!(runtime.backend.profile.access_reads-reads,6);
            assert_eq!(runtime.backend.profile.access_writes,writes);
            assert_eq!(Arc::as_ptr(&runtime.backend.world.chunks[&pos]),original_ptr);
            {
                let chunk=Arc::get_mut(runtime.backend.world.chunks.get_mut(&pos).unwrap()).unwrap();
                assert_eq!(chunk.blocks.is_cold(),cold);
                assert_eq!(chunk.blocks.has_read_cache(),cold);
                assert_eq!(chunk.blocks.take_recent_cold_access(),cold);
            }
            stages.push(capture(&runtime));
            // Hold the pre-write snapshot to force real COW. A fresh slice must
            // be acquired after each changed write/fallback, never retained
            // across a write to the native owner's current column.
            let retained=runtime.backend.world.chunks[&pos].clone();
            let expected=retained.blocks.with_slice(|states|states.to_vec());
            // Insert stale metadata only immediately before the write, so the
            // checkpoint oracle never has to accept an invalid pre-write tile.
            let mut container=LiveContainer::empty(points[2].to_array(),54,1,27);
            container.slots[0]=Some(Stack::new(264,49));
            runtime.backend.live_containers.insert(points[2].to_array(),container);
            let effects=runtime.backend.block_effects.changed.len();
            runtime.access().replace_ore_line(&[points[0],points[1],points[2],points[3],points[0],points[4],points[5]],14<<4,1);
            assert!(!Arc::ptr_eq(&retained,&runtime.backend.world.chunks[&pos]));
            assert!(runtime.backend.world.chunks[&pos].blocks.is_dense());
            assert_eq!(retained.blocks.is_cold(),cold);
            retained.blocks.with_slice(|states|assert_eq!(states,expected.as_slice()));
            assert_eq!(&runtime.backend.block_effects.changed[effects..],&[points[0],points[2],points[3],points[5]]);
            assert!(runtime.backend.live_containers.is_empty());
            assert_eq!(runtime.backend.entities.live.values().map(|e|u32::from(e.stack.count)).sum::<u32>(),49);
            assert_eq!(runtime.backend.inert_ore_writes,if reference {0} else {3});
            {
                let chunk=Arc::get_mut(runtime.backend.world.chunks.get_mut(&pos).unwrap()).unwrap();
                assert_eq!(chunk.blocks.take_recent_cold_access(),cold);
            }
            for (i,&p) in points.iter().enumerate() {
                assert_eq!(runtime.backend.world.get(p),if matches!(i,1|4) {48} else {14<<4});
            }
            stages.push(capture(&runtime));
            // Full checkpoint bytes, prepared indexes, logical reads/writes and
            // light revision also cover provider/RNG/order/precipitation state.
            runs.push(stages);
        }
        for (stage,(expected,actual)) in runs[0].iter().zip(&runs[1]).enumerate() {
            same(expected,actual,stage);
        }
    }
}
