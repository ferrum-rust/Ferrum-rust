//! Independent pre-change general-setter oracle. This deliberately duplicates
//! the original fill and pipeline so changing the optimized writer cannot change
//! the reference's metadata semantics or density traversal.
use super::*;
use crate::{vanilla::lighting::{LightChunk, Lighting, PreparedLight}, world::{ChunkPos, World}};
use std::sync::Arc;

fn empty_world() -> World {
    World { chunks: Default::default(), spawn: Default::default(), label: String::new(),
        save_path: Default::default(), edits: Default::default(), imported: false, light_revision: 0 }
}

fn assert_unobserved_metadata(chunk: &Chunk) {
    // Exhaustive fields keep this proof tied to Chunk::empty/set's complete state.
    let Chunk { blocks, biomes: _, top: _, random_tick_sections, scheduled_tick_blocks,
        partial_light_sections, skulls, signs,
        containers, hoppers, entities, pistons, tiles, light, visual_light, light_edits } = chunk;
    assert!(blocks.is_dense());
    assert!(random_tick_sections.get().is_none());
    assert!(scheduled_tick_blocks.get().is_none());
    assert!(partial_light_sections.get().is_none());
    assert!(skulls.get().is_none());
    assert!(signs.get().is_none());
    assert!(containers.is_empty());
    assert!(hoppers.is_empty());
    assert!(entities.is_empty());
    assert!(pistons.is_empty());
    assert!(tiles.is_empty());
    assert!(light.is_none());
    assert!(visual_light.is_none());
    assert!(light_edits.is_empty());
}

fn assert_chunk(actual: &Chunk, expected: &Chunk) {
    assert_eq!(actual.blocks.as_slice(), expected.blocks.as_slice());
    assert_eq!(actual.biomes, expected.biomes);
    assert_eq!(actual.top, expected.top);
    assert_unobserved_metadata(actual);
    assert_unobserved_metadata(expected);
    // Lazy derivative contents must agree too, without mutating either input.
    let a = actual.clone();
    let b = expected.clone();
    assert_eq!(a.random_tick_mask(), b.random_tick_mask());
    assert_eq!(a.scheduled_tick_candidates(), b.scheduled_tick_candidates());
}

fn assert_light(a: &LightChunk, b: &LightChunk) {
    let LightChunk { height, minimum, sections, sky, block, gaps, queued_checks } = a;
    assert_eq!(height, &b.height);
    assert_eq!(*minimum, b.minimum);
    assert_eq!(sections, &b.sections);
    assert_eq!(sky.as_slice(), b.sky.as_slice());
    assert_eq!(block.as_slice(), b.block.as_slice());
    assert_eq!(gaps, &b.gaps);
    assert_eq!(*queued_checks, b.queued_checks);
}

fn assert_prepared_light(actual: &Chunk, expected: &Chunk, pos: ChunkPos) {
    for no_sky in [false, true] {
        let mut a_world = empty_world();
        let mut b_world = empty_world();
        let mut a = Lighting::new(&a_world, no_sky);
        let mut b = Lighting::new(&b_world, no_sky);
        b.use_reference_path(true);
        let prepared = PreparedLight::new(actual, no_sky);
        a_world.chunks.insert(pos, Arc::new(actual.clone()));
        b_world.chunks.insert(pos, Arc::new(expected.clone()));
        assert!(a.add_prepared_chunk(&a_world, pos, prepared));
        b.add_chunk(&b_world, pos);
        assert_light(&a.chunks[&pos], &b.chunks[&pos]);
        assert_eq!(a.take_render_dirty(), b.take_render_dirty());
        let a = a.render_snapshot(pos).unwrap();
        let b = b.render_snapshot(pos).unwrap();
        assert_eq!(a.values, b.values);
        assert_eq!(a.height, b.height);
        assert_eq!(a.sections, b.sections);
        assert_eq!(a.no_sky, b.no_sky);
    }
}

fn cases() -> [(i64, ChunkPos, i32, bool, bool); 6] {
    [
        (189, (0, 0), 63, false, false),
        (8924706420471469774, (-7, 11), 63, false, false),
        (-1789, (13, -9), 63, true, false),
        (189, (-1, -1), 0, false, true),
        (8924706420471469774, (2, 3), 256, true, false),
        (19, (-11, 7), 300, false, true),
    ]
}

#[test]
fn fresh_base_fill_matches_general_setter_all_states_top_metadata_and_prepared_light() {
    for (seed, (x, z), sea, lava, amplified) in cases() {
        let mut terrain = Terrain::new(seed);
        terrain.settings.sea_level = sea;
        terrain.settings.use_lava_oceans = lava;
        terrain.amplified = amplified;
        let initial_random = terrain.initial_random().state();
        let generator = BiomeGenerator::new(seed);
        let biomes = generator.get_biomes_for_generation(x * 4 - 2, z * 4 - 2, 10, 10);
        let actual = terrain.base_chunk(x, z, &biomes);
        let expected = original_base_chunk(&terrain, x, z, &biomes);
        assert_chunk(&actual, &expected);
        assert_prepared_light(&actual, &expected, (x, z));
        assert_eq!(terrain.initial_random().state(), initial_random);
    }
}

