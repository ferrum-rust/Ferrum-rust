use super::*;
use std::sync::Arc;

// Complete original body, deliberately separate from relight_inner and its
// DIRECT=false arm. Both scans retain the original World::get calls.
fn original_relight(light: &mut Lighting, world: &World, p: IVec3) {
    let pos = cp(p);
    let col = column(p);
    let old = light.chunks[&pos].height[col] & 255;
    let mut height = old.max(p.y);
    while height > 0
        && material::light_opacity(world.get(IVec3::new(p.x, height - 1, p.z))) == 0
    {
        height -= 1;
    }
    if height == old {
        return;
    }
    if !light.no_sky {
        for y in height.min(old)..=height.max(old) {
            light.check(world, Kind::Sky, IVec3::new(p.x, y, p.z));
        }
    }
    light.chunks.get_mut(&pos).unwrap().height[col] = height;
    light.render_dirty.insert(pos);
    if !light.no_sky {
        let c = light.chunks.get_mut(&pos).unwrap();
        for y in height.min(old)..height.max(old) {
            if c.sections[y as usize / 16] {
                c.sky[index(IVec3::new(p.x, y, p.z))] = if height < old { 15 } else { 0 };
            }
        }
        let mut value = 15i32;
        let mut y = height;
        while y > 0 && value > 0 {
            y -= 1;
            value = (value
                - material::light_opacity(world.get(IVec3::new(p.x, y, p.z))).max(1) as i32)
                .max(0);
            if c.sections[y as usize / 16] {
                c.sky[index(IVec3::new(p.x, y, p.z))] = value as u8;
            }
        }
    }
    let c = light.chunks.get_mut(&pos).unwrap();
    c.minimum = c.minimum.min(height);
    if !light.no_sky {
        for d in HORIZONTAL {
            light.update_neighbor_height(
                world,
                p.x + d.x,
                p.z + d.z,
                old.min(height),
                old.max(height),
            );
        }
        light.update_neighbor_height(world, p.x, p.z, old.min(height), old.max(height));
    }
}

fn fixture(center: ChunkPos) -> World {
    let mut chunks = crate::world::ChunkMap::new();
    for cx in center.0 - 2..=center.0 + 2 {
        for cz in center.1 - 2..=center.1 + 2 {
            let mut chunk = Chunk::empty();
            for x in 0..16 {
                for z in 0..16 {
                    chunk.set(x, 0, z, 16);
                    chunk.set(x, 6, z, 2 << 4);
                }
            }
            chunk.set(14, 9, 15, 18 << 4);
            chunk.set(15, 10, 15, 18 << 4);
            chunk.set(0, 13, 0, 161 << 4);
            chunk.set(8, 2, 8, 50 << 4);
            chunks.insert((cx, cz), Arc::new(chunk));
        }
    }
    World {
        chunks, spawn: Default::default(), label: String::new(), save_path: Default::default(),
        edits: Default::default(), imported: false, light_revision: 0,
    }
}

fn same_light(actual: &Lighting, expected: &Lighting) {
    assert_eq!(actual.no_sky, expected.no_sky);
    assert_eq!(actual.sky_subtracted, expected.sky_subtracted);
    assert_eq!(actual.chunks.len(), expected.chunks.len());
    for (pos, a) in &actual.chunks {
        let b = &expected.chunks[pos];
        assert_eq!((&a.height, a.minimum, a.sections, &a.gaps, a.queued_checks),
            (&b.height, b.minimum, b.sections, &b.gaps, b.queued_checks), "metadata {pos:?}");
        assert!(a.sky == b.sky, "sky {pos:?}");
        assert!(a.block == b.block, "block {pos:?}");
    }
    assert_eq!(actual.render_dirty, expected.render_dirty);
    assert!(actual.scratch.is_empty());
    assert!(expected.scratch.is_empty());
}

