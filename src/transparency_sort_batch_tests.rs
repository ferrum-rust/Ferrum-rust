//! Independent frame-order and ownership oracles for bounded batch sorting.
use super::*;
use super::super::{DistanceKeys, Quad};

#[derive(Default)]
struct OriginalOrder {
    quads: Vec<Quad>, by_distance: Vec<(std::cmp::Reverse<u32>, usize)>,
    indices: Vec<u32>, eye: Option<Vec3>, mask: u16,
}
// Copied independently from the pre-optimization tuple algorithm. No batch
// admission, lane, production packed-key or temporal-coherence helper is used.
fn original_sort(order: &mut OriginalOrder, eye: Vec3, mask: u16) -> Sort {
    if order.quads.is_empty()
        || (order.mask == mask && order.eye.is_some_and(|p| p.distance_squared(eye) <= 1.))
    { return Sort::Skipped; }
    order.mask = mask; order.eye = Some(eye);
    order.by_distance.clear();
    order.by_distance.extend(order.quads.iter().enumerate()
        .filter(|(_, q)| mask & (1 << q.section) != 0)
        .map(|(i, q)| (std::cmp::Reverse(q.center.distance_squared(eye).to_bits()), i)));
    order.by_distance.sort_unstable();
    let indices = order.by_distance.iter().flat_map(|&(_, i)| order.quads[i].indices).collect::<Vec<_>>();
    let result = if order.indices == indices { Sort::Identical } else { Sort::Changed };
    order.indices = indices; result
}
fn order(count: usize, salt: usize, wide_keys: bool) -> Order {
    let bits = [0, 0x8000_0000, 0x7f80_0000, 0xff80_0000, 0x7fc0_0123, 0xff80_0001];
    let mut order = Order { quads: (0..count).map(|i| {
        let center = if i % 127 == 0 { Vec3::from_array([
            bits[(i + salt) % bits.len()], bits[(i + salt + 1) % bits.len()], 0].map(f32::from_bits)) }
            else { Vec3::new((i % 17) as f32 - 8., (i % 19) as f32, (i % 23) as f32 - salt as f32) };
        let base = (i as u32) * 4;
        Quad { center, indices: [base, base + 1, base + 2, base, base + 2, base + 3], section: (i % 16) as u8 }
    }).collect(), force_wide: wide_keys, ..Default::default() };
    order.by_distance.configure(count, wide_keys); order.by_distance.reserve(count);
    order.indices.reserve(count * 6); order.narrow_indices.reserve(count * 6);
    order
}
fn original(actual: &Order) -> OriginalOrder {
    OriginalOrder { quads: actual.quads.clone(), ..Default::default() }
}
fn allocations(order: &Order) -> (usize, usize, usize, usize, usize, usize, usize, usize, bool) {
    let (pointer, capacity, wide) = match &order.by_distance {
        DistanceKeys::Packed(keys) => (keys.as_ptr() as usize, keys.capacity(), false),
        DistanceKeys::Wide(keys) => (keys.as_ptr() as usize, keys.capacity(), true),
    };
    (order.quads.as_ptr() as usize, order.quads.capacity(), pointer, capacity,
        order.indices.as_ptr() as usize, order.indices.capacity(),
        order.narrow_indices.as_ptr() as usize, order.narrow_indices.capacity(), wide)
}
fn same(actual: &Order, expected: &OriginalOrder) {
    assert_eq!(actual.indices, expected.indices);
    assert_eq!(actual.by_distance.rows(), expected.by_distance);
    assert_eq!(actual.mask, expected.mask);
    assert_eq!(actual.eye.map(|v| v.to_array().map(f32::to_bits)),
        expected.eye.map(|v| v.to_array().map(f32::to_bits)));
    assert_eq!(actual.quads.len(), expected.quads.len());
    for (a, b) in actual.quads.iter().zip(&expected.quads) {
        assert_eq!(a.center.to_array().map(f32::to_bits), b.center.to_array().map(f32::to_bits));
        assert_eq!(a.indices, b.indices); assert_eq!(a.section, b.section);
    }
}

