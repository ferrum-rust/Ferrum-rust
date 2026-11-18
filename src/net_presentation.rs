use crate::{network::PlayerInfo, protocol::Reader};
use anyhow::{ensure, Result};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone)]
pub struct Team {
    pub label: String,
    pub prefix: String,
    pub suffix: String,
    // Kept in the scoreboard even when no 3D nameplate is being drawn.
    #[allow(dead_code)]
    pub flags: u8,
    #[allow(dead_code)]
    pub visibility: String,
    pub color: i8,
}
#[derive(Debug)]
pub enum Packet {
    Sound {
        name: String,
        pos: [f64; 3],
        volume: f32,
        pitch: f32,
    },
    Effect {
        id: i32,
        pos: [i32; 3],
        data: i32,
        global: bool,
    },
    Particles {
        id: u8,
        far: bool,
        pos: [f32; 3],
        offset: [f32; 3],
        speed: f32,
        count: i32,
        args: [i32; 2],
    },
    Objective {
        name: String,
        action: u8,
        data: Option<(String, String)>,
    },
    Score {
        player: String,
        objective: String,
        value: Option<i32>,
    },
    Display {
        slot: u8,
        objective: String,
    },
    Team {
        name: String,
        action: u8,
        data: Option<Team>,
        players: Vec<String>,
    },
    Title {
        kind: u8,
        text: String,
        times: [i32; 3],
    },
    HeaderFooter(String, String),
}
pub fn decode(id: i32, r: &mut Reader<'_>) -> Result<Packet> {
    let packet = match id {
        0x28 => Packet::Effect {
            id: r.i32()?,
            pos: r.position()?,
            data: r.i32()?,
            global: r.bool()?,
        },
        0x29 => {
            let name = r.string(256)?;
            let pos = [r.i32()?, r.i32()?, r.i32()?].map(|v| v as f64 / 8.);
            let volume = r.f32()?;
            ensure!(volume.is_finite(), "Invalid sound volume");
            Packet::Sound {
                name,
                pos,
                volume,
                pitch: r.u8()? as f32 / 63.,
            }
        }
        0x2a => {
            let number = r.i32()?;
            let id = if (0..42).contains(&number) {
                number as u8
            } else {
                35
            };
            let far = r.bool()? || matches!(id, 0 | 1 | 2 | 41);
            let pos = [r.f32()?, r.f32()?, r.f32()?];
            let offset = [r.f32()?, r.f32()?, r.f32()?];
            let speed = r.f32()?;
            let count = r.i32()?;
            let mut args = [0; 2];
            ensure!(
                pos.into_iter()
                    .chain(offset)
                    .chain([speed])
                    .all(f32::is_finite),
                "Invalid particle coordinates"
            );
            for a in args.iter_mut().take(match id {
                36 => 2,
                37 | 38 => 1,
                _ => 0,
            }) {
                *a = r.varint()?;
            }
            Packet::Particles {
                id,
                far,
                pos,
                offset,
                speed,
                count,
                args,
            }
        }
        0x3b => {
            let name = r.string(16)?;
            let action = r.u8()?;
            ensure!(action <= 2, "Invalid objective action");
            let data = if action == 1 {
                None
            } else {
                Some((r.string(32)?, r.string(16)?))
            };
            Packet::Objective { name, action, data }
        }
        0x3c => {
            let player = r.string(40)?;
            let action = r.count(1)?;
            let objective = r.string(16)?;
            let value = if action == 0 { Some(r.varint()?) } else { None };
            Packet::Score {
                player,
                objective,
                value,
            }
        }
        0x3d => {
            let slot = r.u8()?;
            ensure!(slot < 19, "Invalid display slot");
            Packet::Display {
                slot,
                objective: r.string(16)?,
            }
        }
        0x3e => {
            let name = r.string(16)?;
            let action = r.u8()?;
            ensure!(action <= 4, "Invalid team action");
            let data = if matches!(action, 0 | 2) {
                Some(Team {
                    label: r.string(32)?,
                    prefix: r.string(16)?,
                    suffix: r.string(16)?,
                    flags: r.u8()?,
                    visibility: r.string(32)?,
                    color: r.i8()?,
                })
            } else {
                None
            };
            let players = if matches!(action, 0 | 3 | 4) {
                let n = r.count(r.remaining().len())?;
                (0..n).map(|_| r.string(40)).collect::<Result<_>>()?
            } else {
                vec![]
            };
            Packet::Team {
                name,
                action,
                data,
                players,
            }
        }
        0x45 => {
            let kind = r.count(4)? as u8;
            let text = if kind <= 1 {
                r.string(32767)?
            } else {
                String::new()
            };
            let times = if kind == 2 {
                [r.i32()?, r.i32()?, r.i32()?]
            } else {
                [-1; 3]
            };
            Packet::Title { kind, text, times }
        }
        0x47 => Packet::HeaderFooter(r.string(32767)?, r.string(32767)?),
        _ => anyhow::bail!("Not a presentation packet: {id:#x}"),
    };
    r.finish()?;
    Ok(packet)
}
#[derive(Debug, Clone, Default)]
pub struct Objective {
    pub title: String,
    pub render_type: String,
    pub scores: BTreeMap<String, i32>,
}
#[derive(Debug, Clone, Default)]
pub struct PlayerEntry {
    pub name: String,
    pub display: Option<String>,
    pub mode: i32,
    pub latency: i32,
}
#[derive(Debug, Clone)]
pub struct State {
    pub objectives: HashMap<String, Objective>,
    pub display: [String; 19],
    pub teams: HashMap<String, Team>,
    pub memberships: HashMap<String, String>,
    pub header: String,
    pub footer: String,
    pub players: HashMap<[u8; 16], PlayerEntry>,
    pub skins: HashMap<[u8; 16], crate::player_skins::SkinProfile>,
    pub local_uuid: Option<[u8; 16]>,
    pub mojang_uuid: Option<[u8; 16]>,
    pub title: String,
    pub subtitle: String,
    pub times: [i32; 3],
    pub timer: i32,
}
impl Default for State {
    fn default() -> Self {
        Self {
            objectives: Default::default(),
            display: Default::default(),
            teams: Default::default(),
            memberships: Default::default(),
            header: String::new(),
            footer: String::new(),
            players: Default::default(),
            skins: Default::default(),
            local_uuid: None,
            mojang_uuid: None,
            title: String::new(),
            subtitle: String::new(),
            times: [10, 70, 20],
            timer: 0,
        }
    }
}
impl State {
    fn fallback_mojang_profile(&self, uuid: [u8; 16]) -> Option<crate::player_skins::SkinProfile> {
        if (uuid[6] >> 4) == 4 {
            if let Some(p) = crate::player_skins::poll_mojang_profile(uuid) {
                return Some(p);
            }
        }
        if self.local_uuid == Some(uuid) {
            if let Some(m) = self.mojang_uuid {
                return crate::player_skins::poll_mojang_profile(m);
            }
        }
        None
    }
    pub fn slim_for(&self, uuid: [u8; 16]) -> bool {
        if let Some(p) = self.skins.get(&uuid) {
            if p.url.is_some() {
                return p.slim;
            }
        }
        if let Some(p) = self.fallback_mojang_profile(uuid) {
            return p.slim;
        }
        self.skins
            .get(&uuid)
            .map_or_else(|| crate::player_skins::default_slim(uuid), |p| p.slim)
    }
    pub fn skin_region(
        &self,
        r: &crate::resources::Resources,
        uuid: [u8; 16],
    ) -> crate::player_skins::SkinRegion {
        if let Some(p) = self.skins.get(&uuid) {
            if p.url.is_some() {
                return r.skins.resolve(r, p.url.as_ref(), p.slim);
            }
        }
        if let Some(p) = self.fallback_mojang_profile(uuid) {
            return r.skins.resolve(r, p.url.as_ref(), p.slim);
        }
        let slim = self.slim_for(uuid);
        r.skins.resolve(r, None, slim)
    }
    pub fn local_slim(&self) -> Option<bool> {
        if let Some(uuid) = self.local_uuid {
            if let Some(p) = self.skins.get(&uuid) {
                if p.url.is_some() {
                    return Some(p.slim);
                }
            }
            if let Some(p) = self.fallback_mojang_profile(uuid) {
                return Some(p.slim);
            }
            return self.skins.get(&uuid).map(|p| p.slim);
        }
        self.mojang_uuid
            .and_then(crate::player_skins::poll_mojang_profile)
            .map(|p| p.slim)
    }
    pub fn local_skin_region(
        &self,
        r: &crate::resources::Resources,
        fallback_slim: bool,
    ) -> crate::player_skins::SkinRegion {
        if let Some(uuid) = self.local_uuid {
            return self.skin_region(r, uuid);
        }
        if let Some(p) = self
            .mojang_uuid
            .and_then(crate::player_skins::poll_mojang_profile)
        {
            return r.skins.resolve(r, p.url.as_ref(), p.slim);
        }
        r.skins.resolve(r, None, fallback_slim)
    }
    pub fn update_player(&mut self, action: u8, p: &PlayerInfo) {
        if action == 0 {
            if let Some(profile) = crate::player_skins::parse_properties(p.uuid, &p.properties) {
                self.skins.insert(p.uuid, profile);
            } else {
                if (p.uuid[6] >> 4) == 4 && self.skins.len() < 64 {
                    crate::player_skins::request_mojang_profile(p.uuid);
                }
                self.skins
                    .entry(p.uuid)
                    .or_insert_with(|| crate::player_skins::SkinProfile::default_for(p.uuid));
            }
            self.players.insert(
                p.uuid,
                PlayerEntry {
                    name: p.name.clone().unwrap_or_default(),
                    display: p.display_name.clone(),
                    mode: p.mode.unwrap_or_default(),
                    latency: p.latency.unwrap_or_default(),
                },
            );
        } else if action == 4 {
            self.players.remove(&p.uuid);
        } else if let Some(entry) = self.players.get_mut(&p.uuid) {
            match action {
                1 => entry.mode = p.mode.unwrap_or_default(),
                2 => entry.latency = p.latency.unwrap_or_default(),
                3 => entry.display = p.display_name.clone(),
                _ => {}
            }
        }
    }
    pub fn tick(&mut self) {
        if self.timer > 0 {
            self.timer -= 1;
            if self.timer == 0 {
                self.title.clear();
                self.subtitle.clear();
            }
        }
    }
    pub fn objective(&self, slot: usize) -> Option<&Objective> {
        self.objectives.get(&self.display[slot])
    }
    pub fn team(&self, name: &str) -> Option<&Team> {
        self.memberships
            .get(name)
            .and_then(|team| self.teams.get(team))
    }
    pub fn reset_world_scoreboard(&mut self) {
        self.objectives.clear();
        self.display = Default::default();
        self.teams.clear();
        self.memberships.clear();
        self.title.clear();
        self.subtitle.clear();
        self.times = [10, 70, 20];
        self.timer = 0;
    }
    pub fn formatted_name(&self, name: &str) -> String {
        self.team(name).map_or_else(
            || name.to_owned(),
            |t| {
                if (!t.prefix.contains('§') || t.prefix.ends_with("§r") || t.prefix.ends_with("§R"))
                    && (0..15).contains(&t.color)
                {
                    let c = b"0123456789abcdef"[t.color as usize] as char;
                    format!("{}§{c}{name}{}", t.prefix, t.suffix)
                } else {
                    format!("{}{name}{}", t.prefix, t.suffix)
                }
            },
        )
    }
    pub fn apply(&mut self, p: Packet) {
        match p {
            Packet::Objective {
                name, action: 1, ..
            } => {
                self.objectives.remove(&name);
                for slot in &mut self.display {
                    if *slot == name {
                        slot.clear()
                    }
                }
            }
            Packet::Objective {
                name,
                data: Some((title, render_type)),
                ..
            } => {
                let obj = self.objectives.entry(name).or_default();
                obj.title = title;
                obj.render_type = render_type;
            }
            Packet::Score {
                player,
                objective,
                value,
            } => {
                if objective.is_empty() && value.is_none() {
                    for obj in self.objectives.values_mut() {
                        obj.scores.remove(&player);
                    }
                } else if let Some(obj) = self.objectives.get_mut(&objective) {
                    if let Some(value) = value {
                        obj.scores.insert(player, value);
                    } else {
                        obj.scores.remove(&player);
                    }
                }
            }
            Packet::Display { slot, objective } => self.display[usize::from(slot)] = objective,
            Packet::Team {
                name, action: 1, ..
            } => {
                self.teams.remove(&name);
                self.memberships.retain(|_, team| *team != name);
            }
            Packet::Team {
                name,
                action,
                data,
                players,
            } => {
                if let Some(info) = data {
                    self.teams.insert(name.clone(), info);
                }
                if self.teams.contains_key(&name) {
                    for player in players {
                        if action == 4 {
                            if self.memberships.get(&player) == Some(&name) {
                                self.memberships.remove(&player);
                            }
                        } else {
                            self.memberships.insert(player, name.clone());
                        }
                    }
                }
            }
            Packet::Title { kind, text, times } => match kind {
                0 => {
                    self.title = text;
                    self.timer = self.times.into_iter().fold(0i32, i32::saturating_add);
                }
                1 => self.subtitle = text,
                2 => {
                    for (old, new) in self.times.iter_mut().zip(times) {
                        if new >= 0 {
                            *old = new
                        }
                    }
                    if self.timer > 0 {
                        self.timer = self.times.into_iter().fold(0i32, i32::saturating_add);
                    }
                }
                _ => {
                    self.title.clear();
                    self.subtitle.clear();
                    self.timer = 0;
                    if kind == 4 {
                        self.times = [10, 70, 20];
                    }
                }
            },
            Packet::HeaderFooter(h, f) => {
                self.header = h;
                self.footer = f
            }
            _ => {}
        }
    }
}
#[derive(Debug)]
pub struct WorldEvents {
    pub particle_setting: u8,
    pub sounds: Vec<(String, f32, f32, glam::DVec3)>,
    pub(crate) explosions: Vec<(glam::DVec3, u8)>,
    random: crate::vanilla::spawning::EntityRandom,
}
impl Default for WorldEvents {
    fn default() -> Self {
        Self {
            particle_setting: 0,
            sounds: vec![],
            explosions: Vec::new(),
            random: crate::vanilla::spawning::EntityRandom::new(
                crate::vanilla::random::JavaRandom::independent_seed(),
            ),
        }
    }
}
pub(crate) fn tick_explosion_clouds(explosions: &mut Vec<(glam::DVec3,u8)>, particles: &mut Vec<crate::gameplay::Particle>, rng: &mut crate::vanilla::random::JavaRandom) {
        explosions.retain_mut(|(at, age)| {
            for _ in 0..6 {
                let offset = glam::DVec3::from_array(std::array::from_fn(|_| {
                    (rng.next_double() - rng.next_double()) * 4.
                }));
                if particles.len() < 4096 {
                    if let Some(p) = crate::gameplay::Particle::network_sprite(
                        1,
                        *at + offset,
                        glam::DVec3::X * f64::from(*age) / 8.,
                        rng,
                    ) {
                        particles.push(p);
                    }
                }
            }
            *age += 1;
            *age < 8
        });
}
impl WorldEvents {
    pub fn tick(&mut self, sim: &mut crate::gameplay::Simulation) {
        sim.presentation.tick();
        tick_explosion_clouds(&mut self.explosions,&mut sim.particles,&mut self.random.random);
    }
    pub fn handle(
        &mut self,
        packet: Packet,
        player: &crate::player::Player,
        sim: &mut crate::gameplay::Simulation,
        world: &crate::world::World,
    ) {
        use glam::DVec3;
        let listener = DVec3::from_array(player.precise_position());
        match packet {
            Packet::Sound {
                name,
                pos,
                volume,
                pitch,
            } => self.sound(&name, DVec3::from_array(pos), listener, volume, pitch),
            Packet::Particles {
                id,
                far,
                pos,
                offset,
                speed,
                count,
                args,
            } => {
                let origin = glam::Vec3::from_array(pos).as_dvec3();
                let spread = glam::Vec3::from_array(offset).as_dvec3();
                let gaussian = |r: &mut crate::vanilla::spawning::EntityRandom| {
                    DVec3::new(r.next_gaussian(), r.next_gaussian(), r.next_gaussian())
                };
                for _ in 0..if count == 0 { 1 } else { count.clamp(0, 4096) } {
                    let (point, velocity) = if count == 0 {
                        (origin, spread * f64::from(speed))
                    } else {
                        (
                            origin + gaussian(&mut self.random) * spread,
                            gaussian(&mut self.random) * f64::from(speed),
                        )
                    };
                    let visible = match self.particle_setting {
                        0 => true,
                        1 => self.random.random.next_int(3) != 0,
                        _ => false,
                    };
                    if far
                        || matches!(id, 0..=2 | 41)
                        || (visible && point.distance_squared(listener) <= 256.)
                    {
                        self.particle(sim, world, id, point, velocity, args);
                    }
                }
            }
            Packet::Effect {
                id,
                pos,
                data,
                global,
            } => {
                let block = glam::IVec3::from_array(pos);
                let mut point = block.as_dvec3() + DVec3::splat(0.5);
                if global {
                    point = listener + (point - listener).normalize_or_zero() * 2.;
                }
                let random = &mut self.random.random;
                let (name, volume, pitch) = match id {
                    1000 => ("random.click", 1., 1.),
                    1001 => ("random.click", 1., 1.2),
                    1002 => ("random.bow", 1., 1.2),
                    1003 | 1006 | 1020..=1022 => {
                        let (name, volume) = match id {
                            1003 => ("random.door_open", 1.),
                            1006 => ("random.door_close", 1.),
                            1020 => ("random.anvil_break", 1.),
                            1021 => ("random.anvil_use", 1.),
                            _ => ("random.anvil_land", 0.3),
                        };
                        (name, volume, 0.9 + random.next_float() * 0.1)
                    }
                    1004 => (
                        "random.fizz",
                        0.5,
                        2.6 + (random.next_float() - random.next_float()) * 0.8,
                    ),
                    1005 => {
                        let records = [
                            "13", "cat", "blocks", "chirp", "far", "mall", "mellohi", "stal",
                            "strad", "ward", "11", "wait",
                        ];
                        if let Some(record) =
                            data.checked_sub(2256).and_then(|i| records.get(i as usize))
                        {
                            self.sound(&format!("records.{record}"), point, listener, 4., 1.);
                        }
                        return;
                    }
                    1013 if global => ("mob.wither.spawn", 1., 1.),
                    1018 if global => ("mob.enderdragon.end", 5., 1.),
                    1007..=1012 | 1014..=1017 => {
                        let (name, volume) = match id {
                            1007 => ("mob.ghast.charge", 10.),
                            1008 => ("mob.ghast.fireball", 10.),
                            1009 => ("mob.ghast.fireball", 2.),
                            1010 => ("mob.zombie.wood", 2.),
                            1011 => ("mob.zombie.metal", 2.),
                            1012 => ("mob.zombie.woodbreak", 2.),
                            1014 => ("mob.wither.shoot", 2.),
                            1015 => ("mob.bat.takeoff", 0.05),
                            1016 => ("mob.zombie.infect", 2.),
                            _ => ("mob.zombie.unfect", 2.),
                        };
                        (
                            name,
                            volume,
                            1. + (random.next_float() - random.next_float()) * 0.2,
                        )
                    }
                    2001 => {
                        let state = (((data & 4095) << 4) | ((data >> 12) & 15)) as u16;
                        if state >> 4 == 0 || state >> 4 >= 256 {
                            return;
                        }
                        let state = crate::gameplay::actual_dig_state(world, block, state);
                        sim.block_destroy_effects(block, state);
                        crate::vanilla::hand_placement::break_sound(state)
                    }
                    2000 => {
                        let dx = f64::from(data.rem_euclid(3) - 1);
                        let dz = f64::from((data.div_euclid(3)).rem_euclid(3) - 1);
                        let origin =
                            block.as_dvec3() + DVec3::new(dx * 0.6 + 0.5, 0.5, dz * 0.6 + 0.5);
                        for _ in 0..10 {
                            let speed = self.random.random.next_double() * 0.2 + 0.01;
                            let at = DVec3::new(
                                origin.x
                                    + dx * 0.01
                                    + (self.random.random.next_double() - 0.5) * dz * 0.5,
                                origin.y + (self.random.random.next_double() - 0.5) * 0.5,
                                origin.z
                                    + dz * 0.01
                                    + (self.random.random.next_double() - 0.5) * dx * 0.5,
                            );
                            let vel = DVec3::new(
                                dx * speed + self.random.next_gaussian() * 0.01,
                                -0.03 + self.random.next_gaussian() * 0.01,
                                dz * speed + self.random.next_gaussian() * 0.01,
                            );
                            if self.should_spawn_effect_particle() {
                                self.particle(sim, world, 11, at, vel, [0; 2]);
                            }
                        }
                        return;
                    }
                    2002 => {
                        let base = block.as_dvec3();
                        for _ in 0..8 {
                            let vel = DVec3::new(
                                self.random.next_gaussian() * 0.15,
                                self.random.random.next_double() * 0.2,
                                self.random.next_gaussian() * 0.15,
                            );
                            if self.should_spawn_effect_particle() {
                                self.particle(sim, world, 36, base, vel, [373, data]);
                            }
                        }
                        let rgb = crate::ui::creative::potion_color_for_meta(data as u16);
                        let is_instant = crate::ui::creative::potion_effects_for_meta(data as u16)
                            .iter()
                            .any(|e| crate::ui::creative::potion_is_instant(e.id));
                        for _ in 0..100 {
                            let spell_speed = self.random.random.next_double() * 4.0;
                            let angle = self.random.random.next_double() * std::f64::consts::TAU;
                            let vx = angle.cos() * spell_speed;
                            let vy = 0.01 + self.random.random.next_double() * 0.5;
                            let vz = angle.sin() * spell_speed;
                            let brightness = 0.75 + self.random.random.next_float() * 0.25;
                            if self.should_spawn_effect_particle() && sim.particles.len() < 4096 {
                                if let Some(p) = crate::gameplay::Particle::potion_splash_effect(
                                    is_instant,
                                    base + DVec3::new(vx * 0.1, 0.3, vz * 0.1),
                                    DVec3::new(vx, vy, vz),
                                    spell_speed as f32,
                                    brightness,
                                    rgb,
                                    &mut self.random.random,
                                ) {
                                    sim.particles.push(p);
                                }
                            }
                        }
                        (
                            "game.potion.smash",
                            1.,
                            0.9 + self.random.random.next_float() * 0.1,
                        )
                    }
                    2003 => {
                        let eye_pt = block.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
                        for _ in 0..8 {
                            let vel = DVec3::new(
                                self.random.next_gaussian() * 0.15,
                                self.random.random.next_double() * 0.2,
                                self.random.next_gaussian() * 0.15,
                            );
                            if self.should_spawn_effect_particle() {
                                self.particle(sim, world, 36, eye_pt, vel, [381, 0]);
                            }
                        }
                        for step in 0..40 {
                            let angle = f64::from(step) * (std::f64::consts::TAU / 40.0);
                            let (sin, cos) = angle.sin_cos();
                            let ring_at = eye_pt + DVec3::new(cos * 5.0, -0.4, sin * 5.0);
                            for mult in [-5.0, -7.0] {
                                if self.should_spawn_effect_particle() {
                                    self.particle(
                                        sim,
                                        world,
                                        24,
                                        ring_at,
                                        DVec3::new(cos * mult, 0.0, sin * mult),
                                        [0; 2],
                                    );
                                }
                            }
                        }
                        return;
                    }
                    2004 => {
                        for _ in 0..20 {
                            let d1 = DVec3::new(
                                (self.random.random.next_double() - 0.5) * 2.0,
                                (self.random.random.next_double() - 0.5) * 2.0,
                                (self.random.random.next_double() - 0.5) * 2.0,
                            );
                            let d2 = DVec3::new(
                                (self.random.random.next_double() - 0.5) * 2.0,
                                (self.random.random.next_double() - 0.5) * 2.0,
                                (self.random.random.next_double() - 0.5) * 2.0,
                            );
                            if self.should_spawn_effect_particle() {
                                self.particle(sim, world, 11, point + d1, DVec3::ZERO, [0; 2]);
                            }
                            if self.should_spawn_effect_particle() {
                                self.particle(sim, world, 26, point + d2, DVec3::ZERO, [0; 2]);
                            }
                        }
                        return;
                    }
                    2005 => {
                        let n = if data == 0 { 15 } else { data.clamp(0, 4096) };
                        for _ in 0..n {
                            let vel = DVec3::new(
                                self.random.next_gaussian() * 0.02,
                                self.random.next_gaussian() * 0.02,
                                self.random.next_gaussian() * 0.02,
                            );
                            let at = DVec3::new(
                                f64::from(block.x) + f64::from(self.random.random.next_float()),
                                f64::from(block.y) + f64::from(self.random.random.next_float()),
                                f64::from(block.z) + f64::from(self.random.random.next_float()),
                            );
                            if self.should_spawn_effect_particle() {
                                self.particle(sim, world, 21, at, vel, [0; 2]);
                            }
                        }
                        return;
                    }
                    _ => return,
                };
                self.sound(name, point, listener, volume, pitch);
            }
            other => sim.presentation.apply(other),
        }
    }
    fn should_spawn_effect_particle(&mut self) -> bool {
        match self.particle_setting {
            0 => true,
            1 => self.random.random.next_int(3) != 0,
            _ => false,
        }
    }
    fn sound(
        &mut self,
        name: &str,
        source: glam::DVec3,
        listener: glam::DVec3,
        volume: f32,
        pitch: f32,
    ) {
        let range = 16. * f64::from(volume.max(1.));
        if volume > 0. && source.distance_squared(listener) < range * range {
            self.sounds.push((
                name.trim_start_matches("minecraft:").to_owned(),
                volume,
                pitch,
                source,
            ));
        }
    }
    pub(crate) fn spawn_client_particle(
        &mut self,
        sim: &mut crate::gameplay::Simulation,
        world: &crate::world::World,
        listener: glam::DVec3,
        id: u8,
        point: glam::DVec3,
        velocity: glam::DVec3,
    ) {
        let visible = match self.particle_setting {
            0 => true,
            1 => self.random.random.next_int(3) != 0,
            _ => false,
        };
        if matches!(id, 0..=2 | 41) || (visible && point.distance_squared(listener) <= 256.) {
            self.particle(sim, world, id, point, velocity, [0; 2]);
        }
    }
    fn particle(
        &mut self,
        sim: &mut crate::gameplay::Simulation,
        world: &crate::world::World,
        id: u8,
        point: glam::DVec3,
        velocity: glam::DVec3,
        args: [i32; 2],
    ) {
        if sim.particles.len() >= 4096 {
            return;
        }
        use crate::gameplay::Particle;
        if id == 2 {
            if self.explosions.len() < 512 {
                self.explosions.push((point, 0));
            }
            return;
        }
        if id == 36 {
            if let Some(item) = u16::try_from(args[0]).ok().filter(|v| *v != 0) {
                sim.particles.push(Particle::item(
                    item,
                    args[1] as u16,
                    false,
                    point,
                    velocity,
                    &mut self.random.random,
                ));
            }
        } else if id == 37 {
            let state = (((args[0] & 4095) << 4) | ((args[0] >> 12) & 15)) as u16;
            if (1..256).contains(&(state >> 4)) {
                sim.particles.push(Particle::digging(
                    point,
                    velocity,
                    state,
                    point.floor().as_ivec3(),
                    false,
                    &mut self.random.random,
                ));
            }
        } else if id == 38 {
            let state = (((args[0] & 4095) << 4) | ((args[0] >> 12) & 15)) as u16;
            if (1..256).contains(&(state >> 4)) {
                if let Some(p) = Particle::block_dust(
                    point,
                    velocity,
                    state,
                    point.floor().as_ivec3(),
                    &mut self.random.random,
                ) {
                    sim.particles.push(p);
                }
            }
        } else if matches!(id, 9 | 10) {
            sim.particles.push(Particle::critical(
                id == 10,
                point,
                velocity,
                &mut self.random.random,
                world,
            ));
        } else if let Some(p) = Particle::special(id, point, velocity, &mut self.random.random) {
            sim.particles.push(p);
        } else if let Some(p) =
            Particle::network_sprite(id, point, velocity, &mut self.random.random)
        {
            sim.particles.push(p);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Writer;
    fn message(id: i32, make: impl FnOnce(&mut Writer)) -> Packet {
        let mut w = Writer::new();
        make(&mut w);
        decode(id, &mut Reader::new(&w.0)).unwrap()
    }
    #[test]
    fn original_jar_serialized_every_particle_and_presentation_packet_family() {
        let examples: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-presentation-packets.json"
        ))
        .unwrap();
        let mut state = State::default();
        let mut seen = std::collections::BTreeSet::new();
        for row in examples {
            let id = row["id"].as_i64().unwrap() as i32;
            let hex = row["payload"].as_str().unwrap();
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect::<Vec<_>>();
            let packet = decode(id, &mut Reader::new(&bytes)).unwrap();
            match packet {
                Packet::Particles {
                    id,
                    far,
                    count,
                    args,
                    ..
                } => {
                    seen.insert(id);
                    assert!(far);
                    assert_eq!(count, 10);
                    if id == 36 {
                        assert_eq!(args, [264, 0]);
                    } else if matches!(id, 37 | 38) {
                        assert_eq!(args[0], 4099);
                    }
                }
                Packet::Effect { id, pos, data, .. } => {
                    assert_eq!((id, pos, data), (2001, [-37, 74, 191], 0x1003))
                }
                Packet::Sound {
                    name,
                    pos,
                    volume,
                    pitch,
                } => {
                    assert_eq!(name, "note.harp");
                    assert_eq!(pos, [-1.25, 74.5, 3.75]);
                    assert_eq!(volume, 0.75);
                    assert_eq!(pitch, 78. / 63.);
                }
                data => state.apply(data),
            }
        }
        assert_eq!(seen, (0..42).collect());
        assert_eq!(state.formatted_name("Alex"), "§aAlex!");
        assert_eq!(state.objective(1).unwrap().scores["Alex"], -2);
        assert_eq!(state.timer, 8);
        assert!(state.title.contains("Hello"));
        assert!(state.header.contains("Header") && state.footer.contains("Footer"));
    }
    #[test]
    fn objective_team_score_and_tab_lifecycle_follow_the_play_protocol() {
        let mut s = State::default();
        s.apply(message(0x3b, |w| {
            w.string("side").u8(0).string("Points").string("integer");
        }));
        s.apply(message(0x3d, |w| {
            w.u8(1).string("side");
        }));
        s.apply(message(0x3c, |w| {
            w.string("Alex").varint(0).string("side").varint(-2);
        }));
        s.apply(message(0x3e, |w| {
            w.string("red")
                .u8(0)
                .string("Red")
                .string("§c")
                .string("!")
                .u8(3)
                .string("always")
                .u8(12)
                .varint(1)
                .string("Alex");
        }));
        assert_eq!(s.objective(1).unwrap().scores["Alex"], -2);
        assert_eq!(s.formatted_name("Alex"), "§cAlex!");
        let team = s.team("Alex").unwrap();
        assert_eq!(
            (&*team.label, team.flags, &*team.visibility),
            ("Red", 3, "always")
        );
        s.apply(message(0x47, |w| {
            w.string(r#"{"text":"header"}"#)
                .string(r#"{"text":"footer"}"#);
        }));
        assert!(s.header.contains("header") && s.footer.contains("footer"));
        s.apply(message(0x3e, |w| {
            w.string("red").u8(4).varint(1).string("Alex");
        }));
        assert_eq!(s.formatted_name("Alex"), "Alex");
        s.apply(message(0x3c, |w| {
            w.string("Alex").varint(1).string("");
        }));
        assert!(s.objective(1).unwrap().scores.is_empty());
        s.apply(message(0x3b, |w| {
            w.string("side").u8(1);
        }));
        assert!(s.display[1].is_empty());
    }
    #[test]
    fn title_duration_clear_and_reset_use_twenty_hz_ticks() {
        let mut s = State::default();
        s.apply(message(0x45, |w| {
            w.varint(2).i32(-1).i32(0).i32(1);
        }));
        s.apply(message(0x45, |w| {
            w.varint(1).string(r#""subtitle""#);
        }));
        assert_eq!(s.timer, 0);
        s.apply(message(0x45, |w| {
            w.varint(0).string(r#""title""#);
        }));
        assert_eq!(s.timer, 11);
        for _ in 0..11 {
            s.tick();
        }
        assert!(s.title.is_empty() && s.subtitle.is_empty());
        s.apply(message(0x45, |w| {
            w.varint(4);
        }));
        assert_eq!(s.times, [10, 70, 20]);
    }
    #[test]
    fn world_sound_particle_arguments_and_signed_packed_positions() {
        let effect = message(0x28, |w| {
            w.i32(2001).position([-45, 80, 31]).i32(3).bool(false);
        });
        assert!(matches!(
            effect,
            Packet::Effect {
                id: 2001,
                pos: [-45, 80, 31],
                data: 3,
                global: false
            }
        ));
        let sound = message(0x29, |w| {
            w.string("note.harp")
                .i32(-10)
                .i32(16)
                .i32(32)
                .f32(0.75)
                .u8(126);
        });
        assert!(matches!(
            sound,
            Packet::Sound {
                pos: [-1.25, 2., 4.],
                pitch: 2.,
                volume: 0.75,
                ..
            }
        ));
        for (id, args) in [
            (23, vec![]),
            (36, vec![264, 7]),
            (37, vec![4099]),
            (38, vec![4099]),
            (999, vec![]),
        ] {
            let mut w = Writer::new();
            w.i32(id).bool(true);
            for _ in 0..7 {
                w.f32(0.5);
            }
            w.i32(0);
            for &arg in &args {
                w.varint(arg);
            }
            let Packet::Particles {
                id: parsed,
                far,
                count,
                args: got,
                ..
            } = decode(0x2a, &mut Reader::new(&w.0)).unwrap()
            else {
                panic!()
            };
            assert_eq!(parsed, if id < 42 { id as u8 } else { 35 });
            assert!(far);
            assert_eq!(count, 0);
            assert_eq!(got[..args.len()], args);
            w.u8(0);
            assert!(decode(0x2a, &mut Reader::new(&w.0)).is_err());
        }
    }
}
