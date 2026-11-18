use super::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
struct TestWorld {
    pattern: i32,
    blocks: BTreeMap<[i32; 3], u16>,
    chests: BTreeMap<[i32; 3], Vec<Option<LootStack>>>,
    spawners: BTreeMap<[i32; 3], String>,
}
impl StrongholdWorld for TestWorld {
    fn get(&self, p: [i32; 3]) -> u16 {
        if let Some(&s) = self.blocks.get(&p) {
            return s;
        }
        let mut id = if p[1] < 0 || p[1] > 63 { 0 } else { 1 };
        if self.pattern == 1 && (p[0] * 3 + p[1] * 2 + p[2] * 7).rem_euclid(17) < 4 {
            id = 0;
        }
        if self.pattern == 2 && p == [-4, 39, -5] {
            id = 9;
        }
        id << 4
    }
    fn set(&mut self, p: [i32; 3], state: u16) {
        if !(0..=255).contains(&p[1]) {
            return;
        }
        self.blocks.insert(p, state);
        if state >> 4 == 54 {
            self.chests.entry(p).or_insert_with(|| vec![None; 27]);
        } else {
            self.chests.remove(&p);
        }
        if state >> 4 == 52 {
            self.spawners.entry(p).or_insert_with(|| "Pig".into());
        } else {
            self.spawners.remove(&p);
        }
    }
    fn chest(&mut self, p: [i32; 3], slots: Vec<Option<LootStack>>) {
        self.chests.insert(p, slots);
    }
    fn spawner(&mut self, p: [i32; 3]) {
        self.spawners.insert(p, "Silverfish".into());
    }
}
fn bbox(value: &Value) -> BoundingBox {
    let n: Vec<i32> = serde_json::from_value(value.clone()).unwrap();
    BoundingBox::new([n[0], n[1], n[2]], [n[3], n[4], n[5]])
}
fn from_nbt(row: &Value) -> Piece {
    let kind = [
        Kind::Straight,
        Kind::Prison,
        Kind::LeftTurn,
        Kind::RightTurn,
        Kind::RoomCrossing,
        Kind::StairsStraight,
        Kind::Stairs,
        Kind::Crossing,
        Kind::ChestCorridor,
        Kind::Library,
        Kind::PortalRoom,
        Kind::Start,
        Kind::Corridor,
    ]
    .into_iter()
    .find(|k| row["id"] == k.id())
    .unwrap();
    let mut p = Piece::blank(
        kind,
        bbox(&row["BB"]),
        row["O"].as_u64().unwrap() as u8,
        row["GD"].as_i64().unwrap() as i32,
    );
    p.door = [Door::Opening, Door::Wood, Door::Grates, Door::Iron]
        .into_iter()
        .find(|d| row["EntryDoor"] == d.name())
        .unwrap();
    p.exits = if kind == Kind::Crossing {
        ["leftLow", "leftHigh", "rightLow", "rightHigh"].map(|k| row[k].as_bool().unwrap())
    } else {
        [
            row["Left"].as_bool().unwrap_or(false),
            row["Right"].as_bool().unwrap_or(false),
            false,
            false,
        ]
    };
    p.large = row["Tall"].as_bool().unwrap_or(false);
    p.source = row["Source"].as_bool().unwrap_or(false);
    p.room_type = row["Type"].as_i64().unwrap_or(0) as i32;
    p.steps = row["Steps"].as_i64().unwrap_or(0) as i32;
    p
}
#[test]
fn all_template_blocks_random_stream_loot_spawner_and_flags_match_original() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-stronghold-population.json"
    ))
    .unwrap();
    let mut count = 0;
    for (case, row) in fixture.as_array().unwrap().iter().enumerate() {
        let mut piece = from_nbt(&row["before"]);
        let mut world = TestWorld {
            pattern: row["pattern"].as_i64().unwrap() as i32,
            blocks: BTreeMap::new(),
            chests: BTreeMap::new(),
            spawners: BTreeMap::new(),
        };
        let mut random = JavaRandom::new(row["seed"].as_str().unwrap().parse().unwrap());
        for (i, clip) in row["clips"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                piece.populate(&mut world, bbox(&clip[0]), &mut random),
                clip[1].as_bool().unwrap(),
                "case {case} clip {i}"
            );
        }
        assert_eq!(
            random.next_long().to_string(),
            row["next"].as_str().unwrap(),
            "RNG case {case} {:?}",
            piece.kind
        );
        assert_eq!(
            super::tests::nbt(&piece),
            row["after"],
            "persistent flags case {case}"
        );
        let expected = row["blocks"].as_array().unwrap();
        assert_eq!(
            world.blocks.len(),
            expected.len(),
            "block count case {case}"
        );
        for block in expected {
            let p = std::array::from_fn(|i| block[i].as_i64().unwrap() as i32);
            assert_eq!(
                world.blocks.get(&p).copied(),
                Some(block[3].as_u64().unwrap() as u16),
                "case {case} {:?} block {p:?}",
                piece.kind
            );
            count += 1;
        }
        let mut chests: Vec<([i32; 3], Vec<Option<LootStack>>)> =
            serde_json::from_value(row["chests"].clone()).unwrap();
        chests.sort_by_key(|v| v.0);
        assert_eq!(
            world.chests.into_iter().collect::<Vec<_>>(),
            chests,
            "chests case {case}"
        );
        let mut spawners: Vec<([i32; 3], String)> =
            serde_json::from_value(row["spawners"].clone()).unwrap();
        spawners.sort_by_key(|v| v.0);
        assert_eq!(
            world.spawners.into_iter().collect::<Vec<_>>(),
            spawners,
            "spawners case {case}"
        );
        let restored: Piece =
            serde_json::from_str(&serde_json::to_string(&piece).unwrap()).unwrap();
        assert_eq!(json!(restored), json!(piece));
    }
    assert_eq!(count, 105093);
}
