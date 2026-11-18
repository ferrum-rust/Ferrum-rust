use super::*;
use std::sync::Weak;

struct Quiet;
impl Hooks for Quiet {
    fn populate(&self, _: &mut Access<'_>, _: ChunkPos, _: &mut JavaRandom) -> Result<()> { Ok(()) }
}
struct Old {
    position: ChunkPos,
    weak: Weak<crate::world::Chunk>,
    _strong: Option<Arc<crate::world::Chunk>>,
}
fn fixture(count: usize, no_sky: bool) -> (Runtime, Vec<Old>) {
    let mut runtime = Runtime::new(189,1234,false,Arc::new(Quiet));
    runtime.backend.light.no_sky = no_sky;
    for index in 0..count {
        let position = (index as i32 - 33, -2);
        let mut chunk = crate::world::Chunk::empty();
        chunk.set(0,0,0,7<<4); chunk.set(2,3,4,59<<4); chunk.set(5,15,7,13<<4);
        chunk.top = if index % 9 == 0 { 1 } else { 16 }; // stale top is intentional
        if index & 1 != 0 { chunk.random_tick_mask(); }
        if index & 2 != 0 { chunk.scheduled_tick_candidates(); }
        let mut light = super::super::super::lighting::LightChunk::default();
        light.sections[0] = true; light.height.fill(16); light.sky.fill((index % 16) as u8);
        light.block.fill(((index + 7) % 16) as u8);
        if index % 11 != 0 {
            chunk.light = Some(Arc::new(super::super::super::lighting::ChunkLight::from_chunk(&light,no_sky)));
        }
        runtime.backend.world.chunks.insert(position,Arc::new(chunk));
        if index % 7 != 0 { runtime.backend.light.chunks.insert(position,light); }
        runtime.backend.dirty_chunks.insert(position);
    }
    // A deliberately aliased pair checks COW order, including lazy cache state
    // copied from a preceding position that has just been published.
    if count > 31 {
        let earlier = runtime.backend.world.chunks[&(-3,-2)].clone();
        runtime.backend.world.chunks.insert((-2,-2),earlier);
    }
    runtime.backend.light.invalidate_render_cache();
    // Mixed unchanged/non-light-dirty cases, in addition to missing light/raw.
    runtime.backend.light.take_render_dirty();
    for index in (0..count).step_by(3) {
        let p = IVec3::new((index as i32 - 33) * 16 + 3,7,-29);
        runtime.backend.light.set(&runtime.backend.world,Kind::Block,p,((index+1)%16) as u8);
    }
    if count != 0 { runtime.backend.dirty_chunks.insert((1000,-2)); }
    let mut positions = runtime.backend.world.chunks.keys().copied().collect::<Vec<_>>(); positions.sort_unstable();
    let old = positions.into_iter().enumerate().map(|(index,position)| {
        let chunk = &runtime.backend.world.chunks[&position];
        Old { position, weak: Arc::downgrade(chunk), _strong: (index%3==0).then(||chunk.clone()) }
    }).collect();
    (runtime,old)
}
fn equal_updates(a:&[(ChunkPos,Arc<crate::world::Chunk>)],b:&[(ChunkPos,Arc<crate::world::Chunk>)]) {
    assert_eq!(a.iter().map(|(p,_)|p).collect::<Vec<_>>(),b.iter().map(|(p,_)|p).collect::<Vec<_>>());
    for ((p,a),(_,b)) in a.iter().zip(b) {
        assert_eq!(a.blocks,b.blocks,"blocks {p:?}"); assert_eq!(a.biomes,b.biomes);
        assert_eq!(a.top,b.top); assert_eq!(a.hoppers,b.hoppers); assert_eq!(a.tiles,b.tiles); assert_eq!(a.pistons,b.pistons);
        assert_eq!(a.random_tick_mask(),b.random_tick_mask());
        assert_eq!(a.scheduled_tick_candidates(),b.scheduled_tick_candidates());
        match (&a.light,&b.light) {
            (Some(a),Some(b))=>{ assert_eq!(a.values,b.values); assert_eq!(a.height,b.height);
                assert_eq!(a.sections,b.sections); assert_eq!(a.no_sky,b.no_sky); },
            (None,None)=>{}, _=>panic!("light presence {p:?}"),
        }
    }
}
fn old_state(runtime:&Runtime,old:&[Old])->Vec<(ChunkPos,bool,bool,Option<(bool,bool,u16)>)> {
    old.iter().map(|old| {
        let upgraded=old.weak.upgrade();
        let same=upgraded.as_ref().is_some_and(|c|Arc::ptr_eq(c,&runtime.backend.world.chunks[&old.position]));
        let indexes=upgraded.as_ref().map(|c|(c.random_tick_sections.get().is_some(),c.scheduled_tick_blocks.get().is_some(),c.blocks[0]));
        (old.position,upgraded.is_some(),same,indexes)
    }).collect()
}

#[test]
fn bounded_publication_matches_original_arrays_indices_cow_and_missing_or_unchanged_states() {
    for count in [0,1,62,63,64,65] { for no_sky in [false,true] {
        let (mut original,old_original)=fixture(count,no_sky);
        let (mut parallel,old_parallel)=fixture(count,no_sky);
        original.use_reference_publication(true);
        let before=(parallel.backend.provider_random.state(),parallel.backend.world_random.state(),
            serde_json::to_value(&parallel.provider).unwrap());
        let a=original.take_chunk_updates(); let b=parallel.take_chunk_updates();
        equal_updates(&a,&b);
        assert_eq!(old_state(&original,&old_original),old_state(&parallel,&old_parallel));
        assert!(parallel.backend.dirty_chunks.is_empty());
        assert!(parallel.backend.light.take_render_dirty().is_empty());
        assert_eq!((parallel.backend.provider_random.state(),parallel.backend.world_random.state(),
            serde_json::to_value(&parallel.provider).unwrap()),before);
        assert!(parallel.backend.profile.publication.maximum_prepared_columns<=PREPARED_LIMIT);
        assert_eq!(parallel.backend.profile.publication.parallel_calls,u64::from(count!=0 && count+1>=PARALLEL_MINIMUM));
        for (p,_) in &a { original.backend.dirty_chunks.insert(*p); parallel.backend.dirty_chunks.insert(*p); }
        let aa=original.take_chunk_updates(); let bb=parallel.take_chunk_updates();
        equal_updates(&aa,&bb);
        for ((_,old),(_,next)) in b.iter().zip(&bb) {
            if old.light.is_some() { assert!(Arc::ptr_eq(old,next));
                assert!(Arc::ptr_eq(old.light.as_ref().unwrap(),next.light.as_ref().unwrap())); }
        }
        assert!(parallel.take_chunk_updates().is_empty());
    } }
}

#[test]
fn cold_owner_publication_promotes_before_sharing_and_preserves_retained_snapshots() {
    fn prepare(count:usize,no_sky:bool)->(Runtime,Vec<(ChunkPos,Arc<crate::world::Chunk>)>,Vec<(ChunkPos,*const crate::world::Chunk)>) {
        let (mut runtime,old)=fixture(count,no_sky);drop(old);
        let mut positions=runtime.backend.world.chunks.keys().copied().collect::<Vec<_>>();positions.sort_unstable();
        for p in &positions {
            let c=Arc::make_mut(runtime.backend.world.chunks.get_mut(p).unwrap());
            assert!(c.blocks.compress_if_saving(1024));
            assert!(!c.blocks.has_read_cache());
        }
        let mut held=Vec::new();let mut unique=Vec::new();
        for (i,p) in positions.into_iter().enumerate() {
            let c=&runtime.backend.world.chunks[&p];
            if i%3==0 {held.push((p,Arc::clone(c)));}
            else {unique.push((p,Arc::as_ptr(c)));}
        }
        (runtime,held,unique)
    }
    for count in [1,63,65] {for no_sky in [false,true] {
        let (mut actual,old_actual,unique)=prepare(count,no_sky);
        let (mut expected,old_expected,_)=prepare(count,no_sky);
        expected.use_reference_publication(true);
        let before=(actual.backend.provider_random.state(),actual.backend.world_random.state(),
            serde_json::to_value(&actual.provider).unwrap(),actual.backend.world.light_revision,
            actual.backend.world.edits.clone());
        let updates=actual.take_chunk_updates();let reference=expected.take_chunk_updates();
        equal_updates(&updates,&reference);
        assert!(updates.iter().all(|(_,c)|c.blocks.is_dense()));
        assert!(actual.backend.world.chunks.values().all(|c|c.blocks.is_dense()));
        for ((p,old),(_,original)) in old_actual.iter().zip(&old_expected) {
            assert!(old.blocks.is_cold()&&!old.blocks.has_read_cache(),"old snapshot hydrated at {p:?}");
            assert_eq!(old.blocks,original.blocks);
            assert!(!old.blocks.has_read_cache()&&!original.blocks.has_read_cache());
            assert!(!Arc::ptr_eq(old,&actual.backend.world.chunks[p]));
        }
        for (p,old_pointer) in unique {assert_eq!(Arc::as_ptr(&actual.backend.world.chunks[&p]),old_pointer,
            "unique owner promotion changed Chunk allocation at {p:?}");}
        assert_eq!((actual.backend.provider_random.state(),actual.backend.world_random.state(),
            serde_json::to_value(&actual.provider).unwrap(),actual.backend.world.light_revision,
            actual.backend.world.edits.clone()),before);
        for (p,_) in &updates {actual.backend.dirty_chunks.insert(*p);}
        let again=actual.take_chunk_updates();
        for ((p,a),(_,b)) in updates.iter().zip(&again) {
            assert!(b.blocks.is_dense());
            if a.light.is_some(){assert!(Arc::ptr_eq(a,b),"Dense republication COW at {p:?}");}
        }
    }}
}

#[test]
fn cold_publication_weak_only_and_upgraded_aliases_detach_safely() {
    for hold in [false,true] {
        let (mut runtime,old)=fixture(1,false);drop(old);
        let p=(-33,-2);
        let chunk=runtime.backend.world.chunks.get_mut(&p).unwrap();
        assert!(Arc::get_mut(chunk).unwrap().blocks.compress_if_saving(1024));
        let values=chunk.blocks.clone();
        let weak=Arc::downgrade(chunk);
        let held=hold.then(||weak.upgrade().unwrap());
        let updates=runtime.take_chunk_updates();
        assert_eq!(updates.len(),1);assert!(updates[0].1.blocks.is_dense());
        assert_eq!(values,updates[0].1.blocks);assert!(!values.has_read_cache());
        if let Some(held)=held {
            assert!(held.blocks.is_cold()&&!held.blocks.has_read_cache());
            assert_eq!(held.blocks,updates[0].1.blocks);assert!(!held.blocks.has_read_cache());
            assert!(!Arc::ptr_eq(&held,&updates[0].1));
        }else{assert!(weak.upgrade().is_none(),"weak-only old owner must be dissociated");}
    }
}

#[test]
fn unchanged_dense_publication_keeps_weak_only_owner_and_packed_light_identity() {
    let (mut runtime,old)=fixture(2,false);drop(old);
    let p=(-32,-2);
    runtime.backend.dirty_chunks.retain(|position|*position==p);
    runtime.backend.light.take_render_dirty();
    let chunk=&runtime.backend.world.chunks[&p];
    assert!(chunk.blocks.is_dense());
    let pointer=Arc::as_ptr(chunk);let weak=Arc::downgrade(chunk);
    let packed=Arc::clone(chunk.light.as_ref().expect("unchanged packed light"));
    let values=chunk.blocks.clone();
    let updates=runtime.take_chunk_updates();
    assert_eq!(updates.len(),1);assert_eq!(updates[0].0,p);
    assert!(updates[0].1.blocks.is_dense());assert_eq!(updates[0].1.blocks,values);
    assert_eq!(Arc::as_ptr(&updates[0].1),pointer);
    assert!(Arc::ptr_eq(&weak.upgrade().expect("Dense weak owner stays valid"),&updates[0].1));
    assert!(Arc::ptr_eq(&packed,updates[0].1.light.as_ref().unwrap()));
}
