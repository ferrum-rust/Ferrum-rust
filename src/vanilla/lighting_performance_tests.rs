use super::*;
use std::sync::Arc;

fn fixture(center: ChunkPos) -> World {
    let mut chunks = crate::world::ChunkMap::new();
    for cx in center.0 - 2..=center.0 + 2 { for cz in center.1 - 2..=center.1 + 2 {
        let mut chunk = Chunk::empty();
        for x in 0..16 { for z in 0..16 {
            let height = 28 + (cx * 16 + x + cz * 16 + z).rem_euclid(35);
            for y in 0..height {
                if !(18..22).contains(&y) || x % 4 == 0 { chunk.set(x,y,z,1 << 4); }
            }
            let surface = match (x + z) % 5 { 0 => 18, 1 => 20, 2 => 9, 3 => 79, _ => 2 };
            chunk.set(x, height, z, surface << 4);
            if x == 0 && z == 15 { chunk.set(x,255,z,89 << 4); }
        } }
        chunks.insert((cx,cz), Arc::new(chunk));
    } }
    World { chunks, spawn: Default::default(), label: String::new(), save_path: Default::default(),
        edits: Default::default(), imported: false, light_revision: 0 }
}

fn assert_equal(fast: &mut Lighting, reference: &mut Lighting) {
    assert_eq!(serde_json::to_value(&*fast).unwrap(), serde_json::to_value(&*reference).unwrap());
    assert_eq!(fast.take_render_dirty(), reference.take_render_dirty());
    assert!(fast.scratch.is_empty(), "scratch storage never holds pending light work");
}

#[test]
fn direct_column_and_reused_light_queue_match_reference_at_negative_edges_and_translucent_layers() {
    for center in [(0,0),(-3,2)] { for no_sky in [false,true] {
        let mut world = fixture(center);
        let mut fast = Lighting::new(&world, no_sky);
        let mut reference = fast.clone();
        reference.use_reference_path(true);
        let mut positions = world.chunks.keys().copied().collect::<Vec<_>>();
        positions.sort_unstable();
        for &pos in &positions {
            fast.generate_skylight_map(&world,pos);
            reference.generate_skylight_map(&world,pos);
        }
        assert_equal(&mut fast,&mut reference);
        for p in [center,(center.0-1,center.1),(center.0,center.1+1)] {
            assert_eq!(fast.initialize_chunk(&world,p,63, |_|true),
                reference.initialize_chunk(&world,p,63, |_|true));
        }
        assert_equal(&mut fast,&mut reference);
        for (offset,state) in [(IVec3::new(7,10,7),15<<4),(IVec3::new(7,10,7),16<<4),
            (IVec3::new(7,10,7),(1<<4)|3),(IVec3::new(7,10,7),0),
            (IVec3::new(15,45,0),89<<4),(IVec3::new(15,45,0),0),
            (IVec3::new(0,65,15),20<<4),(IVec3::new(0,65,15),18<<4),
            (IVec3::new(0,65,15),0),(IVec3::new(1,0,1),0),(IVec3::new(0,255,15),0)] {
            let p = IVec3::new(center.0*16,0,center.1*16)+offset;
            let old = world.get(p);
            world.set(p,state);
            fast.world_block_changed(&world,p,old);
            reference.world_block_changed(&world,p,old);
            assert_equal(&mut fast,&mut reference);
        }
    } }
}

#[test]
fn unchanged_light_checks_allocate_nothing_and_propagation_reuses_bounded_scratch() {
    let center = (0,0);
    let mut world = fixture(center);
    let mut light = Lighting::new(&world,false);
    let p = IVec3::new(8,100,8);
    light.take_render_dirty();
    assert_eq!(light.scratch.capacity(),0);
    for kind in [Kind::Sky,Kind::Block] { for _ in 0..100 {
        assert!(light.check(&world,kind,p));
        assert_eq!(light.scratch.capacity(),0, "an unchanged light check must not allocate its 128KiB queue");
    } }
    assert!(light.take_render_dirty().is_empty());
    world.set(p,89<<4);
    light.world_block_changed(&world,p,0);
    assert!(light.scratch.capacity()>=32768);
    assert!(light.scratch.is_empty());
    let allocation = light.scratch.as_ptr();
    world.set(p,0);
    light.world_block_changed(&world,p,89<<4);
    assert_eq!(light.scratch.as_ptr(),allocation);
    assert!(light.scratch.is_empty());
}

