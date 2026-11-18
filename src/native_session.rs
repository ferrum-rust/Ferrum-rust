//! Serial owner of an opt-in native world. Runtime construction, provider loads,
//! edits, scheduled ticks and checkpoints all run on one worker. The caller owns
//! player/entity gameplay and the initial spawn/load sequence; this does not
//! substitute the legacy simulation's block scheduler for the native scheduler.
// The host is verified separately before the gameplay caller is enabled.
#![allow(dead_code)]
use crate::vanilla::server_teleport::{Admission, Correction, Gate, Location, MovementPacket};
use crate::{
    vanilla::{
        drops::DropEvent,
        item_entity::ItemEntity,
        mineshaft::ChestMinecart,
        population::GeneratedEntity,
        runtime::{
            EntityEffect, FallingEffect, FallingEntry, InventoryAction, InventoryReceipt,
            InventoryStamp, InventoryView, PlayerFrame, Runtime, RuntimeSnapshot,
            VanillaPopulation,
        },
        spawning::AnimalSpawn,
        village::VillagerSpawn,
    },
    world::{Chunk, ChunkPos},
};
use anyhow::{anyhow, ensure, Context, Result};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use glam::IVec3;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{hash_map::RandomState, BTreeMap, BTreeSet, VecDeque},
    fs::{self, File, OpenOptions},
    hash::BuildHasher,
    io::{BufReader, BufWriter, Read},
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
    thread::{self, JoinHandle},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[path = "native_session_maintenance.rs"]
pub(crate) mod maintenance;

const FORMAT: &str = "ferrum-native-session";
const GENERATOR: u8 = 4;
const QUEUE_CAPACITY: usize = 64;

#[derive(Clone, Debug)]
pub struct Config {
    pub seed: i64,
    pub world_id: String,
    pub save_root: PathBuf,
    pub map_features: bool,
    /// Must only be enabled after the native scheduled-update oracle passes.
    /// Unsupported entity/block tick callbacks still return an explicit error.
    pub enable_scheduled_ticks: bool,
    /// Optional Chebyshev radius around the active saved entry chunk for Ready
    /// chunk publication on resume; `None` publishes every retained chunk.
    pub resume_publish_radius: Option<i32>,
}
impl Config {
    pub fn new(seed: i64, world_id: impl Into<String>, save_root: impl Into<PathBuf>) -> Self {
        Self {
            seed,
            world_id: world_id.into(),
            save_root: save_root.into(),
            map_features: true,
            enable_scheduled_ticks: false,
            resume_publish_radius: None,
        }
    }
    pub fn checkpoint_path(&self) -> Result<PathBuf> {
        ensure!(
            !self.world_id.is_empty()
                && self.world_id.len() <= 80
                && self
                    .world_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "Native world ID must contain 1–80 ASCII letters, digits, '-' or '_'"
        );
        let root = if self.save_root.is_absolute() {
            self.save_root.clone()
        } else {
            std::env::current_dir()?.join(&self.save_root)
        };
        // Generator 1/2 overlays and generator 3 terrain previews never use
        // this directory or checkpoint format, even with the same world ID.
        Ok(root
            .join("native-v4")
            .join(&self.world_id)
            .join("world.json.gz"))
    }
}

