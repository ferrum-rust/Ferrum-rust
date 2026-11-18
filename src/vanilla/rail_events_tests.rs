use super::*;
use crate::vanilla::spawning;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
};
fn hash(mut h: u64, n: i32) -> u64 {
    for b in n.to_le_bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    h
}
struct World {
    blocks: BTreeMap<[i32; 3], u16>,
    power: BTreeSet<[i32; 3]>,
    reads: Cell<u64>,
    read_hash: Cell<u64>,
    writes: Vec<[i32; 5]>,
    notifications: Vec<[i32; 4]>,
    power_queries: RefCell<Vec<[i32; 3]>>,
    drops: u64,
}
impl RailWorld for World {
    fn get(&self, p: IVec3) -> u16 {
        self.reads.set(self.reads.get() + 1);
        let mut h = self.read_hash.get();
        for n in p.to_array() {
            h = hash(h, n);
        }
        self.read_hash.set(h);
        self.blocks
            .get(&p.to_array())
            .copied()
            .unwrap_or(if p.y == 10 { 16 } else { 0 })
    }
    fn set(&mut self, p: IVec3, state: u16, flags: u8) {
        self.writes
            .push([p.x, p.y, p.z, state as i32, flags as i32]);
        self.blocks.insert(p.to_array(), state);
    }
    fn is_powered(&self, p: IVec3) -> bool {
        self.power_queries.borrow_mut().push(p.to_array());
        self.power.contains(&p.to_array())
    }
    fn solid_top(&self, p: IVec3) -> bool {
        spawning::solid_top(self.get(p))
    }
    fn drop_block(&mut self, _p: IVec3, _state: u16) {
        self.drops += 1;
    }
    fn notify_neighbors(&mut self, p: IVec3, source: u16) {
        self.notifications.push([p.x, p.y, p.z, source as i32]);
    }
}
#[test]
fn original_rail_graph_and_callbacks_match_read_write_and_notification_order() {
    let rows: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-rail-events.json"
    ))
    .unwrap();
    for (case, row) in rows.as_array().unwrap().iter().enumerate() {
        let initial: Vec<[i32; 4]> = serde_json::from_value(row["initial"].clone()).unwrap();
        let power: Vec<[i32; 3]> = serde_json::from_value(row["power"].clone()).unwrap();
        let mut world = World {
            blocks: initial
                .into_iter()
                .map(|p| ([p[0], p[1], p[2]], p[3] as u16))
                .collect(),
            power: power.into_iter().collect(),
            reads: Cell::new(0),
            read_hash: Cell::new(0xcbf29ce484222325),
            writes: Vec::new(),
            notifications: Vec::new(),
            power_queries: RefCell::new(Vec::new()),
            drops: 0,
        };
        let center = row["center"].as_u64().unwrap() as u16;
        let p = IVec3::new(0, 11, 0);
        let mut result = center;
        match row["kind"].as_str().unwrap() {
            "shape" => {
                result = update_shape(&mut world, p, center, row["force"].as_bool().unwrap());
            }
            "added" => {
                assert!(on_added(&mut world, p, center));
            }
            "neighbor" => {
                assert!(neighbor_changed(
                    &mut world,
                    p,
                    center,
                    row["source_power"].as_bool().unwrap()
                ));
            }
            "removed" => {
                assert!(on_removed(&mut world, p, center));
            }
            _ => unreachable!(),
        }
        assert_eq!(
            result as u64,
            row["result"].as_u64().unwrap(),
            "result case {case}"
        );
        assert_eq!(
            world.reads.get(),
            row["reads"].as_u64().unwrap(),
            "read count case {case}"
        );
        assert_eq!(
            world.read_hash.get().to_string(),
            row["read_hash"].as_str().unwrap(),
            "read sequence case {case}"
        );
        assert_eq!(
            serde_json::to_value(world.writes).unwrap(),
            row["writes"],
            "writes case {case}"
        );
        assert_eq!(
            serde_json::to_value(world.notifications).unwrap(),
            row["notifications"],
            "notifications case {case}"
        );
        assert_eq!(
            serde_json::to_value(world.power_queries.into_inner()).unwrap(),
            row["power_queries"],
            "power queries case {case}"
        );
        assert_eq!(
            world.drops,
            row["drops"].as_u64().unwrap(),
            "drop callback count case {case}"
        );
    }
    assert_eq!(rows.as_array().unwrap().len(), 8312);
}

fn blank_world() -> World {
    World {
        blocks: BTreeMap::new(),
        power: BTreeSet::new(),
        reads: Cell::new(0),
        read_hash: Cell::new(0),
        writes: Vec::new(),
        notifications: Vec::new(),
        power_queries: RefCell::new(Vec::new()),
        drops: 0,
    }
}

fn cascade_power(world: &mut World, seed: IVec3) {
    let mut queue = std::collections::VecDeque::from([seed]);
    let mut steps = 0usize;
    while let Some(center) = queue.pop_front() {
        if steps >= 256 {
            break;
        }
        let writes_before = world.writes.len();
        let state = world.get(center);
        if matches!(state >> 4, 27 | 157) {
            steps += 1;
            neighbor_changed(world, center, state, true);
        }
        let new_writes: Vec<IVec3> = world.writes[writes_before..]
            .iter()
            .map(|w| IVec3::new(w[0], w[1], w[2]))
            .collect();
        for changed in new_writes {
            for d in [
                IVec3::X,
                IVec3::NEG_X,
                IVec3::Z,
                IVec3::NEG_Z,
                IVec3::Y,
                IVec3::NEG_Y,
            ] {
                queue.push_back(changed + d);
            }
        }
    }
}

