//! Integrated EntityArrow and EntityThrowable flight, impacts and pickup.
use super::*;
use crate::vanilla::{entity_pick, math, ray_trace, spawning::EntityRandom};
use glam::DVec3;
#[cfg(test)]
#[path = "projectile_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Payload {
    Arrow(bow::BowShot),
    Snowball,
    Egg,
    Pearl,
    ExperienceBottle,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Projectile {
    pub payload: Payload,
    pub position: DVec3,
    pub previous: DVec3,
    pub velocity: DVec3,
    pub yaw: f32,
    pub pitch: f32,
    pub previous_yaw: f32,
    pub previous_pitch: f32,
    pub shake: u8,
    pub age: u32,
    pub(crate) dimension: i32,
    random: u64,
    pub(crate) embedded: Option<(IVec3, u16, u16)>,
}

fn expanded_hit(start: DVec3, end: DVec3, bounds: crate::vanilla::movement::Aabb) -> Option<DVec3> {
    let lo = bounds.min - DVec3::splat(f64::from(0.3f32));
    let hi = bounds.max + DVec3::splat(f64::from(0.3f32));
    ray_trace::intercept([lo.x, lo.y, lo.z, hi.x, hi.y, hi.z], start, end).map(|(_, p)| p)
}

impl Projectile {
    fn launched(player: &Player, payload: Payload, dimension: i32, seed: i64) -> Self {
        let yaw = player.java_rotation()[0] / 180. * std::f32::consts::PI;
        let pitch = player.java_rotation()[1] / 180. * std::f32::consts::PI;
        let mut position = DVec3::from_array(player.precise_position());
        position.y += f64::from(player.eye_height());
        position.y -= f64::from(0.1f32);
        position.x -= f64::from(math::cos(yaw) * 0.16);
        position.z -= f64::from(math::sin(yaw) * 0.16);
        let mut random = EntityRandom::new(seed);
        let arrow = matches!(payload, Payload::Arrow(_));
        let factor = if arrow { 1. } else { 0.4 };
        let direction = Vec3::new(
            -math::sin(yaw) * math::cos(pitch) * factor,
            -math::sin(if matches!(payload, Payload::ExperienceBottle) {
                (player.java_rotation()[1] - 20.) / 180. * std::f32::consts::PI
            } else {
                pitch
            }) * factor,
            math::cos(yaw) * math::cos(pitch) * factor,
        )
        .as_dvec3();
        let mut velocity = direction / f64::from(direction.length() as f32);
        for axis in 0..3 {
            let error = random.next_gaussian();
            let sign = if arrow && random.random.next_boolean() {
                -1.
            } else {
                1.
            };
            velocity[axis] += error * sign * f64::from(0.0075f32);
        }
        let speed = match payload {
            Payload::Arrow(shot) => shot.speed,
            Payload::ExperienceBottle => 0.7,
            _ => 1.5,
        };
        velocity *= f64::from(speed);
        let angle = (ai::motion::atan2(velocity.x, velocity.z) * 180.
            / f64::from(std::f32::consts::PI)) as f32;
        let horizontal =
            f64::from((velocity.x * velocity.x + velocity.z * velocity.z).sqrt() as f32);
        let pitch = (ai::motion::atan2(velocity.y, horizontal) * 180.
            / f64::from(std::f32::consts::PI)) as f32;
        Self {
            payload,
            position,
            previous: position,
            velocity,
            yaw: angle,
            previous_yaw: angle,
            pitch,
            previous_pitch: pitch,
            shake: 0,
            age: 0,
            dimension,
            random: random.state().0,
            embedded: None,
        }
    }
    fn rotate(&mut self) {
        let yaw = (ai::motion::atan2(self.velocity.x, self.velocity.z) * 180.
            / f64::from(std::f32::consts::PI)) as f32;
        let horizontal = f64::from(
            (self.velocity.x * self.velocity.x + self.velocity.z * self.velocity.z).sqrt() as f32,
        );
        let pitch = (ai::motion::atan2(self.velocity.y, horizontal) * 180.
            / f64::from(std::f32::consts::PI)) as f32;
        let smooth = |previous: &mut f32, value: f32| {
            while value - *previous < -180. {
                *previous -= 360.;
            }
            while value - *previous >= 180. {
                *previous += 360.;
            }
            *previous + (value - *previous) * 0.2
        };
        self.yaw = smooth(&mut self.previous_yaw, yaw);
        self.pitch = smooth(&mut self.previous_pitch, pitch);
    }
}

