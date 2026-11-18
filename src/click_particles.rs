use crate::vanilla::{
    math,
    movement::{collision, Aabb, MovementWorld},
    random::JavaRandom,
};
use glam::{DVec3, IVec3, Vec3};

#[derive(Clone, Copy, Debug)]
enum ScaleCurve {
    Flat,
    Ramp32,
    FlameShrink,
    LavaShrink,
}

#[derive(Clone, Copy, Debug)]
enum FrameMode {
    Static(u8),
    Reverse8(u8),
    WakeCycle,
}

#[derive(Clone, Copy, Debug)]
enum LightMode {
    World,
    Lava,
    Fullbright,
    SkyRamp,
    FlameRamp,
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    Digging {
        state: u16,
        origin: IVec3,
    },
    Critical {
        magic: bool,
        rgb: [f32; 3],
    },
    Explosion,
    Footstep,
    Item {
        id: u16,
        meta: u16,
        full: bool,
    },
    Portal {
        origin: DVec3,
        frame: u8,
    },
    EnchantmentTable {
        origin: DVec3,
        frame: u8,
    },
    Drip {
        lava: bool,
        bob_timer: u8,
        frame: u8,
    },
    MobAppearance,
    Sprite {
        frame_mode: FrameMode,
        scale_curve: ScaleCurve,
        light_mode: LightMode,
        gravity: f64,
        friction: f32,
        no_clip: bool,
        boost_blocked_y: bool,
        water_only: bool,
        fade_half: bool,
        lava_smoke: bool,
        rain_drop_die: bool,
    },
}

#[derive(Clone, Debug)]
pub struct Particle {
    kind: Kind,
    position: DVec3,
    previous: DVec3,
    velocity: DVec3,
    bounds: Aabb,
    age: u8,
    max_age: u8,
    scale: f32,
    jitter: [f32; 2],
    color: [u8; 4],
    rng_state: u32,
}

impl Particle {
    fn new(
        kind: Kind,
        position: DVec3,
        direction: DVec3,
        math_random: &mut JavaRandom,
    ) -> (Self, JavaRandom) {
        let mut rand = JavaRandom::new(JavaRandom::independent_seed());
        let jitter = [rand.next_float() * 3., rand.next_float() * 3.];
        let scale = (rand.next_float() * 0.5 + 0.5) * 2.;
        let max_age = (4. / (rand.next_float() * 0.9 + 0.1)) as u8;
        let rng_state = (rand.next_int(i32::MAX) as u32).max(1);
        let motion = direction
            + DVec3::from_array(std::array::from_fn(|_| {
                (math_random.next_double() * 2. - 1.) * f64::from(0.4f32)
            }));
        let len = (motion.length() as f32).max(1e-6);
        let speed = (math_random.next_double() + math_random.next_double() + 1.) as f32 * 0.15;
        let velocity = motion / f64::from(len) * f64::from(speed) * f64::from(0.4f32)
            + DVec3::Y * f64::from(0.1f32);
        let r = f64::from(0.2f32 / 2.);
        (
            Self {
                kind,
                position,
                previous: position,
                velocity,
                age: 0,
                max_age,
                scale,
                jitter,
                color: [255; 4],
                rng_state,
                bounds: Aabb {
                    min: position - DVec3::new(r, 0., r),
                    max: position + DVec3::new(r, f64::from(0.2f32), r),
                },
            },
            rand,
        )
    }

    fn next_rand(&mut self) -> u32 {
        let mut x = self.rng_state;
        if x == 0 {
            x = 0x9E37_79B9;
        }
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng_state = x;
        x
    }

    fn shift_pos(&mut self, delta: DVec3) {
        self.position += delta;
        self.previous = self.position;
        self.bounds.min += delta;
        self.bounds.max += delta;
    }

    pub fn digging(
        position: DVec3,
        direction: DVec3,
        state: u16,
        origin: IVec3,
        hit: bool,
        random: &mut JavaRandom,
    ) -> Self {
        let (mut p, _) = Self::new(Kind::Digging { state, origin }, position, direction, random);
        p.color = [153, 153, 153, 255];
        p.scale *= 0.5;
        if hit {
            let up = DVec3::Y * f64::from(0.1f32);
            p.velocity = (p.velocity - up) * f64::from(0.2f32) + up;
            p.scale *= 0.6;
            p.bounds.max = p.bounds.min + DVec3::splat(f64::from(0.2f32 * 0.6));
        }
        p
    }

    pub fn block_dust(
        position: DVec3,
        velocity: DVec3,
        state: u16,
        origin: IVec3,
        random: &mut JavaRandom,
    ) -> Option<Self> {
        if matches!(state >> 4, 0 | 166) {
            return None;
        }
        let mut p = Self::digging(position, DVec3::ZERO, state, origin, false, random);
        p.velocity = velocity;
        Some(p)
    }

    pub fn digging_block(&self) -> Option<(u16, IVec3)> {
        if let Kind::Digging { state, origin } = self.kind {
            Some((state, origin))
        } else {
            None
        }
    }

