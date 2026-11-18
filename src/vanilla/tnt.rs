use super::java_map::{JavaKey, JavaMap};
use super::{
    movement::{self, collision, Aabb, MovementWorld},
    random::JavaRandom,
    ray_trace,
};
use glam::{DVec3, IVec3};
use serde::{Deserialize, Serialize};
include!("blast_resistance.rs");

const SIZE: f32 = 0.98;

impl JavaKey for IVec3 {
    fn hash_code(&self) -> i32 {
        self.z
            .wrapping_mul(31)
            .wrapping_add(self.y)
            .wrapping_mul(31)
            .wrapping_add(self.x)
    }
    fn compare(&self, other: &Self) -> std::cmp::Ordering {
        (self.y, self.z, self.x).cmp(&(other.y, other.z, other.x))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Primed {
    pub position: DVec3,
    pub previous: DVec3,
    pub velocity: DVec3,
    pub bounds: Aabb,
    pub fuse: i32,
    pub player_lit: bool,
    grounded: bool,
    in_web: bool,
}

impl Primed {
    pub fn new(p: DVec3, velocity: DVec3, fuse: i32, player_lit: bool) -> Self {
        Self {
            position: p,
            previous: p,
            velocity,
            bounds: super::entity_pick::body(p, SIZE, SIZE),
            fuse,
            player_lit,
            grounded: false,
            in_web: false,
        }
    }
    pub fn launch(math: &mut JavaRandom) -> DVec3 {
        let angle = f64::from((math.next_double() * f64::from(std::f32::consts::PI) * 2.) as f32);
        DVec3::new(
            f64::from(-(angle.sin() as f32) * 0.02),
            f64::from(0.2f32),
            f64::from(-(angle.cos() as f32) * 0.02),
        )
    }
    /// Returns the explosion origin on the tick that consumes fuse zero.
    pub fn tick(&mut self, world: &impl MovementWorld) -> Option<DVec3> {
        self.previous = self.position;
        self.velocity.y -= f64::from(0.04f32);
        let mut delta = self.velocity;
        if self.in_web {
            delta *= DVec3::new(0.25, f64::from(0.05f32), 0.25);
            self.velocity = DVec3::ZERO;
            self.in_web = false;
        }
        let step =
            collision::move_body_with_step(world, self.bounds, delta, self.grounded, false, 0.);
        self.bounds = step.bounds;
        self.position = self.bounds.feet();
        self.grounded = step.grounded;
        for (axis, clipped) in step.clipped.into_iter().enumerate() {
            if clipped {
                let under = (self.position - DVec3::Y * f64::from(0.2f32))
                    .floor()
                    .as_ivec3();
                if axis == 1 && world.get(under) >> 4 == 165 {
                    if self.velocity.y < 0. {
                        self.velocity.y = -self.velocity.y;
                    }
                } else {
                    self.velocity[axis] = 0.;
                }
            }
        }
        let min = (self.bounds.min + 0.001).floor().as_ivec3();
        let max = (self.bounds.max - 0.001).floor().as_ivec3();
        for x in min.x..=max.x {
            for y in min.y..=max.y {
                for z in min.z..=max.z {
                    match world.get(IVec3::new(x, y, z)) >> 4 {
                        30 => self.in_web = true,
                        88 => {
                            self.velocity.x *= 0.4;
                            self.velocity.z *= 0.4;
                        }
                        _ => {}
                    }
                }
            }
        }
        self.velocity *= f64::from(0.98f32);
        if self.grounded {
            self.velocity *= DVec3::new(f64::from(0.7f32), -0.5, f64::from(0.7f32));
        }
        self.fuse -= 1;
        if self.fuse < 0 {
            return Some(self.position + DVec3::Y * f64::from(SIZE / 16.));
        }
        if let (_, Some(current)) = movement::water_contact(world, self.bounds) {
            self.velocity += current * 0.014;
        }
        None
    }
}

/// Rays start on the surface of a 16³ cube. Both the step length and energy
/// loss use the original float precision even though positions are doubles.
pub fn footprint(
    world: &impl ray_trace::Access,
    center: DVec3,
    power: f32,
    random: &mut JavaRandom,
) -> Vec<IVec3> {
    let mut seen = JavaMap::default();
    for x in 0..16 {
        for y in 0..16 {
            for z in 0..16 {
                if [x, y, z].iter().all(|&n| n != 0 && n != 15) {
                    continue;
                }
                let direction =
                    DVec3::from_array([x, y, z].map(|n| f64::from(n as f32 / 15. * 2. - 1.)));
                let step = (direction / direction.length()) * f64::from(0.3f32);
                let mut at = center;
                let mut energy = power * (0.7 + random.next_float() * 0.6);
                while energy > 0. {
                    let p = at.floor().as_ivec3();
                    let id = usize::from(world.get(p) >> 4);
                    if id != 0 {
                        energy -= (RESISTANCE.get(id).copied().unwrap_or(0.) + 0.3) * 0.3;
                    }
                    if energy > 0. {
                        seen.insert(p, ());
                    }
                    at += step;
                    energy -= 0.22500001;
                }
            }
        }
    }
    // Block destruction, drops and chain fuses consume RNG in the world's HashSet traversal order.
    seen.entries().map(|(p, ())| *p).collect()
}

pub fn exposure(world: &impl ray_trace::Access, center: DVec3, body: Aabb) -> f32 {
    let size = body.max - body.min;
    let step = (size * 2. + 1.).recip();
    let mut offset = (1. - step.recip().floor() * step) * 0.5;
    offset.y = 0.;
    let options = ray_trace::Options {
        stop_on_liquid: false,
        ignore_without_box: false,
        return_last_miss: false,
    };
    let (mut clear, mut total) = (0, 0);
    let mut x = 0f32;
    while x <= 1. {
        let mut y = 0f32;
        while y <= 1. {
            let mut z = 0f32;
            while z <= 1. {
                let from = body.min + size * DVec3::new(x.into(), y.into(), z.into()) + offset;
                clear += i32::from(ray_trace::trace(world, from, center, options).is_none());
                total += 1;
                z = (f64::from(z) + step.z) as f32;
            }
            y = (f64::from(y) + step.y) as f32;
        }
        x = (f64::from(x) + step.x) as f32;
    }
    clear as f32 / total as f32
}

pub fn impact(
    world: &impl ray_trace::Access,
    center: DVec3,
    power: f32,
    body: Aabb,
    eye_height: f32,
) -> Option<(f32, DVec3)> {
    let radius = f64::from(power * 2.);
    let feet = body.feet();
    let distance = f64::from(feet.distance(center) as f32) / radius;
    if distance > 1. {
        return None;
    }
    let delta = feet + DVec3::Y * f64::from(eye_height) - center;
    let length = f64::from(delta.length() as f32);
    if length == 0. {
        return None;
    }
    let strength = (1. - distance) * f64::from(exposure(world, center, body));
    let damage = ((strength * strength + strength) * 0.5 * 8. * radius + 1.) as i32;
    Some((damage as f32, (delta / length) * strength))
}
