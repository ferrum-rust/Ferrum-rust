//! Opt-in original DEFAULT stage orchestration. Whole-world differential tests
//! must still cover global block callbacks and entity initialization before this
//! can replace the validated terrain-only default.
use super::*;
use crate::vanilla::{
    population::{self, decoration::Decorator},
    spawning::{self, SpawnArea, SpawnWorld},
};

pub struct VanillaPopulation;
impl Hooks for VanillaPopulation {
    fn supports_inert_ore_writes(&self) -> bool { true }
    fn supports_ore_coordinate_lookahead(&self) -> bool { true }
    fn populate(
        &self,
        access: &mut Access<'_>,
        pos: ChunkPos,
        random: &mut JavaRandom,
    ) -> Result<()> {
        access.with(|_, b| b.ticks.set_fall_instantly(true));
        let origin = IVec3::new(pos.0 * 16, 0, pos.1 * 16);
        let biome = FeatureWorld::biome(access, origin + IVec3::new(16, 0, 16));
        let seed = access.with(|_, b| b.seed);
        random.set_seed(seed);
        let a = (random.next_long() / 2).wrapping_mul(2).wrapping_add(1);
        let b = (random.next_long() / 2).wrapping_mul(2).wrapping_add(1);
        random.set_seed(
            (i64::from(pos.0)
                .wrapping_mul(a)
                .wrapping_add(i64::from(pos.1).wrapping_mul(b)))
                ^ seed,
        );
        let village = access.measured(Phase::Structures, |access| access.populate_structures(pos, random, false))?;
        access.measured(Phase::LakesDungeons, |access| population::lakes_and_dungeons(access, random, pos.0, pos.1, village));
        // Subclass prefixes execute before BiomeDecorator's reentry guard.
        // Their mutable plains/type fields live in the shared world adapter.
        access.measured(Phase::DecorationPrefix, |access| Decorator::default().decorate_prefix(access, random, origin, biome))?;
        let owner = Decorator::decorator_owner(biome);
        let mut decorator = access.with(|_, b| {
            ensure!(
                b.active_decorators.insert(owner),
                "Already decorating biome {owner}"
            );
            Ok(b.decorators.remove(&owner).unwrap_or_default())
        })?;
        access.measured(Phase::DecorationBody, |access| decorator.decorate_body_before_springs(access, random, origin, biome))?;
        if Decorator::generates_springs(biome) {
            access.measured(Phase::Springs, |access| {
            for fluid in [8, 10] {
                for _ in 0..if fluid == 8 { 50 } else { 20 } {
                    let x = random.next_int(16) + 8;
                    let z = random.next_int(16) + 8;
                    let bound = if fluid == 8 {
                        random.next_int(248) + 8
                    } else {
                        let inner = random.next_int(240) + 8;
                        random.next_int(inner) + 8
                    };
                    let y = random.next_int(bound);
                    let mut ticks = access.with(|_, b| b.ticks.share_scheduler());
                    let (_, mut effects) = ticks.generate_spring_access(
                        access,
                        origin + IVec3::new(x, y, z),
                        fluid,
                        random,
                    );
                    access.with(|_, b| {
                        b.block_effects.changed.append(&mut effects.changed);
                        b.block_effects.drops.append(&mut effects.drops);
                        b.block_effects.washed.append(&mut effects.washed);
                    });
                }
            }
            });
        }
        access.with(|_, b| {
            b.active_decorators.remove(&owner);
            b.decorators.insert(owner, decorator);
        });
        // Subclass suffixes run after currentWorld is cleared.
        access.measured(Phase::DecorationSuffix, |access| Decorator::default().decorate_after_springs(access, random, origin, biome))?;
        let mut world_random = access.world_random();
        let animals = access.measured(Phase::Animals, |access| spawning::perform_worldgen_spawning(
            access,
            biome,
            SpawnArea {
                x: origin.x + 8,
                z: origin.z + 8,
                width: 16,
                depth: 16,
            },
            random,
            &mut world_random,
        ))?;
        access.with(|_, b| b.animals.extend(animals));
        access.measured(Phase::IceSnow, |access| population::ice_and_snow(access, origin));
        // Intentionally not a nesting counter: inner vanilla populate clears
        // this static flag, affecting the suspended outer population.
        access.with(|_, b| b.ticks.set_fall_instantly(false));
        Ok(())
    }
    fn repopulate(&self, access: &mut Access<'_>, pos: ChunkPos) -> Result<bool> {
        if !access
            .with(|p, b| b.map_features && p.record(pos).is_some_and(|c| c.inhabited_time < 3600))
        {
            return Ok(false);
        }
        let mut random = access.with(|_, b| b.provider_random.share());
        access.populate_structures(pos, &mut random, true)
    }
}
impl Access<'_> {
    fn populate_structures(
        &mut self,
        pos: ChunkPos,
        random: &mut JavaRandom,
        monument_only: bool,
    ) -> Result<bool> {
        self.with(|provider, b| {
            if !b.map_features {
                return Ok(false);
            }
            for x in pos.0..=pos.0 + 1 {
                for z in pos.1..=pos.1 + 1 {
                    ensure!(
                        provider.contains((x, z)),
                        "Missing structure population neighbor {x},{z}"
                    );
                }
            }
            let changed_start = b.output.changed.len();
            let mut world = structures::LoadedStructures {
                world: &mut b.world,
                record_overlay_edits: b.record_overlay_edits,
                light: &mut b.light,
                random: &mut b.world_random,
                ticks: b.ticks.share_scheduler(),
                output: &mut b.structure_entities,
                live_containers: &mut b.live_containers,
                tile_revision: &mut b.tile_revision,
                live_spawners: &mut b.live_spawners,
                falling: &mut b.falling,
                entities: &mut b.entities,
                hooks: b.hooks.clone(),
                changes: &mut b.output,
                precipitation: &mut b.precipitation,
                drops: &mut b.drops,
                drop_tiles: &mut b.drop_tiles,
                do_tile_drops: b.do_tile_drops,
            };
            let present = if monument_only {
                b.structures
                    .populate_monuments_with(&mut world, pos, random)
            } else {
                b.structures.populate_with(&mut world, pos, random)
            };
            for p in &b.output.changed[changed_start..] {
                b.dirty_chunks.insert(cp(*p));
                provider.mark_block_modified(cp(*p));
            }
            Ok(present)
        })
    }
}
impl SpawnWorld for Access<'_> {
    fn get(&mut self, p: IVec3) -> Result<u16> {
        self.get_block(p)
    }
    fn top_solid_or_liquid(&mut self, x: i32, z: i32) -> Result<IVec3> {
        self.provide((x.div_euclid(16), z.div_euclid(16)))?;
        Ok(IVec3::new(x, FeatureWorld::top_solid(self, x, z), z))
    }
    fn inside_border(&self, p: IVec3) -> bool {
        (-29_999_984..29_999_984).contains(&p.x) && (-29_999_984..29_999_984).contains(&p.z)
    }
}
