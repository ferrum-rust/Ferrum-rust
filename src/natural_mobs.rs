use super::{random, unit, MobKind, Simulation};
use crate::vanilla::{
    atmosphere,
    entity_pick::body,
    material,
    movement::collision,
    random::JavaRandom,
    spawning::{solid_top, AnimalKind},
};
use crate::world::World;
use glam::{IVec3, Vec3};
use std::{collections::BTreeMap, sync::LazyLock};

// monster, creature, ambient, waterCreature in the installed biome registry
type BiomeLists = BTreeMap<u8, [Vec<[u8; 4]>; 4]>;
static BIOMES: LazyLock<BiomeLists> =
    LazyLock::new(|| serde_json::from_str(include_str!("vanilla/mob_biomes189.json")).unwrap());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distant_and_persistent_mobs_do_not_fill_the_loaded_spawn_cap() {
        let world = super::super::tests::flat_world();
        let mut sim = Simulation::new(super::super::Mode::Survival, 189);
        let id = sim.spawn_registered(MobKind::Zombie, world.spawn, 0.);
        assert_eq!(sim.natural_count(&world, Group::Monster), 1);
        sim.command_entities.mobs.get_mut(&id).unwrap().persistent = true;
        assert_eq!(sim.natural_count(&world, Group::Monster), 0);
        sim.command_entities.mobs.get_mut(&id).unwrap().persistent = false;
        sim.mobs[0].position.x += 1024.;
        assert_eq!(sim.natural_count(&world, Group::Monster), 0);
        sim.mobs[0].position = world.spawn;
        assert_eq!(sim.natural_count(&world, Group::Monster), 1);
    }

    #[test]
    fn biome_tables_preserve_all_original_passive_entries_and_special_pools() {
        assert_eq!(BIOMES.len(), 61);
        for (&biome, groups) in BIOMES.iter() {
            let (_, original) = crate::vanilla::spawning::spawn_list(biome).unwrap();
            let expected: Vec<_> = original
                .iter()
                .map(|entry| {
                    [
                        MobKind::from(entry.kind).vanilla_id(),
                        entry.weight as u8,
                        entry.min as u8,
                        entry.max as u8,
                    ]
                })
                .collect();
            assert_eq!(groups[Group::Creature as usize], expected, "biome {biome}");
            for [id, weight, min, max] in groups.iter().flatten() {
                assert!(MobKind::from_id(*id).is_some() && *weight > 0 && *min > 0 && min <= max);
            }
        }
        assert_eq!(BIOMES[&9][Group::Monster as usize], [[58, 10, 4, 4]]);
        assert_eq!(BIOMES[&14][Group::Creature as usize], [[96, 8, 4, 8]]);
        assert!(BIOMES[&21][Group::Monster as usize].contains(&[98, 2, 1, 1]));
    }

    #[test]
    fn overworld_hostiles_respect_raw_skylight_and_sample_underground_caves_uniformly() {
        let mut world = super::super::tests::flat_world();
        for x in -6..=6 {
            for z in -6..=6 {
                let mut chunk = crate::world::Chunk::empty();
                for dx in 0..16 {
                    for dz in 0..16 {
                        chunk.set(dx, 0, dz, 7 << 4);
                        for y in [10, 20, 30, 40, 50] {
                            chunk.set(dx, y, dz, 1 << 4);
                        }
                        chunk.set(dx, 63, dz, 2 << 4);
                    }
                }
                world.chunks.insert((x, z), std::sync::Arc::new(chunk));
            }
        }
        crate::lighting_view::LightingView::new(&mut world);
        let mut sim = Simulation::new(super::super::Mode::Survival, 189);
        let surface_pos = IVec3::new(40, 64, 0);
        let cave_pos = IVec3::new(40, 11, 0);
        assert_eq!(world.light_uv(surface_pos, 0)[1], 15 * 16);
        assert_eq!(world.light_uv(cave_pos, 0)[1], 0);

        // Noon clear sky (sky_loss = 0): open surface is rejected 100% of the time,
        // while dark cave (raw_sky = 0, light = 0) always passes.
        for _ in 0..64 {
            assert!(!sim.habitat(&world, surface_pos, MobKind::Zombie, 1, 0, false));
            assert!(sim.habitat(&world, cave_pos, MobKind::Zombie, 1, 0, false));
        }

        // Midnight (sky_loss = 11 -> effective light = 4) still checks raw skylight (15)
        // against random % 32 before checking effective light <= random % 8, so surface
        // spawns fail roughly half the time instead of passing whenever 4 <= random % 8.
        let mut night_surface_passes = 0;
        for _ in 0..512 {
            if sim.habitat(&world, surface_pos, MobKind::Zombie, 1, 11, false) {
                night_surface_passes += 1;
            }
        }
        assert!(
            (40..220).contains(&night_surface_passes),
            "unexpected night surface pass count {night_surface_passes}"
        );

        // Daytime thunderstorm uses skylightSubtracted = 10 -> effective light = 5,
        // allowing occasional thunderstorm spawns when raw_sky <= random % 32.
        let mut thunder_passes = 0;
        for _ in 0..512 {
            if sim.habitat(&world, surface_pos, MobKind::Zombie, 1, 3, true) {
                thunder_passes += 1;
            }
        }
        assert!(thunder_passes > 0);

        // Full natural_spawns in daylight Overworld spawns zero surface monsters
        // while spawning underground cave monsters and surface passive creatures.
        let mut daylight = Simulation::new(super::super::Mode::Survival, 189);
        daylight.clock = 6000;
        for _ in 0..16 {
            daylight.natural_spawns(&world, Vec3::new(0.5, 64., 0.5), 2);
        }
        let hostiles: Vec<_> = daylight.mobs.iter().filter(|m| m.kind.hostile()).collect();
        let creatures: Vec<_> = daylight
            .mobs
            .iter()
            .filter(|m| Group::Creature.contains(m.kind))
            .collect();
        assert!(!hostiles.is_empty());
        assert!(hostiles.iter().all(|m| m.position.y < 60.));
        assert!(!creatures.is_empty());
        assert!(creatures.iter().all(|m| m.position.y >= 64.));
    }
}
#[derive(Clone, Copy, PartialEq)]
enum Group {
    Monster,
    Creature,
    Ambient,
    Water,
}
impl From<AnimalKind> for MobKind {
    fn from(a: AnimalKind) -> Self {
        match a {
            AnimalKind::Cow => Self::Cow,
            AnimalKind::Chicken => Self::Chicken,
            AnimalKind::Sheep => Self::Sheep,
            AnimalKind::Pig => Self::Pig,
            AnimalKind::Horse => Self::Horse,
            AnimalKind::Wolf => Self::Wolf,
            AnimalKind::Rabbit => Self::Rabbit,
            AnimalKind::Mooshroom => Self::Mooshroom,
        }
    }
}
impl Group {
    fn contains(self, kind: MobKind) -> bool {
        match self {
            Self::Ambient => kind == MobKind::Bat,
            Self::Water => kind == MobKind::Squid,
            Self::Monster => kind.hostile(),
            Self::Creature => matches!(
                kind,
                MobKind::Cow
                    | MobKind::Sheep
                    | MobKind::Pig
                    | MobKind::Chicken
                    | MobKind::Horse
                    | MobKind::Mooshroom
                    | MobKind::Rabbit
                    | MobKind::Ocelot
                    | MobKind::Wolf
            ),
        }
    }
}
fn weighted(list: &[[u8; 4]], rng: &mut u64) -> Option<(MobKind, usize)> {
    let total = list.iter().map(|e| u64::from(e[1])).sum::<u64>();
    if total == 0 {
        return None;
    }
    let mut pick = random(rng) % total;
    for &[id, weight, min, max] in list {
        if pick < u64::from(weight) {
            let count = usize::from(min) + (random(rng) % u64::from(max - min + 1)) as usize;
            return Some((MobKind::from_id(id)?, count));
        }
        pick -= u64::from(weight);
    }
    None
}
pub(super) fn slime_chunk(p: IVec3, seed: i64) -> bool {
    let (x, z) = (p.x >> 4, p.z >> 4);
    let mut s = seed;
    for term in [
        i64::from(x.wrapping_mul(x).wrapping_mul(4987142)),
        i64::from(x.wrapping_mul(5947611)),
        i64::from(z.wrapping_mul(z)) * 4392871,
        i64::from(z.wrapping_mul(389711)),
    ] {
        s = s.wrapping_add(term);
    }
    JavaRandom::new(s ^ 987234911).next_int(10) == 0
}
impl Simulation {
    pub fn spawn_initial(&mut self, world: &World, player: Vec3) {
        if !self.mob_spawning || !self.mobs.is_empty() {
            return;
        }
        for _ in 0..8 {
            self.spawn_pack(world, player, Group::Creature, 10);
        }
    }
    pub(super) fn natural_spawns(&mut self, world: &World, player: Vec3, difficulty: u8) {
        if !self.mob_spawning
            || self.commands.player_mode == Some(3)
            || !self.total_ticks.is_multiple_of(80)
        {
            return;
        }
        let chunks = world.chunks.len().min(289);
        if difficulty > 0 {
            for _ in 0..3 {
                self.spawn_pack(world, player, Group::Monster, (chunks * 70 / 289).max(4));
            }
        }
        for (group, cap) in [
            (Group::Creature, 10),
            (Group::Ambient, 15),
            (Group::Water, 5),
        ] {
            self.spawn_pack(world, player, group, (chunks * cap / 289).max(1));
        }
    }
    fn spawn_pack(&mut self, world: &World, player: Vec3, group: Group, cap: usize) {
        let room = cap.saturating_sub(self.natural_count(world, group));
        if room == 0 {
            return;
        }
        let (rain, thunder) = self.weather.strengths(1.);
        let sky_loss = atmosphere::skylight_subtracted(self.clock as i64, rain, rain * thunder);
        let thundering = f64::from(rain * thunder) > 0.9;
        for _ in 0..24 {
            let angle = unit(&mut self.rng) * std::f32::consts::TAU;
            let radius = 24. + unit(&mut self.rng) * 64.;
            let x = (player.x + angle.sin() * radius).floor() as i32;
            let z = (player.z + angle.cos() * radius).floor() as i32;
            if !world.chunks.contains_key(&(x >> 4, z >> 4)) {
                continue;
            }
            let at = IVec3::new(x, 0, z);
            let biome = match self.dimension {
                -1 => 8,
                1 => 9,
                _ => crate::assets::biome_colors::biome_at(world, at),
            };
            let y = match group {
                Group::Water => 46 + (random(&mut self.rng) % 17) as i32,
                Group::Ambient => (random(&mut self.rng) % 63) as i32,
                Group::Monster => {
                    let chunk = &world.chunks[&(x >> 4, z >> 4)];
                    let col_height = chunk
                        .light
                        .as_ref()
                        .and_then(|l| {
                            l.height
                                .get(((z.rem_euclid(16)) * 16 + x.rem_euclid(16)) as usize)
                                .copied()
                        })
                        .filter(|&h| h > 0)
                        .unwrap_or(chunk.top);
                    let top_y = ((col_height.max(0) + 16) / 16 * 16).clamp(16, 256);
                    (random(&mut self.rng) % top_y as u64) as i32
                }
                _ => {
                    let Some(y) = super::surface(world, x, z) else {
                        continue;
                    };
                    y
                }
            };
            let pack_pos = at.with_y(y);
            const FORTRESS_MONSTERS: [[u8; 4]; 4] = [
                [61, 10, 2, 3],
                [57, 5, 4, 4],
                [51, 10, 4, 4],
                [62, 3, 4, 4],
            ];
            let seed = self.world_commands.imported_seed.unwrap_or(self.spawn_seed);
            let pool: &[[u8; 4]] = if self.dimension == -1
                && group == Group::Monster
                && crate::dimension_world::fortress_monster_spawn(world, seed, pack_pos)
            {
                &FORTRESS_MONSTERS
            } else {
                let Some(lists) = BIOMES.get(&biome) else {
                    continue;
                };
                &lists[group as usize]
            };
            let Some((kind, count)) = weighted(pool, &mut self.rng) else {
                continue;
            };
            let mut p = pack_pos;
            let mut spawned = 0;
            let mut animal_group = None;
            for attempt in 0..count.min(room) * 4 {
                if attempt != 0 {
                    p.x = x + (random(&mut self.rng) % 11) as i32 - 5;
                    p.z = z + (random(&mut self.rng) % 11) as i32 - 5;
                }
                let pos = p.as_vec3() + Vec3::new(0.5, 0., 0.5);
                if pos.distance_squared(player) < 576.
                    || pos.distance_squared(world.spawn) < 576.
                    || !world.contains(pos)
                    || !self.habitat(world, p, kind, biome, sky_loss, thundering)
                {
                    continue;
                }
                let (w, h) = kind.size();
                let bounds = body(pos.as_dvec3(), w, h);
                if collision::colliders(world, bounds)
                    .iter()
                    .any(|b| bounds.intersects(*b))
                    || self.mobs.iter().any(|m| {
                        let (width, height) = self.mob_size(m);
                        bounds.intersects(body(m.position.as_dvec3(), width, height))
                    })
                {
                    continue;
                }
                let yaw = unit(&mut self.rng) * std::f32::consts::TAU;
                self.spawn_registered_group(kind, pos, yaw, &mut animal_group);
                spawned += 1;
                if spawned == count.min(room) {
                    break;
                }
            }
            if spawned > 0 {
                return;
            }
        }
    }
    fn natural_count(&self, world: &World, group: Group) -> usize {
        self.mobs
            .iter()
            .filter(|mob| {
                group.contains(mob.kind)
                    && world.contains(mob.position)
                    && !self
                        .command_entities
                        .mobs
                        .get(&mob.id)
                        .is_some_and(|data| data.persistent)
            })
            .count()
    }
    fn habitat(
        &mut self,
        world: &World,
        p: IVec3,
        kind: MobKind,
        biome: u8,
        sky_loss: u8,
        thundering: bool,
    ) -> bool {
        let state = world.get(p);
        let floor = world.get(p - IVec3::Y);
        if kind == MobKind::Squid {
            return matches!(state >> 4, 8 | 9) && matches!(floor >> 4, 8 | 9);
        }
        if material::normal_cube(state) || material::is_liquid(state) {
            return false;
        }
        let light = world.game_light(p, sky_loss, true, self.dimension == 0);
        if kind == MobKind::Bat {
            return p.y < 63 && light <= (random(&mut self.rng) % 4) as u8;
        }
        if !solid_top(floor) || matches!(floor >> 4, 7 | 166) {
            return false;
        }
        match kind {
            MobKind::Slime => {
                let moon =
                    [1., 0.75, 0.5, 0.25, 0., 0.25, 0.5, 0.75][(self.clock / 24000 % 8) as usize];
                (p.y < 40
                    && slime_chunk(
                        p,
                        self.world_commands.imported_seed.unwrap_or(self.spawn_seed),
                    )
                    && random(&mut self.rng) % 10 == 0)
                    || (biome == 6
                        && (51..70).contains(&p.y)
                        && unit(&mut self.rng) < 0.5
                        && unit(&mut self.rng) < moon
                        && light <= (random(&mut self.rng) % 8) as u8)
            }
            MobKind::Mooshroom => floor >> 4 == 110 && light > 8,
            MobKind::Ocelot => {
                p.y >= 63 && matches!(floor >> 4, 2 | 18 | 161) && random(&mut self.rng) % 3 != 0
            }
            MobKind::Ghast => random(&mut self.rng) % 20 == 0,
            MobKind::PigZombie | MobKind::MagmaCube | MobKind::Blaze => true,
            _ if kind.hostile() => {
                if self.dimension == 0 {
                    let raw_sky = world.light_uv(p, 0)[1] / 16;
                    if raw_sky > (random(&mut self.rng) % 32) as u8 {
                        return false;
                    }
                }
                let monster_light = if thundering {
                    world.game_light(p, 10, true, self.dimension == 0)
                } else {
                    light
                };
                monster_light <= (random(&mut self.rng) % 8) as u8
            }
            _ => floor >> 4 == 2 && light > 8,
        }
    }
}
