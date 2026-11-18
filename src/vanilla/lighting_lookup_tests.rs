use super::*;
use std::sync::Arc;

// Independent pre-optimization pair: both ordinary checks perform their own
// availability query, retaining the current single-check propagation body.
fn original_pair(light: &mut Lighting, world: &World, p: IVec3) -> bool {
    let sky = !light.no_sky && light.check(world, Kind::Sky, p);
    light.check(world, Kind::Block, p) || sky
}

#[test]
fn paired_availability_keeps_sky_then_block_propagation_and_missing_border_results() {
    for center in [(-2, 1), (1_874_999, 0), (-1_875_000, 0)] {
        let mut base = world_at(center);
        for x in center.0 - 2..=center.0 + 2 { for z in center.1 - 2..=center.1 + 2 {
            base.chunks.entry((x, z)).or_insert_with(|| Arc::new(Chunk::empty()));
        } }
        for no_sky in [false, true] { for missing in [false, true] {
            let mut world = base.clone();
            let mut actual = Lighting::new(&world, no_sky);
            if missing { actual.chunks.remove(&(center.0 - 1, center.1)); }
            let mut expected = actual.clone();
            actual.take_render_dirty(); expected.take_render_dirty();
            for (x, y, z) in [(7, -18, 7), (7, -17, 7), (7, -1, 7), (0, 0, 15),
                (7, 31, 7), (15, 63, 0), (7, 255, 7), (7, 256, 7), (7, 272, 7), (7, 273, 7)] {
                let p = IVec3::new(center.0 * 16 + x, y, center.1 * 16 + z);
                for state in [50 << 4, 16, 0] {
                    world.set_raw(p, state);
                    assert_eq!(actual.check_both(&world, p), original_pair(&mut expected, &world, p),
                        "pair {p:?} no_sky={no_sky} missing={missing}");
                    same_light(&mut actual, &mut expected);
                }
            }
        } }
    }
}

fn world_at(center: ChunkPos) -> World {
    let mut chunks = crate::world::ChunkMap::new();
    for x in center.0 - 1..=center.0 + 1 { for z in center.1 - 1..=center.1 + 1 {
        let mut chunk = Chunk::empty();
        chunk.set(3, 7, 5, 16);
        chunk.set(8, 63, 8, 16);
        chunks.insert((x, z), Arc::new(chunk));
    } }
    World { chunks, spawn: Default::default(), label: String::new(), save_path: Default::default(),
        edits: Default::default(), imported: false, light_revision: 0 }
}

fn unusual_light(world: &World, no_sky: bool, alternating: bool) -> Lighting {
    let mut light = Lighting::new(world, no_sky);
    for (&(x, z), chunk) in &mut light.chunks {
        for (i, value) in chunk.height.iter_mut().enumerate() { *value = (i as i32 * 3 + x + z).rem_euclid(257); }
        chunk.minimum = *chunk.height.iter().min().unwrap();
        chunk.sections = std::array::from_fn(|section| !alternating || section % 2 == 0);
        for i in 0..65536 {
            chunk.sky[i] = ((i as i32 + x * 3 + z).rem_euclid(16)) as u8;
            chunk.block[i] = ((i as i32 * 7 + x + z * 3).rem_euclid(16)) as u8;
        }
        chunk.gaps[3] = true;
        chunk.queued_checks = 319;
    }
    light.take_render_dirty();
    light
}

fn same_light(actual: &mut Lighting, expected: &mut Lighting) {
    assert_eq!(actual.no_sky, expected.no_sky);
    assert_eq!(actual.sky_subtracted, expected.sky_subtracted);
    assert_eq!(actual.chunks.len(), expected.chunks.len());
    for (p, a) in &actual.chunks {
        let b = &expected.chunks[p];
        assert_eq!((&a.height, a.minimum, a.sections, &a.gaps, a.queued_checks),
            (&b.height, b.minimum, b.sections, &b.gaps, b.queued_checks), "column {p:?}");
        assert!(a.sky == b.sky, "sky bytes in {p:?}");
        assert!(a.block == b.block, "block bytes in {p:?}");
    }
    assert_eq!(actual.take_render_dirty(), expected.take_render_dirty());
}

