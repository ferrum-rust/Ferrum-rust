//! The playable Game path must retain original population across upgrade/save/streaming.
use super::*;
use clap::Parser;
use std::{collections::BTreeMap, fs, path::Path};

struct TempProject(PathBuf);
impl TempProject {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrum-population-game-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
}
impl Drop for TempProject {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("Population game test retained at {}", self.0.display());
            return;
        }
        // Never recursively remove any path except our newly created direct
        // child of the resolved system temp directory.
        let actual = self.0.canonicalize().unwrap();
        let temp = std::env::temp_dir().canonicalize().unwrap();
        assert_eq!(actual.parent(), Some(temp.as_path()));
        assert!(actual
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("ferrum-population-game-"));
        fs::remove_dir_all(actual).unwrap();
    }
}

fn flora(world: &World, watched: &[(i32, i32)]) -> BTreeMap<[i32; 3], u16> {
    let mut result = BTreeMap::new();
    for &(x, z) in watched {
        let chunk = world
            .chunks
            .get(&(x, z))
            .expect("requested chunk must be visible");
        for (i, &state) in chunk.blocks.iter().enumerate() {
            if matches!(state >> 4, 17 | 18 | 31 | 37 | 38 | 78 | 161 | 162 | 175) {
                result.insert(
                    [
                        x * 16 + (i % 16) as i32,
                        (i / 256) as i32,
                        z * 16 + ((i / 16) % 16) as i32,
                    ],
                    state,
                );
            }
        }
    }
    result
}

