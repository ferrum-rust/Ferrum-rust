use super::*;
use std::panic::{catch_unwind, AssertUnwindSafe};

fn enter_at(book: &mut Book, attempt: u64, kind: Kind, now: u64) -> u64 {
    let id = book.new_span(attempt, kind);
    assert_ne!(id, IGNORED);
    assert!(book.can_enter(attempt, id));
    book.enter(id, now);
    id
}
fn exit_at(book: &mut Book, attempt: u64, id: u64, now: u64) {
    assert!(book.can_exit(attempt, id));
    book.exit(id, now);
    assert!(book.close(id));
}
fn complete(book: &mut Book, frame: u64, public_ms: f64) -> u64 {
    let attempt = book.begin(frame, frame as f64 / 10.).unwrap();
    let core = enter_at(book, attempt, Kind::Core, 10_000);
    let wait = enter_at(book, attempt, Kind::Wait, 20_000);
    exit_at(book, attempt, wait, 40_000);
    let native = enter_at(book, attempt, Kind::Native, 50_000);
    exit_at(book, attempt, native, 80_000);
    exit_at(book, attempt, core, 110_000);
    book.end(attempt, Outcome::Success);
    book.public_elapsed(public_ms).unwrap();
    attempt
}

#[test]
fn nested_scopes_keep_exact_residuals_and_successful_present_anchors() {
    let mut book = Book::new();
    // First measured gate may have no attempted acquire: warmup crossed inside render.
    book.presented(None, None, 500.).unwrap();
    assert_eq!(book.unassociated_presents, 1);
    let a = complete(&mut book, 91, 0.125);
    book.presented(None, None, 501.).unwrap();
    let b = complete(&mut book, 94, 0.225);
    book.presented(Some(0), Some(501.), 503.).unwrap();
    assert_eq!(book.totals.attempts, 2);
    assert_eq!(book.totals.scope_complete, 2);
    assert_eq!(book.totals.ns, [200_000, 40_000, 60_000]);
    assert_eq!(book.totals.core_residual_ns, 100_000);
    assert!((book.totals.public_residual_ms - 0.15).abs() < 1e-12);
    assert_eq!((book.totals.presented, book.totals.first_present), (2, 1));
    let first = book.top.iter().flatten().find(|r| r.sequence == a).unwrap();
    let second = book.top.iter().flatten().find(|r| r.sequence == b).unwrap();
    assert_eq!((first.sample_index, first.begin_ms, first.end_ms), (None, None, Some(501.)));
    assert_eq!((second.frame_id, second.sample_index, second.begin_ms, second.end_ms),
        (94, Some(0), Some(501.), Some(503.)));
    assert!(first.presented && second.presented);
    let j = second.json();
    assert_eq!(j["scope_counts"], serde_json::json!([1, 1, 1]));
    assert!((j["core_residual_ms"].as_f64().unwrap() - 0.05).abs() < 1e-12);
    assert_eq!(book.lifecycle_errors, 0);
    assert!(book.slots.iter().all(|s| s.id == 0));

    // A failed acquisition cannot lend its frame identity to the next present.
    let failed = book.begin(95, 9.5).unwrap();
    book.end(failed, Outcome::Error);
    book.public_elapsed(0.01).unwrap();
    book.presented(Some(1), Some(503.), 504.).unwrap();
    assert_eq!((book.totals.native_errors, book.unassociated_presents), (1, 2));
    complete(&mut book, 96, 0.15);
    book.clear_pending();
    assert_eq!(book.unstamped, 1);
    book.presented(Some(2), Some(504.), 505.).unwrap();
    assert_eq!(book.unassociated_presents, 3);
}

