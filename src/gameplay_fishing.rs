//! ItemFishingRod, EntityFishHook and ItemEnderEye in the integrated world.
use super::*;
use crate::vanilla::{
    biome::BiomeGenerator,
    drops::DropNbt,
    entity_pick::body,
    loot, math,
    movement::{collision, Aabb},
    random::JavaRandom,
    ray_trace,
    spawning::EntityRandom,
    stronghold,
};
use glam::DVec3;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FishingState {
    pub hook: Option<Hook>,
    pub eyes: Vec<Eye>,
    stronghold_seed: Option<i64>,
    strongholds: Vec<IVec3>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hook {
    pub position: DVec3,
    pub previous: DVec3,
    pub velocity: DVec3,
    caught_mob: Option<u64>,
    ground: Option<(IVec3, u16)>,
    grounded_ticks: u16,
    catchable: i32,
    waiting: i32,
    approach: i32,
    fish_angle: f32,
    random_state: u64,
    gaussian: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Eye {
    pub position: DVec3,
    pub previous: DVec3,
    velocity: DVec3,
    target: DVec3,
    age: u8,
    drop_item: bool,
}

impl Eye {
    fn new(position: DVec3, destination: IVec3, rng: &mut JavaRandom) -> Self {
        let destination = destination.as_dvec3();
        let delta = destination - position;
        let distance = (delta.x * delta.x + delta.z * delta.z).sqrt() as f32;
        let target = if distance > 12. {
            position
                + DVec3::new(
                    delta.x / f64::from(distance) * 12.,
                    8.,
                    delta.z / f64::from(distance) * 12.,
                )
        } else {
            destination
        };
        Self {
            position,
            previous: position,
            velocity: DVec3::ZERO,
            target,
            age: 0,
            drop_item: rng.next_int(5) > 0,
        }
    }
    fn tick(&mut self) -> bool {
        self.previous = self.position;
        self.position += self.velocity;
        let speed = self.velocity.x.hypot(self.velocity.z) as f32;
        let delta = self.target - self.position;
        let distance = delta.x.hypot(delta.z) as f32;
        let angle = ai::motion::atan2(delta.z, delta.x) as f32;
        let mut next_speed = f64::from(speed) + f64::from(distance - speed) * 0.0025;
        if distance < 1. {
            next_speed *= 0.8;
            self.velocity.y *= 0.8;
        }
        self.velocity.x = f64::from(angle).cos() * next_speed;
        self.velocity.z = f64::from(angle).sin() * next_speed;
        let desired_y = if self.position.y < self.target.y {
            1.
        } else {
            -1.
        };
        self.velocity.y += (desired_y - self.velocity.y) * f64::from(0.015f32);
        self.age = self.age.saturating_add(1);
        self.age <= 80
    }
}

fn hook_bounds(at: DVec3) -> Aabb {
    body(at, 0.25, 0.25)
}

fn water_fraction(world: &World, at: DVec3) -> f64 {
    let bounds = hook_bounds(at);
    let mut wet = 0.;
    for slice in 0..5 {
        let low = bounds.min.y + 0.25 * f64::from(slice) / 5.;
        let high = bounds.min.y + 0.25 * f64::from(slice + 1) / 5.;
        let mut touched = false;
        for x in bounds.min.x.floor() as i32..=(bounds.max.x.floor() as i32) {
            for y in low.floor() as i32..=(high.floor() as i32) {
                for z in bounds.min.z.floor() as i32..=(bounds.max.z.floor() as i32) {
                    let state = world.get(IVec3::new(x, y, z));
                    if matches!(state >> 4, 8 | 9) {
                        let level = state & 15;
                        let surface =
                            f64::from(y + 1) - if level < 8 { f64::from(level) / 8. } else { 0. };
                        touched |= surface >= low;
                    }
                }
            }
        }
        if touched {
            wet += 0.2;
        }
    }
    wet
}

fn weighted<'a>(
    entries: &'a [(u16, u16, u8, i32, f32, bool)],
    rng: &mut JavaRandom,
) -> &'a (u16, u16, u8, i32, f32, bool) {
    let mut pick = rng.next_int(entries.iter().map(|e| e.3).sum());
    for entry in entries {
        pick -= entry.3;
        if pick < 0 {
            return entry;
        }
    }
    unreachable!()
}