    pub fn special(
        id: u8,
        position: DVec3,
        arguments: DVec3,
        random: &mut JavaRandom,
    ) -> Option<Self> {
        match id {
            23 => {
                let (mut p, _) = Self::new(
                    Kind::Sprite {
                        frame_mode: FrameMode::Static(64),
                        scale_curve: ScaleCurve::Ramp32,
                        light_mode: LightMode::World,
                        gravity: 0.,
                        friction: 0.66,
                        no_clip: false,
                        boost_blocked_y: true,
                        water_only: false,
                        fade_half: false,
                        lava_smoke: false,
                        rain_drop_die: false,
                    },
                    position,
                    DVec3::ZERO,
                    random,
                );
                p.velocity *= f64::from(0.01f32);
                p.velocity.y += 0.2;
                p.max_age = 6;
                p.scale = p.scale * 0.75 * 2.;
                let rgb = [0., 1. / 3., 2. / 3.].map(|offset| {
                    math::sin((arguments.x as f32 + offset) * std::f32::consts::PI * 2.) * 0.65
                        + 0.35
                });
                let [r, g, b] = rgb.map(|c| (c.clamp(0., 1.) * 255.) as u8);
                p.color = [r, g, b, 255];
                Some(p)
            }
            24 => {
                let (mut p, mut rand) = Self::new(
                    Kind::Portal {
                        origin: position,
                        frame: 0,
                    },
                    position,
                    arguments,
                    random,
                );
                p.velocity = arguments;
                let strength = rand.next_float() * 0.6 + 0.4;
                p.scale = rand.next_float() * 0.2 + 0.5;
                p.max_age = (random.next_double() * 10.) as u8 + 40;
                p.kind = Kind::Portal {
                    origin: position,
                    frame: (random.next_double() * 8.) as u8,
                };
                let rgb = [strength * 0.9, strength * 0.3, strength];
                let [r, g, b] = rgb.map(|c| (c.clamp(0., 1.) * 255.) as u8);
                p.color = [r, g, b, 255];
                Some(p)
            }
            30 => {
                let (mut p, _) = Self::new(
                    Kind::Sprite {
                        frame_mode: FrameMode::Reverse8(7),
                        scale_curve: ScaleCurve::Ramp32,
                        light_mode: LightMode::World,
                        gravity: 0.,
                        friction: 0.96,
                        no_clip: false,
                        boost_blocked_y: true,
                        water_only: false,
                        fade_half: false,
                        lava_smoke: false,
                        rain_drop_die: false,
                    },
                    position,
                    DVec3::ZERO,
                    random,
                );
                p.velocity *= f64::from(0.1f32);
                let strength = random.next_double() as f32 * 0.4 + 0.6;
                let mut channels = arguments.as_vec3().to_array();
                if channels[0] == 0. {
                    channels[0] = 1.;
                }
                let rgb = channels.map(|v| {
                    ((random.next_double() * f64::from(0.2f32)) as f32 + 0.8) * v * strength
                });
                p.scale *= 0.75;
                p.max_age = (8. / (random.next_double() * 0.8 + 0.2)) as u8;
                let [r, g, b] = rgb.map(|c| (c.clamp(0., 1.) * 255.) as u8);
                p.color = [r, g, b, 255];
                Some(p)
            }
            _ => None,
        }
    }

    pub fn surface_texture(&self) -> Option<(&'static str, bool)> {
        match self.kind {
            Kind::Footstep => Some(("textures/particle/footprint.png", true)),
            Kind::Explosion => Some(("textures/entity/explosion.png", false)),
            _ => None,
        }
    }

    pub fn item(
        id: u16,
        meta: u16,
        full: bool,
        point: DVec3,
        velocity: DVec3,
        random: &mut JavaRandom,
    ) -> Self {
        let (mut p, _) = Self::new(Kind::Item { id, meta, full }, point, DVec3::ZERO, random);
        p.scale *= 0.5;
        if full {
            p.velocity = DVec3::ZERO;
            p.max_age = 80;
            p.scale = 5.;
        } else {
            p.velocity = p.velocity * 0.1 + velocity;
        }
        p
    }

    pub fn breaking_poof(id: u16, point: DVec3, random: &mut JavaRandom) -> Self {
        let (mut p, _) = Self::new(Kind::Item { id, meta: 0, full: false }, point, DVec3::ZERO, random);
        p.scale *= 0.5;
        p
    }

    pub fn item_texture(&self) -> Option<(u16, u16)> {
        if let Kind::Item { id, meta, .. } = self.kind {
            Some((id, meta))
        } else {
            None
        }
    }

    pub fn is_mob_appearance(&self) -> Option<f32> {
        if matches!(self.kind, Kind::MobAppearance) {
            Some((f32::from(self.age) / f32::from(self.max_age.max(1))).clamp(0., 1.))
        } else {
            None
        }
    }

    pub fn light_override(&self, world_light: [u8; 4]) -> [u8; 4] {
        let phase = (f32::from(self.age) / f32::from(self.max_age.max(1))).clamp(0., 1.);
        match self.kind {
            Kind::Explosion => [240, 0, 0, 0],
            Kind::Drip { lava: true, .. } => [240, world_light[1], 0, 0],
            Kind::Portal { .. } | Kind::EnchantmentTable { .. } => {
                let f4 = phase * phase * phase * phase;
                let sky = (i32::from(world_light[1]) + (f4 * 240.) as i32).clamp(0, 240) as u8;
                [world_light[0], sky, 0, 0]
            }
            Kind::Sprite { light_mode, .. } => match light_mode {
                LightMode::World => world_light,
                LightMode::Lava => [240, world_light[1], 0, 0],
                LightMode::Fullbright => [240, 240, 0, 0],
                LightMode::SkyRamp => {
                    let f4 = phase * phase * phase * phase;
                    let sky = (i32::from(world_light[1]) + (f4 * 240.) as i32).clamp(0, 240) as u8;
                    [world_light[0], sky, 0, 0]
                }
                LightMode::FlameRamp => {
                    let block =
                        (i32::from(world_light[0]) + (phase * 240.) as i32).clamp(0, 240) as u8;
                    [block, world_light[1], 0, 0]
                }
            },
            _ => world_light,
        }
    }

