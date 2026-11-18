//! Original ChunkProviderServer load/provide/populate lifecycle. Block data,
//! lighting and generation live in Backend; reentrant world reads call this
//! controller again, preserving original first-population and neighbor order.
use crate::world::ChunkPos;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[path = "provider_structures.rs"]
mod structures;
#[allow(unused_imports)]
pub use structures::{NativeStructures, StructureStore};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkRecord {
    pub terrain_populated: bool,
    pub light_populated: bool,
    pub modified: bool,
    pub last_save_time: i64,
    pub inhabited_time: i64,
}
/// Backend owns actual chunk bytes, tiles/entities, shared World RNG and
/// population-generator objects. Callbacks may synchronously provide neighbors.
pub trait Backend: Sized {
    fn load_saved(&mut self, provider: &mut Provider, pos: ChunkPos)
        -> Result<Option<ChunkRecord>>;
    fn generate(&mut self, provider: &mut Provider, pos: ChunkPos) -> Result<ChunkRecord>;
    fn recreate_structures(&mut self, provider: &mut Provider, pos: ChunkPos) -> Result<()>;
    fn on_load(&mut self, provider: &mut Provider, pos: ChunkPos) -> Result<()>;
    /// Terrain-populated is already true on entry, just as func_150809_p.
    /// Return the original light-populated result; don't fabricate lit chunks.
    fn initialize_light(&mut self, provider: &mut Provider, pos: ChunkPos) -> Result<bool>;
    fn populate(&mut self, provider: &mut Provider, pos: ChunkPos) -> Result<()>;
    /// Existing populated chunks still get the original late monument hook.
    fn repopulate(&mut self, provider: &mut Provider, pos: ChunkPos) -> Result<bool>;
    fn modified(&mut self, _provider: &mut Provider, _pos: ChunkPos) -> Result<()> {
        Ok(())
    }
    fn load_failed(&mut self, _pos: ChunkPos, _message: &str) {}
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Provider {
    pub version: u32,
    pub chunk_load_override: bool,
    pub finding_spawn: bool,
    pub world_time: i64,
    #[serde(with = "chunk_records")]
    chunks: BTreeMap<ChunkPos, ChunkRecord>,
    loaded_order: Vec<ChunkPos>,
    /// Persisted successful population history, useful for exact event replay.
    pub population_order: Vec<ChunkPos>,
    /// Only a successful modified=true assignment is cached. Mutable record
    /// access and publication invalidate it; snapshots always start uncached.
    #[serde(skip, default)]
    last_modified_column: Option<ChunkPos>,
}
impl Default for Provider {
    fn default() -> Self {
        Self {
            version: 1,
            chunk_load_override: true,
            finding_spawn: false,
            world_time: 0,
            chunks: BTreeMap::new(),
            loaded_order: Vec::new(),
            population_order: Vec::new(),
            last_modified_column: None,
        }
    }
}
mod chunk_records {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        map: &BTreeMap<ChunkPos, ChunkRecord>,
        s: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        map.iter().collect::<Vec<_>>().serialize(s)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> std::result::Result<BTreeMap<ChunkPos, ChunkRecord>, D::Error> {
        Ok(Vec::<(ChunkPos, ChunkRecord)>::deserialize(d)?
            .into_iter()
            .collect())
    }
}
impl Provider {
    pub fn contains(&self, pos: ChunkPos) -> bool {
        self.chunks.contains_key(&pos)
    }
    pub fn record(&self, pos: ChunkPos) -> Option<&ChunkRecord> {
        self.chunks.get(&pos)
    }
    pub fn record_mut(&mut self, pos: ChunkPos) -> Option<&mut ChunkRecord> {
        self.last_modified_column = None;
        self.chunks.get_mut(&pos)
    }
    /// Mark block data modified without repeating a map lookup for consecutive
    /// writes in the same column. This does not suppress any backend callbacks.
    /// Returns false for an unpublished column, which is never cached.
    pub fn mark_block_modified(&mut self, pos: ChunkPos) -> bool {
        if self.last_modified_column == Some(pos) {
            return true;
        }
        let Some(record) = self.chunks.get_mut(&pos) else { return false; };
        record.modified = true;
        self.last_modified_column = Some(pos);
        true
    }
    pub fn loaded_order(&self) -> &[ChunkPos] {
        &self.loaded_order
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "Unsupported native provider snapshot version {}",
            self.version
        );
        ensure!(
            self.loaded_order.len() == self.chunks.len(),
            "Provider snapshot loaded order length mismatch"
        );
        let mut seen = std::collections::HashSet::new();
        ensure!(
            self.loaded_order
                .iter()
                .all(|p| self.chunks.contains_key(p) && seen.insert(p)),
            "Provider snapshot loaded order contains missing or duplicate chunk"
        );
        Ok(())
    }
    /// None is the original shared EmptyChunk fallback, not a newly generated
    /// empty real chunk. Backend world reads must preserve that distinction.
    pub fn provide<B: Backend>(
        &mut self,
        backend: &mut B,
        pos: ChunkPos,
    ) -> Result<Option<ChunkPos>> {
        if self.contains(pos) {
            return Ok(Some(pos));
        }
        if self.finding_spawn || self.chunk_load_override {
            self.load(backend, pos)?;
            Ok(Some(pos))
        } else {
            Ok(None)
        }
    }
    pub fn load<B: Backend>(&mut self, backend: &mut B, pos: ChunkPos) -> Result<()> {
        if self.contains(pos) {
            return Ok(());
        }
        let saved = match self.load_file(backend, pos) {
            Ok(saved) => saved,
            Err(e) => {
                backend.load_failed(pos, &e.to_string());
                None
            }
        };
        let record = match saved {
            Some(record) => record,
            None => backend.generate(self, pos)?,
        };
        // Publication precedes onChunkLoad and all possible recursive reads.
        self.last_modified_column = None;
        self.chunks.insert(pos, record);
        self.loaded_order.push(pos);
        backend.on_load(self, pos)?;
        self.populate_neighbors(backend, pos)
    }
    fn load_file<B: Backend>(
        &mut self,
        backend: &mut B,
        pos: ChunkPos,
    ) -> Result<Option<ChunkRecord>> {
        let mut saved = backend.load_saved(self, pos)?;
        if let Some(record) = &mut saved {
            record.last_save_time = self.world_time;
            // Original loadChunkFromFile catches both loader errors and
            // recreateStructures errors, then regenerates the chunk.
            backend.recreate_structures(self, pos)?;
        }
        Ok(saved)
    }
    pub fn populate<B: Backend>(&mut self, backend: &mut B, pos: ChunkPos) -> Result<()> {
        ensure!(
            self.provide(backend, pos)?.is_some(),
            "Cannot populate the disabled-load dummy chunk"
        );
        if self.chunks[&pos].terrain_populated {
            return Ok(());
        }
        self.chunks.get_mut(&pos).unwrap().terrain_populated = true;
        self.chunks.get_mut(&pos).unwrap().light_populated = true;
        let light = backend.initialize_light(self, pos)?;
        self.chunks.get_mut(&pos).unwrap().light_populated = light;
        backend.populate(self, pos)?;
        self.population_order.push(pos);
        self.mark_modified(backend, pos)
    }
    fn mark_modified<B: Backend>(&mut self, backend: &mut B, pos: ChunkPos) -> Result<()> {
        assert!(self.mark_block_modified(pos), "Cannot mark missing provider chunk {pos:?}");
        backend.modified(self, pos)
    }
    fn populate_neighbors<B: Backend>(&mut self, backend: &mut B, (x, z): ChunkPos) -> Result<()> {
        // Snapshot before the first hook. Reentrant population may load more
        // neighbors, but Java retains these original eight booleans.
        let north = self.contains((x, z - 1));
        let east = self.contains((x + 1, z));
        let south = self.contains((x, z + 1));
        let west = self.contains((x - 1, z));
        let nw = self.contains((x - 1, z - 1));
        let se = self.contains((x + 1, z + 1));
        let sw = self.contains((x - 1, z + 1));
        let ne = self.contains((x + 1, z - 1));
        for (ready, pos) in [
            (east && south && se, (x, z)),
            (west && south && sw, (x - 1, z)),
            (north && east && ne, (x, z - 1)),
            (nw && north && west, (x - 1, z - 1)),
        ] {
            if !ready {
                continue;
            }
            if !self.chunks[&pos].terrain_populated {
                self.populate(backend, pos)?;
            } else if backend.repopulate(self, pos)? {
                self.mark_modified(backend, pos)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    #[derive(Default)]
    struct TraceBackend {
        saved: BTreeMap<ChunkPos, bool>,
        nested: Vec<[i32; 4]>,
        repopulate: bool,
        events: Vec<Value>,
        reset_modified: bool,
        modified_neighbor: Option<ChunkPos>,
    }
    impl TraceBackend {
        fn event(&mut self, name: &str, p: ChunkPos) {
            self.events.push(json!([name, p.0, p.1]));
        }
    }
    impl Backend for TraceBackend {
        fn load_saved(&mut self, _: &mut Provider, p: ChunkPos) -> Result<Option<ChunkRecord>> {
            self.event("load_file", p);
            ensure!(p != (90, 90), "oracle load failure");
            Ok(self.saved.get(&p).map(|&terrain_populated| ChunkRecord {
                terrain_populated,
                ..Default::default()
            }))
        }
        fn generate(&mut self, _: &mut Provider, p: ChunkPos) -> Result<ChunkRecord> {
            self.event("generate", p);
            Ok(ChunkRecord::default())
        }
        fn recreate_structures(&mut self, _: &mut Provider, p: ChunkPos) -> Result<()> {
            self.event("recreate", p);
            ensure!(p != (91, 90), "oracle recreate failure");
            Ok(())
        }
        fn on_load(&mut self, provider: &mut Provider, p: ChunkPos) -> Result<()> {
            assert!(provider.contains(p));
            self.event("on_load", p);
            Ok(())
        }
        fn initialize_light(&mut self, provider: &mut Provider, p: ChunkPos) -> Result<bool> {
            assert!(provider.record(p).unwrap().terrain_populated);
            self.event("light", p);
            Ok(false)
        }
        fn populate(&mut self, provider: &mut Provider, p: ChunkPos) -> Result<()> {
            self.event("populate", p);
            for row in self.nested.clone() {
                if (row[0], row[1]) == p {
                    provider.provide(self, (row[2], row[3]))?;
                }
            }
            Ok(())
        }
        fn repopulate(&mut self, _: &mut Provider, p: ChunkPos) -> Result<bool> {
            self.event("repopulate", p);
            Ok(self.repopulate)
        }
        fn modified(&mut self, provider: &mut Provider, p: ChunkPos) -> Result<()> {
            assert!(provider.record(p).unwrap().modified);
            self.event("modified", p);
            if let Some(neighbor) = self.modified_neighbor.take() {
                provider.provide(self, neighbor)?;
                assert!(provider.mark_block_modified(neighbor));
            }
            if self.reset_modified {
                provider.record_mut(p).unwrap().modified = false;
            }
            Ok(())
        }
    }
    #[test]
    fn block_modified_cache_invalidates_mutable_record_borrows_and_publication() {
        let mut provider = Provider::default();
        let mut backend = TraceBackend::default();
        let a = (0, 0);
        let b = (8, 8);
        provider.load(&mut backend, a).unwrap();
        assert!(provider.mark_block_modified(a));
        assert!(provider.mark_block_modified(a));
        assert_eq!(provider.last_modified_column, Some(a));

        provider.record_mut(a).unwrap().modified = false;
        assert_eq!(provider.last_modified_column, None);
        assert!(provider.mark_block_modified(a));
        assert!(provider.record(a).unwrap().modified);
        assert!(provider.record_mut(b).is_none());
        assert_eq!(provider.last_modified_column, None);
        assert!(provider.mark_block_modified(a));

        // An absent column cannot become a cached successful mark. Publishing
        // a saved/generated record also invalidates the earlier column cache.
        assert!(!provider.mark_block_modified(b));
        provider.load(&mut backend, b).unwrap();
        assert_eq!(provider.last_modified_column, None);
        assert!(!provider.record(b).unwrap().modified);
        assert!(provider.mark_block_modified(b));
        assert!(provider.record(b).unwrap().modified);
        provider.validate().unwrap();
    }
    #[test]
    fn restored_unmodified_record_is_marked_even_when_external_dirty_set_contains_it() {
        let mut provider = Provider::default();
        let mut backend = TraceBackend::default();
        let pos = (-3, 7);
        provider.load(&mut backend, pos).unwrap();
        assert!(provider.mark_block_modified(pos));
        let mut snapshot = json!([provider, [pos]]);
        assert!(snapshot[0].get("last_modified_column").is_none());
        // Runtime dirty publication state is independent of Chunk.modified.
        // A restored dirty column can legitimately have modified=false.
        snapshot[0]["chunks"][0][1]["modified"] = json!(false);
        let (mut restored, dirty): (Provider, Vec<ChunkPos>) =
            serde_json::from_value(snapshot).unwrap();
        assert_eq!(dirty, vec![pos]);
        assert_eq!(restored.last_modified_column, None);
        assert!(!restored.record(pos).unwrap().modified);
        assert!(restored.mark_block_modified(pos));
        assert!(restored.record(pos).unwrap().modified);
        restored.validate().unwrap();
    }
    #[test]
    fn modified_callbacks_repeat_after_cached_marks_and_reentrant_record_changes() {
        let mut provider = Provider::default();
        let pos = (0, 0);
        let neighbor = (8, 8);
        let mut backend = TraceBackend {
            reset_modified: true, modified_neighbor: Some(neighbor),
            ..Default::default()
        };
        provider.load(&mut backend, pos).unwrap();
        provider.mark_modified(&mut backend, pos).unwrap();
        assert!(provider.record(neighbor).unwrap().modified);
        assert!(!provider.record(pos).unwrap().modified);
        assert_eq!(provider.last_modified_column, None);
        provider.mark_modified(&mut backend, pos).unwrap();
        assert!(!provider.record(pos).unwrap().modified);
        backend.reset_modified = false;
        provider.mark_modified(&mut backend, pos).unwrap();
        provider.mark_modified(&mut backend, pos).unwrap();
        assert!(provider.record(pos).unwrap().modified);
        assert_eq!(backend.events.iter().filter(|event| event[0] == "modified").count(), 4);
        provider.validate().unwrap();
    }
    #[test]
    fn original_provider_event_order_and_snapshot_resume_match() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-provider.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            // Restore the controller at every possible operation boundary.
            for checkpoint in 0..=case["operations"].as_array().unwrap().len() {
                let mut p = Provider::default();
                let mut b = TraceBackend {
                    saved: case["saved"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| {
                            (
                                (v[0].as_i64().unwrap() as i32, v[1].as_i64().unwrap() as i32),
                                v[2].as_i64().unwrap() != 0,
                            )
                        })
                        .collect(),
                    nested: serde_json::from_value(case["nested"].clone()).unwrap(),
                    repopulate: case["repopulate_result"].as_bool().unwrap(),
                    events: Vec::new(),
                    ..Default::default()
                };
                let mut results = Vec::new();
                for (i, op) in case["operations"].as_array().unwrap().iter().enumerate() {
                    if i == checkpoint {
                        p = serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();
                        p.validate().unwrap();
                    }
                    let n = |i: usize| op[i].as_i64().unwrap_or(0) as i32;
                    let pos = (n(1), n(2));
                    results.push(match n(0) {
                        0 => {
                            p.load(&mut b, pos).unwrap();
                            json!(true)
                        }
                        1 => json!(p.provide(&mut b, pos).unwrap().is_some()),
                        2 => {
                            p.populate(&mut b, pos).unwrap();
                            json!(true)
                        }
                        3 => {
                            p.chunk_load_override = n(1) != 0;
                            Value::Null
                        }
                        4 => {
                            p.finding_spawn = n(1) != 0;
                            Value::Null
                        }
                        _ => unreachable!(),
                    });
                }
                assert_eq!(
                    json!(results),
                    case["results"],
                    "{} checkpoint {checkpoint}",
                    case["name"]
                );
                assert_eq!(
                    json!(b.events),
                    case["events"],
                    "{} checkpoint {checkpoint}",
                    case["name"]
                );
                let loaded = p
                    .loaded_order()
                    .iter()
                    .map(|&(x, z)| {
                        let c = p.record((x, z)).unwrap();
                        json!([x, z, c.terrain_populated, c.light_populated, c.modified])
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    json!(loaded),
                    case["loaded"],
                    "{} checkpoint {checkpoint}",
                    case["name"]
                );
                p.validate().unwrap();
            }
        }
    }
}
