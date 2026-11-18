use super::*;
use crate::{gameplay::Mode, world::Chunk};
use glam::Vec3;
use serde_json::json;
use winit::keyboard::KeyCode;

fn state() -> LocalTickState {
    let mut chunk = Chunk::empty();
    for x in 0..16 { for z in 0..16 { chunk.set(x, 0, z, 1 << 4); } }
    let world = World { chunks: crate::world::ChunkMap::from([((0, 0), Arc::new(chunk))]),
        spawn: Vec3::new(8.5, 1., 8.5), label: "local tick ownership".into(),
        save_path: Default::default(), edits: Default::default(), imported: false, light_revision: 0 };
    let mut player = Player::new(world.spawn);
    player.set_pose_f64([8.5, 1., 8.5], 0., 0., true);
    let mut sim = Simulation::default();
    sim.mode = Mode::Creative;
    sim.mob_spawning = false;
    sim.keep_inventory = true;
    sim.local_name = "ThreadFixture".into();
    LocalTickState { world, player, sim }
}

#[test]
fn local_tick_poll_never_waits_for_inflight_owner_and_returns_every_outcome_once() {
    let (started, began) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let worker = OwnerWorker::new("local-owner-fixture", move |state: &mut LocalTickState, _: ()| {
        started.send(()).unwrap();
        gate.recv().unwrap();
        state.world.set(IVec3::new(4, 1, 4), 41 << 4);
        state.sim.clock += 1;
        state.player.look(12.5, -4.);
        TickOutput {
            outcome: Outcome { changed: vec![IVec3::X, IVec3::Z, IVec3::X],
                sounds: vec![("a", 1.), ("b", 0.5)], pitched_sounds: vec![("c", 0.8, 1.2)],
                collected_items: vec![19, 7], guest_item_stats: vec![("guest".into(), "stat".into())],
                ..Default::default() },
            chat: vec![json!({"text":"first"}), json!({"text":"second"})],
            lan_chat: vec!["host message".into()],
            world_sounds: vec![("world-sound".into(), DVec3::ONE, 1., 0.5)],
            open_command_block: Some(IVec3::new(7, 2, 1)), died: true,
        }
    }).unwrap();
    let mut worker = worker;
    let mut owner = Some(state());
    let mirror = owner.as_ref().unwrap().clone();
    let id = worker.submit(&mut owner, ()).unwrap();
    assert!(owner.is_none());
    began.recv_timeout(Duration::from_secs(2)).unwrap();

    // The gate stays closed until polling proves it can return. A blocking
    // implementation fails with a timeout rather than hanging this test.
    let (checked, result) = mpsc::channel();
    let checker = thread::spawn(move || {
        for _ in 0..256 { assert!(worker.poll().unwrap().is_none()); }
        let mut rejected = Some(state());
        assert!(worker.submit(&mut rejected, ()).is_err());
        assert!(rejected.is_some());
        checked.send(worker).unwrap_or_else(|_| panic!("poll receiver vanished"));
    });
    let early = result.recv_timeout(Duration::from_secs(2));
    release.send(()).unwrap();
    checker.join().unwrap();
    let mut worker = early.expect("poll blocked on the tick owner");
    let done = completed(worker.drain().unwrap().unwrap());
    assert_eq!(done.id, id);
    assert_eq!(done.state.sim.clock, mirror.sim.clock + 1);
    assert_eq!(done.state.world.get(IVec3::new(4, 1, 4)), 41 << 4);
    assert_eq!(mirror.world.get(IVec3::new(4, 1, 4)), 0);
    assert_ne!(done.state.player.java_rotation(), mirror.player.java_rotation());
    assert_eq!(done.outcome.changed, vec![IVec3::X, IVec3::Z, IVec3::X]);
    assert_eq!(done.outcome.sounds, vec![("a", 1.), ("b", 0.5)]);
    assert_eq!(done.outcome.pitched_sounds, vec![("c", 0.8, 1.2)]);
    assert_eq!(done.outcome.collected_items, vec![19, 7]);
    assert_eq!(done.outcome.guest_item_stats, vec![("guest".into(), "stat".into())]);
    assert_eq!(done.chat, vec![json!({"text":"first"}), json!({"text":"second"})]);
    assert_eq!(done.lan_chat, vec!["host message"]);
    assert_eq!(done.world_sounds, vec![("world-sound".into(), DVec3::ONE, 1., 0.5)]);
    assert_eq!(done.open_command_block, Some(IVec3::new(7, 2, 1)));
    assert!(done.died);
    assert!(worker.poll().unwrap().is_none());
    assert!(worker.drain().unwrap().is_none());
}

