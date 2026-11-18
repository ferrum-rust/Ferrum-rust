//! Original vegetation support checks and neighbor-change write order.
//! Drop generation, world lighting and recursive neighbor delivery are caller-owned.
use super::material;
use glam::IVec3;
const HORIZONTAL: [IVec3; 4] = [IVec3::NEG_Z, IVec3::X, IVec3::Z, IVec3::NEG_X];
pub trait PlantWorld {
    fn get(&self, p: IVec3) -> u16;
    /// World.getLight(pos), without night-time sky subtraction.
    fn light(&self, p: IVec3) -> u8;
    fn can_see_sky(&self, p: IVec3) -> bool;
    fn set(&mut self, p: IVec3, state: u16, flags: u8);
    fn drop_block(&mut self, p: IVec3, state: u16);
    fn normal_cube_or(&self, p: IVec3, fallback: bool) -> bool {
        if !(0..256).contains(&p.y)
            || !(-30_000_000..30_000_000).contains(&p.x)
            || !(-30_000_000..30_000_000).contains(&p.z)
        {
            return fallback;
        }
        let state = self.get(p);
        material::opaque(state) && material::full_cube(state)
    }
    fn destroy(&mut self, p: IVec3) {
        let state = self.get(p);
        if state >> 4 == 0 {
            return;
        }
        self.drop_block(p, state);
        self.set(p, 0, 3);
    }
}
pub fn supports(id: u16) -> bool {
    matches!(id,6|31|32|37..=40|50|59|60|78|81|83|104..=106|111|115|127|141|142|175)
}
fn soil(state: u16) -> bool {
    matches!(state >> 4, 2 | 3 | 60)
}
fn vine_face(state: u16) -> bool {
    material::full_cube(state) && material::blocks_movement(state)
}
fn torch_stays(world: &impl PlantWorld, p: IVec3, state: u16) -> bool {
    let attached = match state & 15 {
        1 => IVec3::NEG_X,
        2 => IVec3::X,
        3 => IVec3::NEG_Z,
        4 => IVec3::Z,
        _ => IVec3::NEG_Y,
    };
    if attached != IVec3::NEG_Y {
        return world.normal_cube_or(p + attached, true);
    }
    let below = world.get(p - IVec3::Y);
    super::spawning::solid_top(below) || matches!(below >> 4, 20 | 85 | 95 | 113 | 139 | 188..=192)
}
fn check_torch(world: &mut impl PlantWorld, p: IVec3, state: u16) -> bool {
    if state >> 4 == 50 && torch_stays(world, p, state) {
        return true;
    }
    if world.get(p) >> 4 == 50 {
        world.drop_block(p, state);
        world.set(p, 0, 3);
    }
    false
}
/// Ordinary torches have an immediate placement support check. Redstone
/// torch power/scheduling belongs to its distinct subclass callbacks.
pub fn block_added(world: &mut impl PlantWorld, p: IVec3, state: u16) -> bool {
    if state >> 4 != 50 {
        return false;
    }
    check_torch(world, p, state);
    true
}
pub fn can_stay(world: &impl PlantWorld, p: IVec3, state: u16) -> bool {
    match state >> 4 {
        6 | 31 | 37 | 38 => soil(world.get(p - IVec3::Y)),
        32 => matches!(world.get(p - IVec3::Y) >> 4, 3 | 12 | 159 | 172),
        39 | 40 => {
            if !(0..256).contains(&p.y) {
                return false;
            }
            let below = world.get(p - IVec3::Y);
            below >> 4 == 110
                || below == (3 << 4 | 2)
                || (world.light(p) < 13 && material::full_block(below))
        }
        50 => torch_stays(world, p, state),
        59 | 141 | 142 => {
            (world.light(p) >= 8 || world.can_see_sky(p)) && world.get(p - IVec3::Y) >> 4 == 60
        }
        60 => !material::is_solid(world.get(p + IVec3::Y)),
        78 => {
            let below = world.get(p - IVec3::Y);
            !matches!(below >> 4, 79 | 174)
                && (material::is_leaves(below)
                    || (below >> 4 == 78 && below & 15 >= 6)
                    || (material::opaque_cube(below) && material::blocks_movement(below)))
        }
        81 => {
            !HORIZONTAL
                .iter()
                .any(|&d| material::is_solid(world.get(p + d)))
                && matches!(world.get(p - IVec3::Y) >> 4, 12 | 81)
        }
        83 => {
            let below = world.get(p - IVec3::Y);
            if below >> 4 == 83 {
                return true;
            }
            matches!(below >> 4, 2 | 3 | 12)
                && HORIZONTAL
                    .iter()
                    .any(|&d| matches!(world.get(p + d - IVec3::Y) >> 4, 8 | 9))
        }
        104 | 105 => world.get(p - IVec3::Y) >> 4 == 60,
        111 => (0..256).contains(&p.y) && matches!(world.get(p - IVec3::Y), 128 | 144),
        115 => world.get(p - IVec3::Y) >> 4 == 88,
        127 => {
            let facing = [IVec3::Z, IVec3::NEG_X, IVec3::NEG_Z, IVec3::X][(state & 3) as usize];
            let log = world.get(p + facing);
            log >> 4 == 17 && log & 3 == 3
        }
        175 => {
            if state & 8 != 0 {
                world.get(p - IVec3::Y) >> 4 == 175
            } else {
                world.get(p + IVec3::Y) >> 4 == 175 && soil(world.get(p - IVec3::Y))
            }
        }
        _ => true,
    }
}
/// Returns false for block classes outside this module's vegetation scope.
pub fn neighbor_changed(world: &mut impl PlantWorld, p: IVec3, state: u16) -> bool {
    let id = state >> 4;
    if !supports(id) {
        return false;
    }
    if id == 50 {
        if check_torch(world, p, state) && !torch_stays(world, p, state) {
            world.drop_block(p, state);
            world.set(p, 0, 3);
        }
        return true;
    }
    if id == 106 {
        let mut meta = state & 15;
        for (d, bit) in [
            (IVec3::NEG_Z, 4),
            (IVec3::X, 8),
            (IVec3::Z, 1),
            (IVec3::NEG_X, 2),
        ] {
            if meta & bit == 0 || vine_face(world.get(p + d)) {
                continue;
            }
            let above = world.get(p + IVec3::Y);
            if above >> 4 != 106 || above & bit == 0 {
                meta &= !bit;
            }
        }
        if meta == 0 {
            world.drop_block(p, state);
            world.set(p, 0, 3);
        } else if meta != state & 15 {
            world.set(p, (106 << 4) | meta, 2);
        }
        return true;
    }
    if can_stay(world, p, state) {
        return true;
    }
    match id {
        60 => world.set(p, 3 << 4, 3),
        81 => world.destroy(p),
        127 => {
            world.set(p, 0, 3);
            world.drop_block(p, state);
        }
        175 => {
            let upper = state & 8 != 0;
            let top = if upper { p } else { p + IVec3::Y };
            let bottom = if upper { p - IVec3::Y } else { p };
            let top_present = upper || world.get(top) >> 4 == 175;
            let bottom_present = !upper || world.get(bottom) >> 4 == 175;
            if top_present {
                world.set(top, 0, 2);
            }
            if bottom_present {
                world.set(bottom, 0, 3);
                if !upper {
                    world.drop_block(bottom, state);
                }
            }
        }
        _ => {
            world.drop_block(p, state);
            world.set(p, 0, 3);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vanilla::{
        drops::{self, DropEvent, DropTile, DropWorld},
        random::JavaRandom,
    };
    use serde::Deserialize;
    use std::collections::HashMap;
    #[derive(Deserialize, Debug)]
    struct Case {
        state: u16,
        below: u16,
        above: u16,
        mask: u8,
        light: u8,
        sky: bool,
        y: i32,
        writes: Vec<[i32; 5]>,
        world_next: String,
    }
    struct Fixture {
        blocks: HashMap<IVec3, u16>,
        light: u8,
        sky: bool,
        writes: Vec<[i32; 5]>,
        random: JavaRandom,
    }
    impl PlantWorld for Fixture {
        fn get(&self, p: IVec3) -> u16 {
            self.blocks.get(&p).copied().unwrap_or(0)
        }
        fn light(&self, _p: IVec3) -> u8 {
            self.light
        }
        fn can_see_sky(&self, _p: IVec3) -> bool {
            self.sky
        }
        fn set(&mut self, p: IVec3, state: u16, flags: u8) {
            if !(0..256).contains(&p.y) {
                return;
            }
            self.writes
                .push([p.x, p.y, p.z, state as i32, flags as i32]);
            self.blocks.insert(p, state);
        }
        fn drop_block(&mut self, p: IVec3, state: u16) {
            let mut random = self.random.share();
            drops::drop_block(self, &mut random, p, state, 1., 0).unwrap();
        }
    }
    impl DropWorld for Fixture {
        fn is_remote(&self) -> bool {
            false
        }
        fn do_tile_drops(&self) -> bool {
            false
        }
        fn tile_entity(&mut self, _p: IVec3) -> anyhow::Result<Option<DropTile>> {
            Ok(None)
        }
        fn emit(&mut self, event: DropEvent) -> anyhow::Result<()> {
            panic!("doTileDrops=false plant emitted {event:?}")
        }
    }
    #[test]
    fn vegetation_neighbor_conditions_and_ordered_writes_match_original_bytecode() {
        let cases: Vec<Case> = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-plant-events.json"
        ))
        .unwrap();
        assert_eq!(cases.len(), 50244);
        for case in cases {
            let p = IVec3::new(0, case.y, 0);
            let mut world = Fixture {
                blocks: HashMap::from([
                    (p, case.state),
                    (p - IVec3::Y, case.below),
                    (p + IVec3::Y, case.above),
                ]),
                light: case.light,
                sky: case.sky,
                writes: Vec::new(),
                random: JavaRandom::new(189),
            };
            for (i, d) in HORIZONTAL.into_iter().enumerate() {
                if case.mask & (1 << i) != 0 {
                    world.blocks.insert(p + d, (17 << 4) | 3);
                    world.blocks.insert(p + d - IVec3::Y, 9 << 4);
                }
            }
            assert!(neighbor_changed(&mut world, p, case.state));
            assert_eq!(world.writes, case.writes, "{case:?}");
            assert_eq!(
                world.random.next_long().to_string(),
                case.world_next,
                "{case:?} world RNG"
            );
        }
    }
    #[test]
    fn torch_added_and_neighbor_callbacks_match_every_original_support_state() {
        #[derive(Deserialize, Debug)]
        struct Torch {
            state: u16,
            support: u16,
            mask: u8,
            y: i32,
            added: bool,
            removed: bool,
            writes: Vec<[i32; 5]>,
            world_next: String,
        }
        let cases: Vec<Torch> = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-torch-events.json"
        ))
        .unwrap();
        assert_eq!(cases.len(), 14180);
        for case in cases {
            let p = IVec3::new(0, case.y, 0);
            let mut world = Fixture {
                blocks: HashMap::from([
                    (p, if case.removed { 0 } else { case.state }),
                    (p - IVec3::Y, case.support),
                ]),
                light: 15,
                sky: true,
                writes: Vec::new(),
                random: JavaRandom::new(189),
            };
            for (i, d) in HORIZONTAL.into_iter().enumerate() {
                if case.mask & (1 << i) != 0 {
                    world.blocks.insert(p + d, case.support);
                }
            }
            if case.added {
                assert!(block_added(&mut world, p, case.state));
            } else {
                assert!(neighbor_changed(&mut world, p, case.state));
            }
            assert_eq!(world.writes, case.writes, "{case:?}");
            assert_eq!(
                world.random.next_long().to_string(),
                case.world_next,
                "{case:?} world RNG"
            );
        }
    }
}
