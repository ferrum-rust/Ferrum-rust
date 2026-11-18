use super::*;
use serde_json::{json, Value};
use std::{collections::HashMap, io::Read};

struct Fixture {
    scenario: i32,
    tick: usize,
    edits: HashMap<IVec3, u16>,
    events: Vec<Value>,
}
impl MovementWorld for Fixture {
    fn get(&self, p: IVec3) -> u16 {
        if let Some(state) = self.edits.get(&p) {
            return *state;
        }
        let s = self.scenario;
        if !(0..256).contains(&p.y) || matches!(s, 12 | 14 | 20) {
            return 0;
        }
        if p.y == 0 {
            return match s {
                2 => 44,
                7 => 88,
                8 => 165,
                9 => 85,
                _ => 1,
            } << 4;
        }
        if matches!(s, 1 | 3 | 18) && p.y == 1 {
            return 50 << 4 | 5;
        }
        if s == 4 && p.y <= 3 {
            return 9 << 4;
        }
        if matches!(s, 5 | 17) && p.y <= 3 {
            return 11 << 4;
        }
        if s == 6 && (2..=4).contains(&p.y) {
            return 30 << 4;
        }
        if s == 10 && p.y == 1 {
            return 36 << 4;
        }
        if s == 19 && p.x == 10 && p.y < 15 {
            return 1 << 4;
        }
        0
    }
    fn loaded_column(&self, _: i32, _: i32) -> bool {
        true
    }
    fn block_boxes(&self, p: IVec3, state: u16, out: &mut Vec<Aabb>) {
        let height = match state >> 4 {
            1 | 12 | 13 | 145 | 165 => 1.,
            44 => 0.5,
            88 => 0.875,
            85 => 1.5,
            _ => return,
        };
        out.push(Aabb {
            min: p.as_dvec3(),
            max: p.as_dvec3() + DVec3::new(1., height, 1.),
        });
    }
}
impl FallingWorld for Fixture {
    fn is_remote(&self) -> bool {
        self.scenario == 15
    }
    fn do_entity_drops(&self) -> bool {
        self.scenario != 18
    }
    fn set_block(&mut self, p: IVec3, state: u16, flags: i32) -> Result<bool> {
        let ok = self.scenario != 16 || state == 0;
        self.events
            .push(json!(["set", p.x, p.y, p.z, state, flags, ok]));
        if ok {
            self.edits.insert(p, state);
        }
        Ok(ok)
    }
    fn can_place(&mut self, p: IVec3, state: u16) -> Result<bool> {
        Ok(super::super::material::replaceable(self.get(p))
            || state >> 4 == 145 && self.get(p) >> 4 == 50)
    }
    fn placed(&mut self, p: IVec3, state: u16, _: Option<&DropNbt>) -> Result<()> {
        if state >> 4 == 145 {
            self.events.push(json!(["effect", 1022, p.x, p.y, p.z, 0]));
        }
        Ok(())
    }
    fn drop_item(&mut self, position: DVec3, state: u16) -> Result<()> {
        let id = state >> 4;
        let meta = if id == 145 {
            (state & 15) >> 2
        } else if id == 12 {
            state & 15
        } else {
            0
        };
        self.events
            .push(json!(["drop", bits(position.to_array()), id, 1, meta, 10]));
        Ok(())
    }
    fn damage_entities(&mut self, bounds: Aabb, anvil: bool, amount: f32) -> Result<()> {
        if bounds.intersects(Aabb {
            min: DVec3::new(-100., 0., -100.),
            max: DVec3::new(100., 4., 100.),
        }) {
            for id in [1, 2] {
                self.events.push(json!([
                    "damage",
                    id,
                    if anvil { "anvil" } else { "fallingBlock" },
                    amount.to_bits()
                ]));
            }
        }
        Ok(())
    }
    fn contact(&mut self, _: IVec3, _: u16, _: &mut Falling) -> Result<()> {
        Ok(())
    }
    fn raining_at(&self, _: IVec3) -> bool {
        self.scenario == 17 && self.tick > 4
    }
    fn sound(&mut self, name: &str, volume: f32, pitch: f32) -> Result<()> {
        self.events
            .push(json!(["sound", name, volume.to_bits(), pitch.to_bits()]));
        Ok(())
    }
}
fn bits(values: impl IntoIterator<Item = f64>) -> Vec<String> {
    values
        .into_iter()
        .map(|v| format!("{:x}", v.to_bits()))
        .collect()
}
fn snapshot(entity: &Falling, world: &mut Fixture) -> Value {
    let b = entity.bounds;
    let mut values = Vec::new();
    for v in [
        entity.position,
        entity.velocity,
        b.min,
        b.max,
        entity.previous,
    ] {
        values.extend(v.to_array());
    }
    json!({
        "bits":bits(values),
        "flags":[entity.grounded,entity.collided_horizontal,entity.collided_vertical,entity.dead,entity.in_web,entity.cannot_place],
        "age":entity.age,"fall_distance":entity.fall_distance.to_bits(),"state":entity.state,
        "fire":entity.fire,"random":entity.random_state.to_string(),"events":std::mem::take(&mut world.events)
    })
}
#[test]
fn original_falling_entity_52353_ticks_collision_anvil_drops_and_resume_match() {
    let mut text = String::new();
    flate2::read::GzDecoder::new(
        include_bytes!("../../tests/fixtures/vanilla189-falling-entity.json.gz").as_slice(),
    )
    .read_to_string(&mut text)
    .unwrap();
    let rows: Vec<Value> = serde_json::from_str(&text).unwrap();
    let mut count = 0;
    for (index, row) in rows.iter().enumerate() {
        let scenario = row["scenario"].as_i64().unwrap() as i32;
        let motion = row["motion"].as_i64().unwrap();
        let initial = &row["initial"];
        let raw: Vec<f64> = initial["bits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| f64::from_bits(u64::from_str_radix(v.as_str().unwrap(), 16).unwrap()))
            .collect();
        let state = initial["state"].as_u64().unwrap() as u16;
        let random = JavaRandom::new(row["seed"].as_str().unwrap().parse().unwrap());
        let mut entity = Falling::new(state, DVec3::from_slice(&raw[..3]), random.state());
        entity.velocity = DVec3::from_slice(&raw[3..6]);
        entity.age = initial["age"].as_i64().unwrap() as i32;
        entity.no_clip = scenario == 20;
        entity.should_drop_item = motion != 1;
        entity.hurt_entities = state >> 4 == 145 || motion == 2;
        let mut world = Fixture {
            scenario,
            tick: 0,
            edits: HashMap::new(),
            events: Vec::new(),
        };
        if !matches!(scenario, 11 | 15) {
            world
                .edits
                .insert(entity.position.floor().as_ivec3(), state);
        }
        assert_eq!(snapshot(&entity, &mut world), *initial, "initial {index}");
        for (tick, expected) in row["ticks"].as_array().unwrap().iter().enumerate() {
            world.tick = tick;
            entity.tick(&mut world).unwrap();
            assert_eq!(
                snapshot(&entity, &mut world),
                *expected,
                "case {index}, scenario {scenario}, motion {motion}, tick {tick}"
            );
            if tick % 17 == 8 {
                entity = serde_json::from_slice(&serde_json::to_vec(&entity).unwrap()).unwrap();
            }
            count += 1;
        }
    }
    assert_eq!(rows.len(), 504);
    assert_eq!(count, 52_353);
}

#[test]
fn original_falling_block_1008_loaded_instant_anvil_default_state_cases_match() {
    use crate::block_ticks::{BlockAccess, BlockTicks};
    struct Trigger {
        y: i32,
        mode: i32,
        support: i32,
        edits: HashMap<IVec3, u16>,
        events: Vec<Value>,
    }
    impl BlockAccess for Trigger {
        fn get(&self, p: IVec3) -> u16 {
            if !(0..256).contains(&p.y) {
                return 0;
            }
            if let Some(s) = self.edits.get(&p) {
                return *s;
            }
            match self.support {
                0 if p.y == 0 => 1 << 4,
                1 if p.y == 10 => 1 << 4,
                2 if p.y == self.y - 1 => 1 << 4,
                3 if p.y == self.y - 1 => 9 << 4,
                4 if p.y == self.y - 1 => 11 << 4,
                3 | 4 if p.y == self.y - 3 => 1 << 4,
                5 if p.y == self.y - 2 => 31 << 4 | 1,
                _ => 0,
            }
        }
        fn chunk_loaded(&self, (x, z): crate::world::ChunkPos) -> bool {
            self.mode & 1 == 0 || x != -2 || z != -2
        }
        fn set_before_added(&mut self, p: IVec3, state: u16) -> bool {
            self.edits.insert(p, state);
            self.events.push(json!(["set", p.x, p.y, p.z, state]));
            true
        }
        fn spawn_falling(&mut self, p: IVec3, state: u16) -> bool {
            self.events.push(json!([
                "spawn",
                bits((p.as_dvec3() + DVec3::new(0.5, 0., 0.5)).to_array()),
                state,
                state >> 4 == 145
            ]));
            true
        }
    }
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-falling-block.json"
    ))
    .unwrap();
    for (i, row) in rows.iter().enumerate() {
        let state = row["state"].as_u64().unwrap() as u16;
        let y = row["y"].as_i64().unwrap() as i32;
        let p = IVec3::new(8, y, 8);
        let mut world = Trigger {
            y,
            mode: row["mode"].as_i64().unwrap() as i32,
            support: row["support"].as_i64().unwrap() as i32,
            edits: HashMap::from([(p, state)]),
            events: Vec::new(),
        };
        let mut ticks = BlockTicks::default();
        ticks.set_fall_instantly(world.mode & 2 != 0);
        ticks.schedule_block_update(p, state >> 4, 0, 0);
        ticks.tick_access(&mut world).unwrap();
        assert_eq!(json!(world.events), row["events"], "case {i}: {row}");
    }
    assert_eq!(rows.len(), 1008);
}

#[test]
fn original_can_place_gravity_on_every_canonical_target_matches() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-falling-placement.json"
    ))
    .unwrap();
    for row in &rows {
        let target = row[0].as_u64().unwrap() as u16;
        for (i, id) in [12, 13, 145].into_iter().enumerate() {
            assert_eq!(
                can_place_gravity(id << 4, target),
                row[i + 1].as_bool(),
                "block {id} on {target}"
            );
        }
    }
    assert_eq!(rows.len(), 1394);
}
