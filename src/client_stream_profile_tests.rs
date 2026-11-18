//! Replays identical native chunk snapshots through the old and new visual
//! pipelines. CPU frame work and asynchronous completion latency are separate.
use super::*;
use crate::lighting_view::LightingView;
use clap::Parser;
use serde_json::{json, Value};
use std::{fs, path::Path};

struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("ferrum-stream-profile-{}-{}",
            std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("Streaming profile retained at {}", self.0.display());
            return;
        }
        let path = self.0.canonicalize().unwrap();
        let temp = std::env::temp_dir().canonicalize().unwrap();
        assert_eq!(path.parent(), Some(temp.as_path()));
        assert!(path.file_name().unwrap().to_string_lossy().starts_with("ferrum-stream-profile-"));
        fs::remove_dir_all(path).unwrap();
    }
}

fn milliseconds(start: Instant) -> f64 { start.elapsed().as_secs_f64() * 1000. }
fn stats(samples: &[f64]) -> Value {
    assert!(!samples.is_empty());
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    json!({"samples":sorted.len(), "p50_ms":sorted[(sorted.len()-1)/2],
        "p95_ms":sorted[(sorted.len()-1)*95/100], "max_ms":sorted.last().unwrap(),
        "sum_ms":sorted.iter().sum::<f64>()})
}

/// Construct a visual-only replay from the same immutable native input. No
/// session, generator, save or simulation tick runs during the comparison.
fn replay(loaded: &Game, initial: &HashSet<ChunkPos>) -> Game {
    let mut world = loaded.world.clone();
    world.chunks.retain(|p, _| initial.contains(p));
    world.invalidate_lighting();
    let lighting = Some(LightingView::new(&mut world));
    let visual_revision = world.light_revision;
    let mut sim = loaded.sim.clone();
    sim.clock = 6000;
    sim.mobs.clear();
    Game {
        options: loaded.options.clone(), player: Player::new(world.spawn), world, sim, lighting,
        dirty: HashSet::new(), rebuild: None, uploads: VecDeque::new(), prepared: VecDeque::new(),
        entry_pending: HashSet::new(), entry_uploaded: Default::default(), visual_revision, chunk_timings: Default::default(), presentation_copy_counts: Default::default(), stream: None, stream_center: None,
        spare_presentation_brains: Default::default(),
        imports_pending: false, attempted: HashSet::new(), remote: None, population: None, distant: None,
    }
}

#[derive(Default)]
struct Legacy {
    rebuild: Option<mpsc::Receiver<MeshBatch>>,
    uploads: VecDeque<(ChunkPos, Mesh)>,
}
impl Legacy {
    fn update(&mut self, game: &mut Game, renderer: &mut Renderer, assets: &Arc<Assets>, settings: &Settings) {
        // Before the visual worker change, lighting reconciliation and raw
        // mesh partitioning both ran in the frame loop. Collection waited for
        // every dirty chunk, using the global Rayon pool inside one worker.
        if let Some(lighting) = &mut game.lighting {
            for p in lighting.update(&mut game.world) { game.dirty.extend(mesh_neighbors(p)); }
        }
        if let Some(rx) = &self.rebuild {
            match rx.try_recv() {
                Ok(meshes) => { self.uploads.extend(meshes); self.rebuild = None; }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => panic!("Legacy visual worker stopped"),
            }
        }
        for _ in 0..settings.chunk_uploads.max(1) {
            let Some((p, mesh)) = self.uploads.pop_front() else { break; };
            renderer.upload(p, mesh);
        }
        if self.rebuild.is_none() && !game.dirty.is_empty() {
            let world = game.world.clone();
            let positions = std::mem::take(&mut game.dirty).into_iter()
                .filter(|p| world.chunks.contains_key(p)).collect::<Vec<_>>();
            let assets = assets.clone();
            let settings = settings.clone();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let meshes = positions.par_iter().map(|&p|
                    (p, mesh::build_with_settings(&world, p, &assets, &settings))).collect();
                let _ = tx.send(meshes);
            });
            self.rebuild = Some(rx);
        }
    }
    fn done(&self, game: &Game) -> bool {
        self.rebuild.is_none() && self.uploads.is_empty() && game.dirty.is_empty()
    }
}

