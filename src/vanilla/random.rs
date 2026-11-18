//! java.util.Random's 48-bit generator, including Java signed overflow rules.
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
#[cfg(test)]
thread_local! {
    static INDEPENDENT_SEED_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}
#[cfg(test)]
pub(crate) fn independent_seed_calls_for_thread() -> u64 {
    INDEPENDENT_SEED_CALLS.with(std::cell::Cell::get)
}

#[derive(Debug)]
enum State {
    Owned(u64),
    Shared(Arc<AtomicU64>),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_reentrant_handles_follow_reseeds_but_clones_snapshot() {
        let mut root = JavaRandom::new(-189);
        let mut callback = root.share();
        let mut nested = callback.share();
        let mut expected = JavaRandom::new(-189);
        assert_eq!(root.next_long(), expected.next_long());
        assert_eq!(callback.next_double(), expected.next_double());
        let mut snapshot = callback.clone();
        let mut snapshot_expected = expected.clone();
        nested.set_seed(12345678);
        expected.set_seed(12345678);
        assert_eq!(callback.next_int(31), expected.next_int(31));
        assert_eq!(root.next_long(), expected.next_long());
        assert_eq!(snapshot.next_long(), snapshot_expected.next_long());
        assert_eq!(root.state(), nested.state());
    }
}
#[derive(Debug)]
pub struct JavaRandom {
    state: State,
}
/// Cloning remains a deterministic snapshot. Explicit share() models Java
/// references to the same Random during reentrant chunk generation callbacks.
impl Clone for JavaRandom {
    fn clone(&self) -> Self {
        Self::from_state(self.state())
    }
}
impl JavaRandom {
    const MULTIPLIER: u64 = 0x5deece66d;
    const MASK: u64 = (1u64 << 48) - 1;
    /// Java 8's default Random combines the process-wide seed uniquifier with
    /// a monotonic clock whose epoch is arbitrary, as with System.nanoTime().
    pub fn independent_seed() -> i64 {
        #[cfg(test)]
        INDEPENDENT_SEED_CALLS.with(|calls| calls.set(calls.get() + 1));
        use std::sync::OnceLock;
        use std::time::{Instant, SystemTime, UNIX_EPOCH};
        static UNIQUE: AtomicU64 = AtomicU64::new(8_682_522_807_148_012);
        static CLOCK: OnceLock<(Instant, u64)> = OnceLock::new();
        let (origin, epoch) = CLOCK.get_or_init(|| {
            (
                Instant::now(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64,
            )
        });
        const STEP: u64 = 181_783_497_276_652_981;
        let old = UNIQUE
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |old| {
                Some(old.wrapping_mul(STEP))
            })
            .unwrap();
        (old.wrapping_mul(STEP) ^ epoch.wrapping_add(origin.elapsed().as_nanos() as u64)) as i64
    }
    pub fn new(seed: i64) -> Self {
        let mut random = Self {
            state: State::Owned(0),
        };
        random.set_seed(seed);
        random
    }
    pub fn set_seed(&mut self, seed: i64) {
        let value = (seed as u64 ^ Self::MULTIPLIER) & Self::MASK;
        match &mut self.state {
            State::Owned(state) => *state = value,
            State::Shared(state) => state.store(value, Ordering::Relaxed),
        }
    }
    /// Internal 48-bit state, for persisting and sharing the original world's RNG.
    pub fn state(&self) -> u64 {
        match &self.state {
            State::Owned(state) => *state,
            State::Shared(state) => state.load(Ordering::Relaxed),
        }
    }
    pub fn from_state(state: u64) -> Self {
        Self {
            state: State::Owned(state & Self::MASK),
        }
    }
    pub fn share(&mut self) -> Self {
        if let State::Owned(state) = self.state {
            self.state = State::Shared(Arc::new(AtomicU64::new(state)));
        }
        let State::Shared(state) = &self.state else {
            unreachable!()
        };
        Self {
            state: State::Shared(state.clone()),
        }
    }
    pub fn next_bits(&mut self, bits: u32) -> u32 {
        assert!((1..=32).contains(&bits));
        let advance =
            |state: u64| state.wrapping_mul(Self::MULTIPLIER).wrapping_add(11) & Self::MASK;
        let state = match &mut self.state {
            State::Owned(state) => {
                *state = advance(*state);
                *state
            }
            State::Shared(state) => {
                // java.util.Random uses the same CAS transition. Callback
                // handles do not hold a lock while calling back into World.
                let old = state
                    .try_update(Ordering::Relaxed, Ordering::Relaxed, |old| {
                        Some(advance(old))
                    })
                    .unwrap();
                advance(old)
            }
        };
        (state >> (48 - bits)) as u32
    }
    pub fn next_i32(&mut self) -> i32 {
        self.next_bits(32) as i32
    }
    pub fn next_int(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "Java Random bound must be positive");
        if (bound as u32).is_power_of_two() {
            return ((bound as i64 * self.next_bits(31) as i64) >> 31) as i32;
        }
        loop {
            let bits = self.next_bits(31) as i32;
            let value = bits % bound;
            if bits.wrapping_sub(value).wrapping_add(bound - 1) >= 0 {
                return value;
            }
        }
    }
    pub fn next_long(&mut self) -> i64 {
        ((self.next_i32() as i64) << 32).wrapping_add(self.next_i32() as i64)
    }
    pub fn next_boolean(&mut self) -> bool {
        self.next_bits(1) != 0
    }
    pub fn next_float(&mut self) -> f32 {
        self.next_bits(24) as f32 / (1u32 << 24) as f32
    }
    pub fn next_double(&mut self) -> f64 {
        (((self.next_bits(26) as u64) << 27) + self.next_bits(27) as u64) as f64
            / (1u64 << 53) as f64
    }
    pub fn next_bytes(&mut self, output: &mut [u8]) {
        for group in output.chunks_mut(4) {
            let mut value = self.next_i32();
            for byte in group {
                *byte = value as u8;
                value >>= 8;
            }
        }
    }
}
