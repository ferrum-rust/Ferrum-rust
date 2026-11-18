//! Returned validation inputs must behave exactly like the former full map.
use super::*;

fn retained(input: &Inputs, lights: &[ChunkPos], meshes: &[ChunkPos]) -> Inputs {
    let mut result = input.clone();
    retain_validation_inputs(&mut result, lights.iter().copied(), meshes.iter().copied());
    result
}

fn same_answers(full: &Inputs, reduced: &Inputs, current: &Inputs,
    lights: &[ChunkPos], meshes: &[ChunkPos]) {
    for &p in lights {
        assert_eq!(light_inputs_unchanged(current, full, p),
            light_inputs_unchanged(current, reduced, p), "light dependencies at {p:?}");
        // retry_publications reads exactly the original center snapshot.
        match (full.get(&p), reduced.get(&p)) {
            (Some(a), Some(b)) => assert!(Arc::ptr_eq(a, b)),
            (None, None) => {},
            _ => panic!("retry center was lost at {p:?}"),
        }
    }
    for &p in meshes {
        assert_eq!(unchanged(current, &stamp(full, p)),
            unchanged(current, &stamp(reduced, p)), "mesh dependencies at {p:?}");
    }
}

#[test]
fn reduced_inputs_preserve_full_map_validation_for_load_unload_and_arc_only_changes() {
    let shared = Arc::new(Chunk::empty());
    let mut full: Inputs = (-8..=8).flat_map(|x| (-8..=8).map(move |z| (x,z)))
        .map(|p| (p, shared.clone())).collect();
    // Exercise originally absent cells within both dependency radii.
    full.remove(&(-3, 2)); full.remove(&(5, -4));
    let lights = [(-2, 1), (4, -3)];
    let meshes = [(-5, -5), (2, 4), (4, -3)];
    let reduced = retained(&full, &lights, &meshes);
    assert!(reduced.len() < full.len());
    assert!(reduced.len() <= 25 * lights.len() + 9 * meshes.len());
    same_answers(&full, &reduced, &full, &lights, &meshes);
    let changed = Arc::new(Chunk::empty());
    for x in -9..=9 { for z in -9..=9 {
        let p = (x,z);
        let mut current = full.clone();
        current.remove(&p);
        same_answers(&full, &reduced, &current, &lights, &meshes);
        // Same bytes, different Arc: covers tile-only replacements as well as
        // newly loaded neighbors without depending on content equality.
        current.insert(p, changed.clone());
        same_answers(&full, &reduced, &current, &lights, &meshes);
    } }
}

#[test]
fn reduced_inputs_keep_rejected_light_centers_and_drop_unneeded_payloads_in_the_worker() {
    let keep = Arc::new(Chunk::empty());
    let discard = Arc::new(Chunk::empty());
    let kept = Arc::downgrade(&keep);
    let discarded = Arc::downgrade(&discard);
    let mut input = Inputs::from_iter([((0,0), keep), ((8,8), discard)]);
    // This is the exact worker-end helper. No additional asynchronous queue
    // retains the discarded snapshot after the result becomes publishable.
    let reduced = std::thread::spawn(move || {
        retain_validation_inputs(&mut input, [(0,0)], std::iter::empty());
        assert!(discarded.upgrade().is_none());
        input
    }).join().unwrap();
    assert_eq!(reduced.len(), 1);
    assert!(kept.upgrade().is_some());
    drop(reduced);
    assert!(kept.upgrade().is_none());
}

#[test]
fn reduced_inputs_keep_overlapping_and_empty_dependency_sets_exact() {
    let shared = Arc::new(Chunk::empty());
    let full: Inputs = (-3..=3).flat_map(|x| (-3..=3).map(move |z| (x,z)))
        .map(|p| (p, shared.clone())).collect();
    let reduced = retained(&full, &[(0,0), (0,0)], &[(0,0), (0,0)]);
    assert_eq!(reduced.len(), 25);
    for &p in reduced.keys() { assert!(p.0.abs() <= 2 && p.1.abs() <= 2); }
    let mesh_only = retained(&full, &[], &[(0,0)]);
    assert_eq!(mesh_only.len(), 9);
    let empty = retained(&full, &[], &[]);
    assert!(empty.is_empty());
    assert_eq!(empty.capacity(), 0);
}
