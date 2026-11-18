use super::*;
use super::super::{ChunkLight, LightChunk};

fn next(random: &mut u64) -> u8 {
    *random ^= *random << 13; *random ^= *random >> 7; *random ^= *random << 17;
    (*random >> 32) as u8
}
fn compressible(seed: u64) -> Vec<u8> {
    let mut random = seed;
    (0..65536).map(|i| if i % 17 == 0 { next(&mut random) & 15 } else { (i / 4096) as u8 }).collect()
}
fn cold(bytes: Vec<u8>) -> LightBytes {
    let mut result = LightBytes::from(bytes);
    assert!(result.compress_if_saving(0).unwrap());
    assert!(result.is_cold() && !result.has_read_cache());
    result
}

#[test]
fn randomized_dense_cold_wire_and_byte_roundtrips_preserve_original_arrays() {
    fn send_sync<T: Send + Sync>() {} send_sync::<LightBytes>();
    for seed in 1..=16 {
        let expected = compressible(seed);
        let dense = LightBytes::from(expected.clone());
        let cold = cold(expected.clone());
        let wire = serde_json::to_vec(&expected).unwrap();
        assert_eq!(serde_json::to_vec(&dense).unwrap(), wire);
        assert_eq!(serde_json::to_vec(&cold).unwrap(), wire);
        let restored: LightBytes = serde_json::from_slice(&wire).unwrap();
        assert_eq!(restored, expected);
        assert_eq!(cold, expected);
        assert_eq!(expected, cold);
        assert_eq!(dense, cold);
        assert_eq!(cold.len(), 65536);
        assert_eq!(cold.capacity(), 0, "len/equality/serde must not hydrate");
        assert!(!cold.has_read_cache());
        assert!(!restored.is_cold(), "wire deserialization preserves the existing dense path");
    }
}

#[test]
fn cloned_cold_caches_are_independent_and_first_mutation_promotes_once() {
    let expected = compressible(77);
    let mut original = cold(expected.clone());
    assert_eq!(original[731], expected[731]);
    assert!(original.has_read_cache());
    let mut copied = original.clone();
    assert!(copied.is_cold() && !copied.has_read_cache());
    if let (Storage::Cold { encoded: Encoding::Runs(a), .. }, Storage::Cold { encoded: Encoding::Runs(b), .. }) = (&original.storage, &copied.storage) {
        assert!(Arc::ptr_eq(a, b), "clones share only immutable compressed storage");
    } else { panic!("cold clones"); }
    assert_eq!(copied[0], expected[0]);
    let cached = copied.as_slice().as_ptr();
    copied[731] ^= 7;
    assert!(!copied.is_cold());
    assert_eq!(copied.as_slice().as_ptr(), cached, "promotion transfers the existing read cache");
    let dense = copied.as_slice().as_ptr();
    for at in [0, 1, 17, 4095, 32768, 65535] { copied[at] ^= 1; }
    assert_eq!(copied.as_slice().as_ptr(), dense, "subsequent mutations reuse dense storage");
    assert_eq!(original, expected, "mutating one clone cannot modify another cache or snapshot");
    assert!(original.release_read_cache() >= expected.len());
    assert!(!original.has_read_cache());
    assert_eq!(original.with_bytes(|bytes| bytes.to_vec()), expected);
    assert!(!original.has_read_cache(), "temporary reads release their decode allocation");
    let mut from_cold = original.clone();
    from_cold[19] = 3;
    assert!(!from_cold.is_cold() && !original.has_read_cache());
}

#[test]
fn mixed_random_access_and_recompression_match_an_independent_vec() {
    let mut expected = compressible(918);
    let mut actual = cold(expected.clone());
    let mut random = 0x913ea847dc102b73u64;
    for step in 0..256 {
        if step % 9 == 0 { actual.compress_if_saving(0).unwrap(); }
        if step % 5 == 0 { actual.release_read_cache(); }
        let at = (usize::from(next(&mut random)) << 8) | usize::from(next(&mut random));
        assert_eq!(actual[at], expected[at]);
        let value = next(&mut random) & 15;
        actual[at] = value; expected[at] = value;
        assert_eq!(actual[at], value);
        if step % 31 == 0 { assert_eq!(actual, expected); }
    }
    assert_eq!(actual, expected);
    assert_eq!(serde_json::to_vec(&actual).unwrap(), serde_json::to_vec(&expected).unwrap());
}

