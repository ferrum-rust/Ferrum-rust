//! The worker owns the ordered falling/item/player entity phase and section queries.
use super::*;
use crate::gameplay::xp_orb::{
    motion::{Experience, OrbWorld, Target},
    Orb,
};
use crate::inventory::Stack;
use crate::vanilla::{
    drops::{DropEvent, DropStack},
    item_entity::{self, ItemEffect, ItemEntity, ItemWorld},
    movement::{Aabb, MovementWorld},
    spawning::EntityRandom,
};
use glam::DVec3;
use serde::{Deserialize, Serialize};
#[cfg(test)]
#[path = "runtime_entity_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntityKey {
    Falling(u64),
    Item(u64),
    Orb(i32),
    Player,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveOrb {
    pub body: Orb,
    pub section: Option<[i32; 3]>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayerFrame {
    pub name: String,
    pub position: DVec3,
    pub bounds: Aabb,
    pub riding_bounds: Option<Aabb>,
    pub yaw: f32,
    pub pitch: f32,
    pub eye_height: f32,
    pub alive: bool,
    pub spectator: bool,
    #[serde(default)]
    pub sneaking: bool,
    #[serde(default)]
    pub grounded: bool,
    #[serde(default)]
    pub eyes_in_water: bool,
    #[serde(default)]
    pub haste: Option<i32>,
    #[serde(default)]
    pub fatigue: Option<i32>,
    #[serde(default = "initial_acknowledgement")]
    pub movement_acknowledged: bool,
}
fn initial_acknowledgement() -> bool {
    true
}
impl PlayerFrame {
    pub fn standing(name: impl Into<String>, position: DVec3) -> Self {
        Self {
            name: name.into(),
            position,
            bounds: Aabb::player(position),
            riding_bounds: None,
            yaw: 0.,
            pitch: 0.,
            eye_height: 1.62,
            alive: true,
            spectator: false,
            sneaking: false,
            grounded: false,
            eyes_in_water: false,
            haste: None,
            fatigue: None,
            movement_acknowledged: true,
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.is_empty()
                && self.name.chars().count() <= 16
                && self.position.is_finite()
                && self.yaw.is_finite()
                && self.pitch.is_finite()
                && self.eye_height.is_finite(),
            "Invalid native player frame"
        );
        ensure!(
            self.position.abs().max_element() <= 30_000_000.,
            "Native player exceeds world bounds"
        );
        for b in std::iter::once(&self.bounds).chain(&self.riding_bounds) {
            ensure!(
                b.min.is_finite()
                    && b.max.is_finite()
                    && b.min.cmple(b.max).all()
                    && (b.max - b.min).max_element() <= 32.,
                "Invalid native player bounds"
            );
        }
        Ok(())
    }
    fn pickup_bounds(&self) -> Aabb {
        if let Some(rider) = self.riding_bounds {
            Aabb {
                min: self.bounds.min.min(rider.min),
                max: self.bounds.max.max(rider.max),
            }
            .expand(DVec3::new(1., 0., 1.))
        } else {
            self.bounds.expand(DVec3::new(1., 0.5, 1.))
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EntityEffect {
    BreakProgress {
        position: [i32; 3],
        stage: i32,
    },
    BlockBreak {
        position: [i32; 3],
        state: u16,
    },
    Exhaustion {
        amount: f32,
    },
    ToolBreak {
        id: u16,
        pitch: f32,
    },
    WorldEvent {
        position: [i32; 3],
        id: i32,
    },
    Item(ItemEffect),
    Stat {
        player: String,
        id: String,
    },
    Pickup {
        player: String,
        item_id: u64,
        count: u8,
    },
    OrbPickup {
        player: String,
        orb_id: i32,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NativeEntities {
    pub order: Vec<EntityKey>,
    pub next_item: u64,
    #[serde(with = "live_map")]
    pub live: BTreeMap<u64, ItemEntity>,
    #[serde(default)]
    pub orbs: BTreeMap<i32, LiveOrb>,
    #[serde(default)]
    pub experience: Experience,
    #[serde(default)]
    pub next_entity_id: i32,
    #[serde(default)]
    pub player_entity_id: Option<i32>,
    #[serde(default)]
    pub player_ticks: i32,
    #[serde(with = "section_map")]
    sections: BTreeMap<[i32; 3], Vec<EntityKey>>,
    pub math_state: Option<u64>,
    pub inventory_random: Option<(u64, Option<f64>)>,
    pub player: Option<PlayerFrame>,
    pub player_random: Option<u64>,
    pub effects: Vec<EntityEffect>,
    /// Earlier schemas saved spawn requests, without an item lifetime or pickup ledger.
    pub legacy_drop_prefix: usize,
    #[serde(default)]
    pub digging: Digging,
    #[serde(default)]
    pub xp_owned_after: Option<usize>,
}
mod live_map {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        m: &BTreeMap<u64, ItemEntity>,
        s: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        m.values().collect::<Vec<_>>().serialize(s)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> std::result::Result<BTreeMap<u64, ItemEntity>, D::Error> {
        let mut out = BTreeMap::new();
        for e in Vec::<ItemEntity>::deserialize(d)? {
            let id = e.id;
            if out.insert(id, e).is_some() {
                return Err(serde::de::Error::custom("Duplicate native item entity"));
            }
        }
        Ok(out)
    }
}
mod section_map {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        m: &BTreeMap<[i32; 3], Vec<EntityKey>>,
        s: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        m.iter().collect::<Vec<_>>().serialize(s)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> std::result::Result<BTreeMap<[i32; 3], Vec<EntityKey>>, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Entry {
            Key(EntityKey),
            LegacyItem(u64),
        }
        let mut out = BTreeMap::new();
        for (p, entries) in Vec::<([i32; 3], Vec<Entry>)>::deserialize(d)? {
            let ids = entries
                .into_iter()
                .map(|entry| match entry {
                    Entry::Key(key) => key,
                    Entry::LegacyItem(id) => EntityKey::Item(id),
                })
                .collect();
            if out.insert(p, ids).is_some() {
                return Err(serde::de::Error::custom("Duplicate native entity section"));
            }
        }
        Ok(out)
    }
}
impl NativeEntities {
    pub(super) fn fresh() -> Self {
        Self {
            xp_owned_after: Some(0),
            ..Self::default()
        }
    }
    pub fn validate(&self, falling: &FallingManager, drop_count: usize) -> Result<()> {
        ensure!(
            self.legacy_drop_prefix <= drop_count,
            "Invalid legacy drop boundary"
        );
        ensure!(
            self.xp_owned_after.is_none_or(|n| n <= drop_count),
            "Invalid legacy XP boundary"
        );
        ensure!(
            self.math_state
                .into_iter()
                .chain(self.player_random)
                .chain(self.inventory_random.map(|r| r.0))
                .all(|r| r < 1 << 48),
            "Invalid native entity random state"
        );
        if let Some(frame) = &self.player {
            frame.validate()?;
        }
        ensure!(
            self.inventory_random
                .and_then(|s| s.1)
                .is_none_or(f64::is_finite),
            "Invalid cached inventory Gaussian"
        );
        let mut order = HashSet::new();
        ensure!(
            self.order.iter().all(|key| order.insert(*key)),
            "Duplicate native entity iteration key"
        );
        ensure!(
            order.len()
                == self.live.len()
                    + self.orbs.len()
                    + falling.live.len()
                    + usize::from(order.contains(&EntityKey::Player)),
            "Native entity iteration count mismatch"
        );
        ensure!(
            falling
                .live
                .iter()
                .all(|e| order.contains(&EntityKey::Falling(e.id))),
            "Missing falling entity iteration key"
        );
        let mut indexed = HashSet::new();
        for (key, ids) in &self.sections {
            for id in ids {
                ensure!(
                    indexed.insert(*id)
                        && match *id {
                            EntityKey::Item(id) =>
                                self.live.get(&id).is_some_and(|e| e.section == Some(*key)),
                            EntityKey::Orb(id) =>
                                self.orbs.get(&id).is_some_and(|e| e.section == Some(*key)),
                            _ => false,
                        },
                    "Invalid native item section index"
                );
            }
        }
        for (&id, e) in &self.live {
            ensure!(
                e.id == id
                    && id < self.next_item
                    && order.contains(&EntityKey::Item(id))
                    && e.random_state < 1 << 48
                    && e.position.is_finite()
                    && e.velocity.is_finite()
                    && e.bounds.min.is_finite()
                    && e.bounds.max.is_finite()
                    && e.bounds.min.cmple(e.bounds.max).all()
                    && item_entity::facts(e.stack.id).is_some()
                    && (e.dead || e.stack.count > 0),
                "Invalid live native item"
            );
            ensure!(
                e.section.is_some() == indexed.contains(&EntityKey::Item(id)),
                "Missing item section entry"
            );
        }
        for (&id, e) in &self.orbs {
            ensure!(
                id == e.body.id
                    && order.contains(&EntityKey::Orb(id))
                    && e.body.random_state < 1 << 48
                    && e.body.position.is_finite()
                    && e.body.velocity.is_finite(),
                "Invalid native experience orb"
            );
            ensure!(
                e.section.is_some() == indexed.contains(&EntityKey::Orb(id)),
                "Missing native experience orb section"
            );
        }
        Ok(())
    }
    pub(super) fn legacy(falling: &FallingManager, drop_count: usize) -> Self {
        Self {
            order: falling
                .live
                .iter()
                .map(|e| EntityKey::Falling(e.id))
                .collect(),
            legacy_drop_prefix: drop_count,
            xp_owned_after: Some(drop_count),
            ..Self::default()
        }
    }
    pub fn with_math<T>(&mut self, f: impl FnOnce(&mut JavaRandom) -> T) -> T {
        let mut math = self
            .math_state
            .map(JavaRandom::from_state)
            .unwrap_or_else(|| JavaRandom::new(JavaRandom::independent_seed()));
        let result = f(&mut math);
        self.math_state = Some(math.state());
        result
    }
    pub fn allocate_entity_id(&mut self) -> i32 {
        let id = self.next_entity_id;
        self.next_entity_id = id.wrapping_add(1);
        id
    }
    pub(super) fn register_player_id(&mut self) {
        if self.player_entity_id.is_none() {
            self.player_entity_id = Some(self.allocate_entity_id());
        }
    }
    pub(super) fn prepare_player_constructor(&mut self) {
        self.register_player_id();
        self.with_math(|r| {
            r.next_double();
            r.next_double();
            r.next_double();
        });
    }
    fn remove_section(&mut self, slot: &mut Option<[i32; 3]>, key: EntityKey) {
        if let Some(section) = slot.take() {
            if let Some(ids) = self.sections.get_mut(&section) {
                ids.retain(|id| *id != key);
                if ids.is_empty() {
                    self.sections.remove(&section);
                }
            }
        }
    }
    fn locate(
        &mut self,
        slot: &mut Option<[i32; 3]>,
        key: EntityKey,
        position: DVec3,
        loaded: bool,
    ) {
        let raw = (position / 16.).floor().as_ivec3().to_array();
        if *slot == Some(raw) && loaded {
            return;
        }
        self.remove_section(slot, key);
        if loaded {
            let section = [raw[0], raw[1].clamp(0, 15), raw[2]];
            self.sections.entry(section).or_default().push(key);
            *slot = Some(section);
        }
    }
    fn query(&self, bounds: Aabb) -> Vec<EntityKey> {
        let min = ((bounds.min - DVec3::splat(2.)) / 16.).floor().as_ivec3();
        let max = ((bounds.max + DVec3::splat(2.)) / 16.).floor().as_ivec3();
        let mut out = Vec::new();
        for x in min.x..=max.x {
            for z in min.z..=max.z {
                for y in min.y.clamp(0, 15)..=max.y.clamp(0, 15) {
                    if let Some(ids) = self.sections.get(&[x, y, z]) {
                        out.extend(ids.iter().copied().filter(|id| {
                            match *id {
                                EntityKey::Item(id) => self
                                    .live
                                    .get(&id)
                                    .is_some_and(|e| e.bounds.intersects(bounds)),
                                EntityKey::Orb(id) => self
                                    .orbs
                                    .get(&id)
                                    .is_some_and(|e| e.body.bounds.intersects(bounds)),
                                _ => false,
                            }
                        }));
                    }
                }
            }
        }
        out
    }
    fn spawn(
        &mut self,
        hooks: &dyn Hooks,
        position: DVec3,
        stack: Stack,
        delay: i32,
        loaded: bool,
    ) -> Option<u64> {
        let id = self.next_item;
        self.allocate_entity_id();
        self.next_item = id.checked_add(1).expect("Native item entity ID exhausted");
        let random_state = hooks.item_random_state(id);
        let mut item =
            self.with_math(|math| ItemEntity::new(id, position, stack, math, random_state));
        item.pickup_delay = delay;
        if !loaded {
            return None;
        }
        self.locate(&mut item.section, EntityKey::Item(id), position, true);
        self.live.insert(id, item);
        self.order.push(EntityKey::Item(id));
        Some(id)
    }
    fn spawn_orb(
        &mut self,
        hooks: &dyn Hooks,
        position: DVec3,
        value: i32,
        loaded: bool,
    ) -> Result<()> {
        let id = self.allocate_entity_id();
        ensure!(!self.orbs.contains_key(&id), "Experience orb ID exhausted");
        let random_state = hooks.orb_random_state(id);
        let orb = self.with_math(|math| Orb::new(id, position, value, math, random_state));
        if loaded {
            let mut section = None;
            let key = EntityKey::Orb(id);
            self.locate(&mut section, key, position, true);
            self.orbs.insert(id, LiveOrb { body: orb, section });
            self.order.push(key);
        }
        Ok(())
    }
    pub(super) fn emit(
        &mut self,
        hooks: &dyn Hooks,
        world: &World,
        history: &mut Vec<DropEvent>,
        event: DropEvent,
    ) -> Result<()> {
        if let DropEvent::Item {
            position,
            stack,
            pickup_delay,
        } = &event
        {
            let mut item = item_entity::stack_from_metadata(
                stack.id,
                stack.count,
                u16::try_from(stack.meta)?,
            )?;
            if let Some(tag) = &stack.nbt {
                item = item.with_nbt(tag.clone());
            }
            let p = DVec3::from_array(*position);
            ensure!(p.is_finite(), "Invalid dropped item position");
            self.spawn(
                hooks,
                p,
                item,
                *pickup_delay,
                world.chunks.contains_key(&cp(p.floor().as_ivec3())),
            );
        }
        if let DropEvent::Experience { position, value } = &event {
            let p = DVec3::from_array(*position);
            ensure!(p.is_finite(), "Invalid experience orb position");
            self.spawn_orb(
                hooks,
                p,
                *value,
                world.chunks.contains_key(&cp(p.floor().as_ivec3())),
            )?;
        }
        history.push(event);
        Ok(())
    }
    pub(super) fn spill(
        &mut self,
        hooks: &dyn Hooks,
        world: &World,
        history: &mut Vec<DropEvent>,
        tile: &LiveContainer,
    ) -> Result<()> {
        let mut rng = self
            .inventory_random
            .map(|(s, g)| EntityRandom::from_state(s, g))
            .unwrap_or_else(|| EntityRandom::new(JavaRandom::independent_seed()));
        for stack in tile.slots.iter().flatten() {
            let mut offset = || f64::from(rng.random.next_float() * 0.8 + 0.1);
            let p = IVec3::from_array(tile.position).as_dvec3()
                + DVec3::new(offset(), offset(), offset());
            let mut remaining = stack.count;
            while remaining > 0 {
                let count = ((rng.random.next_int(21) + 10) as u8).min(remaining);
                remaining -= count;
                let mut split = stack.copied();
                split.count = count;
                let spawned = self.spawn(
                    hooks,
                    p,
                    split,
                    0,
                    world.chunks.contains_key(&cp(p.floor().as_ivec3())),
                );
                let sigma = 0.05f32 as f64;
                let velocity = DVec3::new(
                    rng.next_gaussian() * sigma,
                    rng.next_gaussian() * sigma + 0.2f32 as f64,
                    rng.next_gaussian() * sigma,
                );
                if let Some(id) = spawned {
                    self.live.get_mut(&id).unwrap().velocity = velocity;
                }
                history.push(DropEvent::Item {
                    position: p.to_array(),
                    pickup_delay: 0,
                    stack: DropStack {
                        id: stack.id,
                        count,
                        meta: i32::from(if crate::vanilla::armor::maximum_damage(stack.id) > 0 {
                            stack.damage
                        } else {
                            stack.meta
                        }),
                        nbt: stack.nbt.as_deref().cloned(),
                    },
                });
            }
        }
        self.inventory_random = Some(rng.state());
        Ok(())
    }
}

impl Runtime {
    /// Stacks have already been removed from a staged inventory. All failure
    /// checks precede mutation of the corresponding live item/RNG state.
    pub(super) fn throw_stacks(&mut self, stacks: Vec<Stack>, track_thrower: bool) -> Result<bool> {
        if stacks.is_empty() {
            return Ok(true);
        }
        let b = &mut self.backend;
        let Some(frame) = b.entities.player.clone() else {
            return Ok(false);
        };
        let Some(state) = b.entities.player_random else {
            return Ok(false);
        };
        let p = item_entity::throw_position(frame.position,frame.eye_height);
        if !b.world.chunks.contains_key(&cp(p.floor().as_ivec3())) {
            return Ok(false);
        }
        ensure!(
            b.entities
                .next_item
                .checked_add(stacks.len() as u64)
                .is_some(),
            "Native item ID exhausted"
        );
        ensure!(
            stacks
                .iter()
                .all(|s| s.count > 0 && item_entity::facts(s.id).is_some()),
            "Invalid player throw stack"
        );
        let mut random = JavaRandom::from_state(state);
        for stack in stacks {
            let record = DropEvent::Item {
                position: p.to_array(),
                pickup_delay: 40,
                stack: DropStack {
                    id: stack.id,
                    count: stack.count,
                    meta: i32::from(if crate::vanilla::armor::maximum_damage(stack.id) > 0 {
                        stack.damage
                    } else {
                        stack.meta
                    }),
                    nbt: stack.nbt.as_deref().cloned(),
                },
            };
            let id = b
                .entities
                .spawn(b.hooks.as_ref(), p, stack, 40, true)
                .expect("Checked drop chunk");
            let item = b.entities.live.get_mut(&id).expect("Inserted native item");
            item.velocity = item_entity::throw_velocity(frame.yaw, frame.pitch, &mut random);
            if track_thrower {
                item.thrower = Some(frame.name.clone());
                b.entities.effects.push(EntityEffect::Stat {
                    player: frame.name.clone(),
                    id: "stat.drop".into(),
                });
            }
            b.drops.push(record);
        }
        b.entities.player_random = Some(random.state());
        Ok(true)
    }
    pub fn set_player_frame(&mut self, frame: PlayerFrame) -> Result<()> {
        frame.validate()?;
        let entities = &mut self.backend.entities;
        entities.register_player_id();
        if !entities.order.contains(&EntityKey::Player) {
            entities.order.push(EntityKey::Player);
        }
        if entities.player_random.is_none() {
            entities.player_random = Some(
                self.backend
                    .initial_player
                    .map(|p| p.random_state)
                    .unwrap_or_else(|| self.backend.hooks.player_random_state()),
            );
        }
        entities.player = Some(frame);
        Ok(())
    }
    fn entity_area_loaded(&self, position: DVec3) -> bool {
        let p = position.floor().as_ivec3();
        ((p.x - 32).div_euclid(16)..=(p.x + 32).div_euclid(16)).all(|x| {
            ((p.z - 32).div_euclid(16)..=(p.z + 32).div_euclid(16))
                .all(|z| self.provider.contains((x, z)))
        })
    }
    pub fn tick_entities(&mut self) -> Result<()> {
        let mut index = 0;
        while index < self.backend.entities.order.len() {
            let key = self.backend.entities.order[index];
            let dead = match key {
                EntityKey::Falling(id) => {
                    let at = self
                        .backend
                        .falling
                        .live
                        .iter()
                        .position(|e| e.id == id)
                        .ok_or_else(|| anyhow::anyhow!("Missing queued falling entity"))?;
                    let mut e = self.backend.falling.live.remove(at);
                    let result = if self.entity_area_loaded(e.body.position) {
                        self.tick_falling_entity(&mut e)
                    } else {
                        Ok(())
                    };
                    let dead = e.body.dead;
                    if !dead {
                        self.backend.falling.live.insert(at, e);
                    }
                    if let Err(error) = result {
                        if dead {
                            self.backend.entities.order.remove(index);
                        }
                        return Err(error);
                    }
                    dead
                }
                EntityKey::Item(id) => {
                    let mut e = self
                        .backend
                        .entities
                        .live
                        .remove(&id)
                        .ok_or_else(|| anyhow::anyhow!("Missing queued item entity"))?;
                    let mut result = Ok(());
                    if !e.dead && self.entity_area_loaded(e.position) {
                        if e.section.is_some() {
                            result = e.tick(&mut ItemAccess {
                                access: self.access(),
                            });
                        }
                        let loaded = self.provider.contains(cp(e.position.floor().as_ivec3()));
                        self.backend
                            .entities
                            .locate(&mut e.section, key, e.position, loaded);
                    }
                    let dead = e.dead;
                    if dead {
                        self.backend.entities.remove_section(&mut e.section, key);
                    } else {
                        self.backend.entities.live.insert(id, e);
                    }
                    if let Err(error) = result {
                        if dead {
                            self.backend.entities.order.remove(index);
                        }
                        return Err(error);
                    }
                    dead
                }
                EntityKey::Orb(id) => {
                    let mut e = self
                        .backend
                        .entities
                        .orbs
                        .remove(&id)
                        .ok_or_else(|| anyhow::anyhow!("Missing queued experience orb"))?;
                    let mut result = Ok(());
                    if !e.body.dead && self.entity_area_loaded(e.body.position) {
                        if e.section.is_some() {
                            let target = self.backend.entities.player.as_ref().map(|p| Target {
                                position: p.position,
                                eye: p.position + DVec3::Y * f64::from(p.eye_height),
                                spectator: p.spectator,
                            });
                            let mut sounds = Vec::new();
                            result = e.body.tick(
                                &mut ItemAccess {
                                    access: self.access(),
                                },
                                target,
                                &mut sounds,
                            );
                            for (name, volume, pitch) in sounds {
                                self.backend.entities.effects.push(EntityEffect::Item(
                                    ItemEffect::Sound {
                                        position: e.body.position,
                                        name: name.into(),
                                        volume,
                                        pitch,
                                    },
                                ));
                            }
                        }
                        let loaded = self
                            .provider
                            .contains(cp(e.body.position.floor().as_ivec3()));
                        self.backend
                            .entities
                            .locate(&mut e.section, key, e.body.position, loaded);
                    }
                    let dead = e.body.dead;
                    if dead {
                        self.backend.entities.remove_section(&mut e.section, key);
                    } else {
                        self.backend.entities.orbs.insert(id, e);
                    }
                    if let Err(error) = result {
                        if dead {
                            self.backend.entities.order.remove(index);
                        }
                        return Err(error);
                    }
                    dead
                }
                EntityKey::Player => {
                    if self
                        .backend
                        .entities
                        .player
                        .as_ref()
                        .is_some_and(|f| self.entity_area_loaded(f.position))
                    {
                        self.tick_digging()?;
                        self.close_unusable_window()?;
                        self.pickup_nearby_items()?;
                    }
                    false
                }
            };
            if dead {
                self.backend.entities.order.remove(index);
            } else {
                index += 1;
            }
        }
        Ok(())
    }
    fn pickup_nearby_items(&mut self) -> Result<()> {
        if self
            .backend
            .entities
            .player
            .as_ref()
            .is_some_and(|p| !self.entity_area_loaded(p.position))
        {
            return Ok(());
        }
        let b = &mut self.backend;
        if b.entities.player.is_none() {
            return Ok(());
        }
        b.entities.player_ticks = b.entities.player_ticks.wrapping_add(1);
        if b.entities.experience.cooldown > 0 {
            b.entities.experience.cooldown -= 1;
        }
        let Some(frame) = b
            .entities
            .player
            .clone()
            .filter(|p| p.alive && !p.spectator)
        else {
            return Ok(());
        };
        let Some(player) = &mut b.player_inventory else {
            return Ok(());
        };
        let ids = b.entities.query(frame.pickup_bounds());
        for key in ids {
            if let EntityKey::Orb(id) = key {
                let mut sounds = Vec::new();
                let orb = b.entities.orbs.get_mut(&id).expect("Indexed orb missing");
                if orb.body.collect(
                    &mut b.entities.experience,
                    b.entities.player_ticks,
                    &mut sounds,
                ) {
                    for (name, volume, pitch) in sounds {
                        b.entities
                            .effects
                            .push(EntityEffect::Item(ItemEffect::Sound {
                                position: frame.position,
                                name: name.into(),
                                volume,
                                pitch,
                            }));
                        if name == "random.orb" {
                            b.entities.effects.push(EntityEffect::OrbPickup {
                                player: frame.name.clone(),
                                orb_id: id,
                            });
                        }
                    }
                }
                continue;
            }
            let EntityKey::Item(id) = key else {
                continue;
            };
            let item = b.entities.live.get_mut(&id).unwrap();
            if item.dead {
                continue;
            }
            let result = item.pickup(&mut player.inventory, &frame.name, player.creative, false)?;
            if result.removed_count > 0 {
                player.revision = player
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("Native player inventory revision exhausted"))?;
            }
            for (player, id) in result.stats {
                b.entities.effects.push(EntityEffect::Stat { player, id });
            }
            if let Some((volume, pitch)) = result.sound {
                b.entities
                    .effects
                    .push(EntityEffect::Item(ItemEffect::Sound {
                        position: frame.position,
                        name: "random.pop".into(),
                        volume,
                        pitch,
                    }));
            }
            if let Some(count) = result.announced_count {
                b.entities.effects.push(EntityEffect::Pickup {
                    player: frame.name.clone(),
                    item_id: id,
                    count,
                });
            }
        }
        Ok(())
    }
}
struct ItemAccess<'a> {
    access: Access<'a>,
}
impl MovementWorld for ItemAccess<'_> {
    fn get(&self, p: IVec3) -> u16 {
        self.access
            .get_block(p)
            .expect("Native item block read failed")
    }
    fn block_boxes(&self, p: IVec3, state: u16, out: &mut Vec<Aabb>) {
        self.access
            .with(|_, b| MovementWorld::block_boxes(&b.world, p, state, out));
    }
    fn loaded_column(&self, x: i32, z: i32) -> bool {
        self.access
            .with(|p, _| p.contains((x.div_euclid(16), z.div_euclid(16))))
    }
}
impl ItemWorld for ItemAccess<'_> {
    fn full_collision_cube(&mut self, p: IVec3) -> Result<bool> {
        let state = self.access.get_block(p)?;
        let tile = self.access.with(|_, b| b.world.piston_at(p));
        Ok(crate::vanilla::collision_shapes::full_collision_cube(
            p,
            state,
            |q| self.get(q),
            tile,
        ))
    }
    fn merge_nearby(&mut self, e: &mut ItemEntity) -> Result<()> {
        self.access.with(|_, b| {
            let ids = b.entities.query(e.bounds.expand(DVec3::new(0.5, 0., 0.5)));
            for key in ids {
                let EntityKey::Item(id) = key else {
                    continue;
                };
                if let Some(target) = b.entities.live.get_mut(&id) {
                    ItemEntity::combine(e, target);
                }
            }
        });
        Ok(())
    }
    fn contact(&mut self, p: IVec3, state: u16, e: &mut ItemEntity) -> Result<()> {
        let hooks = self.access.with(|_, b| b.hooks.clone());
        hooks.item_contact(&mut self.access, p, state, e)
    }
    fn raining_at(&self, p: IVec3) -> bool {
        let hooks = self.access.with(|_, b| b.hooks.clone());
        hooks.raining_at(&self.access, p)
    }
    fn effect(&mut self, e: ItemEffect) {
        self.access
            .with(|_, b| b.entities.effects.push(EntityEffect::Item(e)));
    }
}
impl OrbWorld for ItemAccess<'_> {
    fn full_collision_cube(&mut self, p: IVec3) -> Result<bool> {
        ItemWorld::full_collision_cube(self, p)
    }
    fn raining_at(&self, p: IVec3) -> bool {
        ItemWorld::raining_at(self, p)
    }
}
