//! Hashing for the path finder's existing Java i32 point key only. This table
//! is never traversed: node insertion and heap order remain caller-owned.
use std::{
    collections::HashMap,
    hash::{BuildHasherDefault, Hasher},
};

pub(crate) type PointTable = HashMap<i32, usize, BuildHasherDefault<PointHasher>>;

#[derive(Default)]
pub(crate) struct PointHasher(u64);

// Each xor-shift is invertible and both multipliers are odd, so this mixes
// distinct 32-bit keys without introducing a new full-hash alias. The original
// path_hash coordinate aliases still arrive here as the same i32 map key.
#[inline]
fn mix(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

impl Hasher for PointHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
    #[inline]
    fn write_i32(&mut self, key: i32) {
        self.0 = mix(self.0 ^ u64::from(key as u32));
    }
    fn write(&mut self, bytes: &[u8]) {
        // i32::hash uses write_i32. Keep a defined fallback for Hasher's byte
        // entry point without weakening or changing the table's key equality.
        for &byte in bytes {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
        }
        self.0 = mix(self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        hash::{BuildHasher, RandomState},
        hint::black_box,
        time::Instant,
    };

    fn original_key([x, y, z]: [i32; 3]) -> i32 {
        (y & 255)
            | ((x & 32767) << 8)
            | ((z & 32767) << 24)
            | if x < 0 { i32::MIN } else { 0 }
            | if z < 0 { 32768 } else { 0 }
    }
    fn sequences() -> Vec<Vec<i32>> {
        [
            [0, 64, 0],
            [-129, 63, -255],
            [32760, 70, -32760],
            [-32760, 128, 32760],
            [2048, 5, 4096],
            [29_999_968, 250, -29_999_968],
        ]
        .into_iter()
        .map(|center| {
            let mut keys = Vec::new();
            // Expanding bounded searches and the original Z,-X,X,-Z neighbor
            // order produce repeated points, steps and signed coordinate keys.
            for radius in 0i32..=16 {
                for x in -radius..=radius {
                    for z in -radius..=radius {
                        if x.abs().max(z.abs()) != radius {
                            continue;
                        }
                        let y = center[1] + ((x.wrapping_mul(3) + z).rem_euclid(5) == 0) as i32;
                        for [dx, dz] in [[0, 1], [-1, 0], [1, 0], [0, -1]] {
                            keys.push(original_key([center[0] + x + dx, y, center[2] + z + dz]));
                        }
                    }
                }
            }
            // Original PathPoint keys alias z+256 and y+256 intentionally.
            keys.extend([
                original_key(center),
                original_key([center[0], center[1], center[2] + 256]),
                original_key([center[0], center[1] + 256, center[2]]),
            ]);
            keys
        })
        .collect()
    }

    #[test]
    fn original_i32_key_aliases_and_point_entry_bindings_are_preserved() {
        assert_eq!(original_key([7, 64, 3]), original_key([7, 320, 259]));
        for keys in sequences() {
            let mut standard = HashMap::<i32, usize>::new();
            let mut mixed = PointTable::default();
            for key in keys {
                let next = standard.len();
                let expected = *standard.entry(key).or_insert(next);
                let actual = *mixed.entry(key).or_insert(next);
                assert_eq!(actual, expected);
                assert_eq!(mixed.len(), standard.len());
            }
            for (key, expected) in standard {
                assert_eq!(mixed.get(&key), Some(&expected));
            }
        }
    }

    fn exercise<S: BuildHasher + Default>(sequences: &[Vec<i32>], repeats: usize) -> (u64, f64) {
        let start = Instant::now();
        let mut sum = 0u64;
        for _ in 0..repeats {
            for keys in sequences {
                let mut table = HashMap::<i32, usize, S>::default();
                for &key in keys {
                    let next = table.len();
                    sum = sum.wrapping_add(*table.entry(black_box(key)).or_insert(next) as u64);
                }
                black_box(table);
            }
        }
        (black_box(sum), start.elapsed().as_secs_f64() * 1000.)
    }

    #[test]
    #[ignore = "standalone optimized point-table microbenchmark; not an FPS measurement"]
    fn realistic_point_entry_hash_comparison() {
        let keys = sequences();
        let operations = keys.iter().map(Vec::len).sum::<usize>() * 64;
        // Warm both implementations, then alternate their order to limit bias.
        exercise::<RandomState>(&keys, 4);
        exercise::<BuildHasherDefault<PointHasher>>(&keys, 4);
        for trial in 0..7 {
            let (standard, mixed) = if trial % 2 == 0 {
                (
                    exercise::<RandomState>(&keys, 64),
                    exercise::<BuildHasherDefault<PointHasher>>(&keys, 64),
                )
            } else {
                let mixed = exercise::<BuildHasherDefault<PointHasher>>(&keys, 64);
                (exercise::<RandomState>(&keys, 64), mixed)
            };
            assert_eq!(standard.0, mixed.0);
            eprintln!("point_table trial={trial} operations={operations} standard_ms={:.6} mixed_ms={:.6} checksum={}",
                standard.1,mixed.1,standard.0);
        }
    }
}