#[test]
fn borrowed_iteration_ranges_and_dense_fallback_keep_vec_semantics() {
    let expected = compressible(124);
    let mut values = cold(expected.clone());
    assert_eq!(&values[4090..4100], &expected[4090..4100]);
    assert_eq!(values.iter().zip(&values).map(|(&a, &b)| u64::from(a + b)).sum::<u64>(),
        expected.iter().map(|v| u64::from(v * 2)).sum::<u64>());
    for value in &mut values { *value &= 7; }
    assert!(!values.is_cold());
    assert!(values.iter().all(|v| *v <= 7));
    values[100..110].fill(5);
    assert!(values[100..110].iter().all(|v| *v == 5));
    let mut oversized = LightBytes::from(vec![7; MAX_COLD_BYTES + 1]);
    let before = oversized.as_slice().as_ptr();
    assert!(!oversized.compress_if_saving(0).unwrap());
    assert_eq!(oversized.as_slice().as_ptr(), before);
    let mut too_small_to_save = LightBytes::from(vec![1, 2, 3]);
    assert!(!too_small_to_save.compress_if_saving(0).unwrap());
    let mut profitable_but_disallowed = LightBytes::from(vec![0; 65536]);
    assert!(!profitable_but_disallowed.compress_if_saving(usize::MAX).unwrap());
    assert!(!profitable_but_disallowed.is_cold());
}

#[test]
fn malformed_dense_wire_is_preserved_and_corrupt_internal_streams_fail_without_caching() {
    // Match the old Vec deserializer exactly; checkpoint admission remains in
    // RuntimeSnapshot::validate rather than silently masking or padding here.
    for wire in ["[]", "[1,255]", "[0,15,16]"] {
        let values: LightBytes = serde_json::from_str(wire).unwrap();
        let expected: Vec<u8> = serde_json::from_str(wire).unwrap();
        assert_eq!(serde_json::to_vec(&values).unwrap(), serde_json::to_vec(&expected).unwrap());
    }
    assert!(serde_json::from_str::<LightBytes>("[256]").is_err());
    assert!(serde_json::from_str::<LightBytes>("{\"encoded\":[]}").is_err());
    let invalids = [
        (Encoding::Runs(Arc::from([0, 0, 4])), 65536),
        (Encoding::Runs(Arc::from([1, 0])), 65536),
        (Encoding::Runs(Arc::from([1, 0, 4])), 65536),
        (Encoding::Runs(Arc::from([255, 255, 4, 2, 0, 4])), 65536),
        (Encoding::Runs(Arc::from([])), 65536),
        (Encoding::Runs(Arc::from([1, 0, 4, 1, 0, 4])), 2),
        (Encoding::Uniform(4), 0), (Encoding::Uniform(4), 65537),
    ];
    for (encoded, length) in invalids {
        let mut corrupt = LightBytes::from_storage(Storage::Cold {
            encoded, length, read_cache: OnceLock::new() });
        assert!(corrupt.try_with_bytes(|bytes| bytes.len()).is_err());
        assert!(serde_json::to_vec(&corrupt).is_err());
        assert!(!corrupt.has_read_cache());
        assert!(!corrupt.take_recent_cold_access());
    }
}

