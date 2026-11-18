//! Local implementation of 1.8.9 Container.slotClick modes 0..6.
//! Slot addressing is independent of GUI coordinates and packet slot numbers.
use super::{fuel, smelt, Container, Kind, Special};
use crate::inventory::{
    accepts_armor, armor_slot, recipe, stack_limit, ClickAction, Inventory, SlotRef, Stack,
};
use crate::villagers::{merchant::MerchantWindow, Trader};

#[cfg(test)]
#[path = "extra_windows_tests.rs"]
mod extra_windows_tests;
#[cfg(test)]
#[path = "inventory_identity_tests.rs"]
mod identity_tests;

struct Window<'a> {
    inventory: &'a mut Inventory,
    container: Option<&'a mut Container>,
    merchant: Option<(&'a mut MerchantWindow, &'a mut Trader)>,
    width: usize,
    creative:bool,
    drops:Vec<Stack>,
}
pub(crate) fn brewing_ingredient(stack:&Stack)->bool{
    matches!(stack.id,289|331|348|353|370|372|375|376|377|378|382|396|414)
        || matches!(stack.id,349|350) && stack.meta==3
}
fn potion_bottle(stack:&Stack)->bool{matches!(stack.id,373|374)}
impl Window<'_> {
    fn locked(&self,slot:SlotRef)->bool{slot==SlotRef::Container(2) && self.container.as_ref().is_some_and(|c|c.special==Special::Anvil && c.anvil_locked)}
    fn personal(&self) -> bool {
        self.container.is_none() && self.merchant.is_none()
    }
    fn get(&self, slot: SlotRef) -> Option<Stack> {
        if let (Some((window, _)), SlotRef::Container(i)) = (&self.merchant, slot) {
            return match i {
                0 | 1 => window.inputs()[i].clone(),
                2 => window.output.clone(),
                _ => None,
            };
        }
        match slot {
            SlotRef::Player(i) => self.inventory.slots.get(i).cloned().flatten(),
            SlotRef::Craft(i) if self.personal() && i < self.width * self.width => {
                self.inventory.crafting.get(i).cloned().flatten()
            }
            SlotRef::Container(i) => self
                .container
                .as_ref()
                .and_then(|c| c.slots.get(i))
                .cloned()
                .flatten(),
            SlotRef::CraftOutput if self.personal() => {
                recipe(&self.inventory.crafting, self.width).map(|r| r.output)
            }
            _ => None,
        }
    }
    fn set(&mut self, slot: SlotRef, value: Option<Stack>) {
        if let (Some((window, trader)), SlotRef::Container(i)) = (&mut self.merchant, slot) {
            if i < 2 {
                window.set_input(i, value.filter(|s| s.count > 0), trader);
            }
            return;
        }
        if slot == SlotRef::Container(0) {
            if let Some(c) = &mut self.container {
                if c.kind == Kind::Furnace
                    && !c.slots[0]
                        .as_ref()
                        .zip(value.as_ref())
                        .is_some_and(|(a, b)| a.same(b))
                {
                    c.cook = 0;
                }
            }
        }
        let target = match slot {
            SlotRef::Player(i) => self.inventory.slots.get_mut(i),
            SlotRef::Craft(i) if self.personal() && i < self.width * self.width => {
                self.inventory.crafting.get_mut(i)
            }
            SlotRef::Container(i) => self.container.as_mut().and_then(|c| c.slots.get_mut(i)),
            _ => None,
        };
        if let Some(target) = target {
            *target = value.filter(|s| s.count > 0);
        }
    }
    fn valid(&self, slot: SlotRef, item: &Stack) -> bool {
        if let (Some(_), SlotRef::Container(i)) = (&self.merchant, slot) {
            return i < 2;
        }
        match slot {
            SlotRef::Player(i) if i < 36 => true,
            SlotRef::Player(i) if i < 40 && self.personal() => accepts_armor(i, item),
            SlotRef::Craft(i) => self.personal() && i < self.width * self.width,
            SlotRef::Container(i) => self.container.as_ref().is_some_and(|c| {
                match c.special {
                    Special::Anvil=>return i<2,
                    Special::Enchant=>return i==0 || i==1 && item.id==351 && item.meta==4,
                    Special::Beacon=>return i==0 && matches!(item.id,264|265|266|388),
                    Special::Brewing=>return if i<3{potion_bottle(item)}else{i==3 && brewing_ingredient(item)},
                    Special::Horse{armor}=>return match i{0=>item.id==329 && c.slots.first().is_some_and(Option::is_none),1=>armor && c.slots.len()>=2 && matches!(item.id,417..=419),_=>i<c.slots.len()},
                    Special::None|Special::Dispenser|Special::Hopper=>()
                }
                i < c.slots.len()
                    && (c.kind == Kind::Chest
                        || i == 0
                        || (i == 1 && (fuel(item) > 0 || item.id == 325)))
            }),
            _ => false,
        }
    }
    fn limit(&self, slot: SlotRef, item: &Stack) -> u8 {
        if matches!(slot, SlotRef::Player(36..=39))
            || slot==SlotRef::Container(0) && self.container.as_ref().is_some_and(|c|matches!(c.special,Special::Enchant|Special::Beacon))
            || matches!(slot,SlotRef::Container(0..=2)) && self.container.as_ref().is_some_and(|c|c.special==Special::Brewing)
            || (slot == SlotRef::Container(1)
                && item.id == 325
                && self
                    .container
                    .as_ref()
                    .is_some_and(|c| c.kind == Kind::Furnace))
        {
            1
        } else {
            stack_limit(item.id)
        }
    }
    fn slots(&self) -> Vec<SlotRef> {
        let mut slots = Vec::new();
        if self.merchant.is_some() {
            slots.extend((0..3).map(SlotRef::Container));
        } else if let Some(c) = &self.container {
            slots.extend((0..c.slots.len()).map(SlotRef::Container));
        } else {
            slots.extend((0..self.width * self.width).map(SlotRef::Craft));
            if self.width == 2 {
                slots.extend((36..40).rev().map(SlotRef::Player));
            }
        }
        slots.extend((9..36).chain(0..9).map(SlotRef::Player));
        slots
    }
    fn take(&mut self, slot: SlotRef, count: u8) -> Option<Stack> {
        if let (Some((window, trader)), SlotRef::Container(i)) = (&mut self.merchant, slot) {
            return match i {
                0 | 1 => window.remove_input(i, count, trader),
                2 => window.take_output(trader),
                _ => None,
            };
        }
        let item = self.get(slot)?;
        if slot==SlotRef::Container(2) {
            if let Some(c)=self.container.as_mut().filter(|c|c.special==Special::Anvil) {
                c.slots[0]=None;
                if let Some(material)=c.slots[1].as_mut(){
                    if c.anvil_materials>0 && material.count>c.anvil_materials {material.count-=c.anvil_materials;}else{c.slots[1]=None;}
                }
                c.slots[2]=None;
                c.anvil_materials=0;
                return Some(item);
            }
        }
        if slot == SlotRef::CraftOutput {
            // Recipe output is one indivisible crafting operation.
            let r = recipe(&self.inventory.crafting, self.width)?;
            self.drops.extend(crate::vanilla::crafting::consume(self.inventory,r,self.creative));
            return Some(item);
        }
        let n = count.min(item.count);
        self.set(
            slot,
            (n < item.count).then_some(Stack {
                count: item.count - n,
                ..item.clone()
            }),
        );
        Some(Stack {
            count: n,
            ..if n < item.count { item.copied() } else { item }
        })
    }
    fn merge(&mut self, mut item: Stack, slots: &[SlotRef]) -> Option<Stack> {
        for pass in 0..2 {
            for &slot in slots {
                if !self.valid(slot, &item) {
                    continue;
                }
                let existing = self.get(slot);
                // Container.mergeItemStack uses item limits for shift-clicks, including
                // stacks of glass bottles entering a brewing slot with a normal click limit of one.
                let limit=if matches!(slot,SlotRef::Container(0..=2)) && self.container.as_ref().is_some_and(|c|c.special==Special::Brewing){stack_limit(item.id)}else{self.limit(slot,&item)};
                let moved = match existing {
                    Some(s) if pass == 0 && s.same(&item) => {
                        let n = limit
                            .saturating_sub(s.count)
                            .min(item.count);
                        self.set(
                            slot,
                            Some(Stack {
                                count: s.count + n,
                                ..s
                            }),
                        );
                        n
                    }
                    None if pass == 1 => {
                        let n = limit.min(item.count);
                        self.set(
                            slot,
                            Some(Stack {
                                count: n,
                                ..item.copied()
                            }),
                        );
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
    fn quick_move(&mut self, slot: SlotRef) -> bool {
        let Some(item) = self.get(slot) else { return false };
        let main = || (9..36).chain(0..9).map(SlotRef::Player).collect::<Vec<_>>();
        let reverse_main = || {
            (0..9)
                .rev()
                .chain((9..36).rev())
                .map(SlotRef::Player)
                .collect::<Vec<_>>()
        };
        if self.merchant.is_some() {
            let targets = match slot {
                SlotRef::Container(2) => reverse_main(),
                SlotRef::Container(0 | 1) => main(),
                SlotRef::Player(0..9) => (9..36).map(SlotRef::Player).collect(),
                SlotRef::Player(9..36) => (0..9).map(SlotRef::Player).collect(),
                _ => return false,
            };
            return if slot == SlotRef::Container(2) {
                let mut moved = false;
                while let Some(output) = self.get(slot).filter(|next| next.id == item.id) {
                    let count = output.count;
                    if self
                        .merge(output, &targets)
                        .is_some_and(|rest| rest.count == count)
                    {
                        break;
                    }
                    // ContainerMerchant charges one trade even on a partial
                    // merge. retrySlotClick repeats while the result item matches.
                    self.take(slot, count);
                    moved = true;
                }
                moved
            } else {
                let count = item.count;
                let rest = self.merge(item, &targets);
                let moved = rest.as_ref().is_none_or(|s| s.count != count);
                self.set(slot, rest);
                moved
            };
        }
        let targets = match slot {
            SlotRef::Container(i) => {
                if self
                    .container
                    .as_ref()
                    .is_some_and(|c| c.kind == Kind::Furnace || c.special==Special::Anvil)
                    && i != 2
                {
                    main()
                } else {
                    reverse_main()
                }
            }
            SlotRef::Craft(_) | SlotRef::Player(36..=39) => main(),
            SlotRef::CraftOutput => reverse_main(),
            SlotRef::Player(i) if i < 36 => {
                if let Some(c) = &self.container {
                    if c.special == Special::Brewing {
                        if c.slots[3].is_none() && brewing_ingredient(&item){vec![SlotRef::Container(3)]}
                        else if potion_bottle(&item){(0..3).map(SlotRef::Container).collect()}
                        else if i<9{(9..36).map(SlotRef::Player).collect()}else{(0..9).map(SlotRef::Player).collect()}
                    }else if let Special::Horse{armor}=c.special{
                        if armor && matches!(item.id,417..=419) && c.slots.get(1).is_some_and(Option::is_none){vec![SlotRef::Container(1)]}
                        else if item.id==329 && c.slots.first().is_some_and(Option::is_none){vec![SlotRef::Container(0)]}
                        else{(2..c.slots.len()).map(SlotRef::Container).collect()}
                    }else if c.special == Special::Anvil{
                        vec![SlotRef::Container(0),SlotRef::Container(1)]
                    }else if c.special == Special::Enchant{
                        if item.id==351 && item.meta==4{vec![SlotRef::Container(1)]}
                        else if c.slots[0].is_none(){
                            let mut single=item.copied();single.count=1;if item.count>1{single.nbt=None;}
                            self.set(SlotRef::Container(0),Some(single));self.take(slot,1);return true
                        }else{return false}
                    }else if c.special == Special::Beacon{
                        if item.count==1 && c.slots[0].is_none() && self.valid(SlotRef::Container(0),&item){vec![SlotRef::Container(0)]}
                        else if i<9{(9..36).map(SlotRef::Player).collect()}else{(0..9).map(SlotRef::Player).collect()}
                    }else if c.kind == Kind::Chest {
                        (0..c.slots.len()).map(SlotRef::Container).collect()
                    } else if smelt(&item).is_some() {
                        vec![SlotRef::Container(0)]
                    } else if fuel(&item) > 0 {
                        vec![SlotRef::Container(1)]
                    } else if i < 9 {
                        (9..36).map(SlotRef::Player).collect()
                    } else {
                        (0..9).map(SlotRef::Player).collect()
                    }
                } else if self.width == 2
                    && armor_slot(item.id).is_some_and(|a| self.inventory.slots[a].is_none())
                {
                    vec![SlotRef::Player(armor_slot(item.id).unwrap())]
                } else if i < 9 {
                    (9..36).map(SlotRef::Player).collect()
                } else {
                    (0..9).map(SlotRef::Player).collect()
                }
            }
            _ => return false,
        };
        if slot == SlotRef::CraftOutput {
            // transferStackInSlot charges the recipe on a partial merge.
            // retrySlotClick compares the remaining result's Item instance.
            let mut moved = false;
            while let Some(output)=self.get(slot).filter(|s|s.id==item.id){
                let count=output.count;
                if self.merge(output,&targets).is_some_and(|left|left.count==count){
                    break;
                }
                self.take(slot,count);
                moved = true;
            }
            moved
        } else {
            let count = item.count;
            let rest = self.merge(item, &targets);
            let moved = rest.as_ref().is_none_or(|s| s.count != count);
            if moved && slot==SlotRef::Container(2) && self.container.as_ref().is_some_and(|c|c.special==Special::Anvil){
                self.take(slot,count);
            } else { self.set(slot, rest); }
            moved
        }
    }
}

pub fn window_click(
    inventory: &mut Inventory,
    container: Option<&mut Container>,
    slot: SlotRef,
    action: ClickAction,
    creative: bool,
    crafting_width: usize,
) -> Vec<Stack> {
    predict_window_click(inventory, container, slot, action, creative, crafting_width).0
}
pub(crate) fn predict_window_click(
    inventory: &mut Inventory, container: Option<&mut Container>, slot: SlotRef,
    action: ClickAction, creative: bool, crafting_width: usize,
) -> (Vec<Stack>, bool) {
    let w = Window {
        inventory,
        container,
        merchant: None,
        width: crafting_width.clamp(2, 3),
        creative,drops:Vec::new(),
    };
    click(w, slot, action, creative)
}
pub fn merchant_click(
    inventory: &mut Inventory,
    merchant: &mut MerchantWindow,
    trader: &mut Trader,
    slot: SlotRef,
    action: ClickAction,
    creative: bool,
) -> Vec<Stack> {
    predict_merchant_click(inventory, merchant, trader, slot, action, creative).0
}
pub(crate) fn predict_merchant_click(
    inventory: &mut Inventory, merchant: &mut MerchantWindow, trader: &mut Trader,
    slot: SlotRef, action: ClickAction, creative: bool,
) -> (Vec<Stack>, bool) {
    let w = Window {
        inventory,
        container: None,
        merchant: Some((merchant, trader)),
        width: 0,
        creative,drops:Vec::new(),
    };
    click(w, slot, action, creative)
}
fn click(mut w: Window<'_>, slot: SlotRef, action: ClickAction, creative: bool) -> (Vec<Stack>, bool) {
    let mut transferred = false;
    if w.locked(slot) && !matches!(action,ClickAction::Clone|ClickAction::Collect{..}|ClickAction::Drag{..}){return (w.drops,false)}
    match action {
        ClickAction::Pickup { right } if slot == SlotRef::Outside => {
            if let Some(mut c) = w.inventory.cursor.take() {
                let n = if right { 1 } else { c.count };
                w.drops.push(Stack {
                    count: n,
                    ..if right { c.copied() } else { c.clone() }
                });
                c.count -= n;
                w.inventory.cursor = (c.count > 0).then_some(c);
            }
        }
        ClickAction::Pickup { right } => match (w.inventory.cursor.clone(), w.get(slot)) {
            (None, Some(s)) => {
                w.inventory.cursor =
                    w.take(slot, if right { s.count.div_ceil(2) } else { s.count });
            }
            (Some(mut c), None) if w.valid(slot, &c) => {
                let n = (if right { 1 } else { c.count }).min(w.limit(slot, &c));
                w.set(
                    slot,
                    Some(Stack {
                        count: n,
                        ..c.copied()
                    }),
                );
                c.count -= n;
                w.inventory.cursor = (c.count > 0).then_some(c);
            }
            (Some(mut c), Some(mut s)) if w.valid(slot, &c) => {
                if c.same(&s) {
                    let n = w.limit(slot, &c).saturating_sub(s.count).min(if right {
                        1
                    } else {
                        c.count
                    });
                    s.count += n;
                    c.count -= n;
                    w.set(slot, Some(s));
                    w.inventory.cursor = (c.count > 0).then_some(c);
                } else if c.count <= w.limit(slot, &c) {
                    w.set(slot, Some(c));
                    w.inventory.cursor = Some(s);
                }
            }
            (Some(mut c), Some(s))
                if c.same(&s) && c.count as u16 + s.count as u16 <= stack_limit(c.id) as u16 =>
            {
                if let Some(taken) = w.take(slot, s.count) {
                    c.count += taken.count;
                    w.inventory.cursor = Some(c);
                }
            }
            _ => {}
        },
        ClickAction::QuickMove => transferred = w.quick_move(slot),
        ClickAction::Hotbar(button) if button < 9 && slot != SlotRef::Player(button) => {
            let hotbar = w.inventory.slots[button].clone();
            let clicked = w.get(slot);
            let direct = hotbar.is_none()
                || (matches!(slot, SlotRef::Player(_))
                    && hotbar
                        .as_ref()
                        .is_some_and(|s| w.valid(slot, s)));
            let empty = w.inventory.slots[..36].iter().position(Option::is_none);
            if let Some(clicked) = clicked {
                if direct || empty.is_some() {
                    w.inventory.slots[button] = Some(clicked.copied());
                    if direct {
                        w.take(slot, clicked.count);
                        w.set(slot, hotbar);
                    } else if let Some(previous) = hotbar {
                        // The original returns the old hotbar stack before
                        // SlotCrafting distributes buckets and other leftovers.
                        w.inventory.add(previous);
                        w.take(slot, clicked.count);
                        w.set(slot,None);
                    }
                }
            } else if let Some(hotbar) = hotbar.filter(|s| w.valid(slot, s))
            {
                w.inventory.slots[button] = None;
                w.set(slot, Some(hotbar));
            }
        }
        ClickAction::Clone if creative && w.inventory.cursor.is_none() => {
            w.inventory.cursor = w.get(slot).map(|s| Stack {
                count: stack_limit(s.id),
                ..s.copied()
            });
        }
        ClickAction::Throw { all } if w.inventory.cursor.is_none() => {
            if let Some(s) = w.get(slot) {
                if let Some(item) = w.take(slot, if all { s.count } else { 1 }) {
                    w.drops.push(item);
                }
            }
        }
        ClickAction::Collect { reverse } if slot != SlotRef::Outside && (w.get(slot).is_none() || w.locked(slot)) => {
            if let Some(mut cursor) = w.inventory.cursor.clone() {
                let mut slots = w.slots();
                if reverse {
                    slots.reverse();
                }
                for pass in 0..2 {
                    for &slot in &slots {
                        if w.locked(slot){continue}
                        if let Some(s) = w.get(slot).filter(|s| s.same(&cursor)) {
                            if pass == 0 && s.count == stack_limit(s.id) {
                                continue;
                            }
                            let n = stack_limit(cursor.id)
                                .saturating_sub(cursor.count)
                                .min(s.count);
                            if n > 0 {
                                w.take(slot, n);
                                cursor.count += n;
                            }
                        }
                    }
                }
                w.inventory.cursor = Some(cursor);
            }
        }
        ClickAction::Drag { mode, slots } if mode < 2 || (mode == 2 && creative) => {
            if let Some(mut cursor) = w.inventory.cursor.clone() {
                let mut selected = Vec::new();
                for slot in slots {
                    if !selected.contains(&slot)
                        && selected.len() < cursor.count as usize
                        && w.valid(slot, &cursor)
                        && w.get(slot)
                            .is_none_or(|s| s.same(&cursor) && s.count <= stack_limit(s.id))
                    {
                        selected.push(slot);
                    }
                }
                if !selected.is_empty() {
                    let n = match mode {
                        0 => cursor.count / selected.len() as u8,
                        1 => 1,
                        _ => stack_limit(cursor.id),
                    };
                    let mut left = cursor.count as i32;
                    for slot in selected {
                        let existing = w.get(slot).map_or(0, |s| s.count);
                        let target =
                            (existing as u16 + n as u16).min(w.limit(slot, &cursor) as u16) as u8;
                        left -= (target - existing) as i32;
                        w.set(
                            slot,
                            Some(Stack {
                                count: target,
                                ..cursor.copied()
                            }),
                        );
                    }
                    cursor.count = left.max(0) as u8;
                    w.inventory.cursor = (cursor.count > 0).then(|| cursor.copied());
                }
            }
        }
        _ => {}
    }
    (w.drops, transferred)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn count(
        inventory: &Inventory,
        chest: &Container,
        dropped: &[Stack],
    ) -> std::collections::BTreeMap<(u16, u16, u16), u32> {
        let mut totals = std::collections::BTreeMap::new();
        for s in inventory
            .slots
            .iter()
            .chain(&inventory.crafting)
            .chain(std::iter::once(&inventory.cursor))
            .chain(&chest.slots)
            .flatten()
            .chain(dropped)
        {
            *totals.entry((s.id, s.meta, s.damage)).or_default() += s.count as u32;
        }
        totals
    }
    #[test]
    fn chest_hotbar_mode_moves_old_hotbar_to_inventory_instead_of_chest() {
        // Container.slotClick mode2: external inventory + occupied hotbar uses getFirstEmptyStack.
        let mut inv = Inventory::default();
        inv.slots[0] = Some(Stack::new(3, 8));
        let mut chest = Container::new(Kind::Chest);
        chest.slots[0] = Some(Stack::new(4, 12));
        window_click(
            &mut inv,
            Some(&mut chest),
            SlotRef::Container(0),
            ClickAction::Hotbar(0),
            false,
            2,
        );
        assert_eq!(inv.slots[0], Some(Stack::new(4, 12)));
        assert_eq!(inv.slots[1], Some(Stack::new(3, 8)));
        assert!(chest.slots[0].is_none());
    }
    #[test]
    fn quick_move_uses_vanilla_window_order_and_armor_limits() {
        let mut inv = Inventory::default();
        let mut chest = Container::new(Kind::Chest);
        chest.slots[0] = Some(Stack::new(4, 12));
        window_click(
            &mut inv,
            Some(&mut chest),
            SlotRef::Container(0),
            ClickAction::QuickMove,
            false,
            2,
        );
        assert_eq!(inv.slots[8], Some(Stack::new(4, 12)));
        inv.slots[39] = Some(Stack::new(310, 1));
        window_click(
            &mut inv,
            None,
            SlotRef::Player(39),
            ClickAction::QuickMove,
            false,
            2,
        );
        assert_eq!(inv.slots[9], Some(Stack::new(310, 1)));
        inv.cursor = Some(Stack::new(86, 16));
        window_click(
            &mut inv,
            None,
            SlotRef::Player(39),
            ClickAction::Pickup { right: false },
            false,
            2,
        );
        assert_eq!(inv.slots[39], Some(Stack::new(86, 1)));
        assert_eq!(inv.cursor, Some(Stack::new(86, 15)));
    }
    #[test]
    fn drag_distribution_uses_initial_count_and_leaves_overflow_on_cursor() {
        let mut inv = Inventory {
            cursor: Some(Stack::new(4, 10)),
            ..Default::default()
        };
        inv.slots[10] = Some(Stack::new(4, 63));
        window_click(
            &mut inv,
            None,
            SlotRef::Outside,
            ClickAction::Drag {
                mode: 0,
                slots: vec![SlotRef::Player(9), SlotRef::Player(10), SlotRef::Player(11)],
            },
            false,
            2,
        );
        assert_eq!(inv.slots[9].as_ref().unwrap().count, 3);
        assert_eq!(inv.slots[10].as_ref().unwrap().count, 64);
        assert_eq!(inv.slots[11].as_ref().unwrap().count, 3);
        assert_eq!(inv.cursor.as_ref().unwrap().count, 3);
    }
    #[test]
    fn furnace_output_takes_all_or_nothing_with_cursor_and_bucket_fuel_limit_is_one() {
        let mut inv = Inventory {
            cursor: Some(Stack::new(265, 63)),
            ..Default::default()
        };
        let mut furnace = Container::new(Kind::Furnace);
        furnace.slots[2] = Some(Stack::new(265, 2));
        window_click(
            &mut inv,
            Some(&mut furnace),
            SlotRef::Container(2),
            ClickAction::Pickup { right: false },
            false,
            2,
        );
        assert_eq!(inv.cursor.as_ref().unwrap().count, 63);
        assert_eq!(furnace.slots[2].as_ref().unwrap().count, 2);
        inv.cursor = Some(Stack::new(325, 16));
        window_click(
            &mut inv,
            Some(&mut furnace),
            SlotRef::Container(1),
            ClickAction::Pickup { right: false },
            false,
            2,
        );
        assert_eq!(furnace.slots[1], Some(Stack::new(325, 1)));
        assert_eq!(inv.cursor.as_ref().unwrap().count, 15);
    }
    #[test]
    fn five_thousand_mixed_container_actions_conserve_every_item() {
        let mut inv = Inventory::default();
        let mut chest = Container::new(Kind::Chest);
        chest.slots.resize(54, None);
        for i in 0..36 {
            inv.slots[i] =
                (i % 3 == 0).then_some(Stack::new(3 + (i % 2) as u16, 1 + (i * 7 % 64) as u8));
        }
        for i in 0..54 {
            chest.slots[i] =
                (i % 4 == 0).then_some(Stack::new(3 + (i % 2) as u16, 1 + (i * 11 % 64) as u8));
        }
        let expected = count(&inv, &chest, &[]);
        let mut dropped = Vec::new();
        let mut rng = 189u64;
        for _ in 0..5000 {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let slot = if rng & 1 == 0 {
                SlotRef::Player((rng as usize >> 8) % 36)
            } else {
                SlotRef::Container((rng as usize >> 8) % 54)
            };
            let action = match (rng >> 2) % 7 {
                0 => ClickAction::Pickup {
                    right: rng & 2 != 0,
                },
                1 => ClickAction::QuickMove,
                2 => ClickAction::Hotbar(((rng >> 16) % 9) as usize),
                3 => ClickAction::Throw { all: rng & 2 != 0 },
                4 => ClickAction::Collect {
                    reverse: rng & 2 != 0,
                },
                5 => ClickAction::Drag {
                    mode: (rng & 1) as u8,
                    slots: vec![
                        slot,
                        SlotRef::Player((rng as usize >> 18) % 36),
                        SlotRef::Container((rng as usize >> 24) % 54),
                    ],
                },
                _ => ClickAction::Pickup { right: false },
            };
            dropped.extend(window_click(
                &mut inv,
                Some(&mut chest),
                slot,
                action,
                false,
                2,
            ));
            assert_eq!(count(&inv, &chest, &dropped), expected);
            for s in inv
                .slots
                .iter()
                .chain(&chest.slots)
                .chain(std::iter::once(&inv.cursor))
                .flatten()
            {
                assert!(s.count > 0 && s.count <= stack_limit(s.id));
            }
        }
    }
}
