//! Protocol-47 transport and a small server-authoritative client session.
//! Encrypted online-mode servers require an authenticated Minecraft Services session.
#![allow(dead_code)]
#[path = "chat_packets.rs"]
mod chat_packets;
#[path = "special_payload.rs"]
pub(crate) mod special_payload;
#[path = "online_login.rs"]
pub(crate) mod online_login;
#[cfg(test)]
#[path = "network_loopback_tests.rs"]
mod loopback_tests;
#[path = "play_events.rs"]
pub mod play_events;
use crate::protocol::{self, FrameDecoder, Packet, Reader, Slot, Writer};
use anyhow::{bail, ensure, Context, Result};
use std::{
    collections::VecDeque,
    io::{ErrorKind, Read, Write},
    net::{Shutdown, TcpStream, ToSocketAddrs},
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// Vanilla degrees: yaw zero faces +Z; positive pitch looks down.
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
}
impl Pose {
    fn validate(self) -> Result<()> {
        ensure!(
            self.x.is_finite()
                && self.y.is_finite()
                && self.z.is_finite()
                && self.yaw.is_finite()
                && self.pitch.is_finite(),
            "Non-finite player pose"
        );
        ensure!(
            self.x.abs() <= 30_000_000.0
                && self.z.abs() <= 30_000_000.0
                && self.y.abs() <= 30_000_000.0,
            "Player pose outside vanilla bounds"
        );
        Ok(())
    }
}

#[derive(Debug)]
pub struct ServerStatus {
    pub json: serde_json::Value,
    pub latency: Duration,
}

#[derive(Clone, Debug)]
pub struct ChunkData {
    pub x: i32,
    pub z: i32,
    pub ground_up: bool,
    pub mask: u16,
    /// Full 16*256*16 block array in (y,z,x) order; omitted sections are zero.
    /// For partial updates only sections selected by mask replace local data.
    pub blocks: Vec<u16>,
    pub block_light: Vec<u8>,
    pub sky_light: Option<Vec<u8>>,
    pub biomes: Option<Vec<u8>>,
}
impl ChunkData {
    pub fn unload(&self) -> bool {
        self.ground_up && self.mask == 0 && self.biomes.is_none()
    }
}

#[derive(Debug)]
pub enum Metadata {
    Byte(i8),
    Short(i16),
    Int(i32),
    Float(f32),
    String(String),
    Slot(Option<Slot>),
    Position([i32; 3]),
    Rotation([f32; 3]),
}
pub fn read_metadata(r: &mut Reader<'_>) -> Result<Vec<(u8, Metadata)>> {
    let mut values = Vec::new();
    let mut seen = 0usize;
    loop {
        let header = r.u8()?;
        if header == 127 {
            return Ok(values);
        }
        seen += 1;
        ensure!(seen <= 64, "Entity metadata exceeds watcher count");
        let index = header & 31;
        let value = match header >> 5 {
            0 => Metadata::Byte(r.i8()?),
            1 => Metadata::Short(r.i16()?),
            2 => Metadata::Int(r.i32()?),
            3 => Metadata::Float(r.f32()?),
            4 => Metadata::String(r.string(32767)?),
            5 => Metadata::Slot(r.slot()?),
            6 => Metadata::Position([r.i32()?, r.i32()?, r.i32()?]),
            7 => Metadata::Rotation([r.f32()?, r.f32()?, r.f32()?]),
            _ => unreachable!(),
        };
        if let Some((_, existing)) = values.iter_mut().find(|(old, _)| *old == index) {
            *existing = value;
        } else {
            values.push((index, value));
        }
    }
}
fn entity_position(r: &mut Reader<'_>) -> Result<[f64; 3]> {
    Ok([
        r.i32()? as f64 / 32.0,
        r.i32()? as f64 / 32.0,
        r.i32()? as f64 / 32.0,
    ])
}
fn entity_angle(r: &mut Reader<'_>) -> Result<f32> {
    Ok(r.i8()? as f32 * 360.0 / 256.0)
}
fn entity_velocity(r: &mut Reader<'_>) -> Result<[f64; 3]> {
    Ok([
        r.i16()? as f64 / 8000.0,
        r.i16()? as f64 / 8000.0,
        r.i16()? as f64 / 8000.0,
    ])
}
#[derive(Debug)]
pub struct PlayerInfo {
    pub uuid: [u8; 16],
    pub name: Option<String>,
    pub properties: Vec<(String, String, Option<String>)>,
    pub mode: Option<i32>,
    pub latency: Option<i32>,
    pub display_name: Option<String>,
}
pub(crate) fn read_player_info(r: &mut Reader<'_>) -> Result<(u8, Vec<PlayerInfo>)> {
    let action = r.count(4)? as u8;
    let count = r.count(r.remaining().len() / 16)?;
    let mut players = Vec::with_capacity(count);
    for _ in 0..count {
        let uuid = r.take(16)?.try_into()?;
        let mut info = PlayerInfo {
            uuid,
            name: None,
            properties: Vec::new(),
            mode: None,
            latency: None,
            display_name: None,
        };
        if action == 0 {
            info.name = Some(r.string(16)?);
            let properties = r.count(r.remaining().len() / 3)?;
            for _ in 0..properties {
                let name = r.string(32767)?;
                let value = r.string(32767)?;
                let signature = if r.bool()? {
                    Some(r.string(32767)?)
                } else {
                    None
                };
                info.properties.push((name, value, signature));
            }
        }
        if action <= 1 {
            let mode = r.varint()?;
            info.mode = Some(if (-1..=3).contains(&mode) { mode } else { 0 });
        }
        if matches!(action, 0 | 2) {
            info.latency = Some(r.varint()?);
        }
        if matches!(action, 0 | 3) && r.bool()? {
            info.display_name = Some(r.string(32767)?);
        }
        players.push(info);
    }
    r.finish()?;
    Ok((action, players))
}
pub(crate) fn write_interaction(w: &mut Writer, id: i32, hit: Option<[f32; 3]>) -> Result<()> {
    w.varint(id).varint(if hit.is_some() { 2 } else { 0 });
    if let Some(hit) = hit {
        for c in hit {
            ensure!(c.is_finite(), "Invalid entity cursor");
            w.f32(c);
        }
    }
    Ok(())
}
pub(crate) fn read_attach(r: &mut Reader<'_>) -> Result<(i32, i32, u8)> {
    let attachment = (r.i32()?, r.i32()?, r.u8()?);
    r.finish()?;
    Ok(attachment)
}
pub(crate) fn write_creative_slot(w: &mut Writer, slot: i16, stack: Option<&Slot>) -> Result<()> {
    ensure!((36..=44).contains(&slot), "Invalid creative hotbar slot");
    w.i16(slot).slot(stack);
    Ok(())
}
pub(crate) fn write_placement(
    w: &mut Writer,
    position: [i32; 3],
    face: u8,
    held: Option<&Slot>,
    cursor: [f32; 3],
) -> Result<()> {
    ensure!(face <= 5 || face == 255, "Invalid block placement face");
    w.position(position).u8(face).slot(held);
    for component in cursor {
        ensure!(component.is_finite(), "Non-finite block cursor");
        // C08 writes a Java int cast to byte. A face-plane intersection can
        // be a few ULPs outside the nominal unit cube.
        w.u8((component * 16.0) as i32 as u8);
    }
    Ok(())
}

#[derive(Debug)]
pub enum Event {
    WorldAction(play_events::Update),
    Presentation(crate::net_presentation::Packet),
    Login {
        uuid: String,
        username: String,
    },
    JoinGame {
        entity_id: i32,
        gamemode: u8,
        dimension: i32,
        difficulty: u8,
        reduced_debug: bool,
        flat_world: bool,
    },
    Respawn {
        dimension: i32,
        gamemode: u8,
        difficulty: u8,
        flat_world: bool,
    },
    Position {
        pose: Pose,
        correction_id: u64,
        relative:u8,
    },
    Chunk(ChunkData),
    BlockChanges(Vec<([i32; 3], u16)>),
    SpawnPosition([i32; 3]),
    Time {
        age: i64,
        time: i64,
    },
    Health {
        health: f32,
        food: i32,
        saturation: f32,
    },
    Experience {
        progress: f32,
        level: i32,
        total: i32,
    },
    HeldSlot(u8),
    Chat {
        json: String,
        position: u8,
    },
    TabComplete(Vec<String>),
    Abilities {
        flags: u8,
        flying_speed: f32,
        walking_speed: f32,
    },
    Properties {
        entity_id: i32,
        properties: Vec<crate::vanilla::movement::attribute::Snapshot>,
    },
    EntityEffect {
        entity_id: i32,
        effect_id: i8,
        amplifier: i8,
        duration: i32,
        show_particles: bool,
        duration_max: bool,
    },
    RemoveEntityEffect {
        entity_id: i32,
        effect_id: u8,
    },
    Velocity {
        entity_id: i32,
        velocity: [f64; 3],
    },
    SpawnMob {
        entity_id: i32,
        kind: u8,
        pose: Pose,
        head_yaw: f32,
        velocity: [f64; 3],
        metadata: Vec<(u8, Metadata)>,
    },
    SpawnPlayer {
        entity_id: i32,
        uuid: [u8; 16],
        pose: Pose,
        held: i16,
        metadata: Vec<(u8, Metadata)>,
    },
    SpawnObject {
        entity_id: i32,
        kind: u8,
        pose: Pose,
        data: i32,
        velocity: Option<[f64; 3]>,
    },
    EntityMove {
        entity_id: i32,
        delta: Option<[f64; 3]>,
        look: Option<[f32; 2]>,
        on_ground: bool,
    },
    EntityTeleport {
        entity_id: i32,
        pose: Pose,
    },
    EntityHeadLook {
        entity_id: i32,
        yaw: f32,
    },
    Attach {
        entity_id: i32,
        vehicle: i32,
        leash: u8,
    },
    PlayerList {
        action: u8,
        players: Vec<PlayerInfo>,
    },
    TileUpdate {
        position: glam::IVec3,
        action: u8,
        tag: crate::vanilla::drops::DropNbt,
    },
    Border(crate::vanilla::click_border::Update),
    SignUpdate {
        position: glam::IVec3,
        lines: [String; 4],
    },
    DestroyEntities(Vec<i32>),
    EntityMetadata {
        entity_id: i32,
        metadata: Vec<(u8, Metadata)>,
    },
    EntityStatus {
        entity_id: i32,
        status: i8,
    },
    EntityEquipment {
        entity_id: i32,
        slot: i16,
        item: Option<Slot>,
    },
    CollectItem {
        collected: i32,
        collector: i32,
    },
    WindowItems {
        window_id: u8,
        items: Vec<Option<Slot>>,
    },
    OpenWindow {
        window_id: u8,
        kind: String,
        title: String,
        slots: u8,
        entity_id: Option<i32>,
    },
    CloseWindow(u8),
    GameState {
        reason: u8,
        value: f32,
    },
    SetSlot {
        window_id: i8,
        slot: i16,
        item: Option<Slot>,
    },
    Transaction {
        window_id: u8,
        action: i16,
        accepted: bool,
    },
    ResourcePack {
        url: String,
        hash: String,
    },
    Disconnect(String),
}

