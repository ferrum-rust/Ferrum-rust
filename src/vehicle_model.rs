//! The local 1.8.9 JAR provides panel geometry, UVs and textures. The server supplies each pose.
use super::*;
use std::sync::LazyLock;
#[path = "vehicle_transform.rs"]
pub mod pose;
pub use pose::{Actor, Damage};

type Quad = [[i32; 5]; 4];
static BODIES: LazyLock<[Vec<Quad>; 2]> = LazyLock::new(|| {
    serde_json::from_str(include_str!("vanilla/vehicle_models189.json"))
        .expect("1.8.9 vehicle model panels")
});
pub(super) fn warm_geometry() -> usize {
    LazyLock::force(&BODIES).len()
}
fn cargo(
    mesh: &mut Mesh,
    transform: Mat4,
    state: u16,
    offset: i32,
    assets: &Assets,
    resources: &Resources,
    light: [u8; 4],
    alpha: f32,
    tnt_fuse: Option<i32>,
) {
    let mut transform = transform
        * Mat4::from_scale(Vec3::splat(0.75))
        * Mat4::from_translation(Vec3::new(-0.5, (offset as f32 - 8.) / 16., 0.5));
    if let Some(fuse) = tnt_fuse.filter(|&f| f >= 0) {
        let swell = 1.0
            + (1.0 - ((fuse as f32 - alpha + 1.0) / 100.0).clamp(0.0, 1.0))
                .powi(2)
                .powi(3)
                * 0.3;
        transform *= Mat4::from_scale(Vec3::splat(swell));
    }
    let transform = transform * Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let start = mesh.vertices.len();
    if crate::chest_render::is_chest(state) {
        crate::chest_render::draw_cargo_chest(mesh, transform, state, resources, light);
    } else if let Some(faces) = assets.block_models.get(state) {
        for quad in faces {
            let base = mesh.vertices.len() as u32;
            for v in &quad.vertices {
                mesh.vertices.push(Vertex {
                    position: transform.transform_point3(v.position).to_array(),
                    uv: v.uv,
                    layer: quad.layer,
                    color: [quad.shade, quad.shade, quad.shade, 255],
                    light,
                });
            }
            mesh.indices.extend([0, 1, 2, 0, 2, 3].map(|i| base + i));
            mesh.visible_faces += 1;
        }
    } else {
        block_cube(
            mesh,
            transform * Mat4::from_translation(Vec3::splat(0.5)),
            state,
            1.,
            assets,
        );
        for v in &mut mesh.vertices[start..] {
            v.light = light;
        }
    }
    if let Some(fuse) = tnt_fuse.filter(|&f| f >= 0 && (f / 5) % 2 == 0) {
        let remaining = fuse as f32 - alpha + 1.0;
        let flash = (((1.0 - remaining / 100.0) * 0.8).clamp(0.0, 1.0) * 255.0) as u8;
        for v in &mut mesh.vertices[start..] {
            v.layer |= WHITE_FLASH;
            v.color[3] = flash;
        }
    }
}
pub fn draw(d: &mut Dynamic, c: &ModelContext<'_>, id: i32, a: &Actor) {
    draw_with_tnt_fuse(d, c, id, a, None);
}
pub fn draw_with_tnt_fuse(
    d: &mut Dynamic,
    c: &ModelContext<'_>,
    id: i32,
    a: &Actor,
    tnt_fuse: Option<i32>,
) {
    d.total += 1;
    let feet = a.from.lerp(a.to, f64::from(c.alpha)).as_vec3();
    let reach = c.settings.entity_distance as f32;
    let top = a
        .display
        .map_or(0.8, |(_, n)| (n as f32 / 16. * 0.75 + 1.).max(0.8));
    if feet.distance_squared(c.eye) > reach * reach
        || !c
            .frustum
            .visible(feet - Vec3::new(1., 0.5, 1.), feet + Vec3::new(1., top, 1.))
    {
        d.culled += 1;
        return;
    }
    d.visible += 1;
    crate::models::shadow_radius(&mut d.translucent, c, feet, 0.5, 1.0, a.cart);
    let transform = a.transform(id, c.alpha, &|p| c.world.get(p));
    let shaded = transform * Mat4::from_scale(Vec3::new(-1., -1., 1.));
    let texture = c.resources.sprite(if a.cart {
        "textures/entity/minecart.png"
    } else {
        "textures/entity/boat.png"
    });
    let uv = [
        texture.width as f32 / (64. * 256.),
        texture.height as f32 / (32. * 256.),
    ];
    let light = c
        .world
        .visual_light_uv((feet + Vec3::Y * 0.5).floor().as_ivec3(), 0);
    if let Some((state, offset)) = a.display {
        if state >> 4 != 0 {
            cargo(
                &mut d.opaque,
                transform,
                state,
                offset,
                c.assets,
                c.resources,
                light,
                c.alpha,
                tnt_fuse,
            );
        }
    }
    for quad in &BODIES[usize::from(a.cart)] {
        let points = quad.map(|v| {
            shaded.transform_point3(Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32) / 40960.)
        });
        let normal = (points[1] - points[0])
            .cross(points[2] - points[0])
            .normalize_or_zero();
        let shade = (crate::models::entity_face_shade(normal) * 255.) as u8;
        let start = d.opaque.vertices.len() as u32;
        for (p, v) in points.into_iter().zip(quad) {
            d.opaque.vertices.push(Vertex {
                position: p.to_array(),
                uv: [v[3] as f32 * uv[0], v[4] as f32 * uv[1]],
                layer: texture.layer | ATLAS,
                color: [shade, shade, shade, 255],
                light,
            });
        }
        d.opaque
            .indices
            .extend([0, 1, 2, 0, 2, 3].map(|i| start + i));
        d.opaque.visible_faces += 1;
    }
}