#[test]
fn light_snapshot_pack_and_json_clone_do_not_hydrate_cold_arrays() {
    let mut light = LightChunk::default();
    light.sky = compressible(41).into(); light.block = compressible(99).into();
    let expected_wire = serde_json::to_vec(&light).unwrap();
    let expected_pack = ChunkLight::from_chunk(&light, false);
    assert!(light.sky.compress_if_saving(0).unwrap());
    assert!(light.block.compress_if_saving(0).unwrap());
    for no_sky in [false, true] {
        let copied = light.clone();
        let packed = ChunkLight::from_chunk(&copied, no_sky);
        assert_eq!(packed.values, expected_pack.values);
        assert_eq!(packed.height, light.height);
        assert_eq!(packed.sections, light.sections);
        assert_eq!(packed.no_sky, no_sky);
        assert_eq!(serde_json::to_vec(&copied).unwrap(), expected_wire);
        assert!(!copied.sky.has_read_cache() && !copied.block.has_read_cache());
    }
    assert!(!light.sky.has_read_cache() && !light.block.has_read_cache());
}

#[test]
fn native_checkpoint_validate_serialize_and_resume_preserve_cold_state_without_hydrate_all() {
    use crate::vanilla::runtime::{Runtime, RuntimeSnapshot, VanillaPopulation};
    let mut runtime = Runtime::for_overworld_verification(189, 1234, false);
    runtime.use_overlay_journal(false); // Match NativeSession's complete-chunk ownership.
    for p in [(0, 0), (0, 1), (1, 0), (1, 1)] { runtime.provide(p).unwrap(); }
    drop(runtime.take_chunk_updates());
    let baseline = serde_json::to_value(runtime.checkpoint().unwrap()).unwrap();
    let rng = (runtime.backend.provider_random.state(), runtime.backend.world_random.state());
    for light in runtime.backend.light.chunks.values_mut() {
        assert!(light.sky.compress_if_saving(0).unwrap());
        assert!(light.block.compress_if_saving(0).unwrap());
    }
    let cold = |runtime: &Runtime| runtime.backend.light.chunks.values()
        .all(|l| l.sky.is_cold() && l.block.is_cold() && !l.sky.has_read_cache() && !l.block.has_read_cache());
    assert!(cold(&runtime));
    let checkpoint = runtime.checkpoint().unwrap();
    checkpoint.validate().unwrap();
    assert!(checkpoint.light.chunks.values().all(|l| !l.sky.has_read_cache() && !l.block.has_read_cache()));
    let wire = serde_json::to_vec(&checkpoint).unwrap();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&wire).unwrap(), baseline);
    assert!(cold(&runtime));
    let mut resumed = Runtime::resume(checkpoint, Arc::new(VanillaPopulation)).unwrap();
    let updates = resumed.take_chunk_updates();
    assert_eq!(updates.len(), resumed.provider.loaded_order().len());
    assert!(updates.iter().all(|(_, c)| c.light.is_some()));
    assert!(cold(&resumed), "resume publication must pack temporary bytes, not retain every decoded array");
    assert_eq!((resumed.backend.provider_random.state(), resumed.backend.world_random.state()), rng);
    assert_eq!(serde_json::to_value(resumed.checkpoint().unwrap()).unwrap(), baseline);
    let decoded: RuntimeSnapshot = serde_json::from_slice(&wire).unwrap();
    decoded.validate().unwrap();
    assert_eq!(serde_json::to_value(&decoded).unwrap(), baseline);
    assert!(decoded.light.chunks.values().all(|l| !l.sky.is_cold() && !l.block.is_cold()));
    let mut malformed = decoded.clone();
    malformed.light.chunks.values_mut().next().unwrap().sky = vec![0; 12].into();
    assert!(malformed.validate().is_err());
    let mut malformed = decoded;
    malformed.light.chunks.values_mut().next().unwrap().block[3] = 16;
    assert!(malformed.validate().is_err());
}