#[test]
fn fixed_top32_matches_independent_stable_sort_without_growing_storage() {
    let mut book = Book::new();
    let pointers = (book.top.as_ptr(), book.slots.as_ptr(), book.stack.as_ptr());
    let mut expected = Vec::new();
    for frame in 0..113u64 {
        let ms = 0.2 + ((frame * 37) % 19) as f64 * 0.01;
        let sequence = complete(&mut book, frame, ms);
        book.presented(Some(frame as usize), Some(frame as f64), frame as f64 + 1.).unwrap();
        expected.push((sequence, ms));
    }
    // Standard stable sorting preserves older equal-duration observations.
    expected.sort_by(|a, b| b.1.total_cmp(&a.1));
    expected.truncate(32);
    let actual: Vec<_> = book.top.iter().flatten().map(|r| (r.sequence, r.public_ms.unwrap())).collect();
    assert_eq!(actual, expected);
    assert_eq!(book.totals.attempts, 113);
    assert_eq!(book.totals.scope_complete, 113);
    assert_eq!(book.totals.presented, 113);
    assert_eq!((book.top.as_ptr(), book.slots.as_ptr(), book.stack.as_ptr()), pointers);
    assert_eq!((book.top.len(), book.slots.len(), book.stack.len()), (32, 32, 8));
    for invalid in [f64::NAN, f64::INFINITY, -1.] {
        let attempt = book.begin(999, 99.).unwrap();
        book.end(attempt, Outcome::Success);
        assert!(book.public_elapsed(invalid).is_err());
    }
    assert_eq!(book.totals.invalid, 3);
    assert!(book.top.iter().flatten().all(|r| r.rank().is_finite()));
}

#[test]
fn clone_lifetimes_stale_ids_and_exhaustion_never_alias_a_new_attempt() {
    let mut book = Book::new();
    let attempt = book.begin(1, 1.).unwrap();
    let old = enter_at(&mut book, attempt, Kind::Core, 0);
    book.clone_id(old);
    assert!(book.can_exit(attempt, old)); book.exit(old, 1);
    assert!(!book.close(old));
    assert_eq!(book.slots.iter().find(|s| s.id == old).unwrap().refs, 1);
    book.end(attempt, Outcome::Success); book.public_elapsed(1.).unwrap();
    book.presented(None, None, 1.).unwrap();
    let next = book.begin(2, 2.).unwrap();
    let fresh = book.new_span(next, Kind::Core);
    assert!(fresh > old);
    assert!(!book.can_enter(next, old));
    assert!(book.close(old));
    assert!(book.can_enter(next, fresh)); book.enter(fresh, 2);
    exit_at(&mut book, next, fresh, 3);
    book.end(next, Outcome::Success); book.public_elapsed(1.).unwrap();
    assert_eq!(book.totals.invalid, 1);
    assert!(book.slots.iter().all(|s| s.id == 0));

    let mut full = Book::new(); let a = full.begin(3, 3.).unwrap();
    let ids: Vec<_> = (0..SLOTS).map(|_| full.new_span(a, Kind::Core)).collect();
    assert_eq!(full.new_span(a, Kind::Core), IGNORED);
    assert_eq!(full.slot_overflow, 1);
    for id in ids { assert!(full.close(id)); }
    assert!(full.new_span(a, Kind::Core) > SLOTS as u64);
    let mut serial = Book::new(); serial.next_span = u64::MAX;
    let a = serial.begin(4, 4.).unwrap();
    assert_eq!(serial.new_span(a, Kind::Core), IGNORED);
    assert_eq!(serial.serial_exhaustion, 1);
    assert!(serial.slots.iter().all(|s| s.id == 0));
    let mut attempts = Book::new(); attempts.next_attempt = u64::MAX;
    assert!(attempts.begin(5, 5.).is_none());
    assert_eq!(attempts.serial_exhaustion, 1);
    assert!(attempts.active.is_none());
}

