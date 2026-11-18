use super::*;
use serde_json::{json, Value};
fn hash(mut h: u64, value: u64) -> u64 {
    for b in value.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}
pub(super) fn bbox(b: BoundingBox) -> [i32; 6] {
    [b.min[0], b.min[1], b.min[2], b.max[0], b.max[1], b.max[2]]
}
pub(super) fn nbt(p: &Piece) -> Value {
    let mut row = json!({"id":p.kind.id(),"BB":bbox(p.bounds),"O":p.orientation,"GD":p.depth});
    match p.kind {
        Kind::Corridor | Kind::Corridor2 => row["Chest"] = json!(p.chest_pending),
        Kind::Throne => row["Mob"] = json!(p.spawner_made),
        Kind::End => row["Seed"] = json!(p.fill_seed),
        _ => {}
    }
    row
}
#[test]
fn candidates_rng_and_every_structure_piece_match_original() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-fortress.json"
    ))
    .unwrap();
    for row in fixture["queries"].as_array().unwrap() {
        let seed = row["seed"].as_str().unwrap().parse::<i64>().unwrap();
        let mut random = JavaRandom::new(0);
        let mut h = 14695981039346656037u64;
        for x in -80..=80 {
            for z in -80..=80 {
                let can = candidate_random(seed, x, z, &mut random);
                h = hash(h, can as u64);
                h = hash(h, random.next_long() as u64);
            }
        }
        assert_eq!(
            h.to_string(),
            row["hash"].as_str().unwrap(),
            "candidate/RNG seed {seed}"
        );
    }
    let mut count = 0;
    for row in fixture["starts"].as_array().unwrap() {
        let seed = row["seed"].as_str().unwrap().parse::<i64>().unwrap();
        let cx = row["chunk"][0].as_i64().unwrap() as i32;
        let cz = row["chunk"][1].as_i64().unwrap() as i32;
        let context = format!("seed {seed} chunk {cx},{cz}");
        let mut random = JavaRandom::new(0);
        assert!(candidate_random(seed, cx, cz, &mut random));
        let start = Fortress::with_random(&mut random, cx, cz);
        assert_eq!(
            random.next_long().to_string(),
            row["next"].as_str().unwrap(),
            "RNG {context}"
        );
        assert_eq!(json!(bbox(start.bounds)), row["bounds"], "bounds {context}");
        let expected = row["pieces"].as_array().unwrap();
        assert_eq!(start.pieces.len(), expected.len(), "piece count {context}");
        for (i, (piece, row)) in start.pieces.iter().zip(expected).enumerate() {
            assert_eq!(nbt(piece), *row, "piece {i} {context}");
            count += 1;
        }
    }
    assert_eq!(count, 16554);
}
