//! Pure bookkeeping oracles; no Device, Queue, clock read, or GPU resource.
use super::*;

fn cpu() -> CpuTimes { CpuTimes { acquire_ms: 1., encode_ms: 2., submit_ms: 3., present_ms: 4. } }
fn submit(book: &mut Book, frame: u64) -> Ticket {
    let ticket = book.reserve(frame, frame as f32 / 10.).unwrap();
    book.started(ticket).unwrap(); book.ended(ticket).unwrap();
    book.submitted(ticket, cpu()).unwrap(); ticket
}

#[test]
fn features_and_resource_bounds_are_explicit_without_optional_arena_changes() {
    let query = wgpu::Features::TIMESTAMP_QUERY;
    let encoder = wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
    let resolve = wgpu::DownlevelFlags::NONBLOCKING_QUERY_RESOLVE;
    for available in [wgpu::Features::empty(), query, encoder] {
        assert!(supported(available, resolve).is_err());
    }
    assert!(supported(query | encoder, wgpu::DownlevelFlags::empty()).is_err());
    let arena = wgpu::Features::MULTI_DRAW_INDIRECT;
    let requested = supported(query | encoder | arena, resolve).unwrap();
    assert_eq!(requested, query | encoder);
    assert_eq!(arena | requested, arena | query | encoder, "caller unions rather than replaces arena features");
    assert_eq!((MAX_ROWS, BYTES_PER_ROW, READBACK_BYTES, RESOLVE_BYTES), (131_072, 16, 2_097_152, 256));
    assert_eq!(READBACK_BYTES % wgpu::MAP_ALIGNMENT, 0);
    assert_eq!(RESOLVE_BYTES % wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT, 0);
    assert_eq!((MAX_ROWS as u64 - 1) * BYTES_PER_ROW + BYTES_PER_ROW, READBACK_BYTES);
    assert!(Book::new(MAX_ROWS + 1).is_err());
    let mut empty = Book::new(0).unwrap();
    assert!(empty.reserve(1, 1.).is_none()); assert_eq!(empty.overflow, 1);
    assert!(empty.start_finish().unwrap()); assert!(empty.rows.is_empty());
}

#[test]
fn ordered_rows_keep_fixed_storage_and_reject_stale_foreign_or_uncertain_tickets() {
    let mut book = Book::new(3).unwrap(); let pointer = book.rows.as_ptr(); let capacity = book.rows.capacity();
    let abandoned = book.reserve(7, 0.7).unwrap(); book.clear_pending_present();
    assert_eq!(book.canceled_reserved, 1); assert!(book.pending.is_none());
    let current = book.reserve(8, 0.8).unwrap();
    assert_eq!(current.slot, abandoned.slot); assert_ne!(current.serial, abandoned.serial);
    assert!(book.started(abandoned).is_err());
    let mut other = Book::new(3).unwrap(); let foreign = other.reserve(8, 0.8).unwrap();
    assert_ne!(current.owner, foreign.owner); assert!(book.started(foreign).is_err());
    assert_eq!(book.pending.unwrap().phase, Phase::Reserved);
    assert!(book.ended(current).is_err());
    book.started(current).unwrap(); assert!(book.started(current).is_err());
    book.ended(current).unwrap(); book.submitted(current, cpu()).unwrap();
    assert!(book.submitted(current, cpu()).is_err());
    for frame in [9, 10] { submit(&mut book, frame); }
    assert_eq!(book.rows.iter().map(|r| r.renderer_frame_id).collect::<Vec<_>>(), vec![8, 9, 10]);
    assert_eq!(book.rows.as_ptr(), pointer); assert_eq!(book.rows.capacity(), capacity);
    for _ in 0..3 { assert!(book.reserve(11, 1.1).is_none()); }
    assert_eq!(book.overflow, 3); assert_eq!(book.rows.len(), 3);
    assert_eq!(book.rows.as_ptr(), pointer); assert_eq!(book.rows.capacity(), capacity);
    assert!(book.pending.is_none()); assert_eq!(book.terminal, Terminal::Open);
    for ended in [false, true] {
        let mut unsafe_book = Book::new(2).unwrap(); let ticket = unsafe_book.reserve(1, 0.).unwrap();
        unsafe_book.started(ticket).unwrap(); if ended { unsafe_book.ended(ticket).unwrap(); }
        unsafe_book.clear_pending_present();
        assert_eq!(unsafe_book.terminal, Terminal::Failed);
        assert!(unsafe_book.reserve(2, 0.).is_none());
        assert!(unsafe_book.start_finish().is_err()); assert!(unsafe_book.rows.is_empty());
    }
}

