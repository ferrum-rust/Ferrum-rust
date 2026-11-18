//! The integrated server command dispatcher. Remote chat still travels over protocol 47.
use super::{Mode, Simulation};
use crate::{player::Player, resources::Resources, world::World, Options};
use glam::{DVec3, IVec3};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};

#[path = "commands/entities.rs"]
mod entities;
pub(crate) use entities::{read_stack, stack_tag};
#[path = "commands/scoreboard.rs"]
mod scoreboard;
#[path = "commands/selector.rs"]
mod selector;
#[path = "commands/server.rs"]
mod server;
#[cfg(test)]
#[path = "commands/server_tests.rs"]
mod server_tests;
#[path = "commands/snbt.rs"]
mod snbt;
#[path = "commands/world.rs"]
mod world;

type CmdResult<T> = Result<T, Value>;
type Reply = CmdResult<Vec<Value>>;
type Entry = (String, u8, Vec<String>, Vec<usize>);
#[derive(Deserialize)]
struct Catalog {
    entities:BTreeSet<String>,
    commands: BTreeMap<String, Entry>,
    statistics: BTreeMap<String, String>,
    criteria: BTreeSet<String>,
}
static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    serde_json::from_str(include_str!("commands/catalog189.json"))
        .expect("original command registry")
});
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct CommandState {
    pub(crate) player_mode: Option<u8>,
    pub(crate) default_mode: Option<u8>,
    board: scoreboard::Board,
    stat_bindings: BTreeMap<String, BTreeMap<String, (String, String)>>,
    #[serde(skip)]
    debug: Option<Profile>,
}
#[derive(Clone, Debug)]
struct Profile {
    since: std::time::Instant,
    first_tick: u64,
    sections: BTreeMap<&'static str, std::time::Duration>,
    maxima: BTreeMap<&'static str, std::time::Duration>,
    calls: BTreeMap<&'static str, u64>,
    subsections: BTreeMap<&'static str, Subsection>,
}
#[derive(Default, Clone, Debug)]
struct Subsection {
    total: std::time::Duration,
    maximum: std::time::Duration,
    calls: u64,
}
impl Profile {
    fn record(&mut self, label: &'static str, duration: std::time::Duration) {
        *self.sections.entry(label).or_default() += duration;
        let maximum = self.maxima.entry(label).or_default();
        *maximum = (*maximum).max(duration);
        *self.calls.entry(label).or_default() += 1;
    }
    fn record_subsection(&mut self, label: &'static str, duration: std::time::Duration) {
        let sample = self.subsections.entry(label).or_default();
        sample.total += duration;
        sample.maximum = sample.maximum.max(duration);
        sample.calls += 1;
    }
}
impl CommandState {
    pub(crate) fn start_profile(&mut self, total_ticks: u64) {
        self.debug = Some(Profile {
            since: std::time::Instant::now(), first_tick: total_ticks,
            sections: BTreeMap::new(), maxima: BTreeMap::new(), calls: BTreeMap::new(),
            subsections: BTreeMap::new(),
        });
    }
    /// Read without stopping the original /debug session. Section durations
    /// are exclusive consecutive intervals, so their totals can be summed.
    pub(crate) fn profile_report(&self) -> Value {
        let Some(profile) = &self.debug else { return Value::Null; };
        let sections = profile.sections.iter().map(|(&label, duration)| (label,
            json!({"total_ms":duration.as_secs_f64()*1000.,
                "max_ms":profile.maxima[label].as_secs_f64()*1000.,
                "calls":profile.calls[label]}))).collect::<BTreeMap<_,_>>();
        let subsections = profile.subsections.iter().map(|(&label, sample)| (label,
            json!({"total_ms":sample.total.as_secs_f64()*1000.,
                "max_ms":sample.maximum.as_secs_f64()*1000.,"calls":sample.calls})))
            .collect::<BTreeMap<_,_>>();
        json!({"first_tick":profile.first_tick,"elapsed_ms":profile.since.elapsed().as_secs_f64()*1000.,
            "measured_ms":profile.sections.values().map(|d|d.as_secs_f64()*1000.).sum::<f64>(),
            "sections":sections,"subsections":subsections})
    }
    pub(super) fn sample(&self) -> Option<std::time::Instant> {
        self.debug.as_ref().map(|_| std::time::Instant::now())
    }
    pub(super) fn measure(&mut self, sample: &mut Option<std::time::Instant>, label: &'static str) {
        if let (Some(s), Some(profile)) = (sample, self.debug.as_mut()) {
            let now = std::time::Instant::now();
            profile.record(label, now.duration_since(*s));
            *s = now;
        }
    }
    /// A separate consecutive timer inside a top-level section. These samples
    /// are diagnostic detail and never contribute to measured_ms or /debug's
    /// exclusive section percentages. A per-entity sample counts as one call.
    pub(super) fn measure_subsection(&mut self, sample: &mut Option<std::time::Instant>, label: &'static str) {
        if let (Some(s), Some(profile)) = (sample, self.debug.as_mut()) {
            let now = std::time::Instant::now();
            profile.record_subsection(label, now.duration_since(*s));
            *s = now;
        }
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn profile_reports_exclusive_totals_maxima_and_calls_without_stopping() {
        let mut state = CommandState::default();
        assert!(state.sample().is_none());
        assert!(state.profile_report().is_null());
        state.start_profile(189);
        let profile = state.debug.as_mut().unwrap();
        for ms in [3,7,2] { profile.record("entities",Duration::from_millis(ms)); }
        profile.record("randomBlockTicks",Duration::from_millis(5));
        let report = state.profile_report();
        assert_eq!(report["first_tick"],189);
        assert_eq!(report["measured_ms"],17.);
        assert_eq!(report["sections"]["entities"],json!({"total_ms":12.,"max_ms":7.,"calls":3}));
        assert_eq!(report["sections"]["randomBlockTicks"],json!({"total_ms":5.,"max_ms":5.,"calls":1}));
        assert!(state.sample().is_some());
        // Existing /debug text output continues reading the original totals.
        assert_eq!(state.debug.as_ref().unwrap().sections["entities"],Duration::from_millis(12));
        let restored:CommandState=serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
        assert!(restored.profile_report().is_null(),"profiling must never enter saved game state");
        state.start_profile(200);
        assert_eq!(state.profile_report()["first_tick"],200);
        assert_eq!(state.profile_report()["sections"],json!({}));
    }

    #[test]
    fn nested_entity_profile_does_not_inflate_exclusive_totals() {
        let mut state = CommandState::default();
        state.start_profile(189);
        let profile = state.debug.as_mut().unwrap();
        profile.record("entities",Duration::from_millis(12));
        profile.record("randomBlockTicks",Duration::from_millis(5));
        for ms in [3,7] { profile.record_subsection("entities.navigation",Duration::from_millis(ms)); }
        profile.record_subsection("entities.burn",Duration::from_millis(2));
        let report = state.profile_report();
        assert_eq!(report["measured_ms"],17.);
        assert_eq!(report["sections"].as_object().unwrap().len(),2);
        assert_eq!(report["subsections"]["entities.navigation"],
            json!({"total_ms":10.,"max_ms":7.,"calls":2}));
        assert_eq!(report["subsections"]["entities.burn"],
            json!({"total_ms":2.,"max_ms":2.,"calls":1}));
        assert_eq!(state.debug.as_ref().unwrap().sections.values().sum::<Duration>(),Duration::from_millis(17));
        state.start_profile(200);
        assert_eq!(state.profile_report()["subsections"],json!({}));
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Block(IVec3),
    Player,
    Mob(u64),
    Drop(u64),
    Orb(i32),
}
#[derive(Clone, Copy, Default)]
struct StatValues {
    data: [i32; 5],
    written: [bool; 5],
}
impl std::ops::Index<usize> for StatValues {
    type Output = i32;
    fn index(&self, i: usize) -> &i32 {
        &self.data[i]
    }
}
impl std::ops::IndexMut<usize> for StatValues {
    fn index_mut(&mut self, i: usize) -> &mut i32 {
        self.written[i] = true;
        &mut self.data[i]
    }
}
struct Context<'a> {
    sim: &'a mut Simulation,
    player: &'a mut Player,
    world: &'a mut World,
    resources: &'a Resources,
    options: &'a Options,
    sender: Target,
    origin: DVec3,
    changed: Vec<IVec3>,
    sounds: Vec<(String, DVec3, f32, f32)>,
    results: StatValues,
    depth: u8,
    forwarded: Vec<Value>,
    permission: Option<u8>,
    block_messages: Option<super::command_blocks::Messages>,
}
impl<'a> Context<'a> {
    fn new(
        sim: &'a mut Simulation,
        player: &'a mut Player,
        world: &'a mut World,
        resources: &'a Resources,
        options: &'a Options,
    ) -> Self {
        let origin = player.precise_position().into();
        Self {
            sim,
            player,
            world,
            resources,
            options,
            sender: Target::Player,
            origin,
            changed: vec![],
            sounds: vec![],
            results: Default::default(),
            depth: 0,
            forwarded: vec![],
            permission: None,
            block_messages: None,
        }
    }
    fn allowed(&self, name: &str) -> bool {
        if let Some(level) = self.permission { return CATALOG.commands[name].1 <= level; }
        name == "seed"
            || CATALOG.commands[name].1 == 0
            || self.sim.lan.as_ref().is_some_and(|host| host.allow_commands)
            || (self.options.world_rules.allow_cheats
                && !self.options.world_rules.hardcore
                && !self.sim.hardcore)
    }
    fn block_error(&mut self, message: &Value) {
        if let Some(messages) = &mut self.block_messages { messages.error(message); }
    }
    fn block_success(&mut self, name: &str, args: &[&str], replies: &[Value]) {
        // The nested invocation already routed its own output before /execute
        // restored the caller's sender and origin.
        if name == "execute" || self.block_messages.is_none() { return; }
        let sender = self.target_name(self.sender);
        let feedback = self.sim.world_commands.rules.get("commandBlockOutput")
            .is_none_or(|v| v != "false")
            && super::command_blocks::operator(self.sim, self.options);
        let sender_feedback = matches!(self.sender, Target::Block(_))
            || self.sim.send_command_feedback;
        self.block_messages.as_mut().unwrap().success(name, args, replies, &sender,
            &self.options.username, feedback, sender_feedback);
    }
}
fn canonical(name: &str) -> Option<&'static str> {
    if let Some((k, _)) = CATALOG.commands.get_key_value(name) {
        return Some(k);
    }
    CATALOG
        .commands
        .iter()
        .find(|(_, v)| v.2.iter().any(|a| a == name))
        .map(|(k, _)| k.as_str())
}
fn tr(key: &str, args: Vec<Value>) -> Value {
    json!({"translate":key,"with":args})
}
fn error(key: &str, args: Vec<Value>) -> Value {
    let mut v = tr(key, args);
    v["color"] = json!("red");
    v
}
fn usage(name: &str) -> Value {
    let key = CATALOG.commands.get(name).map_or(name, |e| e.0.as_str());
    error("commands.generic.usage", vec![tr(key, vec![])])
}
fn integer(s: &str, min: i32, max: i32) -> CmdResult<i32> {
    let v = s
        .parse::<i32>()
        .map_err(|_| error("commands.generic.num.invalid", vec![json!(s)]))?;
    if v < min {
        return Err(error(
            "commands.generic.num.tooSmall",
            vec![json!(v), json!(min)],
        ));
    }
    if v > max {
        return Err(error(
            "commands.generic.num.tooBig",
            vec![json!(v), json!(max)],
        ));
    }
    Ok(v)
}
fn coordinate(base: f64, s: &str, center: bool) -> CmdResult<f64> {
    let rel = s.strip_prefix('~');
    let raw = rel.unwrap_or(s);
    let value = if rel.is_some() && raw.is_empty() {
        0.
    } else {
        raw.parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .ok_or_else(|| error("commands.generic.num.invalid", vec![json!(s)]))?
    };
    Ok(if rel.is_some() {
        base + value
    } else if center && !raw.contains('.') {
        value + 0.5
    } else {
        value
    })
}
fn names<'a>(items: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    items.into_iter().map(str::to_owned).collect()
}
fn entity_command(n: &str) -> bool {
    matches!(
        n,
        "give"
            | "clear"
            | "xp"
            | "effect"
            | "enchant"
            | "gamemode"
            | "kill"
            | "tp"
            | "spawnpoint"
            | "spreadplayers"
            | "summon"
            | "replaceitem"
            | "entitydata"
            | "tellraw"
            | "title"
            | "say"
            | "tell"
            | "me"
    )
}
fn world_command(n: &str) -> bool {
    matches!(
        n,
        "setblock"
            | "fill"
            | "clone"
            | "blockdata"
            | "testforblock"
            | "testforblocks"
            | "setworldspawn"
            | "time"
            | "weather"
            | "toggledownfall"
            | "difficulty"
            | "gamerule"
            | "worldborder"
            | "particle"
            | "playsound"
            | "seed"
    )
}
fn username_index(n: &str, a: &[&str], i: usize) -> bool {
    match n {
        "replaceitem" => i == 1 && a.first() == Some(&"entity"),
        "stats" => i == 1 && a.first() == Some(&"entity"),
        "scoreboard" => scoreboard::username_index(a, i),
        _ => CATALOG.commands[n].3.contains(&i),
    }
}
fn summon_vehicle(ctx: &mut Context<'_>, a: &[&str]) -> Option<Reply> {
    use super::vehicles::{LocalBoat, LocalMinecart, MinecartKind};
    use crate::vanilla::{drops::DropNbt as Tag, item_nbt};
    let name = *a.first()?;
    let base_kind = match name {
        "Boat" => None,
        "Minecart" | "MinecartRideable" => Some(MinecartKind::Rideable),
        "MinecartChest" => Some(MinecartKind::Chest),
        "MinecartFurnace" => Some(MinecartKind::Furnace),
        "MinecartTNT" => Some(MinecartKind::Tnt),
        "MinecartHopper" => Some(MinecartKind::Hopper),
        _ => return None,
    };
    Some((|| {
        let p = if a.len() >= 4 {
            DVec3::new(
                coordinate(ctx.origin.x, a[1], true)?,
                coordinate(ctx.origin.y, a[2], false)?,
                coordinate(ctx.origin.z, a[3], true)?,
            )
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
        let tag = if a.len() > 4 {
            Some(snbt::parse(&a[4..].join(" ")).map_err(|e| {
                error(
                    "commands.summon.tagError",
                    e["with"].as_array().cloned().unwrap_or_default(),
                )
            })?)
        } else {
            None
        };
        let map = tag.as_ref().and_then(item_nbt::compound);
        let tag_f64 = |t: &Tag| -> Option<f64> {
            match t {
                Tag::Byte(v) => Some(f64::from(*v)),
                Tag::Short(v) => Some(f64::from(*v)),
                Tag::Int(v) => Some(f64::from(*v)),
                Tag::Long(v) => Some(*v as f64),
                Tag::Float(v) => Some(f64::from(*v)),
                Tag::Double(v) => Some(*v),
                _ => None,
            }
            .filter(|n| n.is_finite())
        };
        let motion = map.and_then(|m| match m.get("Motion") {
            Some(Tag::List { values, .. }) if values.len() >= 3 => Some(DVec3::new(
                tag_f64(&values[0])?,
                tag_f64(&values[1])?,
                tag_f64(&values[2])?,
            )),
            _ => None,
        });
        let rotation = map.and_then(|m| match m.get("Rotation") {
            Some(Tag::List { values, .. }) if values.len() >= 2 => {
                Some([tag_f64(&values[0])? as f32, tag_f64(&values[1])? as f32])
            }
            _ => None,
        });
        ctx.sim.next_id = ctx.sim.next_id.wrapping_add(1).max(1);
        while ctx.sim.minecarts.iter().any(|c| c.id == ctx.sim.next_id)
            || ctx.sim.boats.iter().any(|b| b.id == ctx.sim.next_id)
            || ctx.sim.mobs.iter().any(|m| m.id == ctx.sim.next_id)
        {
            ctx.sim.next_id = ctx.sim.next_id.wrapping_add(1).max(1);
        }
        let id = ctx.sim.next_id;
        if let Some(mut kind) = base_kind {
            if name == "Minecart" {
                if let Some(m) = map {
                    if m.contains_key("Type") {
                        kind = match item_nbt::number(m.get("Type")) {
                            1 => MinecartKind::Chest,
                            2 => MinecartKind::Furnace,
                            3 => MinecartKind::Tnt,
                            5 => MinecartKind::Hopper,
                            _ => MinecartKind::Rideable,
                        };
                    }
                }
            }
            let mut spawn_pos = p;
            let rstate = ctx.world.get(b);
            if crate::vanilla::rail_events::is_rail(rstate)
                && (spawn_pos.y - f64::from(b.y)).abs() < 1e-6
            {
                let raw = if rstate >> 4 == 66 {
                    rstate & 15
                } else {
                    rstate & 7
                };
                let lift = if (2..=5).contains(&raw) { 0.5 } else { 0.0 };
                spawn_pos.y = f64::from(b.y) + 0.0625 + lift;
            }
            let mut cart = LocalMinecart::new(id, kind, spawn_pos);
            if let Some(v) = motion {
                cart.velocity = v;
            }
            if let Some([yaw, pitch]) = rotation {
                cart.yaw = yaw;
                cart.prev_yaw = yaw;
                cart.pitch = pitch;
                cart.prev_pitch = pitch;
            }
            if let Some(m) = map {
                if let Some(Tag::String(s)) = m.get("CustomName") {
                    if !s.is_empty() {
                        cart.custom_name = Some(s.clone());
                    }
                }
                if m.contains_key("Fuel") {
                    cart.fuel = item_nbt::number(m.get("Fuel")).clamp(0, 32000);
                }
                if m.contains_key("TNTFuse") {
                    cart.fuse = item_nbt::number(m.get("TNTFuse"));
                }
                if let Some(Tag::List { values, .. }) = m.get("Items") {
                    for entry in values {
                        if let Some(im) = item_nbt::compound(entry) {
                            let slot = item_nbt::number(im.get("Slot")) as usize;
                            if slot < cart.slots.len() {
                                cart.slots[slot] = read_stack(entry);
                            }
                        }
                    }
                }
            }
            ctx.sim.minecarts.push(cart);
            if rstate >> 4 == 28 {
                let mut out = super::Outcome::default();
                ctx.sim.update_detector_rail(ctx.world, b, &mut out);
                ctx.changed.extend(out.changed);
            }
        } else {
            let yaw = rotation.map_or(0.0, |r| r[0]);
            let mut boat = LocalBoat::new(id, p, yaw);
            if let Some(v) = motion {
                boat.velocity = v;
            }
            if let Some([_, pitch]) = rotation {
                boat.pitch = pitch;
                boat.prev_pitch = pitch;
            }
            ctx.sim.boats.push(boat);
        }
        Ok(if ctx.permission.is_some() || ctx.sim.send_command_feedback {
            vec![tr("commands.summon.success", vec![])]
        } else {
            vec![]
        })
    })())
}
fn dispatch(ctx: &mut Context<'_>, raw: &str) -> Vec<Value> {
    let text = raw.trim().strip_prefix('/').unwrap_or(raw.trim());
    let mut words = text.split(' ');
    let command = words.next().unwrap_or("");
    ctx.results = StatValues::default();
    let Some(n) = canonical(command) else {
        ctx.results[0] = 0;
        server::apply_stats(ctx);
        let e = error("commands.generic.notFound", vec![]);
        ctx.block_error(&e);
        return vec![e];
    };
    if !ctx.allowed(n) {
        ctx.results[0] = 0;
        server::apply_stats(ctx);
        let e = error("commands.generic.permission", vec![]);
        ctx.block_error(&e);
        return vec![e];
    }
    let mut args: Vec<_> = words.collect();
    let mut replies = vec![];
    let index = args
        .iter()
        .enumerate()
        .find(|(i, a)| username_index(n, &args, *i) && selector::multiple(a))
        .map(|(i, _)| i);
    let targets = if let Some(i) = index {
        match ctx.targets(args[i]) {
            Ok(v) => v.into_iter().map(|t| ctx.target_key(t)).collect(),
            Err(e) => {
                ctx.results[0] = 0;
                ctx.block_error(&e);
                return vec![e];
            }
        }
    } else {
        vec![String::new()]
    };
    let mut success = 0;
    ctx.results[2] = targets.len() as i32;
    for target in &targets {
        if let Some(i) = index {
            args[i] = target
        }
        let result = if n == "summon" {
            if let Some(res) = summon_vehicle(ctx, &args) {
                res
            } else {
                entities::run(ctx, n, &args)
            }
        } else if entity_command(n) {
            entities::run(ctx, n, &args)
        } else if world_command(n) {
            world::run(ctx, n, &args)
        } else {
            server::run(ctx, n, &args)
        };
        match result {
            Ok(v) => {
                success += 1;
                ctx.block_success(n, &args, &v);
                if ctx.permission.is_some() || ctx.sim.send_command_feedback
                    || entity_command(n)
                    || matches!(n, "help" | "seed" | "execute")
                    || (n == "scoreboard" && args.get(1) == Some(&"list"))
                    || (n == "gamerule" && args.len() <= 1)
                {
                    replies.extend(v)
                }
            }
            Err(e) => {
                ctx.block_error(&e);
                replies.append(&mut ctx.forwarded);
                replies.push(e)
            }
        }
    }
    ctx.results[0] = success;
    server::apply_stats(ctx);
    replies
}
pub(crate) struct Output {
    pub messages: Vec<Value>,
    pub changed: Vec<IVec3>,
    pub sounds: Vec<(String, DVec3, f32, f32)>,
}
pub(crate) fn execute(
    sim: &mut Simulation,
    player: &mut Player,
    world: &mut World,
    resources: &Resources,
    options: &Options,
    input: &str,
) -> Output {
    let mut ctx = Context::new(sim, player, world, resources, options);
    let messages = dispatch(&mut ctx, input);
    Output {
        messages,
        changed: ctx.changed,
        sounds: ctx.sounds,
    }
}
pub(crate) fn execute_block(
    sim: &mut Simulation, player: &mut Player, world: &mut World,
    resources: &Resources, options: &Options, position: IVec3, input: &str,
) -> (Output, i32, Option<Value>) {
    let mut ctx = Context::new(sim, player, world, resources, options);
    ctx.sender = Target::Block(position);
    ctx.origin = position.as_dvec3() + DVec3::splat(0.5);
    ctx.permission = Some(2);
    ctx.block_messages = Some(Default::default());
    dispatch(&mut ctx, input);
    let success = ctx.results[0];
    let messages = ctx.block_messages.take().unwrap();
    (Output { messages: messages.chat, changed: ctx.changed, sounds: ctx.sounds }, success, messages.last)
}
fn candidates(ctx: &Context<'_>, input: &str) -> Vec<String> {
    let mut words = input.split(' ');
    let first = words.next().unwrap_or("");
    let args: Vec<_> = words.collect();
    if args.is_empty() {
        return CATALOG
            .commands
            .iter()
            .filter(|(n, _)| ctx.allowed(n))
            .flat_map(|(n, e)| std::iter::once(n).chain(e.2.iter()))
            .cloned()
            .collect();
    }
    let Some(n) = canonical(first) else {
        return vec![];
    };
    if !ctx.allowed(n) {
        return vec![];
    }
    if entity_command(n) {
        let mut list = entities::complete(ctx, n, &args);
        if n == "summon" && args.len() == 1 {
            list.extend(
                [
                    "Boat",
                    "MinecartRideable",
                    "MinecartChest",
                    "MinecartFurnace",
                    "MinecartTNT",
                    "MinecartHopper",
                ]
                .map(String::from),
            );
        }
        list
    } else if world_command(n) {
        world::complete(ctx, n, &args)
    } else {
        server::complete(ctx, n, &args)
    }
}
pub(crate) fn completions(
    sim: &mut Simulation,
    player: &mut Player,
    world: &mut World,
    resources: &Resources,
    options: &Options,
    input: &str,
) -> Vec<String> {
    let ctx = Context::new(sim, player, world, resources, options);
    let prefix = input.split(' ').next_back().unwrap_or("").to_lowercase();
    let mut choices = if let Some(input) = input.strip_prefix('/') {
        let mut words = candidates(&ctx, input);
        if !input.contains(' ') {
            for w in &mut words {
                w.insert(0, '/')
            }
        }
        words
    } else {
        vec![options.username.clone()]
    };
    let matches = |s: &str| s.to_lowercase().starts_with(&prefix);
    if choices.iter().any(|s| matches(s)) {
        choices.retain(|s| matches(s))
    } else {
        choices.retain(|s| s.strip_prefix("minecraft:").is_some_and(matches))
    }
    choices.sort_unstable();
    choices.dedup();
    choices
}
pub(crate) fn sync_board(sim: &mut Simulation) {
    sim.commands.board.present(&mut sim.presentation)
}
pub(crate) fn award_pickup(stats:&mut std::collections::HashMap<String,i32>,key:&str) {
    if let Some(parent)=CATALOG.statistics.get(key) {
        if parent.is_empty() || stats.get(parent).copied().unwrap_or(0)>0 {
            let value=stats.entry(key.to_owned()).or_default();
            *value=value.wrapping_add(1);
        }
    }
}
pub(super) fn tick_scores(sim: &mut Simulation) {
    scoreboard::observe(sim)
}