#[test]
fn fresh_base_fill_preserves_surface_carvers_provider_rng_and_generated_light() {
    for (seed, (x, z), sea, lava, amplified) in cases() {
        let mut terrain = Terrain::new(seed);
        terrain.settings.sea_level = sea;
        terrain.settings.use_lava_oceans = lava;
        terrain.amplified = amplified;
        let generator = BiomeGenerator::new(seed);
        let mut actual_rng = JavaRandom::new(-434);
        let mut expected_rng = JavaRandom::new(-434);
        let actual = terrain.generate_with_random(x, z, &generator, &mut actual_rng).unwrap();
        let expected = original_generate_with_random(&terrain, x, z, &generator, &mut expected_rng).unwrap();
        assert_chunk(&actual, &expected);
        assert_eq!(actual_rng.state(), expected_rng.state());
        assert_prepared_light(&actual, &expected, (x, z));
    }
}

fn original_base_chunk(terrain: &Terrain, chunk_x: i32, chunk_z: i32, biomes: &[u8]) -> Chunk {
    let density = terrain.density(chunk_x, chunk_z, biomes);
    let mut chunk = Chunk::empty();
    for x in 0..4 {
        for z in 0..4 {
            for y in 0..32 {
                let i00 = (x * 5 + z) * 33 + y;
                let i01 = (x * 5 + z + 1) * 33 + y;
                let i10 = ((x + 1) * 5 + z) * 33 + y;
                let i11 = ((x + 1) * 5 + z + 1) * 33 + y;
                let mut a = density[i00];
                let mut b = density[i01];
                let mut c = density[i10];
                let mut d = density[i11];
                let da = (density[i00 + 1] - a) * 0.125;
                let db = (density[i01 + 1] - b) * 0.125;
                let dc = (density[i10 + 1] - c) * 0.125;
                let dd = (density[i11 + 1] - d) * 0.125;
                for sy in 0..8 {
                    let mut left = a;
                    let mut right = b;
                    let dx = (c - a) * 0.25;
                    let dx_right = (d - b) * 0.25;
                    for sx in 0..4 {
                        let dz = (right - left) * 0.25;
                        let mut value = left - dz;
                        for sz in 0..4 {
                            value += dz;
                            let by = y * 8 + sy;
                            if value > 0. {
                                chunk.set(
                                    (x * 4 + sx) as i32,
                                    by as i32,
                                    (z * 4 + sz) as i32,
                                    1 << 4,
                                );
                            } else if (by as i32) < terrain.settings.sea_level {
                                chunk.set(
                                    (x * 4 + sx) as i32,
                                    by as i32,
                                    (z * 4 + sz) as i32,
                                    if terrain.settings.use_lava_oceans {
                                        11 << 4
                                    } else {
                                        9 << 4
                                    },
                                );
                            }
                        }
                        left += dx;
                        right += dx_right;
                    }
                    a += da;
                    b += db;
                    c += dc;
                    d += dd;
                }
            }
        }
    }
    chunk
}

fn original_generate_with_random(
    terrain: &Terrain,
    chunk_x: i32,
    chunk_z: i32,
    generator: &BiomeGenerator,
    random: &mut JavaRandom,
) -> Result<Chunk> {
    random.set_seed(
        (chunk_x as i64)
            .wrapping_mul(341873128712)
            .wrapping_add((chunk_z as i64).wrapping_mul(132897987541)),
    );
    let generation = generator.get_biomes_for_generation(
        chunk_x.wrapping_mul(4).wrapping_sub(2),
        chunk_z.wrapping_mul(4).wrapping_sub(2),
        10,
        10,
    );
    let biomes =
        generator.get_biomes(chunk_x.wrapping_mul(16), chunk_z.wrapping_mul(16), 16, 16);
    let mut chunk = original_base_chunk(terrain, chunk_x, chunk_z, &generation);
    let tops =
        terrain.replace_surface_tops_with_random(chunk_x, chunk_z, &mut chunk, &biomes, random)?;
    chunk.biomes.copy_from_slice(&biomes);
    if terrain.settings.use_caves {
        super::super::carvers::caves(terrain.seed, chunk_x, chunk_z, &mut chunk.blocks, &tops);
    }
    if terrain.settings.use_ravines {
        super::super::carvers::ravines(terrain.seed, chunk_x, chunk_z, &mut chunk.blocks, &tops);
    }
    Ok(chunk)
}
