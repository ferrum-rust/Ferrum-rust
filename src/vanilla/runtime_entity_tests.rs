use super::*;
use crate::{inventory::Inventory, vanilla::drops::DropNbt, world::Chunk};
use serde_json::{json, Value};
use std::io::Read;

struct Flat;
impl Hooks for Flat {
    fn populate(&self, _: &mut Access<'_>, _: ChunkPos, _: &mut JavaRandom) -> Result<()> {
        Ok(())
    }
    fn item_random_state(&self, id: u64) -> u64 {
        JavaRandom::new(917 + 31 * id as i64).state()
    }
    fn player_random_state(&self) -> u64 {
        JavaRandom::new(521).state()
    }
    fn falling_random_state(&self, id: u64) -> u64 {
        JavaRandom::new(189 + 17 * id as i64).state()
    }
    fn orb_random_state(&self, id: i32) -> u64 {
        JavaRandom::new(991 + 41 * i64::from(id)).state()
    }
}
fn base() -> RuntimeSnapshot {
    let mut runtime = Runtime::new(189, 0, false, Arc::new(Flat));
    for x in -3..=3 {
        for z in -3..=3 {
            runtime.provide((x, z)).unwrap();
        }
    }
    let mut flat = Chunk::empty();
    flat.blocks[..256].fill(7 << 4);
    flat.top = 1;
    for c in runtime.backend.world.chunks.values_mut() {
        *c = Arc::new(flat.clone());
    }
    runtime.backend.light = Lighting::new(&runtime.backend.world, false);
    runtime.backend.world_random.set_seed(0);
    runtime.backend.entities.math_state = Some(JavaRandom::new(3189).state());
    runtime.backend.entities.inventory_random = Some((JavaRandom::new(491).state(), None));
    runtime.checkpoint().unwrap()
}
fn stack(id: u16, count: u8, meta: u16, tag: i32) -> Stack {
    let s = item_entity::stack_from_metadata(id, count, meta).unwrap();
    if tag == 0 {
        s
    } else {
        s.with_nbt(DropNbt::Compound(BTreeMap::from([(
            "variant".into(),
            DropNbt::Int(tag),
        )])))
    }
}
fn value(s: &Stack) -> Value {
    let tag = match s.nbt.as_deref() {
        Some(DropNbt::Compound(m)) => match m.get("variant") {
            Some(DropNbt::Int(n)) => *n,
            _ => 0,
        },
        _ => 0,
    };
    json!([
        s.id,
        s.count,
        if crate::vanilla::armor::maximum_damage(s.id) > 0 {
            s.damage
        } else {
            s.meta
        },
        tag
    ])
}
fn action(r: &mut Runtime, op: InventoryAction) {
    let stamp = r.inventory_view(1).stamp;
    assert!(r.inventory_action(1, stamp, op).unwrap().rejected.is_none());
}
fn frame(r: &mut Runtime, x: f64, offset: f64) {
    let mut f = PlayerFrame::standing("Player", DVec3::new(x + offset, 1., 8.5));
    f.yaw = (x * 13.7) as f32;
    f.pitch = 12.375;
    r.set_player_frame(f).unwrap();
}
fn prepare(r: &mut Runtime, m: usize, x: f64) {
    r.backend.entities.next_entity_id = if x < 0. {
        -49
    } else if x > 15. {
        57
    } else {
        3
    };
    r.backend.entities.register_player_id();
    let first = matches!(m, 0 | 2 | 9 | 11 | 13);
    let mut inventory = Inventory::default();
    if m == 2 {
        for s in &mut inventory.slots[..36] {
            *s = Some(stack(4, 64, 0, 0));
        }
        inventory.slots[5] = Some(stack(264, 62, 0, 0));
    }
    if m == 6 {
        inventory.cursor = Some(stack(35, 9, 2, 6));
        inventory.crafting[0] = Some(stack(264, 4, 0, 0));
        inventory.crafting[3] = Some(stack(276, 1, 919, 0));
    }
    if matches!(m, 7 | 8) {
        inventory.slots[0] = Some(stack(264, 8, 0, 7));
    }
    if first {
        frame(r, x, 0.);
    }
    if m <= 3 || m == 9 {
        for (offset, id, count, delay) in [
            (0.15, 264, 5, if m == 0 { 0 } else { 1 }),
            (-0.12, 264, 3, if m == 9 { 23 } else { 0 }),
            (0.26, 17, 1, if m == 9 { 15 } else { 0 }),
        ] {
            let b = &mut r.backend;
            b.entities.spawn(
                b.hooks.as_ref(),
                DVec3::new(x + offset, 1., 8.5),
                stack(id, count, 0, 0),
                delay,
                true,
            );
        }
    }
    if m >= 10 {
        for (offset, id, value) in [
            (0.15, 0, 5),
            (-0.12, 17, 1),
            (-0.21, 0, if m == 13 { 1237 } else { 3 }),
            (0.16, 264, 1),
            (0.27, 0, 17),
        ] {
            let b = &mut r.backend;
            let p = DVec3::new(x + offset, 1., 8.5);
            if id == 0 {
                b.entities
                    .emit(
                        b.hooks.as_ref(),
                        &b.world,
                        &mut b.drops,
                        DropEvent::Experience {
                            position: p.to_array(),
                            value,
                        },
                    )
                    .unwrap();
            } else {
                b.entities
                    .spawn(b.hooks.as_ref(), p, stack(id, 1, 0, 0), 0, true);
            }
        }
    }
    if !first {
        frame(r, x, if matches!(m, 3 | 12) { 6. } else { 0. });
    }
    action(
        r,
        InventoryAction::Initialize {
            inventory,
            creative: false,
        },
    );
    if m == 4 {
        let p = IVec3::new(x.floor() as i32, 1, 8);
        r.access().set_block_flags(p, 54 << 4 | 2, 3).unwrap();
        let tile = r.backend.live_containers.get_mut(&p.to_array()).unwrap();
        tile.slots[0] = Some(stack(264, 64, 0, 7));
        tile.slots[3] = Some(stack(276, 1, 919, 0));
        tile.slots[26] = Some(stack(35, 31, 8, 0));
        r.access().set_block_flags(p, 0, 3).unwrap();
        frame(r, x, 6.);
    }
    if m == 5 {
        for (y, state) in [(1, 44 << 4), (6, 12 << 4 | 1)] {
            r.access()
                .set_block_flags(IVec3::new(x.floor() as i32, y, 8), state, 3)
                .unwrap();
        }
        frame(r, x, 6.);
    }
    if m == 6 {
        action(r, InventoryAction::Close);
    }
    if matches!(m, 7 | 8) {
        action(r, InventoryAction::DropSelected { all: m == 8 });
    }
}
fn observed(r: &mut Runtime) -> Value {
    let b = &mut r.backend;
    let es = &mut b.entities;
    let mut order = Vec::new();
    for key in &es.order {
        match *key {
            EntityKey::Orb(id) => {
                let o = &es.orbs[&id];
                let bits = o
                    .body
                    .position
                    .to_array()
                    .into_iter()
                    .chain(o.body.velocity.to_array())
                    .map(|v| format!("{:x}", v.to_bits()))
                    .collect::<Vec<_>>();
                let s = o.section.unwrap();
                order.push(json!([
                    "xp",
                    id,
                    o.body.value,
                    o.body.age,
                    o.body.dead,
                    bits,
                    o.body.random_state.to_string(),
                    s[0],
                    s[1],
                    s[2]
                ]));
            }
            EntityKey::Player => order.push(json!(["p"])),
            EntityKey::Falling(id) => {
                let e = b.falling.live.iter().find(|v| v.id == id).unwrap();
                order.push(json!(["f", id, e.body.age]));
            }
            EntityKey::Item(id) => {
                let i = &es.live[&id];
                let bits = i
                    .position
                    .to_array()
                    .into_iter()
                    .chain(i.velocity.to_array())
                    .map(|v| format!("{:x}", v.to_bits()))
                    .collect::<Vec<_>>();
                let s = i.section.unwrap();
                order.push(json!([
                    "i",
                    id,
                    value(&i.stack),
                    i.age,
                    i.pickup_delay,
                    i.ticks,
                    i.dead,
                    bits,
                    i.random_state.to_string(),
                    s[0],
                    s[1],
                    s[2],
                    i.thrower
                ]));
            }
        }
    }
    let slots = b.player_inventory.as_ref().unwrap().inventory.slots[..36]
        .iter()
        .enumerate()
        .filter_map(|(i, s)| Some(json!([i, value(s.as_ref()?)])))
        .collect::<Vec<_>>();
    let events = std::mem::take(&mut es.effects)
        .into_iter()
        .filter_map(|e| match e {
            EntityEffect::Stat { id, .. } => Some(json!(["stat", id])),
            EntityEffect::Pickup { item_id, count, .. } => Some(json!(["pickup", item_id, count])),
            EntityEffect::OrbPickup { orb_id, .. } => Some(json!(["xpPickup", orb_id, 1])),
            EntityEffect::Item(ItemEffect::Sound {
                name,
                volume,
                pitch,
                ..
            }) if matches!(
                name.as_str(),
                "random.pop" | "random.orb" | "random.levelup"
            ) =>
            {
                Some(json!([
                    "sound",
                    name,
                    volume.to_bits() as i32,
                    pitch.to_bits() as i32
                ]))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    json!([
        r.provider.world_time,
        order,
        slots,
        events,
        es.math_state.unwrap().to_string(),
        es.inventory_random.unwrap().0.to_string(),
        es.player_random.unwrap().to_string(),
        b.world_random.state().to_string(),
        [
            es.experience.total,
            es.experience.level,
            es.experience.progress.to_bits() as i32,
            es.experience.cooldown,
            es.experience.score
        ]
    ])
}
#[test]
fn original_worldserver_item_player_falling_iteration_and_spills_match_on_resume() {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("../../tests/fixtures/vanilla189-runtime-items.json.gz")[..],
    )
    .read_to_end(&mut bytes)
    .unwrap();
    let rows: Value = serde_json::from_slice(&bytes).unwrap();
    let start = base();
    for row in rows.as_array().unwrap() {
        let mode = row["mode"].as_u64().unwrap() as usize;
        let x = row["x"].as_f64().unwrap();
        let mut r = Runtime::resume(start.clone(), Arc::new(Flat)).unwrap();
        prepare(&mut r, mode, x);
        for (tick, expected) in row["frames"].as_array().unwrap().iter().enumerate() {
            if tick == 30 && matches!(mode, 3..=5 | 12) {
                frame(&mut r, x, 0.);
            }
            if tick > 0 {
                r.tick_blocks_and_falling().unwrap();
            }
            let got = observed(&mut r);
            for field in 0..9 {
                assert_eq!(
                    got[field], expected[field],
                    "mode={mode} x={x} tick={tick} field={field}"
                );
            }
            if tick == 17 {
                let mut snapshot = r.checkpoint().unwrap();
                let bytes = serde_json::to_vec(&snapshot.entities).unwrap();
                snapshot.entities = serde_json::from_slice(&bytes).unwrap();
                r = Runtime::resume(snapshot, Arc::new(Flat)).unwrap();
            }
        }
    }
}

#[test]
fn thrown_stacks_and_nonfinite_item_tags_survive_reconnect_without_replay() {
    let mut r = Runtime::new(189, 0, false, Arc::new(Flat));
    r.provide((0, 0)).unwrap();
    let tag = DropNbt::Compound(BTreeMap::from([(
        "payload".into(),
        DropNbt::Float(f32::from_bits(0x7fc01234)),
    )]));
    let mut inventory = Inventory::default();
    inventory.slots[0] = Some(stack(264, 2, 0, 0).with_nbt(tag.clone()));
    action(
        &mut r,
        InventoryAction::Initialize {
            inventory,
            creative: false,
        },
    );
    let stamp = r.inventory_view(1).stamp;
    let before = r.backend.entities.math_state;
    assert!(r
        .inventory_action(1, stamp, InventoryAction::DropSelected { all: false })
        .unwrap()
        .rejected
        .is_some());
    assert_eq!(r.backend.entities.math_state, before);
    assert!(r.backend.entities.live.is_empty());
    frame(&mut r, 8.5, 0.);
    assert!(r
        .inventory_action(1, stamp, InventoryAction::DropSelected { all: false })
        .unwrap()
        .rejected
        .is_none());
    assert!(r
        .inventory_action(1, stamp, InventoryAction::DropSelected { all: false })
        .unwrap()
        .rejected
        .is_some());
    let copy: RuntimeSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&r.checkpoint().unwrap()).unwrap()).unwrap();
    let mut r = Runtime::resume(copy, Arc::new(Flat)).unwrap();
    assert!(r
        .inventory_action(2, stamp, InventoryAction::DropSelected { all: false })
        .unwrap()
        .rejected
        .is_some());
    let player = &r.backend.player_inventory.as_ref().unwrap().inventory;
    let encoded = crate::vanilla::item_nbt::encode(&tag).unwrap();
    assert_eq!(player.slots[0].as_ref().unwrap().count, 1);
    assert_eq!(r.backend.entities.live.len(), 1);
    let body = &r.backend.entities.live[&0];
    assert_eq!(body.stack.count, 1);
    assert_eq!(
        crate::vanilla::item_nbt::encode(body.stack.nbt.as_ref().unwrap()).unwrap(),
        encoded
    );
    let DropEvent::Item { stack, .. } = &r.backend.drops[0] else {
        panic!("Missing item spawn record")
    };
    assert_eq!(
        crate::vanilla::item_nbt::encode(stack.nbt.as_ref().unwrap()).unwrap(),
        encoded
    );
    let legacy = json!({"id":264,"count":1,"meta":0,"nbt":{"Compound":{"variant":{"Int":7}}}});
    let s: DropStack = serde_json::from_value(legacy).unwrap();
    assert_eq!(
        s.nbt,
        Some(DropNbt::Compound(BTreeMap::from([(
            "variant".into(),
            DropNbt::Int(7)
        )])))
    );
}