struct MemorySlots { rows: Vec<(Order, u16)>, takes: Vec<usize>, puts: Vec<usize> }
impl MemorySlots {
    fn new(jobs: usize, quads: usize) -> Self {
        Self { rows: (0..jobs).map(|i| (order(quads, i, i % 3 == 0), u16::MAX)).collect(),
            takes: Vec::new(), puts: Vec::new() }
    }
}
impl Slots for MemorySlots {
    fn len(&self) -> usize { self.rows.len() }
    fn order(&self, slot: usize) -> &Order { &self.rows[slot].0 }
    fn mask(&self, slot: usize) -> u16 { self.rows[slot].1 }
    fn take(&mut self, slot: usize) -> Order {
        self.takes.push(slot); std::mem::take(&mut self.rows[slot].0)
    }
    fn put(&mut self, slot: usize, order: Order) {
        // The only live Order was moved out; returning it must not overwrite
        // geometry/buffers installed by another concurrent owner.
        assert!(self.rows[slot].0.quads.is_empty());
        self.puts.push(slot); self.rows[slot].0 = order;
    }
}

fn batch(mode: Mode) -> Batch {
    let mut batch = Batch::new(mode);
    if mode.scoped() && batch.pool.is_none() {
        // Make correctness independent of the CI machine's logical CPU count.
        batch.pool = Some(rayon::ThreadPoolBuilder::new().num_threads(mode.helper_count()).build().unwrap());
        batch.pool_error = None;
    }
    batch
}
fn frame(enabled: bool) -> Frame { Frame { enabled, ..Default::default() } }

#[test]
fn ordinary_default_is_cpu_guarded_overrides_require_benchmark_and_capture_forces_serial() {
    use clap::Parser;
    let defaults = crate::Options::try_parse_from(["ferrum"]).unwrap();
    assert_eq!(defaults.benchmark_transparency_sort, None);
    let selected=defaults.benchmark_transparency_sort.unwrap_or_default();
    assert!(matches!(selected,Mode::ScopedWide|Mode::ScopedTwo));
    let mut ordinary=Batch::default();
    assert_eq!(ordinary.mode,selected);
    let report=ordinary.report();
    assert_eq!(report["mode"],if selected==Mode::ScopedTwo {"scoped-two"}else{"scoped-wide"});
    assert_eq!(report["configured_helpers"],selected.helper_count());
    assert_eq!(report["helper_threads"],ordinary.helper_threads());
    assert_eq!(report["ordinary_policy"]["two_minimum_available_logical_cpus"],16);
    assert_eq!(report["ordinary_policy"]["below_or_unknown_mode"],"scoped-wide");
    assert_eq!(report["job_capacity"],1024);
    // Broadcast visits EVERY worker. install() can choose either worker in Two,
    // and an inline pool object's address alone can survive its replacement.
    let helpers=|batch:&Batch|->std::collections::HashSet<std::thread::ThreadId> {
        batch.pool.as_ref().map(|p|p.broadcast(|_|std::thread::current().id())
            .into_iter().collect()).unwrap_or_default()
    };
    let worker_ids=helpers(&ordinary);
    assert_eq!(worker_ids.len(),ordinary.helper_threads());
    let storage=(ordinary.jobs.as_ptr(),ordinary.jobs.capacity(),ordinary.helper_threads());
    let pool_object=ordinary.pool.as_ref().map(|p|p as *const rayon::ThreadPool);
    ordinary.configure(defaults.benchmark_transparency_sort.unwrap_or_default());
    assert_eq!((ordinary.jobs.as_ptr(),ordinary.jobs.capacity(),ordinary.helper_threads()),storage);
    assert_eq!(ordinary.pool.as_ref().map(|p|p as *const rayon::ThreadPool),pool_object);
    assert_eq!(helpers(&ordinary),worker_ids,"default configure must retain the whole worker pool");
    for (text, mode) in [("original", Mode::Original), ("owner-batched", Mode::OwnerBatched), ("scoped-one", Mode::ScopedOne), ("scoped-large", Mode::ScopedLarge), ("scoped-wide", Mode::ScopedWide), ("scoped-two", Mode::ScopedTwo)] {
        assert!(crate::Options::try_parse_from(["ferrum", "--benchmark-transparency-sort", text]).is_err());
        let parsed = crate::Options::try_parse_from(["ferrum", "--benchmark", "--benchmark-transparency-sort", text]).unwrap();
        assert_eq!(parsed.benchmark_transparency_sort, Some(mode));
        assert_eq!(parsed.benchmark_transparency_sort.unwrap_or_default(),mode,
            "an explicit override must bypass the ordinary CPU policy");
        let captured = crate::Options::try_parse_from(["ferrum", "--benchmark", "--benchmark-streaming",
            "--benchmark-capture-transparency", "--benchmark-transparency-sort", text]).unwrap();
        let explicit=batch(captured.benchmark_transparency_sort.unwrap());
        assert_eq!(explicit.mode,mode);assert_eq!(explicit.report()["mode"],text);
        assert_eq!(explicit.report()["configured_helpers"],mode.helper_count());
        assert!(!explicit.wants_batch(captured.benchmark_capture_transparency));
    }
    assert!(crate::Options::try_parse_from(["ferrum", "--benchmark", "--benchmark-transparency-sort", "unknown"]).is_err());
}

