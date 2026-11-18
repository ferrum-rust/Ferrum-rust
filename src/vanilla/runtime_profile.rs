//! Exclusive owner-phase measurements. Nested population switches the active
//! phase, so a recursive neighbor load is not charged to both callers.
use super::*;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub(super) enum Phase {
    Terrain, Discovery, AddLight, InitializeLight, Population, Structures,
    LakesDungeons, DecorationPrefix, DecorationBody, Springs, DecorationSuffix,
    Animals, IceSnow, Ores,
}
const NAMES: [&str; 14] = ["terrain", "structure_discovery", "light_add", "light_initialize",
    "population_other", "structure_population", "lakes_dungeons", "decoration_prefix",
    "decoration_body", "springs", "decoration_suffix", "animals", "ice_snow", "ores"];
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct PhaseTiming {
    pub phase: &'static str,
    pub calls: u64,
    pub exclusive_ms: f64,
}
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct RuntimeTimings {
    pub publication: PublicationTimings,
    pub phases: [PhaseTiming; 14],
    pub access_reads: u64,
    pub access_writes: u64,
    pub ore_lookahead: OreLookaheadStats,
}
impl Default for RuntimeTimings {
    fn default() -> Self { Profile::default().snapshot() }
}
pub(super) struct Profile {
    pub publication: PublicationTimings,
    elapsed: [Duration; 14],
    calls: [u64; 14],
    stack: Vec<Phase>,
    since: Instant,
    pub access_reads: u64,
    pub access_writes: u64,
    pub ore_lookahead: OreLookaheadStats,
}
impl Default for Profile {
    fn default() -> Self { Self { publication: PublicationTimings::default(), elapsed: [Duration::ZERO; 14], calls: [0; 14],
        stack: Vec::new(), since: Instant::now(), access_reads: 0, access_writes: 0, ore_lookahead: OreLookaheadStats::default() } }
}
impl Profile {
    fn charge(&mut self, now: Instant) {
        if let Some(&phase) = self.stack.last() { self.elapsed[phase as usize] += now - self.since; }
        self.since = now;
    }
    pub fn enter(&mut self, phase: Phase) {
        self.charge(Instant::now());
        self.calls[phase as usize] += 1;
        self.stack.push(phase);
    }
    pub fn leave(&mut self) {
        self.charge(Instant::now());
        self.stack.pop().expect("Unbalanced native generation phase");
    }
    pub fn snapshot(&self) -> RuntimeTimings {
        RuntimeTimings { publication: self.publication, phases: std::array::from_fn(|i| PhaseTiming {
            phase: NAMES[i], calls: self.calls[i], exclusive_ms: self.elapsed[i].as_secs_f64() * 1000.,
        }), access_reads: self.access_reads, access_writes: self.access_writes, ore_lookahead: self.ore_lookahead }
    }
}
impl Runtime {
    pub fn runtime_timings(&self) -> RuntimeTimings { self.backend.profile.snapshot() }
}
impl RuntimeBackend {
    pub(super) fn measured<R>(&mut self, phase: Phase, action: impl FnOnce(&mut Self) -> R) -> R {
        self.profile.enter(phase);
        let result = action(self);
        self.profile.leave();
        result
    }
}
impl Access<'_> {
    pub(super) fn measured<R>(&mut self, phase: Phase, action: impl FnOnce(&mut Self) -> R) -> R {
        self.with(|_, b| b.profile.enter(phase));
        let result = action(self);
        self.with(|_, b| b.profile.leave());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generation_phase_accounting_excludes_recursive_children_and_idle_time() {
        let mut profile = Profile::default();
        let initial = Instant::now();
        profile.since = initial;
        profile.stack.push(Phase::Population);
        profile.charge(initial + Duration::from_millis(10));
        profile.stack.push(Phase::InitializeLight);
        profile.charge(initial + Duration::from_millis(40));
        profile.stack.pop();
        profile.charge(initial + Duration::from_millis(60));
        profile.stack.pop();
        profile.charge(initial + Duration::from_millis(100));
        let timings = profile.snapshot();
        assert_eq!(timings.phases[Phase::Population as usize].exclusive_ms, 30.);
        assert_eq!(timings.phases[Phase::InitializeLight as usize].exclusive_ms, 30.);
        assert_eq!(timings.phases.iter().map(|p| p.exclusive_ms).sum::<f64>(), 60.);
    }

    #[test]
    fn ore_phase_is_appended_and_excludes_recursive_generation_from_decoration() {
        assert_eq!(Phase::IceSnow as usize, 12);
        assert_eq!(Phase::Ores as usize, 13);
        let mut profile = Profile::default();
        let initial = Instant::now();
        profile.since = initial;
        profile.stack.push(Phase::DecorationBody);
        profile.charge(initial + Duration::from_millis(10));
        profile.stack.push(Phase::Ores);
        profile.charge(initial + Duration::from_millis(30));
        profile.stack.push(Phase::Terrain);
        profile.charge(initial + Duration::from_millis(37));
        profile.stack.pop();
        profile.charge(initial + Duration::from_millis(42));
        profile.stack.pop();
        profile.charge(initial + Duration::from_millis(50));
        profile.stack.pop();
        profile.charge(initial + Duration::from_millis(100));
        let timings = profile.snapshot();
        assert_eq!(timings.phases[Phase::DecorationBody as usize].exclusive_ms, 18.);
        assert_eq!(timings.phases[Phase::Ores as usize].exclusive_ms, 25.);
        assert_eq!(timings.phases[Phase::Terrain as usize].exclusive_ms, 7.);
        assert_eq!(timings.phases[Phase::Ores as usize].phase, "ores");
        assert_eq!(timings.phases.iter().map(|p| p.exclusive_ms).sum::<f64>(), 50.);
    }

    #[test]
    fn native_ore_phase_wraps_whole_stage_without_changing_rng_or_outputs() {
        struct Quiet;
        impl Hooks for Quiet {
            fn supports_inert_ore_writes(&self) -> bool { true }
            fn populate(&self, _: &mut Access<'_>, _: ChunkPos, _: &mut JavaRandom) -> Result<()> { Ok(()) }
            fn player_random_state(&self) -> u64 { JavaRandom::new(189).state() }
        }
        let mut base = Runtime::new(189,1234,false,Arc::new(Quiet));
        for pos in [(0,0),(0,1),(1,0),(1,1)] { base.provide(pos).unwrap(); }
        let snapshot = base.checkpoint().unwrap();
        let mut expected = None;
        for timed in [false,true] {
            let mut runtime = Runtime::resume(snapshot.clone(),Arc::new(Quiet)).unwrap();
            let mut random = runtime.backend.provider_random.share();
            runtime.access().measured(Phase::DecorationBody, |access| {
                if timed { access.generate_default_ores(&mut random,IVec3::ZERO); }
                else { super::super::super::population::default_ores(access,&mut random,IVec3::ZERO); }
            });
            assert!(runtime.backend.profile.stack.is_empty());
            let timings = runtime.runtime_timings();
            assert_eq!(timings.phases[Phase::DecorationBody as usize].calls,1);
            assert_eq!(timings.phases[Phase::Ores as usize].calls,u64::from(timed));
            let mut checkpoint = runtime.checkpoint().unwrap();
            checkpoint.dirty_chunks.sort_unstable();
            checkpoint.edits.sort_unstable_by_key(|(p,_)| *p);
            let state = (serde_json::to_vec(&checkpoint).unwrap(),random.state(),
                timings.access_reads,timings.access_writes,runtime.backend.world.light_revision);
            if let Some(expected) = &expected {
                assert!(expected == &state,"ore timing changed checkpoint, shared RNG or logical accesses");
            } else { expected = Some(state); }
        }
    }

    #[test]
    #[ignore = "paired full original spawn profile; run alone with optimized tests"]
    fn original_625_startup_light_profile_preserves_hash_rng_and_order() {
        let mut rows = Vec::new();
        let mut expected = None;
        for reference in [true,false,true,false] {
            let mut runtime = Runtime::for_overworld_verification(189,1234,true);
            runtime.backend.light.use_reference_path(reference);
            let begin = Instant::now();
            runtime.prepare_overworld_spawn().unwrap();
            let spawn_ms = begin.elapsed().as_secs_f64()*1000.;
            let begin = Instant::now();
            let mut publication_ms = 0.;
            for start in (0..625).step_by(32) {
                runtime.load_initial_spawn_batch(start,32).unwrap();
                let before = Instant::now();
                drop(runtime.take_chunk_updates());
                publication_ms += before.elapsed().as_secs_f64()*1000.;
            }
            let wall_ms = begin.elapsed().as_secs_f64()*1000.;
            let fingerprint = fingerprint(&runtime);
            assert_eq!(fingerprint["blocks_biomes_lights"], "00971aef0283004f", "retained pre-change original 625-chunk digest");
            assert_eq!(runtime.backend.provider_random.state(),78486281047925);
            assert_eq!(runtime.backend.world_random.state(),210890393092178);
            if let Some(expected) = &expected { assert_eq!(&fingerprint,expected); }
            else { expected=Some(fingerprint.clone()); }
            let row=serde_json::json!({"reference_lighting":reference,"wall_ms":wall_ms,
                "spawn_ms":spawn_ms,"publication_ms":publication_ms,"terrain":runtime.terrain_timings(),
                "runtime":runtime.runtime_timings(),"fingerprint":fingerprint["blocks_biomes_lights"],
                "loaded_chunks":runtime.provider.loaded_order().len()});
            eprintln!("{row}");
            rows.push(row);
        }
        std::fs::create_dir_all("reports").unwrap();
        std::fs::write("reports/native-startup-light-profile.json",serde_json::to_vec_pretty(
            &serde_json::json!({"seed":189,"world_random_seed":1234,"runs":rows})).unwrap()).unwrap();
    }

    #[test]
    #[ignore = "paired full original spawn access profile; run alone with optimized tests"]
    fn original_625_startup_access_profile_preserves_hash_rng_and_order() {
        let mut rows=Vec::new();
        let mut expected=None;
        for reference in [true,false,true,false] {
            let mut runtime=Runtime::for_overworld_verification(189,1234,true);
            runtime.backend.reference_access=reference;
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
            let fingerprint=fingerprint(&runtime);
            assert_eq!(fingerprint["blocks_biomes_lights"],"00971aef0283004f");
            assert_eq!(runtime.backend.provider_random.state(),78486281047925);
            assert_eq!(runtime.backend.world_random.state(),210890393092178);
            if let Some(expected)=&expected { assert_eq!(&fingerprint,expected); }
            else { expected=Some(fingerprint.clone()); }
            let row=serde_json::json!({"reference_access":reference,"wall_ms":wall_ms,
                "spawn_ms":spawn_ms,"publication_ms":publication_ms,"terrain":runtime.terrain_timings(),
                "runtime":runtime.runtime_timings(),"fingerprint":fingerprint["blocks_biomes_lights"],
                "loaded_chunks":runtime.provider.loaded_order().len()});
            eprintln!("{row}");rows.push(row);
        }
        std::fs::create_dir_all("reports").unwrap();
        std::fs::write("reports/native-startup-access-profile.json",serde_json::to_vec_pretty(
            &serde_json::json!({"seed":189,"world_random_seed":1234,"runs":rows})).unwrap()).unwrap();
    }

    #[test]
    #[ignore = "paired full original spawn write scratch profile; run alone with optimized tests"]
    fn original_625_startup_write_profile_preserves_hash_rng_and_order() {
        let mut rows=Vec::new();
        let mut expected=None;
        for reference in [true,false,true,false] {
            let mut runtime=Runtime::for_overworld_verification(189,1234,true);
            runtime.backend.reference_write_scratch=reference;
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
            let fingerprint=fingerprint(&runtime);
            assert_eq!(fingerprint["blocks_biomes_lights"],"00971aef0283004f");
            assert_eq!(runtime.backend.provider_random.state(),78486281047925);
            assert_eq!(runtime.backend.world_random.state(),210890393092178);
            if let Some(expected)=&expected { assert_eq!(&fingerprint,expected); }
            else { expected=Some(fingerprint.clone()); }
            let row=serde_json::json!({"reference_write_scratch":reference,"wall_ms":wall_ms,
                "spawn_ms":spawn_ms,"publication_ms":publication_ms,"terrain":runtime.terrain_timings(),
                "runtime":runtime.runtime_timings(),"fingerprint":fingerprint["blocks_biomes_lights"],
                "scratch_buffers":runtime.backend.write_scratch.len(),
                "loaded_chunks":runtime.provider.loaded_order().len()});
            eprintln!("{row}");rows.push(row);
        }
        std::fs::create_dir_all("reports").unwrap();
        std::fs::write("reports/native-startup-write-profile.json",serde_json::to_vec_pretty(
            &serde_json::json!({"seed":189,"world_random_seed":1234,"runs":rows})).unwrap()).unwrap();
    }

    #[test]
    #[ignore = "paired full original spawn journal profile; run alone with optimized tests"]
    fn original_625_startup_journal_profile_preserves_hash_rng_and_order() {
        let mut rows=Vec::new();
        let mut expected=None;
        for recording in [true,false,true,false] {
            let mut runtime=Runtime::for_overworld_verification(189,1234,true);
            runtime.use_overlay_journal(recording);
            let begin=Instant::now();
            runtime.prepare_overworld_spawn().unwrap();
            let spawn_ms=begin.elapsed().as_secs_f64()*1000.;
            let begin=Instant::now();
            let mut publication_ms=0.;
            let mut journal_entries=0;
            for start in (0..625).step_by(32) {
                runtime.load_initial_spawn_batch(start,32).unwrap();
                let before=Instant::now();
                drop(runtime.take_chunk_updates());
                // NativeSession clears this trace at each publication. Match
                // that real lifecycle instead of growing an unused benchmark map.
                journal_entries+=runtime.backend.world.edits.len();
                runtime.backend.world.edits.clear();
                publication_ms+=before.elapsed().as_secs_f64()*1000.;
            }
            let wall_ms=begin.elapsed().as_secs_f64()*1000.;
            let fingerprint=fingerprint(&runtime);
            assert_eq!(fingerprint["blocks_biomes_lights"],"00971aef0283004f");
            assert_eq!(runtime.backend.provider_random.state(),78486281047925);
            assert_eq!(runtime.backend.world_random.state(),210890393092178);
            assert_eq!(journal_entries==0,!recording);
            if let Some(expected)=&expected {assert_eq!(&fingerprint,expected);}
            else {expected=Some(fingerprint.clone());}
            let row=serde_json::json!({"record_overlay_edits":recording,"wall_ms":wall_ms,
                "spawn_ms":spawn_ms,"publication_ms":publication_ms,"terrain":runtime.terrain_timings(),
                "runtime":runtime.runtime_timings(),"fingerprint":fingerprint["blocks_biomes_lights"],
                "journal_entries":journal_entries,"loaded_chunks":runtime.provider.loaded_order().len()});
            eprintln!("{row}");rows.push(row);
        }
        std::fs::create_dir_all("reports").unwrap();
        std::fs::write("reports/native-startup-journal-profile.json",serde_json::to_vec_pretty(
            &serde_json::json!({"seed":189,"world_random_seed":1234,"runs":rows})).unwrap()).unwrap();
    }

    #[test]
    #[ignore = "paired full original spawn ore batching profile; run alone with optimized tests"]
    fn original_625_startup_ore_batch_profile_preserves_hash_rng_order_and_access_counts() {
        let mut rows = Vec::new();
        let mut expected = None;
        let mut expected_counts = None;
        for reference in [true, false, true, false] {
            let mut runtime = Runtime::for_overworld_verification(189, 1234, true);
            runtime.use_overlay_journal(false);
            runtime.backend.reference_ore_batch = reference;
            let begin = Instant::now();
            runtime.prepare_overworld_spawn().unwrap();
            let spawn_ms = begin.elapsed().as_secs_f64() * 1000.;
            let begin = Instant::now();
            let mut publication_ms = 0.;
            for start in (0..625).step_by(32) {
                runtime.load_initial_spawn_batch(start, 32).unwrap();
                let before = Instant::now();
                drop(runtime.take_chunk_updates());
                publication_ms += before.elapsed().as_secs_f64() * 1000.;
            }
            let wall_ms = begin.elapsed().as_secs_f64() * 1000.;
            let fingerprint = fingerprint(&runtime);
            assert_eq!(fingerprint["blocks_biomes_lights"], "00971aef0283004f");
            assert_eq!(runtime.backend.provider_random.state(), 78486281047925);
            assert_eq!(runtime.backend.world_random.state(), 210890393092178);
            if let Some(expected) = &expected { assert_eq!(&fingerprint, expected); }
            else { expected = Some(fingerprint.clone()); }
            let counts = (runtime.backend.profile.access_reads, runtime.backend.profile.access_writes);
            if let Some(expected) = expected_counts { assert_eq!(counts, expected, "ore batching must perform the original reads and writes"); }
            else { expected_counts = Some(counts); }
            let row = serde_json::json!({"reference_ore_batch":reference,"wall_ms":wall_ms,
                "spawn_ms":spawn_ms,"publication_ms":publication_ms,"terrain":runtime.terrain_timings(),
                "runtime":runtime.runtime_timings(),"fingerprint":fingerprint["blocks_biomes_lights"],
                "record_overlay_edits":false,"loaded_chunks":runtime.provider.loaded_order().len()});
            eprintln!("{row}"); rows.push(row);
        }
        std::fs::create_dir_all("reports").unwrap();
        std::fs::write("reports/native-startup-ore-batch-profile.json", serde_json::to_vec_pretty(
            &serde_json::json!({"seed":189,"world_random_seed":1234,"runs":rows})).unwrap()).unwrap();
    }

    #[test]
    #[ignore = "paired full original spawn prechecked-write profile; run alone with optimized tests"]
    fn original_625_prechecked_write_profile_preserves_state_rng_order_and_logical_accesses() {
        let mut rows = Vec::new();
        let mut expected = None;
        let mut expected_counts = None;
        for reference in [true, false, true, false] {
            let mut runtime = Runtime::for_overworld_verification(189, 1234, true);
            runtime.use_overlay_journal(false);
            runtime.backend.reference_prechecked_write = reference;
            let begin = Instant::now();
            runtime.prepare_overworld_spawn().unwrap();
            let spawn_ms = begin.elapsed().as_secs_f64() * 1000.;
            let begin = Instant::now();
            let mut publication_ms = 0.;
            for start in (0..625).step_by(32) {
                runtime.load_initial_spawn_batch(start, 32).unwrap();
                let before = Instant::now();
                drop(runtime.take_chunk_updates());
                publication_ms += before.elapsed().as_secs_f64() * 1000.;
            }
            let wall_ms = begin.elapsed().as_secs_f64() * 1000.;
            let fingerprint = fingerprint(&runtime);
            assert_eq!(fingerprint["blocks_biomes_lights"], "00971aef0283004f");
            assert_eq!(runtime.backend.provider_random.state(), 78486281047925);
            assert_eq!(runtime.backend.world_random.state(), 210890393092178);
            if let Some(expected) = &expected { assert_eq!(&fingerprint, expected); }
            else { expected = Some(fingerprint.clone()); }
            let counts = (runtime.backend.profile.access_reads, runtime.backend.profile.access_writes);
            if let Some(expected) = expected_counts { assert_eq!(counts, expected); }
            else { expected_counts = Some(counts); }
            let row = serde_json::json!({"reference_prechecked_write":reference,"wall_ms":wall_ms,
                "spawn_ms":spawn_ms,"publication_ms":publication_ms,"terrain":runtime.terrain_timings(),
                "runtime":runtime.runtime_timings(),"fingerprint":fingerprint["blocks_biomes_lights"],
                "logical_access_reads":counts.0,"logical_access_writes":counts.1,
                "record_overlay_edits":false,"loaded_chunks":runtime.provider.loaded_order().len()});
            eprintln!("{row}"); rows.push(row);
        }
        std::fs::create_dir_all("reports").unwrap();
        std::fs::write("reports/native-startup-prechecked-write-profile.json", serde_json::to_vec_pretty(
            &serde_json::json!({"seed":189,"world_random_seed":1234,
                "note":"Access counts are logical original operations, including reads eliminated by native fast paths.",
                "runs":rows})).unwrap()).unwrap();
    }

    #[test]
    #[ignore = "paired full original spawn inert ore profile; run alone with optimized tests"]
    fn original_625_inert_ore_profile_preserves_state_rng_order_effects_and_accesses() {
        fn position_hash(positions: &[IVec3]) -> u64 {
            let mut hash = 0xcbf29ce484222325u64;
            for p in positions {
                for byte in p.to_array().iter().flat_map(|v| v.to_le_bytes()) {
                    hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
                }
            }
            hash
        }
        let mut rows = Vec::new();
        let mut expected = None;
        for reference in [true, false, true, false] {
            let mut runtime = Runtime::for_overworld_verification(189, 1234, true);
            runtime.use_overlay_journal(false);
            runtime.backend.reference_inert_ore = reference;
            let begin = Instant::now();
            runtime.prepare_overworld_spawn().unwrap();
            let spawn_ms = begin.elapsed().as_secs_f64() * 1000.;
            let begin = Instant::now();
            let mut publication_ms = 0.;
            for start in (0..625).step_by(32) {
                runtime.load_initial_spawn_batch(start, 32).unwrap();
                let before = Instant::now();
                drop(runtime.take_chunk_updates());
                publication_ms += before.elapsed().as_secs_f64() * 1000.;
            }
            let wall_ms = begin.elapsed().as_secs_f64() * 1000.;
            let mut fingerprint = fingerprint(&runtime);
            assert_eq!(fingerprint["blocks_biomes_lights"], "00971aef0283004f");
            assert_eq!(runtime.backend.provider_random.state(), 78486281047925);
            assert_eq!(runtime.backend.world_random.state(), 210890393092178);
            fingerprint["ore_write_effects"] = serde_json::json!({
                "logical_reads":runtime.backend.profile.access_reads,
                "logical_writes":runtime.backend.profile.access_writes,
                "light_revision":runtime.backend.world.light_revision,
                "tile_revision":runtime.backend.tile_revision,
                "output_count":runtime.backend.output.changed.len(),
                "output_hash":position_hash(&runtime.backend.output.changed),
                "effects_count":runtime.backend.block_effects.changed.len(),
                "effects_hash":position_hash(&runtime.backend.block_effects.changed),
                "ticks":runtime.backend.ticks,
            });
            if let Some(expected) = &expected {
                let expected: &serde_json::Value = expected;
                for (field, value) in expected.as_object().unwrap() {
                    assert!(value == &fingerprint[field], "inert ore fingerprint field {field} differs");
                }
            } else { expected = Some(fingerprint.clone()); }
            if reference { assert_eq!(runtime.backend.inert_ore_writes, 0); }
            else { assert!(runtime.backend.inert_ore_writes > 100_000); }
            // Compare the entire scheduler above, but keep diagnostic output
            // bounded: its ordered queues contain tens of thousands of ticks.
            let mut reported_effects = fingerprint["ore_write_effects"].clone();
            let ticks = reported_effects.as_object_mut().unwrap().remove("ticks").unwrap();
            let tick_bytes = serde_json::to_vec(&ticks).unwrap();
            let tick_hash = tick_bytes.iter().fold(0xcbf29ce484222325u64,
                |h, &b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3));
            reported_effects["scheduler_bytes"] = tick_bytes.len().into();
            reported_effects["scheduler_hash"] = format!("{tick_hash:016x}").into();
            let row = serde_json::json!({"reference_inert_ore":reference,"wall_ms":wall_ms,
                "spawn_ms":spawn_ms,"publication_ms":publication_ms,"terrain":runtime.terrain_timings(),
                "runtime":runtime.runtime_timings(),"fingerprint":fingerprint["blocks_biomes_lights"],
                "ore_write_effects":reported_effects,
                "inert_ore_writes":runtime.backend.inert_ore_writes,
                "record_overlay_edits":false,"loaded_chunks":runtime.provider.loaded_order().len()});
            eprintln!("{row}"); rows.push(row);
        }
        std::fs::create_dir_all("reports").unwrap();
        std::fs::write("reports/native-startup-inert-ore-profile.json", serde_json::to_vec_pretty(
            &serde_json::json!({"seed":189,"world_random_seed":1234,"runs":rows})).unwrap()).unwrap();
    }

    fn fingerprint(runtime: &Runtime) -> serde_json::Value {
        let mut hash=0xcbf29ce484222325u64;
        let mut consume=|bytes:&[u8]| { for &byte in bytes { hash=(hash ^ u64::from(byte)).wrapping_mul(0x100000001b3); } };
        for &pos in runtime.provider.loaded_order() {
            consume(&pos.0.to_le_bytes());consume(&pos.1.to_le_bytes());
            let chunk=&runtime.backend.world.chunks[&pos];
            for block in chunk.blocks.iter() { consume(&block.to_le_bytes()); }
            consume(&chunk.biomes);
            let light=&runtime.backend.light.chunks[&pos];
            consume(&light.sky);consume(&light.block);
            for height in &light.height { consume(&height.to_le_bytes()); }
        }
        serde_json::json!({"blocks_biomes_lights":format!("{hash:016x}"),
            "provider":runtime.provider,"structures":runtime.backend.structures,
            "provider_random":runtime.backend.provider_random.state(),"world_random":runtime.backend.world_random.state(),
            "animals":runtime.backend.animals,"containers":runtime.backend.live_containers.iter().collect::<Vec<_>>(),
            "events":runtime.backend.events,"decorators":runtime.backend.decorators,
            "precipitation":runtime.backend.precipitation.iter().collect::<Vec<_>>()})
    }
}
