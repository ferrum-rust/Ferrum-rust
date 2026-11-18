//! Rendering-only state while the local tick owns the authoritative Simulation.
//! This is never ticked, saved, or merged back. Inputs are replayed only after
//! the complete owner returns; normal Clone and serde retain their full meaning.
use super::{MobBrain, Simulation};
type BrainMap = std::collections::BTreeMap<u64, MobBrain>;

impl Simulation {
    pub(crate) fn presentation_clone(&self) -> Self {
        self.presentation_clone_reusing(&mut BrainMap::new())
    }

    pub(crate) fn presentation_clone_reusing(&self, spare: &mut BrainMap) -> Self {
        self.presentation_clone_impl::<false>(&mut CopyProfile::default(), spare)
    }

    pub(crate) fn presentation_clone_profiled(&self) -> (Self, CopyProfile) {
        self.presentation_clone_profiled_reusing(&mut BrainMap::new())
    }

    pub(crate) fn presentation_clone_profiled_reusing(&self, spare: &mut BrainMap) -> (Self, CopyProfile) {
        let mut profile = CopyProfile::default();
        let started = std::time::Instant::now();
        let copy = self.presentation_clone_impl::<true>(&mut profile, spare);
        profile.ms[11] = (started.elapsed().as_secs_f64() * 1000.
            - profile.ms[..11].iter().sum::<f64>()).max(0.);
        // O(1) lengths only. Recursive NBT/slot censuses do not belong in a frame.
        profile.counts = [self.command_entities.mobs.len(), self.command_entities.ender.len(),
            self.mob_brains.len(), self.pick_sections.len(), self.trades.len(), self.containers.len(),
            self.maps.data.len(), self.mobs.len(), self.drops.len(), self.particles.len()];
        (copy, profile)
    }

    fn presentation_clone_impl<const PROFILE: bool>(&self, profile: &mut CopyProfile, spare: &mut BrainMap) -> Self {
        // An explicit literal makes additions to Simulation require a decision.
        // Do not use Simulation::default(): it consumes constructor RNG seeds.
        Self {
            spawn_seed: self.spawn_seed.clone(),
            lan: self.lan.clone(),
            lan_guests: self.lan_guests.clone(),
            command_entities: profile.copy::<PROFILE, _>(0, || self.command_entities.clone()),
            commands: profile.copy::<PROFILE, _>(7, || self.commands.clone()),
            world_commands: profile.copy::<PROFILE, _>(6, || self.world_commands.clone()),
            statistics: self.statistics.clone(),
            mode: self.mode.clone(),
            local_name: self.local_name.clone(),
            presentation: profile.copy::<PROFILE, _>(9, || self.presentation.clone()),
            server_scene: profile.copy::<PROFILE, _>(8, || self.server_scene.clone()),
            inventory: profile.copy::<PROFILE, _>(10, || self.inventory.clone()),
            health: self.health.clone(),
            max_health: self.max_health.clone(),
            fire_ticks: self.fire_ticks.clone(),
            fire_flag: self.fire_flag.clone(),
            food: self.food.clone(),
            previous_food: self.previous_food.clone(),
            natural_regeneration: self.natural_regeneration.clone(),
            player_ticks: self.player_ticks.clone(),
            saturation: self.saturation.clone(),
            exhaustion: self.exhaustion.clone(),
            food_timer: self.food_timer.clone(),
            absorption: self.absorption.clone(),
            effects: self.effects.clone(),
            death_message_key: self.death_message_key.clone(),
            air: self.air.clone(),
            experience: self.experience.clone(),
            xp: self.xp.clone(),
            keep_inventory: self.keep_inventory.clone(),
            bed_spawn: self.bed_spawn.clone(),
            sleep: self.sleep.clone(),
            daylight_cycle: self.daylight_cycle.clone(),
            send_command_feedback: self.send_command_feedback.clone(),
            world_difficulty: self.world_difficulty.clone(),
            server_reduced_debug: self.server_reduced_debug.clone(),
            difficulty_locked: self.difficulty_locked.clone(),
            hardcore: self.hardcore.clone(),
            flat_world: self.flat_world.clone(),
            dimension: self.dimension.clone(),
            weather: self.weather.clone(),
            total_ticks: self.total_ticks.clone(),
            sleep_notice: self.sleep_notice.clone(),
            mob_loot: self.mob_loot.clone(),
            mob_spawning: self.mob_spawning.clone(),
            xp_orbs: self.xp_orbs.clone(),
            pickup_fx: self.pickup_fx.clone(),
            xp_math_random: self.xp_math_random.clone(),
            voice_random: self.voice_random.clone(),
            clock: self.clock.clone(),
            hand_swing: self.hand_swing.clone(),
            equipped: self.equipped.clone(),
            click_controller: self.click_controller.clone(),
            world_border: self.world_border.clone(),
            mobs: self.mobs.clone(),
            dying_mobs: self.dying_mobs.clone(),
            horse_anims: self.horse_anims.clone(),
            minecarts: self.minecarts.clone(),
            boats: self.boats.clone(),
            ridden_minecart: self.ridden_minecart,
            ridden_boat: self.ridden_boat,
            minecart_window: self.minecart_window,
            show_minecart_window: self.show_minecart_window,
            ridden_pig: self.ridden_pig,
            pig_boost_ticks: self.pig_boost_ticks,
            pig_boost_total: self.pig_boost_total,
            pig_speed: self.pig_speed,
            ridden_horse: self.ridden_horse.clone(),
            horse_rider: self.horse_rider.clone(),
            horse_window: self.horse_window.clone(),
            show_horse_window: self.show_horse_window.clone(),
            pick_sections: profile.copy::<PROFILE, _>(2, || self.pick_sections.clone()),
            pick_sequence: self.pick_sequence.clone(),
            mob_brains: profile.copy_brains::<PROFILE>(&self.mob_brains, spare),
            trades: profile.copy::<PROFILE, _>(3, || self.trades.clone()),
            villager_homes: Default::default(),
            pending_golem_spawns: Vec::new(),
            trading_with: self.trading_with.clone(),
            merchant_window: self.merchant_window.clone(),
            drops: self.drops.clone(),
            arrows: self.arrows.clone(),
            mob_shots: self.mob_shots.clone(),
            projectiles: self.projectiles.clone(),
            fishing: self.fishing.clone(),
            maps: profile.copy::<PROFILE, _>(5, || self.maps.clone()),
            primed_tnt: self.primed_tnt.clone(),
            explosion_clouds: Vec::new(),
            note_pitches: self.note_pitches.clone(),
            powered_notes: Default::default(),
            note_events: Vec::new(),
            containers: profile.copy::<PROFILE, _>(4, || self.containers.clone()),
            imported_container_chunks: Default::default(),
            imported_entity_chunks: Default::default(),
            block_ticks: self.block_ticks.presentation_clone(),
            redstone: Default::default(),
            random_tick_lcg: self.random_tick_lcg.clone(),
            rng: self.rng.clone(),
            player_random_state: self.player_random_state.clone(),
            enchantment_random_state: self.enchantment_random_state.clone(),
            next_id: self.next_id.clone(),
            yaw: self.yaw.clone(),
            pitch: self.pitch.clone(),
            particles: self.particles.clone(),
            critical_emitters: Default::default(),
            particle_random: self.particle_random.clone(),
            damage_sounds: self.damage_sounds.clone(),
            pending_world_sounds: self.pending_world_sounds.clone(),
            pending_changed_blocks: Vec::new(),
            end_crystals: self.end_crystals.clone(),
            overworld_spawn: self.overworld_spawn,
            end_dragon_spawned: self.end_dragon_spawned,
            end_dragon_slain: self.end_dragon_slain,
            dimension_slices: std::collections::BTreeMap::new(),
            active_populated_chunks: std::collections::BTreeSet::new(),
            teleporter_caches: Default::default(),
            spawner_spins: self.spawner_spins.clone(),
            breaking: self.breaking.clone(),
            break_target: self.break_target.clone(),
            pending_sign_edit: self.pending_sign_edit,
            eating: self.eating.clone(),
            item_use: self.item_use.clone(),
            hurt_camera: self.hurt_camera.clone(),
            hurt_ticks: self.hurt_ticks.clone(),
            last_damage: self.last_damage.clone(),
            place_ticks: self.place_ticks.clone(),
            right_click_delay: self.right_click_delay.clone(),
            right_was_down: self.right_was_down.clone(),
            fall_distance: self.fall_distance.clone(),
        }
    }
}

