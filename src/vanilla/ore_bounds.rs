//! Use hardware floor where available without changing ore arithmetic or the
//! portable CPU requirement. Resolve once per vein, never change the RNG path.
type Compute = unsafe fn([f64; 3], f64) -> ([i32; 3], [i32; 3]);

#[derive(Clone, Copy)]
pub(super) struct Bounds(Option<Compute>);

impl Bounds {
    #[inline]
    pub(super) fn new() -> Self {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            use std::sync::OnceLock;
            static COMPUTE: OnceLock<Option<Compute>> = OnceLock::new();
            Self(*COMPUTE.get_or_init(|| {
                std::is_x86_feature_detected!("sse4.1").then_some(hardware as Compute)
            }))
        }
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        { Self(None) }
    }

    #[inline]
    pub(super) fn get(self, center: [f64; 3], radius: f64) -> ([i32; 3], [i32; 3]) {
        if let Some(compute) = self.0 {
            // Only new() constructs this private option, after checking the
            // actual CPU. The arithmetic has no memory/alias preconditions.
            unsafe { compute(center, radius) }
        } else {
            portable(center, radius)
        }
    }
}

#[inline]
fn portable(center: [f64; 3], radius: f64) -> ([i32; 3], [i32; 3]) {
    (center.map(|v| (v - radius).floor() as i32),
        center.map(|v| (v + radius).floor() as i32))
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "sse4.1")]
unsafe fn hardware(center: [f64; 3], radius: f64) -> ([i32; 3], [i32; 3]) {
    // Keep six independent subtract/add, floor and saturating casts. Do not
    // replace division, round centers first, enable fast math or require AVX.
    (center.map(|v| (v - radius).floor() as i32),
        center.map(|v| (v + radius).floor() as i32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_and_portable_ore_bounds_match_original_casts_at_boundaries() {
        let selected = Bounds::new();
        let fallback = Bounds(None);
        let original = |center: [f64;3], radius: f64| {
            let mut low = [0;3]; let mut high = [0;3];
            for i in 0..3 { low[i] = (center[i]-radius).floor() as i32; }
            for i in 0..3 { high[i] = (center[i]+radius).floor() as i32; }
            (low,high)
        };
        for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0., 0., -1., 1.,
            -30_000_000., 30_000_000., 2147483647., -2147483648.,
            f64::from_bits(1), -f64::from_bits(1)] {
            for radius in [0., 0.5, 1., 4.5, f64::INFINITY, f64::NAN] {
                let center = [x, x+0.499999999, x-0.000000001];
                assert_eq!(selected.get(center,radius), original(center,radius));
                assert_eq!(fallback.get(center,radius), original(center,radius));
            }
        }
        let mut random = 189u64;
        for i in 0..100_000 {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            let radius = (random as u16) as f64 / 65536. * 4.5 + 0.5;
            let center = [((random >> 32) as i32 % 30_000_000) as f64 + 0.499999999,
                (i % 256) as f64 - 0.5, ((random >> 8) as i32 % 30_000_000) as f64 - 0.000000001];
            let arbitrary = [f64::from_bits(random), f64::from_bits(!random),
                f64::from_bits(random.rotate_left(31))];
            for center in [center,arbitrary] {
                assert_eq!(selected.get(center,radius), original(center,radius));
                assert_eq!(fallback.get(center,radius), original(center,radius));
            }
        }
    }
}
