use super::Particle;
use crate::vanilla::{movement::Aabb, random::JavaRandom};
use crate::world::World;
use glam::DVec3;

#[derive(Clone, Debug)]
struct Emitter {
    target: u64,
    bounds: Aabb,
    magic: bool,
    remaining: u8,
    random: JavaRandom,
    particle_random: JavaRandom,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Emitters(Vec<Emitter>);

impl Emitter {
    fn batch(&mut self, particles: &mut Vec<Particle>, world: &World, viewer: DVec3, setting: u8) {
        let center = (self.bounds.min + self.bounds.max) * 0.5;
        let span = (self.bounds.max - self.bounds.min) * 0.25;
        for _ in 0..16 {
            let delta = DVec3::from_array(std::array::from_fn(|_| {
                f64::from(self.random.next_float() * 2. - 1.)
            }));
            let at = center + delta * span;
            if delta.length_squared() > 1. || at.distance_squared(viewer) > 256.
                || setting >= 2 || (setting == 1 && self.random.next_int(3) == 0)
                || particles.len() >= 4096
            {
                continue;
            }
            particles.push(Particle::critical(
                self.magic, at, delta + DVec3::Y * 0.2, &mut self.particle_random, world,
            ));
        }
        self.remaining -= 1;
    }
}

impl Emitters {
    #[cfg(test)]
    pub fn is_empty(&self) -> bool { self.0.is_empty() }

    pub fn start(
        &mut self, target: u64, bounds: Aabb, magic: bool, particles: &mut Vec<Particle>,
        world: &World, viewer: DVec3, setting: u8,
    ) {
        if setting >= 2 || self.0.len() >= 512 { return }
        let mut emitter = Emitter {
            target, bounds, magic, remaining: 3,
            random: JavaRandom::new(JavaRandom::independent_seed()),
            particle_random: JavaRandom::new(JavaRandom::independent_seed()),
        };
        emitter.batch(particles, world, viewer, setting);
        self.0.push(emitter);
    }

    pub fn tick(
        &mut self, particles: &mut Vec<Particle>, world: &World, viewer: DVec3, setting: u8,
        bounds: impl Fn(u64) -> Option<Aabb>,
    ) {
        self.0.retain_mut(|emitter| {
            if let Some(body) = bounds(emitter.target) {
                emitter.bounds = body;
            }
            emitter.batch(particles, world, viewer, setting);
            emitter.remaining > 0
        });
    }
}
