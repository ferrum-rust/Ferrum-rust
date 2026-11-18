//! Eight EntityIronGolem action goals. Navigation supplies path requests and
//! accepts move commands independently of the goal selector's tick phase.
use super::super::{
    destination::{self, Direction, Search},
    goals::{Goals, Selector},
};
use super::{
    targets::{Actor, ActorId},
    Golem,
};
use glam::{DVec3, IVec3};

type Path = Vec<IVec3>;
const ATTACK: u16 = 0;
const TOWARD: u16 = 1;
const VILLAGE: u16 = 2;
const HOME: u16 = 3;
const ROSE: u16 = 4;
const WANDER: u16 = 5;
const WATCH: u16 = 6;
const IDLE: u16 = 7;
pub trait World {
    fn actor(&self, id: ActorId) -> Option<Actor>;
    fn nearest_door(&self, center: IVec3, used: &[IVec3]) -> Option<IVec3>;
    fn nearest_villager(&self) -> Option<ActorId>;
    fn nearest_player(&self, range: f64) -> Option<ActorId>;
    fn path_entity(&mut self, id: ActorId) -> Option<Path>;
    fn path_block(&mut self, p: IVec3, break_doors: bool) -> Option<Path>;
    fn set_path(&mut self, path: Option<Path>, speed: f64);
    fn clear_path(&mut self);
    fn no_path(&self) -> bool;
    fn entity_eye(&self, id: ActorId) -> Option<DVec3>;
    fn look_entity(&mut self, id: ActorId, yaw: f32, pitch: f32);
    fn look_position(&mut self, p: DVec3, yaw: f32, pitch: f32);
    fn strike(&mut self, id: ActorId, core: &mut Golem);
    fn rose(&mut self, hold: bool, core: &mut Golem);
}
#[derive(Clone, Copy)]
pub struct Context {
    pub position: DVec3,
    pub eye: DVec3,
    pub width: f32,
    pub age: i32,
    pub day: bool,
    pub home: Option<(IVec3, f32)>,
    pub target: Option<ActorId>,
}
#[derive(Clone, Debug, Default)]
struct State {
    attack_path: Option<Path>,
    attack_delay: i32,
    attack_ticks: i32,
    last_target: DVec3,
    pursuing: Option<ActorId>,
    destination: IVec3,
    door: Option<IVec3>,
    used_doors: Vec<IVec3>,
    door_path: Option<Path>,
    break_doors: bool,
    rose_target: Option<ActorId>,
    rose_ticks: i32,
    watched: Option<ActorId>,
    watch_ticks: i32,
    idle_ticks: i32,
    idle_x: f64,
    idle_z: f64,
}
#[derive(Clone)]
pub struct ActionGoals {
    tasks: Selector,
    state: State,
}
impl Default for ActionGoals {
    fn default() -> Self {
        let mut tasks = Selector::default();
        for (id, priority, mutex) in [
            (ATTACK, 1, 3),
            (TOWARD, 2, 1),
            (VILLAGE, 3, 1),
            (HOME, 4, 1),
            (ROSE, 5, 3),
            (WANDER, 6, 1),
            (WATCH, 7, 2),
            (IDLE, 8, 3),
        ] {
            tasks.add(id, priority, mutex, true);
        }
        Self {
            tasks,
            state: State::default(),
        }
    }
}
impl ActionGoals {
    pub fn tick(&mut self, ctx: Context, world: &mut impl World, golem: &mut Golem) {
        self.tasks.tick(&mut Owner {
            ctx,
            world,
            core: golem,
            state: &mut self.state,
        });
    }
    #[cfg(test)]
    fn active(&self) -> Vec<u16> {
        self.tasks.active_actions()
    }
}
struct Owner<'a, W> {
    ctx: Context,
    world: &'a mut W,
    core: &'a mut Golem,
    state: &'a mut State,
}
impl<W: World> Owner<'_, W> {
    fn target(&self) -> Option<Actor> {
        self.ctx.target.and_then(|id| self.world.actor(id))
    }
    fn sample(&mut self, h: i32, v: i32, direction: Direction) -> Option<IVec3> {
        destination::choose(
            &mut self.core.random,
            Search {
                position: self.ctx.position,
                home: self.ctx.home,
                horizontal: h,
                vertical: v,
                direction,
            },
            |_| 0.,
        )
    }
    fn inside_home(&self, position: DVec3) -> bool {
        self.ctx.home.is_none_or(|(center, r)| {
            center.as_dvec3().distance_squared(position.floor()) < f64::from(r * r)
        })
    }
    fn village(&mut self) -> bool {
        if self.state.used_doors.len() > 15 {
            self.state.used_doors.remove(0);
        }
        if self.ctx.day {
            return false;
        }
        self.state.door = self
            .world
            .nearest_door(self.ctx.position.floor().as_ivec3(), &self.state.used_doors);
        let Some(door) = self.state.door else {
            return false;
        };
        // MoveThroughVillage remembers getEnterDoors and restores that value
        // via setBreakDoors, not the previous breakDoors flag.
        self.state.door_path = self.world.path_block(door, false);
        self.state.break_doors = true;
        if self.state.door_path.is_some() {
            return true;
        }
        let Some(intermediate) = self.sample(10, 7, Direction::Towards(door.as_dvec3())) else {
            return false;
        };
        self.state.door_path = self.world.path_block(intermediate, false);
        self.state.door_path.is_some()
    }
    fn seek(&mut self, h: i32, v: i32, direction: Direction) -> bool {
        if let Some(p) = self.sample(h, v, direction) {
            self.state.destination = p;
            true
        } else {
            false
        }
    }
    fn move_to_destination(&mut self, speed: f64) {
        let path = self
            .world
            .path_block(self.state.destination, self.state.break_doors);
        self.world.set_path(path, speed);
    }
    fn attack_tick(&mut self) {
        let Some(target) = self.target() else {
            return;
        };
        self.world.look_entity(target.id, 30., 30.);
        let distance = self.ctx.position.distance_squared(target.position);
        self.state.attack_delay = self.state.attack_delay.wrapping_sub(1);
        if self.state.attack_delay <= 0
            && (self.state.last_target == DVec3::ZERO
                || target.position.distance_squared(self.state.last_target) >= 1.
                || self.core.random.next_float() < 0.05)
        {
            self.state.last_target = target.position;
            self.state.attack_delay = 4 + self.core.random.next_int(7);
            if distance > 1024. {
                self.state.attack_delay += 10;
            } else if distance > 256. {
                self.state.attack_delay += 5;
            }
            if let Some(path) = self.world.path_entity(target.id) {
                self.world.set_path(Some(path), 1.);
            } else {
                self.state.attack_delay += 15;
            }
        }
        self.state.attack_ticks = (self.state.attack_ticks - 1).max(0);
        let reach = self.ctx.width * 2.;
        let target_width = (target.bounds.max.x - target.bounds.min.x) as f32;
        if distance <= f64::from(reach * reach + target_width) && self.state.attack_ticks <= 0 {
            self.state.attack_ticks = 20;
            self.world.strike(target.id, self.core);
        }
    }
}
impl<W: World> Goals for Owner<'_, W> {
    fn should_start(&mut self, id: u16) -> bool {
        match id {
            ATTACK => {
                let Some(target) = self.target().filter(|a| a.alive) else {
                    return false;
                };
                self.state.attack_path = self.world.path_entity(target.id);
                self.state.attack_path.is_some()
            }
            TOWARD => {
                self.state.pursuing = self.ctx.target;
                let Some(target) = self
                    .target()
                    .filter(|a| a.position.distance_squared(self.ctx.position) <= 1024.)
                else {
                    return false;
                };
                self.seek(16, 7, Direction::Towards(target.position))
            }
            VILLAGE => self.village(),
            HOME => {
                if self.inside_home(self.ctx.position) {
                    return false;
                }
                self.seek(
                    16,
                    7,
                    Direction::Towards(self.ctx.home.unwrap().0.as_dvec3()),
                )
            }
            ROSE => {
                if !self.ctx.day || self.core.random.next_int(8000) != 0 {
                    return false;
                }
                self.state.rose_target = self.world.nearest_villager();
                self.state.rose_target.is_some()
            }
            WANDER => {
                self.ctx.age < 100
                    && self.core.random.next_int(120) == 0
                    && self.seek(10, 7, Direction::Any)
            }
            WATCH => {
                if self.core.random.next_float() >= 0.02 {
                    return false;
                }
                self.state.watched = self.world.nearest_player(6.);
                self.state.watched.is_some()
            }
            IDLE => self.core.random.next_float() < 0.02,
            _ => unreachable!(),
        }
    }
    fn should_continue(&mut self, id: u16) -> bool {
        match id {
            ATTACK => self
                .target()
                .is_some_and(|a| a.alive && self.inside_home(a.position)),
            TOWARD => {
                !self.world.no_path()
                    && self
                        .state
                        .pursuing
                        .and_then(|id| self.world.actor(id))
                        .is_some_and(|a| {
                            a.alive && a.position.distance_squared(self.ctx.position) < 1024.
                        })
            }
            VILLAGE => {
                !self.world.no_path()
                    && self.state.door.is_some_and(|d| {
                        d.as_dvec3().distance_squared(self.ctx.position)
                            > f64::from((self.ctx.width + 4.) * (self.ctx.width + 4.))
                    })
            }
            HOME | WANDER => !self.world.no_path(),
            ROSE => self.state.rose_ticks > 0,
            WATCH => self
                .state
                .watched
                .and_then(|id| self.world.actor(id))
                .is_some_and(|a| {
                    a.alive
                        && a.position.distance_squared(self.ctx.position) <= 36.
                        && self.state.watch_ticks > 0
                }),
            IDLE => self.state.idle_ticks >= 0,
            _ => unreachable!(),
        }
    }
    fn start(&mut self, id: u16) {
        match id {
            ATTACK => {
                self.world.set_path(self.state.attack_path.take(), 1.);
                self.state.attack_delay = 0;
            }
            TOWARD => self.move_to_destination(0.9),
            VILLAGE => self.world.set_path(self.state.door_path.take(), 0.6),
            HOME => self.move_to_destination(1.),
            ROSE => {
                self.state.rose_ticks = 400;
                self.world.rose(true, self.core);
            }
            WANDER => self.move_to_destination(0.6),
            WATCH => self.state.watch_ticks = 40 + self.core.random.next_int(40),
            IDLE => {
                let angle = std::f64::consts::PI * 2. * self.core.random.next_double();
                (self.state.idle_x, self.state.idle_z) = crate::vanilla::math::unit_circle(angle);
                self.state.idle_ticks = 20 + self.core.random.next_int(20);
            }
            _ => unreachable!(),
        }
    }
    fn reset(&mut self, id: u16) {
        match id {
            ATTACK => self.world.clear_path(),
            TOWARD => self.state.pursuing = None,
            VILLAGE => {
                if let Some(d) = self.state.door.filter(|d| {
                    self.world.no_path() || d.as_dvec3().distance_squared(self.ctx.position) < 16.
                }) {
                    self.state.used_doors.push(d);
                }
            }
            ROSE => {
                self.world.rose(false, self.core);
                self.state.rose_target = None;
            }
            WATCH => self.state.watched = None,
            _ => {}
        }
    }
    fn update(&mut self, id: u16) {
        match id {
            ATTACK => self.attack_tick(),
            ROSE => {
                if let Some(v) = self.state.rose_target {
                    self.world.look_entity(v, 30., 30.);
                }
                self.state.rose_ticks -= 1;
            }
            WATCH => {
                if let Some(p) = self.state.watched.and_then(|id| self.world.entity_eye(id)) {
                    self.world.look_position(p, 10., 40.);
                }
                self.state.watch_ticks -= 1;
            }
            IDLE => {
                self.state.idle_ticks -= 1;
                self.world.look_position(
                    self.ctx.eye + DVec3::new(self.state.idle_x, 0., self.state.idle_z),
                    10.,
                    40.,
                );
            }
            _ => {}
        }
    }
}
#[cfg(test)]
#[path = "golem_action_tests.rs"]
mod tests;
