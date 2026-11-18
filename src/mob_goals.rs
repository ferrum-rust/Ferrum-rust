//! EntityAITasks preserves registration order during setup and start order
//! during updates. Equal-priority goals can conflict through their mutex bits.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Entry {
    key: u32,
    action: u16,
    priority: i32,
    mutex: i32,
    interruptible: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Selector {
    entries: Vec<Entry>,
    active: Vec<u32>,
    clock: i32,
    next: u32,
}
pub trait Goals {
    fn should_start(&mut self, action: u16) -> bool;
    fn should_continue(&mut self, action: u16) -> bool;
    fn start(&mut self, action: u16);
    fn reset(&mut self, action: u16);
    fn update(&mut self, action: u16);
}
impl Selector {
    pub fn add(&mut self, action: u16, priority: i32, mutex: i32, interruptible: bool) {
        self.entries.push(Entry {
            key: self.next,
            action,
            priority,
            mutex,
            interruptible,
        });
        self.next = self.next.wrapping_add(1);
    }
    #[cfg(test)]
    pub fn remove(&mut self, action: u16, owner: &mut impl Goals) {
        self.entries.retain(|entry| {
            if entry.action != action {
                return true;
            }
            if let Some(i) = self.active.iter().position(|key| *key == entry.key) {
                owner.reset(action);
                self.active.remove(i);
            }
            false
        });
    }
    fn compatible(&self, entry: &Entry) -> bool {
        self.entries.iter().all(|other| {
            other.key == entry.key
                || !self.active.contains(&other.key)
                || if entry.priority >= other.priority {
                    entry.mutex & other.mutex == 0
                } else {
                    other.interruptible
                }
        })
    }
    pub fn active(&self, action: u16) -> bool {
        self.entries
            .iter()
            .any(|e| e.action == action && self.active.contains(&e.key))
    }
    #[cfg(test)]
    pub fn active_actions(&self) -> Vec<u16> {
        self.active
            .iter()
            .map(|key| {
                self.entries
                    .iter()
                    .find(|entry| entry.key == *key)
                    .unwrap()
                    .action
            })
            .collect()
    }
    pub fn tick(&mut self, owner: &mut impl Goals) {
        let setup = self.clock % 3 == 0;
        self.clock = self.clock.wrapping_add(1);
        if setup {
            for entry in &self.entries {
                if let Some(i) = self.active.iter().position(|key| *key == entry.key) {
                    if self.compatible(entry) && owner.should_continue(entry.action) {
                        continue;
                    }
                    owner.reset(entry.action);
                    self.active.remove(i);
                }
                if self.compatible(entry) && owner.should_start(entry.action) {
                    owner.start(entry.action);
                    self.active.push(entry.key);
                }
            }
        } else {
            self.active.retain(|key| {
                let action = self.entries.iter().find(|e| e.key == *key).unwrap().action;
                if owner.should_continue(action) {
                    true
                } else {
                    owner.reset(action);
                    false
                }
            });
        }
        for key in &self.active {
            let action = self.entries.iter().find(|e| e.key == *key).unwrap().action;
            owner.update(action);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::io::Read;
    struct Owner {
        ticks: i32,
        seeds: Vec<i32>,
        calls: Vec<Value>,
    }
    impl Goals for Owner {
        fn should_start(&mut self, a: u16) -> bool {
            self.calls.push(json!(["test", a]));
            (self.ticks * 7 + self.seeds[a as usize] + i32::from(a) * 11).rem_euclid(13) < 7
        }
        fn should_continue(&mut self, a: u16) -> bool {
            self.calls.push(json!(["continue", a]));
            (self.ticks * 3 + self.seeds[a as usize] + i32::from(a) * 17).rem_euclid(19) < 12
        }
        fn start(&mut self, a: u16) {
            self.calls.push(json!(["start", a]));
        }
        fn reset(&mut self, a: u16) {
            self.calls.push(json!(["reset", a]));
        }
        fn update(&mut self, a: u16) {
            self.calls.push(json!(["update", a]));
        }
    }
    #[test]
    fn entity_ai_tasks_callbacks_match_original_in_all_10368_updates() {
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(
            include_bytes!("../tests/fixtures/vanilla189-goal-selector.json.gz").as_slice(),
        )
        .read_to_end(&mut bytes)
        .unwrap();
        let cases: Value = serde_json::from_slice(&bytes).unwrap();
        let mut count = 0;
        for (index, row) in cases.as_array().unwrap().iter().enumerate() {
            let defs = row[0].as_array().unwrap();
            let mut owner = Owner {
                ticks: 0,
                seeds: defs.iter().map(|a| a[1].as_i64().unwrap() as i32).collect(),
                calls: Vec::new(),
            };
            let add = |selector: &mut Selector, id: u16, priority: i32| {
                let mask = defs[id as usize][0].as_i64().unwrap() as i32;
                let seed = defs[id as usize][1].as_i64().unwrap() as i32;
                selector.add(id, priority, mask, (i32::from(id) + seed) % 4 != 0);
            };
            let mut goals = Selector::default();
            for r in row[1].as_array().unwrap() {
                add(
                    &mut goals,
                    r[0].as_u64().unwrap() as u16,
                    r[1].as_i64().unwrap() as i32,
                );
            }
            for frame in row[2].as_array().unwrap() {
                owner.ticks += 1;
                owner.calls.clear();
                for edit in frame[0].as_array().unwrap() {
                    let action = edit[1].as_u64().unwrap() as u16;
                    if edit[0] == "add" {
                        add(&mut goals, action, edit[2].as_i64().unwrap() as i32);
                    } else {
                        goals.remove(action, &mut owner);
                    }
                }
                goals.tick(&mut owner);
                assert_eq!(
                    json!(&owner.calls),
                    frame[1],
                    "selector {index}, tick {}",
                    owner.ticks
                );
                count += 1;
            }
        }
        assert_eq!(count, 10368);
    }
}
