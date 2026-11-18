//! The index may skip only an overlay scan with no valid matching entries.
use super::*;

fn verify(journal: &EditJournal) {
    for x in -5..=5 { for z in -5..=5 {
        let incoming = [(x,z), (x+11,z-7)];
        let expected = journal.values.iter().filter(|(p,_)|
            (0..256).contains(&p[1]) && incoming.contains(&column(**p)))
            .map(|(p,s)| (*p,*s)).collect::<Vec<_>>();
        let (intersects, _) = journal.intersects_columns(incoming);
        assert_eq!(intersects, !expected.is_empty(), "{incoming:?}");
        let actual = if intersects {
            journal.values.iter().filter(|(p,_)|
                (0..256).contains(&p[1]) && incoming.contains(&column(**p)))
                .map(|(p,s)| (*p,*s)).collect::<Vec<_>>()
        } else { Vec::new() };
        assert_eq!(actual, expected, "full scan order must remain intact");
    } }
}

#[test]
fn column_membership_tracks_air_boundaries_removal_and_copies() {
    let mut journal = EditJournal::default();
    verify(&journal);
    for y in [-1, 0, 255, 256] {
        journal.insert([-1,y,-17], 0);
        verify(&journal);
    }
    assert_eq!(journal.columns.get().unwrap()[&(-1,-2)].len(), 2);
    journal.insert([-1,0,-17], 154 << 4);
    assert_eq!(journal.columns.get().unwrap()[&(-1,-2)].len(), 2);
    for y in [-1, 0, 255, 256] {
        journal.remove(&[-1,y,-17]);
        journal.remove(&[-1,y,-17]);
        verify(&journal);
    }
    assert!(!journal.columns.get().unwrap().contains_key(&(-1,-2)));
    journal.insert([48,20,-32], 7);
    let copied = journal.clone();
    assert!(copied.columns.get().is_none());
    verify(&copied);
    journal.clear();
    verify(&journal);
    assert!(!copied.is_empty());
}

#[test]
fn column_membership_matches_full_scan_across_mixed_mutations() {
    let mut journal = EditJournal::default();
    let mut reference = journal.values.clone();
    let mut rng = 189u64;
    for step in 0..600 {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        let p = [((rng >> 32) % 160) as i32 - 80,
            [-1,0,20,255,256][(rng % 5) as usize], ((rng >> 48) % 160) as i32 - 80];
        let state = (rng as u16) & 0xfff;
        match step % 17 {
            0 => { journal.clear(); reference.clear(); },
            1..=4 => { assert_eq!(journal.remove(&p), reference.remove(&p)); },
            5 => {
                let batch = [(p,0), ([p[0]+16, 255, p[2]-16],154<<4), (p,state)];
                journal.extend(batch); reference.extend(batch);
                assert!(journal.columns.get().is_none());
            },
            _ => { assert_eq!(journal.insert(p,state), reference.insert(p,state)); },
        }
        assert_eq!(journal.values.iter().collect::<Vec<_>>(), reference.iter().collect::<Vec<_>>());
        verify(&journal);
    }
}

