use super::*;
use crate::{
    gameplay::{Mode, Outcome},
    settings::Settings,
};
use glam::{IVec3, Vec3};

fn queued(view: &Mutex<View>, n: usize) {
    let start = Instant::now();
    loop {
        if view
            .lock()
            .unwrap()
            .users
            .get("Guest")
            .is_some_and(|g| g.inputs.len() >= n)
        {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "LAN inputs not received"
        );
        thread::sleep(Duration::from_millis(3));
    }
}
fn until(w: &mut Wire, test: impl Fn(&Packet) -> bool) -> Packet {
    let start = Instant::now();
    loop {
        if let Some(p) = w.poll().unwrap() {
            if test(&p) {
                return p;
            }
        }
        assert!(
            start.elapsed() < Duration::from_secs(4),
            "Expected LAN packet missing"
        );
    }
}
fn tick(
    host: &Host,
    world: &mut World,
    sim: &mut Simulation,
    p: &Player,
    settings: &Settings,
) -> Outcome {
    sim.total_ticks += 1;
    let mut out = Outcome::default();
    use clap::Parser;
    let options = Options::parse_from(["ferrum189", "--username", "Host"]);
    host.update(
        world,
        sim,
        p,
        settings,
        crate::validation::resources(),
        &options,
        &mut out,
    );
    out
}
fn expect_block(p: &Packet, pos: IVec3, s: i32) -> bool {
    if p.id != 0x23 {
        return false;
    }
    let mut r = Reader::new(&p.data);
    r.position().ok() == Some(pos.to_array()) && r.varint().ok() == Some(s)
}
const GUEST: &str = "Guest";
const UUID: &str = "0cc5779e-0a84-4f64-b8dc-fac556017db3";
const SPAWN: [f64; 3] = [8.5, 42., 8.5];
#[test]
#[ignore = "opens a disposable protocol 47 LAN session and loads installed resources"]
fn lan_guest_receives_bed_position_and_wake_animation_for_the_host() {
    let (host, mut world, mut sim, mut player, settings, mut wire, handle) = session(0, false);
    let head = IVec3::new(6, 42, 8);
    world.set(head, 26 << 4 | 8);
    world.set(head-IVec3::Z, 26 << 4);
    sim.clock = 18000;
    assert_eq!(sim.try_bed(&mut world, &mut player, head).0, crate::vanilla::sleep::Attempt::Asleep);
    tick(&host, &mut world, &mut sim, &player, &settings);
    let packet = until(&mut wire, |p| p.id == 0x0a);
    let mut enter = Reader::new(&packet.data);
    assert_eq!((enter.varint().unwrap(), enter.position().unwrap()), (1, head.to_array()));
    sim.wake(&mut world, &mut player, false, true);
    tick(&host, &mut world, &mut sim, &player, &settings);
    let packet = until(&mut wire, |p| p.id == 0x0b);
    let mut wake = Reader::new(&packet.data);
    assert_eq!((wake.varint().unwrap(), wake.u8().unwrap()), (1, 2));
    drop(wire);
    handle.join().unwrap();
}
fn session(
    guest_mode: u8,
    allow_commands: bool,
) -> (
    Host,
    World,
    Simulation,
    Player,
    Settings,
    Wire,
    thread::JoinHandle<()>,
) {
    let name = GUEST;
    let uuid = UUID;
    let spawn = SPAWN;
    let mut world = World {
        spawn: DVec3::from(spawn).as_vec3(),
        chunks: crate::world::ChunkMap::new(),
        label: "Test".into(),
        edits: Default::default(),
        save_path: Default::default(),
        imported: false,
        light_revision: 0,
    };
    let mut chunk = Chunk::empty();
    for z in 0..16 {
        for x in 0..16 {
            chunk.set(x, 41, z, 16);
        }
    }
    chunk.set(9, 42, 8, 3 << 4);
    world.chunks.insert((0, 0), Arc::new(chunk));
    let host_player = Player::new(Vec3::new(6.5, 42., 8.5));
    let mut sim = Simulation::default();
    sim.mode = Mode::Creative;
    sim.inventory.slots[0] = Some(crate::inventory::Stack::new(264, 4));
    let settings = Settings::default();
    let view = Arc::new(Mutex::new(View {
        label: "Test".into(),
        owner: "Host".into(),
        guest_mode,
        hardcore: false,
        dimension: 0,
        host_actor: Actor::new(
            1,
            crate::ui::offline_uuid("Host"),
            "Host",
            host_player.precise_position(),
        ),
        spawn,
        chunks: world.chunks.clone(),
        clock: 0,
        total: 0,
        difficulty: 2,
        users: HashMap::new(),
        next_actor: FIRST_GUEST,
        departed: vec![],
        drops: Default::default(),
        messages: VecDeque::new(),
        host_chat: Default::default(),
        sequence: 0,
    }));
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let host = Host {
        view: view.clone(),
        _life: Arc::new(()),
        port: address.port(),
        allow_commands,
    };
    let data = view.clone();
    let life = Arc::downgrade(&host._life);
    // Enter the same play connection after account verification. Authentication code has no test bypass.
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream.set_nodelay(true).unwrap();
        let mut w = Wire {
            stream,
            decoder: Default::default(),
            cipher: None,
        };
        let _ = admitted(&mut w, name, uuid, &data, life);
    });
    let stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(40)))
        .unwrap();
    stream.set_nodelay(true).unwrap();
    let mut wire = Wire {
        stream,
        decoder: Default::default(),
        cipher: None,
    };
    assert_eq!(wire.next().unwrap().id, 3);
    wire.decoder.compression = Some(256);
    assert_eq!(wire.next().unwrap().id, 2);
    (host, world, sim, host_player, settings, wire, handle)
}

