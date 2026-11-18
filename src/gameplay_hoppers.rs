//! TileEntityHopper updates for the local 20 Hz world.
use super::{
    container_key, container_parts,
    vehicles::{LocalMinecart, MinecartKind},
    Simulation,
};
use crate::{
    container::{self, Container, Kind, Special},
    inventory::{stack_limit, Stack},
    vanilla::{entity_pick, movement::Aabb, notes::SIDES},
    world::World,
};
use glam::{DVec3, IVec3};

pub(crate) fn insert_into_slots(slots: &mut [Option<Stack>], stack: &mut Stack) -> bool {
    let before = stack.count;
    for slot in slots.iter_mut() {
        match slot {
            Some(current) if current.same(stack) => {
                let moved = stack
                    .count
                    .min(stack_limit(stack.id).saturating_sub(current.count));
                current.count += moved;
                stack.count -= moved;
            }
            None => {
                *slot = Some(stack.copied());
                stack.count = 0;
            }
            _ => (),
        }
        if stack.count == 0 {
            break;
        }
    }
    before != stack.count
}

fn container_cart_at(minecarts: &[LocalMinecart], pos: IVec3) -> Option<usize> {
    let min = pos.as_dvec3();
    let cell = Aabb {
        min,
        max: min + DVec3::ONE,
    };
    minecarts.iter().position(|cart| {
        matches!(cart.kind, MinecartKind::Chest | MinecartKind::Hopper)
            && cell.intersects(entity_pick::body(cart.position, 0.98, 0.7))
    })
}

fn insert(c: &mut Container, stack: &mut Stack, from: usize) -> bool {
    let before = stack.count;
    for (i, slot) in c.slots.iter_mut().enumerate() {
        let valid = if c.kind == Kind::Furnace {
            from == 1 && i == 0
                || from >= 2 && i == 1 && (container::fuel(stack) > 0 || stack.id == 325)
        } else {
            match c.special {
                Special::Brewing if from == 1 => i == 3 && container::brewing_ingredient(stack),
                Special::Brewing => i < 3 && matches!(stack.id, 373 | 374),
                Special::Beacon => matches!(stack.id, 264 | 265 | 266 | 388),
                _ => true,
            }
        };
        if !valid {
            continue;
        }
        match slot {
            Some(current) if current.same(stack) => {
                let moved = stack
                    .count
                    .min(stack_limit(stack.id).saturating_sub(current.count));
                current.count += moved;
                stack.count -= moved;
            }
            None => {
                *slot = Some(stack.copied());
                stack.count = 0;
            }
            _ => (),
        }
        if stack.count == 0 {
            break;
        }
    }
    let changed = before != stack.count;
    if changed && c.special == Special::Hopper && c.transfer_cooldown <= 1 {
        c.transfer_cooldown = 8;
    }
    changed
}