#[test]
fn admission_boundaries_capture_selection_and_unavailable_pool_never_take_an_order() {
    for (count, quads, expected) in [
        (0, 0, Admission::Small), (1, 32768, Admission::Small), (31, 1058, Admission::Small),
        (32, 1023, Admission::Small), (32, 1024, Admission::Ready { jobs: 32, quads: 32768 }),
        (512, 64, Admission::Ready { jobs: 512, quads: 32768 }), (513, 64, Admission::Limit),
        (32, 8192, Admission::Ready { jobs: 32, quads: 262144 }), (32, 8193, Admission::Limit),
    ] {
        let mut slots = MemorySlots::new(count, quads);
        assert_eq!(admission(&slots, Vec3::ZERO, Mode::ScopedOne), expected);
        let expected_large = match expected {
            Admission::Ready { quads, .. } if quads < LARGE_MIN_QUADS => Admission::Small,
            unchanged => unchanged,
        };
        assert_eq!(admission(&slots, Vec3::ZERO, Mode::ScopedLarge), expected_large);
        if !matches!(expected, Admission::Ready { .. }) {
            let before = slots.rows.iter().map(|(o, _)| allocations(o)).collect::<Vec<_>>();
            let mut worker = batch(Mode::OwnerBatched);
            assert!(!worker.run_slots(&mut slots, Vec3::ZERO, &mut frame(false)));
            assert!(slots.takes.is_empty() && slots.puts.is_empty() && worker.jobs.is_empty());
            assert_eq!(slots.rows.iter().map(|(o, _)| allocations(o)).collect::<Vec<_>>(), before);
        }
    }
    let mut slots = MemorySlots::new(32, 1024);
    let mut original = batch(Mode::Original);
    assert!(!original.wants_batch(false)); assert!(!original.wants_batch(true));
    assert!(!original.run_slots(&mut slots, Vec3::ZERO, &mut frame(true)));
    assert!(slots.takes.is_empty());
    for mode in [Mode::OwnerBatched, Mode::ScopedOne, Mode::ScopedLarge, Mode::ScopedWide, Mode::ScopedTwo] {
        let mut candidate = batch(mode);
        assert!(candidate.wants_batch(false)); assert!(!candidate.wants_batch(true));
        if mode.scoped() {
            candidate.pool = None; let mut profile = frame(true);
            assert!(!candidate.wants_batch(false),"unavailable helper must select the original preparation path");
            assert!(!candidate.run_slots(&mut slots, Vec3::ZERO, &mut profile));
            assert_eq!(profile.counters.transparency_batch_unavailable, 1);
            assert!(slots.takes.is_empty() && candidate.jobs.is_empty());
        }
    }
}

fn original_writes(expected: &[OriginalOrder], results: &[Sort]) -> Vec<(usize, u64, Vec<u8>)> {
    let mut writes = Vec::new();
    for (slot, (order, result)) in expected.iter().zip(results).enumerate() {
        if *result != Sort::Changed || order.indices.is_empty() { continue; }
        let wide = slot % 2 != 0; let stride = if wide { 4 } else { 2 };
        let mut bytes = Vec::new();
        for &index in &order.indices {
            if wide { bytes.extend(index.to_ne_bytes()); }
            else { bytes.extend(u16::try_from(index).unwrap().to_ne_bytes()); }
        }
        if bytes.len() % 4 != 0 { bytes.extend([0, 0]); }
        writes.push((slot, (slot as u64 * 6) * stride, bytes));
    }
    writes
}
fn actual_writes(slots: &mut MemorySlots, results: &[Sort]) -> Vec<(usize, u64, Vec<u8>)> {
    let mut writes = Vec::new();
    for (slot, ((order, _), result)) in slots.rows.iter_mut().zip(results).enumerate() {
        if *result != Sort::Changed || order.indices.is_empty() { continue; }
        let wide = slot % 2 != 0; let stride = if wide { 4 } else { 2 };
        let bytes = if wide { bytemuck::cast_slice(&order.indices).to_vec() } else {
            crate::renderer::geometry::narrow_into(&order.indices, &mut order.narrow_indices);
            bytemuck::cast_slice(&order.narrow_indices).to_vec()
        };
        writes.push((slot, (slot as u64 * 6) * stride, bytes));
    }
    writes
}

