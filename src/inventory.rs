use crate::vanilla::{drops::DropNbt, item_nbt};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stack {
    pub id: u16,
    #[serde(default)]
    pub meta: u16,
    pub count: u8,
    #[serde(default)]
    pub damage: u16,
    #[serde(
        default,
        with = "item_nbt::optional_arc",
        skip_serializing_if = "Option::is_none"
    )]
    pub nbt: Option<Arc<DropNbt>>,
    /// Java ItemStack object identity. Value comparisons and saved items omit it.
    #[serde(skip, default = "Stack::fresh_identity")]
    pub identity: u64,
}
impl PartialEq for Stack {
    fn eq(&self, other: &Self) -> bool {
        self.same(other) && self.count == other.count
    }
}
impl Eq for Stack {}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SlotRef {
    Player(usize),
    Craft(usize),
    Container(usize),
    CraftOutput,
    Outside,
}
#[derive(Clone, Debug)]
pub enum ClickAction {
    Pickup { right: bool },
    QuickMove,
    Hotbar(usize),
    Clone,
    Throw { all: bool },
    Collect { reverse: bool },
    Drag { mode: u8, slots: Vec<SlotRef> },
}
impl Stack {
    pub fn fresh_identity() -> u64 {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }
    /// Logical ItemStack.copy / splitStack, unlike Rust's reference-value copy.
    pub fn copied(&self) -> Self {
        Self {
            identity: Self::fresh_identity(),
            ..self.clone()
        }
    }
    pub fn with_nbt(mut self, tag: DropNbt) -> Self {
        self.nbt = Some(Arc::new(tag));
        self
    }
    pub fn enchantments(&self) -> Vec<[i16; 2]> {
        item_nbt::enchantments(self.nbt.as_deref())
    }
    pub fn enchantment_level(&self, id: i16) -> i32 {
        item_nbt::enchantment_level(self.nbt.as_deref(), id)
    }
    pub fn combat_item(&self) -> crate::vanilla::armor::Item {
        crate::vanilla::armor::Item {
            id: self.id,
            count: i32::from(self.count),
            damage: i32::from(self.damage),
            unbreakable: self.unbreakable(),
            enchantments: self.enchantments(),
        }
    }
    pub fn unbreakable(&self) -> bool {
        item_nbt::unbreakable(self.nbt.as_deref())
    }
    pub fn new(id: u16, count: u8) -> Self {
        Self {
            id,
            meta: 0,
            count,
            damage: 0,
            nbt: None,
            identity: Self::fresh_identity(),
        }
    }
    pub fn block(state: u16) -> Self {
        Self {
            id: state >> 4,
            meta: state & 15,
            count: 1,
            damage: 0,
            nbt: None,
            identity: Self::fresh_identity(),
        }
    }
    pub fn state(&self) -> u16 {
        self.id << 4 | self.meta & 15
    }
    pub fn same(&self, other: &Self) -> bool {
        self.id == other.id
            && self.meta == other.meta
            && self.damage == other.damage
            && self.same_tags(other)
    }
    pub fn same_tags(&self, other: &Self) -> bool {
        match (&self.nbt, &other.nbt) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                (self.identity == other.identity && Arc::ptr_eq(a, b)) || a.as_ref() == b.as_ref()
            }
            _ => false,
        }
    }
}
pub fn stack_limit(id: u16) -> u8 {
    crate::vanilla::item_entity::facts(id).map_or(64, |facts| facts.max_count)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    None,
    Sword,
    Pick,
    Axe,
    Shovel,
    Hoe,
    Shears,
}
pub fn tool(id: u16) -> (Tool, u8, f32, u16) {
    let (kind, tier) = match id {
        268 => (Tool::Sword, 1),
        269 => (Tool::Shovel, 1),
        270 => (Tool::Pick, 1),
        271 => (Tool::Axe, 1),
        290 => (Tool::Hoe, 1),
        272 => (Tool::Sword, 2),
        273 => (Tool::Shovel, 2),
        274 => (Tool::Pick, 2),
        275 => (Tool::Axe, 2),
        291 => (Tool::Hoe, 2),
        267 => (Tool::Sword, 3),
        256 => (Tool::Shovel, 3),
        257 => (Tool::Pick, 3),
        258 => (Tool::Axe, 3),
        292 => (Tool::Hoe, 3),
        276 => (Tool::Sword, 4),
        277 => (Tool::Shovel, 4),
        278 => (Tool::Pick, 4),
        279 => (Tool::Axe, 4),
        293 => (Tool::Hoe, 4),
        283 => (Tool::Sword, 5),
        284 => (Tool::Shovel, 5),
        285 => (Tool::Pick, 5),
        286 => (Tool::Axe, 5),
        294 => (Tool::Hoe, 5),
        359 => (Tool::Shears, 3),
        _ => return (Tool::None, 0, 1.0, 0),
    };
    let (speed, durability) = match tier {
        1 => (2.0, 59),
        2 => (4.0, 131),
        3 => (6.0, 250),
        4 => (8.0, 1561),
        _ => (12.0, 32),
    };
    (
        kind,
        if tier == 5 { 1 } else { tier },
        if kind == Tool::Shears { 15.0 } else { speed },
        if kind == Tool::Shears {
            238
        } else {
            durability
        },
    )
}
pub fn armor_slot(id: u16) -> Option<usize> {
    if (298..=317).contains(&id) {
        Some(39 - (id as usize - 298) % 4)
    } else {
        None
    }
}
pub fn accepts_armor(index: usize, item: &Stack) -> bool {
    armor_slot(item.id) == Some(index) || (index == 39 && matches!(item.id, 86 | 397))
}
pub fn armor_points(id: u16) -> u8 {
    if !(298..=317).contains(&id) {
        return 0;
    }
    let set = (id - 298) / 4;
    let part = (id - 298) % 4;
    [
        [1, 3, 2, 1],
        [2, 5, 4, 1],
        [2, 6, 5, 2],
        [3, 8, 6, 3],
        [2, 5, 3, 1],
    ][set as usize][part as usize]
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Inventory {
    pub slots: Vec<Option<Stack>>,
    pub selected: usize,
    #[serde(skip)]
    pub cursor: Option<Stack>,
    #[serde(skip)]
    pub crafting: Vec<Option<Stack>>,
}
impl Default for Inventory {
    fn default() -> Self {
        Self {
            slots: vec![None; 40],
            selected: 0,
            cursor: None,
            crafting: vec![None; 9],
        }
    }
}
impl Inventory {
    pub fn sanitize(&mut self) {
        self.slots.resize(40, None);
        self.slots.truncate(40);
        self.selected = self.selected.min(8);
        self.crafting.resize(9, None);
        for slot in &mut self.slots {
            if let Some(s) = slot {
                if s.count == 0 || s.id == 0 {
                    *slot = None;
                } else {
                    s.count = s.count.min(stack_limit(s.id));
                }
            }
        }
    }
    pub fn held(&self) -> Option<Stack> {
        self.slots[self.selected].clone()
    }
    pub fn add(&mut self, mut item: Stack) -> Option<Stack> {
        if self.slots.len() >= 36 && crate::vanilla::item_entity::facts(item.id).is_some() {
            // addItemStackToInventory may insert part of a stack even when its
            // return value is false. The mutated count is the authoritative remainder.
            crate::vanilla::item_entity::add_to_player(self, &mut item, false)
                .expect("Registered item and valid player inventory");
            return (item.count > 0).then_some(item);
        }
        self.merge_into(item, &std::array::from_fn::<_, 36, _>(|i| i))
    }
    /// Container slot order is main inventory (9..36), then hotbar (0..9).
    pub fn merge_into(&mut self, mut item: Stack, targets: &[usize]) -> Option<Stack> {
        for pass in 0..2 {
            for &index in targets {
                let Some(slot) = self.slots.get_mut(index) else {
                    continue;
                };
                let limit = if index >= 36 { 1 } else { stack_limit(item.id) };
                if index >= 36 && !accepts_armor(index, &item) {
                    continue;
                }
                let moved = match slot {
                    Some(s) if pass == 0 && s.same(&item) => {
                        let n = limit.saturating_sub(s.count).min(item.count);
                        s.count += n;
                        n
                    }
                    None if pass == 1 => {
                        let n = limit.min(item.count);
                        *slot = Some(Stack {
                            count: n,
                            ..item.copied()
                        });
                        n
                    }
                    _ => 0,
                };
                item.count -= moved;
                if item.count == 0 {
                    return None;
                }
            }
        }
        Some(item)
    }
    pub fn take_selected(&mut self, count: u8) -> Option<Stack> {
        let s = self.slots[self.selected].as_mut()?;
        let n = count.min(s.count);
        let result = Stack {
            count: n,
            ..if n < s.count { s.copied() } else { s.clone() }
        };
        s.count -= n;
        if s.count == 0 {
            self.slots[self.selected] = None;
        }
        Some(result)
    }
    pub fn armor_enchantment(&self, id: i16) -> i32 {
        self.slots[36..40]
            .iter()
            .flatten()
            .fold(0, |highest, stack| highest.max(stack.enchantment_level(id)))
    }
    pub fn armor(&self) -> u8 {
        self.slots[36..]
            .iter()
            .flatten()
            .map(|s| armor_points(s.id))
            .sum::<u8>()
    }
    #[cfg(test)]
    pub fn click(&mut self, index: usize, right: bool, shift: bool) {
        let slot = if (100..109).contains(&index) {
            SlotRef::Craft(index - 100)
        } else {
            SlotRef::Player(index)
        };
        crate::container::window_click(
            self,
            None,
            slot,
            if shift {
                ClickAction::QuickMove
            } else {
                ClickAction::Pickup { right }
            },
            false,
            if index >= 100 { 3 } else { 2 },
        );
    }
    pub fn close(&mut self) -> Vec<Stack> {
        // Container.onContainerClosed and ContainerPlayer/Workbench drop carried
        // items and crafting inputs; they do not silently move them into inventory.
        self.cursor
            .take()
            .into_iter()
            .chain(self.crafting.iter_mut().filter_map(Option::take))
            .collect()
    }
    #[cfg(test)]
    pub fn craft(&mut self, width: usize) -> bool {
        let Some(recipe) = recipe(&self.crafting, width) else {
            return false;
        };
        if let Some(cursor) = &self.cursor {
            if !cursor.same(&recipe.output)
                || cursor.count + recipe.output.count > stack_limit(cursor.id)
            {
                return false;
            }
        }
        let output=recipe.output.clone();
        crate::vanilla::crafting::consume(self,recipe,false);
        if let Some(s) = &mut self.cursor {
            s.count += output.count;
        } else {
            self.cursor = Some(output);
        }
        true
    }
}
pub use crate::vanilla::crafting::recipe;
pub fn from_nbt(tag: &crate::nbt::Tag) -> Option<Stack> {
    use crate::nbt::Tag;
    use std::sync::OnceLock;
    static IDS: OnceLock<std::collections::HashMap<String, u16>> = OnceLock::new();
    let id_tag = tag.get("id")?;
    let id = if let Some(id) = id_tag.int() {
        u16::try_from(id).ok()?
    } else {
        let ids = IDS.get_or_init(|| {
            let mut ids = std::collections::HashMap::new();
            if let Ok(bytes) = std::fs::read(".local/resources/registry.json") {
                if let Ok(registry) = serde_json::from_slice::<crate::resources::Registry>(&bytes) {
                    for (id, def) in registry.blocks.into_iter().chain(registry.items) {
                        ids.insert(def.name, id);
                    }
                }
            }
            ids
        });
        *ids.get(id_tag.string()?.trim_start_matches("minecraft:"))?
    };
    let count = tag
        .get("Count")
        .and_then(Tag::int)
        .unwrap_or(1)
        .clamp(0, 64) as u8;
    if count == 0 || id == 0 {
        return None;
    }
    let damage = tag
        .get("Damage")
        .and_then(Tag::int)
        .unwrap_or(0)
        .clamp(0, 65535) as u16;
    let durable = crate::vanilla::armor::maximum_damage(id) > 0;
    Some(Stack {
        nbt: tag.get("tag").map(item_nbt::from_tag).map(Arc::new),
        identity: crate::inventory::Stack::fresh_identity(),
        id,
        meta: if durable { 0 } else { damage },
        count: count.min(stack_limit(id)),
        damage: if durable { damage } else { 0 },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closing_drops_carried_and_crafting_items_even_with_empty_inventory() {
        let mut inv = Inventory {
            cursor: Some(Stack::new(4, 12)),
            ..Default::default()
        };
        inv.crafting[0] = Some(Stack::new(17, 3));
        inv.slots[5] = Some(Stack::new(3, 9));
        let dropped = inv.close();
        assert_eq!(dropped, vec![Stack::new(4, 12), Stack::new(17, 3)]);
        assert!(inv.cursor.is_none());
        assert!(inv.crafting.iter().all(Option::is_none));
        assert_eq!(
            inv.slots.iter().flatten().cloned().collect::<Vec<_>>(),
            vec![Stack::new(3, 9)]
        );
        assert!(inv.close().is_empty());
    }
    #[test]
    fn inventory_split_merge_preserves_count() {
        let mut i = Inventory::default();
        i.slots[0] = Some(Stack::new(4, 63));
        assert_eq!(i.add(Stack::new(4, 5)), None);
        assert_eq!(i.slots[0].as_ref().unwrap().count, 64);
        assert_eq!(i.slots[1].as_ref().unwrap().count, 4);
        i.click(0, true, false);
        assert_eq!(i.cursor.as_ref().unwrap().count, 32);
        i.click(1, true, false);
        assert_eq!(i.slots[1].as_ref().unwrap().count, 5);
        assert_eq!(i.cursor.as_ref().unwrap().count, 31);
        let before: i32 = i
            .slots
            .iter()
            .flatten()
            .map(|s| s.count as i32)
            .sum::<i32>()
            + i.cursor.as_ref().unwrap().count as i32;
        assert_eq!(before, 68);
    }
    #[test]
    fn recipes_consume_inputs_without_duplication() {
        let mut i = Inventory::default();
        i.crafting[0] = Some(Stack::new(17, 2));
        assert!(i.craft(2));
        assert_eq!(i.cursor.as_ref().unwrap().id, 5);
        assert_eq!(i.cursor.as_ref().unwrap().count, 4);
        assert_eq!(i.crafting[0].as_ref().unwrap().count, 1);
        assert!(i.craft(2));
        assert_eq!(i.cursor.as_ref().unwrap().count, 8);
        assert!(!i.craft(2));
    }
    #[test]
    fn full_inventory_returns_remainder() {
        let mut i = Inventory::default();
        for s in &mut i.slots[..36] {
            *s = Some(Stack::new(4, 64));
        }
        assert_eq!(i.add(Stack::new(3, 5)), Some(Stack::new(3, 5)));
    }
}
