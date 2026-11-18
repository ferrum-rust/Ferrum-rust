use super::*;
use crate::vanilla::{armor, item_nbt};
use serde_json::{json, Value};

fn stack(v: &Value, tags: &[Arc<DropNbt>]) -> Option<Stack> {
    if v.is_null() {
        return None;
    }
    let id = v[0].as_u64().unwrap() as u16;
    let mut stack = Stack::new(id, v[2].as_u64().unwrap() as u8);
    let damage = v[1].as_u64().unwrap() as u16;
    if armor::maximum_damage(id) > 0 {
        stack.damage = damage;
    } else {
        stack.meta = damage;
    }
    stack.nbt = v[3].as_u64().map(|i| tags[i as usize].clone());
    Some(stack)
}
fn value(stack: Option<&Stack>) -> Value {
    stack.map_or(Value::Null, |s| {
        json!([
            s.id,
            s.meta,
            s.damage,
            s.count,
            s.nbt.as_deref().map(|tag| item_nbt::encode(tag).unwrap())
        ])
    })
}
fn offers(v: &Value, tags: &[Arc<DropNbt>]) -> Vec<Offer> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|row| Offer {
            cost: stack(&row[0], tags).unwrap(),
            second: stack(&row[1], tags),
            output: stack(&row[2], tags).unwrap(),
            uses: row[3].as_i64().unwrap() as i32,
            max_uses: row[4].as_i64().unwrap() as i32,
            reward_exp: true,
            supported: true,
        })
        .collect()
}
#[test]
fn merchant_tag_matching_selected_recipe_and_output_payment_match_original() {
    let data: Value =
        serde_json::from_str(include_str!("../tests/fixtures/vanilla189-merchant.json")).unwrap();
    let tags = data["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            let hex = v.as_str().unwrap();
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect::<Vec<_>>();
            Arc::new(item_nbt::decode(&bytes).unwrap().unwrap())
        })
        .collect::<Vec<_>>();
    for (case, row) in data["comparisons"].as_array().unwrap().iter().enumerate() {
        let required = stack(&row[0], &tags).unwrap();
        let given = stack(&row[1], &tags).unwrap();
        assert_eq!(
            matches_cost(&given, &required) && given.count >= required.count,
            row[2].as_bool().unwrap(),
            "cost case {case}"
        );
    }
    let variants = data["offers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|list| offers(list, &tags))
        .collect::<Vec<_>>();
    for (case, row) in data["trades"].as_array().unwrap().iter().enumerate() {
        let mut trader = Trader {
            offers: variants[row[0].as_u64().unwrap() as usize].clone(),
            ..Default::default()
        };
        let mut window = MerchantWindow::default();
        window.select(row[1].as_i64().unwrap() as i32, &mut trader);
        window.set_input(0, stack(&row[2][0], &tags), &mut trader);
        window.set_input(1, stack(&row[2][1], &tags), &mut trader);
        assert_eq!(
            window.current_recipe.map_or(-1, |i| i as i64),
            row[3].as_i64().unwrap(),
            "trade {case} selected"
        );
        let expected = stack(&row[4], &tags);
        assert_eq!(
            value(window.output.as_ref()),
            value(expected.as_ref()),
            "trade {case} preview"
        );
        let copy = window
            .current_recipe
            .zip(window.output.as_ref())
            .is_some_and(|(i, out)| out.identity != trader.offers[i].output.identity);
        assert_eq!(
            copy,
            row[5].as_bool().unwrap(),
            "trade {case} output identity"
        );
        if window.current_recipe.is_some() {
            let taken = window.take_output(&mut trader);
            assert_eq!(
                value(taken.as_ref()),
                value(expected.as_ref()),
                "trade {case} pickup takes entire result"
            );
        }
        for slot in 0..3 {
            let actual = if slot == 2 {
                window.output.as_ref()
            } else {
                window.inputs()[slot].as_ref()
            };
            let expected = stack(&row[6][slot], &tags);
            assert_eq!(
                value(actual),
                value(expected.as_ref()),
                "trade {case} final slot {slot}"
            );
        }
        assert_eq!(
            json!(trader.offers.iter().map(|o| o.uses).collect::<Vec<_>>()),
            row[7],
            "trade {case} uses"
        );
    }
    assert_eq!(data["comparisons"].as_array().unwrap().len(), 576);
    assert_eq!(data["trades"].as_array().unwrap().len(), 3136);
}
#[test]
fn merchant_container_clicks_partial_quickmoves_and_close_match_original() {
    use crate::inventory::{ClickAction, Inventory, SlotRef};
    let data: Value = serde_json::from_str(include_str!(
        "../tests/fixtures/vanilla189-merchant-clicks.json"
    ))
    .unwrap();
    let tags = data["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            let hex = v.as_str().unwrap();
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect::<Vec<_>>();
            Arc::new(item_nbt::decode(&bytes).unwrap().unwrap())
        })
        .collect::<Vec<_>>();
    for case in data["cases"].as_array().unwrap() {
        let mut trader = Trader {
            offers: offers(&case["offers"], &tags),
            ..Default::default()
        };
        let mut window = MerchantWindow::default();
        let mut inventory = Inventory::default();
        let initial = &case["initial"];
        for (i, s) in initial[0].as_array().unwrap().iter().enumerate() {
            inventory.slots[i] = stack(s, &tags);
        }
        inventory.cursor = stack(&initial[2], &tags);
        window.select(case["selected"].as_i64().unwrap_or(0) as i32, &mut trader);
        for i in 0..2 {
            window.set_input(i, stack(&initial[1][i], &tags), &mut trader);
        }
        let mut dropped = Vec::new();
        let mut drag = Vec::new();
        for (i, step) in case["steps"].as_array().unwrap().iter().enumerate() {
            let op = &step[0];
            if op[0] == "close" {
                dropped.extend(inventory.cursor.take());
                dropped.extend(window.close());
            } else {
                let wire = op[1].as_i64().unwrap();
                let button = op[2].as_u64().unwrap() as u8;
                let slot = match wire {
                    0..=2 => SlotRef::Container(wire as usize),
                    3..=29 => SlotRef::Player(wire as usize + 6),
                    30..=38 => SlotRef::Player(wire as usize - 30),
                    _ => SlotRef::Outside,
                };
                let action = match op[3].as_u64().unwrap() {
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
                            drag.push(slot);
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
                    dropped.extend(crate::container::merchant_click(
                        &mut inventory,
                        &mut window,
                        &mut trader,
                        slot,
                        action,
                        case["creative"] == true,
                    ));
                }
            }
            let expected = &step[1];
            for slot in 0..36 {
                assert_eq!(
                    value(inventory.slots[slot].as_ref()),
                    value(stack(&expected[0][slot], &tags).as_ref()),
                    "{} step {i} inventory {slot}",
                    case["name"]
                );
            }
            for slot in 0..3 {
                let actual = if slot == 2 {
                    window.output.as_ref()
                } else {
                    window.inputs()[slot].as_ref()
                };
                assert_eq!(
                    value(actual),
                    value(stack(&expected[1][slot], &tags).as_ref()),
                    "{} step {i} merchant {slot}",
                    case["name"]
                );
            }
            assert_eq!(
                value(inventory.cursor.as_ref()),
                value(stack(&expected[2], &tags).as_ref()),
                "{} step {i} cursor",
                case["name"]
            );
            assert_eq!(
                json!(trader.offers.iter().map(|o| o.uses).collect::<Vec<_>>()),
                expected[3],
                "{} step {i} stock",
                case["name"]
            );
            assert_eq!(
                dropped.iter().map(|s| value(Some(s))).collect::<Vec<_>>(),
                expected[4]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| value(stack(s, &tags).as_ref()))
                    .collect::<Vec<_>>(),
                "{} step {i} drops",
                case["name"]
            );
        }
    }
}