#[test]
fn invalid_nesting_clock_reversal_and_bad_anchors_stay_explicit() {
    let mut book = Book::new(); let a = book.begin(1, 1.).unwrap();
    let orphan = book.new_span(a, Kind::Wait);
    assert!(!book.can_enter(a, orphan)); assert!(book.close(orphan));
    let core = enter_at(&mut book, a, Kind::Core, 100);
    assert!(!book.can_enter(a, core));
    let premature_native = book.new_span(a, Kind::Native);
    assert!(!book.can_enter(a, premature_native));
    assert!(book.close(premature_native));
    let wait = enter_at(&mut book, a, Kind::Wait, 110);
    assert!(!book.can_exit(a, core)); // Out-of-order exit must not pop child.
    assert_eq!(book.depth, 2);
    assert!(book.can_exit(a, wait)); book.exit(wait, 109); // No unsigned wrap.
    assert!(book.close(wait));
    exit_at(&mut book, a, core, 150);
    book.end(a, Outcome::Success); book.public_elapsed(1.).unwrap();
    assert_eq!(book.totals.invalid, 1);
    assert_eq!(book.totals.ns, [50, 0, 0]);
    assert_eq!(book.top[0].unwrap().json()["core_residual_ms"], serde_json::Value::Null);
    assert!(book.presented(Some(0), None, 2.).is_err());
    assert_eq!(book.totals.presented, 0);
    complete(&mut book, 3, 0.01); // Public wrapper cannot be shorter than core.
    assert!(book.top.iter().flatten().any(|r| r.frame_id == 3 && r.invalid));
}

fn fake_shared() -> (Arc<Shared>, Arc<(AtomicU64, AtomicU64)>) {
    let fake = Arc::new((AtomicU64::new(100), AtomicU64::new(0)));
    let id = NEXT_CAPTURE.fetch_add(1, Ordering::Relaxed);
    let shared = Arc::new(Shared { id, book: Mutex::new(Book::new()),
        clock: Clock { epoch: Instant::now(), fake: Some(Arc::clone(&fake)) },
        foreign: AtomicU64::new(0), contention: AtomicU64::new(0), reentry: AtomicU64::new(0),
        registered_mask: AtomicU64::new(0) });
    (shared, fake)
}
struct ContextReset(Option<(u64, u64)>);
impl Drop for ContextReset { fn drop(&mut self) { CONTEXT.with(|c| c.set(self.0)); } }

#[test]
fn rejected_callbacks_foreign_thread_and_contention_read_no_clock() {
    let (shared, fake) = fake_shared(); let sink = Sink { shared: Arc::clone(&shared) };
    let a = shared.book.lock().unwrap().begin(1, 1.).unwrap();
    let raw = shared.book.lock().unwrap().new_span(a, Kind::Core); let id = Id::from_u64(raw);
    let _reset = ContextReset(CONTEXT.with(|c| c.replace(None)));
    sink.enter(&id); sink.exit(&id); sink.enter(&Id::from_u64(IGNORED));
    assert_eq!(fake.1.load(Ordering::Relaxed), 0);
    CONTEXT.with(|c| c.set(Some((shared.id, a))));
    {
        let _held = shared.book.lock().unwrap(); sink.enter(&id);
        assert_eq!(fake.1.load(Ordering::Relaxed), 0);
    }
    assert_eq!(shared.contention.load(Ordering::Relaxed), 1);
    let foreign = Arc::clone(&shared); let foreign_id = id.clone();
    std::thread::spawn(move || Sink { shared: foreign }.enter(&foreign_id)).join().unwrap();
    assert_eq!(fake.1.load(Ordering::Relaxed), 0);
    sink.enter(&id); assert_eq!(fake.1.load(Ordering::Relaxed), 1);
    sink.enter(&id); assert_eq!(fake.1.load(Ordering::Relaxed), 1);
    sink.exit(&Id::from_u64(raw + 999)); assert_eq!(fake.1.load(Ordering::Relaxed), 1);
    fake.0.store(250, Ordering::Relaxed); sink.exit(&id);
    assert_eq!(fake.1.load(Ordering::Relaxed), 2);
    assert!(sink.try_close(id));
    assert!(shared.foreign.load(Ordering::Relaxed) >= 3);
    assert_eq!(shared.book.lock().unwrap().active.as_ref().unwrap().ns[0], 150);
}

