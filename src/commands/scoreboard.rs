use super::*;
use crate::net_presentation::{Objective, State, Team};
const COLORS: [&str; 16] = [
    "black",
    "dark_blue",
    "dark_green",
    "dark_aqua",
    "dark_red",
    "dark_purple",
    "gold",
    "gray",
    "dark_gray",
    "blue",
    "green",
    "aqua",
    "red",
    "light_purple",
    "yellow",
    "white",
];
const OPTIONS: [&str; 5] = [
    "color",
    "friendlyfire",
    "seeFriendlyInvisibles",
    "nametagVisibility",
    "deathMessageVisibility",
];
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
struct Obj {
    criteria: String,
    label: String,
    scores: BTreeMap<String, i32>,
    unlocked: BTreeSet<String>,
    #[serde(default)]
    observed: i32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Group {
    label: String,
    color: i8,
    flags: u8,
    visibility: String,
    death_visibility: String,
}
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub(super) struct Board {
    objects: BTreeMap<String, Obj>,
    groups: BTreeMap<String, Group>,
    pub(super) members: BTreeMap<String, String>,
    display: [String; 19],
}
fn stat_for(criteria: &str) -> Option<&str> {
    match criteria {
        "deathCount" => Some("stat.deaths"),
        "playerKillCount" => Some("stat.playerKills"),
        "totalKillCount" => Some("stat.mobKills"),
        key if CATALOG.statistics.contains_key(key) => Some(key),
        _ => None,
    }
}
pub(super) fn observe(sim: &mut Simulation) {
    let mut changed = false;
    for obj in sim.commands.board.objects.values_mut() {
        let health = obj.criteria == "health";
        let value = if health {
            (sim.health + sim.absorption).ceil() as i32
        } else if let Some(key) = stat_for(&obj.criteria) {
            sim.statistics.get(key).copied().unwrap_or(0)
        } else {
            continue;
        };
        if obj.observed == value {
            continue;
        }
        let score = obj.scores.entry(sim.local_name.clone()).or_default();
        *score = if health {
            value
        } else {
            score.wrapping_add(value.wrapping_sub(obj.observed))
        };
        obj.observed = value;
        changed = true;
    }
    if changed {
        sync_board(sim)
    }
}
fn slots() -> Vec<String> {
    let mut v = names(["list", "sidebar", "belowName"]);
    v.extend(COLORS.map(|c| format!("sidebar.team.{c}")));
    v
}
impl Board {
    pub(super) fn score(&self, user: &str, obj: &str) -> Option<i32> {
        self.objects.get(obj)?.scores.get(user).copied()
    }
    pub(super) fn write_existing(&mut self, user: &str, obj: &str, n: i32) {
        if let Some(s) = self
            .objects
            .get_mut(obj)
            .and_then(|o| o.scores.get_mut(user))
        {
            *s = n
        }
    }
    fn users(&self) -> BTreeSet<String> {
        self.objects
            .values()
            .flat_map(|o| o.scores.keys().cloned())
            .collect()
    }
    pub(super) fn present(&self, state: &mut State) {
        state.display.clone_from(&self.display);
        state.objectives = self
            .objects
            .iter()
            .map(|(k, o)| {
                (
                    k.clone(),
                    Objective {
                        title: o.label.clone(),
                        render_type: if o.criteria == "health" {
                            "hearts"
                        } else {
                            "integer"
                        }
                        .into(),
                        scores: o.scores.clone(),
                    },
                )
            })
            .collect();
        state.memberships = self.members.clone().into_iter().collect();
        state.teams = self
            .groups
            .iter()
            .map(|(k, t)| {
                let color = usize::try_from(t.color)
                    .ok()
                    .and_then(|i| "0123456789abcdef".chars().nth(i));
                (
                    k.clone(),
                    Team {
                        label: t.label.clone(),
                        flags: t.flags,
                        visibility: t.visibility.clone(),
                        color: t.color,
                        prefix: color.map_or_else(String::new, |c| format!("§{c}")),
                        suffix: if color.is_some() { "§r" } else { "" }.into(),
                    },
                )
            })
            .collect();
    }
    fn object(&mut self, id: &str, write: bool) -> CmdResult<&mut Obj> {
        let o = self
            .objects
            .get_mut(id)
            .ok_or_else(|| error("commands.scoreboard.objectiveNotFound", vec![json!(id)]))?;
        if write && o.criteria == "health" {
            return Err(error(
                "commands.scoreboard.objectiveReadOnly",
                vec![json!(id)],
            ));
        }
        Ok(o)
    }
}
fn ok(key: &str, v: Vec<Value>) -> Reply {
    Ok(vec![tr(&format!("commands.scoreboard.{key}"), v)])
}
fn err(key: &str, v: Vec<Value>) -> Value {
    error(&format!("commands.scoreboard.{key}"), v)
}
fn user(ctx: &Context<'_>, s: &str) -> CmdResult<String> {
    let name = if s.starts_with('@') {
        ctx.target_key(ctx.one(s)?)
    } else {
        s.to_owned()
    };
    if name.encode_utf16().count() > 40 {
        return Err(err("players.name.tooLong", vec![json!(s), json!(40)]));
    }
    Ok(name)
}
pub(super) fn username_index(a: &[&str], i: usize) -> bool {
    match a.first().copied() {
        Some("players") => i == 2 || (a.get(1) == Some(&"operation") && i == 5),
        Some("teams") => i == 2,
        _ => false,
    }
}
fn cmd_usage(a: &[&str]) -> Value {
    let path = a.iter().take(2).copied().collect::<Vec<_>>().join(".");
    let key = if path.is_empty() {
        "commands.scoreboard.usage".into()
    } else {
        format!("commands.scoreboard.{path}.usage")
    };
    usage(&key)
}
pub(super) fn run(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    let stars: Vec<_> = a
        .iter()
        .enumerate()
        .filter(|(i, s)| **s == "*" && username_index(a, *i))
        .map(|(i, _)| i)
        .collect();
    let result = match stars.as_slice() {
        [] => command(ctx, a),
        [index] => {
            let mut replies = vec![];
            let mut successes = 0;
            for name in ctx.sim.commands.board.users() {
                let mut words = a.to_vec();
                words[*index] = &name;
                match command(ctx, &words) {
                    Ok(v) => {
                        successes += 1;
                        replies.extend(v)
                    }
                    Err(e) => replies.push(e),
                }
            }
            ctx.results[2] = successes;
            if successes == 0 {
                Err(err("allMatchesFailed", vec![]))
            } else {
                Ok(replies)
            }
        }
        _ => Err(err("noMultiWildcard", vec![])),
    };
    sync_board(ctx.sim);
    result
}
fn command(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    match a.first().copied() {
        Some("objectives") => objectives(ctx, a),
        Some("players") => players(ctx, a),
        Some("teams") => teams(ctx, a),
        _ => Err(usage("scoreboard")),
    }
}
fn objectives(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    let b = &mut ctx.sim.commands.board;
    match a {
        [_, "add", id, criteria, label @ ..] => {
            if !CATALOG.criteria.contains(*criteria) {
                return Err(err("objectives.add.wrongType", vec![json!(criteria)]));
            }
            if b.objects.contains_key(*id) {
                return Err(err("objectives.add.alreadyExists", vec![json!(id)]));
            }
            if id.encode_utf16().count() > 16 {
                return Err(err("objectives.add.tooLong", vec![json!(id), json!(16)]));
            }
            let label = if label.is_empty() {
                id.to_string()
            } else {
                label.join(" ")
            };
            if label.encode_utf16().count() > 32 {
                return Err(err(
                    "objectives.add.displayTooLong",
                    vec![json!(label), json!(32)],
                ));
            }
            let observed = stat_for(criteria)
                .and_then(|key| ctx.sim.statistics.get(key))
                .copied()
                .unwrap_or_default();
            b.objects.insert(
                id.to_string(),
                Obj {
                    criteria: criteria.to_string(),
                    label,
                    observed,
                    ..Obj::default()
                },
            );
            ok("objectives.add.success", vec![json!(id)])
        }
        [_, "remove", id] => {
            b.object(id, false)?;
            b.objects.remove(*id);
            for s in &mut b.display {
                if s == id {
                    s.clear()
                }
            }
            ok("objectives.remove.success", vec![json!(id)])
        }
        [_, "setdisplay", slot, rest @ ..] if rest.len() <= 1 => {
            let idx = slots()
                .iter()
                .position(|s| s.eq_ignore_ascii_case(slot))
                .ok_or_else(|| err("objectives.setdisplay.invalidSlot", vec![json!(slot)]))?;
            if let Some(id) = rest.first() {
                b.object(id, false)?;
                b.display[idx] = id.to_string();
                ok(
                    "objectives.setdisplay.successSet",
                    vec![json!(slot), json!(id)],
                )
            } else {
                b.display[idx].clear();
                ok("objectives.setdisplay.successCleared", vec![json!(slot)])
            }
        }
        [_, "list", ..] => {
            ctx.results[4] = b.objects.len() as i32;
            if b.objects.is_empty() {
                return Err(err("objectives.list.empty", vec![]));
            }
            let mut v = ok("objectives.list.count", vec![json!(b.objects.len())])?;
            for (k, o) in &b.objects {
                v.push(tr(
                    "commands.scoreboard.objectives.list.entry",
                    vec![json!(k), json!(o.label), json!(o.criteria)],
                ))
            }
            Ok(v)
        }
        _ => Err(cmd_usage(a)),
    }
}
fn players(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    let who = a
        .get(2)
        .map(|s| user(ctx, s))
        .transpose()?
        .unwrap_or_default();
    let other = if a.get(1) == Some(&"operation") {
        a.get(5).map(|s| user(ctx, s)).transpose()?
    } else {
        None
    };
    if let [_, op, raw, _, _, tag @ ..] = a {
        if matches!(*op, "set" | "add" | "remove") && !tag.is_empty() {
            let entity = ctx.one(raw)?;
            let pattern = snbt::parse(&tag.join(" "))?;
            if !snbt::matches(&pattern, &entities::entity_nbt(ctx, entity)) {
                return Err(err("players.set.tagMismatch", vec![json!(raw)]));
            }
        }
    }
    let b = &mut ctx.sim.commands.board;
    match a {
        [_, "list"] => {
            let users = b.users();
            ctx.results[4] = users.len() as i32;
            if users.is_empty() {
                return Err(err("players.list.empty", vec![]));
            }
            let mut v = ok("players.list.count", vec![json!(users.len())])?;
            v.push(json!({"text":users.into_iter().collect::<Vec<_>>().join(", ")}));
            Ok(v)
        }
        [_, "list", _] => {
            let entries: Vec<_> = b
                .objects
                .iter()
                .filter_map(|(k, o)| o.scores.get(&who).map(|s| (k, o, s)))
                .collect();
            ctx.results[4] = entries.len() as i32;
            if entries.is_empty() {
                return Err(err("players.list.player.empty", vec![json!(who)]));
            }
            let mut v = ok(
                "players.list.player.count",
                vec![json!(entries.len()), json!(who)],
            )?;
            for (k, o, s) in entries {
                v.push(tr(
                    "commands.scoreboard.players.list.player.entry",
                    vec![json!(s), json!(o.label), json!(k)],
                ))
            }
            Ok(v)
        }
        [_, op, _, obj, n, ..] if matches!(*op, "add" | "remove" | "set") => {
            let o = b.object(obj, true)?;
            let num = integer(n, if *op == "set" { i32::MIN } else { 0 }, i32::MAX)?;
            let s = o.scores.entry(who.clone()).or_default();
            *s = match *op {
                "add" => s.wrapping_add(num),
                "remove" => s.wrapping_sub(num),
                _ => num,
            };
            ok(
                "players.set.success",
                vec![json!(obj), json!(who), json!(s)],
            )
        }
        [_, "reset", _, rest @ ..] if rest.len() <= 1 => {
            if let Some(obj) = rest.first() {
                let o = b.object(obj, false)?;
                o.scores.remove(&who);
                o.unlocked.remove(&who);
                ok("players.resetscore.success", vec![json!(obj), json!(who)])
            } else {
                for o in b.objects.values_mut() {
                    o.scores.remove(&who);
                    o.unlocked.remove(&who);
                }
                ok("players.reset.success", vec![json!(who)])
            }
        }
        [_, "enable", _, obj] => {
            let o = b.object(obj, false)?;
            if o.criteria != "trigger" {
                return Err(err("players.enable.noTrigger", vec![json!(obj)]));
            }
            o.scores.entry(who.clone()).or_default();
            o.unlocked.insert(who.clone());
            ok("players.enable.success", vec![json!(obj), json!(who)])
        }
        [_, "test", _, obj, min, rest @ ..] if rest.len() <= 1 => {
            let o = b.object(obj, false)?;
            let s = o
                .scores
                .get(&who)
                .ok_or_else(|| err("players.test.notFound", vec![json!(obj), json!(who)]))?;
            let lo = if *min == "*" {
                i32::MIN
            } else {
                integer(min, i32::MIN, i32::MAX)?
            };
            let hi = match rest.first() {
                None | Some(&"*") => i32::MAX,
                Some(x) => integer(x, lo, i32::MAX)?,
            };
            let args = vec![json!(s), json!(lo), json!(hi)];
            if (lo..=hi).contains(s) {
                ok("players.test.success", args)
            } else {
                Err(err("players.test.failed", args))
            }
        }
        [_, "operation", _, obj, operation, _, src] => {
            let other = other.unwrap_or_default();
            b.object(obj, true)?;
            b.object(src, false)?;
            let r = b
                .score(&other, src)
                .ok_or_else(|| err("players.operation.notFound", vec![json!(src), json!(other)]))?;
            let left = b.object(obj, true)?.scores.entry(who).or_default();
            let old = *left;
            *left = match *operation {
                "+=" => old.wrapping_add(r),
                "-=" => old.wrapping_sub(r),
                "*=" => old.wrapping_mul(r),
                "/=" if r != 0 => old.wrapping_div(r),
                "%=" if r != 0 => old.wrapping_rem(r),
                "/=" | "%=" => old,
                "=" | "><" => r,
                "<" => old.min(r),
                ">" => old.max(r),
                _ => {
                    return Err(err(
                        "players.operation.invalidOperation",
                        vec![json!(operation)],
                    ))
                }
            };
            if *operation == "><" {
                b.object(src, false)?.scores.insert(other, old);
            }
            ok("players.operation.success", vec![])
        }
        _ => Err(cmd_usage(a)),
    }
}
fn teams(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    let op = a.get(1).copied().unwrap_or("");
    let team = a.get(2).copied().unwrap_or("");
    if matches!(op, "join" | "leave") {
        let first = if op == "join" { 3 } else { 2 };
        if op == "join" && !ctx.sim.commands.board.groups.contains_key(team) {
            return Err(err("teamNotFound", vec![json!(team)]));
        }
        let mut users = BTreeSet::new();
        for s in a.get(first..).unwrap_or(&[]) {
            if s.starts_with('@') {
                for t in ctx.targets(s)? {
                    users.insert(ctx.target_key(t));
                }
            } else {
                users.insert(user(ctx, s)?);
            }
        }
        if a.len() == first {
            users.insert(ctx.target_key(ctx.sender));
        }
        let members = &mut ctx.sim.commands.board.members;
        let mut failed = vec![];
        for s in &users {
            if op == "join" {
                members.insert(s.clone(), team.into());
            } else if members.remove(s).is_none() {
                failed.push(s.clone())
            }
        }
        ctx.results[2] = (users.len() - failed.len()) as i32;
        if !failed.is_empty() {
            return Err(err(
                "teams.leave.failure",
                vec![json!(failed.len()), json!(failed.join(", "))],
            ));
        }
        let mut args = vec![json!(users.len())];
        if op == "join" {
            args.push(json!(team))
        }
        args.push(json!(users.into_iter().collect::<Vec<_>>().join(", ")));
        return ok(&format!("teams.{op}.success"), args);
    }
    let b = &mut ctx.sim.commands.board;
    let members = || {
        b.members
            .iter()
            .filter(|(_, t)| *t == team)
            .map(|(u, _)| u.clone())
            .collect::<Vec<_>>()
    };
    if matches!(op, "empty" | "remove" | "option") || op == "list" && a.len() > 2 {
        if !b.groups.contains_key(team) {
            return Err(err("teamNotFound", vec![json!(team)]));
        }
    }
    match a {
        [_, "add", name, rest @ ..] => {
            if b.groups.contains_key(*name) {
                return Err(err("teams.add.alreadyExists", vec![json!(name)]));
            }
            if name.encode_utf16().count() > 16 {
                return Err(err("teams.add.tooLong", vec![json!(name), json!(16)]));
            }
            let label = if rest.is_empty() {
                name.to_string()
            } else {
                rest.join(" ")
            };
            if label.encode_utf16().count() > 32 {
                return Err(err(
                    "teams.add.displayTooLong",
                    vec![json!(label), json!(32)],
                ));
            }
            b.groups.insert(
                name.to_string(),
                Group {
                    label,
                    flags: 3,
                    color: -1,
                    visibility: "always".into(),
                    death_visibility: "always".into(),
                },
            );
            ok("teams.add.success", vec![json!(name)])
        }
        [_, "remove", _] => {
            b.groups.remove(team);
            b.members.retain(|_, t| t != team);
            ok("teams.remove.success", vec![json!(team)])
        }
        [_, "empty", _] => {
            let n = members().len();
            ctx.results[2] = n as i32;
            if n == 0 {
                return Err(err("teams.empty.alreadyEmpty", vec![json!(team)]));
            }
            b.members.retain(|_, t| t != team);
            ok("teams.empty.success", vec![json!(n), json!(team)])
        }
        [_, "list"] => {
            ctx.results[4] = b.groups.len() as i32;
            if b.groups.is_empty() {
                return Err(err("teams.list.empty", vec![]));
            }
            let mut v = ok("teams.list.count", vec![json!(b.groups.len())])?;
            for (k, g) in &b.groups {
                v.push(tr(
                    "commands.scoreboard.teams.list.entry",
                    vec![
                        json!(k),
                        json!(g.label),
                        json!(b.members.values().filter(|t| *t == k).count()),
                    ],
                ))
            }
            Ok(v)
        }
        [_, "list", _] => {
            let m = members();
            ctx.results[4] = m.len() as i32;
            if m.is_empty() {
                return Err(err("teams.list.player.empty", vec![json!(team)]));
            }
            let mut v = ok("teams.list.player.count", vec![json!(m.len()), json!(team)])?;
            v.push(json!({"text":m.join(", ")}));
            Ok(v)
        }
        [_, "option", _, key, val] => {
            let g = b.groups.get_mut(team).unwrap();
            match key.to_ascii_lowercase().as_str() {
                "color" => {
                    g.color = if val.eq_ignore_ascii_case("reset") {
                        -1
                    } else {
                        COLORS
                            .iter()
                            .position(|c| c.eq_ignore_ascii_case(val))
                            .ok_or_else(|| cmd_usage(a))? as i8
                    }
                }
                "friendlyfire" | "seefriendlyinvisibles" => {
                    let mask = if key.eq_ignore_ascii_case("friendlyfire") {
                        1
                    } else {
                        2
                    };
                    if val.eq_ignore_ascii_case("true") {
                        g.flags |= mask
                    } else if val.eq_ignore_ascii_case("false") {
                        g.flags &= !mask
                    } else {
                        return Err(cmd_usage(a));
                    }
                }
                "nametagvisibility" | "deathmessagevisibility"
                    if ["always", "never", "hideForOtherTeams", "hideForOwnTeam"].contains(val) =>
                {
                    if key.eq_ignore_ascii_case("nametagVisibility") {
                        g.visibility = val.to_string()
                    } else {
                        g.death_visibility = val.to_string()
                    }
                }
                _ => return Err(cmd_usage(a)),
            }
            ok(
                "teams.option.success",
                vec![json!(key), json!(team), json!(val)],
            )
        }
        _ => Err(cmd_usage(a)),
    }
}
pub(super) fn trigger(ctx: &mut Context<'_>, a: &[&str]) -> Reply {
    let [id, op, v, ..] = a else {
        return Err(usage("trigger"));
    };
    if ctx.sender != Target::Player {
        return Err(error("commands.trigger.invalidPlayer", vec![]));
    }
    let who = ctx.options.username.clone();
    let o = ctx
        .sim
        .commands
        .board
        .objects
        .get_mut(*id)
        .filter(|o| o.criteria == "trigger" && o.scores.contains_key(&who))
        .ok_or_else(|| error("commands.trigger.invalidObjective", vec![json!(id)]))?;
    let value = integer(v, i32::MIN, i32::MAX)?;
    if !o.unlocked.contains(&who) {
        return Err(error("commands.trigger.disabled", vec![json!(id)]));
    }
    let s = o.scores.get_mut(&who).unwrap();
    *s = match *op {
        "set" => value,
        "add" => s.wrapping_add(value),
        _ => return Err(error("commands.trigger.invalidMode", vec![json!(op)])),
    };
    o.unlocked.remove(&who);
    sync_board(ctx.sim);
    Ok(if ctx.sim.mode == Mode::Creative {
        vec![tr(
            "commands.trigger.success",
            vec![json!(id), json!(op), json!(v)],
        )]
    } else {
        vec![]
    })
}
pub(super) fn complete(ctx: &Context<'_>, name: &str, a: &[&str]) -> Vec<String> {
    let b = &ctx.sim.commands.board;
    let obj = || b.objects.keys().cloned().collect();
    let team = || b.groups.keys().cloned().collect();
    let users = || {
        let mut u = b.users();
        u.insert(ctx.options.username.clone());
        u.into_iter().collect()
    };
    if name == "trigger" {
        return match a {
            [_] => b
                .objects
                .iter()
                .filter(|(_, o)| o.criteria == "trigger")
                .map(|(k, _)| k.clone())
                .collect(),
            [_, _] => names(["add", "set"]),
            _ => vec![],
        };
    }
    match a {
        [_] => names(["objectives", "players", "teams"]),
        ["objectives", _] => names(["list", "add", "remove", "setdisplay"]),
        ["players", _] => names([
            "set",
            "add",
            "remove",
            "reset",
            "list",
            "enable",
            "test",
            "operation",
        ]),
        ["teams", _] => names(["list", "add", "remove", "empty", "join", "leave", "option"]),
        ["objectives", "add", _, _] => CATALOG.criteria.iter().cloned().collect(),
        ["objectives", "setdisplay", _] => slots(),
        ["objectives", "remove", _] | ["objectives", "setdisplay", _, _] => obj(),
        ["teams", "option", _, _] => names(OPTIONS),
        ["teams", "option", _, "color", _] => names(COLORS),
        ["teams", "option", _, k, _] if k.to_ascii_lowercase().contains("visibility") => {
            names(["always", "never", "hideForOtherTeams", "hideForOwnTeam"])
        }
        ["teams", "option", _, _, _] => names(["true", "false"]),
        ["teams", "leave", ..] | ["teams", "join", _, _, ..] => users(),
        ["teams", _, _] => team(),
        ["players", _, _] | ["players", "operation", _, _, _, _] => users(),
        ["players", _, _, _] | ["players", "operation", _, _, _, _, _] => obj(),
        ["players", "operation", _, _, _] => {
            names(["+=", "-=", "*=", "/=", "%=", "=", "<", ">", "><"])
        }
        _ => vec![],
    }
}
