use super::MovementWorld;
use glam::{DVec3, IVec3};
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Aabb {
    pub min: DVec3,
    pub max: DVec3,
}
impl Aabb {
    pub fn player(p: DVec3) -> Self {
        let radius = (0.6f32 / 2.) as f64;
        Self {
            min: p - DVec3::new(radius, 0., radius),
            max: p + DVec3::new(radius, 1.8f32 as f64, radius),
        }
    }
    pub fn feet(self) -> DVec3 {
        DVec3::new(
            (self.min.x + self.max.x) / 2.,
            self.min.y,
            (self.min.z + self.max.z) / 2.,
        )
    }
    pub fn offset(self, d: DVec3) -> Self {
        Self {
            min: self.min + d,
            max: self.max + d,
        }
    }
    pub fn expand(self, d: DVec3) -> Self {
        let first = self.min - d;
        let second = self.max + d;
        Self {
            min: first.min(second),
            max: first.max(second),
        }
    }
    pub(crate) fn sweep(self, d: DVec3) -> Self {
        Self {
            min: self.min + d.min(DVec3::ZERO),
            max: self.max + d.max(DVec3::ZERO),
        }
    }
    pub fn intersects(self, b: Self) -> bool {
        (0..3).all(|i| self.max[i] > b.min[i] && self.min[i] < b.max[i])
    }
    fn clip(self, b: Self, axis: usize, d: f64) -> f64 {
        if (0..3).any(|i| i != axis && (b.max[i] <= self.min[i] || b.min[i] >= self.max[i])) {
            return d;
        }
        if d > 0. && b.max[axis] <= self.min[axis] {
            d.min(self.min[axis] - b.max[axis])
        } else if d < 0. && b.min[axis] >= self.max[axis] {
            d.max(self.max[axis] - b.min[axis])
        } else {
            d
        }
    }
}
pub(crate) struct Movement {
    pub bounds: Aabb,
    pub grounded: bool,
    pub clipped: [bool; 3],
    pub delta: DVec3,
}
pub(crate) fn colliders(world: &impl MovementWorld, query: Aabb) -> Vec<Aabb> {
    let min = query.min.floor().as_ivec3();
    let max = (query.max + DVec3::ONE).floor().as_ivec3();
    let mut out = Vec::new();
    for x in min.x..max.x {
        for z in min.z..max.z {
            if !world.loaded_column(x, z) {
                continue;
            }
            for y in min.y - 1..max.y {
                let p = IVec3::new(x, y, z);
                world.block_boxes(p, world.get(p), &mut out);
            }
        }
    }
    out.retain(|b| b.intersects(query));
    world.extra_colliders(query, &mut out);
    out
}
fn clip_axis(obstacles: &[Aabb], body: &mut Aabb, axis: usize, mut delta: f64) -> f64 {
    for block in obstacles {
        delta = block.clip(*body, axis, delta);
    }
    let mut offset = DVec3::ZERO;
    offset[axis] = delta;
    *body = body.offset(offset);
    delta
}
fn reduce(value: f64) -> f64 {
    if (-0.05..0.05).contains(&value) {
        0.
    } else if value > 0. {
        value - 0.05
    } else {
        value + 0.05
    }
}
pub(crate) fn move_body(
    world: &impl MovementWorld,
    original: Aabb,
    delta: DVec3,
    grounded: bool,
    sneak: bool,
) -> Movement {
    move_body_with_step(world, original, delta, grounded, sneak, 0.6f32 as f64)
}
pub(crate) fn move_body_with_step(
    world: &impl MovementWorld,
    original: Aabb,
    mut delta: DVec3,
    grounded: bool,
    sneak: bool,
    step_height: f64,
) -> Movement {
    if sneak {
        while delta.x != 0.
            && colliders(world, original.offset(DVec3::new(delta.x, -1., 0.))).is_empty()
        {
            delta.x = reduce(delta.x);
        }
        while delta.z != 0.
            && colliders(world, original.offset(DVec3::new(0., -1., delta.z))).is_empty()
        {
            delta.z = reduce(delta.z);
        }
        while delta.x != 0.
            && delta.z != 0.
            && colliders(world, original.offset(DVec3::new(delta.x, -1., delta.z))).is_empty()
        {
            delta.x = reduce(delta.x);
            delta.z = reduce(delta.z);
        }
    }
    let obstacles = colliders(world, original.sweep(delta));
    let mut body = original;
    let mut moved = DVec3::ZERO;
    for axis in [1, 0, 2] {
        moved[axis] = clip_axis(&obstacles, &mut body, axis, delta[axis]);
    }
    if step_height > 0.
        && (grounded || delta.y != moved.y && delta.y < 0.)
        && (delta.x != moved.x || delta.z != moved.z)
    {
        let obstacles = colliders(
            world,
            original.sweep(DVec3::new(delta.x, step_height, delta.z)),
        );
        let mut choices = [(original, DVec3::ZERO); 2];
        for (i, (b, m)) in choices.iter_mut().enumerate() {
            let footprint = if i == 0 {
                original.sweep(DVec3::new(delta.x, 0., delta.z))
            } else {
                original
            };
            let mut rise = step_height;
            for block in &obstacles {
                rise = block.clip(footprint, 1, rise);
            }
            *b = original.offset(DVec3::Y * rise);
            m.x = clip_axis(&obstacles, b, 0, delta.x);
            m.z = clip_axis(&obstacles, b, 2, delta.z);
            m.y = rise;
        }
        let distance = |m: DVec3| m.x * m.x + m.z * m.z;
        let i = usize::from(distance(choices[0].1) <= distance(choices[1].1));
        let (mut candidate, mut motion) = choices[i];
        motion.y = clip_axis(&obstacles, &mut candidate, 1, -motion.y);
        if distance(moved) < distance(motion) {
            body = candidate;
            moved = motion;
        }
    }
    let clipped = [delta.x != moved.x, delta.y != moved.y, delta.z != moved.z];
    Movement {
        bounds: body,
        grounded: clipped[1] && delta.y < 0.,
        clipped,
        delta: moved,
    }
}
