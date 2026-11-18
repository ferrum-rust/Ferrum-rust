//! Single-player C08 processing against the worker's current MP pose and slots.
use super::*;
use crate::inventory::Stack;
use crate::vanilla::{
    collision_shapes::{self, BlockView},
    hand_placement,
};
use glam::DVec3;
include!("runtime_activation_data.rs");
#[cfg(test)]
#[path = "runtime_use_tests.rs"]
mod tests;
const SIDES: [IVec3; 6] = [
    IVec3::NEG_Y,
    IVec3::Y,
    IVec3::NEG_Z,
    IVec3::Z,
    IVec3::NEG_X,
    IVec3::X,
];
#[derive(Clone, Debug)]
pub struct UseBlock {
    pub stamp: InventoryStamp,
    pub clicked: [i32; 3],
    pub expected: u16,
    pub face: u8,
    /// C08 stores sixteenths of the clicked block.
    pub cursor: [u8; 3],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UseOutcome {
    Stale,
    NotReady,
    OutOfReach,
    OutsideBorder,
    TooHigh,
    Ignored,
    Activated,
    Placed,
    UnsupportedBlock(u16),
    UnsupportedItem(u16),
    UnsupportedTileTag,
}
#[derive(Clone, Debug)]
pub struct UseReceipt {
    pub outcome: UseOutcome,
    pub stamp: InventoryStamp,
    pub corrections: [([i32; 3], u16); 2],
    /// Original 1003/1006 event at the clicked coordinate.
    pub auxiliary: Option<i32>,
    pub place_sound: Option<([f64; 3], &'static str, f32, f32)>,
}
struct View<'a, 'b>(&'a Access<'b>);
impl BlockView for View<'_, '_> {
    fn get(&self, p: IVec3) -> u16 {
        self.0.get_block(p).expect("Native placement read failed")
    }
    fn piston(&self, p: IVec3) -> Option<collision_shapes::PistonTile> {
        self.0.with(|_, b| b.world.piston_at(p))
    }
}
impl hand_placement::Access for View<'_, '_> {
    fn loaded(&self, p: IVec3) -> bool {
        self.0
            .provide(cp(p))
            .expect("Native placement chunk load failed")
            .is_some()
    }
    fn light(&self, p: IVec3) -> u8 {
        self.0
            .combined(p, true)
            .expect("Native placement light read failed")
    }
}
impl Runtime {
    pub fn use_block(&mut self, epoch: u64, request: UseBlock) -> Result<UseReceipt> {
        let p = IVec3::from_array(request.clicked);
        let side = *SIDES
            .get(request.face as usize)
            .ok_or_else(|| anyhow::anyhow!("Invalid C08 block face"))?;
        ensure!(
            request.cursor.iter().all(|&v| v <= 16)
                && p.to_array()
                    .into_iter()
                    .all(|v| v.unsigned_abs() <= 30_000_000),
            "Invalid C08 click"
        );
        let actual = self.access().get_block(p)?;
        let current = self.inventory_view(epoch);
        let mut result = UseReceipt {
            outcome: UseOutcome::Ignored,
            stamp: current.stamp,
            corrections: [
                (p.to_array(), actual),
                ((p + side).to_array(), self.access().get_block(p + side)?),
            ],
            auxiliary: None,
            place_sound: None,
        };
        if current.stamp != request.stamp || actual != request.expected {
            result.outcome = UseOutcome::Stale;
            return Ok(result);
        }
        let (Some(frame), Some(player)) = (self.backend.entities.player.clone(), current.player)
        else {
            result.outcome = UseOutcome::NotReady;
            return Ok(result);
        };
        if p.y >= 256 || p.y == 255 && side == IVec3::Y {
            result.outcome = UseOutcome::TooHigh;
        } else if !frame.movement_acknowledged {
            result.outcome = UseOutcome::NotReady;
        } else if frame
            .position
            .distance_squared(p.as_dvec3() + DVec3::splat(0.5))
            >= 64.
        {
            result.outcome = UseOutcome::OutOfReach;
        } else if p.x < -29_999_984 || p.z < -29_999_984 || p.x >= 29_999_984 || p.z >= 29_999_984 {
            result.outcome = UseOutcome::OutsideBorder;
        } else {
            let held = player.inventory.slots[player.inventory.selected].as_ref();
            let block = actual >> 4;
            let activate = (!frame.sneaking || held.is_none() || frame.spectator)
                && BLOCK_ACTIVATION.contains(&block);
            let activation = if activate {
                self.activate_native_block(epoch, &frame, p, actual, &mut result)?
            } else {
                false
            };
            if !activation && !frame.spectator {
                if let Some(stack) = held {
                    self.place_native_item(
                        &frame,
                        p,
                        &request,
                        stack,
                        player.creative,
                        &mut result,
                    )?;
                }
            }
        }
        if matches!(
            result.outcome,
            UseOutcome::UnsupportedBlock(_)
                | UseOutcome::UnsupportedItem(_)
                | UseOutcome::UnsupportedTileTag
        ) {
            return Ok(result);
        }
        // C08's selected-slot reconciliation replaces the server's held
        // ItemStack with a fresh copy for ordinary ItemBlocks (max use time 0).
        // Long-use food, bows, swords, etc. keep identity throughout active use.
        let player = self
            .backend
            .player_inventory
            .as_mut()
            .expect("Initialized inventory");
        if let Some(stack) = &mut player.inventory.slots[player.inventory.selected] {
            if hand_placement::supported(stack.id) || stack.id == 355 {
                *stack = stack.copied();
            }
        }
        if player.revision == current.stamp.revision {
            player.revision = player
                .revision
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("Native inventory revision exhausted"))?;
        }
        result.stamp = self.inventory_view(epoch).stamp;
        result.corrections[0].1 = self.access().get_block(p)?;
        result.corrections[1].1 = self.access().get_block(p + side)?;
        Ok(result)
    }
    fn activate_native_block(
        &mut self,
        epoch: u64,
        frame: &PlayerFrame,
        p: IVec3,
        state: u16,
        out: &mut UseReceipt,
    ) -> Result<bool> {
        let id = state >> 4;
        if id == 58 && !frame.spectator {
            let open = InventoryAction::OpenWorkbench {
                position: p.to_array(),
                player: frame.position,
            };
            self.inventory_action(epoch, self.inventory_view(epoch).stamp, open)?;
            self.backend.entities.effects.push(EntityEffect::Stat {
                player: frame.name.clone(),
                id: "stat.craftingTableInteraction".into(),
            });
            out.outcome = UseOutcome::Activated;
            return Ok(true);
        }
        if matches!(id, 54 | 146) {
            let open = InventoryAction::OpenChest {
                position: p.to_array(),
                player: frame.position,
            };
            let opened = self.inventory_action(epoch, self.inventory_view(epoch).stamp, open)?;
            if opened.rejected.is_none() && !frame.spectator {
                self.backend.entities.effects.push(EntityEffect::Stat {
                    player: frame.name.clone(),
                    id: if id == 54 {
                        "stat.chestOpened".into()
                    } else {
                        "stat.trappedChestTriggered".into()
                    },
                });
            }
            out.outcome = UseOutcome::Activated;
            return Ok(true);
        }
        if frame.spectator {
            return Ok(false);
        }
        let mut target = p;
        let changed = match id {
            73 | 74 => {
                self.access().activate_redstone_ore(p, state)?;
                return Ok(false);
            }
            46 if self
                .backend
                .player_inventory
                .as_ref()
                .and_then(|p| p.inventory.held())
                .is_none_or(|s| !matches!(s.id, 259 | 385)) =>
            {
                return Ok(false)
            }
            // Every original stair delegates activation to its plain model
            // block. Those model blocks return false and allow Item.onItemUse.
            53 | 67 | 108 | 109 | 114 | 128 | 134..=136 | 156 | 163 | 164 | 180 => {
                return Ok(false)
            }
            71 | 167 => {
                out.outcome = UseOutcome::Activated;
                return Ok(true);
            }
            64 | 193..=197 => {
                if state & 8 != 0 {
                    target -= IVec3::Y;
                }
                let s = self.access().get_block(target)?;
                if s >> 4 != id {
                    return Ok(false);
                }
                s ^ 4
            }
            96 => state ^ 4,
            107 | 183..=187 => {
                let yaw = ((f64::from(frame.yaw) / 90. + 0.5).floor() as i32 & 3) as u16;
                if state & 4 == 0 && state & 3 == (yaw + 2) & 3 {
                    ((state & !3) | yaw) ^ 4
                } else {
                    state ^ 4
                }
            }
            _ => {
                out.outcome = UseOutcome::UnsupportedBlock(id);
                return Ok(true);
            }
        };
        self.access().set_block_flags(target, changed, 2)?;
        out.auxiliary = Some(if changed & 4 != 0 { 1003 } else { 1006 });
        out.outcome = UseOutcome::Activated;
        Ok(true)
    }
    fn place_native_item(
        &mut self,
        frame: &PlayerFrame,
        p: IVec3,
        request: &UseBlock,
        held: &Stack,
        creative: bool,
        out: &mut UseReceipt,
    ) -> Result<()> {
        if held.id == 355 {
            let mut access = self.access();
            let Some([(foot, state), (head, upper)]) =
                crate::vanilla::sleep::bed_placement(&View(&access), p, request.face, frame.yaw)
            else {
                return Ok(());
            };
            let placed = access.set_block_flags(foot, state, 3)?;
            if placed {
                access.set_block_flags(head, upper, 3)?;
            }
            if !creative {
                self.backend
                    .player_inventory
                    .as_mut()
                    .expect("Initialized inventory")
                    .inventory
                    .take_selected(1);
            }
            if let Some(stat) = super::super::mining::item_statistic(355, false) {
                self.backend.entities.effects.push(EntityEffect::Stat {
                    player: frame.name.clone(),
                    id: stat.into(),
                });
            }
            out.outcome = if placed {
                UseOutcome::Placed
            } else {
                UseOutcome::Activated
            };
            return Ok(());
        }
        if !hand_placement::supported(held.id) || held.meta > 15 {
            out.outcome = UseOutcome::UnsupportedItem(held.id);
            return Ok(());
        }
        if let Some(super::super::drops::DropNbt::Compound(tag)) = held.nbt.as_deref() {
            if tag.contains_key("BlockEntityTag")
                || tag.contains_key("display")
                    && matches!(held.id, 23 | 54 | 61 | 62 | 146 | 154 | 158)
            {
                out.outcome = UseOutcome::UnsupportedTileTag;
                return Ok(());
            }
        }
        let input = hand_placement::Input {
            item: held.id,
            meta: held.meta,
            count: held.count,
            clicked: p,
            side: request.face,
            hit_y: f32::from(request.cursor[1]) / 16.,
            yaw: frame.yaw,
            player: frame.position,
            eye_height: frame.eye_height,
        };
        let body = frame.bounds;
        let mut access = self.access();
        let plan = hand_placement::plan(&View(&access), input, |bounds| {
            access.with(|_, b| {
                (frame.alive && body.intersects(bounds))
                    || b.falling
                        .live
                        .iter()
                        .any(|f| !f.body.dead && f.body.bounds.intersects(bounds))
                    || b.hooks.placement_blocked(bounds)
            })
        })?;
        if !plan.accepted {
            return Ok(());
        }
        let stat = match held.id {
            331 => "stat.useItem.minecraft.redstone", 356 => "stat.useItem.minecraft.repeater",
            404 => "stat.useItem.minecraft.comparator",
            id if hand_placement::door_block(id).is_some() => super::super::mining::item_statistic(id, false).unwrap_or(""),
            id => USE_STATS[id as usize],
        };
        if !stat.is_empty() {
            access.with(|_, b| {
                b.entities.effects.push(EntityEffect::Stat {
                    player: frame.name.clone(),
                    id: stat.into(),
                })
            });
        }
        out.outcome = UseOutcome::Activated;
        let Some(&(at, state)) = plan.writes.first() else {
            return Ok(());
        };
        let door = hand_placement::door_block(held.id).is_some();
        // ItemDoor writes both halves before notifying either set of neighbours.
        let flags = if door { 2 } else { 3 };
        if !access.set_block_flags(at, state, flags)? {
            return Ok(());
        }
        for (p, s) in plan.writes.into_iter().skip(1) {
            access.set_block_flags(p, s, flags)?;
        }
        if door {
            use super::super::rail_events::RailWorld;
            access.notify_neighbors(at, state >> 4);
            access.notify_neighbors(at + IVec3::Y, state >> 4);
        }
        if !creative && plan.consume {
            let player = self
                .backend
                .player_inventory
                .as_mut()
                .expect("Initialized inventory");
            player.inventory.take_selected(1);
        }
        out.outcome = UseOutcome::Placed;
        if held.id == 331 || door {
            return Ok(());
        }
        let (name, volume, pitch) = PLACE_SOUND[(state >> 4) as usize];
        out.place_sound = Some((
            at.to_array().map(|n| f64::from(n as f32 + 0.5)),
            name,
            volume,
            pitch,
        ));
        Ok(())
    }
}
impl Access<'_> {
    pub(super) fn activate_redstone_ore(&mut self, p: IVec3, state: u16) -> Result<()> {
        let mut random = self.with(|_, b| b.world_random.share());
        for side in [
            IVec3::Y,
            IVec3::NEG_Y,
            IVec3::Z,
            IVec3::NEG_Z,
            IVec3::X,
            IVec3::NEG_X,
        ] {
            let mut pos = p
                .to_array()
                .map(|n| f64::from(n as f32 + random.next_float()));
            if !super::super::material::opaque_cube(self.get_block(p + side)?) {
                let axis = if side.x != 0 {
                    0
                } else if side.y != 0 {
                    1
                } else {
                    2
                };
                pos[axis] = f64::from(p[axis]) + if side[axis] > 0 { 1.0625 } else { -0.0625 };
            }
            if pos[0] < f64::from(p.x)
                || pos[0] > f64::from(p.x + 1)
                || pos[1] < 0.
                || pos[1] > f64::from(p.y + 1)
                || pos[2] < f64::from(p.z)
                || pos[2] > f64::from(p.z + 1)
            {
                self.with(|_, b| {
                    b.entities.effects.push(EntityEffect::Item(
                        super::super::item_entity::ItemEffect::Particle {
                            position: DVec3::from_array(pos),
                            name: "reddust".into(),
                            velocity: DVec3::ZERO,
                        },
                    ))
                });
            }
        }
        if state >> 4 == 73 {
            self.set_block_flags(p, 74 << 4, 3)?;
        }
        Ok(())
    }
}
