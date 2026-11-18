//! Opt-in eventful world boundary. Each nested population callback gets a fresh
//! adapter cell; no global mutable borrow spans a reentrant World operation.
//! The caller must supply validated population and block callbacks. This module
//! is deliberately not installed as the default generator.
use super::{
    biome::BiomeGenerator,
    lighting::{Kind, Lighting},
    loot::LootStack,
    population::{FeatureWorld, GeneratedEntity, Population},
    provider::{Backend, ChunkRecord, NativeStructures, Provider},
    random::JavaRandom,
    terrain::Terrain,
};
use crate::{
    block_ticks::{BlockAccess, BlockTicks, TickResult},
    world::{ChunkPos, World},
};
use anyhow::{ensure, Result};
use glam::IVec3;
use std::{cell::RefCell, collections::HashMap, path::PathBuf, sync::Arc};
#[path = "runtime_break.rs"]
mod break_callbacks;
#[path="runtime_tile_break.rs"]
mod tile_break;
#[path = "runtime_snapshot.rs"]
mod snapshot;
#[path = "runtime_spawn.rs"]
mod spawn;
#[path = "runtime_terrain.rs"]
mod terrain_work;
pub use terrain_work::TerrainTimings;
#[path = "runtime_profile.rs"]
mod profiling;
#[path = "runtime_publication.rs"]
mod publication;
pub use publication::PublicationTimings;
#[path = "runtime_inert_ore.rs"]
mod inert_ore;
#[path = "runtime_inert_column.rs"]
mod inert_column;
pub use profiling::RuntimeTimings;
pub use super::population::ore_lookahead::Stats as OreLookaheadStats;
use profiling::Phase;
#[allow(unused_imports)]
pub use snapshot::{ChunkSnapshot, RuntimeSnapshot};
pub use spawn::initial_spawn_region;
#[path = "runtime_player_spawn.rs"]
mod player_spawn;
pub use player_spawn::InitialPlayer;
#[path = "runtime_entities.rs"]
mod entities;
#[path = "runtime_facing_effects.rs"]
mod facing_effects;
#[path = "runtime_falling.rs"]
mod gravity;
pub use entities::{EntityEffect, EntityKey, NativeEntities, PlayerFrame};
#[path="runtime_use.rs"]
mod block_use;
#[path="runtime_dig.rs"]
mod dig;
#[allow(unused_imports)]
pub use dig::{DigAction,DigBlock,DigOutcome,DigReceipt,Digging};
#[allow(unused_imports)]
pub use block_use::{UseBlock,UseOutcome,UseReceipt};
#[path = "runtime_inventory.rs"]
mod inventories;
pub use gravity::{FallingEffect, FallingEntry, FallingManager};
pub use inventories::{
    stack_from_loot, InventoryAction, InventoryReceipt, InventoryStamp, InventoryView,
    LiveContainer, NativePlayerInventory,
};
#[path = "runtime_population.rs"]
mod phases;
#[path = "runtime_plant_effects.rs"]
mod plant_effects;
#[path = "runtime_rail_effects.rs"]
mod rail_effects;
#[path="runtime_openables.rs"]
mod openables;
#[path="runtime_fire.rs"]
pub(crate) mod fire;
pub use fire::{FireEnvironment, PendingTnt};
pub(crate) use fire::{rain_in_biome, biome_can_rain};
#[path = "runtime_structures.rs"]
mod structures;
use super::population::decoration::Decorator;
pub use phases::VanillaPopulation;
use std::collections::{BTreeMap, HashSet};

