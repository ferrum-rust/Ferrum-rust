use super::{Drop, Mode, Outcome, Simulation};
use crate::{
    inventory::Stack,
    player::Player,
    vanilla::{
        collision_shapes,
        item_entity::{ItemEffect, ItemWorld},
        movement::{Aabb, MovementWorld},
        random::JavaRandom,
    },
    world::World,
};
use anyhow::Result;
use glam::{DVec3, IVec3};
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;

#[cfg(test)]
#[path = "live_item_tests.rs"]
mod tests;

fn item(id: u64, stack: Stack, pos: DVec3, velocity: DVec3, delay: i32) -> Drop {
    let mut entity_random = JavaRandom::new(JavaRandom::independent_seed());
    entity_random.next_long();
    entity_random.next_long();
    let mut math = JavaRandom::new(JavaRandom::independent_seed());
    let mut drop = Drop::new(id, pos, stack, &mut math, entity_random.state());
    drop.previous = pos;
    drop.velocity = velocity;
    drop.pickup_delay = delay;
    drop
}

// Player saves previously stored only the visible position and velocity as floats.
// Rebuild the item bounds at those feet on load and preserve the outstanding timer.
pub(super) fn read_drops<'de, D: Deserializer<'de>>(de: D) -> Result<Vec<Drop>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Saved {
        Current(Drop),
        Legacy {
            id: u64,
            item: Stack,
            position: DVec3,
            previous: DVec3,
            velocity: DVec3,
            age: i32,
            pickup_delay: i32,
            hover_start: Option<f32>,
        },
    }
    Ok(Vec::<Saved>::deserialize(de)?
        .into_iter()
        .map(|d| match d {
            Saved::Current(d) => d,
            Saved::Legacy {
                id,
                item: stack,
                position,
                previous,
                velocity,
                age,
                pickup_delay,
                hover_start,
            } => {
                let mut d = item(id, stack, position, velocity, pickup_delay);
                d.age = age;
                d.ticks = age.max(0);
                d.previous = previous;
                d.first_update = age == 0;
                if let Some(phase) = hover_start {
                    d.hover_start = phase
                }
                d
            }
        })
        .collect())
}
#[cfg(test)]
#[test]
fn legacy_save_restores_item_bounds_and_keeps_an_in_progress_pickup_timer() {
    let old = serde_json::json!({"drops":[{
        "item":Stack::new(4,3),"id":12,"position":[5.0,64.0,8.0],"previous":[4.875,64.0,8.0],
        "velocity":[0.125,0.0,0.0],"age":13,"pickup_delay":27,"hover_start":0.75
    }]});
    let loaded: Simulation = serde_json::from_value(old).unwrap();
    let mut item = loaded.drops[0].clone();
    assert_eq!((item.stack.count, item.age, item.pickup_delay), (3, 13, 27));
    assert_eq!(item.bounds.min, DVec3::new(4.875, 64., 7.875));
    assert_eq!(item.bounds.max, DVec3::new(5.125, 64.25, 8.125));
    item.owner = Some("Alex".into());
    item.thrower = Some("Steve".into());
    let sim = Simulation {
        drops: vec![item.clone()],
        ..Default::default()
    };
    let again: Simulation = serde_json::from_slice(&serde_json::to_vec(&sim).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(&again.drops[0]).unwrap(),
        serde_json::to_value(&item).unwrap()
    );
}
impl Drop {
    pub(crate) fn set_feet(&mut self, pos: DVec3) {
        self.bounds = Aabb {
            min: pos - DVec3::new(0.125, 0., 0.125),
            max: pos + DVec3::new(0.125, 0.25, 0.125),
        };
        self.position = pos;
    }
}
impl Simulation {
    pub(crate) fn spawn_item_entity(
        &mut self,
        stack: Stack,
        pos: DVec3,
        velocity: DVec3,
        delay: i32,
    ) -> &mut Drop {
        self.next_id = self.next_id.wrapping_add(1);
        self.drops
            .push(item(self.next_id, stack, pos, velocity, delay));
        self.drops.last_mut().unwrap()
    }
    pub(crate) fn tick_player_item_pickup(&mut self, player: &Player, out: &mut Outcome) {
        if self.health <= 0. || self.commands.player_mode == Some(3) {
            return;
        }
        let reach = player.precise_bounds().expand(DVec3::new(1., 0.5, 1.));
        for drop in &mut self.drops {
            if !reach.intersects(drop.bounds) {
                continue;
            }
            let thrower_online = drop.stack.id == 264
                && drop
                    .thrower
                    .as_deref()
                    .is_some_and(|name| self.lan.as_ref().is_some_and(|lan| lan.has_guest(name)));
            let Ok(result) = drop.pickup(
                &mut self.inventory,
                &self.local_name,
                self.mode == Mode::Creative,
                thrower_online,
            ) else {
                continue;
            };
            if let Some(count) = result.announced_count {
                out.collected_items.push(drop.id);
                self.pickup_fx
                    .item_count(drop, count, super::item_pickup_fx::Collector::Local);
            }
            if let Some((volume, pitch)) = result.sound {
                out.world_sounds
                    .push(("random.pop", drop.position, volume, pitch));
            }
            for (player, stat) in result.stats {
                if player == self.local_name {
                    super::local_commands::award_pickup(&mut self.statistics, &stat);
                } else {
                    out.guest_item_stats.push((player, stat));
                }
            }
        }
    }
    pub(crate) fn tick_drop_motion(&mut self, world: &World, out: &mut Outcome) {
        self.move_items(world, out, false);
    }
    pub(crate) fn tick_remote_drops(&mut self, world: &World, out: &mut Outcome) {
        self.move_items(world, out, true);
    }
    fn move_items(&mut self, world: &World, out: &mut Outcome, remote: bool) {
        if self.drops.is_empty() {
            return;
        }
        let mut sections = HashMap::<IVec3, Vec<usize>>::new();
        if !remote && self.drops.len() > 1 {
            for (i, d) in self.drops.iter().enumerate().filter(|(_, d)| !d.dead) {
                sections.entry(section(d.position)).or_default().push(i);
            }
        }
        let wet = self.dimension == 0 && self.weather.strengths(1.).0 > 0.2;
        for i in 0..self.drops.len() {
            let (before, rest) = self.drops.split_at_mut(i);
            let (source, after) = rest.split_first_mut().unwrap();
            if source.dead || !world.contains(source.position.as_vec3()) {
                continue;
            }
            let old = section(source.position);
            let mut view = PlayItems {
                world,
                sections: &sections,
                before,
                after,
                out,
                remote,
                wet,
            };
            if let Err(e) = source.tick(&mut view) {
                eprintln!("Item {} update: {e}", source.id);
            }
            let new = section(source.position);
            if old != new && !sections.is_empty() {
                if let Some(indices) = sections.get_mut(&old) {
                    indices.retain(|j| *j != i);
                }
                sections.entry(new).or_default().push(i);
            }
        }
        self.drops.retain(|d| !d.dead && d.stack.count > 0);
    }
}
fn section(pos: DVec3) -> IVec3 {
    pos.floor().as_ivec3() >> 4
}
struct PlayItems<'a> {
    world: &'a World,
    sections: &'a HashMap<IVec3, Vec<usize>>,
    before: &'a mut [Drop],
    after: &'a mut [Drop],
    out: &'a mut Outcome,
    remote: bool,
    wet: bool,
}
impl MovementWorld for PlayItems<'_> {
    fn get(&self, p: IVec3) -> u16 {
        self.world.get(p)
    }
    fn block_boxes(&self, p: IVec3, state: u16, out: &mut Vec<Aabb>) {
        self.world.block_boxes(p, state, out);
    }
    fn loaded_column(&self, x: i32, z: i32) -> bool {
        self.world.loaded_column(x, z)
    }
}
impl ItemWorld for PlayItems<'_> {
    fn is_remote(&self) -> bool {
        self.remote
    }
    fn full_collision_cube(&mut self, p: IVec3) -> Result<bool> {
        Ok(collision_shapes::full_collision_cube(
            p,
            self.get(p),
            |q| self.get(q),
            self.world.piston_at(p),
        ))
    }
    fn merge_nearby(&mut self, source: &mut Drop) -> Result<()> {
        if self.sections.is_empty() {
            return Ok(());
        }
        let area = source.bounds.expand(DVec3::new(0.5, 0., 0.5));
        // Sections index feet, while the query intersects the entire 1/4-block item box.
        let lo = section(area.min - DVec3::new(0.125, 0.25, 0.125));
        let hi = section(area.max + DVec3::new(0.125, 0., 0.125));
        for x in lo.x..=hi.x {
            for z in lo.z..=hi.z {
                for y in lo.y..=hi.y {
                    for &j in self
                        .sections
                        .get(&IVec3::new(x, y, z))
                        .into_iter()
                        .flatten()
                    {
                        let n = self.before.len();
                        let other = if j < n {
                            &mut self.before[j]
                        } else if j > n {
                            &mut self.after[j - n - 1]
                        } else {
                            continue;
                        };
                        if other.bounds.intersects(area) {
                            Drop::combine(source, other);
                        }
                        if source.dead {
                            return Ok(());
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn contact(&mut self, _p: IVec3, state: u16, d: &mut Drop) -> Result<()> {
        if state >> 4 == 118 && state & 3 != 0 && d.fire > 0 {
            d.fire = 0;
        }
        Ok(())
    }
    fn raining_at(&self, p: IVec3) -> bool {
        if !self.wet {
            return false;
        }
        use crate::vanilla::material;
        let biome = crate::assets::biome_colors::biome_at(self.world, p);
        crate::vanilla::runtime::rain_in_biome(biome, p)
            && !(p.y.max(1)..256).any(|y| {
                let state = self.get(IVec3::new(p.x, y, p.z));
                material::blocks_movement(state) || material::is_liquid(state)
            })
    }
    fn effect(&mut self, e: ItemEffect) {
        if let ItemEffect::Sound {
            position,
            name,
            volume,
            pitch,
        } = e
        {
            let clip = match name.as_str() {
                "random.fizz" => "random.fizz",
                "game.neutral.swim.splash" => "game.neutral.swim.splash",
                _ => return,
            };
            self.out.world_sounds.push((clip, position, volume, pitch));
        }
    }
}
