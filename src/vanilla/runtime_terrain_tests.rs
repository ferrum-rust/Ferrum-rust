use super::*;

struct PreparedLightHooks;
impl Hooks for PreparedLightHooks {
    fn player_random_state(&self) -> u64 { JavaRandom::new(189).state() }
    fn populate(&self, access: &mut Access<'_>, pos: ChunkPos, _: &mut JavaRandom) -> Result<()> {
        if pos == (0,0) {
            // Consume a prepared future column from inside original neighbor
            // population, then edit it after its initial light is installed.
            let p=IVec3::new(5*16+3,120,-4*16+7);
            access.provide((5,-4))?;
            access.set_block(p,89 << 4)?;
        }
        Ok(())
    }
}
fn checkpoint_bytes(runtime: &Runtime) -> Vec<u8> {
    let mut snapshot=runtime.checkpoint().unwrap();
    snapshot.dirty_chunks.sort_unstable();
    snapshot.edits.sort_unstable_by_key(|(p,_)| *p);
    serde_json::to_vec(&snapshot).unwrap()
}

#[test]
fn prepared_light_install_preserves_nested_loads_callbacks_rng_and_dirty_publication() {
    let positions=[(0,0),(0,1),(1,0),(1,1),(5,-4)];
    let mut expected=None;
    for reference in [true,false] {
        let mut runtime=Runtime::new(189,1234,true,Arc::new(PreparedLightHooks));
        runtime.backend.reference_prepared_light=reference;
        let before=checkpoint_bytes(&runtime);
        runtime.prefetch_terrain(&positions).unwrap();
        runtime.prepare_terrain(&positions).unwrap();
        assert_eq!(checkpoint_bytes(&runtime),before,"prepared arrays must stay private");
        assert!(runtime.backend.light.take_render_dirty().is_empty());
        let mut stages=Vec::new();
        for &pos in &positions[..4] {
            runtime.provide(pos).unwrap();
            stages.push((checkpoint_bytes(&runtime),runtime.backend.light.take_render_dirty()));
        }
        assert!(runtime.provider.contains((5,-4)));
        assert!(runtime.backend.prepared_terrain.is_empty());
        assert_eq!(runtime.backend.world.get(IVec3::new(83,120,-57)),89 << 4);
        assert_eq!(runtime.terrain_timings().prepared_light_installed,if reference {0}else{5});
        assert_eq!(runtime.terrain_timings().prepared_light_fallbacks,0);
        if let Some(expected)=&expected { assert!(expected==&stages,"prepared light changed callback-boundary state"); }
        else { expected=Some(stages); }
    }
}

#[test]
fn prepared_terrain_light_rechecks_dimension_and_reference_mode_at_installation() {
    let pos=(7,-9);
    for (prepared_mode,current_mode,reference) in [(false,true,false),(true,false,false),(false,false,true)] {
        let mut runtime=Runtime::new(189,1234,false,Arc::new(PreparedLightHooks));
        runtime.backend.light.no_sky=prepared_mode;
        runtime.prepare_terrain(&[pos]).unwrap();
        assert!(runtime.backend.prepared_terrain[&pos].2.is_some());
        runtime.backend.light.no_sky=current_mode;
        runtime.backend.light.use_reference_path(reference);
        runtime.provide(pos).unwrap();
        let mut serial=Runtime::new(189,1234,false,Arc::new(PreparedLightHooks));
        serial.backend.light.no_sky=current_mode;
        serial.backend.light.use_reference_path(reference);
        serial.provide(pos).unwrap();
        assert_eq!(checkpoint_bytes(&runtime),checkpoint_bytes(&serial));
        assert_eq!(runtime.backend.light.take_render_dirty(),serial.backend.light.take_render_dirty());
        assert_eq!(runtime.terrain_timings().prepared_light_installed,0);
        assert_eq!(runtime.terrain_timings().prepared_light_fallbacks,1);
    }
    let mut runtime=Runtime::new(189,1234,false,Arc::new(PreparedLightHooks));
    runtime.backend.light.use_reference_path(true);
    runtime.prepare_terrain(&[pos]).unwrap();
    assert!(runtime.backend.prepared_terrain[&pos].2.is_none(),"reference preparation must keep owner calculation");
}

