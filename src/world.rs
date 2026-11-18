use crate::{
    assets,
    nbt::{self, Tag},
    Options,
};
use anyhow::{bail, ensure, Context, Result};
use flate2::read::{GzDecoder, ZlibDecoder};
use glam::{IVec3, Vec3};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::File,
    io::{BufReader, BufWriter, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
};

pub const HEIGHT: i32 = 256;
pub type ChunkPos = (i32, i32);
/// Chunk coordinates are hot integer keys, independent of traversal order.
/// Gameplay that requires an order uses the provider's ordered records.
pub type ChunkMap<V = Arc<Chunk>> = ahash::AHashMap<ChunkPos, V>;
#[path = "edit_journal.rs"]
mod edit_journal;
pub(crate) use edit_journal::EditChanges;

#[derive(Debug, Default)]
struct JournalTracking {
    dirty: std::collections::HashSet<[i32; 3]>,
    reset: bool,
}

/// O(1) copy-on-write sparse block-edit journal backed by `Arc`.
/// Cloning `World` shares the underlying edit map and derived column index
/// without copying thousands of entries while keeping sync cursors owner-local.
#[derive(Debug, Default)]
pub struct EditJournal {
    inner: Arc<edit_journal::EditJournal>,
    tracking: Option<JournalTracking>,
}

impl EditJournal {
    pub fn insert(&mut self, position: [i32; 3], state: u16) -> Option<u16> {
        let old = Arc::make_mut(&mut self.inner).insert(position, state);
        if old != Some(state) {
            self.record_changed(position);
        }
        old
    }

    pub fn remove(&mut self, position: &[i32; 3]) -> Option<u16> {
        let old = Arc::make_mut(&mut self.inner).remove(position);
        if old.is_some() {
            self.record_changed(*position);
        }
        old
    }

    pub fn clear(&mut self) {
        if !self.inner.is_empty() {
            Arc::make_mut(&mut self.inner).clear();
        }
        if let Some(tracking) = &mut self.tracking {
            tracking.dirty.clear();
            tracking.reset = true;
        }
    }

    fn record_changed(&mut self, position: [i32; 3]) {
        if let Some(tracking) = &mut self.tracking {
            if !tracking.reset {
                tracking.dirty.insert(position);
            }
        }
    }

    pub(crate) fn intersects_columns(
        &self,
        positions: impl IntoIterator<Item = ChunkPos>,
    ) -> (bool, usize) {
        self.inner.intersects_columns(positions)
    }

    pub(crate) fn matches_published_columns<'a>(
        &self,
        chunks: impl IntoIterator<Item = (ChunkPos, &'a Chunk)>,
    ) -> (bool, usize) {
        self.inner.matches_published_columns(chunks)
    }

    pub(crate) fn take_changes(&mut self) -> EditChanges {
        let Some(tracking) = &mut self.tracking else {
            self.tracking = Some(JournalTracking::default());
            return EditChanges::Reset;
        };
        if std::mem::take(&mut tracking.reset) {
            tracking.dirty.clear();
            EditChanges::Reset
        } else {
            EditChanges::Positions(std::mem::take(&mut tracking.dirty))
        }
    }
}

impl Clone for EditJournal {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            tracking: None,
        }
    }
}

impl PartialEq for EditJournal {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner) || *self.inner == *other.inner
    }
}
impl Eq for EditJournal {}

