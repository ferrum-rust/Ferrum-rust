//! Original tree/plant feature placement. Biome selection is a separate stage.
use super::{solid, FeatureWorld, HORIZONTAL};
use crate::vanilla::random::JavaRandom;
use glam::IVec3;
use serde::{Deserialize, Serialize};

fn leaves_or_air(state: u16) -> bool {
    matches!(state >> 4, 0 | 18 | 161)
}
fn tree_replaceable(state: u16) -> bool {
    matches!(state >> 4, 0 | 2 | 3 | 6 | 17 | 18 | 106 | 161 | 162)
}
fn soil(state: u16) -> bool {
    matches!(state >> 4, 2 | 3 | 60)
}
fn offset(random: &mut JavaRandom) -> IVec3 {
    IVec3::new(
        random.next_int(8) - random.next_int(8),
        random.next_int(4) - random.next_int(4),
        random.next_int(8) - random.next_int(8),
    )
}
fn vine(world: &mut impl FeatureWorld, p: IVec3, metadata: u16, hanging: bool) {
    world.set(p, (106 << 4) | metadata);
    if hanging {
        for down in 1..=4 {
            let q = p - IVec3::Y * down;
            if world.get(q) != 0 {
                break;
            }
            world.set(q, (106 << 4) | metadata);
        }
    }
}

/// WorldGenTrees: oak or small jungle, including hanging vines and cocoa.
pub fn tree(
    world: &mut impl FeatureWorld,
    random: &mut JavaRandom,
    origin: IVec3,
    min_height: i32,
    wood: u16,
    leaf: u16,
    vines: bool,
) -> bool {
    let height = random.next_int(3) + min_height;
    rounded_tree(world, random, origin, height, (wood, leaf, vines, true))
}

pub fn birch(
    world: &mut impl FeatureWorld,
    random: &mut JavaRandom,
    origin: IVec3,
    extra_height: bool,
) -> bool {
    let mut height = random.next_int(3) + 5;
    if extra_height {
        height += random.next_int(7);
    }
    rounded_tree(
        world,
        random,
        origin,
        height,
        ((17 << 4) | 2, (18 << 4) | 2, false, false),
    )
}

fn rounded_tree(
    world: &mut impl FeatureWorld,
    random: &mut JavaRandom,
    origin: IVec3,
    height: i32,
    config: (u16, u16, bool, bool),
) -> bool {
    let (wood, leaf, vines, replace_vines) = config;
    let canopy_replaceable =
        |state: u16| leaves_or_air(state) || replace_vines && crate::vanilla::material::is_vine(state);
    if origin.y < 1 || origin.y + height + 1 > 256 {
        return false;
    }
    for y in origin.y..=origin.y + 1 + height {
        let radius = if y >= origin.y + height - 1 {
            2
        } else if y == origin.y {
            0
        } else {
            1
        };
        for x in origin.x - radius..=origin.x + radius {
            for z in origin.z - radius..=origin.z + radius {
                if !(0..256).contains(&y) || !tree_replaceable(world.get(IVec3::new(x, y, z))) {
                    return false;
                }
            }
        }
    }
    if !soil(world.get(origin - IVec3::Y)) || origin.y >= 256 - height - 1 {
        return false;
    }
    if world.get(origin - IVec3::Y) >> 4 != 3 {
        world.set(origin - IVec3::Y, 3 << 4);
    }
    for y in origin.y - 3 + height..=origin.y + height {
        let relative = y - (origin.y + height);
        let radius = 1 - relative / 2;
        for x in origin.x - radius..=origin.x + radius {
            for z in origin.z - radius..=origin.z + radius {
                if (x - origin.x).abs() == radius
                    && (z - origin.z).abs() == radius
                    && (random.next_int(2) == 0 || relative == 0)
                {
                    continue;
                }
                let p = IVec3::new(x, y, z);
                if canopy_replaceable(world.get(p)) {
                    world.set(p, leaf);
                }
            }
        }
    }
    let sides = [
        (IVec3::NEG_X, 8),
        (IVec3::X, 2),
        (IVec3::NEG_Z, 1),
        (IVec3::Z, 4),
    ];
    for y in 0..height {
        let p = origin + IVec3::Y * y;
        if !canopy_replaceable(world.get(p)) {
            continue;
        }
        world.set(p, wood);
        if vines && y > 0 {
            for (direction, meta) in sides {
                if random.next_int(3) > 0 && world.get(p + direction) == 0 {
                    vine(world, p + direction, meta, false);
                }
            }
        }
    }
    if vines {
        for y in origin.y - 3 + height..=origin.y + height {
            let radius = 2 - (y - (origin.y + height)) / 2;
            for x in origin.x - radius..=origin.x + radius {
                for z in origin.z - radius..=origin.z + radius {
                    let p = IVec3::new(x, y, z);
                    if !matches!(world.get(p) >> 4, 18 | 161) {
                        continue;
                    }
                    for (direction, meta) in sides {
                        if random.next_int(4) == 0 && world.get(p + direction) == 0 {
                            vine(world, p + direction, meta, true);
                        }
                    }
                }
            }
        }
        if random.next_int(5) == 0 && height > 5 {
            for level in 0..2 {
                for (index, direction) in HORIZONTAL.iter().enumerate() {
                    if random.next_int(4 - level) == 0 {
                        let age = random.next_int(3);
                        world.set(
                            origin - *direction + IVec3::Y * (height - 5 + level),
                            (127 << 4) | ((age as u16) << 2) | [2, 3, 0, 1][index],
                        );
                    }
                }
            }
        }
    }
    true
}

