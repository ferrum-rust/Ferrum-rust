use super::{drops_local::LocalDrops, JavaRandom, Mode, Outcome, Simulation, World};
use crate::vanilla::{
    atmosphere,
    plant_events::{self, PlantWorld},
    population::vegetation::BigMushroom,
};
use glam::{IVec3, Vec3};

const HORIZONTAL: [IVec3; 4] = [IVec3::NEG_Z, IVec3::X, IVec3::Z, IVec3::NEG_X];

/// Block.getTickRandomly() in the 1.8.9 registry, including blocks whose callback is empty.
pub(crate) fn ticks_randomly(id: u16) -> bool {
    matches!(id, 2 | 6 | 8 | 10 | 11 | 18 | 28 | 31 | 32 | 37..=40 | 50 | 51 |
        59 | 60 | 70 | 72 | 74..=81 | 83 | 86 | 90..=92 | 104..=106 | 110 |
        111 | 115 | 127 | 131 | 132 | 141..=143 | 147 | 148 | 161 | 171 | 175)
}

fn next_position(lcg: &mut i32) -> IVec3 {
    *lcg = lcg.wrapping_mul(3).wrapping_add(1_013_904_223);
    let bits = *lcg >> 2;
    IVec3::new(bits & 15, (bits >> 16) & 15, (bits >> 8) & 15)
}

struct Farm<'a> {
    world: &'a mut World,
    sim: &'a mut Simulation,
    out: &'a mut Outcome,
    difficulty: u8,
}

impl PlantWorld for Farm<'_> {
    fn get(&self, p: IVec3) -> u16 {
        self.world.get(p)
    }
    fn light(&self, p: IVec3) -> u8 {
        self.world.game_light(p, 0, false, self.sim.dimension == 0)
    }
    fn can_see_sky(&self, p: IVec3) -> bool {
        self.world.can_see_sky(p)
    }
    fn set(&mut self, p: IVec3, state: u16, _flags: u8) {
        if self.get(p) != state && self.world.set(p, state) {
            self.out.changed.push(p);
        }
    }
    fn drop_block(&mut self, p: IVec3, state: u16) {
        self.sim.drop_farm_block(self.world, p, state, 0);
    }
    fn normal_cube_or(&self, p: IVec3, fallback: bool) -> bool {
        if !(0..256).contains(&p.y)
            || !(-30_000_000..30_000_000).contains(&p.x)
            || !(-30_000_000..30_000_000).contains(&p.z)
            || !self.world.contains(p.as_vec3())
        {
            return fallback;
        }
        let state = self.get(p);
        crate::vanilla::material::opaque(state) && crate::vanilla::material::full_cube(state)
    }
}

