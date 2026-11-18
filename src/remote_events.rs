use super::*;
use crate::network::play_events::{Combat, MapPatch, Offer, Plugin, Update};

#[derive(Debug)]
pub struct MapData {
    pub scale: i8,
    pub icons: Vec<[u8; 3]>,
    pub colors: Vec<u8>,
    revision: u64,
}
impl MapData {
    fn update(&mut self, patch: MapPatch) {
        self.revision = self.revision.wrapping_add(1);
        self.scale = patch.scale;
        self.icons = patch.icons;
        let [x, y, w, h] = patch.rect;
        for row in 0..h {
            self.colors[(y + row) * 128 + x..(y + row) * 128 + x + w]
                .copy_from_slice(&patch.colors[row * w..(row + 1) * w]);
        }
    }
}
#[derive(Default)]
pub(crate) struct State {
    pub packs: std::collections::VecDeque<(String, String)>,
    pub stats: HashMap<String, i32>,
    pub stats_revision: u64,
    pub brand: String,
    pub maps: HashMap<i32, MapData>,
    pub camera: Option<i32>,
    pub sign: Option<(IVec3, [String; 4])>,
    pub pending_signs: HashMap<IVec3, [String; 4]>,
    pub window: Option<Window>,
    pub opens: Option<bool>,
    pub book: bool,
    sleeping: HashMap<i32, IVec3>,
    lightning: HashMap<i32, ([f64; 3], u8)>,
    pub(super) swings: HashMap<i32, u8>,
    cracks: HashMap<i32, (IVec3, u8, u16)>,
    pub(super) sounds: Vec<(&'static str, f32, f32)>,
}
pub(crate) struct Window {
    pub id: u8,
    pub kind: String,
    pub title: String,
    pub count: usize,
    pub slots: Vec<Option<Slot>>,
    pub container: crate::container::Container,
    pub properties: HashMap<i16, i16>,
    pub anvil_name:String,
    pub offers: Vec<Offer>,
}
impl State {
    pub fn reset_world(&mut self) {
        let stats = std::mem::take(&mut self.stats);
        let brand = std::mem::take(&mut self.brand);
        *self = Self {
            stats,
            brand,
            ..Default::default()
        };
    }
    pub fn remove_entity(&mut self, id: i32) {
        self.sleeping.remove(&id);
        self.swings.remove(&id);
        self.lightning.remove(&id);
        if self.camera == Some(id) {
            self.camera = None;
        }
    }
    pub fn tick(&mut self, out: &mut gameplay::Outcome) {
        self.cracks.retain(|_, (_, _, age)| {
            *age += 1;
            *age <= 400
        });
        self.swings.retain(|_, age| {
            *age += 1;
            *age < 6
        });
        self.lightning.retain(|_, (_, age)| {
            *age += 1;
            *age < 8
        });
        out.pitched_sounds.append(&mut self.sounds);
    }
    fn map(&mut self, patch: MapPatch) {
        self.maps
            .entry(patch.id)
            .or_insert_with(|| MapData {
                scale: 0,
                icons: vec![],
                colors: vec![0; 128 * 128],
                revision: 0,
            })
            .update(patch);
    }
}
fn pose(p: [f64; 3]) -> Pose {
    Pose {
        x: p[0],
        y: p[1],
        z: p[2],
        ..Default::default()
    }
}
fn simple_text(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::String(s) => out.push_str(s),
        serde_json::Value::Array(a) => {
            for v in a {
                simple_text(v, out)
            }
        }
        serde_json::Value::Object(o) => {
            if let Some(s) = o.get("text").and_then(|v| v.as_str()) {
                out.push_str(s)
            }
            if let Some(v) = o.get("extra") {
                simple_text(v, out)
            }
        }
        _ => {}
    }
}
pub(crate) fn plain(json: &str) -> String {
    if let Ok(v) = serde_json::from_str(json) {
        let mut out = String::new();
        simple_text(&v, &mut out);
        out
    } else {
        json.to_owned()
    }
}
fn has_visible_label_text(text: &str) -> bool {
    let mut chars = text.chars();
    let mut decorated = false;
    while let Some(ch) = chars.next() {
        if ch == '§' {
            if let Some(code) = chars.next() {
                match code.to_ascii_lowercase() {
                    'm' | 'n' => decorated = true,
                    '0'..='9' | 'a'..='f' | 'r' => decorated = false,
                    _ => {}
                }
            }
            continue;
        }
        if !ch.is_whitespace() || decorated {
            return true;
        }
    }
    false
}
impl Remote {
    pub(super) fn sync_camera(&self, sim: &mut Simulation) {
        sim.server_scene.camera = self.extra.camera;
        sim.server_scene.camera_hurt = self.extra.camera.and_then(|id| self.entities.get(&id))
            .map(|e| (e.hurt_camera, e.dead));
    }
    pub(super) fn hurt_animation(&mut self, id: i32, sim: &mut Simulation) {
        if id == self.entity_id {
            sim.hurt_camera.hurt();
        } else if let Some(e) = self.entities.get_mut(&id).filter(|e| e.living()) {
            e.hurt_camera.hurt();
            if let Some(m) = sim.mobs.iter_mut().find(|m| m.id == local_id(id)) {
                m.hurt = e.hurt_camera.hurt_time;
            }
        }
    }
    pub(super) fn sync_scene(&self, sim: &mut Simulation) {
        self.sync_camera(sim);
        let s = &mut sim.server_scene;
        s.burning.clear();
        s.bosses.clear();
        s.cracks.clear();
        s.cracks
            .extend(self.extra.cracks.values().map(|(p, n, _)| (*p, *n)));
        s.lightning.clear();
        s.lightning.extend(
            self.extra
                .lightning
                .values()
                .map(|(p, n)| (DVec3::from(*p).as_vec3(), *n)),
        );
        for actor in s.actors.values_mut() {
            actor.previous = actor.feet;
        }
        s.actors.retain(|id, _| self.entities.contains_key(id));
        s.mobs.retain(|id, _| {
            self.entities.get(id).is_some_and(|e| {
                !(e.dead && e.dragon.is_none() && e.hurt_camera.death_time >= 20)
            })
        });
        s.basic_mobs.retain(|lid, _| {
            self.entities.get(&(*lid as i32)).is_some_and(|e| {
                !(e.dead && e.dragon.is_none() && e.hurt_camera.death_time >= 20)
            })
        });
        let fishing_anglers: std::collections::HashSet<i32> = self
            .entities
            .values()
            .filter_map(|e| match e.kind {
                EntityKind::Object(90, angler) if angler > 0 => Some(angler),
                _ => None,
            })
            .collect();
        s.local_fishing_cast = fishing_anglers.contains(&self.entity_id);
        s.projectiles.clear();
        s.labels.clear();
        for (&id, e) in &self.entities {
            let swing_ticks = self
                .extra
                .swings
                .get(&id)
                .map_or(0, |&age| 6u8.saturating_sub(age));
            let burning = match e.mob_id {
                Some(61) => e.appearance.flags & 1 != 0, // Blaze's charged watcher
                Some(56 | 57 | 62 | 63 | 64) => false,
                Some(51) if e.appearance.subtype == 1 => false,
                _ => e.flags & 1 != 0,
            };
            if burning { s.burning.insert(id); }
            if let Some(b)=e.boss.as_ref(){s.bosses.push(crate::models::boss_bar::Candidate{id,section:glam::IVec3::new(e.section[0],e.section[2],e.section[1]),
                order:e.inserted,at:e.position,boss:(**b).clone()});}
            if !matches!(e.kind, EntityKind::Player(_))
                && self.extra.camera != Some(id)
                && e.custom_name_visible
                && has_visible_label_text(&e.custom_name)
                && !(e.dead && e.dragon.is_none() && e.hurt_camera.death_time >= 20)
            {
                let height = entity_height(e);
                let sneaking = (e.flags & 0x20 == 0) && (e.flags & 0x02 != 0);
                s.labels.push(crate::models::server::RemoteLabel {
                    id,
                    previous: e.previous_position.as_vec3(),
                    position: e.position.as_vec3(),
                    y_offset: height + 0.5 - if sneaking { 0.25 } else { 0.0 },
                    text: e.custom_name.clone(),
                    sneaking,
                });
            }
            if let EntityKind::Object(
                kind @ (51 | 60 | 61 | 62 | 63 | 64 | 65 | 66 | 70 | 72 | 73 | 75 | 76 | 90),
                data,
            ) = e.kind
            {
                if e.flags & 32 == 0 {
                    let pos = e.position.as_vec3();
                    let previous = e.previous_position.as_vec3();
                    let [yaw, pitch] = e.rotation;
                    let [previous_yaw, previous_pitch] = e.previous_rotation;
                    s.projectiles.push(crate::models::server::RemoteProjectile {
                        id,
                        kind,
                        data,
                        is_local_owner: data == self.entity_id,
                        invulnerable: e.invulnerable_skull,
                        previous,
                        position: pos,
                        previous_yaw,
                        yaw,
                        previous_pitch,
                        pitch,
                    });
                }
                continue;
            }
            if let (EntityKind::Object(kind @ (1|10),variant),Some(v))=(e.kind,e.vehicle.as_ref()){
                use crate::models::vehicle::{Actor,Damage};
                if e.flags&32!=0{s.vehicles.remove(&id);continue}
                let at=e.position;
                let actor=s.vehicles.entry(id).or_insert(Actor{cart:kind==10,from:at,to:at,previous_yaw:v.yaw,yaw:v.yaw,
                    previous_pitch:v.pitch,pitch:v.pitch,damage:Damage::default(),display:None});
                actor.from=actor.to;actor.to=at;actor.previous_yaw=actor.yaw;actor.yaw=v.yaw;
                actor.previous_pitch=actor.pitch;actor.pitch=v.pitch;
                actor.damage=Damage{ticks:v.hit_ticks,amount:v.damage,direction:v.direction};actor.display=v.display(variant);
                continue
            }
            if let Some(species) = e.mob_id.filter(|&sp| mob_kind(sp).is_some()) {
                if e.dead && e.dragon.is_none() && e.hurt_camera.death_time >= 20 {
                    s.basic_mobs.remove(&local_id(id));
                    continue;
                }
                let baby = if matches!(species, 54 | 57) {
                    e.appearance.age > 0
                } else {
                    matches!(species, 90..=93 | 120) && e.appearance.age < 0
                };
                s.basic_mobs.insert(
                    local_id(id),
                    crate::models::server::BasicMobVisual {
                        baby,
                        invisible: e.flags & 32 != 0,
                        villager_profession: e.villager_profession,
                        sheep_color: e.sheep_color,
                        sheep_sheared: e.sheep_sheared,
                        pig_saddled: e.pig_saddled,
                        skeleton_wither: species == 51 && e.appearance.subtype == 1,
                        zombie_villager: species == 54 && e.appearance.subtype != 0,
                        creeper_state: e.creeper_state,
                        creeper_powered: e.creeper_powered,
                        creeper_ignited: e.creeper_ignited,
                        creeper_swell: e.creeper_swell,
                        prev_creeper_swell: e.prev_creeper_swell,
                        swing_ticks,
                        eat_ticks: e.eat_ticks,
                        death_time: e.hurt_camera.death_time,
                        head_yaw: e.head,
                        pitch: e.rotation[1],
                        held: e.hand.clone(),
                        armor: e.armor.clone(),
                    },
                );
                continue;
            }
            if let Some(species) = e
                .mob_id
                .filter(|&sp| mob_kind(sp).is_none() && crate::remote_mobs::species(sp).is_some())
            {
                if e.dead && e.dragon.is_none() && e.hurt_camera.death_time >= 20 {
                    s.mobs.remove(&id);
                    continue;
                }
                let feet = e.position.as_vec3();
                let mut appearance = e.appearance;
                if species == 100 {
                    let ridden = self.riding == Some(id)
                        || self.entities.values().any(|other| other.riding == Some(id));
                    if ridden {
                        appearance.flags |= 256;
                    } else {
                        appearance.flags &= !256;
                    }
                }
                let beam_target_pos = if species == 68 {
                    e.beam_target.and_then(|tid| {
                        if tid == self.entity_id {
                            Some(self.local_player_center)
                        } else {
                            self.entities.get(&tid).map(|te| {
                                te.position.as_vec3() + Vec3::Y * (entity_height(te) * 0.5)
                            })
                        }
                    })
                } else {
                    None
                };
                let actor = s.mobs.entry(id).or_insert_with(|| crate::remote_mobs::Actor {
                    species,
                    previous: feet,
                    feet,
                    yaw: e.pose.yaw,
                    appearance,
                    collar_color: e.collar_color,
                    ..Default::default()
                });
                let step = (feet - actor.feet).with_y(0.).length();
                actor.previous = actor.feet;
                actor.feet = feet;
                actor.gait.advance(actor.yaw.to_radians(), step);
                actor.species = species;
                actor.yaw = e.vehicle.as_ref().map_or(e.rotation[0], |v| v.yaw);
                actor.head_yaw = e.head;
                actor.pitch = e.rotation[1];
                actor.appearance = appearance;
                actor.hurt = e.dead || e.hurt_camera.hurt_time > 0;
                actor.invisible = e.flags & 32 != 0;
                actor.death_time = if e.dragon.is_some() { 0 } else { e.hurt_camera.death_time };
                actor.swing_ticks = swing_ticks;
                actor.attack_ticks = e.attack_ticks;
                actor.rose_ticks = e.rose_ticks;
                actor.eat_ticks = e.eat_ticks;
                actor.held.clone_from(&e.hand);
                actor.armor.clone_from(&e.armor);
                actor.horse_anim = e.horse_anim;
                actor.horse_armor = e.horse_armor;
                actor.collar_color = e.collar_color;
                actor.beam_target = e.beam_target;
                actor.beam_target_pos = beam_target_pos;
                actor.slime_squish = e.slime_squish;
                actor.prev_slime_squish = e.prev_slime_squish;
                actor.dragon_death_ticks = e.dragon_death_ticks;
                actor.dragon_flight.clone_from(&e.dragon);
                continue;
            }
            let EntityKind::Player(uuid) = e.kind else {
                continue;
            };
            if self.player_modes.get(&uuid) == Some(&3) {
                s.actors.remove(&id);
                continue;
            }
            let p = e.position.as_vec3();
            let alex = sim.presentation.slim_for(uuid);
            let head_yaw = e.head.unwrap_or(e.rotation[0]);
            let actor = s
                .actors
                .entry(id)
                .or_insert_with(|| {
                    let mut anim = crate::player::third_person::Animation::default();
                    anim.slim = alex;
                    anim.tick(DVec3::ZERO, head_yaw);
                    crate::models::server::Actor {
                        uuid: Some(uuid),
                        gait: Default::default(),
                        anim,
                        swing_anim: Default::default(),
                        previous: p,
                        feet: p,
                        previous_yaw: head_yaw,
                        yaw: head_yaw,
                        previous_pitch: e.rotation[1],
                        pitch: e.rotation[1],
                        skin: 0,
                        alex,
                        held: None,
                        armor: Default::default(),
                        sneaking: false,
                        invisible: e.flags & 32 != 0,
                        using_item: false,
                        fishing_cast: false,
                        hurt: false,
                        bed: None,
                        swing: 0.,
                        name: None,
                    }
                });
            actor.previous = actor.feet;
            actor.feet = p;
            let delta = (actor.feet - actor.previous).as_dvec3();
            actor.gait.advance(actor.yaw.to_radians(), (actor.feet - actor.previous).with_y(0.).length());
            actor.previous_yaw = actor.yaw;
            actor.yaw = head_yaw;
            actor.previous_pitch = actor.pitch;
            actor.pitch = e.rotation[1];
            if self.extra.swings.get(&id) == Some(&0) {
                actor.swing_anim.start(6);
            }
            actor.swing_anim.advance(6);
            actor.anim.slim = alex;
            actor.anim.riding = e.riding.is_some();
            actor.anim.mount_yaw = e
                .riding
                .and_then(|vid| self.entities.get(&vid))
                .and_then(|ve| ve.mob_id.map(|_| {
                    let body_yaw = ve.vehicle.as_ref().map_or(ve.rotation[0], |v| v.yaw);
                    let last = actor.anim.mount_yaw.map_or(body_yaw, |m| m[1]);
                    [last, body_yaw]
                }));
            actor.anim.swinging = actor.swing_anim.in_progress();
            actor.anim.tick(delta, head_yaw);
            actor.uuid = Some(uuid);
            actor.skin = e.skin;
            actor.alex = alex;
            actor.held.clone_from(&e.hand);
            actor.armor.clone_from(&e.armor);
            actor.sneaking = e.flags & 2 != 0;
            actor.invisible = e.flags & 32 != 0;
            actor.using_item = e.flags & 16 != 0;
            actor.fishing_cast = fishing_anglers.contains(&id);
            actor.bed = self.extra.sleeping.get(&id).copied();
            actor.hurt = e.dead || e.hurt_camera.hurt_time > 0;
            actor.swing = actor.swing_anim.sample(1.);
            actor.name = self
                .names
                .get(&uuid)
                .or_else(|| self.tab_names.get(&uuid))
                .cloned();
        }
        if let Some((id, map)) = sim
            .inventory
            .held()
            .filter(|i| i.id == 358)
            .and_then(|i| Some((i32::from(i.meta), self.extra.maps.get(&i32::from(i.meta))?)))
        {
            if s.map
                .as_ref()
                .is_none_or(|m| m.id != id || m.revision != map.revision)
            {
                s.map = Some(crate::models::server::MapSurface {
                    id,
                    revision: map.revision,
                    rgba: Arc::new(crate::models::server::pixels(&map.colors)),
                    icons: map.icons.clone(),
                });
            }
        } else {
            s.map = None;
        }
    }
    pub fn camera_player(&self) -> Option<Player> {
        let id = self.extra.camera?;
        let e = self.entities.get(&id)?;
        let mut p = Player::new(e.position.as_vec3());
        p.set_pose_f64(e.position.to_array(), e.pose.yaw, e.pose.pitch, false);
        p.sleeping = self.extra.sleeping.contains_key(&id);
        p.previous = p.position;
        Some(p)
    }
    pub(super) fn critical_bounds(&self, id: i32, player: &Player) -> Option<crate::vanilla::movement::Aabb> {
        use crate::vanilla::entity_pick::{body, object_bounds};
        if id == self.entity_id {
            return Some(body(player.position.as_dvec3(), 0.6, 1.8));
        }
        let entity = self.entities.get(&id)?;
        if entity.mob_id == Some(30) {
            return object_bounds(78, 0, entity.position, entity.armor_flags).map(|info| info.0);
        }
        let [w, h, _] = if let Some(kind) = entity.mob_id {
            entity.appearance.size(kind)?
        } else {
            match entity.kind {
                EntityKind::Player(_) => [0.6, 1.8, 0.1],
                EntityKind::Object(kind, data) => return object_bounds(kind, data, entity.position, entity.armor_flags).map(|info| info.0),
                EntityKind::DragonPart(i) => { let [w, h] = dragon::SIZES[i as usize]; [w, h, 0.] }
                _ => return None,
            }
        };
        Some(body(entity.position, w, h))
    }

