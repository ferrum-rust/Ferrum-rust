//! Integrated EntityHorse interaction, inventory and mounted travel.
use super::{command_entity_state::MobData, *};
use crate::{
    container::{Container, Kind, Special},
    vanilla::{
        drops::DropNbt as Tag,
        item_nbt,
        movement::{self, collision, Aabb, Travel},
    },
};
use glam::DVec3;
use winit::keyboard::KeyCode;

pub(super) fn saddled(data: Option<&MobData>) -> bool {
    data.and_then(|d| d.extra.get("SaddleItem"))
        .and_then(local_commands::read_stack)
        .is_some_and(|s| s.id == 329)
        || species_ai::number(data, "Saddle") != 0
}

fn equipment(data: Option<&MobData>, ridden: bool) -> Container {
    let kind = species_ai::number(data, "Type");
    let mut c = Container::new(Kind::Chest);
    c.special = Special::Horse { armor: kind == 0 };
    c.horse_ridden = ridden;
    let chest = matches!(kind, 1 | 2) && species_ai::number(data, "ChestedHorse") != 0;
    c.slots.truncate(if chest { 17 } else { 2 });
    if let Some(d) = data {
        for (slot, key) in ["SaddleItem", "ArmorItem"].iter().enumerate() {
            c.slots[slot] = d.extra.get(*key).and_then(local_commands::read_stack);
        }
        if c.slots[0].is_none() && saddled(data) {
            c.slots[0] = Some(Stack::new(329, 1));
        }
        if let Some(Tag::List { values, .. }) = d.extra.get("Items") {
            for tag in values {
                if let Some(m) = item_nbt::compound(tag) {
                    let index = item_nbt::number(m.get("Slot")) as usize;
                    if index >= 2 && index < c.slots.len() {
                        c.slots[index] = local_commands::read_stack(tag);
                    }
                }
            }
        }
    }
    c
}

pub(super) fn drops(data: Option<&MobData>) -> Vec<Stack> {
    let mut items: Vec<_> = equipment(data, false).slots.into_iter().flatten().collect();
    if species_ai::number(data, "ChestedHorse") != 0 {
        items.push(Stack::new(54, 1));
    }
    items
}

