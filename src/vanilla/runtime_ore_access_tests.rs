use super::*;

struct OreHooks;
impl Hooks for OreHooks {
    fn populate(&self, access: &mut Access<'_>, pos: ChunkPos, random: &mut JavaRandom) -> Result<()> {
        let before = random.next_int(1_000);
        if pos == (0, 0) {
            // A missing-column read can recursively load/populate another
            // column and change a position that this outer line already read.
            access.get_block(IVec3::new(32, 20, 16))?;
            access.set_block(IVec3::new(4, 20, 15), 1 << 4)?;
        }
        let after = random.next_int(1_000);
        let world = access.world_random().next_int(1_000);
        access.with(|_, b| b.event(&format!("ore-populate:{before}:{after}:{world}"), pos));
        Ok(())
    }
    fn block_added(&self, access: &mut Access<'_>, p: IVec3, old: u16, new: u16) -> Result<()> {
        access.with(|_, b| b.event(&format!("ore-added:{:?}:{old}:{new}", p.to_array()), cp(p)));
        if p == IVec3::new(4, 20, 0) && new == 41 << 4 {
            // The write changes whether subsequent candidates match. Reading
            // all candidate states before writing would fail this fixture.
            access.set_block(IVec3::new(4, 20, 1), 4 << 4)?;
            access.set_block(IVec3::new(4, 20, 2), 1 << 4)?;
            access.replace_ore_line(&[IVec3::new(4, 20, 6), IVec3::new(4, 20, 7)], 14 << 4, 1);
            access.provide((2, 0))?;
        }
        Ok(())
    }
}

fn comparable(runtime: &Runtime) -> serde_json::Value {
    let mut snapshot = runtime.checkpoint().unwrap();
    snapshot.dirty_chunks.sort_unstable();
    snapshot.edits.sort_unstable_by_key(|(p, _)| *p);
    let mut value = serde_json::to_value(snapshot).unwrap();
    // Constructor entity RNG is independently seeded; no entities are created
    // by these features. All provider/world RNG and callback effects are kept.
    value.as_object_mut().unwrap().remove("entities");
    value
}

#[test]
fn ordered_native_ore_lines_preserve_nested_writes_missing_loads_metadata_and_rng() {
    let mut runs = Vec::new();
    for reference in [true, false] {
        let mut runtime = Runtime::new(189, 1234, false, Arc::new(OreHooks));
        runtime.backend.reference_ore_batch = reference;
        for pos in [(0, 0), (0, 1), (1, 0)] { runtime.provide(pos).unwrap(); }
        for z in 0..19 {
            runtime.access().set_block(IVec3::new(4, 20, z), if z == 2 { 4 << 4 } else { 1 << 4 }).unwrap();
        }
        for z in 0..4 { runtime.access().set_block(IVec3::new(5, 20, z), (1 << 4) | 3).unwrap(); }
        for z in 0..4 { runtime.access().set_block(IVec3::new(6, 20, z), (87 << 4) | (z as u16 & 3)).unwrap(); }
        let unchanged = comparable(&runtime);
        let counts = (runtime.backend.profile.access_reads, runtime.backend.profile.access_writes);
        runtime.access().replace_ore_line(&[], 41 << 4, 1);
        assert!(unchanged == comparable(&runtime));
        assert_eq!(counts, (runtime.backend.profile.access_reads, runtime.backend.profile.access_writes));

        let mut line = (0..19).map(|z| IVec3::new(4, 20, z)).collect::<Vec<_>>();
        line.extend([IVec3::new(4, -1, 0), IVec3::new(4, 256, 0),
            IVec3::new(-30_000_001, 20, 0), IVec3::new(30_000_000, 20, 0),
            IVec3::new(16, 20, 16), IVec3::new(4, 20, 15)]);
        runtime.access().replace_ore_line(&line, 41 << 4, 1);
        assert_eq!(runtime.backend.world.get(IVec3::new(4, 20, 1)), 4 << 4);
        assert_eq!(runtime.backend.world.get(IVec3::new(4, 20, 2)), 41 << 4);
        assert_eq!(runtime.backend.world.get(IVec3::new(4, 20, 6)), 14 << 4);
        assert_eq!(runtime.backend.world.get(IVec3::new(4, 20, 7)), 14 << 4);
        assert_eq!(runtime.backend.world.get(IVec3::new(4, 20, 15)), 41 << 4,
            "reentrant missing-column population restored stone before its revisit");
        assert!(runtime.provider.population_order.contains(&(0, 0)));
        assert!(runtime.provider.contains((2, 1)), "missing reads must retain nested provider loads");

        // Same block-ID metadata continues to match and must retain the original
        // set-attempt effects even when this position was already written.
        let metadata = [IVec3::new(5, 20, 0), IVec3::new(5, 20, 0), IVec3::new(5, 20, 1), IVec3::new(5, 20, 0)];
        runtime.access().replace_ore_line(&metadata, (1 << 4) | 5, 1);
        runtime.access().replace_ore_line(&(0..4).map(|z| IVec3::new(6, 20, z)).collect::<Vec<_>>(), 153 << 4, 87);
        assert_eq!(runtime.backend.world.get(IVec3::new(5, 20, 0)), (1 << 4) | 5);
        assert_eq!(runtime.backend.world.get(IVec3::new(6, 20, 3)), 153 << 4);

        runtime.provider.chunk_load_override = false;
        let before_disabled = comparable(&runtime);
        let disabled = [IVec3::new(96, 20, 96), IVec3::new(4, -1, 0), IVec3::new(30_000_000, 20, 0)];
        for source in [1, 0] { runtime.access().replace_ore_line(&disabled, 16, source); }
        assert!(before_disabled == comparable(&runtime), "dummy/invalid air cannot become a real chunk or block");

        let counts = (runtime.backend.profile.access_reads, runtime.backend.profile.access_writes);
        let next_provider_random = runtime.backend.provider_random.next_long();
        let next_world_random = runtime.backend.world_random.next_long();
        let checkpoint = runtime.checkpoint().unwrap();
        let mut resumed = Runtime::resume(checkpoint, Arc::new(OreHooks)).unwrap();
        resumed.backend.reference_ore_batch = reference;
        resumed.access().replace_ore_line(&metadata, (1 << 4) | 1, 1);
        runs.push((comparable(&resumed), counts, next_provider_random, next_world_random,
            resumed.backend.profile.access_reads, resumed.backend.profile.access_writes));
    }
    for (field, expected) in runs[0].0.as_object().unwrap() {
        assert!(expected == &runs[1].0[field], "checkpoint field {field} differs for native ore batching");
    }
    assert_eq!(runs[0].1, runs[1].1, "every original valid block read/write is accounted for");
    assert_eq!((runs[0].2, runs[0].3), (runs[1].2, runs[1].3));
    assert_eq!((runs[0].4, runs[0].5), (runs[1].4, runs[1].5), "resume keeps access accounting");
}