    pub fn network_sprite(
        id: u8,
        at: DVec3,
        velocity: DVec3,
        random: &mut JavaRandom,
    ) -> Option<Self> {
        match id {
            23 | 24 | 30 => return Self::special(id, at, velocity, random),
            31 => {
                return Some(if velocity == DVec3::ZERO {
                    Self::breaking_poof(332, at, random)
                } else {
                    Self::item(332, 0, false, at, velocity, random)
                });
            }
            33 => {
                return Some(if velocity == DVec3::ZERO {
                    Self::breaking_poof(341, at, random)
                } else {
                    Self::item(341, 0, false, at, velocity, random)
                });
            }
            35 => return Some(Self::item(166, 0, true, at, DVec3::ZERO, random)),
            41 => {
                let (mut p, _) = Self::new(Kind::MobAppearance, at, DVec3::ZERO, random);
                p.velocity = DVec3::ZERO;
                p.age = 0;
                p.max_age = 30;
                return Some(p);
            }
            9 | 10 => {
                let magic = id == 10;
                let (mut p, _) = Self::new(
                    Kind::Critical {
                        magic,
                        rgb: [0.; 3],
                    },
                    at,
                    DVec3::ZERO,
                    random,
                );
                p.velocity = p.velocity * f64::from(0.1f32) + velocity * 0.4;
                let gray = (random.next_double() * f64::from(0.3f32) + f64::from(0.6f32)) as f32;
                let rgb = if magic {
                    [gray * 0.3, gray * 0.8, gray]
                } else {
                    [gray; 3]
                };
                p.kind = Kind::Critical { magic, rgb };
                p.scale *= 0.75;
                p.max_age = (6. / (random.next_double() * 0.8 + 0.6)) as u8;
                return Some(p);
            }
            1 | 2 | 28 => {
                let (mut p, mut rng) = Self::new(
                    if id == 28 {
                        Kind::Footstep
                    } else {
                        Kind::Explosion
                    },
                    at,
                    DVec3::ZERO,
                    random,
                );
                p.velocity = DVec3::ZERO;
                if id != 28 {
                    p.max_age = (6 + rng.next_int(4)) as u8;
                    p.scale = 2. * (1. - velocity.x as f32 * 0.5);
                    let gray = ((rng.next_float() * 0.6 + 0.4) * 255.) as u8;
                    p.color = [gray, gray, gray, 255];
                } else {
                    p.scale = 0.125;
                    p.max_age = 200;
                }
                return Some(p);
            }
            20 | 34 => {
                let spawn_at = if id == 20 { at + DVec3::Y * 0.5 } else { at };
                let (mut p, _) = Self::new(
                    Kind::Sprite {
                        frame_mode: FrameMode::Static(if id == 20 { 81 } else { 80 }),
                        scale_curve: ScaleCurve::Ramp32,
                        light_mode: LightMode::World,
                        gravity: 0.,
                        friction: 0.86,
                        no_clip: false,
                        boost_blocked_y: true,
                        water_only: false,
                        fade_half: false,
                        lava_smoke: false,
                        rain_drop_die: false,
                    },
                    spawn_at,
                    DVec3::ZERO,
                    random,
                );
                p.velocity = p.velocity * f64::from(0.01f32) + DVec3::Y * 0.1;
                p.scale = p.scale * 0.75 * 2.;
                p.max_age = 16;
                return Some(p);
            }
            18 | 19 => {
                let lava = id == 19;
                let (mut p, mut rng) = Self::new(
                    Kind::Drip {
                        lava,
                        bob_timer: 40,
                        frame: 113,
                    },
                    at,
                    DVec3::ZERO,
                    random,
                );
                p.velocity = DVec3::ZERO;
                p.max_age = (64. / (f64::from(rng.next_float()) * 0.8 + 0.2)).min(250.) as u8;
                p.color = if lava {
                    [255, 255, 127, 255]
                } else {
                    [51, 77, 255, 255]
                };
                return Some(p);
            }
            25 => {
                let (mut p, mut rng) = Self::new(
                    Kind::EnchantmentTable {
                        origin: at,
                        frame: 225,
                    },
                    at,
                    velocity,
                    random,
                );
                let frame = 225 + (rng.next_float() * 26.) as u8;
                p.kind = Kind::EnchantmentTable { origin: at, frame };
                p.velocity = velocity;
                p.scale = rng.next_float() * 0.5 + 0.2;
                let b = rng.next_float() * 0.6 + 0.4;
                p.color = [
                    (0.9 * b * 255.) as u8,
                    (0.9 * b * 255.) as u8,
                    (b * 255.) as u8,
                    255,
                ];
                p.max_age = (rng.next_float() * 10.) as u8 + 30;
                return Some(p);
            }
            0 => {
                let (mut p, mut rng) = Self::new(
                    Kind::Sprite {
                        frame_mode: FrameMode::Reverse8(7),
                        scale_curve: ScaleCurve::Flat,
                        light_mode: LightMode::World,
                        gravity: -0.004,
                        friction: 0.90,
                        no_clip: false,
                        boost_blocked_y: false,
                        water_only: false,
                        fade_half: false,
                        lava_smoke: false,
                        rain_drop_die: false,
                    },
                    at,
                    DVec3::ZERO,
                    random,
                );
                p.velocity = velocity
                    + DVec3::from_array(std::array::from_fn(|_| {
                        f64::from(rng.next_float() * 2. - 1.) * 0.05
                    }));
                let c = ((rng.next_float() * 0.3 + 0.7) * 255.) as u8;
                p.color = [c, c, c, 255];
                let r = rng.next_float();
                p.scale = r * r * 6. + 1.;
                p.max_age = ((16. / (f64::from(rng.next_float()) * 0.8 + 0.2)) as u8)
                    .saturating_add(2);
                return Some(p);
            }
            3 => {
                let (mut p, mut rng) = Self::new(
                    Kind::Sprite {
                        frame_mode: FrameMode::Reverse8(167),
                        scale_curve: ScaleCurve::Flat,
                        light_mode: LightMode::Fullbright,
                        gravity: 0.004,
                        friction: 0.91,
                        no_clip: false,
                        boost_blocked_y: false,
                        water_only: false,
                        fade_half: true,
                        lava_smoke: false,
                        rain_drop_die: false,
                    },
                    at,
                    DVec3::ZERO,
                    random,
                );
                p.velocity = velocity;
                p.scale *= 0.75;
                p.max_age = 48 + rng.next_int(12) as u8;
                return Some(p);
            }
            4 => {
                let (mut p, mut rng) = Self::new(
                    Kind::Sprite {
                        frame_mode: FrameMode::Static(32),
                        scale_curve: ScaleCurve::Flat,
                        light_mode: LightMode::World,
                        gravity: -0.002,
                        friction: 0.85,
                        no_clip: false,
                        boost_blocked_y: false,
                        water_only: true,
                        fade_half: false,
                        lava_smoke: false,
                        rain_drop_die: false,
                    },
                    at,
                    DVec3::ZERO,
                    random,
                );
                p.velocity = velocity * 0.2
                    + DVec3::from_array(std::array::from_fn(|_| {
                        f64::from(rng.next_float() * 2. - 1.) * 0.02
                    }));
                p.scale *= rng.next_float() * 0.6 + 0.2;
                p.max_age = (8. / (f64::from(rng.next_float()) * 0.8 + 0.2)) as u8;
                return Some(p);
            }
            5 => {
                let (mut p, mut rng) = Self::new(
                    Kind::Sprite {
                        frame_mode: FrameMode::Static(20),
                        scale_curve: ScaleCurve::Flat,
                        light_mode: LightMode::World,
                        gravity: 0.04,
                        friction: 0.98,
                        no_clip: false,
                        boost_blocked_y: false,
                        water_only: false,
                        fade_half: false,
                        lava_smoke: false,
                        rain_drop_die: false,
                    },
                    at,
                    DVec3::ZERO,
                    random,
                );
                let frame = 20 + rng.next_int(4) as u8;
                if let Kind::Sprite { frame_mode, .. } = &mut p.kind {
                    *frame_mode = FrameMode::Static(frame);
                }
                let base = p.velocity;
                p.velocity = if velocity.y == 0. && (velocity.x != 0. || velocity.z != 0.) {
                    DVec3::new(velocity.x, velocity.y + 0.1, velocity.z)
                } else {
                    DVec3::new(
                        base.x * 0.3,
                        f64::from(rng.next_float() * 0.2 + 0.1),
                        base.z * 0.3,
                    )
                };
                p.max_age = (8. / (f64::from(rng.next_float()) * 0.8 + 0.2)) as u8;
                return Some(p);
            }
            6 => {
                let (mut p, mut rng) = Self::new(
                    Kind::Sprite {
                        frame_mode: FrameMode::WakeCycle,
                        scale_curve: ScaleCurve::Flat,
                        light_mode: LightMode::World,
                        gravity: 0.,
                        friction: 0.98,
                        no_clip: false,
                        boost_blocked_y: false,
                        water_only: false,
                        fade_half: false,
                        lava_smoke: false,
                        rain_drop_die: false,
                    },
                    at,
                    DVec3::ZERO,
                    random,
                );
                p.velocity = velocity;
                p.max_age = (8. / (f64::from(rng.next_float()) * 0.8 + 0.2)) as u8;
                return Some(p);
            }
            7 => {
                let (mut p, mut rng) = Self::new(
                    Kind::Sprite {
                        frame_mode: FrameMode::Static(0),
                        scale_curve: ScaleCurve::Flat,
                        light_mode: LightMode::World,
                        gravity: 0.,
                        friction: 1.0,
                        no_clip: false,
                        boost_blocked_y: false,
                        water_only: true,
                        fade_half: false,
                        lava_smoke: false,
                        rain_drop_die: false,
                    },
                    at,
                    DVec3::ZERO,
                    random,
                );
                p.shift_pos(DVec3::new(0., -0.125, 0.));
                p.velocity = DVec3::ZERO;
                p.color = [102, 102, 179, 255];
                p.scale *= rng.next_float() * 0.6 + 0.2;
                p.max_age = (16. / (f64::from(rng.next_float()) * 0.8 + 0.2)) as u8;
                return Some(p);
            }
            8 | 21 | 22 => {
                let frame = if id == 21 { 82 } else { 0 };
                let (mut p, mut rng) = Self::new(
                    Kind::Sprite {
                        frame_mode: FrameMode::Static(frame),
                        scale_curve: ScaleCurve::Flat,
                        light_mode: LightMode::World,
                        gravity: 0.,
                        friction: 0.99,
                        no_clip: true,
                        boost_blocked_y: false,
                        water_only: false,
                        fade_half: false,
                        lava_smoke: false,
                        rain_drop_die: false,
                    },
                    at,
                    DVec3::ZERO,
                    random,
                );
                let base = p.velocity;
                p.velocity = base * 0.02 + velocity * 0.02;
                if id != 21 {
                    let gray = ((rng.next_float() * 0.1 + 0.2) * 255.) as u8;
                    p.color = [gray, gray, gray, 255];
                }
                p.scale *= rng.next_float() * 0.6 + 0.5;
                p.max_age = (20. / (f64::from(rng.next_float()) * 0.8 + 0.2)) as u8;
                return Some(p);
            }
            11 | 12 => {
                let (mut p, mut rng) = Self::new(
                    Kind::Sprite {
                        frame_mode: FrameMode::Reverse8(7),
                        scale_curve: ScaleCurve::Ramp32,
                        light_mode: LightMode::World,
                        gravity: -0.004,
                        friction: 0.96,
                        no_clip: false,
                        boost_blocked_y: true,
                        water_only: false,
                        fade_half: false,
                        lava_smoke: false,
                        rain_drop_die: false,
                    },
                    at,
                    DVec3::ZERO,
                    random,
                );
                p.velocity = p.velocity * 0.1 + velocity;
                let gray = (rng.next_float() * 0.3 * 255.) as u8;
                p.color = [gray, gray, gray, 255];
                let mult = if id == 12 { 2.5 } else { 1.0 };
                p.scale *= 0.75 * mult;
                p.max_age = ((8. / (f64::from(rng.next_float()) * 0.8 + 0.2)) * f64::from(mult))
                    .min(250.) as u8;
                return Some(p);
            }
            13..=17 => {
                let base_frame = if id == 14 || id == 17 { 151 } else { 135 };
                let dir = if matches!(id, 15 | 16) {
                    DVec3::ZERO
                } else {
                    velocity
                };
                let (mut p, mut rng) = Self::new(
                    Kind::Sprite {
                        frame_mode: FrameMode::Reverse8(base_frame),
                        scale_curve: ScaleCurve::Flat,
                        light_mode: LightMode::World,
                        gravity: -0.004,
                        friction: 0.96,
                        no_clip: false,
                        boost_blocked_y: true,
                        water_only: false,
                        fade_half: false,
                        lava_smoke: false,
                        rain_drop_die: false,
                    },
                    at,
                    dir,
                    random,
                );
                p.velocity.y *= 0.2;
                if matches!(id, 15 | 16) || (velocity.x == 0. && velocity.z == 0.) {
                    p.velocity.x *= 0.1;
                    p.velocity.z *= 0.1;
                }
                p.scale *= 0.75;
                p.max_age = (8. / (f64::from(rng.next_float()) * 0.8 + 0.2)) as u8;
                if matches!(id, 15 | 16) {
                    p.color = [
                        (velocity.x.clamp(0., 1.) * 255.) as u8,
                        (velocity.y.clamp(0., 1.) * 255.) as u8,
                        (velocity.z.clamp(0., 1.) * 255.) as u8,
                        if id == 16 { 38 } else { 255 },
                    ];
                } else if id == 17 {
                    let b = ((rng.next_float() * 0.5 + 0.35) * 255.) as u8;
                    p.color = [b, 0, b, 255];
                }
                return Some(p);
            }
            26 => {
                let (mut p, mut rng) = Self::new(
                    Kind::Sprite {
                        frame_mode: FrameMode::Static(48),
                        scale_curve: ScaleCurve::FlameShrink,
                        light_mode: LightMode::FlameRamp,
                        gravity: 0.,
                        friction: 0.96,
                        no_clip: true,
                        boost_blocked_y: false,
                        water_only: false,
                        fade_half: false,
                        lava_smoke: false,
                        rain_drop_die: false,
                    },
                    at,
                    DVec3::ZERO,
                    random,
                );
                let jitter_pos = DVec3::from_array(std::array::from_fn(|_| {
                    f64::from((rng.next_float() - rng.next_float()) * 0.05)
                }));
                p.shift_pos(jitter_pos);
                p.velocity = p.velocity * 0.01 + velocity;
                p.max_age = ((8. / (f64::from(rng.next_float()) * 0.8 + 0.2)) as u8)
                    .saturating_add(4);
                return Some(p);
            }
            27 => {
                let (mut p, mut rng) = Self::new(
                    Kind::Sprite {
                        frame_mode: FrameMode::Static(49),
                        scale_curve: ScaleCurve::LavaShrink,
                        light_mode: LightMode::Lava,
                        gravity: 0.03,
                        friction: 0.999,
                        no_clip: false,
                        boost_blocked_y: false,
                        water_only: false,
                        fade_half: false,
                        lava_smoke: true,
                        rain_drop_die: false,
                    },
                    at,
                    DVec3::ZERO,
                    random,
                );
                p.velocity = DVec3::new(
                    p.velocity.x * 0.8,
                    f64::from(rng.next_float() * 0.4 + 0.05),
                    p.velocity.z * 0.8,
                );
                p.scale *= rng.next_float() * 2. + 0.2;
                p.max_age = (16. / (f64::from(rng.next_float()) * 0.8 + 0.2)) as u8;
                return Some(p);
            }
            29 => {
                let (mut p, mut rng) = Self::new(
                    Kind::Sprite {
                        frame_mode: FrameMode::Reverse8(7),
                        scale_curve: ScaleCurve::Ramp32,
                        light_mode: LightMode::World,
                        gravity: 0.,
                        friction: 0.96,
                        no_clip: false,
                        boost_blocked_y: false,
                        water_only: false,
                        fade_half: false,
                        lava_smoke: false,
                        rain_drop_die: false,
                    },
                    at,
                    DVec3::ZERO,
                    random,
                );
                p.velocity = p.velocity * 0.1 + velocity;
                let gray = ((1. - rng.next_float() * 0.3) * 255.) as u8;
                p.color = [gray, gray, gray, 255];
                p.scale *= 0.75 * 2.5;
                p.max_age = ((8. / (f64::from(rng.next_float()) * 0.8 + 0.3)) * 2.5).min(250.) as u8;
                return Some(p);
            }
            32 => {
                let (mut p, mut rng) = Self::new(
                    Kind::Sprite {
                        frame_mode: FrameMode::Reverse8(7),
                        scale_curve: ScaleCurve::Ramp32,
                        light_mode: LightMode::World,
                        gravity: 0.03,
                        friction: 0.99,
                        no_clip: false,
                        boost_blocked_y: false,
                        water_only: false,
                        fade_half: false,
                        lava_smoke: false,
                        rain_drop_die: false,
                    },
                    at,
                    DVec3::ZERO,
                    random,
                );
                p.velocity = p.velocity * 0.1 + velocity;
                let gray = ((1. - rng.next_float() * 0.3) * 255.) as u8;
                p.color = [gray, gray, gray, 255];
                p.scale *= 0.75;
                p.max_age = (8. / (f64::from(rng.next_float()) * 0.8 + 0.2)) as u8;
                return Some(p);
            }
            39 => {
                let (mut p, mut rng) = Self::new(
                    Kind::Sprite {
                        frame_mode: FrameMode::Static(19),
                        scale_curve: ScaleCurve::Flat,
                        light_mode: LightMode::World,
                        gravity: 0.06,
                        friction: 0.98,
                        no_clip: false,
                        boost_blocked_y: false,
                        water_only: false,
                        fade_half: false,
                        lava_smoke: false,
                        rain_drop_die: true,
                    },
                    at,
                    DVec3::ZERO,
                    random,
                );
                let frame = 19 + rng.next_int(4) as u8;
                if let Kind::Sprite { frame_mode, .. } = &mut p.kind {
                    *frame_mode = FrameMode::Static(frame);
                }
                p.velocity = DVec3::new(
                    p.velocity.x * 0.3,
                    f64::from(rng.next_float() * 0.2 + 0.1),
                    p.velocity.z * 0.3,
                );
                p.max_age = (8. / (f64::from(rng.next_float()) * 0.8 + 0.2)) as u8;
                return Some(p);
            }
            _ => None,
        }
    }