impl Simulation {
    pub fn riding_saddled_horse(&self) -> bool {
        let Some(id) = self.ridden_horse else {
            return false;
        };
        self.mobs
            .iter()
            .any(|m| m.id == id && m.kind == MobKind::Horse && m.health > 0.)
            && saddled(self.command_entities.mobs.get(&id))
    }
    pub fn horse_jump_power(&self) -> Option<f32> {
        self.riding_saddled_horse()
            .then(|| self.horse_rider.power())
    }
    pub fn mount_health(&self) -> Option<(f32, f32)> {
        let id = self.ridden_horse.or(self.ridden_pig)?;
        let m = self.mobs.iter().find(|m| m.id == id && m.health > 0.)?;
        let max_hp = m.kind.max_health(self.command_entities.mobs.get(&id));
        (max_hp > 0.).then_some((m.health, max_hp))
    }
    fn emit_horse_particles(
        &mut self,
        position: Vec3,
        width: f32,
        height: f32,
        id: u8,
        count: usize,
    ) {
        let mut rng = JavaRandom::from_state(self.particle_random[1]);
        for _ in 0..count {
            let vel = DVec3::new(
                (rng.next_double() * 2. - 1.) * 0.02,
                (rng.next_double() * 2. - 1.) * 0.02,
                (rng.next_double() * 2. - 1.) * 0.02,
            );
            let at = position.as_dvec3()
                + DVec3::new(
                    (f64::from(rng.next_float()) * f64::from(width) * 2.) - f64::from(width),
                    0.5 + f64::from(rng.next_float()) * f64::from(height),
                    (f64::from(rng.next_float()) * f64::from(width) * 2.) - f64::from(width),
                );
            if let Some(p) = Particle::network_sprite(id, at, vel, &mut rng) {
                self.particles.push(p);
            }
        }
        self.particle_random[1] = rng.state();
    }
    pub(crate) fn request_horse_window(&mut self, id: u64) -> bool {
        if species_ai::number(self.command_entities.mobs.get(&id), "Tame") == 0 {
            return false;
        }
        self.horse_window = Some(id);
        self.show_horse_window = true;
        true
    }
    pub(crate) fn horse_window_view(&self, player: &Player) -> Option<Container> {
        let id = self.horse_window?;
        self.mobs
            .iter()
            .any(|m| {
                m.id == id && m.health > 0. && player.position.distance_squared(m.position) < 64.
            })
            .then(|| {
                equipment(
                    self.command_entities.mobs.get(&id),
                    self.ridden_horse == Some(id),
                )
            })
    }
    pub(crate) fn horse_window_click(
        &mut self,
        slot: crate::inventory::SlotRef,
        action: crate::inventory::ClickAction,
    ) -> Vec<Stack> {
        let Some(id) = self.horse_window else {
            return Vec::new();
        };
        let mut c = equipment(
            self.command_entities.mobs.get(&id),
            self.ridden_horse == Some(id),
        );
        let dropped = crate::container::window_click(
            &mut self.inventory,
            Some(&mut c),
            slot,
            action,
            self.mode == Mode::Creative,
            2,
        );
        let extra = &mut self.command_entities.mobs.entry(id).or_default().extra;
        extra.remove("Saddle");
        for (slot, key) in ["SaddleItem", "ArmorItem"].iter().enumerate() {
            if let Some(stack) = &c.slots[slot] {
                extra.insert((*key).into(), local_commands::stack_tag(stack));
            } else {
                extra.remove(*key);
            }
        }
        let mut items = Vec::new();
        for (slot, stack) in c.slots.iter().enumerate().skip(2) {
            let Some(stack) = stack else { continue };
            if let Tag::Compound(mut tag) = local_commands::stack_tag(stack) {
                tag.insert("Slot".into(), Tag::Byte(slot as i8));
                items.push(Tag::Compound(tag));
            }
        }
        extra.insert(
            "Items".into(),
            Tag::List {
                element: 10,
                values: items,
            },
        );
        dropped
    }
    pub(super) fn interact_horse(
        &mut self,
        index: usize,
        player: &mut Player,
        out: &mut Outcome,
    ) -> bool {
        let m = &self.mobs[index];
        if m.kind != MobKind::Horse || m.health <= 0. {
            return false;
        }
        let (id, hp, yaw, pos) = (m.id, m.health, m.yaw, m.position);
        if self.ridden_horse == Some(id) {
            return false;
        }
        let item = self.inventory.held().map(|s| s.id);
        if item == Some(383) {
            return false;
        }
        let data = self.command_entities.mobs.get(&id);
        let tame = species_ai::number(data, "Tame") != 0;
        let kind = species_ai::number(data, "Type");
        let baby = self.is_baby(id);
        if kind >= 3 && !tame {
            return false;
        }
        let max_health = MobKind::Horse.max_health(data);
        if !baby
            && tame
            && (player.sneaking()
                || item == Some(329) && !saddled(data)
                || kind == 0 && matches!(item, Some(417..=419)))
        {
            return self.request_horse_window(id);
        }
        let food = match item {
            Some(296) => Some((2., 20, 3)),
            Some(353) => Some((1., 30, 3)),
            Some(170) => Some((20., 180, 0)),
            Some(260) => Some((3., 60, 3)),
            Some(391) => Some((3., 60, 3)),
            Some(396) => Some((4., 60, 5)),
            Some(322) => Some((10., 240, 10)),
            _ => None,
        };
        if let Some((heal, growth, increase)) = food.filter(|_| kind < 3) {
            let b = self.mob_brains.entry(id).or_default();
            let breed = tame && b.growing_age == 0 && matches!(item, Some(396 | 322));
            if hp < max_health || baby || breed || !tame && increase > 0 {
                let entered_love = breed && b.love == 0;
                let grew_foal = b.growing_age < 0;
                if breed {
                    b.love = 600;
                }
                if b.growing_age < 0 {
                    b.growing_age = (b.growing_age + growth * 20).min(0);
                }
                self.mobs[index].health = (hp + heal).min(max_health);
                let temper = (species_ai::number(data, "Temper") + increase).min(100);
                self.command_entities
                    .mobs
                    .entry(id)
                    .or_default()
                    .extra
                    .insert("Temper".into(), Tag::Int(temper));
                if self.mode == Mode::Survival {
                    self.inventory.take_selected(1);
                }
                let anim = self.horse_anims.entry(id).or_default();
                anim.mouth = 1.0;
                anim.tail_ticks = 8;
                out.world_sounds.push(("random.eat", pos.as_dvec3(), 1., 1.));
                if entered_love {
                    self.emit_horse_particles(pos, 1.4, 1.6, 34, 7);
                } else if grew_foal {
                    self.emit_horse_particles(pos, 0.7, 0.8, 21, 5);
                }
                return true;
            }
        }
        if !baby
            && tame
            && matches!(kind, 1 | 2)
            && item == Some(54)
            && species_ai::number(data, "ChestedHorse") == 0
        {
            self.command_entities
                .mobs
                .entry(id)
                .or_default()
                .extra
                .insert("ChestedHorse".into(), Tag::Byte(1));
            if self.mode == Mode::Survival {
                self.inventory.take_selected(1);
            }
            out.world_sounds
                .push(("mob.chicken.plop", pos.as_dvec3(), 1., 1.));
            return true;
        }
        if !tame && (item.is_some() || player.sneaking()) {
            let anim = self.horse_anims.entry(id).or_default();
            anim.stand_ticks = 20;
            anim.head_lean = 0.0;
            anim.rearing = (anim.rearing + (1.0 - anim.rearing) * 0.4 + 0.05).min(1.0);
            out.world_sounds
                .push(("mob.horse.angry", pos.as_dvec3(), 1., 1.));
            return true;
        }
        if baby || player.sneaking() || self.ridden_horse.is_some() {
            return false;
        }
        self.clear_local_mounts(player);
        self.ridden_horse = Some(id);
        self.horse_rider = Default::default();
        if let Some(anim) = self.horse_anims.get_mut(&id) {
            anim.stand_ticks = 0;
            anim.head_lean = 0.0;
        }
        player.riding = false;
        player.set_rotation_degrees(yaw.to_degrees() - 180., 0.);
        self.fall_distance = 0.;
        self.seat_rider(player);
        true
    }
    pub(super) fn update_mount(&mut self, world: &World, player: &mut Player, out: &mut Outcome) {
        if let Some(pig_id) = self.ridden_pig {
            if !self
                .mobs
                .iter()
                .any(|m| m.id == pig_id && m.kind == MobKind::Pig && m.health > 0.)
            {
                self.ridden_pig = None;
            }
        }
        if let Some(horse_id) = self.ridden_horse {
            if !self
                .mobs
                .iter()
                .any(|m| m.id == horse_id && m.kind == MobKind::Horse && m.health > 0.)
            {
                self.ridden_horse = None;
                self.horse_rider = Default::default();
            }
        }
        if self.is_riding_local() && player.riding {
            player.begin_mounted_tick();
        } else if !self.is_riding_local() && player.riding {
            player.riding = false;
            player.third_person.riding = false;
            player.third_person.mount_yaw = None;
        }
        self.horse_anims.retain(|hid, _| {
            self.mobs
                .iter()
                .any(|m| m.id == *hid && m.kind == MobKind::Horse && m.health > 0.)
        });
        let mut anim_rng = JavaRandom::from_state(self.particle_random[2]);
        for i in 0..self.mobs.len() {
            if self.mobs[i].kind != MobKind::Horse || self.mobs[i].health <= 0. {
                continue;
            }
            let hid = self.mobs[i].id;
            let ridden = self.ridden_horse == Some(hid);
            let is_saddled = saddled(self.command_entities.mobs.get(&hid));
            let grounded = self.mob_brains.get(&hid).is_none_or(|b| b.grounded);
            if let Some(brain) = self.mob_brains.get_mut(&hid) {
                if let Some(loc) = brain.movement.as_mut() {
                    loc.body.step_height = Some(if ridden { 1.0 } else { 0.5 });
                }
            }
            let anim = self.horse_anims.entry(hid).or_default();
            anim.prev_rearing = anim.rearing;
            anim.prev_head_lean = anim.head_lean;
            anim.prev_mouth = anim.mouth;
            anim.tail_ticks = anim.tail_ticks.saturating_sub(1);
            anim.run_crazy_ticks = anim.run_crazy_ticks.saturating_sub(1);
            if !ridden && anim.tail_ticks == 0 && anim_rng.next_int(200) == 0 {
                anim.tail_ticks = 8;
            }
            if !ridden && grounded && anim.stand_ticks == 0 {
                if self.mobs[i].eat_ticks == 0 && anim_rng.next_int(300) == 0 {
                    self.mobs[i].eat_ticks = 50;
                } else if self.mobs[i].eat_ticks == 0 && anim_rng.next_int(1000) == 0 {
                    anim.stand_ticks = 20;
                }
            }
            if ridden || anim.stand_ticks > 0 {
                self.mobs[i].eat_ticks = 0;
            } else if self.mobs[i].eat_ticks > 0 {
                self.mobs[i].eat_ticks -= 1;
            }
            let grazing = !ridden && anim.stand_ticks == 0 && self.mobs[i].eat_ticks > 0;
            if grazing {
                anim.head_lean = (anim.head_lean + (1.0 - anim.head_lean) * 0.4 + 0.05).min(1.0);
                if self.mobs[i].eat_ticks % 10 < 5 {
                    anim.mouth = (anim.mouth + (1.0 - anim.mouth) * 0.7 + 0.05).min(1.0);
                } else {
                    anim.mouth = (anim.mouth - 0.15).max(0.0);
                }
            } else {
                anim.head_lean = (anim.head_lean - anim.head_lean * 0.4 - 0.05).max(0.0);
                anim.mouth = (anim.mouth - 1.0 / 30.0).max(0.0);
            }
            if anim.stand_ticks > 0 {
                anim.stand_ticks -= 1;
                anim.rearing = (anim.rearing + (1.0 - anim.rearing) * 0.4 + 0.05).min(1.0);
            } else {
                anim.rearing = (anim.rearing
                    + (0.8 * anim.rearing * anim.rearing * anim.rearing - anim.rearing) * 0.6
                    - 0.05)
                    .max(0.0);
            }
            let horiz_sq = (self.mobs[i].position - self.mobs[i].previous)
                .with_y(0.)
                .length_squared();
            if ridden && is_saddled && grounded && horiz_sq > 0.0025 {
                anim.gallop_ticks = anim.gallop_ticks.wrapping_add(1);
            } else if horiz_sq <= 0.0004 {
                anim.gallop_ticks = 0;
            }
        }
        self.particle_random[2] = anim_rng.state();
        let Some(id) = self.ridden_horse else {
            return;
        };
        let Some(mob_index) = self
            .mobs
            .iter()
            .position(|m| m.id == id && m.kind == MobKind::Horse && m.health > 0.)
        else {
            self.ridden_horse = None;
            self.horse_rider = Default::default();
            player.riding = false;
            player.third_person.riding = false;
            player.third_person.mount_yaw = None;
            return;
        };
        let position = self.mobs[mob_index].position;
        let mut dismount = player.sneaking() || player.is_spectator() || self.health <= 0.;
        let (no_ai, mut tame, is_saddled, temper) = {
            let data = self.command_entities.mobs.entry(id).or_default();
            (
                data.no_ai,
                species_ai::number(Some(data), "Tame") != 0,
                saddled(Some(data)),
                species_ai::number(Some(data), "Temper"),
            )
        };
        if !dismount && !no_ai && !tame {
            self.horse_rider.ticks = self.horse_rider.ticks.saturating_add(1);
            let mut crazy_rng = JavaRandom::from_state(self.particle_random[2]);
            let need_burst = self
                .mob_brains
                .get(&id)
                .and_then(|b| b.wander_target)
                .is_none_or(|t| position.distance_squared(t) < 2.25)
                || self.mobs[mob_index].turn_ticks <= 36;
            if need_burst {
                let angle = crazy_rng.next_float() * std::f32::consts::TAU;
                let dist = 4.0 + crazy_rng.next_float() * 6.0;
                self.mob_brains.entry(id).or_default().wander_target =
                    Some(position + Vec3::new(angle.sin(), 0.0, -angle.cos()) * dist);
                self.mobs[mob_index].turn_ticks = 60 + crazy_rng.next_int(40) as u32;
            }
            self.particle_random[2] = crazy_rng.state();
            if let Some(anim) = self.horse_anims.get_mut(&id) {
                anim.run_crazy_ticks = 10;
            }
            if random(&mut self.rng) % 50 == 0 {
                if (random(&mut self.rng) % 100) < temper.max(0) as u64 {
                    let owner = mob_pets::owner_id(&self.local_name);
                    let data = self.command_entities.mobs.entry(id).or_default();
                    data.extra.insert("Tame".into(), Tag::Byte(1));
                    data.extra.insert("OwnerUUID".into(), Tag::String(owner));
                    tame = true;
                    if let Some(brain) = self.mob_brains.get_mut(&id) {
                        brain.wander_target = None;
                        brain.navigator.clear();
                    }
                    self.mobs[mob_index].turn_ticks = 10;
                    if let Some(anim) = self.horse_anims.get_mut(&id) {
                        anim.run_crazy_ticks = 0;
                    }
                    self.emit_horse_particles(position, 1.4, 1.6, 34, 7);
                    out.world_sounds
                        .push(("mob.horse.idle", position.as_dvec3(), 0.5, 1.));
                } else {
                    let data = self.command_entities.mobs.entry(id).or_default();
                    data.extra
                        .insert("Temper".into(), Tag::Int((temper + 5).min(100)));
                    let anim = self.horse_anims.entry(id).or_default();
                    anim.stand_ticks = 20;
                    anim.run_crazy_ticks = 0;
                    anim.head_lean = 0.0;
                    anim.rearing = (anim.rearing + (1.0 - anim.rearing) * 0.4 + 0.05).min(1.0);
                    out.world_sounds
                        .push(("mob.horse.angry", position.as_dvec3(), 1., 1.));
                    self.emit_horse_particles(position, 1.4, 1.6, 11, 7);
                    dismount = true;
                }
            }
        }
        if !dismount && tame && !is_saddled {
            if let Some(brain) = self.mob_brains.get_mut(&id) {
                brain.wander_target = None;
                brain.navigator.clear();
            }
            self.mobs[mob_index].turn_ticks = 10;
        }
        if dismount {
            let mut feet = position.as_dvec3() + DVec3::Y * 1.0;
            let mut found = false;
            'place: for y in [0., 1., -1.] {
                for (x, z) in [
                    (1., 0.),
                    (-1., 0.),
                    (0., 1.),
                    (0., -1.),
                    (1., 1.),
                    (-1., 1.),
                    (1., -1.),
                    (-1., -1.),
                ] {
                    let candidate = position.as_dvec3() + DVec3::new(x, y, z);
                    let bounds = Aabb::player(candidate);
                    if collision::colliders(world, bounds).is_empty()
                        && !collision::colliders(world, bounds.offset(-DVec3::Y * 0.1)).is_empty()
                    {
                        feet = candidate;
                        found = true;
                        break 'place;
                    }
                }
            }
            if !found {
                for y in [0., 1., 1.6] {
                    let candidate = position.as_dvec3() + DVec3::Y * y;
                    if collision::colliders(world, Aabb::player(candidate)).is_empty() {
                        feet = candidate;
                        break;
                    }
                }
            }
            player.riding = false;
            player.third_person.riding = false;
            player.third_person.mount_yaw = None;
            player.set_position_f64(feet.to_array());
            player.set_velocity_f64([0.; 3]);
            self.ridden_horse = None;
            self.horse_rider = Default::default();
        }
    }
    pub(super) fn seat_rider(&mut self, player: &mut Player) {
        if (self.ridden_minecart.is_some() || self.ridden_boat.is_some())
            && self.seat_vehicle_rider(player)
        {
            return;
        }
        let Some(id) = self.ridden_horse else {
            if let Some(pig_id) = self.ridden_pig {
                if let Some(m) = self
                    .mobs
                    .iter()
                    .find(|m| m.id == pig_id && m.kind == MobKind::Pig && m.health > 0.)
                {
                    let seat = DVec3::Y * (f64::from(0.9f32) * 0.75 - 0.35);
                    player.set_position_f64((m.position.as_dvec3() + seat).to_array());
                    player.set_velocity_f64([0.; 3]);
                    player.jumped = false;
                    player.riding = true;
                    player.third_person.riding = true;
                    let yaw = m.yaw.to_degrees() - 180.;
                    player.third_person.mount_yaw =
                        Some([player.third_person.mount_yaw.map_or(yaw, |a| a[1]), yaw]);
                } else {
                    self.ridden_pig = None;
                    player.riding = false;
                    player.third_person.riding = false;
                    player.third_person.mount_yaw = None;
                }
            }
            return;
        };
        let Some(m) = self.mobs.iter().find(|m| m.id == id && m.health > 0.) else {
            self.ridden_horse = None;
            self.horse_rider = Default::default();
            player.riding = false;
            player.third_person.riding = false;
            player.third_person.mount_yaw = None;
            return;
        };
        let rear = self.horse_anims.get(&id).map_or(0., |a| a.prev_rearing);
        let yaw = m.yaw.to_degrees() - 180.;
        let angle = yaw * std::f32::consts::PI / 180.;
        let forward = 0.7 * rear;
        let seat = DVec3::new(
            f64::from(forward * crate::vanilla::math::sin(angle)),
            f64::from(1.6f32) * 0.75 - 0.35 + f64::from(0.15 * rear),
            -f64::from(forward * crate::vanilla::math::cos(angle)),
        );
        player.set_position_f64((m.position.as_dvec3() + seat).to_array());
        player.set_velocity_f64([0.; 3]);
        player.jumped = false;
        player.riding = true;
        player.third_person.riding = true;
        player.third_person.mount_yaw =
            Some([player.third_person.mount_yaw.map_or(yaw, |a| a[1]), yaw]);
        if std::mem::take(&mut self.horse_rider.jumped_this_tick) {
            let anim = self.horse_anims.entry(id).or_default();
            anim.stand_ticks = 20;
            anim.head_lean = 0.0;
            anim.rearing = (anim.rearing + (1.0 - anim.rearing) * 0.4 + 0.05).min(1.0);
        }
    }
}

