use super::*;
use std::sync::Arc;

fn world() -> World {
    World { chunks: Default::default(), spawn: Default::default(), label: String::new(),
        save_path: Default::default(), edits: Default::default(), imported: false, light_revision: 0 }
}
fn assert_same(a: &mut Lighting, b: &mut Lighting) {
    assert_eq!(serde_json::to_value(&*a).unwrap(), serde_json::to_value(&*b).unwrap());
    assert_eq!(a.take_render_dirty(), b.take_render_dirty());
    assert!(a.scratch.is_empty() && b.scratch.is_empty());
}

#[test]
fn prepared_initial_light_matches_original_empty_sections_translucency_and_height_edges() {
    for no_sky in [false, true] { for case in 0..5 {
        let pos = (-3, 7);
        let mut terrain = world();
        let mut chunk = Chunk::empty();
        match case {
            0 => {},
            1 => { chunk.set(0,0,0,16); },
            2 => { chunk.set(15,255,15,16); },
            3 => {
                for x in 0..16 { for z in 0..16 {
                    chunk.set(x,0,z,7 << 4);
                    chunk.set(x,32,z,9 << 4);
                    chunk.set(x,48,z,18 << 4);
                    chunk.set(x,64,z,20 << 4);
                    chunk.set(x,80,z,79 << 4);
                    if (x+z)%3 == 0 { chunk.set(x,79,z,89 << 4); }
                } }
            },
            _ => {
                // Presence is based on states, not opacity or the cached top.
                chunk.set(1,255,2,65535);
                chunk.set(2,15,3,20 << 4);
                chunk.set(3,127,4,50 << 4);
                chunk.top = 0;
            },
        }
        let prepared = PreparedLight::new(&chunk, no_sky);
        // A neighboring emitter and arbitrary populated light do not influence
        // initial per-column maps; they matter only in later initialization.
        let mut neighbor = Chunk::empty();
        neighbor.set(15,70,0,89 << 4);
        terrain.chunks.insert((-4,7),Arc::new(neighbor));
        let mut fast = Lighting::new(&terrain,no_sky);
        fast.chunks.get_mut(&(-4,7)).unwrap().block.fill(11);
        fast.sky_subtracted = 9;
        let mut direct = fast.clone();
        direct.use_reference_path(true);
        terrain.chunks.insert(pos,Arc::new(chunk));
        assert!(fast.add_prepared_chunk(&terrain,pos,prepared));
        direct.add_chunk(&terrain,pos);
        assert_same(&mut fast,&mut direct);
        if case == 0 {
            assert_eq!(fast.chunks[&pos].minimum,i32::MAX);
            assert!(fast.chunks[&pos].height.iter().all(|&v| v==0));
        }
        assert_eq!(fast.chunks[&pos].queued_checks,4096);
        assert!(fast.chunks[&pos].gaps.iter().all(|&v| !v));
    } }
}

#[test]
fn prepared_initial_light_falls_back_after_mode_change_or_reference_activation() {
    let pos=(2,-2);
    let mut terrain=world();
    let mut chunk=Chunk::empty();
    chunk.set(3,16,7,16);
    for (prepared_mode,current_mode,reference) in [(false,true,false),(true,false,false),(false,false,true)] {
        let prepared=PreparedLight::new(&chunk,prepared_mode);
        let mut actual=Lighting::new(&terrain,current_mode);
        actual.use_reference_path(reference);
        let mut expected=actual.clone();
        terrain.chunks.insert(pos,Arc::new(chunk.clone()));
        assert!(!actual.add_prepared_chunk(&terrain,pos,prepared));
        expected.add_chunk(&terrain,pos);
        assert_same(&mut actual,&mut expected);
        terrain.chunks.remove(&pos);
    }
}
