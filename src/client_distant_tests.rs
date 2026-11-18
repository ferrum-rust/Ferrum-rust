use super::*;
use clap::Parser;

fn scene() -> DistantScene {
    let options = Options::parse_from(["ferrum189"]);
    let source = DistantTerrain::new(&options).unwrap().unwrap();
    let mut scene = DistantScene::new(source);
    scene.set_view((0, 0), 128);
    scene
}
fn blank_world() -> World {
    World {
        chunks: crate::world::ChunkMap::new(),
        spawn: Vec3::ZERO,
        label: String::new(),
        save_path: Default::default(),
        edits: Default::default(),
        imported: false,
        light_revision: 0,
    }
}
fn ground() -> Arc<Chunk> {
    let mut chunk = Chunk::empty();
    for z in 0..16 {
        for x in 0..16 {
            chunk.set(x, 63, z, 2 << 4);
        }
    }
    Arc::new(chunk)
}
fn base(position: TilePos) -> TerrainTile {
    TerrainTile {
        position,
        heights: vec![
            Surface {
                height: 64,
                state: 2 << 4,
                biome: 1
            };
            GRID * GRID
        ],
    }
}
fn install(scene: &mut DistantScene, p: TilePos) {
    let tile = Arc::new(base(p));
    scene.cached.insert(p, tile.clone());
    scene.resident.insert(p, tile);
    scene.resident_strides.insert(p,mesh_stride(scene.center.unwrap().0,p));
}
fn begin(scene: &mut DistantScene, p: TilePos) -> Work {
    let work = scene.work(p);
    assert!(scene
        .pending
        .insert(p, (work.epoch, work.version))
        .is_none());
    work
}
fn finish(work: Work) -> Completed {
    let p = work.position;
    let epoch = work.epoch;
    let version = work.version;
    let stride = work.stride;
    let tile = work
        .base
        .as_ref()
        .map_or_else(|| base(p), |tile| tile.as_ref().clone());
    Completed {
        position: p,
        epoch,
        version,
        stride,
        result: Ok(work.finish(tile, |_| crate::models::empty())),
        milliseconds: 1.,
    }
}

#[test]
fn requested_tiles_cover_real_128_radius_and_clip_original_world_border() {
    for center in [(0, 0), (-1, -1), (7, 8), (-129, 129)] {
        let positions = tiles(center, 128).into_iter().collect::<HashSet<_>>();
        assert_eq!(positions.len(), 1089);
        for x in center.0 - 128..=center.0 + 128 {
            for z in center.1 - 128..=center.1 + 128 {
                assert!(positions.contains(&tile_of((x, z))));
            }
        }
        assert!(positions.len() * GRID * GRID * std::mem::size_of::<Surface>() < 8 * 1024 * 1024);
    }
    for center in [
        (-1_875_000, -1_875_000),
        (1_874_999, 1_874_999),
        (-1_875_000, 0),
        (0, 1_874_999),
    ] {
        let positions = tiles(center, 128).into_iter().collect::<HashSet<_>>();
        assert!(!positions.is_empty());
        assert!(positions
            .iter()
            .all(|&(x, z)| (-234375..234375).contains(&x) && (-234375..234375).contains(&z)));
        for x in (center.0 - 128).max(-1_875_000)..=(center.0 + 128).min(1_874_999) {
            for z in (center.1 - 128).max(-1_875_000)..=(center.1 + 128).min(1_874_999) {
                assert!(positions.contains(&tile_of((x, z))));
            }
        }
    }
    assert!(tiles((0, 0), 24).is_empty());
    assert_eq!(IN_FLIGHT, 2);
}

#[test]
fn mesh_density_bands_keep_every_near_chunk_fine_including_negative_tiles() {
    assert_eq!(mesh_stride((0,0),(8,0)),4); // tile intersects the 64-chunk band
    assert_eq!(mesh_stride((0,0),(9,0)),8);
    assert_eq!(mesh_stride((0,0),(12,0)),8);
    assert_eq!(mesh_stride((0,0),(13,0)),16);
    assert_eq!(mesh_stride((0,0),(-8,0)),4);
    assert_eq!(mesh_stride((0,0),(-9,0)),8);
    assert_eq!(mesh_stride((0,0),(-13,0)),16);
    for center in [(0,0),(-1,-1),(7,8),(-129,129)] {
        for x in center.0-64..=center.0+64 {
            for z in center.1-64..=center.1+64 {
                assert_eq!(mesh_stride(center,tile_of((x,z))),4);
            }
        }
    }
    let counts=[4u8,8,16].map(|stride| tiles((0,0),128).into_iter()
        .filter(|&p|mesh_stride((0,0),p)==stride).count());
    assert_eq!(counts,[289,336,464]);
}