impl Farm<'_> {
    fn uproot(&mut self, p: IVec3, state: u16, random: &mut JavaRandom) {
        LocalDrops::new(self.sim, self.world).farm(random, p, state, 0);
        self.set(p, 0, 3);
    }

    fn growth_light(&self, p: IVec3) -> u8 {
        let (rain, thunder) = self.sim.weather.strengths(1.);
        let subtract = atmosphere::skylight_subtracted(self.sim.clock as i64, rain, thunder * rain);
        self.world.game_light(p, subtract, true, self.sim.dimension == 0)
    }

    fn rain_reaches(&self, p: IVec3) -> bool {
        self.sim.dimension == 0
            && self.sim.weather.strengths(1.).0 > 0.2
            && self.world.rain_exposed(p)
    }

    fn farmland(&mut self, p: IVec3, state: u16) {
        let watered = (-4..=4).any(|x| {
            (-4..=4)
                .any(|z| (0..=1).any(|y| matches!(self.get(p + IVec3::new(x, y, z)) >> 4, 8 | 9)))
        });
        if watered || self.rain_reaches(p + IVec3::Y) {
            self.set(p, 60 << 4 | 7, 2);
        } else if state & 15 > 0 {
            self.set(p, state - 1, 2);
        } else if !matches!(self.get(p + IVec3::Y) >> 4, 59 | 104 | 105 | 141 | 142) {
            self.set(p, 3 << 4, 3);
        }
    }

    fn mushroom_spread(&mut self, p: IVec3, state: u16, random: &mut JavaRandom) {
        if random.next_int(25) != 0 {
            return;
        }
        let mut remaining = 5;
        for z in -4..=4 {
            for y in -1..=1 {
                for x in -4..=4 {
                    if self.get(p + IVec3::new(x, y, z)) >> 4 == state >> 4 {
                        remaining -= 1;
                        if remaining == 0 {
                            return;
                        }
                    }
                }
            }
        }
        let mut center = p;
        let mut candidate = p;
        for attempt in 0..5 {
            candidate = center
                + IVec3::new(
                    random.next_int(3) - 1,
                    random.next_int(2) - random.next_int(2),
                    random.next_int(3) - 1,
                );
            if attempt < 4
                && self.get(candidate) == 0
                && plant_events::can_stay(self, candidate, state)
            {
                center = candidate;
            }
        }
        if self.get(candidate) == 0 && plant_events::can_stay(self, candidate, state) {
            self.set(candidate, state, 2);
        }
    }

    fn crop(&mut self, p: IVec3, state: u16, random: &mut JavaRandom) {
        let id = state >> 4;
        let stem = id == 104 || id == 105;
        if !plant_events::can_stay(self, p, state) {
            self.uproot(p, self.get(p), random);
        }
        if self.growth_light(p + IVec3::Y) < 9 || (!stem && state & 15 >= 7) {
            return;
        }
        let bound = (25. / crop_growth_chance(self.world, p, id)) as i32 + 1;
        if random.next_int(bound) != 0 {
            return;
        }
        if state & 15 < 7 {
            self.set(p, state + 1, 2);
        } else {
            let fruit = if id == 104 { 86 } else { 103 };
            if HORIZONTAL.iter().any(|d| self.get(p + *d) >> 4 == fruit) {
                return;
            }
            let at = p + HORIZONTAL[random.next_int(4) as usize];
            if self.get(at) == 0 && matches!(self.get(at - IVec3::Y) >> 4, 2 | 3 | 60) {
                // BlockPumpkin's default FACING is NORTH (legacy metadata 2).
                self.set(at, if fruit == 86 { 86 << 4 | 2 } else { 103 << 4 }, 3);
            }
        }
    }

    fn column_plant(&mut self, p: IVec3, state: u16, random: &mut JavaRandom) {
        let id = state >> 4;
        if id == 83 && !plant_events::can_stay(self, p, state) {
            self.uproot(p, self.get(p), random);
            return;
        }
        if self.get(p + IVec3::Y) != 0 {
            return;
        }
        if self.get(p - IVec3::Y) >> 4 == id && self.get(p - IVec3::Y * 2) >> 4 == id {
            return;
        }
        if state & 15 == 15 {
            self.set(p + IVec3::Y, id << 4, 3);
            self.set(p, id << 4, 4);
            if id == 81 && !plant_events::can_stay(self, p + IVec3::Y, id << 4) {
                self.uproot(p + IVec3::Y, id << 4, random);
            }
        } else {
            self.set(p, state + 1, 4);
        }
    }

    fn tick(&mut self, p: IVec3, state: u16, random: &mut JavaRandom) {
        match state >> 4 {
            11 | 51 => {
                let mut effects = crate::block_ticks::TickResult::default();
                if state >> 4 == 11 {
                    self.sim.block_ticks.lava_random_tick(self.world, p, random, &mut effects);
                } else {
                    self.sim.block_ticks.update_local_fire(self.world, p, state, random, &mut effects);
                }
                self.sim.fire_outcome(effects, self.out);
            }
            39 | 40 => self.mushroom_spread(p, state, random),
            59 | 104 | 105 | 141 | 142 => self.crop(p, state, random),
            60 => self.farmland(p, state),
            74 => self.set(p, 73 << 4, 3),
            78..=80 => {
                let ice = state >> 4 == 79;
                let threshold = if ice { 11 - crate::vanilla::material::light_opacity(state) } else { 11 };
                if crate::vanilla::weather::block_light(self.world, p) > threshold {
                    if ice && self.sim.dimension == -1 {
                        self.set(p, 0, 3);
                    } else {
                        LocalDrops::new(self.sim, self.world).farm(random, p, state, 0);
                        self.set(p, if ice { 9 << 4 } else { 0 }, 3);
                    }
                }
            }
            81 | 83 => self.column_plant(p, state, random),
            90 => {
                if self.sim.dimension == 0
                    && self.sim.mob_spawning
                    && self.difficulty > 0
                    && random.next_int(2000) < i32::from(self.difficulty)
                {
                    let mut support = p;
                    while !crate::vanilla::spawning::solid_top(self.get(support)) && support.y > 0 {
                        support -= IVec3::Y;
                    }
                    if p.y > 0
                        && !crate::vanilla::material::normal_cube(self.get(support + IVec3::Y))
                    {
                        let yaw = super::unit(&mut self.sim.rng) * std::f32::consts::TAU;
                        let mut group = None;
                        self.sim.spawn_registered_group(
                            super::MobKind::PigZombie,
                            Vec3::new(
                                support.x as f32 + 0.5,
                                (support.y + 1) as f32,
                                support.z as f32,
                            ),
                            yaw,
                            &mut group,
                        );
                    }
                }
            }
            115 => {
                if state & 15 < 3 && random.next_int(10) == 0 {
                    self.set(p, state + 1, 2);
                }
                if !plant_events::can_stay(self, p, state) {
                    self.uproot(p, self.get(p), random);
                }
            }
            127 => {
                if !plant_events::can_stay(self, p, state) {
                    self.uproot(p, self.get(p), random);
                } else if random.next_int(5) == 0 && state & 15 < 8 {
                    self.set(p, state + 4, 2);
                }
            }
            _ => {}
        }
    }

    fn bonemeal(&mut self, p: IVec3, state: u16, random: &mut JavaRandom) -> bool {
        let id = state >> 4;
        match id {
            59 | 104 | 105 | 141 | 142 if state & 15 < 7 => {
                self.set(
                    p,
                    id << 4 | ((state & 15) + 2 + random.next_int(4) as u16).min(7),
                    2,
                );
            }
            127 if state & 15 < 8 => self.set(p, state + 4, 2),
            39 | 40 => {
                if f64::from(random.next_float()) < 0.4 {
                    self.set(p, 0, 3);
                    let mut big = BigMushroom {
                        kind: Some(if id == 39 { 99 } else { 100 }),
                    };
                    if !big.generate_blocks(
                        self,
                        random,
                        p,
                        |farm, at| {
                            if farm.world.contains(at.as_vec3()) {
                                farm.get(at)
                            } else {
                                7 << 4
                            }
                        },
                        |farm, at, state| farm.set(at, state, 3),
                    ) {
                        self.set(p, state, 3);
                    }
                }
            }
            _ => return false,
        }
        true
    }
}

