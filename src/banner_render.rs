use super::texture::Region;
use crate::{
    mesh::Mesh,
    models::{ModelContext, ATLAS},
    vanilla::{item_nbt, math},
};
use glam::{IVec3, Mat4, Vec3};
use std::f32::consts::PI;

pub(crate) fn visible(c: &ModelContext<'_>, mesh: &mut Mesh) {
    let cam_cx = (c.eye.x.floor() as i32).div_euclid(16);
    let cam_cz = (c.eye.z.floor() as i32).div_euclid(16);
    for dz in -5..=5 {
        for dx in -5..=5 {
            let (cx, cz) = (cam_cx + dx, cam_cz + dz);
            let Some(chunk) = c.world.chunks.get(&(cx, cz)) else {
                continue;
            };
            if chunk.tiles.is_empty() {
                continue;
            }
            let cdx = (cx * 16 + 8) as f32 - c.eye.x;
            let cdz = (cz * 16 + 8) as f32 - c.eye.z;
            if cdx * cdx + cdz * cdz > 76.0 * 76.0 {
                continue;
            }
            for (key, tag) in &chunk.tiles {
                let [x, y, z] = *key;
                let state = chunk.get(x, y, z);
                if !matches!(state >> 4, 176 | 177) {
                    continue;
                }
                let pos = IVec3::new(cx * 16 + x, y, cz * 16 + z);
                let at = pos.as_vec3();
                if (at + Vec3::splat(0.5)).distance_squared(c.eye) >= 4096.
                    || !c.frustum.visible(at - Vec3::Y, at + Vec3::new(1., 2., 1.))
                {
                    continue;
                }
                let region = c
                    .resources
                    .banners
                    .region(c.resources, item_nbt::compound(tag), 0);
                draw(
                    mesh,
                    pos,
                    state,
                    c.sim.total_ticks,
                    c.alpha,
                    region,
                    c.world.visual_light_uv(pos, 0),
                );
            }
        }
    }
}
fn draw(
    mesh: &mut Mesh,
    pos: IVec3,
    state: u16,
    time: u64,
    partial: f32,
    region: Region,
    light: [u8; 4],
) {
    let at = pos.as_vec3();
    let standing = state >> 4 == 176;
    let meta = state & 15;
    let scale = 2. / 3.;
    let base = if standing {
        Mat4::from_translation(at + Vec3::new(0.5, 0.75 * scale, 0.5))
            * Mat4::from_rotation_y(-(meta as f32) * PI / 8.)
    } else {
        let degrees: f32 = match meta {
            2 => 180.,
            4 => 90.,
            5 => -90.,
            _ => 0.,
        };
        Mat4::from_translation(at + Vec3::new(0.5, -0.25 * scale, 0.5))
            * Mat4::from_rotation_y((-degrees).to_radians())
            * Mat4::from_translation(Vec3::new(0., -0.3125, -0.4375))
    } * Mat4::from_scale(Vec3::new(scale, -scale, -scale) / 16.);
    let phase = (pos
        .x
        .wrapping_mul(7)
        .wrapping_add(pos.y.wrapping_mul(9))
        .wrapping_add(pos.z.wrapping_mul(13))) as f32
        + time as f32
        + partial;
    let sway = (-0.0125 + 0.01 * math::cos(phase * PI * 0.02)) * PI;
    let cloth =
        base * Mat4::from_translation(Vec3::new(0., -32., 0.)) * Mat4::from_rotation_x(sway);
    model_box(
        mesh,
        cloth,
        [-10., 0., -2.],
        [20., 40., 1.],
        [0., 0.],
        region,
        light,
    );
    if standing {
        model_box(
            mesh,
            base,
            [-1., -30., -1.],
            [2., 42., 2.],
            [44., 0.],
            region,
            light,
        );
    }
    model_box(
        mesh,
        base,
        [-10., -32., -1.],
        [20., 2., 2.],
        [0., 42.],
        region,
        light,
    );
}
fn model_box(
    mesh: &mut Mesh,
    transform: Mat4,
    min: [f32; 3],
    size: [f32; 3],
    uv: [f32; 2],
    region: Region,
    light: [u8; 4],
) {
    crate::model_box::draw(mesh, transform, min, size, uv, crate::model_box::Texture {
        layer: ATLAS | region.layer,
        origin: region.origin,
        scale: [1. / 256.; 2],
    }, light);
}
#[test]
fn original_banner_boxes_vertices_and_uvs() {
    let region = Region {
        layer: 3,
        origin: [0.5, 0.25],
    };
    let mut mesh = crate::models::empty();
    for (part, (min, size, uv)) in [
        ([-10., 0., -2.], [20., 40., 1.], [0., 0.]),
        ([-1., -30., -1.], [2., 42., 2.], [44., 0.]),
        ([-10., -32., -1.], [20., 2., 2.], [0., 42.]),
    ]
    .into_iter()
    .enumerate()
    {
        mesh.vertices.clear();
        model_box(
            &mut mesh,
            Mat4::IDENTITY,
            min,
            size,
            uv,
            region,
            [0, 240, 0, 0],
        );
        for (v, &[x, y, z, u, t]) in mesh
            .vertices
            .iter()
            .zip(&super::tests::ORIGINAL.boxes[part])
        {
            assert_eq!(v.position, [x, y, z]);
            assert_eq!(v.uv, [0.5 + u / 4., 0.25 + t / 4.]);
        }
    }
    mesh.vertices.clear();
    draw(&mut mesh, IVec3::ZERO, 176 << 4, 12, 0., region, [0; 4]);
    assert_eq!(mesh.vertices.len(), 72);
    let at = mesh.vertices[3].position;
    mesh.vertices.clear();
    draw(&mut mesh, IVec3::ZERO, 176 << 4, 38, 0.5, region, [0; 4]);
    assert_ne!(mesh.vertices[3].position, at);
    mesh.vertices.clear();
    draw(
        &mut mesh,
        IVec3::ZERO,
        177 << 4 | 5,
        38,
        0.5,
        region,
        [0; 4],
    );
    assert_eq!(mesh.vertices.len(), 48);
}