#[derive(Clone, Debug)]
pub struct BlockEdit {
    pub position: [i32; 3],
    pub state: u16,
    /// Optional compare-and-set guard against a stale client-side block view.
    pub expected: Option<u16>,
    pub notify_neighbors: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RejectedEdit {
    pub position: [i32; 3],
    pub expected: u16,
    pub actual: u16,
}
#[derive(Debug)]
pub enum Command {
    /// Choose (or restore) the original world spawn and begin its initial load.
    PrepareSpawn,
    /// Continue the prepared region's original X/Z order on the world owner.
    LoadSpawnBatch {
        count: u16,
    },
    /// Fresh progressive startup only: retain dirty block/light snapshots
    /// until the original 625-column barrier completes. Progress and all other
    /// metadata are still published after each ordinary generation batch.
    LoadSpawnBatchDeferred {
        count: u16,
    },
    /// Immutable creation pose. Current saved gameplay pose is separate.
    PreparePlayer {
        adventure: bool,
    },
    /// Spawn one original XP orb (e.g. a completed trade); mining/drop logic
    /// submits its own DropEvent synchronously on this same owner.
    SpawnExperience {
        position: [f64; 3],
        value: i32,
    },
    UseBlock(crate::vanilla::runtime::UseBlock),
    DigBlock(crate::vanilla::runtime::DigBlock),
    ReadTile {position:[i32;3]},
    /// Move the authoritative MP frame and publish the original S08 payload.
    CorrectPlayer {
        epoch: u64,
        correction: Correction,
    },
    /// C03 admission only. An admitted packet can proceed to MP body processing.
    ConfirmPosition {
        epoch: u64,
        packet: MovementPacket,
    },
    /// Supply the player pose used during this owner's next entity tick.
    PlayerFrame {
        epoch: u64,
        frame: PlayerFrame,
    },
    /// Order and duplicates are preserved: changing the request order changes
    /// the original population sequence. There is no parallel stateless path.
    Generate(Vec<ChunkPos>),
    /// Startup only: prepare bounded pure terrain for following requests while
    /// this batch is populated. `ahead` never publishes a provider request.
    GenerateAhead {
        positions: Vec<ChunkPos>,
        ahead: Vec<ChunkPos>,
    },
    Edits(Vec<BlockEdit>),
    Inventory {
        stamp: InventoryStamp,
        action: InventoryAction,
    },
    /// Apply the world owner's difficulty, rain state and doFireTick rule.
    SetFireEnvironment(crate::vanilla::runtime::FireEnvironment),
    Tick {
        steps: u32,
    },
    /// Opaque caller-owned player/entity/inventory state is committed in the
    /// same atomic file as the authoritative world. None keeps the last value.
    Snapshot {
        gameplay: Option<Value>,
    },
    Shutdown {
        gameplay: Option<Value>,
    },
}

pub use crate::vanilla::runtime::LiveContainer as NativeContainer;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum GeneratedSpawn {
    Villager(VillagerSpawn),
    ChestMinecart(ChestMinecart),
    Witch([i32; 3]),
    ElderGuardian([i32; 3]),
    Animal(AnimalSpawn),
    Feature(GeneratedEntity),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpawnRecord {
    /// Stable publication identity, not a Minecraft network entity ID.
    pub family: u8,
    pub index: u64,
    pub spawn: GeneratedSpawn,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DropRecord {
    /// Stable retained-event identity; callers can persist consumption to avoid
    /// respawning a previously collected item when generation metadata reloads.
    pub index: u64,
    pub event: DropEvent,
    /// Live items are already simulated by this owner. Legacy requests retain
    /// their previous consumer when restoring an older checkpoint.
    #[serde(default)]
    pub owned_by_runtime: bool,
}
#[derive(Clone, Debug, Default)]
pub struct MetadataUpdate {
    /// Ready/resume supplies all retained generation metadata. The gameplay
    /// payload can contain subsequent entity death/movement/inventory changes.
    pub reset: bool,
    pub containers: Vec<NativeContainer>,
    pub removed_containers: Vec<[i32; 3]>,
    pub spawners: Vec<([i32; 3], String)>,
    pub removed_spawners: Vec<[i32; 3]>,
    pub spawned: Vec<SpawnRecord>,
    pub drops: Vec<DropRecord>,
}
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct GenerationTimings {
    /// Owner command time, including terrain preparation and serial callbacks.
    pub generate_ms: f64,
    pub publication_ms: f64,
    /// Authoritative columns retained by the owner, including off-screen history.
    /// This is distinct from the bounded client view and distant visual tiles.
    pub retained_chunks: usize,
    pub terrain: crate::vanilla::runtime::TerrainTimings,
    pub runtime: crate::vanilla::runtime::RuntimeTimings,
    /// Cumulative owner maintenance; a pass becomes visible in the next ordinary publication.
    pub maintenance: maintenance::Metrics,
}
#[derive(Clone, Default)]
pub struct Update {
    pub generation_timings: GenerationTimings,
    pub chunks: Vec<(ChunkPos, Arc<Chunk>)>,
    /// Authoritative chunk positions retained by the server on `metadata.reset`.
    pub retained_positions: Vec<ChunkPos>,
    /// Includes surrounding meshes whose edge/corner AO or light may change.
    pub remesh: Vec<ChunkPos>,
    pub metadata: MetadataUpdate,
    pub inventory: Option<InventoryView>,
    pub falling: Vec<FallingEntry>,
    pub falling_effects: Vec<FallingEffect>,
    pub items: Vec<ItemEntity>,
    pub orbs: Vec<crate::gameplay::xp_orb::Orb>,
    pub experience: crate::gameplay::xp_orb::motion::Experience,
    pub entity_effects: Vec<EntityEffect>,
    pub player_frame: Option<PlayerFrame>,
    pub world_time: i64,
}
impl std::fmt::Debug for Update {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Update")
            .field(
                "chunks",
                &self.chunks.iter().map(|(p, _)| p).collect::<Vec<_>>(),
            )
            .field("remesh", &self.remesh)
            .field("metadata", &self.metadata)
            .field("falling_count", &self.falling.len())
            .field("falling_effects", &self.falling_effects)
            .field("items", &self.items.len())
            .field("entity_effects", &self.entity_effects.len())
            .field("world_time", &self.world_time)
            .finish()
    }
}
#[derive(Clone, Debug)]
pub enum Reply {
    Inventory(InventoryReceipt),
    SpawnPrepared {
        point: [i32; 3],
        initial_region: Vec<ChunkPos>,
    },
    SpawnAreaProgress {
        loaded: usize,
    },
    PlayerPrepared {
        initial: crate::vanilla::runtime::InitialPlayer,
    },
    PlayerFrame {
        accepted: bool,
    },
    ExperienceSpawned,
    PlayerCorrection {
        payload: Vec<u8>,
    },
    PositionAdmission(Admission),
    PlayerUnavailable,
    UsedBlock(crate::vanilla::runtime::UseReceipt),
    Dug(crate::vanilla::runtime::DigReceipt),
    Tile {position:[i32;3],tag:Option<Arc<crate::vanilla::drops::DropNbt>>},
    Generated,
    Edited {
        rejected: Vec<RejectedEdit>,
    },
    FireEnvironmentSet,
    AwaitingTnt {
        completed_steps: u32,
        pending: Vec<crate::vanilla::runtime::PendingTnt>,
    },
    Ticked {
        steps: u32,
    },
    Saved {
        path: PathBuf,
    },
    ShuttingDown {
        path: PathBuf,
    },
}
#[derive(Clone, Debug)]
pub enum Event {
    Ready {
        resumed: bool,
        path: Option<PathBuf>,
        gameplay: Value,
        update: Update,
    },
    Completed {
        request_id: u64,
        reply: Reply,
        update: Update,
    },
    /// A failed native callback can have partially changed the world, just as
    /// a vanilla generation exception can. Stop instead of saving that state.
    Failed {
        request_id: Option<u64>,
        message: String,
    },
    Stopped,
}
struct Envelope {
    id: u64,
    command: Command,
    deferred_gameplay: Option<Box<dyn FnOnce() -> Result<Value> + Send>>,
    maintenance_view: Option<maintenance::ViewHint>,
}
pub struct NativeSession {
    sender: Option<mpsc::SyncSender<Envelope>>,
    receiver: mpsc::Receiver<Event>,
    next_id: u64,
    terminal: bool,
    closing: bool,
    scheduled_ticks: bool,
    persistent: bool,
    worker: Option<JoinHandle<()>>,
    maintenance_view: Option<maintenance::ViewHint>,
    staged_gameplay: VecDeque<Box<dyn FnOnce() -> Result<Value> + Send>>,
}
impl Drop for NativeSession {
    fn drop(&mut self) {
        // Reopening a just-closed world must not race the prior worker's lock.
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl NativeSession {
    /// Returns immediately after spawning. File loading and Runtime creation
    /// happen on the worker; the first event is Ready or Failed.
    pub fn spawn(config: Config) -> Result<Self> {
        Self::spawn_inner(config, None)
    }
    /// The same serial world host for reproducible rendering and import QA.
    /// It creates no directories, locks, or checkpoint files.
    pub fn in_memory(seed: i64, world_random_seed: i64) -> Result<Self> {
        Self::spawn_inner(
            Config::new(seed, "preview", PathBuf::new()),
            Some(world_random_seed),
        )
    }
    /// Disposable generation session with the same structure settings as a
    /// playable world. No checkpoint directory or save lock is created.
    pub fn in_memory_with_config(config: Config, world_random_seed: i64) -> Result<Self> {
        Self::spawn_inner(config, Some(world_random_seed))
    }
    fn spawn_inner(config: Config, reference_seed: Option<i64>) -> Result<Self> {
        let persistent = reference_seed.is_none();
        let path = if persistent {
            config.checkpoint_path()?
        } else {
            PathBuf::new()
        };
        let scheduled_ticks = config.enable_scheduled_ticks;
        let (sender, requests) = mpsc::sync_channel(QUEUE_CAPACITY);
        // At most 64 queued commands can produce updates while the UI stalls.
        // An unbounded result channel guarantees terminal errors are observable.
        let (events, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("native-world-v4".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_worker(config, path, reference_seed, requests, &events)
                }));
                match result {
                    Ok(Ok(())) => {
                        let _ = events.send(Event::Stopped);
                    }
                    Ok(Err(error)) => {
                        let _ = events.send(Event::Failed {
                            request_id: None,
                            message: format!("{error:#}"),
                        });
                    }
                    Err(panic) => {
                        let message = panic
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_string()))
                            .unwrap_or_else(|| "Native world worker panicked".into());
                        let _ = events.send(Event::Failed {
                            request_id: None,
                            message,
                        });
                    }
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            receiver,
            next_id: 1,
            terminal: false,
            closing: false,
            scheduled_ticks,
            persistent,
            worker: Some(worker),
            maintenance_view: None,
            staged_gameplay: VecDeque::new(),
        })
    }
    pub(crate) fn set_maintenance_view(&mut self, view: Option<maintenance::ViewHint>) {
        self.maintenance_view = view;
    }
    pub(crate) fn stage_snapshot_gameplay(
        &mut self,
        build: impl FnOnce() -> Result<Value> + Send + 'static,
    ) {
        self.staged_gameplay.push_back(Box::new(build));
    }
    pub fn submit(&mut self, command: Command) -> Result<u64> {
        validate_command(&command)?;
        ensure!(
            self.persistent
                || !matches!(command, Command::Snapshot { .. } | Command::Shutdown { .. }),
            "In-memory session has no durable checkpoint; use join to close it"
        );
        ensure!(!self.terminal, "Native world worker is stopped");
        ensure!(
            !self.closing,
            "Native world shutdown has already been queued"
        );
        ensure!(
            !matches!(command, Command::Tick { .. }) || self.scheduled_ticks,
            "Native scheduled ticks are not enabled for this session"
        );
        let id = self.next_id;
        let next_id = id
            .checked_add(1)
            .ok_or_else(|| anyhow!("Native request ID overflow"))?;
        let closing = matches!(command, Command::Shutdown { .. });
        let deferred_gameplay = if matches!(&command, Command::Snapshot { gameplay: None }) {
            self.staged_gameplay.pop_front()
        } else {
            None
        };
        let sender = self
            .sender
            .as_ref()
            .ok_or_else(|| anyhow!("Native session is closing"))?;
        if let Err(error) = sender.try_send(Envelope {
            id,
            command,
            deferred_gameplay,
            maintenance_view: self.maintenance_view,
        }) {
            match error {
                mpsc::TrySendError::Full(mut env) => {
                    if let Some(build) = env.deferred_gameplay.take() {
                        self.staged_gameplay.push_front(build);
                    }
                    return Err(anyhow!(
                        "Native world request queue is full; retry after receiving updates"
                    ));
                }
                mpsc::TrySendError::Disconnected(_) => {
                    return Err(anyhow!("Native world worker is disconnected"));
                }
            }
        }
        self.next_id = next_id;
        self.closing = closing;
        Ok(id)
    }
    pub fn try_event(&mut self) -> Option<Event> {
        match self.receiver.try_recv() {
            Ok(event) => {
                self.terminal |= matches!(event, Event::Failed { .. } | Event::Stopped);
                Some(event)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) if !self.terminal => {
                self.terminal = true;
                Some(Event::Failed {
                    request_id: None,
                    message: "Native world worker disconnected without a terminal result".into(),
                })
            }
            Err(mpsc::TryRecvError::Disconnected) => None,
        }
    }
    /// Blocking receive for the loading worker. Interactive frames use try_event.
    pub fn receive(&mut self) -> Result<Event> {
        let event = self.receiver.recv().context("Native worker disconnected")?;
        self.terminal |= matches!(event, Event::Failed { .. } | Event::Stopped);
        Ok(event)
    }
    /// Use after Shutdown completes to report worker panics. Drop also joins
    /// the worker, but only Snapshot/Shutdown provides a durable-save barrier.
    pub fn join(mut self) -> Result<()> {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| anyhow!("Native world worker panicked"))?;
        }
        Ok(())
    }
}
fn validate_command(command: &Command) -> Result<()> {
    match command {
        Command::CorrectPlayer { correction, .. } => correction.validate()?,
        Command::SpawnExperience { position, value } => ensure!(
            *value > 0
                && position
                    .iter()
                    .all(|v| v.is_finite() && v.abs() <= 30_000_000.),
            "Invalid experience spawn"
        ),
        Command::PlayerFrame { frame, .. } => frame.validate()?,
        Command::LoadSpawnBatch { count } | Command::LoadSpawnBatchDeferred { count } => ensure!(
            (1..=256).contains(count),
            "Initial spawn batch size must be 1–256"
        ),
        Command::Generate(positions) | Command::GenerateAhead { positions, .. } => {
            ensure!(
                positions.len() <= 256,
                "Generate accepts at most 256 ordered chunks per request"
            );
            ensure!(
                positions
                    .iter()
                    .all(|&(x, z)| (-1_875_000..1_875_000).contains(&x)
                        && (-1_875_000..1_875_000).contains(&z)),
                "Chunk request exceeds the original world coordinate limits"
            );
            if let Command::GenerateAhead { ahead, .. } = command {
                ensure!(ahead.len() <= 64, "Terrain lookahead accepts at most 64 chunks");
                ensure!(ahead.iter().all(|&(x,z)| (-1_875_000..1_875_000).contains(&x)
                    && (-1_875_000..1_875_000).contains(&z)),
                    "Terrain lookahead exceeds original world coordinate limits");
            }
        }
        Command::Edits(edits) => {
            ensure!(edits.len() <= 65536, "Too many edits in one native command");
            ensure!(
                edits.iter().all(|e| (0..256).contains(&e.position[1])
                    && (-30_000_000..30_000_000).contains(&e.position[0])
                    && (-30_000_000..30_000_000).contains(&e.position[2])),
                "Block edit exceeds the original world coordinate limits"
            );
        }
        Command::SetFireEnvironment(environment) => {
            ensure!(environment.difficulty <= 3, "Invalid native fire difficulty");
        }
        Command::Tick { steps } => ensure!((1..=1000).contains(steps), "Tick accepts 1–1000 steps"),
        _ => {}
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct Checkpoint {
    format: String,
    generator: u8,
    world_id: String,
    seed: i64,
    gameplay: Value,
    runtime: RuntimeSnapshot,
}
struct Worker {
    inventory_epoch: u64,
    movement_gate: Gate,
    network_tick: i32,
    disconnected: bool,
    spawn_load_cursor: Option<usize>,
    fresh_spawn_publication: bool,
    maintenance_cursor: maintenance::Cursor,
    maintenance_policy: maintenance::Policy,
    _lock: Option<File>,
    config: Config,
    path: PathBuf,
    runtime: Runtime,
    gameplay: Value,
    published: Publication,
    generation_timings: GenerationTimings,
}
impl Worker {
    fn open(config: Config, path: PathBuf, reference_seed: Option<i64>) -> Result<(Self, bool)> {
        // An OS lock expires on process death; a stale lock file cannot prevent
        // recovery after a crash. Two workers must never own the same world.
        let lock = if reference_seed.is_none() {
            let directory = path
                .parent()
                .context("Native checkpoint lacks a directory")?;
            fs::create_dir_all(directory)?;
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(directory.join("session.lock"))?;
            lock.try_lock()
                .context("Native world is already open by another session")?;
            Some(lock)
        } else {
            None
        };
        let resumed = lock.is_some() && path.is_file();
        let (mut runtime, gameplay) = if resumed {
            let file = read_checkpoint(File::open(&path)?)?;
            ensure!(
                file.format == FORMAT && file.generator == GENERATOR,
                "File is not a generator-4 native world checkpoint"
            );
            ensure!(
                file.world_id == config.world_id
                    && file.seed == config.seed
                    && file.runtime.seed == config.seed,
                "Native world checkpoint identity/seed mismatch"
            );
            ensure!(
                file.runtime.map_features == config.map_features,
                "Cannot change map features when resuming a native world"
            );
            let mut resumed_runtime = Runtime::resume(file.runtime, Arc::new(VanillaPopulation))?;
            resumed_runtime.mark_enclosed_lighting_settled();
            (resumed_runtime, file.gameplay)
        } else {
            (
                Runtime::new(
                    config.seed,
                    reference_seed
                        .unwrap_or_else(crate::vanilla::random::JavaRandom::independent_seed),
                    config.map_features,
                    Arc::new(VanillaPopulation),
                ),
                Value::Null,
            )
        };
        // Native publications and durable checkpoints use complete chunks.
        // Gameplay's sparse overlay lives in PopulatedWorld, not this owner.
        // Disable the redundant journal for both new and resumed sessions.
        runtime.use_overlay_journal(false);
        // Only complete chunks/metadata cross command boundaries. Ordinary
        // writes and every other effect field keep their existing behavior.
        runtime.retain_inert_change_diagnostics(false);
        runtime.backend.world.label = format!("Native 1.8.9 · {}", config.world_id);
        runtime.backend.world.save_path = path.clone();
        let mut movement_gate = Gate::default();
        if let Some(f) = runtime
            .backend
            .entities
            .player
            .as_ref()
            .filter(|f| !f.movement_acknowledged)
        {
            let current = Location {
                position: f.position,
                rotation: [f.yaw, f.pitch],
            };
            movement_gate.correct(
                current,
                Correction {
                    location: current,
                    flags: 0,
                },
            );
        }
        Ok((
            Self {
                _lock: lock,
                inventory_epoch: RandomState::new()
                    .hash_one(SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()),
                spawn_load_cursor: None,
                fresh_spawn_publication: !resumed,
                maintenance_cursor: maintenance::Cursor::default(),
                maintenance_policy: maintenance::Policy::default(),
                network_tick: 0,
                movement_gate,
                disconnected: false,
                config,
                path,
                runtime,
                gameplay,
                published: Publication::default(),
                generation_timings: GenerationTimings::default(),
            },
            resumed,
        ))
    }
    fn save(&self) -> Result<()> {
        let file = Checkpoint {
            format: FORMAT.into(),
            generator: GENERATOR,
            world_id: self.config.world_id.clone(),
            seed: self.config.seed,
            gameplay: self.gameplay.clone(),
            runtime: self.runtime.checkpoint()?,
        };
        atomic_checkpoint(&self.path, &file)
    }
    fn execute(&mut self, command: Command) -> Result<(Reply, bool)> {
        let generation = matches!(command, Command::PrepareSpawn | Command::LoadSpawnBatch { .. } | Command::LoadSpawnBatchDeferred { .. }
            | Command::PreparePlayer { .. } | Command::Generate(_) | Command::GenerateAhead { .. });
        let begin = Instant::now();
        let result = self.execute_inner(command);
        if generation {
            self.generation_timings.generate_ms += begin.elapsed().as_secs_f64() * 1000.;
        }
        result
    }
    fn execute_inner(&mut self, command: Command) -> Result<(Reply, bool)> {
        if !self.runtime.backend.pending_tnt.is_empty() && !matches!(command, Command::ReadTile { .. }) {
            return Ok((Reply::AwaitingTnt { completed_steps: 0, pending: self.runtime.backend.pending_tnt.clone() }, false));
        }
        if self.disconnected
            && matches!(
                command,
                Command::PlayerFrame { .. }
                    | Command::CorrectPlayer { .. }
                    | Command::ConfirmPosition { .. }
                    | Command::Inventory { .. }
                    | Command::UseBlock(_)
            )
        {
            return Ok((Reply::PlayerUnavailable, false));
        }
        if let Command::GenerateAhead { positions, ahead } = &command {
            let requests = positions.iter().chain(ahead).copied().collect::<Vec<_>>();
            self.runtime.prefetch_terrain(&requests)?;
        }
        match command {
            Command::PrepareSpawn => {
                let point = self.runtime.prepare_overworld_spawn()?;
                self.runtime.settle_populated_lighting()?;
                self.spawn_load_cursor = Some(0);
                Ok((
                    Reply::SpawnPrepared {
                        point: point.to_array(),
                        initial_region: crate::vanilla::runtime::initial_spawn_region(point),
                    },
                    false,
                ))
            }
            Command::LoadSpawnBatch { count } | Command::LoadSpawnBatchDeferred { count } => {
                let start = self
                    .spawn_load_cursor
                    .context("PrepareSpawn must precede LoadSpawnBatch")?;
                let loaded = self
                    .runtime
                    .load_initial_spawn_batch(start, usize::from(count))?;
                self.runtime.settle_populated_lighting()?;
                self.spawn_load_cursor = Some(loaded);
                Ok((Reply::SpawnAreaProgress { loaded }, false))
            }
            Command::PreparePlayer { adventure } => {
                ensure!(
                    self.spawn_load_cursor == Some(625),
                    "Prepare the original spawn region before player join"
                );
                let initial = self.runtime.prepare_initial_player(adventure)?;
                self.runtime.settle_populated_lighting()?;
                Ok((Reply::PlayerPrepared { initial }, false))
            }
            Command::CorrectPlayer { epoch, correction } => {
                let Some(mut frame) = self
                    .runtime
                    .backend
                    .entities
                    .player
                    .clone()
                    .filter(|_| epoch == self.inventory_epoch)
                else {
                    return Ok((Reply::PlayerUnavailable, false));
                };
                let current = Location {
                    position: frame.position,
                    rotation: [frame.yaw, frame.pitch],
                };
                let mut gate = self.movement_gate;
                let dest = gate.correct(current, correction);
                frame.position = dest.position;
                // Entity.setPosition rebuilds its bounds around the current width/height.
                let half_width = (frame.bounds.max.x - frame.bounds.min.x) * 0.5;
                let height = frame.bounds.max.y - frame.bounds.min.y;
                frame.bounds.min = dest.position - glam::DVec3::new(half_width, 0., half_width);
                frame.bounds.max = dest.position + glam::DVec3::new(half_width, height, half_width);
                [frame.yaw, frame.pitch] = dest.rotation;
                frame.movement_acknowledged = false;
                if frame.validate().is_err() {
                    return Ok((Reply::PlayerUnavailable, false));
                }
                self.runtime.set_player_frame(frame)?;
                self.movement_gate = gate;
                Ok((
                    Reply::PlayerCorrection {
                        payload: correction.payload(),
                    },
                    false,
                ))
            }
            Command::ConfirmPosition { epoch, packet } => {
                let Some(frame) = self
                    .runtime
                    .backend
                    .entities
                    .player
                    .as_mut()
                    .filter(|_| epoch == self.inventory_epoch)
                else {
                    return Ok((Reply::PlayerUnavailable, false));
                };
                let current = Location {
                    position: frame.position,
                    rotation: [frame.yaw, frame.pitch],
                };
                let decision = self
                    .movement_gate
                    .admit(packet, self.network_tick, current, false);
                self.disconnected = matches!(decision, Admission::Disconnect(_));
                frame.movement_acknowledged = self.movement_gate.acknowledged && !self.disconnected;
                Ok((Reply::PositionAdmission(decision), false))
            }
            Command::PlayerFrame { epoch, mut frame } => {
                let accepted = epoch == self.inventory_epoch && self.movement_gate.acknowledged;
                if accepted {
                    frame.movement_acknowledged = true;
                    self.runtime.set_player_frame(frame)?;
                }
                Ok((Reply::PlayerFrame { accepted }, false))
            }
            Command::SpawnExperience { position, value } => {
                crate::vanilla::drops::DropWorld::emit(
                    &mut self.runtime.access(),
                    DropEvent::Experience { position, value },
                )?;
                Ok((Reply::ExperienceSpawned, false))
            }
            Command::UseBlock(request) => Ok((
                Reply::UsedBlock(self.runtime.use_block(self.inventory_epoch, request)?),
                false,
            )),
            Command::DigBlock(request)=>Ok((Reply::Dug(self.runtime.dig_block(self.inventory_epoch,request)?),false)),
            Command::ReadTile{position}=>Ok((Reply::Tile{position,tag:self.runtime.tile_snapshot(IVec3::from_array(position))?.map(Arc::new)},false)),
            Command::Generate(positions) | Command::GenerateAhead { positions, .. } => {
                self.runtime.prepare_terrain(&positions)?;
                for pos in positions {
                    self.runtime.provide(pos)?;
                    // A renderer may have evicted an unchanged chunk while the
                    // serial owner retained it. Explicit requests republish it.
                    self.runtime.backend.dirty_chunks.insert(pos);
                    self.runtime.settle_populated_lighting()?;
                }
                Ok((Reply::Generated, false))
            }
            Command::Edits(edits) => {
                let mut rejected = Vec::new();
                for edit in edits {
                    let p = IVec3::from_array(edit.position);
                    let actual = self.runtime.access().get_block(p)?;
                    if let Some(expected) = edit.expected {
                        if actual != expected {
                            rejected.push(RejectedEdit {
                                position: edit.position,
                                expected,
                                actual,
                            });
                            self.runtime
                                .backend
                                .dirty_chunks
                                .insert((p.x.div_euclid(16), p.z.div_euclid(16)));
                            continue;
                        }
                    }
                    self.runtime.access().set_block_flags(
                        p,
                        edit.state,
                        2 | u8::from(edit.notify_neighbors),
                    )?;
                }
                Ok((Reply::Edited { rejected }, false))
            }
            Command::Inventory { stamp, action } => Ok((
                Reply::Inventory(self.runtime.inventory_action(
                    self.inventory_epoch,
                    stamp,
                    action,
                )?),
                false,
            )),
            Command::SetFireEnvironment(environment) => {
                self.runtime.backend.fire_environment = environment;
                Ok((Reply::FireEnvironmentSet, false))
            }
            Command::Tick { steps } => {
                ensure!(
                    self.config.enable_scheduled_ticks,
                    "Native scheduled ticks are not enabled for this session"
                );
                for completed in 0..steps {
                    self.network_tick = self.network_tick.wrapping_add(1);
                    if !self.runtime.tick_blocks_and_falling()? {
                        return Ok((Reply::AwaitingTnt { completed_steps: completed, pending: self.runtime.backend.pending_tnt.clone() }, false));
                    }
                }
                Ok((Reply::Ticked { steps }, false))
            }
            Command::Snapshot { gameplay } | Command::Shutdown { gameplay } => {
                unreachable!(
                    "Snapshot and Shutdown are handled before command dispatch: {gameplay:?}"
                )
            }
        }
    }
    fn update_after_reply(&mut self, reply: &Reply, deferred_spawn: bool) -> Update {
        let defer_chunks = deferred_spawn && self.fresh_spawn_publication
            && matches!(reply, Reply::SpawnAreaProgress { loaded } if *loaded < 625);
        if matches!(reply, Reply::SpawnAreaProgress { loaded } if *loaded >= 625) {
            self.fresh_spawn_publication = false;
        }
        self.update_with_chunks(false, !defer_chunks)
    }
    fn resume_view_center(&self) -> Option<ChunkPos> {
        let dim = self
            .gameplay
            .get("simulation")
            .and_then(|s| s.get("dimension"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if dim != 0 {
            return self
                .runtime
                .backend
                .world_spawn
                .map(|p| (p.x >> 4, p.z >> 4));
        }
        let pos = self.gameplay.get("position")?.as_array()?;
        if pos.len() != 3 {
            return None;
        }
        let x = pos[0].as_f64()?;
        let z = pos[2].as_f64()?;
        if !x.is_finite() || !z.is_finite() {
            return None;
        }
        Some((
            (x.floor() as i32).div_euclid(16),
            (z.floor() as i32).div_euclid(16),
        ))
    }
    fn ready_update(&mut self, resumed: bool) -> Update {
        if resumed {
            if let (Some(radius), Some(center)) =
                (self.config.resume_publish_radius, self.resume_view_center())
            {
                let render_dirty = self.runtime.backend.light.take_render_dirty();
                for p in render_dirty {
                    if (p.0 - center.0).abs() <= radius && (p.1 - center.1).abs() <= radius {
                        self.runtime.backend.dirty_chunks.insert(p);
                    }
                }
                self.runtime.backend.dirty_chunks.retain(|p| {
                    (p.0 - center.0).abs() <= radius && (p.1 - center.1).abs() <= radius
                });
            }
        }
        self.update(true)
    }
    fn update(&mut self, reset: bool) -> Update {
        self.update_with_chunks(reset, true)
    }
    fn update_with_chunks(&mut self, reset: bool, publish_chunks: bool) -> Update {
        let begin = Instant::now();
        // Deferred startup replies must not drain either dirty set. The final
        // ordinary spawn reply publishes their union after all original loads.
        let chunks = if publish_chunks { self.runtime.take_chunk_updates() } else { Vec::new() };
        let retained_positions = if reset {
            self.runtime.provider.loaded_order().to_vec()
        } else {
            Vec::new()
        };
        let mut remesh = BTreeSet::new();
        for &(p, _) in &chunks {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    let p = (p.0 + dx, p.1 + dz);
                    if self.runtime.backend.world.chunks.contains_key(&p) {
                        remesh.insert(p);
                    }
                }
            }
        }
        let metadata = self.published.take(&self.runtime, reset);
        // Published chunks contain these writes, or retained dirty state will
        // include them in the final deferred snapshot. The original provider
        // history/structure state and retained entity metadata remain intact;
        // diagnostic write lists and legacy sparse overlays are not a second
        // authoritative save layer and must not grow on every terrain request.
        self.runtime.backend.output.changed.clear();
        self.runtime.backend.block_effects.changed.clear();
        self.runtime.backend.events.clear();
        self.runtime.backend.world.edits.clear();
        let mut update = Update {
            generation_timings: GenerationTimings::default(),
            chunks,
            retained_positions,
            remesh: remesh.into_iter().collect(),
            metadata,
            inventory: Some(self.runtime.inventory_view(self.inventory_epoch)),
            falling: self.runtime.backend.falling.live.clone(),
            falling_effects: std::mem::take(&mut self.runtime.backend.falling.effects),
            items: self
                .runtime
                .backend
                .entities
                .live
                .values()
                .filter(|e| !e.dead)
                .cloned()
                .collect(),
            entity_effects: std::mem::take(&mut self.runtime.backend.entities.effects),
            orbs: self
                .runtime
                .backend
                .entities
                .orbs
                .values()
                .filter(|o| !o.body.dead)
                .map(|o| o.body.clone())
                .collect(),
            experience: self.runtime.backend.entities.experience.clone(),
            player_frame: self.runtime.backend.entities.player.clone(),
            world_time: self.runtime.provider.world_time,
        };
        self.generation_timings.publication_ms += begin.elapsed().as_secs_f64() * 1000.;
        self.generation_timings.retained_chunks = self.runtime.backend.world.chunks.len();
        self.generation_timings.terrain = self.runtime.terrain_timings();
        self.generation_timings.runtime = self.runtime.runtime_timings();
        update.generation_timings = self.generation_timings;
        update
    }
}
fn run_worker(
    config: Config,
    path: PathBuf,
    reference_seed: Option<i64>,
    requests: mpsc::Receiver<Envelope>,
    events: &mpsc::Sender<Event>,
) -> Result<()> {
    let (mut worker, resumed) = Worker::open(config, path, reference_seed)?;
    // Persist the independently seeded World.rand before publishing a new
    // session. A restart before its first terrain request must not reseed it.
    if !resumed && reference_seed.is_none() {
        worker.save()?;
    }
    let ready = Event::Ready {
        resumed,
        path: reference_seed.is_none().then(|| worker.path.clone()),
        gameplay: worker.gameplay.clone(),
        update: worker.ready_update(resumed),
    };
    if events.send(ready).is_err() {
        return Ok(());
    }
    while let Ok(Envelope {
        id,
        command,
        deferred_gameplay,
        maintenance_view,
    }) = requests.recv()
    {
        let maintenance = maintenance::Context::capture(&command, maintenance_view,
            worker.maintenance_policy.enabled(), worker.fresh_spawn_publication);
        let deferred_spawn = matches!(&command, Command::LoadSpawnBatchDeferred { .. });
        let result = match command {
            Command::Snapshot { gameplay } => (|| -> Result<(Reply, bool)> {
                if let Some(value) = gameplay {
                    worker.gameplay = value;
                } else if let Some(build) = deferred_gameplay {
                    worker.gameplay = build()?;
                }
                worker.save()?;
                Ok((
                    Reply::Saved {
                        path: worker.path.clone(),
                    },
                    false,
                ))
            })(),
            Command::Shutdown { gameplay } => {
                if let Some(value) = gameplay {
                    worker.gameplay = value;
                }
                worker.save().map(|()| {
                    (
                        Reply::ShuttingDown {
                            path: worker.path.clone(),
                        },
                        true,
                    )
                })
            }
            command => worker.execute(command),
        };
        match result {
            Ok((reply, stop)) => {
                let maintenance = maintenance::successful(maintenance, &reply);
                let update = worker.update_after_reply(&reply, deferred_spawn);
                if events
                    .send(Event::Completed {
                        request_id: id,
                        reply,
                        update,
                    })
                    .is_err()
                    || stop
                {
                    return Ok(());
                }
                // Same owner, after the ordinary publication/send; no new event,
                // provider request, game tick, RNG draw or dirty publication.
                worker.maintenance_policy.after_publication(&mut worker.runtime,
                    &mut worker.maintenance_cursor, maintenance);
                worker.generation_timings.maintenance = worker.maintenance_policy.metrics();
            }
            Err(error) => {
                let _ = events.send(Event::Failed {
                    request_id: Some(id),
                    message: format!("{error:#}"),
                });
                return Ok(());
            }
        }
    }
    Ok(())
}
fn read_checkpoint(reader: impl Read) -> Result<Checkpoint> {
    let compressed = GzDecoder::new(BufReader::new(reader));
    read_checkpoint_json(compressed)
}
fn read_checkpoint_json(reader: impl Read) -> Result<Checkpoint> {
    read_checkpoint_json_limited(reader, 512 * 1024 * 1024)
}
fn read_checkpoint_json_limited(mut reader: impl Read, limit: usize) -> Result<Checkpoint> {
    // Typed block/light arrays parse faster from a byte slice than through
    // serde_json's per-byte Read adapter. Cap temporary JSON storage so very
    // large retained worlds can still load using the streaming decoder.
    let mut bytes = Vec::new();
    reader.by_ref().take(limit as u64).read_to_end(&mut bytes)
        .context("Read native world checkpoint")?;
    // Probe separately: pushing byte limit+1 could double a 512 MiB Vec's
    // capacity. Reading EOF here also validates the gzip trailer at the cap.
    let mut extra = Vec::new();
    reader.by_ref().take(1).read_to_end(&mut extra)
        .context("Read native world checkpoint")?;
    if extra.is_empty() {
        serde_json::from_slice(&bytes).context("Read native world checkpoint")
    } else {
        let prefix = std::io::Cursor::new(bytes).chain(std::io::Cursor::new(extra));
        read_checkpoint_json_buffered(prefix.chain(reader))
    }
}
fn read_checkpoint_json_buffered(reader: impl Read) -> Result<Checkpoint> {
    // Large-file fallback: avoid entering the inflater for each JSON number.
    serde_json::from_reader(BufReader::with_capacity(256 * 1024, reader))
        .context("Read native world checkpoint")
}
fn atomic_checkpoint(path: &Path, checkpoint: &Checkpoint) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("Native checkpoint lacks a parent directory"))?;
    fs::create_dir_all(parent)?;
    let nonce =
        RandomState::new().hash_one(SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos());
    let temporary = parent.join(format!(".world-{}-{nonce:016x}.tmp", std::process::id()));
    let result = (|| {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        // serde emits tiny numeric tokens for millions of block/light values.
        // Buffer BEFORE compression as well as before disk writes.
        let compressed = GzEncoder::new(BufWriter::new(file), Compression::fast());
        let mut json = BufWriter::with_capacity(256 * 1024, compressed);
        serde_json::to_writer(&mut json, checkpoint)?;
        let compressed = json.into_inner().map_err(|e| e.into_error())?;
        let buffered = compressed.finish()?;
        let file = buffered.into_inner().map_err(|e| e.into_error())?;
        file.sync_all()?;
        // std::fs::rename uses replacement semantics on Windows as well.
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[derive(Default)]
struct Publication {
    entities: [usize; 6],
    drops: usize,
    containers: BTreeMap<[i32; 3], u64>,
    spawners: BTreeMap<[i32; 3], String>,
}
impl Publication {
    fn take(&mut self, runtime: &Runtime, reset: bool) -> MetadataUpdate {
        if reset {
            *self = Self::default();
        }
        let b = &runtime.backend;
        let mut next = MetadataUpdate {
            reset,
            ..Default::default()
        };
        let containers = b
            .live_containers
            .iter()
            .map(|(p, c)| (*p, c.revision))
            .collect::<BTreeMap<_, _>>();
        next.removed_containers = self
            .containers
            .keys()
            .filter(|p| !containers.contains_key(*p))
            .copied()
            .collect();
        next.containers = b
            .live_containers
            .iter()
            .filter(|(p, c)| self.containers.get(*p) != Some(&c.revision))
            .map(|(_, c)| c.clone())
            .collect();
        self.containers = containers;
        let spawners = b.live_spawners.clone();
        next.removed_spawners = self
            .spawners
            .keys()
            .filter(|p| !spawners.contains_key(*p))
            .copied()
            .collect();
        next.spawners = spawners
            .iter()
            .filter(|(p, v)| self.spawners.get(*p) != Some(*v))
            .map(|(p, v)| (*p, v.clone()))
            .collect();
        self.spawners = spawners;
        macro_rules! emit {
            ($family:expr,$values:expr,$variant:ident) => {{
                for (index, value) in $values.iter().enumerate().skip(self.entities[$family]) {
                    next.spawned.push(SpawnRecord {
                        family: $family as u8,
                        index: index as u64,
                        spawn: GeneratedSpawn::$variant(value.clone()),
                    });
                }
                self.entities[$family] = $values.len();
            }};
        }
        emit!(0, b.structure_entities.villagers, Villager);
        emit!(1, b.structure_entities.carts, ChestMinecart);
        emit!(2, b.structure_entities.witches, Witch);
        emit!(3, b.structure_entities.elders, ElderGuardian);
        emit!(4, b.animals, Animal);
        emit!(5, b.output.entities, Feature);
        next.drops.extend(
            b.drops
                .iter()
                .enumerate()
                .skip(self.drops)
                .map(|(index, event)| DropRecord {
                    index: index as u64,
                    event: event.clone(),
                    owned_by_runtime: match event {
                        DropEvent::Item { .. } => index >= b.entities.legacy_drop_prefix,
                        DropEvent::Experience { .. } => {
                            b.entities.xp_owned_after.is_some_and(|at| index >= at)
                        }
                        _ => false,
                    },
                }),
        );
        self.drops = b.drops.len();
        next
    }
}

#[cfg(test)]
#[path = "native_session_performance_tests.rs"]
mod performance_tests;
#[cfg(test)]
#[path = "native_session_cold_light_tests.rs"]
mod cold_light_tests;
#[cfg(test)]
#[path = "native_session_spawn_publication_tests.rs"]
mod spawn_publication_tests;
#[cfg(test)]
#[path = "native_session_journal_tests.rs"]
mod journal_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        inventory::Stack,
        vanilla::{drops::DropNbt, loot::LootStack, scattered::GeneratedContainer},
    };
    use std::time::Duration;
    fn receive(session: &mut NativeSession) -> Event {
        let event = session
            .receiver
            .recv_timeout(Duration::from_secs(60))
            .expect("native worker response");
        session.terminal |= matches!(event, Event::Failed { .. } | Event::Stopped);
        event
    }
    #[test]
    fn in_memory_preview_has_no_checkpoint_and_remains_usable_after_rejected_save() {
        let mut session = NativeSession::in_memory(189, 1234).unwrap();
        assert!(matches!(
            receive(&mut session),
            Event::Ready {
                path: None,
                resumed: false,
                ..
            }
        ));
        assert!(session
            .submit(Command::Snapshot { gameplay: None })
            .is_err());
        assert!(session
            .submit(Command::Shutdown { gameplay: None })
            .is_err());
        let id = session.submit(Command::PrepareSpawn).unwrap();
        let Event::Completed {
            request_id,
            reply:
                Reply::SpawnPrepared {
                    point,
                    initial_region,
                },
            ..
        } = receive(&mut session)
        else {
            panic!("missing native spawn");
        };
        assert_eq!(id, request_id);
        assert_eq!(point, [-116, 64, 311]);
        assert_eq!(initial_region.len(), 625);
        session.join().unwrap();
    }
    #[test]
    fn server_correction_waits_for_matching_position_before_accepting_a_new_frame() {
        let mut session = NativeSession::in_memory(189, 1234).unwrap();
        let Event::Ready { update, .. } = receive(&mut session) else {
            panic!("startup");
        };
        let epoch = update.inventory.unwrap().stamp.epoch;
        let frame = PlayerFrame::standing("Player", glam::DVec3::new(10.5, 2., 5.5));
        let request = |s: &mut NativeSession, c: Command| {
            let id = s.submit(c).unwrap();
            let Event::Completed {
                request_id,
                reply,
                update,
            } = receive(s)
            else {
                panic!("missing reply");
            };
            assert_eq!(id, request_id);
            (reply, update.player_frame)
        };
        request(
            &mut session,
            Command::PlayerFrame {
                epoch,
                frame: frame.clone(),
            },
        );
        let dest = Location {
            position: glam::DVec3::new(2., 64., -3.),
            rotation: [450., -15.],
        };
        let correction = Correction {
            location: dest,
            flags: 1,
        };
        let (reply, moved) = request(&mut session, Command::CorrectPlayer { epoch, correction });
        let moved = moved.unwrap();
        assert!(
            matches!(reply,Reply::PlayerCorrection {payload} if payload == correction.payload())
        );
        assert_eq!(moved.position, glam::DVec3::new(12.5, 64., -3.));
        assert_eq!(moved.yaw, 90.);
        assert!(!moved.movement_acknowledged);
        let (reply, _) = request(
            &mut session,
            Command::PlayerFrame {
                epoch,
                frame: frame.clone(),
            },
        );
        assert!(matches!(reply, Reply::PlayerFrame { accepted: false }));
        let packet = |offset| MovementPacket {
            position: Some(moved.position + glam::DVec3::X * offset),
            rotation: None,
        };
        let (reply, _) = request(
            &mut session,
            Command::ConfirmPosition {
                epoch: epoch ^ 1,
                packet: packet(0.),
            },
        );
        assert!(matches!(reply, Reply::PlayerUnavailable));
        let (reply, _) = request(
            &mut session,
            Command::ConfirmPosition {
                epoch,
                packet: packet(0.5),
            },
        );
        assert!(matches!(
            reply,
            Reply::PositionAdmission(Admission::Pending)
        ));
        let (reply, body) = request(
            &mut session,
            Command::ConfirmPosition {
                epoch,
                packet: packet(0.125),
            },
        );
        assert!(matches!(
            reply,
            Reply::PositionAdmission(Admission::Admitted)
        ));
        let body = body.unwrap();
        assert!(body.movement_acknowledged);
        assert_eq!(body.position, moved.position);
        let (reply, _) = request(
            &mut session,
            Command::ConfirmPosition {
                epoch,
                packet: packet(f64::NAN),
            },
        );
        assert!(matches!(
            reply,
            Reply::PositionAdmission(Admission::Disconnect(_))
        ));
        let (reply, _) = request(&mut session, Command::PlayerFrame { epoch, frame });
        assert!(matches!(reply, Reply::PlayerUnavailable));
        session.join().unwrap();
    }
    #[test]
    fn ordered_worker_edits_checkpoint_and_resume_preserve_native_authority() {
        let root = std::env::temp_dir().join(format!(
            "ferrum-native-host-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let config = Config::new(189, "worker-test", &root);
        let path = config.checkpoint_path().unwrap();
        let mut session = NativeSession::spawn(config.clone()).unwrap();
        let Event::Ready {
            resumed: false,
            update,
            ..
        } = receive(&mut session)
        else {
            panic!("new session ready")
        };
        let initial_stamp = update.inventory.unwrap().stamp;
        let mut player = crate::inventory::Inventory::default();
        player.slots[0] = Some(Stack::new(264, 7));
        session
            .submit(Command::Inventory {
                stamp: initial_stamp,
                action: InventoryAction::Initialize {
                    inventory: player,
                    creative: false,
                },
            })
            .unwrap();
        let Event::Completed {
            reply: Reply::Inventory(initialized),
            ..
        } = receive(&mut session)
        else {
            panic!("inventory initialization")
        };
        assert!(initialized.rejected.is_none());
        let old_stamp = initialized.view.stamp;
        let mut duplicate = NativeSession::spawn(config.clone()).unwrap();
        assert!(matches!(receive(&mut duplicate), Event::Failed { .. }));
        duplicate.join().unwrap();
        let generated = session
            .submit(Command::Generate(vec![(0, 0), (1, 0), (0, 1), (1, 1)]))
            .unwrap();
        let position = [7, 200, 7];
        let edited = session
            .submit(Command::Edits(vec![BlockEdit {
                position,
                state: 89 << 4,
                expected: Some(0),
                notify_neighbors: false,
            }]))
            .unwrap();
        let saved = session
            .submit(Command::Snapshot {
                gameplay: Some(serde_json::json!({"player":[1,2,3],"inventory_version":1})),
            })
            .unwrap();
        assert!(
            matches!(receive(&mut session), Event::Completed {request_id,reply:Reply::Generated,..} if request_id==generated)
        );
        let Event::Completed {
            request_id,
            reply: Reply::Edited { rejected },
            update,
        } = receive(&mut session)
        else {
            panic!("edit response")
        };
        assert_eq!(request_id, edited);
        assert!(rejected.is_empty());
        let chunk = update.chunks.iter().find(|(p, _)| *p == (0, 0)).unwrap();
        assert_eq!(chunk.1.blocks[Chunk::index(7, 200, 7)], 89 << 4);
        assert!(chunk.1.light.is_some());
        assert!(
            matches!(receive(&mut session), Event::Completed {request_id,reply:Reply::Saved {..},..} if request_id==saved)
        );
        assert!(path.is_file());
        session
            .submit(Command::Shutdown { gameplay: None })
            .unwrap();
        assert!(matches!(
            receive(&mut session),
            Event::Completed {
                reply: Reply::ShuttingDown { .. },
                ..
            }
        ));
        assert!(matches!(receive(&mut session), Event::Stopped));
        session.join().unwrap();
        let mut resumed = NativeSession::spawn(config).unwrap();
        let Event::Ready {
            resumed: true,
            update,
            gameplay,
            ..
        } = receive(&mut resumed)
        else {
            panic!("resume ready")
        };
        assert_eq!(gameplay["player"], serde_json::json!([1, 2, 3]));
        let inventory = update.inventory.as_ref().unwrap();
        assert_ne!(inventory.stamp.epoch, old_stamp.epoch);
        assert_eq!(
            inventory.player.as_ref().unwrap().inventory.slots[0],
            Some(Stack::new(264, 7))
        );
        resumed
            .submit(Command::Inventory {
                stamp: old_stamp,
                action: InventoryAction::Close,
            })
            .unwrap();
        let Event::Completed {
            reply: Reply::Inventory(rejected),
            ..
        } = receive(&mut resumed)
        else {
            panic!("stale pre-reconnect transaction")
        };
        assert!(rejected.rejected.is_some());
        assert_eq!(rejected.view.stamp, inventory.stamp);
        assert!(update.metadata.reset);
        assert_eq!(
            update
                .chunks
                .iter()
                .find(|(p, _)| *p == (0, 0))
                .unwrap()
                .1
                .blocks[Chunk::index(7, 200, 7)],
            89 << 4
        );
        resumed
            .submit(Command::Edits(vec![BlockEdit {
                position,
                state: 0,
                expected: Some(0),
                notify_neighbors: true,
            }]))
            .unwrap();
        let Event::Completed {
            reply: Reply::Edited { rejected },
            ..
        } = receive(&mut resumed)
        else {
            panic!("guard response")
        };
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].actual, 89 << 4);
        resumed
            .submit(Command::Shutdown { gameplay: None })
            .unwrap();
        receive(&mut resumed);
        receive(&mut resumed);
        resumed.join().unwrap();
        fs::remove_file(path).unwrap();
        fs::remove_file(root.join("native-v4/worker-test/session.lock")).unwrap();
        fs::remove_dir(root.join("native-v4/worker-test")).unwrap();
        fs::remove_dir(root.join("native-v4")).unwrap();
        fs::remove_dir(root).unwrap();
    }
    #[test]
    fn enchanted_loot_is_preserved_and_unsupported_containers_are_rejected() {
        assert!(Config::new(0, "../legacy", "saves")
            .checkpoint_path()
            .is_err());
        let mut slots = vec![None; 27];
        slots[0] = Some(LootStack {
            id: 403,
            meta: 0,
            count: 1,
            enchants: vec![[16, 5]],
        });
        let container = NativeContainer::generated(
            &GeneratedContainer {
                position: [0; 3],
                block: 54,
                slots,
            },
            1,
        )
        .legacy_chest()
        .unwrap();
        let expected = DropNbt::Compound(BTreeMap::from([(
            "StoredEnchantments".into(),
            DropNbt::List {
                element: 10,
                values: vec![DropNbt::Compound(BTreeMap::from([
                    ("id".into(), DropNbt::Short(16)),
                    ("lvl".into(), DropNbt::Short(5)),
                ]))],
            },
        )]));
        let stack = container.slots[0].as_ref().unwrap();
        assert_eq!(stack.nbt.as_deref(), Some(&expected));
        let saved = serde_json::to_vec(stack).unwrap();
        let restored: Stack = serde_json::from_slice(&saved).unwrap();
        assert_eq!(restored, *stack);
        assert!(NativeContainer {
            position: [0; 3],
            block: 23,
            revision: 1,
            slots: vec![None; 9]
        }
        .legacy_chest()
        .is_err());
    }
    #[test]
    fn worker_tnt_boundary_accepts_snapshots_and_resumes_without_replaying_ignition() {
        use crate::vanilla::runtime::FireEnvironment;
        let root = std::env::temp_dir().join(format!(
            "ferrum-native-fire-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let mut config = Config::new(189, "tnt-test", &root);
        config.enable_scheduled_ticks = true;
        config.map_features = false;
        let path = config.checkpoint_path().unwrap();

        // Place the original oracle's TNT scene in clear space above terrain.
        // Pin the two original random streams before launching the real worker.
        let mut runtime = Runtime::new(189, 1895, false, Arc::new(VanillaPopulation));
        runtime.provide((0, 0)).unwrap();
        Arc::make_mut(runtime.backend.world.chunks.get_mut(&(0, 0)).unwrap()).biomes.fill(1);
        runtime.backend.world_random.set_seed(1895);
        runtime.backend.entities.math_state = Some(crate::vanilla::random::JavaRandom::new(8195).state());
        let initial = Checkpoint {
            format: FORMAT.into(), generator: GENERATOR, world_id: config.world_id.clone(),
            seed: 189, gameplay: Value::Null, runtime: runtime.checkpoint().unwrap(),
        };
        atomic_checkpoint(&path, &initial).unwrap();
        drop(initial);
        drop(runtime);
        let mut session = NativeSession::spawn(config.clone()).unwrap();
        assert!(matches!(receive(&mut session), Event::Ready { .. }));
        let conditions = FireEnvironment { do_fire_tick: true, difficulty: 0, raining: false };
        assert!(session.submit(Command::SetFireEnvironment(FireEnvironment { difficulty: 4, ..conditions })).is_err());
        session.submit(Command::SetFireEnvironment(conditions)).unwrap();
        assert!(matches!(receive(&mut session), Event::Completed { reply: Reply::FireEnvironmentSet, .. }));

        let cases: Value = serde_json::from_reader(GzDecoder::new(
            &include_bytes!("../tests/fixtures/vanilla189-fire-ticks.json.gz")[..]
        )).unwrap();
        let scene = &cases["cases"][6];
        let edits = scene["edits"].as_array().unwrap().iter().map(|row| {
            let [x,y,z,state]: [i32;4] = serde_json::from_value(row.clone()).unwrap();
            BlockEdit { position: [x,y+180,z], state: state as u16, expected: None, notify_neighbors: false }
        }).collect();
        session.submit(Command::Edits(edits)).unwrap();
        assert!(matches!(receive(&mut session), Event::Completed { reply: Reply::Edited { .. }, .. }));
        session.submit(Command::Tick { steps: 150 }).unwrap();
        let Event::Completed { reply: Reply::AwaitingTnt { completed_steps, pending }, update, .. }
            = receive(&mut session) else { panic!("expected pending TNT entity phase"); };
        let stop_tick = scene["frames"].as_array().unwrap().last().unwrap()[0].as_i64().unwrap();
        assert_eq!(i64::from(completed_steps), stop_tick - 1);
        assert_eq!(update.world_time, stop_tick);
        assert_eq!(pending.len(), 1);
        let pending = serde_json::to_value(pending).unwrap();

        session.submit(Command::Snapshot { gameplay: None }).unwrap();
        assert!(matches!(receive(&mut session), Event::Completed { reply: Reply::Saved { .. }, .. }));
        session.submit(Command::Shutdown { gameplay: None }).unwrap();
        receive(&mut session);
        receive(&mut session);
        session.join().unwrap();
        let file: Checkpoint = serde_json::from_reader(GzDecoder::new(File::open(&path).unwrap())).unwrap();
        assert_eq!(file.runtime.fire_environment.difficulty, 0);
        assert_eq!(serde_json::to_value(file.runtime.pending_tnt).unwrap(), pending);
        let mut session = NativeSession::spawn(config).unwrap();
        assert!(matches!(receive(&mut session), Event::Ready { resumed: true, .. }));
        for command in [Command::Tick { steps: 3 }, Command::Edits(vec![BlockEdit {
            position:[8,185,8],state:0,expected:None,notify_neighbors:true
        }])] {
            session.submit(command).unwrap();
            let Event::Completed { reply: Reply::AwaitingTnt { completed_steps: 0, pending: retained }, update, .. }
                = receive(&mut session) else { panic!("pending TNT must survive retry"); };
            assert_eq!(update.world_time, stop_tick);
            assert_eq!(serde_json::to_value(retained).unwrap(), pending);
        }
        session.submit(Command::Shutdown { gameplay: None }).unwrap();
        receive(&mut session);
        receive(&mut session);
        session.join().unwrap();
        fs::remove_file(path).unwrap();
        let directory = root.join("native-v4/tnt-test");
        fs::remove_file(directory.join("session.lock")).unwrap();
        fs::remove_dir(directory).unwrap();
        fs::remove_dir(root.join("native-v4")).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn worker_tick_phase_and_checkpoint_use_one_clock() {
        let root = std::env::temp_dir().join(format!(
            "ferrum-native-clock-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut config = Config::new(0, "clock-test", &root);
        config.enable_scheduled_ticks = true;
        let path = config.checkpoint_path().unwrap();
        let mut session = NativeSession::spawn(config).unwrap();
        assert!(matches!(receive(&mut session), Event::Ready { .. }));
        session.submit(Command::Tick { steps: 3 }).unwrap();
        let Event::Completed {
            reply: Reply::Ticked { steps: 3 },
            update,
            ..
        } = receive(&mut session)
        else {
            panic!("native tick response")
        };
        assert_eq!(update.world_time, 3);
        session
            .submit(Command::Shutdown { gameplay: None })
            .unwrap();
        receive(&mut session);
        receive(&mut session);
        session.join().unwrap();
        let saved: Checkpoint =
            serde_json::from_reader(GzDecoder::new(File::open(&path).unwrap())).unwrap();
        assert_eq!(saved.runtime.provider.world_time, 3);
        assert_eq!(saved.runtime.ticks.clock(), 3);
        fs::remove_file(path).unwrap();
        fs::remove_file(root.join("native-v4/clock-test/session.lock")).unwrap();
        fs::remove_dir(root.join("native-v4/clock-test")).unwrap();
        fs::remove_dir(root.join("native-v4")).unwrap();
        fs::remove_dir(root).unwrap();
    }
    #[test]
    fn replacing_generated_tiles_never_resurrects_historical_loot_or_spawner_type() {
        let mut runtime = Runtime::for_overworld_verification(189, 1234, true);
        for x in -48..=-45 {
            for z in 46..=49 {
                runtime.provide((x, z)).unwrap();
            }
        }
        let chest = runtime
            .backend
            .live_containers
            .values()
            .find(|c| c.block == 54 && c.slots.iter().any(Option::is_some))
            .unwrap()
            .clone();
        let p = IVec3::from_array(chest.position);
        let mut publication = Publication::default();
        assert!(publication
            .take(&runtime, true)
            .containers
            .iter()
            .any(|c| c.position == chest.position && c.slots.iter().any(Option::is_some)));
        runtime.access().set_block(p, 0).unwrap();
        assert!(publication
            .take(&runtime, false)
            .removed_containers
            .contains(&chest.position));
        runtime.access().set_block(p, 54 << 4 | 2).unwrap();
        let changed = publication.take(&runtime, false);
        assert!(changed
            .containers
            .iter()
            .any(|c| c.position == chest.position && c.slots.iter().all(Option::is_none)));
        let spawner = p + IVec3::new(0, 10, 0);
        runtime.access().set_block(spawner, 52 << 4).unwrap();
        crate::vanilla::population::FeatureWorld::spawner(&mut runtime.access(), spawner, "Zombie");
        runtime.access().set_block(spawner, 0).unwrap();
        runtime.access().set_block(spawner, 52 << 4).unwrap();
        assert_eq!(runtime.backend.live_spawners[&spawner.to_array()], "Pig");
        let bytes = serde_json::to_vec(&runtime.checkpoint().unwrap()).unwrap();
        let restored = Runtime::resume(
            serde_json::from_slice(&bytes).unwrap(),
            Arc::new(VanillaPopulation),
        )
        .unwrap();
        let published = Publication::default().take(&restored, true);
        assert!(published
            .containers
            .iter()
            .find(|c| c.position == chest.position)
            .unwrap()
            .slots
            .iter()
            .all(Option::is_none));
        assert!(published
            .spawners
            .contains(&(spawner.to_array(), "Pig".into())));
    }
}
