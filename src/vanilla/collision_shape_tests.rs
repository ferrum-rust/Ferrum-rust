use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
struct Corpus {
    shapes: Vec<Vec<[f64; 6]>>,
    canonical: Vec<[i32; 3]>,
    contexts: Vec<[i32; 9]>,
    far: Vec<[i32; 6]>,
    pistons: Vec<[i32; 5]>,
    placement: Vec<Vec<i32>>,
    placement_context: Vec<[i32; 9]>,
    placement_far: Vec<[i32; 5]>,
    placement_history: Vec<[i32; 5]>,
    single: Vec<[i32; 3]>,
    single_context: Vec<[i32; 2]>,
    single_far: Vec<[i32; 2]>,
}
#[test]
fn original_existing_single_boxes_and_full_cube_query_match_block_state() {
    let corpus: Corpus = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-collision-shapes.json"
    ))
    .unwrap();
    let verify = |p: IVec3, state: u16, neighbors: [u16; 6], expected: i32| {
        let sides = [
            IVec3::NEG_Z,
            IVec3::X,
            IVec3::Z,
            IVec3::NEG_X,
            IVec3::NEG_Y,
            IVec3::Y,
        ];
        let get = |q| {
            if q == p {
                state
            } else {
                sides
                    .iter()
                    .position(|d| p + *d == q)
                    .map_or(0, |i| neighbors[i])
            }
        };
        let actual = single_box_for(p, state, get, None).map(|b| {
            let a = b.min - p.as_dvec3();
            let z = b.max - p.as_dvec3();
            [a.x, a.y, a.z, z.x, z.y, z.z].map(f64::to_bits)
        });
        let expected = corpus.shapes[expected as usize].first();
        assert_eq!(
            actual,
            expected.map(|b| b.map(f64::to_bits)),
            "cell {state} p={p:?} neighbors={neighbors:?}"
        );
        let full =
            expected.is_some_and(|b| ((b[3] - b[0]) + (b[4] - b[1]) + (b[5] - b[2])) / 3. >= 1.);
        assert_eq!(full_collision_cube(p, state, get, None), full);
    };
    for &[state, direct, _] in &corpus.single {
        verify(IVec3::ZERO, state as u16, [0; 6], direct);
    }
    for (row, shape) in corpus.contexts.iter().zip(&corpus.single_context) {
        verify(
            IVec3::ZERO,
            row[0] as u16,
            std::array::from_fn(|i| row[i + 1] as u16),
            shape[0],
        );
    }
    for (row, shape) in corpus.far.iter().zip(&corpus.single_far) {
        verify(
            IVec3::new(row[1], row[2], row[3]),
            row[0] as u16,
            [0; 6],
            shape[0],
        );
    }
    assert_eq!(
        (
            corpus.single.len(),
            corpus.single_context.len(),
            corpus.single_far.len()
        ),
        (1394, 77687, 4182)
    );
}

