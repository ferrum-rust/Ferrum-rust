//! Exact four-plane visibility evaluation; original planes remain in Frustum.
//! Ordered comparisons and bitwise corner selection preserve NaN/-0 handling.
//! Extra evaluated lanes do not promise identical floating exception flags.
use glam::{Vec3, Vec4};

struct FourPlanes { x: Vec4, y: Vec4, z: Vec4, w: Vec4 }
impl FourPlanes {
    fn new(p: [Vec4; 4]) -> Self {
        Self {
            x: Vec4::new(p[0].x, p[1].x, p[2].x, p[3].x),
            y: Vec4::new(p[0].y, p[1].y, p[2].y, p[3].y),
            z: Vec4::new(p[0].z, p[1].z, p[2].z, p[3].z),
            w: Vec4::new(p[0].w, p[1].w, p[2].w, p[3].w),
        }
    }
    #[inline]
    fn visible(&self, min: Vec3, max: Vec3) -> bool {
        let zero = Vec4::ZERO;
        let x = Vec4::select(self.x.cmpge(zero), Vec4::splat(max.x), Vec4::splat(min.x));
        let y = Vec4::select(self.y.cmpge(zero), Vec4::splat(max.y), Vec4::splat(min.y));
        let z = Vec4::select(self.z.cmpge(zero), Vec4::splat(max.z), Vec4::splat(min.z));
        (((self.x * x + self.y * y) + self.z * z) + self.w).cmpge(zero).all()
    }
}
pub(super) struct Planes { groups: [FourPlanes; 2] }
impl Planes {
    pub(super) fn new(planes: [Vec4; 6]) -> Self {
        Self { groups: [
            FourPlanes::new([planes[0], planes[1], planes[2], planes[3]]),
            // Duplicate real planes: a dummy zero plane rejects infinite boxes.
            FourPlanes::new([planes[4], planes[5], planes[4], planes[5]]),
        ] }
    }
    #[inline]
    pub(super) fn visible(&self, min: Vec3, max: Vec3) -> bool {
        self.groups[0].visible(min, max) && self.groups[1].visible(min, max)
    }
}