fn done(game: &Game) -> bool {
    game.rebuild.is_none() && game.uploads.is_empty() && game.prepared.is_empty()
        && game.dirty.is_empty() && game.lighting.is_some()
        && game.visual_revision == game.world.light_revision
}

fn screenshot(renderer: &mut Renderer, game: Option<&Game>, assets: &Assets,
    settings: &Settings, path: &Path) {
    let gui = Gui { screen: Screen::Playing, hide_hud: true, ..Default::default() };
    renderer.screenshot(path, &RenderInput {
        selected_block: None, assets, resources: crate::validation::resources(), settings,
        gui: &gui, ui: &ui::Frame::default(), world: game.map(|g| &g.world),
        player: game.map(|g| &g.player), simulation: game.map(|g| &g.sim), alpha: 1., time: 0.,
    }).unwrap();
}

struct Measurements {
    report: Value,
    world: World,
    frame: image::RgbaImage,
}

fn measure(loaded: &Game, assets: &Arc<Assets>, settings: &Settings, initial: &HashSet<ChunkPos>,
    batches: &[Vec<(ChunkPos, Arc<Chunk>)>], center: ChunkPos, modern: bool, temp: &Path) -> Measurements {
    let mut game = replay(loaded, initial);
    let mut renderer = pollster::block_on(Renderer::new(None, assets,
        crate::validation::resources(), settings, None)).unwrap();
    renderer.resize(16, 16);
    // Both runs start with the same visible chunks. Warmup is outside the
    // streaming measurement, including their GPU allocation and submission.
    for (p, mesh) in mesh_world(&game.world, assets, settings) { renderer.upload(p, mesh); }
    let path = temp.join(if modern { "new.png" } else { "legacy.png" });
    screenshot(&mut renderer, None, assets, settings, &path);
    let glow = IVec3::new(center.0*16+8, 230, center.1*16+8);
    assert_eq!(game.world.get(glow), 0);
    let mut legacy = Legacy::default();
    let mut frames = Vec::new();
    let mut non_idle = Vec::new();
    let mut latencies = Vec::new();
    let mut gpu_drains = Vec::new();
    let mut rows = Vec::new();
    let measured = Instant::now();
    let mut batch_index = 0;
    // Half the batches, place a local source, the rest, remove it. This exercises
    // local light edits without changing any native incoming chunk snapshot.
    for phase in 0..batches.len()+2 {
        let is_place = phase == batches.len()/2;
        let is_remove = phase == batches.len()+1;
        let job = Instant::now();
        let first = Instant::now();
        let label;
        if is_place || is_remove {
            label = if is_place { "place_glowstone" } else { "remove_glowstone" };
            game.world.set(glow, if is_place { 89<<4 } else { 0 });
            game.mark(glow);
        } else {
            label = "integrate_eight";
            for p in game.world.integrate(batches[batch_index].clone()) {
                game.dirty.extend(mesh_neighbors(p));
            }
            batch_index += 1;
        }
        let mut first_frame = Some(first);
        let mut phase_frames = Vec::new();
        loop {
            let frame = first_frame.take().unwrap_or_else(Instant::now);
            if modern {
                game.update_visuals(&mut renderer, assets, settings, center).unwrap();
            } else {
                legacy.update(&mut game, &mut renderer, assets, settings);
            }
            let elapsed = milliseconds(frame);
            frames.push(elapsed);
            phase_frames.push(elapsed);
            // Poll-only frames are still included above; retain a second
            // distribution so sleeping does not bury the actual work spikes.
            if elapsed >= 0.05 { non_idle.push(elapsed); }
            if if modern { done(&game) } else { legacy.done(&game) } { break; }
            assert!(job.elapsed() < Duration::from_secs(60), "visual phase timed out: {label}");
            std::thread::sleep(Duration::from_millis(4));
        }
        let latency = milliseconds(job);
        latencies.push(latency);
        // Headless write_buffer calls need a submission. Drain each batch to
        // prevent staging allocations accumulating for the whole replay.
        // Tiny readback/PNG encoding is NOT included in active frame samples.
        let drain = Instant::now();
        screenshot(&mut renderer, None, assets, settings, &path);
        let drain_ms = milliseconds(drain);
        gpu_drains.push(drain_ms);
        rows.push(json!({"phase":phase,"kind":label,"chunks":game.world.chunks.len(),
            "active_frame_cpu":stats(&phase_frames),"async_completion_latency_ms":latency,
            "separate_gpu_submit_readback_ms":drain_ms}));
        if is_place {
            assert_eq!(game.world.light_uv(glow+IVec3::X, 0)[0]/16, 14,
                "local source must propagate before the phase is complete");
        }
        if is_remove {
            assert_eq!(game.world.light_uv(glow+IVec3::X, 0)[0], 0,
                "removed source must not retain stale worker light");
        }
    }
    let total = milliseconds(measured);
    assert_eq!(batch_index, batches.len());
    assert_eq!(game.world.chunks.len(), loaded.world.chunks.len());
    assert_eq!(game.world.get(glow), 0);
    renderer.resize(640, 360);
    // Chunk.top is an allocation upper bound, and deliberately does not
    // decrease when the high glowstone probe is removed. Scan the untouched
    // input columns instead so the camera stays near the actual surface.
    let camera_height = (-1..=1).flat_map(|dx| (-1..=1).map(move |dz| (dx,dz)))
        .map(|(dx,dz)| (0..crate::world::HEIGHT).rev().find(|&y|
            loaded.world.get(IVec3::new(center.0*16+8+dx,y,center.1*16+8+dz)) != 0)
            .unwrap_or(63)+1).max().unwrap()+8;
    game.player.set_pose_f64([f64::from(center.0*16)+8.5, f64::from(camera_height),
        f64::from(center.1*16)+8.5], 35., 45., false);
    for _ in 0..40 { renderer.tick_lighting(&game.world, &game.player, &game.sim, settings.render_distance); }
    screenshot(&mut renderer, Some(&game), assets, settings, &path);
    assert!(renderer.visible_chunks > 0 && renderer.triangles > 100,
        "final comparison must actually render terrain");
    let frame = image::open(&path).unwrap().to_rgba8();
    // Draw-count assertions alone can pass while every submitted triangle is
    // behind opaque distance fog. Compare against the exact same camera/sky/
    // lightmap/hand after removing only uploaded terrain meshes.
    for &p in game.world.chunks.keys() { renderer.remove(p); }
    let empty_path = path.with_file_name(if modern { "new-empty.png" } else { "legacy-empty.png" });
    screenshot(&mut renderer, Some(&game), assets, settings, &empty_path);
    let empty = image::open(empty_path).unwrap().to_rgba8();
    let terrain_pixels = frame.pixels().zip(empty.pixels()).filter(|(a,b)|
        (0..3).any(|c| a[c].abs_diff(b[c]) > 8)).count();
    assert!(terrain_pixels > (frame.width()*frame.height()/10) as usize,
        "terrain must visibly affect more than 10% of the image, got {terrain_pixels} at y={camera_height}");
    let report = json!({"pipeline":if modern {"bounded_async_visuals"} else {"legacy_frame_lighting_raw_upload"},
        "active_frame_cpu":stats(&frames),
        "non_idle_frame_cpu_over_0_05ms":if non_idle.is_empty() { Value::Null } else { stats(&non_idle) },
        "phase_async_completion_latency":stats(&latencies),
        "separate_gpu_submit_readback":stats(&gpu_drains), "replay_wall_ms":total,
        "adapter":renderer.adapter_name, "phases":rows,
        "final_camera":game.player.precise_position(),"visible_terrain_pixels":terrain_pixels});
    Measurements { report, world: game.world.clone(), frame }
}