#[test]
fn acquire_preserves_results_unwind_and_previous_thread_context() {
    let (shared, fake) = fake_shared();
    let dispatch = Dispatch::new(Sink { shared: Arc::clone(&shared) });
    let mut capture = Capture { shared: Arc::clone(&shared), dispatch,
        registered_before_first_attempt: None, warmup_complete_before_first_attempt: None,
        warmup_interest_rebuilt: false, warmup_interest_rebuilds_before_first_attempt: None };
    let previous = CONTEXT.with(|c| c.replace(None)); let _reset = ContextReset(previous);
    let value = Box::new(123u64); let pointer = (&*value) as *const u64;
    let result: Result<Box<u64>, &'static str> = capture.acquire(1, 1., || Ok(value));
    let returned = result.unwrap(); assert_eq!((&*returned) as *const u64, pointer);
    capture.public_elapsed(0.1).unwrap(); capture.presented(None, None, 1.).unwrap();
    let error: Result<(), &'static str> = capture.acquire(2, 2., || Err("original native error"));
    assert_eq!(error, Err("original native error")); capture.public_elapsed(0.2).unwrap();
    assert!(CONTEXT.with(Cell::get).is_none());
    let panicked = catch_unwind(AssertUnwindSafe(|| {
        let _: Result<(), ()> = capture.acquire(3, 3., || panic!("original acquisition panic"));
    }));
    assert!(panicked.is_err()); assert!(CONTEXT.with(Cell::get).is_none());
    assert!(shared.book.lock().unwrap().active.is_none());
    assert_eq!(shared.book.lock().unwrap().totals.unwinds, 1);
    CONTEXT.with(|c| c.set(Some((u64::MAX, 7))));
    let before = shared.book.lock().unwrap().totals.attempts;
    let nested: Result<u32, ()> = capture.acquire(4, 4., || Ok(9));
    assert_eq!(nested, Ok(9));
    assert_eq!(CONTEXT.with(Cell::get), Some((u64::MAX, 7)));
    assert_eq!(shared.book.lock().unwrap().totals.attempts, before);
    assert_eq!(shared.reentry.load(Ordering::Relaxed), 1);
    // Context/result bookkeeping itself never calls the scope clock.
    assert_eq!(fake.1.load(Ordering::Relaxed), 0);
    assert_eq!(capture.report()["warmup_interest_rebuilds"], 0);
    assert_eq!(capture.report()["warmup_interest_rebuilds_before_first_attempt"], 0);
}

#[test]
fn three_pinned_callsite_filters_are_static_and_do_not_accept_events() {
    for (name, target, file, line, kind) in [
        ("Surface::get_current_texture", "wgpu_core::present", "wgpu-core-24.0.5/src/present.rs", 121, Kind::Core),
        ("Device::wait", "wgpu_hal::vulkan::device", "wgpu-hal-24.0.4/src/vulkan/device.rs", 2598, Kind::Wait),
        ("vkAcquireNextImageKHR", "wgpu_hal::vulkan::instance", "wgpu-hal-24.0.4/src/vulkan/instance.rs", 1044, Kind::Native),
    ] {
        for path in [format!("/registry/{file}"), format!("C:\\registry\\{}", file.replace('/', "\\"))] {
            let previous = CONTEXT.with(|c| c.replace(None)); let _reset = ContextReset(previous);
            assert_eq!(filter_parts(name, target, Some(&path), Some(line), true), Some(kind));
            CONTEXT.with(|c| c.set(Some((123, 456))));
            assert_eq!(filter_parts(name, target, Some(&path), Some(line), true), Some(kind));
            assert_eq!(filter_parts(name, target, Some(&path), Some(line), false), None);
            assert_eq!(filter_parts(name, target, Some(&path), Some(line + 1), true), None);
            assert_eq!(filter_parts(name, "other::module", Some(&path), Some(line), true), None);
            assert_eq!(filter_parts("wrong name", target, Some(&path), Some(line), true), None);
        }
        assert_eq!(filter_parts(name, target, None, Some(line), true), None);
        assert_eq!(filter_parts(name, target, Some(file), None, true), None);
        assert_eq!(filter_parts(name, target, Some(file.rsplit('/').next().unwrap()), Some(line), true), None);
        assert_eq!(filter_parts(name, target, Some(&format!("/registry/x{file}")), Some(line), true), None);
    }
}