#[test]
fn existing_section_light_writes_match_original_nibbles_dirty_state_and_invalid_positions() {
    let world = world_at((-1, 0));
    for no_sky in [false, true] {
        let mut actual = unusual_light(&world, no_sky, false);
        let mut expected = actual.clone();
        expected.use_reference_path(true);
        for p in [IVec3::new(-16, 0, 0), IVec3::new(-1, 15, 15), IVec3::new(0, 16, 0),
            IVec3::new(-8, 255, 7), IVec3::new(-8, -1, 7), IVec3::new(-8, 256, 7),
            IVec3::new(64, 80, 64), IVec3::new(-30_000_001, 80, 0), IVec3::new(30_000_000, 80, 0)] {
            for kind in [Kind::Sky, Kind::Block] { for value in [0, 0, 15, 31, 16, 7, 7, 255] {
                actual.set(&world, kind, p, value);
                expected.set(&world, kind, p, value);
                same_light(&mut actual, &mut expected);
            } }
        }
        if no_sky {
            let p = IVec3::new(-8, 32, 7);
            let old_sky = actual.chunks[&cp(p)].sky[index(p)];
            actual.set(&world, Kind::Sky, p, 5);
            assert_eq!(actual.chunks[&cp(p)].sky[index(p)], old_sky);
            assert_eq!(actual.take_render_dirty(), vec![cp(p)],
                "no-sky get returns zero, so a suppressed nonzero Sky write still dirties");
            actual.set(&world, Kind::Sky, p, 16);
            assert!(actual.take_render_dirty().is_empty(), "masked zero does not dirty no-sky storage");
        }
    }
}

#[test]
fn light_lookup_fast_path_keeps_original_section_creation_and_skylight_regeneration() {
    let mut world = world_at((0, 0));
    world.set_raw(IVec3::new(4, 255, 6), 89 << 4);
    for no_sky in [false, true] {
        let mut actual = unusual_light(&world, no_sky, true);
        let mut expected = actual.clone();
        expected.use_reference_path(true);
        for p in [IVec3::new(4, 17, 6), IVec3::new(-1, 49, 15), IVec3::new(4, 255, 6)] {
            assert!(!actual.chunks[&cp(p)].sections[p.y as usize / 16]);
            actual.set(&world, Kind::Sky, p, 0);
            expected.set(&world, Kind::Sky, p, 0);
            same_light(&mut actual, &mut expected);
            assert!(actual.chunks[&cp(p)].sections[p.y as usize / 16]);
            actual.set(&world, Kind::Block, p, 29);
            expected.set(&world, Kind::Block, p, 29);
            same_light(&mut actual, &mut expected);
        }
    }
}

#[test]
fn raw_column_reuse_matches_original_neighbors_height_defaults_borders_and_early_outs() {
    for center in [(-2, 1), (1_874_999, 0), (-1_875_000, 0)] {
        for no_sky in [false, true] { for alternating in [false, true] {
            let mut world = world_at(center);
            let mut actual = unusual_light(&world, no_sky, alternating);
            // Missing neighboring light is distinct from missing terrain.
            actual.chunks.remove(&(center.0 + 1, center.1));
            for missing_center in [false, true] {
                if missing_center { actual.chunks.remove(&center); }
                let mut expected = actual.clone();
                expected.use_reference_path(true);
                for x in [-1, 0, 7, 15, 16] { for z in [0, 15] {
                    for y in [-2, 0, 15, 16, 63, 255, 256] {
                        let p = IVec3::new(center.0 * 16 + x, y, center.1 * 16 + z);
                        for state in [0, 16, 18 << 4, 20 << 4, 8 << 4, 10 << 4, 50 << 4, 89 << 4, u16::MAX] {
                            world.set_raw(p, state);
                            for kind in [Kind::Sky, Kind::Block] {
                                assert_eq!(actual.raw(&world, kind, p), expected.raw(&world, kind, p),
                                    "raw {kind:?} at {p:?} state {state} no_sky={no_sky} alternating={alternating} missing={missing_center}");
                            }
                        }
                    }
                } }
                same_light(&mut actual, &mut expected);
            }
        } }
    }
}

// Independent pre-change vertical-scan bodies, retaining ordinary checks.
fn previous_neighbor_height(light: &mut Lighting, world: &World, x: i32, z: i32, min: i32, max: i32) {
        if max > min && light.area_loaded(IVec3::new(x, 0, z), 16) {
            for y in min..max {
                light.check(world, Kind::Sky, IVec3::new(x, y, z));
            }
        }
    }