#[test]
fn lookahead_is_bounded_and_cannot_publish_chunks_rng_or_checkpoint_state() {
    let mut runtime = Runtime::for_overworld_verification(189, 1234, true);
    let untouched = serde_json::to_value(runtime.checkpoint().unwrap()).unwrap();
    let positions = (0..128)
        .map(|i| (i / 8 + 40, i % 8 - 40))
        .collect::<Vec<_>>();
    runtime.prefetch_terrain(&positions).unwrap();
    assert_eq!(runtime.backend.terrain_jobs.len(), 64);
    assert_eq!(runtime.terrain_timings().lookahead_submitted, 64);
    runtime.prefetch_terrain(&positions).unwrap();
    assert_eq!(runtime.terrain_timings().lookahead_submitted, 64);
    assert_eq!(
        serde_json::to_value(runtime.checkpoint().unwrap()).unwrap(),
        untouched,
        "speculation must be absent from provider records, lights, RNG and saves"
    );
    runtime.prepare_terrain(&positions[..64]).unwrap();
    assert_eq!(runtime.backend.prepared_terrain.len(), 64);
    assert!(runtime.backend.terrain_jobs.is_empty());
    assert_eq!(
        serde_json::to_value(runtime.checkpoint().unwrap()).unwrap(),
        untouched
    );

    // Consume a future coordinate first, as a reentrant decorator can do.
    let pos = positions[37];
    runtime.provide(pos).unwrap();
    let mut serial = Runtime::for_overworld_verification(189, 1234, true);
    serial.provide(pos).unwrap();
    assert_eq!(
        runtime.provider.loaded_order(),
        serial.provider.loaded_order()
    );
    assert_eq!(runtime.backend.events, serial.backend.events);
    assert_eq!(
        runtime.backend.provider_random.state(),
        serial.backend.provider_random.state()
    );
    assert_eq!(
        runtime.backend.world_random.state(),
        serial.backend.world_random.state()
    );
    assert_eq!(
        runtime.backend.world.chunks[&pos].blocks,
        serial.backend.world.chunks[&pos].blocks
    );
    assert_eq!(
        serde_json::to_value(&runtime.backend.light).unwrap(),
        serde_json::to_value(&serial.backend.light).unwrap()
    );
    runtime.prefetch_terrain(&positions[64..]).unwrap();
    assert_eq!(
        runtime.backend.terrain_jobs.len() + runtime.backend.prepared_terrain.len(),
        64
    );
    assert_eq!(runtime.terrain_timings().lookahead_submitted, 65);
    assert_eq!(runtime.terrain_timings().lookahead_peak, 64);
    runtime.prepare_terrain(&positions[64..65]).unwrap();

    let snapshot = runtime.checkpoint().unwrap();
    let resumed = Runtime::resume(snapshot, Arc::new(VanillaPopulation)).unwrap();
    assert!(resumed.backend.terrain_jobs.is_empty());
    assert!(resumed.backend.prepared_terrain.is_empty());
    assert_eq!(
        resumed.provider.loaded_order(),
        serial.provider.loaded_order()
    );
    assert_eq!(
        resumed.backend.world.chunks[&pos].blocks,
        serial.backend.world.chunks[&pos].blocks
    );
}