    pub fn apply_world_event(
        &mut self,
        update: Update,
        world: &mut World,
        player: &mut Player,
        sim: &mut Simulation,
        dirty: &mut HashSet<ChunkPos>,
    ) -> Result<()> {
        match update {
            Update::Bed(id, p) if id == self.entity_id => {
                sim.sleep.head = Some(p);
                sim.sleep.timer = 0;
                player.sleeping = true;
                player.set_position_f64(crate::vanilla::sleep::resting_position(p, world.get(p)));
                player.set_velocity_f64([0.; 3]);
            }
            Update::Bed(id, p) => {
                if let Some(e) = self.entities.get_mut(&id).filter(|e| matches!(e.kind, EntityKind::Player(_))) {
                    e.position = DVec3::from_array(crate::vanilla::sleep::resting_position(p, world.get(p)));
                    e.target_position = e.position;
                    e.steps = 0;
                    self.extra.sleeping.insert(id, p);
                }
            }
            Update::Animation(id, 1) => {
                self.hurt_animation(id, sim);
                if let Some(v) = self.entities.get_mut(&id).and_then(|e| e.vehicle.as_mut()) {
                    v.hurt();
                }
            }
            Update::Animation(id, kind @ (4 | 5)) => {
                if let Some(bounds) = self.critical_bounds(id, player) {
                    sim.critical_emitters.start(local_id(id), bounds, kind == 5, &mut sim.particles,
                        world, player.position.as_dvec3(), self.visuals.particle_setting);
                }
            }
            Update::Animation(id, kind) if id == self.entity_id => {
                if kind == 0 {
                    sim.hand_swing
                        .start(crate::first_person::swing_duration(&sim.effects));
                }
                if kind == 2 {
                    sim.wake(world, player, false, false);
                }
            }
            Update::Animation(id, kind) => {
                if let Some(entity) = self.entities.get_mut(&id) {
                    if kind == 0 {
                        self.extra.swings.insert(id, 0);
                        if let Some(m) = sim.mobs.iter_mut().find(|m| m.id == local_id(id)) {
                            m.swing_ticks = 6;
                        }
                    }
                    if kind == 2 {
                        if let Some(head) = self.extra.sleeping.remove(&id).filter(|&p| world.get(p) >> 4 == 26) {
                            let bed = crate::vanilla::respawn::BedSpawn { point: head, forced: false };
                            entity.position = DVec3::from_array(crate::vanilla::sleep::wake_position(world, bed));
                        }
                    }
                }
            }
            Update::Painting(id, title, p, facing) => {
                self.spawn_entity(
                    id,
                    EntityKind::Other,
                    pose((p.as_dvec3() + DVec3::splat(0.5)).to_array()),
                );
                sim.server_scene.paintings.insert(id, (title, p, facing));
            }
            Update::Orb(id, p, value) => {
                self.remove_entity(id, sim);
                self.spawn_entity(id, EntityKind::Other, pose(p));
                let mut rng = crate::vanilla::random::JavaRandom::from_state(sim.xp_math_random);
                let orb = gameplay::xp_orb::Orb::new(
                    id,
                    p.into(),
                    value.into(),
                    &mut rng,
                    sim.xp_math_random,
                );
                sim.xp_math_random = rng.state();
                sim.xp_orbs.push(orb);
            }
            Update::BlockAction(p, action, param, block @ (29 | 33)) => {
                let mut changed = Vec::new();
                if let Some((sound, vol, pitch)) =
                    sim.remote_piston_action(world, p, action, param, block, &mut changed)
                {
                    self.visuals.sounds.push((
                        sound.into(),
                        vol,
                        pitch,
                        p.as_dvec3() + DVec3::splat(0.5),
                    ));
                }
                for c in changed {
                    world.edits.remove(&c.to_array());
                    self.live_tiles.remove(&c);
                    dirty.extend(neighbors((c.x.div_euclid(16), c.z.div_euclid(16))));
                }
            }
            Update::BlockAction(p, instrument, pitch, block) if world.get(p) >> 4 == block => {
                if block == 25 {
                    sim.queue_note(p, instrument, pitch);
                } else if block == 52 && instrument == 1 {
                    sim.reset_mp_spawner_delay(world, p);
                } else if instrument == 1 && matches!(block, 54 | 130 | 146) {
                    sim.server_scene.chest_lids.server_users(p, pitch);
                }
            }
            Update::BlockAction(..) => {}
            Update::Break(id, p, n) => {
                if n <= 9 {
                    self.extra.cracks.insert(id, (p, n, 0));
                } else {
                    self.extra.cracks.remove(&id);
                }
            }
            Update::Explosion {
                center,
                strength,
                blocks,
                push,
            } => {
                self.visuals.handle(
                    crate::net_presentation::Packet::Sound {
                        name: "random.explode".into(),
                        pos: center.map(f64::from),
                        volume: 4.,
                        pitch: 0.7,
                    },
                    player,
                    sim,
                    world,
                );
                self.visuals.handle(
                    crate::net_presentation::Packet::Particles {
                        id: if strength >= 2. { 2 } else { 1 },
                        far: true,
                        pos: center,
                        offset: [0.; 3],
                        speed: 0.,
                        count: 1,
                        args: [0; 2],
                    },
                    player,
                    sim,
                    world,
                );
                for p in blocks {
                    self.live_tiles.remove(&p);
                    world.set(p, 0);
                    world.edits.remove(&p.to_array());
                    dirty.extend(neighbors((p.x.div_euclid(16), p.z.div_euclid(16))));
                }
                let v = player.precise_velocity();
                player.set_velocity_f64(std::array::from_fn(|i| v[i] + f64::from(push[i])));
            }
            Update::Global(id, 1, p) => {
                self.extra.lightning.insert(id, (p, 0));
                let pos = DVec3::from_array(p);
                let thunder_pitch = 0.8 + self.pickup_random.next_float() * 0.2;
                let explode_pitch = 0.5 + self.pickup_random.next_float() * 0.2;
                self.visuals.sounds.push((
                    "ambient.weather.thunder".into(),
                    10_000.0,
                    thunder_pitch,
                    pos,
                ));
                self.visuals.sounds.push((
                    "random.explode".into(),
                    2.0,
                    explode_pitch,
                    pos,
                ));
            }
            Update::Global(..) => {}
            Update::Property(id, k, v) => {
                if let Some(w) = self.extra.window.as_mut().filter(|w| w.id == id) {
                    w.properties.insert(k, v);
                }
            }
            Update::Map(p) => self.extra.map(p),
            Update::Sign(p) => {
                use crate::vanilla::drops::DropNbt;
                let lines = if let Some(pending) = self.extra.pending_signs.get(&p) {
                    pending.clone()
                } else {
                    let tag = world.tile(p).and_then(crate::vanilla::item_nbt::compound);
                    std::array::from_fn(|i| {
                        match tag.and_then(|t| t.get(&format!("Text{}", i + 1))) {
                            Some(DropNbt::String(s)) => plain(s),
                            _ => String::new(),
                        }
                    })
                };
                self.extra.sign = Some((p, lines));
            }
            Update::Stats(stats) => {
                self.extra.stats.extend(stats);
                self.extra.stats_revision = self.extra.stats_revision.wrapping_add(1);
            }
            Update::Plugin(Plugin::Brand(s)) => self.extra.brand = s,
            Update::Plugin(Plugin::Offers(id, offers)) => {
                if let Some(w) = self
                    .extra
                    .window
                    .as_mut()
                    .filter(|w| i32::from(w.id) == id && w.kind == "minecraft:villager")
                {
                    w.offers = offers;
                    self.sync_window(sim)?;
                }
            }
            Update::Plugin(Plugin::Book) => {
                self.extra.book = sim.inventory.held().is_some_and(|s| s.id == 387)
            }
            Update::Difficulty(d) => {
                sim.world_difficulty = Some(d);
                sim.difficulty_locked = false;
            }
            Update::Camera(id) => {
                if id == self.entity_id {
                    self.extra.camera = None;
                } else if self.entities.contains_key(&id) {
                    self.extra.camera = Some(id);
                }
            }
            // In the original, combat events feed the optional Twitch stream only;
            // health/death UI is controlled by S06. Entity.clientUpdateEntityNBT is empty.
            Update::Combat(Combat::Start | Combat::End(..) | Combat::Died(..))
            | Update::EntityNbt(_)
            | Update::Plugin(Plugin::Other) => {}
        }
        Ok(())
    }
}
