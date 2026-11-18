//! Native default-overworld GenLayer graph from 1.8.9. All layer arithmetic,
//! seed overflow, random call order, and Voronoi tie-breaking are preserved.
use super::biome_data::BIOMES;

#[derive(Debug)]
pub struct Biome {
    pub id: u8,
    pub name: &'static str,
    pub class: &'static str,
    pub base_height: f32,
    pub height_variation: f32,
    pub temperature: f32,
    pub rainfall: f32,
    pub snowy: bool,
    pub mesa: bool,
    pub temp_category: u8,
    pub top: u16,
    pub filler: u16,
}
pub fn get_biome(id: i32) -> Option<&'static Biome> {
    BIOMES.iter().find(|b| b.id as i32 == id)
}
pub fn biome(id: u8) -> &'static Biome {
    get_biome(id as i32).expect("Unregistered original biome ID")
}
fn ocean(id: i32) -> bool {
    matches!(id, 0 | 10 | 24)
}
fn equal(a: i32, b: i32) -> bool {
    if a == b {
        return true;
    }
    if a == 38 || a == 39 {
        return b == 38 || b == 39;
    }
    match (get_biome(a), get_biome(b)) {
        (Some(a), Some(b)) => a.class == b.class,
        _ => false,
    }
}
fn neighbors_compatible(a: i32, b: i32) -> bool {
    if equal(a, b) {
        true
    } else {
        match (get_biome(a), get_biome(b)) {
            (Some(a), Some(b)) => {
                a.temp_category == b.temp_category || a.temp_category == 2 || b.temp_category == 2
            }
            _ => false,
        }
    }
}
fn jungle(id: i32) -> bool {
    get_biome(id).is_some_and(|b| b.class == "BiomeGenJungle")
}
fn jungle_neighbor(id: i32) -> bool {
    jungle(id) || matches!(id, 21 | 22 | 23 | 4 | 5) || ocean(id)
}
fn mesa(id: i32) -> bool {
    get_biome(id).is_some_and(|b| b.mesa)
}
fn step(seed: i64, add: i64) -> i64 {
    seed.wrapping_mul(
        seed.wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407),
    )
    .wrapping_add(add)
}
#[derive(Clone, Copy)]
struct LayerRandom {
    world: i64,
    state: i64,
}
impl LayerRandom {
    fn new(world: i64, x: i32, z: i32) -> Self {
        let mut state = world;
        for value in [x, z, x, z] {
            state = step(state, value as i64);
        }
        Self { world, state }
    }
    fn next(&mut self, bound: i32) -> i32 {
        let mut result = ((self.state >> 24) % bound as i64) as i32;
        if result < 0 {
            result += bound;
        }
        self.state = step(self.state, self.world);
        result
    }
    fn choose(&mut self, values: &[i32]) -> i32 {
        values[self.next(values.len() as i32) as usize]
    }
    fn mode(&mut self, a: i32, b: i32, c: i32, d: i32) -> i32 {
        if b == c && c == d {
            b
        } else if (a == b && a == c)
            || (a == b && a == d)
            || (a == c && a == d)
            || (a == b && c != d)
            || (a == c && b != d)
            || (a == d && b != c)
        {
            a
        } else if (b == c && a != d) || (b == d && a != c) {
            b
        } else if c == d && a != b {
            c
        } else {
            self.choose(&[a, b, c, d])
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Island,
    Zoom,
    Fuzzy,
    AddIsland,
    RemoveOcean,
    Snow,
    CoolWarm,
    HeatIce,
    Special,
    Mushroom,
    DeepOcean,
    Biome,
    BiomeEdge,
    RiverInit,
    Hills,
    River,
    Rare,
    Shore,
    Smooth,
    Mix,
    Voronoi,
}
struct Node {
    kind: Kind,
    base: i64,
    world: i64,
    parent: Option<usize>,
    extra: Option<usize>,
}
pub struct BiomeGenerator {
    fixed_biome: Option<i32>,
    nodes: Vec<Node>,
    generation: usize,
    voronoi: usize,
}
impl BiomeGenerator {
    pub fn are_biomes_viable(&self, x: i32, z: i32, radius: i32, allowed: &[u8]) -> bool {
        let min_x = x.wrapping_sub(radius) >> 2;
        let min_z = z.wrapping_sub(radius) >> 2;
        let max_x = x.wrapping_add(radius) >> 2;
        let max_z = z.wrapping_add(radius) >> 2;
        self.get_biomes_for_generation(
            min_x,
            min_z,
            (max_x - min_x + 1) as usize,
            (max_z - min_z + 1) as usize,
        )
        .iter()
        .all(|b| allowed.contains(b))
    }
    fn node(
        &mut self,
        kind: Kind,
        salt: i64,
        parent: Option<usize>,
        extra: Option<usize>,
    ) -> usize {
        let mut base = salt;
        for _ in 0..3 {
            base = step(base, salt);
        }
        let index = self.nodes.len();
        self.nodes.push(Node {
            kind,
            base,
            world: 0,
            parent,
            extra,
        });
        index
    }
    fn unary(&mut self, kind: Kind, salt: i64, parent: usize) -> usize {
        self.node(kind, salt, Some(parent), None)
    }
    fn zoom(&mut self, mut node: usize, count: usize) -> usize {
        for i in 0..count {
            node = self.unary(Kind::Zoom, 1000 + i as i64, node);
        }
        node
    }
    fn seed(&mut self, index: usize, seed: i64) {
        if let Some(parent) = self.nodes[index].parent {
            self.seed(parent, seed);
        }
        // Hills does NOT initialize its second parent's seed in original 1.8.9.
        // The shared RiverInit gets initialized through Mix, but the two Hills
        // noise Zoom nodes retain worldGenSeed=0. Seeding all nodes changes maps.
        if self.nodes[index].kind == Kind::Mix {
            self.seed(self.nodes[index].extra.unwrap(), seed);
        }
        let mut world = seed;
        for _ in 0..3 {
            world = step(world, self.nodes[index].base);
        }
        self.nodes[index].world = world;
    }
    pub fn new(seed: i64) -> Self {
        Self::configured(seed, 4, 4, None)
    }
    pub fn configured(
        seed: i64,
        biome_size: usize,
        river_size: usize,
        fixed_biome: Option<i32>,
    ) -> Self {
        let mut result = Self {
            fixed_biome,
            nodes: Vec::new(),
            generation: 0,
            voronoi: 0,
        };
        let mut land = result.node(Kind::Island, 1, None, None);
        for (kind, salt) in [
            (Kind::Fuzzy, 2000),
            (Kind::AddIsland, 1),
            (Kind::Zoom, 2001),
            (Kind::AddIsland, 2),
            (Kind::AddIsland, 50),
            (Kind::AddIsland, 70),
            (Kind::RemoveOcean, 2),
            (Kind::Snow, 2),
            (Kind::AddIsland, 3),
            (Kind::CoolWarm, 2),
            (Kind::HeatIce, 2),
            (Kind::Special, 3),
            (Kind::Zoom, 2002),
            (Kind::Zoom, 2003),
            (Kind::AddIsland, 4),
            (Kind::Mushroom, 5),
            (Kind::DeepOcean, 4),
        ] {
            land = result.unary(kind, salt, land);
        }
        let river_init = result.unary(Kind::RiverInit, 100, land);
        let mut biomes = result.unary(Kind::Biome, 200, land);
        biomes = result.zoom(biomes, 2);
        biomes = result.unary(Kind::BiomeEdge, 1000, biomes);
        let hills_noise = result.zoom(river_init, 2);
        biomes = result.node(Kind::Hills, 1000, Some(biomes), Some(hills_noise));
        let mut river = result.zoom(river_init, 2);
        river = result.zoom(river, river_size);
        river = result.unary(Kind::River, 1, river);
        river = result.unary(Kind::Smooth, 1000, river);
        biomes = result.unary(Kind::Rare, 1001, biomes);
        for i in 0..biome_size {
            biomes = result.unary(Kind::Zoom, 1000 + i as i64, biomes);
            if i == 0 {
                biomes = result.unary(Kind::AddIsland, 3, biomes);
            }
            if i == 1 || biome_size == 1 {
                biomes = result.unary(Kind::Shore, 1000, biomes);
            }
        }
        biomes = result.unary(Kind::Smooth, 1000, biomes);
        result.generation = result.node(Kind::Mix, 100, Some(biomes), Some(river));
        result.voronoi = result.unary(Kind::Voronoi, 10, result.generation);
        result.seed(result.generation, seed);
        result.seed(result.voronoi, seed);
        result
    }
    /// Quart-resolution IDs consumed by the terrain density biome weighting.
    pub fn get_biomes_for_generation(
        &self,
        x: i32,
        z: i32,
        width: usize,
        height: usize,
    ) -> Vec<u8> {
        self.checked(self.generation, x, z, width, height)
    }
    /// Block-resolution IDs consumed by surface replacement and chunk Biomes.
    pub fn get_biomes(&self, x: i32, z: i32, width: usize, height: usize) -> Vec<u8> {
        self.checked(self.voronoi, x, z, width, height)
    }
    /// WorldChunkManager.findBiomePosition. Its counter increases only when
    /// a candidate replaces the selected entry, including the first one.
    pub fn find_biome_position(
        &self,
        x: i32,
        z: i32,
        radius: i32,
        random: &mut super::random::JavaRandom,
        accepted: impl Fn(u8) -> bool,
    ) -> Option<[i32; 2]> {
        let (min_x, min_z) = (x.wrapping_sub(radius) >> 2, z.wrapping_sub(radius) >> 2);
        let width = (x.wrapping_add(radius) >> 2) - min_x + 1;
        let height = (z.wrapping_add(radius) >> 2) - min_z + 1;
        let mut selected = None;
        let mut replacements = 0;
        for (i, id) in self
            .get_biomes_for_generation(min_x, min_z, width as usize, height as usize)
            .into_iter()
            .enumerate()
        {
            if accepted(id) && (selected.is_none() || random.next_int(replacements + 1) == 0) {
                selected = Some([
                    (min_x + i as i32 % width) << 2,
                    (min_z + i as i32 / width) << 2,
                ]);
                replacements += 1;
            }
        }
        selected
    }
    fn checked(&self, node: usize, x: i32, z: i32, w: usize, h: usize) -> Vec<u8> {
        assert!(w > 0 && h > 0 && w <= 4096 && h <= 4096 && w * h <= 4_194_304);
        self.ints(node, x, z, w, h)
            .into_iter()
            .map(|v| u8::try_from(v).expect("Final biome layer ID"))
            .collect()
    }
    fn ints(&self, index: usize, x: i32, z: i32, w: usize, h: usize) -> Vec<i32> {
        let node = &self.nodes[index];
        let mut out = vec![0; w * h];
        let rng = |px: i32, pz: i32| LayerRandom::new(node.world, px, pz);
        if node.kind == Kind::Island {
            for dz in 0..h {
                for dx in 0..w {
                    out[dx + dz * w] = if rng(x.wrapping_add(dx as i32), z.wrapping_add(dz as i32))
                        .next(10)
                        == 0
                    {
                        1
                    } else {
                        0
                    };
                }
            }
            if x > -(w as i32) && x <= 0 && z > -(h as i32) && z <= 0 {
                out[(-x) as usize + (-z) as usize * w] = 1;
            }
            return out;
        }
        let parent = node.parent.unwrap();
        if matches!(node.kind, Kind::Zoom | Kind::Fuzzy) {
            let px = x >> 1;
            let pz = z >> 1;
            let pw = (w >> 1) + 2;
            let ph = (h >> 1) + 2;
            let data = self.ints(parent, px, pz, pw, ph);
            let sw = (pw - 1) * 2;
            let mut zoom = vec![0; sw * (ph - 1) * 2];
            for dz in 0..ph - 1 {
                let mut a = data[dz * pw];
                let mut c = data[(dz + 1) * pw];
                for dx in 0..pw - 1 {
                    let mut r = rng(
                        (px.wrapping_add(dx as i32)).wrapping_shl(1),
                        (pz.wrapping_add(dz as i32)).wrapping_shl(1),
                    );
                    let b = data[dx + 1 + dz * pw];
                    let d = data[dx + 1 + (dz + 1) * pw];
                    let i = dx * 2 + dz * 2 * sw;
                    zoom[i] = a;
                    zoom[i + sw] = r.choose(&[a, c]);
                    zoom[i + 1] = r.choose(&[a, b]);
                    zoom[i + 1 + sw] = if node.kind == Kind::Fuzzy {
                        r.choose(&[a, b, c, d])
                    } else {
                        r.mode(a, b, c, d)
                    };
                    a = b;
                    c = d;
                }
            }
            for dz in 0..h {
                let start = (dz + (z & 1) as usize) * sw + (x & 1) as usize;
                out[dz * w..(dz + 1) * w].copy_from_slice(&zoom[start..start + w]);
            }
            return out;
        }
        if node.kind == Kind::Voronoi {
            return self.voronoi_ints(index, x, z, w, h);
        }
        if node.kind == Kind::Mix {
            let data = self.ints(parent, x, z, w, h);
            let river = self.ints(node.extra.unwrap(), x, z, w, h);
            for i in 0..out.len() {
                out[i] = if matches!(data[i], 0 | 24) {
                    data[i]
                } else if river[i] == 7 {
                    match data[i] {
                        12 => 11,
                        14 | 15 => 15,
                        _ => river[i] & 255,
                    }
                } else {
                    data[i]
                };
            }
            return out;
        }
        if matches!(node.kind, Kind::Special | Kind::Biome | Kind::RiverInit) {
            let data = self.ints(parent, x, z, w, h);
            for dz in 0..h {
                for dx in 0..w {
                    let i = dx + dz * w;
                    let mut r = rng(x.wrapping_add(dx as i32), z.wrapping_add(dz as i32));
                    let value = data[i];
                    out[i] = match node.kind {
                        Kind::Special => {
                            if value != 0 && r.next(13) == 0 {
                                value | ((1 + r.next(15)) << 8 & 0xf00)
                            } else {
                                value
                            }
                        }
                        Kind::RiverInit => {
                            if value > 0 {
                                r.next(299999) + 2
                            } else {
                                0
                            }
                        }
                        Kind::Biome if self.fixed_biome.is_some() => self.fixed_biome.unwrap(),
                        Kind::Biome => {
                            let special = (value & 0xf00) >> 8;
                            let value = value & !0xf00;
                            if ocean(value) || value == 14 {
                                value
                            } else {
                                match value {
                                    1 => {
                                        if special > 0 {
                                            if r.next(3) == 0 {
                                                39
                                            } else {
                                                38
                                            }
                                        } else {
                                            r.choose(&[2, 2, 2, 35, 35, 1])
                                        }
                                    }
                                    2 => {
                                        if special > 0 {
                                            21
                                        } else {
                                            r.choose(&[4, 29, 3, 1, 27, 6])
                                        }
                                    }
                                    3 => {
                                        if special > 0 {
                                            32
                                        } else {
                                            r.choose(&[4, 3, 5, 1])
                                        }
                                    }
                                    4 => r.choose(&[12, 12, 12, 30]),
                                    _ => 14,
                                }
                            }
                        }
                        _ => unreachable!(),
                    };
                }
            }
            return out;
        }
        let pw = w + 2;
        let data = self.ints(parent, x.wrapping_sub(1), z.wrapping_sub(1), pw, h + 2);
        let extra = if node.kind == Kind::Hills {
            self.ints(
                node.extra.unwrap(),
                x.wrapping_sub(1),
                z.wrapping_sub(1),
                pw,
                h + 2,
            )
        } else {
            Vec::new()
        };
        for dz in 0..h {
            for dx in 0..w {
                let i = dx + 1 + (dz + 1) * pw;
                let center = data[i];
                let n = [data[i - pw], data[i + 1], data[i - 1], data[i + pw]];
                let corners = [
                    data[i - pw - 1],
                    data[i - pw + 1],
                    data[i + pw - 1],
                    data[i + pw + 1],
                ];
                let mut r = rng(x.wrapping_add(dx as i32), z.wrapping_add(dz as i32));
                out[dx + dz * w] = match node.kind {
                    Kind::AddIsland => {
                        if center == 0 && corners.iter().any(|&v| v != 0) {
                            let mut count = 1;
                            let mut selected = 1;
                            for v in corners {
                                if v != 0 {
                                    if r.next(count) == 0 {
                                        selected = v;
                                    }
                                    count += 1;
                                }
                            }
                            if r.next(3) == 0 {
                                selected
                            } else if selected == 4 {
                                4
                            } else {
                                0
                            }
                        } else if center > 0 && corners.contains(&0) && r.next(5) == 0 {
                            if center == 4 {
                                4
                            } else {
                                0
                            }
                        } else {
                            center
                        }
                    }
                    Kind::RemoveOcean => {
                        if center == 0 && n == [0; 4] && r.next(2) == 0 {
                            1
                        } else {
                            center
                        }
                    }
                    Kind::Snow => {
                        if center == 0 {
                            0
                        } else {
                            match r.next(6) {
                                0 => 4,
                                1 => 3,
                                _ => 1,
                            }
                        }
                    }
                    Kind::CoolWarm => {
                        if center == 1 && n.iter().any(|&v| v == 3 || v == 4) {
                            2
                        } else {
                            center
                        }
                    }
                    Kind::HeatIce => {
                        if center == 4 && n.iter().any(|&v| v == 1 || v == 2) {
                            3
                        } else {
                            center
                        }
                    }
                    Kind::Mushroom => {
                        if center == 0 && corners == [0; 4] && r.next(100) == 0 {
                            14
                        } else {
                            center
                        }
                    }
                    Kind::DeepOcean => {
                        if center == 0 && n == [0; 4] {
                            24
                        } else {
                            center
                        }
                    }
                    Kind::BiomeEdge => biome_edge(center, n),
                    Kind::Hills => hills(center, extra[i], n, &mut r),
                    Kind::River => {
                        let normalize = |v| if v >= 2 { 2 + (v & 1) } else { v };
                        let c = normalize(center);
                        if n.iter().any(|&v| normalize(v) != c) {
                            7
                        } else {
                            -1
                        }
                    }
                    Kind::Rare => {
                        if r.next(57) == 0 && center == 1 {
                            129
                        } else {
                            center
                        }
                    }
                    Kind::Shore => shore(center, n),
                    Kind::Smooth => {
                        let [north, east, west, south] = n;
                        if west == east && north == south {
                            if r.next(2) == 0 {
                                west
                            } else {
                                north
                            }
                        } else if north == south {
                            north
                        } else if west == east {
                            west
                        } else {
                            center
                        }
                    }
                    _ => unreachable!(),
                };
            }
        }
        out
    }
    fn voronoi_ints(&self, index: usize, x: i32, z: i32, w: usize, h: usize) -> Vec<i32> {
        let x = x.wrapping_sub(2);
        let z = z.wrapping_sub(2);
        let node = &self.nodes[index];
        let px = x >> 2;
        let pz = z >> 2;
        let pw = (w >> 2) + 2;
        let ph = (h >> 2) + 2;
        let data = self.ints(node.parent.unwrap(), px, pz, pw, ph);
        let sw = (pw - 1) * 4;
        let mut zoom = vec![0; sw * (ph - 1) * 4];
        for dz in 0..ph - 1 {
            let mut a = data[dz * pw];
            let mut c = data[(dz + 1) * pw];
            for dx in 0..pw - 1 {
                let mut offsets = [[0.; 2]; 4];
                for (k, [ox, oz]) in [[0, 0], [1, 0], [0, 1], [1, 1]].into_iter().enumerate() {
                    let mut r = LayerRandom::new(
                        node.world,
                        (px.wrapping_add(dx as i32).wrapping_add(ox)).wrapping_shl(2),
                        (pz.wrapping_add(dz as i32).wrapping_add(oz)).wrapping_shl(2),
                    );
                    offsets[k] = [
                        (r.next(1024) as f64 / 1024.0 - 0.5) * 3.6 + ox as f64 * 4.0,
                        (r.next(1024) as f64 / 1024.0 - 0.5) * 3.6 + oz as f64 * 4.0,
                    ];
                }
                let b = data[dx + 1 + dz * pw] & 255;
                let d = data[dx + 1 + (dz + 1) * pw] & 255;
                for sub_z in 0..4 {
                    for sub_x in 0..4 {
                        let distances = offsets.map(|[ox, oz]| {
                            (sub_z as f64 - oz) * (sub_z as f64 - oz)
                                + (sub_x as f64 - ox) * (sub_x as f64 - ox)
                        });
                        let [da, db, dc, dd] = distances;
                        zoom[(dz * 4 + sub_z) * sw + dx * 4 + sub_x] =
                            if da < db && da < dc && da < dd {
                                a
                            } else if db < da && db < dc && db < dd {
                                b
                            } else if dc < da && dc < db && dc < dd {
                                c
                            } else {
                                d
                            };
                    }
                }
                a = b;
                c = d;
            }
        }
        let mut out = vec![0; w * h];
        for dz in 0..h {
            let start = (dz + (z & 3) as usize) * sw + (x & 3) as usize;
            out[dz * w..(dz + 1) * w].copy_from_slice(&zoom[start..start + w]);
        }
        out
    }
}
fn biome_edge(center: i32, n: [i32; 4]) -> i32 {
    if equal(center, 3) {
        return if n.iter().all(|&v| neighbors_compatible(v, 3)) {
            center
        } else {
            20
        };
    }
    for (from, to) in [(38, 37), (39, 37), (32, 5)] {
        if center == from {
            return if n.iter().all(|&v| equal(v, from)) {
                center
            } else {
                to
            };
        }
    }
    if center == 2 && n.contains(&12) {
        34
    } else if center == 6 {
        if n.iter().any(|v| matches!(v, 2 | 30 | 12)) {
            1
        } else if n.contains(&21) {
            23
        } else {
            center
        }
    } else {
        center
    }
}
fn hills(center: i32, noise: i32, n: [i32; 4], r: &mut LayerRandom) -> i32 {
    let mutate = (noise - 2) % 29 == 0;
    if center != 0 && noise >= 2 && (noise - 2) % 29 == 1 && center < 128 {
        return if get_biome(center + 128).is_some() {
            center + 128
        } else {
            center
        };
    }
    if r.next(3) != 0 && !mutate {
        return center;
    }
    let mut hill = match center {
        2 => 17,
        4 => 18,
        27 => 28,
        29 => 1,
        5 => 19,
        32 => 33,
        30 => 31,
        1 => {
            if r.next(3) == 0 {
                18
            } else {
                4
            }
        }
        12 => 13,
        21 => 22,
        0 => 24,
        3 => 34,
        35 => 36,
        _ => {
            if equal(center, 38) {
                37
            } else if center == 24 && r.next(3) == 0 {
                if r.next(2) == 0 {
                    1
                } else {
                    4
                }
            } else {
                center
            }
        }
    };
    if mutate && hill != center {
        hill = if get_biome(hill + 128).is_some() {
            hill + 128
        } else {
            center
        };
    }
    if hill != center && n.iter().filter(|&&v| equal(v, center)).count() >= 3 {
        hill
    } else {
        center
    }
}
fn shore(center: i32, n: [i32; 4]) -> i32 {
    let touches_ocean = n.iter().any(|&v| ocean(v));
    if center == 14 {
        if n.contains(&0) {
            15
        } else {
            center
        }
    } else if jungle(center) {
        if !n.iter().all(|&v| jungle_neighbor(v)) {
            23
        } else if touches_ocean {
            16
        } else {
            center
        }
    } else if matches!(center, 3 | 34 | 20) {
        if !ocean(center) && touches_ocean {
            25
        } else {
            center
        }
    } else if get_biome(center).is_some_and(|b| b.snowy) {
        if !ocean(center) && touches_ocean {
            26
        } else {
            center
        }
    } else if matches!(center, 37 | 38) {
        if touches_ocean || n.iter().all(|&v| mesa(v)) {
            center
        } else {
            2
        }
    } else if !matches!(center, 0 | 24 | 7 | 6) && touches_ocean {
        16
    } else {
        center
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_biome_pipeline_matches_original_java_every_cell() {
        let corpus: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/vanilla189-biomes.json"))
                .unwrap();
        let mut compared = 0;
        for row in corpus["cases"].as_array().unwrap() {
            let seed = row["seed"].as_str().unwrap().parse().unwrap();
            let generator = BiomeGenerator::new(seed);
            let region = &row["region"];
            let x = region[0].as_i64().unwrap() as i32;
            let z = region[1].as_i64().unwrap() as i32;
            let w = region[2].as_u64().unwrap() as usize;
            let h = region[3].as_u64().unwrap() as usize;
            let layer = row["layer"].as_u64().unwrap();
            let actual = if layer == 0 {
                generator.get_biomes_for_generation(x, z, w, h)
            } else {
                generator.get_biomes(x, z, w, h)
            };
            for (i, (a, b)) in actual
                .iter()
                .zip(row["values"].as_array().unwrap())
                .enumerate()
            {
                assert_eq!(
                    *a as i64,
                    b.as_i64().unwrap(),
                    "seed={seed}, layer={layer}, origin=({x},{z}), cell=({},{})",
                    i % w,
                    i / w
                );
                compared += 1;
            }
        }
        assert_eq!(compared, 1_233_582);
    }
}
