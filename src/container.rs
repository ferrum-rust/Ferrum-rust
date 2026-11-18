#[cfg(test)]
use crate::inventory::Inventory;
use crate::inventory::{stack_limit, Stack};
use serde::{Deserialize, Serialize};
#[path = "inventory_window.rs"]
mod window;
pub use window::{merchant_click, window_click};
pub(crate) use window::{predict_merchant_click, predict_window_click};
pub(crate) use window::brewing_ingredient;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Chest,
    Furnace,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Special {
    #[default] None,
    Anvil,
    Enchant,
    Beacon,
    Brewing,
    Horse{armor:bool},
    Hopper,
    Dispenser,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Container {
    #[serde(skip)]
    pub(crate) brew_time:i16,
    #[serde(skip)]
    pub(crate) horse_id:Option<i32>,
    #[serde(skip)]
    pub(crate) horse_ridden:bool,
    #[serde(default)]
    pub(crate) special: Special,
    #[serde(default = "new_cooldown")]
    pub(crate) transfer_cooldown: i32,
    #[serde(skip)]
    pub(crate) anvil_locked:bool,
    #[serde(skip)]
    pub(crate) anvil_materials:u8,
    pub kind: Kind,
    pub slots: Vec<Option<Stack>>,
    pub burn: u32,
    pub burn_total: u32,
    pub cook: u32,
}
fn new_cooldown() -> i32 { -1 }
impl Container {
    pub fn for_block(id: u16) -> Option<Self> {
        let (special, slots) = match id {
            54 | 146 => return Some(Self::new(Kind::Chest)),
            61 | 62 => return Some(Self::new(Kind::Furnace)),
            23 | 158 => (Special::Dispenser, 9),
            117 => (Special::Brewing, 4),
            138 => (Special::Beacon, 1),
            154 => (Special::Hopper, 5),
            _ => return None,
        };
        let mut c = Self::new(Kind::Chest);
        c.special = special;
        c.slots.truncate(slots);
        Some(c)
    }
    pub fn new(kind: Kind) -> Self {
        Self {
            brew_time:0,
            horse_id:None,
            horse_ridden:false,
            special: Special::None,
            transfer_cooldown: new_cooldown(),
            anvil_locked:false,
            anvil_materials:0,
            kind,
            slots: vec![None; if kind == Kind::Chest { 27 } else { 3 }],
            burn: 0,
            burn_total: 0,
            cook: 0,
        }
    }
    #[cfg(test)]
    pub fn click(&mut self, inventory: &mut Inventory, index: usize, right: bool, shift: bool) {
        window_click(
            inventory,
            Some(self),
            crate::inventory::SlotRef::Container(index),
            if shift {
                crate::inventory::ClickAction::QuickMove
            } else {
                crate::inventory::ClickAction::Pickup { right }
            },
            false,
            2,
        );
    }
    pub fn tick(&mut self) {
        if self.kind != Kind::Furnace {
            return;
        }
        self.burn = self.burn.saturating_sub(1);
        let output = self.slots[0].as_ref().and_then(smelt);
        let can_smelt = output.as_ref().is_some_and(|out| {
            self.slots[2]
                .as_ref()
                .is_none_or(|s| s.same(out) && s.count + out.count <= stack_limit(s.id))
        });
        if can_smelt && self.burn == 0 {
            if let Some(f) = self.slots[1].clone() {
                let duration = fuel(&f);
                if duration > 0 {
                    self.burn = duration;
                    self.burn_total = duration;
                    consume(&mut self.slots[1]);
                    if f.id == 327 {
                        self.slots[1] = Some(Stack::new(325, 1));
                    }
                }
            }
        }
        if can_smelt && self.burn > 0 {
            self.cook += 1;
            if self.cook >= 200 {
                let output = output.unwrap();
                if self.slots[0]
                    .as_ref()
                    .is_some_and(|s| s.id == 19 && s.meta == 1)
                    && self.slots[1].as_ref().is_some_and(|s| s.id == 325)
                {
                    self.slots[1] = Some(Stack::new(326, 1));
                }
                consume(&mut self.slots[0]);
                if let Some(s) = &mut self.slots[2] {
                    s.count += output.count;
                } else {
                    self.slots[2] = Some(output);
                }
                self.cook = 0;
            }
        } else if self.burn > 0 || (self.slots[0].is_some() && self.slots[1].is_some()) {
            self.cook = 0;
        } else {
            self.cook = self.cook.saturating_sub(2);
        }
    }
}
fn consume(slot: &mut Option<Stack>) {
    if let Some(s) = slot {
        s.count -= 1;
        if s.count == 0 {
            *slot = None;
        }
    }
}
pub fn fuel(s: &Stack) -> u32 {
    match s.id {
        263 => 1600,
        173 => 16000,
        327 => 20000,
        369 => 2400,
        280 | 6 => 100,
        5
        | 17
        | 25
        | 47
        | 53
        | 54
        | 58
        | 72
        | 84
        | 85
        | 96
        | 107
        | 125
        | 134..=136
        | 143
        | 146
        | 151
        | 162..=164
        | 178
        | 183..=192
        | 425 => 300,
        126 => 150,
        268..=271 | 290 => 200,
        _ => 0,
    }
}
pub fn smelt(s: &Stack) -> Option<Stack> {
    let mut out = Stack::new(
        match s.id {
            15 => 265,
            14 => 266,
            16 => 263,
            56 => 264,
            129 => 388,
            12 => 20,
            4 => 1,
            319 => 320,
            363 => 364,
            365 => 366,
            423 => 424,
            411 => 412,
            337 => 336,
            81 => 351,
            17 | 162 => 263,
            349 => 350,
            392 => 393,
            87 => 405,
            82 => 172,
            98 if s.meta == 0 => 98,
            19 if s.meta == 1 => 19,
            73 => 331,
            21 => 351,
            153 => 406,
            _ => return None,
        },
        1,
    );
    if s.id == 81 {
        out.meta = 2;
    }
    if s.id == 21 {
        out.meta = 4;
    }
    if s.id == 98 {
        out.meta = 2;
    }
    if matches!(s.id, 17 | 162) {
        out.meta = 1;
    }
    if s.id == 349 {
        if s.meta > 1 {
            return None;
        }
        out.meta = s.meta;
    }
    Some(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_furnace_recipes_and_cook_reset_rules() {
        assert!(
            smelt(&Stack::new(1, 1)).is_none(),
            "stone cannot smelt into itself"
        );
        assert_eq!(smelt(&Stack::new(98, 1)).unwrap().meta, 2);
        assert_eq!(smelt(&Stack::new(21, 1)).unwrap().meta, 4);
        assert_eq!(
            fuel(&Stack::new(170, 1)),
            0,
            "hay is not a 1.8.9 furnace fuel"
        );
        assert_eq!(fuel(&Stack::new(47, 1)), 300);
        let mut furnace = Container::new(Kind::Furnace);
        furnace.burn = 200;
        furnace.cook = 150;
        furnace.slots[0] = Some(Stack::new(15, 1));
        furnace.slots[2] = Some(Stack::new(265, 64));
        furnace.tick();
        assert_eq!(furnace.cook, 0);
        furnace.slots[0] = Some(Stack {
            nbt: None,
            identity: crate::inventory::Stack::fresh_identity(),
            id: 19,
            meta: 1,
            count: 1,
            damage: 0,
        });
        furnace.slots[1] = Some(Stack::new(325, 1));
        furnace.slots[2] = None;
        furnace.burn = 201;
        for _ in 0..200 {
            furnace.tick();
        }
        assert_eq!(furnace.slots[2], Some(Stack::new(19, 1)));
        assert_eq!(furnace.slots[1], Some(Stack::new(326, 1)));
    }
    #[test]
    fn smelting_consumes_fuel_and_output_is_take_only() {
        let mut c = Container::new(Kind::Furnace);
        c.slots[0] = Some(Stack::new(15, 2));
        c.slots[1] = Some(Stack::new(263, 1));
        for _ in 0..400 {
            c.tick();
        }
        assert_eq!(c.slots[0], None);
        assert_eq!(c.slots[1], None);
        assert_eq!(c.slots[2], Some(Stack::new(265, 2)));
        let mut i = Inventory {
            cursor: Some(Stack::new(4, 1)),
            ..Default::default()
        };
        c.click(&mut i, 2, false, false);
        assert_eq!(c.slots[2], Some(Stack::new(265, 2)));
        i.cursor = None;
        c.click(&mut i, 2, false, true);
        assert_eq!(i.slots[8], Some(Stack::new(265, 2)));
        assert!(c.slots[2].is_none());
    }
    #[test]
    fn blocked_furnace_does_not_consume_fuel() {
        let mut c = Container::new(Kind::Furnace);
        c.slots[0] = Some(Stack::new(15, 1));
        c.slots[1] = Some(Stack::new(263, 1));
        c.slots[2] = Some(Stack::new(265, 64));
        for _ in 0..300 {
            c.tick();
        }
        assert_eq!(c.slots[1], Some(Stack::new(263, 1)));
        assert_eq!(c.cook, 0);
    }
}
