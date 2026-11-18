//! Structure clips are centered eight blocks inside their already-loaded 2x2
//! population neighborhood. Preserve eventful writes, and fail explicitly if a
//! structure callback would require an unmodeled cascading chunk load.
use super::*;
use crate::vanilla::{mineshaft, monument, scattered, stronghold, village};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Block-ID replacement destroys the old tile. Metadata-only orientation
/// changes retain it. Histories remain diagnostic; only these maps are live.
pub(super) fn tile_state_changed(
    containers: &mut BTreeMap<[i32; 3], LiveContainer>,
    tile_revision: &mut u64,
    spawners: &mut BTreeMap<[i32; 3], String>,
    p: [i32; 3],
    old: u16,
    new: u16,
) {
    if old >> 4 == new >> 4 {
        return;
    }
    containers.remove(&p);
    spawners.remove(&p);
    let id = new >> 4;
    let size = match id {
        54 | 146 => 27,
        23 | 158 => 9,
        _ => 0,
    };
    if size > 0 {
        containers.insert(
            p,
            LiveContainer::empty(p, id, inventories::next_tile_revision(tile_revision), size),
        );
    }
    if id == 52 {
        spawners.insert(p, "Pig".into());
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StructureEntities {
    pub villagers: Vec<village::VillagerSpawn>,
    pub carts: Vec<mineshaft::ChestMinecart>,
    pub witches: Vec<[i32; 3]>,
    pub elders: Vec<[i32; 3]>,
    pub containers: Vec<scattered::GeneratedContainer>,
    #[serde(with = "super::snapshot::position_map")]
    pub spawners: BTreeMap<[i32; 3], String>,
}
pub(super) struct LoadedStructures<'a> {
    pub world: &'a mut World,
    pub record_overlay_edits: bool,
    pub light: &'a mut Lighting,
    pub random: &'a mut JavaRandom,
    pub ticks: BlockTicks,
    pub output: &'a mut StructureEntities,
    pub live_containers: &'a mut BTreeMap<[i32; 3], LiveContainer>,
    pub tile_revision: &'a mut u64,
    pub live_spawners: &'a mut BTreeMap<[i32; 3], String>,
    pub falling: &'a mut FallingManager,
    pub entities: &'a mut NativeEntities,
    pub hooks: Arc<dyn Hooks>,
    pub changes: &'a mut Population,
    pub precipitation: &'a mut BTreeMap<ChunkPos, Vec<i32>>,
    pub drops: &'a mut Vec<super::super::drops::DropEvent>,
    pub drop_tiles: &'a mut BTreeMap<[i32; 3], super::super::drops::DropTile>,
    pub do_tile_drops: bool,
}
impl LoadedStructures<'_> {
    fn read(&self, p: [i32; 3]) -> u16 {
        let p = IVec3::from_array(p);
        if !valid(p) {
            return 0;
        }
        assert!(
            self.world.chunks.contains_key(&cp(p)),
            "Structure callback requires a cascading chunk load at {p:?}"
        );
        self.world.get(p)
    }
    fn write(&mut self, p: [i32; 3], state: u16) {
        let mut ticks = self.ticks.share_scheduler();
        ticks.set_block(self, IVec3::from_array(p), state, false);
    }
    fn container(&mut self, p: [i32; 3], block: u16, slots: Vec<Option<LootStack>>) {
        self.output.containers.retain(|c| c.position != p);
        let source = scattered::GeneratedContainer {
            position: p,
            block,
            slots,
        };
        self.live_containers.insert(
            p,
            LiveContainer::generated(&source, inventories::next_tile_revision(self.tile_revision)),
        );
        self.output.containers.push(source);
    }
    fn spawner(&mut self, p: [i32; 3], entity: &str) {
        self.live_spawners.insert(p, entity.into());
        self.output.spawners.insert(p, entity.into());
        let pos = IVec3::from_array(p);
        if let Some(chunk) = self.world.chunks.get_mut(&cp(pos)) {
            let local = [pos.x.rem_euclid(16), pos.y, pos.z.rem_euclid(16)];
            let chunk = Arc::make_mut(chunk);
            let existing = chunk.tiles.get(&local).map(Arc::as_ref);
            let mut tile = crate::vanilla::middle_pick::tile_snapshot(52, pos, existing, None)
                .or_else(|| existing.cloned());
            if let Some(super::super::drops::DropNbt::Compound(tag)) = &mut tile {
                tag.insert(
                    "EntityId".into(),
                    super::super::drops::DropNbt::String(entity.to_string()),
                );
            }
            if let Some(tile) = tile {
                chunk.tiles.insert(local, Arc::new(tile));
            }
        }
    }
}
impl BlockAccess for LoadedStructures<'_> {
    fn spawn_falling(&mut self, p: IVec3, state: u16) -> bool {
        let random = self.hooks.falling_random_state(self.falling.next_id);
        let id = self.falling.next_id;
        self.entities.allocate_entity_id();
        let result = self.falling.spawn(p, state, random);
        if result {
            self.entities.order.push(EntityKey::Falling(id));
        }
        result
    }
    fn neighbor_changed(&mut self, p: IVec3, state: u16, source: u16) {
        plant_effects::changed(self, p, state);
        rail_effects::changed(self, p, state, source);
        fire::changed(self,p,state);
        if let Some((pos,id))=openables::changed(self,p,state,source){self.entities.effects.push(EntityEffect::WorldEvent{position:pos.to_array(),id});}
    }
    fn drop_block_item(&mut self, p: IVec3, state: u16) -> bool {
        let mut random = self.random.share();
        super::super::drops::drop_block(self, &mut random, p, state, 1., 0)
            .expect("Structure block drop failed");
        true
    }
    fn before_set_attempt(&mut self, p: IVec3) {
        if !valid(p) {
            return;
        }
        let cache = self
            .precipitation
            .get_mut(&cp(p))
            .expect("Structure write needs unloaded chunk");
        let col = (p.z.rem_euclid(16) * 16 + p.x.rem_euclid(16)) as usize;
        if p.y >= cache[col] - 1 {
            cache[col] = -999;
        }
    }
    fn get(&self, p: IVec3) -> u16 {
        self.read(p.to_array())
    }
    fn accessible(&self, _p: IVec3) -> bool {
        true
    }
    fn chunk_loaded(&self, p: ChunkPos) -> bool {
        self.world.chunks.contains_key(&p)
    }
    fn set_before_added(&mut self, p: IVec3, state: u16) -> bool {
        if !valid(p) {
            return false;
        }
        let old = self.read(p.to_array());
        if old == state {
            return false;
        }
        let tile_drop=tile_break::item(self.world,p,old,state);
        assert!(if self.record_overlay_edits { self.world.set(p,state) }
            else { self.world.set_raw(p,state) });
        if let Some(item)=tile_drop{let mut random=self.random.share();super::super::drops::spawn_item(self,&mut random,p,item).expect("Structure tile drop failed");}
        if old >> 4 != state >> 4 {
            if let Some(tile) = self.live_containers.get(&p.to_array()) {
                self.entities
                    .spill(self.hooks.as_ref(), self.world, self.drops, tile)
                    .expect("Native structure inventory spill failed");
            }
        }
        let mut ticks = self.ticks.share_scheduler();
        break_callbacks::leaves(self, &mut ticks, p, old, state);
        rail_effects::removed(self, p, old, state);
        if self.world.get(p) >> 4 != state >> 4 {
            return false;
        }
        tile_state_changed(
            self.live_containers,
            self.tile_revision,
            self.live_spawners,
            p.to_array(),
            old,
            state,
        );
        if old >> 4 != state >> 4 {
            self.drop_tiles.remove(&p.to_array());
        }
        self.light.chunk_block_changed(self.world, p, old);
        self.output
            .containers
            .retain(|c| c.position != p.to_array() || c.block == state >> 4);
        if state >> 4 != 52 {
            self.output.spawners.remove(&p.to_array());
        }
        self.changes.changed.push(p);
        true
    }
    fn after_added(&mut self, p: IVec3, old: u16, new: u16) {
        plant_effects::added(self, p, old, new);
        fire::added(self,p,old,new);
        rail_effects::added(self, p, old, new);
        facing_effects::added(self, p, old, new);
        self.light.after_block_added_state(self.world, p, old, new);
    }
    fn world_random_int(&mut self, bound: i32) -> Option<i32> {
        Some(self.random.next_int(bound))
    }
}
impl super::super::drops::DropWorld for LoadedStructures<'_> {
    fn is_remote(&self) -> bool {
        false
    }
    fn do_tile_drops(&self) -> bool {
        self.do_tile_drops
    }
    fn tile_entity(&mut self, p: IVec3) -> Result<Option<super::super::drops::DropTile>> {
        Ok(self.drop_tiles.get(&p.to_array()).cloned())
    }
    fn emit(&mut self, event: super::super::drops::DropEvent) -> Result<()> {
        self.entities
            .emit(self.hooks.as_ref(), self.world, self.drops, event)
    }
}
impl mineshaft::MineshaftWorld for LoadedStructures<'_> {
    fn get(&self, p: [i32; 3]) -> u16 {
        self.read(p)
    }
    fn set(&mut self, p: [i32; 3], s: u16) {
        self.write(p, s);
    }
    fn cart(&mut self, cart: mineshaft::ChestMinecart) {
        self.output.carts.push(cart);
    }
    fn spawner(&mut self, p: [i32; 3]) {
        LoadedStructures::spawner(self, p, "CaveSpider");
    }
}
impl village::VillageWorld for LoadedStructures<'_> {
    fn get(&self, p: [i32; 3]) -> u16 {
        self.read(p)
    }
    fn set(&mut self, p: [i32; 3], s: u16) {
        self.write(p, s);
    }
    fn chest(&mut self, p: [i32; 3], slots: Vec<Option<LootStack>>) {
        self.container(p, 54, slots);
    }
    fn villager(&mut self, p: [i32; 3], profession: Option<u8>) {
        // EntityVillager.onInitialSpawn always rolls world.rand before the
        // building-specific profession override, including fixed professions.
        let rolled = self.random.next_int(5) as u8;
        self.output.villagers.push(village::VillagerSpawn {
            position: p,
            profession: Some(profession.unwrap_or(rolled)),
        });
    }
}
impl stronghold::StrongholdWorld for LoadedStructures<'_> {
    fn get(&self, p: [i32; 3]) -> u16 {
        self.read(p)
    }
    fn set(&mut self, p: [i32; 3], s: u16) {
        self.write(p, s);
    }
    fn chest(&mut self, p: [i32; 3], slots: Vec<Option<LootStack>>) {
        self.container(p, 54, slots);
    }
    fn spawner(&mut self, p: [i32; 3]) {
        LoadedStructures::spawner(self, p, "Silverfish");
    }
}
impl scattered::ScatteredWorld for LoadedStructures<'_> {
    fn get(&self, p: [i32; 3]) -> u16 {
        self.read(p)
    }
    fn set(&mut self, p: [i32; 3], s: u16) {
        self.write(p, s);
    }
    fn container(&mut self, p: [i32; 3], block: u16, slots: Vec<Option<LootStack>>) {
        LoadedStructures::container(self, p, block, slots);
    }
    fn witch(&mut self, p: [i32; 3]) {
        self.output.witches.push(p);
    }
}
impl monument::MonumentWorld for LoadedStructures<'_> {
    fn get(&self, p: [i32; 3]) -> u16 {
        self.read(p)
    }
    fn set(&mut self, p: [i32; 3], s: u16) {
        self.write(p, s);
    }
    fn elder(&mut self, p: [i32; 3]) {
        self.output.elders.push(p);
    }
}
