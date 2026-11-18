//! Experimental server-authoritative game adapter; never runs local survival or writes a local save.
#[path="remote_events.rs"]
mod world_events;
#[path="remote_windows.rs"]
mod windows;
#[path="remote_vehicle.rs"]
mod vehicle;
pub(crate) use vehicle::Rider;
#[path="remote_dragon.rs"]
pub(crate) mod dragon;
pub(crate) use windows::REMOTE_TRADER;
use crate::{
    gameplay::{self, Mob, MobKind, Mode, Simulation},
    inventory::{Stack, Tool},
    network::{ChunkData, Client, Command, DigAction, EntityAction, Event, Metadata, Pose},
    player::Player,
    protocol::Slot,
    renderer::Renderer,
    resources::Resources,
    world::{Chunk, ChunkPos, World},
    Options,
};
use anyhow::{bail, Result};
use glam::{DVec3, IVec3, Vec3};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

#[derive(Clone, Copy)]
enum EntityKind {
    DragonPart(u8),
    Mob(MobKind),
    Player([u8; 16]),
    Item,
    Object(u8, i32),
    Other,
}
struct ServerEntity {
    hurt_camera: gameplay::HurtCamera,
    dead: bool,
    health: Option<f32>,
    max_health: Option<f32>,
    eat_ticks: u8,
    attack_ticks: u8,
    rose_ticks: u16,
    horse_anim: crate::horse_model::HorseAnim,
    horse_armor: u8,
    collar_color: u8,
    creeper_state: i8,
    creeper_powered: bool,
    creeper_ignited: bool,
    creeper_swell: u8,
    prev_creeper_swell: u8,
    sheep_color: u8,
    sheep_sheared: bool,
    pig_saddled: bool,
    villager_profession: u8,
    enderman_block: u16,
    enderman_meta: u8,
    beam_target: Option<i32>,
    slime_squish: f32,
    prev_slime_squish: f32,
    was_on_ground: bool,
    death_poofed: bool,
    pending_destroy: bool,
    dragon_death_ticks: u16,
    item_velocity: DVec3,
    boss:Option<Box<crate::models::boss_bar::Boss>>,
    dragon:Option<Box<dragon::Flight>>,
    target_position:DVec3,
    rotation:[f32;2],
    appearance:crate::remote_mobs::Appearance,
    vehicle:Option<Box<vehicle::Vehicle>>,
    mob_id:Option<u8>,
    flags:u8,skin:u8,hand:Option<Stack>,head:Option<f32>,
    pose: Pose,
    kind: EntityKind,
    steps: u8,
    previous_position: DVec3,
    position: DVec3,
    previous_rotation: [f32; 2],
    section: [i32; 3],
    inserted: u64,
    armor_flags: u8,
    invulnerable_skull: bool,
    frame_item: Option<Stack>,
    armor: [Option<Stack>; 4],
    fireball_accel: DVec3,
    object_velocity: DVec3,
    has_object_velocity: bool,
    riding: Option<i32>,
    custom_name: String,
    custom_name_visible: bool,
    potion_color: u32,
    potion_ambient: bool,
}
fn entity_height(e: &ServerEntity) -> f32 {
    if matches!(e.kind, EntityKind::Object(78, _)) || e.mob_id == Some(30) {
        if e.armor_flags & 0x10 != 0 {
            0.0
        } else if e.armor_flags & 0x01 != 0 {
            0.9875
        } else {
            1.975
        }
    } else if let Some(s) = e.mob_id {
        if matches!(s, 55 | 62) && (e.appearance.body as i8) < 0 {
            0.51000005 * f32::from(e.appearance.body as i8)
        } else {
            e.appearance
                .size(s)
                .or_else(|| crate::remote_mobs::species(s).map(|sp| sp.size))
                .map_or(1.8, |sz| sz[1])
        }
    } else {
        match e.kind {
            EntityKind::Player(_) => 1.8,
            EntityKind::Object(64 | 66, _) => 0.3125,
            EntityKind::Object(kind, data) => {
                crate::vanilla::entity_pick::object_bounds(kind, data, DVec3::ZERO, e.armor_flags)
                    .map_or(0.5, |(b, _, _)| (b.max.y - b.min.y) as f32)
            }
            EntityKind::DragonPart(i) => dragon::SIZES[i as usize][1],
            _ => 0.5,
        }
    }
}
fn rider_y_offset(e: &ServerEntity) -> f64 {
    if matches!(e.kind, EntityKind::Player(_))
        || e.mob_id == Some(51)
        || (matches!(e.mob_id, Some(54 | 57)) && e.appearance.age <= 0)
    {
        -0.35
    } else {
        0.0
    }
}
impl ServerEntity {
    fn living(&self) -> bool {
        self.mob_id.is_some() || matches!(self.kind, EntityKind::Player(_) | EntityKind::Object(78, _))
    }
    fn correct_item(&mut self, world: &World, sim: &mut Simulation, id: i32, teleport: bool) {
        let packet = DVec3::new(self.pose.x,self.pose.y,self.pose.z);
        if teleport && (packet-self.position).abs().cmplt(DVec3::new(0.03125,0.015625,0.03125)).all() {
            return;
        }
        self.position = packet;
        if let Some(d) = sim.drops.iter_mut().find(|d| d.id == local_id(id)) {
            d.set_feet(packet);
            let boxes = crate::vanilla::movement::collision::colliders(world, d.bounds.expand(DVec3::new(-0.03125,0.,-0.03125)));
            if let Some(top) = boxes.iter().map(|b|b.max.y).reduce(f64::max) {
                let offset = DVec3::Y * (top - d.bounds.min.y);
                self.position += offset;
            }
            d.set_feet(self.position);
            d.grounded = self.pose.on_ground;
        }
    }
    fn interpolate_to_packet(&mut self,teleport:bool){
        let packet=DVec3::new(self.pose.x,self.pose.y,self.pose.z);
        let diff=(packet-self.position).abs();
        self.target_position=if teleport && diff.cmplt(DVec3::new(0.03125,0.015625,0.03125)).all(){self.position}else{packet};
        if matches!(self.kind, EntityKind::Object(60 | 61 | 62 | 63 | 64 | 65 | 66 | 72 | 73 | 75 | 76, _)) {
            self.position = self.target_position;
            self.rotation = [self.pose.yaw, self.pose.pitch];
            self.steps = 0;
            if self.pose.on_ground
                && !matches!(self.kind, EntityKind::Object(63 | 64 | 66 | 76, _))
            {
                self.has_object_velocity = false;
                self.object_velocity = DVec3::ZERO;
            }
        } else {
            self.steps=self.interpolation_steps();
        }
    }
    fn interpolation_steps(&self) -> u8 {
        if self.mob_id.is_some(){return 3}
        match self.kind {
            EntityKind::Object(10, _) => 5,
            EntityKind::Object(1, _) => 8,
            EntityKind::Object(78 | 90, _)
            | EntityKind::Mob(_)
            | EntityKind::Player(_)
            | EntityKind::Item => 3,
            _ => 1,
        }
    }
}
fn mob_kind(id: u8) -> Option<MobKind> {
    Some(match id {
        90 => MobKind::Pig,
        91 => MobKind::Sheep,
        92 => MobKind::Cow,
        93 => MobKind::Chicken,
        54 => MobKind::Zombie,
        51 => MobKind::Skeleton,
        50 => MobKind::Creeper,
        52 => MobKind::Spider,
        120 => MobKind::Villager,
        _ => return None,
    })
}
fn local_id(id: i32) -> u64 {
    id as u32 as u64
}
fn position(p: Pose) -> Vec3 {
    Vec3::new(p.x as f32, p.y as f32, p.z as f32)
}
fn apply_correction(player: &mut Player, pose: Pose, relative: u8) -> Pose {
    let current = crate::vanilla::server_teleport::Location {
        position: DVec3::from_array(player.precise_position()),
        rotation: player.java_rotation(),
    };
    let dest = crate::vanilla::server_teleport::Correction {
        location: crate::vanilla::server_teleport::Location {
            position: DVec3::new(pose.x, pose.y, pose.z),
            rotation: [pose.yaw, pose.pitch],
        },
        flags: relative,
    }
    .resolve(current);
    let before = player.precise_velocity();
    let grounded = player.grounded;
    player.set_pose_f64(
        dest.position.to_array(),
        dest.rotation[0],
        dest.rotation[1],
        grounded,
    );
    player.set_velocity_f64(std::array::from_fn(|i| {
        if relative & (1 << i) != 0 {
            before[i]
        } else {
            0.
        }
    }));
    Pose {
        x: dest.position.x,
        y: dest.position.y,
        z: dest.position.z,
        yaw: dest.rotation[0],
        pitch: dest.rotation[1],
        on_ground: false,
    }
}
fn movement_pose(player: &Player) -> Pose {
    let [x, y, z] = player.precise_position();
    let [yaw, pitch] = player.java_rotation();
    Pose {
        x,
        y,
        z,
        yaw,
        pitch,
        on_ground: player.grounded,
    }
}
fn set_remote_tile(world: &mut World, p: IVec3, tag: crate::vanilla::drops::DropNbt) {
    if let Some(chunk) = world
        .chunks
        .get_mut(&(p.x.div_euclid(16), p.z.div_euclid(16)))
    {
        Arc::make_mut(chunk)
            .tiles
            .insert([p.x.rem_euclid(16), p.y, p.z.rem_euclid(16)], Arc::new(tag));
    }
}
fn apply_chunk(world: &mut World, data: ChunkData) {
    let p = (data.x, data.z);
    if data.unload() {
        world.unload_chunk(p);
        return;
    }
    let chunk = world
        .chunks
        .entry(p)
        .or_insert_with(|| Arc::new(Chunk::empty()));
    let chunk = Arc::make_mut(chunk);
    let mut light = if data.ground_up {
        crate::vanilla::lighting::LightChunk::default()
    } else {
        chunk
            .light
            .as_ref()
            .map_or_else(Default::default, |value| value.to_chunk())
    };
    let light_bytes = data.mask.count_ones() as usize * 2048;
    // The packet decoder validates these lengths. Empty arrays also occur in
    // synthetic tests which intentionally exercise only block replacement.
    if data.block_light.len() == light_bytes {
        let mut offset = 0;
        for section in 0..16 {
            if data.mask & (1 << section) == 0 {
                continue;
            }
            light.sections[section] = true;
            for i in 0..4096 {
                let shift = (i & 1) * 4;
                light.block[section * 4096 + i] = (data.block_light[offset + i / 2] >> shift) & 15;
                light.sky[section * 4096 + i] = data
                    .sky_light
                    .as_ref()
                    .filter(|v| v.len() == light_bytes)
                    .map_or(0, |values| (values[offset + i / 2] >> shift) & 15);
            }
            offset += 2048;
        }
    }
    if let Some(biomes) = data.biomes.as_ref().filter(|b| b.len() == 256) {
        chunk.biomes.copy_from_slice(biomes);
    }
    // S21 replaces block arrays directly instead of passing through Chunk::set.
    chunk.invalidate_tick_indexes();
    if data.ground_up {
        // S21 ground-up loads a fresh Chunk. A section-only fill reuses the
        // existing Chunk and its tile entities in the original client.
        chunk.blocks = data.blocks.into();
        chunk.tiles.clear();
        chunk.pistons.clear();
        chunk.containers.clear();
        chunk.entities.clear();
    } else {
        for section in 0..16 {
            if data.mask & (1 << section) != 0 {
                let start = section * 4096;
                chunk.blocks[start..start + 4096]
                    .copy_from_slice(&data.blocks[start..start + 4096]);
            }
        }
        let blocks = &chunk.blocks;
        let state_at = |x: i32, y: i32, z: i32| -> u16 {
            if (0..16).contains(&x) && (0..256).contains(&y) && (0..16).contains(&z) {
                blocks[((y as usize) << 8) | ((z as usize) << 4) | (x as usize)]
            } else {
                0
            }
        };
        chunk.pistons.retain(|&[x, y, z], _| {
            let sec = (y as usize) >> 4;
            sec >= 16 || (data.mask & (1 << sec)) == 0 || (state_at(x, y, z) >> 4) == 36
        });
        chunk.tiles.retain(|&[x, y, z], _| {
            let sec = (y as usize) >> 4;
            sec >= 16
                || (data.mask & (1 << sec)) == 0
                || crate::vanilla::middle_pick::retains_tile(36, state_at(x, y, z) >> 4)
                || (state_at(x, y, z) >> 4) != 0
        });
    }
    chunk.top = chunk
        .blocks
        .iter()
        .rposition(|&s| s != 0)
        .map_or(0, |i| (i / 256 + 1) as i32);
    let scan_top = chunk.top.clamp(0, 256);
    for z in 0..16 {
        for x in 0..16 {
            light.height[(z * 16 + x) as usize] = (0..scan_top)
                .rev()
                .find(|&y| crate::vanilla::material::light_opacity(chunk.get(x, y, z)) > 0)
                .map_or(0, |y| y + 1);
        }
    }
    chunk.light_edits.clear();
    chunk.visual_light = None;
    chunk.light = Some(Arc::new(crate::vanilla::lighting::ChunkLight::from_chunk(
        &light,
        data.sky_light.is_none(),
    )));
    world.invalidate_lighting();
}