pub(super) fn farm_drop(state: u16) -> bool {
    matches!(
        state >> 4,
        6 | 27 | 28 | 37..=40 | 50 | 55 | 59 | 63 | 65 | 66 | 68 | 69 | 75..=77
            | 81 | 83 | 93 | 94 | 99 | 100 | 103..=105 | 111 | 115 | 127 | 131
            | 141..=143 | 149 | 150 | 157 | 171
    ) || (state >> 4 == 175 && state & 8 == 0 && !matches!(state & 7, 2 | 3))
}

impl Simulation {
    pub(super) fn drop_farm_block(&mut self, world: &World, p: IVec3, state: u16, fortune: i32) {
        let mut random = self.block_ticks.world_random();
        LocalDrops::new(self, world).farm(&mut random, p, state, fortune);
        self.block_ticks.set_world_random(&random);
    }

    pub(super) fn check_plant_at(&mut self, world: &mut World, p: IVec3, out: &mut Outcome) {
        if !(0..256).contains(&p.y) || !world.contains(p.as_vec3()) {
            return;
        }
        let state = world.get(p);
        if !plant_events::supports(state >> 4) {
            return;
        }
        let start = out.changed.len();
        {
            let difficulty = if self.hardcore { 3 } else { self.world_difficulty.unwrap_or(2).min(3) };
            let mut farm = Farm {
                sim: self,
                world,
                out,
                difficulty,
            };
            plant_events::neighbor_changed(&mut farm, p, state);
        }
        for i in start..out.changed.len() {
            let q = out.changed[i];
            self.block_ticks.notify(world, q);
        }
    }

