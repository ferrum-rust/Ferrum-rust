use super::*;

struct PublicationHooks;
impl PublicationHooks {
    fn assert_published(access: &Access<'_>) {
        access.with(|provider, backend| {
            assert_eq!(provider.loaded_order().len(),backend.world.chunks.len());
            assert!(backend.world.chunks.keys().all(|&p| provider.contains(p)));
        });
    }
}
impl Hooks for PublicationHooks {
    fn initialize_light(&self, access: &mut Access<'_>, pos: ChunkPos) -> Result<bool> {
        Self::assert_published(access);
        Ok(access.with(|provider,b| b.light.initialize_chunk(&b.world,pos,63,
            |p| provider.record(p).is_some_and(|r|r.terrain_populated))))
    }
    fn populate(&self, access: &mut Access<'_>, pos: ChunkPos, random: &mut JavaRandom) -> Result<()> {
        Self::assert_published(access);
        random.set_seed(i64::from(pos.0).wrapping_mul(341873128712)
            .wrapping_add(i64::from(pos.1).wrapping_mul(132897987541)));
        let before=random.next_int(1000);
        if pos==(0,0) {
            access.get_block(IVec3::new(32,70,0))?;
            access.get_block(IVec3::new(32,70,16))?;
        }
        Self::assert_published(access);
        let after=random.next_int(1000);
        let world=access.world_random().next_int(1000);
        access.with(|_,b| b.event(&format!("population:{before}:{after}:{world}"),pos));
        Ok(())
    }
}

fn comparable(runtime: &Runtime) -> serde_json::Value {
    let mut snapshot=runtime.checkpoint().unwrap();
    // This notification set has no iteration-order contract. Preserve the
    // provider's loaded/population order and every other checkpoint field.
    snapshot.dirty_chunks.sort_unstable();
    snapshot.edits.sort_unstable_by_key(|(p, _)| *p);
    let mut value=serde_json::to_value(snapshot).unwrap();
    value.as_object_mut().unwrap().remove("entities"); // independently seeded constructors
    value
}

