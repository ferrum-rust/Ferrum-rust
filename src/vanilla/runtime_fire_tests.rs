use super::*;
use crate::world::Chunk;
use serde_json::{json, Value};
use std::io::Read;
struct Flat;
impl Hooks for Flat {
    fn populate(&self, _: &mut Access<'_>, _: ChunkPos, _: &mut JavaRandom) -> Result<()> {
        Ok(())
    }
}
fn base() -> RuntimeSnapshot {
    let mut r = Runtime::new(189, 0, false, Arc::new(Flat));
    for x in -1..=2 {
        for z in -1..=2 {
            r.provide((x, z)).unwrap();
        }
    }
    let mut flat = Chunk::empty();
    flat.blocks[..256].fill(7 << 4);
    flat.top = 1;
    for c in r.backend.world.chunks.values_mut() {
        *c = Arc::new(flat.clone());
    }
    r.backend.light = Lighting::new(&r.backend.world, false);
    r.checkpoint().unwrap()
}
fn observed(r: &Runtime) -> Value {
    let mut blocks = Vec::new();
    for x in 5..=32 {
        for z in 5..=32 {
            for y in 3..=28 {
                let s = r.backend.world.get(IVec3::new(x, y, z));
                if matches!(s >> 4, 51 | 90) {
                    blocks.push(json!([x, y, z, s]));
                }
            }
        }
    }
    let mut queue = Vec::new();
    let ticks = serde_json::to_value(&r.backend.ticks).unwrap();
    for (time, entries) in ticks["queue"].as_object().unwrap() {
        for e in entries.as_array().unwrap() {
            if e["kind"] == 51 {
                let p = &e["position"];
                queue.push(json!([
                    p[0],
                    p[1],
                    p[2],
                    time.parse::<u64>().unwrap(),
                    e["priority"]
                ]));
            }
        }
    }
    json!([blocks, queue, r.backend.world_random.state().to_string()])
}
#[test]
fn original_fire_support_portal_frame_fill_teardown_and_tick_queue_match() {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("../../tests/fixtures/vanilla189-fire-portal.json.gz")[..],
    )
    .read_to_end(&mut bytes)
    .unwrap();
    let cases: Value = serde_json::from_slice(&bytes).unwrap();
    let start = base();
    for row in cases.as_array().unwrap() {
        let mut r = Runtime::resume(start.clone(), Arc::new(Flat)).unwrap();
        for edit in row["edits"].as_array().unwrap() {
            let [x, y, z, s]: [i32; 4] = serde_json::from_value(edit.clone()).unwrap();
            r.access()
                .set_block_flags(IVec3::new(x, y, z), s as u16, 2)
                .unwrap();
        }
        r.backend.world_random.set_seed(731);
        r.access()
            .set_block_flags(IVec3::new(8, 5, 8), 51 << 4, 3)
            .unwrap();
        assert_eq!(
            observed(&r),
            row["frames"][0],
            "original start {:?}",
            row["description"]
        );
        let cut: [i32; 3] = serde_json::from_value(row["cut"].clone()).unwrap();
        r.access()
            .set_block_flags(IVec3::from_array(cut), 0, 3)
            .unwrap();
        assert_eq!(
            observed(&r),
            row["frames"][1],
            "original teardown {:?}",
            row["description"]
        );
    }
}

fn scheduled_observation(runtime: &Runtime, base: i32) -> Value {
    let mut hash = 0xcbf29ce484222325_u64;
    for x in 0..16 {
        for z in 0..16 {
            for y in base - 2..=base + 8 {
                hash ^= u64::from(runtime.backend.world.get(IVec3::new(x, y, z)));
                hash = hash.wrapping_mul(0x100000001b3);
            }
        }
    }
    let ticks = serde_json::to_value(&runtime.backend.ticks).unwrap();
    let mut times: Vec<_> = ticks["queue"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(time, entries)| (time.parse::<u64>().unwrap(), entries))
        .collect();
    times.sort_by_key(|(time, _)| *time);
    let mut queue = Vec::new();
    for (time, entries) in times {
        for entry in entries.as_array().unwrap() {
            let p = &entry["position"];
            queue.push(json!([
                p[0],
                p[1],
                p[2],
                entry["kind"],
                time,
                entry["priority"]
            ]));
        }
    }
    let primed = runtime
        .backend
        .pending_tnt
        .iter()
        .map(|body| {
            let bits = body
                .position
                .to_array()
                .into_iter()
                .chain(body.velocity.to_array())
                .map(|n| format!("{:x}", n.to_bits()))
                .collect::<Vec<_>>();
            json!([bits, body.fuse])
        })
        .collect::<Vec<_>>();
    json!([
        hash.to_string(),
        queue,
        runtime.backend.world_random.state().to_string(),
        primed
    ])
}

#[test]
fn original_scheduled_fire_spread_weather_rng_queue_and_resume_match() {
    let data: Value = serde_json::from_reader(flate2::read::GzDecoder::new(
        &include_bytes!("../../tests/fixtures/vanilla189-fire-ticks.json.gz")[..],
    ))
    .unwrap();
    let start = base();
    let mut cases = 0;
    let mut frames = 0;
    let mut tnt_cases = 0;
    for row in data["cases"].as_array().unwrap() {
        let id = row["index"].as_i64().unwrap();
        let mut runtime = Runtime::resume(start.clone(), Arc::new(Flat)).unwrap();
        runtime.backend.fire_environment = FireEnvironment {
            do_fire_tick: row["enabled"].as_bool().unwrap(),
            difficulty: row["difficulty"].as_u64().unwrap() as u8,
            raining: row["rain"].as_bool().unwrap(),
        };
        let base = row["base"].as_i64().unwrap() as i32;
        for chunk in runtime.backend.world.chunks.values_mut() {
            Arc::make_mut(chunk)
                .biomes
                .fill(row["biome"].as_u64().unwrap() as u8);
        }
        runtime.backend.world_random.set_seed(1889 + id);
        runtime.backend.entities.math_state = Some(JavaRandom::new(8189 + id).state());
        for edit in row["edits"].as_array().unwrap() {
            let [x, y, z, s]: [i32; 4] = serde_json::from_value(edit.clone()).unwrap();
            runtime
                .access()
                .set_block_flags(IVec3::new(x, y, z), s as u16, 2)
                .unwrap();
        }
        let mut now = 0;
        for frame in row["frames"].as_array().unwrap() {
            let target = frame[0].as_i64().unwrap();
            while now < target {
                runtime.tick_blocks_and_falling().unwrap();
                now += 1;
                if now == 55 {
                    runtime =
                        Runtime::resume(runtime.checkpoint().unwrap(), Arc::new(Flat)).unwrap();
                }
            }
            assert_eq!(
                scheduled_observation(&runtime, base),
                frame[1],
                "scene {id}, tick {now}"
            );
            assert_eq!(
                runtime.backend.entities.math_state.unwrap().to_string(),
                frame[2].as_str().unwrap(),
                "primed TNT Math.random, scene {id}, tick {now}"
            );
            frames += 1;
        }
        if !runtime.backend.pending_tnt.is_empty() {
            let before = scheduled_observation(&runtime, base);
            let time = runtime.provider.world_time;
            runtime = Runtime::resume(runtime.checkpoint().unwrap(), Arc::new(Flat)).unwrap();
            assert!(!runtime.tick_blocks_and_falling().unwrap());
            assert_eq!(time, runtime.provider.world_time);
            assert_eq!(before, scheduled_observation(&runtime, base));
            tnt_cases += 1;
        }
        cases += 1;
    }
    assert_eq!((cases, frames, tnt_cases), (112, 1640, 15));
}
