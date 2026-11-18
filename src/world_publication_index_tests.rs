use super::*;

fn same(expected: &Chunk, actual: &Chunk) {
    assert_eq!(expected.random_tick_sections.get().map(|s| (s.counts,s.mask)),
        actual.random_tick_sections.get().map(|s| (s.counts,s.mask)));
    assert_eq!(expected.scheduled_tick_blocks.get(), actual.scheduled_tick_blocks.get());
}
#[test]
fn detached_single_scan_indexes_match_original_distinct_top_limits_and_stale_caches() {
    for length in [1023,4095,4096,9000,65536] { for top in [-1,0,1,15,16,17,255,256,300] {
        for initialized in 0..4 {
            let mut chunk = Chunk::empty();
            chunk.blocks = (0..length).map(|i| match i % 5 { 0=>59<<4, 1=>13<<4, 2=>8<<4, _=>16 }).collect();
            chunk.top = top;
            // Deliberately stale preinitialized values must remain unmodified.
            if initialized & 1 != 0 {
                let mut ticks = RandomTickSections::default(); ticks.counts[3]=777; ticks.mask=8;
                assert!(chunk.random_tick_sections.set(ticks).is_ok());
            }
            if initialized & 2 != 0 { assert!(chunk.scheduled_tick_blocks.set(vec![(9,189)]).is_ok()); }
            let expected = chunk.clone();
            let before = chunk.clone();
            let prepared = chunk.prepare_tick_indexes_detached();
            same(&before, &chunk); // preparation is detached, including OnceLocks
            expected.prepare_tick_indexes();
            chunk.install_prepared_tick_indexes(prepared);
            same(&expected, &chunk);
            assert_eq!(chunk.blocks, expected.blocks);
        }
    } }
}

#[test]
fn detached_index_install_keeps_a_first_alias_initializer() {
    let chunk = Chunk::empty();
    let prepared = chunk.prepare_tick_indexes_detached();
    let mut ticks = RandomTickSections::default(); ticks.counts[0]=11; ticks.mask=1;
    assert!(chunk.random_tick_sections.set(ticks).is_ok());
    assert!(chunk.scheduled_tick_blocks.set(vec![(7,13<<4)]).is_ok());
    let expected = chunk.clone();
    chunk.install_prepared_tick_indexes(prepared);
    same(&expected, &chunk);
}
