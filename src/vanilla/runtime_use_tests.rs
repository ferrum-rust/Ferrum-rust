use super::*;
use crate::{inventory::Inventory, world::Chunk};
use serde_json::{json, Value};
use std::io::Read;
struct Flat;
impl Hooks for Flat {
    fn populate(&self, _: &mut Access<'_>, _: ChunkPos, _: &mut JavaRandom) -> Result<()> {
        Ok(())
    }
}
fn base() -> RuntimeSnapshot {
    let mut r = Runtime::new(189, 0, false, Arc::new(Flat));
    for x in -1..=1 {
        for z in -1..=1 {
            r.provide((x, z)).unwrap();
        }
    }
    let mut flat = Chunk::empty();
    flat.blocks[..256].fill(7 << 4);
    flat.top = 1;
    for c in r.backend.world.chunks.values_mut() {
        *c = Arc::new(flat.clone());
    }
    r.backend.light = Lighting::new(&r.backend.world, false);
    r.checkpoint().unwrap()
}
fn region(r: &Runtime, clicked: IVec3) -> BTreeMap<[i32; 3], u16> {
    let mut map = BTreeMap::new();
    for x in 6..=10 {
        for y in (clicked.y - 2).max(0)..=(clicked.y + 2).min(255) {
            for z in 6..=10 {
                let p = IVec3::new(x, y, z);
                map.insert(p.to_array(), r.backend.world.get(p));
            }
        }
    }
    map
}
#[test]
fn original_c08_handler_block_edits_activation_corrections_and_held_counts_match() {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("../../tests/fixtures/vanilla189-runtime-use.json.gz")[..],
    )
    .read_to_end(&mut bytes)
    .unwrap();
    let data: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json!(BLOCK_ACTIVATION), data["overrides"]);
    let start = base();
    for row in data["cases"].as_array().unwrap() {
        let mode = row["mode"].as_u64().unwrap();
        let side = row["side"].as_u64().unwrap() as u8;
        let variant = row["variant"].as_u64().unwrap() as u8;
        let label = (mode, side, variant);
        let mut r = Runtime::resume(start.clone(), Arc::new(Flat)).unwrap();
        for edit in row["edits"].as_array().unwrap() {
            let [x, y, z, state]: [i32; 4] = serde_json::from_value(edit.clone()).unwrap();
            r.access()
                .set_block_flags(IVec3::new(x, y, z), state as u16, 2)
                .unwrap();
        }
        let [x, y, z, expected]: [i32; 4] = serde_json::from_value(row["clicked"].clone()).unwrap();
        let clicked = IVec3::new(x, y, z);
        let position: [f64; 3] = serde_json::from_value(row["pos"].clone()).unwrap();
        let mut frame = PlayerFrame::standing("Player", DVec3::from_array(position));
        frame.yaw = row["yaw"].as_f64().unwrap() as f32;
        frame.sneaking = row["sneak"].as_bool().unwrap();
        r.set_player_frame(frame).unwrap();
        let [item, meta]: [u16; 2] = serde_json::from_value(row["item"].clone()).unwrap();
        let mut inventory = Inventory::default();
        if item != 0 {
            let mut stack = Stack::new(item, 3);
            stack.meta = meta;
            inventory.slots[0] = Some(stack);
        }
        let stamp = r.inventory_view(1).stamp;
        r.inventory_action(
            1,
            stamp,
            InventoryAction::Initialize {
                inventory,
                creative: row["creative"].as_bool().unwrap(),
            },
        )
        .unwrap();
        let before = region(&r, clicked);
        let request = UseBlock {
            stamp: r.inventory_view(1).stamp,
            clicked: clicked.to_array(),
            expected: expected as u16,
            face: side,
            cursor: [8, (variant % 5) * 4, 8],
        };
        let reply = r.use_block(1, request.clone()).unwrap();
        let changed = region(&r, clicked)
            .into_iter()
            .filter(|(p, s)| before.get(p) != Some(s))
            .map(|(p, s)| [p[0], p[1], p[2], i32::from(s)])
            .collect::<Vec<_>>();
        let mut want: Vec<[i32; 4]> = serde_json::from_value(row["changed"].clone()).unwrap();
        want.sort();
        assert_eq!(changed, want, "writes {label:?}, {:?}", reply.outcome);
        let corrections = reply
            .corrections
            .map(|(p, s)| [p[0], p[1], p[2], i32::from(s)]);
        assert_eq!(
            json!(corrections),
            row["corrected"],
            "corrections {label:?}"
        );
        let aux = reply
            .auxiliary
            .map(|id| [x, y, z, id])
            .into_iter()
            .collect::<Vec<_>>();
        assert_eq!(json!(aux), row["aux"], "aux {label:?}");
        let sounds = reply
            .place_sound
            .map(|(p, name, v, pitch)| json!([p[0], p[1], p[2], name, v, pitch]))
            .into_iter()
            .collect::<Vec<_>>();
        let expected_sounds = row["sounds"].as_array().unwrap();
        assert_eq!(sounds.len(), expected_sounds.len(), "sound count {label:?}");
        for (got, want) in sounds.iter().zip(expected_sounds) {
            for i in 0..4 {
                assert_eq!(got[i], want[i], "sound {label:?} field={i}");
            }
            for i in 4..6 {
                assert_eq!(
                    got[i].as_f64().unwrap() as f32,
                    want[i].as_f64().unwrap() as f32,
                    "sound {label:?} field={i}"
                );
            }
        }
        let stats = r
            .backend
            .entities
            .effects
            .iter()
            .filter_map(|e| {
                if let EntityEffect::Stat { id, .. } = e {
                    Some(id)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(json!(stats), row["stats"], "stats {label:?}");
        let player = r.backend.player_inventory.as_ref().unwrap();
        assert_eq!(
            json!(player.inventory.slots[0].as_ref().map_or(0, |s| s.count)),
            row["count"],
            "count {label:?}"
        );
        assert_eq!(json!(player.window_id), row["window"], "window {label:?}");
        assert_eq!(
            r.use_block(1, request).unwrap().outcome,
            UseOutcome::Stale,
            "retry {label:?}"
        );
    }
}