fn fishing_loot(rod: &Stack, world_roll: f32, rng: &mut JavaRandom) -> (Stack, &'static str) {
    const JUNK: &[(u16, u16, u8, i32, f32, bool)] = &[
        (301, 0, 1, 10, 0.9, false),
        (334, 0, 1, 10, 0., false),
        (352, 0, 1, 10, 0., false),
        (373, 0, 1, 10, 0., false),
        (287, 0, 1, 5, 0., false),
        (346, 0, 1, 2, 0.9, false),
        (281, 0, 1, 10, 0., false),
        (280, 0, 1, 5, 0., false),
        (351, 0, 10, 1, 0., false),
        (131, 0, 1, 10, 0., false),
        (367, 0, 1, 10, 0., false),
    ];
    const TREASURE: &[(u16, u16, u8, i32, f32, bool)] = &[
        (111, 0, 1, 1, 0., false),
        (421, 0, 1, 1, 0., false),
        (329, 0, 1, 1, 0., false),
        (261, 0, 1, 1, 0.25, true),
        (346, 0, 1, 1, 0.25, true),
        (340, 0, 1, 1, 0., true),
    ];
    const FISH: &[(u16, u16, u8, i32, f32, bool)] = &[
        (349, 0, 1, 60, 0., false),
        (349, 1, 1, 25, 0., false),
        (349, 2, 1, 2, 0., false),
        (349, 3, 1, 13, 0., false),
    ];
    let luck = rod.enchantment_level(61) as f32;
    let lure = rod.enchantment_level(62) as f32;
    let junk = (0.1 - luck * 0.025 - lure * 0.01).clamp(0., 1.);
    let treasure = (0.05 + luck * 0.01 - lure * 0.01).clamp(0., 1.);
    let (pool, stat) = if world_roll < junk {
        (JUNK, "stat.junkFished")
    } else if world_roll - junk < treasure {
        (TREASURE, "stat.treasureFished")
    } else {
        (FISH, "stat.fishCaught")
    };
    let &(id, meta, count, _, damage, enchant) = weighted(pool, rng);
    let mut stack = Stack::new(id, count);
    stack.meta = meta;
    if damage > 0. {
        let max_damage = match id {
            301 => 65,
            346 => 64,
            261 => 384,
            _ => 0,
        };
        let cap = (damage * max_damage as f32) as i32;
        let bound = rng.next_int(cap) + 1;
        stack.damage = (max_damage - rng.next_int(bound)).clamp(1, cap) as u16;
    }
    if enchant {
        let enchants = loot::enchant_item(rng, id, 30);
        let key = if id == 340 {
            stack.id = 403;
            "StoredEnchantments"
        } else {
            "ench"
        };
        let entries = enchants
            .into_iter()
            .map(|[id, level]| {
                DropNbt::Compound(
                    [
                        ("id".into(), DropNbt::Short(id as i16)),
                        ("lvl".into(), DropNbt::Short(level as i16)),
                    ]
                    .into(),
                )
            })
            .collect();
        stack.nbt = Some(std::sync::Arc::new(DropNbt::Compound(
            [(
                key.into(),
                DropNbt::List {
                    element: 10,
                    values: entries,
                },
            )]
            .into(),
        )));
    }
    (stack, stat)
}

impl Simulation {
    pub(crate) fn use_fishing_or_eye(
        &mut self,
        world: &World,
        player: &Player,
        out: &mut Outcome,
    ) -> bool {
        let Some(held) = self.inventory.held() else {
            return false;
        };
        if held.id == 346 {
            self.hand_swing.start(crate::first_person::swing_duration(&self.effects));
            if let Some(hook) = self.fishing.hook.take() {
                self.retract_hook(hook, &held, player);
            } else {
                let mut rng = EntityRandom::new(random(&mut self.rng) as i64);
                let [yaw, pitch] = player
                    .java_rotation()
                    .map(|v| v / 180. * std::f32::consts::PI);
                let position = player.precise_eye(1.)
                    - DVec3::new(
                        f64::from(math::cos(yaw) * 0.16),
                        f64::from(0.1f32),
                        f64::from(math::sin(yaw) * 0.16),
                    );
                let mut velocity = DVec3::new(
                    f64::from(-math::sin(yaw) * math::cos(pitch) * 0.4),
                    f64::from(-math::sin(pitch) * 0.4),
                    f64::from(math::cos(yaw) * math::cos(pitch) * 0.4),
                );
                velocity /= f64::from(velocity.length() as f32);
                for axis in 0..3 {
                    velocity[axis] += rng.next_gaussian() * f64::from(0.0075f32);
                }
                velocity *= 1.5;
                let (random_state, gaussian) = rng.state();
                self.fishing.hook = Some(Hook {
                    position,
                    previous: position,
                    velocity,
                    caught_mob: None,
                    ground: None,
                    grounded_ticks: 0,
                    catchable: 0,
                    waiting: 0,
                    approach: 0,
                    fish_angle: 0.,
                    random_state,
                    gaussian,
                });
                let pitch = 0.4 / (unit(&mut self.rng) * 0.4 + 0.8);
                out.world_sounds.push((
                    "random.bow",
                    DVec3::from_array(player.precise_position()),
                    0.5,
                    pitch,
                ));
                *self
                    .statistics
                    .entry("stat.useItem.minecraft.fishing_rod".into())
                    .or_default() += 1;
            }
            self.place_ticks = 4;
            return true;
        }
        if held.id != 381 {
            return false;
        }
        if ray_trace::trace(
            world,
            player.eye(1.).as_dvec3(),
            player.eye(1.).as_dvec3() + player.direction().as_dvec3() * 5.,
            ray_trace::Options::item(false),
        )
        .is_some_and(|hit| world.get(hit.position) >> 4 == 120)
        {
            return true;
        }
        if self.dimension != 0 || self.flat_world {
            return true;
        }
        let seed = self.world_commands.imported_seed.unwrap_or(self.spawn_seed);
        if self.fishing.stronghold_seed != Some(seed) {
            let biomes = BiomeGenerator::new(seed);
            self.fishing.strongholds = stronghold::positions(seed, &biomes)
                .into_iter()
                .map(|[x, z]| IVec3::new(x * 16 + 8, 64, z * 16 + 8))
                .collect();
            self.fishing.stronghold_seed = Some(seed);
        }
        let position = DVec3::from_array(player.precise_position());
        if let Some(&target) = self.fishing.strongholds.iter().min_by(|a, b| {
            a.as_dvec3()
                .distance_squared(position)
                .total_cmp(&b.as_dvec3().distance_squared(position))
        }) {
            let mut rng = JavaRandom::new(random(&mut self.rng) as i64);
            self.fishing.eyes.push(Eye::new(position, target, &mut rng));
            let pitch = 0.4 / (unit(&mut self.rng) * 0.4 + 0.8);
            out.world_sounds.push(("random.bow", position, 0.5, pitch));
            if self.mode != Mode::Creative {
                self.inventory.take_selected(1);
            }
            *self
                .statistics
                .entry("stat.useItem.minecraft.ender_eye".into())
                .or_default() += 1;
        }
        self.place_ticks = 4;
        true
    }

