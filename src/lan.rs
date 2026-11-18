//! Protocol 47 LAN publication, owned by the open world. Dropping the world closes admission.
use crate::{
    gameplay::Simulation,
    network::online_login::{signed_hex, Ciphers},
    player::Player,
    protocol::{self, FrameDecoder, Packet, Reader, Writer},
    world::{Chunk, ChunkPos, World},
    Options,
};
use anyhow::{anyhow, bail, ensure, Context, Result};
use rand::{rngs::OsRng, RngCore};
use rsa::{pkcs8::EncodePublicKey, Pkcs1v15Encrypt, RsaPrivateKey, RsaPublicKey};
use serde_json::json;
use sha1::{Digest, Sha1};
use std::{
    collections::{HashMap, VecDeque},
    io::{ErrorKind, Read, Write},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::{Arc, Mutex, Weak},
    thread,
    time::{Duration, Instant},
};

#[path = "lan_guest.rs"]
mod guest;
use glam::DVec3;
pub(crate) use guest::SavedGuest;
use guest::{Action, Actor, Guest, FIRST_GUEST};
#[derive(Clone)]
pub(crate) struct Host {
    view: Arc<Mutex<View>>,
    _life: Arc<()>,
    pub port: u16,
    pub allow_commands: bool,
}
impl std::fmt::Debug for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Host").field("port", &self.port).finish()
    }
}
struct View {
    label: String,
    owner: String,
    guest_mode: u8,
    hardcore: bool,
    dimension: i8,
    chunks: crate::world::ChunkMap<Arc<Chunk>>,
    spawn: [f64; 3],
    clock: i64,
    total: i64,
    difficulty: u8,
    users: HashMap<String, Guest>,
    host_actor: Actor,
    next_actor: i32,
    departed: Vec<[u8; 16]>,
    drops: HashMap<i32, GroundItem>,
    messages: VecDeque<(u64, String)>,
    host_chat: VecDeque<String>,
    sequence: u64,
}
impl View {
    fn chat(&mut self, line: String) {
        self.sequence = self.sequence.wrapping_add(1);
        self.messages.push_back((self.sequence, line));
        while self.messages.len() > 100 {
            self.messages.pop_front();
        }
    }
}
type PendingSign = (usize, String, glam::IVec3, [String; 4]);
static PENDING_SIGNS: std::sync::OnceLock<Mutex<VecDeque<PendingSign>>> =
    std::sync::OnceLock::new();
