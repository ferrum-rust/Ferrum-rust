//! Original dropped item initialization, stack merging and player pickup.
use super::{armor, movement::Aabb, random::JavaRandom};
use crate::inventory::{Inventory, Stack};
use anyhow::{ensure, Result};
use glam::DVec3;
use serde::{Deserialize, Serialize};

include!("item_entity_data.rs");
#[path = "item_motion.rs"]
mod motion;
#[allow(unused_imports)]
pub use motion::{push_out_of_blocks, ItemEffect, ItemWorld};
/// EntityPlayer.dropItem evaluates the eye offset in this order using double positions.
pub(crate) fn throw_position(feet:DVec3, eye_height:f32)->DVec3 {
    DVec3::new(feet.x,feet.y-f64::from(0.3f32)+f64::from(eye_height),feet.z)
}
pub(crate) fn throw_velocity(yaw: f32, pitch: f32, r: &mut JavaRandom) -> DVec3 {
    use crate::vanilla::math::{cos, sin};
    let yaw = yaw / 180. * std::f32::consts::PI;
    let pitch = pitch / 180. * std::f32::consts::PI;
    let direction = DVec3::new(
        f64::from(-sin(yaw) * cos(pitch) * 0.3),
        f64::from(-sin(pitch) * 0.3 + 0.1),
        f64::from(cos(yaw) * cos(pitch) * 0.3),
    );
    let angle = f64::from(r.next_float() * std::f32::consts::PI * 2.);
    let strength = f64::from(0.02 * r.next_float());
    direction
        + DVec3::new(
            angle.cos() * strength,
            f64::from((r.next_float() - r.next_float()) * 0.1),
            angle.sin() * strength,
        )
}
const fn build_facts() -> [u32; 2268] {
    let mut result = [0; 2268];
    let mut i = 0;
    while i < ITEM_FACTS.len() {
        let f = ITEM_FACTS[i];
        result[f[0] as usize] = f[1] as u32 | (f[2] as u32) << 7 | (f[3] as u32) << 8;
        i += 1;
    }
    result
}
const FACTS: [u32; 2268] = build_facts();
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemFacts {
    pub max_count: u8,
    pub has_subtypes: bool,
    pub max_damage: u16,
}
pub fn facts(id: u16) -> Option<ItemFacts> {
    let value = *FACTS.get(id as usize)?;
    (value != 0).then_some(ItemFacts {
        max_count: (value & 127) as u8,
        has_subtypes: value & 128 != 0,
        max_damage: (value >> 8) as u16,
    })
}
pub fn stack_from_metadata(id: u16, count: u8, metadata: u16) -> Result<Stack> {
    let props = facts(id).ok_or_else(|| anyhow::anyhow!("Unknown original item ID {id}"))?;
    let mut stack = Stack::new(id, count);
    if props.max_damage > 0 {
        stack.damage = metadata;
    } else {
        stack.meta = metadata;
    }
    Ok(stack)
}
fn metadata(s: &Stack) -> u16 {
    if armor::maximum_damage(s.id) > 0 {
        s.damage
    } else {
        s.meta
    }
}
fn damaged(s: &Stack) -> bool {
    armor::maximum_damage(s.id) > 0 && s.damage > 0 && !s.unbreakable()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ItemEntity {
    #[serde(default)]
    pub section: Option<[i32; 3]>,
    pub id: u64,
    pub stack: Stack,
    pub position: DVec3,
    pub previous: DVec3,
    pub velocity: DVec3,
    pub bounds: Aabb,
    pub age: i32,
    pub pickup_delay: i32,
    pub health: i32,
    pub ticks: i32,
    pub yaw: f32,
    pub hover_start: f32,
    pub owner: Option<String>,
    pub thrower: Option<String>,
    pub grounded: bool,
    pub clipped_horizontal: bool,
    pub clipped_vertical: bool,
    pub in_web: bool,
    pub in_water: bool,
    pub no_clip: bool,
    pub first_update: bool,
    pub silent: bool,
    pub dead: bool,
    pub fall_distance: f32,
    pub fire: i32,
    pub random_state: u64,
}
impl ItemEntity {
    pub fn new(
        id: u64,
        position: DVec3,
        stack: Stack,
        math: &mut JavaRandom,
        random_state: u64,
    ) -> Self {
        let hover_start = (math.next_double() * std::f64::consts::PI * 2.) as f32;
        let yaw = (math.next_double() * 360.) as f32;
        let mut horizontal = || (math.next_double() * 0.2f32 as f64 - 0.1f32 as f64) as f32 as f64;
        let velocity = DVec3::new(horizontal(), 0.2f32 as f64, horizontal());
        Self {
            section: None,
            id,
            stack,
            position,
            previous: DVec3::ZERO,
            velocity,
            bounds: Aabb {
                min: position - DVec3::new(0.125, 0., 0.125),
                max: position + DVec3::new(0.125, 0.25, 0.125),
            },
            age: 0,
            pickup_delay: 0,
            health: 5,
            ticks: 0,
            yaw,
            hover_start,
            owner: None,
            thrower: None,
            grounded: false,
            clipped_horizontal: false,
            clipped_vertical: false,
            in_web: false,
            in_water: false,
            no_clip: false,
            first_update: true,
            silent: false,
            dead: false,
            fall_distance: 0.,
            fire: 0,
            random_state,
        }
    }
    pub fn combine(a: &mut Self, b: &mut Self) -> bool {
        if a.id == b.id
            || a.dead
            || b.dead
            || a.pickup_delay == 32767
            || b.pickup_delay == 32767
            || a.age == -32768
            || b.age == -32768
            || a.stack.id != b.stack.id
            || !a.stack.same_tags(&b.stack)
        {
            return false;
        }
        let Some(props) = facts(a.stack.id) else {
            return false;
        };
        if props.has_subtypes && metadata(&a.stack) != metadata(&b.stack) {
            return false;
        }
        if b.stack.count < a.stack.count {
            return Self::combine(b, a);
        }
        let combined = u16::from(a.stack.count) + u16::from(b.stack.count);
        if combined > u16::from(props.max_count) {
            return false;
        }
        b.stack.count = combined as u8;
        b.pickup_delay = b.pickup_delay.max(a.pickup_delay);
        b.age = b.age.min(a.age);
        a.dead = true;
        true
    }
    pub fn pickup(
        &mut self,
        inventory: &mut Inventory,
        player: &str,
        creative: bool,
        thrower_online: bool,
    ) -> Result<Pickup> {
        let mut result = Pickup::default();
        if self.dead
            || self.pickup_delay != 0
            || self
                .owner
                .as_deref()
                .is_some_and(|o| o != player && 6000i32.wrapping_sub(self.age) > 200)
        {
            return Ok(result);
        }
        let original = self.stack.count;
        let announce = add_to_player(inventory, &mut self.stack, creative)?;
        result.removed_count = original - self.stack.count;
        if !announce {
            return Ok(result);
        }
        let achievement = match self.stack.id {
            17 | 162 => Some("achievement.mineWood"),
            334 => Some("achievement.killCow"),
            264 => Some("achievement.diamonds"),
            369 => Some("achievement.blazeRod"),
            _ => None,
        };
        if let Some(a) = achievement {
            result.stats.push((player.to_owned(), a.to_owned()));
        }
        if self.stack.id == 264 && thrower_online {
            if let Some(name) = self.thrower.as_deref().filter(|name| *name != player) {
                result
                    .stats
                    .push((name.into(), "achievement.diamondsToYou".into()));
            }
        }
        if !self.silent {
            let mut r = JavaRandom::from_state(self.random_state);
            let pitch = ((r.next_float() - r.next_float()) * 0.7 + 1.) * 2.;
            self.random_state = r.state();
            result.sound = Some((0.2, pitch));
        }
        result.announced_count = Some(original);
        if self.stack.count == 0 {
            self.dead = true;
        }
        Ok(result)
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Pickup {
    pub removed_count: u8,
    pub announced_count: Option<u8>,
    pub sound: Option<(f32, f32)>,
    pub stats: Vec<(String, String)>,
}

/// InventoryPlayer.addItemStackToInventory returns the final store attempt's
/// result. A partial insertion can change slots while suppressing pickup sound.
pub(crate) fn add_to_player(inv: &mut Inventory, stack: &mut Stack, creative: bool) -> Result<bool> {
    if stack.count == 0 || stack.id == 0 {
        return Ok(false);
    }
    let props =
        facts(stack.id).ok_or_else(|| anyhow::anyhow!("Unknown pickup item {}", stack.id))?;
    ensure!(
        inv.slots.len() >= 36,
        "Player inventory lacks its 36 main slots"
    );
    let slots = &mut inv.slots[..36];
    if damaged(stack) {
        if let Some(empty) = slots.iter_mut().find(|s| s.is_none()) {
            *empty = Some(stack.copied());
            stack.count = 0;
            return Ok(true);
        }
        if creative {
            stack.count = 0;
            return Ok(true);
        }
        return Ok(false);
    }
    loop {
        let target = slots
            .iter()
            .position(|slot| {
                slot.as_ref().is_some_and(|s| {
                    s.id == stack.id
                        && props.max_count > 1
                        && !damaged(s)
                        && s.count < props.max_count
                        && s.count < 64
                        && (!props.has_subtypes || metadata(s) == metadata(stack))
                        && s.same_tags(stack)
                })
            })
            .or_else(|| slots.iter().position(Option::is_none));
        let Some(index) = target else {
            if creative {
                stack.count = 0;
                return Ok(true);
            }
            return Ok(false);
        };
        let dest = slots[index].get_or_insert_with(|| Stack {
            count: 0,
            ..stack.copied()
        });
        let moved = stack
            .count
            .min(props.max_count.saturating_sub(dest.count))
            .min(64u8.saturating_sub(dest.count));
        if moved == 0 {
            if creative {
                stack.count = 0;
                return Ok(true);
            }
            return Ok(false);
        }
        dest.count += moved;
        stack.count -= moved;
        if stack.count == 0 {
            return Ok(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vanilla::drops::DropNbt;
    use serde_json::{json, Value};
    use std::{collections::BTreeMap, io::Read, sync::OnceLock};
    fn fixture() -> &'static Value {
        static DATA: OnceLock<Value> = OnceLock::new();
        DATA.get_or_init(|| {
            let mut bytes = Vec::new();
            flate2::read::GzDecoder::new(
                &include_bytes!("../../tests/fixtures/vanilla189-item-entities.json.gz")[..],
            )
            .read_to_end(&mut bytes)
            .unwrap();
            serde_json::from_slice(&bytes).unwrap()
        })
    }
    fn stack(row: &Value) -> Option<Stack> {
        if row.is_null() {
            return None;
        }
        let mut s = stack_from_metadata(
            row[0].as_u64().unwrap() as u16,
            row[1].as_u64().unwrap() as u8,
            row[2].as_u64().unwrap() as u16,
        )
        .unwrap();
        let tag = row[3].as_i64().unwrap() as i32;
        if tag != 0 {
            s = s.with_nbt(DropNbt::Compound(BTreeMap::from([(
                "variant".into(),
                DropNbt::Int(tag),
            )])));
        }
        Some(s)
    }
    fn stack_row(s: &Stack) -> Value {
        let tag = s
            .nbt
            .as_deref()
            .and_then(crate::vanilla::item_nbt::compound)
            .and_then(|m| m.get("variant"));
        json!([
            s.id,
            s.count,
            metadata(s),
            crate::vanilla::item_nbt::number(tag)
        ])
    }
    fn entity(row: &Value, id: u64) -> ItemEntity {
        let mut e = ItemEntity::new(
            id,
            DVec3::new(8.5, 8., 8.5),
            stack(&row[0]).unwrap(),
            &mut JavaRandom::new(0),
            row[4].as_str().unwrap().parse().unwrap(),
        );
        e.age = row[1].as_i64().unwrap() as i32;
        e.pickup_delay = row[2].as_i64().unwrap() as i32;
        e.dead = row[3].as_bool().unwrap();
        e
    }
    fn entity_row(e: &ItemEntity) -> Value {
        json!([
            stack_row(&e.stack),
            e.age,
            e.pickup_delay,
            e.dead,
            e.random_state.to_string()
        ])
    }
    #[test]
    fn original_registry_and_entityitem_constructor_math_random_match() {
        assert_eq!(json!(ITEM_FACTS), fixture()["facts"]);
        for row in fixture()["constructors"].as_array().unwrap() {
            let mut math = JavaRandom::new(row[0].as_str().unwrap().parse().unwrap());
            let e = ItemEntity::new(0, DVec3::ZERO, Stack::new(1, 1), &mut math, 0);
            assert_eq!(
                json!([
                    row[0],
                    e.hover_start.to_bits(),
                    e.yaw.to_bits(),
                    e.velocity.to_array().map(|v| format!("{:x}", v.to_bits())),
                    math.state().to_string()
                ]),
                *row
            );
        }
    }
    #[test]
    fn original_entityitem_merges_preserve_target_order_delay_age_and_tags() {
        for row in fixture()["merges"].as_array().unwrap() {
            let mut a = entity(&row["before"][0], 0);
            let mut b = entity(&row["before"][1], 1);
            assert_eq!(json!(ItemEntity::combine(&mut a, &mut b)), row["merged"]);
            assert_eq!(json!([entity_row(&a), entity_row(&b)]), row["after"]);
        }
    }
    #[test]
    fn original_entityitem_player_pickup_inventory_sound_and_rng_match() {
        for (n, row) in fixture()["pickups"].as_array().unwrap().iter().enumerate() {
            let mut e = entity(&row["before"], 0);
            let name = row["owner"].as_str().unwrap();
            if !name.is_empty() {
                e.owner = Some(name.into());
            }
            let mut inv = Inventory::default();
            for (i, s) in row["slots"].as_array().unwrap().iter().enumerate() {
                inv.slots[i] = stack(s);
            }
            let pickup = e
                .pickup(
                    &mut inv,
                    "Player",
                    row["creative"].as_bool().unwrap(),
                    false,
                )
                .unwrap();
            let mut calls = pickup
                .stats
                .iter()
                .map(|(_, stat)| json!(["stat", stat, 1]))
                .collect::<Vec<_>>();
            if let Some(count) = pickup.announced_count {
                calls.push(json!(["pickup", count]));
            }
            let sounds = pickup
                .sound
                .map(|(volume, pitch)| {
                    json!(["sound", "random.pop", volume.to_bits(), pitch.to_bits()])
                })
                .into_iter()
                .collect::<Vec<_>>();
            assert_eq!(entity_row(&e), row["after"], "entity pickup {n}");
            assert_eq!(
                json!(inv.slots[..36]
                    .iter()
                    .map(|s| s.as_ref().map(stack_row))
                    .collect::<Vec<_>>()),
                row["result"],
                "inventory pickup {n}"
            );
            assert_eq!(json!(calls), row["calls"], "pickup callbacks {n}");
            assert_eq!(json!(sounds), row["sounds"], "pickup sounds {n}");
        }
    }
}
