use super::*;
use crate::vanilla::item_entity;
use std::sync::Arc;
type Map = BTreeMap<String, Tag>;
fn taglist(values: impl IntoIterator<Item = Tag>, kind: u8) -> Tag {
    Tag::List {
        element: kind,
        values: values.into_iter().collect(),
    }
}
fn doubles(v: DVec3) -> Tag {
    taglist(v.to_array().map(Tag::Double), 6)
}
fn numeric(v: &Tag) -> Option<f64> {
    Some(match v {
        Tag::Byte(v) => *v as f64,
        Tag::Short(v) => *v as f64,
        Tag::Int(v) => *v as f64,
        Tag::Long(v) => *v as f64,
        Tag::Float(v) => *v as f64,
        Tag::Double(v) => *v,
        _ => return None,
    })
}
fn vector<const N: usize>(v: Option<&Tag>) -> Option<[f64; N]> {
    let Tag::List { values, .. } = v? else {
        return None;
    };
    if values.len() < N {
        return None;
    }
    let mut out = [0.; N];
    for (dest, t) in out.iter_mut().zip(values) {
        *dest = numeric(t)?;
        if !dest.is_finite() {
            return None;
        }
    }
    Some(out)
}
pub(super) fn snbt_text(v: &Tag) -> String {
    match v {
        Tag::Byte(n) => format!("{n}b"),
        Tag::Short(n) => format!("{n}s"),
        Tag::Int(n) => n.to_string(),
        Tag::Long(n) => format!("{n}L"),
        Tag::Float(n) => format!("{n:?}f"),
        Tag::Double(n) => format!("{n:?}d"),
        Tag::String(n) => format!("\"{}\"", n.replace('"', "\\\"")),
        Tag::Compound(m) => format!(
            "{{{}}}",
            m.iter()
                .map(|(k, v)| format!("{k}:{}", snbt_text(v)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Tag::List { values, .. } => format!(
            "[{}]",
            values
                .iter()
                .enumerate()
                .map(|(i, v)| format!("{i}:{}", snbt_text(v)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Tag::Ints(v) => format!(
            "[{}]",
            v.iter().map(|n| format!("{n},")).collect::<String>()
        ),
        Tag::Bytes(v) => format!("[{} bytes]", v.len()),
        Tag::Longs(v) => format!("{v:?}"),
    }
}
pub(crate) fn stack_tag(s: &Stack) -> Tag {
    let name = items::registry()
        .items
        .iter()
        .find(|(_, id)| **id == s.id)
        .map_or("minecraft:air", |(n, _)| n.as_str());
    let mut m = Map::from([
        ("id".into(), Tag::String(name.into())),
        ("Damage".into(), Tag::Short(items::meta(s) as i16)),
        ("Count".into(), Tag::Byte(s.count as i8)),
    ]);
    if let Some(n) = s.nbt.as_deref() {
        m.insert("tag".into(), n.clone());
    }
    Tag::Compound(m)
}
pub(crate) fn read_stack(t: &Tag) -> Option<Stack> {
    let m = item_nbt::compound(t)?;
    let id = match m.get("id")? {
        Tag::String(s) => items::item(s).ok()?,
        n => numeric(n)? as u16,
    };
    let count = item_nbt::number(m.get("Count")) as i8;
    if count <= 0 {
        return None;
    }
    let mut s = item_entity::stack_from_metadata(
        id,
        count as u8,
        (item_nbt::number(m.get("Damage")) as i16).max(0) as u16,
    )
    .ok()?;
    s.nbt = m
        .get("tag")
        .filter(|v| matches!(v, Tag::Compound(_)))
        .cloned()
        .map(Arc::new);
    Some(s)
}
fn inventory<'a>(slots: impl IntoIterator<Item = (i32, &'a Stack)>) -> Tag {
    taglist(
        slots.into_iter().map(|(i, s)| {
            let Tag::Compound(mut t) = stack_tag(s) else {
                unreachable!()
            };
            t.insert("Slot".into(), Tag::Byte(i as i8));
            Tag::Compound(t)
        }),
        10,
    )
}
fn effect_tags(e: &crate::vanilla::potions::Effects) -> Tag {
    taglist(
        e.ordered().map(|e| {
            Tag::Compound(Map::from([
                ("Id".into(), Tag::Byte(e.id as i8)),
                ("Amplifier".into(), Tag::Byte(e.amplifier as i8)),
                ("Duration".into(), Tag::Int(e.duration)),
                ("Ambient".into(), Tag::Byte(e.ambient as i8)),
                ("ShowParticles".into(), Tag::Byte(e.show_particles as i8)),
            ]))
        }),
        10,
    )
}
pub(super) fn entity_nbt(ctx: &Context<'_>, t: Target) -> Tag {
    if let Target::Block(p) = t { return ctx.world.tile(p).cloned().unwrap_or_else(|| Tag::Compound(Default::default())); }
    let mut m = Map::new();
    let [yaw, pitch] = movement::rotation(ctx, t);
    let pos = ctx.position(t);
    let mut motion = DVec3::ZERO;
    let mut grounded = false;
    match t {
        Target::Block(_) => unreachable!(),
        Target::Player => {
            let s = &ctx.sim;
            m.insert("Fire".into(), Tag::Short(s.fire_ticks as i16));
            motion = DVec3::from_array(ctx.player.precise_velocity());
            grounded = ctx.player.grounded;
            m.insert(
                "Inventory".into(),
                inventory(s.inventory.slots.iter().enumerate().filter_map(|(i, s)| {
                    s.as_ref()
                        .map(|s| (if i >= 36 { i + 64 } else { i } as i32, s))
                })),
            );
            m.insert(
                "EnderItems".into(),
                inventory(
                    s.command_entities
                        .ender
                        .iter()
                        .filter_map(|(&i, s)| s.as_ref().map(|s| (i - 200, s))),
                ),
            );
            m.extend([
                ("HealF".into(), Tag::Float(s.health)),
                ("Health".into(), Tag::Short(s.health.ceil() as i16)),
                ("AbsorptionAmount".into(), Tag::Float(s.absorption)),
                ("foodLevel".into(), Tag::Int(s.food)),
                ("foodSaturationLevel".into(), Tag::Float(s.saturation)),
                ("foodExhaustionLevel".into(), Tag::Float(s.exhaustion)),
                ("XpLevel".into(), Tag::Int(s.xp.level)),
                ("XpP".into(), Tag::Float(s.xp.progress)),
                ("XpTotal".into(), Tag::Int(s.xp.total)),
                ("Score".into(), Tag::Int(s.xp.score)),
                (
                    "playerGameType".into(),
                    Tag::Int(i32::from(
                        s.commands
                            .player_mode
                            .unwrap_or((s.mode == Mode::Creative) as u8),
                    )),
                ),
                (
                    "SelectedItemSlot".into(),
                    Tag::Int(s.inventory.selected as i32),
                ),
                ("Air".into(), Tag::Short(s.air as i16)),
            ]);
            if s.effects.ordered().next().is_some() {
                m.insert("ActiveEffects".into(), effect_tags(&s.effects));
            }
            if let Some(b) = s.bed_spawn {
                m.insert("SpawnX".into(), Tag::Int(b.point.x));
                m.insert("SpawnY".into(), Tag::Int(b.point.y));
                m.insert("SpawnZ".into(), Tag::Int(b.point.z));
                m.insert("SpawnForced".into(), Tag::Byte(b.forced as i8));
            }
        }
        Target::Mob(id) => {
            if let Some(mob) = ctx.sim.mobs.iter().find(|m| m.id == id) {
                let brain = ctx.sim.mob_brains.get(&id);
                m.insert("Fire".into(), Tag::Short(brain.map_or(0, |b| b.fire_ticks as i16)));
                grounded = brain.is_some_and(|b| b.grounded);
                motion = mob.velocity.as_dvec3();
                let d = ctx.sim.command_entities.mobs.get(&id);
                if let Some(d) = d {
                    m.extend(d.extra.clone());
                    if d.effects.ordered().next().is_some() {
                        m.insert("ActiveEffects".into(), effect_tags(&d.effects));
                    }
                }
                m.extend([
                    ("id".into(), Tag::String(mob.kind.name().into())),
                    ("HealF".into(), Tag::Float(mob.health)),
                    ("Health".into(), Tag::Short(mob.health.ceil() as i16)),
                    ("HurtTime".into(), Tag::Short(mob.hurt as i16)),
                    (
                        "AbsorptionAmount".into(),
                        Tag::Float(d.map_or(0., |d| d.absorption)),
                    ),
                    (
                        "PersistenceRequired".into(),
                        Tag::Byte(d.is_some_and(|d| d.persistent) as i8),
                    ),
                    (
                        "CustomName".into(),
                        Tag::String(d.map_or("", |d| &d.name).into()),
                    ),
                    (
                        "Invulnerable".into(),
                        Tag::Byte(d.is_some_and(|d| d.invulnerable) as i8),
                    ),
                ]);
                if d.is_some_and(|d| d.no_ai) {
                    m.insert("NoAI".into(), Tag::Byte(1));
                }
                if d.is_some_and(|d| d.silent) {
                    m.insert("Silent".into(), Tag::Byte(1));
                }
                m.insert(
                    "Equipment".into(),
                    taglist(
                        (99..=103).map(|i| {
                            d.and_then(|d| d.equipment.get(&i))
                                .and_then(Option::as_ref)
                                .map_or(Tag::Compound(Map::new()), stack_tag)
                        }),
                        10,
                    ),
                );
                if mob.kind.ageable() {
                    m.insert("Age".into(), Tag::Int(brain.map_or(0, |b| b.growing_age)));
                    if mob.kind != MobKind::Villager {
                        m.insert(
                            "InLove".into(),
                            Tag::Int(brain.map_or(0, |b| b.love.into())),
                        );
                    }
                }
                if let Some(trader) = ctx.sim.trades.get(&id) {
                    m.insert("Profession".into(), Tag::Int(trader.profession.into()));
                    m.insert("Career".into(), Tag::Int(trader.career.id));
                    m.insert("CareerLevel".into(), Tag::Int(trader.career.level));
                    m.insert("Riches".into(), Tag::Int(trader.career.wealth));
                    m.insert("Willing".into(), Tag::Byte(trader.career.willing as i8));
                    m.insert(
                        "Inventory".into(),
                        inventory(d.into_iter().flat_map(|d| d.equipment.iter()).filter_map(
                            |(i, s)| {
                                (*i >= 300)
                                    .then(|| s.as_ref().map(|s| (i - 300, s)))
                                    .flatten()
                            },
                        )),
                    );
                }
            }
        }
        Target::Drop(id) => {
            if let Some(d) = ctx.sim.drops.iter().find(|d| d.id == id) {
                motion = d.velocity;
                m.extend([
                    ("id".into(), Tag::String("Item".into())),
                    ("Age".into(), Tag::Short(d.age as i16)),
                    ("PickupDelay".into(), Tag::Short(d.pickup_delay as i16)),
                    ("Item".into(), stack_tag(&d.stack)),
                ]);
                if let Some(name)=&d.owner { m.insert("Owner".into(),Tag::String(name.clone())); }
                if let Some(name)=&d.thrower { m.insert("Thrower".into(),Tag::String(name.clone())); }
            }
        }
        Target::Orb(id) => {
            if let Some(o) = ctx.sim.xp_orbs.iter().find(|o| o.id == id) {
                motion = o.velocity;
                m.extend([
                    ("id".into(), Tag::String("XPOrb".into())),
                    ("Value".into(), Tag::Short(o.value as i16)),
                    ("Health".into(), Tag::Short(o.health as i16)),
                    ("Age".into(), Tag::Short(o.age as i16)),
                ]);
            }
        }
    }
    m.extend([
        ("Pos".into(), doubles(pos)),
        ("Motion".into(), doubles(motion)),
        (
            "Rotation".into(),
            taglist([Tag::Float(yaw), Tag::Float(pitch)], 5),
        ),
        ("OnGround".into(), Tag::Byte(grounded as i8)),
        ("Dimension".into(), Tag::Int(ctx.sim.dimension)),
    ]);
    Tag::Compound(m)
}
pub(super) fn drop_item(
    sim: &mut Simulation,
    s: Stack,
    pos: glam::Vec3,
    vel: glam::Vec3,
    age: i32,
    delay: i32,
) -> u64 {
    let d=sim.spawn_item_entity(s,pos.as_dvec3(),vel.as_dvec3(),delay);
    d.age=age;
    d.id
}
fn apply(ctx: &mut Context<'_>, t: Target, m: &Map) {
    let num = |k: &str| item_nbt::number(m.get(k));
    if let Some(pos) = vector::<3>(m.get("Pos")) {
        let rot = vector::<2>(m.get("Rotation")).unwrap_or([0.; 2]);
        movement::set(ctx, t, DVec3::from_array(pos), rot.map(|n| n as f32));
    }
    let motion = vector::<3>(m.get("Motion"))
        .map(|v| DVec3::from_array(v.map(|n| if n.abs() > 10. { 0. } else { n })));
    match t {
        Target::Mob(id) => {
            let Some(mob) = ctx.sim.mobs.iter_mut().find(|m| m.id == id) else {
                return;
            };
            let d = ctx.sim.command_entities.mobs.entry(id).or_default();
            if let Some(v) = motion {
                mob.velocity = v.as_vec3()
            }
            if let Some(v) = vector::<2>(m.get("Rotation")) {
                mob.yaw = (180. + v[0] as f32).to_radians();
                d.pitch = v[1] as f32;
            }
            for key in [
                "CustomNameVisible",
                "CanPickUpLoot",
                "Color",
                "Sheared",
                "Saddle",
                "ExplosionRadius",
                "Fuse",
                "ignited",
                "powered",
                "SkeletonType",
                "IsBaby",
                "IsVillager",
                "ConversionTime", "Size", "RabbitType", "CatType", "Type", "Variant", "Elder",
                "Owner", "OwnerUUID", "Tame", "Angry", "Sitting", "CollarColor", "Anger",
                "Attributes", "ArmorItem", "SaddleItem", "ChestedHorse", "Bred", "EatingHaystack",
                "Reproduced", "Temper", "carried", "carriedData", "BatFlags", "Invul",
                "PlayerCreated", "Lifetime", "PersistenceRequired", "Pumpkin", "Items",
            ] {
                if let Some(v) = m.get(key) {
                    d.extra.insert(key.into(), v.clone());
                }
            }
            if let Some(h) = m.get("HealF").or_else(|| m.get("Health")).and_then(numeric) {
                mob.health = (h as f32).clamp(0., mob.kind.max_health(Some(d)));
            } else {
                mob.health = mob.kind.max_health(Some(d));
            }
            let brain = ctx.sim.mob_brains.entry(id).or_default();
            brain.fire_ticks = num("Fire") as i16 as i32;
            brain.growing_age = num("Age");
            brain.love = num("InLove").max(0) as u16;
            brain.grounded = num("OnGround") != 0;
            if m.contains_key("EggLayTime") {
                brain.egg_timer = num("EggLayTime").clamp(0, 65535) as u16
            }
            d.no_ai = num("NoAI") != 0;
            d.persistent = num("PersistenceRequired") != 0;
            d.invulnerable = num("Invulnerable") != 0;
            d.silent = num("Silent") != 0;
            if let Some(Tag::String(name)) = m.get("CustomName") {
                d.name = name.clone()
            }
            d.absorption = m.get("AbsorptionAmount").and_then(numeric).unwrap_or(0.) as f32;
            if let Some(Tag::List { values, .. }) = m.get("Equipment") {
                for (i, s) in values.iter().take(5).enumerate() {
                    d.equipment.insert(i as i32 + 99, read_stack(s));
                }
            }
            if let Some(Tag::List { values, .. }) = m.get("ActiveEffects") {
                d.effects = Default::default();
                for v in values.iter().filter_map(item_nbt::compound) {
                    let mut e = Effect::new(
                        item_nbt::number(v.get("Id")) as u8,
                        item_nbt::number(v.get("Duration")),
                        item_nbt::number(v.get("Amplifier")) as i8 as i32,
                    );
                    e.ambient = item_nbt::number(v.get("Ambient")) != 0;
                    e.show_particles = v.get("ShowParticles").is_none()
                        || item_nbt::number(v.get("ShowParticles")) != 0;
                    let _ = d.effects.load_effect(e);
                }
            }
            if mob.kind == MobKind::Villager {
                let profession = num("Profession").rem_euclid(5) as u8;
                let trader = ctx
                    .sim
                    .trades
                    .entry(id)
                    .or_insert_with(|| crate::villagers::Trader::spawned(profession));
                trader.profession = profession;
                trader.career.id = num("Career");
                trader.career.level = num("CareerLevel");
                trader.career.wealth = num("Riches");
                trader.career.willing = num("Willing") != 0;
                trader.custom_name = (!d.name.is_empty()).then(|| d.name.clone());
                if let Some(Tag::List { values, .. }) = m.get("Inventory") {
                    d.equipment.retain(|&slot, _| slot < 300);
                    for (i, s) in values.iter().take(8).enumerate() {
                        d.equipment.insert(300 + i as i32, read_stack(s));
                    }
                }
            }

        }
        Target::Drop(id) => {
            if let Some(d) = ctx.sim.drops.iter_mut().find(|d| d.id == id) {
                if let Some(v) = motion {
                    d.velocity = v
                }
                d.age = i32::from(num("Age") as i16);
                d.pickup_delay = i32::from(num("PickupDelay") as i16);
                let name = |k| match m.get(k) {Some(Tag::String(s)) if !s.is_empty()=>Some(s.clone()), _=>None};
                d.owner=name("Owner"); d.thrower=name("Thrower");
                if let Some(item) = m.get("Item").and_then(read_stack) {
                    d.stack = item
                } else {
                    d.age = 6000
                }
            }
        }
        Target::Orb(id) => {
            if let Some(o) = ctx.sim.xp_orbs.iter_mut().find(|o| o.id == id) {
                if let Some(v) = motion {
                    o.velocity = v
                }
                o.age = num("Age");
                if m.contains_key("Health"){o.health = num("Health");}
                o.value = num("Value");
            }
        }
        Target::Player | Target::Block(_) => (),
    }
}
pub(super) fn edit(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    if a.len() < 2 {
        return Err(usage("entitydata"));
    }
    let t = ctx.one(a[0])?;
    if t == Target::Player {
        return Err(error(
            "commands.entitydata.noPlayers",
            vec![json!(ctx.target_name(t))],
        ));
    }
    let original = entity_nbt(ctx, t);
    let mut merged = original.clone();
    let mut patch = tag_at(a, 1, "entitydata")?.unwrap();
    if let Tag::Compound(m) = &mut patch {
        m.remove("UUIDMost");
        m.remove("UUIDLeast");
    }
    snbt::merge(&mut merged, patch);
    let shown = json!(snbt_text(&merged));
    if original == merged {
        return Err(error("commands.entitydata.failed", vec![shown]));
    }
    apply(ctx, t, item_nbt::compound(&merged).unwrap());
    notify(ctx, "commands.entitydata.success", vec![shown])
}
pub(super) fn summon(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    let Some(&id) = a.first() else {
        return Err(usage("summon"));
    };
    let p = if a.len() >= 4 {
        position(ctx.origin, &a[1..4], true)?
    } else {
        ctx.origin
    };
    let b = p.floor().as_ivec3();
    if !(0..256).contains(&b.y)
        || !ctx
            .world
            .chunks
            .contains_key(&(b.x.div_euclid(16), b.z.div_euclid(16)))
    {
        return Err(error("commands.summon.outOfWorld", vec![]));
    }
    let tag = tag_at(a, 4, "summon")?.unwrap_or_else(|| Tag::Compound(Map::new()));
    let m = item_nbt::compound(&tag).unwrap();
    let t = match id {
        "Item" => {
            let s = m.get("Item").and_then(read_stack);
            if let Some(s) = s {
                Target::Drop(drop_item(ctx.sim, s, p.as_vec3(), glam::Vec3::ZERO, 0, 0))
            } else {
                return notify(ctx, "commands.summon.success", vec![]);
            }
        }
        "XPOrb" => {
            ctx.sim.spawn_xp(p, item_nbt::number(m.get("Value")));
            Target::Orb(ctx.sim.next_id as i32)
        }
        _ => {
            let kind = MobKind::from_name(id).ok_or_else(|| error("commands.summon.failed", vec![]))?;
            let id = if a.len() > 4 {
                ctx.sim.spawn_registered_raw(kind, p.as_vec3(), std::f32::consts::PI)
            } else {
                ctx.sim.spawn_registered(kind, p.as_vec3(), std::f32::consts::PI)
            };
            Target::Mob(id)
        }
    };
    if a.len() > 4 || !matches!(t, Target::Mob(_)) { apply(ctx, t, m); }
    movement::set(ctx, t, p, movement::rotation(ctx, t));
    notify(ctx, "commands.summon.success", vec![])
}