    pub fn potion_splash(
        at: DVec3,
        motion: DVec3,
        speed: f32,
        brightness: f32,
        random: &mut JavaRandom,
    ) -> Option<Self> {
        Self::potion_splash_colored(at, motion, speed, brightness, 0x385dc6, random)
    }

    pub fn potion_splash_colored(
        at: DVec3,
        motion: DVec3,
        speed: f32,
        brightness: f32,
        rgb: u32,
        random: &mut JavaRandom,
    ) -> Option<Self> {
        Self::potion_splash_effect(false, at, motion, speed, brightness, rgb, random)
    }

    pub fn potion_splash_effect(
        instant: bool,
        at: DVec3,
        motion: DVec3,
        speed: f32,
        brightness: f32,
        rgb: u32,
        random: &mut JavaRandom,
    ) -> Option<Self> {
        let mut p = Self::network_sprite(if instant { 14 } else { 13 }, at, motion, random)?;
        p.color = [
            ((rgb >> 16) & 0xff) as u8,
            ((rgb >> 8) & 0xff) as u8,
            (rgb & 0xff) as u8,
            255,
        ];
        for channel in &mut p.color[..3] {
            *channel = (f32::from(*channel) * brightness) as u8;
        }
        p.velocity = (p.velocity - DVec3::Y * 0.1) * f64::from(speed) + DVec3::Y * 0.1;
        Some(p)
    }

