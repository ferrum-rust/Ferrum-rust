//! Playable DEFAULT world generation through the original serial population
//! owner. Gameplay and scheduled block ticks remain owned by Simulation.
use crate::{
    gameplay::{container_key, Simulation},
    native_session::{
        BlockEdit, Command, Config, Event, GeneratedSpawn, MetadataUpdate, NativeContainer,
        NativeSession, Reply, SpawnRecord, Update,
    },
    player_save::{Kinematics, PlayerSave},
    world::{ChunkPos, World},
    Options,
};
use anyhow::{bail, ensure, Context, Result};
use glam::{IVec3, Vec3};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    sync::Arc,
    time::Instant,
};

const SAVE_VERSION: u32 = 1;
const GENERATE_BATCH: usize = 32;
const STREAM_BATCH: usize = 8;
const EDIT_BATCH: usize = 4096;

type FrameTimings = BTreeMap<&'static str, [f64; 3]>;

/// Counts share the benchmark chunk-timing window. They contain no gameplay
/// state and are never persisted in a world checkpoint.
#[derive(Default, Serialize)]
pub(crate) struct PublicationWork {
    completed_events: u64,
    incoming_chunks: u64,
    max_incoming_chunks: usize,
    accepted_chunks: u64,
    replaced_chunks: u64,
    same_snapshot_chunks: u64,
    unique_old_chunks_at_replacement: u64,
    overlay_journal_entries_scanned: u64,
    overlay_index_entries_scanned: u64,
    overlay_scans_skipped: u64,
    overlay_candidate_entries_examined: u64,
    overlay_matching_publications_skipped: u64,
    overlaid_blocks: u64,
    queued_edit_journal_entries_scanned: u64,
    queued_commands_rebuilt: u64,
    dirty_columns_for_tile_restore: u64,
    pending_containers_examined: u64,
    pending_spawns_examined: u64,
}