impl Simulation {
    pub(super) fn launch_bow(&mut self, player: &Player, shot: bow::BowShot) {
        let seed = random(&mut self.rng) as i64;
        self.projectiles.push(Projectile::launched(
            player,
            Payload::Arrow(shot),
            self.dimension,
            seed,
        ));
    }

    pub(super) fn throw_held_projectile(
        &mut self,
        player: &Player,
        item: &Stack,
        out: &mut Outcome,
    ) -> bool {
        let (payload, name) = match item.id {
            332 => (Payload::Snowball, "snowball"),
            344 => (Payload::Egg, "egg"),
            384 => (Payload::ExperienceBottle, "experience_bottle"),
            368 if self.mode == Mode::Creative => return true,
            368 => (Payload::Pearl, "ender_pearl"),
            _ => return false,
        };
        let mut rng = JavaRandom::new(random(&mut self.rng) as i64);
        out.world_sounds.push((
            "random.bow",
            DVec3::from_array(player.precise_position()),
            0.5,
            0.4 / (rng.next_float() * 0.4 + 0.8),
        ));
        let seed = random(&mut self.rng) as i64;
        self.projectiles
            .push(Projectile::launched(player, payload, self.dimension, seed));
        if self.mode != Mode::Creative {
            self.inventory.take_selected(1);
        }
        let stat = self
            .statistics
            .entry(format!("stat.useItem.minecraft.{name}"))
            .or_default();
        *stat = stat.wrapping_add(1);
        self.place_ticks = 4;
        true
    }

