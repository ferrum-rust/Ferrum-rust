//! Applies original block-click callbacks to the single-player world or a predicted remote world.
use super::{JavaRandom, Mode, Outcome, Particle, Simulation, World};
use glam::IVec3;

pub(crate) fn actual_dig_state(world: &World, pos: IVec3, state: u16) -> u16 {
    if state >> 4 == 175 && (state & 8) != 0 {
        let below = world.get(pos - IVec3::Y);
        if below >> 4 == 175 && (below & 8) == 0 {
            return (175 << 4) | 8 | (below & 7);
        }
    }
    state
}

impl Simulation {
    pub fn block_destroy_effects(&mut self, pos: IVec3, state: u16) {
        if state >> 4 == 0 {
            return;
        }
        let mut random = JavaRandom::from_state(self.particle_random[2]);
        for x in 0..4 {
            for y in 0..4 {
                for z in 0..4 {
                    let offset = (IVec3::new(x, y, z).as_dvec3() + glam::DVec3::splat(0.5)) / 4.;
                    self.particles.push(Particle::digging(
                        pos.as_dvec3() + offset,
                        offset - glam::DVec3::splat(0.5),
                        state,
                        pos,
                        false,
                        &mut random,
                    ));
                }
            }
        }
        self.particle_random[2] = random.state();
    }
    pub fn block_hit_effect(&mut self, world: &World, pos: IVec3, face: u8) {
        let state = world.get(pos);
        let dig_state = actual_dig_state(world, pos, state);
        let mut random = JavaRandom::from_state(self.particle_random[1]);
        let position =
            crate::vanilla::special_click::hit_origin(world, pos, state, face, &mut random);
        self.particle_random[1] = random.state();
        if let Some(position) = position {
            let mut motion = JavaRandom::from_state(self.particle_random[2]);
            self.particles.push(Particle::digging(
                position,
                glam::DVec3::ZERO,
                dig_state,
                pos,
                true,
                &mut motion,
            ));
            self.particle_random[2] = motion.state();
        }
    }
    fn click_particle(&mut self, effect: crate::vanilla::special_click::Effect) {
        if let crate::vanilla::special_click::Effect::Particle(id, p, v) = effect {
            let mut random = JavaRandom::from_state(self.particle_random[2]);
            if let Some(particle) = Particle::special(id, p, v, &mut random) {
                self.particles.push(particle);
            }
            self.particle_random[2] = random.state();
        }
    }
    pub fn click_special_block(
        &mut self,
        world: &mut World,
        pos: IVec3,
        remote: bool,
        out: &mut Outcome,
    ) {
        use crate::vanilla::special_click::{self, Effect};
        if !matches!(world.get(pos) >> 4, 25 | 73 | 74 | 92 | 122) {
            return;
        }
        let note = self.note_pitch(world, pos);
        let can_eat = self.food < 20 && self.mode == Mode::Survival;
        let dragon_egg_sp = !remote && world.get(pos) >> 4 == 122;
        let mut client_random = if dragon_egg_sp {
            self.block_ticks.world_random()
        } else {
            JavaRandom::from_state(self.particle_random[0])
        };
        let client_effects =
            special_click::click(world, pos, true, can_eat, note, &mut client_random);
        if !dragon_egg_sp {
            self.particle_random[0] = client_random.state();
        }
        for &effect in &client_effects {
            self.click_particle(effect);
        }
        let effects = if remote {
            client_effects
        } else {
            let mut server_random = self.block_ticks.world_random();
            let effects =
                special_click::click(world, pos, false, can_eat, note, &mut server_random);
            self.block_ticks.set_world_random(&server_random);
            effects
        };
        for effect in effects {
            match effect {
                Effect::EatCake => {
                    let mut food = self.food_state();
                    food.add_stats(2, 0.1);
                    self.set_food_state(food);
                }
                Effect::Set(p, state, flags) => {
                    // An egg's target may cross the streaming boundary. Keep the source until the destination is writable.
                    if flags == 2 && state >> 4 == 122 && !world.contains(p.as_vec3()) {
                        break;
                    }
                    if world.set(p, state) {
                        if remote {
                            world.edits.remove(&p.to_array());
                        } else if state >> 4 == 122 {
                            self.block_ticks.schedule_block_update(p, 122, 2, 0);
                        }
                        out.changed.push(p);
                    }
                }
                Effect::Note(inst, note) => {
                    self.queue_note(pos, inst, note);
                }
                Effect::Particle(..) => {}
            }
        }
    }
    pub fn tick_particles(&mut self, world: &World) {
        self.particles.retain_mut(|particle| particle.tick(world));
    }
}

