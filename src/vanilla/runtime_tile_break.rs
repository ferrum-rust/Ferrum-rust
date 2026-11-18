//! Contents ejected by Block.breakBlock before removal of its tile.
use super::*;
use crate::vanilla::{
    drops::{DropNbt, DropStack},
    item_nbt, middle_pick,
};
pub(super) fn item(world: &World, p: IVec3, old: u16, new: u16) -> Option<DropStack> {
    let id = old >> 4;
    if new >> 4 == id || !matches!(id, 140 | 144) {
        return None;
    }
    let local = [p.x.rem_euclid(16), p.y, p.z.rem_euclid(16)];
    let stored = world
        .chunks
        .get(&cp(p))
        .and_then(|c| c.tiles.get(&local))
        .map(Arc::as_ref);
    let tile = middle_pick::tile_snapshot(id, p, stored, None)?;
    let fields = item_nbt::compound(&tile)?;
    if id == 140 {
        let item = match fields.get("Item") {
            Some(DropNbt::String(s)) => middle_pick::named_item(s).or_else(|| s.parse().ok()),
            value => u16::try_from(item_nbt::number(value)).ok(),
        }?;
        if item == 0 {
            return None;
        }
        return Some(DropStack {
            id: item,
            count: 1,
            meta: item_nbt::number(fields.get("Data")).max(0),
            nbt: None,
        });
    }
    if old & 8 != 0 {
        return None;
    }
    let kind = item_nbt::number(fields.get("SkullType"));
    let nbt = if kind == 3 {
        fields
            .get("Owner")
            .filter(|tag| matches!(tag, DropNbt::Compound(_)))
            .map(|v| DropNbt::Compound(BTreeMap::from([("SkullOwner".into(), v.clone())])))
    } else {
        None
    };
    Some(DropStack {
        id: 397,
        count: 1,
        meta: kind.max(0),
        nbt,
    })
}
