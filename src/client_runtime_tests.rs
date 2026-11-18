//! Headless profiling of the actual populated Game, without renderer/FPS claims.
use super::*;
use clap::Parser;
use serde_json::{json, Value};
use std::fs;

struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("ferrum-runtime-profile-{}-{}",
            std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&p).unwrap();
        Self(p.canonicalize().unwrap())
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("Runtime profile retained at {}",self.0.display());
            return;
        }
        let actual = self.0.canonicalize().unwrap();
        assert_eq!(actual.parent(),Some(std::env::temp_dir().canonicalize().unwrap().as_path()));
        assert!(actual.file_name().unwrap().to_string_lossy().starts_with("ferrum-runtime-profile-"));
        fs::remove_dir_all(actual).unwrap();
    }
}
fn stats(samples: &[f64]) -> Value {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    json!({"samples":sorted.len(), "p50_ms":sorted[sorted.len()/2],
        "p95_ms":sorted[((sorted.len()-1)*95)/100], "max_ms":sorted.last().unwrap(),
        "sum_ms":sorted.iter().sum::<f64>()})
}

#[test]
#[ignore = "loads the actual 529-chunk populated Game and profiles 120 ordinary simulation ticks"]
fn populated_game_120_ticks_profile_with_streaming_and_background_autosave() {
    let temp = Temporary::new();
    let resources = crate::validation::resources();
    let assets = crate::validation::assets();
    let mut options = Options::parse_from(["ferrum189", "--generator", "4", "--creative",
        "--seed", "8924706420471469774", "--render-distance", "10", "--save-id", "runtime-profile"]);
    options.project_dir = temp.0.clone();
    options.world_rules.allow_cheats = true;
    let settings = Settings { render_distance:10, ..Default::default() };
    let start = Instant::now();
    let mut game = Game::load(options,assets,&settings).unwrap();
    let load_ms = start.elapsed().as_secs_f64()*1000.;
    assert_eq!(game.world.chunks.len(),529);
    assert!(game.population.is_some());
    // Game::load's real CPU meshes were built; rendering/upload is deliberately
    // excluded from this simulation/streaming/lighting measurement.
    game.uploads.clear();
    let initial_mobs = game.sim.mobs.len();
    let [x,_,z] = game.player.precise_position();
    let far = ((x.floor() as i32 >> 4)+24,(z.floor() as i32 >> 4)+24);
    let debug = gameplay::local_commands::execute(&mut game.sim,&mut game.player,&mut game.world,
        resources,&game.options,"/debug start");
    assert!(debug.messages.iter().any(|m| m["translate"]=="commands.debug.start"));
    let first_tick = game.sim.total_ticks;
    let mut tick_samples = Vec::new();
    let mut poll_samples = Vec::new();
    let mut light_samples = Vec::new();
    let mut total_samples = Vec::new();
    let mut rows = Vec::new();
    let mut autosave_ms = 0.;
    let mut request_ms = 0.;
    let measured = Instant::now();
    for tick in 0..120 {
        let total = Instant::now();
        if tick==40 {
            let t = Instant::now();
            game.population.as_mut().unwrap().request_chunks(&mut game.world,vec![far]).unwrap();
            request_ms = t.elapsed().as_secs_f64()*1000.;
            let t = Instant::now();
            assert!(game.autosave().unwrap());
            autosave_ms = t.elapsed().as_secs_f64()*1000.;
        }
        let t = Instant::now();
        let outcome = game.sim.tick(&mut game.world,&mut game.player,resources,&settings,false,false);
        for p in outcome.changed { game.mark(p); }
        let tick_ms = t.elapsed().as_secs_f64()*1000.;
        let t = Instant::now();
        game.dirty.extend(game.population.as_mut().unwrap().poll(&mut game.world,&mut game.sim).unwrap());
        let poll_ms = t.elapsed().as_secs_f64()*1000.;
        let t = Instant::now();
        if let Some(lighting) = &mut game.lighting {
            for p in lighting.update(&mut game.world) { game.dirty.extend(mesh_neighbors(p)); }
        }
        let lighting_ms = t.elapsed().as_secs_f64()*1000.;
        let total_ms = total.elapsed().as_secs_f64()*1000.;
        tick_samples.push(tick_ms); poll_samples.push(poll_ms);
        light_samples.push(lighting_ms); total_samples.push(total_ms);
        rows.push(json!({"tick":tick,"simulation_ms":tick_ms,"host_poll_ms":poll_ms,
            "lighting_ms":lighting_ms,"total_ms":total_ms,
            "chunks":game.world.chunks.len(),"mobs":game.sim.mobs.len()}));
    }
    let total_ms = measured.elapsed().as_secs_f64()*1000.;
    assert_eq!(game.sim.total_ticks-first_tick,120);
    let debug = gameplay::local_commands::execute(&mut game.sim,&mut game.player,&mut game.world,
        resources,&game.options,"/debug stop");
    assert!(debug.messages.iter().any(|m| m["translate"]=="commands.debug.stop"));
    let sections = fs::read_dir(temp.0.join("debug")).unwrap()
        .map(|entry| fs::read_to_string(entry.unwrap().path()).unwrap()).collect::<Vec<_>>();
    let t = Instant::now();
    game.population.as_mut().unwrap().drain(&mut game.world,&mut game.sim).unwrap();
    let final_wait_ms = t.elapsed().as_secs_f64()*1000.;
    assert!(game.world.chunks.contains_key(&far));
    assert!(game.world.save_path.is_file());
    let summary = json!({"seed":"8924706420471469774","render_distance":10,
        "initial_chunks":529,"initial_mobs":initial_mobs,"ticks":120,"load_and_mesh_ms":load_ms,
        "simulation":stats(&tick_samples),"host_poll":stats(&poll_samples),
        "lighting":stats(&light_samples),"whole_tick":stats(&total_samples),
        "stream_request_ms":request_ms,"autosave_enqueue_ms":autosave_ms,
        "120_ticks_total_ms":total_ms,"post_measurement_durable_drain_ms":final_wait_ms,
        "debug_sections":sections,"rows":rows,
        "note":"Headless CPU simulation timings; no frame rendering or FPS measurement. Autosave requested at tick 40 while generation is in flight."});
    eprintln!("Real Game 120-tick summary: {}",json!({"load_ms":load_ms,"total_ms":total_ms,
        "simulation":stats(&tick_samples),"host_poll":stats(&poll_samples),"lighting":stats(&light_samples),
        "autosave_enqueue_ms":autosave_ms,"post_measurement_drain_ms":final_wait_ms}));
    if let Some(path) = std::env::var_os("FERRUM_RUNTIME_PROFILE").map(PathBuf::from) {
        if let Some(parent)=path.parent() { fs::create_dir_all(parent).unwrap(); }
        fs::write(path,serde_json::to_vec_pretty(&summary).unwrap()).unwrap();
    }
}
