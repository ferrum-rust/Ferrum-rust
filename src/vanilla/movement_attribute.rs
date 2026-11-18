//! Original movement attribute operations and Java 8 HashSet traversal.
//! Native vanilla movement has at most sprint/speed/slowness modifiers. The
//! ordinary bucket/resize path also handles server-supplied modifier lists;
//! pathological tree bins with equal UUID hash codes have JVM-identity-dependent
//! traversal and are outside the bit-for-bit fixture claim.
use anyhow::{ensure, Result};
use std::cell::Cell;
pub const SPRINT: [u64; 2] = [0x662a6b8dda3e4c1c, 0x881396ea6097278d];
pub const SPEED: [u64; 2] = [0x91aeaa56376b4498, 0x935b2f7f68070635];
pub const SLOWNESS: [u64; 2] = [0x7107de5e7ce84030, 0x940e514c1f160890];
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Modifier {
    pub uuid: [u64; 2],
    pub amount: f64,
    pub operation: u8,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub name: String,
    pub base: f64,
    pub modifiers: Vec<Modifier>,
}
fn hash(uuid: [u64; 2]) -> usize {
    let bits = uuid[0] ^ uuid[1];
    let hash = (bits ^ (bits >> 32)) as u32;
    (hash ^ (hash >> 16)) as usize
}
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
struct ModifierSet {
    buckets: Vec<Vec<Modifier>>,
    len: usize,
}
impl ModifierSet {
    fn insert(&mut self, modifier: Modifier) {
        if self.buckets.is_empty() {
            self.buckets = vec![Vec::new(); 16];
        }
        let index = hash(modifier.uuid) & (self.buckets.len() - 1);
        self.buckets[index].push(modifier);
        self.len += 1;
        if self.len > self.buckets.len() * 3 / 4
            || (self.buckets[index].len() >= 9 && self.buckets.len() < 64)
        {
            let old = std::mem::take(&mut self.buckets);
            self.buckets = vec![Vec::new(); old.len() * 2];
            let mask = self.buckets.len() - 1;
            for modifier in old.into_iter().flatten() {
                self.buckets[hash(modifier.uuid) & mask].push(modifier);
            }
        }
    }
    fn contains(&self, uuid: [u64; 2]) -> bool {
        !self.buckets.is_empty()
            && self.buckets[hash(uuid) & (self.buckets.len() - 1)]
                .iter()
                .any(|m| m.uuid == uuid)
    }
    fn remove(&mut self, uuid: [u64; 2]) {
        if self.buckets.is_empty() {
            return;
        }
        let index = hash(uuid) & (self.buckets.len() - 1);
        if let Some(i) = self.buckets[index].iter().position(|m| m.uuid == uuid) {
            self.buckets[index].remove(i);
            self.len -= 1;
        }
    }
    fn clear(&mut self) {
        self.buckets.iter_mut().for_each(Vec::clear);
        self.len = 0;
    }
    fn copy_ordered(&self) -> Vec<Modifier> {
        // HashSet(Collection): max((int)(size/.75f)+1,16), rounded to a power
        // of two. This copy can reorder a set of exactly twelve modifiers.
        let requested = ((self.len as f32 / 0.75) as usize + 1).max(16);
        let mut copy = Self {
            buckets: vec![Vec::new(); requested.next_power_of_two()],
            len: 0,
        };
        for modifier in self.buckets.iter().flatten() {
            copy.insert(modifier.clone());
        }
        copy.buckets.into_iter().flatten().collect()
    }
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Attribute {
    base: f64,
    modifiers: [ModifierSet; 3],
    #[serde(skip)]
    cached: Cell<Option<f64>>,
}
impl Default for Attribute {
    fn default() -> Self {
        Self {
            base: 0.1f32 as f64,
            modifiers: std::array::from_fn(|_| ModifierSet::default()),
            cached: Cell::new(None),
        }
    }
}
impl Attribute {
    pub fn apply(&mut self, modifier: Modifier) -> Result<()> {
        ensure!(
            modifier.operation <= 2,
            "Invalid attribute operation {}",
            modifier.operation
        );
        ensure!(
            !self.modifiers.iter().any(|s| s.contains(modifier.uuid)),
            "Modifier is already applied on this attribute"
        );
        self.modifiers[modifier.operation as usize].insert(modifier);
        self.cached.set(None);
        Ok(())
    }
    pub fn remove(&mut self, uuid: [u64; 2]) {
        for set in &mut self.modifiers {
            set.remove(uuid);
        }
        self.cached.set(None);
    }
    pub fn replace(&mut self, snapshot: &Snapshot) -> Result<()> {
        self.base = snapshot.base;
        self.cached.set(None);
        self.modifiers.iter_mut().for_each(ModifierSet::clear);
        for modifier in &snapshot.modifiers {
            self.apply(modifier.clone())?;
        }
        Ok(())
    }
    pub fn set_modifier(&mut self, uuid: [u64; 2], amount: Option<f64>) {
        self.remove(uuid);
        if let Some(amount) = amount {
            self.apply(Modifier {
                uuid,
                amount,
                operation: 2,
            })
            .expect("known original modifier operation");
        }
    }
    pub fn value(&self) -> f64 {
        if let Some(value) = self.cached.get() {
            return value;
        }
        let mut base = self.base;
        for m in self.modifiers[0].copy_ordered() {
            base += m.amount;
        }
        let mut value = base;
        for m in self.modifiers[1].copy_ordered() {
            value += base * m.amount;
        }
        for m in self.modifiers[2].copy_ordered() {
            value *= 1. + m.amount;
        }
        let value = value.clamp(0., 1024.);
        self.cached.set(Some(value));
        value
    }
    #[cfg(test)]
    pub fn ordered(&self, operation: usize) -> Vec<Modifier> {
        self.modifiers[operation].copy_ordered()
    }
}

/// S20PacketEntityProperties' wire format. Negative signed collection counts
/// execute zero iterations in the original reader; invalid operations fail in
/// AttributeModifier's constructor, while duplicate UUIDs fail at application.
pub fn read_properties(reader: &mut crate::protocol::Reader<'_>) -> Result<(i32, Vec<Snapshot>)> {
    let entity_id = reader.varint()?;
    let count = reader.i32()?;
    let mut snapshots = Vec::new();
    for _ in 0..count {
        let name = reader.string(64)?;
        let base = reader.f64()?;
        let count = reader.varint()?;
        let mut modifiers = Vec::new();
        for _ in 0..count {
            let uuid = [reader.i64()? as u64, reader.i64()? as u64];
            let amount = reader.f64()?;
            let operation = reader.i8()?;
            ensure!(
                (0..=2).contains(&operation),
                "Invalid attribute operation {operation}"
            );
            modifiers.push(Modifier {
                uuid,
                amount,
                operation: operation as u8,
            });
        }
        snapshots.push(Snapshot {
            name,
            base,
            modifiers,
        });
    }
    reader.finish()?;
    Ok((entity_id, snapshots))
}
#[cfg(test)]
#[path = "movement_attribute_tests.rs"]
mod tests;
