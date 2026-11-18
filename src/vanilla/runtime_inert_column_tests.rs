use super::*;
use sha1::{Digest, Sha1};
use std::{cell::Cell, time::Instant};

struct QuietHooks;
impl Hooks for QuietHooks {
    fn supports_inert_ore_writes(&self) -> bool { true }
    fn populate(&self, _: &mut Access<'_>, _: ChunkPos, _: &mut JavaRandom) -> Result<()> { Ok(()) }
    fn player_random_state(&self) -> u64 { JavaRandom::new(189).state() }
}
fn source(runtime: &mut Runtime, p: IVec3, state: u16) {
    runtime.access().set_block(p, state).unwrap();
}
fn digest(value: &impl serde::Serialize) -> String {
    struct HashWriter(Sha1);
    impl std::io::Write for HashWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    let mut writer=HashWriter(Sha1::new());
    serde_json::to_writer(&mut writer,value).unwrap();
    format!("{:x}",writer.0.finalize())
}
fn capture(runtime: &Runtime) -> serde_json::Value {
    let mut snapshot = runtime.checkpoint().unwrap();
    snapshot.dirty_chunks.sort_unstable();
    snapshot.edits.sort_unstable_by_key(|(p, _)| *p);
    // Keep complete checkpoint semantics (including entity RNG) while retaining
    // only a compact per-field hash between the paired runs.
    let fields = serde_json::to_value(snapshot).unwrap().as_object().unwrap().iter()
        .map(|(name, value)| (name.clone(), serde_json::json!(digest(value))))
        .collect::<serde_json::Map<_, _>>();
    let indexes = runtime.provider.loaded_order().iter().map(|p| {
        let chunk = &runtime.backend.world.chunks[p];
        (*p, chunk.random_tick_mask(), chunk.scheduled_tick_candidates().to_vec())
    }).collect::<Vec<_>>();
    serde_json::json!({"checkpoint":fields,"reads":runtime.backend.profile.access_reads,
        "writes":runtime.backend.profile.access_writes,"revision":runtime.backend.world.light_revision,
        "indexes":indexes})
}
fn equal(a: &serde_json::Value, b: &serde_json::Value, stage: usize) {
    for (field, value) in a.as_object().unwrap() {
        if field == "checkpoint" {
            for (name, hash) in value.as_object().unwrap() {
                assert!(hash == &b[field][name], "column stage {stage}: checkpoint {name} differs");
            }
        } else { assert!(value == &b[field], "column stage {stage}: {field} differs"); }
    }
}

#[test]
fn scoped_columns_preserve_multirow_order_cow_indexes_journal_wrap_and_dirty_resets() {
    let points = [-1, 0].into_iter().flat_map(|x| [16, 17].into_iter()
        .flat_map(move |y| (-8..8).map(move |z| IVec3::new(x, y, z)))).collect::<Vec<_>>();
    assert_eq!(points.len(), 64);
    let columns = [(-1, -1), (-1, 0), (0, -1), (0, 0)];
    for no_sky in [false, true] {
        let mut base = Runtime::new(189, 1234, false, Arc::new(QuietHooks));
        for pos in columns { base.provide(pos).unwrap(); }
        for (i, &p) in points.iter().enumerate() {
            source(&mut base, p, if i % 11 == 0 { 64 } else { 16 | (i % 7) as u16 });
        }
        base.backend.light = Lighting::new(&base.backend.world, no_sky);
        let snapshot = base.checkpoint().unwrap();
        for journal in [false, true] {
            let mut runs = Vec::new();
            for reference in [true, false] {
                let mut runtime = Runtime::resume(snapshot.clone(), Arc::new(QuietHooks)).unwrap();
                runtime.use_overlay_journal(journal);
                runtime.backend.reference_inert_column = reference;
                for chunk in runtime.backend.world.chunks.values() { chunk.prepare_tick_indexes(); }
                let retained = columns.map(|p| (p, runtime.backend.world.chunks[&p].clone()));
                let old_hashes = retained.iter().map(|(_, c)| digest(&c.blocks)).collect::<Vec<_>>();
                runtime.access().replace_ore_line(&points, 17, 1);
                for ((p, chunk), hash) in retained.iter().zip(&old_hashes) {
                    assert_eq!(&digest(&chunk.blocks), hash, "published snapshot changed");
                    assert!(!Arc::ptr_eq(chunk, &runtime.backend.world.chunks[p]));
                }
                let mut stages = vec![capture(&runtime)];
                let same_chunk = runtime.backend.world.chunks[&(-1, -1)].clone();
                let writes = runtime.backend.inert_column_writes;
                let revision = runtime.backend.world.light_revision;
                runtime.access().replace_ore_line(&[points[1], points[1], points[11]], 17, 1);
                assert_eq!(runtime.backend.inert_column_writes, writes);
                assert_eq!(runtime.backend.world.light_revision, revision);
                assert!(Arc::ptr_eq(&same_chunk, &runtime.backend.world.chunks[&(-1, -1)]));
                drop(same_chunk);
                stages.push(capture(&runtime));

                for &p in &points { source(&mut runtime, p, 16); }
                drop(runtime.take_chunk_updates());
                assert!(runtime.backend.dirty_chunks.is_empty());
                for p in columns { runtime.provider.record_mut(p).unwrap().modified = false; }
                runtime.backend.world.light_revision = u64::MAX - 1;
                let weak = Arc::downgrade(&runtime.backend.world.chunks[&(0, 0)]);
                let begin = runtime.backend.block_effects.changed.len();
                runtime.access().replace_ore_line(&points, 224, 1);
                assert_eq!(runtime.backend.world.light_revision, 62);
                assert!(weak.upgrade().is_none(), "first changed write must dissociate weak snapshots");
                assert_eq!(&runtime.backend.block_effects.changed[begin..], points.as_slice());
                assert!(columns.iter().all(|p| runtime.backend.dirty_chunks.contains(p)
                    && runtime.provider.record(*p).unwrap().modified));
                if reference { assert_eq!(runtime.backend.inert_column_writes, 0); }
                else {
                    assert!(runtime.backend.inert_column_writes >= 64);
                    assert!(runtime.backend.inert_column_batches < runtime.backend.inert_column_writes);
                }
                stages.push(capture(&runtime));
                let mut resumed = Runtime::resume(runtime.checkpoint().unwrap(), Arc::new(QuietHooks)).unwrap();
                resumed.use_overlay_journal(journal);
                resumed.backend.reference_inert_column = reference;
                assert_eq!(resumed.backend.inert_column_writes, 0);
                for &p in &points { source(&mut resumed, p, 16); }
                resumed.access().replace_ore_line(&points, 240, 1);
                stages.push(capture(&resumed));
                runs.push(stages);
            }
            for (stage, (a, b)) in runs[0].iter().zip(&runs[1]).enumerate() { equal(a, b, stage); }
        }
    }
}

#[test]
fn scoped_column_falls_back_before_section_surface_and_unread_boundaries() {
    let points = [IVec3::new(1,20,1), IVec3::new(2,52,2), IVec3::new(3,20,3),
        IVec3::new(4,70,4), IVec3::new(5,20,5), IVec3::new(6,-1,6),
        IVec3::new(7,20,7), IVec3::new(16,20,16), IVec3::new(8,20,8)];
    let mut base = Runtime::new(189,1234,false,Arc::new(QuietHooks));
    base.provide((0,0)).unwrap();
    for &p in points.iter().filter(|p| valid(**p) && cp(**p)==(0,0)) { source(&mut base,p,16); }
    let snapshot=base.checkpoint().unwrap();
    let mut runs=Vec::new();
    for reference in [true,false] {
        let mut runtime=Runtime::resume(snapshot.clone(),Arc::new(QuietHooks)).unwrap();
        runtime.backend.reference_inert_column=reference;
        let light=runtime.backend.light.chunks.get_mut(&(0,0)).unwrap();
        light.sections[3]=false;
        light.height[2*16+2]=100;
        light.sections[4]=true;
        light.height[4*16+4]=70;
        runtime.access().replace_ore_line(&points,224,1);
        assert!(runtime.backend.light.chunks[&(0,0)].sections[3]);
        assert!(runtime.provider.contains((1,1)), "unread boundary must still load its column");
        assert_eq!(runtime.backend.world.get(points[8]),224,"resume after missing-column fallback");
        if !reference { assert!(runtime.backend.inert_column_batches>=3); }
        runs.push(capture(&runtime));
    }
    equal(&runs[0],&runs[1],0);
}

#[test]
fn declined_first_candidate_preserves_arc_and_all_logical_write_state() {
    let mut runtime=Runtime::new(189,1234,false,Arc::new(QuietHooks));
    runtime.provide((0,0)).unwrap();
    let p=IVec3::new(4,20,4);
    source(&mut runtime,p,16);
    for mode in 0..6 {
        let saved_light=runtime.backend.light.chunks[&(0,0)].clone();
        let mut missing=None;
        match mode {
            0=>runtime.backend.light.chunks.get_mut(&(0,0)).unwrap().sections[1]=false,
            1=>runtime.backend.light.chunks.get_mut(&(0,0)).unwrap().height[4*16+4]=20,
            2=>missing=runtime.backend.light.chunks.remove(&(0,0)),
            3=>runtime.backend.light.use_reference_path(true),
            4=>runtime.backend.reference_inert_column=true,
            _=>{},
        }
        let retained=runtime.backend.world.chunks[&(0,0)].clone();
        let before=(runtime.backend.profile.access_reads,runtime.backend.profile.access_writes,
            runtime.backend.world.light_revision,runtime.backend.output.changed.len(),
            runtime.backend.inert_column_writes,runtime.backend.inert_column_batches);
        let result=runtime.access().with(|provider,b|try_write_column(provider,b,&[p],1,
            ReadCandidate{position:p,old:16},if mode==5 {16}else{224},1));
        assert!(matches!(result,ColumnProgress::Declined));
        assert!(Arc::ptr_eq(&retained,&runtime.backend.world.chunks[&(0,0)]));
        assert_eq!(before,(runtime.backend.profile.access_reads,runtime.backend.profile.access_writes,
            runtime.backend.world.light_revision,runtime.backend.output.changed.len(),
            runtime.backend.inert_column_writes,runtime.backend.inert_column_batches));
        runtime.backend.light.chunks.insert((0,0),missing.unwrap_or(saved_light));
        runtime.backend.light.use_reference_path(false);
        runtime.backend.reference_inert_column=false;
    }
}

struct FallbackHooks { armed: Cell<bool> }
impl Hooks for FallbackHooks {
    fn supports_inert_ore_writes(&self)->bool {true}
    fn player_random_state(&self)->u64 {JavaRandom::new(189).state()}
    fn populate(&self,_:&mut Access<'_>,_:ChunkPos,_:&mut JavaRandom)->Result<()> {Ok(())}
    fn block_added(&self,access:&mut Access<'_>,p:IVec3,_:u16,new:u16)->Result<()> {
        // This one armed target has live inventory metadata before its write,
        // so it is explicitly outside the metadata-free opt-in contract.
        if p==IVec3::new(4,20,3) && new==224 && self.armed.replace(false) {
            let n=access.world_random().next_int(1000);
            access.with(|_,b|b.event(&format!("metadata-fallback:{n}"),(p.x,p.z)));
            access.set_block(IVec3::new(4,20,4),64)?;
        }
        Ok(())
    }
}

#[test]
fn scoped_column_releases_before_metadata_spills_and_reentrant_future_mutations() {
    use crate::{inventory::Stack,vanilla::drops::{DropNbt,DropTile}};
    let hooks=||Arc::new(FallbackHooks{armed:Cell::new(true)});
    let mut base=Runtime::new(189,1234,false,hooks());
    base.provide((0,0)).unwrap();
    let points=(0..9).map(|z|IVec3::new(4,20,z)).collect::<Vec<_>>();
    for &p in &points {source(&mut base,p,16);}
    base.backend.entities.inventory_random=Some((JavaRandom::new(54321).state(),None));
    base.backend.entities.math_state=Some(JavaRandom::new(6789).state());
    let snapshot=base.checkpoint().unwrap();
    let mut runs=Vec::new();
    for reference in [true,false] {
        let mut runtime=Runtime::resume(snapshot.clone(),hooks()).unwrap();
        runtime.backend.reference_inert_column=reference;
        Arc::make_mut(runtime.backend.world.chunks.get_mut(&(0,0)).unwrap()).tiles.insert(
            points[1].to_array(),Arc::new(DropNbt::Compound(BTreeMap::from([("id".into(),DropNbt::String("Chest".into()))]))));
        let mut chest=LiveContainer::empty(points[3].to_array(),54,1,27);
        chest.slots[0]=Some(Stack::new(264,49));
        runtime.backend.tile_revision=1;
        runtime.backend.live_containers.insert(points[3].to_array(),chest);
        runtime.backend.live_spawners.insert(points[5].to_array(),"Pig".into());
        runtime.backend.drop_tiles.insert(points[7].to_array(),DropTile::Piston{state:16});
        let begin=runtime.backend.block_effects.changed.len();
        runtime.access().replace_ore_line(&points,224,1);
        assert_eq!(&runtime.backend.block_effects.changed[begin..],
            &[points[0],points[1],points[2],points[4],points[3],points[5],points[6],points[7],points[8]]);
        assert_eq!(runtime.backend.world.get(points[4]),64,"future predicate must observe nested write");
        assert!(runtime.backend.live_containers.is_empty()&&runtime.backend.live_spawners.is_empty()
            &&runtime.backend.drop_tiles.is_empty()&&runtime.backend.world.chunks[&(0,0)].tiles.is_empty());
        assert_eq!(runtime.backend.entities.live.values().map(|e|u32::from(e.stack.count)).sum::<u32>(),49);
        assert_eq!(runtime.backend.inert_column_writes,if reference {0}else{4});
        runs.push(capture(&runtime));
    }
    equal(&runs[0],&runs[1],0);
}

struct OptOutHooks;
impl Hooks for OptOutHooks {
    fn populate(&self,_:&mut Access<'_>,_:ChunkPos,_:&mut JavaRandom)->Result<()> {Ok(())}
    fn block_added(&self,access:&mut Access<'_>,p:IVec3,_:u16,new:u16)->Result<()> {
        if p==IVec3::new(4,20,0) && new==224 {access.set_block(p+IVec3::Z,64)?;}
        Ok(())
    }
}
#[test]
fn column_optimization_respects_all_reference_switches_and_custom_hook_opt_out() {
    let points=[IVec3::new(4,20,0),IVec3::new(4,20,1)];
    let mut base=Runtime::new(189,1234,false,Arc::new(QuietHooks));
    base.provide((0,0)).unwrap();
    for &p in &points {source(&mut base,p,16);}
    let snapshot=base.checkpoint().unwrap();
    let mut expected=None;
    for mode in 0..4 {
        let mut runtime=Runtime::resume(snapshot.clone(),Arc::new(QuietHooks)).unwrap();
        runtime.backend.reference_inert_column=mode==1;
        runtime.backend.reference_inert_ore=mode==2;
        runtime.backend.reference_ore_batch=mode==3;
        runtime.access().replace_ore_line(&points,224,1);
        assert_eq!(runtime.backend.inert_column_writes,if mode==0 {2}else{0});
        if mode>=2 {assert_eq!(runtime.backend.inert_ore_writes,0);}
        let state=capture(&runtime);
        if let Some(expected)=&expected {equal(expected,&state,mode);}
        else {expected=Some(state);}
    }
    let mut base=Runtime::new(189,1234,false,Arc::new(OptOutHooks));
    base.provide((0,0)).unwrap();
    for &p in &points {source(&mut base,p,16);}
    let snapshot=base.checkpoint().unwrap();
    let mut runs=Vec::new();
    for reference in [true,false] {
        let mut runtime=Runtime::resume(snapshot.clone(),Arc::new(OptOutHooks)).unwrap();
        runtime.backend.reference_inert_column=reference;
        runtime.access().replace_ore_line(&points,224,1);
        assert_eq!(runtime.backend.world.get(points[1]),64);
        assert_eq!(runtime.backend.inert_column_writes,0);
        runs.push(capture(&runtime));
    }
    equal(&runs[0],&runs[1],0);
}

struct PrecipitationResetHooks { armed: Cell<bool> }
impl Hooks for PrecipitationResetHooks {
    fn supports_inert_ore_writes(&self) -> bool { true }
    fn player_random_state(&self) -> u64 { JavaRandom::new(189).state() }
    fn populate(&self, _: &mut Access<'_>, _: ChunkPos, _: &mut JavaRandom) -> Result<()> { Ok(()) }
    fn block_added(&self, access: &mut Access<'_>, p: IVec3, _: u16, new: u16) -> Result<()> {
        // The armed target has stored tile metadata, so this callback is outside
        // the metadata-free opt-in contract and must execute through fallback.
        if p == IVec3::new(-4,20,-10) && new == 17 && self.armed.replace(false) {
            access.with(|_, b| {
                let column = |z: i32| (z.rem_euclid(16)*16+12) as usize;
                let before = &b.precipitation[&(-1,-1)];
                assert_eq!((-15..=-10).map(|z| before[column(z)]).collect::<Vec<_>>(),
                    vec![22,-999,22,-999,-999,-999]);
                // Replace the allocation, not just its values. A scoped borrow
                // must be released before fallback and reacquired afterwards.
                b.precipitation.remove(&(-1,-1));
                let mut cache = vec![22;256];
                for z in [-9,-7,-6,-5] { cache[column(z)] = 21; }
                b.precipitation.insert((-1,-1),cache);
                Arc::make_mut(b.world.chunks.get_mut(&(-1,-1)).unwrap()).tiles
                    .remove(&[12,20,6]);
            });
        }
        Ok(())
    }
}

#[test]
fn scoped_precipitation_cache_preserves_noop_boundaries_and_reacquires_after_fallback() {
    use crate::vanilla::drops::DropNbt;
    let points = [IVec3::new(-4,20,-15), IVec3::new(-4,20,-14),
        IVec3::new(-4,20,-13), IVec3::new(-4,20,-12), IVec3::new(-4,20,-11),
        IVec3::new(-4,20,-10), IVec3::new(-4,20,-9), IVec3::new(-4,20,-8),
        IVec3::new(-4,20,-7), IVec3::new(0,20,-7), IVec3::new(-4,20,-6),
        IVec3::new(-4,20,-5)];
    // Checkpoints retain the population-hook type. Use the same hook here,
    // disarmed until the resumed run installs its stale-metadata target.
    let mut base = Runtime::new(189,1234,false,
        Arc::new(PrecipitationResetHooks{armed:Cell::new(false)}));
    for pos in [(-1,-1),(0,-1)] { base.provide(pos).unwrap(); }
    for (i, &p) in points.iter().enumerate() {
        source(&mut base,p,if matches!(i,0|1|3|6|10) {17}else{16});
    }
    let snapshot = base.checkpoint().unwrap();
    let mut runs = Vec::new();
    for mode in [3,1,0] {
        let mut runtime = Runtime::resume(snapshot.clone(),
            Arc::new(PrecipitationResetHooks{armed:Cell::new(true)})).unwrap();
        runtime.backend.reference_ore_batch = mode == 3;
        runtime.backend.reference_inert_column = mode == 1;
        let mut cache = vec![21;256];
        for p in [points[0],points[2]] {
            cache[(p.z.rem_euclid(16)*16+p.x.rem_euclid(16)) as usize] = 22;
        }
        runtime.backend.precipitation.insert((-1,-1),cache);
        let other_cache = runtime.backend.precipitation.remove(&(0,-1)).unwrap();
        Arc::make_mut(runtime.backend.world.chunks.get_mut(&(-1,-1)).unwrap()).tiles.insert(
            [12,20,6],Arc::new(DropNbt::Compound(BTreeMap::from([
                ("id".into(),DropNbt::String("Chest".into()))]))));
        // Repeat a same-state attempt before the first changed candidate, then
        // cover same-state attempts inside a retained writer and after fallback.
        let mut ordered = points.to_vec();
        ordered.insert(2,points[1]);
        runtime.access().replace_ore_line(&ordered,17,1);
        assert!(!runtime.backend.precipitation.contains_key(&(0,-1)),
            "a missing optional cache must not be inserted by a write");
        let at = |p: IVec3| (p.z.rem_euclid(16)*16+p.x.rem_euclid(16)) as usize;
        let result = &runtime.backend.precipitation[&(-1,-1)];
        assert_eq!(points.iter().enumerate().filter(|(i,_)| *i!=9)
            .map(|(_, &p)| result[at(p)]).collect::<Vec<_>>(),
            vec![22,22,22,22,22,22,-999,22,-999,-999,-999]);
        assert!(runtime.backend.world.chunks[&(-1,-1)].tiles.is_empty());
        if mode == 0 { assert!(runtime.backend.inert_column_writes >= 4); }
        runtime.backend.precipitation.insert((0,-1),other_cache);
        let mut stages = vec![capture(&runtime)];

        // A new invocation must re-read cache availability, even for a wholly
        // no-op run. There is no chunk COW or block revision for these attempts.
        runtime.backend.precipitation.remove(&(-1,-1));
        let retained = runtime.backend.world.chunks[&(-1,-1)].clone();
        let revision = runtime.backend.world.light_revision;
        runtime.access().replace_ore_line(&points[..9],17,1);
        assert!(!runtime.backend.precipitation.contains_key(&(-1,-1)));
        assert!(Arc::ptr_eq(&retained,&runtime.backend.world.chunks[&(-1,-1)]));
        assert_eq!(runtime.backend.world.light_revision,revision);
        runtime.backend.precipitation.insert((-1,-1),vec![21;256]);
        runtime.access().replace_ore_line(&points[..9],17,1);
        assert!(points[..9].iter().all(|&p| runtime.backend.precipitation[&(-1,-1)][at(p)] == -999));
        stages.push(capture(&runtime));
        runs.push(stages);
    }
    for other in &runs[1..] {
        for (stage,(expected,actual)) in runs[0].iter().zip(other).enumerate() {
            equal(expected,actual,stage);
        }
    }
}

#[test]
#[ignore="paired full original625 scoped-column profile; run alone with optimized tests"]
fn original_625_scoped_column_profile_preserves_hash_rng_order_effects_and_accesses() {
    fn terrain_hash(runtime:&Runtime)->String {
        let mut hash=0xcbf29ce484222325u64;
        let mut add=|bytes:&[u8]| {for &byte in bytes {hash=(hash^u64::from(byte)).wrapping_mul(0x100000001b3);}};
        for &pos in runtime.provider.loaded_order() {
            add(&pos.0.to_le_bytes());add(&pos.1.to_le_bytes());
            let chunk=&runtime.backend.world.chunks[&pos];
            for block in &chunk.blocks {add(&block.to_le_bytes());}
            add(&chunk.biomes);
            let light=&runtime.backend.light.chunks[&pos];
            add(&light.sky);add(&light.block);
            for height in &light.height {add(&height.to_le_bytes());}
        }
        format!("{hash:016x}")
    }
    let mut rows=Vec::new();
    let mut expected:Option<serde_json::Value>=None;
    for reference in [true,false,true,false] {
        let mut runtime=Runtime::for_overworld_verification(189,1234,true);
        runtime.use_overlay_journal(false);
        runtime.backend.reference_inert_column=reference;
        let begin=Instant::now();
        runtime.prepare_overworld_spawn().unwrap();
        let spawn_ms=begin.elapsed().as_secs_f64()*1000.;
        let begin=Instant::now();
        let mut publication_ms=0.;
        for start in (0..625).step_by(32) {
            runtime.load_initial_spawn_batch(start,32).unwrap();
            let before=Instant::now();
            drop(runtime.take_chunk_updates());
            publication_ms+=before.elapsed().as_secs_f64()*1000.;
        }
        let wall_ms=begin.elapsed().as_secs_f64()*1000.;
        let fingerprint=terrain_hash(&runtime);
        assert_eq!(fingerprint,"00971aef0283004f");
        assert_eq!(runtime.backend.provider_random.state(),78486281047925);
        assert_eq!(runtime.backend.world_random.state(),210890393092178);
        let state=serde_json::json!({"terrain":fingerprint,"provider":digest(&runtime.provider),
            "structures":digest(&runtime.backend.structures),"events":digest(&runtime.backend.events),
            "animals":digest(&runtime.backend.animals),"decorators":digest(&runtime.backend.decorators),
            "containers":digest(&runtime.backend.live_containers.iter().collect::<Vec<_>>()),
            "precipitation":digest(&runtime.backend.precipitation.iter().collect::<Vec<_>>()),
            "ticks":digest(&runtime.backend.ticks),"output":digest(&runtime.backend.output),
            "effects":digest(&runtime.backend.block_effects.changed),
            "reads":runtime.backend.profile.access_reads,"writes":runtime.backend.profile.access_writes,
            "revision":runtime.backend.world.light_revision,"tile_revision":runtime.backend.tile_revision});
        if let Some(expected)=&expected {
            for (name,value) in expected.as_object().unwrap() {
                assert!(value==&state[name],"scoped-column original625 field {name} differs");
            }
        } else {expected=Some(state.clone());}
        if reference {assert_eq!(runtime.backend.inert_column_writes,0);}
        else {assert!(runtime.backend.inert_column_writes>100_000);}
        let row=serde_json::json!({"reference_inert_column":reference,"wall_ms":wall_ms,
            "spawn_ms":spawn_ms,"publication_ms":publication_ms,"terrain":runtime.terrain_timings(),
            "runtime":runtime.runtime_timings(),"fingerprint":fingerprint,"state":state,
            "column_writes":runtime.backend.inert_column_writes,
            "column_batches":runtime.backend.inert_column_batches});
        eprintln!("{row}");rows.push(row);
    }
    std::fs::create_dir_all("reports").unwrap();
    std::fs::write("reports/native-startup-scoped-column-profile.json",serde_json::to_vec_pretty(&rows).unwrap()).unwrap();
}
