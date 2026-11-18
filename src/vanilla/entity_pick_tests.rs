use super::*;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
struct Row {
    case: (usize, usize, bool, f32),
    eye: [String; 3],
    look: [String; 3],
    view: [String; 6],
    riding: i32,
    entities: Vec<Entry>,
    blocks: Vec<[i32; 4]>,
    kind: String,
    pointed: i32,
    entity: Option<i32>,
    face: Option<i8>,
    pos: Option<[i32; 3]>,
    point: Option<[String; 3]>,
    interact_at: Option<String>,
}
#[derive(Deserialize)]
struct Entry {
    id: i32,
    bounds: [String; 6],
    section: [i32; 3],
    allow: bool,
    pointed: bool,
    border: f32,
    feet: [String; 3],
}
struct Blocks(HashMap<IVec3, u16>);
impl ray_trace::Access for Blocks {
    fn get(&self, p: IVec3) -> u16 {
        self.0.get(&p).copied().unwrap_or(0)
    }
}
fn numbers<const N: usize>(bits: [String; N]) -> [f64; N] {
    bits.map(|s| f64::from_bits(u64::from_str_radix(&s, 16).unwrap()))
}
fn bounds(raw: [String; 6]) -> Aabb {
    let [x, y, z, a, b, c] = numbers(raw);
    Aabb {
        min: DVec3::new(x, y, z),
        max: DVec3::new(a, b, c),
    }
}
#[test]
fn original_renderer_entity_pick_chunk_order_riding_frames_and_distance_ties_match() {
    let rows: Vec<Row> = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-entity-pick.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 486);
    for row in rows {
        let w = Blocks(
            row.blocks
                .into_iter()
                .map(|[x, y, z, s]| (IVec3::new(x, y, z), s as u16))
                .collect(),
        );
        let view = View {
            bounds: bounds(row.view),
            eye: DVec3::from_array(numbers(row.eye)),
            look: DVec3::from_array(numbers(row.look)),
            creative: row.case.2,
        };
        let positions: HashMap<_, _> = row
            .entities
            .iter()
            .map(|e| (e.id, DVec3::from_array(numbers(e.feet.clone()))))
            .collect();
        let found = view.pick(
            &w,
            row.entities
                .into_iter()
                .enumerate()
                .filter(|(_, e)| e.allow)
                .map(|(n, e)| Candidate {
                    id: e.id,
                    section: e.section,
                    bounds: bounds(e.bounds),
                    inserted: n as u64,
                    border: e.border,
                    living_or_frame: e.pointed,
                }),
            (row.riding != 0).then_some(row.riding),
        );
        let (kind, entity, pointed, point, pos, face) = match found {
            Some(Hit::Block(b)) => ("BLOCK", 0, 0, Some(b.point), Some(b.position), Some(b.face)),
            Some(Hit::Miss {
                point,
                position,
                face,
            }) => ("MISS", 0, 0, Some(point), Some(position), face),
            Some(Hit::Entity { id, point, pointed }) => (
                "ENTITY",
                id,
                if pointed { id } else { 0 },
                Some(point),
                None,
                None,
            ),
            None => ("NONE", 0, 0, None, None, None),
        };
        if let Some(expected) = row.interact_at {
            let mut bytes = crate::protocol::Writer::new();
            crate::network::write_interaction(
                &mut bytes,
                entity,
                Some(relative_hit(point.unwrap(), positions[&entity])),
            )
            .unwrap();
            let hex: String = bytes.0.iter().map(|b| format!("{b:02x}")).collect();
            assert_eq!(hex, expected, "original C02 INTERACT_AT {:?}", row.case);
        }
        assert_eq!(
            (
                kind,
                entity,
                pointed,
                point.map(|p| p.to_array().map(f64::to_bits)),
                pos.map(|p| p.to_array()),
                face
            ),
            (
                row.kind.as_str(),
                row.entity.unwrap_or(0),
                row.pointed,
                row.point.map(|bits| numbers(bits).map(f64::to_bits)),
                row.pos,
                row.face.filter(|&v| v >= 0).map(|v| v as u8)
            ),
            "original scene {:?}",
            row.case
        );
    }
}
fn payload(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}
#[derive(Deserialize)]
struct PacketCases {
    players: Vec<PlayerPacket>,
    attach: Vec<Attach>,
    objects: Vec<Object>,
    head_turns: Vec<Turn>,
}
#[derive(Deserialize)]
struct Turn {
    dx: f64,
    dy: f64,
    before: [String; 3],
    after: [String; 3],
}
#[derive(Deserialize)]
struct PlayerPacket {
    wire: String,
    action: u8,
    entries: Vec<Profile>,
}
#[derive(Deserialize)]
struct Profile {
    uuid: String,
    name: Option<String>,
    props: usize,
    mode: Option<i32>,
    ping: i32,
    display: Option<String>,
}
#[derive(Deserialize)]
struct Attach {
    wire: String,
    result: (i32, i32, u8),
}
#[derive(Deserialize)]
struct Object {
    kind: u8,
    data: i32,
    pose: [String; 3],
    feet: [String; 3],
    bounds: [String; 6],
    allow: bool,
    pointed: bool,
    border: f32,
}
#[test]
fn original_spawn_object_shapes_attachment_and_player_list_predicates_match() {
    let cases: PacketCases = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-entity-pick-packets.json"
    ))
    .unwrap();
    assert_eq!(
        (cases.players.len(), cases.attach.len(), cases.objects.len()),
        (25, 60, 363)
    );
    for packet in cases.players {
        let wire = payload(&packet.wire);
        let (action, players) =
            crate::network::read_player_info(&mut crate::protocol::Reader::new(&wire)).unwrap();
        assert_eq!(action, packet.action);
        assert_eq!(players.len(), packet.entries.len());
        for (player, e) in players.into_iter().zip(packet.entries) {
            assert_eq!(player.uuid.as_slice(), payload(&e.uuid));
            assert_eq!(player.name, e.name);
            assert_eq!(player.properties.len(), e.props);
            assert_eq!(player.mode, e.mode);
            assert_eq!(player.latency.unwrap_or(0), e.ping);
            let display = player.display_name.map(|s| {
                serde_json::from_str::<serde_json::Value>(&s).unwrap()["text"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            });
            assert_eq!(display, e.display);
        }
    }
    for packet in cases.attach {
        let wire = payload(&packet.wire);
        assert_eq!(
            crate::network::read_attach(&mut crate::protocol::Reader::new(&wire)).unwrap(),
            packet.result
        );
    }
    for o in cases.objects {
        let position = object_position(o.kind, o.data, DVec3::from_array(numbers(o.pose)));
        assert_eq!(
            position.to_array().map(f64::to_bits),
            numbers(o.feet).map(f64::to_bits),
            "object {} data {}",
            o.kind,
            o.data
        );
        let pick = object_bounds(o.kind, o.data, position, 0);
        assert_eq!(pick.is_some(), o.allow, "object {} data {}", o.kind, o.data);
        if let Some((b, border, pointed)) = pick {
            let actual = [b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z];
            assert_eq!(
                actual.map(f64::to_bits),
                numbers(o.bounds).map(f64::to_bits),
                "object {} data {}",
                o.kind,
                o.data
            );
            assert_eq!((border.to_bits(), pointed), (o.border.to_bits(), o.pointed));
        }
    }
}
fn play_world() -> crate::world::World {
    use crate::world::{Chunk, World};
    let mut c = Chunk::empty();
    for x in 0..16 {
        for z in 0..16 {
            c.set(x, 0, z, 16);
        }
    }
    World {
        chunks: crate::world::ChunkMap::from([((0, 0), std::sync::Arc::new(c))]),
        spawn: glam::Vec3::new(8.5, 1., 8.),
        label: "Picking".into(),
        save_path: Default::default(),
        edits: Default::default(),
        imported: false,
        light_revision: 0,
    }
}
#[test]
fn original_sp_mouse_yaw_and_item_direction_update_at_different_tick_phases() {
    let cases: PacketCases = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-entity-pick-packets.json"
    ))
    .unwrap();
    let w = play_world();
    let mut player = crate::player::Player::new(w.spawn);
    player.set_rotation_degrees(0., 0.);
    for t in cases.head_turns {
        player.look(t.dx, t.dy);
        let previous_look = numbers(t.before);
        assert_eq!(
            player.pick_view(false, 1.).look.to_array().map(f64::to_bits),
            previous_look.map(f64::to_bits)
        );
        let item = player.direction();
        player.tick(&w);
        assert!(player.pick_view(false, 0.).look.abs_diff_eq(
            DVec3::from_array(previous_look), 0.00011),
            "the first rendered frame must still follow the previous head angle");
        assert_eq!(
            player.pick_view(false, 1.).look.to_array().map(f64::to_bits),
            numbers(t.after).map(f64::to_bits)
        );
        assert_eq!(player.pick_view(false, 1.).look, item.as_dvec3());
    }
}
#[test]
fn local_target_selection_occludes_merchant_and_block_behind_a_nearer_actor() {
    use crate::gameplay::{LocalTarget, Mob, MobKind, Mode, Simulation};
    let mut w = play_world();
    let mut player = crate::player::Player::new(w.spawn);
    player.set_rotation_degrees(0., 0.);
    let mut sim = Simulation::default();
    let spawn = |id, kind, z| {
        let position = glam::Vec3::new(8.5, 1., z);
        Mob {
            id,
            kind,
            position,
            previous: position,
            velocity: glam::Vec3::ZERO,
            yaw: 0.,
            health: 10.,
            age: 0,
            turn_ticks: 10,
            hurt: 0,
            fuse: 0,
            swing_ticks: 0,
            death_ticks: 0,
            eat_ticks: 0,
            attack_ticks: 0,
            rose_ticks: 0,
            prev_fuse: 0,
        }
    };
    sim.mobs.push(spawn(20, MobKind::Villager, 10.8));
    sim.mobs.push(spawn(30, MobKind::Zombie, 9.9));
    w.set(IVec3::new(8, 2, 11), 16);
    assert_eq!(
        sim.pick(&w, &player).unwrap().entity(),
        Some(LocalTarget::Mob(30))
    );
    sim.mobs.pop();
    assert_eq!(
        sim.pick(&w, &player).unwrap().entity(),
        Some(LocalTarget::Mob(20))
    );
    sim.mobs[0].position.z = 12.2;
    assert_eq!(
        sim.pick(&w, &player).unwrap().block().unwrap().position,
        IVec3::new(8, 2, 11)
    );
    w.set(IVec3::new(8, 2, 11), 0);
    assert!(matches!(
        sim.pick(&w, &player),
        Some(Hit::Miss { face: None, .. })
    ));
    sim.mode = Mode::Creative;
    assert_eq!(
        sim.pick(&w, &player).unwrap().entity(),
        Some(LocalTarget::Mob(20))
    );
}
