use crate::{
    assets::{self, Assets},
    world::{ChunkPos, World},
};
use bytemuck::{Pod, Zeroable};
use glam::{IVec3, Mat4, Vec3, Vec4};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct Vertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub layer: u32,
    pub color: [u8; 4],
    pub light: [u8; 4],
}
#[path = "section_visibility.rs"]
pub(crate) mod visibility;
#[cfg(test)]
#[path = "foliage_render_tests.rs"]
mod foliage_tests;
#[cfg(test)]
#[path = "leaf_render_tests.rs"]
mod leaf_tests;
pub struct Mesh {
    pub(crate) sections: Option<visibility::TerrainSections>,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub min: Vec3,
    pub max: Vec3,
    pub visible_faces: usize,
}
impl Mesh {
    pub fn quads(&self) -> usize {
        self.indices.len() / 6
    }
}
#[derive(Clone, Copy, PartialEq)]
struct Face {
    layer: u32,
    color: [u8; 4],
    light: [[u8; 4]; 4],
    ao: [f32; 4],
    uv: CubeUv,
    overlay: Option<(u32, [u8; 4])>,
}
#[derive(Clone, Copy, PartialEq)]
enum CubeUv {
    Model(assets::block_models::CubeFace),
    Tile(u8),
    Grass(u8),
}
impl CubeUv {
    fn grass(p: IVec3, select_variant: bool) -> Self {
        if !select_variant {
            return Self::Grass(0);
        }
        // MathHelper.getCoordinateRandom promotes X only after its 32-bit product.
        let key = i64::from(p.x.wrapping_mul(3_129_871)) ^ (i64::from(p.z) * 116_129_781) ^ i64::from(p.y);
        let hash = key.wrapping_mul(key).wrapping_mul(42_317_861).wrapping_add(key.wrapping_mul(11));
        Self::Grass(((hash as i32 >> 16).unsigned_abs() % 4) as u8)
    }

    fn corners(self, axis: usize, positive: bool, w: f32, h: f32) -> [[f32; 2]; 4] {
        // Positions are ordered (0,0), (w,0), (w,h), (0,h) along the two face axes.
        let (mut uv, turns) = match self {
            Self::Model(face) => return [[0.,0.],[w,0.],[w,h],[0.,h]].map(|[x,y]| {
                std::array::from_fn(|i| f32::from(face.origin[i]) + x*f32::from(face.du[i]) + y*f32::from(face.dv[i]))
            }),
            Self::Grass(turns) => match (axis, positive) {
                (1, true) => ([[0., 0.], [0., w], [h, w], [h, 0.]], turns),
                (1, false) => ([[0., w], [0., 0.], [h, 0.], [h, w]], (4 - turns) % 4),
                (0, false) => ([[0., w], [0., 0.], [h, 0.], [h, w]], 0),
                (0, true) => ([[h, w], [h, 0.], [0., 0.], [0., w]], 0),
                (2, false) => ([[w, h], [0., h], [0., 0.], [w, 0.]], 0),
                _ => ([[0., h], [w, h], [w, 0.], [0., 0.]], 0),
            },
            Self::Tile(turns) if axis == 0 => ([[0., w], [0., 0.], [h, 0.], [h, w]], turns),
            Self::Tile(turns) => ([[0., h], [w, h], [w, 0.], [0., 0.]], turns),
        };
        for _ in 0..turns {
            for p in &mut uv {
                *p = [p[1], -p[0]];
            }
        }
        uv
    }
}
fn blend_layer(state: u16) -> u32 {
    if matches!(state >> 4, 8 | 9 | 79 | 90 | 95 | 132 | 160 | 165) {
        crate::models::BLEND
    } else {
        0
    }
}

fn texture_flags(state: u16) -> u32 {
    // EntityRenderer turns mipmaps off only for EnumWorldBlockLayer.CUTOUT.
    // Grass blocks, leaves and panes use CUTOUT_MIPPED and retain distance mips.
    blend_layer(state)
        | if matches!(state >> 4,
            6 | 20 | 26..=28 | 30..=32 | 37..=40 | 50..=52 | 55 | 59 | 64..=66
            | 71 | 75 | 76 | 81 | 83 | 92..=94 | 96 | 104..=106 | 111 | 115 | 117
            | 127 | 138 | 140..=142 | 149 | 150 | 157 | 167 | 175 | 193..=197)
        { crate::models::NO_MIPMAP } else { 0 }
}

fn graphics_opaque(state: u16, fancy: bool) -> bool {
    if crate::vanilla::material::is_leaves(state) { !fancy }
    else { crate::vanilla::material::opaque_cube(state) }
}

fn graphics_flags(state: u16, fancy: bool) -> u32 {
    texture_flags(state) | if !fancy && crate::vanilla::material::is_leaves(state) {
        crate::models::OPAQUE_LEAF
    } else { 0 }
}

