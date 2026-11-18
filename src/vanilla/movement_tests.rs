use super::*;
use crate::world::Chunk;
use std::{path::PathBuf, sync::Arc};
pub(crate) fn initial(x: i32, y: i32, z: i32, scenario: i32) -> u16 {
    if !(0..256).contains(&y) {
        return 0;
    }
    if y == 0 {
        if scenario == 9 && z < 7 {
            return 0;
        }
        return match scenario {
            14 => 79,
            15 => 88,
            16 => 165,
            _ => 1,
        } << 4;
    }
    if scenario == 4 && y <= 3 {
        return 9 << 4;
    }
    if scenario == 5 && y <= 3 {
        return 11 << 4;
    }
    if scenario == 6 && y == 1 {
        return 8 << 4 | x.rem_euclid(8) as u16;
    }
    if scenario == 7 && x == 8 && z == 7 && y <= 8 {
        return 65 << 4 | 3;
    }
    if scenario == 7 && z == 6 && y <= 8 {
        return 1 << 4;
    }
    if scenario == 8 && x == 8 && z == 7 && y <= 8 {
        return 106 << 4 | 4;
    }
    if scenario == 8 && z == 6 && y <= 8 {
        return 1 << 4;
    }
    if scenario == 10 && z <= 6 && y == 1 {
        return 44 << 4;
    }
    if scenario == 11 && z == 6 && y == 1 {
        return 53 << 4 | 3;
    }
    if scenario == 12 && z == 6 && y < 5 {
        return 1 << 4;
    }
    if scenario == 13 && y == 3 {
        return 1 << 4;
    }
    if scenario == 17 && (5..=8).contains(&z) && y <= 3 {
        return 30 << 4;
    }
    if scenario == 30 && y == 1 {
        return 8 << 4 | (x + 2 * z).rem_euclid(8) as u16;
    }
    if scenario == 31 && x <= 7 && y <= 4 {
        return 1 << 4;
    }
    if scenario == 31 && y <= 3 {
        return 8 << 4 | 8;
    }
    if scenario == 32 && z == 6 && y <= 4 {
        return 1 << 4;
    }
    if scenario == 32 && y <= 3 {
        return 9 << 4;
    }
    if scenario == 33 && x == 8 && z == 8 && y <= 2 {
        return 1 << 4;
    }
    if scenario == 34 && y <= 3 {
        return 9 << 4;
    }
    if scenario == 35 && y <= 3 {
        return 30 << 4;
    }
    0
}
#[test]
fn original_entityplayersp_5040_ticks_match_f64_motion_aabb_and_input_state() {
    compare(
        include_str!("../../tests/fixtures/vanilla189-movement.json"),
        5040,
    );
}
#[test]
fn original_entityplayersp_2520_ticks_match_flight_capabilities_currents_and_chunk_edges() {
    compare(
        include_str!("../../tests/fixtures/vanilla189-movement-extended.json"),
        2520,
    );
}
#[test]
fn original_entityplayersp_1680_ticks_match_speed_slowness_jump_blindness_and_depth_strider() {
    compare(
        include_str!("../../tests/fixtures/vanilla189-movement-potions.json"),
        1680,
    );
}
fn compare(fixture: &str, expected_ticks: usize) {
    let rows: serde_json::Value = serde_json::from_str(fixture).unwrap();
    let mut count = 0;
    let mut failures = Vec::new();
    for row in rows.as_array().unwrap() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let scenario = row["scenario"].as_i64().unwrap() as i32;
            let ox = row["origin"][0].as_i64().unwrap() as i32;
            let oz = row["origin"][1].as_i64().unwrap() as i32;
            let mut chunks = crate::world::ChunkMap::new();
            for cx in -2..=2 {
                for cz in -2..=2 {
                    if matches!(scenario, 34 | 35) && (cx != 0 || cz != 0) {
                        continue;
                    }
                    let mut c = Chunk::empty();
                    for x in 0..16 {
                        for z in 0..16 {
                            for y in 0..32 {
                                c.set(
                                    x,
                                    y,
                                    z,
                                    initial(
                                        cx * 16 + x,
                                        y,
                                        cz * 16 + z,
                                        if scenario == 20 || scenario >= 42 {
                                            4
                                        } else {
                                            scenario
                                        },
                                    ),
                                );
                            }
                        }
                    }
                    chunks.insert(
                        (ox.div_euclid(16) + cx, oz.div_euclid(16) + cz),
                        Arc::new(c),
                    );
                }
            }
            let world = World {
                chunks,
                spawn: Vec3::ZERO,
                label: String::new(),
                save_path: PathBuf::new(),
                edits: Default::default(),
                imported: false,
                light_revision: 0,
            };
            let position = std::array::from_fn(|i| row["position"][i].as_f64().unwrap());
            let mut player = Player::new(Vec3::ZERO);
            player.set_pose_f64(
                position,
                row["yaw"].as_f64().unwrap() as f32,
                0.,
                row["grounded"].as_bool().unwrap(),
            );
            player.flying = row["flying"].as_bool().unwrap();
            player.allow_flying = row["allow_flying"].as_bool().unwrap();
            player.fly_speed = row["fly_speed"].as_f64().unwrap() as f32;
            player.set_velocity_f64(std::array::from_fn(|i| {
                row["velocity"][i].as_f64().unwrap()
            }));
            for (t, tick) in row["ticks"].as_array().unwrap().iter().enumerate() {
                // A native client checkpoint retains prediction history, bounds
                // and attribute buckets. Physical keys come from the next input.
                if t % 17 == 8 {
                    player = serde_json::from_slice(&serde_json::to_vec(&player).unwrap()).unwrap();
                }
                player.keys.clear();
                let input = tick["input"].as_u64().unwrap();
                for (i, key) in [
                    KeyCode::KeyW,
                    KeyCode::KeyS,
                    KeyCode::KeyA,
                    KeyCode::KeyD,
                    KeyCode::Space,
                    KeyCode::ShiftLeft,
                    KeyCode::ControlLeft,
                ]
                .into_iter()
                .enumerate()
                {
                    if input & (1 << i) != 0 {
                        player.keys.insert(key);
                    }
                }
                player.can_sprint = tick["can_sprint"].as_bool().unwrap();
                player.using_item = input & 128 != 0;
                player.speed_boost = tick["effects"][0].as_i64().map(|v| v as i32);
                player.slowness = tick["effects"][1].as_i64().map(|v| v as i32);
                player.jump_boost = tick["effects"][2].as_i64().map(|v| v as i32);
                player.blinded = !tick["effects"][3].is_null();
                player.depth_strider = tick["depth_strider"].as_i64().unwrap() as i32;
                player.tick(&world);
                let m = &player.motion;
                let actual = [
                    m.position.x,
                    m.position.y,
                    m.position.z,
                    m.velocity.x,
                    m.velocity.y,
                    m.velocity.z,
                    m.bounds.min.x,
                    m.bounds.min.y,
                    m.bounds.min.z,
                    m.bounds.max.x,
                    m.bounds.max.y,
                    m.bounds.max.z,
                ];
                for (i, (value, expected)) in actual
                    .into_iter()
                    .zip(tick["bits"].as_array().unwrap())
                    .enumerate()
                {
                    let expected = u64::from_str_radix(expected.as_str().unwrap(), 16).unwrap();
                    assert_eq!(
                    value.to_bits(),
                    expected,
                    "scenario={scenario} yaw={} tick={t} component={i} native={value:?} java={:?}",
                    row["yaw"],
                    f64::from_bits(expected)
                );
                }
                assert_eq!(
                    serde_json::json!([
                        m.grounded,
                        m.sprinting,
                        m.flying,
                        m.in_water,
                        m.on_ladder,
                        m.jumped,
                        m.collided_horizontal
                    ]),
                    tick["flags"],
                    "flags scenario={scenario} yaw={} tick={t}",
                    row["yaw"]
                );
                assert_eq!(
                    m.jump_delay as u64,
                    tick["jump_delay"].as_u64().unwrap(),
                    "jump timer case {scenario} tick{t}"
                );
                assert_eq!(
                    m.sprint_toggle_ticks as u64,
                    tick["sprint_timer"].as_u64().unwrap(),
                    "sprint timer case {scenario} tick{t}"
                );
                assert_eq!(
                    m.air_speed.to_bits() as u64,
                    tick["air_speed"].as_u64().unwrap(),
                    "air acceleration case {scenario} tick{t}"
                );
                assert_eq!(
                    m.fly_toggle_ticks as u64,
                    tick["fly_timer"].as_u64().unwrap(),
                    "flight timer scenario={scenario} tick={t}"
                );
                assert_eq!(
                    serde_json::json!(m.flight_changes),
                    tick["flight_changes"],
                    "flight ability notifications scenario={scenario} tick={t}"
                );
                assert_eq!(
                    m.eyes_in_water,
                    tick["eyes_water"].as_bool().unwrap(),
                    "eyes water scenario={scenario} tick={t}"
                );
                assert_eq!(
                    m.jump_sprinting,
                    tick["jump_sprinting"].as_bool().unwrap(),
                    "jump sprint scenario={scenario} tick={t}"
                );
                count += 1;
            }
        }));
        if let Err(error) = result {
            let message = error.downcast_ref::<String>().cloned().unwrap_or_else(|| {
                error
                    .downcast_ref::<&str>()
                    .copied()
                    .unwrap_or("unknown movement failure")
                    .to_owned()
            });
            failures.push(message);
        }
    }
    assert!(
        failures.is_empty(),
        "{} movement trajectories differed:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(count, expected_ticks);
}