#[test]
fn lan_placed_banner_sends_its_design_after_the_chunk() {
    use crate::vanilla::{drops::DropNbt as Tag, item_nbt, tile_updates};
    let (host, mut world, mut sim, player, settings, mut wire, handle) = session(1, false);
    let support = IVec3::new(10, 41, 8);
    let at = support + IVec3::Y;
    let patterns = Tag::List { element: 10, values: vec![Tag::Compound([
        ("Pattern".into(), Tag::String("cs".into())), ("Color".into(), Tag::Int(11))
    ].into())] };
    let design = Tag::Compound([("Patterns".into(), patterns.clone()), ("Base".into(), Tag::Int(4))].into());
    let stack = crate::inventory::Stack::new(425, 1).with_nbt(Tag::Compound([("BlockEntityTag".into(), design)].into()));
    let slot = crate::remote::to_slot(&stack).unwrap();
    until(&mut wire, |p| p.id == 0x21);
    wire.send(0x10, Writer::new().i16(36).slot(Some(&slot))).unwrap();
    wire.send(0x08, Writer::new().position(support.to_array()).u8(1).slot(Some(&slot)).bytes(&[8,16,8])).unwrap();
    queued(&host.view, 2);
    tick(&host, &mut world, &mut sim, &player, &settings);
    assert_eq!(world.get(at) >> 4, 176);
    until(&mut wire, |p| p.id == 0x21);
    let packet = until(&mut wire, |p| p.id == 0x35);
    let (position, action, tag) = tile_updates::decode_tile(&mut Reader::new(&packet.data)).unwrap();
    assert_eq!((position, action), (at, 6));
    let fields = item_nbt::compound(&tag).unwrap();
    assert_eq!(item_nbt::number(fields.get("Base")), 4);
    assert_eq!(fields.get("Patterns"), Some(&patterns));
    drop(host);
    drop(wire);
    handle.join().unwrap();
}