    fn retract_hook(&mut self, hook: Hook, rod: &Stack, player: &Player) {
        let delta = player.position.as_dvec3() - hook.position;
        let pull = delta * 0.1 + DVec3::Y * f64::from((delta.length() as f32).sqrt()) * 0.08;
        let mut cost = 0;
        let mut rng = JavaRandom::from_state(hook.random_state);
        if let Some(mob) = hook
            .caught_mob
            .and_then(|id| self.mobs.iter_mut().find(|m| m.id == id && m.health > 0.))
        {
            mob.velocity += pull.as_vec3();
            cost = 3;
        } else if hook.catchable > 0 {
            let mut world_rng = self.block_ticks.world_random();
            let roll = world_rng.next_float();
            self.block_ticks.set_world_random(&world_rng);
            let (item, stat) = fishing_loot(rod, roll, &mut rng);
            self.spawn_item_entity(item, hook.position, pull, 0);
            self.spawn_xp(
                player.position.as_dvec3() + DVec3::new(0., 0.5, 0.5),
                rng.next_int(6) + 1,
            );
            *self.statistics.entry(stat.into()).or_default() += 1;
            cost = 1;
        }
        if hook.ground.is_some() {
            cost = 2;
        }
        if cost > 0 && self.mode != Mode::Creative {
            self.wear_slot(self.inventory.selected, cost, false);
        }
    }

    pub(crate) fn use_eye_on_frame(
        &mut self,
        world: &mut World,
        pos: IVec3,
        out: &mut Outcome,
    ) -> bool {
        let state = world.get(pos);
        if state >> 4 != 120 || state & 4 != 0 {
            return false;
        }
        if !world.set(pos, state | 4) {
            return false;
        }
        // The integrated creative interaction manager restores ItemStack size after onItemUse.
        if self.mode != Mode::Creative {
            self.inventory.take_selected(1);
        }
        out.changed.push(pos);
        self.block_ticks.notify(world, pos);
        let mut rng = JavaRandom::from_state(self.particle_random[2]);
        for _ in 0..16 {
            let at = pos.as_dvec3()
                + DVec3::new(
                    f64::from((5. + rng.next_float() * 6.) / 16.),
                    0.8125,
                    f64::from((5. + rng.next_float() * 6.) / 16.),
                );
            self.particles
                .extend(Particle::network_sprite(11, at, DVec3::ZERO, &mut rng));
        }
        self.particle_random[2] = rng.state();
        // ItemEnderEye checks the 12 eye-bearing blocks from the clicked frame's facing.
        let facing = [IVec3::Z, IVec3::NEG_X, IVec3::NEG_Z, IVec3::X][(state & 3) as usize];
        let side = IVec3::new(-facing.z, 0, facing.x);
        let frame = |p| world.get(p) >> 4 == 120 && world.get(p) & 4 != 0;
        let row: Vec<_> = (-2..=2)
            .filter(|i| world.get(pos + side * *i) >> 4 == 120)
            .collect();
        if row.len() != 3 || row[2] != row[0] + 2 || row.iter().any(|i| !frame(pos + side * *i)) {
            return true;
        }
        let first = row[0];
        if (first..=first + 2).any(|i| !frame(pos + facing * 4 + side * i))
            || [first - 1, first + 3]
                .into_iter()
                .any(|i| (1..=3).any(|j| !frame(pos + side * i + facing * j)))
        {
            return true;
        }
        for i in first..=first + 2 {
            for j in 1..=3 {
                let at = pos + side * i + facing * j;
                if world.set(at, 119 << 4) {
                    out.changed.push(at);
                    self.block_ticks.notify(world, at);
                }
            }
        }
        true
    }