#[test]
fn density_changes_remesh_unchanged_surfaces_and_reject_old_band_jobs() {
    let mut scene=scene();
    let p=(13,0);
    install(&mut scene,p);
    assert_eq!(scene.resident_strides[&p],16);
    let old_surface=scene.resident[&p].clone();
    scene.set_view((40,0),128);
    assert!(scene.dirty.contains(&p));
    let fine=begin(&mut scene,p);
    assert_eq!(fine.stride,4);
    assert!(scene.completed(finish(fine)).is_some(),
        "density changes need new geometry even with an identical surface");
    assert_eq!(scene.resident_strides[&p],4);
    assert!(Arc::ptr_eq(&old_surface,&scene.resident[&p]));
    scene.set_view((0,0),128);
    let coarse=begin(&mut scene,p);
    assert_eq!(coarse.stride,16);
    scene.set_view((40,0),128);
    assert!(scene.completed(finish(coarse)).is_none(),
        "an old coarse completion cannot replace the now-required fine mesh");
    assert_eq!(scene.resident_strides[&p],4);
    assert!(scene.dirty.contains(&p));
    let same=begin(&mut scene,p);
    assert!(scene.completed(finish(same)).is_none());
    assert!(!scene.dirty.contains(&p));
    scene.reset_pack();
    assert!(scene.resident_strides.is_empty());
    let fresh=begin(&mut scene,p);
    assert!(scene.completed(finish(fresh)).is_some());
    assert_eq!(scene.resident_strides[&p],4);
}

#[test]
fn first_tile_completion_uses_current_density_after_camera_moves() {
    let mut scene=scene();
    let p=(13,0);
    let old=begin(&mut scene,p);
    assert_eq!(old.stride,16);
    scene.set_view((40,0),128);
    assert!(scene.completed(finish(old)).is_none());
    assert!(scene.cached.contains_key(&p));
    assert!(!scene.resident.contains_key(&p));
    let fresh=begin(&mut scene,p);
    assert_eq!(fresh.stride,4);
    assert!(scene.completed(finish(fresh)).is_some());
    assert_eq!(scene.resident_strides[&p],4);
}

#[test]
fn edits_during_busy_job_survive_unload_and_old_ack_cannot_clear_new_capture() {
    let mut scene = scene();
    let p = (0, 0);
    install(&mut scene, p);
    let mut world = blank_world();
    world.chunks.insert(p, ground());
    scene.observe(&world);
    let old = begin(&mut scene, p);
    assert!(world.set(IVec3::new(2, 70, 2), 4 << 4));
    scene.capture_chunk(&world, p);
    let latest = scene.captures[&p].stamp;
    world.unload_chunk(p);
    assert!(
        scene.completed(finish(old)).is_none(),
        "busy old job cannot upload over a newer capture"
    );
    assert_eq!(scene.captures[&p].stamp, latest);
    assert_eq!(scene.resident[&p].heights[0].height, 64);
    let work = begin(&mut scene, p);
    assert!(scene.completed(finish(work)).is_some());
    assert_eq!(
        scene.resident[&p].heights[0],
        Surface {
            height: 71,
            state: 4 << 4,
            biome: 1
        }
    );
    assert!(scene.captures.is_empty());
    assert_eq!(scene.full_captures, 0);
    assert!(scene.dirty.is_empty());
    // A pack rebuild after unload must use captured surfaces, not pure cache.
    scene.reset_pack();
    let work = begin(&mut scene, p);
    assert!(work.captures.is_empty());
    assert!(scene.completed(finish(work)).is_some());
    assert_eq!(scene.resident[&p].heights[0].height, 71);
}