#[test]
fn original_owner_and_scoped_frames_match_tuple_order_restoration_and_gpu_payload_sequence() {
    for mode in [Mode::OwnerBatched, Mode::ScopedOne, Mode::ScopedTwo] {
        let mut candidate = batch(mode);
        let quads=if mode==Mode::ScopedTwo {3072}else{1024};
        let mut slots = MemorySlots::new(64, quads);
        for (slot, (order, _)) in slots.rows.iter_mut().enumerate() {
            if slot % 11 == 0 { *order = Order::default(); }
        }
        let mut expected = slots.rows.iter().map(|(o, _)| original(o)).collect::<Vec<_>>();
        for (slot, ((order, _), oracle)) in slots.rows.iter_mut().zip(&mut expected).enumerate() {
            if slot % 7 == 0 {
                assert_eq!(order.sort(Vec3::ZERO, u16::MAX), original_sort(oracle, Vec3::ZERO, u16::MAX));
            }
        }
        let pointer = candidate.jobs.as_ptr(); let capacity = candidate.jobs.capacity();
        assert!(capacity >= MAX_JOBS);
        let path = [(Vec3::ZERO, u16::MAX), (Vec3::ZERO, u16::MAX), (Vec3::X, u16::MAX),
            (Vec3::X * 1.0001, u16::MAX), (Vec3::ZERO, 0), (Vec3::X * 4., 0),
            (Vec3::splat(f32::NAN), 0xaaaa), (Vec3::splat(f32::INFINITY), u16::MAX),
            (Vec3::splat(f32::NEG_INFINITY), 0x5555), (Vec3::splat(-0.), u16::MAX),
            (Vec3::new(-8., 4., 11.), u16::MAX), (Vec3::new(8., 4., 11.), u16::MAX)];
        let (mut batched, mut fallback) = (0, 0);
        for (step, (eye, mask)) in path.into_iter().enumerate() {
            // Repartition one slot between frames; key width and geometry may
            // change independently without retaining a previous frame's eye.
            if step == 10 {
                slots.rows[5].0 = order(quads, 189, true);
                expected[5] = original(&slots.rows[5].0);
            }
            for (_, at) in &mut slots.rows { *at = mask; }
            let results = expected.iter_mut().map(|order| original_sort(order, eye, mask)).collect::<Vec<_>>();
            let selected = results.iter().enumerate().filter(|(_, r)| **r != Sort::Skipped)
                .map(|(slot, _)| slot).collect::<Vec<_>>();
            let allocations_before = slots.rows.iter().map(|(o, _)| allocations(o)).collect::<Vec<_>>();
            slots.takes.clear(); slots.puts.clear(); let mut profile = frame(step % 2 == 0);
            let accepted = candidate.run_slots(&mut slots, eye, &mut profile);
            let actual = if accepted {
                batched += 1;
                assert_eq!(slots.takes, selected); assert_eq!(slots.puts, selected);
                assert!(candidate.jobs.iter().all(|job| job.order.quads.is_empty() && job.order.indices.is_empty()));
                let mut actual = vec![Sort::Skipped; slots.rows.len()];
                for job in &candidate.jobs {
                    actual[job.slot] = job.result;
                    if !profile.enabled { assert_eq!(job.elapsed_ms, 0.); }
                }
                actual
            } else {
                fallback += 1;
                assert!(slots.takes.is_empty() && slots.puts.is_empty());
                slots.rows.iter_mut().map(|(o, m)| o.sort(eye, *m)).collect()
            };
            assert_eq!(actual, results);
            for ((order, _), oracle) in slots.rows.iter().zip(&expected) { same(order, oracle); }
            assert_eq!(actual_writes(&mut slots, &actual), original_writes(&expected, &results));
            assert_eq!(slots.rows.iter().map(|(o, _)| allocations(o)).collect::<Vec<_>>(), allocations_before);
            // CPU seam's stand-in for complete(): all state already returned,
            // and clearing result placeholders cannot drop a geometry buffer.
            candidate.jobs.clear();
            assert_eq!((candidate.jobs.as_ptr(), candidate.jobs.capacity()), (pointer, capacity));
        }
        assert!(batched >= 7 && fallback >= 2);
    }
}

