//! Native World.updateEntities phase for naturally falling sand, gravel and anvils.
use super::*;
use crate::vanilla::{
    drops::{DropEvent, DropStack},
    falling::{Falling, FallingWorld},
    movement::{Aabb, MovementWorld},
};
use glam::DVec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FallingEntry {
    /// Stable local publication ID, retained across checkpoints.
    pub id: u64,
    pub body: Falling,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FallingEffect {
    Auxiliary {
        id: i32,
        position: [i32; 3],
        data: i32,
    },
    Sound {
        name: String,
        position: DVec3,
        volume: f32,
        pitch: f32,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FallingManager {
    pub next_id: u64,
    pub live: Vec<FallingEntry>,
    pub effects: Vec<FallingEffect>,
    pub do_entity_drops: bool,
}
impl Default for FallingManager {
    fn default() -> Self {
        Self {
            next_id: 0,
            live: Vec::new(),
            effects: Vec::new(),
            do_entity_drops: true,
        }
    }
}
impl FallingManager {
    pub(super) fn spawn(&mut self, p: IVec3, state: u16, random_state: u64) -> bool {
        let mut body = Falling::new(state, p.as_dvec3() + DVec3::new(0.5, 0., 0.5), random_state);
        body.hurt_entities = state >> 4 == 145;
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .expect("Native falling entity ID exhausted");
        self.live.push(FallingEntry { id, body });
        true
    }
    pub fn validate(&self) -> Result<()> {
        let mut ids = HashSet::new();
        for e in &self.live {
            ensure!(
                e.id < self.next_id && ids.insert(e.id),
                "Invalid checkpoint falling entity ID"
            );
            let b = &e.body;
            ensure!(
                !b.dead && matches!(b.state >> 4, 12 | 13 | 145) && b.random_state < 1 << 48,
                "Invalid checkpoint falling block state"
            );
            ensure!(
                b.position.is_finite()
                    && b.previous.is_finite()
                    && b.velocity.is_finite()
                    && b.bounds.min.is_finite()
                    && b.bounds.max.is_finite()
                    && b.bounds.min.cmple(b.bounds.max).all(),
                "Invalid checkpoint falling block bounds"
            );
        }
        Ok(())
    }
}

impl Runtime {
    /// Scheduled blocks execute before the entity phase. Appends during an
    /// entity callback join the tail of the same iteration, as in World's
    /// index-based loadedEntityList loop. Suspended callbacks retain no borrow.
    pub fn tick_blocks_and_falling(&mut self) -> Result<bool> {
        if !self.backend.pending_tnt.is_empty() { return Ok(false); }
        self.provider.world_time = self.provider.world_time.wrapping_add(1);
        let mut ticks = self.backend.ticks.share_scheduler();
        let mut effects = ticks.tick_access(&mut self.access())?;
        self.backend
            .block_effects
            .changed
            .append(&mut effects.changed);
        self.backend.block_effects.drops.append(&mut effects.drops);
        self.backend
            .block_effects
            .washed
            .append(&mut effects.washed);
        // Preserve the completed block phase. A primed TNT body requires the
        // original entity and explosion phases before the world can advance.
        if !self.backend.pending_tnt.is_empty() { return Ok(false); }
        self.tick_entities()?;
        Ok(true)
    }
    pub(super) fn tick_falling_entity(&mut self, entity: &mut FallingEntry) -> Result<()> {
        let mut adapter = GravityWorld {
            access: self.access(),
            effects: Vec::new(),
        };
        let result = entity.body.tick(&mut adapter);
        for effect in &mut adapter.effects {
            if let FallingEffect::Sound { position, .. } = effect {
                *position = entity.body.position;
            }
        }
        let mut emitted = adapter.effects;
        self.backend.falling.effects.append(&mut emitted);
        result
    }
}

struct GravityWorld<'a> {
    access: Access<'a>,
    effects: Vec<FallingEffect>,
}
impl MovementWorld for GravityWorld<'_> {
    fn get(&self, p: IVec3) -> u16 {
        self.access
            .get_block(p)
            .expect("Native falling block read failed")
    }
    fn loaded_column(&self, x: i32, z: i32) -> bool {
        self.access
            .with(|provider, _| provider.contains((x.div_euclid(16), z.div_euclid(16))))
    }
    fn block_boxes(&self, p: IVec3, state: u16, out: &mut Vec<Aabb>) {
        self.access
            .with(|_, b| MovementWorld::block_boxes(&b.world, p, state, out));
    }
}
impl FallingWorld for GravityWorld<'_> {
    fn is_remote(&self) -> bool {
        false
    }
    fn do_entity_drops(&self) -> bool {
        self.access.with(|_, b| b.falling.do_entity_drops)
    }
    fn set_block(&mut self, p: IVec3, state: u16, flags: i32) -> Result<bool> {
        self.access.set_block_flags(p, state, flags as u8)
    }
    fn can_place(&mut self, p: IVec3, state: u16) -> Result<bool> {
        let target = self.access.get_block(p)?;
        let place = crate::vanilla::falling::can_place_gravity(state, target)
            .ok_or_else(|| anyhow::anyhow!("Unsupported natural falling block {}", state >> 4))?;
        Ok(place)
    }
    fn placed(
        &mut self,
        p: IVec3,
        state: u16,
        _tile: Option<&crate::vanilla::drops::DropNbt>,
    ) -> Result<()> {
        // None of the three natural falling blocks provides a TileEntity, so
        // the original tile-data copy condition is false for these block IDs.
        ensure!(
            matches!(state >> 4, 12 | 13 | 145),
            "Unsupported falling tile placement"
        );
        if state >> 4 == 145 {
            self.effects.push(FallingEffect::Auxiliary {
                id: 1022,
                position: p.to_array(),
                data: 0,
            });
        }
        Ok(())
    }
    fn drop_item(&mut self, position: DVec3, state: u16) -> Result<()> {
        let id = state >> 4;
        let meta = match id {
            12 => state & 1,
            13 => 0,
            145 => (state & 12) >> 2,
            _ => anyhow::bail!("Unsupported falling item {id}"),
        };
        crate::vanilla::drops::DropWorld::emit(
            &mut self.access,
            DropEvent::Item {
                position: position.to_array(),
                stack: DropStack {
                    id,
                    count: 1,
                    meta: i32::from(meta),
                    nbt: None,
                },
                pickup_delay: 10,
            },
        )
    }
    fn damage_entities(&mut self, bounds: Aabb, anvil: bool, amount: f32) -> Result<()> {
        let hooks = self.access.with(|_, b| b.hooks.clone());
        hooks.falling_damage(&mut self.access, bounds, anvil, amount)
    }
    fn contact(&mut self, p: IVec3, state: u16, entity: &mut Falling) -> Result<()> {
        let hooks = self.access.with(|_, b| b.hooks.clone());
        hooks.falling_contact(&mut self.access, p, state, entity)
    }
    fn raining_at(&self, p: IVec3) -> bool {
        let hooks = self.access.with(|_, b| b.hooks.clone());
        hooks.raining_at(&self.access, p)
    }
    fn sound(&mut self, name: &str, volume: f32, pitch: f32) -> Result<()> {
        self.effects.push(FallingEffect::Sound {
            name: name.to_string(),
            position: DVec3::ZERO,
            volume,
            pitch,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Chunk;
    use serde_json::{json, Value};
    use std::io::Read;

    struct FlatOracle;
    impl Hooks for FlatOracle {
        fn populate(
            &self,
            _access: &mut Access<'_>,
            _pos: ChunkPos,
            _random: &mut JavaRandom,
        ) -> Result<()> {
            Ok(())
        }
        fn falling_random_state(&self, id: u64) -> u64 {
            JavaRandom::new(189 + 17 * id as i64).state()
        }
        fn falling_damage(
            &self,
            _access: &mut Access<'_>,
            _bounds: Aabb,
            _anvil: bool,
            _amount: f32,
        ) -> Result<()> {
            // The original fixture contains only FallingBlock and item entities.
            Ok(())
        }
    }
    fn flat_checkpoint() -> RuntimeSnapshot {
        let mut runtime = Runtime::new(189, 0, false, Arc::new(FlatOracle));
        for x in -2..=2 {
            for z in -2..=2 {
                runtime.provide((x, z)).unwrap();
            }
        }
        let mut flat = Chunk::empty();
        flat.blocks[..256].fill(7 << 4);
        flat.top = 1;
        for c in runtime.backend.world.chunks.values_mut() {
            *c = Arc::new(flat.clone());
        }
        runtime.backend.light = Lighting::new(&runtime.backend.world, false);
        runtime.backend.world_random.set_seed(0);
        runtime.checkpoint().unwrap()
    }
    fn bits(values: impl IntoIterator<Item = f64>) -> Vec<String> {
        values
            .into_iter()
            .map(|n| format!("{:x}", n.to_bits()))
            .collect()
    }
    fn observed(runtime: &Runtime) -> Value {
        let b = &runtime.backend;
        let mut blocks = Vec::new();
        for y in 1..=12 {
            for z in 6..=10 {
                for x in 6..=10 {
                    let state = b.world.get(IVec3::new(x, y, z));
                    if state != 0 {
                        blocks.push(json!([x, y, z, state]));
                    }
                }
            }
        }
        let falling = b
            .falling
            .live
            .iter()
            .map(|e| {
                let f = &e.body;
                json!([
                    e.id,
                    f.state,
                    f.age,
                    bits(
                        f.position
                            .to_array()
                            .into_iter()
                            .chain(f.velocity.to_array())
                    ),
                    f.grounded,
                    f.random_state.to_string()
                ])
            })
            .collect::<Vec<_>>();
        let drops = b
            .drops
            .iter()
            .map(|event| match event {
                DropEvent::Item {
                    position,
                    stack,
                    pickup_delay,
                } => json!([
                    bits(*position),
                    stack.id,
                    stack.meta,
                    stack.count,
                    pickup_delay
                ]),
                _ => panic!("Unexpected falling fixture drop event"),
            })
            .collect::<Vec<_>>();
        let effects = b
            .falling
            .effects
            .iter()
            .map(|e| match e {
                FallingEffect::Auxiliary { id, position, data } => {
                    json!([id, position[0], position[1], position[2], data])
                }
                FallingEffect::Sound { .. } => panic!("Unexpected falling fixture sound"),
            })
            .collect::<Vec<_>>();
        json!([
            runtime.provider.world_time,
            blocks,
            falling,
            drops,
            effects,
            b.world_random.state().to_string()
        ])
    }
    #[test]
    fn original_worldserver_scheduled_falling_and_midflight_resume_match() {
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(
            &include_bytes!("../../tests/fixtures/vanilla189-runtime-falling.json.gz")[..],
        )
        .read_to_end(&mut bytes)
        .unwrap();
        let cases: Value = serde_json::from_slice(&bytes).unwrap();
        let base = flat_checkpoint();
        for case in cases.as_array().unwrap() {
            let mut runtime = Runtime::resume(base.clone(), Arc::new(FlatOracle)).unwrap();
            for row in case["edits"].as_array().unwrap() {
                let edit: [i32; 4] = serde_json::from_value(row.clone()).unwrap();
                runtime
                    .access()
                    .set_block_flags(IVec3::new(edit[0], edit[1], edit[2]), edit[3] as u16, 3)
                    .unwrap();
            }
            for frame in case["frames"].as_array().unwrap() {
                let time = frame[0].as_i64().unwrap();
                if time > 0 {
                    if time == 5 && case["scenario"] == 2 {
                        runtime
                            .access()
                            .set_block_flags(IVec3::new(8, 5, 8), 0, 3)
                            .unwrap();
                    }
                    runtime.tick_blocks_and_falling().unwrap();
                }
                assert_eq!(
                    &observed(&runtime),
                    frame,
                    "state {} scenario {} tick {time}",
                    case["state"],
                    case["scenario"]
                );
                if time == 7 {
                    assert!(!runtime.backend.falling.live.is_empty());
                    let json = serde_json::to_vec(&runtime.checkpoint().unwrap()).unwrap();
                    runtime = Runtime::resume(
                        serde_json::from_slice(&json).unwrap(),
                        Arc::new(FlatOracle),
                    )
                    .unwrap();
                    assert_eq!(&observed(&runtime), frame, "resumed tick {time}");
                }
            }
        }
    }
}