fn equal_world(actual: &World, expected: &World) {
    assert_eq!(actual.chunks.len(), expected.chunks.len());
    assert_eq!(actual.edits, expected.edits);
    for (p, a) in &actual.chunks {
        let b = &expected.chunks[p];
        assert_eq!(a.blocks, b.blocks, "different blocks at {p:?}");
        assert_eq!(a.biomes, b.biomes, "different biome at {p:?}");
        let a = a.light.as_ref().expect("published native light");
        let b = b.light.as_ref().expect("published native light");
        assert_eq!(a.values, b.values, "different sky/block light at {p:?}");
        assert_eq!(a.height, b.height, "different height at {p:?}");
        assert_eq!(a.sections, b.sections, "different light sections at {p:?}");
        assert_eq!(a.no_sky, b.no_sky);
    }
}

#[test]
#[ignore = "small Vulkan replay comparing visible terrain after streaming and a light source edit"]
fn small_streaming_replay_keeps_visible_terrain_and_final_lighting_identical() {
    let temp = Temporary::new();
    let assets = Arc::new(Assets::load(&crate::minecraft_dir().join("versions/1.8.9/1.8.9.jar")).unwrap());
    let settings = Settings { render_distance: 2, clouds: 0, ..Default::default() };
    let mut options = Options::parse_from(["ferrum189", "--generator", "1", "--creative",
        "--seed", "8924706420471469774", "--render-distance", "2", "--save-id", "small-stream-replay"]);
    options.project_dir = temp.0.clone();
    let mut loaded = Game::load(options, &assets, &settings).unwrap();
    assert_eq!(loaded.world.chunks.len(), 25);
    assert!(loaded.population.is_none());
    loaded.uploads.clear();
    let [x, _, z] = loaded.player.precise_position();
    let center = ((x.floor() as i32)>>4, (z.floor() as i32)>>4);
    let initial = loaded.world.chunks.keys().copied()
        .filter(|p| (p.0-center.0).abs()<=1 && (p.1-center.1).abs()<=1).collect::<HashSet<_>>();
    assert_eq!(initial.len(), 9);
    let mut incoming = loaded.world.chunks.iter().filter(|(p, _)| !initial.contains(p))
        .map(|(&p,c)| (p,c.clone())).collect::<Vec<_>>();
    incoming.sort_unstable_by_key(|(p,_)| *p);
    assert_eq!(incoming.len(), 16);
    let batches = incoming.chunks(8).map(|chunk| chunk.to_vec()).collect::<Vec<_>>();
    let old = measure(&loaded, &assets, &settings, &initial, &batches, center, false, &temp.0);
    let new = measure(&loaded, &assets, &settings, &initial, &batches, center, true, &temp.0);
    equal_world(&new.world, &old.world);
    for (p,chunk) in &loaded.world.chunks {
        assert_eq!(new.world.chunks[p].blocks, chunk.blocks);
    }
    let different_pixels = old.frame.pixels().zip(new.frame.pixels()).filter(|(a,b)| a!=b).count();
    eprintln!("Small streaming Vulkan replay: terrain pixels legacy={}, new={}; differing pixels={different_pixels}",
        old.report["visible_terrain_pixels"],new.report["visible_terrain_pixels"]);
    if let Some(path) = std::env::var_os("FERRUM_STREAM_CAPTURE").map(PathBuf::from) {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) { fs::create_dir_all(parent).unwrap(); }
        let mut sheet = image::RgbaImage::new(1280, 360);
        image::imageops::replace(&mut sheet, &old.frame, 0, 0);
        image::imageops::replace(&mut sheet, &new.frame, 640, 0);
        sheet.save(path).unwrap();
    }
    assert_eq!(different_pixels, 0, "legacy/new visible terrain must match exactly");
}

