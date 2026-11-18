use super::{material, ray_trace::Access};
use glam::IVec3;
include!("note_power_data.rs");
pub const SIDES: [IVec3; 6] = [
    IVec3::NEG_Y,
    IVec3::Y,
    IVec3::NEG_Z,
    IVec3::Z,
    IVec3::NEG_X,
    IVec3::X,
];

pub fn tune(pitch: u8) -> u8 {
    (pitch + 1) % 25
}
pub fn trigger(world: &impl Access, p: IVec3, pitch: u8) -> Option<(u8, u8)> {
    (world.get(p + IVec3::Y) >> 4 == 0).then(|| {
        (
            super::special_click::instrument(world.get(p - IVec3::Y)),
            pitch,
        )
    })
}
pub fn rising_edge(previous: &mut bool, powered: bool) -> bool {
    let plays = powered && !*previous;
    *previous = powered;
    plays
}
pub fn wire_attachment(world: &impl Access, p: IVec3, face: usize) -> &'static str {
    let next = p + SIDES[face];
    let state = world.get(next);
    let normal = |s| material::blocks_movement(s) && material::full_cube(s);
    let connects = match state >> 4 {
        55 => true,
        93 | 94 => {
            let direction = [3, 4, 2, 5][usize::from(state & 3)];
            direction == face || direction == (face ^ 1)
        }
        _ => material::can_provide_power(state),
    };
    if connects || !normal(state) && world.get(next - IVec3::Y) >> 4 == 55 {
        "side"
    } else if !normal(world.get(p + IVec3::Y))
        && normal(state)
        && world.get(next + IVec3::Y) >> 4 == 55
    {
        "up"
    } else {
        "none"
    }
}
fn output(
    world: &impl Access,
    p: IVec3,
    face: usize,
    strong: bool,
    comparator: &impl Fn(IVec3) -> i32,
    allow_wire: bool,
) -> u8 {
    let state = world.get(p);
    if state >> 4 == 55 {
        if !allow_wire || state & 15 == 0 || face == 0 {
            return 0;
        }
        if face == 1 {
            return (state & 15) as u8;
        }
        let mut wires = 0u8;
        for side in 2..6 {
            if electrically_connected(world, p, side) {
                wires |= 1 << side;
            }
        }
        let perpendicular = if face < 4 { 0b110000 } else { 0b001100 };
        return if wires == 0 || wires & (1 << face) != 0 && wires & perpendicular == 0 {
            (state & 15) as u8
        } else {
            0
        };
    }
    let Ok(i) = SOURCES.binary_search_by_key(&state, |row| row.0) else {
        return 0;
    };
    let (_, weak, hard) = SOURCES[i];
    if (if strong { hard } else { weak }) & (1 << face) == 0 {
        return 0;
    }
    match state >> 4 {
        147 | 148 | 151 | 178 => (state & 15) as u8,
        149 | 150 => comparator(p).clamp(0, 15) as u8,
        _ => 15,
    }
}
fn electrically_connected(world: &impl Access, p: IVec3, face: usize) -> bool {
    let q = p + SIDES[face];
    let state = world.get(q);
    let normal = material::normal_cube(state);
    (!material::normal_cube(world.get(p + IVec3::Y))
        && normal
        && world.get(q + IVec3::Y) >> 4 == 55)
        || !normal && world.get(q - IVec3::Y) >> 4 == 55
        || match state >> 4 {
            55 => true,
            93 | 94 => [3, 4, 2, 5][(state & 3) as usize] / 2 == face / 2,
            _ => material::can_provide_power(state),
        }
}
pub fn side_power(
    world: &impl Access,
    near: IVec3,
    face: usize,
    allow_wire: bool,
    comparator: &impl Fn(IVec3) -> i32,
) -> u8 {
    if material::normal_cube(world.get(near)) {
        SIDES
            .iter()
            .enumerate()
            .map(|(side, &axis)| output(world, near + axis, side, true, comparator, allow_wire))
            .max()
            .unwrap_or(0)
    } else {
        output(world, near, face, false, comparator, allow_wire)
    }
}
pub fn powered(world: &impl Access, p: IVec3, comparator: impl Fn(IVec3) -> i32) -> bool {
    SIDES.iter().enumerate().any(|(face, &axis)| {
        let near = p + axis;
        side_power(world, near, face, true, &comparator) > 0
    })
}
pub fn diode_facing(s: u16) -> usize {
    [3, 4, 2, 5][(s & 3) as usize]
}
pub fn comparator_tile(world: &crate::world::World, p: IVec3) -> i32 {
    use super::item_nbt;
    item_nbt::number(
        world
            .tile(p)
            .and_then(item_nbt::compound)
            .and_then(|s| s.get("OutputSignal")),
    )
}
pub fn diode_side_input(world: &crate::world::World, p: IVec3, repeater: bool) -> u8 {
    let face = diode_facing(world.get(p));
    let sides = if face < 4 { 4..6 } else { 2..4 };
    sides
        .map(|i| {
            let q = p + SIDES[i];
            let s = world.get(q);
            if repeater && !matches!(s >> 4, 93 | 94 | 149 | 150) || !material::can_provide_power(s)
            {
                0
            } else if s >> 4 == 55 {
                (s & 15) as u8
            } else {
                output(world, q, i, true, &|p| comparator_tile(world, p), true)
            }
        })
        .max()
        .unwrap_or(0)
}
pub fn repeater_locked(world: &crate::world::World, p: IVec3) -> bool {
    diode_side_input(world, p, true) > 0
}
#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    use serde_json::{json, Value};
    use std::collections::HashMap;
    #[derive(Default)]
    struct Grid(HashMap<IVec3, u16>);
    impl Access for Grid {
        fn get(&self, p: IVec3) -> u16 {
            self.0.get(&p).copied().unwrap_or(0)
        }
    }
    #[test]
    fn original_tuning_block_events_and_power_edges_match() {
        let data: Value = serde_json::from_reader(GzDecoder::new(
            &include_bytes!("../../tests/fixtures/vanilla189-note-blocks.json.gz")[..],
        ))
        .unwrap();
        let p = IVec3::new(3, 67, -12);
        let turns = data["turns"].as_array().unwrap();
        assert_eq!(turns.len(), 500);
        for row in turns {
            let remote = row[0].as_bool().unwrap();
            let mut pitch = row[1].as_u64().unwrap() as u8;
            let mut w = Grid::default();
            w.0.insert(p, 25 << 4);
            w.0.insert(p - IVec3::Y, (row[2].as_u64().unwrap() as u16) << 4);
            if row[3] == true {
                w.0.insert(p + IVec3::Y, 20 << 4);
            }
            let mut events = vec![];
            let mut particles = vec![];
            if !remote {
                pitch = tune(pitch);
                if let Some((inst, note)) = trigger(&w, p, pitch) {
                    let (file, vol, tone) = super::super::special_click::note_sound(inst, note);
                    events.push(json!(["note", inst, note]));
                    events.push(json!([
                        "sound",
                        file.replace('/', "."),
                        vol.to_bits(),
                        tone.to_bits()
                    ]));
                    if let super::super::special_click::Effect::Particle(id, pos, v) =
                        super::super::special_click::note_particle(p, note)
                    {
                        let bits = [pos.x, pos.y, pos.z, v.x, v.y, v.z]
                            .map(|n| format!("{:x}", n.to_bits()));
                        particles.push(json!([id, bits]));
                    }
                }
            }
            assert_eq!(
                json!([true, pitch, !remote, events, particles]),
                json!([row[4], row[5], row[6], row[7], row[8]])
            );
        }
        let rows = data["power"].as_array().unwrap();
        assert_eq!(rows.len(), 8478);
        for row in rows {
            let (raw, face, scenario) = (
                row[0].as_u64().unwrap() as u16,
                row[1].as_u64().unwrap() as usize,
                row[2].as_u64().unwrap() as usize,
            );
            let mut w = Grid::default();
            let near = p + SIDES[face];
            if scenario == 0 {
                w.0.insert(near, raw);
            } else if scenario <= 6 {
                w.0.insert(near, 16);
                w.0.insert(near + SIDES[scenario - 1], raw);
            } else {
                w.0.insert(near, raw);
                for (side, &dir) in SIDES.iter().enumerate().skip(2) {
                    if (side + face) % 2 == scenario - 7 && near + dir != p {
                        w.0.insert(near + dir, 55 << 4 | 9);
                    }
                }
            }
            w.0.insert(p, 25 << 4);
            let power = powered(&w, p, |_| 13);
            let mut latch = false;
            let count = usize::from(rising_edge(&mut latch, power) && trigger(&w, p, 7).is_some())
                + usize::from(rising_edge(&mut latch, power));
            let before = latch;
            rising_edge(&mut latch, false);
            assert_eq!(
                json!([power, count, before, latch]),
                json!([row[3], row[4], row[5], row[6]]),
                "state {raw}, face {face}, scenario {scenario}"
            );
        }
    }
}