struct SyntheticCallsite;
static SYNTHETIC_CALLSITE: SyntheticCallsite = SyntheticCallsite;
impl tracing::callsite::Callsite for SyntheticCallsite {
    fn set_interest(&self, _: Interest) {}
    fn metadata(&self) -> &'static Metadata<'static> { &SYNTHETIC_CORE }
}
static SYNTHETIC_CORE: Metadata<'static> = Metadata::new(
    "Surface::get_current_texture", "wgpu_core::present", tracing::Level::INFO,
    Some("/registry/wgpu-core-24.0.5/src/present.rs"), Some(121), Some("wgpu_core::present"),
    tracing::field::FieldSet::new(&[], tracing::callsite::Identifier(&SYNTHETIC_CALLSITE)),
    tracing::metadata::Kind::SPAN);
static SYNTHETIC_UNRELATED: Metadata<'static> = Metadata::new(
    "Surface::get_current_texture", "unrelated::present", tracing::Level::INFO,
    Some("/registry/wgpu-core-24.0.5/src/present.rs"), Some(121), Some("unrelated::present"),
    tracing::field::FieldSet::new(&[], tracing::callsite::Identifier(&SYNTHETIC_CALLSITE)),
    tracing::metadata::Kind::SPAN);
static SYNTHETIC_WRONG_LEVEL: Metadata<'static> = Metadata::new(
    "Surface::get_current_texture", "wgpu_core::present", tracing::Level::DEBUG,
    Some("/registry/wgpu-core-24.0.5/src/present.rs"), Some(121), Some("wgpu_core::present"),
    tracing::field::FieldSet::new(&[], tracing::callsite::Identifier(&SYNTHETIC_CALLSITE)),
    tracing::metadata::Kind::SPAN);
static SYNTHETIC_WAIT: Metadata<'static> = Metadata::new(
    "Device::wait", "wgpu_hal::vulkan::device", tracing::Level::INFO,
    Some("/registry/wgpu-hal-24.0.4/src/vulkan/device.rs"), Some(2598), Some("wgpu_hal::vulkan::device"),
    tracing::field::FieldSet::new(&[], tracing::callsite::Identifier(&SYNTHETIC_CALLSITE)),
    tracing::metadata::Kind::SPAN);
static SYNTHETIC_NATIVE: Metadata<'static> = Metadata::new(
    "vkAcquireNextImageKHR", "wgpu_hal::vulkan::instance", tracing::Level::INFO,
    Some("/registry/wgpu-hal-24.0.4/src/vulkan/instance.rs"), Some(1044), Some("wgpu_hal::vulkan::instance"),
    tracing::field::FieldSet::new(&[], tracing::callsite::Identifier(&SYNTHETIC_CALLSITE)),
    tracing::metadata::Kind::SPAN);