    pub(crate) fn tick_fishing_and_eyes(
        &mut self,
        world: &World,
        player: &mut Player,
        out: &mut Outcome,
    ) {
        let mut eyes = std::mem::take(&mut self.fishing.eyes);
        let mut particles_rng = EntityRandom::from_state(self.particle_random[2], None);
        eyes.retain_mut(|eye| {
            let alive = eye.tick();
            let p = eye.position - eye.velocity * 0.25;
            let at = p + DVec3::new(
                particles_rng.random.next_double() * 0.6 - 0.3,
                -0.5,
                particles_rng.random.next_double() * 0.6 - 0.3,
            );
            if self.particles.len() < 4000 {
                self.particles.extend(Particle::special(
                    24,
                    at,
                    eye.velocity,
                    &mut particles_rng.random,
                ));
            }
            if !alive {
                if eye.drop_item {
                    let mut random = JavaRandom::new(JavaRandom::independent_seed());
                    let x = (random.next_double() * f64::from(0.2f32) - f64::from(0.1f32)) as f32;
                    let z = (random.next_double() * f64::from(0.2f32) - f64::from(0.1f32)) as f32;
                    self.spawn_item_entity(
                        Stack::new(381, 1),
                        eye.position,
                        DVec3::new(x.into(), f64::from(0.2f32), z.into()),
                        0,
                    );
                } else {
                    for _ in 0..8 {
                        if self.particles.len() >= 4000 {
                            break;
                        }
                        let vel = DVec3::new(
                            particles_rng.next_gaussian() * 0.15,
                            particles_rng.random.next_double() * 0.2,
                            particles_rng.next_gaussian() * 0.15,
                        );
                        self.particles.push(Particle::item(
                            381,
                            0,
                            false,
                            eye.position,
                            vel,
                            &mut particles_rng.random,
                        ));
                    }
                    let mut angle = 0.0f64;
                    while angle < std::f64::consts::TAU {
                        if self.particles.len() >= 4000 {
                            break;
                        }
                        let (s, c) = angle.sin_cos();
                        let ring = eye.position + DVec3::new(c * 5.0, -0.4, s * 5.0);
                        for speed in [-5.0, -7.0] {
                            if self.particles.len() >= 4000 {
                                break;
                            }
                            self.particles.extend(Particle::special(
                                24,
                                ring,
                                DVec3::new(c * speed, 0.0, s * speed),
                                &mut particles_rng.random,
                            ));
                        }
                        angle += std::f64::consts::TAU / 40.0;
                    }
                }
            }
            alive
        });
        self.particle_random[2] = particles_rng.random.state();
        self.fishing.eyes = eyes;
        let Some(mut hook) = self.fishing.hook.take() else {
            return;
        };
        let rod = self.inventory.held();
        if self.health <= 0.
            || rod.as_ref().is_none_or(|s| s.id != 346)
            || hook.position.distance_squared(player.position.as_dvec3()) > 1024.
        {
            return;
        }
        hook.previous = hook.position;
        if let Some(mob) = hook
            .caught_mob
            .and_then(|id| self.mobs.iter().find(|m| m.id == id && m.health > 0.))
        {
            hook.position =
                mob.position.as_dvec3() + DVec3::Y * f64::from(self.mob_size(mob).1) * 0.8;
            self.fishing.hook = Some(hook);
            return;
        }
        hook.caught_mob = None;
        let mut rng = EntityRandom::from_state(hook.random_state, hook.gaussian);
        let mut effect_rng = JavaRandom::from_state(self.particle_random[2]);
        if let Some((tile, state)) = hook.ground {
            if world.get(tile) >> 4 == state >> 4 {
                hook.grounded_ticks = hook.grounded_ticks.saturating_add(1);
                if hook.grounded_ticks < 1200 {
                    self.fishing.hook = Some(hook);
                }
                return;
            }
            hook.ground = None;
            hook.grounded_ticks = 0;
            for axis in 0..3 {
                hook.velocity[axis] *= f64::from(rng.random.next_float() * 0.2);
            }
        }
        let end = hook.position + hook.velocity;
        let hit = ray_trace::trace(
            world,
            hook.position,
            end,
            ray_trace::Options {
                stop_on_liquid: false,
                ignore_without_box: false,
                return_last_miss: false,
            },
        );
        let endpoint = hit.map_or(end, |h| h.point);
        let mut nearest = None;
        for mob in self.mobs.iter().filter(|m| m.health > 0.) {
            let (width, height) = self.mob_size(mob);
            let bounds = body(mob.position.as_dvec3(), width, height)
                .expand(DVec3::splat(f64::from(0.3f32)));
            let b = [
                bounds.min.x,
                bounds.min.y,
                bounds.min.z,
                bounds.max.x,
                bounds.max.y,
                bounds.max.z,
            ];
            if let Some((_, point)) = ray_trace::intercept(b, hook.position, endpoint) {
                let distance = point.distance_squared(hook.position);
                if nearest.is_none_or(|(_, d)| distance < d) {
                    nearest = Some((mob.id, distance));
                }
            }
        }
        if let Some((id, _)) = nearest {
            let mob = self.mobs.iter_mut().find(|mob| mob.id == id).unwrap();
            let data = self.command_entities.mobs.get_mut(&id);
            let silent = data.as_ref().is_some_and(|data| data.silent);
            let invulnerable = data.as_ref().is_some_and(|data| data.invulnerable);
            let brain = self.mob_brains.entry(id).or_default();
            if !invulnerable {
                if let Some(fresh) = melee::apply_mob_hit(
                    mob,
                    brain,
                    data,
                    0.,
                    HitSource {
                        projectile: true,
                        has_attacker: true,
                        ..Default::default()
                    },
                    &mut self.enchantment_random_state,
                ) {
                    hook.caught_mob = Some(id);
                    brain.player_hit_ticks = 100;
                    brain.hurt(player.position);
                    if fresh {
                        mob.velocity = melee::recoil(
                            mob.position.as_dvec3(),
                            mob.velocity.as_dvec3(),
                            DVec3::from_array(player.precise_position()),
                            &mut self.xp_math_random,
                        )
                        .as_vec3();
                        let trader_random = self
                            .trades
                            .get_mut(&id)
                            .map(|trader| &mut trader.career.random_state);
                        melee::mob_hurt_feedback(mob, brain, trader_random, true, silent, out);
                    }
                }
            }
        } else if let Some(hit) = hit {
            hook.position = hit.point;
            hook.ground = Some((hit.position, world.get(hit.position)));
            self.fishing.hook = Some(hook);
            return;
        }
        let movement = collision::move_body_with_step(
            world,
            hook_bounds(hook.position),
            hook.velocity,
            false,
            false,
            0.,
        );
        hook.position = movement.bounds.feet();
        for axis in 0..3 {
            if movement.clipped[axis] {
                hook.velocity[axis] = 0.;
            }
        }
        let mut drag = if movement.grounded || movement.clipped[0] || movement.clipped[2] {
            0.5f32
        } else {
            0.92f32
        };
        let water = water_fraction(world, hook.position);
        if water > 0. {
            let at = hook.position.floor().as_ivec3() + IVec3::Y;
            let mut elapsed = 1;
            if rng.random.next_float() < 0.25
                && self.weather.strengths(1.).0 > 0.2
                && world.rain_exposed(at)
            {
                elapsed = 2;
            }
            if rng.random.next_float() < 0.5 && !world.can_see_sky(at) {
                elapsed -= 1;
            }
            if hook.catchable > 0 {
                hook.catchable -= 1;
                if hook.catchable == 0 {
                    hook.waiting = 0;
                    hook.approach = 0;
                }
            } else if hook.approach > 0 {
                hook.approach -= elapsed;
                if hook.approach <= 0 {
                    hook.velocity.y -= f64::from(0.2f32);
                    out.world_sounds.push((
                        "random.splash",
                        hook.position,
                        0.25,
                        1. + (rng.random.next_float() - rng.random.next_float()) * 0.4,
                    ));
                    hook.catchable = 10 + rng.random.next_int(21);
                    for _ in 0..6 {
                        self.particles.extend(Particle::network_sprite(
                            4,
                            hook.position + DVec3::Y * 0.1,
                            DVec3::ZERO,
                            &mut effect_rng,
                        ));
                    }
                } else {
                    hook.fish_angle =
                        (f64::from(hook.fish_angle) + rng.next_gaussian() * 4.) as f32;
                    let angle = hook.fish_angle * std::f32::consts::PI / 180.;
                    let sin = math::sin(angle);
                    let cos = math::cos(angle);
                    let at = DVec3::new(
                        hook.position.x + f64::from(sin * hook.approach as f32 * 0.1),
                        hook.position.y.floor() + 1.,
                        hook.position.z + f64::from(cos * hook.approach as f32 * 0.1),
                    );
                    if matches!(
                        world.get(IVec3::new(at.x as i32, at.y as i32 - 1, at.z as i32)) >> 4,
                        8 | 9
                    ) {
                        if rng.random.next_float() < 0.15 {
                            self.particles.extend(Particle::network_sprite(
                                4,
                                at - DVec3::Y * 0.1,
                                DVec3::new(sin.into(), 0.1, cos.into()),
                                &mut effect_rng,
                            ));
                        }
                        for sign in [-1., 1.] {
                            self.particles.extend(Particle::network_sprite(
                                6,
                                at,
                                DVec3::new(
                                    f64::from(cos * 0.04) * sign,
                                    0.01,
                                    f64::from(-sin * 0.04) * sign,
                                ),
                                &mut effect_rng,
                            ));
                        }
                    }
                }
            } else if hook.waiting > 0 {
                hook.waiting -= elapsed;
                let mut chance = 0.15f32;
                if hook.waiting < 20 {
                    chance = (f64::from(chance) + f64::from(20 - hook.waiting) * 0.05) as f32;
                } else if hook.waiting < 40 {
                    chance = (f64::from(chance) + f64::from(40 - hook.waiting) * 0.02) as f32;
                } else if hook.waiting < 60 {
                    chance = (f64::from(chance) + f64::from(60 - hook.waiting) * 0.01) as f32;
                }
                if rng.random.next_float() < chance {
                    let angle = (rng.random.next_float() * 360.) * std::f32::consts::PI / 180.;
                    let radius = 25. + rng.random.next_float() * 35.;
                    let at = DVec3::new(
                        hook.position.x + f64::from(math::sin(angle) * radius * 0.1),
                        hook.position.y.floor() + 1.,
                        hook.position.z + f64::from(math::cos(angle) * radius * 0.1),
                    );
                    if matches!(
                        world.get(IVec3::new(at.x as i32, at.y as i32 - 1, at.z as i32)) >> 4,
                        8 | 9
                    ) {
                        for _ in 0..2 + rng.random.next_int(2) {
                            self.particles.extend(Particle::network_sprite(
                                5,
                                at,
                                DVec3::ZERO,
                                &mut effect_rng,
                            ));
                        }
                    }
                }
                if hook.waiting <= 0 {
                    hook.fish_angle = rng.random.next_float() * 360.;
                    hook.approach = 20 + rng.random.next_int(61);
                }
            } else {
                hook.waiting = 100 + rng.random.next_int(801)
                    - rod.as_ref().unwrap().enchantment_level(62) * 100;
            }
            if hook.catchable > 0 {
                hook.velocity.y -= f64::from(
                    rng.random.next_float() * rng.random.next_float() * rng.random.next_float(),
                ) * 0.2;
            }
        }
        hook.velocity.y += f64::from(0.04f32) * (water * 2. - 1.);
        if water > 0. {
            drag = (f64::from(drag) * 0.9) as f32;
            hook.velocity.y *= 0.8;
        }
        hook.velocity *= f64::from(drag);
        (hook.random_state, hook.gaussian) = rng.state();
        self.particle_random[2] = effect_rng.state();
        self.fishing.hook = Some(hook);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_original_eye_lifecycle_and_fishing_cast_air_frames_match() {
        #[derive(Deserialize)]
        struct Oracle {
            eyes: Vec<EyeTrace>,
            hooks: Vec<HookTrace>,
            catches: Vec<CatchTrace>,
        }
        #[derive(Deserialize)]
        struct EyeTrace {
            target: [i32; 3],
            frames: Vec<[f64; 6]>,
            drop: bool,
            dead: bool,
        }
        #[derive(Deserialize)]
        struct HookTrace {
            aim: [f32; 2],
            frames: Vec<[f64; 6]>,
        }
        #[derive(Deserialize)]
        struct CatchTrace {
            level: i16,
            roll: f32,
            seed: i64,
            stack: [u16; 3],
            enchants: Vec<[i16; 2]>,
        }
        let oracle: Oracle = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-fishing-eye.json"
        ))
        .unwrap();
        let compare = |position: DVec3, velocity: DVec3, expected: [f64; 6]| {
            let actual = [
                position.x, position.y, position.z, velocity.x, velocity.y, velocity.z,
            ];
            for i in 0..6 {
                assert!(
                    (actual[i] - expected[i]).abs() < 2e-6,
                    "axis {i}, actual {actual:?}, original {expected:?}"
                );
            }
        };
        for trace in oracle.eyes {
            let mut rng = JavaRandom::new(189);
            let mut eye = Eye::new(
                DVec3::new(8., 64., 8.),
                IVec3::from_array(trace.target),
                &mut rng,
            );
            compare(eye.position, eye.velocity, trace.frames[0]);
            let mut alive = true;
            for &frame in &trace.frames[1..] {
                alive = eye.tick();
                compare(eye.position, eye.velocity, frame);
            }
            assert_eq!(eye.drop_item, trace.drop);
            assert_eq!(!alive, trace.dead);
        }
        let world = super::super::tests::flat_world();
        for trace in oracle.hooks {
            let mut sim = Simulation::new(Mode::Survival, 189);
            sim.inventory.slots[0] = Some(Stack::new(346, 1));
            let mut player = Player::new(Vec3::new(8.5, 64., 8.5));
            player.set_rotation_degrees(trace.aim[0], trace.aim[1]);
            let mut out = Outcome::default();
            assert!(sim.use_fishing_or_eye(&world, &player, &mut out));
            let hook = sim.fishing.hook.as_ref().unwrap();
            compare(hook.position, hook.velocity, trace.frames[0]);
            for &frame in &trace.frames[1..] {
                sim.tick_fishing_and_eyes(&world, &mut player, &mut out);
                let hook = sim.fishing.hook.as_ref().unwrap();
                compare(hook.position, hook.velocity, frame);
            }
        }
        for trace in oracle.catches {
            let mut rod = Stack::new(346, 1);
            if trace.level > 0 {
                rod.nbt = Some(std::sync::Arc::new(DropNbt::Compound(
                    [(
                        "ench".into(),
                        DropNbt::List {
                            element: 10,
                            values: [61, 62]
                                .into_iter()
                                .map(|id| {
                                    DropNbt::Compound(
                                        [
                                            ("id".into(), DropNbt::Short(id)),
                                            ("lvl".into(), DropNbt::Short(trace.level)),
                                        ]
                                        .into(),
                                    )
                                })
                                .collect(),
                        },
                    )]
                    .into(),
                )));
            }
            let (caught, _) = fishing_loot(&rod, trace.roll, &mut JavaRandom::new(trace.seed));
            assert_eq!(
                [
                    caught.id,
                    u16::from(caught.count),
                    if matches!(caught.id, 261 | 301 | 346) {
                        caught.damage
                    } else {
                        caught.meta
                    }
                ],
                trace.stack,
                "catch seed {} level {}",
                trace.seed,
                trace.level
            );
            let tag = caught
                .nbt
                .as_deref()
                .and_then(crate::vanilla::item_nbt::compound);
            let key = if caught.id == 403 {
                "StoredEnchantments"
            } else {
                "ench"
            };
            let mut actual = Vec::new();
            if let Some(DropNbt::List { values, .. }) = tag.and_then(|m| m.get(key)) {
                for e in values {
                    let map = crate::vanilla::item_nbt::compound(e).unwrap();
                    actual.push([
                        crate::vanilla::item_nbt::number(map.get("id")) as i16,
                        crate::vanilla::item_nbt::number(map.get("lvl")) as i16,
                    ]);
                }
            }
            assert_eq!(actual, trace.enchants);
        }
    }

    #[test]
    fn ender_eye_targets_clamps_rises_and_expires_after_eighty_one_ticks() {
        let start = DVec3::new(8., 64., 8.);
        let mut rng = JavaRandom::new(189);
        let mut eye = Eye::new(start, IVec3::new(108, 20, 8), &mut rng);
        assert_eq!(eye.target, DVec3::new(20., 72., 8.));
        for _ in 0..80 {
            assert!(eye.tick());
        }
        assert!(eye.position.x > start.x && eye.position.y > start.y);
        let mut saved: Eye = serde_json::from_slice(&serde_json::to_vec(&eye).unwrap()).unwrap();
        assert!(!eye.tick());
        assert!(!saved.tick());
        assert_eq!(saved.position, eye.position);
        let near = Eye::new(start, IVec3::new(10, 24, 8), &mut rng);
        assert_eq!(near.target, DVec3::new(10., 24., 8.));
        let mut drops = 0;
        for _ in 0..1000 {
            drops += i32::from(Eye::new(start, IVec3::ZERO, &mut rng).drop_item);
        }
        assert!((750..850).contains(&drops));
    }

    #[test]
    fn eye_insertion_consumes_stack_and_only_closes_complete_portal_ring() {
        let mut world = super::super::tests::flat_world();
        let mut sim = Simulation::new(Mode::Survival, 189);
        let center = IVec3::new(7, 1, 6);
        for dx in -1..=1 {
            world.set(center + IVec3::new(dx, 0, -2), (120 << 4) | 4);
            world.set(center + IVec3::new(dx, 0, 2), (120 << 4) | 6);
        }
        for dz in -1..=1 {
            world.set(center + IVec3::new(-2, 0, dz), (120 << 4) | 7);
            world.set(center + IVec3::new(2, 0, dz), (120 << 4) | 5);
        }
        let clicked = center - IVec3::Z * 2;
        let last = center + IVec3::Z * 2;
        world.set(clicked, 120 << 4);
        world.set(last, (120 << 4) | 2);
        sim.inventory.slots[0] = Some(Stack::new(381, 2));
        let mut out = Outcome::default();
        assert!(sim.use_eye_on_frame(&mut world, clicked, &mut out));
        assert_eq!(world.get(center), 0);
        assert_eq!(sim.inventory.held().unwrap().count, 1);
        assert!(sim.use_eye_on_frame(&mut world, last, &mut out));
        for x in -1..=1 {
            for z in -1..=1 {
                assert_eq!(world.get(center + IVec3::new(x, 0, z)), 119 << 4);
            }
        }
        assert!(sim.inventory.held().is_none());
        sim.mode = Mode::Creative;
        sim.inventory.slots[0] = Some(Stack::new(381, 1));
        world.set(clicked, 120 << 4);
        assert!(sim.use_eye_on_frame(&mut world, clicked, &mut out));
        assert_eq!(sim.inventory.held().unwrap().count, 1);
    }

    #[test]
    fn original_fishing_loot_categories_include_enchanted_treasure_and_all_fish() {
        let rod = Stack::new(346, 1);
        let mut rng = JavaRandom::new(189);
        let mut fish = std::collections::BTreeSet::new();
        let mut treasure = std::collections::BTreeSet::new();
        for _ in 0..300 {
            let (caught, stat) = fishing_loot(&rod, 0.5, &mut rng);
            assert_eq!(stat, "stat.fishCaught");
            assert_eq!(caught.id, 349);
            fish.insert(caught.meta);
            let (caught, stat) = fishing_loot(&rod, 0.125, &mut rng);
            assert_eq!(stat, "stat.treasureFished");
            treasure.insert(caught.id);
            if matches!(caught.id, 261 | 346 | 403) {
                assert!(caught.nbt.is_some());
            }
            let (_, stat) = fishing_loot(&rod, 0., &mut rng);
            assert_eq!(stat, "stat.junkFished");
        }
        assert_eq!(fish.len(), 4);
        assert_eq!(treasure.len(), 6);
    }

    #[test]
    #[ignore = "uses the installed resource pack through the normal gameplay input path"]
    fn eye_launch_uses_held_right_click_timing_and_preserves_creative_stack() {
        let resources = crate::validation::resources();
        let settings = Settings::default();
        for mode in [Mode::Survival, Mode::Creative] {
            let mut world = super::super::tests::flat_world();
            let mut sim = Simulation::new(mode, 189);
            sim.mob_spawning = false;
            sim.inventory.slots[0] = Some(Stack::new(381, 3));
            let mut player = Player::new(world.spawn);
            player.set_rotation_degrees(0., -30.);
            for _ in 0..9 {
                sim.tick(&mut world, &mut player, &resources, &settings, false, true);
            }
            assert_eq!(sim.fishing.eyes.len(), 3, "mode {mode:?}");
            assert_eq!(sim.statistics["stat.useItem.minecraft.ender_eye"], 3);
            if mode == Mode::Creative {
                assert_eq!(sim.inventory.held().unwrap().count, 3);
            } else {
                assert!(sim.inventory.held().is_none());
            }
            assert!(sim.fishing.eyes[0].position.y > 1.);
            for _ in 0..90 {
                sim.tick(&mut world, &mut player, &resources, &settings, false, false);
            }
            assert!(sim.fishing.eyes.is_empty());
            assert!(sim.drops.iter().all(|drop| drop.pickup_delay == 0));
        }
    }

    #[test]
    #[ignore = "uses the installed resource pack through the normal gameplay input path"]
    fn rod_casts_bobs_bites_reels_loot_and_resets_when_unequipped() {
        let mut world = super::super::tests::flat_world();
        for x in 0..16 {
            for z in 0..16 {
                for y in 1..=4 {
                    world.set(IVec3::new(x, y, z), 9 << 4);
                }
            }
        }
        world.set(IVec3::new(8, 4, 2), 1 << 4);
        let mut sim = Simulation::new(Mode::Survival, 189);
        sim.mob_spawning = false;
        sim.inventory.slots[0] = Some(Stack::new(346, 1));
        let mut player = Player::new(Vec3::new(8.5, 5., 2.5));
        player.set_rotation_degrees(0., 28.);
        let resources = crate::validation::resources();
        let settings = Settings::default();
        sim.tick(&mut world, &mut player, &resources, &settings, false, true);
        assert!(sim.fishing.hook.is_some());
        let first = sim.fishing.hook.as_ref().unwrap().position;
        let mut bit = false;
        for _ in 0..1600 {
            sim.tick(&mut world, &mut player, &resources, &settings, false, false);
            if sim.fishing.hook.as_ref().is_some_and(|h| h.catchable > 0) {
                bit = true;
                break;
            }
        }
        assert!(
            bit,
            "bobber should enter water and reach a bite window; hook={:?}, player={:?}, health={}",
            sim.fishing.hook, player.position, sim.health
        );
        assert!(sim.fishing.hook.as_ref().unwrap().position.z > first.z);
        sim.tick(&mut world, &mut player, &resources, &settings, false, true);
        assert!(sim.fishing.hook.is_none());
        assert_eq!(sim.inventory.held().unwrap().damage, 1);
        assert!(sim.drops.iter().any(|drop| drop.pickup_delay == 0));
        assert!(sim
            .statistics
            .get("stat.fishCaught")
            .or_else(|| sim.statistics.get("stat.treasureFished"))
            .or_else(|| sim.statistics.get("stat.junkFished"))
            .is_some());
        for _ in 0..4 {
            sim.tick(&mut world, &mut player, &resources, &settings, false, false);
        }
        sim.tick(&mut world, &mut player, &resources, &settings, false, true);
        assert!(sim.fishing.hook.is_some());
        let mut restored: Simulation =
            serde_json::from_slice(&serde_json::to_vec(&sim).unwrap()).unwrap();
        restored.inventory.slots[0] = None;
        restored.tick(&mut world, &mut player, &resources, &settings, false, false);
        assert!(restored.fishing.hook.is_none());
    }
}