#[test]
fn partially_panicking_extend_invalidates_membership_before_consumption() {
    let mut journal = EditJournal::default();
    journal.insert([0,1,0], 16);
    assert!(matches!(journal.take_changes(),EditChanges::Reset));
    assert!(!journal.intersects_columns([(3,-2)]).0);
    let empty=super::super::Chunk::empty();
    assert_eq!(journal.matches_published_columns([((3,-2),&empty)]),(true,0));
    let EditChanges::Positions(dirty)=journal.take_changes() else {panic!("delta")};
    assert!(dirty.is_empty(),"read-only filter must not add sync changes");
    let mut n = 0;
    let iter = std::iter::from_fn(move || {
        n += 1;
        if n == 1 { Some(([48,0,-32],0)) } else { panic!("interrupted extension") }
    });
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| journal.extend(iter)));
    assert!(result.is_err());
    assert!(journal.columns.get().is_none());
    assert_eq!(journal.matches_published_columns([((3,-2),&empty)]),(false,0),
        "interrupted Extend cannot reuse the old derived membership");
    assert_eq!(journal.intersects_columns([(3,-2)]), (true,2));
    assert_eq!(journal.intersects_columns([(3,-2)]), (true,0));
    assert_eq!(journal.matches_published_columns([((3,-2),&empty)]),(true,1));
    let EditChanges::Positions(dirty)=journal.take_changes() else {panic!("delta")};
    assert_eq!(dirty,HashSet::from([[48,0,-32]]),"filter must not drain pending sync changes");
    assert_eq!(journal.matches_published_columns([((3,-2),&empty)]),(true,1));
    let EditChanges::Positions(dirty)=journal.take_changes() else {panic!("delta")};
    assert!(dirty.is_empty(),"repeated matching probes must not create sync changes");
    verify(&journal);
}

fn no_write_oracle(journal: &EditJournal, chunks: &[(super::super::ChunkPos, super::super::Chunk)]) -> bool {
    chunks.iter().all(|(cp, chunk)| journal.values.iter().all(|(p, state)|
        !(0..256).contains(&p[1]) || column(*p) != *cp ||
        chunk.get(p[0].rem_euclid(16), p[1], p[2].rem_euclid(16)) == *state))
}

#[test]
fn published_membership_proves_only_exact_noop_cells_and_checks_duplicate_columns() {
    use super::super::Chunk;
    let mut journal = EditJournal::default();
    journal.extend([
        ([-1,0,-17], 0), ([-16,255,-32], 154 << 4),
        ([-2,20,-18], 52 << 4), ([-3,40,-19], 8 << 4),
        ([-1,-1,-17], 1), ([-1,256,-17], 1), ([80,10,80], 1),
    ]);
    let mut chunk = Chunk::empty();
    for (&p, &state) in &journal {
        if (0..256).contains(&p[1]) && column(p) == (-1,-2) {
            chunk.set(p[0].rem_euclid(16), p[1], p[2].rem_euclid(16), state);
        }
    }
    chunk.prepare_tick_indexes();
    assert_eq!(journal.matches_published_columns([((-1,-2), &chunk)]), (false,0),
        "missing optional index must conservatively retain the full scan");
    assert!(journal.intersects_columns([(-1,-2)]).0);
    assert_eq!(journal.matches_published_columns([((-1,-2), &chunk)]), (true,4));
    let before_hoppers=chunk.hoppers.clone();
    let before_tiles=chunk.tiles.clone();
    let before_scheduled=chunk.scheduled_tick_candidates().to_vec();
    assert_eq!(journal.matches_published_columns([((-1,-2), &chunk)]), (true,4));
    assert_eq!(chunk.hoppers,before_hoppers);
    assert_eq!(chunk.tiles,before_tiles);
    assert_eq!(chunk.scheduled_tick_candidates(),before_scheduled);
    // A repeated publication coordinate can carry a different snapshot. Never
    // deduplicate the payload check merely because incoming membership is a set.
    let mut stale=chunk.clone(); stale.set(15,0,15,16);
    let rows=vec![((-1,-2),chunk),((-1,-2),stale)];
    assert!(!no_write_oracle(&journal,&rows));
    assert!(!journal.matches_published_columns(rows.iter().map(|(p,c)|(*p,c))).0);
}

