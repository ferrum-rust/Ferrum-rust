//! Original chunk skylight/height state and bounded World.checkLightFor queues.
//! World block callbacks and provider loading remain caller-owned.
use super::material;
use crate::world::{Chunk, ChunkPos, World};
use glam::IVec3;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
#[path = "lighting_bytes_simple.rs"]
mod bytes;
pub(crate) use bytes::LightBytes;
#[cfg(test)]
#[cfg(test)]
use std::collections::HashMap;
// Light propagation repeatedly probes integer chunk coordinates. Key hashing
// is unrelated to the original traversal order; checkpoints sort this map.
type LightChunks = ahash::AHashMap<ChunkPos, LightChunk>;
const ENUM_DIRECTIONS: [IVec3; 6] = [
    IVec3::NEG_Y,
    IVec3::Y,
    IVec3::NEG_Z,
    IVec3::Z,
    IVec3::NEG_X,
    IVec3::X,
];
const GROW_DIRECTIONS: [IVec3; 6] = [
    IVec3::NEG_X,
    IVec3::X,
    IVec3::NEG_Y,
    IVec3::Y,
    IVec3::NEG_Z,
    IVec3::Z,
];
const HORIZONTAL: [IVec3; 4] = [IVec3::NEG_Z, IVec3::X, IVec3::Z, IVec3::NEG_X];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Sky,
    Block,
}
impl Kind {
    fn default_value(self) -> u8 {
        if self == Self::Sky {
            15
        } else {
            0
        }
    }
}

