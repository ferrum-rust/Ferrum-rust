//! Original furnace, dispenser/dropper and chest onBlockAdded orientation.
use super::material;
use glam::IVec3;
pub trait FacingWorld {
    fn get(&self, p: IVec3) -> u16;
    fn set(&mut self, p: IVec3, state: u16, flags: u8);
}
fn full(state: u16) -> bool {
    material::full_block(state)
}
pub(crate) fn chest_facing(p: IVec3, state: u16, get: impl Fn(IVec3) -> u16) -> u16 {
    let id = state >> 4;
    let n = get(p - IVec3::Z);
    let s = get(p + IVec3::Z);
    let w = get(p - IVec3::X);
    let e = get(p + IVec3::X);
    let mut facing = state & 15;
    if n >> 4 == id || s >> 4 == id {
        let adjacent = if n >> 4 == id {
            p - IVec3::Z
        } else {
            p + IVec3::Z
        };
        let aw = get(adjacent - IVec3::X);
        let ae = get(adjacent + IVec3::X);
        facing = if (if n >> 4 == id { n } else { s }) & 15 == 4 {
            4
        } else {
            5
        };
        if (full(w) || full(aw)) && !full(e) && !full(ae) {
            facing = 5;
        }
        if (full(e) || full(ae)) && !full(w) && !full(aw) {
            facing = 4;
        }
    } else if w >> 4 == id || e >> 4 == id {
        let adjacent = if w >> 4 == id {
            p - IVec3::X
        } else {
            p + IVec3::X
        };
        let an = get(adjacent - IVec3::Z);
        let ass = get(adjacent + IVec3::Z);
        facing = if (if w >> 4 == id { w } else { e }) & 15 == 2 {
            2
        } else {
            3
        };
        if (full(n) || full(an)) && !full(s) && !full(ass) {
            facing = 3;
        }
        if (full(s) || full(ass)) && !full(n) && !full(an) {
            facing = 2;
        }
    }
    facing
}
fn chest(world: &mut impl FacingWorld, p: IVec3, state: u16) {
    let facing = chest_facing(p, state, |q| world.get(q));
    world.set(p, (state & !15) | facing, 3);
}
pub fn added(world: &mut impl FacingWorld, p: IVec3, state: u16) -> bool {
    let id = state >> 4;
    match id {
        61 | 62 => {
            let n = full(world.get(p - IVec3::Z));
            let s = full(world.get(p + IVec3::Z));
            let w = full(world.get(p - IVec3::X));
            let e = full(world.get(p + IVec3::X));
            let f = state & 15;
            let f = match f {
                2 if n && !s => 3,
                3 if s && !n => 2,
                4 if w && !e => 5,
                5 if e && !w => 4,
                _ => f,
            };
            world.set(p, id << 4 | f, 2);
        }
        23 | 158 => {
            let n = full(world.get(p - IVec3::Z));
            let s = full(world.get(p + IVec3::Z));
            let mut f = state & 7;
            if f == 2 && n && !s {
                f = 3;
            } else if f == 3 && s && !n {
                f = 2;
            } else {
                let w = full(world.get(p - IVec3::X));
                let e = full(world.get(p + IVec3::X));
                if f == 4 && w && !e {
                    f = 5;
                } else if f == 5 && e && !w {
                    f = 4;
                }
            }
            world.set(p, id << 4 | f, 2);
        }
        54 | 146 => {
            chest(world, p, state);
            for d in [IVec3::NEG_Z, IVec3::X, IVec3::Z, IVec3::NEG_X] {
                let neighbor = world.get(p + d);
                if neighbor >> 4 == id {
                    chest(world, p + d, neighbor);
                }
            }
        }
        _ => return false,
    }
    true
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::{cell::RefCell, collections::BTreeMap};
    struct Test {
        blocks: BTreeMap<[i32; 3], u16>,
        reads: RefCell<Vec<[i32; 3]>>,
        writes: Vec<[i32; 5]>,
    }
    impl FacingWorld for Test {
        fn get(&self, p: IVec3) -> u16 {
            self.reads.borrow_mut().push(p.to_array());
            *self.blocks.get(&p.to_array()).unwrap_or(&0)
        }
        fn set(&mut self, p: IVec3, s: u16, f: u8) {
            self.blocks.insert(p.to_array(), s);
            self.writes.push([p.x, p.y, p.z, s as i32, f as i32]);
        }
    }
    #[test]
    fn original_container_added_reads_and_writes_match() {
        let data: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/vanilla189-facing.json"))
                .unwrap();
        for row in data["cases"].as_array().unwrap() {
            let state = row["state"].as_u64().unwrap() as u16;
            let mut w = Test {
                blocks: BTreeMap::new(),
                reads: RefCell::default(),
                writes: Vec::new(),
            };
            for b in row["blocks"].as_array().unwrap() {
                w.blocks.insert(
                    [
                        b[0].as_i64().unwrap() as i32,
                        b[1].as_i64().unwrap() as i32,
                        b[2].as_i64().unwrap() as i32,
                    ],
                    b[3].as_u64().unwrap() as u16,
                );
            }
            assert!(added(&mut w, IVec3::new(0, 64, 0), state));
            assert_eq!(json!(*w.reads.borrow()), row["reads"], "reads state{state}");
            assert_eq!(json!(w.writes), row["writes"], "writes state{state}");
        }
    }
}