fn synthetic_scope_pairs(fake: &Arc<(AtomicU64, AtomicU64)>) {
    // Route through the actual thread-local Dispatch installed by Capture.
    tracing::dispatcher::get_default(|sink| {
    let new = |meta: &'static Metadata<'static>| {
        let values = meta.fields().value_set_all(&[]);
        let id = sink.new_span(&Attributes::new_root(meta, &values));
        assert_ne!(id.into_u64(), IGNORED); id
    };
    fake.0.store(1_000_000, Ordering::Relaxed);
    let core = new(&SYNTHETIC_CORE); sink.enter(&core);
    let wait = new(&SYNTHETIC_WAIT); sink.enter(&wait);
    fake.0.store(2_000_000, Ordering::Relaxed); sink.exit(&wait); assert!(sink.try_close(wait));
    let native = new(&SYNTHETIC_NATIVE); sink.enter(&native);
    fake.0.store(3_000_000, Ordering::Relaxed); sink.exit(&native); assert!(sink.try_close(native));
    fake.0.store(4_000_000, Ordering::Relaxed); sink.exit(&core); assert!(sink.try_close(core));
    });
}

#[test]
fn real_warmup_scope_callbacks_prove_coverage_without_reading_clock_or_polluting_measured_rows() {
    let (shared, fake) = fake_shared(); let sink = Sink { shared: Arc::clone(&shared) };
    for meta in [&SYNTHETIC_CORE, &SYNTHETIC_WAIT, &SYNTHETIC_NATIVE] {
        assert!(sink.register_callsite(meta).is_always());
    }
    let dispatch = Dispatch::new(Sink { shared: Arc::clone(&shared) });
    let mut capture = Capture { shared: Arc::clone(&shared), dispatch,
        registered_before_first_attempt: None, warmup_complete_before_first_attempt: None,
        warmup_interest_rebuilt: false, warmup_interest_rebuilds_before_first_attempt: None };
    let previous = CONTEXT.with(|c| c.replace(None)); let _reset = ContextReset(previous);
    let value = capture.warmup_acquire(8, 0.5, || {
        synthetic_scope_pairs(&fake); Ok::<_, &'static str>(17)
    });
    assert_eq!(value, Ok(17)); assert_eq!(fake.1.load(Ordering::Relaxed), 0);
    {
        let book = shared.book.lock().unwrap();
        assert_eq!((book.warmup.attempts, book.warmup.success, book.warmup.successful_scope_complete), (1, 1, 1));
        assert_eq!(book.warmup.scope_counts, [1, 1, 1]);
        assert_eq!(book.totals.attempts, 0);
        assert!(book.top.iter().all(Option::is_none));
        assert!(book.active.is_none() && book.await_public.is_none() && book.pending_present.is_none());
        assert!(book.slots.iter().all(|s| s.id == 0));
    }
    let error: Result<(), &'static str> = capture.warmup_acquire(9, 0.6, || Err("original warmup error"));
    assert_eq!(error, Err("original warmup error"));
    assert_eq!(fake.1.load(Ordering::Relaxed), 0);
    capture.acquire(10, 1., || { synthetic_scope_pairs(&fake); Ok::<_, ()>(()) }).unwrap();
    assert_eq!(fake.1.load(Ordering::Relaxed), 6);
    capture.public_elapsed(4.).unwrap(); capture.presented(None, None, 1000.).unwrap();
    let report = capture.report();
    assert_eq!(report["warmup_successful_scope_complete_before_first_attempt"], 1);
    assert_eq!(report["warmup_interest_rebuilds"], 1);
    assert_eq!(report["warmup_interest_rebuilds_before_first_attempt"], 1);
    assert_eq!(report["registered_scope_mask_before_first_attempt"], 7);
    assert_eq!(report["warmup"]["attempts"], 2);
    assert_eq!(report["warmup"]["native_errors"], 1);
    assert_eq!(report["warmup"]["callback_timing_enabled"], false);
    assert_eq!(report["attempts"], 1);
    assert_eq!(report["scope_ms_totals"], serde_json::json!([3., 1., 1.]));
    assert_eq!(report["top"].as_array().unwrap().len(), 1);
    assert_eq!(report["top"][0]["sequence"], 3);
    assert_eq!(report["top"][0]["renderer_frame_id"], 10);
    assert!(CONTEXT.with(Cell::get).is_none());
}

