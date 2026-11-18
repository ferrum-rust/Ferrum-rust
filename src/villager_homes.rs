//! Saved Overworld VillageCollection home lookup and reputation.
use crate::nbt::{self, Tag};
use anyhow::{ensure, Result};
use flate2::read::GzDecoder;
use glam::IVec3;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    fs::File,
    io::Read,
    path::Path,
};
#[path = "villager_collection.rs"]
pub mod collection;
#[path = "villager_reputation.rs"]
pub mod reputation;
#[cfg(test)]
#[path = "villager_home_tests.rs"]
mod tests;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Homes {
    villages: Vec<Village>,
    ticks: i32,
    queue: VecDeque<IVec3>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct Village {
    active: bool,
    center: IVec3,
    sum: IVec3,
    radius: i32,
    reputation: BTreeMap<String, i32>,
    doors: Vec<Door>,
    residents: i32,
    golems: i32,
    last_added: i32,
    last_tick: i32,
    no_breed_since: i32,
    #[serde(skip)]
    attackers: Vec<reputation::Aggressor>,
}
impl Default for Village {
    fn default() -> Self {
        Self {
            active: true,
            center: IVec3::ZERO,
            sum: IVec3::ZERO,
            radius: 0,
            reputation: BTreeMap::new(),
            doors: Vec::new(),
            residents: 0,
            golems: 0,
            last_added: 0,
            last_tick: 0,
            no_breed_since: 0,
            attackers: Vec::new(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Door {
    position: IVec3,
    inside: IVec3,
    timestamp: i32,
    restriction: i32,
}
impl Homes {
    pub fn import(path: &Path) -> Result<Self> {
        let file = path.join("data/villages.dat");
        if !file.is_file() {
            return Ok(Self::default());
        }
        let mut bytes = Vec::new();
        GzDecoder::new(File::open(file)?)
            .take(32 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= 32 * 1024 * 1024,
            "villages.dat exceeds the NBT size limit"
        );
        let root = nbt::read(&bytes)?;
        let profiles = ["usercache.json", "usercache.json.old"]
            .iter()
            .find_map(|name| {
                let path = path.parent()?.parent()?.join(name);
                let bytes = std::fs::read(path).ok()?;
                serde_json::from_slice::<Vec<Profile>>(&bytes).ok()
            })
            .unwrap_or_default()
            .into_iter()
            .map(|p| (p.uuid.to_lowercase(), p.name))
            .collect::<BTreeMap<_, _>>();
        Ok(Self::from_data(root.get("data"), &profiles))
    }
    fn from_data(data: Option<&Tag>, profiles: &BTreeMap<String, String>) -> Self {
        let mut villages = Vec::new();
        for tag in data
            .and_then(|d| d.get("Villages"))
            .and_then(Tag::list)
            .into_iter()
            .flatten()
        {
            let int = |k| tag.get(k).and_then(Tag::int).unwrap_or(0);
            let mut reputation = BTreeMap::new();
            for player in tag.get("Players").and_then(Tag::list).into_iter().flatten() {
                let name = if let Some(uuid) = player.get("UUID") {
                    uuid.string()
                        .and_then(|s| profiles.get(&s.to_lowercase()))
                        .map(String::as_str)
                } else {
                    player.get("Name").and_then(Tag::string)
                };
                if let Some(name) = name {
                    reputation.insert(
                        name.to_owned(),
                        player.get("S").and_then(Tag::int).unwrap_or(0),
                    );
                }
            }
            villages.push(Village {
                center: IVec3::new(int("CX"), int("CY"), int("CZ")),
                sum: IVec3::new(int("ACX"), int("ACY"), int("ACZ")),
                radius: int("Radius"),
                last_added: int("Stable"),
                last_tick: int("Tick"),
                no_breed_since: int("MTick"),
                golems: int("Golems"),
                residents: int("PopSize"),
                doors: tag
                    .get("Doors")
                    .and_then(Tag::list)
                    .into_iter()
                    .flatten()
                    .map(|d| {
                        let n = |key| d.get(key).and_then(Tag::int).unwrap_or(0);
                        let dx = n("IDX");
                        let dz = n("IDZ");
                        let inside = if dx < 0 {
                            IVec3::NEG_X
                        } else if dx > 0 {
                            IVec3::X
                        } else if dz < 0 {
                            IVec3::NEG_Z
                        } else {
                            IVec3::Z
                        };
                        Door {
                            position: IVec3::new(n("X"), n("Y"), n("Z")),
                            inside,
                            timestamp: n("TS"),
                            restriction: 0,
                        }
                    })
                    .collect(),
                reputation,
                ..Default::default()
            });
        }
        Self {
            villages,
            ticks: data
                .and_then(|d| d.get("Tick"))
                .and_then(Tag::int)
                .unwrap_or(0),
            ..Default::default()
        }
    }
    pub fn nearest(&self, p: IVec3) -> Option<usize> {
        let mut nearest = None;
        let mut best = f64::from(f32::MAX);
        for (index, village) in self.villages.iter().enumerate() {
            if !village.active {
                continue;
            }
            let d = (village.center.as_dvec3() - p.as_dvec3()).length_squared();
            let reach = village.radius.wrapping_add(32) as f32;
            if d < best && d <= f64::from(reach * reach) {
                best = d;
                nearest = Some(index);
            }
        }
        nearest
    }
    pub fn reward(&mut self, index: usize, player: &str) {
        if let Some(village) = self.villages.get_mut(index) {
            village.change_score(player, 1);
        }
    }
    pub fn restriction(&self, index: usize) -> Option<(IVec3, f32)> {
        self.villages
            .get(index)
            .map(|v| (v.center, v.radius as f32))
    }
}
#[derive(Deserialize)]
struct Profile {
    uuid: String,
    name: String,
}
