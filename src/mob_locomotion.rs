//! Server EntityLiving move/jump helper and LivingBase travel. The shared
//! movement body retains double-precision bounds and resolves block collision.
use super::motion::MoveControl;
use crate::vanilla::movement::{self, Aabb, MovementWorld, State, Travel};
use crate::vanilla::random::JavaRandom;
use glam::DVec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Locomotion {
    pub body: State,
    pub control: MoveControl,
    pub swing: f32,
    pub amount: f32,
    pub previous_amount: f32,
    pub void_damage: bool,
    width: f32,
    height: f32,
}
#[derive(Clone, Copy, Default)]
pub struct Drive {
    pub command: Option<(DVec3, f64)>,
    pub movement_speed: f64,
    pub force_jump: bool,
    pub suppress_jump: bool,
    pub depth_strider: i32,
    pub jump_boost: Option<i32>,
    pub ladder_override: Option<bool>,
    pub step_height: Option<f32>,
}
impl Locomotion {
    pub fn new(position: DVec3, width: f32, height: f32, yaw: f32, grounded: bool) -> Self {
        let mut body = State::new(position);
        let r = f64::from(width * 0.5);
        body.bounds = Aabb {
            min: position - DVec3::new(r, 0., r),
            max: position + DVec3::new(r, f64::from(height), r),
        };
        body.grounded = grounded;
        Self {
            body,
            control: MoveControl::new(yaw),
            swing: 0.,
            amount: 0.,
            previous_amount: 0.,
            void_damage: false,
            width,
            height,
        }
    }
    /// Mirror external teleports, impulses and growth without throwing away
    /// the authoritative double coordinates when the render copies agree.
    pub(crate) fn reconcile(&mut self, world: &impl MovementWorld, mob: &super::super::Mob, size: (f32, f32)) {
        let body = &mut self.body;
        if body.position.as_vec3() != mob.position {
            let position = mob.position.as_dvec3();
            body.bounds = body.bounds.offset(position - body.position);
            body.position = position;
        }
        if body.velocity.as_vec3() != mob.velocity {
            body.velocity = mob.velocity.as_dvec3();
        }
        if (self.width, self.height) != size {
            let (width, height) = size;
            body.bounds.max =
                body.bounds.min + DVec3::new(f64::from(width), f64::from(height), f64::from(width));
            if width > self.width {
                let d = f64::from(self.width - width);
                body.move_entity(world, DVec3::new(d, 0., d), false);
            }
            self.width = width;
            self.height = height;
        }
    }
    pub fn tick(&mut self, world: &impl MovementWorld, input: Drive) {
        let b = &mut self.body;
        if let Some(step) = input.step_height {
            b.step_height = Some(step);
        }
        b.touching_cactus = false;
        b.touching_fire = false;
        b.previous = b.position;
        let (wet, flow) = movement::water_contact(world, b.bounds);
        b.in_water = wet;
        if let Some(flow) = flow {
            b.velocity += flow * 0.014;
        }
        self.void_damage = b.position.y < -64.;
        b.jump_delay = b.jump_delay.saturating_sub(1);
        for axis in 0..3 {
            if b.velocity[axis].abs() < 0.005 {
                b.velocity[axis] = 0.;
            }
        }
        self.control
            .update(b.position, input.command, input.movement_speed);
        let lava = movement::touches_liquid(
            world,
            b.bounds.expand(DVec3::new(
                -f64::from(0.1f32),
                -f64::from(0.4f32),
                -f64::from(0.1f32),
            )),
            10,
            11,
        );
        b.in_lava = lava;
        if input.suppress_jump {
            self.control.jump = false;
        }
        if !input.suppress_jump && (self.control.jump || input.force_jump) {
            if wet || lava {
                b.velocity.y += f64::from(0.04f32);
            } else if b.grounded && b.jump_delay == 0 {
                b.velocity.y = f64::from(0.42f32);
                if let Some(amplifier) = input.jump_boost {
                    b.velocity.y += f64::from(amplifier.wrapping_add(1) as f32 * 0.1);
                }
                b.jump_delay = 10;
            }
        } else {
            b.jump_delay = 0;
        }
        self.control.forward *= 0.98;
        b.travel(
            world,
            Travel {
                strafe: 0.,
                forward: self.control.forward,
                yaw: self.control.yaw,
                sneak: false,
                depth_strider: input.depth_strider,
                water: wet,
                lava,
                land_speed: self.control.ai_speed,
                air_speed: 0.02,
                ladder_override: input.ladder_override,
                server: true,
            },
        );
        let dx = b.position.x - b.previous.x;
        let dz = b.position.z - b.previous.z;
        let distance = ((dx * dx + dz * dz).sqrt() as f32 * 4.).min(1.);
        self.previous_amount = self.amount;
        self.amount += (distance - self.amount) * 0.4;
        self.swing += self.amount;
    }
}
pub fn drive_mob(
    world: &impl MovementWorld,
    mob: &mut super::super::Mob,
    brain: &mut super::MobBrain,
    size: (f32, f32),
    destination: Option<glam::Vec3>,
    speed: f64,
) {
    drive_mob_with_effects(world, mob, brain, None, size, destination, speed);
}