/// Hooks are explicit because unsupported generation must never be marked as a
/// completed original population. Implementations may synchronously read/write
/// through Access, including loading and populating further chunks.
pub trait Hooks {
    /// Pure opt-in for the native ore path. For metadata-free stone/netherrack
    /// replaced by the explicit inert ore/stone whitelist, block_added must
    /// have no reads, writes, random draws, or other observable effects.
    fn supports_inert_ore_writes(&self) -> bool { false }
    /// Separate pure opt-in for fixed default-ore coordinate speculation.
    /// Real RNG transitions and all callbacks remain on the original owner.
    fn supports_ore_coordinate_lookahead(&self) -> bool { false }
    /// Additional live actors whose original preventEntitySpawning flag is set.
    /// The local player and falling blocks are already queried by the runtime.
    fn placement_blocked(&self,_bounds:super::movement::Aabb)->bool {false}
    fn item_random_state(&self, _id: u64) -> u64 {
        self.player_random_state()
    }
    fn orb_random_state(&self,_id:i32)->u64 {self.player_random_state()}
    fn player_random_state(&self) -> u64 {
        let mut r = JavaRandom::new(JavaRandom::independent_seed());
        r.next_long();
        r.next_long();
        r.state()
    }
    fn item_contact(
        &self,
        _access: &mut Access<'_>,
        p: IVec3,
        state: u16,
        e: &mut super::item_entity::ItemEntity,
    ) -> Result<()> {
        let requires = match state >> 4 {
            70 | 72 | 90 | 119 | 132 | 147 | 148 => true,
            28 | 143 => state & 8 == 0,
            118 => state & 3 != 0 && e.fire > 0,
            _ => false,
        };
        ensure!(
            !requires,
            "Item contact with block {} at {p:?} requires its entity callback",
            state >> 4
        );
        Ok(())
    }
    /// Entity constructs its own Random, then consumes both UUID longs before
    /// BlockFalling initializes the body. Hosts may supply the corresponding
    /// post-construction state from their entity allocator.
    fn falling_random_state(&self, _id: u64) -> u64 {
        let mut random = JavaRandom::new(JavaRandom::independent_seed());
        random.next_long();
        random.next_long();
        random.state()
    }
    fn falling_damage(
        &self,
        _access: &mut Access<'_>,
        _bounds: super::movement::Aabb,
        _anvil: bool,
        _amount: f32,
    ) -> Result<()> {
        anyhow::bail!("Falling block damage requires an authoritative living-entity registry")
    }
    fn falling_contact(
        &self,
        _access: &mut Access<'_>,
        p: IVec3,
        state: u16,
        entity: &mut super::falling::Falling,
    ) -> Result<()> {
        let stateful = match state >> 4 {
            70 | 72 | 90 | 119 | 132 | 147 | 148 => true,
            28 | 143 => state & 8 == 0,
            118 => state & 3 != 0 && entity.fire > 0,
            _ => false,
        };
        ensure!(
            !stateful,
            "Falling block contact with block {} at {p:?} requires its entity callback",
            state >> 4
        );
        Ok(())
    }
    fn raining_at(&self, access: &Access<'_>, p: IVec3) -> bool {
        fire::raining_at(access, p)
    }
    /// The eventful generator has no entity/circuit activation step. A gameplay
    /// host must supply its exact redstone query before activating sources.
    fn is_powered(&self, _access: &Access<'_>, _p: IVec3) -> bool {
        false
    }
    fn key(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
    fn initialize_light(&self, access: &mut Access<'_>, pos: ChunkPos) -> Result<bool> {
        Ok(access.with(|provider, backend| {
            backend
                .light
                .initialize_chunk(&backend.world, pos, 63, |p| {
                    provider.record(p).is_some_and(|r| r.terrain_populated)
                })
        }))
    }
    fn populate(
        &self,
        access: &mut Access<'_>,
        pos: ChunkPos,
        random: &mut JavaRandom,
    ) -> Result<()>;
    fn repopulate(&self, _access: &mut Access<'_>, _pos: ChunkPos) -> Result<bool> {
        Ok(false)
    }
    fn block_added(&self, _access: &mut Access<'_>, _p: IVec3, _old: u16, _new: u16) -> Result<()> {
        Ok(())
    }
}

pub struct Runtime {
    pub provider: Provider,
    pub backend: RuntimeBackend,
}
pub struct RuntimeBackend {
    pub entities: NativeEntities,
    pub seed: i64,
    pub world_spawn: Option<IVec3>,
    pub initial_player: Option<InitialPlayer>,
    pub world: World,
    pub light: Lighting,
    pub structures: NativeStructures,
    pub map_features: bool,
    pub output: Population,
    pub ticks: BlockTicks,
    pub block_effects: TickResult,
    pub events: Vec<(String, ChunkPos)>,
    pub provider_random: JavaRandom,
    pub world_random: JavaRandom,
    pub double_plant_type: u16,
    pub plains_counts: BTreeMap<u8, (i32, i32)>,
    pub precipitation: BTreeMap<ChunkPos, Vec<i32>>,
    pub structure_entities: structures::StructureEntities,
    pub live_containers: BTreeMap<[i32; 3], LiveContainer>,
    pub tile_revision: u64,
    pub player_inventory: Option<NativePlayerInventory>,
    pub live_spawners: BTreeMap<[i32; 3], String>,
    pub falling: FallingManager,
    pub animals: Vec<super::spawning::AnimalSpawn>,
    pub drops: Vec<super::drops::DropEvent>,
    pub drop_tiles: BTreeMap<[i32; 3], super::drops::DropTile>,
    pub do_tile_drops: bool,
    pub fire_environment: FireEnvironment,
    pub pending_tnt: Vec<PendingTnt>,
    pub dirty_chunks: HashSet<ChunkPos>,
    settled_light: HashSet<ChunkPos>,
    settled_loaded_len: usize,
    settled_populated_len: usize,
    decorators: BTreeMap<u8, Decorator>,
    active_decorators: HashSet<u8>,
    terrain: Arc<Terrain>,
    // Terrain and initial per-column light can be prepared in parallel.
    // Population, discovery and RNG publication retain the serial order.
    prepared_terrain: HashMap<ChunkPos, terrain_work::Prepared>,
    terrain_jobs: HashMap<ChunkPos, terrain_work::TerrainJob>,
    terrain_timings: TerrainTimings,
    profile: profiling::Profile,
    // Callback-local results are removed before entering BlockTicks. Nested
    // writes therefore never share a buffer or borrow, and checkpoints omit it.
    write_scratch: Vec<TickResult>,
    // Standalone diagnostic runtimes keep their legacy sparse write journal.
    // NativeSession saves full chunks and clears this journal after every
    // command, so it can skip the redundant insertion at each block write.
    record_overlay_edits: bool,
    // Standalone Runtime snapshots expose ordered per-write diagnostics. The
    // NativeSession owner can omit only inert-ore positions: its ordinary
    // command publication clears both lists and publishes complete dirty chunks.
    record_inert_change_diagnostics: bool,
    #[cfg(test)]
    reference_access: bool,
    #[cfg(test)]
    reference_prepared_light: bool,
    #[cfg(test)]
    reference_precipitation_scan: bool,
    #[cfg(test)]
    reference_publication: bool,
    #[cfg(test)]
    reference_write_scratch: bool,
    #[cfg(test)]
    ore_lookahead_mode: Option<super::population::ore_lookahead::TestMode>,
    #[cfg(test)]
    reference_ore_batch: bool,
    #[cfg(test)]
    reference_prechecked_write: bool,
    #[cfg(test)]
    reference_inert_ore: bool,
    #[cfg(test)]
    inert_ore_writes: u64,
    #[cfg(test)]
    reference_inert_column: bool,
    #[cfg(test)]
    inert_column_writes: u64,
    #[cfg(test)]
    inert_column_batches: u64,
    biomes: Arc<BiomeGenerator>,
    hooks: Arc<dyn Hooks>,
}
impl Runtime {
    /// World.rand has an independent seed in vanilla. Tests must supply it;
    /// deriving it implicitly from the terrain seed would invent behavior.
    pub fn new(
        seed: i64,
        world_random_seed: i64,
        map_features: bool,
        hooks: Arc<dyn Hooks>,
    ) -> Self {
        let world = World {
            chunks: crate::world::ChunkMap::new(),
            spawn: glam::Vec3::ZERO,
            label: "Native runtime verification".into(),
            save_path: PathBuf::new(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        };
        let light = Lighting::new(&world, false);
        let terrain = Terrain::new(seed);
        let provider_random = terrain.initial_random();
        Self {
            provider: Provider::default(),
            backend: RuntimeBackend {
                entities: NativeEntities::fresh(),
                seed,
                world_spawn: None,
                initial_player: None,
                world,
                light,
                structures: NativeStructures::new(seed),
                map_features,
                output: Population::default(),
                ticks: BlockTicks::default(),
                block_effects: TickResult::default(),
                events: Vec::new(),
                provider_random,
                world_random: JavaRandom::new(world_random_seed),
                double_plant_type: 0,
                plains_counts: BTreeMap::new(),
                precipitation: BTreeMap::new(),
                structure_entities: structures::StructureEntities::default(),
                live_containers: BTreeMap::new(),
                tile_revision: 0,
                player_inventory: None,
                live_spawners: BTreeMap::new(),
                falling: FallingManager::default(),
                animals: Vec::new(),
                drops: Vec::new(),
                drop_tiles: BTreeMap::new(),
                do_tile_drops: true,
                fire_environment: FireEnvironment::default(),
                pending_tnt: Vec::new(),
                dirty_chunks: HashSet::new(),
                settled_light: HashSet::new(),
                settled_loaded_len: 0,
                settled_populated_len: 0,
                decorators: BTreeMap::new(),
                active_decorators: HashSet::new(),
                terrain: Arc::new(terrain),
                prepared_terrain: HashMap::new(),
                terrain_jobs: HashMap::new(),
                terrain_timings: TerrainTimings::default(),
                profile: profiling::Profile::default(),
                write_scratch: Vec::new(),
                record_overlay_edits: true,
                record_inert_change_diagnostics: true,
                #[cfg(test)]
                reference_access: false,
                #[cfg(test)]
                reference_prepared_light: false,
                #[cfg(test)]
                reference_precipitation_scan: false,
                #[cfg(test)]
                reference_publication: false,
                #[cfg(test)]
                reference_write_scratch: false,
                #[cfg(test)]
                ore_lookahead_mode: None,
                #[cfg(test)]
                reference_ore_batch: false,
                #[cfg(test)]
                reference_prechecked_write: false,
                #[cfg(test)]
                reference_inert_ore: false,
                #[cfg(test)]
                inert_ore_writes: 0,
                #[cfg(test)]
                reference_inert_column: false,
                #[cfg(test)]
                inert_column_writes: 0,
                #[cfg(test)]
                inert_column_batches: 0,
                biomes: Arc::new(BiomeGenerator::new(seed)),
                hooks,
            },
        }
    }
    pub fn provide(&mut self, pos: ChunkPos) -> Result<Option<ChunkPos>> {
        self.provider.provide(&mut self.backend, pos)
    }
    pub fn access(&mut self) -> Access<'_> {
        Access::new(&mut self.provider, &mut self.backend)
    }
    pub(crate) fn use_overlay_journal(&mut self, enabled: bool) {
        self.backend.record_overlay_edits=enabled;
        if !enabled { self.backend.world.edits=Default::default(); }
    }
    /// NativeSession-only policy; does not clear existing/restored diagnostics.
    /// Runtime::new/resume keep the ordinary default, and snapshots omit policy.
    pub(crate) fn retain_inert_change_diagnostics(&mut self, enabled: bool) {
        self.backend.record_inert_change_diagnostics = enabled;
    }
    pub fn for_overworld_verification(
        seed: i64,
        world_random_seed: i64,
        map_features: bool,
    ) -> Self {
        Self::new(
            seed,
            world_random_seed,
            map_features,
            Arc::new(VanillaPopulation),
        )
    }
    /// Every block- or light-dirty chunk, in stable coordinate order, with an
    /// immutable snapshot of the authoritative light arrays. A mesh consumer
    /// should also invalidate adjacent meshes that sample this chunk's edges.
    pub fn take_chunk_updates(&mut self) -> Vec<(ChunkPos, Arc<crate::world::Chunk>)> {
        publication::take(self)
    }
    fn lighting_enclosed(&self, pos: ChunkPos) -> bool {
        (-2..=2).all(|dx| (-2..=2).all(|dz| self.provider.contains((pos.0 + dx, pos.1 + dz))))
            && (-2..=1).all(|dx| {
                (-2..=1).all(|dz| {
                    self.provider
                        .record((pos.0 + dx, pos.1 + dz))
                        .is_some_and(|r| r.terrain_populated)
                })
            })
    }
    pub(crate) fn mark_enclosed_lighting_settled(&mut self) {
        let settled: Vec<ChunkPos> = self
            .provider
            .loaded_order()
            .iter()
            .copied()
            .filter(|&pos| self.lighting_enclosed(pos))
            .collect();
        self.backend.settled_light.extend(settled);
        self.backend.settled_loaded_len = self.provider.loaded_order().len();
        self.backend.settled_populated_len = self.provider.population_order.len();
    }
    /// Complete deferred initial column/overhang lighting and gap rechecks once
    /// a chunk's 5x5 light neighborhood is loaded and all 16 population sources
    /// that can touch its 3x3 column neighborhood have finished populating.
    pub fn settle_populated_lighting(&mut self) -> Result<()> {
        let loaded_len = self.provider.loaded_order().len();
        let populated_len = self.provider.population_order.len();
        if self.backend.light.no_sky {
            self.backend.settled_loaded_len = loaded_len;
            self.backend.settled_populated_len = populated_len;
            return Ok(());
        }
        if self.backend.settled_loaded_len == loaded_len
            && self.backend.settled_populated_len == populated_len
        {
            return Ok(());
        }
        let mut neighborhood = std::collections::BTreeSet::new();
        for &origin in &self.provider.loaded_order()[self.backend.settled_loaded_len..] {
            for dx in -2..=2 {
                for dz in -2..=2 {
                    neighborhood.insert((origin.0 + dx, origin.1 + dz));
                }
            }
        }
        for &origin in &self.provider.population_order[self.backend.settled_populated_len..] {
            for dx in -2..=2 {
                for dz in -2..=2 {
                    neighborhood.insert((origin.0 + dx, origin.1 + dz));
                }
            }
        }
        self.backend.settled_loaded_len = loaded_len;
        self.backend.settled_populated_len = populated_len;
        let mut candidates = Vec::new();
        for pos in neighborhood {
            if !self.provider.contains(pos) {
                continue;
            }
            let needs_initial = !self.backend.settled_light.contains(&pos);
            let needs_gaps = !needs_initial
                && self
                    .backend
                    .light
                    .chunks
                    .get(&pos)
                    .is_some_and(|c| c.gaps.iter().any(|&g| g));
            if !needs_initial && !needs_gaps {
                continue;
            }
            if self.lighting_enclosed(pos) {
                candidates.push((pos, needs_initial));
            }
        }
        for (pos, needs_initial) in candidates {
            if needs_initial {
                let ok = self.backend.light.initialize_chunk(
                    &self.backend.world,
                    pos,
                    63,
                    |p| self.provider.record(p).is_some_and(|r| r.terrain_populated),
                );
                debug_assert!(ok);
                if self
                    .provider
                    .record(pos)
                    .is_some_and(|r| !r.light_populated)
                {
                    let record = self.provider.record_mut(pos).unwrap();
                    record.light_populated = true;
                    record.modified = true;
                }
                self.backend.settled_light.insert(pos);
            } else {
                self.backend
                    .light
                    .recheck_gaps(&self.backend.world, pos, false);
            }
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn use_reference_publication(&mut self, enabled: bool) {
        self.backend.reference_publication = enabled;
    }
}
impl RuntimeBackend {
    fn event(&mut self, name: &str, pos: ChunkPos) {
        self.events.push((name.into(), pos));
    }
}
impl Backend for RuntimeBackend {
    fn load_saved(&mut self, _: &mut Provider, _: ChunkPos) -> Result<Option<ChunkRecord>> {
        Ok(None)
    }
    fn generate(&mut self, _: &mut Provider, pos: ChunkPos) -> Result<ChunkRecord> {
        self.event("generate", pos);
        let (chunk, prepared_light) = self.measured(Phase::Terrain, |this| -> Result<_> { Ok(if let Some((chunk, state, light)) = this.take_prepared_terrain(pos)? {
            // Preserve shared/reentrant references to the provider Random.
            // setSeed scrambles with this multiplier, so this restores raw bits.
            this.provider_random.set_seed((state ^ 0x5deece66d) as i64);
            (chunk, light)
        } else {
            (this.terrain.generate_with_random(pos.0, pos.1, &this.biomes, &mut this.provider_random)?, None)
        }) })?;
        if self.map_features {
            self.measured(Phase::Discovery, |this| this.structures
                .discover(pos, &this.biomes, &mut this.world_random));
        }
        self.world.chunks.insert(pos, Arc::new(chunk));
        self.dirty_chunks.insert(pos);
        self.precipitation.insert(pos, vec![-999; 256]);
        self.measured(Phase::AddLight, |this| {
            #[cfg(test)]
            let prepared_light = if this.reference_prepared_light { None } else { prepared_light };
            if let Some(light) = prepared_light {
                if this.light.add_prepared_chunk(&this.world, pos, light) {
                    this.terrain_timings.prepared_light_installed += 1;
                } else {
                    this.terrain_timings.prepared_light_fallbacks += 1;
                }
            } else {
                this.light.add_chunk(&this.world, pos);
            }
        });
        // Original Chunk.generateSkylightMap marks the generated chunk dirty.
        Ok(ChunkRecord {
            modified: true,
            ..ChunkRecord::default()
        })
    }
    fn recreate_structures(&mut self, _: &mut Provider, pos: ChunkPos) -> Result<()> {
        if self.map_features {
            self.structures
                .discover(pos, &self.biomes, &mut self.world_random);
        }
        Ok(())
    }
    fn on_load(&mut self, _: &mut Provider, pos: ChunkPos) -> Result<()> {
        self.event("on_load", pos);
        Ok(())
    }
    fn initialize_light(&mut self, provider: &mut Provider, pos: ChunkPos) -> Result<bool> {
        let hooks = self.hooks.clone();
        self.measured(Phase::InitializeLight, |this| hooks.initialize_light(&mut Access::new(provider, this), pos))
    }
    fn populate(&mut self, provider: &mut Provider, pos: ChunkPos) -> Result<()> {
        self.event("populate", pos);
        let hooks = self.hooks.clone();
        let mut random = self.provider_random.share();
        self.measured(Phase::Population, |this| hooks.populate(&mut Access::new(provider, this), pos, &mut random))
    }
    fn repopulate(&mut self, provider: &mut Provider, pos: ChunkPos) -> Result<bool> {
        let hooks = self.hooks.clone();
        hooks.repopulate(&mut Access::new(provider, self), pos)
    }
}

pub struct Access<'a> {
    // A nested Backend callback creates another Access using safe reborrows.
    // It never tries to acquire this suspended outer cell a second time.
    context: RefCell<(&'a mut Provider, &'a mut RuntimeBackend)>,
}
fn valid(p: IVec3) -> bool {
    (0..256).contains(&p.y)
        && (-30_000_000..30_000_000).contains(&p.x)
        && (-30_000_000..30_000_000).contains(&p.z)
}
fn cp(p: IVec3) -> ChunkPos {
    (p.x.div_euclid(16), p.z.div_euclid(16))
}
fn invalidate_precipitation(cache: &mut BTreeMap<ChunkPos, Vec<i32>>, pos: ChunkPos, p: IVec3) {
    if let Some(cache) = cache.get_mut(&pos) {
        invalidate_precipitation_column(cache, p);
    }
}
#[inline]
fn invalidate_precipitation_column(cache: &mut [i32], p: IVec3) {
    let col = (p.z.rem_euclid(16) * 16 + p.x.rem_euclid(16)) as usize;
    if p.y >= cache[col] - 1 {
        cache[col] = -999;
    }
}
impl<'a> Access<'a> {
    fn new(provider: &'a mut Provider, backend: &'a mut RuntimeBackend) -> Self {
        Self {
            context: RefCell::new((provider, backend)),
        }
    }
    pub fn with<R>(&self, f: impl FnOnce(&mut Provider, &mut RuntimeBackend) -> R) -> R {
        let mut context = self.context.borrow_mut();
        let (provider, backend) = &mut *context;
        f(provider, backend)
    }
    pub fn provide(&self, pos: ChunkPos) -> Result<Option<ChunkPos>> {
        self.with(|provider, backend| {
            #[cfg(test)]
            if backend.reference_access { return provider.provide(backend, pos); }
            // Same publication invariant as get_block: an installed column
            // already has its provider record and has no loaded-path callback.
            if backend.world.chunks.contains_key(&pos) { return Ok(Some(pos)); }
            provider.provide(backend, pos)
        })
    }
    pub fn get_block(&self, p: IVec3) -> Result<u16> {
        if !valid(p) {
            return Ok(0);
        }
        self.with(|provider, backend| {
            backend.profile.access_reads += 1;
            #[cfg(test)]
            if backend.reference_access {
                if provider.provide(backend, cp(p))?.is_none() { return Ok(0); }
                return Ok(backend.world.get(p));
            }
            // Runtime publishes block storage immediately before Provider's
            // record, with no intervening Access/Hooks callback. Resume also
            // restores a validated storage/provider bijection before exposing
            // Access. A present column is therefore already logically loaded;
            // repeating Provider's BTree lookup cannot add any lifecycle work.
            if let Some(chunk) = backend.world.chunks.get(&cp(p)) {
                return Ok(chunk.get(p.x.rem_euclid(16), p.y, p.z.rem_euclid(16)));
            }
            if provider.provide(backend, cp(p))?.is_none() {
                return Ok(0);
            }
            Ok(backend.world.get(p))
        })
    }
    /// Chunk write phase before Block.onBlockAdded. The caller can run original
    /// callbacks, then finish_world_write without holding a world/light borrow.
    pub fn set_before_added(&self, p: IVec3, state: u16) -> Result<bool> {
        if !valid(p) {
            return Ok(false);
        }
        self.with(|provider, backend| {
            let direct = true;
            #[cfg(test)]
            let direct = direct && !backend.reference_access;
            let loaded = direct && backend.world.chunks.contains_key(&cp(p));
            if !loaded && provider.provide(backend, cp(p))?.is_none() {
                return Ok(false);
            }
            let old = backend.world.get(p);
            if old == state {
                return Ok(false);
            }
            backend.profile.access_writes += 1;
            let tile_drop=tile_break::item(&backend.world,p,old,state);
            let written=if backend.record_overlay_edits { backend.world.set(p,state) }
                else { backend.world.set_raw(p,state) };
            ensure!(
                written,
                "Published chunk missing during native write"
            );
            if let Some(item)=tile_drop{
                let mut random=backend.world_random.share();
                super::drops::spawn_item(&mut Access::new(provider,backend),&mut random,p,item)?;
            }
            if old >> 4 != state >> 4 {
                if let Some(tile) = backend.live_containers.get(&p.to_array()) {
                    backend.entities.spill(
                        backend.hooks.as_ref(),
                        &backend.world,
                        &mut backend.drops,
                        tile,
                    )?;
                }
            }
            let mut ticks = backend.ticks.share_scheduler();
            break_callbacks::leaves(
                &mut Access::new(provider, backend),
                &mut ticks,
                p,
                old,
                state,
            );
            rail_effects::removed(&mut Access::new(provider, backend), p, old, state);
            if backend.world.get(p) >> 4 != state >> 4 {
                return Ok(false);
            }
            structures::tile_state_changed(
                &mut backend.live_containers,
                &mut backend.tile_revision,
                &mut backend.live_spawners,
                p.to_array(),
                old,
                state,
            );
            if old >> 4 != state >> 4 {
                backend.drop_tiles.remove(&p.to_array());
            }
            backend.light.chunk_block_changed(&backend.world, p, old);
            backend.output.changed.push(p);
            backend.dirty_chunks.insert(cp(p));
            provider.mark_block_modified(cp(p));
            Ok(true)
        })
    }
    pub fn finish_world_write(&self, p: IVec3, old: u16, requested: u16) {
        self.with(|_, backend| {
            backend
                .light
                .after_block_added_state(&backend.world, p, old, requested)
        });
    }
    pub fn set_block(&mut self, p: IVec3, state: u16) -> Result<bool> {
        self.set_block_flags(p, state, 2)
    }
    pub fn set_block_flags(&mut self, p: IVec3, state: u16, flags: u8) -> Result<bool> {
        if !valid(p) {
            return Ok(false);
        }
        self.before_set_attempt(p);
        let old = self.get_block(p)?;
        if old == state {
            return Ok(false);
        }
        let (mut ticks, mut result, prechecked) = self.with(|_, backend| {
            let result = backend.write_scratch.pop().unwrap_or_default();
            #[cfg(test)]
            let result = if backend.reference_write_scratch { TickResult::default() } else { result };
            let prechecked = true;
            #[cfg(test)]
            let prechecked = prechecked && !backend.reference_prechecked_write;
            if prechecked {
                // Preserve the logical-access diagnostic while eliding the
                // duplicate loaded/no-load read in BlockTicks::write.
                backend.profile.access_reads += 1;
            }
            (backend.ticks.share_scheduler(), result, prechecked)
        });
        if prechecked {
            // Only scratch allocation and scheduler-handle cloning occurred
            // since old was read. A repeated before_set_attempt would repeat
            // the same loaded/no-load lookup and idempotent cache invalidation.
            ticks.set_block_prechecked_into(self, p, old, state, flags & 1 != 0, &mut result);
        } else {
            ticks.set_block_into(self, p, state, flags & 1 != 0, &mut result);
        }
        let changed = result.changed.contains(&p);
        self.with(|_, backend| {
            backend.block_effects.changed.append(&mut result.changed);
            backend.block_effects.drops.append(&mut result.drops);
            backend.block_effects.washed.append(&mut result.washed);
            result.clear();
            // Most writes need one entry; a few callbacks recurse. Retain a
            // bounded pool rather than one allocation per generated ore block.
            if backend.write_scratch.len() < 4 {
                backend.write_scratch.push(result);
            }
        });
        Ok(changed)
    }
    pub fn world_random(&self) -> JavaRandom {
        self.with(|_, backend| backend.world_random.share())
    }
    fn combined(&self, p: IVec3, neighbors: bool) -> Result<u8> {
        if !(-30_000_000..30_000_000).contains(&p.x) || !(-30_000_000..30_000_000).contains(&p.z) {
            return Ok(15);
        }
        if neighbors && super::material::neighbor_brightness(self.get_block(p)?) {
            let mut value = 0;
            for d in [IVec3::Y, IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z] {
                value = value.max(self.combined(p + d, false)?);
            }
            return Ok(value);
        }
        if p.y < 0 {
            return Ok(0);
        }
        let p = IVec3::new(p.x, p.y.min(255), p.z);
        self.provide(cp(p))?;
        Ok(self.with(|_, backend| backend.light.combined(&backend.world, p, false)))
    }
}
impl BlockAccess for Access<'_> {
    fn scheduled_tick(&mut self, p: IVec3, state: u16) -> bool {
        if state >> 4 != 51 { return false; }
        fire::update(self, p, state);
        true
    }
    fn spawn_falling(&mut self, p: IVec3, state: u16) -> bool {
        self.with(|_, b| {
            let random = b.hooks.falling_random_state(b.falling.next_id);
            let id = b.falling.next_id;
            b.entities.allocate_entity_id();
            let result = b.falling.spawn(p, state, random);
            if result {
                b.entities.order.push(EntityKey::Falling(id));
            }
            result
        })
    }
    fn neighbor_changed(&mut self, p: IVec3, state: u16, source: u16) {
        plant_effects::changed(self, p, state);
        rail_effects::changed(self, p, state, source);
        fire::changed(self,p,state);
        if let Some((pos,id))=openables::changed(self,p,state,source){
            self.with(|_,b|b.entities.effects.push(EntityEffect::WorldEvent{position:pos.to_array(),id}));
        }
    }
    fn drop_block_item(&mut self, p: IVec3, state: u16) -> bool {
        let mut random = self.world_random();
        super::drops::drop_block(self, &mut random, p, state, 1., 0)
            .expect("Native block drop callback failed");
        true
    }
    fn before_set_attempt(&mut self, p: IVec3) {
        if !valid(p) {
            return;
        }
        let pos = cp(p);
        self.provide(pos)
            .expect("Native write target load failed");
        self.with(|_, b| invalidate_precipitation(&mut b.precipitation, pos, p));
    }
    fn get(&self, p: IVec3) -> u16 {
        self.get_block(p)
            .expect("Native callback block read failed")
    }
    fn chunk_loaded(&self, pos: ChunkPos) -> bool {
        self.with(|p, _| p.contains(pos))
    }
    fn accessible(&self, p: IVec3) -> bool {
        let _ = p;
        true
    }
    fn set_before_added(&mut self, p: IVec3, state: u16) -> bool {
        Access::set_before_added(self, p, state).expect("Native callback block write failed")
    }
    fn after_added(&mut self, p: IVec3, old: u16, requested: u16) {
        plant_effects::added(self, p, old, requested);
        fire::added(self,p,old,requested);
        rail_effects::added(self, p, old, requested);
        facing_effects::added(self, p, old, requested);
        let hooks = self.with(|_, backend| backend.hooks.clone());
        hooks
            .block_added(self, p, old, requested)
            .expect("Native Block.onBlockAdded failed");
        self.finish_world_write(p, old, requested);
    }
    fn world_random_int(&mut self, bound: i32) -> Option<i32> {
        Some(self.with(|_, backend| backend.world_random.next_int(bound)))
    }
}
impl super::drops::DropWorld for Access<'_> {
    fn is_remote(&self) -> bool {
        false
    }
    fn do_tile_drops(&self) -> bool {
        self.with(|_, b| b.do_tile_drops)
    }
    fn tile_entity(&mut self, p: IVec3) -> Result<Option<super::drops::DropTile>> {
        Ok(self.with(|_, b| b.drop_tiles.get(&p.to_array()).cloned()))
    }
    fn emit(&mut self, event: super::drops::DropEvent) -> Result<()> {
        self.with(|_, b| {
            b.entities
                .emit(b.hooks.as_ref(), &b.world, &mut b.drops, event)
        })
    }
}
impl Access<'_> {
    fn prepare_ore_geometry(&mut self, random: &mut JavaRandom, origin: IVec3) -> Option<OreLookaheadStats> {
        if !self.with(|_, b| b.hooks.supports_ore_coordinate_lookahead()) { return None; }
        #[cfg(test)]
        if let Some(mode) = self.with(|_, b| b.ore_lookahead_mode) {
            return super::population::ore_lookahead::run_for_test(self, random, origin, mode);
        }
        let pool = terrain_work::publication_pool().ok()?;
        super::population::ore_lookahead::try_run(self, random, origin, pool)
    }
}
impl FeatureWorld for Access<'_> {
    fn generate_default_ores(&mut self, random: &mut JavaRandom, origin: IVec3) {
        self.measured(Phase::Ores, |access| {
            if let Some(stats) = access.prepare_ore_geometry(random, origin) {
                access.with(|_, b| b.profile.ore_lookahead.add(stats));
            } else {
                super::population::default_ores(access, random, origin);
            }
        });
    }
    fn replace_ore_line(&mut self, positions: &[IVec3], ore: u16, replace_id: u16) {
        if self.replace_inert_ore_line(positions, ore, replace_id) { return; }
        #[cfg(test)]
        if self.with(|_, b| b.reference_ore_batch) {
            for &p in positions {
                if self.get_block(p).expect("Native feature block read failed") >> 4 == replace_id {
                    self.set_block(p, ore).expect("Native feature block write failed");
                }
            }
            return;
        }
        enum Next { Read(IVec3), Write(IVec3), Continue }
        let mut cursor = 0;
        while cursor < positions.len() {
            let next = self.with(|_, b| {
                let p = positions[cursor];
                if !valid(p) { return Next::Read(p); }
                let pos = cp(p);
                let Some(chunk) = b.world.chunks.get(&pos) else { return Next::Read(p); };
                // Already-loaded non-matching reads and exact no-op attempts
                // share this borrow. Stop before a changed write or missing
                // read: either can reenter population and mutate neighbors.
                while cursor < positions.len() {
                    let p = positions[cursor];
                    if !valid(p) || cp(p) != pos { break; }
                    b.profile.access_reads += 1;
                    let state = chunk.get(p.x.rem_euclid(16), p.y, p.z.rem_euclid(16));
                    cursor += 1;
                    if state >> 4 == replace_id {
                        if state != ore { return Next::Write(p); }
                        // set_block_flags would perform a side-effect-free
                        // loaded provide, invalidate precipitation, read this
                        // unchanged state once more, then return before every
                        // scheduler, hook, light, dirty or RNG operation.
                        invalidate_precipitation(&mut b.precipitation, pos, p);
                        b.profile.access_reads += 1;
                    }
                }
                Next::Continue
            });
            match next {
                Next::Read(p) => {
                    cursor += 1;
                    if self.get_block(p).expect("Native feature block read failed") >> 4 == replace_id {
                        self.set_block(p, ore).expect("Native feature block write failed");
                    }
                }
                Next::Write(p) => { self.set_block(p, ore).expect("Native feature block write failed"); }
                Next::Continue => {}
            }
        }
    }
    fn set_plains_counts(&mut self, id: u8, flowers: i32, grass: i32) {
        self.with(|_, b| {
            b.plains_counts.insert(id, (flowers, grass));
        });
    }
    fn plains_counts(&self, id: u8, fallback: (i32, i32)) -> (i32, i32) {
        self.with(|_, b| b.plains_counts.get(&id).copied().unwrap_or(fallback))
    }
    fn precipitation_height(&self, x: i32, z: i32) -> i32 {
        let pos = (x.div_euclid(16), z.div_euclid(16));
        self.provide(pos).expect("Native precipitation read failed");
        self.with(|_, b| {
            let col = (z.rem_euclid(16) * 16 + x.rem_euclid(16)) as usize;
            let cached = b.precipitation[&pos][col];
            if cached != -999 {
                return cached;
            }
            let top = b.light.chunks[&pos]
                .sections
                .iter()
                .rposition(|v| *v)
                .unwrap_or(0) as i32
                * 16
                + 15;
            // provide() completed before this borrow. This fixed column scan
            // contains no callbacks, writes, RNG draws or Access read counts.
            let chunk = b.world.chunks.get(&pos);
            let read = |y| {
                #[cfg(test)]
                if b.reference_precipitation_scan { return b.world.get(IVec3::new(x, y, z)); }
                chunk.map_or(0, |c| c.get(x.rem_euclid(16), y, z.rem_euclid(16)))
            };
            let value = (1..=top)
                .rev()
                .find(|&y| {
                    let state = read(y);
                    super::material::blocks_movement(state) || super::material::is_liquid(state)
                })
                .map_or(-1, |y| y + 1);
            b.precipitation.get_mut(&pos).unwrap()[col] = value;
            value
        })
    }
    fn set_double_plant_type(&mut self, meta: u16) {
        self.with(|_, backend| backend.double_plant_type = meta);
    }
    fn double_plant_type(&self, _fallback: u16) -> u16 {
        self.with(|_, backend| backend.double_plant_type)
    }
    fn get(&self, p: IVec3) -> u16 {
        self.get_block(p).expect("Native feature block read failed")
    }
    fn set(&mut self, p: IVec3, state: u16) {
        self.set_block(p, state)
            .expect("Native feature block write failed");
    }
    fn biome(&self, p: IVec3) -> u8 {
        self.with(|provider, backend| {
            // World.getBiomeGenForCoords does not create an unloaded chunk.
            if provider.contains(cp(p)) {
                backend.world.chunks[&cp(p)].biomes
                    [(p.z.rem_euclid(16) * 16 + p.x.rem_euclid(16)) as usize]
            } else {
                backend.biomes.get_biomes(p.x, p.z, 1, 1)[0]
            }
        })
    }
    fn light(&self, p: IVec3, sky: bool) -> u8 {
        // getLightFor checks isBlockLoaded and never creates a neighbor.
        self.with(|_, backend| {
            backend
                .light
                .get(if sky { Kind::Sky } else { Kind::Block }, p)
        })
    }
    fn combined_light(&self, p: IVec3) -> u8 {
        self.combined(p, true)
            .expect("Native combined light read failed")
    }
    fn height(&self, x: i32, z: i32) -> i32 {
        if !(-30_000_000..30_000_000).contains(&x) || !(-30_000_000..30_000_000).contains(&z) {
            return 64;
        }
        // World.getHeight returns zero if this chunk is not yet loaded.
        self.with(|_, backend| backend.light.height(IVec3::new(x, 0, z)))
    }
    fn chest(&mut self, p: IVec3, slots: Vec<Option<LootStack>>) {
        self.with(|_, backend| {
            let source = super::scattered::GeneratedContainer {
                position: p.to_array(),
                block: 54,
                slots: slots.clone(),
            };
            backend.live_containers.insert(
                p.to_array(),
                LiveContainer::generated(
                    &source,
                    inventories::next_tile_revision(&mut backend.tile_revision),
                ),
            );
            backend.output.chests.push((p, slots));
        });
    }
    fn spawner(&mut self, p: IVec3, entity: &'static str) {
        self.with(|_, backend| {
            backend.live_spawners.insert(p.to_array(), entity.into());
            if let Some(chunk) = backend.world.chunks.get_mut(&cp(p)) {
                let local = [p.x.rem_euclid(16), p.y, p.z.rem_euclid(16)];
                if let Some(tile) = Arc::make_mut(chunk).tiles.get_mut(&local) {
                    if let super::drops::DropNbt::Compound(tag) = Arc::make_mut(tile) {
                        tag.insert("EntityId".into(), super::drops::DropNbt::String(entity.into()));
                    }
                }
            }
            backend.output.spawners.push((p, entity.into()));
        });
    }
    fn entity(&mut self, entity: GeneratedEntity) {
        self.with(|_, backend| backend.output.entities.push(entity));
    }
}

