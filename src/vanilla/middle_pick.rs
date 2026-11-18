//! Minecraft.middleClickMouse and InventoryPlayer.setCurrentItem.
use super::{drops::DropNbt, item_nbt};
use crate::{
    container::{Container, Kind},
    inventory::{Inventory, Stack},
};
use glam::IVec3;
use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock},
};

include!("middle_pick_data.rs");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: u16,
    pub damage: u16,
    pub match_damage: bool,
}
fn item_row(id: u16) -> Option<&'static (u16, &'static str, u8)> {
    ITEM.binary_search_by_key(&id, |row| row.0)
        .ok()
        .map(|i| &ITEM[i])
}
pub(crate) fn named_item(name: &str) -> Option<u16> {
    ITEM.iter()
        .find(|row| row.1 == name || row.1.strip_prefix("minecraft:") == Some(name))
        .map(|row| row.0)
}
pub(super) fn item_name(id: u16) -> Option<&'static str> {
    item_row(id).map(|row| row.1)
}
fn tile_row(id: u16) -> Option<&'static (u16, &'static str, &'static str)> {
    TILE.binary_search_by_key(&id, |row| row.0)
        .ok()
        .map(|i| &TILE[i])
}
pub fn compatible_tile(id: u16, kind: &str) -> bool {
    tile_row(id).is_some_and(|row| row.1 == kind)
}
pub fn retains_tile(old: u16, new: u16) -> bool {
    old == new || matches!((old, new), (61, 62) | (62, 61))
}
pub fn block(raw: u16, below: u16, tile: Option<&DropNbt>) -> Option<Item> {
    let value = *BLOCK.get(usize::from(raw))?;
    let mut id = (value >> 9) as u16;
    let mut damage = ((value >> 1) & 255) as u16;
    if id == 0 {
        return None;
    }
    let mut subtype = value & 1 != 0;
    let data = tile.and_then(item_nbt::compound);
    match raw >> 4 {
        175 if raw & 8 != 0 && below >> 4 == 175 => {
            damage = if (below & 7) <= 5 && below & 8 == 0 {
                below & 7
            } else {
                0
            };
        }
        144 => {
            damage = data.map_or(0, |m| item_nbt::number(m.get("SkullType")).max(0) as u16);
        }
        140 => {
            if let Some(m) = data {
                let pot = match m.get("Item") {
                    Some(DropNbt::String(name)) => named_item(name),
                    other => u16::try_from(item_nbt::number(other))
                        .ok()
                        .filter(|&i| item_row(i).is_some()),
                };
                if let Some(item) = pot {
                    id = item;
                    damage = item_nbt::number(m.get("Data")).max(0) as u16;
                    subtype = item_row(id).is_some_and(|row| row.2 & 1 != 0);
                }
            }
        }
        _ => {}
    }
    Some(Item {
        id,
        damage,
        match_damage: subtype,
    })
}
pub fn entity(registry_id: u8, frame_item: Option<&Stack>) -> Option<Item> {
    if registry_id == 18 {
        if let Some(s) = frame_item {
            return Some(Item {
                id: s.id,
                damage: metadata(s),
                match_damage: true,
            });
        }
    }
    let &(_, id, damage) = ENTITY.iter().find(|r| r.0 == registry_id)?;
    Some(Item {
        id,
        damage,
        match_damage: id == 383,
    })
}
fn metadata(stack: &Stack) -> u16 {
    if super::armor::maximum_damage(stack.id) > 0 {
        stack.damage
    } else {
        stack.meta
    }
}
fn stack(item: Item, count: u8) -> Stack {
    let mut s = Stack::new(item.id, count);
    if super::armor::maximum_damage(item.id) > 0 {
        s.damage = item.damage;
    } else {
        s.meta = item.damage;
    }
    s
}
fn matches(s: &Stack, item: Item, damage: bool) -> bool {
    s.id == item.id && (!damage || metadata(s) == item.damage)
}
fn enchantable(s: &Stack) -> bool {
    let bit = if s.count == 1 { 2 } else { 4 };
    item_row(s.id).is_some_and(|row| row.2 & bit != 0)
        && !matches!(
            s.nbt
                .as_deref()
                .and_then(item_nbt::compound)
                .and_then(|m| m.get("ench")),
            Some(DropNbt::List { .. })
        )
}
pub fn select(inv: &mut Inventory, item: Item, creative: bool) {
    let index = |inv: &Inventory, item: Item, damage: bool| {
        inv.slots[..36]
            .iter()
            .position(|s| s.as_ref().is_some_and(|s| matches(s, item, damage)))
    };
    let old = inv.selected;
    if let Some(slot) = index(inv, item, item.match_damage).filter(|&slot| slot < 9) {
        inv.selected = slot;
        return;
    }
    if !creative {
        return;
    }
    if let Some(empty) = inv.slots[..9].iter().position(Option::is_none) {
        inv.selected = empty;
    }
    if let Some(old) = &inv.slots[old] {
        if enchantable(old)
            && index(
                inv,
                Item {
                    id: old.id,
                    damage: metadata(old),
                    match_damage: true,
                },
                true,
            ) == Some(inv.selected)
        {
            return;
        }
    }
    let count = if let Some(source) = index(inv, item, true) {
        let count = inv.slots[source].as_ref().unwrap().count;
        inv.slots[source] = inv.slots[inv.selected].take();
        count
    } else {
        1
    };
    inv.slots[inv.selected] = Some(stack(item, count));
}
pub fn pick_with_tile(item: Item, tile: &DropNbt) -> Stack {
    let mut s = stack(item, 1);
    let mut map = BTreeMap::new();
    if item.id == 397 {
        if let Some(owner) = item_nbt::compound(tile).and_then(|m| m.get("Owner")) {
            let profile = if matches!(owner, DropNbt::Compound(_)) {
                owner.clone()
            } else {
                DropNbt::Compound(BTreeMap::new())
            };
            map.insert("SkullOwner".into(), profile);
            return s.with_nbt(DropNbt::Compound(map));
        }
    }
    map.insert("BlockEntityTag".into(), tile.clone());
    map.insert(
        "display".into(),
        DropNbt::Compound(BTreeMap::from([(
            "Lore".into(),
            DropNbt::List {
                element: 8,
                values: vec![DropNbt::String("(+NBT)".into())],
            },
        )])),
    );
    s.nbt = Some(Arc::new(DropNbt::Compound(map)));
    s
}
pub fn block_click(
    inv: &mut Inventory,
    world: &crate::world::World,
    p: IVec3,
    creative: bool,
    ctrl: bool,
    container: Option<&Container>,
    stored: Option<&DropNbt>,
) -> bool {
    let state = world.get(p);
    let loaded;
    let stored = if world.imported {
        loaded = match stored
            .map(|tag| super::tile_import::load(state >> 4, p, tag))
            .transpose()
        {
            Ok(tag) => tag,
            Err(_) => return false,
        };
        loaded.as_ref()
    } else {
        stored
    };
    let Some(item) = block(state, world.get(p - IVec3::Y), stored) else {
        return false;
    };
    if creative && ctrl {
        if let Some(tile) = tile_snapshot(state >> 4, p, stored, container) {
            inv.slots[inv.selected] = Some(pick_with_tile(item, &tile));
            return true;
        }
    }
    select(inv, item, creative);
    true
}
static DEFAULTS: LazyLock<BTreeMap<u16, DropNbt>> = LazyLock::new(|| {
    TILE.iter()
        .map(|&(id, _, hex)| {
            let bytes: Vec<_> = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect();
            (id, item_nbt::decode(&bytes).unwrap().unwrap())
        })
        .collect()
});
pub(super) fn default_at(id: u16, pos: IVec3) -> Option<DropNbt> {
    tile_snapshot(id, pos, DEFAULTS.get(&id), None)
}
pub fn tile_snapshot(
    id: u16,
    pos: IVec3,
    stored: Option<&DropNbt>,
    container: Option<&Container>,
) -> Option<DropNbt> {
    let original = stored.or_else(|| (id != 36).then(|| DEFAULTS.get(&id)).flatten())?;
    let mut tile = item_nbt::compound(original)?.clone();
    let name = tile_row(id)?.1;
    tile.insert("id".into(), DropNbt::String(name.into()));
    for (k, v) in ["x", "y", "z"].into_iter().zip(pos.to_array()) {
        tile.insert(k.into(), DropNbt::Int(v));
    }
    if let Some(c) = container {
        let stacks: Vec<_> = c
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let s = s.as_ref()?;
                let mut m = BTreeMap::from([
                    ("Slot".into(), DropNbt::Byte(i as i8)),
                    ("id".into(), DropNbt::String(item_row(s.id)?.1.into())),
                    ("Count".into(), DropNbt::Byte(s.count as i8)),
                    ("Damage".into(), DropNbt::Short(metadata(s) as i16)),
                ]);
                if let Some(tag) = &s.nbt {
                    m.insert("tag".into(), tag.as_ref().clone());
                }
                Some(DropNbt::Compound(m))
            })
            .collect();
        tile.insert(
            "Items".into(),
            DropNbt::List {
                element: if stacks.is_empty() { 0 } else { 10 },
                values: stacks,
            },
        );
        if c.kind == Kind::Furnace {
            tile.insert("BurnTime".into(), DropNbt::Short(c.burn as i16));
            tile.insert("CookTime".into(), DropNbt::Short(c.cook as i16));
        }
        if id == 154 {
            tile.insert("TransferCooldown".into(), DropNbt::Int(c.transfer_cooldown));
        }
    }
    Some(DropNbt::Compound(tile))
}
#[cfg(test)]
#[path = "middle_pick_tests.rs"]
mod tests;
