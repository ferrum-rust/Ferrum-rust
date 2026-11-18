use super::{command_entity_state::MobData, *};
use crate::vanilla::{drops::DropNbt, item_nbt, random::JavaRandom};
use MobKind::*;

pub(crate) fn number(data: Option<&MobData>, key: &str) -> i32 {
    item_nbt::number(data.and_then(|d| d.extra.get(key))) as i32
}

pub(crate) fn slime_size(data: Option<&MobData>) -> i32 {
    number(data, "Size").clamp(0, 126) + 1
}

pub(crate) fn enderman_can_see_player(m: &Mob, player: &Player, world: &World) -> bool {
    let eye = player.precise_eye(1.0);
    let target_eye = glam::DVec3::new(
        f64::from(m.position.x),
        f64::from(m.position.y) + 2.55,
        f64::from(m.position.z),
    );
    crate::vanilla::ray_trace::trace(
        world,
        eye,
        target_eye,
        crate::vanilla::ray_trace::Options {
            stop_on_liquid: false,
            ignore_without_box: true,
            return_last_miss: false,
        },
    )
    .is_none()
}

pub(crate) fn enderman_stared_at_with_los(
    m: &Mob,
    player: &Player,
    pumpkin: bool,
    can_see: bool,
) -> bool {
    if pumpkin || !can_see {
        return false;
    }
    let vec3 = player.direction().as_dvec3().normalize_or_zero();
    let target_mid = glam::DVec3::new(
        f64::from(m.position.x),
        f64::from(m.position.y) + 1.45,
        f64::from(m.position.z),
    );
    let vec31 = target_mid - player.precise_eye(1.0);
    let d0 = vec31.length();
    if d0 <= 1.0e-6 {
        return false;
    }
    let d1 = vec3.dot(vec31 / d0);
    d1 > 1.0 - 0.025 / d0
}

pub(crate) fn enderman_stared_at(
    m: &Mob,
    player: &Player,
    pumpkin: bool,
    world: &World,
) -> bool {
    if pumpkin {
        return false;
    }
    enderman_stared_at_with_los(m, player, pumpkin, enderman_can_see_player(m, player, world))
}

pub(super) fn pursues(
    m: &Mob,
    brain: &mut MobBrain,
    data: Option<&MobData>,
    player: &Player,
    pumpkin: bool,
    can_target_player: bool,
    armor_visibility: f32,
    silent: bool,
    world: &World,
    sky_loss: u8,
    dimension: i32,
    out: &mut Outcome,
) -> bool {
    if m.kind == Enderman {
        if !can_target_player {
            brain.anger_ticks = 0;
            brain.stare_ticks = 0;
            brain.player_hit_ticks = 0;
            brain.enderman_screaming = false;
            brain.enderman_unseen_ticks = 0;
            brain.enderman_teleport_delay = 0;
            return false;
        }
        if brain.anger_ticks > 0 && !brain.enderman_screaming {
            brain.enderman_screaming = true;
        }
        let distance = m.position.distance(player.position);
        let can_see = enderman_can_see_player(m, player, world);
        if brain.enderman_screaming {
            if can_see {
                brain.enderman_unseen_ticks = 0;
            } else {
                brain.enderman_unseen_ticks = brain.enderman_unseen_ticks.saturating_add(1);
            }
            if distance > 64.0 || brain.enderman_unseen_ticks > 60 {
                brain.enderman_screaming = false;
                brain.anger_ticks = 0;
                brain.stare_ticks = 0;
                brain.enderman_unseen_ticks = 0;
                brain.enderman_teleport_delay = 0;
                return false;
            }
            brain.anger_ticks = brain.anger_ticks.max(400);
            return true;
        }
        let mut acquire_range = 64.0 * armor_visibility;
        if player.sneaking() {
            acquire_range *= 0.8;
        }
        let in_box = distance <= acquire_range && (m.position.y - player.position.y).abs() <= 4.0;
        let staring = in_box && enderman_stared_at_with_los(m, player, pumpkin, can_see);
        brain.stare_ticks = if staring {
            brain.stare_ticks.saturating_add(1)
        } else {
            0
        };
        if brain.stare_ticks >= 5 {
            brain.stare_ticks = 0;
            brain.enderman_screaming = true;
            brain.anger_ticks = 400;
            brain.enderman_unseen_ticks = 0;
            brain.enderman_teleport_delay = 0;
            if !silent {
                out.world_sounds
                    .push(("mob.endermen.stare", m.position.as_dvec3(), 1.0, 1.0));
            }
            return true;
        }
        return false;
    }
    if !can_target_player {
        return false;
    }
    match m.kind {
        PigZombie | Wolf => brain.anger_ticks > 0,
        IronGolem => number(data, "PlayerCreated") == 0 && brain.anger_ticks > 0,
        Rabbit => number(data, "RabbitType") == 99,
        Giant => false, // EntityGiantZombie installs no target or attack tasks.
        Spider | CaveSpider => {
            if brain.player_hit_ticks > 0 || brain.anger_ticks > 0 {
                true
            } else {
                let eyes = (m.position + Vec3::Y * (m.kind.size().1 * 0.65))
                    .floor()
                    .as_ivec3();
                let light = world.game_light(eyes, sky_loss, true, dimension == 0);
                let brightness = crate::vanilla::lightmap::brightness_table(dimension)
                    [usize::from(light.min(15))];
                brightness <= 0.5
            }
        }
        _ => m.kind.hostile(),
    }
}