#[test]
fn stale_result_retains_other_chunk_changes_and_compares_against_uploaded_surface() {
    let mut scene = scene();
    let p = (0, 0);
    install(&mut scene, p);
    let mut world = blank_world();
    world.chunks.insert((0, 0), ground());
    world.chunks.insert((1, 0), ground());
    assert!(world.set(IVec3::new(2, 70, 2), 4 << 4));
    scene.observe(&world);
    let old = begin(&mut scene, p);
    // The later Arc differs only in lighting; nevertheless it invalidates the
    // old version. The already processed edit still needs its first GPU upload.
    let cloned = world.chunks[&(1, 0)].as_ref().clone();
    world.chunks.insert((1, 0), Arc::new(cloned));
    world.invalidate_lighting();
    scene.observe(&world);
    assert!(scene.completed(finish(old)).is_none());
    assert_eq!(scene.cached[&p].heights[0].height, 71);
    assert_eq!(scene.resident[&p].heights[0].height, 64);
    assert!(!scene.captures.contains_key(&(0, 0)));
    assert!(scene.captures.contains_key(&(1, 0)));
    let work = begin(&mut scene, p);
    assert!(
        scene.completed(finish(work)).is_some(),
        "comparison must use last uploaded, not merely last processed surface"
    );
    assert_eq!(scene.resident[&p].heights[0].height, 71);
}

#[test]
fn light_only_arc_replacements_skip_remesh_but_population_and_removal_refresh() {
    let mut scene = scene();
    let p = (0, 0);
    install(&mut scene, p);
    let mut world = blank_world();
    world.chunks.insert(p, ground());
    scene.observe(&world);
    let first = begin(&mut scene, p);
    assert!(scene.completed(finish(first)).is_none());
    for _ in 0..3 {
        world
            .chunks
            .insert(p, Arc::new(world.chunks[&p].as_ref().clone()));
        world.invalidate_lighting();
        scene.observe(&world);
        let work = begin(&mut scene, p);
        let tile = work.base.as_ref().unwrap().as_ref().clone();
        let epoch = work.epoch;
        let version = work.version;
        let stride = work.stride;
        let finished = work.finish(tile, |_| panic!("unchanged surfaces must not build a mesh"));
        assert!(scene
            .completed(Completed {
                position: p,
                epoch,
                version,
                stride,
                result: Ok(finished),
                milliseconds: 1.
            })
            .is_none());
    }
    assert_eq!(scene.unchanged_jobs, 4);
    assert!(world.set(IVec3::new(2, 75, 2), 18 << 4));
    scene.observe(&world);
    let work = begin(&mut scene, p);
    assert!(scene.completed(finish(work)).is_some());
    assert_eq!(scene.resident[&p].heights[0].height, 76);
    assert!(world.set(IVec3::new(2, 75, 2), 0));
    scene.capture_chunk(&world, p);
    world.unload_chunk(p);
    let work = begin(&mut scene, p);
    assert!(scene.completed(finish(work)).is_some());
    assert_eq!(scene.resident[&p].heights[0].height, 64);
}

#[test]
fn pack_change_rejects_old_geometry_but_retains_newest_captured_surfaces() {
    let mut scene = scene();
    let p = (0, 0);
    install(&mut scene, p);
    let mut world = blank_world();
    world.chunks.insert(p, ground());
    world.set(IVec3::new(2, 80, 2), 12 << 4);
    scene.observe(&world);
    let old = begin(&mut scene, p);
    let epoch = old.epoch;
    scene.reset_pack();
    assert_ne!(scene.epoch, epoch);
    assert!(scene.completed(finish(old)).is_none());
    assert!(!scene.resident.contains_key(&p));
    assert!(scene.cached.contains_key(&p));
    let work = begin(&mut scene, p);
    assert!(work.uploaded.is_none());
    assert!(scene.completed(finish(work)).is_some());
    assert_eq!(
        scene.resident[&p].heights[0],
        Surface {
            height: 81,
            state: 12 << 4,
            biome: 1
        }
    );
}

