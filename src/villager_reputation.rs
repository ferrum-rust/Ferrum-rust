//! Village aggressor identity, renewal and player reputation adjustments.
use super::{Homes, Village};
#[cfg(test)]
use glam::DVec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResidentActor {
    pub entity: u64,
    pub player: Option<String>,
}
#[derive(Clone, Debug)]
pub(super) struct Aggressor {
    actor: ResidentActor,
    tick: i32,
}
#[derive(Clone, Copy)]
pub enum Death<'a> {
    Entity {
        actor: &'a ResidentActor,
        hostile: bool,
    },
    Unattributed {
        nearby_player: bool,
    },
}
impl Village {
    pub(super) fn change_score(&mut self, name: &str, delta: i32) {
        let score = self.reputation.entry(name.to_owned()).or_default();
        *score = score.wrapping_add(delta).clamp(-30, 10);
    }
    pub(super) fn expire_aggressors(&mut self, tick: i32, alive: impl Fn(&ResidentActor) -> bool) {
        self.attackers
            .retain(|a| alive(&a.actor) && tick.wrapping_sub(a.tick).wrapping_abs() <= 300);
    }
    fn renew(&mut self, actor: ResidentActor) {
        if let Some(existing) = self.attackers.iter_mut().find(|a| a.actor == actor) {
            existing.tick = self.last_tick;
        } else {
            self.attackers.push(Aggressor {
                actor,
                tick: self.last_tick,
            });
        }
    }
}
impl Homes {
    /// A live villager sends status 13 for a player aggressor, including when
    /// the village's reputation has already reached its minimum.
    pub fn hurt(
        &mut self,
        home: Option<usize>,
        actor: Option<ResidentActor>,
        child: bool,
        alive: bool,
    ) -> bool {
        let Some((v, actor)) = home.and_then(|i| self.villages.get_mut(i)).zip(actor) else {
            return false;
        };
        v.renew(actor.clone());
        if let Some(name) = actor.player {
            v.change_score(&name, if child { -3 } else { -1 });
            return alive;
        }
        false
    }
    pub fn killed(&mut self, home: Option<usize>, cause: Death<'_>) {
        let Some(v) = home.and_then(|i| self.villages.get_mut(i)) else {
            return;
        };
        match cause {
            Death::Entity { actor, .. } if actor.player.is_some() => {
                v.change_score(actor.player.as_ref().unwrap(), -2)
            }
            Death::Entity { hostile: true, .. }
            | Death::Unattributed {
                nearby_player: true,
            } => v.no_breed_since = v.last_tick,
            _ => {}
        }
    }
    #[cfg(test)]
    pub fn nearest_aggressor(
        &self,
        home: usize,
        center: DVec3,
        position: impl Fn(&ResidentActor) -> Option<DVec3>,
    ) -> Option<&ResidentActor> {
        let village = self.villages.get(home)?;
        let mut nearest = None;
        let mut best = f64::MAX;
        for entry in &village.attackers {
            if let Some(distance) = position(&entry.actor).map(|p| p.distance_squared(center)) {
                if distance <= best {
                    nearest = Some(&entry.actor);
                    best = distance;
                }
            }
        }
        nearest
    }
    pub fn hostile_player(&self, home: usize, name: &str) -> bool {
        self.villages
            .get(home)
            .and_then(|v| v.reputation.get(name))
            .is_some_and(|s| *s <= -15)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::io::Read;
    #[test]
    fn village_player_reputation_child_penalty_aggressor_ties_and_deaths_match_original() {
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(
            include_bytes!("../tests/fixtures/vanilla189-village-reputation.json.gz").as_slice(),
        )
        .read_to_end(&mut bytes)
        .unwrap();
        let data: Value = serde_json::from_slice(&bytes).unwrap();
        let actors = [
            ResidentActor {
                entity: 0,
                player: Some("Player".into()),
            },
            ResidentActor {
                entity: 1,
                player: None,
            },
            ResidentActor {
                entity: 2,
                player: None,
            },
        ];
        for row in data.as_array().unwrap() {
            let home = row[1].as_bool().unwrap().then_some(0);
            let mut homes = Homes {
                villages: vec![Village {
                    last_tick: 147,
                    ..Default::default()
                }],
                ..Default::default()
            };
            homes.villages[0].change_score("Player", row[4].as_i64().unwrap() as i32);
            let player_x = row[5].as_f64().unwrap();
            let snapshot = |homes: &Homes, status: Vec<i32>| {
                let nearest = homes.nearest_aggressor(0, DVec3::Y, |key| {
                    let i = actors.iter().position(|a| a == key)?;
                    Some(DVec3::new([player_x, -3., 3.][i], 1., 0.))
                });
                let index = nearest
                    .and_then(|key| actors.iter().position(|a| a == key))
                    .map_or(-1, |n| n as i32);
                json!([
                    homes.villages[0].reputation["Player"],
                    homes.villages[0].no_breed_since,
                    index,
                    status
                ])
            };
            for frame in row[6].as_array().unwrap() {
                let i = frame[0].as_u64().unwrap() as usize;
                let status = homes.hurt(
                    home,
                    actors.get(i).cloned(),
                    row[2].as_bool().unwrap(),
                    row[3].as_bool().unwrap(),
                );
                assert_eq!(
                    snapshot(&homes, if status { vec![13] } else { Vec::new() }),
                    frame[1]
                );
            }
            homes.killed(
                home,
                match row[7].as_i64().unwrap() {
                    0 => Death::Entity {
                        actor: &actors[0],
                        hostile: false,
                    },
                    1 => Death::Entity {
                        actor: &actors[2],
                        hostile: true,
                    },
                    2 => Death::Unattributed {
                        nearby_player: player_x * player_x < 256.,
                    },
                    _ => Death::Entity {
                        actor: &actors[1],
                        hostile: false,
                    },
                },
            );
            assert_eq!(snapshot(&homes, vec![3]), row[8]);
            assert_eq!(
                homes.hostile_player(0, "Player"),
                row[8][0].as_i64().unwrap() <= -15
            );
        }
        assert_eq!(data.as_array().unwrap().len(), 192);
    }
}