pub(super) fn targets_other(kind: MobKind, other: MobKind, data: Option<&MobData>) -> bool {
    match kind {
        IronGolem | SnowGolem => other.hostile() && !(kind == IronGolem && other == Creeper),
        Wolf => matches!(other, Sheep | Rabbit | Skeleton) && !tamed(data),
        Ocelot => other == Chicken && !tamed(data),
        Wither => !matches!(other, Wither | Zombie | Skeleton | PigZombie | EnderDragon),
        Guardian => other == Squid,
        _ => false,
    }
}

pub(crate) fn tamed(data: Option<&MobData>) -> bool {
    data.is_some_and(
        |data| matches!(data.extra.get("OwnerUUID"), Some(DropNbt::String(s)) if !s.is_empty()),
    )
}

pub(super) fn melee_damage(kind: MobKind, data: Option<&MobData>, rng: &mut u64) -> Option<f32> {
    let mut dmg = match kind {
        Zombie => 3.,
        PigZombie => 5.,
        Enderman => 7.,
        Blaze => 6.,
        Skeleton if number(data, "SkeletonType") == 1 => {
            let sword_bonus = match data
                .and_then(|d| d.equipment.get(&99))
                .and_then(Option::as_ref)
                .map(|s| s.id)
            {
                Some(268 | 283) => 2.,
                Some(272) => 3.,
                Some(267) => 4.,
                Some(276) => 5.,
                _ => 0.,
            };
            4. + sword_bonus
        }
        Spider | CaveSpider | Endermite | Wolf => 2.,
        Silverfish => 1.,
        Slime if slime_size(data) > 1 => slime_size(data) as f32,
        MagmaCube => (slime_size(data) + 2) as f32,
        IronGolem => 7. + (random(rng) % 15) as f32,
        Rabbit if number(data, "RabbitType") == 99 => 8.,
        Ocelot => 3.,
        EnderDragon => 10.,
        _ => return None,
    };
    if let Some(eff) = data.and_then(|d| d.effects.get(18)) {
        dmg -= 0.5 * (eff.amplifier.wrapping_add(1) as f32);
    }
    if let Some(eff) = data.and_then(|d| d.effects.get(5)) {
        dmg *= 1.0 + 1.3 * (eff.amplifier.wrapping_add(1) as f32);
    }
    (dmg > 0.0).then_some(dmg)
}

pub(super) fn ambient_for(kind: MobKind, brain: Option<&MobBrain>) -> Option<&'static str> {
    if kind == Enderman && brain.is_some_and(|b| b.enderman_screaming) {
        return Some("mob.endermen.scream");
    }
    ambient(kind)
}

