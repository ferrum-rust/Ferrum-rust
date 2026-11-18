//! Original 1.8.9 impulse command blocks: editable Control tiles and level-2
//! command senders. Redstone scheduling lives with the other circuit timers.
use super::{local_commands, Mode, Outcome, Simulation};
use crate::{
    player::Player,
    resources::Resources,
    vanilla::{drops::DropNbt as N, item_nbt},
    world::World,
    Options,
};
use glam::IVec3;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct State {
    pub command: String,
    pub track: bool,
    pub output: Option<String>,
    pub success: i32,
}

/// CommandBlockLogic.addChatMessage writes only the sender's last response.
/// Broadcasts and operator notifications have separate recipients in the
/// original server; keeping them separate also preserves nested /execute.
#[derive(Default)]
pub(crate) struct Messages {
    pub chat: Vec<Value>,
    pub last: Option<Value>,
}
impl Messages {
    pub fn error(&mut self, message: &Value) {
        self.last = Some(message.clone());
    }
    pub fn success(
        &mut self,
        command: &str,
        args: &[&str],
        messages: &[Value],
        sender: &str,
        recipient: &str,
        feedback: bool,
        sender_feedback: bool,
    ) {
        for message in messages {
            let key = message
                .get("translate")
                .and_then(Value::as_str)
                .unwrap_or("");
            if matches!(command, "say" | "me" | "tellraw") || key == "gameMode.changed" {
                self.chat.push(message.clone());
                continue;
            }
            if command == "tell" {
                self.chat.push(message.clone());
                let mut outgoing = message.clone();
                outgoing["translate"] = json!("commands.message.display.outgoing");
                outgoing["with"][0] = json!(recipient);
                self.last = Some(outgoing);
                continue;
            }
            let sender_only = matches!(command, "help" | "seed")
                || command == "gamerule" && args.len() <= 1
                || command == "scoreboard" && args.get(1) == Some(&"list")
                || key == "commands.clear.testing"
                || key == "commands.worldborder.get.success"
                || key.starts_with("commands.spreadplayers.spreading.")
                || key.starts_with("commands.spreadplayers.info.");
            if sender_only {
                self.last = Some(message.clone());
                continue;
            }
            // CommandGameMode calls notifyOperators with flag 1: its target
            // gets gameMode.changed, while the sender gets no success text.
            if sender_feedback && command != "gamemode" {
                self.last = Some(message.clone());
            }
            if feedback {
                self.chat.push(json!({"translate":"chat.type.admin",
                    "with":[sender,message],"color":"gray","italic":true}));
            }
        }
    }
}

fn tag(world: &World, p: IVec3) -> BTreeMap<String, N> {
    world
        .tile(p)
        .and_then(item_nbt::compound)
        .cloned()
        .unwrap_or_else(|| {
            BTreeMap::from([
                ("id".into(), N::String("Control".into())),
                ("x".into(), N::Int(p.x)),
                ("y".into(), N::Int(p.y)),
                ("z".into(), N::Int(p.z)),
                ("Command".into(), N::String(String::new())),
                ("CustomName".into(), N::String("@".into())),
                ("TrackOutput".into(), N::Byte(1)),
                ("SuccessCount".into(), N::Int(0)),
            ])
        })
}
pub(crate) fn read(world: &World, p: IVec3) -> State {
    let t = tag(world, p);
    let text = |key: &str| match t.get(key) {
        Some(N::String(s)) => Some(s.clone()),
        _ => None,
    };
    State {
        command: text("Command").unwrap_or_default(),
        track: t
            .get("TrackOutput")
            .is_none_or(|n| item_nbt::number(Some(n)) != 0),
        output: text("LastOutput"),
        success: item_nbt::number(t.get("SuccessCount")) as i32,
    }
}
fn store(sim: &mut Simulation, world: &mut World, p: IVec3, t: BTreeMap<String, N>) {
    if world.get(p) >> 4 == 137 {
        sim.world_commands
            .edit_tile(world, p, 137 << 4, Some(N::Compound(t)));
    }
}
pub(crate) fn operator(sim: &Simulation, options: &Options) -> bool {
    sim.lan.as_ref().is_some_and(|l| l.allow_commands)
        || options.world_rules.allow_cheats && !options.world_rules.hardcore && !sim.hardcore
}
pub(crate) fn save(
    sim: &mut Simulation,
    world: &mut World,
    options: &Options,
    p: IVec3,
    command: &str,
    track: bool,
) -> bool {
    if world.get(p) >> 4 != 137
        || sim.mode != Mode::Creative
        || sim.commands.player_mode == Some(3)
        || !operator(sim, options)
        || command.encode_utf16().count() > 32767
    {
        return false;
    }
    let mut t = tag(world, p);
    t.insert("Command".into(), N::String(command.into()));
    t.insert("SuccessCount".into(), N::Int(0));
    t.insert("TrackOutput".into(), N::Byte(i8::from(track)));
    if !track {
        t.remove("LastOutput");
    }
    store(sim, world, p, t);
    true
}
pub(crate) fn placed(
    sim: &mut Simulation,
    world: &mut World,
    p: IVec3,
    item: &crate::inventory::Stack,
) {
    let mut t = tag(world, p);
    t.insert(
        "TrackOutput".into(),
        N::Byte(i8::from(sim.send_command_feedback)),
    );
    if let Some(name) = item
        .nbt
        .as_deref()
        .and_then(item_nbt::compound)
        .and_then(|m| m.get("display"))
        .and_then(item_nbt::compound)
        .and_then(|m| m.get("Name"))
    {
        if let N::String(name) = name {
            t.insert("CustomName".into(), N::String(name.clone()));
        }
    }
    store(sim, world, p, t);
}

