//! Local 20 Hz scheduled block updates based on 1.8.9 BlockDynamicLiquid,
//! BlockStaticLiquid, BlockFalling and EntityFallingBlock. Queue and falling
//! entities are serialized with the player's local simulation. Unloaded chunks
//! are deferred, never treated as air. Fire spread is separate.
use crate::{
    vanilla::random::JavaRandom,
    world::{ChunkPos, World, HEIGHT},
};
#[path = "block_ticks_falling.rs"]
mod falling_entities;
#[path = "block_ticks_fire.rs"]
mod fire;
use crate::vanilla::runtime::FireEnvironment;
pub use crate::vanilla::falling::Falling as FallingBlock;
pub use falling_entities::FallingImpact;
use glam::{IVec3, Vec3};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, Mutex, MutexGuard},
};

/// Eventful access used by synchronous liquid generation. Runtime adapters can
/// lazily provide chunks and run lighting at the original callback boundaries.
pub trait BlockAccess {
    fn get(&self, p: IVec3) -> u16;
    fn chunk_loaded(&self, pos: ChunkPos) -> bool;
    fn accessible(&self, p: IVec3) -> bool {
        (0..HEIGHT).contains(&p.y) && self.chunk_loaded((p.x.div_euclid(16), p.z.div_euclid(16)))
    }
    /// Chunk precipitation-cache invalidation precedes even same-state writes.
    fn before_set_attempt(&mut self, _p: IVec3) {}
    /// Raw replacement and Chunk lighting, before Block.onBlockAdded.
    fn set_before_added(&mut self, p: IVec3, state: u16) -> bool;
    /// World lighting, after Block.onBlockAdded, before neighbor notifications.
    fn after_added(&mut self, _p: IVec3, _old: u16, _requested: u16) {}
    /// None uses the persisted scheduler RNG; runtime adapters return a draw
    /// from the shared World.rand, including reentrant provider reseeds.
    fn world_random_int(&mut self, _bound: i32) -> Option<i32> {
        None
    }
    /// Return true when the native drop callback has emitted its actual results.
    fn drop_block_item(&mut self, _p: IVec3, _state: u16) -> bool {
        false
    }
    /// Remaining Block.onNeighborBlockChange implementations (liquids/falling
    /// blocks are handled by this engine). Source is the notifying block ID.
    fn neighbor_changed(&mut self, _p: IVec3, _state: u16, _source: u16) {}
    fn note_powered(&self, _pos: IVec3) -> Option<bool> {
        None
    }
    /// Native entity managers can own the original deferred falling entity.
    fn spawn_falling(&mut self, _p: IVec3, _state: u16) -> bool {
        false
    }
    /// Remaining original scheduled block callbacks. False reports an
    /// unsupported tick to the native host instead of silently consuming it.
    fn scheduled_tick(&mut self, _p: IVec3, _state: u16) -> bool {
        false
    }
}
impl BlockAccess for World {
    fn get(&self, p: IVec3) -> u16 {
        World::get(self, p)
    }
    fn chunk_loaded(&self, pos: ChunkPos) -> bool {
        self.chunks.contains_key(&pos)
    }
    fn note_powered(&self, pos: IVec3) -> Option<bool> {
        use crate::vanilla::{item_nbt, notes};
        Some(notes::powered(self, pos, |p| {
            item_nbt::number(self.tile(p).and_then(item_nbt::compound).and_then(|tile| tile.get("OutputSignal")))
        }))
    }
    fn set_before_added(&mut self, p: IVec3, state: u16) -> bool {
        self.set(p, state)
    }
}

const HORIZONTAL: [IVec3; 4] = [IVec3::NEG_Z, IVec3::X, IVec3::Z, IVec3::NEG_X];
const NEIGHBORS: [IVec3; 6] = [
    IVec3::NEG_X,
    IVec3::X,
    IVec3::NEG_Y,
    IVec3::Y,
    IVec3::NEG_Z,
    IVec3::Z,
];

