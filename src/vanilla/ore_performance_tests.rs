//! Exact access-order reference for the original WorldGenMinable loop.
use super::*;
use std::{cell::RefCell, collections::BTreeMap, hint::black_box, time::Instant};

// Retained verbatim pre-optimization traversal. The existing population corpus
// separately verifies its final geometry and Random state against Java 1.8.9.
fn original_minable_in(
    world: &mut impl FeatureWorld,
    random: &mut JavaRandom,
    p: IVec3,
    ore: u16,
    size: i32,
    replace_id: u16,
) -> bool {
    let angle = random.next_float() * std::f32::consts::PI;
    let a = [
        (p.x.wrapping_add(8) as f32 + math::sin(angle) * size as f32 / 8.) as f64,
        (p.y + random.next_int(3) - 2) as f64,
        (p.z.wrapping_add(8) as f32 + math::cos(angle) * size as f32 / 8.) as f64,
    ];
    let b = [
        (p.x.wrapping_add(8) as f32 - math::sin(angle) * size as f32 / 8.) as f64,
        (p.y + random.next_int(3) - 2) as f64,
        (p.z.wrapping_add(8) as f32 - math::cos(angle) * size as f32 / 8.) as f64,
    ];
    for step in 0..size {
        let fraction = step as f32 / size as f32;
        let center: [f64; 3] = std::array::from_fn(|i| a[i] + (b[i] - a[i]) * fraction as f64);
        let radius_random = random.next_double() * size as f64 / 16.;
        let radius =
            ((math::sin(std::f32::consts::PI * fraction) + 1.) as f64 * radius_random + 1.) / 2.;
        let low: [i32; 3] = center.map(|v| (v - radius).floor() as i32);
        let high: [i32; 3] = center.map(|v| (v + radius).floor() as i32);
        for x in low[0]..=high[0] {
            let dx = (x as f64 + 0.5 - center[0]) / radius;
            if dx * dx >= 1. { continue; }
            for y in low[1]..=high[1] {
                let dy = (y as f64 + 0.5 - center[1]) / radius;
                if dx * dx + dy * dy >= 1. { continue; }
                for z in low[2]..=high[2] {
                    let dz = (z as f64 + 0.5 - center[2]) / radius;
                    let p = IVec3::new(x, y, z);
                    if dx * dx + dy * dy + dz * dz < 1. && world.get(p) >> 4 == replace_id {
                        world.set(p, ore);
                    }
                }
            }
        }
    }
    true
}

