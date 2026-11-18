//! Original EntityPlayerMP constructor position and independent entity RNG.
use super::*;
use crate::vanilla::respawn::{lift, offset_column, top_column};
use glam::DVec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct InitialPlayer {
    pub position: [f64; 3],
    pub random_state: u64,
}
struct Rules {
    spawn_protection: i32,
    border_distance: f64,
    no_sky: bool,
    adventure: bool,
}
impl Runtime {
    fn top_spawn_column(&mut self, x: i32, z: i32) -> Result<i32> {
        let cp = (x >> 4, z >> 4);
        self.provide(cp)?;
        let top_section = self
            .backend
            .light
            .chunks
            .get(&cp)
            .and_then(|c| c.sections.iter().rposition(|present| *present))
            .unwrap_or(0);
        Ok(top_column(
            &self.backend.world,
            IVec3::new(x, 0, z),
            top_section,
        ))
    }
    fn first_player_pose(
        &mut self,
        mut p: IVec3,
        rules: Rules,
        random: &mut JavaRandom,
        mut column: impl FnMut(IVec3, i32),
        overlap: impl FnMut(usize),
    ) -> Result<InitialPlayer> {
        if !rules.no_sky && !rules.adventure {
            let border = rules.border_distance.floor() as i32;
            let radius = rules
                .spawn_protection
                .wrapping_sub(6)
                .max(5)
                .min(border)
                .max(1);
            offset_column(&mut p, radius, random)?;
            let y = self.top_spawn_column(p.x, p.z)?;
            column(p, y);
            p.y = y;
        }
        let position = lift(
            &self.backend.world,
            p.as_dvec3() + DVec3::new(0.5, 0., 0.5),
            255.,
            overlap,
        );
        Ok(InitialPlayer {
            position: position.to_array(),
            random_state: random.state(),
        })
    }
    /// Store the original creation pose separately from the current player
    /// position in the gameplay checkpoint. Retrying consumes no extra RNG.
    pub fn prepare_initial_player(&mut self, adventure: bool) -> Result<InitialPlayer> {
        if let Some(initial) = self.backend.initial_player {
            return Ok(initial);
        }
        let anchor = self.prepare_overworld_spawn()?;
        self.backend.entities.prepare_player_constructor();
        let mut random = JavaRandom::new(JavaRandom::independent_seed());
        // Entity assigns a random UUID before EntityPlayer replaces it with
        // the profile UUID. LivingBase's visual draws use Math.random().
        random.next_long();
        random.next_long();
        let rules = Rules {
            spawn_protection: 16,
            border_distance: 29_999_984. - f64::from(anchor.x.abs().max(anchor.z.abs())),
            no_sky: self.backend.light.no_sky,
            adventure,
        };
        let initial = self.first_player_pose(anchor, rules, &mut random, |_, _| {}, |_| {})?;
        self.backend.initial_player = Some(initial);
        Ok(initial)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    #[test]
    fn original_entityplayermp_576_initial_positions_rng_and_collision_lifts_match() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-player-spawn.json"
        ))
        .unwrap();
        let mut world = Runtime::for_overworld_verification(189, 1234, true);
        let anchor = world.prepare_overworld_spawn().unwrap();
        let mut start = 0;
        while start < 625 {
            start = world.load_initial_spawn_batch(start, 128).unwrap();
        }
        let rows = fixture["players"].as_array().unwrap();
        for (i, row) in rows.iter().enumerate() {
            let mut random = JavaRandom::new(row["seed"].as_str().unwrap().parse().unwrap());
            let mode = row["mode"].as_u64().unwrap();
            let mut top = Vec::new();
            let mut collisions = Vec::new();
            let rules = Rules {
                spawn_protection: 16,
                border_distance: row["distance"].as_f64().unwrap(),
                no_sky: mode == 2,
                adventure: mode == 1,
            };
            let actual = world
                .first_player_pose(
                    anchor,
                    rules,
                    &mut random,
                    |p, y| top = vec![p.x, p.y, p.z, y],
                    |n| collisions.push(n),
                )
                .unwrap();
            assert_eq!(json!(top), row["top"], "case {i} column");
            assert_eq!(
                json!(collisions),
                row["collisions"],
                "case {i} collision lift"
            );
            assert_eq!(json!(actual.position), row["position"], "case {i} feet");
            assert_eq!(
                actual.random_state.to_string(),
                row["rng"].as_str().unwrap(),
                "case {i} entity RNG"
            );
        }
        assert_eq!(rows.len(), 576);
    }
}