#[derive(Default)]
pub(crate) struct CopyProfile {
    ms: [f64; 12],
    counts: [usize; 10],
    brains_reused: bool,
}
impl CopyProfile {
    fn copy_brains<const PROFILE: bool>(&mut self, source: &BrainMap, spare: &mut BrainMap) -> BrainMap {
        let started = PROFILE.then(std::time::Instant::now);
        let reused = source.len() == spare.len() && source.keys().eq(spare.keys());
        let result = if reused {
            let mut result = std::mem::take(spare);
            for (target, brain) in result.values_mut().zip(source.values()) {
                // Existing explicit projection copies every scalar/RNG, never routes.
                *target = brain.presentation_clone();
            }
            result
        } else {
            // Leave old spare for the next worker-retirement handoff.
            source.iter().map(|(&id, brain)| (id, brain.presentation_clone())).collect()
        };
        self.brains_reused = reused;
        if let Some(started) = started { self.ms[1] += started.elapsed().as_secs_f64() * 1000.; }
        result
    }
    #[inline]
    fn copy<const PROFILE: bool, T>(&mut self, index: usize, clone: impl FnOnce() -> T) -> T {
        if !PROFILE { return clone(); }
        let started = std::time::Instant::now();
        let value = clone();
        self.ms[index] += started.elapsed().as_secs_f64() * 1000.;
        value
    }
    pub(crate) fn timings(&self) -> impl Iterator<Item = (&'static str, f64)> + '_ {
        ["tick_presentation_sim_command_entities", "tick_presentation_sim_mob_brains",
            "tick_presentation_sim_pick_sections", "tick_presentation_sim_trades",
            "tick_presentation_sim_containers", "tick_presentation_sim_maps",
            "tick_presentation_sim_world_commands", "tick_presentation_sim_commands",
            "tick_presentation_sim_server_scene", "tick_presentation_sim_presentation",
            "tick_presentation_sim_inventory", "tick_presentation_sim_other"]
            .into_iter().zip(self.ms)
    }
    pub(crate) fn counts(&self) -> impl Iterator<Item = (&'static str, usize)> + '_ {
        ["command_mobs", "ender_slots", "mob_brains", "pick_sections", "trades", "containers",
            "maps", "mobs", "drops", "particles"].into_iter().zip(self.counts)
            .chain([("mob_brains_reused", usize::from(self.brains_reused))])
    }
}

#[cfg(test)]
#[path = "gameplay_presentation_tests.rs"]
mod tests;
