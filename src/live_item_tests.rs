use crate::gameplay::{Mode, Outcome, Simulation};
use crate::inventory::Stack;
use crate::player::Player;
use crate::vanilla::item_entity::ItemEntity;
use crate::vanilla::random::JavaRandom;
use crate::world::{Chunk, World};
use glam::{DVec3, Vec3};
use serde::Deserialize;
use std::sync::{Arc, LazyLock};

#[derive(Deserialize)]
struct Frame(i32, i32, i32, [f64; 3], [f64; 3], [f64; 3], u8, u8, bool);
#[derive(Deserialize)]
struct Trace {
    name: String,
    all: bool,
    pitch: f32,
    seed: i64,
    thrower: String,
    frames: Vec<Frame>,
}
#[derive(Deserialize)]
struct Reference {
    traces: Vec<Trace>,
    #[serde(rename = "serverMovementPackets")]
    packets: Vec<Trace>,
    bounds: Vec<([f64; 3], bool)>,
    capacity: Vec<(u8, bool, u8, u8, bool)>,
}
static REFERENCE: LazyLock<Reference> = LazyLock::new(|| {
    serde_json::from_reader(flate2::read::GzDecoder::new(
        &include_bytes!("../tests/fixtures/vanilla189-q-pickup.json.gz")[..],
    ))
    .unwrap()
});

fn floor() -> World {
    let mut chunk = Chunk::empty();
    for x in 0..16 {
        for z in 0..16 {
            chunk.set(x, 63, z, 16);
        }
    }
    World {
        chunks: [((0, 0), Arc::new(chunk))].into(),
        spawn: Vec3::new(8., 64., 8.),
        label: "Q pickup".into(),
        save_path: Default::default(),
        edits: Default::default(),
        imported: false,
        light_revision: 0,
    }
}

#[test]
#[ignore = "loads installed 1.8.9 resources for the normal singleplayer game tick"]
fn walking_into_pickup_reach_uses_the_previous_accepted_position() {
    let mut world=floor();
    let feet=world.spawn.as_dvec3();
    let mut player=Player::new(world.spawn);
    player.set_pose_f64(feet.to_array(),0.,0.,true);
    player.set_velocity_f64([0.25,0.,0.]);
    let mut sim=Simulation::default();
    let id=sim.spawn_item_entity(Stack::new(1,1),feet+DVec3::new(1.50,0.,0.),DVec3::ZERO,0).id;
    let settings=crate::settings::Settings::default();
    let r=crate::validation::resources();
    let first=sim.tick(&mut world,&mut player,r,&settings,false,false);
    assert!(first.collected_items.is_empty());
    assert!(player.precise_position()[0]>feet.x+0.08);
    let second=sim.tick(&mut world,&mut player,r,&settings,false,false);
    assert_eq!(second.collected_items,[id]);
    assert_eq!(sim.inventory.slots[0].as_ref().map(|s|(s.id,s.count)),Some((1,1)));
}

fn stone_count(sim: &Simulation) -> u8 {
    sim.inventory.slots[..36]
        .iter()
        .flatten()
        .filter(|s| s.id == 1)
        .map(|s| s.count)
        .sum()
}

#[test]
fn original_q_ctrl_q_birth_motion_and_pickup_callbacks() {
    let world = floor();
    for (trace, packet) in REFERENCE
        .traces
        .iter()
        .map(|t| (t, false))
        .chain(REFERENCE.packets.iter().map(|t| (t, true)))
    {
        let mut sim = Simulation::default();
        sim.local_name = trace.thrower.clone();
        sim.player_random_state = JavaRandom::new(trace.seed).state();
        sim.inventory.slots[0] = Some(Stack::new(1, 12));
        let mut player = Player::new(world.spawn);
        player.set_pose_f64(world.spawn.as_dvec3().to_array(), 0., trace.pitch, true);
        sim.throw_selected(&player, trace.all);
        assert_eq!(
            sim.drops.len(),
            1,
            "{} must create an entity immediately",
            trace.name
        );
        assert_eq!(
            sim.drops[0].thrower.as_deref(),
            Some(trace.thrower.as_str())
        );
        assert!(sim.drops[0].owner.is_none());
        for (index, Frame(tick, age, delay, pos, velocity, at, inventory, remaining, dead)) in
            trace.frames.iter().enumerate()
        {
            let previous = sim.drops[0].clone();
            if *tick != 0 {
                // C04 invokes the living player's collision pass before accepting its new position.
                let callback_position = if packet {
                    trace.frames[index - 1].5
                } else {
                    *at
                };
                player.set_position_f64(callback_position);
                let mut out = Outcome::default();
                sim.tick_player_item_pickup(&player, &mut out);
                sim.tick_drop_motion(&world, &mut out);
            }
            let caption = format!("{} at tick {tick}", trace.name);
            assert_eq!(stone_count(&sim), *inventory, "{caption} inventory");
            let alive = sim.drops.iter().find(|d| !d.dead);
            assert_eq!(alive.is_none(), *dead, "{caption} alive");
            assert_eq!(
                alive.map_or(0, |d| d.stack.count),
                *remaining,
                "{caption} remaining"
            );
            // The collected entity does not receive a final motion tick.
            let entity = alive.unwrap_or(&previous);
            assert_eq!(
                (entity.age, entity.pickup_delay),
                (*age, *delay),
                "{caption} age / delay"
            );
            for (observed, original) in [(entity.position, *pos), (entity.velocity, *velocity)] {
                assert!(
                    observed.abs_diff_eq(DVec3::from_array(original), 1.0e-10),
                    "{caption}: Rust {observed:?} original {original:?}"
                );
            }
        }
    }
}

fn loose_stone(at: DVec3, count: u8) -> ItemEntity {
    ItemEntity::new(
        1,
        at,
        Stack::new(1, count),
        &mut JavaRandom::new(189),
        JavaRandom::new(5).state(),
    )
}

#[test]
fn original_q_pickup_strict_box_and_inventory_capacity() {
    let at = DVec3::new(8., 64., 8.);
    let player = Player::new(at.as_vec3());
    for &(offset, expected) in &REFERENCE.bounds {
        let mut sim = Simulation::default();
        sim.drops
            .push(loose_stone(at + DVec3::from_array(offset), 1));
        sim.tick_player_item_pickup(&player, &mut Outcome::default());
        assert_eq!(stone_count(&sim) == 1, expected, "drop offset {offset:?}");
    }
    for &(room, creative, inventory, remaining, dead) in &REFERENCE.capacity {
        let mut sim = Simulation::default();
        sim.mode = if creative {
            Mode::Creative
        } else {
            Mode::Survival
        };
        sim.inventory.slots[..36].fill_with(|| Some(Stack::new(3, 64)));
        sim.inventory.slots[0] = Some(Stack::new(1, 64 - room));
        sim.drops.push(loose_stone(at, 8));
        sim.tick_player_item_pickup(&player, &mut Outcome::default());
        let alive = sim.drops.iter().find(|d| !d.dead);
        assert_eq!(
            (
                stone_count(&sim),
                alive.map_or(0, |d| d.stack.count),
                alive.is_none()
            ),
            (inventory, remaining, dead),
            "room {room} creative {creative}"
        );
    }
}