#[test]
fn published_noop_filter_matches_original_reads_across_mutations_clone_reset_and_extremes() {
    use super::super::Chunk;
    let mut journal=EditJournal::default();
    let mut reference=journal.values.clone();
    let mut rng=189u64;
    let mut chunks=vec![((-1,-2),Chunk::empty()),((0,0),Chunk::empty()),
        ((i32::MIN.div_euclid(16),i32::MAX.div_euclid(16)),Chunk::empty())];
    for step in 0..200 {
        rng=rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        let p=if step%7==0 {[i32::MIN,255,i32::MAX]} else {
            [((rng>>32)%32) as i32-16,[-1,0,20,255,256][step%5],((rng>>48)%48) as i32-32]};
        let state=if step%3==0 {0} else {(rng as u16)&0xfff};
        match step%19 {
            0=>{journal.clear();reference.clear();},
            1|2=>{assert_eq!(journal.remove(&p),reference.remove(&p));},
            3=>{journal.extend([(p,state),(p,0)]);reference.extend([(p,state),(p,0)]);},
            _=>{assert_eq!(journal.insert(p,state),reference.insert(p,state));},
        }
        assert_eq!(journal.values.iter().collect::<Vec<_>>(),reference.iter().collect::<Vec<_>>());
        journal.intersects_columns(chunks.iter().map(|(p,_)|*p));
        assert_eq!(journal.matches_published_columns(chunks.iter().map(|(p,c)|(*p,c))).0,
            no_write_oracle(&journal,&chunks));
        // Acknowledge the current values, leaving removed cells alone exactly
        // like the original publication overlay (the journal is not an undo).
        for (cp,chunk) in &mut chunks { for (&p,&state) in &journal {
            if (0..256).contains(&p[1])&&column(p)==*cp {
                chunk.set(p[0].rem_euclid(16),p[1],p[2].rem_euclid(16),state);
            }
        }}
        assert!(journal.matches_published_columns(chunks.iter().map(|(p,c)|(*p,c))).0);
        let cloned=journal.clone();
        assert!(cloned.columns.get().is_none());
        cloned.intersects_columns(chunks.iter().map(|(p,_)|*p));
        assert!(cloned.matches_published_columns(chunks.iter().map(|(p,c)|(*p,c))).0);
    }
}

