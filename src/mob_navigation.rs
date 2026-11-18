//! PathNavigateGround waypoint selection and side-effect-free move command.
use super::{passable, Walker};
use crate::vanilla::movement::{collision, Aabb, MovementWorld};
use glam::{DVec3, IVec3};

#[derive(Clone, Debug)]
struct Route {
    points: Vec<IVec3>,
    index: usize,
    length: i32,
}
#[derive(Clone, Debug, Default)]
pub struct Navigator {
    route: Option<Route>,
    ticks: i32,
    checked_at: i32,
    checked_position: DVec3,
}
fn position(world: &impl MovementWorld, walker: Walker, swim: bool) -> DVec3 {
    let pos = walker.position;
    let y = if !swim || !walker.in_water {
        (pos.y + 0.5) as i32
    } else {
        let start = pos.y as i32;
        let mut at = pos.floor().as_ivec3();
        at.y = start;
        while matches!(world.get(at) >> 4, 8 | 9) {
            at.y += 1;
            if at.y - start > 16 {
                at.y = start;
                break;
            }
        }
        at.y
    };
    DVec3::new(pos.x, f64::from(y), pos.z)
}
fn waypoint(p: IVec3, width: f32) -> DVec3 {
    let offset = f64::from((width + 1.) as i32) * 0.5;
    p.as_dvec3() + DVec3::new(offset, 0., offset)
}
#[derive(Clone, Copy)]
struct Clearance {
    size: IVec3,
    from: DVec3,
    dx: f64,
    dz: f64,
    wet: bool,
}
fn safe(world: &impl MovementWorld, p: IVec3, check: Clearance) -> bool {
    let x0 = p.x - check.size.x / 2;
    let z0 = p.z - check.size.z / 2;
    for x in x0..x0 + check.size.x {
        for z in z0..z0 + check.size.z {
            let dx = f64::from(x) + 0.5 - check.from.x;
            let dz = f64::from(z) + 0.5 - check.from.z;
            if dx * check.dx + dz * check.dz < 0. {
                continue;
            }
            for y in p.y..p.y + check.size.y {
                let q = IVec3::new(x, y, z);
                if !passable(world, q, world.get(q)) {
                    return false;
                }
            }
            let below = world.get(IVec3::new(x, p.y - 1, z)) >> 4;
            if below == 0 || matches!(below, 10 | 11) || matches!(below, 8 | 9) && !check.wet {
                return false;
            }
        }
    }
    true
}
pub fn direct(world: &impl MovementWorld, from: DVec3, to: DVec3, size: IVec3, wet: bool) -> bool {
    let mut x = from.x.floor() as i32;
    let mut z = from.z.floor() as i32;
    let y = from.y as i32;
    let mut dx = to.x - from.x;
    let mut dz = to.z - from.z;
    let length = dx * dx + dz * dz;
    if length < 1.0e-8 {
        return false;
    }
    let inv = 1. / length.sqrt();
    dx *= inv;
    dz *= inv;
    let check = Clearance {
        size,
        from,
        dx,
        dz,
        wet,
    };
    if !safe(
        world,
        IVec3::new(x, y, z),
        Clearance {
            size: size + IVec3::new(2, 0, 2),
            ..check
        },
    ) {
        return false;
    }
    let step_x = if dx < 0. { -1 } else { 1 };
    let step_z = if dz < 0. { -1 } else { 1 };
    let end_x = to.x.floor() as i32;
    let end_z = to.z.floor() as i32;
    let mut next_x = (f64::from(x) - from.x + if dx >= 0. { 1. } else { 0. }) / dx;
    let mut next_z = (f64::from(z) - from.z + if dz >= 0. { 1. } else { 0. }) / dz;
    let delta_x = 1. / dx.abs();
    let delta_z = 1. / dz.abs();
    while (end_x - x) * step_x > 0 || (end_z - z) * step_z > 0 {
        if next_x < next_z {
            next_x += delta_x;
            x += step_x;
        } else {
            next_z += delta_z;
            z += step_z;
        }
        if !safe(world, IVec3::new(x, y, z), check) {
            return false;
        }
    }
    true
}
impl Navigator {
    #[cfg(test)]
    pub(super) fn cursor(&self) -> Option<[i32; 2]> {
        self.route.as_ref().map(|r| [r.index as i32, r.length])
    }
    pub fn clear(&mut self) {
        self.route = None;
    }
    pub fn no_path(&self) -> bool {
        self.route
            .as_ref()
            .is_none_or(|r| r.index as i32 >= r.length)
    }
    pub fn set(
        &mut self,
        world: &impl MovementWorld,
        walker: Walker,
        swim: bool,
        points: Option<Vec<IVec3>>,
    ) -> bool {
        let Some(points) = points else {
            self.clear();
            return false;
        };
        if self.route.as_ref().is_none_or(|r| r.points != points) {
            self.route = Some(Route {
                index: 0,
                length: points.len() as i32,
                points,
            });
        }
        if self.route.as_ref().unwrap().length == 0 {
            return false;
        }
        self.checked_at = self.ticks;
        self.checked_position = position(world, walker, swim);
        true
    }
    /// Vanilla's avoid-sun path truncation is applied by the sunlight-aware
    /// goal (e.g. skeletons). It may set a negative logical path length.
    pub fn avoid_sun(&mut self, exposed: impl Fn(IVec3) -> bool, walker: Walker) {
        let at = walker.position;
        if exposed(IVec3::new(
            at.x.floor() as i32,
            (at.y + 0.5) as i32,
            at.z.floor() as i32,
        )) {
            return;
        }
        if let Some(route) = &mut self.route {
            for i in 0..route.length.max(0) as usize {
                if exposed(route.points[i]) {
                    route.length = i as i32 - 1;
                    return;
                }
            }
        }
    }
    pub fn tick(
        &mut self,
        world: &impl MovementWorld,
        walker: Walker,
        grounded: bool,
        swim: bool,
        requirement: f32,
    ) -> Option<DVec3> {
        self.ticks = self.ticks.wrapping_add(1);
        if self.no_path() {
            return None;
        }
        let entity = position(world, walker, swim);
        let route = self.route.as_mut().unwrap();
        if grounded || swim && (walker.in_water || walker.in_lava) {
            let end = (route.index..route.length as usize)
                .find(|i| route.points[*i].y != entity.y as i32)
                .unwrap_or(route.length as usize);
            let near = f64::from(walker.width * walker.width * requirement);
            for i in route.index..end {
                if entity.distance_squared(waypoint(route.points[i], walker.width)) < near {
                    route.index = i + 1;
                }
            }
            let width = walker.width.ceil() as i32;
            let size = IVec3::new(width, walker.height as i32 + 1, width);
            for i in (route.index..end).rev() {
                if direct(
                    world,
                    entity,
                    waypoint(route.points[i], walker.width),
                    size,
                    walker.in_water,
                ) {
                    route.index = i;
                    break;
                }
            }
            if self.ticks.wrapping_sub(self.checked_at) > 100 {
                let stuck = entity.distance_squared(self.checked_position) < 2.25;
                self.checked_position = entity;
                self.checked_at = self.ticks;
                if stuck {
                    self.clear();
                    return None;
                }
            }
        } else {
            let next = waypoint(route.points[route.index], walker.width);
            if entity.y > next.y
                && !grounded
                && entity.x.floor() == next.x.floor()
                && entity.z.floor() == next.z.floor()
            {
                route.index += 1;
            }
        }
        if self.no_path() {
            return None;
        }
        let route = self.route.as_ref().unwrap();
        let next = waypoint(route.points[route.index], walker.width);
        let cell = Aabb {
            min: next - DVec3::splat(0.5),
            max: next + DVec3::splat(0.5),
        };
        let raised = cell.offset(DVec3::Y);
        let mut dy = -1.;
        for block in collision::colliders(
            world,
            Aabb {
                min: cell.min - DVec3::Y,
                max: cell.max,
            },
        ) {
            if raised.max.x <= block.min.x
                || raised.min.x >= block.max.x
                || raised.max.z <= block.min.z
                || raised.min.z >= block.max.z
            {
                continue;
            }
            if raised.min.y >= block.max.y {
                dy = f64::max(dy, block.max.y - raised.min.y);
            }
        }
        Some(next + DVec3::Y * dy)
    }
}