#[test]
fn simple_codec_uniform_runs_split_limits_and_raw_fallback_are_exact() {
    for value in [0, 15, 255] {
        let mut uniform = cold(vec![value; 65536]);
        assert_eq!(uniform.encoded_bytes(), 1);
        assert_eq!(uniform.encoded_capacity_bytes(), 0);
        assert_eq!(uniform.with_bytes(|bytes| bytes.to_vec()), vec![value; 65536]);
        assert!(!uniform.take_recent_cold_access());
    }
    let split = Encoding::Runs(Arc::from([255, 255, 7, 1, 0, 7]));
    assert_eq!(decode(&split, 65536).unwrap(), vec![7; 65536]);
    let mut almost = vec![7; 65536]; almost[65535] = 3;
    let encoded = cold(almost.clone());
    assert_eq!(encoded.encoded_bytes(), 6);
    assert_eq!(encoded, almost);
    let mut noise = 918273645u64;
    for raw in [(0..65536).map(|i| (i & 1) as u8).collect::<Vec<_>>(),
        (0..65536).map(|_| next(&mut noise)).collect()] {
        let mut values = LightBytes::from(raw.clone());
        let ptr = values.as_slice().as_ptr(); let capacity = values.capacity();
        assert!(!values.compress_if_saving(0).unwrap());
        assert_eq!(values.as_slice().as_ptr(), ptr);
        assert_eq!(values.capacity(), capacity);
        assert_eq!(values, raw);
    }
    let mut minimum = LightBytes::from(vec![0; 65536]);
    assert!(!minimum.compress_if_saving(65536).unwrap());
    assert!(minimum.compress_if_saving(65535).unwrap());
    let mut one = LightBytes::from(vec![0]);
    assert!(!one.compress_if_saving(0).unwrap());
}

#[test]
fn cold_only_access_notice_and_owner_deadline_survive_promotion_but_never_clone_or_wire() {
    let expected = compressible(123);
    let mut values = LightBytes::from(expected.clone());
    values.set_next_eligible_generation(77);
    let _ = values[0]; values[0] = expected[0]; let _ = values.iter().count();
    assert!(!values.take_recent_cold_access(), "Dense access has no recency write");
    assert!(values.compress_if_saving(0).unwrap());
    assert_eq!(values.next_eligible_generation(), 77);
    assert!(!values.take_recent_cold_access(), "encoding is not gameplay use");
    assert_eq!(values.with_bytes(|bytes| bytes.to_vec()), expected);
    let wire = serde_json::to_vec(&values).unwrap();
    assert_eq!(wire, serde_json::to_vec(&expected).unwrap());
    let _ = format!("{values:?}"); let _ = values.capacity(); let _ = values.encoded_bytes();
    assert!(!values.has_read_cache() && !values.take_recent_cold_access());
    assert_eq!(values[91], expected[91]);
    assert!(values.take_recent_cold_access()); assert!(!values.take_recent_cold_access());
    assert_eq!(values[91], expected[91]);
    assert!(values.take_recent_cold_access(), "even cached Cold reads mark recent use");
    let mut cloned = values.clone();
    assert!(cloned.is_cold() && !cloned.has_read_cache());
    assert_eq!(cloned.next_eligible_generation(), 0);
    assert!(!cloned.take_recent_cold_access());
    assert_eq!(values.next_eligible_generation(), 77);
    assert!(!values.take_recent_cold_access(), "Clone is not a read notice");
    let _ = values.with_bytes(|bytes| bytes[1]);
    assert!(!values.take_recent_cold_access(), "bulk use of an existing cache is also neutral");
    values.release_read_cache();
    assert!(!values.take_recent_cold_access() && !values.has_read_cache());
    values[91] = 5;
    assert!(!values.is_cold() && values.take_recent_cold_access());
    assert_eq!(values.next_eligible_generation(), 77);
    values[92] = 4; let _ = values[92];
    assert!(!values.take_recent_cold_access(), "promoted Dense has no per-access metadata write");
    let mut dense_clone = values.clone();
    assert_eq!(dense_clone.next_eligible_generation(), 0);
    assert!(!dense_clone.take_recent_cold_access());
    let mut loaded: LightBytes = serde_json::from_slice(&wire).unwrap();
    assert_eq!(loaded.next_eligible_generation(), 0);
    assert!(!loaded.take_recent_cold_access() && !loaded.is_cold());
    assert_eq!(cloned, expected);
}

