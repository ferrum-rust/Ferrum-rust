use super::*;
fn patterned(seed:u16)->Vec<u16> {
    (0..STATES).map(|i| ((i/256) as u16).wrapping_mul(257)^seed).collect()
}
fn cold(values:Vec<u16>)->BlockStates {
    let mut result=BlockStates::from(values);
    assert!(result.compress_if_saving(1024));
    assert!(result.is_cold()&&!result.has_read_cache());result
}
#[test]
fn dense_cold_full_u16_values_and_numeric_array_wire_match_vec() {
    fn send_sync<T:Send+Sync>(){}send_sync::<BlockStates>();
    let mut long=vec![u16::MAX;STATES];long[STATES-1]=0x8001;
    for expected in [vec![0;STATES],vec![u16::MAX;STATES],long,patterned(0x8000)] {
        let dense=BlockStates::from(expected.clone());let mut encoded=cold(expected.clone());
        let wire=serde_json::to_vec(&expected).unwrap();
        assert_eq!(serde_json::to_vec(&dense).unwrap(),wire);
        assert_eq!(serde_json::to_vec(&encoded).unwrap(),wire);
        let restored:BlockStates=serde_json::from_slice(&wire).unwrap();
        assert!(restored.is_dense());assert_eq!(restored,expected);assert_eq!(dense,encoded);
        assert_eq!(encoded.with_slice(|v|v.to_vec()),expected);
        assert!(!encoded.has_read_cache()&&!encoded.take_recent_cold_access());
        for i in [0,1,255,256,32768,65534,65535] {assert_eq!(encoded.value_at(i),expected[i]);}
        assert!(encoded.has_read_cache()&&encoded.take_recent_cold_access());
        assert_eq!(encoded.into_iter().collect::<Vec<_>>(),expected);
    }
    let all=(0..=u16::MAX).collect::<Vec<_>>();let mut raw=BlockStates::from(all.clone());
    let pointer=raw.as_slice().as_ptr();let capacity=raw.capacity();
    assert!(!raw.compress_if_saving(0));assert!(raw.is_dense());
    assert_eq!(raw.as_slice().as_ptr(),pointer);assert_eq!(raw.capacity(),capacity);assert_eq!(raw,all);
}
#[test]
fn clone_and_clone_from_preserve_value_isolation_and_dense_reuse() {
    let values=patterned(0x4012);let source=BlockStates::from(values.clone());
    let mut retained=source.clone();assert_ne!(retained.as_slice().as_ptr(),source.as_slice().as_ptr());
    retained[10]=0xffff;assert_eq!(source[10],values[10]);
    let mut target=Vec::with_capacity(STATES*2);target.extend([0u16;17]);
    let mut target=BlockStates::from(target);let pointer=target.as_slice().as_ptr();let capacity=target.capacity();
    target.clone_from(&source);assert_eq!(target,values);assert_eq!(target.as_slice().as_ptr(),pointer);
    assert_eq!(target.capacity(),capacity);
    target.clone_from(&BlockStates::from(vec![7,8,9]));
    assert_eq!(target.as_slice().as_ptr(),pointer);assert_eq!(target.as_slice(),[7,8,9]);
    let mut original=cold(values.clone());original.set_next_eligible_generation(99);
    let _=original[3];assert!(original.has_read_cache());
    let mut clone=original.clone();
    assert!(clone.is_cold()&&!clone.has_read_cache());assert_eq!(clone.next_eligible_generation(),0);
    assert!(!clone.take_recent_cold_access());assert!(original.take_recent_cold_access());
    if let (Storage::Cold(a),Storage::Cold(b))=(&original.storage,&clone.storage) {
        if let (Encoding::Runs(a),Encoding::Runs(b))=(&a.encoded,&b.encoded){assert!(Arc::ptr_eq(a,b));}
        else{panic!("patterned fixture must use runs");}
    }else{panic!("cold clones");}
    clone[3]=0xdead;assert_eq!(original.value_at(3),values[3]);assert_eq!(clone[3],0xdead);
    target.clone_from(&original);assert!(target.is_cold()&&!target.has_read_cache());
    assert_eq!(target,values);target[4]=0xbeef;assert_eq!(original.value_at(4),values[4]);
    target.clone_from(&source);assert!(target.is_dense());assert_eq!(target,values);
}
#[test]
fn slice_mutations_match_vec_and_cached_promotion_moves_its_allocation() {
    for cached in [false,true] {
        let mut expected=patterned(0x8f00);let mut actual=cold(expected.clone());
        let cached_pointer=if cached {Some(actual.as_slice().as_ptr())}else{None};
        if cached {assert!(actual.take_recent_cold_access());}
        actual[0]=0xffff;expected[0]=0xffff;
        assert!(actual.is_dense()&&actual.take_recent_cold_access());
        if let Some(pointer)=cached_pointer{assert_eq!(actual.as_slice().as_ptr(),pointer);}
        actual[3..11].fill(0x8001);expected[3..11].fill(0x8001);
        actual[12..16].copy_from_slice(&[0xffff,0,0x8000,1]);expected[12..16].copy_from_slice(&[0xffff,0,0x8000,1]);
        actual[20..40].reverse();expected[20..40].reverse();
        for value in &mut actual {*value=value.wrapping_add(1);}
        for value in &mut expected {*value=value.wrapping_add(1);}
        assert_eq!(actual.as_slice(),expected);assert!(!actual.take_recent_cold_access());
        assert!(!actual.promote_to_dense());
        assert_eq!(actual.into_vec(),expected);
    }
}
#[test]
fn transient_bulk_equality_validation_and_clone_never_leave_caches_or_notices() {
    let expected=patterned(9);let mut values=cold(expected.clone());
    values.set_next_eligible_generation(123);
    let wire=serde_json::to_vec(&values).unwrap();assert_eq!(wire,serde_json::to_vec(&expected).unwrap());
    assert_eq!(values.with_slice(|v|v.iter().filter(|&&state|state==0x8009).count()),
        expected.iter().filter(|&&state|state==0x8009).count());
    assert!(values==expected&&values==values.clone());
    assert_eq!(values.capacity(),0);assert!(!values.has_read_cache()&&!values.take_recent_cold_access());
    assert_eq!(values.next_eligible_generation(),123);
    assert_eq!(values.release_read_cache(),0);
    let _=values.value_at(42);assert!(values.take_recent_cold_access());
    let resident=values.capacity()*2;assert_eq!(values.release_read_cache(),resident);
    assert!(resident>=RAW_BYTES);assert!(!values.has_read_cache()&&!values.take_recent_cold_access());
    assert_eq!(serde_json::to_vec(&values).unwrap(),wire);
    assert!(!values.has_read_cache());
}
#[test]
fn malformed_dense_lengths_and_json_errors_keep_vec_semantics() {
    for expected in [vec![],vec![0x8000],vec![u16::MAX;STATES-1],vec![1;STATES+1]] {
        let mut actual=BlockStates::from(expected.clone());let pointer=actual.as_slice().as_ptr();
        assert!(!actual.compress_if_saving(0));assert_eq!(actual.as_slice().as_ptr(),pointer);
        let wire=serde_json::to_vec(&expected).unwrap();assert_eq!(serde_json::to_vec(&actual).unwrap(),wire);
        let restored:BlockStates=serde_json::from_slice(&wire).unwrap();assert_eq!(restored,expected);
        assert_eq!(restored.len(),expected.len());
    }
    for json in ["[-1]","[65536]","[1.5]","null","{\"Runs\":[1,2,3,4]}"] {
        assert_eq!(serde_json::from_str::<BlockStates>(json).unwrap_err().to_string(),
            serde_json::from_str::<Vec<u16>>(json).unwrap_err().to_string());
    }
    let mut exact=BlockStates::from(vec![0xffff;STATES]);
    assert!(!exact.compress_if_saving(RAW_BYTES-1));assert!(exact.compress_if_saving(RAW_BYTES-2));
}
fn invalid_cold(runs:Vec<u8>,length:usize)->BlockStates {
    BlockStates::from_storage(Storage::Cold(Box::new(ColdStates{encoded:Encoding::Runs(runs.into()),
        length,read_cache:OnceLock::new()})))
}
#[test]
fn malformed_internal_runs_are_bounded_and_do_not_commit_failed_mutation() {
    for runs in [vec![],vec![0,0,1,0],vec![1,0,255,255],vec![255,255,0,0,2,0,0,0],
        vec![1,0,0],vec![1;RAW_BYTES]] {
        let mut values=invalid_cold(runs,STATES);
        assert!(values.try_with_slice(|_|()).is_err());assert!(serde_json::to_vec(&values).is_err());
        assert!(!values.has_read_cache()&&!values.take_recent_cold_access());
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{values[0]=8;})).is_err());
        assert!(values.is_cold()&&!values.has_read_cache()&&!values.take_recent_cold_access());
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{let _=values.value_at(0);})).is_err());
        assert!(values.take_recent_cold_access()&&!values.has_read_cache());
    }
    assert!(decode(&Encoding::Uniform(1),STATES+1).is_err());
    assert!(decode(&Encoding::Uniform(1),0).is_err());
}
#[test]
fn future_outer_owner_admission_and_publication_keep_old_cold_copies_isolated() {
    #[derive(Clone)]struct Holder{blocks:BlockStates}
    let expected=patterned(9);let mut owner=Arc::new(Holder{blocks:expected.clone().into()});
    let weak=Arc::downgrade(&owner);assert!(Arc::get_mut(&mut owner).is_none());drop(weak);
    let strong=Arc::clone(&owner);assert!(Arc::get_mut(&mut owner).is_none());drop(strong);
    assert!(Arc::get_mut(&mut owner).unwrap().blocks.compress_if_saving(1024));
    // Simulate an unexpected retained cold alias: safe COW publication may
    // promote only the new owner and cannot mutate or hydrate the old snapshot.
    let old=Arc::clone(&owner);
    assert!(Arc::make_mut(&mut owner).blocks.promote_to_dense());
    assert!(owner.blocks.is_dense());assert!(old.blocks.is_cold()&&!old.blocks.has_read_cache());
    let published=Arc::clone(&owner);assert!(published.blocks.is_dense());
    Arc::make_mut(&mut owner).blocks[4]=0xace1;
    assert_eq!(published.blocks[4],expected[4]);assert_eq!(old.blocks,expected);
    assert!(!old.blocks.has_read_cache());
}