#[derive(Default)]
pub struct TickResult {
    pub primed_tnt: Vec<IVec3>,
    pub note_updates: Vec<(IVec3, bool)>,
    pub changed: Vec<IVec3>,
    /// Failed falling-block placements; each entry drops one block item.
    pub drops: Vec<(Vec3, u16)>,
    /// Water-displaced plants/circuits; gameplay owns their normal loot rules.
    pub washed: Vec<(IVec3, u16)>,
    pub impacts: Vec<FallingImpact>,
    pub sounds: Vec<(&'static str, f32, f32)>,
    pub world_sounds: Vec<(&'static str, glam::DVec3, f32, f32)>,
    pub mix_smoke: Vec<IVec3>,
    pub detector_rails: Vec<IVec3>,
}
impl TickResult {
    /// Recycle callback-local allocations without carrying effects into the
    /// next write. The caller must publish any effects it owns before this.
    pub(crate) fn clear(&mut self) {
        self.primed_tnt.clear();
        self.note_updates.clear();
        self.changed.clear();
        self.drops.clear();
        self.washed.clear();
        self.impacts.clear();
        self.sounds.clear();
        self.world_sounds.clear();
        self.mix_smoke.clear();
        self.detector_rails.clear();
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct Scheduled {
    position: [i32; 3],
    kind: u16,
    #[serde(default)]
    priority: i32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BlockTicks {
    #[serde(default, deserialize_with = "falling_entities::read_saved")]
    pub falling: Vec<FallingBlock>,
    #[serde(flatten, with = "scheduler_serde")]
    scheduler: Arc<Mutex<Scheduler>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct Scheduler {
    clock: u64,
    nether: bool,
    do_entity_drops: bool,
    #[serde(with = "queue_times")]
    queue: BTreeMap<u64, Vec<Scheduled>>,
    notifications: Vec<[i32; 3]>,
    neighbor_events: Vec<[i32; 3]>,
    random_seed: u64,
    #[serde(skip)]
    known_chunks: HashSet<ChunkPos>,
    #[serde(skip)]
    pending: HashSet<([i32; 3], u16)>,
    #[serde(skip)]
    indexed: bool,
    #[serde(skip)]
    immediate: bool,
    #[serde(skip)]
    fall_instantly: bool,
    #[serde(skip)]
    fire_environment: FireEnvironment,
    #[serde(skip)]
    end_dimension: bool,
}
impl Default for Scheduler {
    fn default() -> Self {
        Self {
            clock: 0,
            nether: false,
            do_entity_drops: true,
            queue: BTreeMap::new(),
            notifications: Vec::new(),
            neighbor_events: Vec::new(),
            random_seed: 0x5DEECE66D,
            known_chunks: HashSet::new(),
            pending: HashSet::new(),
            indexed: false,
            immediate: false,
            fall_instantly: false,
            fire_environment: FireEnvironment::default(),
            end_dimension: false,
        }
    }
}
impl Default for BlockTicks {
    fn default() -> Self {
        Self {
            falling: Vec::new(),
            scheduler: Arc::new(Mutex::new(Scheduler::default())),
        }
    }
}
impl Clone for BlockTicks {
    fn clone(&self) -> Self {
        Self {
            falling: self.falling.clone(),
            scheduler: Arc::new(Mutex::new(self.state().clone())),
        }
    }
}
impl BlockTicks {
    /// An isolated presentation scheduler, never an alias to the ticking owner.
    /// Falling entities are drawn and picked; the scheduler's work is not.
    pub(crate) fn presentation_clone(&self) -> Self {
        let state = self.state();
        Self {
            falling: self.falling.clone(),
            scheduler: Arc::new(Mutex::new(Scheduler {
                clock: state.clock,
                nether: state.nether,
                do_entity_drops: state.do_entity_drops,
                queue: BTreeMap::new(),
                notifications: Vec::new(),
                neighbor_events: Vec::new(),
                random_seed: state.random_seed,
                known_chunks: HashSet::new(),
                pending: HashSet::new(),
                indexed: state.indexed,
                immediate: state.immediate,
                fall_instantly: state.fall_instantly,
                fire_environment: state.fire_environment.clone(),
                end_dimension: state.end_dimension,
            })),
        }
    }
}

#[cfg(test)]
#[path = "block_ticks_presentation_tests.rs"]
mod presentation_tests;

mod scheduler_serde {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        state: &Arc<Mutex<Scheduler>>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        state.lock().unwrap_or_else(|e| e.into_inner()).serialize(s)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> Result<Arc<Mutex<Scheduler>>, D::Error> {
        Ok(Arc::new(Mutex::new(Scheduler::deserialize(d)?)))
    }
}
// Flattened serde values do not apply serde_json's numeric map-key adapter.
// Explicit string keys retain the existing on-disk clock -> entries format.
mod queue_times {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        map: &BTreeMap<u64, Vec<Scheduled>>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        map.iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect::<BTreeMap<_, _>>()
            .serialize(s)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> Result<BTreeMap<u64, Vec<Scheduled>>, D::Error> {
        BTreeMap::<String, Vec<Scheduled>>::deserialize(d)?
            .into_iter()
            .map(|(k, v)| {
                k.parse::<u64>()
                    .map(|k| (k, v))
                    .map_err(serde::de::Error::custom)
            })
            .collect()
    }
}
fn kind(state: u16) -> u16 {
    match state >> 4 {
        8 | 9 => 8,
        10 | 11 => 10,
        12 => 12,
        13 => 13,
        122 => 122,
        145 => 145,
        _ => 0,
    }
}
pub fn is_liquid(state: u16) -> bool {
    matches!(state >> 4, 8..=11)
}
pub fn same_liquid(a: u16, b: u16) -> bool {
    let a = kind(a);
    a != 0 && a <= 10 && a == kind(b)
}
fn loaded(world: &impl BlockAccess, p: IVec3) -> bool {
    (0..HEIGHT).contains(&p.y) && world.chunk_loaded((p.x.div_euclid(16), p.z.div_euclid(16)))
}
fn level(world: &impl BlockAccess, p: IVec3, fluid: u16) -> i32 {
    let state = world.get(p);
    if kind(state) == fluid {
        (state & 15) as i32
    } else {
        -1
    }
}
pub fn material_blocks_movement(state: u16) -> bool {
    crate::vanilla::material::blocks_movement(state)
}
pub fn material_is_solid(state: u16) -> bool {
    crate::vanilla::material::is_solid(state)
}
fn flow_blocked(world: &impl BlockAccess, p: IVec3) -> bool {
    if !world.accessible(p) {
        return true;
    }
    let state = world.get(p);
    matches!(state >> 4, 64 | 71 | 193..=197 | 63 | 65 | 83)
        || crate::vanilla::material::is_portal(state)
        || material_blocks_movement(state)
}
fn can_flow(world: &impl BlockAccess, p: IVec3, fluid: u16) -> bool {
    let other = kind(world.get(p));
    world.accessible(p) && other != fluid && other != 10 && !flow_blocked(world, p)
}
fn can_fall(state: u16) -> bool {
    matches!(state >> 4, 0 | 8..=11 | 51)
}
impl BlockTicks {
    fn state(&self) -> MutexGuard<'_, Scheduler> {
        self.scheduler.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// Callback-local scheduler handle. Locks never span a World callback.
    /// Falling entities remain owned by the single simulation/entity manager.
    pub fn share_scheduler(&self) -> Self {
        Self {
            falling: Vec::new(),
            scheduler: self.scheduler.clone(),
        }
    }
    pub fn clock(&self) -> u64 {
        self.state().clock
    }
    pub fn set_clock(&mut self, time: u64) {
        self.state().clock = time;
    }
    pub fn set_do_entity_drops(&mut self, enabled: bool) {
        self.state().do_entity_drops = enabled;
    }
    pub fn set_nether(&mut self, nether: bool) {
        self.state().nether = nether;
    }
    pub fn set_fall_instantly(&mut self, value: bool) {
        self.state().fall_instantly = value;
    }
    fn nether(&self) -> bool {
        self.state().nether
    }
    pub(crate) fn immediate(&self) -> bool {
        self.state().immediate
    }

    /// World generation must pass the same world Random used by other world
    /// callbacks. The first forced update uses the population Random; recursive
    /// schedules use the world's Random, matching WorldServer.updateBlockTick.
    pub fn generate_spring(
        &mut self,
        world: &mut World,
        p: IVec3,
        fluid: u16,
        population_random: &mut JavaRandom,
        world_random: &mut JavaRandom,
    ) -> (bool, TickResult) {
        self.set_world_random(world_random);
        let result = self.generate_spring_access(world, p, fluid, population_random);
        *world_random = self.world_random();
        result
    }
    pub fn generate_spring_access(
        &mut self,
        world: &mut impl BlockAccess,
        p: IVec3,
        fluid: u16,
        population_random: &mut JavaRandom,
    ) -> (bool, TickResult) {
        assert!(matches!(fluid, 8 | 10));
        let mut out = TickResult::default();
        if world.get(p + IVec3::Y) >> 4 != 1
            || world.get(p - IVec3::Y) >> 4 != 1
            || !matches!(world.get(p) >> 4, 0 | 1)
        {
            return (false, out);
        }
        let solids = HORIZONTAL
            .iter()
            .filter(|&&d| world.get(p + d) >> 4 == 1)
            .count();
        let air = HORIZONTAL
            .iter()
            .filter(|&&d| world.get(p + d) == 0)
            .count();
        if solids == 3 && air == 1 {
            self.write(world, p, fluid << 4, false, &mut out);
            let mut forced = self.force_liquid_update(world, p, fluid, population_random);
            out.note_updates.append(&mut forced.note_updates);
            out.changed.append(&mut forced.changed);
            out.drops.append(&mut forced.drops);
            out.washed.append(&mut forced.washed);
            out.sounds.append(&mut forced.sounds);
            out.world_sounds.append(&mut forced.world_sounds);
            out.mix_smoke.append(&mut forced.mix_smoke);
        }
        (true, out)
    }
    /// World.forceBlockUpdateTick for an already placed flowing water/lava block.
    pub fn force_liquid_update(
        &mut self,
        world: &mut impl BlockAccess,
        p: IVec3,
        fluid: u16,
        random: &mut JavaRandom,
    ) -> TickResult {
        assert!(matches!(fluid, 8 | 10));
        let mut out = TickResult::default();
        self.state().immediate = true;
        self.liquid_tick(world, p, fluid, &mut out, Some(random));
        self.state().immediate = false;
        out
    }
    pub fn world_random(&self) -> JavaRandom {
        JavaRandom::from_state(self.state().random_seed)
    }
    pub fn set_world_random(&mut self, random: &JavaRandom) {
        self.state().random_seed = random.state();
    }
    fn request_update(
        &mut self,
        world: &mut impl BlockAccess,
        p: IVec3,
        k: u16,
        delay: u64,
        out: &mut TickResult,
    ) {
        if self.immediate() && matches!(k, 8 | 10 | 12 | 13 | 122 | 145) {
            // Original immediate updates require the complete +/-8 block
            // neighborhood. A failed availability check does not queue a tick.
            let available = (p.x - 8).div_euclid(16)..=(p.x + 8).div_euclid(16);
            if available.clone().all(|x| {
                ((p.z - 8).div_euclid(16)..=(p.z + 8).div_euclid(16))
                    .all(|z| world.chunk_loaded((x, z)))
            }) && (0..HEIGHT).contains(&p.y)
                && world.get(p) >> 4 == k
            {
                if matches!(k, 8 | 10) {
                    self.liquid_tick(world, p, k, out, None);
                } else {
                    self.fall_update(world, p, out);
                }
            }
        } else {
            self.schedule(p, k, delay);
        }
    }
    fn fall_update(&mut self, world: &mut impl BlockAccess, p: IVec3, out: &mut TickResult) {
        if p.y < 0 || !can_fall(world.get(p - IVec3::Y)) {
            return;
        }
        let state = world.get(p);
        let area = ((p.x - 32).div_euclid(16)..=(p.x + 32).div_euclid(16)).all(|x| {
            ((p.z - 32).div_euclid(16)..=(p.z + 32).div_euclid(16))
                .all(|z| world.chunk_loaded((x, z)))
        });
        let instantly = self.state().fall_instantly;
        if instantly || !area {
            self.write(world, p, 0, true, out);
            let mut below = p - IVec3::Y;
            while can_fall(world.get(below)) && below.y > 0 {
                below -= IVec3::Y;
            }
            // Original instant fallback uses defaultState, losing red-sand meta,
            // and discards the block when the supporting block is at Y=0.
            if below.y > 0 {
                let default_state = (state >> 4) << 4 | if state >> 4 == 145 { 2 } else { 0 };
                self.write(world, below + IVec3::Y, default_state, true, out);
            }
        } else if !world.spawn_falling(p, state) {
            self.falling.push(falling_entities::spawn(p, state));
        }
    }
    fn draw_world(&mut self, world: &mut impl BlockAccess, bound: i32) -> i32 {
        if let Some(value) = world.world_random_int(bound) {
            return value;
        }
        let mut random = self.world_random();
        let value = random.next_int(bound);
        self.set_world_random(&random);
        value
    }
    pub(crate) fn fizz(&mut self, world: &mut impl BlockAccess) -> (&'static str, f32, f32) {
        // Two nextFloat draws for pitch. Math.random smoke positions are separate.
        let a = self.draw_world(world, 1 << 24) as f32 / 16777216.;
        let b = self.draw_world(world, 1 << 24) as f32 / 16777216.;
        ("random.fizz", 0.5, 2.6 + (a - b) * 0.8)
    }
    pub fn set_block(
        &mut self,
        world: &mut impl BlockAccess,
        p: IVec3,
        state: u16,
        notify: bool,
    ) -> TickResult {
        let mut out = TickResult::default();
        self.write(world, p, state, notify, &mut out);
        out
    }
    /// Same callback sequence as set_block, using caller-owned empty scratch.
    /// The buffer remains local while recursive world callbacks run.
    pub(crate) fn set_block_into(
        &mut self,
        world: &mut impl BlockAccess,
        p: IVec3,
        state: u16,
        notify: bool,
        out: &mut TickResult,
    ) {
        self.write(world, p, state, notify, out);
    }
    /// Internal native path after before_set_attempt and get already ran.
    /// The caller must not invoke a world callback or mutate the world between
    /// that read and this call. Changed writes retain every ordinary callback.
    pub(crate) fn set_block_prechecked_into(
        &mut self,
        world: &mut impl BlockAccess,
        p: IVec3,
        old: u16,
        state: u16,
        notify: bool,
        out: &mut TickResult,
    ) {
        self.write_after_attempt(world, p, old, state, notify, out);
    }
    pub fn notify_neighbors(
        &mut self,
        world: &mut impl BlockAccess,
        p: IVec3,
        source: u16,
    ) -> TickResult {
        let mut out = TickResult::default();
        self.neighbors_changed(world, p, source, &mut out);
        out
    }
    fn index(&mut self) {
        let mut state = self.state();
        if !state.indexed {
            state.pending = state
                .queue
                .values()
                .flatten()
                .map(|s| (s.position, s.kind))
                .collect();
            state.indexed = true;
        }
    }
    fn delay(&self, fluid: u16) -> u64 {
        match fluid {
            8 => 5,
            10 => {
                if self.nether() {
                    10
                } else {
                    30
                }
            }
            _ => 2,
        }
    }
    fn schedule(&mut self, p: IVec3, kind: u16, delay: u64) {
        self.schedule_block_update(p, kind, delay, 0);
    }
    /// WorldServer.scheduleBlockUpdate (also used to restore saved TileTicks).
    /// This variant deliberately does not test whether the chunk is loaded.
    pub fn schedule_block_update(&mut self, p: IVec3, kind: u16, delay: u64, priority: i32) {
        if kind == 0 {
            return;
        }
        self.index();
        let mut state = self.state();
        if state.pending.insert((p.to_array(), kind)) {
            let at = state.clock.wrapping_add(delay);
            state.queue.entry(at).or_default().push(Scheduled {
                position: p.to_array(),
                kind,
                priority,
            });
        }
    }
    /// Call after any externally made block edit, including placing/removing a
    /// bucket source or supporting block. No world writes happen in this method.
    pub fn notify(&mut self, world: &World, p: IVec3) {
        let id = world.get(p) >> 4;
        if id == 51 { self.discover_fire(p); }
        if matches!(id, 8 | 10 | 12 | 13 | 122 | 145) {
            self.schedule(p, id, self.delay(id));
        }
        self.state().neighbor_events.push(p.to_array());
        if kind(world.get(p)) == 10 {
            self.state().notifications.push(p.to_array());
        }
    }
    pub(crate) fn pending_neighbor_positions(&self) -> Vec<IVec3> {
        self.state().neighbor_events.iter().copied().map(IVec3::from_array).collect()
    }
    fn neighbors_changed(
        &mut self,
        world: &mut impl BlockAccess,
        p: IVec3,
        source: u16,
        out: &mut TickResult,
    ) {
        for d in NEIGHBORS {
            let at = p + d;
            let state = world.get(at);
            if state >> 4 == 25 {
                if let Some(powered) = world.note_powered(at) {
                    out.note_updates.push((at, powered));
                }
            }
            let k = kind(state);
            if k == 10 && self.mixing(world, at, out) {
                continue;
            }
            if matches!(state >> 4, 9 | 11) {
                // BlockStaticLiquid converts immediately, with flag 2 (no
                // recursive neighbor notification), then schedules its tick.
                self.write(world, at, (k << 4) | (state & 15), false, out);
                self.request_update(world, at, k, self.delay(k), out);
            } else if matches!(k, 12 | 13 | 122 | 145) {
                self.request_update(world, at, k, self.delay(k), out);
            } else {
                world.neighbor_changed(at, state, source);
            }
        }
    }
    fn discover_chunks(&mut self, world: &World) {
        // One short lock for membership bookkeeping; never hold it across
        // scheduling or fire callbacks, which access the scheduler themselves.
        let mut fresh: Vec<_> = {
            let mut scheduler = self.state();
            scheduler.known_chunks.retain(|p| world.chunks.contains_key(p));
            let fresh = world.chunks.keys()
                .filter(|p| !scheduler.known_chunks.contains(p)).copied().collect::<Vec<_>>();
            scheduler.known_chunks.extend(fresh.iter().copied());
            fresh
        };
        fresh.sort_unstable();
        for chunk_pos in fresh {
            let chunk = &world.chunks[&chunk_pos];
            let limit = chunk.top.clamp(0, HEIGHT) as usize * 256;
            // Prepared on the native publication worker, then maintained by
            // Chunk::set. Sorted linear indices preserve the old Y/Z/X scan.
            for &(index, state) in chunk.scheduled_tick_candidates().iter()
                .take_while(|(index, _)| usize::from(*index) < limit) {
                let i = i32::from(index);
                let p = IVec3::new(chunk_pos.0 * 16 + (i & 15), i >> 8,
                    chunk_pos.1 * 16 + ((i >> 4) & 15));
                if state >> 4 == 51 {
                    self.discover_fire(p);
                    continue;
                }
                let k = kind(state);
                let active = match k {
                    // Chunk.onChunkLoad does not call BlockFalling.onBlockAdded.
                    // Naturally suspended sand/gravel/anvils wait for an actual
                    // placement, neighbor update or restored scheduled tick.
                    12 | 13 | 122 | 145 => false,
                    _ => state >> 4 == k,
                };
                if active {
                    self.schedule(p, k, self.delay(k));
                }
            }
        }
    }
    fn write(
        &mut self,
        world: &mut impl BlockAccess,
        p: IVec3,
        state: u16,
        notify: bool,
        out: &mut TickResult,
    ) {
        world.before_set_attempt(p);
        let old = world.get(p);
        self.write_after_attempt(world, p, old, state, notify, out);
    }
    fn write_after_attempt(
        &mut self,
        world: &mut impl BlockAccess,
        p: IVec3,
        old: u16,
        state: u16,
        notify: bool,
        out: &mut TickResult,
    ) {
        if old == state {
            return;
        }
        let old_id = old >> 4;
        if world.set_before_added(p, state) {
            out.changed.push(p);
            if old_id != state >> 4 {
                let id = state >> 4;
                if (!matches!(id, 10 | 11) || !self.mixing(world, p, out))
                    && matches!(id, 8 | 10 | 12 | 13 | 122 | 145)
                {
                    self.request_update(world, p, id, self.delay(id), out);
                }
            }
            world.after_added(p, old, state);
            if notify {
                self.neighbors_changed(world, p, old_id, out);
            }
        }
    }
    fn mixing(&mut self, world: &mut impl BlockAccess, p: IVec3, out: &mut TickResult) -> bool {
        let state = world.get(p);
        if kind(state) != 10
            || !NEIGHBORS
                .iter()
                .any(|&d| d != IVec3::NEG_Y && kind(world.get(p + d)) == 8)
        {
            return false;
        }
        let replacement = match state & 15 {
            0 => 49,
            1..=4 => 4,
            _ => return false,
        };
        self.write(world, p, replacement << 4, true, out);
        let (clip, vol, pitch) = self.fizz(world);
        out.world_sounds
            .push((clip, p.as_dvec3() + glam::DVec3::splat(0.5), vol, pitch));
        out.mix_smoke.push(p);
        true
    }
    pub fn tick(&mut self, world: &mut World) -> TickResult {
        self.index();
        let mut out = TickResult::default();
        self.discover_chunks(world);
        // A newly added lava block checks itself before neighboring stationary
        // liquids wake. Loading a chunk by itself fires no onBlockAdded calls.
        let notifications = std::mem::take(&mut self.state().notifications);
        for p in notifications {
            self.mixing(world, IVec3::from_array(p), &mut out);
        }
        let neighbor_events = std::mem::take(&mut self.state().neighbor_events);
        for p in neighbor_events {
            let source = world.get(IVec3::from_array(p)) >> 4;
            self.neighbors_changed(world, IVec3::from_array(p), source, &mut out);
            for d in NEIGHBORS {
                let at = IVec3::from_array(p) + d;
                let state = world.get(at);
                if matches!(state >> 4, 51 | 90) { self.fire_neighbor(world, at, state, &mut out); }
            }
        }
        // Extract due work under a short queue lock. Reentrant world reads can
        // enqueue more work after the guard is dropped.
        self.set_clock(self.clock().wrapping_add(1));
        let due = self.take_due();
        for scheduled in due {
            let p = IVec3::from_array(scheduled.position);
            if !loaded(world, p) {
                self.schedule(p, scheduled.kind, 20);
                continue;
            }
            if world.get(p) >> 4 != scheduled.kind {
                continue;
            }
            if scheduled.kind == 51 {
                let mut random = self.world_random();
                self.update_local_fire(world, p, world.get(p), &mut random, &mut out);
                self.set_world_random(&random);
            } else if matches!(scheduled.kind, 12 | 13 | 122 | 145) {
                self.fall_update(world, p, &mut out);
            } else if scheduled.kind == 28 {
                out.detector_rails.push(p);
            } else {
                self.liquid_tick(world, p, scheduled.kind, &mut out, None);
            }
        }
        falling_entities::tick(self, world, &mut out);
        out.changed.sort_unstable_by_key(|p| (p.x, p.y, p.z));
        out.changed.dedup();
        out
    }
    fn take_due(&mut self) -> Vec<Scheduled> {
        self.index();
        let mut state = self.state();
        let mut budget = 1000;
        let mut due = Vec::new();
        while budget > 0
            && state
                .queue
                .first_key_value()
                .is_some_and(|(&time, _)| time <= state.clock)
        {
            let (time, mut batch) = state.queue.pop_first().unwrap();
            // Stable sort retains NextTickListEntry's insertion serial for ties.
            batch.sort_by_key(|entry| entry.priority);
            let count = batch.len().min(budget);
            let rest = batch.split_off(count);
            if !rest.is_empty() {
                state.queue.insert(time, rest);
            }
            budget -= count;
            for scheduled in batch {
                state.pending.remove(&(scheduled.position, scheduled.kind));
                due.push(scheduled);
            }
        }
        due
    }
    /// One native world's scheduled-block phase. Entity movement and random
    /// block ticks belong to their own phases; no legacy chunk scan is run.
    pub fn tick_access(&mut self, world: &mut impl BlockAccess) -> anyhow::Result<TickResult> {
        self.set_clock(self.clock().wrapping_add(1));
        let mut out = TickResult::default();
        for scheduled in self.take_due() {
            let p = IVec3::from_array(scheduled.position);
            // Original retry uses scheduleUpdate(0), whose loaded-area guard
            // rejects this same missing chunk. Unload persistence is separate.
            if !loaded(world, p) {
                continue;
            }
            let state = world.get(p);
            let id = state >> 4;
            let associated = id == scheduled.kind
                || matches!(
                    (id, scheduled.kind),
                    (75, 76) | (76, 75) | (93, 94) | (94, 93) | (149, 150) | (150, 149)
                );
            if id == 0 || !associated {
                continue;
            }
            match id {
                8 | 10 => self.liquid_tick(world, p, id, &mut out, None),
                12 | 13 | 122 | 145 => {
                    if !can_fall(world.get(p - IVec3::Y)) {
                        continue;
                    }
                    let available =
                        ((p.x - 32).div_euclid(16)..=(p.x + 32).div_euclid(16)).all(|x| {
                            ((p.z - 32).div_euclid(16)..=(p.z + 32).div_euclid(16))
                                .all(|z| world.chunk_loaded((x, z)))
                        });
                    let instantly = self.state().fall_instantly;
                    if instantly || !available {
                        self.fall_update(world, p, &mut out);
                    } else {
                        anyhow::ensure!(
                            world.spawn_falling(p, state),
                            "Native falling entity manager is required at {p:?}"
                        );
                    }
                }
                _ => anyhow::ensure!(
                    world.scheduled_tick(p, state),
                    "Unsupported native scheduled block {id} at {p:?}"
                ),
            }
        }
        Ok(out)
    }
    fn flow_into(
        &mut self,
        world: &mut impl BlockAccess,
        p: IVec3,
        fluid: u16,
        depth: i32,
        out: &mut TickResult,
    ) {
        if !can_flow(world, p, fluid) {
            return;
        }
        let old = world.get(p);
        if old != 0 && fluid == 10 {
            let (clip, vol, pitch) = self.fizz(world);
            out.world_sounds
                .push((clip, p.as_dvec3() + glam::DVec3::splat(0.5), vol, pitch));
            out.mix_smoke.push(p);
        }
        if old != 0 && !is_liquid(old) && fluid == 8 && !world.drop_block_item(p, old) {
            out.washed.push((p, old));
        }
        self.write(world, p, (fluid << 4) | (depth as u16 & 15), true, out);
    }
    fn liquid_tick(
        &mut self,
        world: &mut impl BlockAccess,
        p: IVec3,
        fluid: u16,
        out: &mut TickResult,
        random: Option<&mut JavaRandom>,
    ) {
        let mut depth = level(world, p, fluid);
        // Both original flag reads occur after level() and before the next
        // world callback. Keep this snapshot local to the current update.
        let (decay, mut delay) = if fluid == 10 {
            if self.nether() { (1, 10) } else { (2, 30) }
        } else {
            (1, self.delay(fluid))
        };
        if depth > 0 {
            let mut smallest = -100;
            let mut sources = 0;
            for d in HORIZONTAL {
                let mut n = level(world, p + d, fluid);
                if n < 0 {
                    continue;
                }
                if n == 0 {
                    sources += 1;
                }
                if n >= 8 {
                    n = 0;
                }
                if smallest < 0 || n < smallest {
                    smallest = n;
                }
            }
            let mut next = smallest + decay;
            if next >= 8 || smallest < 0 {
                next = -1;
            }
            let above = level(world, p + IVec3::Y, fluid);
            if above >= 0 {
                next = if above >= 8 { above } else { above + 8 };
            }
            let below = world.get(p - IVec3::Y);
            if sources >= 2
                && fluid == 8
                && (material_is_solid(below) || kind(below) == 8 && below & 15 == 0)
            {
                next = 0;
            }
            if fluid == 10 && depth < 8 && next < 8 && next > depth {
                // Java Random LCG; separate scheduler RNG is deterministic, but
                // does not reproduce the complete world's interleaved RNG stream.
                let slow = if let Some(random) = random {
                    random.next_int(4) != 0
                } else {
                    self.draw_world(world, 4) != 0
                };
                if slow {
                    delay *= 4;
                }
            }
            if next == depth {
                self.write(world, p, ((fluid + 1) << 4) | depth as u16, false, out);
            } else {
                depth = next;
                if depth < 0 {
                    self.write(world, p, 0, true, out);
                } else {
                    self.write(world, p, (fluid << 4) | depth as u16, false, out);
                    self.request_update(world, p, fluid, delay, out);
                    self.neighbors_changed(world, p, fluid, out);
                }
            }
        } else {
            self.write(world, p, ((fluid + 1) << 4) | depth as u16, false, out);
        }
        let down = p - IVec3::Y;
        if can_flow(world, down, fluid) {
            if fluid == 10 && kind(world.get(down)) == 8 {
                self.write(world, down, 1 << 4, true, out);
                let (clip, vol, pitch) = self.fizz(world);
                out.world_sounds.push((
                    clip,
                    down.as_dvec3() + glam::DVec3::splat(0.5),
                    vol,
                    pitch,
                ));
                out.mix_smoke.push(down);
                return;
            }
            self.flow_into(
                world,
                down,
                fluid,
                if depth >= 8 { depth } else { depth + 8 },
                out,
            );
        } else if depth >= 0 && (depth == 0 || flow_blocked(world, down)) {
            let next = if depth >= 8 { 1 } else { depth + decay };
            if next >= 8 {
                return;
            }
            for d in flow_directions(world, p, fluid) {
                self.flow_into(world, p + d, fluid, next, out);
            }
        }
    }
}
fn flow_cost(world: &impl BlockAccess, p: IVec3, fluid: u16, distance: u8, back: IVec3) -> u16 {
    let mut best = 1000;
    for d in HORIZONTAL {
        let at = p + d;
        if d == back || flow_blocked(world, at) || level(world, at, fluid) == 0 {
            continue;
        }
        if !flow_blocked(world, at - IVec3::Y) {
            return distance as u16;
        }
        if distance < 4 {
            best = best.min(flow_cost(world, at, fluid, distance + 1, -d));
        }
    }
    best
}
struct FlowDirections(u8);
impl Iterator for FlowDirections {
    type Item = IVec3;
    fn next(&mut self) -> Option<IVec3> {
        if self.0 == 0 { return None; }
        let index = self.0.trailing_zeros() as usize;
        self.0 &= self.0 - 1;
        Some([IVec3::NEG_Z, IVec3::Z, IVec3::NEG_X, IVec3::X][index])
    }
}
fn flow_directions(world: &impl BlockAccess, p: IVec3, fluid: u16) -> FlowDirections {
    let mut best = 1000;
    let mut directions = 0;
    // Search still follows Plane.HORIZONTAL: NORTH, EAST, SOUTH, WEST.
    // Bits instead follow EnumSet's result order: NORTH, SOUTH, WEST, EAST.
    for (d, bit) in HORIZONTAL.into_iter().zip([1, 8, 2, 4]) {
        let at = p + d;
        if flow_blocked(world, at) || level(world, at, fluid) == 0 {
            continue;
        }
        let cost = if flow_blocked(world, at - IVec3::Y) {
            flow_cost(world, at, fluid, 1, -d)
        } else {
            0
        };
        if cost < best {
            directions = 0;
        }
        if cost <= best {
            directions |= bit;
            best = cost;
        }
    }
    FlowDirections(directions)
}

#[cfg(test)]
#[path = "block_ticks_flow_tests.rs"]
mod flow_tests;

#[cfg(test)]
#[path = "block_ticks_load_tests.rs"]
mod load_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Chunk;
    use std::{collections::HashMap, path::PathBuf, sync::Arc};
    // Independent full-scan discovery preserves scheduler insertion order and
    // fire RNG; gravity uses the vanilla explicit-event-only load rule.
    fn discover_by_full_scan(ticks: &mut BlockTicks, world: &World) {
        ticks.state().known_chunks.retain(|p| world.chunks.contains_key(p));
        let mut fresh = world.chunks.keys()
            .filter(|p| !ticks.state().known_chunks.contains(p)).copied().collect::<Vec<_>>();
        fresh.sort_unstable();
        for cp in fresh {
            ticks.state().known_chunks.insert(cp);
            let chunk = &world.chunks[&cp];
            for y in 0..chunk.top.min(HEIGHT) { for z in 0..16 { for x in 0..16 {
                let state = chunk.get(x, y, z);
                let p = IVec3::new(cp.0 * 16 + x, y, cp.1 * 16 + z);
                if state >> 4 == 51 { ticks.discover_fire(p); continue; }
                let k = kind(state);
                if k == 0 { continue; }
                let active = match k {
                    // Independent full scan follows the same vanilla load rule:
                    // only explicit falling-block events create gravity ticks.
                    12 | 13 | 122 | 145 => false,
                    _ => state >> 4 == k,
                };
                if active { ticks.schedule(p, k, ticks.delay(k)); }
            } } }
        }
    }
    fn assert_discovery_equal(actual: &mut BlockTicks, reference: &mut BlockTicks, world: &World) {
        actual.discover_chunks(world);
        discover_by_full_scan(reference, world);
        assert_eq!(serde_json::to_value(&*actual).unwrap(), serde_json::to_value(&*reference).unwrap());
        assert_eq!(actual.state().known_chunks, reference.state().known_chunks);
        assert_eq!(actual.world_random().state(), reference.world_random().state());
    }
    #[test]
    fn cached_scheduled_discovery_preserves_scan_order_rng_and_reload_behavior() {
        let mut world = flat_world();
        world.chunks.clear();
        let mut random = JavaRandom::new(189);
        let ids = [0u16, 1, 8, 9, 10, 11, 12, 13, 51, 145];
        for cp in [(2, -1), (-2, 1), (0, 0), (1, 0)] {
            let mut chunk = Chunk::empty();
            for _ in 0..1024 {
                let x = random.next_int(16);
                let y = random.next_int(256);
                let z = random.next_int(16);
                let id = ids[random.next_int(ids.len() as i32) as usize];
                chunk.set(x, y, z, id << 4 | random.next_int(16) as u16);
            }
            chunk.set(0, 1, 0, 51 << 4);
            chunk.set(1, 1, 0, 12 << 4);
            chunk.set(2, 1, 0, 9 << 4);
            chunk.set(3, 1, 0, 11 << 4);
            chunk.prepare_tick_indexes();
            world.chunks.insert(cp, Arc::new(chunk));
        }
        for nether in [false, true] {
            let mut world = world.clone();
            let mut actual = BlockTicks::default();
            actual.set_nether(nether);
            actual.set_clock(99);
            actual.set_world_random(&JavaRandom::new(189));
            // Persisted fire ticks must suppress both a second schedule and a
            // second random delay draw when the chunk is first discovered.
            actual.schedule_block_update(IVec3::new(0, 1, 0), 51, 17, -1);
            let mut reference = actual.clone();
            assert_discovery_equal(&mut actual, &mut reference, &world);
            let first = serde_json::to_value(&actual).unwrap();
            assert_discovery_equal(&mut actual, &mut reference, &world);
            assert_eq!(serde_json::to_value(&actual).unwrap(), first);

            // Modify a warm index, keeping the original loaded-chunk rule:
            // discovery itself does not requeue already-known columns.
            world.set(IVec3::new(0, 1, 0), 0);
            world.set(IVec3::new(1, 1, 0), 145 << 4 | 2);
            world.set(IVec3::new(2, 1, 0), 8 << 4 | 3);
            world.set(IVec3::new(3, 1, 0), 51 << 4 | 7);
            world.set(IVec3::new(4, 255, 0), 51 << 4 | 15);
            assert_discovery_equal(&mut actual, &mut reference, &world);
            let removed = world.chunks.remove(&(0, 0)).unwrap();
            assert_discovery_equal(&mut actual, &mut reference, &world);
            actual.set_clock(150);
            reference.set_clock(150);
            assert_eq!(serde_json::to_value(actual.take_due()).unwrap(),
                serde_json::to_value(reference.take_due()).unwrap());
            world.chunks.insert((0, 0), removed);
            assert_discovery_equal(&mut actual, &mut reference, &world);

            // Serde deliberately forgets discovery membership; pending queue
            // restoration still deduplicates fire and keeps the same RNG state.
            let mut actual: BlockTicks = serde_json::from_value(serde_json::to_value(actual).unwrap()).unwrap();
            let mut reference: BlockTicks = serde_json::from_value(serde_json::to_value(reference).unwrap()).unwrap();
            assert_discovery_equal(&mut actual, &mut reference, &world);
        }
    }
    #[test]
    fn native_scheduled_queue_matches_original_priority_budget_and_reentry() {
        struct Probe {
            ticks: BlockTicks,
            loaded: bool,
            nested: bool,
            stale: HashSet<IVec3>,
            called: Vec<[i64; 4]>,
        }
        impl BlockAccess for Probe {
            fn get(&self, p: IVec3) -> u16 {
                if self.stale.contains(&p) {
                    0
                } else {
                    1 << 4
                }
            }
            fn chunk_loaded(&self, _: ChunkPos) -> bool {
                self.loaded
            }
            fn set_before_added(&mut self, _: IVec3, _: u16) -> bool {
                false
            }
            fn scheduled_tick(&mut self, p: IVec3, _: u16) -> bool {
                self.called.push([
                    self.ticks.clock() as i64,
                    p.x as i64,
                    p.y as i64,
                    p.z as i64,
                ]);
                if self.nested && p.x == 0 {
                    self.ticks
                        .schedule_block_update(IVec3::new(1, 64, 0), 1, 0, 0);
                    self.ticks.schedule_block_update(p, 1, 0, 0);
                }
                true
            }
        }
        fn compare(probe: &Probe, ticks: &BlockTicks, expected: &serde_json::Value) {
            let mut queue = Vec::new();
            for (&time, entries) in &ticks.state().queue {
                let mut entries = entries.clone();
                entries.sort_by_key(|e| e.priority);
                for entry in entries {
                    queue.push([
                        entry.position[0] as i64,
                        entry.position[1] as i64,
                        entry.position[2] as i64,
                        time as i64,
                        entry.priority as i64,
                    ]);
                }
            }
            assert_eq!(serde_json::json!([probe.called, queue]), *expected);
        }
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-scheduled-ticks.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            let mut ticks = BlockTicks::default();
            let mut probe = Probe {
                ticks: ticks.share_scheduler(),
                loaded: true,
                nested: case["mode"] == 4,
                stale: HashSet::new(),
                called: Vec::new(),
            };
            let mut step = 0;
            for action in case["actions"].as_array().unwrap() {
                let a = action
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n.as_i64().unwrap() as i32)
                    .collect::<Vec<_>>();
                match a[0] {
                    0 => ticks.schedule_block_update(
                        IVec3::new(a[1], a[2], a[3]),
                        1,
                        a[4] as u64,
                        a[5],
                    ),
                    1 => probe.loaded = a[1] != 0,
                    2 => {
                        probe.stale.insert(IVec3::new(a[1], a[2], a[3]));
                    }
                    3 => {
                        if step == 0 {
                            compare(&probe, &ticks, &case["snapshots"][0]);
                        }
                        ticks.tick_access(&mut probe).unwrap();
                        step += 1;
                        compare(&probe, &ticks, &case["snapshots"][step]);
                        if step == 3 {
                            ticks = serde_json::from_slice(&serde_json::to_vec(&ticks).unwrap())
                                .unwrap();
                            probe.ticks = ticks.share_scheduler();
                        }
                    }
                    _ => unreachable!(),
                }
            }
            assert_eq!(step, 6);
        }
    }
    #[test]
    fn springs_match_original_worldserver_recursion_and_both_rng_streams() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/vanilla189-springs.json"))
                .unwrap();
        assert_eq!(cases.as_array().unwrap().len(), 504);
        // The eventful lighting/runtime adapter also verifies the vegetation
        // cases. This legacy World adapter has no plant/item callback manager.
        for (number, case) in cases
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .filter(|(_, case)| case["mode"].as_u64().unwrap() < 13)
        {
            let mut world = flat_world();
            world.chunks.clear();
            for cx in -2..=2 {
                for cz in -2..=2 {
                    let mut chunk = Chunk::empty();
                    for x in 0..16 {
                        for z in 0..16 {
                            chunk.set(x, 0, z, 7 << 4);
                        }
                    }
                    world.chunks.insert((cx, cz), Arc::new(chunk));
                }
            }
            for edit in case["initial"].as_array().unwrap() {
                world.set(
                    IVec3::new(
                        edit[0].as_i64().unwrap() as i32,
                        edit[1].as_i64().unwrap() as i32,
                        edit[2].as_i64().unwrap() as i32,
                    ),
                    edit[3].as_u64().unwrap() as u16,
                );
            }
            let seed = case["seed"].as_str().unwrap().parse::<i64>().unwrap();
            let p = IVec3::new(
                case["position"][0].as_i64().unwrap() as i32,
                case["position"][1].as_i64().unwrap() as i32,
                case["position"][2].as_i64().unwrap() as i32,
            );
            let mut population = JavaRandom::new(seed);
            let mut world_random = JavaRandom::new(seed ^ 0x123456789abcdef);
            let mut scheduler = BlockTicks::default();
            scheduler.set_fall_instantly(case["fall_instantly"].as_bool().unwrap_or(false));
            let (success, _) = scheduler.generate_spring(
                &mut world,
                p,
                case["fluid"].as_u64().unwrap() as u16,
                &mut population,
                &mut world_random,
            );
            assert_eq!(
                success,
                case["success"].as_bool().unwrap(),
                "spring case {number}: {case}"
            );
            let expected: HashMap<_, _> = case["final"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    (
                        (
                            e[0].as_i64().unwrap() as i32,
                            e[1].as_i64().unwrap() as i32,
                            e[2].as_i64().unwrap() as i32,
                        ),
                        e[3].as_u64().unwrap() as u16,
                    )
                })
                .collect();
            for y in 1..=32 {
                for z in p.z - 12..=p.z + 12 {
                    for x in p.x - 12..=p.x + 12 {
                        assert_eq!(
                            world.get(IVec3::new(x, y, z)),
                            *expected.get(&(x, y, z)).unwrap_or(&0),
                            "spring {number} fluid={} mode={} direction={} at{x},{y},{z}",
                            case["fluid"],
                            case["mode"],
                            case["direction"]
                        );
                    }
                }
            }
            assert_eq!(
                population.next_long().to_string(),
                case["population_next"].as_str().unwrap(),
                "spring {number} population Random"
            );
            assert_eq!(
                world_random.next_long().to_string(),
                case["world_next"].as_str().unwrap(),
                "spring {number} world Random"
            );
            let pending: Vec<_> = scheduler
                .state()
                .queue
                .iter()
                .flat_map(|(&time, entries)| {
                    entries.iter().map(move |e| {
                        vec![
                            time as i64,
                            e.position[0] as i64,
                            e.position[1] as i64,
                            e.position[2] as i64,
                            e.kind as i64,
                        ]
                    })
                })
                .collect();
            let expected_pending: Vec<Vec<i64>> =
                serde_json::from_value(case["pending"].clone()).unwrap();
            assert_eq!(
                pending, expected_pending,
                "spring {number} scheduled updates"
            );
        }
    }
    fn flat_world() -> World {
        let mut chunk = Chunk::empty();
        for x in 0..16 {
            for z in 0..16 {
                chunk.set(x, 0, z, 1 << 4);
            }
        }
        World {
            chunks: crate::world::ChunkMap::from([((0, 0), Arc::new(chunk))]),
            spawn: Vec3::new(8.5, 1., 8.5),
            label: String::new(),
            save_path: PathBuf::new(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        }
    }
    fn advance(ticks: &mut BlockTicks, world: &mut World, count: usize) -> TickResult {
        let mut result = TickResult::default();
        for _ in 0..count {
            let tick = ticks.tick(world);
            result.changed.extend(tick.changed);
            result.drops.extend(tick.drops);
            result.washed.extend(tick.washed);
        }
        result
    }
    fn falling_world() -> World {
        let mut world = flat_world();
        let empty = Arc::new(Chunk::empty());
        for x in -2..=2 {
            for z in -2..=2 {
                world
                    .chunks
                    .entry((x, z))
                    .or_insert_with(|| Arc::clone(&empty));
            }
        }
        world
    }
    #[test]
    fn water_waits_five_ticks_and_has_seven_block_horizontal_reach() {
        let mut world = flat_world();
        let source = IVec3::new(8, 1, 8);
        world.set(source, 8 << 4);
        let mut ticks = BlockTicks::default();
        advance(&mut ticks, &mut world, 4);
        assert_eq!(world.get(source + IVec3::X), 0);
        ticks.tick(&mut world);
        assert_eq!(kind(world.get(source + IVec3::X)), 8);
        assert_eq!(world.get(source + IVec3::X) & 15, 1);
        advance(&mut ticks, &mut world, 140);
        for distance in 1..=7 {
            assert_eq!(level(&world, source + IVec3::X * distance, 8), distance);
        }
        assert_eq!(world.get(IVec3::new(0, 1, 8)), 0);
    }
    #[test]
    fn water_prioritizes_downward_flow_and_removal_recedes() {
        let mut world = flat_world();
        let source = IVec3::new(8, 4, 8);
        world.set(source, 8 << 4);
        let mut ticks = BlockTicks::default();
        advance(&mut ticks, &mut world, 5);
        assert_eq!(level(&world, source - IVec3::Y, 8), 8);
        assert_eq!(world.get(source + IVec3::X), 0);
        world.set(source, 0);
        ticks.notify(&world, source);
        advance(&mut ticks, &mut world, 160);
        assert!(world
            .chunks
            .values()
            .all(|c| c.blocks.iter().all(|&s| kind(s) != 8)));
    }
    #[test]
    fn source_regeneration_requires_two_water_sources_and_support() {
        let mut world = flat_world();
        let center = IVec3::new(8, 1, 8);
        world.set(center - IVec3::X, 9 << 4);
        world.set(center + IVec3::X, 9 << 4);
        world.set(center, (8 << 4) | 1);
        let mut ticks = BlockTicks::default();
        advance(&mut ticks, &mut world, 5);
        assert_eq!(level(&world, center, 8), 0);
        let mut world = flat_world();
        let center = IVec3::new(8, 3, 8);
        world.set(center - IVec3::X, 9 << 4);
        world.set(center + IVec3::X, 9 << 4);
        world.set(center, (8 << 4) | 1);
        let mut ticks = BlockTicks::default();
        advance(&mut ticks, &mut world, 5);
        assert_ne!(level(&world, center, 8), 0);
    }
    #[test]
    fn overworld_lava_has_thirty_tick_delay_and_decay_two() {
        let mut world = flat_world();
        let source = IVec3::new(8, 1, 8);
        world.set(source, 10 << 4);
        let mut ticks = BlockTicks::default();
        advance(&mut ticks, &mut world, 29);
        assert_eq!(world.get(source + IVec3::X), 0);
        ticks.tick(&mut world);
        assert_eq!(level(&world, source + IVec3::X, 10), 2);
        advance(&mut ticks, &mut world, 180);
        for distance in 1..=3 {
            assert_eq!(
                level(&world, source + IVec3::X * distance, 10),
                distance * 2
            );
        }
        assert_eq!(world.get(source + IVec3::X * 4), 0);
    }
    #[test]
    fn liquid_contact_matches_original_for_both_placement_orders_on_every_face_and_level() {
        let p = IVec3::new(8, 5, 8);
        let mut world = flat_world();
        for lava in [10, 11] {
            for level in 0..16 {
                for face in NEIGHBORS {
                    for water_first in [false, true] {
                        let q = p + face;
                        let state = (lava << 4) | level;
                        world.set(p, if water_first { 0 } else { state });
                        world.set(q, if water_first { 9 << 4 } else { 0 });
                        let mut ticks = BlockTicks::default();
                        let (target, new_state) = if water_first { (p, state) } else { (q, 9 << 4) };
                        world.set(target, new_state);
                        ticks.notify(&world, target);
                        let result = ticks.tick(&mut world);
                        let hardens = face != IVec3::NEG_Y && level <= 4;
                        let expected = if hardens {
                            (if level == 0 { 49 } else { 4 }) << 4
                        } else if !water_first {
                            (10 << 4) | level
                        } else {
                            state
                        };
                        assert_eq!(world.get(p), expected, "lava {lava}:{level}, {face:?}, water first {water_first}");
                        assert_eq!(result.mix_smoke.contains(&p), hardens);
                        world.set(p, 0);
                        world.set(q, 0);
                    }
                }
            }
        }
    }
    #[test]
    fn lava_mixing_distinguishes_sources_flows_and_downward_water() {
        for (depth, expected) in [(0, 49), (3, 4), (5, 10)] {
            let mut world = flat_world();
            let lava = IVec3::new(8, 2, 8);
            world.set(lava, (10 << 4) | depth);
            world.set(lava + IVec3::X, 9 << 4);
            let mut ticks = BlockTicks::default();
            ticks.notify(&world, lava + IVec3::X);
            ticks.tick(&mut world);
            assert_eq!(world.get(lava) >> 4, expected);
        }
        let mut world = flat_world();
        let lava = IVec3::new(8, 2, 8);
        world.set(lava, 10 << 4);
        world.set(lava - IVec3::Y, 9 << 4);
        // Block source water from spreading up beside the lava during its delay.
        for d in HORIZONTAL {
            world.set(lava - IVec3::Y + d, 1 << 4);
            world.set(lava + d, 1 << 4);
        }
        let mut ticks = BlockTicks::default();
        advance(&mut ticks, &mut world, 30);
        assert_eq!(world.get(lava - IVec3::Y), 1 << 4);
    }
    #[test]
    fn downhill_route_is_preferred_over_equal_height_spreading() {
        let mut world = flat_world();
        let source = IVec3::new(8, 2, 8);
        for x in 0..16 {
            for z in 0..16 {
                world.set(IVec3::new(x, 1, z), 1 << 4);
            }
        }
        world.set(IVec3::new(10, 1, 8), 0);
        world.set(source, 8 << 4);
        let mut ticks = BlockTicks::default();
        advance(&mut ticks, &mut world, 5);
        assert_eq!(level(&world, source + IVec3::X, 8), 1);
        assert_eq!(world.get(source - IVec3::X), 0);
        assert_eq!(world.get(source + IVec3::Z), 0);
    }
    #[test]
    fn water_displaces_crops_but_signs_and_doors_block_flow() {
        let mut world = flat_world();
        let source = IVec3::new(8, 1, 8);
        world.set(source, 8 << 4);
        world.set(source + IVec3::X, (59 << 4) | 7);
        world.set(source - IVec3::X, 63 << 4);
        world.set(source + IVec3::Z, (64 << 4) | 4);
        let mut ticks = BlockTicks::default();
        let result = advance(&mut ticks, &mut world, 5);
        assert!(result.washed.contains(&(source + IVec3::X, (59 << 4) | 7)));
        assert_eq!(world.get(source - IVec3::X), 63 << 4);
        assert_eq!(world.get(source + IVec3::Z), (64 << 4) | 4);
    }
    #[test]
    fn falling_block_delay_acceleration_drag_and_landing_match_reference() {
        let mut world = falling_world();
        let p = IVec3::new(8, 5, 8);
        world.set(p, (12 << 4) | 1);
        let mut ticks = BlockTicks::default();
        ticks.notify(&world, p); // This fixture places a block, rather than loading it.
        ticks.tick(&mut world);
        assert_eq!(world.get(p), (12 << 4) | 1);
        ticks.tick(&mut world);
        assert_eq!(world.get(p), 0);
        assert_eq!(ticks.falling.len(), 1);
        assert!((ticks.falling[0].position.y - 4.96).abs() < 1e-5);
        ticks.tick(&mut world);
        assert!((ticks.falling[0].position.y - 4.8808).abs() < 1e-5);
        assert!((ticks.falling[0].velocity.y - (-0.077616)).abs() < 1e-5);
        let result = advance(&mut ticks, &mut world, 60);
        assert!(ticks.falling.is_empty());
        assert!(result.drops.is_empty());
        assert_eq!(world.get(IVec3::new(8, 1, 8)), (12 << 4) | 1);
    }
    #[test]
    fn unsupported_stack_falls_in_order_and_torch_landing_drops_item() {
        let mut world = falling_world();
        world.set(IVec3::new(8, 4, 8), 12 << 4);
        world.set(IVec3::new(8, 5, 8), 13 << 4);
        let mut ticks = BlockTicks::default();
        ticks.notify(&world, IVec3::new(8, 4, 8));
        ticks.notify(&world, IVec3::new(8, 5, 8));
        advance(&mut ticks, &mut world, 80);
        assert_eq!(world.get(IVec3::new(8, 1, 8)), 12 << 4);
        assert_eq!(world.get(IVec3::new(8, 2, 8)), 13 << 4);
        let mut world = falling_world();
        world.set(IVec3::new(8, 1, 8), 50 << 4);
        world.set(IVec3::new(8, 5, 8), 13 << 4);
        let mut ticks = BlockTicks::default();
        ticks.notify(&world, IVec3::new(8, 5, 8));
        let result = advance(&mut ticks, &mut world, 80);
        assert_eq!(result.drops.len(), 1);
        assert_eq!(result.drops[0].1, 13 << 4);
        assert_eq!(world.get(IVec3::new(8, 1, 8)), 50 << 4);
    }
    #[test]
    fn waterfall_pull_uses_material_solidity_not_collision_shapes() {
        let p = IVec3::new(8, 4, 8);
        for (id, pulls) in [
            (1, true),
            (63, true),
            (68, true),
            (70, true),
            (72, true),
            (27, false),
            (28, false),
            (79, false),
        ] {
            for height in [0, 1] {
                let mut world = flat_world();
                world.set(p, (8 << 4) | 8);
                world.set(p + IVec3::X + IVec3::Y * height, id << 4);
                assert_eq!(
                    crate::vanilla::movement::flow(&world, p).as_vec3(),
                    if pulls { Vec3::NEG_Y } else { Vec3::ZERO },
                    "neighbor ID {id}, height {height}"
                );
            }
        }
    }
    #[test]
    fn scheduled_updates_and_inflight_blocks_survive_serialization() {
        let mut world = falling_world();
        world.set(IVec3::new(8, 10, 8), 13 << 4);
        world.set(IVec3::new(3, 1, 3), 8 << 4);
        let mut ticks = BlockTicks::default();
        ticks.notify(&world, IVec3::new(8, 10, 8));
        advance(&mut ticks, &mut world, 3);
        let data = serde_json::to_vec(&ticks).unwrap();
        let mut resumed: BlockTicks = serde_json::from_slice(&data).unwrap();
        let mut cloned = world.clone();
        advance(&mut ticks, &mut world, 100);
        advance(&mut resumed, &mut cloned, 100);
        assert_eq!(world.chunks[&(0, 0)].blocks, cloned.chunks[&(0, 0)].blocks);
        assert_eq!(ticks.falling.len(), resumed.falling.len());
    }

    #[test]
    fn local_saved_f32_faller_migrates_and_lands_once() {
        let mut world = falling_world();
        let mut ticks: BlockTicks = serde_json::from_value(serde_json::json!({
            "clock":4,"falling":[{"state":193,"position":[8.5,4.8808,8.5],"previous":[8.5,4.96,8.5],"velocity":[0.0,-0.077616,0.0],"age":2}]
        })).unwrap();
        assert_eq!(ticks.falling[0].position.y, 4.8808f32 as f64);
        assert_eq!(ticks.falling[0].age, 2);
        assert!(ticks.state().do_entity_drops);
        let result = advance(&mut ticks, &mut world, 30);
        assert!(ticks.falling.is_empty());
        assert!(result.drops.is_empty());
        assert_eq!(world.get(IVec3::new(8, 1, 8)), 193);
    }
    #[test]
    fn nested_world_callbacks_share_queue_without_holding_scheduler_locks() {
        struct Nested {
            world: World,
            ticks: BlockTicks,
            entered: bool,
        }
        impl BlockAccess for Nested {
            fn get(&self, p: IVec3) -> u16 {
                self.world.get(p)
            }
            fn chunk_loaded(&self, p: ChunkPos) -> bool {
                self.world.chunks.contains_key(&p)
            }
            fn set_before_added(&mut self, p: IVec3, state: u16) -> bool {
                self.world.set(p, state)
            }
            fn after_added(&mut self, _p: IVec3, _old: u16, _requested: u16) {
                if self.entered {
                    return;
                }
                self.entered = true;
                let mut callback = self.ticks.share_scheduler();
                callback.set_block(self, IVec3::new(2, 3, 2), 8 << 4, false);
                callback.set_block(self, IVec3::new(6, 3, 6), 10 << 4, false);
                callback.set_block(self, IVec3::new(2, 3, 2), 8 << 4, false);
            }
        }
        let mut ticks = BlockTicks::default();
        ticks.set_clock(100);
        ticks.set_nether(true);
        let mut world = Nested {
            world: flat_world(),
            ticks: ticks.share_scheduler(),
            entered: false,
        };
        ticks.set_block(&mut world, IVec3::new(10, 3, 10), 8 << 4, false);
        let saved = serde_json::to_value(&ticks).unwrap();
        assert_eq!(saved["clock"], 100);
        assert_eq!(saved["nether"], true);
        assert!(saved.get("scheduler").is_none());
        let queue = &saved["queue"];
        assert_eq!(queue["105"].as_array().unwrap().len(), 2);
        assert_eq!(queue["110"].as_array().unwrap().len(), 1);
        assert_eq!(queue["105"][0]["position"], serde_json::json!([10, 3, 10]));
        assert_eq!(queue["105"][1]["position"], serde_json::json!([2, 3, 2]));
        assert_eq!(world.ticks.clock(), 100);
        let snapshot = ticks.clone();
        world.ticks.set_clock(101);
        assert_eq!(ticks.clock(), 101);
        assert_eq!(snapshot.clock(), 100);
        // Original forceBlockUpdateTick uses a shared boolean, not nesting depth.
        ticks.state().immediate = true;
        assert!(world.ticks.immediate());
        world.ticks.state().immediate = false;
        assert!(!ticks.immediate());
        let restored: BlockTicks = serde_json::from_value(saved.clone()).unwrap();
        assert_eq!(serde_json::to_value(restored).unwrap(), saved);
    }
}