fn pending_signs() -> &'static Mutex<VecDeque<PendingSign>> {
    PENDING_SIGNS.get_or_init(|| Mutex::new(VecDeque::new()))
}
impl Drop for View {
    fn drop(&mut self) {
        let key = std::ptr::from_ref::<Self>(self) as usize;
        if let Ok(mut q) = pending_signs().lock() {
            q.retain(|(k, _, _, _)| *k != key);
        }
    }
}
fn sign_packet(position: [i32; 3], tag: &crate::vanilla::drops::DropNbt) -> Option<Writer> {
    let map = crate::vanilla::item_nbt::compound(tag)?;
    let mut w = Writer::new();
    w.position(position);
    for key in ["Text1", "Text2", "Text3", "Text4"] {
        let raw = match map.get(key) {
            Some(crate::vanilla::drops::DropNbt::String(s)) if !s.is_empty() => s.as_str(),
            _ => "\"\"",
        };
        let json = if serde_json::from_str::<serde_json::Value>(raw).is_ok() {
            raw.to_owned()
        } else {
            serde_json::to_string(raw).unwrap_or_else(|_| "\"\"".into())
        };
        if json.encode_utf16().count() <= 32767 {
            w.string(&json);
        } else {
            w.string("\"\"");
        }
    }
    Some(w)
}
fn sanitize_guest_sign_line(raw: &str) -> String {
    let runs = crate::ui::chat::components::parse(raw, |k| k.to_owned());
    let text: String = runs.into_iter().map(|r| r.text).collect();
    let mut stripped = String::new();
    let mut chars = text.chars();
    let mut units = 0usize;
    while let Some(ch) = chars.next() {
        if ch == '§' {
            let _ = chars.next();
            continue;
        }
        if ch < ' ' || ch == '\u{7f}' {
            continue;
        }
        let len16 = ch.len_utf16();
        if units + len16 > 384 {
            break;
        }
        units += len16;
        stripped.push(ch);
    }
    serde_json::to_string(&stripped).unwrap_or_else(|_| "\"\"".into())
}
pub(crate) fn publish(
    sim: &mut Simulation,
    world: &World,
    player: &Player,
    options: &Options,
) -> Result<u16> {
    publish_with_settings(sim, world, player, options, 0, false)
}
pub(crate) fn publish_with_settings(
    sim: &mut Simulation,
    world: &World,
    player: &Player,
    options: &Options,
    guest_mode: u8,
    allow_commands: bool,
) -> Result<u16> {
    ensure!(sim.lan.is_none(), "Already published");
    ensure!(guest_mode <= 3, "Invalid LAN game mode");
    let listener = TcpListener::bind(("0.0.0.0", 0))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let key = Arc::new(RsaPrivateKey::new(&mut OsRng, 1024)?);
    let view = Arc::new(Mutex::new(View {
        label: world.label.clone(),
        owner: options.username.clone(),
        guest_mode,
        hardcore: sim.hardcore,
        dimension: sim.dimension as i8,
        chunks: world.chunks.clone(),
        spawn: player.precise_position(),
        clock: sim.clock as i64,
        total: sim.total_ticks as i64,
        difficulty: sim.world_difficulty.unwrap_or(2),
        users: HashMap::new(),
        next_actor: FIRST_GUEST,
        departed: Vec::new(),
        drops: Default::default(),
        host_actor: Actor::new(
            1,
            crate::ui::offline_uuid(&options.username),
            &options.username,
            player.precise_position(),
        ),
        messages: VecDeque::new(),
        host_chat: VecDeque::new(),
        sequence: 0,
    }));
    let life = Arc::new(());
    let weak = Arc::downgrade(&life);
    let data = view.clone();
    let label = world.label.clone();
    thread::Builder::new()
        .name("lan-listener".into())
        .spawn(move || {
            let udp = UdpSocket::bind(("0.0.0.0", 0)).ok();
            let mut next = Instant::now();
            let sessions = Arc::new(());
            while weak.strong_count() > 0 {
                if Instant::now() >= next {
                    if let Some(socket) = &udp {
                        let _ = socket.send_to(
                            format!("[MOTD]{label}[/MOTD][AD]{port}[/AD]").as_bytes(),
                            ("224.0.2.60", 4445),
                        );
                    }
                    next = Instant::now() + Duration::from_millis(1500);
                }
                match listener.accept() {
                    Ok((stream, _)) if Arc::strong_count(&sessions) <= 16 => {
                        let (key, data, weak, slot) =
                            (key.clone(), data.clone(), weak.clone(), sessions.clone());
                        let _ = thread::Builder::new()
                            .name("lan-client".into())
                            .spawn(move || {
                                let _slot = slot;
                                let _ = connection(stream, &key, data, weak);
                            });
                    }
                    Ok(_) => (),
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(25))
                    }
                    Err(_) => break,
                }
            }
        })?;
    sim.lan = Some(Host {
        view,
        _life: life,
        port,
        allow_commands,
    });
    Ok(port)
}
impl Host {
    pub(crate) fn has_guest(&self, name: &str) -> bool {
        self.view.lock().unwrap().users.contains_key(name)
    }
    pub fn update(
        &self,
        world: &mut World,
        sim: &mut Simulation,
        player: &Player,
        settings: &crate::settings::Settings,
        resources: &crate::resources::Resources,
        options: &Options,
        out: &mut crate::gameplay::Outcome,
    ) {
        let Ok(mut v) = self.view.lock() else { return };
        v.dimension = sim.dimension as i8;
        v.clock = sim.clock as i64;
        v.total = sim.total_ticks as i64;
        v.difficulty = sim.world_difficulty.unwrap_or(2);
        v.host_actor.feet = player.precise_position().into();
        v.host_actor.look = player.java_rotation();
        v.host_actor.mode = sim
            .commands
            .player_mode
            .unwrap_or(u8::from(sim.mode == crate::gameplay::Mode::Creative));
        v.host_actor.ground = player.grounded;
        v.host_actor.bed = sim.sleep.head;
        v.host_actor.flags = u8::from(sim.player_burning())
            | (u8::from(player.sneaking()) << 1)
            | (u8::from(player.sprinting) << 3)
            | (u8::from(player.using_item) << 4)
            | (u8::from(player.is_spectator() || sim.effects.get(14).is_some()) << 5);
        v.host_actor.hand = sim
            .inventory
            .held()
            .as_ref()
            .and_then(|s| crate::remote::to_slot(s).ok());
        let mut saves = std::mem::take(&mut sim.lan_guests);
        let mut pickups = guest::PickupEvents {
            online: v
                .users
                .keys()
                .cloned()
                .chain(Some(v.owner.clone()))
                .collect(),
            collected: out.collected_items.iter().map(|id| (*id, 1)).collect(),
            stats: std::mem::take(&mut out.guest_item_stats),
            sounds: out
                .pitched_sounds
                .iter()
                .filter(|s| matches!(s.0, "random/pop" | "random.pop"))
                .map(|(_, volume, pitch)| (player.precise_position().into(), *volume, *pitch))
                .chain(
                    out.world_sounds
                        .iter()
                        .filter(|s| matches!(s.0, "random/pop" | "random.pop"))
                        .map(|(_, pos, volume, pitch)| (*pos, *volume, *pitch)),
                )
                .collect(),
        };
        let start = out.changed.len();
        for uuid in std::mem::take(&mut v.departed) {
            if let Some(save) = saves.get_mut(&uuid_text(uuid)) {
                save.disconnected(sim);
            }
        }
        for guest in v.users.values_mut() {
            let key = uuid_text(guest.actor.uuid);
            let host_pending_sign = sim.pending_sign_edit.take();
            guest.tick(
                world,
                sim,
                settings,
                resources,
                options,
                self.allow_commands,
                out,
                player,
                saves.entry(key).or_default(),
                &mut pickups,
            );
            if let Some(sign_pos) = sim.pending_sign_edit.take() {
                let mut w = Writer::new();
                w.position(sign_pos.to_array());
                guest.send(0x36, w);
            }
            sim.pending_sign_edit = host_pending_sign;
            let a = &guest.actor;
            let feet = a.feet.as_vec3();
            let slim = a.uuid.chunks_exact(4).fold(0, |sum, n| sum ^ n[3]) & 1 != 0;
            let render =
                sim.server_scene
                    .actors
                    .entry(a.id)
                    .or_insert_with(|| {
                        let mut anim = crate::player::third_person::Animation::default();
                        anim.slim = slim;
                        crate::models::server::Actor {
                            anim,
                            previous: feet,
                            feet,
                            previous_yaw: a.look[0],
                            yaw: a.look[0],
                            previous_pitch: a.look[1],
                            pitch: a.look[1],
                            skin: 127,
                            alex: slim,
                            name: Some(a.name.clone()),
                            ..Default::default()
                        }
                    });
            let delta = (feet - render.feet).as_dvec3();
            render.previous = render.feet;
            render.gait.advance(render.yaw.to_radians(), (feet - render.feet).with_y(0.).length());
            render.feet = feet;
            render.previous_yaw = render.yaw;
            render.yaw = a.look[0];
            render.previous_pitch = render.pitch;
            render.pitch = a.look[1];
            render.anim.slim = slim;
            render.anim.tick(delta, a.look[0]);
            render.swing_anim.advance(6);
            render.name = Some(a.name.clone());
            render.held = a
                .hand
                .as_ref()
                .and_then(|s| crate::remote::from_slot(s).ok());
        }
        sim.server_scene.actors.retain(|id, _| {
            *id < FIRST_GUEST || v.users.values().any(|guest| guest.actor.id == *id)
        });
        for (source, volume, pitch) in pickups.sounds {
            for user in v
                .users
                .values_mut()
                .filter(|g| g.actor.feet.distance_squared(source) < 16. * 16.)
            {
                let mut packet = Writer::new();
                packet.string("random.pop");
                for n in source.to_array() {
                    packet.i32((n * 8.) as i32);
                }
                packet.f32(volume).u8((pitch * 63.) as i32 as u8);
                user.send(0x29, packet);
            }
        }
        for (id, collector) in pickups.collected {
            let id = drop_id(id);
            if let Some(feet) = v.drops.get(&id).map(|item| item.feet) {
                for user in v
                    .users
                    .values_mut()
                    .filter(|g| g.actor.feet.distance_squared(feet) < 64. * 64.)
                {
                    let mut packet = Writer::new();
                    packet.varint(id).varint(collector);
                    user.send(0x0d, packet);
                }
            }
        }
        for (name, stat) in pickups.stats {
            if name == v.owner {
                crate::gameplay::local_commands::award_pickup(&mut sim.statistics, &stat);
            } else if let Some(guest) = v.users.get(&name) {
                if let Some(save) = saves.get_mut(&uuid_text(guest.actor.uuid)) {
                    save.record_stat(&stat);
                }
            }
        }
        sim.lan_guests = saves;
        let view_key = std::ptr::from_ref::<View>(&*v) as usize;
        let sign_edits = {
            let mut q = pending_signs().lock().unwrap_or_else(|e| e.into_inner());
            let mut matched = Vec::new();
            let mut idx = 0;
            while idx < q.len() {
                if q[idx].0 == view_key {
                    matched.push(q.remove(idx).unwrap());
                } else {
                    idx += 1;
                }
            }
            matched
        };
        for (_, guest_name, pos, raw_lines) in sign_edits {
            let can_edit = v.users.get(&guest_name).is_some_and(|g| {
                g.actor.mode != 3
                    && g.actor
                        .feet
                        .distance_squared(pos.as_dvec3() + DVec3::splat(0.5))
                        < 64.0 * 64.0
            });
            if !can_edit || !(0..256).contains(&pos.y) {
                continue;
            }
            let state = world.get(pos);
            if !matches!(state >> 4, 63 | 68) {
                continue;
            }
            let Some(base) =
                crate::vanilla::middle_pick::tile_snapshot(state >> 4, pos, world.tile(pos), None)
            else {
                continue;
            };
            let sanitized = raw_lines.map(|line| sanitize_guest_sign_line(&line));
            if let Ok(updated) = crate::vanilla::tile_updates::apply_sign(&base, true, sanitized) {
                sim.world_commands.edit_tile(world, pos, state, Some(updated));
                out.changed.push(pos);
                if let Some(tag) = world.tile(pos) {
                    if let Some(pkt) = sign_packet(pos.to_array(), tag) {
                        let (cx, cz) = (pos.x.div_euclid(16), pos.z.div_euclid(16));
                        for user in v.users.values_mut() {
                            let (gx, gz) = (
                                user.actor.feet.x.floor() as i32 >> 4,
                                user.actor.feet.z.floor() as i32 >> 4,
                            );
                            if (gx - cx).abs() <= 4 && (gz - cz).abs() <= 4 {
                                user.send(0x33, Writer(pkt.0.clone()));
                            }
                        }
                    }
                }
            }
        }
        for &p in &out.changed[start..] {
            sim.block_ticks.notify(world, p);
        }
        // EntityTracker starts watching newly spawned stacks at their throw
        // pose. The first world update then advances the independent item.
        let births: HashMap<_, _> = sim
            .drops
            .iter()
            .filter(|d| d.ticks == 0 && !d.dead)
            .map(|d| (d.id, (d.position, d.velocity)))
            .collect();
        sim.tick_drop_motion(world, out);
        v.drops.clear();
        for d in &sim.drops {
            if let Ok(item) = crate::remote::to_slot(&d.stack) {
                let (feet, velocity) = births
                    .get(&d.id)
                    .copied()
                    .unwrap_or((d.position, d.velocity));
                v.drops.insert(
                    drop_id(d.id),
                    GroundItem {
                        feet,
                        velocity,
                        yaw: d.yaw,
                        ticks: d.ticks,
                        grounded: d.grounded,
                        slot: item,
                    },
                );
            }
        }
        if sim.total_ticks % 10 == 0 || out.changed.len() != start {
            v.chunks.clone_from(&world.chunks);
        }
    }
    pub fn host_say(&self, name: &str, text: &str) {
        if let Ok(mut v) = self.view.lock() {
            v.chat(format!("<{name}> {text}"));
        }
    }
    pub fn drain(&self) -> Vec<String> {
        self.view
            .lock()
            .map(|mut v| v.host_chat.drain(..).collect())
            .unwrap_or_default()
    }
}
struct Wire {
    stream: TcpStream,
    decoder: FrameDecoder,
    cipher: Option<Ciphers>,
}
impl Wire {
    fn send(&mut self, id: i32, p: &Writer) -> Result<()> {
        let mut bytes = protocol::frame(id, &p.0, self.decoder.compression)?;
        if let Some(c) = &mut self.cipher {
            c.send.encrypt(&mut bytes);
        }
        self.stream.write_all(&bytes)?;
        Ok(())
    }
    fn poll(&mut self) -> Result<Option<Packet>> {
        if let Some(p) = self.decoder.next()? {
            return Ok(Some(p));
        }
        let mut buf = [0; 8192];
        match self.stream.read(&mut buf) {
            Ok(0) => bail!("Peer closed"),
            Ok(n) => {
                if let Some(c) = &mut self.cipher {
                    c.receive.decrypt(&mut buf[..n]);
                }
                self.decoder.feed(&buf[..n])?;
                self.decoder.next()
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    fn next(&mut self) -> Result<Packet> {
        let start = Instant::now();
        loop {
            if let Some(p) = self.poll()? {
                return Ok(p);
            }
            ensure!(
                start.elapsed() < Duration::from_secs(10),
                "Handshake timed out"
            )
        }
    }
}
fn connection(
    stream: TcpStream,
    key: &RsaPrivateKey,
    view: Arc<Mutex<View>>,
    life: Weak<()>,
) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_millis(100)))?;
    stream.set_write_timeout(Some(Duration::from_secs(4)))?;
    stream.set_nodelay(true)?;
    let mut w = Wire {
        stream,
        decoder: Default::default(),
        cipher: None,
    };
    let p = w.next()?;
    ensure!(p.id == 0, "Invalid handshake");
    let mut r = Reader::new(&p.data);
    let version = r.varint()?;
    r.string(255)?;
    r.u16()?;
    let state = r.varint()?;
    if state == 1 {
        ensure!(w.next()?.id == 0, "Invalid status request");
        let value = {
            let v = view.lock().unwrap();
            json!({"version":{"name":"1.8.9","protocol":47},"players":{"max":8,"online":1+v.users.len()},"description":{"text":v.label}})
        };
        w.send(0, Writer::new().string(&value.to_string()))?;
        let p = w.next()?;
        ensure!(p.id == 1 && p.data.len() == 8, "Invalid ping");
        return w.send(1, &Writer(p.data));
    }
    ensure!(state == 2 && version == 47, "Protocol 47 required");
    let p = w.next()?;
    ensure!(p.id == 0, "Missing login start");
    let name = Reader::new(&p.data).string(16)?;
    ensure!(
        !name.is_empty() && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'),
        "Invalid username"
    );
    let der = RsaPublicKey::from(key).to_public_key_der()?;
    let mut token = [0; 4];
    OsRng.fill_bytes(&mut token);
    w.send(
        1,
        Writer::new()
            .string("")
            .varint(der.as_ref().len() as i32)
            .bytes(der.as_ref())
            .varint(4)
            .bytes(&token),
    )?;
    let p = w.next()?;
    ensure!(p.id == 1, "Missing encryption response");
    let mut r = Reader::new(&p.data);
    let n = r.count(256)?;
    let secret: [u8; 16] = key
        .decrypt(Pkcs1v15Encrypt, r.take(n)?)?
        .try_into()
        .map_err(|_| anyhow!("Invalid secret"))?;
    let n = r.count(256)?;
    ensure!(
        key.decrypt(Pkcs1v15Encrypt, r.take(n)?)? == token,
        "Invalid challenge"
    );
    r.finish()?;
    w.cipher = Some(Ciphers::new(&secret));
    let hash = signed_hex(
        Sha1::new()
            .chain_update(secret)
            .chain_update(der.as_ref())
            .finalize()
            .into(),
    );
    let profile = || -> Result<String> {
        let agent = ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(10)))
            .build()
            .new_agent();
        let mut response = agent
            .get("https://sessionserver.mojang.com/session/minecraft/hasJoined")
            .query("username", &name)
            .query("serverId", &hash)
            .call()?;
        let data: serde_json::Value = serde_json::from_str(&response.body_mut().read_to_string()?)?;
        let id = data["id"].as_str().context("Unknown account")?;
        ensure!(
            data["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(&name))
                && id.len() == 32
                && id.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid profile"
        );
        Ok(format!(
            "{}-{}-{}-{}-{}",
            &id[..8],
            &id[8..12],
            &id[12..16],
            &id[16..20],
            &id[20..]
        ))
    };
    let uuid = match profile() {
        Ok(id) => id,
        Err(e) => {
            w.send(
                0,
                Writer::new()
                    .string(r#"{"translate":"multiplayer.disconnect.unverified_username"}"#),
            )?;
            return Err(e);
        }
    };
    admitted(&mut w, &name, &uuid, &view, life)
}

