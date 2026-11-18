use super::*;
use crate::vanilla::drops::DropNbt as Tag;

pub(super) fn owner_id(name: &str) -> String {
    use std::fmt::Write;
    let mut text = String::with_capacity(36);
    for (i, byte) in crate::ui::offline_uuid(name).iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            text.push('-');
        }
        write!(text, "{byte:02x}").unwrap();
    }
    text
}

pub(super) fn owned_by(data: Option<&command_entity_state::MobData>, owner: &str) -> bool {
    matches!(data.and_then(|d| d.extra.get("OwnerUUID")), Some(Tag::String(id)) if id.eq_ignore_ascii_case(owner))
}

impl Simulation {
    pub(super) fn spawn_offspring(
        &mut self,
        kind: MobKind,
        position: Vec3,
        first: u64,
        second: u64,
    ) {
        let child = self.spawn_registered_raw(kind, position, 0.);
        self.mob_brains.entry(child).or_default().growing_age = -24000;
        let a = self.command_entities.mobs.get(&first);
        let b = self.command_entities.mobs.get(&second);
        let mut extra = std::collections::BTreeMap::new();
        match kind {
            MobKind::Wolf | MobKind::Ocelot => {
                for key in ["OwnerUUID", "CatType"] {
                    if let Some(value) = a.and_then(|d| d.extra.get(key)) {
                        extra.insert(key.to_owned(), value.clone());
                    }
                }
            }
            MobKind::Rabbit => {
                let parent = if random(&mut self.rng) & 1 == 0 { a } else { b };
                let coat = species_ai::number(parent, "RabbitType");
                extra.insert("RabbitType".into(), Tag::Int(coat));
            }
            MobKind::Sheep => {
                let parents = [a, b].map(|p| {
                    let mut dye = Stack::new(351, 1);
                    dye.meta = 15 - (species_ai::number(p, "Color") & 15) as u16;
                    dye
                });
                let mixed = crate::vanilla::crafting::recipe(
                    &[
                        Some(parents[0].clone()),
                        Some(parents[1].clone()),
                        None,
                        None,
                    ],
                    2,
                );
                let dye = mixed.filter(|recipe| recipe.output.id == 351).map_or_else(
                    || parents[(random(&mut self.rng) & 1) as usize].meta,
                    |recipe| recipe.output.meta,
                );
                extra.insert("Color".into(), Tag::Byte((15 - dye) as i8));
            }
            MobKind::Horse => {
                let types = [a, b].map(|p| species_ai::number(p, "Type"));
                let ty = if types[0] == types[1] { types[0] } else { 2 };
                extra.insert("Type".into(), Tag::Int(ty));
                if ty == 0 {
                    let variants = [a, b].map(|p| species_ai::number(p, "Variant"));
                    let coat = match random(&mut self.rng) % 9 {
                        0..=3 => variants[0] & 255,
                        4..=7 => variants[1] & 255,
                        _ => (random(&mut self.rng) % 7) as i32,
                    };
                    let markings = match random(&mut self.rng) % 5 {
                        0..=1 => variants[0] & 0xff00,
                        2..=3 => variants[1] & 0xff00,
                        _ => ((random(&mut self.rng) % 5) as i32) << 8,
                    };
                    extra.insert("Variant".into(), Tag::Int(coat | markings));
                }
                let rolled = 15 + random(&mut self.rng) % 8 + random(&mut self.rng) % 9;
                let hp =
                    (kind.max_health(a) as f64 + kind.max_health(b) as f64 + rolled as f64) / 3.;
                extra.insert(
                    "Attributes".into(),
                    Tag::List {
                        element: 10,
                        values: vec![Tag::Compound(
                            [
                                ("Name".into(), Tag::String("generic.maxHealth".into())),
                                ("Base".into(), Tag::Double(hp)),
                            ]
                            .into(),
                        )],
                    },
                );
            }
            _ => {}
        }
        if !extra.is_empty() {
            let data = self.command_entities.mobs.entry(child).or_default();
            data.extra = extra;
            self.mobs.last_mut().unwrap().health = kind.max_health(Some(data));
        }
    }
    pub(super) fn emit_mob_particles(
        &mut self,
        position: Vec3,
        size: (f32, f32),
        id: u8,
        count: usize,
    ) {
        let mut rng = JavaRandom::from_state(self.particle_random[1]);
        let (width, height) = size;
        for _ in 0..count {
            let vel = glam::DVec3::new(
                (rng.next_double() * 2. - 1.) * 0.02,
                (rng.next_double() * 2. - 1.) * 0.02,
                (rng.next_double() * 2. - 1.) * 0.02,
            );
            let at = position.as_dvec3()
                + glam::DVec3::new(
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

    pub(super) fn swing_arm(&mut self) {
        self.hand_swing
            .start(crate::first_person::swing_duration(&self.effects));
    }

    fn fill_held_container(&mut self, player: &Player, filled: Stack) {
        if self.mode == Mode::Creative {
            let already_has = self
                .inventory
                .slots
                .iter()
                .flatten()
                .any(|s| s.id == filled.id && s.meta == filled.meta);
            if !already_has {
                let _ = self.inventory.add(filled);
            }
            return;
        }
        let count = self.inventory.held().map_or(0, |s| s.count);
        if count <= 1 {
            self.inventory.slots[self.inventory.selected] = Some(filled);
        } else {
            self.inventory.take_selected(1);
            if let Some(rest) = self.inventory.add(filled) {
                self.throw_local_stack(player, rest, ThrowSource::Hotbar);
            }
        }
    }

    pub(super) fn interact_pet_or_mooshroom(
        &mut self,
        index: usize,
        player: &Player,
        out: &mut Outcome,
    ) -> bool {
        let mut p = player.clone();
        self.interact_pet_or_mooshroom_mut(index, &mut p, out)
    }

    pub(super) fn interact_pet_or_mooshroom_mut(
        &mut self,
        index: usize,
        player: &mut Player,
        out: &mut Outcome,
    ) -> bool {
        let Some(mob) = self.mobs.get(index) else {
            return false;
        };
        let (kind, id, pos, mob_health, mob_yaw) =
            (mob.kind, mob.id, mob.position, mob.health, mob.yaw);
        if mob_health <= 0. {
            return false;
        }
        let item = self.inventory.held();
        if let Some(s) = item.as_ref().filter(|s| s.id == 421) {
            if let Some(Tag::String(custom_name)) = s
                .nbt
                .as_deref()
                .and_then(crate::vanilla::item_nbt::compound)
                .and_then(|t| t.get("display"))
                .and_then(crate::vanilla::item_nbt::compound)
                .and_then(|d| d.get("Name"))
                .filter(|tag| matches!(tag, Tag::String(text) if !text.is_empty()))
            {
                let name = custom_name.clone();
                let data = self.command_entities.mobs.entry(id).or_default();
                data.name = name.clone();
                data.persistent = true;
                data.extra.insert("CustomName".into(), Tag::String(name));
                data.extra.insert("PersistenceRequired".into(), Tag::Byte(1));
                if self.mode != Mode::Creative {
                    self.inventory.take_selected(1);
                }
                self.swing_arm();
                return true;
            }
            return false;
        }
        if kind == MobKind::Creeper && item.as_ref().is_some_and(|s| s.id == 259) {
            out.world_sounds.push((
                "fire.ignite",
                pos.as_dvec3(),
                1.0,
                unit(&mut self.rng) * 0.4 + 0.8,
            ));
            self.swing_arm();
            self.command_entities
                .mobs
                .entry(id)
                .or_default()
                .extra
                .insert("ignited".into(), Tag::Byte(1));
            if self.mode != Mode::Creative {
                self.wear_slot(self.inventory.selected, 1, false);
            }
            return true;
        }
        if kind == MobKind::Sheep {
            let sheared =
                species_ai::number(self.command_entities.mobs.get(&id), "Sheared") != 0;
            let baby = self.is_baby(id);
            if !baby && !sheared && item.as_ref().is_some_and(|s| s.id == 359) {
                let color =
                    (species_ai::number(self.command_entities.mobs.get(&id), "Color") & 15) as u16;
                self.command_entities
                    .mobs
                    .entry(id)
                    .or_default()
                    .extra
                    .insert("Sheared".into(), Tag::Byte(1));
                let count = 1 + (random(&mut self.rng) % 3) as usize;
                for _ in 0..count {
                    let mut wool = Stack::new(35, 1);
                    wool.meta = color;
                    let vel = Vec3::new(
                        (unit(&mut self.rng) - unit(&mut self.rng)) * 0.05,
                        0.05 + unit(&mut self.rng) * 0.05,
                        (unit(&mut self.rng) - unit(&mut self.rng)) * 0.05,
                    );
                    self.spawn_drop(wool, pos + Vec3::Y * 1.0, vel);
                }
                if self.mode != Mode::Creative {
                    self.wear_slot(self.inventory.selected, 1, false);
                }
                out.world_sounds
                    .push(("mob.sheep.shear", pos.as_dvec3(), 1.0, 1.0));
                self.swing_arm();
                return true;
            }
            if !sheared {
                if let Some(s) = item.as_ref().filter(|s| s.id == 351) {
                    let new_color = (15 - (s.meta & 15)) as i8;
                    let cur_color = (species_ai::number(
                        self.command_entities.mobs.get(&id),
                        "Color",
                    ) & 15) as i8;
                    if cur_color != new_color {
                        self.command_entities
                            .mobs
                            .entry(id)
                            .or_default()
                            .extra
                            .insert("Color".into(), Tag::Byte(new_color));
                        if self.mode != Mode::Creative {
                            self.inventory.take_selected(1);
                        }
                        self.swing_arm();
                        return true;
                    }
                }
            }
            return false;
        }
        if matches!(kind, MobKind::Cow | MobKind::Mooshroom) && !self.is_baby(id) {
            if item.as_ref().is_some_and(|s| s.id == 325) {
                self.fill_held_container(player, Stack::new(335, 1));
                self.swing_arm();
                return true;
            }
            if kind == MobKind::Mooshroom {
                match item.as_ref().map(|s| s.id) {
                    Some(281) => {
                        self.fill_held_container(player, Stack::new(282, 1));
                        self.swing_arm();
                        return true;
                    }
                    Some(359) => {
                        self.emit_mob_particles(pos, MobKind::Mooshroom.size(), 1, 1);
                        self.mobs.remove(index);
                        self.mob_brains.remove(&id);
                        let old = self.command_entities.mobs.remove(&id).unwrap_or_default();
                        let cow = self.spawn_registered(MobKind::Cow, pos, mob_yaw);
                        self.mobs.last_mut().unwrap().health = mob_health;
                        let cow_data = self.command_entities.mobs.entry(cow).or_default();
                        cow_data.name = old.name;
                        cow_data.persistent = old.persistent;
                        for key in ["CustomName", "PersistenceRequired"] {
                            if let Some(val) = old.extra.get(key) {
                                cow_data.extra.insert(key.into(), val.clone());
                            }
                        }
                        for _ in 0..5 {
                            self.spawn_drop(
                                Stack::new(40, 1),
                                pos + Vec3::Y * 1.3,
                                Vec3::Y * 0.2,
                            );
                        }
                        if self.mode != Mode::Creative {
                            self.wear_slot(self.inventory.selected, 1, false);
                        }
                        out.world_sounds
                            .push(("mob.sheep.shear", pos.as_dvec3(), 1., 1.));
                        self.swing_arm();
                        return true;
                    }
                    _ => return false,
                }
            }
            return false;
        }
        if kind == MobKind::Pig && !self.is_baby(id) {
            let saddled =
                species_ai::number(self.command_entities.mobs.get(&id), "Saddle") != 0;
            let item_id = item.as_ref().map_or(0, |s| s.id);
            if !saddled && item_id == 329 {
                self.command_entities
                    .mobs
                    .entry(id)
                    .or_default()
                    .extra
                    .insert("Saddle".into(), Tag::Byte(1));
                out.world_sounds
                    .push(("mob.horse.leather", pos.as_dvec3(), 0.5, 1.0));
                if self.mode != Mode::Creative {
                    self.inventory.take_selected(1);
                }
                self.swing_arm();
                return true;
            }
            if saddled
                && self.ridden_pig != Some(id)
                && !ai::breeding_food(MobKind::Pig, item_id)
                && item_id != 383
            {
                if player.sneaking() {
                    return false;
                }
                self.clear_local_mounts(player);
                self.ridden_pig = Some(id);
                self.pig_boost_ticks = 0;
                self.pig_boost_total = 0;
                self.pig_speed = 0.;
                self.fall_distance = 0.;
                self.seat_rider(player);
                self.swing_arm();
                return true;
            }
            return false;
        }
        if !matches!(kind, MobKind::Wolf | MobKind::Ocelot) {
            return false;
        }
        let data = self.command_entities.mobs.get(&id);
        let item_id = item.as_ref().map_or(0, |s| s.id);
        if !species_ai::tamed(data) {
            let can_tame = {
                let brain = self.mob_brains.entry(id).or_default();
                if kind == MobKind::Wolf {
                    item_id == 352
                        && brain.anger_ticks == 0
                        && species_ai::number(data, "Angry") == 0
                } else {
                    item_id == 349 && pos.distance_squared(player.position) < 9.
                }
            };
            if !can_tame {
                return false;
            }
            if self.mode != Mode::Creative {
                self.inventory.take_selected(1);
            }
            self.swing_arm();
            if random(&mut self.rng) % 3 == 0 {
                let fields = &mut self.command_entities.mobs.entry(id).or_default().extra;
                fields.insert("OwnerUUID".into(), Tag::String(owner_id(&self.local_name)));
                fields.insert("Sitting".into(), Tag::Byte(1));
                let brain = self.mob_brains.entry(id).or_default();
                brain.navigator.clear();
                brain.anger_ticks = 0;
                brain.target_mob = None;
                if kind == MobKind::Wolf {
                    self.mobs[index].health = 20.;
                    fields.insert("Angry".into(), Tag::Byte(0));
                    fields.entry("CollarColor".into()).or_insert(Tag::Byte(1));
                } else {
                    fields.insert(
                        "CatType".into(),
                        Tag::Int(1 + (random(&mut self.rng) % 3) as i32),
                    );
                }
                self.emit_mob_particles(pos, kind.size(), 34, 7);
            } else {
                self.emit_mob_particles(pos, kind.size(), 11, 7);
            }
            return true;
        }
        if kind == MobKind::Wolf {
            if let Some(s) = item
                .as_ref()
                .filter(|s| ai::breeding_food(kind, s.id) && mob_health < 20.)
            {
                if let Some(food) = food::food_values(s.id, s.meta) {
                    self.mobs[index].health = (mob_health + food.nutrition as f32).min(20.);
                    if self.mode != Mode::Creative {
                        self.inventory.take_selected(1);
                    }
                    self.swing_arm();
                    return true;
                }
            }
            if let Some(s) = item.as_ref().filter(|s| s.id == 351) {
                // CollarColor stores vanilla 1.8.9 EnumDyeColor.getDyeDamage() (0=black..14=orange, 1=red default, 15=white).
                let color = (s.meta & 15) as i8;
                let collar = data.and_then(|d| d.extra.get("CollarColor"));
                if color != collar.map_or(1, |v| crate::vanilla::item_nbt::number(Some(v)) as i8) {
                    self.command_entities
                        .mobs
                        .entry(id)
                        .or_default()
                        .extra
                        .insert("CollarColor".into(), Tag::Byte(color));
                    if self.mode != Mode::Creative {
                        self.inventory.take_selected(1);
                    }
                    self.swing_arm();
                    return true;
                }
            }
        }
        if owned_by(data, &owner_id(&self.local_name))
            && !ai::breeding_food(kind, item_id)
            && item_id != 383
        {
            let sitting = species_ai::number(data, "Sitting") == 0;
            self.command_entities
                .mobs
                .entry(id)
                .or_default()
                .extra
                .insert("Sitting".into(), Tag::Byte(i8::from(sitting)));
            let brain = self.mob_brains.entry(id).or_default();
            brain.navigator.clear();
            brain.anger_ticks = 0;
            brain.target_mob = None;
            self.swing_arm();
            return true;
        }
        false
    }
}
