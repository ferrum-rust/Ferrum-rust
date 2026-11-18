//! Original active-effect map traversal, combination and effect callbacks.
//! Combat, attributes and visual particles are explicit caller boundaries.
use super::item_food::Effect;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Added,
    Changed(bool),
    Removed,
}
pub trait Target {
    fn remote(&self) -> bool;
    fn health(&self) -> f32;
    fn max_health(&self) -> f32;
    fn undead(&self) -> bool {
        false
    }
    fn effect_immune(&self, _id: u8) -> bool {
        false
    }
    fn heal(&mut self, amount: f32);
    fn attack(&mut self, wither: bool, amount: f32);
    fn exhaustion(&mut self, amount: f32);
    fn saturation(&mut self, amount: i32);
    fn changed(&mut self, effect: Effect, change: Change);
}

/// Integer keys 1..23 cannot treeify. Preserve Java HashMap's bucket capacity,
/// insertion chains and resize splits: removing effects never shrinks it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Effects {
    buckets: Vec<Vec<Effect>>,
}
/// EntityPlayerSP overrides both heal and attackEntityFrom as no-ops; its
/// remote world also gates exhaustion/saturation and attribute callbacks.
struct ClientTarget;
impl Target for ClientTarget {
    fn remote(&self) -> bool {
        true
    }
    fn health(&self) -> f32 {
        20.
    }
    fn max_health(&self) -> f32 {
        20.
    }
    fn heal(&mut self, _: f32) {}
    fn attack(&mut self, _: bool, _: f32) {}
    fn exhaustion(&mut self, _: f32) {}
    fn saturation(&mut self, _: i32) {}
    fn changed(&mut self, _: Effect, _: Change) {}
}
impl Effects {
    pub fn add_client(&mut self, effect: Effect) -> Result<()> {
        self.add(effect, &mut ClientTarget)
    }
    pub fn remove_client(&mut self, id: u8) {
        self.remove(id, &mut ClientTarget);
    }
    pub fn tick_client(&mut self) {
        self.tick(&mut ClientTarget);
    }
    /// EntityLivingBase NBT load uses Map.put, without combine or attribute
    /// callbacks; absorption and serialized attribute modifiers load separately.
    pub fn load_effect(&mut self, effect: Effect) -> Result<()> {
        ensure!(
            (1..=23).contains(&effect.id),
            "Unknown original potion {}",
            effect.id
        );
        if self.buckets.is_empty() {
            self.buckets = vec![Vec::new(); 16];
        }
        let bucket = effect.id as usize & (self.buckets.len() - 1);
        if let Some(old) = self.buckets[bucket].iter_mut().find(|e| e.id == effect.id) {
            *old = effect;
        } else {
            self.buckets[bucket].push(effect);
            if self.ordered().count() > self.buckets.len() * 3 / 4 {
                let old = std::mem::take(&mut self.buckets);
                self.buckets = vec![Vec::new(); old.len() * 2];
                let mask = self.buckets.len() - 1;
                for effect in old.into_iter().flatten() {
                    self.buckets[effect.id as usize & mask].push(effect);
                }
            }
        }
        Ok(())
    }
    pub fn get(&self, id: u8) -> Option<Effect> {
        if self.buckets.is_empty() {
            return None;
        }
        self.buckets[id as usize & (self.buckets.len() - 1)]
            .iter()
            .find(|e| e.id == id)
            .copied()
    }
    pub fn ordered(&self) -> impl Iterator<Item = Effect> + '_ {
        self.buckets.iter().flat_map(|b| b.iter().copied())
    }
    pub fn add(&mut self, mut effect: Effect, target: &mut impl Target) -> Result<()> {
        ensure!(
            (1..=23).contains(&effect.id),
            "Unknown original potion {}",
            effect.id
        );
        if (target.undead() && matches!(effect.id, 10 | 19)) || target.effect_immune(effect.id) {
            return Ok(());
        }
        if self.buckets.is_empty() {
            self.buckets = vec![Vec::new(); 16];
        }
        let bucket = effect.id as usize & (self.buckets.len() - 1);
        if let Some(old) = self.buckets[bucket].iter_mut().find(|e| e.id == effect.id) {
            old.combine(effect);
            effect = *old;
            target.changed(effect, Change::Changed(true));
        } else {
            self.buckets[bucket].push(effect);
            if self.ordered().count() > self.buckets.len() * 3 / 4 {
                let old = std::mem::take(&mut self.buckets);
                self.buckets = vec![Vec::new(); old.len() * 2];
                let mask = self.buckets.len() - 1;
                for effect in old.into_iter().flatten() {
                    self.buckets[effect.id as usize & mask].push(effect);
                }
            }
            target.changed(effect, Change::Added);
        }
        Ok(())
    }
    pub fn remove(&mut self, id: u8, target: &mut impl Target) {
        if self.buckets.is_empty() {
            return;
        }
        let bucket = id as usize & (self.buckets.len() - 1);
        if let Some(i) = self.buckets[bucket].iter().position(|e| e.id == id) {
            let effect = self.buckets[bucket].remove(i);
            target.changed(effect, Change::Removed);
        }
    }
    pub fn clear(&mut self, target: &mut impl Target) {
        if target.remote() {
            return;
        }
        let ids = self.ordered().map(|e| e.id).collect::<Vec<_>>();
        for id in ids {
            self.remove(id, target);
        }
    }
    pub fn tick(&mut self, target: &mut impl Target) {
        for bucket in &mut self.buckets {
            let mut i = 0;
            while i < bucket.len() {
                let effect = &mut bucket[i];
                if effect.duration > 0 {
                    if ready(*effect) {
                        perform(*effect, target);
                    }
                    effect.duration -= 1;
                }
                if effect.duration <= 0 {
                    if !target.remote() {
                        target.changed(bucket.remove(i), Change::Removed);
                        continue;
                    }
                } else if effect.duration % 600 == 0 {
                    target.changed(*effect, Change::Changed(false));
                }
                i += 1;
            }
        }
    }
}
pub fn read_effect(tag: &crate::nbt::Tag) -> Option<Effect> {
    use crate::nbt::Tag;
    let int = |name| super::food::nbt_int(tag.get(name));
    let id = int("Id") as i8;
    if !(1..=23).contains(&id) {
        return None;
    }
    Some(Effect {
        id: id as u8,
        duration: int("Duration"),
        amplifier: i32::from(int("Amplifier") as i8),
        ambient: int("Ambient") as i8 != 0,
        show_particles: match tag.get("ShowParticles") {
            Some(Tag::Byte(v)) => *v != 0,
            _ => true,
        },
    })
}
pub fn ready(effect: Effect) -> bool {
    let interval = match effect.id {
        10 => 50i32,
        19 => 25,
        20 => 40,
        6 | 7 | 23 => return effect.duration >= 1,
        17 => return true,
        _ => return false,
    } >> (effect.amplifier & 31);
    interval <= 0 || effect.duration % interval == 0
}
fn perform(effect: Effect, target: &mut impl Target) {
    let amp = effect.amplifier;
    match effect.id {
        10 if target.health() < target.max_health() => target.heal(1.),
        19 if target.health() > 1. => target.attack(false, 1.),
        20 => target.attack(true, 1.),
        17 => target.exhaustion(0.025f32 * amp.wrapping_add(1) as f32),
        23 if !target.remote() => target.saturation(amp.wrapping_add(1)),
        6 | 7 => {
            if (effect.id == 6) != target.undead() {
                target.heal(4i32.wrapping_shl(amp as u32).max(0) as f32);
            } else {
                target.attack(false, 6i32.wrapping_shl(amp as u32) as f32);
            }
        }
        _ => {}
    }
}
pub fn absorption_attribute(current: f32, amplifier: i32, apply: bool) -> f32 {
    let amount = 4i32.wrapping_mul(amplifier.wrapping_add(1)) as f32;
    let value = if apply {
        current + amount
    } else {
        current - amount
    };
    if value < 0. {
        0.
    } else {
        value
    }
}
/// Potion resistance stage, before enchantment protection and absorption.
pub fn resistance_damage(
    amount: f32,
    amplifier: Option<i32>,
    absolute: bool,
    out_of_world: bool,
) -> f32 {
    if absolute {
        return amount;
    }
    let amount = if let Some(amp) = amplifier.filter(|_| !out_of_world) {
        amount * 25i32.wrapping_sub(amp.wrapping_add(1).wrapping_mul(5)) as f32 / 25.
    } else {
        amount
    };
    if amount <= 0. {
        0.
    } else {
        amount
    }
}

#[cfg(test)]
#[path = "potions_tests.rs"]
mod tests;