#[test]
fn shared_cold_reads_initialize_only_their_instance_cache() {
    let mut values = cold(compressible(33));
    let mut other = values.clone();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let values = &values;
            scope.spawn(move || { assert_eq!(values.as_slice().len(), 65536); });
        }
    });
    assert!(values.has_read_cache() && values.take_recent_cold_access());
    assert!(!other.has_read_cache() && !other.take_recent_cold_access());
}


// Independent pre-outlining bodies retain the access/cache/panic sequence.
fn original_slice(bytes: &LightBytes) -> &[u8] {
    match &bytes.storage {
        Storage::Dense(values) => values,
        Storage::Cold { encoded, length, read_cache } => {
            bytes.recent_cold_access.store(true, Ordering::Relaxed);
            read_cache.get_or_init(|| decode(encoded, *length)
                .expect("internally encoded light must decode exactly"))
        }
        Storage::SharedPacked(_) => panic!("native accessor reference excludes client packed storage"),
    }
}
fn original_mut_slice(bytes: &mut LightBytes) -> &mut [u8] {
    if let Storage::Cold { encoded, length, read_cache } = &mut bytes.storage {
        let values = read_cache.take().unwrap_or_else(||
            decode(encoded, *length).expect("internally encoded light must decode exactly"));
        bytes.recent_cold_access.store(true, Ordering::Relaxed);
        bytes.storage = Storage::Dense(values);
    }
    match &mut bytes.storage { Storage::Dense(values) => values, _ => unreachable!() }
}
fn accessor_state(bytes: &LightBytes) -> (bool, bool, bool, u64, Vec<u8>) {
    (bytes.is_cold(), bytes.has_read_cache(), bytes.recent_cold_access.load(Ordering::Relaxed),
        bytes.next_eligible_generation(), bytes.with_bytes(|values| values.to_vec()))
}
#[test]
fn outlined_access_matches_original_dense_cold_cached_and_promotion_state() {
    for variant in 0..3 {
        let expected = compressible(123);
        let mut original = LightBytes::from(expected.clone());
        let mut outlined = LightBytes::from(expected.clone());
        for bytes in [&mut original, &mut outlined] {
            bytes.set_next_eligible_generation(77);
            if variant != 0 { assert!(bytes.compress_if_saving(0).unwrap()); }
        }
        if variant == 2 {
            assert_eq!(original_slice(&original), outlined.as_slice());
            assert_eq!(original.take_recent_cold_access(), outlined.take_recent_cold_access());
        }
        assert_eq!(accessor_state(&original), accessor_state(&outlined));
        assert_eq!(original_slice(&original)[17], outlined.as_slice()[17]);
        assert_eq!(accessor_state(&original), accessor_state(&outlined));
        assert_eq!(original.take_recent_cold_access(), outlined.take_recent_cold_access());
        let original_pointer = original_slice(&original).as_ptr();
        let outlined_pointer = outlined.as_slice().as_ptr();
        original_mut_slice(&mut original)[91] = 14;
        outlined.as_mut_slice()[91] = 14;
        assert_eq!(accessor_state(&original), accessor_state(&outlined));
        assert_eq!(original_slice(&original).as_ptr(), original_pointer);
        assert_eq!(outlined.as_slice().as_ptr(), outlined_pointer);
        assert_eq!(original.take_recent_cold_access(), outlined.take_recent_cold_access());
        original_mut_slice(&mut original)[92] = 4;
        outlined.as_mut_slice()[92] = 4;
        assert_eq!(accessor_state(&original), accessor_state(&outlined));
        assert!(!outlined.take_recent_cold_access(), "Dense accesses remain notice-free");
    }
    // Mutation without a preceding ordinary read must decode and promote too.
    let mut original = cold(compressible(52));
    let mut outlined = cold(compressible(52));
    original_mut_slice(&mut original)[0] = 15;
    outlined.as_mut_slice()[0] = 15;
    assert_eq!(accessor_state(&original), accessor_state(&outlined));
}
#[test]
fn outlined_access_preserves_original_corrupt_cold_failure_notice_order() {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    for mutate in [false, true] {
        let create = || LightBytes::from_storage(Storage::Cold {
            encoded: Encoding::Runs(Arc::from([0, 0, 4])), length: 65536, read_cache: OnceLock::new() });
        let mut original = create(); let mut outlined = create();
        original.set_next_eligible_generation(77); outlined.set_next_eligible_generation(77);
        let left = catch_unwind(AssertUnwindSafe(|| {
            if mutate { original_mut_slice(&mut original)[0] = 1; }
            else { let _ = original_slice(&original)[0]; }
        }));
        let right = catch_unwind(AssertUnwindSafe(|| {
            if mutate { outlined.as_mut_slice()[0] = 1; }
            else { let _ = outlined.as_slice()[0]; }
        }));
        assert!(left.is_err() && right.is_err());
        assert!(original.is_cold() && outlined.is_cold());
        assert!(!original.has_read_cache() && !outlined.has_read_cache());
        assert_eq!(original.next_eligible_generation(), outlined.next_eligible_generation());
        assert_eq!(original.take_recent_cold_access(), !mutate);
        assert_eq!(outlined.take_recent_cold_access(), !mutate);
    }
}

