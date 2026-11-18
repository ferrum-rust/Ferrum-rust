use super::*;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Default)]
struct Grid {
    cells: HashMap<IVec3, u16>,
    light: u8,
}
impl collision_shapes::BlockView for Grid {
    fn get(&self, p: IVec3) -> u16 {
        self.cells.get(&p).copied().unwrap_or(0)
    }
}
impl Access for Grid {
    fn light(&self, _: IVec3) -> u8 {
        self.light
    }
}
#[derive(Deserialize)]
struct Corpus {
    states: Vec<u16>,
    replaceable: Vec<[u16; 2]>,
    bits: Vec<(u16, u8, u8, String)>,
    random: Vec<([i32; 14], bool)>,
    clicks: Vec<Click>,
}
#[derive(Deserialize)]
struct Click {
    cells: Vec<[i32; 4]>,
    id: u16,
    meta: u16,
    side: u8,
    hit_y: f32,
    yaw: f32,
    player: [f64; 3],
    light: u8,
    accepted: bool,
    error: bool,
    count: u8,
    writes: Vec<[i32; 4]>,
}
fn corpus() -> Corpus {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-hand-placement.json"
    ))
    .unwrap()
}
#[test]
fn original_hand_supports_cover_all_canonical_states_and_clicked_replacement() {
    let corpus = corpus();
    let mut grid = Grid::default();
    let p = IVec3::new(0, 64, 0);
    let mut count = 0;
    for [state, answer] in corpus.replaceable {
        grid.cells.clear();
        grid.cells.insert(p, state);
        assert_eq!(
            replaceable_for_click(&grid, p),
            answer != 0,
            "replaceable {state}"
        );
    }
    for (block, side, pattern, hex) in corpus.bits {
        let bits = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect::<Vec<_>>();
        for (i, &state) in corpus.states.iter().enumerate() {
            grid.cells.clear();
            grid.light = if pattern == 3 { 0 } else { 15 };
            match pattern {
                0 | 3 => {
                    grid.cells.insert(p - IVec3::Y, state);
                }
                1 => {
                    grid.cells.insert(p - SIDES[side as usize], state);
                }
                2 => {
                    grid.cells.insert(p, state);
                    grid.cells.insert(p - IVec3::Y, 16);
                }
                _ => unreachable!(),
            }
            assert_eq!(
                can_replace(&grid, p, block, side),
                bits[i / 8] & (1 << (i & 7)) != 0,
                "candidate={block} side={side} pattern={pattern} state={state}"
            );
            count += 1;
        }
    }
    let directions = [
        IVec3::NEG_Z,
        IVec3::X,
        IVec3::Z,
        IVec3::NEG_X,
        IVec3::NEG_Y,
        IVec3::Y,
    ];
    for (input, answer) in &corpus.random {
        grid.cells.clear();
        grid.light = input[2] as u8;
        grid.cells.insert(p, input[3] as u16);
        for (i, dir) in directions.iter().enumerate() {
            grid.cells.insert(p + *dir, input[4 + i] as u16);
        }
        for (i, dir) in HORIZONTAL.iter().enumerate() {
            grid.cells.insert(p + *dir - IVec3::Y, input[10 + i] as u16);
        }
        assert_eq!(
            can_replace(&grid, p, input[0] as u16, input[1] as u8),
            *answer,
            "mixed neighborhood {input:?}"
        );
    }
    assert_eq!((count, corpus.random.len()), (2484108, 19008));
}
#[test]
fn actual_itemblock_5400_clicks_match_acceptance_consumption_and_written_states() {
    let corpus = corpus();
    let p = IVec3::new(0, 64, 0);
    let mut mismatches = Vec::new();
    for (index, row) in corpus.clicks.iter().enumerate() {
        let mut grid = Grid {
            light: row.light,
            ..Default::default()
        };
        for &[x, y, z, state] in &row.cells {
            grid.cells.insert(IVec3::new(x, y, z), state as u16);
        }
        let player = DVec3::from_array(row.player);
        let bounds = Aabb::player(player);
        let input = Input {
            item: row.id,
            meta: row.meta,
            count: 2,
            clicked: p,
            side: row.side,
            hit_y: row.hit_y,
            yaw: row.yaw,
            player,
            eye_height: 1.62,
        };
        let result = plan(&grid, input, |box_| bounds.intersects(box_));
        let error = result.is_err();
        let result = result.unwrap_or_default();
        let writes = result
            .writes
            .iter()
            .map(|(p, s)| [p.x, p.y, p.z, i32::from(*s)])
            .collect::<Vec<_>>();
        let mismatch = (error, result.accepted, u8::from(result.consume), &writes)
            != (row.error, row.accepted, 2 - row.count, &row.writes);
        if mismatch && mismatches.len() < 40 {
            mismatches.push(format!("case {index} item={} meta={} side={} native=({error},{},{},{writes:?}) java=({},{},{},{:?})",row.id,row.meta,row.side,result.accepted,result.consume,row.error,row.accepted,2-row.count,row.writes));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    assert_eq!(corpus.clicks.len(), 5400);
}