#[derive(Default)]
struct PublicationProfile<'a> {
    timings: Option<&'a mut FrameTimings>,
    since: Option<Instant>,
}
impl PublicationProfile<'_> {
    fn enabled(&self) -> bool { self.timings.is_some() }
    fn start(&mut self) {
        self.since = self.timings.as_ref().map(|_| Instant::now());
    }
    fn phase(&mut self, name: &'static str) {
        if let (Some(timings), Some(since)) = (&mut self.timings, &mut self.since) {
            let now = Instant::now();
            let ms = now.duration_since(*since).as_secs_f64() * 1000.;
            let stats = timings.entry(name).or_default();
            stats[0] += 1.; stats[1] += ms; stats[2] = stats[2].max(ms);
            *since = now;
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct StartupTimings {
    pub resumed: bool,
    pub total_ms: f64,
    pub ready_ms: f64,
    pub restore_gameplay_ms: f64,
    pub original_spawn_ms: f64,
    pub deferred_spawn_publications: usize,
    pub initial_player_ms: f64,
    pub view_chunks_ms: f64,
    pub progressive_view: bool,
    pub deferred_view_requests: usize,
    pub overlay_ms: f64,
    /// Included in the stage totals above, not an additional stage.
    pub client_publication_ms: f64,
    pub owner: crate::native_session::GenerationTimings,
}
#[cfg(test)]
#[path = "populated_world_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "populated_world_edit_tests.rs"]
mod edit_tests;

pub(crate) struct Loaded {
    pub host: PopulatedWorld,
    pub world: World,
    pub pose: Option<PlayerSave>,
    pub simulation: Option<Simulation>,
}

#[derive(Serialize, Deserialize)]
struct GameplaySave {
    version: u32,
    position: [f64; 3],
    motion: Option<Kinematics>,
    simulation: Simulation,
    edits: Vec<([i32; 3], u16)>,
    seen_spawns: BTreeSet<(u8, u64)>,
    seen_containers: BTreeSet<[i32; 3]>,
    #[serde(default)]
    seen_spawners: BTreeSet<[i32; 3]>,
    pending_spawns: Vec<SpawnRecord>,
    pending_containers: Vec<NativeContainer>,
    #[serde(default)]
    pending_spawners: Vec<([i32; 3], String)>,
    /// The immutable first-view request order survives a snapshot taken before
    /// background population completes. Already retained requests are no-ops
    /// on resume; new camera requests must never overtake this sequence.
    #[serde(default)]
    initial_view_plan: Vec<ChunkPos>,
}

pub(crate) struct PopulatedWorld {
    session: NativeSession,
    persistent: bool,
    queued: VecDeque<Command>,
    in_flight: Option<u64>,
    sent_edits: HashMap<[i32; 3], u16>,
    submitted_rechecks: HashSet<[i32; 3]>,
    pending_edits: BTreeMap<[i32; 3], u16>,
    deferred_edits: HashMap<ChunkPos, BTreeMap<[i32; 3], u16>>,
    wanted: HashSet<ChunkPos>,
    known_chunks: HashSet<ChunkPos>,
    dirty: BTreeSet<ChunkPos>,
    seen_spawns: BTreeSet<(u8, u64)>,
    seen_containers: BTreeSet<[i32; 3]>,
    seen_spawners: BTreeSet<[i32; 3]>,
    pending_spawns: BTreeMap<(u8, u64), SpawnRecord>,
    pending_containers: BTreeMap<[i32; 3], NativeContainer>,
    pending_spawners: BTreeMap<[i32; 3], String>,
    adoption_dirty: bool,
    saving: bool,
    pending_autosave_capture: Option<Box<dyn FnOnce() -> Result<serde_json::Value> + Send>>,
    startup: StartupTimings,
    generation_timings: crate::native_session::GenerationTimings,
    initial_view_plan: Vec<ChunkPos>,
    publication_work: PublicationWork,
    maintenance_view: crate::native_session::maintenance::ViewState,
}

fn chunk_of(position: [f64; 3]) -> ChunkPos {
    (
        (position[0].floor() as i32).div_euclid(16),
        (position[2].floor() as i32).div_euclid(16),
    )
}

fn region(center: ChunkPos, radius: i32) -> Vec<ChunkPos> {
    // PlayerManager creates its server chunks in X-major, then Z-major order.
    (-radius..=radius)
        .flat_map(|x| (-radius..=radius).map(move |z| (center.0 + x, center.1 + z)))
        .collect()
}

fn initial_view_commands(plan: &[ChunkPos]) -> VecDeque<Command> {
    plan.chunks(GENERATE_BATCH).enumerate().map(|(index,batch)| {
        let next=((index+1)*GENERATE_BATCH).min(plan.len());
        Command::GenerateAhead { positions:batch.to_vec(),
            ahead:plan[next..(next+64).min(plan.len())].to_vec() }
    }).collect()
}

fn entry_neighborhood_ready(world: &World, center: ChunkPos) -> bool {
    (-1..=1).all(|x| (-1..=1).all(|z|
        world.chunks.get(&(center.0+x,center.1+z)).is_some_and(|chunk|chunk.light.is_some())))
}

impl PopulatedWorld {
    fn new(session: NativeSession, persistent: bool) -> Self {
        Self {
            session, persistent, queued: VecDeque::new(), in_flight: None,
            sent_edits: HashMap::new(), wanted: HashSet::new(), known_chunks: HashSet::new(),
            submitted_rechecks: HashSet::new(),
            pending_edits: BTreeMap::new(), deferred_edits: HashMap::new(),
            dirty: BTreeSet::new(), seen_spawns: BTreeSet::new(), seen_containers: BTreeSet::new(),
            seen_spawners: BTreeSet::new(),
            pending_spawns: BTreeMap::new(), pending_containers: BTreeMap::new(),
            pending_spawners: BTreeMap::new(),
            adoption_dirty: true, saving: false,
            pending_autosave_capture: None,
            startup: StartupTimings::default(),
            generation_timings: crate::native_session::GenerationTimings::default(),
            initial_view_plan: Vec::new(),
            publication_work: PublicationWork::default(),
            maintenance_view: Default::default(),
        }
    }
    pub(crate) fn load(options: &Options, progress: &mut dyn FnMut(u8)) -> Result<Loaded> {
        let started = Instant::now();
        ensure!(
            options.world.is_none()
                && options.structure_preview.is_none()
                && options
                    .world_rules
                    .world_type
                    .eq_ignore_ascii_case("default"),
            "Populated native generation requires a DEFAULT overworld"
        );
        let world_id = options
            .save_id
            .clone()
            .unwrap_or_else(|| format!("seed-{:016x}", options.seed));
        let mut config = Config::new(
            options.seed as i64,
            world_id,
            options.project_dir.join("saves"),
        );
        config.map_features = options.world_rules.structures;
        config.enable_scheduled_ticks = false;
        config.resume_publish_radius = Some((options.render_distance as i32).max(3) + 1);
        let persistent = !options.fresh && !options.benchmark;
        let mut session = if persistent {
            NativeSession::spawn(config)?
        } else {
            NativeSession::in_memory_with_config(config, 1234)?
        };
        let (resumed, path, gameplay, ready) = match session.receive()? {
            Event::Ready {
                resumed,
                path,
                gameplay,
                update,
            } => (resumed, path, gameplay, update),
            Event::Failed { message, .. } => bail!("{message}"),
            event => bail!("Unexpected native startup: {event:?}"),
        };
        let ready_ms = started.elapsed().as_secs_f64() * 1000.;
        let restore_started = Instant::now();
        let saved = if gameplay.is_null() {
            None
        } else {
            let saved: GameplaySave =
                serde_json::from_value(gameplay).context("Read playable native world state")?;
            ensure!(
                saved.version == SAVE_VERSION && crate::player_save::valid_position(saved.position),
                "Unsupported or invalid playable native save"
            );
            Some(saved)
        };
        // The old terrain-only overlay is read without modifying its file.
        // A v4 checkpoint with no gameplay may be an interrupted first load.
        let migration = if saved.is_none() && persistent {
            World::read_generation_migration(options)?
        } else {
            None
        };
        let fresh_progressive = !resumed && saved.is_none() && migration.is_none()
            && options.progressive_world_entry();
        let mut pose = saved
            .as_ref()
            .map(|s| PlayerSave {
                position: s.position,
                motion: s.motion,
            })
            .or_else(|| migration.as_ref().and_then(|m| m.pose));
        let mut world = World {
            chunks: Default::default(),
            spawn: Vec3::ZERO,
            label: format!(
                "Native 1.8.9 populated world / seed {}",
                options.seed as i64
            ),
            save_path: path.unwrap_or_default(),
            edits: Default::default(),
            // Native generation supplies its own original animal placement.
            imported: true,
            light_revision: 0,
        };
        let mut host = Self::new(session, persistent);
        host.startup.resumed = resumed;
        host.startup.ready_ms = ready_ms;
        let had_migration = migration.is_some();
        let simulation = if let Some(saved) = saved {
            host.initial_view_plan = saved.initial_view_plan;
            world.edits.extend(saved.edits);
            // Some sparse migration edits may belong to not-yet-generated
            // regions. Rebuild acknowledgements as chunks are republished;
            // rewriting an identical retained state has no world callbacks.
            host.seen_spawns = saved.seen_spawns;
            host.seen_containers = saved.seen_containers;
            host.seen_spawners = saved.seen_spawners;
            host.pending_spawns.extend(
                saved
                    .pending_spawns
                    .into_iter()
                    .map(|s| ((s.family, s.index), s)),
            );
            host.pending_containers.extend(
                saved
                    .pending_containers
                    .into_iter()
                    .map(|c| (c.position, c)),
            );
            host.pending_spawners.extend(saved.pending_spawners);
            Some(saved.simulation)
        } else if let Some(migration) = migration {
            world.edits.extend(migration.edits);
            migration.simulation
        } else {
            None
        };
        // The visible cache can be much smaller than the retained server world.
        let radius = (options.render_distance as i32).max(3) + 1;
        if let Some(p) = pose {
            if simulation.as_ref().is_some_and(|s| s.dimension != 0) {
                host.wanted.extend(ready.chunks.iter().map(|(pos, _)| *pos));
            } else {
                host.wanted.extend(region(chunk_of(p.position), radius));
            }
        }
        host.apply_loading(&mut world, ready);
        host.startup.restore_gameplay_ms = restore_started.elapsed().as_secs_f64() * 1000.;
        let spawn_started = Instant::now();
        let (reply, update) = host.exchange(Command::PrepareSpawn)?;
        let Reply::SpawnPrepared {
            point,
            initial_region,
        } = reply
        else {
            bail!("Native owner did not prepare its spawn");
        };
        world.spawn = IVec3::from_array(point).as_vec3();
        if pose.is_none() {
            host.wanted
                .extend(region((point[0] >> 4, point[2] >> 4), radius));
        }
        host.apply_loading(&mut world, update);
        progress(5);
        // Provider.load immediately returns for retained chunks. A completed
        // playable save can omit those twenty empty round trips; interrupted
        // first loads still finish the original X/Z sequence before joining.
        let mut loaded = if resumed && pose.is_some()
            && initial_region.iter().all(|p| host.known_chunks.contains(p)) {
            initial_region.len()
        } else { 0 };
        while loaded < initial_region.len() {
            let command = if fresh_progressive { Command::LoadSpawnBatchDeferred { count: 32 } }
                else { Command::LoadSpawnBatch { count: 32 } };
            let (reply, update) = host.exchange(command)?;
            let Reply::SpawnAreaProgress { loaded: next } = reply else {
                bail!("Missing native spawn-region progress");
            };
            ensure!(next > loaded, "Native spawn-region load made no progress");
            loaded = next;
            if fresh_progressive && loaded < initial_region.len() {
                host.startup.deferred_spawn_publications += 1;
            }
            host.apply_loading(&mut world, update);
            progress(5 + (65 * loaded / initial_region.len()) as u8);
        }
        progress(70);
        host.startup.original_spawn_ms = spawn_started.elapsed().as_secs_f64() * 1000.;
        let player_started = Instant::now();
        if pose.is_none() {
            let (reply, update) = host.exchange(Command::PreparePlayer { adventure: false })?;
            let Reply::PlayerPrepared { initial } = reply else {
                bail!("Native player spawn failed");
            };
            pose = Some(PlayerSave {
                position: initial.position,
                motion: None,
            });
            host.apply_loading(&mut world, update);
        }
        host.startup.initial_player_ms = player_started.elapsed().as_secs_f64() * 1000.;
        let view_started = Instant::now();
        // An autosave may have committed only a prefix of the first view.
        // Complete that original sequence before deriving requests from the
        // saved player position or a newly selected render distance.
        host.restore_initial_view(&mut world)?;
        let center = if simulation.as_ref().is_some_and(|s| s.dimension != 0) {
            (point[0] >> 4, point[2] >> 4)
        } else {
            chunk_of(pose.unwrap().position)
        };
        let positions = region(center, radius);
        host.wanted = positions.iter().copied().collect();
        world.chunks.retain(|p, _| host.wanted.contains(p));
        host.maintenance_desired(center, radius);
        host.maintenance_commit(center, radius);
        let missing = host.missing_generation_requests(&world, &positions);
        let resumed_progressive = resumed
            && !had_migration
            && options.progressive_world_entry()
            && !missing.is_empty();
        let progressive =
            (fresh_progressive || resumed_progressive) && entry_neighborhood_ready(&world, center);
        let deferred = if progressive { missing } else {
            host.generate_initial_view_loading(&mut world,&missing)?;
            Vec::new()
        };
        host.startup.progressive_view = progressive;
        host.startup.deferred_view_requests = deferred.len();
        host.startup.view_chunks_ms = view_started.elapsed().as_secs_f64() * 1000.;
        let overlay_started = Instant::now();
        // Finish population before replaying migration writes. Existing player
        // blocks win over new flora; the old world is still available unchanged.
        host.queue_edits(&mut world);
        while let Some(command) = host.queued.pop_front() {
            let (reply, update) = host.exchange(command)?;
            Self::successful(&reply)?;
            host.apply_loading(&mut world, update);
        }
        if let Some(sim) = &simulation {
            if let Some(spawn) = sim.world_commands.spawn {
                world.spawn = IVec3::from_array(spawn).as_vec3();
            }
            let chunks = world.chunks.keys().copied().collect::<Vec<_>>();
            sim.world_commands.restore_tiles(&mut world, &chunks);
        }
        ensure!(
            !world.chunks.is_empty(),
            "Native generation produced no visible chunks"
        );
        progress(75);
        host.dirty.clear();
        host.startup.overlay_ms = overlay_started.elapsed().as_secs_f64() * 1000.;
        host.startup.total_ms = started.elapsed().as_secs_f64() * 1000.;
        host.startup.owner = host.generation_timings;
        host.begin_initial_view(deferred)?;
        Ok(Loaded {
            host,
            world,
            pose,
            simulation,
        })
    }

    pub(crate) fn startup_metrics(&self) -> &StartupTimings {
        &self.startup
    }

    pub(crate) fn initial_view_pending(&self) -> bool { !self.initial_view_plan.is_empty() }

    fn generate_initial_view_loading(&mut self, world: &mut World, plan: &[ChunkPos]) -> Result<()> {
        for command in initial_view_commands(plan) {
            let (reply,update)=self.exchange(command)?;
            Self::successful(&reply)?;
            self.apply_loading(world,update);
        }
        Ok(())
    }

    fn restore_initial_view(&mut self, world: &mut World) -> Result<()> {
        // A snapshot follows the in-flight generation even when its gameplay
        // capture preceded that result. Retained-state filtering handles this
        // race without trusting a client-side cursor or reordering new loads.
        let remaining=self.initial_view_plan.iter().copied()
            .filter(|p|!self.known_chunks.contains(p)).collect::<Vec<_>>();
        self.generate_initial_view_loading(world,&remaining)?;
        self.initial_view_plan.clear();
        Ok(())
    }

    fn begin_initial_view(&mut self, plan: Vec<ChunkPos>) -> Result<()> {
        ensure!(!self.busy(),"Initial view cannot overtake an earlier native command");
        self.queued.extend(initial_view_commands(&plan));
        self.initial_view_plan=plan;
        self.start_next()
    }

    fn apply_loading(&mut self, world: &mut World, update: Update) {
        let begin = Instant::now();
        self.apply(world, update);
        self.startup.client_publication_ms += begin.elapsed().as_secs_f64() * 1000.;
    }

    fn exchange(&mut self, command: Command) -> Result<(Reply, Update)> {
        ensure!(
            self.in_flight.is_none(),
            "A native request is already in flight"
        );
        self.record_submitted_edits(&command);
        let request = self.session.submit(command)?;
        match self.session.receive()? {
            Event::Completed {
                request_id,
                reply,
                update,
            } if request_id == request => Ok((reply, update)),
            Event::Failed { message, .. } => bail!("{message}"),
            event => bail!("Unexpected native reply for {request}: {event:?}"),
        }
    }

    fn successful(reply: &Reply) -> Result<()> {
        match reply {
            Reply::Generated | Reply::Saved { .. } => Ok(()),
            Reply::Edited { rejected } if rejected.is_empty() => Ok(()),
            other => bail!("Native generation request could not complete: {other:?}"),
        }
    }

    fn refresh_pending_edit(&mut self, position: [i32; 3], state: Option<u16>) {
        let chunk = (position[0].div_euclid(16), position[2].div_euclid(16));
        self.pending_edits.remove(&position);
        if let Some(deferred) = self.deferred_edits.get_mut(&chunk) {
            deferred.remove(&position);
            if deferred.is_empty() { self.deferred_edits.remove(&chunk); }
        }
        let Some(state) = state.filter(|state| self.sent_edits.get(&position) != Some(state)) else { return; };
        if self.known_chunks.contains(&chunk) {
            self.pending_edits.insert(position, state);
        } else {
            self.deferred_edits.entry(chunk).or_default().insert(position, state);
        }
    }

    fn queue_edits(&mut self, world: &mut World) -> usize {
        // This is the sole dirty-cursor consumer. The durable overlay remains
        // intact for save files and racing chunk publications. Other World
        // owners do not opt in and therefore retain no dirty tombstones.
        let inspected = match world.edits.take_changes() {
            crate::world::EditChanges::Reset => {
                self.pending_edits.clear();
                self.deferred_edits.clear();
                self.submitted_rechecks.clear();
                for (&position, &state) in &world.edits {
                    self.refresh_pending_edit(position, Some(state));
                }
                world.edits.len()
            }
            crate::world::EditChanges::Positions(mut positions) => {
                // A captured autosave may submit an older value after another
                // queue boundary consumed a revert equal to the previous sent
                // value. That submission changes our comparison baseline even
                // without another journal mutation. Re-read only those keys,
                // including removed entries, against the latest durable map.
                positions.extend(std::mem::take(&mut self.submitted_rechecks));
                let inspected = positions.len();
                for position in positions {
                    self.refresh_pending_edit(position, world.edits.get(&position).copied());
                }
                inspected
            }
        };
        // Sorted pending keys reproduce the old full-scan/sort command order.
        // Keep entries until SUBMISSION, so canceled unsent commands rebuild
        // from the latest values without losing unmodified pending writes.
        let edits = self.pending_edits.iter().map(|(position, state)| BlockEdit {
                position: *position,
                state: *state,
                expected: None,
                notify_neighbors: false,
            }).collect::<Vec<_>>();
        for batch in edits.chunks(EDIT_BATCH) {
            self.queued.push_back(Command::Edits(batch.to_vec()));
        }
        inspected
    }

    fn record_submitted_edits(&mut self, command: &Command) {
        if let Command::Edits(edits) = command {
            for edit in edits {
                self.sent_edits.insert(edit.position, edit.state);
                self.submitted_rechecks.insert(edit.position);
                if self.pending_edits.get(&edit.position) == Some(&edit.state) {
                    self.pending_edits.remove(&edit.position);
                }
            }
        }
    }

    fn start_next(&mut self) -> Result<()> {
        if self.in_flight.is_none() {
            if let Some(command) = self.queued.pop_front() {
                self.record_submitted_edits(&command);
                if matches!(&command, Command::Snapshot { gameplay: None }) {
                    if let Some(build) = self.pending_autosave_capture.take() {
                        self.session.stage_snapshot_gameplay(build);
                    }
                }
                self.in_flight = Some(self.session.submit(command)?);
            }
        }
        Ok(())
    }

    fn publish_maintenance_hint(&mut self) {
        self.session.set_maintenance_view(self.maintenance_view.hint());
    }
    pub(crate) fn maintenance_desired(&mut self, center: ChunkPos, radius: i32) {
        self.maintenance_view.desired(center, radius);
        self.publish_maintenance_hint();
    }
    pub(crate) fn maintenance_commit(&mut self, center: ChunkPos, radius: i32) {
        self.maintenance_view.commit(center, radius);
        self.publish_maintenance_hint();
    }
    pub(crate) fn maintenance_first_play_presented(&mut self) {
        if self.maintenance_view.first_play_presented() { self.publish_maintenance_hint(); }
    }

    pub(crate) fn request_chunks(
        &mut self,
        world: &mut World,
        mut positions: Vec<ChunkPos>,
    ) -> Result<()> {
        ensure!(
            !self.busy(),
            "Previous native chunk generation is still running"
        );
        positions.sort_unstable();
        positions.dedup();
        // An explicit offscreen request can extend the client cache before its
        // next ordinary trim; protect that extension without scanning history.
        self.maintenance_view.include_requests(&positions);
        self.publish_maintenance_hint();
        self.wanted = world
            .chunks
            .keys()
            .copied()
            .chain(positions.iter().copied())
            .collect();
        self.queue_edits(world);
        for batch in self.missing_generation_requests(world, &positions).chunks(STREAM_BATCH) {
            self.queued.push_back(Command::Generate(batch.to_vec()));
        }
        self.start_next()
    }

    fn generation_requests(&self, targets: &[ChunkPos]) -> Vec<ChunkPos> {
        let mut requests = targets.to_vec();
        let targets = targets.iter().copied().collect::<HashSet<_>>();
        let mut halo = BTreeSet::new();
        for &(x, z) in &targets {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    let p = (x + dx, z + dz);
                    if !targets.contains(&p)
                        && !self.known_chunks.contains(&p)
                        && (-1_875_000..1_875_000).contains(&p.0)
                        && (-1_875_000..1_875_000).contains(&p.1)
                    {
                        halo.insert(p);
                    }
                }
            }
        }
        // Finish the requested X/Z sequence first. Neighbor loads then let the
        // provider populate all four origins that can decorate each target.
        requests.extend(halo);
        requests
    }

    fn missing_generation_requests(&self, world: &World, targets: &[ChunkPos]) -> Vec<ChunkPos> {
        // Ready already publishes retained chunks. Requesting those again only
        // recopies blocks/light; an evicted chunk is absent from this cache and
        // still gets explicitly republished. Keep the original missing-target
        // and halo ordering, including population after interrupted loads.
        self.generation_requests(targets).into_iter()
            .filter(|p| !world.chunks.contains_key(p)).collect()
    }

    pub(crate) fn flush_overworld_edits(&mut self, world: &World) -> Result<()> {
        self.pending_edits.clear();
        self.deferred_edits.clear();
        self.submitted_rechecks.clear();
        for (&position, &state) in &world.edits {
            self.refresh_pending_edit(position, Some(state));
        }
        let edits = self
            .pending_edits
            .iter()
            .map(|(position, state)| BlockEdit {
                position: *position,
                state: *state,
                expected: None,
                notify_neighbors: false,
            })
            .collect::<Vec<_>>();
        if self.saving {
            for batch in edits.chunks(EDIT_BATCH) {
                self.queued.push_back(Command::Edits(batch.to_vec()));
            }
        } else {
            let remaining = std::mem::take(&mut self.queued);
            for batch in edits.chunks(EDIT_BATCH) {
                self.queued.push_back(Command::Edits(batch.to_vec()));
            }
            self.queued.extend(
                remaining
                    .into_iter()
                    .filter(|c| !matches!(c, Command::Edits(_))),
            );
        }
        self.start_next()
    }

    pub(crate) fn ensure_priority_chunks_blocking(
        &mut self,
        world: &mut World,
        sim: &mut crate::gameplay::Simulation,
        positions: &[ChunkPos],
    ) -> Result<std::collections::HashSet<ChunkPos>> {
        self.wanted.extend(world.chunks.keys().copied());
        let mut dirty: HashSet<ChunkPos> = self.poll(world, sim)?.into_iter().collect();
        let mut needed = Vec::new();
        let mut seen = HashSet::new();
        for &p in positions {
            if (-1_875_000..1_875_000).contains(&p.0)
                && (-1_875_000..1_875_000).contains(&p.1)
                && seen.insert(p)
                && !world.chunks.get(&p).is_some_and(|c| c.light.is_some())
            {
                needed.push(p);
            }
        }
        if needed.is_empty() {
            return Ok(dirty);
        }

        self.maintenance_view.include_requests(&needed);
        self.publish_maintenance_hint();
        self.wanted.extend(needed.iter().copied());

        if self.in_flight.is_some() {
            let event = self.session.receive()?;
            if sim.dimension != 0 {
                self.completed_detached(event)?;
            } else {
                self.completed(event, world)?;
            }
        }
        while self
            .queued
            .iter()
            .any(|command| matches!(command, Command::Snapshot { .. }))
        {
            self.start_next()?;
            let event = self.session.receive()?;
            if sim.dimension != 0 {
                self.completed_detached(event)?;
            } else {
                self.completed(event, world)?;
            }
        }

        let deferred_queue: VecDeque<Command> = std::mem::take(&mut self.queued)
            .into_iter()
            .filter(|c| !matches!(c, Command::Edits(_)))
            .collect();
        if sim.dimension == 0 {
            self.queue_edits(world);
            while let Some(command) = self.queued.pop_front() {
                let (reply, update) = self.exchange(command)?;
                Self::successful(&reply)?;
                self.apply(world, update);
            }
        }

        needed.retain(|p| !world.chunks.get(p).is_some_and(|c| c.light.is_some()));
        if !needed.is_empty() {
            let mut requests = self.missing_generation_requests(world, &needed);
            for &p in &needed {
                if !requests.contains(&p) {
                    requests.push(p);
                }
            }
            for batch in requests.chunks(STREAM_BATCH) {
                let (reply, update) = self.exchange(Command::Generate(batch.to_vec()))?;
                Self::successful(&reply)?;
                if sim.dimension != 0 {
                    self.apply_detached(update);
                } else {
                    self.apply(world, update);
                }
            }
        }

        if sim.dimension == 0 && self.adoption_dirty {
            self.adopt_metadata(world, sim);
            self.adoption_dirty = false;
        }
        dirty.extend(std::mem::take(&mut self.dirty));
        self.queued.extend(deferred_queue);
        self.start_next()?;
        Ok(dirty)
    }

    pub(crate) fn load_overworld_region_blocking(
        &mut self,
        world: &mut World,
        sim: &mut Simulation,
        center: ChunkPos,
        radius: i32,
    ) -> Result<()> {
        let mut restored = crate::world::EditJournal::default();
        for (position, state) in sim.overworld_saved_edits(world) {
            restored.insert(position, state);
        }
        world.edits = restored;
        let positions = region(center, radius);
        self.maintenance_view.include_requests(&positions);
        self.publish_maintenance_hint();
        self.wanted = world
            .chunks
            .keys()
            .copied()
            .chain(positions.iter().copied())
            .collect();

        while let Some(event) = self.session.try_event() {
            self.completed(event, world)?;
        }

        let all_lit_ready = positions.iter().all(|p| {
            !((-1_875_000..1_875_000).contains(&p.0) && (-1_875_000..1_875_000).contains(&p.1))
                || world.chunks.get(p).is_some_and(|c| c.light.is_some())
        });
        let edits_synced = world.edits.len() == self.sent_edits.len()
            && world
                .edits
                .iter()
                .all(|(pos, state)| self.sent_edits.get(pos) == Some(state));
        if all_lit_ready && edits_synced {
            let _ = world.edits.take_changes();
            if self.adoption_dirty {
                self.adopt_metadata(world, sim);
                self.adoption_dirty = false;
            }
            let loaded = world.chunks.keys().copied().collect::<Vec<_>>();
            sim.world_commands.restore_tiles(world, &loaded);
            sim.import_containers(world);
            crate::villagers::sync_import(world, sim);
            return Ok(());
        }

        if self.in_flight.is_some() {
            let event = self.session.receive()?;
            self.completed(event, world)?;
        }
        while self
            .queued
            .iter()
            .any(|command| matches!(command, Command::Snapshot { .. }))
        {
            self.start_next()?;
            let event = self.session.receive()?;
            self.completed(event, world)?;
        }
        let deferred_queue: VecDeque<Command> = std::mem::take(&mut self.queued)
            .into_iter()
            .filter(|c| !matches!(c, Command::Edits(_)))
            .collect();
        self.queue_edits(world);
        while let Some(command) = self.queued.pop_front() {
            let (reply, update) = self.exchange(command)?;
            Self::successful(&reply)?;
            self.apply(world, update);
        }
        let unlit_or_missing: Vec<ChunkPos> = positions
            .iter()
            .copied()
            .filter(|p| {
                (-1_875_000..1_875_000).contains(&p.0)
                    && (-1_875_000..1_875_000).contains(&p.1)
                    && !world.chunks.get(p).is_some_and(|c| c.light.is_some())
            })
            .collect();
        if !unlit_or_missing.is_empty() {
            let mut missing = self.missing_generation_requests(world, &unlit_or_missing);
            for &p in &unlit_or_missing {
                if !missing.contains(&p) {
                    missing.push(p);
                }
            }
            for batch in missing.chunks(STREAM_BATCH) {
                let (reply, update) = self.exchange(Command::Generate(batch.to_vec()))?;
                Self::successful(&reply)?;
                self.apply(world, update);
            }
        }
        self.adopt_metadata(world, sim);
        self.adoption_dirty = false;
        let loaded = world.chunks.keys().copied().collect::<Vec<_>>();
        sim.world_commands.restore_tiles(world, &loaded);
        sim.import_containers(world);
        crate::villagers::sync_import(world, sim);
        self.queued.extend(deferred_queue);
        self.start_next()?;
        Ok(())
    }

    fn queue_pending_overworld_edits(&mut self) {
        let edits = self
            .pending_edits
            .iter()
            .map(|(position, state)| BlockEdit {
                position: *position,
                state: *state,
                expected: None,
                notify_neighbors: false,
            })
            .collect::<Vec<_>>();
        for batch in edits.chunks(EDIT_BATCH) {
            self.queued.push_back(Command::Edits(batch.to_vec()));
        }
    }

    fn apply_detached(&mut self, mut update: Update) {
        self.generation_timings = update.generation_timings;
        if update.metadata.reset {
            for &position in &update.retained_positions {
                if self.known_chunks.insert(position) {
                    if let Some(deferred) = self.deferred_edits.remove(&position) {
                        self.pending_edits.extend(deferred);
                    }
                }
            }
        }
        for (position, _) in std::mem::take(&mut update.chunks) {
            if self.known_chunks.insert(position) {
                if let Some(deferred) = self.deferred_edits.remove(&position) {
                    self.pending_edits.extend(deferred);
                }
            }
        }
        self.collect_metadata(std::mem::take(&mut update.metadata));
    }

    fn completed_detached(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Completed {
                request_id,
                reply,
                update,
            } if self.in_flight == Some(request_id) => {
                self.in_flight = None;
                Self::successful(&reply)?;
                if matches!(reply, Reply::Saved { .. }) {
                    self.saving = false;
                }
                self.apply_detached(update);
                if !self
                    .queued
                    .iter()
                    .any(|command| matches!(command, Command::GenerateAhead { .. }))
                {
                    self.initial_view_plan.clear();
                }
            }
            Event::Failed { message, .. } => bail!("{message}"),
            other => bail!("Unexpected native generation event: {other:?}"),
        }
        Ok(())
    }

    pub(crate) fn busy(&self) -> bool {
        self.in_flight.is_some() || !self.queued.is_empty()
    }

    fn completed(&mut self, event: Event, world: &mut World) -> Result<()> {
        self.completed_profiled(event, world, &mut PublicationProfile::default())
    }

    fn completed_profiled(&mut self, event: Event, world: &mut World,
        profile: &mut PublicationProfile<'_>) -> Result<()> {
        match event {
            Event::Completed {
                request_id,
                reply,
                update,
            } if self.in_flight == Some(request_id) => {
                self.in_flight = None;
                Self::successful(&reply)?;
                if matches!(reply, Reply::Saved { .. }) { self.saving = false; }
                if profile.enabled() { self.publication_work.completed_events += 1; }
                self.apply_profiled(world, update, profile);
                if !self.queued.iter().any(|command|matches!(command,Command::GenerateAhead {..})) {
                    self.initial_view_plan.clear();
                }
            }
            Event::Failed { message, .. } => bail!("{message}"),
            other => bail!("Unexpected native generation event: {other:?}"),
        }
        Ok(())
    }

    pub(crate) fn poll(
        &mut self,
        world: &mut World,
        sim: &mut Simulation,
    ) -> Result<Vec<ChunkPos>> {
        self.poll_profiled(world, sim, None)
    }

    pub(crate) fn publication_work(&self) -> &PublicationWork { &self.publication_work }
    pub(crate) fn retained_native_chunks(&self) -> usize { self.generation_timings.retained_chunks }
    pub(crate) fn native_light_maintenance(&self) -> crate::native_session::maintenance::Metrics {
        self.generation_timings.maintenance
    }

    pub(crate) fn poll_profiled(&mut self, world: &mut World, sim: &mut Simulation,
        timings: Option<&mut FrameTimings>) -> Result<Vec<ChunkPos>> {
        if sim.dimension != 0 {
            while let Some(event) = self.session.try_event() {
                self.completed_detached(event)?;
            }
            if !self.saving
                && self.in_flight.is_none()
                && (!self.queued.is_empty() || !self.pending_edits.is_empty())
            {
                let remaining = std::mem::take(&mut self.queued);
                self.queue_pending_overworld_edits();
                self.queued.extend(
                    remaining
                        .into_iter()
                        .filter(|c| !matches!(c, Command::Edits(_))),
                );
            }
            self.start_next()?;
            return Ok(Vec::new());
        }
        let mut profile = PublicationProfile { timings, since: None };
        while let Some(event) = self.session.try_event() {
            profile.start();
            self.completed_profiled(event, world, &mut profile)?;
        }
        // Gameplay may have changed blocks while an earlier Generate was busy.
        // Insert these edits before the next generation batch, never afterward.
        if !self.saving && self.in_flight.is_none() && !self.queued.is_empty() {
            profile.start();
            if profile.enabled() {
                self.publication_work.queued_commands_rebuilt += self.queued.len() as u64;
            }
            let remaining = std::mem::take(&mut self.queued);
            let inspected = self.queue_edits(world);
            if profile.enabled() {
                self.publication_work.queued_edit_journal_entries_scanned += inspected as u64;
            }
            profile.phase("native_queue_edits");
            // Unsent edits are rebuilt from the latest state. Replaying an old
            // queued value after a newer one would corrupt subsequent terrain.
            self.queued.extend(
                remaining
                    .into_iter()
                    .filter(|c| !matches!(c, Command::Edits(_))),
            );
            profile.phase("native_queue_rebuild");
        }
        let submits = self.in_flight.is_none() && !self.queued.is_empty();
        if submits { profile.start(); }
        self.start_next()?;
        if submits { profile.phase("native_submit"); }
        if self.adoption_dirty {
            profile.start();
            self.adopt_metadata_profiled(world, sim, &mut profile);
            self.adoption_dirty = false;
        }
        Ok(std::mem::take(&mut self.dirty).into_iter().collect())
    }

    pub(crate) fn drain(
        &mut self,
        world: &mut World,
        sim: &mut Simulation,
    ) -> Result<Vec<ChunkPos>> {
        if sim.dimension != 0 {
            loop {
                self.poll(world, sim)?;
                if !self.busy() {
                    break;
                }
                let event = self.session.receive()?;
                self.completed_detached(event)?;
            }
            return Ok(Vec::new());
        }
        let mut dirty = BTreeSet::new();
        loop {
            dirty.extend(self.poll(world, sim)?);
            if !self.busy() {
                break;
            }
            let event = self.session.receive()?;
            if sim.dimension != 0 {
                self.completed_detached(event)?;
            } else {
                self.completed(event, world)?;
            }
        }
        Ok(dirty.into_iter().collect())
    }

    pub(crate) fn save(
        &mut self,
        world: &mut World,
        pose: PlayerSave,
        sim: &mut Simulation,
    ) -> Result<()> {
        if sim.dimension != 0 {
            sim.snapshot_active_dimension_for_save(world);
            if self.in_flight.is_some() {
                let event = self.session.receive()?;
                self.completed_detached(event)?;
            }
            while self
                .queued
                .iter()
                .any(|command| matches!(command, Command::Snapshot { .. }))
            {
                self.start_next()?;
                let event = self.session.receive()?;
                self.completed_detached(event)?;
            }
            if self.initial_view_pending() {
                let remaining = std::mem::take(&mut self.queued)
                    .into_iter()
                    .filter(|command| !matches!(command, Command::Edits(_)))
                    .collect();
                self.queue_pending_overworld_edits();
                let edits = std::mem::replace(&mut self.queued, remaining);
                for command in edits {
                    let (reply, update) = self.exchange(command)?;
                    Self::successful(&reply)?;
                    self.apply_detached(update);
                }
            } else {
                self.queue_pending_overworld_edits();
                self.drain(world, sim)?;
                self.queue_pending_overworld_edits();
                self.drain(world, sim)?;
            }
            if !self.persistent {
                return Ok(());
            }
            let save = self.capture(world, pose, sim)?;
            let (reply, update) = self.exchange(Command::Snapshot {
                gameplay: Some(save),
            })?;
            Self::successful(&reply)?;
            self.apply_detached(update);
            return Ok(());
        }
        // Gameplay is paused by the caller. A new world may still have many
        // seconds of distant terrain queued; its durable request plan lets us
        // save the completed prefix instead of blocking exit on that whole view.
        if self.initial_view_pending() {
            self.finish_initial_view_save_prefix(world, sim)?;
        } else {
            self.queue_edits(world);
            let dirty = self.drain(world, sim)?;
            self.dirty.extend(dirty);
            // A preserved autosave can submit an older captured value after
            // the first pass consumed a revert equal to the previous sent
            // baseline. With no trailing terrain request, drain has no later
            // queue boundary. Reconcile once more while gameplay is paused,
            // before capturing the final native/gameplay checkpoint together.
            self.queue_edits(world);
            let dirty = self.drain(world, sim)?;
            self.dirty.extend(dirty);
        }
        if !self.persistent {
            return Ok(());
        }
        let save = self.capture(world, pose, sim)?;
        let (reply, update) = self.exchange(Command::Snapshot {
            gameplay: Some(save),
        })?;
        Self::successful(&reply)?;
        self.apply(world, update);
        Ok(())
    }

    fn finish_initial_view_save_prefix(&mut self, world: &mut World, sim: &mut Simulation) -> Result<()> {
        // Consume only the already submitted request, without poll() starting
        // another terrain batch. Its publications precede the new capture.
        if self.in_flight.is_some() {
            let event = self.session.receive()?;
            self.completed(event, world)?;
        }
        // An earlier autosave owns its captured edits until its barrier has
        // completed. Preserve that command order and do not silently discard a
        // queued snapshot when replacing stale unsent edits below.
        while self.queued.iter().any(|command| matches!(command, Command::Snapshot { .. })) {
            self.start_next()?;
            let event = self.session.receive()?;
            self.completed(event, world)?;
        }
        let remaining = std::mem::take(&mut self.queued).into_iter()
            .filter(|command| !matches!(command, Command::Edits(_))).collect();
        self.queue_edits(world);
        let edits = std::mem::replace(&mut self.queued, remaining);
        for command in edits {
            let (reply, update) = self.exchange(command)?;
            Self::successful(&reply)?;
            self.apply(world, update);
        }
        if self.adoption_dirty {
            self.adopt_metadata(world, sim);
            self.adoption_dirty = false;
        }
        // Leave the remaining generation queued. The next poll resumes it if
        // gameplay continues; closing the world need not join one extra batch.
        Ok(())
    }

    /// Capture one gameplay instant and enqueue its matching edits and durable
    /// save. Never wait on terrain generation, compression or disk from a frame.
    pub(crate) fn autosave(&mut self, world: &mut World, pose: PlayerSave, sim: &Simulation) -> Result<bool> {
        if !self.persistent { return Ok(true); }
        if self.saving { return Ok(false); }
        let mut save = self.capture_save(world, pose, sim);
        self.pending_autosave_capture = Some(Box::new(move || {
            save.edits.sort_unstable_by_key(|(p, _)| *p);
            Ok(serde_json::to_value(save)?)
        }));
        let remaining = std::mem::take(&mut self.queued);
        if sim.dimension != 0 {
            self.queue_pending_overworld_edits();
        } else {
            self.queue_edits(world);
        }
        self.queued.push_back(Command::Snapshot { gameplay: None });
        // Pending generation can resume after this snapshot. Queued edits are
        // replaced by the captured current values above, never replayed later.
        self.queued.extend(remaining.into_iter().filter(|c| !matches!(c, Command::Edits(_))));
        // poll must not substitute newer gameplay edits before the save barrier.
        self.saving = true;
        self.start_next()?;
        Ok(true)
    }

    fn capture_save(&self, world: &World, pose: PlayerSave, sim: &Simulation) -> GameplaySave {
        let (simulation, edits) = if sim.dimension != 0 {
            let mut snap = sim.clone();
            snap.snapshot_active_dimension_for_save(world);
            let edits = snap.overworld_saved_edits(world);
            (snap, edits)
        } else {
            let edits = world
                .edits
                .iter()
                .map(|(p, s)| (*p, *s))
                .collect::<Vec<_>>();
            (sim.clone(), edits)
        };
        GameplaySave {
            version: SAVE_VERSION,
            position: pose.position,
            motion: pose.motion,
            simulation,
            edits,
            seen_spawns: self.seen_spawns.clone(),
            seen_containers: self.seen_containers.clone(),
            seen_spawners: self.seen_spawners.clone(),
            pending_spawns: self.pending_spawns.values().cloned().collect(),
            pending_containers: self.pending_containers.values().cloned().collect(),
            pending_spawners: self
                .pending_spawners
                .iter()
                .map(|(&p, e)| (p, e.clone()))
                .collect(),
            initial_view_plan: self.initial_view_plan.clone(),
        }
    }

    fn capture(&self, world: &World, pose: PlayerSave, sim: &Simulation) -> Result<serde_json::Value> {
        let mut save = self.capture_save(world, pose, sim);
        save.edits.sort_unstable_by_key(|(p, _)| *p);
        Ok(serde_json::to_value(save)?)
    }

    fn apply(&mut self, world: &mut World, update: Update) {
        self.apply_profiled(world, update, &mut PublicationProfile::default());
    }

    fn apply_profiled(&mut self, world: &mut World, mut update: Update,
        profile: &mut PublicationProfile<'_>) {
        self.generation_timings = update.generation_timings;
        if update.metadata.reset {
            for &position in &update.retained_positions {
                if self.known_chunks.insert(position) {
                    if let Some(deferred) = self.deferred_edits.remove(&position) {
                        self.pending_edits.extend(deferred);
                    }
                }
            }
        }
        let mut chunks = std::mem::take(&mut update.chunks);
        if profile.enabled() {
            self.publication_work.incoming_chunks += chunks.len() as u64;
            self.publication_work.max_incoming_chunks = self.publication_work.max_incoming_chunks.max(chunks.len());
        }
        for (position, _) in &chunks {
            if self.known_chunks.insert(*position) {
                // Even an invisible halo makes saved/migration edits writable.
                // Their latest mutations are reconciled by queue_edits before
                // submitting; a save barrier still freezes captured commands.
                if let Some(deferred) = self.deferred_edits.remove(position) {
                    self.pending_edits.extend(deferred);
                }
            }
        }
        // Halo publications and acknowledgements need their metadata, but no
        // visible replacement. Filter them before scanning the entire retained
        // edit journal or grouping overlays that would immediately be dropped.
        chunks.retain(|(p, _)| self.wanted.contains(p) || world.chunks.contains_key(p));
        if profile.enabled() { self.publication_work.accepted_chunks += chunks.len() as u64; }
        profile.phase("native_apply_filter");
        if chunks.is_empty() {
            self.collect_metadata(std::mem::take(&mut update.metadata));
            profile.phase("native_metadata_collect");
            drop(chunks);
            drop(update);
            profile.phase("native_update_release");
            return;
        }
        let incoming = chunks.iter().map(|(p, _)| *p).collect::<HashSet<_>>();
        let mut overlays: HashMap<ChunkPos, Vec<([i32; 3], u16)>> = HashMap::new();
        let (overlap, index_scanned) = world.edits.intersects_columns(incoming.iter().copied());
        let (already_matches, candidates_examined) = if overlap {
            world.edits.matches_published_columns(chunks.iter().map(|(p,c)| (*p,c.as_ref())))
        } else { (false,0) };
        if profile.enabled() {
            self.publication_work.overlay_index_entries_scanned += index_scanned as u64;
            self.publication_work.overlay_candidate_entries_examined += candidates_examined as u64;
            if overlap && !already_matches {
                self.publication_work.overlay_journal_entries_scanned += world.edits.len() as u64;
            } else {
                self.publication_work.overlay_scans_skipped += 1;
                if already_matches { self.publication_work.overlay_matching_publications_skipped += 1; }
            }
        }
        if overlap && !already_matches {
            // Membership only rejects an empty intersection. Preserve the
            // original full-map order, including hopper insertion order.
            for (&p, &state) in &world.edits {
                let cp = (p[0].div_euclid(16), p[2].div_euclid(16));
                if incoming.contains(&cp) && (0..256).contains(&p[1]) {
                    overlays.entry(cp).or_default().push((p, state));
                }
            }
        }
        profile.phase("native_overlay_scan");
        let mut replaced = Vec::new();
        for (position, mut chunk) in chunks {
            // A publication can race local mining/placement. Keep every latest
            // local edit, and recalculate light for any differing snapshot.
            let mut overlaid = false;
            if let Some(edits) = overlays.get(&position) {
                for &(p, state) in edits {
                    if chunk.get(p[0].rem_euclid(16), p[1], p[2].rem_euclid(16)) != state {
                        if profile.enabled() { self.publication_work.overlaid_blocks += 1; }
                        Arc::make_mut(&mut chunk).set(
                            p[0].rem_euclid(16),
                            p[1],
                            p[2].rem_euclid(16),
                            state,
                        );
                        overlaid = true;
                    }
                }
            }
            if overlaid {
                let c = Arc::make_mut(&mut chunk);
                c.light = None;
                c.light_edits.clear();
            }
            if world.chunks.get(&position).is_some_and(|old| Arc::ptr_eq(old, &chunk)) {
                if profile.enabled() { self.publication_work.same_snapshot_chunks += 1; }
                continue;
            }
            if profile.enabled() {
                self.publication_work.replaced_chunks += 1;
                if world.chunks.get(&position).is_some_and(|old| Arc::strong_count(old) == 1) {
                    self.publication_work.unique_old_chunks_at_replacement += 1;
                }
            }
            world.chunks.insert(position, chunk);
            replaced.push(position);
        }
        if !replaced.is_empty() {
            self.adoption_dirty = true;
            world.invalidate_lighting();
            self.dirty.extend(replaced);
            self.dirty.extend(
                std::mem::take(&mut update.remesh)
                    .into_iter()
                    .filter(|p| world.chunks.contains_key(p)),
            );
        }
        profile.phase("native_replace_and_drop");
        self.collect_metadata(std::mem::take(&mut update.metadata));
        profile.phase("native_metadata_collect");
        drop(overlays);
        drop(incoming);
        profile.phase("native_apply_scratch_release");
        drop(update);
        profile.phase("native_update_release");
    }

    fn collect_metadata(&mut self, metadata: MetadataUpdate) {
        self.adoption_dirty |= !metadata.spawned.is_empty()
            || !metadata.containers.is_empty()
            || !metadata.spawners.is_empty();
        for record in metadata.spawned {
            let key = (record.family, record.index);
            if !self.seen_spawns.contains(&key) {
                self.pending_spawns.entry(key).or_insert(record);
            }
        }
        for tile in metadata.containers {
            if !self.seen_containers.contains(&tile.position) {
                self.pending_containers.insert(tile.position, tile);
            }
        }
        for position in metadata.removed_containers {
            self.pending_containers.remove(&position);
        }
        for (position, entity) in metadata.spawners {
            if !self.seen_spawners.contains(&position) {
                self.pending_spawners.insert(position, entity);
            }
        }
        for position in metadata.removed_spawners {
            self.pending_spawners.remove(&position);
        }
    }

    fn adopt_metadata(&mut self, world: &mut World, sim: &mut Simulation) {
        self.adopt_metadata_profiled(world, sim, &mut PublicationProfile::default());
    }

    fn adopt_metadata_profiled(&mut self, world: &mut World, sim: &mut Simulation,
        profile: &mut PublicationProfile<'_>) {
        if profile.enabled() {
            self.publication_work.dirty_columns_for_tile_restore += self.dirty.len() as u64;
            self.publication_work.pending_containers_examined += self.pending_containers.len() as u64;
            self.publication_work.pending_spawns_examined += self.pending_spawns.len() as u64;
        }
        let mut restore_columns = self.dirty.clone();
        let ready_spawners = self
            .pending_spawners
            .keys()
            .copied()
            .filter(|p| {
                world
                    .chunks
                    .contains_key(&(p[0].div_euclid(16), p[2].div_euclid(16)))
            })
            .collect::<Vec<_>>();
        for position in ready_spawners {
            let entity = self.pending_spawners.remove(&position).unwrap();
            self.seen_spawners.insert(position);
            let p = IVec3::from_array(position);
            if world.get(p) >> 4 != 52 {
                continue;
            }
            let cp = (p.x.div_euclid(16), p.z.div_euclid(16));
            if let Some(chunk) = world.chunks.get_mut(&cp) {
                let local = [p.x.rem_euclid(16), p.y, p.z.rem_euclid(16)];
                let chunk = Arc::make_mut(chunk);
                let existing = chunk.tiles.get(&local).map(|t| t.as_ref());
                if let Some(mut tag) =
                    crate::vanilla::middle_pick::tile_snapshot(52, p, existing, None)
                {
                    if let crate::vanilla::drops::DropNbt::Compound(fields) = &mut tag {
                        fields.insert(
                            "EntityId".into(),
                            crate::vanilla::drops::DropNbt::String(entity),
                        );
                    }
                    chunk.tiles.insert(local, Arc::new(tag));
                }
                restore_columns.insert(cp);
            }
        }
        let chunks = restore_columns.into_iter().collect::<Vec<_>>();
        sim.world_commands.restore_tiles(world, &chunks);
        profile.phase("native_restore_tiles");
        let ready = self
            .pending_containers
            .keys()
            .copied()
            .filter(|p| {
                world
                    .chunks
                    .contains_key(&(p[0].div_euclid(16), p[2].div_euclid(16)))
            })
            .collect::<Vec<_>>();
        for position in ready {
            let tile = self.pending_containers.remove(&position).unwrap();
            self.seen_containers.insert(position);
            let p = IVec3::from_array(position);
            if world.get(p) >> 4 != tile.block {
                continue;
            }
            if let Some(mut container) = crate::container::Container::for_block(tile.block) {
                container.slots = tile.slots;
                sim.containers.entry(container_key(p)).or_insert(container);
            }
        }
        profile.phase("native_adopt_containers");
        let ready = self
            .pending_spawns
            .iter()
            .filter_map(|(&key, record)| {
                spawn_position(&record.spawn)
                    .filter(|p| world.contains(*p))
                    .map(|_| key)
            })
            .collect::<Vec<_>>();
        let mut spawns = Vec::new();
        for key in ready {
            self.seen_spawns.insert(key);
            let mut record = self.pending_spawns.remove(&key).unwrap();
            // Reserve a separate stable ID space from ordinary gameplay mobs.
            record.family |= 0x40;
            spawns.push(record);
        }
        crate::client::native_view::display_spawns(world, sim, spawns);
        profile.phase("native_adopt_spawns");
    }
}

fn spawn_position(spawn: &GeneratedSpawn) -> Option<Vec3> {
    Some(match spawn {
        GeneratedSpawn::Villager(v) => IVec3::from_array(v.position).as_vec3() + Vec3::splat(0.5),
        GeneratedSpawn::ChestMinecart(c) => glam::DVec3::from_array(c.position).as_vec3(),
        GeneratedSpawn::Animal(a) => glam::DVec3::from_array(a.position).as_vec3(),
        GeneratedSpawn::Witch(p) | GeneratedSpawn::ElderGuardian(p) => {
            IVec3::from_array(*p).as_vec3()
        }
        GeneratedSpawn::Feature(e)
            if e.kind == crate::vanilla::population::GeneratedEntityKind::EnderDragon =>
        {
            glam::DVec3::from_array(e.position).as_vec3()
        }
        // Keep unsupported generated entities in the checkpoint for later adoption.
        _ => return None,
    })
}
