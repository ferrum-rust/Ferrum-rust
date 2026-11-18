//! Original durability and armor-enchantment callbacks. The player, world and
//! EnchantmentHelper RNG streams are distinct, including visual break draws.
use super::random::JavaRandom;
use serde::{Deserialize, Serialize};
include!("armor_data.rs");

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub id: u16,
    pub count: i32,
    pub damage: i32,
    pub unbreakable: bool,
    /// Original list order and duplicate/unknown ids are significant.
    pub enchantments: Vec<[i16; 2]>,
}
impl Item {
    pub fn level(&self, id: i16) -> i32 {
        self.enchantments
            .iter()
            .find(|e| e[0] == id)
            .map_or(0, |e| i32::from(e[1]))
    }
}
pub fn maximum_damage(id: u16) -> i32 {
    ITEMS.iter().find(|r| r.0 == id).map_or(0, |r| r.1)
}
pub fn armor_points(id: u16) -> i32 {
    ITEMS.iter().find(|r| r.0 == id).map_or(0, |r| r.3)
}
pub fn is_armor(id: u16) -> bool {
    ITEMS.iter().find(|r| r.0 == id).is_some_and(|r| r.2)
}
#[derive(Clone, Debug)]
pub struct Broken {
    pub id: u16,
    pub count: i32,
    pub damage: i32,
    pub pitch: f32,
}
pub fn attempt_damage(item: &mut Item, mut amount: i32, entity: &mut JavaRandom) -> bool {
    let maximum = maximum_damage(item.id);
    if maximum <= 0 || item.unbreakable {
        return false;
    }
    if amount > 0 {
        let level = item.level(34);
        let mut negated = 0;
        if level > 0 {
            for _ in 0..amount {
                if is_armor(item.id) && entity.next_float() < 0.6 {
                    continue;
                }
                if entity.next_int(level + 1) > 0 {
                    negated += 1;
                }
            }
        }
        amount -= negated;
        if amount <= 0 {
            return false;
        }
    }
    item.damage = item.damage.wrapping_add(amount);
    item.damage > maximum
}
pub fn damage_item(
    item: &mut Item,
    amount: i32,
    creative: bool,
    entity: &mut JavaRandom,
    world: &mut JavaRandom,
) -> Option<Broken> {
    if creative || !attempt_damage(item, amount, entity) {
        return None;
    }
    let broken = Broken {
        id: item.id,
        count: item.count,
        damage: item.damage,
        pitch: 0.8f32 + world.next_float() * 0.4f32,
    };
    // Five crack particles consume three entity floats each. Their Math.random
    // vertical component belongs to a separate process-global visual stream.
    for _ in 0..15 {
        entity.next_float();
    }
    item.count = item.count.wrapping_sub(1).max(0);
    item.damage = 0;
    Some(broken)
}
pub fn damage_armor(
    items: &mut [Option<Item>; 4],
    amount: f32,
    creative: bool,
    entity: &mut JavaRandom,
    world: &mut JavaRandom,
) -> Vec<Broken> {
    let amount = amount / 4.;
    let amount = if amount < 1. { 1. } else { amount } as i32;
    let mut broken = Vec::new();
    for slot in items {
        if let Some(item) = slot.as_mut().filter(|item| is_armor(item.id)) {
            if let Some(event) = damage_item(item, amount, creative, entity, world) {
                broken.push(event);
            }
            if item.count == 0 {
                *slot = None;
            }
        }
    }
    broken
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Source {
    pub creative_harm: bool,
    pub fire: bool,
    pub fall: bool,
    pub explosion: bool,
    pub projectile: bool,
    pub unblockable: bool,
    pub absolute: bool,
    pub drowning: bool,
    pub falling_object: bool,
    pub has_attacker: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct DefenseContext {
    pub source: Source,
    pub creative: bool,
    pub blocking: bool,
    pub resistance: Option<i32>,
    pub absorption: f32,
}
#[derive(Clone, Debug)]
pub struct Defended {
    pub health_damage: f32,
    pub absorption: f32,
    pub breaks: Vec<Broken>,
}
pub fn defend(
    items: &mut [Option<Item>; 4],
    mut amount: f32,
    context: DefenseContext,
    entity: &mut JavaRandom,
    world: &mut JavaRandom,
    enchantment: &mut JavaRandom,
) -> Defended {
    let source = context.source;
    let mut breaks = Vec::new();
    if !source.unblockable {
        if context.blocking && amount > 0. {
            amount = (1. + amount) * 0.5;
        }
        // Armor points are read BEFORE durability wear; protective enchantments
        // are read AFTER it, so a piece breaking here loses protection this hit.
        let points = items
            .iter()
            .flatten()
            .map(|i| armor_points(i.id))
            .sum::<i32>();
        let scaled = amount * (25 - points) as f32;
        breaks = damage_armor(items, amount, context.creative, entity, world);
        amount = scaled / 25.;
    }
    amount = super::potions::resistance_damage(
        amount,
        context.resistance,
        source.absolute,
        source.creative_harm,
    );
    if !source.absolute && (amount > 0. || amount.is_nan()) {
        let modifier = protection(
            items.iter().flatten().map(|i| i.enchantments.as_slice()),
            source,
            enchantment,
        )
        .min(20);
        if modifier > 0 {
            amount = amount * (25 - modifier) as f32 / 25.;
        }
    }
    let remainder = amount - context.absorption;
    let health_damage = if remainder.is_nan() || remainder > 0. {
        remainder
    } else {
        0.
    };
    let absorption = context.absorption - (amount - health_damage);
    Defended {
        health_damage,
        absorption: if absorption < 0. { 0. } else { absorption },
        breaks,
    }
}
pub fn protection_modifier(id: i16, level: i16, source: Source) -> i32 {
    if source.creative_harm {
        return 0;
    }
    let level = i32::from(level);
    let base = 6i32.wrapping_add(level.wrapping_mul(level)) as f32 / 3.;
    let scale = match id {
        0 => 0.75f32,
        1 if source.fire => 1.25,
        2 if source.fall => 2.5,
        3 if source.explosion => 1.5,
        4 if source.projectile => 1.5,
        _ => return 0,
    };
    let value = base * scale;
    let truncated = value as i32;
    if value < truncated as f32 {
        truncated.wrapping_sub(1)
    } else {
        truncated
    }
}
pub fn protection<'a>(
    enchantments: impl IntoIterator<Item = &'a [[i16; 2]]>,
    source: Source,
    random: &mut JavaRandom,
) -> i32 {
    let mut sum = 0i32;
    for item in enchantments {
        for e in item {
            sum = sum.wrapping_add(protection_modifier(e[0], e[1], source));
        }
    }
    sum = sum.clamp(0, 25);
    ((sum + 1) >> 1) + random.next_int((sum >> 1) + 1)
}

#[cfg(test)]
#[path = "armor_tests.rs"]
mod tests;