#[derive(Debug)]
struct Retired {
    id: u8,
    dropped: mpsc::Sender<(u8, String)>,
}
impl Drop for Retired {
    fn drop(&mut self) {
        let _ = self.dropped.send((self.id, thread::current().name().unwrap_or("").into()));
    }
}
#[test]
fn local_tick_retirement_is_bounded_and_runs_on_owner_thread() {
    let (drop_tx, drops) = mpsc::channel();
    let (started, began) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let mut worker = OwnerWorker::new("retirement-owner", move |_: &mut Retired, _: ()| {
        started.send(()).unwrap(); gate.recv().unwrap();
    }).unwrap();
    let mut owner = Some(Retired { id: 0, dropped: drop_tx.clone() });
    worker.submit(&mut owner, ()).unwrap();
    began.recv_timeout(Duration::from_secs(2)).unwrap();
    worker.retire(Retired { id: 1, dropped: drop_tx.clone() }).unwrap();
    worker.retire(Retired { id: 2, dropped: drop_tx.clone() }).unwrap();
    let rejected = worker.retire(Retired { id: 3, dropped: drop_tx }).unwrap_err();
    assert_eq!(rejected.id, 3);
    assert!(drops.try_recv().is_err());
    release.send(()).unwrap();
    let done = worker.drain().unwrap().unwrap();
    assert_eq!(done.state.id, 0);
    drop(worker); // joins, including both queued retirements
    assert_eq!(drops.recv_timeout(Duration::from_secs(2)).unwrap(), (1, "retirement-owner".into()));
    assert_eq!(drops.recv_timeout(Duration::from_secs(2)).unwrap(), (2, "retirement-owner".into()));
    assert!(drops.try_recv().is_err());
    drop((done, rejected));
}

#[test]
fn local_tick_failure_keeps_owned_state_and_refuses_to_repeat_partial_tick() {
    let mut worker = OwnerWorker::new("panic-owner", |state: &mut Vec<u8>, _: ()| -> () {
        state.push(2); panic!("fixture tick failure");
    }).unwrap();
    let mut owner = Some(vec![1]);
    worker.submit(&mut owner, ()).unwrap();
    let error = match worker.drain() { Err(error) => error, Ok(_) => panic!("panic was hidden") };
    assert!(error.to_string().contains("fixture tick failure"));
    assert_eq!(worker.failed_state.take(), Some(vec![1, 2]));
    let mut next = Some(vec![9]);
    assert!(worker.submit(&mut next, ()).is_err());
    assert_eq!(next, Some(vec![9]));
}

#[test]
fn local_tick_completion_wakes_only_after_authoritative_reply_is_available() {
    let (wake, events) = mpsc::channel();
    let mut worker = OwnerWorker::with_wake("wake-owner", |state: &mut u32, _: ()| {
        *state += 1;
        "published"
    }, move || { wake.send(()).unwrap(); }).unwrap();
    let mut owner = Some(41);
    worker.submit(&mut owner, ()).unwrap();
    events.recv_timeout(Duration::from_secs(2)).unwrap();
    let result = worker.poll().unwrap().expect("wake must follow reply publication");
    assert_eq!((result.state, result.output), (42, "published"));
    assert!(worker.poll().unwrap().is_none());
    assert!(events.try_recv().is_err());
}