pub(super) fn ambient(kind: MobKind) -> Option<&'static str> {
    Some(match kind {
        Cow | Mooshroom => "mob.cow.say",
        Pig => "mob.pig.say",
        Sheep => "mob.sheep.say",
        Chicken => "mob.chicken.say",
        Zombie => "mob.zombie.say",
        Skeleton => "mob.skeleton.say",
        Spider | CaveSpider => "mob.spider.say",
        PigZombie => "mob.zombiepig.zpig",
        Ghast => "mob.ghast.moan",
        Blaze => "mob.blaze.breathe",
        Enderman => "mob.endermen.idle",
        Silverfish | Endermite => "mob.silverfish.say",
        Wither => "mob.wither.idle",
        Bat => "mob.bat.idle",
        EnderDragon => "mob.enderdragon.growl",
        Guardian => "mob.guardian.idle",
        Horse => "mob.horse.idle",
        Rabbit => "mob.rabbit.idle",
        Wolf => "mob.wolf.bark",
        _ => return None,
    })
}

pub(super) fn hit_voice(kind: MobKind, death: bool) -> Option<&'static str> {
    let (hit, end) = match kind {
        Mooshroom => ("mob.cow.hurt", "mob.cow.hurt"),
        CaveSpider => ("mob.spider.say", "mob.spider.death"),
        Rabbit => ("mob.rabbit.hurt", "mob.rabbit.death"),
        Horse => ("mob.horse.hit", "mob.horse.death"),
        Wolf => ("mob.wolf.hurt", "mob.wolf.death"),
        Ocelot => ("mob.cat.hitt", "mob.cat.hitt"),
        Bat => ("mob.bat.hurt", "mob.bat.death"),
        Ghast => ("mob.ghast.scream", "mob.ghast.death"),
        Blaze => ("mob.blaze.hit", "mob.blaze.death"),
        Enderman => ("mob.endermen.hit", "mob.endermen.death"),
        Endermite | Silverfish => ("mob.silverfish.hit", "mob.silverfish.kill"),
        Guardian => ("mob.guardian.hit", "mob.guardian.death"),
        PigZombie => ("mob.zombiepig.zpighurt", "mob.zombiepig.zpigdeath"),
        IronGolem => ("mob.irongolem.hit", "mob.irongolem.death"),
        Wither => ("mob.wither.hurt", "mob.wither.death"),
        EnderDragon => ("mob.enderdragon.hit", "mob.enderdragon.end"),
        Slime | MagmaCube => ("mob.slime.small", "mob.slime.small"),
        _ => return None,
    };
    Some(if death { end } else { hit })
}