#[test]
fn identical_ore_attempts_preserve_cached_precipitation_without_callbacks_or_writes() {
    let mut runs = Vec::new();
    for reference in [true, false] {
        let mut runtime = Runtime::new(189, 1234, false, Arc::new(OreHooks));
        runtime.backend.reference_ore_batch = reference;
        runtime.provide((-1, -1)).unwrap();
        runtime.provide((0, 0)).unwrap();
        let stone = (0..5).map(|z| IVec3::new(-1, 20, z - 16)).collect::<Vec<_>>();
        let air = (0..5).map(|z| IVec3::new(4, 150, z)).collect::<Vec<_>>();
        let ore = (1 << 4) | 5;
        for &p in &stone { runtime.access().set_block(p, ore).unwrap(); }
        for &p in &air { runtime.access().set_block(p, 0).unwrap(); }
        let col = |p: IVec3| (p.z.rem_euclid(16) * 16 + p.x.rem_euclid(16)) as usize;
        // Use explicitly valid caches: an already invalid -999 cache alone
        // would fail to detect accidentally omitted no-op invalidation.
        for (&p, height) in stone.iter().zip([20, 21, 22, -999, 0]) {
            runtime.backend.precipitation.get_mut(&cp(p)).unwrap()[col(p)] = height;
        }
        for (&p, height) in air.iter().zip([150, 151, 152, -999, 0]) {
            runtime.backend.precipitation.get_mut(&cp(p)).unwrap()[col(p)] = height;
        }
        runtime.provider.chunk_load_override = false;
        let mut before = comparable(&runtime);
        before.as_object_mut().unwrap().remove("precipitation");
        let writes = runtime.backend.profile.access_writes;
        let initial_cache = runtime.backend.precipitation.clone();
        let initial_reads = runtime.backend.profile.access_reads;

        runtime.access().replace_ore_line(&stone, ore, 87);
        assert_eq!(runtime.backend.precipitation, initial_cache,
            "non-matching source IDs do not attempt a set or invalidate precipitation");
        assert_eq!(runtime.backend.profile.access_reads - initial_reads, stone.len() as u64);
        let mut repeated = stone.clone();
        repeated.extend([stone[0], stone[2]]);
        let reads = runtime.backend.profile.access_reads;
        runtime.access().replace_ore_line(&repeated, ore, 1);
        assert_eq!(runtime.backend.profile.access_reads - reads, 2 * repeated.len() as u64,
            "every matching no-op includes the original attempt's second read");
        assert_eq!(stone.iter().map(|&p| runtime.backend.precipitation[&cp(p)][col(p)]).collect::<Vec<_>>(),
            vec![-999, -999, 22, -999, -999]);

        let mut zero_line = air.clone();
        zero_line.extend([IVec3::new(4, -1, 0), IVec3::new(4, 256, 0),
            IVec3::new(30_000_000, 150, 0), IVec3::new(96, 150, 96)]);
        let reads = runtime.backend.profile.access_reads;
        runtime.access().replace_ore_line(&zero_line, 0, 0);
        assert_eq!(runtime.backend.profile.access_reads - reads, 2 * air.len() as u64 + 2,
            "invalid coordinates have no counted reads; disabled valid dummy air has two");
        assert_eq!(air.iter().map(|&p| runtime.backend.precipitation[&cp(p)][col(p)]).collect::<Vec<_>>(),
            vec![-999, -999, 152, -999, -999]);
        assert!(!runtime.provider.contains((6, 6)), "a no-op cannot load a disabled column");
        assert_eq!(runtime.backend.profile.access_writes, writes);
        let mut after = comparable(&runtime);
        after.as_object_mut().unwrap().remove("precipitation");
        for (field, value) in before.as_object().unwrap() {
            assert!(value == &after[field], "no-op changed authoritative field {field}");
        }
        runs.push((comparable(&runtime), runtime.backend.profile.access_reads));
    }
    for (field, value) in runs[0].0.as_object().unwrap() {
        assert!(value == &runs[1].0[field], "checkpoint field {field} differs for same-state ore attempts");
    }
    assert_eq!(runs[0].1, runs[1].1);
}
