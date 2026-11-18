//! Ordinary-rail callback bridge. Activated redstone is supplied by the caller;
//! generated structures start unpowered until an entity/circuit simulation acts.
use super::*;
use crate::vanilla::{
    material,
    plant_events::PlantWorld,
    rail_events::{self, RailWorld},
    spawning,
};
pub(super) fn added(world: &mut impl RailWorld, p: IVec3, old: u16, new: u16) {
    if old >> 4 != new >> 4 {
        rail_events::on_added(world, p, new);
    }
}
pub(super) fn removed(world: &mut impl RailWorld, p: IVec3, old: u16, new: u16) {
    if old >> 4 != new >> 4 {
        rail_events::on_removed(world, p, old);
    }
}
pub(super) fn changed(world: &mut impl RailWorld, p: IVec3, state: u16, source: u16) {
    rail_events::neighbor_changed(world, p, state, material::can_provide_power(source << 4));
}
impl RailWorld for Access<'_> {
    fn get(&self, p: IVec3) -> u16 {
        self.get_block(p).expect("Native rail read failed")
    }
    fn set(&mut self, p: IVec3, state: u16, flags: u8) {
        PlantWorld::set(self, p, state, flags);
    }
    fn is_powered(&self, p: IVec3) -> bool {
        let hooks = self.with(|_, b| b.hooks.clone());
        hooks.is_powered(self, p)
    }
    fn solid_top(&self, p: IVec3) -> bool {
        spawning::solid_top(RailWorld::get(self, p))
    }
    fn drop_block(&mut self, p: IVec3, state: u16) {
        BlockAccess::drop_block_item(self, p, state);
    }
    fn notify_neighbors(&mut self, p: IVec3, source: u16) {
        let mut ticks = self.with(|_, b| b.ticks.share_scheduler());
        let mut effects = ticks.notify_neighbors(self, p, source);
        self.with(|_, b| {
            b.block_effects.changed.append(&mut effects.changed);
            b.block_effects.drops.append(&mut effects.drops);
            b.block_effects.washed.append(&mut effects.washed);
        });
    }
}
impl RailWorld for structures::LoadedStructures<'_> {
    fn get(&self, p: IVec3) -> u16 {
        BlockAccess::get(self, p)
    }
    fn set(&mut self, p: IVec3, state: u16, flags: u8) {
        PlantWorld::set(self, p, state, flags);
    }
    fn is_powered(&self, _p: IVec3) -> bool {
        false
    }
    fn solid_top(&self, p: IVec3) -> bool {
        spawning::solid_top(RailWorld::get(self, p))
    }
    fn drop_block(&mut self, p: IVec3, state: u16) {
        BlockAccess::drop_block_item(self, p, state);
    }
    fn notify_neighbors(&mut self, p: IVec3, source: u16) {
        self.ticks
            .share_scheduler()
            .notify_neighbors(self, p, source);
    }
}
