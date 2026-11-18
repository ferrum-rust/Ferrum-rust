//! Local player attacks enter here after the normal left-click picker chooses a living target.
use super::*;
use glam::DVec3;

fn melee_power(
    held: Option<&Stack>,
    victim: MobKind,
    effects: &Effects,
    critical: bool,
) -> (f32, Tool, i32, bool) {
    let (kind, tier, _, _) = tool(held.map_or(0, |s| s.id));
    // Gold shares wooden damage even though its mining tier/speed is different.
    let tier = f64::from(if tier == 5 { 1 } else { tier });
    let base = match kind {
        Tool::Sword => 4. + tier,
        Tool::Axe => 3. + tier,
        Tool::Pick => 2. + tier,
        Tool::Shovel => 1. + tier,
        _ => 1.,
    };
    let level = |id| held.map_or(0, |s| s.enchantment_level(id));
    let weakness = effects
        .get(18)
        .map_or(0., |e| 0.5 * f64::from(e.amplifier.wrapping_add(1)));
    let strength = effects
        .get(5)
        .map_or(0., |e| 1.3 * f64::from(e.amplifier.wrapping_add(1)));
    let mut amount = ((base - weakness) * (1. + strength)).clamp(0., 2048.) as f32;
    if critical {
        amount *= 1.5;
    }
    let mut bonus = 1.25 * level(16) as f32;
    if matches!(victim, MobKind::Skeleton | MobKind::Zombie | MobKind::PigZombie | MobKind::Wither) {
        bonus += 2.5 * level(17) as f32;
    } else if matches!(victim, MobKind::Spider | MobKind::CaveSpider | MobKind::Silverfish | MobKind::Endermite) {
        bonus += 2.5 * level(18) as f32;
    }
    (amount + bonus, kind, level(19), bonus > 0.)
}

pub(super) fn defended_damage(
    amount: f32,
    kind: MobKind,
    data: Option<&mut command_entity_state::MobData>,
    source: HitSource,
    random: &mut JavaRandom,
) -> f32 {
    let equipment = data
        .as_ref()
        .into_iter()
        .flat_map(|d| d.equipment.range(99..=103))
        .filter_map(|(_, slot)| slot.as_ref())
        .map(Stack::combat_item)
        .collect::<Vec<_>>();
    let points: i32 = equipment.iter().map(|s| armor::armor_points(s.id)).sum();
    let points = match kind {
        MobKind::Zombie | MobKind::PigZombie => (points + 2).min(20),
        MobKind::MagmaCube => (3 * species_ai::slime_size(data.as_deref())).min(20),
        MobKind::Horse => match data.as_ref().and_then(|d| d.extra.get("ArmorItem"))
            .and_then(local_commands::read_stack).map(|s| s.id) {
            Some(417) => 5,
            Some(418) => 7,
            Some(419) => 11,
            _ => 0,
        },
        _ => points,
    };
    let resistance = data
        .as_ref()
        .and_then(|d| d.effects.get(11))
        .map(|e| e.amplifier);
    let mut damage = potions::resistance_damage(
        if source.unblockable { amount } else { amount * (25 - points) as f32 / 25. },
        resistance,
        false,
        false,
    );
    if damage > 0. {
        let protection = armor::protection(
            equipment.iter().map(|s| s.enchantments.as_slice()),
            source,
            random,
        )
        .min(20);
        if protection > 0 {
            damage = damage * (25 - protection) as f32 / 25.;
        }
    }
    if let Some(d) = data {
        let health_damage = (damage - d.absorption).max(0.);
        d.absorption = (d.absorption - damage).max(0.);
        health_damage
    } else {
        damage
    }
}

pub(super) fn recoil(position: DVec3, velocity: DVec3, attacker: DVec3, math_random: &mut u64) -> DVec3 {
    let mut dx = attacker.x - position.x;
    let mut dz = attacker.z - position.z;
    if dx * dx + dz * dz < 1e-4 {
        let mut random = JavaRandom::from_state(*math_random);
        while dx * dx + dz * dz < 1e-4 {
            dx = (random.next_double() - random.next_double()) * 0.01;
            dz = (random.next_double() - random.next_double()) * 0.01;
        }
        *math_random = random.state();
    }
    let length = f64::from((dx * dx + dz * dz).sqrt() as f32);
    let impulse = f64::from(0.4f32);
    DVec3::new(
        velocity.x / 2. - dx / length * impulse,
        (velocity.y / 2. + impulse).min(impulse),
        velocity.z / 2. - dz / length * impulse,
    )
}

