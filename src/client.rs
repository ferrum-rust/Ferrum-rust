use crate::{
    assets::Assets,
    audio::Audio,
    gameplay::{self, Mode, Simulation},
    inventory::{ClickAction, SlotRef, Stack},
    mesh::{self, Mesh},
    player::Player,
    renderer::{RenderInput, Renderer},
    resources::Resources,
    settings::{Setting, Settings},
    ui::{self, Action, Gui, Screen, Stats, WorldEntry},
    world::{Chunk, ChunkPos, World},
    Options,
};
use anyhow::Result;
use glam::{IVec3, Vec3};
use rayon::prelude::*;
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{mpsc, Arc},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Fullscreen, Window, WindowId},
};
#[path = "client_input.rs"]
mod input;
#[path="client_creative.rs"]
mod creative;
#[cfg(test)]
#[path = "client_ui_tests.rs"]
mod ui_tests;
#[cfg(test)]
#[path = "population_game_tests.rs"]
mod population_game_tests;
#[cfg(test)]
#[path = "client_runtime_tests.rs"]
mod runtime_tests;
#[cfg(test)]
#[path = "client_stream_profile_tests.rs"]
mod stream_profile_tests;
#[path = "client_meshing.rs"]
mod meshing;
#[path = "client_local_tick.rs"]
mod local_tick;
#[path = "client_interval_profile.rs"]
mod interval_profile;
#[path = "client_distant.rs"]
mod distant;
use meshing::{MeshWork, PendingMesh};
type MeshBatch = Vec<(ChunkPos, Mesh)>;
type ChunkBatch = Result<Vec<(ChunkPos, Arc<Chunk>)>>;
fn mesh_neighbors((x, z): ChunkPos) -> [ChunkPos; 9] {
    std::array::from_fn(|i| (x + (i % 3) as i32 - 1, z + (i / 3) as i32 - 1))
}
struct Game {
    options: Options,
    world: World,
    lighting: Option<crate::lighting_view::LightingView>,
    player: Player,
    sim: Simulation,
    // At most one reusable presentation-only map; never saved or authoritative.
    spare_presentation_brains: std::collections::BTreeMap<u64, gameplay::ai::MobBrain>,
    dirty: HashSet<ChunkPos>,
    rebuild: Option<mpsc::Receiver<MeshWork>>,
    uploads: VecDeque<(ChunkPos, crate::renderer::PreparedChunk)>,
    prepared: VecDeque<PendingMesh>,
    entry_pending: HashSet<ChunkPos>,
    entry_uploaded: HashMap<ChunkPos, meshing::MeshStamp>,
    visual_revision: u64,
    chunk_timings: std::collections::BTreeMap<&'static str, [f64; 3]>,
    presentation_copy_counts: std::collections::BTreeMap<&'static str, [u64; 3]>,
    stream: Option<mpsc::Receiver<ChunkBatch>>,
    stream_center: Option<ChunkPos>,
    imports_pending: bool,
    attempted: HashSet<ChunkPos>,
    remote: Option<crate::remote::Remote>,
    population: Option<crate::populated_world::PopulatedWorld>,
    distant: Option<distant::DistantScene>,
}
struct CachedDimensionView {
    dimension: i32,
    chunks: crate::world::ChunkMap,
    lighting: Option<crate::lighting_view::LightingView>,
}
static CACHED_DIMENSIONS: std::sync::LazyLock<
    std::sync::Mutex<HashMap<usize, CachedDimensionView>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));
