//! Original Block.dropBlockAsItemWithChance dispatch and world.rand ordering.
//! Tool/silk-touch harvesting and breakBlock inventory ejection are separate
//! original callbacks; this API does not substitute for those callbacks.
use super::random::JavaRandom;
use anyhow::{ensure, Context, Result};
use glam::IVec3;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
include!("drops_data.rs");

/// Typed 1.8.9 NBT. Preserve arbitrary banner-pattern data, including numeric
/// tag widths and empty-list element type, instead of reducing it to JSON text.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum DropNbt {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Bytes(Vec<i8>),
    String(String),
    List { element: u8, values: Vec<DropNbt> },
    Compound(BTreeMap<String, DropNbt>),
    Ints(Vec<i32>),
    Longs(Vec<i64>),
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DropStack {
    pub id: u16,
    pub count: u8,
    pub meta: i32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "super::item_nbt::optional_owned"
    )]
    pub nbt: Option<DropNbt>,
}
impl DropStack {
    fn new(id: u16, meta: i32) -> Self {
        Self {
            id,
            count: 1,
            meta: meta.max(0),
            nbt: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind")]
pub enum DropEvent {
    Item {
        position: [f64; 3],
        stack: DropStack,
        pickup_delay: i32,
    },
    Experience {
        position: [f64; 3],
        value: i32,
    },
    Silverfish {
        position: [f64; 3],
        yaw: f32,
    },
    /// Entity status for the immediately preceding silverfish spawn.
    EntityStatus {
        status: u8,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DropTile {
    /// Serialized TileEntityBanner.writeToNBT compound; coordinates/id are
    /// removed by the original block callback before copying BlockEntityTag.
    Banner {
        base_color: i32,
        written_tag: BTreeMap<String, DropNbt>,
    },
    Piston {
        state: u16,
    },
}
pub trait DropWorld {
    fn is_remote(&self) -> bool;
    fn do_tile_drops(&self) -> bool;
    fn tile_entity(&mut self, p: IVec3) -> Result<Option<DropTile>>;
    fn emit(&mut self, event: DropEvent) -> Result<()>;
}
fn state_data(state: u16) -> Result<(i16, i16)> {
    let index = STATES
        .binary_search_by_key(&state, |row| row.0)
        .ok()
        .with_context(|| format!("Invalid original block state {state}"))?;
    Ok((STATES[index].1, STATES[index].2))
}
pub(crate) fn spawn_item(
    world: &mut impl DropWorld,
    random: &mut JavaRandom,
    p: IVec3,
    stack: DropStack,
) -> Result<()> {
    if world.is_remote() || !world.do_tile_drops() {
        return Ok(());
    }
    let position = p
        .to_array()
        .map(|n| n as f64 + ((random.next_float() * 0.5f32) as f64 + 0.25));
    world.emit(DropEvent::Item {
        position,
        stack,
        pickup_delay: 10,
    })
}
fn experience(world: &mut impl DropWorld, p: IVec3, mut value: i32) -> Result<()> {
    if world.is_remote() {
        return Ok(());
    }
    while value > 0 {
        let split = [2477, 1237, 617, 307, 149, 73, 37, 17, 7, 3, 1]
            .into_iter()
            .find(|n| *n <= value)
            .unwrap();
        value -= split;
        world.emit(DropEvent::Experience {
            position: p.to_array().map(|n| n as f64 + 0.5),
            value: split,
        })?;
    }
    Ok(())
}
fn quantity(state: u16, fortune: i32, random: &mut JavaRandom) -> i32 {
    match state >> 4 {
        8..=11 | 20 | 34 | 51 | 52 | 78 | 79 | 90 | 92 | 95 | 106 | 115 | 119 | 137 | 174 => 0,
        47 => 3,
        80 | 82 => 4,
        130 => 8,
        43 | 125 | 181 => 2,
        89 => (2 + random.next_int(3) + random.next_int(fortune + 1)).clamp(1, 4),
        99 | 100 => (random.next_int(10) - 7).max(0),
        103 => (3 + random.next_int(5) + random.next_int(fortune + 1)).min(9),
        73 | 74 => 4 + random.next_int(2) + random.next_int(fortune + 1),
        169 => (2 + random.next_int(2) + random.next_int(fortune + 1)).clamp(1, 5),
        31 => 1 + random.next_int(fortune * 2 + 1),
        16 | 21 | 56 | 129 | 153 => {
            let bonus = if fortune > 0 {
                (random.next_int(fortune + 2) - 1).max(0) + 1
            } else {
                1
            };
            let count = if state >> 4 == 21 {
                4 + random.next_int(5)
            } else {
                1
            };
            count.wrapping_mul(bonus)
        }
        _ => 1,
    }
}
fn item(state: u16, fortune: i32, random: &mut JavaRandom) -> Result<Option<DropStack>> {
    let (mut id, meta) = state_data(state)?;
    match state >> 4 {
        13 => {
            id = if random.next_int(10 - fortune.min(3) * 3) == 0 {
                318
            } else {
                13
            }
        }
        31 => id = if random.next_int(8) == 0 { 295 } else { -1 },
        175 if state & 8 == 0 && state & 7 == 2 => {
            id = if random.next_int(8) == 0 { 295 } else { -1 }
        }
        _ => {}
    }
    Ok((id >= 0).then(|| DropStack::new(id as u16, i32::from(meta))))
}
fn base_drop(
    world: &mut impl DropWorld,
    random: &mut JavaRandom,
    p: IVec3,
    state: u16,
    chance: f32,
    fortune: i32,
) -> Result<()> {
    if world.is_remote() {
        return Ok(());
    }
    let count = quantity(state, fortune, random);
    for _ in 0..count {
        if random.next_float() > chance {
            continue;
        }
        if let Some(stack) = item(state, fortune, random)? {
            spawn_item(world, random, p, stack)?;
        }
    }
    Ok(())
}
/// Calls the original drop callback. The shared RNG belongs to World, not the
/// population provider. Item motion/hover and entity UUIDs are independent
/// constructor randomness and intentionally absent from these spawn requests.
pub fn drop_block(
    world: &mut impl DropWorld,
    random: &mut JavaRandom,
    p: IVec3,
    state: u16,
    chance: f32,
    fortune: i32,
) -> Result<()> {
    ensure!(
        (0..=i32::MAX / 2 - 1).contains(&fortune),
        "Unsupported invalid Fortune value {fortune}"
    );
    state_data(state)?;
    match state >> 4 {
        0 | 144 | 166 => {}
        26 => {
            if state & 8 == 0 {
                base_drop(world, random, p, state, chance, 0)?;
            }
        }
        36 => {
            if !world.is_remote() {
                if let Some(DropTile::Piston { state }) = world.tile_entity(p)? {
                    ensure!(state >> 4 != 36, "Recursive moving-piston drop state");
                    drop_block(world, random, p, state, 1., 0)?;
                }
            }
        }
        176 | 177 => {
            if let Some(DropTile::Banner {
                base_color,
                mut written_tag,
            }) = world.tile_entity(p)?
            {
                for name in ["x", "y", "z", "id"] {
                    written_tag.remove(name);
                }
                let mut stack = DropStack::new(425, base_color);
                stack.nbt = Some(DropNbt::Compound(BTreeMap::from([(
                    "BlockEntityTag".into(),
                    DropNbt::Compound(written_tag),
                )])));
                spawn_item(world, random, p, stack)?;
            } else {
                base_drop(world, random, p, state, chance, fortune)?;
            }
        }
        127 => {
            for _ in 0..if state & 12 >= 8 { 3 } else { 1 } {
                spawn_item(world, random, p, DropStack::new(351, 3))?;
            }
        }
        18 | 161 => {
            if !world.is_remote() {
                let mut chance = if state >> 4 == 18 && state & 3 == 3 {
                    40i32
                } else {
                    20
                };
                if fortune > 0 {
                    chance = chance
                        .wrapping_sub(2i32.wrapping_shl(fortune as u32))
                        .max(10);
                }
                if random.next_int(chance) == 0 {
                    if let Some(stack) = item(state, fortune, random)? {
                        spawn_item(world, random, p, stack)?;
                    }
                }
                chance = 200;
                if fortune > 0 {
                    chance = chance
                        .wrapping_sub(10i32.wrapping_shl(fortune as u32))
                        .max(40);
                }
                if (state >> 4 == 18 && state & 3 == 0 || state >> 4 == 161 && state & 3 == 1)
                    && random.next_int(chance) == 0
                {
                    spawn_item(world, random, p, DropStack::new(260, 0))?;
                }
            }
        }
        115 => {
            if !world.is_remote() {
                let mut count = 1;
                if state & 3 >= 3 {
                    count = 2 + random.next_int(3);
                    if fortune > 0 {
                        count += random.next_int(fortune + 1);
                    }
                }
                for _ in 0..count {
                    spawn_item(world, random, p, DropStack::new(372, 0))?;
                }
            }
        }
        97 => {
            if !world.is_remote() && world.do_tile_drops() {
                world.emit(DropEvent::Silverfish {
                    position: [p.x as f64 + 0.5, p.y as f64, p.z as f64 + 0.5],
                    yaw: 0.,
                })?;
                world.emit(DropEvent::EntityStatus { status: 20 })?;
            }
        }
        59 | 141 | 142 => {
            base_drop(world, random, p, state, chance, 0)?;
            if !world.is_remote() && state & 7 == 7 {
                let seed = match state >> 4 {
                    141 => 391,
                    142 => 392,
                    _ => 295,
                };
                for _ in 0..3 + fortune {
                    if random.next_int(15) <= 7 {
                        spawn_item(world, random, p, DropStack::new(seed, 0))?;
                    }
                }
                if state >> 4 == 142 && random.next_int(50) == 0 {
                    spawn_item(world, random, p, DropStack::new(394, 0))?;
                }
            }
        }
        104 | 105 => {
            base_drop(world, random, p, state, chance, fortune)?;
            if !world.is_remote() {
                for _ in 0..3 {
                    if random.next_int(15) <= i32::from(state & 7) {
                        spawn_item(
                            world,
                            random,
                            p,
                            DropStack::new(if state >> 4 == 104 { 361 } else { 362 }, 0),
                        )?;
                    }
                }
            }
        }
        52 => {
            base_drop(world, random, p, state, chance, fortune)?;
            let value = 15 + random.next_int(15) + random.next_int(15);
            experience(world, p, value)?;
        }
        16 | 21 | 56 | 129 | 153 | 73 | 74 => {
            base_drop(world, random, p, state, chance, fortune)?;
            let value = match state >> 4 {
                16 => random.next_int(3),
                56 | 129 => 3 + random.next_int(5),
                21 | 153 => 2 + random.next_int(4),
                _ => 1 + random.next_int(5),
            };
            experience(world, p, value)?;
        }
        84 => base_drop(world, random, p, state, chance, 0)?,
        _ => base_drop(world, random, p, state, chance, fortune)?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct World {
        remote: bool,
        tiles: bool,
        special: u8,
        events: Vec<DropEvent>,
    }
    impl DropWorld for World {
        fn is_remote(&self) -> bool {
            self.remote
        }
        fn do_tile_drops(&self) -> bool {
            self.tiles
        }
        fn tile_entity(&mut self, _: IVec3) -> Result<Option<DropTile>> {
            Ok(match self.special {
                1 => Some(DropTile::Banner {
                    base_color: 12,
                    written_tag: BTreeMap::from([
                        ("id".into(), DropNbt::String("Banner".into())),
                        ("x".into(), DropNbt::Int(-19)),
                        ("y".into(), DropNbt::Int(65)),
                        ("z".into(), DropNbt::Int(36)),
                        ("Base".into(), DropNbt::Int(12)),
                        (
                            "Patterns".into(),
                            DropNbt::List {
                                element: 10,
                                values: vec![DropNbt::Compound(BTreeMap::from([
                                    ("Pattern".into(), DropNbt::String("cre".into())),
                                    ("Color".into(), DropNbt::Int(6)),
                                    ("ExtraByte".into(), DropNbt::Byte(-3)),
                                    ("ExtraLong".into(), DropNbt::Long(189000000001)),
                                ]))],
                            },
                        ),
                    ]),
                }),
                2 => Some(DropTile::Piston {
                    state: (31 << 4) | 1,
                }),
                _ => None,
            })
        }
        fn emit(&mut self, event: DropEvent) -> Result<()> {
            self.events.push(event);
            Ok(())
        }
    }
    #[test]
    fn entire_registry_drops_and_rng_match_original_bytecode() {
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/vanilla189-drops.json"))
                .unwrap();
        for (index, row) in data["cases"].as_array().unwrap().iter().enumerate() {
            let mut world = World {
                remote: row["remote"].as_bool().unwrap(),
                tiles: row["tiles"].as_bool().unwrap(),
                special: row["special"].as_u64().unwrap() as u8,
                events: Vec::new(),
            };
            let mut random = JavaRandom::new(row["seed"].as_str().unwrap().parse().unwrap());
            let state = row["state"].as_u64().unwrap() as u16;
            drop_block(
                &mut world,
                &mut random,
                IVec3::new(-19, 65, 36),
                state,
                row["chance"].as_f64().unwrap() as f32,
                row["fortune"].as_i64().unwrap() as i32,
            )
            .unwrap();
            let actual: Vec<_> = world
                .events
                .iter()
                .map(|event| {
                    let mut value = serde_json::to_value(event).unwrap();
                    // Compare the typed tag contents to the original JVM
                    // record, independently of the checkpoint wire encoding.
                    if let DropEvent::Item { stack, .. } = event {
                        if let Some(tag) = &stack.nbt {
                            value["stack"]["nbt"] = serde_json::to_value(tag).unwrap();
                        }
                    }
                    let position = match event {
                        DropEvent::Item { position, .. }
                        | DropEvent::Experience { position, .. }
                        | DropEvent::Silverfish { position, .. } => Some(position),
                        _ => None,
                    };
                    if let Some(position) = position {
                        value.as_object_mut().unwrap().remove("position");
                        value["position_bits"] =
                            serde_json::json!(position.map(|v| v.to_bits().to_string()));
                    }
                    value
                })
                .collect();
            assert_eq!(
                serde_json::json!(actual),
                row["events"],
                "case{index} state{state} profile{row}"
            );
            assert_eq!(
                random.next_long().to_string(),
                row["tail"].as_str().unwrap(),
                "case{index} state{state} RNG profile{row}"
            );
        }
        assert_eq!(data["cases"].as_array().unwrap().len(), 49299);
    }
}
