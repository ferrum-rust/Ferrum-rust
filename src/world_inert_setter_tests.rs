use super::*;
use crate::vanilla::{collision_shapes::PistonTile, drops::DropNbt};

fn targets() -> Vec<u16> {
    (16..=22).chain([48,224,240,256,336,896,1168,2064,2448]).collect()
}
fn marker(value: i32) -> Arc<DropNbt> { Arc::new(DropNbt::Int(value)) }
fn fixture(indexes: u8) -> Chunk {
    let mut chunk=Chunk::empty();
    chunk.set(7,40,7,59 << 4);
    chunk.set(7,56,7,13 << 4);
    chunk.biomes.fill(6);
    chunk.containers.push((IVec3::new(5,6,7), crate::container::Container::new(crate::container::Kind::Chest)));
    // Preserve stale unrelated records exactly; this is not a metadata repair.
    chunk.hoppers.extend([[0,0,0],[15,255,15]]);
    chunk.pistons.insert([0,0,0], PistonTile {carried:16,facing:1,extending:true,progress:0.5});
    chunk.tiles.insert([9,9,9],marker(189));
    chunk.light=Some(Arc::new(crate::vanilla::lighting::ChunkLight::from_chunk(
        &crate::vanilla::lighting::LightChunk::default(),false)));
    if indexes & 1 != 0 { chunk.random_tick_mask(); }
    if indexes & 2 != 0 { chunk.scheduled_tick_candidates(); }
    chunk
}
fn equal(expected:&Chunk,actual:&Chunk) {
    assert_eq!(expected.blocks,actual.blocks);
    assert_eq!(expected.biomes,actual.biomes);
    assert_eq!(expected.top,actual.top);
    assert_eq!(expected.hoppers,actual.hoppers);
    assert_eq!(expected.pistons,actual.pistons);
    assert_eq!(expected.tiles,actual.tiles);
    assert_eq!(expected.random_tick_sections.get().map(|s|(s.counts,s.mask)),
        actual.random_tick_sections.get().map(|s|(s.counts,s.mask)));
    assert_eq!(expected.scheduled_tick_blocks.get(),actual.scheduled_tick_blocks.get());
    assert_eq!(serde_json::to_value(&expected.containers).unwrap(),serde_json::to_value(&actual.containers).unwrap());
    assert_eq!(serde_json::to_value(&expected.entities).unwrap(),serde_json::to_value(&actual.entities).unwrap());
    match (&expected.light,&actual.light) {
        (Some(a),Some(b))=>assert!(Arc::ptr_eq(a,b),"setter must not replace lighting"),
        (None,None)=>{},
        _=>panic!("setter changed light presence"),
    }
}
#[test]
fn inert_setter_whitelist_has_no_generic_tick_or_entity_index_transitions() {
    let targets=targets();
    for state in 0..=u16::MAX {
        assert_eq!(inert_ore_replacement(state),targets.contains(&state));
    }
    for state in targets.into_iter().chain((0..16).flat_map(|meta|[16|meta,(87<<4)|meta])) {
        assert!(!crate::gameplay::growth::ticks_randomly(state>>4));
        assert!(!scheduled_tick_candidate(state));
        assert!(!matches!(state>>4,36|52|154));
    }
    for forbidden in [0,13<<4,74<<4,97<<4,17<<4,89<<4] {
        assert!(!inert_ore_replacement(forbidden));
    }
}
#[test]
fn known_inert_setter_matches_generic_set_with_initialized_and_lazy_indexes_and_stale_tiles() {
    for initialized in 0..4 {
        let mut expected=fixture(initialized);
        let mut actual=expected.clone();
        for old in [16,31,87<<4,(87<<4)|15] {
            for state in targets() {
                for [x,y,z] in [[0,0,0],[15,15,15],[0,16,0],[15,31,15],[1,127,2],[15,255,15]] {
                    expected.set(x,y,z,old);
                    actual.set(x,y,z,old);
                    // Deliberately stale top and same-state cases must still
                    // reproduce generic set's unconditional nonair top update.
                    expected.top=-1;actual.top=-1;
                    expected.tiles.insert([x,y,z],marker(42));
                    actual.tiles.insert([x,y,z],marker(42));
                    expected.set(x,y,z,state);
                    actual.set_inert_ore_known(Chunk::index(x,y,z),old,state);
                    equal(&expected,&actual);
                }
            }
        }
    }
}
#[test]
fn known_inert_setter_preserves_the_callers_strong_and_weak_chunk_cow() {
    let mut baseline=fixture(3);
    baseline.set(15,255,15,16);
    let published=Arc::new(baseline);
    let before=published.blocks.clone();
    let mut expected=published.clone();
    let mut actual=published.clone();
    Arc::make_mut(&mut expected).set(15,255,15,224);
    Arc::make_mut(&mut actual).set_inert_ore_known(65535,16,224);
    equal(&expected,&actual);
    assert_eq!(published.blocks,before);
    assert!(!Arc::ptr_eq(&published,&actual));
    let mut expected=Arc::new((*published).clone());
    let mut actual=Arc::new((*published).clone());
    let weak_expected=Arc::downgrade(&expected);
    let weak_actual=Arc::downgrade(&actual);
    Arc::make_mut(&mut expected).set(15,255,15,336);
    Arc::make_mut(&mut actual).set_inert_ore_known(65535,16,336);
    assert!(weak_expected.upgrade().is_none() && weak_actual.upgrade().is_none());
    equal(&expected,&actual);
}
