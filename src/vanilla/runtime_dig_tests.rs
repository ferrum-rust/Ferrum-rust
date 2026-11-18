use super::*;
use crate::{
    inventory::{Inventory, Stack},
    vanilla::drops::{DropEvent, DropNbt},
    world::Chunk,
};
use glam::DVec3;
use serde_json::{json, Value};
use std::io::Read;
const POS: IVec3 = IVec3::new(8, 4, 8);
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
fn bits(p: [f64; 3]) -> [String; 3] {
    p.map(|v| format!("{:x}", v.to_bits()))
}
fn start(r: &mut Runtime, row: &Value) {
    for edit in row["edits"].as_array().unwrap() {
        let [x, y, z, state]: [i32; 4] = serde_json::from_value(edit.clone()).unwrap();
        r.access()
            .set_block_flags(IVec3::new(x, y, z), state as u16, 2)
            .unwrap();
    }
    let variant = row["v"].as_i64().unwrap();
    let id = row["state"].as_u64().unwrap() >> 4;
    if matches!(id, 140 | 144) {
        let mut tile = if id == 140 {
            BTreeMap::from([
                ("Item".into(), DropNbt::String("minecraft:sapling".into())),
                ("Data".into(), DropNbt::Int(3)),
            ])
        } else {
            BTreeMap::from([("SkullType".into(), DropNbt::Byte(1))])
        };
        tile.insert(
            "id".into(),
            DropNbt::String(if id == 140 {
                "FlowerPot".into()
            } else {
                "Skull".into()
            }),
        );
        Arc::make_mut(r.backend.world.chunks.get_mut(&(0, 0)).unwrap())
            .tiles
            .insert([8, 4, 8], Arc::new(DropNbt::Compound(tile)));
    }
    let mut frame = PlayerFrame::standing(
        "Player",
        DVec3::new(8.5, 3., 8.5 + row["distance"].as_f64().unwrap()),
    );
    frame.grounded = row["ground"].as_bool().unwrap();
    r.set_player_frame(frame).unwrap();
    let id = row["item"].as_u64().unwrap() as u16;
    let mut inv = Inventory::default();
    if id != 0 {
        let mut s = Stack::new(id, 1);
        if variant == 10 {
            s.damage = armor::maximum_damage(id) as u16;
        }
        let enchants = match variant {
            2 => vec![(32, 4), (35, 3)],
            3 => vec![(33, 1)],
            _ => vec![],
        };
        if !enchants.is_empty() {
            let tags = enchants
                .into_iter()
                .map(|(id, lvl)| {
                    DropNbt::Compound(BTreeMap::from([
                        ("id".into(), DropNbt::Short(id)),
                        ("lvl".into(), DropNbt::Short(lvl)),
                    ]))
                })
                .collect();
            s = s.with_nbt(DropNbt::Compound(BTreeMap::from([(
                "ench".into(),
                DropNbt::List {
                    element: 10,
                    values: tags,
                },
            )])));
        }
        inv.slots[0] = Some(s);
    }
    let stamp = r.inventory_view(1).stamp;
    r.inventory_action(
        1,
        stamp,
        InventoryAction::Initialize {
            inventory: inv,
            creative: row["creative"].as_bool().unwrap(),
        },
    )
    .unwrap();
    r.backend.entities.math_state = Some(JavaRandom::new(1167 + variant).state());
    r.backend.entities.player_random = Some(JavaRandom::new(189 + variant).state());
    r.backend.world_random.set_seed(731);
    r.backend.entities.effects.clear();
    r.backend.drops.clear();
}
fn observe(r: &mut Runtime, corrections: Vec<([i32; 3], u16)>, exhaustion: &mut f32) -> Value {
    let mut cracks = Vec::new();
    let mut aux = Vec::new();
    let mut drops = Vec::new();
    let mut stats = Vec::new();
    for e in std::mem::take(&mut r.backend.entities.effects) {
        match e {
            EntityEffect::BreakProgress {
                position: [x, y, z],
                stage,
            } => cracks.push(json!([x, y, z, stage])),
            EntityEffect::BlockBreak {
                position: [x, y, z],
                state,
            } => aux.push(json!([2001, x, y, z, (state >> 4) | ((state & 15) << 12)])),
            EntityEffect::WorldEvent {
                position: [x, y, z],
                id,
            } => aux.push(json!([id, x, y, z, 0])),
            EntityEffect::Stat { id, .. } => stats.push(json!(id)),
            EntityEffect::Exhaustion { amount } => *exhaustion = (*exhaustion + amount).min(40.),
            _ => {}
        }
    }
    for e in std::mem::take(&mut r.backend.drops) {
        match e {
            DropEvent::Item {
                position, stack, ..
            } => drops.push(json!([
                "item",
                bits(position),
                stack.id,
                stack.count,
                stack.meta
            ])),
            DropEvent::Experience { position, value } => {
                drops.push(json!(["xp", bits(position), value]))
            }
            _ => panic!("Unexpected original mining fixture entity"),
        }
    }
    let b = &r.backend;
    let d = &b.entities.digging;
    let held = b
        .player_inventory
        .as_ref()
        .unwrap()
        .inventory
        .held()
        .map(|s| {
            json!([
                s.id,
                s.count,
                if armor::maximum_damage(s.id) > 0 {
                    s.damage
                } else {
                    s.meta
                }
            ])
        });
    let corr = corrections
        .into_iter()
        .map(|([x, y, z], s)| [x, y, z, i32::from(s)])
        .collect::<Vec<_>>();
    json!([
        [
            d.clock,
            d.active,
            d.position.to_array(),
            d.initial,
            d.finishing,
            d.finish_position.to_array(),
            d.finish_initial,
            d.stage
        ],
        b.world.get(POS),
        held,
        corr,
        cracks,
        aux,
        drops,
        stats,
        exhaustion.to_bits() as i32,
        b.world_random.state().to_string(),
        b.entities.player_random.unwrap().to_string(),
        b.entities.math_state.unwrap().to_string()
    ])
}
#[test]
fn original_server_c07_state_progress_break_drops_and_rng_match_with_resume() {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("../../tests/fixtures/vanilla189-runtime-dig.json.gz")[..],
    )
    .read_to_end(&mut bytes)
    .unwrap();
    let cases: Value = serde_json::from_slice(&bytes).unwrap();
    let base = base();
    let mut failures = BTreeMap::new();
    let mut total = 0;
    for row in cases.as_array().unwrap() {
        let v = row["v"].as_i64().unwrap();
        let mut r = Runtime::resume(base.clone(), Arc::new(Flat)).unwrap();
        start(&mut r, row);
        let mut exhaustion = 0.;
        for (t, expected) in row["frames"].as_array().unwrap().iter().enumerate() {
            if t > 0 {
                r.tick_digging().unwrap();
            }
            let action = if t == 0 {
                Some(DigAction::Start)
            } else if t == 2 && v == 4 {
                Some(DigAction::Abort)
            } else if t == if v == 1 { 12 } else { 3 } {
                Some(DigAction::Stop)
            } else {
                None
            };
            let corrections = action.map_or_else(Vec::new, |action| {
                let request = DigBlock {
                    stamp: r.inventory_view(1).stamp,
                    position: POS.to_array(),
                    face: 1,
                    action,
                };
                r.dig_block(1, request).unwrap().corrections
            });
            let got = observe(&mut r, corrections, &mut exhaustion);
            for i in 0..12 {
                if got[i] != expected[i] {
                    total += 1;
                    failures
                        .entry((row["state"].as_i64().unwrap(), i))
                        .or_insert_with(|| {
                            format!("v={v} t={t} observed={} original={}", got[i], expected[i])
                        });
                }
            }
            if t == 4 {
                let mut snapshot = r.checkpoint().unwrap();
                let data = serde_json::to_vec(&snapshot.entities).unwrap();
                snapshot.entities = serde_json::from_slice(&data).unwrap();
                r = Runtime::resume(snapshot, Arc::new(Flat)).unwrap();
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{total} C07 differences. First per block/field: {failures:?}"
    );
}
