//! MerchantRecipeList, InventoryMerchant and SlotMerchantResult selection.
use super::{Offer, Trader};
use crate::{inventory::Stack, vanilla::drops::DropNbt};
use std::sync::Arc;

fn contains_required_tag(required: &DropNbt, given: &DropNbt) -> bool {
    match (required, given) {
        (DropNbt::Compound(required), DropNbt::Compound(given)) => {
            required.iter().all(|(name, tag)| {
                given
                    .get(name)
                    .is_some_and(|actual| contains_required_tag(tag, actual))
            })
        }
        // Merchant matching passes false for NBTUtil's list-subset flag.
        _ => required == given,
    }
}
pub fn matches_cost(given: &Stack, required: &Stack) -> bool {
    if given.id != required.id || given.meta != required.meta || given.damage != required.damage {
        return false;
    }
    let Some(expected) = &required.nbt else {
        return true;
    };
    given.nbt.as_ref().is_some_and(|actual| {
        (given.identity == required.identity && Arc::ptr_eq(expected, actual))
            || contains_required_tag(expected, actual)
    })
}
fn accepts(offer: &Offer, first: &Stack, second: Option<&Stack>) -> bool {
    if !matches_cost(first, &offer.cost) || first.count < offer.cost.count {
        return false;
    }
    match (&offer.second, second) {
        (Some(cost), Some(given)) => matches_cost(given, cost) && given.count >= cost.count,
        (None, None) => true,
        _ => false,
    }
}
impl Trader {
    /// Index zero, negative and out-of-range indexes search the complete list.
    /// The search returns its first match even when that recipe is sold out.
    pub fn find_recipe(
        &self,
        first: &Stack,
        second: Option<&Stack>,
        selected: i32,
    ) -> Option<usize> {
        if selected > 0 && (selected as usize) < self.offers.len() {
            let index = selected as usize;
            return accepts(&self.offers[index], first, second).then_some(index);
        }
        self.offers
            .iter()
            .position(|offer| accepts(offer, first, second))
    }
}

#[derive(Clone, Debug, Default)]
pub struct MerchantWindow {
    inputs: [Option<Stack>; 2],
    pub selected: i32,
    pub current_recipe: Option<usize>,
    pub output: Option<Stack>,
}
impl MerchantWindow {
    pub fn inputs(&self) -> &[Option<Stack>; 2] {
        &self.inputs
    }
    pub fn select(&mut self, index: i32, trader: &mut Trader) {
        self.selected = index;
        self.refresh(trader);
    }
    pub fn set_input(&mut self, slot: usize, mut stack: Option<Stack>, trader: &mut Trader) {
        if slot >= self.inputs.len() {
            return;
        }
        if let Some(stack) = &mut stack {
            stack.count = stack.count.min(64);
        }
        self.inputs[slot] = stack;
        self.refresh(trader);
    }
    pub fn remove_input(&mut self, slot: usize, count: u8, trader: &mut Trader) -> Option<Stack> {
        let input = self.inputs.get_mut(slot)?;
        let stack = input.as_mut()?;
        let result = if stack.count <= count {
            input.take()?
        } else {
            let mut split = stack.copied();
            split.count = count;
            stack.count -= count;
            split
        };
        self.refresh(trader);
        Some(result)
    }
    pub fn refresh(&mut self, trader: &mut Trader) {
        let first = self.inputs[0].as_ref().or(self.inputs[1].as_ref());
        let second = if self.inputs[0].is_some() {
            self.inputs[1].as_ref()
        } else {
            None
        };
        let available = |index: &usize| {
            let offer = &trader.offers[*index];
            offer.supported && offer.uses < offer.max_uses
        };
        self.current_recipe = first.and_then(|first| {
            trader
                .find_recipe(first, second, self.selected)
                .filter(available)
                .or_else(|| {
                    second
                        .and_then(|second| trader.find_recipe(second, Some(first), self.selected))
                        .filter(available)
                })
        });
        self.output = self
            .current_recipe
            .map(|index| trader.offers[index].output.copied());
        if let Some(sound) = trader.verify_selling_item(self.output.is_some()) {
            trader.pending.push(super::careers::TradeEffects {
                sound: Some(sound),
                ..Default::default()
            });
        }
    }
    fn deduct(&mut self, offer: &Offer, first: usize, second: usize) -> bool {
        let Some(a) = self.inputs[first].as_ref() else {
            return false;
        };
        if a.id != offer.cost.id {
            return false;
        }
        match (&self.inputs[second], &offer.second) {
            (Some(b), Some(cost)) if b.id == cost.id => {}
            (None, None) => {}
            _ => return false,
        }
        // SlotMerchantResult.doTrade checks item ID only, after the preview
        // already checked metadata and required tags. It tries slot 0 first.
        for (slot, count) in [
            (first, offer.cost.count),
            (second, offer.second.as_ref().map_or(0, |s| s.count)),
        ] {
            if let Some(stack) = &mut self.inputs[slot] {
                stack.count = stack.count.saturating_sub(count);
                if stack.count == 0 {
                    self.inputs[slot] = None;
                }
            }
        }
        true
    }
    pub fn take_output(&mut self, trader: &mut Trader) -> Option<Stack> {
        let index = self.current_recipe?;
        let offer = trader.offers.get_mut(index)?;
        if !self.deduct(offer, 0, 1) && !self.deduct(offer, 1, 0) {
            self.refresh(trader);
            return None;
        }
        let output = self.output.take()?;
        let effects = trader.use_recipe(index);
        trader.pending.push(effects);
        self.refresh(trader);
        Some(output)
    }
    pub fn close(&mut self) -> Vec<Stack> {
        // ContainerMerchant removes the two inputs without recomputing its
        // preview. The client then discards the closed window.
        self.inputs.iter_mut().filter_map(Option::take).collect()
    }
}

#[cfg(test)]
#[path = "villager_merchant_tests.rs"]
mod tests;
