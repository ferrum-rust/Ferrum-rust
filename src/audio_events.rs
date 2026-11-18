//! 1.8.9 sounds.json registration. Pack files append to an existing event unless replace is set.
use crate::vanilla::random::JavaRandom;
use anyhow::{ensure, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Deserialize, Clone, Copy, Default)]
#[serde(rename_all = "lowercase")]
enum Category {
    #[default]
    Master,
    Music,
    Record,
    Weather,
    Block,
    Hostile,
    Neutral,
    Player,
    Ambient,
}
#[derive(Deserialize, Clone, Copy, Default)]
#[serde(rename_all = "lowercase")]
enum Kind {
    #[default]
    File,
    Event,
}
#[derive(Deserialize)]
struct Definition {
    #[serde(default)]
    category: Category,
    #[serde(default)]
    replace: bool,
    #[serde(default)]
    sounds: Vec<Sound>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Sound {
    Name(String),
    Full {
        name: String,
        #[serde(rename = "type", default)]
        kind: Kind,
        #[serde(default = "one")]
        volume: f32,
        #[serde(default = "one")]
        pitch: f32,
        #[serde(default = "one_weight")]
        weight: i32,
        #[serde(default)]
        stream: bool,
    },
}
fn one() -> f32 {
    1.
}
fn one_weight() -> i32 {
    1
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Clip {
    pub path: String,
    pub pitch: f32,
    pub volume: f32,
    pub stream: bool,
}
enum Target {
    File(Clip),
    Event(String),
}
struct Entry {
    target: Target,
    weight: i32,
}
#[derive(Default)]
struct Event {
    category: usize,
    choices: Vec<Entry>,
    total: i32,
}
#[derive(Default)]
pub(super) struct Bank {
    events: BTreeMap<String, Event>,
    secret: Vec<String>,
    files: HashMap<String, usize>,
}
fn location(s: &str) -> Option<&str> {
    let s = s.strip_prefix("minecraft:").unwrap_or(s);
    (s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"_-/.".contains(&b))
        && s.split('/').all(|part| !matches!(part, "" | "." | "..")))
    .then_some(s)
}
impl Bank {
    pub fn append(&mut self, json: &[u8], exists: impl Fn(&str) -> bool) -> Result<()> {
        let definitions: BTreeMap<String, Definition> = serde_json::from_slice(json)?;
        // Gson rejects this manifest before registering any of its events.
        for def in definitions.values() {
            for sound in &def.sounds {
                if let Sound::Full {
                    volume,
                    pitch,
                    weight,
                    ..
                } = sound
                {
                    ensure!(
                        *volume > 0. && *pitch > 0. && *weight > 0,
                        "Invalid sound weight, volume or pitch"
                    );
                }
            }
        }
        for (key, def) in definitions {
            let Some(key) = location(&key) else { continue };
            let create = || Event {
                category: def.category as usize,
                ..Default::default()
            };
            let event = self.events.entry(key.into()).or_insert_with(create);
            if def.replace {
                *event = create()
            }
            for sound in def.sounds {
                let (name, kind, volume, pitch, weight, stream) = match sound {
                    Sound::Name(s) => (s, Kind::File, 1., 1., 1, false),
                    Sound::Full {
                        name,
                        kind,
                        volume,
                        pitch,
                        weight,
                        stream,
                    } => (name, kind, volume, pitch, weight, stream),
                };
                let Some(name) = location(&name) else {
                    continue;
                };
                let target = match kind {
                    Kind::File if exists(name) => Target::File(Clip {
                        path: name.into(),
                        pitch,
                        volume,
                        stream,
                    }),
                    Kind::File => continue,
                    Kind::Event => Target::Event(name.into()),
                };
                event.choices.push(Entry { target, weight });
            }
        }
        Ok(())
    }
    pub fn finish(&mut self) {
        let mut seen = HashMap::new();
        let mut blocked = HashSet::new();
        self.secret.clear();
        self.files.clear();
        for (key, event) in &self.events {
            weight(key, &self.events, &mut seen, &mut blocked, 0);
            if (3..=7).contains(&event.category) {
                self.secret.push(key.clone());
            }
            for choice in &event.choices {
                if let Target::File(clip) = &choice.target {
                    self.files
                        .entry(clip.path.clone())
                        .or_insert(event.category);
                }
            }
        }
        for (key, event) in &mut self.events {
            event.total = seen[key];
            for (i, entry) in event.choices.iter_mut().enumerate() {
                if let Target::Event(other) = &entry.target {
                    entry.weight = if blocked.contains(&(key.clone(), i)) {
                        0
                    } else {
                        seen.get(other).copied().unwrap_or(0)
                    };
                }
            }
        }
    }
    pub fn category(&self, name: &str) -> usize {
        self.files.get(name).copied().unwrap_or(0)
    }
    pub fn secret(&self, rng: &mut JavaRandom) -> Option<(Clip, usize)> {
        if self.secret.is_empty() {
            return None;
        }
        let i = rng.next_int(self.secret.len() as i32) as usize;
        self.pick(&self.secret[i], rng)
    }
    pub fn pick(&self, key: &str, rng: &mut JavaRandom) -> Option<(Clip, usize)> {
        let mut event = self.events.get(key)?;
        let category = event.category;
        for _ in 0..64 {
            if event.total == 0 {
                return None;
            }
            let mut index = rng.next_int(event.total);
            let entry = event.choices.iter().find(|entry| {
                index -= entry.weight;
                index < 0
            })?;
            match &entry.target {
                Target::File(clip) => return Some((clip.clone(), category)),
                Target::Event(next) => event = self.events.get(next)?,
            }
        }
        None
    }
}
fn weight(
    key: &str,
    events: &BTreeMap<String, Event>,
    seen: &mut HashMap<String, i32>,
    blocked: &mut HashSet<(String, usize)>,
    depth: u32,
) -> i32 {
    if let Some(value) = seen.get(key) {
        return *value;
    }
    let Some(event) = events.get(key) else {
        return 0;
    };
    if depth >= 64 {
        return 0;
    }
    seen.insert(key.into(), 0);
    let mut total = 0i32;
    for (i, choice) in event.choices.iter().enumerate() {
        let w = match &choice.target {
            Target::File(_) => choice.weight,
            Target::Event(next) => {
                let w = weight(next, events, seen, blocked, depth + 1);
                if w == 0 {
                    blocked.insert((key.into(), i));
                }
                w
            }
        };
        total = total.saturating_add(w);
    }
    seen.insert(key.into(), total);
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layered_events_preserve_category_and_resolve_weighted_aliases() {
        let mut bank = Bank::default();
        let low=br#"{
            "gui.button.press":{"sounds":["click"]},
            "child":{"category":"player","sounds":[{"name":"a","weight":3,"volume":0.8,"pitch":1.2,"stream":true}]},
            "alias":{"category":"hostile","sounds":[{"name":"minecraft:child","type":"event","weight":99,"pitch":4}]},
            "discard":{"sounds":["old"]},
            "bad":{"sounds":[{"type":"event","name":"bad"},"missing","../outside","ok"]}
        }"#;
        let high = br#"{
            "child":{"category":"weather","sounds":["b"]},
            "discard":{"replace":true,"category":"ambient","sounds":["new"]}
        }"#;
        bank.append(low, |path| path != "missing").unwrap();
        bank.append(high, |_| true).unwrap();
        bank.finish();
        assert_eq!(
            (bank.events["alias"].total, bank.events["child"].total),
            (4, 4)
        );
        assert_eq!(bank.events["child"].category, 7);
        assert_eq!(bank.events["bad"].total, 1);
        assert_eq!(bank.secret, ["alias", "child"]);
        let mut rng = JavaRandom::new(123);
        let mut a = 0;
        for _ in 0..1000 {
            let (clip, category) = bank.pick("alias", &mut rng).unwrap();
            assert_eq!(category, 5);
            if clip.path == "a" {
                a += 1;
                assert_eq!((clip.volume, clip.pitch, clip.stream), (0.8, 1.2, true));
            } else {
                assert_eq!(clip.path, "b");
            }
        }
        assert!((700..800).contains(&a));
        assert_eq!(bank.pick("bad", &mut rng).unwrap().0.path, "ok");
        assert_eq!(bank.pick("discard", &mut rng).unwrap().0.path, "new");
        assert_eq!(bank.pick("gui.button.press", &mut rng).unwrap().1, 0);
    }

    #[test]
    #[ignore = "requires locally imported Minecraft resources"]
    fn original_sound_registry_and_secret_and_music_pools() {
        let r = crate::validation::resources();
        let mut bank = Bank::default();
        for bytes in r.sound_manifests().unwrap() {
            bank.append(&bytes, |s| r.has_sound(s)).unwrap();
        }
        bank.finish();
        assert_eq!(bank.events.len(), 246);
        assert_eq!(bank.secret.len(), 211);
        let mut rng = JavaRandom::new(674);
        assert_eq!(
            bank.pick("gui.button.press", &mut rng).unwrap().0.path,
            "random/click"
        );
        assert_eq!(bank.events["music.menu"].total, 4);
        let mut tracks = std::collections::BTreeSet::new();
        for _ in 0..128 {
            let (clip, category) = bank.pick("music.menu", &mut rng).unwrap();
            assert!(clip.stream);
            assert_eq!(category, 1);
            tracks.insert(clip.path);
            let (other, c) = bank.secret(&mut rng).unwrap();
            assert!(r.has_sound(&other.path) && (3..=7).contains(&c));
        }
        assert_eq!(tracks.len(), 4);
    }
}
