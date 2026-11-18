//! ContainerRepair calculations. The server remains authoritative for remote items and experience.
use super::{drops::DropNbt as N, item_nbt, item_registry, loot};
use crate::inventory::Stack;
use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock},
};

static MATERIALS: LazyLock<BTreeMap<u16, Vec<u16>>> =
    LazyLock::new(|| serde_json::from_str(include_str!("anvil_materials189.json")).unwrap());

pub(crate) struct Plan {
    pub cost: i32,
    pub materials: u8,
    pub output: Option<Stack>,
}
impl Plan {
    fn empty() -> Self {
        Self {
            cost: 0,
            materials: 0,
            output: None,
        }
    }
}

fn tags(s: &Stack) -> Option<&BTreeMap<String, N>> {
    s.nbt.as_deref().and_then(item_nbt::compound)
}
pub(crate) fn custom_name(s: &Stack) -> Option<&str> {
    match tags(s)?
        .get("display")
        .and_then(item_nbt::compound)?
        .get("Name")?
    {
        N::String(v) => Some(v),
        _ => None,
    }
}
fn prior(s: &Stack) -> i32 {
    match tags(s).and_then(|t| t.get("RepairCost")) {
        Some(N::Int(cost)) => *cost,
        _ => 0,
    }
}
fn levels(s: &Stack) -> Vec<(i16, i16)> {
    let key = if s.id == 403 {
        "StoredEnchantments"
    } else {
        "ench"
    };
    let Some(N::List {
        element: 10,
        values,
    }) = tags(s).and_then(|t| t.get(key))
    else {
        return vec![];
    };
    let mut result: Vec<(i16, i16)> = Vec::new();
    for attr in values.iter().filter_map(item_nbt::compound) {
        let pair = (
            item_nbt::number(attr.get("id")) as i16,
            item_nbt::number(attr.get("lvl")) as i16,
        );
        if let Some(old) = result.iter_mut().find(|e| e.0 == pair.0) {
            *old = pair
        } else {
            result.push(pair)
        }
    }
    result
}
fn edit_enchants(
    left: &Stack,
    right: &Stack,
    creative: bool,
    book: bool,
    levels: &mut Vec<(i16, i16)>,
) -> i32 {
    let mut cost = 0;
    for (id, new_level) in self::levels(right) {
        let Some(e) = item_registry::registry()
            .enchantments
            .values()
            .find(|e| e.0 == i32::from(id))
        else {
            continue;
        };
        let existing = levels.iter().find(|v| v.0 == id).map_or(0, |v| v.1);
        let level = if existing == new_level {
            i32::from(new_level) + 1
        } else {
            i32::from(new_level.max(existing))
        };
        let mut applicable = creative || left.id == 403 || e.4.contains(&left.id);
        // The original adds one cost per conflicting enchantment even when it rejects the candidate.
        let conflict = levels
            .iter()
            .filter(|(old, _)| {
                *old != id
                    && item_registry::registry()
                        .enchantments
                        .values()
                        .any(|other| other.0 == i32::from(*old) && other.5.contains(&id))
            })
            .count() as i32;
        if conflict != 0 {
            cost += conflict;
            applicable = false;
        }
        if !applicable {
            continue;
        }
        let level = level.min(e.3) as i16;
        if let Some(old) = levels.iter_mut().find(|v| v.0 == id) {
            old.1 = level
        } else {
            levels.push((id, level))
        }
        let multiplier: i32 = match loot::enchantment_weight(id) {
            Some(10) => 1,
            Some(5) => 2,
            Some(2) => 4,
            Some(1) => 8,
            _ => 0,
        };
        cost += (if book {
            (multiplier / 2).max(1)
        } else {
            multiplier
        }) * i32::from(level);
    }
    cost
}
fn put_levels(tags: &mut BTreeMap<String, N>, id: u16, levels: &[(i16, i16)]) {
    let mut values = vec![];
    // Enchanted books keep existing StoredEnchantments tags, updating only stronger levels.
    if id == 403 {
        if let Some(N::List {
            element: 10,
            values: old,
        }) = tags.get("StoredEnchantments")
        {
            values = old.clone();
        }
    }
    for &(id, lvl) in levels {
        if loot::enchantment_weight(id).is_none() {
            continue;
        }
        let entry = N::Compound(BTreeMap::from([
            ("id".into(), N::Short(id)),
            ("lvl".into(), N::Short(lvl)),
        ]));
        if let Some(N::Compound(e)) = values.iter_mut().find(|v| {
            item_nbt::compound(v).is_some_and(|e| item_nbt::number(e.get("id")) as i16 == id)
        }) {
            if (item_nbt::number(e.get("lvl")) as i16) < lvl {
                e.insert("lvl".into(), N::Short(lvl));
            }
        } else {
            values.push(entry)
        }
    }
    if values.is_empty() {
        tags.remove("ench");
    } else {
        tags.insert(
            if id == 403 {
                "StoredEnchantments"
            } else {
                "ench"
            }
            .into(),
            N::List {
                element: 10,
                values,
            },
        );
    }
}
pub(crate) fn calculate(
    left: Option<&Stack>,
    right: Option<&Stack>,
    name: &str,
    creative: bool,
) -> Plan {
    let Some(left) = left else {
        return Plan::empty();
    };
    let mut output = left.copied();
    let mut levels = levels(left);
    let mut total = 0;
    let mut materials = 0;
    if let Some(right) = right {
        let max = super::armor::maximum_damage(left.id);
        let damageable = max > 0 && !left.unbreakable();
        let book = right.id == 403 && !self::levels(right).is_empty();
        if damageable
            && MATERIALS
                .get(&left.id)
                .is_some_and(|all| all.contains(&right.id))
        {
            let quarter = max / 4;
            if quarter == 0 || left.damage == 0 {
                return Plan::empty();
            }
            let mut damage = i32::from(left.damage);
            while damage > 0 && materials < right.count {
                damage -= quarter.min(damage);
                materials += 1;
            }
            total += i32::from(materials);
            output.damage = damage as u16;
        } else {
            if !book && !(left.id == right.id && damageable) {
                return Plan::empty();
            }
            if damageable && !book {
                let repaired = (i32::from(left.damage) + i32::from(right.damage)
                    - max
                    - max * 12 / 100)
                    .max(0) as u16;
                if repaired < output.damage {
                    output.damage = repaired;
                    total += 2;
                }
            }
            total += edit_enchants(left, right, creative, book, &mut levels);
        }
    }
    let mut nbt = tags(left).cloned().unwrap_or_default();
    let blank = name.trim().is_empty();
    let renamed = if blank {
        custom_name(left).is_some()
    } else {
        custom_name(left) != Some(name)
    };
    if renamed {
        total += 1;
        if blank {
            if let Some(N::Compound(d)) = nbt.get_mut("display") {
                d.remove("Name");
                if d.is_empty() {
                    nbt.remove("display");
                }
            }
        } else {
            let mut display = nbt
                .remove("display")
                .as_ref()
                .and_then(item_nbt::compound)
                .cloned()
                .unwrap_or_default();
            display.insert("Name".into(), N::String(name.into()));
            nbt.insert("display".into(), N::Compound(display));
        }
    }
    let left_prior = prior(left);
    let right_prior = right.map_or(0, prior);
    let mut cost = left_prior.wrapping_add(right_prior).wrapping_add(total);
    if renamed && total == 1 && cost >= 40 {
        cost = 39;
    }
    if total <= 0 || cost >= 40 && !creative {
        return Plan {
            cost,
            materials,
            output: None,
        };
    }
    let prior = if right.is_some() {
        left_prior.max(right_prior)
    } else {
        left_prior
    };
    nbt.insert(
        "RepairCost".into(),
        N::Int(prior.wrapping_mul(2).wrapping_add(1)),
    );
    put_levels(&mut nbt, left.id, &levels);
    output.nbt = Some(Arc::new(N::Compound(nbt)));
    Plan {
        cost,
        materials,
        output: Some(output),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    type Item = (u16, u8, u16, i32, Option<String>, Vec<(i16, i16)>);
    fn input(v: &Item) -> Stack {
        let mut s = Stack::new(v.0, v.1);
        s.damage = v.2;
        let mut n = BTreeMap::from([
            ("RepairCost".into(), N::Int(v.3)),
            ("kept".into(), N::Byte(42)),
        ]);
        if let Some(name) = &v.4 {
            n.insert(
                "display".into(),
                N::Compound(BTreeMap::from([("Name".into(), N::String(name.clone()))])),
            );
        }
        put_levels(&mut n, s.id, &v.5);
        s.nbt = Some(Arc::new(N::Compound(n)));
        s
    }
    #[test]
    fn original_anvil_two_input_cost_material_and_output() {
        type Row = (Item, Option<Item>, String, bool, i32, u8, Option<Item>);
        let cases: Vec<Row> = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-anvil-repairs.json"
        ))
        .unwrap();
        for (i, (left, right, name, creative, cost, mats, output)) in cases.iter().enumerate() {
            let result = calculate(
                Some(&input(left)),
                right.as_ref().map(input).as_ref(),
                name,
                *creative,
            );
            assert_eq!((result.cost, result.materials), (*cost, *mats), "case {i}");
            assert_eq!(result.output, output.as_ref().map(input), "case {i}");
        }
    }
}