#[test]
#[ignore = "optimized real 529-chunk streaming replay with Vulkan uploads; run alone for meaningful timings"]
fn populated_streaming_compares_legacy_and_bounded_visual_frame_work() {
    let temp = Temporary::new();
    let all = Instant::now();
    let assets = Arc::new(Assets::load(&crate::minecraft_dir().join("versions/1.8.9/1.8.9.jar")).unwrap());
    let settings = Settings { render_distance: 10, clouds: 0, ..Default::default() };
    let mut options = Options::parse_from(["ferrum189", "--generator", "4", "--creative",
        "--seed", "8924706420471469774", "--render-distance", "10", "--save-id", "stream-profile"]);
    options.project_dir = temp.0.clone();
    let load = Instant::now();
    let mut loaded = Game::load(options, &assets, &settings).unwrap();
    let load_ms = milliseconds(load);
    assert_eq!(loaded.world.chunks.len(), 529);
    assert!(loaded.population.is_some());
    // The full native terrain is generated exactly once. Its owner and unused
    // initial full-view CPU meshes are no longer needed by either replay.
    loaded.population.take();
    loaded.uploads.clear();
    let [x, _, z] = loaded.player.precise_position();
    let center = ((x.floor() as i32)>>4, (z.floor() as i32)>>4);
    let initial = loaded.world.chunks.keys().copied()
        .filter(|p| (p.0-center.0).abs()<=4 && (p.1-center.1).abs()<=4).collect::<HashSet<_>>();
    assert_eq!(initial.len(), 81);
    let mut incoming = loaded.world.chunks.iter().filter(|(p, _)| !initial.contains(p))
        .map(|(&p, c)| (p, c.clone())).collect::<Vec<_>>();
    incoming.sort_unstable_by_key(|(p, _)| ((p.0-center.0).pow(2)+(p.1-center.1).pow(2), *p));
    assert_eq!(incoming.len(), 448);
    let batches = incoming.chunks(8).map(|chunk| chunk.to_vec()).collect::<Vec<_>>();
    let old = measure(&loaded, &assets, &settings, &initial, &batches, center, false, &temp.0);
    let new = measure(&loaded, &assets, &settings, &initial, &batches, center, true, &temp.0);
    equal_world(&new.world, &old.world);
    for (p, chunk) in &loaded.world.chunks {
        assert_eq!(new.world.chunks[p].blocks, chunk.blocks, "replay changed native terrain at {p:?}");
    }
    let different_pixels = old.frame.pixels().zip(new.frame.pixels()).filter(|(a,b)| a!=b).count();
    let report = json!({"seed":"8924706420471469774", "render_distance":10,
        "initial_chunks":81,"streamed_chunks":448,"batch_size":8,"local_edits":2,
        "same_binary":true,"debug_assertions":cfg!(debug_assertions),
        "cold_game_load_and_initial_mesh_ms":load_ms,"total_test_ms":milliseconds(all),
        "legacy":old.report,"new":new.report,"final_block_light_snapshots_equal":true,
        "final_render_different_pixels":different_pixels,
        "note":"CPU visual-update work including real Vulkan buffer uploads; no FPS claim. The legacy control uses the same current lighting/mesher but restores their former thread placement and whole-dirty batch scheduling. Polling sleeps 4 ms. Cold loading, warmup and per-phase GPU submit/readback are excluded from active-frame CPU samples. Legacy runs first, so this is a single diagnostic replay, not a randomized benchmark."});
    eprintln!("Streaming profile summary: {}", json!({"load_ms":load_ms,
        "legacy":report["legacy"]["active_frame_cpu"],"new":report["new"]["active_frame_cpu"],
        "legacy_latency":report["legacy"]["phase_async_completion_latency"],
        "new_latency":report["new"]["phase_async_completion_latency"],"different_pixels":different_pixels}));
    if let Some(path) = std::env::var_os("FERRUM_STREAM_PROFILE").map(PathBuf::from) {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) { fs::create_dir_all(parent).unwrap(); }
        fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        let mut sheet = image::RgbaImage::new(1280, 360);
        image::imageops::replace(&mut sheet, &old.frame, 0, 0);
        image::imageops::replace(&mut sheet, &new.frame, 640, 0);
        sheet.save(path.with_extension("png")).unwrap();
    }
    assert_eq!(different_pixels, 0, "final legacy/new GPU terrain must match");
}