/// WorldGenTaiga2: the common conical spruce tree, including trunk truncation.
pub fn spruce(world: &mut impl FeatureWorld, random: &mut JavaRandom, origin: IVec3) -> bool {
    let height = random.next_int(4) + 6;
    let bare = 1 + random.next_int(2);
    let foliage = height - bare;
    let max_radius = 2 + random.next_int(2);
    if origin.y < 1 || origin.y + height + 1 > 256 {
        return false;
    }
    for y in origin.y..=origin.y + 1 + height {
        let radius = if y - origin.y < bare { 0 } else { max_radius };
        for x in origin.x - radius..=origin.x + radius {
            for z in origin.z - radius..=origin.z + radius {
                if !(0..256).contains(&y) || !leaves_or_air(world.get(IVec3::new(x, y, z))) {
                    return false;
                }
            }
        }
    }
    if !soil(world.get(origin - IVec3::Y)) || origin.y >= 256 - height - 1 {
        return false;
    }
    if world.get(origin - IVec3::Y) >> 4 != 3 {
        world.set(origin - IVec3::Y, 3 << 4);
    }
    let mut radius = random.next_int(2);
    let mut target = 1;
    let mut reset = 0;
    for layer in 0..=foliage {
        let y = origin.y + height - layer;
        for x in origin.x - radius..=origin.x + radius {
            for z in origin.z - radius..=origin.z + radius {
                let p = IVec3::new(x, y, z);
                if (x - origin.x).abs() == radius && (z - origin.z).abs() == radius && radius > 0
                    || super::full(world.get(p))
                {
                    continue;
                }
                world.set(p, (18 << 4) | 1);
            }
        }
        if radius >= target {
            radius = reset;
            reset = 1;
            target = (target + 1).min(max_radius);
        } else {
            radius += 1;
        }
    }
    let trim = random.next_int(3);
    for y in 0..height - trim {
        let p = origin + IVec3::Y * y;
        if leaves_or_air(world.get(p)) {
            world.set(p, (17 << 4) | 1);
        }
    }
    true
}

/// WorldGenTaiga1, the tall narrow pine.
pub fn pine(world: &mut impl FeatureWorld, random: &mut JavaRandom, origin: IVec3) -> bool {
    let height = random.next_int(5) + 7;
    let bare = height - random.next_int(2) - 3;
    let foliage = height - bare;
    let max_radius = 1 + random.next_int(foliage + 1);
    if origin.y < 1 || origin.y + height + 1 > 256 {
        return false;
    }
    for y in origin.y..=origin.y + 1 + height {
        let radius = if y - origin.y < bare { 0 } else { max_radius };
        for x in origin.x - radius..=origin.x + radius {
            for z in origin.z - radius..=origin.z + radius {
                if !(0..256).contains(&y) || !tree_replaceable(world.get(IVec3::new(x, y, z))) {
                    return false;
                }
            }
        }
    }
    if !matches!(world.get(origin - IVec3::Y) >> 4, 2 | 3) || origin.y >= 256 - height - 1 {
        return false;
    }
    if world.get(origin - IVec3::Y) >> 4 != 3 {
        world.set(origin - IVec3::Y, 3 << 4);
    }
    let mut radius = 0;
    for y in (origin.y + bare..=origin.y + height).rev() {
        for x in origin.x - radius..=origin.x + radius {
            for z in origin.z - radius..=origin.z + radius {
                let p = IVec3::new(x, y, z);
                if (x - origin.x).abs() == radius && (z - origin.z).abs() == radius && radius > 0
                    || super::full(world.get(p))
                {
                    continue;
                }
                world.set(p, (18 << 4) | 1);
            }
        }
        if radius >= 1 && y == origin.y + bare + 1 {
            radius -= 1;
        } else if radius < max_radius {
            radius += 1;
        }
    }
    for y in 0..height - 1 {
        let p = origin + IVec3::Y * y;
        if leaves_or_air(world.get(p)) {
            world.set(p, (17 << 4) | 1);
        }
    }
    true
}

pub fn acacia(world: &mut impl FeatureWorld, random: &mut JavaRandom, origin: IVec3) -> bool {
    let height = random.next_int(3) + random.next_int(3) + 5;
    if origin.y < 1 || origin.y + height + 1 > 256 {
        return false;
    }
    for y in origin.y..=origin.y + 1 + height {
        let radius = if y >= origin.y + height - 1 {
            2
        } else if y == origin.y {
            0
        } else {
            1
        };
        for x in origin.x - radius..=origin.x + radius {
            for z in origin.z - radius..=origin.z + radius {
                if !(0..256).contains(&y) || !tree_replaceable(world.get(IVec3::new(x, y, z))) {
                    return false;
                }
            }
        }
    }
    if !matches!(world.get(origin - IVec3::Y) >> 4, 2 | 3) || origin.y >= 256 - height - 1 {
        return false;
    }
    if world.get(origin - IVec3::Y) >> 4 != 3 {
        world.set(origin - IVec3::Y, 3 << 4);
    }
    let direction = HORIZONTAL[random.next_int(4) as usize];
    let bend = height - random.next_int(4) - 1;
    let mut steps = 3 - random.next_int(3);
    let mut horizontal = IVec3::new(origin.x, 0, origin.z);
    let mut top = 0;
    for y in 0..height {
        if y >= bend && steps > 0 {
            horizontal += direction;
            steps -= 1;
        }
        let p = horizontal + IVec3::Y * (origin.y + y);
        if leaves_or_air(world.get(p)) {
            world.set(p, 162 << 4);
            top = p.y;
        }
    }
    fn leaf(world: &mut impl FeatureWorld, p: IVec3) {
        if leaves_or_air(world.get(p)) {
            world.set(p, 161 << 4);
        }
    }
    let mut crown = horizontal + IVec3::Y * top;
    for x in -3i32..=3 {
        for z in -3i32..=3 {
            if x.abs() != 3 || z.abs() != 3 {
                leaf(world, crown + IVec3::new(x, 0, z));
            }
        }
    }
    crown += IVec3::Y;
    for x in -1..=1 {
        for z in -1..=1 {
            leaf(world, crown + IVec3::new(x, 0, z));
        }
    }
    for d in [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z] {
        leaf(world, crown + d * 2);
    }
    horizontal = IVec3::new(origin.x, 0, origin.z);
    let other = HORIZONTAL[random.next_int(4) as usize];
    if other != direction {
        let start = bend - random.next_int(2) - 1;
        let count = 1 + random.next_int(3);
        top = 0;
        for y in start..height.min(start + count) {
            if y < 1 {
                continue;
            }
            horizontal += other;
            let p = horizontal + IVec3::Y * (origin.y + y);
            if leaves_or_air(world.get(p)) {
                world.set(p, 162 << 4);
                top = p.y;
            }
        }
        if top > 0 {
            crown = horizontal + IVec3::Y * top;
            for x in -2i32..=2 {
                for z in -2i32..=2 {
                    if x.abs() != 2 || z.abs() != 2 {
                        leaf(world, crown + IVec3::new(x, 0, z));
                    }
                }
            }
            crown += IVec3::Y;
            for x in -1..=1 {
                for z in -1..=1 {
                    leaf(world, crown + IVec3::new(x, 0, z));
                }
            }
        }
    }
    true
}

