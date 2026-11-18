use super::drops_local::{canonical_drop_state, extra_tile_drop, LocalDrops};
use super::{
    hostile_damage, HitSource, JavaRandom, MobKind, Mode, Outcome, Player, Settings, Simulation,
    World,
};
use crate::vanilla::{drops, entity_pick, tnt};
use glam::{DVec3, IVec3};
#[cfg(test)]
#[path = "gameplay_tnt_tests.rs"]
mod tests;

impl Simulation {
    pub(super) fn prime_block(
        &mut self,
        world: &mut World,
        p: IVec3,
        player_lit: bool,
        out: &mut Outcome,
    ) -> bool {
        if world.get(p) >> 4 != 46 || !world.set(p, 0) {
            return false;
        }
        out.changed.push(p);
        self.block_ticks.notify(world, p);
        self.spawn_primed(p, 80, player_lit);
        out.world_sounds
            .push(("random.fuse", p.as_dvec3() + DVec3::new(0.5, 0., 0.5), 1., 1.));
        true
    }
    pub(super) fn spawn_primed(&mut self, p: IVec3, fuse: i32, player_lit: bool) {
        let mut math = JavaRandom::from_state(self.xp_math_random);
        let velocity = tnt::Primed::launch(&mut math);
        self.xp_math_random = math.state();
        let at = DVec3::new(
            f64::from(p.x as f32 + 0.5),
            f64::from(p.y),
            f64::from(p.z as f32 + 0.5),
        );
        self.primed_tnt
            .push(tnt::Primed::new(at, velocity, fuse, player_lit));
    }
    pub(super) fn light_tnt_by_hand(
        &mut self,
        world: &mut World,
        p: IVec3,
        out: &mut Outcome,
    ) -> bool {
        let Some(id @ (259 | 385)) = self.inventory.held().map(|s| s.id) else {
            return false;
        };
        if !self.prime_block(world, p, true, out) {
            return false;
        }
        if self.mode == Mode::Survival {
            if id == 259 {
                self.wear_slot(self.inventory.selected, 1, false);
            } else {
                self.inventory.take_selected(1);
            }
        }
        self.place_ticks = 4;
        true
    }
    pub(super) fn tick_tnt(
        &mut self,
        world: &mut World,
        player: &mut Player,
        settings: &Settings,
        out: &mut Outcome,
    ) {
        // Appended chain reactions also tick in this iteration of loadedEntityList.
        if self.primed_tnt.is_empty() && self.explosion_clouds.is_empty() {
            return;
        }
        let mut i = 0;
        while i < self.primed_tnt.len() {
            let body = &mut self.primed_tnt[i];
            if !world.contains(body.position.as_vec3()) {
                i += 1;
                continue;
            }
            if let Some(origin) = body.tick(world) {
                let lit = body.player_lit;
                self.primed_tnt.remove(i);
                self.explode(world, player, settings, origin, 4., true, lit, out);
            } else {
                let mut math = JavaRandom::from_state(self.particle_random[2]);
                if self.particles.len() < 4096 {
                    self.particles.extend(super::Particle::network_sprite(
                        11,
                        body.position + DVec3::Y * 0.5,
                        DVec3::ZERO,
                        &mut math,
                    ));
                }
                self.particle_random[2] = math.state();
                i += 1;
            }
        }
        let mut random = JavaRandom::from_state(self.particle_random[2]);
        crate::net_presentation::tick_explosion_clouds(
            &mut self.explosion_clouds,
            &mut self.particles,
            &mut random,
        );
        self.particle_random[2] = random.state();
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn explode(
        &mut self,
        world: &mut World,
        player: &mut Player,
        settings: &Settings,
        center: DVec3,
        power: f32,
        terrain: bool,
        player_lit: bool,
        out: &mut Outcome,
    ) {
        self.explode_excluding(world, player, settings, center, power, terrain, player_lit, None, out);
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn explode_excluding(
        &mut self,
        world: &mut World,
        player: &mut Player,
        settings: &Settings,
        center: DVec3,
        power: f32,
        terrain: bool,
        player_lit: bool,
        source_mob: Option<u64>,
        out: &mut Outcome,
    ) {
        self.explode_with_fire(
            world,
            player,
            settings,
            center,
            power,
            terrain,
            player_lit,
            false,
            source_mob,
            out,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn explode_with_fire(
        &mut self,
        world: &mut World,
        player: &mut Player,
        settings: &Settings,
        center: DVec3,
        power: f32,
        terrain: bool,
        player_lit: bool,
        flaming: bool,
        source_mob: Option<u64>,
        out: &mut Outcome,
    ) {
        let mut random = self.block_ticks.world_random();
        let affected = tnt::footprint(world, center, power, &mut random);
        self.block_ticks.set_world_random(&random);
        let mut detonated_crystals = Vec::new();
        self.end_crystals.retain(|crystal| {
            let bounds = entity_pick::body(crystal.position, 2.0, 2.0);
            let hit = tnt::impact(world, center, power, bounds, 0.0)
                .is_some_and(|(damage, _)| damage > 0.0);
            if hit {
                detonated_crystals.push(crystal.position);
            }
            !hit
        });
        let source = HitSource {
            explosion: true,
            has_attacker: player_lit,
            ..Default::default()
        };
        if self.commands.player_mode != Some(3) {
            if let Some((damage, push)) = tnt::impact(
                world,
                center,
                power,
                player.precise_bounds(),
                player.eye_height(),
            ) {
                let amount = if player_lit {
                    damage
                } else {
                    let difficulty = self.difficulty(settings);
                    if difficulty == 1 {
                        (damage / 2. + 1.).min(damage)
                    } else {
                        hostile_damage(damage, difficulty)
                    }
                };
                if self.hurt_from(amount, source) && self.health <= 0. {
                    self.death_message_key = Some("death.attack.explosion");
                }
                if self.mode != Mode::Creative {
                    player.set_velocity_f64(
                        (DVec3::from_array(player.precise_velocity()) + push).to_array(),
                    );
                }
            }
        }
        let mut enchantment = JavaRandom::from_state(self.enchantment_random_state);
        for mob in &mut self.mobs {
            if mob.health <= 0. || Some(mob.id) == source_mob {
                continue;
            }
            let brain = self.mob_brains.entry(mob.id).or_default();
            let scale = if brain.growing_age < 0 { 0.5 } else { 1. };
            let (width, height) = mob.kind.adult_size(self.command_entities.mobs.get(&mob.id));
            let body = entity_pick::body(mob.position.as_dvec3(), width * scale, height * scale);
            let eye = match mob.kind {
                MobKind::Chicken | MobKind::Cow => height * scale,
                MobKind::Sheep => height * scale * 0.95,
                MobKind::Villager => 1.62 * scale,
                MobKind::Zombie => {
                    if scale == 1. {
                        1.74
                    } else {
                        0.93
                    }
                }
                MobKind::Skeleton => 1.74,
                MobKind::Spider => 0.65,
                _ => height * scale * 0.85,
            };
            if let Some((damage, push)) = tnt::impact(world, center, power, body, eye) {
                mob.velocity += push.as_vec3();
                let data = self.command_entities.mobs.get_mut(&mob.id);
                if data.as_ref().is_some_and(|d| d.invulnerable) {
                    continue;
                }
                let accepted = if mob.hurt > 0 {
                    damage - brain.last_damage
                } else {
                    damage
                };
                if accepted > 0. {
                    mob.health = (mob.health
                        - super::melee::defended_damage(
                            accepted,
                            mob.kind,
                            data,
                            source,
                            &mut enchantment,
                        ))
                    .max(0.);
                    mob.hurt = 10;
                    brain.last_damage = damage;
                }
            }
        }
        self.enchantment_random_state = enchantment.state();
        for body in &mut self.primed_tnt {
            if let Some((_, push)) = tnt::impact(world, center, power, body.bounds, 0.) {
                body.velocity += push;
            }
        }
        for item in &mut self.drops {
            if let Some((damage, push)) = tnt::impact(world, center, power, item.bounds, 0.2125) {
                item.velocity += push;
                if item.stack.id != 399 {
                    item.health -= damage as i32;
                    item.dead |= item.health <= 0;
                }
            }
        }
        self.drops.retain(|i| !i.dead);
        for orb in &mut self.xp_orbs {
            if let Some((damage, push)) = tnt::impact(world, center, power, orb.bounds, 0.425) {
                orb.velocity += push;
                orb.health -= damage as i32;
                orb.dead |= orb.health <= 0;
            }
        }
        let mut random = self.block_ticks.world_random();
        let pitch = (1. + (random.next_float() - random.next_float()) * 0.2) * 0.7;
        out.world_sounds.push(("random.explode", center, 4., pitch));
        self.explosion_clouds.push((center, 0));
        if terrain {
            for &p in &affected {
                let state = world.get(p);
                if state == 0 {
                    continue;
                }
                if state >> 4 != 46 {
                    let extra = extra_tile_drop(self, world, p, state);
                    let norm = canonical_drop_state(state);
                    let mut local = LocalDrops::new(self, world);
                    if let Err(error) = drops::drop_block(&mut local, &mut random, p, norm, 1. / power, 0) {
                        if norm != (state & !15) {
                            let _ = drops::drop_block(&mut local, &mut random, p, state & !15, 1. / power, 0);
                        } else {
                            eprintln!("Explosion drop for {state} at {p}: {error}");
                        }
                    }
                    if let Some(stack) = extra {
                        local.spawn(&mut random, p, stack);
                    }
                }
                if world.set(p, 0) {
                    let id = state >> 4;
                    if matches!(id, 63 | 68 | 140) {
                        self.containers.remove(&super::container_key(p));
                    }
                    if matches!(id, 75 | 76 | 93 | 94 | 149 | 150) {
                        self.purge_circuit_timers_at(p, id);
                    }
                    self.world_commands.edit_tile(world, p, 0, None);
                    out.changed.push(p);
                    self.block_ticks.notify(world, p);
                    if id == 46 {
                        self.spawn_primed(p, 10 + random.next_int(20), player_lit);
                    }
                }
            }
        }
        if flaming {
            for &p in &affected {
                if world.get(p) == 0
                    && crate::vanilla::material::opaque_cube(world.get(p - IVec3::Y))
                    && random.next_int(3) == 0
                    && world.set(p, 51 << 4)
                {
                    out.changed.push(p);
                    self.block_ticks.notify(world, p);
                }
            }
        }
        self.block_ticks.set_world_random(&random);
        for cpos in detonated_crystals {
            self.explode(world, player, settings, cpos, 6.0, true, false, out);
        }
    }
}
