//! Persist additional fields supported by the integrated command server.
use crate::{
    gameplay::{Mob, MobKind, Simulation},
    inventory::Stack,
    vanilla::{
        drops::DropNbt,
        item_food::Effect,
        potions::{Change, Effects, Target},
    },
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub mobs: BTreeMap<u64, MobData>,
    pub ender: BTreeMap<i32, Option<Stack>>,
}
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MobData {
    pub name: String,
    pub silent: bool,
    pub no_ai: bool,
    pub invulnerable: bool,
    pub persistent: bool,
    pub effects: Effects,
    pub equipment: BTreeMap<i32, Option<Stack>>,
    pub absorption: f32,
    pub pitch: f32,
    pub extra: BTreeMap<String, DropNbt>,
}
pub enum EffectEdit {
    Add(Effect),
    Remove(u8),
    Clear,
    Tick,
}
impl EffectEdit {
    fn apply(self, effects: &mut Effects, target: &mut impl Target) {
        match self {
            Self::Add(e) => {
                effects.add(e, target).expect("registered potion");
            }
            Self::Remove(id) => effects.remove(id, target),
            Self::Clear => effects.clear(target),
            Self::Tick => effects.tick(target),
        }
    }
}
impl Simulation {
    pub fn command_player_effect(&mut self, edit: EffectEdit) {
        let mut effects = std::mem::take(&mut self.effects);
        let resistance = effects.get(11).map(|e| e.amplifier);
        edit.apply(
            &mut effects,
            &mut super::FoodPotionTarget {
                sim: self,
                resistance,
            },
        );
        self.effects = effects;
    }
    pub fn apply_local_mode(&mut self, id: u8, player: &mut crate::player::Player) {
        self.commands.player_mode = Some(id);
        player.set_spectator(id == 3);
        self.mode = if matches!(id, 1 | 3) {
            super::Mode::Creative
        } else {
            super::Mode::Survival
        };
        self.fall_distance = 0.;
        player.allow_flying = matches!(id, 1 | 3);
        player.flying = id == 3 || id == 1 && player.flying;
        player.can_sprint = player.allow_flying || self.food > 6;
    }
}
struct MobTarget<'a> {
    mob: &'a mut Mob,
    absorption: &'a mut f32,
    max: f32,
    base: f32,
    wither_skeleton: bool,
}
impl Target for MobTarget<'_> {
    fn remote(&self) -> bool {
        false
    }
    fn health(&self) -> f32 {
        self.mob.health
    }
    fn max_health(&self) -> f32 {
        self.max
    }
    fn undead(&self) -> bool {
        matches!(self.mob.kind, MobKind::Zombie | MobKind::Skeleton | MobKind::PigZombie | MobKind::Wither)
    }
    fn effect_immune(&self, id: u8) -> bool {
        (matches!(self.mob.kind, MobKind::Spider | MobKind::CaveSpider) && id == 19)
            || ((self.mob.kind == MobKind::Wither
                || (self.mob.kind == MobKind::Skeleton && self.wither_skeleton))
                && id == 20)
    }
    fn heal(&mut self, v: f32) {
        if self.mob.health > 0. {
            self.mob.health = (self.mob.health + v).min(self.max)
        }
    }
    fn attack(&mut self, _: bool, v: f32) {
        let shield = v.min(*self.absorption);
        *self.absorption -= shield;
        self.mob.health = (self.mob.health - v + shield).max(0.);
    }
    fn exhaustion(&mut self, _: f32) {}
    fn saturation(&mut self, _: i32) {}
    fn changed(&mut self, e: Effect, c: Change) {
        if e.id == 22 {
            if matches!(c, Change::Removed | Change::Changed(true)) {
                *self.absorption =
                    crate::vanilla::potions::absorption_attribute(*self.absorption, e.amplifier, false);
            }
            if matches!(c, Change::Added | Change::Changed(true)) {
                *self.absorption =
                    crate::vanilla::potions::absorption_attribute(*self.absorption, e.amplifier, true);
            }
        }
        if e.id == 21 && c == Change::Removed {
            self.mob.health = self.mob.health.min(self.base)
        }
    }
}
impl MobData {
    pub fn effect(&mut self, mob: &mut Mob, edit: EffectEdit) {
        let base = mob.kind.max_health(Some(self));
        let max = base
            + self
                .effects
                .get(21)
                .map_or(0., |e| 4. * (e.amplifier + 1) as f32);
        let wither_skeleton =
            crate::vanilla::item_nbt::number(self.extra.get("SkeletonType")) == 1;
        edit.apply(
            &mut self.effects,
            &mut MobTarget {
                mob,
                absorption: &mut self.absorption,
                max,
                base,
                wither_skeleton,
            },
        );
    }
}
