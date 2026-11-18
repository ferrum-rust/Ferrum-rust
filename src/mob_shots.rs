use super::*;
use crate::vanilla::ray_trace;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub enum Payload {
    Fireball,
    SmallFireball,
    Skull,
    Snowball,
    Splash(u16),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Shot {
    pub payload: Payload,
    pub previous: Vec3,
    pub position: Vec3,
    velocity: Vec3,
    acceleration: Vec3,
    age: u16,
    owner: u64,
    #[serde(default)]
    from_player: bool,
    #[serde(default)]
    pub nbt: Option<crate::vanilla::drops::DropNbt>,
}

impl Shot {
    pub(super) fn thrown_splash(
        player: &Player,
        meta: u16,
        rng: &mut crate::vanilla::random::JavaRandom,
    ) -> Self {
        Self::thrown_splash_with_nbt(player, meta, None, rng)
    }

    pub(super) fn thrown_splash_with_nbt(
        player: &Player,
        meta: u16,
        nbt: Option<crate::vanilla::drops::DropNbt>,
        rng: &mut crate::vanilla::random::JavaRandom,
    ) -> Self {
        let position = player.eye(1.0) - Vec3::Y * 0.1;
        let [yaw, pitch] = player.java_rotation();
        let yr = yaw.to_radians();
        let pr = pitch.to_radians();
        let pr_toss = (pitch - 20.0).to_radians();
        let dir = Vec3::new(-yr.sin() * pr.cos(), -pr_toss.sin(), yr.cos() * pr.cos())
            .normalize_or_zero();
        let mut gauss = || {
            let u1 = rng.next_double().max(1e-12);
            let u2 = rng.next_double() * std::f64::consts::TAU;
            ((-2.0 * u1.ln()).sqrt() * u2.cos()) as f32
        };
        let spread = Vec3::new(gauss(), gauss(), gauss()) * 0.0075;
        let velocity = (dir + spread).normalize_or_zero() * 0.5;
        Self {
            payload: Payload::Splash(meta),
            previous: position,
            position,
            velocity,
            acceleration: Vec3::NEG_Y * 0.05,
            age: 0,
            owner: 0,
            from_player: true,
            nbt,
        }
    }

    pub(super) fn aim(
        mob: &Mob,
        brain: &mut MobBrain,
        target: Vec3,
        health: f32,
        effects: &Effects,
        rng: &mut u64,
    ) -> Option<Self> {
        Self::aim_with_undead(mob, brain, target, health, effects, false, rng)
    }

    pub(super) fn aim_with_undead(
        mob: &Mob,
        brain: &mut MobBrain,
        target: Vec3,
        health: f32,
        effects: &Effects,
        target_undead: bool,
        rng: &mut u64,
    ) -> Option<Self> {
        if brain.attack_timer > 0 {
            return None;
        }
        let distance = target.distance(mob.position);
        let horiz_dist = (target - mob.position).with_y(0.).length();
        use MobKind::*;
        let payload = match mob.kind {
            Ghast if distance < 64. => {
                brain.attack_step += 1;
                if brain.attack_step < 20 {
                    return None;
                }
                brain.attack_step = 0;
                brain.attack_timer = 40;
                Payload::Fireball
            }
            Blaze if (2.0..48.).contains(&distance) => {
                brain.attack_step += 1;
                if brain.attack_step == 1 {
                    brain.attack_timer = 60;
                    return None;
                }
                if brain.attack_step > 4 {
                    brain.attack_timer = 100;
                    brain.attack_step = 0;
                    return None;
                }
                brain.attack_timer = 6;
                Payload::SmallFireball
            }
            Wither if distance < 30. => {
                brain.attack_timer = 40;
                Payload::Skull
            }
            SnowGolem if distance < 10. => {
                brain.attack_timer = 20;
                Payload::Snowball
            }
            Witch if distance < 10. => {
                brain.attack_timer = 60;
                let potion = if target_undead {
                    32725
                } else if horiz_dist >= 8. && effects.get(2).is_none() {
                    32698
                } else if health >= 8. && effects.get(19).is_none() {
                    32660
                } else if horiz_dist <= 3. && effects.get(18).is_none() && unit(rng) < 0.25 {
                    32696
                } else {
                    32732
                };
                Payload::Splash(potion)
            }
            _ => return None,
        };
        let mut position = mob.position + Vec3::Y * mob.kind.size().1 * 0.75;
        let mut impulse = target - position;
        if matches!(payload, Payload::Snowball | Payload::Splash(_)) {
            impulse.y += impulse.with_y(0.).length() * 0.2;
        }
        let heading = impulse.normalize_or_zero();
        let (velocity, acceleration) = match payload {
            Payload::Snowball => (heading * 1.6, Vec3::NEG_Y * 0.03),
            Payload::Splash(_) => (heading * 0.75, Vec3::NEG_Y * 0.05),
            _ => {
                if payload == Payload::Fireball {
                    position += heading * 4.;
                }
                (Vec3::ZERO, heading * 0.1)
            }
        };
        Some(Self {
            payload,
            previous: position,
            position,
            velocity,
            acceleration,
            age: 0,
            owner: mob.id,
            from_player: false,
            nbt: None,
        })
    }
}

fn resolve_splash_effects(
    potion: u16,
    nbt: Option<&crate::vanilla::drops::DropNbt>,
) -> (Vec<crate::ui::creative::PotionEffectData>, u32, bool) {
    use crate::ui::creative::{
        potion_color_and_glint, potion_effects_for_stack, potion_is_instant, PotionEffectData,
    };
    let (effects, rgb) = if nbt.is_none() && matches!(potion, 2 | 7 | 18 | 19) {
        match potion {
            2 => (
                vec![PotionEffectData {
                    id: 2,
                    amplifier: 0,
                    duration: 1800,
                }],
                5926017,
            ),
            19 => (
                vec![PotionEffectData {
                    id: 19,
                    amplifier: 0,
                    duration: 900,
                }],
                5149489,
            ),
            18 => (
                vec![PotionEffectData {
                    id: 18,
                    amplifier: 0,
                    duration: 1800,
                }],
                0x484d48,
            ),
            _ => (
                vec![PotionEffectData {
                    id: 7,
                    amplifier: 0,
                    duration: 1,
                }],
                4393481,
            ),
        }
    } else {
        let mut stack = Stack::new(373, 1);
        stack.meta = potion;
        stack.nbt = nbt.cloned().map(std::sync::Arc::new);
        let effs = potion_effects_for_stack(&stack);
        let color = potion_color_and_glint(&stack).0;
        (effs, color)
    };
    let instant = effects.iter().any(|e| potion_is_instant(e.id));
    (effects, rgb, instant)
}

fn entry(start: Vec3, delta: Vec3, feet: Vec3, size: (f32, f32)) -> Option<f32> {
    let radius = size.0 / 2. + 0.3;
    let lo = feet - Vec3::new(radius, 0.3, radius);
    let hi = feet + Vec3::new(radius, size.1 + 0.3, radius);
    let mut near = 0.0_f32;
    let mut far = 1.0_f32;
    for i in 0..3 {
        if delta[i].abs() < 1e-8 {
            if start[i] < lo[i] || start[i] > hi[i] {
                return None;
            }
        } else {
            let a = (lo[i] - start[i]) / delta[i];
            let b = (hi[i] - start[i]) / delta[i];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return None;
            }
        }
    }
    Some(near)
}