    pub(super) fn check_farm_support(
        &mut self,
        world: &mut World,
        source: IVec3,
        out: &mut Outcome,
    ) {
        let start = out.changed.len();
        {
            let difficulty = if self.hardcore { 3 } else { self.world_difficulty.unwrap_or(2).min(3) };
            let mut farm = Farm {
                sim: self,
                world,
                out,
                difficulty,
            };
            for dir in [
                IVec3::ZERO,
                IVec3::NEG_X,
                IVec3::X,
                IVec3::NEG_Y,
                IVec3::Y,
                IVec3::NEG_Z,
                IVec3::Z,
            ] {
                let p = source + dir;
                if !(0..256).contains(&p.y) || !farm.world.contains(p.as_vec3()) {
                    continue;
                }
                let state = farm.get(p);
                if plant_events::supports(state >> 4) {
                    plant_events::neighbor_changed(&mut farm, p, state);
                }
            }
        }
        for i in start..out.changed.len() {
            let q = out.changed[i];
            self.block_ticks.notify(world, q);
        }
    }

    pub(super) fn apply_farm_bonemeal(
        &mut self,
        world: &mut World,
        p: IVec3,
        out: &mut Outcome,
    ) -> bool {
        let state = world.get(p);
        let mut random = self.block_ticks.world_random();
        let difficulty = if self.hardcore { 3 } else { self.world_difficulty.unwrap_or(2).min(3) };
        if !(Farm {
            sim: self,
            world,
            out,
            difficulty,
        })
        .bonemeal(p, state, &mut random)
        {
            return false;
        }
        self.block_ticks.set_world_random(&random);
        if self.mode == Mode::Survival {
            self.inventory.take_selected(1);
        }
        self.place_ticks = 4;
        let height = crate::vanilla::ray_trace::outline_bounds(world, p, world.get(p))
            .max
            .y
            - f64::from(p.y);
        let mut fx =
            crate::vanilla::spawning::EntityRandom::from_state(self.particle_random[0], None);
        let mut motion = JavaRandom::from_state(self.particle_random[2]);
        for _ in 0..15 {
            let velocity =
                glam::DVec3::from_array(std::array::from_fn(|_| fx.next_gaussian() * 0.02));
            let at = glam::DVec3::new(
                f64::from(p.x as f32 + fx.random.next_float()),
                f64::from(p.y) + f64::from(fx.random.next_float()) * height,
                f64::from(p.z as f32 + fx.random.next_float()),
            );
            if let Some(part) = super::Particle::network_sprite(21, at, velocity, &mut motion) {
                self.particles.push(part);
            }
        }
        self.particle_random[0] = fx.random.state();
        self.particle_random[2] = motion.state();
        true
    }

    #[cfg(test)]
    pub(crate) fn random_block_tick(&mut self, world: &mut World, p: IVec3, out: &mut Outcome) {
        let state = world.get(p);
        let mut random = self.block_ticks.world_random();
        let difficulty = if self.hardcore { 3 } else { self.world_difficulty.unwrap_or(2).min(3) };
        Farm {
            sim: self,
            world,
            out,
            difficulty,
        }
        .tick(p, state, &mut random);
        self.block_ticks.set_world_random(&random);
    }

    pub(super) fn random_block_ticks(
        &mut self,
        world: &mut World,
        player: Vec3,
        distance: u32,
        difficulty: u8,
        out: &mut Outcome,
    ) {
        let attempts = self
            .world_commands
            .rules
            .get("randomTickSpeed")
            .map_or(3, |v| {
                v.parse::<i32>()
                    .unwrap_or_else(|_| i32::from(v.eq_ignore_ascii_case("true")))
            });
        let center: IVec3 = player.floor().as_ivec3() >> 4;
        let mut active: Vec<_> = world
            .chunks
            .iter()
            .filter(|((x, z), _)| {
                (x - center.x).abs() <= distance as i32 && (z - center.z).abs() <= distance as i32
            })
            .map(|(&pos, c)| (pos, c.random_tick_mask()))
            .collect();
        active.sort_unstable_by_key(|(pos, _)| *pos);
        let mut random = self.block_ticks.world_random();
        let (rain, thunder) = self.weather.strengths(1.);
        for ((x, z), mask) in active {
            // Weather probes also advance the coordinate LCG. This matters for fixed arrays of sections.
            if random.next_int(100_000) == 0 && rain * thunder > 0.9 {
                next_position(&mut self.random_tick_lcg);
            }
            if random.next_int(16) == 0 {
                let column = next_position(&mut self.random_tick_lcg);
                if self.dimension == 0 {
                    crate::vanilla::weather::update_column(world, x * 16 + column.x,
                        z * 16 + column.z, rain > 0.2, &mut random, &mut out.changed);
                }
            }
            for section in 0..16 {
                if mask & (1 << section) == 0 {
                    continue;
                }
                for _ in 0..attempts {
                    let p = IVec3::new(x * 16, section * 16, z * 16)
                        + next_position(&mut self.random_tick_lcg);
                    let state = world.get(p);
                    if ticks_randomly(state >> 4) {
                        Farm {
                            sim: self,
                            world,
                            out,
                            difficulty,
                        }
                        .tick(p, state, &mut random);
                    }
                }
            }
        }
        self.block_ticks.set_world_random(&random);
    }
}

