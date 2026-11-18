//! C07 and the original ItemInWorldManager break-progress state machine.
use super::*;
use crate::vanilla::{
    armor,
    drops::{DropStack, DropWorld},
    food, material, mining,
};
use serde::{Deserialize, Serialize};
const SIDES: [IVec3; 6] = [
    IVec3::NEG_Y,
    IVec3::Y,
    IVec3::NEG_Z,
    IVec3::Z,
    IVec3::NEG_X,
    IVec3::X,
];
#[cfg(test)]
#[path = "runtime_dig_tests.rs"]
mod tests;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Digging {
    pub clock: i32,
    pub active: bool,
    pub position: IVec3,
    pub initial: i32,
    pub finishing: bool,
    pub finish_position: IVec3,
    pub finish_initial: i32,
    pub stage: i32,
}
impl Default for Digging {
    fn default() -> Self {
        Self {
            clock: 0,
            active: false,
            position: IVec3::ZERO,
            initial: 0,
            finishing: false,
            finish_position: IVec3::ZERO,
            finish_initial: 0,
            stage: -1,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum DigAction {
    Start,
    Stop,
    Abort,
}
#[derive(Clone, Debug)]
pub struct DigBlock {
    pub stamp: InventoryStamp,
    pub position: [i32; 3],
    pub face: u8,
    pub action: DigAction,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DigOutcome {
    Accepted,
    Stale,
    NotReady,
    OutsideReach,
    UnsupportedBlock(u16),
}
#[derive(Clone, Debug)]
pub struct DigReceipt {
    pub outcome: DigOutcome,
    pub stamp: InventoryStamp,
    pub corrections: Vec<([i32; 3], u16)>,
}
fn supported(state: u16) -> bool {
    let callbacks = mining::callbacks(state);
    matches!(
        callbacks[0].as_str(),
        "Block" | "BlockStairs" | "BlockRedstoneOre"
    ) && matches!(
        callbacks[1].as_str(),
        "Block" | "BlockDoor" | "BlockBed" | "BlockDoublePlant" | "BlockFlowerPot" | "BlockSkull"
    ) && (matches!(callbacks[2].as_str(), "Block" | "BlockStairs")
        || state >> 4 == 46 && state & 1 == 0)
        && matches!(
            callbacks[3].as_str(),
            "Block"
                | "BlockOldLeaf"
                | "BlockNewLeaf"
                | "BlockTallGrass"
                | "BlockDeadBush"
                | "BlockVine"
                | "BlockIce"
                | "BlockSnow"
                | "BlockBanner"
                | "BlockDoublePlant"
        )
}
impl Runtime {
    fn dig_rate(&self, state: u16) -> f32 {
        let b = &self.backend;
        let Some(f) = &b.entities.player else {
            return 0.;
        };
        let Some(p) = &b.player_inventory else {
            return 0.;
        };
        let c = mining::Conditions {
            grounded: f.grounded,
            eyes_in_water: f.eyes_in_water,
            aqua_affinity: p.inventory.armor_enchantment(6) > 0,
            haste: f.haste,
            fatigue: f.fatigue,
        };
        mining::increment(
            state,
            p.inventory.slots[p.inventory.selected].as_ref(),
            mining::hardness(state),
            c,
        )
    }
    fn crack(&mut self, p: IVec3, stage: i32) {
        self.backend
            .entities
            .effects
            .push(EntityEffect::BreakProgress {
                position: p.to_array(),
                stage,
            });
    }
    pub(super) fn tick_digging(&mut self) -> Result<()> {
        let d = &mut self.backend.entities.digging;
        d.clock = d.clock.wrapping_add(1);
        if !d.finishing && !d.active {
            return Ok(());
        }
        let (p, initial, finishing) = if d.finishing {
            (d.finish_position, d.finish_initial, true)
        } else {
            (d.position, d.initial, false)
        };
        let clock = d.clock;
        let stage = d.stage;
        let state = self.access().get_block(p)?;
        if state >> 4 == 0 {
            if finishing {
                self.backend.entities.digging.finishing = false;
            } else {
                self.crack(p, -1);
                self.backend.entities.digging.stage = -1;
                self.backend.entities.digging.active = false;
            }
            return Ok(());
        }
        let progress = self.dig_rate(state) * clock.wrapping_sub(initial).wrapping_add(1) as f32;
        let next = (progress * 10.) as i32;
        if next != stage {
            self.crack(p, next);
            self.backend.entities.digging.stage = next;
        }
        if finishing && progress >= 1. {
            self.backend.entities.digging.finishing = false;
            if supported(state) {
                self.harvest(p, state)?;
            }
        }
        Ok(())
    }
    pub fn dig_block(&mut self, epoch: u64, request: DigBlock) -> Result<DigReceipt> {
        let view = self.inventory_view(epoch);
        let mut out = DigReceipt {
            outcome: DigOutcome::Accepted,
            stamp: view.stamp,
            corrections: vec![],
        };
        if view.stamp != request.stamp {
            out.outcome = DigOutcome::Stale;
            return Ok(out);
        }
        let (Some(player), Some(frame)) = (view.player, self.backend.entities.player.clone())
        else {
            out.outcome = DigOutcome::NotReady;
            return Ok(out);
        };
        let p = IVec3::from_array(request.position);
        let side = *SIDES
            .get(request.face as usize)
            .ok_or_else(|| anyhow::anyhow!("Invalid C07 direction"))?;
        let d = frame.position - (p.as_dvec3() + glam::DVec3::splat(0.5)) + glam::DVec3::Y * 1.5;
        if d.length_squared() > 36. || p.y >= 256 {
            out.outcome = DigOutcome::OutsideReach;
            return Ok(out);
        }
        let state = self.access().get_block(p)?;
        if !supported(state) {
            out.outcome = DigOutcome::UnsupportedBlock(state >> 4);
            return Ok(out);
        }
        match request.action {
            DigAction::Start => {
                if p.x < -29_999_984 || p.x >= 29_999_984 || p.z < -29_999_984 || p.z >= 29_999_984
                {
                    out.corrections.push((request.position, state));
                } else if !frame.spectator {
                    let fire = self.access().get_block(p + side)? >> 4 == 51;
                    if fire {
                        self.backend
                            .entities
                            .effects
                            .push(EntityEffect::WorldEvent {
                                position: (p + side).to_array(),
                                id: 1004,
                            });
                        self.access().set_block_flags(p + side, 0, 3)?;
                    }
                    if player.creative {
                        if !fire && self.harvest(p, state)? {
                            out.corrections
                                .push((request.position, self.access().get_block(p)?));
                        }
                    } else {
                        self.backend.entities.digging.initial = self.backend.entities.digging.clock;
                        if matches!(state >> 4, 73 | 74) {
                            self.access().activate_redstone_ore(p, state)?;
                        }
                        let amount = if state >> 4 == 0 {
                            1.
                        } else {
                            self.dig_rate(state)
                        };
                        if state >> 4 != 0 && amount >= 1. {
                            let now = self.access().get_block(p)?;
                            self.harvest(p, now)?;
                        } else {
                            let d = &mut self.backend.entities.digging;
                            d.active = true;
                            d.position = p;
                            d.stage = (amount * 10.) as i32;
                            self.crack(p, (amount * 10.) as i32);
                        }
                    }
                }
            }
            DigAction::Stop => {
                let d = &self.backend.entities.digging;
                if d.position == p && state >> 4 != 0 {
                    let progress = self.dig_rate(state)
                        * d.clock.wrapping_sub(d.initial).wrapping_add(1) as f32;
                    if progress >= 0.7 {
                        self.backend.entities.digging.active = false;
                        self.crack(p, -1);
                        self.harvest(p, state)?;
                    } else if !d.finishing {
                        let d = &mut self.backend.entities.digging;
                        d.active = false;
                        d.finishing = true;
                        d.finish_position = p;
                        d.finish_initial = d.initial;
                    }
                }
                let current = self.access().get_block(p)?;
                if current >> 4 != 0 {
                    out.corrections.push((request.position, current));
                }
            }
            DigAction::Abort => {
                self.backend.entities.digging.active = false;
                self.crack(self.backend.entities.digging.position, -1);
                if state >> 4 != 0 {
                    out.corrections.push((request.position, state));
                }
            }
        }
        let player = self.backend.player_inventory.as_mut().unwrap();
        player.revision = player
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Native inventory revision exhausted"))?;
        out.stamp = self.inventory_view(epoch).stamp;
        Ok(out)
    }
    fn harvest(&mut self, p: IVec3, state: u16) -> Result<bool> {
        let player = self
            .backend
            .player_inventory
            .as_ref()
            .expect("Mining without player");
        let held = player.inventory.held();
        let creative = player.creative;
        if creative
            && held
                .as_ref()
                .is_some_and(|s| matches!(s.id, 267 | 268 | 272 | 276 | 283))
        {
            return Ok(false);
        }
        let id = state >> 4;
        let can = food::can_harvest(state, held.as_ref().map(|s| s.id));
        self.backend
            .entities
            .effects
            .push(EntityEffect::BlockBreak {
                position: p.to_array(),
                state,
            });
        let tile = DropWorld::tile_entity(&mut self.access(), p)?;
        if creative && id == 144 {
            self.access().set_block_flags(p, state | 8, 4)?;
        }
        if creative && id == 140 {
            if let Some(c) = self.backend.world.chunks.get_mut(&cp(p)) {
                Arc::make_mut(c)
                    .tiles
                    .remove(&[p.x.rem_euclid(16), p.y, p.z.rem_euclid(16)]);
            }
        }
        if id == 26 && creative && state & 8 != 0 {
            let direction = [IVec3::Z, IVec3::NEG_X, IVec3::NEG_Z, IVec3::X][(state & 3) as usize];
            if self.access().get_block(p - direction)? >> 4 == 26 {
                self.access().set_block_flags(p - direction, 0, 3)?;
            }
        }
        if id == 175 {
            if state & 8 != 0 {
                let below = p - IVec3::Y;
                let lower = self.access().get_block(below)?;
                if lower >> 4 == 175 {
                    if !creative {
                        if held.as_ref().is_some_and(|s| s.id == 359) && matches!(lower & 7, 2 | 3)
                        {
                            self.shear_double_plant(p, lower)?;
                        } else {
                            self.backend
                                .entities
                                .effects
                                .push(EntityEffect::BlockBreak {
                                    position: below.to_array(),
                                    state: lower,
                                });
                            let mut r = self.backend.world_random.share();
                            super::super::drops::drop_block(
                                &mut self.access(),
                                &mut r,
                                below,
                                lower,
                                1.,
                                0,
                            )?;
                        }
                    }
                    self.access().set_block_flags(below, 0, 3)?;
                }
            } else if creative && self.access().get_block(p + IVec3::Y)? >> 4 == 175 {
                self.access().set_block_flags(p + IVec3::Y, 0, 2)?;
            }
        }
        if creative
            && matches!(id, 64 | 71 | 193..=197)
            && state & 8 != 0
            && self.access().get_block(p - IVec3::Y)? >> 4 == id
        {
            self.access().set_block_flags(p - IVec3::Y, 0, 3)?;
        }
        let removed = self.access().set_block_flags(p, 0, 3)?;
        if creative {
            return Ok(true);
        }
        self.wear_mining_item(state)?;
        if removed && can {
            self.harvest_drops(p, state, tile)?;
        }
        Ok(removed)
    }
    fn shear_double_plant(&mut self, p: IVec3, state: u16) -> Result<()> {
        if let Some(stat) = mining::statistic(state) {
            self.backend.entities.effects.push(EntityEffect::Stat {
                player: self.backend.entities.player.as_ref().unwrap().name.clone(),
                id: stat.into(),
            });
        }
        let mut random = self.backend.world_random.share();
        let meta = if state & 7 == 2 { 1 } else { 2 };
        super::super::drops::spawn_item(
            &mut self.access(),
            &mut random,
            p,
            DropStack {
                id: 31,
                count: 2,
                meta,
                nbt: None,
            },
        )
    }
    fn wear_mining_item(&mut self, state: u16) -> Result<()> {
        let b = &mut self.backend;
        let p = b.player_inventory.as_mut().expect("Mining without player");
        let slot = p.inventory.selected;
        let Some(stack) = &mut p.inventory.slots[slot] else {
            return Ok(());
        };
        let wear = match stack.id {
            359 if matches!(state >> 4, 18 | 30 | 31 | 35 | 106 | 132 | 161) => 1,
            267 | 268 | 272 | 276 | 283 => {
                if mining::hardness(state) != 0. {
                    2
                } else {
                    0
                }
            }
            256..=258 | 269..=271 | 273..=275 | 277..=279 | 284..=286 => {
                i32::from(mining::hardness(state) != 0.)
            }
            _ => return Ok(()),
        };
        let mut item = armor::Item {
            id: stack.id,
            count: i32::from(stack.count),
            damage: i32::from(stack.damage),
            unbreakable: stack.unbreakable(),
            enchantments: stack.enchantments(),
        };
        let item_id = stack.id;
        let name = b.entities.player.as_ref().unwrap().name.clone();
        let mut local = JavaRandom::from_state(
            b.entities
                .player_random
                .expect("Initialized native player's random"),
        );
        if let Some(broken) =
            armor::damage_item(&mut item, wear, false, &mut local, &mut b.world_random)
        {
            b.entities.with_math(|r| {
                for _ in 0..5 {
                    r.next_double();
                }
            });
            b.entities.effects.push(EntityEffect::ToolBreak {
                id: broken.id,
                pitch: broken.pitch,
            });
            if let Some(stat) = mining::item_statistic(item_id, true) {
                b.entities.effects.push(EntityEffect::Stat {
                    player: name.clone(),
                    id: stat.into(),
                });
            }
        }
        b.entities.player_random = Some(local.state());
        stack.count = item.count as u8;
        stack.damage = item.damage as u16;
        if item.count == 0 {
            p.inventory.slots[slot] = None;
        }
        if let Some(stat) = mining::item_statistic(item_id, false) {
            b.entities.effects.push(EntityEffect::Stat {
                player: name,
                id: stat.into(),
            });
        }
        p.revision = p
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Native inventory revision exhausted"))?;
        Ok(())
    }
    fn harvest_drops(
        &mut self,
        p: IVec3,
        state: u16,
        tile: Option<super::super::drops::DropTile>,
    ) -> Result<()> {
        let id = state >> 4;
        let held = self
            .backend
            .player_inventory
            .as_ref()
            .unwrap()
            .inventory
            .held();
        let shear = held.as_ref().is_some_and(|s| s.id == 359);
        if id == 175 && state & 8 == 0 && matches!(state & 7, 2 | 3) && shear {
            return self.shear_double_plant(p, state);
        }
        let banner = matches!(&tile, Some(super::super::drops::DropTile::Banner { .. }));
        if !(matches!(id, 176 | 177) && banner) {
            if let Some(stat) = mining::statistic(state) {
                self.backend.entities.effects.push(EntityEffect::Stat {
                    player: self.backend.entities.player.as_ref().unwrap().name.clone(),
                    id: stat.into(),
                });
            }
        }
        if let Some(amount) =
            food::harvest_exhaustion(state, held.as_ref().map(|s| s.id), banner, false)
        {
            self.backend
                .entities
                .effects
                .push(EntityEffect::Exhaustion { amount });
        }
        let mut random = self.backend.world_random.share();
        let mut access = self.access();
        let custom = if id == 78 {
            Some((332, ((state & 7) + 2) as u8, 0))
        } else if shear && matches!(id, 18 | 31 | 32 | 106 | 161) {
            Some((
                id,
                1,
                match id {
                    18 => state & 3,
                    161 => state & 1,
                    31 => state & 3,
                    _ => 0,
                },
            ))
        } else {
            None
        };
        if let Some((item, count, meta)) = custom {
            super::super::drops::spawn_item(
                &mut access,
                &mut random,
                p,
                DropStack {
                    id: item,
                    count,
                    meta: i32::from(meta),
                    nbt: None,
                },
            )?;
            if id == 78 {
                access.set_block_flags(p, 0, 3)?;
            }
            return Ok(());
        }
        if matches!(id, 176 | 177) {
            if let Some(super::super::drops::DropTile::Banner {
                base_color,
                written_tag,
            }) = tile
            {
                use super::super::drops::DropNbt;
                let mut tag = BTreeMap::from([("Base".into(), DropNbt::Int(base_color))]);
                if let Some(patterns) = written_tag.get("Patterns") {
                    tag.insert("Patterns".into(), patterns.clone());
                }
                return super::super::drops::spawn_item(
                    &mut access,
                    &mut random,
                    p,
                    DropStack {
                        id: 425,
                        count: 1,
                        meta: base_color,
                        nbt: Some(DropNbt::Compound(BTreeMap::from([(
                            "BlockEntityTag".into(),
                            DropNbt::Compound(tag),
                        )]))),
                    },
                );
            }
        }
        if held.as_ref().is_some_and(|s| s.enchantment_level(33) > 0) && mining::can_silk(state) {
            if let Some(s) = mining::silk(state) {
                super::super::drops::spawn_item(
                    &mut access,
                    &mut random,
                    p,
                    DropStack {
                        id: s.id,
                        count: s.count,
                        meta: i32::from(s.meta),
                        nbt: None,
                    },
                )?;
            }
        } else {
            let fortune = held.as_ref().map_or(0, |s| s.enchantment_level(35));
            super::super::drops::drop_block(&mut access, &mut random, p, state, 1., fortune)?;
            if id == 79 {
                let below = access.get_block(p - IVec3::Y)?;
                if material::blocks_movement(below) || material::is_liquid(below) {
                    access.set_block_flags(p, 8 << 4, 3)?;
                }
            }
        }
        Ok(())
    }
}
