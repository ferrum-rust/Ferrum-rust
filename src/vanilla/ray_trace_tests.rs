use super::*;
use serde::Deserialize;
use std::collections::HashMap;

type EncodedHit = (u8, i32, i32, i32, u8, String, String, String);
#[derive(Deserialize)]
struct Corpus {
    boxes: Vec<[f64; 6]>,
    registry: Vec<[i32; 3]>,
    contexts: Vec<[i32; 8]>,
    inputs: Vec<[f64; 6]>,
    hits: Vec<EncodedHit>,
    canonical: Vec<(u16, Vec<[i32; 9]>)>,
    scenes: Vec<Scene>,
    c08: Vec<Option<String>>,
}
#[derive(Deserialize)]
struct Scene {
    cells: Vec<[i32; 4]>,
    rays: Vec<([String; 6], [i32; 8])>,
}
#[derive(Default)]
struct Grid(HashMap<IVec3, u16>);
impl Access for Grid {
    fn get(&self, p: IVec3) -> u16 {
        self.0.get(&p).copied().unwrap_or(0)
    }
}
fn bits(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}
fn read() -> Corpus {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-ray-trace.json"
    ))
    .unwrap()
}
fn options(f: usize) -> Options {
    Options {
        stop_on_liquid: f & 1 != 0,
        ignore_without_box: f & 2 != 0,
        return_last_miss: f & 4 != 0,
    }
}
fn expected(data: &Corpus, index: i32) -> Option<Hit> {
    let (kind, x, y, z, face, hx, hy, hz) = data.hits.get(usize::try_from(index).ok()?)?;
    Some(Hit {
        kind: if *kind == 0 { Kind::Miss } else { Kind::Block },
        position: IVec3::new(*x, *y, *z),
        face: *face,
        point: DVec3::new(bits(hx), bits(hy), bits(hz)),
    })
}
fn same(a: Option<Hit>, b: Option<Hit>) -> bool {
    a.map(|h| {
        (
            h.kind,
            h.position,
            h.face,
            h.point.to_array().map(f64::to_bits),
        )
    }) == b.map(|h| {
        (
            h.kind,
            h.position,
            h.face,
            h.point.to_array().map(f64::to_bits),
        )
    })
}
#[test]
fn legacy_torch_edits_restore_narrow_pick_bounds_without_rewriting_network_states() {
    let p = IVec3::new(0, 64, 0);
    let back = p + IVec3::Z;
    let mut grid = Grid::default();
    grid.0.insert(back, 16);
    let cast = |w: &Grid, x| trace(
        w, DVec3::new(x, 64.3, -1.), DVec3::new(x, 64.3, 2.), Options::PICK
    ).unwrap().position;
    for id in [50, 75, 76] {
        let raw = id << 4;
        grid.0.insert(p, raw);
        assert_eq!(cast(&grid, 0.5), back, "unmapped original section state");
        let restored = collision_shapes::legacy_overlay_state(raw);
        assert_eq!(restored, raw | 5);
        grid.0.insert(p, restored);
        assert_eq!(cast(&grid, 0.5), p);
        assert_eq!(cast(&grid, 0.7), back);
        for side in 1..=5 {
            assert_eq!(collision_shapes::legacy_overlay_state(raw | side), raw | side);
        }
    }
    assert_eq!(collision_shapes::legacy_overlay_state(u16::MAX), u16::MAX);
}
#[test]
fn original_selection_bounds_match_all_canonical_and_neighbor_contexts() {
    let data = read();
    let mut grid = Grid::default();
    let p = IVec3::new(0, 64, 0);
    for &[state, index, flags] in &data.registry {
        grid.0.clear();
        grid.0.insert(p, state as u16);
        assert_eq!(FLAGS[state as usize] as i32, flags);
        assert_eq!(
            selection_bounds(&grid, p, state as u16).map(f64::to_bits),
            data.boxes[index as usize].map(f64::to_bits),
            "isolated {state}"
        );
    }
    let sides = [
        IVec3::NEG_Z,
        IVec3::X,
        IVec3::Z,
        IVec3::NEG_X,
        IVec3::NEG_Y,
        IVec3::Y,
    ];
    for row in &data.contexts {
        grid.0.clear();
        grid.0.insert(p, row[0] as u16);
        for (i, d) in sides.iter().enumerate() {
            grid.0.insert(p + *d, row[i + 1] as u16);
        }
        assert_eq!(
            selection_bounds(&grid, p, row[0] as u16).map(f64::to_bits),
            data.boxes[row[7] as usize].map(f64::to_bits),
            "neighbor {row:?}"
        );
    }
    assert_eq!((data.registry.len(), data.contexts.len()), (1394, 99991));
}
#[test]
fn original_block_rays_and_world_dda_match_exact_hit_vectors_faces_and_misses() {
    let data = read();
    let mut grid = Grid::default();
    let p = IVec3::new(0, 64, 0);
    let mut count = 0;
    let mut errors = Vec::new();
    for (state, rows) in &data.canonical {
        grid.0.clear();
        grid.0.insert(p, *state);
        for (i, (r, row)) in data.inputs.iter().zip(rows).enumerate() {
            let a = DVec3::new(r[0], r[1] + 64., r[2]);
            let b = DVec3::new(r[3], r[4] + 64., r[5]);
            let direct = block(&grid, p, *state, a, b);
            let want = expected(&data, row[0]);
            if !same(direct, want) && errors.len() < 12 {
                errors.push(format!(
                    "direct state={state} ray={i} native={direct:?} original={want:?}"
                ));
            }
            count += 1;
            for (f, &idx) in row[1..].iter().enumerate() {
                let actual = trace(&grid, a, b, options(f));
                let want = expected(&data, idx);
                if !same(actual, want) && errors.len() < 12 {
                    errors.push(format!(
                        "world state={state} ray={i} flags={f} native={actual:?} original={want:?}"
                    ));
                }
                count += 1;
            }
        }
    }
    assert_eq!(count, 777852);
    let mut mixed = 0;
    for (scene_id, scene) in data.scenes.iter().enumerate() {
        grid.0.clear();
        for &[x, y, z, s] in &scene.cells {
            grid.0.insert(IVec3::new(x, y, z), s as u16);
        }
        for (i, (input, outputs)) in scene.rays.iter().enumerate() {
            let v = input.each_ref().map(|s| bits(s));
            let a = DVec3::new(v[0], v[1], v[2]);
            let b = DVec3::new(v[3], v[4], v[5]);
            for (f, &id) in outputs.iter().enumerate() {
                let actual = trace(&grid, a, b, options(f));
                let want = expected(&data, id);
                if !same(actual, want) && errors.len() < 12 {
                    errors.push(format!("mixed scene={scene_id} ray={i} flags={f} native={actual:?} original={want:?}"));
                }
                mixed += 1;
            }
        }
    }
    assert_eq!(mixed, 16640);
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}
#[test]
fn original_c08_wire_uses_cursor_point_from_actual_block_face() {
    let data = read();
    let mut packets = 0;
    for (i, hex) in data.c08.iter().enumerate() {
        let Some(hex) = hex else {
            continue;
        };
        let hit = expected(&data, i as i32).unwrap();
        let mut writer = crate::protocol::Writer::new();
        crate::network::write_placement(
            &mut writer,
            hit.position.to_array(),
            hit.face,
            None,
            hit.cursor(),
        )
        .unwrap();
        let bytes = (0..hex.len())
            .step_by(2)
            .map(|j| u8::from_str_radix(&hex[j..j + 2], 16).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(writer.0, bytes, "hit {hit:?}");
        packets += 1;
    }
    assert!(packets > 1000);
}

#[test]
fn end_portal_frame_selection_outline_raycast_and_collision_match_vanilla_13_sixteenths() {
    let p = IVec3::new(0, 64, 0);
    let back = p + IVec3::Z;
    let mut grid = Grid::default();
    grid.0.insert(back, 16);

    for state in 1920..=1927u16 {
        grid.0.insert(p, state);
        let sel = selection_bounds(&grid, p, state);
        assert_eq!(sel, [0.0, 0.0, 0.0, 1.0, 0.8125, 1.0], "selection_bounds for {state}");
        assert_eq!(sel[4], 0.8125);

        let out_origin = outline_bounds(&grid, IVec3::ZERO, state);
        assert_eq!(out_origin.min, DVec3::ZERO);
        assert_eq!(out_origin.max, DVec3::new(1.0, 0.8125, 1.0));
        assert_eq!(out_origin.max.y, 0.8125);

        let out_world = outline_bounds(&grid, p, state);
        assert_eq!(out_world.min, DVec3::new(0.0, 64.0, 0.0));
        assert_eq!(out_world.max, DVec3::new(1.0, 64.8125, 1.0));
    }

    for state in [1920u16, 1924u16] {
        grid.0.insert(p, state);
        let over_from = DVec3::new(0.5, 64.9, -1.0);
        let over_to = DVec3::new(0.5, 64.9, 2.0);
        assert!(block(&grid, p, state, over_from, over_to).is_none());
        let over_hit = trace(&grid, over_from, over_to, Options::PICK).unwrap();
        assert_eq!(over_hit.kind, Kind::Block);
        assert_eq!(over_hit.position, back);

        let mid_from = DVec3::new(0.5, 64.5, -1.0);
        let mid_to = DVec3::new(0.5, 64.5, 2.0);
        let direct_hit = block(&grid, p, state, mid_from, mid_to).unwrap();
        assert_eq!(direct_hit.position, p);
        let pick_hit = trace(&grid, mid_from, mid_to, Options::PICK).unwrap();
        assert_eq!(pick_hit.kind, Kind::Block);
        assert_eq!(pick_hit.position, p);

        assert!(!collision_shapes::full_collision_cube(p, state, |q| grid.get(q), None));
        let single = collision_shapes::single_box_for(p, state, |q| grid.get(q), None).unwrap();
        assert_eq!(single.min, DVec3::new(0.0, 64.0, 0.0));
        assert_eq!(single.max, DVec3::new(1.0, 64.8125, 1.0));
    }

    let mut empty_boxes = Vec::new();
    collision_shapes::emit(IVec3::ZERO, 1920, |_| 1920, None, |b| {
        empty_boxes.push([b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z]);
    });
    assert_eq!(empty_boxes, vec![[0.0, 0.0, 0.0, 1.0, 0.8125, 1.0]]);

    let mut filled_boxes = Vec::new();
    collision_shapes::emit(IVec3::ZERO, 1924, |_| 1924, None, |b| {
        filled_boxes.push([b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z]);
    });
    assert_eq!(
        filled_boxes,
        vec![
            [0.0, 0.0, 0.0, 1.0, 0.8125, 1.0],
            [0.3125, 0.8125, 0.3125, 0.6875, 1.0, 0.6875],
        ]
    );
}