pub(crate) fn execute(
    sim: &mut Simulation,
    player: &mut Player,
    world: &mut World,
    resources: &Resources,
    options: &Options,
    p: IVec3,
) -> local_commands::Output {
    if world.get(p) >> 4 != 137 {
        return local_commands::Output {
            messages: Vec::new(),
            changed: Vec::new(),
            sounds: Vec::new(),
        };
    }
    let state = read(world, p);
    let (mut out, success, last) =
        local_commands::execute_block(sim, player, world, resources, options, p, &state.command);
    // Commands can destroy or replace their own block; never resurrect its tile.
    if world.get(p) >> 4 == 137 {
        let mut t = tag(world, p);
        t.insert("SuccessCount".into(), N::Int(success));
        t.remove("LastOutput");
        if state.track {
            if let Some(message) = last {
                let output = json!({"text":format!("[{}] ", clock()),"extra":[message]});
                t.insert("LastOutput".into(), N::String(output.to_string()));
            }
        }
        store(sim, world, p, t);
        out.changed.push(p);
    }
    // Commands can change distant power sources (or remove themselves).
    // Notify all changed blocks as well as this block's comparator output.
    let mut circuit = Outcome::default();
    sim.update_circuits(world, player, out.changed.clone(), &mut circuit);
    out.changed.extend(circuit.changed);
    out
}