    pub fn critical(
        magic: bool,
        at: DVec3,
        direction: DVec3,
        random: &mut JavaRandom,
        world: &impl MovementWorld,
    ) -> Self {
        let (mut p, _) = Self::new(
            Kind::Critical {
                magic,
                rgb: [0.; 3],
            },
            at,
            DVec3::ZERO,
            random,
        );
        p.velocity = p.velocity * f64::from(0.1f32) + direction * 0.4;
        let gray = (random.next_double() * f64::from(0.3f32) + f64::from(0.6f32)) as f32;
        p.kind = Kind::Critical {
            magic,
            rgb: [gray; 3],
        };
        p.scale *= 0.75;
        p.max_age = (6. / (random.next_double() * 0.8 + 0.6)) as u8;
        p.tick(world);
        if let Kind::Critical { magic: true, rgb } = &mut p.kind {
            rgb[0] *= 0.3;
            rgb[1] *= 0.8;
        }
        p
    }

    pub fn tick(&mut self, world: &impl MovementWorld) -> bool {
        let mut ignored = Vec::new();
        self.tick_with_spawns(world, &mut ignored)
    }

    pub fn tick_with_spawns(
        &mut self,
        world: &impl MovementWorld,
        spawned: &mut Vec<Self>,
    ) -> bool {
        self.previous = self.position;
        let old_age = self.age;
        self.age = self.age.saturating_add(1);
        match self.kind {
            Kind::Explosion | Kind::Footstep | Kind::MobAppearance => {
                return self.age < self.max_age;
            }
            Kind::Portal { origin, .. } => {
                let phase = f32::from(old_age) / f32::from(self.max_age.max(1));
                let f = 1. - (-phase + phase * phase * 2.);
                self.position =
                    origin + self.velocity * f64::from(f) + DVec3::Y * f64::from(1. - phase);
                return old_age < self.max_age;
            }
            Kind::EnchantmentTable { origin, .. } => {
                let f = 1. - f32::from(old_age) / f32::from(self.max_age.max(1));
                let f2 = (1. - f).powi(4);
                self.position =
                    origin + self.velocity * f64::from(f) - DVec3::Y * f64::from(f2 * 1.2);
                return old_age < self.max_age;
            }
            Kind::Drip {
                lava,
                mut bob_timer,
                mut frame,
            } => {
                if lava {
                    let bob = f32::from(bob_timer);
                    let g = (16. / (56. - bob) * 255.).clamp(0., 255.) as u8;
                    let b = (4. / (48. - bob) * 255.).clamp(0., 255.) as u8;
                    self.color = [255, g, b, 255];
                } else {
                    self.color = [51, 77, 255, 255];
                }
                if bob_timer > 0 {
                    bob_timer -= 1;
                    self.velocity *= 0.02;
                    frame = 113;
                } else {
                    self.velocity.y -= 0.06;
                    frame = 112;
                }
                let movement = collision::move_body_with_step(
                    world,
                    self.bounds,
                    self.velocity,
                    false,
                    false,
                    0.,
                );
                self.bounds = movement.bounds;
                self.position = self.bounds.feet();
                for (axis, clipped) in movement.clipped.into_iter().enumerate() {
                    if clipped {
                        self.velocity[axis] = 0.;
                    }
                }
                self.velocity *= 0.98;
                if movement.grounded {
                    if !lava {
                        let seed = i64::from(self.next_rand());
                        let mut rng = JavaRandom::new(seed);
                        if let Some(splash) =
                            Self::network_sprite(5, self.position, DVec3::ZERO, &mut rng)
                        {
                            spawned.push(splash);
                        }
                        return false;
                    }
                    frame = 114;
                    self.velocity.x *= 0.7;
                    self.velocity.z *= 0.7;
                }
                self.kind = Kind::Drip {
                    lava,
                    bob_timer,
                    frame,
                };
                if bob_timer == 0
                    && matches!(
                        world.get(self.position.floor().as_ivec3()) >> 4,
                        8..=11
                    )
                {
                    return false;
                }
                return old_age < self.max_age;
            }
            _ => {}
        }

        let mut no_clip = false;
        let mut boost_blocked_y = false;
        let mut water_only = false;
        let mut fade_half = false;
        let mut lava_smoke = false;
        let mut rain_drop_die = false;
        let friction = match self.kind {
            Kind::Digging { .. } => {
                self.velocity.y -= 0.04;
                0.98f32
            }
            Kind::Item { full, .. } => {
                if !full {
                    self.velocity.y -= 0.04;
                }
                0.98
            }
            Kind::Critical { .. } => 0.7,
            Kind::Sprite {
                gravity,
                friction,
                no_clip: nc,
                boost_blocked_y: bby,
                water_only: wo,
                fade_half: fh,
                lava_smoke: ls,
                rain_drop_die: rdd,
                ..
            } => {
                self.velocity.y -= gravity;
                no_clip = nc;
                boost_blocked_y = bby;
                water_only = wo;
                fade_half = fh;
                lava_smoke = ls;
                rain_drop_die = rdd;
                friction
            }
            Kind::Portal { .. }
            | Kind::EnchantmentTable { .. }
            | Kind::Drip { .. }
            | Kind::Explosion
            | Kind::Footstep
            | Kind::MobAppearance => unreachable!(),
        };

        if lava_smoke {
            let frac = f32::from(old_age) / f32::from(self.max_age.max(1));
            let rand_bits = self.next_rand();
            let pseudo_rand = (rand_bits as f32) / (u32::MAX as f32);
            if frac < pseudo_rand {
                let mut rng = JavaRandom::new(i64::from(rand_bits));
                if let Some(smoke) =
                    Self::network_sprite(11, self.position, self.velocity, &mut rng)
                {
                    spawned.push(smoke);
                }
            }
        }

        let grounded = if no_clip {
            let delta = self.velocity;
            self.bounds.min += delta;
            self.bounds.max += delta;
            self.position = self.bounds.feet();
            false
        } else {
            let movement = collision::move_body_with_step(
                world,
                self.bounds,
                self.velocity,
                false,
                false,
                0.,
            );
            self.bounds = movement.bounds;
            self.position = self.bounds.feet();
            for (axis, clipped) in movement.clipped.into_iter().enumerate() {
                if clipped {
                    self.velocity[axis] = 0.;
                }
            }
            movement.grounded
        };

        if boost_blocked_y && self.position.y == self.previous.y {
            self.velocity.x *= 1.1;
            self.velocity.z *= 1.1;
        }
        self.velocity *= f64::from(friction);
        if let Kind::Critical { rgb, .. } = &mut self.kind {
            rgb[1] = (f64::from(rgb[1]) * 0.96) as f32;
            rgb[2] = (f64::from(rgb[2]) * 0.9) as f32;
            self.velocity.y -= f64::from(0.02f32);
        }
        if grounded && !water_only {
            if rain_drop_die && (self.next_rand() & 1) == 0 {
                return false;
            }
            self.velocity.x *= f64::from(0.7f32);
            self.velocity.z *= f64::from(0.7f32);
        }
        if fade_half {
            let half = self.max_age / 2;
            if self.age > half {
                let rem = (self.max_age - half).max(1);
                let frac = 1. - f32::from(self.age - half) / f32::from(rem);
                self.color[3] = (frac.clamp(0., 1.) * 255.) as u8;
            }
        }
        if water_only
            && !matches!(
                world.get(self.position.floor().as_ivec3()) >> 4,
                8 | 9
            )
        {
            return false;
        }
        old_age < self.max_age
    }