#[test]
fn local_tick_player_clone_preserves_private_motion_rotation_and_animation() {
    let mut original = state();
    original.player.look(123.125, -17.75);
    original.player.keys.insert(KeyCode::KeyW);
    original.player.keys.insert(KeyCode::ControlLeft);
    let mut fall = 0.;
    for _ in 0..7 { original.player.tick_local(&original.world, &mut fall); }
    original.player.look(-2.125, 5.375);
    let mut copy = original.player.clone();
    assert_eq!(copy.keys, original.player.keys);
    assert_eq!(serde_json::to_value(&copy).unwrap(), serde_json::to_value(&original.player).unwrap());
    assert_eq!(format!("{:?}", copy.third_person), format!("{:?}", original.player.third_person));
    let mut copy_fall = fall;
    for tick in 0..24 {
        if tick == 3 { original.player.keys.insert(KeyCode::Space); copy.keys.insert(KeyCode::Space); }
        if tick == 8 { original.player.keys.clear(); copy.keys.clear(); }
        original.player.look(0.125, -0.375);
        copy.look(0.125, -0.375);
        original.player.tick_local(&original.world, &mut fall);
        copy.tick_local(&original.world, &mut copy_fall);
        assert_eq!(copy.precise_position(), original.player.precise_position());
        assert_eq!(copy.precise_velocity(), original.player.precise_velocity());
        assert_eq!(copy.java_rotation(), original.player.java_rotation());
        assert_eq!(copy.precise_eye(0.37), original.player.precise_eye(0.37));
        assert_eq!(serde_json::to_value(&copy).unwrap(), serde_json::to_value(&original.player).unwrap());
        assert_eq!(format!("{:?}", copy.third_person), format!("{:?}", original.player.third_person));
        assert_eq!(copy_fall, fall);
    }
}

fn output_value(out: &Outcome) -> Value {
    json!({"changed":out.changed,"sounds":out.sounds,"pitched":out.pitched_sounds,
        "collected":out.collected_items,"guest":out.guest_item_stats,
        "commands":out.command_blocks,"editor":out.open_command_block})
}
#[test]
#[ignore = "requires installed resources; executes actual local gameplay, command blocks and death"]
fn local_tick_actual_worker_matches_synchronous_state_rng_outcomes_and_commands() {
    use clap::Parser;
    let resources = Arc::new(Resources::load().unwrap());
    let mut options = Options::parse_from(["ferrum189", "--creative"]);
    options.world_rules.allow_cheats = true;
    let settings = Settings { render_distance: 3, ..Default::default() };
    let mut expected = state();
    let command = IVec3::new(3, 1, 3);
    expected.world.set(command, 137 << 4);
    assert!(gameplay::command_blocks::save(&mut expected.sim, &mut expected.world,
        &options, command, "say worker-order", false));
    expected.world.set(command + IVec3::X, 152 << 4);
    expected.sim.block_ticks.notify(&expected.world, command + IVec3::X);
    let mut actual = Some(expected.clone());
    let mut worker = LocalTickWorker::new().unwrap();
    let mut messages = 0;
    for tick in 0..16 {
        for state in [&mut expected, actual.as_mut().unwrap()] {
            state.player.look(0.25, -0.125);
            if tick < 8 { state.player.keys.insert(KeyCode::KeyW); }
            else { state.player.keys.clear(); }
            if tick == 15 { state.sim.health = 0.; }
        }
        let input = || TickInput { resources: resources.clone(), settings: settings.clone(), options: options.clone(),
            left: false, right: false, chest: None, death_screen: false };
        let before = step(&mut expected, input());
        worker.submit(&mut actual, input()).unwrap();
        assert!(actual.is_none());
        let after = worker.drain().unwrap().unwrap();
        assert_eq!(output_value(&after.outcome), output_value(&before.outcome));
        assert_eq!(after.chat, before.chat);
        assert_eq!(after.lan_chat, before.lan_chat);
        assert_eq!(after.world_sounds, before.world_sounds);
        assert_eq!(after.open_command_block, before.open_command_block);
        assert_eq!(after.died, before.died);
        assert_eq!(after.died, tick == 15);
        messages += after.chat.len();
        assert_eq!(serde_json::to_value(&after.state.sim).unwrap(), serde_json::to_value(&expected.sim).unwrap(), "simulation tick {tick}");
        assert_eq!(serde_json::to_value(&after.state.player).unwrap(), serde_json::to_value(&expected.player).unwrap(), "player tick {tick}");
        assert_eq!(after.state.world.edits, expected.world.edits);
        for (pos, chunk) in &expected.world.chunks {
            assert_eq!(after.state.world.chunks[pos].blocks, chunk.blocks);
            assert_eq!(after.state.world.chunks[pos].tiles, chunk.tiles);
        }
        actual = Some(after.state);
    }
    assert!(messages > 0, "the powered command block must execute and publish its chat");
    assert_eq!(actual.as_ref().unwrap().sim.statistics.get("stat.deaths"), Some(&1));
}