    pub(super) fn tick_projectiles(
        &mut self,
        world: &mut World,
        player: &mut Player,
        out: &mut Outcome,
    ) {
        for mut projectile in std::mem::take(&mut self.projectiles) {
            if projectile.dimension != self.dimension
                || !projectile.position.is_finite()
                || !projectile.velocity.is_finite()
            {
                continue;
            }
            if matches!(projectile.payload, Payload::Pearl) && self.health <= 0. {
                continue;
            }
            if !world.contains(projectile.position.as_vec3()) {
                self.projectiles.push(projectile);
                continue;
            }
            projectile.previous = projectile.position;
            projectile.previous_yaw = projectile.yaw;
            projectile.previous_pitch = projectile.pitch;
            projectile.shake = projectile.shake.saturating_sub(1);
            let mut rng = JavaRandom::from_state(projectile.random);
            if let Some((pos, state, ticks)) = projectile.embedded {
                if world.get(pos) == state {
                    projectile.embedded = Some((pos, state, ticks.saturating_add(1)));
                    if ticks + 1 < 1200 && !self.pickup_arrow(&projectile, player, &mut rng, out) {
                        projectile.random = rng.state();
                        self.projectiles.push(projectile);
                    }
                    continue;
                }
                projectile.embedded = None;
                projectile.velocity *= DVec3::new(
                    f64::from(rng.next_float() * 0.2),
                    f64::from(rng.next_float() * 0.2),
                    f64::from(rng.next_float() * 0.2),
                );
                projectile.age = 0;
                projectile.random = rng.state();
                self.projectiles.push(projectile);
                continue;
            }
            projectile.age = projectile.age.saturating_add(1);
            let start = projectile.position;
            let end = start + projectile.velocity;
            let is_arrow = matches!(projectile.payload, Payload::Arrow(_));
            let block = ray_trace::trace(
                world,
                start,
                end,
                ray_trace::Options {
                    stop_on_liquid: false,
                    ignore_without_box: is_arrow,
                    return_last_miss: false,
                },
            );
            let clipped = block.map_or(end, |hit| hit.point);
            let mut closest = start.distance_squared(clipped);
            let mut target: Option<(Option<u64>, usize)> = None;
            for mob in self.mobs.iter().filter(|m| m.health > 0.) {
                if mob.kind == MobKind::EnderDragon {
                    for (part_idx, _, part_box) in
                        melee::dragon_part_boxes(mob, self.mob_brains.get(&mob.id))
                    {
                        let Some(point) = expanded_hit(start, clipped, part_box) else {
                            continue;
                        };
                        let distance = point.distance_squared(start);
                        if distance <= closest {
                            closest = distance;
                            target = Some((Some(mob.id), part_idx));
                        }
                    }
                    continue;
                }
                let (width, height) = self.mob_size(mob);
                let Some(point) = expanded_hit(
                    start,
                    clipped,
                    entity_pick::body(mob.position.as_dvec3(), width, height),
                ) else {
                    continue;
                };
                let distance = point.distance_squared(start);
                if distance <= closest {
                    closest = distance;
                    target = Some((Some(mob.id), 0));
                }
            }
            if projectile.age >= 5
                && self.mode != Mode::Creative
                && !matches!(projectile.payload, Payload::Pearl)
                && self.commands.player_mode != Some(3)
            {
                if let Some(point) = expanded_hit(start, clipped, player.precise_bounds()) {
                    if point.distance_squared(start) < closest {
                        target = Some((None, 0));
                    }
                }
            }
            if let Some((target_id, dragon_part)) = target {
                if self.projectile_hit(
                    &mut projectile,
                    target_id,
                    dragon_part,
                    world,
                    player,
                    &mut rng,
                    out,
                ) {
                    continue;
                }
            } else if let Some(hit) = block {
                if is_arrow {
                    projectile.velocity = (hit.point - start).as_vec3().as_dvec3();
                    projectile.position -= projectile.velocity
                        / f64::from(projectile.velocity.length() as f32)
                        * f64::from(0.05f32);
                    projectile.embedded = Some((hit.position, world.get(hit.position), 0));
                    projectile.shake = 7;
                    if let Payload::Arrow(shot) = &mut projectile.payload {
                        shot.critical = false;
                    }
                    out.world_sounds.push((
                        "random.bowhit",
                        projectile.position,
                        1.,
                        1.2 / (rng.next_float() * 0.2 + 0.9),
                    ));
                } else if world.get(hit.position) >> 4 != 90 {
                    self.finish_throw(&projectile, player, &mut rng, out);
                    continue;
                }
            }
            if projectile.position.y < -64. {
                continue;
            }
            if let Payload::Arrow(shot) = &mut projectile.payload {
                shot.fire_ticks = shot.fire_ticks.saturating_sub(1);
                if shot.critical {
                    let mut visual = JavaRandom::from_state(self.particle_random[2]);
                    for n in 0..4 {
                        if self.particles.len() >= 4096 {
                            break;
                        }
                        self.particles.push(Particle::critical(
                            false,
                            projectile.position + projectile.velocity * (f64::from(n) / 4.),
                            -projectile.velocity + DVec3::Y * 0.2,
                            &mut visual,
                            world,
                        ));
                    }
                    self.particle_random[2] = visual.state();
                }
            }
            projectile.position += projectile.velocity;
            projectile.rotate();
            let water = matches!(
                world.get(projectile.position.floor().as_ivec3()) >> 4,
                8 | 9
            );
            let drag = if water {
                if is_arrow {
                    0.6f32
                } else {
                    0.8f32
                }
            } else {
                0.99f32
            };
            projectile.velocity *= f64::from(drag);
            projectile.velocity.y -= f64::from(match projectile.payload {
                Payload::Arrow(_) => 0.05f32,
                Payload::ExperienceBottle => 0.07f32,
                _ => 0.03f32,
            });
            if water {
                if let Payload::Arrow(shot) = &mut projectile.payload {
                    shot.fire_ticks = 0;
                }
            }
            projectile.random = rng.state();
            self.projectiles.push(projectile);
        }
    }