pub struct Remote {
    pub(crate) extra:world_events::State,
    pub client: Client,
    correction: u64,
    entity_id: i32,
    has_valid_health: bool,
    pub(crate) game_mode:u8,
    can_destroy:crate::vanilla::click_border::CanDestroy,
    dimension:i32,
    pub ready: bool,
    sprint: bool,
    sneak: bool,
    selected: usize,
    delay: u8,
    click_id: i16,
    personal_click_id: i16,
    slots: Vec<Option<Slot>>,
    cursor: Option<Slot>,
    entities: HashMap<i32, ServerEntity>,
    entity_order: Vec<i32>,
    section_sequence: u64,
    riding: Option<i32>,
    rider:vehicle::Rider,
    player_modes: HashMap<[u8; 16], i32>,
    live_tiles: HashMap<IVec3,crate::vanilla::tile_updates::Live>,
    disable_damage: bool,
    using_item: bool,
    use_ticks: u8,
    right_presses: u8,
    pending_drops: VecDeque<bool>,
    pending_commands: VecDeque<Command>,
    defer_tick_commands: bool,
    outside_border: bool,
    effect_duration_max: std::collections::HashSet<u8>,
    inventory_blocked: bool,
    pub notice: String,
    pub visuals: crate::net_presentation::WorldEvents,
    pickup_random: crate::vanilla::random::JavaRandom,
    pub chat_lines: Vec<(String,u8)>,
    pub tab_completions: Vec<Vec<String>>,
    pub names: HashMap<[u8;16],String>,
    tab_names: HashMap<[u8; 16], String>,
    pub(super) local_player_center: Vec3,
    pub fancy_graphics: bool,
    step_distance: f32,
    next_step_distance: f32,
    fall_distance: f32,
}
fn neighbors(p: ChunkPos) -> [ChunkPos; 5] {
    [
        p,
        (p.0 - 1, p.1),
        (p.0 + 1, p.1),
        (p.0, p.1 - 1),
        (p.0, p.1 + 1),
    ]
}
pub fn from_slot(slot: &Slot) -> Result<Stack> {
    let durability = crate::vanilla::armor::maximum_damage(slot.id as u16) > 0;
    Ok(Stack {
        nbt: crate::vanilla::item_nbt::decode(&slot.nbt)?.map(std::sync::Arc::new),
        identity: crate::inventory::Stack::fresh_identity(),
        id: slot.id as u16,
        count: slot.count,
        meta: if durability {
            0
        } else {
            slot.damage.max(0) as u16
        },
        damage: if durability {
            slot.damage.max(0) as u16
        } else {
            0
        },
    })
}
pub(crate) fn to_slot(s: &Stack) -> Result<Slot> {
    Ok(Slot {
        id: s.id as i16,
        count: s.count,
        damage: if crate::vanilla::armor::maximum_damage(s.id) > 0 {
            s.damage as i16
        } else {
            s.meta as i16
        },
        nbt: s
            .nbt
            .as_deref()
            .map(crate::vanilla::item_nbt::encode)
            .transpose()?
            .unwrap_or_else(|| vec![0]),
    })
}
pub(crate) fn inventory_index(slot: usize) -> Option<usize> {
    match slot {
        36..=44 => Some(slot - 36),
        9..=35 => Some(slot),
        5..=8 => Some(39 - (slot - 5)),
        _ => None,
    }
}
pub(crate) fn wire_index(index: usize) -> Option<usize> {
    match index {
        0..=8 => Some(index + 36),
        9..=35 => Some(index),
        36..=39 => Some(44 - index),
        _ => None,
    }
}
#[cfg(test)]
fn predict_click(cursor:&mut Option<Slot>,slot:&mut Option<Slot>,index:usize,right:bool){
    use crate::inventory::{ClickAction,SlotRef,Inventory};
    let mut inv=Inventory{cursor:cursor.as_ref().map(from_slot).transpose().unwrap(),..Default::default()};
    inv.slots[index]=slot.as_ref().map(from_slot).transpose().unwrap();
    crate::container::window_click(&mut inv,None,SlotRef::Player(index),ClickAction::Pickup{right},false,2);
    *cursor=inv.cursor.as_ref().map(to_slot).transpose().unwrap();
    *slot=inv.slots[index].as_ref().map(to_slot).transpose().unwrap();
}
impl Remote {
    pub fn connect(options: &Options, settings: &crate::settings::Settings) -> Result<Self> {
        let client = Client::connect(options.connect.as_deref().unwrap(), &options.username, settings.into())?;
        let mut remote = Self::with_client(client);
        remote.defer_tick_commands = true;
        Ok(remote)
    }
    pub(crate) fn with_client(client: Client) -> Self {
        Self {
            client,
            extra:Default::default(),
            correction: 0,
            entity_id: 0,
            has_valid_health: false,
            game_mode:0,
            can_destroy:Default::default(),
            dimension:0,
            ready: false,
            sprint: false,
            sneak: false,
            selected: 0,
            delay: 0,
            click_id: 0,
            personal_click_id: 0,
            slots: vec![None; 45],
            cursor: None,
            entities: HashMap::new(),
            entity_order: Vec::new(),
            section_sequence: 0,
            riding: None,
            rider:Default::default(),
            player_modes: HashMap::new(),
            live_tiles:HashMap::new(),
            disable_damage: false,
            using_item: false,
            use_ticks: 0,
            right_presses: 0,
            pending_drops: VecDeque::new(),
            pending_commands: VecDeque::new(),
            defer_tick_commands: false,
            outside_border: false,
            effect_duration_max: Default::default(),
            inventory_blocked: false,
            notice: String::new(),
            visuals: Default::default(),
            pickup_random: crate::vanilla::random::JavaRandom::new(crate::vanilla::random::JavaRandom::independent_seed()),
            chat_lines: Vec::new(),
            tab_completions: Vec::new(),
            names: HashMap::new(),
            tab_names: HashMap::new(),
            local_player_center: Vec3::ZERO,
            fancy_graphics: true,
            step_distance: 0.0,
            next_step_distance: 1.0,
            fall_distance: 0.0,
        }
    }
    pub fn poll(
        &mut self,
        world: &mut World,
        player: &mut Player,
        sim: &mut Simulation,
        dirty: &mut HashSet<ChunkPos>,
        renderer: &mut Renderer,
    ) -> Result<()> {
        self.poll_world(world,player,sim,dirty,Some(renderer))
    }
    pub fn queue_drop(&mut self, all: bool) {
        self.pending_drops.push_back(all);
    }
    pub fn queue_use_press(&mut self) {
        self.right_presses = self.right_presses.saturating_add(1);
    }
    pub(crate) fn queue_tick_command(&mut self, command: Command) -> Result<()> {
        if self.defer_tick_commands {
            self.pending_commands.push_back(command);
            Ok(())
        } else {
            self.client.send(command)
        }
    }
    fn flush_tick_commands(&mut self) -> Result<()> {
        while let Some(command) = self.pending_commands.pop_front() {
            self.client.send(command)?;
        }
        Ok(())
    }
    fn clear_open_window_commands(&mut self) {
        self.pending_commands.retain(|cmd| {
            !matches!(
                cmd,
                Command::ClickWindow { window_id, .. }
                    | Command::CloseWindow(window_id)
                    | Command::Enchant(window_id, _)
                    if *window_id != 0
            ) && !matches!(
                cmd,
                Command::SelectTrade(_)
                    | Command::WindowPayload(
                        crate::network::special_payload::Payload::Anvil(_)
                            | crate::network::special_payload::Payload::Beacon(_, _)
                    )
            )
        });
    }
    pub(crate) fn poll_world(&mut self,world:&mut World,player:&mut Player,sim:&mut Simulation,dirty:&mut HashSet<ChunkPos>,mut renderer:Option<&mut Renderer>)->Result<()> {
        while let Some(event) = self.client.try_event() {
            match event {
                Event::WorldAction(update)=>self.apply_world_event(update,world,player,sim,dirty)?,
                Event::ResourcePack{url,hash}=>self.extra.packs.push_back((url,hash)),
                Event::Login { uuid,username } => {
                    if let Some(parsed) = crate::player_skins::parse_uuid_hex(&uuid) {
                        sim.presentation.local_uuid = Some(parsed);
                        player.third_person.slim = crate::player_skins::default_slim(parsed);
                        if parsed[6] >> 4 == 4 {
                            crate::player_skins::request_mojang_profile(parsed);
                        }
                    }
                    sim.local_name.clone_from(&username);
                    self.notice = format!("Connected: {username}");
                }
                Event::JoinGame {
                    entity_id,
                    gamemode,
                    dimension,
                    flat_world,
                    difficulty,
                    reduced_debug,
                } => {
                    if !world.chunks.is_empty() || self.ready {
                        if !world.chunks.is_empty() {
                            world.chunks.clear();
                            world.invalidate_lighting();
                        }
                        world.edits.clear();
                        if let Some(r) = renderer.as_deref_mut() {
                            r.clear();
                        }
                        sim.mobs.clear();
                        sim.drops.clear();
                        sim.xp_orbs.clear();
                        sim.pickup_fx = Default::default();
                        sim.critical_emitters = Default::default();
                        sim.particles.clear();
                        sim.server_scene = Default::default();
                        self.extra.reset_world();
                        sim.mob_brains.clear();
                        self.entities.clear();
                        self.entity_order.clear();
                        self.live_tiles.clear();
                        self.riding = None;
                        sim.effects = Default::default();
                        self.effect_duration_max.clear();
                        sim.world_border = Default::default();
                        sim.weather = Default::default();
                        sim.presentation.reset_world_scoreboard();
                        sim.clear_remote_pistons();
                        self.ready = false;
                        sim.click_controller = Default::default();
                        let slim = player.third_person.slim;
                        let keys = std::mem::take(&mut player.keys);
                        let mut fresh = Player::new(player.position);
                        fresh.third_person.slim = slim;
                        fresh.keys = keys;
                        *player = fresh;
                        sim.inventory = Default::default();
                        self.slots = vec![None; 45];
                        self.cursor = None;
                        self.selected = 0;
                    }
                    sim.hardcore = gamemode & 8 != 0;
                    sim.fire_flag = false;
                    self.has_valid_health = false;
                    sim.hurt_camera = Default::default();
                    sim.world_difficulty = Some(difficulty.min(3));
                    sim.server_reduced_debug = reduced_debug;
                    self.entity_id = entity_id;
                    self.dimension=dimension;
                    sim.dimension = dimension;
                    sim.flat_world = flat_world;
                    self.sprint = false;
                    self.sneak = false;
                    self.using_item = false;
                    self.use_ticks = 0;
                    self.right_presses = 0;
                    self.pending_drops.clear();
                    self.pending_commands.clear();
                    self.outside_border = false;
                    self.step_distance = 0.0;
                    self.next_step_distance = 1.0;
                    self.fall_distance = 0.0;
                    self.apply_mode(gamemode&7,player,sim);
                    self.client.send(Command::JoinAck)?;
                }
                Event::Respawn { gamemode, dimension, flat_world, difficulty } => {
                    sim.fire_flag = false;
                    self.has_valid_health = false;
                    sim.hurt_camera = Default::default();
                    sim.health = sim.max_health;
                    sim.world_difficulty = Some(difficulty.min(3));
                    if !world.chunks.is_empty() {
                        world.chunks.clear();
                        world.invalidate_lighting();
                    }
                    world.edits.clear();
                    if let Some(r)=renderer.as_deref_mut(){r.clear();}
                    sim.mobs.clear();
                    sim.drops.clear();
                    sim.xp_orbs.clear();
                    sim.pickup_fx = Default::default();
                    sim.critical_emitters = Default::default();
                    sim.particles.clear();
                    sim.server_scene=Default::default();
                    sim.clear_remote_pistons();
                    self.extra.reset_world();
                    sim.mob_brains.clear();
                    self.entities.clear();
                    self.entity_order.clear();
                    self.live_tiles.clear();
                    self.riding = None;
                    sim.effects = Default::default();
                    self.effect_duration_max.clear();
                    player.reset_respawn();
                    self.sprint = false;
                    self.sneak = false;
                    self.apply_mode(gamemode & 7, player, sim);
                    if dimension!=self.dimension {
                        sim.world_border=Default::default();
                        sim.weather=Default::default();
                        let pitch = self.pickup_random.next_float() * 0.4 + 0.8;
                        self.extra.sounds.push(("portal.travel", 0.25, pitch));
                    }
                    self.dimension=dimension;
                    sim.dimension = dimension;
                    sim.flat_world = flat_world;
                    self.ready = false;
                    sim.click_controller = Default::default();
                    self.using_item = false;
                    self.use_ticks = 0;
                    self.right_presses = 0;
                    self.pending_drops.clear();
                    self.pending_commands.clear();
                    self.outside_border = false;
                    self.step_distance = 0.0;
                    self.next_step_distance = 1.0;
                    self.fall_distance = 0.0;
                }
                Event::Position {
                    pose,
                    correction_id,
                    relative,
                } => {
                    self.correction = correction_id;
                    self.ready = true;
                    if relative & 2 == 0 {
                        self.fall_distance = 0.0;
                    }
                    let resolved = apply_correction(player, pose, relative);
                    self.client.send(Command::TeleportAck {
                        pose: resolved,
                        correction_id,
                    })?;
                }
                Event::Chunk(data) => {
                    let p = (data.x, data.z);
                    let unloading = data.unload();
                    if data.ground_up {
                        self.live_tiles.retain(|pos,_| (pos.x.div_euclid(16),pos.z.div_euclid(16))!=p);
                    }
                    if unloading {
                        if let Some(r)=renderer.as_deref_mut(){r.remove(p);}
                        self.extra.pending_signs.retain(|pos, _| (pos.x.div_euclid(16), pos.z.div_euclid(16)) != p);
                    }
                    apply_chunk(world, data);
                    if !unloading && !self.extra.pending_signs.is_empty() {
                        let ready: Vec<IVec3> = self
                            .extra
                            .pending_signs
                            .keys()
                            .copied()
                            .filter(|pos| {
                                (pos.x.div_euclid(16), pos.z.div_euclid(16)) == p
                                    && matches!(world.get(*pos) >> 4, 63 | 68)
                            })
                            .collect();
                        for pos in ready {
                            if let Some(lines) = self.extra.pending_signs.remove(&pos) {
                                let id = world.get(pos) >> 4;
                                if let Some(current) =
                                    crate::vanilla::middle_pick::tile_snapshot(id, pos, world.tile(pos), None)
                                {
                                    set_remote_tile(
                                        world,
                                        pos,
                                        crate::vanilla::tile_updates::apply_sign(&current, true, lines)?,
                                    );
                                }
                            }
                        }
                    }
                    dirty.extend(neighbors(p));
                }
                Event::BlockChanges(changes) => {
                    for (p, state) in changes {
                        let p = IVec3::from_array(p);
                        if world.get(p)>>4 != state>>4 { self.live_tiles.remove(&p); }
                        world.set(p, state);
                        world.edits.remove(&p.to_array());
                        if matches!(state >> 4, 63 | 68) {
                            if let Some(lines) = self.extra.pending_signs.remove(&p) {
                                if let Some(current) = crate::vanilla::middle_pick::tile_snapshot(
                                    state >> 4,
                                    p,
                                    world.tile(p),
                                    None,
                                ) {
                                    set_remote_tile(
                                        world,
                                        p,
                                        crate::vanilla::tile_updates::apply_sign(&current, true, lines)?,
                                    );
                                }
                            }
                        }
                        dirty.extend(neighbors((p.x.div_euclid(16), p.z.div_euclid(16))));
                    }
                }
                Event::TileUpdate{position:p,action,tag}=>{
                    let id=world.get(p)>>4;
                    if let Some(current)=crate::vanilla::middle_pick::tile_snapshot(id,p,world.tile(p),None){
                        let live=self.live_tiles.entry(p).or_insert(crate::vanilla::tile_updates::Live::from_written(&current)?);
                        if let Some(written)=live.update(id,action,&tag)? {
                            set_remote_tile(world,p,written);
                            dirty.extend(neighbors((p.x.div_euclid(16),p.z.div_euclid(16))));
                        }
                    }
                }
                Event::SignUpdate{position:p,lines}=>{
                    let chunk_pos = (p.x.div_euclid(16), p.z.div_euclid(16));
                    if world.chunks.contains_key(&chunk_pos) {
                        let id = world.get(p) >> 4;
                        let snap_id = if matches!(id, 63 | 68) { id } else { 63 };
                        if let Some(current) =
                            crate::vanilla::middle_pick::tile_snapshot(snap_id, p, world.tile(p), None)
                        {
                            set_remote_tile(
                                world,
                                p,
                                crate::vanilla::tile_updates::apply_sign(&current, true, lines.clone())?,
                            );
                            dirty.extend(neighbors(chunk_pos));
                        }
                        if matches!(id, 63 | 68) {
                            self.extra.pending_signs.remove(&p);
                        } else {
                            self.extra.pending_signs.insert(p, lines);
                        }
                    } else {
                        self.extra.pending_signs.insert(p, lines);
                    }
                }
                Event::SpawnPosition(p) => world.spawn = IVec3::from_array(p).as_vec3(),
                Event::Time { age, time } => {
                    sim.total_ticks = age as u64;
                    sim.clock = time.unsigned_abs();
                }
                Event::Health {
                    health,
                    food,
                    saturation,
                } => {
                    if self.has_valid_health && health < sim.health {
                        sim.hurt_camera.hurt();
                    }
                    self.has_valid_health = true;
                    sim.health = health;
                    sim.food = food.clamp(0, 20);
                    sim.saturation = saturation;
                }
                Event::Experience { progress, level, total } => {
                    sim.xp.progress = progress;
                    sim.xp.level = level;
                    sim.xp.total = total;
                    sim.experience = total.max(0) as u32;
                }
                Event::HeldSlot(slot) => {
                    sim.inventory.selected = (slot as usize).min(8);
                    self.selected = sim.inventory.selected;
                    self.using_item = false;
                    self.use_ticks = 0;
                }
                Event::Abilities {
                    flags,
                    flying_speed,
                    walking_speed,
                } => {
                    self.disable_damage = flags & 1 != 0;
                    player.flying = flags & 2 != 0;
                    player.allow_flying = flags & 4 != 0;
                    player.fly_speed = flying_speed;
                    player.walk_speed = walking_speed;
                }
                Event::Properties {
                    entity_id,
                    properties,
                } if entity_id == self.entity_id => {
                    for property in &properties {
                        if property.name.eq_ignore_ascii_case("generic.movementSpeed") {
                            if player.apply_movement_attribute(property).is_err() {
                                let mut deduped = property.clone();
                                let mut seen = Vec::with_capacity(deduped.modifiers.len());
                                deduped.modifiers.retain(|m| {
                                    if seen.contains(&m.uuid) {
                                        false
                                    } else {
                                        seen.push(m.uuid);
                                        true
                                    }
                                });
                                let _ = player.apply_movement_attribute(&deduped);
                            }
                        }
                    }
                }
                Event::Properties{entity_id,properties}=>{
                    if let Some(e)=self.entities.get_mut(&entity_id){
                        for prop in properties.iter().filter(|p|p.name.eq_ignore_ascii_case("generic.maxHealth")){
                            let mut attr=crate::vanilla::movement::attribute::Attribute::default();
                            if attr.replace(prop).is_ok(){
                                let max_hp = attr.value() as f32;
                                e.max_health = Some(max_hp.max(1.));
                                if let Some(boss)=e.boss.as_mut(){boss.max=max_hp;}
                            }
                        }
                    }
                }
                Event::EntityEffect {
                    entity_id,
                    effect_id,
                    amplifier,
                    duration,
                    show_particles,
                    duration_max,
                } if entity_id == self.entity_id => {
                    let id = effect_id as u8;
                    // combine() deliberately preserves an existing effect's
                    // max-duration display flag; it changes only on insertion.
                    if sim.effects.get(id).is_none() && duration_max {
                        self.effect_duration_max.insert(id);
                    }
                    sim.effects.add_client(crate::vanilla::item_food::Effect {
                        id,
                        duration,
                        amplifier: amplifier as i32,
                        ambient: false,
                        show_particles,
                    })?;
                }
                Event::RemoveEntityEffect {
                    entity_id,
                    effect_id,
                } if entity_id == self.entity_id => {
                    sim.effects.remove_client(effect_id);
                    self.effect_duration_max.remove(&effect_id);
                }
                Event::Velocity {
                    entity_id,
                    velocity,
                } if entity_id == self.entity_id => {
                    player.set_velocity_f64(velocity);
                }
                Event::Velocity{entity_id,velocity}=>{
                    if let Some(e) = self.entities.get_mut(&entity_id) {
                        if matches!(e.kind, EntityKind::Item) {
                            e.item_velocity = velocity.into();
                            if let Some(item) = sim.drops.iter_mut().find(|d| d.id == local_id(entity_id)) { item.velocity=e.item_velocity; }
                        }
                        if let Some(v) = e.vehicle.as_mut() { v.velocity(velocity); }
                        e.object_velocity = velocity.into();
                        e.has_object_velocity =
                            matches!(e.kind, EntityKind::Object(63 | 64 | 66 | 76, _))
                                || velocity != [0.; 3];
                        if matches!(e.kind, EntityKind::Object(60, _)) && velocity != [0.; 3] {
                            let yaw = (velocity[0].atan2(velocity[2]).to_degrees()) as f32;
                            let pitch = (velocity[1].atan2(velocity[0].hypot(velocity[2])).to_degrees()) as f32;
                            e.rotation = [yaw, pitch];
                            e.pose.yaw = yaw;
                            e.pose.pitch = pitch;
                        }
                    }
                }
                Event::OpenWindow { window_id, kind, title, slots, entity_id } => {
                    let horse=entity_id.and_then(|id|self.entities.get(&id).filter(|e|e.mob_id==Some(100)).map(|e|(id,e.appearance.subtype==0)));
                    if kind=="EntityHorse" && horse.is_none(){continue}
                    self.clear_open_window_commands();
                    self.open_window(window_id, kind, title, slots, sim);
                    if let (Some((id,armor)),Some(w))=(horse,self.extra.window.as_mut()){
                        w.container.horse_id=Some(id);w.container.special=crate::container::Special::Horse{armor};
                    }
                }
                Event::CloseWindow(_) => {
                    self.clear_open_window_commands();
                    self.reset_window(sim)?;
                    self.extra.opens = Some(false);
                }
                Event::WindowItems { window_id, items } => self.window_items(window_id,items,sim)?,
                Event::SetSlot { window_id, slot, item } => self.set_window_slot(window_id,slot,item,sim)?,
                Event::GameState { reason: 3, value } => {
                    let mode=(value+0.5).floor() as i32;
                    self.apply_mode(if (0..=3).contains(&mode){mode as u8}else{0},player,sim);
                }
                Event::GameState { reason: reason @ (1 | 2 | 7 | 8), value } => sim.weather.game_state(reason, value),
                Event::GameState { reason: 10, .. } => {
                    let mut rng = crate::vanilla::random::JavaRandom::from_state(sim.particle_random[2]);
                    if sim.particles.len() < 4000 {
                        if let Some(p) = gameplay::Particle::network_sprite(
                            41,
                            player.precise_eye(1.0),
                            DVec3::ZERO,
                            &mut rng,
                        ) {
                            sim.particles.push(p);
                        }
                    }
                    sim.particle_random[2] = rng.state();
                    self.visuals.sounds.push((
                        "mob.guardian.curse".into(),
                        1.0,
                        1.0,
                        DVec3::from_array(player.precise_position()),
                    ));
                }
                Event::Border(update)=>sim.world_border.apply(update,crate::vanilla::click_border::now()),
                Event::SpawnMob {
                    entity_id,
                    kind,
                    pose,
                    head_yaw,
                    velocity,
                    metadata,
                } => {
                    let species=kind;
                    let kind = mob_kind(species);
                    sim.server_scene.basic_mobs.remove(&local_id(entity_id));
                    sim.server_scene.mobs.remove(&entity_id);
                    self.spawn_entity(entity_id, kind.map_or(EntityKind::Other, EntityKind::Mob), pose);
                    if let Some(entity)=self.entities.get_mut(&entity_id){
                        entity.mob_id=Some(species);entity.head=Some(head_yaw);entity.boss=crate::models::boss_bar::Boss::new(species).map(Box::new);
                        if species==100{
                            let mut veh=vehicle::Vehicle::new(vehicle::Kind::Horse,pose);
                            veh.velocity(velocity);
                            entity.vehicle=Some(veh);
                        }
                        if species==63{entity.dragon=Some(Box::new(dragon::Flight::new(pose.y,pose.yaw)));}
                    }
                    if species==63{
                        for i in 0..7u8{self.spawn_entity(entity_id.wrapping_add(1+i32::from(i)),EntityKind::DragonPart(i),pose);}
                    }
                    if let Some(kind) = kind {
                        sim.mobs.retain(|m| m.id != local_id(entity_id));
                        let position = position(pose);
                        sim.mobs.push(Mob {
                            id: local_id(entity_id),
                            kind,
                            position,
                            previous: position,
                            velocity: Vec3::ZERO,
                            yaw: (180. + pose.yaw).to_radians(),
                            health: kind.health(),
                            age: 0,
                            turn_ticks: 0,
                            hurt: 0,
                            fuse: 0,
                            swing_ticks: 0,
                            death_ticks: 0,
                            eat_ticks: 0,
                            attack_ticks: 0,
                            rose_ticks: 0,
                            prev_fuse: 0,
                        });
                    }
                    self.metadata(entity_id, metadata, player, sim)?;
                }
                Event::SpawnPlayer {
                    entity_id,
                    uuid,
                    pose,
                    held,metadata
                } => {
                    self.spawn_entity(entity_id, EntityKind::Player(uuid), pose);
                    if held>0{self.entities.get_mut(&entity_id).unwrap().hand=Some(Stack::new(held as u16,1));}
                    self.metadata(entity_id, metadata, player, sim)?;
                }
                Event::EntityHeadLook{entity_id,yaw}=>{if let Some(e)=self.entities.get_mut(&entity_id){e.head=Some(yaw);}}
                Event::EntityEquipment{entity_id,slot,item}=>{
                    if entity_id==self.entity_id && (0..=4).contains(&slot){
                        let n=if slot==0{sim.inventory.selected}else{35+slot as usize};
                        let mut next=item.as_ref().map(from_slot).transpose()?;
                        if let (Some(prev),Some(incoming))=(&sim.inventory.slots[n],&mut next){
                            if prev==incoming{incoming.identity=prev.identity;}
                        }
                        let changed=sim.inventory.slots[n]!=next;
                        sim.inventory.slots[n]=next;
                        if let Some(s)=wire_index(n).and_then(|n|self.slots.get_mut(n)){*s=item;}
                        if slot == 0 && changed {
                            self.using_item = false;
                            self.use_ticks = 0;
                        }
                    }else if let Some(e)=self.entities.get_mut(&entity_id){
                        let parsed=item.as_ref().map(from_slot).transpose()?;
                        if slot==0{e.hand=parsed;}
                        else if (1..=4).contains(&slot){e.armor[(slot-1) as usize]=parsed;}
                    }
                }
                Event::SpawnObject {
                    entity_id,
                    kind,
                    pose,
                    data,
                    velocity,
                } => {
                    let class = if kind == 2 {
                        EntityKind::Item
                    } else {
                        EntityKind::Object(kind, data)
                    };
                    self.spawn_entity(entity_id, class, pose);
                    if kind == 2 { self.entities.get_mut(&entity_id).unwrap().item_velocity = velocity.unwrap_or_default().into(); }
                    if let Some(e) = self.entities.get_mut(&entity_id) {
                        if matches!(kind, 63 | 64 | 66) {
                            if let Some(v) = velocity {
                                let dir = DVec3::from(v);
                                let len = dir.length();
                                if len > 0.0 {
                                    e.fireball_accel = dir / len * 0.1;
                                }
                            }
                            e.object_velocity = DVec3::ZERO;
                            e.has_object_velocity = true;
                        } else if kind == 76 {
                            e.object_velocity = velocity.unwrap_or_default().into();
                            e.has_object_velocity = true;
                        } else if let Some(v) = velocity {
                            if let Some(vehicle) = e.vehicle.as_mut() {
                                vehicle.velocity(v);
                            }
                            e.object_velocity = v.into();
                            e.has_object_velocity = v != [0.; 3];
                            if kind == 60 && v != [0.; 3] {
                                let yaw = (v[0].atan2(v[2]).to_degrees()) as f32;
                                let pitch = (v[1].atan2(v[0].hypot(v[2])).to_degrees()) as f32;
                                e.rotation = [yaw, pitch];
                                e.previous_rotation = [yaw, pitch];
                                e.pose.yaw = yaw;
                                e.pose.pitch = pitch;
                            }
                        }
                    }
                }
                Event::Attach {
                    entity_id,
                    vehicle,
                    leash: 0,
                } if entity_id == self.entity_id => {
                    player.riding = false;
                    if vehicle < 0 {
                        player.third_person.riding = false;
                        player.third_person.mount_yaw = None;
                        if let Some(e) = self.riding.and_then(|id| self.entities.get(&id)) {
                            let lift = e
                                .vehicle
                                .as_ref()
                                .map_or_else(|| f64::from(entity_height(e)), |v| v.height());
                            player.set_position_f64((e.position + DVec3::Y * lift).to_array());
                        }
                    }
                    self.rider = Default::default();
                    self.riding = (vehicle >= 0).then_some(vehicle);
                    self.fall_distance = 0.0;
                }
                Event::Attach {
                    entity_id,
                    vehicle,
                    leash: 0,
                } => {
                    let mount = (vehicle >= 0)
                        .then(|| self.vehicle_mount_point(vehicle, player))
                        .flatten();
                    if let Some(e) = self.entities.get_mut(&entity_id) {
                        e.riding = (vehicle >= 0).then_some(vehicle);
                        if let Some(mount) = mount {
                            let next = mount + DVec3::Y * rider_y_offset(e);
                            e.position = next;
                            e.previous_position = next;
                            e.target_position = next;
                            e.steps = 0;
                            let section = crate::vanilla::entity_pick::section(next);
                            if e.section != section {
                                self.section_sequence += 1;
                                e.inserted = self.section_sequence;
                                e.section = section;
                            }
                        }
                    }
                }
                Event::Attach { .. } => {}
                Event::Presentation(packet) => self.visuals.handle(packet, player, sim, world),
                Event::PlayerList { action, players } => {
                    for info in players {
                        sim.presentation.update_player(action, &info);
                        if action == 0 && sim.presentation.local_uuid == Some(info.uuid) {
                            player.third_person.slim = sim.presentation.slim_for(info.uuid);
                        }
                        match action {
                            0 => {
                                if let Some(name) = info.name {
                                    self.tab_names.insert(info.uuid, name.clone());
                                    self.names.insert(info.uuid, name);
                                }
                                self.player_modes.insert(info.uuid, info.mode.unwrap_or(0));
                            }
                            1 => {
                                if let Some(mode) = self.player_modes.get_mut(&info.uuid) {
                                    *mode = info.mode.unwrap_or(0);
                                }
                            }
                            4 => {
                                self.names.remove(&info.uuid);
                                self.player_modes.remove(&info.uuid);
                            }
                            _ => {}
                        }
                    }
                }
                Event::EntityMove {
                    entity_id,
                    delta,
                    look,
                    on_ground,
                } => {
                    if let Some(e) = self.entities.get_mut(&entity_id) {
                        if let Some(d) = delta {
                            e.pose.x += d[0];
                            e.pose.y += d[1];
                            e.pose.z += d[2];
                        }
                        if let Some(l) = look {
                            e.pose.yaw = l[0];
                            e.pose.pitch = l[1];
                        } else if e.vehicle.is_none(){
                            [e.pose.yaw,e.pose.pitch]=e.rotation;
                        }
                        e.pose.on_ground = on_ground;
                        if let Some(v)=e.vehicle.as_mut(){v.correction(&mut e.position,e.pose,false,self.riding==Some(entity_id),&mut e.steps);}
                        else if matches!(e.kind,EntityKind::Item){e.correct_item(world, sim, entity_id, false);}
                        else{e.interpolate_to_packet(false);}
                    }
                }
                Event::EntityTeleport { entity_id, pose } => {
                    if let Some(e) = self.entities.get_mut(&entity_id) {
                        e.pose = pose;
                        if let Some(v)=e.vehicle.as_mut(){v.correction(&mut e.position,e.pose,true,self.riding==Some(entity_id),&mut e.steps);}
                        else if matches!(e.kind,EntityKind::Item){e.correct_item(world, sim, entity_id, true);}
                        else{e.interpolate_to_packet(true);}
                    }
                }
                Event::EntityMetadata {
                    entity_id,
                    metadata,
                } => self.metadata(entity_id, metadata, player, sim)?,
                Event::EntityStatus { entity_id, status } => {
                    if entity_id == self.entity_id && status == 9 {
                        if let Some(s) = self.held_stack(sim) {
                            if let Some(sound) = Self::use_item_sound(&s) {
                                self.visuals.sounds.push((
                                    sound.into(),
                                    0.5,
                                    0.95,
                                    DVec3::from_array(player.precise_position()),
                                ));
                                if sound == "random.eat" {
                                    sim.emit_eating_crumbs(player, s.id, s.meta, 16);
                                }
                            }
                            if s.id == 335 {
                                sim.effects = Default::default();
                                self.effect_duration_max.clear();
                                sim.absorption = 0.;
                                if self.game_mode != 1 && !self.disable_damage {
                                    let bucket = Stack::new(325, 1);
                                    let wire = to_slot(&bucket).ok();
                                    if let Some(slot_ref) = sim.inventory.slots.get_mut(self.selected) {
                                        *slot_ref = Some(bucket);
                                    }
                                    if let Some(slot_ref) = self.slots.get_mut(36 + self.selected) {
                                        *slot_ref = wire;
                                    }
                                }
                            }
                        }
                        self.using_item = false;
                        self.use_ticks = 0;
                    }
                    if entity_id == self.entity_id && matches!(status, 22 | 23) {
                        sim.server_reduced_debug = status == 22;
                    }
                    if status == 2 {
                        self.hurt_animation(entity_id, sim);
                        if entity_id == self.entity_id {
                            let pitch = (self.pickup_random.next_float()
                                - self.pickup_random.next_float())
                                * 0.2
                                + 1.0;
                            self.visuals.sounds.push((
                                "game.player.hurt".into(),
                                1.0,
                                pitch,
                                DVec3::from_array(player.precise_position()),
                            ));
                        }
                    } else if status == 3 {
                        if entity_id == self.entity_id {
                            sim.health = 0.;
                            let pitch = (self.pickup_random.next_float()
                                - self.pickup_random.next_float())
                                * 0.2
                                + 1.0;
                            self.visuals.sounds.push((
                                "game.player.die".into(),
                                1.0,
                                pitch,
                                DVec3::from_array(player.precise_position()),
                            ));
                        } else if let Some(e) = self.entities.get_mut(&entity_id).filter(|e| e.living()) {
                            e.dead = true;
                        }
                    } else if status == 4 {
                        if let Some(e) = self.entities.get_mut(&entity_id) {
                            e.attack_ticks = 10;
                            self.visuals.sounds.push(("mob.irongolem.throw".into(), 1., 1., e.position));
                        }
                        if let Some(m) = sim.mobs.iter_mut().find(|m| m.id == local_id(entity_id)) {
                            m.attack_ticks = 10;
                        }
                    } else if status == 10 {
                        if let Some(e) = self.entities.get_mut(&entity_id) {
                            e.eat_ticks = 40;
                        }
                        if let Some(m) = sim.mobs.iter_mut().find(|m| m.id == local_id(entity_id)) {
                            m.eat_ticks = 40;
                        }
                    } else if status == 11 {
                        if let Some(e) = self.entities.get_mut(&entity_id) {
                            e.rose_ticks = 400;
                        }
                        if let Some(m) = sim.mobs.iter_mut().find(|m| m.id == local_id(entity_id)) {
                            m.rose_ticks = 400;
                        }
                    } else if matches!(status, 6 | 7 | 12 | 13 | 14 | 18) {
                        if status == 6 {
                            if let Some(e) = self.entities.get_mut(&entity_id) {
                                if e.mob_id == Some(100) {
                                    e.horse_anim.stand_ticks = 20;
                                    if let Some(v) = e.vehicle.as_mut() {
                                        v.stand_ticks = 20;
                                    }
                                }
                            }
                        }
                        let (pid, count) = match status {
                            6 => (11, 7),
                            7 | 18 => (34, 7),
                            12 => (34, 5),
                            13 => (20, 5),
                            _ => (21, 5),
                        };
                        self.emit_entity_status_particles(entity_id, pid, count, world, player, sim);
                    } else if status == 8 {
                        if let Some((pos, h)) = self
                            .entities
                            .get(&entity_id)
                            .map(|e| (e.position, entity_height(e)))
                        {
                            let mut rng =
                                crate::vanilla::random::JavaRandom::from_state(sim.particle_random[2]);
                            let listener = player.position.as_dvec3();
                            for _ in 0..12 {
                                let px = pos.x + (f64::from(rng.next_float()) - 0.5) * 0.6;
                                let py = pos.y + f64::from(h) * 0.6;
                                let pz = pos.z + (f64::from(rng.next_float()) - 0.5) * 0.6;
                                self.visuals.spawn_client_particle(
                                    sim,
                                    world,
                                    listener,
                                    5,
                                    DVec3::new(px, py, pz),
                                    DVec3::new(0.0, 0.1, 0.0),
                                );
                            }
                            let pitch = (rng.next_float() - rng.next_float()) * 0.2 + 1.0;
                            sim.particle_random[2] = rng.state();
                            self.visuals
                                .sounds
                                .push(("mob.wolf.shake".into(), 0.4, pitch, pos));
                        }
                    } else if status == 15 {
                        if let Some((pos, h)) = self
                            .entities
                            .get(&entity_id)
                            .map(|e| (e.position, entity_height(e)))
                        {
                            let mut gauss = crate::vanilla::spawning::EntityRandom::from_state(
                                sim.particle_random[2],
                                None,
                            );
                            let count = (10 + gauss.random.next_int(35)) as usize;
                            let listener = player.position.as_dvec3();
                            for _ in 0..count {
                                let gx = gauss.next_gaussian() * 0.12999999523162842;
                                let gy = gauss.next_gaussian() * 0.12999999523162842;
                                let gz = gauss.next_gaussian() * 0.12999999523162842;
                                self.visuals.spawn_client_particle(
                                    sim,
                                    world,
                                    listener,
                                    17,
                                    DVec3::new(pos.x + gx, pos.y + f64::from(h) + 0.5 + gy, pos.z + gz),
                                    DVec3::new(gx, gy, gz),
                                );
                            }
                            sim.particle_random[2] = gauss.random.state();
                        }
                    } else if status == 20 {
                        self.emit_entity_status_particles(entity_id, 0, 20, world, player, sim);
                    }
                }
                Event::DestroyEntities(ids) => {
                    for id in ids {
                        let defer = self.entities.get_mut(&id).is_some_and(|e| {
                            if e.dead && e.mob_id.is_some() && e.dragon.is_none() && e.hurt_camera.death_time < 20 {
                                e.pending_destroy = true;
                                true
                            } else {
                                false
                            }
                        });
                        if defer {
                            if self.riding == Some(id) {
                                self.riding = None;
                            }
                            for other in self.entities.values_mut() {
                                if other.riding == Some(id) {
                                    other.riding = None;
                                }
                            }
                        } else {
                            self.remove_entity(id, sim);
                        }
                    }
                }
                Event::CollectItem { collected, collector } => {
                    if let Some(e) = self.entities.get(&collected) {
                        use crate::gameplay::item_pickup_fx::Collector;
                        let target = self.entities.get(&collector).filter(|_| collector != self.entity_id)
                            .map_or(Collector::Local, |e| Collector::Actor {
                                id: collector,
                                previous: sim.server_scene.actors.get(&collector).map_or(e.position, |a| a.previous.as_dvec3()),
                                feet: e.position,
                            });
                        let sound = if let Some(orb) = sim.xp_orbs.iter().find(|orb|orb.id==collected) {
                            sim.pickup_fx.orb(orb,target); "random.orb"
                        } else {
                            if let Some(d) = sim.drops.iter().find(|d|d.id==local_id(collected)) {
                                sim.pickup_fx.item(d,target);
                            }
                            "random.pop"
                        };
                        let pitch = ((self.pickup_random.next_float()-self.pickup_random.next_float())*0.7+1.)*2.;
                        self.visuals.sounds.push((sound.into(),0.2,pitch,e.position));
                    }
                    self.remove_entity(collected,sim);
                }
                Event::Transaction{window_id,action,accepted}=>self.confirm_transaction(window_id,action,accepted)?,

                Event::Chat { json, position } => {
                    self.chat_lines.push((json, position));
                }
                Event::TabComplete(matches) => self.tab_completions.push(matches),
                Event::Disconnect(reason) => bail!("Server disconnected: {reason}"),
                _ => {}
            }
        }
        self.local_player_center = player.position + Vec3::Y * 0.9;
        self.sync_camera(sim);
        if let Some(window) = self.extra.window.as_mut() {
            let container = &mut window.container;
            container.horse_ridden = container.horse_id.is_some() && container.horse_id == self.riding;
        }
        Ok(())
    }
    fn use_item_sound(s: &Stack) -> Option<&'static str> {
        if s.id == 335 || (s.id == 373 && s.meta & 0x4000 == 0) {
            Some("random.drink")
        } else if s.id != 261 && crate::inventory::tool(s.id).0 != Tool::Sword {
            Some("random.eat")
        } else {
            None
        }
    }
    fn emit_entity_status_particles(
        &mut self,
        entity_id: i32,
        particle_id: u8,
        count: usize,
        world: &World,
        player: &Player,
        sim: &mut Simulation,
    ) {
        let Some(e) = self.entities.get(&entity_id) else {
            return;
        };
        let pos = e.position;
        let [w, h] = e
            .mob_id
            .and_then(|s| {
                e.appearance
                    .size(s)
                    .or_else(|| crate::remote_mobs::species(s).map(|sp| sp.size))
                    .map(|sz| [sz[0], sz[1]])
            })
            .unwrap_or_else(|| [0.6, entity_height(e)]);
        let seed = (sim.total_ticks ^ (entity_id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ u64::from(particle_id)) as i64;
        let mut rng = crate::vanilla::random::JavaRandom::new(seed);
        let listener = player.position.as_dvec3();
        for _ in 0..count {
            let vx = (rng.next_double() - 0.5) * 0.04;
            let vy = (rng.next_double() - 0.5) * 0.04;
            let vz = (rng.next_double() - 0.5) * 0.04;
            let px = pos.x + f64::from((rng.next_float() * 2.0 - 1.0) * w);
            let py = pos.y + 0.5 + f64::from(rng.next_float() * h);
            let pz = pos.z + f64::from((rng.next_float() * 2.0 - 1.0) * w);
            self.visuals.spawn_client_particle(
                sim,
                world,
                listener,
                particle_id,
                DVec3::new(px, py, pz),
                DVec3::new(vx, vy, vz),
            );
        }
    }
    pub(crate) fn creative_slot(&mut self,slot:i16,stack:Option<&Stack>)->Result<()> {
        let stack=stack.map(to_slot).transpose()?;
        self.queue_tick_command(Command::CreativeSlot{slot,stack:stack.clone()})?;
        if (0..45).contains(&slot){let i=slot as usize;if i>=self.slots.len(){self.slots.resize(i+1,None);}self.slots[i]=stack;}
        Ok(())
    }
    pub(crate) fn creative_cursor(&mut self,stack:Option<&Stack>)->Result<()> {
        self.cursor=stack.map(to_slot).transpose()?; Ok(())
    }
    pub fn close_inventory(&mut self, sim: &mut Simulation) -> Result<()> {
        if self.extra.opens==Some(false){return Ok(())}
        let id = self.extra.window.as_ref().map_or(0, |w| w.id);
        self.queue_tick_command(Command::CloseWindow(id))?;
        self.reset_window(sim)?;
        Ok(())
    }
    fn confirm_transaction(&mut self,window_id:u8,action:i16,accepted:bool)->Result<()>{
        let active=self.extra.window.as_ref().map_or(0,|w|w.id);
        if !accepted && (window_id==0 || window_id==active){
            self.client.send(Command::ConfirmTransaction{window_id,action})?;
            if self.extra.window.as_ref().is_some_and(|w| w.id == window_id) {
                self.inventory_blocked=true;
                self.notice="Inventory corrected by server; waiting for synchronization".into();
            }
        }
        Ok(())
    }
    fn spawn_entity(&mut self, id: i32, kind: EntityKind, pose: Pose) {
        let position = DVec3::new(pose.x, pose.y, pose.z);
        let position = if let EntityKind::Object(kind, data) = kind {
            crate::vanilla::entity_pick::object_position(kind, data, position)
        } else {
            position
        };
        self.section_sequence += 1;
        self.entity_order.retain(|&old| old != id);
        self.entity_order.push(id);
        self.entities.insert(
            id,
            ServerEntity {
                hurt_camera: Default::default(),
                dead: false,
                health: None,
                max_health: None,
                eat_ticks: 0,
                attack_ticks: 0,
                rose_ticks: 0,
                horse_anim: Default::default(),
                horse_armor: 0,
                collar_color: 14,
                creeper_state: -1,
                creeper_powered: false,
                creeper_ignited: false,
                creeper_swell: 0,
                prev_creeper_swell: 0,
                sheep_color: 0,
                sheep_sheared: false,
                pig_saddled: false,
                villager_profession: 0,
                enderman_block: 0,
                enderman_meta: 0,
                beam_target: None,
                slime_squish: 0.0,
                prev_slime_squish: 0.0,
                was_on_ground: pose.on_ground,
                death_poofed: false,
                pending_destroy: false,
                dragon_death_ticks: 0,
                item_velocity: DVec3::ZERO,
                boss:None,
                dragon:None,
                target_position:position,
                rotation:[pose.yaw,pose.pitch],
                appearance:Default::default(),
                vehicle:vehicle::Vehicle::spawn(kind,pose),
                mob_id:None,
                flags:0,skin:0,hand:None,head:None,
                pose,
                kind,
                steps: 0,
                previous_position: position,
                position,
                previous_rotation: [pose.yaw, pose.pitch],
                inserted: self.section_sequence,
                section: crate::vanilla::entity_pick::section(position),
                armor_flags: 0,
                invulnerable_skull: false,
                frame_item: None,
                armor: Default::default(),
                fireball_accel: DVec3::ZERO,
                object_velocity: DVec3::ZERO,
                has_object_velocity: false,
                riding: None,
                custom_name: String::new(),
                custom_name_visible: false,
                potion_color: 0,
                potion_ambient: false,
            },
        );
    }
    fn remove_entity(&mut self, id: i32, sim: &mut Simulation) {
        if self.entities.remove(&id).is_some_and(|e|e.dragon.is_some()){
            for i in 1..=7{let part=id.wrapping_add(i);if self.entities.get(&part).is_some_and(|e|matches!(e.kind,EntityKind::DragonPart(_))){self.remove_entity(part,sim)}}
        }
        for other in self.entities.values_mut() {
            if other.riding == Some(id) {
                other.riding = None;
            }
        }
        self.extra.remove_entity(id);
        sim.server_scene.vehicles.remove(&id);sim.server_scene.actors.remove(&id);sim.server_scene.mobs.remove(&id);sim.server_scene.basic_mobs.remove(&local_id(id));sim.server_scene.paintings.remove(&id);
        sim.server_scene.projectiles.retain(|p| p.id != id);
        sim.server_scene.labels.retain(|l| l.id != id);
        sim.server_scene.burning.remove(&id);
        sim.xp_orbs.retain(|orb|orb.id!=id);
        self.entity_order.retain(|&old| old != id);
        if self.riding == Some(id) {
            self.riding = None;
        }
        sim.mobs.retain(|m| m.id != local_id(id));
        sim.drops.retain(|d| d.id != local_id(id));
        sim.mob_brains.remove(&local_id(id));
    }
    fn metadata(
        &mut self,
        id: i32,
        metadata: Vec<(u8, Metadata)>,
        player: &mut Player,
        sim: &mut Simulation,
    ) -> Result<()> {
        // S1C updates watched values directly (including Entity flag bit 3 = sprinting).
        // Only S06, S19 and S0B play hurt.
        if id == self.entity_id {
            for (index, value) in metadata {
                match (index, value) {
                    (6, Metadata::Float(health)) if health.is_finite() => sim.health = health,
                    (17, Metadata::Float(abs)) if abs.is_finite() => sim.absorption = abs.max(0.),
                    (0, Metadata::Byte(flags)) => {
                        sim.fire_flag = flags & 1 != 0;
                        player.set_watched_sprinting(flags & 8 != 0);
                    }
                    _ => {}
                }
            }
            return Ok(());
        }
        let Some(entity) = self.entities.get_mut(&id) else {
            return Ok(());
        };
        for (index, value) in metadata {
            match (index, &value) {
                (2, Metadata::String(name)) => entity.custom_name.clone_from(name),
                (3, Metadata::Byte(v)) => entity.custom_name_visible = *v != 0,
                (7, Metadata::Int(color)) => entity.potion_color = *color as u32,
                (8, Metadata::Byte(ambient)) => entity.potion_ambient = *ambient != 0,
                _ => {}
            }
            if let (6, Metadata::Float(health)) = (index, &value) {
                if health.is_finite() {
                    entity.health = Some(*health);
                    if entity.living() {
                        entity.dead = *health <= 0.;
                    }
                }
            }
            if let Some(boss)=entity.boss.as_mut(){match (index,&value){
                (2,Metadata::String(name))=>boss.name=name.as_str().into(),
                (6,Metadata::Float(h)) if h.is_finite()=>boss.health=*h,
                _=>()
            }}
            if let (EntityKind::Object(1|10,_),Some(v))=(entity.kind,entity.vehicle.as_mut()){
                match (index,&value){
                    (17,Metadata::Int(n))=>v.hit_ticks=*n,
                    (18,Metadata::Int(n))=>v.direction=*n,
                    (19,Metadata::Float(n)) if n.is_finite()=>v.damage=*n,
                    (16,Metadata::Byte(n))=>v.lit=n&1!=0,
                    (20,Metadata::Int(n))=>v.display_state=(((n&0xfff)<<4)|((n>>12)&15)) as u16,
                    (21,Metadata::Int(n))=>v.display_offset=*n,
                    (22,Metadata::Byte(n))=>v.custom_display=*n==1,
                    _=>()
                }
            }
            match (entity.mob_id, index, &value) {
                (Some(mob), 12, Metadata::Byte(b)) => {
                    entity.appearance.age = if matches!(mob, 54 | 57) {
                        i8::from(*b != 0)
                    } else {
                        *b
                    };
                }
                (Some(mob), 12, Metadata::Int(n)) => {
                    entity.appearance.age = if matches!(mob, 54 | 57) {
                        i8::from(*n != 0)
                    } else if *n < 0 {
                        -1
                    } else if *n > 0 {
                        1
                    } else {
                        0
                    };
                }
                (Some(54), 13, Metadata::Byte(b)) => entity.appearance.subtype = u8::from(*b != 0),
                (Some(55 | 62), 16, Metadata::Byte(b)) => entity.appearance.body = *b as u8,
                (Some(50), 16, Metadata::Byte(b)) => {
                    let prev_fusing = entity.creeper_state > 0 || entity.creeper_ignited;
                    entity.creeper_state = *b;
                    if !prev_fusing && (entity.creeper_state > 0 || entity.creeper_ignited) {
                        self.visuals.sounds.push(("creeper.primed".into(), 1., 0.5, entity.position));
                    }
                }
                (Some(50), 17, Metadata::Byte(b)) => entity.creeper_powered = *b != 0,
                (Some(50), 18, Metadata::Byte(b)) => {
                    let prev_fusing = entity.creeper_state > 0 || entity.creeper_ignited;
                    entity.creeper_ignited = *b != 0;
                    if !prev_fusing && (entity.creeper_state > 0 || entity.creeper_ignited) {
                        self.visuals.sounds.push(("creeper.primed".into(), 1., 0.5, entity.position));
                    }
                }
                (Some(90), 16, Metadata::Byte(b)) => entity.pig_saddled = (*b & 1) != 0,
                (Some(91), 16, Metadata::Byte(b)) => {
                    entity.sheep_color = (*b as u8) & 0x0f;
                    entity.sheep_sheared = (*b & 0x10) != 0;
                }
                (Some(120), 16, Metadata::Int(v)) => entity.villager_profession = (*v).clamp(0, 4) as u8,
                (Some(120), 16, Metadata::Byte(b)) => entity.villager_profession = (*b).clamp(0, 4) as u8,
                (Some(68), 16, Metadata::Int(f)) => entity.appearance.flags = *f,
                (Some(68), 16, Metadata::Byte(b)) => entity.appearance.flags = i32::from(*b),
                (Some(68), 17, Metadata::Int(target)) => {
                    entity.beam_target = (*target > 0).then_some(*target);
                }
                (Some(52 | 59 | 61 | 65 | 56), 16, Metadata::Byte(b)) => {
                    entity.appearance.flags = i32::from(*b);
                }
                (Some(58), 16, Metadata::Short(state)) => {
                    let raw = *state as u16;
                    entity.enderman_block = raw & 0xfff;
                    if (raw >> 12) != 0 && entity.enderman_meta == 0 {
                        entity.enderman_meta = (raw >> 12) as u8;
                    }
                    entity.appearance.variant =
                        i32::from((entity.enderman_block << 4) | u16::from(entity.enderman_meta & 15));
                }
                (Some(58), 16, Metadata::Byte(b)) => {
                    entity.enderman_block = u16::from(*b as u8);
                    entity.appearance.variant =
                        i32::from((entity.enderman_block << 4) | u16::from(entity.enderman_meta & 15));
                }
                (Some(58), 17, Metadata::Byte(m)) => {
                    entity.enderman_meta = (*m as u8) & 15;
                    entity.appearance.variant =
                        i32::from((entity.enderman_block << 4) | u16::from(entity.enderman_meta));
                }
                (Some(58), 18, Metadata::Byte(s)) => {
                    if *s != 0 {
                        entity.appearance.flags |= 1;
                    } else {
                        entity.appearance.flags &= !1;
                    }
                }
                (Some(95 | 98), 16, Metadata::Byte(b)) => entity.appearance.flags = i32::from(*b),
                (Some(95), 20, Metadata::Byte(b)) => entity.collar_color = 15 - ((*b as u8) & 15),
                (Some(101 | 98), 18, Metadata::Byte(b))
                | (Some(100), 19, Metadata::Byte(b))
                | (Some(51), 13, Metadata::Byte(b)) => entity.appearance.subtype = *b as u8,
                (Some(100), 20, Metadata::Int(v)) => entity.appearance.variant = *v,
                (Some(100), 22, Metadata::Int(v)) => entity.horse_armor = (*v).clamp(0, 3) as u8,
                (Some(100), 22, Metadata::Byte(b)) => entity.horse_armor = (*b).clamp(0, 3) as u8,
                _ => ()
            }
            match (entity.kind, index, value) {
                (_, 16, Metadata::Int(flags)) if entity.mob_id == Some(100) => {
                    entity.appearance.flags = flags;
                    if let Some(v) = &mut entity.vehicle {
                        v.horse_flags = flags;
                    }
                }
                (_,0,Metadata::Byte(v))=>entity.flags=v as u8,
                (EntityKind::Player(_),10,Metadata::Byte(v))=>entity.skin=v as u8,
                (EntityKind::Object(71, _), 8, Metadata::Slot(item)) => {
                    entity.frame_item = item.as_ref().map(from_slot).transpose()?;
                }
                (EntityKind::Object(66, _), 10, Metadata::Byte(flags)) => {
                    entity.invulnerable_skull = flags == 1;
                }
                (EntityKind::Object(51, _), 9, Metadata::Byte(flags)) => {
                    entity.invulnerable_skull = flags == 0;
                }
                (_, 10, Metadata::Byte(flags))
                    if matches!(entity.kind, EntityKind::Object(78, _))
                        || entity.mob_id == Some(30) =>
                {
                    entity.armor_flags = flags as u8;
                }
                (EntityKind::Mob(_), 6, Metadata::Float(health)) => {
                    if health.is_finite() {
                        if let Some(m) = sim.mobs.iter_mut().find(|m| m.id == local_id(id)) {
                            m.health = health;
                        }
                    }
                }
                (EntityKind::Mob(kind), 12, Metadata::Byte(age)) => {
                    let baby = if matches!(kind, MobKind::Zombie) { age != 0 } else { !kind.hostile() && age < 0 };
                    sim.mob_brains.entry(local_id(id)).or_default().growing_age =
                        if baby { -1 } else { 0 };
                }
                (EntityKind::Mob(kind), 12, Metadata::Int(age)) => {
                    let baby = if matches!(kind, MobKind::Zombie) { age != 0 } else { !kind.hostile() && age < 0 };
                    sim.mob_brains.entry(local_id(id)).or_default().growing_age =
                        if baby { -1 } else { 0 };
                }
                (EntityKind::Item, 10, Metadata::Slot(item)) => {
                    if let Some(item) = item {
                        let stack=from_slot(&item)?;
                        if let Some(previous)=sim.drops.iter_mut().find(|d|d.id==local_id(id)){
                            previous.stack=stack;
                            continue;
                        }
                        let item = sim.spawn_item_entity(stack, entity.position, entity.item_velocity, 0);
                        item.id = local_id(id);
                        item.previous = item.position;
                    }else{sim.drops.retain(|d|d.id!=local_id(id));}
                }
                _ => {}
            }
        }
        Ok(())
    }
    pub fn riding_any_horse(&self) -> bool {
        self.riding
            .and_then(|id| self.entities.get(&id))
            .is_some_and(|entity| entity.mob_id == Some(100))
    }
    pub(crate) fn riding_horse(&self)->bool {
        self.riding.and_then(|id|self.entities.get(&id)).and_then(|entity|entity.vehicle.as_ref()).is_some_and(|v|v.kind==vehicle::Kind::Horse && v.horse_flags&4!=0)
    }
    pub fn horse_jump_power(&self) -> Option<f32> {
        self.riding_horse().then(|| self.rider.power())
    }
    pub fn mount_health(&self) -> Option<(f32, f32)> {
        let id = self.riding?;
        let entity = self.entities.get(&id)?;
        let species = entity.mob_id?;
        let default_max = match species {
            90 => 10.0,
            100 => 20.0,
            s => mob_kind(s).map_or(20.0, |k| k.health()),
        };
        let max_hp = entity
            .max_health
            .unwrap_or(default_max)
            .max(entity.health.unwrap_or(0.0))
            .max(1.0);
        let hp = entity.health.unwrap_or(max_hp).clamp(0.0, max_hp);
        Some((hp, max_hp))
    }
    pub(crate) fn pick(
        &self,
        world: &World,
        player: &Player,
        sim: &Simulation,
    ) -> Option<crate::vanilla::entity_pick::Hit<i32>> {
        self.pick_at(world, player, sim, 1.)
    }
    pub(crate) fn player_camera(&self) -> bool {
        self.extra.camera.and_then(|id| self.entities.get(&id))
            .is_none_or(|e| matches!(e.kind, EntityKind::Player(_)))
    }
    pub(crate) fn pick_at(
        &self, world: &World, player: &Player, sim: &Simulation, partial: f32,
    ) -> Option<crate::vanilla::entity_pick::Hit<i32>> {
        use crate::vanilla::entity_pick::{self, Candidate};
        let actors = self.entities.iter().filter_map(|(&id, e)| {
            if self.extra.camera == Some(id) { return None; }
            let feet = e.position;
            let size = |w, h, border, living| (entity_pick::body(feet, w, h), border, living);
            let (bounds, border, living_or_frame) = match e.kind {
                EntityKind::DragonPart(i)=>{let [w,h]=dragon::SIZES[i as usize];size(w,h,0.1,false)}
                _ if e.mob_id == Some(30) => {
                    entity_pick::object_bounds(78, 0, feet, e.armor_flags)?
                }
                _ if e.mob_id.is_some() => {
                    let [width,height,border]=e.appearance.size(e.mob_id?)?;
                    size(width,height,border,true)
                }
                EntityKind::Player(uuid) if self.player_modes.get(&uuid) != Some(&3) => {
                    size(0.6, 1.8, 0.1, true)
                }
                EntityKind::Object(kind, data) => {
                    entity_pick::object_bounds(kind, data, feet, e.armor_flags)?
                }
                _ => return None,
            };
            Some(Candidate {
                id,
                bounds,
                section: e.section,
                inserted: e.inserted,
                border,
                living_or_frame,
            })
        });
        player
            .pick_view(sim.mode == Mode::Creative, partial)
            .pick(world, actors, self.riding)
    }
    fn apply_mode(&mut self,mode:u8,player:&mut Player,sim:&mut Simulation) {
        self.game_mode=mode;
        self.disable_damage=matches!(mode,1|3);
        player.set_spectator(mode==3);
        sim.mode=if mode==1 {Mode::Creative}else{Mode::Survival};
        player.allow_flying=matches!(mode,1|3);
        if mode==3 {player.flying=true;} else if mode!=1 {player.flying=false;}
    }
    fn sync_held_slot(&mut self, sim: &Simulation) -> Result<()> {
        if sim.inventory.selected != self.selected {
            self.selected = sim.inventory.selected;
            self.client.send(Command::HeldSlot(self.selected as u16))?;
            self.using_item = false;
            self.use_ticks = 0;
        }
        Ok(())
    }
    fn held_stack(&self, sim: &Simulation) -> Option<Stack> {
        sim.inventory
            .slots
            .get(self.selected)
            .cloned()
            .flatten()
            .or_else(|| {
                self.slots
                    .get(36 + self.selected)
                    .and_then(|s| s.as_ref())
                    .and_then(|s| from_slot(s).ok())
            })
    }
    fn held_wire(&self, sim: &Simulation) -> Option<Slot> {
        self.slots
            .get(36 + self.selected)
            .cloned()
            .flatten()
            .or_else(|| {
                sim.inventory
                    .slots
                    .get(self.selected)
                    .and_then(|s| s.as_ref())
                    .and_then(|s| to_slot(s).ok())
            })
    }
    fn can_start_using_item(&self, s: &Stack, sim: &Simulation, r: &Resources) -> bool {
        if crate::inventory::tool(s.id).0 == Tool::Sword {
            return true;
        }
        if s.id == 261 {
            return self.game_mode == 1
                || s.enchantment_level(51) > 0
                || sim
                    .inventory
                    .slots
                    .iter()
                    .flatten()
                    .any(|a| a.id == 262 && a.count > 0)
                || self.slots.iter().flatten().any(|a| a.id == 262 && a.count > 0);
        }
        if s.id == 335 {
            return true;
        }
        if s.id == 373 {
            return s.meta & 0x4000 == 0;
        }
        if s.id == 322 {
            return !self.disable_damage;
        }
        !self.disable_damage
            && sim.food < 20
            && r.registry.items.get(&s.id).is_some_and(|i| i.food > 0)
    }
    fn placement_blocked(
        &self,
        player: &Player,
        alive: bool,
        bounds: crate::vanilla::movement::Aabb,
    ) -> bool {
        use crate::vanilla::entity_pick;
        if !player.is_spectator() && alive && player.precise_bounds().intersects(bounds) {
            return true;
        }
        self.entities.values().any(|e| {
            if e.dead {
                return false;
            }
            let entity_box = match e.kind {
                _ if e.mob_id == Some(30) => {
                    entity_pick::object_bounds(78, 0, e.position, e.armor_flags).map(|t| t.0)
                }
                _ if e.mob_id.is_some() => e
                    .appearance
                    .size(e.mob_id.unwrap())
                    .map(|[w, h, _]| entity_pick::body(e.position, w, h)),
                EntityKind::Player(uuid) if self.player_modes.get(&uuid) != Some(&3) => {
                    Some(entity_pick::body(e.position, 0.6, 1.8))
                }
                EntityKind::Object(kind @ (1 | 10 | 50 | 70 | 78), data) => {
                    entity_pick::object_bounds(kind, data, e.position, e.armor_flags).map(|t| t.0)
                }
                _ => None,
            };
            entity_box.is_some_and(|b| b.intersects(bounds))
        })
    }
    fn apply_attack_slowdown(&self, id: i32, player: &mut Player, sim: &Simulation) {
        if self.game_mode == 3 {
            return;
        }
        let Some(target) = self.entities.get(&id) else {
            return;
        };
        if !matches!(
            target.kind,
            EntityKind::Player(_) | EntityKind::Object(1 | 10 | 51 | 63, _)
        ) {
            return;
        }
        let held = self.held_stack(sim);
        let item_bonus = held.as_ref().map_or(0.0, |s| {
            let (kind, tier, _, _) = crate::inventory::tool(s.id);
            match kind {
                Tool::Sword => 3.0 + f64::from(tier),
                Tool::Axe => 2.0 + f64::from(tier),
                Tool::Pick => 1.0 + f64::from(tier),
                Tool::Shovel => f64::from(tier),
                _ => 0.0,
            }
        });
        let weakness = sim
            .effects
            .get(18)
            .map_or(0.0, |e| -0.5 * f64::from(e.amplifier.wrapping_add(1)));
        let mut dmg = 1.0 + item_bonus + weakness;
        if let Some(strength) = sim.effects.get(5) {
            dmg *= 1.0 + 1.3 * f64::from(strength.amplifier.wrapping_add(1));
        }
        let f = dmg as f32;
        let f2 = held.as_ref().map_or(0.0_f32, |s| {
            if matches!(target.kind, EntityKind::Player(_)) {
                s.enchantment_level(16) as f32 * 1.25
            } else {
                0.0
            }
        });
        let knockback =
            held.as_ref().map_or(0, |s| s.enchantment_level(19)) + i32::from(player.sprinting);
        if (f > 0.0 || f2 > 0.0) && knockback > 0 {
            let mut v = player.precise_velocity();
            v[0] *= 0.6;
            v[2] *= 0.6;
            player.set_velocity_f64(v);
            player.clear_sprint();
        }
    }
    fn right_click_mouse(
        &mut self,
        world: &mut World,
        player: &Player,
        sim: &mut Simulation,
        r: &Resources,
        picked: Option<crate::vanilla::entity_pick::Hit<i32>>,
        out: &mut gameplay::Outcome,
    ) -> Result<()> {
        use crate::vanilla::{click_border, entity_pick::Hit, hand_placement};
        if sim.click_controller.hitting {
            return Ok(());
        }
        self.delay = 4;
        self.sync_held_slot(sim)?;
        let mut send_air_use = true;
        let held_stack = self.held_stack(sim);
        let held_wire = self.held_wire(sim);
        match picked {
            Some(Hit::Entity {
                id: entity_id,
                point,
                ..
            }) => {
                if let Some(e) = self.entities.get(&entity_id) {
                    if matches!(e.kind, EntityKind::Object(1 | 10 | 71 | 77 | 78, _))
                        || e.mob_id == Some(30)
                        || e.mob_id == Some(120) && e.appearance.age >= 0
                        || e.mob_id == Some(100) && e.appearance.age >= 0
                        || e.mob_id == Some(90)
                            && e.appearance.age >= 0
                            && (e.pig_saddled || held_stack.as_ref().is_some_and(|s| s.id == 329))
                    {
                        send_air_use = false;
                    }
                    self.client.send(Command::Interact {
                        entity_id,
                        hit: Some(crate::vanilla::entity_pick::relative_hit(point, e.position)),
                    })?;
                    if !matches!(e.kind, EntityKind::Object(78, _)) && e.mob_id != Some(30) {
                        self.client.send(Command::Interact {
                            entity_id,
                            hit: None,
                        })?;
                    }
                }
            }
            Some(Hit::Block(hit)) if (world.get(hit.position) >> 4) != 0 => {
                if sim.world_border.contains(hit.position, click_border::now()) {
                    let sneaking_with_item = player.sneaking() && held_stack.is_some();
                    let activated = self.game_mode != 3
                        && (crate::ui::server_book::block_consumes(
                            world,
                            hit.position,
                            self.game_mode,
                            sneaking_with_item,
                        ) || (!sneaking_with_item
                            && matches!(world.get(hit.position) >> 4, 63 | 68)));
                    let alive = sim.health > 0.0;
                    let mut preplanned = None;
                    let mut blocked_item_block = false;
                    if self.game_mode != 3 && !activated {
                        if let Some(s) = held_stack
                            .as_ref()
                            .filter(|s| s.id < 256 && hand_placement::supported(s.id))
                        {
                            let input = hand_placement::Input::from_hit(player, s, hit);
                            let action = hand_placement::plan(world, input, |b| {
                                self.placement_blocked(player, alive, b)
                            })
                            .unwrap_or_default();
                            if !action.accepted {
                                blocked_item_block = true;
                            } else {
                                preplanned = Some(action);
                            }
                        }
                    }
                    if !blocked_item_block {
                        self.client.send(Command::Place {
                            position: hit.position.to_array(),
                            face: hit.face,
                            held: held_wire.clone(),
                            cursor: hit.cursor(),
                        })?;
                        let mut right_click_consumed = activated || self.game_mode == 3;
                        let mut depleted_survival_stack = false;
                        if !right_click_consumed {
                            if let Some(s) = held_stack.as_ref() {
                                let can_edit = click_border::can_place_on(
                                    self.game_mode,
                                    world.get(hit.position) >> 4,
                                    Some(s),
                                );
                                if can_edit && matches!(s.id, 383 | 385) {
                                    // ItemMonsterPlacer / ItemFireball.onItemUse returns true immediately on world.isRemote.
                                    right_click_consumed = true;
                                } else if can_edit
                                    && matches!(s.id, 328 | 342 | 343 | 407 | 408)
                                    && crate::vanilla::rail_events::is_rail(world.get(hit.position))
                                {
                                    right_click_consumed = true;
                                } else if can_edit
                                    && s.id == 323
                                    && hit.face != 0
                                    && crate::vanilla::material::is_solid(world.get(hit.position))
                                    && crate::vanilla::material::replaceable(world.get(hit.adjacent()))
                                {
                                    right_click_consumed = true;
                                } else if can_edit && hand_placement::supported(s.id)
                                {
                                    let action = match preplanned {
                                        Some(a) => a,
                                        None => {
                                            let input =
                                                hand_placement::Input::from_hit(player, s, hit);
                                            hand_placement::plan(world, input, |b| {
                                                self.placement_blocked(player, alive, b)
                                            })
                                            .unwrap_or_default()
                                        }
                                    };
                                    if action.accepted {
                                        right_click_consumed = true;
                                        if action.consume {
                                            let mut writes = action.writes.into_iter();
                                            if let Some((first, state)) = writes.next() {
                                                if world.set(first, state) {
                                                    out.changed.push(first);
                                                    if crate::vanilla::rail_events::is_rail(state) {
                                                        for q in crate::gameplay::block_support::predict_rail_added(world, first) {
                                                            out.changed.push(q);
                                                        }
                                                    }
                                                    for (p, st) in writes {
                                                        if world.set(p, st) {
                                                            out.changed.push(p);
                                                        }
                                                    }
                                                    if self.game_mode != 1 {
                                                        if sim
                                                            .inventory
                                                            .slots
                                                            .get(self.selected)
                                                            .and_then(|x| x.as_ref())
                                                            .is_some()
                                                        {
                                                            sim.inventory.take_selected(1);
                                                            let updated = sim
                                                                .inventory
                                                                .slots
                                                                .get(self.selected)
                                                                .and_then(|x| x.as_ref())
                                                                .and_then(|x| to_slot(x).ok());
                                                            if let Some(slot_ref) =
                                                                self.slots.get_mut(36 + self.selected)
                                                            {
                                                                *slot_ref = updated;
                                                            }
                                                            if sim
                                                                .inventory
                                                                .slots
                                                                .get(self.selected)
                                                                .and_then(|x| x.as_ref())
                                                                .is_none()
                                                            {
                                                                depleted_survival_stack = true;
                                                            }
                                                        } else if let Some(Some(wire)) =
                                                            self.slots.get_mut(36 + self.selected)
                                                        {
                                                            wire.count =
                                                                wire.count.saturating_sub(1);
                                                            if wire.count == 0 {
                                                                self.slots[36 + self.selected] =
                                                                    None;
                                                                depleted_survival_stack = true;
                                                            }
                                                        }
                                                    }
                                                    if s.id != 331
                                                        && hand_placement::door_block(s.id)
                                                            .is_none()
                                                    {
                                                        let (name, vol, pitch) =
                                                            hand_placement::place_sound(state);
                                                        out.world_sounds.push((
                                                            name,
                                                            first.as_dvec3() + DVec3::splat(0.5),
                                                            vol,
                                                            pitch,
                                                        ));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if right_click_consumed {
                            send_air_use = false;
                            sim.hand_swing
                                .start(crate::first_person::swing_duration(&sim.effects));
                            self.client.send(Command::Swing)?;
                        }
                        if held_stack.is_none() || depleted_survival_stack {
                            return Ok(());
                        }
                    }
                }
            }
            _ => {}
        }
        if send_air_use && self.game_mode != 3 {
            if let Some(s) = self.held_stack(sim) {
                let wire = self.held_wire(sim);
                self.client.send(Command::Place {
                    position: [-1; 3],
                    face: 255,
                    held: wire,
                    cursor: [0.; 3],
                })?;
                if s.id == 386 {
                    self.extra.book = true;
                } else if self.can_start_using_item(&s, sim, r) {
                    self.using_item = true;
                    self.use_ticks = 0;
                } else if self.apply_client_air_use(&s, world, player, sim, out) {
                    sim.equipped.reset_progress();
                }
            }
        }
        Ok(())
    }
    fn consume_held_one(&mut self, sim: &mut Simulation) {
        if self.game_mode == 1 {
            return;
        }
        if sim
            .inventory
            .slots
            .get(self.selected)
            .and_then(|x| x.as_ref())
            .is_some()
        {
            sim.inventory.take_selected(1);
            let updated = sim
                .inventory
                .slots
                .get(self.selected)
                .and_then(|x| x.as_ref())
                .and_then(|x| to_slot(x).ok());
            if let Some(slot_ref) = self.slots.get_mut(36 + self.selected) {
                *slot_ref = updated;
            }
        } else if let Some(Some(wire)) = self.slots.get_mut(36 + self.selected) {
            wire.count = wire.count.saturating_sub(1);
            if wire.count == 0 {
                self.slots[36 + self.selected] = None;
            }
        }
    }
    fn apply_client_air_use(
        &mut self,
        s: &Stack,
        world: &mut World,
        player: &Player,
        sim: &mut Simulation,
        out: &mut gameplay::Outcome,
    ) -> bool {
        use crate::vanilla::{click_border, entity_pick, movement::collision, ray_trace};
        match s.id {
            332 | 344 | 368 | 384 => {
                if self.game_mode != 1 {
                    self.consume_held_one(sim);
                    return true;
                }
                false
            }
            373 if s.meta & 0x4000 != 0 => {
                if self.game_mode != 1 {
                    self.consume_held_one(sim);
                    return true;
                }
                false
            }
            333 => {
                let origin = player.precise_eye(1.0);
                let look = player.direction().as_dvec3();
                let reach = 5.0;
                let Some(hit) = ray_trace::trace(
                    world,
                    origin,
                    origin + look * reach,
                    ray_trace::Options::item(true),
                ) else {
                    return false;
                };
                let query = player
                    .precise_bounds()
                    .sweep(look * reach)
                    .expand(DVec3::ONE);
                let inside_entity = self.entities.iter().any(|(&id, e)| {
                    if Some(id) == self.riding || e.dead {
                        return false;
                    }
                    let bounds_border = match e.kind {
                        _ if e.mob_id == Some(30) => {
                            entity_pick::object_bounds(78, 0, e.position, e.armor_flags)
                                .map(|(b, border, _)| (b, border))
                        }
                        _ if e.mob_id.is_some() => e
                            .appearance
                            .size(e.mob_id.unwrap())
                            .map(|[w, h, _]| (entity_pick::body(e.position, w, h), 0.1_f32)),
                        EntityKind::Player(uuid) if self.player_modes.get(&uuid) != Some(&3) => {
                            Some((entity_pick::body(e.position, 0.6, 1.8), 0.1_f32))
                        }
                        EntityKind::Object(kind, data) => entity_pick::object_bounds(
                            kind,
                            data,
                            e.position,
                            e.armor_flags,
                        )
                        .map(|(b, border, _)| (b, border)),
                        _ => None,
                    };
                    bounds_border.is_some_and(|(b, border)| {
                        query.intersects(b) && {
                            let expanded = b.expand(DVec3::splat(f64::from(border)));
                            origin.cmpgt(expanded.min).all() && origin.cmplt(expanded.max).all()
                        }
                    })
                });
                if inside_entity || hit.kind != ray_trace::Kind::Block {
                    return false;
                }
                let mut bp = hit.position;
                if world.get(bp) >> 4 == 78 {
                    bp.y -= 1;
                }
                let boat_box = entity_pick::body(
                    DVec3::new(bp.x as f64 + 0.5, bp.y as f64 + 1.0, bp.z as f64 + 0.5),
                    1.5,
                    0.6,
                )
                .expand(DVec3::splat(-0.1));
                let boats = self
                    .entity_order
                    .iter()
                    .filter(|&&id| Some(id) != self.riding)
                    .filter_map(|id| self.entities.get(id))
                    .filter(|e| !e.dead && matches!(e.kind, EntityKind::Object(1, _)))
                    .map(|e| entity_pick::body(e.position, 1.5, 0.6))
                    .collect();
                let collision_world = RemoteCollisionWorld {
                    world,
                    border_bounds: sim.world_border.bounds(click_border::now()),
                    feet: player.precise_position(),
                    outside_border: std::cell::Cell::new(self.outside_border),
                    boats,
                };
                if !collision::colliders(&collision_world, boat_box).is_empty() {
                    return false;
                }
                if self.game_mode != 1 {
                    self.consume_held_one(sim);
                    return true;
                }
                false
            }
            111 => {
                let origin = player.precise_eye(1.0);
                let look = player.direction().as_dvec3();
                let Some(hit) = ray_trace::trace(
                    world,
                    origin,
                    origin + look * 5.0,
                    ray_trace::Options::item(true),
                ) else {
                    return false;
                };
                if hit.kind != ray_trace::Kind::Block {
                    return false;
                }
                let water_pos = hit.position;
                let above = water_pos + IVec3::Y;
                if !sim.world_border.contains(water_pos, click_border::now())
                    || !click_border::can_place_on(
                        self.game_mode,
                        world.get(water_pos) >> 4,
                        Some(s),
                    )
                {
                    return false;
                }
                let water_state = world.get(water_pos);
                if matches!(water_state >> 4, 8 | 9)
                    && (water_state & 15) == 0
                    && world.get(above) == 0
                    && (0..256).contains(&above.y)
                {
                    if world.set(above, 111 << 4) {
                        out.changed.push(above);
                    }
                    if self.game_mode != 1 {
                        self.consume_held_one(sim);
                        return true;
                    }
                }
                false
            }
            _ => false,
        }
    }
    fn click_target(&mut self,world:&World,player:&Player,sim:&Simulation,hit:Option<crate::vanilla::entity_pick::Hit<i32>>) -> crate::vanilla::click_controller::Target {
        use crate::vanilla::click_controller;
        let mut target=click_controller::target(world,player,sim,hit,true);
        if let click_controller::Target::Block(block)=&mut target{
            block.can_edit=self.can_destroy.allows(self.game_mode,block.state>>4,&sim.inventory);
        }
        target
    }
    fn apply_controller(&mut self,actions:Vec<crate::vanilla::click_controller::Action>,entity:Option<i32>,
        player:&mut Player,world:&mut World,sim:&mut Simulation,out:&mut gameplay::Outcome)->Result<()> {
        use crate::vanilla::click_controller::{Action,Dig,step_sound};
        for action in actions {
            match action {
                Action::Swing=>{
                    sim.hand_swing.start(crate::first_person::swing_duration(&sim.effects));
                    self.client.send(Command::Swing)?;
                }
                Action::Attack=>{
                    if let Some(id)=entity{
                        self.sync_held_slot(sim)?;
                        self.client.send(Command::Attack(id))?;
                        self.apply_attack_slowdown(id, player, sim);
                    }
                }
                Action::Dig(action,p,face)=>self.client.send(Command::Dig {
                    action:match action{Dig::Start=>DigAction::Start,Dig::Abort=>DigAction::Abort,Dig::Stop=>DigAction::Finish},
                    position:p.to_array(),face,
                })?,
                Action::Destroy(p)|Action::Extinguish(p)=>{
                    let state=crate::gameplay::actual_dig_state(world,p,world.get(p));
                    if world.set(p,0) {
                        let center = p.as_dvec3() + DVec3::splat(0.5);
                        if matches!(action,Action::Destroy(_)){
                            sim.block_destroy_effects(p,state);
                            let (name, vol, pitch) = crate::vanilla::hand_placement::break_sound(state);
                            out.world_sounds.push((name, center, vol, pitch));
                        } else {
                            let pitch = 2.6 + (self.pickup_random.next_float() - self.pickup_random.next_float()) * 0.8;
                            out.world_sounds.push(("random.fizz", center, 0.5, pitch));
                        }
                        world.edits.remove(&p.to_array());self.live_tiles.remove(&p);
                        out.changed.push(p);
                    }
                }
                Action::StepSound(p,state)=>{
                    if let Some((name, vol, pitch))=step_sound(state){
                        out.world_sounds.push((name, p.as_dvec3() + DVec3::splat(0.5), vol, pitch));
                    }
                }
                Action::Clicked(p)=>sim.click_special_block(world,p,true,out),
                Action::HitEffect(p,face)=>sim.block_hit_effect(world,p,face),
                Action::Stage(..)=>{}
            }
        }
        Ok(())
    }
    fn vehicle_mount_point(&self, vid: i32, player: &Player) -> Option<DVec3> {
        if vid == self.entity_id {
            return Some(DVec3::from_array(player.precise_position()) + DVec3::Y * (1.8 * 0.75));
        }
        let ve = self.entities.get(&vid)?;
        if let Some(v) = &ve.vehicle {
            if ve.mob_id != Some(100) || ve.appearance.age >= 0 {
                return Some(v.seat(ve.position) + DVec3::Y * 0.35);
            }
        }
        let h = f64::from(entity_height(ve));
        let factor = if matches!(ve.mob_id, Some(52 | 59)) { 0.5 } else { 0.75 };
        Some(ve.position + DVec3::Y * (h * factor))
    }
    fn sync_riding_passengers(&mut self, player: &Player) {
        for _ in 0..4 {
            let mut moved_any = false;
            for i in 0..self.entity_order.len() {
                let id = self.entity_order[i];
                let Some(vid) = self.entities.get(&id).and_then(|e| e.riding) else {
                    continue;
                };
                let Some(mount) = self.vehicle_mount_point(vid, player) else {
                    continue;
                };
                let e = self.entities.get_mut(&id).unwrap();
                let next = mount + DVec3::Y * rider_y_offset(e);
                if (e.position - next).length_squared() > 1e-12 {
                    moved_any = true;
                }
                e.position = next;
                e.target_position = next;
                let section = crate::vanilla::entity_pick::section(next);
                if e.section != section {
                    self.section_sequence += 1;
                    e.inserted = self.section_sequence;
                    e.section = section;
                }
            }
            if !moved_any {
                break;
            }
        }
    }
    pub fn tick(
        &mut self,
        world: &mut World,
        player: &mut Player,
        sim: &mut Simulation,
        r: &Resources,
        left: bool,
        right: bool,
    ) -> Result<gameplay::Outcome> {
        let mut out=gameplay::Outcome::default();
        sim.total_ticks=sim.total_ticks.wrapping_add(1);
        sim.pickup_fx.tick();
        self.extra.tick(&mut out);
        sim.flush_remote_notes(world,&mut out);
        let [px, _, pz] = player.precise_position();
        let chunk_loaded = world
            .chunks
            .contains_key(&(px.floor() as i32 >> 4, pz.floor() as i32 >> 4));
        if !self.ready || !chunk_loaded {
            return Ok(out);
        }
        if self.riding.and_then(|id| self.entities.get(&id)).is_some() {
            if player.riding {
                player.begin_mounted_tick();
            }
        } else if player.riding {
            player.riding = false;
            player.third_person.riding = false;
            player.third_person.mount_yaw = None;
        }
        self.defer_tick_commands = true;
        sim.hurt_camera.tick(sim.health <= 0.);
        sim.sleep.tick_timer();
        // Minecraft.runTick resolves hotbar selection, drops, and clicks before updating entities.
        self.sync_held_slot(sim)?;
        self.flush_tick_commands()?;
        while let Some(all) = self.pending_drops.pop_front() {
            if self.game_mode != 3 {
                self.client.send(Command::Dig {
                    action: if all {
                        DigAction::DropStack
                    } else {
                        DigAction::DropOne
                    },
                    position: [0; 3],
                    face: 0,
                })?;
            }
        }
        self.delay = self.delay.saturating_sub(1);
        let picked=self.pick(world,player,sim);
        let entity=picked.and_then(|hit|hit.entity());
        let creative=sim.mode==Mode::Creative;
        let presses=sim.click_controller.presses(left);
        let mut target=self.click_target(world,player,sim,picked);
        let events=sim.click_controller.press_phase(&mut target,presses,creative,sim.inventory.held().as_ref(),self.using_item);
        self.apply_controller(events,entity,player,world,sim,&mut out)?;
        if self.using_item {
            self.right_presses = 0;
            self.use_ticks = self.use_ticks.saturating_add(1);
            if !right {
                self.client.send(Command::Dig {
                    action: DigAction::ReleaseUse,
                    position: [0; 3],
                    face: 0,
                })?;
                self.using_item = false;
                self.use_ticks = 0;
            } else if self.use_ticks >= 7 && self.use_ticks % 4 == 0 {
                if let Some(s) = self.held_stack(sim) {
                    if let Some(sound) = Self::use_item_sound(&s) {
                        out.world_sounds.push((
                            sound,
                            DVec3::from_array(player.precise_position()),
                            0.5,
                            0.95,
                        ));
                        if sound == "random.eat" {
                            sim.emit_eating_crumbs(player, s.id, s.meta, 5);
                        }
                    }
                }
            }
        } else {
            let right_presses = std::mem::take(&mut self.right_presses);
            for _ in 0..right_presses {
                self.right_click_mouse(world, player, sim, r, picked, &mut out)?;
            }
            if right && self.delay == 0 && !self.using_item {
                self.right_click_mouse(world, player, sim, r, picked, &mut out)?;
            }
        }
        let mut target=self.click_target(world,player,sim,picked);
        let events=sim.click_controller.held_phase(&mut target,left,creative,sim.inventory.held().as_ref(),self.using_item);
        self.apply_controller(events,entity,player,world,sim,&mut out)?;
        sim.break_target=sim.click_controller.hitting.then_some(sim.click_controller.current);
        sim.breaking=sim.click_controller.damage;
        self.visuals.tick(sim);
        sim.step_particles(world);
        sim.tick_spawner_spins(world, player.position, self.game_mode == 3, true);
        {
            let (rain_strength, _) = sim.weather.strengths(1.0);
            let raining = self.dimension == 0 && rain_strength > 0.2;
            let creative_holding_barrier =
                self.game_mode == 1 && self.held_stack(sim).is_some_and(|s| s.id == 166);
            let mut ambient_sounds = Vec::new();
            crate::ambient_particles::tick_block_and_rain_particles(
                world,
                DVec3::from_array(player.precise_position()),
                player.precise_eye(1.0),
                creative_holding_barrier,
                raining,
                if self.dimension == 0 { rain_strength } else { 0.0 },
                self.visuals.particle_setting,
                self.fancy_graphics,
                sim.total_ticks,
                &mut sim.particles,
                &mut ambient_sounds,
            );
            for s in ambient_sounds {
                self.visuals
                    .sounds
                    .push((s.name.into(), s.volume, s.pitch, s.pos));
            }
        }
        sim.critical_emitters.tick(&mut sim.particles, world, player.position.as_dvec3(),
            self.visuals.particle_setting, |id| self.critical_bounds(id as i32, player));
        self.rider.input.yaw=movement_pose(player).yaw;
        // World updates its loaded entities in insertion order. Crossing a chunk
        // section appends to that section's list, which decides ties when picking.
        let mut dragon_parts=Vec::new();
        let mut client_particles: Vec<(u8, DVec3, DVec3)> = Vec::new();
        for id in &self.entity_order {
            let mounted = self
                .entities
                .get(id)
                .and_then(|e| e.riding)
                .is_some_and(|vid| vid == self.entity_id || self.entities.contains_key(&vid));
            let Some(e) = self.entities.get_mut(id) else {
                continue;
            };
            e.previous_position = e.position;
            e.previous_rotation = e.rotation;
            // The dragon uses deathTicks instead of EntityLivingBase.deathTime.
            e.hurt_camera.tick(e.dead && e.dragon.is_none());
            e.eat_ticks = e.eat_ticks.saturating_sub(1);
            e.attack_ticks = e.attack_ticks.saturating_sub(1);
            e.rose_ticks = e.rose_ticks.saturating_sub(1);
            if e.mob_id == Some(50) {
                e.prev_creeper_swell = e.creeper_swell;
                if e.creeper_state > 0 || e.creeper_ignited {
                    e.creeper_swell = (e.creeper_swell + 1).min(30);
                } else {
                    e.creeper_swell = e.creeper_swell.saturating_sub(1);
                }
            }
            if matches!(e.mob_id, Some(55 | 62)) {
                e.prev_slime_squish = e.slime_squish;
                if e.pose.on_ground && !e.was_on_ground {
                    e.slime_squish = 0.5;
                    if !e.dead {
                        let mut rng = crate::vanilla::random::JavaRandom::from_state(
                            sim.particle_random[2],
                        );
                        gameplay::emit_slime_landing_particles(
                            &mut sim.particles,
                            &mut rng,
                            e.position,
                            i32::from(e.appearance.body.max(1)),
                            e.mob_id == Some(62),
                        );
                        sim.particle_random[2] = rng.state();
                    }
                } else if !e.pose.on_ground && e.was_on_ground {
                    e.slime_squish = -0.35;
                } else {
                    e.slime_squish *= 0.6;
                }
                e.was_on_ground = e.pose.on_ground;
            }
            if e.mob_id == Some(100) {
                e.horse_anim.prev_rearing = e.horse_anim.rearing;
                e.horse_anim.prev_head_lean = e.horse_anim.head_lean;
                e.horse_anim.prev_mouth = e.horse_anim.mouth;
                e.horse_anim.tail_ticks = e.horse_anim.tail_ticks.saturating_sub(1);
                e.horse_anim.run_crazy_ticks = e.horse_anim.run_crazy_ticks.saturating_sub(1);
                if e.horse_anim.tail_ticks == 0
                    && (sim.total_ticks.wrapping_add(local_id(*id).wrapping_mul(31)) % 200) == 0
                {
                    e.horse_anim.tail_ticks = 8;
                }
                let eating_grass = (e.appearance.flags & 32 != 0) && e.horse_anim.rearing == 0.0;
                if eating_grass {
                    e.horse_anim.head_lean =
                        (e.horse_anim.head_lean + (1.0 - e.horse_anim.head_lean) * 0.4 + 0.05)
                            .min(1.0);
                } else {
                    e.horse_anim.head_lean =
                        (e.horse_anim.head_lean - e.horse_anim.head_lean * 0.4 - 0.05).max(0.0);
                }
                let mouth_open = (e.appearance.flags & 128 != 0)
                    || (eating_grass && (sim.total_ticks % 10) < 5);
                if mouth_open {
                    e.horse_anim.mouth =
                        (e.horse_anim.mouth + (1.0 - e.horse_anim.mouth) * 0.7 + 0.05).min(1.0);
                } else {
                    e.horse_anim.mouth =
                        (e.horse_anim.mouth - e.horse_anim.mouth * 0.7 - 0.05).max(0.0);
                }
            }
            if e.dead
                && e.mob_id.is_some()
                && e.dragon.is_none()
                && e.hurt_camera.death_time == 20
                && !e.death_poofed
            {
                e.death_poofed = true;
                let [w, h] = e
                    .mob_id
                    .and_then(|s| {
                        e.appearance
                            .size(s)
                            .or_else(|| crate::remote_mobs::species(s).map(|sp| sp.size))
                            .map(|sz| [sz[0], sz[1]])
                    })
                    .unwrap_or([0.6, 1.8]);
                let mut rng = crate::vanilla::random::JavaRandom::new(
                    (sim.total_ticks ^ (*id as u64).wrapping_mul(0x517C_C1B7_2722_0A95)) as i64,
                );
                for _ in 0..20 {
                    let vx = (rng.next_double() - 0.5) * 0.04;
                    let vy = (rng.next_double() - 0.5) * 0.04;
                    let vz = (rng.next_double() - 0.5) * 0.04;
                    let ppos = DVec3::new(
                        e.position.x + f64::from((rng.next_float() * 2.0 - 1.0) * w),
                        e.position.y + f64::from(rng.next_float() * h),
                        e.position.z + f64::from((rng.next_float() * 2.0 - 1.0) * w),
                    );
                    client_particles.push((0, ppos, DVec3::new(vx, vy, vz)));
                }
            }
            if let Some(flight)=e.dragon.as_mut(){flight.record(e.position.y,e.rotation[0]);}
            if e.dragon.is_some() {
                if e.dead {
                    e.dragon_death_ticks = (e.dragon_death_ticks + 1).min(200);
                } else {
                    e.dragon_death_ticks = 0;
                }
            }
            if mounted {
                if e.steps > 0 {
                    let steps = f64::from(e.steps);
                    let yaw = f64::from(e.rotation[0]);
                    let pitch = f64::from(e.rotation[1]);
                    let dy = (f64::from(e.pose.yaw) - yaw + 180.).rem_euclid(360.) - 180.;
                    e.rotation = [
                        (yaw + dy / steps) as f32,
                        (pitch + (f64::from(e.pose.pitch) - pitch) / steps) as f32,
                    ];
                    e.steps -= 1;
                }
                continue;
            }
            if let Some(v)=e.vehicle.as_mut(){
                let input=(self.riding==Some(*id)).then_some(self.rider.input);
                v.tick(world,&mut e.position,e.pose,&mut e.steps,input);
                if e.mob_id == Some(100) {
                    let (prev_rear, rear) = v.rearing_anim();
                    e.horse_anim.prev_rearing = prev_rear;
                    e.horse_anim.rearing = rear;
                    e.horse_anim.stand_ticks = v.stand_ticks;
                    let horiz_sq = (e.position - e.previous_position)
                        .with_y(0.0)
                        .length_squared();
                    if self.riding == Some(*id)
                        && (v.horse_flags & 4 != 0)
                        && v.grounded
                        && horiz_sq > 0.0025
                    {
                        e.horse_anim.gallop_ticks = e.horse_anim.gallop_ticks.wrapping_add(1);
                    } else if horiz_sq <= 0.0004 {
                        e.horse_anim.gallop_ticks = 0;
                    }
                }
            }else if e.steps>0{
                let steps=f64::from(e.steps);
                e.position+=(e.target_position-e.position)/steps;
                let yaw=f64::from(e.rotation[0]);let pitch=f64::from(e.rotation[1]);
                let dy=(f64::from(e.pose.yaw)-yaw+180.).rem_euclid(360.)-180.;
                e.rotation=[(yaw+dy/steps) as f32,(pitch+(f64::from(e.pose.pitch)-pitch)/steps) as f32];
                e.steps-=1;
            }else if let (
                EntityKind::Object(
                    kind @ (60 | 61 | 62 | 63 | 64 | 65 | 66 | 70 | 72 | 73 | 75 | 76 | 90),
                    _,
                ),
                true,
            ) = (e.kind, e.has_object_velocity)
            {
                if matches!(kind, 63 | 64 | 66) {
                    e.position += e.object_velocity;
                    if e.object_velocity.length_squared() > 1e-9 {
                        let horiz = e.object_velocity.x.hypot(e.object_velocity.z);
                        let target_yaw =
                            (e.object_velocity.z.atan2(e.object_velocity.x).to_degrees()) as f32
                                + 90.0;
                        let target_pitch =
                            (horiz.atan2(e.object_velocity.y).to_degrees()) as f32 - 90.0;
                        let dy =
                            (target_yaw - e.previous_rotation[0] + 180.0).rem_euclid(360.0) - 180.0;
                        let dp = (target_pitch - e.previous_rotation[1] + 180.0).rem_euclid(360.0)
                            - 180.0;
                        e.rotation = [
                            e.previous_rotation[0] + dy * 0.2,
                            e.previous_rotation[1] + dp * 0.2,
                        ];
                    }
                    let in_water = matches!(world.get(e.position.floor().as_ivec3()) >> 4, 8 | 9);
                    let drag = if in_water {
                        for _ in 0..4 {
                            client_particles.push((
                                4,
                                e.position - e.object_velocity * 0.25,
                                e.object_velocity,
                            ));
                        }
                        0.8
                    } else if kind == 66 && e.invulnerable_skull {
                        0.73
                    } else {
                        0.95
                    };
                    e.object_velocity += e.fireball_accel;
                    e.object_velocity *= drag;
                    client_particles.push((11, e.position + DVec3::Y * 0.5, DVec3::ZERO));
                } else if kind == 76 {
                    e.position += e.object_velocity;
                    e.object_velocity.x *= 1.15;
                    e.object_velocity.z *= 1.15;
                    e.object_velocity.y += 0.04;
                    client_particles.push((3, e.position - DVec3::Y * 0.3, DVec3::ZERO));
                } else {
                    let next = e.position + e.object_velocity;
                    let probe = crate::vanilla::movement::Aabb {
                        min: next - DVec3::splat(0.05),
                        max: next + DVec3::splat(0.05),
                    };
                    if crate::vanilla::movement::collision::colliders(world, probe).is_empty() {
                        e.position = next;
                        let (drag, gravity) = match kind {
                            60 | 73 => (0.99, 0.05),
                            61 | 62 | 65 | 75 => (0.99, 0.03),
                            70 => (0.98, 0.04),
                            90 => {
                                let in_water =
                                    matches!(world.get(next.floor().as_ivec3()) >> 4, 8 | 9);
                                (0.92, if in_water { 0.0 } else { 0.03999999910593033 })
                            }
                            _ => (0.95, 0.0),
                        };
                        e.object_velocity.y -= gravity;
                        e.object_velocity *= drag;
                        if kind == 60 && e.object_velocity.length_squared() > 1e-6 {
                            let yaw =
                                (e.object_velocity.x.atan2(e.object_velocity.z).to_degrees()) as f32;
                            let pitch = (e
                                .object_velocity
                                .y
                                .atan2(e.object_velocity.x.hypot(e.object_velocity.z))
                                .to_degrees()) as f32;
                            e.rotation = [yaw, pitch];
                        }
                    } else {
                        e.has_object_velocity = false;
                        e.object_velocity = DVec3::ZERO;
                    }
                }
            }else if e.dragon.is_none(){continue}
            if let Some(flight)=&e.dragon{dragon_parts.push((*id,flight.parts(e.position,e.rotation[0])));}
            let section = crate::vanilla::entity_pick::section(e.position);
            if e.section != section {
                self.section_sequence += 1;
                e.inserted = self.section_sequence;
                e.section = section;
            }
        }
        let listener = player.position.as_dvec3();
        for (pid, ppos, pvel) in client_particles {
            self.visuals
                .spawn_client_particle(sim, world, listener, pid, ppos, pvel);
        }
        let expired_dead: Vec<i32> = self
            .entities
            .iter()
            .filter_map(|(&id, e)| {
                let done = if e.dragon.is_some() {
                    e.dragon_death_ticks >= 200
                } else {
                    e.hurt_camera.death_time >= 20
                };
                (e.pending_destroy && done).then_some(id)
            })
            .collect();
        for id in expired_dead {
            self.remove_entity(id, sim);
        }
        for (parent,parts) in dragon_parts{for (i,p) in parts.into_iter().enumerate(){
            if let Some(e)=self.entities.get_mut(&parent.wrapping_add(i as i32+1)){if matches!(e.kind,EntityKind::DragonPart(_)){e.position=p;e.section=crate::vanilla::entity_pick::section(p);}}
        }}
        self.sync_riding_passengers(player);
        {
            let listener = DVec3::from_array(player.precise_position());
            let particle_setting = self.visuals.particle_setting;
            let mut rng = crate::vanilla::random::JavaRandom::from_state(sim.particle_random[2]);
            for &id in &self.entity_order {
                let Some(e) = self.entities.get(&id) else {
                    continue;
                };
                if e.position.distance_squared(listener) > 32.0 * 32.0 {
                    continue;
                }
                if matches!(e.kind, EntityKind::Object(50, _)) {
                    if sim.particles.len() < 4000 {
                        if let Some(p) = gameplay::Particle::network_sprite(
                            11,
                            e.position + DVec3::Y * 0.5,
                            DVec3::ZERO,
                            &mut rng,
                        ) {
                            sim.particles.push(p);
                        }
                    }
                    continue;
                }
                if e.dead {
                    continue;
                }
                let [w, h] = e
                    .mob_id
                    .and_then(|s| {
                        e.appearance
                            .size(s)
                            .or_else(|| crate::remote_mobs::species(s).map(|sp| sp.size))
                            .map(|sz| [sz[0], sz[1]])
                    })
                    .unwrap_or_else(|| [0.6, entity_height(e)]);
                if let Some(species) = e.mob_id {
                    gameplay::emit_mob_ambient_particles(
                        &mut sim.particles,
                        particle_setting,
                        &mut rng,
                        species,
                        e.position,
                        w,
                        h,
                    );
                }
                if e.potion_color != 0 && e.living() {
                    gameplay::emit_potion_swirl(
                        &mut sim.particles,
                        particle_setting,
                        &mut rng,
                        e.position,
                        w,
                        h,
                        e.potion_color,
                        e.potion_ambient,
                        e.flags & 0x20 != 0,
                    );
                }
                if let EntityKind::Player(uuid) = e.kind {
                    if (e.flags & 0x08 != 0)
                        && (e.flags & 0x20 == 0)
                        && self.player_modes.get(&uuid) != Some(&3)
                        && e.pose.on_ground
                        && e.riding.is_none()
                        && !matches!(world.get(e.position.floor().as_ivec3()) >> 4, 8 | 9)
                    {
                        let delta = e.position - e.previous_position;
                        gameplay::emit_running_dust(
                            &mut sim.particles,
                            world,
                            &mut rng,
                            e.position,
                            0.6,
                            delta.x,
                            delta.z,
                        );
                    }
                }
            }
            sim.particle_random[2] = rng.state();
        }
        self.local_player_center = player.position + Vec3::Y * 0.9;
        self.sync_scene(sim);
        sim.mobs.retain(|m| {
            self.entities
                .get(&(m.id as i32))
                .is_some_and(|e| !(e.dead && e.dragon.is_none() && e.hurt_camera.death_time >= 20))
        });
        for m in &mut sim.mobs {
            m.previous = m.position;
            m.hurt = m.hurt.saturating_sub(1);
            m.age = m.age.wrapping_add(1);
            m.swing_ticks = self
                .extra
                .swings
                .get(&(m.id as i32))
                .map_or(0, |&age| 6u8.saturating_sub(age));
            if let Some(e) = self.entities.get_mut(&(m.id as i32)) {
                m.position = e.position.as_vec3();
                sim.mob_brains.entry(m.id).or_default().gait.advance(m.yaw, (m.position - m.previous).with_y(0.).length());
                m.yaw=(180. + e.rotation[0]).to_radians();
                m.velocity = m.position - m.previous;
                m.death_ticks = e.hurt_camera.death_time;
                m.eat_ticks = e.eat_ticks;
                m.attack_ticks = e.attack_ticks;
                m.rose_ticks = e.rose_ticks;
                m.prev_fuse = e.prev_creeper_swell;
                m.fuse = e.creeper_swell;
            }
        }
        sim.tick_remote_drops(world, &mut out);
        for d in &sim.drops {
            if let Some(e) = self.entities.get_mut(&(d.id as i32)) {
                e.position = d.position;
            }
        }
        for orb in &mut sim.xp_orbs {
            if let Some(e)=self.entities.get(&orb.id){orb.previous=orb.position;orb.position=e.position;orb.age=orb.age.wrapping_add(1);}
        }
        player.can_sprint = sim.food > 6 || player.allow_flying;
        player.using_item = self.using_item;
        player.suppress_view_input = self.extra.camera.is_some();
        sim.effects.tick_client();
        sim.hand_swing.advance(crate::first_person::swing_duration(&sim.effects));
        let held_clone = sim.inventory.held();
        let used_ticks = self.using_item.then_some(u32::from(self.use_ticks));
        let bow_ticks = held_clone
            .as_ref()
            .filter(|stack| stack.id == 261)
            .and(used_ticks);
        sim.equipped
            .tick_with_fov(held_clone, used_ticks, player.fov_modifier(bow_ticks));
        player.third_person.swinging=sim.hand_swing.in_progress();
        player.jump_boost = sim.effects.get(8).map(|e| e.amplifier);
        player.blinded = sim.effects.get(15).is_some();
        player.depth_strider = sim.inventory.armor_enchantment(8);
        {
            let mut prng = crate::vanilla::random::JavaRandom::from_state(sim.particle_random[2]);
            let portal_rand = prng.next_float();
            sim.particle_random[2] = prng.state();
            if let Some(pitch) = player.step_portal_client(
                sim.effects.get(9).is_some_and(|e| e.duration > 60),
                portal_rand,
            ) {
                out.pitched_sounds.push(("portal.trigger", 0.25, pitch));
            }
        }
        // Speed/slowness attributes come from S20 on remote sessions; applying
        // their potion modifiers here would duplicate server-owned attributes.
        if let Some((rid, seat, mount_yaw, saddled)) = self.riding.and_then(|rid| {
            let mount = self.vehicle_mount_point(rid, player)?;
            let vehicle = self.entities.get(&rid)?;
            let seat = mount + DVec3::Y * (-0.35);
            let body_yaw = vehicle.vehicle.as_ref().map_or(vehicle.rotation[0], |v| v.yaw);
            let mount_yaw = vehicle.mob_id.map(|_| {
                let last = player.third_person.mount_yaw.map_or(body_yaw, |p| p[1]);
                [last, body_yaw]
            });
            let saddled = vehicle
                .vehicle
                .as_ref()
                .is_some_and(|v| v.kind == vehicle::Kind::Horse && v.horse_flags & 4 != 0);
            Some((rid, seat, mount_yaw, saddled))
        }) {
            use winit::keyboard::KeyCode;
            self.fall_distance = 0.0;
            let java_yaw = player.java_rotation()[0];
            player.third_person.tick(DVec3::ZERO, java_yaw);
            player.third_person.riding = true;
            player.third_person.mount_yaw = mount_yaw;
            player.set_position_f64(seat.to_array());
            player.set_velocity_f64([0.; 3]);
            player.riding = true;
            self.local_player_center = player.position + Vec3::Y * 0.9;
            let input = |pos, neg| {
                f32::from(player.keys.contains(&pos)) - f32::from(player.keys.contains(&neg))
            };
            let sneak = player.sneaking();
            let scale = if sneak { 0.3f32 } else { 1. };
            let pose = movement_pose(player);
            let jump = player.keys.contains(&KeyCode::Space);
            let strafe = input(KeyCode::KeyA, KeyCode::KeyD) * scale * 0.98;
            let forward = input(KeyCode::KeyW, KeyCode::KeyS) * scale * 0.98;
            self.rider.input = vehicle::RiderInput {
                strafe,
                forward,
                yaw: pose.yaw,
            };
            if let Some(power) = self.rider.jump(saddled, jump) {
                if power > 0 {
                    if let Some(re) = self.entities.get_mut(&rid) {
                        re.horse_anim.stand_ticks = 20;
                        if let Some(rv) = re.vehicle.as_mut() {
                            rv.stand_ticks = 20;
                        }
                    }
                }
                self.client.send(Command::EntityAction {
                    action: EntityAction::HorseJump,
                    parameter: power,
                })?;
            }
            self.client.send(Command::Riding {
                pose,
                correction_id: self.correction,
                strafe,
                forward,
                jump,
                sneak,
            })?;
            sim.tick_remote_pistons(world, player, &mut out, |_, _, _| {});
            return Ok(out);
        }
        let boats: Vec<_> = self
            .entity_order
            .iter()
            .filter(|&&id| Some(id) != self.riding)
            .filter_map(|id| self.entities.get(id))
            .filter(|e| !e.dead && matches!(e.kind, EntityKind::Object(1, _)))
            .map(|e| crate::vanilla::entity_pick::body(e.position, 1.5, 0.6))
            .collect();
        let border_bounds = sim.world_border.bounds(crate::vanilla::click_border::now());
        if player.sleeping {
            self.fall_distance = 0.0;
            player.third_person.tick(DVec3::ZERO, player.java_rotation()[0]);
        } else {
            let old_feet = DVec3::from_array(player.precise_position());
            let was_grounded = player.grounded;
            let was_in_water = player.in_water;
            let pre_vel = player.precise_velocity();
            let collision_world = RemoteCollisionWorld {
                world,
                border_bounds,
                feet: player.precise_position(),
                outside_border: std::cell::Cell::new(self.outside_border),
                boats: boats.clone(),
            };
            player.tick_with(&collision_world);
            self.outside_border = collision_world.outside_border.get();
            player.apply_portal_contact();
            let feet = DVec3::from_array(player.precise_position());
            self.emit_local_movement_sounds(world, player, old_feet, was_grounded, feet);
            if self.game_mode != 3 {
                let mut fx_rng =
                    crate::vanilla::random::JavaRandom::from_state(sim.particle_random[2]);
                if !was_in_water && player.in_water && pre_vel[1] < -0.2 {
                    out.world_sounds.push(gameplay::emit_water_entry_splash(
                        &mut sim.particles,
                        &mut fx_rng,
                        feet,
                        0.6,
                        pre_vel,
                    ));
                }
                if player.sprinting && player.grounded && !player.in_water && !player.flying {
                    let vel = player.precise_velocity();
                    gameplay::emit_running_dust(
                        &mut sim.particles,
                        world,
                        &mut fx_rng,
                        feet,
                        0.6,
                        vel[0],
                        vel[2],
                    );
                }
                if let Some((color, ambient)) = gameplay::effects_liquid_color(&sim.effects) {
                    gameplay::emit_potion_swirl(
                        &mut sim.particles,
                        self.visuals.particle_setting,
                        &mut fx_rng,
                        feet,
                        0.6,
                        1.8,
                        color,
                        ambient,
                        sim.effects.get(14).is_some_and(|e| e.duration > 0),
                    );
                }
                sim.particle_random[2] = fx_rng.state();
            }
        }
        for &flying in player.flight_changes() {
            self.client.send(Command::Flying(flying))?;
        }
        if player.sprinting != self.sprint {
            self.sprint = player.sprinting;
            self.client.send(Command::EntityAction {
                action: if self.sprint {
                    EntityAction::StartSprint
                } else {
                    EntityAction::StopSprint
                },
                parameter: 0,
            })?;
        }
        let sneak_state = player.sneaking() && !player.sleeping;
        if sneak_state != self.sneak {
            self.sneak = sneak_state;
            self.client.send(Command::EntityAction {
                action: if self.sneak {
                    EntityAction::StartSneak
                } else {
                    EntityAction::StopSneak
                },
                parameter: 0,
            })?;
        }
        if self.extra.camera.is_none(){self.client.send(Command::Movement {
            pose: movement_pose(player),
            correction_id: self.correction,
        })?;}
        let mut outside_border = self.outside_border;
        sim.tick_remote_pistons(world, player, &mut out, |w, p, delta| {
            let collision_world = RemoteCollisionWorld {
                world: w,
                border_bounds,
                feet: p.precise_position(),
                outside_border: std::cell::Cell::new(outside_border),
                boats: boats.clone(),
            };
            p.move_by_piston(&collision_world, delta);
            outside_border = collision_world.outside_border.get();
        });
        self.outside_border = outside_border;
        Ok(out)
    }
    pub fn middle_click(
        &mut self,
        world: &World,
        player: &Player,
        sim: &mut Simulation,
        ctrl: bool,
    ) -> Result<()> {
        use crate::vanilla::{entity_pick::Hit, middle_pick};
        let creative = sim.mode == Mode::Creative;
        let valid = match self.pick(world, player, sim) {
            Some(Hit::Block(hit)) => middle_pick::block_click(
                &mut sim.inventory,
                world,
                hit.position,
                creative,
                ctrl,
                None,
                world.tile(hit.position),
            ),
            Some(Hit::Entity { id, .. }) if creative => {
                let e = &self.entities[&id];
                let registry = match e.kind {
                    _ if e.mob_id.is_some()=>e.mob_id.unwrap(),
                    EntityKind::Mob(kind) => kind.vanilla_id(),
                    EntityKind::Object(1, _) => 41,
                    EntityKind::Object(71, _) => 18,
                    EntityKind::Object(77, _) => 8,
                    EntityKind::Object(78, _) => 30,
                    EntityKind::Object(10, kind) => [42, 43, 44, 45, 46, 47, 40]
                        .get(kind as usize)
                        .copied()
                        .unwrap_or(42),
                    _ => return Ok(()),
                };
                let Some(item) = middle_pick::entity(registry, e.frame_item.as_ref()) else {
                    return Ok(());
                };
                middle_pick::select(&mut sim.inventory, item, true);
                true
            }
            _ => false,
        };
        if valid && !self.defer_tick_commands {
            self.sync_held_slot(sim)?;
        }
        if valid && creative {
            let slot = 36 + sim.inventory.selected;
            let item = sim.inventory.slots[sim.inventory.selected].as_ref();
            let stack = item.map(to_slot).transpose()?;
            self.queue_tick_command(Command::CreativeSlot {
                slot: slot as i16,
                stack: stack.clone(),
            })?;
            if self.slots.len() <= slot {
                self.slots.resize(slot + 1, None);
            }
            self.slots[slot] = stack;
        }
        Ok(())
    }

    fn emit_local_movement_sounds(
        &mut self,
        world: &World,
        player: &Player,
        old_feet: DVec3,
        was_grounded: bool,
        feet: DVec3,
    ) {
        if player.is_spectator() || self.game_mode == 3 || player.flying {
            self.fall_distance = 0.0;
            return;
        }
        let delta = feet - old_feet;
        let mut under = IVec3::new(
            feet.x.floor() as i32,
            (feet.y - f64::from(0.2_f32)).floor() as i32,
            feet.z.floor() as i32,
        );
        let mut block = world.get(under);
        if block == 0 {
            let below = world.get(under - IVec3::Y);
            if matches!(below >> 4, 85 | 107 | 113 | 139 | 183..=192) {
                under -= IVec3::Y;
                block = below;
            }
        }
        let in_web = world.get(feet.floor().as_ivec3()) >> 4 == 30
            || world.get((feet + DVec3::Y).floor().as_ivec3()) >> 4 == 30;
        if player.in_water || player.on_ladder || in_web {
            self.fall_distance = 0.0;
        } else if player.grounded {
            if self.fall_distance > 3.0
                && !player.allow_flying
                && !(block >> 4 == 165 && !player.sneaking())
            {
                let boost = player
                    .jump_boost
                    .map_or(0.0, |level| level.wrapping_add(1) as f32);
                let amount = (self.fall_distance - 3.0 - boost).ceil();
                if amount > 0.0 {
                    let fall_sound = if amount > 4.0 {
                        "game.player.hurt.fall.big"
                    } else {
                        "game.player.hurt.fall.small"
                    };
                    self.visuals
                        .sounds
                        .push((fall_sound.into(), 1.0, 1.0, feet));
                    if block != 0 {
                        if let Some((clip, m_vol, m_pitch)) =
                            crate::vanilla::click_controller::step_sound(block)
                        {
                            self.visuals.sounds.push((
                                clip.into(),
                                (m_vol * 8.0 - 1.0) * 0.5,
                                m_pitch * 1.5,
                                feet,
                            ));
                        }
                    }
                }
            }
            self.fall_distance = 0.0;
        } else if delta.y < 0.0 {
            self.fall_distance += (-delta.y) as f32;
        }
        if !(was_grounded && player.sneaking()) {
            let step_delta = if player.in_water || player.on_ladder {
                delta.length() as f32 * 0.6
            } else {
                delta.x.hypot(delta.z) as f32 * 0.6
            };
            self.step_distance += step_delta;
            if (player.grounded || player.in_water || player.on_ladder)
                && self.step_distance > self.next_step_distance
            {
                self.next_step_distance = self.step_distance.floor() + 1.0;
                if player.in_water {
                    let [vx, vy, vz] = player.precise_velocity();
                    let speed = (vx * vx * 0.2 + vy * vy + vz * vz * 0.2).sqrt() as f32;
                    let vol = (speed * 0.35).min(1.0);
                    let pitch = 1.0
                        + (self.pickup_random.next_float() - self.pickup_random.next_float())
                            * 0.4;
                    self.visuals
                        .sounds
                        .push(("game.player.swim".into(), vol, pitch, feet));
                } else {
                    let above = world.get(under + IVec3::Y);
                    let step_state = if above >> 4 == 78 {
                        78 << 4
                    } else if player.on_ladder && block == 0 {
                        65 << 4
                    } else {
                        block
                    };
                    if !matches!(step_state >> 4, 0 | 8..=11) {
                        if let Some((clip, m_vol, m_pitch)) =
                            crate::vanilla::click_controller::step_sound(step_state)
                        {
                            self.visuals.sounds.push((
                                clip.into(),
                                (m_vol * 8.0 - 1.0) * 0.15,
                                m_pitch * 2.0,
                                feet,
                            ));
                        }
                    }
                }
            }
        }
    }
}
struct RemoteCollisionWorld<'a> {
    world: &'a World,
    border_bounds: [f64; 4],
    feet: [f64; 3],
    outside_border: std::cell::Cell<bool>,
    boats: Vec<crate::vanilla::movement::Aabb>,
}
impl crate::vanilla::movement::MovementWorld for RemoteCollisionWorld<'_> {
    fn get(&self, p: IVec3) -> u16 {
        self.world.get(p)
    }
    fn block_boxes(&self, p: IVec3, state: u16, out: &mut Vec<crate::vanilla::movement::Aabb>) {
        let bl = self.outside_border.get();
        let [mut d, mut d2, mut d3, mut d4] = self.border_bounds;
        if bl {
            d += 1.0;
            d2 += 1.0;
            d3 -= 1.0;
            d4 -= 1.0;
        } else {
            d -= 1.0;
            d2 -= 1.0;
            d3 += 1.0;
            d4 += 1.0;
        }
        let bl2 = self.feet[0] > d && self.feet[0] < d3 && self.feet[2] > d2 && self.feet[2] < d4;
        if bl && bl2 {
            self.outside_border.set(false);
        } else if !bl && !bl2 {
            self.outside_border.set(true);
        }
        let [px, pz] = [f64::from(p.x), f64::from(p.z)];
        let contains = px + 1.0 > self.border_bounds[0]
            && px < self.border_bounds[2]
            && pz + 1.0 > self.border_bounds[1]
            && pz < self.border_bounds[3];
        let state = if contains || !bl2 { state } else { 1 << 4 };
        self.world.block_boxes(p, state, out);
    }
    fn loaded_column(&self, x: i32, z: i32) -> bool {
        self.world.loaded_column(x, z)
    }
    fn extra_colliders(
        &self,
        query: crate::vanilla::movement::Aabb,
        out: &mut Vec<crate::vanilla::movement::Aabb>,
    ) {
        for &boat in &self.boats {
            if boat.intersects(query) {
                out.push(boat);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "uses installed resources for network-client world ticks"]
    fn network_bed_pose_and_sleep_fade_follow_the_server_until_wake() {
        use crate::network::play_events::Update;
        let (client, sent, incoming) = Client::test_transport();
        let mut remote = Remote::with_client(client);
        remote.entity_id = 1;
        remote.ready = true;
        let head = IVec3::new(8, 64, 8);
        let state = (26 << 4) | 8 | 1;
        let mut player = Player::new(head.as_vec3());
        let mut sim = Simulation::default();
        let mut world = World {
            chunks: [((0, 0), Arc::new(Chunk::empty()))].into(), spawn: player.position,
            label: String::new(), save_path: Default::default(), edits: Default::default(),
            imported: true, light_revision: 0,
        };
        world.set(head, state);
        remote.spawn_entity(2, EntityKind::Player([0; 16]), movement_pose(&player));
        let mut dirty = HashSet::new();
        let rest = crate::vanilla::sleep::resting_position(head, state);
        for id in [1,2] {
            incoming.send(Event::WorldAction(Update::Bed(id, head))).unwrap();
        }
        remote.poll_world(&mut world, &mut player, &mut sim, &mut dirty, None).unwrap();
        for _ in 0..103 {
            remote.tick(&mut world, &mut player, &mut sim, crate::validation::resources(), false, false).unwrap();
            sent.try_iter().for_each(drop);
        }
        assert!(player.sleeping);
        assert_eq!(player.precise_position(), rest);
        assert_eq!(sim.sleep.timer, 100);
        assert_eq!(sim.server_scene.actors[&2].bed, Some(head));
        assert_eq!(sim.server_scene.actors[&2].feet, DVec3::from(rest).as_vec3());
        for id in [1,2] {
            incoming.send(Event::WorldAction(Update::Animation(id, 2))).unwrap();
        }
        remote.poll_world(&mut world, &mut player, &mut sim, &mut dirty, None).unwrap();
        assert!(!player.sleeping);
        for _ in 0..10 {
            remote.tick(&mut world, &mut player, &mut sim, crate::validation::resources(), false, false).unwrap();
            sent.try_iter().for_each(drop);
        }
        assert_eq!(sim.sleep.timer, 0);
        assert_eq!(sim.server_scene.actors[&2].bed, None);
    }
    #[test]
    fn fire_watchers_drive_local_player_mobs_and_charged_blaze_in_the_scene() {
        let (client, _, _) = Client::test_transport();
        let mut remote = Remote::with_client(client);
        let mut sim = Simulation::default();
        let mut player = Player::new(Vec3::ZERO);
        remote.entity_id = 10;
        let pose = movement_pose(&player);
        for (id, species) in [(11, 91), (12, 61), (13, 62), (14, 51)] {
            remote.spawn_entity(id, EntityKind::Other, pose);
            remote.entities.get_mut(&id).unwrap().mob_id = Some(species);
        }
        for id in 10..=14 {
            remote.metadata(id, vec![(0, Metadata::Byte(9))], &mut player, &mut sim).unwrap();
        }
        remote.metadata(14, vec![(13, Metadata::Byte(1))], &mut player, &mut sim).unwrap();
        remote.sync_scene(&mut sim);
        assert!(sim.player_burning());
        assert!(player.sprinting);
        assert_eq!(sim.server_scene.burning, [11].into_iter().collect());
        remote.metadata(10, vec![(0, Metadata::Byte(8))], &mut player, &mut sim).unwrap();
        remote.metadata(11, vec![(0, Metadata::Byte(8))], &mut player, &mut sim).unwrap();
        remote.metadata(12, vec![(16, Metadata::Byte(1))], &mut player, &mut sim).unwrap();
        remote.metadata(14, vec![(13, Metadata::Byte(0))], &mut player, &mut sim).unwrap();
        remote.sync_scene(&mut sim);
        assert!(!sim.player_burning());
        assert!(player.sprinting);
        remote.metadata(10, vec![(0, Metadata::Byte(0))], &mut player, &mut sim).unwrap();
        assert!(!player.sprinting);
        assert_eq!(sim.server_scene.burning, [12, 14].into_iter().collect());
    }
    #[test]
    fn remote_critical_animation_uses_the_server_target_and_sprite() {
        use crate::network::play_events::Update;
        let (client, _sent, incoming) = Client::test_transport();
        let mut remote = Remote::with_client(client);
        remote.entity_id = 1;
        let mut player = Player::new(Vec3::new(8., 64., 8.));
        let mut sim = Simulation::default();
        let mut world = World {
            chunks: [((0, 0), Arc::new(Chunk::empty()))].into(), spawn: player.position,
            label: String::new(), save_path: Default::default(), edits: Default::default(),
            imported: true, light_revision: 0,
        };
        let mut dirty = HashSet::new();
        let mut target = movement_pose(&player);
        target.x += 3.;
        remote.spawn_entity(2, EntityKind::Player([0; 16]), target);
        for (id, kind, frame) in [(999, 4, 65), (1, 4, 65), (2, 5, 66)] {
            sim.particles.clear();
            sim.critical_emitters = Default::default();
            incoming.send(Event::WorldAction(Update::Animation(id, kind))).unwrap();
            remote.poll_world(&mut world, &mut player, &mut sim, &mut dirty, None).unwrap();
            assert_eq!(sim.critical_emitters.is_empty(), id == 999);
            // Empty sphere samples are valid. The emitter still follows its entity for two more ticks.
            for _ in 0..2 {
                sim.critical_emitters.tick(&mut sim.particles, &world, player.position.as_dvec3(), 0,
                    |id| remote.critical_bounds(id as i32, &player));
            }
            assert!(sim.critical_emitters.is_empty());
            assert_eq!(sim.particles.is_empty(), id == 999);
            let target_x = if id == 1 { player.position.x } else { target.x as f32 };
            for particle in &sim.particles {
                let (position, _, _, _, uv) = particle.appearance(0.);
                assert_eq!(uv[..2], [(frame % 16) as f32 / 32., (frame / 16) as f32 / 32.]);
                assert!((position.x - target_x).abs() < 0.16);
            }
        }
    }
    #[test]
    #[ignore = "requires locally imported resources for the multiplayer client tick"]
    fn remote_hurt_camera_follows_health_status_animation_and_spectator_target() {
        use crate::network::play_events::Update;
        let (client, _sent, incoming) = Client::test_transport();
        let mut remote = Remote::with_client(client);
        let mut player = Player::new(Vec3::new(8., 64., 8.));
        let mut sim = Simulation::default();
        let mut world = World {
            chunks: [((0, 0), Arc::new(Chunk::empty()))].into(), spawn: player.position,
            label: String::new(), save_path: Default::default(), edits: Default::default(),
            imported: true, light_revision: 0,
        };
        let pose = movement_pose(&player);
        let mut dirty = HashSet::new();
        let health = |health| Event::Health { health, food: 20, saturation: 5. };
        let status = |entity_id, status| Event::EntityStatus { entity_id, status };
        let animation = |id, kind| Event::WorldAction(Update::Animation(id, kind));
        let camera = |id| Event::WorldAction(Update::Camera(id));
        let mob = Event::SpawnPlayer { entity_id: 2, uuid: [0; 16], pose, held: 0, metadata: vec![] };
        let boat = Event::SpawnObject { entity_id: 3, kind: 1, pose, data: 0, velocity: None };
        let dead = Event::EntityMetadata { entity_id: 2, metadata: vec![(6, Metadata::Float(0.))] };
        let respawn = Event::Respawn { dimension: 0, gamemode: 0, difficulty: 2, flat_world: true };
        // Each None runs a normal client tick, otherwise sample immediately after packet dispatch.
        // State is (own hurt time, camera target hurt time/death time/dead).
        remote.ready = true;
        remote.entity_id = 1;
        for (packet, own, target) in [
            (Some(health(12.)), 0, None), (Some(health(13.)), 0, None),
            (Some(health(9.)), 10, None), (None, 9, None),
            (Some(health(9.)), 9, None), (Some(health(10.)), 9, None),
            // WorldServer can deliver the player's S1C health change before S06.
            // The duplicate value cannot restart the client's active hurt animation.
            (Some(Event::EntityMetadata { entity_id: 1, metadata: vec![(6, Metadata::Float(7.))] }), 9, None),
            (Some(health(7.)), 9, None),
            (Some(animation(1, 1)), 10, None), (None, 9, None),
            (Some(status(1, 2)), 10, None), (None, 9, None),
            (Some(mob), 9, None), (Some(status(2, 2)), 9, None),
            (Some(camera(2)), 9, Some((10, 0, false))),
            (None, 8, Some((9, 0, false))),
            (Some(animation(2, 0)), 8, Some((9, 0, false))),
            (Some(animation(2, 1)), 8, Some((10, 0, false))),
            (Some(dead), 8, Some((10, 0, true))), (None, 7, Some((9, 1, true))),
            (Some(boat), 7, Some((9, 1, true))), (Some(camera(3)), 7, Some((0, 0, false))),
            (Some(animation(3, 1)), 7, Some((0, 0, false))),
            (Some(Event::DestroyEntities(vec![3])), 7, None),
            (Some(status(1, 3)), 7, None), (None, 6, None),
            (Some(respawn), 0, None), (Some(health(5.)), 0, None),
        ] {
            if let Some(packet) = packet {
                incoming.send(packet).unwrap();
                remote.poll_world(&mut world, &mut player, &mut sim, &mut dirty, None).unwrap();
            } else {
                remote.tick(&mut world, &mut player, &mut sim, crate::validation::resources(), false, false).unwrap();
            }
            assert_eq!(sim.hurt_camera.hurt_time, own);
            assert_eq!(sim.server_scene.camera_hurt.map(|(h, dead)| (h.hurt_time, h.death_time, dead)), target);
        }
        assert_eq!(sim.hurt_camera.death_time, 0);
    }
    #[test]
    #[ignore="run scripts/test_vanilla_loopback.py --vehicle-probe against the original server"]
    fn installed_189_horse_ride(){
        let port = std::env::var("MC189_LOOPBACK_PORT").expect("run scripts/test_vanilla_loopback.py --vehicle-probe");
        let settings=crate::settings::Settings{render_distance:2,..Default::default()};
        let client=Client::connect(&format!("127.0.0.1:{port}"),"HorseProbe",(&settings).into()).unwrap();
        let mut remote=Remote::with_client(client);
        let mut world=World{chunks:Default::default(),spawn:Vec3::ZERO,label:String::new(),save_path:Default::default(),edits:Default::default(),imported:true,light_revision:0};
        let mut sim=Simulation::default();let mut player=Player::new(Vec3::new(0.,64.,0.));let mut dirty=HashSet::new();
        let res=crate::validation::resources();let since=std::time::Instant::now();let mut riding_ticks=0;
        let (mut jump,mut travel)=(false,false);
        loop{
            remote.poll_world(&mut world,&mut player,&mut sim,&mut dirty,None).unwrap();
            if remote.riding_horse(){
                use winit::keyboard::KeyCode;
                riding_ticks+=1;player.keys.insert(KeyCode::KeyW);
                if (6..=15).contains(&riding_ticks){player.keys.insert(KeyCode::Space);}else{player.keys.remove(&KeyCode::Space);}
            }
            for (line,_) in remote.chat_lines.drain(..){jump|=line.contains("oracle-horse-jump");travel|=line.contains("oracle-horse-travel");}
            if jump && travel{return}
            remote.tick(&mut world,&mut player,&mut sim,res,false,false).unwrap();
            assert!(since.elapsed()<std::time::Duration::from_secs(30),"mounted_ticks={riding_ticks}, travel={travel}, jump={jump}");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    #[test]
    #[ignore="run scripts/test_vanilla_loopback.py --mob-probe with the installed original server"]
    fn installed_189_mob_attack(){
        let port = std::env::var("MC189_LOOPBACK_PORT").expect("run scripts/test_vanilla_loopback.py --mob-probe");
        let settings=crate::settings::Settings{render_distance:2,..Default::default()};
        let client=Client::connect(&format!("127.0.0.1:{port}"),"MobProbe",(&settings).into()).unwrap();
        let mut remote=Remote::with_client(client);
        let mut world=World{chunks:Default::default(),spawn:Vec3::ZERO,label:String::new(),save_path:Default::default(),edits:Default::default(),imported:true,light_revision:0};
        let mut player=Player::new(Vec3::new(0.,64.,0.));let mut sim=Simulation::default();let mut dirty=HashSet::new();
        let res=crate::validation::resources();let since=std::time::Instant::now();let mut clicked=false;
        loop{
            remote.poll_world(&mut world,&mut player,&mut sim,&mut dirty,None).unwrap();
            if remote.chat_lines.iter().any(|(s,_)|s.contains("oracle-golem-hit-")){
                assert!(clicked);return
            }
            let target=remote.pick(&world,&player,&sim);
            let click=!clicked && matches!(target,Some(crate::vanilla::entity_pick::Hit::Entity{id,..}) if sim.server_scene.mobs.get(&id).is_some_and(|m|m.species==99));
            if click{sim.click_controller.queue_press();clicked=true;}
            remote.tick(&mut world,&mut player,&mut sim,res,click,false).unwrap();
            assert!(since.elapsed()<std::time::Duration::from_secs(30),"No original server damage confirmation. clicked={clicked} pos={:?} target={target:?}",player.precise_position());
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    #[test]
    #[ignore="run scripts/test_vanilla_loopback.py --window-probe with the installed original server"]
    fn installed_189_anvil_rename() {
        use crate::inventory::{SlotRef,ClickAction};
        use crate::ui::special_windows::Special;
        let port=std::env::var("MC189_LOOPBACK_PORT").expect("loopback fixture port");
        let settings=crate::settings::Settings{render_distance:2,..Default::default()};
        let client=Client::connect(&format!("127.0.0.1:{port}"),"WindowProbe",(&settings).into()).unwrap();
        let mut remote=Remote::with_client(client);
        let mut world=World{chunks:Default::default(),spawn:Vec3::ZERO,label:String::new(),save_path:Default::default(),edits:Default::default(),imported:true,light_revision:0};
        let mut player=Player::new(Vec3::new(0.,64.,0.));
        let mut sim=Simulation::default();
        let mut dirty=HashSet::new();
        let mut special=Special::default();
        let mut phase=0;
        let resources=crate::validation::resources();
        let since=std::time::Instant::now();
        loop {
            remote.poll_world(&mut world,&mut player,&mut sim,&mut dirty,None).unwrap();
            if remote.chat_lines.iter().any(|(s,_)|s.contains("oracle-anvil-repaired-1")) {
                assert_eq!(phase,4);
                return;
            }
            let hit=remote.pick(&world,&player,&sim).and_then(|p|p.block());
            let use_block=phase==0 && hit.is_some_and(|h|world.get(h.position)>>4==145);
            remote.tick(&mut world,&mut player,&mut sim,resources,false,use_block).unwrap();
            if use_block {phase=1;}
            if let Some(w)=remote.extra.window.as_ref().filter(|w|w.kind=="minecraft:anvil") {
                let initial=special.sync(&w.kind,w.container.slots[0].as_ref(),&w.properties,resources,"ko_KR");
                let got_input=w.container.slots[0].as_ref().is_some_and(|s|s.id==276);
                // w.slots is the received S2F wire stack. The displayed output can
                // already contain a predicted result before the Java server replies.
                let got_renamed=w.slots[2].as_ref().and_then(|s|from_slot(s).ok()).is_some_and(|s|{
                    s.id==276 && crate::vanilla::anvil::custom_name(&s)==Some("모루 189")
                });
                let got_repaired=w.slots[2].as_ref().and_then(|s|from_slot(s).ok()).is_some_and(|s|
                    s.id==257 && s.damage==0 && crate::vanilla::anvil::custom_name(&s).is_none());
                let cost=w.properties.get(&0).copied();
                let materials=w.container.anvil_materials;
                if let Some(initial)=initial {remote.rename_anvil(initial).unwrap();}
                if phase==1 && got_input {
                    let Special::Anvil{field,..}=&mut special else {panic!("Missing anvil GUI")};
                    field.key(winit::keyboard::KeyCode::KeyA,false,true,30);
                    field.insert("모루 189",30);
                    remote.rename_anvil(special.rename().unwrap()).unwrap();
                    phase=2;
                } else if phase==2 && got_renamed {
                    assert_eq!(cost,Some(1),"cost for renaming a new diamond sword");
                    remote.window_click(SlotRef::Container(2),ClickAction::Pickup{right:false},&mut sim).unwrap();
                    phase=3;
                } else if phase==3 && got_repaired && remote.chat_lines.iter().any(|(s,_)|s.contains("oracle-anvil-collected-4")) {
                    assert_eq!((cost,materials),(Some(3),3),"three quarters of iron pickaxe repair");
                    remote.window_click(SlotRef::Container(2),ClickAction::Pickup{right:false},&mut sim).unwrap();
                    let repaired=&remote.extra.window.as_ref().unwrap().container.slots;
                    assert!(repaired[0].is_none());
                    assert_eq!(repaired[1].as_ref().map(|s|(s.id,s.count)),Some((265,3)));
                    phase=4;
                }
            }
            assert!(since.elapsed()<std::time::Duration::from_secs(30),"Anvil probe timed out, phase={phase}, hit={hit:?}, pose={:?}, window={:?}, xp={}",player.precise_position(),remote.extra.window.as_ref().map(|w|&w.kind),sim.xp.level);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    #[test]
    fn inventory_wire_mapping_is_bijective() {
        for i in 0..40 {
            assert_eq!(wire_index(i).and_then(inventory_index), Some(i));
        }
    }
    #[test]
    fn metadata_and_durability_are_distinct() {
        assert_eq!(
            from_slot(&Slot {
                id: 35,
                count: 2,
                damage: 14,
                nbt: vec![0]
            })
            .unwrap(),
            Stack {
                nbt: None,
                identity: crate::inventory::Stack::fresh_identity(),
                id: 35,
                count: 2,
                meta: 14,
                damage: 0
            }
        );
        assert_eq!(
            from_slot(&Slot {
                id: 276,
                count: 1,
                damage: 379,
                nbt: vec![0]
            })
            .unwrap()
            .damage,
            379
        );
    }
    #[test]
    fn authoritative_chunk_updates_replace_only_selected_sections() {
        let mut world = World {
            chunks: crate::world::ChunkMap::new(),
            spawn: Vec3::ZERO,
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: true,
            light_revision: 0,
        };
        let mut blocks = vec![0; 65536];
        blocks[0] = 16;
        blocks[4096] = 32;
        blocks[4097] = 8 << 4;
        apply_chunk(
            &mut world,
            ChunkData {
                x: -1,
                z: 2,
                ground_up: true,
                mask: 3,
                blocks,
                block_light: vec![0x42; 4096],
                sky_light: Some(vec![0x75; 4096]),
                biomes: Some(vec![21; 256]),
            },
        );
        world.chunks[&(-1, 2)].prepare_tick_indexes();
        assert_eq!(world.chunks[&(-1, 2)].random_tick_mask(), 2);
        assert_eq!(world.chunks[&(-1, 2)].scheduled_tick_candidates(), &[(4097, 8 << 4)]);
        let mut blocks = vec![0; 65536];
        blocks[4096] = 48;
        blocks[4097] = 13 << 4;
        apply_chunk(
            &mut world,
            ChunkData {
                x: -1,
                z: 2,
                ground_up: false,
                mask: 2,
                blocks,
                block_light: vec![0xa9; 2048],
                sky_light: Some(vec![0xfe; 2048]),
                biomes: None,
            },
        );
        assert_eq!(world.chunks[&(-1, 2)].blocks[0], 16);
        assert_eq!(world.chunks[&(-1, 2)].blocks[4096], 48);
        assert_eq!(world.chunks[&(-1, 2)].random_tick_mask(), 0);
        assert_eq!(world.chunks[&(-1, 2)].scheduled_tick_candidates(), &[(4097, 13 << 4)]);
        assert_eq!(
            world.light_uv(IVec3::new(-16, 0, 32), 0),
            [2 * 16, 5 * 16, 0, 0]
        );
        assert_eq!(
            world.light_uv(IVec3::new(-15, 0, 32), 0),
            [4 * 16, 7 * 16, 0, 0]
        );
        assert_eq!(
            world.light_uv(IVec3::new(-16, 16, 32), 0),
            [9 * 16, 14 * 16, 0, 0]
        );
        assert_eq!(
            world.light_uv(IVec3::new(-15, 16, 32), 0),
            [10 * 16, 15 * 16, 0, 0]
        );
        assert_eq!(world.chunks[&(-1, 2)].biomes, [21; 256]);
        assert!(world.edits.is_empty());
        apply_chunk(
            &mut world,
            ChunkData {
                x: -1,
                z: 2,
                ground_up: true,
                mask: 0,
                blocks: vec![0; 65536],
                block_light: vec![],
                sky_light: None,
                biomes: None,
            },
        );
        assert!(world.chunks.is_empty());
    }
    #[test]
    fn correction_maps_coordinates_and_resets_predicted_motion() {
        let mut player = Player::new(Vec3::ZERO);
        player.velocity = Vec3::ONE;
        apply_correction(
            &mut player,
            Pose {
                x: 12.5,
                y: 70.,
                z: -40.,
                yaw: 0.,
                pitch: 30.,
                on_ground: false,
            },
            0,
        );
        assert_eq!(player.position, Vec3::new(12.5, 70., -40.));
        assert_eq!(player.previous, player.position);
        assert_eq!(player.velocity, Vec3::ZERO);
        assert!((player.direction().z - 30f32.to_radians().cos()).abs() < 0.0001);
        assert!((player.direction().y + 0.5).abs() < 0.0001);
        let exact = Pose {
            x: 29_999_999.12345678,
            y: 70.00000000017,
            z: -8_388_609.437_5,
            yaw: 89.12345,
            pitch: -38.375,
            on_ground: true,
        };
        apply_correction(&mut player, exact,0);
        let sent = movement_pose(&player);
        assert_eq!(
            [sent.x.to_bits(), sent.y.to_bits(), sent.z.to_bits()],
            [exact.x.to_bits(), exact.y.to_bits(), exact.z.to_bits()]
        );
        assert_eq!(
            [sent.yaw.to_bits(), sent.pitch.to_bits()],
            [exact.yaw.to_bits(), exact.pitch.to_bits()]
        );
        assert_eq!(player.precise_velocity(), [0.0; 3]);
    }
    #[test]
    fn raw_inventory_prediction_retains_nbt_and_rejects_different_tags() {
        let tagged = Slot {
            id: 1,
            count: 5,
            damage: 0,
            nbt: vec![10, 0, 0, 0],
        };
        let mut cursor = None;
        let mut slot = Some(tagged.clone());
        predict_click(&mut cursor, &mut slot, 0, true);
        assert_eq!(cursor.as_ref().unwrap().count, 3);
        assert_eq!(slot.as_ref().unwrap().count, 2);
        assert_eq!(cursor.as_ref().unwrap().nbt, tagged.nbt);
        predict_click(&mut cursor, &mut slot, 0, true);
        assert_eq!(cursor.as_ref().unwrap().count, 2);
        assert_eq!(slot.as_ref().unwrap().count, 3);
        slot.as_mut().unwrap().nbt = vec![0];
        predict_click(&mut cursor, &mut slot, 0, false);
        assert_eq!(cursor.as_ref().unwrap().count, 3);
        assert_eq!(cursor.as_ref().unwrap().nbt, vec![0]);
        assert_eq!(slot.as_ref().unwrap().nbt, tagged.nbt);
    }
    #[test]
    fn armor_click_never_places_invalid_items_or_stacks() {
        let mut cursor = Some(Slot {
            id: 1,
            count: 64,
            damage: 0,
            nbt: vec![0],
        });
        let mut slot = None;
        predict_click(&mut cursor, &mut slot, 39, false);
        assert!(slot.is_none());
        assert_eq!(cursor.as_ref().unwrap().count, 64);
        cursor = Some(Slot {
            id: 310,
            count: 1,
            damage: 72,
            nbt: vec![0],
        });
        predict_click(&mut cursor, &mut slot, 39, false);
        assert!(cursor.is_none());
        assert_eq!(slot.as_ref().unwrap().damage, 72);
    }
    #[test]
    fn join_and_teleport_preserve_main_thread_transaction_sandwich_and_ground_state() {
        let (client, sent, incoming) = Client::test_transport();
        let mut remote = Remote::with_client(client);
        let mut player = Player::new(Vec3::new(10.0, 64.0, 10.0));
        player.set_pose_f64([10.0, 64.0, 10.0], 350.0, 15.0, true);
        player.set_velocity_f64([0.2, -0.08, 0.3]);
        let mut sim = Simulation::default();
        let mut world = World {
            chunks: [((0, 0), Arc::new(Chunk::empty()))].into(),
            spawn: player.position,
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: true,
            light_revision: 0,
        };
        let mut dirty = HashSet::new();
        incoming
            .send(Event::JoinGame {
                entity_id: 42,
                gamemode: 0,
                dimension: 0,
                difficulty: 2,
                reduced_debug: false,
                flat_world: false,
            })
            .unwrap();
        remote
            .poll_world(&mut world, &mut player, &mut sim, &mut dirty, None)
            .unwrap();
        assert!(matches!(sent.try_recv().unwrap(), Command::JoinAck));
        assert!(sent.try_recv().is_err());

        player.set_pose_f64([10.0, 64.0, 10.0], 350.0, 15.0, true);
        player.set_velocity_f64([0.2, -0.08, 0.3]);
        incoming
            .send(Event::Transaction {
                window_id: 0,
                action: -101,
                accepted: false,
            })
            .unwrap();
        incoming
            .send(Event::Position {
                pose: Pose {
                    x: 2.0,
                    y: 65.0,
                    z: -1.0,
                    yaw: 25.0,
                    pitch: -5.0,
                    on_ground: false,
                },
                correction_id: 7,
                relative: 0b01001,
            })
            .unwrap();
        incoming
            .send(Event::Transaction {
                window_id: 0,
                action: -102,
                accepted: false,
            })
            .unwrap();
        remote
            .poll_world(&mut world, &mut player, &mut sim, &mut dirty, None)
            .unwrap();
        assert!(matches!(
            sent.try_recv().unwrap(),
            Command::ConfirmTransaction {
                window_id: 0,
                action: -101
            }
        ));
        match sent.try_recv().unwrap() {
            Command::TeleportAck {
                pose,
                correction_id,
            } => {
                assert_eq!(correction_id, 7);
                assert_eq!([pose.x, pose.y, pose.z], [12.0, 65.0, -1.0]);
                assert_eq!([pose.yaw, pose.pitch], [15.0, -5.0]);
                assert!(!pose.on_ground);
            }
            other => panic!("Expected TeleportAck between S32 confirmations, got {other:?}"),
        }
        assert!(matches!(
            sent.try_recv().unwrap(),
            Command::ConfirmTransaction {
                window_id: 0,
                action: -102
            }
        ));
        assert!(sent.try_recv().is_err());
        assert!(player.grounded);
        assert_eq!(player.precise_velocity(), [0.2, 0.0, 0.0]);
    }
    #[test]
    fn pvp_attack_slowdown_applies_0_6_and_clears_sprint_only_on_players_and_vanilla_objects() {
        let (client, _sent, _incoming) = Client::test_transport();
        let mut remote = Remote::with_client(client);
        let mut player = Player::new(Vec3::new(8.0, 64.0, 8.0));
        let sim = Simulation::default();
        let pose = movement_pose(&player);
        remote.spawn_entity(2, EntityKind::Player([1; 16]), pose);
        remote.spawn_entity(3, EntityKind::Mob(MobKind::Zombie), pose);
        remote.entities.get_mut(&3).unwrap().mob_id = Some(54);
        player.sprinting = true;
        player.set_velocity_f64([0.25, -0.0784, 0.25]);
        remote.apply_attack_slowdown(3, &mut player, &sim);
        assert!(player.sprinting);
        assert_eq!(player.precise_velocity(), [0.25, -0.0784, 0.25]);
        remote.apply_attack_slowdown(2, &mut player, &sim);
        assert!(!player.sprinting);
        assert!((player.precise_velocity()[0] - 0.15).abs() < 1e-12);
        assert_eq!(player.precise_velocity()[1], -0.0784);
        assert!((player.precise_velocity()[2] - 0.15).abs() < 1e-12);
    }
    #[test]
    fn golden_apple_noslow_and_blocked_itemblock_c08_sequence() {
        use crate::vanilla::{entity_pick::Hit, ray_trace};
        let (client, sent, _incoming) = Client::test_transport();
        let mut remote = Remote::with_client(client);
        remote.ready = true;
        remote.game_mode = 0;
        let res = crate::validation::resources();
        let mut sim = Simulation::default();
        sim.food = 20;
        sim.health = 20.0;
        let gapple = Stack::new(322, 1);
        assert!(remote.can_start_using_item(&gapple, &sim, res));
        remote.disable_damage = true;
        assert!(!remote.can_start_using_item(&gapple, &sim, res));
        remote.disable_damage = false;

        let mut player = Player::new(Vec3::new(8.5, 64.0, 8.5));
        player.set_pose_f64([8.5, 64.0, 8.5], 0.0, 90.0, true);
        let mut world = World {
            chunks: [((0, 0), Arc::new(Chunk::empty()))].into(),
            spawn: player.position,
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: true,
            light_revision: 0,
        };
        let base = IVec3::new(8, 63, 8);
        world.set(base, 1 << 4);
        let hit = Hit::Block(ray_trace::Hit {
            kind: ray_trace::Kind::Block,
            position: base,
            face: 1,
            point: DVec3::new(8.5, 64.0, 8.5),
        });
        let mut out = gameplay::Outcome::default();

        // 1. Empty hand on non-interactive stone: sends block C08 only, NO Swing!
        remote
            .right_click_mouse(&mut world, &player, &mut sim, res, Some(hit), &mut out)
            .unwrap();
        assert!(matches!(
            sent.try_recv().unwrap(),
            Command::Place {
                position: [8, 63, 8],
                face: 1,
                held: None,
                ..
            }
        ));
        assert!(sent.try_recv().is_err());

        // 2. Golden apple on stone at full hunger: block C08 -> air C08(-1,-1,-1,255), using_item = true!
        sim.inventory.slots[0] = Some(gapple.clone());
        remote.slots[36] = Some(to_slot(&gapple).unwrap());
        remote
            .right_click_mouse(&mut world, &player, &mut sim, res, Some(hit), &mut out)
            .unwrap();
        assert!(matches!(
            sent.try_recv().unwrap(),
            Command::Place {
                position: [8, 63, 8],
                face: 1,
                ..
            }
        ));
        assert!(matches!(
            sent.try_recv().unwrap(),
            Command::Place {
                position: [-1, -1, -1],
                face: 255,
                ..
            }
        ));
        assert!(sent.try_recv().is_err());
        assert!(remote.using_item);
        remote.using_item = false;

        // 3. ItemBlock (Stone) targeting (8,64,8) inside player's own bounding box:
        // World.canBlockBePlaced returns false BEFORE block C08, so ONLY air C08(-1,-1,-1,255) is sent!
        let stone = Stack::new(1, 64);
        sim.inventory.slots[0] = Some(stone.clone());
        remote.slots[36] = Some(to_slot(&stone).unwrap());
        remote
            .right_click_mouse(&mut world, &player, &mut sim, res, Some(hit), &mut out)
            .unwrap();
        assert!(matches!(
            sent.try_recv().unwrap(),
            Command::Place {
                position: [-1, -1, -1],
                face: 255,
                ..
            }
        ));
        assert!(sent.try_recv().is_err());
        assert_eq!(world.get(IVec3::new(8, 64, 8)), 0);
    }

    #[test]
    fn fireball_acceleration_physics_destroy_and_fire_charge_block_click() {
        use crate::vanilla::{entity_pick::Hit, ray_trace};
        let (client, sent, incoming) = Client::test_transport();
        let mut remote = Remote::with_client(client);
        remote.ready = true;
        remote.game_mode = 0;
        let res = crate::validation::resources();
        let mut sim = Simulation::default();
        let mut player = Player::new(Vec3::new(8.5, 64.0, 8.5));
        player.set_pose_f64([8.5, 64.0, 8.5], 0.0, 90.0, true);
        let mut world = World {
            chunks: [
                ((0, 0), Arc::new(Chunk::empty())),
                ((1, 0), Arc::new(Chunk::empty())),
            ]
            .into(),
            spawn: player.position,
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: true,
            light_revision: 0,
        };
        let base = IVec3::new(8, 63, 8);
        world.set(base, 1 << 4);
        // Place a wall right along the fireball's flight path to verify client physics never halts early.
        world.set(IVec3::new(9, 65, 8), 1 << 4);

        let fire_charge = Stack::new(385, 3);
        sim.inventory.slots[0] = Some(fire_charge.clone());
        remote.slots[36] = Some(to_slot(&fire_charge).unwrap());
        let hit = Hit::Block(ray_trace::Hit {
            kind: ray_trace::Kind::Block,
            position: base,
            face: 1,
            point: DVec3::new(8.5, 64.0, 8.5),
        });
        let mut out = gameplay::Outcome::default();
        remote
            .right_click_mouse(&mut world, &player, &mut sim, res, Some(hit), &mut out)
            .unwrap();
        assert!(matches!(
            sent.try_recv().unwrap(),
            Command::Place {
                position: [8, 63, 8],
                face: 1,
                ..
            }
        ));
        assert!(matches!(sent.try_recv().unwrap(), Command::Swing));
        assert!(
            sent.try_recv().is_err(),
            "fire_charge block click must not send a duplicate air C08"
        );

        let mut dirty = HashSet::new();
        incoming
            .send(Event::SpawnObject {
                entity_id: 777,
                kind: 63,
                pose: Pose {
                    x: 8.5,
                    y: 65.2,
                    z: 8.5,
                    yaw: 0.0,
                    pitch: 0.0,
                    on_ground: false,
                },
                data: 1,
                velocity: Some([0.3, 0.0, 0.4]),
            })
            .unwrap();
        remote
            .poll_world(&mut world, &mut player, &mut sim, &mut dirty, None)
            .unwrap();
        let e = &remote.entities[&777];
        assert!((e.fireball_accel - DVec3::new(0.06, 0.0, 0.08)).length() < 1e-9);
        assert_eq!(e.object_velocity, DVec3::ZERO);

        for _ in 0..10 {
            remote
                .tick(&mut world, &mut player, &mut sim, res, false, false)
                .unwrap();
            remote.sync_scene(&mut sim);
        }
        let proj = sim
            .server_scene
            .projectiles
            .iter()
            .find(|p| p.id == 777)
            .expect("spawned fireball must be synced to server_scene");
        assert!(
            (proj.position - Vec3::new(8.5, 65.2, 8.5)).length() > 2.5,
            "fireball must accelerate continuously instead of decaying or stopping on client wall: {:?}",
            proj.position
        );
        assert!(
            (proj.position - proj.previous).length() > 0.3,
            "fireball must preserve previous tick position for smooth interpolation"
        );

        incoming.send(Event::DestroyEntities(vec![777])).unwrap();
        remote
            .poll_world(&mut world, &mut player, &mut sim, &mut dirty, None)
            .unwrap();
        assert!(
            sim.server_scene.projectiles.iter().all(|p| p.id != 777),
            "DestroyEntities must immediately remove lingering fireball from server_scene"
        );
    }

    #[test]
    fn mp_horse_mount_interpolation_milk_spawn_egg_and_mob_sync() {
        use crate::vanilla::{entity_pick::Hit, movement::attribute::Snapshot, ray_trace};
        let (client, sent, incoming) = Client::test_transport();
        let mut remote = Remote::with_client(client);
        remote.ready = true;
        remote.entity_id = 1;
        remote.game_mode = 0;
        let res = crate::validation::resources();
        let mut sim = Simulation::default();
        let mut player = Player::new(Vec3::new(8.5, 64.0, 8.5));
        player.set_pose_f64([8.5, 64.0, 8.5], 0.0, 90.0, true);
        let mut world = World {
            chunks: [((0, 0), Arc::new(Chunk::empty()))].into(),
            spawn: player.position,
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: true,
            light_revision: 0,
        };
        let base = IVec3::new(8, 63, 8);
        world.set(base, 1 << 4);
        let mut dirty = HashSet::new();

        // 1. SpawnEgg (383) block right-click swings arm and does not send duplicate air C08.
        let egg = Stack::new(383, 1);
        sim.inventory.slots[0] = Some(egg.clone());
        remote.slots[36] = Some(to_slot(&egg).unwrap());
        let hit = Hit::Block(ray_trace::Hit {
            kind: ray_trace::Kind::Block,
            position: base,
            face: 1,
            point: DVec3::new(8.5, 64.0, 8.5),
        });
        let mut out = gameplay::Outcome::default();
        remote
            .right_click_mouse(&mut world, &player, &mut sim, res, Some(hit), &mut out)
            .unwrap();
        assert!(matches!(sent.try_recv().unwrap(), Command::Place { position: [8, 63, 8], face: 1, .. }));
        assert!(matches!(sent.try_recv().unwrap(), Command::Swing));
        assert!(sent.try_recv().is_err());

        // 2. Local player DW 17 absorption + Milk Bucket (335) finish on status 9.
        sim.effects.add_client(crate::vanilla::item_food::Effect {
            id: 1,
            duration: 200,
            amplifier: 0,
            ambient: false,
            show_particles: true,
        }).unwrap();
        let milk = Stack::new(335, 1);
        sim.inventory.slots[0] = Some(milk.clone());
        remote.slots[36] = Some(to_slot(&milk).unwrap());
        incoming
            .send(Event::EntityMetadata {
                entity_id: 1,
                metadata: vec![(17, Metadata::Float(4.0))],
            })
            .unwrap();
        incoming
            .send(Event::EntityStatus {
                entity_id: 1,
                status: 9,
            })
            .unwrap();
        remote
            .poll_world(&mut world, &mut player, &mut sim, &mut dirty, None)
            .unwrap();
        assert_eq!(sim.absorption, 0.0);
        assert!(sim.effects.get(1).is_none());
        assert_eq!(sim.inventory.slots[0].as_ref().map(|s| s.id), Some(325));

        // 3. Unsaddled tamed horse: riding_any_horse is true while riding_horse is false;
        // mount_health reads 0x20 maxHealth and DW 6 HP; camera interpolates across mounted ticks.
        incoming
            .send(Event::SpawnMob {
                entity_id: 500,
                kind: 100,
                pose: Pose { x: 8.5, y: 64.0, z: 8.5, yaw: 0.0, pitch: 0.0, on_ground: true },
                head_yaw: 15.0,
                velocity: [0.0; 3],
                metadata: vec![
                    (6, Metadata::Float(22.0)),
                    (16, Metadata::Int(2)),
                    (22, Metadata::Int(3)),
                ],
            })
            .unwrap();
        incoming
            .send(Event::Properties {
                entity_id: 500,
                properties: vec![Snapshot {
                    name: "generic.maxHealth".into(),
                    base: 26.0,
                    modifiers: vec![],
                }],
            })
            .unwrap();
        incoming
            .send(Event::Attach {
                entity_id: 1,
                vehicle: 500,
                leash: 0,
            })
            .unwrap();
        remote
            .poll_world(&mut world, &mut player, &mut sim, &mut dirty, None)
            .unwrap();
        assert!(remote.riding_any_horse());
        assert!(!remote.riding_horse());
        assert_eq!(remote.mount_health(), Some((22.0, 26.0)));

        // First mounted tick snaps previous == position; next tick preserves previous != position.
        remote.tick(&mut world, &mut player, &mut sim, res, false, false).unwrap();
        assert_eq!(player.previous, player.position);
        incoming
            .send(Event::EntityTeleport {
                entity_id: 500,
                pose: Pose { x: 11.5, y: 64.0, z: 8.5, yaw: 0.0, pitch: 0.0, on_ground: true },
            })
            .unwrap();
        remote
            .poll_world(&mut world, &mut player, &mut sim, &mut dirty, None)
            .unwrap();
        remote.tick(&mut world, &mut player, &mut sim, res, false, false).unwrap();
        assert!((player.position.x - player.previous.x).abs() > 0.5);
        while sent.try_recv().is_ok() {}

        // 4. Wolf collar conversion + deferred 20-tick death removal on DestroyEntities.
        incoming
            .send(Event::SpawnMob {
                entity_id: 600,
                kind: 95,
                pose: Pose { x: 9.0, y: 64.0, z: 9.0, yaw: 0.0, pitch: 0.0, on_ground: true },
                head_yaw: 0.0,
                velocity: [0.0; 3],
                metadata: vec![
                    (16, Metadata::Byte(4)),
                    (20, Metadata::Byte(11)), // dyeDamage 11 = yellow -> wool 15 - 11 = 4
                ],
            })
            .unwrap();
        remote
            .poll_world(&mut world, &mut player, &mut sim, &mut dirty, None)
            .unwrap();
        remote.sync_scene(&mut sim);
        assert_eq!(sim.server_scene.mobs[&600].collar_color, 4);

        incoming.send(Event::EntityStatus { entity_id: 600, status: 3 }).unwrap();
        incoming.send(Event::DestroyEntities(vec![600])).unwrap();
        remote
            .poll_world(&mut world, &mut player, &mut sim, &mut dirty, None)
            .unwrap();
        remote.tick(&mut world, &mut player, &mut sim, res, false, false).unwrap();
        assert!(sim.server_scene.mobs.contains_key(&600), "dead mob stays visible during 20-tick death roll");
        for _ in 0..19 {
            remote.tick(&mut world, &mut player, &mut sim, res, false, false).unwrap();
        }
        assert!(!sim.server_scene.mobs.contains_key(&600), "dead mob removed once death_time reaches 20");
    }

    #[test]
    fn mp_local_player_hurt_death_3d_sounds_and_out_of_order_attach() {
        let (client, _sent, incoming) = Client::test_transport();
        let mut remote = Remote::with_client(client);
        remote.ready = true;
        remote.entity_id = 1;
        remote.game_mode = 0;
        let mut sim = Simulation::default();
        let mut player = Player::new(Vec3::new(12.5, 64.0, -7.25));
        player.set_pose_f64([12.5, 64.0, -7.25], 0.0, 0.0, true);
        let mut world = World {
            chunks: [
                ((0, -1), Arc::new(Chunk::empty())),
                ((0, 0), Arc::new(Chunk::empty())),
            ]
            .into(),
            spawn: player.position,
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: true,
            light_revision: 0,
        };
        let mut dirty = HashSet::new();
        let expected_pos = DVec3::new(12.5, 64.0, -7.25);

        // 1. S19 EntityStatus status=2 on self.entity_id emits 3D "game.player.hurt".
        incoming
            .send(Event::EntityStatus {
                entity_id: 1,
                status: 2,
            })
            .unwrap();
        remote
            .poll_world(&mut world, &mut player, &mut sim, &mut dirty, None)
            .unwrap();
        assert_eq!(sim.hurt_camera.hurt_time, 10);
        assert_eq!(remote.visuals.sounds.len(), 1);
        let (ref name, vol, pitch, pos) = remote.visuals.sounds[0];
        assert_eq!(name, "game.player.hurt");
        assert_eq!(vol, 1.0);
        assert!((0.8..=1.2).contains(&pitch), "unexpected hurt pitch {pitch}");
        assert_eq!(pos, expected_pos);
        remote.visuals.sounds.clear();

        // 2. S19 EntityStatus status=3 on self.entity_id emits 3D "game.player.die" and zeroes health.
        incoming
            .send(Event::EntityStatus {
                entity_id: 1,
                status: 3,
            })
            .unwrap();
        remote
            .poll_world(&mut world, &mut player, &mut sim, &mut dirty, None)
            .unwrap();
        assert_eq!(sim.health, 0.0);
        assert_eq!(remote.visuals.sounds.len(), 1);
        let (ref name, vol, pitch, pos) = remote.visuals.sounds[0];
        assert_eq!(name, "game.player.die");
        assert_eq!(vol, 1.0);
        assert!((0.8..=1.2).contains(&pitch), "unexpected death pitch {pitch}");
        assert_eq!(pos, expected_pos);
        remote.visuals.sounds.clear();

        // 3. Out-of-order S1B AttachEntity before S0E/S0F preserves remote.riding,
        // Pig dismount lifts feet by entity_height, and DestroyEntities clears remote.riding.
        incoming
            .send(Event::Attach {
                entity_id: 1,
                vehicle: 900,
                leash: 0,
            })
            .unwrap();
        remote
            .poll_world(&mut world, &mut player, &mut sim, &mut dirty, None)
            .unwrap();
        assert_eq!(remote.riding, Some(900));

        incoming
            .send(Event::SpawnObject {
                entity_id: 900,
                kind: 10,
                pose: Pose {
                    x: 12.5,
                    y: 64.0,
                    z: -7.25,
                    yaw: 0.0,
                    pitch: 0.0,
                    on_ground: true,
                },
                data: 0,
                velocity: Some([0.0; 3]),
            })
            .unwrap();
        remote
            .poll_world(&mut world, &mut player, &mut sim, &mut dirty, None)
            .unwrap();
        assert_eq!(remote.riding, Some(900));
        assert!(remote.entities.contains_key(&900));

        incoming.send(Event::DestroyEntities(vec![900])).unwrap();
        remote
            .poll_world(&mut world, &mut player, &mut sim, &mut dirty, None)
            .unwrap();
        assert_eq!(remote.riding, None);

        // Saddled Pig dismount lifts player feet by entity_height (0.9) instead of 0.0.
        incoming
            .send(Event::SpawnMob {
                entity_id: 901,
                kind: 90,
                pose: Pose {
                    x: 14.5,
                    y: 65.0,
                    z: -5.5,
                    yaw: 0.0,
                    pitch: 0.0,
                    on_ground: true,
                },
                head_yaw: 0.0,
                velocity: [0.0; 3],
                metadata: vec![(16, Metadata::Byte(1))],
            })
            .unwrap();
        incoming
            .send(Event::Attach {
                entity_id: 1,
                vehicle: 901,
                leash: 0,
            })
            .unwrap();
        remote
            .poll_world(&mut world, &mut player, &mut sim, &mut dirty, None)
            .unwrap();
        assert_eq!(remote.riding, Some(901));
        incoming
            .send(Event::Attach {
                entity_id: 1,
                vehicle: -1,
                leash: 0,
            })
            .unwrap();
        remote
            .poll_world(&mut world, &mut player, &mut sim, &mut dirty, None)
            .unwrap();
        assert_eq!(remote.riding, None);
        assert!(
            (player.precise_position()[1] - (65.0 + f64::from(0.9_f32))).abs() < 1e-6,
            "dismounted Pig height should lift feet by 0.9, got {:?}",
            player.precise_position()
        );

        // 4. Local MP footstep & fall landing sounds emit into remote.visuals.sounds at 3D feet.
        world.set(IVec3::new(12, 63, -8), 1 << 4);
        player.set_pose_f64([12.5, 64.0, -7.25], 0.0, 0.0, true);
        player.allow_flying = false;
        player.flying = false;
        remote.emit_local_movement_sounds(
            &world,
            &player,
            DVec3::new(10.5, 64.0, -7.25),
            true,
            expected_pos,
        );
        assert!(
            remote
                .visuals
                .sounds
                .iter()
                .any(|(s, _, _, p)| s.contains("stone") && *p == expected_pos),
            "expected stone footstep sound at feet, got {:?}",
            remote.visuals.sounds
        );
        remote.visuals.sounds.clear();
        remote.fall_distance = 8.5;
        remote.emit_local_movement_sounds(&world, &player, expected_pos, false, expected_pos);
        assert!(
            remote
                .visuals
                .sounds
                .iter()
                .any(|(s, v, _, p)| s == "game.player.hurt.fall.big" && *v == 1.0 && *p == expected_pos),
            "expected big fall landing sound at feet, got {:?}",
            remote.visuals.sounds
        );
    }
}
#[cfg(test)]
#[path="remote_plugin_probe.rs"]
mod plugin_probe;