fn uuid_text(id: [u8; 16]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}
fn decode_uuid(uuid: &str) -> Result<[u8; 16]> {
    let hex = uuid.replace('-', "");
    ensure!(hex.len() == 32 && hex.is_ascii(), "Invalid UUID");
    let mut id = [0; 16];
    for (i, v) in id.iter_mut().enumerate() {
        *v = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)?;
    }
    Ok(id)
}
// Called only after the session service has verified the account.
fn admitted(
    w: &mut Wire,
    name: &str,
    uuid: &str,
    view: &Mutex<View>,
    life: Weak<()>,
) -> Result<()> {
    let profile = decode_uuid(uuid)?;
    let (own, spawn, level, seq, mode, hardcore, dimension) = {
        let mut v = view.lock().unwrap();
        ensure!(
            !name.eq_ignore_ascii_case(&v.owner)
                && v.users.len() < 7
                && v.users
                    .values()
                    .all(|g| g.actor.uuid != profile && !g.actor.name.eq_ignore_ascii_case(name)),
            "Duplicate or full"
        );
        let own = v.next_actor;
        v.next_actor = v
            .next_actor
            .checked_add(1)
            .context("Entity ids exhausted")?;
        let spawn = v.spawn;
        let mode = v.guest_mode;
        let mut actor = Actor::new(own, profile, name, spawn);
        actor.mode = mode;
        v.users.insert(name.to_owned(), Guest::new(actor));
        (
            own,
            spawn,
            v.difficulty,
            v.sequence,
            mode,
            v.hardcore,
            v.dimension,
        )
    };
    let result = play(
        w, name, uuid, own, spawn, level, seq, mode, hardcore, dimension, view, life,
    );
    let mut v = view.lock().unwrap();
    v.users.remove(name);
    v.departed.push(profile);
    result
}
fn terrain(w: &mut Wire, pos: ChunkPos, c: &Chunk, dimension: i8) -> Result<()> {
    let mask = (0..16).fold(0u16, |mask, i| {
        if c.blocks[i * 4096..(i + 1) * 4096].iter().any(|&b| b != 0) {
            mask | (1 << i)
        } else {
            mask
        }
    });
    let sections: Vec<_> = (0..16).filter(|i| mask & (1u16 << i) != 0).collect();
    let skies: &[bool] = if dimension == 0 { &[false, true] } else { &[false] };
    let mut data = Vec::with_capacity(sections.len() * (8192 + skies.len() * 2048) + 256);
    for i in &sections {
        for s in &c.blocks[i * 4096..(i + 1) * 4096] {
            data.extend(s.to_le_bytes())
        }
    }
    for &sky in skies {
        for i in &sections {
            for j in (i * 4096..(i + 1) * 4096).step_by(2) {
                let nibble = |n: usize| {
                    c.light.as_ref().map_or(if sky { 15 } else { 0 }, |l| {
                        if sky {
                            l.values[n] >> 4
                        } else {
                            l.values[n] & 15
                        }
                    })
                };
                data.push(nibble(j) | (nibble(j + 1) << 4));
            }
        }
    }
    data.extend(c.biomes);
    w.send(
        0x21,
        Writer::new()
            .i32(pos.0)
            .i32(pos.1)
            .bool(true)
            .u16(mask)
            .varint(data.len() as i32)
            .bytes(&data),
    )?;
    // S21 only carries block states. TileEntityBanner sends S35; TileEntitySign sends S33.
    for (&[x, y, z], tag) in &c.tiles {
        let id = c.get(x, y, z) >> 4;
        if matches!(id, 176 | 177) {
            let position = [pos.0 * 16 + x, y, pos.1 * 16 + z];
            let data = crate::vanilla::item_nbt::encode(tag)?;
            w.send(0x35, Writer::new().position(position).u8(6).bytes(&data))?;
        } else if matches!(id, 63 | 68) {
            let position = [pos.0 * 16 + x, y, pos.1 * 16 + z];
            if let Some(pkt) = sign_packet(position, tag) {
                w.send(0x33, &pkt)?;
            }
        }
    }
    Ok(())
}
fn play(
    w: &mut Wire,
    name: &str,
    uuid: &str,
    own: i32,
    spawn: [f64; 3],
    level: u8,
    mut seq: u64,
    mode: u8,
    hardcore: bool,
    mut dimension: i8,
    view: &Mutex<View>,
    life: Weak<()>,
) -> Result<()> {
    w.send(3, Writer::new().varint(256))?;
    w.decoder.compression = Some(256);
    w.send(2, Writer::new().string(uuid).string(name))?;
    w.send(
        1,
        Writer::new()
            .i32(own)
            .u8(mode | if hardcore { 8 } else { 0 })
            .u8(dimension as u8)
            .u8(level)
            .u8(8)
            .string("default")
            .bool(false),
    )?;
    w.send(0x39, &abilities(mode, mode == 3))?;
    w.send(5, Writer::new().position(spawn.map(|p| p.floor() as i32)))?;
    w.send(
        8,
        Writer::new()
            .f64(spawn[0])
            .f64(spawn[1])
            .f64(spawn[2])
            .f32(0.)
            .f32(0.)
            .u8(0),
    )?;
    w.send(
        0x38,
        Writer::new()
            .varint(0)
            .varint(1)
            .bytes(&decode_uuid(uuid)?)
            .string(name)
            .varint(0)
            .varint(i32::from(mode))
            .varint(0)
            .bool(false),
    )?;
    let mut at: [f64; 3];
    let mut seen = HashMap::new();
    let mut items = HashMap::new();
    let mut revision = u64::MAX;
    w.stream.set_read_timeout(Some(Duration::from_millis(30)))?;
    let mut loaded: HashMap<ChunkPos, Arc<Chunk>> = HashMap::new();
    let mut next = Instant::now();
    let mut last = Instant::now();
    let mut keep = 0;
    while life.strong_count() > 0 && last.elapsed() < Duration::from_secs(45) {
        if Instant::now() >= next {
            {
                let (actors, ground, inventory, replies) = {
                    let mut v = view.lock().unwrap();
                    let actors = std::iter::once(v.host_actor.clone())
                        .chain(v.users.values().map(|g| g.actor.clone()))
                        .collect::<Vec<_>>();
                    let g = v.users.get_mut(name).context("Disconnected player")?;
                    at = g.actor.feet.to_array();
                    let inv = if revision != g.revision && !g.inventory.is_empty() {
                        revision = g.revision;
                        Some((g.inventory.clone(), g.cursor.clone()))
                    } else {
                        None
                    };
                    let replies = std::mem::take(&mut g.replies);
                    (actors, v.drops.clone(), inv, replies)
                };
                for (id, reply) in replies {
                    w.send(id, &reply)?;
                }
                if let Some((slots, cursor)) = inventory {
                    let mut packet = Writer::new();
                    packet.u8(0).i16(slots.len() as i16);
                    for item in &slots {
                        packet.slot(item.as_ref());
                    }
                    w.send(0x30, &packet)?;
                    w.send(0x2f, Writer::new().u8(255).i16(-1).slot(cursor.as_ref()))?;
                }
                actors_update(w, own, at.into(), &mut seen, actors)?;
                items_update(w, at.into(), &mut items, ground)?;
            }
            let (cur_dim, cur_diff, cur_mode, clock, total, mut chunks, lines) = {
                let v = view.lock().unwrap();
                let dim_changed = v.dimension != dimension;
                let center = (at[0].floor() as i32 >> 4, at[2].floor() as i32 >> 4);
                let chunks: Vec<_> = v
                    .chunks
                    .iter()
                    .filter(|(p, c)| {
                        (p.0 - center.0).abs() <= 4
                            && (p.1 - center.1).abs() <= 4
                            && (dim_changed || !loaded.get(p).is_some_and(|old| Arc::ptr_eq(old, c)))
                    })
                    .map(|(p, c)| (*p, c.clone()))
                    .collect();
                (
                    v.dimension,
                    v.difficulty,
                    v.users.get(name).map_or(mode, |g| g.actor.mode),
                    v.clock,
                    v.total,
                    chunks,
                    v.messages
                        .iter()
                        .filter(|(s, _)| *s > seq)
                        .cloned()
                        .collect::<Vec<_>>(),
                )
            };
            if cur_dim != dimension {
                dimension = cur_dim;
                loaded.clear();
                seen.clear();
                items.clear();
                w.send(
                    0x07,
                    Writer::new()
                        .i32(i32::from(dimension))
                        .u8(cur_diff)
                        .u8(cur_mode)
                        .string("default"),
                )?;
            }
            let center = (at[0].floor() as i32 >> 4, at[2].floor() as i32 >> 4);
            let unload = loaded
                .keys()
                .copied()
                .filter(|p| (p.0 - center.0).abs() > 4 || (p.1 - center.1).abs() > 4)
                .collect::<Vec<_>>();
            for p in unload {
                loaded.remove(&p);
                w.send(
                    0x21,
                    Writer::new().i32(p.0).i32(p.1).bool(true).u16(0).varint(0),
                )?;
            }
            chunks.sort_by_key(|(p, _)| {
                ((p.0 - (at[0].floor() as i32 >> 4)).abs()
                    + (p.1 - (at[2].floor() as i32 >> 4)).abs(), *p)
            });
            for (p, c) in chunks.into_iter().take(8) {
                terrain(w, p, &c, dimension)?;
                loaded.insert(p, c);
            }
            for (s, text) in lines {
                seq = s;
                w.send(
                    2,
                    Writer::new()
                        .string(&json!({"text":text}).to_string())
                        .u8(0),
                )?;
            }
            if keep % 20 == 0 {
                w.send(3, Writer::new().i64(total).i64(clock))?;
            }
            if keep % 40 == 0 {
                w.send(0, Writer::new().varint(keep))?
            }
            keep = (keep + 1) % 20000;
            next = Instant::now() + Duration::from_millis(50);
        }
        if let Some(p) = w.poll()? {
            last = Instant::now();
            let mut r = Reader::new(&p.data);
            match p.id {
                1 => {
                    let text = r.string(100)?;
                    if text.starts_with('/') {
                        let mut v = view.lock().unwrap();
                        let g = v.users.get_mut(name).context("Disconnected")?;
                        ensure!(g.inputs.len() < 128, "Too many pending inputs");
                        g.inputs.push_back(Action::Command(text));
                    } else if !text.chars().any(|c| c < ' ' || c == '§') {
                        let line = format!("<{name}> {text}");
                        let mut v = view.lock().unwrap();
                        if v.host_chat.len() < 100 {
                            v.host_chat.push_back(line.clone());
                        }
                        v.chat(line);
                    }
                }
                3..=9 | 0x0a | 0x0b | 0x0d | 0x0e | 0x0f | 0x10 | 0x13 | 0x14 | 0x16 => {
                    let action = match p.id {
                        3..=6 => {
                            let pos = matches!(p.id, 4 | 6)
                                .then(|| {
                                    Ok::<_, anyhow::Error>(DVec3::new(r.f64()?, r.f64()?, r.f64()?))
                                })
                                .transpose()?;
                            let look = matches!(p.id, 5 | 6)
                                .then(|| Ok::<_, anyhow::Error>([r.f32()?, r.f32()?]))
                                .transpose()?;
                            ensure!(
                                pos.is_none_or(
                                    |p| p.is_finite() && p.abs().max_element() <= 30_000_000.
                                ) && look.is_none_or(|r| r.iter().all(|v| v.is_finite())),
                                "Invalid pose"
                            );
                            Action::Move(pos, look, r.bool()?)
                        }
                        7 => {
                            let s = r.u8()?;
                            let pos = r.position()?.into();
                            r.u8()?;
                            Action::Dig(s, pos)
                        }
                        8 => {
                            let pos = r.position()?.into();
                            let face = r.u8()?;
                            r.slot()?;
                            let offsets = [r.u8()?, r.u8()?, r.u8()?];
                            Action::Place(pos, face, offsets)
                        }
                        9 => {
                            let i = r.i16()?;
                            ensure!((0..9).contains(&i), "Invalid hotbar");
                            Action::Select(i as usize)
                        }
                        0x0a => Action::Swing,
                        0x16 => match r.varint()? {
                            1 => Action::RequestStats,
                            2 => Action::OpenInventory,
                            _ => continue,
                        },
                        0x0b => {
                            r.varint()?;
                            let action = r.varint()?;
                            r.varint()?;
                            if !matches!(action, 0 | 1) {
                                continue;
                            }
                            Action::Sneak(action == 0)
                        }
                        0x10 => Action::CreativeSlot(r.i16()?, r.slot()?),
                        0x13 => {
                            let flags = r.u8()?;
                            r.f32()?;
                            r.f32()?;
                            Action::Flight(flags & 2 != 0)
                        }
                        0x14 => {
                            let input = r.string(32767)?;
                            if r.bool()? {
                                r.position()?;
                            }
                            Action::Complete(input)
                        }
                        0x0f => {
                            let window = r.u8()?;
                            let tx = r.i16()?;
                            r.bool()?;
                            Action::Confirm(window, tx)
                        }
                        0x0d => {
                            let id = r.u8()?;
                            if id != 0 {
                                continue;
                            }
                            Action::Close
                        }
                        _ => {
                            Action::Click(r.u8()?, r.i16()?, r.u8()?, r.i16()?, r.u8()?, r.slot()?)
                        }
                    };
                    r.finish()?;
                    let mut v = view.lock().unwrap();
                    let g = v.users.get_mut(name).context("Disconnected")?;
                    ensure!(g.inputs.len() < 128, "Too many pending inputs");
                    g.inputs.push_back(action);
                }
                0x12 => {
                    let (pos, lines) = crate::vanilla::tile_updates::decode_sign(&mut r)?;
                    let key = {
                        let v = view.lock().unwrap();
                        ensure!(v.users.contains_key(name), "Disconnected");
                        std::ptr::from_ref::<View>(&*v) as usize
                    };
                    let mut q = pending_signs().lock().unwrap_or_else(|e| e.into_inner());
                    ensure!(q.len() < 128, "Too many pending sign updates");
                    q.push_back((key, name.to_owned(), pos, lines));
                }
                _ => (),
            }
        }
    }
    Ok(())
}
#[derive(Clone, PartialEq)]
struct GroundItem {
    feet: DVec3,
    velocity: DVec3,
    yaw: f32,
    ticks: i32,
    grounded: bool,
    slot: protocol::Slot,
}
fn drop_id(n: u64) -> i32 {
    -1 - (n & 0x3fff_ffff) as i32
}
fn angle(v: f32) -> u8 {
    ((v * 256. / 360.) as i32) as u8
}
fn abilities(mode: u8, flying: bool) -> Writer {
    let mut w = Writer::new();
    let flags = match mode {
        1 => 13 | if flying { 2 } else { 0 },
        3 => 7,
        _ => 0,
    };
    w.u8(flags).f32(0.05).f32(0.1);
    w
}
fn player_list_mode(actor: &Actor) -> Writer {
    let mut w = Writer::new();
    w.varint(1)
        .varint(1)
        .bytes(&actor.uuid)
        .varint(i32::from(actor.mode));
    w
}
fn xyz(w: &mut Writer, p: DVec3) -> &mut Writer {
    for n in p.to_array() {
        w.i32((n * 32.).floor() as i32);
    }
    w
}
fn teleport(w: &mut Wire, id: i32, feet: DVec3, look: [f32; 2], ground: bool) -> Result<()> {
    let mut p = Writer::new();
    p.varint(id);
    xyz(&mut p, feet)
        .u8(angle(look[0]))
        .u8(angle(look[1]))
        .bool(ground);
    w.send(0x18, &p)
}
fn actors_update(
    w: &mut Wire,
    own: i32,
    feet: DVec3,
    seen: &mut HashMap<i32, Actor>,
    actors: Vec<Actor>,
) -> Result<()> {
    let actors = actors
        .into_iter()
        .filter(|a| a.id != own && feet.distance_squared(a.feet) < 128. * 128.)
        .map(|a| (a.id, a))
        .collect::<HashMap<_, _>>();
    for (id, old) in seen.iter() {
        if !actors.contains_key(id) {
            w.send(0x13, Writer::new().varint(1).varint(*id))?;
            w.send(0x38, Writer::new().varint(4).varint(1).bytes(&old.uuid))?;
        }
    }
    for (id, a) in &actors {
        let old = seen.get(id);
        if old.is_some_and(|o| o.mode != a.mode) {
            w.send(0x38, &player_list_mode(a))?;
        }
        if old.is_none() {
            w.send(
                0x38,
                Writer::new()
                    .varint(0)
                    .varint(1)
                    .bytes(&a.uuid)
                    .string(&a.name)
                    .varint(0)
                    .varint(i32::from(a.mode))
                    .varint(0)
                    .bool(false),
            )?;
            let mut p = Writer::new();
            p.varint(*id).bytes(&a.uuid);
            xyz(&mut p, a.feet)
                .u8(angle(a.look[0]))
                .u8(angle(a.look[1]))
                .i16(a.hand.as_ref().map_or(0, |s| s.id))
                .u8(0)
                .u8(a.flags)
                .u8(10)
                .u8(127)
                .u8(127);
            w.send(0x0c, &p)?;
        } else if old.is_some_and(|o| o.feet != a.feet || o.look != a.look || o.ground != a.ground)
        {
            teleport(w, *id, a.feet, a.look, a.ground)?;
        }
        if old.is_some_and(|o| o.flags != a.flags) {
            w.send(0x1c, Writer::new().varint(*id).u8(0).u8(a.flags).u8(127))?;
        }
        if old.and_then(|o| o.bed) != a.bed {
            if let Some(head) = a.bed {
                w.send(0x0a, Writer::new().varint(*id).position(head.to_array()))?;
            } else {
                w.send(0x0b, Writer::new().varint(*id).u8(2))?;
            }
        }
        if old.is_none_or(|o| o.look[0] != a.look[0]) {
            w.send(0x19, Writer::new().varint(*id).u8(angle(a.look[0])))?;
        }
        if old.map(|o| o.hand.as_ref()) != Some(a.hand.as_ref()) {
            w.send(4, Writer::new().varint(*id).i16(0).slot(a.hand.as_ref()))?;
        }
    }
    *seen = actors;
    Ok(())
}
fn items_update(
    w: &mut Wire,
    feet: DVec3,
    seen: &mut HashMap<i32, GroundItem>,
    mut items: HashMap<i32, GroundItem>,
) -> Result<()> {
    items.retain(|_, i| feet.distance_squared(i.feet) < 64. * 64.);
    for id in seen.keys().filter(|id| !items.contains_key(id)) {
        w.send(0x13, Writer::new().varint(1).varint(*id))?;
    }
    seen.retain(|id, _| items.contains_key(id));
    for (id, item) in items {
        let quantize = |v: f64| (v * 32.).floor() as i32;
        let motion = |w: &mut Writer, v: DVec3| {
            for n in v.to_array() {
                w.i16((n.clamp(-3.9, 3.9) * 8000.) as i16);
            }
        };
        let meta = |w: &mut Wire| -> Result<()> {
            w.send(
                0x1c,
                Writer::new()
                    .varint(id)
                    .u8(0xaa)
                    .slot(Some(&item.slot))
                    .u8(127),
            )
        };
        let Some(last) = seen.get_mut(&id) else {
            let mut p = Writer::new();
            p.varint(id).u8(2);
            xyz(&mut p, item.feet).u8(0).u8(angle(item.yaw)).i32(1);
            motion(&mut p, item.velocity);
            w.send(0x0e, &p)?;
            meta(w)?;
            let mut p = Writer::new();
            p.varint(id);
            motion(&mut p, item.velocity);
            w.send(0x12, &p)?;
            seen.insert(id, item);
            continue;
        };
        let metadata = item.slot != last.slot;
        let elapsed = item.ticks.saturating_sub(last.ticks);
        if elapsed < 20 && !metadata {
            continue;
        }
        let old = last.feet.to_array().map(quantize);
        let new = item.feet.to_array().map(quantize);
        let diff = [new[0] - old[0], new[1] - old[1], new[2] - old[2]];
        let dv = item.velocity.distance_squared(last.velocity);
        if dv > 0.0004 || dv > 0. && item.velocity == DVec3::ZERO {
            let mut p = Writer::new();
            p.varint(id);
            motion(&mut p, item.velocity);
            w.send(0x12, &p)?;
            last.velocity = item.velocity;
        }
        let force =
            last.grounded != item.grounded || diff.iter().any(|n| i8::try_from(*n).is_err());
        if force {
            teleport(w, id, item.feet, [item.yaw, 0.], item.grounded)?;
        } else if diff.iter().any(|n| n.abs() >= 4) || item.ticks / 60 != last.ticks / 60 {
            w.send(
                0x15,
                Writer::new()
                    .varint(id)
                    .u8(diff[0] as u8)
                    .u8(diff[1] as u8)
                    .u8(diff[2] as u8)
                    .bool(item.grounded),
            )?;
        } else {
            if metadata {
                meta(w)?;
                last.slot = item.slot;
            }
            last.ticks = item.ticks;
            continue;
        }
        if metadata {
            meta(w)?;
            last.slot = item.slot;
        }
        last.ticks = item.ticks;
        last.feet = item.feet;
        last.grounded = item.grounded;
    }
    Ok(())
}
#[cfg(test)]
#[path = "lan_tests.rs"]
mod tests;
