//! Original WorldBorder and the S44 actions that change click eligibility.
use crate::protocol::Reader;
use anyhow::{bail, ensure, Result};
use glam::IVec3;
use std::time::{SystemTime, UNIX_EPOCH};
include!("click_border_data.rs");
#[derive(Clone, Copy, Debug)]
pub enum Update {
    Size(f64),
    Lerp(f64, f64, i64),
    Center(f64, f64),
    Init {
        center: [f64; 2],
        start: f64,
        end: f64,
        duration: i64,
        limit: i32,
        distance: i32,
        time: i32,
    },
    Time(i32),
    Distance(i32),
}
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
fn varlong(r: &mut Reader<'_>) -> Result<i64> {
    let mut value = 0u64;
    for i in 0..10 {
        let b = r.u8()?;
        ensure!(i != 9 || b & 0xfe == 0, "VarLong exceeds 64 bits");
        value |= u64::from(b & 127) << (7 * i);
        if b & 128 == 0 {
            return Ok(value as i64);
        }
    }
    bail!("Invalid border duration")
}
pub fn decode(r: &mut Reader<'_>) -> Result<Update> {
    let update = match r.varint()? {
        0 => Update::Size(r.f64()?),
        1 => Update::Lerp(r.f64()?, r.f64()?, varlong(r)?),
        2 => Update::Center(r.f64()?, r.f64()?),
        3 => Update::Init {
            center: [r.f64()?, r.f64()?],
            start: r.f64()?,
            end: r.f64()?,
            duration: varlong(r)?,
            limit: r.varint()?,
            distance: r.varint()?,
            time: r.varint()?,
        },
        4 => Update::Time(r.varint()?),
        5 => Update::Distance(r.varint()?),
        _ => bail!("Unknown S44 action"),
    };
    r.finish()?;
    Ok(update)
}
#[derive(Clone, Debug)]
pub struct Border {
    pub center: [f64; 2],
    start: f64,
    end: f64,
    started: i64,
    duration: i64,
    limit: i32,
    pub warning_time: i32,
    pub warning_distance: i32,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct SavedBorder {
    center: [f64; 2],
    size: f64,
    target: f64,
    remaining: i64,
    limit: i32,
    warning_time: i32,
    warning_distance: i32,
}
impl serde::Serialize for Border {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let t = now();
        let remaining = if self.start == self.end {
            0
        } else {
            self.duration
                .saturating_sub(t.saturating_sub(self.started))
                .max(0)
        };
        SavedBorder {
            center: self.center,
            size: self.diameter(t),
            target: self.end,
            remaining,
            limit: self.limit,
            warning_time: self.warning_time,
            warning_distance: self.warning_distance,
        }
        .serialize(serializer)
    }
}
impl<'de> serde::Deserialize<'de> for Border {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = SavedBorder::deserialize(de)?;
        Ok(Self {
            center: s.center,
            start: s.size,
            end: if s.remaining > 0 { s.target } else { s.size },
            started: now(),
            duration: s.remaining,
            limit: s.limit,
            warning_time: s.warning_time,
            warning_distance: s.warning_distance,
        })
    }
}
impl Default for Border {
    fn default() -> Self {
        Self {
            center: [0.; 2],
            start: 60_000_000.,
            end: 60_000_000.,
            started: 0,
            duration: 0,
            limit: 29_999_984,
            warning_time: 15,
            warning_distance: 5,
        }
    }
}
impl Border {
    pub fn from_world_info(data: &crate::nbt::Tag, t: i64) -> Self {
        use crate::nbt::Tag;
        let mut border = Self::default();
        let num = |k: &str| data.get(k).and_then(Tag::number);
        border.center = [
            num("BorderCenterX").unwrap_or(0.),
            num("BorderCenterZ").unwrap_or(0.),
        ];
        let size = num("BorderSize").unwrap_or(60_000_000.);
        let time = match data.get("BorderSizeLerpTime") {
            Some(Tag::Long(v)) => *v,
            _ => num("BorderSizeLerpTime").unwrap_or(0.) as i64,
        };
        if time > 0 {
            border.transition(size, num("BorderSizeLerpTarget").unwrap_or(0.), time, t);
        } else {
            border.transition(size, size, 0, t);
        }
        if num("BorderWarningBlocks").is_some() {
            border.warning_distance = super::food::nbt_int(data.get("BorderWarningBlocks"));
        }
        if num("BorderWarningTime").is_some() {
            border.warning_time = super::food::nbt_int(data.get("BorderWarningTime"));
        }
        border
    }
    pub fn target(&self)->f64{self.end}
    pub fn remaining(&self,t:i64)->i64{if self.start==self.end{0}else{self.duration.saturating_sub(t.saturating_sub(self.started)).max(0)}}
    fn transition(&mut self, start: f64, end: f64, duration: i64, now: i64) {
        self.start = start;
        self.end = end;
        self.duration = duration;
        self.started = now;
    }
    pub fn apply(&mut self, update: Update, now: i64) {
        match update {
            Update::Size(v) => self.transition(v, v, 0, now),
            Update::Lerp(a, b, d) => self.transition(a, b, d, now),
            Update::Center(x, z) => self.center = [x, z],
            Update::Time(t) => self.warning_time = t,
            Update::Distance(d) => self.warning_distance = d,
            Update::Init {
                center,
                start,
                end,
                duration,
                limit,
                distance,
                time,
            } => {
                self.center = center;
                self.limit = limit;
                self.warning_time = time;
                self.warning_distance = distance;
                if duration > 0 {
                    self.transition(start, end, duration, now)
                } else {
                    self.transition(end, end, 0, now)
                }
            }
        }
    }
    pub fn diameter(&self, now: i64) -> f64 {
        if self.start == self.end {
            return self.start;
        }
        // WorldBorder deliberately divides two Java floats before widening to double.
        let part = f64::from(now.wrapping_sub(self.started) as f32 / self.duration as f32);
        if part >= 1. {
            self.end
        } else {
            self.start + (self.end - self.start) * part
        }
    }
    pub fn bounds(&self, now: i64) -> [f64; 4] {
        let half = self.diameter(now) / 2.;
        let limit = f64::from(self.limit);
        let [x, z] = self.center;
        let min = |v: f64| if v < -limit { -limit } else { v };
        let max = |v: f64| if v > limit { limit } else { v };
        [min(x - half), min(z - half), max(x + half), max(z + half)]
    }
    pub fn contains(&self, p: IVec3, now: i64) -> bool {
        let half = self.diameter(now) / 2.;
        let limit = f64::from(self.limit);
        let [x, z] = self.center;
        let [px, pz] = [f64::from(p.x), f64::from(p.z)];
        let min = |v: f64| if v < -limit { -limit } else { v };
        let max = |v: f64| if v > limit { limit } else { v };
        px + 1. > min(x - half)
            && px < max(x + half)
            && pz + 1. > min(z - half)
            && pz < max(z + half)
    }
}
pub fn block_name_matches(block: u16, name: &str) -> bool {
    let Some((_, key)) = BLOCK_NAMES.iter().find(|(id, _)| *id == block) else {
        return false;
    };
    let path = match name.split_once(':') {
        Some((domain, path)) if domain.len() <= 1 || domain.eq_ignore_ascii_case("minecraft") => {
            Some(path)
        }
        Some(_) => None,
        None => Some(name),
    };
    path.is_some_and(|path| key.strip_prefix("minecraft:") == Some(path))
        || name.parse::<i32>().ok() == Some(i32::from(block))
}
pub fn can_edit(mode: u8, block: u16, held: Option<&crate::inventory::Stack>) -> bool {
    permits_edit(mode, block, held, "CanDestroy")
}
pub fn can_place_on(mode: u8, block: u16, held: Option<&crate::inventory::Stack>) -> bool {
    permits_edit(mode, block, held, "CanPlaceOn")
}
fn permits_edit(mode: u8, block: u16, held: Option<&crate::inventory::Stack>, tag: &str) -> bool {
    use crate::vanilla::{drops::DropNbt, item_nbt};
    if mode == 3 {
        return false;
    }
    if mode != 2 {
        return true;
    }
    let tags = held
        .and_then(|s| s.nbt.as_deref())
        .and_then(item_nbt::compound);
    matches!(tags.and_then(|m|m.get(tag)),Some(DropNbt::List{element:8,values}) if values.iter().any(|s|matches!(s,DropNbt::String(s) if block_name_matches(block,s))))
}
#[derive(Default)]
pub struct CanDestroy {
    items: std::collections::HashMap<u64, (u16, bool)>,
}
impl CanDestroy {
    pub fn allows(&mut self, mode: u8, block: u16, inv: &crate::inventory::Inventory) -> bool {
        if mode != 2 {
            return mode != 3;
        }
        let Some(item) = inv.slots[inv.selected].as_ref() else {
            return false;
        };
        if let Some(&(last, result)) = self.items.get(&item.identity) {
            if last == block {
                return result;
            }
        }
        if self.items.len() > inv.slots.len() {
            self.items
                .retain(|id, _| inv.slots.iter().flatten().any(|s| s.identity == *id));
        }
        let result = can_edit(mode, block, Some(item));
        self.items.insert(item.identity, (block, result));
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::io::Read;
    #[derive(Deserialize)]
    struct Data {
        border: Vec<Case>,
        tools: Vec<(u16, String, bool)>,
        cache: Vec<(bool, String, u16, bool)>,
        levels: Vec<(String, [String; 4], i64, i32, i32)>,
    }
    #[derive(Deserialize)]
    struct Case {
        wire: String,
        elapsed: i64,
        diameter: [String; 1],
        center: [String; 2],
        warnings: [i32; 2],
        contains: Vec<(i32, i32, bool)>,
    }
    #[test]
    fn original_s44_transitions_border_membership_and_adventure_can_destroy_registry_match() {
        let mut json = vec![];
        flate2::read::GzDecoder::new(
            &include_bytes!("../../tests/fixtures/vanilla189-click-border.json.gz")[..],
        )
        .read_to_end(&mut json)
        .unwrap();
        let data: Data = serde_json::from_slice(&json).unwrap();
        assert_eq!((data.border.len(), data.tools.len()), (90, 1386));
        for row in data.border {
            let bytes: Vec<_> = (0..row.wire.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&row.wire[i..i + 2], 16).unwrap())
                .collect();
            let mut border = Border::default();
            border.apply(decode(&mut Reader::new(&bytes)).unwrap(), 0);
            assert_eq!(
                border.diameter(row.elapsed).to_bits(),
                u64::from_str_radix(&row.diameter[0], 16).unwrap(),
                "S44 {}",
                row.wire
            );
            assert_eq!(
                border.center.map(f64::to_bits),
                row.center.map(|s| u64::from_str_radix(&s, 16).unwrap())
            );
            assert_eq!([border.warning_time, border.warning_distance], row.warnings);
            for (x, z, inside) in row.contains {
                assert_eq!(
                    border.contains(IVec3::new(x, 63, z), row.elapsed),
                    inside,
                    "S44 {}",
                    row.wire
                );
            }
        }
        for (id, name, expected) in data.tools {
            assert_eq!(
                block_name_matches(id, &name),
                expected,
                "CanDestroy {id}/{name}"
            );
        }
        let mut cache = CanDestroy::default();
        let mut inv = crate::inventory::Inventory::default();
        inv.slots[0] = Some(crate::inventory::Stack::new(278, 1));
        for (copy, name, id, result) in data.cache {
            use crate::vanilla::drops::DropNbt;
            let s = inv.slots[0].as_mut().unwrap();
            s.nbt = Some(std::sync::Arc::new(DropNbt::Compound(
                std::collections::BTreeMap::from([(
                    "CanDestroy".into(),
                    DropNbt::List {
                        element: 8,
                        values: vec![DropNbt::String(name)],
                    },
                )]),
            )));
            if copy {
                s.identity = crate::inventory::Stack::fresh_identity();
            }
            assert_eq!(cache.allows(2, id, &inv), result);
        }
        for (bytes, values, duration, time, distance) in data.levels {
            let bytes: Vec<_> = (0..bytes.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&bytes[i..i + 2], 16).unwrap())
                .collect();
            let raw = crate::nbt::read(&bytes).unwrap();
            let border = Border::from_world_info(&raw, 189);
            let [x, z, start, target] = values.map(|s| u64::from_str_radix(&s, 16).unwrap());
            assert_eq!(border.center.map(f64::to_bits), [x, z]);
            assert_eq!(border.start.to_bits(), start);
            assert_eq!(
                border.end.to_bits(),
                if duration > 0 { target } else { start }
            );
            assert_eq!(
                (
                    border.duration,
                    border.warning_time,
                    border.warning_distance
                ),
                (duration, time, distance)
            );
        }
    }
}