fn crop_growth_chance(world: &World, pos: IVec3, crop: u16) -> f32 {
    let mut chance = 1.0;
    for dx in -1..=1 {
        for dz in -1..=1 {
            let soil = world.get(pos + IVec3::new(dx, -1, dz));
            if soil >> 4 == 60 {
                let fertility = if soil & 15 > 0 { 3.0 } else { 1.0 };
                chance += fertility / if dx == 0 && dz == 0 { 1.0 } else { 4.0 };
            }
        }
    }
    let same = |dx, dz| world.get(pos + IVec3::new(dx, 0, dz)) >> 4 == crop;
    if ((same(-1, 0) || same(1, 0)) && (same(0, -1) || same(0, 1)))
        || same(-1, -1)
        || same(-1, 1)
        || same(1, -1)
        || same(1, 1)
    {
        chance *= 0.5;
    }
    chance
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vanilla::lighting::ChunkLight;
    use crate::world::Chunk;
    use serde::Deserialize;
    use std::{collections::HashMap, sync::Arc};

    #[derive(Deserialize)]
    struct Case {
        initial: Vec<[i32; 4]>,
        light: u8,
        growth_light: u8,
        rain: bool,
        seed: i64,
        meal: bool,
        accepted: bool,
        writes: Vec<[i32; 5]>,
        next: String,
    }

    fn empty() -> World {
        World {
            chunks: [((0, 0), Arc::new(Chunk::empty()))].into(),
            edits: Default::default(),
            spawn: Vec3::ZERO,
            label: String::new(),
            save_path: Default::default(),
            imported: false,
            light_revision: 0,
        }
    }

    #[test]
    fn prewarmed_section_counts_preserve_growth_weather_random_and_writes() {
        let mut cold = empty();
        let chunk = Arc::make_mut(cold.chunks.get_mut(&(0, 0)).unwrap());
        chunk.biomes.fill(12);
        for x in 0..16 { for z in 0..16 {
            chunk.set(x, 63, z, if x < 8 { 60 << 4 | 7 } else { 2 << 4 });
            if x < 8 { chunk.set(x, 64, z, 59 << 4); }
        } }
        let mut warm = cold.clone();
        // Make a separate cache so cold initialization is exercised too.
        Arc::make_mut(warm.chunks.get_mut(&(0, 0)).unwrap()).prepare_tick_indexes();
        assert!(cold.chunks[&(0, 0)].random_tick_sections.get().is_none());
        let mut original = Simulation { random_tick_lcg: 189, ..Default::default() };
        original.weather.game_state(7, 1.);
        original.block_ticks.set_world_random(&JavaRandom::new(189));
        original.world_commands.rules.insert("randomTickSpeed".into(), "64".into());
        let mut cached = original.clone();
        let mut changes = 0;
        for _ in 0..64 {
            let mut a = Outcome::default();
            let mut b = Outcome::default();
            original.random_block_ticks(&mut cold, Vec3::new(8., 65., 8.), 16, 2, &mut a);
            cached.random_block_ticks(&mut warm, Vec3::new(8., 65., 8.), 16, 2, &mut b);
            assert_eq!(a.changed, b.changed);
            assert_eq!(original.random_tick_lcg, cached.random_tick_lcg);
            assert_eq!(original.block_ticks.world_random().state(), cached.block_ticks.world_random().state());
            assert_eq!(cold.chunks[&(0, 0)].blocks, warm.chunks[&(0, 0)].blocks);
            changes += a.changed.len();
        }
        assert!(changes > 0, "the equivalence run must exercise actual block writes");
    }

    #[test]
    fn original_random_growth_and_bonemeal_callbacks_match() {
        let (ids, cases): (Vec<u16>, Vec<Case>) =
            serde_json::from_reader(flate2::read::GzDecoder::new(
                &include_bytes!("../tests/fixtures/vanilla189-crop-growth.json.gz")[..],
            ))
            .unwrap();
        assert_eq!(
            ids,
            (0..=197)
                .filter(|&id| ticks_randomly(id))
                .collect::<Vec<_>>()
        );
        assert_eq!(cases.len(), 2134);
        let at = IVec3::new(8, 64, 8);
        for case in cases {
            let mut world = empty();
            let chunk = Arc::make_mut(world.chunks.get_mut(&(0, 0)).unwrap());
            for &[x, y, z, s] in &case.initial {
                chunk.set(x, y, z, s as u16);
            }
            let mut light = ChunkLight {
                values: vec![case.light; 65536],
                height: vec![0; 256],
                sections: [true; 16],
                no_sky: false,
            };
            light.values[Chunk::index(8, 65, 8)] = case.growth_light;
            chunk.light = Some(Arc::new(light));
            let mut sim = Simulation {
                clock: 6000,
                ..Simulation::default()
            };
            if case.rain {
                sim.weather.game_state(7, 1.);
            }
            sim.world_commands
                .rules
                .insert("doTileDrops".into(), "false".into());
            sim.block_ticks
                .set_world_random(&JavaRandom::new(case.seed));
            let state = world.get(at);
            let mut out = Outcome::default();
            if case.meal {
                sim.inventory.slots[0] = Some(crate::inventory::Stack::new(351, 2));
                sim.inventory.slots[0].as_mut().unwrap().meta = 15;
                let item = sim.inventory.held().unwrap();
                let accepted =
                    sim.use_item_on_block(&mut world, item, (at, at + IVec3::Y), &mut out);
                assert_eq!(accepted, case.accepted);
                assert_eq!(
                    sim.inventory.held().unwrap().count,
                    2 - u8::from(case.accepted)
                );
            } else {
                sim.random_block_tick(&mut world, at, &mut out);
            }
            let expected_positions: Vec<_> = case
                .writes
                .iter()
                .map(|&[x, y, z, _, _]| IVec3::new(x, y, z))
                .collect();
            assert_eq!(
                out.changed, expected_positions,
                "state {state}, seed {}, meal {}",
                case.seed, case.meal
            );
            let mut expected: HashMap<_, _> = case
                .initial
                .into_iter()
                .map(|[x, y, z, s]| (IVec3::new(x, y, z), s))
                .collect();
            for [x, y, z, s, _] in case.writes {
                expected.insert(IVec3::new(x, y, z), s);
            }
            for (pos, raw) in expected {
                assert_eq!(
                    world.get(pos),
                    raw as u16,
                    "state {state}, seed {}, at {pos}",
                    case.seed
                );
            }
            assert_eq!(
                sim.block_ticks.world_random().next_long().to_string(),
                case.next,
                "state {state}, seed {}",
                case.seed
            );
        }
    }

    #[test]
    fn snow_and_ice_melt_at_original_block_light_boundaries() {
        let p = IVec3::new(8,64,8);
        for state in [78 << 4, (78 << 4) | 7, 79 << 4, 80 << 4] {
            for light in 0..=15 {
                for dimension in [0,-1] {
                    let mut world=empty();
                    let chunk=Arc::make_mut(world.chunks.get_mut(&(0,0)).unwrap());
                    chunk.set(8,64,8,state);
                    let mut values=vec![255;65536];
                    values[Chunk::index(8,64,8)]=240|light;
                    chunk.light=Some(Arc::new(ChunkLight {values,height:vec![0;256],sections:[true;16],no_sky:false}));
                    let mut sim=Simulation {dimension,..Default::default()};
                    let mut out=Outcome::default();
                    let mut random=JavaRandom::new(189);
                    Farm {world:&mut world,sim:&mut sim,out:&mut out,difficulty:2}.tick(p,state,&mut random);
                    let melted=light>if state>>4==79 {8} else {11};
                    let expected=if melted {if state>>4==79&&dimension==0 {9<<4} else {0}} else {state};
                    assert_eq!(world.get(p),expected,"state {state}, raw light {light}, dimension {dimension}");
                    assert_eq!(out.changed.contains(&p),melted);
                    let drops:u32=sim.drops.iter().map(|d|u32::from(d.stack.count)).sum();
                    assert_eq!(drops,if melted&&state>>4==80 {4}else{0});
                    assert!(sim.drops.iter().all(|d|d.stack.id==332));
                }
            }
        }
    }

    #[test]
    fn falling_snow_still_accumulates_when_random_block_ticks_are_disabled() {
        let mut world = empty();
        let chunk = Arc::make_mut(world.chunks.get_mut(&(0, 0)).unwrap());
        chunk.biomes.fill(12);
        for x in 0..16 { for z in 0..16 { chunk.set(x, 63, z, 2 << 4); } }
        let mut sim = Simulation { random_tick_lcg: 189, ..Default::default() };
        sim.weather.game_state(7, 1.);
        sim.block_ticks.set_world_random(&JavaRandom::new(189));
        sim.world_commands.rules.insert("randomTickSpeed".into(), "0".into());
        let mut out = Outcome::default();
        for _ in 0..1024 { sim.random_block_ticks(&mut world, Vec3::new(8.,64.,8.), 2, 2, &mut out); }
        let covered = (0..16).flat_map(|x|(0..16).map(move |z|(x,z)))
            .filter(|&(x,z)|world.get(IVec3::new(x,64,z)) >> 4 == 78).count();
        assert!(covered > 30, "weather must place snow with randomTickSpeed=0, got {covered}");
        assert!(out.changed.iter().any(|p|p.y == 64));
    }

    #[test]
    fn planted_seeds_and_random_tick_speed_work_in_loaded_world() {
        let mut w = empty();
        let empty_section = IVec3::new(8, 95, 8);
        Arc::make_mut(w.chunks.get_mut(&(0, 0)).unwrap()).light = Some(Arc::new(ChunkLight {
            values: vec![0; 65536], height: vec![128; 256], sections: [false; 16], no_sky: false,
        }));
        assert_eq!(w.game_light(empty_section, 0, false, false), 0);
        assert_eq!(w.game_light(empty_section, 6, true, true), 9);
        Arc::make_mut(w.chunks.get_mut(&(0, 0)).unwrap()).light = None;
        let mut sim = Simulation {
            clock: 6000,
            random_tick_lcg: 189,
            ..Simulation::default()
        };
        let mut out = Outcome::default();
        let soil = IVec3::new(8, 63, 8);
        for dx in -1..=1 {
            for dz in -1..=1 {
                w.set(soil + IVec3::new(dx, 0, dz), 60 << 4 | 7);
            }
        }
        w.set(soil + IVec3::X * 2, 9 << 4);
        for (seed, stem, fruit) in [(361, 104, 86), (362, 105, 103)] {
            sim.inventory.slots[0] = Some(crate::inventory::Stack::new(seed, 1));
            let item = sim.inventory.held().unwrap();
            assert!(sim.use_item_on_block(&mut w, item, (soil, soil + IVec3::Y), &mut out));
            let p = soil + IVec3::Y;
            assert_eq!(w.get(p), stem << 4);
            sim.world_commands
                .rules
                .insert("randomTickSpeed".into(), "0".into());
            sim.random_block_ticks(&mut w, p.as_vec3(), 8, 2, &mut out);
            assert_eq!(w.get(p), stem << 4);
            sim.world_commands
                .rules
                .insert("randomTickSpeed".into(), "4096".into());
            for _ in 0..256 {
                sim.random_block_ticks(&mut w, p.as_vec3(), 8, 2, &mut out);
                if HORIZONTAL.iter().any(|d| w.get(p + *d) >> 4 == fruit) {
                    break;
                }
            }
            assert_eq!(w.get(p), stem << 4 | 7);
            let fruit_pos = HORIZONTAL
                .iter()
                .map(|d| p + *d)
                .find(|p| w.get(*p) >> 4 == fruit)
                .expect("mature stem bears fruit");
            sim.destroy_picked_block(&mut w, fruit_pos, &Default::default(), &mut out);
            assert_eq!(w.get(fruit_pos), 0);
            let item_id = if fruit == 86 { 86 } else { 360 };
            assert!(sim.drops.iter().any(|drop| drop.stack.id == item_id));
            w.set(p, 0);
            // The fruit may have covered bare tilled soil long enough to turn it back to dirt.
            for dx in -1..=1 {
                for dz in -1..=1 {
                    w.set(soil + IVec3::new(dx, 0, dz), 60 << 4 | 7);
                }
            }
        }
    }
}
