//! Block.onBlockClicked specializations and the positions selected by EffectRenderer.
use super::{
    random::JavaRandom,
    ray_trace::{self, Access},
};
use glam::{DVec3, IVec3};
include!("special_click_data.rs");

#[derive(Clone, Copy, Debug)]
pub enum Effect {
    Set(IVec3, u16, u8),
    Particle(u8, DVec3, DVec3),
    Note(u8, u8),
    EatCake,
}
const DIRECTIONS: [IVec3; 6] = [
    IVec3::NEG_Y,
    IVec3::Y,
    IVec3::NEG_Z,
    IVec3::Z,
    IVec3::NEG_X,
    IVec3::X,
];
pub fn click(
    world: &impl Access,
    p: IVec3,
    remote: bool,
    can_eat: bool,
    note: u8,
    random: &mut JavaRandom,
) -> Vec<Effect> {
    let state = world.get(p);
    let mut effects = vec![];
    match state >> 4 {
        25 if !remote => {
            if let Some((instrument, pitch)) = super::notes::trigger(world, p, note) {
                effects.push(Effect::Note(instrument, pitch));
            }
        }
        73 | 74 => {
            for (i, axis) in [
                IVec3::Y,
                IVec3::NEG_Y,
                IVec3::Z,
                IVec3::NEG_Z,
                IVec3::X,
                IVec3::NEG_X,
            ]
            .into_iter()
            .enumerate()
            {
                let mut q = DVec3::new(
                    f64::from(p.x as f32 + random.next_float()),
                    f64::from(p.y as f32 + random.next_float()),
                    f64::from(p.z as f32 + random.next_float()),
                );
                if !super::material::opaque_cube(world.get(p + axis)) {
                    let coord = [1, 1, 2, 2, 0, 0][i];
                    q[coord] = f64::from(p[coord]) + if i % 2 == 0 { 1.0625 } else { -0.0625 };
                }
                let b = p.as_dvec3();
                // The original lower Y comparison is against zero.
                if q.x < b.x
                    || q.x > b.x + 1.
                    || q.y < 0.
                    || q.y > b.y + 1.
                    || q.z < b.z
                    || q.z > b.z + 1.
                {
                    effects.push(Effect::Particle(30, q, DVec3::ZERO));
                }
            }
            if state >> 4 == 73 {
                effects.push(Effect::Set(p, 74 << 4, 3));
            }
        }
        92 if can_eat => {
            effects.push(Effect::EatCake);
            effects.push(Effect::Set(
                p,
                if state & 15 < 6 { state + 1 } else { 0 },
                3,
            ));
        }
        122 => {
            for _ in 0..1000 {
                let pos = p + IVec3::new(
                    random.next_int(16) - random.next_int(16),
                    random.next_int(8) - random.next_int(8),
                    random.next_int(16) - random.next_int(16),
                );
                if world.get(pos) >> 4 != 0 {
                    continue;
                }
                if remote {
                    for _ in 0..128 {
                        let d = random.next_double();
                        let velocity = DVec3::new(
                            f64::from((random.next_float() - 0.5) * 0.2),
                            f64::from((random.next_float() - 0.5) * 0.2),
                            f64::from((random.next_float() - 0.5) * 0.2),
                        );
                        let source = pos.as_dvec3() + (p - pos).as_dvec3() * d;
                        let point = DVec3::new(
                            source.x + (random.next_double() - 0.5) + 0.5,
                            source.y + random.next_double() - 0.5,
                            source.z + (random.next_double() - 0.5) + 0.5,
                        );
                        effects.push(Effect::Particle(24, point, velocity));
                    }
                } else {
                    effects.push(Effect::Set(pos, state, 2));
                    effects.push(Effect::Set(p, 0, 3));
                }
                break;
            }
        }
        _ => {}
    }
    effects
}
pub(super) fn instrument(state: u16) -> u8 {
    INSTRUMENTS
        .get(usize::from(state >> 4))
        .copied()
        .unwrap_or(0)
}
pub fn note_sound(instrument: u8, note: u8) -> (&'static str, f32, f32) {
    let files = [
        "note.harp",
        "note.bd",
        "note.snare",
        "note.hat",
        "note.bassattack",
    ];
    let pitch = 2.0f64.powf((f64::from(note) - 12.) / 12.) as f32;
    (
        files
            .get(usize::from(instrument))
            .copied()
            .unwrap_or(files[0]),
        3.,
        pitch,
    )
}
pub fn note_particle(p: IVec3, note: u8) -> Effect {
    Effect::Particle(
        23,
        p.as_dvec3() + DVec3::new(0.5, 1.2, 0.5),
        DVec3::new(f64::from(note) / 24., 0., 0.),
    )
}
pub fn hit_origin(
    world: &impl Access,
    p: IVec3,
    state: u16,
    face: u8,
    rand: &mut JavaRandom,
) -> Option<DVec3> {
    let i = usize::from(state);
    if HAS_HIT.get(i / 8).copied().unwrap_or(0) & (1 << (i & 7)) == 0 {
        return None;
    }
    let bounds = ray_trace::selection_bounds(world, p, state);
    let margin = f64::from(0.1f32);
    let size = f64::from(0.1f32 * 2.);
    let mut q = DVec3::ZERO;
    for axis in 0..3 {
        q[axis] = f64::from(p[axis])
            + rand.next_double() * (bounds[axis + 3] - bounds[axis] - size)
            + margin
            + bounds[axis];
    }
    let normal = DIRECTIONS[usize::from(face)];
    let axis = if normal.x != 0 {
        0
    } else if normal.y != 0 {
        1
    } else {
        2
    };
    q[axis] = if normal[axis] < 0 {
        f64::from(p[axis]) + bounds[axis] - margin
    } else {
        f64::from(p[axis]) + bounds[axis + 3] + margin
    };
    Some(q)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::vanilla::food::FoodState;
    use flate2::read::GzDecoder;
    use serde_json::{json, Value};

    fn corpus() -> Value {
        serde_json::from_reader(GzDecoder::new(
            &include_bytes!("../../tests/fixtures/vanilla189-special-click.json.gz")[..],
        ))
        .unwrap()
    }
    struct Scene {
        pos: IVec3,
        state: u16,
        mask: i32,
        below: u16,
    }
    impl Access for Scene {
        fn get(&self, pos: IVec3) -> u16 {
            let d = pos - self.pos;
            if d == IVec3::ZERO {
                return self.state;
            }
            if self.state >> 4 == 122 {
                return u16::from(
                    self.mask == 31 || (31 * d.x + 13 * d.y + 7 * d.z) & self.mask != 0,
                ) << 4;
            }
            if self.state >> 4 == 25 && d == IVec3::NEG_Y {
                return self.below << 4;
            }
            DIRECTIONS
                .iter()
                .position(|&axis| axis == d)
                .map_or(0, |i| u16::from(self.mask & (1 << i) != 0) << 4)
        }
    }
    fn bits(value: f64) -> String {
        format!("{:x}", value.to_bits())
    }

    #[test]
    fn original_special_click_callbacks_particle_arguments_food_and_world_random_match() {
        let data = corpus();
        let cases = data["clicks"].as_array().unwrap();
        assert_eq!(cases.len(), 908);
        // SpecialClickOracle uses BLOCK_STATE_IDS.getByValue(raw). An unmapped
        // (id << 4), e.g. a directionless furnace, was read as air by its World.
        let registry: std::collections::HashSet<_> = data["hits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row[0].as_u64().unwrap() as u16)
            .collect();
        for r in cases {
            let state = r[1].as_u64().unwrap() as u16;
            let i = r[2].as_i64().unwrap() as i32;
            let pos = if i % 3 == 0 {
                IVec3::new(16000003, 63, -16000005)
            } else {
                IVec3::new(-3, 63, 19)
            };
            let world = Scene {
                pos,
                state,
                mask: if state >> 4 == 122 {
                    [0, 1, 3, 7, 15, 31][i as usize % 6]
                } else {
                    i & 63
                },
                below: if registry.contains(&(((i % 198) as u16) << 4)) {
                    (i % 198) as u16
                } else {
                    0
                },
            };
            let mut rand = JavaRandom::new(r[3].as_i64().unwrap());
            let mut food = FoodState {
                level: 16 + i % 5,
                saturation: 1.25,
                ..Default::default()
            };
            let mut events = Vec::new();
            let mut particles = Vec::new();
            let particle =
                |id, p: DVec3, v: DVec3| json!([id, ([p.x, p.y, p.z, v.x, v.y, v.z].map(bits))]);
            for effect in click(
                &world,
                pos,
                r[0].as_bool().unwrap(),
                i % 4 != 0 && food.level < 20,
                (i % 25) as u8,
                &mut rand,
            ) {
                match effect {
                    Effect::Set(p, value, flags) => {
                        events.push(json!(["set", p.x, p.y, p.z, value, flags]))
                    }
                    Effect::EatCake => food.add_stats(2, 0.1),
                    Effect::Particle(id, p, v) => particles.push(particle(id, p, v)),
                    Effect::Note(id, note) => {
                        let (sound, volume, pitch) = note_sound(id, note);
                        events.push(json!(["note", id, note]));
                        events.push(json!([
                            "sound",
                            sound.replace('/', "."),
                            volume.to_bits(),
                            pitch.to_bits()
                        ]));
                        if let Effect::Particle(id, p, v) = note_particle(pos, note) {
                            particles.push(particle(id, p, v));
                        }
                    }
                }
            }
            let label = format!("state {state} scene {i} client {}", r[0]);
            assert_eq!(json!(events), r[4], "{label} events");
            assert_eq!(json!(particles), r[6], "{label} particle arguments");
            assert_eq!(rand.state(), r[7].as_u64().unwrap(), "{label} RNG");
            assert_eq!(
                json!([food.level, food.saturation.to_bits()]),
                json!([r[8], r[9]]),
                "{label} cake"
            );
        }
    }
    #[test]
    fn original_hit_particle_origins_all_block_states_and_six_faces_match() {
        let data = corpus();
        let cases = data["hits"].as_array().unwrap();
        assert_eq!(cases.len() * 6, 8364);
        for r in cases {
            let state = r[0].as_u64().unwrap() as u16;
            let pos = IVec3::new(16000003, 63, -16000005);
            let world = Scene {
                pos,
                state,
                mask: 0,
                below: 0,
            };
            for face in 0..6 {
                let mut rand = JavaRandom::new(i64::from(state) * 189 + i64::from(face));
                let result =
                    hit_origin(&world, pos, state, face, &mut rand).map(|v| v.to_array().map(bits));
                assert_eq!(
                    json!(result),
                    r[1][usize::from(face)],
                    "state {state}, face {face}"
                );
            }
        }
    }
}
