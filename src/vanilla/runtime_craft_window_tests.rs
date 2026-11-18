use super::*;
use serde_json::{json, Value};
use std::io::Read;
fn stack(v: &Value) -> Stack {
    let id = v[0].as_u64().unwrap() as u16;
    let mut s = crate::vanilla::item_entity::stack_from_metadata(
        id,
        v[1].as_u64().unwrap() as u8,
        v[2].as_u64().unwrap() as u16,
    )
    .unwrap();
    let hex = v[3].as_str().unwrap();
    if !hex.is_empty() {
        let bytes = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect::<Vec<_>>();
        s = s.with_nbt(crate::vanilla::item_nbt::decode(&bytes).unwrap().unwrap());
    }
    s
}
fn slots(v: &Value, n: usize) -> Vec<Option<Stack>> {
    let mut slots = vec![None; n];
    for pair in v.as_array().unwrap() {
        slots[pair[0].as_u64().unwrap() as usize] = Some(stack(&pair[1]));
    }
    slots
}
fn val(s: &Stack) -> Value {
    json!([
        s.id,
        s.count,
        if armor::maximum_damage(s.id) > 0 {
            s.damage
        } else {
            s.meta
        },
        s.nbt.as_deref()
    ])
}
fn sparse(s: &[Option<Stack>]) -> Value {
    Value::Array(
        s.iter()
            .enumerate()
            .filter_map(|(i, s)| Some(json!([i, val(s.as_ref()?)])))
            .collect(),
    )
}
fn want(frame: &Value) -> Value {
    json!([
        sparse(&slots(&frame[0], 36)),
        frame[1].as_array().map(|_| val(&stack(&frame[1]))),
        sparse(&slots(&frame[2], 9)),
        sparse(&slots(&frame[3], 9)),
        frame[4]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| val(&stack(v)))
            .collect::<Vec<_>>()
    ])
}
fn observed(r: &Runtime) -> Value {
    let p = r.backend.player_inventory.as_ref().unwrap();
    let mut drops = Vec::new();
    for key in &r.backend.entities.order {
        if let EntityKey::Item(id) = key {
            drops.push(val(&r.backend.entities.live[id].stack));
        }
    }
    let (table, personal) = if p.workbench.is_some() {
        (
            sparse(&p.inventory.crafting),
            sparse(p.personal_crafting.as_ref().unwrap()),
        )
    } else {
        (json!([]), sparse(&p.inventory.crafting))
    };
    json!([
        sparse(&p.inventory.slots[..36]),
        p.inventory.cursor.as_ref().map(val),
        table,
        personal,
        drops
    ])
}
#[test]
fn original_workbench_slot_click_remainders_and_close_drops_match_after_checkpoint() {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("../../tests/fixtures/vanilla189-crafting-window.json.gz")[..],
    )
    .read_to_end(&mut bytes)
    .unwrap();
    let cases: Value = serde_json::from_slice(&bytes).unwrap();
    let mut base = Runtime::for_overworld_verification(189, 0, false);
    base.provide((0, 0)).unwrap();
    base.access()
        .set_block_flags(IVec3::new(8, 1, 8), 58 << 4, 2)
        .unwrap();
    let base = base.checkpoint().unwrap();
    let mut failures = BTreeMap::new();
    let mut n = 0;
    for row in cases.as_array().unwrap() {
        let mut r = Runtime::resume(base.clone(), Arc::new(VanillaPopulation)).unwrap();
        let pos = DVec3::new(8.5, 2., 8.5);
        r.set_player_frame(PlayerFrame::standing("Player", pos))
            .unwrap();
        let initial = &row["initial"];
        let test = row["test"].as_u64().unwrap();
        let inv = Inventory {
            slots: slots(&initial[0], 40),
            crafting: slots(&initial[3], 9),
            ..Inventory::default()
        };
        let act = |r: &mut Runtime, action| {
            let stamp = r.inventory_view(1).stamp;
            assert!(r
                .inventory_action(1, stamp, action)
                .unwrap()
                .rejected
                .is_none());
        };
        act(
            &mut r,
            InventoryAction::Initialize {
                inventory: inv,
                creative: test == 4,
            },
        );
        act(
            &mut r,
            InventoryAction::OpenWorkbench {
                position: [8, 1, 8],
                player: pos,
            },
        );
        r.backend
            .player_inventory
            .as_mut()
            .unwrap()
            .inventory
            .crafting = slots(&initial[2], 9);
        r = Runtime::resume(r.checkpoint().unwrap(), Arc::new(VanillaPopulation)).unwrap();
        let op = match test {
            0 | 1 => ClickAction::Pickup { right: test == 1 },
            2 => ClickAction::QuickMove,
            3 => ClickAction::Throw { all: true },
            4 => ClickAction::Clone,
            _ => ClickAction::Hotbar(0),
        };
        act(
            &mut r,
            InventoryAction::Click {
                slot: SlotRef::CraftOutput,
                action: op,
                player: pos,
            },
        );
        for phase in ["clicked", "closed"] {
            if phase == "closed" {
                act(&mut r, InventoryAction::Close);
            }
            let actual = observed(&r);
            let expected = want(&row[phase]);
            for field in 0..5 {
                if actual[field] != expected[field] {
                    n += 1;
                    failures.entry((test, phase, field)).or_insert_with(|| {
                        format!(
                            "mode={} profile={} observed={} actual={}",
                            row["mode"], row["profile"], actual[field], expected[field]
                        )
                    });
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{n} workbench differences; first per action/phase: {failures:?}"
    );
}