#[derive(Clone, Copy, Debug)]
#[repr(i32)]
pub enum DigAction {
    Start = 0,
    Abort = 1,
    Finish = 2,
    DropStack = 3,
    DropOne = 4,
    ReleaseUse = 5,
}
#[derive(Clone, Copy, Debug)]
#[repr(i32)]
pub enum EntityAction {
    StartSneak = 0,
    StopSneak = 1,
    LeaveBed = 2,
    StartSprint = 3,
    StopSprint = 4,
    HorseJump = 5,
    HorseInventory = 6,
}
#[derive(Debug, Clone)]
pub struct ClientPreferences {
    language: String,
    view_distance: u8,
    chat_visibility: u8,
    chat_colors: bool,
    skin_parts: u8,
}

impl From<&crate::settings::Settings> for ClientPreferences {
    fn from(settings: &crate::settings::Settings) -> Self {
        let locale = &settings.language;
        let mut language = locale.split_once('_').map_or_else(|| locale.clone(), |(name, region)| {
            format!("{name}_{}", region.to_ascii_uppercase())
        });
        if language.chars().count() > 7 {
            language = language.chars().take(7).collect();
        }
        if language.is_empty() {
            language = "en_US".into();
        }
        Self {
            language,
            view_distance: settings.full_detail_distance().clamp(2, 16) as u8,
            chat_visibility: settings.menu.chat_visibility.min(2),
            chat_colors: settings.menu.chat_colors,
            skin_parts: settings.menu.skin_parts,
        }
    }
}
impl ClientPreferences {
    fn packet(&self) -> Writer {
        let mut packet = Writer::new();
        let lang: String = self.language.chars().take(7).collect();
        packet
            .string(if lang.is_empty() { "en_US" } else { &lang })
            .u8(self.view_distance.clamp(2, 16))
            .u8(self.chat_visibility.min(2))
            .bool(self.chat_colors)
            .u8(self.skin_parts);
        packet
    }
}

#[derive(Debug)]
pub enum Command {
    Riding{pose:Pose,correction_id:u64,strafe:f32,forward:f32,jump:bool,sneak:bool},
    Enchant(u8,u8),
    PackStatus(String,u8),
    SelectTrade(i32),
    WindowPayload(special_payload::Payload),
    UpdateSign([i32;3],[String;4]),
    JoinAck,
    TeleportAck {
        pose: Pose,
        correction_id: u64,
    },
    Movement {
        pose: Pose,
        correction_id: u64,
    },
    Chat(String),
    TabComplete { input: String, target: Option<[i32; 3]> },
    HeldSlot(u16),
    Swing,
    EntityAction {
        action: EntityAction,
        parameter: i32,
    },
    Dig {
        action: DigAction,
        position: [i32; 3],
        face: u8,
    },
    Place {
        position: [i32; 3],
        face: u8,
        held: Option<Slot>,
        cursor: [f32; 3],
    },
    Attack(i32),
    Interact {
        entity_id: i32,
        hit: Option<[f32; 3]>,
    },
    ClickWindow {
        window_id: u8,
        slot: i16,
        button: u8,
        action: i16,
        mode: u8,
        clicked: Option<Slot>,
    },
    CreativeSlot {
        slot: i16,
        stack: Option<Slot>,
    },
    CloseWindow(u8),
    ConfirmTransaction{window_id:u8,action:i16},
    Settings(ClientPreferences),
    /// Only changes flight when the server has granted allowFlying.
    Flying(bool),
    Respawn,
    RequestStats,
    InventoryAchievement,
    Spectate([u8;16]),
    Disconnect,
}

pub struct Client {
    commands: SyncSender<Command>,
    events: Receiver<Event>,
    // Kept separate so a full bounded event queue cannot hide its own failure.
    terminal: Receiver<String>,
}
impl Client {
    #[cfg(test)]
    pub(crate) fn window_test_transport() -> (Self, Receiver<Command>) {
        let (client, commands, _) = Self::test_transport();
        (client, commands)
    }
    #[cfg(test)]
    pub(crate) fn test_transport() -> (Self, Receiver<Command>, SyncSender<Event>) {
        let (commands, rx) = mpsc::sync_channel(64);
        let (tx, events) = mpsc::sync_channel(64);
        let (_, terminal) = mpsc::channel();
        (Self { commands, events, terminal }, rx, tx)
    }
    pub fn connect(address: &str, username: &str, preferences: ClientPreferences) -> Result<Self> {
        let profile_name=std::env::var("FERRUM189_MC_USERNAME").ok();
        let username=profile_name.as_deref().unwrap_or(username);
        ensure!(
            !username.is_empty()
                && username.len() <= 16
                && username
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "Use a 1-16 character ASCII Minecraft username"
        );
        let (stream, host, port) = connect_socket(address, Duration::from_secs(5))?;
        stream.set_nonblocking(true)?;
        let (command_tx, command_rx) = mpsc::sync_channel(1024);
        let (event_tx, event_rx) = mpsc::sync_channel(65_536);
        let (terminal_tx, terminal_rx) = mpsc::channel();
        let username = username.to_owned();
        thread::Builder::new()
            .name("minecraft-network".into())
            .spawn(move || {
                let result =
                    Session::new(stream, event_tx.clone(), preferences).run(&host, port, &username, command_rx);
                if let Err(error) = result {
                    let _ = terminal_tx.send(format!("{error:#}"));
                }
            })?;
        Ok(Self {
            commands: command_tx,
            events: event_rx,
            terminal: terminal_rx,
        })
    }
    pub fn send(&self, command: Command) -> Result<()> {
        match self.commands.try_send(command) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Disconnected(_)) => {
                if let Ok(reason) = self.terminal.try_recv() {
                    bail!("{reason}");
                }
                bail!("Network command queue unavailable: channel closed")
            }
            Err(e) => Err(e).context("Network command queue unavailable"),
        }
    }
    pub fn try_event(&self) -> Option<Event> {
        self.events
            .try_recv()
            .ok()
            .or_else(|| self.terminal.try_recv().ok().map(Event::Disconnect))
    }
    pub fn recv_timeout(&self, timeout: Duration) -> Result<Event> {
        match self.events.recv_timeout(timeout) {
            Ok(event) => Ok(event),
            Err(error) => self
                .terminal
                .try_recv()
                .map(Event::Disconnect)
                .map_err(|_| anyhow::anyhow!("Waiting for server event: {error}")),
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.commands.try_send(Command::Disconnect);
    }
}

