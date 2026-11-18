use crate::{
    assets,
    inventory::{tool, Inventory, Stack, Tool},
    player::Player,
    resources::Resources,
    settings::Settings,
    vanilla::food::{self, Context as FoodContext, FoodState},
    vanilla::{
        armor::{self, DefenseContext, Source as HitSource},
        item_food::{self, Effect, FoodStack, UseState},
        potions::{self, Change, Effects},
        random::JavaRandom,
    },
    world::World,
};
use anyhow::Result;
use glam::{IVec3, Vec3};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[path = "gameplay_presentation.rs"]
mod presentation_copy;

#[path = "mob_species_ai.rs"]
pub(crate) mod species_ai;
#[path = "local_horse.rs"]
mod horse;
#[path = "local_vehicles.rs"]
pub(crate) mod vehicles;
#[path = "mob_shots.rs"]
pub(crate) mod mob_shots;
#[path = "gameplay_bow.rs"]
pub(crate) mod bow;
#[path = "gameplay_projectiles.rs"]
pub(crate) mod projectiles;
#[path = "gameplay_fishing.rs"]
pub(crate) mod fishing;
#[path = "gameplay_maps.rs"]
pub(crate) mod maps;
#[path = "ai.rs"]
pub mod ai;
#[path = "gameplay_redstone.rs"]
mod redstone;
#[path = "gameplay_drops.rs"]
mod drops_local;
#[path = "block_support.rs"]
pub(crate) mod block_support;
#[path = "gameplay_tnt.rs"]
mod explosives;
#[path = "gameplay_hoppers.rs"]
mod hoppers;
#[path = "commands/entity_state.rs"]
pub(crate) mod command_entity_state;
#[path = "local_commands.rs"]
pub mod local_commands;
#[path = "gameplay_command_blocks.rs"]
pub(crate) mod command_blocks;
#[path="commands/world_state.rs"]
pub(crate) mod command_world_state;
#[path = "xp_orb.rs"]
pub mod xp_orb;
use ai::MobBrain;
#[cfg(test)]
#[path = "gameplay_armor_tests.rs"]
mod armor_tests;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Survival,
    Creative,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalTarget {
    Mob(u64),
    Minecart(u64),
    Boat(u64),
    EndCrystal(u64),
    Falling(usize),
    Tnt(usize),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PortalTravel {
    Nether(crate::dimension_world::NetherPortalEntry),
    End,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EndCrystal {
    pub id: u64,
    pub position: glam::DVec3,
    pub bottom: bool,
    #[serde(skip)]
    pub beam_target: Option<glam::Vec3>,
}
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SpawnerSpin {
    pub prev_rot: f64,
    pub rot: f64,
    pub mp_delay: i32,
}
fn deserialize_tile_edits<'de, D>(
    deserializer: D,
) -> std::result::Result<
    Vec<([i32; 3], (u16, Option<crate::vanilla::drops::DropNbt>))>,
    D::Error,
>
where
    D: serde::Deserializer<'de>,
{
    struct TileEditsVisitor;
    impl<'de> serde::de::Visitor<'de> for TileEditsVisitor {
        type Value = Vec<([i32; 3], (u16, Option<crate::vanilla::drops::DropNbt>))>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a sequence of tile edit entries or a legacy map")
        }
        fn visit_seq<A>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
            while let Some(entry) = seq.next_element()? {
                out.push(entry);
            }
            Ok(out)
        }
        fn visit_map<M>(self, mut map: M) -> std::result::Result<Self::Value, M::Error>
        where
            M: serde::de::MapAccess<'de>,
        {
            let mut out = Vec::new();
            while let Some((key, val)) =
                map.next_entry::<String, (u16, Option<crate::vanilla::drops::DropNbt>)>()?
            {
                let mut parts = key
                    .trim_matches(|c| c == '[' || c == ']')
                    .split(',')
                    .filter_map(|s| s.trim().parse::<i32>().ok());
                if let (Some(x), Some(y), Some(z)) = (parts.next(), parts.next(), parts.next()) {
                    out.push(([x, y, z], val));
                }
            }
            Ok(out)
        }
    }
    deserializer.deserialize_any(TileEditsVisitor)
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct DimensionSlice {
    pub edits: Vec<([i32; 3], u16)>,
    pub containers: std::collections::BTreeMap<String, crate::container::Container>,
    pub mobs: Vec<Mob>,
    #[serde(skip)]
    pub dying_mobs: Vec<Mob>,
    #[serde(default, deserialize_with = "live_items::read_drops")]
    pub drops: Vec<Drop>,
    pub xp_orbs: Vec<xp_orb::Orb>,
    pub projectiles: Vec<projectiles::Projectile>,
    pub arrows: Vec<Arrow>,
    pub mob_shots: Vec<mob_shots::Shot>,
    pub primed_tnt: Vec<crate::vanilla::tnt::Primed>,
    pub minecarts: Vec<vehicles::LocalMinecart>,
    pub boats: Vec<vehicles::LocalBoat>,
    pub end_crystals: Vec<EndCrystal>,
    pub block_ticks: crate::block_ticks::BlockTicks,
    #[serde(default, deserialize_with = "deserialize_tile_edits")]
    pub tile_edits: Vec<([i32; 3], (u16, Option<crate::vanilla::drops::DropNbt>))>,
    pub populated_chunks: std::collections::BTreeSet<(i32, i32)>,
    pub imported_container_chunks: std::collections::BTreeSet<crate::world::ChunkPos>,
    pub imported_entity_chunks: std::collections::BTreeSet<crate::world::ChunkPos>,
    pub command_mobs: std::collections::BTreeMap<u64, command_entity_state::MobData>,
    pub mob_brains: std::collections::BTreeMap<u64, MobBrain>,
    pub trades: std::collections::BTreeMap<u64, crate::villagers::Trader>,
    pub note_pitches: std::collections::BTreeMap<String, u8>,
}
#[path = "mob_kind.rs"]
mod mob_kind;
#[path = "mob_spawner.rs"]
mod mob_spawner;
pub use mob_kind::MobKind;
#[path = "natural_mobs.rs"]
mod natural_mobs;
#[path = "mob_construction.rs"]
mod mob_construction;
#[path = "mob_pets.rs"]
mod mob_pets;
#[cfg(test)]
#[path = "mob_world_tests.rs"]
mod mob_world_tests;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mob {
    pub id: u64,
    pub kind: MobKind,
    pub position: Vec3,
    pub previous: Vec3,
    pub velocity: Vec3,
    pub yaw: f32,
    pub health: f32,
    pub age: u32,
    pub turn_ticks: u32,
    pub hurt: u8,
    pub fuse: u8,
    #[serde(skip, default)]
    pub swing_ticks: u8,
    #[serde(skip, default)]
    pub death_ticks: u8,
    #[serde(skip, default)]
    pub eat_ticks: u8,
    #[serde(skip, default)]
    pub attack_ticks: u8,
    #[serde(skip, default)]
    pub rose_ticks: u16,
    #[serde(skip, default)]
    pub prev_fuse: u8,
}
pub use crate::vanilla::item_entity::ItemEntity as Drop;
#[path = "item_pickup_fx.rs"]
pub(crate) mod item_pickup_fx;
#[path = "live_items.rs"]
pub(crate) mod live_items;
#[path="gameplay_throw.rs"]
mod player_throw;
pub(crate) use player_throw::ThrowSource;
#[path = "gameplay_growth.rs"]
pub(crate) mod growth;
#[path = "gameplay_melee.rs"]
mod melee;
#[path = "gameplay_burn.rs"]
mod burn;
#[path = "gameplay_click.rs"]
mod click_feedback;
pub(crate) use click_feedback::actual_dig_state;
#[cfg(test)]
#[path = "gameplay_survival_tests.rs"]
mod survival_tests;
#[path = "click_particles.rs"]
mod click_particles;
#[path = "critical_particles.rs"]
mod critical_particles;
#[cfg(test)]
#[path = "placement_swing_tests.rs"]
mod placement_swing_tests;
pub use click_particles::Particle;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Arrow {
    pub position: Vec3,
    pub previous: Vec3,
    pub velocity: Vec3,
    pub age: u16,
}
/// Transient EntityLivingBase timers on the rendering client. Hurt resistance
/// on the integrated server has its own 20-tick timer and different semantics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HurtCamera {
    pub hurt_time: u8,
    pub death_time: u8,
}
impl HurtCamera {
    pub fn hurt(&mut self) {
        self.hurt_time = 10;
    }
    pub fn tick(&mut self, dead: bool) {
        self.hurt_time = self.hurt_time.saturating_sub(1);
        if dead {
            // onDeathUpdate removes the client entity on its twentieth tick.
            self.death_time = self.death_time.saturating_add(1).min(20);
        } else {
            self.death_time = 0;
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Simulation {
    pub(crate) spawn_seed: i64,
    #[serde(skip)]
    pub(crate) lan: Option<crate::lan::Host>,
    pub(crate) lan_guests:std::collections::BTreeMap<String,crate::lan::SavedGuest>,
    pub command_entities: command_entity_state::State,
    pub(crate) commands: local_commands::CommandState,
    pub world_commands: command_world_state::State,
    pub statistics:std::collections::HashMap<String,i32>,
    pub mode: Mode,
    #[serde(skip)]
    pub local_name: String,
    #[serde(skip)]
    pub presentation: crate::net_presentation::State,
    #[serde(skip)]
    pub server_scene:crate::models::server::Scene,
    pub inventory: Inventory,
    pub health: f32,
    pub max_health: f32,
    pub fire_ticks: i32,
    #[serde(skip)]
    pub(crate) fire_flag: bool,
    pub food: i32,
    pub previous_food: i32,
    pub natural_regeneration: bool,
    #[serde(skip)]
    player_ticks: i32,
    pub saturation: f32,
    pub exhaustion: f32,
    pub food_timer: i32,
    pub absorption: f32,
    pub effects: Effects,
    #[serde(skip)]
    pub death_message_key: Option<&'static str>,
    pub air: i32,
    pub experience: u32,
    pub xp: xp_orb::motion::Experience,
    pub keep_inventory: bool,
    pub bed_spawn: Option<crate::vanilla::respawn::BedSpawn>,
    pub sleep: crate::vanilla::sleep::Sleep,
    pub daylight_cycle: bool,
    pub(crate) send_command_feedback: bool,
    pub world_difficulty: Option<u8>,
    #[serde(skip)]
    pub server_reduced_debug: bool,
    pub difficulty_locked: bool,
    pub hardcore: bool,
    pub flat_world: bool,
    pub dimension: i32,
    pub weather: crate::vanilla::weather::Weather,
    #[serde(default = "absent_total_ticks")]
    pub total_ticks: u64,
    #[serde(skip)]
    pub sleep_notice: Option<(&'static str, u8)>,
    pub mob_loot: bool,
    pub mob_spawning: bool,
    pub xp_orbs: Vec<xp_orb::Orb>,
    #[serde(skip)]
    pub(crate) pickup_fx: item_pickup_fx::Effects,
    pub xp_math_random: u64,
    #[serde(skip, default = "independent_voice_random")]
    voice_random: [u64; 4],
    pub clock: u64,
    #[serde(skip)]
    pub hand_swing: crate::first_person::Swing,
    #[serde(skip)]
    pub equipped:crate::first_person::Equipped,
    #[serde(skip)]
    pub click_controller: crate::vanilla::click_controller::Controller,
    pub world_border: crate::vanilla::click_border::Border,
    pub mobs: Vec<Mob>,
    #[serde(skip)]
    pub dying_mobs: Vec<Mob>,
    #[serde(skip)]
    pub(crate) horse_anims: std::collections::BTreeMap<u64, crate::horse_model::HorseAnim>,
    #[serde(default)]
    pub minecarts: Vec<vehicles::LocalMinecart>,
    #[serde(default)]
    pub boats: Vec<vehicles::LocalBoat>,
    #[serde(default)]
    pub ridden_minecart: Option<u64>,
    #[serde(default)]
    pub ridden_boat: Option<u64>,
    #[serde(skip)]
    pub minecart_window: Option<u64>,
    #[serde(skip)]
    pub show_minecart_window: bool,
    #[serde(default)]
    pub ridden_pig: Option<u64>,
    #[serde(default)]
    pub pig_boost_ticks: u32,
    #[serde(default)]
    pub pig_boost_total: u32,
    #[serde(default)]
    pub pig_speed: f32,
    pub(crate) ridden_horse: Option<u64>,
    #[serde(skip)]
    horse_rider: crate::remote::Rider,
    #[serde(skip)]
    pub(crate) horse_window: Option<u64>,
    #[serde(skip)]
    pub(crate) show_horse_window: bool,
    // Section lists append entities when they first enter or cross a section.
    pick_sections: std::collections::BTreeMap<u64, ([i32; 3], u64, u64)>,
    pick_sequence: u64,
    pub mob_brains: std::collections::BTreeMap<u64, MobBrain>,
    pub trades: std::collections::BTreeMap<u64, crate::villagers::Trader>,
    pub villager_homes: crate::villagers::homes::Homes,
    /// Spawn requests emitted during village iteration are adopted immediately after the village tick.
    pub pending_golem_spawns: Vec<IVec3>,
    #[serde(skip)]
    pub trading_with: Option<u64>,
    #[serde(skip)]
    pub merchant_window: crate::villagers::merchant::MerchantWindow,
    #[serde(deserialize_with="live_items::read_drops")]
    pub drops: Vec<Drop>,
    pub arrows: Vec<Arrow>,
    #[serde(default)]
    pub mob_shots: Vec<mob_shots::Shot>,
    pub projectiles: Vec<projectiles::Projectile>,
    pub fishing: fishing::FishingState,
    pub maps: maps::Maps,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub primed_tnt: Vec<crate::vanilla::tnt::Primed>,
    #[serde(skip)]
    explosion_clouds: Vec<(glam::DVec3,u8)>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    note_pitches: std::collections::BTreeMap<String, u8>,
    #[serde(skip)]
    powered_notes: std::collections::HashSet<IVec3>,
    #[serde(skip)]
    note_events: Vec<(IVec3, u8, u8)>,
    pub containers: std::collections::BTreeMap<String, crate::container::Container>,
    pub imported_container_chunks: std::collections::BTreeSet<crate::world::ChunkPos>,
    pub imported_entity_chunks: std::collections::BTreeSet<crate::world::ChunkPos>,
    pub block_ticks: crate::block_ticks::BlockTicks,
    redstone: redstone::Circuits,
    random_tick_lcg: i32,
    pub rng: u64,
    /// Separate post-constructor RNG states for Entity.rand and the original
    /// EnchantmentHelper shared RNG. World.rand is owned by BlockTicks.
    pub player_random_state: u64,
    pub enchantment_random_state: u64,
    pub next_id: u64,
    pub yaw: f32,
    pub pitch: f32,
    #[serde(skip)]
    pub particles: Vec<Particle>,
    #[serde(skip)]
    pub(crate) critical_emitters: critical_particles::Emitters,
    #[serde(skip, default = "independent_particle_random")]
    pub(crate) particle_random: [u64; 3],
    #[serde(skip)]
    damage_sounds: Vec<(&'static str, f32, f32)>,
    #[serde(skip)]
    pub(crate) pending_world_sounds: Vec<(&'static str, glam::DVec3, f32, f32)>,
    #[serde(skip)]
    pub(crate) pending_changed_blocks: Vec<IVec3>,
    #[serde(default)]
    pub(crate) end_crystals: Vec<EndCrystal>,
    #[serde(default)]
    pub(crate) overworld_spawn: Option<[f32; 3]>,
    #[serde(default)]
    pub(crate) end_dragon_spawned: bool,
    #[serde(default)]
    pub(crate) end_dragon_slain: bool,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub(crate) dimension_slices: std::collections::BTreeMap<i32, DimensionSlice>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub(crate) active_populated_chunks: std::collections::BTreeSet<(i32, i32)>,
    #[serde(skip)]
    pub(crate) teleporter_caches:
        std::collections::BTreeMap<i32, crate::dimension_world::TeleporterCache>,
    #[serde(skip)]
    pub(crate) spawner_spins: std::collections::BTreeMap<[i32; 3], SpawnerSpin>,
    #[serde(skip)]
    pub breaking: f32,
    #[serde(skip)]
    pub break_target: Option<IVec3>,
    #[serde(skip, default)]
    pub pending_sign_edit: Option<IVec3>,
    #[serde(skip)]
    pub eating: u8,
    #[serde(skip)]
    item_use: UseState,
    #[serde(skip)]
    pub hurt_camera: HurtCamera,
    #[serde(skip)]
    hurt_ticks: u8,
    #[serde(skip)]
    last_damage: f32,
    #[serde(skip)]
    pub(crate) place_ticks: u8,
    #[serde(skip)]
    right_click_delay: u8,
    #[serde(skip)]
    right_was_down: bool,
    pub(crate) fall_distance: f32,
}
impl Default for Simulation {
    fn default() -> Self {
        Self {
            spawn_seed: 0,
            command_entities: Default::default(),
            world_commands: Default::default(),
            mode: Mode::Survival,
            statistics:Default::default(),
            commands:Default::default(),
            lan:None,
            lan_guests:Default::default(),
            local_name: String::new(),
            presentation: Default::default(),
            server_scene:Default::default(),
            inventory: Inventory::default(),
            health: 20.0,
            max_health: 20.0,
            fire_ticks: 0,
            fire_flag: false,
            food: 20,
            previous_food: 20,
            natural_regeneration: true,
            player_ticks: 0,
            saturation: 5.0,
            exhaustion: 0.0,
            food_timer: 0,
            absorption: 0.,
            effects: Effects::default(),
            death_message_key: None,
            air: 300,
            experience: 0,
            xp: Default::default(),
            keep_inventory: false,
            bed_spawn: None,
            sleep: Default::default(),
            daylight_cycle: true,
            send_command_feedback: true,
            world_difficulty: None,
            server_reduced_debug: false,
            difficulty_locked: false,
            hardcore: false,
            flat_world: false,
            dimension: 0,
            weather: Default::default(),
            total_ticks: 0,
            sleep_notice: None,
            mob_loot: true,
            mob_spawning: true,
            xp_orbs: Vec::new(),
            pickup_fx: Default::default(),
            xp_math_random: JavaRandom::new(JavaRandom::independent_seed()).state(),
            voice_random: independent_voice_random(),
            clock: 0,
            hand_swing: Default::default(),
            equipped:Default::default(),
            click_controller: Default::default(),
            world_border: Default::default(),
            mobs: Vec::new(),
            dying_mobs: Vec::new(),
            horse_anims: Default::default(),
            minecarts: Vec::new(),
            boats: Vec::new(),
            ridden_minecart: None,
            ridden_boat: None,
            minecart_window: None,
            show_minecart_window: false,
            ridden_pig: None,
            pig_boost_ticks: 0,
            pig_boost_total: 0,
            pig_speed: 0.0,
            ridden_horse: None,
            horse_rider: Default::default(),
            horse_window: None,
            show_horse_window: false,
            pick_sections: Default::default(),
            pick_sequence: 0,
            mob_brains: Default::default(),
            trades: Default::default(),
            villager_homes: Default::default(),
            pending_golem_spawns: Vec::new(),
            trading_with: None,
            merchant_window: Default::default(),
            drops: Vec::new(),
            arrows: Vec::new(),
            mob_shots: Vec::new(),
            projectiles: Vec::new(),
            fishing: fishing::FishingState::default(),
            maps: maps::Maps::default(),
            primed_tnt: Vec::new(),
            explosion_clouds: Vec::new(),
            note_pitches: Default::default(),
            powered_notes: Default::default(),
            note_events: Vec::new(),
            containers: Default::default(),
            imported_container_chunks: Default::default(),
            imported_entity_chunks: Default::default(),
            block_ticks: Default::default(),
            redstone: Default::default(),
            random_tick_lcg: JavaRandom::new(JavaRandom::independent_seed()).next_i32(),
            rng: 189,
            player_random_state: JavaRandom::new(189).state(),
            enchantment_random_state: JavaRandom::new(0x55555).state(),
            next_id: 1,
            yaw: -0.6,
            pitch: -0.15,
            particles: Vec::new(),
            critical_emitters: Default::default(),
            particle_random: independent_particle_random(),
            damage_sounds: Vec::new(),
            pending_world_sounds: Vec::new(),
            pending_changed_blocks: Vec::new(),
            end_crystals: Vec::new(),
            overworld_spawn: None,
            end_dragon_spawned: false,
            end_dragon_slain: false,
            dimension_slices: Default::default(),
            active_populated_chunks: Default::default(),
            teleporter_caches: Default::default(),
            spawner_spins: Default::default(),
            breaking: 0.0,
            break_target: None,
            pending_sign_edit: None,
            eating: 0,
            item_use: UseState::default(),
            hurt_camera: HurtCamera::default(),
            hurt_ticks: 0,
            last_damage: 0.,
            place_ticks: 0,
            right_click_delay: 0,
            right_was_down: false,
            fall_distance: 0.0,
        }
    }
}
fn absent_total_ticks() -> u64 {
    u64::MAX
}
#[derive(Default)]
pub struct Outcome {
    pub open_command_block: Option<IVec3>,
    pub command_blocks: Vec<IVec3>,
    pub collected_items: Vec<u64>,
    pub guest_item_stats: Vec<(String,String)>,
    pub changed: Vec<IVec3>,
    pub sounds: Vec<(&'static str, f32)>,
    pub pitched_sounds: Vec<(&'static str, f32, f32)>,
    pub world_sounds: Vec<(&'static str, glam::DVec3, f32, f32)>,
    pub portal_travel: Option<PortalTravel>,
}
struct FoodPotionTarget<'a> {
    sim: &'a mut Simulation,
    resistance: Option<i32>,
}
fn independent_particle_random() -> [u64; 3] {
    std::array::from_fn(|_| JavaRandom::new(JavaRandom::independent_seed()).state())
}
fn independent_voice_random() -> [u64; 4] {
    std::array::from_fn(|_| JavaRandom::new(JavaRandom::independent_seed()).state())
}
impl Simulation {
    pub fn flush_trade_feedback(&mut self, id: u64, _listener: Vec3) {
        let Some(position) = self.mobs.iter().find(|m| m.id == id).map(|m| m.position) else {
            return;
        };
        let Some(trader) = self.trades.get_mut(&id) else {
            return;
        };
        let home = trader.career.home;
        for event in std::mem::take(&mut trader.pending) {
            if let Some(value) = event.experience {
                self.spawn_xp(position.as_dvec3() + glam::DVec3::Y * 0.5, value);
            }
            if let Some((home, name)) = home.zip(event.reputation_player) {
                self.villager_homes.reward(home, &name);
            }
            if let Some((name, volume, pitch)) = event.sound {
                let event_name = match name {
                    "mob.villager.haggle" => "mob.villager.haggle",
                    "mob.villager.idle" => "mob.villager.idle",
                    "mob.villager.yes" => "mob.villager.yes",
                    "mob.villager.no" => "mob.villager.no",
                    _ => continue,
                };
                self.pending_world_sounds
                    .push((event_name, position.as_dvec3(), volume, pitch));
            }
        }
    }
}
impl potions::Target for FoodPotionTarget<'_> {
    fn remote(&self) -> bool {
        false
    }
    fn health(&self) -> f32 {
        self.sim.health
    }
    fn max_health(&self) -> f32 {
        self.sim.max_health
    }
    fn heal(&mut self, amount: f32) {
        if self.sim.health > 0. {
            self.sim.health = (self.sim.health + amount).clamp(0., self.sim.max_health);
        }
    }
    fn attack(&mut self, wither: bool, amount: f32) {
        self.sim
            .damage_resisted(amount, false, false, false, self.resistance);
        if self.sim.health <= 0. {
            self.sim.death_message_key = Some(if wither {
                "death.attack.wither"
            } else {
                "death.attack.magic"
            });
        }
    }
    fn exhaustion(&mut self, amount: f32) {
        self.sim.add_exhaustion(amount);
    }
    fn saturation(&mut self, amount: i32) {
        let mut food = self.sim.food_state();
        food.add_stats(amount, 1.);
        self.sim.set_food_state(food);
    }
    fn changed(&mut self, effect: Effect, change: Change) {
        if effect.id == 11 {
            self.resistance = if change == Change::Removed {
                None
            } else {
                Some(effect.amplifier)
            };
        }
        if effect.id == 21 {
            if matches!(change, Change::Removed | Change::Changed(true)) {
                self.sim.max_health =
                    (self.sim.max_health - 4. * (effect.amplifier + 1) as f32).max(20.);
                self.sim.health = self.sim.health.min(self.sim.max_health);
            }
            if matches!(change, Change::Added | Change::Changed(true)) {
                self.sim.max_health += 4. * (effect.amplifier + 1) as f32;
            }
        }
        if effect.id == 22 {
            if matches!(change, Change::Removed | Change::Changed(true)) {
                self.sim.absorption =
                    potions::absorption_attribute(self.sim.absorption, effect.amplifier, false);
            }
            if matches!(change, Change::Added | Change::Changed(true)) {
                self.sim.absorption =
                    potions::absorption_attribute(self.sim.absorption, effect.amplifier, true);
            }
        }
    }
}
fn random(state: &mut u64) -> u64 {
    if *state == 0 {
        *state = 189;
    }
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}
fn unit(state: &mut u64) -> f32 {
    (random(state) >> 40) as f32 / 16777216.0
}
pub fn harvestable(state: u16, held: Option<Stack>) -> bool {
    food::can_harvest(state, held.map(|item| item.id))
}

pub fn block_drops(state: u16, held: Option<Stack>, rng: &mut u64) -> Vec<Stack> {
    let id = state >> 4;
    if !harvestable(state, held.clone()) {
        return vec![];
    }
    let stack = match id {
        26 if state & 8 != 0 => return Vec::new(),
        26 => Stack::new(355, 1),
        64 | 71 | 193..=197 if state & 8 != 0 => return Vec::new(),
        64 | 71 | 193..=197 => Stack::new(crate::vanilla::hand_placement::door_item(id).unwrap(), 1),
        154 => Stack::new(154, 1),
        34 | 36 => return Vec::new(),
        27 | 28 | 29 | 33 | 50 | 65 | 66 | 69 | 70 | 72 | 77 | 96 | 131 | 143 | 147 | 148
        | 157 | 167 => Stack::new(id, 1),
        55 => Stack::new(331, 1),
        63 | 68 => Stack::new(323, 1),
        75 | 76 => Stack::new(76, 1),
        83 => Stack::new(338, 1),
        93 | 94 => Stack::new(356, 1),
        149 | 150 => Stack::new(404, 1),
        140 => Stack::new(390, 1),
        144 => Stack::new(397, 1),
        127 => Stack {
            nbt: None,
            identity: crate::inventory::Stack::fresh_identity(),
            id: 351,
            meta: 3,
            count: if ((state & 15) >> 2) >= 2 { 3 } else { 1 },
            damage: 0,
        },
        123 | 124 => Stack::new(123, 1),
        0 | 7..=11 | 20 | 79 | 90 | 95 | 102 | 119 | 120 | 166 => return vec![],
        1 if state & 15 == 0 => Stack::new(4, 1),
        2 | 60 | 110 => Stack::new(3, 1),
        16 => Stack::new(263, 1),
        21 => Stack {
            nbt: None,
            identity: crate::inventory::Stack::fresh_identity(),
            id: 351,
            meta: 4,
            count: 4 + (random(rng) % 6) as u8,
            damage: 0,
        },
        56 => Stack::new(264, 1),
        73 | 74 => Stack::new(331, 4 + (random(rng) % 2) as u8),
        129 => Stack::new(388, 1),
        89 => Stack::new(348, 2 + (random(rng) % 3) as u8),
        47 => Stack::new(340, 3),
        62 => Stack::new(61, 1),
        103 => Stack::new(360, 3 + (random(rng) % 5) as u8),
        59 | 141 | 142 => {
            let seed = match id {
                59 => 295,
                141 => 391,
                _ => 392,
            };
            let mature = state & 15 >= 7;
            let mut result = vec![Stack::new(if mature && id == 59 { 296 } else { seed }, 1)];
            if mature {
                let count = (0..3).filter(|_| random(rng) % 15 <= 7).count() as u8;
                if count > 0 {
                    result.push(Stack::new(seed, count));
                }
                if id == 142 && random(rng).is_multiple_of(50) {
                    result.push(Stack::new(394, 1));
                }
            }
            return result;
        }
        115 => Stack::new(
            372,
            if state & 15 >= 3 {
                2 + (random(rng) % 3) as u8
            } else {
                1
            },
        ),
        18 | 161 => {
            if held.as_ref().is_some_and(|s| s.id == 359) {
                Stack::block((id << 4) | (state & 3))
            } else if random(rng).is_multiple_of(20) {
                Stack {
                    nbt: None,
                    identity: crate::inventory::Stack::fresh_identity(),
                    id: 6,
                    meta: if id == 161 {
                        4 + (state & 1)
                    } else {
                        state & 3
                    },
                    count: 1,
                    damage: 0,
                }
            } else {
                return vec![];
            }
        }
        31 | 175 => {
            if held.as_ref().is_some_and(|s| s.id == 359) {
                Stack::block(state)
            } else if random(rng).is_multiple_of(8) {
                Stack::new(295, 1)
            } else {
                return vec![];
            }
        }
        32 => return vec![],
        17 | 162 => Stack::block((id << 4) | (state & 3)),
        78 | 80 => {
            if held.as_ref().is_some_and(|s| tool(s.id).0 == Tool::Shovel) {
                Stack::new(332, if id == 80 { 4 } else { 1 })
            } else {
                return vec![];
            }
        }
        _ => Stack::block(state),
    };
    vec![stack]
}
impl Simulation {
    pub(crate) fn destroy_command_drops(&mut self,state:u16,p:IVec3){
        if self.world_commands.rules.get("doTileDrops").is_some_and(|v|v=="false"){return}
        for stack in block_drops(state,Some(Stack::new(278,1)),&mut self.rng){self.spawn_drop(stack,p.as_vec3()+Vec3::splat(0.5),Vec3::Y*0.15);}
    }
    pub fn container_view(&self, world: &World, pos: IVec3) -> Option<crate::container::Container> {
        let parts = container_parts(world, pos)?;
        let id = world.get(pos) >> 4;
        let mut container = self
            .containers
            .get(&container_key(parts[0]))
            .cloned()
            .or_else(|| crate::container::Container::for_block(id))?;
        if parts.len() == 2 {
            let second = self
                .containers
                .get(&container_key(parts[1]))
                .cloned()
                .unwrap_or_else(|| crate::container::Container::new(crate::container::Kind::Chest));
            container.slots.truncate(27);
            container.slots.extend(second.slots.into_iter().take(27));
            container.slots.resize(54, None);
        }
        Some(container)
    }
    /// Apply one GUI transaction to the logical window, then split double chests
    /// back into their two persistent block inventories before returning.
    pub fn window_click(
        &mut self,
        world: &World,
        pos: Option<IVec3>,
        slot: crate::inventory::SlotRef,
        action: crate::inventory::ClickAction,
        crafting_width: usize,
    ) -> Vec<Stack> {
        let creative = self.mode == Mode::Creative;
        if let Some(pos) = pos {
            let Some(parts) = container_parts(world, pos) else {
                return Vec::new();
            };
            let Some(mut container) = self.container_view(world, pos) else {
                return Vec::new();
            };
            let dropped = crate::container::window_click(
                &mut self.inventory,
                Some(&mut container),
                slot,
                action,
                creative,
                crafting_width,
            );
            if parts.len() == 2 {
                let second = container.slots.split_off(27);
                let mut other = crate::container::Container::new(crate::container::Kind::Chest);
                other.slots = second;
                self.containers.insert(container_key(parts[1]), other);
            }
            self.containers.insert(container_key(parts[0]), container);
            dropped
        } else {
            crate::container::window_click(
                &mut self.inventory,
                None,
                slot,
                action,
                creative,
                crafting_width,
            )
        }
    }
    pub fn import_containers(&mut self, world: &World) {
        // Both generated structures and Anvil chunks carry initial container data.
        // Remember processed chunks so depleted or destroyed chests cannot refill.
        for (&chunk_pos, chunk) in &world.chunks {
            if !self.imported_container_chunks.insert(chunk_pos) {
                continue;
            }
            for (pos, container) in &chunk.containers {
                if matches!(world.get(*pos) >> 4, 23 | 54 | 146 | 61 | 62 | 117 | 138 | 154 | 158) {
                    self.containers
                        .entry(container_key(*pos))
                        .or_insert_with(|| container.clone());
                }
            }
        }
    }
    pub fn import_player(&mut self, path: &Path) -> Result<()> {
        use crate::nbt::Tag;
        let root = crate::world::level_data(path)?;
        let Some(data) = root.get("Data") else {
            return Ok(());
        };
        self.world_border = crate::vanilla::click_border::Border::from_world_info(
            data,
            crate::vanilla::click_border::now(),
        );
        self.villager_homes = crate::villagers::homes::Homes::import(path)?;
        self.flat_world = data.get("generatorName").and_then(Tag::string).is_some_and(|name| name.eq_ignore_ascii_case("flat"));
        self.weather = crate::vanilla::weather::Weather::from_level(data);
        self.hardcore = crate::player_save::nbt_boolean(data.get("hardcore"));
        self.difficulty_locked = crate::player_save::nbt_boolean(data.get("DifficultyLocked"));
        self.world_difficulty = if self.hardcore { Some(3) } else {
            crate::player_save::nbt_integer(data.get("Difficulty")).map(|n| n.clamp(0,3) as u8)
        };
        if let Some(Tag::Long(seed))=data.get("RandomSeed"){self.world_commands.imported_seed=Some(*seed);}
        if let Some(n)=data.get("BorderDamagePerBlock").and_then(Tag::number){self.world_commands.border_damage=n;}
        if let Some(n)=data.get("BorderSafeZone").and_then(Tag::number){self.world_commands.border_buffer=n;}
        if let Some(rules) = data.get("GameRules") {
            if let Tag::Compound(entries)=rules{
                self.world_commands.rules.extend(entries.iter().filter_map(|(k,v)|Some((k.clone(),v.string()?.to_owned()))));
                self.server_reduced_debug=self.world_commands.rules.get("reducedDebugInfo").is_some_and(|v|v=="true");
            }
            if let Some(value) = rules.get("sendCommandFeedback").and_then(Tag::string) {
                self.send_command_feedback = value.eq_ignore_ascii_case("true");
            }
            if let Some(value) = rules.get("doMobSpawning").and_then(Tag::string) {
                self.mob_spawning = value.eq_ignore_ascii_case("true");
            }
            if let Some(value) = rules.get("keepInventory").and_then(Tag::string) {
                self.keep_inventory = value.eq_ignore_ascii_case("true");
            }
            if let Some(value) = rules.get("doMobLoot").and_then(Tag::string) {
                self.mob_loot = value.eq_ignore_ascii_case("true");
            }
        }
        if let Some(clock) = data
            .get("DayTime")
            .or_else(|| data.get("Time"))
            .and_then(Tag::number)
        {
            self.clock = clock.max(0.0) as u64;
        }
        if let Some(rule) = data
            .get("GameRules")
            .and_then(|rules| rules.get("naturalRegeneration"))
            .and_then(Tag::string)
        {
            self.natural_regeneration = rule.eq_ignore_ascii_case("true");
        }
        self.total_ticks = data
            .get("Time")
            .and_then(Tag::number)
            .unwrap_or(self.clock as f64) as u64;
        if let Some(value) = data
            .get("GameRules")
            .and_then(|r| r.get("doDaylightCycle"))
            .and_then(Tag::string)
        {
            self.daylight_cycle = value.eq_ignore_ascii_case("true");
        }
        if let Some(rule) = data
            .get("GameRules")
            .and_then(|rules| rules.get("doEntityDrops"))
            .and_then(Tag::string)
        {
            self.block_ticks
                .set_do_entity_drops(rule.eq_ignore_ascii_case("true"));
        }
        let Some(p) = data.get("Player") else {
            return Ok(());
        };
        self.fall_distance = p.get("FallDistance").and_then(Tag::number).unwrap_or(0.) as f32;
        self.fire_ticks = crate::vanilla::food::nbt_int(p.get("Fire")) as i16 as i32;
        self.bed_spawn = crate::vanilla::respawn::BedSpawn::read(p);
        self.sleep.timer = crate::vanilla::food::nbt_int(p.get("SleepTimer")) as i16 as i32;
        if crate::player_save::nbt_boolean(p.get("Sleeping")) {
            if let Some(pose) = crate::player_save::PlayerSave::from_level(data) {
                self.sleep.head = Some(glam::DVec3::from_array(pose.position).floor().as_ivec3());
            }
        }
        if let Some(items) = p.get("Inventory").and_then(Tag::list) {
            for item in items {
                let slot = item.get("Slot").and_then(Tag::int).unwrap_or(-1);
                let index = match slot {
                    0..=35 => slot as usize,
                    100..=103 => (slot - 100 + 36) as usize,
                    _ => continue,
                };
                self.inventory.slots[index] = crate::inventory::from_nbt(item);
            }
        }
        self.inventory.selected = p
            .get("SelectedItemSlot")
            .and_then(Tag::int)
            .unwrap_or(0)
            .clamp(0, 8) as usize;
        if let Some(attributes) = p.get("Attributes").and_then(Tag::list) {
            if let Some(attribute) = attributes
                .iter()
                .find(|a| a.get("Name").and_then(Tag::string) == Some("generic.maxHealth"))
            {
                if let Some(base) = attribute.get("Base").and_then(Tag::number) {
                    self.max_health = base.clamp(0.0, 1024.0) as f32;
                }
            }
        }
        self.health = p
            .get("HealF")
            .or_else(|| p.get("Health"))
            .and_then(Tag::number)
            .unwrap_or(self.max_health as f64)
            .clamp(0.0, self.max_health as f64) as f32;
        let mut food = FoodState::default();
        food.read_nbt(p);
        self.set_food_state(food);
        self.absorption = p
            .get("AbsorptionAmount")
            .and_then(Tag::number)
            .unwrap_or(0.) as f32;
        if self.absorption < 0. {
            self.absorption = 0.;
        }
        if let Some(effects) = p.get("ActiveEffects").and_then(Tag::list) {
            for effect in effects.iter().filter_map(potions::read_effect) {
                self.effects.load_effect(effect)?;
            }
        }
        self.xp.total = p.get("XpTotal").and_then(Tag::int).unwrap_or(0);
        self.xp.level = p.get("XpLevel").and_then(Tag::int).unwrap_or(0);
        self.xp.progress = p.get("XpP").and_then(Tag::number).unwrap_or(0.) as f32;
        self.xp.score = p.get("Score").and_then(Tag::int).unwrap_or(0);
        self.experience = self.xp.total.max(0) as u32;
        if let Some(mode)=p.get("playerGameType").and_then(Tag::int).filter(|v|(0..=3).contains(v)){
            self.commands.player_mode=Some(mode as u8);
            self.mode=if matches!(mode,1|3){Mode::Creative}else{Mode::Survival};
        }
        if let Some(rotation) = p.get("Rotation").and_then(Tag::list) {
            if rotation.len() >= 2 {
                self.yaw = std::f32::consts::PI
                    + rotation[0].number().unwrap_or(0.0) as f32 * std::f32::consts::PI / 180.0;
                self.pitch = -(rotation[1].number().unwrap_or(0.0) as f32)
                    .to_radians()
                    .clamp(-1.55, 1.55);
            }
        }
        Ok(())
    }
    pub fn new(mode: Mode, seed: u64) -> Self {
        Self {
            mode,
            spawn_seed: seed as i64,
            rng: seed.max(1),
            player_random_state: JavaRandom::new(seed as i64 ^ 0x42d19).state(),
            enchantment_random_state: JavaRandom::new(seed as i64 ^ 0x55555).state(),
            ..Default::default()
        }
    }
    pub fn read_legacy_save(path: &Path) -> Result<Option<Self>> {
        let file = path.with_extension("player.json");
        match std::fs::File::open(file) {
            Ok(file) => Ok(Some(serde_json::from_reader(std::io::BufReader::new(
                file,
            ))?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    pub fn restore(mut self) -> Self {
        local_commands::sync_board(&mut self);
        self.server_reduced_debug=self.world_commands.rules.get("reducedDebugInfo").is_some_and(|v|v.eq_ignore_ascii_case("true"));
        self.inventory.sanitize();
        if self.total_ticks == u64::MAX {
            self.total_ticks = self.clock;
        }
        self.mobs.retain(|m| {
            m.position.is_finite()
                && (m.health > 0.0
                    || (m.kind == MobKind::EnderDragon
                        && m.health >= 0.0
                        && self
                            .mob_brains
                            .get(&m.id)
                            .is_some_and(|b| b.dragon_death_ticks > 0 && b.dragon_death_ticks < 200)))
        });
        self.end_crystals.retain(|c| c.position.is_finite());
        self.drops
            .retain(|d| d.position.is_finite() && d.stack.count > 0 && d.age < 6000);
        if !self.max_health.is_finite() || self.max_health < 0.0 {
            self.max_health = 20.0;
        }
        self.health = self.health.clamp(0.0, self.max_health);
        // FoodStats.readNBT preserves signed timer/level and finite float bits;
        // exhausted, negative and >20 values are resolved by its next update.
        self.previous_food = 20;
        // Migrate local saves made before levels and fractional progress were saved.
        if self.xp.total == 0 && self.experience > 0 {
            let (level, progress) = experience_level(self.experience);
            self.xp.level = level as i32;
            self.xp.progress = progress;
            self.xp.total = self.experience.min(i32::MAX as u32) as i32;
        }
        self
    }
    fn alloc_crystal_id(&mut self) -> u64 {
        loop {
            let id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1).max(1);
            if !self.minecarts.iter().any(|m| m.id == id)
                && !self.boats.iter().any(|b| b.id == id)
                && !self.mobs.iter().any(|m| m.id == id)
                && !self.end_crystals.iter().any(|c| c.id == id)
            {
                return id;
            }
        }
    }
    pub(crate) fn destroy_end_crystal(
        &mut self,
        id: u64,
        world: &mut World,
        player: &mut Player,
        settings: &Settings,
        out: &mut Outcome,
    ) -> bool {
        let Some(idx) = self.end_crystals.iter().position(|c| c.id == id) else {
            return false;
        };
        let crystal = self.end_crystals.remove(idx);
        self.explode(world, player, settings, crystal.position, 6.0, true, false, out);
        true
    }
    pub(crate) fn spawner_spin_degrees(&self, pos: IVec3, alpha: f32) -> f32 {
        let Some(s) = self.spawner_spins.get(&pos.to_array()) else {
            return 0.0;
        };
        let delta = (s.rot - s.prev_rot).rem_euclid(360.0);
        ((s.prev_rot + delta * f64::from(alpha)) as f32) * 10.0
    }
    pub(crate) fn tick_spawner_spins(
        &mut self,
        world: &World,
        player_pos: Vec3,
        spectator: bool,
        is_remote: bool,
    ) {
        let pcx = (player_pos.x.floor() as i32).div_euclid(16);
        let pcz = (player_pos.z.floor() as i32).div_euclid(16);
        for cx in (pcx - 3)..=(pcx + 3) {
            for cz in (pcz - 3)..=(pcz + 3) {
                let Some(chunk) = world.chunks.get(&(cx, cz)) else {
                    continue;
                };
                for &packed in chunk.spawner_positions() {
                    let pos = IVec3::new(
                        cx * 16 + i32::from(packed & 15),
                        i32::from(packed >> 8),
                        cz * 16 + i32::from((packed >> 4) & 15),
                    );
                    if world.get(pos) >> 4 != 52 {
                        continue;
                    }
                    let (nbt_delay, _min_delay, range) = world
                        .tile(pos)
                        .and_then(crate::vanilla::item_nbt::compound)
                        .map_or((20, 200, 16), |fields| {
                            let num = |k: &str, def: i32| {
                                fields
                                    .get(k)
                                    .map_or(def, |t| crate::vanilla::item_nbt::number(Some(t)))
                            };
                            (
                                num("Delay", 20),
                                num("MinSpawnDelay", 200),
                                num("RequiredPlayerRange", 16),
                            )
                        });
                    let range_f = range.max(0) as f32;
                    let active = !spectator
                        && (pos.as_vec3() + Vec3::splat(0.5)).distance_squared(player_pos)
                            < range_f * range_f;
                    let key = pos.to_array();
                    let is_new = !self.spawner_spins.contains_key(&key);
                    let entry = self.spawner_spins.entry(key).or_default();
                    if is_new {
                        entry.mp_delay = nbt_delay;
                    }
                    entry.prev_rot = entry.rot;
                    if active {
                        let delay = if is_remote {
                            let cur = entry.mp_delay;
                            if entry.mp_delay > 0 {
                                entry.mp_delay -= 1;
                            }
                            cur
                        } else {
                            nbt_delay
                        };
                        entry.rot = (entry.rot
                            + f64::from(1000.0_f32 / (delay.max(0) as f32 + 200.0_f32)))
                            % 360.0;
                    }
                }
            }
        }
        self.spawner_spins.retain(|pos, _| {
            (pos[0].div_euclid(16) - pcx).abs() <= 6 && (pos[2].div_euclid(16) - pcz).abs() <= 6
        });
    }
    pub(crate) fn reset_mp_spawner_delay(&mut self, world: &World, pos: IVec3) {
        let min_delay = world
            .tile(pos)
            .and_then(crate::vanilla::item_nbt::compound)
            .and_then(|f| f.get("MinSpawnDelay"))
            .map_or(200, |t| crate::vanilla::item_nbt::number(Some(t)));
        self.spawner_spins.entry(pos.to_array()).or_default().mp_delay = min_delay;
    }
    fn extract_tile_edits(
        &self,
    ) -> Vec<([i32; 3], (u16, Option<crate::vanilla::drops::DropNbt>))> {
        self.world_commands.clone_tile_edits()
    }
    fn replace_tile_edits(
        &mut self,
        tile_edits: Vec<([i32; 3], (u16, Option<crate::vanilla::drops::DropNbt>))>,
    ) {
        self.world_commands.set_tile_edits(tile_edits);
    }
    fn collect_world_edits(world: &World) -> Vec<([i32; 3], u16)> {
        let mut edits: Vec<([i32; 3], u16)> =
            world.edits.iter().map(|(&pos, &state)| (pos, state)).collect();
        edits.sort_unstable_by_key(|&(pos, _)| pos);
        edits
    }
    pub(crate) fn stash_active_dimension(&mut self, world: &World) {
        self.ridden_horse = None;
        self.ridden_pig = None;
        self.ridden_minecart = None;
        self.ridden_boat = None;
        self.horse_rider = Default::default();
        self.pig_boost_ticks = 0;
        self.pig_boost_total = 0;
        self.pig_speed = 0.0;
        self.horse_window = None;
        self.show_horse_window = false;
        self.minecart_window = None;
        self.show_minecart_window = false;
        self.trading_with = None;
        if self.dimension == 0 {
            self.overworld_spawn = Some(world.spawn.to_array());
        }
        let tile_edits = self.world_commands.take_tile_edits();
        let slice = DimensionSlice {
            edits: Self::collect_world_edits(world),
            containers: std::mem::take(&mut self.containers),
            mobs: std::mem::take(&mut self.mobs),
            dying_mobs: std::mem::take(&mut self.dying_mobs),
            drops: std::mem::take(&mut self.drops),
            xp_orbs: std::mem::take(&mut self.xp_orbs),
            projectiles: std::mem::take(&mut self.projectiles),
            arrows: std::mem::take(&mut self.arrows),
            mob_shots: std::mem::take(&mut self.mob_shots),
            primed_tnt: std::mem::take(&mut self.primed_tnt),
            minecarts: std::mem::take(&mut self.minecarts),
            boats: std::mem::take(&mut self.boats),
            end_crystals: std::mem::take(&mut self.end_crystals),
            block_ticks: std::mem::take(&mut self.block_ticks),
            tile_edits,
            populated_chunks: std::mem::take(&mut self.active_populated_chunks),
            imported_container_chunks: std::mem::take(&mut self.imported_container_chunks),
            imported_entity_chunks: std::mem::take(&mut self.imported_entity_chunks),
            command_mobs: std::mem::take(&mut self.command_entities.mobs),
            mob_brains: std::mem::take(&mut self.mob_brains),
            trades: std::mem::take(&mut self.trades),
            note_pitches: std::mem::take(&mut self.note_pitches),
        };
        self.dimension_slices.insert(self.dimension, slice);
    }
    pub(crate) fn snapshot_active_dimension_for_save(&mut self, world: &World) {
        if self.dimension == 0 {
            self.overworld_spawn = Some(world.spawn.to_array());
        }
        let slice = DimensionSlice {
            edits: Self::collect_world_edits(world),
            containers: self.containers.clone(),
            mobs: self.mobs.clone(),
            dying_mobs: Vec::new(),
            drops: self.drops.clone(),
            xp_orbs: self.xp_orbs.clone(),
            projectiles: self.projectiles.clone(),
            arrows: self.arrows.clone(),
            mob_shots: self.mob_shots.clone(),
            primed_tnt: self.primed_tnt.clone(),
            minecarts: self.minecarts.clone(),
            boats: self.boats.clone(),
            end_crystals: self.end_crystals.clone(),
            block_ticks: self.block_ticks.clone(),
            tile_edits: self.extract_tile_edits(),
            populated_chunks: self.active_populated_chunks.clone(),
            imported_container_chunks: self.imported_container_chunks.clone(),
            imported_entity_chunks: self.imported_entity_chunks.clone(),
            command_mobs: self.command_entities.mobs.clone(),
            mob_brains: self.mob_brains.clone(),
            trades: self.trades.clone(),
            note_pitches: self.note_pitches.clone(),
        };
        self.dimension_slices.insert(self.dimension, slice);
    }
    pub(crate) fn activate_dimension(&mut self, target_dim: i32, world: &mut World) {
        self.dimension = target_dim;
        let slice = self.dimension_slices.remove(&target_dim).unwrap_or_default();
        self.containers = slice.containers;
        self.mobs = slice.mobs;
        self.dying_mobs = slice.dying_mobs;
        self.drops = slice.drops;
        self.xp_orbs = slice.xp_orbs;
        self.projectiles = slice.projectiles;
        self.arrows = slice.arrows;
        self.mob_shots = slice.mob_shots;
        self.primed_tnt = slice.primed_tnt;
        self.minecarts = slice.minecarts;
        self.boats = slice.boats;
        self.end_crystals = slice.end_crystals;
        self.block_ticks = slice.block_ticks;
        self.block_ticks.set_nether(target_dim == -1);
        self.active_populated_chunks = slice.populated_chunks;
        self.imported_container_chunks = slice.imported_container_chunks;
        self.imported_entity_chunks = slice.imported_entity_chunks;
        self.command_entities.mobs = slice.command_mobs;
        self.mob_brains = slice.mob_brains;
        self.trades = slice.trades;
        self.note_pitches = slice.note_pitches;
        self.replace_tile_edits(slice.tile_edits);
        world.edits.clear();
        world.edits.extend(slice.edits.iter().copied());
        for &(pos, state) in &slice.edits {
            if world
                .chunks
                .contains_key(&(pos[0].div_euclid(16), pos[2].div_euclid(16)))
            {
                let _ = world.set(IVec3::from_array(pos), state);
            }
        }
        if !world.chunks.is_empty() {
            let loaded: Vec<_> = world.chunks.keys().copied().collect();
            self.world_commands.restore_tiles(world, &loaded);
        }
        if target_dim == 0 {
            if let Some(cmd_spawn) = self.world_commands.spawn {
                world.spawn = IVec3::from_array(cmd_spawn).as_vec3();
            } else if let Some(spawn) = self.overworld_spawn {
                world.spawn = Vec3::from_array(spawn);
            }
        } else if target_dim == 1 {
            world.spawn = Vec3::new(100.5, 49.0, 0.5);
        }
        self.particles.clear();
        self.explosion_clouds.clear();
        self.critical_emitters = Default::default();
        self.horse_anims.clear();
        self.pick_sections.clear();
        self.powered_notes.clear();
        self.note_events.clear();
        self.redstone = Default::default();
        self.spawner_spins.clear();
        self.pending_changed_blocks.clear();
    }
    pub(crate) fn overworld_saved_edits(&self, world: &World) -> Vec<([i32; 3], u16)> {
        if self.dimension == 0 {
            Self::collect_world_edits(world)
        } else {
            self.dimension_slices
                .get(&0)
                .map_or_else(Vec::new, |slice| slice.edits.clone())
        }
    }
    pub(crate) fn saved_edits_for_dimension(
        &self,
        world: &World,
        dim: i32,
    ) -> std::collections::BTreeMap<[i32; 3], u16> {
        if self.dimension == dim {
            world.edits.iter().map(|(&pos, &state)| (pos, state)).collect()
        } else {
            self.dimension_slices
                .get(&dim)
                .map_or_else(std::collections::BTreeMap::new, |slice| {
                    slice.edits.iter().copied().collect()
                })
        }
    }
    pub(crate) fn teleporter_cache_and_rng(
        &mut self,
        to_dim: i32,
    ) -> (&mut crate::dimension_world::TeleporterCache, &mut u64) {
        (self.teleporter_caches.entry(to_dim).or_default(), &mut self.rng)
    }
    pub(crate) fn adopt_dimension_meta(
        &mut self,
        dimension: i32,
        world: &mut World,
        pos: crate::world::ChunkPos,
        meta: crate::dimension_world::GeneratedDimensionMeta,
    ) {
        if !self.active_populated_chunks.insert(pos) {
            return;
        }
        for (cpos, items) in meta.chests {
            let mut chest = crate::container::Container::new(crate::container::Kind::Chest);
            for (slot, stack) in items {
                if slot < chest.slots.len() && stack.count > 0 {
                    chest.slots[slot] = Some(stack);
                }
            }
            self.containers
                .entry(container_key(IVec3::from_array(cpos)))
                .or_insert(chest);
        }
        for (spos_arr, entity_id) in meta.spawners {
            use crate::vanilla::drops::DropNbt as Tag;
            let spos = IVec3::from_array(spos_arr);
            let cur = world.get(spos);
            let state = if cur >> 4 == 52 { cur } else { 52 << 4 };
            let tag = std::collections::BTreeMap::from([
                ("id".into(), Tag::String("MobSpawner".into())),
                ("EntityId".into(), Tag::String(entity_id.into())),
                ("x".into(), Tag::Int(spos.x)),
                ("y".into(), Tag::Int(spos.y)),
                ("z".into(), Tag::Int(spos.z)),
                ("Delay".into(), Tag::Short(20)),
            ]);
            self.world_commands
                .edit_tile(world, spos, state, Some(Tag::Compound(tag)));
        }
        for c in meta.end_crystals {
            let cpos = glam::DVec3::from_array(c);
            if !self
                .end_crystals
                .iter()
                .any(|ec| ec.position.distance_squared(cpos) < 0.01)
            {
                let id = self.alloc_crystal_id();
                self.end_crystals.push(EndCrystal {
                    id,
                    position: cpos,
                    bottom: true,
                    beam_target: None,
                });
            }
            if dimension == 1 {
                let fpos = cpos.floor().as_ivec3();
                if world.contains(fpos.as_vec3())
                    && world.get(fpos) >> 4 != 51
                    && world.set(fpos, 51 << 4)
                {
                    self.block_ticks.notify(world, fpos);
                    self.pending_changed_blocks.push(fpos);
                }
            }
        }
        if dimension == 1
            && meta.ender_dragon.is_some()
            && !self.end_dragon_spawned
            && !self.end_dragon_slain
        {
            let dpos = meta.ender_dragon.map_or(Vec3::new(0.0, 128.0, 0.0), |[x, y, z]| {
                if y > 0.0 {
                    Vec3::new(x as f32, y as f32, z as f32)
                } else {
                    Vec3::new(0.0, 128.0, 0.0)
                }
            });
            let dragon_id = self.spawn_registered(MobKind::EnderDragon, dpos, 0.0);
            self.command_entities
                .mobs
                .entry(dragon_id)
                .or_default()
                .persistent = true;
            self.end_dragon_spawned = true;
        }
    }
    pub(crate) fn return_from_end_to_overworld(
        &mut self,
        world: &mut World,
        player: &mut Player,
    ) {
        if self.dimension != 0 {
            self.stash_active_dimension(world);
            self.activate_dimension(0, world);
        }
        self.clear_local_mounts(player);
        self.minecart_window = None;
        self.show_minecart_window = false;
        self.horse_window = None;
        self.show_horse_window = false;
        self.sleep = Default::default();
        self.fall_distance = 0.0;
        let base_spawn = self
            .overworld_spawn
            .map(Vec3::from_array)
            .unwrap_or(world.spawn);
        world.spawn = base_spawn;
        let mut random = JavaRandom::from_state(self.player_random_state);
        let saved_bed_before = self.bed_spawn;
        let bed_chunk_loaded = saved_bed_before.is_some_and(|b| {
            b.forced
                || world
                    .chunks
                    .contains_key(&(b.point.x.div_euclid(16), b.point.z.div_euclid(16)))
                || world.edits.contains_key(&b.point.to_array())
        });
        let mut bed_arg = if bed_chunk_loaded {
            self.bed_spawn
        } else {
            None
        };
        let mut feet = crate::vanilla::respawn::recreate(
            world,
            base_spawn.floor().as_ivec3(),
            &mut bed_arg,
            &mut random,
        );
        if bed_chunk_loaded {
            self.bed_spawn = bed_arg;
        } else if let Some(saved) = saved_bed_before {
            feet = [
                f64::from(saved.point.x) + 0.5,
                f64::from(saved.point.y) + 0.6,
                f64::from(saved.point.z) + 0.5,
            ];
        } else if !world.chunks.contains_key(&(
            (feet[0].floor() as i32).div_euclid(16),
            (feet[2].floor() as i32).div_euclid(16),
        )) && feet[1] <= 1.0
            && base_spawn.y > 1.0
        {
            feet[1] = f64::from(base_spawn.y);
        }
        self.player_random_state = random.state();
        let [yaw, pitch] = player.java_rotation();
        player.reset_portal_travel();
        player.set_pose_f64(feet, yaw, pitch, false);
        player.set_velocity_f64([0.0; 3]);
    }
    pub fn spawn_drop(&mut self, item: Stack, position: Vec3, velocity: Vec3) {
        if item.count == 0 {
            return;
        }
        self.spawn_item_entity(item, position.as_dvec3(), velocity.as_dvec3(), 10);
    }
    pub fn return_cursor(&mut self, player: &Player) {
        if let Some(id) = self.trading_with {
            self.flush_trade_feedback(id, player.position);
        }
        let mut merchant = std::mem::take(&mut self.merchant_window);
        for item in self.inventory.close().into_iter().chain(merchant.close()) {
            self.throw_local_stack(player,item,ThrowSource::Window);
        }
        if let Some(id) = self.trading_with.take() {
            if let Some(trader) = self.trades.get_mut(&id) {
                trader.customer = None;
            }
        }
    }
    pub fn begin_trading(&mut self, id: u64, customer: &str) -> Result<bool> {
        if self.trading_with.is_some() || self.inventory.held().is_some_and(|s| s.id == 383) {
            return Ok(false);
        }
        let trader = self
            .trades
            .entry(id)
            .or_insert_with(|| crate::villagers::Trader::spawned(0));
        if trader.customer.is_some() || (trader.initialized && trader.offers.is_empty()) {
            return Ok(false);
        }
        trader.ensure_offers()?;
        trader.customer = Some(customer.to_owned());
        self.trading_with = Some(id);
        Ok(true)
    }
    #[cfg(test)]
    pub fn damage(&mut self, amount: f32) {
        self.hurt_from(amount, HitSource::default());
    }
    fn hurt_from(&mut self, amount: f32, source: HitSource) -> bool {
        self.hurt_with_resistance(amount, source, self.effects.get(11).map(|e| e.amplifier))
    }
    fn damage_resisted(
        &mut self,
        amount: f32,
        armored: bool,
        absolute: bool,
        out_of_world: bool,
        resistance: Option<i32>,
    ) {
        let source = HitSource {
            unblockable: !armored,
            absolute,
            creative_harm: out_of_world,
            ..Default::default()
        };
        self.hurt_with_resistance(amount, source, resistance);
    }
    fn hurt_with_resistance(
        &mut self,
        mut amount: f32,
        source: HitSource,
        resistance: Option<i32>,
    ) -> bool {
        if (self.mode == Mode::Creative && !source.creative_harm)
            || self.health <= 0.
            || amount == 0.
            || (source.fire && self.effects.get(12).is_some())
        {
            return false;
        }
        if source.falling_object && self.inventory.slots[39].is_some() {
            let mut entity = JavaRandom::from_state(self.player_random_state);
            let wear = (amount * 4. + entity.next_float() * amount * 2.) as i32;
            self.player_random_state = entity.state();
            // The original keeps a broken helmet reference until damageArmor runs.
            self.wear_slot(39, wear, true);
            amount *= 0.75;
        }
        let fresh_hit = self.hurt_ticks <= 10;
        if fresh_hit {
            self.last_damage = amount;
            self.hurt_ticks = 20;
        } else {
            if amount <= self.last_damage {
                return false;
            }
            let difference = amount - self.last_damage;
            self.last_damage = amount;
            amount = difference;
        }
        self.damage_entity(amount, source, resistance);
        if fresh_hit {
            // The original integrated server sends status 2 to the hit player
            // even when armor, resistance, or absorption prevented health loss.
            self.hurt_camera.hurt();
            let mut entity = JavaRandom::from_state(self.player_random_state);
            if !source.drowning {
                entity.next_double();
            } // setBeenAttacked checks knockback resistance.
            if source.has_attacker {
                entity.next_double();
            } // knockBack checks it independently.
            let pitch = (entity.next_float() - entity.next_float()) * 0.2 + 1.;
            self.player_random_state = entity.state();
            let sound = if self.health <= 0. {
                "game.player.die"
            } else {
                "game.player.hurt"
            };
            self.damage_sounds.push((sound, 1., pitch));
        }
        true
    }
    fn damage_entity(&mut self, amount: f32, source: HitSource, resistance: Option<i32>) {
        let mut items = std::array::from_fn(|i| {
            self.inventory.slots[36 + i]
                .as_ref()
                .map(Stack::combat_item)
        });
        let mut entity = JavaRandom::from_state(self.player_random_state);
        let mut enchantment = JavaRandom::from_state(self.enchantment_random_state);
        let mut world = self.block_ticks.world_random();
        let context = DefenseContext {
            source,
            creative: self.mode == Mode::Creative,
            blocking: self.blocking(),
            resistance,
            absorption: self.absorption,
        };
        let defended = armor::defend(
            &mut items,
            amount,
            context,
            &mut entity,
            &mut world,
            &mut enchantment,
        );
        self.player_random_state = entity.state();
        self.enchantment_random_state = enchantment.state();
        self.block_ticks.set_world_random(&world);
        for (slot, result) in self.inventory.slots[36..40].iter_mut().zip(items) {
            if let Some(result) = result {
                if let Some(stack) = slot {
                    stack.count = result.count as u8;
                    stack.damage = result.damage as u16;
                }
            } else {
                *slot = None;
            }
        }
        self.damage_sounds.extend(
            defended
                .breaks
                .iter()
                .map(|event| ("random.break", 0.8, event.pitch)),
        );
        self.absorption = defended.absorption;
        if defended.health_damage != 0. {
            self.add_exhaustion(if source.unblockable { 0. } else { 0.3 });
            self.health = (self.health - defended.health_damage).clamp(0., self.max_health);
        }
    }
    fn wear_slot(&mut self, slot: usize, amount: i32, retain_empty: bool) {
        let Some(stack) = self.inventory.slots[slot].as_mut() else {
            return;
        };
        let mut item = stack.combat_item();
        let mut entity = JavaRandom::from_state(self.player_random_state);
        let mut world = self.block_ticks.world_random();
        if let Some(event) = armor::damage_item(
            &mut item,
            amount,
            self.mode == Mode::Creative,
            &mut entity,
            &mut world,
        ) {
            self.damage_sounds.push(("random.break", 0.8, event.pitch));
        }
        self.player_random_state = entity.state();
        self.block_ticks.set_world_random(&world);
        stack.count = item.count as u8;
        stack.damage = item.damage as u16;
        if stack.count == 0 && !retain_empty {
            self.inventory.slots[slot] = None;
        }
    }
    fn blocking(&self) -> bool {
        self.inventory.slots[self.inventory.selected]
            .as_ref()
            .is_some_and(|item| {
                self.item_use.identity() == Some(item.identity) && tool(item.id).0 == Tool::Sword
            })
    }
    fn update_motion_effects(&self, player: &mut Player) {
        player.speed_boost = self.effects.get(1).map(|e| e.amplifier);
        player.slowness = self.effects.get(2).map(|e| e.amplifier);
        player.jump_boost = self.effects.get(8).map(|e| e.amplifier);
        player.blinded = self.effects.get(15).is_some();
        player.depth_strider = self.inventory.armor_enchantment(8);
    }
    fn falling_impact(&mut self, impact: &crate::block_ticks::FallingImpact, player: &Player) {
        use crate::vanilla::movement::Aabb;
        use glam::DVec3;
        if impact.bounds.intersects(player.precise_bounds()) {
            // Both DamageSource.anvil and DamageSource.fallingBlock wear headgear.
            let hit = self.hurt_from(
                impact.amount,
                HitSource {
                    falling_object: true,
                    ..Default::default()
                },
            );
            if hit && self.health <= 0. {
                self.death_message_key = Some(if impact.anvil {
                    "death.attack.anvil"
                } else {
                    "death.attack.fallingBlock"
                });
            }
        }
        let mut enchantment = JavaRandom::from_state(self.enchantment_random_state);
        for mob in &mut self.mobs {
            if mob.health <= 0. || mob.kind == MobKind::EnderDragon {
                continue;
            }
            let brain = self.mob_brains.entry(mob.id).or_default();
            let scale = if brain.growing_age < 0 { 0.5 } else { 1. };
            let (width, height) = mob.kind.adult_size(self.command_entities.mobs.get(&mob.id));
            let radius = (width * scale * 0.5) as f64;
            let position = mob.position.as_dvec3();
            let bounds = Aabb {
                min: position - DVec3::new(radius, 0., radius),
                max: position + DVec3::new(radius, (height * scale) as f64, radius),
            };
            if !impact.bounds.intersects(bounds) {
                continue;
            }
            let amount = if mob.hurt > 0 {
                if impact.amount <= brain.last_damage {
                    continue;
                }
                impact.amount - brain.last_damage
            } else {
                mob.hurt = 10;
                impact.amount
            };
            brain.last_damage = impact.amount;
            // Current native mobs carry no equipment. Zombies have two innate
            // armor points. The original empty enchantment pass still draws.
            let points = if mob.kind == MobKind::Zombie { 2 } else { 0 };
            let damage = amount * (25 - points) as f32 / 25.;
            if damage > 0. {
                armor::protection(std::iter::empty(), HitSource::default(), &mut enchantment);
                mob.health = (mob.health - damage).max(0.);
                if mob.kind == MobKind::Villager && mob.health == 0. {
                    let home = self.trades.get(&mob.id).and_then(|shop| shop.career.home);
                    self.villager_homes.killed(
                        home,
                        crate::villagers::homes::reputation::Death::Unattributed {
                            nearby_player: mob.position.as_dvec3().distance_squared(
                                glam::DVec3::from_array(player.precise_position()),
                            ) < 256.,
                        },
                    );
                }
            }
        }
        self.enchantment_random_state = enchantment.state();
    }
    fn food_state(&self) -> FoodState {
        FoodState {
            level: self.food,
            saturation: self.saturation,
            exhaustion: self.exhaustion,
            timer: self.food_timer,
            previous_level: self.previous_food,
        }
    }
    fn set_food_state(&mut self, state: FoodState) {
        self.food = state.level;
        self.saturation = state.saturation;
        self.exhaustion = state.exhaustion;
        self.food_timer = state.timer;
        self.previous_food = state.previous_level;
    }
    fn food_context(&self, difficulty: u8) -> FoodContext {
        FoodContext {
            difficulty,
            natural_regeneration: self.natural_regeneration,
            health: self.health,
            max_health: self.max_health,
        }
    }
    fn difficulty(&self, settings: &Settings) -> u8 {
        if self.hardcore { 3 } else { self.world_difficulty.unwrap_or(settings.difficulty).min(3) }
    }
    fn add_exhaustion(&mut self, amount: f32) {
        let mut state = self.food_state();
        state.player_exhaustion(amount, self.mode == Mode::Creative, false);
        self.set_food_state(state);
    }
    fn tick_peaceful_food(&mut self, difficulty: u8) {
        let mut state = self.food_state();
        let heal = state.peaceful_prefix(self.food_context(difficulty), self.player_ticks);
        self.set_food_state(state);
        if heal {
            self.health = food::heal(self.health, self.max_health);
        }
    }
    fn tick_food(&mut self, difficulty: u8) {
        let mut state = self.food_state();
        let update = state.update(self.food_context(difficulty));
        self.set_food_state(state);
        if update.heal {
            self.health = food::heal(self.health, self.max_health);
        }
        if update.starve {
            self.damage_resisted(1., false, true, false, None);
        }
    }
    fn add_food_effect(&mut self, effect: Effect) {
        let mut effects = std::mem::take(&mut self.effects);
        let resistance = effects.get(11).map(|e| e.amplifier);
        effects
            .add(
                effect,
                &mut FoodPotionTarget {
                    sim: self,
                    resistance,
                },
            )
            .expect("Original food has a valid effect");
        self.effects = effects;
    }
    fn tick_potions(&mut self) {
        let mut effects = std::mem::take(&mut self.effects);
        let resistance = effects.get(11).map(|e| e.amplifier);
        effects.tick(&mut FoodPotionTarget {
            sim: self,
            resistance,
        });
        self.effects = effects;
    }
    fn start_eating(&mut self, item: Stack) {
        if let Ok(allowed) = item_food::can_start(
            FoodStack {
                id: item.id,
                meta: item.meta,
                count: i32::from(item.count),
            },
            &self.food_state(),
            self.mode == Mode::Creative,
        ) {
            self.item_use.start(item.identity, allowed);
            self.eating = if self.item_use.active() {
                (33 - self.item_use.remaining()).clamp(1, 32) as u8
            } else {
                0
            };
        }
    }
    fn tick_item_use(&mut self, right: bool, out: &mut Outcome) {
        self.tick_item_use_at(right, None, out);
    }
    fn tick_item_use_at(&mut self, right: bool, player: Option<&Player>, out: &mut Outcome) {
        if !right {
            self.item_use.stop();
        }
        let held = self.inventory.held();
        let update = self.item_use.tick(held.as_ref().map(|s| s.identity), false);
        let food_held = held
            .as_ref()
            .is_some_and(|s| food::food_values(s.id, s.meta).is_some());
        let drink_held = held
            .as_ref()
            .is_some_and(|s| s.id == 335 || (s.id == 373 && s.meta & 0x4000 == 0));
        let sound_pos =
            player.map_or(glam::DVec3::ZERO, |p| glam::DVec3::from_array(p.precise_position()));
        if update.animation && food_held && !drink_held {
            out.world_sounds
                .push(("random.eat", sound_pos, 0.5, 0.9 + unit(&mut self.rng) * 0.2));
            if let (Some(p), Some(item)) = (player, held.as_ref()) {
                self.emit_eating_crumbs(p, item.id, item.meta, 5);
            }
        }
        if (update.animation || update.finished) && drink_held {
            out.world_sounds.push((
                "random.drink",
                sound_pos,
                0.5,
                0.9 + unit(&mut self.rng) * 0.1,
            ));
        }
        if update.finished && drink_held {
            if let Some(item) = held.clone() {
                if item.id == 335 {
                    self.command_player_effect(command_entity_state::EffectEdit::Clear);
                    self.absorption = 0.;
                    if self.mode != Mode::Creative {
                        let empty = Stack::new(325, 1);
                        if item.count <= 1 {
                            self.inventory.slots[self.inventory.selected] = Some(empty);
                        } else {
                            self.inventory.take_selected(1);
                            if let Some(rest) = self.inventory.add(empty) {
                                if let Some(p) = player {
                                    self.throw_local_stack(p, rest, ThrowSource::Hotbar);
                                } else {
                                    self.spawn_drop(rest, Vec3::Y, Vec3::Y * 0.1);
                                }
                            }
                        }
                    }
                } else if item.id == 373 {
                    for eff in crate::ui::creative::potion_effects_for_stack(&item) {
                        if eff.id == 6 {
                            if self.health > 0. {
                                let heal = (4i32 << eff.amplifier.max(0)) as f32;
                                self.health = (self.health + heal).min(self.max_health);
                            }
                        } else if eff.id == 7 {
                            if self.mode == Mode::Survival
                                && self.hurt_from(
                                    (6i32 << eff.amplifier.max(0)) as f32,
                                    HitSource {
                                        unblockable: true,
                                        ..Default::default()
                                    },
                                )
                                && self.health <= 0.
                            {
                                self.death_message_key = Some("death.attack.magic");
                            }
                        } else if (1..=23).contains(&eff.id) {
                            self.command_player_effect(command_entity_state::EffectEdit::Add(
                                Effect::new(eff.id, eff.duration.max(1), eff.amplifier),
                            ));
                        }
                    }
                    if self.mode != Mode::Creative {
                        let bottle = Stack::new(374, 1);
                        if item.count <= 1 {
                            self.inventory.slots[self.inventory.selected] = Some(bottle);
                        } else {
                            self.inventory.take_selected(1);
                            if let Some(rest) = self.inventory.add(bottle) {
                                if let Some(p) = player {
                                    self.throw_local_stack(p, rest, ThrowSource::Hotbar);
                                } else {
                                    self.spawn_drop(rest, Vec3::Y, Vec3::Y * 0.1);
                                }
                            }
                        }
                    }
                }
            }
        } else if update.finished && food_held {
            if let Some(item) = held {
                if let Some(p) = player {
                    self.emit_eating_crumbs(p, item.id, item.meta, 16);
                }
                let mut food = self.food_state();
                let mut random = self.block_ticks.world_random();
                let result = item_food::finish(
                    &mut food,
                    &mut random,
                    FoodStack {
                        id: item.id,
                        meta: item.meta,
                        count: i32::from(item.count),
                    },
                    false,
                )
                .expect("Active use is a food");
                self.block_ticks.set_world_random(&random);
                self.set_food_state(food);
                self.inventory.slots[self.inventory.selected] = if result.stack.count == 0 {
                    None
                } else {
                    let mut stack = if result.stack.id == item.id {
                        item
                    } else {
                        Stack::new(result.stack.id, result.stack.count as u8)
                    };
                    stack.count = result.stack.count as u8;
                    stack.meta = result.stack.meta;
                    Some(stack)
                };
                for effect in result.effects {
                    self.add_food_effect(effect);
                }
                out.world_sounds
                    .push(("random.burp", sound_pos, 0.5, result.burp_pitch));
            }
        }
        self.eating = if (food_held || drink_held) && self.item_use.active() {
            (33 - self.item_use.remaining()).clamp(1, 32) as u8
        } else {
            0
        };
    }
    pub fn is_baby(&self, id: u64) -> bool {
        self.mob_brains.get(&id).is_some_and(|b| b.growing_age < 0)
            || crate::vanilla::item_nbt::number(self.command_entities.mobs.get(&id).and_then(|d| d.extra.get("IsBaby"))) != 0
    }
    fn exchange_held(&mut self, replacement: u16, position: Vec3) {
        if self.mode == Mode::Creative {
            return;
        }
        self.inventory.take_selected(1);
        let item = Stack::new(replacement, 1);
        if self.inventory.held().is_none() {
            self.inventory.slots[self.inventory.selected] = Some(item);
        } else if let Some(rest) = self.inventory.add(item) {
            self.spawn_drop(rest, position, Vec3::Y * 0.1);
        }
    }
    fn fire_outcome(&mut self, effects: crate::block_ticks::TickResult, out: &mut Outcome) {
        for p in effects.primed_tnt { self.spawn_primed(p,80,false); }
        out.changed.extend(effects.changed);
        out.pitched_sounds.extend(effects.sounds);
        out.world_sounds.extend(effects.world_sounds);
    }
    fn liquid_smoke(&mut self, p: IVec3, evaporating: bool) {
        let mut random = JavaRandom::from_state(self.particle_random[2]);
        for _ in 0..8 {
            let x = random.next_double();
            let y = if evaporating { random.next_double() } else { 1.2 };
            let z = random.next_double();
            let at = p.as_dvec3() + glam::DVec3::new(x, y, z);
            if self.particles.len() < 4000 {
                if let Some(smoke) = Particle::network_sprite(12, at, glam::DVec3::ZERO, &mut random) {
                    self.particles.push(smoke);
                }
            }
        }
        self.particle_random[2] = random.state();
    }
    pub(crate) fn on_sponge_added(
        &mut self,
        world: &mut World,
        pos: IVec3,
        state: u16,
        mut out: Option<&mut Outcome>,
    ) {
        if state >> 4 != 19 {
            return;
        }
        let wet = (state & 1) != 0;
        if self.dimension == -1 && wet {
            if world.set(pos, 19 << 4) {
                self.block_ticks.notify(world, pos);
                if let Some(o) = out.as_deref_mut() {
                    o.changed.push(pos);
                } else {
                    self.pending_changed_blocks.push(pos);
                }
            }
            let (_, vol, pitch) = self.block_ticks.fizz(world);
            let sound_pos = pos.as_dvec3() + glam::DVec3::splat(0.5);
            if let Some(o) = out {
                o.world_sounds.push(("random.fizz", sound_pos, vol, pitch));
            } else {
                self.pending_world_sounds
                    .push(("random.fizz", sound_pos, vol, pitch));
            }
            let top_open = world.get(pos + IVec3::Y) == 0;
            let mut random = JavaRandom::from_state(self.particle_random[2]);
            for _ in 0..8 {
                let x = random.next_double();
                let y = if top_open { 1.05 } else { random.next_double() };
                let z = random.next_double();
                let at = pos.as_dvec3() + glam::DVec3::new(x, y, z);
                if self.particles.len() < 4000 {
                    if let Some(smoke) =
                        Particle::network_sprite(12, at, glam::DVec3::ZERO, &mut random)
                    {
                        self.particles.push(smoke);
                    }
                }
            }
            self.particle_random[2] = random.state();
            return;
        }
        if !wet {
            let mut queue = std::collections::VecDeque::new();
            let mut absorbed = Vec::new();
            queue.push_back((pos, 0i32));
            let mut count = 0usize;
            const FACES: [IVec3; 6] = [
                IVec3::NEG_Y,
                IVec3::Y,
                IVec3::NEG_Z,
                IVec3::Z,
                IVec3::NEG_X,
                IVec3::X,
            ];
            while let Some((cur, dist)) = queue.pop_front() {
                for dir in FACES {
                    let next = cur + dir;
                    if !matches!(world.get(next) >> 4, 8 | 9) {
                        continue;
                    }
                    if world.set(next, 0) {
                        absorbed.push(next);
                        count += 1;
                        if dist < 6 {
                            queue.push_back((next, dist + 1));
                        }
                    }
                }
                if count > 64 {
                    break;
                }
            }
            if count > 0 {
                for &p in &absorbed {
                    self.block_ticks.notify(world, p);
                    if let Some(o) = out.as_deref_mut() {
                        o.changed.push(p);
                    } else {
                        self.pending_changed_blocks.push(p);
                    }
                }
                if world.set(pos, (19 << 4) | 1) {
                    self.block_ticks.notify(world, pos);
                    if let Some(o) = out.as_deref_mut() {
                        o.changed.push(pos);
                    } else {
                        self.pending_changed_blocks.push(pos);
                    }
                }
                self.block_destroy_effects(pos, 8 << 4);
            }
        }
    }
    pub(crate) fn step_particles(&mut self, world: &World) {
        let mut spawned = Vec::new();
        self.particles
            .retain_mut(|particle| particle.tick_with_spawns(world, &mut spawned));
        let room = 4000usize.saturating_sub(self.particles.len());
        self.particles.extend(spawned.into_iter().take(room));
    }
    pub(crate) fn emit_eating_crumbs(&mut self, player: &Player, id: u16, meta: u16, count: usize) {
        let mut rng = JavaRandom::from_state(self.particle_random[2]);
        emit_eating_crumbs_into(&mut self.particles, &mut rng, player, id, meta, count);
        self.particle_random[2] = rng.state();
    }
    fn displaced_drops(&mut self, world: &World, p: IVec3, state: u16) {
        if self.world_commands.rules.get("doTileDrops").is_some_and(|v| v == "false") { return; }
        if growth::farm_drop(state) {
            self.drop_farm_block(world, p, state, 0);
        } else {
            for item in block_drops(state, None, &mut self.rng) {
                self.spawn_drop(item, p.as_vec3() + Vec3::splat(0.5), Vec3::Y * 0.1);
            }
        }
    }
    fn use_item_on_block(
        &mut self,
        world: &mut World,
        item: Stack,
        hit: (IVec3, IVec3),
        out: &mut Outcome,
    ) -> bool {
        let (pos, adjacent) = hit;
        let state = world.get(pos);
        let id = state >> 4;
        let above = pos + IVec3::Y;
        if id == 122 {
            self.click_special_block(world, pos, false, out);
            self.place_ticks = 4;
            return true;
        }
        if item.id == 381 && id == 120 {
            return self.use_eye_on_frame(world, pos, out);
        }
        if item.id == 383 {
            let Some(kind) = u8::try_from(item.meta).ok().and_then(MobKind::from_id).filter(|k| k.has_egg()) else {
                return false;
            };
            if id == 52 {
                use crate::vanilla::drops::DropNbt as Tag;
                let mut tag = world.tile(pos).and_then(crate::vanilla::item_nbt::compound).cloned().unwrap_or_default();
                tag.insert("id".into(), Tag::String("MobSpawner".into()));
                // Vanilla's weighted entry, when present, overrides the base mob ID.
                if !tag.contains_key("SpawnData") {
                    tag.insert("EntityId".into(), Tag::String(kind.name().into()));
                }
                for (key, coordinate) in ["x", "y", "z"].into_iter().zip(pos.to_array()) {
                    tag.insert(key.into(), Tag::Int(coordinate));
                }
                self.world_commands.edit_tile(world, pos, state, Some(Tag::Compound(tag)));
                out.changed.push(pos);
            } else {
                let liquid = matches!(id, 8..=11);
                let at = if liquid { pos } else { adjacent };
                if !world.contains(at.as_vec3()) || !(0..256).contains(&at.y) { return false; }
                let fence_top = adjacent == above && matches!(id, 85 | 113 | 188..=192);
                let position = at.as_vec3() + Vec3::new(0.5, if liquid || fence_top { 0.5 } else { 0. }, 0.5);
                self.spawn_egg_mob(&item, kind, position, false);
            }
            if self.mode == Mode::Survival { self.inventory.take_selected(1); }
            self.place_ticks = 4;
            return true;
        }
        if matches!(item.id, 325..=327) {
            let (target, new_state, bucket) = if item.id == 325 {
                if !matches!(id, 8..=11) || state & 15 != 0 { return false; }
                (pos, 0, if id <= 9 { 326 } else { 327 })
            } else {
                if !(0..crate::world::HEIGHT).contains(&adjacent.y) || !world.contains(adjacent.as_vec3()) {
                    return false;
                }
                let old = world.get(adjacent);
                if crate::vanilla::material::is_solid(old) { return false; }
                if self.dimension == -1 && item.id == 326 {
                    let (_, vol, pitch) = self.block_ticks.fizz(world);
                    out.world_sounds.push((
                        "random.fizz",
                        adjacent.as_dvec3() + glam::DVec3::splat(0.5),
                        vol,
                        pitch,
                    ));
                    self.liquid_smoke(adjacent, true);
                    self.exchange_held(325, adjacent.as_vec3() + Vec3::Y);
                    self.place_ticks = 4;
                    return true;
                }
                if old != 0 && !crate::vanilla::material::is_liquid(old) {
                    self.displaced_drops(world, adjacent, old);
                }
                (adjacent, if item.id == 326 { 8 << 4 } else { 10 << 4 }, 325)
            };
            let prev_state = world.get(target);
            if prev_state != new_state && world.set(target, new_state) {
                out.changed.push(target);
                if crate::vanilla::rail_events::is_rail(prev_state) {
                    block_support::on_rail_removed(self, world, target, prev_state, out);
                }
                if new_state >> 4 == 8 {
                    for dir in [
                        IVec3::NEG_Y,
                        IVec3::Y,
                        IVec3::NEG_Z,
                        IVec3::Z,
                        IVec3::NEG_X,
                        IVec3::X,
                    ] {
                        let np = target + dir;
                        if world.get(np) == 19 << 4 {
                            self.on_sponge_added(world, np, 19 << 4, Some(out));
                        }
                    }
                }
            }
            self.exchange_held(bucket, target.as_vec3() + Vec3::Y);
            self.place_ticks = 4;
            return true;
        }
        if matches!(item.id,259|385) {
            if !(0..256).contains(&adjacent.y) || !world.contains(adjacent.as_vec3()) { return false; }
            if world.get(adjacent) == 0 {
                let mut pitch = JavaRandom::from_state(self.particle_random[2]);
                let sound_at = adjacent.as_dvec3() + glam::DVec3::splat(0.5);
                out.world_sounds.push(if item.id == 259 {
                    ("fire.ignite", sound_at, 1., pitch.next_float() * 0.4 + 0.8)
                } else {
                    (
                        "item.fireCharge.use",
                        sound_at,
                        1.,
                        (pitch.next_float() - pitch.next_float()) * 0.2 + 1.,
                    )
                });
                self.particle_random[2] = pitch.state();
                let effects = self.block_ticks.ignite_air(world, adjacent, self.dimension);
                self.fire_outcome(effects, out);
            }
            if self.mode == Mode::Survival {
                if item.id == 259 { self.wear_slot(self.inventory.selected,1,false); }
                else { self.inventory.take_selected(1); }
            }
            self.place_ticks = 4;
            return true;
        }
        if item.id == 351 && item.meta == 15 && self.apply_farm_bonemeal(world, pos, out) {
            return true;
        }
        if item.id == 338 {
            use crate::vanilla::hand_placement::{can_replace, place_sound, replaceable_for_click};
            let target = if replaceable_for_click(world, pos) { pos } else { adjacent };
            if !can_replace(world, target, 83, 1) || !world.set(target, 83 << 4) {
                return false;
            }
            if self.mode == Mode::Survival { self.inventory.take_selected(1); }
            self.place_ticks = 4;
            out.changed.push(target);
            let (s, v, p) = place_sound(83 << 4);
            out.world_sounds
                .push((s, target.as_dvec3() + glam::DVec3::splat(0.5), v, p));
            return true;
        }
        let mut changed = None;
        let mut consume = false;
        if tool(item.id).0 == Tool::Hoe && adjacent != pos - IVec3::Y && world.get(above) == 0 {
            let tilled = match (id, state & 15) {
                (2, _) | (3, 0) => Some(60 << 4),
                (3, 1) => Some(3 << 4),
                _ => None,
            };
            if let Some(tilled) = tilled {
                changed = Some((pos, tilled));
                if self.mode == Mode::Survival {
                    self.wear_slot(self.inventory.selected, 1, false);
                }
            }
        } else if adjacent == above && world.get(above) == 0 {
            let crop = match (item.id, id) {
                (295, 60) => Some(59),
                (361, 60) => Some(104),
                (362, 60) => Some(105),
                (391, 60) => Some(141),
                (392, 60) => Some(142),
                (372, 88) => Some(115),
                _ => None,
            };
            if let Some(crop) = crop {
                changed = Some((above, crop << 4));
                consume = true;
            }
        }
        if item.id == 351 && item.meta == 3 && id == 17 && state & 3 == 3
            && adjacent.y == pos.y && world.get(adjacent) == 0
        {
            let facing = match pos - adjacent {
                IVec3::Z => 0, IVec3::NEG_X => 1, IVec3::NEG_Z => 2, IVec3::X => 3,
                _ => return false,
            };
            changed = Some((adjacent, (127 << 4) | facing));
            consume = true;
        }
        if let Some((position, replacement)) = changed {
            if world.set(position, replacement) {
                out.changed.push(position);
                if consume && self.mode == Mode::Survival {
                    self.inventory.take_selected(1);
                }
                self.place_ticks = 4;
                let at = position.as_dvec3() + glam::DVec3::splat(0.5);
                if consume {
                    let (s, v, p) = crate::vanilla::hand_placement::place_sound(replacement);
                    out.world_sounds.push((s, at, v, p));
                } else {
                    out.world_sounds.push(("step.gravel", at, 1.0, 0.8));
                }
                return true;
            }
        }
        false
    }
    pub fn die(&mut self, player: &Player) {
        let deaths=self.statistics.entry("stat.deaths".into()).or_default();
        *deaths=deaths.wrapping_add(1);
        self.statistics.insert("stat.timeSinceDeath".into(),0);
        self.return_cursor(player);
        if self.keep_inventory {
            return;
        }
        if self.mob_loot {
            self.spawn_xp_total(
                player.position.as_dvec3(),
                self.xp.level.wrapping_mul(7).min(100),
            );
        }
        let items = self
            .inventory
            .slots
            .iter_mut()
            .filter_map(Option::take)
            .collect::<Vec<_>>();
        for item in items {
            let velocity =
                Vec3::new(unit(&mut self.rng) - 0.5, 0.2, unit(&mut self.rng) - 0.5) * 0.25;
            self.spawn_drop(item, player.position + Vec3::Y, velocity);
        }
    }
    pub fn respawn(&mut self, player: &mut Player, world: &World) {
        self.fire_ticks = 0;
        self.fire_flag = false;
        self.sleep = Default::default();
        if !self.keep_inventory {
            self.experience = 0;
            self.xp = Default::default();
        }
        self.health = self.max_health;
        self.food = 20;
        self.previous_food = 20;
        self.player_ticks = 0;
        self.saturation = 5.0;
        self.exhaustion = 0.0;
        self.food_timer = 0;
        self.effects = Effects::default();
        self.death_message_key = None;
        self.absorption = 0.;
        self.item_use.stop();
        self.eating = 0;
        self.hurt_camera = HurtCamera::default();
        self.hurt_ticks = 0;
        self.last_damage = 0.;
        self.air = 300;
        self.fall_distance = 0.0;
        self.clear_local_mounts(player);
        self.minecart_window = None;
        self.show_minecart_window = false;
        self.pig_boost_ticks = 0;
        self.pig_boost_total = 0;
        self.pig_speed = 0.0;
        let mut random = JavaRandom::new(JavaRandom::independent_seed());
        random.next_long();
        random.next_long();
        let feet = crate::vanilla::respawn::recreate(
            world,
            world.spawn.floor().as_ivec3(),
            &mut self.bed_spawn,
            &mut random,
        );
        self.player_random_state = random.state();
        let keys = std::mem::take(&mut player.keys);
        *player = Player::new(Vec3::ZERO);
        player.keys = keys;
        player.selected = self.inventory.selected;
        player.allow_flying = self.mode == Mode::Creative;
        player.set_pose_f64(feet, 0., 0., false);
    }
    fn kill_drops(&mut self, id:u64, kind: MobKind, position: Vec3, burning: bool) {
        let data=self.command_entities.mobs.get(&id);
        let hit=self.mob_brains.get(&id).is_some_and(|b| b.player_hit_ticks > 0);
        let size = species_ai::slime_size(data);
        let sheared_sheep =
            kind == MobKind::Sheep && species_ai::number(data, "Sheared") != 0;
        let sheep_color = if kind == MobKind::Sheep {
            species_ai::number(data, "Color").clamp(0, 15) as u16
        } else {
            0
        };
        let pig_saddled =
            kind == MobKind::Pig && species_ai::number(data, "Saddle") != 0;
        if matches!(kind, MobKind::Slime|MobKind::MagmaCube) && size>1 {
            let child_size=size/2;
            let count=2+random(&mut self.rng)%3;
            for i in 0..count {
                let offset=Vec3::new(((i%2) as f32-0.5)*size as f32/4.,0.5,((i/2) as f32-0.5)*size as f32/4.);
                let yaw = unit(&mut self.rng) * std::f32::consts::TAU;
                let child_id = self.spawn_registered_raw(kind, position + offset, yaw);
                self.mobs.last_mut().unwrap().health = (child_size * child_size) as f32;
                let child=self.command_entities.mobs.entry(child_id).or_default();
                child.extra.insert("Size".into(),crate::vanilla::drops::DropNbt::Int(child_size-1));
            }
        }
        if !self.mob_loot || kind == MobKind::Villager {
            return;
        }
        let data = self.command_entities.mobs.get(&id);
        let primary = match kind {
            MobKind::Cow => 363 + u16::from(burning),
            MobKind::Pig => 319 + u16::from(burning),
            MobKind::Sheep => 423 + u16::from(burning),
            MobKind::Chicken => 365 + u16::from(burning),
            MobKind::Zombie => 367,
            MobKind::Skeleton if species_ai::number(data, "SkeletonType") == 0 => 352,
            MobKind::Creeper => 289,
            _ => {
                let looting=if hit {self.inventory.held().map_or(0,|s|s.enchantment_level(21))} else {0};
                let mut rng=JavaRandom::new(random(&mut self.rng) as i64);
                let data=self.command_entities.mobs.get(&id);
                for stack in species_ai::loot(kind,data,burning,hit,looting,&mut rng) {
                    self.spawn_drop(stack,position+Vec3::Y*0.5,Vec3::Y*0.18);
                }
                let xp=match kind {MobKind::EnderDragon=>12000,MobKind::Wither=>50,
                    MobKind::Bat|MobKind::IronGolem|MobKind::SnowGolem=>0, MobKind::Blaze|MobKind::Guardian=>10,
                    MobKind::Slime|MobKind::MagmaCube=>size, kind if kind.hostile()=>5,_=>1+rng.next_int(3)};
                if xp>0 && (hit || kind==MobKind::EnderDragon){self.spawn_xp_total(position.as_dvec3(),xp);}
                return;
            }
        };
        let count = 1 + (random(&mut self.rng) % 2) as u8;
        self.spawn_drop(
            Stack::new(primary, count),
            position + Vec3::Y * 0.5,
            Vec3::Y * 0.18,
        );
        let extra = match kind {
            MobKind::Cow => Some(Stack::new(334, 1)),
            MobKind::Pig if pig_saddled => Some(Stack::new(329, 1)),
            MobKind::Sheep if !sheared_sheep => {
                let mut wool = Stack::new(35, 1);
                wool.meta = sheep_color;
                Some(wool)
            }
            MobKind::Chicken => Some(Stack::new(288, 1)),
            MobKind::Skeleton => Some(Stack::new(262, 1)),
            _ => None,
        };
        if let Some(extra_stack) = extra {
            self.spawn_drop(extra_stack, position + Vec3::Y * 0.6, Vec3::Y * 0.2);
        }
        if self.mob_loot {
            self.spawn_xp_total(position.as_dvec3(), if kind.hostile() { 5 } else { 1 });
        }
    }
    fn update_pick_sections(&mut self) {
        for mob in &self.mobs {
            let section = crate::vanilla::entity_pick::section(mob.position.as_dvec3());
            if let Some(entry) = self.pick_sections.get_mut(&mob.id) {
                if entry.0 == section {
                    entry.2 = self.total_ticks;
                    continue;
                }
            }
            self.pick_sequence += 1;
            self.pick_sections
                .insert(mob.id, (section, self.pick_sequence, self.total_ticks));
        }
        self.pick_sections
            .retain(|_, entry| entry.2 == self.total_ticks);
    }
    pub fn pick(
        &self,
        world: &World,
        player: &Player,
    ) -> Option<crate::vanilla::entity_pick::Hit<LocalTarget>> {
        self.pick_at(world, player, 1.)
    }
    pub(crate) fn pick_at(
        &self, world: &World, player: &Player, partial: f32,
    ) -> Option<crate::vanilla::entity_pick::Hit<LocalTarget>> {
        use crate::vanilla::entity_pick::{self, Candidate};
        let mobs = self.mobs.iter().enumerate().flat_map(|(i, mob)| {
            let feet = mob.position.as_dvec3();
            let section = entity_pick::section(feet);
            let base_inserted = self
                .pick_sections
                .get(&mob.id)
                .filter(|e| e.0 == section)
                .map_or(self.pick_sequence + 1 + i as u64, |e| e.1);
            if mob.kind == MobKind::EnderDragon {
                if mob.health <= 0.0 {
                    return Vec::new();
                }
                return melee::dragon_part_boxes(mob, self.mob_brains.get(&mob.id))
                    .into_iter()
                    .map(|(part_idx, center, bounds)| Candidate {
                        id: LocalTarget::Mob(mob.id),
                        bounds,
                        section: entity_pick::section(center),
                        inserted: base_inserted * 8 + part_idx as u64,
                        border: 0.1,
                        living_or_frame: false,
                    })
                    .collect();
            }
            let (width, height) = mob.kind.adult_size(self.command_entities.mobs.get(&mob.id));
            let scale = if self.is_baby(mob.id) { 0.5 } else { 1. };
            vec![Candidate {
                id: LocalTarget::Mob(mob.id),
                bounds: entity_pick::body(feet, width * scale, height * scale),
                section,
                inserted: base_inserted * 8,
                border: 0.1,
                living_or_frame: true,
            }]
        });
        let after_mobs = (self.pick_sequence + 1 + self.mobs.len() as u64) * 8;
        let falling = self
            .block_ticks
            .falling
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.dead)
            .map(|(i, e)| Candidate {
                id: LocalTarget::Falling(i),
                bounds: e.bounds,
                section: entity_pick::section(e.position),
                inserted: after_mobs + i as u64,
                border: 0.1,
                living_or_frame: false,
            });
        let tnt = self.primed_tnt.iter().enumerate().map(|(i,e)| Candidate {
            id: LocalTarget::Tnt(i),
            bounds: e.bounds,
            section: entity_pick::section(e.position),
            inserted: after_mobs + (self.block_ticks.falling.len() + i) as u64,
            border: 0.1,
            living_or_frame:false,
        });
        let crystals = self.end_crystals.iter().enumerate().map(|(i, c)| Candidate {
            id: LocalTarget::EndCrystal(c.id),
            bounds: entity_pick::body(c.position, 2.0, 2.0),
            section: entity_pick::section(c.position),
            inserted: after_mobs
                + (self.block_ticks.falling.len() + self.primed_tnt.len() + i) as u64,
            border: 0.1,
            living_or_frame: false,
        });
        let view = player.pick_view(self.mode == Mode::Creative, partial);
        let picked = view.pick(
            world,
            mobs.chain(falling).chain(tnt).chain(crystals),
            self.ridden_horse.or(self.ridden_pig).map(LocalTarget::Mob),
        );
        if self.minecarts.is_empty() && self.boats.is_empty() {
            return picked;
        }
        let origin = view.eye;
        let reach = if view.creative { 6.0 } else { 4.5 };
        let limit = match picked {
            Some(entity_pick::Hit::Entity { point, .. })
            | Some(entity_pick::Hit::Miss {
                point,
                face: None,
                ..
            }) => origin.distance(point).min(reach),
            Some(entity_pick::Hit::Block(hit)) => origin.distance(hit.point).min(reach),
            _ => reach,
        };
        let end = origin + view.look * reach;
        let best_t = if reach > 0.0 { limit / reach } else { 1.0 };
        if let Some((t, v_target)) = self.pick_local_vehicle(origin, end, best_t) {
            let point = origin + (end - origin) * t.clamp(0.0, 1.0);
            if !view.creative && origin.distance(point) > 3.0 {
                return Some(entity_pick::Hit::Miss {
                    point,
                    position: point.floor().as_ivec3(),
                    face: None,
                });
            }
            return Some(entity_pick::Hit::Entity {
                id: v_target,
                point,
                pointed: false,
            });
        }
        picked
    }
    pub fn middle_click(&mut self, world: &World, player: &Player, ctrl: bool) {
        use crate::vanilla::{entity_pick::Hit, middle_pick};
        let creative = self.mode == Mode::Creative;
        match self.pick(world, player) {
            Some(Hit::Block(hit)) => {
                let container = self.containers.get(&container_key(hit.position));
                let tuned = self.note_snapshot(world, hit.position);
                middle_pick::block_click(
                    &mut self.inventory,
                    world,
                    hit.position,
                    creative,
                    ctrl,
                    container,
                    tuned.as_ref().or_else(|| world.tile(hit.position)),
                );
            }
            Some(Hit::Entity {
                id: LocalTarget::Mob(id),
                ..
            }) if creative => {
                if let Some(item) = self
                    .mobs
                    .iter()
                    .find(|m| m.id == id)
                    .and_then(|m| middle_pick::entity(m.kind.vanilla_id(), None))
                {
                    middle_pick::select(&mut self.inventory, item, true);
                }
            }
            Some(Hit::Entity {
                id: LocalTarget::Minecart(id),
                ..
            }) if creative => {
                if let Some(item) = self.middle_pick_minecart(id) {
                    middle_pick::select(&mut self.inventory, item, true);
                }
            }
            Some(Hit::Entity {
                id: LocalTarget::Boat(id),
                ..
            }) if creative => {
                if let Some(item) = self.middle_pick_boat(id) {
                    middle_pick::select(&mut self.inventory, item, true);
                }
            }
            Some(Hit::Entity {
                id: LocalTarget::EndCrystal(_),
                ..
            }) => {}
            _ => {}
        }
    }
    pub(crate) fn destroy_picked_block(
        &mut self,
        world: &mut World,
        pos: IVec3,
        settings: &Settings,
        out: &mut Outcome,
    ) {
        let state = world.get(pos);
        if state == 0 {
            return;
        }
        let id = state >> 4;
        let dig_state = actual_dig_state(world, pos, state);
        let can_harvest = harvestable(state, self.inventory.held());
        let extra_tile = matches!(id, 140 | 144)
            .then(|| drops_local::extra_tile_drop(self, world, pos, state))
            .flatten();
        let banner_drop = matches!(id, 176 | 177).then(|| crate::banner::dropped(world, pos));
        if id == 25 {
            self.forget_note(pos);
        }
        if id == 34 && state & 7 < 6 {
            let base = pos - crate::vanilla::notes::SIDES[(state & 7) as usize];
            let piston = world.get(base);
            if matches!(piston >> 4, 29 | 33) && piston & 8 != 0 {
                if self.mode == Mode::Survival && self.world_commands.rules.get("doTileDrops").is_none_or(|s| s != "false") {
                    self.spawn_drop(Stack::new(piston >> 4, 1), base.as_vec3()+Vec3::splat(0.5), Vec3::Y*0.1);
                }
                if world.set(base, 0) {
                    out.changed.push(base);
                    self.block_ticks.notify(world, base);
                }
            }
        }
        let multiblock_handled =
            block_support::on_player_harvest_multiblock(self, world, pos, state, can_harvest, out);
        if matches!(id, 63 | 68 | 140) {
            self.containers.remove(&container_key(pos));
        }
        if banner_drop.is_some() || matches!(id, 52 | 63 | 68 | 140 | 144 | 149 | 150) {
            self.world_commands.edit_tile(world, pos, 0, None);
        }
        if matches!(id, 75 | 76 | 93 | 94 | 149 | 150) {
            self.purge_circuit_timers_at(pos, id);
        }
        if !multiblock_handled {
            if world.set(pos, 0) {
                out.changed.push(pos);
                self.block_ticks.notify(world, pos);
                if crate::vanilla::rail_events::is_rail(state) {
                    block_support::on_rail_removed(self, world, pos, state, out);
                }
            }
            if state & 8 == 0
                && (id == 175 || crate::vanilla::hand_placement::door_item(id).is_some())
            {
                let upper = pos + IVec3::Y;
                if world.get(upper) >> 4 == id && world.set(upper, 0) {
                    out.changed.push(upper);
                    self.block_ticks.notify(world, upper);
                }
            }
        }
        if id == 26 && !multiblock_handled {
            // Each neighboring bed checks for its own partner. A bed facing
            // another way can belong to an entirely different pair.
            let mut pending = vec![pos];
            while let Some(changed) = pending.pop() {
                for delta in [IVec3::NEG_X, IVec3::X, IVec3::NEG_Z, IVec3::Z] {
                    let p = changed + delta;
                    let bed = world.get(p);
                    if bed >> 4 == 26
                        && world.get(crate::vanilla::sleep::other_half(p, bed)) >> 4 != 26
                        && world.set(p, 0)
                    {
                        out.changed.push(p);
                        self.block_ticks.notify(world, p);
                        if self.mode == Mode::Survival && bed & 8 == 0 {
                            self.spawn_drop(
                                Stack::new(355, 1),
                                p.as_vec3() + Vec3::splat(0.5),
                                Vec3::Y * 0.15,
                            );
                        }
                        pending.push(p);
                    }
                }
            }
        }
        if self.mode == Mode::Survival {
            let do_drops = self
                .world_commands
                .rules
                .get("doTileDrops")
                .is_none_or(|s| s != "false");
            let drop_to_stack = |d: crate::vanilla::drops::DropStack| {
                let mut s = Stack::new(d.id, d.count.max(1) as u8);
                s.meta = d.meta.max(0) as u16;
                s.nbt = d.nbt.map(std::sync::Arc::new);
                s
            };
            if !multiblock_handled {
                if id == 97 || growth::farm_drop(state) {
                    let held = self.inventory.held();
                    let silk = held.as_ref().is_some_and(|s| s.enchantment_level(33) > 0)
                        && crate::vanilla::mining::can_silk(state);
                    if silk {
                        if do_drops {
                            if let Some(stack) = crate::vanilla::mining::silk(state) {
                                self.spawn_drop(
                                    stack,
                                    pos.as_vec3() + Vec3::splat(0.5),
                                    Vec3::Y * 0.15,
                                );
                            }
                        }
                    } else {
                        let fortune = held.as_ref().map_or(0, |s| s.enchantment_level(35));
                        self.drop_farm_block(world, pos, state, fortune);
                    }
                } else if do_drops {
                    let drops = if let Some(banner) = banner_drop {
                        vec![banner]
                    } else if id == 144 {
                        if state & 8 == 0 {
                            extra_tile.map(drop_to_stack).into_iter().collect()
                        } else {
                            Vec::new()
                        }
                    } else if id == 140 {
                        let mut items = vec![Stack::new(390, 1)];
                        if let Some(plant) = extra_tile {
                            items.push(drop_to_stack(plant));
                        }
                        items
                    } else {
                        block_drops(state, self.inventory.held(), &mut self.rng)
                    };
                    for item in drops {
                        self.spawn_drop(item, pos.as_vec3() + Vec3::splat(0.5), Vec3::Y * 0.15);
                    }
                }
            }
            let wear = self
                .inventory
                .held()
                .map_or(0, |held| match tool(held.id).0 {
                    Tool::Sword if crate::vanilla::mining::hardness(state) != 0. => 2,
                    Tool::Pick | Tool::Axe | Tool::Shovel
                        if crate::vanilla::mining::hardness(state) != 0. =>
                    {
                        1
                    }
                    Tool::Shears if matches!(id, 18 | 161 | 30 | 31 | 106 | 132 | 35) => 1,
                    _ => 0,
                });
            if wear > 0 {
                self.wear_slot(self.inventory.selected, wear, false);
            }
            if can_harvest {
                if let Some(exhaustion) = food::harvest_exhaustion(
                    state,
                    self.inventory.held().map(|item| item.id),
                    matches!(id, 176 | 177),
                    false,
                ) {
                    self.add_exhaustion(exhaustion);
                }
            }
        }
        let (s, v, p) = crate::vanilla::hand_placement::break_sound(dig_state);
        out.world_sounds
            .push((s, pos.as_dvec3() + glam::DVec3::splat(0.5), v, p));
        if settings.particles < 2 {
            self.block_destroy_effects(pos, dig_state);
        }
    }
    pub(crate) fn alert_owned_wolves(&mut self, target_id: u64, around: Vec3) {
        let owner = mob_pets::owner_id(&self.local_name);
        let Some((tkind, tpos)) = self
            .mobs
            .iter()
            .find(|m| m.id == target_id && m.health > 0.)
            .map(|m| (m.kind, m.position))
        else {
            return;
        };
        if matches!(tkind, MobKind::Creeper | MobKind::Ghast) {
            return;
        }
        if tkind == MobKind::Wolf
            && mob_pets::owned_by(self.command_entities.mobs.get(&target_id), &owner)
        {
            return;
        }
        let wolf_ids = self
            .mobs
            .iter()
            .filter(|m| {
                m.kind == MobKind::Wolf
                    && m.id != target_id
                    && m.health > 0.
                    && (m.position.distance_squared(around) < 576.
                        || m.position.distance_squared(tpos) < 576.)
                    && {
                        let data = self.command_entities.mobs.get(&m.id);
                        species_ai::tamed(data)
                            && species_ai::number(data, "Sitting") == 0
                            && mob_pets::owned_by(data, &owner)
                    }
            })
            .map(|m| m.id)
            .collect::<Vec<_>>();
        for wid in wolf_ids {
            self.mob_brains.entry(wid).or_default().target_mob = Some(target_id);
        }
    }
    fn vehicle_melee_damage(&self, player: &Player) -> f32 {
        let held = self.inventory.slots[self.inventory.selected].as_ref();
        let (kind, tier, _, _) = tool(held.map_or(0, |s| s.id));
        let tier = f64::from(if tier == 5 { 1 } else { tier });
        let base = match kind {
            Tool::Sword => 4. + tier,
            Tool::Axe => 3. + tier,
            Tool::Pick => 2. + tier,
            Tool::Shovel => 1. + tier,
            _ => 1.,
        };
        let weakness = self
            .effects
            .get(18)
            .map_or(0., |e| 0.5 * f64::from(e.amplifier.wrapping_add(1)));
        let strength = self
            .effects
            .get(5)
            .map_or(0., |e| 1.3 * f64::from(e.amplifier.wrapping_add(1)));
        let mut dmg = ((base - weakness) * (1. + strength)).clamp(0., 2048.) as f32;
        let critical = self.fall_distance > 0.
            && !player.grounded
            && !player.on_ladder
            && !player.in_water
            && !player.third_person.riding
            && self.effects.get(15).is_none();
        if critical {
            dmg *= 1.5;
        }
        dmg + 1.25 * held.map_or(0, |s| s.enchantment_level(16)) as f32
    }
    fn apply_left_actions(
        &mut self,
        actions: Vec<crate::vanilla::click_controller::Action>,
        entity_hit: Option<LocalTarget>,
        player: &mut Player,
        world: &mut World,
        settings: &Settings,
        out: &mut Outcome,
    ) {
        use crate::vanilla::click_controller::{self, Action};
        for action in actions {
            match action {
                Action::Swing => {
                    self.hand_swing
                        .start(crate::first_person::swing_duration(&self.effects));
                }
                Action::Attack => match entity_hit {
                    Some(LocalTarget::Mob(id)) => {
                        if let Some(i) = self.mobs.iter().position(|m| m.id == id) {
                            let before = self.mobs.get(i).map(|m| (m.id, m.kind, m.hurt));
                            self.attack_picked_mob(i, player, world, settings.particles, out);
                            if let Some((tid, tkind, prev_hurt)) = before {
                                let hit_landed = self.mobs.get(i).is_some_and(|m| {
                                    m.id == tid && (m.hurt > prev_hurt || m.health <= 0.)
                                });
                                if hit_landed {
                                    if matches!(tkind, MobKind::Spider | MobKind::CaveSpider) {
                                        self.mob_brains.entry(tid).or_default().anger_ticks =
                                            400 + (random(&mut self.rng) % 400) as u16;
                                    }
                                    if self.mobs.get(i).is_some_and(|m| m.id == tid && m.health > 0.) {
                                        self.alert_owned_wolves(tid, player.position);
                                    }
                                }
                            }
                        }
                    }
                    Some(LocalTarget::Minecart(id)) => {
                        let dmg = self.vehicle_melee_damage(player);
                        self.attack_minecart(id, world, player, dmg, out);
                    }
                    Some(LocalTarget::Boat(id)) => {
                        let dmg = self.vehicle_melee_damage(player);
                        self.attack_boat(id, player, dmg, out);
                    }
                    Some(LocalTarget::EndCrystal(id)) => {
                        self.destroy_end_crystal(id, world, player, settings, out);
                    }
                    _ => {}
                },
                Action::Destroy(p) => self.destroy_picked_block(world, p, settings, out),
                Action::Extinguish(p) => {
                    self.extinguish_fire_at(world, p, out);
                }
                Action::StepSound(p, state) => {
                    if let Some((clip, vol, pitch)) = click_controller::step_sound(state) {
                        out.world_sounds
                            .push((clip, p.as_dvec3() + glam::DVec3::splat(0.5), vol, pitch));
                    }
                }
                Action::Clicked(p) => self.click_special_block(world, p, false, out),
                Action::HitEffect(p, face) => self.block_hit_effect(world, p, face),
                Action::Dig(click_controller::Dig::Start, p, face) => {
                    self.extinguish_clicked_fire(world, p, face, out);
                }
                Action::Dig(..) | Action::Stage(..) => {}
            }
        }
    }
    fn process_interactions(
        &mut self,
        world: &mut World,
        player: &mut Player,
        settings: &Settings,
        buttons: (bool, bool),
        out: &mut Outcome,
    ) {
        let (left, right) = buttons;
        let mode = self.commands.player_mode.unwrap_or(u8::from(self.mode == Mode::Creative));
        if mode == 3 {
            self.click_controller = Default::default();
            self.break_target = None;
            self.breaking = 0.;
            return;
        }
        use crate::vanilla::ray_trace::Options as RayOptions;
        let picked = self.pick(world, player);
        let block_hit = picked.and_then(|h| h.block());
        let hit = block_hit.map(|h| (h.position, h.adjacent()));
        let use_hit = match self.inventory.held().map(|s| s.id) {
            Some(383) => hit.or_else(|| {
                player
                    .block_ray(world, 5., RayOptions::item(true))
                    .filter(|h| matches!(world.get(h.position) >> 4, 8..=11))
                    .map(|h| (h.position, h.adjacent()))
            }),
            Some(id @ (325..=327 | 333)) => player
                .block_ray(world, 5., RayOptions::item(matches!(id, 325 | 333)))
                .map(|h| (h.position, h.adjacent())),
            _ => hit,
        };
        let entity_hit = picked.and_then(|hit| hit.entity());
        let mob_hit = match entity_hit {
            Some(LocalTarget::Mob(id)) => self.mobs.iter().position(|m| m.id == id),
            _ => None,
        };
        use crate::vanilla::click_controller;
        let creative = self.mode == Mode::Creative;
        let using = self.item_use.active();
        let presses = self.click_controller.presses(left);
        let held = self.inventory.held();
        let can_edit = block_hit.is_none_or(|hit| crate::vanilla::click_border::can_edit(mode, world.get(hit.position) >> 4, held.as_ref()));
        let can_place = mode != 2 || block_hit.is_some_and(|hit|
            crate::vanilla::click_border::can_place_on(mode, world.get(hit.position) >> 4, held.as_ref()));
        let mut target = click_controller::target(world, player, self, picked, can_edit);
        let events = self.click_controller.press_phase(
            &mut target,
            presses,
            creative,
            self.inventory.held().as_ref(),
            using,
        );
        self.apply_left_actions(events, entity_hit, player, world, settings, out);
        let right = right && !self.click_controller.hitting;
        if right {
            self.right_click_delay = 4;
        }
        let held_nametag = self
            .inventory
            .held()
            .is_some_and(|s| {
                s.id == 421
                    && s.nbt
                        .as_deref()
                        .and_then(crate::vanilla::item_nbt::compound)
                        .and_then(|t| t.get("display"))
                        .and_then(crate::vanilla::item_nbt::compound)
                        .and_then(|d| d.get("Name"))
                        .is_some_and(|tag| matches!(tag, crate::vanilla::drops::DropNbt::String(n) if !n.trim().is_empty()))
            });
        let used_animal = if right && self.place_ticks == 0 {
            match entity_hit {
                Some(LocalTarget::Minecart(id)) => {
                    if self.interact_minecart(id, world, player, out) {
                        self.swing_arm();
                        self.place_ticks = 4;
                        true
                    } else {
                        false
                    }
                }
                Some(LocalTarget::Boat(id)) => {
                    if self.interact_boat(id, world, player) {
                        self.swing_arm();
                        self.place_ticks = 4;
                        true
                    } else {
                        false
                    }
                }
                _ => {
                    if mob_hit.is_some_and(|i| {
                        if held_nametag {
                            self.interact_pet_or_mooshroom_mut(i, player, out)
                                || self.interact_horse(i, player, out)
                        } else {
                            self.interact_horse(i, player, out)
                                || self.interact_pet_or_mooshroom_mut(i, player, out)
                        }
                    }) {
                        self.place_ticks = 4;
                        true
                    } else if let (Some(i), Some(item)) = (mob_hit, self.inventory.held()) {
                        let (id, kind, position) =
                            (self.mobs[i].id, self.mobs[i].kind, self.mobs[i].position);
                        if item.id == 383 && (kind.ageable() || kind == MobKind::Horse) {
                            if item.meta == u16::from(kind.vanilla_id()) {
                                self.spawn_egg_child(&item, kind, position, id);
                                if self.mode == Mode::Survival {
                                    self.inventory.take_selected(1);
                                }
                                self.hand_swing
                                    .start(crate::first_person::swing_duration(&self.effects));
                            }
                            self.place_ticks = 4;
                            true
                        } else if ai::breeding_food(kind, item.id)
                            && (!matches!(kind, MobKind::Wolf | MobKind::Ocelot)
                                || species_ai::tamed(self.command_entities.mobs.get(&id)))
                        {
                            let was_baby = self.is_baby(id);
                            if self.mob_brains.entry(id).or_default().feed() {
                                if self.mode == Mode::Survival {
                                    self.inventory.take_selected(1);
                                }
                                let (w, h) = kind.adult_size(self.command_entities.mobs.get(&id));
                                let scale = if was_baby { 0.5 } else { 1. };
                                self.emit_mob_particles(
                                    position,
                                    (w * scale, h * scale),
                                    if was_baby { 21 } else { 34 },
                                    7,
                                );
                                self.hand_swing
                                    .start(crate::first_person::swing_duration(&self.effects));
                                self.place_ticks = 4;
                                out.world_sounds
                                    .push(("random.eat", position.as_dvec3(), 0.5, 1.0));
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
            }
        } else {
            false
        };
        let used_pig_boost = if right
            && !used_animal
            && self.ridden_pig.is_some()
            && self.inventory.held().is_some_and(|s| s.id == 398)
        {
            if self.pig_boost_ticks == 0 && self.place_ticks == 0 {
                let dur = 140 + (random(&mut self.rng) % 841) as u32;
                self.pig_boost_ticks = dur;
                self.pig_boost_total = dur;
                if self.mode == Mode::Survival {
                    let slot = self.inventory.selected;
                    self.wear_slot(slot, 7, false);
                    if self.inventory.slots[slot].is_none() {
                        self.inventory.slots[slot] = Some(Stack::new(346, 1));
                    }
                }
                self.swing_arm();
                self.place_ticks = 4;
            }
            true
        } else {
            false
        };
        let tuned_note = right
            && !used_animal
            && !used_pig_boost
            && (!player.sneaking() || self.inventory.held().is_none())
            && block_hit.is_some_and(|hit| {
                let id = world.get(hit.position) >> 4;
                if id == 122 {
                    self.click_special_block(world, hit.position, false, out);
                    self.place_ticks = 4;
                    return true;
                }
                if id == 26
                    && (self.dimension != 0
                        || crate::assets::biome_colors::biome_at(world, hit.position) == 8)
                {
                    let _ = self.try_bed_with_outcome(world, player, hit.position, settings, out);
                    self.place_ticks = 4;
                    return true;
                }
                self.light_tnt_by_hand(world, hit.position, out)
                    || self.activate_circuit(world, hit.position, out)
                    || self.tune_note(world, hit.position)
                    || matches!(id, 63 | 68)
            });
        let used_vehicle_item = if right
            && !used_animal
            && !used_pig_boost
            && !tuned_note
            && self.place_ticks == 0
        {
            match self.inventory.held().map(|s| s.id) {
                Some(item_id @ (328 | 342 | 343 | 407 | 408))
                    if can_place
                        && block_hit.is_some_and(|hit| {
                            crate::vanilla::rail_events::is_rail(world.get(hit.position))
                        }) =>
                {
                    let pos = block_hit.unwrap().position;
                    if self.try_place_minecart_on_rail(world, player, pos, item_id, out) {
                        self.swing_arm();
                        self.place_ticks = 4;
                        true
                    } else {
                        false
                    }
                }
                Some(333) if mode != 2 || can_place => {
                    if self.try_place_boat(world, player, out) {
                        self.swing_arm();
                        self.place_ticks = 4;
                        true
                    } else {
                        false
                    }
                }
                _ => false,
            }
        } else {
            false
        };
        let used_block = tuned_note || right
            && can_place
            && !used_animal
            && !used_pig_boost
            && !used_vehicle_item
            && self.place_ticks == 0
            && self
                .inventory
                .held()
                .zip(use_hit)
                .is_some_and(|(item, hit)| self.use_item_on_block(world, item, hit, out));
        // Minecraft.rightClickMouse animates accepted block callbacks. Buckets
        // use Item.onItemRightClick even when their internal ray hits a block.
        if used_block && block_hit.is_some()
            && (tuned_note || !held.as_ref().is_some_and(|item| matches!(item.id, 325..=327)))
        {
            self.animate_block_use(held.as_ref());
        }
        if right && !used_animal && !used_pig_boost && !used_vehicle_item && !used_block {
            if let Some(item) = self.inventory.held() {
                if item.id == 335 {
                    self.item_use.start_for(item.identity, 32);
                    self.eating = if self.item_use.active() {
                        (33 - self.item_use.remaining()).clamp(1, 32) as u8
                    } else {
                        0
                    };
                } else if item.id == 373 {
                    if item.meta & 0x4000 != 0 {
                        if self.place_ticks == 0 {
                            let mut prng = JavaRandom::from_state(self.particle_random[2]);
                            let pitch = 0.4 / (prng.next_float() * 0.4 + 0.8);
                            out.world_sounds.push((
                                "random.bow",
                                glam::DVec3::from_array(player.precise_position()),
                                0.5,
                                pitch,
                            ));
                            self.hand_swing
                                .start(crate::first_person::swing_duration(&self.effects));
                            let splash_nbt = item.nbt.as_deref().cloned();
                            if self.mode == Mode::Survival {
                                self.inventory.take_selected(1);
                            }
                            self.mob_shots.push(mob_shots::Shot::thrown_splash_with_nbt(
                                player,
                                item.meta,
                                splash_nbt,
                                &mut prng,
                            ));
                            self.particle_random[2] = prng.state();
                            self.place_ticks = 4;
                        }
                    } else {
                        self.item_use.start_for(item.identity, 32);
                        self.eating = if self.item_use.active() {
                            (33 - self.item_use.remaining()).clamp(1, 32) as u8
                        } else {
                            0
                        };
                    }
                } else if food::food_values(item.id, item.meta).is_some() {
                    self.start_eating(item);
                } else if tool(item.id).0 == Tool::Sword {
                    self.item_use.start_for(item.identity, 72000);
                } else if item.id == 261 {
                    self.start_bow(&item);
                } else if self.place_ticks == 0 && item.id == 395 {
                    self.use_empty_map(player, &item);
                } else if self.place_ticks == 0 && matches!(item.id, 332 | 344 | 368 | 384) {
                    self.throw_held_projectile(player, &item, out);
                } else if self.place_ticks == 0 && matches!(item.id, 346 | 381) {
                    self.use_fishing_or_eye(world, player, out);
                } else if self.place_ticks == 0 && can_place
                    && (matches!(item.id, 323 | 355 | 397 | 425) || crate::vanilla::hand_placement::supported(item.id))
                {
                    if let Some(hit) = block_hit {
                        if self.place_held_block(world, player, &item, hit, out) {
                            self.animate_block_use(Some(&item));
                        }
                    }
                }
            }
        }
        let mut target = click_controller::target(world, player, self, picked, can_edit);
        let events = self.click_controller.held_phase(
            &mut target,
            left,
            creative,
            self.inventory.held().as_ref(),
            self.item_use.active(),
        );
        self.apply_left_actions(events, entity_hit, player, world, settings, out);
        self.break_target = self
            .click_controller
            .hitting
            .then_some(self.click_controller.current);
        self.breaking = self.click_controller.damage;
    }
    fn animate_block_use(&mut self, previous: Option<&Stack>) {
        self.hand_swing
            .start(crate::first_person::swing_duration(&self.effects));
        // Minecraft.rightClickMouse lowers a consumed stack, and every held
        // stack used on a block in Creative, after the block callback succeeds.
        // An exhausted stack follows normal unequipping; the original returns
        // from the block branch before resetting its equipped progress.
        if previous.zip(self.inventory.held().as_ref()).is_some_and(|(before, after)|
            self.mode == Mode::Creative || after.count != before.count)
        {
            self.equipped.reset_progress();
        }
    }
    pub(crate) fn place_held_block(
        &mut self,
        world: &mut World,
        player: &Player,
        item: &Stack,
        hit: crate::vanilla::ray_trace::Hit,
        out: &mut Outcome,
    ) -> bool {
        if item.id == 323 {
            return self.place_sign(world, player, item, hit, out);
        }
        if item.id == 397 {
            let changes = out.changed.len();
            self.place_skull(world, player, item, hit, out);
            return out.changed.len() != changes;
        }
        if item.id == 425 {
            let changes = out.changed.len();
            self.place_banner(world, item, hit, player.java_rotation()[0], out);
            return out.changed.len() != changes;
        }
        if item.id == 355 {
            if let Some(parts) = crate::vanilla::sleep::bed_placement(
                world,
                hit.position,
                hit.face,
                player.java_rotation()[0],
            ) {
                if world.set(parts[0].0, parts[0].1) {
                    out.changed.push(parts[0].0);
                    if world.set(parts[1].0, parts[1].1) {
                        out.changed.push(parts[1].0);
                    }
                }
                if self.mode == Mode::Survival {
                    self.inventory.take_selected(1);
                }
                self.place_ticks = 4;
                return true;
            }
            return false;
        }
        use crate::vanilla::{hand_placement, movement::Aabb};
        use glam::DVec3;
        let input = hand_placement::Input::from_hit(player, item, hit);
        let vehicle_boxes = self.vehicle_solid_boxes(false);
        let blocked = |bounds: Aabb| {
            player.precise_bounds().intersects(bounds)
                || vehicle_boxes.iter().any(|b| b.intersects(bounds))
                || self.primed_tnt.iter().any(|t| t.bounds.intersects(bounds))
                || self
                    .block_ticks
                    .falling
                    .iter()
                    .any(|entity| !entity.dead && entity.bounds.intersects(bounds))
                || self.mobs.iter().any(|mob| {
                    if mob.kind == MobKind::EnderDragon {
                        return mob.health > 0.0
                            && melee::dragon_part_boxes(mob, self.mob_brains.get(&mob.id))
                                .into_iter()
                                .any(|(_, _, b)| b.intersects(bounds));
                    }
                    let scale = if self.is_baby(mob.id) { 0.5 } else { 1. };
                    let (width, height) = mob.kind.adult_size(self.command_entities.mobs.get(&mob.id));
                    let radius = (width * scale * 0.5) as f64;
                    let feet = mob.position.as_dvec3();
                    Aabb {
                        min: feet - DVec3::new(radius, 0., radius),
                        max: feet + DVec3::new(radius, (height * scale) as f64, radius),
                    }
                    .intersects(bounds)
                })
        };
        let Ok(action) = hand_placement::plan(world, input, blocked) else {
            return false;
        };
        if !action.accepted {
            return false;
        }
        self.place_ticks = 4;
        if !action.consume {
            return true;
        }
        let mut writes = action.writes.into_iter();
        let Some((first, state)) = writes.next() else {
            return true;
        };
        if !world.set(first, state) {
            return true;
        }
        if state >> 4 == 25 {
            self.placed_note(first, item);
        }
        if state >> 4 == 137 { command_blocks::placed(self, world, first, item); }
        if matches!(state >> 4,93|149) {
            self.diode_placed(world,first,state);
        }
        out.changed.push(first);
        if state >> 4 == 19 {
            self.on_sponge_added(world, first, state, Some(out));
        }
        if crate::vanilla::rail_events::is_rail(state) {
            block_support::on_rail_added(self, world, first, out);
        }
        if matches!(state >> 4, 86 | 91) {
            self.build_mob(world, first, false, out);
        }
        for (p, s) in writes {
            if world.set(p, s) {
                out.changed.push(p);
            }
        }
        if self.mode == Mode::Survival {
            self.inventory.take_selected(1);
        }
        if item.id != 331 && hand_placement::door_block(item.id).is_none() {
            let (s, v, p) = hand_placement::place_sound(state);
            out.world_sounds
                .push((s, first.as_dvec3() + glam::DVec3::splat(0.5), v, p));
        }
        true
    }
    fn place_sign(
        &mut self,
        world: &mut World,
        player: &Player,
        item: &Stack,
        hit: crate::vanilla::ray_trace::Hit,
        out: &mut Outcome,
    ) -> bool {
        if !(1..6).contains(&hit.face) || !crate::vanilla::material::is_solid(world.get(hit.position)) {
            return false;
        }
        let p = hit.adjacent();
        if !(0..crate::world::HEIGHT).contains(&p.y)
            || !world.contains(p.as_vec3())
            || !crate::vanilla::material::replaceable(world.get(p))
            || [IVec3::NEG_Z, IVec3::Z, IVec3::NEG_X, IVec3::X]
                .into_iter()
                .any(|d| world.get(p + d) >> 4 == 81)
        {
            return false;
        }
        let state = if hit.face == 1 {
            let yaw = player.java_rotation()[0];
            let rot = (((f64::from(yaw + 180.0) * 16.0 / 360.0) + 0.5).floor() as i32 & 15) as u16;
            (63 << 4) | rot
        } else {
            (68 << 4) | u16::from(hit.face)
        };
        if !world.set(p, state) {
            return false;
        }
        let stored = item
            .nbt
            .as_deref()
            .and_then(crate::vanilla::item_nbt::compound)
            .and_then(|m| m.get("BlockEntityTag"));
        if let Some(bet) = stored {
            let tile = crate::vanilla::middle_pick::tile_snapshot(state >> 4, p, Some(bet), None);
            self.world_commands.edit_tile(world, p, state, tile);
        } else {
            let tile = crate::vanilla::middle_pick::tile_snapshot(63, p, None, None);
            self.world_commands.edit_tile(world, p, state, tile);
            self.pending_sign_edit = Some(p);
        }
        self.block_ticks.notify(world, p);
        out.changed.push(p);
        let (s, v, pitch) = crate::vanilla::hand_placement::place_sound(state);
        out.world_sounds
            .push((s, p.as_dvec3() + glam::DVec3::splat(0.5), v, pitch));
        if self.mode == Mode::Survival {
            self.inventory.take_selected(1);
        }
        self.place_ticks = 4;
        true
    }
    fn land(
        &mut self,
        world: &mut World,
        player: &Player,
        hit: crate::vanilla::movement::Landing,
        out: &mut Outcome,
    ) {
        if hit.state >> 4 == 60 {
            let mut random = self.block_ticks.world_random();
            let trample = random.next_float() < hit.distance - 0.5;
            self.block_ticks.set_world_random(&random);
            if trample && world.set(hit.position, 3 << 4) {
                out.changed.push(hit.position);
            }
        }
        if player.allow_flying || (hit.state >> 4 == 165 && !player.sneaking()) {
            return;
        }
        let boost = player
            .jump_boost
            .map_or(0., |level| level.wrapping_add(1) as f32);
        let amount = (hit.distance - 3. - boost).ceil();
        if amount <= 0. {
            return;
        }
        let feet = glam::DVec3::from_array(player.precise_position());
        out.world_sounds.push((
            if amount > 4. {
                "game.player.hurt.fall.big"
            } else {
                "game.player.hurt.fall.small"
            },
            feet,
            1.,
            1.,
        ));
        if self.hurt_from(
            amount,
            HitSource {
                fall: true,
                unblockable: true,
                ..Default::default()
            },
        ) && self.health <= 0.
        {
            self.death_message_key = Some(if hit.distance > 5. {
                "death.fell.accident.generic"
            } else {
                "death.attack.fall"
            });
        }
        let below = (feet - glam::DVec3::Y * f64::from(0.2f32))
            .floor()
            .as_ivec3();
        let state = world.get(below);
        if state != 0 {
            if hit.distance > 3.0 && !matches!(state >> 4, 166) {
                let mut gauss = crate::vanilla::spawning::EntityRandom::from_state(
                    self.particle_random[2],
                    None,
                );
                let factor = (0.2 + (hit.distance - 3.0).ceil() / 15.0).clamp(0.2, 2.5);
                let count = ((150.0 * factor) as usize).min(64);
                for _ in 0..count {
                    if self.particles.len() >= 4000 {
                        break;
                    }
                    let vel = glam::DVec3::new(
                        gauss.next_gaussian() * 0.15,
                        gauss.next_gaussian().abs() * 0.15,
                        gauss.next_gaussian() * 0.15,
                    );
                    if let Some(p) =
                        Particle::block_dust(feet, vel, state, below, &mut gauss.random)
                    {
                        self.particles.push(p);
                    }
                }
                self.particle_random[2] = gauss.random.state();
            }
            if let Some((clip, volume, pitch)) = crate::vanilla::click_controller::step_sound(state)
            {
                // The original mining sample table stores (volume + 1)/8 and pitch/2.
                out.world_sounds
                    .push((clip, feet, (volume * 8. - 1.) * 0.5, pitch * 1.5));
            }
        }
    }
    fn tick_player_with_boats(
        &mut self,
        world: &World,
        player: &mut Player,
    ) -> Option<crate::vanilla::movement::Landing> {
        let collision_world = LocalCollisionWorld {
            world,
            boats: self.vehicle_solid_boxes(true),
        };
        player.tick_with(&collision_world);
        let feet = glam::DVec3::from_array(player.precise_position());
        let dy = player.movement_delta()[1];
        let in_web = world.get(feet.floor().as_ivec3()) >> 4 == 30
            || world.get((feet + glam::DVec3::Y).floor().as_ivec3()) >> 4 == 30;
        if player.in_water || player.flying || in_web {
            self.fall_distance = 0.0;
            None
        } else if player.grounded {
            let mut under = IVec3::new(
                feet.x.floor() as i32,
                (feet.y - f64::from(0.2f32)).floor() as i32,
                feet.z.floor() as i32,
            );
            let mut block = world.get(under);
            if block >> 4 == 0 {
                let below = world.get(under - IVec3::Y);
                if matches!(below >> 4, 85 | 107 | 113 | 139 | 183..=192) {
                    block = below;
                    under -= IVec3::Y;
                }
            }
            let landing = (self.fall_distance > 0.0).then_some(crate::vanilla::movement::Landing {
                distance: self.fall_distance,
                state: block,
                position: under,
            });
            self.fall_distance = 0.0;
            landing
        } else {
            if dy < 0.0 {
                self.fall_distance = (f64::from(self.fall_distance) - dy) as f32;
            }
            None
        }
    }
    pub fn tick(
        &mut self,
        world: &mut World,
        player: &mut Player,
        _r: &Resources,
        settings: &Settings,
        left: bool,
        right: bool,
    ) -> Outcome {
        self.tick_local(world, player, settings, left, right)
    }
    pub(crate) fn tick_local(
        &mut self,
        world: &mut World,
        player: &mut Player,
        settings: &Settings,
        left: bool,
        right: bool,
    ) -> Outcome {
        let mut out = Outcome::default();
        self.total_ticks = self.total_ticks.wrapping_add(1);
        self.pickup_fx.tick();
        let mut debug_tick=self.commands.sample();
        self.presentation.tick();
        let play=self.statistics.entry("stat.playOneMinute".into()).or_default();*play=play.wrapping_add(1);
        if self.health>0. {let alive=self.statistics.entry("stat.timeSinceDeath".into()).or_default();*alive=alive.wrapping_add(1);}
        let mut weather_random = self.block_ticks.world_random();
        self.weather.tick(&mut weather_random);
        self.block_ticks.set_world_random(&weather_random);
        self.tick_sleep(world, player, &mut out);
        if let Some((_, remaining)) = &mut self.sleep_notice {
            *remaining = remaining.saturating_sub(1);
            if *remaining == 0 {
                self.sleep_notice = None;
            }
        }
        self.player_ticks = self.player_ticks.wrapping_add(1);
        if self.xp.cooldown > 0 {
            self.xp.cooldown -= 1;
        }
        self.place_ticks = self.place_ticks.saturating_sub(1);
        self.right_click_delay = self.right_click_delay.saturating_sub(1);
        let pressed = right && !self.right_was_down;
        self.right_was_down = right;
        self.commands.measure(&mut debug_tick,"weatherAndSleep");
        let was_using = self.item_use.active();
        let use_click = right && !was_using && (pressed || self.right_click_delay == 0);
        player.selected = self.inventory.selected;
        // Minecraft.runTick dispatches clicks before World.updateEntities. An
        // accepted click starts a 32-tick food use and ticks it to 31 immediately.
        self.process_interactions(world, player, settings, (left, use_click), &mut out);
        if !right && was_using {
            let bow_pos = glam::DVec3::from_array(player.precise_position());
            if let Some(shot) = self.release_bow(bow_pos, &mut out) {
                self.launch_bow(player, shot);
            }
        }
        self.tick_item_use_at(right, Some(player), &mut out);
        self.hand_swing
            .advance(crate::first_person::swing_duration(&self.effects));
        let held=self.inventory.held();
        let used=held.as_ref().filter(|s|Some(s.identity)==self.item_use.identity())
            .map(|s|if matches!(s.id,261|267|268|272|276|283){72000}else{32})
            .map(|total|(total-self.item_use.remaining()).max(0) as u32);
        let bow_ticks = held.as_ref().filter(|s| s.id == 261).and(used);
        self.equipped
            .tick_with_fov(held, used, player.fov_modifier(bow_ticks));
        self.commands.measure(&mut debug_tick,"playerInteractions");
        self.tick_local_maps(world, player);
        self.commands.measure(&mut debug_tick,"localMaps");
        self.hurt_ticks = self.hurt_ticks.saturating_sub(1);
        self.tick_potions();
        let difficulty = self.difficulty(settings);
        self.block_ticks.set_nether(self.dimension == -1);
        self.block_ticks.set_fire_environment(crate::vanilla::runtime::FireEnvironment {
            do_fire_tick: self.world_commands.rules.get("doFireTick").is_none_or(|v| v.eq_ignore_ascii_case("true")),
            raining: self.dimension == 0 && self.weather.strengths(1.).0 > 0.2,
            difficulty,
        }, self.dimension);
        self.tick_peaceful_food(difficulty);
        player.can_sprint = self.mode == Mode::Creative || self.food > 6;
        player.allow_flying = self.mode == Mode::Creative;
        player.using_item = self.item_use.active();
        self.update_motion_effects(player);
        if player.position.y < -64.0 {
            self.damage_resisted(4., false, false, true, None);
        }
        if self.mode == Mode::Survival {
            player.flying = false;
        }
        let was_in_portal = player.in_portal && !player.riding && !self.is_riding_local();
        if was_in_portal && self.dimension != 1 {
            let creative = self
                .commands
                .player_mode
                .map_or(self.mode == Mode::Creative, |m| m == 1);
            let max_ticks = if creative { 1 } else { 80 };
            player.portal_counter = player.portal_counter.saturating_add(1);
            if player.portal_counter >= max_ticks {
                player.portal_counter = max_ticks;
                player.portal_cooldown = 10;
                let [yaw, pitch] = player.java_rotation();
                out.portal_travel = Some(PortalTravel::Nether(
                    crate::dimension_world::NetherPortalEntry {
                        trigger_pos: glam::DVec3::from_array(player.precise_position()),
                        yaw,
                        pitch,
                        velocity: glam::DVec3::from_array(player.precise_velocity()),
                        last_portal_pos: player.last_portal_pos,
                        last_portal_vec: player.last_portal_vec,
                        teleport_direction: player.teleport_direction,
                    },
                ));
            }
        } else {
            player.portal_counter = player.portal_counter.saturating_sub(4);
        }
        {
            let mut prng = JavaRandom::from_state(self.particle_random[2]);
            let portal_rand = prng.next_float();
            self.particle_random[2] = prng.state();
            if let Some(pitch) = player.step_portal_client(
                self.effects.get(9).is_some_and(|e| e.duration > 60),
                portal_rand,
            ) {
                out.pitched_sounds.push(("portal.trigger", 0.25, pitch));
            }
        }
        let was_in_water = player.in_water;
        let pre_vel = player.precise_velocity();
        let old = player.position;
        // The integrated server runs EntityPlayerMP.onUpdateEntity (including
        // item collisions) before it accepts the position in the movement packet.
        self.tick_player_item_pickup(player, &mut out);
        self.update_pig_mount(world, player);
        self.update_mount(world, player, &mut out);
        self.tick_local_vehicles(world, player, &mut out);
        if player.sleeping || self.is_riding_local() {
            player.third_person.tick(glam::DVec3::ZERO, player.java_rotation()[0]);
        } else {
            player.third_person.swinging = self.hand_swing.in_progress();
            let landing = if self.boats.is_empty() {
                player.tick_local(world, &mut self.fall_distance)
            } else {
                self.tick_player_with_boats(world, player)
            };
            if let Some(landing) = landing {
                self.land(world, player, landing, &mut out);
            }
            player.apply_portal_contact_with_world(world);
            if !player.riding
                && !self.is_riding_local()
                && player.touching_end_portal()
                && out.portal_travel.is_none()
            {
                out.portal_travel = Some(PortalTravel::End);
            }
            if !player.is_spectator() {
                let mut fx_rng = JavaRandom::from_state(self.particle_random[2]);
                let feet = glam::DVec3::from_array(player.precise_position());
                if !was_in_water && player.in_water && pre_vel[1] < -0.2 {
                    out.world_sounds.push(emit_water_entry_splash(
                        &mut self.particles,
                        &mut fx_rng,
                        feet,
                        0.6,
                        pre_vel,
                    ));
                }
                if player.sprinting && player.grounded && !player.in_water && !player.flying {
                    let vel = player.precise_velocity();
                    emit_running_dust(
                        &mut self.particles,
                        world,
                        &mut fx_rng,
                        feet,
                        0.6,
                        vel[0],
                        vel[2],
                    );
                }
                if let Some((color, ambient)) = effects_liquid_color(&self.effects) {
                    emit_potion_swirl(
                        &mut self.particles,
                        settings.particles,
                        &mut fx_rng,
                        feet,
                        0.6,
                        1.8,
                        color,
                        ambient,
                        self.effects.get(14).is_some_and(|e| e.duration > 0),
                    );
                }
                self.particle_random[2] = fx_rng.state();
            }
            if player.touching_cactus() && self.hurt_from(1., HitSource::default()) && self.health <= 0. {
                self.death_message_key = Some("death.attack.cactus");
            }
        }
        if !self.is_riding_local()
            && player.grounded
            && !player.flying
            && (player.position - old).length_squared() > 0.001
            && self.total_ticks.is_multiple_of(8)
        {
            let feet = glam::DVec3::from_array(player.precise_position());
            let mut pos = IVec3::new(
                feet.x.floor() as i32,
                (feet.y - f64::from(0.2f32)).floor() as i32,
                feet.z.floor() as i32,
            );
            let mut state = world.get(pos);
            if state >> 4 == 0 {
                let below = world.get(pos - IVec3::Y);
                if matches!(below >> 4, 85 | 107 | 113 | 139 | 183..=192) {
                    pos -= IVec3::Y;
                    state = below;
                }
            }
            let above = world.get(pos + IVec3::Y);
            let step_state = if above >> 4 == 78 { 78 << 4 } else { state };
            if !matches!(step_state >> 4, 0 | 8..=11) {
                if let Some((clip, m_vol, m_pitch)) =
                    crate::vanilla::click_controller::step_sound(step_state)
                {
                    out.world_sounds.push((
                        clip,
                        feet,
                        (m_vol * 8.0 - 1.0) * 0.15,
                        m_pitch * 2.0,
                    ));
                }
            }
        }
        self.yaw = player.yaw;
        self.pitch = player.pitch;
        if self.mode == Mode::Survival && !player.is_spectator() && !player.sleeping {
            let riding = self.is_riding_local();
            let eyes_in_water = if riding {
                crate::vanilla::movement::eyes_in_water(
                    world,
                    glam::DVec3::from_array(player.precise_position()),
                    false,
                )
            } else {
                player.eyes_in_water
            };
            if !riding {
                if player.jumped {
                    let jumps = self.statistics.entry("stat.jump".into()).or_default();
                    *jumps = jumps.wrapping_add(1);
                    self.add_exhaustion(food::jump_exhaustion(player.jump_sprinting));
                }
                if let Some(amount) = food::movement_exhaustion(
                    player.movement_delta(),
                    food::Movement {
                        riding: false,
                        eyes_in_water,
                        in_water: player.in_water,
                        on_ladder: player.on_ladder,
                        on_ground: player.grounded,
                        sprinting: player.sprinting,
                    },
                ) {
                    self.add_exhaustion(amount);
                }
            }
            if eyes_in_water && self.effects.get(13).is_none() {
                self.air -= 1;
                if self.air <= -20 {
                    self.air = 0;
                    if self.hurt_from(
                        2.,
                        HitSource {
                            drowning: true,
                            unblockable: true,
                            ..Default::default()
                        },
                    ) && self.health <= 0.
                    {
                        self.death_message_key = Some("death.attack.drown");
                    }
                }
            } else {
                self.air = 300;
            }
        } else if !player.sleeping {
            self.air = 300;
        }
        self.tick_player_fire(world, player);
        self.tick_food(difficulty);
        self.commands.measure(&mut debug_tick,"playerAndEntities");
        let mut world_random = self.block_ticks.world_random();
        self.villager_homes.tick(
            &mut crate::villagers::homes::collection::LocalVillageWorld {
                world,
                mobs: &self.mobs,
                golem_spawns: &mut self.pending_golem_spawns,
                player_name: &self.local_name,
                player_alive: self.health > 0.,
            },
            &mut world_random,
        );
        for at in std::mem::take(&mut self.pending_golem_spawns) {
            self.spawn_registered_raw(MobKind::IronGolem,at.as_vec3(),std::f32::consts::PI);
        }
        self.block_ticks.set_world_random(&world_random);
        self.commands.measure(&mut debug_tick,"villages");
        self.tick_xp(world, player, &mut out);
        self.commands.measure(&mut debug_tick,"experienceOrbs");
        self.tick_dying_mobs();
        self.tick_entities(world, player, settings, &mut out);
        self.commands.measure(&mut debug_tick,"entities");
        self.seat_rider(player);
        self.update_pick_sections();
        self.commands.measure(&mut debug_tick,"entityPicking");
        self.random_block_ticks(
            world,
            player.position,
            settings.render_distance,
            difficulty,
            &mut out,
        );
        self.commands.measure(&mut debug_tick,"randomBlockTicks");
        if !self.pending_changed_blocks.is_empty() {
            out.changed.append(&mut self.pending_changed_blocks);
        }
        let mut circuits_ticked = false;
        let mut neighbor_cursor = 0usize;
        let mut changed_cursor = 0usize;
        let mut notified_cursor = 0usize;
        self.flush_neighbor_updates(
            world,
            player,
            &mut circuits_ticked,
            &mut neighbor_cursor,
            &mut changed_cursor,
            &mut notified_cursor,
            &mut out,
        );
        self.commands.measure(&mut debug_tick,"blockNeighborsAndCircuits");
        let blocks = self.block_ticks.tick(world);
        neighbor_cursor = 0;
        self.commands.measure(&mut debug_tick,"scheduledBlockTicks");
        for &p in &blocks.primed_tnt { self.spawn_primed(p,80,false); }
        for impact in &blocks.impacts {
            self.falling_impact(impact, player);
        }
        for (pos, powered) in blocks.note_updates {
            self.note_neighbor(world, pos, powered);
        }
        self.flush_note_events(world, &mut out);
        out.pitched_sounds.extend(blocks.sounds);
        out.world_sounds.extend(blocks.world_sounds);
        for p in blocks.mix_smoke { self.liquid_smoke(p, false); }
        out.changed.extend(blocks.changed);
        notified_cursor = out.changed.len();
        for p in blocks.detector_rails {
            self.update_detector_rail(world, p, &mut out);
        }
        self.commands.measure(&mut debug_tick,"blockResults");
        // Powered blocks and scheduled fire can both create a TNT entity during
        // this tick's block phase, before the first fuse decrement.
        self.tick_tnt(world, player, settings, &mut out);
        self.commands.measure(&mut debug_tick,"primedTnt");
        self.flush_neighbor_updates(
            world,
            player,
            &mut circuits_ticked,
            &mut neighbor_cursor,
            &mut changed_cursor,
            &mut notified_cursor,
            &mut out,
        );
        for (position, state) in blocks.drops {
            self.spawn_drop(Stack::block(state), position, Vec3::Y * 0.1);
        }
        for (position, state) in blocks.washed {
            self.displaced_drops(world, position, state);
        }
        self.commands.measure(&mut debug_tick,"blockUpdatesAndDrops");
        self.tick_hoppers(world);
        self.commands.measure(&mut debug_tick,"hoppers");
        self.tick_mob_spawners(world, player, difficulty);
        self.tick_spawner_spins(world, player.position, player.is_spectator(), false);
        self.commands.measure(&mut debug_tick,"mobSpawners");
        for (key, container) in &mut self.containers {
            let coordinates = key
                .split(',')
                .filter_map(|v| v.parse::<i32>().ok())
                .collect::<Vec<_>>();
            if coordinates.len() != 3 {
                continue;
            }
            let pos = IVec3::new(coordinates[0], coordinates[1], coordinates[2]);
            if !world.contains(pos.as_vec3()) {
                continue;
            }
            container.tick();
            if container.kind == crate::container::Kind::Furnace {
                let state = world.get(pos);
                if matches!(state >> 4, 61 | 62) {
                    let updated = ((if container.burn > 0 { 62 } else { 61 }) << 4) | (state & 15);
                    if updated != state && world.set(pos, updated) {
                        out.changed.push(pos);
                    }
                }
            }
        }
        for pos in &out.changed {
            if world.get(*pos) >> 4 != 25 {
                self.forget_note(*pos);
            }
            if world.get(*pos) == 0 {
                if let Some(container) = self.containers.remove(&container_key(*pos)) {
                    for stack in container.slots.into_iter().flatten() {
                        self.spawn_drop(stack, pos.as_vec3() + Vec3::splat(0.5), Vec3::Y * 0.15);
                    }
                }
            }
        }
        self.commands.measure(&mut debug_tick,"blocksAndContainers");
        self.step_particles(world);
        {
            let (rain_strength, _) = self.weather.strengths(1.0);
            let raining = self.dimension == 0 && rain_strength > 0.2;
            let creative_holding_barrier =
                self.mode == Mode::Creative && self.inventory.held().is_some_and(|s| s.id == 166);
            let mut ambient_sounds = Vec::new();
            crate::ambient_particles::tick_block_and_rain_particles(
                world,
                glam::DVec3::from_array(player.precise_position()),
                player.precise_eye(1.0),
                creative_holding_barrier,
                raining,
                if self.dimension == 0 { rain_strength } else { 0.0 },
                settings.particles,
                settings.fancy,
                self.total_ticks,
                &mut self.particles,
                &mut ambient_sounds,
            );
            for s in ambient_sounds {
                out.world_sounds.push((s.name, s.pos, s.volume, s.pitch));
            }
        }
        self.critical_emitters.tick(&mut self.particles, world, player.position.as_dvec3(), settings.particles, |id| {
            let mob = self.mobs.iter().find(|m| m.id == id)?;
            let (w, h) = mob.kind.adult_size(self.command_entities.mobs.get(&mob.id));
            let scale = if self.mob_brains.get(&id).is_some_and(|b| b.growing_age < 0) { 0.5 } else { 1. };
            Some(crate::vanilla::entity_pick::body(mob.position.as_dvec3(), w * scale, h * scale))
        });
        self.commands.measure(&mut debug_tick,"particles");
        local_commands::tick_scores(self);
        let damage_pos = glam::DVec3::from_array(player.precise_position());
        for (sound, vol, pitch) in self.damage_sounds.drain(..) {
            out.world_sounds.push((sound, damage_pos, vol, pitch));
        }
        out.world_sounds.append(&mut self.pending_world_sounds);
        if !self.pending_changed_blocks.is_empty() {
            out.changed.append(&mut self.pending_changed_blocks);
        }
        // The rendering client's living tick follows delivery of status/health
        // from the integrated server. Its first visible hurtTime is nine.
        self.hurt_camera.tick(self.health <= 0.);
        self.commands.measure(&mut debug_tick,"scoreboardAndFeedback");
        out
    }
    fn update_pig_mount(&mut self, world: &World, player: &mut Player) {
        self.pig_boost_ticks = self.pig_boost_ticks.saturating_sub(1);
        if self.pig_boost_ticks == 0 {
            self.pig_boost_total = 0;
        }
        let Some(pig_id) = self.ridden_pig else {
            return;
        };
        let pig_pos = self
            .mobs
            .iter()
            .find(|m| m.id == pig_id && m.kind == MobKind::Pig && m.health > 0.)
            .map(|m| m.position);
        let saddled =
            species_ai::number(self.command_entities.mobs.get(&pig_id), "Saddle") != 0;
        if pig_pos.is_none()
            || !saddled
            || self.health <= 0.
            || player.is_spectator()
            || player.sneaking()
        {
            if let Some(position) = pig_pos {
                use crate::vanilla::movement::{collision, Aabb};
                use glam::DVec3;
                let mut feet = position.as_dvec3() + DVec3::Y * 0.85;
                let mut found = false;
                'place: for y in [0., 1., -1.] {
                    for (x, z) in [
                        (1., 0.),
                        (-1., 0.),
                        (0., 1.),
                        (0., -1.),
                        (1., 1.),
                        (-1., 1.),
                        (1., -1.),
                        (-1., -1.),
                    ] {
                        let candidate = position.as_dvec3() + DVec3::new(x, y, z);
                        let bounds = Aabb::player(candidate);
                        if collision::colliders(world, bounds).is_empty()
                            && !collision::colliders(world, bounds.offset(-DVec3::Y * 0.1))
                                .is_empty()
                        {
                            feet = candidate;
                            found = true;
                            break 'place;
                        }
                    }
                }
                if !found {
                    for y in [0., 0.85, 1.5] {
                        let candidate = position.as_dvec3() + DVec3::Y * y;
                        if collision::colliders(world, Aabb::player(candidate)).is_empty() {
                            feet = candidate;
                            break;
                        }
                    }
                }
                player.set_position_f64(feet.to_array());
                player.set_velocity_f64([0.; 3]);
            }
            self.ridden_pig = None;
            self.pig_boost_ticks = 0;
            self.pig_boost_total = 0;
            self.fall_distance = 0.;
            player.riding = false;
            player.third_person.riding = false;
            player.third_person.mount_yaw = None;
        }
    }
    fn tick_dying_mobs(&mut self) {
        if self.dying_mobs.is_empty() {
            return;
        }
        let mut entity =
            crate::vanilla::spawning::EntityRandom::from_state(self.particle_random[1], None);
        let mut random = JavaRandom::from_state(self.particle_random[2]);
        let mut remaining = Vec::with_capacity(self.dying_mobs.len());
        for mut corpse in std::mem::take(&mut self.dying_mobs) {
            corpse.previous = corpse.position;
            corpse.hurt = corpse.hurt.saturating_sub(1);
            corpse.death_ticks = corpse.death_ticks.saturating_add(1);
            if corpse.death_ticks >= 20 {
                let (width, height) = corpse
                    .kind
                    .adult_size(self.command_entities.mobs.get(&corpse.id));
                let feet = corpse.position.as_dvec3();
                for _ in 0..20.min(4096usize.saturating_sub(self.particles.len())) {
                    let motion = glam::DVec3::from_array(std::array::from_fn(|_| {
                        entity.next_gaussian() * 0.02
                    }));
                    let offsets = glam::DVec3::new(
                        f64::from(entity.random.next_float() * width * 2.) - f64::from(width),
                        f64::from(entity.random.next_float() * height),
                        f64::from(entity.random.next_float() * width * 2.) - f64::from(width),
                    );
                    if let Some(particle) =
                        Particle::network_sprite(0, feet + offsets, motion, &mut random)
                    {
                        self.particles.push(particle);
                    }
                }
            } else {
                remaining.push(corpse);
            }
        }
        self.dying_mobs = remaining;
        self.particle_random[1] = entity.state().0;
        self.particle_random[2] = random.state();
    }
    fn flush_neighbor_updates(
        &mut self,
        world: &mut World,
        player: &mut Player,
        circuits_ticked: &mut bool,
        neighbor_cursor: &mut usize,
        changed_cursor: &mut usize,
        notified_cursor: &mut usize,
        out: &mut Outcome,
    ) {
        for _ in 0..64 {
            let all_neighbors = self.block_ticks.pending_neighbor_positions();
            let start_n = (*neighbor_cursor).min(all_neighbors.len());
            let new_neighbors = &all_neighbors[start_n..];
            *neighbor_cursor = all_neighbors.len();

            let start_c = (*changed_cursor).min(out.changed.len());
            let new_changed = out.changed[start_c..].to_vec();
            *changed_cursor = out.changed.len();

            if new_neighbors.is_empty() && new_changed.is_empty() && *circuits_ticked {
                break;
            }

            let support_before = out.changed.len();
            for &p in new_neighbors.iter().chain(new_changed.iter()) {
                self.check_banner_support(world, p, out);
                self.check_farm_support(world, p, out);
                block_support::check_neighbors_support(self, world, p, out);
                let cur = world.get(p);
                if self.dimension == -1 && cur == ((19 << 4) | 1) {
                    self.on_sponge_added(world, p, cur, Some(out));
                } else if matches!(cur >> 4, 8 | 9) {
                    for dir in [
                        IVec3::NEG_Y,
                        IVec3::Y,
                        IVec3::NEG_Z,
                        IVec3::Z,
                        IVec3::NEG_X,
                        IVec3::X,
                    ] {
                        let np = p + dir;
                        if world.get(np) == (19 << 4) {
                            self.on_sponge_added(world, np, 19 << 4, Some(out));
                        }
                    }
                }
            }

            let mut circuit_seed = Vec::with_capacity(
                new_neighbors.len() + new_changed.len() + (out.changed.len() - support_before),
            );
            circuit_seed.extend_from_slice(new_neighbors);
            circuit_seed.extend_from_slice(&new_changed);
            circuit_seed.extend_from_slice(&out.changed[support_before..]);
            *changed_cursor = out.changed.len();

            if !*circuits_ticked {
                *circuits_ticked = true;
                self.update_circuits(world, player, circuit_seed, out);
            } else if !circuit_seed.is_empty() {
                let mut already_ticked = std::mem::take(&mut self.redstone.moving);
                self.update_circuits(world, player, circuit_seed, out);
                already_ticked.append(&mut self.redstone.moving);
                self.redstone.moving = already_ticked;
            }

            let start_notify = (*notified_cursor).min(out.changed.len());
            for i in start_notify..out.changed.len() {
                let pos = out.changed[i];
                self.block_ticks.notify(world, pos);
            }
            *notified_cursor = out.changed.len();

            let total_neighbors = self.block_ticks.pending_neighbor_positions().len();
            if *neighbor_cursor >= total_neighbors && *changed_cursor >= out.changed.len() {
                break;
            }
        }
    }
    fn tick_entities(
        &mut self,
        world: &mut World,
        player: &mut Player,
        settings: &Settings,
        out: &mut Outcome,
    ) {
        let mut debug_entities = self.commands.sample();
        let difficulty = self.difficulty(settings);
        self.natural_spawns(world, player.position, difficulty);
        self.commands.measure_subsection(&mut debug_entities,"entities.spawn");
        let mut attacks = Vec::new();
        let mut prey_attacks = Vec::new();
        let mut explosions = Vec::new();
        let mut boss_explosions = Vec::new();
        let mut arrows = Vec::new();
        let mut eggs = Vec::new();
        let mut births = Vec::new();
        let mut trade_feedback = Vec::new();
        let held = self.inventory.held().map(|s| s.id);
        let (rain, thunder) = self.weather.strengths(1.);
        let sky_loss =
            crate::vanilla::atmosphere::skylight_subtracted(self.clock as i64, rain, rain * thunder);
        let neighbors = self
            .mobs
            .iter()
            .filter(|m| m.health > 0.0)
            .map(|m| {
                let brain = self.mob_brains.get(&m.id);
                (
                    m.id,
                    m.kind,
                    m.position,
                    brain.map_or(0, |b| b.growing_age),
                    brain.map_or(0, |b| b.love),
                )
            })
            .collect::<Vec<_>>();
        let climate = burn::Climate::new(self);
        let local_owner = std::cell::OnceCell::new();
        if self.dimension == 1 {
            for crystal in &self.end_crystals {
                let fpos = crystal.position.floor().as_ivec3();
                if world.contains(fpos.as_vec3())
                    && world.get(fpos) >> 4 != 51
                    && world.set(fpos, 51 << 4)
                {
                    self.block_ticks.notify(world, fpos);
                    out.changed.push(fpos);
                }
            }
        }
        let can_target_player = self.mode == Mode::Survival
            && self.commands.player_mode != Some(1)
            && !player.is_spectator()
            && self.health > 0.0;
        let armor_visibility = if self.effects.get(14).is_some() {
            (((36..=39)
                .filter(|&s| self.inventory.slots[s].is_some())
                .count() as f32)
                / 4.0)
                .max(0.1)
                * 0.7
        } else {
            1.0
        };
        if self.dimension == 1 {
            for crystal in &self.end_crystals {
                let fpos = crystal.position.floor().as_ivec3();
                if world.contains(fpos.as_vec3())
                    && world.get(fpos) >> 4 != 51
                    && world.set(fpos, 51 << 4)
                {
                    self.block_ticks.notify(world, fpos);
                    out.changed.push(fpos);
                }
            }
        }
        self.commands.measure_subsection(&mut debug_entities,"entities.setup_targeting");
        for i in 0..self.mobs.len() {
            let m = &mut self.mobs[i];
            if difficulty == 0 && m.kind.hostile() && m.kind!=MobKind::EnderDragon {
                m.health = -100.;
                self.commands.measure_subsection(&mut debug_entities,"entities.setup_targeting");
                continue;
            }
            m.age += 1;
            m.hurt = m.hurt.saturating_sub(1);
            m.swing_ticks = m.swing_ticks.saturating_sub(1);
            m.attack_ticks = m.attack_ticks.saturating_sub(1);
            m.rose_ticks = m.rose_ticks.saturating_sub(1);
            m.prev_fuse = m.fuse;
            if m.kind != MobKind::Horse {
                m.eat_ticks = m.eat_ticks.saturating_sub(1);
            }
            m.previous = m.position;
            let previous_yaw = m.yaw;
            if m.kind == MobKind::Skeleton
                && !self
                    .command_entities
                    .mobs
                    .get(&m.id)
                    .is_some_and(|d| d.equipment.contains_key(&99))
            {
                let skel_type =
                    species_ai::number(self.command_entities.mobs.get(&m.id), "SkeletonType");
                let weapon = if skel_type == 1 { 272 } else { 261 };
                self.command_entities
                    .mobs
                    .entry(m.id)
                    .or_default()
                    .equipment
                    .insert(99, Some(Stack::new(weapon, 1)));
            }
            let data = self.command_entities.mobs.get_mut(&m.id);
            if m.kind == MobKind::Wither && m.health > 0. {
                let invul = species_ai::number(data.as_deref(),"Invul");
                if invul>0 {
                    if let Some(data)=data { data.extra.insert("Invul".into(),crate::vanilla::drops::DropNbt::Int(invul-1)); }
                    if m.age%10==0 {m.health=(m.health+10.).min(300.);}
                    if invul==1 {boss_explosions.push((m.id, m.position+Vec3::Y*3.));}
                    self.commands.measure_subsection(&mut debug_entities,"entities.setup_targeting");
                    continue;
                }
                if m.age%20==0 {m.health=(m.health+1.).min(300.);}
            }
            let mut silent = false;
            if let Some(data) = data {
                if m.kind == MobKind::Endermite && world.contains(m.position) {
                    let mut lifetime = species_ai::number(Some(data), "Lifetime");
                    if !data.persistent {
                        lifetime = lifetime.saturating_add(1);
                        data.extra.insert("Lifetime".into(), crate::vanilla::drops::DropNbt::Int(lifetime));
                    }
                    if lifetime >= 2400 {
                        m.health = -100.;
                        self.commands.measure_subsection(&mut debug_entities,"entities.setup_targeting");
                        continue;
                    }
                }
                data.effect(m, command_entity_state::EffectEdit::Tick);
                silent = data.silent;
                if data.no_ai || (m.health <= 0. && m.kind != MobKind::EnderDragon) {
                    let brain = self.mob_brains.entry(m.id).or_default();
                    if m.kind == MobKind::EnderDragon && m.health > 0. {
                        brain.dragon_prev_wing_phase = brain.dragon_wing_phase;
                        brain.dragon_wing_phase = 0.5;
                    }
                    brain.gait.advance(previous_yaw, 0.);
                    brain.tick_revenge();
                    self.commands.measure_subsection(&mut debug_entities,"entities.setup_targeting");
                    if m.health > 0. && world.contains(m.position) {
                        let random = self.trades.get_mut(&m.id).map(|v| &mut v.career.random_state);
                        burn::tick_mob(world, climate, m, brain, Some(data), random, &mut self.enchantment_random_state, out);
                    }
                    self.commands.measure_subsection(&mut debug_entities,"entities.burn");
                    continue;
                }
            }
            if m.kind == MobKind::EnderDragon && m.health <= 0.0 {
                if m.health <= -99.0 {
                    self.commands.measure_subsection(&mut debug_entities,"entities.setup_targeting");
                    continue;
                }
                m.health = 0.0;
                let (dt, dpos) = {
                    let brain = self.mob_brains.entry(m.id).or_default();
                    brain.dragon_death_ticks = brain.dragon_death_ticks.saturating_add(1);
                    let dt = brain.dragon_death_ticks;
                    brain.dragon_prev_wing_phase = brain.dragon_wing_phase;
                    brain.dragon_wing_phase += 0.2;
                    if brain.dragon_wing_phase >= 1.0 {
                        let wrap = brain.dragon_wing_phase.floor();
                        brain.dragon_wing_phase -= wrap;
                        brain.dragon_prev_wing_phase -= wrap;
                    }
                    let base = m.position.as_dvec3();
                    if dt == 1 && !silent {
                        out.world_sounds
                            .push(("mob.enderdragon.end", base, 5.0, 1.0));
                    }
                    let mut prng = JavaRandom::from_state(self.particle_random[2]);
                    if self.particles.len() < 4096 {
                        let offset = glam::DVec3::new(
                            (f64::from(prng.next_float()) - 0.5) * 8.0,
                            2.0 + (f64::from(prng.next_float()) - 0.5) * 4.0,
                            (f64::from(prng.next_float()) - 0.5) * 8.0,
                        );
                        if let Some(p) =
                            Particle::network_sprite(1, base + offset, glam::DVec3::ZERO, &mut prng)
                        {
                            self.particles.push(p);
                        }
                    }
                    if (180..=200).contains(&dt) && self.particles.len() < 4096 {
                        let offset = glam::DVec3::new(
                            (f64::from(prng.next_float()) - 0.5) * 8.0,
                            2.0 + (f64::from(prng.next_float()) - 0.5) * 4.0,
                            (f64::from(prng.next_float()) - 0.5) * 8.0,
                        );
                        if let Some(p) =
                            Particle::network_sprite(2, base + offset, glam::DVec3::ZERO, &mut prng)
                        {
                            self.particles.push(p);
                        }
                    }
                    self.particle_random[2] = prng.state();
                    m.position.y += 0.1;
                    m.velocity = Vec3::new(0.0, 0.1, 0.0);
                    m.yaw += 20.0_f32.to_radians();
                    brain.gait.previous_yaw = Some(previous_yaw);
                    let pos_y = f64::from(m.position.y);
                    let yaw_deg = m.yaw.to_degrees();
                    brain
                        .dragon_flight
                        .get_or_insert_with(|| {
                            Box::new(crate::remote::dragon::Flight::new(pos_y, yaw_deg))
                        })
                        .record(pos_y, yaw_deg);
                    if dt >= 200 {
                        m.health = -100.0;
                    }
                    (dt, m.position)
                };
                if self.mob_loot && dt > 150 && dt.is_multiple_of(5) {
                    self.spawn_xp_total(dpos.as_dvec3(), 1000);
                }
                if dt >= 200 {
                    if self.mob_loot {
                        self.spawn_xp_total(dpos.as_dvec3(), 2000);
                    }
                    let changed =
                        crate::dimension_world::build_end_exit_podium_at(world, dpos, true);
                    for &cpos in &changed {
                        self.block_ticks.notify(world, cpos);
                    }
                    self.pending_changed_blocks.extend(changed.iter().copied());
                    out.changed.extend(changed);
                    let mut entity_rng = crate::vanilla::spawning::EntityRandom::from_state(
                        self.particle_random[1],
                        None,
                    );
                    let mut prng = JavaRandom::from_state(self.particle_random[2]);
                    let feet = dpos.as_dvec3();
                    for _ in 0..20.min(4096usize.saturating_sub(self.particles.len())) {
                        let motion = glam::DVec3::from_array(std::array::from_fn(|_| {
                            entity_rng.next_gaussian() * 0.02
                        }));
                        let offsets = glam::DVec3::new(
                            f64::from(entity_rng.random.next_float() * 32.0 - 16.0),
                            f64::from(entity_rng.random.next_float() * 8.0),
                            f64::from(entity_rng.random.next_float() * 32.0 - 16.0),
                        );
                        if let Some(p) =
                            Particle::network_sprite(0, feet + offsets, motion, &mut prng)
                        {
                            self.particles.push(p);
                        }
                    }
                    self.particle_random[1] = entity_rng.state().0;
                    self.particle_random[2] = prng.state();
                    self.end_dragon_slain = true;
                }
                self.commands.measure_subsection(&mut debug_entities,"entities.remaining");
                continue;
            }
            let mut diff = player.position - m.position;
            let mut distance = diff.length();
            let ambient_range = if m.kind == MobKind::EnderDragon { 160.0 } else { 24.0 };
            if !silent && m.kind != MobKind::Villager
                && distance < ambient_range
                && self
                    .clock
                    .wrapping_add(m.id.wrapping_mul(31))
                    .is_multiple_of(180)
            {
                if let Some(voice) = species_ai::ambient_for(m.kind, self.mob_brains.get(&m.id)) {
                    out.world_sounds.push((
                        voice,
                        m.position.as_dvec3(),
                        if matches!(m.kind, MobKind::Ghast | MobKind::EnderDragon) { 5. } else { 1. },
                        1.,
                    ));
                }
            }
            if !world.contains(m.position) {
                self.commands.measure_subsection(&mut debug_entities,"entities.setup_targeting");
                continue;
            }
            let brain = self.mob_brains.entry(m.id).or_default();
            brain.tick();
            if m.kind == MobKind::EnderDragon && m.health > 0.0 {
                if let Some(cid) = brain.dragon_healing_crystal {
                    if !self.end_crystals.iter().any(|c| c.id == cid) {
                        brain.dragon_healing_crystal = None;
                        melee::veer_dragon_on_hit(m, brain);
                        let invul = self
                            .command_entities
                            .mobs
                            .get(&m.id)
                            .is_some_and(|d| d.invulnerable);
                        if !invul
                            && melee::apply_mob_hit(
                                m,
                                brain,
                                self.command_entities.mobs.get_mut(&m.id),
                                10.0,
                                HitSource {
                                    explosion: true,
                                    ..Default::default()
                                },
                                &mut self.enchantment_random_state,
                            ) == Some(true)
                        {
                            melee::mob_hurt_feedback(m, brain, None, false, silent, out);
                        }
                        if m.health <= 0.0 {
                            self.commands.measure_subsection(&mut debug_entities,"entities.remaining");
                            continue;
                        }
                    } else if m.age.is_multiple_of(10) && m.health < m.kind.health() {
                        m.health = (m.health + 1.0).min(m.kind.health());
                    }
                }
                if brain.dragon_healing_crystal.is_none()
                    || random(&mut self.rng).is_multiple_of(10)
                {
                    let nearest = self
                        .end_crystals
                        .iter()
                        .map(|c| (c.id, (c.position.as_vec3() - m.position).length_squared()))
                        .filter(|&(_, d2)| d2 < 32.0 * 32.0)
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(cid, _)| cid);
                    brain.dragon_healing_crystal = nearest;
                }
            }
            if m.kind == MobKind::Sheep && m.health > 0. {
                let grief = self
                    .world_commands
                    .rules
                    .get("mobGriefing")
                    .is_none_or(|v| v.eq_ignore_ascii_case("true"));
                let p = m.position.floor().as_ivec3();
                if m.eat_ticks == 4 {
                    let ate = if world.get(p) >> 4 == 31 {
                        if grief && world.set(p, 0) {
                            out.changed.push(p);
                            self.block_ticks.notify(world, p);
                            let (clip, vol, pitch) =
                                crate::vanilla::hand_placement::break_sound(31 << 4);
                            out.world_sounds.push((
                                clip,
                                p.as_dvec3() + glam::DVec3::splat(0.5),
                                vol,
                                pitch,
                            ));
                        }
                        true
                    } else if world.get(p - IVec3::Y) >> 4 == 2 {
                        let below = p - IVec3::Y;
                        if grief && world.set(below, 3 << 4) {
                            out.changed.push(below);
                            self.block_ticks.notify(world, below);
                            let (clip, vol, pitch) =
                                crate::vanilla::hand_placement::break_sound(2 << 4);
                            out.world_sounds.push((
                                clip,
                                below.as_dvec3() + glam::DVec3::splat(0.5),
                                vol,
                                pitch,
                            ));
                        }
                        true
                    } else {
                        false
                    };
                    if ate {
                        self.command_entities
                            .mobs
                            .entry(m.id)
                            .or_default()
                            .extra
                            .insert("Sheared".into(), crate::vanilla::drops::DropNbt::Byte(0));
                        if brain.growing_age < 0 {
                            brain.growing_age = (brain.growing_age + 1200).min(0);
                        }
                    }
                } else if m.eat_ticks == 0
                    && brain.grounded
                    && (world.get(p) >> 4 == 31 || world.get(p - IVec3::Y) >> 4 == 2)
                {
                    let sheared =
                        species_ai::number(self.command_entities.mobs.get(&m.id), "Sheared") != 0;
                    let chance = if brain.growing_age < 0 {
                        50
                    } else if sheared {
                        400
                    } else {
                        1000
                    };
                    if random(&mut self.rng) % chance == 0 {
                        m.eat_ticks = 40;
                    }
                }
            }
            if m.kind==MobKind::Enderman {
                let wet=matches!(world.get(m.position.floor().as_ivec3())>>4,8|9) || (self.weather.strengths(1.).0>0.2 && world.rain_exposed(m.position.floor().as_ivec3()));
                species_ai::enderman_tick(
                    world,
                    m,
                    brain,
                    self.dimension,
                    sky_loss,
                    wet,
                    silent,
                    &mut self.rng,
                    &mut self.particle_random[2],
                    &mut self.particles,
                    out,
                );
                if m.health <= 0. {
                    self.commands.measure_subsection(&mut debug_entities,"entities.remaining");
                    continue;
                }
                diff = player.position - m.position;
                distance = diff.length();
            }
            if m.kind==MobKind::SnowGolem {
                use crate::assets::biome_colors::{biome_at,temperature};
                let feet=m.position.floor().as_ivec3();
                let wet=matches!(world.get(feet)>>4,8|9) || (self.weather.strengths(1.).0>0.2 && world.rain_exposed(feet));
                let hot=temperature(biome_at(world,feet),feet)>1.;
                if (wet || hot) && m.hurt==0 {m.health=(m.health-1.).max(0.);m.hurt=10;}
                if self.world_commands.rules.get("mobGriefing").is_none_or(|v|v.eq_ignore_ascii_case("true")){
                    for dx in [-0.25,0.25] {for dz in [-0.25,0.25] {
                        let p=(m.position+Vec3::new(dx,0.,dz)).floor().as_ivec3();
                        if world.get(p)==0 && crate::vanilla::spawning::solid_top(world.get(p-IVec3::Y)) && temperature(biome_at(world,p),p)<0.8 && world.set(p,78<<4) {out.changed.push(p);}
                    }}
                }
            }
            let mut trading = false;
            let mut avoid = None;
            let mut restriction = None;
            let data = self.command_entities.mobs.get(&m.id);
            let wither_skeleton =
                m.kind == MobKind::Skeleton && species_ai::number(data, "SkeletonType") == 1;
            let pet = matches!(m.kind, MobKind::Wolf | MobKind::Ocelot) && species_ai::tamed(data);
            let sitting = pet && species_ai::number(data, "Sitting") != 0;
            if let Some(tid) = brain.target_mob {
                let valid = !sitting
                    && neighbors.iter().any(|n| {
                        n.0 == tid
                            && !matches!(n.1, MobKind::Creeper | MobKind::Ghast)
                            && n.2.distance_squared(m.position) < 1024.
                    });
                if !valid {
                    brain.target_mob = None;
                }
            }
            let pumpkin = self.inventory.slots[39].as_ref().is_some_and(|s| s.id == 86);
            let hostile_reputation = can_target_player
                && m.kind == MobKind::IronGolem
                && species_ai::number(data,"PlayerCreated")==0
                && self.villager_homes.nearest(m.position.floor().as_ivec3()).is_some_and(|home|self.villager_homes.hostile_player(home,&self.local_name));
            let targets_player = hostile_reputation || species_ai::pursues(
                m,
                brain,
                data,
                player,
                pumpkin,
                can_target_player,
                armor_visibility,
                silent,
                world,
                sky_loss,
                self.dimension,
                out,
            );
            let base_range = match m.kind { MobKind::Ghast => 100., MobKind::Enderman => 64., MobKind::Blaze => 48., _ => 32. };
            let effective_range = if m.kind == MobKind::Enderman
                || brain.anger_ticks > 0
                || brain.player_hit_ticks > 0
            {
                base_range
            } else {
                base_range * armor_visibility
            };
            let target = targets_player && distance <= effective_range && can_target_player;
            let hunts_mobs = matches!(m.kind,MobKind::IronGolem|MobKind::SnowGolem|MobKind::Wolf|MobKind::Ocelot|MobKind::Wither|MobKind::Guardian);
            let defended_prey = if m.kind == MobKind::Wolf && pet && !sitting {
                brain.target_mob.and_then(|tid| neighbors.iter().find(|n| n.0 == tid))
            } else {
                None
            };
            let prey = defended_prey.or_else(|| {
                hunts_mobs
                    .then(|| {
                        neighbors
                            .iter()
                            .filter(|n| {
                                n.0 != m.id
                                    && species_ai::targets_other(m.kind, n.1, data)
                                    && n.2.distance_squared(m.position) < 256.
                            })
                            .min_by(|a, b| {
                                a.2.distance_squared(m.position)
                                    .total_cmp(&b.2.distance_squared(m.position))
                            })
                    })
                    .flatten()
            });
            if m.kind == MobKind::IronGolem
                && !target
                && prey.is_none()
                && m.rose_ticks == 0
                && self.dimension == 0
                && ai::clear_day(self.clock)
                && neighbors
                    .iter()
                    .any(|n| n.1 == MobKind::Villager && n.2.distance_squared(m.position) < 36.)
                && random(&mut self.rng) % 800 == 0
            {
                m.rose_ticks = 400;
            }
            if m.kind == MobKind::Villager {
                if let Some(trader) = self.trades.get_mut(&m.id) {
                    let career = &mut trader.career;
                    if career.regeneration_ticks > 0 {
                        if career.regeneration_ticks % 50 == 0 && m.health > 0. {
                            m.health = (m.health + 1.).min(m.kind.health());
                        }
                        career.regeneration_ticks -= 1;
                    }
                    if let Some(sound) = trader.tick_ambient(brain.growing_age < 0) {
                        trader
                            .pending
                            .push(crate::villagers::careers::TradeEffects {
                                sound: Some(sound),
                                ..Default::default()
                            });
                    }
                    let goals = brain.merchant_goals.get_or_insert_with(Default::default);
                    let water = matches!(world.get(m.position.floor().as_ivec3()) >> 4, 8 | 9);
                    let position = m.position.as_dvec3();
                    let customer = glam::DVec3::from_array(player.precise_position());
                    trading = goals.tick(
                        trader,
                        crate::villagers::goals::Conditions {
                            alive: m.health > 0.,
                            grounded: brain.grounded,
                            in_water: water,
                            velocity_changed: std::mem::take(&mut brain.velocity_changed),
                            customer_alive: self.health > 0.,
                            distance_squared: position.distance_squared(customer),
                        },
                    );
                    if goals.watching() {
                        m.yaw = diff.x.atan2(-diff.z);
                    }
                    if trader.check_home() {
                        let position = m.position.floor().as_ivec3();
                        self.villager_homes.enqueue(position);
                        trader.career.home = self.villager_homes.nearest(position);
                        trader.career.restriction = trader
                            .career
                            .home
                            .and_then(|id| self.villager_homes.restriction(id));
                    }
                    restriction = trader.career.restriction;
                    match trader.tick_restock(trader.career.home.is_some()) {
                        Ok(effect) => trader.pending.push(effect),
                        Err(error) => eprintln!("Villager {} restock: {error}", m.id),
                    }
                    if !trader.pending.is_empty() {
                        trade_feedback.push(m.id);
                    }
                }
                avoid = neighbors
                    .iter()
                    .filter(|n| n.1 == MobKind::Zombie && n.2.distance_squared(m.position) < 64.)
                    .min_by(|a, b| {
                        a.2.distance_squared(m.position)
                            .total_cmp(&b.2.distance_squared(m.position))
                    })
                    .map(|n| n.2);
            };
            let mating = if brain.love > 0 && brain.growing_age == 0 {
                neighbors
                    .iter()
                    .filter(|n| {
                        n.0 != m.id
                            && n.1 == m.kind
                            && n.3 == 0
                            && n.4 > 0
                            && n.2.distance_squared(m.position) < 64.0
                    })
                    .min_by(|a, b| {
                        a.2.distance_squared(m.position)
                            .total_cmp(&b.2.distance_squared(m.position))
                    })
            } else {
                None
            };
            let tempting = brain.panic == 0
                && brain.tempt_delay == 0
                && distance < 10.0
                && held
                    .as_ref()
                    .is_some_and(|item| ai::tempting_food(m.kind, *item));
            if brain.tempted && !tempting {
                brain.tempt_delay = 100;
            }
            brain.tempted = tempting;
            let parent = if brain.growing_age < 0 {
                neighbors
                    .iter()
                    .filter(|n| {
                        n.0 != m.id
                            && n.1 == m.kind
                            && n.3 >= 0
                            && n.2.distance_squared(m.position) < 256.0
                    })
                    .min_by(|a, b| {
                        a.2.distance_squared(m.position)
                            .total_cmp(&b.2.distance_squared(m.position))
                    })
            } else {
                None
            };
            let flee = if brain.flee_target.is_some()
                || (m.kind == MobKind::Villager && avoid.is_some())
                || (m.kind != MobKind::Villager && brain.panic > 0)
                || (!m.kind.hostile() && m.kind != MobKind::Villager && brain.fire_ticks > 0)
            {
                let random = self
                    .trades
                    .get_mut(&m.id)
                    .map(|v| &mut v.career.random_state);
                brain.flee(world, m, self.clock, avoid, restriction, random)
            } else {
                None
            };
            let mut requested_speed = 1.0;
            let mut destination = None;
            let mut stop_distance = 0.4;
            let ridden_this_pig =
                self.ridden_pig == Some(m.id) && species_ai::number(data, "Saddle") != 0;
            let owned_pet = pet
                && mob_pets::owned_by(
                    data,
                    local_owner.get_or_init(|| mob_pets::owner_id(&self.local_name)),
                );
            if owned_pet
                && !sitting
                && species_ai::number(data, "Leashed") == 0
                && !target
                && prey.is_none()
                && distance >= 12.0
            {
                let base = player.position.floor().as_ivec3();
                'teleport: for (dx, dz) in [
                    (-2, -2),
                    (2, -2),
                    (-2, 2),
                    (2, 2),
                    (-2, 0),
                    (2, 0),
                    (0, -2),
                    (0, 2),
                ] {
                    for dy in [-1, 0, 1] {
                        let at = base + IVec3::new(dx, dy, dz);
                        if world.contains(at.as_vec3())
                            && crate::vanilla::spawning::solid_top(world.get(at - IVec3::Y))
                            && !crate::vanilla::material::blocks_movement(world.get(at))
                            && !crate::vanilla::material::blocks_movement(world.get(at + IVec3::Y))
                            && !matches!(world.get(at) >> 4, 8..=11)
                        {
                            m.position = at.as_vec3() + Vec3::new(0.5, 0., 0.5);
                            m.previous = m.position;
                            m.velocity = Vec3::ZERO;
                            brain.movement = None;
                            brain.navigator.clear();
                            brain.path_target = None;
                            brain.path_timer = 0;
                            break 'teleport;
                        }
                    }
                }
            }
            if ridden_this_pig {
                brain.navigator.clear();
                brain.path_timer = 0;
                brain.path_target = None;
            } else if trading || sitting || m.eat_ticks > 0 {
                // A merchant stops navigating while serving a nearby living customer,
                // sitting pets stay put, and grazing sheep pause while eating grass.
                brain.navigator.clear();
                brain.path_timer = 0;
                brain.path_target = None;
                if trading || sitting {
                    m.yaw = diff.x.atan2(-diff.z);
                }
            } else if let Some(goal) = flee {
                destination = Some(goal);
                requested_speed = match m.kind {
                    MobKind::Cow => 2.,
                    MobKind::Chicken => 1.4,
                    MobKind::Villager => 0.6,
                    _ => 1.25,
                };
            } else if let Some(goal) = restriction.and_then(|home| {
                let random = self
                    .trades
                    .get_mut(&m.id)
                    .map(|t| &mut t.career.random_state);
                brain.toward_home(world, m, self.clock, home, random)
            }) {
                destination = Some(goal);
                requested_speed = 0.6;
            } else if let Some(mate) = mating {
                if brain.mate != Some(mate.0) {
                    brain.courtship = 0;
                }
                brain.mate = Some(mate.0);
                brain.courtship = brain.courtship.saturating_add(1);
                destination = Some(mate.2);
                stop_distance = 1.5;
                if brain.courtship >= 60
                    && m.id < mate.0
                    && m.position.distance_squared(mate.2) < 9.0
                {
                    births.push((m.id, mate.0, m.kind, m.position));
                }
            } else if tempting {
                destination = Some(player.position);
                stop_distance = 2.5;
                requested_speed = match m.kind {
                    MobKind::Cow => 1.25,
                    MobKind::Sheep => 1.1,
                    MobKind::Pig => 1.2,
                    _ => 1.,
                };
            } else if target {
                if m.kind == MobKind::Enderman {
                    let p_eye = player.precise_eye(1.0);
                    let d2 = m.position.as_dvec3().distance_squared(glam::DVec3::from_array(
                        player.precise_position(),
                    ));
                    if species_ai::enderman_stared_at(m, player, pumpkin, world) {
                        if d2 < 16.0 {
                            species_ai::teleport_enderman_randomly(
                                world,
                                m,
                                brain,
                                &mut self.rng,
                                &mut self.particle_random[2],
                                &mut self.particles,
                                silent,
                                out,
                            );
                            diff = player.position - m.position;
                            distance = diff.length();
                        }
                        destination = None;
                        brain.navigator.clear();
                        brain.path_target = None;
                        brain.path_timer = 0;
                        brain.enderman_teleport_delay = 0;
                        if diff.x * diff.x + diff.z * diff.z > 0.0001 {
                            m.yaw = diff.x.atan2(-diff.z);
                        }
                    } else {
                        destination = Some(player.position);
                        stop_distance = 1.1;
                        requested_speed = 1.0;
                        brain.enderman_teleport_delay =
                            brain.enderman_teleport_delay.saturating_add(1);
                        if d2 > 256.0
                            && brain.enderman_teleport_delay >= 30
                            && species_ai::teleport_enderman_to_entity(
                                world,
                                m,
                                brain,
                                p_eye,
                                &mut self.rng,
                                &mut self.particle_random[2],
                                &mut self.particles,
                                silent,
                                out,
                            )
                        {
                            brain.enderman_teleport_delay = 0;
                            diff = player.position - m.position;
                            distance = diff.length();
                        }
                    }
                } else {
                    destination = Some(player.position);
                    stop_distance = match m.kind {
                        MobKind::Skeleton if !wither_skeleton => 4.,
                        MobKind::Witch | MobKind::Guardian => 8.,
                        MobKind::Blaze | MobKind::Wither => 12.,
                        _=>1.1
                    };
                }
            } else if let Some(prey) = prey {
                destination = Some(prey.2);
                stop_distance = 1.;
            } else if owned_pet && distance > 5. {
                destination = Some(player.position);
                stop_distance = 2.;
            } else if let Some(parent) = parent {
                destination = Some(parent.2);
                stop_distance = 3.0;
                requested_speed = if m.kind == MobKind::Cow { 1.25 } else { 1.1 };
            } else {
                m.turn_ticks = m.turn_ticks.saturating_sub(1);
                if m.turn_ticks == 0 {
                    m.turn_ticks = 80 + (random(&mut self.rng) % 120) as u32;
                    let angle = unit(&mut self.rng) * std::f32::consts::TAU;
                    brain.wander_target = Some(
                        m.position
                            + Vec3::new(angle.sin(), 0.0, -angle.cos())
                                * (3.0 + unit(&mut self.rng) * 5.0),
                    );
                }
                if m.turn_ticks > 35 {
                    destination = brain.wander_target;
                    requested_speed = if m.kind == MobKind::Villager { 0.6 } else { 1. };
                }
            }
            if mating.is_none() {
                brain.mate = None;
                brain.courtship = 0;
            }
            let (width,height) = m.kind.adult_size(data);
            let scale = if brain.growing_age < 0 || species_ai::number(data,"IsBaby") != 0 {0.5} else {1.};
            let size = (width*scale,height*scale);
            let mut waypoint = None;
            let flight_target=if m.kind==MobKind::EnderDragon {(!player.is_spectator()).then_some(player.position)}
                else {target.then_some(player.position).or(prey.map(|p|p.2))};
            if m.kind == MobKind::EnderDragon && m.health > 0.0 {
                brain.dragon_prev_wing_phase = brain.dragon_wing_phase;
                let horiz_speed = m.velocity.with_y(0.0).length();
                let base_f = 0.2 / (horiz_speed * 10.0 + 1.0);
                let f = if brain.dragon_slowed {
                    base_f * 0.5
                } else {
                    base_f * 2.0_f32.powf(m.velocity.y)
                };
                brain.dragon_wing_phase += f;
                if brain.dragon_wing_phase >= 1.0 {
                    let wrap = brain.dragon_wing_phase.floor();
                    brain.dragon_wing_phase -= wrap;
                    brain.dragon_prev_wing_phase -= wrap;
                    if !silent {
                        out.world_sounds.push((
                            "mob.enderdragon.wings",
                            m.position.as_dvec3(),
                            5.0,
                            0.8 + unit(&mut self.rng) * 0.3,
                        ));
                    }
                }
                let pos_y = f64::from(m.position.y);
                let yaw_deg = m.yaw.to_degrees();
                brain
                    .dragon_flight
                    .get_or_insert_with(|| {
                        Box::new(crate::remote::dragon::Flight::new(pos_y, yaw_deg))
                    })
                    .record(pos_y, yaw_deg);
            }
            self.commands.measure_subsection(&mut debug_entities,"entities.setup_targeting");
            let was_grounded = brain.grounded;
            let free_motion = if ridden_this_pig {
                self.drive_ridden_pig(world, i, player);
                true
            } else if self.ridden_horse == Some(m.id) && horse::saddled(data) {
                horse::drive(world, m, brain, data, player, &mut self.horse_rider);
                true
            } else {
                ai::locomotion::free_move(world,m,brain,size,flight_target, &mut self.rng)
            };
            let m = &mut self.mobs[i];
            let brain = self.mob_brains.entry(m.id).or_default();
            self.commands.measure_subsection(&mut debug_entities,"entities.locomotion");
            if let Some(destination) = destination.filter(|_| !free_motion && m.kind != MobKind::Giant) {
                let horizontal = Vec3::new(
                    destination.x - m.position.x,
                    0.0,
                    destination.z - m.position.z,
                );
                if horizontal.length() > stop_distance || (destination.y - m.position.y).abs() > 1.0
                {
                    waypoint = brain.steer(
                        world,
                        m,
                        destination,
                        ai::Navigation {
                            size,
                            attack_target: target,
                            entity_destination: flee.is_none()
                                && (mating.is_some() || tempting || target || parent.is_some()),
                            difficulty,
                            avoid_sun: m.kind == MobKind::Skeleton
                                && !wither_skeleton
                                && ai::clear_day(self.clock),
                        },
                    );
                } else if horizontal.length_squared() > 0.01 {
                    m.yaw = horizontal.x.atan2(-horizontal.z);
                }
            }
            self.commands.measure_subsection(&mut debug_entities,"entities.navigation");
            if matches!(m.kind, MobKind::Spider | MobKind::CaveSpider)
                && target
                && (2.0..=6.0).contains(&distance)
                && brain.grounded
                && random(&mut self.rng) % 10 == 0
            {
                let horiz = Vec3::new(diff.x, 0., diff.z).normalize_or_zero();
                m.velocity.x = horiz.x * 0.4 + m.velocity.x * 0.2;
                m.velocity.z = horiz.z * 0.4 + m.velocity.z * 0.2;
                m.velocity.y = 0.4;
                brain.grounded = false;
                if let Some(loc) = brain.movement.as_mut() {
                    loc.body.velocity = m.velocity.as_dvec3();
                    loc.body.grounded = false;
                }
            }
            if !free_motion {
                ai::locomotion::drive_mob_with_effects(
                    world,
                    m,
                    brain,
                    self.command_entities.mobs.get(&m.id).map(|d| &d.effects),
                    size,
                    waypoint,
                    f64::from(requested_speed),
                );
            }
            self.commands.measure_subsection(&mut debug_entities,"entities.locomotion");
            if m.kind==MobKind::EnderDragon && m.health > 0.0 {
                let parts = melee::dragon_part_boxes(m, Some(brain));
                if m.hurt == 0 && !player.is_spectator() {
                    let pbounds = player.precise_bounds();
                    let wing_box_1 = parts[5]
                        .2
                        .expand(glam::DVec3::new(4.0, 2.0, 4.0))
                        .offset(glam::DVec3::new(0.0, -2.0, 0.0));
                    let wing_box_2 = parts[6]
                        .2
                        .expand(glam::DVec3::new(4.0, 2.0, 4.0))
                        .offset(glam::DVec3::new(0.0, -2.0, 0.0));
                    let body_wing_hit = parts[1].2.intersects(pbounds)
                        || wing_box_1.intersects(pbounds)
                        || wing_box_2.intersects(pbounds);
                    let head_hit = parts[0].2.expand(glam::DVec3::ONE).intersects(pbounds);
                    if head_hit && can_target_player {
                        attacks.push((
                            10.0,
                            None,
                            MobKind::EnderDragon,
                            false,
                            Some(m.id),
                            m.position,
                            silent,
                        ));
                    }
                    if body_wing_hit {
                        if self.mode != Mode::Creative {
                            let center = (parts[1].2.min + parts[1].2.max) * 0.5;
                            let ppos = glam::DVec3::from_array(player.precise_position());
                            let dx = ppos.x - center.x;
                            let dz = ppos.z - center.z;
                            let len = (dx * dx + dz * dz).sqrt().max(0.1);
                            let mut vel = player.precise_velocity();
                            vel[0] += dx / len * 4.0;
                            vel[1] += 0.2;
                            vel[2] += dz / len * 4.0;
                            player.set_velocity_f64(vel);
                        }
                        if can_target_player {
                            attacks.push((
                                5.0,
                                None,
                                MobKind::EnderDragon,
                                false,
                                Some(m.id),
                                m.position,
                                silent,
                            ));
                        }
                    }
                }
                let grief=self.world_commands.rules.get("mobGriefing").is_none_or(|v|v.eq_ignore_ascii_case("true"));
                brain.dragon_slowed=false;
                for box_aabb in [parts[0].2, parts[1].2] {
                    let lo = box_aabb.min.floor().as_ivec3();
                    let hi = box_aabb.max.floor().as_ivec3();
                    let mut destroyed_any = false;
                    for x in lo.x..=hi.x {for y in lo.y..=hi.y {for z in lo.z..=hi.z {
                        let pos=IVec3::new(x,y,z);let block=world.get(pos)>>4;
                        if block==0{continue;}
                        if !grief || matches!(block,7|49|121|137|166) {brain.dragon_slowed=true;continue;}
                        if world.set(pos,0){
                            self.world_commands.edit_tile(world,pos,0,None);
                            out.changed.push(pos);
                            self.block_ticks.notify(world,pos);
                            destroyed_any = true;
                        }
                    }}}
                    if destroyed_any && self.particles.len() < 4096 {
                        let mut prng = JavaRandom::from_state(self.particle_random[2]);
                        let span = box_aabb.max - box_aabb.min;
                        let ppos = box_aabb.min
                            + glam::DVec3::new(
                                span.x * f64::from(prng.next_float()),
                                span.y * f64::from(prng.next_float()),
                                span.z * f64::from(prng.next_float()),
                            );
                        if let Some(p) =
                            Particle::network_sprite(1, ppos, glam::DVec3::ZERO, &mut prng)
                        {
                            self.particles.push(p);
                        }
                        self.particle_random[2] = prng.state();
                    }
                }
            }
            brain.gait.previous_yaw = Some(previous_yaw);
            let villager_random = self.trades.get_mut(&m.id).map(|v| &mut v.career.random_state);
            self.commands.measure_subsection(&mut debug_entities,"entities.remaining");
            burn::tick_mob(world, climate, m, brain, self.command_entities.mobs.get_mut(&m.id), villager_random, &mut self.enchantment_random_state, out);
            self.commands.measure_subsection(&mut debug_entities,"entities.burn");
            if m.health <= 0. {
                if m.kind == MobKind::Villager {
                    let home = self.trades.get(&m.id).and_then(|shop| shop.career.home);
                    self.villager_homes.killed(home, crate::villagers::homes::reputation::Death::Unattributed {
                        nearby_player: distance < 16.,
                    });
                }
                self.commands.measure_subsection(&mut debug_entities,"entities.remaining");
                continue;
            }
            let data = self.command_entities.mobs.get(&m.id);
            if distance < 32.0 {
                let mut prng = JavaRandom::from_state(self.particle_random[2]);
                let feet = m.position.as_dvec3();
                if !was_grounded
                    && brain.grounded
                    && matches!(m.kind, MobKind::Slime | MobKind::MagmaCube)
                {
                    let slime_size = species_ai::slime_size(data).max(1);
                    emit_slime_landing_particles(
                        &mut self.particles,
                        &mut prng,
                        feet,
                        slime_size,
                        m.kind == MobKind::MagmaCube,
                    );
                }
                emit_mob_ambient_particles(
                    &mut self.particles,
                    settings.particles,
                    &mut prng,
                    m.kind.vanilla_id(),
                    feet,
                    size.0,
                    size.1,
                );
                if let Some(d) = data {
                    if let Some((color, ambient)) = effects_liquid_color(&d.effects) {
                        emit_potion_swirl(
                            &mut self.particles,
                            settings.particles,
                            &mut prng,
                            feet,
                            size.0,
                            size.1,
                            color,
                            ambient,
                            d.effects.get(14).is_some_and(|e| e.duration > 0),
                        );
                    }
                }
                self.particle_random[2] = prng.state();
            }
            if let Some(prey) = prey.filter(|p| !target && p.2.distance_squared(m.position) < (size.0 + 1.).powi(2) && brain.attack_timer == 0) {
                if let Some(damage) = species_ai::melee_damage(m.kind, data, &mut self.rng) {
                    m.swing_ticks = 6;
                    if m.kind == MobKind::IronGolem {
                        m.attack_ticks = 10;
                        if !silent {
                            out.world_sounds.push((
                                "mob.irongolem.throw",
                                m.position.as_dvec3(),
                                1.,
                                1.,
                            ));
                        }
                    }
                    prey_attacks.push((prey.0, damage, m.kind == MobKind::IronGolem));
                    brain.attack_timer = 20;
                }
            }
            let melee_reach = if m.kind == MobKind::EnderDragon {
                false
            } else {
                distance < (size.0 + 1.).max(1.6)
            };
            if target && melee_reach
                && m.kind != MobKind::Creeper
                && !(m.kind == MobKind::Skeleton && !wither_skeleton)
                && brain.attack_timer == 0
                && !world.occluded(m.position + Vec3::Y, player.eye(1.0))
            {
                if let Some(base) = species_ai::melee_damage(m.kind, data, &mut self.rng) {
                    m.swing_ticks = 6;
                    if m.kind == MobKind::IronGolem {
                        m.attack_ticks = 10;
                    }
                    let ignites_on_hit = m.kind == MobKind::Zombie && brain.fire_ticks > 0
                        && self.command_entities.mobs.get(&m.id).and_then(|d| d.equipment.get(&99)).is_none_or(Option::is_none);
                    attacks.push((
                        hostile_damage(base, difficulty),
                        ignites_on_hit.then_some(m.id),
                        m.kind,
                        wither_skeleton,
                        Some(m.id),
                        m.position,
                        silent,
                    ));
                    brain.attack_timer = 20;
                }
            }
            let aim=if target {Some((player.eye(1.), None))}else{prey.map(|p|(p.2+Vec3::Y*0.7,Some(p.0)))};
            if let Some((at,entity))=aim.filter(|(at,_)| !world.occluded(m.position+Vec3::Y*(size.1*0.75),*at)){
                if m.kind==MobKind::Guardian {
                    brain.attack_step=brain.attack_step.saturating_add(1);
                    let elder=species_ai::number(data,"Elder")!=0;
                    if brain.attack_step >= if elder{70}else{90} {
                        let damage=if elder{8.}else{6.};
                        if let Some(id)=entity {prey_attacks.push((id,damage,false));}
                        else {
                            attacks.push((
                                hostile_damage(damage, difficulty),
                                None,
                                MobKind::Guardian,
                                false,
                                Some(m.id),
                                m.position,
                                silent,
                            ));
                        }
                        brain.attack_step=0;
                    }
                } else {
                    let target_undead = entity.is_some()
                        && prey.is_some_and(|p| {
                            matches!(
                                p.1,
                                MobKind::Zombie
                                    | MobKind::PigZombie
                                    | MobKind::Skeleton
                                    | MobKind::Wither
                            )
                        });
                    if let Some(shot) = mob_shots::Shot::aim_with_undead(
                        m,
                        brain,
                        at,
                        self.health,
                        &self.effects,
                        target_undead,
                        &mut self.rng,
                    ) {
                        self.mob_shots.push(shot);
                    }
                }
            } else if matches!(m.kind,MobKind::Guardian|MobKind::Blaze) {brain.attack_step=0;}
            if m.kind==MobKind::Endermite && m.age>=2400 && !data.is_some_and(|d|d.persistent){m.health=-100.;}
            if m.kind == MobKind::Creeper {
                let ignited = species_ai::number(data, "ignited") != 0;
                if ignited
                    || (target
                        && distance < 3.0
                        && !world.occluded(m.position + Vec3::Y, player.eye(1.0)))
                {
                    m.fuse = m.fuse.saturating_add(1);
                } else {
                    m.fuse = m.fuse.saturating_sub(1);
                }
                if m.fuse == 1 && m.prev_fuse == 0 && !silent {
                    out.world_sounds
                        .push(("creeper.primed", m.position.as_dvec3(), 1.0, 0.5));
                }
                if m.fuse >= 30 {
                    explosions.push(m.position);
                    m.health = -100.0;
                }
            }
            if m.kind == MobKind::Skeleton
                && !wither_skeleton
                && target
                && distance < 16.0
                && brain.attack_timer == 0
                && !world.occluded(m.position + Vec3::Y * 1.5, player.eye(1.0))
            {
                let position = m.position + Vec3::Y * 1.5;
                arrows.push(Arrow {
                    position,
                    previous: position,
                    velocity: (player.eye(1.0) - position).normalize() * 0.8,
                    age: 0,
                });
                m.swing_ticks = 6;
                if !silent {
                    out.world_sounds.push((
                        "random.bow",
                        m.position.as_dvec3(),
                        1.0,
                        1.0 / (unit(&mut self.rng) * 0.4 + 0.8),
                    ));
                }
                brain.attack_timer = 60;
            }
            if m.kind == MobKind::Chicken && brain.growing_age >= 0 {
                if brain.egg_timer == 0 {
                    brain.egg_timer = 6000 + (random(&mut self.rng) % 6000) as u16;
                }
                brain.egg_timer -= 1;
                if brain.egg_timer == 0 {
                    eggs.push(m.position);
                }
            }
            if m.kind.hostile() && !matches!(m.kind, MobKind::Wither | MobKind::EnderDragon)
                && distance > 128.0 && !data.is_some_and(|d| d.persistent) {
                m.health = -100.0;
            }
            self.commands.measure_subsection(&mut debug_entities,"entities.remaining");
        }
        for id in trade_feedback {
            self.flush_trade_feedback(id, player.position);
        }
        for (first, second, kind, position) in births {
            let eligible = [first, second].iter().all(|id| {
                self.mob_brains
                    .get(id)
                    .is_some_and(|b| b.love > 0 && b.growing_age == 0)
            });
            if !eligible {
                continue;
            }
            for id in [first, second] {
                let brain = self.mob_brains.entry(id).or_default();
                brain.growing_age = 6000;
                brain.love = 0;
                brain.courtship = 0;
                brain.mate = None;
            }
            self.spawn_offspring(kind, position, first, second);
            if self.mob_loot {
                let value = 1 + (random(&mut self.rng) % 7) as i32;
                self.spawn_xp(position.as_dvec3(), value);
            }
        }
        for position in eggs {
            self.spawn_drop(Stack::new(344, 1), position + Vec3::Y * 0.2, Vec3::ZERO);
            out.world_sounds
                .push(("mob.chicken.plop", position.as_dvec3(), 1.0, 1.0));
        }
        for (id,damage,lift) in prey_attacks {
            if let Some(mob) = self.mobs.iter_mut().find(|m| m.id == id && m.health > 0.) {
                let brain = self.mob_brains.entry(id).or_default();
                let data = self.command_entities.mobs.get_mut(&id);
                let silent = data.as_ref().is_some_and(|d| d.silent);
                if !data.as_ref().is_some_and(|d|d.invulnerable) && melee::apply_mob_hit(mob,brain,data,damage,HitSource { has_attacker:true, ..Default::default() }, &mut self.enchantment_random_state) == Some(true) {
                    melee::mob_hurt_feedback(mob,brain,None,false,silent,out);
                    if lift {mob.velocity.y += 0.4;}
                }
            }
        }
        for (
            damage,
            burning_zombie,
            attacker_kind,
            wither_hit,
            attacker_id,
            attacker_pos,
            attacker_silent,
        ) in attacks
        {
            let hit = self.hurt_from(
                damage,
                HitSource {
                    has_attacker: true,
                    ..Default::default()
                },
            );
            if hit && attacker_kind == MobKind::IronGolem {
                let mut velocity=player.precise_velocity(); velocity[1]+=0.4; player.set_velocity_f64(velocity);
                if !attacker_silent {
                    out.world_sounds.push((
                        "mob.irongolem.throw",
                        attacker_pos.as_dvec3(),
                        1.,
                        1.,
                    ));
                }
            }
            if hit && attacker_kind == MobKind::CaveSpider && difficulty >= 2 {
                self.command_player_effect(command_entity_state::EffectEdit::Add(crate::vanilla::item_food::Effect::new(19,if difficulty==2 {140}else{300},0)));
            }
            if hit && wither_hit {
                self.command_player_effect(command_entity_state::EffectEdit::Add(
                    crate::vanilla::item_food::Effect::new(20, 200, 0),
                ));
            }
            if hit {
                if let Some(aid) = attacker_id {
                    self.alert_owned_wolves(aid, player.position);
                }
            }
            if let Some(id) = burning_zombie.filter(|_| hit) {
                let brain = self.mob_brains.get_mut(&id).unwrap();
                let state = brain.entity_random.get_or_insert_with(|| JavaRandom::new(JavaRandom::independent_seed()).state());
                let mut random = JavaRandom::from_state(*state);
                if random.next_float() < f32::from(difficulty) * 0.3 {
                    burn::set_fire(&mut self.fire_ticks, 2 * i32::from(difficulty), self.inventory.armor_enchantment(1));
                }
                *state = random.state();
            }
        }
        self.commands.measure_subsection(&mut debug_entities,"entities.remaining");
        for crystal in &mut self.end_crystals {
            crystal.beam_target = None;
        }
        if !self.end_crystals.is_empty() {
            for m in &self.mobs {
                if m.kind == MobKind::EnderDragon && m.health > 0.0 {
                    if let Some(cid) = self
                        .mob_brains
                        .get(&m.id)
                        .and_then(|b| b.dragon_healing_crystal)
                    {
                        if let Some(crystal) = self.end_crystals.iter_mut().find(|c| c.id == cid) {
                            crystal.beam_target = Some(m.position + Vec3::Y * 2.0);
                        }
                    }
                }
            }
        }
        self.arrows.extend(arrows);
        self.tick_mob_shots(world,player,settings,out);
        let mut destroyed_crystals = Vec::new();
        let mut arrow_hits = 0;
        for arrow in &mut self.arrows {
            arrow.previous = arrow.position;
            arrow.age += 1;
            let next = arrow.position + arrow.velocity;
            let start_d = arrow.previous.as_dvec3();
            let end_d = next.as_dvec3();
            if !self.end_crystals.is_empty() {
                if let Some(cid) = self.end_crystals.iter().find_map(|c| {
                    let b = [
                        c.position.x - 1.3,
                        c.position.y - 0.3,
                        c.position.z - 1.3,
                        c.position.x + 1.3,
                        c.position.y + 2.3,
                        c.position.z + 1.3,
                    ];
                    let inside = start_d.x >= b[0]
                        && start_d.x <= b[3]
                        && start_d.y >= b[1]
                        && start_d.y <= b[4]
                        && start_d.z >= b[2]
                        && start_d.z <= b[5];
                    (inside
                        || crate::vanilla::ray_trace::intercept(b, start_d, end_d).is_some())
                    .then_some(c.id)
                }) {
                    destroyed_crystals.push(cid);
                    arrow.age = 200;
                    continue;
                }
            }
            let mut hit_dragon = false;
            for m in &self.mobs {
                if m.kind == MobKind::EnderDragon && m.health > 0.0 {
                    let parts = melee::dragon_part_boxes(m, self.mob_brains.get(&m.id));
                    if parts.iter().any(|(_, _, bounds)| {
                        let b = [
                            bounds.min.x,
                            bounds.min.y,
                            bounds.min.z,
                            bounds.max.x,
                            bounds.max.y,
                            bounds.max.z,
                        ];
                        let inside = start_d.x >= b[0]
                            && start_d.x <= b[3]
                            && start_d.y >= b[1]
                            && start_d.y <= b[4]
                            && start_d.z >= b[2]
                            && start_d.z <= b[5];
                        inside || crate::vanilla::ray_trace::intercept(b, start_d, end_d).is_some()
                    }) {
                        let brain = self.mob_brains.entry(m.id).or_default();
                        melee::veer_dragon_on_hit(m, brain);
                        arrow.age = 200;
                        hit_dragon = true;
                        break;
                    }
                }
            }
            if hit_dragon {
                continue;
            }
            if world.occluded(arrow.position, next) {
                arrow.age = 200;
            } else {
                arrow.position = next;
            }
            arrow.velocity.y -= 0.025;
            if arrow.position.distance_squared(player.position + Vec3::Y) < 0.6 {
                arrow_hits += 1;
                arrow.age = 200;
            }
        }
        self.arrows.retain(|a| a.age < 200);
        if arrow_hits > 0 {
            self.hurt_from(
                3.,
                HitSource {
                    projectile: true,
                    has_attacker: true,
                    ..Default::default()
                },
            );
        }
        if !self.end_crystals.is_empty() && !self.projectiles.is_empty() {
            let mut kept_projectiles = Vec::with_capacity(self.projectiles.len());
            for projectile in std::mem::take(&mut self.projectiles) {
                if projectile.embedded.is_none()
                    && projectile.dimension == self.dimension
                    && projectile.position.is_finite()
                    && projectile.velocity.is_finite()
                {
                    let start = projectile.position;
                    let end = start + projectile.velocity;
                    let is_arrow = matches!(projectile.payload, projectiles::Payload::Arrow(_));
                    let clipped = crate::vanilla::ray_trace::trace(
                        world,
                        start,
                        end,
                        crate::vanilla::ray_trace::Options {
                            stop_on_liquid: false,
                            ignore_without_box: is_arrow,
                            return_last_miss: false,
                        },
                    )
                    .map_or(end, |hit| hit.point);
                    if let Some((cid, _)) = self
                        .end_crystals
                        .iter()
                        .filter_map(|c| {
                            let b = [
                                c.position.x - 1.3,
                                c.position.y - 0.3,
                                c.position.z - 1.3,
                                c.position.x + 1.3,
                                c.position.y + 2.3,
                                c.position.z + 1.3,
                            ];
                            let inside = start.x >= b[0]
                                && start.x <= b[3]
                                && start.y >= b[1]
                                && start.y <= b[4]
                                && start.z >= b[2]
                                && start.z <= b[5];
                            if inside {
                                Some((c.id, 0.0))
                            } else {
                                crate::vanilla::ray_trace::intercept(b, start, clipped)
                                    .map(|(_, p)| (c.id, p.distance_squared(start)))
                            }
                        })
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                    {
                        destroyed_crystals.push(cid);
                        continue;
                    }
                }
                kept_projectiles.push(projectile);
            }
            self.projectiles = kept_projectiles;
        }
        for cid in destroyed_crystals {
            self.destroy_end_crystal(cid, world, player, settings, out);
        }
        self.tick_projectiles(world, player, out);
        self.tick_fishing_and_eyes(world, player, out);
        self.commands.measure_subsection(&mut debug_entities,"entities.projectiles");
        for m in &self.mobs {
            if m.health <= 0.0 && m.health > -99.0 && m.kind != MobKind::EnderDragon {
                let mut corpse = m.clone();
                corpse.death_ticks = 1;
                self.dying_mobs.push(corpse);
            }
        }
        let dead = self
            .mobs
            .iter()
            .filter(|m| {
                m.health <= 0.0
                    && m.health > -99.0
                    && m.kind != MobKind::EnderDragon
                    && !self.is_baby(m.id)
            })
            .map(|m| (m.id, m.kind, m.position, self.mob_brains.get(&m.id).is_some_and(|b| b.fire_ticks > 0)))
            .collect::<Vec<_>>();
        self.mobs
            .retain(|m| m.health > 0.0 || (m.kind == MobKind::EnderDragon && m.health > -99.0));
        for (id,kind,p,burning) in dead {
            self.kill_drops(id,kind,p,burning);
        }
        let survivors = self.mobs.iter().map(|mob| mob.id).collect::<std::collections::HashSet<_>>();
        self.command_entities.mobs.retain(|id, _| survivors.contains(id));
        self.mob_brains
            .retain(|id, _| survivors.contains(id));
        self.commands.measure_subsection(&mut debug_entities,"entities.drops");
        for (source_id, p) in boss_explosions {
            let terrain=self.world_commands.rules.get("mobGriefing").is_none_or(|v|v.eq_ignore_ascii_case("true"));
            self.explode_excluding(world,player,settings,p.as_dvec3(),7.,terrain,false,Some(source_id),out);
        }
        for p in explosions {
            let terrain=self.world_commands.rules.get("mobGriefing").is_none_or(|value|value.eq_ignore_ascii_case("true"));
            self.explode(world,player,settings,p.as_dvec3(),3.,terrain,false,out);
        }
        self.commands.measure_subsection(&mut debug_entities,"entities.explosions");
        if self.lan.is_none() {
            self.tick_drop_motion(world, out);
        }
        self.commands.measure_subsection(&mut debug_entities,"entities.drops");
    }
}
struct LocalCollisionWorld<'a> {
    world: &'a World,
    boats: Vec<crate::vanilla::movement::Aabb>,
}
impl crate::vanilla::movement::MovementWorld for LocalCollisionWorld<'_> {
    fn get(&self, p: IVec3) -> u16 {
        self.world.get(p)
    }
    fn block_boxes(&self, p: IVec3, state: u16, out: &mut Vec<crate::vanilla::movement::Aabb>) {
        <World as crate::vanilla::movement::MovementWorld>::block_boxes(self.world, p, state, out);
    }
    fn loaded_column(&self, x: i32, z: i32) -> bool {
        <World as crate::vanilla::movement::MovementWorld>::loaded_column(self.world, x, z)
    }
    fn extra_colliders(
        &self,
        query: crate::vanilla::movement::Aabb,
        out: &mut Vec<crate::vanilla::movement::Aabb>,
    ) {
        for &boat in &self.boats {
            if boat.intersects(query) {
                out.push(boat);
            }
        }
    }
}
pub fn container_parts(world: &World, pos: IVec3) -> Option<Vec<IVec3>> {
    let id = world.get(pos) >> 4;
    if matches!(id, 23 | 61 | 62 | 117 | 138 | 154 | 158) {
        return Some(vec![pos]);
    }
    if !matches!(id, 54 | 146) {
        return None;
    }
    let mut parts = vec![pos];
    for step in [IVec3::NEG_Z, IVec3::Z, IVec3::NEG_X, IVec3::X] {
        if world.get(pos + step) >> 4 == id {
            parts.push(pos + step);
            break;
        }
    }
    for part in &parts {
        if crate::vanilla::material::normal_cube(world.get(*part + IVec3::Y)) {
            return None;
        }
    }
    parts.sort_by_key(|p| (p.z, p.x));
    Some(parts)
}
#[cfg(test)]
fn can_place_chest(world: &World, pos: IVec3, id: u16) -> bool {
    crate::vanilla::hand_placement::can_replace(world, pos, id, 1)
}
pub fn container_key(pos: IVec3) -> String {
    format!("{},{},{}", pos.x, pos.y, pos.z)
}
pub fn experience_level(mut xp: u32) -> (u32, f32) {
    let mut level = 0;
    loop {
        let required = if level >= 30 {
            112 + (level - 30) * 9
        } else if level >= 15 {
            37 + (level - 15) * 5
        } else {
            7 + level * 2
        };
        if xp < required {
            return (level, xp as f32 / required as f32);
        }
        xp -= required;
        level += 1;
        if level >= 10000 {
            return (level, 0.0);
        }
    }
}
pub fn surface(world: &World, x: i32, z: i32) -> Option<i32> {
    if !world.contains(Vec3::new(x as f32, 0.0, z as f32)) {
        return None;
    }
    for y in (0..254).rev() {
        let p = IVec3::new(x, y, z);
        if assets::solid(world.get(p))
            && world.get(p + IVec3::Y) == 0
            && world.get(p + IVec3::Y * 2) == 0
        {
            return Some(y + 1);
        }
    }
    None
}
fn hostile_damage(base: f32, difficulty: u8) -> f32 {
    match difficulty {
        0 => 0.,
        1 => base / 2. + 1.,
        3 => base * 3. / 2.,
        _ => base,
    }
}

pub fn break_sound(state: u16) -> &'static str {
    match state >> 4 {
        2 | 3 | 18 | 31 | 161 => "dig.grass",
        5 | 17 | 47 | 53 | 54 | 58 | 162 => "dig.wood",
        12 => "dig.sand",
        20 | 79 | 95 | 102 => "dig.glass",
        35 => "dig.cloth",
        _ => "dig.stone",
    }
}

pub(crate) fn emit_eating_crumbs_into(
    particles: &mut Vec<Particle>,
    rng: &mut JavaRandom,
    player: &Player,
    id: u16,
    meta: u16,
    count: usize,
) {
    let [yaw_deg, pitch_deg] = player.java_rotation();
    let pitch_rad = -pitch_deg * std::f32::consts::PI / 180.0;
    let yaw_rad = -yaw_deg * std::f32::consts::PI / 180.0;
    let rotate = |mut v: glam::DVec3| -> glam::DVec3 {
        let (sp, cp) = (f64::from(pitch_rad.sin()), f64::from(pitch_rad.cos()));
        v = glam::DVec3::new(v.x, v.y * cp + v.z * sp, v.z * cp - v.y * sp);
        let (sy, cy) = (f64::from(yaw_rad.sin()), f64::from(yaw_rad.cos()));
        glam::DVec3::new(v.x * cy + v.z * sy, v.y, v.z * cy - v.x * sy)
    };
    let eye = player.precise_eye(1.0);
    for _ in 0..count {
        if particles.len() >= 4000 {
            break;
        }
        let vel = rotate(glam::DVec3::new(
            (f64::from(rng.next_float()) - 0.5) * 0.1,
            f64::from(rng.next_float()) * 0.1 + 0.1,
            0.0,
        )) + glam::DVec3::Y * 0.05;
        let offset = rotate(glam::DVec3::new(
            (f64::from(rng.next_float()) - 0.5) * 0.3,
            -f64::from(rng.next_float()) * 0.6 - 0.3,
            0.6,
        ));
        particles.push(Particle::item(id, meta, false, eye + offset, vel, rng));
    }
}

pub(crate) fn effects_liquid_color(effects: &Effects) -> Option<(u32, bool)> {
    let mut r = 0.0f32;
    let mut g = 0.0f32;
    let mut b = 0.0f32;
    let mut weight_sum = 0.0f32;
    let mut all_ambient = true;
    for e in effects.ordered() {
        if e.duration <= 0 || !e.show_particles {
            continue;
        }
        let color: u32 = match e.id {
            1 => 0x7CAFC6,
            2 => 0x5A6C81,
            3 => 0xD9C043,
            4 => 0x4A4217,
            5 => 0x932423,
            6 => 0xF82423,
            7 => 0x430A09,
            8 => 0x22FF4C,
            9 => 0x551D4A,
            10 => 0xCD5CAB,
            11 => 0x99453A,
            12 => 0xE49A3A,
            13 => 0x2E5299,
            14 => 0x7F8392,
            15 => 0x1F1F23,
            16 => 0x1F1FA1,
            17 => 0x587653,
            18 => 0x484D48,
            19 => 0x4E9331,
            20 => 0x352A27,
            21 => 0xF87D23,
            22 => 0x2552A5,
            23 => 0xF82423,
            _ => continue,
        };
        if !e.ambient {
            all_ambient = false;
        }
        let w = (e.amplifier.max(0) + 1) as f32;
        r += w * (((color >> 16) & 255) as f32 / 255.0);
        g += w * (((color >> 8) & 255) as f32 / 255.0);
        b += w * ((color & 255) as f32 / 255.0);
        weight_sum += w;
    }
    if weight_sum <= 0.0 {
        return None;
    }
    let ri = ((r / weight_sum * 255.0).clamp(0.0, 255.0)) as u32;
    let gi = ((g / weight_sum * 255.0).clamp(0.0, 255.0)) as u32;
    let bi = ((b / weight_sum * 255.0).clamp(0.0, 255.0)) as u32;
    let rgb = (ri << 16) | (gi << 8) | bi;
    (rgb != 0).then_some((rgb, all_ambient))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_potion_swirl(
    particles: &mut Vec<Particle>,
    particle_setting: u8,
    rng: &mut JavaRandom,
    feet: glam::DVec3,
    width: f32,
    height: f32,
    color: u32,
    ambient: bool,
    invisible: bool,
) {
    if color == 0 || particles.len() >= 4000 || particle_setting >= 2 {
        return;
    }
    let mut spawn = if invisible {
        rng.next_int(15) == 0
    } else {
        rng.next_int(2) != 0
    };
    if ambient {
        spawn &= rng.next_int(5) == 0;
    }
    if !spawn || (particle_setting == 1 && rng.next_int(3) == 0) {
        return;
    }
    let rgb = glam::DVec3::new(
        f64::from(((color >> 16) & 255) as f32 / 255.0),
        f64::from(((color >> 8) & 255) as f32 / 255.0),
        f64::from((color & 255) as f32 / 255.0),
    );
    let pos = feet
        + glam::DVec3::new(
            (rng.next_double() - 0.5) * f64::from(width),
            rng.next_double() * f64::from(height),
            (rng.next_double() - 0.5) * f64::from(width),
        );
    let id = if ambient { 16 } else { 15 };
    if let Some(p) = Particle::network_sprite(id, pos, rgb, rng) {
        particles.push(p);
    }
}

pub(crate) fn emit_running_dust(
    particles: &mut Vec<Particle>,
    world: &World,
    rng: &mut JavaRandom,
    feet: glam::DVec3,
    width: f32,
    vx: f64,
    vz: f64,
) {
    if particles.len() >= 4000 {
        return;
    }
    let below = (feet - glam::DVec3::Y * f64::from(0.2f32))
        .floor()
        .as_ivec3();
    let state = world.get(below);
    if matches!(state >> 4, 0 | 8..=11 | 166) {
        return;
    }
    let pos = glam::DVec3::new(
        feet.x + (f64::from(rng.next_float()) - 0.5) * f64::from(width),
        feet.y + 0.1,
        feet.z + (f64::from(rng.next_float()) - 0.5) * f64::from(width),
    );
    let vel = glam::DVec3::new(-vx * 4.0, 1.5, -vz * 4.0);
    particles.push(Particle::digging(pos, vel, state, below, false, rng));
}

pub(crate) fn emit_water_entry_splash(
    particles: &mut Vec<Particle>,
    rng: &mut JavaRandom,
    feet: glam::DVec3,
    width: f32,
    vel: [f64; 3],
) -> (&'static str, glam::DVec3, f32, f32) {
    let speed = ((vel[0] * vel[0] * 0.2 + vel[1] * vel[1] + vel[2] * vel[2] * 0.2).sqrt() as f32
        * 0.2)
        .min(1.0);
    let pitch = 1.0 + (rng.next_float() - rng.next_float()) * 0.4;
    let y = feet.y.floor() + 1.0;
    let count = (1.0 + width * 20.0).max(1.0) as usize;
    for _ in 0..count {
        if particles.len() >= 4000 {
            break;
        }
        let dx = f64::from((rng.next_float() * 2.0 - 1.0) * width);
        let dz = f64::from((rng.next_float() * 2.0 - 1.0) * width);
        let bvel = glam::DVec3::new(
            vel[0],
            vel[1] - f64::from(rng.next_float() * 0.2),
            vel[2],
        );
        if let Some(p) = Particle::network_sprite(
            4,
            glam::DVec3::new(feet.x + dx, y, feet.z + dz),
            bvel,
            rng,
        ) {
            particles.push(p);
        }
    }
    for _ in 0..count {
        if particles.len() >= 4000 {
            break;
        }
        let dx = f64::from((rng.next_float() * 2.0 - 1.0) * width);
        let dz = f64::from((rng.next_float() * 2.0 - 1.0) * width);
        let svel = glam::DVec3::new(vel[0], vel[1], vel[2]);
        if let Some(p) = Particle::network_sprite(
            5,
            glam::DVec3::new(feet.x + dx, y, feet.z + dz),
            svel,
            rng,
        ) {
            particles.push(p);
        }
    }
    ("game.neutral.swim.splash", feet, speed, pitch)
}

pub(crate) fn emit_slime_landing_particles(
    particles: &mut Vec<Particle>,
    rng: &mut JavaRandom,
    feet: glam::DVec3,
    slime_size: i32,
    magma: bool,
) {
    let count = (slime_size.max(1) * 8) as usize;
    let sprite = if magma { 26 } else { 33 };
    for _ in 0..count {
        if particles.len() >= 4000 {
            break;
        }
        let angle = rng.next_float() * std::f32::consts::TAU;
        let dist = rng.next_float() * 0.5 + 0.5;
        let dx = f64::from(angle.sin() * slime_size as f32 * 0.5 * dist);
        let dz = f64::from(angle.cos() * slime_size as f32 * 0.5 * dist);
        if let Some(p) =
            Particle::network_sprite(sprite, feet + glam::DVec3::new(dx, 0.0, dz), glam::DVec3::ZERO, rng)
        {
            particles.push(p);
        }
    }
}

pub(crate) fn emit_mob_ambient_particles(
    particles: &mut Vec<Particle>,
    particle_setting: u8,
    rng: &mut JavaRandom,
    vanilla_id: u8,
    feet: glam::DVec3,
    width: f32,
    height: f32,
) {
    if particle_setting >= 2 || particles.len() >= 4000 {
        return;
    }
    match vanilla_id {
        61 => {
            for _ in 0..2 {
                if particles.len() >= 4000
                    || (particle_setting == 1 && rng.next_int(3) == 0)
                {
                    continue;
                }
                let pos = feet
                    + glam::DVec3::new(
                        (rng.next_double() - 0.5) * f64::from(width),
                        rng.next_double() * f64::from(height),
                        (rng.next_double() - 0.5) * f64::from(width),
                    );
                if let Some(p) = Particle::network_sprite(12, pos, glam::DVec3::ZERO, rng) {
                    particles.push(p);
                }
            }
        }
        58 | 67 => {
            for _ in 0..2 {
                if particles.len() >= 4000
                    || (particle_setting == 1 && rng.next_int(3) == 0)
                {
                    continue;
                }
                let pos = feet
                    + glam::DVec3::new(
                        (rng.next_double() - 0.5) * f64::from(width),
                        rng.next_double() * f64::from(height)
                            - if vanilla_id == 58 { 0.25 } else { 0.0 },
                        (rng.next_double() - 0.5) * f64::from(width),
                    );
                let vel = glam::DVec3::new(
                    (rng.next_double() - 0.5) * 2.0,
                    -rng.next_double(),
                    (rng.next_double() - 0.5) * 2.0,
                );
                if let Some(p) = Particle::special(24, pos, vel, rng) {
                    particles.push(p);
                }
            }
        }
        64 => {
            for _ in 0..3 {
                if particles.len() >= 4000
                    || (particle_setting == 1 && rng.next_int(3) == 0)
                {
                    continue;
                }
                let pos = feet
                    + glam::DVec3::new(
                        (rng.next_double() - 0.5) * f64::from(width),
                        f64::from(height) * 0.65 + (rng.next_double() - 0.5) * 0.6,
                        (rng.next_double() - 0.5) * f64::from(width),
                    );
                if let Some(p) = Particle::network_sprite(11, pos, glam::DVec3::ZERO, rng) {
                    particles.push(p);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn flat_world() -> World {
        let mut chunk = crate::world::Chunk::empty();
        for x in 0..16 {
            for z in 0..16 {
                chunk.set(x, 0, z, 2 << 4);
            }
        }
        World {
            chunks: crate::world::ChunkMap::from([((0, 0), std::sync::Arc::new(chunk))]),
            spawn: Vec3::new(8.5, 1.0, 8.5),
            label: "AI fixture".into(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        }
    }
    #[test]
    #[ignore = "requires locally imported Minecraft resources"]
    fn right_click_note_tunes_persists_and_only_retriggers_on_rising_power() {
        use crate::vanilla::special_click::note_sound;
        use clap::Parser;
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let save_id = format!("note-tuning-test-{}-{unique}", std::process::id());
        let options = crate::Options::parse_from([
            "ferrum189",
            "--generator",
            "1",
            "--render-distance",
            "2",
            "--save-id",
            &save_id,
        ]);
        let mut world = World::load(&options).unwrap().world;
        let p = IVec3::new(0, 100, 2);
        let mut player = Player::new(Vec3::new(0.5, 99., 0.5));
        player.set_rotation_degrees(0., 0.);
        assert!(world.set(p, 25 << 4));
        assert!(world.set(p - IVec3::Y, 5 << 4));
        let r = Resources::load().unwrap();
        let settings = Settings::default();
        let mut sim = Simulation::default();
        let out = sim.tick(&mut world, &mut player, &r, &settings, false, true);
        let (n1_clip, n1_vol, n1_pitch) = note_sound(4, 1);
        assert!(
            out.world_sounds
                .iter()
                .any(|&(c, _, v, pt)| (c, v, pt) == (n1_clip, n1_vol, n1_pitch)),
            "target {:?}, sounds {:?}",
            sim.pick(&world, &player),
            out.world_sounds
        );
        sim.tick(&mut world, &mut player, &r, &settings, false, false);
        world.set(p + IVec3::Y, 20 << 4);
        let muted = sim.tick(&mut world, &mut player, &r, &settings, false, true);
        assert!(!muted
            .world_sounds
            .iter()
            .any(|sound| sound.0.starts_with("note.")));
        world
            .save(
                &options,
                crate::player_save::PlayerSave::capture(&player, sim.mode),
                &sim,
            )
            .unwrap();
        let loaded = World::load(&options).unwrap();
        let mut world = loaded.world;
        let mut sim = loaded.simulation.unwrap();
        world.set(p + IVec3::Y, 0);
        let source = p + IVec3::X;
        let (n2_clip, n2_vol, n2_pitch) = note_sound(4, 2);
        for (state, plays) in [(152 << 4, 1), (152 << 4, 0), (0, 0), (152 << 4, 1)] {
            world.set(source, state);
            sim.block_ticks.notify(&world, source);
            let out = sim.tick(&mut world, &mut player, &r, &settings, false, false);
            assert_eq!(
                out.world_sounds
                    .iter()
                    .filter(|&&(c, _, v, pt)| (c, v, pt) == (n2_clip, n2_vol, n2_pitch))
                    .count(),
                plays
            );
        }
        let snapshot = sim.note_snapshot(&world, p).unwrap();
        assert_eq!(
            crate::vanilla::item_nbt::compound(&snapshot)
                .unwrap()
                .get("note"),
            Some(&crate::vanilla::drops::DropNbt::Byte(2))
        );
        sim.destroy_picked_block(&mut world, p, &settings, &mut Outcome::default());
        world.set(p, 25 << 4);
        assert!(sim.note_snapshot(&world, p).is_none());
        std::fs::remove_file(&world.save_path).unwrap();
    }
    #[test]
    #[ignore = "requires locally imported Minecraft resources"]
    fn original_accepted_use_ticks_match_normal_gameplay_entry_point() {
        use serde_json::{json, Value};
        let resources = Resources::load().unwrap();
        let cases: Value =
            serde_json::from_str(include_str!("../tests/fixtures/vanilla189-use-order.json"))
                .unwrap();
        for (case, row) in cases["cases"].as_array().unwrap().iter().enumerate() {
            let pattern = row[2].as_u64().unwrap();
            let mut world = flat_world();
            let mut player = Player::new(world.spawn);
            let mut sim = Simulation {
                food: 6,
                saturation: 0.,
                right_click_delay: if matches!(pattern, 1 | 2) { 4 } else { 0 },
                right_was_down: matches!(pattern, 1 | 2),
                ..Default::default()
            };
            let mut item = Stack::new(row[0].as_u64().unwrap() as u16, 2);
            item.meta = row[1].as_u64().unwrap() as u16;
            sim.inventory.slots[0] = Some(item);
            for (step, expected) in row[3].as_array().unwrap().iter().enumerate() {
                if step == 11 && pattern == 4 {
                    sim.inventory.slots[0] = sim.inventory.held().map(|held| held.copied());
                }
                if step == 11 && pattern == 5 {
                    sim.inventory.slots[1] = sim.inventory.slots[0].take();
                    sim.inventory.selected = 1;
                }
                let right = expected[0].as_bool().unwrap();
                sim.tick(
                    &mut world,
                    &mut player,
                    &resources,
                    &Settings::default(),
                    false,
                    right,
                );
                let held = sim.inventory.held().map(|s| json!([s.id, s.meta, s.count]));
                assert_eq!(
                    json!([
                        right,
                        sim.right_click_delay,
                        sim.item_use.active(),
                        sim.item_use.remaining(),
                        held,
                        sim.food
                    ]),
                    *expected,
                    "original use case {case}, tick {}",
                    step + 1
                );
            }
        }
    }
    fn cow(id: u64, x: f32) -> Mob {
        let position = Vec3::new(x, 1.0, 8.5);
        Mob {
            id,
            kind: MobKind::Cow,
            position,
            previous: position,
            velocity: Vec3::ZERO,
            yaw: 0.0,
            health: 10.0,
            age: 0,
            turn_ticks: 100,
            hurt: 0,
            fuse: 0,
            swing_ticks: 0,
            death_ticks: 0,
            eat_ticks: 0,
            attack_ticks: 0,
            rose_ticks: 0,
            prev_fuse: 0,
        }
    }
    #[test]
    fn exhaustion_boundary_and_regeneration_timer_follow_foodstats() {
        let mut sim = Simulation {
            exhaustion: 4.0,
            ..Default::default()
        };
        sim.tick_food(2);
        assert_eq!(sim.saturation, 5.0);
        sim.exhaustion = 12.1;
        sim.tick_food(2);
        assert!((sim.exhaustion - 8.1).abs() < 0.001);
        assert_eq!(sim.saturation, 4.0);
        sim.exhaustion = 0.0;
        sim.health = 19.5;
        for _ in 0..79 {
            sim.tick_food(2);
        }
        assert_eq!(sim.health, 19.5);
        sim.tick_food(2);
        assert_eq!(sim.health, 20.0);
        assert_eq!(sim.exhaustion, 3.0);
        sim.health = 19.0;
        sim.tick_food(2);
        sim.food = 17;
        sim.tick_food(2);
        assert_eq!(sim.food_timer, 0);
        sim.add_exhaustion(100.0);
        assert_eq!(sim.exhaustion, 40.0);
    }
    #[test]
    fn starvation_difficulty_floors_and_armor_bypass() {
        for (difficulty, starting, expected) in
            [(1, 10.0, 10.0), (2, 1.0, 1.0), (3, 1.0, 0.0), (2, 2.0, 1.0)]
        {
            let mut sim = Simulation {
                food: 0,
                saturation: 0.0,
                health: starting,
                ..Default::default()
            };
            sim.inventory.slots[36] = Some(Stack::new(313, 1));
            sim.inventory.slots[37] = Some(Stack::new(312, 1));
            sim.inventory.slots[38] = Some(Stack::new(311, 1));
            sim.inventory.slots[39] = Some(Stack::new(310, 1));
            for _ in 0..80 {
                sim.tick_food(difficulty);
            }
            assert_eq!(sim.health, expected);
        }
    }
    #[test]
    fn golden_apple_recipes_preserve_their_variant_through_eating() {
        for (rim, meta) in [(266, 0), (41, 1)] {
            let mut grid = vec![Some(Stack::new(rim, 1)); 9];
            grid[4] = Some(Stack::new(260, 1));
            let result = crate::vanilla::crafting::recipe(&grid, 3).unwrap().output;
            assert_eq!((result.id, result.meta, result.count), (322, meta, 1));
            let mut sim = Simulation::default();
            sim.start_eating(result.clone());
            sim.inventory.slots[0] = Some(result);
            let mut out = Outcome::default();
            for _ in 0..32 { sim.tick_item_use(true, &mut out); }
            assert!(sim.inventory.held().is_none());
            let effect = |id| sim.effects.get(id).map(|e| (e.amplifier, e.duration));
            assert_eq!(effect(22), Some((0, 2400)));
            assert_eq!(effect(10), Some(if meta == 0 { (1, 100) } else { (4, 600) }));
            assert_eq!(effect(11), (meta == 1).then_some((0, 6000)));
            assert_eq!(effect(12), (meta == 1).then_some((0, 6000)));
        }
    }
    #[test]
    fn critical_hit_requires_falling_unmounted_dry_and_sighted() {
        let world = flat_world();
        for case in 0..7 {
            let mut sim = Simulation::default();
            sim.mobs.push(cow(2, 7.5));
            let mut player = Player::new(world.spawn);
            sim.fall_distance = if case == 1 { 0. } else { 1. };
            player.grounded = case == 2;
            player.on_ladder = case == 3;
            player.in_water = case == 4;
            player.third_person.riding = case == 5;
            if case == 6 {
                sim.effects.add_client(crate::vanilla::item_food::Effect::new(15, 100, 0)).unwrap();
            }
            let mut out = Outcome::default();
            sim.attack_picked_mob(0, &mut player, &world, 0, &mut out);
            assert_eq!(sim.mobs[0].health, if case == 0 { 8.5 } else { 9. }, "case {case}");
            let count = sim.particles.len();
            assert_eq!(!sim.critical_emitters.is_empty(), case == 0, "case {case}");
            sim.attack_picked_mob(0, &mut player, &world, 0, &mut out);
            assert_eq!(sim.particles.len(), count, "rejected second hit emitted particles");
        }
    }
    #[test]
    fn animals_follow_wheat_and_flee_after_damage() {
        let mut world = flat_world();
        let mut player = Player::new(Vec3::new(10.5, 1.0, 8.5));
        let mut sim = Simulation::default();
        sim.mobs.push(cow(2, 4.5));
        sim.inventory.slots[0] = Some(Stack::new(296, 1));
        let mut out = Outcome::default();
        for _ in 0..30 {
            sim.tick_entities(&mut world, &mut player, &Settings::default(), &mut out);
        }
        let approached = sim.mobs[0].position.x;
        assert!(approached > 5.5, "cow should approach wheat: {approached}");
        sim.mob_brains.entry(2).or_default().entity_random = Some(JavaRandom::new(189).state());
        sim.attack_picked_mob(0, &mut player, &world, 0, &mut out);
        for _ in 0..30 {
            sim.tick_entities(&mut world, &mut player, &Settings::default(), &mut out);
        }
        let brain = &sim.mob_brains[&2];
        assert!(brain.panic > 0 && !brain.tempted);
        assert!(
            (sim.mobs[0].position - Vec3::new(approached, 1., 8.5)).length() > 0.5,
            "the panicking cow must follow its sampled destination"
        );
    }
    #[test]
    fn namespaced_imported_mob_ids_tick_without_overflow() {
        let mut world = flat_world();
        let mut player = Player::new(world.spawn);
        let mut sim = Simulation::default();
        sim.mobs.push(cow(u64::MAX - 5, 6.5));
        sim.tick_entities(
            &mut world,
            &mut player,
            &Settings::default(),
            &mut Outcome::default(),
        );
        assert_eq!(sim.mobs[0].age, 1);
    }
    #[test]
    fn survivor_cleanup_retains_unloaded_mob_state_and_removes_only_absent_ids() {
        let mut world = flat_world();
        let mut player = Player::new(world.spawn);
        let mut sim = Simulation { mob_spawning: false, mob_loot: false, ..Default::default() };
        let mut dead = cow(2, 6.5);
        dead.health = -100.;
        sim.mobs = vec![cow(1, 6.5), dead, cow(3, 10000.5)];
        for id in [1, 2, 3, 99] {
            sim.command_entities.mobs.entry(id).or_default().no_ai = true;
            sim.mob_brains.entry(id).or_default().panic = 42;
        }
        sim.tick_entities(&mut world, &mut player, &Settings::default(), &mut Outcome::default());
        assert_eq!(sim.mobs.iter().map(|m| m.id).collect::<Vec<_>>(), vec![1, 3]);
        assert!(sim.command_entities.mobs.contains_key(&1));
        assert!(sim.command_entities.mobs.contains_key(&3));
        assert!(sim.mob_brains.contains_key(&1));
        assert!(sim.mob_brains.contains_key(&3));
        for id in [2, 99] {
            assert!(!sim.command_entities.mobs.contains_key(&id));
            assert!(!sim.mob_brains.contains_key(&id));
        }
    }
    #[test]
    fn grounded_merchant_stops_for_a_customer_and_resumes_after_close() {
        let mut world = flat_world();
        let mut player = Player::new(world.spawn);
        let mut sim = Simulation::default();
        let mut merchant = cow(2, 6.5);
        merchant.kind = MobKind::Villager;
        merchant.velocity = Vec3::new(0., -0.0784, 0.);
        sim.mobs.push(merchant);
        sim.mob_brains.entry(2).or_default().wander_target = Some(Vec3::new(10.5, 1.0, 8.5));
        sim.mob_brains.get_mut(&2).unwrap().grounded = true;
        assert!(sim.begin_trading(2, "Player").unwrap());
        for _ in 0..20 {
            sim.tick_entities(
                &mut world,
                &mut player,
                &Settings::default(),
                &mut Outcome::default(),
            );
        }
        assert_eq!(sim.mobs[0].position, Vec3::new(6.5, 1.0, 8.5));
        assert!(sim.mob_brains[&2].navigator.no_path());
        assert_eq!(sim.trades[&2].customer.as_deref(), Some("Player"));
        let restored: Simulation =
            serde_json::from_slice(&serde_json::to_vec(&sim).unwrap()).unwrap();
        assert_eq!(restored.trading_with, None);
        sim.return_cursor(&player);
        for _ in 0..20 {
            sim.tick_entities(
                &mut world,
                &mut player,
                &Settings::default(),
                &mut Outcome::default(),
            );
        }
        assert!(
            sim.mobs[0].position.x > 7.0,
            "merchant resumes its walking goal after closing trade"
        );
    }
    #[test]
    fn breeding_spawns_one_baby_and_persists_parent_cooldowns() {
        let mut world = flat_world();
        let mut player = Player::new(Vec3::new(12.5, 1.0, 12.5));
        let mut sim = Simulation {
            next_id: 3,
            ..Default::default()
        };
        sim.mobs = vec![cow(2, 5.5), cow(3, 7.5)];
        sim.mob_brains.entry(2).or_default().feed();
        sim.mob_brains.entry(3).or_default().feed();
        for _ in 0..60 {
            sim.tick_entities(
                &mut world,
                &mut player,
                &Settings::default(),
                &mut Outcome::default(),
            );
        }
        assert_eq!(sim.mobs.len(), 3);
        assert_eq!(sim.mob_brains[&2].growing_age, 6000);
        assert_eq!(sim.mob_brains[&3].love, 0);
        assert!(sim.is_baby(4));
        assert_eq!(sim.mob_brains[&4].growing_age, -24000);
        let restored: Simulation =
            serde_json::from_slice(&serde_json::to_vec(&sim).unwrap()).unwrap();
        assert_eq!(restored.mob_brains[&2].growing_age, 6000);
        assert!(restored.is_baby(4));
    }
    #[test]
    fn navigation_avoids_walls_and_unsafe_drops() {
        let mut world = flat_world();
        for z in 3..=12 {
            for y in 1..=3 {
                world.set(IVec3::new(7, y, z), 16);
            }
        }
        let path = ai::ground_route(
            &world,
            &cow(2, 4.5),
            Vec3::new(10.5, 1.0, 8.5),
            ai::Navigation {
                size: (0.9, 1.3),
                attack_target: false,
                entity_destination: true,
                difficulty: 2,
                avoid_sun: false,
            },
        );
        assert_eq!(path.last(), Some(&Vec3::new(10.5, 1.0, 8.5)));
        assert!(path.iter().any(|p| p.z < 3.0 || p.z >= 13.0));
        assert!(path.iter().all(|p| p.y == 1.0));
    }
    #[test]
    fn farming_tills_plants_grows_and_harvests_vanilla_items() {
        let mut world = flat_world();
        let mut sim = Simulation::default();
        let soil = IVec3::new(8, 0, 8);
        let crop = soil + IVec3::Y;
        let mut out = Outcome::default();
        sim.inventory.slots[0] = Some(Stack::new(290, 1));
        assert!(sim.use_item_on_block(
            &mut world,
            sim.inventory.held().unwrap(),
            (soil, crop),
            &mut out
        ));
        assert_eq!(world.get(soil), 60 << 4);
        assert_eq!(sim.inventory.held().unwrap().damage, 1);
        sim.inventory.slots[0] = Some(Stack::new(295, 2));
        assert!(sim.use_item_on_block(
            &mut world,
            sim.inventory.held().unwrap(),
            (soil, crop),
            &mut out
        ));
        assert_eq!(world.get(crop), 59 << 4);
        assert_eq!(sim.inventory.held().unwrap().count, 1);
        world.set(soil + IVec3::X * 4, 9 << 4);
        sim.random_block_tick(&mut world, soil, &mut out);
        assert_eq!(world.get(soil) & 15, 7);
        sim.clock = 6000;
        for _ in 0..1024 {
            sim.random_block_tick(&mut world, crop, &mut out);
        }
        assert_eq!(world.get(crop), (59 << 4) | 7);
        let loot = block_drops(world.get(crop), None, &mut sim.rng);
        assert!(loot.iter().any(|s| s.id == 296 && s.count == 1));
        let premature = block_drops(59 << 4, None, &mut sim.rng);
        assert_eq!(premature[0].id, 295);
        assert!(!premature.iter().any(|s| s.id == 296));
    }
    #[test]
    fn bucket_placement_uses_original_material_rules_and_nether_vaporization() {
        let target = IVec3::new(8, 1, 8);
        for (block, allowed) in [(0,true), (1,false), (50,true), (63,false), (66,true), (70,false), (83,true)] {
            let mut world = flat_world();
            world.set(target, block << 4);
            let mut sim = Simulation::default();
            let bucket = Stack::new(326, 1);
            sim.inventory.slots[0] = Some(bucket.clone());
            let mut out = Outcome::default();
            assert_eq!(sim.use_item_on_block(&mut world, bucket, (target - IVec3::Y, target), &mut out), allowed);
            assert_eq!(world.get(target), if allowed { 8 << 4 } else { block << 4 });
            assert_eq!(sim.inventory.held().unwrap().id, if allowed { 325 } else { 326 });
        }
        let mut world = flat_world();
        let mut sim = Simulation::default();
        sim.dimension = -1;
        let bucket = Stack::new(326, 1);
        sim.inventory.slots[0] = Some(bucket.clone());
        let mut out = Outcome::default();
        assert!(sim.use_item_on_block(&mut world, bucket, (target - IVec3::Y, target), &mut out));
        assert_eq!(world.get(target), 0);
        assert_eq!(sim.inventory.held().unwrap().id, 325);
        assert_eq!(sim.particles.len(), 8);
        assert_eq!(out.world_sounds[0].0, "random.fizz");
    }
    #[test]
    fn buckets_collect_source_blocks_and_return_empty_bucket() {
        let mut world = flat_world();
        let mut sim = Simulation::default();
        let source = IVec3::new(6, 1, 6);
        let mut out = Outcome::default();
        world.set(source, 9 << 4);
        sim.inventory.slots[0] = Some(Stack::new(325, 1));
        assert!(sim.use_item_on_block(
            &mut world,
            sim.inventory.held().unwrap(),
            (source, source + IVec3::Y),
            &mut out
        ));
        assert_eq!(world.get(source), 0);
        assert_eq!(sim.inventory.held().unwrap().id, 326);
        let floor = IVec3::new(7, 0, 7);
        assert!(sim.use_item_on_block(
            &mut world,
            sim.inventory.held().unwrap(),
            (floor, floor + IVec3::Y),
            &mut out
        ));
        assert_eq!(sim.inventory.held().unwrap().id, 325);
        assert_eq!(world.get(floor + IVec3::Y), 8 << 4);
    }
    #[test]
    #[ignore = "requires locally imported Minecraft resources"]
    fn generated_chest_loot_imports_and_breaking_never_reimports_contents() {
        let mut world = flat_world();
        let chest = IVec3::new(8, 1, 8);
        world.set(chest, 54 << 4 | 2);
        let mut container = crate::container::Container::new(crate::container::Kind::Chest);
        container.slots[0] = Some(Stack::new(388, 3));
        std::sync::Arc::make_mut(world.chunks.get_mut(&(0, 0)).unwrap())
            .containers
            .push((chest, container));
        let mut sim = Simulation::new(Mode::Creative, 189);
        sim.import_containers(&world);
        assert_eq!(
            sim.containers[&container_key(chest)].slots[0],
            Some(Stack::new(388, 3))
        );
        let mut player = Player::new(Vec3::new(8.5, 1.0, 11.5));
        player.yaw = 0.0;
        player.pitch = -0.35;
        let resources = Resources::load().unwrap();
        let outcome = sim.tick(
            &mut world,
            &mut player,
            &resources,
            &Settings::default(),
            true,
            false,
        );
        assert!(outcome.changed.contains(&chest));
        assert!(!sim.containers.contains_key(&container_key(chest)));
        assert_eq!(
            sim.drops
                .iter()
                .filter(|d| d.stack.id == 388)
                .map(|d| d.stack.count as u32)
                .sum::<u32>(),
            3
        );
        // Save/reload, restoring a chest block at the old position, and re-streaming
        // the chunk must not restore its original structure loot.
        let mut restored: Simulation =
            serde_json::from_slice(&serde_json::to_vec(&sim).unwrap()).unwrap();
        world.set(chest, 54 << 4 | 2);
        restored.import_containers(&world);
        assert!(!restored.containers.contains_key(&container_key(chest)));
        assert_eq!(
            restored
                .drops
                .iter()
                .filter(|d| d.stack.id == 388)
                .map(|d| d.stack.count as u32)
                .sum::<u32>(),
            3
        );
    }
    #[test]
    fn double_chest_orders_halves_consistently_and_commits_without_duplication() {
        use crate::inventory::{ClickAction, SlotRef};
        let mut world = flat_world();
        let west = IVec3::new(6, 1, 8);
        let east = west + IVec3::X;
        world.set(west, 54 << 4 | 2);
        world.set(east, 54 << 4 | 2);
        let mut sim = Simulation::default();
        let mut a = crate::container::Container::new(crate::container::Kind::Chest);
        a.slots[0] = Some(Stack::new(264, 2));
        let mut b = crate::container::Container::new(crate::container::Kind::Chest);
        b.slots[0] = Some(Stack::new(388, 3));
        sim.containers.insert(container_key(west), a);
        sim.containers.insert(container_key(east), b);
        for clicked in [west, east] {
            let view = sim.container_view(&world, clicked).unwrap();
            assert_eq!(view.slots.len(), 54);
            assert_eq!(view.slots[0], Some(Stack::new(264, 2)));
            assert_eq!(view.slots[27], Some(Stack::new(388, 3)));
        }
        sim.window_click(
            &world,
            Some(east),
            SlotRef::Container(27),
            ClickAction::Pickup { right: false },
            2,
        );
        assert_eq!(sim.inventory.cursor, Some(Stack::new(388, 3)));
        assert!(sim.containers[&container_key(east)].slots[0].is_none());
        sim.window_click(
            &world,
            Some(west),
            SlotRef::Container(1),
            ClickAction::Pickup { right: false },
            2,
        );
        assert_eq!(
            sim.containers[&container_key(west)].slots[1],
            Some(Stack::new(388, 3))
        );
        sim.window_click(
            &world,
            Some(east),
            SlotRef::Container(0),
            ClickAction::QuickMove,
            2,
        );
        assert_eq!(sim.inventory.slots[8], Some(Stack::new(264, 2)));
        let restored: Simulation =
            serde_json::from_slice(&serde_json::to_vec(&sim).unwrap()).unwrap();
        assert_eq!(
            restored
                .container_view(&world, east)
                .unwrap()
                .slots
                .iter()
                .flatten()
                .map(|s| s.count as u32)
                .sum::<u32>(),
            3
        );
        world.set(west + IVec3::Y, 16);
        assert!(
            sim.container_view(&world, east).is_none(),
            "blocked half blocks the entire double chest"
        );
        assert!(!can_place_chest(&world, east + IVec3::X, 54));
    }
    #[test]
    #[ignore = "requires locally imported Minecraft resources"]
    fn creative_mining_rays_ignore_fluid_and_break_the_solid_behind_it() {
        let mut world = flat_world();
        let mut sim = Simulation::new(Mode::Creative, 189);
        let water = IVec3::new(8, 2, 8);
        let stone = IVec3::new(8, 2, 6);
        world.set(water, 9 << 4);
        world.set(stone, 16);
        let mut player = Player::new(Vec3::new(8.5, 1., 10.5));
        player.yaw = 0.;
        player.pitch = 0.;
        let resources = Resources::load().unwrap();
        let out = sim.tick(
            &mut world,
            &mut player,
            &resources,
            &Settings::default(),
            true,
            false,
        );
        assert!(out.changed.contains(&stone));
        assert_eq!(world.get(stone), 0);
        assert!(matches!(world.get(water) >> 4, 8 | 9));
        assert!(!out.changed.contains(&water));
        assert_eq!(sim.break_target, None);
    }
    #[test]
    fn tools_control_speed_and_drops() {
        assert!(!harvestable(56 << 4, None));
        assert!(!harvestable(56 << 4, Some(Stack::new(270, 1))));
        assert!(harvestable(56 << 4, Some(Stack::new(257, 1))));
        let c = crate::vanilla::mining::Conditions {
            grounded: true,
            eyes_in_water: false,
            aqua_affinity: false,
            haste: None,
            fatigue: None,
        };
        let iron_pick = Stack::new(257, 1);
        let rate = |held| {
            crate::vanilla::mining::increment(
                1 << 4,
                held,
                crate::vanilla::mining::hardness(1 << 4),
                c,
            )
        };
        assert!(rate(Some(&iron_pick)) > rate(None));
        let mut rng = 1;
        assert!(block_drops(20 << 4, None, &mut rng).is_empty());
        assert_eq!(
            block_drops(1 << 4, Some(Stack::new(270, 1)), &mut rng)[0].id,
            4
        );
    }
    #[test]
    fn dropped_stacks_merge_and_creative_is_invulnerable() {
        let mut s = Simulation::new(Mode::Creative, 2);
        s.spawn_drop(Stack::new(4, 3), Vec3::ZERO, Vec3::ZERO);
        s.spawn_drop(Stack::new(4, 4), Vec3::ZERO, Vec3::ZERO);
        assert_eq!(s.drops.len(), 2, "spawning alone never merges stacks");
        let mut w = crate::world::World { spawn: Vec3::ZERO, chunks: Default::default(), label: String::new(), save_path: Default::default(), imported: false, edits: Default::default(), light_revision: 0 };
        let mut chunk=crate::world::Chunk::empty();chunk.set(0,0,0,16);w.chunks.insert((0,0),std::sync::Arc::new(chunk));
        for d in &mut s.drops { d.set_feet(glam::DVec3::new(0.5,1.,0.5)); }
        for _ in 0..25 {s.tick_drop_motion(&w,&mut Outcome::default());}
        assert_eq!(s.drops.len(),1);
        assert_eq!(s.drops[0].stack.count,7);
        s.damage(20.0);
        assert_eq!(s.health, 20.0);
    }
    #[test]
    fn original_food_nbt_and_game_rules_survive_native_save_reload() {
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/vanilla189-food.json")).unwrap();
        let fixture = &data["legacy_import"];
        let encoded = fixture["gzip_hex"].as_str().unwrap();
        let bytes: Vec<_> = (0..encoded.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&encoded[i..i + 2], 16).unwrap())
            .collect();
        let path = std::env::temp_dir().join(format!(
            "ferrum-food-import-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("level.dat"), bytes).unwrap();
        let mut sim = Simulation::default();
        sim.import_player(&path).unwrap();
        assert!(!sim.natural_regeneration);
        assert_eq!(sim.clock, 123456);
        assert_eq!(sim.player_ticks, 0);
        let expected = &fixture["food"];
        assert_eq!(sim.food as i64, expected["food"].as_i64().unwrap());
        assert_eq!(sim.food_timer as i64, expected["timer"].as_i64().unwrap());
        assert_eq!(
            u64::from(sim.saturation.to_bits()),
            expected["saturation"].as_u64().unwrap()
        );
        assert_eq!(
            u64::from(sim.exhaustion.to_bits()),
            expected["exhaustion"].as_u64().unwrap()
        );
        assert_eq!(
            u64::from(sim.health.to_bits()),
            expected["health"].as_u64().unwrap()
        );
        assert_eq!(
            u64::from(sim.max_health.to_bits()),
            fixture["maximum"].as_u64().unwrap()
        );
        let save = path.join("world.json");
        std::fs::write(
            save.with_extension("player.json"),
            serde_json::to_vec(&sim).unwrap(),
        )
        .unwrap();
        let restored = Simulation::read_legacy_save(&save)
            .unwrap()
            .unwrap()
            .restore();
        assert_eq!(restored.food, -1);
        assert_eq!(restored.food_timer, 80);
        assert_eq!(restored.saturation.to_bits(), (-0.0f32).to_bits());
        assert_eq!(restored.exhaustion, 41.0);
        assert_eq!(restored.max_health, 40.0);
        assert!(!restored.natural_regeneration);
        std::fs::remove_file(save.with_extension("player.json")).unwrap();
        std::fs::remove_file(path.join("level.dat")).unwrap();
        std::fs::remove_dir(path).unwrap();
    }
    #[test]
    fn actual_food_completion_is_connected_to_inventory_effects_and_world_rng() {
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/vanilla189-item-food.json"))
                .unwrap();
        for (case, row) in data["finish"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .filter(|(_, r)| r["remote"] == false)
        {
            let mut sim = Simulation {
                food: row["level"].as_i64().unwrap() as i32,
                saturation: 0.,
                mode: if row["immune"] == true {
                    Mode::Creative
                } else {
                    Mode::Survival
                },
                ..Default::default()
            };
            let mut held = Stack::new(row["id"].as_u64().unwrap() as u16, 2);
            held.meta = row["meta"].as_u64().unwrap() as u16;
            sim.inventory.slots[0] = Some(held.clone());
            sim.item_use.start(held.identity, true);
            sim.block_ticks
                .set_world_random(&crate::vanilla::random::JavaRandom::new(
                    row["seed"].as_str().unwrap().parse().unwrap(),
                ));
            let mut out = Outcome::default();
            for _ in 0..32 {
                sim.tick_item_use(true, &mut out);
            }
            assert_eq!(
                sim.food as i64,
                row["food"]["food"].as_i64().unwrap(),
                "case{case} food"
            );
            assert_eq!(
                sim.saturation.to_bits() as u64,
                row["food"]["saturation"].as_u64().unwrap(),
                "case{case} saturation"
            );
            assert_eq!(
                sim.absorption.to_bits() as u64,
                row["absorption"].as_u64().unwrap(),
                "case{case} absorption"
            );
            let returned = sim.inventory.held().unwrap();
            assert_eq!(
                serde_json::json!([returned.id, returned.meta, returned.count]),
                row["returned"],
                "case{case} stack"
            );
            assert_eq!(
                returned.identity == held.identity,
                returned.id == held.id,
                "case{case} returned identity"
            );
            assert_eq!(
                sim.effects.ordered().count(),
                row["effects"].as_array().unwrap().len(),
                "case{case} effects"
            );
            for effect in row["effects"].as_array().unwrap() {
                let native = sim.effects.get(effect[0].as_u64().unwrap() as u8).unwrap();
                assert_eq!(
                    serde_json::json!([native.id, native.duration, native.amplifier]),
                    *effect,
                    "case{case} effect"
                );
            }
            let burp = out
                .world_sounds
                .iter()
                .find(|s| s.0 == "random.burp")
                .expect("expected random.burp sound");
            assert_eq!(
                burp.3.to_bits() as u64,
                row["pitches"][0].as_u64().unwrap(),
                "case{case} burp"
            );
            assert_eq!(
                sim.block_ticks.world_random().next_long().to_string(),
                row["tail"].as_str().unwrap(),
                "case{case} RNG"
            );
        }
    }
    #[test]
    fn using_food_follows_stack_identity_and_effects_survive_save() {
        let mut sim = Simulation {
            food: 10,
            ..Default::default()
        };
        let item = Stack::new(260, 3);
        sim.inventory.slots[0] = Some(item.clone());
        sim.start_eating(item.clone());
        let mut out = Outcome::default();
        sim.tick_item_use(true, &mut out);
        sim.inventory.slots[0] = Some(item.copied());
        sim.tick_item_use(true, &mut out);
        assert!(!sim.item_use.active());
        assert_eq!(sim.food, 10);
        sim.inventory.slots[0] = Some(item.clone());
        sim.start_eating(item.clone());
        sim.inventory.slots[0] = None;
        sim.inventory.slots[1] = Some(item);
        sim.inventory.selected = 1;
        for _ in 0..32 {
            sim.tick_item_use(true, &mut out);
        }
        assert_eq!(sim.inventory.held().unwrap().count, 2);
        assert_eq!(sim.food, 14);
        sim.add_food_effect(Effect::new(22, 2400, 0));
        sim.add_food_effect(Effect::new(10, 100, 1));
        let restored: Simulation =
            serde_json::from_str(&serde_json::to_string(&sim).unwrap()).unwrap();
        assert_eq!(restored.absorption, 4.);
        assert_eq!(restored.effects.get(10).unwrap().duration, 100);
        assert!(!restored.item_use.active());
    }
    #[test]
    fn damage_absorption_resistance_and_hit_window_match_original_player() {
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/vanilla189-potions.json"))
                .unwrap();
        let amounts = [
            1f32,
            3.,
            0.,
            f32::from_bits(0x3f800001),
            8.,
            1.,
            1.,
            2.,
            2.,
            2.,
            2.,
            2.,
            1.,
            1.,
            1.,
            6.,
            3.,
            3.,
            3.,
            4.,
            4.,
            1.,
            2.,
            9.,
        ];
        for (case, row) in data["combat"].as_array().unwrap().iter().enumerate() {
            let mut sim = Simulation {
                mode: if row[1] == true {
                    Mode::Creative
                } else {
                    Mode::Survival
                },
                absorption: f32::from_bits(row[5].as_u64().unwrap() as u32),
                ..Default::default()
            };
            let armor = row[0].as_u64().unwrap();
            match armor {
                3 => sim.inventory.slots[38] = Some(Stack::new(299, 1)),
                7 => {
                    sim.inventory.slots[38] = Some(Stack::new(307, 1));
                    sim.inventory.slots[39] = Some(Stack::new(298, 1));
                }
                20 => {
                    for i in 0..4 {
                        sim.inventory.slots[39 - i] = Some(Stack::new(310 + i as u16, 1));
                    }
                }
                _ => {}
            }
            assert_eq!(u64::from(sim.inventory.armor()), armor);
            let resistance = row[2].as_i64().unwrap() as i32;
            let resistance = (resistance != -99).then_some(resistance);
            let fire = row[3] == true;
            let source = row[4].as_u64().unwrap();
            for (tick, amount) in amounts.into_iter().enumerate() {
                sim.hurt_ticks = sim.hurt_ticks.saturating_sub(1);
                if !(fire && source == 4) {
                    sim.damage_resisted(amount, source >= 4, source == 2, source == 3, resistance);
                }
                assert_eq!(
                    serde_json::json!([
                        sim.health.to_bits(),
                        sim.absorption.to_bits(),
                        sim.exhaustion.to_bits(),
                        sim.hurt_ticks
                    ]),
                    row[6][tick],
                    "combat{case} tick{tick}"
                );
            }
        }
    }
    #[test]
    fn creative_regeneration_and_peaceful_order_follow_player_callbacks() {
        let mut sim = Simulation {
            mode: Mode::Creative,
            health: 19.,
            food_timer: 79,
            ..Default::default()
        };
        sim.add_exhaustion(0.8);
        assert_eq!(sim.exhaustion, 0.);
        sim.tick_food(2);
        assert_eq!(sim.health, 20.);
        assert_eq!(sim.exhaustion, 3.);
        sim.health = 19.;
        sim.food = 17;
        sim.food_timer = 79;
        sim.player_ticks = 20;
        sim.tick_peaceful_food(0);
        sim.tick_food(0);
        assert_eq!(sim.health, 20.);
        assert_eq!(sim.food, 18);
        assert_eq!(sim.food_timer, 0);
        assert_eq!(sim.exhaustion, 3.);
        sim.health = 19.;
        sim.food = 17;
        sim.natural_regeneration = false;
        sim.tick_peaceful_food(0);
        sim.tick_food(0);
        assert_eq!(sim.health, 19.);
        assert_eq!(sim.food, 17);
    }
    #[test]
    fn milk_drinking_clears_effects_returns_bucket_and_potions_drink_or_splash() {
        let mut sim = Simulation {
            absorption: 8.0,
            ..Default::default()
        };
        sim.command_player_effect(command_entity_state::EffectEdit::Add(Effect::new(1, 600, 1)));
        sim.command_player_effect(command_entity_state::EffectEdit::Add(Effect::new(21, 600, 0)));
        assert_eq!(sim.max_health, 24.0);
        let milk = Stack::new(335, 1);
        sim.inventory.slots[0] = Some(milk.clone());
        sim.item_use.start_for(milk.identity, 32);
        let mut out = Outcome::default();
        for _ in 0..32 {
            sim.tick_item_use(true, &mut out);
        }
        assert_eq!(sim.effects.ordered().count(), 0);
        assert_eq!(sim.absorption, 0.0);
        assert_eq!(sim.max_health, 20.0);
        assert_eq!(sim.inventory.held().map(|s| (s.id, s.count)), Some((325, 1)));
        assert!(out.world_sounds.iter().any(|s| s.0 == "random.drink"));

        let milk_stack = Stack::new(335, 2);
        sim.inventory.slots[0] = Some(milk_stack.clone());
        sim.item_use.start_for(milk_stack.identity, 32);
        for _ in 0..32 {
            sim.tick_item_use(true, &mut out);
        }
        assert_eq!(sim.inventory.slots[0].as_ref().map(|s| (s.id, s.count)), Some((335, 1)));
        assert!(sim.inventory.slots[1..].iter().flatten().any(|s| s.id == 325 && s.count == 1));

        let mut potion = Stack::new(373, 1);
        potion.meta = 8194;
        sim.inventory.slots[0] = Some(potion.clone());
        sim.item_use.start_for(potion.identity, 32);
        let mut pot_out = Outcome::default();
        for _ in 0..32 {
            sim.tick_item_use(true, &mut pot_out);
        }
        assert!(pot_out.world_sounds.iter().any(|s| s.0 == "random.drink"));
        assert!(!pot_out.world_sounds.iter().any(|s| s.0 == "random.eat"));
        assert!(sim.effects.get(1).is_some());
        assert_eq!(sim.inventory.held().map(|s| s.id), Some(374));

        sim.command_player_effect(command_entity_state::EffectEdit::Clear);
        let mut world = flat_world();
        let mut player = Player::new(Vec3::new(8.5, 1.0, 8.5));
        player.set_rotation_degrees(0.0, 90.0);
        sim.mobs.push(cow(2, 9.2));
        let mut prng = JavaRandom::new(7);
        let splash = mob_shots::Shot::thrown_splash(&player, 0x4000 | 8194, &mut prng);
        sim.mob_shots.push(splash);
        let mut splash_out = Outcome::default();
        for _ in 0..8 {
            sim.tick_mob_shots(&mut world, &mut player, &Settings::default(), &mut splash_out);
        }
        assert!(sim.mob_shots.is_empty());
        assert!(splash_out.world_sounds.iter().any(|s| s.0 == "game.potion.smash"));
        assert!(sim.effects.get(1).is_some());
        assert!(sim.command_entities.mobs[&2].effects.get(1).is_some());
    }
    #[test]
    fn sheep_shears_dye_regrowth_and_spawn_egg_variant_inheritance() {
        use crate::vanilla::drops::DropNbt as Tag;
        let mut world = flat_world();
        let mut player = Player::new(Vec3::new(8.5, 1.0, 8.5));
        let mut sim = Simulation {
            mob_spawning: false,
            ..Default::default()
        };
        let mut sheep = cow(2, 6.5);
        sheep.kind = MobKind::Sheep;
        sim.mobs.push(sheep);
        let mut out = Outcome::default();

        let mut dye = Stack::new(351, 2);
        dye.meta = 4;
        sim.inventory.slots[0] = Some(dye);
        assert!(sim.interact_pet_or_mooshroom(0, &player, &mut out));
        assert_eq!(species_ai::number(sim.command_entities.mobs.get(&2), "Color"), 11);
        assert_eq!(sim.inventory.held().unwrap().count, 1);

        sim.inventory.slots[0] = Some(Stack::new(359, 1));
        assert!(sim.interact_pet_or_mooshroom(0, &player, &mut out));
        assert_eq!(species_ai::number(sim.command_entities.mobs.get(&2), "Sheared"), 1);
        assert_eq!(sim.inventory.held().unwrap().damage, 1);
        assert!(out.world_sounds.iter().any(|s| s.0 == "mob.sheep.shear"));
        let wool_drops = sim.drops.iter().filter(|d| d.stack.id == 35 && d.stack.meta == 11).count();
        assert!((1..=3).contains(&wool_drops));

        sim.drops.clear();
        sim.kill_drops(2, MobKind::Sheep, Vec3::new(6.5, 1.0, 8.5), false);
        assert!(!sim.drops.iter().any(|d| d.stack.id == 35));

        sim.mobs[0].eat_ticks = 5;
        sim.tick_entities(&mut world, &mut player, &Settings::default(), &mut out);
        assert_eq!(world.get(IVec3::new(6, 0, 8)), 3 << 4);
        assert_eq!(species_ai::number(sim.command_entities.mobs.get(&2), "Sheared"), 0);

        sim.drops.clear();
        sim.kill_drops(2, MobKind::Sheep, Vec3::new(6.5, 1.0, 8.5), false);
        assert!(sim.drops.iter().any(|d| d.stack.id == 35 && d.stack.meta == 11 && d.stack.count == 1));

        let egg = Stack::new(383, 1);
        let lamb = sim.spawn_egg_child(&egg, MobKind::Sheep, Vec3::new(6.5, 1.0, 8.5), 2);
        assert!(sim.is_baby(lamb));
        assert_eq!(species_ai::number(sim.command_entities.mobs.get(&lamb), "Color"), 11);

        let horse_id = sim.spawn_registered_raw(MobKind::Horse, Vec3::new(7.5, 1.0, 8.5), 0.0);
        let hdata = sim.command_entities.mobs.entry(horse_id).or_default();
        hdata.extra.insert("Type".into(), Tag::Int(0));
        hdata.extra.insert("Variant".into(), Tag::Int(0x0205));
        let foal = sim.spawn_egg_child(&egg, MobKind::Horse, Vec3::new(7.5, 1.0, 8.5), horse_id);
        assert!(sim.is_baby(foal));
        assert_eq!(species_ai::number(sim.command_entities.mobs.get(&foal), "Variant"), 0x0205);
    }
    #[test]
    fn wolf_defense_teleport_pig_saddle_creeper_ignition_and_dying_mobs_poof() {
        use crate::vanilla::drops::DropNbt as Tag;
        let mut world = flat_world();
        let mut player = Player::new(Vec3::new(8.5, 1.0, 8.5));
        let mut sim = Simulation {
            mob_spawning: false,
            ..Default::default()
        };
        let mut wolf = cow(2, 6.5);
        wolf.kind = MobKind::Wolf;
        wolf.health = 8.0;
        sim.mobs.push(wolf);
        let mut out = Outcome::default();

        let owner = mob_pets::owner_id(&sim.local_name);
        {
            let wdata = sim.command_entities.mobs.entry(2).or_default();
            wdata.extra.insert("OwnerUUID".into(), Tag::String(owner));
            wdata.extra.insert("CollarColor".into(), Tag::Byte(1));
        }
        sim.mobs[0].health = 12.0;
        sim.inventory.slots[0] = Some(Stack::new(364, 1));
        assert!(sim.interact_pet_or_mooshroom(0, &player, &mut out));
        assert_eq!(sim.mobs[0].health, 20.0);
        assert_eq!(sim.mob_brains.entry(2).or_default().love, 0);

        let mut dye = Stack::new(351, 1);
        dye.meta = 4;
        sim.inventory.slots[0] = Some(dye);
        assert!(sim.interact_pet_or_mooshroom(0, &player, &mut out));
        assert_eq!(species_ai::number(sim.command_entities.mobs.get(&2), "CollarColor"), 4);

        let mut zombie = cow(3, 9.5);
        zombie.kind = MobKind::Zombie;
        zombie.health = 20.0;
        sim.mobs.push(zombie);
        sim.alert_owned_wolves(3, player.position);
        assert_eq!(sim.mob_brains[&2].target_mob, Some(3));

        sim.mob_brains.get_mut(&2).unwrap().target_mob = None;
        sim.mobs[0].position = Vec3::new(1.0, 1.0, 1.0);
        player.position = Vec3::new(13.5, 1.0, 13.5);
        sim.tick_entities(&mut world, &mut player, &Settings::default(), &mut out);
        assert!(sim.mobs[0].position.distance(player.position) < 5.0);
        player.position = Vec3::new(8.5, 1.0, 8.5);

        let mut pig = cow(4, 7.5);
        pig.kind = MobKind::Pig;
        sim.mobs.push(pig);
        let pig_idx = sim.mobs.iter().position(|m| m.id == 4).unwrap();
        sim.inventory.slots[0] = Some(Stack::new(329, 1));
        assert!(sim.interact_pet_or_mooshroom(pig_idx, &player, &mut out));
        assert_eq!(species_ai::number(sim.command_entities.mobs.get(&4), "Saddle"), 1);
        assert!(sim.interact_pet_or_mooshroom(pig_idx, &player, &mut out));
        assert_eq!(sim.ridden_pig, Some(4));
        sim.drops.clear();
        sim.kill_drops(4, MobKind::Pig, Vec3::new(7.5, 1.0, 8.5), false);
        assert!(sim.drops.iter().any(|d| d.stack.id == 329));
        sim.ridden_pig = None;

        let mut creeper = cow(5, 12.5);
        creeper.kind = MobKind::Creeper;
        sim.mobs.push(creeper);
        let creeper_idx = sim.mobs.iter().position(|m| m.id == 5).unwrap();
        sim.inventory.slots[0] = Some(Stack::new(259, 1));
        assert!(sim.interact_pet_or_mooshroom(creeper_idx, &player, &mut out));
        assert_eq!(species_ai::number(sim.command_entities.mobs.get(&5), "ignited"), 1);
        assert!(out.world_sounds.iter().any(|s| s.0 == "fire.ignite"));
        sim.tick_entities(&mut world, &mut player, &Settings::default(), &mut out);
        assert!(out.world_sounds.iter().any(|s| s.0 == "creeper.primed"));

        let z_idx = sim.mobs.iter().position(|m| m.id == 3).unwrap();
        sim.mobs[z_idx].health = 0.0;
        sim.particles.clear();
        sim.tick_entities(&mut world, &mut player, &Settings::default(), &mut out);
        assert_eq!(sim.dying_mobs.len(), 1);
        assert_eq!(sim.dying_mobs[0].death_ticks, 1);
        for _ in 0..19 {
            sim.tick_dying_mobs();
        }
        assert!(sim.dying_mobs.is_empty());
        assert_eq!(sim.particles.len(), 20);
    }
    #[test]
    fn mining_drops_orientation_strip_and_fixed_point_neighbor_cascade() {
        let mut rng = 12345u64;
        assert_eq!(block_drops(63 << 4 | 5, None, &mut rng), vec![Stack::new(323, 1)]);
        assert_eq!(block_drops(68 << 4 | 3, None, &mut rng), vec![Stack::new(323, 1)]);
        assert_eq!(block_drops(83 << 4 | 7, None, &mut rng), vec![Stack::new(338, 1)]);
        assert_eq!(block_drops(149 << 4 | 2, None, &mut rng), vec![Stack::new(404, 1)]);
        assert_eq!(block_drops(150 << 4 | 6, None, &mut rng), vec![Stack::new(404, 1)]);
        let immature_cocoa = block_drops(127 << 4 | 1, None, &mut rng);
        assert_eq!((immature_cocoa[0].id, immature_cocoa[0].meta, immature_cocoa[0].count), (351, 3, 1));
        let mature_cocoa = block_drops(127 << 4 | (2 << 2), None, &mut rng);
        assert_eq!((mature_cocoa[0].id, mature_cocoa[0].meta, mature_cocoa[0].count), (351, 3, 3));
        for id in [27u16, 28, 50, 65, 66, 69, 70, 72, 77, 96, 131, 143, 147, 148, 157, 167] {
            let pick = Stack::new(257, 1);
            let drops = block_drops((id << 4) | 5, Some(pick), &mut rng);
            assert_eq!(drops[0].meta, 0, "block {id} should strip orientation meta");
        }

        let mut world = flat_world();
        let mut player = Player::new(world.spawn);
        let mut sim = Simulation::default();
        let mut out = Outcome::default();
        let base = IVec3::new(8, 1, 8);
        world.set(IVec3::new(7, 0, 8), 9 << 4);
        world.set(base, 83 << 4);
        world.set(base + IVec3::Y, 83 << 4);
        world.set(base + IVec3::Y * 2, 83 << 4);
        world.set(base, 0);
        sim.block_ticks.notify(&world, base);
        let (mut ticked, mut nc, mut cc, mut notc) = (false, 0, 0, 0);
        sim.flush_neighbor_updates(
            &mut world,
            &mut player,
            &mut ticked,
            &mut nc,
            &mut cc,
            &mut notc,
            &mut out,
        );
        assert_eq!(world.get(base + IVec3::Y), 0);
        assert_eq!(world.get(base + IVec3::Y * 2), 0);
    }
    #[test]
    fn sp_sign_placement_creates_standing_and_wall_signs_and_queues_editor() {
        use crate::vanilla::{
            drops::DropNbt,
            ray_trace::{Hit, Kind},
        };
        use std::sync::Arc;

        let mut world = flat_world();
        let support = IVec3::new(8, 1, 8);
        world.set(support, 1 << 4);
        let mut player = Player::new(Vec3::new(8.5, 1.0, 5.5));
        player.set_rotation_degrees(0.0, 0.0);
        let mut sim = Simulation::new(Mode::Survival, 189);
        sim.inventory.slots[0] = Some(Stack::new(323, 3));
        let mut out = Outcome::default();

        let top_hit = Hit {
            kind: Kind::Block,
            position: support,
            face: 1,
            point: support.as_dvec3() + glam::DVec3::new(0.5, 1.0, 0.5),
        };
        let held = sim.inventory.held().unwrap();
        assert!(sim.place_held_block(&mut world, &player, &held, top_hit, &mut out));
        let standing_pos = support + IVec3::Y;
        assert_eq!(world.get(standing_pos) >> 4, 63);
        assert_eq!(sim.pending_sign_edit.take(), Some(standing_pos));
        assert_eq!(sim.inventory.held().unwrap().count, 2);
        assert!(world.tile(standing_pos).is_some());

        sim.place_ticks = 0;
        let north_hit = Hit {
            kind: Kind::Block,
            position: support,
            face: 2,
            point: support.as_dvec3() + glam::DVec3::new(0.5, 0.5, 0.0),
        };
        let held = sim.inventory.held().unwrap();
        assert!(sim.place_held_block(&mut world, &player, &held, north_hit, &mut out));
        let wall_pos = support + IVec3::NEG_Z;
        assert_eq!(world.get(wall_pos), (68 << 4) | 2);
        assert_eq!(sim.pending_sign_edit.take(), Some(wall_pos));
        assert_eq!(sim.inventory.held().unwrap().count, 1);

        let mut tagged = Stack::new(323, 1);
        tagged.nbt = Some(Arc::new(DropNbt::Compound(
            [(
                "BlockEntityTag".into(),
                DropNbt::Compound(
                    [("Text1".into(), DropNbt::String("\"SavedLine\"".into()))].into(),
                ),
            )]
            .into(),
        )));
        sim.inventory.slots[0] = Some(tagged);
        sim.place_ticks = 0;
        let east_hit = Hit {
            kind: Kind::Block,
            position: support,
            face: 5,
            point: support.as_dvec3() + glam::DVec3::new(1.0, 0.5, 0.5),
        };
        let held = sim.inventory.held().unwrap();
        assert!(sim.place_held_block(&mut world, &player, &held, east_hit, &mut out));
        let tagged_pos = support + IVec3::X;
        assert_eq!(world.get(tagged_pos), (68 << 4) | 5);
        assert_eq!(sim.pending_sign_edit, None);
    }
    #[test]
    fn distant_lava_mix_and_burning_item_emit_positional_world_sounds_not_2d() {
        let mut world = flat_world();
        let mut sim = Simulation::new(Mode::Survival, 189);
        let lava_pos = IVec3::new(2, 1, 2);
        world.set(lava_pos, 10 << 4);
        world.set(lava_pos + IVec3::X, 8 << 4);
        sim.block_ticks.notify(&world, lava_pos);
        let block_res = sim.block_ticks.tick(&mut world);
        assert_eq!(world.get(lava_pos) >> 4, 49);
        assert!(block_res.sounds.is_empty());
        assert!(block_res
            .world_sounds
            .iter()
            .any(|&(clip, pos, _, _)| matches!(clip, "random.fizz" | "random/fizz")
                && (pos - glam::DVec3::new(2.5, 1.5, 2.5)).length() < 0.01));

        let burn_pos = IVec3::new(12, 1, 12);
        world.set(burn_pos, 11 << 4);
        sim.spawn_drop(
            Stack::new(4, 1),
            Vec3::new(12.5, 2.02, 12.5),
            Vec3::new(0.0, -0.2, 0.0),
        );
        let mut out = Outcome::default();
        sim.tick_drop_motion(&world, &mut out);
        assert!(out.sounds.is_empty());
        assert!(out.pitched_sounds.is_empty());
        assert!(out
            .world_sounds
            .iter()
            .any(|&(clip, pos, _, _)| clip == "random.fizz"
                && (pos - glam::DVec3::new(12.5, 1.8, 12.5)).length() < 1.0));
    }

    #[test]
    fn nether_portal_timer_triggers_at_80_survival_1_creative_and_decays_by_4() {
        let mut world = flat_world();
        world.set(IVec3::new(8, 1, 8), 90 << 4);
        world.set(IVec3::new(8, 2, 8), 90 << 4);
        let settings = Settings::default();
        let mut sim = Simulation::new(Mode::Survival, 189);
        sim.mob_spawning = false;
        let mut player = Player::new(Vec3::new(8.5, 1.0, 8.5));

        // First tick latches in_portal during movement after the pre-move portal check.
        let first = sim.tick_local(&mut world, &mut player, &settings, false, false);
        assert_eq!(first.portal_travel, None);
        assert!(player.in_portal);
        assert_eq!(player.portal_counter, 0);

        for expected in 1..80 {
            let out = sim.tick_local(&mut world, &mut player, &settings, false, false);
            assert_eq!(out.portal_travel, None, "early travel at {expected}");
            assert_eq!(player.portal_counter, expected);
        }
        let travel = sim.tick_local(&mut world, &mut player, &settings, false, false);
        assert!(matches!(travel.portal_travel, Some(PortalTravel::Nether(_))));
        if let Some(PortalTravel::Nether(entry)) = travel.portal_travel {
            assert_eq!(entry.last_portal_pos, IVec3::new(8, 1, 8));
        }
        assert_eq!(player.portal_counter, 80);
        assert_eq!(player.portal_cooldown, 10);

        // Step out of the portal after partial charge and verify -4 decay per tick.
        let mut player2 = Player::new(Vec3::new(8.5, 1.0, 8.5));
        sim.tick_local(&mut world, &mut player2, &settings, false, false);
        for _ in 0..20 {
            sim.tick_local(&mut world, &mut player2, &settings, false, false);
        }
        assert_eq!(player2.portal_counter, 20);
        world.set(IVec3::new(8, 1, 8), 0);
        world.set(IVec3::new(8, 2, 8), 0);
        // The tick right after breaking the portal consumes the last latched in_portal,
        // then subsequent ticks outside decay by 4.
        sim.tick_local(&mut world, &mut player2, &settings, false, false);
        let peaked = player2.portal_counter;
        assert!(!player2.in_portal);
        sim.tick_local(&mut world, &mut player2, &settings, false, false);
        assert_eq!(player2.portal_counter, peaked - 4);
        sim.tick_local(&mut world, &mut player2, &settings, false, false);
        assert_eq!(player2.portal_counter, peaked - 8);

        // Creative mode triggers Nether travel after 1 tick of in_portal.
        world.set(IVec3::new(8, 1, 8), 90 << 4);
        world.set(IVec3::new(8, 2, 8), 90 << 4);
        sim.mode = Mode::Creative;
        let mut creative_player = Player::new(Vec3::new(8.5, 1.0, 8.5));
        let c0 = sim.tick_local(&mut world, &mut creative_player, &settings, false, false);
        assert_eq!(c0.portal_travel, None);
        assert!(creative_player.in_portal);
        let c1 = sim.tick_local(&mut world, &mut creative_player, &settings, false, false);
        assert!(matches!(c1.portal_travel, Some(PortalTravel::Nether(_))));
    }

    #[test]
    fn end_portal_contact_immediately_triggers_end_travel() {
        let mut world = flat_world();
        world.set(IVec3::new(8, 1, 8), 119 << 4);
        let mut sim = Simulation::new(Mode::Survival, 189);
        sim.mob_spawning = false;
        let mut player = Player::new(Vec3::new(8.5, 1.0, 8.5));
        let out = sim.tick_local(&mut world, &mut player, &Settings::default(), false, false);
        assert!(player.touching_end_portal());
        assert_eq!(out.portal_travel, Some(PortalTravel::End));
    }

    #[test]
    fn dimension_slices_isolate_mobs_containers_drops_and_edits() {
        let mut world = flat_world();
        world.spawn = Vec3::new(8.5, 1.0, 8.5);
        let mut sim = Simulation::new(Mode::Survival, 189);

        // Populate Overworld (0).
        world.set(IVec3::new(2, 1, 2), 45 << 4);
        let mut ow_chest = crate::container::Container::new(crate::container::Kind::Chest);
        ow_chest.slots[0] = Some(Stack::new(264, 5));
        sim.containers
            .insert(container_key(IVec3::new(2, 2, 2)), ow_chest);
        sim.spawn_registered(MobKind::Cow, Vec3::new(4.5, 1.0, 4.5), 0.0);
        sim.spawn_drop(Stack::new(264, 3), Vec3::new(5.5, 1.5, 5.5), Vec3::ZERO);

        // Stash Overworld and activate Nether (-1).
        sim.stash_active_dimension(&world);
        world.edits.clear();
        sim.activate_dimension(-1, &mut world);
        assert_eq!(sim.dimension, -1);
        assert!(sim.mobs.is_empty());
        assert!(sim.containers.is_empty());
        assert!(sim.drops.is_empty());
        assert!(world.edits.is_empty());
        assert!(sim
            .overworld_saved_edits(&world)
            .contains(&([2, 1, 2], 45 << 4)));

        // Populate Nether (-1).
        world.set(IVec3::new(6, 1, 6), 87 << 4);
        let mut nether_chest = crate::container::Container::new(crate::container::Kind::Chest);
        nether_chest.slots[1] = Some(Stack::new(372, 4));
        sim.containers
            .insert(container_key(IVec3::new(6, 2, 6)), nether_chest);
        sim.spawn_registered(MobKind::Blaze, Vec3::new(7.5, 1.0, 7.5), 0.0);
        sim.spawn_drop(Stack::new(369, 2), Vec3::new(7.5, 1.5, 7.5), Vec3::ZERO);

        // Stash Nether and activate End (1).
        sim.stash_active_dimension(&world);
        world.edits.clear();
        sim.activate_dimension(1, &mut world);
        assert_eq!(sim.dimension, 1);
        assert_eq!(world.spawn, Vec3::new(100.5, 49.0, 0.5));
        assert!(sim.mobs.is_empty());
        assert!(sim.containers.is_empty());
        assert!(sim.drops.is_empty());
        world.set(IVec3::new(10, 1, 10), 121 << 4);
        let crystal_id = sim.alloc_crystal_id();
        sim.end_crystals.push(EndCrystal {
            id: crystal_id,
            position: glam::DVec3::new(10.5, 2.0, 10.5),
            bottom: true,
            beam_target: None,
        });

        // Return to Nether (-1) and verify isolation.
        sim.stash_active_dimension(&world);
        world.edits.clear();
        sim.activate_dimension(-1, &mut world);
        assert_eq!(sim.mobs.len(), 1);
        assert_eq!(sim.mobs[0].kind, MobKind::Blaze);
        assert_eq!(sim.drops.len(), 1);
        assert_eq!(sim.drops[0].stack.id, 369);
        assert!(sim.end_crystals.is_empty());
        assert_eq!(world.edits.get(&[6, 1, 6]).copied(), Some(87 << 4));
        assert!(!world.edits.contains_key(&[2, 1, 2]));

        // Return to Overworld (0) and verify original state + spawn.
        sim.stash_active_dimension(&world);
        world.edits.clear();
        sim.activate_dimension(0, &mut world);
        assert_eq!(world.spawn, Vec3::new(8.5, 1.0, 8.5));
        assert_eq!(sim.mobs.len(), 1);
        assert_eq!(sim.mobs[0].kind, MobKind::Cow);
        assert_eq!(sim.drops.len(), 1);
        assert_eq!(sim.drops[0].stack.id, 264);
        assert_eq!(
            sim.containers[&container_key(IVec3::new(2, 2, 2))].slots[0]
                .as_ref()
                .map(|s| (s.id, s.count)),
            Some((264, 5))
        );
        assert_eq!(world.edits.get(&[2, 1, 2]).copied(), Some(45 << 4));
    }

    #[test]
    fn bed_in_nether_and_end_explodes_and_ignites_fire() {
        for dim in [-1, 1] {
            let mut chunks = crate::world::ChunkMap::default();
            for cx in -1..=1 {
                for cz in -1..=1 {
                    let mut chunk = crate::world::Chunk::empty();
                    for x in 0..16 {
                        for z in 0..16 {
                            chunk.set(x, 0, z, 7 << 4);
                            for y in 1..=8 {
                                chunk.set(x, y, z, 87 << 4);
                            }
                        }
                    }
                    chunks.insert((cx, cz), std::sync::Arc::new(chunk));
                }
            }
            let mut world = World {
                chunks,
                spawn: Vec3::new(8.5, 9.0, 8.5),
                label: "Bed explosion test".into(),
                save_path: Default::default(),
                edits: Default::default(),
                imported: false,
                light_revision: 0,
            };
            let foot = IVec3::new(8, 9, 8);
            let head = IVec3::new(8, 9, 9);
            world.set(foot, 26 << 4);
            world.set(head, (26 << 4) | 8);

            let mut sim = Simulation::new(Mode::Survival, 189);
            sim.dimension = dim;
            sim.mob_spawning = false;
            let mut player = Player::new(Vec3::new(8.5, 9.0, 6.5));
            player.set_rotation_degrees(0.0, 25.0);
            let mut out = Outcome::default();
            let (attempt, _) = sim.try_bed_with_outcome(
                &mut world,
                &mut player,
                foot,
                &Settings::default(),
                &mut out,
            );
            assert_eq!(attempt, crate::vanilla::sleep::Attempt::Exploded);
            assert!(!player.sleeping);
            assert_ne!(world.get(foot) >> 4, 26);
            assert_ne!(world.get(head) >> 4, 26);
            assert!(out
                .world_sounds
                .iter()
                .any(|&(clip, _, _, _)| clip == "random.explode"));
            assert!(
                out.changed.iter().any(|&p| world.get(p) >> 4 == 51),
                "expected flaming bed explosion in dim {dim} to place fire"
            );
        }
    }

    #[test]
    fn slaying_ender_dragon_in_end_builds_exit_podium_and_grants_boss_xp() {
        let mut chunks = crate::world::ChunkMap::default();
        for cx in -1..=1 {
            for cz in -1..=1 {
                let mut chunk = crate::world::Chunk::empty();
                for x in 0..16 {
                    for z in 0..16 {
                        for y in 0..=58 {
                            chunk.set(x, y, z, 121 << 4);
                        }
                    }
                }
                chunks.insert((cx, cz), std::sync::Arc::new(chunk));
            }
        }
        let mut world = World {
            chunks,
            spawn: Vec3::new(100.5, 49.0, 0.5),
            label: "End podium test".into(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        };
        let mut sim = Simulation::new(Mode::Survival, 189);
        sim.mob_spawning = false;
        sim.activate_dimension(1, &mut world);
        sim.adopt_dimension_meta(
            1,
            &mut world,
            (0, 0),
            crate::dimension_world::GeneratedDimensionMeta {
                chests: Vec::new(),
                spawners: Vec::new(),
                end_crystals: vec![[12.5, 60.0, 0.5]],
                ender_dragon: Some([0.0, 128.0, 0.0]),
            },
        );
        assert!(sim.end_dragon_spawned);
        assert!(!sim.end_dragon_slain);
        assert_eq!(sim.end_crystals.len(), 1);
        assert_eq!(world.get(IVec3::new(12, 60, 0)) >> 4, 51);
        assert_eq!(sim.mobs.len(), 1);
        assert_eq!(sim.mobs[0].kind, MobKind::EnderDragon);

        let mut player = Player::new(Vec3::new(14.5, 59.0, 14.5));
        sim.mobs[0].position = Vec3::new(5.5, 70.0, -3.5);
        sim.mobs[0].health = 0.0;

        let out1 = sim.tick_local(&mut world, &mut player, &Settings::default(), false, false);
        assert!(!sim.end_dragon_slain);
        assert_eq!(sim.mobs.len(), 1);
        assert!(sim.dying_mobs.is_empty());
        assert!((sim.mobs[0].position.y - 70.1).abs() < 1e-4);
        assert!(out1
            .world_sounds
            .iter()
            .any(|&(clip, _, _, _)| clip == "mob.enderdragon.end"));

        for _ in 2..200 {
            let out_mid =
                sim.tick_local(&mut world, &mut player, &Settings::default(), false, false);
            assert!(!sim.end_dragon_slain);
            assert!(!out_mid
                .changed
                .iter()
                .any(|&p| matches!(world.get(p) >> 4, 7 | 50 | 119 | 122)));
        }
        assert_eq!(world.get(IVec3::new(12, 60, 0)) >> 4, 51);

        let out = sim.tick_local(&mut world, &mut player, &Settings::default(), false, false);
        assert!(sim.end_dragon_slain);
        assert!(sim.mobs.is_empty());
        let total_xp: i32 = sim.xp_orbs.iter().map(|o| o.value).sum();
        assert_eq!(total_xp, 12_000);
        assert!(out.changed.iter().any(|&p| world.get(p) >> 4 == 119));
        assert!(out.changed.iter().any(|&p| world.get(p) >> 4 == 7));
        assert!(out.changed.iter().any(|&p| world.get(p) >> 4 == 50));
        assert!(out.changed.iter().any(|&p| world.get(p) >> 4 == 122));
        assert_eq!(world.get(IVec3::new(5, 68, -4)) >> 4, 122);
        assert_eq!(world.get(IVec3::new(0, 68, 0)), 0);
    }

    #[test]
    fn player_fov_modifier_matches_189_attributes_and_bow_draw() {
        let mut p = Player::new(Vec3::new(8.5, 1.0, 8.5));
        assert!((p.fov_modifier(None) - 1.0).abs() < 1e-6);

        p.sprinting = true;
        assert!((p.fov_modifier(None) - 1.15).abs() < 1e-5);

        p.flying = true;
        assert!((p.fov_modifier(None) - 1.1 * 1.15).abs() < 1e-5);
        p.flying = false;
        p.sprinting = false;

        p.speed_boost = Some(1); // Speed II: +40% -> (1.4 + 1)/2 = 1.2
        assert!((p.fov_modifier(None) - 1.2).abs() < 1e-5);
        p.speed_boost = None;

        p.slowness = Some(0); // Slowness I: -15% -> (0.85 + 1)/2 = 0.925
        assert!((p.fov_modifier(None) - 0.925).abs() < 1e-5);
        p.slowness = None;

        assert!((p.fov_modifier(Some(10)) - (1.0 - 0.25 * 0.15)).abs() < 1e-5);
        assert!((p.fov_modifier(Some(30)) - 0.85).abs() < 1e-5);
    }

    #[test]
    fn enderman_stare_pumpkin_creative_aggro_and_dodge_match_189() {
        let mut world = flat_world();
        let settings = Settings::default();
        let mut sim = Simulation::new(Mode::Creative, 189);
        sim.mob_spawning = false;
        sim.clock = 18_000; // Midnight so daylight escape does not interfere.
        let eid = sim.spawn_registered(MobKind::Enderman, Vec3::new(8.5, 1.0, 14.5), 0.0);
        let mut player = Player::new(Vec3::new(8.5, 1.0, 8.5));
        player.set_rotation_degrees(0.0, 4.0); // Looking straight at Enderman (+Z, eye ~2.62 vs 2.45).
        let dir = (Vec3::new(8.5, 2.45, 14.5) - player.eye(1.0)).normalize();
        let yaw_deg = (-dir.x).atan2(dir.z).to_degrees();
        let pitch_deg = (-dir.y).asin().to_degrees();
        player.set_rotation_degrees(yaw_deg, pitch_deg);

        // Creative stare & melee hit must NEVER aggro Enderman.
        for _ in 0..10 {
            sim.tick_local(&mut world, &mut player, &settings, false, false);
        }
        assert!(!sim.mob_brains[&eid].enderman_screaming);
        assert_eq!(sim.mob_brains[&eid].anger_ticks, 0);

        // Pumpkin helmet in Survival prevents stare aggro.
        sim.mode = Mode::Survival;
        sim.inventory.slots[39] = Some(Stack::new(86, 1));
        for _ in 0..10 {
            sim.tick_local(&mut world, &mut player, &settings, false, false);
        }
        assert!(!sim.mob_brains[&eid].enderman_screaming);

        // Removing Pumpkin triggers screaming + stare sound after 5 consecutive stare ticks.
        sim.inventory.slots[39] = None;
        let mut heard_stare = false;
        for _ in 0..6 {
            let out = sim.tick_local(&mut world, &mut player, &settings, false, false);
            heard_stare |= out
                .world_sounds
                .iter()
                .any(|&(c, _, _, _)| c == "mob.endermen.stare");
        }
        assert!(heard_stare);
        assert!(sim.mob_brains[&eid].enderman_screaming);

        // Switching to Creative immediately clears Enderman anger & screaming.
        sim.mode = Mode::Creative;
        sim.tick_local(&mut world, &mut player, &settings, false, false);
        assert!(!sim.mob_brains[&eid].enderman_screaming);
        assert_eq!(sim.mob_brains[&eid].anger_ticks, 0);
    }

    #[test]
    fn splash_potions_undead_inversion_immunities_and_dragon_multipart_damage_match_189() {
        let mut world = flat_world();
        let settings = Settings::default();
        let mut sim = Simulation::new(Mode::Survival, 189);
        sim.mob_spawning = false;
        sim.clock = 18_000;
        let mut player = Player::new(Vec3::new(2.5, 1.0, 2.5));

        let zid = sim.spawn_registered(MobKind::Zombie, Vec3::new(8.5, 1.0, 8.5), 0.0);
        let sid = sim.spawn_registered(MobKind::Spider, Vec3::new(8.5, 1.0, 8.5), 0.0);
        let cow_id = sim.spawn_registered(MobKind::Cow, Vec3::new(8.5, 1.0, 8.5), 0.0);
        sim.mobs.iter_mut().find(|m| m.id == zid).unwrap().health = 10.0;

        // Direct Instant Health I (meta 16453) hurts Zombie by 6 << 0 = 6 (with 2 innate armor -> 5.52)
        // while Instant Damage I (meta 16460) heals Zombie by 4 << 0 = 4!
        let mut rng = JavaRandom::new(42);
        let mut shot_heal = mob_shots::Shot::thrown_splash(&player, 16453, &mut rng);
        shot_heal.position = Vec3::new(8.5, 1.2, 8.5);
        shot_heal.previous = Vec3::new(8.5, 2.0, 8.5);
        sim.mob_shots.push(shot_heal);
        let mut out = Outcome::default();
        sim.tick_mob_shots(&mut world, &mut player, &settings, &mut out);
        let z_after_heal = sim.mobs.iter().find(|m| m.id == zid).unwrap().health;
        assert!(z_after_heal < 5.0, "Instant Health I must hurt undead (got {z_after_heal})");

        let mut shot_harm = mob_shots::Shot::thrown_splash(&player, 16460, &mut rng);
        shot_harm.position = Vec3::new(8.5, 1.2, 8.5);
        shot_harm.previous = Vec3::new(8.5, 2.0, 8.5);
        sim.mob_shots.push(shot_harm);
        sim.tick_mob_shots(&mut world, &mut player, &settings, &mut out);
        let z_after_harm = sim.mobs.iter().find(|m| m.id == zid).unwrap().health;
        assert!(
            (z_after_harm - (z_after_heal + 4.0)).abs() < 1e-4,
            "Instant Damage I must heal undead by 4"
        );

        // Poison splash (meta 16388) poisons Cow, while Spider (52) & Zombie (undead) are immune!
        let mut shot_poison = mob_shots::Shot::thrown_splash(&player, 16388, &mut rng);
        shot_poison.position = Vec3::new(8.5, 1.2, 8.5);
        shot_poison.previous = Vec3::new(8.5, 2.0, 8.5);
        sim.mob_shots.push(shot_poison);
        sim.tick_mob_shots(&mut world, &mut player, &settings, &mut out);
        assert!(sim
            .command_entities
            .mobs
            .get(&cow_id)
            .is_some_and(|d| d.effects.get(19).is_some()));
        assert!(sim
            .command_entities
            .mobs
            .get(&sid)
            .is_none_or(|d| d.effects.get(19).is_none()));
        assert!(sim
            .command_entities
            .mobs
            .get(&zid)
            .is_none_or(|d| d.effects.get(19).is_none()));

        // EnderDragon multipart: head takes full damage, wing takes dmg / 4 + 1, healing crystal break deals 10.
        let did = sim.spawn_registered(MobKind::EnderDragon, Vec3::new(8.5, 6.0, 8.5), 0.0);
        let cid = sim.alloc_crystal_id();
        sim.end_crystals.push(EndCrystal {
            id: cid,
            position: glam::DVec3::new(12.5, 8.0, 8.5),
            bottom: true,
            beam_target: None,
        });
        sim.tick_entities(&mut world, &mut player, &settings, &mut out);
        assert_eq!(sim.mob_brains[&did].dragon_healing_crystal, Some(cid));
        sim.end_crystals.clear();
        sim.mobs.iter_mut().find(|m| m.id == did).unwrap().hurt = 0;
        sim.tick_entities(&mut world, &mut player, &settings, &mut out);
        let d_health = sim.mobs.iter().find(|m| m.id == did).unwrap().health;
        assert!(
            (d_health - 190.0).abs() < 1e-4,
            "destroying linked crystal must deal 10 explosion damage (got {d_health})"
        );
    }

    #[test]
    fn nether_death_autosave_round_trips_dimension_slices_and_tile_edits() {
        use crate::vanilla::drops::DropNbt as Tag;
        let mut world = flat_world();
        world.spawn = Vec3::new(12.5, 5.0, 20.5);
        let mut sim = Simulation::new(Mode::Survival, 12345);
        let mut player = Player::new(Vec3::new(12.5, 5.0, 20.5));

        // Overworld has block edits + tile entity edits (e.g., dungeon spawner & sign)
        let ow_spawner = IVec3::new(6, 5, 6);
        world.set(ow_spawner, 52 << 4);
        sim.world_commands.edit_tile(
            &mut world,
            ow_spawner,
            52 << 4,
            Some(Tag::Compound(std::collections::BTreeMap::from([
                ("id".into(), Tag::String("MobSpawner".into())),
                ("EntityId".into(), Tag::String("Zombie".into())),
                ("x".into(), Tag::Int(ow_spawner.x)),
                ("y".into(), Tag::Int(ow_spawner.y)),
                ("z".into(), Tag::Int(ow_spawner.z)),
            ]))),
        );

        // Travel to Nether (-1)
        sim.stash_active_dimension(&world);
        let mut nether_world = flat_world();
        sim.activate_dimension(-1, &mut nether_world);
        let nether_spawner = IVec3::new(3, 5, 3);
        nether_world.set(nether_spawner, 52 << 4);
        sim.world_commands.edit_tile(
            &mut nether_world,
            nether_spawner,
            52 << 4,
            Some(Tag::Compound(std::collections::BTreeMap::from([
                ("id".into(), Tag::String("MobSpawner".into())),
                ("EntityId".into(), Tag::String("Blaze".into())),
                ("x".into(), Tag::Int(nether_spawner.x)),
                ("y".into(), Tag::Int(nether_spawner.y)),
                ("z".into(), Tag::Int(nether_spawner.z)),
            ]))),
        );

        // Simulate /kill + death autosave capture in Nether
        sim.health = 0.0;
        sim.die(&player);
        sim.snapshot_active_dimension_for_save(&nether_world);

        // Must serialize & deserialize cleanly without "key must be a string"
        let json_val = serde_json::to_value(&sim)
            .expect("Simulation with Nether & Overworld tile_edits must serialize to JSON");
        let mut restored: Simulation = serde_json::from_value(json_val)
            .expect("Simulation with Nether & Overworld tile_edits must deserialize from JSON");

        // Legacy empty-map tile_edits format must also deserialize cleanly
        let legacy_slice: DimensionSlice =
            serde_json::from_value(serde_json::json!({ "tile_edits": {} }))
                .expect("legacy empty map tile_edits must deserialize");
        assert!(legacy_slice.tile_edits.is_empty());

        // Respawn back to Overworld (0)
        restored.stash_active_dimension(&nether_world);
        let mut ow_restored_world = flat_world();
        restored.activate_dimension(0, &mut ow_restored_world);
        assert_eq!(ow_restored_world.spawn, Vec3::new(12.5, 5.0, 20.5));
        assert_eq!(ow_restored_world.get(ow_spawner), 52 << 4);
        assert!(ow_restored_world.tile(ow_spawner).is_some());
        restored.respawn(&mut player, &ow_restored_world);
        assert_eq!(restored.dimension, 0);
        assert_eq!(restored.health, restored.max_health);
    }

    #[test]
    fn wet_sponge_dries_immediately_in_nether_and_absorbs_water_in_overworld() {
        let mut world = flat_world();
        let mut sim = Simulation::new(Mode::Survival, 42);

        // 1. Overworld: Dry Sponge (19:0) absorbs adjacent water (8 << 4) and turns Wet (19:1)
        let water_pos = IVec3::new(6, 5, 5);
        let sponge_pos = IVec3::new(5, 5, 5);
        world.set(water_pos, 8 << 4);
        world.set(sponge_pos, 19 << 4);
        let mut out_ow = Outcome::default();
        sim.on_sponge_added(&mut world, sponge_pos, 19 << 4, Some(&mut out_ow));
        assert_eq!(world.get(water_pos), 0, "water must be absorbed");
        assert_eq!(
            world.get(sponge_pos),
            (19 << 4) | 1,
            "dry sponge must become wet sponge after absorbing water"
        );

        // 2. Nether: Wet Sponge (19:1) placed by hand or /setblock immediately dries into Dry Sponge (19:0)
        sim.stash_active_dimension(&world);
        let mut nether_world = flat_world();
        sim.activate_dimension(-1, &mut nether_world);

        let mut player = Player::new(Vec3::new(5.5, 5.0, 8.5));
        player.yaw = 180.0;
        player.pitch = 45.0;
        sim.inventory.slots[0] = Some(Stack {
            id: 19,
            count: 2,
            damage: 0,
            meta: 1,
            nbt: None,
            identity: 77,
        });
        sim.inventory.selected = 0;

        let place_pos = IVec3::new(5, 5, 5);
        nether_world.set(place_pos, (19 << 4) | 1);
        let mut out_nether = Outcome::default();
        sim.on_sponge_added(
            &mut nether_world,
            place_pos,
            (19 << 4) | 1,
            Some(&mut out_nether),
        );
        assert_eq!(
            nether_world.get(place_pos),
            19 << 4,
            "wet sponge in Nether must dry immediately into dry sponge"
        );
        assert!(
            out_nether
                .world_sounds
                .iter()
                .any(|&(clip, _, _, _)| clip == "random.fizz"),
            "drying wet sponge in Nether must emit random.fizz"
        );
        assert!(
            !sim.particles.is_empty(),
            "drying wet sponge in Nether must emit steam smoke particles"
        );
    }
}