#[test]
fn full_chunk_capture_memory_is_bounded_and_compact_fallback_preserves_unloaded_edits() {
    let mut scene = scene();
    let mut world = blank_world();
    let chunk = ground();
    // Shared immutable chunks avoid allocating hundreds of MiB in this bound
    // regression; each distinct capture still consumes one retention slot.
    for z in -26..=26 {
        for x in -26..=26 {
            world.chunks.insert((x, z), chunk.clone());
        }
    }
    scene.observe(&world);
    assert_eq!(scene.full_captures, MAX_CAPTURED_CHUNKS);
    assert_eq!(scene.compacted_on_frame, 2809 - MAX_CAPTURED_CHUNKS as u64);
    let p = *scene
        .captures
        .iter()
        .find(|(_, c)| matches!(c.data, CaptureData::Samples(_)))
        .unwrap()
        .0;
    world.set(IVec3::new(p.0 * 16 + 2, 100, p.1 * 16 + 2), 1 << 4);
    scene.capture_chunk(&world, p);
    let CaptureData::Samples(samples) = &scene.captures[&p].data else {
        panic!("overflow stays compact");
    };
    assert_eq!(samples[0].height, 101);
    world.unload_chunk(p);
    let tile = tile_of(p);
    let work = begin(&mut scene, tile);
    assert!(scene.completed(finish(work)).is_some());
    let index = (p.1.rem_euclid(8) * 4) as usize * GRID + (p.0.rem_euclid(8) * 4) as usize;
    assert_eq!(scene.resident[&tile].heights[index].height, 101);
    scene.set_view((1024, 1024), 128);
    assert_eq!(scene.full_captures, 0);
    assert!(scene.captures.is_empty());
    assert!(scene.cached.is_empty());
    assert!(scene.observed.is_empty());
}

#[test]
fn worker_panic_returns_error_completion_and_releases_pending_slot() {
    let mut scene = scene();
    let p = (0, 0);
    let work = begin(&mut scene, p);
    let error = guarded_work(p, || panic!("synthetic worker failure"));
    assert!(error.is_err());
    assert!(scene
        .completed(Completed {
            position: p,
            epoch: work.epoch,
            version: work.version,
            stride: work.stride,
            result: error,
            milliseconds: 1.
        })
        .is_none());
    assert!(scene.pending.is_empty());
    assert!(scene.error.as_ref().unwrap().contains("worker panicked"));
    assert!(!scene.ready());
}

#[test]
fn disabled_distant_view_does_not_track_changed_chunks_and_reenable_captures_fresh_state() {
    let mut scene = scene();
    let p = (0, 0);
    install(&mut scene, p);
    let mut world = blank_world();
    world.chunks.insert(p, ground());
    scene.observe(&world);
    let old = begin(&mut scene, p);
    assert_eq!(scene.suspend((0, 0), 16), vec![p]);
    assert!(scene.captures.is_empty());
    assert_eq!(scene.full_captures, 0);
    let stamp = scene.next_stamp;
    for height in 70..=80 {
        world.set(IVec3::new(2, height, 2), 1 << 4);
        scene.observe(&world);
        scene.capture_chunk(&world, p);
    }
    assert!(scene.observed.is_empty());
    assert!(scene.captures.is_empty());
    assert!(scene.dirty.is_empty());
    assert_eq!(scene.next_stamp, stamp);
    assert_eq!(scene.observed_revision, None);
    scene.sender.send(finish(old)).unwrap();
    scene.suspend((0, 0), 16);
    assert!(scene.pending.is_empty());
    assert!(scene.cached.is_empty());
    scene.set_view((0, 0), 128);
    scene.observe(&world);
    let work = begin(&mut scene, p);
    assert!(scene.completed(finish(work)).is_some());
    assert_eq!(scene.resident[&p].heights[0].height, 81);
}

