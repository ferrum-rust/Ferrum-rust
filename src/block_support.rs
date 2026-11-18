//! Unified 1.8.9 block support checks, neighbor cascades, multiblock harvest,
//! and authentic scatter-drop helpers for singleplayer simulation.
use super::{
    container_key,
    drops_local::{extra_tile_drop, tile_drops_enabled, LocalDrops},
    Mode, Outcome, Simulation, World,
};
use crate::vanilla::{
    drops::DropStack,
    hand_placement::door_item,
    item_nbt, material,
    notes::{self, SIDES},
    plant_events,
    rail_events::{self, RailWorld},
    sleep,
    spawning::solid_top,
};
use glam::IVec3;
use std::collections::HashSet;

#[inline]
pub(crate) fn is_fence(id: u16) -> bool {
    matches!(id, 85 | 113 | 188..=192)
}

#[inline]
fn loaded_or(world: &World, p: IVec3, predicate: impl FnOnce(u16) -> bool) -> bool {
    if !(0..256).contains(&p.y) {
        return false;
    }
    if !world.contains(p.as_vec3()) {
        return true;
    }
    predicate(world.get(p))
}

/// Spawns the authentic 1.8.9 item drop(s) for a block broken by support loss,
/// piston push, or fluid displacement, with vanilla 0.25..0.75 jitter and scatter.
/// Must be called BEFORE `world.set(pos, 0)` when the block owns a TileEntity.
pub(crate) fn drop_broken_block(sim: &mut Simulation, world: &World, pos: IVec3, state: u16) {
    if state == 0 || !tile_drops_enabled(sim) {
        return;
    }
    let id = state >> 4;
    let extra = extra_tile_drop(sim, world, pos, state);
    let mut random = sim.block_ticks.world_random();
    {
        let mut drops = LocalDrops::new(sim, world);
        match id {
            63 | 68 => drops.spawn(
                &mut random,
                pos,
                DropStack {
                    id: 323,
                    count: 1,
                    meta: 0,
                    nbt: None,
                },
            ),
            93 | 94 => drops.spawn(
                &mut random,
                pos,
                DropStack {
                    id: 356,
                    count: 1,
                    meta: 0,
                    nbt: None,
                },
            ),
            149 | 150 => drops.spawn(
                &mut random,
                pos,
                DropStack {
                    id: 404,
                    count: 1,
                    meta: 0,
                    nbt: None,
                },
            ),
            70 | 72 | 147 | 148 => drops.spawn(
                &mut random,
                pos,
                DropStack {
                    id,
                    count: 1,
                    meta: 0,
                    nbt: None,
                },
            ),
            140 => {
                drops.spawn(
                    &mut random,
                    pos,
                    DropStack {
                        id: 390,
                        count: 1,
                        meta: 0,
                        nbt: None,
                    },
                );
                if let Some(plant) = extra {
                    drops.spawn(&mut random, pos, plant);
                }
            }
            144 => {
                if let Some(skull) = extra {
                    drops.spawn(&mut random, pos, skull);
                }
            }
            64 | 71 | 193..=197 => {
                if state & 8 == 0 {
                    if let Some(item) = door_item(id) {
                        drops.spawn(
                            &mut random,
                            pos,
                            DropStack {
                                id: item,
                                count: 1,
                                meta: 0,
                                nbt: None,
                            },
                        );
                    }
                }
            }
            26 => {
                if state & 8 == 0 {
                    drops.spawn(
                        &mut random,
                        pos,
                        DropStack {
                            id: 355,
                            count: 1,
                            meta: 0,
                            nbt: None,
                        },
                    );
                }
            }
            _ => drops.farm(&mut random, pos, state, 0),
        }
    }
    sim.block_ticks.set_world_random(&random);
}

fn clear_block(
    sim: &mut Simulation,
    world: &mut World,
    pos: IVec3,
    old_state: u16,
    drop: bool,
    out: &mut Outcome,
) {
    if drop {
        drop_broken_block(sim, world, pos, old_state);
    }
    let id = old_state >> 4;
    if matches!(id, 63 | 68 | 140) {
        sim.containers.remove(&container_key(pos));
    }
    if matches!(id, 63 | 68 | 140 | 144 | 149 | 150 | 176 | 177) {
        sim.world_commands.edit_tile(world, pos, 0, None);
    }
    if matches!(id, 75 | 76 | 93 | 94 | 149 | 150) {
        sim.purge_circuit_timers_at(pos, id);
    }
    if world.set(pos, 0) {
        out.changed.push(pos);
        sim.block_ticks.notify(world, pos);
    }
}