fn shade_rgb(rgb: u32, axis: usize, positive: bool) -> [u8; 4] {
    let base = [(rgb >> 16) & 255, (rgb >> 8) & 255, rgb & 255];
    let shade = match (axis, positive) {
        (1, true) => 1.0,
        (1, false) => 0.5,
        (0, _) => 0.6,
        _ => 0.8,
    };
    [
        (base[0] as f32 * shade) as u8,
        (base[1] as f32 * shade) as u8,
        (base[2] as f32 * shade) as u8,
        255,
    ]
}
fn tint(
    world: &World,
    assets: &Assets,
    p: IVec3,
    state: u16,
    axis: usize,
    positive: bool,
) -> [u8; 4] {
    let rgb = if state >> 4 == 2
        && (axis != 1 || !positive || matches!(world.get(p + IVec3::Y) >> 4, 78 | 80))
    {
        0xffffff
    } else {
        assets.biome_colors.block(world, p, state)
    };
    shade_rgb(rgb, axis, positive)
}
fn face_lighting(
    world: &World,
    p: IVec3,
    axis: usize,
    positive: bool,
    bounds: [Vec3; 2],
    settings: &crate::settings::Settings,
) -> ([f32; 4], [[u8; 4]; 4]) {
    use crate::vanilla::{ambient_occlusion as ao, material};
    let face = [[4, 5], [0, 1], [2, 3]][axis][usize::from(positive)];
    let (min, max) = (bounds[0], bounds[1]);
    let at = if positive { max[axis] } else { min[axis] };
    let offset =
        (if positive { at > 0.9999 } else { at < 0.0001 }) || material::full_cube(world.get(p));
    let u = (axis + 1) % 3;
    let v = (axis + 2) % 3;
    let partial = min[u] >= 0.0001 || min[v] >= 0.0001 || max[u] <= 0.9999 || max[v] <= 0.9999;
    if settings.smooth_lighting == 0 || material::light_value(world.get(p)) != 0 {
        let step = p + ao::DIRECTIONS[face];
        let mixed = if !offset && crate::visual_light::aperture_open(world, p, face, step) {
            crate::visual_light::max_mixed(
                crate::visual_light::visual_mixed(world, p),
                crate::visual_light::visual_mixed(world, step),
            )
        } else {
            let query = if offset { step } else { p };
            crate::visual_light::visual_mixed(world, query)
        };
        return ([1.0; 4], [ao::uv(mixed); 4]);
    }
    let mut lo = min;
    let mut hi = max;
    lo[axis] = at;
    hi[axis] = at;
    let result = crate::visual_light::sample_ao(
        world,
        p,
        face,
        [lo.y, hi.y, lo.z, hi.z, lo.x, hi.x],
        offset,
        partial,
        settings.fancy,
    );
    let original = ao::vertices(face);
    let direction = [[4, 5], [0, 1], [2, 3]];
    let map = [(0, 0), (1, 0), (1, 1), (0, 1)].map(|(a, b)| {
        original
            .iter()
            .position(|vertex| vertex[u] == direction[u][a] && vertex[v] == direction[v][b])
            .unwrap()
    });
    (
        map.map(|i| result.shade[i]),
        map.map(|i| ao::uv(result.brightness[i])),
    )
}
fn quad(
    mesh: &mut Mesh,
    positions: [Vec3; 4],
    width: f32,
    height: f32,
    face: Face,
    positive: bool,
    axis: usize,
) {
    let start = mesh.vertices.len() as u32;
    // Each merged face repeats the tile in world units via an array texture.
    let uv = face.uv.corners(axis, positive, width, height);
    for i in 0..4 {
        let mut color = face.color;
        for component in &mut color[..3] {
            *component = (*component as f32 * face.ao[i]) as u8;
        }
        mesh.vertices.push(Vertex {
            position: positions[i].to_array(),
            uv: uv[i],
            layer: face.layer,
            color,
            light: face.light[i],
        });
    }
    let order = if positive {
        [0, 1, 2, 0, 2, 3]
    } else {
        [0, 3, 2, 0, 2, 1]
    };
    mesh.indices.extend(order.map(|i| start + i));
    if let Some((layer, color)) = face.overlay {
        quad(
            mesh,
            positions,
            width,
            height,
            Face {
                layer,
                color,
                overlay: None,
                ..face
            },
            positive,
            axis,
        );
    }
}
#[cfg(test)]
pub fn build(world: &World, pos: ChunkPos, assets: &Assets) -> Mesh {
    build_with_settings(world, pos, assets, &crate::settings::Settings::default())
}
pub fn build_with_settings(
    world: &World,
    pos: ChunkPos,
    assets: &Assets,
    settings: &crate::settings::Settings,
) -> Mesh {
    let chunk = &world.chunks[&pos];
    let origin = IVec3::new(pos.0 * 16, 0, pos.1 * 16);
    let dims = [16, chunk.top.max(1), 16];
    let mut sections = visibility::TerrainSections::new(chunk, settings.fancy);
    let mut mesh = Mesh {
        sections: None,
        vertices: Vec::new(),
        indices: Vec::new(),
        min: origin.as_vec3(),
        max: (origin + IVec3::new(16, dims[1], 16)).as_vec3(),
        visible_faces: 0,
    };
    let mut mask = vec![None; (16 * dims[1].max(16)) as usize];
    for axis in 0..3 {
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        let width = dims[u] as usize;
        let height = dims[v] as usize;
        for positive in [false, true] {
            let mut step = IVec3::ZERO;
            step[axis] = if positive { 1 } else { -1 };
            for slice in 0..dims[axis] {
                for j in 0..height {
                    for i in 0..width {
                        let mut p = IVec3::ZERO;
                        p[axis] = slice;
                        p[u] = i as i32;
                        p[v] = j as i32;
                        let state = chunk.get(p.x, p.y, p.z);
                        if state == 0
                            || matches!(state >> 4, 63 | 68 | 90 | 119 | 132 | 144 | 166 | 176 | 177)
                            || assets::crossed(state)
                            || assets::partial_model(state) || assets.block_models.custom_shape(state)
                            || crate::block_ticks::is_liquid(state)
                        {
                            mask[j * width + i] = None;
                            continue;
                        }
                        let np = p + step;
                        let neighbor = if (0..16).contains(&np.x)
                            && (0..crate::world::HEIGHT).contains(&np.y)
                            && (0..16).contains(&np.z)
                        {
                            chunk.get(np.x, np.y, np.z)
                        } else {
                            world.get(origin + np)
                        };
                        let leaf = |state: u16| matches!(state >> 4, 18 | 161);
                        let transparent = (assets::see_through(neighbor)
                            || neighbor >> 4 == 52
                            || assets::partial_model(neighbor) || assets.block_models.custom_shape(neighbor))
                            && (settings.fancy || !leaf(neighbor));
                        // BlockBreakable uses state identity for the two
                        // glass blocks, even when the neighbor is opaque.
                        let mut visible = if matches!(state >> 4, 20 | 95) {
                            state != neighbor
                        } else if leaf(state) {
                            // Fancy leaves use Block.shouldSideBeRendered even
                            // against the same leaf state. In fast mode both leaf
                            // block IDs become opaque, independent of metadata.
                            if leaf(neighbor) {
                                settings.fancy
                            } else {
                                !graphics_opaque(neighbor, settings.fancy)
                            }
                        } else {
                            transparent && (neighbor != state || !assets::see_through(state))
                        };
                        if visible
                            && matches!(
                                neighbor >> 4,
                                44 | 126
                                    | 182
                                    | 53
                                    | 67
                                    | 108
                                    | 109
                                    | 114
                                    | 128
                                    | 134..=136
                                    | 156
                                    | 163
                                    | 164
                                    | 180
                            )
                            && crate::visual_light::face_mask(
                                world,
                                origin + np,
                                ([4, 0, 2][axis] + usize::from(positive)) ^ 1,
                            ) == 0b1111
                        {
                            visible = false;
                        }
                        mask[j * width + i] = if visible {
                            mesh.visible_faces += 1;
                            let world_p = origin + p;
                            let (ao, light) = face_lighting(
                                world,
                                world_p,
                                axis,
                                positive,
                                [Vec3::ZERO, Vec3::ONE],
                                settings,
                            );
                            let snowy_grass = state >> 4 == 2
                                && matches!(world.get(world_p + IVec3::Y) >> 4, 78 | 80);
                            let cube=assets.block_models.cube_at(state,[4,0,2][axis]+usize::from(positive),world_p,settings.alternate_blocks);
                            let uv = if state >> 4 == 2 {
                                CubeUv::grass(world_p, axis == 1 && !snowy_grass && settings.alternate_blocks)
                            } else if let Some(face)=cube {
                                CubeUv::Model(face)
                            } else { CubeUv::Tile(0) };
                            let overlay = if state >> 4 == 2 && axis != 1 && !snowy_grass {
                                assets.materials.get("grass_side_overlay").map(|&layer| {
                                    (
                                        layer,
                                        shade_rgb(
                                            assets.biome_colors.block(world, world_p, state),
                                            axis,
                                            positive,
                                        ),
                                    )
                                })
                            } else {
                                None
                            };
                            mesh.visible_faces += usize::from(overlay.is_some());
                            Some(Face {
                                layer: if snowy_grass && axis != 1 {
                                    assets
                                        .materials
                                        .get("grass_side_snowed")
                                        .copied()
                                        .unwrap_or_else(|| assets.texture(state, axis, positive))
                                } else {
                                    cube.map_or_else(||assets.texture(state,axis,positive),|face|face.layer)
                                } | graphics_flags(state, settings.fancy),
                                color: tint(world, assets, world_p, state, axis, positive),
                                light,
                                ao,
                                uv,
                                overlay,
                            })
                        } else {
                            None
                        };
                    }
                }
                let mut j = 0;
                while j < height {
                    let mut i = 0;
                    while i < width {
                        let Some(face) = mask[j * width + i] else {
                            i += 1;
                            continue;
                        };
                        let mut w = 1;
                        let merge = settings.greedy_meshing
                            && face.ao.iter().all(|a| *a == face.ao[0])
                            && face.light.iter().all(|light| *light == face.light[0]);
                        while merge && i + w < width && (u != 1 || i / 16 == (i+w)/16)
                            && mask[j * width + i + w] == Some(face) {
                            w += 1;
                        }
                        let mut h = 1;
                        'grow: while merge && j + h < height && (v != 1 || j / 16 == (j+h)/16) {
                            for k in 0..w {
                                if mask[(j + h) * width + i + k] != Some(face) {
                                    break 'grow;
                                }
                            }
                            h += 1;
                        }
                        let mut p = origin.as_vec3();
                        p[axis] += slice as f32 + if positive { 1.0 } else { 0.0 };
                        p[u] += i as f32;
                        p[v] += j as f32;
                        let mut du = Vec3::ZERO;
                        du[u] = w as f32;
                        let mut dv = Vec3::ZERO;
                        dv[v] = h as f32;
                        quad(
                            &mut mesh,
                            [p, p + du, p + du + dv, p + dv],
                            w as f32,
                            h as f32,
                            face,
                            positive,
                            axis,
                        );
                        let source_y = if axis == 1 { slice as usize } else if u == 1 { i } else { j };
                        sections.owners.resize(mesh.quads(), (source_y/16) as u8);
                        for y in 0..h {
                            for x in 0..w {
                                mask[(j + y) * width + i + x] = None;
                            }
                        }
                        i += w;
                    }
                    j += 1;
                }
            }
        }
    }
    for y in 0..chunk.top {
        for z in 0..16 {
            for x in 0..16 {
                let state = chunk.get(x, y, z);
                if crate::chest_render::is_chest(state) {
                    sections.chests.push(origin + IVec3::new(x, y, z));
                } else if state != 0 {
                    special_block_mesh(&mut mesh, world, origin + IVec3::new(x, y, z), state, assets, settings);
                }
            }
        }
        sections.owners.resize(mesh.quads(),(y/16) as u8);
    }
    mesh.sections=Some(sections);
    mesh
}

fn special_faces<'a>(
    world: &World,
    assets: &'a Assets,
    block_pos: IVec3,
    state: u16,
    alternate: bool,
) -> Option<&'a [assets::block_models::BakedFace]> {
    if state >> 4 == 51 {
        return assets.block_models.fire(world, block_pos);
    }
    if state >> 4 == 140 {
        return assets.block_models.flower_pot(state, world.tile(block_pos));
    }
    if state >> 4 == 132 {
        let mut mask = 0;
        for (i, (dir, hook_facing)) in [
            (IVec3::NEG_Z, 0u16),
            (IVec3::X, 1u16),
            (IVec3::Z, 2u16),
            (IVec3::NEG_X, 3u16),
        ]
        .into_iter()
        .enumerate()
        {
            let n = world.get(block_pos + dir);
            let connected = (n >> 4 == 132 && (n & 2) == (state & 2))
                || (n >> 4 == 131 && (n & 3) == hook_facing);
            if connected {
                mask |= 1 << i;
            }
        }
        return Some(assets.block_models.tripwire(state, mask));
    }
    if matches!(state >> 4, 107 | 183..=187) {
        let in_wall = if matches!(state & 3, 0 | 2) {
            world.get(block_pos + IVec3::NEG_X) >> 4 == 139
                || world.get(block_pos + IVec3::X) >> 4 == 139
        } else {
            world.get(block_pos + IVec3::NEG_Z) >> 4 == 139
                || world.get(block_pos + IVec3::Z) >> 4 == 139
        };
        return assets.block_models.fence_gate(state, in_wall);
    }
    if state >> 4 == 90 {
        let portal_state = if matches!(state & 3, 1 | 2) {
            (90 << 4) | (state & 3)
        } else {
            let z_axis = (world.get(block_pos + IVec3::NEG_Z) >> 4 == 90
                || world.get(block_pos + IVec3::Z) >> 4 == 90)
                && world.get(block_pos + IVec3::NEG_X) >> 4 != 90
                && world.get(block_pos + IVec3::X) >> 4 != 90;
            (90 << 4) | if z_axis { 2 } else { 1 }
        };
        return assets.block_models.get_at(portal_state, block_pos, alternate);
    }
    if crate::vanilla::hand_placement::door_item(state >> 4).is_some() {
        let other = if state & 8 == 0 { block_pos + IVec3::Y } else { block_pos - IVec3::Y };
        return assets.block_models.door(state, world.get(other));
    }
    let pane = matches!(state >> 4, 101 | 102 | 160);
    if pane {
        let mask = [IVec3::NEG_Z, IVec3::X, IVec3::Z, IVec3::NEG_X]
            .into_iter()
            .enumerate()
            .fold(0, |mask, (i, d)| {
                let connected =
                    crate::vanilla::collision_shapes::connected(state, world.get(block_pos + d));
                mask | (u8::from(connected) << i)
            });
        assets.block_models.pane(state, mask)
    } else if matches!(state >> 4, 104 | 105) {
        let fruit = if state >> 4 == 104 { 86 } else { 103 };
        if let Some(face) = [IVec3::NEG_Z, IVec3::X, IVec3::Z, IVec3::NEG_X]
            .into_iter().position(|d| world.get(block_pos + d) >> 4 == fruit) {
            assets.block_models.attached_stem(state, face)
        } else {
            assets.block_models.get(state)
        }
    } else if matches!(state >> 4, 93 | 94) {
        assets.block_models.repeater(
            state,
            crate::vanilla::notes::repeater_locked(world, block_pos),
        )
    } else if state >> 4 == 106 {
        assets.block_models.vine(
            state,
            crate::vanilla::material::normal_cube(world.get(block_pos + IVec3::Y)),
        )
    } else if state >> 4 == 55 {
        let side = |f| match crate::vanilla::notes::wire_attachment(world, block_pos, f) {
            "side" => 1,
            "up" => 2,
            _ => 0,
        };
        Some(assets.block_models.wire(side(5), side(2), side(3), side(4)))
    } else {
        assets
            .block_models
            .get_at(state, block_pos, alternate)
            .or_else(|| assets.block_models.carried(state))
    }
}