#[test]
fn subscriber_interest_is_static_but_span_creation_rechecks_context_and_metadata() {
    let (shared, fake) = fake_shared(); let sink = Sink { shared: Arc::clone(&shared) };
    let previous = CONTEXT.with(|c| c.replace(None)); let _reset = ContextReset(previous);
    assert!(sink.register_callsite(&SYNTHETIC_CORE).is_always());
    assert!(sink.register_callsite(&SYNTHETIC_UNRELATED).is_never());
    assert!(sink.register_callsite(&SYNTHETIC_WRONG_LEVEL).is_never());
    assert!(sink.enabled(&SYNTHETIC_CORE));
    assert!(!sink.enabled(&SYNTHETIC_WRONG_LEVEL));
    assert_eq!(shared.registered_mask.load(Ordering::Relaxed), 1);
    let values = SYNTHETIC_CORE.fields().value_set_all(&[]);
    let attrs = Attributes::new_root(&SYNTHETIC_CORE, &values);
    assert_eq!(sink.new_span(&attrs).into_u64(), IGNORED);
    let a = shared.book.lock().unwrap().begin(1, 1.).unwrap();
    CONTEXT.with(|c| c.set(Some((shared.id, a))));
    assert!(sink.register_callsite(&SYNTHETIC_CORE).is_always());
    let ignored_values = SYNTHETIC_UNRELATED.fields().value_set_all(&[]);
    let ignored_attrs = Attributes::new_root(&SYNTHETIC_UNRELATED, &ignored_values);
    // tracing may still deliver a span from an Interest::never site when another
    // subscriber was interested. The callback must not trust registration alone.
    assert_eq!(sink.new_span(&ignored_attrs).into_u64(), IGNORED);
    let id = sink.new_span(&attrs);
    assert_ne!(id.into_u64(), IGNORED);
    assert_eq!(fake.1.load(Ordering::Relaxed), 0);
    sink.enter(&id); fake.0.store(175, Ordering::Relaxed); sink.exit(&id);
    assert!(sink.try_close(id));
    assert_eq!(fake.1.load(Ordering::Relaxed), 2);
    assert_eq!(shared.book.lock().unwrap().active.as_ref().unwrap().ns[0], 75);
}

// A unique registered callsite with controllable cached interest. The real
// single-Dispatch outside->inside macro failure is reproduced separately in an
// isolated process: parallel normal tests must not assume a global Dispatch count.
struct RebuildWaitCallsite { interest: AtomicU64 }
static REBUILD_WAIT_CALLSITE: RebuildWaitCallsite = RebuildWaitCallsite { interest: AtomicU64::new(0) };
impl tracing::callsite::Callsite for RebuildWaitCallsite {
    fn set_interest(&self, interest: Interest) {
        self.interest.store(if interest.is_always() {2} else if interest.is_never() {0} else {1}, Ordering::SeqCst);
    }
    fn metadata(&self) -> &'static Metadata<'static> { &REBUILD_WAIT_METADATA }
}
static REBUILD_WAIT_METADATA: Metadata<'static> = Metadata::new(
    "Device::wait", "wgpu_hal::vulkan::device", tracing::Level::INFO,
    Some("/registry/wgpu-hal-24.0.4/src/vulkan/device.rs"), Some(2598), Some("wgpu_hal::vulkan::device"),
    tracing::field::FieldSet::new(&[], tracing::callsite::Identifier(&REBUILD_WAIT_CALLSITE)),
    tracing::metadata::Kind::SPAN);

