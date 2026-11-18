//! Exact native callback-boundary checkpoint. This preserves in-memory Java-map
//! traversal and generator state; it does not silently apply an Anvil restart.
use super::*;
use crate::{block_states::BlockStates, container::Container, villagers::ImportedEntity, world::Chunk};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChunkSnapshot {
    pub position: ChunkPos,
    pub blocks: BlockStates,
    pub biomes: Vec<u8>,
    pub top: i32,
    pub containers: Vec<(IVec3, Container)>,
    pub entities: Vec<ImportedEntity>,
    #[serde(default)]
    pub pistons: Vec<([i32; 3], super::super::collision_shapes::PistonTile)>,
    #[serde(default, with = "tile_pairs")]
    pub tiles: Vec<([i32; 3], Arc<crate::vanilla::drops::DropNbt>)>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    pub version: u32,
    pub hooks: String,
    pub seed: i64,
    pub map_features: bool,
    pub provider: Provider,
    pub chunks: Vec<ChunkSnapshot>,
    pub spawn: glam::Vec3,
    #[serde(default)]
    pub world_spawn: Option<IVec3>,
    #[serde(default)]
    pub initial_player: Option<InitialPlayer>,
    pub label: String,
    pub save_path: PathBuf,
    pub edits: Vec<([i32; 3], u16)>,
    pub imported: bool,
    pub light: Lighting,
    pub structures: NativeStructures,
    pub ticks: BlockTicks,
    pub provider_random: u64,
    pub world_random: u64,
    pub decorators: Vec<(u8, Decorator)>,
    pub double_plant_type: u16,
    pub plains_counts: Vec<(u8, (i32, i32))>,
    pub precipitation: Vec<(ChunkPos, Vec<i32>)>,
    pub structure_entities: structures::StructureEntities,
    /// Schema 2 recorded generated loot; migrate it before publishing schema 3.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub live_containers: Vec<super::super::scattered::GeneratedContainer>,
    #[serde(default)]
    pub tile_slots: Vec<LiveContainer>,
    #[serde(default)]
    pub tile_revision: u64,
    #[serde(default)]
    pub player_inventory: Option<NativePlayerInventory>,
    pub live_spawners: Vec<([i32; 3], String)>,
    #[serde(default)]
    pub falling: FallingManager,
    #[serde(default)]
    pub entities: Option<NativeEntities>,
    pub animals: Vec<super::super::spawning::AnimalSpawn>,
    pub drops: Vec<super::super::drops::DropEvent>,
    pub drop_tiles: Vec<([i32; 3], super::super::drops::DropTile)>,
    pub do_tile_drops: bool,
    #[serde(default)]
    pub fire_environment: FireEnvironment,
    #[serde(default)]
    pub pending_tnt: Vec<PendingTnt>,
    pub output: Population,
    pub changed: Vec<IVec3>,
    pub falling_drops: Vec<(glam::Vec3, u16)>,
    pub washed: Vec<(IVec3, u16)>,
    pub events: Vec<(String, ChunkPos)>,
    pub dirty_chunks: Vec<ChunkPos>,
}
impl RuntimeSnapshot {
    fn upgrade(&mut self) -> Result<()> {
        if self.version == 2 {
            ensure!(
                self.tile_slots.is_empty()
                    && self.tile_revision == 0
                    && self.player_inventory.is_none(),
                "Mixed native inventory checkpoint schemas"
            );
            self.live_containers.sort_unstable_by_key(|c| c.position);
            for old in self.live_containers.drain(..) {
                self.tile_slots.push(LiveContainer::generated(
                    &old,
                    inventories::next_tile_revision(&mut self.tile_revision),
                ));
            }
            self.version = 3;
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 3,
            "Unsupported runtime checkpoint version {}",
            self.version
        );
        self.provider.validate()?;
        ensure!(self.fire_environment.difficulty <= 3, "Invalid native fire difficulty");
        let mut pending_ids = HashSet::new();
        ensure!(
            self.pending_tnt.iter().all(|t| t.fuse == 80
                && t.position.is_finite()
                && t.velocity.is_finite()
                && t.entity_random < 1 << 48
                && pending_ids.insert(t.id)),
            "Invalid pending primed TNT constructor"
        );
        ensure!(
            self.pending_tnt.windows(2).all(|t| t[0].insertion_index < t[1].insertion_index),
            "Invalid pending primed TNT insertion order"
        );
        self.falling.validate()?;
        if let Some(e) = &self.entities {
            e.validate(&self.falling, self.drops.len())?;
        }
        ensure!(
            self.live_containers.is_empty(),
            "Schema 3 contains unmigrated legacy tile slots"
        );
        if let Some(player) = &self.player_inventory {
            player.validate()?;
        }
        ensure!(
            self.structures.seed == self.seed,
            "Checkpoint structure seed differs from world seed"
        );
        ensure!(
            self.provider_random < 1 << 48 && self.world_random < 1 << 48,
            "Invalid 48-bit Random state"
        );
        ensure!(
            self.provider.loaded_order().len() == self.chunks.len(),
            "Checkpoint provider/chunk count differs"
        );
        let mut seen = HashSet::new();
        for chunk in &self.chunks {
            ensure!(
                seen.insert(chunk.position) && self.provider.contains(chunk.position),
                "Duplicate or unpublished checkpoint chunk {:?}",
                chunk.position
            );
            ensure!(
                chunk.blocks.len() == 65536
                    && chunk.biomes.len() == 256
                    && (0..=256).contains(&chunk.top),
                "Invalid checkpoint chunk dimensions"
            );
            let light = self
                .light
                .chunks
                .get(&chunk.position)
                .ok_or_else(|| anyhow::anyhow!("Checkpoint chunk lacks lighting"))?;
            ensure!(
                light.height.len() == 256
                    && light.gaps.len() == 256
                    && light.sky.len() == 65536
                    && light.block.len() == 65536,
                "Invalid checkpoint light dimensions"
            );
            ensure!(
                light.sky.try_with_bytes(|values| values.iter().all(|v| *v <= 15))?
                    && light.block.try_with_bytes(|values| values.iter().all(|v| *v <= 15))?,
                "Invalid checkpoint light nibble"
            );
        }
        ensure!(
            self.light.chunks.len() == self.chunks.len(),
            "Checkpoint has orphan lighting chunks"
        );
        let mut precip = HashSet::new();
        let mut tiles = HashSet::new();
        let chunk_by_pos: HashMap<ChunkPos, &ChunkSnapshot> =
            if !self.tile_slots.is_empty() || !self.live_spawners.is_empty() {
                self.chunks.iter().map(|c| (c.position, c)).collect()
            } else {
                HashMap::new()
            };
        for tile in &self.tile_slots {
            ensure!(
                tile.slots
                    .iter()
                    .flatten()
                    .all(|s| s.id != 0 && s.count != 0),
                "Invalid empty live container stack"
            );
            ensure!(
                tile.revision > 0 && tile.revision <= self.tile_revision,
                "Invalid native tile revision"
            );
            ensure!(tiles.insert(tile.position), "Duplicate live container");
            let p = IVec3::from_array(tile.position);
            ensure!(valid(p), "Invalid live container position");
            let chunk = chunk_by_pos
                .get(&cp(p))
                .copied()
                .ok_or_else(|| anyhow::anyhow!("Live container chunk missing"))?;
            ensure!(
                chunk.blocks.try_with_slice(|blocks|
                    blocks[Chunk::index(p.x.rem_euclid(16), p.y, p.z.rem_euclid(16))] >> 4)?
                    == tile.block,
                "Live container block mismatch"
            );
            let size = match tile.block {
                54 | 146 => 27,
                23 | 158 => 9,
                _ => 0,
            };
            ensure!(
                size > 0 && tile.slots.len() == size,
                "Unsupported live container dimensions"
            );
        }
        for (position, _) in &self.live_spawners {
            ensure!(tiles.insert(*position), "Duplicate live tile");
            let p = IVec3::from_array(*position);
            ensure!(valid(p), "Invalid live spawner position");
            let chunk = chunk_by_pos
                .get(&cp(p))
                .copied()
                .ok_or_else(|| anyhow::anyhow!("Live spawner chunk missing"))?;
            ensure!(
                chunk.blocks.try_with_slice(|blocks|
                    blocks[Chunk::index(p.x.rem_euclid(16), p.y, p.z.rem_euclid(16))] >> 4)? == 52,
                "Live spawner block mismatch"
            );
        }
        ensure!(
            self.precipitation.len() == self.chunks.len()
                && self
                    .precipitation
                    .iter()
                    .all(|(p, v)| seen.contains(p) && precip.insert(*p) && v.len() == 256),
            "Checkpoint precipitation cache mismatch"
        );
        Ok(())
    }
}
impl Runtime {
    pub fn checkpoint(&self) -> Result<RuntimeSnapshot> {
        let b = &self.backend;
        ensure!(
            b.active_decorators.is_empty(),
            "Cannot checkpoint a suspended biome decorator"
        );
        let snapshot = RuntimeSnapshot {
            version: 3,
            hooks: b.hooks.key().into(),
            seed: b.seed,
            map_features: b.map_features,
            provider: self.provider.clone(),
            chunks: self
                .provider
                .loaded_order()
                .iter()
                .map(|&p| {
                    let c = &b.world.chunks[&p];
                    ChunkSnapshot {
                        position: p,
                        blocks: c.blocks.clone(),
                        biomes: c.biomes.to_vec(),
                        top: c.top,
                        containers: c.containers.clone(),
                        entities: c.entities.clone(),
                        pistons: {
                            let mut tiles =
                                c.pistons.iter().map(|(p, t)| (*p, *t)).collect::<Vec<_>>();
                            tiles.sort_unstable_by_key(|(p, _)| *p);
                            tiles
                        },
                        tiles: {
                            let mut entries = c
                                .tiles
                                .iter()
                                .map(|(&p, t)| (p, t.clone()))
                                .collect::<Vec<_>>();
                            entries.sort_unstable_by_key(|(p, _)| *p);
                            entries
                        },
                    }
                })
                .collect(),
            spawn: b.world.spawn,
            world_spawn: b.world_spawn,
            initial_player: b.initial_player,
            label: b.world.label.clone(),
            save_path: b.world.save_path.clone(),
            edits: b.world.edits.iter().map(|(p, s)| (*p, *s)).collect(),
            imported: b.world.imported,
            light: b.light.clone(),
            structures: b.structures.clone(),
            ticks: b.ticks.clone(),
            provider_random: b.provider_random.state(),
            world_random: b.world_random.state(),
            decorators: b
                .decorators
                .iter()
                .map(|(id, d)| (*id, d.clone()))
                .collect(),
            double_plant_type: b.double_plant_type,
            plains_counts: b.plains_counts.iter().map(|(k, v)| (*k, *v)).collect(),
            precipitation: b
                .precipitation
                .iter()
                .map(|(p, v)| (*p, v.clone()))
                .collect(),
            structure_entities: b.structure_entities.clone(),
            falling: b.falling.clone(),
            entities: Some(b.entities.clone()),
            live_containers: Vec::new(),
            tile_slots: b.live_containers.values().cloned().collect(),
            tile_revision: b.tile_revision,
            player_inventory: b.player_inventory.clone(),
            live_spawners: b
                .live_spawners
                .iter()
                .map(|(p, v)| (*p, v.clone()))
                .collect(),
            animals: b.animals.clone(),
            drops: b.drops.clone(),
            drop_tiles: b.drop_tiles.iter().map(|(p, t)| (*p, t.clone())).collect(),
            do_tile_drops: b.do_tile_drops,
            fire_environment: b.fire_environment,
            pending_tnt: b.pending_tnt.clone(),
            output: b.output.clone(),
            changed: b.block_effects.changed.clone(),
            falling_drops: b.block_effects.drops.clone(),
            washed: b.block_effects.washed.clone(),
            events: b.events.clone(),
            dirty_chunks: b.dirty_chunks.iter().copied().collect(),
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
    pub fn resume(mut snapshot: RuntimeSnapshot, hooks: Arc<dyn Hooks>) -> Result<Self> {
        snapshot.upgrade()?;
        snapshot.validate()?;
        ensure!(
            snapshot.hooks == hooks.key(),
            "Runtime checkpoint population hook differs: {} vs {}",
            snapshot.hooks,
            hooks.key()
        );
        let mut runtime = Self::new(snapshot.seed, 0, snapshot.map_features, hooks);
        runtime.provider = snapshot.provider;
        let b = &mut runtime.backend;
        b.world.chunks = snapshot
            .chunks
            .into_iter()
            .map(|c| -> Result<_> {
                Ok((
                    c.position,
                    Arc::new(Chunk {
                        random_tick_sections: Default::default(),
                        scheduled_tick_blocks: Default::default(),
                        partial_light_sections: Default::default(),
                        skulls: Default::default(),
                        signs: Default::default(),
                        // Rebuild the original array-order index without retaining
                        // a hydrated cache in an in-memory cold checkpoint.
                        hoppers: c.blocks.try_with_slice(|blocks| blocks.iter().enumerate()
                            .filter(|(_, s)| **s >> 4 == 154)
                            .map(|(i, _)| [(i & 15) as i32, (i >> 8) as i32, ((i >> 4) & 15) as i32]).collect())?,
                        blocks: c.blocks,
                        biomes: c.biomes.try_into().unwrap(),
                        top: c.top,
                        containers: c.containers,
                        entities: c.entities,
                        pistons: c.pistons.into_iter().collect(),
                        tiles: c.tiles.into_iter().collect(),
                        light: None,
                        visual_light: None,
                        light_edits: Vec::new(),
                    }),
                ))
            })
            .collect::<Result<_>>()?;
        b.world.spawn = snapshot.spawn;
        b.world_spawn = snapshot.world_spawn;
        b.initial_player = snapshot.initial_player;
        b.world.label = snapshot.label;
        b.world.save_path = snapshot.save_path;
        b.world.edits = snapshot.edits.into_iter().collect();
        b.world.imported = snapshot.imported;
        b.light = snapshot.light;
        b.light.invalidate_render_cache();
        b.structures = snapshot.structures;
        b.ticks = snapshot.ticks;
        b.provider_random = JavaRandom::from_state(snapshot.provider_random);
        b.world_random = JavaRandom::from_state(snapshot.world_random);
        b.decorators = snapshot.decorators.into_iter().collect();
        b.double_plant_type = snapshot.double_plant_type;
        b.plains_counts = snapshot.plains_counts.into_iter().collect();
        b.precipitation = snapshot.precipitation.into_iter().collect();
        b.structure_entities = snapshot.structure_entities;
        b.falling = snapshot.falling;
        b.entities = snapshot
            .entities
            .unwrap_or_else(|| NativeEntities::legacy(&b.falling, snapshot.drops.len()));
        b.entities
            .xp_owned_after
            .get_or_insert(snapshot.drops.len());
        b.live_containers = snapshot
            .tile_slots
            .into_iter()
            .map(|c| (c.position, c))
            .collect();
        b.tile_revision = snapshot.tile_revision;
        b.player_inventory = snapshot.player_inventory;
        b.live_spawners = snapshot.live_spawners.into_iter().collect();
        b.animals = snapshot.animals;
        b.drops = snapshot.drops;
        b.drop_tiles = snapshot.drop_tiles.into_iter().collect();
        b.do_tile_drops = snapshot.do_tile_drops;
        b.fire_environment = snapshot.fire_environment;
        b.pending_tnt = snapshot.pending_tnt;
        b.output = snapshot.output;
        b.block_effects = TickResult {
            changed: snapshot.changed,
            drops: snapshot.falling_drops,
            washed: snapshot.washed,
            ..TickResult::default()
        };
        b.events = snapshot.events;
        b.dirty_chunks = snapshot.dirty_chunks.into_iter().collect();
        Ok(runtime)
    }
}
mod tile_pairs {
    use super::*;
    use crate::vanilla::drops::DropNbt;
    type TilePair = ([i32; 3], Arc<DropNbt>);
    #[derive(Serialize, Deserialize)]
    struct Binary(#[serde(with = "crate::vanilla::item_nbt::optional_arc")] Option<Arc<DropNbt>>);
    pub fn serialize<S: serde::Serializer>(
        tiles: &[TilePair],
        s: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        tiles
            .iter()
            .map(|(p, t)| (*p, Binary(Some(t.clone()))))
            .collect::<Vec<_>>()
            .serialize(s)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> std::result::Result<Vec<TilePair>, D::Error> {
        let mut seen = HashSet::new();
        Vec::<([i32; 3], Binary)>::deserialize(d)?
            .into_iter()
            .map(|(p, t)| {
                if !(0..16).contains(&p[0])
                    || !(0..256).contains(&p[1])
                    || !(0..16).contains(&p[2])
                    || !seen.insert(p)
                {
                    return Err(serde::de::Error::custom(
                        "Invalid checkpoint tile coordinates",
                    ));
                }
                t.0.map(|t| (p, t))
                    .ok_or_else(|| serde::de::Error::custom("Missing checkpoint tile NBT"))
            })
            .collect()
    }
}
pub(super) mod position_map {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        map: &BTreeMap<[i32; 3], String>,
        s: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        map.iter().collect::<Vec<_>>().serialize(s)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> std::result::Result<BTreeMap<[i32; 3], String>, D::Error> {
        Ok(Vec::<([i32; 3], String)>::deserialize(d)?
            .into_iter()
            .collect())
    }
}

#[cfg(test)]
#[path = "runtime_snapshot_block_states_tests.rs"]
mod block_states_tests;