#[test]
fn lan_options_control_join_abilities_creative_slots_and_command_permissions() {
    use crate::inventory::Stack;
    for (mode, allow, flags) in [(0, false, 0), (1, false, 13), (2, true, 0), (3, true, 7)] {
        let (host, mut world, mut sim, player, settings, mut wire, handle) = session(mode, allow);
        sim.lan = Some(host.clone());
        let join = until(&mut wire, |p| p.id == 1);
        let mut r = Reader::new(&join.data);
        assert_eq!(r.i32().unwrap(), FIRST_GUEST);
        assert_eq!(r.u8().unwrap(), mode);
        let caps = until(&mut wire, |p| p.id == 0x39);
        let mut r = Reader::new(&caps.data);
        assert_eq!(
            (r.u8().unwrap(), r.f32().unwrap(), r.f32().unwrap()),
            (flags, 0.05, 0.1)
        );
        let item = crate::remote::to_slot(&Stack::new(20, 12)).unwrap();
        wire.send(0x10, Writer::new().i16(36).slot(Some(&item)))
            .unwrap();
        let target = IVec3::new(9, 42, 8);
        wire.send(7, Writer::new().u8(0).position(target.to_array()).u8(1))
            .unwrap();
        wire.send(1, Writer::new().string("/time set night"))
            .unwrap();
        queued(&host.view, 3);
        tick(&host, &mut world, &mut sim, &player, &settings);
        let feedback = until(&mut wire, |p| p.id == 2);
        let mut r = Reader::new(&feedback.data);
        let text = r.string(32767).unwrap();
        assert!(
            text.contains(if allow {
                "commands.time.set"
            } else {
                "commands.generic.permission"
            }),
            "{text}"
        );
        assert_eq!(world.get(target) == 0, mode == 1);
        assert_eq!(sim.clock == 13000, allow);
        assert_eq!(sim.commands.player_mode, None);
        assert_eq!(sim.mode, Mode::Creative);
        assert_eq!(sim.inventory.slots[0], Some(Stack::new(264, 4)));
        let saved = serde_json::to_value(&sim).unwrap();
        assert_eq!(
            saved["lan_guests"][UUID.replace("-", "")]["inventory"]["slots"][0]["id"].as_u64(),
            (mode == 1).then_some(20)
        );
        if allow {
            wire.send(1, Writer::new().string("/gamemode creative"))
                .unwrap();
            queued(&host.view, 1);
            tick(&host, &mut world, &mut sim, &player, &settings);
            let update = until(&mut wire, |p| p.id == 0x2b);
            let mut r = Reader::new(&update.data);
            assert_eq!((r.u8().unwrap(), r.f32().unwrap()), (3, 1.0));
            assert_eq!(host.view.lock().unwrap().users[GUEST].actor.mode, 1);
            assert_eq!(
                sim.commands.player_mode, None,
                "guest command must only change its sender"
            );
        }
        drop(wire);
        handle.join().unwrap();
    }
}

