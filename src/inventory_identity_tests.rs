use super::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
fn stack(v: &Value) -> Option<Stack> {
    if v.is_null() {
        return None;
    }
    let mut s = Stack::new(v[0].as_u64().unwrap() as u16, v[1].as_u64().unwrap() as u8);
    s.meta = v[2].as_u64().unwrap() as u16;
    Some(s)
}
fn snapshot(inv: &Inventory, chest: &Container, ids: &mut BTreeMap<u64, usize>) -> Value {
    let mut item = |s: Option<Stack>| {
        s.map_or(Value::Null, |s| {
            let next = ids.len() + 1;
            let id = *ids.entry(s.identity).or_insert(next);
            json!([s.id, s.count, s.meta, id])
        })
    };
    let p = inv.slots[..36]
        .iter()
        .cloned()
        .map(&mut item)
        .collect::<Vec<_>>();
    let c = chest
        .slots
        .iter()
        .cloned()
        .map(&mut item)
        .collect::<Vec<_>>();
    json!([p, c, item(inv.cursor.clone())])
}
fn slot(wire: i32) -> SlotRef {
    match wire {
        0..=26 => SlotRef::Container(wire as usize),
        27..=53 => SlotRef::Player(wire as usize - 18),
        54..=62 => SlotRef::Player(wire as usize - 54),
        _ => SlotRef::Outside,
    }
}
#[test]
fn stack_identity_through_original_window_operations() {
    let cases: Value = serde_json::from_str(include_str!(
        "../tests/fixtures/vanilla189-inventory-identity.json"
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        let mut inv = Inventory::default();
        let mut chest = Container::new(Kind::Chest);
        for (i, v) in case["initial"][0].as_array().unwrap().iter().enumerate() {
            inv.slots[i] = stack(v);
        }
        for (i, v) in case["initial"][1].as_array().unwrap().iter().enumerate() {
            chest.slots[i] = stack(v);
        }
        inv.cursor = stack(&case["initial"][2]);
        let mut ids = BTreeMap::new();
        let mut drag = Vec::new();
        assert_eq!(snapshot(&inv, &chest, &mut ids), case["initial"]);
        for (i, step) in case["steps"].as_array().unwrap().iter().enumerate() {
            let op = &step[0];
            if op[0] == "add" {
                assert!(inv
                    .add(Stack::new(
                        op[1].as_u64().unwrap() as u16,
                        op[2].as_u64().unwrap() as u8
                    ))
                    .is_none());
            } else {
                let target = slot(op[1].as_i64().unwrap() as i32);
                let button = op[2].as_u64().unwrap() as u8;
                let mode = op[3].as_u64().unwrap();
                let action = match mode {
                    0 => Some(ClickAction::Pickup { right: button == 1 }),
                    1 => Some(ClickAction::QuickMove),
                    2 => Some(ClickAction::Hotbar(button as usize)),
                    3 => Some(ClickAction::Clone),
                    4 => Some(ClickAction::Throw { all: button == 1 }),
                    5 => match button & 3 {
                        0 => {
                            drag.clear();
                            None
                        }
                        1 => {
                            drag.push(target);
                            None
                        }
                        _ => Some(ClickAction::Drag {
                            mode: button >> 2,
                            slots: std::mem::take(&mut drag),
                        }),
                    },
                    6 => Some(ClickAction::Collect {
                        reverse: button == 1,
                    }),
                    _ => unreachable!(),
                };
                if let Some(action) = action {
                    window_click(&mut inv, Some(&mut chest), target, action, true, 2);
                }
            }
            assert_eq!(
                snapshot(&inv, &chest, &mut ids),
                step[1],
                "{} step {i} {op}",
                case["name"]
            );
        }
    }
}
#[test]
fn persistence_preserves_values_and_replaces_object_identities() {
    let original = Stack::new(260, 4);
    let decoded: Stack = serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
    assert_eq!(original, decoded);
    assert_ne!(original.identity, decoded.identity);
    assert_eq!(original, original.copied());
    assert_ne!(original.identity, original.copied().identity);
}