fn rebuild_registered_scope_pairs(fake: &Arc<(AtomicU64, AtomicU64)>) {
    tracing::dispatcher::get_default(|sink| {
        let enter = |meta: &'static Metadata<'static>| {
            let values = meta.fields().value_set_all(&[]);
            let id = sink.new_span(&Attributes::new_root(meta, &values));
            assert_ne!(id.into_u64(), IGNORED); sink.enter(&id); id
        };
        fake.0.store(1_000_000, Ordering::Relaxed);
        let core = enter(&SYNTHETIC_CORE);
        // Match tracing's cached-interest decision before creating the selected
        // wait span; a stale never cache cannot be bypassed by direct dispatch.
        let interest = REBUILD_WAIT_CALLSITE.interest.load(Ordering::SeqCst);
        assert!(interest == 2 || interest == 1 && sink.enabled(&REBUILD_WAIT_METADATA));
        let wait = enter(&REBUILD_WAIT_METADATA);
        fake.0.store(2_000_000, Ordering::Relaxed); sink.exit(&wait); assert!(sink.try_close(wait));
        let native = enter(&SYNTHETIC_NATIVE);
        fake.0.store(3_000_000, Ordering::Relaxed); sink.exit(&native); assert!(sink.try_close(native));
        fake.0.store(4_000_000, Ordering::Relaxed); sink.exit(&core); assert!(sink.try_close(core));
    });
}

#[test]
fn first_warmup_rebuild_recovers_registered_never_interest_before_native_closure() {
    tracing::callsite::register(&REBUILD_WAIT_CALLSITE);
    let (shared, fake) = fake_shared();
    let dispatch = Dispatch::new(Sink { shared: Arc::clone(&shared) });
    let mut capture = Capture { shared: Arc::clone(&shared), dispatch,
        registered_before_first_attempt: None, warmup_complete_before_first_attempt: None,
        warmup_interest_rebuilt: false, warmup_interest_rebuilds_before_first_attempt: None };
    let previous = CONTEXT.with(|c| c.replace(None)); let _reset = ContextReset(previous);
    // Seed the exact cached-never state deterministically, without assuming the
    // normal harness has only one live subscriber or widening pinned metadata.
    REBUILD_WAIT_CALLSITE.interest.store(0, Ordering::SeqCst);
    assert_eq!(capture.report()["warmup_interest_rebuilds"], 0);
    assert!(capture.report()["warmup_interest_rebuilds_before_first_attempt"].is_null());
    for frame in [8, 9] {
        assert_eq!(capture.warmup_acquire(frame, 0.5, || {
            rebuild_registered_scope_pairs(&fake); Ok::<_, &'static str>(frame)
        }), Ok(frame));
        assert_eq!(fake.1.load(Ordering::Relaxed), 0);
        assert_eq!(capture.report()["warmup_interest_rebuilds"], 1);
        assert!(capture.report()["warmup_interest_rebuilds_before_first_attempt"].is_null());
    }
    capture.acquire(10, 1., || { rebuild_registered_scope_pairs(&fake); Ok::<_, ()>(()) }).unwrap();
    assert_eq!(fake.1.load(Ordering::Relaxed), 6);
    capture.public_elapsed(4.).unwrap(); capture.presented(None, None, 1000.).unwrap();
    let report = capture.report();
    assert_eq!(report["warmup_interest_rebuilds"], 1);
    assert_eq!(report["warmup_interest_rebuilds_before_first_attempt"], 1);
    assert_eq!(report["warmup_successful_scope_complete_before_first_attempt"], 2);
    assert_eq!(report["warmup"]["scope_completion_counts"], serde_json::json!([2, 2, 2]));
    assert_eq!(report["attempts"], 1);
    assert_eq!(report["successful_scope_complete_attempts"], 1);
    assert_eq!(report["scope_ms_totals"], serde_json::json!([3., 1., 1.]));
    assert_eq!(report["lifecycle_errors"], 0);
    assert_eq!(report["live_span_handles"], 0);
    assert!(CONTEXT.with(Cell::get).is_none());
}