pub fn drive_mob_with_effects(
    world: &impl MovementWorld,
    mob: &mut super::super::Mob,
    brain: &mut super::MobBrain,
    effects: Option<&crate::vanilla::potions::Effects>,
    size: (f32, f32),
    destination: Option<glam::Vec3>,
    speed: f64,
) {
    use super::super::MobKind;
    let movement = brain.movement.get_or_insert_with(|| {
        Box::new(Locomotion::new(
            mob.position.as_dvec3(),
            size.0,
            size.1,
            mob.yaw.to_degrees() - 180.,
            brain.grounded,
        ))
    });
    movement.reconcile(world, mob, size);
    let water = movement::water_contact(world, movement.body.bounds).0;
    let lava = movement::touches_liquid(
        world,
        movement.body.bounds.expand(DVec3::new(
            -f64::from(0.1f32),
            -f64::from(0.4f32),
            -f64::from(0.1f32),
        )),
        10,
        11,
    );
    let liquid = water || lava;
    if !liquid {
        brain.swimming = false;
    } else if brain.swim_ticks % 3 == 0 {
        brain.swimming = true;
    }
    brain.swim_ticks = brain.swim_ticks.wrapping_add(1);
    let mut force_jump = if brain.swimming {
        let state = brain.entity_random.get_or_insert_with(|| {
            let mut r = JavaRandom::new(JavaRandom::independent_seed());
            r.next_long();
            r.next_long();
            r.state()
        });
        let mut r = JavaRandom::from_state(*state);
        let jump = r.next_float() < 0.8;
        *state = r.state();
        jump
    } else {
        false
    };
    use MobKind::*;
    let slime_size = (size.0 / 0.51000005).round().max(1.);
    let mut command = destination;
    if matches!(mob.kind, Slime | MagmaCube | Rabbit) && movement.body.grounded {
        if brain.jump_timer == 0 && (destination.is_some() || mob.kind != Rabbit) {
            let state = brain
                .entity_random
                .get_or_insert_with(|| JavaRandom::new(JavaRandom::independent_seed()).state());
            let mut rng = JavaRandom::from_state(*state);
            let delay = if mob.kind == Rabbit {
                10
            } else {
                10 + rng.next_int(20) as u8
            };
            brain.jump_timer = if mob.kind == MagmaCube {
                delay.saturating_mul(4)
            } else {
                delay
            };
            force_jump = true;
            *state = rng.state();
            if mob.kind == MagmaCube {
                movement.body.velocity.y = 0.42 + f64::from(slime_size) * 0.1;
                force_jump = false;
            }
        } else {
            command = None;
        }
    }
    let mut base_speed = match mob.kind {
        Cow | Mooshroom | SnowGolem => f64::from(0.2f32),
        Sheep | Zombie | PigZombie | Blaze => f64::from(0.23f32),
        Spider | CaveSpider | Enderman | Wolf | Ocelot | Rabbit => f64::from(0.3f32),
        Villager | Giant | Guardian => 0.5,
        Horse => f64::from(0.225f32),
        Slime | MagmaCube => f64::from(0.2 + slime_size * 0.1),
        _ => 0.25,
    };
    if mob.kind == Enderman && brain.enderman_screaming {
        base_speed += 0.15000000596046448;
    }
    if let Some(eff) = effects.and_then(|e| e.get(1)) {
        base_speed *= 1.0 + 0.20000000298023224 * f64::from(eff.amplifier.wrapping_add(1));
    }
    if let Some(eff) = effects.and_then(|e| e.get(2)) {
        base_speed = (base_speed
            * (1.0 - 0.15000000596046448 * f64::from(eff.amplifier.wrapping_add(1))))
        .max(0.0);
    }
    if mob.kind == Enderman {
        force_jump = false;
    }
    let jump_boost = effects.and_then(|e| e.get(8)).map(|e| e.amplifier);
    movement.tick(
        world,
        Drive {
            command: command.map(|p| (p.as_dvec3(), speed)),
            movement_speed: base_speed,
            force_jump,
            suppress_jump: mob.kind == Enderman,
            jump_boost,
            step_height: (mob.kind == Enderman).then_some(1.0),
            ladder_override: matches!(mob.kind, Spider | CaveSpider)
                .then_some(movement.body.collided_horizontal),
            ..Default::default()
        },
    );
    if mob.kind == MobKind::Chicken && !movement.body.grounded && movement.body.velocity.y < 0. {
        movement.body.velocity.y *= 0.6;
    }
    if movement.void_damage {
        mob.health = 0.;
    }
    mob.position = movement.body.position.as_vec3();
    mob.velocity = movement.body.velocity.as_vec3();
    brain.gait = crate::entity_visual::Gait {
        previous_yaw: Some(mob.yaw),
        previous_amount: movement.previous_amount,
        amount: movement.amount,
        swing: movement.swing,
    };
    mob.yaw = (movement.control.yaw + 180.).to_radians();
    brain.grounded = movement.body.grounded;
}
fn dragon_flight(
    mob: &mut super::super::Mob,
    brain: &mut super::MobBrain,
    player: Option<glam::Vec3>,
) {
    use crate::vanilla::spawning::EntityRandom;
    use glam::Vec3;
    let state = brain
        .entity_random
        .get_or_insert_with(|| JavaRandom::new(JavaRandom::independent_seed()).state());
    let mut random = EntityRandom::from_state(*state, None);
    let mut goal = brain.hover_target.unwrap_or(Vec3::new(0., 100., 0.));
    let previous = goal - mob.position;
    if let Some(player) = player.filter(|_| brain.dragon_chasing) {
        goal = player;
        goal.y += ((player - mob.position).with_y(0.).length() / 80. - 0.6).min(10.);
    } else {
        goal.x += random.next_gaussian() as f32 * 2.;
        goal.z += random.next_gaussian() as f32 * 2.;
    }
    if brain.dragon_slowed || !(100.0..=22500.).contains(&previous.length_squared()) {
        let player_in_range = player.filter(|p| {
            let d2 = p.distance_squared(mob.position);
            (100.0..=22500.0).contains(&d2)
        });
        brain.dragon_chasing = !brain.dragon_slowed
            && player_in_range.is_some()
            && random.random.next_int(3) == 0;
        if let Some(p) = player_in_range.filter(|_| brain.dragon_chasing) {
            goal = p;
        } else {
            brain.dragon_chasing = false;
            loop {
                let y = 70. + random.random.next_float() * 50.;
                goal = Vec3::new(
                    random.random.next_float() * 120. - 60.,
                    y,
                    random.random.next_float() * 120. - 60.,
                );
                if goal.distance_squared(mob.position) > 100. {
                    break;
                }
            }
        }
    }
    *state = random.random.state();
    brain.hover_target = Some(goal);
    let horizontal = previous.with_y(0.).length().max(0.0001);
    mob.velocity.y += (previous.y / horizontal).clamp(-0.6, 0.6) * 0.1;
    let wanted = std::f32::consts::PI - previous.x.atan2(previous.z);
    let turn = ((wanted - mob.yaw).to_degrees() + 180.).rem_euclid(360.) - 180.;
    let forward = Vec3::new(mob.yaw.sin(), mob.velocity.y, -mob.yaw.cos()).normalize_or_zero();
    let alignment = ((forward.dot((goal - mob.position).normalize_or_zero()) + 0.5) / 1.5).max(0.);
    let speed = mob.velocity.with_y(0.).length() + 1.;
    let limited = speed.min(40.);
    brain.dragon_yaw_velocity =
        brain.dragon_yaw_velocity * 0.8 + turn.clamp(-50., 50.) * 0.7 / (speed * limited);
    let old_yaw = mob.yaw;
    mob.yaw += (brain.dragon_yaw_velocity * 0.1).to_radians();
    let response = 2. / (limited + 1.);
    mob.velocity += Vec3::new(mob.yaw.sin(), 0., -mob.yaw.cos())
        * (0.06 * (alignment * response + 1. - response));
    mob.position += mob.velocity * if brain.dragon_slowed { 0.8 } else { 1. };
    let drag = 0.8 + 0.075 * (1. + mob.velocity.normalize_or_zero().dot(forward));
    mob.velocity *= Vec3::new(drag, 0.91, drag);
    brain
        .gait
        .advance(old_yaw, (mob.position - mob.previous).with_y(0.).length());
}
/// Airborne and submerged mobs steer in three dimensions; they do not request ground paths.
pub fn free_move(
    world: &crate::world::World,
    mob: &mut super::super::Mob,
    brain: &mut super::MobBrain,
    size: (f32, f32),
    target: Option<glam::Vec3>,
    rng: &mut u64,
) -> bool {
    use super::super::{unit, MobKind::*};
    use glam::Vec3;
    if mob.kind == EnderDragon {
        dragon_flight(mob, brain, target);
        return true;
    }
    if !matches!(
        mob.kind,
        Bat | Ghast | EnderDragon | Wither | Squid | Guardian | Blaze
    ) {
        return false;
    }
    let wet = matches!(world.get(mob.position.floor().as_ivec3()) >> 4, 8 | 9);
    if matches!(mob.kind, Guardian | Squid) && !wet {
        if mob.kind == Guardian && brain.grounded {
            mob.velocity += Vec3::new((unit(rng) - 0.5) * 0.8, 0.5, (unit(rng) - 0.5) * 0.8);
        }
        if mob.age % 20 == 0 && mob.kind == Squid && brain.air_ticks >= 300 {
            mob.health -= 2.;
        }
        brain.air_ticks = brain.air_ticks.saturating_add(1);
        return false;
    }
    if mob.kind == Blaze && target.is_none() {
        if mob.velocity.y < 0. {
            mob.velocity.y *= 0.6;
        }
        return false;
    }
    brain.air_ticks = 0;
    let movement = brain.movement.get_or_insert_with(|| {
        Box::new(Locomotion::new(
            mob.position.as_dvec3(),
            size.0,
            size.1,
            mob.yaw.to_degrees() - 180.,
            brain.grounded,
        ))
    });
    movement.reconcile(world, mob, size);
    let mut goal = target;
    if matches!(mob.kind, Ghast | Bat | Squid) {
        goal = None;
    }
    if mob.kind == Wither || mob.kind == Blaze {
        goal = goal.map(|p| p + Vec3::Y * 4.);
    }
    if goal.is_none() {
        let refresh = brain
            .hover_target
            .is_none_or(|p| p.distance_squared(mob.position) < 2.)
            || movement.body.collided_horizontal
            || mob.age % 60 == 0;
        if refresh {
            let extent = if mob.kind == Ghast || mob.kind == EnderDragon {
                16.
            } else {
                6.
            };
            let offset = Vec3::new(unit(rng) - 0.5, unit(rng) - 0.5, unit(rng) - 0.5) * extent * 2.;
            let p = mob.position + offset;
            let id = world.get(p.floor().as_ivec3()) >> 4;
            if if wet { matches!(id, 8 | 9) } else { id == 0 } {
                brain.hover_target = Some(p);
            }
        }
        goal = brain.hover_target;
    }
    let heading = goal.map_or(Vec3::ZERO, |p| (p - mob.position).normalize_or_zero());
    let speed = match mob.kind {
        EnderDragon => 0.75,
        Wither => 0.43,
        Ghast => 0.32,
        Bat => 0.42,
        _ => 0.23,
    };
    let velocity = mob
        .velocity
        .lerp(heading * speed, if mob.kind == Bat { 0.2 } else { 0.1 });
    let body = &mut movement.body;
    body.previous = body.position;
    body.in_water = wet;
    body.in_lava = movement::touches_liquid(world, body.bounds, 10, 11);
    body.velocity = velocity.as_dvec3();
    body.move_entity(world, body.velocity, false);
    mob.position = body.position.as_vec3();
    mob.velocity = body.velocity.as_vec3();
    let delta = mob.position - mob.previous;
    brain.gait.advance(mob.yaw, delta.with_y(0.).length());
    brain.grounded = body.grounded;
    if delta.with_y(0.).length_squared() > 0.0001 {
        mob.yaw = delta.x.atan2(-delta.z);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::IVec3;
    use serde_json::{json, Value};
    use std::io::Read;
    struct World {
        layout: i32,
    }
    impl MovementWorld for World {
        fn get(&self, p: IVec3) -> u16 {
            if self.layout == 16 && p.x == 9 && (1..=4).contains(&p.y) {
                return 65 << 4 | 2;
            }
            if self.layout == 17 && p.z == 11 && (1..=4).contains(&p.y) {
                return 16;
            }
            if !(0..256).contains(&p.y) || self.layout == 13 {
                return 0;
            }
            if p.y == 0 {
                return (match self.layout {
                    1 => 79,
                    2 => 174,
                    3 => 165,
                    4 => 88,
                    5 => 44,
                    _ => 1,
                }) << 4;
            }
            match self.layout {
                6 if (2..=4).contains(&p.y) => 30 << 4,
                7 if p.y <= 3 => 9 << 4,
                8 if p.y <= 3 => 8 << 4 | p.x.rem_euclid(8) as u16,
                9 if p.y <= 3 => 10 << 4,
                10 | 14 if p.y <= 3 => 51 << 4,
                11 if p.x == 8 && p.z == 8 && (2..=4).contains(&p.y) => 16,
                12 if p.x == 0 && p.y <= 6 => 16,
                15 if p.x == 8 && p.z == 8 && p.y <= 3 => 81 << 4,
                _ => 0,
            }
        }
        fn block_boxes(&self, p: IVec3, state: u16, out: &mut Vec<Aabb>) {
            crate::vanilla::collision_shapes::emit(
                p,
                state,
                |q| self.get(q),
                None,
                |b| out.push(b),
            );
        }
        fn loaded_column(&self, _: i32, _: i32) -> bool {
            true
        }
    }
    #[test]
    fn server_living_motion_bounds_helper_and_limb_swing_match_18432_original_ticks() {
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(
            include_bytes!("../tests/fixtures/vanilla189-mob-motion.json.gz").as_slice(),
        )
        .read_to_end(&mut bytes)
        .unwrap();
        let data: Value = serde_json::from_slice(&bytes).unwrap();
        let mut count = 0;
        for (i, row) in data.as_array().unwrap().iter().enumerate() {
            let world = World {
                layout: row["layout"].as_i64().unwrap() as i32,
            };
            let (width, height) = [(0.6f32, 1.8f32), (0.9, 1.3), (1.4, 2.9), (0.3, 0.9)]
                [row["kind"].as_u64().unwrap() as usize];
            let mut mob = Locomotion::new(
                serde_json::from_value(row["start"].clone()).unwrap(),
                width,
                height,
                f32::from_bits(row["yaw"].as_u64().unwrap() as u32),
                row["ground"].as_bool().unwrap(),
            );
            for (t, f) in row["frames"].as_array().unwrap().iter().enumerate() {
                let command = (!f[0].is_null()).then(|| {
                    (
                        serde_json::from_value(f[0].clone()).unwrap(),
                        f[1].as_f64().unwrap(),
                    )
                });
                mob.tick(
                    &world,
                    Drive {
                        command,
                        movement_speed: row["attribute"].as_f64().unwrap(),
                        force_jump: f[2].as_bool().unwrap(),
                        ..Default::default()
                    },
                );
                let b = &mob.body;
                let bits = [b.position, b.velocity, b.bounds.min, b.bounds.max]
                    .into_iter()
                    .flat_map(|v| v.to_array())
                    .map(|v| format!("{:x}", v.to_bits()))
                    .collect::<Vec<_>>();
                let native = json!([
                    bits,
                    [
                        b.grounded,
                        b.collided_horizontal,
                        b.in_water,
                        b.in_web,
                        mob.void_damage
                    ],
                    ([
                        mob.control.yaw,
                        mob.control.ai_speed,
                        mob.control.forward,
                        mob.swing,
                        mob.amount,
                        mob.previous_amount
                    ]
                    .map(f32::to_bits))
                ]);
                assert_eq!(
                    native,
                    json!([f[3], f[4], f[5]]),
                    "mob {i}, layout{}, tick{t}",
                    world.layout
                );
                count += 1;
            }
        }
        assert_eq!(count, 18432);
    }
}