pub fn desert_well(world: &mut impl FeatureWorld, mut origin: IVec3) -> bool {
    while world.get(origin) == 0 && origin.y > 2 {
        origin.y -= 1;
    }
    if world.get(origin) != (12 << 4) {
        return false;
    }
    for x in -2..=2 {
        for z in -2..=2 {
            if world.get(origin + IVec3::new(x, -1, z)) == 0
                && world.get(origin + IVec3::new(x, -2, z)) == 0
            {
                return false;
            }
        }
    }
    for y in -1..=0 {
        for x in -2..=2 {
            for z in -2..=2 {
                world.set(origin + IVec3::new(x, y, z), 24 << 4);
            }
        }
    }
    world.set(origin, 8 << 4);
    for d in HORIZONTAL {
        world.set(origin + d, 8 << 4);
    }
    for x in -2..=2 {
        for z in -2..=2 {
            if x == -2 || x == 2 || z == -2 || z == 2 {
                world.set(origin + IVec3::new(x, 1, z), 24 << 4);
            }
        }
    }
    for d in HORIZONTAL {
        world.set(origin + d * 2 + IVec3::Y, (44 << 4) | 1);
    }
    for x in -1..=1 {
        for z in -1..=1 {
            world.set(
                origin + IVec3::new(x, 4, z),
                if x == 0 && z == 0 {
                    24 << 4
                } else {
                    (44 << 4) | 1
                },
            );
        }
    }
    for y in 1..=3 {
        for x in [-1, 1] {
            for z in [-1, 1] {
                world.set(origin + IVec3::new(x, y, z), 24 << 4);
            }
        }
    }
    true
}