pub(super) fn loot(
    kind: MobKind,
    data: Option<&MobData>,
    burning: bool,
    player_kill: bool,
    looting: i32,
    rng: &mut JavaRandom,
) -> Vec<Stack> {
    let mut result = Vec::new();
    let mut drop = |id, count: i32| {
        if count > 0 {
            result.push(Stack::new(id, count as u8));
        }
    };
    let bonus = looting.clamp(0, 127);
    match kind {
        Ghast => {
            drop(370, rng.next_int(2) + rng.next_int(1 + bonus));
            drop(289, rng.next_int(3) + rng.next_int(1 + bonus));
        }
        PigZombie => {
            drop(367, rng.next_int(2 + bonus));
            drop(371, rng.next_int(2 + bonus));
        }
        Enderman => drop(368, rng.next_int(2 + bonus)),
        Blaze if player_kill => drop(369, rng.next_int(2 + bonus)),
        Wither => drop(399, 1),
        IronGolem => {
            drop(38, rng.next_int(3));
            drop(265, 3 + rng.next_int(3));
        }
        SnowGolem => drop(332, rng.next_int(16)),
        Rabbit => {
            drop(415, rng.next_int(2) + rng.next_int(1 + bonus));
            drop(if burning { 412 } else { 411 }, rng.next_int(2));
        }
        Squid => drop(351, 1 + rng.next_int(3 + bonus)),
        Mooshroom => {
            drop(334, rng.next_int(3) + rng.next_int(1 + bonus));
            drop(
                if burning { 364 } else { 363 },
                1 + rng.next_int(3) + rng.next_int(1 + bonus),
            );
        }
        Slime if slime_size(data) == 1 => drop(341, rng.next_int(3) + rng.next_int(1 + bonus)),
        MagmaCube if slime_size(data) > 1 => {
            drop(378, rng.next_int(4) - 2 + rng.next_int(1 + bonus))
        }
        Skeleton if number(data, "SkeletonType") == 1 => {
            drop(263, rng.next_int(3) - 1 + rng.next_int(1 + bonus));
            drop(352, rng.next_int(3) + rng.next_int(1 + bonus));
            if rng.next_int(200) < 5 + 2 * bonus {
                let mut skull = Stack::new(397, 1);
                skull.meta = 1;
                result.push(skull);
            }
        }
        Spider | CaveSpider => {
            drop(287, rng.next_int(3) + rng.next_int(1 + bonus));
            if player_kill && (rng.next_int(3) == 0 || rng.next_int(1 + bonus) > 0) {
                drop(375, 1);
            }
        }
        Horse => {
            let skip = rng.next_int(4) == 0;
            let id = match number(data, "Type") {
                4 => 352,
                3 if skip => 0,
                3 => 367,
                _ => 334,
            };
            if id != 0 {
                drop(id, rng.next_int(3) + rng.next_int(1 + bonus));
            }
            result.extend(super::horse::drops(data));
        }
        Guardian => {
            drop(409, rng.next_int(3) + rng.next_int(1 + bonus));
            if rng.next_int(3 + bonus) > 1 {
                drop(349, 1)
            } else if rng.next_int(3 + bonus) > 1 {
                drop(410, 1)
            };
            if player_kill && number(data, "Elder") != 0 {
                result.push(Stack::block(19 << 4 | 1));
            }
        }
        Witch => {
            let drops = [348, 353, 331, 375, 374, 289, 280, 280];
            for _ in 0..1 + rng.next_int(3) {
                let count = rng.next_int(3);
                let id = drops[rng.next_int(8) as usize];
                drop(id, count + rng.next_int(bonus + 1));
            }
        }
        _ => {}
    }
    result
}

/// EntityEnderman.teleportTo first finds a supporting block, then tests its full body for collisions and liquid.
pub(crate) fn teleport_enderman_to(
    world: &World,
    mob: &mut Mob,
    brain: &mut MobBrain,
    requested: glam::DVec3,
    particle_rng: &mut u64,
    particles: &mut Vec<crate::gameplay::click_particles::Particle>,
    silent: bool,
    out: &mut Outcome,
) -> bool {
    use crate::vanilla::movement::{self, Aabb, MovementWorld};
    let old_pos = mob.position.as_dvec3();
    let bx = requested.x.floor() as i32;
    let mut by = requested.y.floor() as i32;
    let bz = requested.z.floor() as i32;
    if !world.loaded_column(bx, bz) {
        return false;
    }
    let mut found_ground = false;
    while !found_ground && by > 0 {
        if crate::vanilla::material::blocks_movement(world.get(IVec3::new(bx, by - 1, bz))) {
            found_ground = true;
        } else {
            by -= 1;
        }
    }
    if !found_ground {
        return false;
    }
    let final_pos = glam::DVec3::new(requested.x, f64::from(by), requested.z);
    let (w, h) = mob.kind.size();
    let half = f64::from(w) * 0.5;
    let bounds = Aabb {
        min: final_pos - glam::DVec3::new(half, 0., half),
        max: final_pos + glam::DVec3::new(half, f64::from(h), half),
    };
    if !movement::collision::colliders(world, bounds).is_empty()
        || movement::any_liquid(world, bounds)
        || movement::touches_liquid(world, bounds, 8, 11)
    {
        return false;
    }
    mob.position = final_pos.as_vec3();
    mob.previous = mob.position;
    mob.velocity = Vec3::ZERO;
    brain.movement = None;
    brain.path_target = None;
    brain.navigator.clear();
    let mut prng = JavaRandom::from_state(*particle_rng);
    for j in 0..128 {
        let d6 = f64::from(j) / 127.0;
        let vx = f64::from((prng.next_float() - 0.5) * 0.2);
        let vy = f64::from((prng.next_float() - 0.5) * 0.2);
        let vz = f64::from((prng.next_float() - 0.5) * 0.2);
        let px = final_pos.x
            + (old_pos.x - final_pos.x) * d6
            + (prng.next_double() - 0.5) * f64::from(w) * 2.0;
        let py = final_pos.y + (old_pos.y - final_pos.y) * d6 + prng.next_double() * f64::from(h);
        let pz = final_pos.z
            + (old_pos.z - final_pos.z) * d6
            + (prng.next_double() - 0.5) * f64::from(w) * 2.0;
        if particles.len() < 4096 {
            if let Some(p) = crate::gameplay::click_particles::Particle::network_sprite(
                24,
                glam::DVec3::new(px, py, pz),
                glam::DVec3::new(vx, vy, vz),
                &mut prng,
            ) {
                particles.push(p);
            }
        }
    }
    *particle_rng = prng.state();
    if !silent {
        out.world_sounds
            .push(("mob.endermen.portal", old_pos, 1.0, 1.0));
        out.world_sounds
            .push(("mob.endermen.portal", final_pos, 1.0, 1.0));
    }
    true
}