#[test]
fn client_packed_variant_preserves_native_storage_layout() {
    use std::mem::{align_of, size_of};
    // The exact native representation immediately before client sharing, not
    // a hard-coded size that assumes a particular target or Rust layout.
    #[allow(dead_code)]
    enum NativeStorage {
        Dense(Vec<u8>),
        Cold { encoded: Encoding, length: usize, read_cache: OnceLock<Vec<u8>> },
    }
    #[allow(dead_code)]
    struct NativeBytes {
        storage: NativeStorage, recent_cold_access: AtomicBool, next_eligible_generation: u64,
    }
    assert_eq!(size_of::<Storage>(), size_of::<NativeStorage>());
    assert_eq!(align_of::<Storage>(), align_of::<NativeStorage>());
    assert_eq!(size_of::<LightBytes>(), size_of::<NativeBytes>());
    assert_eq!(align_of::<LightBytes>(), align_of::<NativeBytes>());
}

fn packed_source() -> Arc<ChunkLight> {
    Arc::new(ChunkLight {
        values: (0..65536).map(|i| (i % 256) as u8).collect(),
        height: (0..256).map(|i| (i * 17) % 257).collect(),
        sections: std::array::from_fn(|i| i % 3 != 0), no_sky: true,
    })
}
fn unpack_reference(source: &ChunkLight, kind: super::super::Kind) -> Vec<u8> {
    let mut values = Vec::with_capacity(source.values.len());
    for &packed in &source.values {
        values.push(match kind { super::super::Kind::Sky => packed >> 4,
            super::super::Kind::Block => packed & 15 });
    }
    values
}
#[test]
fn shared_packed_metadata_clone_bulk_and_wire_are_exact_without_hydration() {
    use super::super::Kind;
    let source = packed_source(); let equal_source = Arc::new((*source).clone());
    for kind in [Kind::Sky, Kind::Block] {
        let expected = unpack_reference(&source, kind);
        let mut bytes = LightBytes::from_shared_packed(source.clone(), kind);
        bytes.set_next_eligible_generation(77);
        assert!(bytes.is_shared_packed() && !bytes.is_cold() && !bytes.is_dense());
        assert_eq!(bytes.len(), 65536); assert!(!bytes.is_empty());
        assert!(bytes.shares_packed(&source, kind));
        assert!(!bytes.shares_packed(&equal_source, kind));
        let other_kind = if kind == Kind::Sky { Kind::Block } else { Kind::Sky };
        assert!(!bytes.shares_packed(&source, other_kind));
        assert_eq!(bytes.capacity(), 0); assert_eq!(bytes.encoded_bytes(), 0);
        assert_eq!(bytes.encoded_capacity_bytes(), 0);
        assert!(!bytes.compress_if_saving(0).unwrap());
        assert_eq!(bytes.with_bytes(|v| v.to_vec()), expected);
        let wire = serde_json::to_vec(&bytes).unwrap();
        assert_eq!(wire, serde_json::to_vec(&expected).unwrap());
        assert_eq!(bytes, expected);
        let _ = format!("{bytes:?}");
        let mut cloned = bytes.clone();
        assert!(cloned.shares_packed(&source, kind));
        assert_eq!(cloned.next_eligible_generation(), 0);
        assert!(!cloned.take_recent_cold_access());
        assert!(!bytes.has_read_cache() && !cloned.has_read_cache());
        assert_eq!(bytes.capacity(), 0); assert_eq!(cloned.capacity(), 0);
        assert!(!bytes.take_recent_cold_access());
        assert_eq!(bytes.next_eligible_generation(), 77);
        let loaded: LightBytes = serde_json::from_slice(&wire).unwrap();
        assert!(loaded.is_dense() && !loaded.is_shared_packed());
        assert_eq!(loaded, expected);
    }
}

