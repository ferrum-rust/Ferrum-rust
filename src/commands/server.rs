use super::*;
const STATS: [&str; 5] = [
    "SuccessCount",
    "AffectedBlocks",
    "AffectedEntities",
    "AffectedItems",
    "QueryResult",
];
fn ok(key: &str, args: Vec<Value>) -> Reply {
    Ok(vec![tr(key, args)])
}
fn help(ctx: &Context<'_>, a: &[&str]) -> Reply {
    let entries: Vec<_> = CATALOG
        .commands
        .iter()
        .filter(|(n, _)| ctx.allowed(n))
        .collect();
    let pages = entries.len().div_ceil(7);
    let page = match a.first() {
        None => 1,
        Some(s) => match integer(s, 1, pages as i32) {
            Ok(n) => n as usize,
            Err(e) => {
                return Err(if let Some(n) = canonical(s) {
                    usage(n)
                } else if s.parse::<i32>().is_ok() {
                    e
                } else {
                    error("commands.generic.notFound", vec![])
                })
            }
        },
    };
    let mut header = tr("commands.help.header", vec![json!(page), json!(pages)]);
    header["color"] = json!("dark_green");
    let mut out = vec![header];
    for (name, entry) in entries.into_iter().skip((page - 1) * 7).take(7) {
        let mut v = tr(&entry.0, vec![]);
        v["clickEvent"] = json!({"action":"suggest_command","value":format!("/{name} ")});
        out.push(v)
    }
    if page == 1 && ctx.depth == 0 && ctx.permission.is_none() {
        let mut footer = tr("commands.help.footer", vec![]);
        footer["color"] = json!("green");
        out.push(footer)
    }
    Ok(out)
}
fn xyz(base: DVec3, a: &[&str]) -> CmdResult<DVec3> {
    Ok(DVec3::new(
        coordinate(base.x, a[0], false)?,
        coordinate(base.y, a[1], false)?,
        coordinate(base.z, a[2], false)?,
    ))
}
fn execute(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    if a.len() < 5 {
        return Err(usage("execute"));
    }
    let target = ctx.one(a[0])?;
    let origin = xyz(ctx.position(target), &a[1..4])?;
    let command = if a[4] == "detect" && a.len() > 10 {
        let pos = xyz(origin, &a[5..8])?.floor().as_ivec3();
        let id = world::block_id(a[8])?;
        let meta = integer(a[9], -1, 15)?;
        let block = ctx.world.get(pos);
        if block >> 4 != id || meta >= 0 && i32::from(block & 15) != meta {
            return Err(error(
                "commands.execute.failed",
                vec![json!("detect"), json!(ctx.target_name(target))],
            ));
        }
        &a[10..]
    } else {
        &a[4..]
    };
    let command = command.join(" ");
    if ctx.depth >= 32 {
        return Err(error(
            "commands.execute.failed",
            vec![json!(command), json!(ctx.target_name(target))],
        ));
    }
    let previous = (ctx.origin, ctx.sender, ctx.results);
    ctx.origin = origin;
    ctx.sender = target;
    ctx.depth += 1;
    let mut out = dispatch(ctx, &command);
    let success = ctx.results[0];
    ctx.depth -= 1;
    (ctx.origin, ctx.sender, ctx.results) = previous;
    if success == 0 {
        ctx.forwarded.append(&mut out);
        return Err(error(
            "commands.execute.failed",
            vec![json!(command), json!(ctx.target_name(target))],
        ));
    }
    Ok(out)
}
fn achievement(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    let [op, id, rest @ ..] = a else {
        return Err(usage("achievement"));
    };
    let give = op.eq_ignore_ascii_case("give");
    if !give && !op.eq_ignore_ascii_case("take") {
        return Err(usage("achievement"));
    }
    let t = if let Some(raw) = rest.first() {
        ctx.one(raw)?
    } else {
        ctx.sender
    };
    if t != Target::Player {
        return Err(error("commands.generic.player.notFound", vec![]));
    }
    let who = ctx.target_name(t);
    let registry = &CATALOG.statistics;
    let key = format!(
        "commands.achievement.{}",
        if give { "give" } else { "take" }
    );
    let stats = &mut ctx.sim.statistics;
    if *id == "*" {
        for k in registry.keys().filter(|k| k.starts_with("achievement.")) {
            if give {
                stats.insert(k.clone(), 1);
            } else {
                stats.remove(k);
            }
        }
        return ok(&format!("{key}.success.all"), vec![json!(who)]);
    }
    if !registry.contains_key(*id) {
        return Err(error(
            "commands.achievement.unknownAchievement",
            vec![json!(id)],
        ));
    }
    let title = tr(id, vec![]);
    let unlocked = stats.get(*id).copied().unwrap_or(0) > 0;
    if id.starts_with("achievement.") {
        if unlocked == give {
            return Err(error(
                if give {
                    "commands.achievement.alreadyHave"
                } else {
                    "commands.achievement.dontHave"
                },
                vec![json!(who), title],
            ));
        }
        if give {
            let mut p = *id;
            while let Some(parent) = registry.get(p) {
                stats.insert(p.to_owned(), 1);
                if parent.is_empty() {
                    break;
                }
                p = parent;
            }
        } else {
            stats.retain(|k, _| {
                let mut p = k.as_str();
                while !p.is_empty() {
                    if p == *id {
                        return false;
                    }
                    p = registry.get(p).map_or("", String::as_str)
                }
                true
            })
        }
    } else if give {
        let n = stats.entry(id.to_string()).or_default();
        *n = n.wrapping_add(1)
    } else {
        stats.remove(*id);
    }
    ok(
        &format!("{key}.success.one"),
        if give {
            vec![json!(who), title]
        } else {
            vec![title, json!(who)]
        },
    )
}
fn stats(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    let (key, args) = match a {
        ["entity", who, rest @ ..] => {
            let target = ctx.one(who)?;
            (ctx.target_key(target), rest)
        }
        ["block", x, y, z, rest @ ..] => {
            let p = xyz(ctx.origin, &[x, y, z])?.floor().as_ivec3();
            if !matches!(ctx.world.get(p) >> 4, 63 | 68 | 137) {
                return Err(error(
                    "commands.stats.noCompatibleBlock",
                    p.to_array().map(|n| json!(n)).to_vec(),
                ));
            }
            (format!("block,{},{},{}", p.x, p.y, p.z), rest)
        }
        _ => return Err(usage("stats")),
    };
    let Some(&ty) = args.get(1) else {
        return Err(usage("stats"));
    };
    if !STATS.contains(&ty) {
        return Err(error("commands.stats.failed", vec![]));
    }
    match args {
        ["set", _, who, obj, ..] => {
            ctx.sim
                .commands
                .stat_bindings
                .entry(key)
                .or_default()
                .insert(ty.into(), (who.to_string(), obj.to_string()));
            ok(
                "commands.stats.success",
                vec![json!(ty), json!(obj), json!(who)],
            )
        }
        ["clear", _, ..] => {
            if let Some(map) = ctx.sim.commands.stat_bindings.get_mut(&key) {
                map.remove(ty);
            }
            ok("commands.stats.cleared", vec![json!(ty)])
        }
        _ => Err(usage("stats")),
    }
}
pub(super) fn apply_stats(ctx: &mut Context<'_>) {
    let Some(bindings) = ctx
        .sim
        .commands
        .stat_bindings
        .get(&ctx.target_key(ctx.sender))
        .cloned()
    else {
        return;
    };
    for ((index, ty), value) in STATS.iter().enumerate().zip(ctx.results.data) {
        if !ctx.results.written[index] {
            continue;
        }
        if let Some((who, obj)) = bindings.get(*ty) {
            let who = if who.starts_with('@') {
                match ctx.one(who) {
                    Ok(t) => ctx.target_key(t),
                    Err(_) => continue,
                }
            } else {
                who.clone()
            };
            ctx.sim.commands.board.write_existing(&who, obj, value)
        }
    }
    sync_board(ctx.sim)
}
pub(super) fn run(ctx: &mut Context<'_>, name: &str, a: &[&str]) -> Reply {
    match name {
        "help" => help(ctx, a),
        "execute" => execute(ctx, a),
        "achievement" => achievement(ctx, a),
        "stats" => stats(ctx, a),
        "scoreboard" => scoreboard::run(ctx, a),
        "trigger" => scoreboard::trigger(ctx, a),
        "testfor" => {
            let Some((raw, tag)) = a.split_first() else {
                return Err(usage(name));
            };
            let target = ctx.one(raw)?;
            if !tag.is_empty()
                && !snbt::matches(
                    &snbt::parse(&tag.join(" "))?,
                    &entities::entity_nbt(ctx, target),
                )
            {
                return Err(error(
                    "commands.testfor.failure",
                    vec![json!(ctx.target_name(target))],
                ));
            }
            ok(
                "commands.testfor.success",
                vec![json!(ctx.target_name(target))],
            )
        }
        "defaultgamemode" => {
            let Some(s) = a.first() else {
                return Err(usage(name));
            };
            let id = match s.to_ascii_lowercase().as_str() {
                "s" | "survival" => 0,
                "c" | "creative" => 1,
                "a" | "adventure" => 2,
                "sp" | "spectator" => 3,
                _ => integer(s, 0, 3)? as u8,
            };
            crate::world_library::Library::default_mode(ctx.options,id).map_err(|_|error("commands.generic.exception",vec![]))?;
            ctx.sim.commands.default_mode = Some(id);
            ok(
                "commands.defaultgamemode.success",
                vec![tr(
                    [
                        "gameMode.survival",
                        "gameMode.creative",
                        "gameMode.adventure",
                        "gameMode.spectator",
                    ][id as usize],
                    vec![],
                )],
            )
        }
        "debug" => match a {
            ["start"] => {
                ctx.sim.commands.start_profile(ctx.sim.total_ticks);
                ok("commands.debug.start", vec![])
            }
            ["stop"] => {
                let profile = ctx
                    .sim
                    .commands
                    .debug
                    .take()
                    .ok_or_else(|| error("commands.debug.notStarted", vec![]))?;
                let seconds = profile.since.elapsed().as_secs_f64();
                let ticks = ctx.sim.total_ticks.wrapping_sub(profile.first_tick);
                let folder = ctx.options.project_dir.join("debug");
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let file = folder.join(format!("profile-results-{stamp}.txt"));
                let mut report=format!("---- Minecraft Profiler Results ----\nTime span: {:.0} ms\nTick span: {ticks} ticks\nAverage: {:.2} ticks per second\n--- BEGIN PROFILE DUMP ---\n",seconds*1000.,ticks as f64/seconds.max(0.001));
                let active: f64 = profile
                    .sections
                    .values()
                    .map(std::time::Duration::as_secs_f64)
                    .sum();
                for (section, duration) in profile.sections {
                    let t = duration.as_secs_f64();
                    report.push_str(&format!(
                        "[00] {section} - {:.2}%/{:.2}% ({:.3} ms)\n",
                        100. * t / active.max(1e-9),
                        100. * t / seconds.max(1e-9),
                        t * 1000.
                    ));
                }
                report.push_str("--- END PROFILE DUMP ---\n");
                std::fs::create_dir_all(folder)
                    .and_then(|_| std::fs::write(file, report))
                    .map_err(|_| error("commands.generic.exception", vec![]))?;
                ok(
                    "commands.debug.stop",
                    vec![json!(format!("{seconds:.2}")), json!(ticks)],
                )
            }
            _ => Err(usage(name)),
        },
        "publish" => crate::lan::publish(ctx.sim, ctx.world, ctx.player, ctx.options)
            .map(|port| vec![tr("commands.publish.started", vec![json!(port)])])
            .map_err(|_| error("commands.publish.failed", vec![])),
        _ => Err(error("commands.generic.notFound", vec![])),
    }
}
pub(super) fn complete(ctx: &Context<'_>, name: &str, a: &[&str]) -> Vec<String> {
    match (name, a) {
        ("scoreboard" | "trigger", _) => scoreboard::complete(ctx, name, a),
        ("help", [_]) => CATALOG
            .commands
            .iter()
            .flat_map(|(k, e)| std::iter::once(k).chain(e.2.iter()))
            .cloned()
            .collect(),
        ("defaultgamemode", [_]) => names(["survival", "creative", "adventure", "spectator"]),
        ("debug", [_]) => names(["start", "stop"]),
        ("achievement", [_]) => names(["give", "take"]),
        ("achievement", [_, _]) => CATALOG.statistics.keys().cloned().collect(),
        ("achievement", [_, _, _]) | ("testfor" | "execute", [_]) | ("stats", ["entity", _]) => {
            vec![ctx.options.username.clone()]
        }
        ("stats", [_]) => names(["entity", "block"]),
        ("stats", ["entity", _, _] | ["block", _, _, _, _]) => names(["set", "clear"]),
        ("stats", ["entity", _, _, _] | ["block", _, _, _, _, _]) => names(STATS),
        ("execute", [_, _, _, _, tail @ ..]) if !tail.is_empty() => {
            if tail[0] == "detect" && tail.len() >= 7 {
                candidates(ctx, &tail[6..].join(" "))
            } else {
                candidates(ctx, &tail.join(" "))
            }
        }
        _ => vec![],
    }
}
