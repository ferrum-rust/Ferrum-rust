//! Original block hardness, tool strength, harvesting and silk-touch outputs.
use super::{food, item_entity};
use crate::inventory::Stack;
use serde::Deserialize;
use std::{collections::HashMap, sync::OnceLock};
#[derive(Deserialize)]
struct Block(i32, bool, [String; 4], String);
#[derive(Deserialize)]
struct Data {
    blocks: Vec<Block>,
    strength: Vec<[i32; 3]>,
    silk: Vec<[u16; 3]>,
    item_stats: Vec<(u16, String, String)>,
}
struct Registry {
    blocks: Vec<Block>,
    strength: HashMap<(u16, u16), f32>,
    silk: HashMap<u16, (u16, u16)>,
    item_stats: HashMap<u16, (String, String)>,
}
fn registry() -> &'static Registry {
    static DATA: OnceLock<Registry> = OnceLock::new();
    DATA.get_or_init(|| {
        let d: Data = serde_json::from_str(include_str!("mining_registry.json"))
            .expect("Original mining registry");
        Registry {
            blocks: d.blocks,
            strength: d
                .strength
                .into_iter()
                .map(|[item, block, bits]| {
                    ((item as u16, block as u16), f32::from_bits(bits as u32))
                })
                .collect(),
            silk: d
                .silk
                .into_iter()
                .map(|[state, item, meta]| (state, (item, meta)))
                .collect(),
            item_stats: d
                .item_stats
                .into_iter()
                .map(|(id, use_id, break_id)| (id, (use_id, break_id)))
                .collect(),
        }
    })
}
fn block(state: u16) -> &'static Block {
    registry()
        .blocks
        .get((state >> 4) as usize)
        .unwrap_or(&registry().blocks[0])
}
pub fn hardness(state: u16) -> f32 {
    f32::from_bits(block(state).0 as u32)
}
pub fn strength(state: u16, held: Option<&Stack>) -> f32 {
    held.and_then(|s| registry().strength.get(&(s.id, state >> 4)))
        .copied()
        .unwrap_or(1.)
}
pub fn silk(state: u16) -> Option<Stack> {
    if !block(state).1 {
        return None;
    }
    let &(id, meta) = registry().silk.get(&state)?;
    item_entity::stack_from_metadata(id, 1, meta).ok()
}
pub fn can_silk(state: u16) -> bool {
    block(state).1
}
pub fn callbacks(state: u16) -> &'static [String; 4] {
    &block(state).2
}
pub fn statistic(state: u16) -> Option<&'static str> {
    let id = &block(state).3;
    (!id.is_empty()).then_some(id)
}
pub fn item_statistic(id: u16, broken: bool) -> Option<&'static str> {
    let pair = registry().item_stats.get(&id)?;
    let s = if broken { &pair.1 } else { &pair.0 };
    (!s.is_empty()).then_some(s)
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Conditions {
    pub grounded: bool,
    pub eyes_in_water: bool,
    pub aqua_affinity: bool,
    pub haste: Option<i32>,
    pub fatigue: Option<i32>,
}
pub fn increment(state: u16, held: Option<&Stack>, hardness: f32, c: Conditions) -> f32 {
    if hardness < 0. {
        return 0.;
    }
    let mut speed = strength(state, held);
    if speed > 1. {
        let efficiency = held.map_or(0, |s| s.enchantment_level(32));
        if efficiency > 0 {
            speed += efficiency.wrapping_mul(efficiency).wrapping_add(1) as f32;
        }
    }
    if let Some(amp) = c.haste {
        speed *= 1. + amp.wrapping_add(1) as f32 * 0.2;
    }
    if let Some(amp) = c.fatigue {
        speed *= match amp {
            0 => 0.3,
            1 => 0.09,
            2 => 0.0027,
            _ => 0.00081,
        };
    }
    if c.eyes_in_water && !c.aqua_affinity {
        speed /= 5.;
    }
    if !c.grounded {
        speed /= 5.;
    }
    speed
        / hardness
        / if food::can_harvest(state, held.map(|s| s.id)) {
            30.
        } else {
            100.
        }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    #[test]
    fn original_block_relative_hardness_and_tool_harvest_match_real_player_modifiers() {
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(
            &include_bytes!("../../tests/fixtures/vanilla189-mining-rates.json.gz")[..],
        )
        .read_to_end(&mut bytes)
        .unwrap();
        let rows: Vec<[i32; 10]> = serde_json::from_slice(&bytes).unwrap();
        use super::super::drops::DropNbt;
        for (index, [state, id, eff, haste, fatigue, water, ground, aqua, bits, can]) in
            rows.into_iter().enumerate()
        {
            let state = state as u16;
            let held = (id != 0).then(|| {
                let mut stack = Stack::new(id as u16, 1);
                if eff > 0 {
                    stack =
                        stack.with_nbt(DropNbt::Compound(std::collections::BTreeMap::from([(
                            "ench".into(),
                            DropNbt::List {
                                element: 10,
                                values: vec![DropNbt::Compound(std::collections::BTreeMap::from(
                                    [
                                        ("id".into(), DropNbt::Short(32)),
                                        ("lvl".into(), DropNbt::Short(eff as i16)),
                                    ],
                                ))],
                            },
                        )])));
                }
                stack
            });
            let c = Conditions {
                grounded: ground != 0,
                eyes_in_water: water != 0,
                aqua_affinity: aqua != 0,
                haste: (haste >= 0).then_some(haste),
                fatigue: (fatigue >= 0).then_some(fatigue),
            };
            assert_eq!(
                food::can_harvest(state, held.as_ref().map(|s| s.id)),
                can != 0,
                "harvest #{index}"
            );
            assert_eq!(
                increment(state, held.as_ref(), hardness(state), c).to_bits(),
                bits as u32,
                "hardness #{index} state={state} item={id} c={c:?}"
            );
        }
    }
}
