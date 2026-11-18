use super::*;
use serde::Deserialize;
type StoredStack = (u16, u8, u16, Option<String>);
type Change = (usize, Option<StoredStack>);
type InventoryCase = (u16, u16, bool, bool, usize, Vec<Change>, usize, Vec<Change>);
#[derive(Deserialize)]
struct Data {
    imports: Vec<(u16, String, String)>,
    registry: Vec<(u16, u16, u16, bool)>,
    plants: Vec<(u16, u16, u16)>,
    tiles: Vec<Tile>,
    inventory: Vec<InventoryCase>,
    entities: Vec<(u8, Option<StoredStack>)>,
    frames: Vec<(u16, u16, StoredStack, String)>,
}
#[test]
fn original_anvil_tile_read_write_normalization_before_ctrl_pick_matches() {
    let cases = data().imports;
    assert_eq!(cases.len(), 108);
    for (i, (state, input, output)) in cases.into_iter().enumerate() {
        assert_eq!(
            super::super::tile_import::load(state >> 4, IVec3::new(-3, 63, 19), &tag(&input))
                .unwrap(),
            tag(&output),
            "original TileEntity.createAndLoadEntity case {i}, block {}",
            state >> 4
        );
    }
}
#[derive(Deserialize)]
struct Tile {
    raw: u16,
    tile: String,
    name: String,
    plain: Option<StoredStack>,
    ctrl: Option<StoredStack>,
}
fn data() -> Data {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-middle-pick.json"
    ))
    .unwrap()
}
fn tag(s: &str) -> DropNbt {
    let bytes: Vec<_> = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect();
    item_nbt::decode(&bytes).unwrap().unwrap()
}
fn resolve(s: StoredStack) -> Stack {
    let (id, count, damage, nbt) = s;
    let mut stack = stack(
        Item {
            id,
            damage,
            match_damage: false,
        },
        count,
    );
    stack.nbt = nbt.map(|n| Arc::new(tag(&n)));
    stack
}
#[test]
fn original_middle_click_all_registry_blocks_tile_items_nbt_and_entity_types_match() {
    let d = data();
    assert_eq!(
        (
            d.registry.len(),
            d.plants.len(),
            d.tiles.len(),
            d.entities.len(),
            d.frames.len()
        ),
        (1394, 24, 83, 60, 20)
    );
    for (state, id, damage, match_damage) in d.registry {
        assert_eq!(
            block(state, 0, None),
            (id != 0).then_some(Item {
                id,
                damage,
                match_damage
            }),
            "packed {state}"
        );
    }
    for (state, lower, damage) in d.plants {
        assert_eq!(
            block(state, (175 << 4) | lower, None).unwrap().damage,
            damage
        );
    }
    for t in d.tiles {
        let stored = tag(&t.tile);
        let id = t.raw >> 4;
        assert!(compatible_tile(id, &t.name));
        let loaded = tile_snapshot(id, IVec3::new(-3, 63, 19), Some(&stored), None).unwrap();
        assert_eq!(loaded, stored);
        let plain = block(t.raw, 0, Some(&loaded));
        assert_eq!(
            plain.map(|v| stack(v, 1)),
            t.plain.map(resolve),
            "tile {} plain",
            t.name
        );
        assert_eq!(
            plain.map(|v| pick_with_tile(v, &loaded)),
            t.ctrl.map(resolve),
            "tile {} ctrl",
            t.name
        );
    }
    for (id, expected) in d.entities {
        assert_eq!(
            entity(id, None).map(|v| stack(v, 1)),
            expected.map(resolve),
            "entity id {id}"
        );
    }
    for (id, damage, expected, wire) in d.frames {
        let framed = stack(
            Item {
                id,
                damage,
                match_damage: true,
            },
            8,
        );
        let mut inv = Inventory::default();
        inv.slots.fill(None);
        select(&mut inv, entity(18, Some(&framed)).unwrap(), true);
        assert_eq!(inv.slots[0], Some(resolve(expected)));
        let mut packet = crate::protocol::Writer::new();
        crate::network::write_creative_slot(
            &mut packet,
            36,
            Some(&crate::remote::to_slot(inv.slots[0].as_ref().unwrap()).unwrap()),
        )
        .unwrap();
        assert_eq!(
            packet
                .0
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            wire
        );
    }
}
#[test]
fn original_inventory_pick_slots_counts_nbt_and_enchantability_match() {
    let cases = data().inventory;
    assert_eq!(cases.len(), 640);
    for (i, (id, damage, match_damage, creative, selected, before, end, changes)) in
        cases.into_iter().enumerate()
    {
        let mut inv = Inventory::default();
        inv.slots.fill(None);
        inv.selected = selected;
        for (i, s) in before {
            inv.slots[i] = s.map(resolve);
        }
        let mut expected = inv.slots.clone();
        for (i, s) in changes {
            expected[i] = s.map(resolve);
        }
        select(
            &mut inv,
            Item {
                id,
                damage,
                match_damage,
            },
            creative,
        );
        assert_eq!(inv.selected, end, "case {i}");
        assert_eq!(inv.slots, expected, "case {i}");
    }
}
