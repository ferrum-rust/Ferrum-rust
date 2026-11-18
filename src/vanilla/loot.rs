//! Original weighted chest rolls and level-30 enchanted books.
use super::random::JavaRandom;
use serde::{Deserialize, Serialize};

struct Enchantment {
    id: u16,
    weight: i32,
    ranges: &'static [(i32, i32)],
    incompatible: &'static [u16],
}
include!("mineshaft_enchantments.rs");
include!("item_enchant_data.rs");

pub(super) fn enchantment_weight(id: i16) -> Option<i32> {
    ENCHANTMENTS.iter().find(|e| i32::from(e.id) == i32::from(id)).map(|e| e.weight)
}


#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LootStack {
    pub id: u16,
    pub meta: u16,
    pub count: u8,
    pub enchants: Vec<[u16; 2]>,
}
#[derive(Clone, Debug)]
pub struct LootEntry {
    pub id: u16,
    pub meta: u16,
    pub min: i32,
    pub max: i32,
    pub weight: i32,
    pub enchants: Vec<[u16; 2]>,
}
impl LootEntry {
    pub fn new(id: u16, meta: u16, min: i32, max: i32, weight: i32) -> Self {
        Self {
            id,
            meta,
            min,
            max,
            weight,
            enchants: Vec::new(),
        }
    }
    pub fn book(random: &mut JavaRandom) -> Self {
        Self {
            id: 403,
            meta: 0,
            min: 1,
            max: 1,
            weight: 1,
            enchants: enchanted_book(random),
        }
    }
}

pub fn fill_loot(
    random: &mut JavaRandom,
    entries: &[LootEntry],
    rolls: i32,
    size: usize,
) -> Vec<Option<LootStack>> {
    let total: i32 = entries.iter().map(|e| e.weight).sum();
    let mut slots = vec![None; size];
    for _ in 0..rolls {
        let mut choice = random.next_int(total);
        let entry = entries
            .iter()
            .find(|e| {
                choice -= e.weight;
                choice < 0
            })
            .unwrap();
        let count = entry.min + random.next_int(entry.max - entry.min + 1);
        let limit = crate::inventory::stack_limit(entry.id) as i32;
        let (copies, count) = if count <= limit {
            (1, count)
        } else {
            (count, 1)
        };
        for _ in 0..copies {
            let slot = random.next_int(size as i32) as usize;
            slots[slot] = Some(LootStack {
                id: entry.id,
                meta: entry.meta,
                count: count as u8,
                enchants: entry.enchants.clone(),
            });
        }
    }
    slots
}

/// EnchantmentHelper.buildEnchantmentList(book,30). Java's HashMap bucket order
/// is part of weighted selection, including its pre-removal table capacity.
pub fn enchanted_book(random: &mut JavaRandom) -> Vec<[u16; 2]> {
    enchant_item(random, 340, 30)
}

/// EnchantmentHelper.buildEnchantmentList for the actual item's enchantability
/// and the original enchantment type predicate. Books accept the full pool.
pub fn enchant_item(random: &mut JavaRandom, item: u16, requested: i32) -> Vec<[u16; 2]> {
    let Some((_, enchantability, applicable)) = ITEM_ENCHANT.iter().find(|row| row.0 == item)
    else {
        return Vec::new();
    };
    let bound = ((enchantability / 2) >> 1) + 1;
    let adjusted = requested.wrapping_add(1 + random.next_int(bound) + random.next_int(bound));
    let variation = (random.next_float() + random.next_float() - 1.0) * 0.15;
    let power = ((adjusted as f32 * (1.0 + variation) + 0.5) as i32).max(1);
    let mut candidates: Vec<(&Enchantment, u16)> = ENCHANTMENTS
        .iter()
        .filter(|e| applicable.contains(&e.id))
        .filter_map(|e| {
            e.ranges
                .iter()
                .enumerate()
                .rfind(|(_, (min, max))| power >= *min && power <= *max)
                .map(|(level, _)| (e, level as u16 + 1))
        })
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }
    let mut capacity = 16;
    while candidates.len() > capacity * 3 / 4 {
        capacity *= 2;
    }
    candidates.sort_by_key(|(e, _)| (e.id as usize & (capacity - 1), e.id));
    fn select(random: &mut JavaRandom, entries: &[(&Enchantment, u16)]) -> [u16; 2] {
        let mut choice = random.next_int(entries.iter().map(|(e, _)| e.weight).sum());
        let (e, level) = entries
            .iter()
            .find(|(e, _)| {
                choice -= e.weight;
                choice < 0
            })
            .unwrap();
        [e.id, *level]
    }
    let mut result = vec![select(random, &candidates)];
    let mut chance = power;
    while random.next_int(50) <= chance {
        candidates.retain(|(e, _)| {
            result.iter().all(|[id, _]| {
                !ENCHANTMENTS
                    .iter()
                    .find(|e| e.id == *id)
                    .unwrap()
                    .incompatible
                    .contains(&e.id)
            })
        });
        if !candidates.is_empty() {
            result.push(select(random, &candidates));
        }
        chance >>= 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn level_30_books_and_rng_match_original_registry() {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-mineshafts.json"
        ))
        .unwrap();
        for row in data["books"].as_array().unwrap() {
            let seed: i64 = row["seed"].as_str().unwrap().parse().unwrap();
            let mut random = JavaRandom::new(seed);
            assert_eq!(
                serde_json::to_value(enchanted_book(&mut random)).unwrap(),
                row["enchants"],
                "book seed {seed}"
            );
            assert_eq!(
                random.next_long().to_string(),
                row["next_long"].as_str().unwrap(),
                "book RNG {seed}"
            );
        }
    }
}