/// Damage is drawn on the same block faces as terrain. BreakingFour projects the
/// destroy sprite from the model coordinates, even when the face texture is rotated.
pub(crate) fn block_damage(
    mesh: &mut Mesh,
    world: &World,
    assets: &Assets,
    settings: &crate::settings::Settings,
    pos: IVec3,
    stage: u8,
    chest_angle: f32,
) {
    let state = world.get(pos);
    // Moving pistons and banners have separate renderers. In 1.8.9 banners
    // do not participate in RenderGlobal's tile-entity damage pass.
    if matches!(state >> 4, 0 | 8..=11 | 36 | 119 | 166 | 176 | 177) {
        return;
    }
    let Some(&layer) = assets.materials.get(&format!("destroy_stage_{stage}")) else {
        return;
    };
    if crate::chest_render::is_chest(state) {
        crate::chest_render::damage(mesh, world, pos, layer, chest_angle);
        return;
    }
    if state >> 4 == 144 {
        crate::skull::damage(mesh, world, pos, layer);
        return;
    }
    if matches!(state >> 4, 63 | 68) {
        crate::sign_render::damage(mesh, world, pos, layer);
        return;
    }
    let first_vertex = mesh.vertices.len();
    let first_index = mesh.indices.len();
    if assets::partial_model(state)
        || matches!(state >> 4, 90 | 132)
        || assets::crossed(state)
        || assets.block_models.custom_shape(state)
    {
        special_block_mesh(mesh, world, pos, state, assets, settings);
    } else if let Some(faces) = assets.block_models.carried(state) {
        baked_faces(mesh, world, assets, pos, state, faces, Vec3::ZERO, true, settings.fancy);
    } else {
        // Older/unknown block states displayed with the terrain fallback still
        // need visible breaking feedback, including custom pack missing models.
        partial_mesh(mesh, world, pos, state, assets, settings);
    }
    for (quad, triangles) in mesh.vertices[first_vertex..]
        .chunks_exact_mut(4)
        .zip(mesh.indices[first_index..].chunks_exact(6))
    {
        let point = |i: usize| Vec3::from_array(quad[(triangles[i] % 4) as usize].position);
        let normal = (point(1) - point(0)).cross(point(2) - point(0));
        let mut side = 1;
        let mut largest = 0.;
        for (i, dir) in crate::vanilla::ambient_occlusion::DIRECTIONS
            .iter()
            .enumerate()
        {
            let dot = normal.dot(dir.as_vec3());
            if dot > largest {
                largest = dot;
                side = i;
            }
        }
        for v in quad {
            let p = Vec3::from_array(v.position) - pos.as_vec3();
            v.uv = match side {
                0 => [p.x, 1. - p.z],
                1 => [p.x, p.z],
                2 => [1. - p.x, 1. - p.y],
                3 => [p.x, 1. - p.y],
                4 => [p.z, 1. - p.y],
                _ => [1. - p.z, 1. - p.y],
            };
            v.layer = layer;
        }
    }
}

fn special_block_mesh(
    mesh: &mut Mesh,
    world: &World,
    block_pos: IVec3,
    mut state: u16,
    assets: &Assets,
    settings: &crate::settings::Settings,
) {
    if matches!(state >> 4, 63 | 68 | 119 | 144 | 166 | 176 | 177) {
        return;
    }
    if crate::block_ticks::is_liquid(state) {
        fluid_mesh(mesh, world, block_pos, state, assets, settings.fancy);
        return;
    }
    if matches!(state >> 4, 27 | 28 | 66 | 157) {
        rail_mesh(mesh, world, block_pos, state, assets);
        return;
    }
    let pane = matches!(state >> 4, 101 | 102 | 160);
    if pane
        || matches!(state >> 4, 55 | 90 | 107 | 132 | 183..=187)
        || assets.block_models.custom_shape(state)
        || assets::block_models::BlockModels::static_model(state >> 4)
    {
        let faces = special_faces(world, assets, block_pos, state, settings.alternate_blocks);
        if let Some(quads) = faces {
            baked_faces(
                mesh,
                world,
                assets,
                block_pos,
                state,
                quads,
                Vec3::ZERO,
                true,
                settings.fancy,
            );
        }
        return;
    }
    if state >> 4 == 36 {
        return;
    } // Renderer interpolates moving piston tiles each frame.
    if assets::partial_model(state) {
        partial_mesh(mesh, world, block_pos, state, assets, settings);
        return;
    }
    if !assets::crossed(state) {
        return;
    }
    if state >> 4 == 175 && state & 8 != 0 {
        let below = world.get(block_pos - IVec3::Y);
        state = (175 << 4) | 8 | (below & 7);
    }
    let p = block_pos.as_vec3();
    if matches!(state >> 4, 59 | 141 | 142) {
        crop_mesh(mesh, world, p, state, assets);
        return;
    }
    let (lo, hi) = (0.8 / 16.0, 15.2 / 16.0);
    let a = [
        p + Vec3::new(lo, 0.0, lo),
        p + Vec3::new(hi, 0.0, hi),
        p + Vec3::new(hi, 1.0, hi),
        p + Vec3::new(lo, 1.0, lo),
    ];
    let b = [
        p + Vec3::new(hi, 0.0, lo),
        p + Vec3::new(lo, 0.0, hi),
        p + Vec3::new(lo, 1.0, hi),
        p + Vec3::new(hi, 1.0, lo),
    ];
    let face = Face {
        layer: assets.texture(state, 2, true) | texture_flags(state),
        color: tint(world, assets, block_pos, state, 1, true),
        light: [crate::vanilla::ambient_occlusion::uv(crate::visual_light::visual_mixed(
            world, block_pos,
        )); 4],
        ao: [1.0; 4],
        uv: CubeUv::Tile(0),
        overlay: None,
    };
    for positions in [a, b] {
        for positive in [false, true] {
            quad(mesh, positions, 1.0, 1.0, face, positive, 2);
        }
    }
    mesh.visible_faces += 4;
}