#[test]
fn owner_and_helper_panics_join_then_restore_every_owned_order_and_reuse_job_capacity() {
    for mode in [Mode::OwnerBatched, Mode::ScopedOne] {
        for lane in [Lane::Owner, Lane::Worker] {
            if mode == Mode::OwnerBatched && lane == Lane::Worker { continue; }
            for after in [false, true] {
                let mut candidate = batch(mode); let mut slots = MemorySlots::new(40, 1024);
                let pointers = slots.rows.iter().map(|(o, _)| allocations(o)).collect::<Vec<_>>();
                let mut probe = slots.rows.iter().enumerate().map(|(slot, (o, mask))| Job {
                    slot, mask: *mask, order: order(o.quads.len(), slot, slot % 3 == 0),
                    result: Sort::Skipped, elapsed_ms: 0.,
                }).collect::<Vec<_>>();
                let (cut, main_left) = split(&probe);
                let target = if mode == Mode::OwnerBatched { 0 } else {
                    match (lane, main_left) { (Lane::Owner, true) | (Lane::Worker, false) => 0, _ => cut }
                };
                probe.clear();
                candidate.fault = Fault { lane: Some(lane), slot: target, after };
                let buffer = (candidate.jobs.as_ptr(), candidate.jobs.capacity());
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
                    candidate.run_slots(&mut slots, Vec3::ZERO, &mut frame(true))));
                assert!(result.is_err(), "{mode:?}/{lane:?}/after={after}");
                assert_eq!(slots.takes, (0..slots.rows.len()).collect::<Vec<_>>());
                assert_eq!(slots.puts, slots.takes);
                assert_eq!(slots.rows.iter().map(|(o, _)| allocations(o)).collect::<Vec<_>>(), pointers);
                assert!(candidate.jobs.is_empty());
                assert_eq!((candidate.jobs.as_ptr(), candidate.jobs.capacity()), buffer);
                // No pending helper can continue mutating a restored Order.
                // The next frame can safely start from the partial-but-owned
                // CPU state; rendering the interrupted frame never proceeds.
                candidate.fault = Fault::default(); slots.takes.clear(); slots.puts.clear();
                assert!(candidate.run_slots(&mut slots, Vec3::X * 16., &mut frame(false)));
                assert!(!slots.takes.is_empty());
                assert_eq!(slots.takes, slots.puts);
                candidate.jobs.clear();
            }
        }
    }
}

