//! Canonical native tile slots and atomic player/window transactions.
#[cfg(test)]
#[path="runtime_craft_window_tests.rs"]
mod workbench_tests;
use super::*;
use crate::{
    container::{window_click, Container, Kind},
    inventory::{ClickAction, Inventory, SlotRef, Stack},
    vanilla::{armor, drops::DropNbt, material, scattered::GeneratedContainer},
};
use glam::DVec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveContainer {
    pub position: [i32; 3],
    pub block: u16,
    pub revision: u64,
    pub slots: Vec<Option<Stack>>,
}
impl LiveContainer {
    pub fn empty(position: [i32; 3], block: u16, revision: u64, size: usize) -> Self {
        Self {
            position,
            block,
            revision,
            slots: vec![None; size],
        }
    }
    pub fn generated(value: &GeneratedContainer, revision: u64) -> Self {
        Self {
            position: value.position,
            block: value.block,
            revision,
            slots: value
                .slots
                .iter()
                .map(|s| s.as_ref().map(stack_from_loot))
                .collect(),
        }
    }
    pub fn legacy_chest(&self) -> Result<Container> {
        ensure!(
            matches!(self.block, 54 | 146) && self.slots.len() == 27,
            "This native tile needs a different container screen"
        );
        let mut container = Container::new(Kind::Chest);
        container.slots = self.slots.clone();
        Ok(container)
    }
}
pub fn stack_from_loot(value: &LootStack) -> Stack {
    let mut stack = Stack::new(value.id, value.count);
    if armor::maximum_damage(value.id) > 0 {
        stack.damage = value.meta;
    } else {
        stack.meta = value.meta;
    }
    if !value.enchants.is_empty() {
        let values = value
            .enchants
            .iter()
            .map(|&[id, level]| {
                DropNbt::Compound(BTreeMap::from([
                    ("id".into(), DropNbt::Short(id as i16)),
                    ("lvl".into(), DropNbt::Short(level as i16)),
                ]))
            })
            .collect();
        let key = if value.id == 403 {
            "StoredEnchantments"
        } else {
            "ench"
        };
        stack = stack.with_nbt(DropNbt::Compound(BTreeMap::from([(
            key.into(),
            DropNbt::List {
                element: 10,
                values,
            },
        )])));
    }
    stack
}
#[cfg(test)]
pub(super) fn oracle_loot(stack: &Stack) -> LootStack {
    let key = if stack.id == 403 {
        "StoredEnchantments"
    } else {
        "ench"
    };
    let enchants = match stack
        .nbt
        .as_deref()
        .and_then(crate::vanilla::item_nbt::compound)
        .and_then(|map| map.get(key))
    {
        Some(DropNbt::List {
            element: 10,
            values,
        }) => values
            .iter()
            .map(|value| {
                let c = crate::vanilla::item_nbt::compound(value).unwrap();
                [
                    crate::vanilla::item_nbt::number(c.get("id")) as u16,
                    crate::vanilla::item_nbt::number(c.get("lvl")) as u16,
                ]
            })
            .collect(),
        _ => Vec::new(),
    };
    LootStack {
        id: stack.id,
        count: stack.count,
        meta: if armor::maximum_damage(stack.id) > 0 {
            stack.damage
        } else {
            stack.meta
        },
        enchants,
    }
}
pub(super) fn next_tile_revision(clock: &mut u64) -> u64 {
    *clock = clock
        .checked_add(1)
        .expect("Native tile revision exhausted");
    *clock
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryStamp {
    pub epoch: u64,
    pub revision: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindowPart {
    pub position: [i32; 3],
    pub revision: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativePlayerInventory {
    #[serde(with = "full_inventory")]
    pub inventory: Inventory,
    pub creative: bool,
    pub revision: u64,
    pub window_id: u8,
    pub next_window: u8,
    pub parts: Vec<WindowPart>,
    #[serde(default)]
    pub workbench: Option<[i32; 3]>,
    #[serde(default)]
    pub personal_crafting: Option<Vec<Option<Stack>>>,
}
mod full_inventory {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        inv: &Inventory,
        s: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        (&inv.slots, inv.selected, &inv.cursor, &inv.crafting).serialize(s)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> std::result::Result<Inventory, D::Error> {
        let (slots, selected, cursor, crafting) = Deserialize::deserialize(d)?;
        Ok(Inventory {
            slots,
            selected,
            cursor,
            crafting,
        })
    }
}
impl NativePlayerInventory {
    pub fn validate(&self) -> Result<()> {
        validate_inventory(&self.inventory)?;
        ensure!(
            self.revision > 0 && self.window_id <= 100 && self.next_window <= 100,
            "Invalid native player inventory revision/window"
        );
        ensure!(
            (self.window_id == 0) == (self.parts.is_empty() && self.workbench.is_none())
                && self.parts.len() <= 5,
            "Invalid native chest window parts"
        );
        ensure!(
            self.workbench.is_some() == self.personal_crafting.is_some()
                && self
                    .workbench
                    .is_none_or(|p| valid(IVec3::from_array(p)) && self.parts.is_empty())
                && self.personal_crafting.as_ref().is_none_or(|g| g.len() == 9),
            "Invalid native crafting window"
        );
        let mut seen = HashSet::new();
        ensure!(
            self.parts
                .iter()
                .all(|p| valid(IVec3::from_array(p.position))
                    && seen.insert(p.position)
                    && p.revision > 0),
            "Invalid native window tile reference"
        );
        Ok(())
    }
}
fn validate_inventory(inv: &Inventory) -> Result<()> {
    ensure!(
        inv.slots.len() == 40 && inv.selected < 9 && inv.crafting.len() == 9,
        "Invalid native player inventory dimensions"
    );
    ensure!(
        inv.slots
            .iter()
            .chain(&inv.crafting)
            .chain(std::iter::once(&inv.cursor))
            .flatten()
            .all(|s| s.id != 0 && s.count > 0),
        "Invalid empty native item stack"
    );
    Ok(())
}
#[derive(Clone, Debug)]
pub enum InventoryAction {
    Initialize {
        inventory: Inventory,
        creative: bool,
    },
    OpenChest {
        position: [i32; 3],
        player: DVec3,
    },
    OpenWorkbench {
        position: [i32; 3],
        player: DVec3,
    },
    Click {
        slot: SlotRef,
        action: ClickAction,
        player: DVec3,
    },
    Close,
    DropSelected {
        all: bool,
    },
    SelectHotbar {
        slot: usize,
    },
}
#[derive(Clone, Debug)]
pub struct InventoryView {
    pub stamp: InventoryStamp,
    pub player: Option<NativePlayerInventory>,
    pub container_slots: Vec<Option<Stack>>,
    pub window_current: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InventoryRejection {
    Stale,
    AlreadyInitialized,
    Uninitialized,
    InvalidInventory,
    InvalidWindow,
    OutOfReach,
    InvalidSlot,
    RequiresPlayerDrop,
}
#[derive(Clone, Debug)]
pub struct InventoryReceipt {
    pub rejected: Option<InventoryRejection>,
    pub view: InventoryView,
}

impl Runtime {
    /// Read the live tile on the serial world owner. Published chunk geometry
    /// can outlive a chest click or spawner assignment.
    pub fn tile_snapshot(&mut self,p:IVec3)->Result<Option<DropNbt>> {
        let state=self.access().get_block(p)?;
        let b=&self.backend;
        let id=state>>4;
        let local=[p.x.rem_euclid(16),p.y,p.z.rem_euclid(16)];
        let stored=b.world.chunks.get(&cp(p)).and_then(|c|c.tiles.get(&local)).map(Arc::as_ref);
        let live=b.live_containers.get(&p.to_array()).map(|tile|{
            let mut c=Container::new(Kind::Chest);c.slots=tile.slots.clone();c
        });
        let banner=match b.drop_tiles.get(&p.to_array()){
            Some(super::super::drops::DropTile::Banner{written_tag,..}) if matches!(id,176|177)=>Some(DropNbt::Compound(written_tag.clone())),
            _=>None,
        };
        let mut tile=super::super::middle_pick::tile_snapshot(id,p,banner.as_ref().or(stored),live.as_ref());
        if id==52{
            if let (Some(DropNbt::Compound(fields)),Some(mob))=(&mut tile,b.live_spawners.get(&p.to_array())){
                fields.insert("EntityId".into(),DropNbt::String(mob.clone()));
            }
        }
        Ok(tile)
    }
    pub(super) fn close_unusable_window(&mut self) -> Result<()> {
        let (Some(player), Some(frame)) = (
            self.backend.player_inventory.as_ref(),
            self.backend.entities.player.as_ref(),
        ) else {
            return Ok(());
        };
        if player.window_id == 0 {
            return Ok(());
        }
        let usable = if let Some(pos) = player.workbench {
            let p = IVec3::from_array(pos);
            self.backend.world.get(p) >> 4 == 58 && reachable(p, frame.position)
        } else {
            player.parts.iter().all(|part| {
                let p = IVec3::from_array(part.position);
                self.backend
                    .live_containers
                    .get(&part.position)
                    .is_some_and(|c| c.revision == part.revision)
                    && reachable(p, frame.position)
            })
        };
        if !usable {
            let stamp = self.inventory_view(0).stamp;
            self.inventory_action(0, stamp, InventoryAction::Close)?;
        }
        Ok(())
    }
    fn rejected_inventory(
        &self,
        epoch: u64,
        reason: InventoryRejection,
    ) -> Result<InventoryReceipt> {
        Ok(InventoryReceipt {
            rejected: Some(reason),
            view: self.inventory_view(epoch),
        })
    }
    pub fn inventory_view(&self, epoch: u64) -> InventoryView {
        let player = self.backend.player_inventory.clone();
        let revision = player.as_ref().map_or(0, |p| p.revision);
        let mut slots = Vec::new();
        let mut current = true;
        if let Some(p) = &player {
            if let Some(pos) = p.workbench {
                current = self.backend.world.get(IVec3::from_array(pos)) >> 4 == 58;
            }
            for part in &p.parts {
                match self.backend.live_containers.get(&part.position) {
                    Some(c) if c.revision == part.revision => slots.extend(c.slots.iter().cloned()),
                    _ => {
                        current = false;
                        slots.clear();
                        break;
                    }
                }
            }
        }
        InventoryView {
            stamp: InventoryStamp { epoch, revision },
            player,
            container_slots: slots,
            window_current: current,
        }
    }
    pub fn inventory_action(
        &mut self,
        epoch: u64,
        stamp: InventoryStamp,
        op: InventoryAction,
    ) -> Result<InventoryReceipt> {
        let view = self.inventory_view(epoch);
        if stamp != view.stamp {
            return self.rejected_inventory(epoch, InventoryRejection::Stale);
        }
        if let InventoryAction::Initialize {
            inventory,
            creative,
        } = op
        {
            if view.player.is_some() {
                return self.rejected_inventory(epoch, InventoryRejection::AlreadyInitialized);
            }
            if validate_inventory(&inventory).is_err() {
                return self.rejected_inventory(epoch, InventoryRejection::InvalidInventory);
            }
            self.backend.player_inventory = Some(NativePlayerInventory {
                inventory,
                creative,
                revision: 1,
                window_id: 0,
                next_window: 0,
                parts: vec![],
                workbench: None,
                personal_crafting: None,
            });
            if !self.backend.entities.order.contains(&EntityKey::Player) {
                self.backend.entities.register_player_id();
                self.backend.entities.order.push(EntityKey::Player);
            }
            if let Some(initial) = self
                .backend
                .initial_player
                .filter(|_| self.backend.entities.player.is_none())
            {
                self.set_player_frame(PlayerFrame::standing(
                    "Player",
                    DVec3::from_array(initial.position),
                ))?;
            }
            return Ok(InventoryReceipt {
                rejected: None,
                view: self.inventory_view(epoch),
            });
        }
        let Some(mut p) = view.player.clone() else {
            return self.rejected_inventory(epoch, InventoryRejection::Uninitialized);
        };
        let next_revision = p
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Native inventory revision exhausted"))?;
        match op {
            InventoryAction::OpenWorkbench { position, player } => {
                let at = IVec3::from_array(position);
                if !reachable(at, player) {
                    return self.rejected_inventory(epoch, InventoryRejection::OutOfReach);
                }
                if self.access().get_block(at)? >> 4 != 58 {
                    return self.rejected_inventory(epoch, InventoryRejection::InvalidWindow);
                }
                // displayGui creates the new ContainerWorkbench directly. A
                // replaced table matrix is distinct from the personal four
                // crafting slots; the carried stack belongs to InventoryPlayer.
                if p.workbench.take().is_some() {
                    p.inventory.crafting=p.personal_crafting.take().expect("Missing personal crafting slots");
                }
                p.personal_crafting =
                    Some(std::mem::replace(&mut p.inventory.crafting, vec![None; 9]));
                p.workbench = Some(position);
                p.parts.clear();
                p.next_window = p.next_window % 100 + 1;
                p.window_id = p.next_window;
            }
            InventoryAction::OpenChest { position, player } => {
                let origin = IVec3::from_array(position);
                if !reachable(origin, player) {
                    return self.rejected_inventory(epoch, InventoryRejection::OutOfReach);
                }
                let access = self.access();
                let block = access.get_block(origin)? >> 4;
                if !matches!(block, 54 | 146)
                    || material::normal_cube(access.get_block(origin + IVec3::Y)?)
                {
                    return self.rejected_inventory(epoch, InventoryRejection::InvalidWindow);
                }
                let mut positions = vec![position];
                for dir in [IVec3::NEG_Z, IVec3::X, IVec3::Z, IVec3::NEG_X] {
                    let at = origin + dir;
                    if access.get_block(at)? >> 4 != block {
                        continue;
                    }
                    if !reachable(at, player)
                        || material::normal_cube(access.get_block(at + IVec3::Y)?)
                    {
                        return self.rejected_inventory(epoch, InventoryRejection::InvalidWindow);
                    }
                    if dir == IVec3::NEG_Z || dir == IVec3::NEG_X {
                        positions.insert(0, at.to_array());
                    } else {
                        positions.push(at.to_array());
                    }
                }
                let mut parts = Vec::new();
                for position in positions {
                    let Some(c) = self.backend.live_containers.get(&position) else {
                        return self.rejected_inventory(epoch, InventoryRejection::InvalidWindow);
                    };
                    parts.push(WindowPart {
                        position,
                        revision: c.revision,
                    });
                }
                if p.window_id != 0 {
                    let drops = close_drops(&mut p);
                    if !self.throw_stacks(drops, false)? {
                        return self
                            .rejected_inventory(epoch, InventoryRejection::RequiresPlayerDrop);
                    }
                }
                p.parts = parts;
                p.next_window = p.next_window % 100 + 1;
                p.window_id = p.next_window;
            }
            InventoryAction::Close => {
                let drops = close_drops(&mut p);
                if !self.throw_stacks(drops, false)? {
                    return self.rejected_inventory(epoch, InventoryRejection::RequiresPlayerDrop);
                }
                p.parts.clear();
                p.window_id = 0;
            }
            InventoryAction::DropSelected { all } => {
                let slot = p.inventory.selected;
                let Some(mut stack) = p.inventory.slots[slot].take() else {
                    return Ok(InventoryReceipt {
                        rejected: None,
                        view,
                    });
                };
                let dropped = if !all && stack.count > 1 {
                    stack.count -= 1;
                    let mut split = stack.copied();
                    split.count = 1;
                    p.inventory.slots[slot] = Some(stack);
                    split
                } else {
                    stack
                };
                if !self.throw_stacks(vec![dropped], true)? {
                    return self.rejected_inventory(epoch, InventoryRejection::RequiresPlayerDrop);
                }
            }
            InventoryAction::SelectHotbar { slot } => {
                if slot >= 9 {
                    return self.rejected_inventory(epoch, InventoryRejection::InvalidSlot);
                }
                p.inventory.selected = slot;
            }
            InventoryAction::Click {
                slot,
                action,
                player,
            } => {
                if !view.window_current {
                    return self.rejected_inventory(epoch, InventoryRejection::Stale);
                }
                if p.workbench
                    .is_some_and(|q| !reachable(IVec3::from_array(q), player))
                    || p.parts
                        .iter()
                        .any(|part| !reachable(IVec3::from_array(part.position), player))
                {
                    return self.rejected_inventory(epoch, InventoryRejection::OutOfReach);
                }
                let valid_slot = |s: SlotRef| match s {
                    SlotRef::Player(i) => i < if p.window_id == 0 { 40 } else { 36 },
                    SlotRef::Container(i) => !p.parts.is_empty() && i < view.container_slots.len(),
                    SlotRef::Craft(i) => {
                        if p.workbench.is_some() {
                            i < 9
                        } else {
                            p.window_id == 0 && i < 4
                        }
                    }
                    SlotRef::CraftOutput => p.window_id == 0 || p.workbench.is_some(),
                    SlotRef::Outside => true,
                };
                if !valid_slot(slot)
                    || matches!(&action,ClickAction::Drag{slots,..} if slots.iter().any(|s|!valid_slot(*s)))
                {
                    return self.rejected_inventory(epoch, InventoryRejection::InvalidSlot);
                }
                let mut container = (!p.parts.is_empty()).then(|| {
                    let mut c = Container::new(Kind::Chest);
                    c.slots = view.container_slots.clone();
                    c
                });
                let dropped = window_click(
                    &mut p.inventory,
                    container.as_mut(),
                    slot,
                    action,
                    p.creative,
                    if p.workbench.is_some() { 3 } else { 2 },
                );
                // No staged player/tile change commits until the world can own
                // the corresponding thrown EntityItem in this same operation.
                ensure!(
                    self.backend
                        .tile_revision
                        .checked_add(p.parts.len() as u64)
                        .is_some(),
                    "Native tile revision exhausted"
                );
                if !self.throw_stacks(dropped, false)? {
                    return self.rejected_inventory(epoch, InventoryRejection::RequiresPlayerDrop);
                }
                if let Some(container) = container {
                    for (part, slots) in p.parts.iter_mut().zip(container.slots.as_chunks::<27>().0)
                    {
                        let tile = self
                            .backend
                            .live_containers
                            .get_mut(&part.position)
                            .expect("Validated open chest missing");
                        if tile.slots != slots
                            || tile.slots.iter().zip(slots).any(|(a, b)| {
                                a.as_ref().map(|s| s.identity) != b.as_ref().map(|s| s.identity)
                            })
                        {
                            tile.slots = slots.to_vec();
                            tile.revision = next_tile_revision(&mut self.backend.tile_revision);
                        }
                        part.revision = tile.revision;
                    }
                }
            }
            InventoryAction::Initialize { .. } => unreachable!(),
        }
        p.revision = next_revision;
        self.backend.player_inventory = Some(p);
        Ok(InventoryReceipt {
            rejected: None,
            view: self.inventory_view(epoch),
        })
    }
}
fn close_drops(player: &mut NativePlayerInventory) -> Vec<Stack> {
    let mut stacks: Vec<Stack> = player.inventory.cursor.take().into_iter().collect();
    if player.workbench.take().is_some() {
        stacks.extend(
            player
                .inventory
                .crafting
                .iter_mut()
                .filter_map(Option::take),
        );
        player.inventory.crafting = player
            .personal_crafting
            .take()
            .expect("Crafting window missing personal matrix");
    } else if player.window_id == 0 {
        stacks.extend(
            player.inventory.crafting[..4]
                .iter_mut()
                .filter_map(Option::take),
        );
    }
    stacks
}
fn reachable(p: IVec3, player: DVec3) -> bool {
    player.is_finite() && player.distance_squared(p.as_dvec3() + DVec3::splat(0.5)) <= 64.
}

#[cfg(test)]
mod tests {
    use super::*;
    fn seeded_chest() -> Runtime {
        let mut r = Runtime::for_overworld_verification(189, 1234, false);
        r.provide((0, 0)).unwrap();
        let west = IVec3::new(5, 180, 6);
        r.access().set_block(west, 54 << 4 | 2).unwrap();
        r.access().set_block(west + IVec3::X, 54 << 4 | 2).unwrap();
        let mut slots = vec![None; 27];
        slots[0] = Some(LootStack {
            id: 403,
            meta: 0,
            count: 1,
            enchants: vec![[16, 5], [34, 3]],
        });
        FeatureWorld::chest(&mut r.access(), west, slots);
        r
    }
    fn apply(r: &mut Runtime, epoch: u64, action: InventoryAction) -> InventoryReceipt {
        let stamp = r.inventory_view(epoch).stamp;
        r.inventory_action(epoch, stamp, action).unwrap()
    }
    #[test]
    fn double_chest_clicks_are_atomic_across_retry_cursor_save_and_reconnect() {
        let mut r = seeded_chest();
        let epoch = 71;
        let player = DVec3::new(5.5, 180., 6.5);
        let mut inventory = Inventory::default();
        inventory.slots[0] = Some(Stack::new(1, 12));
        assert!(apply(
            &mut r,
            epoch,
            InventoryAction::Initialize {
                inventory,
                creative: false
            }
        )
        .rejected
        .is_none());
        let open = apply(
            &mut r,
            epoch,
            InventoryAction::OpenChest {
                position: [6, 180, 6],
                player,
            },
        );
        assert!(open.rejected.is_none());
        assert_eq!(open.view.container_slots.len(), 54);
        let book = open.view.container_slots[0].as_ref().unwrap().clone();
        let click = InventoryAction::Click {
            slot: SlotRef::Container(0),
            action: ClickAction::Pickup { right: false },
            player,
        };
        let picked = r
            .inventory_action(epoch, open.view.stamp, click.clone())
            .unwrap();
        assert!(picked.rejected.is_none());
        assert_eq!(
            picked
                .view
                .player
                .as_ref()
                .unwrap()
                .inventory
                .cursor
                .as_ref(),
            Some(&book)
        );
        assert!(picked.view.container_slots[0].is_none());
        assert_eq!(
            r.inventory_action(epoch, open.view.stamp, click)
                .unwrap()
                .rejected,
            Some(InventoryRejection::Stale)
        );
        let denied = apply(
            &mut r,
            epoch,
            InventoryAction::Click {
                slot: SlotRef::Outside,
                action: ClickAction::Pickup { right: false },
                player,
            },
        );
        assert_eq!(
            denied.rejected,
            Some(InventoryRejection::RequiresPlayerDrop)
        );
        assert_eq!(denied.view.stamp, picked.view.stamp);
        let saved = serde_json::to_vec(&r.checkpoint().unwrap()).unwrap();
        r = Runtime::resume(
            serde_json::from_slice(&saved).unwrap(),
            Arc::new(VanillaPopulation),
        )
        .unwrap();
        let next_epoch = 72;
        let place = InventoryAction::Click {
            slot: SlotRef::Container(27),
            action: ClickAction::Pickup { right: false },
            player,
        };
        assert_eq!(
            r.inventory_action(next_epoch, picked.view.stamp, place.clone())
                .unwrap()
                .rejected,
            Some(InventoryRejection::Stale)
        );
        let placed = apply(&mut r, next_epoch, place);
        assert!(placed.rejected.is_none());
        assert!(placed
            .view
            .player
            .as_ref()
            .unwrap()
            .inventory
            .cursor
            .is_none());
        assert_eq!(placed.view.container_slots[27].as_ref(), Some(&book));
        assert!(placed.view.container_slots[0].is_none());
        let stale = placed.view.stamp;
        let east = IVec3::new(6, 180, 6);
        r.access().set_block(east, 0).unwrap();
        r.access().set_block(east, 54 << 4 | 2).unwrap();
        let after = r
            .inventory_action(
                next_epoch,
                stale,
                InventoryAction::Click {
                    slot: SlotRef::Container(27),
                    action: ClickAction::Pickup { right: false },
                    player,
                },
            )
            .unwrap();
        assert_eq!(after.rejected, Some(InventoryRejection::Stale));
        assert!(!after.view.window_current);
        assert!(r.backend.live_containers[&east.to_array()]
            .slots
            .iter()
            .all(Option::is_none));
        assert!(apply(&mut r, next_epoch, InventoryAction::Close)
            .rejected
            .is_none());
    }
    #[test]
    fn schema_two_generated_loot_migrates_to_full_stack_tags_and_metadata() {
        let r = seeded_chest();
        let mut legacy = r.checkpoint().unwrap();
        legacy.version = 2;
        legacy.live_containers = legacy
            .tile_slots
            .iter()
            .map(|c| GeneratedContainer {
                position: c.position,
                block: c.block,
                slots: c
                    .slots
                    .iter()
                    .map(|s| s.as_ref().map(oracle_loot))
                    .collect(),
            })
            .collect();
        let mut wire = serde_json::to_value(&legacy).unwrap();
        for name in ["tile_slots", "tile_revision", "player_inventory"] {
            wire.as_object_mut().unwrap().remove(name);
        }
        let mut migrated = Runtime::resume(
            serde_json::from_value(wire).unwrap(),
            Arc::new(VanillaPopulation),
        )
        .unwrap();
        assert_eq!(migrated.backend.live_containers.len(), 2);
        let b = migrated.backend.live_containers[&[5, 180, 6]].slots[0]
            .as_ref()
            .unwrap();
        assert_eq!(oracle_loot(b).enchants, vec![[16, 5], [34, 3]]);
        assert!(migrated.backend.player_inventory.is_none());
        assert_eq!(migrated.checkpoint().unwrap().version, 3);
        let init = apply(
            &mut migrated,
            1,
            InventoryAction::Initialize {
                inventory: Inventory::default(),
                creative: false,
            },
        );
        assert!(init.rejected.is_none());
        assert_eq!(
            apply(
                &mut migrated,
                1,
                InventoryAction::Initialize {
                    inventory: Inventory::default(),
                    creative: true
                }
            )
            .rejected,
            Some(InventoryRejection::AlreadyInitialized)
        );
    }
}