#[allow(clippy::too_many_arguments)]
fn baked_faces(mesh: &mut Mesh, world:&World, assets:&Assets, at:IVec3, state:u16, quads:&[assets::block_models::BakedFace], offset:Vec3, cull:bool, fancy:bool) {
    use crate::vanilla::ambient_occlusion::{DIRECTIONS,uv};
    for q in quads {
        if let Some(side)=q.cull_face.filter(|_|cull) {
            let n=world.get(at+DIRECTIONS[side]);
            if (matches!(state>>4,101|102|160|165) && n>>4 == state>>4) || graphics_opaque(n, fancy) {continue;}
        }
        let sample = if q.neighbor_light || q.cull_face.is_some() {
            let adj = at + DIRECTIONS[q.facing];
            if !crate::vanilla::material::full_cube(state)
                && crate::vanilla::material::blocks_light(world.get(adj))
            {
                at
            } else {
                adj
            }
        } else {
            at
        };
        let light=uv(crate::visual_light::visual_mixed(world,sample));
        let mut color=[q.shade,q.shade,q.shade,255];
        if q.tinted {
            let rgb = if state>>4==55 {
                let n=f32::from(state & 15)/15.;
                let r=if state&15==0 {0.3} else {n*0.6+0.4};
                ((r*255.) as u32)<<16 | (((n*n*0.7-0.5).max(0.)*255.) as u32)<<8
            } else { assets.biome_colors.block(world,at,state) };
            for (c,shift) in color[..3].iter_mut().zip([16,8,0]) { *c=((u32::from(*c)*((rgb>>shift)&255))/255) as u8; }
        }
        let start=mesh.vertices.len() as u32;
        for v in q.vertices {
            let position=at.as_vec3()+v.position+offset;
            mesh.min=mesh.min.min(position);
            mesh.max=mesh.max.max(position);
            mesh.vertices.push(Vertex{position:position.to_array(),uv:v.uv,layer:q.layer|graphics_flags(state, fancy),color,light});
        }
        mesh.indices.extend([0,1,2,0,2,3].map(|i|start+i));
        mesh.visible_faces+=1;
    }
}
pub(crate) fn primed_tnt(c:&crate::models::ModelContext<'_>, mesh:&mut Mesh) {
    let state=46<<4;
    let Some(faces)=c.assets.block_models.carried(state) else {return;};
    for t in &c.sim.primed_tnt {
        let center=t.previous.lerp(t.position,c.alpha.into()).as_vec3()+Vec3::Y*0.5;
        let remaining=t.fuse as f32-c.alpha+1.;
        let scale=1.+0.3*(1.-remaining/10.).clamp(0.,1.).powi(4);
        let half=Vec3::splat(0.5*scale);
        if !c.frustum.visible(center-half,center+half) {continue;}
        let light=c.world.visual_light_uv(center.floor().as_ivec3(),0);
        let flash=((1.-remaining/100.)*0.8).clamp(0.,1.);
        let flashing=t.fuse/5%2==0;
        for face in faces {
            let first=mesh.vertices.len() as u32;
            for v in face.vertices {
                mesh.vertices.push(Vertex{
                    position:(center+(v.position-0.5)*scale).to_array(),uv:v.uv,
                    layer:face.layer | if flashing {1<<25} else {0},
                    color:[face.shade,face.shade,face.shade,(flash*255.) as u8],light,
                });
            }
            mesh.indices.extend([0,1,2,0,2,3].map(|i|first+i));
            mesh.visible_faces+=1;
        }
    }
}
pub(crate) fn moving_pistons(c:&crate::models::ModelContext<'_>, opaque:&mut Mesh, translucent:&mut Mesh) {
    let cam_cx = (c.eye.x.floor() as i32).div_euclid(16);
    let cam_cz = (c.eye.z.floor() as i32).div_euclid(16);
    for dz in -5..=5 {
        for dx in -5..=5 {
            let (cx, cz) = (cam_cx + dx, cam_cz + dz);
            let Some(chunk) = c.world.chunks.get(&(cx, cz)) else { continue; };
            if chunk.pistons.is_empty() { continue; }
            for (&[x,y,z],t) in &chunk.pistons {
                let p=IVec3::new(cx*16+x,y,cz*16+z);
                if (p.as_vec3()+Vec3::splat(0.5)).distance_squared(c.eye) >= 4096. { continue; }
                if !c.frustum.visible(p.as_vec3()-Vec3::ONE,p.as_vec3()+Vec3::splat(2.)) { continue; }
                let direction=crate::vanilla::ambient_occlusion::DIRECTIONS[usize::from(t.facing.min(5))].as_vec3();
                let progress=c.world.piston_render_progress(p,c.alpha);
                if progress >= 1. {continue;}
                let offset=direction*(if t.extending {progress-1.} else {1.-progress});
                let mut render=|state,quads:Option<&[assets::block_models::BakedFace]>,offset,cull| {
                    if let Some(quads)=quads {
                        let mesh=if blend_layer(state)==0 {&mut *opaque} else {&mut *translucent};
                        baked_faces(mesh,c.world,c.assets,p,state,quads,offset,cull,c.settings.fancy);
                    }
                };
                if t.carried>>4==34 && progress<0.5 {
                    render(t.carried,c.assets.block_models.head(t.carried,true),offset,false);
                } else if !t.extending && c.world.piston_renders_head(p) {
                    let head=34<<4|u16::from(t.facing)|if t.carried>>4==29 {8} else {0};
                    render(head,c.assets.block_models.head(head,progress>=0.5),offset,false);
                    render(t.carried,c.assets.block_models.get(t.carried),Vec3::ZERO,true);
                } else {
                    render(t.carried,c.assets.block_models.carried(t.carried),offset,false);
                }
            }
        }
    }
}

fn rail_mesh(mesh: &mut Mesh, world: &World, p: IVec3, state: u16, assets: &Assets) {
    let shape = assets::rail_shape(state);
    let rotation = match shape {
        1..=3 => 1,
        7 => 1,
        8 => 2,
        9 => 3,
        _ => 0,
    };
    // rail_flat/curved are double-sided planes at y=1/16. The original
    // raised_ne/sw rotate +/-45 around (8,9,8) with rescale=true, giving
    // identical 0..1 footprints and endpoint heights 1/16 and 17/16.
    let uv = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];
    let positions = uv.map(|[x, z]| {
        let y = match shape {
            2 | 4 => 17.0 / 16.0 - z,
            3 | 5 => 1.0 / 16.0 + z,
            _ => 1.0 / 16.0,
        };
        let mut point = Vec3::new(x, y, z);
        for _ in 0..rotation {
            point = Vec3::new(1.0 - point.z, point.y, point.x);
        }
        p.as_vec3() + point
    });
    for point in positions {
        mesh.min = mesh.min.min(point);
        mesh.max = mesh.max.max(point);
    }
    for positive in [true, false] {
        let first = mesh.vertices.len();
        quad(
            mesh,
            positions,
            1.0,
            1.0,
            Face {
                layer: assets.texture(state, 1, positive) | texture_flags(state),
                color: shade_rgb(0xffffff, 1, positive),
                light: [crate::vanilla::ambient_occlusion::uv(
                    crate::visual_light::visual_mixed(world, p),
                ); 4],
                ao: [1.0; 4],
                uv: CubeUv::Tile(0),
                overlay: None,
            },
            positive,
            1,
        );
        // UVs rotate with the model (blockstates use y rotation, uvlock=false).
        // Matching world UVs on both sides reproduce rail_flat's flipped-down
        // UV rectangle without mirroring the track when seen from below.
        for (vertex, uv) in mesh.vertices[first..].iter_mut().zip(uv) {
            vertex.uv = uv;
        }
    }
    mesh.visible_faces += 2;
}

fn fluid_light(world: &World, p: IVec3) -> [u8; 4] {
    let a = world.visual_light_uv(p, 0);
    let b = world.visual_light_uv(p + IVec3::Y, 0);
    [a[0].max(b[0]), a[1].max(b[1]), 0, 0]
}
fn fluid_corner_height(world: &World, p: IVec3, state: u16) -> f32 {
    let mut samples = 0;
    let mut depth = 0.;
    for corner in 0..4 {
        let at = p - IVec3::new(corner & 1, 0, (corner >> 1) & 1);
        if crate::block_ticks::same_liquid(world.get(at + IVec3::Y), state) {
            return 1.;
        }
        let other = world.get(at);
        if crate::block_ticks::same_liquid(other, state) {
            let level = other & 15;
            let empty = (if level >= 8 { 1 } else { level + 1 }) as f32 / 9.;
            if level == 0 || level >= 8 {
                depth += empty * 10.;
                samples += 10;
            }
            depth += empty;
            samples += 1;
        } else if !crate::block_ticks::material_is_solid(other) {
            depth += 1.;
            samples += 1;
        }
    }
    if samples == 0 {
        1.
    } else {
        1. - depth / samples as f32
    }
}
const FLOW_TEXTURE: u32 = 1 << 26;

fn fluid_mesh(mesh: &mut Mesh, world: &World, p: IVec3, state: u16, assets: &Assets, fancy: bool) {
    use crate::vanilla::{material, math};
    let same = |at| crate::block_ticks::same_liquid(world.get(at), state);
    let visible = |at| {
        let other = world.get(at);
        !crate::block_ticks::same_liquid(other, state) && if material::is_leaves(other) {
            fancy
        } else {
            !material::opaque_cube(other)
        }
    };
    let water = matches!(state >> 4, 8 | 9);
    let still = assets
        .materials
        .get(if water { "water_still" } else { "lava_still" })
        .copied()
        .unwrap_or(0);
    let flowing = FLOW_TEXTURE | u32::from(!water);
    let show_top = !same(p + IVec3::Y);
    let corners = [p, p + IVec3::Z, p + IVec3::X + IVec3::Z, p + IVec3::X];
    let heights = corners.map(|at| fluid_corner_height(world, at, state) - if show_top { 0.001 } else { 0. });
    let base = p.as_vec3();
    let top = [
        base + Vec3::Y * heights[0],
        base + Vec3::Z + Vec3::Y * heights[1],
        base + Vec3::X + Vec3::Z + Vec3::Y * heights[2],
        base + Vec3::X + Vec3::Y * heights[3],
    ];
    if show_top {
        let flow = crate::vanilla::movement::flow(world, p);
        let moving = flow.x != 0. || flow.z != 0.;
        let uv = if moving {
            let angle = (crate::gameplay::ai::motion::atan2(flow.z, flow.x) - std::f64::consts::FRAC_PI_2) as f32;
            let sin = math::sin(angle) * 0.25;
            let cos = math::cos(angle) * 0.25;
            [
                [0.5 - cos - sin, 0.5 - cos + sin],
                [0.5 - cos + sin, 0.5 + cos + sin],
                [0.5 + cos + sin, 0.5 + cos - sin],
                [0.5 + cos - sin, 0.5 - cos - sin],
            ]
        } else {
            [[0., 0.], [0., 1.], [1., 1.], [1., 0.]]
        };
        let face = Face {
            layer: (if moving { flowing } else { still }) | texture_flags(state),
            color: tint(world, assets, p, state, 1, true),
            light: [fluid_light(world, p); 4],
            ao: [1.0; 4],
            uv: CubeUv::Tile(0),
            overlay: None,
        };
        for positive in [true, false] {
            if !positive && !(-1..=1).any(|x| (-1..=1).any(|z| {
                let other = world.get(p + IVec3::new(x, 1, z));
                !crate::block_ticks::same_liquid(other, state) && !material::full_block(other)
            })) {
                break;
            }
            let first = mesh.vertices.len();
            quad(mesh, top, 1., 1., face, positive, 1);
            for (vertex, uv) in mesh.vertices[first..].iter_mut().zip(uv) {
                vertex.uv = uv;
            }
            mesh.visible_faces += 1;
        }
    }
    if visible(p - IVec3::Y) {
        let first = mesh.vertices.len();
        quad(
            mesh,
            [
                base,
                base + Vec3::Z,
                base + Vec3::X + Vec3::Z,
                base + Vec3::X,
            ],
            1.,
            1.,
            Face {
                layer: still | texture_flags(state),
                color: [127, 127, 127, 255],
                light: [fluid_light(world, p - IVec3::Y); 4],
                ao: [1.0; 4],
                uv: CubeUv::Tile(0),
                overlay: None,
            },
            false,
            1,
        );
        for (vertex, uv) in mesh.vertices[first..].iter_mut().zip([[0., 0.], [0., 1.], [1., 1.], [1., 0.]]) {
            vertex.uv = uv;
        }
        mesh.visible_faces += 1;
    }
    for (direction, a, b, axis) in [
        (IVec3::NEG_Z, 0, 3, 2),
        (IVec3::Z, 2, 1, 2),
        (IVec3::NEG_X, 1, 0, 0),
        (IVec3::X, 3, 2, 0),
    ] {
        if !visible(p + direction) {
            continue;
        }
        let inward = direction.as_vec3() * -0.001;
        let first_top = top[a] + inward;
        let second_top = top[b] + inward;
        let bottom_a = Vec3::new(first_top.x, base.y, first_top.z);
        let bottom_b = Vec3::new(second_top.x, base.y, second_top.z);
        let positions = [first_top, second_top, bottom_b, bottom_a];
        let uv = [
            [0., (1. - heights[a]) * 0.5],
            [0.5, (1. - heights[b]) * 0.5],
            [0.5, 0.5],
            [0., 0.5],
        ];
        let color = tint(world, assets, p, state, axis, true);
        let face = Face {
            layer: flowing | texture_flags(state),
            color,
            light: [fluid_light(world, p + direction); 4],
            ao: [1.0; 4],
            uv: CubeUv::Tile(0),
            overlay: None,
        };
        for positive in [true, false] {
            let first = mesh.vertices.len();
            quad(mesh, positions, 1., 1., face, positive, axis);
            for (vertex, uv) in mesh.vertices[first..].iter_mut().zip(uv) {
                vertex.uv = uv;
            }
            mesh.visible_faces += 1;
        }
    }
}