impl Simulation {
    fn note_tag_pitch(tag: Option<&crate::vanilla::drops::DropNbt>) -> u8 {
        use crate::vanilla::item_nbt;
        let fields = tag.and_then(item_nbt::compound);
        (item_nbt::number(fields.and_then(|fields| fields.get("note"))) as i8).clamp(0, 24) as u8
    }
    fn note_pitch(&self, world: &World, pos: IVec3) -> u8 {
        self.note_pitches
            .get(&super::container_key(pos))
            .copied()
            .unwrap_or_else(|| Self::note_tag_pitch(world.tile(pos)))
            .min(24)
    }
    pub(super) fn placed_note(&mut self, pos: IVec3, stack: &crate::inventory::Stack) {
        use crate::vanilla::item_nbt;
        self.forget_note(pos);
        let tag = stack
            .nbt
            .as_deref()
            .and_then(item_nbt::compound)
            .and_then(|fields| fields.get("BlockEntityTag"));
        self.note_pitches
            .insert(super::container_key(pos), Self::note_tag_pitch(tag));
    }
    pub(super) fn forget_note(&mut self, pos: IVec3) {
        if !self.note_pitches.is_empty() {
            self.note_pitches.remove(&super::container_key(pos));
        }
        self.powered_notes.remove(&pos);
    }
    pub(super) fn note_snapshot(
        &self,
        world: &World,
        pos: IVec3,
    ) -> Option<crate::vanilla::drops::DropNbt> {
        use crate::vanilla::{drops::DropNbt, middle_pick};
        if world.get(pos) >> 4 != 25 {
            return None;
        }
        let pitch = *self.note_pitches.get(&super::container_key(pos))?;
        let DropNbt::Compound(mut tag) =
            middle_pick::tile_snapshot(25, pos, world.tile(pos), None)?
        else {
            return None;
        };
        tag.insert("note".into(), DropNbt::Byte(pitch as i8));
        Some(DropNbt::Compound(tag))
    }
    pub(crate) fn queue_note(&mut self, p: IVec3, inst: u8, note: u8) {
        let event = (p, inst, note);
        if !self.note_events.contains(&event) {
            self.note_events.push(event);
        }
    }
    fn trigger_note(&mut self, world: &World, p: IVec3, pitch: u8) {
        if let Some((instrument, pitch)) = crate::vanilla::notes::trigger(world, p, pitch) {
            self.queue_note(p, instrument, pitch);
        }
    }
    pub(super) fn tune_note(&mut self, world: &World, p: IVec3) -> bool {
        if world.get(p) >> 4 != 25 {
            return false;
        }
        let pitch = crate::vanilla::notes::tune(self.note_pitch(world, p));
        self.note_pitches.insert(super::container_key(p), pitch);
        self.trigger_note(world, p, pitch);
        self.hand_swing
            .start(crate::first_person::swing_duration(&self.effects));
        true
    }
    pub(super) fn note_neighbor(&mut self, world: &World, p: IVec3, powered: bool) {
        if world.get(p) >> 4 != 25 {
            return;
        }
        let mut was = self.powered_notes.contains(&p);
        if crate::vanilla::notes::rising_edge(&mut was, powered) {
            self.trigger_note(world, p, self.note_pitch(world, p));
        }
        if powered {
            self.powered_notes.insert(p);
        } else {
            self.powered_notes.remove(&p);
        }
    }
    pub(crate) fn flush_remote_notes(&mut self,world:&World,out:&mut Outcome){self.flush_note_events(world,out);}
    pub(super) fn flush_note_events(&mut self, world: &World, out: &mut Outcome) {
        for (p, instrument, pitch) in std::mem::take(&mut self.note_events) {
            if world.get(p) >> 4 == 25 {
                let (name, vol, note_pitch) =
                    crate::vanilla::special_click::note_sound(instrument, pitch);
                out.world_sounds.push((
                    name,
                    p.as_dvec3() + glam::DVec3::splat(0.5),
                    vol,
                    note_pitch,
                ));
                self.click_particle(crate::vanilla::special_click::note_particle(p, pitch));
            }
        }
    }
}
