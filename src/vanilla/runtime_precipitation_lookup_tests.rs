use super::*;
use std::panic::{catch_unwind, AssertUnwindSafe};

struct QuietHooks;
impl Hooks for QuietHooks {
    fn populate(&self, _: &mut Access<'_>, _: ChunkPos, _: &mut JavaRandom) -> Result<()> { Ok(()) }
}

fn state(runtime: &Runtime) -> serde_json::Value {
    serde_json::json!({"provider":runtime.provider,
        "provider_random":runtime.backend.provider_random.state(),
        "world_random":runtime.backend.world_random.state(),
        "reads":runtime.backend.profile.access_reads,"writes":runtime.backend.profile.access_writes,
        "events":runtime.backend.events})
}

#[test]
fn fixed_precipitation_column_matches_original_scan_cache_y_edges_and_logical_accesses() {
    let mut runtime = Runtime::new(189, 1234, false, Arc::new(QuietHooks));
    for pos in [(-2, 3), (1_874_999, 0), (-1_875_000, 0)] {
        runtime.provide(pos).unwrap();
        let (x, z) = (pos.0 * 16 + 15, pos.1 * 16);
        for no_sky in [false, true] {
            runtime.backend.light.no_sky = no_sky;
            for (y, block, present, expected) in [(0, 16, true, -1), (1, 16, true, 2),
                (20, 18 << 4, true, 21), (30, 9 << 4, true, 31),
                (15, 50 << 4, true, -1), (255, 16, true, 256), (255, 16, false, -1)] {
                let mut chunk = crate::world::Chunk::empty();
                chunk.set(15, y, 0, block);
                runtime.backend.world.chunks.insert(pos, Arc::new(chunk));
                let light = runtime.backend.light.chunks.get_mut(&pos).unwrap();
                light.sections.fill(false);
                if present { light.sections[y as usize / 16] = true; }
                let mut cache = vec![-999; 256]; cache[7] = 123;
                let before = state(&runtime);
                let mut oracle = None;
                for reference in [true, false] {
                    runtime.backend.reference_precipitation_scan = reference;
                    runtime.backend.precipitation.insert(pos, cache.clone());
                    let actual = FeatureWorld::precipitation_height(&runtime.access(), x, z);
                    assert_eq!(actual, expected, "y={y} state={block} present={present}");
                    assert_eq!(state(&runtime), before, "loaded scan must not draw/read/write/reload");
                    let computed = runtime.backend.precipitation[&pos].clone();
                    assert_eq!(computed[7], 123, "other columns retain their cache");
                    if let Some(expected) = &oracle { assert_eq!(&computed, expected); }
                    else { oracle = Some(computed); }
                    assert_eq!(FeatureWorld::precipitation_height(&runtime.access(), x, z), actual,
                        "a populated cache is reused, including -1 and 256");
                    runtime.backend.precipitation.get_mut(&pos).unwrap()[15] = 77;
                    assert_eq!(FeatureWorld::precipitation_height(&runtime.access(), x, z), 77,
                        "cached values are not repaired by the scan optimization");
                }
            }
        }
    }
}

#[test]
fn fixed_precipitation_scan_retains_original_missing_load_and_disabled_load_behavior() {
    let mut runs = Vec::new();
    for reference in [true, false] {
        let mut runtime = Runtime::new(189, 1234, false, Arc::new(QuietHooks));
        runtime.backend.reference_precipitation_scan = reference;
        let value = FeatureWorld::precipitation_height(&runtime.access(), -17, 49);
        assert!(runtime.provider.contains((-2, 3)), "provide remains before every cache lookup");
        runs.push(serde_json::json!({"value":value,"state":state(&runtime),
            "cache":runtime.backend.precipitation.iter().collect::<Vec<_>>() }));
        runtime.provider.chunk_load_override = false;
        let before = state(&runtime);
        // Existing unavailable-cache behavior is a panic after provide(None).
        // This narrowly scoped optimization must not silently change it.
        assert!(catch_unwind(AssertUnwindSafe(|| {
            FeatureWorld::precipitation_height(&runtime.access(), 1600, 1600)
        })).is_err());
        assert_eq!(state(&runtime), before);
        assert!(!runtime.provider.contains((100, 100)));
    }
    assert_eq!(runs[0], runs[1]);
}

#[test]
fn fixed_precipitation_scan_keeps_missing_raw_storage_as_air_without_reloading() {
    let mut runtime = Runtime::new(189, 1234, false, Arc::new(QuietHooks));
    let pos = (-2, 3);
    runtime.provide(pos).unwrap();
    runtime.backend.world.chunks.remove(&pos);
    runtime.backend.light.chunks.get_mut(&pos).unwrap().sections.fill(true);
    let before = state(&runtime);
    let mut expected = None;
    for no_sky in [false, true] {
        runtime.backend.light.no_sky = no_sky;
        for reference in [true, false] {
            runtime.backend.reference_precipitation_scan = reference;
            runtime.backend.precipitation.insert(pos, vec![-999; 256]);
            assert_eq!(FeatureWorld::precipitation_height(&runtime.access(), -17, 48), -1);
            assert!(!runtime.backend.world.chunks.contains_key(&pos));
            assert_eq!(state(&runtime), before);
            let cache = &runtime.backend.precipitation[&pos];
            assert_eq!(cache[15], -1);
            if let Some(expected) = &expected { assert_eq!(cache, expected); }
            else { expected = Some(cache.clone()); }
        }
    }
}