#[test]
fn direct_column_reads_preserve_air_below_world_for_negative_sea_levels() {
    let center = (-3,2);
    let world = fixture(center);
    let mut fast = Lighting::new(&world,false);
    let mut reference = fast.clone();
    reference.use_reference_path(true);
    // -1 includes Y=0; -2 also reads Y=-1, which World.get defines as air.
    for sea_level in [-1,-2] {
        assert_eq!(fast.initialize_chunk(&world,center,sea_level, |_|true),
            reference.initialize_chunk(&world,center,sea_level, |_|true));
        assert_equal(&mut fast,&mut reference);
    }
}

#[test]
fn opaque_replacements_match_original_sections_height_repairs_and_render_dirty() {
    for center in [(0, 0), (-3, 2)] { for no_sky in [false, true] {
        let mut world = fixture(center);
        let mut fast = Lighting::new(&world, no_sky);
        let mut reference = fast.clone();
        reference.use_reference_path(true);
        assert_equal(&mut fast, &mut reference);
        let origin = IVec3::new(center.0 * 16, 0, center.1 * 16);
        // Explicitly force uncommon cached-light states. Simply writing ores
        // under ordinary terrain would never exercise the required fallback.
        for (offset, height, section, old, new) in [
            (IVec3::new(8, 10, 8), 50, true, 16, 224),
            (IVec3::new(15, 31, 0), 60, false, 16, 17),
            (IVec3::new(0, 45, 15), 45, true, 16, 1168),
            (IVec3::new(15, 63, 0), 40, true, 87 << 4, 2448),
            (IVec3::new(0, 80, 15), 40, false, 16, 48),
            (IVec3::new(1, 0, 1), 1, true, 16, 896),
            (IVec3::new(0, 255, 15), 256, true, 16, 2064),
            (IVec3::new(15, 255, 0), 240, false, 16, 336),
            (IVec3::new(8, 10, 8), 50, false, 17, 17),
        ] {
            let p = origin + offset;
            let previous = world.get(p);
            world.set(p, old);
            fast.world_block_changed(&world, p, previous);
            reference.world_block_changed(&world, p, previous);
            assert_equal(&mut fast, &mut reference);
            for lighting in [&mut fast, &mut reference] {
                let c = lighting.chunks.get_mut(&cp(p)).unwrap();
                c.height[column(p)] = height;
                c.sections[p.y as usize / 16] = section;
            }
            world.set(p, new);
            fast.opaque_block_replaced(&world, p, old, new);
            reference.chunk_block_changed(&world, p, old);
            // Inspect the phase boundary as well as final lighting. A helper
            // cannot defer a surface repair past the original publication.
            assert_equal(&mut fast, &mut reference);
            fast.after_block_added_state(&world, p, old, new);
            reference.after_block_added_state(&world, p, old, new);
            assert_equal(&mut fast, &mut reference);
        }
        // The reference switch must force the original phase even when a
        // specialized caller needs to create a section and repair its height.
        let p = origin + IVec3::new(12, 100, 12);
        let previous = world.get(p);
        world.set(p, 16);
        fast.world_block_changed(&world, p, previous);
        reference.world_block_changed(&world, p, previous);
        assert_equal(&mut fast, &mut reference);
        let mut via_helper = reference.clone();
        for lighting in [&mut via_helper, &mut reference] {
            let c = lighting.chunks.get_mut(&cp(p)).unwrap();
            c.height[column(p)] = 50;
            c.sections[p.y as usize / 16] = false;
        }
        world.set(p, 224);
        via_helper.opaque_block_replaced(&world, p, 16, 224);
        reference.chunk_block_changed(&world, p, 16);
        assert_equal(&mut via_helper, &mut reference);
        fast = via_helper;
        fast.use_reference_path(false);
        let p = origin + IVec3::new(7, 10, 7);
        world.set(p, 224);
        fast.chunks.remove(&cp(p));
        reference.chunks.remove(&cp(p));
        for position in [p, origin + IVec3::new(8, -1, 8), origin + IVec3::new(8, 256, 8)] {
            fast.opaque_block_replaced(&world, position, 16, 224);
            reference.chunk_block_changed(&world, position, 16);
            fast.after_block_added_state(&world, position, 16, 224);
            reference.after_block_added_state(&world, position, 16, 224);
            assert_equal(&mut fast, &mut reference);
        }
    } }
}