    fn pickup_arrow(
        &mut self,
        projectile: &Projectile,
        player: &Player,
        rng: &mut JavaRandom,
        out: &mut Outcome,
    ) -> bool {
        let Payload::Arrow(shot) = projectile.payload else {
            return false;
        };
        if projectile.shake != 0 || self.health <= 0. || self.commands.player_mode == Some(3) {
            return false;
        }
        let b = player.precise_bounds();
        let p = projectile.position;
        if p.x + 0.25 < b.min.x - 1.
            || p.x - 0.25 > b.max.x + 1.
            || p.z + 0.25 < b.min.z - 1.
            || p.z - 0.25 > b.max.z + 1.
            || p.y + 0.5 < b.min.y - 0.5
            || p.y > b.max.y + 0.5
        {
            return false;
        }
        let picked = match shot.pickup {
            1 => self.inventory.add(Stack::new(262, 1)).is_none(),
            2 => self.mode == Mode::Creative,
            _ => false,
        };
        if picked {
            out.world_sounds.push((
                "random.pop",
                p,
                0.2,
                ((rng.next_float() - rng.next_float()) * 0.7 + 1.) * 2.,
            ));
        }
        picked
    }

    fn projectile_hit(
        &mut self,
        projectile: &mut Projectile,
        target: Option<u64>,
        dragon_part: usize,
        world: &World,
        player: &mut Player,
        rng: &mut JavaRandom,
        out: &mut Outcome,
    ) -> bool {
        // EntityExpBottle.onImpact never attacks its target, even for zero
        // damage (which would otherwise trigger hurt/knockback callbacks).
        if matches!(projectile.payload, Payload::ExperienceBottle) {
            self.finish_throw(projectile, player, rng, out);
            return true;
        }
        let arrow = match projectile.payload {
            Payload::Arrow(shot) => Some(shot),
            _ => None,
        };
        let enderman_silent = target
            .and_then(|id| self.command_entities.mobs.get(&id))
            .is_some_and(|d| d.silent);
        if let Some(mob) = target.and_then(|id| {
            self.mobs
                .iter_mut()
                .find(|m| m.id == id && m.kind == MobKind::Enderman)
        }) {
            let brain = self.mob_brains.entry(mob.id).or_default();
            let state = brain
                .entity_random
                .get_or_insert_with(|| JavaRandom::new(JavaRandom::independent_seed()).state());
            let mut random = JavaRandom::from_state(*state);
            let mut escaped = false;
            for _ in 0..64 {
                let target_pos = mob.position.as_dvec3()
                    + DVec3::new(
                        (random.next_double() - 0.5) * 64.,
                        f64::from(random.next_int(64) - 32),
                        (random.next_double() - 0.5) * 64.,
                    );
                if species_ai::teleport_enderman_to(
                    world,
                    mob,
                    brain,
                    target_pos,
                    &mut self.particle_random[2],
                    &mut self.particles,
                    enderman_silent,
                    out,
                ) {
                    escaped = true;
                    break;
                }
            }
            brain.entity_random = Some(random.state());
            if arrow.is_some() {
                if !escaped {
                    projectile.velocity *= -0.1;
                    projectile.yaw += 180.;
                    projectile.previous_yaw += 180.;
                    projectile.age = 0;
                }
                return false;
            }
            self.finish_throw(projectile, player, rng, out);
            return true;
        }
        let mut damage = if let Some(shot) = arrow {
            let mut amount =
                (f64::from(projectile.velocity.length() as f32) * shot.damage).ceil() as i32;
            if shot.critical {
                amount += rng.next_int(amount / 2 + 2);
            }
            amount as f32
        } else {
            0.
        };
        let source = HitSource {
            projectile: true,
            has_attacker: true,
            ..Default::default()
        };
        let mut accepted = false;
        if let Some(id) = target {
            if let Some(mob) = self.mobs.iter_mut().find(|m| m.id == id) {
                let brain = self.mob_brains.entry(id).or_default();
                let silent = self
                    .command_entities
                    .mobs
                    .get(&id)
                    .is_some_and(|d| d.silent);
                let data = self.command_entities.mobs.get_mut(&id);
                if matches!(projectile.payload, Payload::Snowball) && mob.kind == MobKind::Blaze {
                    damage = 3.;
                }
                if mob.kind == MobKind::EnderDragon && dragon_part != 0 {
                    damage = damage / 4.0 + 1.0;
                }
                let immune = data.as_ref().is_some_and(|d| d.invulnerable)
                    || (mob.kind == MobKind::Wither && mob.health <= 150. && arrow.is_some());
                if !immune {
                    if let Some(fresh) = melee::apply_mob_hit(
                        mob,
                        brain,
                        data,
                        damage,
                        source,
                        &mut self.enchantment_random_state,
                    ) {
                        accepted = true;
                        brain.player_hit_ticks = 100;
                        if mob.kind == MobKind::EnderDragon {
                            melee::veer_dragon_on_hit(mob, brain);
                            if fresh {
                                melee::mob_hurt_feedback(mob, brain, None, false, silent, out);
                            }
                        } else {
                            brain.hurt(player.position);
                            if fresh {
                                mob.velocity = melee::recoil(
                                    mob.position.as_dvec3(),
                                    mob.velocity.as_dvec3(),
                                    DVec3::from_array(player.precise_position()),
                                    &mut self.xp_math_random,
                                )
                                .as_vec3();
                                melee::mob_hurt_feedback(mob, brain, None, true, silent, out);
                            }
                            if let Some(shot) = arrow {
                                if shot.fire_ticks > 0 && mob.kind != MobKind::Enderman {
                                    burn::set_fire(&mut brain.fire_ticks, 5, 0);
                                }
                                let length = projectile.velocity.x.hypot(projectile.velocity.z);
                                if shot.punch > 0 && length > 0. {
                                    mob.velocity += (DVec3::new(
                                        projectile.velocity.x,
                                        0.,
                                        projectile.velocity.z,
                                    ) / length
                                        * f64::from(shot.punch)
                                        * 0.6
                                        + DVec3::Y * 0.1)
                                        .as_vec3();
                                }
                            }
                        }
                    }
                }
            }
        } else {
            accepted = self.hurt_from(damage, source);
        }
        if arrow.is_some() {
            if accepted {
                out.world_sounds.push((
                    "random.bowhit",
                    projectile.position,
                    1.,
                    1.2 / (rng.next_float() * 0.2 + 0.9),
                ));
                return true;
            }
            projectile.velocity *= -0.1;
            projectile.yaw += 180.;
            projectile.previous_yaw += 180.;
            projectile.age = 0;
            return false;
        }
        self.finish_throw(projectile, player, rng, out);
        true
    }

