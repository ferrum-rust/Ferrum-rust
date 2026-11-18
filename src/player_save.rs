//! Persistent player pose for original overworld imports and local overlays.
use crate::{gameplay::Mode, nbt::Tag, player::Player};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Kinematics {
    velocity: [f64; 3],
    rotation: [f32; 2],
    grounded: bool,
    flying: bool,
    allow_flying: bool,
    fly_speed: f32,
    walk_speed: f32,
    pub creative: bool,
    #[serde(default)]
    pub dimension: i32,
}
#[derive(Clone, Copy, Debug)]
pub struct PlayerSave {
    pub position: [f64; 3],
    pub motion: Option<Kinematics>,
}
pub fn valid_position([x, y, z]: [f64; 3]) -> bool {
    (-30_000_000.0..30_000_000.0).contains(&x)
        && y.is_finite()
        && (-30_000_000.0..30_000_000.0).contains(&z)
}
pub(crate) fn nbt_integer(tag: Option<&Tag>) -> Option<i32> {
    let numeric = tag.filter(|t| t.number().is_some())?;
    Some(crate::vanilla::food::nbt_int(Some(numeric)))
}
pub(crate) fn nbt_boolean(tag: Option<&Tag>) -> bool {
    nbt_integer(tag).unwrap_or(0) as i8 != 0
}
impl PlayerSave {
    pub fn capture(player: &Player, mode: Mode) -> Self {
        Self::capture_in_dimension(player, mode, 0)
    }
    #[allow(dead_code)]
    pub fn capture_in_dimension(player: &Player, mode: Mode, dimension: i32) -> Self {
        Self {
            position: player.precise_position(),
            motion: Some(Kinematics {
                velocity: player.precise_velocity(),
                rotation: player.java_rotation(),
                grounded: player.grounded,
                flying: player.flying,
                allow_flying: player.allow_flying,
                fly_speed: player.fly_speed,
                walk_speed: player.walk_speed,
                creative: mode == Mode::Creative,
                dimension,
            }),
        }
    }
    #[allow(dead_code)]
    pub fn dimension(&self) -> i32 {
        self.motion.map_or(0, |k| k.dimension)
    }
    pub fn apply(self, player: &mut Player) {
        if let Some(k) = self.motion {
            player.set_pose_f64(self.position, k.rotation[0], k.rotation[1], k.grounded);
            player.set_velocity_f64(k.velocity);
            player.flying = k.flying;
            player.allow_flying = k.allow_flying;
            player.fly_speed = k.fly_speed;
            player.walk_speed = k.walk_speed;
        } else {
            player.set_position_f64(self.position);
        }
    }
    /// Entity.readFromNBT uses double lists for Pos/Motion and float lists for
    /// Rotation. A list of a different element type reads as an empty list.
    pub fn from_level(data: &Tag) -> Option<Self> {
        let player = data.get("Player")?;
        let dimension = nbt_integer(player.get("Dimension")).unwrap_or(0);
        if !matches!(dimension, -1 | 0 | 1) {
            return None;
        }
        let list = player.get("Pos")?.list()?;
        let position = std::array::from_fn(|i| match list.get(i) {
            Some(Tag::Double(v)) => *v,
            _ => 0.,
        });
        if !valid_position(position) {
            return None;
        }
        let motion = player.get("Motion").and_then(Tag::list).unwrap_or_default();
        let velocity = std::array::from_fn(|i| match motion.get(i) {
            Some(Tag::Double(v)) if v.abs() <= 10. => *v,
            _ => 0.,
        });
        let rotation = player
            .get("Rotation")
            .and_then(Tag::list)
            .unwrap_or_default();
        let rotation = std::array::from_fn(|i| match rotation.get(i) {
            Some(Tag::Float(v)) => *v % 360.,
            _ => 0.,
        });
        let abilities = player.get("abilities");
        let ability = |key| abilities.and_then(|a| a.get(key)).and_then(Tag::number);
        let flag = |key| nbt_boolean(abilities.and_then(|a| a.get(key)));
        let mode =
            nbt_integer(player.get("playerGameType")).or_else(|| nbt_integer(data.get("GameType")));
        let creative = mode.map_or_else(|| flag("instabuild"), |m| m == 1);
        let allow_flying = mode.map_or_else(|| flag("mayfly"), |m| matches!(m, 1 | 3));
        let flying = mode == Some(3) || allow_flying && flag("flying");
        let speeds = ability("flySpeed");
        Some(Self {
            position,
            motion: Some(Kinematics {
                velocity,
                rotation,
                grounded: nbt_boolean(player.get("OnGround")),
                flying,
                allow_flying,
                fly_speed: speeds.map_or(0.05, |v| v as f32),
                walk_speed: if speeds.is_some() {
                    ability("walkSpeed").unwrap_or(0.) as f32
                } else {
                    0.1
                },
                creative,
                dimension,
            }),
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[test]
    fn original_entityplayermp_nbt_preserves_double_pose_motion_and_game_type_capabilities() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-player-pose.json"
        ))
        .unwrap();
        for (i, row) in rows.iter().enumerate() {
            let hex = row["nbt"].as_str().unwrap();
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|n| u8::from_str_radix(&hex[n..n + 2], 16).unwrap())
                .collect::<Vec<_>>();
            let data = Tag::Compound(BTreeMap::from([(
                "Player".into(),
                crate::nbt::read(&bytes).unwrap(),
            )]));
            let pose = PlayerSave::from_level(&data).unwrap();
            let bed = crate::vanilla::respawn::BedSpawn::read(data.get("Player").unwrap());
            let coords = bed.map(|b| [b.point.x, b.point.y, b.point.z, i32::from(b.forced)]);
            assert_eq!(
                serde_json::json!(coords),
                row["bed"],
                "case {i} personal spawn"
            );
            let k = pose.motion.unwrap();
            for (key, values) in [("pos", pose.position), ("velocity", k.velocity)] {
                for (j, actual) in values.into_iter().enumerate() {
                    assert_eq!(
                        actual.to_bits(),
                        row[key][j].as_f64().unwrap().to_bits(),
                        "case {i} {key} axis {j}"
                    );
                }
            }
            for (j, actual) in k.rotation.into_iter().enumerate() {
                assert_eq!(
                    actual.to_bits(),
                    (row["rotation"][j].as_f64().unwrap() as f32).to_bits()
                );
            }
            assert_eq!(k.grounded, row["grounded"].as_bool().unwrap());
            assert_eq!(k.creative, row["creative"].as_bool().unwrap());
            assert_eq!(k.flying, row["flying"].as_bool().unwrap());
            assert_eq!(k.allow_flying, row["allow_flying"].as_bool().unwrap());
            assert_eq!(
                k.fly_speed.to_bits(),
                row["fly_speed"].as_u64().unwrap() as u32
            );
            assert_eq!(
                k.walk_speed.to_bits(),
                row["walk_speed"].as_u64().unwrap() as u32
            );
        }
        assert_eq!(rows.len(), 64);
    }

    #[test]
    fn from_level_preserves_nether_and_end_player_dimension() {
        for dim in [-1, 0, 1] {
            let player = Tag::Compound(BTreeMap::from([
                ("Dimension".into(), Tag::Int(dim)),
                (
                    "Pos".into(),
                    Tag::List(vec![Tag::Double(12.5), Tag::Double(64.0), Tag::Double(-8.5)]),
                ),
            ]));
            let data = Tag::Compound(BTreeMap::from([("Player".into(), player)]));
            let pose = PlayerSave::from_level(&data).expect("valid dimension should load");
            assert_eq!(pose.motion.unwrap().dimension, dim);
            assert_eq!(pose.dimension(), dim);
        }
    }
}