#[test]
fn shared_packed_read_and_promotion_isolate_planes_clones_and_original_snapshot() {
    use super::super::Kind;
    let source = packed_source(); let original = source.values.clone();
    let sky_expected = unpack_reference(&source, Kind::Sky);
    let block_expected = unpack_reference(&source, Kind::Block);
    let mut sky = LightBytes::from_shared_packed(source.clone(), Kind::Sky);
    let mut block = LightBytes::from_shared_packed(source.clone(), Kind::Block);
    let mut copied_sky = sky.clone();
    sky.set_next_eligible_generation(77);
    assert_eq!(&sky[17..300], &sky_expected[17..300]);
    assert!(sky.has_read_cache() && sky.shares_packed(&source, Kind::Sky));
    assert_eq!(sky.capacity(), 65536);
    assert!(!block.has_read_cache() && !copied_sky.has_read_cache());
    let pointer = sky.as_slice().as_ptr();
    sky[17] = 14;
    assert!(sky.is_dense() && !sky.shares_packed(&source, Kind::Sky));
    assert_eq!(sky.as_slice().as_ptr(), pointer, "promotion must transfer its private cache");
    assert_eq!(sky.next_eligible_generation(), 77);
    assert!(!sky.take_recent_cold_access());
    assert_eq!(copied_sky.with_bytes(|v| v.to_vec()), sky_expected);
    assert!(!copied_sky.has_read_cache());
    // Mutating an untouched other plane decodes only that plane and promotes it.
    block[17] = 3;
    assert!(block.is_dense()); assert!(!block.take_recent_cold_access());
    assert_eq!(block[18], block_expected[18]);
    assert_eq!(source.values, original);
    assert!(!copied_sky.take_recent_cold_access());
    assert_eq!(copied_sky[31], sky_expected[31]);
    assert_eq!(copied_sky.release_read_cache(), 65536);
    assert!(copied_sky.is_shared_packed() && !copied_sky.has_read_cache());
    assert_eq!(copied_sky.release_read_cache(), 0);
    assert!(!copied_sky.take_recent_cold_access());
}

#[test]
fn shared_packed_concurrent_reads_and_owner_lifetimes_remain_independent() {
    use super::super::Kind;
    let source = packed_source(); let weak = Arc::downgrade(&source);
    let mut bytes = LightBytes::from_shared_packed(source.clone(), Kind::Block);
    let mut cloned = bytes.clone();
    drop(source);
    assert!(weak.upgrade().is_some());
    std::thread::scope(|scope| {
        for _ in 0..4 { let bytes = &bytes;
            scope.spawn(move || { assert_eq!(bytes[255], 15); assert_eq!(bytes.as_slice().len(), 65536); });
        }
    });
    assert!(bytes.has_read_cache()); assert!(!bytes.take_recent_cold_access());
    assert!(!cloned.has_read_cache() && !cloned.take_recent_cold_access());
    let after_read = bytes.clone();
    assert!(!after_read.has_read_cache());
    drop(bytes); drop(after_read);
    assert!(weak.upgrade().is_some(), "unhydrated clone retains its packed source");
    cloned[0] = 7;
    assert!(cloned.is_dense() && !cloned.take_recent_cold_access());
    assert!(weak.upgrade().is_none(), "last promotion releases the immutable packed source");
}

