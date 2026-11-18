//! Adapter for original vegetation neighbor callbacks, shared by recursive
//! decoration and already-loaded structure clips.
use super::*;
use crate::vanilla::plant_events::{self, PlantWorld};

pub(super) fn changed(access: &mut impl PlantWorld, p: IVec3, state: u16) {
    plant_events::neighbor_changed(access, p, state);
}
pub(super) fn added(access: &mut impl PlantWorld, p: IVec3, old: u16, new: u16) {
    if old >> 4 != new >> 4 {
        plant_events::block_added(access, p, new);
    }
}
impl PlantWorld for Access<'_> {
    fn normal_cube_or(&self, p: IVec3, fallback: bool) -> bool {
        if !valid(p)
            || self
                .provide(cp(p))
                .expect("Native torch support load failed")
                .is_none()
        {
            return fallback;
        }
        let state = self.get_block(p).expect("Native torch support read failed");
        crate::vanilla::material::opaque(state) && crate::vanilla::material::full_cube(state)
    }
    fn get(&self, p: IVec3) -> u16 {
        self.get_block(p).expect("Native plant read failed")
    }
    fn light(&self, p: IVec3) -> u8 {
        if p.y < 0 {
            return 0;
        }
        let p = IVec3::new(p.x, p.y.min(255), p.z);
        self.provide(cp(p))
            .expect("Native plant light chunk load failed");
        self.with(|_, b| b.light.unsubtracted(&b.world, p))
    }
    fn can_see_sky(&self, p: IVec3) -> bool {
        self.provide(cp(p))
            .expect("Native plant sky chunk load failed");
        self.with(|_, b| b.light.can_see_sky(p))
    }
    fn set(&mut self, p: IVec3, state: u16, flags: u8) {
        let mut ticks = self.with(|_, b| b.ticks.share_scheduler());
        let mut effects = ticks.set_block(self, p, state, flags & 1 != 0);
        self.with(|_, b| {
            b.block_effects.changed.append(&mut effects.changed);
            b.block_effects.drops.append(&mut effects.drops);
            b.block_effects.washed.append(&mut effects.washed);
        });
    }
    fn drop_block(&mut self, p: IVec3, state: u16) {
        assert!(BlockAccess::drop_block_item(self, p, state));
    }
}
impl PlantWorld for structures::LoadedStructures<'_> {
    fn get(&self, p: IVec3) -> u16 {
        BlockAccess::get(self, p)
    }
    fn light(&self, p: IVec3) -> u8 {
        self.light.unsubtracted(self.world, p)
    }
    fn can_see_sky(&self, p: IVec3) -> bool {
        self.light.can_see_sky(p)
    }
    fn set(&mut self, p: IVec3, state: u16, flags: u8) {
        let mut ticks = self.ticks.share_scheduler();
        ticks.set_block(self, p, state, flags & 1 != 0);
    }
    fn drop_block(&mut self, p: IVec3, state: u16) {
        assert!(BlockAccess::drop_block_item(self, p, state));
    }
}
