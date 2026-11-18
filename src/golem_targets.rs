//! Target tasks registered by EntityIronGolem. Each goal owns its own reach
//! cache as EntityAITarget does; that cache can span two different candidates.
use super::super::goals::{Goals, Selector};
use crate::vanilla::{movement::Aabb, random::JavaRandom};
use glam::{DVec3, IVec3};
pub type ActorId = u64;
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Player,
    Creeper,
    Ghast,
    Hostile,
    Passive,
}
pub fn can_attack_kind(created: bool, kind: Kind) -> bool {
    !matches!(kind, Kind::Creeper | Kind::Ghast) && !(created && kind == Kind::Player)
}
#[derive(Clone, Copy)]
pub struct Actor {
    pub id: ActorId,
    pub position: DVec3,
    pub bounds: Aabb,
    pub kind: Kind,
    pub alive: bool,
    pub invisible: bool,
    pub immune: bool,
    pub same_team: bool,
}
pub trait World {
    fn actor(&self, id: ActorId) -> Option<Actor>;
    fn entities(&self, bounds: Aabb) -> Vec<ActorId>;
    fn nearest_aggressor(&self) -> Option<ActorId>;
    fn low_reputation_player(&self) -> Option<ActorId>;
    fn path_end(&mut self, id: ActorId) -> Option<IVec3>;
}
#[derive(Clone, Copy, Default)]
struct Search {
    status: i32,
    delay: i32,
}
#[derive(Clone, Copy)]
pub struct Context {
    pub position: DVec3,
    pub bounds: Aabb,
    pub home: Option<(IVec3, f32)>,
    pub member: bool,
    pub player_created: bool,
    pub range: f64,
    pub revenge: Option<ActorId>,
    pub revenge_timer: i32,
}
#[derive(Clone)]
pub struct Targeting {
    tasks: Selector,
    search: [Search; 3],
    pending: [Option<ActorId>; 3],
    pub target: Option<ActorId>,
    revenge_timer: i32,
}
impl Default for Targeting {
    fn default() -> Self {
        let mut tasks = Selector::default();
        for action in 0..3 {
            tasks.add(action, i32::from(action) + 1, 1, true);
        }
        Self {
            tasks,
            search: [Search::default(); 3],
            pending: [None; 3],
            target: None,
            revenge_timer: 0,
        }
    }
}
impl Targeting {
    pub fn tick(&mut self, ctx: Context, world: &mut impl World, random: &mut JavaRandom) {
        let mut owner = Owner {
            ctx,
            world,
            random,
            search: &mut self.search,
            pending: &mut self.pending,
            target: &mut self.target,
            revenge_timer: &mut self.revenge_timer,
        };
        self.tasks.tick(&mut owner);
    }
}
struct Owner<'a, W> {
    ctx: Context,
    world: &'a mut W,
    random: &'a mut JavaRandom,
    search: &'a mut [Search; 3],
    pending: &'a mut [Option<ActorId>; 3],
    target: &'a mut Option<ActorId>,
    revenge_timer: &'a mut i32,
}
impl<W: World> Owner<'_, W> {
    fn suitable(&mut self, goal: usize, target: Option<ActorId>) -> bool {
        let Some(actor) = target.and_then(|id| self.world.actor(id)) else {
            return false;
        };
        if !actor.alive
            || actor.same_team
            || !can_attack_kind(self.ctx.player_created, actor.kind)
            || actor.kind == Kind::Player && actor.immune
        {
            return false;
        }
        if let Some((center, radius)) = self.ctx.home {
            if center.as_dvec3().distance_squared(actor.position.floor())
                >= f64::from(radius * radius)
            {
                return false;
            }
        }
        if goal == 1 {
            return true;
        }
        let search = &mut self.search[goal];
        search.delay = search.delay.wrapping_sub(1);
        if search.delay <= 0 {
            search.status = 0;
        }
        if search.status == 0 {
            search.delay = 10 + self.random.next_int(5);
            search.status = if self.world.path_end(actor.id).is_some_and(|end| {
                let p = actor.position.floor().as_ivec3();
                let dx = end.x.wrapping_sub(p.x);
                let dz = end.z.wrapping_sub(p.z);
                f64::from(dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz))) <= 2.25
            }) {
                1
            } else {
                2
            };
        }
        search.status != 2
    }
    fn defend(&mut self) -> bool {
        if !self.ctx.member {
            return false;
        }
        let candidate = self.world.nearest_aggressor();
        self.pending[0] = candidate;
        if candidate
            .and_then(|id| self.world.actor(id))
            .is_some_and(|a| a.kind == Kind::Creeper)
        {
            return false;
        }
        if self.suitable(0, candidate) {
            return true;
        }
        if self.random.next_int(20) == 0 {
            let player = self.world.low_reputation_player();
            self.pending[0] = player;
            self.suitable(0, player)
        } else {
            false
        }
    }
    fn nearest(&mut self) -> bool {
        if self.random.next_int(10) != 0 {
            return false;
        }
        let bounds = self
            .ctx
            .bounds
            .expand(DVec3::new(self.ctx.range, 4., self.ctx.range));
        let mut selected = None;
        let mut best = f64::MAX;
        for id in self.world.entities(bounds) {
            let Some(a) = self.world.actor(id) else {
                continue;
            };
            if !matches!(a.kind, Kind::Hostile | Kind::Ghast)
                || a.invisible
                || !self.suitable(2, Some(id))
            {
                continue;
            }
            let distance = a.position.distance_squared(self.ctx.position);
            if distance < best {
                best = distance;
                selected = Some(id);
            }
        }
        if selected.is_some() {
            self.pending[2] = selected;
        }
        selected.is_some()
    }
}
impl<W: World> Goals for Owner<'_, W> {
    fn should_start(&mut self, action: u16) -> bool {
        match action {
            0 => self.defend(),
            1 => {
                self.ctx.revenge_timer != *self.revenge_timer && self.suitable(1, self.ctx.revenge)
            }
            2 => self.nearest(),
            _ => unreachable!(),
        }
    }
    fn should_continue(&mut self, _: u16) -> bool {
        self.target
            .and_then(|id| self.world.actor(id))
            .is_some_and(|a| {
                a.alive
                    && !a.same_team
                    && a.position.distance_squared(self.ctx.position)
                        <= self.ctx.range * self.ctx.range
                    && !(a.kind == Kind::Player && a.immune)
            })
    }
    fn start(&mut self, action: u16) {
        if action == 1 {
            *self.target = self.ctx.revenge;
            *self.revenge_timer = self.ctx.revenge_timer;
        } else {
            *self.target = self.pending[action as usize];
        }
        self.search[action as usize] = Default::default();
    }
    fn reset(&mut self, _: u16) {
        *self.target = None;
    }
    fn update(&mut self, _: u16) {}
}
#[cfg(test)]
#[path = "golem_target_tests.rs"]
mod tests;
