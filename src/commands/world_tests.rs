use super::*;
use crate::{
    player::Player,
    world::{Chunk, World},
    Options,
};
use clap::Parser;
use glam::Vec3;
use std::sync::Arc;

#[test]
#[ignore = "requires imported 1.8.9 resources"]
fn original_world_commands_via_chat_dispatcher() {
    let cases: Vec<(String, String, Vec<u16>, u64)> = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-world-commands.json"
    ))
    .unwrap();
    let res = crate::validation::resources();
    let mut options = Options::parse_from(["ferrum189", "--seed", "189"]);
    options.world_rules.allow_cheats = true;
    let mut sim = Simulation::default();
    sim.clock = 0;
    sim.total_ticks = 0;
    let mut world = World {
        chunks: (0..=1)
            .flat_map(|x| (0..=1).map(move |z| ((x, z), Arc::new(Chunk::empty()))))
            .collect(),
        spawn: Vec3::new(8.5, 42., 8.5),
        edits: Default::default(),
        label: "commands".into(),
        save_path: Default::default(),
        imported: false,
        light_revision: 0,
    };
    let mut player = Player::new(world.spawn);
    for (cmd, message, states, time) in cases {
        let out = super::super::execute(
            &mut sim,
            &mut player,
            &mut world,
            res,
            &options,
            &format!("/{cmd}"),
        );
        let reply = out
            .messages
            .last()
            .unwrap_or_else(|| panic!("{cmd} supplied no response"));
        assert_eq!(
            reply
                .get("translate")
                .or_else(|| reply.get("text"))
                .and_then(Value::as_str),
            Some(message.as_str()),
            "{cmd}: {reply}"
        );
        let mut i = 0;
        for z in 8..=10 {
            for y in 42..=44 {
                for x in 8..=14 {
                    assert_eq!(
                        world.get(IVec3::new(x, y, z)),
                        states[i],
                        "{cmd} at ({x},{y},{z})"
                    );
                    i += 1;
                }
            }
        }
        assert_eq!(time, sim.clock, "{cmd}");
    }
    let stored = serde_json::to_vec(&sim).unwrap();
    let sim: Simulation = serde_json::from_slice(&stored).unwrap();
    assert_eq!(sim.world_commands.spawn, Some([8, 42, 7]));
    assert_eq!(sim.world_difficulty, Some(3));
    assert_eq!(sim.world_commands.rules["custom"], "hello world");
    assert_eq!(sim.world_commands.border_buffer, 2.);
}