#[cfg(test)]
#[path = "runtime_access_tests.rs"]
mod access_tests;
#[cfg(test)]
#[path = "runtime_precipitation_lookup_tests.rs"]
mod precipitation_lookup_tests;

#[cfg(test)]
#[path = "runtime_ore_access_tests.rs"]
mod ore_access_tests;
#[cfg(test)]
#[path = "runtime_journal_tests.rs"]
mod journal_tests;
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct OracleHooks;
    impl Hooks for OracleHooks {
        fn populate(
            &self,
            access: &mut Access<'_>,
            (x, z): ChunkPos,
            random: &mut JavaRandom,
        ) -> Result<()> {
            let seed = access.with(|_, backend| backend.seed);
            random.set_seed(seed);
            let a = (random.next_long() / 2).wrapping_mul(2).wrapping_add(1);
            let b = (random.next_long() / 2).wrapping_mul(2).wrapping_add(1);
            random.set_seed(
                (i64::from(x)
                    .wrapping_mul(a)
                    .wrapping_add(i64::from(z).wrapping_mul(b)))
                    ^ seed,
            );
            let before = random.next_int(1000);
            let world_before = access.world_random().next_int(17);
            access.with(|_, backend| {
                backend.event(&format!("before:{before}:{world_before}"), (x, z))
            });
            if (x, z) == (0, 0) {
                access.get_block(IVec3::new(32, 70, 0))?;
            }
            if (x, z) == (1, 0) {
                access.get_block(IVec3::new(48, 70, 0))?;
                access.get_block(IVec3::new(48, 70, 16))?;
            }
            access.set_block(IVec3::new(x * 16 + 8, 80, z * 16 + 8), 89 << 4)?;
            let after = random.next_int(1000);
            let world_after = access.world_random().next_int(17);
            access
                .with(|_, backend| backend.event(&format!("after:{after}:{world_after}"), (x, z)));
            Ok(())
        }
    }
    fn hash(bytes: impl Iterator<Item = u8>) -> String {
        format!(
            "{:x}",
            bytes.fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(b))
                .wrapping_mul(0x100000001b3))
        )
    }
    #[test]
    fn render_updates_include_light_only_changes_and_keep_old_snapshots_immutable() {
        let mut runtime = Runtime::new(189, 1234, false, Arc::new(OracleHooks));
        runtime.provide((0, 0)).unwrap();
        let first = runtime.take_chunk_updates();
        assert_eq!(first.len(), 1);
        assert!(first[0].1.random_tick_sections.get().is_some());
        assert!(first[0].1.scheduled_tick_blocks.get().is_some());
        assert!(runtime.take_chunk_updates().is_empty());
        runtime.backend.dirty_chunks.insert((0, 0));
        let republished = runtime.take_chunk_updates();
        assert!(Arc::ptr_eq(&first[0].1, &republished[0].1),
            "an unchanged requested chunk must reuse the renderer's block allocation");
        assert!(Arc::ptr_eq(first[0].1.light.as_ref().unwrap(),
            republished[0].1.light.as_ref().unwrap()),
            "unchanged lighting must not repack 65536 values");
        let p = IVec3::new(3, 40, 7);
        let old = first[0].1.light.as_ref().unwrap().get(Kind::Block, p);
        let new = if old == 0 { 12 } else { 0 };
        runtime
            .backend
            .light
            .set(&runtime.backend.world, Kind::Block, p, new);
        assert!(runtime.backend.dirty_chunks.is_empty());
        let changed = runtime.take_chunk_updates();
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].0, (0, 0));
        assert_eq!(
            changed[0].1.light.as_ref().unwrap().get(Kind::Block, p),
            new
        );
        assert_eq!(first[0].1.light.as_ref().unwrap().get(Kind::Block, p), old);
        assert!(runtime.take_chunk_updates().is_empty());
        let checkpoint = runtime.checkpoint().unwrap();
        let mut restored = Runtime::resume(checkpoint, Arc::new(OracleHooks)).unwrap();
        assert!(restored.backend.dirty_chunks.is_empty());
        let changed = restored.take_chunk_updates();
        assert_eq!(changed.len(), 1);
        assert_eq!(
            changed[0].1.light.as_ref().unwrap().get(Kind::Block, p),
            new
        );
    }
    #[test]
    fn actual_worldserver_reentrant_reads_population_rng_and_lighting_match() {
        compare_runtime(
            include_str!("../../tests/fixtures/vanilla189-runtime.json"),
            false,
            false,
        );
    }
    #[test]
    fn actual_worldserver_complete_overworld_population_matches() {
        compare_runtime(
            include_str!("../../tests/fixtures/vanilla189-runtime-full.json"),
            true,
            false,
        );
    }
    #[test]
    fn reported_seed_population_matches_original_grass_flowers_snow_and_trees() {
        let fixture = include_str!("../../tests/fixtures/vanilla189-reported-population.json");
        let data: Value = serde_json::from_str(fixture).unwrap();
        let cases = data["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 6, "three reported locations in both request orders");
        for feature in ["logs", "leaves", "tall_grass", "flowers", "snow_layers"] {
            assert!(cases.iter().any(|case| case["surface_counts"][feature].as_u64().unwrap() > 0),
                "the original fixture must exercise {feature}");
        }
        // Compare every block/light byte, load/populate event, RNG checkpoint,
        // generated container and entity, not merely decoration counts.
        compare_runtime(fixture, true, false);
    }
    #[test]
    fn original_structures_and_population_in_large_reordered_regions_match() {
        compare_runtime(
            include_str!("../../tests/fixtures/vanilla189-runtime-extended.json"),
            true,
            false,
        );
    }
    #[test]
    fn populated_structures_and_rng_resume_after_every_large_region_request() {
        compare_runtime(
            include_str!("../../tests/fixtures/vanilla189-runtime-extended.json"),
            true,
            true,
        );
    }
    #[test]
    fn native_json_checkpoint_resume_matches_original_after_every_request() {
        compare_runtime(
            include_str!("../../tests/fixtures/vanilla189-runtime.json"),
            false,
            true,
        );
        compare_runtime(
            include_str!("../../tests/fixtures/vanilla189-runtime-full.json"),
            true,
            true,
        );
    }
    #[test]
    fn parallel_terrain_preparation_preserves_original_population_and_reentrant_random() {
        compare_runtime_mode(include_str!("../../tests/fixtures/vanilla189-runtime-full.json"), true, false, true);
        compare_runtime_mode(include_str!("../../tests/fixtures/vanilla189-reported-population.json"), true, false, true);
    }
    #[test]
    fn bounded_terrain_lookahead_matches_original_population_reentrant_rng_and_resume() {
        compare_runtime_preparation(include_str!("../../tests/fixtures/vanilla189-runtime-full.json"), true, true, true, true);
        compare_runtime_preparation(include_str!("../../tests/fixtures/vanilla189-reported-population.json"), true, false, true, true);
    }
    fn compare_runtime(fixture: &str, full: bool, checkpoint: bool) {
        compare_runtime_mode(fixture, full, checkpoint, false);
    }
    fn compare_runtime_mode(fixture: &str, full: bool, checkpoint: bool, prepare: bool) {
        compare_runtime_preparation(fixture, full, checkpoint, prepare, false);
    }
    fn compare_runtime_preparation(fixture: &str, full: bool, checkpoint: bool, prepare: bool, lookahead: bool) {
        let fixture: Value = serde_json::from_str(fixture).unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let trace = std::env::var("MC189_RUNTIME_TRACE")
                .ok()
                .is_some_and(|v| case["raw_prefix"].as_str() == Some(v.as_str()));
            if std::env::var_os("MC189_RUNTIME_TRACE").is_some() && !trace {
                continue;
            }
            let seed = case["seed"].as_str().unwrap().parse().unwrap();
            let hooks: Arc<dyn Hooks> = if full {
                Arc::new(VanillaPopulation)
            } else {
                Arc::new(OracleHooks)
            };
            let mut runtime = Runtime::new(seed, 1234, full, hooks.clone());
            if prepare && !lookahead {
                let requests = case["operations"].as_array().unwrap().iter().map(|op|
                    (op[0].as_i64().unwrap() as i32, op[1].as_i64().unwrap() as i32)).collect::<Vec<_>>();
                runtime.prepare_terrain(&requests).unwrap();
                assert!(runtime.provider.loaded_order().is_empty(), "preparation must not load/populate chunks");
                assert!(runtime.backend.events.is_empty(), "preparation must not emit provider events");
            }
            let mut checkpoints = Vec::new();
            let operations = case["operations"].as_array().unwrap();
            for (index, op) in operations.iter().enumerate() {
                if lookahead {
                    let requests = operations[index..].iter().take(64).map(|op|
                        (op[0].as_i64().unwrap() as i32, op[1].as_i64().unwrap() as i32)).collect::<Vec<_>>();
                    runtime.prefetch_terrain(&requests).unwrap();
                    // Short batches deliberately permit decorators to consume
                    // a future in-flight result through a nested provider call.
                    runtime.prepare_terrain(&requests[..requests.len().min(3)]).unwrap();
                    assert!(runtime.backend.terrain_jobs.len() + runtime.backend.prepared_terrain.len() <= 64);
                }
                runtime
                    .provide((
                        op[0].as_i64().unwrap() as i32,
                        op[1].as_i64().unwrap() as i32,
                    ))
                    .unwrap();
                checkpoints.push(json!([
                    op[0],
                    op[1],
                    runtime.backend.provider_random.state().to_string(),
                    runtime.backend.world_random.state().to_string()
                ]));
                if trace {
                    eprintln!(
                        "checkpoint {op} provider={} world={}",
                        runtime.backend.provider_random.state(),
                        runtime.backend.world_random.state()
                    );
                }
                if checkpoint {
                    let bytes = serde_json::to_vec(&runtime.checkpoint().unwrap()).unwrap();
                    runtime =
                        Runtime::resume(serde_json::from_slice(&bytes).unwrap(), hooks.clone())
                            .unwrap();
                }
            }
            if !case["checkpoints"].is_null() {
                assert_eq!(
                    json!(checkpoints),
                    case["checkpoints"],
                    "intermediate RNG {}",
                    case["raw_prefix"]
                );
            }
            let actual = runtime
                .backend
                .events
                .iter()
                .filter(|(name, _)| name != "on_load")
                .map(|(name, p)| json!([name, p.0, p.1]))
                .collect::<Vec<_>>();
            assert_eq!(json!(actual), case["events"], "runtime events seed {seed}");
            let access = runtime.access();
            let p = IVec3::new(1600, 70, 1600);
            let queries = json!([
                access.height(1600, 1600),
                access.light(p, true) as i32,
                access.light(p, false) as i32,
                access.biome(p) as i32
            ]);
            assert_eq!(queries, case["queries"]);
            let actual = runtime
                .provider
                .loaded_order()
                .iter()
                .map(|&pos| {
                    let chunk = &runtime.backend.world.chunks[&pos];
                    let record = runtime.provider.record(pos).unwrap();
                    let blocks = hash(chunk.blocks.iter().flat_map(|s| s.to_le_bytes()));
                    let expected_blocks = case["chunks"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|r| r[0] == pos.0 && r[1] == pos.1)
                        .and_then(|r| r[4].as_str());
                    if full && expected_blocks != Some(blocks.as_str()) {
                        let reverse = if case["operations"][0] == json!([0, 0]) {
                            0
                        } else {
                            1
                        };
                        let fallback = format!("{seed}_{reverse}");
                        let prefix = case["raw_prefix"].as_str().unwrap_or(&fallback);
                        let path = format!(
                            ".local/oracles/runtime/world/blocks/{prefix}_{}_{}.bin",
                            pos.0, pos.1
                        );
                        if let Ok(bytes) = std::fs::read(path) {
                            let mut pairs = BTreeMap::<(u16, u16), usize>::new();
                            let mut first = Vec::new();
                            for (i, (actual, pair)) in chunk
                                .blocks
                                .iter()
                                .zip(bytes.as_chunks::<2>().0)
                                .enumerate()
                            {
                                let expected = u16::from_le_bytes([pair[0], pair[1]]);
                                if *actual != expected {
                                    *pairs.entry((*actual, expected)).or_default() += 1;
                                    if first.len() < 12 {
                                        first.push((
                                            i % 16,
                                            i / 256,
                                            (i / 16) % 16,
                                            *actual,
                                            expected,
                                        ));
                                    }
                                }
                            }
                            if !pairs.is_empty() {
                                eprintln!("chunk {pos:?} differences {pairs:?}; first {first:?}");
                            }
                        }
                    }
                    let light = hash((0..65536).flat_map(|i| {
                        let p =
                            IVec3::new(pos.0 * 16 + i % 16, i / 256, pos.1 * 16 + (i / 16) % 16);
                        [
                            runtime.backend.light.get(Kind::Sky, p),
                            runtime.backend.light.get(Kind::Block, p),
                        ]
                    }));
                    json!([
                        pos.0,
                        pos.1,
                        record.terrain_populated,
                        record.light_populated,
                        blocks,
                        light
                    ])
                })
                .collect::<Vec<_>>();
            if json!(actual) != case["chunks"] {
                eprintln!(
                    "RNG for {}: provider={} expected={}, world={} expected={}",
                    case["raw_prefix"],
                    runtime.backend.provider_random.clone().next_long(),
                    case["provider_next_long"],
                    runtime.backend.world_random.clone().next_long(),
                    case["world_next_long"]
                );
            }
            assert_eq!(
                json!(actual),
                case["chunks"],
                "runtime chunk/light arrays seed {seed}"
            );
            assert_eq!(
                runtime.backend.provider_random.next_long().to_string(),
                case["provider_next_long"].as_str().unwrap(),
                "provider RNG {}",
                case["raw_prefix"]
            );
            assert_eq!(
                runtime.backend.world_random.next_long().to_string(),
                case["world_next_long"].as_str().unwrap(),
                "world RNG {}",
                case["raw_prefix"]
            );
            if let Some(expected) = case.get("villagers") {
                let mut actual = runtime
                    .backend
                    .structure_entities
                    .villagers
                    .iter()
                    .map(|v| {
                        [
                            v.position[0],
                            v.position[1],
                            v.position[2],
                            i32::from(v.profession.unwrap()),
                        ]
                    })
                    .collect::<Vec<_>>();
                let mut expected: Vec<[i32; 4]> = serde_json::from_value(expected.clone()).unwrap();
                actual.sort_unstable();
                expected.sort_unstable();
                assert_eq!(actual, expected, "village entities {}", case["raw_prefix"]);
            }
            if let Some(expected) = case.get("containers") {
                let actual = runtime
                    .backend
                    .live_containers
                    .iter()
                    .map(|(p, c)| {
                        (
                            *p,
                            (
                                c.block,
                                c.slots
                                    .iter()
                                    .map(|s| s.as_ref().map(inventories::oracle_loot))
                                    .collect::<Vec<_>>(),
                            ),
                        )
                    })
                    .collect::<BTreeMap<_, _>>();
                let expected =
                    serde_json::from_value::<Vec<([i32; 3], u16, Vec<Option<LootStack>>)>>(
                        expected.clone(),
                    )
                    .unwrap()
                    .into_iter()
                    .map(|(p, id, slots)| (p, (id, slots)))
                    .collect::<BTreeMap<_, _>>();
                assert_eq!(
                    actual, expected,
                    "generated inventories {}",
                    case["raw_prefix"]
                );
            }
            if let Some(expected) = case.get("spawners") {
                let actual = runtime.backend.live_spawners.clone();
                let expected = serde_json::from_value::<Vec<([i32; 3], String)>>(expected.clone())
                    .unwrap()
                    .into_iter()
                    .collect::<BTreeMap<_, _>>();
                assert_eq!(
                    actual, expected,
                    "generated spawners {}",
                    case["raw_prefix"]
                );
            }
        }
    }

    struct OverhangHooks;
    impl Hooks for OverhangHooks {
        fn populate(
            &self,
            access: &mut Access<'_>,
            pos: ChunkPos,
            _random: &mut JavaRandom,
        ) -> Result<()> {
            if pos == (0, 0) {
                for x in 13..=15 {
                    for z in 13..=15 {
                        access.set_block(IVec3::new(x, 150, z), 1 << 4)?;
                    }
                }
            }
            Ok(())
        }
    }

    #[test]
    fn settle_populated_lighting_relights_enclosed_overhangs_without_rng_and_is_batch_invariant() {
        let mut eager = Runtime::new(189, 1234, false, Arc::new(OverhangHooks));
        let mut batched = Runtime::new(189, 1234, false, Arc::new(OverhangHooks));
        let under = IVec3::new(14, 149, 14);
        for pos in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            eager.provide(pos).unwrap();
            batched.provide(pos).unwrap();
            batched.settle_populated_lighting().unwrap();
        }
        assert_eq!(eager.backend.light.get(Kind::Sky, under), 0);
        assert_eq!(batched.backend.light.get(Kind::Sky, under), 0);
        drop(batched.take_chunk_updates());

        for x in -2..=2 {
            for z in -2..=2 {
                eager.provide((x, z)).unwrap();
                batched.provide((x, z)).unwrap();
                batched.settle_populated_lighting().unwrap();
            }
        }
        let rng_before = (
            eager.backend.provider_random.state(),
            eager.backend.world_random.state(),
            super::super::random::independent_seed_calls_for_thread(),
        );
        let access_before = (
            eager.runtime_timings().access_reads,
            eager.runtime_timings().access_writes,
        );
        eager.settle_populated_lighting().unwrap();
        assert_eq!(
            rng_before,
            (
                eager.backend.provider_random.state(),
                eager.backend.world_random.state(),
                super::super::random::independent_seed_calls_for_thread(),
            )
        );
        assert_eq!(
            access_before,
            (
                eager.runtime_timings().access_reads,
                eager.runtime_timings().access_writes,
            )
        );
        assert_eq!(eager.backend.light.get(Kind::Sky, under), 13);
        assert_eq!(batched.backend.light.get(Kind::Sky, under), 13);
        assert!(eager.provider.record((0, 0)).unwrap().light_populated);
        let published = batched.take_chunk_updates();
        let center = published.iter().find(|(p, _)| *p == (0, 0)).unwrap();
        assert_eq!(
            center.1.light.as_ref().unwrap().get(Kind::Sky, under),
            13
        );
        drop(eager.take_chunk_updates());
        use sha1::Digest;
        let mut eager_cp = eager.checkpoint().unwrap();
        let mut batched_cp = batched.checkpoint().unwrap();
        eager_cp.edits.sort_unstable();
        batched_cp.edits.sort_unstable();
        assert_eq!(
            sha1::Sha1::digest(serde_json::to_vec(&eager_cp).unwrap()),
            sha1::Sha1::digest(serde_json::to_vec(&batched_cp).unwrap())
        );
        eager.settle_populated_lighting().unwrap();
        assert!(eager.take_chunk_updates().is_empty());
    }
}