#[test]
fn detector_rail_orients_east_west_and_ascending_slopes_on_added() {
    // East-West auto-orientation (meta = 1) when placed next to West/East rails.
    let mut world = blank_world();
    let p = IVec3::new(0, 11, 0);
    world.blocks.insert([-1, 11, 0], 66 << 4 | 1);
    world.blocks.insert([1, 11, 0], 66 << 4 | 1);
    world.blocks.insert(p.to_array(), 28 << 4);
    assert!(on_added(&mut world, p, 28 << 4));
    assert_eq!(world.get(p), (28 << 4) | 1);

    // All 4 ascending slopes (2 = East, 3 = West, 4 = North, 5 = South) on placement.
    for (upper_rail, support_wall, expected_meta) in [
        ([1, 12, 0], [1, 11, 0], 2u16),
        ([-1, 12, 0], [-1, 11, 0], 3u16),
        ([0, 12, -1], [0, 11, -1], 4u16),
        ([0, 12, 1], [0, 11, 1], 5u16),
    ] {
        let mut world = blank_world();
        world.blocks.insert(support_wall, 1 << 4);
        world.blocks.insert(upper_rail, 66 << 4);
        world.blocks.insert(p.to_array(), 28 << 4);
        assert!(on_added(&mut world, p, 28 << 4));
        assert_eq!(world.get(p), (28 << 4) | expected_meta);
    }
}

#[test]
fn normal_rail_forms_curves_and_flips_on_t_junction_redstone_power() {
    let p = IVec3::new(0, 11, 0);
    // 4 curve shapes: 6 (SE), 7 (SW), 8 (NW), 9 (NE)
    for (a, b, expected_shape) in [
        ([0, 11, 1], [1, 11, 0], 6u16),
        ([0, 11, 1], [-1, 11, 0], 7u16),
        ([0, 11, -1], [-1, 11, 0], 8u16),
        ([0, 11, -1], [1, 11, 0], 9u16),
    ] {
        let mut world = blank_world();
        world.blocks.insert(a, 66 << 4);
        world.blocks.insert(b, 66 << 4);
        world.blocks.insert(p.to_array(), 66 << 4);
        assert!(on_added(&mut world, p, 66 << 4));
        assert_eq!(world.get(p), (66 << 4) | expected_shape);
    }

    // T-junction (South + West + East): unpowered -> shape 6 (SE, South-East rule), powered -> shape 7 (SW).
    let mut world = blank_world();
    world.blocks.insert([0, 11, 1], 66 << 4);
    world.blocks.insert([-1, 11, 0], 66 << 4 | 1);
    world.blocks.insert([1, 11, 0], 66 << 4 | 1);
    world.blocks.insert(p.to_array(), 66 << 4);
    assert!(on_added(&mut world, p, 66 << 4));
    assert_eq!(world.get(p), (66 << 4) | 6);

    // Non-power neighbor changes do NOT flip the T-junction even if power set changes.
    world.power.insert(p.to_array());
    let cur = world.get(p);
    assert!(neighbor_changed(&mut world, p, cur, false));
    assert_eq!(world.get(p), (66 << 4) | 6);

    // Power-capable neighbor change flips T-junction to shape 7 (SW), and unpowering flips back to 6 (SE).
    let cur = world.get(p);
    assert!(neighbor_changed(&mut world, p, cur, true));
    assert_eq!(world.get(p), (66 << 4) | 7);
    world.power.remove(&p.to_array());
    let cur = world.get(p);
    assert!(neighbor_changed(&mut world, p, cur, true));
    assert_eq!(world.get(p), (66 << 4) | 6);
}

#[test]
fn powered_and_activator_rails_propagate_power_bit_up_to_eight_rails() {
    for rail_id in [27u16, 157u16] {
        let mut world = blank_world();
        for x in 0..=10 {
            world.blocks.insert([x, 11, 0], (rail_id << 4) | 1);
        }
        let origin = IVec3::new(0, 11, 0);
        world.power.insert(origin.to_array());
        cascade_power(&mut world, origin);

        // Origin (0) + 8 connected rails (1..=8) receive bit 8; distance 9..=10 remain unpowered.
        for x in 0..=8 {
            assert_eq!(
                world.get(IVec3::new(x, 11, 0)) & 8,
                8,
                "rail_id {rail_id} at x={x} should be powered"
            );
        }
        for x in 9..=10 {
            assert_eq!(
                world.get(IVec3::new(x, 11, 0)) & 8,
                0,
                "rail_id {rail_id} at x={x} beyond 8-rail limit must stay unpowered"
            );
        }

        // Removing power clears bit 8 across all 0..=8 rails.
        world.power.remove(&origin.to_array());
        cascade_power(&mut world, origin);
        for x in 0..=10 {
            assert_eq!(world.get(IVec3::new(x, 11, 0)) & 8, 0);
        }
    }
}
