//! Overworld VillageCollection tick, door detection and home membership.
use super::{Door, Homes, Village};
use crate::vanilla::{material, movement::Aabb, random::JavaRandom, spawning};
use glam::{DVec3, IVec3};

pub trait VillageWorld {
    fn block(&self, p: IVec3) -> u16;
    fn sky(&self, p: IVec3) -> bool;
    fn residents(&self, bounds: Aabb) -> i32;
    fn golems(&self, bounds: Aabb) -> i32;
    fn spawn_golem(&mut self, position: IVec3);
    fn loaded(&self, _p: IVec3) -> bool {
        true
    }
    fn alive(&self, _actor: &super::reputation::ResidentActor) -> bool {
        true
    }
}
fn wood_door(state: u16) -> bool {
    matches!(state >> 4, 64 | 193..=197)
}
fn distance(a: IVec3, b: IVec3) -> f64 {
    (a.as_dvec3() - b.as_dvec3()).length_squared()
}
fn same_door(a: IVec3, b: IVec3) -> bool {
    a.x == b.x && a.z == b.z && a.y.abs_diff(b.y) <= 1
}
pub struct LocalVillageWorld<'a> {
    pub world: &'a crate::world::World,
    pub mobs: &'a [crate::gameplay::Mob],
    pub golem_spawns: &'a mut Vec<IVec3>,
    pub player_name: &'a str,
    pub player_alive: bool,
}
impl VillageWorld for LocalVillageWorld<'_> {
    fn alive(&self, actor: &super::reputation::ResidentActor) -> bool {
        if let Some(name) = &actor.player {
            self.player_alive && self.player_name == name
        } else {
            self.mobs
                .iter()
                .any(|m| m.id == actor.entity && m.health > 0.)
        }
    }
    fn block(&self, p: IVec3) -> u16 {
        self.world.get(p)
    }
    fn loaded(&self, p: IVec3) -> bool {
        self.world
            .chunks
            .contains_key(&(p.x.div_euclid(16), p.z.div_euclid(16)))
    }
    fn sky(&self, p: IVec3) -> bool {
        let Some(chunk) = self
            .world
            .chunks
            .get(&(p.x.div_euclid(16), p.z.div_euclid(16)))
        else {
            return false;
        };
        (p.y.max(0)..chunk.top).all(|y| {
            material::light_opacity(chunk.get(p.x.rem_euclid(16), y, p.z.rem_euclid(16))) == 0
        })
    }
    fn residents(&self, bounds: Aabb) -> i32 {
        self.mobs
            .iter()
            .filter(|m| m.kind == crate::gameplay::MobKind::Villager && m.health > 0.)
            .filter(|m| {
                let (width, height) = m.kind.size();
                let p = m.position.as_dvec3();
                let r = f64::from(width * 0.5);
                bounds.intersects(Aabb {
                    min: p - DVec3::new(r, 0., r),
                    max: p + DVec3::new(r, f64::from(height), r),
                })
            })
            .count() as i32
    }
    fn golems(&self, b: Aabb) -> i32 {
        let r = f64::from(1.4f32 * 0.5);
        self.mobs.iter().filter(|m| m.health>0. && m.kind==crate::gameplay::MobKind::IronGolem)
            .map(|m|m.position.as_dvec3())
            .chain(self.golem_spawns.iter().map(|p|p.as_dvec3()))
            .filter(|&p| {
                b.intersects(Aabb {
                    min: p - DVec3::new(r, 0., r),
                    max: p + DVec3::new(r, f64::from(2.9f32), r),
                })
            })
            .count() as i32
    }
    fn spawn_golem(&mut self, p: IVec3) {
        self.golem_spawns.push(p);
    }
}
impl Homes {
    pub fn enqueue(&mut self, p: IVec3) {
        if self.queue.len() <= 64 && !self.queue.contains(&p) {
            self.queue.push_back(p);
        }
    }
    pub fn tick(&mut self, world: &mut impl VillageWorld, random: &mut JavaRandom) {
        self.ticks = self.ticks.wrapping_add(1);
        for village in self.villages.iter_mut().filter(|v| v.active) {
            village.tick(self.ticks, world, random);
            village.active = !village.doors.is_empty();
        }
        if let Some(center) = self.queue.pop_front() {
            self.scan(center, world);
        }
    }
    fn scan(&mut self, center: IVec3, world: &impl VillageWorld) {
        let mut added: Vec<Door> = Vec::new();
        for dx in -16..16 {
            for dy in -4..4 {
                for dz in -16..16 {
                    let p = center + IVec3::new(dx, dy, dz);
                    if !world.loaded(p) {
                        continue;
                    }
                    let state = world.block(p);
                    if !wood_door(state) {
                        continue;
                    }
                    let found = added
                        .iter_mut()
                        .find(|d| same_door(d.position, p))
                        .or_else(|| {
                            self.villages
                                .iter_mut()
                                .filter(|v| {
                                    v.active
                                        && distance(p, v.center)
                                            <= f64::from(v.radius.wrapping_mul(v.radius))
                                })
                                .find_map(|v| v.doors.iter_mut().find(|d| same_door(d.position, p)))
                        });
                    if let Some(door) = found {
                        door.timestamp = self.ticks;
                        continue;
                    }
                    let lower = if state & 8 != 0 {
                        world.block(p - IVec3::Y)
                    } else {
                        state
                    };
                    let facing =
                        [IVec3::X, IVec3::Z, IVec3::NEG_X, IVec3::NEG_Z][(lower & 3) as usize];
                    let sky_count = |direction, cap| {
                        let mut count = 0;
                        for step in 1..=5 {
                            if world.sky(p + direction * step) {
                                count += 1;
                                if count >= cap {
                                    break;
                                }
                            }
                        }
                        count
                    };
                    let front = sky_count(facing, 5);
                    let back = sky_count(-facing, front + 1);
                    if front != back {
                        added.push(Door {
                            position: p,
                            inside: if front < back { facing } else { -facing },
                            timestamp: self.ticks,
                            restriction: 0,
                        });
                    }
                }
            }
        }
        for door in added {
            let index = self.nearest(door.position).unwrap_or_else(|| {
                self.villages.push(Village::default());
                self.villages.len() - 1
            });
            let village = &mut self.villages[index];
            village.sum += door.position;
            village.last_added = door.timestamp;
            village.doors.push(door);
            village.measure();
        }
    }
}
impl Village {
    fn measure(&mut self) {
        let count = self.doors.len() as i32;
        if count == 0 {
            self.center = IVec3::ZERO;
            self.radius = 0;
            return;
        }
        self.center = self.sum / count;
        let farthest = self
            .doors
            .iter()
            .map(|d| distance(d.position, self.center) as i32)
            .max()
            .unwrap_or(0);
        self.radius = 32.max(f64::from(farthest).sqrt() as i32 + 1);
    }
    fn tick(&mut self, tick: i32, world: &mut impl VillageWorld, random: &mut JavaRandom) {
        self.last_tick = tick;
        let reset = random.next_int(50) == 0;
        let count = self.doors.len();
        self.doors.retain_mut(|d| {
            if reset {
                d.restriction = 0;
            }
            if !world.loaded(d.position) {
                return true;
            }
            let live = wood_door(world.block(d.position))
                && tick.wrapping_sub(d.timestamp).wrapping_abs() <= 1200;
            if !live {
                self.sum -= d.position;
            }
            live
        });
        if self.doors.len() != count {
            self.measure();
        }
        self.expire_aggressors(tick, |a| world.alive(a));
        let extent = DVec3::new(f64::from(self.radius), 4., f64::from(self.radius));
        let bounds = Aabb {
            min: self.center.as_dvec3() - extent,
            max: self.center.as_dvec3() + extent,
        };
        if tick % 20 == 0 {
            self.residents = world.residents(bounds);
            if self.residents == 0 {
                self.reputation.clear();
            }
        }
        if tick % 30 == 0 {
            self.golems = world.golems(bounds);
        }
        if self.golems < self.residents / 10 && self.doors.len() > 20 && random.next_int(7000) == 0
        {
            for _ in 0..10 {
                let p = self.center
                    + IVec3::new(
                        random.next_int(16) - 8,
                        random.next_int(6) - 3,
                        random.next_int(16) - 8,
                    );
                if distance(self.center, p) >= f64::from(self.radius.wrapping_mul(self.radius))
                    || !spawning::solid_top(world.block(p - IVec3::Y))
                {
                    continue;
                }
                let free = (p.x - 1..p.x + 1).all(|x| {
                    (p.y..p.y + 4).all(|y| {
                        (p.z - 1..p.z + 1)
                            .all(|z| !material::normal_cube(world.block(IVec3::new(x, y, z))))
                    })
                });
                if free {
                    world.spawn_golem(p);
                    self.golems += 1;
                    break;
                }
            }
        }
    }
}