fn previous_check_column(light: &mut Lighting, world: &World, pos: ChunkPos,
        x: i32, z: i32, sea_level: i32) -> bool {
        let chunk = world.chunks.get(&pos);
        let state = |y| if (0..256).contains(&y) { chunk.map_or(0, |c| c.get(x, y, z)) } else { 0 };
        let top = light.chunks[&pos]
            .sections
            .iter()
            .rposition(|v| *v)
            .unwrap_or(0) as i32
            * 16;
        let mut opaque_seen = false;
        let mut below_sea = false;
        let mut y = top + 15;
        let mut last_y = 0;
        while y > sea_level || y > 0 && !below_sea {
            let p = IVec3::new(pos.0 * 16 + x, y, pos.1 * 16 + z);
            last_y = y;
            let opacity = material::light_opacity(state(y));
            if opacity == 255 && y < sea_level {
                below_sea = true;
            }
            if !opaque_seen && opacity > 0 {
                opaque_seen = true;
            } else if opaque_seen && opacity == 0 && !light.check_both(world, p) {
                return false;
            }
            y -= 1;
        }
        for y in (1..=last_y).rev() {
            let p = IVec3::new(pos.0 * 16 + x, y, pos.1 * 16 + z);
            if material::light_value(state(y)) > 0 {
                light.check_both(world, p);
            }
        }
        true
    }

#[test]
fn fixed_column_availability_matches_original_vertical_scans_and_missing_edges() {
    for center in [(-2, 1), (1_874_999, 0), (-1_875_000, 0)] {
        let mut world = world_at(center);
        for x in center.0-2..=center.0+2 { for z in center.1-2..=center.1+2 {
            world.chunks.entry((x,z)).or_insert_with(|| Arc::new(Chunk::empty()));
        } }
        for x in 0..16 { for z in 0..16 {
            for y in [12, 32, 80] {
                world.set_raw(IVec3::new(center.0*16+x,y,center.1*16+z),16);
            }
        } }
        world.set_raw(IVec3::new(center.0*16+7,7,center.1*16+7),89<<4);
        for no_sky in [false, true] { for missing in [None,Some((center.0-2,center.1)),Some(center)] {
            let mut actual=Lighting::new(&world,no_sky);
            if let Some(p)=missing { actual.chunks.remove(&p); }
            let mut expected=actual.clone();
            actual.take_render_dirty(); expected.take_render_dirty();
            for (x,z) in [(0,0),(0,15),(7,7),(15,0),(15,15)] {
                let (wx,wz)=(center.0*16+x,center.1*16+z);
                for (min,max) in [(-19,-17),(-18,1),(0,0),(40,39),(0,83),(254,275)] {
                    actual.update_neighbor_height(&world,wx,wz,min,max);
                    previous_neighbor_height(&mut expected,&world,wx,wz,min,max);
                    same_light(&mut actual,&mut expected);
                }
                if missing != Some(center) {
                    for sea in [0,63,255,257] {
                        assert_eq!(actual.check_column(&world,center,x,z,sea),
                            previous_check_column(&mut expected,&world,center,x,z,sea));
                        same_light(&mut actual,&mut expected);
                    }
                }
            }
        } }
    }
}

#[test]
fn fixed_column_availability_cache_is_local_lazy_and_keeps_y_guards() {
    let world=world_at((0,0));
    let mut light=Lighting::new(&world,false);
    let mut available=None;
    assert!(!light.fixed_column_loaded(IVec3::new(7,-18,7),&mut available));
    assert_eq!(available,None,"invalid Y must not initialize membership");
    assert!(light.fixed_column_loaded(IVec3::new(7,-17,7),&mut available));
    assert_eq!(available,Some(true));
    assert!(!light.fixed_column_loaded(IVec3::new(7,273,7),&mut available));
    assert!(light.fixed_column_loaded(IVec3::new(7,272,7),&mut available));
    light.chunks.remove(&(1,0));
    let mut next_call=None;
    assert!(!light.fixed_column_loaded(IVec3::new(7,64,7),&mut next_call));
    assert_eq!(next_call,Some(false),"a later call cannot reuse stale membership");
}
