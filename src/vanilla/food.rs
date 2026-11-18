//! Original FoodStats arithmetic and EntityPlayer exhaustion callbacks.
use crate::nbt::Tag;
use serde::{Deserialize, Serialize};
include!("food_data.rs");

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct FoodState {
    pub level: i32,
    pub saturation: f32,
    pub exhaustion: f32,
    pub timer: i32,
    pub previous_level: i32,
}
impl Default for FoodState {
    fn default() -> Self {
        Self {
            level: 20,
            saturation: 5.,
            exhaustion: 0.,
            timer: 0,
            previous_level: 20,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Context {
    pub difficulty: u8,
    pub natural_regeneration: bool,
    pub health: f32,
    pub max_health: f32,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Update {
    pub heal: bool,
    pub starve: bool,
}
fn java_min(a: f32, b: f32) -> f32 {
    if a.is_nan() {
        a
    } else if b.is_nan() {
        b
    } else if a == 0. && b == 0. {
        f32::from_bits(a.to_bits() | b.to_bits())
    } else if a < b {
        a
    } else {
        b
    }
}
fn java_max(a: f32, b: f32) -> f32 {
    if a.is_nan() {
        a
    } else if b.is_nan() {
        b
    } else if a == 0. && b == 0. {
        f32::from_bits(a.to_bits() & b.to_bits())
    } else if a > b {
        a
    } else {
        b
    }
}
fn number(tag: &Tag) -> bool {
    matches!(
        tag,
        Tag::Byte(_) | Tag::Short(_) | Tag::Int(_) | Tag::Long(_) | Tag::Float(_) | Tag::Double(_)
    )
}
pub(crate) fn nbt_int(tag: Option<&Tag>) -> i32 {
    match tag {
        Some(Tag::Byte(n)) => i32::from(*n),
        Some(Tag::Short(n)) => i32::from(*n),
        Some(Tag::Int(n)) => *n,
        Some(Tag::Long(n)) => *n as i32,
        Some(Tag::Float(n)) => {
            let truncated = *n as i32;
            if *n < truncated as f32 {
                truncated.wrapping_sub(1)
            } else {
                truncated
            }
        }
        Some(Tag::Double(n)) => {
            let truncated = *n as i32;
            if *n < truncated as f64 {
                truncated.wrapping_sub(1)
            } else {
                truncated
            }
        }
        _ => 0,
    }
}
fn float(tag: Option<&Tag>) -> f32 {
    match tag {
        Some(Tag::Byte(n)) => *n as f32,
        Some(Tag::Short(n)) => *n as f32,
        Some(Tag::Int(n)) => *n as f32,
        Some(Tag::Long(n)) => *n as f32,
        Some(Tag::Float(n)) => *n,
        Some(Tag::Double(n)) => *n as f32,
        _ => 0.,
    }
}
impl FoodState {
    pub fn read_nbt(&mut self, tag: &Tag) {
        if tag.get("foodLevel").is_some_and(number) {
            self.level = nbt_int(tag.get("foodLevel"));
            self.timer = nbt_int(tag.get("foodTickTimer"));
            self.saturation = float(tag.get("foodSaturationLevel"));
            self.exhaustion = float(tag.get("foodExhaustionLevel"));
        }
    }
    pub fn add_stats(&mut self, amount: i32, modifier: f32) {
        self.level = amount.wrapping_add(self.level).min(20);
        self.saturation = java_min(
            self.saturation + amount as f32 * modifier * 2.,
            self.level as f32,
        );
    }
    pub fn add_exhaustion(&mut self, amount: f32) {
        self.exhaustion = java_min(self.exhaustion + amount, 40.);
    }
    pub fn player_exhaustion(&mut self, amount: f32, immune: bool, remote: bool) {
        if !immune && !remote {
            self.add_exhaustion(amount);
        }
    }
    /// EntityPlayer.onLivingUpdate prefix, BEFORE FoodStats.onUpdate. Return
    /// whether heal(1) is invoked; EntityLivingBase.heal ignores dead players.
    pub fn peaceful_prefix(&mut self, context: Context, ticks_existed: i32) -> bool {
        if context.difficulty != 0 || !context.natural_regeneration {
            return false;
        }
        let heal = context.health < context.max_health && ticks_existed % 20 == 0;
        if self.level < 20 && ticks_existed % 10 == 0 {
            self.level = self.level.wrapping_add(1);
        }
        heal
    }
    pub fn update(&mut self, context: Context) -> Update {
        let mut result = Update::default();
        self.previous_level = self.level;
        if self.exhaustion > 4. {
            self.exhaustion -= 4.;
            if self.saturation > 0. {
                self.saturation = java_max(self.saturation - 1., 0.);
            } else if context.difficulty != 0 {
                self.level = self.level.wrapping_sub(1).max(0);
            }
        }
        if context.natural_regeneration
            && self.level >= 18
            && context.health > 0.
            && context.health < context.max_health
        {
            self.timer = self.timer.wrapping_add(1);
            if self.timer >= 80 {
                result.heal = true;
                self.add_exhaustion(3.);
                self.timer = 0;
            }
        } else if self.level <= 0 {
            self.timer = self.timer.wrapping_add(1);
            if self.timer >= 80 {
                result.starve = context.health > 10.
                    || context.difficulty == 3
                    || context.health > 1. && context.difficulty == 2;
                self.timer = 0;
            }
        } else {
            self.timer = 0;
        }
        result
    }
}
/// Health clamping in EntityLivingBase.heal/setHealth (NaN comparisons retain
/// the original value, rather than silently selecting a finite endpoint).
pub fn heal(health: f32, maximum: f32) -> f32 {
    if health <= 0. || health.is_nan() {
        return health;
    }
    let value = health + 1.;
    if value < 0. {
        0.
    } else if value > maximum {
        maximum
    } else {
        value
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Movement {
    pub riding: bool,
    pub eyes_in_water: bool,
    pub in_water: bool,
    pub on_ladder: bool,
    pub on_ground: bool,
    pub sprinting: bool,
}
/// Some(0) and None are distinct: an actual addExhaustion(0) still clamps an
/// imported exhaustion value >40, whereas no movement does not call it.
pub fn movement_exhaustion(delta: [f64; 3], movement: Movement) -> Option<f32> {
    if movement.riding {
        return None;
    }
    let [x, y, z] = delta;
    let (squared, rate) = if movement.eyes_in_water {
        (x * x + y * y + z * z, 0.015f32)
    } else if movement.in_water {
        (x * x + z * z, 0.015f32)
    } else if movement.on_ladder || !movement.on_ground {
        return None;
    } else {
        (
            x * x + z * z,
            if movement.sprinting {
                0.099999994f32
            } else {
                0.01f32
            },
        )
    };
    let distance = squared.sqrt() as f32 * 100.;
    let cm = (distance as f64 + 0.5).floor() as i32;
    (cm > 0).then_some(rate * cm as f32 * 0.01)
}
pub fn jump_exhaustion(sprinting: bool) -> f32 {
    if sprinting {
        0.8
    } else {
        0.2
    }
}
#[derive(Clone, Copy, Debug)]
pub struct FoodValue {
    pub nutrition: i32,
    pub saturation: f32,
    pub always_edible: bool,
}
pub fn food_values(id: u16, meta: u16) -> Option<FoodValue> {
    let meta = if meta > 15 { 0 } else { meta as u8 };
    FOOD.iter()
        .find(|row| row.0 == id && row.1 == meta)
        .map(|row| FoodValue {
            nutrition: row.2,
            saturation: f32::from_bits(row.3),
            always_edible: row.4,
        })
}

/// EntityPlayer/InventoryPlayer.canHarvestBlock, including the material's
/// requires-no-tool flag and every original pickaxe/sword/shovel/shears tier.
pub fn can_harvest(state: u16, held: Option<u16>) -> bool {
    let category = match held {
        Some(270 | 285) => 1,
        Some(274) => 2,
        Some(257) => 3,
        Some(278) => 4,
        Some(267 | 268 | 272 | 276 | 283) => 5,
        Some(256 | 269 | 273 | 277 | 284) => 6,
        Some(359) => 7,
        _ => 0,
    };
    HARVEST
        .get((state >> 4) as usize)
        .copied()
        .unwrap_or(HARVEST[0])
        & (1 << category)
        != 0
}
/// Called only when ItemInWorldManager invokes harvestBlock after a successful
/// tool-qualified removal; subclasses with their own harvest path skip .025.
pub fn harvest_exhaustion(
    state: u16,
    held: Option<u16>,
    banner_tile: bool,
    remote: bool,
) -> Option<f32> {
    let id = state >> 4;
    if id == 78 || matches!(id, 176 | 177) && banner_tile {
        return None;
    }
    if !remote
        && held == Some(359)
        && (matches!(id, 18 | 31 | 32 | 106 | 161)
            || id == 175 && state & 8 == 0 && matches!(state & 7, 2 | 3))
    {
        return None;
    }
    Some(0.025)
}
pub fn eyes_below_water_surface(eye_y: f64, block_y: i32, level: u8) -> bool {
    let level = if level >= 8 { 0 } else { level };
    let percent = (f32::from(level) + 1.) / 9. - 0.11111111f32;
    eye_y < ((block_y + 1) as f32 - percent) as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn f(value: &Value) -> f32 {
        f32::from_bits(value.as_u64().unwrap() as u32)
    }
    fn snapshot(food: FoodState, health: f32, heals: i32, damages: i32) -> Value {
        json!({"food":food.level,"timer":food.timer,"saturation":food.saturation.to_bits(),"exhaustion":food.exhaustion.to_bits(),"previous":food.previous_level,"health":health.to_bits(),"heals":heals,"damages":damages})
    }
    #[test]
    fn hunger_and_player_callbacks_match_original_bytecode() {
        let data: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/vanilla189-food.json"))
                .unwrap();
        for (index, row) in data["ticks"].as_array().unwrap().iter().enumerate() {
            let mut food = FoodState::default();
            let n = &row["nbt"];
            let mut tags = std::collections::BTreeMap::from([
                (
                    "foodTickTimer".into(),
                    Tag::Int(n[1].as_i64().unwrap() as i32),
                ),
                ("foodSaturationLevel".into(), Tag::Float(f(&n[2]))),
                ("foodExhaustionLevel".into(), Tag::Float(f(&n[3]))),
            ]);
            match row["presence"].as_u64().unwrap() {
                0 => {
                    tags.insert("foodLevel".into(), Tag::Int(n[0].as_i64().unwrap() as i32));
                }
                2 => {
                    tags.insert("foodLevel".into(), Tag::String("bad type".into()));
                }
                _ => {}
            }
            food.read_nbt(&Tag::Compound(tags));
            let mut context = Context {
                difficulty: row["difficulty"].as_u64().unwrap() as u8,
                natural_regeneration: row["regeneration"].as_bool().unwrap(),
                health: f(&row["initial"]["health"]),
                max_health: f(&row["maximum"]),
            };
            assert_eq!(
                snapshot(food, context.health, 0, 0),
                row["initial"],
                "NBT read case{index}"
            );
            let (mut heals, mut damages) = (0, 0);
            for tick in 0..row["ticks"].as_i64().unwrap() as i32 {
                if food.peaceful_prefix(context, row["start"].as_i64().unwrap() as i32 + tick) {
                    heals += 1;
                    context.health = heal(context.health, context.max_health);
                }
                if !row["remote"].as_bool().unwrap() {
                    let update = food.update(context);
                    if update.heal {
                        heals += 1;
                        context.health = heal(context.health, context.max_health);
                    }
                    if update.starve {
                        damages += 1;
                        if !row["immune"].as_bool().unwrap() && context.health > 0. {
                            context.health = (context.health - 1.).max(0.);
                        }
                    }
                }
            }
            assert_eq!(
                snapshot(food, context.health, heals, damages),
                row["result"],
                "FoodStats sequence{index} {row}"
            );
        }
        for (index, row) in data["actions"].as_array().unwrap().iter().enumerate() {
            let mut food = FoodState {
                level: row["level"].as_i64().unwrap() as i32,
                saturation: f(&row["saturation"]),
                exhaustion: f(&row["initial"]),
                ..Default::default()
            };
            let flags = row["flags"].as_u64().unwrap();
            let amount = f(&row["amount"]);
            let event = match row["action"].as_str().unwrap() {
                "movement" => movement_exhaustion(
                    std::array::from_fn(|i| row["delta"][i].as_f64().unwrap()),
                    Movement {
                        riding: flags & 128 != 0,
                        eyes_in_water: flags & 4 != 0,
                        in_water: flags & 8 != 0,
                        on_ladder: flags & 16 != 0,
                        on_ground: flags & 32 != 0,
                        sprinting: flags & 64 != 0,
                    },
                ),
                "jump" => Some(jump_exhaustion(flags & 64 != 0)),
                "attack" => (amount != 0.).then_some(0.3),
                "mine" => Some(0.025),
                "player_add" => Some(amount),
                "stats_add" => {
                    food.add_exhaustion(amount);
                    None
                }
                "eat" => {
                    food.add_stats(amount as i32, row["delta"][0].as_f64().unwrap() as f32);
                    None
                }
                _ => unreachable!(),
            };
            if let Some(amount) = event {
                food.player_exhaustion(amount, flags & 1 != 0, flags & 2 != 0);
            }
            assert_eq!(
                snapshot(food, 20., 0, 0),
                row["result"],
                "Player exhaustion case{index} {row}"
            );
        }
        assert_eq!(data["ticks"].as_array().unwrap().len(), 2240);
        assert_eq!(data["actions"].as_array().unwrap().len(), 1776);
    }
    #[test]
    fn original_block_harvest_exhaustion_overrides_match() {
        let data: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/vanilla189-food.json"))
                .unwrap();
        for (index, row) in data["harvest"].as_array().unwrap().iter().enumerate() {
            let state = row[0].as_u64().unwrap() as u16;
            let held = row[1].as_bool().unwrap().then_some(359);
            let flags = row[2].as_u64().unwrap();
            let mut food = FoodState::default();
            if let Some(exhaustion) =
                harvest_exhaustion(state, held, row[3].as_bool().unwrap(), flags & 2 != 0)
            {
                food.player_exhaustion(exhaustion, flags & 1 != 0, flags & 2 != 0);
            }
            assert_eq!(snapshot(food, 20., 0, 0), row[4], "harvest{index}: {row}");
        }
        assert_eq!(data["harvest"].as_array().unwrap().len(), 11160);
    }
}