#[derive(Debug, PartialEq)]
enum AccessEvent {
    Read([i32; 3], u16),
    Write([i32; 3], u16),
    ReentrantReseed(u64),
    CallbackWrite([i32; 3], u16),
}
#[derive(Default)]
struct Trace {
    reads: usize,
    blocks: BTreeMap<[i32; 3], u16>,
    events: Vec<AccessEvent>,
}
struct TraceWorld {
    replace_id: u16,
    trace: RefCell<Trace>,
    callback_random: RefCell<Option<JavaRandom>>,
}
fn valid(p: IVec3) -> bool {
    (0..256).contains(&p.y) && (-30_000_000..30_000_000).contains(&p.x)
        && (-30_000_000..30_000_000).contains(&p.z)
}
impl TraceWorld {
    fn initial(&self, p: IVec3) -> u16 {
        if !valid(p) { return 0; }
        match (p.x + p.y + p.z).rem_euclid(11) {
            0 => (self.replace_id << 4) | 3,
            1 => 0,
            2 => 4 << 4,
            _ => self.replace_id << 4,
        }
    }
}
impl FeatureWorld for TraceWorld {
    fn get(&self, p: IVec3) -> u16 {
        let mut trace = self.trace.borrow_mut();
        trace.reads += 1;
        // Neighbor publication can consume/reseed the shared provider Random
        // and mutate an already visited position during a feature read.
        if trace.reads % 127 == 31 {
            if let Some(random) = self.callback_random.borrow_mut().as_mut() {
                let seed = random.next_long() ^ trace.reads as i64;
                random.set_seed(seed);
                trace.events.push(AccessEvent::ReentrantReseed(random.state()));
                let neighbor = p - IVec3::X;
                if valid(neighbor) {
                    trace.blocks.insert(neighbor.to_array(), self.replace_id << 4);
                }
            }
        }
        let state = trace.blocks.get(&p.to_array()).copied().unwrap_or_else(|| self.initial(p));
        trace.events.push(AccessEvent::Read(p.to_array(), state));
        state
    }
    fn set(&mut self, p: IVec3, state: u16) {
        let mut trace = self.trace.borrow_mut();
        trace.events.push(AccessEvent::Write(p.to_array(), state));
        if valid(p) {
            trace.blocks.insert(p.to_array(), state);
            // A write callback can change a later candidate in the same row.
            // Pre-reading a row's states would miss this change.
            let next = p + IVec3::Z;
            if self.callback_random.borrow().is_some() && valid(next) {
                trace.blocks.insert(next.to_array(), 0);
                trace.events.push(AccessEvent::CallbackWrite(next.to_array(), 0));
            }
        }
    }
    fn biome(&self, _: IVec3) -> u8 { 1 }
    fn light(&self, _: IVec3, _: bool) -> u8 { 0 }
    fn chest(&mut self, _: IVec3, _: Vec<Option<LootStack>>) { panic!("ore cannot create a chest") }
    fn spawner(&mut self, _: IVec3, _: &'static str) { panic!("ore cannot create a spawner") }
    fn entity(&mut self, _: GeneratedEntity) { panic!("ore cannot create an entity") }
}

#[test]
fn cached_axis_math_preserves_every_read_write_and_reentrant_random_draw() {
    let cases = [
        (IVec3::new(0, 20, 0), 33, 1, (1 << 4) | 3),
        (IVec3::new(-16, 1, 15), 33, 1, 13 << 4),
        (IVec3::new(15, 254, -17), 17, 1, 16 << 4),
        (IVec3::new(-29_999_998, 40, 29_999_990), 9, 1, 56 << 4),
        (IVec3::new(29_999_990, 40, -29_999_998), 33, 1, 3 << 4),
        (IVec3::new(-9, 15, -9), 14, 87, 153 << 4),
        // Force several full 64-candidate dispatches within one sphere.
        (IVec3::new(7, 40, 7), 64, 1, (1 << 4) | 5),
        (IVec3::new(0, 8, 0), 1, 1, 21 << 4),
        (IVec3::new(0, 8, 0), 0, 1, 21 << 4),
        (IVec3::new(0, 8, 0), -1, 1, 21 << 4),
        // Some spheres exceed the 16-cell cache and use the unchanged fallback.
        (IVec3::new(-25, 70, 15), 129, 1, 14 << 4),
    ];
    let mut reseeds = 0;
    let mut callback_writes = 0;
    for seed in [0, 189, -1, 8924706420471469774] {
        for (p, size, replace_id, ore) in cases {
            for reentrant in [false, true] {
                let mut results = Vec::new();
                for reference in [true, false] {
                    let mut random = JavaRandom::new(seed);
                    let mut world = TraceWorld {
                        replace_id, trace: RefCell::new(Trace::default()),
                        callback_random: RefCell::new(reentrant.then(|| random.share())),
                    };
                    for offset in [IVec3::ZERO, IVec3::X] {
                        let success = if reference {
                            original_minable_in(&mut world, &mut random, p + offset, ore, size, replace_id)
                        } else {
                            minable_in(&mut world, &mut random, p + offset, ore, size, replace_id)
                        };
                        assert!(success);
                    }
                    results.push((world.trace.into_inner(), random.state()));
                }
                let [(expected, expected_rng), (actual, actual_rng)] = results.try_into().ok().unwrap();
                assert_eq!(expected_rng, actual_rng, "seed={seed} pos={p:?} size={size} reentrant={reentrant}");
                assert_eq!(expected.blocks, actual.blocks, "final ore states");
                assert_eq!(expected.events.len(), actual.events.len(), "access count");
                for (i, (expected, actual)) in expected.events.iter().zip(&actual.events).enumerate() {
                    assert_eq!(expected, actual, "access {i}: seed={seed} pos={p:?} size={size} reentrant={reentrant}");
                    reseeds += usize::from(matches!(expected, AccessEvent::ReentrantReseed(_)));
                    callback_writes += usize::from(matches!(expected, AccessEvent::CallbackWrite(_, _)));
                }
            }
        }
    }
    assert!(reseeds > 100, "fixture must exercise mid-vein shared RNG reseeding");
    assert!(callback_writes > 100, "fixture must mutate subsequent row candidates");
}

#[test]
fn cached_ore_batches_keep_original_candidates_and_rng_before_each_sphere() {
    struct Lines {
        batches: Vec<Vec<IVec3>>,
        random: RefCell<JavaRandom>,
        reads: RefCell<Vec<([i32; 3], u64)>>,
    }
    impl FeatureWorld for Lines {
        fn get(&self, p: IVec3) -> u16 {
            let mut reads = self.reads.borrow_mut();
            reads.push((p.to_array(), self.random.borrow().state()));
            if reads.len() % 127 == 31 {
                let mut random = self.random.borrow_mut();
                let seed = random.next_long() ^ reads.len() as i64;
                random.set_seed(seed);
            }
            16
        }
        fn set(&mut self, _: IVec3, _: u16) {}
        fn replace_ore_line(&mut self, positions: &[IVec3], ore: u16, replace_id: u16) {
            assert_eq!((ore, replace_id), (15 << 4, 1));
            assert!((1..=64).contains(&positions.len()));
            for pair in positions.windows(2) {
                assert!(pair[0].to_array() < pair[1].to_array(), "one sphere keeps X/Y/Z order");
            }
            self.batches.push(positions.to_vec());
            for &p in positions {
                if self.get(p) >> 4 == replace_id { self.set(p, ore); }
            }
        }
        fn biome(&self, _: IVec3) -> u8 { unreachable!() }
        fn light(&self, _: IVec3, _: bool) -> u8 { unreachable!() }
        fn chest(&mut self, _: IVec3, _: Vec<Option<LootStack>>) { unreachable!() }
        fn spawner(&mut self, _: IVec3, _: &'static str) { unreachable!() }
        fn entity(&mut self, _: GeneratedEntity) { unreachable!() }
    }
    let mut full_batches = 0;
    let mut multi_row_batches = 0;
    let mut cross_column_batches = 0;
    for (seed, p, size) in [(189, IVec3::new(7, 40, 7), 64),
        (-1, IVec3::new(-17, 1, -17), 64), (189, IVec3::new(7, 40, 7), 33)] {
        let mut reference_random = JavaRandom::new(seed);
        let mut actual_random = JavaRandom::new(seed);
        let mut reference = Lines { batches: Vec::new(), random: RefCell::new(reference_random.share()), reads: RefCell::new(Vec::new()) };
        let mut actual = Lines { batches: Vec::new(), random: RefCell::new(actual_random.share()), reads: RefCell::new(Vec::new()) };
        original_minable_in(&mut reference, &mut reference_random, p, 15 << 4, size, 1);
        minable_in(&mut actual, &mut actual_random, p, 15 << 4, size, 1);
        let candidates: Vec<_> = actual.batches.iter().flatten().map(|p| p.to_array()).collect();
        let expected = reference.reads.into_inner();
        assert_eq!(candidates, expected.iter().map(|&(p, _)| p).collect::<Vec<_>>());
        assert_eq!(actual.reads.into_inner(), expected,
            "callbacks must see the same RNG state, including before each next sphere radius");
        assert_eq!(actual_random.state(), reference_random.state());
        full_batches += actual.batches.iter().filter(|batch| batch.len() == 64).count();
        multi_row_batches += actual.batches.iter().filter(|batch| batch.windows(2)
            .any(|p| (p[0].x, p[0].y) != (p[1].x, p[1].y))).count();
        cross_column_batches += actual.batches.iter().filter(|batch| batch.windows(2)
            .any(|p| (p[0].x.div_euclid(16), p[0].z.div_euclid(16))
                != (p[1].x.div_euclid(16), p[1].z.div_euclid(16)))).count();
    }
    assert!(full_batches > 0, "fixture must force the fixed 64-candidate capacity");
    assert!(multi_row_batches > 0);
    assert!(cross_column_batches > 0);
}

// A cheap read adapter isolates geometry arithmetic. This does not stand in for
// the full native generation profile, which includes real block/light callbacks.
#[derive(Default)]
struct CountingWorld { reads: std::cell::Cell<u64>, writes: u64 }
impl FeatureWorld for CountingWorld {
    fn get(&self, p: IVec3) -> u16 {
        self.reads.set(self.reads.get().wrapping_add(1));
        black_box(p);
        1 << 4
    }
    fn set(&mut self, p: IVec3, state: u16) {
        self.writes = self.writes.wrapping_add(1);
        black_box((p, state));
    }
    fn biome(&self, _: IVec3) -> u8 { 1 }
    fn light(&self, _: IVec3, _: bool) -> u8 { 0 }
    fn chest(&mut self, _: IVec3, _: Vec<Option<LootStack>>) { unreachable!() }
    fn spawner(&mut self, _: IVec3, _: &'static str) { unreachable!() }
    fn entity(&mut self, _: GeneratedEntity) { unreachable!() }
}

#[test]
#[ignore = "isolated ore arithmetic microbenchmark; run alone with optimized tests"]
fn profile_original_and_cached_ore_geometry_with_identical_access_counts() {
    math::sin(0.); // Exclude the one-time original sine table initialization.
    let mut expected = None;
    let mut rows = Vec::new();
    for reference in [true, false, true, false] {
        let mut random = JavaRandom::new(189);
        let mut world = CountingWorld::default();
        let begin = Instant::now();
        for i in 0..20_000 {
            let p = IVec3::new((i % 37) * 16 - 288, (i % 80) + 1, (i % 41) * 16 - 320);
            let size = [33, 33, 33, 17, 9, 8][i as usize % 6];
            if reference { original_minable_in(&mut world, &mut random, p, 15 << 4, size, 1); }
            else { minable_in(&mut world, &mut random, p, 15 << 4, size, 1); }
        }
        let elapsed_ms = begin.elapsed().as_secs_f64() * 1000.;
        let outcome = (world.reads.get(), world.writes, random.state());
        if let Some(expected) = expected { assert_eq!(outcome, expected); }
        else { expected = Some(outcome); }
        let row = serde_json::json!({"reference":reference,"elapsed_ms":elapsed_ms,
            "veins":20_000,"reads":outcome.0,"writes":outcome.1,"random_state":outcome.2});
        eprintln!("{row}"); rows.push(row);
    }
    std::fs::create_dir_all("reports").unwrap();
    std::fs::write("reports/ore-axis-profile.json", serde_json::to_vec_pretty(&rows).unwrap()).unwrap();
}