#[test]
fn guest_q_and_ctrl_q_delay_forty_item_updates_then_collect_over_protocol_47() {
    let (host, mut world, mut sim, player, settings, mut wire, handle) = session(0, false);
    let mut bag = crate::inventory::Inventory::default();
    bag.slots[0] = Some(crate::inventory::Stack::new(4, 4));
    sim.lan_guests.insert(
        UUID.replace('-', ""),
        serde_json::from_value(json!({"inventory":bag})).unwrap(),
    );
    sim.lan = Some(host.clone());
    until(&mut wire, |p| p.id == 1);
    wire.send(5, Writer::new().f32(32.).f32(90.).bool(true))
        .unwrap();
    wire.send(0x0b, Writer::new().varint(FIRST_GUEST).varint(0).varint(0))
        .unwrap();
    for (code, amount) in [(4, 1), (3, 4)] {
        wire.send(7, Writer::new().u8(code).position([0; 3]).u8(0))
            .unwrap();
        wire.send(3, Writer::new().bool(true)).unwrap();
        queued(&host.view, if code == 4 { 4 } else { 2 });
        tick(&host, &mut world, &mut sim, &player, &settings);
        let d = &sim.drops[0];
        assert_eq!(
            (d.stack.id, d.stack.count, d.pickup_delay, d.age),
            (4, amount, 39, 1)
        );
        assert_eq!(
            d.previous,
            crate::vanilla::item_entity::throw_position(SPAWN.into(), 1.62f32 - 0.08)
        );
        assert_eq!(d.thrower.as_deref(), Some("Guest"));
        let id = drop_id(d.id);
        let spawn = until(&mut wire, |p| p.id == 0x0e);
        let mut r = Reader::new(&spawn.data);
        assert_eq!((r.varint().unwrap(), r.u8().unwrap()), (id, 2));
        for v in d.previous.to_array() {
            assert_eq!(r.i32().unwrap(), (v * 32.).floor() as i32);
        }
        r.u8().unwrap();
        r.u8().unwrap();
        assert_eq!(r.i32().unwrap(), 1);
        let launch = [r.i16().unwrap(), r.i16().unwrap(), r.i16().unwrap()];
        assert!(launch[1] < -800 && launch[0].abs() < 161 && launch[2].abs() < 161);
        for n in 1..40 {
            wire.send(3, Writer::new().bool(true)).unwrap();
            queued(&host.view, 1);
            tick(&host, &mut world, &mut sim, &player, &settings);
            assert_eq!(sim.drops[0].pickup_delay, 39 - n);
        }
        wire.send(3, Writer::new().bool(true)).unwrap();
        queued(&host.view, 1);
        tick(&host, &mut world, &mut sim, &player, &settings);
        assert!(sim.drops.is_empty());
        let collect = until(&mut wire, |p| p.id == 0x0d);
        let mut r = Reader::new(&collect.data);
        assert_eq!(
            (r.varint().unwrap(), r.varint().unwrap()),
            (id, FIRST_GUEST)
        );
        let saved = serde_json::to_value(&sim.lan_guests[&UUID.replace('-', "")]).unwrap();
        assert_eq!(saved["inventory"]["slots"][0]["count"], 4);
        assert_eq!(
            saved["profile"]["statistics"]["stat.drop"],
            if code == 4 { 1 } else { 2 }
        );
        assert!(!sim.statistics.contains_key("stat.drop"));
    }
    // The new C04 enters reach. The first player callback still uses the old
    // accepted position. Its following C03 collects at the new feet.
    let pos = DVec3::from(SPAWN) + DVec3::Z * 1.7;
    let id = sim
        .spawn_item_entity(crate::inventory::Stack::new(1, 1), pos, DVec3::ZERO, 0)
        .id;
    wire.send(
        4,
        Writer::new()
            .f64(SPAWN[0])
            .f64(SPAWN[1])
            .f64(SPAWN[2] + 0.6)
            .bool(true),
    )
    .unwrap();
    queued(&host.view, 1);
    tick(&host, &mut world, &mut sim, &player, &settings);
    assert!(sim.drops.iter().any(|d| d.id == id));
    until(&mut wire, |p| p.id == 0x0e);
    wire.send(3, Writer::new().bool(true)).unwrap();
    queued(&host.view, 1);
    tick(&host, &mut world, &mut sim, &player, &settings);
    let collect = until(&mut wire, |p| p.id == 0x0d);
    assert_eq!(Reader::new(&collect.data).varint().unwrap(), drop_id(id));
    drop(wire);
    handle.join().unwrap();
}

