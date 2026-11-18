//! EntityRenderer.getMouseOver, including the original World / Chunk query order.
use super::{movement::Aabb, ray_trace};
use glam::{DVec3, IVec3};

#[derive(Clone, Copy, Debug)]
pub struct Candidate<I> {
    pub id: I,
    pub bounds: Aabb,
    pub section: [i32; 3],
    pub inserted: u64,
    pub border: f32,
    pub living_or_frame: bool,
}
pub fn section(feet: DVec3) -> [i32; 3] {
    let p = (feet / 16.).floor().as_ivec3();
    [p.x, p.z, p.y.clamp(0, 15)]
}
pub fn body(feet: DVec3, width: f32, height: f32) -> Aabb {
    let radius = f64::from(width / 2.);
    Aabb {
        min: feet - DVec3::new(radius, 0., radius),
        max: feet + DVec3::new(radius, f64::from(height), radius),
    }
}
pub fn relative_hit(point: DVec3, position: DVec3) -> [f32; 3] {
    (point - position)
        .to_array()
        .map(|v| if v == 0. { 0. } else { v as f32 })
}
pub fn frame(anchor: IVec3, facing: i32) -> (DVec3, Aabb) {
    let normal =
        [IVec3::Z, IVec3::NEG_X, IVec3::NEG_Z, IVec3::X][(facing % 4).unsigned_abs() as usize];
    let center = anchor.as_dvec3() + DVec3::splat(0.5) - normal.as_dvec3() * 0.46875;
    let half = if normal.z != 0 {
        DVec3::new(0.375, 0.375, 0.03125)
    } else {
        DVec3::new(0.03125, 0.375, 0.375)
    };
    (
        center,
        Aabb {
            min: center - half,
            max: center + half,
        },
    )
}
pub fn object_position(kind: u8, data: i32, packet: DVec3) -> DVec3 {
    match kind {
        71 => frame(packet.floor().as_ivec3(), data).0,
        77 => packet.floor() + DVec3::splat(0.5),
        _ => packet,
    }
}
pub fn object_bounds(
    kind: u8,
    data: i32,
    feet: DVec3,
    armor_flags: u8,
) -> Option<(Aabb, f32, bool)> {
    let size = |w, h, border, living| (body(feet, w, h), border, living);
    Some(match kind {
        1 => size(1.5, 0.6, 0.1, false),
        10 => size(0.98, 0.7, 0.1, false),
        50 | 70 => size(0.98, 0.98, 0.1, false),
        51 => size(2., 2., 0.1, false),
        63 => size(1., 1., 1., false),
        71 => (frame(feet.floor().as_ivec3(), data).1, 0., true),
        77 => (
            Aabb {
                min: feet - DVec3::new(0.1875, 0.125, 0.1875),
                max: feet + DVec3::new(0.1875, 0.375, 0.1875),
            },
            0.1,
            false,
        ),
        78 if armor_flags & 16 == 0 => {
            if armor_flags & 1 != 0 {
                size(0.25, 0.9875, 0.1, true)
            } else {
                size(0.5, 1.975, 0.1, true)
            }
        }
        _ => return None,
    })
}
#[derive(Clone, Copy, Debug)]
pub enum Hit<I> {
    Block(ray_trace::Hit),
    Miss {
        point: DVec3,
        position: IVec3,
        face: Option<u8>,
    },
    Entity {
        id: I,
        point: DVec3,
        pointed: bool,
    },
}
impl<I: Copy> Hit<I> {
    pub fn block(self) -> Option<ray_trace::Hit> {
        if let Self::Block(hit) = self {
            Some(hit)
        } else {
            None
        }
    }
    pub fn entity(self) -> Option<I> {
        if let Self::Entity { id, .. } = self {
            Some(id)
        } else {
            None
        }
    }
}
#[derive(Clone, Copy)]
pub struct View {
    pub bounds: Aabb,
    pub eye: DVec3,
    pub look: DVec3,
    pub creative: bool,
}
impl View {
    pub fn pick<I: Copy + Eq>(
        self,
        world: &impl ray_trace::Access,
        entities: impl IntoIterator<Item = Candidate<I>>,
        riding: Option<I>,
    ) -> Option<Hit<I>> {
        let reach = if self.creative { 5. } else { 4.5 };
        let block = ray_trace::trace(
            world,
            self.eye,
            self.eye + self.look * reach,
            ray_trace::Options::PICK,
        );
        let reach = if self.creative { 6. } else { reach };
        let limit = block.map_or(reach, |hit| distance(hit.point, self.eye));
        let delta = self.look * reach;
        let query = self.bounds.sweep(delta).expand(DVec3::ONE);
        let lower = section(query.min - DVec3::splat(2.));
        let upper = section(query.max + DVec3::splat(2.));
        let mut candidates: Vec<_> = entities
            .into_iter()
            .filter(|e| {
                query.intersects(e.bounds)
                    && (0..3).all(|i| lower[i] <= e.section[i] && e.section[i] <= upper[i])
            })
            .collect();
        candidates.sort_by_key(|e| (e.section, e.inserted));
        let mut chosen = None;
        let mut nearest = limit;
        for e in candidates {
            let b = e.bounds.expand(DVec3::splat(f64::from(e.border)));
            let bounds = [b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z];
            let hit = ray_trace::intercept(bounds, self.eye, self.eye + delta).map(|(_, p)| p);
            if self.eye.cmpgt(b.min).all() && self.eye.cmplt(b.max).all() {
                if nearest >= 0. {
                    chosen = Some((e, hit.unwrap_or(self.eye)));
                    nearest = 0.;
                }
                continue;
            }
            let Some(point) = hit else { continue };
            let d = distance(self.eye, point);
            if d < nearest || nearest == 0. {
                if Some(e.id) == riding {
                    if nearest == 0. {
                        chosen = Some((e, point));
                    }
                } else {
                    chosen = Some((e, point));
                    nearest = d;
                }
            }
        }
        if let Some((e, point)) = chosen {
            if !self.creative && distance(self.eye, point) > 3. {
                return Some(Hit::Miss {
                    point,
                    position: point.floor().as_ivec3(),
                    face: None,
                });
            }
            if nearest < limit || block.is_none() {
                return Some(Hit::Entity {
                    id: e.id,
                    point,
                    pointed: e.living_or_frame,
                });
            }
        }
        block.map(|h| match h.kind {
            ray_trace::Kind::Block => Hit::Block(h),
            ray_trace::Kind::Miss => Hit::Miss {
                point: h.point,
                position: h.position,
                face: Some(h.face),
            },
        })
    }
}
fn distance(a: DVec3, b: DVec3) -> f64 {
    let d = b - a;
    f64::from((d.x * d.x + d.y * d.y + d.z * d.z).sqrt() as f32)
}
#[cfg(test)]
#[path = "entity_pick_tests.rs"]
mod tests;
