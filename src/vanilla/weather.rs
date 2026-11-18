//! Overworld weather timers, strength transitions and client-bound S2B updates.
use super::random::JavaRandom;
use crate::nbt::Tag;
use serde::{Deserialize, Serialize};

pub(crate) fn precipitation_height(world: &crate::world::World, x: i32, z: i32) -> i32 {
    let Some(chunk) = world.chunks.get(&(x >> 4, z >> 4)) else {
        return -1;
    };
    (1..chunk.top.min(256))
        .rev()
        .find(|&y| {
            let state = chunk.get(x & 15, y, z & 15);
            super::material::blocks_movement(state) || super::material::is_liquid(state)
        })
        .map_or(-1, |y| y + 1)
}

pub(crate) fn snow_at(id: u8, position: glam::IVec3) -> bool {
    super::population::temperature(id, position) < 0.15
}

pub(crate) fn block_light(world: &crate::world::World, p: glam::IVec3) -> u8 {
    world
        .chunks
        .get(&(p.x >> 4, p.z >> 4))
        .and_then(|chunk| chunk.light.as_ref())
        .map_or(0, |light| light.get(super::lighting::Kind::Block, p))
}

/// WorldServer's selected weather column: freezing is independent of rainfall.
pub(crate) fn update_column(
    world: &mut crate::world::World,
    x: i32,
    z: i32,
    raining: bool,
    random: &mut JavaRandom,
    changed: &mut Vec<glam::IVec3>,
) {
    use super::{
        lighting::Kind,
        population::{FeatureWorld, WorldRegion},
    };
    let position = glam::IVec3::new(x, precipitation_height(world, x, z), z);
    let mut region = WorldRegion::new(world, position, position, |world, p, sky| {
        world
            .chunks
            .get(&(p.x >> 4, p.z >> 4))
            .and_then(|chunk| chunk.light.as_ref())
            .map_or(if sky { 15 } else { 0 }, |light| {
                light.get(if sky { Kind::Sky } else { Kind::Block }, p)
            })
    })
    .expect("weather only visits loaded columns");
    let below = position - glam::IVec3::Y;
    if region.can_freeze(below) {
        region.set(below, 79 << 4);
    }
    if raining && region.can_snow(position) {
        region.set(position, 78 << 4);
    }
    if raining && super::runtime::biome_can_rain(region.biome(below)) {
        let state = region.get(below);
        if state >> 4 == 118 && random.next_int(20) == 1 && state & 3 < 3 {
            region.set(below, state + 1);
        }
    }
    changed.extend(region.result.changed);
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
struct WeatherCycle {
    ticks: i32,
    active: bool,
    previous: f32,
    current: f32,
}
impl WeatherCycle {
    fn tick(&mut self, random: &mut JavaRandom, active_minimum: i32) {
        if self.ticks <= 0 {
            self.ticks = if self.active {
                random.next_int(12000) + active_minimum
            } else {
                random.next_int(168000) + 12000
            };
        } else {
            self.ticks -= 1;
            if self.ticks == 0 {
                self.active = !self.active;
            }
        }
        self.previous = self.current;
        let step = if self.active { 0.01 } else { -0.01 };
        self.current = ((f64::from(self.current) + step) as f32).clamp(0., 1.);
    }
    fn sample(self, partial: f32) -> f32 {
        self.previous + (self.current - self.previous) * partial
    }
    fn set_strength(&mut self, value: f32) {
        self.previous = value;
        self.current = value;
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Weather {
    clear_ticks: i32,
    rain: WeatherCycle,
    thunder: WeatherCycle,
}
impl Weather {
    pub fn from_level(data: &Tag) -> Self {
        let flag = |name| crate::player_save::nbt_boolean(data.get(name));
        let ticks = |name| crate::player_save::nbt_integer(data.get(name)).unwrap_or(0);
        let raining = flag("raining");
        let thundering = flag("thundering");
        Self {
            clear_ticks: ticks("clearWeatherTime"),
            rain: WeatherCycle {
                ticks: ticks("rainTime"),
                active: raining,
                current: f32::from(raining),
                ..Default::default()
            },
            thunder: WeatherCycle {
                ticks: ticks("thunderTime"),
                active: thundering,
                current: f32::from(raining && thundering),
                ..Default::default()
            },
        }
    }
    pub fn tick(&mut self, random: &mut JavaRandom) {
        if self.clear_ticks > 0 {
            self.clear_ticks -= 1;
            for cycle in [&mut self.rain, &mut self.thunder] {
                cycle.ticks = if cycle.active { 1 } else { 2 };
            }
        }
        self.thunder.tick(random, 3600);
        self.rain.tick(random, 12000);
    }
    pub fn command(&mut self, rain: bool, thunder: bool, ticks: i32) {
        self.clear_ticks = if rain { 0 } else { ticks };
        self.rain.active = rain;
        self.thunder.active = thunder;
        self.rain.ticks = if rain { ticks } else { 0 };
        self.thunder.ticks = self.rain.ticks;
    }
    pub fn toggle_rain(&mut self) {
        self.rain.active = !self.rain.active;
    }
    pub fn clear_after_sleep(&mut self) {
        for cycle in [&mut self.rain, &mut self.thunder] {
            cycle.ticks = 0;
            cycle.active = false;
        }
    }
    /// Thunder before multiplication by rain. Lightmap and sleep take both
    /// independent strengths, while World.getThunderStrength multiplies them.
    pub fn strengths(self, partial: f32) -> (f32, f32) {
        (self.rain.sample(partial), self.thunder.sample(partial))
    }
    pub fn game_state(&mut self, reason: u8, value: f32) {
        match reason {
            1 | 2 => {
                self.rain.active = reason == 1;
                self.rain.set_strength(if reason == 1 { 0. } else { 1. });
            }
            7 => self.rain.set_strength(value),
            8 => self.thunder.set_strength(value),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_weather_column_freezes_sources_and_accumulates_snow_without_covering_plants() {
        use crate::world::{Chunk, World};
        use glam::{IVec3, Vec3};
        use std::sync::Arc;
        let mut chunk = Chunk::empty();
        chunk.biomes.fill(12);
        for x in 0..16 {
            for z in 0..16 {
                chunk.set(x, 63, z, 2 << 4);
            }
        }
        let mut world = World {
            chunks: [((0, 0), Arc::new(chunk))].into(),
            spawn: Vec3::ZERO,
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        };
        let mut random = JavaRandom::new(189);
        let mut changed = Vec::new();
        update_column(&mut world, 1, 1, false, &mut random, &mut changed);
        assert!(changed.is_empty());
        update_column(&mut world, 1, 1, true, &mut random, &mut changed);
        assert_eq!(world.get(IVec3::new(1, 64, 1)), 78 << 4);
        world.set(IVec3::new(2, 63, 2), 9 << 4);
        update_column(&mut world, 2, 2, false, &mut random, &mut changed);
        assert_eq!(
            world.get(IVec3::new(2, 63, 2)),
            79 << 4,
            "freezing must not require rain"
        );
        world.set(IVec3::new(3, 64, 3), (31 << 4) | 1);
        update_column(&mut world, 3, 3, true, &mut random, &mut changed);
        assert_eq!(world.get(IVec3::new(3, 64, 3)), (31 << 4) | 1);
        world.set(IVec3::new(4, 63, 4), (9 << 4) | 1);
        update_column(&mut world, 4, 4, true, &mut random, &mut changed);
        assert_eq!(
            world.get(IVec3::new(4, 63, 4)),
            (9 << 4) | 1,
            "flowing water must not freeze"
        );
        assert!(!snow_at(3, IVec3::new(0, 64, 0)));
        assert!(
            snow_at(3, IVec3::new(0, 160, 0)),
            "mountain altitude must turn rain into snow"
        );
    }
    #[test]
    fn original_world_weather_timers_rng_and_strengths_match() {
        type Case = (i64, i32, i32, i32, bool, bool, u32);
        let cases: Vec<Case> =
            serde_json::from_str(include_str!("../../tests/fixtures/vanilla189-weather.json"))
                .unwrap();
        assert_eq!(cases.len(), 64);
        for (seed, clear, rain, thunder, raining, thundering, expected) in cases {
            let data = Tag::Compound(
                [
                    ("clearWeatherTime", Tag::Int(clear)),
                    ("rainTime", Tag::Int(rain)),
                    ("thunderTime", Tag::Int(thunder)),
                    ("raining", Tag::Byte(i8::from(raining))),
                    ("thundering", Tag::Byte(i8::from(thundering))),
                ]
                .into_iter()
                .map(|(key, tag)| (key.to_owned(), tag))
                .collect(),
            );
            let mut world = Weather::from_level(&data);
            let mut random = JavaRandom::new(seed);
            let mut crc = flate2::Crc::new();
            for tick in 0..=256 {
                if tick > 0 {
                    world.tick(&mut random);
                }
                if tick == 51 {
                    world = serde_json::from_slice(&serde_json::to_vec(&world).unwrap()).unwrap();
                    random = JavaRandom::from_state(random.state());
                }
                let state = random.state();
                for n in [
                    world.clear_ticks as u32,
                    world.rain.ticks as u32,
                    world.thunder.ticks as u32,
                    u32::from(world.rain.active),
                    u32::from(world.thunder.active),
                    state as u32,
                    (state >> 32) as u32,
                ] {
                    crc.update(&n.to_le_bytes());
                }
                for partial in [0., 0.5, 1.] {
                    let (r, t) = world.strengths(partial);
                    crc.update(&r.to_le_bytes());
                    crc.update(&(t * r).to_le_bytes());
                }
            }
            assert_eq!(crc.sum(), expected, "World.updateWeather, seed {seed}");
        }
    }
    #[test]
    fn client_weather_packets_replace_both_strength_samples() {
        let mut w = Weather::default();
        for (reason, value, rain, thunder) in [
            (1, 0., 0., 0.),
            (7, 0.6, 0.6, 0.),
            (8, 0.5, 0.6, 0.5),
            (2, 0., 1., 0.5),
        ] {
            w.game_state(reason, value);
            for partial in [0., 0.5, 1.] {
                assert_eq!(w.strengths(partial), (rain, thunder));
            }
        }
    }
}
