use super::{BlockTicks, FireEnvironment, JavaRandom, TickResult, World, NEIGHBORS};
use crate::vanilla::{
    runtime::fire::{self, FireEffects, FireView, FireWorld},
    spawning::solid_top,
};
use glam::IVec3;

struct Local<'a> {
    world: &'a mut World,
    ticks: &'a mut BlockTicks,
    random: JavaRandom,
    result: &'a mut TickResult,
    environment: FireEnvironment,
    end: bool,
}

impl FireView for Local<'_> {
    fn get(&self, p: IVec3) -> u16 {
        self.world.get(p)
    }
    fn solid_top(&self, p: IVec3) -> bool {
        solid_top(self.get(p))
    }
    fn set(&mut self, p: IVec3, state: u16, flags: u8) {
        let old = self.get(p);
        if old == state || !self.world.set(p, state) {
            return;
        }
        self.result.changed.push(p);
        fire::added(self, p, old, state);
        if flags & 1 != 0 {
            // Fire and portal callbacks are synchronous: changing the next fuel
            // block can remove the fire before the current spread call returns.
            for d in NEIGHBORS {
                let at = p + d;
                let neighbor = self.get(at);
                if matches!(neighbor >> 4, 51 | 90) {
                    fire::changed(self, at, neighbor);
                }
            }
            self.ticks.notify(self.world, p);
        }
    }
}
impl FireWorld for Local<'_> {
    fn end_dimension(&self) -> bool {
        self.end
    }
    fn start_fire_tick(&mut self, p: IVec3) {
        self.ticks
            .schedule_block_update(p, 51, 30 + self.random.next_int(10) as u64, 0);
    }
}
impl FireEffects for Local<'_> {
    fn fire_biome(&self, p: IVec3) -> u8 {
        crate::assets::biome_colors::biome_at(self.world, p)
    }
    fn fire_raining_at(&self, p: IVec3) -> bool {
        self.environment.raining && self.world.rain_exposed(p)
    }
    fn fire_prime_tnt(&mut self, p: IVec3) {
        self.result.primed_tnt.push(p);
        self.result
            .world_sounds
            .push(("random.fuse", p.as_dvec3() + glam::DVec3::new(0.5, 0., 0.5), 1., 1.));
    }
}