struct WriteScratchHooks;
impl Hooks for WriteScratchHooks {
    fn populate(&self, _: &mut Access<'_>, _: ChunkPos, _: &mut JavaRandom) -> Result<()> { Ok(()) }
    fn block_added(&self, access: &mut Access<'_>, p: IVec3, old: u16, new: u16) -> Result<()> {
        access.with(|_, b| b.event(&format!("added:{old}:{new}"), (p.x, p.y)));
        if new == 41 << 4 {
            // Reenter while the outer result already contains its write. A
            // second buffer must preserve the original nested publication order.
            access.set_block_flags(p + IVec3::X, 57 << 4, 2)?;
            access.provide((2, 2))?;
            access.set_block_flags(p + IVec3::Y, 89 << 4, 2)?;
        }
        Ok(())
    }
}

#[test]
fn reused_write_results_preserve_nested_callbacks_failed_writes_and_checkpoint_effects() {
    let mut runs = Vec::new();
    for reference in [true, false] {
        let mut runtime = Runtime::new(189, 1234, false, Arc::new(WriteScratchHooks));
        runtime.backend.reference_write_scratch = reference;
        runtime.provide((0, 0)).unwrap();
        let at = IVec3::new(7, 70, 7);
        assert!(runtime.access().set_block_flags(at, 41 << 4, 2).unwrap());
        assert_eq!(runtime.backend.block_effects.changed,
            vec![at + IVec3::X, at + IVec3::Y, at]);
        assert!(runtime.backend.write_scratch.len() >= 2, "nested writes need distinct result storage");
        assert!(runtime.backend.write_scratch.len() <= 4);
        let allocation = runtime.backend.write_scratch.last().unwrap().changed.as_ptr();
        for state in [15 << 4, 16 << 4, 1 << 4, (1 << 4) | 3] {
            assert!(runtime.access().set_block(at, state).unwrap());
            if !reference {
                assert_eq!(runtime.backend.write_scratch.last().unwrap().changed.as_ptr(), allocation,
                    "ordinary ore writes reuse the same changed-vector allocation");
            }
        }
        let before_noop = comparable(&runtime);
        assert!(!runtime.access().set_block(at, (1 << 4) | 3).unwrap());
        assert!(!runtime.access().set_block(IVec3::new(7, -1, 7), 16).unwrap());
        assert!(!runtime.access().set_block(IVec3::new(7, 256, 7), 16).unwrap());
        assert!(comparable(&runtime) == before_noop, "no-op/invalid writes emit no new effects");
        runtime.provider.chunk_load_override = false;
        let before_failed = comparable(&runtime);
        assert!(!runtime.access().set_block(IVec3::new(64, 70, 64), 16).unwrap());
        assert!(comparable(&runtime) == before_failed, "disabled unloaded write must not change authoritative state");
        for result in &runtime.backend.write_scratch {
            assert!(result.changed.is_empty() && result.drops.is_empty() && result.washed.is_empty()
                && result.primed_tnt.is_empty() && result.note_updates.is_empty()
                && result.impacts.is_empty() && result.sounds.is_empty() && result.mix_smoke.is_empty());
        }
        let checkpoint = runtime.checkpoint().unwrap();
        let resumed = Runtime::resume(checkpoint, Arc::new(WriteScratchHooks)).unwrap();
        assert!(resumed.backend.write_scratch.is_empty(), "scratch is not checkpoint state");
        runs.push(comparable(&resumed));
    }
    for (field, expected) in runs[0].as_object().unwrap() {
        assert!(expected == &runs[1][field], "checkpoint field {field} differs with recycled write results");
    }
}

#[test]
fn prechecked_native_writes_preserve_load_reentry_callbacks_noops_and_precipitation() {
    let mut runs = Vec::new();
    for reference in [true, false] {
        let mut runtime = Runtime::new(189, 1234, false, Arc::new(WriteScratchHooks));
        runtime.backend.reference_prechecked_write = reference;
        runtime.use_overlay_journal(false);
        let at = IVec3::new(7, 200, 7);
        runtime.prepare_terrain(&[(0, 0), (2, 2)]).unwrap();
        runtime.provider.chunk_load_override = false;
        let before_disabled = comparable(&runtime);
        assert!(!runtime.access().set_block_flags(at, 41 << 4, 3).unwrap());
        assert!(before_disabled == comparable(&runtime), "preparation alone cannot bypass disabled provider loads");
        runtime.provider.finding_spawn = true;
        assert!(runtime.access().set_block_flags(at, 41 << 4, 3).unwrap());
        runtime.provider.finding_spawn = false;
        assert!(runtime.provider.contains((0, 0)) && runtime.provider.contains((2, 2)),
            "initial load and reentrant block-added load must both remain ordered");
        assert_eq!(runtime.backend.block_effects.changed,
            vec![at + IVec3::X, at + IVec3::Y, at]);
        let mut states = Vec::new();
        let capture = |runtime: &Runtime| (comparable(runtime),
            (runtime.backend.profile.access_reads, runtime.backend.profile.access_writes));
        states.push(capture(&runtime));
        let col = (at.z.rem_euclid(16) * 16 + at.x.rem_euclid(16)) as usize;
        for (state, cached_height, expected_height) in [
            (15 << 4, 202, 202), ((1 << 4) | 3, 201, -999), ((1 << 4) | 3, 200, -999),
        ] {
            runtime.backend.precipitation.get_mut(&cp(at)).unwrap()[col] = cached_height;
            let changed = runtime.backend.world.get(at) != state;
            // Reset a retained dirty record to verify the modified-cache path
            // cannot cause the subsequent real write to skip this flag.
            if changed { runtime.provider.record_mut(cp(at)).unwrap().modified = false; }
            assert_eq!(runtime.access().set_block_flags(at, state, 3).unwrap(), changed);
            assert_eq!(runtime.backend.precipitation[&cp(at)][col], expected_height);
            assert!(runtime.provider.record(cp(at)).unwrap().modified);
            states.push(capture(&runtime));
        }
        // The shared write body must still run immediate lava mixing and its
        // nested replacement/notification sequence, not just ordinary ores.
        let lava = IVec3::new(11, 200, 11);
        runtime.access().set_block(lava + IVec3::X, 9 << 4).unwrap();
        runtime.access().set_block_flags(lava, 10 << 4, 3).unwrap();
        assert_eq!(runtime.backend.world.get(lava), 49 << 4);
        states.push(capture(&runtime));
        let before_invalid = comparable(&runtime);
        for p in [IVec3::new(7, -1, 7), IVec3::new(7, 256, 7), IVec3::new(30_000_000, 20, 0)] {
            assert!(!runtime.access().set_block(p, 16).unwrap());
        }
        assert!(before_invalid == comparable(&runtime));
        let checkpoint = runtime.checkpoint().unwrap();
        let mut resumed = Runtime::resume(checkpoint, Arc::new(WriteScratchHooks)).unwrap();
        resumed.use_overlay_journal(false);
        resumed.backend.reference_prechecked_write = reference;
        assert!(resumed.access().set_block(at, 16 << 4).unwrap());
        states.push(capture(&resumed));
        runs.push(states);
    }
    for (stage, ((expected, expected_counts), (actual, actual_counts))) in runs[0].iter().zip(&runs[1]).enumerate() {
        for (field, value) in expected.as_object().unwrap() {
            assert!(value == &actual[field], "prechecked stage {stage} differs in checkpoint field {field}");
        }
        assert_eq!(expected_counts, actual_counts, "logical access counts at stage {stage}");
    }
}

#[test]
fn loaded_access_fast_path_preserves_disabled_loads_preparation_reentry_and_rng() {
    let mut runs=Vec::new();
    for reference in [true,false] {
        let mut runtime=Runtime::new(189,1234,false,Arc::new(PublicationHooks));
        runtime.backend.reference_access=reference;
        runtime.provider.chunk_load_override=false;
        let original=comparable(&runtime);
        let requests=[(0,0),(0,1),(1,0),(1,1),(2,0),(2,1)];
        runtime.prepare_terrain(&requests).unwrap();
        assert_eq!(runtime.access().get_block(IVec3::new(0,70,0)).unwrap(),0);
        assert_eq!(comparable(&runtime),original,"prepared but unpublished terrain is still EmptyChunk when loading is disabled");
        runtime.provider.finding_spawn=true;
        runtime.access().get_block(IVec3::new(0,70,0)).unwrap();
        runtime.provider.finding_spawn=false;
        let loaded=comparable(&runtime);
        for _ in 0..20 {
            let expected=runtime.backend.world.get(IVec3::new(0,70,0));
            assert_eq!(runtime.access().get_block(IVec3::new(0,70,0)).unwrap(),expected);
        }
        for p in [IVec3::new(0,-1,0),IVec3::new(0,256,0),IVec3::new(-30_000_001,70,0),
            IVec3::new(30_000_000,70,0),IVec3::new(16,70,0)] {
            assert_eq!(runtime.access().get_block(p).unwrap(),0);
        }
        assert_eq!(comparable(&runtime),loaded,"loaded/invalid/disabled reads cannot consume RNG or lifecycle events");
        runtime.provider.chunk_load_override=true;
        for pos in &requests[..4] { runtime.provide(*pos).unwrap(); }
        assert!(runtime.provider.contains((2,0))&&runtime.provider.contains((2,1)),"population must perform its original nested loads");
        let p=IVec3::new(4,70,4);
        runtime.access().set_block(p,89<<4).unwrap();
        assert_eq!(runtime.access().get_block(p).unwrap(),89<<4,"direct reads must observe current storage after writes");
        let snapshot=runtime.checkpoint().unwrap();
        let mut resumed=Runtime::resume(snapshot,Arc::new(PublicationHooks)).unwrap();
        resumed.backend.reference_access=reference;
        resumed.provider.chunk_load_override=false;
        assert_eq!(resumed.access().get_block(p).unwrap(),89<<4);
        assert_eq!(resumed.access().get_block(IVec3::new(64,70,0)).unwrap(),0);
        runs.push(comparable(&resumed));
    }
    for (field, expected) in runs[0].as_object().unwrap() {
        // A failed full snapshot dump contains millions of block/light values
        // and can itself exhaust memory while another build is linking.
        assert!(expected == &runs[1][field], "checkpoint field {field} differs between access paths");
    }
}
