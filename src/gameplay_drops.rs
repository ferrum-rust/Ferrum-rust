//! Local singleplayer host for the original block drop callbacks.
use super::{JavaRandom, Simulation, World};
use crate::vanilla::{
    drops::{DropEvent, DropNbt, DropStack, DropTile, DropWorld},
    item_nbt, middle_pick,
};
use glam::{DVec3, IVec3};
use std::collections::BTreeMap;

pub(crate) fn tile_drops_enabled(sim: &Simulation) -> bool {
    sim.world_commands
        .rules
        .get("doTileDrops")
        .is_none_or(|value| value != "false")
}

pub(super) fn canonical_drop_state(state: u16) -> u16 {
    let id = state >> 4;
    let meta = state & 15;
    match id {
        26 => {
            if meta & 8 == 0 {
                (26 << 4) | (meta & 3)
            } else {
                (26 << 4) | 8 | (meta & 3)
            }
        }
        27 | 28 | 157 if meta & 7 > 5 => state & !7,
        50 | 75 | 76 if !matches!(meta, 1..=5) => (id << 4) | 5,
        64 | 71 | 193..=197 if meta & 8 != 0 => (id << 4) | 8 | (meta & 3),
        65 | 68 | 177 if !matches!(meta, 2..=5) => (id << 4) | 2,
        66 if meta > 9 => 66 << 4,
        70 | 72 => (id << 4) | (meta & 1),
        77 | 143 if meta & 7 > 5 => (state & !7) | 5,
        144 if meta & 7 > 5 => (state & !7) | 1,
        175 => {
            if meta & 8 != 0 {
                (175 << 4) | 8 | (meta & 3)
            } else if meta > 5 {
                175 << 4
            } else {
                state
            }
        }
        _ => state,
    }
}

pub(super) struct LocalDrops<'a> {
    sim: &'a mut Simulation,
    world: &'a World,
    enabled: bool,
}
impl DropWorld for LocalDrops<'_> {
    fn is_remote(&self) -> bool {
        false
    }
    fn do_tile_drops(&self) -> bool {
        self.enabled
    }
    fn tile_entity(&mut self, p: IVec3) -> anyhow::Result<Option<DropTile>> {
        if let Some(tile) = self.world.piston_at(p) {
            return Ok(Some(DropTile::Piston {
                state: tile.carried,
            }));
        }
        if matches!(self.world.get(p) >> 4, 176 | 177) {
            return Ok(self
                .world
                .tile(p)
                .and_then(item_nbt::compound)
                .map(|tag| DropTile::Banner {
                    base_color: item_nbt::number(tag.get("Base")),
                    written_tag: tag.clone(),
                }));
        }
        Ok(None)
    }
    fn emit(&mut self, event: DropEvent) -> anyhow::Result<()> {
        match event {
            DropEvent::Item {
                position,
                stack,
                pickup_delay,
            } => {
                let mut item = crate::vanilla::item_entity::stack_from_metadata(
                    stack.id,
                    stack.count,
                    stack.meta as u16,
                )?;
                item.nbt = stack.nbt.map(std::sync::Arc::new);
                let mut math = JavaRandom::from_state(self.sim.xp_math_random);
                self.sim.next_id += 1;
                let mut drop = super::Drop::new(
                    self.sim.next_id,
                    DVec3::from_array(position),
                    item,
                    &mut math,
                    JavaRandom::new(JavaRandom::independent_seed()).state(),
                );
                self.sim.xp_math_random = math.state();
                drop.pickup_delay = pickup_delay;
                self.sim.drops.push(drop);
            }
            DropEvent::Experience { position, value } => {
                self.sim.spawn_xp(DVec3::from_array(position), value)
            }
            DropEvent::Silverfish { position, yaw } => {
                self.sim.spawn_registered(
                    super::MobKind::Silverfish,
                    DVec3::from_array(position).as_vec3(),
                    std::f32::consts::PI + yaw.to_radians(),
                );
            }
            DropEvent::EntityStatus { status: 20 } => self.sim.spawn_mob_particles(),
            DropEvent::EntityStatus { .. } => {}
        }
        Ok(())
    }
}

