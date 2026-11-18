//! Inclusive 1.8.9 StructureBoundingBox coordinates and component transforms.
use serde::{Deserialize, Serialize};

/// Block.isFullBlock values from the original 1.8.9 block registry. This is
/// distinct from material solidity and from isFullCube (e.g. soul sand).
pub fn full_block(state: u16) -> bool {
    matches!(state >> 4, 1..=5 | 7 | 12..=19 | 21..=25 | 35 | 41..=43 | 45..=49 |
        56..=58 | 61..=62 | 73..=74 | 80 | 82 | 84 | 86..=89 | 91 | 97..=100 |
        103 | 110 | 112 | 121 | 123..=125 | 129 | 133 | 137 | 152..=153 | 155 |
        158..=159 | 161..=162 | 168..=170 | 172..=174 | 179 | 181)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundingBox {
    pub min: [i32; 3],
    pub max: [i32; 3],
}

/// Original StructureComponent.getMetadataWithOffset, including its deliberately
/// narrow block list and legacy metadata behavior (not a general block rotation).
pub fn rotate_metadata(block: u16, metadata: u8, orientation: u8) -> u8 {
    let n = metadata;
    if block == 66 {
        return if orientation == 1 || orientation == 3 {
            if n == 1 {
                0
            } else {
                1
            }
        } else {
            n
        };
    }
    if matches!(block, 64 | 71 | 193..=197) {
        return match orientation {
            0 if n == 0 => 2,
            0 if n == 2 => 0,
            1 => (n + 1) & 3,
            3 => (n + 3) & 3,
            _ => n,
        };
    }
    if matches!(block, 53 | 67 | 109 | 114 | 128) {
        return match (orientation, n) {
            (0, 2) => 3,
            (0, 3) => 2,
            (1, 0) | (3, 0) => 2,
            (1, 1) | (3, 1) => 3,
            (1, 2) | (3, 3) => 0,
            (1, 3) | (3, 2) => 1,
            _ => n,
        };
    }
    if matches!(block, 65 | 23 | 29 | 33 | 69) {
        return match (orientation, n) {
            (0, 2) | (1, 5) | (3, 5) => 3,
            (0, 3) | (1, 4) | (3, 4) => 2,
            (1, 2) | (3, 3) => 4,
            (1, 3) | (3, 2) => 5,
            _ => n,
        };
    }
    if block == 77 {
        return match (orientation, n) {
            (0, 3) => 4,
            (0, 4) => 3,
            (1, 3) | (3, 4) => 1,
            (1, 4) | (3, 3) => 2,
            (1, 2) | (3, 2) => 3,
            (1, 1) | (3, 1) => 4,
            _ => n,
        };
    }
    // Tripwire hook and all 1.8.9 BlockDirectional subclasses: bed, cocoa,
    // fence gates, pumpkin, repeaters and comparators.
    if matches!(
        block,
        26 | 86 | 91 | 93 | 94 | 107 | 127 | 131 | 149 | 150 | 183..=187
    ) {
        return match (orientation, n & 3) {
            (0, 0) => 2,
            (0, 2) => 0,
            (1, 2) | (3, 0) => 1,
            (1, 0) | (3, 2) => 3,
            (1, 1) | (3, 1) => 2,
            (1, 3) | (3, 3) => 0,
            _ => n,
        };
    }
    n
}
impl BoundingBox {
    pub fn new(min: [i32; 3], max: [i32; 3]) -> Self {
        Self { min, max }
    }
    pub fn empty() -> Self {
        Self {
            min: [i32::MAX; 3],
            max: [i32::MIN; 3],
        }
    }
    /// EnumFacing horizontal indices: 0 South, 1 West, 2 North, 3 East.
    pub fn oriented(
        x: i32,
        y: i32,
        z: i32,
        offset: [i32; 3],
        size: [i32; 3],
        orientation: u8,
    ) -> Self {
        let [ox, oy, oz] = offset;
        let [sx, sy, sz] = size;
        match orientation {
            2 => Self::new(
                [x + ox, y + oy, z - sz + 1 + oz],
                [x + sx - 1 + ox, y + sy - 1 + oy, z + oz],
            ),
            1 => Self::new(
                [x - sz + 1 + oz, y + oy, z + ox],
                [x + oz, y + sy - 1 + oy, z + sx - 1 + ox],
            ),
            3 => Self::new(
                [x + oz, y + oy, z + ox],
                [x + sz - 1 + oz, y + sy - 1 + oy, z + sx - 1 + ox],
            ),
            _ => Self::new(
                [x + ox, y + oy, z + oz],
                [x + sx - 1 + ox, y + sy - 1 + oy, z + sz - 1 + oz],
            ),
        }
    }
    pub fn intersects(&self, other: &Self) -> bool {
        (0..3).all(|axis| self.max[axis] >= other.min[axis] && self.min[axis] <= other.max[axis])
    }
    pub fn intersects_xz(&self, min_x: i32, min_z: i32, max_x: i32, max_z: i32) -> bool {
        self.max[0] >= min_x && self.min[0] <= max_x && self.max[2] >= min_z && self.min[2] <= max_z
    }
    pub fn contains(&self, p: [i32; 3]) -> bool {
        (0..3).all(|axis| p[axis] >= self.min[axis] && p[axis] <= self.max[axis])
    }
    pub fn expand(&mut self, other: &Self) {
        for axis in 0..3 {
            self.min[axis] = self.min[axis].min(other.min[axis]);
            self.max[axis] = self.max[axis].max(other.max[axis]);
        }
    }
    pub fn offset(&mut self, x: i32, y: i32, z: i32) {
        for (axis, d) in [x, y, z].into_iter().enumerate() {
            self.min[axis] += d;
            self.max[axis] += d;
        }
    }
    pub fn size(&self) -> [i32; 3] {
        std::array::from_fn(|axis| self.max[axis] - self.min[axis] + 1)
    }
    pub fn local_to_world(&self, x: i32, y: i32, z: i32, orientation: u8) -> [i32; 3] {
        let wx = match orientation {
            1 => self.max[0] - z,
            3 => self.min[0] + z,
            _ => self.min[0] + x,
        };
        let wz = match orientation {
            2 => self.max[2] - z,
            0 => self.min[2] + z,
            _ => self.min[2] + x,
        };
        [wx, y + self.min[1], wz]
    }
}