impl Simulation {
    pub(super) fn tick_mob_shots(
        &mut self,
        world: &mut World,
        player: &mut Player,
        settings: &Settings,
        out: &mut Outcome,
    ) {
        let grief = self
            .world_commands
            .rules
            .get("mobGriefing")
            .is_none_or(|s| s.eq_ignore_ascii_case("true"));
        let difficulty = self.world_difficulty.unwrap_or(settings.difficulty);
        for mut shot in std::mem::take(&mut self.mob_shots) {
            let start = shot.position;
            let delta = shot.velocity;
            let block = ray_trace::trace(
                world,
                start.as_dvec3(),
                (start + delta).as_dvec3(),
                ray_trace::Options {
                    stop_on_liquid: false,
                    ignore_without_box: false,
                    return_last_miss: false,
                },
            );
            let mut time = block.map_or(1., |h| {
                (h.point.as_vec3() - start).length() / delta.length().max(1e-8)
            });
            let mut target = None;
            let mut target_dragon_part = 0usize;
            if !player.is_spectator()
                && (matches!(shot.payload, Payload::Splash(_)) || self.mode == Mode::Survival)
                && !(shot.from_player && shot.age < 5)
            {
                if let Some(t) =
                    entry(start, delta, player.position, (0.6, 1.8)).filter(|t| *t < time)
                {
                    target = Some(None);
                    time = t;
                }
            }
            for mob in &self.mobs {
                if mob.health <= 0. || (!shot.from_player && mob.id == shot.owner) {
                    continue;
                }
                if mob.kind == MobKind::EnderDragon {
                    for (part_idx, center, _) in
                        melee::dragon_part_boxes(mob, self.mob_brains.get(&mob.id))
                    {
                        let [w, h] = crate::remote::dragon::SIZES[part_idx];
                        let part_feet =
                            Vec3::new(center.x as f32, center.y as f32, center.z as f32);
                        if let Some(t) = entry(start, delta, part_feet, (w, h)).filter(|t| *t < time)
                        {
                            target = Some(Some(mob.id));
                            target_dragon_part = part_idx;
                            time = t;
                        }
                    }
                    continue;
                }
                if let Some(t) =
                    entry(start, delta, mob.position, self.mob_size(mob)).filter(|t| *t < time)
                {
                    target = Some(Some(mob.id));
                    target_dragon_part = 0;
                    time = t;
                }
            }
            if target.is_none() && block.is_none() {
                shot.previous = start;
                shot.position += delta;
                shot.velocity = match shot.payload {
                    Payload::Splash(_) | Payload::Snowball => delta * 0.99 + shot.acceleration,
                    _ => (delta + shot.acceleration) * 0.95,
                };
                shot.age += 1;
                if shot.age < 600 && world.contains(shot.position) {
                    self.mob_shots.push(shot);
                }
                continue;
            }
            let impact = start + delta * time;
            let (base, fire) = match shot.payload {
                Payload::Fireball => (6., true),
                Payload::SmallFireball => (5., true),
                Payload::Skull => (8., false),
                _ => (0., false),
            };
            let source = HitSource {
                projectile: true,
                has_attacker: true,
                fire,
                ..Default::default()
            };
            if let Some(hit) = target {
                if let Some(id) = hit {
                    if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                        let brain = self.mob_brains.entry(id).or_default();
                        let silent = self
                            .command_entities
                            .mobs
                            .get(&id)
                            .is_some_and(|d| d.silent);
                        if m.kind == MobKind::Enderman {
                            species_ai::teleport_enderman_randomly(
                                world,
                                m,
                                brain,
                                &mut self.rng,
                                &mut self.particle_random[2],
                                &mut self.particles,
                                silent,
                                out,
                            );
                        } else {
                            let data = self.command_entities.mobs.get_mut(&id);
                            let mut amount =
                                if shot.payload == Payload::Snowball && m.kind == MobKind::Blaze {
                                    3.
                                } else {
                                    base
                                };
                            if m.kind == MobKind::EnderDragon && amount > 0. {
                                if !shot.from_player {
                                    amount = 0.;
                                } else if target_dragon_part != 0 {
                                    amount = amount / 4.0 + 1.0;
                                }
                            }
                            if amount > 0.
                                && !data.as_ref().is_some_and(|d| d.invulnerable)
                                && !(fire && burn::fire_immune(m.kind, data.as_deref()))
                            {
                                let protection = burn::mob_protection(data.as_deref());
                                if melee::apply_mob_hit(
                                    m,
                                    brain,
                                    data,
                                    amount,
                                    source,
                                    &mut self.enchantment_random_state,
                                ) == Some(true)
                                {
                                    if m.kind == MobKind::EnderDragon {
                                        melee::veer_dragon_on_hit(m, brain);
                                    }
                                    melee::mob_hurt_feedback(m, brain, None, false, silent, out);
                                    if shot.payload == Payload::SmallFireball
                                        && m.kind != MobKind::EnderDragon
                                    {
                                        burn::set_fire(&mut brain.fire_ticks, 5, protection);
                                    }
                                }
                            }
                        }
                    }
                } else if base > 0. && self.hurt_from(hostile_damage(base, difficulty), source) {
                    if shot.payload == Payload::SmallFireball {
                        burn::set_fire(
                            &mut self.fire_ticks,
                            5,
                            self.inventory.armor_enchantment(1),
                        );
                    }
                    if shot.payload == Payload::Skull && difficulty >= 2 {
                        self.command_player_effect(command_entity_state::EffectEdit::Add(
                            Effect::new(20, if difficulty == 2 { 200 } else { 800 }, 1),
                        ));
                    }
                }
            }
            match shot.payload {
                Payload::Fireball | Payload::Skull => self.explode(
                    world,
                    player,
                    settings,
                    impact.as_dvec3(),
                    1.,
                    grief,
                    false,
                    out,
                ),
                Payload::SmallFireball if grief && target.is_none() => {
                    if let Some(hit) = block {
                        let effects =
                            self.block_ticks
                                .ignite_air(world, hit.adjacent(), self.dimension);
                        self.fire_outcome(effects, out);
                    }
                }
                Payload::Splash(potion) => {
                    let (effects, rgb, instant) = resolve_splash_effects(potion, shot.nbt.as_ref());
                    let impact_d = impact.as_dvec3();
                    let splash_box = crate::vanilla::movement::Aabb {
                        min: impact_d - glam::DVec3::new(4.125, 2.125, 4.125),
                        max: impact_d + glam::DVec3::new(4.125, 2.125, 4.125),
                    };
                    if !effects.is_empty() {
                        let affected = self
                            .mobs
                            .iter()
                            .filter_map(|m| {
                                if m.health <= 0. || m.kind == MobKind::EnderDragon {
                                    return None;
                                }
                                let (w, h) = self.mob_size(m);
                                let mob_box =
                                    crate::vanilla::entity_pick::body(m.position.as_dvec3(), w, h);
                                if target != Some(Some(m.id)) && !splash_box.intersects(mob_box) {
                                    return None;
                                }
                                let dist_sq = m.position.as_dvec3().distance_squared(impact_d);
                                if target != Some(Some(m.id)) && dist_sq >= 16.0 {
                                    return None;
                                }
                                let d1 = if target == Some(Some(m.id)) {
                                    1.0
                                } else {
                                    1.0 - dist_sq.sqrt() / 4.0
                                };
                                (d1 > 0.0).then_some((m.id, d1))
                            })
                            .collect::<Vec<_>>();
                        let can_player_provoke =
                            shot.from_player && self.mode == Mode::Survival && !player.is_spectator();
                        for (id, d1) in affected {
                            let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) else {
                                continue;
                            };
                            let undead = matches!(
                                m.kind,
                                MobKind::Zombie
                                    | MobKind::Skeleton
                                    | MobKind::Wither
                                    | MobKind::PigZombie
                            );
                            for eff in &effects {
                                if (eff.id == 6 && !undead) || (eff.id == 7 && undead) {
                                    let heal = (d1
                                        * f64::from(4i32.wrapping_shl(eff.amplifier.max(0) as u32))
                                        + 0.5)
                                        as i32;
                                    if heal > 0 && m.health > 0. {
                                        let data_ref = self.command_entities.mobs.get(&id);
                                        let max_hp = m.kind.max_health(data_ref)
                                            + data_ref
                                                .and_then(|d| d.effects.get(21))
                                                .map_or(0., |e| 4. * (e.amplifier + 1) as f32);
                                        m.health = (m.health + heal as f32).min(max_hp);
                                    }
                                } else if (eff.id == 7 && !undead) || (eff.id == 6 && undead) {
                                    let dmg = (d1
                                        * f64::from(6i32.wrapping_shl(eff.amplifier.max(0) as u32))
                                        + 0.5)
                                        as i32;
                                    if dmg > 0 {
                                        let brain = self.mob_brains.entry(id).or_default();
                                        let silent = self
                                            .command_entities
                                            .mobs
                                            .get(&id)
                                            .is_some_and(|d| d.silent);
                                        if m.kind == MobKind::Enderman {
                                            species_ai::teleport_enderman_randomly(
                                                world,
                                                m,
                                                brain,
                                                &mut self.rng,
                                                &mut self.particle_random[2],
                                                &mut self.particles,
                                                silent,
                                                out,
                                            );
                                            continue;
                                        }
                                        if can_player_provoke {
                                            brain.player_hit_ticks = 100;
                                            if matches!(
                                                m.kind,
                                                MobKind::PigZombie
                                                    | MobKind::Wolf
                                                    | MobKind::Spider
                                                    | MobKind::CaveSpider
                                                    | MobKind::IronGolem
                                            ) {
                                                brain.anger_ticks = 400;
                                            }
                                        }
                                        let data = self.command_entities.mobs.get_mut(&id);
                                        if !data.as_ref().is_some_and(|d| d.invulnerable) {
                                            if melee::apply_mob_hit(
                                                m,
                                                brain,
                                                data,
                                                dmg as f32,
                                                HitSource {
                                                    unblockable: true,
                                                    has_attacker: shot.from_player,
                                                    ..Default::default()
                                                },
                                                &mut self.enchantment_random_state,
                                            ) == Some(true)
                                            {
                                                melee::mob_hurt_feedback(
                                                    m, brain, None, false, silent, out,
                                                );
                                            }
                                        }
                                    }
                                } else if (1..=23).contains(&eff.id) {
                                    let j = (d1 * f64::from(eff.duration) + 0.5) as i32;
                                    if j > 20 {
                                        if can_player_provoke
                                            && matches!(eff.id, 2 | 18 | 19 | 20)
                                            && m.kind != MobKind::Enderman
                                        {
                                            self.mob_brains.entry(id).or_default().player_hit_ticks =
                                                100;
                                        }
                                        self.command_entities.mobs.entry(id).or_default().effect(
                                            m,
                                            command_entity_state::EffectEdit::Add(Effect::new(
                                                eff.id,
                                                j,
                                                eff.amplifier,
                                            )),
                                        );
                                    }
                                }
                            }
                        }
                        let player_feet = glam::DVec3::from_array(player.precise_position());
                        let player_dist_sq = player_feet.distance_squared(impact_d);
                        let player_in_splash = target == Some(None)
                            || (splash_box.intersects(player.precise_bounds())
                                && player_dist_sq < 16.0);
                        if player_in_splash && !player.is_spectator() && self.health > 0. {
                            let d1 = if target == Some(None) {
                                1.0
                            } else {
                                1.0 - player_dist_sq.sqrt() / 4.0
                            };
                            if d1 > 0.0 {
                                for eff in &effects {
                                    if eff.id == 6 {
                                        if self.health > 0. {
                                            let heal = (d1
                                                * f64::from(
                                                    4i32.wrapping_shl(eff.amplifier.max(0) as u32),
                                                )
                                                + 0.5)
                                                as i32;
                                            if heal > 0 {
                                                self.health =
                                                    (self.health + heal as f32).min(self.max_health);
                                            }
                                        }
                                    } else if eff.id == 7 {
                                        if self.mode == Mode::Survival {
                                            let dmg = (d1
                                                * f64::from(
                                                    6i32.wrapping_shl(eff.amplifier.max(0) as u32),
                                                )
                                                + 0.5)
                                                as i32;
                                            if dmg > 0
                                                && self.hurt_from(
                                                    dmg as f32,
                                                    HitSource {
                                                        unblockable: true,
                                                        ..Default::default()
                                                    },
                                                )
                                                && self.health <= 0.
                                            {
                                                self.death_message_key = Some("death.attack.magic");
                                            }
                                        }
                                    } else if eff.id == 23 {
                                        self.command_player_effect(
                                            command_entity_state::EffectEdit::Add(Effect::new(
                                                23,
                                                eff.duration.max(1),
                                                eff.amplifier,
                                            )),
                                        );
                                    } else if (1..=23).contains(&eff.id) {
                                        let j = (d1 * f64::from(eff.duration) + 0.5) as i32;
                                        if j > 20 {
                                            self.command_player_effect(
                                                command_entity_state::EffectEdit::Add(Effect::new(
                                                    eff.id,
                                                    j,
                                                    eff.amplifier,
                                                )),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                    let mut prng =
                        crate::vanilla::random::JavaRandom::from_state(self.particle_random[2]);
                    for _ in 0..8 {
                        if self.particles.len() >= 4096 {
                            break;
                        }
                        let mut gaussian =
                            crate::vanilla::spawning::EntityRandom::from_state(prng.state(), None);
                        let motion = glam::DVec3::new(
                            gaussian.next_gaussian() * 0.15,
                            gaussian.random.next_double() * 0.2,
                            gaussian.next_gaussian() * 0.15,
                        );
                        prng = gaussian.random;
                        self.particles.push(Particle::item(
                            373,
                            potion,
                            false,
                            impact_d,
                            motion,
                            &mut prng,
                        ));
                    }
                    for _ in 0..100 {
                        if self.particles.len() >= 4096 {
                            break;
                        }
                        let radius = prng.next_double() * 4.0;
                        let angle = prng.next_double() * std::f64::consts::TAU;
                        let motion = glam::DVec3::new(
                            angle.cos() * radius,
                            0.01 + prng.next_double() * 0.5,
                            angle.sin() * radius,
                        );
                        let brightness = 0.75 + prng.next_float() * 0.25;
                        if let Some(p) = Particle::potion_splash_effect(
                            instant,
                            impact_d + glam::DVec3::new(motion.x * 0.1, 0.3, motion.z * 0.1),
                            motion,
                            radius as f32,
                            brightness,
                            rgb,
                            &mut prng,
                        ) {
                            self.particles.push(p);
                        }
                    }
                    self.particle_random[2] = prng.state();
                    let smash_pitch = 1.0 / (prng.next_float() * 0.2 + 0.9);
                    out.world_sounds
                        .push(("game.potion.smash", impact_d, 1.0, smash_pitch));
                }
                _ => {}
            }
        }
    }
}
