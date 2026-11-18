use super::{EntityKind, Pose};
use crate::vanilla::movement::{collision, Aabb, MovementWorld};
use glam::{DVec3, IVec3};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Boat,
    Cart,
    Horse,
}
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct RiderInput {
    pub strafe: f32,
    pub forward: f32,
    pub yaw: f32,
}
pub(super) struct Vehicle {
    pub kind: Kind,
    pub yaw: f32,
    pub pitch: f32,
    pub hit_ticks: i32,
    pub damage: f32,
    pub direction: i32,
    pub display_state: u16,
    pub display_offset: i32,
    pub custom_display: bool,
    pub lit: bool,
    motion: DVec3,
    server_velocity: DVec3,
    speed: f64,
    pub grounded: bool,
    pub horse_flags: i32,
    pub stand_ticks: u8,
    rearing: f32,
    previous_rearing: f32,
}
impl Vehicle {
    pub fn new(kind: Kind, p: Pose) -> Box<Self> {
        Box::new(Self {
            kind,
            yaw: p.yaw,
            pitch: p.pitch,
            hit_ticks: 0,
            damage: 0.,
            direction: 1,
            display_state: 0,
            display_offset: 6,
            custom_display: false,
            lit: false,
            motion: DVec3::ZERO,
            server_velocity: DVec3::ZERO,
            speed: 0.07,
            grounded: p.on_ground,
            horse_flags: 0,
            stand_ticks: 0,
            rearing: 0.,
            previous_rearing: 0.,
        })
    }
    pub fn rearing_anim(&self) -> (f32, f32) {
        (self.previous_rearing, self.rearing)
    }
    pub fn spawn(kind: EntityKind, p: Pose) -> Option<Box<Self>> {
        match kind {
            EntityKind::Object(1, _) => Some(Self::new(Kind::Boat, p)),
            EntityKind::Object(10, _) => Some(Self::new(Kind::Cart, p)),
            _ => None,
        }
    }
    pub fn hurt(&mut self) {
        if self.kind != Kind::Horse {
            self.direction = -self.direction;
            self.hit_ticks = 10;
            self.damage *= 11.;
        }
    }
    pub fn display(&self, variant: i32) -> Option<(u16, i32)> {
        if self.kind != Kind::Cart {
            return None;
        }
        if self.custom_display {
            return (self.display_state != 0).then_some((self.display_state, self.display_offset));
        }
        match variant {
            1 => Some((54 << 4 | 2, 8)),
            2 => Some(((if self.lit { 62 } else { 61 }) << 4 | 2, 6)),
            3 => Some((46 << 4, 6)),
            4 => Some((52 << 4, 6)),
            5 => Some((154 << 4, 1)),
            6 => Some((137 << 4, 6)),
            _ => None,
        }
    }
    pub fn velocity(&mut self, v: [f64; 3]) {
        self.motion = v.into();
        self.server_velocity = self.motion;
    }
    pub fn correction(
        &mut self,
        at: &mut DVec3,
        p: Pose,
        teleport: bool,
        riding: bool,
        steps: &mut u8,
    ) {
        let target = DVec3::new(p.x, p.y, p.z);
        self.grounded = p.on_ground;
        if self.kind == Kind::Boat && riding {
            if teleport {
                *at = target;
                self.yaw = p.yaw;
                self.pitch = p.pitch;
                self.velocity([0.; 3]);
                *steps = 0;
            } else if at.distance_squared(target) > 1. {
                *steps = 3;
                self.motion = self.server_velocity;
            }
            return;
        }
        *steps = match self.kind {
            Kind::Boat => 8,
            Kind::Cart => 5,
            Kind::Horse => 3,
        };
        if self.kind != Kind::Horse {
            self.motion = self.server_velocity;
        }
    }
    pub fn tick(
        &mut self,
        world: &impl MovementWorld,
        at: &mut DVec3,
        p: Pose,
        steps: &mut u8,
        rider: Option<RiderInput>,
    ) {
        if self.hit_ticks > 0 {
            self.hit_ticks -= 1;
        }
        if self.damage > 0. {
            self.damage -= 1.;
        }
        if self.kind == Kind::Horse {
            self.previous_rearing = self.rearing;
            let rearing = self.horse_flags & 64 != 0 || self.stand_ticks > 0;
            if self.stand_ticks > 0 {
                self.stand_ticks -= 1;
            }
            if rearing {
                self.rearing = (self.rearing + (1. - self.rearing) * 0.4 + 0.05).min(1.);
            } else {
                self.rearing = ((0.8 * self.rearing * self.rearing * self.rearing - self.rearing)
                    * 0.6
                    + self.rearing
                    - 0.05)
                    .max(0.);
            }
        }
        if self.kind == Kind::Boat {
            // Entity.handleWaterMovement shrinks Y by 0.4 on each side. Because a boat is
            // 0.6 high, AxisAlignedBB sorts the inverted endpoints before its 0.001 contraction.
            let query = Aabb {
                min: *at
                    + DVec3::new(
                        -0.749,
                        f64::from(0.6f32) - f64::from(0.4f32) + 0.001,
                        -0.749,
                    ),
                max: *at + DVec3::new(0.749, f64::from(0.4f32) - 0.001, 0.749),
            };
            if let Some(flow) = crate::vanilla::movement::water_acceleration(world, query).1 {
                self.motion += flow * 0.014;
            }
        }
        if self.kind == Kind::Boat && rider.is_some() {
            self.boat(world, at, rider.unwrap_or_default());
            return;
        }
        if *steps > 0 {
            let n = f64::from(*steps);
            *at += (DVec3::new(p.x, p.y, p.z) - *at) / n;
            let d = (f64::from(p.yaw - self.yaw) + 180.).rem_euclid(360.) - 180.;
            self.yaw = (f64::from(self.yaw) + d / n) as f32;
            self.pitch = (f64::from(self.pitch) + f64::from(p.pitch - self.pitch) / n) as f32;
            *steps -= 1;
        } else if self.kind == Kind::Horse {
            self.motion *= 0.98;
        } else if self.kind == Kind::Boat {
            *at += self.motion;
            if self.grounded {
                self.motion *= 0.5;
            }
            self.drag();
        }
        if self.kind == Kind::Horse {
            for axis in 0..3 {
                if self.motion[axis].abs() < 0.005 {
                    self.motion[axis] = 0.;
                }
            }
        }
        if let Some(input) = rider {
            if self.kind == Kind::Horse && self.horse_flags & 4 != 0 {
                self.yaw = input.yaw;
            }
        }
    }
    fn drag(&mut self) {
        self.motion *= DVec3::new(f64::from(0.99f32), f64::from(0.95f32), f64::from(0.99f32));
    }
    fn boat(&mut self, world: &impl MovementWorld, p: &mut DVec3, input: RiderInput) {
        let bounds = Aabb {
            min: *p - DVec3::new(0.75, 0., 0.75),
            max: *p + DVec3::new(0.75, f64::from(0.6f32), 0.75),
        };
        let mut submersion = 0.;
        for i in 0..5 {
            let min_y = bounds.min.y + (bounds.max.y - bounds.min.y) * f64::from(i) / 5. - 0.125;
            let max_y =
                bounds.min.y + (bounds.max.y - bounds.min.y) * f64::from(i + 1) / 5. - 0.125;
            let mut wet = false;
            'slice: for x in bounds.min.x.floor() as i32..=(bounds.max.x.floor() as i32) {
                for y in min_y.floor() as i32..=(max_y.floor() as i32) {
                    for z in bounds.min.z.floor() as i32..=(bounds.max.z.floor() as i32) {
                        let s = world.get(IVec3::new(x, y, z));
                        if matches!(s >> 4, 8 | 9)
                            && f64::from(y + 1)
                                - if s & 15 < 8 {
                                    f64::from(s & 15) / 8.
                                } else {
                                    0.
                                }
                                >= min_y
                        {
                            wet = true;
                            break 'slice;
                        }
                    }
                }
            }
            if wet {
                submersion += 0.2;
            }
        }
        let old = *p;
        let before = (self.motion.x * self.motion.x + self.motion.z * self.motion.z).sqrt();
        if submersion < 1. {
            self.motion.y += f64::from(0.04f32) * (submersion * 2. - 1.);
        } else {
            if self.motion.y < 0. {
                self.motion.y /= 2.;
            }
            self.motion.y += f64::from(0.007f32);
        }
        let angle = f64::from((input.yaw - input.strafe * 90.) * std::f32::consts::PI / 180.);
        self.motion.x -= angle.sin() * self.speed * f64::from(input.forward) * f64::from(0.05f32);
        self.motion.z += angle.cos() * self.speed * f64::from(input.forward) * f64::from(0.05f32);
        let mut speed = (self.motion.x * self.motion.x + self.motion.z * self.motion.z).sqrt();
        if speed > 0.35 {
            let factor = 0.35 / speed;
            self.motion.x *= factor;
            self.motion.z *= factor;
            speed = 0.35;
        }
        if speed > before && self.speed < 0.35 {
            self.speed = (self.speed + (0.35 - self.speed) / 35.).min(0.35);
        } else {
            self.speed = (self.speed - (self.speed - 0.07) / 35.).max(0.07);
        }
        if self.grounded {
            self.motion *= 0.5;
        }
        let moved =
            collision::move_body_with_step(world, bounds, self.motion, self.grounded, false, 0.);
        *p = moved.bounds.feet();
        self.grounded = moved.grounded;
        for i in 0..3 {
            if moved.clipped[i] {
                self.motion[i] = 0.;
            }
        }
        if !(moved.clipped[0] || moved.clipped[2]) || before <= 0.2975 {
            self.drag();
        }
        self.pitch = 0.;
        let d = old - *p;
        if d.x * d.x + d.z * d.z > 0.001 {
            let wanted =
                (crate::gameplay::ai::motion::atan2(d.z, d.x) * 180. / std::f64::consts::PI) as f32;
            self.yaw += ((wanted - self.yaw + 180.).rem_euclid(360.) - 180.).clamp(-20., 20.);
        }
    }
    pub fn seat(&self, p: DVec3) -> DVec3 {
        match self.kind {
            Kind::Boat => {
                let angle = f64::from(self.yaw) * std::f64::consts::PI / 180.;
                p + DVec3::new(angle.cos() * 0.4, -0.3 - 0.35, angle.sin() * 0.4)
            }
            Kind::Cart => p - DVec3::Y * 0.35,
            Kind::Horse => {
                let angle = self.yaw * std::f32::consts::PI / 180.;
                let forward = 0.7 * self.previous_rearing;
                p + DVec3::new(
                    f64::from(forward * crate::vanilla::math::sin(angle)),
                    f64::from(1.6f32) * 0.75 - 0.35 + f64::from(0.15 * self.previous_rearing),
                    -f64::from(forward * crate::vanilla::math::cos(angle)),
                )
            }
        }
    }
    pub fn height(&self) -> f64 {
        match self.kind {
            Kind::Boat => f64::from(0.6f32),
            Kind::Cart => f64::from(0.7f32),
            Kind::Horse => f64::from(1.6f32),
        }
    }
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Rider {
    pub(super) input: RiderInput,
    pub(crate) ticks: u32,
    pub(crate) jumped_this_tick: bool,
    jump: bool,
    counter: i32,
    power: f32,
}
impl Rider {
    pub fn power(&self) -> f32 {
        self.power.clamp(0., 1.)
    }
    pub fn jump(&mut self, saddled: bool, jump: bool) -> Option<i32> {
        let old = self.jump;
        self.jump = jump;
        if !saddled {
            self.power = 0.;
            return None;
        }
        if self.counter < 0 {
            self.counter += 1;
            if self.counter == 0 {
                self.power = 0.;
            }
        }
        if old && !jump {
            self.counter = -10;
            return Some((self.power * 100.) as i32);
        }
        if !old && jump {
            self.counter = 0;
            self.power = 0.;
        } else if old {
            self.counter += 1;
            self.power = if self.counter < 10 {
                self.counter as f32 * 0.1
            } else {
                0.8 + 2. / (self.counter - 9) as f32 * 0.1
            };
        }
        None
    }
}
#[cfg(test)]
#[path = "remote_vehicle_tests.rs"]
mod oracle_tests;