pub(crate) fn dragon_part_boxes(
    mob: &Mob,
    brain: Option<&ai::MobBrain>,
) -> [(usize, DVec3, crate::vanilla::movement::Aabb); 7] {
    let pos = mob.position.as_dvec3();
    let yaw = mob.yaw.to_degrees();
    let centers = if let Some(flight) = brain.and_then(|b| b.dragon_flight.as_deref()) {
        flight.parts(pos, yaw)
    } else {
        crate::remote::dragon::Flight::new(pos.y, yaw).parts(pos, yaw)
    };
    std::array::from_fn(|idx| {
        let center = centers[idx];
        let [w, h] = crate::remote::dragon::SIZES[idx];
        (idx, center, crate::vanilla::entity_pick::body(center, w, h))
    })
}

pub(crate) fn veer_dragon_on_hit(mob: &Mob, brain: &mut ai::MobBrain) {
    let state = brain
        .entity_random
        .get_or_insert_with(|| JavaRandom::new(JavaRandom::independent_seed()).state());
    let mut rng = JavaRandom::from_state(*state);
    let f3 = crate::vanilla::math::sin(mob.yaw);
    let f4 = crate::vanilla::math::cos(mob.yaw);
    let tx = mob.position.x + f3 * 5.0 + (rng.next_float() - 0.5) * 2.0;
    let ty = mob.position.y + rng.next_float() * 3.0 + 1.0;
    let tz = mob.position.z - f4 * 5.0 + (rng.next_float() - 0.5) * 2.0;
    *state = rng.state();
    brain.hover_target = Some(Vec3::new(tx, ty, tz));
    brain.dragon_chasing = false;
}

// EntityLivingBase applies the stronger-hit difference during the hurt window
// before armor, Protection, Resistance and absorption.
pub(super) fn apply_mob_hit(
    mob: &mut Mob, brain: &mut ai::MobBrain, data: Option<&mut command_entity_state::MobData>,
    raw: f32, source: HitSource, enchantment_state: &mut u64,
) -> Option<bool> {
    if mob.kind == MobKind::Wither && species_ai::number(data.as_deref(),"Invul")>0 {return None;}
    if mob.kind == MobKind::EnderDragon
        && (brain.dragon_death_ticks > 0
            || (!source.has_attacker && !source.explosion && !source.creative_harm))
    {
        return None;
    }
    if source.fire && data.as_ref().is_some_and(|d| d.effects.get(12).is_some()) {
        return None;
    }
    if matches!(mob.kind, MobKind::Cow | MobKind::Pig | MobKind::Sheep | MobKind::Chicken) {
        brain.love = 0;
    }
    let fresh = mob.hurt == 0;
    if mob.health <= 0. || (!fresh && raw <= brain.last_damage) {
        return None;
    }
    let accepted = if fresh { raw } else { raw - brain.last_damage };
    brain.last_damage = raw;
    let mut enchantment = JavaRandom::from_state(*enchantment_state);
    mob.health = (mob.health - defended_damage(accepted, mob.kind, data, source, &mut enchantment)).max(0.);
    *enchantment_state = enchantment.state();
    if fresh {
        mob.hurt = 10;
        brain.velocity_changed = true;
    }
    Some(fresh)
}

