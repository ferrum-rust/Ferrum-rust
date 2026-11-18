//! RandomPositionGenerator's ten weighted samples. Restriction distance,
//! early directional rejection and each RNG draw follow the original order.
use crate::vanilla::random::JavaRandom;
use glam::{DVec3, IVec3};

#[derive(Clone, Copy)]
pub enum Direction {
    Any,
    Towards(DVec3),
    Away(DVec3),
}
#[derive(Clone, Copy)]
pub struct Search {
    pub position: DVec3,
    pub horizontal: i32,
    pub vertical: i32,
    pub home: Option<(IVec3, f32)>,
    pub direction: Direction,
}
pub fn choose(
    random: &mut JavaRandom,
    search: Search,
    mut weight: impl FnMut(IVec3) -> f32,
) -> Option<IVec3> {
    let origin = search.position.floor().as_ivec3();
    let restrict = search.home.is_some_and(|(home, radius)| {
        let limit = f64::from(radius + search.horizontal as f32);
        home.as_dvec3().distance_squared(origin.as_dvec3()) + 4. < limit * limit
    });
    let towards = match search.direction {
        Direction::Any => None,
        Direction::Towards(p) => Some(p - search.position),
        Direction::Away(p) => Some(search.position - p),
    };
    let mut best = None;
    let mut best_weight = -99999.;
    for _ in 0..10 {
        let mut dx = random.next_int(2 * search.horizontal + 1) - search.horizontal;
        let dy = random.next_int(2 * search.vertical + 1) - search.vertical;
        let mut dz = random.next_int(2 * search.horizontal + 1) - search.horizontal;
        if towards.is_some_and(|d| f64::from(dx) * d.x + f64::from(dz) * d.z < 0.) {
            continue;
        }
        if let Some((home, _)) = search.home.filter(|_| search.horizontal > 1) {
            let x = random.next_int(search.horizontal / 2);
            dx += if search.position.x > f64::from(home.x) {
                -x
            } else {
                x
            };
            let z = random.next_int(search.horizontal / 2);
            dz += if search.position.z > f64::from(home.z) {
                -z
            } else {
                z
            };
        }
        let candidate = origin + IVec3::new(dx, dy, dz);
        if restrict
            && search.home.is_some_and(|(h, r)| {
                h.as_dvec3().distance_squared(candidate.as_dvec3()) >= f64::from(r * r)
            })
        {
            continue;
        }
        let score = weight(candidate);
        if score > best_weight {
            best_weight = score;
            best = Some(candidate);
        }
    }
    best
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::io::Read;
    #[test]
    fn random_ai_positions_home_bias_direction_weight_and_rng_match_4096_original_cases() {
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(
            include_bytes!("../tests/fixtures/vanilla189-random-goals.json.gz").as_slice(),
        )
        .read_to_end(&mut bytes)
        .unwrap();
        let data: Value = serde_json::from_slice(&bytes).unwrap();
        for row in data.as_array().unwrap() {
            let n = row[0].as_i64().unwrap();
            let position = serde_json::from_value(row[2].clone()).unwrap();
            let home = &row[3];
            let radius = home[3].as_i64().unwrap();
            let home = (radius >= 0).then(|| {
                (
                    IVec3::new(
                        home[0].as_i64().unwrap() as i32,
                        home[1].as_i64().unwrap() as i32,
                        home[2].as_i64().unwrap() as i32,
                    ),
                    radius as f32,
                )
            });
            let target = serde_json::from_value(row[6].clone()).unwrap();
            let search = Search {
                position,
                home,
                horizontal: row[4].as_i64().unwrap() as i32,
                vertical: row[5].as_i64().unwrap() as i32,
                direction: match n % 3 {
                    0 => Direction::Any,
                    1 => Direction::Towards(target),
                    _ => Direction::Away(target),
                },
            };
            let mut random = JavaRandom::new(row[1].as_str().unwrap().parse().unwrap());
            let mut probes = Vec::new();
            let selected = choose(&mut random, search, |p| {
                probes.push(p);
                match n % 4 {
                    0 => 0.,
                    1 => -100000.,
                    2 => {
                        if p.y % 3 == 0 {
                            10.
                        } else {
                            -0.125
                        }
                    }
                    _ => (p.x * 31 + p.z * 71 + p.y * 7).rem_euclid(17) as f32 * 0.125,
                }
            });
            assert_eq!(
                json!([probes, selected, random.state().to_string()]),
                json!([row[7], row[8], row[9]]),
                "sample{n}"
            );
        }
        assert_eq!(data.as_array().unwrap().len(), 4096);
    }
}