#[test]
fn scoped_large_exact_threshold_matches_original_indices_without_small_case_mutation() {
    let mut candidate = batch(Mode::ScopedLarge);
    assert_eq!(candidate.report()["min_quads"], 98304);
    for mode in [Mode::Original, Mode::OwnerBatched, Mode::ScopedOne] {
        assert_eq!(batch(mode).report()["min_quads"], 32768);
    }
    let mut slots = MemorySlots::new(32, 3072);
    let last = slots.rows.last_mut().unwrap().0.quads.pop().unwrap();
    let before = slots.rows.iter().map(|(o, _)| allocations(o)).collect::<Vec<_>>();
    assert_eq!(admission(&slots, Vec3::ZERO, Mode::ScopedOne), Admission::Ready { jobs: 32, quads: 98303 });
    assert_eq!(admission(&slots, Vec3::ZERO, Mode::ScopedLarge), Admission::Small);
    let mut profile = frame(true);
    assert!(!candidate.run_slots(&mut slots, Vec3::ZERO, &mut profile));
    assert_eq!(profile.counters.transparency_batch_small, 1);
    assert_eq!(profile.counters.transparency_batch_frames, 0);
    assert!(slots.takes.is_empty() && slots.puts.is_empty() && candidate.jobs.is_empty());
    assert_eq!(slots.rows.iter().map(|(o, _)| allocations(o)).collect::<Vec<_>>(), before);
    slots.rows.last_mut().unwrap().0.quads.push(last);
    assert_eq!(admission(&slots, Vec3::ZERO, Mode::ScopedLarge), Admission::Ready { jobs: 32, quads: 98304 });

    let mut expected = slots.rows.iter().map(|(o, _)| original(o)).collect::<Vec<_>>();
    let storage = slots.rows.iter().map(|(o, _)| allocations(o)).collect::<Vec<_>>();
    let jobs_storage = (candidate.jobs.as_ptr(), candidate.jobs.capacity());
    for (step, (eye, mask)) in [(Vec3::ZERO, u16::MAX), (Vec3::ZERO, u16::MAX),
        (Vec3::X * 4., u16::MAX), (Vec3::X * 4., 0xaaaa)].into_iter().enumerate()
    {
        for (_, at) in &mut slots.rows { *at = mask; }
        let results = expected.iter_mut().map(|o| original_sort(o, eye, mask)).collect::<Vec<_>>();
        slots.takes.clear(); slots.puts.clear();
        let mut profile = frame(true);
        let admitted = candidate.run_slots(&mut slots, eye, &mut profile);
        assert_eq!(admitted, step != 1);
        let actual = if admitted {
            assert_eq!(profile.counters.transparency_batch_frames, 1);
            assert_eq!(profile.counters.transparency_batch_jobs, 32);
            assert_eq!(profile.counters.transparency_batch_quads, 98304);
            assert!(profile.counters.transparency_batch_worker_jobs > 0);
            assert_eq!(slots.takes, (0..32).collect::<Vec<_>>());
            assert_eq!(slots.puts, slots.takes);
            let mut actual = vec![Sort::Skipped; 32];
            for job in &candidate.jobs { actual[job.slot] = job.result; }
            actual
        } else {
            assert!(slots.takes.is_empty() && slots.puts.is_empty());
            slots.rows.iter_mut().map(|(o, mask)| o.sort(eye, *mask)).collect()
        };
        assert_eq!(actual, results);
        for ((order, _), oracle) in slots.rows.iter().zip(&expected) { same(order, oracle); }
        assert_eq!(actual_writes(&mut slots, &actual), original_writes(&expected, &results));
        assert_eq!(slots.rows.iter().map(|(o, _)| allocations(o)).collect::<Vec<_>>(), storage);
        candidate.jobs.clear();
        assert_eq!((candidate.jobs.as_ptr(), candidate.jobs.capacity()), jobs_storage);
    }
}

// Admission only borrows metadata. Sharing immutable representative Order
// values lets exact1Mi-quad/1024-job boundaries be tested without constructing
// or sorting one million independent quads. Any ownership attempt is a bug.
struct LengthSlots { count:usize, repeated:Order, last:Order }
impl LengthSlots {
    fn new(count:usize, repeated:usize, last:usize)->Self {
        fn metadata(count:usize)->Order {
            Order {quads:vec![Quad {center:Vec3::ZERO,indices:[0;6],section:0};count],..Default::default()}
        }
        Self {count,repeated:metadata(repeated),last:metadata(last)}
    }
}
impl Slots for LengthSlots {
    fn len(&self)->usize {self.count}
    fn order(&self,slot:usize)->&Order {if slot+1==self.count {&self.last}else{&self.repeated}}
    fn mask(&self,_:usize)->u16 {u16::MAX}
    fn take(&mut self,_:usize)->Order {panic!("admission must not take an Order")}
    fn put(&mut self,_:usize,_:Order) {panic!("admission must not replace an Order")}
}