pub(super) fn mob_hurt_feedback<'a>(
    mob: &Mob, brain: &'a mut ai::MobBrain, villager_random: Option<&'a mut u64>,
    knockback: bool, silent: bool, out: &mut Outcome,
) {
    let random_state = villager_random.unwrap_or_else(|| {
        brain.entity_random.get_or_insert_with(|| JavaRandom::new(JavaRandom::independent_seed()).state())
    });
    let mut rng = JavaRandom::from_state(*random_state);
    rng.next_double(); // setBeenAttacked
    if knockback { rng.next_double(); }
    let pitch = (rng.next_float() - rng.next_float()) * 0.2
        + if brain.growing_age < 0 { 1.5 } else { 1. };
    *random_state = rng.state();
    if !silent {
        let death = mob.health == 0.;
        let name = match (mob.kind, death) {
            (MobKind::EnderDragon, _) => "mob.enderdragon.hit",
            (MobKind::Cow, _) => "mob.cow.hurt",
            (MobKind::Chicken, _) => "mob.chicken.hurt",
            (MobKind::Sheep, _) => "mob.sheep.say",
            (MobKind::Pig, false) => "mob.pig.say",
            (MobKind::Pig, true) => "mob.pig.death",
            (MobKind::Zombie, false) => "mob.zombie.hurt",
            (MobKind::Zombie, true) => "mob.zombie.death",
            (MobKind::Creeper, false) => "mob.creeper.say",
            (MobKind::Creeper, true) => "mob.creeper.death",
            (MobKind::Skeleton, false) => "mob.skeleton.hurt",
            (MobKind::Skeleton, true) => "mob.skeleton.death",
            (MobKind::Spider, false) => "mob.spider.say",
            (MobKind::Spider, true) => "mob.spider.death",
            (MobKind::Villager, false) => "mob.villager.hit",
            (MobKind::Villager, true) => "mob.villager.death",
            (kind, death) => match species_ai::hit_voice(kind, death) { Some(sound) => sound, None => return },
        };
        out.world_sounds.push((
            name,
            mob.position.as_dvec3(),
            if mob.kind == MobKind::Cow {
                0.4
            } else if mob.kind == MobKind::EnderDragon {
                5.
            } else {
                1.
            },
            pitch,
        ));
    }
}