    fn finish_throw(
        &mut self,
        projectile: &Projectile,
        player: &mut Player,
        rng: &mut JavaRandom,
        out: &mut Outcome,
    ) {
        match projectile.payload {
            Payload::ExperienceBottle => {
                let mut world_random = self.block_ticks.world_random();
                let total = 3 + world_random.next_int(5) + world_random.next_int(5);
                self.block_ticks.set_world_random(&world_random);
                self.spawn_xp_total(projectile.position, total);
                let at = projectile.position.floor();
                let mut visual = JavaRandom::from_state(self.particle_random[2]);
                for _ in 0..8 {
                    let mut gaussian = EntityRandom::from_state(visual.state(), None);
                    let motion = DVec3::new(
                        gaussian.next_gaussian() * 0.15,
                        gaussian.random.next_double() * 0.2,
                        gaussian.next_gaussian() * 0.15,
                    );
                    visual = gaussian.random;
                    self.particles
                        .push(Particle::item(373, 0, false, at, motion, &mut visual));
                }
                for _ in 0..100 {
                    if self.particles.len() >= 4096 {
                        break;
                    }
                    let radius = visual.next_double() * 4.;
                    let angle = visual.next_double() * std::f64::consts::TAU;
                    let motion = DVec3::new(
                        angle.cos() * radius,
                        0.01 + visual.next_double() * 0.5,
                        angle.sin() * radius,
                    );
                    let brightness = 0.75 + visual.next_float() * 0.25;
                    if let Some(p) = Particle::potion_splash(
                        at + DVec3::new(motion.x * 0.1, 0.3, motion.z * 0.1),
                        motion,
                        radius as f32,
                        brightness,
                        &mut visual,
                    ) {
                        self.particles.push(p);
                    }
                }
                out.world_sounds.push((
                    "game.potion.smash",
                    projectile.position,
                    1.,
                    0.9 + visual.next_float() * 0.1,
                ));
                self.particle_random[2] = visual.state();
                return;
            }
            Payload::Egg => {
                if rng.next_int(8) == 0 {
                    let count = if rng.next_int(32) == 0 { 4 } else { 1 };
                    for _ in 0..count {
                        let id = self.spawn_registered(
                            MobKind::Chicken,
                            projectile.position.as_vec3(),
                            projectile.yaw.to_radians(),
                        );
                        self.mob_brains.entry(id).or_default().growing_age = -24000;
                    }
                }
            }
            Payload::Pearl if self.health > 0. && !player.sleeping => {
                if rng.next_float() < 0.05 && self.mob_spawning {
                    let id = self.spawn_registered(MobKind::Endermite, player.position, player.yaw);
                    self.command_entities
                        .mobs
                        .entry(id)
                        .or_default()
                        .extra
                        .insert(
                            "PlayerSpawned".into(),
                            crate::vanilla::drops::DropNbt::Byte(1),
                        );
                }
                self.clear_local_mounts(player);
                player.set_position_f64(projectile.position.to_array());
                self.fall_distance = 0.;
                if self.hurt_from(
                    5.,
                    HitSource {
                        fall: true,
                        unblockable: true,
                        ..Default::default()
                    },
                ) && self.health <= 0.
                {
                    self.die(player);
                }
            }
            _ => {}
        }
        let mut visual = EntityRandom::from_state(self.particle_random[2], None);
        let count = if matches!(projectile.payload, Payload::Pearl) {
            32
        } else {
            8
        };
        for _ in 0..count {
            if self.particles.len() >= 4096 {
                break;
            }
            match projectile.payload {
                Payload::Pearl => {
                    let pos = projectile.position + DVec3::Y * visual.random.next_double() * 2.;
                    let vel = DVec3::new(visual.next_gaussian(), 0., visual.next_gaussian());
                    if let Some(p) = Particle::special(24, pos, vel, &mut visual.random) {
                        self.particles.push(p);
                    }
                }
                Payload::Snowball | Payload::Egg => {
                    let id = if matches!(projectile.payload, Payload::Snowball) {
                        332
                    } else {
                        344
                    };
                    self.particles.push(Particle::item(
                        id,
                        0,
                        false,
                        projectile.position,
                        DVec3::ZERO,
                        &mut visual.random,
                    ));
                }
                _ => {}
            }
        }
        self.particle_random[2] = visual.random.state();
        let _ = out;
    }
}