fn crop_mesh(mesh: &mut Mesh, world: &World, p: Vec3, state: u16, assets: &Assets) {
    // The original crop.json has four double-sided planes at 4/16 and 12/16,
    // lowered 1/16 to meet farmland's visual surface.
    let base = p - Vec3::Y / 16.0;
    let face = Face {
        layer: assets.texture(state, 2, true) | texture_flags(state),
        color: [255; 4],
        light: [crate::vanilla::ambient_occlusion::uv(crate::visual_light::visual_mixed(
            world,
            p.floor().as_ivec3(),
        )); 4],
        ao: [1.0; 4],
        uv: CubeUv::Tile(0),
        overlay: None,
    };
    for axis in [0, 2] {
        for at in [0.25, 0.75] {
            let mut start = base;
            start[axis] += at;
            let span = if axis == 0 { Vec3::Z } else { Vec3::X };
            let positions = [start, start + span, start + span + Vec3::Y, start + Vec3::Y];
            for positive in [false, true] {
                quad(mesh, positions, 1.0, 1.0, face, positive, 2);
            }
        }
    }
    mesh.visible_faces += 8;
}

fn partial_mesh(
    mesh: &mut Mesh,
    world: &World,
    p: IVec3,
    state: u16,
    assets: &Assets,
    settings: &crate::settings::Settings,
) {
    let origin = p.as_vec3();
    visual_boxes(world, p, state, |min, max| {
        let local_min = min - origin;
        let local_max = max - origin;
        for axis in 0..3 {
            let u = (axis + 1) % 3;
            let v = (axis + 2) % 3;
            for positive in [false, true] {
                let face_idx = [[4, 5], [0, 1], [2, 3]][axis][usize::from(positive)];
                let my_mask = crate::visual_light::box_face_mask(local_min, local_max, face_idx);
                if my_mask != 0 {
                    let adj = p + crate::vanilla::ambient_occlusion::DIRECTIONS[face_idx];
                    let neighbor = world.get(adj);
                    if graphics_opaque(neighbor, settings.fancy)
                        || (blend_layer(neighbor) == 0
                            && matches!(
                                neighbor >> 4,
                                44 | 126
                                    | 182
                                    | 53
                                    | 67
                                    | 108
                                    | 109
                                    | 114
                                    | 128
                                    | 134..=136
                                    | 156
                                    | 163
                                    | 164
                                    | 180
                            )
                            && (my_mask & !crate::visual_light::face_mask(world, adj, face_idx ^ 1))
                                == 0)
                    {
                        continue;
                    }
                }
                let mut start = min;
                start[axis] = if positive { max[axis] } else { min[axis] };
                let mut du = Vec3::ZERO;
                du[u] = max[u] - min[u];
                let mut dv = Vec3::ZERO;
                dv[v] = max[v] - min[v];
                let first = mesh.vertices.len();
                let (ao, light) = face_lighting(
                    world,
                    p,
                    axis,
                    positive,
                    [min - p.as_vec3(), max - p.as_vec3()],
                    settings,
                );
                quad(
                    mesh,
                    [start, start + du, start + du + dv, start + dv],
                    du[u],
                    dv[v],
                    Face {
                        layer: assets.texture(state, axis, positive) | texture_flags(state),
                        color: tint(world, assets, p, state, axis, positive),
                        light,
                        ao,
                        uv: CubeUv::Tile(0),
                        overlay: None,
                    },
                    positive,
                    axis,
                );
                // Project local block coordinates to crop, rather than stretch,
                // the original tile on slabs, rails, stair risers and door edges.
                for vertex in &mut mesh.vertices[first..] {
                    let local = Vec3::from(vertex.position) - p.as_vec3();
                    vertex.uv = match axis {
                        0 => [
                            if positive { 1.0 - local.z } else { local.z },
                            1.0 - local.y,
                        ],
                        1 => [local.x, local.z],
                        _ => [
                            if positive { local.x } else { 1.0 - local.x },
                            1.0 - local.y,
                        ],
                    };
                }
                mesh.visible_faces += 1;
            }
        }
    });
}

/// Visual bounds deliberately differ from collisions on snow, farmland, soul
/// sand, fences and walls. The remaining shared shapes stay in sync with motion.
fn visual_boxes(world: &World, p: IVec3, state: u16, mut emit: impl FnMut(Vec3, Vec3)) {
    let id = state >> 4;
    let meta = state & 15;
    let origin = p.as_vec3();
    let mut add =
        |min: [f32; 3], max: [f32; 3]| emit(origin + Vec3::from(min), origin + Vec3::from(max));
    match id {
        60 => add([0.0; 3], [1.0, 0.9375, 1.0]),
        78 => add([0.0; 3], [1.0, ((meta & 7) + 1) as f32 / 8.0, 1.0]),
        88 => add([0.0; 3], [1.0; 3]),
        85 | 113 | 139 | 188..=192 => {
            let connects = |d: IVec3| {
                let neighbor = world.get(p + d);
                let other = neighbor >> 4;
                other == id
                    || matches!(other, 107 | 183..=187)
                    || (assets::solid(neighbor)
                        && !assets::see_through(neighbor)
                        && !assets::partial_model(neighbor))
            };
            let directions = [-IVec3::X, IVec3::X, -IVec3::Z, IVec3::Z];
            if id == 139 {
                add([0.25, 0.0, 0.25], [0.75, 1.0, 0.75]);
                for d in directions {
                    if !connects(d) {
                        continue;
                    }
                    let mut min = [0.3125, 0.0, 0.3125];
                    let mut max = [0.6875, 0.8125, 0.6875];
                    if d.x < 0 {
                        min[0] = 0.0;
                    }
                    if d.x > 0 {
                        max[0] = 1.0;
                    }
                    if d.z < 0 {
                        min[2] = 0.0;
                    }
                    if d.z > 0 {
                        max[2] = 1.0;
                    }
                    add(min, max);
                }
            } else {
                add([0.375, 0.0, 0.375], [0.625, 1.0, 0.625]);
                for d in directions {
                    if !connects(d) {
                        continue;
                    }
                    for y in [0.375, 0.75] {
                        let mut min = [0.4375, y, 0.4375];
                        let mut max = [0.5625, y + 0.1875, 0.5625];
                        if d.x < 0 {
                            min[0] = 0.0;
                        }
                        if d.x > 0 {
                            max[0] = 1.0;
                        }
                        if d.z < 0 {
                            min[2] = 0.0;
                        }
                        if d.z > 0 {
                            max[2] = 1.0;
                        }
                        add(min, max);
                    }
                }
            }
        }
        _ => crate::player::block_collision_boxes(world, p, state, emit),
    }
}

#[path = "frustum_packed.rs"]
mod frustum_packed;
#[cfg(test)]
#[path = "frustum_packed_tests.rs"]
mod frustum_packed_tests;