struct Rails<'a> {
    sim: &'a mut Simulation,
    world: &'a mut World,
    out: &'a mut Outcome,
    notified: Vec<(IVec3, u16)>,
}

impl RailWorld for Rails<'_> {
    fn get(&self, p: IVec3) -> u16 {
        self.world.get(p)
    }

    fn set(&mut self, p: IVec3, state: u16, flags: u8) {
        if self.world.set(p, state) {
            self.out.changed.push(p);
            if flags & 1 != 0 {
                self.sim.block_ticks.notify(self.world, p);
                self.notified.push((p, state >> 4));
            }
        } else if flags & 1 != 0 {
            self.sim.block_ticks.notify(self.world, p);
            self.notified.push((p, state >> 4));
        }
    }

    fn is_powered(&self, p: IVec3) -> bool {
        notes::powered(self.world, p, |q| {
            item_nbt::number(
                self.world
                    .tile(q)
                    .and_then(item_nbt::compound)
                    .and_then(|v| v.get("OutputSignal")),
            )
        })
    }

    fn solid_top(&self, p: IVec3) -> bool {
        loaded_or(self.world, p, solid_top)
    }

    fn drop_block(&mut self, p: IVec3, state: u16) {
        drop_broken_block(self.sim, self.world, p, state);
    }

    fn notify_neighbors(&mut self, p: IVec3, source: u16) {
        self.sim.block_ticks.notify(self.world, p);
        self.notified.push((p, source));
    }
}

fn flush_rail_notifications(adapter: &mut Rails<'_>) {
    let mut steps = 0usize;
    while !adapter.notified.is_empty() && steps < 128 {
        let batch = std::mem::take(&mut adapter.notified);
        for (center, source_id) in batch {
            let source_power = material::can_provide_power(source_id << 4);
            for d in SIDES {
                let q = center + d;
                if !(0..256).contains(&q.y) || !adapter.world.contains(q.as_vec3()) {
                    continue;
                }
                let qs = adapter.world.get(q);
                if matches!(qs >> 4, 27 | 157) || (qs >> 4 == 66 && source_power) {
                    steps += 1;
                    rail_events::neighbor_changed(&mut *adapter, q, qs, source_power);
                    if steps >= 128 {
                        break;
                    }
                }
            }
            if steps >= 128 {
                break;
            }
        }
    }
}

fn check_rail_at(
    sim: &mut Simulation,
    world: &mut World,
    pos: IVec3,
    state: u16,
    source_can_provide_power: bool,
    out: &mut Outcome,
) {
    let mut adapter = Rails {
        sim,
        world,
        out,
        notified: Vec::new(),
    };
    rail_events::neighbor_changed(&mut adapter, pos, state, source_can_provide_power);
    flush_rail_notifications(&mut adapter);
}

pub(crate) fn on_rail_added(
    sim: &mut Simulation,
    world: &mut World,
    pos: IVec3,
    out: &mut Outcome,
) {
    if !(0..256).contains(&pos.y) || !world.contains(pos.as_vec3()) {
        return;
    }
    let state = world.get(pos);
    if !rail_events::is_rail(state) {
        return;
    }
    {
        let mut adapter = Rails {
            sim,
            world,
            out,
            notified: Vec::new(),
        };
        rail_events::on_added(&mut adapter, pos, state);
        flush_rail_notifications(&mut adapter);
    }
    if world.get(pos) >> 4 == 28 {
        sim.update_detector_rail(world, pos, out);
    }
}

