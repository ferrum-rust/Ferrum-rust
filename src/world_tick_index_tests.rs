use super::*;

fn original_tick_ids() -> Vec<u16> {
    let (ids, _): (Vec<u16>, serde::de::IgnoredAny) =
        serde_json::from_reader(flate2::read::GzDecoder::new(
            &include_bytes!("../tests/fixtures/vanilla189-crop-growth.json.gz")[..],
        ))
        .unwrap();
    ids
}
fn check(chunk: &Chunk, tick_ids: &[u16]) {
    let mut counts = [0u16; 16];
    let mut scheduled = Vec::new();
    for (index, &state) in chunk.blocks.iter().enumerate() {
        counts[index / 4096] += u16::from(tick_ids.contains(&(state >> 4)));
        if matches!(state >> 4, 8 | 10 | 12 | 13 | 51 | 145)
            || (state >> 4 == 28 && state & 8 != 0)
        {
            scheduled.push((index as u16, state));
        }
    }
    let mask = counts
        .iter()
        .enumerate()
        .fold(0u16, |mask, (section, &count)| {
            mask | (u16::from(count != 0) << section)
        });
    assert_eq!(chunk.random_tick_mask(), mask);
    assert_eq!(chunk.random_tick_sections.get().unwrap().counts, counts);
    assert_eq!(chunk.scheduled_tick_candidates(), scheduled);
}

#[test]
fn original_tick_registry_indexes_remain_exact_across_edits_and_chunk_clones() {
    let ids = original_tick_ids();
    let mut chunk = Chunk::empty();
    // Populate every section before its first index build, including y=255.
    for y in 0..256 {
        let state = ((y % 198) as u16) << 4 | (y & 15) as u16;
        chunk.set(y & 15, y, (y >> 4) & 15, state);
    }
    chunk.prepare_tick_indexes();
    check(&chunk, &ids);
    let original = Arc::new(chunk);
    let mut edited = original.clone();
    let states = [
        0,
        1 << 4,
        8 << 4 | 3,
        9 << 4,
        10 << 4 | 7,
        11 << 4,
        12 << 4,
        13 << 4,
        51 << 4 | 9,
        59 << 4 | 7,
        145 << 4 | 8,
        28 << 4,
        28 << 4 | 8,
        18 << 4 | 4,
    ];
    for round in 0..16 {
        for y in 0..256 {
            let state = states[(round + y as usize) % states.len()];
            Arc::make_mut(&mut edited).set(y & 15, y, (y >> 4) & 15, state);
        }
        assert!(
            edited.random_tick_sections.get().is_some(),
            "edits must update, never invalidate the index"
        );
        assert!(edited.scheduled_tick_blocks.get().is_some());
        check(&edited, &ids);
        check(&original, &ids);
    }
    assert_ne!(
        edited.blocks, original.blocks,
        "the original publication must remain immutable"
    );
}

#[test]
fn full_random_tick_section_keeps_original_reference_count_until_last_removal() {
    let mut chunk = Chunk::empty();
    chunk.prepare_tick_indexes();
    for y in 16..32 {
        for z in 0..16 {
            for x in 0..16 {
                chunk.set(x, y, z, 59 << 4);
            }
        }
    }
    assert_eq!(chunk.random_tick_mask(), 2);
    assert_eq!(chunk.random_tick_sections.get().unwrap().counts[1], 4096);
    for index in 4096..8191 {
        chunk.set(index & 15, index >> 8, (index >> 4) & 15, 1 << 4);
    }
    assert_eq!(chunk.random_tick_mask(), 2);
    assert_eq!(chunk.random_tick_sections.get().unwrap().counts[1], 1);
    chunk.set(15, 31, 15, 0);
    assert_eq!(chunk.random_tick_mask(), 0);
    assert_eq!(chunk.random_tick_sections.get().unwrap().counts[1], 0);
    check(&chunk, &original_tick_ids());
}