#[test]
fn admitted_lan_survivor_mines_places_cannot_mint_and_sees_host_over_protocol_47() {
    let (host, mut world, mut sim, host_player, settings, mut wire, handle) = session(0, false);
    let (name, uuid, spawn) = (GUEST, UUID, SPAWN);
    let view = host.view.clone();
    let join = until(&mut wire, |p| p.id == 1);
    let mut r = Reader::new(&join.data);
    assert_eq!(r.i32().unwrap(), FIRST_GUEST);
    assert_eq!(r.u8().unwrap(), 0);
    tick(&host, &mut world, &mut sim, &host_player, &settings);
    until(&mut wire, |p| {
        p.id == 0x0c && Reader::new(&p.data).varint().unwrap() == 1
    });
    until(&mut wire, |p| p.id == 0x21);
    assert!(sim.server_scene.actors.contains_key(&FIRST_GUEST));

    let target = IVec3::new(9, 42, 8);
    let base = IVec3::new(10, 41, 8);
    let above = base + IVec3::Y;
    let fake = protocol::Slot {
        id: 57,
        count: 64,
        damage: 0,
        nbt: vec![0],
    };
    let place = |w: &mut Wire| {
        w.send(
            8,
            Writer::new()
                .position(base.to_array())
                .u8(1)
                .slot(Some(&fake))
                .bytes(&[8, 16, 8]),
        )
        .unwrap()
    };
    place(&mut wire);
    wire.send(3, Writer::new().bool(true)).unwrap();
    queued(&view, 2);
    tick(&host, &mut world, &mut sim, &host_player, &settings);
    until(&mut wire, |p| expect_block(p, above, 0));
    assert_eq!(world.get(above), 0);

    let dig = |w: &mut Wire, status: u8| {
        w.send(
            7,
            Writer::new().u8(status).position(target.to_array()).u8(4),
        )
        .unwrap()
    };
    dig(&mut wire, 0);
    dig(&mut wire, 2);
    queued(&view, 2);
    tick(&host, &mut world, &mut sim, &host_player, &settings);
    assert_ne!(world.get(target), 0);
    for _ in 0..16 {
        tick(&host, &mut world, &mut sim, &host_player, &settings);
    }
    assert_eq!(world.get(target), 0);
    assert_eq!(sim.drops.len(), 1);
    assert_eq!(sim.drops[0].stack.id, 3);
    until(&mut wire, |p| p.id == 0x0e);
    sim.drops[0].pickup_delay = 0;
    wire.send(3, Writer::new().bool(true)).unwrap();
    queued(&view, 1);
    tick(&host, &mut world, &mut sim, &host_player, &settings);
    let bag = until(&mut wire, |p| {
        if p.id != 0x30 {
            return false;
        }
        let mut r = Reader::new(&p.data);
        r.u8().unwrap();
        let n = r.i16().unwrap();
        let slots = (0..n).map(|_| r.slot().unwrap()).collect::<Vec<_>>();
        slots[36]
            .as_ref()
            .is_some_and(|s| s.id == 3 && s.count == 1)
    });
    assert!(bag.data.len() > 45);
    // The creative-slot write is ignored and the claimed C08 stack is never authoritative.
    wire.send(0x10, Writer::new().i16(36).slot(Some(&fake)))
        .unwrap();
    place(&mut wire);
    queued(&view, 1);
    let out = tick(&host, &mut world, &mut sim, &host_player, &settings);
    assert!(out.changed.contains(&above));
    assert_eq!(world.get(above), 3 << 4);
    until(&mut wire, |p| expect_block(p, above, 48));
    // The refresh is a full, compressed S21. Decode it using the native client's normal chunk decoder.
    until(&mut wire, |p| {
        if p.id != 0x21 {
            return false;
        }
        let mut r = Reader::new(&p.data);
        let (x, z, full, mask) = (
            r.i32().unwrap(),
            r.i32().unwrap(),
            r.bool().unwrap(),
            r.u16().unwrap(),
        );
        let len = r.varint().unwrap() as usize;
        let chunk =
            crate::network::decode_chunk(x, z, full, mask, true, r.take(len).unwrap()).unwrap();
        let index = |p: IVec3| ((p.y << 8) | (p.z << 4) | p.x) as usize;
        full && x == 0
            && z == 0
            && chunk.blocks[index(above)] == 48
            && chunk.blocks[index(target)] == 0
    });
    assert_eq!(sim.mode, Mode::Creative);
    assert_eq!(sim.inventory.held().unwrap().count, 4);
    wire.send(
        0x0e,
        Writer::new()
            .u8(0)
            .i16(36)
            .u8(0)
            .i16(77)
            .u8(0)
            .slot(Some(&fake)),
    )
    .unwrap();
    queued(&view, 1);
    tick(&host, &mut world, &mut sim, &host_player, &settings);
    let ack = until(&mut wire, |p| p.id == 0x32);
    assert_eq!(ack.data, [0, 0, 77, 0]);

    wire.send(4, Writer::new().f64(130.).f64(42.).f64(0.).bool(true))
        .unwrap();
    queued(&view, 1);
    tick(&host, &mut world, &mut sim, &host_player, &settings);
    until(&mut wire, |p| p.id == 8);
    assert_eq!(
        view.lock().unwrap().users[name].actor.feet,
        DVec3::from(spawn)
    );
    drop(wire);
    handle.join().unwrap();
    tick(&host, &mut world, &mut sim, &host_player, &settings);
    assert!(!sim.server_scene.actors.contains_key(&FIRST_GUEST));
    assert_eq!(sim.lan_guests.len(), 1);
    let save = serde_json::to_value(&sim).unwrap();
    assert!(save["lan_guests"][uuid.replace("-", "")]["inventory"]["slots"][0].is_null());
}