pub struct Frustum {
    planes: [Vec4; 6],
    // Explicit experiment only. Inline storage preserves scalar construction
    // by default without a heap allocation; pixel_line still reads planes.
    packed: Option<frustum_packed::Planes>,
}
impl Frustum {
    pub fn new(matrix: Mat4) -> Self {
        Self::new_with_packed(matrix, false)
    }
    pub(crate) fn new_with_packed(matrix: Mat4, packed: bool) -> Self {
        let r0 = matrix.row(0);
        let r1 = matrix.row(1);
        let r2 = matrix.row(2);
        let r3 = matrix.row(3);
        let planes = [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2, r3 - r2];
        Self { planes, packed: packed.then(|| frustum_packed::Planes::new(planes)) }
    }
    #[cfg(test)]
    pub(crate) fn packed_for_test(&self) -> bool { self.packed.is_some() }
    pub fn visible(&self, min: Vec3, max: Vec3) -> bool {
        if let Some(packed) = &self.packed { return packed.visible(min, max); }
        self.planes.iter().all(|p| {
            let corner = Vec3::new(
                if p.x >= 0.0 { max.x } else { min.x },
                if p.y >= 0.0 { max.y } else { min.y },
                if p.z >= 0.0 { max.z } else { min.z },
            );
            p.truncate().dot(corner) + p.w >= 0.0
        })
    }
    /// Expand a GL-width-1 segment in physical pixels, including perspective and camera roll.
    pub(crate) fn pixel_line(&self, mut a: Vec3, mut b: Vec3, height: u32) -> Option<[Vec3; 4]> {
        let near_a = self.planes[4].dot(a.extend(1.));
        let near_b = self.planes[4].dot(b.extend(1.));
        if near_a < 0. && near_b < 0. {
            return None;
        }
        if near_a < 0. {
            a = a.lerp(b, near_a / (near_a - near_b));
        } else if near_b < 0. {
            b = b.lerp(a, near_b / (near_b - near_a));
        }
        let x = (self.planes[0] - self.planes[1]) * 0.5;
        let y = (self.planes[2] - self.planes[3]) * 0.5;
        let w = (self.planes[0] + self.planes[1]) * 0.5;
        let sx = x.truncate().length();
        let sy = y.truncate().length();
        let wa = w.dot(a.extend(1.));
        let wb = w.dot(b.extend(1.));
        let aspect = glam::Vec2::new(sy / sx, 1.);
        let project = |p: Vec3, depth: f32| {
            glam::Vec2::new(x.dot(p.extend(1.)), y.dot(p.extend(1.))) / depth * aspect
        };
        let direction = project(b, wb) - project(a, wa);
        if direction.length_squared() < 1e-12 {
            return None;
        }
        let normal = glam::Vec2::new(direction.y, -direction.x).normalize();
        let offset = (x.truncate() / sx * normal.x + y.truncate() / sy * normal.y)
            / (sy * height.max(1) as f32);
        // A separate depth at each endpoint keeps the entire segment exactly one pixel wide.
        Some([a - offset * wa, a + offset * wa, b + offset * wb, b - offset * wb])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Chunk;
    use std::{collections::HashMap, path::PathBuf, sync::Arc};
    #[test]
    fn translucent_layer_matches_original_block_registry() {
        let blocks: Vec<u16> = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-translucent-blocks.json"
        ))
        .unwrap();
        for state in 0..4096 {
            assert_eq!(
                blend_layer(state) != 0,
                blocks.contains(&(state >> 4)),
                "state {state}"
            );
        }
        for id in [8, 9, 10, 11, 79, 95, 165, 1] {
            let mesh = shape_mesh(id << 4);
            assert!(!mesh.vertices.is_empty(), "block {id}");
            for vertex in mesh.vertices {
                assert_eq!(
                    vertex.layer & crate::models::BLEND != 0,
                    blocks.contains(&id),
                    "block {id}"
                );
            }
        }
    }
    #[test]
    fn rail_models_are_double_sided_planes_with_original_slope_endpoints() {
        for id in [27_u16, 28, 66, 157] {
            for powered in if id == 66 { vec![0] } else { vec![0, 8] } {
                for shape in 0..=if id == 66 { 9 } else { 5 } {
                    let state = (id << 4) | shape | powered;
                    assert!(assets::partial_model(state));
                    let mesh = shape_mesh(state);
                    assert_eq!(mesh.quads(), 2, "rail {id}, metadata {}", shape | powered);
                    assert_eq!(mesh.visible_faces, 2);
                    let (min, max) = vertex_bounds(&mesh);
                    assert_eq!(min, Vec3::new(8., 1.0625, 8.));
                    assert_eq!(
                        max,
                        Vec3::new(
                            9.,
                            if (2..=5).contains(&shape) {
                                2.0625
                            } else {
                                1.0625
                            },
                            9.
                        )
                    );
                    assert!(mesh.max.y >= max.y);
                    for v in &mesh.vertices[..4] {
                        let [x, y, z] = v.position;
                        let expected = match shape {
                            2 => 1.0625 + (x - 8.),
                            3 => 2.0625 - (x - 8.),
                            4 => 2.0625 - (z - 8.),
                            5 => 1.0625 + (z - 8.),
                            _ => 1.0625,
                        };
                        assert_eq!(y, expected);
                        assert_eq!(v.color, [255; 4]);
                    }
                    for (top, bottom) in mesh.vertices[..4].iter().zip(&mesh.vertices[4..]) {
                        assert_eq!(top.position, bottom.position);
                        assert_eq!(top.uv, bottom.uv);
                        assert_eq!(bottom.color, [127, 127, 127, 255]);
                    }
                    for (quad, indices) in mesh.indices.as_chunks::<6>().0.iter().enumerate() {
                        let [a, b, c] = std::array::from_fn(|i| {
                            Vec3::from(mesh.vertices[indices[i] as usize].position)
                        });
                        assert!((b - a).cross(c - a).y * if quad == 0 { 1. } else { -1. } > 0.);
                    }
                }
            }
        }
    }
    #[test]
    fn rail_uvs_rotate_with_straight_and_curved_blockstate_variants() {
        for (shape, expected_uv) in [
            (0, [0., 0.]),
            (1, [0., 1.]),
            (6, [0., 0.]),
            (7, [0., 1.]),
            (8, [1., 1.]),
            (9, [1., 0.]),
        ] {
            let mesh = shape_mesh((66 << 4) | shape);
            let north_west = mesh.vertices[..4]
                .iter()
                .find(|v| v.position[0] == 8. && v.position[2] == 8.)
                .unwrap();
            assert_eq!(north_west.uv, expected_uv, "rail shape {shape}");
        }
        let names = [
            "rail_normal",
            "rail_normal_turned",
            "rail_golden",
            "rail_golden_powered",
            "rail_detector",
            "rail_detector_powered",
            "rail_activator",
            "rail_activator_powered",
        ];
        let assets = Assets {
            identity: Default::default(),
            pixels: vec![],
            tile_size: 16,
            layers: 9,
            materials: names
                .into_iter()
                .enumerate()
                .map(|(i, name)| (name.into(), i as u32 + 1))
                .collect(),
            fallback: HashMap::new(),
            biome_colors: Default::default(),
            block_models: Default::default(),
            animations: Default::default(),
            flowing_tiles: Default::default(),
        };
        for (state, layer) in [
            (66 << 4, 1),
            (66 << 4 | 5, 1),
            (66 << 4 | 6, 2),
            (66 << 4 | 9, 2),
            (27 << 4 | 2, 3),
            (27 << 4 | 10, 4),
            (28 << 4 | 3, 5),
            (28 << 4 | 11, 6),
            (157 << 4 | 4, 7),
            (157 << 4 | 12, 8),
        ] {
            assert_eq!(assets.texture(state, 1, true), layer);
            assert_eq!(assets.texture(state, 1, false), layer);
        }
    }
    #[test]
    fn grass_uvs_match_original_weighted_models_and_face_bakery() {
        #[derive(serde::Deserialize)]
        struct Fixture {
            faces: Vec<(u8, usize, bool, [[i32; 2]; 4])>,
            positions: Vec<[i32; 4]>,
        }
        let reference: Fixture = serde_json::from_str(include_str!("../tests/fixtures/vanilla189-grass-model.json")).unwrap();
        assert_eq!(reference.positions.len(), 80);
        for [x, y, z, model] in reference.positions {
            let p = IVec3::new(x, y, z);
            assert!(CubeUv::grass(p, true) == CubeUv::Grass(model as u8), "{p:?}");
            assert!(CubeUv::grass(p, false) == CubeUv::Grass(0));
        }
        assert_eq!(reference.faces.len(), 46);
        for (model, axis, positive, expected) in reference.faces {
            // Models 0–3 are regular grass rotations, 4 is the single snowy model.
            let turns = if model == 4 { 0 } else { model };
            let corners = CubeUv::Grass(turns).corners(axis, positive, 1., 1.);
            // UVs outside the unit square refer to integer repeats of the same array layer.
            let min = [0, 1].map(|c| corners.iter().map(|uv| uv[c]).fold(f32::INFINITY, f32::min));
            assert_eq!(corners.map(|uv| [(uv[0] - min[0]) as i32, (uv[1] - min[1]) as i32]), expected,
                "grass model {model}, axis {axis}, positive {positive}");
        }
    }
    #[test]
    fn grass_overlays_preserve_dirt_and_original_face_shading() {
        let mut chunk = Chunk::empty();
        chunk.set(7, 1, 8, 2 << 4);
        let mut world = World {
            chunks: crate::world::ChunkMap::from([((0, 0), Arc::new(chunk))]),
            spawn: Vec3::ZERO,
            label: String::new(),
            save_path: PathBuf::new(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        };
        let mut a = Assets {
            identity: Default::default(),
            pixels: vec![],
            tile_size: 16,
            layers: 7,
            materials: HashMap::from([
                ("grass_top".into(), 1),
                ("grass_side".into(), 2),
                ("grass_side_overlay".into(), 3),
                ("dirt".into(), 4),
                ("grass_side_snowed".into(), 5),
            ]),
            fallback: HashMap::new(),
            biome_colors: Default::default(),
            block_models: Default::default(),
            animations: Default::default(),
            flowing_tiles: Default::default(),
        };
        a.biome_colors.grass.fill(0x80c040);
        let settings = crate::settings::Settings {
            smooth_lighting: 0,
            alternate_blocks: true,
            ..Default::default()
        };
        let mesh = build_with_settings(&world, (0, 0), &a, &settings);
        assert_eq!(mesh.quads(), 10);
        let colors = |layer| {
            mesh.vertices
                .iter()
                .filter(move |v| v.layer == layer)
                .map(|v| v.color)
                .collect::<Vec<_>>()
        };
        assert!(colors(1).iter().all(|c| *c == [128, 192, 64, 255]));
        assert!(colors(2)
            .iter()
            .all(|c| matches!(*c, [153, 153, 153, 255] | [204, 204, 204, 255])));
        assert!(colors(3)
            .iter()
            .all(|c| matches!(*c, [76, 115, 38, 255] | [102, 153, 51, 255])));
        assert!(colors(4).iter().all(|c| *c == [127, 127, 127, 255]));
        let bottom_uvs = |mesh: &Mesh| {
            mesh.vertices.iter().filter(|v| v.layer == 4).map(|v| v.uv).collect::<Vec<_>>()
        };
        assert_eq!(bottom_uvs(&mesh), CubeUv::Grass(1).corners(1, false, 1., 1.));
        let mut no_alternatives = settings.clone();
        no_alternatives.alternate_blocks = false;
        let base = build_with_settings(&world, (0, 0), &a, &no_alternatives);
        assert_eq!(bottom_uvs(&base), CubeUv::Grass(0).corners(1, false, 1., 1.));
        world.set(IVec3::new(7, 2, 8), 78 << 4);
        let snowy = build_with_settings(&world, (0, 0), &a, &settings);
        assert_eq!(bottom_uvs(&snowy), bottom_uvs(&base));
        assert!(!snowy.vertices.iter().any(|v| v.layer == 3));
        assert_eq!(snowy.vertices.iter().filter(|v| v.layer == 5).count(), 16);
    }
    fn shape_mesh(state: u16) -> Mesh {
        adjacent_mesh(state, 0, &Default::default())
    }
    #[test]
    #[ignore = "uses installed 1.8.9 torch resources"]
    fn damage_pass_uses_all_90_original_torch_quads_without_a_unit_cube() {
        let assets = crate::validation::assets();
        let mut world = World {
            chunks: [((0, 0), Arc::new(Chunk::empty()))].into(),
            spawn: Vec3::ZERO,
            label: String::new(), save_path: Default::default(),
            edits: Default::default(), imported: false, light_revision: 0,
        };
        let settings = crate::settings::Settings::default();
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-torch-model.json")).unwrap();
        let mut count = 0;
        for row in rows {
            let mut mesh = crate::models::empty();
            world.set(IVec3::ZERO, row[0].as_u64().unwrap() as u16);
            block_damage(&mut mesh, &world, assets, &settings, IVec3::ZERO, 3, 0.);
            let expected = row[2].as_array().unwrap();
            assert_eq!(mesh.quads(), expected.len());
            for (quad, face) in mesh.vertices.chunks_exact(4).zip(expected) {
                for (v, point) in quad.iter().zip(face[2].as_array().unwrap()) {
                    let coords: [f32; 3] = std::array::from_fn(|i| f32::from_bits(point[i].as_u64().unwrap() as u32));
                    assert_eq!(v.position, coords);
                    assert_eq!(v.layer, assets.materials["destroy_stage_3"]);
                }
                count += 1;
            }
        }
        assert_eq!(count, 90);
        for state in [2 << 4, 54 << 4 | 2, 130 << 4 | 2, 146 << 4 | 2, 97 << 4 | 5] {
            let mut mesh = crate::models::empty();
            world.set(IVec3::ZERO, state);
            block_damage(&mut mesh, &world, assets, &settings, IVec3::ZERO, 7, 0.);
            assert!(mesh.quads() >= 6, "missing damage overlay for {state}");
        }
    }
    fn adjacent_mesh(state: u16, neighbor: u16, settings: &crate::settings::Settings) -> Mesh {
        let mut c = Chunk::empty();
        c.set(8, 1, 8, state);
        c.set(9, 1, 8, neighbor);
        let w = World {
            chunks: crate::world::ChunkMap::from([((0, 0), Arc::new(c))]),
            spawn: Vec3::ZERO,
            label: String::new(),
            save_path: PathBuf::new(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        };
        let a = Assets {
            identity: Default::default(),
            pixels: vec![],
            tile_size: 16,
            layers: 1,
            materials: HashMap::new(),
            fallback: HashMap::new(),
            biome_colors: Default::default(),
            block_models: Default::default(),
            animations: Default::default(),
            flowing_tiles: Default::default(),
        };
        build_with_settings(&w, (0, 0), &a, settings)
    }
    #[test]
    fn fancy_leaf_interiors_follow_block_leaves_base_across_states_and_species() {
        for fancy in [false, true] {
            let settings = crate::settings::Settings {
                fancy,
                ..Default::default()
            };
            for (a,b,expected) in [
                (18 << 4 | 4, 18 << 4 | 4, if fancy { 12 } else { 10 }),
                (18 << 4 | 4, 18 << 4 | 9, if fancy { 12 } else { 10 }),
                (18 << 4, 161 << 4 | 1, if fancy { 12 } else { 10 }),
                (161 << 4, 161 << 4 | 8, if fancy { 12 } else { 10 }),
                // Stone never exposes the adjacent leaf face. The reverse
                // stone face is visible through the foliage only in fancy.
                (18 << 4, 1 << 4, if fancy { 11 } else { 10 }),
            ] {
                assert_eq!(adjacent_mesh(a,b,&settings).visible_faces, expected,
                           "fancy={fancy} states={a},{b}");
            }
        }
    }
    fn vertex_bounds(mesh: &Mesh) -> (Vec3, Vec3) {
        mesh.vertices.iter().fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(min, max), vertex| {
                let p = Vec3::from(vertex.position);
                (min.min(p), max.max(p))
            },
        )
    }
    #[test]
    fn partial_models_use_visible_heights_instead_of_full_cubes() {
        for (state, min_y, max_y) in [
            (44 << 4, 1.0, 1.5),
            ((44 << 4) | 8, 1.5, 2.0),
            (60 << 4, 1.0, 1.9375),
            (78 << 4, 1.0, 1.125),
            (85 << 4, 1.0, 2.0),
            (171 << 4, 1.0, 1.0625),
        ] {
            let mesh = shape_mesh(state);
            let (min, max) = vertex_bounds(&mesh);
            assert_eq!(min.y, min_y);
            assert_eq!(max.y, max_y);
            assert_eq!(mesh.quads(), 6);
        }
        let stair = shape_mesh(53 << 4);
        assert_eq!(stair.quads(), 12);
        assert!(stair.vertices.iter().any(|v| v.position[1] == 1.5));
    }
    #[test]
    #[ignore = "uses the installed 1.8.9 door and cactus atlas"]
    fn door_cactus_world_mesh_matches_original_facebakery() {
        let assets = crate::validation::assets();
        let settings = Default::default();
        let mut world = World {
            chunks: [((0, 0), Arc::new(Chunk::empty()))].into(), spawn: Vec3::ZERO,
            label: String::new(), save_path: Default::default(), edits: Default::default(),
            imported: false, light_revision: 0,
        };
        let names: std::collections::HashMap<_, _> =
            assets.materials.iter().map(|(k, v)| (*v, k.as_bytes())).collect();
        let rows: Vec<(u16, u16, u16, u64)> = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-door-cactus-model.json"
        )).unwrap();
        assert_eq!(rows.len(), 7 * 32 + 16);
        let base = IVec3::new(8, 4, 8);
        for (id, lower, half, expected) in rows {
            let top = if id == 81 { 0 } else { id << 4 | 8 | (half & 1) };
            let chunk = Arc::make_mut(world.chunks.get_mut(&(0, 0)).unwrap());
            chunk.set(8, 4, 8, id << 4 | lower);
            chunk.set(8, 5, 8, top);
            let pos = if half & 8 != 0 { base + IVec3::Y } else { base };
            let state = world.get(pos);
            let faces = special_faces(&world, assets, pos, state, false).unwrap();
            let mut hash = 0xcbf29ce484222325u64;
            let mut add = |word: u32| hash = (hash ^ u64::from(word)).wrapping_mul(0x100000001b3);
            for q in faces {
                for &byte in names[&q.layer] { add(byte.into()); }
                add(q.facing as u32);
                add(q.cull_face.unwrap_or(6) as u32);
                add(q.shade.into());
                for v in q.vertices {
                    for word in v.position.to_array().into_iter().chain(v.uv) { add(word.to_bits()); }
                }
            }
            assert_eq!(hash, expected, "block {id}, lower {lower}, half/hinge {half}");
            let mut mesh = crate::models::empty();
            special_block_mesh(&mut mesh, &world, pos, state, assets, &settings);
            assert_eq!(mesh.quads(), if id == 81 { 6 } else { 5 });
            for (face, emitted) in faces.iter().zip(mesh.vertices.chunks_exact(4)) {
                for (point, vertex) in face.vertices.iter().zip(emitted) {
                    assert_eq!(vertex.position, (pos.as_vec3() + point.position).to_array());
                    assert_eq!(vertex.uv, point.uv);
                    assert_eq!(vertex.layer, face.layer | texture_flags(state));
                }
            }
        }
    }
    #[test]
    #[ignore = "requires Vulkan and the installed 1.8.9 resource JAR"]
    fn door_cactus_vulkan_open_closed_and_inset_sides() {
        use crate::{
            gameplay::{Mode, Simulation}, player::Player, renderer::{RenderInput, Renderer},
            settings::Settings, ui::{Frame, Gui, Screen},
        };
        let assets = crate::validation::assets();
        let resources = crate::validation::resources();
        let settings = Settings { clouds: 0, bobbing: false, ..Default::default() };
        let mut renderer = pollster::block_on(Renderer::new(None, assets, resources, &settings, None)).unwrap();
        let (width, height) = (576, 360);
        renderer.resize(width, height);
        let mut chunk = Chunk::empty();
        for x in 0..16 {
            for z in 0..16 { chunk.set(x, 63, z, 12 << 4); }
        }
        let mut world = World {
            chunks: [((0, 0), Arc::new(chunk))].into(), spawn: Vec3::ZERO,
            label: String::new(), save_path: Default::default(), edits: Default::default(),
            imported: false, light_revision: 0,
        };
        world.set(IVec3::new(9, 64, 8), 81 << 4);
        world.set(IVec3::new(7, 65, 8), 64 << 4 | 8);
        let at = glam::DVec3::new(9.35, 64.4, 11.5);
        let mut player = Player::new(at.as_vec3());
        let dir = (glam::DVec3::new(8.6, 64.95, 8.35) - at
            - glam::DVec3::Y * f64::from(player.eye_height())).normalize();
        player.set_pose_f64(at.to_array(), (-dir.x).atan2(dir.z).to_degrees() as f32,
            -dir.y.asin().to_degrees() as f32, true);
        let mut sim = Simulation::new(Mode::Creative, 189);
        sim.clock = 6000;
        let gui = Gui { screen: Screen::Playing, hide_hud: true, ..Default::default() };
        let frame = Frame::default();
        let temp = std::env::temp_dir().join(format!("ferrum-door-cactus-{}.png", std::process::id()));
        let mut frames = Vec::new();
        for lower in [1, 5] {
            world.set(IVec3::new(7, 64, 8), 64 << 4 | lower);
            renderer.upload((0, 0), build_with_settings(&world, (0, 0), assets, &settings));
            renderer.screenshot(&temp, &RenderInput {
                assets, resources, settings: &settings, gui: &gui, ui: &frame,
                selected_block: None, world: Some(&world), player: Some(&player),
                simulation: Some(&sim), alpha: 1., time: 4.,
            }).unwrap();
            frames.push(image::open(&temp).unwrap().to_rgb8());
        }
        std::fs::remove_file(temp).unwrap();
        let changed = frames[0].pixels().zip(frames[1].pixels()).filter(|(a, b)| a != b).count();
        assert!(changed > 1500, "opening the oak door changed {changed} pixels");
        if let Some(path) = std::env::var_os("FERRUM_DOOR_CACTUS_CAPTURE") {
            let mut strip = image::RgbImage::new(width * 2, height);
            for (i, frame) in frames.iter().enumerate() {
                image::imageops::replace(&mut strip, frame, i as i64 * i64::from(width), 0);
            }
            strip.save(path).unwrap();
        }
    }
    #[test]
    #[ignore = "uses the installed 1.8.9 crop models and atlas"]
    fn crop_growth_models_show_all_ages_and_fruit_attachments() {
        let assets = crate::validation::assets();
        let mut world = World {
            chunks: [((0, 0), Arc::new(Chunk::empty()))].into(), spawn: Vec3::ZERO,
            label: String::new(), save_path: Default::default(), edits: Default::default(),
            imported: false, light_revision: 0,
        };
        let p = IVec3::new(8, 4, 8);
        let render = |world: &World, state| {
            let mut mesh = crate::models::empty();
            special_block_mesh(&mut mesh, world, p, state, assets, &Default::default());
            assert!(!mesh.vertices.is_empty());
            assert!(mesh.vertices.iter().all(|v| v.layer != 0));
            mesh
        };
        for (stem, fruit, name) in [(104, 86, "pumpkin"), (105, 103, "melon")] {
            for age in 0..8 {
                let mesh = render(&world, stem << 4 | age);
                let (min, max) = vertex_bounds(&mesh);
                assert_eq!(min.y, p.y as f32 - 1./16.);
                assert_eq!(max.y, p.y as f32 + f32::from(1 + age * 2)/16.);
            }
            let mut rotations = Vec::new();
            for d in [IVec3::NEG_Z, IVec3::X, IVec3::Z, IVec3::NEG_X] {
                world.set(p + d, fruit << 4);
                let mesh = render(&world, stem << 4 | 7);
                let upper = assets.materials[&format!("{name}_stem_connected")];
                let quad: Vec<_> = mesh.vertices.iter().filter(|v| v.layer & 65535 == upper)
                    .map(|v| v.position.map(f32::to_bits)).collect();
                assert!(!quad.is_empty());
                assert!(!rotations.contains(&quad));
                rotations.push(quad);
                world.set(p + d, 0);
            }
        }
        for age in 0..4 {
            let stage = [0, 1, 1, 2][age as usize];
            let layer = assets.materials[&format!("nether_wart_stage_{stage}")];
            assert!(render(&world, 115 << 4 | age).vertices.iter().all(|v| v.layer & 65535 == layer));
        }
        for meta in 0..12 {
            let layer = assets.materials[&format!("cocoa_stage_{}", meta / 4)];
            assert!(render(&world, 127 << 4 | meta).vertices.iter().all(|v| v.layer & 65535 == layer));
        }
    }
    #[test]
    fn crops_use_four_double_sided_planes_at_farmland_surface() {
        for id in [59, 141, 142] {
            let mesh = shape_mesh((id << 4) | 7);
            assert_eq!(mesh.quads(), 8);
            let (min, max) = vertex_bounds(&mesh);
            assert_eq!(min.y, 0.9375);
            assert_eq!(max.y, 1.9375);
            assert!(mesh.vertices.iter().all(|v| v.color == [255; 4]));
        }
    }
    #[test]
    fn fluid_surface_uses_weighted_source_heights_and_adjacent_levels() {
        let mut c = Chunk::empty();
        for x in 7..=9 {
            for z in 7..=9 {
                c.set(x, 1, z, 9 << 4);
            }
        }
        let mut w = World {
            chunks: crate::world::ChunkMap::from([((0, 0), Arc::new(c))]),
            spawn: Vec3::ZERO,
            label: String::new(),
            save_path: PathBuf::new(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        };
        let p = IVec3::new(8, 1, 8);
        assert!((fluid_corner_height(&w, p, 9 << 4) - 8. / 9.).abs() < 1e-6);
        w.set(p + IVec3::Y, 8 << 4);
        assert_eq!(fluid_corner_height(&w, p, 9 << 4), 1.);
        w.set(p + IVec3::Y, 0);
        for x in 7..=9 {
            for z in 7..=9 {
                w.set(IVec3::new(x, 1, z), (8 << 4) | ((x - 7) * 2 + 1) as u16);
            }
        }
        let west = fluid_corner_height(&w, p, 8 << 4);
        let east = fluid_corner_height(&w, p + IVec3::X, 8 << 4);
        assert!(
            west > east,
            "surface must slope down towards shallower flow"
        );
        assert!(crate::vanilla::movement::flow(&w, p).x > 0.);
    }
    #[test]
    fn isolated_source_is_not_rendered_as_a_full_height_cube() {
        let mesh = shape_mesh(9 << 4);
        let (_, max) = vertex_bounds(&mesh);
        // Source samples get weight11; three adjacent air cells get weight1.
        assert!((max.y - (1. + 1. - (11. / 9. + 3.) / 14. - 0.001)).abs() < 1e-5);
        assert_eq!(mesh.quads(), 11); // double-sided top + 4 sides, single bottom.
    }
    #[test]
    fn solid_cube_collapses_to_six_quads_and_seams_are_hidden() {
        let mut c = Chunk::empty();
        for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    c.set(x, y, z, 16);
                }
            }
        }
        let mut chunks = crate::world::ChunkMap::new();
        chunks.insert((0, 0), Arc::new(c.clone()));
        let mut w = World {
            chunks,
            spawn: Vec3::ZERO,
            label: String::new(),
            save_path: PathBuf::new(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        };
        let a = Assets {
            identity: Default::default(),
            pixels: vec![],
            tile_size: 16,
            layers: 1,
            materials: HashMap::new(),
            fallback: HashMap::new(),
            biome_colors: Default::default(),
            block_models: Default::default(),
            animations: Default::default(),
            flowing_tiles: Default::default(),
        };
        let m = build(&w, (0, 0), &a);
        assert_eq!(m.quads(), 6);
        assert_eq!(m.visible_faces, 1536);
        for tri in m.indices.as_chunks::<3>().0 {
            let p = tri
                .iter()
                .map(|&i| Vec3::from_array(m.vertices[i as usize].position))
                .collect::<Vec<_>>();
            let normal = (p[1] - p[0]).cross(p[2] - p[0]);
            assert!(normal.dot(p[0] - Vec3::splat(8.0)) > 0.0);
        }
        w.chunks.insert((1, 0), Arc::new(c));
        assert_eq!(build(&w, (0, 0), &a).quads(), 5);
    }
    #[test]
    fn frustum_rejects_behind_camera() {
        let f = Frustum::new(Mat4::perspective_rh(1.2, 1.0, 0.1, 100.0));
        assert!(f.visible(Vec3::new(-1.0, -1.0, -5.0), Vec3::new(1.0, 1.0, -3.0)));
        assert!(!f.visible(Vec3::new(-1.0, -1.0, 3.0), Vec3::new(1.0, 1.0, 5.0)));
    }
}