pub(crate) fn on_rail_removed(
    sim: &mut Simulation,
    world: &mut World,
    pos: IVec3,
    old_state: u16,
    out: &mut Outcome,
) {
    if !rail_events::is_rail(old_state) {
        return;
    }
    {
        let mut adapter = Rails {
            sim,
            world,
            out,
            notified: Vec::new(),
        };
        rail_events::on_removed(&mut adapter, pos, old_state);
        flush_rail_notifications(&mut adapter);
    }
    let shape = if old_state >> 4 == 66 {
        old_state & 15
    } else {
        old_state & 7
    };
    let source_power = material::can_provide_power(old_state);
    let mut centers = vec![pos, pos - IVec3::Y];
    if (2..=5).contains(&shape) {
        centers.push(pos + IVec3::Y);
    }
    let mut visited = HashSet::new();
    for center in centers {
        for offset in std::iter::once(IVec3::ZERO).chain(SIDES) {
            let q = center + offset;
            if !(0..256).contains(&q.y) || !world.contains(q.as_vec3()) || !visited.insert(q) {
                continue;
            }
            let qs = world.get(q);
            if rail_events::is_rail(qs) {
                check_rail_at(sim, world, q, qs, source_power, out);
            }
        }
    }
}

pub(crate) fn notify_rail_power(
    sim: &mut Simulation,
    world: &mut World,
    pos: IVec3,
    out: &mut Outcome,
) {
    if !(0..256).contains(&pos.y) || !world.contains(pos.as_vec3()) {
        return;
    }
    let state = world.get(pos);
    if !rail_events::is_rail(state) {
        return;
    }
    let mut adapter = Rails {
        sim,
        world,
        out,
        notified: Vec::new(),
    };
    rail_events::neighbor_changed(&mut adapter, pos, state, true);
    flush_rail_notifications(&mut adapter);
}

struct PredictRails<'a> {
    world: &'a mut World,
    modified: Vec<IVec3>,
}

impl RailWorld for PredictRails<'_> {
    fn get(&self, p: IVec3) -> u16 {
        self.world.get(p)
    }

    fn set(&mut self, p: IVec3, state: u16, _flags: u8) {
        self.world.set(p, state);
        if !self.modified.contains(&p) {
            self.modified.push(p);
        }
    }

    fn is_powered(&self, _p: IVec3) -> bool {
        false
    }

    fn solid_top(&self, p: IVec3) -> bool {
        loaded_or(self.world, p, solid_top)
    }

    fn drop_block(&mut self, _p: IVec3, _state: u16) {}

    fn notify_neighbors(&mut self, _p: IVec3, _source: u16) {}
}

pub(crate) fn predict_rail_added(world: &mut World, pos: IVec3) -> Vec<IVec3> {
    if !(0..256).contains(&pos.y) || !world.contains(pos.as_vec3()) {
        return Vec::new();
    }
    let state = world.get(pos);
    if !rail_events::is_rail(state) {
        return Vec::new();
    }
    let mut adapter = PredictRails {
        world,
        modified: Vec::new(),
    };
    rail_events::on_added(&mut adapter, pos, state);
    adapter.modified
}

/// Checks whether the block at `pos` still has valid attachment/floor/partner
/// support in `world`; if unsupported, drops its authentic 1.8.9 item(s), cleans
/// up any TileEntity/container state, sets `pos` to air, and records `out.changed`.
pub(crate) fn check_support_at(
    sim: &mut Simulation,
    world: &mut World,
    pos: IVec3,
    out: &mut Outcome,
) {
    check_support_at_from(sim, world, pos, false, out);
}