pub(crate) fn teleport_enderman_randomly(
    world: &World,
    mob: &mut Mob,
    brain: &mut MobBrain,
    rng: &mut u64,
    particle_rng: &mut u64,
    particles: &mut Vec<crate::gameplay::click_particles::Particle>,
    silent: bool,
    out: &mut Outcome,
) -> bool {
    let origin = mob.position.as_dvec3();
    let mut jrng = JavaRandom::new(random(rng) as i64);
    for _ in 0..64 {
        let tx = origin.x + (jrng.next_double() - 0.5) * 64.0;
        let ty = origin.y + f64::from(jrng.next_int(64) - 32);
        let tz = origin.z + (jrng.next_double() - 0.5) * 64.0;
        if teleport_enderman_to(
            world,
            mob,
            brain,
            glam::DVec3::new(tx, ty, tz),
            particle_rng,
            particles,
            silent,
            out,
        ) {
            return true;
        }
    }
    false
}

pub(crate) fn teleport_enderman_to_entity(
    world: &World,
    mob: &mut Mob,
    brain: &mut MobBrain,
    target_eye: glam::DVec3,
    rng: &mut u64,
    particle_rng: &mut u64,
    particles: &mut Vec<crate::gameplay::click_particles::Particle>,
    silent: bool,
    out: &mut Outcome,
) -> bool {
    let origin = mob.position.as_dvec3();
    let self_mid = glam::DVec3::new(origin.x, origin.y + 1.45, origin.z);
    let dir = (self_mid - target_eye).normalize_or_zero();
    let mut jrng = JavaRandom::new(random(rng) as i64);
    for _ in 0..64 {
        let tx = origin.x + (jrng.next_double() - 0.5) * 8.0 - dir.x * 16.0;
        let ty = origin.y + f64::from(jrng.next_int(16) - 8) - dir.y * 16.0;
        let tz = origin.z + (jrng.next_double() - 0.5) * 8.0 - dir.z * 16.0;
        if teleport_enderman_to(
            world,
            mob,
            brain,
            glam::DVec3::new(tx, ty, tz),
            particle_rng,
            particles,
            silent,
            out,
        ) {
            return true;
        }
    }
    false
}

