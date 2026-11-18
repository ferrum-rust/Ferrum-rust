//! TexturedQuad vertex order and pixel-space UV rectangles from the 1.8.9 ModelBox.
use crate::mesh::{Mesh, Vertex};
use glam::{Mat4, Vec3};

#[derive(Clone, Copy)]
pub(crate) struct Texture {
    pub layer: u32,
    pub origin: [f32; 2],
    pub scale: [f32; 2],
}

pub(crate) fn draw(
    mesh: &mut Mesh,
    transform: Mat4,
    min: [f32; 3],
    size: [f32; 3],
    uv: [f32; 2],
    texture: Texture,
    light: [u8; 4],
) {
    let p = Vec3::from_array(min);
    let [w, h, d] = size;
    let [u, v] = uv;
    let pts = [
        [0., 0., 0.],
        [w, 0., 0.],
        [w, h, 0.],
        [0., h, 0.],
        [0., 0., d],
        [w, 0., d],
        [w, h, d],
        [0., h, d],
    ]
    .map(|a| transform.transform_point3(p + Vec3::from_array(a)));
    // ModelBox's top face uses a reversed V interval.
    let faces = [
        ([5, 1, 2, 6], [u + d + w, v + d, u + d + w + d, v + d + h]),
        ([0, 4, 7, 3], [u, v + d, u + d, v + d + h]),
        ([5, 4, 0, 1], [u + d, v, u + d + w, v + d]),
        ([2, 3, 7, 6], [u + d + w, v + d, u + d + w + w, v]),
        ([1, 0, 3, 2], [u + d, v + d, u + d + w, v + d + h]),
        (
            [4, 5, 6, 7],
            [u + d + w + d, v + d, u + d + w + d + w, v + d + h],
        ),
    ];
    let normals = [
        Vec3::X,
        Vec3::NEG_X,
        Vec3::NEG_Y,
        Vec3::Y,
        Vec3::NEG_Z,
        Vec3::Z,
    ];
    for ((indices, [u1, v1, u2, v2]), normal) in faces.into_iter().zip(normals) {
        let normal = transform.transform_vector3(normal).normalize();
        let shade = (crate::models::standard_item_light(normal) * 255.).round() as u8;
        let first = mesh.vertices.len() as u32;
        for (i, [s, t]) in indices
            .into_iter()
            .zip([[u2, v1], [u1, v1], [u1, v2], [u2, v2]])
        {
            mesh.vertices.push(Vertex {
                position: pts[i].to_array(),
                uv: [
                    texture.origin[0] + s * texture.scale[0],
                    texture.origin[1] + t * texture.scale[1],
                ],
                layer: texture.layer,
                color: [shade, shade, shade, 255],
                light,
            });
        }
        mesh.indices.extend([0, 1, 2, 0, 2, 3].map(|k| first + k));
        mesh.visible_faces += 1;
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_inflated(
    mesh: &mut Mesh,
    transform: Mat4,
    min: [f32; 3],
    size: [f32; 3],
    inflate: f32,
    uv: [f32; 2],
    texture: Texture,
    light: [u8; 4],
    double_sided: bool,
) {
    draw_inflated_tinted(
        mesh,
        transform,
        min,
        size,
        inflate,
        uv,
        texture,
        light,
        double_sided,
        0,
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_inflated_tinted(
    mesh: &mut Mesh,
    transform: Mat4,
    min: [f32; 3],
    size: [f32; 3],
    inflate: f32,
    uv: [f32; 2],
    texture: Texture,
    light: [u8; 4],
    double_sided: bool,
    extra_flags: u32,
) {
    let [w, h, d] = size;
    let [u, v] = uv;
    let [x0, y0, z0] = [min[0] - inflate, min[1] - inflate, min[2] - inflate];
    let [x1, y1, z1] = [
        min[0] + w + inflate,
        min[1] + h + inflate,
        min[2] + d + inflate,
    ];
    let pts = [
        [x0, y0, z0],
        [x1, y0, z0],
        [x1, y1, z0],
        [x0, y1, z0],
        [x0, y0, z1],
        [x1, y0, z1],
        [x1, y1, z1],
        [x0, y1, z1],
    ]
    .map(|a| transform.transform_point3(Vec3::from_array(a)));
    let faces = [
        ([5, 1, 2, 6], [u + d + w, v + d, u + d + w + d, v + d + h]),
        ([0, 4, 7, 3], [u, v + d, u + d, v + d + h]),
        ([5, 4, 0, 1], [u + d, v, u + d + w, v + d]),
        ([2, 3, 7, 6], [u + d + w, v + d, u + d + w + w, v]),
        ([1, 0, 3, 2], [u + d, v + d, u + d + w, v + d + h]),
        (
            [4, 5, 6, 7],
            [u + d + w + d, v + d, u + d + w + d + w, v + d + h],
        ),
    ];
    let normals = [
        Vec3::X,
        Vec3::NEG_X,
        Vec3::NEG_Y,
        Vec3::Y,
        Vec3::NEG_Z,
        Vec3::Z,
    ];
    let layer = texture.layer | extra_flags;
    for ((indices, [u1, v1, u2, v2]), local_normal) in faces.into_iter().zip(normals) {
        let normal = transform.transform_vector3(local_normal).normalize_or_zero();
        let corners: [([f32; 3], [f32; 2]); 4] = std::array::from_fn(|k| {
            let [s, t] = [[u2, v1], [u1, v1], [u1, v2], [u2, v2]][k];
            (
                pts[indices[k]].to_array(),
                [
                    texture.origin[0] + s * texture.scale[0],
                    texture.origin[1] + t * texture.scale[1],
                ],
            )
        });
        let shade = (crate::models::standard_item_light(normal) * 255.)
            .round()
            .clamp(0., 255.) as u8;
        let first = mesh.vertices.len() as u32;
        for (position, uv) in corners {
            mesh.vertices.push(Vertex {
                position,
                uv,
                layer,
                color: [shade, shade, shade, 255],
                light,
            });
        }
        mesh.indices.extend([0, 1, 2, 0, 2, 3].map(|k| first + k));
        mesh.visible_faces += 1;
        if double_sided {
            let back_shade = (crate::models::standard_item_light(-normal) * 255.)
                .round()
                .clamp(0., 255.) as u8;
            let back = mesh.vertices.len() as u32;
            for (position, uv) in corners {
                mesh.vertices.push(Vertex {
                    position,
                    uv,
                    layer,
                    color: [back_shade, back_shade, back_shade, 255],
                    light,
                });
            }
            mesh.indices.extend([0, 2, 1, 0, 3, 2].map(|k| back + k));
            mesh.visible_faces += 1;
        }
    }
}
