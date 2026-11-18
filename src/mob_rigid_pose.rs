//! Interpolate ModelBox poses as rigid parts, rather than shrinking their vertices.
use super::{Frame, Tri, POSES, SPECIES};
use glam::{Mat3, Mat4, Quat, Vec3};
use std::{collections::BTreeMap, sync::LazyLock};

pub(super) fn point(triangle: &Tri, vertex: usize) -> Vec3 {
    let i = vertex * 5;
    Vec3::new(
        triangle[i] as f32,
        triangle[i + 1] as f32,
        triangle[i + 2] as f32,
    ) / 2048.
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Transform {
    pub center: Vec3,
    pub rotation: Quat,
}
impl Transform {
    pub fn interpolate(self, mut next: Self, t: f32) -> Self {
        if t == 0. {
            return self;
        }
        if t == 1. {
            return next;
        }
        if self.rotation.dot(next.rotation) < 0. {
            next.rotation = -next.rotation;
        }
        let delta = (next.rotation * self.rotation.conjugate()).normalize();
        let (axis, angle) = delta.to_axis_angle();
        let center = if angle.abs() < 0.001 {
            self.center.lerp(next.center, t)
        } else {
            // Screw interpolation follows the hinge arc instead of detaching the part at mid-pose.
            let displacement = next.center - self.center;
            let parallel = axis * displacement.dot(axis);
            let perpendicular = displacement - parallel;
            let pivot = (perpendicular + axis.cross(perpendicular) / (angle * 0.5).tan()) * 0.5;
            self.center + pivot - Quat::from_axis_angle(axis, angle * t) * pivot + parallel * t
        };
        Self {
            center,
            rotation: self.rotation.slerp(next.rotation, t).normalize(),
        }
    }
}

pub(super) struct Part {
    pub center: Vec3,
    basis: Option<(usize, Mat3)>,
    planar: bool,
}
#[derive(Clone, Copy)]
pub(super) struct Sample {
    pub transform: Transform,
    pub error: f32,
    pub valid: bool,
}
impl Part {
    #[cfg(test)]
    pub fn has_surface(&self) -> bool {
        self.basis.is_some()
    }
    fn new(triangles: &[Tri]) -> Self {
        let center = bounds_center(triangles);
        let basis = triangles
            .iter()
            .enumerate()
            .map(|(i, t)| {
                (
                    i,
                    (point(t, 1) - point(t, 0))
                        .cross(point(t, 2) - point(t, 0))
                        .length_squared(),
                )
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|(_, area)| *area > 1e-12)
            .and_then(|(i, _)| basis(&triangles[i]).map(|b| (i, b)));
        let planar = basis.is_some_and(|(_, basis)| {
            triangles
                .iter()
                .all(|t| (0..3).all(|j| (point(t, j) - center).dot(basis.z_axis).abs() < 0.001))
        });
        Self {
            center,
            basis,
            planar,
        }
    }
    fn fit(&self, rest: &[Tri], frame: &[Tri]) -> Sample {
        let identity = Transform {
            center: self.center,
            rotation: Quat::IDENTITY,
        };
        let Some((index, rest_basis)) = self.basis else {
            return Sample {
                transform: identity,
                error: 0.,
                valid: false,
            };
        };
        let center = bounds_center(frame);
        let Some(frame_basis) = basis(&frame[index]) else {
            return Sample {
                transform: identity,
                error: f32::INFINITY,
                valid: false,
            };
        };
        let mut covariance = Mat3::ZERO;
        for (a, b) in rest.iter().zip(frame) {
            for j in 0..3 {
                let p = point(a, j) - self.center;
                let q = point(b, j) - center;
                covariance += Mat3::from_cols(q * p.x, q * p.y, q * p.z);
            }
        }
        let length = |matrix: Mat3| {
            (matrix.x_axis.length_squared()
                + matrix.y_axis.length_squared()
                + matrix.z_axis.length_squared())
            .sqrt()
        };
        if self.planar {
            // Flat ModelBoxes have no covariance on their normal axis. Complete that axis
            // from both sides of the face before polar fitting; using one thin triangle
            // alone amplifies the fixture's 1/2048-block rounding.
            let mut a_normal = Vec3::ZERO;
            let mut b_normal = Vec3::ZERO;
            for (a, b) in rest.iter().zip(frame) {
                let a_cross = (point(a, 1) - point(a, 0)).cross(point(a, 2) - point(a, 0));
                if a_cross.length_squared() < 1e-12 {
                    continue;
                }
                let sign = if a_cross.dot(rest_basis.z_axis) < 0. {
                    -1.
                } else {
                    1.
                };
                a_normal += a_cross * sign;
                b_normal += (point(b, 1) - point(b, 0)).cross(point(b, 2) - point(b, 0)) * sign;
            }
            let a = a_normal.normalize_or_zero();
            let b = b_normal.normalize_or_zero();
            covariance += Mat3::from_cols(b * a.x, b * a.y, b * a.z) * length(covariance);
        }
        let norm = (covariance.x_axis.length_squared()
            + covariance.y_axis.length_squared()
            + covariance.z_axis.length_squared())
        .sqrt();
        let mut rotation = covariance / norm.max(f32::MIN_POSITIVE);
        if rotation.determinant() > 1e-12 {
            // Polar orthogonalization uses every corner, avoiding amplified rounding on thin limbs.
            for _ in 0..32 {
                let next = (rotation + rotation.inverse().transpose()) * 0.5;
                let converged = next.abs_diff_eq(rotation, 1e-6);
                rotation = next;
                if converged {
                    break;
                }
            }
        } else {
            // A flat wing has no invertible covariance, but its triangle still defines a frame.
            rotation = frame_basis * rest_basis.transpose();
        }
        let transform = Transform {
            center,
            rotation: Quat::from_mat3(&rotation).normalize(),
        };
        let mut error = 0_f32;
        for (a, b) in rest.iter().zip(frame) {
            for j in 0..3 {
                error = error.max(
                    (transform.rotation * (point(a, j) - self.center) + transform.center
                        - point(b, j))
                    .length(),
                );
            }
        }
        Sample {
            transform,
            error,
            valid: error <= 0.002,
        }
    }
    pub fn matrix(&self, from: Sample, to: Sample, fraction: f32, amount: f32) -> Mat4 {
        if !from.valid || !to.valid {
            return Mat4::IDENTITY;
        }
        let animated = from.transform.interpolate(to.transform, fraction);
        let transform = Transform {
            center: self.center,
            rotation: Quat::IDENTITY,
        }
        .interpolate(animated, amount.clamp(0., 1.));
        Mat4::from_rotation_translation(
            transform.rotation,
            transform.center - transform.rotation * self.center,
        )
    }
}

fn basis(t: &Tri) -> Option<Mat3> {
    let x = (point(t, 1) - point(t, 0)).normalize_or_zero();
    let z = x.cross(point(t, 2) - point(t, 0)).normalize_or_zero();
    (z.length_squared() > 0.5).then(|| Mat3::from_cols(x, z.cross(x), z))
}
fn bounds_center(triangles: &[Tri]) -> Vec3 {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = -min;
    for t in triangles {
        for j in 0..3 {
            let p = point(t, j);
            min = min.min(p);
            max = max.max(p);
        }
    }
    (min + max) * 0.5
}

pub(super) struct Rig {
    pub parts: Vec<Part>,
    pub frames: BTreeMap<usize, Vec<Sample>>,
}
impl Rig {
    fn new<'a>(rest: &[Tri], frames: impl Iterator<Item = &'a Frame>) -> Self {
        let parts: Vec<_> = rest.chunks(12).map(Part::new).collect();
        let frames = frames
            .filter(|frame| frame.len() == rest.len())
            .map(|frame| {
                let poses = parts
                    .iter()
                    .zip(rest.chunks(12))
                    .zip(frame.chunks(12))
                    .map(|((part, a), b)| part.fit(a, b))
                    .collect();
                (frame.as_ptr() as usize, poses)
            })
            .collect();
        Self { parts, frames }
    }
}

pub(super) static RIGS: LazyLock<BTreeMap<usize, Rig>> = LazyLock::new(|| {
    let mut rigs = BTreeMap::new();
    for (&id, species) in SPECIES.iter() {
        let additional = POSES.get(&id).filter(|_| !matches!(id, 54 | 55));
        rigs.insert(
            species.frames[0].as_ptr() as usize,
            Rig::new(
                &species.frames[0],
                species
                    .frames
                    .iter()
                    .chain(additional.into_iter().flatten()),
            ),
        );
    }
    for id in [54, 55] {
        let frames = &POSES[&id];
        rigs.insert(
            frames[0].as_ptr() as usize,
            Rig::new(&frames[0], frames.iter()),
        );
    }
    rigs
});
