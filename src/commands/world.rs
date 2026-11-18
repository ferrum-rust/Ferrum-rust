use super::{coordinate, error, integer, tr, usage, CmdResult, Context, Reply, Target};
use crate::{
    gameplay::Simulation,
    vanilla::{
        click_border::{self, Update},
        drops::DropNbt,
    },
};
use glam::{DVec3, IVec3};
use serde_json::{json, Value};
use std::collections::BTreeMap;
#[path = "regions.rs"]
mod regions;

pub(super) fn block_id(raw: &str) -> CmdResult<u16> {
    let key = raw.strip_prefix("minecraft:").unwrap_or(raw);
    crate::world_types::blocks()
        .get(key)
        .map(|v| v.0)
        .ok_or_else(|| {
            error(
                "commands.give.block.notFound",
                vec![json!(if raw.contains(':') {
                    raw.into()
                } else {
                    format!("minecraft:{raw}")
                })],
            )
        })
}
fn state(raw: &str, meta: i32) -> CmdResult<u16> {
    let id = block_id(raw)?;
    let m = crate::world_types::blocks()[raw.strip_prefix("minecraft:").unwrap_or(raw)].1
        [meta as usize];
    Ok(id << 4 | (m.max(0) as u16))
}
fn response(key: &str, args: Vec<Value>) -> Reply {
    Ok(vec![tr(key, args)])
}
fn fail(key: &str) -> Value {
    error(key, vec![])
}
fn decimal(raw: &str, lo: f64, hi: f64) -> CmdResult<f64> {
    let n = raw
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| error("commands.generic.num.invalid", vec![json!(raw)]))?;
    if n < lo {
        return Err(error(
            "commands.generic.double.tooSmall",
            vec![json!(n), json!(lo)],
        ));
    }
    if n > hi {
        return Err(error(
            "commands.generic.double.tooBig",
            vec![json!(n), json!(hi)],
        ));
    }
    Ok(n)
}
fn blockpos(ctx: &Context<'_>, args: &[&str]) -> CmdResult<IVec3> {
    let origin = ctx.origin.floor();
    let p = DVec3::new(
        coordinate(origin.x, args[0], false)?,
        coordinate(origin.y, args[1], false)?,
        coordinate(origin.z, args[2], false)?,
    );
    for (v, min, max) in [
        (p.x, -30_000_000., 30_000_000.),
        (p.y, 0., 256.),
        (p.z, -30_000_000., 30_000_000.),
    ] {
        if v < min {
            return Err(error(
                "commands.generic.double.tooSmall",
                vec![json!(v), json!(min)],
            ));
        }
        if v > max {
            return Err(error(
                "commands.generic.double.tooBig",
                vec![json!(v), json!(max)],
            ));
        }
    }
    Ok(p.floor().as_ivec3())
}
const RULES: [(&str, &str); 15] = [
    ("commandBlockOutput", "true"),
    ("doDaylightCycle", "true"),
    ("doEntityDrops", "true"),
    ("doFireTick", "true"),
    ("doMobLoot", "true"),
    ("doMobSpawning", "true"),
    ("doTileDrops", "true"),
    ("keepInventory", "false"),
    ("logAdminCommands", "true"),
    ("mobGriefing", "true"),
    ("naturalRegeneration", "true"),
    ("randomTickSpeed", "3"),
    ("reducedDebugInfo", "false"),
    ("sendCommandFeedback", "true"),
    ("showDeathMessages", "true"),
];
fn rules(sim: &Simulation) -> BTreeMap<String, String> {
    let mut rules: BTreeMap<_, _> = RULES
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    for (k, v) in [
        ("doDaylightCycle", sim.daylight_cycle),
        ("keepInventory", sim.keep_inventory),
        ("doMobLoot", sim.mob_loot),
        ("doMobSpawning", sim.mob_spawning),
        ("naturalRegeneration", sim.natural_regeneration),
        ("sendCommandFeedback", sim.send_command_feedback),
    ] {
        rules.insert(k.to_owned(), v.to_string());
    }
    rules.extend(sim.world_commands.rules.clone());
    rules
}
fn gamerule(ctx: &mut Context<'_>, args: &[&str]) -> Reply {
    let existing = rules(ctx.sim);
    let Some(&name) = args.first() else {
        let mut names = existing.keys().cloned().collect::<Vec<_>>();
        let last = names.pop().unwrap_or_default();
        return Ok(vec![
            json!({"text":format!("{} and {last}",names.join(", "))}),
        ]);
    };
    if args.len() == 1 {
        let value = existing
            .get(name)
            .ok_or_else(|| error("commands.gamerule.norule", vec![json!(name)]))?;
        ctx.results[4] = if value == "true" {
            1
        } else {
            value.parse().unwrap_or(0)
        };
        return Ok(vec![json!({"text":format!("{name} = {value}")})]);
    }
    let value = args[1..].join(" ");
    if RULES.iter().any(|(k, v)| *k == name && *v != "3")
        && !matches!(value.as_str(), "true" | "false")
    {
        return Err(error(
            "commands.generic.boolean.invalid",
            vec![json!(value)],
        ));
    }
    let enabled = value.eq_ignore_ascii_case("true");
    let sim = &mut ctx.sim;
    match name {
        "naturalRegeneration" => sim.natural_regeneration = enabled,
        "keepInventory" => sim.keep_inventory = enabled,
        "doDaylightCycle" => sim.daylight_cycle = enabled,
        "doMobLoot" => sim.mob_loot = enabled,
        "doMobSpawning" => sim.mob_spawning = enabled,
        "sendCommandFeedback" => sim.send_command_feedback = enabled,
        "reducedDebugInfo" => sim.server_reduced_debug = enabled,
        "doEntityDrops" => sim.block_ticks.set_do_entity_drops(enabled),
        _ => {}
    }
    sim.world_commands.rules.insert(name.to_owned(), value);
    response("commands.gamerule.success", vec![])
}
fn seconds(raw: &str) -> CmdResult<i64> {
    let n = raw
        .parse::<i64>()
        .map_err(|_| error("commands.generic.num.invalid", vec![json!(raw)]))?;
    if n < 0 {
        return Err(error(
            "commands.generic.num.tooSmall",
            vec![json!(n), json!(0)],
        ));
    }
    if n > i64::MAX / 1000 {
        return Err(error(
            "commands.generic.num.tooBig",
            vec![json!(n), json!(i64::MAX / 1000)],
        ));
    }
    Ok(n * 1000)
}
fn border(ctx: &mut Context<'_>, args: &[&str]) -> Reply {
    let t = click_border::now();
    let b = &mut ctx.sim.world_border;
    match args {
        ["set" | "add", size] | ["set" | "add", size, _] => {
            let adding = args[0] == "add";
            let old = if adding { b.diameter(t) } else { b.target() };
            let next = if adding {
                old + decimal(size, -old, 60_000_000. - old)?
            } else {
                decimal(size, 1., 60_000_000.)?
            };
            let mut duration = args
                .get(2)
                .map(|raw| seconds(raw))
                .transpose()?
                .unwrap_or(0);
            if adding {
                duration = duration.saturating_add(b.remaining(t));
            }
            let mut values = vec![json!(format!("{next:.1}")), json!(format!("{old:.1}"))];
            let key = if duration > 0 {
                b.apply(Update::Lerp(old, next, duration), t);
                values.push(json!((duration / 1000).to_string()));
                if old > next {
                    "commands.worldborder.setSlowly.shrink.success"
                } else {
                    "commands.worldborder.setSlowly.grow.success"
                }
            } else {
                b.apply(Update::Size(next), t);
                "commands.worldborder.set.success"
            };
            response(key, values)
        }
        ["center", x, z] => {
            let x = coordinate(ctx.origin.x.floor() + 0.5, x, true)?;
            let z = coordinate(ctx.origin.z.floor() + 0.5, z, true)?;
            b.apply(Update::Center(x, z), t);
            response(
                "commands.worldborder.center.success",
                vec![json!(x), json!(z)],
            )
        }
        ["damage", kind, raw] if matches!(*kind, "buffer" | "amount") => {
            let n = decimal(raw, 0., f64::MAX)?;
            let (prev, digits) = if *kind == "amount" {
                (
                    std::mem::replace(&mut ctx.sim.world_commands.border_damage, n),
                    2,
                )
            } else {
                (
                    std::mem::replace(&mut ctx.sim.world_commands.border_buffer, n),
                    1,
                )
            };
            response(
                &format!("commands.worldborder.damage.{kind}.success"),
                vec![
                    json!(format!("{n:.digits$}")),
                    json!(format!("{prev:.digits$}")),
                ],
            )
        }
        ["warning", kind, raw] if matches!(*kind, "time" | "distance") => {
            let n = integer(raw, 0, i32::MAX)?;
            let prev = if *kind == "time" {
                let p = b.warning_time;
                b.apply(Update::Time(n), t);
                p
            } else {
                let p = b.warning_distance;
                b.apply(Update::Distance(n), t);
                p
            };
            response(
                &format!("commands.worldborder.warning.{kind}.success"),
                vec![json!(n), json!(prev)],
            )
        }
        ["get", ..] => {
            let size = b.diameter(t);
            ctx.results[4] = (size + 0.5).floor() as i32;
            response(
                "commands.worldborder.get.success",
                vec![json!(format!("{size:.0}"))],
            )
        }
        [sub, ..] if matches!(*sub, "center" | "set" | "add" | "damage" | "warning") => {
            Err(usage(&format!("commands.worldborder.{sub}.usage")))
        }
        [] => Err(usage("worldborder")),
        _ => Ok(vec![]),
    }
}
const PARTICLES: [&str; 42] = [
    "explode",
    "largeexplode",
    "hugeexplosion",
    "fireworksSpark",
    "bubble",
    "splash",
    "wake",
    "suspended",
    "depthsuspend",
    "crit",
    "magicCrit",
    "smoke",
    "largesmoke",
    "spell",
    "instantSpell",
    "mobSpell",
    "mobSpellAmbient",
    "witchMagic",
    "dripWater",
    "dripLava",
    "angryVillager",
    "happyVillager",
    "townaura",
    "note",
    "portal",
    "enchantmenttable",
    "flame",
    "lava",
    "footstep",
    "cloud",
    "reddust",
    "snowballpoof",
    "snowshovel",
    "slime",
    "heart",
    "barrier",
    "iconcrack_",
    "blockcrack_",
    "blockdust_",
    "droplet",
    "take",
    "mobappearance",
];
fn particle(ctx: &mut Context<'_>, args: &[&str]) -> Reply {
    if args.len() < 8 {
        return Err(usage("particle"));
    }
    let unknown = || error("commands.particle.notFound", vec![json!(args[0])]);
    let id = PARTICLES
        .iter()
        .position(|n| {
            if n.ends_with('_') {
                args[0].starts_with(n)
            } else {
                args[0] == *n
            }
        })
        .ok_or_else(unknown)?;
    let mut data = [0; 2];
    if (36..=38).contains(&id) {
        for (i, arg) in args[0].splitn(3, '_').skip(1).enumerate() {
            if arg.is_empty() {
                continue;
            }
            if i > 0 && id != 36 {
                return Err(unknown());
            }
            data[i] = arg.parse().map_err(|_| unknown())?;
        }
    }
    let mut pos = [0.; 3];
    let mut offset = [0.; 3];
    for i in 0..3 {
        pos[i] = coordinate(ctx.origin[i], args[i + 1], true)? as f32;
        offset[i] = decimal(args[i + 4], -f64::MAX, f64::MAX)? as f32;
    }
    let speed = decimal(args[7], -f64::MAX, f64::MAX)? as f32;
    let count = args
        .get(8)
        .map(|s| integer(s, 0, i32::MAX))
        .transpose()?
        .unwrap_or(0);
    let far = args.get(9) == Some(&"force");
    let mut events = crate::net_presentation::WorldEvents::default();
    events.handle(
        crate::net_presentation::Packet::Particles {
            id: id as u8,
            far,
            pos,
            offset,
            speed,
            count,
            args: data,
        },
        ctx.player,
        ctx.sim,
        ctx.world,
    );
    for explosion in events.explosions {
        if ctx.sim.explosion_clouds.len() < 512 {
            ctx.sim.explosion_clouds.push(explosion);
        }
    }
    response(
        "commands.particle.success",
        vec![json!(args[0]), json!(count.max(1))],
    )
}
fn playsound(ctx: &mut Context<'_>, args: &[&str]) -> Reply {
    if args.len() < 2 {
        return Err(usage("playsound"));
    }
    let target = ctx.one(args[1])?;
    if target != Target::Player {
        return Err(fail("commands.generic.player.notFound"));
    }
    let listener = DVec3::from_array(ctx.player.precise_position());
    let mut p = ctx.origin;
    for i in 0..3 {
        if let Some(s) = args.get(i + 2) {
            p[i] = coordinate(p[i], s, i != 1)?;
        }
    }
    let mut volume = args
        .get(5)
        .map(|s| decimal(s, 0., f64::from(f32::MAX)))
        .transpose()?
        .unwrap_or(1.);
    let pitch = args
        .get(6)
        .map(|s| decimal(s, 0., 2.))
        .transpose()?
        .unwrap_or(1.);
    let min = args
        .get(7)
        .map(|s| decimal(s, 0., 1.))
        .transpose()?
        .unwrap_or(0.);
    let name = ctx.target_name(target);
    if p.distance(listener) > 16. * volume.max(1.) {
        if min == 0. {
            return Err(error("commands.playsound.playerTooFar", vec![json!(name)]));
        }
        p = listener + (p - listener).normalize_or_zero() * 2.;
        volume = min;
    }
    ctx.sounds
        .push((args[0].to_owned(), p, volume as f32, pitch as f32));
    response(
        "commands.playsound.success",
        vec![json!(args[0]), json!(name)],
    )
}
pub(super) fn run(ctx: &mut Context<'_>, name: &str, args: &[&str]) -> Reply {
    match name {
        "seed" => response(
            "commands.seed.success",
            vec![json!(ctx
                .sim
                .world_commands
                .imported_seed
                .unwrap_or(ctx.options.seed as i64)
                .to_string())],
        ),
        "gamerule" => gamerule(ctx, args),
        "worldborder" => border(ctx, args),
        "particle" => particle(ctx, args),
        "playsound" => playsound(ctx, args),
        "time" if args.len() >= 2 => {
            let n = match (args[0], args[1]) {
                ("set", "day") => 1000,
                ("set", "night") => 13000,
                ("set" | "add", s) => integer(s, 0, i32::MAX)?,
                ("query", "daytime") => (ctx.sim.clock % i32::MAX as u64) as i32,
                ("query", "gametime") => (ctx.sim.total_ticks % i32::MAX as u64) as i32,
                _ => return Err(usage(name)),
            };
            let key = match args[0] {
                "set" => {
                    ctx.sim.clock = n as u64;
                    "commands.time.set"
                }
                "add" => {
                    ctx.sim.clock = ctx.sim.clock.wrapping_add(n as u64);
                    "commands.time.added"
                }
                _ => {
                    ctx.results[4] = n;
                    "commands.time.query"
                }
            };
            response(key, vec![json!(n.to_string())])
        }
        "toggledownfall" => {
            ctx.sim.weather.toggle_rain();
            response("commands.downfall.success", vec![])
        }
        "weather" if matches!(args.len(), 1 | 2) => {
            let ticks = if let Some(s) = args.get(1) {
                integer(s, 1, 1_000_000)?
            } else {
                let mut rng = crate::vanilla::random::JavaRandom::new(
                    crate::vanilla::random::JavaRandom::independent_seed(),
                );
                300 + rng.next_int(600)
            } * 20;
            let kind = args[0].to_ascii_lowercase();
            if !matches!(&*kind, "clear" | "rain" | "thunder") {
                return Err(usage(name));
            }
            ctx.sim
                .weather
                .command(kind != "clear", kind == "thunder", ticks);
            response(&format!("commands.weather.{kind}"), vec![])
        }
        "difficulty" if !args.is_empty() => {
            let d = match args[0].to_ascii_lowercase().as_str() {
                "peaceful" | "p" => 0,
                "easy" | "e" => 1,
                "normal" | "n" => 2,
                "hard" | "h" => 3,
                _ => integer(args[0], 0, 3)?,
            };
            ctx.sim.world_difficulty = Some(if ctx.sim.hardcore { 3 } else { d as u8 });
            response(
                "commands.difficulty.success",
                vec![tr(
                    [
                        "options.difficulty.peaceful",
                        "options.difficulty.easy",
                        "options.difficulty.normal",
                        "options.difficulty.hard",
                    ][d as usize],
                    vec![],
                )],
            )
        }
        "setworldspawn" => {
            let p = match args.len() {
                0 => ctx.origin.floor().as_ivec3(),
                3 => blockpos(ctx, args)?,
                _ => return Err(usage(name)),
            };
            ctx.world.spawn = p.as_vec3();
            ctx.sim.world_commands.spawn = Some(p.to_array());
            response(
                "commands.setworldspawn.success",
                p.to_array().map(|v| json!(v)).to_vec(),
            )
        }
        "setblock" | "fill" | "clone" | "blockdata" | "testforblock" | "testforblocks" => {
            regions::run(ctx, name, args)
        }
        _ => Err(usage(name)),
    }
}
pub(super) fn complete(ctx: &Context<'_>, name: &str, args: &[&str]) -> Vec<String> {
    let n = args.len();
    let previous = args.first().copied().unwrap_or("");
    let static_list: &[&str] = match (name, n) {
        ("time", 1) => &["set", "add", "query"],
        ("time", 2) if previous == "set" => &["day", "night"],
        ("time", 2) if previous == "query" => &["daytime", "gametime"],
        ("weather", 1) => &["clear", "rain", "thunder"],
        ("difficulty", 1) => &["peaceful", "easy", "normal", "hard"],
        ("setblock", 6) => &["replace", "destroy", "keep"],
        ("fill", 9) => &["replace", "destroy", "keep", "hollow", "outline"],
        ("testforblocks", 10) => &["all", "masked"],
        ("clone", 10) => &["replace", "masked", "filtered"],
        ("clone", 11) => &["normal", "force", "move"],
        ("worldborder", 1) => &["set", "center", "damage", "warning", "add", "get"],
        ("worldborder", 2) if previous == "damage" => &["buffer", "amount"],
        ("worldborder", 2) if previous == "warning" => &["time", "distance"],
        ("particle", 1) => &PARTICLES,
        ("particle", 10) => &["normal", "force"],
        ("gamerule", 2) if RULES.iter().any(|(k, v)| *k == previous && *v != "3") => {
            &["true", "false"]
        }
        ("gamerule", 1) => return rules(ctx.sim).into_keys().collect(),
        ("playsound", 2) => return vec![ctx.options.username.clone()],
        ("fill", 7 | 10) | ("setblock" | "testforblock", 4) | ("clone", 12) => {
            return crate::world_types::blocks()
                .keys()
                .map(|n| format!("minecraft:{n}"))
                .collect()
        }
        _ => &[],
    };
    static_list.iter().map(|s| s.to_string()).collect()
}
#[cfg(test)]
#[path = "world_tests.rs"]
mod oracle_tests;