fn with_cached_dimension_chunks<R>(
    game_key: usize,
    target_dim: i32,
    f: impl FnOnce(Option<&crate::world::ChunkMap>) -> R,
) -> R {
    let guard = CACHED_DIMENSIONS.lock().unwrap_or_else(|e| e.into_inner());
    let chunks = guard
        .get(&game_key)
        .filter(|c| c.dimension == target_dim)
        .map(|c| &c.chunks);
    f(chunks)
}
impl Drop for Game {
    fn drop(&mut self) {
        let key = self as *const Self as usize;
        CACHED_DIMENSIONS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&key);
    }
}
fn mesh_world(world: &World, assets: &Assets, s: &Settings) -> MeshBatch {
    let mut positions = world.chunks.keys().copied().collect::<Vec<_>>();
    positions.sort_unstable();
    positions
        .par_iter()
        .map(|&p| (p, mesh::build_with_settings(world, p, assets, s)))
        .collect()
}
fn initial_mesh_positions(world: &World, center: ChunkPos, progressive: bool) -> Vec<ChunkPos> {
    let mut positions = world.chunks.keys().copied().filter(|&(x, z)| !progressive
        || (x - center.0).abs() <= 1 && (z - center.1).abs() <= 1).collect::<Vec<_>>();
    positions.sort_unstable();
    positions
}
fn entry_meshes_ready(world: &World, center: ChunkPos, pending: &HashSet<ChunkPos>, native: bool) -> bool {
    pending.is_empty() && (!native || mesh_neighbors(center).iter().all(|p|
        world.chunks.get(p).is_some_and(|chunk| chunk.light.is_some())))
}
#[path = "native_view.rs"]
pub(crate) mod native_view;
impl Game {
    fn selected_block(&self, gui: &Gui, view: &Player, partial: f32) -> Option<IVec3> {
        use crate::vanilla::click_border;
        if gui.hide_hud { return None; }
        let (hit, mode) = if let Some(remote) = &self.remote {
            if !remote.player_camera() { return None; }
            (remote.pick_at(&self.world, view, &self.sim, partial)?.block()?, remote.game_mode)
        } else {
            let mode = self.sim.commands.player_mode.unwrap_or(u8::from(self.sim.mode == Mode::Creative));
            (self.sim.pick_at(&self.world, view, partial)?.block()?, mode)
        };
        if !self.sim.world_border.contains(hit.position, click_border::now()) { return None; }
        let id = self.world.get(hit.position) >> 4;
        let visible = match mode {
            2 => {
                let held = self.sim.inventory.held();
                click_border::can_edit(mode, id, held.as_ref()) || click_border::can_place_on(mode, id, held.as_ref())
            }
            // The spectator outline is limited to blocks whose tile implements IInventory.
            3 => matches!(id, 23 | 54 | 61 | 62 | 117 | 138 | 146 | 154 | 158),
            _ => true,
        };
        visible.then_some(hit.position)
    }
    fn load(options: Options, assets: &Assets, s: &Settings) -> Result<Self> {
        Self::load_with_progress(options, assets, s, &mut |_| {})
    }
    fn load_with_progress(mut options: Options, assets: &Assets, s: &Settings,
        progress: &mut dyn FnMut(u8)) -> Result<Self> {
        // Also covers direct/headless loading without client::run. Native world
        // generation and connection setup overlap the one-time animation work.
        crate::remote_mobs::start_render_model_warmup();
        let mut chunk_timings=std::collections::BTreeMap::new();
        if options.benchmark_capture_uploads {
            for name in crate::renderer::upload_capture::PHASE_NAMES {chunk_timings.insert(name,[0.;3]);}
        }
        if let Some(address) = &options.connect {
            crate::remote_mobs::warm_render_models();
            let remote = crate::remote::Remote::connect(&options, s)?;
            let spawn = Vec3::new(0., 64., 0.);
            let mut world = World {
                chunks: Default::default(),
                spawn,
                label: format!("Server: {address}"),
                save_path: PathBuf::new(),
                edits: Default::default(),
                imported: true,
                light_revision: 0,
            };
            let lighting = Some(crate::lighting_view::LightingView::new(&mut world));
            let mut player = Player::new(spawn);
            player.third_person.slim = crate::ui::offline_uuid(&options.username)
                .chunks_exact(4)
                .fold(0, |v, b| v ^ b[3])
                & 1
                != 0;
            let mut sim = Simulation::default();
            sim.local_name.clone_from(&options.username);
            if !options.benchmark && options.screenshot.is_none() {
                if let Ok(raw) = std::env::var("FERRUM189_MC_PROFILE_ID") {
                    if let Some(uuid) = crate::player_skins::parse_uuid_hex(&raw) {
                        sim.presentation.mojang_uuid = Some(uuid);
                        sim.presentation.local_uuid.get_or_insert(uuid);
                        crate::player_skins::request_mojang_profile(uuid);
                    }
                }
            }
            return Ok(Self {
                options,
                world,
                lighting,
                player,
                sim,
                dirty: Default::default(),
                rebuild: None,
                uploads: Default::default(),
                prepared: Default::default(),
                entry_pending: Default::default(),
                entry_uploaded: Default::default(),
                visual_revision: 0,
                chunk_timings,
                presentation_copy_counts: Default::default(),
                spare_presentation_brains: Default::default(),
                stream: None,
                stream_center: None,
                imports_pending: false,
                attempted: Default::default(),
                remote: Some(remote),
                population: None,
                distant: None,
            });
        }
        // The extension is visual-only. Native chunks, callbacks and random
        // ticks retain the previous full-detail limit rather than expanding
        // the authoritative simulation to 66,049 columns.
        options.render_distance = s.full_detail_distance();
        let distant = crate::distant_terrain::DistantTerrain::new(&options)?.map(distant::DistantScene::new);
        anyhow::ensure!(!options.benchmark_distant_ready || distant.is_some(),
            "Distant-ready benchmark requires a native DEFAULT, LargeBiomes, Amplified or Customized world");
        if s.render_distance > 24 && distant.is_none() {
            eprintln!("Distant LOD is unavailable for this world source; full-detail radius remains 24 chunks.");
        }
        let start = Instant::now();
        let populated_view = options.screenshot.is_some()
            && matches!(
                options.screen.as_str(),
                "native-spawn" | "native-player-spawn"
            );
        // Existing DEFAULT terrain saves are upgraded into an independent native checkpoint.
        // An explicit generator-3 CLI preview without a save identity remains available.
        let populated = !populated_view && options.world.is_none() && options.structure_preview.is_none()
            && options.world_rules.world_type.eq_ignore_ascii_case("default")
            && (options.generator == 4 || options.generator == 3 && options.save_id.is_some());
        if !populated && options.generator == 4 { options.generator = 3; }
        let mut population = None;
        let (mut world, pose, saved_simulation, spawns) = if populated {
            options.generator = 4;
            let loaded = crate::populated_world::PopulatedWorld::load(&options, progress)?;
            population = Some(loaded.host);
            (loaded.world, loaded.pose, loaded.simulation, Vec::new())
        } else if populated_view {
            let (world, pose, spawns) = native_view::populated_spawn_view(&options)?;
            (world, pose, None, spawns)
        } else {
            let loaded = World::load(&options)?;
            (loaded.world, loaded.pose, loaded.simulation, Vec::new())
        };
        let mut player = Player::new(world.spawn);
        player.third_person.slim=crate::ui::offline_uuid(&options.username).chunks_exact(4).fold(0,|v,b|v^b[3])&1!=0;
        let mode = if options.creative {
            Mode::Creative
        } else {
            Mode::Survival
        };
        let new_simulation = saved_simulation.is_none();
        let original_player = !populated_view && !options.benchmark && new_simulation;
        let mut sim = saved_simulation
            .map(Simulation::restore)
            .unwrap_or_else(|| Simulation::new(mode, options.seed));
        sim.local_name.clone_from(&options.username);
        if !options.benchmark && options.screenshot.is_none() {
            if let Ok(raw) = std::env::var("FERRUM189_MC_PROFILE_ID") {
                if let Some(uuid) = crate::player_skins::parse_uuid_hex(&raw) {
                    sim.presentation.mojang_uuid = Some(uuid);
                    sim.presentation.local_uuid.get_or_insert(uuid);
                    crate::player_skins::request_mojang_profile(uuid);
                }
            }
        }
        if options.world.is_none() {
            sim.spawn_seed = options.seed as i64;
        }
        if new_simulation && options.world.is_none() {
            sim.flat_world = options.world_rules.world_type.eq_ignore_ascii_case("flat");
            sim.hardcore = options.world_rules.hardcore;
            if sim.hardcore {sim.difficulty_locked = true;sim.world_difficulty=Some(3);}
        }
        if original_player {
            if let Some(path) = &options.world {
                sim.import_player(path)?;
            }
        }
        sim.world_difficulty.get_or_insert(if sim.hardcore { 3 } else { s.difficulty });
        if new_simulation && options.world.is_none() && options.world_rules.bonus_chest && !sim.hardcore {
            crate::world_library::place_bonus_chest(&mut world, &mut sim, options.seed);
        }
        player.yaw = sim.yaw;
        player.pitch = sim.pitch;
        if let Some(pose) = pose {
            pose.apply(&mut player);
            if original_player && options.world.is_some() && !options.creative {
                if let Some(motion) = pose.motion {
                    sim.mode = if motion.creative {
                        Mode::Creative
                    } else {
                        Mode::Survival
                    };
                }
            }
        }
        let imported_dim = if new_simulation {
            pose.and_then(|p| p.motion)
                .map(|m| m.dimension)
                .filter(|&d| d != 0)
        } else {
            None
        };
        if let Some(target_dim) = imported_dim {
            sim.import_containers(&world);
            crate::villagers::sync_import(&world, &mut sim);
            if let Some(host) = &mut population {
                host.flush_overworld_edits(&world)?;
            }
            sim.stash_active_dimension(&world);
            world.chunks.clear();
            world.edits = Default::default();
            world.invalidate_lighting();
            sim.activate_dimension(target_dim, &mut world);
        } else if sim.dimension != 0 {
            let target_dim = sim.dimension;
            world.chunks.clear();
            world.edits = Default::default();
            world.invalidate_lighting();
            sim.activate_dimension(target_dim, &mut world);
        }
        if sim.dimension != 0 {
            let target_dim = sim.dimension;
            let [px, _, pz] = player.precise_position();
            let p_center = (
                (px.floor() as i32).div_euclid(16),
                (pz.floor() as i32).div_euclid(16),
            );
            let radius = if options.progressive_world_entry() {
                3
            } else {
                options.render_distance as i32
            };
            let mut positions = Vec::new();
            for z in -radius..=radius {
                for x in -radius..=radius {
                    positions.push((p_center.0 + x, p_center.1 + z));
                }
            }
            if target_dim == 1 {
                for z in -1..=1 {
                    for x in -1..=1 {
                        if !positions.contains(&(x, z)) {
                            positions.push((x, z));
                        }
                    }
                }
            }
            let edits_map: BTreeMap<[i32; 3], u16> =
                world.edits.iter().map(|(&p, &s)| (p, s)).collect();
            let chunks = crate::dimension_world::read_dimension_chunks(
                target_dim,
                &options,
                &edits_map,
                &positions,
            )?;
            for (_, chunk) in &chunks {
                chunk.prepare_tick_indexes();
            }
            let loaded = world.integrate(chunks);
            sim.world_commands.restore_tiles(&mut world, &loaded);
            sim.import_containers(&world);
            crate::villagers::sync_import(&world, &mut sim);
            for &pos in &loaded {
                let meta =
                    crate::dimension_world::chunk_population_meta(target_dim, &options, pos);
                sim.adopt_dimension_meta(target_dim, &mut world, pos, meta);
            }
        } else {
            sim.import_containers(&world);
            crate::villagers::sync_import(&world, &mut sim);
            native_view::display_spawns(&world, &mut sim, spawns);
            if !world.imported && new_simulation && options.benchmark_scene.is_none() && population.is_none() {
                sim.spawn_initial(&world, player.position);
            }
        }
        if let Some(id)=sim.commands.player_mode{sim.apply_local_mode(id,&mut player);}
        if let Some(scene) = &options.benchmark_scene {
            let side = (2 * scene.render_distance + 1) as usize;
            anyhow::ensure!(
                world.chunks.len() == side * side,
                "Benchmark world has incomplete view distance"
            );
            anyhow::ensure!(sim.mobs.is_empty(), "Benchmark world must contain no mobs");
            sim.clock = scene.time;
            sim.daylight_cycle = false;
            sim.mob_spawning = false;
            player.set_pose_f64(
                scene.player_feet,
                scene.yaw_degrees,
                scene.pitch_degrees,
                true,
            );
        }
        if sim.sleep.head.is_some() {
            sim.wake(&mut world, &mut player, true, false);
        }
        if sim.dimension == 0 && !populated_view && population.is_none() {
            crate::villagers::sync_spawn(&options, &world, &mut sim);
        }
        if sim.dimension == 0 {
            if let Some(host) = &mut population { host.poll(&mut world, &mut sim)?; }
        }
        world.chunks.par_iter().for_each(|(_, chunk)| chunk.prepare_tick_indexes());
        let lighting_started = options.benchmark_streaming.then(Instant::now);
        let lighting = (!populated_view).then(|| {
            crate::lighting_view::LightingView::new_with_sky(&mut world, sim.dimension != 0)
        });
        let initial_lighting_ms = lighting_started.map(|t| t.elapsed().as_secs_f64() * 1000.);
        crate::remote_mobs::warm_render_models();
        let load_ms = start.elapsed().as_secs_f64() * 1000.0;
        let mesh_start = Instant::now();
        let [px, _, pz] = player.precise_position();
        let center = ((px.floor() as i32) >> 4, (pz.floor() as i32) >> 4);
        // The original ordered spawn barrier is complete. A fresh native view
        // can still be generating; enter only after the complete nearby 3x3.
        let progressive = options.progressive_world_entry();
        let initial_positions = initial_mesh_positions(&world, center, progressive);
        let mut meshes = initial_positions.par_iter()
            .map(|&p| (p, mesh::build_with_settings(&world, p, assets, s))).collect::<MeshBatch>();
        let initial_set = initial_positions.iter().copied().collect::<HashSet<_>>();
        let dirty = world.chunks.keys().filter(|p| !initial_set.contains(p)).copied().collect();
        let mesh_ms = mesh_start.elapsed().as_secs_f64() * 1000.0;
        progress(80);
        let faces: usize = meshes.iter().map(|(_, m)| m.visible_faces).sum();
        let quads: usize = meshes.iter().map(|(_, m)| m.quads()).sum();
        let histogram = if options.check {
            let mut states = std::collections::BTreeMap::<u16, u64>::new();
            for c in world.chunks.values() {
                for &state in &c.blocks {
                    if state != 0 {
                        *states.entry(state).or_default() += 1;
                    }
                }
            }
            Some(states)
        } else {
            None
        };
        if options.check
            || options.benchmark
            || options.screenshot.is_some()
            || options.benchmark_output_dir.is_some()
        {
            let reports = options.project_dir.join("reports");
            std::fs::create_dir_all(&reports)?;
            std::fs::write(
                reports.join("world.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "kind":"world_load_and_cpu_mesh", "version":env!("CARGO_PKG_VERSION"),
                    "initial_mesh_chunks":initial_positions.len(), "progressive_mesh":progressive,
                    "requested_visible_radius_chunks":s.render_distance,
                    "world":world.label, "chunks":world.chunks.len(), "imported":world.imported,
                    "world_spawn":world.spawn.to_array(), "player_position":player.precise_position(), "display_mobs":sim.mobs.len(),
                    "player_motion":player.precise_velocity(), "player_rotation":player.java_rotation(),
                    "player_on_ground":player.grounded, "personal_spawn":sim.bed_spawn, "radius_chunks":options.render_distance,
                    "load_ms":load_ms, "mesh_ms":mesh_ms, "exposed_faces":faces,
                    "startup_phases":population.as_ref().map(|p| p.startup_metrics()),
                    "greedy_quads":quads, "block_states":histogram,
                    "geometry_reduction":faces as f64/quads.max(1) as f64,
                    "note":"CPU mesh counts are not an FPS speedup."
                }))?,
            )?;
        }
        println!("{} | {} chunks | load {load_ms:.1} ms, mesh {mesh_ms:.1} ms | {faces} faces -> {quads} quads",world.label,world.chunks.len());
        meshes.sort_unstable_by_key(|(p, _)| ((p.0-center.0).pow(2)+(p.1-center.1).pow(2), *p));
        let entry_pending = mesh_neighbors(center).into_iter()
            .filter(|p| (sim.dimension == 0 && population.is_some()) || world.chunks.contains_key(p)).collect();
        let visual_revision = world.light_revision;
        if options.benchmark_streaming { sim.commands.start_profile(sim.total_ticks); }
        // The loading worker also performs index partitioning; the frame loop
        // only allocates/writes GPU buffers, nearest chunks first.
        let (uploads, prepared) = if progressive {
            (Default::default(), meshing::prepare_initial(&world, meshes, assets, s))
        } else {
            (meshes.into_par_iter().map(|(p, mesh)| (p, crate::renderer::PreparedChunk::new(mesh)))
                .collect::<Vec<_>>().into(), Default::default())
        };
        if let Some(ms)=initial_lighting_ms {chunk_timings.insert("initial_client_lighting",[1.,ms,ms]);}
        Ok(Self {
            options,
            world,
            lighting,
            player,
            sim,
            dirty,
            rebuild: None,
            uploads,
            prepared,
            entry_pending,
            entry_uploaded: Default::default(),
            visual_revision,
            chunk_timings,
            presentation_copy_counts: Default::default(),
            spare_presentation_brains: Default::default(),
            stream: None,
            stream_center: None,
            imports_pending: false,
            attempted: HashSet::new(),
            remote: None,
            population,
            distant,
        })
    }
    fn save(&mut self) -> Result<()> {
        if self.options.benchmark || self.remote.is_some() {
            return Ok(());
        }
        self.sim.return_cursor(&self.player);
        self.sim.yaw = self.player.yaw;
        self.sim.pitch = self.player.pitch;
        if self.sim.dimension != 0 {
            self.sim.snapshot_active_dimension_for_save(&self.world);
        }
        if let Some(host) = &mut self.population {
            host.save(&mut self.world,
                crate::player_save::PlayerSave::capture(&self.player, self.sim.mode), &mut self.sim)?;
            if let Some(lighting) = &mut self.lighting {
                for pos in lighting.update(&mut self.world) { self.dirty.extend(mesh_neighbors(pos)); }
            }
            return Ok(());
        }
        let pose = crate::player_save::PlayerSave::capture(&self.player, self.sim.mode);
        if self.sim.dimension != 0 {
            let mut ow_edits = crate::world::EditJournal::default();
            for (p, s) in self.sim.overworld_saved_edits(&self.world) {
                ow_edits.insert(p, s);
            }
            std::mem::swap(&mut self.world.edits, &mut ow_edits);
            let res = self.world.save(&self.options, pose, &self.sim);
            std::mem::swap(&mut self.world.edits, &mut ow_edits);
            return res;
        }
        self.world.save(&self.options, pose, &self.sim)
    }
    fn mark(&mut self, p: IVec3) {
        let (x, z) = (p.x.div_euclid(16), p.z.div_euclid(16));
        for dx in -1..=1 {
            for dz in -1..=1 {
                if dx == -1 && p.x.rem_euclid(16) != 0 || dx == 1 && p.x.rem_euclid(16) != 15 {
                    continue;
                }
                if dz == -1 && p.z.rem_euclid(16) != 0 || dz == 1 && p.z.rem_euclid(16) != 15 {
                    continue;
                }
                self.dirty.insert((x + dx, z + dz));
            }
        }
    }
    fn autosave(&mut self) -> Result<bool> {
        if self.options.benchmark || self.remote.is_some() { return Ok(true); }
        if let Some(host) = &mut self.population {
            self.sim.yaw = self.player.yaw;
            self.sim.pitch = self.player.pitch;
            if self.sim.dimension != 0 {
                self.sim.snapshot_active_dimension_for_save(&self.world);
            }
            return host.autosave(&mut self.world,
                crate::player_save::PlayerSave::capture(&self.player, self.sim.mode), &self.sim);
        }
        self.save()?;
        Ok(true)
    }
    fn overworld_respawn_centers(&self) -> (ChunkPos, Option<ChunkPos>) {
        let ow_spawn = if self.sim.dimension == 0 {
            self.world.spawn
        } else if let Some(cmd_spawn) = self.sim.world_commands.spawn {
            IVec3::from_array(cmd_spawn).as_vec3()
        } else {
            self.sim
                .overworld_spawn
                .map_or(self.world.spawn, Vec3::from_array)
        };
        let world_spawn = ow_spawn.floor().as_ivec3();
        let spawn_center = (world_spawn.x.div_euclid(16), world_spawn.z.div_euclid(16));
        if let Some(bed) = self.sim.bed_spawn {
            let bed_center = (bed.point.x.div_euclid(16), bed.point.z.div_euclid(16));
            let fallback = (spawn_center != bed_center).then_some(spawn_center);
            (bed_center, fallback)
        } else {
            (spawn_center, None)
        }
    }
    fn take_cached_dimension(
        &mut self,
        target_dim: i32,
    ) -> Option<(crate::world::ChunkMap, Option<crate::lighting_view::LightingView>)> {
        let key = self as *const Self as usize;
        let mut guard = CACHED_DIMENSIONS.lock().unwrap_or_else(|e| e.into_inner());
        if guard.get(&key).is_some_and(|c| c.dimension == target_dim) {
            guard.remove(&key).map(|c| (c.chunks, c.lighting))
        } else {
            None
        }
    }
    fn store_cached_dimension(
        &mut self,
        dimension: i32,
        chunks: crate::world::ChunkMap,
        lighting: Option<crate::lighting_view::LightingView>,
    ) {
        let key = self as *const Self as usize;
        CACHED_DIMENSIONS.lock().unwrap_or_else(|e| e.into_inner()).insert(
            key,
            CachedDimensionView {
                dimension,
                chunks,
                lighting,
            },
        );
    }
    fn ensure_overworld_region(&mut self, center: ChunkPos, radius: i32) -> Result<bool> {
        let mut positions = Vec::new();
        for z in -radius..=radius {
            for x in -radius..=radius {
                positions.push((center.0 + x, center.1 + z));
            }
        }
        if let Some(host) = &mut self.population {
            let dirty = host.ensure_priority_chunks_blocking(
                &mut self.world,
                &mut self.sim,
                &positions,
            )?;
            if !dirty.is_empty() {
                self.dirty.extend(dirty);
                if let Some(lighting) = &mut self.lighting {
                    for pos in lighting.update(&mut self.world) {
                        self.dirty.extend(mesh_neighbors(pos));
                    }
                }
                return Ok(true);
            }
            Ok(false)
        } else {
            let missing: Vec<ChunkPos> = positions
                .into_iter()
                .filter(|cp| !self.world.chunks.contains_key(cp))
                .collect();
            if missing.is_empty() {
                return Ok(false);
            }
            let chunks = World::read_chunks(&self.options, &missing)?;
            for (_, chunk) in &chunks {
                chunk.prepare_tick_indexes();
            }
            let loaded = self.world.integrate(chunks);
            let edits_copy: Vec<_> = self.world.edits.iter().map(|(&p, &s)| (p, s)).collect();
            for (p, s) in edits_copy {
                let cp = (p[0].div_euclid(16), p[2].div_euclid(16));
                if self.world.chunks.contains_key(&cp) && self.world.get(IVec3::from_array(p)) != s
                {
                    self.world.set_raw(IVec3::from_array(p), s);
                }
            }
            self.sim.world_commands.restore_tiles(&mut self.world, &loaded);
            self.sim.import_containers(&self.world);
            crate::villagers::sync_import(&self.world, &mut self.sim);
            crate::villagers::sync_spawn(&self.options, &self.world, &mut self.sim);
            let loaded_any = !loaded.is_empty();
            for pos in loaded {
                self.dirty.extend(mesh_neighbors(pos));
            }
            if loaded_any {
                if let Some(lighting) = &mut self.lighting {
                    for pos in lighting.update(&mut self.world) {
                        self.dirty.extend(mesh_neighbors(pos));
                    }
                }
            }
            Ok(loaded_any)
        }
    }
    pub(crate) fn switch_dimension_region(
        &mut self,
        target_dim: i32,
        target_center: ChunkPos,
        arrival_radius: i32,
        renderer: Option<&mut Renderer>,
        _assets: &Assets,
        _settings: &Settings,
    ) -> Result<()> {
        if self.sim.dimension == 0 {
            if let Some(host) = &mut self.population {
                host.flush_overworld_edits(&self.world)?;
            }
        }
        let departed_dim = self.sim.dimension;
        self.sim.stash_active_dimension(&self.world);
        let departed_chunks = std::mem::take(&mut self.world.chunks);
        let departed_lighting = self.lighting.take();
        let restored = self.take_cached_dimension(target_dim);
        if departed_dim != target_dim && !departed_chunks.is_empty() {
            self.store_cached_dimension(departed_dim, departed_chunks, departed_lighting);
        }
        self.world.invalidate_lighting();
        self.dirty.clear();
        self.rebuild = None;
        self.uploads.clear();
        self.prepared.clear();
        self.entry_pending.clear();
        self.entry_uploaded.clear();
        self.attempted.clear();
        self.stream = None;
        self.stream_center = None;
        self.imports_pending = false;
        if let Some(r) = renderer {
            r.clear();
        }
        if let Some(scene) = &mut self.distant {
            scene.notify_renderer_cleared();
        }
        self.sim.activate_dimension(target_dim, &mut self.world);
        let had_restored_chunks = restored.is_some();
        if let Some((chunks, cached_lighting)) = restored {
            let restored_positions: Vec<ChunkPos> = chunks.keys().copied().collect();
            self.world.chunks = chunks;
            self.lighting = cached_lighting;
            let edits_copy: Vec<_> = self.world.edits.iter().map(|(&p, &s)| (p, s)).collect();
            for (p, s) in edits_copy {
                let cp = (p[0].div_euclid(16), p[2].div_euclid(16));
                if self.world.chunks.contains_key(&cp) && self.world.get(IVec3::from_array(p)) != s
                {
                    self.world.set_raw(IVec3::from_array(p), s);
                }
            }
            self.sim
                .world_commands
                .restore_tiles(&mut self.world, &restored_positions);
        }
        if let Some(l) = &mut self.lighting {
            l.set_no_sky(target_dim != 0);
        } else {
            self.lighting = Some(crate::lighting_view::LightingView::new_with_sky(
                &mut self.world,
                target_dim != 0,
            ));
        }
        let mut positions = Vec::new();
        for z in -arrival_radius..=arrival_radius {
            for x in -arrival_radius..=arrival_radius {
                positions.push((target_center.0 + x, target_center.1 + z));
            }
        }
        if target_dim == 1 {
            for z in -1..=1 {
                for x in -1..=1 {
                    if !positions.contains(&(x, z)) {
                        positions.push((x, z));
                    }
                }
            }
        }
        let missing: Vec<ChunkPos> = positions
            .iter()
            .copied()
            .filter(|p| !self.world.chunks.contains_key(p))
            .collect();
        if target_dim == 0 {
            if let Some(host) = &mut self.population {
                let dirty = host.ensure_priority_chunks_blocking(
                    &mut self.world,
                    &mut self.sim,
                    &positions,
                )?;
                self.dirty.extend(dirty);
            } else if !missing.is_empty() {
                let chunks = World::read_chunks(&self.options, &missing)?;
                for (_, chunk) in &chunks {
                    chunk.prepare_tick_indexes();
                }
                let loaded = self.world.integrate(chunks);
                let edits_copy: Vec<_> = self.world.edits.iter().map(|(&p, &s)| (p, s)).collect();
                for (p, s) in edits_copy {
                    let cp = (p[0].div_euclid(16), p[2].div_euclid(16));
                    if self.world.chunks.contains_key(&cp)
                        && self.world.get(IVec3::from_array(p)) != s
                    {
                        self.world.set_raw(IVec3::from_array(p), s);
                    }
                }
                self.sim.world_commands.restore_tiles(&mut self.world, &loaded);
                self.sim.import_containers(&self.world);
                crate::villagers::sync_import(&self.world, &mut self.sim);
                crate::villagers::sync_spawn(&self.options, &self.world, &mut self.sim);
            }
        } else if !missing.is_empty() {
            let edits_map: BTreeMap<[i32; 3], u16> =
                self.world.edits.iter().map(|(&p, &s)| (p, s)).collect();
            let chunks = crate::dimension_world::read_dimension_chunks(
                target_dim,
                &self.options,
                &edits_map,
                &missing,
            )?;
            for (_, chunk) in &chunks {
                chunk.prepare_tick_indexes();
            }
            let loaded = self.world.integrate(chunks);
            self.sim.world_commands.restore_tiles(&mut self.world, &loaded);
            self.sim.import_containers(&self.world);
            crate::villagers::sync_import(&self.world, &mut self.sim);
            for &pos in &loaded {
                let meta = crate::dimension_world::chunk_population_meta(
                    target_dim,
                    &self.options,
                    pos,
                );
                self.sim.adopt_dimension_meta(target_dim, &mut self.world, pos, meta);
            }
        }
        if had_restored_chunks {
            for &pos in &missing {
                if let Some(chunk) = self.world.chunks.get(&pos) {
                    chunk.prepare_tick_indexes();
                }
            }
        } else {
            self.world
                .chunks
                .par_iter()
                .for_each(|(_, chunk)| chunk.prepare_tick_indexes());
        }
        Ok(())
    }
    fn mesh_arrival_chunks(
        &mut self,
        center: ChunkPos,
        mut renderer: Option<&mut Renderer>,
        assets: &Assets,
        settings: &Settings,
    ) {
        if let Some(l) = &mut self.lighting {
            l.update(&mut self.world);
        }
        self.visual_revision = self.world.light_revision;
        let mut nearby = mesh_neighbors(center)
            .into_iter()
            .filter(|p| self.world.chunks.contains_key(p))
            .collect::<Vec<_>>();
        nearby.sort_unstable_by_key(|p| {
            ((p.0 - center.0).pow(2) + (p.1 - center.1).pow(2), *p)
        });
        let nearby_set: HashSet<ChunkPos> = nearby.iter().copied().collect();
        let prepared: Vec<(ChunkPos, crate::renderer::PreparedChunk)> = nearby
            .par_iter()
            .map(|&p| {
                (
                    p,
                    crate::renderer::PreparedChunk::new(mesh::build_with_settings(
                        &self.world,
                        p,
                        assets,
                        settings,
                    )),
                )
            })
            .collect();
        if let Some(r) = renderer.as_deref_mut() {
            for (p, chunk) in prepared {
                r.upload_prepared(p, chunk);
            }
        } else {
            self.uploads.extend(prepared);
        }
        for &pos in self.world.chunks.keys() {
            if !nearby_set.contains(&pos) {
                self.dirty.insert(pos);
            }
        }
    }
    fn integrate(&mut self, chunks: Vec<(ChunkPos, Arc<Chunk>)>) {
        let loaded = self.world.integrate(chunks);
        self.sim.world_commands.restore_tiles(&mut self.world,&loaded);
        if loaded.is_empty() {
            return;
        }
        if self.sim.dimension != 0 {
            let dim = self.sim.dimension;
            for &pos in &loaded {
                let meta =
                    crate::dimension_world::chunk_population_meta(dim, &self.options, pos);
                self.sim.adopt_dimension_meta(dim, &mut self.world, pos, meta);
            }
        }
        self.imports_pending = true;
        for pos in loaded {
            self.dirty.extend(mesh_neighbors(pos));
        }
    }
    fn update_chunks(
        &mut self,
        renderer: &mut Renderer,
        assets: &Arc<Assets>,
        s: &Settings,
    ) -> Result<()> {
        let mut phase = Instant::now();
        if let Some(remote) = &mut self.remote {
            remote.visuals.particle_setting = s.particles;
            remote.poll(
                &mut self.world,
                &mut self.player,
                &mut self.sim,
                &mut self.dirty,
                renderer,
            )?;
        }
        let [x, _, z] = self.player.precise_position();
        let center = ((x.floor() as i32) >> 4, (z.floor() as i32) >> 4);
        let overworld_populated = self.sim.dimension == 0 && self.population.is_some();
        let radius = if overworld_populated {
            (s.full_detail_distance() as i32).max(3)
        } else {
            s.full_detail_distance() as i32
        };
        self.options.render_distance = s.full_detail_distance();
        if self.sim.dimension == 0 {
            if let Some(host) = &mut self.population {
                // poll may submit a queued batch: stamp this camera before that happens.
                host.maintenance_desired(center, radius + 1);
                let timings = self.options.benchmark_streaming.then_some(&mut self.chunk_timings);
                self.dirty.extend(host.poll_profiled(&mut self.world, &mut self.sim, timings)?);
            }
        }
        self.record_chunk_phase("native_publication", &mut phase);
        if let Some(rx) = &self.stream {
            match rx.try_recv() {
                Ok(batch) => {
                    self.integrate(batch?);
                    self.stream = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.stream = None;
                    anyhow::bail!("Chunk worker stopped");
                }
            }
        }
        self.record_chunk_phase("import_publication", &mut phase);
        if self.remote.is_none()
            && (!self.options.benchmark || self.options.benchmark_streaming)
            && self.stream.is_none()
            && (!overworld_populated || self.population.as_ref().is_none_or(|host| !host.busy()))
            && self.stream_center != Some(center)
        {
            self.stream_center = Some(center);
            let trim_started = self.options.benchmark_streaming.then(Instant::now);
            let expired = self
                .world
                .chunks
                .keys()
                .filter(|p| {
                    !(self.sim.dimension == 1 && p.0.abs() <= 1 && p.1.abs() <= 1)
                        && ((p.0 - center.0).abs() > radius + 1
                            || (p.1 - center.1).abs() > radius + 1)
                })
                .copied()
                .collect::<Vec<_>>();
            self.record_chunk_child("stream_trim_scan", trim_started);
            for (x, z) in expired {
                if self.sim.dimension == 0 {
                    if let Some(scene) = &mut self.distant {
                        let started = self.options.benchmark_streaming.then(Instant::now);
                        scene.capture_chunk(&self.world, (x, z));
                        self.record_chunk_child("stream_lod_capture", started);
                    }
                }
                let started = self.options.benchmark_streaming.then(Instant::now);
                self.world.unload_chunk((x, z));
                self.record_chunk_child("stream_world_unload", started);
                let started = self.options.benchmark_streaming.then(Instant::now);
                renderer.remove((x, z));
                self.record_chunk_child("stream_renderer_remove", started);
                self.dirty
                    .extend([(x - 1, z), (x + 1, z), (x, z - 1), (x, z + 1)]);
            }
            let prepare_started = self.options.benchmark_streaming.then(Instant::now);
            if self.sim.dimension == 0 {
                if let Some(host) = &mut self.population {
                    host.maintenance_commit(center, radius + 1);
                }
            }
            self.attempted.retain(|p| {
                (p.0 - center.0).abs() <= radius + 1 && (p.1 - center.1).abs() <= radius + 1
            });
            let generation_radius = radius + i32::from(overworld_populated);
            let mut positions = (-generation_radius..=generation_radius)
                .flat_map(|z| (-generation_radius..=generation_radius).map(move |x| (center.0 + x, center.1 + z)))
                .filter(|p| !self.world.chunks.contains_key(p) && !self.attempted.contains(p))
                .collect::<Vec<_>>();
            if overworld_populated {
                // PlayerManager creates chunks in X/Z order; only its send queue is distance sorted.
                positions.sort_unstable();
            } else {
                positions.sort_unstable_by_key(|p| (p.0 - center.0).pow(2) + (p.1 - center.1).pow(2));
            }
            let has_more_batches = self.sim.dimension != 0 && positions.len() > 16;
            if self.sim.dimension != 0 {
                positions.truncate(16);
            }
            self.stream_center = (!has_more_batches).then_some(center);
            self.record_chunk_child("stream_request_prepare", prepare_started);
            if !positions.is_empty() {
                let submit_started = self.options.benchmark_streaming.then(Instant::now);
                self.attempted.extend(positions.iter().copied());
                if self.sim.dimension == 0 {
                    if let Some(host) = &mut self.population {
                        host.request_chunks(&mut self.world, positions)?;
                    } else {
                        let options = self.options.clone();
                        let (tx, rx) = mpsc::channel();
                        rayon::spawn(move || {
                            let batch = World::read_chunks(&options, &positions).map(|chunks| {
                                for (_, chunk) in &chunks { chunk.prepare_tick_indexes(); }
                                chunks
                            });
                            let _ = tx.send(batch);
                        });
                        self.stream = Some(rx);
                    }
                } else {
                    let dim = self.sim.dimension;
                    let options = self.options.clone();
                    let edits_map: BTreeMap<[i32; 3], u16> =
                        self.world.edits.iter().map(|(&p, &s)| (p, s)).collect();
                    let (tx, rx) = mpsc::channel();
                    rayon::spawn(move || {
                        let batch = crate::dimension_world::read_dimension_chunks(
                            dim,
                            &options,
                            &edits_map,
                            &positions,
                        )
                        .map(|chunks| {
                            for (_, chunk) in &chunks {
                                chunk.prepare_tick_indexes();
                            }
                            chunks
                        });
                        let _ = tx.send(batch);
                    });
                    self.stream = Some(rx);
                }
                self.record_chunk_child("stream_request_submit", submit_started);
            }
        }
        if self.imports_pending {
            let import_started = self.options.benchmark_streaming.then(Instant::now);
            self.imports_pending = false;
            self.sim.import_containers(&self.world);
            crate::villagers::sync_import(&self.world, &mut self.sim);
            if self.sim.dimension == 0 && self.population.is_none() {
                crate::villagers::sync_spawn(&self.options, &self.world, &mut self.sim);
            }
            self.record_chunk_child("stream_import_metadata", import_started);
        }
        self.record_chunk_phase("stream_requests_and_imports", &mut phase);
        self.update_visuals(renderer, assets, s, center)?;
        self.record_chunk_phase("visual_jobs_gpu_upload", &mut phase);
        Ok(())
    }
    // Children of stream_requests_and_imports. Optional starts ensure ordinary
    // play performs no extra clock reads or timing-map updates. Per-column
    // counts distinguish capture/unload/remove costs from one view request.
    #[inline]
    fn record_chunk_child(&mut self, name: &'static str, started: Option<Instant>) {
        if let Some(started) = started {
            let ms = started.elapsed().as_secs_f64() * 1000.;
            let stats = self.chunk_timings.entry(name).or_default();
            stats[0] += 1.; stats[1] += ms; stats[2] = stats[2].max(ms);
        }
    }
    fn record_upload_observation(&mut self,renderer:&mut Renderer) {
        if !self.options.benchmark_capture_uploads{return;}
        if let Some(observation)=renderer.take_upload_observation() {
            for (name,span) in crate::renderer::upload_capture::PHASE_NAMES.into_iter().zip(observation.phases) {
                if span.calls!=0 {
                    // Preseeded at Game creation, before any upload: no new
                    // timing-map nodes are allocated by this diagnostic hook.
                    let stats=self.chunk_timings.get_mut(name).expect("upload timing keys preseeded");
                    stats[0]+=span.calls as f64;stats[1]+=span.total_ms;stats[2]=stats[2].max(span.max_ms);
                }
            }
        }
    }
    fn record_chunk_phase(&mut self, name: &'static str, since: &mut Instant) {
        if self.options.benchmark_streaming {
            let now = Instant::now();
            let ms = now.duration_since(*since).as_secs_f64() * 1000.;
            let stats = self.chunk_timings.entry(name).or_default();
            stats[0] += 1.; stats[1] += ms; stats[2] = stats[2].max(ms);
            *since = now;
        }
    }
}
fn worlds() -> Vec<WorldEntry> {
    crate::world_library::Library::default().list()
}
fn timestamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
#[cfg(windows)]
struct WinTimerPeriod;
#[cfg(windows)]
impl WinTimerPeriod {
    fn new() -> Self {
        #[link(name = "winmm")]
        extern "system" {
            fn timeBeginPeriod(period: u32) -> u32;
        }
        unsafe {
            timeBeginPeriod(1);
        }
        Self
    }
}
#[cfg(windows)]
impl Drop for WinTimerPeriod {
    fn drop(&mut self) {
        #[link(name = "winmm")]
        extern "system" {
            fn timeEndPeriod(period: u32) -> u32;
        }
        unsafe {
            timeEndPeriod(1);
        }
    }
}
pub fn run(mut options: Options, explicit_render_distance: bool) -> Result<()> {
    crate::remote_mobs::start_render_model_warmup();
    let mut settings = if let Some(scene) = &options.benchmark_scene {
        options.seconds = scene.measure_seconds;
        options.seed = scene.seed;
        options.render_distance = scene.render_distance;
        scene.settings()
    } else {
        Settings::load()?
    };
    if let Some(enabled) = options.entity_culling {
        settings.entity_culling = enabled;
    }
    if explicit_render_distance || options.check || options.benchmark {
        settings.render_distance = options.render_distance;
    }
    if options.vsync {
        settings.vsync = true;
    }
    if let Some(lang) = &options.language {
        settings.language = lang.to_lowercase();
    }
    let jar = options.jar.as_ref().unwrap();
    let load = |packs: &[String]| -> Result<_> {
        let stack = crate::resource_packs::Stack::load(packs)?;
        let assets = Arc::new(Assets::load_with_packs(jar, &stack)?);
        let resources = if options.check { None } else { Some(Arc::new(Resources::load_with_packs(stack)?)) };
        Ok((assets, resources))
    };
    let (assets, resources) = match load(&settings.menu.resource_packs) {
        Ok(pair) => pair,
        Err(error) if !settings.menu.resource_packs.is_empty() => {
            eprintln!("Failed to load selected resource packs: {error:#}. Loading default resources.");
            let pair = load(&[])?;
            // Apply this recovery in memory. A diagnostic run must not overwrite user preferences.
            settings.menu.resource_packs.clear();
            pair
        }
        Err(error) => return Err(error),
    };
    let Some(resources) = resources else {
        Game::load(options, &assets, &settings)?;
        return Ok(());
    };
    settings.locale_unicode = resources.unicode(&settings.language);
    println!(
        "Imported {} resources / {} OGG sounds",
        resources.resource_files, resources.sound_files
    );
    if options.screenshot.is_some() && options.connect.is_none() {
        return screenshot(options, assets, resources, settings);
    }
    #[cfg(windows)]
    let _win_timer_period = WinTimerPeriod::new();
    let event_loop = EventLoop::<AppEvent>::with_user_event().build()?;
    let mut app = App::new(options, assets, resources, settings);
    app.tick_proxy = Some(event_loop.create_proxy());
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.error {
        return Err(error);
    }
    Ok(())
}
fn screenshot(
    mut options: Options,
    assets: Arc<Assets>,
    resources: Arc<Resources>,
    mut settings: Settings,
) -> Result<()> {
    if options.screen == "graphics-check" {
        settings.anaglyph = true;
        settings.mipmaps = 0;
        settings.fancy = false;
        settings.smooth_lighting = 0;
        settings.alternate_blocks = true;
        settings.clouds = 1;
        settings.gamma = 1.0;
    }
    let screen = match options.screen.as_str() {
        "main" => Screen::Main,
        "multiplayer" | "multiplayer-selected" | "add-server" | "edit-server" | "direct-connect" | "delete-server" => Screen::Multiplayer,
        "realms" => Screen::Realms,
        "skin" => Screen::OptionPage(ui::options::Page::Skin),
        "sounds" => Screen::OptionPage(ui::options::Page::Sounds),
        "chat-settings" => Screen::OptionPage(ui::options::Page::Chat),
        "resourcepacks" => Screen::OptionPage(ui::options::Page::Packs),
        "snooper" => Screen::OptionPage(ui::options::Page::Snooper),
        "broadcast" => Screen::OptionPage(ui::options::Page::Broadcast),
        "worlds" | "worlds-empty" | "worlds-selected" => Screen::Worlds,
        "world-rename" => Screen::RenameWorld,
        "world-delete" => Screen::DeleteWorld,
        name if name=="create" || name.starts_with("create-") || name.starts_with("world-recreate") || name.starts_with("flat") || name.starts_with("customized") => Screen::Create,
        "options" => Screen::Options,
        "video" => Screen::Video,
        "languages" => Screen::Languages,
        "controls" => Screen::Controls,
        "command-block" => Screen::CommandBlock,
        "inventory" => Screen::Inventory,
        "crafting" => Screen::Crafting,
        "chest" | "doublechest" | "brewing" | "hopper" | "dispenser" | "dropper" | "horse-inventory" | "donkey-inventory" => Screen::Chest,
        "furnace" => Screen::Furnace,
        "trading" => Screen::Trading,
        "sleeping" => Screen::Sleeping,
        "creative" | "creative-search" | "creative-decorations" | "creative-inventory" => Screen::Creative,
        "pause" => Screen::Pause,
        "share-lan" => Screen::ShareLan,
        "achievements" => Screen::Achievements,
        "death" => Screen::Death,
        _ => Screen::Playing,
    };
    let needs_world = matches!(
        screen,
        Screen::Playing
            | Screen::Inventory
            | Screen::Crafting
            | Screen::Chest
            | Screen::Furnace
            | Screen::Trading
            | Screen::Creative
            | Screen::Pause
            | Screen::ShareLan
            | Screen::Achievements
            | Screen::Death
            | Screen::Sleeping
            | Screen::CommandBlock
    );
    options.render_distance = settings.render_distance;
    let mut game = if needs_world {
        Some(Game::load(options.clone(), &assets, &settings)?)
    } else {
        None
    };
    let mut gui = Gui {
        screen,
        worlds: worlds(),
        ..Default::default()
    };
    if screen == Screen::CommandBlock {
        if let Some(g) = &mut game {
            let pos = g.player.position.floor().as_ivec3() + IVec3::Z * 3;
            g.world.set(pos, 137 << 4);
            g.sim.mode = Mode::Creative;
            g.options.world_rules.allow_cheats = true;
            gameplay::command_blocks::save(&mut g.sim, &mut g.world, &g.options,
                pos, "say Hello Minecraft 1.8.9", true);
            gui.command_block = ui::command_block::Editor::from_world(&g.world, pos, &resources, &settings);
        }
    }
    if screen==Screen::Achievements {
        gui.achievements=ui::achievements::State::new(&options.username);
        if let Some(g)=&game {gui.statistics.open(&g.sim.statistics,None);}
    }
    if matches!(screen, Screen::Inventory | Screen::Creative) {
        gui.creative.set_skin(&options.username, game.as_ref().map(|g| &g.sim));
    }
    if screen==Screen::Creative {
        let tab=match options.screen.as_str(){"creative-decorations"=>1,"creative-inventory"=>ui::creative::PLAYER,"creative-search"=>ui::creative::SEARCH,_=>0};
        gui.creative.select(tab,&resources,&settings.language,false);
        if tab==ui::creative::SEARCH{gui.creative.search.insert(if settings.language.eq_ignore_ascii_case("ko_kr"){"다이아몬드"}else{"diamond"},15);gui.creative.refresh(&resources,&settings.language,false);}
        if let Some(g)=&mut game{g.sim.mode=Mode::Creative;g.sim.inventory.slots.fill(None);}
    }
    if options.screen == "worlds-empty" { gui.worlds.clear(); }
    if matches!(screen,Screen::RenameWorld|Screen::DeleteWorld) || options.screen=="worlds-selected" {
        gui.selected_world=(!gui.worlds.is_empty()).then_some(0);
        if let Some(e)=gui.worlds.first(){gui.world_list.begin_rename(&e.name);}
    }
    if matches!(screen,Screen::Worlds|Screen::RenameWorld|Screen::DeleteWorld) && options.language.as_deref().is_some_and(|l|l.eq_ignore_ascii_case("ko_kr")) {gui.world_list.reference_locale();}
    if screen==Screen::Create {
        gui.create = if options.screen.starts_with("world-recreate") && !gui.worlds.is_empty() {
            ui::create_world::Form::recreate(&gui.worlds[0],&resources,&settings.language)
        } else {ui::create_world::Form::new(&resources,&settings.language)};
        gui.create.capture(&options.screen,&resources,&settings.language);
    }
    if options.screen == "thirdperson" {
        gui.perspective = 2;
    }
    if screen == Screen::Main {gui.title.reference_capture();}
    if screen == Screen::Multiplayer { gui.multiplayer.capture(&options.screen,&resources,&settings.language)?; }
    if screen == Screen::OptionPage(ui::options::Page::Packs) {gui.option_state.open_packs(&settings.menu.resource_packs);}
    if options.screen == "debug" {
        gui.debug = true;
    }
    let mut renderer =
        pollster::block_on(Renderer::new(None, &assets, &resources, &settings, None))?;
    gui.option_state.set_device(&renderer);
    if let Some(g) = &mut game {
        if screen == Screen::Trading {
            g.sim.trades.insert(1, crate::villagers::Trader::default());
            gui.trader = Some(1);
            g.sim.inventory.slots[0] = Some(Stack::new(296, 40));
            g.sim.inventory.slots[1] = Some(Stack::new(388, 5));
        }
        if matches!(screen, Screen::Chest | Screen::Furnace) {
            let pos = g.player.position.floor().as_ivec3();
            gui.container = Some(pos);
            let kind = if screen == Screen::Chest {
                crate::container::Kind::Chest
            } else {
                crate::container::Kind::Furnace
            };
            let mut c = crate::container::Container::new(kind);
            if options.screen == "doublechest" {
                c.slots.resize(54, None);
                c.slots[53] = Some(Stack::new(264, 8));
            }
            c.slots[0] = Some(Stack::new(15, 8));
            c.slots[1] = Some(Stack::new(263, 4));
            if ui::utility_windows::capture(&options.screen,&mut c,&mut g.sim) {
                gui.container_title = if options.screen == "dropper" { Some(resources.tr(&settings.language,"container.dropper")) }
                    else if options.screen == "donkey-inventory" { Some(resources.tr(&settings.language,"entity.donkey.name")) } else { None };
            }
            gui.container_view = Some(c.clone());
            g.sim.containers.insert(gameplay::container_key(pos), c);
        }
        for (p, m) in g.uploads.drain(..) {
            renderer.upload(p, m);
        }
        for _ in 0..40 {
            renderer.tick_lighting(&g.world, &g.player, &g.sim, settings.render_distance);
            if matches!(
                options.screen.as_str(),
                "native-spawn" | "native-player-spawn"
            ) {
                continue;
            }
            g.sim.tick(
                &mut g.world,
                &mut g.player,
                &resources,
                &settings,
                false,
                false,
            );
        }
        if matches!(options.screen.as_str(),"held-grass"|"held-furnace"|"held-stairs"|"held-pickaxe"|"dropped-items"|"item-reference") {
            let eye=g.player.precise_eye(1.)+glam::DVec3::Y*80.;
            g.player.set_pose_f64((eye-glam::DVec3::Y*f64::from(1.62f32)).to_array(),180.,0.,false);
            g.sim.drops.clear();g.sim.mobs.clear();g.sim.xp_orbs.clear();g.sim.clock=6000;
            g.sim.inventory.slots.fill(None);g.sim.hand_swing=Default::default();
            let selected=match options.screen.as_str(){"held-grass"=>Some(2),"held-furnace"=>Some(61),"held-stairs"=>Some(53),"held-pickaxe"=>Some(257),_=>None};
            g.sim.inventory.slots[g.sim.inventory.selected]=selected.map(|id|Stack::new(id,1));
            for _ in 0..8{g.sim.equipped.tick(g.sim.inventory.held(),None);}
            gui.hide_hud=selected.is_none();
            if options.screen=="dropped-items" {
                for (i,(id,n)) in [(2,1),(53,2),(61,17),(257,49)].into_iter().enumerate(){
                    let pos=eye+glam::DVec3::new(-1.5+i as f64,-0.3,-3.);
                    let item=g.sim.spawn_item_entity(Stack::new(id,n),pos,glam::DVec3::ZERO,40);
                    item.age=20;item.hover_start=0.7;
                }
            }
        }
        if options.screen == "xp-orbs" {
            // Isolated screenshot of each original texture frame at a known
            // camera angle, without affecting a saved player or world.
            let eye = g.player.precise_eye(1.) + glam::DVec3::Y * 7.;
            g.player.set_pose_f64(
                (eye - glam::DVec3::Y * f64::from(1.62f32)).to_array(),
                180.,
                0.,
                false,
            );
            g.sim.mobs.clear();
            g.sim.drops.clear();
            g.sim.xp_orbs.clear();
            for (i, value) in [1, 3, 7, 17, 37, 73, 149, 307, 617, 1237, 2477]
                .into_iter()
                .enumerate()
            {
                let dx = (i % 4) as f64 * 0.6 - 0.9;
                let dy = 0.55 - (i / 4) as f64 * 0.52;
                g.sim.spawn_xp(eye + glam::DVec3::new(dx, dy, -2.8), value);
            }
            gui.hide_hud = true;
            g.sim.clock = 6000;
        }
        if matches!(options.screen.as_str(), "bed" | "sleeping") {
            let center = g.world.spawn.floor().as_ivec3();
            let top = (0..254)
                .rev()
                .find(|&y| g.world.get(IVec3::new(center.x, y, center.z)) != 0)
                .unwrap_or(64);
            let head = IVec3::new(center.x, top + 1, center.z);
            for dx in -2..=2 {
                for dz in -2..=2 {
                    let p = head + IVec3::new(dx, 0, dz);
                    g.world.set(p - IVec3::Y, 5 << 4);
                    g.world.set(p, 0);
                    g.world.set(p + IVec3::Y, 0);
                }
            }
            g.world.set(head, (26 << 4) | 8);
            g.world.set(head - IVec3::Z, 26 << 4);
            g.sim.mobs.clear();
            let feet = head.as_dvec3() + glam::DVec3::new(1.8, 0., -2.2);
            g.player.set_pose_f64(feet.to_array(), 25., 28., true);
            if options.screen == "sleeping" {
                g.sim.clock = 15000;
                let (res, _) = g.sim.try_bed(&mut g.world, &mut g.player, head);
                anyhow::ensure!(
                    res == crate::vanilla::sleep::Attempt::Asleep,
                    "Bed screenshot could not enter sleep"
                );
                for _ in 0..45 {
                    g.sim
                        .tick_sleep(&mut g.world, &mut g.player, &mut Default::default());
                }
            } else {
                g.sim.clock = 6000;
            }
            for z in (head.z - 2) >> 4..=(head.z + 2) >> 4 {
                for x in (head.x - 2) >> 4..=(head.x + 2) >> 4 {
                    let cp = (x, z);
                    if g.world.chunks.contains_key(&cp) {
                        renderer.upload(
                            cp,
                            mesh::build_with_settings(&g.world, cp, &assets, &settings),
                        );
                    }
                }
            }
        }
        if options.screen == "native-player-spawn" {
            g.player.set_rotation_degrees(0., 0.);
            g.sim.clock = 6000;
        }
        if options.screen == "hand" {
            let anchor = g.world.spawn.floor().as_ivec3();
            let top = (0..256)
                .rev()
                .find(|&y| g.world.get(IVec3::new(anchor.x, y, anchor.z)) != 0)
                .unwrap_or(64);
            g.player.set_pose_f64(
                [
                    f64::from(anchor.x) + 0.5,
                    f64::from(top + 1),
                    f64::from(anchor.z) + 0.5,
                ],
                0.,
                0.,
                true,
            );
            g.sim.inventory.slots[g.sim.inventory.selected] = None;
            g.sim.hand_swing = Default::default();
            g.sim.clock = 6000;
        }
        if options.screen == "native-spawn" {
            let point = g.world.spawn.as_ivec3();
            let top = (0..256)
                .rev()
                .find(|&y| g.world.get(IVec3::new(point.x, y, point.z)) != 0)
                .unwrap_or(point.y);
            let target = Vec3::new(point.x as f32 + 0.5, top as f32 + 1., point.z as f32 + 0.5);
            let eye = target + Vec3::new(26., 20., 30.);
            g.player
                .set_position_f64((eye - Vec3::Y * 1.62).as_dvec3().to_array());
            let look = (target - eye).normalize();
            g.player.yaw = look.x.atan2(-look.z);
            g.player.pitch = look.y.asin();
            g.sim.clock = 6000;
            gui.hide_hud = true;
        }
        if options.screen == "fluids" {
            // A repeatable Vulkan inspection scene, confined to this in-memory
            // screenshot world. It never writes to a save or the original JAR.
            let center = g.world.spawn.floor().as_ivec3();
            let y = center.y.max(64);
            for z in -10..=10 {
                for x in -10..=10 {
                    for dy in 0..=14 {
                        g.world.set(
                            IVec3::new(center.x + x, y + dy, center.z + z),
                            if dy == 0 { 1 << 4 } else { 0 },
                        );
                    }
                }
            }
            g.sim.block_ticks = Default::default();
            g.sim.mobs.clear();
            for (x, id) in [(-4, 8), (4, 10)] {
                let source = IVec3::new(center.x + x, y + 3, center.z);
                g.world.set(source - IVec3::Y, 1 << 4);
                g.world.set(source - IVec3::Y * 2, 1 << 4);
                g.world.set(source, id << 4);
                g.sim.block_ticks.notify(&g.world, source);
            }
            for _ in 0..90 {
                g.sim.block_ticks.tick(&mut g.world);
            }
            for (x, id) in [(-2, 12), (2, 13)] {
                let p = IVec3::new(center.x + x, y + 8, center.z - 4);
                g.world.set(p, id << 4);
                g.sim.block_ticks.notify(&g.world, p);
            }
            for _ in 0..5 {
                g.sim.block_ticks.tick(&mut g.world);
            }
            g.sim.clock = 6000;
            let target = Vec3::new(center.x as f32, y as f32 + 2., center.z as f32);
            g.player.position = target + Vec3::new(15., 11., 19.);
            g.player.previous = g.player.position;
            let look = (target - g.player.eye(1.)).normalize();
            g.player.yaw = look.x.atan2(-look.z);
            g.player.pitch = look.y.asin();
            gui.hide_hud = true;
            for (p, m) in mesh_world(&g.world, &assets, &settings) {
                renderer.upload(p, m);
            }
        }
        if matches!(
            options.screen.as_str(),
            "lighting" | "nightvision" | "absorption"
        ) {
            // In-memory closed room for GPU lightmap verification; no save I/O.
            let center = g.world.spawn.floor().as_ivec3();
            let y = center.y.max(64);
            for z in -12..=12 {
                for x in -10..=10 {
                    for dy in 0..=8 {
                        let edge = x == -10 || x == 10 || z == -12 || z == 12 || dy == 0 || dy == 8;
                        g.world.set(
                            IVec3::new(center.x + x, y + dy, center.z + z),
                            if edge { 1 << 4 } else { 0 },
                        );
                    }
                }
            }
            for (x, state) in [(-6, (50 << 4) | 5), (6, 89 << 4)] {
                g.world
                    .set(IVec3::new(center.x + x, y + 1, center.z - 4), state);
            }
            g.sim.mobs.clear();
            g.sim.drops.clear();
            g.sim.clock = 6000;
            let eye = Vec3::new(center.x as f32 + 0.5, y as f32 + 4.0, center.z as f32 + 9.0);
            let target = Vec3::new(center.x as f32 + 0.5, y as f32 + 1.5, center.z as f32 - 4.0);
            g.player
                .set_position_f64((eye - Vec3::Y * 1.62).as_dvec3().to_array());
            let look = (target - eye).normalize();
            g.player.yaw = look.x.atan2(-look.z);
            g.player.pitch = look.y.asin();
            gui.hide_hud = options.screen != "absorption";
            if options.screen == "nightvision" {
                g.sim
                    .effects
                    .load_effect(crate::vanilla::item_food::Effect::new(16, 400, 0))?;
            }
            if options.screen == "absorption" {
                g.sim.mode = crate::gameplay::Mode::Survival;
                g.sim.max_health = 20.;
                g.sim.health = 17.;
                g.sim.absorption = 5.;
            }
        }
        if options.screen == "village" || options.screen == "terrain" {
            g.sim.clock = 6000;
            let target = if options.generator == 2 && options.screen == "village" {
                let v = crate::worldgen::nearest_village(options.seed);
                Vec3::new(v.x as f32, v.y as f32 + 1., v.z as f32)
            } else {
                g.world.spawn + Vec3::Y
            };
            g.player.position = target + Vec3::new(39., 31., 39.);
            g.player.previous = g.player.position;
            let look = (target - g.player.position).normalize();
            g.player.yaw = look.x.atan2(-look.z);
            g.player.pitch = look.y.asin();
            gui.hide_hud = true;
        }
        if matches!(screen, Screen::Inventory | Screen::Crafting) {
            for (i, id) in [270, 17, 4, 5, 280, 263, 260, 58, 50].iter().enumerate() {
                g.sim.inventory.slots[i] = Some(Stack::new(*id, if i == 0 { 1 } else { 16 }));
            }
            g.sim.inventory.crafting[0] = Some(Stack::new(17, 2));
        }
        if options.screen == "entities" {
            g.player.pitch = -0.12;
            g.player.yaw = 0.0;
            g.sim.mobs.clear();
            for (i, kind) in [
                gameplay::MobKind::Cow,
                gameplay::MobKind::Pig,
                gameplay::MobKind::Sheep,
                gameplay::MobKind::Chicken,
                gameplay::MobKind::Zombie,
                gameplay::MobKind::Skeleton,
                gameplay::MobKind::Creeper,
                gameplay::MobKind::Spider,
            ]
            .into_iter()
            .enumerate()
            {
                let x = g.player.position.x + (i as f32 - 3.5) * 1.6;
                let z = g.player.position.z - 6.0;
                let y = gameplay::surface(&g.world, x.floor() as i32, z.floor() as i32)
                    .unwrap_or(g.player.position.y as i32);
                let p = Vec3::new(x, y as f32, z);
                g.sim.mobs.push(gameplay::Mob {
                    id: i as u64,
                    kind,
                    position: p,
                    previous: p,
                    velocity: Vec3::ZERO,
                    yaw: std::f32::consts::PI,
                    health: 20.0,
                    age: 0,
                    turn_ticks: 20,
                    hurt: 0,
                    fuse: 0,
                    swing_ticks: 0,
                    death_ticks: 0,
                    eat_ticks: 0,
                    attack_ticks: 0,
                    rose_ticks: 0,
                    prev_fuse: 0,
                });
            }
        }
        if let Some(lighting) = &mut g.lighting {
            let changed = lighting.update(&mut g.world);
            let mut remesh = HashSet::new();
            for (x, z) in changed {
                remesh.extend(mesh_neighbors((x, z)));
            }
            for pos in remesh
                .into_iter()
                .filter(|p| g.world.chunks.contains_key(p))
            {
                renderer.upload(
                    pos,
                    mesh::build_with_settings(&g.world, pos, &assets, &settings),
                );
            }
        }
    }
    if screen==Screen::Creative{if let Some(g)=&mut game{g.sim.total_ticks=0;}}
    let stats = Stats {
        gpu: &renderer.adapter_name,
        time: if screen == Screen::Create { 0. } else { 4. },
        ..Default::default()
    };
    let frame = ui::build(
        &gui,
        &settings,
        &resources,
        &assets,
        renderer.resolution(),
        game.as_ref().map(|g| &g.sim),
        &stats,
    );
    renderer.screenshot(
        options.screenshot.as_ref().unwrap(),
        &RenderInput {
            selected_block: game.as_ref().and_then(|g| g.selected_block(&gui, &g.player, 1.)),
            assets: &assets,
            resources: &resources,
            settings: &settings,
            gui: &gui,
            ui: &frame,
            world: game.as_ref().map(|g| &g.world),
            player: game.as_ref().map(|g| &g.player),
            simulation: game.as_ref().map(|g| &g.sim),
            alpha: 1.0,
            time: 4.0,
        },
    )
}
struct InventoryDrag {
    button: MouseButton,
    origin: SlotRef,
    slots: Vec<SlotRef>,
}
fn action_slot(action: &Action) -> Option<SlotRef> {
    match *action {
        Action::Slot(i) if i >= 100 => Some(SlotRef::Craft(i - 100)),
        Action::Slot(i) => Some(SlotRef::Player(i)),
        Action::ContainerSlot(i) => Some(SlotRef::Container(i)),
        Action::Craft => Some(SlotRef::CraftOutput),
        _ => None,
    }
}
#[derive(Debug)]
enum AppEvent {
    LocalTick,
    WorldLoadReady,
}