pub(super) fn enderman_tick(
    world: &World,
    mob: &mut Mob,
    brain: &mut MobBrain,
    dimension: i32,
    sky_loss: u8,
    wet: bool,
    silent: bool,
    rng: &mut u64,
    particle_rng: &mut u64,
    particles: &mut Vec<crate::gameplay::click_particles::Particle>,
    out: &mut Outcome,
) {
    if wet {
        if mob.hurt == 0 {
            mob.health = (mob.health - 1.).max(0.);
            mob.hurt = 10;
            if !silent {
                if let Some(voice) = hit_voice(Enderman, mob.health <= 0.) {
                    out.world_sounds
                        .push((voice, mob.position.as_dvec3(), 1.0, 1.0));
                }
            }
        }
        teleport_enderman_randomly(world, mob, brain, rng, particle_rng, particles, silent, out);
        return;
    }
    if dimension == 0 && sky_loss < 4 {
        let block_pos = IVec3::new(
            mob.position.x.floor() as i32,
            mob.position.y.round() as i32,
            mob.position.z.floor() as i32,
        );
        let light = world.game_light(block_pos, sky_loss, true, true);
        let bright = crate::vanilla::lightmap::brightness_table(0)[usize::from(light.min(15))];
        if bright > 0.5
            && world.can_see_sky(block_pos)
            && unit(rng) * 30.0 < (bright - 0.4) * 2.0
        {
            brain.anger_ticks = 0;
            brain.stare_ticks = 0;
            brain.player_hit_ticks = 0;
            brain.enderman_screaming = false;
            brain.enderman_unseen_ticks = 0;
            brain.enderman_teleport_delay = 0;
            teleport_enderman_randomly(
                world,
                mob,
                brain,
                rng,
                particle_rng,
                particles,
                silent,
                out,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gameplay::mob_shots::Payload;

    fn scene() -> (World, Simulation, Player) {
        let world = super::super::tests::flat_world();
        let mut sim = Simulation::new(Mode::Survival, 189);
        sim.mob_spawning = false;
        sim.world_difficulty = Some(2);
        sim.clock = 18000;
        let player = Player::new(Vec3::new(8.5, 1., 8.5));
        (world, sim, player)
    }

    #[test]
    fn species_attacks_and_passive_controls_follow_the_local_tick() {
        for (kind, expected) in [
            (Silverfish, Some(1.)),
            (PigZombie, None),
            (Wolf, None),
            (Rabbit, None),
            (Giant, None),
            (CaveSpider, Some(2.)),
        ] {
            let (mut world, mut sim, mut player) = scene();
            sim.spawn_registered_raw(kind, player.position + Vec3::X * 0.8, 0.);
            sim.tick_entities(
                &mut world,
                &mut player,
                &Settings::default(),
                &mut Outcome::default(),
            );
            assert_eq!(sim.health, 20. - expected.unwrap_or(0.), "{kind:?}");
            assert_eq!(sim.effects.get(19).is_some(), kind == CaveSpider);
        }
    }

    #[test]
    fn ranged_species_emit_their_own_projectile_and_wither_assembly_counts_down() {
        let (_, mut sim, player) = scene();
        for (kind, payload) in [
            (Blaze, Payload::SmallFireball),
            (Ghast, Payload::Fireball),
            (Witch, Payload::Splash(32660)),
            (Wither, Payload::Skull),
            (SnowGolem, Payload::Snowball),
        ] {
            let id = sim.spawn_registered_raw(kind, player.position + Vec3::Z * 6., 0.);
            let mob = sim.mobs.last().unwrap();
            let brain = sim.mob_brains.entry(id).or_default();
            let shot = (0..100)
                .find_map(|_| {
                    brain.tick();
                    mob_shots::Shot::aim(
                        mob,
                        brain,
                        player.eye(1.),
                        20.,
                        &sim.effects,
                        &mut sim.rng,
                    )
                })
                .unwrap();
            assert_eq!(shot.payload, payload);
        }
        let (mut world, mut sim, mut player) = scene();
        let id = sim.spawn_registered_raw(Wither, Vec3::new(4., 8., 4.), 0.);
        sim.mobs[0].health = 100.;
        sim.command_entities
            .mobs
            .entry(id)
            .or_default()
            .extra
            .insert("Invul".into(), DropNbt::Int(20));
        for _ in 0..19 {
            sim.tick_entities(
                &mut world,
                &mut player,
                &Settings::default(),
                &mut Outcome::default(),
            );
        }
        assert_eq!(sim.mobs[0].health, 110.);
        assert_eq!(number(sim.command_entities.mobs.get(&id), "Invul"), 1);
        sim.tick_entities(
            &mut world,
            &mut player,
            &Settings::default(),
            &mut Outcome::default(),
        );
        assert_eq!(number(sim.command_entities.mobs.get(&id), "Invul"), 0);
        assert_eq!(sim.mobs[0].health, 120.);
    }

    #[test]
    fn dragon_and_squid_steer_without_ground_navigation_and_slimes_split_on_death() {
        let (mut world, mut sim, mut player) = scene();
        sim.mode = Mode::Creative;
        for x in 1..7 {
            for y in 1..6 {
                for z in 1..7 {
                    world.set(IVec3::new(x, y, z), 9 << 4);
                }
            }
        }
        let squid = sim.spawn_registered_raw(Squid, Vec3::new(4., 3., 4.), 0.);
        sim.mob_brains.entry(squid).or_default().hover_target = Some(Vec3::new(4., 4.5, 5.));
        let dragon = sim.spawn_registered_raw(EnderDragon, Vec3::new(10., 21., 10.), 0.);
        sim.mob_brains.entry(dragon).or_default().hover_target = Some(Vec3::new(8., 90., 8.));
        for _ in 0..8 {
            sim.tick_entities(
                &mut world,
                &mut player,
                &Settings::default(),
                &mut Outcome::default(),
            );
        }
        assert!(sim.mobs.iter().find(|m| m.id == squid).unwrap().position.y > 3.);
        assert!(sim.mobs.iter().find(|m| m.id == dragon).unwrap().position.y > 21.);
        let id = sim.spawn_registered_raw(Slime, Vec3::new(8., 1., 8.), 0.);
        sim.command_entities
            .mobs
            .entry(id)
            .or_default()
            .extra
            .insert("Size".into(), DropNbt::Int(3));
        sim.mobs.last_mut().unwrap().health = 0.;
        sim.tick_entities(
            &mut world,
            &mut player,
            &Settings::default(),
            &mut Outcome::default(),
        );
        let children: Vec<_> = sim.mobs.iter().filter(|m| m.kind == Slime).collect();
        assert!((2..=4).contains(&children.len()));
        assert!(children
            .iter()
            .all(|m| m.health == 4. && slime_size(sim.command_entities.mobs.get(&m.id)) == 2));
    }

    #[test]
    fn spider_daylight_neutrality_wither_skeleton_melee_and_close_skeleton_arrows() {
        let (mut world, mut sim, mut player) = scene();
        sim.clock = 6000;
        let spider_id = sim.spawn_registered_raw(Spider, player.position + Vec3::X * 0.8, 0.);
        sim.tick_entities(
            &mut world,
            &mut player,
            &Settings::default(),
            &mut Outcome::default(),
        );
        assert_eq!(sim.health, 20., "daylight spider must stay passive");
        sim.mob_brains.entry(spider_id).or_default().player_hit_ticks = 40;
        sim.tick_entities(
            &mut world,
            &mut player,
            &Settings::default(),
            &mut Outcome::default(),
        );
        assert!(sim.health < 20., "provoked daylight spider must retaliate");

        let (mut world, mut sim, mut player) = scene();
        let wither_skel = sim.spawn_registered_raw(Skeleton, player.position + Vec3::X * 0.9, 0.);
        sim.command_entities
            .mobs
            .entry(wither_skel)
            .or_default()
            .extra
            .insert("SkeletonType".into(), DropNbt::Byte(1));
        sim.tick_entities(
            &mut world,
            &mut player,
            &Settings::default(),
            &mut Outcome::default(),
        );
        assert!(sim.arrows.is_empty(), "wither skeleton must not shoot arrows");
        assert!(sim.health < 20., "wither skeleton must melee attack");
        assert_eq!(sim.effects.get(20).map(|e| e.duration), Some(200));

        let (mut world, mut sim, mut player) = scene();
        sim.spawn_registered_raw(Skeleton, player.position + Vec3::Z * 2.0, 0.);
        sim.tick_entities(
            &mut world,
            &mut player,
            &Settings::default(),
            &mut Outcome::default(),
        );
        assert_eq!(sim.arrows.len(), 1, "normal skeleton must shoot within < 4.0m");
    }
}
