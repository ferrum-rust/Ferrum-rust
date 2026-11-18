use crate::{
    vanilla::drops::DropNbt,
    world::{ChunkPos, World},
};
use glam::IVec3;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct State {
    pub imported_seed: Option<i64>,
    pub rules: BTreeMap<String, String>,
    pub spawn: Option<[i32; 3]>,
    pub border_damage: f64,
    pub border_buffer: f64,
    // Grouped by chunk so streaming only touches tiles in the reloaded column.
    tiles: BTreeMap<String, Vec<([i32; 3], u16, DropNbt)>>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            imported_seed: None,
            rules: BTreeMap::new(),
            spawn: None,
            border_damage: 0.2,
            border_buffer: 5.,
            tiles: BTreeMap::new(),
        }
    }
}
impl State {
    pub fn edit_tile(&mut self, world: &mut World, p: IVec3, state: u16, tile: Option<DropNbt>) {
        if self.tiles.is_empty() && tile.is_none() {
            return;
        }
        let key = format!("{},{}", p.x.div_euclid(16), p.z.div_euclid(16));
        if let Some(list) = self.tiles.get_mut(&key) {
            list.retain(|(pos, _, _)| *pos != p.to_array());
            if list.is_empty() {
                self.tiles.remove(&key);
            }
        }
        let Some(tile) = tile else { return };
        Self::apply(world, p, tile.clone());
        self.tiles
            .entry(key)
            .or_default()
            .push((p.to_array(), state >> 4, tile));
    }
    fn apply(world: &mut World, p: IVec3, tile: DropNbt) {
        if let Some(c) = world
            .chunks
            .get_mut(&(p.x.div_euclid(16), p.z.div_euclid(16)))
        {
            Arc::make_mut(c).tiles.insert(
                [p.x.rem_euclid(16), p.y, p.z.rem_euclid(16)],
                Arc::new(tile),
            );
        }
    }
    pub fn restore_tiles(&self, world: &mut World, positions: &[ChunkPos]) {
        if self.tiles.is_empty() {
            return;
        }
        for &(x, z) in positions {
            if let Some(list) = self.tiles.get(&format!("{x},{z}")) {
                for (pos, id, tag) in list {
                    let p = IVec3::from_array(*pos);
                    if crate::vanilla::middle_pick::retains_tile(*id, world.get(p) >> 4) {
                        Self::apply(world, p, tag.clone());
                    }
                }
            }
        }
    }
    pub fn take_tile_edits(&mut self) -> Vec<([i32; 3], (u16, Option<DropNbt>))> {
        let mut out = BTreeMap::new();
        for list in std::mem::take(&mut self.tiles).into_values() {
            for (pos, id, tag) in list {
                out.insert(pos, (id, Some(tag)));
            }
        }
        out.into_iter().collect()
    }
    pub fn clone_tile_edits(&self) -> Vec<([i32; 3], (u16, Option<DropNbt>))> {
        let mut out = BTreeMap::new();
        for list in self.tiles.values() {
            for (pos, id, tag) in list {
                out.insert(*pos, (*id, Some(tag.clone())));
            }
        }
        out.into_iter().collect()
    }
    pub fn set_tile_edits(&mut self, edits: Vec<([i32; 3], (u16, Option<DropNbt>))>) {
        self.tiles.clear();
        for (pos, (raw_id, maybe_tag)) in edits {
            if let Some(tag) = maybe_tag {
                let id = if raw_id >= 256 { raw_id >> 4 } else { raw_id };
                let key = format!("{},{}", pos[0].div_euclid(16), pos[2].div_euclid(16));
                let list = self.tiles.entry(key).or_default();
                list.retain(|(p, _, _)| *p != pos);
                list.push((pos, id, tag));
            }
        }
    }
}
