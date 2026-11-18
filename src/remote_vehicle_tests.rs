use super::*;
struct Terrain(i32);
impl MovementWorld for Terrain {
    fn get(&self, p: IVec3) -> u16 {
        if p.y == 0 || (self.0 == 12 && p.z == 6 && (1..5).contains(&p.y)) {
            16
        } else if self.0 == 4 && (1..=3).contains(&p.y) {
            9 << 4
        } else if self.0 == 6 && p.y == 1 {
            (8 << 4) + p.x.rem_euclid(8) as u16
        } else {
            0
        }
    }
    fn block_boxes(&self, p: IVec3, s: u16, out: &mut Vec<Aabb>) {
        if s >> 4 == 1 {
            out.push(Aabb {
                min: p.as_dvec3(),
                max: p.as_dvec3() + DVec3::ONE,
            })
        }
    }
    fn loaded_column(&self, _: i32, _: i32) -> bool {
        true
    }
}
type Tick = (
    usize,
    [f32; 3],
    Option<[f64; 5]>,
    bool,
    i32,
    [f64; 7],
    [f64; 3],
);
type Trace = (usize, i32, f64, Vec<Tick>);
#[test]
fn original_client_boat_cart_horse_and_rider_315_ticks() {
    let cases: Vec<Trace> = serde_json::from_str(include_str!(
        "../tests/fixtures/vanilla189-remote-vehicles.json"
    ))
    .unwrap();
    for (case, terrain, height, ticks) in cases {
        let (kind, ridden) = match case {
            0..=2 => (Kind::Boat, true),
            3 => (Kind::Boat, false),
            4 => (Kind::Cart, true),
            _ => (Kind::Horse, true),
        };
        let mut target = Pose {
            x: 8.5,
            y: height,
            z: 8.5,
            yaw: 0.,
            pitch: 0.,
            on_ground: false,
        };
        let mut at = DVec3::new(8.5, height, 8.5);
        let mut v = Vehicle::new(kind, target);
        let mut steps = 0;
        let mut rider = Rider::default();
        for (i, [yaw, strafe, forward], correct, jump, power, expected, original_seat) in ticks {
            if i == 5 {
                v.velocity([0.11, 0.06, -0.07]);
            }
            if let Some(p) = correct {
                target = Pose {
                    x: p[0],
                    y: p[1],
                    z: p[2],
                    yaw: p[3] as f32,
                    pitch: 0.,
                    on_ground: false,
                };
                v.correction(&mut at, target, i == 32, ridden, &mut steps);
            }
            if kind == Kind::Horse {
                v.horse_flags = 4 | if case == 6 && (10..24).contains(&i) {
                    64
                } else {
                    0
                };
            }
            let input = RiderInput {
                strafe,
                forward,
                yaw,
            };
            v.tick(
                &Terrain(terrain),
                &mut at,
                target,
                &mut steps,
                ridden.then_some(input),
            );
            let seat = v.seat(at);
            let values = [
                at.x,
                at.y,
                at.z,
                v.motion.x,
                v.motion.y,
                v.motion.z,
                f64::from(v.yaw),
                seat.x,
                seat.y,
                seat.z,
            ];
            for (k, (&actual, original)) in values
                .iter()
                .zip(expected.into_iter().chain(original_seat))
                .enumerate()
            {
                assert!(
                    (actual - original).abs() < 2e-6,
                    "case {case}, tick {i}, field {k}: original={original} Rust={actual}"
                );
            }
            if kind == Kind::Horse {
                assert_eq!(
                    rider.jump(true, jump).unwrap_or(-1),
                    power,
                    "horse jump at {i}"
                );
            }
        }
    }
}
