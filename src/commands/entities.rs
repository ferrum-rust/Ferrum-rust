//! Local integrated-server player and entity commands, protocol version 47.
use super::*;
use crate::{
    gameplay::{command_entity_state::EffectEdit, MobKind, Mode, Simulation},
    inventory::Stack,
    vanilla::{drops::DropNbt as Tag, item_food::Effect, item_nbt},
};
use glam::{DVec3, IVec3};
use serde_json::{json, Value};
use std::collections::BTreeMap;
#[path = "entity_chat.rs"]
mod chat;
#[path = "entity_data.rs"]
mod data;
pub(crate) use data::{read_stack, stack_tag};
#[path = "entity_items.rs"]
mod items;
#[path = "entity_movement.rs"]
mod movement;
pub(super) fn entity_nbt(ctx: &Context<'_>, t: Target) -> Tag {
    data::entity_nbt(ctx, t)
}
fn optional_num(args: &[&str], i: usize, default: i32, min: i32, max: i32) -> CmdResult<i32> {
    args.get(i).map_or(Ok(default), |s| integer(s, min, max))
}
fn tag_at(args: &[&str], i: usize, cmd: &str) -> CmdResult<Option<Tag>> {
    if i >= args.len() {
        return Ok(None);
    }
    snbt::parse(&args[i..].join(" ")).map(Some).map_err(|e| {
        error(
            &format!("commands.{cmd}.tagError"),
            e["with"].as_array().cloned().unwrap_or_default(),
        )
    })
}
fn notify(ctx: &Context<'_>, key: &str, args: Vec<Value>) -> Reply {
    Ok(if ctx.permission.is_some() || ctx.sim.send_command_feedback {
        vec![tr(key, args)]
    } else {
        vec![]
    })
}
fn position(origin: DVec3, args: &[&str], center: bool) -> CmdResult<DVec3> {
    Ok(DVec3::new(
        coordinate(origin.x, args[0], center)?,
        coordinate(origin.y, args[1], false)?,
        coordinate(origin.z, args[2], center)?,
    ))
}
fn bounded(v:f64,min:f64,max:f64)->CmdResult<f64>{
    if v<min{Err(error("commands.generic.double.tooSmall",vec![json!(v),json!(min)]))}
    else if v>max{Err(error("commands.generic.double.tooBig",vec![json!(v),json!(max)]))}
    else{Ok(v)}
}
fn block_position(ctx:&Context<'_>,args:&[&str],center:bool)->CmdResult<IVec3>{
    let p=position(ctx.origin.floor(),args,center)?;
    bounded(p.x,-30000000.,30000000.)?;bounded(p.y,0.,256.)?;bounded(p.z,-30000000.,30000000.)?;
    Ok(p.floor().as_ivec3())
}
fn player_target(ctx: &Context<'_>, arg: Option<&str>) -> CmdResult<Target> {
    let target = arg.map_or(Ok(ctx.sender), |s| ctx.one(s));
    if matches!(target, Ok(Target::Player)) {
        Ok(Target::Player)
    } else {
        Err(error(
            if arg.is_some() {
                "commands.generic.player.notFound"
            } else {
                "You must specify which player you wish to perform this action on."
            },
            vec![],
        ))
    }
}
fn xp(ctx: &mut Context<'_>, args: &[&str]) -> Reply {
    let Some(&arg) = args.first() else {
        return Err(usage("xp"));
    };
    let stripped = arg.strip_suffix(['l', 'L']).filter(|s| !s.is_empty());
    let n = integer(stripped.unwrap_or(arg), i32::MIN, i32::MAX)?;
    player_target(ctx, args.get(1).copied())?;
    let ticks = ctx.sim.total_ticks as i32;
    let pos = ctx.position(Target::Player);
    let xp = &mut ctx.sim.xp;
    let key = if stripped.is_some() {
        ctx.results[4] = xp.level;
        xp.level = xp.level.wrapping_add(n);
        if xp.level < 0 {
            xp.level = 0;
            xp.progress = 0.;
            xp.total = 0
        }
        if n > 0 && xp.level % 5 == 0 && xp.last_sound < ticks.saturating_sub(100) {
            ctx.sounds.push((
                "random.levelup".into(),
                pos,
                (xp.level as f32 / 30.).min(1.) * 0.75,
                1.,
            ));
            xp.last_sound = ticks;
        }
        if n < 0 {
            "commands.xp.success.negative.levels"
        } else {
            "commands.xp.success.levels"
        }
    } else {
        ctx.results[4] = xp.total;
        if n < 0 {
            return Err(error("commands.xp.failure.widthdrawXp", vec![]));
        }
        let mut sounds = vec![];
        xp.add(n, ticks, &mut sounds);
        ctx.sounds
            .extend(sounds.into_iter().map(|(s, v, p)| (s.into(), pos, v, p)));
        "commands.xp.success"
    };
    ctx.sim.experience = xp.total.max(0) as u32;
    notify(
        ctx,
        key,
        vec![json!(n.wrapping_abs()), json!(ctx.options.username)],
    )
}
fn gamemode(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    let Some(&name) = a.first() else {
        return Err(usage("gamemode"));
    };
    let id = match name.to_ascii_lowercase().as_str() {
        "s" | "survival" => 0,
        "c" | "creative" => 1,
        "a" | "adventure" => 2,
        "sp" | "spectator" => 3,
        _ => integer(name, 0, 3)? as u8,
    };
    player_target(ctx, a.get(1).copied())?;
    ctx.sim.apply_local_mode(id, ctx.player);
    let mut replies = if ctx.sim.send_command_feedback {
        vec![tr("gameMode.changed", vec![])]
    } else { vec![] };
    if ctx.permission.is_some() {
        replies.push(tr("commands.gamemode.success.other", vec![json!(ctx.options.username),
            tr(["gameMode.survival", "gameMode.creative", "gameMode.adventure", "gameMode.spectator"][id as usize], vec![])]));
    }
    Ok(replies)
}
fn effects(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    if a.len() < 2 {
        return Err(usage("effect"));
    }
    let t = ctx.one(a[0])?;
    let name = ctx.target_name(t);
    let active = match t {
        Target::Player => Some(&ctx.sim.effects),
        Target::Mob(id) => ctx.sim.command_entities.mobs.get(&id).map(|v| &v.effects),
        _ => return Err(error("commands.generic.entity.notFound", vec![])),
    };
    let (edit, key, params) = if a[1] == "clear" {
        if !active.is_some_and(|e| e.ordered().next().is_some()) {
            return Err(error(
                "commands.effect.failure.notActive.all",
                vec![json!(name)],
            ));
        }
        (
            EffectEdit::Clear,
            "commands.effect.success.removed.all",
            vec![json!(name)],
        )
    } else {
        let regs = &items::registry().effects;
        let id = integer(a[1], 1, i32::MAX)
            .or_else(|err| regs.get(&items::resource(a[1])).map(|t| t.0).ok_or(err))?;
        let (_, key, instant) = regs
            .values()
            .find(|t| t.0 == id)
            .ok_or_else(|| error("commands.effect.notFound", vec![json!(id)]))?;
        let seconds = optional_num(a, 2, 30, 0, 1000000)?;
        let amplifier = optional_num(a, 3, 0, 0, 255)?;
        let translated = tr(key, vec![]);
        if seconds > 0 {
            let duration = if *instant {
                if a.len() >= 3 {
                    seconds
                } else {
                    1
                }
            } else {
                seconds * 20
            };
            let mut e = Effect::new(id as u8, duration, amplifier);
            e.show_particles = !a.get(4).is_some_and(|s| s.eq_ignore_ascii_case("true"));
            (
                EffectEdit::Add(e),
                "commands.effect.success",
                vec![
                    translated,
                    json!(id),
                    json!(amplifier),
                    json!(name),
                    json!(seconds),
                ],
            )
        } else {
            if active.and_then(|e| e.get(id as u8)).is_none() {
                return Err(error(
                    "commands.effect.failure.notActive",
                    vec![translated, json!(name)],
                ));
            }
            (
                EffectEdit::Remove(id as u8),
                "commands.effect.success.removed",
                vec![translated, json!(name)],
            )
        }
    };
    match t {
        Target::Player => ctx.sim.command_player_effect(edit),
        Target::Mob(id) => {
            if let Some(m) = ctx.sim.mobs.iter_mut().find(|m| m.id == id) {
                ctx.sim
                    .command_entities
                    .mobs
                    .entry(id)
                    .or_default()
                    .effect(m, edit)
            }
        }
        _ => unreachable!(),
    }
    notify(ctx, key, params)
}
fn kill(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    let t = if let Some(s) = a.first() {
        ctx.one(s)?
    } else {
        player_target(ctx, None)?
    };
    let name = ctx.target_name(t);
    match t {
        Target::Block(_) => return Err(error("commands.generic.player.unspecified", vec![])),
        Target::Player => {
            if ctx.sim.health > 0. {
                ctx.sim.hurt_camera.hurt();
            }
            ctx.sim.health = 0.;
            ctx.sim.death_message_key = Some("death.attack.outOfWorld");
        }
        Target::Mob(id) => {
            if let Some(m) = ctx.sim.mobs.iter_mut().find(|m| m.id == id) {
                m.health = 0.;
            }
        }
        Target::Drop(id) => ctx.sim.drops.retain(|d| d.id != id),
        Target::Orb(id) => ctx.sim.xp_orbs.retain(|o| o.id != id),
    }
    notify(ctx, "commands.kill.successful", vec![json!(name)])
}
pub(super) fn run(ctx: &mut Context<'_>, name: &str, args: &[&str]) -> Reply {
    match name {
        "give" => items::give(ctx, args),
        "clear" => items::clear(ctx, args),
        "replaceitem" => items::replace(ctx, args),
        "enchant" => items::enchant(ctx, args),
        "effect" => effects(ctx, args),
        "xp" => xp(ctx, args),
        "gamemode" => gamemode(ctx, args),
        "tp" => movement::teleport(ctx, args),
        "spawnpoint" => movement::spawnpoint(ctx, args),
        "spreadplayers" => movement::spread(ctx, args),
        "summon" => data::summon(ctx, args),
        "entitydata" => data::edit(ctx, args),
        "kill" => kill(ctx, args),
        "say" | "tell" | "msg" | "w" | "me" | "tellraw" | "title" => chat::run(ctx, name, args),
        _ => Err(error("commands.generic.notFound", vec![])),
    }
}
pub(super) fn complete(ctx: &Context<'_>, name: &str, a: &[&str]) -> Vec<String> {
    let reg = items::registry();
    let n = a.len();
    let user = || vec![ctx.options.username.clone()];
    match (name, n) {
        (
            "give" | "clear" | "enchant" | "effect" | "tell" | "msg" | "w" | "tellraw" | "title"
            | "spawnpoint" | "kill" | "entitydata" | "tp",
            1,
        )
        | ("xp" | "gamemode" | "tp", 2) => user(),
        ("give" | "clear", 2) => reg.items.keys().cloned().collect(),
        ("effect", 2) => reg.effects.keys().cloned().collect(),
        ("enchant", 2) => reg.enchantments.keys().cloned().collect(),
        ("effect", 5) => vec!["true".into(), "false".into()],
        ("gamemode", 1) => ["survival", "creative", "adventure", "spectator"]
            .map(String::from)
            .to_vec(),
        ("title", 2) => ["title", "subtitle", "clear", "reset", "times"]
            .map(String::from)
            .to_vec(),
        ("summon", 1) => MobKind::ALL.iter().map(|kind| kind.name()).chain(["Item", "XPOrb"]).map(String::from).collect(),
        ("replaceitem", 1) => vec!["entity".into(), "block".into()],
        ("replaceitem", 2) if a[0] == "entity" => user(),
        ("replaceitem", 3) if a[0] == "entity" => items::slots().into_keys().collect(),
        ("replaceitem", 5) if a[0] == "block" => items::slots().into_keys().collect(),
        ("replaceitem", 4) if a[0] == "entity" => reg.items.keys().cloned().collect(),
        ("replaceitem", 6) if a[0] == "block" => reg.items.keys().cloned().collect(),
        ("spreadplayers", 5) => vec!["true".into(), "false".into()],
        ("spreadplayers", n) if n >= 6 => user(),
        ("say" | "me" | "tell" | "msg" | "w", _) => user(),
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        player::Player,
        world::{Chunk, World},
        Options,
    };
    use clap::Parser;
    use std::sync::Arc;

    #[test]
    fn original_registry_has_expected_item_effect_and_enchantment_relationships() {
        let r = items::registry();
        assert_eq!(
            (r.items.len(), r.effects.len(), r.enchantments.len()),
            (337, 23, 25)
        );
        assert_eq!(r.items["minecraft:diamond_sword"], 276);
        let sharp = &r.enchantments["minecraft:sharpness"];
        assert!(sharp.4.contains(&276) && sharp.5.contains(&17));
        assert!(!sharp.4.contains(&261));
        assert_eq!(
            r.effects["minecraft:instant_health"],
            (6, "potion.heal".into(), true)
        );
    }

    #[test]
    #[ignore = "requires installed 1.8.9 resources"]
    fn original_player_commands_via_dispatcher() {
        let cases: Vec<Value> = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-entity-commands.json"
        ))
        .unwrap();
        let mut options = Options::parse_from(["ferrum189", "--username", "Player"]);
        options.world_rules.allow_cheats = true;
        let mut sim = Simulation::default();
        let p = glam::Vec3::new(8.5, 42., 8.5);
        let mut world = World {
            chunks: [((0, 0), Arc::new(Chunk::empty()))].into(),
            spawn: p,
            edits: Default::default(),
            label: "commands".into(),
            save_path: Default::default(),
            imported: false,
            light_revision: 0,
        };
        let mut player = Player::new(p);
        player.set_rotation_degrees(0., 0.);
        let r = crate::validation::resources();
        for case in cases {
            let cmd = case[0].as_str().unwrap();
            let out = super::super::execute(&mut sim, &mut player, &mut world, r, &options, cmd);
            let reply = out
                .messages
                .last()
                .unwrap_or_else(|| panic!("{cmd} had no response"));
            assert_eq!(
                reply.get("translate").or_else(|| reply.get("text")),
                Some(&case[1]),
                "{cmd} message: {reply}"
            );
            let slots = sim
                .inventory
                .slots
                .iter()
                .enumerate()
                .filter_map(|(i, s)| {
                    s.as_ref()
                        .map(|s| json!([i, s.id, s.count, items::meta(s)]))
                })
                .collect::<Vec<_>>();
            assert_eq!(json!(slots), case[2], "{cmd} slots");
            let effects = sim
                .effects
                .ordered()
                .map(|e| json!([e.id, e.duration, e.amplifier, e.show_particles]))
                .collect::<Vec<_>>();
            assert_eq!(json!(effects), case[3], "{cmd} effects");
            assert_eq!(
                json!([sim.xp.level, sim.xp.total]),
                json!([case[4], case[5]]),
                "{cmd} XP"
            );
            for (a, b) in player
                .precise_position()
                .into_iter()
                .chain(player.java_rotation().map(f64::from))
                .zip(&case.as_array().unwrap()[6..11])
            {
                assert!(
                    (a - b.as_f64().unwrap()).abs() < 0.00001,
                    "{cmd} pose: {a} vs {b}"
                );
            }
        }
        assert_eq!(sim.bed_spawn.unwrap().point, IVec3::new(9, 44, 11));
        let say = |sim: &mut Simulation, player: &mut Player, world: &mut World, line: &str| {
            super::super::execute(sim, player, world, r, &options, line).messages
        };
        assert_eq!(
            say(
                &mut sim,
                &mut player,
                &mut world,
                "summon Cow 4 10 5 {NoAI:1b,Age:-9,CustomName:Spot}"
            )[0]["translate"],
            "commands.summon.success"
        );
        let id = sim.mobs[0].id;
        assert_eq!(sim.command_entities.mobs[&id].name, "Spot");
        assert_eq!(sim.mob_brains[&id].growing_age, -9);
        assert_eq!(
            say(
                &mut sim,
                &mut player,
                &mut world,
                "entitydata @e[type=Cow,c=1] {HealF:3.5f}"
            )[0]["translate"],
            "commands.entitydata.success"
        );
        assert_eq!(sim.mobs[0].health, 3.5);
        assert_eq!(
            say(
                &mut sim,
                &mut player,
                &mut world,
                "testfor @e[type=Cow,c=1] {HealF:3.5f,NoAI:1b}"
            )[0]["translate"],
            "commands.testfor.success"
        );
    }
}