#[test]
fn shared_packed_constructor_rejects_non_column_lengths() {
    use std::panic::catch_unwind;
    for len in [0, 1, 65535, 65537] {
        let mut source = (*packed_source()).clone(); source.values.resize(len, 15);
        assert!(catch_unwind(|| LightBytes::from_shared_packed(Arc::new(source), super::super::Kind::Sky)).is_err());
    }
}


#[test]
fn scalar_value_reads_preserve_native_dense_cold_values_cache_and_notices() {
    for compressed in [false, true] {
        let expected = compressible(71);
        let mut actual = LightBytes::from(expected.clone());
        let mut reference = LightBytes::from(expected.clone());
        for bytes in [&mut actual, &mut reference] {
            bytes.set_next_eligible_generation(77);
            if compressed { assert!(bytes.compress_if_saving(0).unwrap()); }
        }
        for index in (0..65536).step_by(257).chain([65535, 0, 42]) {
            assert_eq!(actual.value_at(index), reference.as_slice()[index]);
            assert_eq!(actual.has_read_cache(), reference.has_read_cache());
            assert_eq!(actual.capacity(), reference.capacity());
            assert_eq!(actual.take_recent_cold_access(), reference.take_recent_cold_access());
            assert_eq!(actual.next_eligible_generation(), reference.next_eligible_generation());
        }
        use std::panic::{catch_unwind, AssertUnwindSafe};
        actual.release_read_cache(); reference.release_read_cache();
        assert!(catch_unwind(AssertUnwindSafe(|| actual.value_at(65536))).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| reference.as_slice()[65536])).is_err());
        assert_eq!(actual.has_read_cache(), reference.has_read_cache());
        assert_eq!(actual.take_recent_cold_access(), reference.take_recent_cold_access());
    }
    // Malformed Dense data must not acquire the nibble masking of SharedPacked.
    let raw: LightBytes = vec![0, 16, 128, 255].into();
    assert_eq!((0..4).map(|i| raw.value_at(i)).collect::<Vec<_>>(), vec![0,16,128,255]);
}

#[test]
fn scalar_shared_values_are_exact_without_hydrating_and_remain_valid_after_promotion() {
    use super::super::Kind;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let source = packed_source();
    for kind in [Kind::Sky, Kind::Block] {
        let expected = unpack_reference(&source, kind);
        let mut values = LightBytes::from_shared_packed(source.clone(), kind);
        for (index, &value) in expected.iter().enumerate() { assert_eq!(values.value_at(index), value); }
        assert_eq!(values.capacity(), 0); assert!(!values.has_read_cache());
        assert!(values.shares_packed(&source, kind)); assert!(!values.take_recent_cold_access());
        assert!(catch_unwind(AssertUnwindSafe(|| values.value_at(65536))).is_err());
        assert!(!values.has_read_cache(), "invalid scalar reads also never allocate a raw plane");
        let mut copied = values.clone();
        assert_eq!(copied.value_at(17), expected[17]); assert_eq!(copied.capacity(),0);
        let pointer = values.as_slice().as_ptr(); // Explicit slice access retains its old contract.
        assert_eq!(values.value_at(17), expected[17]);
        assert_eq!(values.as_slice().as_ptr(), pointer);
        values[17] = 13; // Explicit mutation still promotes and transfers the cache.
        assert!(values.is_dense()); assert_eq!(values.value_at(17), 13);
        assert_eq!(values.as_slice().as_ptr(), pointer);
        assert_eq!(copied.value_at(17), expected[17]); assert_eq!(copied.capacity(),0);
        assert!(!values.take_recent_cold_access() && !copied.take_recent_cold_access());
    }
}