// The prefilter's iteration order has no authority. This independent original
// overlay body deliberately scans the primary map, groups in that order, then
// applies each accepted occurrence (including duplicate coordinates).
fn apply_original(journal: &EditJournal,
    chunks: &mut [(super::super::ChunkPos, std::sync::Arc<super::super::Chunk>)]) {
    use std::sync::Arc;
    let incoming=chunks.iter().map(|(p,_)|*p).collect::<HashSet<_>>();
    let mut overlays: HashMap<super::super::ChunkPos,Vec<(Position,u16)>>=HashMap::new();
    for (&p,&state) in &journal.values {
        let cp=(p[0].div_euclid(16),p[2].div_euclid(16));
        if incoming.contains(&cp)&&(0..256).contains(&p[1]) {
            overlays.entry(cp).or_default().push((p,state));
        }
    }
    for (cp,chunk) in chunks {
        let mut changed=false;
        if let Some(rows)=overlays.get(cp) { for &(p,state) in rows {
            if chunk.get(p[0].rem_euclid(16),p[1],p[2].rem_euclid(16))!=state {
                Arc::make_mut(chunk).set(p[0].rem_euclid(16),p[1],p[2].rem_euclid(16),state);
                changed=true;
            }
        }}
        if changed {Arc::make_mut(chunk).light=None;}
    }
}
fn assert_chunk_identical(a:&super::super::Chunk,b:&super::super::Chunk) {
    assert_eq!(a.blocks,b.blocks); assert_eq!(a.biomes,b.biomes); assert_eq!(a.top,b.top);
    assert_eq!(a.hoppers,b.hoppers); assert_eq!(a.pistons,b.pistons); assert_eq!(a.tiles,b.tiles);
    assert_eq!(a.tiles.iter().collect::<Vec<_>>(),b.tiles.iter().collect::<Vec<_>>(),
        "tile insertion/layout order must remain the original order");
    assert_eq!(serde_json::to_value(&a.containers).unwrap(),serde_json::to_value(&b.containers).unwrap());
    assert_eq!(serde_json::to_value(&a.entities).unwrap(),serde_json::to_value(&b.entities).unwrap());
    assert_eq!(a.random_tick_sections.get().map(|t|(t.mask,t.counts)),
        b.random_tick_sections.get().map(|t|(t.mask,t.counts)));
    assert_eq!(a.scheduled_tick_blocks.get(),b.scheduled_tick_blocks.get());
    match (&a.light,&b.light) {
        (Some(a),Some(b))=>assert!(std::sync::Arc::ptr_eq(a,b)),
        (None,None)=>{},_=>panic!("light invalidation differs"),
    }
}
#[test]
fn noop_filter_preserves_original_overlay_order_cow_light_indexes_and_stale_metadata() {
    use super::super::Chunk;
    use std::sync::Arc;
    use crate::vanilla::{drops::DropNbt,collision_shapes::PistonTile,lighting::{LightChunk,ChunkLight}};
    let cp=(-1,-2);
    let mut journal=EditJournal::default();
    let mut base=Chunk::empty();
    for (x,state) in [(1,154<<4),(3,36<<4),(4,52<<4),(7,8<<4)] {
        base.set(x,20,1,state); journal.insert([-16+x,20,-31],state);
    }
    // Same-state air and spawner entries must not clean or recreate malformed
    // metadata. Original application tests equality before calling Chunk::set.
    journal.insert([-14,20,-31],0);
    base.tiles.insert([2,20,1],Arc::new(DropNbt::String("stale air metadata".into())));
    base.tiles.remove(&[4,20,1]);
    base.pistons.insert([3,20,1],PistonTile{carried:16,facing:2,extending:true,progress:0.5});
    base.prepare_tick_indexes();
    base.light=Some(Arc::new(ChunkLight::from_chunk(&LightChunk::default(),false)));
    journal.extend([([-15,-1,-31],16),([-15,256,-31],16),([80,20,80],16)]);
    let base=Arc::new(base);
    let weak=Arc::downgrade(&base);
    let mut original=vec![(cp,base.clone())];
    let mut optimized=original.clone();
    journal.intersects_columns([cp]);
    assert!(journal.matches_published_columns(optimized.iter().map(|(p,c)|(*p,c.as_ref()))).0);
    apply_original(&journal,&mut original);
    assert!(Arc::ptr_eq(&base,&optimized[0].1));
    assert!(Arc::ptr_eq(&base,&original[0].1));
    assert!(weak.upgrade().is_some());
    assert_chunk_identical(&original[0].1,&optimized[0].1);
    // Multiple new hoppers and spawners make the original per-column ordering
    // observable. Any difference must choose the complete original fallback.
    for (x,state) in [(5,154<<4),(6,154<<4),(8,52<<4),(9,52<<4)] {
        journal.insert([-16+x,20,-31],state);
    }
    original=vec![(cp,base.clone()),(cp,base.clone())];
    optimized=original.clone();
    assert!(!journal.matches_published_columns(optimized.iter().map(|(p,c)|(*p,c.as_ref()))).0);
    apply_original(&journal,&mut original);
    apply_original(&journal,&mut optimized);
    let ordered=journal.values.iter().filter(|(p,s)| column(**p)==cp&&
        (0..256).contains(&p[1])&&**s>>4==154&&p[0]!=-15)
        .map(|(p,_)|[p[0].rem_euclid(16),p[1],p[2].rem_euclid(16)]).collect::<Vec<_>>();
    assert_eq!(&original[0].1.hoppers[1..],ordered.as_slice());
    for ((_,a),(_,b)) in original.iter().zip(&optimized) {
        assert_chunk_identical(a,b); assert!(a.light.is_none());
    }
    assert_eq!(base.hoppers,vec![[1,20,1]],"shared original snapshot is immutable");
    assert!(base.light.is_some());
    // Fully acknowledged duplicate snapshots skip every write again.
    journal.intersects_columns([cp]);
    assert!(journal.matches_published_columns(original.iter().map(|(p,c)|(*p,c.as_ref()))).0);
}
