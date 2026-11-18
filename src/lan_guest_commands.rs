use super::*;
use crate::gameplay::{local_commands, xp_orb::motion::Experience};
use crate::vanilla::{potions::Effects, respawn::BedSpawn};
use std::collections::BTreeMap;

/// Player-specific data read and written by the integrated command dispatcher.
/// The world, mobs, command scoreboard and scheduled block updates stay shared.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Profile {
    health: f32,
    #[serde(skip)]
    hurt_camera: crate::gameplay::HurtCamera,
    max_health: f32,
    absorption: f32,
    food: i32,
    saturation: f32,
    experience: u32,
    xp: Experience,
    effects: Effects,
    bed_spawn: Option<BedSpawn>,
    pub(super) statistics: HashMap<String, i32>,
    ender: BTreeMap<i32, Option<Stack>>,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            health: 20.,
            hurt_camera: Default::default(),
            max_health: 20.,
            absorption: 0.,
            food: 20,
            saturation: 5.,
            experience: 0,
            xp: Default::default(),
            effects: Default::default(),
            bed_spawn: None,
            statistics: Default::default(),
            ender: Default::default(),
        }
    }
}
impl Profile {
    pub(super) fn alive(&self) -> bool {
        self.health > 0.
    }
    fn exchange(&mut self, sim: &mut Simulation) {
        use std::mem::swap;
        swap(&mut self.health, &mut sim.health);
        swap(&mut self.hurt_camera, &mut sim.hurt_camera);
        swap(&mut self.max_health, &mut sim.max_health);
        swap(&mut self.absorption, &mut sim.absorption);
        swap(&mut self.food, &mut sim.food);
        swap(&mut self.saturation, &mut sim.saturation);
        swap(&mut self.experience, &mut sim.experience);
        swap(&mut self.xp, &mut sim.xp);
        swap(&mut self.effects, &mut sim.effects);
        swap(&mut self.bed_spawn, &mut sim.bed_spawn);
        swap(&mut self.statistics, &mut sim.statistics);
        swap(&mut self.ender, &mut sim.command_entities.ender);
    }
}
impl Guest {
    pub(super) fn command(
        &mut self,
        input: &str,
        complete: bool,
        world: &mut World,
        sim: &mut Simulation,
        player: &mut Player,
        resources: &crate::resources::Resources,
        options: &Options,
        allow: bool,
        out: &mut Outcome,
        profile: &mut Profile,
    ) {
        let mut options = options.clone();
        options.username.clone_from(&self.actor.name);
        options.world_rules.allow_cheats = allow;
        let previous_effects: Vec<_> = profile.effects.ordered().collect();
        profile.exchange(sim);
        let host_name = std::mem::replace(&mut sim.local_name, self.actor.name.clone());
        let fall = sim.fall_distance;
        let death = sim.death_message_key.take();
        if complete {
            let choices =
                local_commands::completions(sim, player, world, resources, &options, input);
            let mut w = Writer::new();
            w.varint(choices.len() as i32);
            for text in choices {
                w.string(&text);
            }
            self.send(0x3a, w);
        } else {
            let reply = local_commands::execute(sim, player, world, resources, &options, input);
            out.changed.extend(reply.changed);
            for text in reply.messages {
                let mut w = Writer::new();
                w.string(&text.to_string()).u8(1);
                self.send(2, w);
            }
            for (name, at, volume, pitch) in reply.sounds {
                let mut w = Writer::new();
                w.string(&name)
                    .i32((at.x * 8.) as i32)
                    .i32((at.y * 8.) as i32)
                    .i32((at.z * 8.) as i32)
                    .f32(volume)
                    .u8((pitch * 63.) as u8);
                self.send(0x29, w);
            }
            if self.actor.feet.to_array() != player.precise_position()
                || self.actor.look != player.java_rotation()
            {
                self.actor.feet = player.precise_position().into();
                self.actor.look = player.java_rotation();
                self.warp();
            }
            let mode = sim.commands.player_mode.unwrap_or(self.actor.mode);
            if mode != self.actor.mode {
                self.actor.mode = mode;
                self.flying = mode == 3 || mode == 1 && self.flying;
                let mut state = Writer::new();
                state.u8(3).f32(f32::from(mode));
                self.send(0x2b, state);
                self.send(0x39, abilities(mode, self.flying));
                self.send(0x38, player_list_mode(&self.actor));
                self.mine = None;
            }
            let mut health = Writer::new();
            health.f32(sim.health).varint(sim.food).f32(sim.saturation);
            self.send(6, health);
            let mut xp = Writer::new();
            xp.f32(sim.xp.progress)
                .varint(sim.xp.level)
                .varint(sim.xp.total);
            self.send(0x1f, xp);
            for effect in previous_effects {
                if sim.effects.get(effect.id).is_none() {
                    let mut w = Writer::new();
                    w.varint(self.actor.id).u8(effect.id);
                    self.send(0x1e, w);
                }
            }
            for effect in sim.effects.ordered() {
                let mut w = Writer::new();
                w.varint(self.actor.id)
                    .u8(effect.id)
                    .u8(effect.amplifier as u8)
                    .varint(effect.duration.clamp(0, 32767))
                    .bool(effect.show_particles);
                self.send(0x1d, w);
            }
        }
        sim.fall_distance = fall;
        sim.death_message_key = death;
        sim.local_name = host_name;
        profile.exchange(sim);
    }
}