impl Simulation {
    // EntityLiving status 20 is emitted immediately after a successful spawn.
    pub(super) fn spawn_mob_particles(&mut self) {
        let Some(mob) = self.mobs.last() else { return };
        let feet = mob.position.as_dvec3();
        let (width, height) = self.mob_size(mob);
        let mut entity =
            crate::vanilla::spawning::EntityRandom::from_state(self.particle_random[1], None);
        let mut random = JavaRandom::from_state(self.particle_random[2]);
        for _ in 0..20.min(4096usize.saturating_sub(self.particles.len())) {
            let motion = DVec3::from_array(std::array::from_fn(|_| entity.next_gaussian() * 0.02));
            let offsets = DVec3::new(
                f64::from(entity.random.next_float() * width * 2.) - f64::from(width),
                f64::from(entity.random.next_float() * height),
                f64::from(entity.random.next_float() * width * 2.) - f64::from(width),
            );
            if let Some(particle) = super::Particle::network_sprite(
                0,
                feet + offsets - motion * 10.,
                motion,
                &mut random,
            ) {
                self.particles.push(particle);
            }
        }
        self.particle_random[1] = entity.state().0;
        self.particle_random[2] = random.state();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gameplay::{MobKind, Mode, Outcome};
    use crate::{inventory::Stack, vanilla::drops::DropNbt as Tag};

    #[test]
    fn breaking_infested_blocks_releases_silverfish_except_silk_creative_and_disabled_drops() {
        let mut world = super::super::tests::flat_world();
        let p = IVec3::new(3, 2, 4);
        let mut sim = Simulation::new(Mode::Survival, 189);
        for meta in 0..6 {
            world.set(p, (97 << 4) | meta);
            sim.destroy_picked_block(&mut world, p, &Default::default(), &mut Outcome::default());
            let m = sim.mobs.last().unwrap();
            assert_eq!(
                (m.kind, m.position),
                (MobKind::Silverfish, glam::Vec3::new(3.5, 2., 4.5))
            );
        }
        assert_eq!(sim.mobs.len(), 6);
        let enchantment =
            Tag::Compound([("id".into(), Tag::Short(33)), ("lvl".into(), Tag::Short(1))].into());
        sim.inventory.slots[0] = Some(
            Stack::new(278, 1).with_nbt(Tag::Compound(
                [(
                    "ench".into(),
                    Tag::List {
                        element: 10,
                        values: vec![enchantment],
                    },
                )]
                .into(),
            )),
        );
        world.set(p, (97 << 4) | 1);
        sim.destroy_picked_block(&mut world, p, &Default::default(), &mut Outcome::default());
        assert_eq!(sim.drops.last().unwrap().stack.id, 4);
        sim.inventory.slots[0] = None;
        sim.world_commands
            .rules
            .insert("doTileDrops".into(), "false".into());
        world.set(p, 97 << 4);
        sim.destroy_picked_block(&mut world, p, &Default::default(), &mut Outcome::default());
        sim.world_commands.rules.remove("doTileDrops");
        sim.mode = Mode::Creative;
        world.set(p, 97 << 4);
        sim.destroy_picked_block(&mut world, p, &Default::default(), &mut Outcome::default());
        assert_eq!(sim.mobs.len(), 6);
    }
}

impl<'a> LocalDrops<'a> {
    pub(super) fn new(sim: &'a mut Simulation, world: &'a World) -> Self {
        let enabled = tile_drops_enabled(sim);
        Self {
            sim,
            world,
            enabled,
        }
    }

    pub(super) fn farm(&mut self, random: &mut JavaRandom, p: IVec3, state: u16, fortune: i32) {
        let norm = canonical_drop_state(state);
        if let Err(error) = crate::vanilla::drops::drop_block(self, random, p, norm, 1., fortune) {
            if norm != (state & !15) {
                let _ =
                    crate::vanilla::drops::drop_block(self, random, p, state & !15, 1., fortune);
            } else {
                eprintln!("Plant drop for {state} at {p}: {error}");
            }
        }
    }

    pub(super) fn spawn(&mut self, random: &mut JavaRandom, p: IVec3, stack: DropStack) {
        if let Err(error) = crate::vanilla::drops::spawn_item(self, random, p, stack) {
            eprintln!("Block drop spawn at {p}: {error}");
        }
    }
}

pub(super) fn extra_tile_drop(
    sim: &Simulation,
    world: &World,
    p: IVec3,
    state: u16,
) -> Option<DropStack> {
    match state >> 4 {
        140 => {
            if let Some(tile) = middle_pick::tile_snapshot(140, p, world.tile(p), None) {
                if let Some(fields) = item_nbt::compound(&tile) {
                    let item = match fields.get("Item") {
                        Some(DropNbt::String(s)) => {
                            middle_pick::named_item(s).or_else(|| s.parse().ok())
                        }
                        value => u16::try_from(item_nbt::number(value)).ok(),
                    };
                    if let Some(id) = item.filter(|&id| id != 0) {
                        return Some(DropStack {
                            id,
                            count: 1,
                            meta: item_nbt::number(fields.get("Data")).max(0),
                            nbt: None,
                        });
                    }
                }
            }
            if let Some(stack) = sim
                .containers
                .get(&super::container_key(p))
                .and_then(|c| c.slots.first())
                .and_then(|s| s.as_ref())
                .filter(|s| s.id != 0 && s.count > 0)
            {
                return Some(DropStack {
                    id: stack.id,
                    count: 1,
                    meta: i32::from(stack.meta),
                    nbt: stack.nbt.as_deref().cloned(),
                });
            }
            let (id, meta) = match state & 15 {
                1 => (38, 0),
                2 => (37, 0),
                3 => (6, 0),
                4 => (6, 1),
                5 => (6, 2),
                6 => (6, 3),
                7 => (40, 0),
                8 => (39, 0),
                9 => (81, 0),
                10 => (32, 0),
                11 => (31, 2),
                12 => (6, 4),
                13 => (6, 5),
                _ => return None,
            };
            Some(DropStack {
                id,
                count: 1,
                meta,
                nbt: None,
            })
        }
        144 => {
            if state & 8 != 0 {
                return None;
            }
            let Some(tile) = middle_pick::tile_snapshot(144, p, world.tile(p), None) else {
                return Some(DropStack {
                    id: 397,
                    count: 1,
                    meta: 0,
                    nbt: None,
                });
            };
            let Some(fields) = item_nbt::compound(&tile) else {
                return Some(DropStack {
                    id: 397,
                    count: 1,
                    meta: 0,
                    nbt: None,
                });
            };
            let kind = item_nbt::number(fields.get("SkullType")).max(0);
            let nbt = if kind == 3 {
                fields
                    .get("Owner")
                    .filter(|tag| matches!(tag, DropNbt::Compound(_)))
                    .cloned()
                    .or_else(|| match fields.get("ExtraType") {
                        Some(DropNbt::String(s)) if !s.is_empty() => {
                            Some(DropNbt::String(s.clone()))
                        }
                        _ => None,
                    })
                    .map(|v| DropNbt::Compound(BTreeMap::from([("SkullOwner".into(), v)])))
            } else {
                None
            };
            Some(DropStack {
                id: 397,
                count: 1,
                meta: kind,
                nbt,
            })
        }
        _ => None,
    }
}