#[test]
fn scoped_wide_has_its_own_exact_limits_without_changing_existing_modes() {
    for mode in [Mode::Original,Mode::OwnerBatched,Mode::ScopedOne,Mode::ScopedLarge] {
        let report=batch(mode).report();
        assert_eq!(report["max_jobs"],512);assert_eq!(report["max_quads"],262144);
        assert_eq!(report["min_jobs"],32);
        assert_eq!(report["min_quads"],if mode==Mode::ScopedLarge {98304}else{32768});
        let slots=LengthSlots::new(512,512,512);
        assert_eq!(admission(&slots,Vec3::ZERO,mode),Admission::Ready{jobs:512,quads:262144});
        assert_eq!(admission(&LengthSlots::new(513,512,512),Vec3::ZERO,mode),Admission::Limit);
        assert_eq!(admission(&LengthSlots::new(32,8192,8193),Vec3::ZERO,mode),Admission::Limit);
    }
    let candidate=batch(Mode::ScopedWide);let report=candidate.report();
    assert_eq!(report["min_jobs"],32);assert_eq!(report["min_quads"],98304);
    assert_eq!(report["max_jobs"],1024);assert_eq!(report["max_quads"],1048576);
    assert_eq!(candidate.jobs.capacity(),1024);
    for (count,repeated,last,expected) in [
        (31,4096,4096,Admission::Small),
        (32,3072,3071,Admission::Small),
        (32,3072,3072,Admission::Ready{jobs:32,quads:98304}),
        (513,512,512,Admission::Ready{jobs:513,quads:262656}),
        (32,8192,8193,Admission::Ready{jobs:32,quads:262145}),
        (1024,1024,1024,Admission::Ready{jobs:1024,quads:1048576}),
        (1025,96,96,Admission::Limit),
        (32,32768,32769,Admission::Limit),
    ] {
        let mut slots=LengthSlots::new(count,repeated,last);
        assert_eq!(admission(&slots,Vec3::ZERO,Mode::ScopedWide),expected);
        if !matches!(expected,Admission::Ready{..}) {
            let mut candidate=batch(Mode::ScopedWide);let mut profile=frame(true);
            assert!(!candidate.run_slots(&mut slots,Vec3::ZERO,&mut profile));
            assert_eq!(profile.counters.transparency_batch_frames,0);
            assert_eq!(profile.counters.transparency_batch_limit,usize::from(expected==Admission::Limit));
            assert_eq!(profile.counters.transparency_batch_small,usize::from(expected==Admission::Small));
            assert!(candidate.jobs.is_empty());
        }
    }
}

#[test]
fn scoped_wide_really_sorts_544_jobs_above_old_quad_limit_with_exact_oracle_and_reuse() {
    let mut slots=MemorySlots::new(544,576);
    for mode in [Mode::OwnerBatched,Mode::ScopedOne,Mode::ScopedLarge] {
        let mut candidate=batch(mode);let mut profile=frame(true);
        assert!(!candidate.run_slots(&mut slots,Vec3::ZERO,&mut profile));
        assert_eq!(profile.counters.transparency_batch_limit,1);
        assert!(slots.takes.is_empty()&&slots.puts.is_empty()&&candidate.jobs.is_empty());
    }
    let mut candidate=batch(Mode::ScopedWide);
    let mut expected=slots.rows.iter().map(|(o,_)|original(o)).collect::<Vec<_>>();
    let storage=slots.rows.iter().map(|(o,_)|allocations(o)).collect::<Vec<_>>();
    let jobs_storage=(candidate.jobs.as_ptr(),candidate.jobs.capacity());
    for (step,(eye,mask)) in [(Vec3::ZERO,u16::MAX),(Vec3::ZERO,u16::MAX),
        (Vec3::X*4.,0xaaaa)].into_iter().enumerate()
    {
        for (_,at) in &mut slots.rows {*at=mask;}
        let results=expected.iter_mut().map(|o|original_sort(o,eye,mask)).collect::<Vec<_>>();
        slots.takes.clear();slots.puts.clear();let mut profile=frame(true);
        let admitted=candidate.run_slots(&mut slots,eye,&mut profile);
        assert_eq!(admitted,step!=1);
        let actual=if admitted {
            assert_eq!(profile.counters.transparency_batch_frames,1);
            assert_eq!(profile.counters.transparency_batch_jobs,544);
            assert_eq!(profile.counters.transparency_batch_quads,313344);
            assert!(profile.counters.transparency_batch_worker_jobs>0);
            assert_eq!(slots.takes,(0..544).collect::<Vec<_>>());assert_eq!(slots.puts,slots.takes);
            let mut values=vec![Sort::Skipped;544];
            for job in &candidate.jobs {values[job.slot]=job.result;}values
        }else{
            assert!(slots.takes.is_empty()&&slots.puts.is_empty());
            slots.rows.iter_mut().map(|(o,m)|o.sort(eye,*m)).collect()
        };
        assert_eq!(actual,results);
        for ((order,_),oracle) in slots.rows.iter().zip(&expected) {same(order,oracle);}
        assert_eq!(actual_writes(&mut slots,&actual),original_writes(&expected,&results));
        assert_eq!(slots.rows.iter().map(|(o,_)|allocations(o)).collect::<Vec<_>>(),storage);
        candidate.jobs.clear();
        assert_eq!((candidate.jobs.as_ptr(),candidate.jobs.capacity()),jobs_storage);
    }
}

#[path="transparency_sort_batch_two_tests.rs"]
mod scoped_two;