#[test]
fn relight_borrow_matches_independent_original_through_canopy_growth_and_removal() {
    for center in [(0, 0), (-3, 2)] {
        for no_sky in [false, true] {
            let mut world = fixture(center);
            let mut actual = Lighting::new(&world, no_sky);
            let mut expected = actual.clone();
            let origin = IVec3::new(center.0 * 16, 0, center.1 * 16);
            for (offset, state, relight_offset) in [
                (IVec3::new(15, 11, 15), 18 << 4, 1),
                (IVec3::new(15, 12, 15), 18 << 4, 1),
                (IVec3::new(15, 17, 15), 161 << 4, 1),
                (IVec3::new(15, 11, 15), 17 << 4, 1),
                (IVec3::new(15, 17, 15), 0, 0),
                (IVec3::new(15, 12, 15), 0, 0),
                (IVec3::new(15, 11, 15), 0, 0),
                (IVec3::new(0, 13, 0), 0, 0),
                (IVec3::new(0, 15, 0), 20 << 4, 1),
                (IVec3::new(0, 15, 0), 8 << 4, 1),
                (IVec3::new(0, 15, 0), 0, 0),
                (IVec3::new(15, 255, 15), 18 << 4, 1),
                (IVec3::new(15, 255, 15), 0, 0),
            ] {
                let p = origin + offset;
                let old = world.get(p);
                world.set_raw(p, state);
                let relight_at = p + IVec3::Y * relight_offset;
                // This isolated oracle leaves the check/get/set implementations
                // identical on both sides and compares the complete light state
                // before and after the ordinary after-block-added phase.
                actual.relight(&world, relight_at);
                original_relight(&mut expected, &world, relight_at);
                same_light(&actual, &expected);
                actual.after_block_added_state(&world, p, old, state);
                expected.after_block_added_state(&world, p, old, state);
                same_light(&actual, &expected);
            }
        }
    }
}

#[test]
fn relight_borrow_keeps_raw_world_y_border_missing_storage_and_section_semantics() {
    // World::get has no X/Z world-border rejection, unlike public light reads.
    // Include columns entirely outside both borders to catch such a new guard.
    for center in [(-2, 1), (-1_875_001, 0), (1_875_000, 0)] {
        for no_sky in [false, true] {
            let base = fixture(center);
            for (y, cached_height, missing_raw, missing_neighbor) in [
                (-17, 12, false, false),
                (-1, 12, false, true),
                (0, 0, false, false),
                (16, 7, false, true),
                (256, 0, false, false),
                (257, 256, false, true),
                (17, 12, true, false),
                (256, 255, true, true),
            ] {
                let mut world = base.clone();
                let p = IVec3::new(center.0 * 16 + 15, y, center.1 * 16 + 15);
                let mut actual = Lighting::new(&world, no_sky);
                {
                    let c = actual.chunks.get_mut(&center).unwrap();
                    c.height[column(p)] = cached_height;
                    c.sections = std::array::from_fn(|section| section % 2 == 0);
                    c.gaps[column(p)] = true;
                    c.queued_checks = 319;
                }
                if missing_neighbor { actual.chunks.remove(&(center.0 + 1, center.1)); }
                if missing_raw { world.chunks.remove(&center); }
                let mut expected = actual.clone();
                actual.relight(&world, p);
                original_relight(&mut expected, &world, p);
                same_light(&actual, &expected);

                // A retained light entry without raw storage is air, whereas a
                // missing light entry retains the old immediate indexing panic.
                actual.chunks.remove(&center);
                expected.chunks.remove(&center);
                let a = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| actual.relight(&world, p)));
                let b = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| original_relight(&mut expected, &world, p)));
                assert!(a.is_err() && b.is_err());
                same_light(&actual, &expected);
            }
        }
    }
}

#[test]
fn relight_reference_switch_matches_independent_original_scan() {
    let mut world = fixture((-1, 0));
    let p = IVec3::new(-1, 19, 15);
    let mut actual = Lighting::new(&world, false);
    let mut expected = actual.clone();
    actual.use_reference_path(true);
    expected.use_reference_path(true);
    for state in [18 << 4, 17 << 4, 0] {
        world.set_raw(p, state);
        actual.relight(&world, p + IVec3::Y);
        original_relight(&mut expected, &world, p + IVec3::Y);
        same_light(&actual, &expected);
    }
}