#[test]
fn fair_dispatch_advances_missing_tiles_and_continuously_dirty_residents() {
    let mut scene = scene();
    let hot = scene.order[0];
    install(&mut scene, hot);
    scene.dirty.insert(hot);
    let mut expected_missing = scene.order.iter().copied().filter(|p| *p != hot)
        .take(9).collect::<VecDeque<_>>();
    let p = scene.next_position().unwrap();
    assert_eq!(Some(p), expected_missing.pop_front());
    let mut missing = begin(&mut scene, p);
    assert_eq!(scene.next_position(), Some(hot));
    let mut refresh = begin(&mut scene, hot);
    assert_eq!(scene.pending.len(), IN_FLIGHT);
    assert_eq!(scene.next_position(), None);
    for _ in 0..8 {
        // A slow cold tile must not freeze a fast stream of nearby updates.
        for _ in 0..3 {
            assert!(scene.completed(finish(refresh)).is_none());
            scene.dirty.insert(hot);
            assert_eq!(scene.next_position(), Some(hot));
            refresh = begin(&mut scene, hot);
            assert_eq!(scene.next_position(), None);
        }
        // The reciprocal case: a hot dirty queue never steals the slot just
        // freed by a missing tile, even though hot is nearest in camera order.
        let completed = missing.position;
        assert!(scene.completed(finish(missing)).is_some());
        assert!(scene.resident.contains_key(&completed));
        let p = scene.next_position().unwrap();
        assert_eq!(Some(p), expected_missing.pop_front());
        missing = begin(&mut scene, p);
        assert_eq!(scene.pending.len(), IN_FLIGHT);
        assert_eq!(scene.next_position(), None);
    }
    assert!(expected_missing.is_empty());
    assert!(scene.completed(finish(missing)).is_some());
    assert!(scene.completed(finish(refresh)).is_none());
    assert_eq!(scene.resident.len(), 10);
}

#[test]
fn fair_dispatch_uses_both_slots_when_only_one_class_has_work() {
    let mut missing_only = scene();
    let first = missing_only.order[0];
    let second = missing_only.order[1];
    assert_eq!(missing_only.next_position(), Some(first));
    let _first = begin(&mut missing_only, first);
    assert_eq!(missing_only.next_position(), Some(second));
    let _second = begin(&mut missing_only, second);
    assert_eq!(missing_only.next_position(), None);

    let mut refresh_only = scene();
    for p in refresh_only.order.clone() { install(&mut refresh_only, p); }
    let first = refresh_only.order[0];
    let second = refresh_only.order[1];
    refresh_only.dirty.extend([second, first]);
    assert_eq!(refresh_only.next_position(), Some(first));
    let _first = begin(&mut refresh_only, first);
    assert_eq!(refresh_only.next_position(), Some(second));
    let _second = begin(&mut refresh_only, second);
    assert_eq!(refresh_only.next_position(), None);
}

#[test]
fn fair_dispatch_retains_edit_rejection_and_teleported_job_budget() {
    let mut scene = scene();
    let hot = (0, 0);
    install(&mut scene, hot);
    let mut world = blank_world();
    world.chunks.insert(hot, ground());
    scene.observe(&world);
    let missing_p = scene.next_position().unwrap();
    assert_ne!(missing_p, hot);
    let missing = begin(&mut scene, missing_p);
    assert_eq!(scene.next_position(), Some(hot));
    let old_edit = begin(&mut scene, hot);
    world.set(IVec3::new(2, 75, 2), 1 << 4);
    scene.observe(&world);
    let latest = scene.captures[&hot].stamp;
    assert!(scene.completed(finish(old_edit)).is_none());
    assert_eq!(scene.resident[&hot].heights[0].height, 64);
    assert_eq!(scene.captures[&hot].stamp, latest);
    assert_eq!(scene.next_position(), Some(hot));
    let latest_edit = begin(&mut scene, hot);
    assert!(scene.completed(finish(latest_edit)).is_some());
    assert_eq!(scene.resident[&hot].heights[0].height, 76);

    // After a teleport, an obsolete job still occupies a real worker slot but
    // must neither publish old-view geometry nor change nearest-first order.
    scene.set_view((1024, 1024), 128);
    assert!(!scene.wanted.contains(&missing_p));
    assert_eq!(scene.pending.len(), 1);
    let new_p = scene.order[0];
    assert_eq!(scene.next_position(), Some(new_p));
    let new_view = begin(&mut scene, new_p);
    assert_eq!(scene.next_position(), None);
    assert!(scene.completed(finish(missing)).is_none());
    assert!(!scene.resident.contains_key(&missing_p));
    assert!(!scene.cached.contains_key(&missing_p));
    assert_eq!(scene.next_position(), Some(scene.order[1]));
    assert!(scene.completed(finish(new_view)).is_some());
    assert_eq!(scene.resident.len(), 1);
}
