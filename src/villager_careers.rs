//! EntityVillager career tables, prices, purchases and the trading-gated reset.
use super::{Offer, Trader};
use crate::{
    inventory::Stack,
    vanilla::{armor, drops::DropNbt, loot, random::JavaRandom},
};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
enum Rule {
    Buy(u16, u16, i32, i32),
    Sell(u16, u16, i32, i32),
    Gear(u16, u16, i32, i32),
    Exchange(u16, u16, i32, i32, u16, u16, i32, i32),
    Book,
}
include!("vanilla/villager_career_data.rs");

fn price(random: &mut JavaRandom, low: i32, high: i32) -> i32 {
    if low >= high {
        low
    } else {
        low + random.next_int(high - low + 1)
    }
}
fn item(id: u16, meta: u16, count: i32) -> Stack {
    let mut stack = Stack::new(id, count as u8);
    if armor::maximum_damage(id) > 0 {
        stack.damage = meta;
    } else {
        stack.meta = meta;
    }
    stack
}
fn enchanted(mut stack: Stack, entries: Vec<[u16; 2]>) -> Stack {
    if entries.is_empty() {
        return stack;
    }
    let name = if stack.id == 403 {
        "StoredEnchantments"
    } else {
        "ench"
    };
    let tags = entries
        .into_iter()
        .map(|[id, level]| {
            DropNbt::Compound(BTreeMap::from([
                ("id".to_owned(), DropNbt::Short(id as i16)),
                ("lvl".to_owned(), DropNbt::Short(level as i16)),
            ]))
        })
        .collect();
    stack = stack.with_nbt(DropNbt::Compound(BTreeMap::from([(
        name.to_owned(),
        DropNbt::List {
            element: 10,
            values: tags,
        },
    )])));
    stack
}
impl Rule {
    fn offer(self, random: &mut JavaRandom) -> Offer {
        let (cost, second, output) = match self {
            Self::Buy(id, meta, low, high) => (
                item(id, meta, price(random, low, high)),
                None,
                item(388, 0, 1),
            ),
            Self::Sell(id, meta, low, high) => {
                let n = price(random, low, high);
                if n < 0 {
                    (item(388, 0, 1), None, item(id, meta, -n))
                } else {
                    (item(388, 0, n), None, item(id, meta, 1))
                }
            }
            Self::Gear(id, meta, low, high) => {
                let cost = item(388, 0, price(random, low, high));
                let strength = 5 + random.next_int(15);
                let output = enchanted(item(id, meta, 1), loot::enchant_item(random, id, strength));
                (cost, None, output)
            }
            Self::Exchange(input, meta, low, high, output, out_meta, out_low, out_high) => {
                let cost = item(input, meta, price(random, low, high));
                let output = item(output, out_meta, price(random, out_low, out_high));
                (cost, Some(item(388, 0, 1)), output)
            }
            Self::Book => {
                let (id, low, high) = BOOK_POOL[random.next_int(BOOK_POOL.len() as i32) as usize];
                let level = price(random, low, high);
                let output = enchanted(item(403, 0, 1), vec![[id, level as u16]]);
                let emeralds = (2 + random.next_int(5 + level * 10) + 3 * level).min(64);
                (item(340, 0, 1), Some(item(388, 0, emeralds)), output)
            }
        };
        Offer {
            cost,
            second,
            output,
            uses: 0,
            max_uses: 7,
            reward_exp: true,
            supported: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Career {
    pub id: i32,
    pub level: i32,
    pub wealth: i32,
    pub willing: bool,
    pub random_state: u64,
    pub reset_ticks: i32,
    pub needs_initialization: bool,
    pub last_buyer: Option<String>,
    pub regeneration_ticks: i32,
    pub living_sound_time: i32,
    pub home_check_ticks: i32,
    pub home: Option<usize>,
    pub restriction:Option<(glam::IVec3,f32)>,
}
impl Default for Career {
    fn default() -> Self {
        Self::new(189)
    }
}
impl Career {
    pub fn new(seed: i64) -> Self {
        Self {
            id: 0,
            level: 0,
            wealth: 0,
            willing: false,
            random_state: JavaRandom::new(seed).state(),
            reset_ticks: 0,
            needs_initialization: false,
            last_buyer: None,
            regeneration_ticks: 0,
            living_sound_time: 0,
            home_check_ticks: 0,
            home: None,
            restriction:None,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct TradeEffects {
    pub experience: Option<i32>,
    pub sound: Option<(&'static str, f32, f32)>,
    pub regeneration: bool,
    pub reputation_player: Option<String>,
}
impl Trader {
    /// Entity.rand is seeded independently of world generation and the entity
    /// ID. The Entity constructor generates its UUID before the first trade.
    pub fn spawned(profession: u8) -> Self {
        let seed = JavaRandom::independent_seed();
        let mut local = JavaRandom::new(seed);
        local.next_long();
        local.next_long();
        let mut trader = Self::seeded(profession, seed);
        trader.career.random_state = local.state();
        trader
    }
    pub fn seeded(profession: u8, seed: i64) -> Self {
        Self {
            offers: Vec::new(),
            profession,
            initialized: false,
            career: Career::new(seed),
            custom_name: None,
            customer: None,
            pending: Vec::new(),
        }
    }
    pub fn ensure_offers(&mut self) -> Result<()> {
        if !self.initialized {
            self.populate()?;
        }
        Ok(())
    }
    pub fn populate(&mut self) -> Result<()> {
        let careers = CAREERS
            .get(self.profession as usize)
            .ok_or_else(|| anyhow::anyhow!("Unknown villager profession {}", self.profession))?;
        let mut random = JavaRandom::from_state(self.career.random_state);
        let (id, level) = if self.career.id == 0 || self.career.level == 0 {
            (random.next_int(careers.len() as i32) + 1, 1)
        } else {
            (self.career.id, self.career.level.wrapping_add(1))
        };
        ensure!(
            id > 0 && (id as usize) <= careers.len(),
            "Unknown career {id} for profession {}",
            self.profession
        );
        if level > 0 {
            if let Some(rules) = careers[id as usize - 1].get(level as usize - 1) {
                self.offers
                    .extend(rules.iter().map(|rule| rule.offer(&mut random)));
            }
        }
        self.career.id = id;
        self.career.level = level;
        self.initialized = true;
        self.career.random_state = random.state();
        Ok(())
    }
    pub fn title_key(&self) -> &'static str {
        match (self.profession, self.career.id) {
            (0, 1) => "entity.Villager.farmer",
            (0, 2) => "entity.Villager.fisherman",
            (0, 3) => "entity.Villager.shepherd",
            (0, 4) => "entity.Villager.fletcher",
            (1, _) => "entity.Villager.librarian",
            (2, _) => "entity.Villager.cleric",
            (3, 1) => "entity.Villager.armor",
            (3, 2) => "entity.Villager.weapon",
            (3, 3) => "entity.Villager.tool",
            (4, 1) => "entity.Villager.butcher",
            (4, 2) => "entity.Villager.leather",
            _ => "entity.Villager.name",
        }
    }
    pub fn use_recipe(&mut self, index: usize) -> TradeEffects {
        let offer = &mut self.offers[index];
        offer.uses = offer.uses.wrapping_add(1);
        self.career.living_sound_time = -80;
        let mut random = JavaRandom::from_state(self.career.random_state);
        let pitch = (random.next_float() - random.next_float()) * 0.2 + 1.;
        let mut xp = 3 + random.next_int(4);
        if offer.uses == 1 || random.next_int(5) == 0 {
            self.career.reset_ticks = 40;
            self.career.needs_initialization = true;
            self.career.willing = true;
            self.career.last_buyer = self.customer.clone();
            xp += 5;
        }
        if offer.cost.id == 388 {
            self.career.wealth = self.career.wealth.wrapping_add(i32::from(offer.cost.count));
        }
        self.career.random_state = random.state();
        TradeEffects {
            experience: offer.reward_exp.then_some(xp),
            sound: Some(("mob.villager.yes", 1., pitch)),
            ..Default::default()
        }
    }
    /// Original updateAITasks restock callback. The village owner consumes the
    /// returned reputation/status event; GUI open/close controls customer.
    pub fn tick_restock(&mut self, village_member: bool) -> Result<TradeEffects> {
        let mut result = TradeEffects::default();
        if self.customer.is_some() || self.career.reset_ticks <= 0 {
            return Ok(result);
        }
        self.career.reset_ticks -= 1;
        if self.career.reset_ticks != 0 {
            return Ok(result);
        }
        if self.career.needs_initialization {
            let mut random = JavaRandom::from_state(self.career.random_state);
            for offer in &mut self.offers {
                if offer.uses >= offer.max_uses {
                    offer.max_uses = offer
                        .max_uses
                        .wrapping_add(random.next_int(6) + random.next_int(6) + 2);
                }
            }
            self.career.random_state = random.state();
            self.populate()?;
            self.career.needs_initialization = false;
            if village_member {
                result.reputation_player = self.career.last_buyer.clone();
            }
        }
        self.career.regeneration_ticks = 200;
        result.regeneration = true;
        Ok(result)
    }
    pub fn tick_ambient(&mut self, child: bool) -> Option<(&'static str, f32, f32)> {
        let mut random = JavaRandom::from_state(self.career.random_state);
        let roll = random.next_int(1000);
        let previous = self.career.living_sound_time;
        self.career.living_sound_time = previous.wrapping_add(1);
        let sound = (roll < previous).then(|| {
            self.career.living_sound_time = -80;
            let pitch =
                (random.next_float() - random.next_float()) * 0.2 + if child { 1.5 } else { 1. };
            (
                if self.customer.is_some() {
                    "mob.villager.haggle"
                } else {
                    "mob.villager.idle"
                },
                1.,
                pitch,
            )
        });
        self.career.random_state = random.state();
        sound
    }
    /// EntityVillager.updateAITasks follows general goal updates/navigation.
    pub fn check_home(&mut self) -> bool {
        self.career.home_check_ticks = self.career.home_check_ticks.wrapping_sub(1);
        if self.career.home_check_ticks <= 0 {
            let mut random = JavaRandom::from_state(self.career.random_state);
            self.career.home_check_ticks = 70 + random.next_int(50);
            self.career.random_state = random.state();
            true
        } else {
            false
        }
    }
    pub fn verify_selling_item(&mut self, present: bool) -> Option<(&'static str, f32, f32)> {
        if self.career.living_sound_time <= -60 {
            return None;
        }
        self.career.living_sound_time = -80;
        let mut random = JavaRandom::from_state(self.career.random_state);
        let pitch = (random.next_float() - random.next_float()) * 0.2 + 1.;
        self.career.random_state = random.state();
        Some((
            if present {
                "mob.villager.yes"
            } else {
                "mob.villager.no"
            },
            1.,
            pitch,
        ))
    }
}
#[cfg(test)]
#[path = "villager_career_tests.rs"]
mod tests;
