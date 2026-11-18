//! Persistent local ItemEmptyMap/ItemMap data, explored pixels and held-map publication.
use super::*;
use std::collections::BTreeMap;
include!("map_colors.rs");

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Maps {
    next_id: u16,
    pub data: BTreeMap<u16, Map>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Map {
    pub center: [i32; 2],
    pub dimension: i8,
    pub scale: u8,
    pub colors: Vec<u8>,
    #[serde(skip)]
    step: u32,
    #[serde(skip)]
    revision: u64,
}
impl Map {
    fn new(position: [f64; 3], scale: u8, dimension: i32) -> Self {
        let size = 128 * (1 << scale);
        let center = [position[0], position[2]].map(|p| {
            (((p + 64.) / f64::from(size)).floor() as i32).wrapping_mul(size) + size / 2 - 64
        });
        Self {
            center,
            dimension: dimension as i8,
            scale,
            colors: vec![0; 128 * 128],
            step: 0,
            revision: 0,
        }
    }
    fn marker(&self, player: &Player, dimension: i32, time: u64) -> Vec<[u8; 3]> {
        if self.dimension != dimension as i8 {
            return Vec::new();
        }
        let scale = (1 << self.scale.min(4)) as f32;
        let p = player.precise_position();
        let x = (p[0] - f64::from(self.center[0])) as f32 / scale;
        let z = (p[2] - f64::from(self.center[1])) as f32 / scale;
        let mut px = (f64::from(x * 2.) + 0.5) as i32 as u8;
        let mut pz = (f64::from(z * 2.) + 0.5) as i32 as u8;
        let (kind, rotation) = if x.abs() <= 63. && z.abs() <= 63. {
            let yaw = f64::from(player.java_rotation()[0]);
            let mut rotation = ((yaw + if yaw < 0. { -8. } else { 8. }) * 16. / 360.) as i32 as u8;
            if self.dimension < 0 {
                let t = (time as i64 / 10) as i32;
                rotation = (t
                    .wrapping_mul(t)
                    .wrapping_mul(34187121)
                    .wrapping_add(t.wrapping_mul(121))
                    >> 15) as u8;
            }
            (0, rotation & 15)
        } else if x.abs() < 320. && z.abs() < 320. {
            if x <= -63. {
                px = 128;
            } else if x >= 63. {
                px = 127;
            }
            if z <= -63. {
                pz = 128;
            } else if z >= 63. {
                pz = 127;
            }
            (6, 0)
        } else {
            return Vec::new();
        };
        vec![[kind << 4 | rotation, px, pz]]
    }
    fn explore(&mut self, world: &World, position: [f64; 3], dimension: i32) {
        if self.dimension != dimension as i8 {
            return;
        }
        self.scale = self.scale.min(4);
        self.colors.resize(128 * 128, 0);
        let scale = 1 << self.scale;
        let x = ((position[0] - f64::from(self.center[0])).floor() as i32) / scale + 64;
        let z = ((position[2] - f64::from(self.center[1])).floor() as i32) / scale + 64;
        let radius = 128 / scale / if dimension != 0 { 2 } else { 1 };
        self.step = self.step.wrapping_add(1);
        let mut continue_column = false;
        for px in x - radius + 1..x + radius {
            if px & 15 != self.step as i32 & 15 && !continue_column {
                continue;
            }
            continue_column = false;
            let mut previous_height = 0.;
            for pz in z - radius - 1..z + radius {
                if !(0..128).contains(&px) || !(-1..128).contains(&pz) {
                    continue;
                }
                let dx = px - x;
                let dz = pz - z;
                let rim = dx * dx + dz * dz > (radius - 2) * (radius - 2);
                let wx = (self.center[0] / scale + px - 64) * scale;
                let wz = (self.center[1] / scale + pz - 64) * scale;
                let Some(chunk) = world.chunks.get(&(wx.div_euclid(16), wz.div_euclid(16))) else {
                    continue;
                };
                let mut counts = [0u16; 36];
                let mut height = 0.;
                let mut liquid_depth = 0;
                if dimension != 0 {
                    let n = wx.wrapping_add(wz.wrapping_mul(231871));
                    let color = if n
                        .wrapping_mul(n)
                        .wrapping_mul(31287121)
                        .wrapping_add(n.wrapping_mul(11))
                        >> 20
                        & 1
                        == 0
                    {
                        10
                    } else {
                        11
                    };
                    counts[color] = 100;
                    height = 100.;
                } else {
                    for ox in 0..scale {
                        for oz in 0..scale {
                            let (lx, lz) = ((wx + ox).rem_euclid(16), (wz + oz).rem_euclid(16));
                            let top = (0..chunk.top.min(256))
                                .rev()
                                .find(|&y| {
                                    crate::vanilla::material::light_opacity(chunk.get(lx, y, lz))
                                        > 0
                                })
                                .map_or(0, |y| y + 1);
                            let mut y = top + 1;
                            let mut state = 0;
                            if y > 1 {
                                loop {
                                    y -= 1;
                                    state = if (0..256).contains(&y) {
                                        chunk.get(lx, y, lz)
                                    } else {
                                        0
                                    };
                                    if color(state) != 0 || y <= 0 {
                                        break;
                                    }
                                }
                                if y > 0 && crate::vanilla::material::is_liquid(state) {
                                    let mut below = y - 1;
                                    loop {
                                        let liquid = crate::vanilla::material::is_liquid(
                                            chunk.get(lx, below, lz),
                                        );
                                        below -= 1;
                                        liquid_depth += 1;
                                        if below <= 0 || !liquid {
                                            break;
                                        }
                                    }
                                }
                            }
                            height += f64::from(y) / f64::from(scale * scale);
                            counts[usize::from(color(state))] += 1;
                        }
                    }
                }
                liquid_depth /= scale * scale;
                let slope = (height - previous_height) * 4. / f64::from(scale + 4)
                    + (f64::from((px + pz) & 1) - 0.5) * 0.4;
                let mut shade = if slope > 0.6 {
                    2
                } else if slope < -0.6 {
                    0
                } else {
                    1
                };
                let base = (0..counts.len())
                    .max_by_key(|&i| (counts[i], std::cmp::Reverse(i)))
                    .unwrap();
                if base == 12 {
                    let depth = f64::from(liquid_depth) * 0.1 + f64::from((px + pz) & 1) * 0.2;
                    shade = if depth < 0.5 {
                        2
                    } else if depth > 0.9 {
                        0
                    } else {
                        1
                    };
                }
                previous_height = height;
                if pz >= 0 && dx * dx + dz * dz < radius * radius && (!rim || (px + pz) & 1 != 0) {
                    let value = (base * 4 + shade) as u8;
                    let offset = (px + pz * 128) as usize;
                    if self.colors[offset] != value {
                        self.colors[offset] = value;
                        self.revision = self.revision.wrapping_add(1);
                        continue_column = true;
                    }
                }
            }
        }
    }
}
fn color(state: u16) -> u8 {
    MAP_COLORS.get(usize::from(state)).copied().unwrap_or(0)
}
impl Maps {
    fn allocate(&mut self, position: [f64; 3], scale: u8, dimension: i32) -> u16 {
        while self.data.contains_key(&self.next_id) {
            self.next_id = self.next_id.wrapping_add(1);
        }
        let id = self.next_id;
        self.next_id = id.wrapping_add(1);
        self.data.insert(id, Map::new(position, scale, dimension));
        id
    }
}
impl Simulation {
    pub(super) fn use_empty_map(&mut self, player: &Player, item: &Stack) {
        let id = self
            .maps
            .allocate(player.precise_position(), 0, self.dimension);
        let mut map = Stack::new(358, 1);
        map.meta = id;
        if item.count == 1 {
            self.inventory.slots[self.inventory.selected] = Some(map);
        } else {
            if self.mode != Mode::Creative {
                self.inventory.take_selected(1);
            }
            if let Some(rest) = self.inventory.add(map) {
                self.throw_local_stack(player, rest, ThrowSource::Window);
            }
            let stat = self
                .statistics
                .entry("stat.useItem.minecraft.map".into())
                .or_default();
            *stat = stat.wrapping_add(1);
        }
        self.equipped.reset_progress();
        self.place_ticks = 4;
    }
    pub(super) fn tick_local_maps(&mut self, world: &World, player: &Player) {
        // Original filled maps created by commands recover missing map data at
        // scale 3 around world spawn; ordinary empty maps start at scale 0.
        for slot in 0..36 {
            if let Some(item) = &mut self.inventory.slots[slot] {
                if item.id == 358 && !self.maps.data.contains_key(&item.meta) {
                    item.meta =
                        self.maps
                            .allocate(world.spawn.as_dvec3().to_array(), 3, self.dimension);
                }
            }
        }
        let Some(item) = self.inventory.held().filter(|item| item.id == 358) else {
            self.server_scene.map = None;
            return;
        };
        let map = self.maps.data.get_mut(&item.meta).unwrap();
        map.explore(world, player.precise_position(), self.dimension);
        let icons = map.marker(player, self.dimension, self.clock);
        let old = self.server_scene.map.as_ref();
        let rgba = old
            .filter(|old| old.id == i32::from(item.meta) && old.revision == map.revision)
            .map(|old| old.rgba.clone())
            .unwrap_or_else(|| std::sync::Arc::new(crate::models::server::pixels(&map.colors)));
        self.server_scene.map = Some(crate::models::server::MapSurface {
            id: i32::from(item.meta),
            revision: map.revision,
            rgba,
            icons,
        });
    }
}

#[cfg(test)]
#[path = "map_tests.rs"]
mod tests;