impl BlockTicks {
    pub(crate) fn set_fire_environment(&mut self, env: FireEnvironment, dimension: i32) {
        let mut scheduler = self.state();
        scheduler.fire_environment = env;
        scheduler.end_dimension = dimension == 1;
    }
    /// Existing Anvil chunks or legacy overlays can contain fire without a saved tick.
    /// Queue restoration and discovery must not spend a second World.rand draw.
    pub(super) fn discover_fire(&mut self, p: IVec3) {
        self.index();
        if !self.state().pending.contains(&(p.to_array(), 51)) {
            let mut random = self.world_random();
            self.schedule_block_update(p, 51, 30 + random.next_int(10) as u64, 0);
            self.set_world_random(&random);
        }
    }
    fn live_fire<'a>(
        &'a mut self,
        world: &'a mut World,
        random: &mut JavaRandom,
        result: &'a mut TickResult,
    ) -> Local<'a> {
        let (environment, end) = {
            let state = self.state();
            (state.fire_environment, state.end_dimension)
        };
        Local {
            ticks: self,
            world,
            random: random.share(),
            result,
            environment,
            end,
        }
    }
    pub(crate) fn update_local_fire(
        &mut self,
        world: &mut World,
        p: IVec3,
        state: u16,
        random: &mut JavaRandom,
        result: &mut TickResult,
    ) {
        let mut fire = self.live_fire(world, random, result);
        let environment = fire.environment;
        fire::update_in(&mut fire, p, state, environment, random);
    }
    pub(crate) fn lava_random_tick(
        &mut self, world: &mut World, p: IVec3, random: &mut JavaRandom, result: &mut TickResult,
    ) {
        use crate::vanilla::material::{blocks_movement, can_burn};
        let mut fire = self.live_fire(world, random, result);
        if !fire.environment.do_fire_tick {
            return;
        }
        let steps = random.next_int(3);
        if steps == 0 {
            for _ in 0..3 {
                let base = p + IVec3::new(random.next_int(3) - 1, 0, random.next_int(3) - 1);
                if fire.get(base + IVec3::Y) == 0 && can_burn(fire.get(base)) {
                    fire.set(base + IVec3::Y, 51 << 4, 3);
                }
            }
        } else {
            let mut at = p;
            for _ in 0..steps {
                at += IVec3::new(random.next_int(3) - 1, 1, random.next_int(3) - 1);
                let state = fire.get(at);
                if state == 0 {
                    if NEIGHBORS.iter().any(|&d| can_burn(fire.get(at + d))) {
                        fire.set(at, 51 << 4, 3);
                        return;
                    }
                } else if blocks_movement(state) {
                    return;
                }
            }
        }
    }
    pub(super) fn fire_neighbor(
        &mut self,
        world: &mut World,
        p: IVec3,
        state: u16,
        result: &mut TickResult,
    ) {
        let mut random = self.world_random();
        fire::changed(&mut self.live_fire(world, &mut random, result), p, state);
        self.set_world_random(&random);
    }
    pub(crate) fn ignite_air(&mut self, world: &mut World, p: IVec3, dimension: i32) -> TickResult {
        let mut result = TickResult::default();
        if world.get(p) == 0 {
            let mut random = self.world_random();
            let mut fire = self.live_fire(world, &mut random, &mut result);
            fire.end = dimension == 1;
            fire.set(p, 51 << 4, 3);
            self.set_world_random(&random);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        gameplay::{Mode, Simulation},
        inventory::Stack,
        player::Player,
        settings::Settings,
        world::Chunk,
    };
    use glam::{DVec3, Vec3};
    use std::sync::Arc;

    fn flat() -> World {
        let mut chunk = Chunk::empty();
        for z in 0..16 {
            for x in 0..16 {
                chunk.set(x, 0, z, 7 << 4);
            }
        }
        World {
            chunks: [((0, 0), Arc::new(chunk))].into(),
            spawn: Vec3::new(8.5, 1., 6.5),
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        }
    }

    #[test]
    fn stationary_lava_random_tick_ignites_fuel_then_fire_consumes_it() {
        let p = IVec3::new(8, 1, 8);
        for (fuel, enabled, expected) in [(17, true, true), (5, true, true), (35, true, true), (1, true, false), (17, false, false)] {
            let mut world = flat();
            let mut sim = Simulation::new(Mode::Survival, 189);
            let mut out = crate::gameplay::Outcome::default();
            sim.block_ticks.set_world_random(&JavaRandom::new(189));
            sim.block_ticks.set_fire_environment(FireEnvironment { do_fire_tick: enabled, ..Default::default() }, 0);
            world.set(p, 11 << 4);
            let wood = p + IVec3::X;
            world.set(wood, fuel << 4);
            for _ in 0..100 {
                sim.random_block_tick(&mut world, p, &mut out);
                if out.changed.iter().any(|&q| world.get(q) >> 4 == 51) { break; }
            }
            assert_eq!(out.changed.iter().any(|&q| world.get(q) >> 4 == 51), expected);
            for _ in 0..2000 {
                if world.get(wood) >> 4 != fuel || !expected { break; }
                sim.random_block_tick(&mut world, p, &mut out);
                sim.block_ticks.tick(&mut world);
            }
            assert_eq!(world.get(wood) >> 4 != fuel, expected);
        }
    }

    #[test]
    fn live_fire_primes_tnt_on_the_original_scheduled_ticks() {
        let data: serde_json::Value = serde_json::from_reader(flate2::read::GzDecoder::new(
            &include_bytes!("../tests/fixtures/vanilla189-fire-ticks.json.gz")[..],
        ))
        .unwrap();
        let mut cases = 0;
        for row in data["cases"].as_array().unwrap() {
            let frame = row["frames"].as_array().unwrap().last().unwrap();
            let expected = frame[1][3].as_array().unwrap();
            if expected.is_empty() {
                continue;
            }
            let mut w = flat();
            let mut ticks = BlockTicks::default();
            Arc::make_mut(w.chunks.get_mut(&(0, 0)).unwrap())
                .biomes
                .fill(row["biome"].as_u64().unwrap() as u8);
            ticks.set_world_random(&JavaRandom::new(1889 + row["index"].as_i64().unwrap()));
            ticks.set_fire_environment(
                FireEnvironment {
                    difficulty: row["difficulty"].as_u64().unwrap() as u8,
                    do_fire_tick: row["enabled"].as_bool().unwrap(),
                    raining: row["rain"].as_bool().unwrap(),
                },
                0,
            );
            for e in row["edits"].as_array().unwrap() {
                let [x, y, z, s]: [i32; 4] = serde_json::from_value(e.clone()).unwrap();
                let p = IVec3::new(x, y, z);
                if s >> 4 == 51 {
                    ticks.ignite_air(&mut w, p, 0);
                }
                w.set(p, s as u16);
            }
            let last = frame[0].as_i64().unwrap();
            for time in 1..=last {
                let effects = ticks.tick(&mut w);
                assert_eq!(
                    effects.primed_tnt.len(),
                    if time == last { expected.len() } else { 0 },
                    "Java fire scene {}, tick {time}",
                    row["index"]
                );
                for (p, body) in effects.primed_tnt.iter().zip(expected) {
                    let at = p.as_dvec3() + DVec3::new(0.5, 0., 0.5);
                    for (i, coord) in at.to_array().into_iter().enumerate() {
                        assert_eq!(
                            format!("{:x}", coord.to_bits()),
                            body[0][i].as_str().unwrap()
                        );
                    }
                }
            }
            cases += 1;
        }
        assert_eq!(cases, 15);
    }

    #[test]
    #[ignore = "uses installed 1.8.9 resources and normal singleplayer right-click"]
    fn right_clicking_beside_tnt_lights_fire_which_primes_and_explodes() {
        let resources = crate::validation::resources();
        for (item_id, enabled) in [(259, true), (385, true), (385, false)] {
            let mut w = flat();
            let fire = IVec3::new(8, 1, 9);
            let tnt = fire + IVec3::X;
            w.set(fire - IVec3::Y, 87 << 4);
            w.set(tnt, 46 << 4);
            let mut sim = Simulation::new(Mode::Survival, 189);
            sim.mob_spawning = false;
            sim.world_commands
                .rules
                .insert("randomTickSpeed".into(), "0".into());
            sim.world_commands
                .rules
                .insert("doFireTick".into(), enabled.to_string());
            sim.inventory.slots[0] = Some(Stack::new(item_id, 2));
            let mut player = Player::new(w.spawn);
            player.set_rotation_degrees(0., (1.62f32 / 3.).atan().to_degrees());
            let settings = Settings::default();
            sim.tick(&mut w, &mut player, resources, &settings, false, true);
            assert_eq!(w.get(fire) >> 4, 51);
            assert_eq!(w.get(tnt) >> 4, 46);
            assert!(sim.primed_tnt.is_empty());
            let held = sim.inventory.held().unwrap();
            if item_id == 259 {
                assert_eq!(held.damage, 1);
            } else {
                assert_eq!(held.count, 1);
            }
            let mut primed = false;
            let mut exploded = false;
            for _ in 0..1200 {
                let out = sim.tick(&mut w, &mut player, resources, &settings, false, false);
                primed |= !sim.primed_tnt.is_empty();
                exploded |= out
                    .world_sounds
                    .iter()
                    .any(|(clip, _, _, _)| *clip == "random.explode");
                if exploded {
                    break;
                }
            }
            assert_eq!((primed, exploded), (enabled, enabled));
        }
    }
}
