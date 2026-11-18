//! Local villager offers, merchant input matching and atomic inventory exchange.
use crate::{
    gameplay::{Mob, MobKind, Simulation},
    inventory::Stack,
    world::World,
    worldgen, Options,
};
use glam::Vec3;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use crate::inventory::Inventory;
#[path = "villager_careers.rs"]
pub mod careers;
#[path = "villager_goals.rs"]
pub mod goals;
#[path = "villager_homes.rs"]
pub mod homes;
#[path = "villager_merchant.rs"]
pub mod merchant;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Offer {
    pub cost: Stack,
    pub second: Option<Stack>,
    pub output: Stack,
    pub uses: i32,
    pub max_uses: i32,
    #[serde(default = "supported_offer")]
    pub reward_exp: bool,
    #[serde(default = "supported_offer")]
    pub supported: bool,
}
fn supported_offer() -> bool {
    true
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trader {
    pub offers: Vec<Offer>,
    #[serde(default)]
    pub profession: u8,
    #[serde(default = "supported_offer")]
    pub initialized: bool,
    #[serde(default)]
    pub career: careers::Career,
    #[serde(default)]
    pub custom_name: Option<String>,
    #[serde(skip)]
    pub customer: Option<String>,
    #[serde(skip)]
    pub pending: Vec<careers::TradeEffects>,
}
impl Default for Trader {
    fn default() -> Self {
        let mut trader = Self::seeded(0, 189);
        trader
            .ensure_offers()
            .expect("Original farmer profession is registered");
        trader
    }
}
#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub enum TradeError {
    MissingOffer,
    OutOfStock,
    MissingItems,
    InventoryFull,
    UnsupportedOffer,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportedEntity {
    pub mob: Mob,
    #[serde(default)]
    fire_ticks: i16,
    #[serde(default)]
    living: crate::gameplay::command_entity_state::MobData,
    pub age: i32,
    pub trader: Option<Trader>,
    pub love: u16,
    pub egg_timer: u16,
}
pub fn read_entity(
    tag: &crate::nbt::Tag,
    chunk: crate::world::ChunkPos,
    index: usize,
) -> Option<ImportedEntity> {
    use crate::nbt::Tag;
    let kind = MobKind::from_name(tag.get("id")?.string()?)?;
    let p = tag.get("Pos")?.list()?;
    if p.len() != 3 {
        return None;
    }
    let position = Vec3::new(
        p[0].number()? as f32,
        p[1].number()? as f32,
        p[2].number()? as f32,
    );
    if !position.is_finite() || position.x.abs() > 30_000_000. || position.z.abs() > 30_000_000. {
        return None;
    }
    let uuid = match (tag.get("UUIDMost"), tag.get("UUIDLeast")) {
        (Some(Tag::Long(a)), Some(Tag::Long(b))) => (*a as u64).rotate_left(17) ^ *b as u64,
        _ => index as u64,
    };
    let id = worldgen::hash(chunk.0, chunk.1, uuid) | (1 << 62);
    let yaw = tag
        .get("Rotation")
        .and_then(Tag::list)
        .and_then(|l| l.first())
        .and_then(Tag::number)
        .unwrap_or(0.) as f32;
    let trader = if kind == MobKind::Villager {
        let profession = (crate::vanilla::food::nbt_int(tag.get("Profession")) % 5).max(0) as u8;
        let offers = tag
            .get("Offers")
            .and_then(|t| t.get("Recipes"))
            .and_then(Tag::list)
            .map(|list| {
                list.iter()
                    .filter_map(|t| {
                        Some(Offer {
                            cost: crate::inventory::from_nbt(t.get("buy")?)?,
                            second: t.get("buyB").and_then(crate::inventory::from_nbt),
                            output: crate::inventory::from_nbt(t.get("sell")?)?,
                            uses: crate::vanilla::food::nbt_int(t.get("uses")),
                            max_uses: t
                                .get("maxUses")
                                .filter(|tag| tag.number().is_some())
                                .map_or(7, |value| crate::vanilla::food::nbt_int(Some(value))),
                            reward_exp: match t.get("rewardExp") {
                                Some(Tag::Byte(value)) => *value != 0,
                                _ => true,
                            },
                            supported: true,
                        })
                    })
                    .collect::<Vec<_>>()
            });
        let mut trader = Trader::spawned(profession);
        trader.initialized = matches!(tag.get("Offers"), Some(Tag::Compound(_)));
        trader.offers = offers.unwrap_or_default();
        trader.career.id = crate::vanilla::food::nbt_int(tag.get("Career"));
        trader.career.level = crate::vanilla::food::nbt_int(tag.get("CareerLevel"));
        trader.career.wealth = crate::vanilla::food::nbt_int(tag.get("Riches"));
        trader.career.willing = (crate::vanilla::food::nbt_int(tag.get("Willing")) as i8) != 0;
        trader.custom_name = tag
            .get("CustomName")
            .and_then(Tag::string)
            .filter(|name| !name.is_empty())
            .map(str::to_owned);
        Some(trader)
    } else {
        None
    };
    let mut living = crate::gameplay::command_entity_state::MobData {
        name: tag.get("CustomName").and_then(Tag::string).unwrap_or_default().to_owned(),
        pitch: tag.get("Rotation").and_then(Tag::list).and_then(|v| v.get(1)).and_then(Tag::number).unwrap_or(0.) as f32,
        invulnerable: crate::player_save::nbt_boolean(tag.get("Invulnerable")),
        no_ai: crate::player_save::nbt_boolean(tag.get("NoAI")),
        silent: crate::player_save::nbt_boolean(tag.get("Silent")),
        persistent: crate::player_save::nbt_boolean(tag.get("PersistenceRequired")),
        absorption: tag.get("AbsorptionAmount").and_then(Tag::number).unwrap_or(0.) as f32,
        ..Default::default()
    };
    if let Tag::Compound(fields) = tag {
        for (key, value) in fields {
            // Preserve species data, including nested horse equipment and owner UUIDs.
            // The simulation writes the live common fields itself when queried via commands.
            if !matches!(key.as_str(), "id"|"Pos"|"Motion"|"Rotation"|"Health"|"HealF"|"Age"|"InLove"|"Fire"
                |"Equipment"|"ActiveEffects"|"AbsorptionAmount"|"CustomName"|"NoAI"|"Invulnerable"|"PersistenceRequired"
                |"Silent"|"UUIDMost"|"UUIDLeast"|"Riding"|"OnGround"|"FallDistance"|"Air"|"HurtTime"|"DeathTime"
                |"Offers"|"Inventory"|"Career"|"CareerLevel"|"Riches"|"Willing"|"Profession") {
                living.extra.insert(key.clone(), crate::vanilla::item_nbt::from_tag(value));
            }
        }
    }
    if let Some(equipment) = tag.get("Equipment").and_then(Tag::list) {
        for (slot, entry) in equipment.iter().take(5).enumerate() {
            if let Some(stack) = crate::inventory::from_nbt(entry) {
                living.equipment.insert(99 + slot as i32, Some(stack));
            }
        }
    }
    if let Some(effects) = tag.get("ActiveEffects").and_then(Tag::list) {
        for effect in effects.iter().filter_map(crate::vanilla::potions::read_effect) {
            let _ = living.effects.load_effect(effect);
        }
    }
    let health = tag.get("HealF").or_else(|| tag.get("Health")).and_then(Tag::number)
        .map_or_else(|| kind.max_health(Some(&living)), |v| v as f32);
    if !health.is_finite() || health <= 0. {
        return None;
    }
    Some(ImportedEntity {
        living,
        fire_ticks: crate::vanilla::food::nbt_int(tag.get("Fire")) as i16,
        mob: Mob {
            id,
            kind,
            position,
            previous: position,
            velocity: tag.get("Motion").and_then(Tag::list).filter(|v| v.len() == 3)
                .and_then(|v| Some(Vec3::new(v[0].number()? as f32,v[1].number()? as f32,v[2].number()? as f32)))
                .filter(|v| v.is_finite()).unwrap_or_default(),
            yaw: (180. + yaw).to_radians(),
            health,
            age: 0,
            turn_ticks: 0,
            hurt: 0,
            fuse: 0,
            swing_ticks: 0,
            death_ticks: 0,
            eat_ticks: 0,
            attack_ticks: 0,
            rose_ticks: 0,
            prev_fuse: 0,
        },
        age: tag.get("Age").and_then(Tag::int).unwrap_or(0),
        trader,
        love: tag
            .get("InLove")
            .and_then(Tag::int)
            .unwrap_or(0)
            .clamp(0, 600) as u16,
        egg_timer: tag
            .get("EggLayTime")
            .and_then(Tag::int)
            .unwrap_or(6000)
            .clamp(1, 12000) as u16,
    })
}
pub fn sync_import(world: &World, sim: &mut Simulation) {
    if !world.imported {
        return;
    }
    // Preserve entity order within each saved chunk, and make fresh-column
    // installation independent of the render world's randomized lookup map.
    let mut fresh = world.chunks.iter()
        .filter(|(pos, _)| !sim.imported_entity_chunks.contains(pos)).collect::<Vec<_>>();
    fresh.sort_unstable_by_key(|(pos, _)| **pos);
    for (&pos, chunk) in fresh {
        sim.imported_entity_chunks.insert(pos);
        for entity in &chunk.entities {
            if sim.mobs.iter().any(|m| m.id == entity.mob.id) {
                continue;
            }
            entity.install(sim);
        }
    }
}
impl ImportedEntity {
    pub(crate) fn install(&self, sim: &mut Simulation) {
        let id = self.mob.id;
        if let Some(trader) = &self.trader { sim.trades.entry(id).or_insert_with(|| trader.clone()); }
        let brain = sim.mob_brains.entry(id).or_default();
        brain.fire_ticks = i32::from(self.fire_ticks);
        sim.command_entities.mobs.entry(id).or_insert_with(|| self.living.clone());
        brain.growing_age = self.age;
        brain.love = self.love;
        brain.egg_timer = self.egg_timer;
        sim.mobs.push(self.mob.clone());
    }
}
#[cfg(test)]
fn consume(inventory: &mut Inventory, cost: Stack) -> bool {
    let mut left = cost.count;
    for slot in inventory.slots.iter_mut().take(36) {
        if let Some(stack) = slot {
            if merchant::matches_cost(stack, &cost) {
                let n = stack.count.min(left);
                stack.count -= n;
                left -= n;
                if stack.count == 0 {
                    *slot = None;
                }
            }
        }
        if left == 0 {
            return true;
        }
    }
    false
}
#[cfg(test)]
impl Trader {
    pub fn trade(&mut self, index: usize, inventory: &mut Inventory) -> Result<Stack, TradeError> {
        let offer = self.offers.get_mut(index).ok_or(TradeError::MissingOffer)?;
        if !offer.supported {
            return Err(TradeError::UnsupportedOffer);
        }
        if offer.uses >= offer.max_uses {
            return Err(TradeError::OutOfStock);
        }
        let mut next = inventory.clone();
        if !consume(&mut next, offer.cost.clone())
            || offer
                .second
                .clone()
                .is_some_and(|cost| !consume(&mut next, cost))
        {
            return Err(TradeError::MissingItems);
        }
        if next.add(offer.output.copied()).is_some() {
            return Err(TradeError::InventoryFull);
        }
        *inventory = next;
        let output = offer.output.copied();
        let events = self.use_recipe(index);
        self.pending.push(events);
        Ok(output)
    }
}
pub fn sync_spawn(options: &Options, world: &World, sim: &mut Simulation) {
    if options.world.is_some() || options.generator != 2 || options.structure_preview.is_some() {
        return;
    }
    let regions = world
        .chunks
        .keys()
        .map(|&(x, z)| (x.div_euclid(16), z.div_euclid(16)))
        .collect::<std::collections::BTreeSet<_>>();
    for (rx, rz) in regions {
        let Some(v) = worldgen::village(rx, rz, options.seed) else {
            continue;
        };
        for i in 0..3u64 {
            let id = v.id.wrapping_add(i);
            let pos = Vec3::new(v.x as f32 + 3.5 + i as f32, v.y as f32, v.z as f32 + 4.5);
            if sim.trades.contains_key(&id) || !world.contains(pos) {
                continue;
            }
            // Trader records remain after death: reloading a chunk must not duplicate villagers.
            sim.trades.insert(id, Trader::spawned(0));
            sim.mobs.push(Mob {
                id,
                kind: MobKind::Villager,
                position: pos,
                previous: pos,
                velocity: Vec3::ZERO,
                yaw: 0.,
                health: 20.,
                age: 0,
                turn_ticks: 0,
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
}
#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    #[test]
    fn imported_actor_order_survives_different_chunk_hash_keys_and_incremental_imports() {
        use crate::{nbt::Tag, world::{Chunk, ChunkMap}};
        use std::sync::Arc;
        let mut source = Vec::new();
        for pos in [(-2, 3), (0, -1), (1, 0)] {
            let mut chunk = Chunk::empty();
            for index in [2, 0, 1] {
                let tag = Tag::Compound([
                    ("id".into(), Tag::String("Pig".into())),
                    ("Pos".into(), Tag::List(vec![Tag::Double(pos.0 as f64 * 16. + index as f64),
                        Tag::Double(70.), Tag::Double(pos.1 as f64 * 16.)])),
                ].into());
                chunk.entities.push(read_entity(&tag, pos, index).unwrap());
            }
            source.push((pos, Arc::new(chunk)));
        }
        let expected = source.iter().flat_map(|(_, c)| c.entities.iter().map(|e| e.mob.id)).collect::<Vec<_>>();
        for seed in 0..8 {
            let mut chunks = ChunkMap::with_hasher(ahash::RandomState::with_seeds(seed, 3, 5, 7));
            chunks.extend(source.iter().rev().cloned());
            let world = World { chunks, spawn: Vec3::ZERO, label: String::new(), save_path: Default::default(),
                edits: Default::default(), imported: true, light_revision: 0 };
            let mut sim = Simulation::default();
            sync_import(&world, &mut sim);
            assert_eq!(sim.mobs.iter().map(|m| m.id).collect::<Vec<_>>(), expected);
            let kept = sim.mobs.remove(0);
            sync_import(&world, &mut sim);
            assert!(!sim.mobs.iter().any(|m| m.id == kept.id), "retained columns must not resurrect entities");
        }
    }
    #[test]
    fn generated_village_has_homes_farms_loot_and_persistent_residents() {
        let options = Options::parse_from([
            "ferrum",
            "--generator",
            "2",
            "--village",
            "--render-distance",
            "2",
            "--fresh",
        ]);
        let world = World::load(&options).unwrap().world;
        let v = worldgen::nearest_village(options.seed);
        assert_eq!(world.get(glam::IVec3::new(v.x, v.y - 1, v.z)) >> 4, 9);
        assert_eq!(world.get(glam::IVec3::new(v.x - 12, v.y, v.z - 9)) >> 4, 64);
        assert_eq!(world.get(glam::IVec3::new(v.x + 22, v.y, v.z)) >> 4, 59);
        assert!(world
            .chunks
            .values()
            .flat_map(|c| &c.containers)
            .any(|(_, c)| c.slots.iter().flatten().any(|s| s.id == 388)));
        let mut sim = Simulation::default();
        sync_spawn(&options, &world, &mut sim);
        assert!(sim.mobs.len() >= 3);
        let before = sim.mobs.len();
        sync_spawn(&options, &world, &mut sim);
        assert_eq!(sim.mobs.len(), before);
        sim.mobs.remove(0);
        let mut restored: Simulation =
            serde_json::from_str(&serde_json::to_string(&sim).unwrap()).unwrap();
        sync_spawn(&options, &world, &mut restored);
        assert_eq!(restored.mobs.len(), before - 1);
    }
    #[test]
    fn imports_legacy_villager_offers_and_does_not_resurrect_killed_entities() {
        use crate::nbt::Tag;
        fn compound(items: Vec<(&str, Tag)>) -> Tag {
            Tag::Compound(items.into_iter().map(|(k, v)| (k.into(), v)).collect())
        }
        fn stack(id: i16, count: i8) -> Tag {
            compound(vec![
                ("id", Tag::Short(id)),
                ("Count", Tag::Byte(count)),
                ("Damage", Tag::Short(0)),
            ])
        }
        let tag = compound(vec![
            ("id", Tag::String("Villager".into())),
            (
                "Pos",
                Tag::List(vec![Tag::Double(1.5), Tag::Double(64.), Tag::Double(1.5)]),
            ),
            ("Age", Tag::Int(-24000)),
            ("Fire", Tag::Short(91)),
            ("Equipment", Tag::List(vec![compound(vec![]), compound(vec![]), compound(vec![]), compound(vec![]), stack(306, 1)])),
            (
                "Offers",
                compound(vec![(
                    "Recipes",
                    Tag::List(vec![compound(vec![
                        ("buy", stack(388, 2)),
                        ("sell", stack(297, 4)),
                        ("uses", Tag::Int(3)),
                        ("maxUses", Tag::Int(8)),
                    ])]),
                )]),
            ),
        ]);
        let entity = read_entity(&tag, (0, 0), 0).unwrap();
        assert_eq!(entity.trader.as_ref().unwrap().offers[0].uses, 3);
        let mut c = crate::world::Chunk::empty();
        c.entities.push(entity.clone());
        let world = World {
            chunks: crate::world::ChunkMap::from([((0, 0), std::sync::Arc::new(c))]),
            spawn: Vec3::ZERO,
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: true,
            light_revision: 0,
        };
        let mut sim = Simulation::default();
        sync_import(&world, &mut sim);
        assert_eq!(sim.mobs.len(), 1);
        assert_eq!(sim.mob_brains[&entity.mob.id].fire_ticks, 91);
        assert_eq!(sim.command_entities.mobs[&entity.mob.id].equipment[&103].as_ref().unwrap().id, 306);
        assert!(sim.is_baby(entity.mob.id));
        sim.mobs.clear();
        sync_import(&world, &mut sim);
        assert!(sim.mobs.is_empty());
    }
    #[test]
    fn trades_are_atomic_and_stock_survives_serialization() {
        // Original profession-0, career-1 fixture, first wheat price 20.
        let mut t = Trader::seeded(0, 13_774_104_864);
        t.ensure_offers().unwrap();
        let mut inv = Inventory::default();
        inv.slots[0] = Some(Stack::new(296, 19));
        assert_eq!(t.trade(0, &mut inv), Err(TradeError::MissingItems));
        assert_eq!(inv.slots[0].as_ref().unwrap().count, 19);
        inv.slots[1] = Some(Stack::new(296, 1));
        assert_eq!(t.trade(0, &mut inv), Ok(Stack::new(388, 1)));
        assert_eq!(
            inv.slots
                .iter()
                .flatten()
                .filter(|s| s.id == 388)
                .map(|s| s.count as u32)
                .sum::<u32>(),
            1
        );
        let t: Trader = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(t.offers[0].uses, 1);
    }
    #[test]
    fn full_inventory_and_sold_out_do_not_charge_customer() {
        // Original profession-0, career-1 fixture, first wheat price 20.
        let mut t = Trader::seeded(0, 13_774_104_864);
        t.ensure_offers().unwrap();
        let mut inv = Inventory::default();
        for slot in inv.slots.iter_mut().take(36) {
            *slot = Some(Stack::new(1, 64));
        }
        inv.slots[0] = Some(Stack::new(388, 64));
        assert_eq!(t.trade(3, &mut inv), Err(TradeError::InventoryFull));
        assert_eq!(inv.slots[0].as_ref().unwrap().count, 64);
        t.offers[3].uses = 0;
        t.offers[3].supported = false;
        assert_eq!(t.trade(3, &mut inv), Err(TradeError::UnsupportedOffer));
        assert_eq!(inv.slots[0].as_ref().unwrap().count, 64);
        t.offers[3].supported = true;
        t.offers[3].uses = 7;
        assert_eq!(t.trade(3, &mut inv), Err(TradeError::OutOfStock));
        assert_eq!(inv.slots[0].as_ref().unwrap().count, 64);
    }
}