#[cfg(test)]
impl Lighting {
    /// Borrow dirty state for the ignored owner probe without draining or cloning light.
    pub(crate) fn diagnostic_render_dirty(&self) -> &HashSet<ChunkPos> { &self.render_dirty }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LightChunk {
    pub height: Vec<i32>,
    pub minimum: i32,
    pub sections: [bool; 16],
    pub sky: LightBytes,
    pub block: LightBytes,
    pub gaps: Vec<bool>,
    pub queued_checks: u16,
}

/// Initial per-column state only. No provider, neighboring light or callback
/// state is read until the owner installs this at the original add_chunk stage.
pub(crate) struct PreparedLight {
    light: LightChunk,
    no_sky: bool,
}
impl PreparedLight {
    pub(crate) fn new(chunk: &Chunk, no_sky: bool) -> Self {
        let mut light = LightChunk::default();
        for (i, state) in chunk.blocks.iter().enumerate() {
            if *state != 0 { light.sections[i / 4096] = true; }
        }
        // Keep the original initial generateSkylightMap loop and its empty
        // column/minimum-height behavior. The owner/reference implementation
        // remains independent and is used as the differential oracle.
        let top = light.sections.iter().rposition(|v| *v).unwrap_or(0) as i32 * 16;
        light.minimum = i32::MAX;
        for x in 0..16 {
            for z in 0..16 {
                let col = (z * 16 + x) as usize;
                for y in (1..=top + 16).rev() {
                    if material::light_opacity(chunk.get(x, y - 1, z)) > 0 {
                        light.height[col] = y;
                        light.minimum = light.minimum.min(y);
                        break;
                    }
                }
                if no_sky { continue; }
                let mut value = 15i32;
                let mut y = top + 15;
                loop {
                    let mut opacity = material::light_opacity(chunk.get(x, y, z)) as i32;
                    if opacity == 0 && value != 15 { opacity = 1; }
                    value -= opacity;
                    if value > 0 && light.sections[y as usize / 16] {
                        light.sky[Chunk::index(x, y, z)] = value as u8;
                    }
                    y -= 1;
                    if y <= 0 || value <= 0 { break; }
                }
            }
        }
        Self { light, no_sky }
    }
}

/// Read-only renderer snapshot. One byte per block, low block/high sky nibble.
/// Section presence and height retain the original empty-section behavior.
#[derive(Clone, Debug)]
pub struct ChunkLight {
    pub values: Vec<u8>,
    pub height: Vec<i32>,
    pub sections: [bool; 16],
    pub no_sky: bool,
}
impl ChunkLight {
    pub fn from_chunk(chunk: &LightChunk, no_sky: bool) -> Self {
        Self {
            values: chunk.sky.with_bytes(|sky| chunk.block.with_bytes(|block|
                sky.iter().zip(block).map(|(sky, block)| (sky << 4) | block).collect())),
            height: chunk.height.clone(),
            sections: chunk.sections,
            no_sky,
        }
    }
    pub fn to_chunk(&self) -> LightChunk {
        LightChunk {
            height: self.height.clone(),
            minimum: *self.height.iter().min().unwrap_or(&0),
            sections: self.sections,
            sky: self.values.iter().map(|v| v >> 4).collect(),
            block: self.values.iter().map(|v| v & 15).collect(),
            gaps: vec![false; 256],
            queued_checks: 4096,
        }
    }
    /// Client reconciliation can share an immutable published snapshot until
    /// a raw plane is actually read or changed. Native Runtime generation and
    /// remote section packet fills retain their existing Dense conversion.
    pub(crate) fn to_shared_chunk(self: &std::sync::Arc<Self>) -> LightChunk {
        // Preserve the eager method's length behavior for synthetic/malformed
        // snapshots; every deferred plane has a bounded 64-KiB hydration.
        if self.values.len() != 65536 { return self.to_chunk(); }
        LightChunk {
            height: self.height.clone(),
            minimum: *self.height.iter().min().unwrap_or(&0),
            sections: self.sections,
            sky: LightBytes::from_shared_packed(self.clone(), Kind::Sky),
            block: LightBytes::from_shared_packed(self.clone(), Kind::Block),
            gaps: vec![false; 256],
            queued_checks: 4096,
        }
    }
    pub fn get(&self, kind: Kind, p: IVec3) -> u8 {
        let p = IVec3::new(p.x, p.y.max(0), p.z);
        if !valid(p) {
            return kind.default_value();
        }
        if !self.sections[p.y as usize / 16] {
            return if p.y >= self.height[column(p)] {
                kind.default_value()
            } else {
                0
            };
        }
        let packed = self.values[index(p)];
        match kind {
            Kind::Sky => {
                if self.no_sky {
                    0
                } else {
                    packed >> 4
                }
            }
            Kind::Block => packed & 15,
        }
    }
}

impl Default for LightChunk {
    fn default() -> Self {
        Self {
            height: vec![0; 256],
            minimum: i32::MAX,
            sections: [false; 16],
            sky: vec![0; 65536].into(),
            block: vec![0; 65536].into(),
            gaps: vec![false; 256],
            queued_checks: 4096,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lighting {
    pub no_sky: bool,
    pub sky_subtracted: u8,
    #[serde(with = "chunk_map")]
    pub chunks: LightChunks,
    /// Render cache invalidation has no effect on game state or checkpoints.
    #[serde(skip)]
    render_dirty: HashSet<ChunkPos>,
    /// checkLightFor is synchronous and non-reentrant. Its queue is empty
    /// between calls and is not authoritative state or checkpoint data.
    #[serde(skip)]
    scratch: Vec<i32>,
    #[cfg(test)]
    #[serde(skip)]
    reference_path: bool,
}
mod chunk_map {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        map: &LightChunks,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let ordered: std::collections::BTreeMap<_, _> = map.iter().collect();
        ordered.into_iter().collect::<Vec<_>>().serialize(s)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> Result<LightChunks, D::Error> {
        Ok(Vec::<(ChunkPos, LightChunk)>::deserialize(d)?
            .into_iter()
            .collect())
    }
}
fn cp(p: IVec3) -> ChunkPos {
    (p.x.div_euclid(16), p.z.div_euclid(16))
}
fn column(p: IVec3) -> usize {
    (p.z.rem_euclid(16) * 16 + p.x.rem_euclid(16)) as usize
}
fn index(p: IVec3) -> usize {
    Chunk::index(p.x.rem_euclid(16), p.y, p.z.rem_euclid(16))
}
fn valid(p: IVec3) -> bool {
    (0..256).contains(&p.y)
        && (-30_000_000..30_000_000).contains(&p.x)
        && (-30_000_000..30_000_000).contains(&p.z)
}
impl Lighting {
    pub(crate) fn preparation_mode(&self) -> Option<bool> {
        #[cfg(test)]
        if self.reference_path { return None; }
        Some(self.no_sky)
    }
    /// Publish only at the original AddLight boundary. A changed dimension
    /// mode or enabled reference path falls back to the original calculation.
    pub(crate) fn add_prepared_chunk(&mut self, world: &World, pos: ChunkPos, prepared: PreparedLight) -> bool {
        if self.preparation_mode() != Some(prepared.no_sky) {
            self.add_chunk(world, pos);
            return false;
        }
        self.chunks.insert(pos, prepared.light);
        self.render_dirty.insert(pos);
        true
    }
    pub fn render_snapshot(&self, pos: ChunkPos) -> Option<std::sync::Arc<ChunkLight>> {
        self.chunks
            .get(&pos)
            .map(|chunk| std::sync::Arc::new(ChunkLight::from_chunk(chunk, self.no_sky)))
    }
    /// Membership only; maintenance must not drain publication state.
    pub(crate) fn render_is_dirty(&self, pos: ChunkPos) -> bool { self.render_dirty.contains(&pos) }
    pub fn take_render_dirty(&mut self) -> Vec<ChunkPos> {
        let mut result = self.render_dirty.drain().collect::<Vec<_>>();
        result.sort_unstable();
        result
    }
    pub fn invalidate_render_cache(&mut self) {
        self.render_dirty.extend(self.chunks.keys());
    }
    pub fn new(world: &World, no_sky: bool) -> Self {
        let mut this = Self {
            no_sky,
            sky_subtracted: 0,
            chunks: LightChunks::new(),
            render_dirty: HashSet::new(),
            scratch: Vec::new(),
            #[cfg(test)]
            reference_path: false,
        };
        for &pos in world.chunks.keys() {
            this.add_chunk(world, pos);
        }
        this
    }
    pub fn add_chunk(&mut self, world: &World, pos: ChunkPos) {
        let mut light = LightChunk::default();
        for (i, state) in world.chunks[&pos].blocks.iter().enumerate() {
            if *state != 0 {
                light.sections[i / 4096] = true;
            }
        }
        self.chunks.insert(pos, light);
        self.generate_skylight_map(world, pos);
    }
    pub fn generate_skylight_map(&mut self, world: &World, pos: ChunkPos) {
        #[cfg(test)]
        if self.reference_path { return self.generate_skylight_map_inner::<false>(world, pos); }
        self.generate_skylight_map_inner::<true>(world, pos);
    }
    fn generate_skylight_map_inner<const DIRECT: bool>(&mut self, world: &World, pos: ChunkPos) {
        self.render_dirty.insert(pos);
        let chunk = world.chunks.get(&pos);
        let state = |x, y, z| if DIRECT { chunk.map_or(0, |c| c.get(x, y, z)) }
            else { world.get(IVec3::new(pos.0 * 16 + x, y, pos.1 * 16 + z)) };
        let light = self.chunks.get_mut(&pos).unwrap();
        let top = light.sections.iter().rposition(|v| *v).unwrap_or(0) as i32 * 16;
        light.minimum = i32::MAX;
        for x in 0..16 {
            for z in 0..16 {
                let col = (z * 16 + x) as usize;
                for y in (1..=top + 16).rev() {
                    if material::light_opacity(state(x, y - 1, z)) > 0
                    {
                        light.height[col] = y;
                        light.minimum = light.minimum.min(y);
                        break;
                    }
                }
                if self.no_sky {
                    continue;
                }
                let mut value = 15i32;
                let mut y = top + 15;
                loop {
                    let mut opacity = material::light_opacity(state(x, y, z)) as i32;
                    if opacity == 0 && value != 15 {
                        opacity = 1;
                    }
                    value -= opacity;
                    if value > 0 && light.sections[y as usize / 16] {
                        light.sky[Chunk::index(x, y, z)] = value as u8;
                    }
                    y -= 1;
                    if y <= 0 || value <= 0 {
                        break;
                    }
                }
            }
        }
    }
    pub fn can_see_sky(&self, p: IVec3) -> bool {
        self.chunks
            .get(&cp(p))
            .is_some_and(|c| p.y >= c.height[column(p)])
    }
    pub fn height(&self, p: IVec3) -> i32 {
        self.chunks.get(&cp(p)).map_or(0, |c| c.height[column(p)])
    }
    pub fn get(&self, kind: Kind, mut p: IVec3) -> u8 {
        if p.y < 0 {
            p.y = 0;
        }
        if !valid(p) {
            return kind.default_value();
        }
        let Some(c) = self.chunks.get(&cp(p)) else {
            return kind.default_value();
        };
        if !c.sections[p.y as usize / 16] {
            return if p.y >= c.height[column(p)] {
                kind.default_value()
            } else {
                0
            };
        }
        match kind {
            Kind::Sky => {
                if self.no_sky {
                    0
                } else {
                    c.sky.value_at(index(p))
                }
            }
            Kind::Block => c.block.value_at(index(p)),
        }
    }
    pub fn neighbor_light(&self, world: &World, kind: Kind, mut p: IVec3) -> u8 {
        if self.no_sky && kind == Kind::Sky {
            return 0;
        }
        p.y = p.y.max(0);
        if !valid(p) || !self.chunks.contains_key(&cp(p)) {
            return kind.default_value();
        }
        if material::neighbor_brightness(world.get(p)) {
            [IVec3::Y, IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z]
                .iter()
                .map(|d| self.get(kind, p + *d))
                .max()
                .unwrap()
        } else {
            self.get(kind, p)
        }
    }
    pub fn combined(&self, world: &World, p: IVec3, neighbors: bool) -> u8 {
        self.combined_inner(world, p, neighbors, self.sky_subtracted)
    }
    pub fn unsubtracted(&self, world: &World, p: IVec3) -> u8 {
        self.combined_inner(world, p, false, 0)
    }
    fn combined_inner(&self, world: &World, p: IVec3, neighbors: bool, subtracted: u8) -> u8 {
        if !(-30_000_000..30_000_000).contains(&p.x) || !(-30_000_000..30_000_000).contains(&p.z) {
            return 15;
        }
        if neighbors && material::neighbor_brightness(world.get(p)) {
            return [IVec3::Y, IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z]
                .iter()
                .map(|d| self.combined_inner(world, p + *d, false, subtracted))
                .max()
                .unwrap();
        }
        if p.y < 0 {
            return 0;
        }
        let p = IVec3::new(p.x, p.y.min(255), p.z);
        let Some(c) = self.chunks.get(&cp(p)) else {
            return 0;
        };
        if !c.sections[p.y as usize / 16] {
            return if self.no_sky {
                0
            } else {
                15u8.saturating_sub(subtracted)
            };
        }
        self.get(Kind::Sky, p)
            .saturating_sub(subtracted)
            .max(self.get(Kind::Block, p))
    }
    pub fn set(&mut self, world: &World, kind: Kind, p: IVec3, value: u8) {
        #[cfg(test)]
        if self.reference_path { return self.set_reference(world, kind, p, value); }
        if !valid(p) { return; }
        let pos = cp(p);
        let Some(c) = self.chunks.get_mut(&pos) else { return; };
        if c.sections[p.y as usize / 16] {
            let at = index(p);
            let before = match kind {
                Kind::Sky => if self.no_sky { 0 } else { c.sky.value_at(at) },
                Kind::Block => c.block.value_at(at),
            };
            let value = value & 15;
            // Even a suppressed Sky write in a no-sky dimension can dirty the
            // renderer: preserve get(Sky)'s zero before comparing the value.
            if before != value { self.render_dirty.insert(pos); }
            match kind {
                Kind::Sky => if !self.no_sky { c.sky[at] = value; },
                Kind::Block => c.block[at] = value,
            }
            return;
        }
        // New sections retain the original dirty check and complete skylight
        // regeneration before the requested nibble is stored.
        self.set_reference(world, kind, p, value);
    }
    fn set_reference(&mut self, world: &World, kind: Kind, p: IVec3, value: u8) {
        if !valid(p) || !self.chunks.contains_key(&cp(p)) {
            return;
        }
        if self.get(kind, p) != value & 15 {
            self.render_dirty.insert(cp(p));
        }
        if !self.chunks[&cp(p)].sections[p.y as usize / 16] {
            self.chunks.get_mut(&cp(p)).unwrap().sections[p.y as usize / 16] = true;
            self.generate_skylight_map(world, cp(p));
        }
        let c = self.chunks.get_mut(&cp(p)).unwrap();
        match kind {
            Kind::Sky => {
                if !self.no_sky {
                    c.sky[index(p)] = value & 15;
                }
            }
            Kind::Block => c.block[index(p)] = value & 15,
        }
    }
    fn raw(&self, world: &World, kind: Kind, p: IVec3) -> u8 {
        #[cfg(test)]
        if self.reference_path { return self.raw_reference(world, kind, p); }
        let pos = cp(p);
        let center_light = if kind == Kind::Sky { self.chunks.get(&pos) } else { None };
        if kind == Kind::Sky && center_light.is_some_and(|c| p.y >= c.height[column(p)]) {
            return 15;
        }
        let state = world.get(p);
        let emitted = material::light_value(state);
        let mut value = if kind == Kind::Sky { 0 } else { emitted };
        let mut opacity = material::light_opacity(state);
        if opacity >= 15 && emitted > 0 { opacity = 1; }
        opacity = opacity.max(1);
        if opacity >= 15 { return 0; }
        if value >= 14 { return value; }
        let center_light = if kind == Kind::Block { self.chunks.get(&pos) } else { center_light };
        for d in ENUM_DIRECTIONS {
            let next = p + d;
            let light = if cp(next) == pos {
                self.get_in_column(kind, next, center_light)
            } else {
                self.get(kind, next)
            };
            value = value.max(light.saturating_sub(opacity));
            if value >= 14 { return value; }
        }
        value
    }
    /// Identical get semantics for an already-resolved column. The borrow is
    /// confined to raw(), which cannot change light storage or run callbacks.
    fn get_in_column(&self, kind: Kind, mut p: IVec3, chunk: Option<&LightChunk>) -> u8 {
        if p.y < 0 { p.y = 0; }
        if !valid(p) { return kind.default_value(); }
        let Some(c) = chunk else { return kind.default_value(); };
        if !c.sections[p.y as usize / 16] {
            return if p.y >= c.height[column(p)] { kind.default_value() } else { 0 };
        }
        match kind {
            Kind::Sky => if self.no_sky { 0 } else { c.sky.value_at(index(p)) },
            Kind::Block => c.block.value_at(index(p)),
        }
    }
    #[cfg(test)]
    fn raw_reference(&self, world: &World, kind: Kind, p: IVec3) -> u8 {
        if kind == Kind::Sky && self.can_see_sky(p) {
            return 15;
        }
        let state = world.get(p);
        let emitted = material::light_value(state);
        let mut value = if kind == Kind::Sky { 0 } else { emitted };
        let mut opacity = material::light_opacity(state);
        if opacity >= 15 && emitted > 0 {
            opacity = 1;
        }
        opacity = opacity.max(1);
        if opacity >= 15 {
            return 0;
        }
        if value >= 14 {
            return value;
        }
        for d in ENUM_DIRECTIONS {
            value = value.max(self.get(kind, p + d).saturating_sub(opacity));
            if value >= 14 {
                return value;
            }
        }
        value
    }
    fn area_loaded(&self, p: IVec3, radius: i32) -> bool {
        p.y + radius >= 0
            && p.y - radius < 256
            && ((p.x - radius).div_euclid(16)..=(p.x + radius).div_euclid(16)).all(|x| {
                ((p.z - radius).div_euclid(16)..=(p.z + radius).div_euclid(16))
                    .all(|z| self.chunks.contains_key(&(x, z)))
            })
    }
    pub fn check(&mut self, world: &World, kind: Kind, origin: IVec3) -> bool {
        #[cfg(test)]
        if self.reference_path { return self.check_inner::<false>(world, kind, origin); }
        self.check_inner::<true>(world, kind, origin)
    }
    fn check_inner<const REUSE: bool>(&mut self, world: &World, kind: Kind, origin: IVec3) -> bool {
        if !self.area_loaded(origin, 17) {
            return false;
        }
        self.check_loaded_inner::<REUSE>(world, kind, origin)
    }
    // Availability was checked immediately before this call. Light updates
    // only change existing columns; no provider or game callback can intervene.
    fn check_loaded_inner<const REUSE: bool>(&mut self, world: &World, kind: Kind, origin: IVec3) -> bool {
        let old = self.get(kind, origin);
        let desired = self.raw(world, kind, origin);
        if REUSE && old == desired { return true; }
        let mut queue = if REUSE { std::mem::take(&mut self.scratch) } else { Vec::with_capacity(32768) };
        if REUSE { queue.reserve(32768); }
        let mut cursor = 0;
        let encode = |p: IVec3, value: u8| {
            let d = p - origin + IVec3::splat(32);
            d.x | (d.y << 6) | (d.z << 12) | ((value as i32) << 18)
        };
        let decode = |v: i32| {
            origin + IVec3::new((v & 63) - 32, ((v >> 6) & 63) - 32, ((v >> 12) & 63) - 32)
        };
        if desired > old {
            queue.push(133152);
        } else if desired < old {
            queue.push(133152 | ((old as i32) << 18));
            while cursor < queue.len() {
                let encoded = queue[cursor];
                cursor += 1;
                let p = decode(encoded);
                let value = ((encoded >> 18) & 15) as u8;
                if self.get(kind, p) != value {
                    continue;
                }
                self.set(world, kind, p, 0);
                if value == 0 || (p - origin).abs().element_sum() >= 17 {
                    continue;
                }
                for d in ENUM_DIRECTIONS {
                    let next = p + d;
                    let opacity = material::light_opacity(world.get(next)).max(1);
                    if value >= opacity
                        && self.get(kind, next) == value - opacity
                        && queue.len() < 32768
                    {
                        queue.push(encode(next, value - opacity));
                    }
                }
            }
            cursor = 0;
        }
        while cursor < queue.len() {
            let p = decode(queue[cursor]);
            cursor += 1;
            let before = self.get(kind, p);
            let desired = self.raw(world, kind, p);
            if before == desired {
                continue;
            }
            self.set(world, kind, p, desired);
            if desired <= before
                || (p - origin).abs().element_sum() >= 17
                || queue.len() >= 32768 - 6
            {
                continue;
            }
            for d in GROW_DIRECTIONS {
                let next = p + d;
                if self.get(kind, next) < desired {
                    queue.push(encode(next, 0));
                }
            }
        }
        if REUSE { queue.clear(); self.scratch = queue; }
        true
    }
    // Cache only one call-local fixed-X/Z availability result. Nested light
    // propagation never inserts/removes columns or invokes provider callbacks.
    // Y eligibility must still be evaluated for every requested position.
    fn fixed_column_loaded(&self, p: IVec3, available: &mut Option<bool>) -> bool {
        if p.y + 17 < 0 || p.y - 17 >= 256 { return false; }
        *available.get_or_insert_with(|| self.area_loaded(p, 17))
    }
    fn check_both_fixed_column(&mut self, world: &World, p: IVec3,
        available: &mut Option<bool>) -> bool {
        if !self.fixed_column_loaded(p, available) { return false; }
        let sky = !self.no_sky && self.check_loaded_inner::<true>(world, Kind::Sky, p);
        self.check_loaded_inner::<true>(world, Kind::Block, p) || sky
    }
    fn update_neighbor_height(&mut self, world: &World, x: i32, z: i32, min: i32, max: i32) {
        #[cfg(test)]
        if self.reference_path {
            return self.update_neighbor_height_inner::<false>(world, x, z, min, max);
        }
        self.update_neighbor_height_inner::<true>(world, x, z, min, max);
    }
    fn update_neighbor_height_inner<const REUSE: bool>(&mut self, world: &World,
        x: i32, z: i32, min: i32, max: i32) {
        if max > min && self.area_loaded(IVec3::new(x, 0, z), 16) {
            let mut available = None;
            for y in min..max {
                let p = IVec3::new(x, y, z);
                if REUSE {
                    if self.fixed_column_loaded(p, &mut available) {
                        self.check_loaded_inner::<true>(world, Kind::Sky, p);
                    }
                } else {
                    self.check(world, Kind::Sky, p);
                }
            }
        }
    }
    fn relight(&mut self, world: &World, p: IVec3) {
        #[cfg(test)]
        if self.reference_path { return self.relight_inner::<false>(world, p); }
        self.relight_inner::<true>(world, p);
    }
    fn relight_inner<const DIRECT: bool>(&mut self, world: &World, p: IVec3) {
        let pos = cp(p);
        let col = column(p);
        let old = self.chunks[&pos].height[col] & 255;
        let mut height = old.max(p.y);
        // Both scans stay in this one raw column. Nested light checks can
        // mutate only Lighting: the World borrow remains immutable throughout.
        // Keep World::get's Y-only guard; it does not reject X/Z world borders.
        let chunk = if DIRECT { world.chunks.get(&pos) } else { None };
        let state = |y| {
            if DIRECT {
                if !(0..256).contains(&y) { return 0; }
                chunk.map_or(0, |c| c.get(p.x.rem_euclid(16), y, p.z.rem_euclid(16)))
            } else {
                world.get(IVec3::new(p.x, y, p.z))
            }
        };
        while height > 0
            && material::light_opacity(state(height - 1)) == 0
        {
            height -= 1;
        }
        if height == old {
            return;
        }
        if !self.no_sky {
            for y in height.min(old)..=height.max(old) {
                self.check(world, Kind::Sky, IVec3::new(p.x, y, p.z));
            }
        }
        self.chunks.get_mut(&pos).unwrap().height[col] = height;
        self.render_dirty.insert(pos);
        if !self.no_sky {
            let c = self.chunks.get_mut(&pos).unwrap();
            for y in height.min(old)..height.max(old) {
                if c.sections[y as usize / 16] {
                    c.sky[index(IVec3::new(p.x, y, p.z))] = if height < old { 15 } else { 0 };
                }
            }
            let mut value = 15i32;
            let mut y = height;
            while y > 0 && value > 0 {
                y -= 1;
                value = (value
                    - material::light_opacity(state(y)).max(1) as i32)
                    .max(0);
                if c.sections[y as usize / 16] {
                    c.sky[index(IVec3::new(p.x, y, p.z))] = value as u8;
                }
            }
        }
        let c = self.chunks.get_mut(&pos).unwrap();
        c.minimum = c.minimum.min(height);
        if !self.no_sky {
            for d in HORIZONTAL {
                self.update_neighbor_height(
                    world,
                    p.x + d.x,
                    p.z + d.z,
                    old.min(height),
                    old.max(height),
                );
            }
            self.update_neighbor_height(world, p.x, p.z, old.min(height), old.max(height));
        }
    }
    /// Whether both original light phases can be omitted for a changed state.
    /// A true result also guarantees this chunk and section already exist.
    /// Callers may retain those height/section arrays only while no intervening
    /// callback or light mutation can occur; other writes must use both phases.
    pub(crate) fn opaque_replacement_is_noop(&self, p: IVec3, old: u16, new: u16) -> bool {
        #[cfg(test)]
        if self.reference_path { return false; }
        if !valid(p) || old == new
            || material::light_opacity(old) != 255 || material::light_value(old) != 0
            || material::light_opacity(new) != 255 || material::light_value(new) != 0
        {
            return false;
        }
        self.chunks.get(&cp(p)).is_some_and(|light| {
            light.sections[p.y as usize / 16] && p.y < light.height[column(p)]
        })
    }
    /// Chunk-light phase for a known replacement between fully opaque,
    /// non-emitting states, after the raw write and before output publication.
    /// The caller must guarantee that world.get(p) is still `new`; no callback
    /// may intervene. Created sections and surface/height repairs retain the
    /// ordinary chunk phase. The separate after-block-added phase stays owned
    /// by the caller, including its original placement among output events.
    pub(crate) fn opaque_block_replaced(&mut self, world: &World, p: IVec3, old: u16, new: u16) {
        debug_assert_eq!((material::light_opacity(old), material::light_value(old)), (255, 0));
        debug_assert_eq!((material::light_opacity(new), material::light_value(new)), (255, 0));
        #[cfg(test)]
        if self.reference_path { return self.chunk_block_changed_reference(world, p, old); }
        if !valid(p) || old == new { return; }
        let Some(light) = self.chunks.get(&cp(p)) else { return; };
        if light.sections[p.y as usize / 16] && p.y < light.height[column(p)] { return; }
        self.chunk_block_changed(world, p, old);
    }
    /// Called after raw block state replacement, before non-light block callbacks.
    pub fn chunk_block_changed(&mut self, world: &World, p: IVec3, old: u16) {
        #[cfg(test)]
        if self.reference_path { return self.chunk_block_changed_reference(world, p, old); }
        if !valid(p) {
            return;
        }
        let pos = cp(p);
        let Some(light) = self.chunks.get_mut(&pos) else { return; };
        let new = world.get(p);
        if new == old {
            return;
        }
        let col = column(p);
        let height = light.height[col];
        let created = !light.sections[p.y as usize / 16];
        if created && new == 0 {
            return;
        }
        light.sections[p.y as usize / 16] = true;
        // Ordinary stone/ore changes stay below the same opaque height and
        // cannot change gap lighting. Keep their common path to one lookup.
        let opacity = material::light_opacity(new);
        let previous = material::light_opacity(old);
        if !created && opacity > 0 && opacity == previous && p.y < height {
            return;
        }
        if created {
            self.render_dirty.insert(pos);
        }
        if created && p.y >= height {
            self.generate_skylight_map(world, pos);
        } else {
            if opacity > 0 {
                if p.y >= height {
                    self.relight(world, p + IVec3::Y);
                }
            } else if p.y == height - 1 {
                self.relight(world, p);
            }
            if opacity != previous
                && (opacity < previous
                    || self.get(Kind::Sky, p) > 0
                    || self.get(Kind::Block, p) > 0)
            {
                self.chunks.get_mut(&pos).unwrap().gaps[col] = true;
            }
        }
    }
    #[cfg(test)]
    fn chunk_block_changed_reference(&mut self, world: &World, p: IVec3, old: u16) {
        if !valid(p) || !self.chunks.contains_key(&cp(p)) {
            return;
        }
        let new = world.get(p);
        if new == old {
            return;
        }
        let pos = cp(p);
        let col = column(p);
        let height = self.chunks[&pos].height[col];
        let created = !self.chunks[&pos].sections[p.y as usize / 16];
        if created && new == 0 {
            return;
        }
        self.chunks.get_mut(&pos).unwrap().sections[p.y as usize / 16] = true;
        if created {
            self.render_dirty.insert(pos);
        }
        if created && p.y >= height {
            self.generate_skylight_map(world, pos);
        } else {
            let opacity = material::light_opacity(new);
            let previous = material::light_opacity(old);
            if opacity > 0 {
                if p.y >= height {
                    self.relight(world, p + IVec3::Y);
                }
            } else if p.y == height - 1 {
                self.relight(world, p);
            }
            if opacity != previous
                && (opacity < previous
                    || self.get(Kind::Sky, p) > 0
                    || self.get(Kind::Block, p) > 0)
            {
                self.chunks.get_mut(&pos).unwrap().gaps[col] = true;
            }
        }
    }
    pub fn world_block_changed(&mut self, world: &World, p: IVec3, old: u16) {
        self.chunk_block_changed(world, p, old);
        self.after_block_added(world, p, old);
    }
    /// World.setBlockState's light checks run after Block.onBlockAdded returns.
    pub fn after_block_added(&mut self, world: &World, p: IVec3, old: u16) {
        let new = world.get(p);
        self.after_block_added_state(world, p, old, new);
    }
    /// Compare the originally requested state, even if onBlockAdded replaced it.
    pub fn after_block_added_state(&mut self, world: &World, p: IVec3, old: u16, new: u16) {
        if material::light_opacity(old) != material::light_opacity(new)
            || material::light_value(old) != material::light_value(new)
        {
            self.check_both(world, p);
        }
    }
    fn check_both(&mut self, world: &World, p: IVec3) -> bool {
        #[cfg(test)]
        if self.reference_path {
            let sky = !self.no_sky && self.check(world, Kind::Sky, p);
            return self.check(world, Kind::Block, p) || sky;
        }
        if !self.area_loaded(p, 17) { return false; }
        let sky = !self.no_sky && self.check_loaded_inner::<true>(world, Kind::Sky, p);
        self.check_loaded_inner::<true>(world, Kind::Block, p) || sky
    }
    fn check_column(
        &mut self,
        world: &World,
        pos: ChunkPos,
        x: i32,
        z: i32,
        sea_level: i32,
    ) -> bool {
        #[cfg(test)]
        if self.reference_path { return self.check_column_inner::<false>(world, pos, x, z, sea_level); }
        self.check_column_inner::<true>(world, pos, x, z, sea_level)
    }
    fn check_column_inner<const DIRECT: bool>(&mut self, world: &World, pos: ChunkPos,
        x: i32, z: i32, sea_level: i32) -> bool {
        let chunk = world.chunks.get(&pos);
        let state = |y| if DIRECT { if (0..256).contains(&y) { chunk.map_or(0, |c| c.get(x, y, z)) } else { 0 } }
            else { world.get(IVec3::new(pos.0 * 16 + x, y, pos.1 * 16 + z)) };
        let top = self.chunks[&pos]
            .sections
            .iter()
            .rposition(|v| *v)
            .unwrap_or(0) as i32
            * 16;
        let mut available = None;
        let mut opaque_seen = false;
        let mut below_sea = false;
        let mut y = top + 15;
        let mut last_y = 0;
        while y > sea_level || y > 0 && !below_sea {
            let p = IVec3::new(pos.0 * 16 + x, y, pos.1 * 16 + z);
            last_y = y;
            let opacity = material::light_opacity(state(y));
            if opacity == 255 && y < sea_level {
                below_sea = true;
            }
            if !opaque_seen && opacity > 0 {
                opaque_seen = true;
            } else if opaque_seen && opacity == 0 {
                let checked = if DIRECT { self.check_both_fixed_column(world, p, &mut available) }
                    else { self.check_both(world, p) };
                if !checked { return false; }
            }
            y -= 1;
        }
        for y in (1..=last_y).rev() {
            let p = IVec3::new(pos.0 * 16 + x, y, pos.1 * 16 + z);
            if material::light_value(state(y)) > 0 {
                if DIRECT { self.check_both_fixed_column(world, p, &mut available); }
                else { self.check_both(world, p); }
            }
        }
        true
    }
    #[cfg(test)]
    pub(crate) fn use_reference_path(&mut self, enabled: bool) { self.reference_path = enabled; }
    /// Chunk.func_150809_p; caller owns terrain/light-populated flags.
    pub fn initialize_chunk(
        &mut self,
        world: &World,
        pos: ChunkPos,
        sea_level: i32,
        terrain_populated: impl Fn(ChunkPos) -> bool,
    ) -> bool {
        if self.no_sky {
            return true;
        }
        if !((pos.0 - 1)..=(pos.0 + 1))
            .all(|x| ((pos.1 - 1)..=(pos.1 + 1)).all(|z| self.chunks.contains_key(&(x, z))))
        {
            return false;
        }
        for x in 0..16 {
            for z in 0..16 {
                if !self.check_column(world, pos, x, z, sea_level) {
                    return false;
                }
            }
        }
        for d in HORIZONTAL {
            let other = (pos.0 + d.x, pos.1 + d.z);
            if !terrain_populated(other) {
                continue;
            }
            for i in 0..16 {
                let (x, z) = if d.x > 0 {
                    (0, i)
                } else if d.x < 0 {
                    (15, i)
                } else if d.z > 0 {
                    (i, 0)
                } else {
                    (i, 15)
                };
                self.check_column(world, other, x, z, sea_level);
            }
        }
        self.chunks.get_mut(&pos).unwrap().gaps.fill(true);
        self.recheck_gaps(world, pos, false);
        true
    }
    pub fn reset_relight_checks(&mut self, pos: ChunkPos) {
        self.chunks.get_mut(&pos).unwrap().queued_checks = 0;
    }
    /// Original amortized eight section-column checks per call.
    pub fn enqueue_relight_checks(&mut self, world: &World, pos: ChunkPos) {
        for _ in 0..8 {
            let n = self.chunks[&pos].queued_checks as i32;
            if n >= 4096 {
                return;
            }
            self.chunks.get_mut(&pos).unwrap().queued_checks += 1;
            let (section, x, z) = (n % 16, n / 16 % 16, n / 256);
            for dy in 0..16 {
                let p = IVec3::new(pos.0 * 16 + x, section * 16 + dy, pos.1 * 16 + z);
                let present = self.chunks[&pos].sections[section as usize];
                let boundary = dy == 0 || dy == 15 || x == 0 || x == 15 || z == 0 || z == 15;
                if (!present && !boundary) || (present && world.get(p) >> 4 != 0) {
                    continue;
                }
                for d in ENUM_DIRECTIONS {
                    if material::light_value(world.get(p + d)) > 0 {
                        self.check_both(world, p + d);
                    }
                }
                self.check_both(world, p);
            }
        }
    }
    pub fn recheck_gaps(&mut self, world: &World, pos: ChunkPos, one: bool) {
        if !self.area_loaded(IVec3::new(pos.0 * 16 + 8, 0, pos.1 * 16 + 8), 16) {
            return;
        }
        for x in 0..16 {
            for z in 0..16 {
                let col = (z * 16 + x) as usize;
                if !self.chunks[&pos].gaps[col] {
                    continue;
                }
                self.chunks.get_mut(&pos).unwrap().gaps[col] = false;
                let p = IVec3::new(pos.0 * 16 + x, 0, pos.1 * 16 + z);
                let height = self.height(p);
                let min = HORIZONTAL
                    .iter()
                    .map(|d| self.chunks.get(&cp(p + *d)).map_or(0, |c| c.minimum))
                    .min()
                    .unwrap();
                for (at, level) in
                    std::iter::once((p, min)).chain(HORIZONTAL.map(|d| (p + d, height)))
                {
                    let top = self.height(at);
                    if top > level {
                        self.update_neighbor_height(world, at.x, at.z, level, top + 1);
                    } else if top < level {
                        self.update_neighbor_height(world, at.x, at.z, top, level + 1);
                    }
                }
                if one {
                    return;
                }
            }
        }
    }
}
#[cfg(test)]
#[path = "lighting_performance_tests.rs"]
mod performance_tests;
#[cfg(test)]
#[path = "lighting_prepared_tests.rs"]
mod prepared_tests;
#[cfg(test)]
#[path = "lighting_lookup_tests.rs"]
mod lookup_tests;
#[cfg(test)]
#[path = "lighting_relight_tests.rs"]
mod relight_tests;
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[derive(Deserialize)]
    struct Case {
        no_sky: bool,
        sea_level: i32,
        steps: Vec<Step>,
    }
    #[derive(Deserialize)]
    struct Step {
        #[serde(default)]
        edits: Vec<[i32; 4]>,
        #[serde(default)]
        gaps: Vec<[i32; 3]>,
        #[serde(default)]
        initialize: Vec<[i32; 3]>,
        #[serde(default)]
        raw_edits: Vec<[i32; 4]>,
        #[serde(default)]
        reset_checks: bool,
        #[serde(default)]
        enqueue: usize,
        snapshot: Snapshot,
    }
    #[derive(Deserialize)]
    struct Snapshot {
        name: String,
        chunks: Vec<ExpectedChunk>,
        queries: Vec<[i32; 10]>,
    }
    #[derive(Deserialize)]
    struct ExpectedChunk {
        pos: ChunkPos,
        height: Vec<i32>,
        minimum: i32,
        gaps: Vec<bool>,
        sections: [bool; 16],
        light: Vec<[usize; 2]>,
        queued_checks: u16,
    }
    fn flat_world() -> World {
        let mut world = World {
            chunks: crate::world::ChunkMap::new(),
            spawn: glam::Vec3::ZERO,
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        };
        for x in -2..=2 {
            for z in -2..=2 {
                let mut c = Chunk::empty();
                for dx in 0..16 {
                    for dz in 0..16 {
                        c.set(dx, 0, dz, 7 << 4);
                    }
                }
                world.chunks.insert((x, z), Arc::new(c));
            }
        }
        world
    }
    #[test]
    fn spring_and_instant_falling_callbacks_preserve_original_lighting() {
        use crate::{
            block_ticks::{BlockAccess, BlockTicks},
            vanilla::{
                drops::{self, DropEvent, DropTile, DropWorld},
                plant_events::{self, PlantWorld},
                random::JavaRandom,
            },
        };
        struct Lit<'a> {
            world: &'a mut World,
            lighting: &'a mut Lighting,
            random: JavaRandom,
            ticks: BlockTicks,
            items: Vec<DropEvent>,
        }
        impl BlockAccess for Lit<'_> {
            fn get(&self, p: IVec3) -> u16 {
                self.world.get(p)
            }
            fn chunk_loaded(&self, p: ChunkPos) -> bool {
                self.world.chunks.contains_key(&p)
            }
            fn set_before_added(&mut self, p: IVec3, state: u16) -> bool {
                let old = self.world.get(p);
                if old == state || !self.world.set(p, state) {
                    return false;
                }
                self.lighting.chunk_block_changed(self.world, p, old);
                true
            }
            fn after_added(&mut self, p: IVec3, old: u16, new: u16) {
                self.lighting
                    .after_block_added_state(self.world, p, old, new);
            }
            fn world_random_int(&mut self, bound: i32) -> Option<i32> {
                Some(self.random.next_int(bound))
            }
            fn drop_block_item(&mut self, p: IVec3, state: u16) -> bool {
                let mut random = self.random.share();
                drops::drop_block(self, &mut random, p, state, 1., 0).unwrap();
                true
            }
            fn neighbor_changed(&mut self, p: IVec3, state: u16, _source: u16) {
                plant_events::neighbor_changed(self, p, state);
            }
        }
        impl DropWorld for Lit<'_> {
            fn is_remote(&self) -> bool {
                false
            }
            fn do_tile_drops(&self) -> bool {
                true
            }
            fn tile_entity(&mut self, _p: IVec3) -> anyhow::Result<Option<DropTile>> {
                Ok(None)
            }
            fn emit(&mut self, event: DropEvent) -> anyhow::Result<()> {
                self.items.push(event);
                Ok(())
            }
        }
        impl PlantWorld for Lit<'_> {
            fn get(&self, p: IVec3) -> u16 {
                self.world.get(p)
            }
            fn light(&self, p: IVec3) -> u8 {
                self.lighting.unsubtracted(self.world, p)
            }
            fn can_see_sky(&self, p: IVec3) -> bool {
                self.lighting.can_see_sky(p)
            }
            fn set(&mut self, p: IVec3, state: u16, flags: u8) {
                let mut ticks = self.ticks.share_scheduler();
                ticks.set_block(self, p, state, flags & 1 != 0);
            }
            fn drop_block(&mut self, p: IVec3, state: u16) {
                self.drop_block_item(p, state);
            }
        }
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/vanilla189-springs.json"))
                .unwrap();
        assert_eq!(
            cases
                .as_array()
                .unwrap()
                .iter()
                .filter(|c| c.get("light_hashes").is_some())
                .count(),
            168
        );
        for (number, case) in cases.as_array().unwrap().iter().enumerate() {
            let seed = case["seed"].as_str().unwrap().parse::<i64>().unwrap();
            let mut world = flat_world();
            let mut lighting = Lighting::new(&world, false);
            for edit in case["setup"].as_array().unwrap() {
                let p = IVec3::new(
                    edit[0].as_i64().unwrap() as i32,
                    edit[1].as_i64().unwrap() as i32,
                    edit[2].as_i64().unwrap() as i32,
                );
                let old = world.get(p);
                world.set(p, edit[3].as_u64().unwrap() as u16);
                lighting.world_block_changed(&world, p, old);
            }
            let mut access = Lit {
                world: &mut world,
                lighting: &mut lighting,
                random: JavaRandom::new(seed ^ 0x123456789abcdef),
                ticks: BlockTicks::default(),
                items: Vec::new(),
            };
            let p = IVec3::new(
                case["position"][0].as_i64().unwrap() as i32,
                case["position"][1].as_i64().unwrap() as i32,
                case["position"][2].as_i64().unwrap() as i32,
            );
            let mut ticks = access.ticks.share_scheduler();
            ticks.set_fall_instantly(case["fall_instantly"].as_bool().unwrap());
            let mut random = JavaRandom::new(seed);
            ticks.generate_spring_access(
                &mut access,
                p,
                case["fluid"].as_u64().unwrap() as u16,
                &mut random,
            );
            assert_eq!(
                random.next_long().to_string(),
                case["population_next"].as_str().unwrap(),
                "spring {number} pop random"
            );
            assert_eq!(
                access.random.next_long().to_string(),
                case["world_next"].as_str().unwrap(),
                "spring {number} shared world random"
            );
            let expected_items = case["items"].as_array().unwrap();
            assert_eq!(
                access.items.len(),
                expected_items.len(),
                "spring {number} items"
            );
            for (item, expected) in access.items.iter().zip(expected_items) {
                let DropEvent::Item {
                    position,
                    stack,
                    pickup_delay,
                } = item
                else {
                    panic!("Unexpected spring drop {item:?}");
                };
                assert_eq!(
                    [stack.id as i64, stack.meta as i64, stack.count as i64],
                    [
                        expected[0].as_i64().unwrap(),
                        expected[1].as_i64().unwrap(),
                        expected[2].as_i64().unwrap()
                    ],
                    "spring {number} item"
                );
                assert_eq!(*pickup_delay, 10);
                for i in 0..3 {
                    assert_eq!(
                        position[i].to_bits(),
                        expected[i + 3].as_f64().unwrap().to_bits(),
                        "spring {number} drop position {i}"
                    );
                }
            }
            let expected: HashMap<IVec3, u16> = case["final"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    (
                        IVec3::new(
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
                        let at = IVec3::new(x, y, z);
                        assert_eq!(
                            access.world.get(at),
                            expected.get(&at).copied().unwrap_or(0),
                            "spring {number} state {at:?}"
                        );
                    }
                }
            }
            for row in case["light_hashes"].as_array().into_iter().flatten() {
                let (cx, cz) = (
                    row[0].as_i64().unwrap() as i32,
                    row[1].as_i64().unwrap() as i32,
                );
                let mut hash = 0xcbf29ce484222325u64;
                for y in 0..256 {
                    for z in 0..16 {
                        for x in 0..16 {
                            let p = IVec3::new(cx * 16 + x, y, cz * 16 + z);
                            let light =
                                lighting.get(Kind::Sky, p) * 16 + lighting.get(Kind::Block, p);
                            hash = (hash ^ u64::from(light)).wrapping_mul(0x100000001b3);
                        }
                    }
                }
                assert_eq!(
                    hash.to_string(),
                    row[2].as_str().unwrap(),
                    "spring {number} chunk {cx},{cz}"
                );
            }
        }
    }
    #[test]
    fn light_propagation_height_sections_and_neighbor_queries_match_original_worldserver() {
        let cases: Vec<Case> = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-lighting.json"
        ))
        .unwrap();
        for (case_id, case) in cases.into_iter().enumerate() {
            let mut world = flat_world();
            let mut lighting = Lighting::new(&world, case.no_sky);
            for step in case.steps {
                for [x, y, z, state] in step.edits {
                    let p = IVec3::new(x, y, z);
                    let old = world.get(p);
                    world.set(p, state as u16);
                    lighting.world_block_changed(&world, p, old);
                }
                for [x, z, one] in step.gaps {
                    lighting.recheck_gaps(&world, (x, z), one != 0);
                }
                let mut populated = std::collections::HashSet::new();
                for [x, z, expected] in step.initialize {
                    populated.insert((x, z));
                    assert_eq!(
                        lighting.initialize_chunk(&world, (x, z), case.sea_level, |p| populated
                            .contains(&p)),
                        expected != 0,
                        "initialize case {case_id} {x},{z}"
                    );
                }
                for [x, y, z, state] in step.raw_edits {
                    let p = IVec3::new(x, y, z);
                    let old = world.get(p);
                    world.set(p, state as u16);
                    lighting.chunk_block_changed(&world, p, old);
                }
                if step.reset_checks {
                    lighting.reset_relight_checks((0, 0));
                }
                for _ in 0..step.enqueue {
                    lighting.enqueue_relight_checks(&world, (0, 0));
                }
                let s = step.snapshot;
                for expected in s.chunks {
                    let actual = &lighting.chunks[&expected.pos];
                    assert_eq!(
                        actual.queued_checks, expected.queued_checks,
                        "case {case_id} {} {:?} queued",
                        s.name, expected.pos
                    );
                    assert_eq!(
                        actual.height, expected.height,
                        "case {case_id} {} {:?} height",
                        s.name, expected.pos
                    );
                    assert_eq!(
                        actual.minimum, expected.minimum,
                        "case {case_id} {} {:?} minimum",
                        s.name, expected.pos
                    );
                    assert_eq!(
                        actual.sections, expected.sections,
                        "case {case_id} {} {:?} sections",
                        s.name, expected.pos
                    );
                    assert_eq!(
                        actual.gaps, expected.gaps,
                        "case {case_id} {} {:?} gaps",
                        s.name, expected.pos
                    );
                    let mut offset = 0;
                    for [count, value] in expected.light {
                        for i in offset..offset + count {
                            let p = IVec3::new(
                                expected.pos.0 * 16 + (i % 16) as i32,
                                (i / 256) as i32,
                                expected.pos.1 * 16 + ((i / 16) % 16) as i32,
                            );
                            let actual =
                                (lighting.get(Kind::Sky, p) << 4) | lighting.get(Kind::Block, p);
                            assert_eq!(
                                actual, value as u8,
                                "case {case_id} {} {p:?} light",
                                s.name
                            );
                        }
                        offset += count;
                    }
                    assert_eq!(offset, 65536);
                }
                for (&pos, chunk) in &mut world.chunks {
                    Arc::make_mut(chunk).light = lighting.render_snapshot(pos);
                }
                for q in s.queries {
                    lighting.sky_subtracted = q[0] as u8;
                    let p = IVec3::new(q[1], q[2], q[3]);
                    let actual = [
                        lighting.get(Kind::Sky, p),
                        lighting.get(Kind::Block, p),
                        lighting.neighbor_light(&world, Kind::Sky, p),
                        lighting.neighbor_light(&world, Kind::Block, p),
                        lighting.combined(&world, p, true),
                        lighting.unsubtracted(&world, p),
                    ]
                    .map(i32::from);
                    assert_eq!(
                        actual,
                        q[4..],
                        "case {case_id} {} query {p:?} subtract {}",
                        s.name,
                        q[0]
                    );
                    assert_eq!(
                        world.light_uv(p, 0),
                        [q[7] as u8 * 16, q[6] as u8 * 16, 0, 0],
                        "render query case {case_id} {} {p:?}",
                        s.name
                    );
                }
                lighting.sky_subtracted = 0;
                if s.name == "new_sections" {
                    lighting =
                        serde_json::from_slice(&serde_json::to_vec(&lighting).unwrap()).unwrap();
                }
            }
        }
    }
}
