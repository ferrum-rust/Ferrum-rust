use crate::{
    vanilla::{
        ambient_occlusion::{self as ao, DIRECTIONS},
        lighting::{ChunkLight, Kind},
        material,
    },
    world::{ChunkPos, World, HEIGHT},
};
use glam::{IVec3, Vec3};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

#[allow(dead_code)]
#[path = "vanilla/ambient_occlusion_data.rs"]
mod ao_data;

#[inline]
pub(crate) fn is_partial_light_block(id: u16) -> bool {
    matches!(
        id,
        44 | 60
            | 126
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
}

#[inline]
pub(crate) fn box_face_mask(min_local: Vec3, max_local: Vec3, face: usize) -> u8 {
    let axis = [1, 1, 2, 2, 0, 0][face];
    let positive = (face & 1) != 0;
    if positive {
        if max_local[axis] <= 0.999 {
            return 0;
        }
    } else if min_local[axis] >= 0.001 {
        return 0;
    }
    let u = (axis + 1) % 3;
    let v = (axis + 2) % 3;
    let mut mask = 0u8;
    for (bit, (cu, cv)) in [
        (0.25, 0.25),
        (0.75, 0.25),
        (0.25, 0.75),
        (0.75, 0.75),
    ]
    .into_iter()
    .enumerate()
    {
        if min_local[u] <= cu && cu <= max_local[u] && min_local[v] <= cv && cv <= max_local[v] {
            mask |= 1 << bit;
        }
    }
    mask
}

pub(crate) fn face_mask(world: &World, p: IVec3, face: usize) -> u8 {
    if !(0..HEIGHT).contains(&p.y) {
        return 0;
    }
    let state = world.get(p);
    let id = state >> 4;
    let meta = state & 15;
    match id {
        44 | 126 | 182 => {
            let (min_y, max_y) = if meta & 8 == 0 { (0.0, 0.5) } else { (0.5, 1.0) };
            box_face_mask(Vec3::new(0.0, min_y, 0.0), Vec3::new(1.0, max_y, 1.0), face)
        }
        // Farmland [0, 0, 0]..[1, 0.9375, 1]: -Y is 0b1111, +Y is 0, and side faces
        // cover only the lower half (0b0011 / 0b0101) so light enters the top 1/16 gap.
        60 => box_face_mask(Vec3::ZERO, Vec3::new(1.0, 0.5, 1.0), face),
        53 | 67 | 108 | 109 | 114 | 128 | 134..=136 | 156 | 163 | 164 | 180 => {
            let origin = p.as_vec3();
            let mut mask = 0u8;
            crate::player::block_collision_boxes(world, p, state, |min, max| {
                mask |= box_face_mask(min - origin, max - origin, face);
            });
            mask
        }
        _ => {
            if material::light_opacity(state) >= 15 {
                0b1111
            } else {
                0
            }
        }
    }
}

#[inline]
pub(crate) fn aperture_open(world: &World, a: IVec3, face: usize, b: IVec3) -> bool {
    (face_mask(world, a, face) | face_mask(world, b, face ^ 1)) != 0b1111
}

fn neighborhood_has_partial(world: &World, c: ChunkPos) -> bool {
    (-1..=1).any(|dx| {
        (-1..=1).any(|dz| {
            world
                .chunks
                .get(&(c.0 + dx, c.1 + dz))
                .is_some_and(|ch| ch.partial_light_mask() != 0)
        })
    })
}

fn cur_light(
    world: &World,
    cows: &HashMap<ChunkPos, ChunkLight>,
    q: IVec3,
    kind: Kind,
    no_sky: bool,
) -> u8 {
    if q.y < 0 {
        return 0;
    }
    let pos = (q.x.div_euclid(16), q.z.div_euclid(16));
    if q.y >= HEIGHT {
        return if kind == Kind::Sky
            && !no_sky
            && world.chunks.get(&pos).is_some_and(|c| c.light.is_some())
        {
            15
        } else {
            0
        };
    }
    let Some(light) = cows
        .get(&pos)
        .or_else(|| world.chunks.get(&pos).and_then(|c| c.light.as_deref()))
    else {
        return 0;
    };
    if kind == Kind::Sky && (no_sky || light.no_sky) {
        0
    } else {
        light.get(kind, q)
    }
}

fn base_sky_open(world: &World, p: IVec3, no_sky: bool) -> bool {
    if no_sky {
        return false;
    }
    let pos = (p.x.div_euclid(16), p.z.div_euclid(16));
    let Some(base) = world.chunks.get(&pos).and_then(|c| c.light.as_deref()) else {
        return false;
    };
    if base.no_sky {
        return false;
    }
    let col = (p.z.rem_euclid(16) * 16 + p.x.rem_euclid(16)) as usize;
    let h = base.height.get(col).copied().unwrap_or(0);
    if p.y + 1 < h {
        return false;
    }
    if p.y + 1 >= HEIGHT {
        true
    } else {
        base.get(Kind::Sky, p + IVec3::Y) == 15
    }
}

fn write_light(
    world: &World,
    cows: &mut HashMap<ChunkPos, ChunkLight>,
    p: IVec3,
    kind: Kind,
    val: u8,
) {
    let pos = (p.x.div_euclid(16), p.z.div_euclid(16));
    let cow = cows
        .entry(pos)
        .or_insert_with(|| (**world.chunks[&pos].light.as_ref().unwrap()).clone());
    if cow.values.len() < 65536 {
        cow.values.resize(65536, 0);
    }
    let sec = (p.y as usize) >> 4;
    if !cow.sections[sec] {
        for y in (sec * 16) as i32..(sec * 16 + 16) as i32 {
            for col in 0..256 {
                let sky = if !cow.no_sky && y >= cow.height.get(col).copied().unwrap_or(0) {
                    15 << 4
                } else {
                    0
                };
                cow.values[y as usize * 256 + col] = sky;
            }
        }
        cow.sections[sec] = true;
    }
    let idx =
        (p.y as usize * 16 + p.z.rem_euclid(16) as usize) * 16 + p.x.rem_euclid(16) as usize;
    let prev = cow.values[idx];
    cow.values[idx] = match kind {
        Kind::Sky => (val << 4) | (prev & 15),
        Kind::Block => (prev & 0xf0) | (val & 15),
    };
}

pub(crate) fn reconcile(
    world: &mut World,
    dirty_chunks: &[ChunkPos],
    no_sky: bool,
) -> Vec<ChunkPos> {
    if dirty_chunks.is_empty() {
        return Vec::new();
    }
    let mut candidates = HashSet::new();
    for &c in dirty_chunks {
        for dx in -1..=1 {
            for dz in -1..=1 {
                let pos = (c.0 + dx, c.1 + dz);
                if world.chunks.contains_key(&pos) {
                    candidates.insert(pos);
                }
            }
        }
    }
    if candidates.is_empty() {
        return Vec::new();
    }

    let mut changed = Vec::new();
    let mut active_targets = HashSet::new();
    for &c in &candidates {
        if neighborhood_has_partial(world, c) {
            if world.chunks[&c].light.is_some() {
                active_targets.insert(c);
            } else if world.chunks[&c].visual_light.is_some() {
                Arc::make_mut(world.chunks.get_mut(&c).unwrap()).visual_light = None;
                changed.push(c);
            }
        } else if world.chunks[&c].visual_light.is_some() {
            Arc::make_mut(world.chunks.get_mut(&c).unwrap()).visual_light = None;
            changed.push(c);
        }
    }

    if active_targets.is_empty() {
        changed.sort_unstable();
        changed.dedup();
        return changed;
    }

    let mut seed_region = HashSet::new();
    for &t in &active_targets {
        for dx in -1..=1 {
            for dz in -1..=1 {
                let pos = (t.0 + dx, t.1 + dz);
                if world.chunks.get(&pos).is_some_and(|ch| ch.light.is_some()) {
                    seed_region.insert(pos);
                }
            }
        }
    }
    let mut seed_chunks: Vec<ChunkPos> = seed_region.iter().copied().collect();
    seed_chunks.sort_unstable();

    let mut cows: HashMap<ChunkPos, ChunkLight> = HashMap::new();
    let mut queue: VecDeque<(IVec3, u8)> = VecDeque::new();

    for kind in [Kind::Sky, Kind::Block] {
        if kind == Kind::Sky && no_sky {
            continue;
        }
        queue.clear();
        for &cpos in &seed_chunks {
            let chunk = &world.chunks[&cpos];
            let mut mask = chunk.partial_light_mask();
            if mask == 0 {
                continue;
            }
            let max_y = chunk.top.clamp(0, HEIGHT);
            while mask != 0 {
                let sec = mask.trailing_zeros() as i32;
                mask &= mask - 1;
                let y_end = (sec * 16 + 16).min(max_y);
                for y in sec * 16..y_end {
                    for lz in 0..16 {
                        for lx in 0..16 {
                            let state = chunk.get(lx, y, lz);
                            if !is_partial_light_block(state >> 4) {
                                continue;
                            }
                            let p = IVec3::new(cpos.0 * 16 + lx, y, cpos.1 * 16 + lz);
                            let mut desired = match kind {
                                Kind::Sky => {
                                    if face_mask(world, p, 1) != 0b1111
                                        && aperture_open(world, p, 1, p + IVec3::Y)
                                        && base_sky_open(world, p, no_sky)
                                    {
                                        15
                                    } else {
                                        0
                                    }
                                }
                                Kind::Block => material::light_value(state),
                            };
                            for (f, &dir) in DIRECTIONS.iter().enumerate() {
                                let q = p + dir;
                                if aperture_open(world, p, f, q) {
                                    desired = desired.max(
                                        cur_light(world, &cows, q, kind, no_sky).saturating_sub(1),
                                    );
                                }
                                if kind == Kind::Block && face_mask(world, p, f) != 0b1111 {
                                    let emit = material::light_value(world.get(q));
                                    if emit > 0 {
                                        desired = desired.max(emit.saturating_sub(1));
                                    }
                                }
                            }
                            if desired > cur_light(world, &cows, p, kind, no_sky) {
                                write_light(world, &mut cows, p, kind, desired);
                                if desired > 1 {
                                    queue.push_back((p, desired));
                                }
                            }
                        }
                    }
                }
            }
        }

        while let Some((u, val)) = queue.pop_front() {
            if val <= 1 || val < cur_light(world, &cows, u, kind, no_sky) {
                continue;
            }
            for (f, &dir) in DIRECTIONS.iter().enumerate() {
                let v = u + dir;
                if !(0..HEIGHT).contains(&v.y) {
                    continue;
                }
                let v_pos = (v.x.div_euclid(16), v.z.div_euclid(16));
                if !seed_region.contains(&v_pos) {
                    continue;
                }
                if !aperture_open(world, u, f, v) {
                    continue;
                }
                let st_v = world.get(v);
                let att = if is_partial_light_block(st_v >> 4) {
                    1
                } else {
                    let op = material::light_opacity(st_v);
                    if op >= 15 {
                        continue;
                    }
                    op.max(1)
                };
                let next = val.saturating_sub(att);
                if next > cur_light(world, &cows, v, kind, no_sky) {
                    write_light(world, &mut cows, v, kind, next);
                    if next > 1 {
                        queue.push_back((v, next));
                    }
                }
            }
        }
    }

    for pos in active_targets {
        let chunk = world.chunks.get_mut(&pos).unwrap();
        match cows.remove(&pos) {
            Some(new_vl) => {
                let same = chunk.visual_light.as_deref().is_some_and(|old_vl| {
                    old_vl.no_sky == new_vl.no_sky
                        && old_vl.sections == new_vl.sections
                        && old_vl.height == new_vl.height
                        && old_vl.values == new_vl.values
                });
                if !same {
                    Arc::make_mut(chunk).visual_light = Some(Arc::new(new_vl));
                    changed.push(pos);
                }
            }
            None => {
                if chunk.visual_light.is_some() {
                    Arc::make_mut(chunk).visual_light = None;
                    changed.push(pos);
                }
            }
        }
    }

    changed.sort_unstable();
    changed.dedup();
    changed
}

#[inline]
pub(crate) fn max_mixed(a: u32, b: u32) -> u32 {
    ((a >> 16).max(b >> 16) << 16) | (a & 0xffff).max(b & 0xffff)
}

#[inline]
pub(crate) fn visual_mixed(world: &World, p: IVec3) -> u32 {
    let state = world.get(p);
    let uv = world.visual_light_uv(p, material::light_value(state));
    (uv[1] as u32) << 16 | uv[0] as u32
}

fn facts(state: u16) -> [u32; 4] {
    ao_data::FACTS
        .get((state >> 4) as usize)
        .copied()
        .unwrap_or(ao_data::FACTS[0])
}

fn shade_at(world: &World, p: IVec3) -> f32 {
    f32::from_bits(facts(world.get(p))[1])
}

fn translucent_at(world: &World, p: IVec3) -> bool {
    facts(world.get(p))[0] != 0
}

fn opaque_visual(world: &World, p: IVec3, fancy: bool) -> bool {
    let state = world.get(p);
    if matches!(state >> 4, 18 | 161) {
        !fancy
    } else {
        facts(state)[2] != 0
    }
}

fn average(a: u32, b: u32, c: u32, center: u32) -> u32 {
    let fallback = |v| if v == 0 { center } else { v };
    ((fallback(a) + fallback(b) + fallback(c) + center) >> 2) & 0xff00ff
}

pub(crate) fn sample_ao(
    world: &World,
    p: IVec3,
    face: usize,
    bounds: [f32; 6],
    offset: bool,
    partial: bool,
    fancy: bool,
) -> ao::Face {
    let step = DIRECTIONS[face];
    let base = if offset { p + step } else { p };
    let side_faces = ao_data::SIDES[face];
    let sides = side_faces.map(|d| DIRECTIONS[d]);
    let positions = sides.map(|d| base + d);
    let side_open: [bool; 4] =
        std::array::from_fn(|k| aperture_open(world, base, side_faces[k], positions[k]));
    let light: [u32; 4] = std::array::from_fn(|k| {
        if side_open[k]
            || (face_mask(world, base, side_faces[k]) != 0b1111
                && material::light_value(world.get(positions[k])) > 0)
        {
            visual_mixed(world, positions[k])
        } else {
            0
        }
    });
    let colors = positions.map(|at| shade_at(world, at));
    let pass: [bool; 4] = std::array::from_fn(|k| {
        let q = positions[k] + step;
        translucent_at(world, q)
            || (is_partial_light_block(world.get(q) >> 4)
                && aperture_open(world, positions[k], face, q))
    });
    let pairs = [(0, 2), (0, 3), (1, 2), (1, 3)];
    let corners = pairs.map(|(a, b)| {
        if pass[a] || pass[b] {
            let at = positions[a] + sides[b];
            let emit_at = material::light_value(world.get(at)) > 0;
            let open_a = side_open[a]
                && (aperture_open(world, positions[a], side_faces[b], at)
                    || (emit_at && face_mask(world, positions[a], side_faces[b]) != 0b1111));
            let open_b = side_open[b]
                && (aperture_open(world, positions[b], side_faces[a], at)
                    || (emit_at && face_mask(world, positions[b], side_faces[a]) != 0b1111));
            let corner_light = if open_a || open_b {
                visual_mixed(world, at)
            } else {
                0
            };
            (shade_at(world, at), corner_light)
        } else {
            (colors[a], light[a])
        }
    });

    let mut center = visual_mixed(world, p);
    if offset {
        center = visual_mixed(world, p + step);
    } else if !opaque_visual(world, p + step, fancy) && aperture_open(world, p, face, p + step) {
        center = max_mixed(center, visual_mixed(world, p + step));
    }
    let center_color = shade_at(world, base);
    let order = [(3, 0, 1), (2, 0, 0), (2, 1, 2), (3, 1, 3)];
    let color = order.map(|(a, b, c)| (colors[a] + colors[b] + corners[c].0 + center_color) * 0.25);
    let brightness = order.map(|(a, b, c)| average(light[a], light[b], corners[c].1, center));
    let mut result = ao::Face {
        shade: [0.0; 4],
        brightness: [0; 4],
    };
    for i in 0..4 {
        let output = ao_data::TRANSLATIONS[face][i];
        if partial && ao_data::PARTIAL[face] {
            let bound = |n: usize| {
                if n < 6 {
                    bounds[n]
                } else {
                    1.0 - bounds[n - 6]
                }
            };
            let indices = ao_data::WEIGHTS[face][i];
            let weights: [f32; 4] =
                std::array::from_fn(|j| bound(indices[j * 2]) * bound(indices[j * 2 + 1]));
            result.shade[output] = color[0] * weights[0]
                + color[1] * weights[1]
                + color[2] * weights[2]
                + color[3] * weights[3];
            let weighted = |shift: u32| {
                let channels = brightness.map(|v| ((v >> shift) & 255u32) as f32);
                (channels[0] * weights[0]
                    + channels[1] * weights[1]
                    + channels[2] * weights[2]
                    + channels[3] * weights[3]) as u32
                    & 255
            };
            result.brightness[output] = weighted(16) << 16 | weighted(0);
        } else {
            result.shade[output] = color[i];
            result.brightness[output] = brightness[i];
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lighting_view::LightingView, world::Chunk};

    fn test_world() -> World {
        let mut chunks = crate::world::ChunkMap::new();
        for cx in -1..=1 {
            for cz in -1..=1 {
                let mut c = Chunk::empty();
                for x in 0..16 {
                    for z in 0..16 {
                        c.set(x, 0, z, 7 << 4);
                    }
                }
                chunks.insert((cx, cz), Arc::new(c));
            }
        }
        World {
            chunks,
            spawn: Vec3::ZERO,
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        }
    }

    #[test]
    fn quadrant_masks_and_apertures_are_symmetric_and_consistent() {
        let mut w = test_world();
        let a = IVec3::new(4, 5, 4);
        let b = IVec3::new(5, 5, 4);
        // Bottom slab next to top slab seals the horizontal seam completely.
        w.set(a, 44 << 4);
        w.set(b, (44 << 4) | 8);
        assert_eq!(face_mask(&w, a, 0), 0b1111);
        assert_eq!(face_mask(&w, a, 1), 0);
        assert_eq!(face_mask(&w, b, 0), 0);
        assert_eq!(face_mask(&w, b, 1), 0b1111);
        assert!(!aperture_open(&w, a, 5, b));
        assert!(!aperture_open(&w, b, 4, a));

        // Two bottom slabs share an open upper half.
        w.set(b, 44 << 4);
        assert!(aperture_open(&w, a, 5, b));
        assert!(aperture_open(&w, b, 4, a));

        // Farmland leaves top and upper side apertures open while sealing -Y.
        w.set(a, 60 << 4);
        assert_eq!(face_mask(&w, a, 0), 0b1111);
        assert_eq!(face_mask(&w, a, 1), 0);
        assert_eq!(face_mask(&w, a, 2), 0b0011);
        assert_eq!(face_mask(&w, a, 4), 0b0101);
    }

    #[test]
    fn slab_tunnel_propagates_and_cleans_up_visual_light_without_mutating_base_light() {
        let mut w = test_world();
        // Enclosed stone box from x=4..=10, y=4..=6, z=4..=6 with a bottom-slab entrance at x=4,y=5,z=5.
        for x in 4..=10 {
            for y in 4..=6 {
                for z in 4..=6 {
                    let boundary = x == 4 || x == 10 || y == 4 || y == 6 || z == 4 || z == 6;
                    if boundary {
                        w.set(IVec3::new(x, y, z), 1 << 4);
                    }
                }
            }
        }
        w.set(IVec3::new(4, 5, 5), 44 << 4); // bottom slab window/gap
        let mut view = LightingView::new(&mut w);

        let torch = IVec3::new(3, 5, 5);
        w.set(torch, (50 << 4) | 5); // torch (light 14) outside the slab gap
        view.update(&mut w);

        // Vanilla game light stays 0 inside the sealed-to-vanilla chamber,
        // while visual light propagates cleanly through the open upper half of the slab!
        assert_eq!(w.light_uv(IVec3::new(5, 5, 5), 0)[0], 0);
        assert_eq!(w.visual_light_uv(IVec3::new(4, 5, 5), 0)[0], 13 * 16);
        assert_eq!(w.visual_light_uv(IVec3::new(5, 5, 5), 0)[0], 12 * 16);

        // Removing the torch restores darkness in visual_light as well (no stale high values).
        w.set(torch, 0);
        view.update(&mut w);
        assert_eq!(w.visual_light_uv(IVec3::new(5, 5, 5), 0)[0], 0);
    }

    #[test]
    fn stair_solid_back_blocks_ao_and_bfs_light_leaks() {
        let mut w = test_world();
        for x in 4..=8 {
            for y in 4..=6 {
                for z in 4..=8 {
                    let boundary = x == 4 || x == 8 || y == 4 || y == 6 || z == 4 || z == 8;
                    if boundary {
                        w.set(IVec3::new(x, y, z), 1 << 4);
                    }
                }
            }
        }
        // Stair at x=4,y=5,z=6 with meta=1 (ascending -X, so +X side at x=4..5? Check meta=0 vs meta=1):
        // meta=0 ascends +X (solid back on +X face 5), facing into the dark room on +X!
        w.set(IVec3::new(4, 5, 6), 53 << 4);
        assert_eq!(face_mask(&w, IVec3::new(4, 5, 6), 5), 0b1111);
        let mut view = LightingView::new(&mut w);
        w.set(IVec3::new(3, 5, 6), 89 << 4); // glowstone outside the stair's open front
        view.update(&mut w);

        // Stair itself receives light on its open -X front, but its solid +X back blocks light into (5,5,6)
        // and prevents AO on the dark room floor (5,4,6) from bleeding glowstone light through the stair back!
        assert!(w.visual_light_uv(IVec3::new(4, 5, 6), 0)[0] > 0);
        assert_eq!(w.visual_light_uv(IVec3::new(5, 5, 6), 0)[0], 0);
        let floor_ao = sample_ao(&w, IVec3::new(5, 4, 6), 1, [1.0, 1.0, 0.0, 1.0, 0.0, 1.0], true, false, true);
        assert_eq!(floor_ao.brightness.map(|b| b & 0xff), [0, 0, 0, 0]);
    }
}