fn check_support_at_from(
    sim: &mut Simulation,
    world: &mut World,
    pos: IVec3,
    source_can_provide_power: bool,
    out: &mut Outcome,
) {
    if !(0..256).contains(&pos.y) || !world.contains(pos.as_vec3()) {
        return;
    }
    let state = world.get(pos);
    if state == 0 {
        return;
    }
    let id = state >> 4;
    let meta = (state & 15) as u8;

    if plant_events::supports(id) {
        sim.check_plant_at(world, pos, out);
        return;
    }

    match id {
        // Pressure plates: Stone (70), Wood (72), Light Weighted (147), Heavy Weighted (148)
        70 | 72 | 147 | 148 => {
            let below = world.get(pos - IVec3::Y);
            if !solid_top(below) && !is_fence(below >> 4) {
                clear_block(sim, world, pos, state, true, out);
            }
        }
        // Carpets (171)
        171 => {
            if world.get(pos - IVec3::Y) >> 4 == 0 {
                clear_block(sim, world, pos, state, true, out);
            }
        }
        // Rails: Powered (27), Detector (28), Normal (66), Activator (157)
        27 | 28 | 66 | 157 => {
            check_rail_at(sim, world, pos, state, source_can_provide_power, out);
        }
        // Buttons: Stone (77), Wood (143)
        77 | 143 => {
            let supported = match meta & 7 {
                0 => loaded_or(world, pos + IVec3::Y, material::normal_cube),
                1 => loaded_or(world, pos - IVec3::X, material::normal_cube),
                2 => loaded_or(world, pos + IVec3::X, material::normal_cube),
                3 => loaded_or(world, pos - IVec3::Z, material::normal_cube),
                4 => loaded_or(world, pos + IVec3::Z, material::normal_cube),
                5 => solid_top(world.get(pos - IVec3::Y)),
                _ => false,
            };
            if !supported {
                clear_block(sim, world, pos, state, true, out);
            }
        }
        // Ladder (65)
        65 => {
            let supported = match meta {
                2 => loaded_or(world, pos + IVec3::Z, material::normal_cube),
                3 => loaded_or(world, pos - IVec3::Z, material::normal_cube),
                4 => loaded_or(world, pos + IVec3::X, material::normal_cube),
                5 => loaded_or(world, pos - IVec3::X, material::normal_cube),
                _ => false,
            };
            if !supported {
                clear_block(sim, world, pos, state, true, out);
            }
        }
        // Trapdoors: Wood (96), Iron (167)
        96 | 167 => {
            let dir = [IVec3::Z, IVec3::NEG_Z, IVec3::X, IVec3::NEG_X][usize::from(meta & 3)];
            let supported = loaded_or(world, pos + dir, |n| {
                (material::opaque(n) && material::full_cube(n))
                    || matches!(
                        n >> 4,
                        43 | 44
                            | 53
                            | 67
                            | 89
                            | 108
                            | 109
                            | 114
                            | 125
                            | 126
                            | 128
                            | 134..=136
                            | 156
                            | 163
                            | 164
                            | 180..=182
                    )
            });
            if !supported {
                clear_block(sim, world, pos, state, true, out);
            }
        }
        // Tripwire Hook (131)
        131 => {
            let attached_dir =
                [IVec3::NEG_Z, IVec3::X, IVec3::Z, IVec3::NEG_X][usize::from(meta & 3)];
            let supported = loaded_or(world, pos + attached_dir, material::normal_cube);
            if !supported {
                if meta & 12 != 0 {
                    for step in 1..42 {
                        let q = pos - attached_dir * step;
                        if !world.contains(q.as_vec3()) {
                            break;
                        }
                        let qs = world.get(q);
                        if qs >> 4 == 131 && (qs & 3) == ((u16::from(meta) & 3) ^ 2) {
                            if world.set(q, qs & 3) {
                                out.changed.push(q);
                                sim.block_ticks.notify(world, q);
                            }
                            break;
                        } else if qs >> 4 == 132 {
                            if qs & 4 != 0 && world.set(q, qs & !4) {
                                out.changed.push(q);
                                sim.block_ticks.notify(world, q);
                            }
                        } else {
                            break;
                        }
                    }
                }
                clear_block(sim, world, pos, state, true, out);
            }
        }
        // Standing Sign (63)
        63 => {
            if !material::is_solid(world.get(pos - IVec3::Y)) {
                clear_block(sim, world, pos, state, true, out);
            }
        }
        // Wall Sign (68)
        68 => {
            let supported = match meta {
                2 => loaded_or(world, pos + IVec3::Z, material::is_solid),
                3 => loaded_or(world, pos - IVec3::Z, material::is_solid),
                4 => loaded_or(world, pos + IVec3::X, material::is_solid),
                5 => loaded_or(world, pos - IVec3::X, material::is_solid),
                _ => false,
            };
            if !supported {
                clear_block(sim, world, pos, state, true, out);
            }
        }
        // Flower Pot (140)
        140 => {
            let below = world.get(pos - IVec3::Y);
            if !solid_top(below) && !is_fence(below >> 4) {
                clear_block(sim, world, pos, state, true, out);
            }
        }
        // Bed partner-half check (26)
        26 => {
            let other = sleep::other_half(pos, state);
            if world.contains(other.as_vec3()) && world.get(other) >> 4 != 26 {
                clear_block(sim, world, pos, state, state & 8 == 0, out);
            }
        }
        // Standing Banner (176)
        176 => {
            if !material::is_solid(world.get(pos - IVec3::Y)) {
                clear_block(sim, world, pos, state, true, out);
            }
        }
        // Wall Banner (177)
        177 => {
            let supported = match meta {
                2 => loaded_or(world, pos + IVec3::Z, material::is_solid),
                3 => loaded_or(world, pos - IVec3::Z, material::is_solid),
                4 => loaded_or(world, pos + IVec3::X, material::is_solid),
                5 => loaded_or(world, pos - IVec3::X, material::is_solid),
                _ => false,
            };
            if !supported {
                clear_block(sim, world, pos, state, true, out);
            }
        }
        // Redstone Wire (55)
        55 => {
            let below = world.get(pos - IVec3::Y);
            if !solid_top(below) && below >> 4 != 89 {
                clear_block(sim, world, pos, state, true, out);
            }
        }
        // Lever (69)
        69 => {
            let f = [0, 5, 4, 3, 2, 1, 1, 0][usize::from(meta & 7)];
            let q = pos - SIDES[f];
            let supported = if f == 1 {
                solid_top(world.get(q))
            } else {
                loaded_or(world, q, material::normal_cube)
            };
            if !supported {
                clear_block(sim, world, pos, state, true, out);
            }
        }
        // Redstone Torches (75, 76)
        75 | 76 => {
            let f = [1, 5, 4, 3, 2, 1, 1, 1][usize::from(meta & 7)];
            let q = pos - SIDES[f];
            let supported = if f == 1 {
                let u = world.get(q);
                solid_top(u) || matches!(u >> 4, 20 | 85 | 95 | 113 | 139 | 188..=192)
            } else {
                loaded_or(world, q, |u| material::opaque(u) && material::full_cube(u))
            };
            if !supported {
                clear_block(sim, world, pos, state, true, out);
            }
        }
        // Repeaters (93, 94) & Comparators (149, 150)
        93 | 94 | 149 | 150 => {
            if !solid_top(world.get(pos - IVec3::Y)) {
                clear_block(sim, world, pos, state, true, out);
            }
        }
        // Doors (64, 71, 193..=197)
        64 | 71 | 193..=197 => {
            let lower = if meta & 8 != 0 { pos - IVec3::Y } else { pos };
            let upper = lower + IVec3::Y;
            let base = world.get(lower);
            if base >> 4 != id {
                let top = world.get(upper);
                if top >> 4 == id {
                    clear_block(sim, world, upper, top, false, out);
                }
            } else {
                let top = world.get(upper);
                let unsupported = !solid_top(world.get(lower - IVec3::Y));
                if top >> 4 != id || unsupported {
                    clear_block(sim, world, lower, base, unsupported, out);
                    if top >> 4 == id {
                        clear_block(sim, world, upper, top, false, out);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Checks support at `source` and all 6 orthogonal neighbors of `source`.
#[allow(dead_code)]
pub(crate) fn check_neighbors_support(
    sim: &mut Simulation,
    world: &mut World,
    source: IVec3,
    out: &mut Outcome,
) {
    let source_power = if (0..256).contains(&source.y) && world.contains(source.as_vec3()) {
        material::can_provide_power(world.get(source))
    } else {
        false
    };
    check_support_at_from(sim, world, source, source_power, out);
    for d in SIDES {
        check_support_at_from(sim, world, source + d, source_power, out);
    }
}

/// Handles player mining of multiblock structures (upper half of DoublePlant `175`,
/// upper half of Doors `64, 71, 193..=197`, Bed `26` head half, and Creative breaks
/// of either half) so partner halves are cleared cleanly with zero double drops
/// and zero Creative/invalid-tool drops.
///
/// Works whether called before or after `world.set(pos, 0)`. Returns `true` when
/// it handled clearing the partner half and spawning any applicable Survival drop.
pub(crate) fn on_player_harvest_multiblock(
    sim: &mut Simulation,
    world: &mut World,
    pos: IVec3,
    state: u16,
    can_harvest: bool,
    out: &mut Outcome,
) -> bool {
    let id = state >> 4;
    match id {
        175 => {
            if state & 8 != 0 {
                let lower_pos = pos - IVec3::Y;
                let lower = world.get(lower_pos);
                if sim.mode == Mode::Survival && lower >> 4 == 175 && lower & 8 == 0 {
                    let sheared = sim.inventory.held().is_some_and(|s| s.id == 359)
                        && matches!(lower & 7, 2 | 3);
                    if sheared {
                        if tile_drops_enabled(sim) {
                            let mut random = sim.block_ticks.world_random();
                            let meta = if lower & 7 == 2 { 1 } else { 2 };
                            LocalDrops::new(sim, world).spawn(
                                &mut random,
                                lower_pos,
                                DropStack {
                                    id: 31,
                                    count: 2,
                                    meta,
                                    nbt: None,
                                },
                            );
                            sim.block_ticks.set_world_random(&random);
                        }
                    } else {
                        sim.drop_farm_block(world, lower_pos, lower, 0);
                    }
                }
                if world.get(pos) >> 4 == 175 && world.set(pos, 0) {
                    out.changed.push(pos);
                    sim.block_ticks.notify(world, pos);
                }
                if lower >> 4 == 175 && world.set(lower_pos, 0) {
                    out.changed.push(lower_pos);
                    sim.block_ticks.notify(world, lower_pos);
                }
                true
            } else if sim.mode == Mode::Creative {
                let upper_pos = pos + IVec3::Y;
                if world.get(pos) >> 4 == 175 && world.set(pos, 0) {
                    out.changed.push(pos);
                    sim.block_ticks.notify(world, pos);
                }
                if world.get(upper_pos) >> 4 == 175 && world.set(upper_pos, 0) {
                    out.changed.push(upper_pos);
                    sim.block_ticks.notify(world, upper_pos);
                }
                true
            } else {
                false
            }
        }
        64 | 71 | 193..=197 => {
            if state & 8 != 0 {
                let lower_pos = pos - IVec3::Y;
                let had_lower = world.get(lower_pos) >> 4 == id;
                if world.get(pos) >> 4 == id && world.set(pos, 0) {
                    out.changed.push(pos);
                    sim.block_ticks.notify(world, pos);
                }
                if had_lower && world.set(lower_pos, 0) {
                    out.changed.push(lower_pos);
                    sim.block_ticks.notify(world, lower_pos);
                }
                if sim.mode == Mode::Survival && can_harvest && had_lower && tile_drops_enabled(sim)
                {
                    if let Some(item) = door_item(id) {
                        let mut random = sim.block_ticks.world_random();
                        LocalDrops::new(sim, world).spawn(
                            &mut random,
                            pos,
                            DropStack {
                                id: item,
                                count: 1,
                                meta: 0,
                                nbt: None,
                            },
                        );
                        sim.block_ticks.set_world_random(&random);
                    }
                }
                true
            } else if sim.mode == Mode::Creative {
                let upper_pos = pos + IVec3::Y;
                if world.get(pos) >> 4 == id && world.set(pos, 0) {
                    out.changed.push(pos);
                    sim.block_ticks.notify(world, pos);
                }
                if world.get(upper_pos) >> 4 == id && world.set(upper_pos, 0) {
                    out.changed.push(upper_pos);
                    sim.block_ticks.notify(world, upper_pos);
                }
                true
            } else {
                false
            }
        }
        26 => {
            if state & 8 != 0 || sim.mode == Mode::Creative {
                let other = sleep::other_half(pos, state);
                let had_other = world.get(other) >> 4 == 26;
                if world.get(pos) >> 4 == 26 && world.set(pos, 0) {
                    out.changed.push(pos);
                    sim.block_ticks.notify(world, pos);
                }
                if had_other && world.set(other, 0) {
                    out.changed.push(other);
                    sim.block_ticks.notify(world, other);
                }
                if sim.mode == Mode::Survival && had_other && tile_drops_enabled(sim) {
                    let mut random = sim.block_ticks.world_random();
                    LocalDrops::new(sim, world).spawn(
                        &mut random,
                        other,
                        DropStack {
                            id: 355,
                            count: 1,
                            meta: 0,
                            nbt: None,
                        },
                    );
                    sim.block_ticks.set_world_random(&random);
                }
                true
            } else {
                false
            }
        }
        _ => false,
    }
}