fn endpoint(address: &str) -> Result<(String, u16, bool)> {
    ensure!(
        !address.is_empty() && address.len() <= 255 && !address.contains(['\0', '/', '\\']),
        "Invalid server address"
    );
    if address.starts_with('[') {
        let end = address.find(']').context("Unclosed IPv6 address")?;
        let host = address[1..end].to_owned();
        let port = if end + 1 == address.len() {
            25565
        } else {
            address[end + 1..]
                .strip_prefix(':')
                .context("Invalid IPv6 port")?
                .parse()?
        };
        return Ok((host, port, true));
    }
    if address.matches(':').count() == 1 {
        let (host, port) = address.rsplit_once(':').unwrap();
        ensure!(!host.is_empty(), "Empty server hostname");
        Ok((host.to_owned(), port.parse()?, true))
    } else {
        Ok((address.to_owned(), 25565, false))
    }
}
#[cfg(target_os = "windows")]
fn resolve_minecraft_srv(host: &str) -> Option<(String, u16)> {
    use std::ffi::c_void;
    if host.eq_ignore_ascii_case("localhost") || host.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    #[repr(C)]
    struct DnsSrvDataW {
        target: *const u16,
        priority: u16,
        weight: u16,
        port: u16,
        pad: u16,
    }
    #[repr(C)]
    struct DnsRecordW {
        next: *mut DnsRecordW,
        name: *mut u16,
        record_type: u16,
        data_length: u16,
        flags: u32,
        ttl: u32,
        reserved: u32,
        srv: DnsSrvDataW,
    }
    #[link(name = "dnsapi")]
    unsafe extern "system" {
        fn DnsQuery_W(
            name: *const u16,
            record_type: u16,
            options: u32,
            extra: *mut c_void,
            results: *mut *mut DnsRecordW,
            reserved: *mut *mut c_void,
        ) -> i32;
        fn DnsRecordListFree(record_list: *mut c_void, free_type: i32);
    }
    let query: Vec<u16> = format!("_minecraft._tcp.{}", host.trim_end_matches('.'))
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut records: *mut DnsRecordW = std::ptr::null_mut();
    let status = unsafe {
        DnsQuery_W(
            query.as_ptr(),
            0x0021,
            0,
            std::ptr::null_mut(),
            &mut records,
            std::ptr::null_mut(),
        )
    };
    if status != 0 || records.is_null() {
        return None;
    }
    let mut best: Option<(u16, u16, String, u16)> = None;
    let mut cur = records;
    unsafe {
        while !cur.is_null() {
            let rec = &*cur;
            if rec.record_type == 0x0021 && !rec.srv.target.is_null() && rec.srv.port != 0 {
                let mut len = 0usize;
                while *rec.srv.target.add(len) != 0 && len < 256 {
                    len += 1;
                }
                let slice = std::slice::from_raw_parts(rec.srv.target, len);
                let target = String::from_utf16_lossy(slice)
                    .trim_end_matches('.')
                    .to_owned();
                if !target.is_empty() && target != "." {
                    let replace = match &best {
                        None => true,
                        Some((prio, weight, _, _)) => {
                            rec.srv.priority < *prio
                                || (rec.srv.priority == *prio && rec.srv.weight > *weight)
                        }
                    };
                    if replace {
                        best = Some((rec.srv.priority, rec.srv.weight, target, rec.srv.port));
                    }
                }
            }
            cur = rec.next;
        }
        DnsRecordListFree(records.cast(), 1);
    }
    best.map(|(_, _, target, port)| (target, port))
}
#[cfg(not(target_os = "windows"))]
fn resolve_minecraft_srv(_host: &str) -> Option<(String, u16)> {
    None
}
fn connect_socket(address: &str, timeout: Duration) -> Result<(TcpStream, String, u16)> {
    let (mut host, mut port, explicit_port) = endpoint(address)?;
    if !explicit_port {
        if let Some((srv_host, srv_port)) = resolve_minecraft_srv(&host) {
            host = srv_host;
            port = srv_port;
        }
    }
    let endpoints: Vec<_> = (host.as_str(), port).to_socket_addrs()?.collect();
    ensure!(!endpoints.is_empty(), "Server hostname did not resolve");
    let mut last_error = None;
    for endpoint in endpoints {
        match TcpStream::connect_timeout(&endpoint, timeout) {
            Ok(stream) => {
                stream.set_nodelay(true)?;
                return Ok((stream, host, port));
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap().into())
}
fn handshake(host: &str, port: u16, next_state: i32) -> Writer {
    let mut w = Writer::new();
    w.varint(protocol::PROTOCOL_VERSION)
        .string(host)
        .u16(port)
        .varint(next_state);
    w
}

pub fn status(address: &str, timeout: Duration) -> Result<ServerStatus> {
    let (mut stream, host, port) = connect_socket(address, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(&protocol::frame(0, &handshake(&host, port, 1).0, None)?)?;
    stream.write_all(&protocol::frame(0, &[], None)?)?;
    let mut decoder = FrameDecoder::default();
    let response = read_packet(&mut stream, &mut decoder)?;
    ensure!(response.id == 0, "Expected status response");
    let mut r = Reader::new(&response.data);
    let json = serde_json::from_str(&r.string(32767)?)?;
    r.finish()?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
    let started = Instant::now();
    let mut ping = Writer::new();
    ping.i64(nonce);
    stream.write_all(&protocol::frame(1, &ping.0, None)?)?;
    let pong = read_packet(&mut stream, &mut decoder)?;
    ensure!(pong.id == 1, "Expected status pong");
    let mut r = Reader::new(&pong.data);
    ensure!(r.i64()? == nonce, "Status pong nonce mismatch");
    r.finish()?;
    Ok(ServerStatus {
        json,
        latency: started.elapsed(),
    })
}
fn read_packet(stream: &mut TcpStream, decoder: &mut FrameDecoder) -> Result<Packet> {
    loop {
        if let Some(packet) = decoder.next()? {
            return Ok(packet);
        }
        let mut bytes = [0; 65536];
        let n = stream.read(&mut bytes)?;
        ensure!(n != 0, "Server closed connection");
        decoder.feed(&bytes[..n])?;
    }
}

struct Session {
    preferences: ClientPreferences,
    stream: TcpStream,
    decoder: FrameDecoder,
    ciphers: Option<online_login::Ciphers>,
    join_session: fn(&str)->Result<()>,
    events: SyncSender<Event>,
    outgoing: VecDeque<Vec<u8>>,
    write_offset: usize,
    queued_bytes: usize,
    play: bool,
    entity_id: i32,
    dimension: i32,
    pose: Option<Pose>,
    last_sent: Option<Pose>,
    correction_id: u64,
    world_epoch_min_correction: u64,
    pending_joins: usize,
    position_ticks: u32,
    abilities: u8,
    flying_speed: f32,
    walking_speed: f32,
}
impl Session {
    fn new(stream: TcpStream, events: SyncSender<Event>, preferences: ClientPreferences) -> Self {
        Self {
            preferences,
            stream,
            events,
            decoder: FrameDecoder::default(),
            ciphers: None,
            join_session: online_login::join_session,
            outgoing: VecDeque::new(),
            write_offset: 0,
            queued_bytes: 0,
            play: false,
            entity_id: 0,
            dimension: 0,
            pose: None,
            last_sent: None,
            correction_id: 0,
            world_epoch_min_correction: 0,
            pending_joins: 0,
            position_ticks: 0,
            abilities: 0,
            flying_speed: 0.05,
            walking_speed: 0.1,
        }
    }
    fn emit(&self, event: Event) -> Result<()> {
        self.events
            .try_send(event)
            .context("Network event queue is full or client closed")
    }
    fn queue(&mut self, id: i32, payload: &[u8]) -> Result<()> {
        let mut bytes = protocol::frame(id, payload, self.decoder.compression)?;
        ensure!(
            self.queued_bytes + bytes.len() <= 4 * 1024 * 1024,
            "Server is not reading outgoing packets"
        );
        // Encrypt once, before queuing. Partial/nonblocking writes resume at
        // write_offset without advancing the feedback register a second time.
        if let Some(ciphers)=&mut self.ciphers {ciphers.send.encrypt(&mut bytes);}
        self.queued_bytes += bytes.len();
        self.outgoing.push_back(bytes);
        Ok(())
    }
    fn flush(&mut self) -> Result<()> {
        while let Some(bytes) = self.outgoing.front() {
            match self.stream.write(&bytes[self.write_offset..]) {
                Ok(0) => bail!("Server closed while writing"),
                Ok(n) => {
                    self.write_offset += n;
                    self.queued_bytes -= n;
                    if self.write_offset == bytes.len() {
                        self.outgoing.pop_front();
                        self.write_offset = 0;
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
    fn run(
        mut self,
        host: &str,
        port: u16,
        username: &str,
        commands: Receiver<Command>,
    ) -> Result<()> {
        self.queue(0, &handshake(host, port, 2).0)?;
        let mut login = Writer::new();
        login.string(username);
        self.queue(0, &login.0)?;
        let mut last_received = Instant::now();
        loop {
            self.flush()?;
            // Bound per-iteration work so a fast server cannot starve commands.
            for _ in 0..32 {
                let mut bytes = [0; 65536];
                match self.stream.read(&mut bytes) {
                    Ok(0) => bail!("Server closed connection"),
                    Ok(n) => {
                        last_received = Instant::now();
                        if let Some(ciphers)=&mut self.ciphers {ciphers.receive.decrypt(&mut bytes[..n]);}
                        self.decoder.feed(&bytes[..n])?;
                        while let Some(packet) = self.decoder.next()? {
                            let is_disconnect = (!self.play && packet.id == 0)
                                || (self.play && packet.id == 0x40);
                            let pid = packet.id;
                            let plen = packet.data.len();
                            let in_play = self.play;
                            let res = self.packet(packet);
                            if is_disconnect {
                                res?;
                            } else {
                                res.with_context(|| {
                                    format!("Packet 0x{pid:02x} (play={in_play}, len={plen})")
                                })?;
                            }
                        }
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e.into()),
                }
            }
            for _ in 0..256 {
                match commands.try_recv() {
                    Ok(Command::Disconnect) | Err(TryRecvError::Disconnected) => {
                        let _ = self.stream.shutdown(Shutdown::Both);
                        return Ok(());
                    }
                    Ok(command) => self.command(command)?,
                    Err(TryRecvError::Empty) => break,
                }
            }
            ensure!(
                last_received.elapsed() < Duration::from_secs(30),
                "Server read timeout"
            );
            self.flush()?;
            thread::sleep(Duration::from_millis(2));
        }
    }
    fn movement(&mut self) -> Result<()> {
        let Some(p) = self.pose else {
            return Ok(());
        };
        let prev = self.last_sent.unwrap_or_default();
        let moved = (p.x - prev.x).powi(2) + (p.y - prev.y).powi(2) + (p.z - prev.z).powi(2) > 0.0009
            || self.position_ticks >= 20;
        let looked = p.yaw != prev.yaw || p.pitch != prev.pitch;
        self.position_ticks += 1;
        let mut w = Writer::new();
        if moved {
            w.f64(p.x).f64(p.y).f64(p.z);
        }
        if looked {
            w.f32(p.yaw).f32(p.pitch);
        }
        w.bool(p.on_ground);
        self.queue(
            match (moved, looked) {
                (false, false) => 3,
                (true, false) => 4,
                (false, true) => 5,
                (true, true) => 6,
            },
            &w.0,
        )?;
        self.last_sent = Some(Pose {
            x: if moved { p.x } else { prev.x },
            y: if moved { p.y } else { prev.y },
            z: if moved { p.z } else { prev.z },
            yaw: p.yaw,
            pitch: p.pitch,
            on_ground: p.on_ground,
        });
        if moved {
            self.position_ticks = 0;
        }
        Ok(())
    }
    fn packet(&mut self, packet: Packet) -> Result<()> {
        let mut r = Reader::new(&packet.data);
        if !self.play {
            match packet.id {
                0 => bail!("Login rejected: {}", r.string(32767)?),
                1 => {
                    ensure!(self.ciphers.is_none(),"Duplicate server encryption request");
                    let (reply,ciphers)=online_login::negotiate(&mut r,self.join_session).context("Authenticated online-mode login failed")?;
                    self.queue(1,&reply.0)?;
                    self.ciphers=Some(ciphers);
                }
                2 => { let uuid = r.string(36)?; let username = r.string(32)?; r.finish()?; self.play = true; self.emit(Event::Login { uuid, username })?; }
                3 => { let threshold = r.varint()?; r.finish()?; self.decoder.compression = if threshold < 0 { None } else { Some(threshold as usize) }; }
                _ => bail!("Unexpected login packet 0x{:02x}", packet.id),
            }
            return Ok(());
        }
        match packet.id {
            0x00 => {
                let id = r.varint()?;
                r.finish()?;
                let mut w = Writer::new();
                w.varint(id);
                self.queue(0, &w.0)?;
            }
            0x01 => {
                self.entity_id = r.i32()?;
                let gamemode = r.u8()?;
                self.dimension = r.i8()? as i32;
                let difficulty = r.u8()?;
                r.u8()?;
                let flat_world = r.string(32)?.eq_ignore_ascii_case("flat");
                let reduced_debug = r.bool()?;
                r.finish()?;
                self.pose = None;
                self.last_sent = None;
                self.position_ticks = 0;
                self.abilities = 0;
                self.flying_speed = 0.05;
                self.walking_speed = 0.1;
                self.correction_id += 1;
                self.world_epoch_min_correction = self.correction_id;
                self.pending_joins += 1;
                self.emit(Event::JoinGame {
                    entity_id: self.entity_id,
                    gamemode,
                    dimension: self.dimension,
                    difficulty,
                    reduced_debug,
                    flat_world,
                })?;
            }
            0x02 => {
                let (json,position) = chat_packets::incoming(&mut r)?;
                self.emit(Event::Chat { json, position })?;
            }
            0x03 => {
                let age = r.i64()?;
                let time = r.i64()?;
                r.finish()?;
                self.emit(Event::Time { age, time })?;
            }
            0x04 => {
                let entity_id = r.varint()?;
                let slot = r.i16()?;
                let item = r.slot()?;
                r.finish()?;
                self.emit(Event::EntityEquipment {
                    entity_id,
                    slot,
                    item,
                })?;
            }
            0x05 => {
                let pos = r.position()?;
                r.finish()?;
                self.emit(Event::SpawnPosition(pos))?;
            }
            0x06 => {
                let health = r.f32()?;
                let food = r.varint()?;
                let saturation = r.f32()?;
                r.finish()?;
                self.emit(Event::Health {
                    health,
                    food,
                    saturation,
                })?;
            }
            0x07 => {
                self.dimension = r.i32()?;
                let difficulty = r.u8()?;
                let gamemode = r.u8()?;
                let flat_world = r.string(32)?.eq_ignore_ascii_case("flat");
                r.finish()?;
                self.pose = None;
                self.last_sent = None;
                self.position_ticks = 0;
                self.correction_id += 1;
                self.world_epoch_min_correction = self.correction_id;
                self.emit(Event::Respawn {
                    dimension: self.dimension,
                    difficulty,
                    gamemode,
                    flat_world,
                })?;
            }
            0x08 => {
                let p = Pose {
                    x: r.f64()?,
                    y: r.f64()?,
                    z: r.f64()?,
                    yaw: r.f32()?,
                    pitch: r.f32()?,
                    on_ground: false,
                };
                let flags = r.u8()?;
                r.finish()?;
                ensure!(flags & !31 == 0, "Invalid position correction flags");
                ensure!(
                    p.x.is_finite()
                        && p.y.is_finite()
                        && p.z.is_finite()
                        && p.yaw.is_finite()
                        && p.pitch.is_finite(),
                    "Non-finite server position correction"
                );
                self.correction_id += 1;
                self.emit(Event::Position {
                    pose: p,
                    correction_id: self.correction_id,
                    relative: flags,
                })?;
            }
            0x09 => {
                let slot = r.u8()?;
                r.finish()?;
                ensure!(slot <= 8, "Invalid held slot");
                self.emit(Event::HeldSlot(slot))?;
            }
            0x0c => {
                let entity_id = r.varint()?;
                let uuid = r.take(16)?.try_into()?;
                let [x, y, z] = entity_position(&mut r)?;
                let pose = Pose {
                    x,
                    y,
                    z,
                    yaw: entity_angle(&mut r)?,
                    pitch: entity_angle(&mut r)?,
                    on_ground: false,
                };
                let held = r.i16()?;
                let metadata = read_metadata(&mut r)?;
                r.finish()?;
                self.emit(Event::SpawnPlayer {
                    entity_id,
                    uuid,
                    pose,
                    held,
                    metadata,
                })?;
            }
            0x0d => {
                let collected = r.varint()?;
                let collector = r.varint()?;
                r.finish()?;
                self.emit(Event::CollectItem {
                    collected,
                    collector,
                })?;
            }
            0x0e => {
                let entity_id = r.varint()?;
                let kind = r.u8()?;
                let [x, y, z] = entity_position(&mut r)?;
                let pitch = entity_angle(&mut r)?;
                let yaw = entity_angle(&mut r)?;
                let data = r.i32()?;
                let velocity = if data > 0 {
                    Some(entity_velocity(&mut r)?)
                } else {
                    None
                };
                r.finish()?;
                self.emit(Event::SpawnObject {
                    entity_id,
                    kind,
                    pose: Pose {
                        x,
                        y,
                        z,
                        yaw,
                        pitch,
                        on_ground: false,
                    },
                    data,
                    velocity,
                })?;
            }
            0x0f => {
                let entity_id = r.varint()?;
                let kind = r.u8()?;
                let [x, y, z] = entity_position(&mut r)?;
                let pose = Pose {
                    x,
                    y,
                    z,
                    yaw: entity_angle(&mut r)?,
                    pitch: entity_angle(&mut r)?,
                    on_ground: false,
                };
                let head_yaw = entity_angle(&mut r)?;
                let velocity = entity_velocity(&mut r)?;
                let metadata = read_metadata(&mut r)?;
                r.finish()?;
                self.emit(Event::SpawnMob {
                    entity_id,
                    kind,
                    pose,
                    head_yaw,
                    velocity,
                    metadata,
                })?;
            }
            0x12 => {
                let entity_id = r.varint()?;
                let velocity = [
                    r.i16()? as f64 / 8000.0,
                    r.i16()? as f64 / 8000.0,
                    r.i16()? as f64 / 8000.0,
                ];
                r.finish()?;
                self.emit(Event::Velocity {
                    entity_id,
                    velocity,
                })?;
            }
            0x13 => {
                let n = r.count(65536)?;
                let mut ids = Vec::with_capacity(n);
                for _ in 0..n {
                    ids.push(r.varint()?);
                }
                r.finish()?;
                self.emit(Event::DestroyEntities(ids))?;
            }
            0x14..=0x17 => {
                let entity_id = r.varint()?;
                let delta = if packet.id == 0x15 || packet.id == 0x17 {
                    Some([
                        r.i8()? as f64 / 32.0,
                        r.i8()? as f64 / 32.0,
                        r.i8()? as f64 / 32.0,
                    ])
                } else {
                    None
                };
                let look = if packet.id == 0x16 || packet.id == 0x17 {
                    Some([entity_angle(&mut r)?, entity_angle(&mut r)?])
                } else {
                    None
                };
                let on_ground = if packet.id != 0x14 { r.bool()? } else { false };
                r.finish()?;
                self.emit(Event::EntityMove {
                    entity_id,
                    delta,
                    look,
                    on_ground,
                })?;
            }
            0x18 => {
                let entity_id = r.varint()?;
                let [x, y, z] = entity_position(&mut r)?;
                let pose = Pose {
                    x,
                    y,
                    z,
                    yaw: entity_angle(&mut r)?,
                    pitch: entity_angle(&mut r)?,
                    on_ground: r.bool()?,
                };
                r.finish()?;
                self.emit(Event::EntityTeleport { entity_id, pose })?;
            }
            0x19 => {
                let entity_id = r.varint()?;
                let yaw = entity_angle(&mut r)?;
                r.finish()?;
                self.emit(Event::EntityHeadLook { entity_id, yaw })?;
            }
            0x1a => {
                let entity_id = r.i32()?;
                let status = r.i8()?;
                r.finish()?;
                self.emit(Event::EntityStatus { entity_id, status })?;
            }
            0x1b => {
                let (entity_id, vehicle, leash) = read_attach(&mut r)?;
                self.emit(Event::Attach {
                    entity_id,
                    vehicle,
                    leash,
                })?;
            }
            0x1c => {
                let entity_id = r.varint()?;
                let metadata = read_metadata(&mut r)?;
                r.finish()?;
                self.emit(Event::EntityMetadata {
                    entity_id,
                    metadata,
                })?;
            }
            0x1d => {
                let entity_id = r.varint()?;
                let effect_id = r.i8()?;
                let amplifier = r.i8()?;
                let duration = r.varint()?;
                let show_particles = r.i8()? != 0;
                r.finish()?;
                self.emit(Event::EntityEffect {
                    entity_id,
                    effect_id,
                    amplifier,
                    duration,
                    show_particles,
                    duration_max: duration == 32767,
                })?;
            }
            0x1e => {
                let entity_id = r.varint()?;
                let effect_id = r.u8()?;
                r.finish()?;
                self.emit(Event::RemoveEntityEffect {
                    entity_id,
                    effect_id,
                })?;
            }
            0x1f => {
                let progress = r.f32()?;
                let level = r.varint()?;
                let total = r.varint()?;
                r.finish()?;
                self.emit(Event::Experience {
                    progress,
                    level,
                    total,
                })?;
            }
            0x20 => {
                let (entity_id, properties) =
                    crate::vanilla::movement::attribute::read_properties(&mut r)?;
                self.emit(Event::Properties {
                    entity_id,
                    properties,
                })?;
            }
            0x21 => {
                let x = r.i32()?;
                let z = r.i32()?;
                let ground_up = r.bool()?;
                let mask = r.u16()?;
                let len = r.count(protocol::MAX_INFLATED_BYTES)?;
                let chunk = decode_chunk(x, z, ground_up, mask, self.dimension == 0, r.take(len)?)?;
                r.finish()?;
                self.emit(Event::Chunk(chunk))?;
            }
            0x22 => {
                let x = r.i32()?;
                let z = r.i32()?;
                let n = r.count(65536)?;
                let mut changes = Vec::with_capacity(n);
                for _ in 0..n {
                    let p = r.u16()?;
                    let state = r.varint()?;
                    ensure!((0..=65535).contains(&state), "Invalid block state");
                    changes.push((
                        [
                            x * 16 + ((p >> 12) & 15) as i32,
                            (p & 255) as i32,
                            z * 16 + ((p >> 8) & 15) as i32,
                        ],
                        state as u16,
                    ));
                }
                r.finish()?;
                self.emit(Event::BlockChanges(changes))?;
            }
            0x23 => {
                let pos = r.position()?;
                let state = r.varint()?;
                ensure!((0..=65535).contains(&state), "Invalid block state");
                r.finish()?;
                self.emit(Event::BlockChanges(vec![(pos, state as u16)]))?;
            }
            0x26 => {
                let sky = r.bool()?;
                let n = r.count(4096)?;
                let mut headers = Vec::with_capacity(n);
                for _ in 0..n {
                    headers.push((r.i32()?, r.i32()?, r.u16()?));
                }
                for (x, z, mask) in headers {
                    let len = chunk_length(mask, sky, true);
                    self.emit(Event::Chunk(decode_chunk_inner(
                        x,
                        z,
                        true,
                        mask,
                        sky,
                        r.take(len)?,
                        true,
                    )?))?;
                }
                r.finish()?;
            }
            0x28..=0x2a | 0x3b..=0x3e | 0x45 | 0x47 => {
                self.emit(Event::Presentation(crate::net_presentation::decode(packet.id, &mut r)?))?;
            }
            0x2b => {
                let reason = r.u8()?;
                let value = r.f32()?;
                r.finish()?;
                ensure!(value.is_finite(), "Invalid game-state value");
                self.emit(Event::GameState { reason, value })?;
            }
            0x2d => {
                let window_id = r.u8()?;
                let kind = r.string(32)?;
                let title = r.string(32767)?;
                let slots = r.u8()?;
                let entity_id = if kind == "EntityHorse" {
                    Some(r.i32()?)
                } else {
                    None
                };
                r.finish()?;
                self.emit(Event::OpenWindow {
                    window_id,
                    kind,
                    title,
                    slots,
                    entity_id,
                })?;
            }
            0x2e => {
                let id = r.u8()?;
                r.finish()?;
                self.emit(Event::CloseWindow(id))?;
            }
            0x2f => {
                let window_id = r.i8()?;
                let slot = r.i16()?;
                let item = r.slot()?;
                r.finish()?;
                self.emit(Event::SetSlot {
                    window_id,
                    slot,
                    item,
                })?;
            }
            0x30 => {
                let window_id = r.u8()?;
                let count = r.i16()?;
                ensure!((0..=4096).contains(&count), "Invalid window item count");
                let mut items = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    items.push(r.slot()?);
                }
                r.finish()?;
                self.emit(Event::WindowItems { window_id, items })?;
            }
            0x32 => {
                let window_id = r.u8()?;
                let action = r.i16()?;
                let accepted = r.bool()?;
                r.finish()?;
                self.emit(Event::Transaction {
                    window_id,
                    action,
                    accepted,
                })?;
            }
            0x33 => {
                let (position, lines) = crate::vanilla::tile_updates::decode_sign(&mut r)?;
                self.emit(Event::SignUpdate { position, lines })?;
            }
            0x35 => {
                let (position, action, tag) = crate::vanilla::tile_updates::decode_tile(&mut r)?;
                self.emit(Event::TileUpdate {
                    position,
                    action,
                    tag,
                })?;
            }
            0x38 => {
                let (action, players) = read_player_info(&mut r)?;
                self.emit(Event::PlayerList { action, players })?;
            }
            0x39 => {
                let flags = r.u8()?;
                let flying_speed = r.f32()?;
                let walking_speed = r.f32()?;
                r.finish()?;
                ensure!(
                    flying_speed.is_finite() && walking_speed.is_finite(),
                    "Invalid server ability speed"
                );
                self.abilities = flags;
                self.flying_speed = flying_speed;
                self.walking_speed = walking_speed;
                self.emit(Event::Abilities {
                    flags,
                    flying_speed,
                    walking_speed,
                })?;
            }
            0x3a => self.emit(Event::TabComplete(chat_packets::matches(&mut r)?))?,
            0x40 => bail!("Server disconnected: {}", r.string(32767)?),
            0x44 => {
                self.emit(Event::Border(crate::vanilla::click_border::decode(&mut r)?))?;
            }
            0x46 => {
                let threshold = r.varint()?;
                r.finish()?;
                self.decoder.compression = if threshold < 0 {
                    None
                } else {
                    Some(threshold as usize)
                };
            }
            0x48 => {
                let url = r.string(32767)?;
                let hash = r.string(40)?;
                r.finish()?;
                self.emit(Event::ResourcePack { url, hash })?;
            }
            id @ (0x0a|0x0b|0x10|0x11|0x24|0x25|0x27|0x2c|0x31|0x34|0x36|0x37|0x3f|0x41|0x42|0x43|0x49) => self.emit(Event::WorldAction(play_events::decode(id,&mut r)?))?,
            _ => bail!("Unknown protocol-47 play packet ID: {:#x}",packet.id),
        }
        Ok(())
    }
    fn command(&mut self, command: Command) -> Result<()> {
        ensure!(self.play, "Wait for login before sending play commands");
        if matches!(command, Command::JoinAck) {
            self.pending_joins = self.pending_joins.saturating_sub(1);
            if self.pending_joins == 0 {
                self.queue(0x15, &self.preferences.packet().0)?;
                let mut brand = Writer::new();
                brand.string("MC|Brand").string("vanilla");
                self.queue(0x17, &brand.0)?;
            }
            return Ok(());
        }
        if self.pending_joins > 0 {
            return Ok(());
        }
        let mut w = Writer::new();
        let id = match command {
            Command::Riding{pose,correction_id,strafe,forward,jump,sneak}=>{
                pose.validate()?;ensure!(strafe.is_finite()&&forward.is_finite(),"Invalid rider input");
                if correction_id==self.correction_id{
                    self.pose=Some(pose);w.f32(pose.yaw).f32(pose.pitch).bool(pose.on_ground);self.queue(5,&w.0)?;
                    let mut input=Writer::new();input.f32(strafe).f32(forward).u8(u8::from(jump)|(u8::from(sneak)<<1));self.queue(0x0c,&input.0)?;
                }return Ok(());
            }
            Command::Enchant(window,choice)=>{ensure!(choice<3,"Invalid enchantment option");w.u8(window).u8(choice);0x11}
            Command::PackStatus(hash,status)=>{ensure!(status<4,"Invalid resource pack status");w.string(&hash).varint(status.into());0x19}
            Command::SelectTrade(index)=>{w.string("MC|TrSel").i32(index);0x17}
            Command::WindowPayload(data)=>{data.write(&mut w)?;0x17}
            Command::UpdateSign(position,lines)=>{
                play_events::write_sign(&mut w,position,lines)?;
                0x12
            }
            Command::JoinAck => unreachable!(),
            Command::TeleportAck {
                pose,
                correction_id,
            } => {
                pose.validate()?;
                if correction_id >= self.world_epoch_min_correction {
                    self.pose = Some(pose);
                    w.f64(pose.x)
                        .f64(pose.y)
                        .f64(pose.z)
                        .f32(pose.yaw)
                        .f32(pose.pitch)
                        .bool(false);
                    self.queue(6, &w.0)?;
                }
                return Ok(());
            }
            Command::Movement {
                pose,
                correction_id,
            } => {
                pose.validate()?;
                if correction_id == self.correction_id {
                    self.pose = Some(pose);
                    self.movement()?;
                }
                return Ok(());
            }
            Command::Chat(message) => {
                chat_packets::send(&mut w,&message)?;
                1
            }
            Command::TabComplete { input, target } => {
                chat_packets::request(&mut w,&input,target)?;
                0x14
            }
            Command::HeldSlot(slot) => {
                ensure!(slot <= 8, "Invalid hotbar slot");
                w.u16(slot);
                9
            }
            Command::Swing => 0x0a,
            Command::EntityAction { action, parameter } => {
                w.varint(self.entity_id)
                    .varint(action as i32)
                    .varint(parameter);
                0x0b
            }
            Command::Dig {
                action,
                position,
                face,
            } => {
                ensure!(face <= 5, "Invalid block face");
                w.varint(action as i32).position(position).u8(face);
                7
            }
            Command::Place {
                position,
                face,
                held,
                cursor,
            } => {
                write_placement(&mut w, position, face, held.as_ref(), cursor)?;
                8
            }
            Command::Attack(entity) => {
                w.varint(entity).varint(1);
                2
            }
            Command::Interact { entity_id, hit } => {
                write_interaction(&mut w, entity_id, hit)?;
                2
            }
            Command::CreativeSlot { slot, stack } => {
                write_creative_slot(&mut w, slot, stack.as_ref())?;
                0x10
            }
            Command::ClickWindow {
                window_id,
                slot,
                button,
                action,
                mode,
                clicked,
            } => {
                ensure!(mode <= 6, "Invalid click-window mode");
                w.u8(window_id)
                    .i16(slot)
                    .u8(button)
                    .i16(action)
                    .u8(mode)
                    .slot(clicked.as_ref());
                0x0e
            }
            Command::ConfirmTransaction{window_id,action}=>{w.u8(window_id).i16(action).bool(true);0x0f}
            Command::CloseWindow(window_id) => {
                w.u8(window_id);
                0x0d
            }
            Command::Settings(preferences) => {
                w = preferences.packet();
                self.preferences = preferences;
                0x15
            }
            Command::Spectate(uuid)=>{w.bytes(&uuid);0x18}
            Command::Respawn | Command::RequestStats | Command::InventoryAchievement => {
                w.varint(match command {Command::Respawn=>0,Command::RequestStats=>1,_=>2});
                0x16
            }
            Command::Flying(flying) => {
                ensure!(
                    !flying || self.abilities & 4 != 0,
                    "Server has not granted flight"
                );
                self.abilities = (self.abilities & !2) | if flying { 2 } else { 0 };
                w.u8(self.abilities)
                    .f32(self.flying_speed)
                    .f32(self.walking_speed);
                0x13
            }
            Command::Disconnect => return Ok(()),
        };
        self.queue(id, &w.0)
    }
}

fn chunk_length(mask: u16, sky: bool, ground_up: bool) -> usize {
    mask.count_ones() as usize * (8192 + 2048 + if sky { 2048 } else { 0 })
        + if ground_up { 256 } else { 0 }
}
pub fn decode_chunk(
    x: i32,
    z: i32,
    ground_up: bool,
    mask: u16,
    sky: bool,
    bytes: &[u8],
) -> Result<ChunkData> {
    decode_chunk_inner(x, z, ground_up, mask, sky, bytes, false)
}
fn decode_chunk_inner(
    x: i32,
    z: i32,
    ground_up: bool,
    mask: u16,
    sky: bool,
    bytes: &[u8],
    bulk: bool,
) -> Result<ChunkData> {
    ensure!(
        (-1_875_000..=1_875_000).contains(&x) && (-1_875_000..=1_875_000).contains(&z),
        "Chunk outside vanilla world bounds"
    );
    let mut blocks = vec![0; 65536];
    // In S21 (non-bulk), ground_up && mask == 0 unloads the chunk whether the
    // server omits the biome array (0 bytes) or includes 256 biome bytes.
    // Some limbo/proxy servers (NanoLimbo/Velocity/ViaBackwards) send ground_up
    // with mask != 0 and only 0 or 256 biome bytes so Vanilla's doPreChunk(true)
    // loads an all-air void column instead of unloading it.
    if !bulk && ground_up && (bytes.is_empty() || bytes.len() == 256) {
        if mask == 0 {
            return Ok(ChunkData {
                x,
                z,
                ground_up,
                mask: 0,
                blocks,
                block_light: vec![],
                sky_light: None,
                biomes: None,
            });
        }
        return Ok(ChunkData {
            x,
            z,
            ground_up,
            mask: 0,
            blocks,
            block_light: vec![],
            sky_light: sky.then(Vec::new),
            biomes: Some(if bytes.len() == 256 {
                bytes.to_vec()
            } else {
                vec![0; 256]
            }),
        });
    }
    let sky = if bytes.len() == chunk_length(mask, sky, ground_up) {
        sky
    } else if mask != 0 && bytes.len() == chunk_length(mask, !sky, ground_up) {
        !sky
    } else {
        bail!(
            "Chunk payload size mismatch: got {}, expected {}",
            bytes.len(),
            chunk_length(mask, sky, ground_up)
        );
    };
    let mut r = Reader::new(bytes);
    for section in 0..16 {
        if mask & (1 << section) != 0 {
            for block in &mut blocks[section * 4096..(section + 1) * 4096] {
                *block = u16::from_le_bytes(r.take(2)?.try_into()?);
            }
        }
    }
    let light_length = mask.count_ones() as usize * 2048;
    let block_light = r.take(light_length)?.to_vec();
    let sky_light = if sky {
        Some(r.take(light_length)?.to_vec())
    } else {
        None
    };
    let biomes = if ground_up {
        Some(r.take(256)?.to_vec())
    } else {
        None
    };
    r.finish()?;
    Ok(ChunkData {
        x,
        z,
        ground_up,
        mask,
        blocks,
        block_light,
        sky_light,
        biomes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn listener() -> (TcpListener, String) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        (listener, address)
    }
    fn server_stream(listener: TcpListener) -> TcpStream {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
    }
    fn accept_login(stream: &mut TcpStream, decoder: &mut FrameDecoder) {
        let packet = read_packet(stream, decoder).unwrap();
        assert_eq!(packet.id, 0);
        let mut r = Reader::new(&packet.data);
        assert_eq!(r.varint().unwrap(), 47);
        r.string(255).unwrap();
        r.u16().unwrap();
        assert_eq!(r.varint().unwrap(), 2);
        r.finish().unwrap();
        let packet = read_packet(stream, decoder).unwrap();
        assert_eq!(packet.id, 0);
        assert_eq!(
            Reader::new(&packet.data).string(16).unwrap(),
            "ProtocolTest"
        );
    }
    fn send(stream: &mut TcpStream, id: i32, w: &Writer, compression: Option<usize>) {
        stream
            .write_all(&protocol::frame(id, &w.0, compression).unwrap())
            .unwrap();
    }

    #[test]
    fn tcp_status_handshake_and_nonce_echo() {
        let (listener, address) = listener();
        let server = thread::spawn(move || {
            let mut stream = server_stream(listener);
            let mut d = FrameDecoder::default();
            let packet = read_packet(&mut stream, &mut d).unwrap();
            let mut r = Reader::new(&packet.data);
            assert_eq!(packet.id, 0);
            assert_eq!(r.varint().unwrap(), 47);
            assert_eq!(r.string(255).unwrap(), "127.0.0.1");
            r.u16().unwrap();
            assert_eq!(r.varint().unwrap(), 1);
            r.finish().unwrap();
            assert!(read_packet(&mut stream, &mut d).unwrap().data.is_empty());
            let mut w = Writer::new();
            w.string(r#"{"version":{"name":"1.8.9","protocol":47},"players":{"max":20,"online":0},"description":{"text":"test"}}"#);
            send(&mut stream, 0, &w, None);
            let ping = read_packet(&mut stream, &mut d).unwrap();
            assert_eq!(ping.id, 1);
            send(&mut stream, 1, &Writer(ping.data), None);
        });
        let status = status(&address, Duration::from_secs(3)).unwrap();
        assert_eq!(status.json["version"]["protocol"], 47);
        server.join().unwrap();
    }

    #[test]
    fn compressed_tcp_login_world_keepalive_correction_and_movement() {
        let (listener, address) = listener();
        let server = thread::spawn(move || {
            let mut stream = server_stream(listener);
            let mut d = FrameDecoder::default();
            accept_login(&mut stream, &mut d);
            let mut w = Writer::new();
            w.varint(32);
            send(&mut stream, 3, &w, None);
            d.compression = Some(32);
            let mut w = Writer::new();
            w.string("00000000-0000-0000-0000-000000000001")
                .string("ProtocolTest");
            send(&mut stream, 2, &w, Some(32));
            let mut w = Writer::new();
            w.i32(123)
                .u8(0)
                .u8(0)
                .u8(2)
                .u8(20)
                .string("default")
                .bool(false);
            send(&mut stream, 1, &w, Some(32));
            let mut w = Writer::new();
            w.f64(10.0).f64(64.0).f64(-8.0).f32(90.0).f32(0.0).u8(0);
            send(&mut stream, 8, &w, Some(32));
            let mut w = Writer::new();
            w.varint(-2147483000);
            send(&mut stream, 0, &w, Some(32));
            // Section y=4 is sent as little-endian legacy block states, followed
            // by block light, skylight, and the column biome array.
            let mut chunk = vec![0u8; chunk_length(1 << 4, true, true)];
            chunk[..2].copy_from_slice(&0x1234u16.to_le_bytes());
            let mut w = Writer::new();
            w.i32(-2)
                .i32(3)
                .bool(true)
                .u16(1 << 4)
                .varint(chunk.len() as i32)
                .bytes(&chunk);
            send(&mut stream, 0x21, &w, Some(32));
            let mut acknowledged = false;
            let mut keepalive = false;
            let mut movement = false;
            let mut settings = false;
            let mut brand = false;
            for _ in 0..80 {
                let packet = read_packet(&mut stream, &mut d).unwrap();
                let mut r = Reader::new(&packet.data);
                match packet.id {
                    0 => {
                        assert_eq!(r.varint().unwrap(), -2147483000);
                        keepalive = true;
                    }
                    6 | 4 => {
                        let x = r.f64().unwrap();
                        let y = r.f64().unwrap();
                        let z = r.f64().unwrap();
                        assert_ne!(x, 9999.0, "stale pre-correction prediction was transmitted");
                        if packet.id == 6 {
                            let yaw = r.f32().unwrap();
                            let pitch = r.f32().unwrap();
                            if x == 10.0 {
                                assert_eq!((y, z, yaw, pitch), (64.0, -8.0, 90.0, 0.0));
                                acknowledged = true;
                            }
                        }
                        let on_ground = r.bool().unwrap();
                        if x == 10.5 {
                            assert_eq!(y, 64.0);
                            assert!(on_ground);
                            movement = true;
                        }
                    }
                    0x15 => {
                        assert_eq!(r.string(7).unwrap(), "ko_KR");
                        assert_eq!(r.u8().unwrap(), 6);
                        assert_eq!(r.u8().unwrap(), 1);
                        assert!(!r.bool().unwrap());
                        assert_eq!(r.u8().unwrap(), 0x25);
                        r.finish().unwrap();
                        settings = true;
                    }
                    0x17 => {
                        assert_eq!(r.string(20).unwrap(), "MC|Brand");
                        assert_eq!(r.string(32767).unwrap(), "vanilla");
                        brand = true;
                    }
                    3 | 5 => {}
                    _ => panic!("Unexpected client packet {:x}", packet.id),
                }
                if acknowledged && keepalive && movement && settings && brand {
                    return;
                }
            }
            panic!("Did not receive required client responses");
        });
        let mut profile = crate::settings::Settings { language: "ko_kr".into(), render_distance: 6, ..Default::default() };
        profile.menu.chat_visibility = 1;
        profile.menu.chat_colors = false;
        profile.menu.skin_parts = 0x25;
        let client = Client::connect(&address, "ProtocolTest", (&profile).into()).unwrap();
        let mut pose = None;
        let mut received_chunk = false;
        for _ in 0..20 {
            match client.recv_timeout(Duration::from_secs(3)).unwrap() {
                Event::JoinGame { .. } => {
                    client.send(Command::JoinAck).unwrap();
                }
                Event::Position {
                    pose: p,
                    correction_id,
                    ..
                } => {
                    client
                        .send(Command::TeleportAck {
                            pose: p,
                            correction_id,
                        })
                        .unwrap();
                    pose = Some((p, correction_id));
                }
                Event::Chunk(chunk) => {
                    assert_eq!((chunk.x, chunk.z, chunk.mask), (-2, 3, 1 << 4));
                    assert_eq!(chunk.blocks[4 * 4096], 0x1234);
                    assert_eq!(chunk.sky_light.unwrap().len(), 2048);
                    received_chunk = true;
                }
                Event::Disconnect(message) => panic!("{message}"),
                _ => {}
            }
            if received_chunk && pose.is_some() {
                break;
            }
        }
        assert!(received_chunk);
        let (mut pose, correction_id) = pose.unwrap();
        let stale = Pose { x: 9999.0, ..pose };
        client
            .send(Command::Movement {
                pose: stale,
                correction_id: correction_id - 1,
            })
            .unwrap();
        pose.x += 0.5;
        pose.on_ground = true;
        client
            .send(Command::Movement {
                pose,
                correction_id,
            })
            .unwrap();
        server.join().unwrap();
    }

    #[test]
    fn encrypted_local_peer_exchanges_compression_login_and_keepalive() {
        use aes::Aes128;
        use cfb8::cipher::KeyIvInit;
        use rand::rngs::OsRng;
        use rsa::{pkcs8::EncodePublicKey,Pkcs1v15Encrypt,RsaPrivateKey,RsaPublicKey};
        let (listener,address)=listener();
        let server=thread::spawn(move|| {
            let mut stream=server_stream(listener);
            let mut d=FrameDecoder::default();
            accept_login(&mut stream,&mut d);
            let key=RsaPrivateKey::new(&mut OsRng,1024).unwrap();
            let der=RsaPublicKey::from(&key).to_public_key_der().unwrap();
            let mut request=Writer::new();request.string("").varint(der.as_ref().len() as i32).bytes(der.as_ref()).varint(4).bytes(b"TEST");
            send(&mut stream,1,&request,None);
            let response=read_packet(&mut stream,&mut d).unwrap();
            assert_eq!(response.id,1);
            let mut r=Reader::new(&response.data);
            let mut clear=||{let n=r.count(256).unwrap();key.decrypt(Pkcs1v15Encrypt,r.take(n).unwrap()).unwrap()};
            let secret:[u8;16]=clear().try_into().unwrap();
            assert_eq!(clear(),b"TEST");r.finish().unwrap();
            let mut encrypt=cfb8::Encryptor::<Aes128>::new((&secret).into(),(&secret).into());
            let mut decrypt=cfb8::Decryptor::<Aes128>::new((&secret).into(),(&secret).into());
            let mut compression=Writer::new();compression.varint(8);
            let mut frames=protocol::frame(3,&compression.0,None).unwrap();
            let mut login=Writer::new();login.string("12345678-1234-1234-1234-123456789abc").string("ProtocolTest");
            frames.extend(protocol::frame(2,&login.0,Some(8)).unwrap());
            let mut keepalive=Writer::new();keepalive.varint(189);
            frames.extend(protocol::frame(0,&keepalive.0,Some(8)).unwrap());
            for part in frames.chunks_mut(3){encrypt.encrypt(part);stream.write_all(part).unwrap();}
            d.compression=Some(8);
            let mut buf=[0;5];
            loop {
                let n=stream.read(&mut buf).unwrap();assert!(n>0);
                decrypt.decrypt(&mut buf[..n]);d.feed(&buf[..n]).unwrap();
                if let Some(packet)=d.next().unwrap(){assert_eq!(packet.id,0);assert_eq!(packet.data,keepalive.0);break;}
            }
        });
        let stream=TcpStream::connect(&address).unwrap();stream.set_nonblocking(true).unwrap();
        let (events,event_rx)=mpsc::sync_channel(512);
        let (_sender,commands)=mpsc::sync_channel(16);
        let mut session=Session::new(stream,events,(&crate::settings::Settings::default()).into());
        // Synthetic session join only inside this loopback test. No account or external HTTP.
        session.join_session=|hash| {assert!(!hash.is_empty());Ok(())};
        let worker=thread::spawn(move||session.run("localhost",25565,"ProtocolTest",commands));
        assert!(matches!(event_rx.recv_timeout(Duration::from_secs(8)).unwrap(),Event::Login{..}));
        server.join().unwrap();
        assert!(worker.join().unwrap().is_err());
    }
    #[test]
    fn malformed_online_mode_challenge_closes_without_a_response() {
        let (listener, address) = listener();
        let server = thread::spawn(move || {
            let mut stream = server_stream(listener);
            let mut d = FrameDecoder::default();
            accept_login(&mut stream, &mut d);
            send(&mut stream, 1, &Writer::new(), None);
            let mut one = [0; 1];
            assert_eq!(
                stream.read(&mut one).unwrap(),
                0,
                "client must close without an authentication bypass response"
            );
        });
        let client = Client::connect(&address, "ProtocolTest", (&crate::settings::Settings::default()).into()).unwrap();
        match client.recv_timeout(Duration::from_secs(3)).unwrap() {
            Event::Disconnect(message) => assert!(message.contains("Authenticated online-mode")),
            event => panic!("{event:?}"),
        }
        server.join().unwrap();
    }

    #[test]
    fn outbound_inventory_statistics_and_spectator_match_189_jar() {
        let (listener,address)=listener();let stream=TcpStream::connect(address).unwrap();let _peer=server_stream(listener);
        let (events,_receiver)=mpsc::sync_channel(1);
        let mut session=Session::new(stream,events,(&crate::settings::Settings::default()).into());session.play=true;
        let oracle:serde_json::Value=serde_json::from_str(include_str!("../tests/fixtures/vanilla189-chat-packets.json")).unwrap();
        let uuid=[0x89,0x12,0x23,0x34,0x45,0x56,0x47,0x78,0x89,0x9a,0xab,0xbc,0xcd,0xde,0xef,0xf0];
        for (cmd,id,key) in [(Command::Respawn,0x16,"status0"),(Command::RequestStats,0x16,"status1"),(Command::InventoryAchievement,0x16,"status2"),(Command::Spectate(uuid),0x18,"spectate")] {
            session.command(cmd).unwrap();let mut frames=FrameDecoder::default();frames.feed(&session.outgoing.pop_front().unwrap()).unwrap();
            let packet=frames.next().unwrap().unwrap();assert_eq!(packet.id,id);
            assert_eq!(packet.data.iter().map(|b|format!("{b:02x}")).collect::<String>(),oracle[key].as_str().unwrap());
        }
    }

    #[test]
    fn bulk_partial_and_nether_chunk_layouts_are_bounded() {
        let mut bytes = vec![0; chunk_length(0x8001, false, false)];
        bytes[..2].copy_from_slice(&0x00f0u16.to_le_bytes());
        bytes[8192..8194].copy_from_slice(&0x0357u16.to_le_bytes());
        let chunk = decode_chunk(0, -1, false, 0x8001, false, &bytes).unwrap();
        assert_eq!(chunk.blocks[0], 0xf0);
        assert_eq!(chunk.blocks[15 * 4096], 0x357);
        assert!(chunk.sky_light.is_none());
        assert!(chunk.biomes.is_none());
        assert!(decode_chunk(0, 0, false, 1, true, &[0; 5]).is_err());
        assert!(decode_chunk(0, 0, true, 0, true, &[]).unwrap().unload());
    }

    #[test]
    fn relative_corrections_resolve_and_echo_the_authoritative_pose() {
        let (listener, address) = listener();
        let stream = TcpStream::connect(address).unwrap();
        let (_server, _) = listener.accept().unwrap();
        let (tx, rx) = mpsc::sync_channel(16);
        let mut session = Session::new(stream, tx, (&crate::settings::Settings::default()).into());
        session.play = true;
        let mut w = Writer::new();
        w.f64(2.0)
            .f64(70.0)
            .f64(3.0)
            .f32(20.0)
            .f32(-5.0)
            .u8(1 | 4 | 8 | 16);
        session.packet(Packet { id: 8, data: w.0 }).unwrap();
        assert!(session.outgoing.is_empty(), "S08 must not ack on the reader thread before S32 ordering");
        let resolved = match rx.recv().unwrap() {
            Event::Position {
                pose,
                correction_id,
                relative,
            } => {
                assert_eq!(correction_id, 1);
                assert_eq!(relative, 1 | 4 | 8 | 16);
                let loc = crate::vanilla::server_teleport::Correction {
                    location: crate::vanilla::server_teleport::Location {
                        position: glam::DVec3::new(pose.x, pose.y, pose.z),
                        rotation: [pose.yaw, pose.pitch],
                    },
                    flags: relative,
                }
                .resolve(crate::vanilla::server_teleport::Location {
                    position: glam::DVec3::new(100.0, 64.0, -10.0),
                    rotation: [350.0, 10.0],
                });
                let resolved = Pose {
                    x: loc.position.x,
                    y: loc.position.y,
                    z: loc.position.z,
                    yaw: loc.rotation[0],
                    pitch: loc.rotation[1],
                    on_ground: false,
                };
                assert_eq!(
                    resolved,
                    Pose {
                        x: 102.0,
                        y: 70.0,
                        z: -7.0,
                        yaw: 10.0,
                        pitch: 5.0,
                        on_ground: false,
                    }
                );
                resolved
            }
            _ => panic!("Expected correction"),
        };
        session
            .command(Command::TeleportAck {
                pose: resolved,
                correction_id: 1,
            })
            .unwrap();
        let mut d = FrameDecoder::default();
        d.feed(session.outgoing.front().unwrap()).unwrap();
        assert_eq!(d.next().unwrap().unwrap().id, 6);
    }

    #[test]
    fn protocol_catalog_matches_all_vanilla_registrations() {
        use protocol::{Direction::*, State::*};
        let catalog = protocol::PACKET_CATALOG;
        assert_eq!(catalog.len(), 111);
        assert_eq!(
            catalog
                .iter()
                .filter(|p| p.state == Play && p.direction == Clientbound)
                .count(),
            74
        );
        assert_eq!(
            catalog
                .iter()
                .filter(|p| p.state == Play && p.direction == Serverbound)
                .count(),
            26
        );
        assert_eq!(
            protocol::packet_name(Play, Clientbound, 0x1a),
            Some("S19PacketEntityStatus")
        );
        assert_eq!(
            protocol::packet_name(Play, Clientbound, 0x49),
            Some("S49PacketUpdateEntityNBT")
        );
        assert_eq!(
            protocol::packet_name(Play, Serverbound, 0x06),
            Some("C03PacketPlayer.C06PacketPlayerPosLook")
        );
    }
    #[test]
    fn walking_packets_follow_simulation_ticks_and_riders_send_look_then_input(){
        let (l,addr)=listener();let stream=TcpStream::connect(addr).unwrap();let _peer=server_stream(l);
        let (tx,_rx)=mpsc::sync_channel(8);
        let mut session=Session::new(stream,tx,(&crate::settings::Settings::default()).into());session.play=true;
        let pose=Pose{x:1.,y:64.,on_ground:true,..Default::default()};
        for _ in 0..22{session.command(Command::Movement{pose,correction_id:0}).unwrap();}
        session.command(Command::Movement{pose,correction_id:2}).unwrap();
        session.command(Command::Riding{pose,correction_id:0,strafe:0.98,forward:-0.98,jump:true,sneak:true}).unwrap();
        let mut d=FrameDecoder::default();let packets=session.outgoing.iter().map(|f|{d.feed(f).unwrap();d.next().unwrap().unwrap()}).collect::<Vec<_>>();
        assert_eq!(packets.len(),24);assert_eq!(packets[0].id,4);
        assert!(packets[1..21].iter().all(|p|p.id==3));
        assert_eq!([packets[21].id,packets[22].id,packets[23].id],[4,5,0x0c]);
        let mut input=Reader::new(&packets[23].data);assert_eq!((input.f32().unwrap(),input.f32().unwrap(),input.u8().unwrap()),(0.98,-0.98,3));input.finish().unwrap();
    }

    #[test]
    fn mob_metadata_spawn_and_relative_motion_use_legacy_units() {
        let (listener, address) = listener();
        let stream = TcpStream::connect(address).unwrap();
        let (_server, _) = listener.accept().unwrap();
        let (tx, rx) = mpsc::sync_channel(16);
        let mut session = Session::new(stream, tx, (&crate::settings::Settings::default()).into());
        session.play = true;
        let mut w = Writer::new();
        w.varint(300)
            .u8(92)
            .i32(-48)
            .i32(2048)
            .i32(16)
            .u8(64)
            .u8(192)
            .u8(32)
            .i16(800)
            .i16(-400)
            .i16(0);
        w.u8(0).u8(2).u8((3 << 5) | 6).f32(10.0).u8(127);
        session
            .packet(Packet {
                id: 0x0f,
                data: w.0,
            })
            .unwrap();
        match rx.recv().unwrap() {
            Event::SpawnMob {
                entity_id,
                kind,
                pose,
                head_yaw,
                velocity,
                metadata,
            } => {
                assert_eq!((entity_id, kind), (300, 92));
                assert_eq!((pose.x, pose.y, pose.z), (-1.5, 64.0, 0.5));
                assert_eq!((pose.yaw, pose.pitch, head_yaw), (90.0, -90.0, 45.0));
                assert_eq!(velocity, [0.1, -0.05, 0.0]);
                assert!(matches!(metadata[1], (6, Metadata::Float(10.0))));
            }
            event => panic!("Unexpected {event:?}"),
        }
        let mut w = Writer::new();
        w.varint(300)
            .u8(255)
            .u8(32)
            .u8(128)
            .u8(64)
            .u8(192)
            .bool(true);
        session
            .packet(Packet {
                id: 0x17,
                data: w.0,
            })
            .unwrap();
        match rx.recv().unwrap() {
            Event::EntityMove {
                delta,
                look,
                on_ground,
                ..
            } => {
                assert_eq!(delta, Some([-1.0 / 32.0, 1.0, -4.0]));
                assert_eq!(look, Some([90.0, -90.0]));
                assert!(on_ground);
            }
            event => panic!("Unexpected {event:?}"),
        }
    }
    #[test]
    fn full_event_queue_cannot_hide_terminal_disconnect() {
        let (listener, address) = listener();
        let stream = TcpStream::connect(address).unwrap();
        let (_server, _) = listener.accept().unwrap();
        let (tx, events) = mpsc::sync_channel(1);
        let session = Session::new(stream, tx, (&crate::settings::Settings::default()).into());
        session.emit(Event::HeldSlot(0)).unwrap();
        let error = session.emit(Event::HeldSlot(1)).unwrap_err();
        let (terminal_tx, terminal) = mpsc::channel();
        terminal_tx.send(error.to_string()).unwrap();
        let (commands, _) = mpsc::sync_channel(1);
        let client = Client {
            commands,
            events,
            terminal,
        };
        assert!(matches!(client.try_event(), Some(Event::HeldSlot(0))));
        assert!(
            matches!(client.try_event(),Some(Event::Disconnect(message)) if message.contains("event queue"))
        );
        assert!(client.try_event().is_none());
    }
    #[test]
    fn flight_request_preserves_server_granted_abilities_and_speeds() {
        let (listener, address) = listener();
        let stream = TcpStream::connect(address).unwrap();
        let (_server, _) = listener.accept().unwrap();
        let (tx, _) = mpsc::sync_channel(1);
        let mut session = Session::new(stream, tx, (&crate::settings::Settings::default()).into());
        session.play = true;
        assert!(session.command(Command::Flying(true)).is_err());
        session.abilities = 13;
        session.flying_speed = 0.08;
        session.walking_speed = 0.12;
        session.command(Command::Flying(true)).unwrap();
        let mut d = FrameDecoder::default();
        d.feed(session.outgoing.front().unwrap()).unwrap();
        let packet = d.next().unwrap().unwrap();
        assert_eq!(packet.id, 0x13);
        let mut r = Reader::new(&packet.data);
        assert_eq!(r.u8().unwrap(), 15);
        assert_eq!(r.f32().unwrap(), 0.08);
        assert_eq!(r.f32().unwrap(), 0.12);
        r.finish().unwrap();
    }
    #[test]
    fn original_effect_packet_pairs_preserve_signed_amplifiers_and_duration_flags() {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-player-effect-packets.json"
        ))
        .unwrap();
        let (listener, address) = listener();
        let stream = TcpStream::connect(address).unwrap();
        let (_server, _) = listener.accept().unwrap();
        let (tx, events) = mpsc::sync_channel(1);
        let mut session = Session::new(stream, tx, (&crate::settings::Settings::default()).into());
        session.play = true;
        let hex = |s: &str| {
            (0..s.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
                .collect()
        };
        for row in data["packets"].as_array().unwrap() {
            session
                .packet(Packet {
                    id: 0x1d,
                    data: hex(row["add"].as_str().unwrap()),
                })
                .unwrap();
            let Event::EntityEffect {
                entity_id,
                effect_id,
                amplifier,
                duration,
                show_particles,
                duration_max,
            } = events.try_recv().unwrap()
            else {
                panic!("missing effect event");
            };
            assert_eq!(entity_id as i64, row["entity_id"].as_i64().unwrap());
            assert_eq!(effect_id as i64, row["effect_id"].as_i64().unwrap());
            assert_eq!(amplifier as i64, row["amplifier"].as_i64().unwrap());
            assert_eq!(duration as i64, row["duration"].as_i64().unwrap());
            assert_eq!(show_particles, row["show_particles"].as_bool().unwrap());
            assert_eq!(duration_max, row["duration_max"].as_bool().unwrap());
            session
                .packet(Packet {
                    id: 0x1e,
                    data: hex(row["remove"].as_str().unwrap()),
                })
                .unwrap();
            let Event::RemoveEntityEffect {
                entity_id,
                effect_id,
            } = events.try_recv().unwrap()
            else {
                panic!("missing remove event");
            };
            assert_eq!(entity_id, 189);
            assert_eq!(effect_id as i64, row["effect_id"].as_i64().unwrap());
        }
        assert_eq!(data["packets"].as_array().unwrap().len(), 360);
    }
}