#[test]
fn retained_column_light_predicate_is_conservative_and_skips_only_original_noops() {
    for center in [(0, 0), (-3, 2)] { for no_sky in [false, true] {
        let mut world = fixture(center);
        world.chunks.retain(|&p, _| p == center);
        let mut fast = Lighting::new(&world, no_sky);
        let mut reference = fast.clone();
        reference.use_reference_path(true);
        assert_equal(&mut fast, &mut reference);
        let p = IVec3::new(center.0 * 16 + 8, 10, center.1 * 16 + 8);
        for (old, new) in [(16, 224), (17, 22), ((87 << 4) | 15, 2448), (16, 48)] {
            let before = world.get(p);
            world.set(p, old);
            fast.world_block_changed(&world, p, before);
            reference.world_block_changed(&world, p, before);
            assert_equal(&mut fast, &mut reference);
            assert!(fast.opaque_replacement_is_noop(p, old, new));
            assert!(!reference.opaque_replacement_is_noop(p, old, new),
                "the original light profile must not use the retained-column writer");
            world.set(p, new);
            // The optimized side omits BOTH phases. Compare the untouched
            // state and dirty set against executing each original phase.
            reference.chunk_block_changed(&world, p, old);
            assert_equal(&mut fast, &mut reference);
            reference.after_block_added_state(&world, p, old, new);
            assert_equal(&mut fast, &mut reference);
        }
        for (old, new) in [(16, 16), (0, 224), (224, 0), (20 << 4, 224),
            (224, 20 << 4), (74 << 4, 224), (224, 74 << 4), (89 << 4, 224)] {
            assert!(!fast.opaque_replacement_is_noop(p, old, new),
                "nonopaque/emitting or unchanged state must use the ordinary adapter");
        }
        for invalid in [IVec3::new(p.x, -1, p.z), IVec3::new(p.x, 256, p.z),
            IVec3::new(-30_000_001, 10, 0), IVec3::new(30_000_000, 10, 0),
            IVec3::new(0, 10, -30_000_001), IVec3::new(0, 10, 30_000_000)] {
            assert!(!fast.opaque_replacement_is_noop(invalid, 16, 224));
        }
        fast.chunks.get_mut(&center).unwrap().sections[p.y as usize / 16] = false;
        assert!(!fast.opaque_replacement_is_noop(p, 16, 224));
        fast.chunks.get_mut(&center).unwrap().sections[p.y as usize / 16] = true;
        for height in [p.y, p.y - 1, -999] {
            fast.chunks.get_mut(&center).unwrap().height[column(p)] = height;
            assert!(!fast.opaque_replacement_is_noop(p, 16, 224));
        }
        fast.chunks.get_mut(&center).unwrap().height[column(p)] = p.y + 1;
        assert!(fast.opaque_replacement_is_noop(p, 16, 224));
        fast.chunks.remove(&center);
        assert!(!fast.opaque_replacement_is_noop(p, 16, 224),
            "true permits callers to index the existing light column");
    } }
}