impl Simulation {
    /// Original Anvil tiles can predate this simulation's support for their container type.
    pub(crate) fn live_container(&mut self, w: &World, p: IVec3) -> &mut Container {
        self.containers.entry(container_key(p)).or_insert_with(|| {
            let id = w.get(p) >> 4;
            let chunk = &w.chunks[&(p.x.div_euclid(16), p.z.div_euclid(16))];
            if w.tile(p).is_some() {
                if let Some((_, c)) = chunk.containers.iter().find(|(q, _)| *q == p) {
                    return c.clone();
                }
            }
            Container::for_block(id).expect("inventory position verified by container_parts")
        })
    }
    fn hopper_transfer(
        &mut self,
        w: &World,
        source: &[IVec3],
        target: &[IVec3],
        from: usize,
    ) -> bool {
        for &p in source {
            let c = self.live_container(w, p);
            if c.slots.iter().all(Option::is_none) {
                continue;
            }
            // Items leave a furnace through getSlotsForFace(DOWN), output first and then empty/water buckets.
            let slots: Vec<usize> = if c.kind == Kind::Furnace {
                vec![2, 1]
            } else if c.special == Special::Brewing {
                vec![0, 1, 2]
            } else {
                (0..c.slots.len()).collect()
            };
            for i in slots {
                let c = self.live_container(w, p);
                let Some(stack) = c.slots[i].as_ref() else {
                    continue;
                };
                if c.kind == Kind::Furnace && i == 1 && !matches!(stack.id, 325 | 326) {
                    continue;
                }
                let mut unit = stack.copied();
                unit.count = 1;
                for &q in target {
                    if insert(self.live_container(w, q), &mut unit, from) {
                        let slot = &mut self.live_container(w, p).slots[i];
                        let original = slot.as_mut().unwrap();
                        original.count -= 1;
                        if original.count == 0 {
                            *slot = None;
                        }
                        self.block_ticks.notify(w, p);
                        self.block_ticks.notify(w, q);
                        return true;
                    }
                }
            }
        }
        false
    }
    fn hopper_pickup(&mut self, w: &World, p: IVec3) -> bool {
        let min = (p + IVec3::Y).as_dvec3();
        let area = Aabb {
            min,
            max: min + DVec3::ONE,
        };
        let c = self.containers.get_mut(&container_key(p)).unwrap();
        for item in &mut self.drops {
            if item.dead || !area.intersects(item.bounds) {
                continue;
            }
            if insert(c, &mut item.stack, 1) {
                self.block_ticks.notify(w, p);
            }
            if item.stack.count == 0 {
                item.dead = true;
                return true;
            }
        }
        false
    }
    fn hopper_push_minecart(&mut self, w: &World, p: IVec3, cart_idx: usize) -> bool {
        let len = self.live_container(w, p).slots.len();
        for i in 0..len {
            let Some(stack) = self.live_container(w, p).slots[i].as_ref() else {
                continue;
            };
            let mut unit = stack.copied();
            unit.count = 1;
            if insert_into_slots(&mut self.minecarts[cart_idx].slots, &mut unit) {
                if self.minecarts[cart_idx].kind == MinecartKind::Hopper
                    && self.minecarts[cart_idx].transfer_cooldown <= 0
                {
                    self.minecarts[cart_idx].transfer_cooldown = 4;
                }
                let slot = &mut self.live_container(w, p).slots[i];
                let original = slot.as_mut().unwrap();
                original.count -= 1;
                if original.count == 0 {
                    *slot = None;
                }
                self.block_ticks.notify(w, p);
                return true;
            }
        }
        false
    }
    fn hopper_pull_minecart(&mut self, w: &World, p: IVec3, cart_idx: usize) -> bool {
        let len = self.minecarts[cart_idx].slots.len();
        for i in 0..len {
            let Some(stack) = self.minecarts[cart_idx].slots[i].as_ref() else {
                continue;
            };
            let mut unit = stack.copied();
            unit.count = 1;
            if insert(self.live_container(w, p), &mut unit, 1) {
                let slot = &mut self.minecarts[cart_idx].slots[i];
                let original = slot.as_mut().unwrap();
                original.count -= 1;
                if original.count == 0 {
                    *slot = None;
                }
                self.block_ticks.notify(w, p);
                return true;
            }
        }
        false
    }
    pub(super) fn tick_hoppers(&mut self, w: &World) {
        let mut loaded: Vec<_> = w
            .chunks
            .iter()
            .filter(|(_, c)| !c.hoppers.is_empty())
            .collect();
        loaded.sort_unstable_by_key(|(pos, _)| **pos);
        for (&(x, z), chunk) in loaded {
            for &[lx, y, lz] in &chunk.hoppers {
                let p = IVec3::new(x * 16 + lx, y, z * 16 + lz);
                let state = w.get(p);
                let face = (state & 7) as usize;
                let c = self.live_container(w, p);
                c.transfer_cooldown = c.transfer_cooldown.saturating_sub(1).max(0);
                if c.transfer_cooldown != 0 || state & 8 != 0 || face >= SIDES.len() {
                    continue;
                }
                let nonempty = c.slots.iter().any(Option::is_some);
                let target_pos = p + SIDES[face];
                let pushed = nonempty
                    && match container_parts(w, target_pos) {
                        Some(parts) => self.hopper_transfer(w, &[p], &parts, face ^ 1),
                        None => container_cart_at(&self.minecarts, target_pos)
                            .is_some_and(|idx| self.hopper_push_minecart(w, p, idx)),
                    };
                let has_space = self
                    .live_container(w, p)
                    .slots
                    .iter()
                    .any(|s| s.as_ref().is_none_or(|s| s.count < stack_limit(s.id)));
                let above_pos = p + IVec3::Y;
                let pulled = has_space
                    && match container_parts(w, above_pos) {
                        Some(parts) => self.hopper_transfer(w, &parts, &[p], 1),
                        None => match container_cart_at(&self.minecarts, above_pos) {
                            Some(idx) => self.hopper_pull_minecart(w, p, idx),
                            None => self.hopper_pickup(w, p),
                        },
                    };
                if pushed || pulled {
                    self.live_container(w, p).transfer_cooldown = 8;
                }
            }
        }
        self.drops.retain(|d| !d.dead);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{gameplay::Outcome, player::Player, vanilla::ray_trace, world::Chunk};
    use std::sync::Arc;

    fn world() -> World {
        World {
            chunks: [((0, 0), Arc::new(Chunk::empty()))].into(),
            spawn: Default::default(),
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        }
    }
    fn trace() -> (Vec<[i32; 7]>, Vec<[i32; 4]>, Vec<[i32; 4]>) {
        serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-doors-hopper.json"
        ))
        .unwrap()
    }
    #[test]
    fn original_door_items_place_both_halves_from_the_held_stack() {
        let (cases, _, _) = trace();
        assert_eq!(cases.len(), 280);
        let p = IVec3::new(8, 2, 8);
        for [id, yaw, pattern, accepted, count, lower, upper] in cases {
            let mut w = world();
            let mut sim = Simulation::default();
            let mut player = Player::new(p.as_vec3());
            player.set_rotation_degrees(yaw as f32, 0.);
            let item = Stack::new(id as u16, 3);
            sim.inventory.slots[0] = Some(item.clone());
            let mut clicked = p - IVec3::Y;
            let mut face = 1;
            w.set(clicked, 16);
            match pattern {
                1 => {
                    w.set(p - IVec3::X, 16);
                }
                2 => {
                    w.set(p + IVec3::X + IVec3::Y, 16);
                }
                3 => {
                    w.set(
                        p + IVec3::X,
                        crate::vanilla::hand_placement::door_block(id as u16).unwrap() << 4,
                    );
                }
                4 => {
                    w.set(p + IVec3::Y, 16);
                }
                5 => face = 2,
                6 => {
                    w.set(p, 31 << 4 | 1);
                    clicked = p;
                }
                7 => {
                    w.set(clicked, 0);
                }
                _ => (),
            }
            let hit = ray_trace::Hit {
                kind: ray_trace::Kind::Block,
                position: clicked,
                face,
                point: clicked.as_dvec3() + DVec3::splat(0.5),
            };
            let mut out = Outcome::default();
            sim.place_held_block(&mut w, &player, &item, hit, &mut out);
            assert_eq!(
                [
                    i32::from(!out.changed.is_empty()),
                    i32::from(sim.inventory.held().unwrap().count),
                    i32::from(w.get(p)),
                    i32::from(w.get(p + IVec3::Y))
                ],
                [accepted, count, lower, upper],
                "{id}/{yaw}/{pattern}"
            );
            assert!(out.pitched_sounds.is_empty() && out.world_sounds.is_empty());
        }
    }
    #[test]
    fn original_hopper_moves_one_item_per_eight_ticks_pauses_under_power_and_resumes_after_save() {
        let mut w = world();
        let mut sim = Simulation::default();
        let p = IVec3::new(8, 2, 8);
        let top = p + IVec3::Y;
        let bottom = p - IVec3::Y;
        w.set(top, 54 << 4 | 2);
        w.set(bottom, 54 << 4 | 2);
        w.set(p, 154 << 4);
        sim.live_container(&w, top).slots[0] = Some(Stack::new(264, 7));
        let mut player = Player::new(w.spawn);
        let power = p + IVec3::X;
        let (_, rows, _) = trace();
        assert_eq!(rows.len(), 76);
        for (tick, expected) in rows.into_iter().enumerate() {
            if tick == 14 || tick == 32 {
                w.set(power, if tick == 14 { 152 << 4 } else { 0 });
                sim.update_circuits(&mut w, &mut player, vec![power], &mut Outcome::default());
                assert_eq!(w.get(p) & 8 != 0, tick == 14);
            }
            if tick == 41 {
                sim = serde_json::from_slice(&serde_json::to_vec(&sim).unwrap()).unwrap();
            }
            sim.tick_hoppers(&w);
            let count = |q| {
                sim.container_view(&w, q)
                    .unwrap()
                    .slots
                    .iter()
                    .flatten()
                    .map(|s| i32::from(s.count))
                    .sum()
            };
            assert_eq!(
                [
                    count(top),
                    count(p),
                    count(bottom),
                    sim.containers[&container_key(p)].transfer_cooldown
                ],
                expected,
                "tick {tick}"
            );
        }
    }
    #[test]
    fn original_door_powered_placement_manual_use_power_edges_and_support_loss() {
        let (_, _, doors) = trace();
        let mut w = world();
        let mut sim = Simulation::default();
        let p = IVec3::new(8, 2, 8);
        let power = p + IVec3::Y + IVec3::X;
        let mut player = Player::new(p.as_vec3());
        player.set_rotation_degrees(0.,0.);
        for [id,step,lower,upper] in doors {
            let mut out = Outcome::default();
            match step {
                0 => {
                    w.set(p - IVec3::Y, 16);
                    w.set(power, 152 << 4);
                    let item = Stack::new(id as u16, 1);
                    sim.inventory.slots[0] = Some(item.clone());
                    sim.place_held_block(&mut w, &player, &item,
                        ray_trace::Hit { kind: ray_trace::Kind::Block, position:p - IVec3::Y, face:1, point:p.as_dvec3() }, &mut out);
                    sim.update_circuits(&mut w,&mut player,out.changed.clone(),&mut out);
                }
                1 | 4 => assert!(sim.activate_circuit(&mut w,p + IVec3::Y,&mut out)),
                2 | 3 => {
                    if step == 3 { w.set(power,0); }
                    sim.update_circuits(&mut w,&mut player,vec![power],&mut out);
                }
                5 => {
                    w.set(p - IVec3::Y,0);
                    sim.update_circuits(&mut w,&mut player,vec![p - IVec3::Y],&mut out);
                    assert_eq!(sim.drops.pop().unwrap().stack.id,id as u16);
                    assert!(sim.drops.is_empty());
                }
                _ => unreachable!(),
            }
            assert_eq!([i32::from(w.get(p)),i32::from(w.get(p+IVec3::Y))], [lower,upper], "door {id}, step {step}");
        }
    }
    #[test]
    fn hopper_collects_only_drops_in_the_block_above_then_feeds_the_furnace_from_its_side() {
        let mut w = world();
        let mut sim = Simulation::default();
        let p = IVec3::new(6, 2, 6);
        w.set(p, 154 << 4 | 5);
        let furnace = p + IVec3::X;
        w.set(furnace, 61 << 4 | 2);
        let inside = p.as_dvec3() + DVec3::new(0.5, 1.01, 0.5);
        sim.spawn_item_entity(Stack::new(4, 3), inside + DVec3::Z * 2., DVec3::ZERO, 40);
        sim.spawn_item_entity(Stack::new(263, 23), inside, DVec3::ZERO, 40);
        sim.tick_hoppers(&w);
        assert_eq!(sim.drops.len(), 1);
        let h = sim.container_view(&w, p).unwrap();
        assert_eq!(
            (h.special, h.slots.len(), h.slots[0].as_ref().unwrap().count),
            (Special::Hopper, 5, 23)
        );
        sim.live_container(&w, p).slots.swap(0, 1);
        sim.live_container(&w, p).slots[0] = Some(Stack::new(4, 1)); // not a fuel
        for _ in 0..8 {
            sim.tick_hoppers(&w);
        }
        let f = sim.live_container(&w, furnace);
        assert!(f.slots[0].is_none() && f.slots[2].is_none());
        assert_eq!(f.slots[1], Some(Stack::new(263, 1)));
        assert_eq!(sim.live_container(&w, p).slots[0], Some(Stack::new(4, 1)));
    }
    #[test]
    fn hopper_pulls_from_chest_minecart_above_and_pushes_into_chest_minecart_at_facing() {
        let mut w = world();
        let mut sim = Simulation::default();
        let p = IVec3::new(6, 4, 6);
        w.set(p, 154 << 4);
        w.set(p + IVec3::Y, 66 << 4);
        w.set(p - IVec3::Y, 66 << 4);
        let player = Player::new(w.spawn);
        let mut out = Outcome::default();
        assert!(sim.try_place_minecart_on_rail(&mut w, &player, p + IVec3::Y, 342, &mut out));
        assert!(sim.try_place_minecart_on_rail(&mut w, &player, p - IVec3::Y, 342, &mut out));
        sim.minecarts[0].slots[0] = Some(Stack::new(264, 3));
        sim.tick_hoppers(&w);
        assert_eq!(sim.minecarts[0].slots[0], Some(Stack::new(264, 2)));
        assert_eq!(sim.live_container(&w, p).slots[0], Some(Stack::new(264, 1)));
        for _ in 0..8 {
            sim.tick_hoppers(&w);
        }
        assert_eq!(sim.minecarts[0].slots[0], Some(Stack::new(264, 1)));
        assert_eq!(sim.minecarts[1].slots[0], Some(Stack::new(264, 1)));
    }
}