impl std::ops::Deref for EditJournal {
    type Target = HashMap<[i32; 3], u16>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'a> IntoIterator for &'a EditJournal {
    type Item = (&'a [i32; 3], &'a u16);
    type IntoIter = std::collections::hash_map::Iter<'a, [i32; 3], u16>;
    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

impl From<HashMap<[i32; 3], u16>> for EditJournal {
    fn from(values: HashMap<[i32; 3], u16>) -> Self {
        Self {
            inner: Arc::new(edit_journal::EditJournal::from(values)),
            tracking: None,
        }
    }
}

impl From<std::collections::BTreeMap<[i32; 3], u16>> for EditJournal {
    fn from(values: std::collections::BTreeMap<[i32; 3], u16>) -> Self {
        Self::from(values.into_iter().collect::<HashMap<_, _>>())
    }
}

impl FromIterator<([i32; 3], u16)> for EditJournal {
    fn from_iter<T: IntoIterator<Item = ([i32; 3], u16)>>(iter: T) -> Self {
        Self::from(iter.into_iter().collect::<HashMap<_, _>>())
    }
}

impl Extend<([i32; 3], u16)> for EditJournal {
    fn extend<T: IntoIterator<Item = ([i32; 3], u16)>>(&mut self, iter: T) {
        let inner = Arc::make_mut(&mut self.inner);
        match &mut self.tracking {
            Some(tracking) if !tracking.reset => {
                inner.extend(iter.into_iter().inspect(|(position, _)| {
                    tracking.dirty.insert(*position);
                }));
            }
            _ => inner.extend(iter),
        }
    }
}

impl Serialize for EditJournal {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let ordered: std::collections::BTreeMap<_, _> = self.inner.iter().collect();
        ordered.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for EditJournal {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let map = HashMap::<[i32; 3], u16>::deserialize(deserializer)?;
        Ok(Self::from(map))
    }
}
/// Exact replacement states admitted by the native inert ore adapter.
pub(crate) fn inert_ore_replacement(state: u16) -> bool {
    (16..=22).contains(&state)
        || matches!(state, 48 | 224 | 240 | 256 | 336 | 896 | 1168 | 2064 | 2448)
}
#[derive(Clone, Default)]
pub(crate) struct RandomTickSections {
    counts: [u16; 16],
    mask: u16,
}
#[derive(Clone, Default)]
pub(crate) struct SpecialBlockIndexes {
    pub(crate) signs: std::sync::OnceLock<Vec<u16>>,
    pub(crate) spawners: std::sync::OnceLock<Vec<u16>>,
    pub(crate) end_portals: std::sync::OnceLock<Vec<u16>>,
    pub(crate) portals: std::sync::OnceLock<Vec<u16>>,
}
impl SpecialBlockIndexes {
    pub(crate) fn get(&self) -> Option<&Vec<u16>> {
        self.signs
            .get()
            .or_else(|| self.spawners.get())
            .or_else(|| self.end_portals.get())
            .or_else(|| self.portals.get())
    }
}
#[derive(Clone)]
pub struct Chunk {
    pub blocks: crate::block_states::BlockStates,
    pub biomes: [u8; 256],
    pub top: i32,
    pub(crate) random_tick_sections: std::sync::OnceLock<RandomTickSections>,
    pub(crate) scheduled_tick_blocks: std::sync::OnceLock<Vec<(u16, u16)>>,
    pub(crate) partial_light_sections: std::sync::OnceLock<u16>,
    pub(crate) skulls: std::sync::OnceLock<Vec<u16>>,
    pub(crate) signs: SpecialBlockIndexes,
    pub containers: Vec<(IVec3, crate::container::Container)>,
    /// Indexed while decoding or editing blocks, in tile insertion order within the chunk.
    pub hoppers: Vec<[i32; 3]>,
    pub entities: Vec<crate::villagers::ImportedEntity>,
    /// Moving-block tiles use chunk-local X/Z, matching section coordinates.
    pub pistons: HashMap<[i32; 3], crate::vanilla::collision_shapes::PistonTile>,
    /// Original typed tile NBT, addressed by chunk-local X/Z. A live inventory
    /// overlays its current slots when producing a pick-block stack.
    pub tiles: HashMap<[i32; 3], Arc<crate::vanilla::drops::DropNbt>>,
    /// Immutable lighting snapshot shared with background mesh jobs.
    pub light: Option<Arc<crate::vanilla::lighting::ChunkLight>>,
    pub visual_light: Option<Arc<crate::vanilla::lighting::ChunkLight>>,
    pub(crate) light_edits: Vec<(u16, u16)>,
}
impl Chunk {
    pub fn empty() -> Self {
        Self {
            blocks: vec![0; 16 * 16 * HEIGHT as usize].into(),
            biomes: [1; 256],
            top: 0,
            random_tick_sections: Default::default(),
            scheduled_tick_blocks: Default::default(),
            partial_light_sections: Default::default(),
            skulls: Default::default(),
            signs: Default::default(),
            containers: Vec::new(),
            hoppers: Vec::new(),
            entities: Vec::new(),
            pistons: HashMap::new(),
            tiles: HashMap::new(),
            light: None,
            visual_light: None,
            light_edits: Vec::new(),
        }
    }
    pub fn index(x: i32, y: i32, z: i32) -> usize {
        ((y * 16 + z) * 16 + x) as usize
    }
    pub fn get(&self, x: i32, y: i32, z: i32) -> u16 {
        self.blocks.value_at(Self::index(x, y, z))
    }
    pub(crate) fn random_tick_mask(&self) -> u16 {
        self.random_tick_sections.get_or_init(|| {
            let mut ticks = RandomTickSections::default();
            for (section, states) in self.blocks.chunks_exact(4096).enumerate()
                .take(((self.top.clamp(0, 256) + 15) / 16) as usize) {
                ticks.counts[section] = states.iter()
                    .filter(|state| crate::gameplay::growth::ticks_randomly(**state >> 4))
                    .count() as u16;
                if ticks.counts[section] != 0 { ticks.mask |= 1 << section; }
            }
            ticks
        }).mask
    }
    pub(crate) fn scheduled_tick_candidates(&self) -> &[(u16, u16)] {
        self.scheduled_tick_blocks.get_or_init(|| self.blocks.iter().enumerate()
            .take(self.top.clamp(0, 256) as usize * 256)
            .filter(|(_, state)| scheduled_tick_candidate(**state))
            .map(|(index, state)| (index as u16, *state)).collect())
    }
    pub(crate) fn partial_light_mask(&self) -> u16 {
        *self.partial_light_sections.get_or_init(|| {
            if self.top <= 0 {
                return 0;
            }
            let max_sections = ((self.top.clamp(0, 256) + 15) / 16) as usize;
            self.blocks.with_slice(|blocks| {
                let mut mask = 0u16;
                for (section, states) in blocks.chunks_exact(4096).enumerate().take(max_sections) {
                    if states
                        .iter()
                        .any(|&state| crate::visual_light::is_partial_light_block(state >> 4))
                    {
                        mask |= 1 << section;
                    }
                }
                mask
            })
        })
    }
    pub(crate) fn skull_positions(&self) -> &[u16] {
        self.skulls.get_or_init(|| {
            if self.top <= 0 {
                return Vec::new();
            }
            let end = self.top.clamp(0, 256) as usize * 256;
            self.blocks.with_slice(|blocks| {
                blocks
                    .iter()
                    .enumerate()
                    .take(end)
                    .filter(|(_, &state)| state >> 4 == 144)
                    .map(|(index, _)| index as u16)
                    .collect()
            })
        })
    }
    pub fn sign_positions(&self) -> &[u16] {
        self.signs.signs.get_or_init(|| {
            if self.top <= 0 {
                return Vec::new();
            }
            let end = self.top.clamp(0, 256) as usize * 256;
            self.blocks.with_slice(|blocks| {
                blocks
                    .iter()
                    .enumerate()
                    .take(end)
                    .filter(|(_, &state)| matches!(state >> 4, 63 | 68))
                    .map(|(index, _)| index as u16)
                    .collect()
            })
        })
    }
    pub fn spawner_positions(&self) -> &[u16] {
        self.signs.spawners.get_or_init(|| {
            if self.top <= 0 {
                return Vec::new();
            }
            let end = self.top.clamp(0, 256) as usize * 256;
            self.blocks.with_slice(|blocks| {
                blocks
                    .iter()
                    .enumerate()
                    .take(end)
                    .filter(|(_, &state)| state >> 4 == 52)
                    .map(|(index, _)| index as u16)
                    .collect()
            })
        })
    }
    pub fn end_portal_positions(&self) -> &[u16] {
        self.signs.end_portals.get_or_init(|| {
            if self.top <= 0 {
                return Vec::new();
            }
            let end = self.top.clamp(0, 256) as usize * 256;
            self.blocks.with_slice(|blocks| {
                blocks
                    .iter()
                    .enumerate()
                    .take(end)
                    .filter(|(_, &state)| state >> 4 == 119)
                    .map(|(index, _)| index as u16)
                    .collect()
            })
        })
    }
    pub fn portal_positions(&self) -> &[u16] {
        self.signs.portals.get_or_init(|| {
            if self.top <= 0 {
                return Vec::new();
            }
            let end = self.top.clamp(0, 256) as usize * 256;
            self.blocks.with_slice(|blocks| {
                blocks
                    .iter()
                    .enumerate()
                    .take(end)
                    .filter(|(_, &state)| state >> 4 == 90)
                    .map(|(index, _)| index as u16)
                    .collect()
            })
        })
    }
    /// Pure indexes are built on the publishing worker before frame-side ticks.
    /// Neither scanning nor maintaining them consumes gameplay RNG.
    pub(crate) fn prepare_tick_indexes(&self) {
        self.random_tick_mask();
        self.scheduled_tick_candidates();
        self.skull_positions();
        self.sign_positions();
        self.spawner_positions();
        self.end_portal_positions();
        self.portal_positions();
    }
    pub(crate) fn invalidate_tick_indexes(&mut self) {
        self.random_tick_sections.take();
        self.scheduled_tick_blocks.take();
        self.partial_light_sections.take();
        self.skulls.take();
        self.signs.signs.take();
        self.signs.spawners.take();
        self.signs.end_portals.take();
        self.signs.portals.take();
    }
    pub fn set(&mut self, x: i32, y: i32, z: i32, state: u16) {
        let index = Self::index(x, y, z);
        let previous = self.blocks[index];
        if (state >> 4 == 154) != (self.blocks[index] >> 4 == 154) {
            if state >> 4 == 154 {
                self.hoppers.push([x, y, z]);
            } else {
                self.hoppers.retain(|p| *p != [x, y, z]);
            }
        }
        if !self.tiles.is_empty()
            && !crate::vanilla::middle_pick::retains_tile(self.blocks[index] >> 4, state >> 4)
        {
            self.tiles.remove(&[x, y, z]);
        }
        if self.blocks[index] >> 4 == 36 && state >> 4 != 36 {
            self.pistons.remove(&[x, y, z]);
        }
        if state >> 4 == 52 && self.blocks[index] >> 4 != 52 {
            if let Some(tile) = crate::vanilla::middle_pick::tile_snapshot(52, IVec3::new(x,y,z), None, None) {
                self.tiles.insert([x,y,z], Arc::new(tile));
            }
        }
        if let Some(ticks) = self.random_tick_sections.get_mut() {
            let old = crate::gameplay::growth::ticks_randomly(previous >> 4);
            let new = crate::gameplay::growth::ticks_randomly(state >> 4);
            if old != new {
                let section = y as usize >> 4;
                ticks.counts[section] = ticks.counts[section] - u16::from(old) + u16::from(new);
                if ticks.counts[section] == 0 { ticks.mask &= !(1 << section); }
                else { ticks.mask |= 1 << section; }
            }
        }
        if previous != state && (scheduled_tick_candidate(previous) || scheduled_tick_candidate(state)) {
            if let Some(candidates) = self.scheduled_tick_blocks.get_mut() {
                let found = candidates.binary_search_by_key(&(index as u16), |&(index, _)| index);
                match (scheduled_tick_candidate(state), found) {
                    (true, Ok(at)) => candidates[at].1 = state,
                    (true, Err(at)) => candidates.insert(at, (index as u16, state)),
                    (false, Ok(at)) => { candidates.remove(at); },
                    (false, Err(_)) => {},
                }
            }
        }
        if previous != state
            && (crate::visual_light::is_partial_light_block(previous >> 4)
                || crate::visual_light::is_partial_light_block(state >> 4))
        {
            self.partial_light_sections.take();
        }
        if (previous >> 4 == 144) != (state >> 4 == 144) {
            if let Some(skulls) = self.skulls.get_mut() {
                match (state >> 4 == 144, skulls.binary_search(&(index as u16))) {
                    (true, Err(at)) => skulls.insert(at, index as u16),
                    (false, Ok(at)) => {
                        skulls.remove(at);
                    }
                    _ => {}
                }
            }
        }
        if matches!(previous >> 4, 63 | 68) != matches!(state >> 4, 63 | 68) {
            if let Some(signs) = self.signs.signs.get_mut() {
                match (
                    matches!(state >> 4, 63 | 68),
                    signs.binary_search(&(index as u16)),
                ) {
                    (true, Err(at)) => signs.insert(at, index as u16),
                    (false, Ok(at)) => {
                        signs.remove(at);
                    }
                    _ => {}
                }
            }
        }
        if (previous >> 4 == 52) != (state >> 4 == 52) {
            if let Some(spawners) = self.signs.spawners.get_mut() {
                match (state >> 4 == 52, spawners.binary_search(&(index as u16))) {
                    (true, Err(at)) => spawners.insert(at, index as u16),
                    (false, Ok(at)) => {
                        spawners.remove(at);
                    }
                    _ => {}
                }
            }
        }
        if (previous >> 4 == 119) != (state >> 4 == 119) {
            if let Some(end_portals) = self.signs.end_portals.get_mut() {
                match (
                    state >> 4 == 119,
                    end_portals.binary_search(&(index as u16)),
                ) {
                    (true, Err(at)) => end_portals.insert(at, index as u16),
                    (false, Ok(at)) => {
                        end_portals.remove(at);
                    }
                    _ => {}
                }
            }
        }
        if (previous >> 4 == 90) != (state >> 4 == 90) {
            if let Some(portals) = self.signs.portals.get_mut() {
                match (state >> 4 == 90, portals.binary_search(&(index as u16))) {
                    (true, Err(at)) => portals.insert(at, index as u16),
                    (false, Ok(at)) => {
                        portals.remove(at);
                    }
                    _ => {}
                }
            }
        }
        if previous != state
            && self.light.is_some()
            && !self.light_edits.iter().any(|&(idx, _)| usize::from(idx) == index)
        {
            self.light_edits.push((index as u16, previous));
        }
        self.blocks[index] = state;
        if state != 0 {
            self.top = self.top.max(y + 1);
        }
    }
    /// Only the native ore adapter calls this, after its release-mode gate
    /// verifies the exact replacement whitelist and old stone/netherrack.
    /// The index and old value were just read under this same unique Chunk
    /// borrow; no callback or other write may intervene. World-level COW,
    /// lighting, journal, revision, provider and output ordering stay outside.
    #[inline]
    pub(crate) fn set_inert_ore_known(&mut self, index: usize, old: u16, state: u16) {
        debug_assert!(index < 65536);
        debug_assert_eq!(self.blocks[index], old);
        debug_assert!(matches!(old >> 4, 1 | 87));
        debug_assert!(inert_ore_replacement(state));
        // Generic Chunk::set preserves old-ID-equal tile data, even stale
        // metadata. Different admitted IDs cannot retain the old tile.
        // Runtime currently rejects target tiles before calling us, but keep
        // this behavior exact independently of that additional outer guard.
        if old >> 4 != state >> 4 && !self.tiles.is_empty() {
            let local = [(index & 15) as i32, (index >> 8) as i32, ((index >> 4) & 15) as i32];
            self.tiles.remove(&local);
        }
        // Both states are non-random-ticking, non-scheduled, non-hopper,
        // non-piston and non-spawner. Leave every derived index, including its
        // initialized/uninitialized status, exactly as generic set would.
        self.blocks[index] = state;
        self.top = self.top.max((index >> 8) as i32 + 1);
    }
}
fn scheduled_tick_candidate(state: u16) -> bool {
    matches!(state >> 4, 8 | 10 | 12 | 13 | 51 | 145) || (state >> 4 == 28 && state & 8 != 0)
}
#[inline]
fn full_visual_occluder(state: u16) -> bool {
    (crate::vanilla::material::opaque_cube(state) && !matches!(state >> 4, 18 | 161))
        || state == ((78 << 4) | 7)
}
#[cfg(test)]
#[path = "world_tick_index_tests.rs"]
mod tick_index_tests;
#[cfg(test)]
#[path = "world_inert_setter_tests.rs"]
mod inert_setter_tests;
#[derive(Clone)]
pub struct World {
    pub chunks: ChunkMap<Arc<Chunk>>,
    pub spawn: Vec3,
    pub label: String,
    pub save_path: PathBuf,
    pub edits: EditJournal,
    pub imported: bool,
    /// Advances when loaded terrain or its lighting input changes. Independent in world snapshots.
    pub light_revision: u64,
}
pub struct Metadata {
    pub spawn: [i32; 3],
    pub legacy: bool,
    pub player: Option<crate::player_save::PlayerSave>,
}

fn decompress(reader: impl Read, kind: u8) -> Result<Vec<u8>> {
    let mut decoder: Box<dyn Read> = match kind {
        1 => Box::new(GzDecoder::new(reader)),
        2 => Box::new(ZlibDecoder::new(reader)),
        3 => Box::new(reader),
        _ => bail!("Unsupported Anvil compression {kind}"),
    };
    let mut bytes = Vec::new();
    decoder
        .by_ref()
        .take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 32 * 1024 * 1024,
        "Uncompressed NBT exceeds 32 MiB"
    );
    Ok(bytes)
}
pub fn world_metadata(path: &Path) -> Result<Metadata> {
    let root = level_data(path)?;
    let data = root.get("Data").context("Missing level.dat Data")?;
    let spawn = ["SpawnX", "SpawnY", "SpawnZ"].map(|key| {
        data.get(key)
            .and_then(Tag::int)
            .unwrap_or(if key == "SpawnY" { 64 } else { 0 })
    });
    let version = data.get("DataVersion").and_then(Tag::int);
    Ok(Metadata {
        spawn,
        player: crate::player_save::PlayerSave::from_level(data),
        legacy: version.is_none_or(|v| v <= 99),
    })
}
pub fn level_data(path: &Path) -> Result<Tag> {
    let load = |name: &str| -> Result<Tag> {
        nbt::read(&decompress(File::open(path.join(name))?, 1)?)
    };
    load("level.dat").or_else(|primary| load("level.dat_old").with_context(||format!("Read level.dat: {primary}")))
}
fn load_chunk(root: &Path, pos: ChunkPos) -> Result<Option<Chunk>> {
    World::load_dimension_chunk(root, 0, pos)
}
impl World {
    pub fn load_dimension_chunk(
        root: &Path,
        dimension: i32,
        pos: ChunkPos,
    ) -> Result<Option<Chunk>> {
        let region_dir = match dimension {
            0 => root.join("region"),
            -1 => root.join("DIM-1").join("region"),
            1 => root.join("DIM1").join("region"),
            _ => return Ok(None),
        };
        let path = region_dir.join(format!(
            "r.{}.{}.mca",
            pos.0.div_euclid(32),
            pos.1.div_euclid(32)
        ));
        if !path.exists() {
            return Ok(None);
        }
        let mut file = File::open(path)?;
        let slot = (pos.0.rem_euclid(32) + pos.1.rem_euclid(32) * 32) as u64;
        file.seek(SeekFrom::Start(slot * 4))?;
        let mut location = [0; 4];
        file.read_exact(&mut location)?;
        let sector = u32::from_be_bytes([0, location[0], location[1], location[2]]) as u64;
        if sector == 0 {
            return Ok(None);
        }
        ensure!(
            sector >= 2 && location[3] > 0,
            "Invalid Anvil sector offset"
        );
        file.seek(SeekFrom::Start(sector * 4096))?;
        let mut size = [0; 4];
        file.read_exact(&mut size)?;
        let size = u32::from_be_bytes(size) as usize;
        ensure!(
            size > 1 && size + 4 <= location[3] as usize * 4096,
            "Invalid Anvil chunk length"
        );
        let mut kind = [0];
        file.read_exact(&mut kind)?;
        let mut data = vec![0; size - 1];
        file.read_exact(&mut data)?;
        let bytes = decompress(&data[..], kind[0])?;
        let root = nbt::read(&bytes)?;
        let level = root.get("Level").context("Not a legacy Anvil chunk")?;
        let sections = level
            .get("Sections")
            .and_then(Tag::list)
            .context("Missing legacy Sections")?;
        let mut chunk = Chunk::empty();
        if let Some(biomes) = level.get("Biomes").and_then(Tag::bytes) {
            ensure!(biomes.len() == 256, "Invalid legacy biome array length");
            chunk.biomes.copy_from_slice(biomes);
        }
        if let Some(entities) = level.get("Entities").and_then(Tag::list) {
            for (index, entity) in entities.iter().enumerate() {
                if let Some(entity) = crate::villagers::read_entity(entity, pos, index) {
                    chunk.entities.push(entity);
                }
            }
        }
        let mut top = 0i32;
        {
            let dst: &mut [u16] = &mut chunk.blocks;
            for section in sections {
                let sy = section
                    .get("Y")
                    .and_then(Tag::int)
                    .context("Missing section Y")?;
                ensure!((0..16).contains(&sy), "Invalid legacy section Y");
                let blocks = section
                    .get("Blocks")
                    .and_then(Tag::bytes)
                    .context("Palette/modern chunks are not 1.8.9 compatible")?;
                let meta = section
                    .get("Data")
                    .and_then(Tag::bytes)
                    .context("Missing block metadata")?;
                let add = section.get("Add").and_then(Tag::bytes);
                ensure!(
                    blocks.len() == 4096
                        && meta.len() == 2048
                        && add.is_none_or(|v| v.len() == 2048),
                    "Invalid block array length"
                );
                let out = &mut dst[(sy as usize) * 4096..(sy as usize) * 4096 + 4096];
                let mut max_nonzero_i: Option<usize> = None;
                if let Some(add) = add {
                    for i in 0..4096 {
                        let shift = (i & 1) * 4;
                        let id = blocks[i] as u16 | (((add[i / 2] >> shift) & 15) as u16) << 8;
                        if id != 0 {
                            let state = id << 4 | ((meta[i / 2] >> shift) & 15) as u16;
                            out[i] = state;
                            max_nonzero_i = Some(i);
                            if id == 154 {
                                chunk.hoppers.push([
                                    (i & 15) as i32,
                                    sy * 16 + (i >> 8) as i32,
                                    ((i >> 4) & 15) as i32,
                                ]);
                            } else if id == 52 {
                                let lx = (i & 15) as i32;
                                let y = sy * 16 + (i >> 8) as i32;
                                let lz = ((i >> 4) & 15) as i32;
                                if let Some(tile) = crate::vanilla::middle_pick::tile_snapshot(
                                    52,
                                    IVec3::new(lx, y, lz),
                                    None,
                                    None,
                                ) {
                                    chunk.tiles.insert([lx, y, lz], Arc::new(tile));
                                }
                            }
                        }
                    }
                } else {
                    for i in 0..4096 {
                        let id = blocks[i] as u16;
                        if id != 0 {
                            let shift = (i & 1) * 4;
                            let state = id << 4 | ((meta[i / 2] >> shift) & 15) as u16;
                            out[i] = state;
                            max_nonzero_i = Some(i);
                            if id == 154 {
                                chunk.hoppers.push([
                                    (i & 15) as i32,
                                    sy * 16 + (i >> 8) as i32,
                                    ((i >> 4) & 15) as i32,
                                ]);
                            } else if id == 52 {
                                let lx = (i & 15) as i32;
                                let y = sy * 16 + (i >> 8) as i32;
                                let lz = ((i >> 4) & 15) as i32;
                                if let Some(tile) = crate::vanilla::middle_pick::tile_snapshot(
                                    52,
                                    IVec3::new(lx, y, lz),
                                    None,
                                    None,
                                ) {
                                    chunk.tiles.insert([lx, y, lz], Arc::new(tile));
                                }
                            }
                        }
                    }
                }
                if let Some(last_i) = max_nonzero_i {
                    top = top.max(sy * 16 + (last_i >> 8) as i32 + 1);
                }
            }
        }
        chunk.top = top;
        if let Some(tiles) = level.get("TileEntities").and_then(Tag::list) {
            for tile in tiles {
                let [Some(x), Some(y), Some(z)] =
                    ["x", "y", "z"].map(|k| tile.get(k).and_then(Tag::int))
                else {
                    continue;
                };
                if x.div_euclid(16) != pos.0
                    || z.div_euclid(16) != pos.1
                    || !(0..HEIGHT).contains(&y)
                {
                    continue;
                }
                let key = [x.rem_euclid(16), y, z.rem_euclid(16)];
                let Some(kind) = tile.get("id").and_then(Tag::string) else {
                    continue;
                };
                let original_kind = match kind {
                    "minecraft:chest" => "Chest",
                    "minecraft:furnace" => "Furnace",
                    "minecraft:hopper" => "Hopper",
                    "minecraft:piston" => "Piston",
                    "minecraft:skull" => "Skull",
                    "minecraft:sign" => "Sign",
                    "minecraft:mob_spawner" => "MobSpawner",
                    "minecraft:end_portal" => "Airportal",
                    name => name,
                };
                if !crate::vanilla::middle_pick::compatible_tile(
                    chunk.get(key[0], key[1], key[2]) >> 4,
                    original_kind,
                ) {
                    continue;
                }
                chunk
                    .tiles
                    .insert(key, Arc::new(crate::vanilla::item_nbt::from_tag(tile)));
                if original_kind == "Piston" {
                    chunk.pistons.insert(
                        key,
                        crate::vanilla::collision_shapes::PistonTile::from_nbt(tile)?,
                    );
                    continue;
                }
                let Some(mut c) =
                    crate::container::Container::for_block(chunk.get(key[0], key[1], key[2]) >> 4)
                else {
                    continue;
                };
                c.transfer_cooldown = tile
                    .get("TransferCooldown")
                    .and_then(Tag::int)
                    .unwrap_or(0);
                if let Some(items) = tile.get("Items").and_then(Tag::list) {
                    for item in items {
                        let slot = item.get("Slot").and_then(Tag::int).unwrap_or(-1);
                        if let Some(slot) = c.slots.get_mut(slot as usize) {
                            *slot = crate::inventory::from_nbt(item);
                        }
                    }
                }
                c.burn = tile
                    .get("BurnTime")
                    .and_then(Tag::int)
                    .unwrap_or(0)
                    .clamp(0, 20000) as u32;
                c.burn_total = c.burn.max(1);
                c.cook = tile
                    .get("CookTime")
                    .and_then(Tag::int)
                    .unwrap_or(0)
                    .clamp(0, 199) as u32;
                chunk.containers.push((IVec3::new(x, y, z), c));
            }
        }
        // Preserve the original saved light arrays, including empty-but-allocated
        // sections. Recomputing them from block geometry loses pending light state.
        let mut light = crate::vanilla::lighting::LightChunk::default();
        if let Some(Tag::Ints(height)) = level.get("HeightMap") {
            ensure!(height.len() == 256, "Invalid legacy height map length");
            light.height.clone_from(height);
        } else {
            for z in 0..16 {
                for x in 0..16 {
                    light.height[(z * 16 + x) as usize] = (0..256)
                        .rev()
                        .find(|&y| crate::vanilla::material::light_opacity(chunk.get(x, y, z)) > 0)
                        .map_or(0, |y| y + 1);
                }
            }
        }
        for section in sections {
            let sy = section.get("Y").and_then(Tag::int).unwrap() as usize;
            light.sections[sy] = true;
            for (name, target) in [
                ("SkyLight", &mut light.sky),
                ("BlockLight", &mut light.block),
            ] {
                if let Some(values) = section.get(name).and_then(Tag::bytes) {
                    ensure!(values.len() == 2048, "Invalid legacy {name} array length");
                    for i in 0..4096 {
                        target[sy * 4096 + i] = (values[i / 2] >> ((i & 1) * 4)) & 15;
                    }
                }
            }
        }
        let no_sky = dimension != 0;
        chunk.light = Some(Arc::new(crate::vanilla::lighting::ChunkLight::from_chunk(
            &light, no_sky,
        )));
        Ok(Some(chunk))
    }
}
fn hash(x: i32, z: i32, seed: u64) -> u64 {
    let mut v = (x as u64).wrapping_mul(0x9e3779b97f4a7c15)
        ^ (z as u64).wrapping_mul(0xbf58476d1ce4e5b9)
        ^ seed;
    v = (v ^ (v >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    v = (v ^ (v >> 27)).wrapping_mul(0x94d049bb133111eb);
    v ^ (v >> 31)
}
pub fn terrain_height(x: i32, z: i32, seed: u64) -> i32 {
    let s = (seed % 1000) as f32;
    (53.0
        + ((x as f32 + s) * 0.021).sin() * 9.0
        + ((z as f32 - s) * 0.029).cos() * 7.0
        + ((x + z) as f32 * 0.071).sin() * 2.5) as i32
}
fn generate(pos: ChunkPos, seed: u64) -> Chunk {
    let mut chunk = Chunk::empty();
    for z in 0..16 {
        for x in 0..16 {
            let wx = pos.0 * 16 + x;
            let wz = pos.1 * 16 + z;
            let h = terrain_height(wx, wz, seed);
            for y in 0..=h.max(49) {
                let id = if y == 0 {
                    7
                } else if y > h {
                    9
                } else if y == h {
                    if h <= 50 {
                        12
                    } else {
                        2
                    }
                } else if y > h - 4 {
                    3
                } else if y < 16
                    && hash(wx.wrapping_add(y * 13), wz, seed ^ 0x1234).is_multiple_of(389)
                {
                    56
                } else if y < 32
                    && hash(wx.wrapping_add(y * 7), wz, seed ^ 0x7463).is_multiple_of(211)
                {
                    14
                } else if y < 64
                    && hash(wx.wrapping_add(y * 5), wz, seed ^ 0x9814).is_multiple_of(97)
                {
                    15
                } else if y < 16
                    && hash(wx.wrapping_add(y * 3), wz, seed ^ 0x8531).is_multiple_of(149)
                {
                    73
                } else if hash(wx + y, wz, seed).is_multiple_of(173) {
                    16
                } else {
                    1
                };
                chunk.set(x, y, z, id << 4);
            }
        }
    }
    // Evaluate trees in a padded area so leaf canopies agree at chunk seams.
    for wz in pos.1 * 16 - 3..pos.1 * 16 + 19 {
        for wx in pos.0 * 16 - 3..pos.0 * 16 + 19 {
            if !hash(wx, wz, seed).is_multiple_of(211) {
                continue;
            }
            let h = terrain_height(wx, wz, seed);
            if h < 52 {
                continue;
            }
            for dy in 1..=7 {
                for dz in -2i32..=2 {
                    for dx in -2i32..=2 {
                        let x = wx + dx - pos.0 * 16;
                        let z = wz + dz - pos.1 * 16;
                        if !(0..16).contains(&x) || !(0..16).contains(&z) {
                            continue;
                        }
                        let id = if dx == 0 && dz == 0 && dy <= 5 {
                            17
                        } else if dy >= 4 && dx.abs() + dz.abs() + (dy - 5i32).abs() <= 4 {
                            18
                        } else {
                            continue;
                        };
                        if chunk.get(x, h + dy, z) == 0 {
                            chunk.set(x, h + dy, z, id << 4);
                        }
                    }
                }
            }
        }
    }
    chunk
}
pub struct LoadedWorld {
    pub world: World,
    pub pose: Option<crate::player_save::PlayerSave>,
    pub simulation: Option<crate::gameplay::Simulation>,
}
/// Read-only handoff from the old terrain-only overlay into a separately saved populated world.
pub(crate) struct LegacyGeneration {
    pub pose: Option<crate::player_save::PlayerSave>,
    pub simulation: Option<crate::gameplay::Simulation>,
    pub edits: Vec<([i32; 3], u16)>,
}
#[derive(Serialize, Deserialize)]
struct Overlay<S = crate::gameplay::Simulation> {
    version: u32,
    source: String,
    edits: Vec<([i32; 3], u16)>,
    player: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    motion: Option<crate::player_save::Kinematics>,
    // Version 1 kept this in a second file. Version 2 commits a whole tick
    // with one rename, including the contents of blocks and the player's hand.
    simulation: Option<S>,
}
pub(crate) fn atomic_json(path: &Path, data: &impl Serialize) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let (temporary, file) = loop {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = path.with_extension(format!("{}-{sequence}.tmp", std::process::id()));
        match File::create_new(&temporary) {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    };
    let result = (|| -> Result<()> {
        let mut buffered = BufWriter::new(file);
        serde_json::to_writer(&mut buffered, data)?;
        let file = buffered.into_inner().map_err(|err| err.into_error())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}
pub(crate) fn overlay_key(source: &str) -> String {
    let key = source.bytes().fold(0xcbf29ce484222325u64, |key,b| (key ^ u64::from(b)).wrapping_mul(0x100000001b3));
    format!("{key:016x}")
}
fn generated_source(options: &Options) -> String {
    if options.generator == 1 {
        format!("demo-{}", options.seed)
    } else if options.generator == 3 {
        // A future populated generator needs a different identity: never add
        // structures/decorators silently to an existing terrain-only save.
        format!("native189-base1-{}", options.seed as i64)
    } else {
        format!("ferrum-v{}-{}", options.generator, options.seed)
    }
}
impl World {
    pub(crate) fn read_generation_migration(options: &Options) -> Result<Option<LegacyGeneration>> {
        if options.fresh || options.benchmark || options.world.is_some() {
            return Ok(None);
        }
        let mut legacy = options.clone();
        legacy.generator = 3;
        let source = generated_source(&legacy);
        let path = options.project_dir.join("saves").join(options.save_id.as_ref()
            .map(|id| format!("world-{id}.json"))
            .unwrap_or_else(|| format!("{}.json", overlay_key(&source))));
        if !path.is_file() { return Ok(None); }
        let overlay: Overlay = serde_json::from_reader(BufReader::new(File::open(&path)?))?;
        ensure!(matches!(overlay.version, 1 | 2) && overlay.source == source,
            "Existing terrain save does not match the population upgrade");
        ensure!(overlay.version == 1 || overlay.simulation.is_some(), "Incomplete version 2 save");
        let pose = overlay.player.filter(|p| crate::player_save::valid_position(*p))
            .map(|position| crate::player_save::PlayerSave { position, motion: overlay.motion });
        let mut simulation = overlay.simulation;
        if simulation.is_none() { simulation = crate::gameplay::Simulation::read_legacy_save(&path)?; }
        Ok(Some(LegacyGeneration { pose, simulation, edits: overlay.edits.into_iter()
            .map(|(p, s)| (p, crate::vanilla::collision_shapes::legacy_overlay_state(s))).collect() }))
    }
    pub fn tile(&self, p: IVec3) -> Option<&crate::vanilla::drops::DropNbt> {
        self.chunks
            .get(&(p.x.div_euclid(16), p.z.div_euclid(16)))?
            .tiles
            .get(&[p.x.rem_euclid(16), p.y, p.z.rem_euclid(16)])
            .map(Arc::as_ref)
    }
    pub fn generated_spawn(options: &Options) -> Result<Vec3> {
        if options.generator == 3 {
            crate::world_types::spawn(options)
        } else {
            Ok(crate::worldgen::spawn(
                options.seed,
                options.village && options.generator == 2,
            ))
        }
    }
    pub fn load(options: &Options) -> Result<LoadedWorld> {
        ensure!(options.world.is_some() || options.generator != 4 || options.structure_preview.is_some(),
            "Populated worlds require the retained native generation session");
        if let Some(kind) = &options.structure_preview {
            use crate::vanilla::generator::{self, PreviewKind};
            let kind = if kind == "village" {
                PreviewKind::Village
            } else {
                PreviewKind::Mineshaft
            };
            let pos = generator::nearest_structure_chunk(options.seed, kind)?;
            let mut preview = generator::canonical_structure_preview(options.seed, kind, pos)?;
            std::fs::create_dir_all("reports")?;
            std::fs::write(
                "reports/structure-preview.json",
                serde_json::to_vec_pretty(&serde_json::json!({
                    "seed":(options.seed as i64).to_string(),"chunk":pos,"render_cutaway":matches!(kind,PreviewKind::Mineshaft),
                    "scope":"Isolated original structure templates on native terrain; canonical population order, no other decorators or complete World callbacks. Not a full vanilla world.",
                    "structures":preview.structures,"villagers":preview.villagers,"mineshaft_entities":preview.mineshaft_entities
                }))?,
            )?;
            if matches!(kind, PreviewKind::Mineshaft) {
                // Only this isolated screenshot removes surrounding rock so
                // underground support/rail geometry can be inspected.
                let bounds = preview.structures.mineshafts[0].bounds;
                preview.world.spawn = Vec3::new(
                    (bounds.min[0] + bounds.max[0]) as f32 * 0.5,
                    (bounds.min[1] + bounds.max[1]) as f32 * 0.5,
                    (bounds.min[2] + bounds.max[2]) as f32 * 0.5,
                );
                preview.world.label.push_str(" / rock cutaway");
                for chunk in preview.world.chunks.values_mut() {
                    for state in &mut Arc::make_mut(chunk).blocks {
                        if !matches!(*state >> 4, 5 | 30 | 50 | 52 | 66 | 85) {
                            *state = 0;
                        }
                    }
                }
            }
            return Ok(LoadedWorld {
                world: preview.world,
                pose: None,
                simulation: None,
            });
        }
        let radius = options.render_distance as i32;
        let (spawn, label, imported, mut pose) = if let Some(path) = &options.world {
            let meta = world_metadata(path)?;
            ensure!(meta.legacy,"This save is newer than 1.8.9. Open a legacy 1.8.9 save; no conversion is performed.");
            (
                IVec3::from_array(meta.spawn).as_vec3(),
                format!(
                    "Imported: {}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ),
                true,
                meta.player,
            )
        } else {
            (
                Self::generated_spawn(options)?,
                if options.generator == 3 {
                    format!(
                        "Native 1.8.9 terrain preview / seed {} / population pending",
                        options.seed as i64
                    )
                } else {
                    format!(
                        "Ferrum terrain v{} / seed {}",
                        options.generator, options.seed
                    )
                },
                false,
                None,
            )
        };
        let source = if let Some(path) = &options.world {
            std::fs::canonicalize(path)?.to_string_lossy().into_owned()
        } else {
            generated_source(options)
        };
        let key = overlay_key(&source);
        let save_path = options.project_dir.join("saves").join(
            options
                .save_id
                .as_ref()
                .map(|id| format!("world-{id}.json"))
                .unwrap_or_else(|| format!("{key}.json")),
        );
        let overlay: Option<Overlay> =
            if !options.fresh && !options.benchmark && save_path.is_file() {
                let overlay: Overlay =
                    serde_json::from_reader(BufReader::new(File::open(&save_path)?))?;
                ensure!(
                    matches!(overlay.version, 1 | 2) && overlay.source == source,
                    "Overlay does not match this world"
                );
                ensure!(
                    overlay.version == 1 || overlay.simulation.is_some(),
                    "Incomplete version 2 save"
                );
                if let Some(position) = overlay
                    .player
                    .filter(|p| crate::player_save::valid_position(*p))
                {
                    pose = Some(crate::player_save::PlayerSave {
                        position,
                        motion: overlay.motion,
                    });
                }
                Some(overlay)
            } else {
                None
            };
        let active_dim = overlay
            .as_ref()
            .and_then(|o| o.simulation.as_ref().map(|s| s.dimension))
            .or_else(|| pose.and_then(|p| p.motion.map(|m| m.dimension)))
            .filter(|&d| matches!(d, -1 | 0 | 1))
            .unwrap_or(0);
        let position = if active_dim != 0 {
            spawn.as_dvec3()
        } else {
            pose.map_or(spawn.as_dvec3(), |p| glam::DVec3::from_array(p.position))
        };
        let center = (
            (position.x.floor() as i32).div_euclid(16),
            (position.z.floor() as i32).div_euclid(16),
        );
        let positions = (-radius..=radius)
            .flat_map(|z| (-radius..=radius).map(move |x| (center.0 + x, center.1 + z)))
            .collect::<Vec<_>>();
        let mut chunks = Self::read_chunks(options, &positions)?
            .into_iter()
            .collect::<ChunkMap<_>>();
        let mut used_dim_fallback = false;
        if chunks.is_empty() && active_dim != 0 {
            let p_pos = pose.map_or(spawn.as_dvec3(), |p| glam::DVec3::from_array(p.position));
            let p_center = (
                (p_pos.x.floor() as i32).div_euclid(16),
                (p_pos.z.floor() as i32).div_euclid(16),
            );
            let dim_positions = (-radius..=radius)
                .flat_map(|z| (-radius..=radius).map(move |x| (p_center.0 + x, p_center.1 + z)))
                .collect::<Vec<_>>();
            chunks = crate::dimension_world::read_dimension_chunks(
                active_dim,
                options,
                &std::collections::BTreeMap::new(),
                &dim_positions,
            )?
            .into_iter()
            .collect::<ChunkMap<_>>();
            used_dim_fallback = !chunks.is_empty();
        }
        ensure!(
            !chunks.is_empty(),
            "No legacy chunks found near player entry at {center:?}"
        );
        let mut world = Self {
            chunks,
            spawn,
            label,
            save_path,
            edits: Default::default(),
            imported,
            light_revision: 0,
        };
        let mut saved_simulation = None;
        if let Some(overlay) = overlay {
            saved_simulation = overlay.simulation;
            for (position, state) in overlay.edits {
                let state = crate::vanilla::collision_shapes::legacy_overlay_state(state);
                world.edits.insert(position, state);
                if !used_dim_fallback {
                    world.set_raw(IVec3::from_array(position), state);
                    if let Some(chunk) = world
                        .chunks
                        .get_mut(&(position[0].div_euclid(16), position[2].div_euclid(16)))
                    {
                        let chunk = Arc::make_mut(chunk);
                        chunk.light = None;
                        chunk.visual_light = None;
                    }
                }
            }
        }
        // Only lift a newly chosen spawn. A saved player may be midair or
        // partly inside a block, and already owns an exact position.
        if pose.is_none() {
            let mut feet = if imported {
                spawn + Vec3::new(0.5, 0., 0.5)
            } else {
                spawn
            };
            for _ in 0..HEIGHT {
                if !assets::solid(world.get(feet.floor().as_ivec3()))
                    && !assets::solid(world.get((feet + Vec3::Y).floor().as_ivec3()))
                {
                    break;
                }
                feet.y += 1.0;
            }
            pose = Some(crate::player_save::PlayerSave {
                position: feet.as_dvec3().to_array(),
                motion: None,
            });
        }
        if saved_simulation.is_none() && !options.fresh && !options.benchmark {
            saved_simulation = crate::gameplay::Simulation::read_legacy_save(&world.save_path)?;
        }
        if let Some(s)=saved_simulation.as_ref(){
            if let Some(p)=s.world_commands.spawn{world.spawn=IVec3::from_array(p).as_vec3();}
            let loaded=world.chunks.keys().copied().collect::<Vec<_>>();
            s.world_commands.restore_tiles(&mut world,&loaded);
        }
        Ok(LoadedWorld {
            world,
            pose,
            simulation: saved_simulation,
        })
    }
    pub fn read_chunks(
        options: &Options,
        positions: &[ChunkPos],
    ) -> Result<Vec<(ChunkPos, Arc<Chunk>)>> {
        ensure!(options.world.is_some() || options.generator != 4,
            "Populated chunks must be requested from the retained native generation session");
        if options.world.is_none() && options.generator == 3 {
            return crate::world_types::read_chunks(options, positions);
        }
        let results = positions
            .par_iter()
            .map(|&pos| {
                let chunk = if let Some(path) = &options.world {
                    load_chunk(path, pos).with_context(|| format!("Chunk {},{}", pos.0, pos.1))?
                } else {
                    let mut chunk = generate(pos, options.seed);
                    if options.generator == 2 {
                        crate::worldgen::decorate(&mut chunk, pos, options.seed);
                    }
                    Some(chunk)
                };
                Ok(chunk.map(|c| (pos, Arc::new(c))))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(results.into_iter().flatten().collect())
    }
    pub fn integrate(&mut self, chunks: Vec<(ChunkPos, Arc<Chunk>)>) -> Vec<ChunkPos> {
        let mut incoming = chunks
            .into_iter()
            .filter(|(p, _)| !self.chunks.contains_key(p))
            .collect::<HashMap<_, _>>();
        for (p, state) in &self.edits {
            if !(0..HEIGHT).contains(&p[1]) {
                continue;
            }
            if let Some(chunk) = incoming.get_mut(&(p[0].div_euclid(16), p[2].div_euclid(16))) {
                let chunk = Arc::make_mut(chunk);
                chunk.set(p[0].rem_euclid(16), p[1], p[2].rem_euclid(16), *state);
                chunk.light = None;
                chunk.visual_light = None;
            }
        }
        let positions = incoming.keys().copied().collect();
        if !incoming.is_empty() {
            self.invalidate_lighting();
        }
        self.chunks.extend(incoming);
        positions
    }
    pub fn invalidate_lighting(&mut self) {
        self.light_revision = self.light_revision.wrapping_add(1);
    }
    pub fn unload_chunk(&mut self, pos: ChunkPos) {
        if self.chunks.remove(&pos).is_some() {
            self.invalidate_lighting();
        }
    }
    fn source_id(&self, options: &Options) -> Result<String> {
        Ok(if let Some(path) = &options.world {
            std::fs::canonicalize(path)?.to_string_lossy().into_owned()
        } else {
            generated_source(options)
        })
    }
    pub fn get(&self, p: IVec3) -> u16 {
        if !(0..HEIGHT).contains(&p.y) {
            return 0;
        }
        self.chunks
            .get(&(p.x.div_euclid(16), p.z.div_euclid(16)))
            .map(|c| c.get(p.x.rem_euclid(16), p.y, p.z.rem_euclid(16)))
            .unwrap_or(0)
    }
    pub(crate) fn can_see_sky(&self, p: IVec3) -> bool {
        let Some(chunk) = self.chunks.get(&(p.x >> 4, p.z >> 4)) else { return false };
        if let Some(light) = &chunk.light {
            return p.y >= light.height[((p.z & 15) * 16 + (p.x & 15)) as usize];
        }
        (p.y.max(0)..chunk.top).all(|y| crate::vanilla::material::light_opacity(chunk.get(p.x & 15, y, p.z & 15)) == 0)
    }
    pub(crate) fn rain_exposed(&self, p: IVec3) -> bool {
        use crate::vanilla::material;
        self.can_see_sky(p)
            && crate::vanilla::runtime::rain_in_biome(crate::assets::biome_colors::biome_at(self,p), p)
            && (p.y.max(1)..self.chunks.get(&(p.x >> 4, p.z >> 4)).map_or(0, |c| c.top))
                .all(|y| {
                    let s = self.get(IVec3::new(p.x, y, p.z));
                    !material::blocks_movement(s) && !material::is_liquid(s)
                })
    }
    pub fn contains(&self, p: Vec3) -> bool {
        self.chunks.contains_key(&(
            (p.x.floor() as i32).div_euclid(16),
            (p.z.floor() as i32).div_euclid(16),
        ))
    }
    pub fn piston_at(&self, p: IVec3) -> Option<crate::vanilla::collision_shapes::PistonTile> {
        self.chunks
            .get(&(p.x.div_euclid(16), p.z.div_euclid(16)))
            .and_then(|chunk| {
                chunk
                    .pistons
                    .get(&[p.x.rem_euclid(16), p.y, p.z.rem_euclid(16)])
            })
            .copied()
    }
    /// World.getLight(pos) and getLightFromNeighbors(pos) use Chunk.getLightSubtracted.
    /// In an unallocated section this lookup depends on the dimension, even below the heightmap.
    pub(crate) fn game_light(&self, p: IVec3, subtraction: u8, neighbors: bool, has_sky: bool) -> u8 {
        use crate::vanilla::{lighting::Kind, material};
        if neighbors && material::neighbor_brightness(self.get(p)) {
            return [IVec3::Y, IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z]
                .into_iter().map(|d| self.game_light(p + d, subtraction, false, has_sky)).max().unwrap();
        }
        if p.y < 0 { return 0; }
        let p = IVec3::new(p.x, p.y.min(255), p.z);
        let light = self.chunks.get(&(p.x >> 4, p.z >> 4)).and_then(|c| c.light.as_ref());
        let sky = has_sky && light.is_none_or(|l| !l.no_sky);
        let Some(light) = light.filter(|l| l.sections[p.y as usize >> 4]) else {
            return if sky { 15u8.saturating_sub(subtraction) } else { 0 };
        };
        let sky_value = if sky { light.get(Kind::Sky, p).saturating_sub(subtraction) } else { 0 };
        sky_value.max(light.get(Kind::Block, p))
    }
    /// Original getCombinedLight coordinates, in sixteenths of a light level.
    pub fn light_uv(&self, mut p: IVec3, minimum_block: u8) -> [u8; 4] {
        use crate::vanilla::{lighting::Kind, material};
        p.y = p.y.max(0);
        let light_at = |at: IVec3, kind: Kind| {
            self.chunks
                .get(&(at.x.div_euclid(16), at.z.div_euclid(16)))
                .and_then(|chunk| chunk.light.as_ref())
                .map_or(if kind == Kind::Sky { 15 } else { 0 }, |light| {
                    light.get(kind, at)
                })
        };
        let no_sky = self
            .chunks
            .get(&(p.x.div_euclid(16), p.z.div_euclid(16)))
            .and_then(|c| c.light.as_ref())
            .is_some_and(|l| l.no_sky);
        let query = |kind: Kind| {
            if no_sky && kind == Kind::Sky {
                return 0;
            }
            if material::neighbor_brightness(self.get(p)) {
                [IVec3::Y, IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z]
                    .into_iter()
                    .map(|d| light_at(p + d, kind))
                    .max()
                    .unwrap()
            } else {
                light_at(p, kind)
            }
        };
        [
            query(Kind::Block).max(minimum_block).min(15) * 16,
            query(Kind::Sky) * 16,
            0,
            0,
        ]
    }
    pub fn visual_light_uv(&self, mut p: IVec3, minimum_block: u8) -> [u8; 4] {
        use crate::vanilla::{ambient_occlusion::DIRECTIONS, lighting::Kind, material};
        p.y = p.y.max(0);
        let chunk_at = |at: IVec3| self.chunks.get(&(at.x.div_euclid(16), at.z.div_euclid(16)));
        let light_at = |at: IVec3, kind: Kind| {
            if at.y < 0 {
                return 0;
            }
            chunk_at(at)
                .and_then(|chunk| chunk.visual_light.as_ref().or(chunk.light.as_ref()))
                .map_or(if kind == Kind::Sky { 15 } else { 0 }, |light| {
                    light.get(kind, at)
                })
        };
        let p_chunk = chunk_at(p);
        let p_light = p_chunk.and_then(|c| c.visual_light.as_ref().or(c.light.as_ref()));
        let no_sky = p_light.is_some_and(|l| l.no_sky);
        let has_visual_light = p_chunk.is_some_and(|c| c.visual_light.is_some());
        let state = self.get(p);
        let query = |kind: Kind| {
            if no_sky && kind == Kind::Sky {
                return 0;
            }
            if crate::visual_light::is_partial_light_block(state >> 4) {
                let own = light_at(p, kind);
                if has_visual_light {
                    own
                } else {
                    (0..6)
                        .map(|f| {
                            let q = p + DIRECTIONS[f];
                            let mut v = if crate::visual_light::aperture_open(self, p, f, q) {
                                light_at(q, kind).saturating_sub(1)
                            } else {
                                0
                            };
                            if kind == Kind::Block
                                && crate::visual_light::face_mask(self, p, f) != 0b1111
                            {
                                v = v.max(material::light_value(self.get(q)).saturating_sub(1));
                            }
                            v
                        })
                        .fold(own, u8::max)
                }
            } else if material::neighbor_brightness(state) {
                [
                    IVec3::ZERO,
                    IVec3::Y,
                    IVec3::NEG_Y,
                    IVec3::X,
                    IVec3::NEG_X,
                    IVec3::Z,
                    IVec3::NEG_Z,
                ]
                .into_iter()
                .map(|d| light_at(p + d, kind))
                .max()
                .unwrap()
            } else {
                light_at(p, kind)
            }
        };
        [
            query(Kind::Block).max(minimum_block).min(15) * 16,
            query(Kind::Sky) * 16,
            0,
            0,
        ]
    }
    /// Write blocks and their tile/index/light state without recording a sparse
    /// gameplay overlay. Native owners persist complete chunk snapshots.
    pub(crate) fn set_raw(&mut self, p: IVec3, state: u16) -> bool {
        if !(0..HEIGHT).contains(&p.y) {
            return false;
        }
        if let Some(chunk) = self
            .chunks
            .get_mut(&(p.x.div_euclid(16), p.z.div_euclid(16)))
        {
            let x = p.x.rem_euclid(16);
            let z = p.z.rem_euclid(16);
            if chunk.get(x, p.y, z) != state {
                let chunk = Arc::make_mut(chunk);
                chunk.set(x, p.y, z, state);
                if state >> 4 == 52 {
                    use crate::vanilla::drops::DropNbt as Tag;
                    if let Some(tile) = chunk.tiles.get_mut(&[x,p.y,z]) {
                        if let Tag::Compound(fields) = Arc::make_mut(tile) {
                            fields.insert("x".into(), Tag::Int(p.x));
                            fields.insert("z".into(), Tag::Int(p.z));
                        }
                    }
                }
                if matches!(state >> 4, 176 | 177) && !chunk.tiles.contains_key(&[x,p.y,z]) {
                    if let Some(tag) = crate::vanilla::middle_pick::tile_snapshot(state >> 4,p,None,None) {
                        chunk.tiles.insert([x,p.y,z], Arc::new(tag));
                    }
                }
                self.invalidate_lighting();
            }
            true
        } else {
            false
        }
    }
    pub fn set(&mut self, p: IVec3, state: u16) -> bool {
        if self.set_raw(p, state) {
            self.edits.insert(p.to_array(), state);
            true
        } else {
            false
        }
    }
    pub fn save(
        &self,
        options: &Options,
        player: crate::player_save::PlayerSave,
        simulation: &crate::gameplay::Simulation,
    ) -> Result<()> {
        let owned_sim;
        let (sim_ref, mut edits) = if simulation.dimension != 0 {
            let mut snap = simulation.clone();
            snap.snapshot_active_dimension_for_save(self);
            let edits = snap.overworld_saved_edits(self);
            owned_sim = snap;
            (&owned_sim, edits)
        } else {
            let edits = self.edits.iter().map(|(p, s)| (*p, *s)).collect::<Vec<_>>();
            (simulation, edits)
        };
        edits.sort_by_key(|(p, _)| *p);
        let data = Overlay {
            version: 2,
            source: self.source_id(options)?,
            edits,
            player: Some(player.position),
            motion: player.motion,
            simulation: Some(sim_ref),
        };
        atomic_json(&self.save_path, &data)
    }
    #[cfg(test)]
    pub fn raycast(&self, origin: Vec3, direction: Vec3, reach: f32) -> Option<(IVec3, IVec3)> {
        use crate::vanilla::ray_trace::{self, Kind, Options};
        let start = origin.as_dvec3();
        ray_trace::trace(
            self,
            start,
            start + direction.as_dvec3() * reach as f64,
            Options::PICK,
        )
        .filter(|hit| hit.kind == Kind::Block)
        .map(|hit| (hit.position, hit.adjacent()))
    }
    /// ItemBucket uses stopOnLiquid: only fluid sources can collide with this ray.
    #[cfg(test)]
    pub fn raycast_liquids(
        &self,
        origin: Vec3,
        direction: Vec3,
        reach: f32,
    ) -> Option<(IVec3, IVec3)> {
        use crate::vanilla::ray_trace::{self, Options};
        let start = origin.as_dvec3();
        ray_trace::trace(
            self,
            start,
            start + direction.as_dvec3() * reach as f64,
            Options::item(true),
        )
        .map(|hit| (hit.position, hit.adjacent()))
    }
    pub fn occluded(&self, from: Vec3, to: Vec3) -> bool {
        let delta = to - from;
        let distance = delta.length();
        distance > 0.01
            && self
                .trace(
                    from,
                    delta / distance,
                    (distance - 0.05).max(0.0),
                    true,
                    false,
                )
                .is_some()
    }
    pub(crate) fn visual_occluded(&self, from: Vec3, to: Vec3) -> bool {
        let delta = to - from;
        let distance = delta.length();
        distance > 0.01
            && self.trace_visual_occluder(from, delta / distance, (distance - 0.05).max(0.0))
    }
    fn trace_visual_occluder(&self, origin: Vec3, direction: Vec3, reach: f32) -> bool {
        let mut block = origin.floor().as_ivec3();
        let step = direction.signum().as_ivec3();
        let delta = Vec3::new(
            if direction.x == 0.0 {
                f32::INFINITY
            } else {
                1.0 / direction.x.abs()
            },
            if direction.y == 0.0 {
                f32::INFINITY
            } else {
                1.0 / direction.y.abs()
            },
            if direction.z == 0.0 {
                f32::INFINITY
            } else {
                1.0 / direction.z.abs()
            },
        );
        let mut next = Vec3::ZERO;
        for axis in 0..3 {
            next[axis] = if direction[axis] == 0.0 {
                f32::INFINITY
            } else {
                ((block[axis] as f32 + if step[axis] > 0 { 1.0 } else { 0.0 }) - origin[axis])
                    / direction[axis]
            };
        }
        let mut column_position = None;
        let mut column: Option<&Chunk> = None;
        for _ in 0..512 {
            let state = if !(0..HEIGHT).contains(&block.y) {
                0
            } else {
                let at = (block.x.div_euclid(16), block.z.div_euclid(16));
                if column_position != Some(at) {
                    column_position = Some(at);
                    column = self.chunks.get(&at).map(Arc::as_ref);
                }
                column.map_or(0, |chunk| {
                    chunk.get(block.x.rem_euclid(16), block.y, block.z.rem_euclid(16))
                })
            };
            if full_visual_occluder(state) {
                return true;
            }
            let axis = if next.x < next.y && next.x < next.z {
                0
            } else if next.y < next.z {
                1
            } else {
                2
            };
            if next[axis] > reach {
                break;
            }
            block[axis] += step[axis];
            next[axis] += delta[axis];
        }
        false
    }
    fn trace(
        &self,
        origin: Vec3,
        direction: Vec3,
        reach: f32,
        opaque_only: bool,
        stop_on_liquid: bool,
    ) -> Option<(IVec3, IVec3)> {
        let mut block = origin.floor().as_ivec3();
        let step = direction.signum().as_ivec3();
        let delta = Vec3::new(
            if direction.x == 0.0 {
                f32::INFINITY
            } else {
                1.0 / direction.x.abs()
            },
            if direction.y == 0.0 {
                f32::INFINITY
            } else {
                1.0 / direction.y.abs()
            },
            if direction.z == 0.0 {
                f32::INFINITY
            } else {
                1.0 / direction.z.abs()
            },
        );
        let mut next = Vec3::ZERO;
        for axis in 0..3 {
            next[axis] = if direction[axis] == 0.0 {
                f32::INFINITY
            } else {
                ((block[axis] as f32 + if step[axis] > 0 { 1.0 } else { 0.0 }) - origin[axis])
                    / direction[axis]
            };
        }
        let mut previous = block;
        // Only this read-only visual ray uses the cache. Cache absence as well
        // as presence; no gameplay callback or mutation can invalidate this borrow.
        let mut column_position = None;
        let mut column: Option<&Chunk> = None;
        for _ in 0..512 {
            let state = if !(0..HEIGHT).contains(&block.y) { 0 } else {
                let at = (block.x.div_euclid(16), block.z.div_euclid(16));
                if column_position != Some(at) {
                    column_position = Some(at);
                    column = self.chunks.get(&at).map(Arc::as_ref);
                }
                column.map_or(0, |chunk| chunk.get(
                    block.x.rem_euclid(16), block.y, block.z.rem_euclid(16)))
            };
            if if opaque_only {
                assets::solid(state) && !assets::see_through(state)
            } else {
                state != 0 && (!matches!(state >> 4, 8..=11) || (stop_on_liquid && state & 15 == 0))
            } {
                return Some((block, previous));
            }
            let axis = if next.x < next.y && next.x < next.z {
                0
            } else if next.y < next.z {
                1
            } else {
                2
            };
            if next[axis] > reach {
                break;
            }
            previous = block;
            block[axis] += step[axis];
            next[axis] += delta[axis];
        }
        None
    }
}
#[cfg(test)]
#[path="world_trace_cache_tests.rs"]
mod trace_cache_tests;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mining_ignores_fluids_but_buckets_target_only_sources() {
        let mut w = empty();
        w.set(IVec3::new(3, 64, 3), 9 << 4);
        w.set(IVec3::new(4, 64, 3), (8 << 4) | 3);
        w.set(IVec3::new(5, 64, 3), 1 << 4);
        let origin = Vec3::new(1.5, 64.5, 3.5);
        assert_eq!(
            w.raycast(origin, Vec3::X, 6.).unwrap().0,
            IVec3::new(5, 64, 3)
        );
        assert_eq!(
            w.raycast_liquids(origin, Vec3::X, 6.).unwrap().0,
            IVec3::new(3, 64, 3)
        );
        w.set(IVec3::new(3, 64, 3), (10 << 4) | 4);
        assert_eq!(
            w.raycast_liquids(origin, Vec3::X, 6.).unwrap().0,
            IVec3::new(5, 64, 3)
        );
    }
    fn empty() -> World {
        let mut chunks = crate::world::ChunkMap::new();
        chunks.insert((-1, 0), Arc::new(Chunk::empty()));
        chunks.insert((0, 0), Arc::new(Chunk::empty()));
        World {
            chunks,
            spawn: Vec3::ZERO,
            label: String::new(),
            save_path: PathBuf::new(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        }
    }
    #[test]
    fn negative_coordinates_and_snapshot_isolation() {
        let mut w = empty();
        let snapshot = w.clone();
        assert!(w.set(IVec3::new(-1, 64, 0), 16));
        assert_eq!(w.get(IVec3::new(-1, 64, 0)), 16);
        assert_eq!(snapshot.get(IVec3::new(-1, 64, 0)), 0);
        assert!(!w.set(IVec3::new(0, 256, 0), 16));
    }
    #[test]
    fn middle_pick_import_validates_tiles_and_reads_live_container_slots() {
        use crate::{
            gameplay::{self, Simulation},
            inventory::{Inventory, Stack},
            vanilla::{drops::DropNbt, item_nbt, middle_pick},
        };
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/middle-pick-import");
        let c = load_chunk(&path, (-1, 1)).unwrap().unwrap();
        assert_eq!(c.tiles.len(), 4);
        assert_eq!(c.containers.len(), 2);
        assert!(!c.tiles.contains_key(&[15, 63, 3])); // stone with a mismatched Chest tile
        let chest = IVec3::new(-3, 63, 19);
        let mut w = empty();
        w.chunks.insert((-1, 1), Arc::new(c));
        let mut sim = Simulation::default();
        sim.import_containers(&w);
        let container = sim
            .containers
            .get_mut(&gameplay::container_key(chest))
            .unwrap();
        assert_eq!(container.slots[4].as_ref().unwrap().id, 260);
        container.slots[4] = Some(Stack::new(264, 9));
        let mut inv = Inventory::default();
        assert!(middle_pick::block_click(
            &mut inv,
            &w,
            chest,
            true,
            true,
            Some(container),
            w.tile(chest)
        ));
        let tag = item_nbt::compound(inv.held().as_ref().unwrap().nbt.as_deref().unwrap())
            .unwrap()
            .clone();
        let tile = item_nbt::compound(&tag["BlockEntityTag"]).unwrap();
        let DropNbt::List { values, .. } = &tile["Items"] else {
            panic!("missing chest items")
        };
        let diamond = item_nbt::compound(&values[0]).unwrap();
        assert_eq!(diamond["id"], DropNbt::String("minecraft:diamond".into()));
        assert_eq!(diamond["Count"], DropNbt::Byte(9));
        assert_eq!(diamond["Slot"], DropNbt::Byte(4));
        let old = w.clone();
        assert!(w.set(chest, 16));
        assert!(w.tile(chest).is_none());
        assert!(old.tile(chest).is_some());
        let furnace = IVec3::new(-2, 63, 19);
        assert!(w.set(furnace, (62 << 4) | 4));
        assert!(w.tile(furnace).is_some());
    }
    #[test]
    fn anvil_preserves_allocated_light_nibbles_and_saved_height_map() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lighting-import");
        let chunk = load_chunk(&root, (-1, -1)).unwrap().unwrap();
        let light = chunk.light.as_ref().unwrap();
        assert!(light.sections[4]);
        assert_eq!(light.sections.iter().filter(|&&v| v).count(), 1);
        assert_eq!(
            light.height,
            (0..256).map(|i| 70 + i % 4).collect::<Vec<_>>()
        );
        assert_eq!(light.values[4 * 4096], 0x52);
        assert_eq!(light.values[4 * 4096 + 1], 0x74);
        use crate::vanilla::lighting::Kind;
        assert_eq!(light.get(Kind::Sky, IVec3::new(-16, 32, -16)), 0);
        assert_eq!(light.get(Kind::Sky, IVec3::new(-16, 96, -16)), 15);
        assert_eq!(light.get(Kind::Block, IVec3::new(-15, 65, -16)), 4);
        assert_eq!(light.get(Kind::Sky, IVec3::new(-15, 65, -16)), 7);
    }
    #[test]
    fn original_anvil_piston_tile_drives_collision_and_is_removed_on_replacement() {
        use crate::vanilla::movement::MovementWorld;
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/piston-import");
        let chunk = load_chunk(&root, (-1, -1)).unwrap().unwrap();
        let tile = chunk.pistons.get(&[15, 65, 14]).unwrap();
        assert_eq!(
            (tile.carried, tile.facing, tile.extending, tile.progress),
            (35 << 4 | 14, 5, true, 0.5)
        );
        let mut world = empty();
        world.chunks.insert((-1, -1), Arc::new(chunk));
        let p = IVec3::new(-1, 65, -2);
        let mut boxes = Vec::new();
        world.block_boxes(p, world.get(p), &mut boxes);
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].min.to_array(), [-1., 65., -2.]);
        assert_eq!(boxes[0].max.to_array(), [-0.5, 66., -1.]);
        let previous = world.clone();
        world.set(p, 16);
        assert!(world.chunks[&(-1, -1)].pistons.is_empty());
        assert_eq!(previous.chunks[&(-1, -1)].pistons.len(), 1);
    }
    #[test]
    fn dda_axis_aligned_and_reach() {
        let mut w = empty();
        w.set(IVec3::new(0, 64, 4), 16);
        assert_eq!(
            w.raycast(Vec3::new(0.5, 64.5, 0.5), Vec3::Z, 5.0),
            Some((IVec3::new(0, 64, 4), IVec3::new(0, 64, 3)))
        );
        assert!(w.raycast(Vec3::new(0.5, 64.5, 0.5), Vec3::Z, 2.0).is_none());
    }
    #[test]
    fn streaming_reapplies_edits_and_preserves_existing_chunks() {
        let mut w = empty();
        let p = IVec3::new(-1, 64, 2);
        w.set(p, 35 << 4 | 14);
        w.chunks.remove(&(-1, 0));
        assert_eq!(
            w.integrate(vec![((-1, 0), Arc::new(Chunk::empty()))]),
            vec![(-1, 0)]
        );
        assert_eq!(w.get(p), 35 << 4 | 14);
        w.set(p, 20 << 4);
        assert!(w
            .integrate(vec![((-1, 0), Arc::new(Chunk::empty()))])
            .is_empty());
        assert_eq!(w.get(p), 20 << 4);
    }
    #[test]
    fn legacy_sidecar_migrates_on_save_and_partial_writes_cannot_mix_ticks() {
        use crate::gameplay::Simulation;
        use clap::Parser;
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let id = format!("atomic-test-{}-{unique}", std::process::id());
        let options = Options::parse_from([
            "ferrum189",
            "--generator",
            "1",
            "--render-distance",
            "2",
            "--save-id",
            &id,
        ]);
        let mut w = World::load(&options).unwrap().world;
        std::fs::create_dir_all(w.save_path.parent().unwrap()).unwrap();
        let p = IVec3::new(-1, 64, 2);
        let pose = |y| crate::player_save::PlayerSave {
            position: [-0.123456789012345, y, 2.5],
            motion: None,
        };
        let old = serde_json::json!({"version":1,"source":generated_source(&options),"edits":[[p.to_array(),16]],"player":pose(65.).position});
        let mut sim = Simulation::default();
        sim.food = 13;
        sim.inventory.slots[0] = Some(crate::inventory::Stack::new(4, 8));
        let legacy = w.save_path.with_extension("player.json");
        std::fs::write(&w.save_path, serde_json::to_vec(&old).unwrap()).unwrap();
        std::fs::write(&legacy, serde_json::to_vec(&sim).unwrap()).unwrap();
        let loaded = World::load(&options).unwrap();
        assert_eq!(loaded.simulation.unwrap().food, 13);
        assert_eq!(loaded.world.get(p), 16);
        w = loaded.world;
        assert!(w.set(p, 4 << 4));
        sim.food = 17;
        sim.inventory.slots[0].as_mut().unwrap().count = 7;
        w.save(&options, pose(66.0), &sim).unwrap();
        // A stale or damaged legacy sidecar and an interrupted staging file
        // must never supersede the committed v2 record.
        std::fs::write(&legacy, b"{").unwrap();
        let partial = w.save_path.with_extension("powercut.tmp");
        std::fs::write(&partial, b"{\"version\":2").unwrap();
        let before = std::fs::read(&w.save_path).unwrap();
        // Non-string map keys force a real Serde error during a subsequent write.
        assert!(atomic_json(&w.save_path, &HashMap::from([([1, 2], 3)])).is_err());
        assert_eq!(std::fs::read(&w.save_path).unwrap(), before);
        let loaded = World::load(&options).unwrap();
        assert_eq!(loaded.pose.unwrap().position, pose(66.).position);
        let sim = loaded.simulation.unwrap();
        assert_eq!(sim.food, 17);
        assert_eq!(sim.inventory.slots[0].as_ref().unwrap().count, 7);
        assert_eq!(loaded.world.get(p), 4 << 4);
        let invalid = serde_json::json!({"version":2,"source":generated_source(&options),"edits":[],"player":null});
        std::fs::write(&w.save_path, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(World::load(&options).is_err());
        std::fs::remove_file(partial).unwrap();
        std::fs::remove_file(legacy).unwrap();
        std::fs::remove_file(&w.save_path).unwrap();
    }
    #[test]
    fn native_provider_is_the_default_and_legacy_save_identities_are_preserved() {
        use clap::Parser;
        let mut options = Options::parse_from(["ferrum189", "--seed", "-1"]);
        assert_eq!(options.generator, 4);
        assert!(World::read_chunks(&options, &[(0, 0)]).is_err());
        options.generator = 3;
        assert_eq!(generated_source(&options), "native189-base1--1");
        let positions = [(-1, 0), (0, -1)];
        let actual = World::read_chunks(&options, &positions).unwrap();
        let generator = crate::vanilla::generator::shared(options.seed);
        for (pos, chunk) in actual {
            let original = generator.chunk(pos).unwrap();
            assert_eq!(chunk.blocks, original.blocks);
            assert_eq!(chunk.biomes, original.biomes);
        }
        let mut legacy = options;
        legacy.seed = 189;
        legacy.generator = 1;
        assert_eq!(generated_source(&legacy), "demo-189");
        legacy.generator = 2;
        assert_eq!(generated_source(&legacy), "ferrum-v2-189");
    }
}

/// Detached values only: computing these must not initialize an old shared
/// snapshot before the publisher performs its original COW boundary.
pub(crate) struct PreparedTickIndexes {
    pub(crate) random: Option<RandomTickSections>,
    pub(crate) scheduled: Option<Vec<(u16, u16)>>,
}
impl Chunk {
    pub(crate) fn prepare_tick_indexes_detached(&self) -> PreparedTickIndexes {
        let mut random = self.random_tick_sections.get().is_none().then(RandomTickSections::default);
        let mut scheduled = self.scheduled_tick_blocks.get().is_none().then(Vec::new);
        let top = self.top.clamp(0, 256) as usize;
        // chunks_exact in the original random-index scan ignores any partial
        // trailing section, and rounds top UP. Scheduled candidates do neither.
        let random_end = if random.is_some() { (self.blocks.len() / 4096).min((top + 15) / 16) * 4096 } else { 0 };
        let scheduled_end = if scheduled.is_some() { self.blocks.len().min(top * 256) } else { 0 };
        for (index, &state) in self.blocks.iter().take(random_end.max(scheduled_end)).enumerate() {
            if index < random_end && crate::gameplay::growth::ticks_randomly(state >> 4) {
                random.as_mut().unwrap().counts[index / 4096] += 1;
            }
            if index < scheduled_end && scheduled_tick_candidate(state) {
                scheduled.as_mut().unwrap().push((index as u16, state));
            }
        }
        if let Some(ticks) = &mut random {
            for (section, count) in ticks.counts.iter().enumerate() {
                if *count != 0 { ticks.mask |= 1 << section; }
            }
        }
        PreparedTickIndexes { random, scheduled }
    }
    pub(crate) fn install_prepared_tick_indexes(&self, prepared: PreparedTickIndexes) {
        // Preserve a cache initialized by another alias, even if deliberately
        // stale. Original get_or_init also keeps its first published value.
        if let Some(value) = prepared.random { let _ = self.random_tick_sections.set(value); }
        if let Some(value) = prepared.scheduled { let _ = self.scheduled_tick_blocks.set(value); }
        self.prepare_tick_indexes();
    }
}

#[cfg(test)]
#[path="world_publication_index_tests.rs"]
mod publication_index_tests;