#[test]
fn present_stamps_follow_actual_app_arm_and_never_leak_across_failed_attempts() {
    let mut book = Book::new(5).unwrap();
    // Warmup can cross its threshold during render: App arms without a ticket.
    book.presented(None, None, 3000.).unwrap(); assert!(book.rows.is_empty());
    submit(&mut book, 10); book.presented(None, None, 3010.).unwrap();
    let arm = book.rows[0].stamp.unwrap();
    assert_eq!((arm.sample_index, arm.begin_ms, arm.end_ms), (None, None, 3010.));
    submit(&mut book, 11); book.presented(Some(0), Some(3010.), 3017.).unwrap();
    let first = book.rows[1].stamp.unwrap();
    assert_eq!((first.sample_index, first.begin_ms, first.end_ms), (Some(0), Some(3010.), 3017.));
    // A repeated stamp has nothing pending and cannot overwrite the first one.
    book.presented(Some(999), Some(0.), 9999.).unwrap();
    assert_eq!(book.rows[1].stamp.unwrap().sample_index, Some(0));
    // A submitted but unanchored frame cannot inherit a later successful stamp
    // after the next render attempt clears state before surface acquisition.
    submit(&mut book, 12); book.clear_pending_present();
    book.presented(Some(1), Some(3017.), 3026.).unwrap();
    assert!(book.rows[2].stamp.is_none());
    // A failed acquisition itself reserves no row; the next success is aligned
    // with the existing App sample index rather than the collector row number.
    book.clear_pending_present(); submit(&mut book, 14);
    book.presented(Some(2), Some(3026.), 3045.).unwrap();
    assert_eq!(book.rows[3].stamp.unwrap().sample_index, Some(2));
    assert_eq!(book.rows[3].renderer_frame_id, 14);
    for (sample, begin, end) in [(Some(0), None, 1.), (None, Some(0.), 1.),
        (Some(0), Some(2.), 1.), (Some(0), Some(-1.), 1.),
        (Some(0), Some(f64::NAN), 1.), (Some(0), Some(0.), f64::INFINITY)] {
        let mut bad = Book::new(1).unwrap(); submit(&mut bad, 1);
        assert!(bad.presented(sample, begin, end).is_err());
        assert!(bad.rows[0].stamp.is_none());
    }
}

#[test]
fn terminal_and_validation_paths_cannot_reuse_encoded_or_finished_storage() {
    let mut book = Book::new(2).unwrap(); book.reserve(1, 0.).unwrap();
    assert!(book.start_finish().unwrap()); assert_eq!(book.canceled_reserved, 1);
    assert_eq!(book.terminal, Terminal::Finishing); assert!(book.start_finish().is_err());
    assert!(book.reserve(2, 0.).is_none()); assert_eq!(book.late_reserves, 1);
    // Capture performs the only final map/unmap, then marks this completion.
    book.terminal = Terminal::Complete;
    assert!(!book.start_finish().unwrap()); assert!(!book.start_finish().unwrap());
    assert!(book.reserve(3, 0.).is_none()); assert!(book.pending.is_none());
    for ended in [false, true] {
        let mut book = Book::new(1).unwrap(); let ticket = book.reserve(1, 0.).unwrap();
        book.started(ticket).unwrap(); if ended { book.ended(ticket).unwrap(); }
        assert!(book.start_finish().is_err()); assert_eq!(book.terminal, Terminal::Failed);
        assert!(book.rows.is_empty());
    }
    for bad_cpu in [CpuTimes { acquire_ms: f64::NAN, ..cpu() },
        CpuTimes { encode_ms: -1., ..cpu() }, CpuTimes { submit_ms: f64::INFINITY, ..cpu() },
        CpuTimes { present_ms: -1., ..cpu() }] {
        let mut book = Book::new(1).unwrap(); let ticket = book.reserve(1, 0.).unwrap();
        book.started(ticket).unwrap(); book.ended(ticket).unwrap();
        assert!(book.submitted(ticket, bad_cpu).is_err()); assert!(book.rows.is_empty());
        assert!(book.start_finish().is_err(), "unconfirmed submission cannot be finalized");
    }
    for bad_time in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut book = Book::new(1).unwrap(); assert!(book.reserve(0, bad_time).is_none());
        assert_eq!(book.terminal, Terminal::Failed); assert!(book.rows.is_empty());
    }
    let mut exhausted = Book::new(1).unwrap(); exhausted.serial = u64::MAX;
    assert!(exhausted.reserve(1, 0.).is_none()); assert_eq!(exhausted.terminal, Terminal::Failed);
    let first_reason = exhausted.failure.clone(); exhausted.fail("a later failure");
    assert_eq!(exhausted.failure, first_reason, "retain the first cause");
}

#[test]
fn gpu_tick_conversion_subtracts_integers_before_scaling_without_wrap_guessing() {
    assert_eq!(elapsed_ms([123, 123], 1.).unwrap(), 0.);
    assert_eq!(elapsed_ms([0, 2_000_000], 0.5).unwrap(), 1.);
    let precise = elapsed_ms([u64::MAX - 7, u64::MAX], 1.).unwrap();
    assert!((precise - 0.000007).abs() < 1e-18, "large timestamps must not be cast before subtraction");
    assert_eq!(elapsed_ms([u64::MAX, 0], 1.), Err("reversed_or_unknown_wrap"));
    assert_eq!(elapsed_ms([9, 8], 1.), Err("reversed_or_unknown_wrap"));
    for period in [0., -1., f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(elapsed_ms([1, 2], period), Err("invalid_timestamp_period"));
    }
    assert_eq!(elapsed_ms([0, u64::MAX], f64::MAX), Err("nonfinite_gpu_elapsed"));
    assert!(elapsed_ms([0, u64::MAX], 1.).unwrap().is_finite());
}
