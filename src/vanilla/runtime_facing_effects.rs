//! Container orientation onAdded bridge; nested writes retain original flags.
use super::*;
use crate::vanilla::{
    facing_events::{self, FacingWorld},
    plant_events::PlantWorld,
};

pub(super) fn added(world: &mut impl FacingWorld, p: IVec3, old: u16, new: u16) {
    if old >> 4 != new >> 4 {
        facing_events::added(world, p, new);
    }
}
impl FacingWorld for Access<'_> {
    fn get(&self, p: IVec3) -> u16 {
        self.get_block(p)
            .expect("Native container orientation read failed")
    }
    fn set(&mut self, p: IVec3, state: u16, flags: u8) {
        PlantWorld::set(self, p, state, flags);
    }
}
impl FacingWorld for structures::LoadedStructures<'_> {
    fn get(&self, p: IVec3) -> u16 {
        BlockAccess::get(self, p)
    }
    fn set(&mut self, p: IVec3, state: u16, flags: u8) {
        PlantWorld::set(self, p, state, flags);
    }
}
