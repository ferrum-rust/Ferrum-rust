//! Inventory mutations from the 1.8.9 integrated server command implementations.
use super::*;
use crate::vanilla::{item_entity, item_nbt, random::JavaRandom};
use std::sync::Arc;
use crate::vanilla::item_registry::Enchantment;
pub(super) use crate::vanilla::item_registry::registry;
pub(super) fn resource(raw: &str) -> String {
    if raw.contains(':') {
        raw.into()
    } else {
        format!("minecraft:{raw}")
    }
}
pub(super) fn item(raw: &str) -> CmdResult<u16> {
    registry()
        .items
        .get(&resource(raw))
        .copied()
        .ok_or_else(|| error("commands.give.item.notFound", vec![json!(raw)]))
}
pub(super) fn meta(item: &Stack) -> u16 {
    if item_entity::facts(item.id).is_some_and(|p| p.max_damage > 0) {
        item.damage
    } else {
        item.meta
    }
}
pub(super) fn stack(args: &[&str], key: &str, air: bool) -> CmdResult<Option<Stack>> {
    let id = if air && resource(args[0]) == "minecraft:air" {
        0
    } else {
        item(args[0])?
    };
    let count = optional_num(args, 1, 1, 1, 64)? as u8;
    let damage = optional_num(args, 2, 0, i32::MIN, i32::MAX)?.max(0) as u16;
    let tag = tag_at(args, 3, key)?;
    if id == 0 {
        return Ok(None);
    }
    let mut s = item_entity::stack_from_metadata(id, count, damage)
        .map_err(|_| error("commands.give.item.notFound", vec![json!(args[0])]))?;
    s.nbt = tag.map(Arc::new);
    Ok(Some(s))
}
fn display(ctx: &Context<'_>, s: &Stack) -> Value {
    let custom = s
        .nbt
        .as_deref()
        .and_then(item_nbt::compound)
        .and_then(|c| c.get("display"))
        .and_then(item_nbt::compound)
        .and_then(|c| c.get("Name"));
    let label = if let Some(Tag::String(n)) = custom {
        n.clone()
    } else {
        ctx.resources.item_name(
            ctx.options.language.as_deref().unwrap_or("en_US"),
            s.id,
            meta(s),
        )
    };
    json!({"text":format!("[{label}]"),"italic":custom.is_some(),"hoverEvent":{"action":"show_item","value":{"text":data::snbt_text(&data::stack_tag(s))}}})
}
pub(super) fn give(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    if a.len() < 2 {
        return Err(usage("give"));
    }
    player_target(ctx, Some(a[0]))?;
    let mut s = stack(&a[1..], "give", false)?.unwrap();
    let count = s.count;
    let shown = display(ctx, &s);
    let added = item_entity::add_to_player(
        &mut ctx.sim.inventory,
        &mut s,
        ctx.sim.mode == Mode::Creative,
    )
    .map_err(|_| error("commands.clear.failure", vec![json!(ctx.options.username)]))?;
    let pos = ctx.position(Target::Player);
    let mut rng = JavaRandom::from_state(ctx.sim.player_random_state);
    if added {
        ctx.sounds.push((
            "random.pop".into(),
            pos,
            0.2,
            ((rng.next_float() - rng.next_float()) * 0.7 + 1.) * 2.,
        ));
    }
    ctx.results[3] = i32::from(count - s.count);
    let fake = added && s.count == 0;
    if fake {
        s.count = 1
    }
    let velocity = ctx.player.direction() * 0.3 + glam::Vec3::Y * 0.1;
    let angle = rng.next_float() * std::f32::consts::TAU;
    let magnitude = 0.02 * rng.next_float();
    let velocity = velocity
        + glam::Vec3::new(
            angle.cos() * magnitude,
            (rng.next_float() - rng.next_float()) * 0.1,
            angle.sin() * magnitude,
        );
    ctx.sim.player_random_state = rng.state();
    let p = ctx.player.eye(1.) - glam::Vec3::Y * 0.3;
    data::drop_item(
        ctx.sim,
        s,
        p,
        velocity,
        if fake { 5999 } else { 0 },
        if fake { 32767 } else { 0 },
    );
    notify(
        ctx,
        "commands.give.success",
        vec![shown, json!(count), json!(ctx.options.username)],
    )
}
fn strict_lists(pattern:&Tag,value:&Tag)->bool{
    match (pattern,value){
        (Tag::Compound(a),Tag::Compound(b))=>a.iter().all(|(k,v)|b.get(k).is_some_and(|other|strict_lists(v,other))),
        _=>pattern==value,
    }
}
pub(super) fn clear(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    player_target(ctx, a.first().copied())?;
    let id = a.get(1).map(|s| item(s)).transpose()?;
    let metadata = optional_num(a, 2, -1, -1, i32::MAX)?;
    let limit = optional_num(a, 3, -1, -1, i32::MAX)?;
    let filter = tag_at(a, 4, "clear")?;
    let mut total = 0;
    let inv = &mut ctx.sim.inventory;
    for (i,slot) in inv.slots.iter_mut().chain(std::iter::once(&mut inv.cursor)).enumerate() {
        let Some(s) = slot else { continue };
        if id.is_some_and(|id| id != s.id)
            || metadata >= 0 && i32::from(meta(s)) != metadata
            || filter
                .as_ref()
                .is_some_and(|f| !s.nbt.as_deref().is_some_and(|n| if i<36{snbt::matches(f,n)}else{strict_lists(f,n)}))
        {
            continue;
        }
        let take = if limit <= 0 {
            i32::from(s.count)
        } else {
            (limit - total).min(i32::from(s.count))
        };
        total += take;
        if limit != 0 {
            s.count -= take as u8;
            if s.count == 0 {
                *slot = None
            }
        }
        if limit > 0 && total == limit {
            break;
        }
    }
    ctx.results[3] = total;
    if total == 0 {
        return Err(error(
            "commands.clear.failure",
            vec![json!(ctx.options.username)],
        ));
    }
    let args = vec![json!(ctx.options.username), json!(total)];
    if limit == 0 {
        Ok(vec![tr("commands.clear.testing", args)])
    } else {
        notify(ctx, "commands.clear.success", args)
    }
}
pub(super) fn enchant(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    if a.len() < 2 {
        return Err(usage("enchant"));
    }
    player_target(ctx, Some(a[0]))?;
    ctx.results[3] = 0;
    let registry = &registry().enchantments;
    let id = integer(a[1], 0, i32::MAX)
        .or_else(|e| registry.get(&resource(a[1])).map(|e| e.0).ok_or(e))?;
    let held = ctx
        .sim
        .inventory
        .held()
        .ok_or_else(|| error("commands.enchant.noItem", vec![]))?;
    let e = registry
        .values()
        .find(|e| e.0 == id)
        .ok_or_else(|| error("commands.enchant.notFound", vec![json!(id)]))?;
    if !e.4.contains(&held.id) {
        return Err(error("commands.enchant.cantEnchant", vec![]));
    }
    let level = optional_num(a, 2, 1, e.2, e.3)?;
    for [old, old_lvl] in held.enchantments() {
        if e.5.contains(&old) {
            let old = registry.values().find(|x| x.0 == i32::from(old)).unwrap();
            let full = |e: &Enchantment, lvl: i32| json!({"text":"","extra":[tr(&e.1,vec![]),{"text":" "},tr(&format!("enchantment.level.{lvl}"),vec![])]});
            return Err(error(
                "commands.enchant.cantCombine",
                vec![full(e, level), full(old, i32::from(old_lvl))],
            ));
        }
    }
    let mut tags = held
        .nbt
        .as_deref()
        .and_then(item_nbt::compound)
        .cloned()
        .unwrap_or_default();
    let mut ench = match tags.remove("ench") {
        Some(Tag::List {
            element: 10,
            values,
        }) => values,
        _ => Vec::new(),
    };
    ench.push(Tag::Compound(BTreeMap::from([
        ("id".into(), Tag::Short(id as i16)),
        ("lvl".into(), Tag::Short(level as i16)),
    ])));
    tags.insert(
        "ench".into(),
        Tag::List {
            element: 10,
            values: ench,
        },
    );
    ctx.sim.inventory.slots[ctx.sim.inventory.selected]
        .as_mut()
        .unwrap()
        .nbt = Some(Arc::new(Tag::Compound(tags)));
    ctx.results[3] = 1;
    notify(ctx, "commands.enchant.success", vec![])
}
pub(super) fn slots() -> BTreeMap<String, i32> {
    let mut s = BTreeMap::from([
        ("slot.weapon".into(), 99),
        ("slot.armor.head".into(), 103),
        ("slot.armor.chest".into(), 102),
        ("slot.armor.legs".into(), 101),
        ("slot.armor.feet".into(), 100),
        ("slot.horse.saddle".into(), 400),
        ("slot.horse.armor".into(), 401),
        ("slot.horse.chest".into(), 499),
    ]);
    for (prefix, n, start) in [
        ("container", 54, 0),
        ("hotbar", 9, 0),
        ("inventory", 27, 9),
        ("enderchest", 27, 200),
        ("villager", 8, 300),
        ("horse", 15, 500),
    ] {
        for i in 0..n {
            s.insert(format!("slot.{prefix}.{i}"), start + i);
        }
    }
    s
}
pub(super) fn replace(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    let (idx, kind) = match a.first() {
        Some(&"entity") => (2, "entity"),
        Some(&"block") => (4, "block"),
        _ => return Err(usage("replaceitem")),
    };
    if a.len() < idx + 2 {
        return Err(usage(&format!("commands.replaceitem.{kind}.usage")));
    }
    let slot = *slots()
        .get(a[idx])
        .ok_or_else(|| error("commands.generic.parameter.invalid", vec![json!(a[idx])]))?;
    let s = stack(&a[idx + 1..], "replaceitem", true)?;
    let n = s
        .as_ref()
        .map_or(optional_num(a, idx + 2, 1, 1, 64)? as u8, |s| s.count);
    let shown = s.as_ref().map_or(json!("Air"), |s| display(ctx, s));
    ctx.results[3] = 0;
    if kind == "block" {
        let p = block_position(ctx,&a[1..4],false)?;
        let key = crate::gameplay::container_key(p);
        let type_id = ctx.world.get(p) >> 4;
        let c = crate::container::Container::for_block(type_id)
            .map(|c| ctx.sim.containers.entry(key).or_insert(c))
            .ok_or_else(|| {
                error(
                    "commands.replaceitem.noContainer",
                    vec![json!(p.x), json!(p.y), json!(p.z)],
                )
            })?;
        if let Some(target) = c.slots.get_mut(slot as usize) {
            *target = s;
        }
    } else {
        let target = ctx.one(a[1])?;
        let armor = |slot: i32, s: &Option<Stack>| {
            s.as_ref().is_none_or(|s| {
                crate::inventory::accepts_armor((slot - 100 + 36) as usize, s)
                    || slot == 103 && (s.id < 256 || s.id == 397)
            })
        };
        let success = match target {
            Target::Player => match slot {
                0..=35 => {
                    ctx.sim.inventory.slots[slot as usize] = s;
                    true
                }
                100..=103 if armor(slot, &s) => {
                    ctx.sim.inventory.slots[(slot - 64) as usize] = s;
                    true
                }
                200..=226 => {
                    ctx.sim.command_entities.ender.insert(slot, s);
                    true
                }
                _ => false,
            },
            Target::Mob(id) => {
                let villager = ctx
                    .sim
                    .mobs
                    .iter()
                    .any(|m| m.id == id && m.kind == MobKind::Villager);
                let valid = match slot {
                    99 => s
                        .as_ref()
                        .is_none_or(|s| !(36..40).any(|i| crate::inventory::accepts_armor(i, s))),
                    100..=103 => armor(slot, &s),
                    300..=307 => villager,
                    _ => false,
                };
                if valid {
                    ctx.sim
                        .command_entities
                        .mobs
                        .entry(id)
                        .or_default()
                        .equipment
                        .insert(slot, s);
                }
                valid
            }
            _ => false,
        };
        if !success {
            return Err(error(
                "commands.replaceitem.failed",
                vec![json!(slot), json!(n), shown],
            ));
        }
    }
    ctx.results[3] = i32::from(n);
    notify(
        ctx,
        "commands.replaceitem.success",
        vec![json!(slot), json!(n), shown],
    )
}