fn clock() -> String {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetLocalTime(time: *mut [u16; 8]);
        }
        let mut t = [0u16; 8];
        unsafe {
            GetLocalTime(&mut t);
        }
        format!("{:02}:{:02}:{:02}", t[4], t[5], t[6])
    }
    #[cfg(not(windows))]
    {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!(
            "{:02}:{:02}:{:02}",
            seconds / 3600 % 24,
            seconds / 60 % 60,
            seconds % 60
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{inventory::Stack, world::Chunk};
    use clap::Parser;
    use glam::Vec3;
    use std::sync::Arc;
    fn setup() -> (World, Simulation, Player, Options, IVec3) {
        let p = IVec3::new(8, 64, 8);
        let mut world = World {
            chunks: crate::world::ChunkMap::from([((0, 0), Arc::new(Chunk::empty()))]),
            spawn: Vec3::new(2., 64., 2.),
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        };
        world.set(p, 137 << 4);
        let player = Player::new(world.spawn);
        let mut options = Options::parse_from(["ferrum189"]);
        options.world_rules.allow_cheats = true;
        (
            world,
            Simulation::new(Mode::Creative, 189),
            player,
            options,
            p,
        )
    }
    #[test]
    fn original_command_block_edges_delay_and_serialized_pending_ticks_match() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-command-block.json"
        ))
        .unwrap();
        for (case, steps) in fixture["edges"].as_array().unwrap().iter().enumerate() {
            let (mut world, mut sim, mut player, _, p) = setup();
            let mut executions = 0;
            for (tick, row) in steps.as_array().unwrap().iter().enumerate() {
                let powered = row[0].as_bool().unwrap();
                sim.total_ticks = tick as u64;
                world.set(p + IVec3::X, if powered { 152 << 4 } else { 0 });
                let mut out = Outcome::default();
                sim.update_circuits(&mut world, &mut player, vec![p + IVec3::X], &mut out);
                executions += out.command_blocks.len();
                let state = serde_json::to_value(&sim.redstone).unwrap();
                let pending = state["timers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|t| t["block"] == 137)
                    .map(|t| t["at"].clone())
                    .collect::<Vec<_>>();
                assert_eq!(
                    json!([powered, world.get(p) & 1, pending, executions]),
                    *row,
                    "case {case}, tick {tick}"
                );
                if tick == 2 {
                    sim = serde_json::from_slice(&serde_json::to_vec(&sim).unwrap()).unwrap();
                }
            }
        }
    }
    #[test]
    fn original_command_block_save_defaults_placement_and_permissions() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-command-block.json"
        ))
        .unwrap();
        let (mut world, mut sim, _, mut options, p) = setup();
        let mut t = tag(&world, p);
        t.insert("SuccessCount".into(), N::Int(7));
        store(&mut sim, &mut world, p, t);
        assert!(save(&mut sim, &mut world, &options, p, "say hello", true));
        let saved = read(&world, p);
        assert_eq!(
            json!([
                saved.command,
                saved.success,
                tag(&world, p)["CustomName"],
                saved.track
            ]),
            json!([
                fixture["save"][0],
                fixture["save"][1],
                N::String(fixture["save"][2].as_str().unwrap().into()),
                fixture["save"][3]
            ])
        );
        options.world_rules.allow_cheats = false;
        assert!(!save(&mut sim, &mut world, &options, p, "kill @p", true));
        options.world_rules.allow_cheats = true;
        sim.mode = Mode::Survival;
        assert!(!save(&mut sim, &mut world, &options, p, "kill @p", true));
        sim.mode = Mode::Creative;
        sim.send_command_feedback = false;
        let mut item = Stack::new(137, 1);
        item.nbt = Some(Arc::new(N::Compound(BTreeMap::from([(
            "display".into(),
            N::Compound(BTreeMap::from([(
                "Name".into(),
                N::String("Controller".into()),
            )])),
        )]))));
        placed(&mut sim, &mut world, p, &item);
        let name = match &tag(&world, p)["CustomName"] {
            N::String(s) => s.clone(),
            _ => panic!(),
        };
        assert_eq!(json!([name, read(&world, p).track]), fixture["placement"]);
    }
    #[test]
    #[ignore = "uses installed 1.8.9 resources for the integrated command dispatcher"]
    fn command_block_sender_relative_commands_output_and_world_reload() {
        let (mut world, mut sim, mut player, mut options, p) = setup();
        let resources = crate::validation::resources();
        assert!(save(
            &mut sim,
            &mut world,
            &options,
            p,
            "setblock ~ ~1 ~ gold_block",
            true
        ));
        options.world_rules.allow_cheats = false;
        let _ = execute(&mut sim, &mut player, &mut world, resources, &options, p);
        assert_eq!(world.get(p + IVec3::Y), 41 << 4);
        assert_eq!(read(&world, p).success, 1);
        assert!(read(&world, p).output.is_some());
        options.world_rules.allow_cheats = true;
        assert!(save(&mut sim, &mut world, &options, p, "say hello", true));
        let out = execute(&mut sim, &mut player, &mut world, resources, &options, p);
        assert_eq!(out.messages[0]["with"][0], "@");
        assert!(save(&mut sim, &mut world, &options, p, "kill", true));
        let health = sim.health;
        execute(&mut sim, &mut player, &mut world, resources, &options, p);
        assert_eq!(sim.health, health);
        assert_eq!(read(&world, p).success, 0);
        assert!(save(
            &mut sim,
            &mut world,
            &options,
            p,
            "give @p diamond 2",
            false
        ));
        execute(&mut sim, &mut player, &mut world, resources, &options, p);
        assert_eq!(
            sim.inventory
                .slots
                .iter()
                .flatten()
                .filter(|s| s.id == 264)
                .map(|s| s.count as u32)
                .sum::<u32>(),
            2
        );
        assert_eq!(read(&world, p).success, 1);
        assert!(read(&world, p).output.is_none());
        let resumed: Simulation =
            serde_json::from_slice(&serde_json::to_vec(&sim).unwrap()).unwrap();
        Arc::make_mut(world.chunks.get_mut(&(0, 0)).unwrap())
            .tiles
            .clear();
        resumed.world_commands.restore_tiles(&mut world, &[(0, 0)]);
        assert_eq!(read(&world, p).command, "give @p diamond 2");
        assert!(!read(&world, p).track);
    }

    #[test]
    #[ignore = "uses installed 1.8.9 resources for original command output routing"]
    fn command_block_distinguishes_sender_broadcast_and_operator_messages() {
        let (mut world, mut sim, mut player, options, p) = setup();
        let resources = crate::validation::resources();
        let mut tile = tag(&world, p);
        tile.insert("CustomName".into(), N::String("Controller".into()));
        store(&mut sim, &mut world, p, tile);
        sim.send_command_feedback = false;
        assert!(save(
            &mut sim,
            &mut world,
            &options,
            p,
            "give @p diamond 1",
            true
        ));
        let out = execute(&mut sim, &mut player, &mut world, resources, &options, p);
        // ServerCommandManager uses commandBlockOutput for operators and
        // TrackOutput for the real command-block sender, independently of
        // sendCommandFeedback. The latter applies to /execute's wrapper.
        assert_eq!(out.messages.len(), 1);
        assert_eq!(out.messages[0]["translate"], "chat.type.admin");
        assert_eq!(out.messages[0]["with"][0], "Controller");
        assert_eq!(
            out.messages[0]["with"][1]["translate"],
            "commands.give.success"
        );
        assert_eq!(out.messages[0]["color"], "gray");
        assert_eq!(out.messages[0]["italic"], true);
        let last: Value = serde_json::from_str(&read(&world, p).output.unwrap()).unwrap();
        assert_eq!(last["extra"][0]["translate"], "commands.give.success");
        assert!(save(
            &mut sim,
            &mut world,
            &options,
            p,
            "execute @p ~ ~ ~ give @p diamond 1",
            true
        ));
        let out = execute(&mut sim, &mut player, &mut world, resources, &options, p);
        assert_eq!(out.messages[0]["with"][0], options.username);
        assert!(read(&world, p).output.is_none());

        sim.world_commands
            .rules
            .insert("commandBlockOutput".into(), "false".into());
        for command in [
            "say hello",
            "execute @p ~ ~ ~ say hello",
            "tellraw @p {\"translate\":\"commands.generic.permission\",\"color\":\"red\"}",
        ] {
            assert!(save(&mut sim, &mut world, &options, p, command, true));
            let out = execute(&mut sim, &mut player, &mut world, resources, &options, p);
            assert_eq!(out.messages.len(), 1, "{command}");
            assert!(
                read(&world, p).output.is_none(),
                "broadcast leaked into sender output: {command}"
            );
        }
        assert!(save(
            &mut sim,
            &mut world,
            &options,
            p,
            "tell @p hello",
            true
        ));
        let out = execute(&mut sim, &mut player, &mut world, resources, &options, p);
        assert_eq!(
            out.messages[0]["translate"],
            "commands.message.display.incoming"
        );
        let last: Value = serde_json::from_str(&read(&world, p).output.unwrap()).unwrap();
        assert_eq!(
            last["extra"][0]["translate"],
            "commands.message.display.outgoing"
        );
        assert_eq!(last["extra"][0]["with"][0], options.username);

        sim.world_commands
            .rules
            .insert("commandBlockOutput".into(), "true".into());
        for (command, expected) in [
            ("missing_command", "commands.generic.notFound"),
            ("debug start", "commands.generic.permission"),
            (
                "execute @p ~ ~ ~ missing_command",
                "commands.execute.failed",
            ),
            ("seed", "commands.seed.success"),
        ] {
            assert!(save(&mut sim, &mut world, &options, p, command, true));
            let out = execute(&mut sim, &mut player, &mut world, resources, &options, p);
            assert!(
                out.messages.is_empty(),
                "private sender response leaked into chat: {command}"
            );
            let last: Value = serde_json::from_str(&read(&world, p).output.unwrap()).unwrap();
            assert_eq!(last["extra"][0]["translate"], expected, "{command}");
        }
    }

    #[test]
    #[ignore = "uses installed 1.8.9 resources for command-to-redstone integration"]
    fn command_block_changed_blocks_notify_redstone_even_after_self_replacement() {
        let resources = crate::validation::resources();
        for command in [
            "setblock ~4 ~ ~ redstone_block",
            "fill ~ ~ ~ ~4 ~ ~ redstone_block",
        ] {
            let (mut world, mut sim, mut player, options, p) = setup();
            let next = p + IVec3::X * 5;
            world.set(next, 137 << 4);
            sim.total_ticks = 42;
            assert!(save(&mut sim, &mut world, &options, p, command, true));
            execute(&mut sim, &mut player, &mut world, resources, &options, p);
            assert_eq!(world.get(next) & 1, 1, "{command}");
            let mut out = Outcome::default();
            sim.update_circuits(&mut world, &mut player, vec![], &mut out);
            assert!(out.command_blocks.is_empty());
            sim.total_ticks += 1;
            sim.update_circuits(&mut world, &mut player, vec![], &mut out);
            assert_eq!(out.command_blocks, vec![next], "{command}");
            if command.starts_with("fill") {
                assert_eq!(world.get(p), 152 << 4);
                assert!(
                    world.tile(p).is_none(),
                    "execution resurrected the destroyed Control tile"
                );
            }
        }
    }
}