fn compare_placement(
    corpus: &Corpus,
    position: IVec3,
    candidate: u16,
    existing: u16,
    neighbors: [u16; 6],
    tile: Option<PistonTile>,
    expected: i32,
) {
    let sides = [
        IVec3::NEG_Z,
        IVec3::X,
        IVec3::Z,
        IVec3::NEG_X,
        IVec3::NEG_Y,
        IVec3::Y,
    ];
    let get = |p| {
        if p == position {
            existing
        } else {
            sides
                .iter()
                .position(|d| position + *d == p)
                .map_or(0, |i| neighbors[i])
        }
    };
    let result = placement_for(position, candidate, get, tile);
    assert_eq!(
        result.is_err(),
        expected < 0,
        "query candidate={candidate} existing={existing} neighbors={neighbors:?}"
    );
    if expected < 0 {
        return;
    }
    let actual = result.unwrap().map(|b| {
        let lo = b.min - position.as_dvec3();
        let hi = b.max - position.as_dvec3();
        [lo.x, lo.y, lo.z, hi.x, hi.y, hi.z].map(f64::to_bits)
    });
    let boxes = &corpus.shapes[expected as usize];
    assert!(boxes.len() <= 1);
    assert_eq!(actual,boxes.first().map(|b|b.map(f64::to_bits)),"box candidate={candidate} existing={existing} position={position:?} neighbors={neighbors:?}");
}
#[test]
fn original_hand_placement_uses_single_default_box_for_every_existing_state() {
    let corpus: Corpus = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-collision-shapes.json"
    ))
    .unwrap();
    let mut matrix_cases = 0;
    for row in &corpus.placement {
        assert_eq!(row.len(), corpus.canonical.len() + 2);
        for (state, shape) in corpus.canonical.iter().zip(&row[2..]) {
            compare_placement(
                &corpus,
                IVec3::ZERO,
                row[0] as u16,
                state[0] as u16,
                [0; 6],
                None,
                *shape,
            );
            matrix_cases += 1;
        }
    }
    for row in &corpus.placement_context {
        compare_placement(
            &corpus,
            IVec3::ZERO,
            row[0] as u16,
            row[1] as u16,
            std::array::from_fn(|i| row[i + 2] as u16),
            None,
            row[8],
        );
    }
    for &[id, x, y, z, shape] in &corpus.placement_far {
        compare_placement(
            &corpus,
            IVec3::new(x, y, z),
            id as u16,
            0,
            [0; 6],
            None,
            shape,
        );
    }
    for &[carried, face, extending, quarter, shape] in &corpus.pistons {
        let tile = PistonTile {
            carried: carried as u16,
            facing: face as u8,
            extending: extending != 0,
            progress: quarter as f32 * 0.25,
        };
        compare_placement(
            &corpus,
            IVec3::ZERO,
            36,
            36 << 4 | face as u16,
            [0; 6],
            Some(tile),
            shape,
        );
    }
    assert_eq!(
        (
            matrix_cases,
            corpus.placement_context.len(),
            corpus.placement_far.len()
        ),
        (276012, 59592, 594)
    );
    // The original singleton experiment is preserved separately. Replaying
    // renderer/raycast mutations globally is outside this pristine profile.
    assert_eq!(corpus.placement_history.len(), 385);
    assert!(corpus.placement_history.iter().all(|r| r[3] != r[4]));
}
fn compare(
    corpus: &Corpus,
    position: IVec3,
    state: u16,
    neighbors: [u16; 6],
    tile: Option<PistonTile>,
    expected: i32,
) {
    let sides = [
        IVec3::NEG_Z,
        IVec3::X,
        IVec3::Z,
        IVec3::NEG_X,
        IVec3::NEG_Y,
        IVec3::Y,
    ];
    let get = |p| {
        if p == position {
            state
        } else {
            sides
                .iter()
                .position(|d| position + *d == p)
                .map_or(0, |i| neighbors[i])
        }
    };
    let mut actual = Vec::new();
    emit(position, state, get, tile, |bounds| {
        let lo = bounds.min - position.as_dvec3();
        let hi = bounds.max - position.as_dvec3();
        actual.push([lo.x, lo.y, lo.z, hi.x, hi.y, hi.z].map(f64::to_bits));
    });
    let wanted = corpus.shapes[expected as usize]
        .iter()
        .map(|b| b.map(f64::to_bits))
        .collect::<Vec<_>>();
    assert_eq!(
        actual, wanted,
        "state={state} pos={position:?} neighbors={neighbors:?} piston={tile:?}"
    );
}
#[test]
fn original_registry_collision_boxes_match_canonical_states_neighbors_far_coordinates_and_pistons()
{
    let corpus: Corpus = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-collision-shapes.json"
    ))
    .unwrap();
    let mut mutable_bounds = 0;
    for &[state, pristine, prepared] in &corpus.canonical {
        compare(&corpus, IVec3::ZERO, state as u16, [0; 6], None, prepared);
        if pristine != prepared {
            assert_eq!(state >> 4, 145);
            mutable_bounds += 1;
        }
    }
    assert_eq!(mutable_bounds, 12);
    let mut chest_history = 0;
    for row in &corpus.contexts {
        compare(
            &corpus,
            IVec3::ZERO,
            row[0] as u16,
            std::array::from_fn(|i| row[i + 1] as u16),
            None,
            row[8],
        );
        if row[7] != row[8] {
            assert!(matches!(row[0] >> 4, 54 | 146));
            chest_history += 1;
        }
    }
    assert_eq!(chest_history, 120);
    for &[state, x, y, z, _, prepared] in &corpus.far {
        compare(
            &corpus,
            IVec3::new(x, y, z),
            state as u16,
            [0; 6],
            None,
            prepared,
        );
    }
    for &[carried, face, extending, quarter, shape] in &corpus.pistons {
        let tile = PistonTile {
            carried: carried as u16,
            facing: face as u8,
            extending: extending != 0,
            progress: quarter as f32 * 0.25,
        };
        compare(
            &corpus,
            IVec3::ZERO,
            36 << 4 | face as u16,
            [0; 6],
            Some(tile),
            shape,
        );
    }
    assert_eq!(
        (
            corpus.canonical.len(),
            corpus.contexts.len(),
            corpus.far.len(),
            corpus.pistons.len()
        ),
        (1394, 77687, 4182, 780)
    );
}