fn request_chunks(game: &mut Game, positions: &[(i32, i32)]) {
    let host = game
        .population
        .as_mut()
        .expect("normal Game must own population");
    host.request_chunks(&mut game.world, positions.to_vec())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    while host.busy() {
        host.poll(&mut game.world, &mut game.sim).unwrap();
        assert!(
            Instant::now() < deadline,
            "populated streaming request timed out"
        );
        if host.busy() {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn options(project: &Path, generator: &str) -> Options {
    let mut options = Options::parse_from([
        "ferrum189",
        "--generator",
        generator,
        "--creative",
        "--seed",
        "-6201231679296862437",
        "--render-distance",
        "3",
        "--save-id",
        "population-upgrade",
    ]);
    options.project_dir = project.to_path_buf();
    options
}

fn capture_if_requested(game: &mut Game, assets: &Assets, settings: &Settings) {
    let Some(path) = std::env::var_os("FERRUM_POPULATED_CAPTURE").map(PathBuf::from) else {
        return;
    };
    // Request the surrounding chunks through the playable owner, including
    // the distant trees/mountain visible from the user's saved viewpoint.
    let mut settings = settings.clone();
    settings.render_distance = 8;
    let positions = (-8..=8)
        .flat_map(|x| (-8..=8).map(move |z| (x, z)))
        .collect::<Vec<_>>();
    request_chunks(game, &positions);
    let resources = crate::validation::resources();
    let mut renderer =
        pollster::block_on(Renderer::new(None, assets, resources, &settings, None)).unwrap();
    renderer.resize(1280, 720);
    for (&position, _) in &game.world.chunks {
        renderer.upload(
            position,
            mesh::build_with_settings(&game.world, position, assets, &settings),
        );
    }
    let mut view = Player::new(Vec3::new(0.5, 76., 0.5));
    view.set_pose_f64([0.5, 76., 0.5], -1.7, 6.9, false);
    let mut shown = game.sim.clone();
    shown.clock = 6000;
    let gui = Gui {
        screen: Screen::Playing,
        hide_hud: true,
        ..Default::default()
    };
    for _ in 0..40 {
        renderer.tick_lighting(&game.world, &view, &shown, 3);
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).unwrap();
    }
    renderer
        .screenshot(
            &path,
            &RenderInput {
                selected_block: None,
                assets,
                resources,
                settings: &settings,
                gui: &gui,
                ui: &ui::Frame::default(),
                world: Some(&game.world),
                player: Some(&view),
                simulation: Some(&shown),
                alpha: 1.,
                time: 0.,
            },
        )
        .unwrap();
    eprintln!("Playable populated capture: {}", path.display());
}

#[test]
#[ignore = "loads installed assets and the original 625-chunk spawn population"]
fn playable_population_migrates_streams_and_saves_without_changing_the_old_overlay() {
    let project = TempProject::new();
    let old_path = project.0.join("saves/world-population-upgrade.json");
    let legacy_edit = IVec3::new(5, 220, 5);
    let new_edit = IVec3::new(6, 220, 5);
    let initial_position = [0.5, 76., 0.5];
    let mut old_sim = Simulation::new(Mode::Creative, -6201231679296862437i64 as u64);
    old_sim.mob_spawning = false;
    old_sim.inventory.selected = 4;
    old_sim.inventory.slots[4] = Some(Stack::new(50, 13));
    crate::world::atomic_json(
        &old_path,
        &serde_json::json!({
            "version": 2,
            "source": "native189-base1--6201231679296862437",
            "edits": [[legacy_edit.to_array(), 41 << 4]],
            "player": initial_position,
            "simulation": old_sim,
        }),
    )
    .unwrap();
    let old_bytes = fs::read(&old_path).unwrap();
    let assets = Assets::load(&crate::minecraft_dir().join("versions/1.8.9/1.8.9.jar")).unwrap();
    let settings = Settings {
        render_distance: 3,
        ..Default::default()
    };
    let mut game = Game::load(options(&project.0, "3"), &assets, &settings).unwrap();
    assert_eq!(game.options.generator, 4);
    assert!(game.population.is_some());
    assert_eq!(game.player.precise_position(), initial_position);
    assert_eq!(game.sim.inventory.selected, 4);
    assert_eq!(game.sim.inventory.held(), Some(Stack::new(50, 13)));
    assert_eq!(game.world.get(legacy_edit), 41 << 4);
    assert_eq!(fs::read(&old_path).unwrap(), old_bytes);
    capture_if_requested(&mut game, &assets, &settings);

    let mut watched = (-3..=3)
        .flat_map(|x| (-3..=3).map(move |z| (x, z)))
        .collect::<Vec<_>>();
    let trees = (-3..=1)
        .flat_map(|x| (5..=9).map(move |z| (x, z)))
        .collect::<Vec<_>>();
    request_chunks(&mut game, &trees);
    watched.extend(trees.iter().copied());
    let generated_flora = flora(&game.world, &watched);
    let base = crate::vanilla::generator::Generator::new(-6201231679296862437);
    for (name, ids) in [
        ("trees", &[17, 162][..]),
        ("leaves", &[18, 161][..]),
        ("grass", &[31][..]),
        ("flowers", &[37, 38][..]),
        ("snow", &[78][..]),
    ] {
        let (&[x, y, z], &state) = generated_flora
            .iter()
            .find(|(_, state)| ids.contains(&(**state >> 4)))
            .unwrap_or_else(|| panic!("playable reported seed is missing {name}"));
        let chunk = base.chunk((x.div_euclid(16), z.div_euclid(16))).unwrap();
        assert_ne!(
            chunk.get(x.rem_euclid(16), y, z.rem_euclid(16)),
            state,
            "{name} must come from original population, not the bare terrain provider"
        );
    }

    // A single request beyond the initial 625 chunks must also finish original
    // population. This catches adapters that return a bare first-load chunk
    // until its neighbors happen to be visited later.
    let far = (
        (game.world.spawn.x.floor() as i32).div_euclid(16) + 40,
        (game.world.spawn.z.floor() as i32).div_euclid(16) + 40,
    );
    request_chunks(&mut game, &[far]);
    let far_blocks = game.world.chunks[&far].blocks.clone();
    assert_ne!(
        far_blocks,
        base.chunk(far).unwrap().blocks,
        "an isolated far request must include original population"
    );
    assert!(
        far_blocks
            .iter()
            .any(|state| matches!(state >> 4, 14..=16 | 21 | 56 | 73 | 74 | 129)),
        "the fully populated far chunk must contain original ores"
    );
    game.world.unload_chunk(far);
    request_chunks(&mut game, &[far]);
    assert_eq!(game.world.chunks[&far].blocks, far_blocks);

    // Eviction followed by the normal streaming request must republish the
    // same populated tree chunk, not regenerate a bare chunk at this position.
    let (&[tx, _, tz], _) = generated_flora
        .iter()
        .find(|(_, state)| matches!(**state >> 4, 17 | 162))
        .unwrap();
    let tree_chunk = (tx.div_euclid(16), tz.div_euclid(16));
    let tree_blocks = game.world.chunks[&tree_chunk].blocks.clone();
    game.world.unload_chunk(tree_chunk);
    request_chunks(&mut game, &[tree_chunk]);
    assert_eq!(game.world.chunks[&tree_chunk].blocks, tree_blocks);
    assert_eq!(flora(&game.world, &watched), generated_flora);

    assert!(game.world.set(new_edit, 57 << 4));
    let saved_position = [2.75, 80.125, 3.25];
    game.player
        .set_pose_f64(saved_position, 133.25, -27.5, false);
    game.player.allow_flying = true;
    game.player.flying = true;
    game.sim.inventory.selected = 6;
    game.sim.inventory.slots[6] = Some(Stack::new(264, 7));
    let inventory = serde_json::to_value(&game.sim.inventory).unwrap();
    game.save().unwrap();
    let checkpoint = game.world.save_path.clone();
    assert!(
        checkpoint.is_file(),
        "normal Game save must write its native checkpoint"
    );
    assert_ne!(checkpoint, old_path);
    assert_eq!(fs::read(&old_path).unwrap(), old_bytes);
    drop(game);

    let mut resumed = Game::load(options(&project.0, "4"), &assets, &settings).unwrap();
    assert_eq!(resumed.player.precise_position(), saved_position);
    assert_eq!(resumed.player.java_rotation(), [133.25, -27.5]);
    assert!(resumed.player.flying);
    assert_eq!(
        serde_json::to_value(&resumed.sim.inventory).unwrap(),
        inventory
    );
    assert_eq!(resumed.world.get(legacy_edit), 41 << 4);
    assert_eq!(resumed.world.get(new_edit), 57 << 4);
    request_chunks(&mut resumed, &trees);
    request_chunks(&mut resumed, &[far]);
    assert_eq!(resumed.world.chunks[&far].blocks, far_blocks);
    assert_eq!(flora(&resumed.world, &watched), generated_flora);
    assert_eq!(resumed.world.chunks[&tree_chunk].blocks, tree_blocks);
    resumed.save().unwrap();
    assert_eq!(fs::read(&old_path).unwrap(), old_bytes);
    drop(resumed);
}
