use super::*;
use crate::world::{Chunk, World};
use clap::Parser;
use glam::Vec3;
use std::sync::Arc;
#[test]
#[ignore = "uses local 1.8.9 extracted resources"]
fn original_score_commands_and_result_bindings() {
    let mut options = Options::parse_from(["ferrum189", "--username", "Player"]);
    options.world_rules.allow_cheats = true;
    let mut world = World {
        chunks: [((0, 0), Arc::new(Chunk::empty()))].into(),
        spawn: Vec3::new(8.5, 42., 8.5),
        label: "command test".into(),
        save_path: Default::default(),
        edits: Default::default(),
        imported: false,
        light_revision: 0,
    };
    let mut player = Player::new(world.spawn);
    let mut sim = Simulation::default();
    let cases: Vec<(String, i32, BTreeMap<String, i32>, String, String)> =
        serde_json::from_str(include_str!("../../tests/fixtures/command_scores189.json")).unwrap();
    let mut ctx = Context::new(
        &mut sim,
        &mut player,
        &mut world,
        crate::validation::resources(),
        &options,
    );
    for (command, success, scores, sidebar, team) in cases {
        let messages = dispatch(&mut ctx, &command);
        assert_eq!(ctx.results[0], success, "{command} {messages:?}");
        let view = &ctx.sim.presentation;
        let actual: BTreeMap<_, _> = view
            .objectives
            .iter()
            .flat_map(|(o, v)| {
                v.scores
                    .iter()
                    .map(move |(who, &n)| (format!("{who}/{o}"), n))
            })
            .collect();
        assert_eq!(actual, scores, "{command}");
        assert_eq!(view.display[1], sidebar, "{command}");
        assert_eq!(
            view.memberships.get("Player").map_or("", String::as_str),
            team,
            "{command}"
        );
    }
    assert_eq!(CATALOG.commands.len(), 44);
    let v = dispatch(&mut ctx, "/? 1");
    assert_eq!(v.len(), 9);
    assert_eq!(v[1]["clickEvent"]["value"], "/achievement ");
    assert_eq!(canonical("msg"), Some("tell"));
    assert_eq!(canonical("w"), Some("tell"));
    dispatch(&mut ctx, "/achievement give achievement.diamonds");
    assert_eq!(ctx.sim.statistics["achievement.mineWood"], 1);
    dispatch(&mut ctx, "/achievement take achievement.mineWood");
    assert!(!ctx.sim.statistics.contains_key("achievement.diamonds"));
    dispatch(&mut ctx, "/scoreboard objectives add gold dummy");
    dispatch(&mut ctx, "/scoreboard players set Player gold 189");
    let loaded: CommandState =
        serde_json::from_value(serde_json::to_value(&ctx.sim.commands).unwrap()).unwrap();
    assert_eq!(loaded.board.score("Player", "gold"), Some(189));
}
#[test]
fn mojangson_widths_lists_and_partial_compounds() {
    use crate::vanilla::drops::DropNbt as T;
    let mut v =
        snbt::parse(r#"{A:{X:3b},Pos:[1.0d,2.0d,3.0d],arr:[1,2,3],CustomName:"a:b"}"#).unwrap();
    assert!(snbt::matches(
        &snbt::parse("{Pos:[3.0d],A:{X:3b}}").unwrap(),
        &v
    ));
    assert!(!snbt::matches(&snbt::parse("{A:{X:3}}").unwrap(), &v));
    snbt::merge(&mut v, snbt::parse("{A:{Y:12s}}").unwrap());
    let T::Compound(tags) = &v else { panic!() };
    assert_eq!(tags["arr"], T::Ints(vec![1, 2, 3]));
    assert_eq!(tags["CustomName"], T::String("a:b".into()));
    assert!(snbt::matches(&snbt::parse("{A:{X:3b,Y:12s}}").unwrap(), &v));
    assert!(snbt::parse("{A:[1}").is_err());
}