pub fn swamp_tree(
    world: &mut impl FeatureWorld,
    random: &mut JavaRandom,
    mut origin: IVec3,
) -> bool {
    let height = random.next_int(4) + 5;
    while matches!(world.get(origin - IVec3::Y) >> 4, 8 | 9) {
        origin.y -= 1;
    }
    if origin.y < 1 || origin.y + height + 1 > 256 {
        return false;
    }
    for y in origin.y..=origin.y + height + 1 {
        let radius = if y >= origin.y + height - 1 {
            3
        } else if y == origin.y {
            0
        } else {
            1
        };
        for x in origin.x - radius..=origin.x + radius {
            for z in origin.z - radius..=origin.z + radius {
                let state = world.get(IVec3::new(x, y, z));
                if !(0..256).contains(&y)
                    || !(leaves_or_air(state) || y <= origin.y && matches!(state >> 4, 8 | 9))
                {
                    return false;
                }
            }
        }
    }
    if !matches!(world.get(origin - IVec3::Y) >> 4, 2 | 3) || origin.y >= 256 - height - 1 {
        return false;
    }
    if world.get(origin - IVec3::Y) >> 4 != 3 {
        world.set(origin - IVec3::Y, 3 << 4);
    }
    for y in origin.y + height - 3..=origin.y + height {
        let relative = y - (origin.y + height);
        let radius = 2 - relative / 2;
        for x in origin.x - radius..=origin.x + radius {
            for z in origin.z - radius..=origin.z + radius {
                if (x - origin.x).abs() == radius
                    && (z - origin.z).abs() == radius
                    && (random.next_int(2) == 0 || relative == 0)
                {
                    continue;
                }
                let p = IVec3::new(x, y, z);
                if !super::full(world.get(p)) {
                    world.set(p, 18 << 4);
                }
            }
        }
    }
    for y in 0..height {
        let p = origin + IVec3::Y * y;
        let state = world.get(p);
        if leaves_or_air(state) || matches!(state >> 4, 8 | 9) {
            world.set(p, 17 << 4);
        }
    }
    for y in origin.y + height - 3..=origin.y + height {
        let radius = 2 - (y - (origin.y + height)) / 2;
        for x in origin.x - radius..=origin.x + radius {
            for z in origin.z - radius..=origin.z + radius {
                let p = IVec3::new(x, y, z);
                if !matches!(world.get(p) >> 4, 18 | 161) {
                    continue;
                }
                for (d, meta) in [
                    (IVec3::NEG_X, 8),
                    (IVec3::X, 2),
                    (IVec3::NEG_Z, 1),
                    (IVec3::Z, 4),
                ] {
                    if random.next_int(4) == 0 && world.get(p + d) == 0 {
                        vine(world, p + d, meta, true);
                    }
                }
            }
        }
    }
    true
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BigMushroom {
    pub kind: Option<u16>,
}
impl BigMushroom {
    pub fn generate(
        &mut self,
        world: &mut impl FeatureWorld,
        random: &mut JavaRandom,
        origin: IVec3,
    ) -> bool {
        self.generate_blocks(world, random, origin, FeatureWorld::get, FeatureWorld::set)
    }
    pub(crate) fn generate_blocks<W>(
        &mut self,
        world: &mut W,
        random: &mut JavaRandom,
        origin: IVec3,
        get: impl Fn(&W, IVec3) -> u16,
        set: impl Fn(&mut W, IVec3, u16),
    ) -> bool {
        let kind = *self
            .kind
            .get_or_insert_with(|| if random.next_boolean() { 99 } else { 100 });
        let height = random.next_int(3) + 4;
        if origin.y < 1 || origin.y + height + 1 >= 256 {
            return false;
        }
        for y in origin.y..=origin.y + height + 1 {
            let radius = if y <= origin.y + 3 { 0 } else { 3 };
            for x in origin.x - radius..=origin.x + radius {
                for z in origin.z - radius..=origin.z + radius {
                    if !(0..256).contains(&y) || !leaves_or_air(get(world, IVec3::new(x, y, z))) {
                        return false;
                    }
                }
            }
        }
        if !matches!(get(world, origin - IVec3::Y) >> 4, 2 | 3 | 110) {
            return false;
        }
        let start = origin.y + height - if kind == 100 { 3 } else { 0 };
        for y in start..=origin.y + height {
            let radius = if kind == 99 {
                3
            } else if y < origin.y + height {
                2
            } else {
                1
            };
            let minx = origin.x - radius;
            let maxx = origin.x + radius;
            let minz = origin.z - radius;
            let maxz = origin.z + radius;
            for x in minx..=maxx {
                for z in minz..=maxz {
                    let mut meta = 5i32;
                    if x == minx {
                        meta -= 1;
                    } else if x == maxx {
                        meta += 1;
                    }
                    if z == minz {
                        meta -= 3;
                    } else if z == maxz {
                        meta += 3;
                    }
                    if kind == 99 || y < origin.y + height {
                        if (x == minx || x == maxx) && (z == minz || z == maxz) {
                            continue;
                        }
                        if x == origin.x - (radius - 1) && z == minz
                            || x == minx && z == origin.z - (radius - 1)
                        {
                            meta = 1;
                        }
                        if x == origin.x + (radius - 1) && z == minz
                            || x == maxx && z == origin.z - (radius - 1)
                        {
                            meta = 3;
                        }
                        if x == origin.x - (radius - 1) && z == maxz
                            || x == minx && z == origin.z + (radius - 1)
                        {
                            meta = 7;
                        }
                        if x == origin.x + (radius - 1) && z == maxz
                            || x == maxx && z == origin.z + (radius - 1)
                        {
                            meta = 9;
                        }
                    }
                    if meta == 5 && y < origin.y + height {
                        meta = 0;
                    }
                    let p = IVec3::new(x, y, z);
                    // Original compares base Y, not this layer, when skipping inside.
                    if origin.y < origin.y + height - 1 && meta == 0 || super::full(get(world, p)) {
                        continue;
                    }
                    set(world, p, (kind << 4) | meta as u16);
                }
            }
        }
        for y in 0..height {
            let p = origin + IVec3::Y * y;
            if !super::full(get(world, p)) {
                set(world, p, (kind << 4) | 10);
            }
        }
        true
    }
}

pub fn ice_path(world: &mut impl FeatureWorld, random: &mut JavaRandom, mut origin: IVec3) -> bool {
    while world.get(origin) == 0 && origin.y > 2 {
        origin.y -= 1;
    }
    if world.get(origin) >> 4 != 80 {
        return false;
    }
    let radius = random.next_int(2) + 2;
    for x in -radius..=radius {
        for z in -radius..=radius {
            if x * x + z * z > radius * radius {
                continue;
            }
            for y in -1..=1 {
                let p = origin + IVec3::new(x, y, z);
                if matches!(world.get(p) >> 4, 3 | 80 | 79) {
                    world.set(p, 174 << 4);
                }
            }
        }
    }
    true
}
pub fn ice_spike(
    world: &mut impl FeatureWorld,
    random: &mut JavaRandom,
    mut origin: IVec3,
) -> bool {
    while world.get(origin) == 0 && origin.y > 2 {
        origin.y -= 1;
    }
    if world.get(origin) >> 4 != 80 {
        return false;
    }
    origin.y += random.next_int(4);
    let height = random.next_int(4) + 7;
    let width = height / 4 + random.next_int(2);
    if width > 1 && random.next_int(60) == 0 {
        origin.y += 10 + random.next_int(30);
    }
    for y in 0..height {
        let radius = (1. - y as f32 / height as f32) * width as f32;
        let bound = radius.ceil() as i32;
        for x in -bound..=bound {
            let dx = x.abs() as f32 - 0.25;
            for z in -bound..=bound {
                let dz = z.abs() as f32 - 0.25;
                if (x != 0 || z != 0) && dx * dx + dz * dz > radius * radius
                    || (x == -bound || x == bound || z == -bound || z == bound)
                        && random.next_float() > 0.75
                {
                    continue;
                }
                let p = origin + IVec3::new(x, y, z);
                if matches!(world.get(p) >> 4, 0 | 3 | 80 | 79) {
                    world.set(p, 174 << 4);
                }
                if y != 0 && bound > 1 {
                    let p = origin + IVec3::new(x, -y, z);
                    if matches!(world.get(p) >> 4, 0 | 3 | 80 | 79) {
                        world.set(p, 174 << 4);
                    }
                }
            }
        }
    }
    let roots = (width - 1).clamp(0, 1);
    for x in -roots..=roots {
        for z in -roots..=roots {
            let mut p = origin + IVec3::new(x, -1, z);
            let mut length = if x.abs() == 1 && z.abs() == 1 {
                random.next_int(5)
            } else {
                50
            };
            while p.y > 50 && matches!(world.get(p) >> 4, 0 | 3 | 80 | 79 | 174) {
                world.set(p, 174 << 4);
                p.y -= 1;
                length -= 1;
                if length <= 0 {
                    p.y -= random.next_int(5) + 1;
                    length = random.next_int(5);
                }
            }
        }
    }
    true
}

fn huge_tree_ready(world: &mut impl FeatureWorld, origin: IVec3, height: i32) -> bool {
    if origin.y < 1 || origin.y + height + 1 > 256 {
        return false;
    }
    for y in 0..=height + 1 {
        let radius = if y == 0 { 1 } else { 2 };
        for x in -radius..=radius {
            for z in -radius..=radius {
                let p = origin + IVec3::new(x, y, z);
                if !(0..256).contains(&p.y) || !tree_replaceable(world.get(p)) {
                    return false;
                }
            }
        }
    }
    if origin.y < 2 || !matches!(world.get(origin - IVec3::Y) >> 4, 2 | 3) {
        return false;
    }
    for d in [IVec3::ZERO, IVec3::X, IVec3::Z, IVec3::X + IVec3::Z] {
        let p = origin - IVec3::Y + d;
        if world.get(p) >> 4 != 3 {
            world.set(p, 3 << 4);
        }
    }
    true
}
fn huge_leaves(
    world: &mut impl FeatureWorld,
    origin: IVec3,
    radius: i32,
    state: u16,
    square: bool,
) {
    let max = radius + if square { 1 } else { 0 };
    for x in -radius..=max {
        for z in -radius..=max {
            let inside = x * x + z * z <= radius * radius
                || square
                    && ((x - 1) * (x - 1) + (z - 1) * (z - 1) <= radius * radius
                        || x * x + (z - 1) * (z - 1) <= radius * radius
                        || (x - 1) * (x - 1) + z * z <= radius * radius);
            let p = origin + IVec3::new(x, 0, z);
            if inside && leaves_or_air(world.get(p)) {
                world.set(p, state);
            }
        }
    }
}
pub fn mega_pine(
    world: &mut impl FeatureWorld,
    random: &mut JavaRandom,
    origin: IVec3,
    spruce: bool,
) -> bool {
    let height = random.next_int(3) + 13 + random.next_int(15);
    if !huge_tree_ready(world, origin, height) {
        return false;
    }
    let depth = random.next_int(5) + if spruce { 13 } else { 3 };
    let mut previous = 0;
    for y in origin.y + height - depth..=origin.y + height {
        let delta = origin.y + height - y;
        let radius = (delta as f32 / depth as f32 * 3.5).floor() as i32;
        huge_leaves(
            world,
            IVec3::new(origin.x, y, origin.z),
            radius
                + if delta > 0 && radius == previous && y & 1 == 0 {
                    1
                } else {
                    0
                },
            (18 << 4) | 1,
            true,
        );
        previous = radius;
    }
    for y in 0..height {
        for d in [IVec3::ZERO, IVec3::X, IVec3::X + IVec3::Z, IVec3::Z] {
            if y >= height - 1 && d != IVec3::ZERO {
                continue;
            }
            let p = origin + IVec3::Y * y + d;
            if leaves_or_air(world.get(p)) {
                world.set(p, (17 << 4) | 1);
            }
        }
    }
    true
}
pub fn mega_pine_soil(world: &mut impl FeatureWorld, random: &mut JavaRandom, origin: IVec3) {
    fn patch(world: &mut impl FeatureWorld, p: IVec3) {
        for x in -2i32..=2 {
            for z in -2i32..=2 {
                if x.abs() == 2 && z.abs() == 2 {
                    continue;
                }
                for y in (-3..=2).rev() {
                    let q = p + IVec3::new(x, y, z);
                    let state = world.get(q);
                    if matches!(state >> 4, 2 | 3) {
                        world.set(q, (3 << 4) | 2);
                        break;
                    }
                    if state != 0 && y < 0 {
                        break;
                    }
                }
            }
        }
    }
    for d in [
        IVec3::new(-1, 0, -1),
        IVec3::new(2, 0, -1),
        IVec3::new(-1, 0, 2),
        IVec3::new(2, 0, 2),
    ] {
        patch(world, origin + d);
    }
    for _ in 0..5 {
        let n = random.next_int(64);
        let x = n % 8;
        let z = n / 8;
        if x == 0 || x == 7 || z == 0 || z == 7 {
            patch(world, origin + IVec3::new(x - 3, 0, z - 3));
        }
    }
}
pub fn block_blob(
    world: &mut impl FeatureWorld,
    random: &mut JavaRandom,
    mut origin: IVec3,
) -> bool {
    while origin.y > 3 && !matches!(world.get(origin - IVec3::Y) >> 4, 1..=3) {
        origin.y -= 1;
    }
    if origin.y <= 3 {
        return false;
    }
    for _ in 0..3 {
        let x = random.next_int(2);
        let y = random.next_int(2);
        let z = random.next_int(2);
        let radius = (x + y + z) as f32 * 0.333 + 0.5;
        for dz in -z..=z {
            for dy in -y..=y {
                for dx in -x..=x {
                    if (dx * dx + dy * dy + dz * dz) as f64 <= (radius * radius) as f64 {
                        world.set(origin + IVec3::new(dx, dy, dz), 48 << 4);
                    }
                }
            }
        }
        origin += IVec3::new(
            -1 + random.next_int(2),
            -random.next_int(2),
            -1 + random.next_int(2),
        );
    }
    true
}
pub fn mega_jungle(world: &mut impl FeatureWorld, random: &mut JavaRandom, origin: IVec3) -> bool {
    let height = random.next_int(3) + 10 + random.next_int(20);
    if !huge_tree_ready(world, origin, height) {
        return false;
    }
    for y in -2..=0 {
        huge_leaves(
            world,
            origin + IVec3::Y * (height + y),
            3 - y,
            (18 << 4) | 3,
            true,
        );
    }
    let mut branch_y = origin.y + height - 2 - random.next_int(4);
    while branch_y > origin.y + height / 2 {
        let angle = random.next_float() * std::f32::consts::PI * 2.;
        let mut branch = origin;
        for step in 0..5 {
            branch = IVec3::new(
                origin.x + (1.5 + crate::vanilla::math::cos(angle) * step as f32) as i32,
                branch_y - 3 + step / 2,
                origin.z + (1.5 + crate::vanilla::math::sin(angle) * step as f32) as i32,
            );
            world.set(branch, (17 << 4) | 3);
        }
        let depth = 1 + random.next_int(2);
        for y in branch_y - depth..=branch_y {
            huge_leaves(
                world,
                IVec3::new(branch.x, y, branch.z),
                1 - (y - branch_y),
                (18 << 4) | 3,
                false,
            );
        }
        branch_y -= 2 + random.next_int(4);
    }
    for y in 0..height {
        for (offset, sides) in [
            (IVec3::ZERO, [(IVec3::NEG_X, 8), (IVec3::NEG_Z, 1)]),
            (IVec3::X, [(IVec3::X, 2), (IVec3::NEG_Z, 1)]),
            (IVec3::X + IVec3::Z, [(IVec3::X, 2), (IVec3::Z, 4)]),
            (IVec3::Z, [(IVec3::NEG_X, 8), (IVec3::Z, 4)]),
        ] {
            if y >= height - 1 && offset != IVec3::ZERO {
                continue;
            }
            let p = origin + IVec3::Y * y + offset;
            if tree_replaceable(world.get(p)) {
                world.set(p, (17 << 4) | 3);
                if y > 0 {
                    for (d, meta) in sides {
                        if random.next_int(3) > 0 && world.get(p + d) == 0 {
                            vine(world, p + d, meta, false);
                        }
                    }
                }
            }
        }
    }
    true
}

pub fn shrub(world: &mut impl FeatureWorld, random: &mut JavaRandom, mut origin: IVec3) -> bool {
    while leaves_or_air(world.get(origin)) && origin.y > 0 {
        origin.y -= 1;
    }
    if matches!(world.get(origin) >> 4, 2 | 3) {
        origin.y += 1;
        world.set(origin, (17 << 4) | 3);
        for y in 0i32..=2 {
            let radius = 2 - y;
            for x in -radius..=radius {
                for z in -radius..=radius {
                    if x.abs() == radius && z.abs() == radius && random.next_int(2) == 0 {
                        continue;
                    }
                    let p = origin + IVec3::new(x, y, z);
                    if !super::full(world.get(p)) {
                        world.set(p, 18 << 4);
                    }
                }
            }
        }
    }
    true
}
pub fn melon(world: &mut impl FeatureWorld, random: &mut JavaRandom, origin: IVec3) -> bool {
    for _ in 0..64 {
        let p = origin + offset(random);
        if crate::vanilla::material::replaceable(world.get(p)) && world.get(p - IVec3::Y) >> 4 == 2
        {
            world.set(p, 103 << 4);
        }
    }
    true
}
pub fn vines(world: &mut impl FeatureWorld, random: &mut JavaRandom, mut p: IVec3) -> bool {
    while p.y < 128 {
        if world.get(p) == 0 {
            for (d, meta) in [
                (IVec3::NEG_Z, 4),
                (IVec3::X, 8),
                (IVec3::Z, 1),
                (IVec3::NEG_X, 2),
            ] {
                let state = world.get(p - d);
                if crate::vanilla::material::full_cube(state)
                    && crate::vanilla::material::blocks_movement(state)
                {
                    world.set(p, (106 << 4) | meta);
                    break;
                }
            }
        } else {
            p += IVec3::new(
                random.next_int(4) - random.next_int(4),
                0,
                random.next_int(4) - random.next_int(4),
            );
        }
        p.y += 1;
    }
    true
}
pub fn dark_oak(world: &mut impl FeatureWorld, random: &mut JavaRandom, origin: IVec3) -> bool {
    let height = random.next_int(3) + random.next_int(2) + 6;
    if origin.y < 1
        || origin.y + height + 1 >= 256
        || !matches!(world.get(origin - IVec3::Y) >> 4, 2 | 3)
    {
        return false;
    }
    for y in 0..=height + 1 {
        let radius = if y >= height - 1 {
            2
        } else if y == 0 {
            0
        } else {
            1
        };
        for x in -radius..=radius {
            for z in -radius..=radius {
                if !tree_replaceable(world.get(origin + IVec3::new(x, y, z))) {
                    return false;
                }
            }
        }
    }
    for d in [IVec3::ZERO, IVec3::X, IVec3::Z, IVec3::X + IVec3::Z] {
        let p = origin - IVec3::Y + d;
        if world.get(p) >> 4 != 3 {
            world.set(p, 3 << 4);
        }
    }
    let direction = HORIZONTAL[random.next_int(4) as usize];
    let bend = height - random.next_int(4);
    let mut steps = 2 - random.next_int(3);
    let mut crown = IVec3::new(origin.x, origin.y + height - 1, origin.z);
    fn wood(world: &mut impl FeatureWorld, p: IVec3) {
        if tree_replaceable(world.get(p)) {
            world.set(p, (162 << 4) | 1);
        }
    }
    fn leaf(world: &mut impl FeatureWorld, p: IVec3) {
        if world.get(p) == 0 {
            world.set(p, (161 << 4) | 1);
        }
    }
    for y in 0..height {
        if y >= bend && steps > 0 {
            crown += direction;
            steps -= 1;
        }
        let p = IVec3::new(crown.x, origin.y + y, crown.z);
        if leaves_or_air(world.get(p)) {
            for d in [IVec3::ZERO, IVec3::X, IVec3::Z, IVec3::X + IVec3::Z] {
                wood(world, p + d);
            }
        }
    }
    for x in -2..=0 {
        for z in -2..=0 {
            for d in [
                IVec3::new(x, -1, z),
                IVec3::new(1 - x, -1, z),
                IVec3::new(x, -1, 1 - z),
                IVec3::new(1 - x, -1, 1 - z),
            ] {
                leaf(world, crown + d);
            }
            if x <= -2 && z <= -1 || x == -1 && z == -2 {
                continue;
            }
            for d in [
                IVec3::new(x, 1, z),
                IVec3::new(1 - x, 1, z),
                IVec3::new(x, 1, 1 - z),
                IVec3::new(1 - x, 1, 1 - z),
            ] {
                leaf(world, crown + d);
            }
        }
    }
    if random.next_boolean() {
        for d in [IVec3::ZERO, IVec3::X, IVec3::X + IVec3::Z, IVec3::Z] {
            leaf(world, crown + IVec3::Y * 2 + d);
        }
    }
    for x in -3i32..=4 {
        for z in -3i32..=4 {
            if x.abs() >= 3 && z.abs() >= 3 {
                continue;
            }
            leaf(world, crown + IVec3::new(x, 0, z));
        }
    }
    for x in -1..=2 {
        for z in -1..=2 {
            if (0..=1).contains(&x) && (0..=1).contains(&z) || random.next_int(3) > 0 {
                continue;
            }
            let length = random.next_int(3) + 2;
            for y in 0..length {
                wood(
                    world,
                    IVec3::new(origin.x + x, crown.y - y - 1, origin.z + z),
                );
            }
            for dx in -1..=1 {
                for dz in -1..=1 {
                    leaf(world, crown + IVec3::new(x + dx, 0, z + dz));
                }
            }
            for dx in -2i32..=2 {
                for dz in -2i32..=2 {
                    if dx.abs() == 2 && dz.abs() == 2 {
                        continue;
                    }
                    leaf(world, crown + IVec3::new(x + dx, -1, z + dz));
                }
            }
        }
    }
    true
}

pub fn end_spike(world: &mut impl FeatureWorld, random: &mut JavaRandom, origin: IVec3) -> bool {
    if world.get(origin) != 0 || world.get(origin - IVec3::Y) >> 4 != 121 {
        return false;
    }
    let height = random.next_int(32) + 6;
    let radius = random.next_int(4) + 1;
    for x in -radius..=radius {
        for z in -radius..=radius {
            if x * x + z * z <= radius * radius + 1
                && world.get(origin + IVec3::new(x, -1, z)) >> 4 != 121
            {
                return false;
            }
        }
    }
    for y in origin.y..(origin.y + height).min(256) {
        for x in -radius..=radius {
            for z in -radius..=radius {
                if x * x + z * z <= radius * radius + 1 {
                    world.set(IVec3::new(origin.x + x, y, origin.z + z), 49 << 4);
                }
            }
        }
    }
    world.entity(super::GeneratedEntity {
        kind: super::GeneratedEntityKind::EnderCrystal,
        position: [
            (origin.x as f32 + 0.5) as f64,
            (origin.y + height) as f64,
            (origin.z as f32 + 0.5) as f64,
        ],
        yaw: random.next_float() * 360.,
    });
    world.set(origin + IVec3::Y * height, 7 << 4);
    true
}

pub fn mushroom(
    world: &mut impl FeatureWorld,
    random: &mut JavaRandom,
    origin: IVec3,
    state: u16,
) -> bool {
    for _ in 0..64 {
        let p = origin + offset(random);
        if world.get(p) != 0 || !(0..256).contains(&p.y) {
            continue;
        }
        let below = world.get(p - IVec3::Y);
        if below >> 4 == 110
            || below == (3 << 4) | 2
            || world.combined_light(p) < 13 && super::full(below)
        {
            world.set(p, state);
        }
    }
    true
}

/// The original big-oak generator retains its chosen height between calls.
/// Keep this object with the biome decorator rather than reconstructing per tree.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BigTree {
    pub height_limit: i32,
    pub leaf_distance: i32,
}
impl Default for BigTree {
    fn default() -> Self {
        Self {
            height_limit: 0,
            leaf_distance: 4,
        }
    }
}
fn add_double(p: IVec3, d: [f64; 3]) -> IVec3 {
    IVec3::new(
        (p.x as f64 + d[0]).floor() as i32,
        (p.y as f64 + d[1]).floor() as i32,
        (p.z as f64 + d[2]).floor() as i32,
    )
}
fn line_point(a: IVec3, delta: IVec3, length: i32, step: i32) -> IVec3 {
    add_double(
        a,
        delta
            .to_array()
            .map(|d| (0.5f32 + step as f32 * (d as f32 / length as f32)) as f64),
    )
}
fn check_line(world: &impl FeatureWorld, a: IVec3, b: IVec3) -> i32 {
    let delta = b - a;
    let length = delta.abs().max_element();
    if length == 0 {
        return -1;
    }
    for step in 0..=length {
        if !tree_replaceable(world.get(line_point(a, delta, length, step))) {
            return step;
        }
    }
    -1
}
fn log_line(world: &mut impl FeatureWorld, a: IVec3, b: IVec3) {
    let delta = b - a;
    let length = delta.abs().max_element();
    for step in 0..=length {
        let p = line_point(a, delta, length, step);
        let dx = (p.x - a.x).abs();
        let dz = (p.z - a.z).abs();
        let axis = if dx.max(dz) == 0 {
            0
        } else if dx >= dz {
            4
        } else {
            8
        };
        world.set(p, (17 << 4) | axis);
    }
}
impl BigTree {
    pub fn prepare(&mut self) {
        self.leaf_distance = 5;
    }
    pub fn generate(
        &mut self,
        world: &mut impl FeatureWorld,
        random: &mut JavaRandom,
        origin: IVec3,
    ) -> bool {
        let mut random = JavaRandom::new(random.next_long());
        if self.height_limit == 0 {
            self.height_limit = 5 + random.next_int(12);
        }
        if !soil(world.get(origin - IVec3::Y)) {
            return false;
        }
        let obstruction = check_line(world, origin, origin + IVec3::Y * (self.height_limit - 1));
        if obstruction != -1 {
            if obstruction < 6 {
                return false;
            }
            self.height_limit = obstruction;
        }
        let height = ((self.height_limit as f64 * 0.618) as i32).min(self.height_limit - 1);
        let attempts = ((1.382 + (self.height_limit as f64 / 13.).powi(2)) as i32).max(1);
        let branch_top = origin.y + height;
        let mut nodes = vec![(
            origin + IVec3::Y * (self.height_limit - self.leaf_distance),
            branch_top,
        )];
        for layer in (0..=self.height_limit - self.leaf_distance).rev() {
            if (layer as f32) < self.height_limit as f32 * 0.3 {
                continue;
            }
            let half = self.height_limit as f32 / 2.;
            let relative = half - layer as f32;
            let radius = if relative == 0. {
                half
            } else if relative.abs() >= half {
                0.
            } else {
                ((half * half - relative * relative) as f64).sqrt() as f32
            } * 0.5;
            for _ in 0..attempts {
                let distance = radius as f64 * (random.next_float() as f64 + 0.328);
                let angle = (random.next_float() * 2.) as f64 * std::f64::consts::PI;
                let p = add_double(
                    origin,
                    [
                        distance * angle.sin() + 0.5,
                        (layer - 1) as f64,
                        distance * angle.cos() + 0.5,
                    ],
                );
                if check_line(world, p, p + IVec3::Y * self.leaf_distance) != -1 {
                    continue;
                }
                let dx = origin.x - p.x;
                let dz = origin.z - p.z;
                let slope = p.y as f64 - ((dx * dx + dz * dz) as f64).sqrt() * 0.381;
                let branch_y = if slope > branch_top as f64 {
                    branch_top
                } else {
                    slope as i32
                };
                if check_line(world, IVec3::new(origin.x, branch_y, origin.z), p) == -1 {
                    nodes.push((p, branch_y));
                }
            }
        }
        for (node, _) in &nodes {
            for y in 0..self.leaf_distance {
                let radius = if y == 0 || y == self.leaf_distance - 1 {
                    2f32
                } else {
                    3.
                };
                let width = (radius as f64 + 0.618) as i32;
                for x in -width..=width {
                    for z in -width..=width {
                        if (x.abs() as f64 + 0.5).powi(2) + (z.abs() as f64 + 0.5).powi(2)
                            > (radius * radius) as f64
                        {
                            continue;
                        }
                        let p = *node + IVec3::new(x, y, z);
                        if leaves_or_air(world.get(p)) {
                            world.set(p, 18 << 4);
                        }
                    }
                }
            }
        }
        log_line(world, origin, origin + IVec3::Y * height);
        for (node, branch_y) in nodes {
            let p = IVec3::new(origin.x, branch_y, origin.z);
            if p != node && (branch_y - origin.y) as f64 >= self.height_limit as f64 * 0.2 {
                log_line(world, p, node);
            }
        }
        true
    }
}