fn attribute(data: Option<&MobData>, name: &str, default: f64) -> f64 {
    let Some(Tag::List { values, .. }) = data.and_then(|d| d.extra.get("Attributes")) else {
        return default;
    };
    values
        .iter()
        .filter_map(item_nbt::compound)
        .find_map(|a| {
            if !matches!(a.get("Name"), Some(Tag::String(n)) if n==name) {
                return None;
            }
            match a.get("Base") {
                Some(Tag::Double(n)) if n.is_finite() => Some(*n),
                _ => None,
            }
        })
        .unwrap_or(default)
}
pub(super) fn drive(
    world: &World,
    m: &mut Mob,
    brain: &mut MobBrain,
    data: Option<&MobData>,
    player: &Player,
    rider: &mut crate::remote::Rider,
) {
    let (width, height) = m.kind.size();
    let movement = brain.movement.get_or_insert_with(|| {
        Box::new(ai::locomotion::Locomotion::new(
            m.position.as_dvec3(),
            width,
            height,
            m.yaw.to_degrees() - 180.,
            brain.grounded,
        ))
    });
    movement.reconcile(world, m, (width, height));
    let body = &mut movement.body;
    body.previous = body.position;
    body.step_height = Some(1.);
    let keys = |p, n| f32::from(player.keys.contains(&p)) - f32::from(player.keys.contains(&n));
    let mut forward = keys(KeyCode::KeyW, KeyCode::KeyS) * 0.98;
    if forward <= 0. {
        forward *= 0.25;
    }
    let strafe = keys(KeyCode::KeyA, KeyCode::KeyD) * 0.98 * 0.5;
    let yaw = player.java_rotation()[0];
    if let Some(charge) = rider
        .jump(true, player.keys.contains(&KeyCode::Space))
        .filter(|&c| c > 0 && body.grounded)
    {
        rider.jumped_this_tick = true;
        let power = if charge >= 90 {
            1.
        } else {
            0.4 + 0.4 * charge as f32 / 90.
        };
        body.velocity.y = attribute(data, "horse.jumpStrength", 0.7) * f64::from(power);
        if let Some(effect) = data.and_then(|d| d.effects.get(8)) {
            body.velocity.y += f64::from((effect.amplifier + 1) as f32 * 0.1);
        }
        if forward > 0. {
            let angle = yaw.to_radians();
            body.velocity.x -= f64::from(crate::vanilla::math::sin(angle) * 0.4 * power);
            body.velocity.z += f64::from(crate::vanilla::math::cos(angle) * 0.4 * power);
        }
    }
    let (water, flow) = movement::water_contact(world, body.bounds);
    if let Some(flow) = flow {
        body.velocity += flow * 0.014;
    }
    let lava = movement::touches_liquid(
        world,
        body.bounds.expand(DVec3::new(-0.1, -0.4, -0.1)),
        10,
        11,
    );
    body.in_water = water;
    body.in_lava = lava;
    let mut speed_attr = attribute(data, "generic.movementSpeed", 0.225);
    if let Some(effect) = data.and_then(|d| d.effects.get(1)) {
        speed_attr *= 1.0 + 0.20000000298023224 * f64::from(effect.amplifier.wrapping_add(1));
    }
    if let Some(effect) = data.and_then(|d| d.effects.get(2)) {
        speed_attr = (speed_attr
            * (1.0 - 0.15000000596046448 * f64::from(effect.amplifier.wrapping_add(1))))
        .max(0.0);
    }
    let speed = speed_attr.clamp(0., 1024.) as f32;
    body.travel(
        world,
        Travel {
            strafe,
            forward,
            yaw,
            sneak: false,
            depth_strider: 0,
            water,
            lava,
            land_speed: speed,
            air_speed: speed * 0.1,
            ladder_override: None,
            server: true,
        },
    );
    m.velocity = body.velocity.as_vec3();
    m.position = body.position.as_vec3();
    brain.grounded = body.grounded;
    brain
        .gait
        .advance(m.yaw, (m.position - m.previous).with_y(0.).length());
    m.yaw = (yaw + 180.).to_radians();
    movement.control.yaw = yaw;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{ClickAction, SlotRef};
    #[test]
    #[ignore = "uses the local game resources for integrated right-click and world ticks"]
    fn horse_right_click_tames_opens_saddle_window_steers_jumps_and_dismounts() {
        let mut world = super::super::tests::flat_world();
        let mut sim = Simulation::new(Mode::Survival, 189);
        sim.mob_spawning = false;
        let mut player = Player::new(Vec3::new(8.5, 1., 5.5));
        player.set_rotation_degrees(0., 15.);
        let id =
            sim.spawn_registered_raw(MobKind::Horse, Vec3::new(8.5, 1., 8.), std::f32::consts::PI);
        let resources = crate::validation::resources();
        let settings = Settings::default();
        let tick = |world: &mut World, sim: &mut Simulation, player: &mut Player, right: bool| {
            sim.tick(world, player, &resources, &settings, false, right);
        };
        sim.command_entities
            .mobs
            .entry(id)
            .or_default()
            .extra
            .insert("Temper".into(), Tag::Int(100));
        tick(&mut world, &mut sim, &mut player, true);
        assert_eq!(sim.ridden_horse, Some(id));
        for _ in 0..1000 {
            if species_ai::number(sim.command_entities.mobs.get(&id), "Tame") != 0 {
                break;
            }
            tick(&mut world, &mut sim, &mut player, false);
        }
        assert_eq!(
            species_ai::number(sim.command_entities.mobs.get(&id), "Tame"),
            1
        );
        assert!(sim.request_horse_window(id));
        sim.inventory.slots[0] = Some(Stack::new(329, 1));
        sim.horse_window_click(SlotRef::Player(0), ClickAction::QuickMove);
        assert!(saddled(sim.command_entities.mobs.get(&id)));
        assert!(sim.inventory.slots[0].is_none());
        sim.horse_window = None;
        let old = sim.mobs[0].position;
        // Reset into the loaded flat area following the untamed horse's random walk.
        sim.mobs[0].position = Vec3::new(4., 1., 4.);
        sim.mobs[0].velocity = Vec3::ZERO;
        player.set_rotation_degrees(0., 0.);
        player.keys.insert(KeyCode::KeyW);
        player.keys.insert(KeyCode::Space);
        for _ in 0..10 {
            tick(&mut world, &mut sim, &mut player, false);
        }
        player.keys.remove(&KeyCode::Space);
        tick(&mut world, &mut sim, &mut player, false);
        assert!(
            sim.mobs[0].position.z > 4.3 && sim.mobs[0].position.y > 1.2,
            "{old:?} -> {:?}",
            sim.mobs[0].position
        );
        assert!((player.position.y - sim.mobs[0].position.y - 0.85).abs() < 1e-5);
        assert!(player.third_person.riding);
        player.keys.clear();
        player.keys.insert(KeyCode::ShiftLeft);
        tick(&mut world, &mut sim, &mut player, false);
        assert_eq!(sim.ridden_horse, None);
        assert!(!player.third_person.riding);
        let reload: Simulation =
            serde_json::from_slice(&serde_json::to_vec(&sim).unwrap()).unwrap();
        assert!(saddled(reload.command_entities.mobs.get(&id)));
    }
    #[test]
    fn horse_drive_wsad_axes_and_rearing_seat_offset_match_ferrum_yaw() {
        let world = super::super::tests::flat_world();
        for (key, check) in [
            (KeyCode::KeyW, (0, -1i32)),
            (KeyCode::KeyS, (0, 1)),
            (KeyCode::KeyD, (1, 0)),
            (KeyCode::KeyA, (-1, 0)),
        ] {
            let mut sim = Simulation::new(Mode::Survival, 189);
            let id = sim.spawn_registered_raw(MobKind::Horse, Vec3::new(8.5, 1., 8.5), 0.);
            let mut player = Player::new(Vec3::new(8.5, 1.85, 8.5));
            player.yaw = 0.; // facing -Z, right +X
            player.keys.insert(key);
            let mut rider = crate::remote::Rider::default();
            let data = sim.command_entities.mobs.get(&id);
            let brain = sim.mob_brains.entry(id).or_default();
            brain.grounded = true;
            drive(&world, &mut sim.mobs[0], brain, data, &player, &mut rider);
            let delta = sim.mobs[0].position - Vec3::new(8.5, 1., 8.5);
            if check.0 != 0 {
                assert!(
                    delta.x * check.0 as f32 > 0.01 && delta.z.abs() < 1e-4,
                    "{key:?} delta={delta:?}"
                );
            } else {
                assert!(
                    delta.z * check.1 as f32 > 0.005 && delta.x.abs() < 1e-4,
                    "{key:?} delta={delta:?}"
                );
            }
        }
    }
}
