//! Original Container.slotClick traces for the specialty inventories exposed by a server.
use super::*;
use serde::Deserialize;

type Item = [u16; 3];
#[derive(Deserialize)]
struct Snapshot {
    slots: Vec<Option<Item>>,
    cursor: Option<Item>,
}
#[derive(Deserialize)]
struct Step {
    #[serde(flatten)]
    after: Snapshot,
    click: [usize; 3],
    returned: Option<Item>,
}
#[derive(Deserialize)]
struct Case {
    kind: String,
    variant: usize,
    initial: Snapshot,
    steps: Vec<Step>,
}
#[derive(Deserialize)]
struct Fixture {
    ingredients: std::collections::BTreeMap<u16, [u16; 2]>,
    cases: Vec<Case>,
    server_brewing_split: Step,
}
fn stack([id, count, meta]: Item) -> Stack {
    crate::vanilla::item_entity::stack_from_metadata(id, count as u8, meta).unwrap()
}
fn item(s: &Stack) -> Item {
    let meta = if crate::vanilla::armor::maximum_damage(s.id) > 0 {
        s.damage
    } else {
        s.meta
    };
    [s.id, u16::from(s.count), meta]
}
fn address(slot: usize, upper: usize) -> SlotRef {
    if slot < upper {
        SlotRef::Container(slot)
    } else if slot < upper + 27 {
        SlotRef::Player(slot - upper + 9)
    } else {
        SlotRef::Player(slot - upper - 27)
    }
}
#[test]
fn actual_brewing_horse_hopper_and_dispenser_slot_clicks() {
    let data: Fixture = serde_json::from_str(include_str!(
        "../tests/fixtures/vanilla189-other-windows.json"
    ))
    .unwrap();
    let client = &data
        .cases
        .iter()
        .find(|c| c.kind == "brewing" && c.variant == 0)
        .unwrap()
        .steps[6];
    let server = &data.server_brewing_split;
    assert_eq!(client.click, server.click);
    assert_eq!(client.returned, Some([374, 6, 0]));
    assert_eq!(client.returned, server.returned);
    assert_eq!(client.after.cursor, Some([374, 3, 0]));
    assert_eq!(server.after.cursor, Some([374, 6, 0]));
    let mut count = 0;
    for case in data.cases {
        let upper = case.initial.slots.len() - 36;
        let mut c = Container::new(Kind::Chest);
        c.special = match case.kind.as_str() {
            "brewing" => Special::Brewing,
            "horse" => Special::Horse { armor: true },
            "donkey" => Special::Horse { armor: false },
            "hopper" => Special::Hopper,
            "dispenser" => Special::Dispenser,
            s => panic!("unknown fixture {s}"),
        };
        c.slots = case.initial.slots[..upper]
            .iter()
            .map(|v| v.map(stack))
            .collect();
        let mut inv = Inventory::default();
        inv.cursor = case.initial.cursor.map(stack);
        for (i, v) in case.initial.slots.iter().enumerate().skip(upper) {
            let SlotRef::Player(slot) = address(i, upper) else {
                unreachable!()
            };
            inv.slots[slot] = v.map(stack);
        }
        for (step, action) in case.steps.into_iter().enumerate() {
            let [wire, button, mode] = action.click;
            let target = address(wire, upper);
            let before = match target {
                SlotRef::Player(i) => &inv.slots[i],
                SlotRef::Container(i) => &c.slots[i],
                _ => unreachable!(),
            }
            .as_ref()
            .map(item);
            let click = match mode {
                0 => ClickAction::Pickup { right: button == 1 },
                1 => ClickAction::QuickMove,
                _ => unreachable!(),
            };
            let (_, moved) = predict_window_click(&mut inv, Some(&mut c), target, click, false, 2);
            let actual = c
                .slots
                .iter()
                .chain(inv.slots[9..36].iter())
                .chain(inv.slots[0..9].iter())
                .map(|s| s.as_ref().map(item))
                .collect::<Vec<_>>();
            let context = format!(
                "{} {} step {step}, click {:?}",
                case.kind, case.variant, action.click
            );
            assert_eq!(actual, action.after.slots, "{context}");
            assert_eq!(
                inv.cursor.as_ref().map(item),
                action.after.cursor,
                "{context} cursor"
            );
            assert_eq!(
                if mode == 1 && !moved { None } else { before },
                action.returned,
                "{context} clicked stack"
            );
            count += 1;
        }
    }
    assert_eq!(count, 75);
    // The generator scans every registered item over the full 0..32767 metadata
    // range. Test both ends of every range and every fish subtype here.
    for id in 0..=2267 {
        if crate::vanilla::item_entity::facts(id).is_none() {
            continue;
        }
        for meta in [0, 1, 2, 3, 4, 15, 32767] {
            let expected = data
                .ingredients
                .get(&id)
                .is_some_and(|r| (r[0]..=r[1]).contains(&meta));
            assert_eq!(
                brewing_ingredient(&stack([id, 1, meta])),
                expected,
                "id={id} meta={meta}"
            );
        }
    }
}