impl Simulation {
    pub(super) fn attack_picked_mob(
        &mut self, i: usize, player: &mut Player, world: &World, particle_setting: u8, out: &mut Outcome,
    ) {
        let mob = &mut self.mobs[i];
        let data = self.command_entities.mobs.get_mut(&mob.id);
        if data.as_ref().is_some_and(|d| d.invulnerable) && self.mode != Mode::Creative {
            return;
        }
        let critical = self.fall_distance > 0.
            && !player.grounded
            && !player.on_ladder
            && !player.in_water
            && !player.third_person.riding
            && self.effects.get(15).is_none();
        let (mut raw, kind, knockback, magic) = melee_power(
            self.inventory.slots[self.inventory.selected].as_ref(),
            mob.kind,
            &self.effects,
            critical,
        );
        if raw <= 0. {
            return;
        }
        let brain = self.mob_brains.entry(mob.id).or_default();
        if mob.kind == MobKind::EnderDragon {
            let creative = self.mode == Mode::Creative;
            let view = player.pick_view(creative, 1.0);
            let reach = if creative { 6.0 } else { 3.0 };
            let end = view.eye + view.look * reach;
            let mut best_part = 1usize;
            let mut best_d2 = f64::INFINITY;
            for (part_idx, _, part_box) in dragon_part_boxes(mob, Some(brain)) {
                let lo = part_box.min - DVec3::splat(f64::from(0.1f32));
                let hi = part_box.max + DVec3::splat(f64::from(0.1f32));
                if view.eye.x >= lo.x
                    && view.eye.x <= hi.x
                    && view.eye.y >= lo.y
                    && view.eye.y <= hi.y
                    && view.eye.z >= lo.z
                    && view.eye.z <= hi.z
                {
                    best_part = part_idx;
                    break;
                }
                if let Some((_, p)) =
                    crate::vanilla::ray_trace::intercept([lo.x, lo.y, lo.z, hi.x, hi.y, hi.z], view.eye, end)
                {
                    let d2 = p.distance_squared(view.eye);
                    if d2 < best_d2 {
                        best_d2 = d2;
                        best_part = part_idx;
                    }
                }
            }
            if best_part != 0 {
                raw = raw / 4.0 + 1.0;
            }
        }
        let animal = matches!(
            mob.kind,
            MobKind::Cow | MobKind::Pig | MobKind::Sheep | MobKind::Chicken
        );
        let silent = data.as_ref().is_some_and(|d| d.silent);
        let fire_aspect = self.inventory.held().map_or(0, |s| s.enchantment_level(20));
        let protection = burn::mob_protection(data.as_deref());
        let immune = burn::fire_immune(mob.kind, data.as_deref());
        let hit_source = HitSource {
            has_attacker: true,
            ..Default::default()
        };
        let Some(fresh) = apply_mob_hit(mob, brain, data, raw, hit_source, &mut self.enchantment_random_state) else {
            return;
        };
        let can_provoke = self.mode == Mode::Survival && !player.is_spectator();
        if mob.kind == MobKind::Enderman {
            if can_provoke {
                brain.player_hit_ticks = 100;
                brain.anger_ticks = 400;
                brain.enderman_screaming = true;
                brain.enderman_unseen_ticks = 0;
            }
        } else {
            brain.player_hit_ticks = 100;
            if matches!(mob.kind, MobKind::Wolf | MobKind::PigZombie | MobKind::IronGolem) {
                brain.anger_ticks = 400 + (random(&mut self.rng) % 400) as u16;
            }
        }
        if fire_aspect > 0 && !immune {
            burn::set_fire(&mut brain.fire_ticks, fire_aspect * 4, protection);
        }

        if mob.kind == MobKind::Villager {
            use crate::villagers::homes::reputation::{Death, ResidentActor};
            let home = self
                .trades
                .get(&mob.id)
                .and_then(|trader| trader.career.home);
            let actor = ResidentActor {
                entity: 0,
                player: Some(self.local_name.clone()),
            };
            self.villager_homes.hurt(
                home,
                Some(actor.clone()),
                brain.growing_age < 0,
                mob.health > 0.,
            );
            if mob.health == 0. {
                self.villager_homes.killed(
                    home,
                    Death::Entity {
                        actor: &actor,
                        hostile: false,
                    },
                );
            }
        }
        if animal {
            brain.hurt(player.position);
        }
        if mob.kind == MobKind::EnderDragon {
            veer_dragon_on_hit(mob, brain);
            if fresh {
                mob_hurt_feedback(mob, brain, None, false, silent, out);
            }
            let knockback = knockback + i32::from(player.sprinting);
            if knockback > 0 {
                let [x, y, z] = player.precise_velocity();
                player.set_velocity_f64([x * 0.6, y, z * 0.6]);
                player.sprinting = false;
            }
        } else {
            let mut velocity = brain
                .movement
                .as_ref()
                .map(|m| m.body.velocity)
                .filter(|v| v.as_vec3() == mob.velocity)
                .unwrap_or_else(|| mob.velocity.as_dvec3());
            if fresh {
                let position = brain
                    .movement
                    .as_ref()
                    .map(|m| m.body.position)
                    .filter(|v| v.as_vec3() == mob.position)
                    .unwrap_or_else(|| mob.position.as_dvec3());
                velocity = recoil(
                    position,
                    velocity,
                    DVec3::from_array(player.precise_position()),
                    &mut self.xp_math_random,
                );
                let random = self.trades.get_mut(&mob.id).map(|v| &mut v.career.random_state);
                mob_hurt_feedback(mob, brain, random, true, silent, out);
            }
            let knockback = knockback + i32::from(player.sprinting);
            if knockback > 0 {
                use crate::vanilla::math::{cos, sin};
                let yaw = player.java_rotation()[0] * std::f32::consts::PI / 180.;
                velocity += DVec3::new(
                    f64::from(-sin(yaw) * knockback as f32 * 0.5),
                    0.1,
                    f64::from(cos(yaw) * knockback as f32 * 0.5),
                );
                let [x, y, z] = player.precise_velocity();
                player.set_velocity_f64([x * 0.6, y, z * 0.6]);
                player.sprinting = false;
                brain.velocity_changed = true;
            }
            mob.velocity = velocity.as_vec3();
            if let Some(body) = &mut brain.movement {
                body.body.velocity = velocity;
            }
        }
        if critical || magic {
            let (w, h) = mob.kind.adult_size(self.command_entities.mobs.get(&mob.id));
            let scale = if brain.growing_age < 0 { 0.5 } else { 1. };
            let bounds = crate::vanilla::entity_pick::body(mob.position.as_dvec3(), w * scale, h * scale);
            for (enabled, enchantment) in [(critical, false), (magic, true)] {
                if enabled {
                    self.critical_emitters.start(mob.id, bounds, enchantment, &mut self.particles,
                        world, player.position.as_dvec3(), particle_setting);
                }
            }
        }
        let wear = match kind {
            Tool::Sword => 1,
            Tool::Pick | Tool::Axe | Tool::Shovel => 2,
            _ => 0,
        };
        if wear > 0 {
            self.wear_slot(self.inventory.selected, wear, false);
        }
        let stat = self
            .statistics
            .entry("stat.damageDealt".into())
            .or_default();
        *stat = stat.wrapping_add((raw * 10.).round() as i32);
        self.add_exhaustion(0.3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vanilla_cow_melee_and_panic_revenge_window() {
        use serde_json::Value;
        let cases: Vec<(u8, Vec<Vec<Value>>)> = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-animal-melee.json"
        ))
        .unwrap();
        for (case, rows) in cases {
            let mut sim = Simulation::new(
                if case == 3 {
                    Mode::Creative
                } else {
                    Mode::Survival
                },
                189,
            );
            let p = Vec3::new(6., 42., 8.);
            sim.mobs.push(Mob {
                id: 1,
                kind: MobKind::Cow,
                position: p,
                previous: p,
                velocity: Vec3::new(0.1, -0.2, 0.3),
                yaw: 0.,
                health: 10.,
                age: 0,
                turn_ticks: 0,
                hurt: 0,
                fuse: 0,
                swing_ticks: 0,
                death_ticks: 0,
                eat_ticks: 0,
                attack_ticks: 0,
                rose_ticks: 0,
                prev_fuse: 0,
            });
            sim.mob_brains.entry(1).or_default().love = 600;
            let data = sim.command_entities.mobs.entry(1).or_default();
            data.invulnerable = matches!(case, 2 | 3);
            data.no_ai = case == 4;
            let mut player = Player::new(Vec3::new(3., 42., 4.));
            player.set_pose_f64([3., 42., 4.], 90., 0., true);
            let mut events = Outcome::default();
            for (i, row) in rows.iter().enumerate() {
                let n = match i {
                    3 => 10,
                    4 => 101,
                    _ => 0,
                };
                for _ in 0..n {
                    sim.mobs[0].hurt = sim.mobs[0].hurt.saturating_sub(1);
                    sim.mob_brains.get_mut(&1).unwrap().tick_revenge();
                }
                if i != 4 {
                    let item = row[0].as_u64().unwrap() as u16;
                    sim.inventory.slots[0] = (item != 0).then(|| Stack::new(item, 1));
                    sim.attack_picked_mob(0, &mut player, &super::super::tests::flat_world(), 0, &mut events);
                }
                let mob = &sim.mobs[0];
                let brain = &sim.mob_brains[&1];
                assert_eq!(
                    f64::from(mob.health),
                    row[1].as_f64().unwrap(),
                    "{case}/{i} health"
                );
                assert_eq!(
                    u64::from(mob.hurt),
                    row[2].as_u64().unwrap(),
                    "{case}/{i} hurt"
                );
                for axis in 0..3 {
                    assert!(
                        (f64::from(mob.velocity[axis]) - row[3 + axis].as_f64().unwrap()).abs()
                            < 1e-6,
                        "{case}/{i} motion {axis}"
                    );
                }
                assert_eq!(
                    brain.panic > 0,
                    row[6].as_bool().unwrap(),
                    "{case}/{i} revenge"
                );
                assert_eq!(
                    brain.love > 0,
                    row[7].as_bool().unwrap(),
                    "{case}/{i} in love"
                );
                assert_eq!(
                    brain.panic > 0,
                    row[8].as_bool().unwrap(),
                    "{case}/{i} panic"
                );
                assert_eq!(
                    u64::from(sim.inventory.slots[0].as_ref().map_or(0, |s| s.damage)),
                    row[9].as_u64().unwrap(),
                    "{case}/{i} wear"
                );
            }
        }
    }
}