struct App {
    requested_pack:Option<(String,String)>,
    downloading_pack:Option<(String,mpsc::Receiver<Result<PathBuf>>)>,
    server_pack:Option<PathBuf>,
    connecting:bool,
    options: Options,
    assets: Arc<Assets>,
    resources: Arc<Resources>,
    settings: Settings,
    settings_path: PathBuf,
    library: crate::world_library::Library,
    quit_requested: bool,
    gui: Gui,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    game: Option<Game>,
    loading: Option<mpsc::Receiver<Result<Game>>>,
    load_updates: Option<mpsc::Receiver<u8>>,
    enter_when_ready: bool,
    ui_frame: ui::Frame,
    audio: Audio,
    keys: HashSet<KeyCode>,
    character_keys: HashMap<PhysicalKey, crate::keybindings::Binding>,
    left: bool,
    right: bool,
    local_right_clicks: u32,
    mouse_accum: [f64; 2],
    accumulator: f32,
    started: Instant,
    last_frame: Instant,
    last_tick_pump: Instant,
    last_stats: Instant,
    last_save: Instant,
    last_window_title: String,
    fps: f32,
    frame_ms: f32,
    stats_frames: u32,
    stats_elapsed: f32,
    screenshot_pending: bool,
    benchmark_start: Option<Instant>,
    entry_started: Option<Instant>,
    entry_data_ready_ms: Option<f64>,
    entry_report: Option<serde_json::Value>,
    benchmark_sample_start: Option<Instant>,
    benchmark_streaming_origin: Option<[f64; 3]>,
    last_benchmark_present: Option<Instant>,
    samples: Vec<f64>,
    timing_max_ms: [f64; 3],
    render_timing_max_ms: [f64; 4],
    interval_profile: interval_profile::Recorder,
    error: Option<anyhow::Error>,
    smooth_camera: input::SmoothCamera,
    drag_control: Option<input::Drag>,
    touch_depth: u32,
    inventory_drag: Option<InventoryDrag>,
    last_inventory_click: Option<(Instant, SlotRef, MouseButton)>,
    local_tick: Option<crate::local_tick::LocalTickWorker>,
    tick_proxy: Option<EventLoopProxy<AppEvent>>,
    local_tick_failure: Option<String>,
    deferred_input: VecDeque<local_tick::DeferredInput>,
    sp_chest_lids: HashSet<IVec3>,
}
impl App {
    fn pickup_action(&self, right: bool) -> ClickAction {
        if self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight) {
            ClickAction::QuickMove
        } else {
            ClickAction::Pickup { right }
        }
    }
    fn hovered_slot(&self) -> Option<SlotRef> {
        self.ui_frame.buttons
            .iter()
            .rev()
            .find(|b| b.rect.contains(self.gui.cursor))
            .and_then(|b| action_slot(&b.action))
    }
    fn has_inventory_screen(&self) -> bool {
        matches!(
            self.gui.screen,
            Screen::Inventory
                | Screen::Crafting
                | Screen::Chest
                | Screen::Furnace
                | Screen::Creative
                | Screen::Trading
        )
    }
    fn window_click(&mut self, slot: SlotRef, action: ClickAction) -> Result<()> {
        if self.gui.screen==Screen::Creative{return self.creative_player_click(slot,action)}
        if let Some(g) = &mut self.game {
            if let Some(remote) = &mut g.remote {
                return remote.window_click(slot,action,&mut g.sim);
            }
            // The integrated server treats a spectator's container as read-only.
            if g.player.is_spectator() {
                return Ok(());
            }
            if self.gui.screen == Screen::Chest && g.sim.horse_window.is_some() {
                for item in g.sim.horse_window_click(slot, action) {
                    g.sim.throw_local_stack(&g.player,item,gameplay::ThrowSource::Window);
                }
                return Ok(());
            }
            if self.gui.screen == Screen::Chest && g.sim.minecart_window.is_some() {
                for item in g.sim.minecart_window_click(slot, action) {
                    g.sim.throw_local_stack(&g.player, item, gameplay::ThrowSource::Window);
                }
                return Ok(());
            }
            if self.gui.screen == Screen::Trading {
                let Some(id) = self.gui.trader else {
                    return Ok(());
                };
                let bound = g.sim.trading_with == Some(id)
                    && g.sim
                        .trades
                        .get(&id)
                        .is_some_and(|shop| shop.customer.as_deref() == Some(&g.options.username))
                    && g.sim
                        .mobs
                        .iter()
                        .any(|mob| mob.id == id && mob.health > 0. && !g.sim.is_baby(id));
                if !bound {
                    self.open(Screen::Playing);
                    return Ok(());
                }
                if let Some(trader) = g.sim.trades.get_mut(&id) {
                    let dropped = crate::container::merchant_click(
                        &mut g.sim.inventory,
                        &mut g.sim.merchant_window,
                        trader,
                        slot,
                        action,
                        g.sim.mode == Mode::Creative,
                    );
                    g.sim.flush_trade_feedback(id, g.player.position);
                    for item in dropped {
                        g.sim.throw_local_stack(&g.player,item,gameplay::ThrowSource::Window);
                    }
                }
                return Ok(());
            }
            let pos = if matches!(self.gui.screen, Screen::Chest | Screen::Furnace) {
                self.gui.container
            } else {
                None
            };
            let width = if self.gui.screen == Screen::Crafting {
                3
            } else {
                2
            };
            for item in g.sim.window_click(&g.world, pos, slot, action, width) {
                g.sim.throw_local_stack(&g.player,item,gameplay::ThrowSource::Window);
            }
        }
        Ok(())
    }
    fn inventory_mouse(&mut self, button: MouseButton, pressed: bool) -> Result<bool> {
        if !self.has_inventory_screen() || self.game.is_none() {
            return Ok(false);
        }
        if !pressed {
            if self
                .inventory_drag
                .as_ref()
                .is_some_and(|d| d.button == button)
            {
                let drag = self.inventory_drag.take().unwrap();
                let action = if drag.slots.len() > 1 || button == MouseButton::Middle {
                    ClickAction::Drag {
                        mode: if button == MouseButton::Right {
                            1
                        } else if button == MouseButton::Middle {
                            2
                        } else {
                            0
                        },
                        slots: drag.slots,
                    }
                } else {
                    ClickAction::Pickup {
                        right: button == MouseButton::Right,
                    }
                };
                self.window_click(drag.origin, action)?;
                return Ok(true);
            }
            return Ok(false);
        }
        let Some(slot) = self.hovered_slot() else {
            let r = self.renderer.as_ref().unwrap().resolution();
            let scale = self.settings.scale(r[0], r[1]);
            let width = if matches!(self.gui.special,ui::special_windows::Special::Beacon{..}){230.}else if self.gui.screen == Screen::Creative {
                195.
            } else {
                176.
            };
            let height = if matches!(self.gui.special,ui::special_windows::Special::Beacon{..}){219.}
            else if matches!(self.gui.special,ui::special_windows::Special::Anvil{..}) || self.gui.enchanting.is_some(){166.}
            else if self.gui.screen == Screen::Creative {
                136.
            } else if self.gui.screen == Screen::Chest {
                self.gui.container_view.as_ref().and_then(|c|ui::utility_windows::height(c.special)).unwrap_or_else(||114. + self
                    .gui
                    .container_view
                    .as_ref()
                    .map_or(3, |c| c.slots.len().div_ceil(9)) as f32
                    * 18.)
            } else {
                166.
            };
            let effects = matches!(self.gui.screen, Screen::Inventory | Screen::Creative)
                && self.game.as_ref().is_some_and(|g| g.sim.effects.ordered().next().is_some());
            let panel = ui::Rect {
                x: ui::inventory_left(r[0] as f32 / scale, width, effects),
                y: (((r[1] as f32 / scale).ceil() - height) / 2.).trunc(),
                w: width,
                h: height,
            };
            let on_button = self
                .ui_frame.buttons
                .iter()
                .any(|b| b.rect.contains(self.gui.cursor));
            if !panel.contains(self.gui.cursor)
                && !on_button
                && matches!(button, MouseButton::Left | MouseButton::Right)
            {
                self.window_click(
                    SlotRef::Outside,
                    ClickAction::Pickup {
                        right: button == MouseButton::Right,
                    },
                )?;
                return Ok(true);
            }
            return Ok(false);
        };
        if !matches!(
            button,
            MouseButton::Left | MouseButton::Right | MouseButton::Middle
        ) {
            return Ok(false);
        }
        let shift =
            self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
        let cursor = self
            .game
            .as_ref()
            .and_then(|g| g.sim.inventory.cursor.as_ref())
            .is_some();
        if button == MouseButton::Middle {
            let creative = self
                .game
                .as_ref()
                .is_some_and(|g| g.sim.mode == Mode::Creative);
            if cursor && creative && !matches!(slot, SlotRef::CraftOutput) {
                self.inventory_drag = Some(InventoryDrag {
                    button,
                    origin: slot,
                    slots: vec![slot],
                });
            } else {
                self.window_click(slot, ClickAction::Clone)?;
            }
            self.last_inventory_click = Some((Instant::now(), slot, button));
            return Ok(true);
        }
        let double = button == MouseButton::Left
            && self
                .last_inventory_click
                .is_some_and(|(time, last, last_button)| {
                    last == slot
                        && last_button == button
                        && time.elapsed() < Duration::from_millis(250)
                });
        self.last_inventory_click = Some((Instant::now(), slot, button));
        if double && cursor && !shift {
            self.window_click(slot, ClickAction::Collect { reverse: false })?;
        } else if shift {
            self.window_click(slot, ClickAction::QuickMove)?;
        } else if cursor && !matches!(slot, SlotRef::CraftOutput) {
            self.inventory_drag = Some(InventoryDrag {
                button,
                origin: slot,
                slots: vec![slot],
            });
        } else {
            self.window_click(
                slot,
                ClickAction::Pickup {
                    right: button == MouseButton::Right,
                },
            )?;
        }
        Ok(true)
    }
    fn new(
        options: Options,
        assets: Arc<Assets>,
        resources: Arc<Resources>,
        settings: Settings,
    ) -> Self {
        let now = Instant::now();
        let interval_profile = interval_profile::Recorder::with_threshold(options.benchmark_slow_frame_ms.unwrap_or_default());
        let hide_hud = options
            .benchmark_scene
            .as_ref()
            .is_some_and(|scene| !scene.hud);
        Self {
            options,
            assets,
            resources,
            settings,
            settings_path: "options.json".into(),
            requested_pack:None,downloading_pack:None,server_pack:None,connecting:false,
            library: Default::default(),
            quit_requested:false,
            gui: Gui {
                hide_hud,
                worlds: worlds(),
                ..Default::default()
            },
            window: None,
            renderer: None,
            game: None,
            loading: None,
            load_updates: None,
            enter_when_ready: false,
            ui_frame: ui::Frame::default(),
            audio: Audio::new(),
            keys: HashSet::new(),
            character_keys: HashMap::new(),
            left: false,
            right: false,
            local_right_clicks: 0,
            mouse_accum: [0.0; 2],
            accumulator: 0.0,
            started: now,
            last_frame: now,
            last_tick_pump: now,
            last_stats: now,
            last_save: now,
            last_window_title: String::new(),
            smooth_camera: Default::default(),
            drag_control: None,
            touch_depth: 0,
            inventory_drag: None,
            last_inventory_click: None,
            local_tick: None,
            tick_proxy: None,
            local_tick_failure: None,
            deferred_input: VecDeque::new(),
            sp_chest_lids: HashSet::new(),
            fps: 0.0,
            frame_ms: 0.0,
            stats_frames: 0,
            stats_elapsed: 0.0,
            screenshot_pending: false,
            benchmark_start: None,
            entry_started: None,
            entry_data_ready_ms: None,
            entry_report: None,
            benchmark_sample_start: None,
            benchmark_streaming_origin: None,
            last_benchmark_present: None,
            samples: Vec::new(),
            timing_max_ms: [0.0; 3],
            render_timing_max_ms: [0.; 4],
            interval_profile,
            error: None,
        }
    }
    fn fail(&mut self, event_loop: &ActiveEventLoop, e: anyhow::Error) {
        eprintln!("{e:#}");
        self.error = Some(e);
        event_loop.exit();
    }
    fn capture(&mut self) {
        let captured = self.gui.screen == Screen::Playing && !self.options.benchmark && self.options.screenshot.is_none();
        if let Some(w) = &self.window {
            if captured {
                if let Err(e) = w
                    .set_cursor_grab(CursorGrabMode::Locked)
                    .or_else(|_| w.set_cursor_grab(CursorGrabMode::Confined))
                {
                    eprintln!("Cursor capture: {e}");
                }
            } else {
                let _ = w.set_cursor_grab(CursorGrabMode::None);
            }
            w.set_cursor_visible(!captured);
            w.set_ime_allowed(self.gui.screen==Screen::Creative || matches!(self.gui.screen, Screen::Video | Screen::Create | Screen::RenameWorld | Screen::Multiplayer | Screen::Chat | Screen::Sleeping | Screen::Sign | Screen::CommandBlock));
        }
        self.keys.clear();
        self.character_keys.clear();
        self.smooth_camera.reset_filter();
        self.touch_depth = 0;
        self.left = false;
        self.right = false;
        self.local_right_clicks = 0;
        self.mouse_accum = [0.0; 2];
        let is_remote = self.game.as_ref().is_some_and(|g| g.remote.is_some());
        if !is_remote {
            self.accumulator = 0.0;
            self.last_tick_pump = Instant::now();
        }
        if let Some(g) = &mut self.game {
            g.player.keys.clear();
            if !is_remote {
                g.player.previous = g.player.position;
            }
        }
    }
    fn open(&mut self, screen: Screen) {
        if self.tick_busy() {
            self.deferred_input.push_back(local_tick::DeferredInput::Open(screen));
            return;
        }
        if screen == Screen::Sleeping && self.gui.screen != screen {
            self.gui.chat.open(false);
        }
        if screen == Screen::Pause && self.gui.screen == Screen::Playing &&
            self.game.as_ref().is_some_and(|g| g.remote.is_none() && g.sim.lan.is_none()) {
            self.audio.pause_sounds();
        } else if screen == Screen::Playing {
            self.audio.resume_sounds();
        }
        if self.gui.screen == Screen::Multiplayer && screen != Screen::Multiplayer {
            self.gui.multiplayer.leave();
        }
        self.inventory_drag = None;
        self.last_inventory_click = None;
        self.gui.container_view = None;
        self.gui.container_title=None;
        self.gui.enchanting=None;
        self.gui.special=Default::default();
        self.drag_control = None;
        self.gui.chat.tab_open = false;
        if matches!(
            self.gui.screen,
            Screen::Inventory
                | Screen::Crafting
                | Screen::Creative
                | Screen::Chest
                | Screen::Furnace
                | Screen::Trading
        ) && screen != self.gui.screen
        {
            if let Some(g) = &mut self.game {
                if let Some(remote) = &mut g.remote {
                    if remote.extra.opens.is_none(){let _ = remote.close_inventory(&mut g.sim);}
                } else {
                    g.sim.horse_window = None;
                    g.sim.minecart_window = None;
                    g.sim.return_cursor(&g.player);
                }
            }
        }
        if screen == Screen::OptionPage(ui::options::Page::Packs)
            && self.gui.screen != Screen::OptionPage(ui::options::Page::IncompatiblePack) {
            self.gui.option_state.open_packs(&self.settings.menu.resource_packs);
            if self.resources.pack_icons_changed(&self.gui.option_state.packs) {
                let selected = self.settings.menu.resource_packs.clone();
                if let Err(error) = self.reload_packs(&selected) {
                    self.gui.option_state.error = error.to_string();
                }
            }
        }
        if screen == Screen::OptionPage(ui::options::Page::Snooper) {
            if let Some(renderer)=&self.renderer {self.gui.option_state.set_device(renderer);}
        }
        if matches!(screen, Screen::Inventory | Screen::Creative) {
            self.gui.creative.set_skin(&self.options.username, self.game.as_ref().map(|g| &g.sim));
        }
        if screen==Screen::Creative{self.gui.creative.select(self.gui.creative.tab,&self.resources,&self.settings.language,self.gui.advanced);}
        self.gui.open(screen);
        self.gui.status.clear();
        self.capture();
    }
    fn start(&mut self, mut options: Options) {
        self.entry_started = Some(Instant::now());
        self.entry_data_ready_ms = None;
        self.entry_report = None;
        self.connecting=options.connect.is_some();
        options.project_dir.clone_from(&self.library.project);
        options.render_distance = self.settings.render_distance;
        let assets = self.assets.clone();
        let settings = self.settings.clone();
        let (tx, rx) = mpsc::channel();
        let (progress_tx, progress_rx) = mpsc::channel();
        self.open(Screen::Loading);
        self.gui.status = self.resources.tr(&self.settings.language, "menu.generatingTerrain");
        if self.connecting{self.gui.status=self.resources.tr(&self.settings.language,"connect.connecting");}
        self.gui.loading_progress = Some(0);
        self.loading = Some(rx);
        self.load_updates = Some(progress_rx);
        self.enter_when_ready = true;
        if let Some(renderer)=&mut self.renderer {renderer.clear();}
        let completion_proxy = self.tick_proxy.clone();
        rayon::spawn(move || {
            let result = Game::load_with_progress(options, &assets, &settings,
                &mut |value| { let _ = progress_tx.send(value); });
            // Publish success or failure before waking its sole frame-side
            // consumer. A replaced/dropped receiver needs no redraw.
            if tx.send(result).is_ok() {
                if let Some(proxy) = completion_proxy {
                    let _ = proxy.send_event(AppEvent::WorldLoadReady);
                }
            }
        });
    }
    fn save(&mut self) -> Result<()> {
        anyhow::ensure!(self.local_tick_failure.is_none(), "A failed local tick cannot be saved");
        anyhow::ensure!(!self.tick_busy(), "Save must follow completion of the active local tick");
        if let Some(g) = &mut self.game {
            g.save()?;
        }
        self.last_save = Instant::now();
        Ok(())
    }
    fn save_quit(&mut self) -> Result<()> {
        let remote=self.game.as_ref().is_some_and(|g|g.remote.is_some());
        self.save()?;
        self.audio.leave_world();
        self.local_tick = None;
        self.deferred_input.clear();
        self.sp_chest_lids.clear();
        self.game = None;
        CACHED_DIMENSIONS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        crate::dimension_world::clear_caches();
        self.local_right_clicks = 0;
        self.requested_pack=None;self.downloading_pack=None;self.connecting=false;
        if remote {self.gui.multiplayer.enter()?;}
        if self.server_pack.take().is_some(){self.reload_packs(&self.settings.menu.resource_packs.clone())?;}
        self.renderer.as_mut().unwrap().clear();
        self.gui.worlds = self.library.list();
        self.open(if remote {Screen::Multiplayer} else {Screen::Main});
        Ok(())
    }
    fn action(&mut self, action: Action, right: bool) -> Result<()> {
        match action {
            Action::CommandCursor(at) => {
                let shift = self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
                self.gui.command_block.command.set_cursor(at, shift);
            }
            Action::CommandOutput => {}
            Action::ToggleCommandOutput => self.gui.command_block.track = !self.gui.command_block.track,
            Action::SaveCommandBlock => {
                let edit = &self.gui.command_block;
                let command = edit.command.value.clone();
                let allowed = self.game.as_mut().filter(|g| g.remote.is_none()).is_some_and(|g|
                    gameplay::command_blocks::save(&mut g.sim, &mut g.world, &g.options,
                        edit.position, &command, edit.track));
                let feedback = if allowed {
                    serde_json::json!({"translate":"advMode.setCommand.success","with":[command]})
                } else { serde_json::json!({"translate":"advMode.notAllowed"}) };
                self.gui.chat.receive(&feedback.to_string(), 1, &self.resources, &self.settings.language);
                self.open(Screen::Playing);
            }
            Action::AnvilCursor(i)=>{let shift=self.keys.contains(&KeyCode::ShiftLeft)||self.keys.contains(&KeyCode::ShiftRight);if let ui::special_windows::Special::Anvil{field,focused,..}=&mut self.gui.special{*focused=true;field.set_cursor(i,shift);}}
            Action::BeaconSelect(secondary,id)=>self.gui.special.beacon(secondary,id),
            Action::BeaconDone=>{
                if let ui::special_windows::Special::Beacon{primary,secondary,..}=self.gui.special{
                    if let Some(r)=self.game.as_mut().and_then(|g|g.remote.as_mut()){
                        if r.confirm_beacon(primary,secondary)?{self.open(Screen::Playing);}
                    }
                }
            }
            Action::BookPage(delta)=>self.gui.book.page_by(delta),
            Action::BookSigning(signing)=>self.gui.book.signing=signing,
            Action::BookFinish(signed)=>{
                if let Some(g)=self.game.as_mut(){
                    if let Some(item)=g.sim.inventory.held().and_then(|s|self.gui.book.finish(&s,signed)){
                        if let Some(remote)=&mut g.remote{
                            remote.queue_tick_command(crate::network::Command::WindowPayload(crate::network::special_payload::Payload::Book{signed,slot:crate::remote::to_slot(&item)?}))?;
                        }
                        g.sim.inventory.slots[g.sim.inventory.selected]=Some(item);
                    }
                }
                self.open(Screen::Playing);
            }
            Action::Enchant(choice)=>{
                if let Some(g)=self.game.as_mut(){
                    if let Some(r)=&mut g.remote{r.enchant_choice(choice,&g.sim)?;}
                }
            }
            Action::ServerPack(accept)=>{
                if let Some((url,hash))=self.requested_pack.take(){
                    if let Some(remote)=self.game.as_ref().and_then(|g|g.remote.as_ref()){
                        remote.client.send(crate::network::Command::PackStatus(hash.clone(),if accept{3}else{1}))?;
                        if accept{
                            let dir=self.library.project.join(".local/server-resource-packs");
                            let expected=hash.clone();let(tx,rx)=mpsc::channel();
                            std::thread::spawn(move||{let _=tx.send(crate::server_pack::download(&url,&expected,&dir));});
                            self.downloading_pack=Some((hash,rx));
                        }
                    }
                }
                if self.gui.screen==Screen::ServerPack {self.open(Screen::Playing);}
            }
            Action::FinishSign=>{
                if self.gui.screen==Screen::Sign{
                    if let Some(g)=self.game.as_mut(){
                        if let Some(remote)=g.remote.as_mut(){
                            remote.queue_tick_command(crate::network::Command::UpdateSign(self.gui.sign.pos,self.gui.sign.lines.clone()))?;
                        } else {
                            let p = glam::IVec3::from_array(self.gui.sign.pos);
                            let state = g.world.get(p);
                            if matches!(state >> 4, 63 | 68) {
                                if let Some(base) = crate::vanilla::middle_pick::tile_snapshot(state >> 4, p, g.world.tile(p), None) {
                                    if let Ok(updated) = crate::vanilla::tile_updates::apply_sign(&base, true, self.gui.sign.lines.clone()) {
                                        g.sim.world_commands.edit_tile(&mut g.world, p, state, Some(updated));
                                        g.mark(p);
                                    }
                                }
                            }
                        }
                    }
                    self.open(Screen::Playing);
                }
            }
            Action::Screen(Screen::Achievements)=>{
                if let Some(g)=&self.game {
                    if let Some(r)=&g.remote {r.client.send(crate::network::Command::RequestStats)?;self.gui.statistics.open(&r.extra.stats,Some(r.extra.stats_revision));}
                    else{self.gui.statistics.open(&g.sim.statistics,None);}
                    self.gui.achievements=ui::achievements::State::new(&g.options.username);
                    self.open(Screen::Achievements);
                }
            }
            Action::Screen(s) => {
                if s == Screen::Multiplayer && self.gui.screen == Screen::Main { self.gui.multiplayer.enter()?; }
                if s == Screen::Worlds {
                    self.gui.worlds = self.library.list();
                }
                if s == Screen::Create && self.gui.screen == Screen::Worlds {
                    self.gui.create = ui::create_world::Form::new(&self.resources,&self.settings.language);
                }
                self.open(s);
            }
            Action::Controls(control) => self.control_action(control)?,
            Action::Server(command) => {
                self.gui.multiplayer.apply(command, &self.resources, &self.settings.language)?;
                if let Some(address)=self.gui.multiplayer.connect_to.take(){
                    let mut opts=self.options.clone();opts.connect=Some(address);self.start(opts);
                }
            }
            Action::Option(command) => self.option_action(command)?,
            Action::Lan(command)=>{
                if self.gui.screen!=Screen::ShareLan{return Ok(())}
                match command {
                    ui::pause::Command::Mode=>self.gui.lan_form.cycle(),
                    ui::pause::Command::Cheats=>self.gui.lan_form.cheats=!self.gui.lan_form.cheats,
                    ui::pause::Command::Start=>{
                        let form=self.gui.lan_form;
                        self.open(Screen::Playing);
                        if let Some(g)=self.game.as_mut().filter(|g| g.remote.is_none() && g.sim.lan.is_none()) {
                            let msg=match crate::lan::publish_with_settings(&mut g.sim,&g.world,&g.player,&g.options,form.mode,form.cheats) {
                                Ok(port)=>serde_json::json!({"translate":"commands.publish.started","with":[port.to_string()]}),
                                Err(_)=>serde_json::json!({"text":"commands.publish.failed"}),
                            };
                            self.gui.chat.receive(&msg.to_string(),1,&self.resources,&self.settings.language);
                        }
                    }
                }
            }
            Action::Statistics(command)=>match command {
                ui::statistics::Command::Open=>{
                    if let Some(game)=&self.game {
                        if let Some(remote)=&game.remote {
                            remote.client.send(crate::network::Command::RequestStats)?;
                            self.gui.statistics.open(&remote.extra.stats,Some(remote.extra.stats_revision));
                        }else{self.gui.statistics.open(&game.sim.statistics,None);}
                        self.open(Screen::Statistics);
                    }
                }
                other=>self.gui.statistics.apply(other)
            },
            Action::ChatCursor(at) if !right => {
                self.gui.chat.caret=at.min(self.gui.chat.input.len());
                self.gui.chat.anchor=self.gui.chat.caret;
                self.gui.chat.cancel_completion();
            }
            Action::ChatCursor(_) => {}
            Action::ChatEvent(event) if !right => {
                use ui::chat::components::Click;
                let shift=self.keys.contains(&KeyCode::ShiftLeft)||self.keys.contains(&KeyCode::ShiftRight);
                if shift {
                    if let Some(text)=event.insertion {self.gui.chat.insert(&text);}
                } else if let Some(click)=event.click {
                    match click {
                        Click::Url(url) if self.settings.menu.chat_links => {
                            if self.settings.menu.chat_links_prompt {
                                self.gui.chat.url=url.to_string();
                                self.open(Screen::ConfirmChatLink);
                            } else {Self::visit_link(&url)?;}
                        }
                        Click::Url(_) => {}
                        Click::Suggest(text) => {
                            self.gui.chat.anchor=0;
                            self.gui.chat.caret=self.gui.chat.input.len();
                            self.gui.chat.insert(&text);
                        }
                        Click::Run(text) => self.send_chat_message(text.to_string())?,
                    }
                }
            }
            Action::ChatEvent(_) => {}
            Action::CopyChatLink => {
                crate::clipboard::set(&self.gui.chat.url);
                self.open(Screen::Chat);
            }
            Action::ChatLinkChoice(yes) => {
                if yes { Self::visit_link(&self.gui.chat.url)?; }
                self.open(Screen::Chat);
            }
            Action::Quit => {
                self.save()?;
                self.quit_requested = true;
            }
            Action::Resume => self.open(Screen::Playing),
            Action::SaveQuit => self.save_quit()?,
            Action::SelectWorld(i) => self.gui.selected_world = Some(i),
            Action::PlayWorld => {
                if let Some(entry) = self.gui.selected_world.and_then(|i| self.gui.worlds.get(i)) {
                    let mut options = self.options.clone();
                    entry.apply_to(&mut options);
                    self.start(options);
                }
            }
            Action::WorldForm(cmd) => {
                let shift = self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
                self.gui.create.apply(cmd, shift, self.gui.cursor);
            }
            Action::Worlds(cmd) => {
                use ui::world_select::Command;
                if let Some(entry) = self.gui.selected_world.and_then(|i| self.gui.worlds.get(i)).cloned() {
                    match cmd {
                        Command::Rename => {
                            self.gui.world_list.begin_rename(&entry.name);
                            self.open(Screen::RenameWorld);
                        }
                        Command::Delete => self.open(Screen::DeleteWorld),
                        Command::Recreate => {
                            self.gui.create = ui::create_world::Form::recreate(&entry, &self.resources, &self.settings.language);
                            self.open(Screen::Create);
                        }
                        Command::NameCursor(pos) => self.gui.world_list.set_cursor(pos),
                        Command::SaveName | Command::ConfirmDelete => {
                            let result = if matches!(cmd,Command::SaveName) {
                                self.library.rename(&entry,self.gui.world_list.name())
                            } else {self.library.delete(&entry)};
                            match result {
                                Ok(()) => {self.gui.worlds=self.library.list(); self.open(Screen::Worlds);}
                                Err(error) => self.gui.status = error.to_string(),
                            }
                        }
                    }
                }
            }
            Action::CreateWorld => {
                let requested = Instant::now();
                let f = &self.gui.create;
                if f.name().is_empty() { return Ok(()); }
                let random_seed=crate::vanilla::random::JavaRandom::new(crate::vanilla::random::JavaRandom::independent_seed()).next_long() as u64;
                let seed = crate::seed::from_gui(f.seed(),random_seed);
                let rules=crate::world_library::Rules {
                    world_type:f.world_type.name().into(),generator_options:f.generator_options.clone(),structures:f.structures,
                    allow_cheats:f.cheats(),bonus_chest:f.bonus(),hardcore:f.game_mode.hardcore(),
                };
                match crate::world_types::validate(&rules).and_then(|()|self.library.create(f.name(),seed,f.game_mode.mode(),rules)) {
                    Ok(entry) => {
                        let mut options = self.options.clone();
                        entry.apply_to(&mut options);
                        self.start(options);
                        self.entry_started = Some(requested);
                    }
                    Err(error) => self.gui.status=error.to_string(),
                }
            }
            Action::Setting(setting) => {
                self.settings.cycle(setting, right);
                self.settings.save_to(&self.settings_path)?;
                if let Some(r) = &mut self.renderer {
                    r.apply_settings(&self.settings);
                }
                if setting == Setting::Fullscreen {
                    self.window
                        .as_ref()
                        .unwrap()
                        .set_fullscreen(if self.settings.fullscreen {
                            Some(Fullscreen::Borderless(None))
                        } else {
                            None
                        });
                }
                if let Some(g) = &mut self.game {
                    if matches!(
                        setting,
                        Setting::Graphics
                            | Setting::Lighting
                            | Setting::Alternatives
                            | Setting::Greedy
                    ) {
                        g.dirty.extend(g.world.chunks.keys());
                    }
                    if setting == Setting::Distance {
                        g.stream_center = None;
                        g.attempted.clear();
                    }
                }
            }
            Action::Language(code) => {
                self.settings.locale_unicode = self.resources.unicode(&code);
                self.settings.language = code;
                self.send_client_settings()?;
                self.settings.save_to(&self.settings_path)?;
            }
            Action::Slot(index) => {
                let slot = if index >= 100 {
                    SlotRef::Craft(index - 100)
                } else {
                    SlotRef::Player(index)
                };
                self.window_click(slot, self.pickup_action(right))?;
            }
            Action::ContainerSlot(index) => {
                self.window_click(SlotRef::Container(index), self.pickup_action(right))?
            }
            Action::Craft => self.window_click(SlotRef::CraftOutput, self.pickup_action(right))?,
            Action::Creative(_) => {} // Creative mouse actions also handle right/middle buttons and tab release.
            Action::SelectTrade(direction) => {
                if let Some(g) = &mut self.game {
                    if let Some(trader) = self.gui.trader.and_then(|id| g.sim.trades.get_mut(&id)) {
                        let selected = (self.gui.trade_index as i32 + direction)
                            .clamp(0, trader.offers.len().saturating_sub(1) as i32);
                        self.gui.trade_index = selected as usize;
                        g.sim.merchant_window.select(selected, trader);
                        if let Some(r)=&mut g.remote{r.queue_tick_command(crate::network::Command::SelectTrade(selected))?;}
                    }
                }
            }
            Action::Respawn => {
                self.sp_chest_lids.clear();
                if let Some(g) = &mut self.game {
                    if let Some(remote) = &g.remote {
                        remote.client.send(crate::network::Command::Respawn)?;
                        self.open(Screen::Playing);
                        return Ok(());
                    }
                    if g.sim.dimension != 0 {
                        let (ow_center, fallback) = g.overworld_respawn_centers();
                        g.switch_dimension_region(
                            0,
                            ow_center,
                            2,
                            self.renderer.as_mut(),
                            &self.assets,
                            &self.settings,
                        )?;
                        if let Some(fallback_center) = fallback {
                            if g.sim.bed_spawn.is_some_and(|bed| bed.exit(&g.world).is_none()) {
                                g.ensure_overworld_region(fallback_center, 2)?;
                            }
                        }
                        g.sim.respawn(&mut g.player, &g.world);
                        let [px, _, pz] = g.player.precise_position();
                        let dest_center = (
                            (px.floor() as i32).div_euclid(16),
                            (pz.floor() as i32).div_euclid(16),
                        );
                        g.mesh_arrival_chunks(
                            dest_center,
                            self.renderer.as_mut(),
                            &self.assets,
                            &self.settings,
                        );
                        g.stream_center = None;
                    } else {
                        let (ow_center, fallback) = g.overworld_respawn_centers();
                        let mut loaded_any = g.ensure_overworld_region(ow_center, 2)?;
                        if let Some(fallback_center) = fallback {
                            if g.sim.bed_spawn.is_some_and(|bed| bed.exit(&g.world).is_none()) {
                                loaded_any |= g.ensure_overworld_region(fallback_center, 2)?;
                            }
                        }
                        g.sim.respawn(&mut g.player, &g.world);
                        if loaded_any {
                            let [px, _, pz] = g.player.precise_position();
                            let dest_center = (
                                (px.floor() as i32).div_euclid(16),
                                (pz.floor() as i32).div_euclid(16),
                            );
                            g.mesh_arrival_chunks(
                                dest_center,
                                self.renderer.as_mut(),
                                &self.assets,
                                &self.settings,
                            );
                        }
                        g.stream_center = None;
                    }
                }
                self.open(Screen::Playing);
            }
            Action::LeaveBed => {
                if let Some(g) = &mut self.game {
                    if let Some(remote)=&mut g.remote{
                        remote.queue_tick_command(crate::network::Command::EntityAction{action:crate::network::EntityAction::LeaveBed,parameter:0})?;
                        return Ok(());
                    }else if let Some(p) = g.sim.wake(&mut g.world, &mut g.player, false, true) {
                        g.mark(p);
                    }
                }
                self.open(Screen::Playing);
            }
        }
        Ok(())
    }
    fn perform_portal_travel(&mut self, travel: gameplay::PortalTravel) -> Result<()> {
        self.sp_chest_lids.clear();
        let Some(g) = &mut self.game else {
            return Ok(());
        };
        if g.remote.is_some() {
            return Ok(());
        }
        match travel {
            gameplay::PortalTravel::Nether(entry) => {
                let cur_dim = g.sim.dimension;
                let target_dim = if cur_dim == -1 { 0 } else { -1 };
                let (min_x, max_x, min_z, max_z) = if target_dim == -1 {
                    let mut nether_border = g.sim.world_border.clone();
                    nether_border.center =
                        [nether_border.center[0] / 8.0, nether_border.center[1] / 8.0];
                    let [min_x, min_z, max_x, max_z] =
                        nether_border.bounds(crate::vanilla::click_border::now());
                    (min_x, max_x, min_z, max_z)
                } else {
                    let [min_x, min_z, max_x, max_z] =
                        g.sim.world_border.bounds(crate::vanilla::click_border::now());
                    (min_x, max_x, min_z, max_z)
                };
                let (target_feet, tx, tz) = crate::dimension_world::resolve_nether_portal_target(
                    cur_dim,
                    target_dim,
                    entry.trigger_pos,
                    min_x,
                    max_x,
                    min_z,
                    max_z,
                );
                let game_key = g as *const Game as usize;
                let saved_edits = g.sim.saved_edits_for_dimension(&g.world, target_dim);
                let total_ticks = g.sim.total_ticks;
                let known_portal =
                    with_cached_dimension_chunks(game_key, target_dim, |cached_chunks| {
                        let (cache, _) = g.sim.teleporter_cache_and_rng(target_dim);
                        crate::dimension_world::find_existing_portal_fast(
                            target_dim,
                            target_feet,
                            tx,
                            tz,
                            total_ticks,
                            cache,
                            &saved_edits,
                            cached_chunks,
                            &g.options,
                        )
                    });
                let (arrival_center, arrival_radius) = if let Some(p) = known_portal {
                    ((p.x.div_euclid(16), p.z.div_euclid(16)), 2)
                } else {
                    ((tx.div_euclid(16), tz.div_euclid(16)), 3)
                };
                g.switch_dimension_region(
                    target_dim,
                    arrival_center,
                    arrival_radius,
                    self.renderer.as_mut(),
                    &self.assets,
                    &self.settings,
                )?;
                let total_ticks = g.sim.total_ticks;
                let (dest_feet, dest_yaw, dest_vel, changed) = {
                    let (cache, rng_state) = g.sim.teleporter_cache_and_rng(target_dim);
                    let mut jrng = crate::vanilla::random::JavaRandom::from_state(*rng_state);
                    let res = crate::dimension_world::place_or_make_nether_portal(
                        &mut g.world,
                        target_dim,
                        &entry,
                        target_feet,
                        tx,
                        tz,
                        known_portal,
                        total_ticks,
                        cache,
                        &mut jrng,
                    );
                    *rng_state = jrng.state();
                    res
                };
                for &pos in &changed {
                    g.sim.block_ticks.notify(&g.world, pos);
                    g.mark(pos);
                }
                if target_dim == 0 && !changed.is_empty() {
                    if let Some(host) = &mut g.population {
                        host.flush_overworld_edits(&g.world)?;
                    }
                }
                g.player.reset_portal_travel();
                g.player
                    .set_pose_f64(dest_feet.to_array(), dest_yaw, entry.pitch, false);
                g.player.set_velocity_f64(dest_vel.to_array());
                g.player.portal_cooldown = 10;
                let dest_center = (
                    (dest_feet.x.floor() as i32).div_euclid(16),
                    (dest_feet.z.floor() as i32).div_euclid(16),
                );
                g.mesh_arrival_chunks(
                    dest_center,
                    self.renderer.as_mut(),
                    &self.assets,
                    &self.settings,
                );
                self.audio
                    .play_pitched(&self.resources, "portal.travel", 0.25, 0.9);
            }
            gameplay::PortalTravel::End => {
                if g.sim.dimension != 1 {
                    g.switch_dimension_region(
                        1,
                        (6, 0),
                        2,
                        self.renderer.as_mut(),
                        &self.assets,
                        &self.settings,
                    )?;
                    let (dest_feet, dest_yaw, changed) =
                        crate::dimension_world::build_end_obsidian_platform(&mut g.world);
                    for &pos in &changed {
                        g.sim.block_ticks.notify(&g.world, pos);
                        g.mark(pos);
                    }
                    g.player.reset_portal_travel();
                    g.player
                        .set_pose_f64(dest_feet.to_array(), dest_yaw, 0.0, true);
                    g.player.portal_cooldown = 10;
                    g.mesh_arrival_chunks(
                        (6, 0),
                        self.renderer.as_mut(),
                        &self.assets,
                        &self.settings,
                    );
                    self.audio
                        .play_pitched(&self.resources, "portal.travel", 0.25, 0.9);
                } else {
                    let (ow_center, fallback) = g.overworld_respawn_centers();
                    g.switch_dimension_region(
                        0,
                        ow_center,
                        2,
                        self.renderer.as_mut(),
                        &self.assets,
                        &self.settings,
                    )?;
                    if let Some(fallback_center) = fallback {
                        if g.sim.bed_spawn.is_some_and(|bed| bed.exit(&g.world).is_none()) {
                            g.ensure_overworld_region(fallback_center, 2)?;
                        }
                    }
                    g.sim.return_from_end_to_overworld(&mut g.world, &mut g.player);
                    g.player.portal_cooldown = 10;
                    let [px, _, pz] = g.player.precise_position();
                    let dest_center = (
                        (px.floor() as i32).div_euclid(16),
                        (pz.floor() as i32).div_euclid(16),
                    );
                    g.mesh_arrival_chunks(
                        dest_center,
                        self.renderer.as_mut(),
                        &self.assets,
                        &self.settings,
                    );
                    self.audio
                        .play_pitched(&self.resources, "portal.travel", 0.25, 0.9);
                }
            }
        }
        Ok(())
    }
    pub(super) fn apply_video_effect(&mut self, effect: ui::sodium_video::Effect) -> Result<()> {
        match effect {
            ui::sodium_video::Effect::None => {}
            ui::sodium_video::Effect::PlayClick => self.audio.play_ui(&self.resources),
            ui::sodium_video::Effect::OpenUrl(url) => Self::visit_link(url)?,
            ui::sodium_video::Effect::Close => {
                let back = self.gui.back();
                self.open(back);
            }
            ui::sodium_video::Effect::Apply(staged) => {
                self.audio.play_ui(&self.resources);
                let prev = self.settings.clone();
                self.settings = *staged;
                self.settings.validate();
                self.settings.save_to(&self.settings_path)?;
                if let Some(r) = &mut self.renderer {
                    r.apply_settings(&self.settings);
                }
                if self.settings.fullscreen != prev.fullscreen && self.options.screenshot.is_none()
                {
                    if let Some(w) = &self.window {
                        w.set_fullscreen(if self.settings.fullscreen {
                            Some(Fullscreen::Borderless(None))
                        } else {
                            None
                        });
                    }
                }
                if let Some(g) = &mut self.game {
                    if self.settings.fancy != prev.fancy
                        || self.settings.smooth_lighting != prev.smooth_lighting
                        || self.settings.alternate_blocks != prev.alternate_blocks
                        || self.settings.greedy_meshing != prev.greedy_meshing
                    {
                        g.dirty.extend(g.world.chunks.keys());
                    }
                    if self.settings.render_distance != prev.render_distance {
                        g.stream_center = None;
                        g.attempted.clear();
                    }
                }
                if self.settings.render_distance != prev.render_distance {
                    self.send_client_settings()?;
                }
            }
        }
        Ok(())
    }
    fn finish_benchmark(&self) -> Result<()> {
        anyhow::ensure!(!self.samples.is_empty(), "No benchmark frames");
        let sum = self.samples.iter().sum::<f64>();
        let avg = self.samples.len() as f64 / sum;
        let mut sorted = self.samples.clone();
        sorted.sort_by(f64::total_cmp);
        let slow = &sorted[sorted.len() - (sorted.len() as f64 * 0.01).ceil() as usize..];
        let low = slow.len() as f64 / slow.iter().sum::<f64>();
        let percentile = |p: f64| sorted[((sorted.len() - 1) as f64 * p) as usize] * 1000.0;
        let r = self.renderer.as_ref().unwrap();
        let scene = self.options.benchmark_scene.as_ref();
        let mut report = serde_json::json!({
            "kind": "native_window_frame_intervals",
            "version": env!("CARGO_PKG_VERSION"),
            "gpu": r.adapter_name,
            "resolution_pixels": r.resolution(),
            "backend": "Vulkan",
            "present_mode": r.present_mode,
            "settings": self.settings,
            "scene": scene,
            "world_entry": self.entry_report,
            "streaming_flight_blocks_per_second": self.options.benchmark_streaming.then_some(12),
            "chunk_phase_calls_total_ms_max_ms": self.game.as_ref().map(|g| &g.chunk_timings),
            "presentation_copy_samples_total_count_max_count": self.game.as_ref().map(|g| &g.presentation_copy_counts),
            "chunk_phase_window": "Includes startup and warmup; slow_present_intervals uses the measured frame window",
            "native_publication_work": self.game.as_ref().and_then(|g|g.population.as_ref()).map(|p|p.publication_work()),
            "retained_native_chunks": self.game.as_ref().and_then(|g|g.population.as_ref()).map(|p|p.retained_native_chunks()),
            "native_light_maintenance": self.game.as_ref().and_then(|g|g.population.as_ref()).map(|p|p.native_light_maintenance()),
            "native_light_maintenance_scope": "Cumulative owner work after first presented play; each pass is visible in the next ordinary publication. Counts include warmup. Payload release counters are cumulative, not process RSS or current retained bytes. The light2ms plus block1ms wall caps are cooperative; nested block_storage reports its own progress/rejection/budget counters.",
            "simulation_phases": self.game.as_ref().map(|g| g.sim.commands.profile_report()),
            "distant_terrain":self.game.as_ref().and_then(|g| g.distant.as_ref()).map(|d| d.report()),
            "distant_gpu_tiles_visible_allocated_bytes":r.distant_stats(),
            "terrain_gpu_allocations": r.terrain_allocation_report(),
            "full_detail_radius_chunks":self.settings.full_detail_distance(),
            "world_directory": self.options.world.as_ref().map(std::fs::canonicalize).transpose()?,
            "frames": sorted.len(),
            "measured_seconds": sum,
            "average_fps": avg,
            "one_percent_low_fps": low,
            "p50_frame_ms": percentile(0.5),
            "p95_frame_ms": percentile(0.95),
            "p99_frame_ms": percentile(0.99),
            "frame_seconds": self.samples,
            "max_work_ms": {
                "simulation_and_streaming": self.timing_max_ms[0],
                "gui": self.timing_max_ms[1],
                "render_and_present": self.timing_max_ms[2]
            },
            "max_render_cpu_ms": {
                "surface_acquire": self.render_timing_max_ms[0],
                "encode": self.render_timing_max_ms[1],
                "finish_and_submit": self.render_timing_max_ms[2],
                "present": self.render_timing_max_ms[3]
            },
            "slow_render_frames": r.frame_profile_report(),
            "slow_present_intervals": self.interval_profile.report(),
            "render_model_cache": crate::remote_mobs::render_model_cache_report(),
            "entities": r.total_entities,
            "loaded_chunks": self.game.as_ref().map(|g| g.world.chunks.len()),
            "visible_chunks": r.visible_chunks,
            "triangles": r.triangles,
            "note": "Wall time between successful window presents after warm-up. Compare only runs of the same scene, adapter and settings."
        });
        // Build separately from the large frame report to keep json! expansion bounded.
        report["streaming_flight_route"] = if self.options.benchmark_streaming { serde_json::json!({
            "kind": if self.options.benchmark_streaming_return_after.is_some() { "out_and_back" } else { "forward" },
            "turnaround_after_measured_seconds": self.options.benchmark_streaming_return_after,
            "measured_anchor_player_xz": self.benchmark_streaming_origin.map(|p| [p[0], p[2]]),
            "final_player_xz": self.game.as_ref().map(|g| { let p = g.player.precise_position(); [p[0], p[2]] }),
            "height_above_spawn_blocks": 48,
            "outbound_yaw_degrees": -90,
            "return_yaw_degrees": self.options.benchmark_streaming_return_after.map(|_| 90),
            "turnaround": "Opt-in return turns yaw instantly by 180 degrees on the first camera update at/after the requested measured time; that frame is included in all samples.",
            "clock": "Return timing starts at the first successful measured present, after unchanged forward warmup. Default forward flight uses the original warmup-inclusive clock.",
            "after_return": "Opt-in return holds the measured anchor after twice the requested turnaround time; no teleport or world edit is added.",
        }) } else { serde_json::Value::Null };
        report["worker_thread_priority"]=crate::worker_priority::report();
        let output = self.options.benchmark_output_dir.as_deref().unwrap_or_else(|| std::path::Path::new("reports"));
        std::fs::create_dir_all(output)?;
        let path = output.join(if scene.is_some() {
            "benchmark-native-scene.json"
        } else {
            "benchmark.json"
        });
        report["transparency_capture"]=r.transparency_capture_report();
        report["transparency_sort_batch"]=r.transparency_sort_report();
        report["near_opaque_shadow"]=r.near_opaque_shadow_report();
        report["upload_capture"]=r.upload_capture_report();
        report["near_arena"]=r.near_arena_report();
        report["surface_configuration"]=r.surface_configuration_report();
        report["arena_upload_capture"]=r.arena_upload_capture_report();
        report["gpu_timestamps"]=r.gpu_timestamps_report();
        report["surface_wait_trace"]=r.surface_wait_trace_report();
        report["pre_acquire_visibility"]=r.pre_acquire_visibility_report();
        report["packed_frustum"]=r.packed_frustum_report();
        report["covered_lod_culling"]=r.covered_lod_culling_report();
        report["transparent_index_upload"]=r.transparent_staging_report();
        report["translucent_draw"]=r.translucent_mdi_report();
        report["diagnostic_not_baseline"]=serde_json::json!(self.options.benchmark_capture_transparency
            ||self.options.benchmark_capture_near_opaque||self.options.benchmark_capture_uploads||self.options.benchmark_capture_arena_uploads
            ||self.options.benchmark_gpu_timestamps||self.options.benchmark_surface_waits);
        report["near_opaque_runs"]=r.near_opaque_runs_report();
        let slow_frame_threshold = self.options.benchmark_slow_frame_ms.unwrap_or_default();
        report["slow_frame_capture"] = serde_json::json!({
            "requested_threshold_ms": self.options.benchmark_slow_frame_ms.map(|v| v.get()),
            "effective_threshold_ms": slow_frame_threshold.get(), "default_threshold_ms": 8.,
            "retained_limit_per_recorder": 32, "inclusive": true,
            "nondefault_diagnostic": slow_frame_threshold.changes_default(),
            "note": "Only bounded slow-record admission changes in BOTH existing CPU recorders. No clocks, present anchors, frame_seconds, successful-frame totals, graphics, route, arena selection or warmup boundaries change. Lower thresholds add bounded retained-record bookkeeping; treat a nondefault run as diagnostic, not a clean FPS control."
        });
        if slow_frame_threshold.changes_default() { report["diagnostic_not_baseline"] = serde_json::json!(true); }
        std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
        r.write_transparency_capture(&output.join("transparency-inputs.bin"))?;
        println!("Benchmark: {avg:.1} FPS average, {low:.1} 1% low");
        Ok(())
    }
    fn frame(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let now = Instant::now();
        self.service_local_ticks(event_loop, now, interval_profile::Origin::Frame)?;
        if event_loop.exiting() { return Ok(()); }
        if self.gui.screen == Screen::Multiplayer {
            self.gui.multiplayer.poll_lan();
        }
        let simulation_active = self.simulation_active();
        if !self.options.benchmark {
            self.audio.set_levels(self.settings.volume, &self.settings.menu.sound_levels);
            let mode = crate::audio::MusicMode::for_world(self.game.as_ref().map(|g| (g.sim.dimension, g.sim.mode == Mode::Creative)));
            self.audio.music(&self.resources, mode, self.game.is_some() && !simulation_active);
        }
        let dt = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;
        if self.gui.screen == Screen::Playing
            && !self.options.benchmark
            && self.options.screenshot.is_none()
            && self.game.is_some()
        {
            let dx_px = self.mouse_accum[0].trunc() as i32;
            let dy_px = self.mouse_accum[1].trunc() as i32;
            self.mouse_accum[0] -= f64::from(dx_px);
            self.mouse_accum[1] -= f64::from(dy_px);
            if dx_px != 0 || dy_px != 0 {
                let f2 = self.settings.sensitivity * 0.6_f32 + 0.2_f32;
                let f3 = f2 * f2 * f2 * 8.0_f32;
                let sign = if self.settings.invert_mouse { -1.0_f32 } else { 1.0_f32 };
                let dx = (dx_px as f32) * f3;
                let dy = (dy_px as f32) * f3 * sign;
                if self.smooth_camera.enabled {
                    self.smooth_camera.pending[0] += dx;
                    self.smooth_camera.pending[1] += dy;
                } else {
                    self.look_local(f64::from(dx), f64::from(dy));
                }
            }
        } else {
            self.mouse_accum = [0.0; 2];
        }
        if let Some(updates) = &self.load_updates {
            if let Some(value) = updates.try_iter().last() {
                self.gui.loading_progress = Some(value);
            }
        }
        if let Some(rx) = &self.loading {
            match rx.try_recv() {
                Ok(result) => {
                    self.loading = None;
                    self.load_updates = None;
                    match result {
                        Ok(g) => {
                            self.entry_data_ready_ms = self.entry_started.map(|at| at.elapsed().as_secs_f64()*1000.);
                            if let Some(level) = g.sim.world_difficulty { self.settings.difficulty = level; }
                            self.game = Some(g);
                            // Time spent in menus/loading is not gameplay time.
                            let reset_now = Instant::now();
                            self.last_save = reset_now;
                            self.last_tick_pump = reset_now;
                            self.accumulator = 0.0;
                        }
                        Err(e) if self.options.benchmark => return Err(e),
                        Err(e) => {
                            self.enter_when_ready = false;
                            if self.connecting{self.open(Screen::Multiplayer);self.gui.multiplayer.connection_error(format!("{e:#}"));self.connecting=false;}
                            else{self.open(Screen::Worlds);self.gui.status = format!("Load failed: {e}");}
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.loading = None;
                    self.load_updates = None;
                    self.enter_when_ready = false;
                    self.open(Screen::Worlds);
                    self.gui.status = "Loading worker stopped".into();
                }
            }
        }
        let mut connection_error = None;
        // Native/light publication mutates authority and therefore waits until
        // the ordered tick loan has returned; drawing never waits for it.
        if !self.tick_busy() && self.local_tick_failure.is_none() {
        if let Some(g) = &mut self.game {
            let chunk_profile = if self.options.benchmark_streaming {
                self.interval_profile.begin_chunk_update(&g.chunk_timings)
            } else { None };
            let chunk_result = g.update_chunks(
                self.renderer.as_mut().unwrap(),
                &self.assets,
                &self.settings,
            );
            self.interval_profile.end_chunk_update(chunk_profile, &g.chunk_timings);
            if let Err(error) = chunk_result {
                if g.remote.is_some() {
                    connection_error = Some(format!("{error:#}"));
                } else {
                    return Err(error);
                }
            }
            if self.enter_when_ready {
                let ow_native = g.sim.dimension == 0 && g.population.is_some();
                if !g.world.chunks.is_empty() {
                    let [x, _, z] = g.player.precise_position();
                    let center = ((x.floor() as i32) >> 4, (z.floor() as i32) >> 4);
                    let nearby = mesh_neighbors(center).into_iter().filter(|p| ow_native || g.world.chunks.contains_key(p)).count().max(1);
                    self.gui.loading_progress = Some((80 + 20 * nearby.saturating_sub(g.entry_pending.len()) / nearby).min(100) as u8);
                }
                if self.loading.is_none()
                    && if let Some(r) = g.remote.as_ref() {
                        r.ready
                    } else {
                        let [x, _, z] = g.player.precise_position();
                        entry_meshes_ready(
                            &g.world,
                            ((x.floor() as i32) >> 4, (z.floor() as i32) >> 4),
                            &g.entry_pending,
                            ow_native,
                        )
                    }
                {
                    self.enter_when_ready = false;
                    self.connecting = false;
                    g.entry_uploaded.clear();
                    let dead = g.sim.health <= 0.0;
                    self.open(if dead { Screen::Death } else { Screen::Playing });
                }
            }
        }
        }
        if let Some(error) = connection_error {
            if self.options.connect.is_some() && self.options.screenshot.is_some(){anyhow::bail!("Remote screenshot: {error}");}
            self.save_quit()?;
            self.open(Screen::Multiplayer);self.gui.multiplayer.connection_error(error);
            return Ok(());
        }
        if let Some(g) = &mut self.game {
            let distant_profile = if self.options.benchmark_streaming { self.interval_profile.timer() } else { None };
            let distant_result = g.update_distant(self.renderer.as_mut().unwrap(), &self.assets, &self.settings);
            self.interval_profile.end_distant_update(distant_profile);
            distant_result?;
        }
        if !self.tick_busy() && !self.options.benchmark && self.options.screenshot.is_none() {
            if let Some(g) = &mut self.game {
                if let Some(slim) = g.sim.presentation.local_slim() {
                    g.player.third_person.slim = slim;
                }
            }
        }
        if !self.tick_busy() && self.game.as_ref().is_some_and(|g| match self.gui.screen {
            Screen::Sleeping => g.sim.sleep.head.is_none(),
            Screen::Trading if g.remote.is_none() => self.gui.trader.is_none_or(|id| {
                g.sim.trading_with != Some(id)
                    || g.sim
                        .trades
                        .get(&id)
                        .is_none_or(|trader| trader.customer.is_none())
                    || !g.sim.mobs.iter().any(|mob| mob.id == id && mob.health > 0.)
            }),
            _ => false,
        }) {
            self.open(Screen::Playing);
        }
        if self.smooth_camera.enabled && self.gui.screen == Screen::Playing {
            let [x,y] = self.smooth_camera.sample((self.accumulator/0.05).min(1.));
            self.look_local(f64::from(x), f64::from(y));
        }
        // Present the real entry pose before the benchmark teleports the camera
        // onto its flight path. Entry timing must describe the uploaded 3x3.
        let benchmark = self.options.benchmark && self.gui.screen == Screen::Playing
            && self.entry_report.is_some()
            && (self.benchmark_start.is_some() || !self.options.benchmark_distant_ready
                || self.game.as_ref().and_then(|g| g.distant.as_ref()).is_some_and(|d| d.ready()));
        let check_scene =
            benchmark && self.benchmark_start.is_none() && self.options.benchmark_scene.is_some();
        let warmup = self
            .options
            .benchmark_scene
            .as_ref()
            .map_or(2.0, |scene| scene.warmup_seconds);
        if benchmark {
            let started = *self.benchmark_start.get_or_insert(now);
            let elapsed = started.elapsed().as_secs_f32();
            if let (Some(scene), Some(g)) = (&self.options.benchmark_scene, &mut self.game) {
                g.player.set_pose_f64(
                    scene.player_feet,
                    scene.yaw_degrees,
                    scene.pitch_degrees,
                    true,
                );
            } else if let Some(g) = &mut self.game {
                g.player.flying = true;
                if self.options.benchmark_streaming {
                    if let (Some(turn_after), Some(origin), Some(sample_start)) = (
                        self.options.benchmark_streaming_return_after,
                        self.benchmark_streaming_origin,
                        self.benchmark_sample_start,
                    ) {
                        let (at, yaw) = crate::benchmark_streaming::out_and_back(
                            origin, sample_start.elapsed().as_secs_f64(), f64::from(turn_after),
                        );
                        g.player.set_pose_f64(at, yaw, 15., false);
                    } else {
                        let at = g.world.spawn + Vec3::new(elapsed * 12., 48., 0.);
                        g.player.set_pose_f64(at.as_dvec3().to_array(), -90., 15., false);
                    }
                } else {
                let angle = elapsed * 0.15;
                g.player.position =
                    g.world.spawn + Vec3::new(angle.sin() * 25.0, 26.0, angle.cos() * 25.0);
                g.player.previous = g.player.position;
                let look = (g.world.spawn + Vec3::Y * 5.0 - g.player.position).normalize();
                g.player.yaw = look.x.atan2(-look.z);
                g.player.pitch = look.y.asin();
                }
            }
        }
        self.stats_frames += 1;
        self.stats_elapsed += dt;
        if now.duration_since(self.last_stats) > Duration::from_millis(500) {
            self.fps = self.stats_frames as f32 / self.stats_elapsed;
            self.frame_ms = self.stats_elapsed / self.stats_frames as f32 * 1000.0;
            self.stats_frames = 0;
            self.stats_elapsed = 0.0;
            self.last_stats = now;
            if let Some(w) = self.window.as_ref() {
                let s = w.inner_size();
                if s.width > 0 && s.height > 0 {
                    let title = format!("Ferrum 1.8.9 | Vulkan | {:.0} FPS", self.fps);
                    if title != self.last_window_title {
                        w.set_title(&title);
                        self.last_window_title = title;
                    }
                }
            }
        }
        if !self.tick_busy() && self.local_tick_failure.is_none() && self.gui.screen == Screen::Playing && self.last_save.elapsed() > Duration::from_secs(30)
        {
            if let Some(g) = &mut self.game {
                if g.autosave()? { self.last_save = Instant::now(); }
            }
        }
        if self.window.as_ref().is_some_and(|w| {
            let s = w.inner_size();
            s.width == 0 || s.height == 0
        }) {
            return Ok(());
        }
        if self.gui.screen==Screen::Playing&&self.game.as_ref().is_some_and(|g|g.remote.is_some()&&g.player.sleeping){self.open(Screen::Sleeping);}
        if let Some(result)=self.downloading_pack.as_ref().and_then(|(_,rx)|match rx.try_recv(){Ok(r)=>Some(r),Err(mpsc::TryRecvError::Disconnected)=>Some(Err(anyhow::anyhow!("Pack download interrupted"))),_=>None}){
            let(hash,_)=self.downloading_pack.take().unwrap();
            let applied=result.and_then(|path|{
                let previous=self.server_pack.replace(path);
                if let Err(e)=self.reload_packs(&self.settings.menu.resource_packs.clone()){self.server_pack=previous;return Err(e)}
                Ok(())
            });
            if let Some(remote)=self.game.as_ref().and_then(|g|g.remote.as_ref()){
                remote.client.send(crate::network::Command::PackStatus(hash,if applied.is_ok(){0}else{2}))?;
            }
            if let Err(e)=applied{self.gui.chat.receive(&serde_json::json!({"text":format!("Resource pack: {e}")}).to_string(),1,&self.resources,&self.settings.language);}
        }
        if self.requested_pack.is_none()&&self.downloading_pack.is_none(){
            if let Some((request,address))=self.game.as_mut().and_then(|g|Some((g.remote.as_mut()?.extra.packs.pop_front()?,g.options.connect.as_deref()?))){
                use ui::multiplayer::ResourceMode;
                let mode=self.gui.multiplayer.pack_mode(address);self.requested_pack=Some(request);
                if mode==ResourceMode::Prompt{self.open(Screen::ServerPack);}else{self.action(Action::ServerPack(mode==ResourceMode::Enabled),false)?;}
            }
        }
        if !self.tick_busy() && self.game.as_mut().is_some_and(|g| g.remote.is_none() && std::mem::take(&mut g.sim.show_horse_window)) {
            if let Some(g) = &mut self.game {
                g.sim.minecart_window = None;
            }
            self.gui.container = None;
            self.open(Screen::Chest);
        }
        if !self.tick_busy() && self.game.as_mut().is_some_and(|g| g.remote.is_none() && std::mem::take(&mut g.sim.show_minecart_window)) {
            if let Some(g) = &mut self.game {
                g.sim.horse_window = None;
            }
            self.gui.container = None;
            self.open(Screen::Chest);
        }
        let remote_window=self.game.as_ref().and_then(|g|{let r=g.remote.as_ref()?;let open=r.extra.opens?;Some(if open{r.extra.window.as_ref()?.screen()}else{Screen::Playing})});
        if let Some(screen)=remote_window{
            self.open(screen);
            if let Some(remote)=self.game.as_mut().and_then(|g|g.remote.as_mut()){
                remote.extra.opens=None;
                self.gui.container_title=remote.extra.window.as_ref().map(|w|ui::chat::components::formatted(&ui::chat::components::parse(&w.title,|key|self.resources.tr(&self.settings.language,key)))).filter(|s|!s.is_empty());
                if screen==Screen::Trading {self.gui.trader=Some(crate::remote::REMOTE_TRADER);self.gui.trade_index=0;}
            }
        }
        if let Some(book)=self.game.as_mut().and_then(|g|{
            if !std::mem::take(&mut g.remote.as_mut()?.extra.book){return None}
            ui::server_book::Book::from_stack(g.sim.inventory.held().as_ref()?)
        }){self.gui.book=book;self.gui.book.author.clone_from(&self.game.as_ref().unwrap().options.username);self.open(Screen::Book);}
        if let Some((pos,lines))=self.game.as_mut().and_then(|g|g.remote.as_mut()?.extra.sign.take()){
            self.gui.sign=ui::sign::Editor{pos:pos.to_array(),lines,row:0};self.open(Screen::Sign);
        } else if let Some(pos) = (!self.tick_busy())
            .then(|| self.game.as_mut())
            .flatten()
            .filter(|g| g.remote.is_none())
            .and_then(|g| g.sim.pending_sign_edit.take())
        {
            self.gui.sign = ui::sign::Editor {
                pos: pos.to_array(),
                lines: Default::default(),
                row: 0,
            };
            self.open(Screen::Sign);
        }
        if matches!(self.gui.screen, Screen::Chest | Screen::Furnace) {
            if let Some(r)=self.game.as_mut().and_then(|g|g.remote.as_mut()){
                if let Some(w)=&r.extra.window{
                    if let Some(rename)=self.gui.special.sync(&w.kind,w.container.slots.first().and_then(Option::as_ref),&w.properties,&self.resources,&self.settings.language){
                        r.rename_anvil(rename)?;
                    }
                }
            }
            self.gui.enchanting=self.game.as_ref().and_then(|g|{
                let w=g.remote.as_ref()?.extra.window.as_ref().filter(|w|w.kind=="minecraft:enchanting_table")?;
                Some(std::array::from_fn(|i|w.properties.get(&(i as i16)).copied().unwrap_or(0)))
            });
            self.gui.container_view = self.game.as_ref().and_then(|g| {
                if let Some(r)=&g.remote{return r.current_window_view();}
                if g.sim.horse_window.is_some() {
                    return g.sim.horse_window_view(&g.player);
                }
                if g.sim.minecart_window.is_some() {
                    return g.sim.minecart_window_view(&g.player);
                }
                self.gui
                    .container
                    .and_then(|pos| g.sim.container_view(&g.world, pos))
            });
            if self.gui.container_view.is_none() {
                self.open(Screen::Playing);
            }
        }
        if let Some(g) = &mut self.game {
            self.gui.option_state.hardcore = g.sim.hardcore;
            self.gui.option_state.difficulty_locked = g.sim.difficulty_locked;
            self.gui.option_state.remote = g.remote.is_some();
            self.gui.spectator.set_spectator(g.player.is_spectator());
            g.sim.server_scene.hide_hud = self.gui.hide_hud;
            let cam = g.remote.as_ref().and_then(|r| r.camera_player());
            let ear = cam.as_ref().unwrap_or(&g.player);
            self.audio.listener(ear.precise_eye(1.0), ear.yaw);
            if let Some(remote) = &mut g.remote {
                for (sound, volume, pitch, source) in remote.visuals.sounds.drain(..) {
                    self.audio
                        .play_world(&self.resources, &sound, volume, pitch, source);
                }
                if matches!(self.gui.screen,Screen::Statistics|Screen::Achievements) {
                    self.gui.statistics.update(remote.extra.stats_revision,&remote.extra.stats);
                }
                for (line, position) in remote.chat_lines.drain(..) {
                    self.gui.chat.receive(&line,position,&self.resources,&self.settings.language);
                }
                for choices in remote.tab_completions.drain(..) {
                    if matches!(self.gui.screen, Screen::Chat | Screen::Sleeping) {self.gui.chat.complete_with(choices);}
                }
                if self.gui.chat.tab_open {
                    self.gui.chat.tab_names = remote.names.values().cloned().collect();
                    self.gui.chat.tab_names.sort();
                }
            }
        }
        let before_gui = Instant::now();
        let waiting_for_tick = self.tick_busy();
        let renderer = self.renderer.as_mut().unwrap();
        let alpha = if waiting_for_tick {
            // The mirror still contains the preceding completed tick. Its
            // previous pose must not be replayed when submission resets time.
            1.0
        } else if (simulation_active || self.gui.screen == Screen::Death) && !self.options.benchmark {
            (self.accumulator / 0.05).clamp(0., 1.)
        } else {
            1.0
        };
        let server_camera=self.game.as_ref().and_then(|g|g.remote.as_ref()?.camera_player());
        if let Some(g)=self.game.as_ref(){
            let camera=server_camera.as_ref().unwrap_or(&g.player);
            self.audio.listener(camera.precise_eye(alpha), camera.yaw);
            let observed=renderer.boss(&g.world,camera,&g.sim,&self.settings,self.gui.perspective,alpha);
            self.gui.boss_bar.frame(observed.as_ref(),!self.gui.hide_hud || self.gui.screen!=Screen::Playing);
        }else{self.gui.boss_bar=Default::default();}
        let stats = Stats {
            fps: self.fps,
            frame_ms: self.frame_ms,
            chunks: renderer.visible_chunks,
            total_chunks: self.game.as_ref().map_or(0, |g| g.world.chunks.len()),
            distant_tiles: self.game.as_ref().and_then(|g| g.distant.as_ref()).map_or((0,0), |d| d.progress()),
            triangles: renderer.triangles,
            entities: renderer.visible_entities,
            total_entities: renderer.total_entities,
            culled: renderer.culled_entities,
            position: self
                .game
                .as_ref()
                .map_or([0.0; 3], |g| g.player.precise_position()),
            yaw: self.game.as_ref().map_or(0.0, |g| g.player.yaw),
            spawn: self.game.as_ref().map_or([0.;3],|g|g.world.spawn.to_array()),
            pitch: self.game.as_ref().map_or(0.0, |g| g.player.pitch),
            gpu: &renderer.adapter_name,
            time: self.started.elapsed().as_secs_f32(),
            portal_time: self.game.as_ref().map_or(0.0, |g| {
                g.player.prev_time_in_portal
                    + (g.player.time_in_portal - g.player.prev_time_in_portal) * alpha
            }),
        };
        if let Some(remote) = self.game.as_ref().and_then(|g| g.remote.as_ref()) {
            self.gui.horse_jump = remote.horse_jump_power();
            self.gui.mount_health = remote.mount_health();
        } else {
            self.gui.horse_jump = None;
            self.gui.mount_health = None;
        }
        self.ui_frame.set_viewport(renderer.resolution(), &self.settings);
        ui::paint(
            &mut self.ui_frame,
            &self.gui,
            &self.settings,
            &self.resources,
            &self.assets,
            self.game.as_ref().map(|g| &g.sim),
            &stats,
        );
        let input = RenderInput {
            selected_block: self.game.as_ref().and_then(|g| {
                g.selected_block(&self.gui, server_camera.as_ref().unwrap_or(&g.player), alpha)
            }),
            assets: &self.assets,
            resources: &self.resources,
            settings: &self.settings,
            gui: &self.gui,
            ui: &self.ui_frame,
            world: self.game.as_ref().map(|g| &g.world),
            player: server_camera.as_ref().or_else(||self.game.as_ref().map(|g| &g.player)),
            simulation: self.game.as_ref().map(|g| &g.sim),
            alpha,
            time: stats.time,
        };
        let interval_context = (stats.time, stats.position);
        let before_render = Instant::now();
        if check_scene {
            anyhow::ensure!(
                Some(renderer.resolution())
                    == self.options.benchmark_scene.as_ref().map(|s| s.resolution),
                "Benchmark window resolution differs from the scene"
            );
        }
        renderer.set_frame_profiling(benchmark && self.benchmark_start
            .is_some_and(|started| started.elapsed().as_secs_f32() >= warmup));
        if self.options.benchmark_capture_transparency {
            renderer.transparency_capture_time(if benchmark {self.benchmark_sample_start.map(|at|at.elapsed().as_secs_f64())}else{None});
        }
        let presented = match renderer.render(&input) {
            Ok(()) => true,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                let size = self.window.as_ref().unwrap().inner_size();
                renderer.resize(size.width, size.height);
                false
            }
            Err(wgpu::SurfaceError::Timeout) => false,
            Err(e) => return Err(e.into()),
        };
        self.interval_profile.frame(now, before_gui, before_render);
        let first_play_frame = presented && self.gui.screen == Screen::Playing && self.entry_report.is_none();
        if first_play_frame {
            if let (Some(requested), Some(game)) = (self.entry_started, self.game.as_ref()) {
                let first_play_ms = requested.elapsed().as_secs_f64() * 1000.;
                let [x, _, z] = game.player.precise_position();
                let center = ((x.floor() as i32).div_euclid(16), (z.floor() as i32).div_euclid(16));
                let report = serde_json::json!({
                    "kind": "request_to_first_presented_playing_frame",
                    "request_to_first_play_ms": first_play_ms,
                    "request_to_data_and_cpu_mesh_ms": self.entry_data_ready_ms,
                    "application_elapsed_ms": self.started.elapsed().as_secs_f64()*1000.,
                    "seed": game.options.seed.to_string(),
                    "render_distance": self.settings.render_distance,
                    "loaded_chunks": game.world.chunks.len(),
                    "nearby_columns_ready": mesh_neighbors(center).iter().all(|p| game.world.chunks.contains_key(p)),
                    "entry_mesh_uploads_remaining": game.entry_pending.len(),
                    "initial_view_pending": game.population.as_ref().is_some_and(|p| p.initial_view_pending()),
                    "startup_phases": game.population.as_ref().map(|p| p.startup_metrics()),
                    "render_model_cache": crate::remote_mobs::render_model_cache_report(),
                    "note": "Starts at the world request after menu assets are ready; includes data, CPU meshes, GPU upload and first successful play-frame presentation. Distant view may still be streaming."
                });
                if self.options.check
                    || self.options.benchmark
                    || self.options.benchmark_output_dir.is_some()
                {
                    let output = self
                        .options
                        .benchmark_output_dir
                        .clone()
                        .unwrap_or_else(|| self.options.project_dir.join("reports"));
                    std::fs::create_dir_all(&output)?;
                    std::fs::write(
                        output.join("world-entry.json"),
                        serde_json::to_vec_pretty(&report)?,
                    )?;
                }
                println!("World entry: {first_play_ms:.1} ms to first presented play frame");
                self.entry_report = Some(report);
            }
        }
        if self.options.benchmark
            && self
                .benchmark_start
                .is_some_and(|s| s.elapsed().as_secs_f32() >= warmup)
        {
            for (i, ms) in [
                (before_gui - now).as_secs_f64() * 1000.0,
                (before_render - before_gui).as_secs_f64() * 1000.0,
                before_render.elapsed().as_secs_f64() * 1000.0,
            ]
            .into_iter()
            .enumerate()
            {
                self.timing_max_ms[i] = self.timing_max_ms[i].max(ms);
            }
            for (maximum, current) in self.render_timing_max_ms.iter_mut().zip(renderer.render_cpu_ms) {
                *maximum = maximum.max(current);
            }
        }
        if self.screenshot_pending {
            self.screenshot_pending = false;
            renderer.screenshot(
                &PathBuf::from(format!("screenshots/{}.png", timestamp())),
                &input,
            )?;
        }
        if let Some(path) = self.options.screenshot.as_ref().filter(|_| self.options.connect.is_some()) {
            let ready = self.game.as_ref().is_some_and(|game| {
                let [x, _, z] = game.player.precise_position();
                let center = ((x.floor() as i32).div_euclid(16), (z.floor() as i32).div_euclid(16));
                mesh_neighbors(center).iter().all(|chunk| game.world.chunks.contains_key(chunk))
                    && game.dirty.is_empty()
                    && game.rebuild.is_none()
                    && game.uploads.is_empty()
                    && game.prepared.is_empty()
            });
            let min_dwell = if (self.options.seconds - 15.0).abs() > f32::EPSILON {
                self.started.elapsed().as_secs_f32() >= self.options.seconds
            } else {
                true
            };
            if self.gui.screen == Screen::Playing
                && presented
                && min_dwell
                && ((ready && renderer.visible_chunks > 0 && renderer.triangles > 0)
                    || (self.started.elapsed() > Duration::from_secs(8)
                        && self.game.as_ref().is_some_and(|g| {
                            g.dirty.is_empty()
                                && g.rebuild.is_none()
                                && g.uploads.is_empty()
                                && g.prepared.is_empty()
                        })))
            {
                renderer.screenshot(path, &input)?;
                println!("Rendered server world: {} ({} visible chunks, {} triangles)",path.display(),renderer.visible_chunks,renderer.triangles);
                event_loop.exit();
            } else if self.started.elapsed() > Duration::from_secs(60) {
                anyhow::bail!("Server world did not finish loading nearby chunks and meshes within 60 seconds");
            }
        }
        if benchmark
            && presented
            && self
                .benchmark_start
                .is_some_and(|s| s.elapsed().as_secs_f32() >= warmup)
        {
            let present = Instant::now();
            self.interval_profile.present(present, self.started, interval_context.0, interval_context.1, self.audio.take_profile());
            if self.options.benchmark_gpu_timestamps {
                renderer.gpu_timestamps_presented(
                    self.last_benchmark_present.map(|_| self.samples.len()),
                    self.last_benchmark_present.map(|at| at.duration_since(self.started).as_secs_f64() * 1000.),
                    present.duration_since(self.started).as_secs_f64() * 1000.,
                )?;
            }
            if self.options.benchmark_surface_waits {
                renderer.surface_wait_trace_presented(
                    self.last_benchmark_present.map(|_| self.samples.len()),
                    self.last_benchmark_present.map(|at| at.duration_since(self.started).as_secs_f64() * 1000.),
                    present.duration_since(self.started).as_secs_f64() * 1000.,
                )?;
            }
            self.audio.set_profiling(true);
            if self.options.benchmark_streaming && self.benchmark_sample_start.is_none() {
                // Read the actual just-presented anchor; no pose change or sample reset.
                self.benchmark_streaming_origin = self.game.as_ref().map(|g| g.player.precise_position());
            }
            let sample_start = *self.benchmark_sample_start.get_or_insert(present);
            if let Some(previous) = self.last_benchmark_present.replace(present) {
                self.samples
                    .push(present.duration_since(previous).as_secs_f64());
                if present.duration_since(sample_start).as_secs_f64() >= f64::from(self.options.seconds) {
                    if self.options.benchmark_scene.is_some() || self.options.benchmark_streaming {
                        let capture_name = if self.options.benchmark_scene.is_some() { "benchmark-native-scene.png" }
                            else { "benchmark-streaming.png" };
                        renderer.screenshot(
                            &self.options.benchmark_output_dir.as_deref().unwrap_or_else(|| std::path::Path::new("reports")).join(capture_name),
                            &input,
                        )?;
                    }
                    // Terminal GPU readback occurs after the last measured
                    // successful-present boundary and never inside the flight.
                    renderer.finish_gpu_timestamps()?;
                    self.finish_benchmark()?;
                    event_loop.exit();
                }
            }
        }
        // RenderInput and optional captures have finished borrowing the world.
        // This only updates the next command's hint, after a successful present.
        if first_play_frame {
            if let Some(host) = self.game.as_mut().and_then(|g| g.population.as_mut()) {
                host.maintenance_first_play_presented();
            }
        }
        Ok(())
    }
    fn key(
        &mut self,
        key: KeyCode,
        pressed: bool,
        repeat: bool,
    ) -> Result<()> {
        if pressed {
            self.keys.insert(key);
        } else {
            self.keys.remove(&key);
        }
        self.gui.video.shift_held =
            self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
        self.gui.video.alt_held =
            self.keys.contains(&KeyCode::AltLeft) || self.keys.contains(&KeyCode::AltRight);
        let code = crate::keybindings::key_code(key);
        if self.gui.screen == Screen::CommandBlock {
            if pressed {
                match key {
                    KeyCode::Escape => self.open(Screen::Playing),
                    KeyCode::Enter | KeyCode::NumpadEnter => {
                        if !self.gui.command_block.command.value.trim().is_empty() {
                            self.action(Action::SaveCommandBlock, false)?;
                        }
                    }
                    _ => {
                        let control = self.control_down();
                        let shift = self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
                        self.gui.command_block.command.key(key, shift, control, 32767);
                    }
                }
            }
            return Ok(());
        }
        if self.gui.screen==Screen::Book {
            if pressed {
                match key {
                    KeyCode::Escape=>self.open(Screen::Playing),
                    KeyCode::Backspace=>self.gui.book.erase(),
                    KeyCode::Enter|KeyCode::NumpadEnter if self.gui.book.signing=>{
                        if !self.gui.book.title.trim().is_empty(){self.action(Action::BookFinish(true),false)?;}
                    }
                    KeyCode::Enter|KeyCode::NumpadEnter=>self.gui.book.insert("\n",&self.resources,&self.settings),
                    KeyCode::KeyV if self.control_down() && !self.gui.book.signing=>self.gui.book.insert(&crate::clipboard::get(),&self.resources,&self.settings),
                    _=>()
                }
            }
            return Ok(());
        }
        if pressed && self.gui.screen==Screen::Chest && !matches!(key,KeyCode::Escape){
            let ctrl=self.control_down();let shift=self.keys.contains(&KeyCode::ShiftLeft)||self.keys.contains(&KeyCode::ShiftRight);
            if let ui::special_windows::Special::Anvil{slot:Some(_),focused:true,field,..}=&mut self.gui.special{
                let before=field.value.clone();field.key(key,shift,ctrl,30);
                if field.value!=before{self.send_anvil_name()?;}
                return Ok(());
            }
        }
        if self.gui.screen==Screen::ServerPack{if pressed&&key==KeyCode::Escape{self.action(Action::ServerPack(false),false)?;}return Ok(())}
        if self.gui.screen==Screen::Sign {
            if pressed{match key{
                KeyCode::ArrowUp=>self.gui.sign.row=(self.gui.sign.row+3)%4,
                KeyCode::ArrowDown|KeyCode::Enter|KeyCode::NumpadEnter=>self.gui.sign.row=(self.gui.sign.row+1)%4,
                KeyCode::Backspace=>{self.gui.sign.lines[self.gui.sign.row].pop();},
                KeyCode::KeyV if self.control_down()=>self.gui.sign.insert(&crate::clipboard::get(),&self.resources,&self.settings),
                KeyCode::Escape=>self.action(Action::FinishSign,false)?,_=>{},
            }}
            return Ok(());
        }
        if self.gui.screen == Screen::Controls && self.gui.editing_binding.is_some() {
            if pressed && !repeat {
                if let Some(code) = code {
                    self.finish_rebinding(if key == KeyCode::Escape { 0 } else { code })?;
                }
            }
            return Ok(());
        }
        if !pressed {
            if let Some(binding) = code.and_then(|c| self.settings.bindings.action_for(c)) {
                self.bound_input(binding, false)?;
            }
            return Ok(());
        }
        if repeat && !matches!(self.gui.screen, Screen::Video | Screen::Multiplayer | Screen::Chat | Screen::Sleeping | Screen::RenameWorld | Screen::Create) {
            return Ok(());
        }
        if key == KeyCode::F4 && !repeat {
            if let Some(renderer) = &mut self.renderer {
                renderer.toggle_shader();
            }
        }
        if !repeat {
            if let Some(global) = code.and_then(|code| crate::keybindings::GLOBAL.into_iter()
                .find(|&binding| code != 0 && self.settings.bindings.code(binding) == code)) {
                self.bound_input(global, true)?;
            }
        }
        if self.screen_key(key)? {
            return Ok(());
        }
        if self.gui.screen == Screen::Playing && self.keys.contains(&KeyCode::F3) {
            match key {
                KeyCode::KeyA => {
                    if let Some(g) = &mut self.game {
                        g.dirty.extend(g.world.chunks.keys());
                    }
                    return Ok(());
                }
                KeyCode::KeyB => {
                    self.gui.hitboxes = !self.gui.hitboxes;
                    return Ok(());
                }
                KeyCode::KeyH => {
                    self.gui.advanced = !self.gui.advanced;
                    return Ok(());
                }
                _ => {}
            }
        }
        match key {
            KeyCode::Escape => {
                if self.gui.screen == Screen::Sleeping {
                    self.action(Action::LeaveBed, false)?;
                } else if self.gui.screen == Screen::Playing {
                    self.open(Screen::Pause);
                } else if self.gui.screen == Screen::Main {
                    // GuiMainMenu.keyTyped intentionally ignores Escape.
                } else if matches!(self.gui.screen, Screen::Options | Screen::OptionPage(_) | Screen::Statistics | Screen::ShareLan | Screen::Achievements | Screen::Controls | Screen::Languages | Screen::ConfirmChatLink) {
                    // GuiScreen.keyTyped closes the GUI; only the Done button returns to its parent.
                    self.open(if self.game.is_some() { Screen::Playing } else { Screen::Main });
                } else if !matches!(self.gui.screen, Screen::Loading | Screen::Death) {
                    let back = self.gui.back();
                    self.open(back);
                }
            }
            KeyCode::F1 if self.gui.screen == Screen::Playing => self.gui.hide_hud = !self.gui.hide_hud,
            KeyCode::F3 if self.gui.screen == Screen::Playing => self.gui.debug = !self.gui.debug,
            _ => {
                if !repeat {
                    if let Some(binding) = code.and_then(|c| self.settings.bindings.action_for(c)).filter(|b| !crate::keybindings::GLOBAL.contains(b)) {
                        self.bound_input(binding, true)?;
                    }
                }
            }
        }
        Ok(())
    }
    fn mouse_button(&mut self,button:MouseButton,pressed:bool)->Result<()> {
        if pressed && self.gui.screen==Screen::Chest && button==MouseButton::Left{
            if let ui::special_windows::Special::Anvil{focused,..}=&mut self.gui.special{
                *focused=self.ui_frame.buttons.iter().any(|b|b.rect.contains(self.gui.cursor)&&matches!(b.action,Action::AnvilCursor(_)));
            }
        }
        if self.gui.screen != Screen::Playing && self.settings.touchscreen {
            if pressed { self.touch_depth += 1; if self.touch_depth > 1 { return Ok(()); } }
            else { self.touch_depth=self.touch_depth.saturating_sub(1); if self.touch_depth > 0 { return Ok(()); } }
        }
        if self.gui.screen==Screen::Creative && self.creative_mouse(button,pressed)?{return Ok(());}
        if pressed && matches!(self.gui.screen, Screen::Worlds | Screen::RenameWorld | Screen::DeleteWorld) {
            if let Some(action) = self.gui.world_list.pointer_down(self.gui.screen,self.gui.cursor,self.ui_frame.size,self.gui.worlds.len(),button) {
                self.action(action,false)?;
                return Ok(());
            }
        }
        if pressed && self.gui.screen == Screen::Create && button == MouseButton::Left {
            self.gui.create.pointer_down(self.gui.cursor,self.ui_frame.size);
        }
        if self.gui.screen == Screen::Multiplayer && pressed {
            let shift = self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
            if let Some(command) = self.gui.multiplayer.pointer_down(self.gui.cursor, self.ui_frame.size, button, shift) {
                self.action(Action::Server(command), false)?;
                return Ok(());
            }
        }
        if !pressed && button == MouseButton::Left {
            self.gui.option_state.end_scroll();
            if let Some(drag) = self.drag_control.take() {
                if matches!(drag, input::Drag::Option(ui::options::Slider::Sound(_), _)) {
                    self.audio.set_levels(self.settings.volume, &self.settings.menu.sound_levels);
                    self.audio.play_ui(&self.resources);
                }
                self.settings.save_to(&self.settings_path)?;
            }
            self.gui.multiplayer.release();
            self.gui.world_list.release();
            self.gui.create.release();
            self.gui.video.release();
        }
        if self.gui.screen == Screen::Video && pressed {
            let shift = self.keys.contains(&KeyCode::ShiftLeft)
                || self.keys.contains(&KeyCode::ShiftRight);
            let effect = ui::sodium_video::pointer_down(
                &mut self.gui,
                &self.settings,
                self.ui_frame.size,
                button,
                shift,
            );
            self.apply_video_effect(effect)?;
            return Ok(());
        }
        if self.gui.screen == Screen::Controls && self.gui.editing_binding.is_some() {
            if pressed { self.finish_rebinding(crate::keybindings::mouse_code(button))?; }
            return Ok(());
        }
        if button==MouseButton::Left && self.gui.screen==Screen::Achievements {self.gui.achievements.clicked(pressed,self.gui.cursor,self.ui_frame.size);}
        if pressed && button == MouseButton::Left && ui::options::begin_scroll(&mut self.gui,&self.resources,self.ui_frame.size) {return Ok(());}
        if self.gui.screen != Screen::Playing
            && self.inventory_mouse(button, pressed)?
        {
            return Ok(());
        }
        if self.gui.screen == Screen::Playing {
            if pressed && button == MouseButton::Middle
                && self.game.as_ref().is_some_and(|g| g.player.is_spectator())
            {
                return self.activate_spectator_slot(None);
            }
            let code = crate::keybindings::mouse_code(button);
            if let Some(binding) = self.settings.bindings.action_for(code) {
                self.bound_input(binding, pressed)?;
            }
        } else if pressed && (button == MouseButton::Left || (self.gui.screen == Screen::Video && button == MouseButton::Right)) {
            if let Some(action) = self
                .ui_frame.buttons
                .iter()
                .rev()
                .find(|b| b.enabled && b.rect.contains(self.gui.cursor))
                .map(|b| b.action.clone())
            {
                self.begin_menu_drag(&action)?;
                match &action {
                    Action::Option(ui::options::Command::CycleShader) => self.audio.play_secret(&self.resources),
                    Action::Option(ui::options::Command::Slider(ui::options::Slider::Sound(_)))
                    | Action::Controls(crate::keybindings::ControlAction::Edit(_) | crate::keybindings::ControlAction::Reset(_))
                    | Action::Server(ui::multiplayer::Command::Select(_) | ui::multiplayer::Command::SetField(_, _))
                    | Action::Slot(_) | Action::ContainerSlot(_) | Action::Craft | Action::Creative(_) => {}
                    _ => self.audio.play_ui(&self.resources),
                }
                self.action(action, button == MouseButton::Right)?;
            } else if self.gui.screen == Screen::Controls && button == MouseButton::Left {
                self.begin_controls_scroll();
            }
        }
        Ok(())
    }
    fn focus_lost(&mut self) {
        self.inventory_drag = None;
        self.last_inventory_click = None;
        self.left = false;
        self.right = false;
        self.local_right_clicks = 0;
        self.keys.clear();
        self.character_keys.clear();
        self.drag_control = None;
        self.touch_depth = 0;
        self.gui.world_list.release();
        self.gui.create.release();
        self.gui.video.release();
        self.gui.video.shift_held = false;
        self.gui.video.alt_held = false;
        if self.gui.screen == Screen::Playing && !self.options.benchmark {
            self.open(Screen::Pause);
        }
    }
    fn text(&mut self, text: &str) {
        if self.gui.screen==Screen::Creative{self.creative_text(text);return}
        if self.control_down() { return }
        if self.gui.screen == Screen::CommandBlock {
            self.gui.command_block.command.insert(text, 32767);
            return;
        }
        if self.gui.screen==Screen::Chest{
            if let ui::special_windows::Special::Anvil{slot:Some(_),focused:true,field,..}=&mut self.gui.special{
                let old=field.value.clone();field.insert(text,30);
                if old!=field.value{if let Err(e)=self.send_anvil_name(){self.gui.status=e.to_string();}}
                return
            }
        }
        if self.gui.screen==Screen::Book {if text.chars().all(|c|c>=' '){self.gui.book.insert(text,&self.resources,&self.settings)}return}
        if self.gui.screen==Screen::Sign{self.gui.sign.insert(text,&self.resources,&self.settings);return}
        if matches!(self.gui.screen, Screen::Chat | Screen::Sleeping) { self.gui.chat.insert(text); return }
        if self.gui.screen == Screen::Multiplayer { self.gui.multiplayer.text(text); return }
        match self.gui.screen {
            Screen::Video => ui::sodium_video::text_input(&mut self.gui, text),
            Screen::Create => self.gui.create.text(text),
            Screen::RenameWorld => self.gui.world_list.text(text),
            _ => {}
        }
    }
}
impl ApplicationHandler<AppEvent> for App {
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::LocalTick => {
                if let Err(error) = self.service_local_ticks(event_loop, Instant::now(), interval_profile::Origin::UserEvent) {
                    self.fail(event_loop, error);
                }
            }
            AppEvent::WorldLoadReady => {
                // Keep result/error handling and entry uploads in frame().
                // Wake once on completion instead of waiting for a menu frame.
                if !event_loop.exiting() && self.loading.is_some() {
                    if let Some(window) = &self.window { window.request_redraw(); }
                }
            }
        }
    }
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let [width, height] = self
            .options
            .benchmark_scene
            .as_ref()
            .map_or([1600, 900], |scene| scene.resolution);
        let attributes = Window::default_attributes()
            .with_visible(self.options.screenshot.is_none())
            .with_title("Ferrum 1.8.9 | Vulkan")
            .with_inner_size(winit::dpi::PhysicalSize::new(width, height))
            .with_min_inner_size(winit::dpi::PhysicalSize::new(640, 480));
        match event_loop.create_window(attributes) {
            Ok(w) => {
                let w = Arc::new(w);
                match pollster::block_on(Renderer::new_with_gpu_stage_diagnostics(
                    Some(w.clone()),
                    &self.assets,
                    &self.resources,
                    &self.settings,
                    self.options.benchmark_adapter.as_deref(),
                    self.options.benchmark_near_arena,
                    self.options.benchmark_capture_transparency || self.options.benchmark_capture_near_opaque
                        || self.options.benchmark_capture_uploads || self.options.benchmark_near_opaque_runs,
                    self.options.benchmark_gpu_timestamps,
                    self.options.benchmark_gpu_stages,
                )) {
                    Ok(mut r) => {
                        r.configure_pre_acquire_visibility(self.options.benchmark_pre_acquire_visibility);
                        r.configure_packed_frustum(self.options.benchmark_packed_frustum);
                        r.configure_covered_lod_culling(self.options.benchmark_cull_covered_lod);
                        if let Err(error) = r.configure_surface_wait_trace(self.options.benchmark_surface_waits) {
                            self.fail(event_loop, error);
                            return;
                        }
                        r.configure_transparency_capture(self.options.benchmark_capture_transparency);
                        r.configure_near_opaque_shadow(self.options.benchmark_capture_near_opaque);
                        r.configure_upload_capture(self.options.benchmark_capture_uploads,Some(self.started));
                        r.configure_arena_upload_capture(self.options.benchmark_capture_arena_uploads,Some(self.started));
                        r.configure_near_opaque_runs(self.options.benchmark_near_opaque_runs);
                        if self.options.benchmark {
                            r.configure_slow_frame_threshold(self.options.benchmark_slow_frame_ms.unwrap_or_default());
                        }
                        r.configure_transparency_sort(self.options.benchmark_transparency_sort.unwrap_or_default());
                        if let Err(error) = r.configure_transparent_staging(self.options.benchmark_transparency_upload.unwrap_or_default()) {
                            self.fail(event_loop, error);
                            return;
                        }
                        if let Err(error) = r.configure_translucent_mdi(self.options.benchmark_transparency_draw.unwrap_or_default()) {
                            self.fail(event_loop, error);
                            return;
                        }
                        self.renderer=Some(r);
                    },
                    Err(e) => {
                        self.fail(event_loop, e);
                        return;
                    }
                }
                if self.settings.fullscreen && self.options.screenshot.is_none() {
                    w.set_fullscreen(Some(Fullscreen::Borderless(None)));
                }
                self.window = Some(w);
                if self.options.play || self.options.world.is_some() || self.options.benchmark {
                    self.start(self.options.clone());
                }
                self.window.as_ref().unwrap().request_redraw();
            }
            Err(e) => self.fail(event_loop, e.into()),
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if event_loop.exiting() { return; }
        if self.local_tick_failure.is_some()
            && !matches!(event, WindowEvent::RedrawRequested | WindowEvent::Resized(_) | WindowEvent::CloseRequested) { return; }
        if self.tick_busy() && !matches!(event, WindowEvent::RedrawRequested | WindowEvent::Resized(_)) {
            self.deferred_input.push_back(local_tick::DeferredInput::Window(_id, event));
            return;
        }
        let result = (|| -> Result<()> {
            match event {
                WindowEvent::CloseRequested => {
                    if self.local_tick_failure.is_none() { self.save()?; }
                    event_loop.exit();
                }
                WindowEvent::Resized(size) => {
                    if let Some(r) = &mut self.renderer {
                        r.resize(size.width, size.height);
                    }
                }
                WindowEvent::RedrawRequested => {
                    if self.window.as_ref().is_some_and(|w| {
                        let s = w.inner_size();
                        s.width > 0 && s.height > 0
                    }) {
                        self.frame(event_loop)?;
                    }
                }
                WindowEvent::Focused(false) => self.focus_lost(),
                WindowEvent::CursorMoved { position, .. } => {
                    let size = self.renderer.as_ref().unwrap().resolution();
                    let scale = self.settings.scale(size[0], size[1]);
                    self.gui.cursor = if matches!(self.gui.screen, Screen::Main | Screen::Pause | Screen::ShareLan | Screen::Achievements | Screen::Options | Screen::OptionPage(_) | Screen::Video
                        | Screen::Inventory | Screen::Creative | Screen::Crafting | Screen::Languages | Screen::Controls | Screen::Realms | Screen::Multiplayer | Screen::Create | Screen::Worlds | Screen::RenameWorld | Screen::DeleteWorld) {
                        input::menu_cursor([position.x, position.y], size, scale)
                    } else {
                        [position.x as f32 / scale, position.y as f32 / scale]
                    };
                    if self.gui.screen == Screen::Video {
                        ui::sodium_video::pointer_move(&mut self.gui, &self.settings, self.ui_frame.size);
                    }
                    if self.gui.screen==Screen::Achievements{self.gui.achievements.drag(self.gui.cursor,self.ui_frame.size);}
                    if self.gui.screen == Screen::Worlds { self.gui.world_list.drag(self.gui.cursor[1],self.ui_frame.size[1],self.gui.worlds.len()); }
                    if self.gui.screen == Screen::Create { self.gui.create.drag(self.gui.cursor,self.ui_frame.size); }
                    if self.gui.screen==Screen::Creative{self.gui.creative.drag(self.gui.cursor[1],self.ui_frame.size[1]);}
                    self.drag_menu()?;
                    if let Some(slot) = self.hovered_slot() {
                        if let Some(drag) = &mut self.inventory_drag {
                            if !drag.slots.contains(&slot) {
                                drag.slots.push(slot);
                            }
                        }
                    }
                }
                WindowEvent::Ime(Ime::Commit(text)) => self.text(&text),
                WindowEvent::KeyboardInput { event, .. } => {
                    self.keyboard_input(event.physical_key, event.state == ElementState::Pressed,
                        event.repeat, event.text.as_deref())?;
                }
                WindowEvent::MouseInput { button, state, .. } => {
                    self.mouse_button(button, state == ElementState::Pressed)?;
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    let y = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32,
                    };
                    if y != 0.0 {
                        if matches!(self.gui.screen, Screen::Playing | Screen::Chat | Screen::Sleeping) {
                            self.world_wheel(y);
                        } else if self.gui.screen==Screen::Creative {
                            self.gui.creative.wheel(y);
                        } else if ui::options::wheel(&mut self.gui, &self.resources, self.ui_frame.size, y) {
                            return Ok(());
                        } else if self.gui.screen == Screen::Worlds {
                            if (32. ..=self.ui_frame.size[1].ceil()-64.).contains(&self.gui.cursor[1]) {self.gui.world_list.wheel(y,self.ui_frame.size[1],self.gui.worlds.len());}
                        } else if self.gui.screen == Screen::Create {
                            self.gui.create.wheel(y,self.ui_frame.size);
                        } else if self.gui.screen == Screen::Controls {
                            self.gui.scroll = crate::keybindings::ControlsList::new(self.ui_frame.size).wheel(self.gui.scroll,y);
                        } else if self.gui.screen == Screen::Multiplayer {
                            self.gui.multiplayer.wheel(y,self.ui_frame.size[1]);
                        } else if self.gui.screen == Screen::Achievements {self.gui.achievements.wheel(y);
                        } else if self.gui.screen == Screen::Statistics {
                            self.gui.statistics.wheel(y,self.ui_frame.size[1]);
                        } else if self.gui.screen == Screen::Video {
                            let ctrl = self.control_down();
                            ui::sodium_video::wheel(
                                &mut self.gui,
                                &self.settings,
                                self.ui_frame.size,
                                y,
                                ctrl,
                            );
                        } else {
                            let limit = match self.gui.screen {
                                Screen::Video => 6,
                                Screen::Languages => self.resources.languages.len() as i32 - 1,
                                _ => 0,
                            }
                            .max(0);
                            self.gui.scroll =
                                (self.gui.scroll - if y > 0.0 { 1 } else { -1 }).clamp(0, limit);
                        }
                    }
                }
                _ => {}
            }
            Ok(())
        })();
        if self.quit_requested {event_loop.exit();}
        if let Err(e) = result {
            eprintln!("{e:#}");
            if self.game.is_some() {
                self.open(Screen::Pause);
                self.gui.status = format!("Error: {e}");
            } else {
                self.fail(event_loop, e);
            }
        }
    }
    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if self.options.screenshot.is_some() || self.local_tick_failure.is_some() || _event_loop.exiting(){return;}
        if self.gui.screen == Screen::Playing && !self.options.benchmark {
            if let DeviceEvent::MouseMotion { delta } = event {
                if self.game.is_some() {
                    self.mouse_accum[0] += delta.0;
                    self.mouse_accum[1] += delta.1;
                }
            }
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.service_local_ticks(event_loop, Instant::now(), interval_profile::Origin::AboutToWait) {
            self.fail(event_loop, error);
            return;
        }
        if event_loop.exiting() { return; }
        if self.options.screenshot.is_some()&&self.options.connect.is_some()&&self.window.is_some(){
            if self.last_frame.elapsed()>=Duration::from_millis(50){if let Err(e)=self.frame(event_loop){self.fail(event_loop,e);return;}}
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.last_frame+Duration::from_millis(50)));return;
        }
        let fps = self.settings.max_fps;
        let tick_deadline = self.local_tick_deadline(Instant::now());
        if let Some(w) = &self.window {
            let size = w.inner_size();
            if size.width == 0 || size.height == 0 {
                let wake = if self.game.as_ref().is_some_and(|g| g.remote.is_some()) {
                    let poll_cap = Instant::now() + Duration::from_millis(10);
                    Some(tick_deadline.map_or(poll_cap, |tick| tick.min(poll_cap)))
                } else {
                    tick_deadline
                };
                event_loop.set_control_flow(wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
                return;
            }
            if fps == 0 {
                event_loop.set_control_flow(ControlFlow::Poll);
                w.request_redraw();
            } else {
                let deadline = self.last_frame + Duration::from_secs_f64(1.0 / fps as f64);
                let wake = tick_deadline.map_or(deadline, |tick| tick.min(deadline));
                if Instant::now() >= wake {
                    w.request_redraw();
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
            }
        }
    }
}