/// Run alone in an optimized test binary: owner generation/population timings
/// exclude final fingerprinting and rendering. This reports, never asserts, a
/// speed ratio because background load and CPU scheduling vary between hosts.
#[test]
#[ignore = "paired startup performance profile; run alone in an optimized binary"]
fn original_625_startup_lookahead_reports_phases_and_equal_worlds() {
    use serde_json::json;
    let mut rows = Vec::new();
    let mut expected = None;
    for lookahead in [false, true, false, true] {
        let mut runtime = Runtime::for_overworld_verification(189, 1234, true);
        let point = runtime.prepare_overworld_spawn().unwrap();
        let region = initial_spawn_region(point);
        let started = Instant::now();
        let mut publication_ms = 0.;
        for start in (0..625).step_by(32) {
            let end = (start + 32).min(625);
            if lookahead {
                runtime.load_initial_spawn_batch(start, 32).unwrap();
            } else {
                runtime.prepare_terrain(&region[start..end]).unwrap();
                for &pos in &region[start..end] {
                    runtime.provider.load(&mut runtime.backend, pos).unwrap();
                }
            }
            let before = Instant::now();
            drop(runtime.take_chunk_updates());
            publication_ms += before.elapsed().as_secs_f64() * 1000.;
        }
        let wall_ms = started.elapsed().as_secs_f64() * 1000.;
        let fingerprint = fingerprint(&runtime);
        if let Some(expected) = &expected {
            assert_eq!(&fingerprint, expected);
        } else {
            expected = Some(fingerprint.clone());
        }
        let row = json!({"lookahead":lookahead,"wall_ms":wall_ms,"publication_ms":publication_ms,
            "terrain":runtime.terrain_timings(),"blocks_biomes_lights":fingerprint["blocks_biomes_lights"],
            "provider_random":fingerprint["provider_random"],"world_random":fingerprint["world_random"],
            "loaded_chunks":runtime.provider.loaded_order().len()});
        eprintln!("{row}");
        rows.push(row);
    }
    let report = json!({"seed":189,"world_random_seed":1234,"batch":32,"limit":64,"runs":rows});
    std::fs::create_dir_all("reports").unwrap();
    std::fs::write(
        "reports/native-startup-lookahead-profile.json",
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}

fn fingerprint(runtime: &Runtime) -> serde_json::Value {
    let mut hash = 0xcbf29ce484222325u64;
    let mut consume = |bytes: &[u8]| {
        for &byte in bytes {
            hash = (hash ^ byte as u64).wrapping_mul(0x100000001b3);
        }
    };
    for &pos in runtime.provider.loaded_order() {
        consume(&pos.0.to_le_bytes());
        consume(&pos.1.to_le_bytes());
        let chunk = &runtime.backend.world.chunks[&pos];
        for block in chunk.blocks.iter() {
            consume(&block.to_le_bytes());
        }
        consume(&chunk.biomes);
        let light = &runtime.backend.light.chunks[&pos];
        consume(&light.sky);
        consume(&light.block);
        for height in &light.height {
            consume(&height.to_le_bytes());
        }
    }
    serde_json::json!({"blocks_biomes_lights":format!("{hash:016x}"),
        "provider":runtime.provider,"structures":runtime.backend.structures,
        "provider_random":runtime.backend.provider_random.state(),
        "world_random":runtime.backend.world_random.state(),"animals":runtime.backend.animals,
        "containers":runtime.backend.live_containers.iter().collect::<Vec<_>>(),
        "events":runtime.backend.events})
}

struct LightProfileHooks;
fn constructor_state(id:u64,family:u64)->u64 {
    let mut random=JavaRandom::new((id.wrapping_mul(0x9e3779b97f4a7c15)^family)as i64);
    random.next_long();random.next_long();random.state()
}
impl Hooks for LightProfileHooks {
    fn supports_ore_coordinate_lookahead(&self)->bool { true }
    fn supports_inert_ore_writes(&self)->bool { VanillaPopulation.supports_inert_ore_writes() }
    fn populate(&self,access:&mut Access<'_>,pos:ChunkPos,random:&mut JavaRandom)->Result<()> {
        VanillaPopulation.populate(access,pos,random)
    }
    fn repopulate(&self,access:&mut Access<'_>,pos:ChunkPos)->Result<bool> {
        VanillaPopulation.repopulate(access,pos)
    }
    fn player_random_state(&self)->u64 {constructor_state(0,189)}
    fn item_random_state(&self,id:u64)->u64 {constructor_state(id,190)}
    fn orb_random_state(&self,id:i32)->u64 {constructor_state(id as u64,191)}
    fn falling_random_state(&self,id:u64)->u64 {constructor_state(id,192)}
}
fn checkpoint_digest(runtime:&Runtime)->String {
    use sha1::{Digest,Sha1};
    struct HashWriter(Sha1);
    impl std::io::Write for HashWriter {
        fn write(&mut self,bytes:&[u8])->std::io::Result<usize> {self.0.update(bytes);Ok(bytes.len())}
        fn flush(&mut self)->std::io::Result<()> {Ok(())}
    }
    let mut snapshot=runtime.checkpoint().unwrap();
    snapshot.dirty_chunks.sort_unstable();snapshot.edits.sort_unstable_by_key(|(p,_)|*p);
    let mut writer=HashWriter(Sha1::new());
    serde_json::to_writer(&mut writer,&snapshot).unwrap();
    format!("{:x}",writer.0.finalize())
}
#[test]
#[ignore="paired full original625 initial-light preparation profile; run alone with optimized tests"]
fn original_625_prepared_initial_light_preserves_hash_rng_callbacks_and_publication() {
    let mut expected=None;
    let mut reports=Vec::new();
    for reference in [true,false,true,false] {
        let mut runtime=Runtime::new(189,1234,true,Arc::new(LightProfileHooks));
        runtime.backend.reference_prepared_light=reference;
        runtime.backend.ore_lookahead_mode=Some(if reference {
            super::super::super::population::ore_lookahead::TestMode::Disabled
        } else { super::super::super::population::ore_lookahead::TestMode::Ready });
        // The complete checkpoint oracle also keeps the original independent
        // light checks and World::get precipitation scan in its reference arm.
        runtime.backend.light.use_reference_path(reference);
        runtime.backend.reference_precipitation_scan=reference;
        runtime.use_reference_publication(reference);
        runtime.use_overlay_journal(false);
        runtime.backend.entities.math_state=Some(constructor_state(0,193));
        runtime.backend.entities.inventory_random=Some((constructor_state(0,194),None));
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
        // Shared packed planes belong exclusively to the client lighting view.
        // Generation and checkpoint ownership must retain native Dense/Cold data.
        for light in runtime.backend.light.chunks.values() {
            assert!(!light.sky.is_shared_packed() && !light.block.is_shared_packed());
        }
        let terrain=fingerprint(&runtime);
        assert_eq!(terrain["blocks_biomes_lights"],"00971aef0283004f");
        assert_eq!(runtime.backend.provider_random.state(),78486281047925);
        assert_eq!(runtime.backend.world_random.state(),210890393092178);
        let state=serde_json::json!({"terrain":terrain,"checkpoint":checkpoint_digest(&runtime),
            "reads":runtime.backend.profile.access_reads,"writes":runtime.backend.profile.access_writes,
            "revision":runtime.backend.world.light_revision,"render_dirty":runtime.backend.light.take_render_dirty()});
        if let Some(expected)=&expected {
            let expected:&serde_json::Value=expected;
            for (field,value) in expected.as_object().unwrap() {
                assert!(value==&state[field],"prepared initial-light original625 {field} differs");
            }
        } else {expected=Some(state.clone());}
        let ore=runtime.runtime_timings().ore_lookahead;
        if reference { assert_eq!(ore.prepared_spheres,0); }
        else { assert!(ore.prepared_spheres>1000,"optimized full625 must exercise prepared coordinates"); }
        if reference {assert_eq!(runtime.terrain_timings().prepared_light_installed,0);}
        else {assert!(runtime.terrain_timings().prepared_light_installed>100);}
        assert_eq!(runtime.terrain_timings().prepared_light_fallbacks,0);
        let row=serde_json::json!({"reference_prepared_light":reference,
            "reference_lighting":reference,"reference_precipitation_scan":reference,
            "reference_publication":reference,"spawn_ms":spawn_ms,
            "wall_ms":wall_ms,"publication_ms":publication_ms,"terrain":runtime.terrain_timings(),
            "runtime":runtime.runtime_timings(),"fingerprint":"00971aef0283004f",
            "checkpoint":state["checkpoint"]});
        eprintln!("{row}");reports.push(row);
    }
    std::fs::create_dir_all("reports").unwrap();
    std::fs::write("reports/native-startup-prepared-light-profile.json",serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
}


#[test]
#[ignore="paired full625 ore speculation correctness/readiness/profile; run alone with optimized tests"]
fn original_625_ore_lookahead_forced_ready_unavailable_and_pool_match() {
    use super::super::super::population::ore_lookahead::TestMode;
    let mut expected=None;let mut reports=Vec::new();
    for (label,mode) in [("baseline",Some(TestMode::Disabled)),("forced_ready",Some(TestMode::Ready)),
        ("forced_unavailable",Some(TestMode::Unavailable)),("pool",None),("baseline_repeat",Some(TestMode::Disabled)),("pool_repeat",None)] {
        let mut runtime=Runtime::new(189,1234,true,Arc::new(LightProfileHooks));
        runtime.backend.ore_lookahead_mode=mode;runtime.use_overlay_journal(false);
        runtime.backend.entities.math_state=Some(constructor_state(0,193));
        runtime.backend.entities.inventory_random=Some((constructor_state(0,194),None));
        let begin=Instant::now();runtime.prepare_overworld_spawn().unwrap();
        for start in (0..625).step_by(32) {
            runtime.load_initial_spawn_batch(start,32).unwrap();drop(runtime.take_chunk_updates());
        }
        let elapsed_ms=begin.elapsed().as_secs_f64()*1000.;
        let terrain=fingerprint(&runtime);
        assert_eq!(terrain["blocks_biomes_lights"],"00971aef0283004f");
        assert_eq!(runtime.backend.provider_random.state(),78486281047925);
        assert_eq!(runtime.backend.world_random.state(),210890393092178);
        let state=serde_json::json!({"terrain":terrain,"checkpoint":checkpoint_digest(&runtime),
            "reads":runtime.backend.profile.access_reads,"writes":runtime.backend.profile.access_writes,
            "revision":runtime.backend.world.light_revision,"render_dirty":runtime.backend.light.take_render_dirty()});
        if let Some(expected)=&expected {
            let expected:&serde_json::Value=expected;
            for(field,value)in expected.as_object().unwrap(){assert!(value==&state[field],"ore lookahead {label} field {field}");}
        }else{expected=Some(state.clone());}
        let stats=runtime.runtime_timings().ore_lookahead;
        match mode {
            Some(TestMode::Disabled)=>assert_eq!(stats.submitted,0),
            Some(TestMode::Ready)=>assert!(stats.prepared_spheres>1000,"forced-ready native branch was not exercised"),
            Some(TestMode::Unavailable)=>{assert!(stats.submitted>0&&stats.unavailable>0);assert_eq!(stats.prepared_spheres,0);},
            None=>{}, // Report actual readiness; a saturated pool is allowed to fall back.
        }
        let row=serde_json::json!({"mode":label,"elapsed_ms":elapsed_ms,"runtime":runtime.runtime_timings(),
            "fingerprint":"00971aef0283004f","checkpoint":state["checkpoint"]});
        eprintln!("{row}");reports.push(row);
    }
    std::fs::create_dir_all("reports").unwrap();
    std::fs::write("reports/native-ore-lookahead-profile.json",serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
}
