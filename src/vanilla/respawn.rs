//! Overworld player creation columns, bed exit search and respawn collision lift.
use super::{
    collision_shapes::BlockView,
    material,
    movement::{collision, Aabb, MovementWorld},
    random::JavaRandom,
    spawning::solid_top,
};
use crate::{
    nbt::Tag,
    player_save::{nbt_boolean, nbt_integer},
    world::World,
};
use glam::{DVec3, IVec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BedSpawn {
    pub point: IVec3,
    pub forced: bool,
}
impl BedSpawn {
    pub fn read(player: &Tag) -> Option<Self> {
        Some(Self {
            point: IVec3::new(
                nbt_integer(player.get("SpawnX"))?,
                nbt_integer(player.get("SpawnY"))?,
                nbt_integer(player.get("SpawnZ"))?,
            ),
            forced: nbt_boolean(player.get("SpawnForced")),
        })
    }
    pub fn exit(self, world: &impl BlockView) -> Option<IVec3> {
        let p = self.point;
        let state = world.get(p);
        if state >> 4 != 26 {
            return (self.forced
                && can_spawn_in_block(state)
                && can_spawn_in_block(world.get(p.wrapping_add(IVec3::Y))))
            .then_some(p);
        }
        bed_exit(world, p, state, 0)
    }
}
fn can_spawn_in_block(state: u16) -> bool {
    matches!(state >> 4, 63 | 68 | 70 | 72 | 147 | 148 | 176 | 177)
        || !material::is_solid(state) && !material::is_liquid(state)
}
fn bed_exit(world: &impl BlockView, head: IVec3, state: u16, mut skip: u32) -> Option<IVec3> {
    let facing = [IVec3::Z, IVec3::NEG_X, IVec3::NEG_Z, IVec3::X][(state & 3) as usize];
    for origin in [head, head.wrapping_sub(facing)] {
        for dx in -1..=1 {
            for dz in -1..=1 {
                let p = origin.wrapping_add(IVec3::new(dx, 0, dz));
                if solid_top(world.get(p.wrapping_sub(IVec3::Y)))
                    && !material::is_solid(world.get(p))
                    && !material::is_solid(world.get(p.wrapping_add(IVec3::Y)))
                {
                    if skip == 0 {
                        return Some(p);
                    }
                    skip -= 1;
                }
            }
        }
    }
    None
}

pub(crate) fn top_column(world: &impl BlockView, p: IVec3, top_section: usize) -> i32 {
    let mut y = (top_section as i32 + 1) * 16;
    while y >= 0 {
        let state = world.get(IVec3::new(p.x, y - 1, p.z));
        if material::blocks_movement(state) && !material::is_leaves(state) {
            break;
        }
        y -= 1;
    }
    y
}
pub(crate) fn lift(
    world: &impl MovementWorld,
    mut feet: DVec3,
    limit: f64,
    mut observe: impl FnMut(usize),
) -> DVec3 {
    loop {
        let count = collision::colliders(world, Aabb::player(feet)).len();
        observe(count);
        if count == 0 || feet.y >= limit {
            return feet;
        }
        feet.y += 1.;
    }
}
pub(crate) fn offset_column(
    p: &mut IVec3,
    radius: i32,
    random: &mut JavaRandom,
) -> anyhow::Result<()> {
    let bound = radius
        .checked_mul(2)
        .ok_or_else(|| anyhow::anyhow!("Invalid player spawn radius"))?;
    p.x = p.x.wrapping_add(random.next_int(bound) - radius);
    p.z = p.z.wrapping_add(random.next_int(bound) - radius);
    Ok(())
}
/// Construct the new player first, then try the previous entity's saved bed
/// and apply the final ServerConfigurationManager collision check.
pub fn recreate(
    world: &World,
    spawn: IVec3,
    bed: &mut Option<BedSpawn>,
    random: &mut JavaRandom,
) -> [f64; 3] {
    let border = 29_999_984i64 - i64::from(spawn.x).abs().max(i64::from(spawn.z).abs());
    let mut p = spawn;
    offset_column(&mut p, border.clamp(1, 10) as i32, random)
        .expect("bounded default overworld radius");
    let chunk = world.chunks.get(&(p.x >> 4, p.z >> 4));
    let top_section = chunk.map_or(0, |c| {
        let block_section = (c.top.saturating_sub(1).max(0) >> 4) as usize;
        let light_section = c
            .light
            .as_ref()
            .and_then(|l| l.sections.iter().rposition(|&v| v))
            .unwrap_or(0);
        block_section.max(light_section)
    });
    p.y = top_column(world, p, top_section);
    let mut feet = lift(world, p.as_dvec3() + DVec3::new(0.5, 0., 0.5), 255., |_| {});
    if let Some(saved) = *bed {
        match saved.exit(world) {
            Some(exit) => {
                // The original server adds these offsets in float before widening.
                feet = (exit.as_vec3() + glam::Vec3::new(0.5, 0.1, 0.5)).as_dvec3();
            }
            None => *bed = None,
        }
    }
    lift(world, feet, 256., |_| {}).to_array()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    #[derive(Default)]
    struct Grid(HashMap<IVec3, u16>);
    impl BlockView for Grid {
        fn get(&self, p: IVec3) -> u16 {
            self.0.get(&p).copied().unwrap_or_default()
        }
    }
    type Layout = (u16, Vec<u16>, Vec<Option<[i32; 3]>>, Option<[i32; 3]>);
    #[test]
    fn original_bed_and_forced_spawns_match_all_states_and_ordered_searches() {
        let (states, layouts): (Vec<(u16, u8)>, Vec<Layout>) = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-bed-spawn.json"
        ))
        .unwrap();
        assert_eq!((states.len(), layouts.len()), (1394, 512));
        let point = IVec3::new(-16, 72, 15);
        let bed = BedSpawn {
            point,
            forced: true,
        };
        let mut grid = Grid::default();
        for (state, result) in states {
            for (bit, offset) in [(0, IVec3::ZERO), (1, IVec3::Y)] {
                grid.0.clear();
                grid.0.insert(point + offset, state);
                assert_eq!(
                    bed.exit(&grid).is_some(),
                    result & (1 << bit) != 0,
                    "state {state} offset {offset}"
                );
            }
        }
        for (index, (meta, cells, exits, expected)) in layouts.into_iter().enumerate() {
            grid.0.clear();
            let mut cells = cells.into_iter();
            for y in -1..=1 {
                for x in -2..=2 {
                    for z in -2..=2 {
                        grid.0
                            .insert(point + IVec3::new(x, y, z), cells.next().unwrap());
                    }
                }
            }
            let state = 26 << 4 | meta;
            grid.0.insert(point, state);
            for (skip, expected) in exits.into_iter().enumerate() {
                assert_eq!(
                    bed_exit(&grid, point, state, skip as u32).map(|p| p.to_array()),
                    expected,
                    "layout {index} skip {skip}"
                );
            }
            let bed = BedSpawn {
                point,
                forced: false,
            };
            assert_eq!(
                bed.exit(&grid).map(|p| p.to_array()),
                expected,
                "layout {index}"
            );
        }
    }
}
