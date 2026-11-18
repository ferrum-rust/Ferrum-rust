use super::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

fn corpus() -> Value {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-scattered.json"
    ))
    .unwrap()
}
fn array(b: BoundingBox) -> [i32; 6] {
    [b.min[0], b.min[1], b.min[2], b.max[0], b.max[1], b.max[2]]
}
fn piece_json(p: &Piece) -> Value {
    let mut value = json!({"id":p.kind.id(),"BB":array(p.bounds),"O":p.orientation,"GD":0,"Width":p.size[0],"Height":p.size[1],"Depth":p.size[2],"HPos":p.ground_level});
    match p.kind {
        FeatureKind::SwampHut => value["Witch"] = p.witch_spawned.into(),
        FeatureKind::JunglePyramid => {
            for (index, key) in [
                "placedMainChest",
                "placedHiddenChest",
                "placedTrap1",
                "placedTrap2",
            ]
            .into_iter()
            .enumerate()
            {
                value[key] = p.flags[index].into();
            }
        }
        FeatureKind::DesertPyramid => {
            for index in 0..4 {
                value[format!("hasPlacedChest{index}")] = p.flags[index].into();
            }
        }
    }
    value
}
fn from_json(value: &Value) -> Piece {
    let kind = match value["id"].as_str().unwrap() {
        "TeDP" => FeatureKind::DesertPyramid,
        "TeJP" => FeatureKind::JunglePyramid,
        "TeSH" => FeatureKind::SwampHut,
        _ => unreachable!(),
    };
    let coords: [i32; 6] = serde_json::from_value(value["BB"].clone()).unwrap();
    let mut piece = Piece::new(kind, &mut JavaRandom::new(0), coords[0], coords[2]);
    piece.orientation = value["O"].as_u64().unwrap() as u8;
    piece.bounds = BoundingBox::new(
        [coords[0], coords[1], coords[2]],
        [coords[3], coords[4], coords[5]],
    );
    piece.ground_level = value["HPos"].as_i64().unwrap() as i32;
    piece
}
#[test]
fn candidate_lattice_biome_identity_and_layout_match_original() {
    let corpus = corpus();
    let expected: HashMap<(i64, u8, i32, i32), &Value> = corpus["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                (
                    row["seed"].as_str().unwrap().parse().unwrap(),
                    row["biome"].as_u64().unwrap() as u8,
                    row["chunk"][0].as_i64().unwrap() as i32,
                    row["chunk"][1].as_i64().unwrap() as i32,
                ),
                row,
            )
        })
        .collect();
    let mut found = 0;
    for seed in [0, 1, -1, 123456789012345, i64::MIN] {
        for biome in [2, 17, 21, 22, 6, 1, 130, 134] {
            for cx in -80..=80 {
                for cz in -80..=80 {
                    let actual = candidate(seed, cx, cz, biome);
                    let expected = expected.get(&(seed, biome, cx, cz));
                    assert_eq!(
                        actual.is_some(),
                        expected.is_some(),
                        "seed{seed}, biome{biome}, chunk{cx},{cz}"
                    );
                    if let (Some(actual), Some(expected)) = (actual, expected) {
                        assert_eq!(json!(array(actual.bounds)), expected["bounds"]);
                        assert_eq!(
                            piece_json(actual.piece.as_ref().unwrap()),
                            expected["piece"]
                        );
                        found += 1;
                    }
                }
            }
        }
    }
    assert_eq!(found, 645);
}
struct TestWorld {
    pattern: i32,
    blocks: BTreeMap<[i32; 3], u16>,
    containers: BTreeMap<[i32; 3], GeneratedContainer>,
    witches: Vec<[i32; 3]>,
}
impl ScatteredWorld for TestWorld {
    fn get(&self, p: [i32; 3]) -> u16 {
        if let Some(state) = self.blocks.get(&p) {
            return *state;
        }
        let [x, y, z] = p;
        let height = if self.pattern == 0 {
            63
        } else {
            60 + (x * 3 + z * 7).rem_euclid(9)
        };
        (if !(0..256).contains(&y) {
            0
        } else if y == 0 {
            7
        } else if y < height - 3 {
            1
        } else if y < height {
            3
        } else if y == height {
            2
        } else if self.pattern == 2 && y < 66 {
            9
        } else {
            0
        }) << 4
    }
    fn set(&mut self, p: [i32; 3], state: u16) {
        if !(0..256).contains(&p[1]) {
            return;
        }
        self.blocks.insert(p, state);
        let id = state >> 4;
        if matches!(id, 23 | 54) {
            if self.containers.get(&p).is_none_or(|c| c.block != id) {
                self.containers.insert(
                    p,
                    GeneratedContainer {
                        position: p,
                        block: id,
                        slots: vec![None; if id == 23 { 9 } else { 27 }],
                    },
                );
            }
        } else {
            self.containers.remove(&p);
        }
    }
    fn container(&mut self, p: [i32; 3], block: u16, slots: Vec<Option<LootStack>>) {
        self.containers.insert(
            p,
            GeneratedContainer {
                position: p,
                block,
                slots,
            },
        );
    }
    fn witch(&mut self, p: [i32; 3]) {
        self.witches.push(p);
    }
}
#[test]
fn every_template_block_loot_trap_spawn_and_persistent_flag_matches_original() {
    let corpus = corpus();
    let mut compared = 0;
    for (index, row) in corpus["cases"].as_array().unwrap().iter().enumerate() {
        let mut piece = from_json(&row["before"]);
        let mut world = TestWorld {
            pattern: row["pattern"].as_i64().unwrap() as i32,
            blocks: BTreeMap::new(),
            containers: BTreeMap::new(),
            witches: Vec::new(),
        };
        let mut random = JavaRandom::new(row["seed"].as_str().unwrap().parse().unwrap());
        for clip in row["clips"].as_array().unwrap() {
            let c: [i32; 6] = serde_json::from_value(clip.clone()).unwrap();
            assert!(piece.populate(
                &mut world,
                BoundingBox::new([c[0], c[1], c[2]], [c[3], c[4], c[5]]),
                &mut random
            ));
        }
        assert_eq!(
            random.next_long().to_string(),
            row["next_long"].as_str().unwrap(),
            "RNG case{index} {:?}",
            piece.kind
        );
        assert_eq!(
            piece_json(&piece),
            row["after"],
            "persistent NBT case{index}"
        );
        let expected = row["blocks"].as_array().unwrap();
        assert_eq!(
            world.blocks.len(),
            expected.len(),
            "write count case{index}"
        );
        for block in expected {
            let b: [i32; 4] = serde_json::from_value(block.clone()).unwrap();
            assert_eq!(
                world.blocks.get(&[b[0], b[1], b[2]]),
                Some(&(b[3] as u16)),
                "block case{index} {b:?}"
            );
        }
        let actual_containers: Vec<_> = world.containers.values().collect();
        let mut expected_containers = row["containers"].as_array().unwrap().clone();
        expected_containers
            .sort_by_key(|c| serde_json::from_value::<[i32; 3]>(c["position"].clone()).unwrap());
        assert_eq!(
            serde_json::to_value(actual_containers).unwrap(),
            json!(expected_containers),
            "containers case{index}"
        );
        assert_eq!(json!(world.witches), row["witches"], "witch case{index}");
        let restored: Piece = serde_json::from_slice(&serde_json::to_vec(&piece).unwrap()).unwrap();
        assert_eq!(piece_json(&restored), row["after"]);
        compared += expected.len();
    }
    assert_eq!(compared, 150592);
}
