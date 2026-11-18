//! 1.8.9 `TileEntityEndPortalRenderer` top starfield surface for block 119 (`end_portal`).
use crate::{
    mesh::{Frustum, Mesh, Vertex},
    models::{Dynamic, ModelContext, END_PORTAL},
    resources::Sprite,
    world::World,
};
use glam::{IVec3, Vec3};

pub(super) fn render(d: &mut Dynamic, c: &ModelContext<'_>) {
    let end_sky = c.resources.sprite("textures/environment/end_sky.png");
    let end_portal = c.resources.sprite("textures/entity/end_portal.png");
    render_with_sprites(&mut d.opaque, c.world, c.eye, c.frustum, end_sky, end_portal);
}

fn render_with_sprites(
    mesh: &mut Mesh,
    world: &World,
    eye: Vec3,
    frustum: &Frustum,
    end_sky: Sprite,
    end_portal: Sprite,
) {
    let layer = end_sky.layer | END_PORTAL;
    let uv = [
        end_sky.width as f32 / 256.0,
        end_sky.height as f32 / 256.0,
    ];
    let color = [
        (end_portal.layer & 0xff) as u8,
        ((end_portal.layer >> 8) & 0xff) as u8,
        (end_portal.width.min(256) * 255 / 256) as u8,
        (end_portal.height.min(256) * 255 / 256) as u8,
    ];
    let light = [240, 240, 0, 0];

    let cam_cx = (eye.x.floor() as i32).div_euclid(16);
    let cam_cz = (eye.z.floor() as i32).div_euclid(16);
    for dz in -5..=5 {
        for dx in -5..=5 {
            let (cx, cz) = (cam_cx + dx, cam_cz + dz);
            let Some(chunk) = world.chunks.get(&(cx, cz)) else {
                continue;
            };
            let cdx = (cx * 16 + 8) as f32 - eye.x;
            let cdz = (cz * 16 + 8) as f32 - eye.z;
            if cdx * cdx + cdz * cdz > 76.0 * 76.0 {
                continue;
            }
            for &packed in chunk.end_portal_positions() {
                let lx = i32::from(packed & 15);
                let lz = i32::from((packed >> 4) & 15);
                let y = i32::from(packed >> 8);
                if chunk.get(lx, y, lz) >> 4 != 119 {
                    continue;
                }
                let at = IVec3::new(cx * 16 + lx, y, cz * 16 + lz).as_vec3();
                if (at + Vec3::splat(0.5)).distance_squared(eye) >= 4096.0
                    || !frustum.visible(at, at + Vec3::new(1.0, 0.75, 1.0))
                {
                    continue;
                }
                let top_y = at.y + 0.75;
                let corners = [
                    [at.x, top_y, at.z],
                    [at.x, top_y, at.z + 1.0],
                    [at.x + 1.0, top_y, at.z + 1.0],
                    [at.x + 1.0, top_y, at.z],
                ];
                let start = mesh.vertices.len() as u32;
                for position in corners {
                    mesh.vertices.push(Vertex {
                        position,
                        uv,
                        layer,
                        color,
                        light,
                    });
                }
                mesh.indices
                    .extend([0, 1, 2, 0, 2, 3].map(|idx| start + idx));
                mesh.visible_faces += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models::empty,
        vanilla::random::JavaRandom,
        world::{Chunk, ChunkMap},
    };
    use glam::Mat4;
    use std::sync::Arc;

    const EXPECTED_COLORS: [[f32; 3]; 16] = [
        [0.10000000, 0.10000000, 0.10000000],
        [0.00631738, 0.05095978, 0.04753906],
        [0.01473936, 0.05423397, 0.05350704],
        [0.02494494, 0.05886573, 0.06152015],
        [0.03494681, 0.06337459, 0.05233256],
        [0.03453727, 0.04706804, 0.06697500],
        [0.04626409, 0.06108771, 0.09075278],
        [0.05361915, 0.08476582, 0.05008522],
        [0.05897343, 0.07285756, 0.10843927],
        [0.05496822, 0.06198072, 0.10531616],
        [0.07629505, 0.07901610, 0.08490379],
        [0.04083666, 0.14194381, 0.13754530],
        [0.11805945, 0.08573964, 0.12881733],
        [0.02955061, 0.19708616, 0.20123141],
        [0.13644970, 0.26000684, 0.20137758],
        [0.06071605, 0.23611547, 0.49611820],
    ];

    const EXPECTED_ROT: [[f32; 2]; 16] = [
        [1.00000000, 0.00000000],
        [0.93969262, 0.34202014],
        [0.71933980, 0.69465837],
        [0.30901699, 0.95105652],
        [-0.24192190, 0.97029573],
        [-0.76604444, 0.64278761],
        [-1.00000000, 0.00000000],
        [-0.71933980, -0.69465837],
        [0.03489950, -0.99939083],
        [0.80901699, -0.58778525],
        [0.93969262, 0.34202014],
        [0.17364818, 0.98480775],
        [-0.80901699, 0.58778525],
        [-0.84804810, -0.52991926],
        [0.24192190, -0.97029573],
        [1.00000000, 0.00000000],
    ];

    #[test]
    fn java_random_31100_starfield_tables_match_shader_constants() {
        let mut rng = JavaRandom::new(31100);
        let wgsl = include_str!("terrain.wgsl");
        let colors_block = wgsl
            .split("const END_PORTAL_COLORS")
            .nth(1)
            .and_then(|s| s.split(");").next())
            .expect("END_PORTAL_COLORS table present in terrain.wgsl");
        let shader_colors: Vec<[f32; 3]> = colors_block
            .lines()
            .filter_map(|line| {
                let inner = line.trim().strip_prefix("vec3<f32>(")?;
                let inner = inner.trim_end_matches(',').strip_suffix(')')?;
                let mut nums = inner
                    .split(',')
                    .map(|part| part.trim().parse::<f32>().expect("valid f32"));
                Some([nums.next()?, nums.next()?, nums.next()?])
            })
            .collect();
        assert_eq!(shader_colors.len(), 16);

        let rot_block = wgsl
            .split("const END_PORTAL_ROT")
            .nth(1)
            .and_then(|s| s.split(");").next())
            .expect("END_PORTAL_ROT table present in terrain.wgsl");
        let shader_rot: Vec<[f32; 2]> = rot_block
            .lines()
            .filter_map(|line| {
                let inner = line.trim().strip_prefix("vec2<f32>(")?;
                let inner = inner.trim_end_matches(',').strip_suffix(')')?;
                let mut nums = inner
                    .split(',')
                    .map(|part| part.trim().parse::<f32>().expect("valid f32"));
                Some([nums.next()?, nums.next()?])
            })
            .collect();
        assert_eq!(shader_rot.len(), 16);

        for i in 0..16usize {
            let r = rng.next_float();
            let g = rng.next_float();
            let b = rng.next_float();
            let rgb = if i == 0 {
                [0.1, 0.1, 0.1]
            } else {
                let denom = (17 - i) as f32;
                [
                    (r * 0.5 + 0.1) / denom,
                    (g * 0.5 + 0.4) / denom,
                    (b * 0.5 + 0.5) / denom,
                ]
            };
            for ch in 0..3 {
                assert!(
                    (rgb[ch] - EXPECTED_COLORS[i][ch]).abs() < 1e-6,
                    "pass {i} channel {ch}: got {}, expected {}",
                    rgb[ch],
                    EXPECTED_COLORS[i][ch]
                );
                assert!(
                    (rgb[ch] - shader_colors[i][ch]).abs() < 1e-6,
                    "terrain.wgsl pass {i} channel {ch}: got {}, expected {}",
                    shader_colors[i][ch],
                    rgb[ch]
                );
            }
            let deg_mod = (((i * i * 4321 + i * 9) * 2) % 360) as f32;
            let (sin, cos) = deg_mod.to_radians().sin_cos();
            assert!(
                (cos - EXPECTED_ROT[i][0]).abs() < 1e-6
                    && (sin - EXPECTED_ROT[i][1]).abs() < 1e-6,
                "pass {i} rot mismatch: got ({cos}, {sin}), expected {:?}",
                EXPECTED_ROT[i]
            );
            assert!(
                (cos - shader_rot[i][0]).abs() < 1e-6
                    && (sin - shader_rot[i][1]).abs() < 1e-6,
                "terrain.wgsl pass {i} rot mismatch: got {:?}, expected ({cos}, {sin})",
                shader_rot[i]
            );
        }
    }

    #[test]
    fn end_portal_block_119_emits_top_starfield_quad_and_culls_distant_blocks() {
        let mut near_chunk = Chunk::empty();
        near_chunk.set(3, 64, 5, 119 << 4);
        near_chunk.set(4, 64, 5, 1 << 4);
        let mut far_chunk = Chunk::empty();
        far_chunk.set(2, 64, 2, 119 << 4);

        let mut chunks = ChunkMap::default();
        chunks.insert((0, 0), Arc::new(near_chunk));
        chunks.insert((10, 10), Arc::new(far_chunk));
        let world = World {
            chunks,
            spawn: Vec3::new(3.5, 66.0, 5.5),
            label: "end_portal_test".into(),
            edits: Default::default(),
            save_path: Default::default(),
            imported: false,
            light_revision: 0,
        };

        let eye = Vec3::new(3.5, 67.0, 5.5);
        let proj_view = Mat4::perspective_rh(70.0_f32.to_radians(), 16.0 / 9.0, 0.05, 256.0)
            * Mat4::look_to_rh(eye, -Vec3::Y, -Vec3::Z);
        let frustum = Frustum::new(proj_view);
        let sky_sprite = Sprite {
            layer: 42,
            width: 128,
            height: 128,
        };
        let portal_sprite = Sprite {
            layer: 300,
            width: 256,
            height: 256,
        };

        let mut mesh = empty();
        render_with_sprites(&mut mesh, &world, eye, &frustum, sky_sprite, portal_sprite);

        assert_eq!(mesh.visible_faces, 1);
        assert_eq!(mesh.vertices.len(), 4);
        assert_eq!(mesh.indices, vec![0, 1, 2, 0, 2, 3]);

        let p0 = Vec3::from_array(mesh.vertices[0].position);
        let p1 = Vec3::from_array(mesh.vertices[1].position);
        let p2 = Vec3::from_array(mesh.vertices[2].position);
        let normal = (p1 - p0).cross(p2 - p0);
        assert!(normal.y > 0.99, "expected +Y CCW normal, got {normal:?}");

        for v in &mesh.vertices {
            assert!((v.position[1] - 64.75).abs() < 1e-6);
            assert_eq!(v.layer, 42 | END_PORTAL);
            assert_eq!(v.uv, [0.5, 0.5]);
            assert_eq!(v.color, [(300 & 0xff) as u8, (300 >> 8) as u8, 255, 255]);
            assert_eq!(v.light, [240, 240, 0, 0]);
        }
    }
}