    pub fn appearance(&self, alpha: f32) -> (Vec3, f32, [u8; 4], Option<u16>, [f32; 4]) {
        let max_age = f32::from(self.max_age.max(1));
        let phase = ((f32::from(self.age) + alpha) / max_age).clamp(0., 1.);
        let position = self
            .previous
            .lerp(self.position, f64::from(alpha))
            .as_vec3();
        let mut color = self.color;
        let (factor, frame) = match self.kind {
            Kind::Footstep => {
                let opacity = ((2. - 2. * phase * phase).clamp(0., 1.) * 51.) as u8;
                return (
                    position,
                    self.scale,
                    [255, 255, 255, opacity],
                    None,
                    [0., 0., 1., 1.],
                );
            }
            Kind::Explosion => {
                let i = (phase * 15.).clamp(0., 15.) as u8;
                return (
                    position,
                    self.scale,
                    self.color,
                    None,
                    [
                        f32::from(i % 4) / 4.,
                        f32::from(i / 4) / 4.,
                        0.24975,
                        0.24975,
                    ],
                );
            }
            Kind::MobAppearance => {
                return (position, self.scale, self.color, None, [0., 0., 1., 1.]);
            }
            Kind::Digging { state, .. } => {
                return (
                    position,
                    self.scale * 0.1,
                    self.color,
                    Some(state),
                    [self.jitter[0] / 4., self.jitter[1] / 4., 0.25, 0.25],
                );
            }
            Kind::Item { full, .. } => {
                let uv = if full {
                    [0., 0., 1., 1.]
                } else {
                    [self.jitter[0] / 4., self.jitter[1] / 4., 0.25, 0.25]
                };
                return (position, self.scale * 0.1, self.color, None, uv);
            }
            Kind::Critical { magic, rgb } => {
                color[..3].copy_from_slice(&rgb.map(|c| (c * 255.) as u8));
                ((phase * 32.).clamp(0., 1.), if magic { 66 } else { 65 })
            }
            Kind::Portal { frame, .. } => (1. - (1. - phase) * (1. - phase), frame),
            Kind::EnchantmentTable { frame, .. } => (1., frame),
            Kind::Drip { frame, .. } => (1., frame),
            Kind::Sprite {
                frame_mode,
                scale_curve,
                fade_half,
                ..
            } => {
                if fade_half {
                    let half = self.max_age / 2;
                    if self.age > half {
                        let rem = f32::from((self.max_age - half).max(1));
                        let frac = 1. - (f32::from(self.age) + alpha - f32::from(half)) / rem;
                        color[3] = (frac.clamp(0., 1.) * 255.) as u8;
                    } else {
                        color[3] = 255;
                    }
                }
                let factor = match scale_curve {
                    ScaleCurve::Flat => 1.,
                    ScaleCurve::Ramp32 => (phase * 32.).clamp(0., 1.),
                    ScaleCurve::FlameShrink => (1. - phase * phase * 0.5).max(0.),
                    ScaleCurve::LavaShrink => (1. - phase * phase).max(0.),
                };
                let frame = match frame_mode {
                    FrameMode::Static(f) => f,
                    FrameMode::Reverse8(base) => base - ((phase * 8.) as u8).min(7),
                    FrameMode::WakeCycle => 19 + (self.max_age.saturating_sub(self.age)) % 4,
                };
                (factor, frame)
            }
        };
        // The resource atlas stores the original 128px particle sheet in a 256px page.
        let uv = [
            f32::from(frame % 16) / 32.,
            f32::from(frame / 16) / 32.,
            0.999 / 32.,
            0.999 / 32.,
        ];
        (position, self.scale * 0.1 * factor, color, None, uv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct TestWorld {
        blocks: HashMap<IVec3, u16>,
    }

    impl MovementWorld for TestWorld {
        fn get(&self, pos: IVec3) -> u16 {
            self.blocks.get(&pos).copied().unwrap_or(0)
        }
        fn block_boxes(&self, p: IVec3, state: u16, out: &mut Vec<Aabb>) {
            let id = state >> 4;
            if id != 0 && !matches!(id, 8..=11) {
                let min = p.as_dvec3();
                out.push(Aabb {
                    min,
                    max: min + DVec3::ONE,
                });
            }
        }
        fn loaded_column(&self, _x: i32, _z: i32) -> bool {
            true
        }
    }

    fn frame_from_uv(uv: [f32; 4]) -> u8 {
        let tx = (uv[0] * 32.).round() as u8;
        let ty = (uv[1] * 32.).round() as u8;
        ty * 16 + tx
    }

    #[test]
    fn fireworks_spark_falls_fullbright_and_fades_after_half_life() {
        let mut rng = JavaRandom::new(42);
        let world = TestWorld::default();
        let mut p =
            Particle::network_sprite(3, DVec3::new(0., 10., 0.), DVec3::ZERO, &mut rng).unwrap();
        assert!((48..60).contains(&p.max_age));
        assert_eq!(p.light_override([16, 32, 0, 0]), [240, 240, 0, 0]);
        let (_, _, c0, _, uv0) = p.appearance(0.);
        assert_eq!(frame_from_uv(uv0), 167);
        assert_eq!(c0[3], 255);

        let start_y = p.position.y;
        let half = p.max_age / 2;
        for _ in 0..=half {
            assert!(p.tick(&world));
        }
        assert!(p.position.y < start_y, "fireworks spark must fall down");
        let (_, _, c_fade, _, _) = p.appearance(0.);
        assert!(c_fade[3] < 255, "alpha must fade once age > max_age / 2");
        while p.age + 1 < p.max_age {
            p.tick(&world);
        }
        let (_, _, c_end, _, uv_end) = p.appearance(1.);
        assert_eq!(c_end[3], 0);
        assert_eq!(frame_from_uv(uv_end), 160);
    }

    #[test]
    fn water_bubble_survives_in_water_and_dies_immediately_in_air() {
        let mut rng = JavaRandom::new(7);
        let mut world = TestWorld::default();
        world.blocks.insert(IVec3::new(0, 5, 0), 9 << 4);
        let mut p =
            Particle::network_sprite(4, DVec3::new(0.5, 5.5, 0.5), DVec3::ZERO, &mut rng).unwrap();
        assert_eq!(frame_from_uv(p.appearance(0.).4), 32);
        assert!(p.tick(&world), "bubble stays alive in water");

        let mut air_bubble =
            Particle::network_sprite(4, DVec3::new(0.5, 8.5, 0.5), DVec3::ZERO, &mut rng).unwrap();
        assert!(
            !air_bubble.tick(&world),
            "bubble dies immediately outside water"
        );
    }

    #[test]
    fn drip_water_and_lava_hang_fall_splash_or_puddle() {
        let mut rng = JavaRandom::new(99);
        let mut world = TestWorld::default();
        world.blocks.insert(IVec3::new(0, 0, 0), 1 << 4);

        let mut water =
            Particle::network_sprite(18, DVec3::new(0.5, 1.5, 0.5), DVec3::ZERO, &mut rng).unwrap();
        water.max_age = 200;
        let mut spawned = Vec::new();
        for _ in 0..40 {
            assert!(water.tick_with_spawns(&world, &mut spawned));
            assert_eq!(frame_from_uv(water.appearance(0.).4), 113);
        }
        let mut alive = true;
        for _ in 0..40 {
            alive = water.tick_with_spawns(&world, &mut spawned);
            if !alive {
                break;
            }
            assert_eq!(frame_from_uv(water.appearance(0.).4), 112);
        }
        assert!(!alive, "water drip dies on hitting ground");
        assert_eq!(spawned.len(), 1, "water drip spawns WATER_SPLASH on impact");
        assert!((20..=23).contains(&frame_from_uv(spawned[0].appearance(0.).4)));

        let mut lava =
            Particle::network_sprite(19, DVec3::new(0.5, 1.5, 0.5), DVec3::ZERO, &mut rng).unwrap();
        lava.max_age = 200;
        assert_eq!(lava.light_override([32, 128, 0, 0]), [240, 128, 0, 0]);
        let mut lava_spawns = Vec::new();
        assert!(lava.tick_with_spawns(&world, &mut lava_spawns));
        let warm_green = lava.appearance(0.).2[1];
        for _ in 1..40 {
            assert!(lava.tick_with_spawns(&world, &mut lava_spawns));
        }
        assert!(
            lava.appearance(0.).2[1] < warm_green,
            "lava drip cools while hanging"
        );
        for _ in 0..30 {
            lava.tick_with_spawns(&world, &mut lava_spawns);
            if frame_from_uv(lava.appearance(0.).4) == 114 {
                break;
            }
        }
        assert_eq!(frame_from_uv(lava.appearance(0.).4), 114);
    }

    #[test]
    fn enchantment_table_arc_and_sky_light_ramp() {
        let mut rng = JavaRandom::new(123);
        let world = TestWorld::default();
        let origin = DVec3::new(5.5, 66.0, 5.5);
        let vel = DVec3::new(2.0, 1.0, -1.5);
        let mut p = Particle::network_sprite(25, origin, vel, &mut rng).unwrap();
        let frame = frame_from_uv(p.appearance(0.).4);
        assert!((225..=250).contains(&frame));
        assert_eq!(p.light_override([16, 32, 0, 0])[1], 32);

        p.tick(&world);
        assert!((p.position - (origin + vel)).length() < 1e-5);
        while p.tick(&world) {}
        assert!(p.light_override([16, 32, 0, 0])[1] >= 200);
    }

    #[test]
    fn lava_spark_pops_upward_glows_and_spawns_trailing_smoke() {
        let mut rng = JavaRandom::new(555);
        let world = TestWorld::default();
        let mut spawned = Vec::new();
        for _ in 0..16 {
            let mut p =
                Particle::network_sprite(27, DVec3::new(0., 64., 0.), DVec3::ZERO, &mut rng)
                    .unwrap();
            assert_eq!(frame_from_uv(p.appearance(0.).4), 49);
            assert!(p.velocity.y >= 0.05);
            assert_eq!(p.light_override([0, 96, 0, 0]), [240, 96, 0, 0]);
            while p.tick_with_spawns(&world, &mut spawned) {}
        }
        assert!(
            !spawned.is_empty(),
            "lava spark must emit trailing SMOKE_NORMAL particles"
        );
    }

    #[test]
    fn cloud_and_snow_shovel_are_white_with_distinct_gravity() {
        let mut rng = JavaRandom::new(888);
        let world = TestWorld::default();
        let mut cloud =
            Particle::network_sprite(29, DVec3::new(0., 20., 0.), DVec3::ZERO, &mut rng).unwrap();
        let mut snow =
            Particle::network_sprite(32, DVec3::new(0., 20., 0.), DVec3::ZERO, &mut rng).unwrap();
        assert!(cloud.appearance(0.).2[0] >= 178);
        assert!(snow.appearance(0.).2[0] >= 178);

        cloud.velocity = DVec3::ZERO;
        snow.velocity = DVec3::ZERO;
        cloud.tick(&world);
        snow.tick(&world);
        assert!(
            (cloud.position.y - 20.0).abs() < 1e-6,
            "cloud has zero gravity"
        );
        assert!(snow.position.y < 20.0, "snow shovel falls down");
    }

    #[test]
    fn mob_appearance_tracks_30_tick_stationary_phase() {
        let mut rng = JavaRandom::new(11);
        let world = TestWorld::default();
        let mut p =
            Particle::network_sprite(41, DVec3::new(1., 2., 3.), DVec3::ZERO, &mut rng).unwrap();
        assert_eq!(p.is_mob_appearance(), Some(0.0));
        for _ in 0..15 {
            assert!(p.tick(&world));
        }
        assert!((p.is_mob_appearance().unwrap() - 0.5).abs() < 1e-5);
        for _ in 15..29 {
            assert!(p.tick(&world));
        }
        assert!(!p.tick(&world));
    }

    #[test]
    fn block_dust_and_breaking_poof_preserve_vanilla_velocities() {
        let mut rng = JavaRandom::new(77);
        assert!(Particle::block_dust(DVec3::ZERO, DVec3::ONE, 0, IVec3::ZERO, &mut rng).is_none());
        assert!(
            Particle::block_dust(DVec3::ZERO, DVec3::ONE, 166 << 4, IVec3::ZERO, &mut rng)
                .is_none()
        );
        let dust = Particle::block_dust(
            DVec3::new(1., 2., 3.),
            DVec3::new(0.25, -0.1, 0.5),
            1 << 4,
            IVec3::new(1, 2, 3),
            &mut rng,
        )
        .unwrap();
        assert_eq!(dust.velocity, DVec3::new(0.25, -0.1, 0.5));

        let poof = Particle::network_sprite(31, DVec3::ZERO, DVec3::ZERO, &mut rng).unwrap();
        assert_eq!(poof.item_texture(), Some((332, 0)));
        assert!(poof.velocity.length() > 0.05);
    }
}