pub fn tall_grass(
    world: &mut impl FeatureWorld,
    random: &mut JavaRandom,
    mut origin: IVec3,
    meta: u16,
) -> bool {
    while leaves_or_air(world.get(origin)) && origin.y > 0 {
        origin.y -= 1;
    }
    for _ in 0..128 {
        let p = origin + offset(random);
        if world.get(p) == 0 && soil(world.get(p - IVec3::Y)) {
            world.set(p, (31 << 4) | meta);
        }
    }
    true
}
pub fn flowers(
    world: &mut impl FeatureWorld,
    random: &mut JavaRandom,
    origin: IVec3,
    state: u16,
) -> bool {
    for _ in 0..64 {
        let p = origin + offset(random);
        if world.get(p) == 0 && soil(world.get(p - IVec3::Y)) {
            world.set(p, state);
        }
    }
    true
}

pub fn double_plant(
    world: &mut impl FeatureWorld,
    random: &mut JavaRandom,
    origin: IVec3,
    meta: u16,
) -> bool {
    let mut generated = false;
    for _ in 0..64 {
        let p = origin + offset(random);
        if world.get(p) == 0 && world.get(p + IVec3::Y) == 0 && soil(world.get(p - IVec3::Y)) {
            let selected = world.double_plant_type(meta);
            world.set(p, (175 << 4) | selected);
            world.set(p + IVec3::Y, (175 << 4) | 10);
            generated = true;
        }
    }
    generated
}
pub fn dead_bush(
    world: &mut impl FeatureWorld,
    random: &mut JavaRandom,
    mut origin: IVec3,
) -> bool {
    while leaves_or_air(world.get(origin)) && origin.y > 0 {
        origin.y -= 1;
    }
    for _ in 0..4 {
        let p = origin + offset(random);
        if world.get(p) == 0 && matches!(world.get(p - IVec3::Y) >> 4, 12 | 172 | 159 | 3) {
            world.set(p, 32 << 4);
        }
    }
    true
}
pub fn reeds(world: &mut impl FeatureWorld, random: &mut JavaRandom, origin: IVec3) -> bool {
    for _ in 0..20 {
        let p = origin
            + IVec3::new(
                random.next_int(4) - random.next_int(4),
                0,
                random.next_int(4) - random.next_int(4),
            );
        let water = HORIZONTAL
            .iter()
            .any(|d| matches!(world.get(p - IVec3::Y + *d) >> 4, 8 | 9));
        if world.get(p) != 0 || !water {
            continue;
        }
        let bound = random.next_int(3) + 1;
        let height = 2 + random.next_int(bound);
        for y in 0..height {
            // Original repeatedly tests base, not the block currently being set.
            if matches!(world.get(p - IVec3::Y) >> 4, 83 | 2 | 3 | 12) {
                world.set(p + IVec3::Y * y, 83 << 4);
            }
        }
    }
    true
}
pub fn cactus(world: &mut impl FeatureWorld, random: &mut JavaRandom, origin: IVec3) -> bool {
    for _ in 0..10 {
        let p = origin + offset(random);
        if world.get(p) != 0 {
            continue;
        }
        let bound = random.next_int(3) + 1;
        let height = 1 + random.next_int(bound);
        for y in 0..height {
            if HORIZONTAL.iter().all(|d| !solid(world.get(p + *d)))
                && matches!(world.get(p - IVec3::Y) >> 4, 81 | 12)
            {
                world.set(p + IVec3::Y * y, 81 << 4);
            }
        }
    }
    true
}
pub fn waterlily(world: &mut impl FeatureWorld, random: &mut JavaRandom, origin: IVec3) -> bool {
    for _ in 0..10 {
        let p = origin + offset(random);
        let below = world.get(p - IVec3::Y);
        if world.get(p) == 0 && matches!(below >> 4, 8 | 9) && below & 15 == 0 {
            world.set(p, 111 << 4);
        }
    }
    true
}
pub fn pumpkin(world: &mut impl FeatureWorld, random: &mut JavaRandom, origin: IVec3) -> bool {
    for _ in 0..64 {
        let p = origin + offset(random);
        if world.get(p) == 0 && world.get(p - IVec3::Y) >> 4 == 2 {
            world.set(p, (86 << 4) | [2, 3, 0, 1][random.next_int(4) as usize]);
        }
    }
    true
}
/// WorldGenSand and WorldGenClay (different replacement predicates and depth).
pub fn sediment(
    world: &mut impl FeatureWorld,
    random: &mut JavaRandom,
    origin: IVec3,
    state: u16,
    size: i32,
    clay: bool,
) -> bool {
    if !matches!(world.get(origin) >> 4, 8 | 9) {
        return false;
    }
    let radius = random.next_int(size - 2) + 2;
    let depth = if clay { 1 } else { 2 };
    for x in origin.x - radius..=origin.x + radius {
        for z in origin.z - radius..=origin.z + radius {
            if (x - origin.x) * (x - origin.x) + (z - origin.z) * (z - origin.z) > radius * radius {
                continue;
            }
            for y in origin.y - depth..=origin.y + depth {
                let p = IVec3::new(x, y, z);
                let id = world.get(p) >> 4;
                if id == 3 || id == if clay { 82 } else { 2 } {
                    world.set(p, state);
                }
            }
        }
    }
    true
}
