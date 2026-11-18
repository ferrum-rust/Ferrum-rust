use super::*;
use crate::protocol::Writer;
use serde_json::{json, Value};
use std::io::Read;
fn i(v: &Value) -> i32 {
    v.as_i64().unwrap() as i32
}
fn yes(v: &Value) -> bool {
    v.as_bool().unwrap()
}
fn net(id: i32, w: Writer) -> Value {
    let hex: String = w.0.iter().map(|b| format!("{b:02x}")).collect();
    json!(["net", id, hex])
}
#[test]
fn original_click_mouse_and_held_controller_c07_state_swing_sounds_and_cracks_match() {
    let mut raw = vec![];
    flate2::read::GzDecoder::new(
        &include_bytes!("../../tests/fixtures/vanilla189-click-controller.json.gz")[..],
    )
    .read_to_end(&mut raw)
    .unwrap();
    let suites: Vec<(usize, bool, Vec<Vec<Value>>)> = serde_json::from_slice(&raw).unwrap();
    assert_eq!(suites.len(), 32);
    for (test, creative, rows) in suites {
        let mut controller = Controller::default();
        let a = IVec3::new(8, 2, 10);
        let b = IVec3::new(9, 2, 10);
        let initial = i(&rows[0][6]) as u16;
        let mut blocks = [initial; 2];
        for r in rows {
            let tick = i(&r[0]);
            let aim = i(&r[1]);
            let pos = if aim == 1 { b } else { a };
            if tick == 29 || tick == 53 {
                blocks[0] = initial;
            }
            let state = i(&r[6]) as u16;
            let block = Block {
                pos,
                face: i(&r[2]) as u8,
                state,
                increment: f32::from_bits(i(&r[7]) as u32),
                in_border: yes(&r[13]),
                can_edit: true,
                adjacent_fire: yes(&r[12]),
            };
            let mut target = match aim {
                0 | 1 => Target::Block(block),
                3 => Target::Entity,
                _ => Target::Miss,
            };
            let item = (i(&r[8]) != 0).then(|| {
                let mut s = Stack::new(i(&r[8]) as u16, 1);
                s.damage = i(&r[9]) as u16;
                s.identity = i(&r[11]) as u64 + 1;
                if yes(&r[10]) {
                    use crate::vanilla::drops::DropNbt;
                    s = s.with_nbt(DropNbt::Compound(std::collections::BTreeMap::from([(
                        "custom".into(),
                        DropNbt::Compound(Default::default()),
                    )])));
                }
                s
            });
            let mut actions = controller.press_phase(
                &mut target,
                i(&r[3]) as u32,
                creative,
                item.as_ref(),
                yes(&r[5]),
            );
            actions.extend(controller.held_phase(
                &mut target,
                yes(&r[4]),
                creative,
                item.as_ref(),
                yes(&r[5]),
            ));
            let mut events = Vec::new();
            for action in actions {
                match action {
                    Action::Swing => events.push(net(10, Writer::new())),
                    Action::Attack => {
                        let mut w = Writer::new();
                        w.varint(88).varint(1);
                        events.push(net(2, w));
                    }
                    Action::Dig(action, p, f) => {
                        let mut w = Writer::new();
                        w.varint(match action {
                            Dig::Start => 0,
                            Dig::Abort => 1,
                            Dig::Stop => 2,
                        })
                        .position(p.to_array())
                        .u8(f);
                        events.push(net(7, w));
                    }
                    Action::Clicked(_) => {}
                    Action::Destroy(p) => {
                        let ix = usize::from(p == b);
                        let raw = blocks[ix];
                        let id = (raw >> 4) as i32 | (i32::from(raw & 15) << 12);
                        events.push(json!(["aux", 2001, p.to_array(), id]));
                        events.push(json!(["set", p.to_array(), 0]));
                        blocks[ix] = 0;
                    }
                    Action::Extinguish(p) => events.push(json!(["set", p.to_array(), 0])),
                    Action::Stage(p, n) => events.push(json!(["stage", p.to_array(), n])),
                    Action::StepSound(_, raw) => {
                        let (k, v, p) = SOUNDS[(raw >> 4) as usize];
                        events.push(json!(["sound", CLIPS[k].0, v, p]));
                    }
                    Action::HitEffect(p, f) => events.push(json!(["particle", p.to_array(), f])),
                }
            }
            let label = format!("original scene {test} creative {creative} tick {tick}");
            assert_eq!(Value::Array(events), r[14], "{label} event order");
            assert_eq!(controller.click_counter, i(&r[15]), "{label} click counter");
            assert_eq!(controller.hitting, yes(&r[16]), "{label} hitting");
            assert_eq!(
                json!(controller.current.to_array()),
                r[17],
                "{label} current"
            );
            assert_eq!(
                controller.damage.to_bits(),
                i(&r[18]) as u32,
                "{label} damage"
            );
            assert_eq!(
                controller.sound_ticks.to_bits(),
                i(&r[19]) as u32,
                "{label} sound"
            );
            assert_eq!(controller.hit_delay, i(&r[20]), "{label} delay");
            assert_eq!(
                blocks,
                [i(&r[21]) as u16, i(&r[22]) as u16],
                "{label} world"
            );
        }
    }
}
