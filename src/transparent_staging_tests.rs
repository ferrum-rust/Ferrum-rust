use super::*;

fn submit(book: &mut Book, eligible: bool, bound: Option<u64>, writes: &[u64])
    -> (Selection, Option<(usize, u64)>, Vec<Option<(usize, u64)>>)
{
    let mut frame = book.begin(eligible).unwrap();
    let selection = frame.plan(bound).unwrap();
    let addresses = writes.iter().map(|&bytes| frame.write(bytes).unwrap()).collect();
    frame.seal().unwrap();
    let remap = frame.submitted().unwrap();
    (selection, remap, addresses)
}

#[test]
fn preflight_matches_independent_padded_payload_and_full_membership() {
    // Independent byte construction, not the production alignment expression.
    for a in [0usize, 4, 12, 24, 36, 48] {
        for b in [0usize, 4, 12, 24, 36, 48] {
            for c in [0usize, 4, 12, 24, 36, 48] {
                let mut expected = Vec::new();
                for n in [a, b, c] {
                    if n == 0 { continue; }
                    while expected.len() % 8 != 0 { expected.push(0u8); }
                    expected.extend(std::iter::repeat_n(1u8, n));
                }
                let actual = [a, b, c].into_iter().fold(Some(0),
                    |total, n| add_bound(total, Some(n as u64)));
                assert_eq!(actual, Some(expected.len() as u64), "{a}/{b}/{c}");
            }
        }
    }
    assert_eq!(add_bound(None, Some(12)), None);
    assert_eq!(add_bound(Some(0), None), None);
    assert_eq!(add_bound(Some(u64::MAX), Some(0)), Some(u64::MAX));
    assert_eq!(add_bound(Some(u64::MAX - 3), Some(4)), None);
    assert_eq!(add_bound(Some(8), Some(u64::MAX)), None);

    // A previous one-quad mask cannot determine the new seven-quad upload bound.
    let previous_active_bytes = 6 * 2;
    let full_membership_bytes = 7 * 6 * 2;
    assert!(full_membership_bytes > previous_active_bytes);
    let mut book = Book::new(88);
    let (selection, remap, addresses) = submit(&mut book, true,
        add_bound(Some(0), Some(full_membership_bytes)), &[full_membership_bytes]);
    assert_eq!(selection, Selection::Staged { slot: 0, epoch: 1 });
    assert_eq!(addresses, vec![Some((0, 0))]);
    assert_eq!(remap, Some((0, 1)));
    assert_eq!(book.counts.staged_bytes, 84);
}

#[test]
fn whole_frame_fallback_never_reserves_or_partially_stages() {
    let mut book = Book::new(64);
    let before = book.slots;
    for (eligible, bound, reason, writes) in [
        (false, None, Fallback::Distance, vec![12, 24]),
        (true, Some(0), Fallback::Empty, vec![]),
        (true, Some(65), Fallback::Oversize, vec![12, 24]),
        (true, None, Fallback::Overflow, vec![12]),
    ] {
        let (selection, remap, addresses) = submit(&mut book, eligible, bound, &writes);
        assert_eq!(selection, Selection::Queue(reason));
        assert_eq!(remap, None);
        assert!(addresses.iter().all(Option::is_none));
        assert_eq!(book.slots, before);
        assert!(!book.active && !book.poisoned);
    }
    assert_eq!(book.counts.queue_frames, 4);
    assert_eq!(book.counts.queue_writes, 5);
    assert_eq!(book.counts.queue_bytes, 84);
    assert_eq!(book.counts.staged_frames, 0);
    assert_eq!(book.counts.remap_requests, 0);
    assert_eq!(book.counts.maximum_preflight_bytes, 65);

    // Exact capacity admits; one extra aligned write is never silently rerouted.
    let (selection, _, addresses) = submit(&mut book, true, Some(64), &[64]);
    assert_eq!(selection, Selection::Staged { slot: 0, epoch: 1 });
    assert_eq!(addresses, vec![Some((0, 0))]);
}

#[test]
fn packed_writes_preserve_holes_and_wait_for_successful_map_completion() {
    let mut book = Book::new(64);
    let (selection, remap, addresses) = submit(&mut book, true, Some(56), &[12, 12, 24]);
    assert_eq!(selection, Selection::Staged { slot: 0, epoch: 1 });
    assert_eq!(addresses, vec![Some((0, 0)), Some((0, 16)), Some((0, 32))]);
    let mut occupied = [false; 64];
    for (address, bytes) in addresses.iter().zip([12usize, 12, 24]) {
        let (slot, offset) = address.unwrap();
        assert_eq!(slot, 0);
        for cell in &mut occupied[offset as usize..offset as usize + bytes] {
            assert!(!*cell, "staged source ranges overlapped");
            *cell = true;
        }
    }
    assert!(occupied[..12].iter().all(|v| *v));
    assert!(occupied[12..16].iter().all(|v| !*v));
    assert!(occupied[28..32].iter().all(|v| !*v));
    assert!(occupied[56..].iter().all(|v| !*v));
    assert_eq!(book.counts.staged_bytes, 48);
    assert_eq!(book.counts.aligned_staged_bytes, 56);
    assert_eq!(book.slots[0].state, SlotState::Pending);
    assert_eq!(book.counts.remap_successes, 0);
    let (slot, epoch) = remap.unwrap();
    book.complete(slot, epoch, true);
    assert_eq!(book.slots[0].state, SlotState::Ready);
    assert_eq!(book.counts.remap_successes, 1);
}

#[test]
fn four_slots_have_exact_epoch_ownership_and_bounded_failure_reuse() {
    let mut book = Book::new(64);
    let mut completions = Vec::new();
    for slot in 0..SLOTS {
        let (selection, remap, addresses) = submit(&mut book, true, Some(12), &[12]);
        assert_eq!(selection, Selection::Staged { slot, epoch: 1 });
        assert_eq!(addresses, vec![Some((slot, 0))]);
        completions.push(remap.unwrap());
    }
    assert_eq!(book.counts.peak_pending_slots, SLOTS);
    assert_eq!(book.slots.len(), SLOTS);
    let pending = book.slots;
    assert_eq!(submit(&mut book, true, Some(12), &[12]).0, Selection::Queue(Fallback::Busy));
    assert_eq!(book.slots, pending);
    book.complete(SLOTS, 1, true);
    book.complete(0, 0, true);
    assert_eq!(book.slots, pending);
    book.complete(completions[0].0, completions[0].1, true);
    book.complete(0, 1, true); // Duplicate completion cannot create another credit.
    assert_eq!(book.counts.ignored_callbacks, 3);
    let (selection, remap, _) = submit(&mut book, true, Some(12), &[12]);
    assert_eq!(selection, Selection::Staged { slot: 0, epoch: 2 });
    assert_eq!(remap, Some((0, 2)));
    book.complete(0, 1, true); // An old callback must not release the new upload.
    assert_eq!(book.slots[0], Slot { epoch: 2, state: SlotState::Pending });
    book.complete(1, 1, false);
    assert_eq!(book.slots[1].state, SlotState::Failed);
    assert_eq!(submit(&mut book, true, Some(12), &[12]).0, Selection::Queue(Fallback::Busy));
    book.complete(0, 2, true);
    let maps = book.counts.remap_requests;
    let (selection, remap, addresses) = submit(&mut book, true, Some(12), &[]);
    assert_eq!(selection, Selection::Staged { slot: 0, epoch: 3 });
    assert_eq!(remap, None); // Identical/Skipped frame leaves the slab mapped.
    assert!(addresses.is_empty());
    assert_eq!(book.counts.remap_requests, maps);
    assert_eq!(book.slots[0].state, SlotState::Ready);
    book.hold_recalled = true;
    assert_eq!(submit(&mut book, true, Some(12), &[12]).0, Selection::Queue(Fallback::Busy));
    book.hold_recalled = false;
    assert!(matches!(submit(&mut book, true, Some(12), &[12]).0,
        Selection::Staged { slot: 0, epoch: 4 }));
    assert_eq!(book.counts.remap_failures, 1);
    assert_eq!(book.counts.ignored_callbacks, 4);
    assert!(!book.poisoned);
}

#[test]
fn selected_frame_drop_and_unwind_poison_before_any_upload_can_be_lost() {
    for progress in 0..3 {
        let mut book = Book::new(64);
        {
            let mut frame = book.begin(true).unwrap();
            frame.plan(Some(12)).unwrap();
            if progress >= 1 { frame.write(12).unwrap(); }
            if progress >= 2 { frame.seal().unwrap(); }
            // Simulate abandoning the encoder, including before its first copy.
        }
        assert!(book.poisoned && !book.active);
        assert_eq!(book.counts.abandoned_frames, 1);
        assert_eq!(book.counts.submitted_frames, 0);
        book.complete(0, 1, true); // Callback cannot forgive unsubmitted state.
        assert!(matches!(book.begin(true), Err(Error::Poisoned)));
        assert!(matches!(book.begin(false), Err(Error::Poisoned)));
    }
    let mut book = Book::new(64);
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut frame = book.begin(true).unwrap();
        frame.plan(Some(12)).unwrap();
        panic!("injected failure after CPU sort and before copy registration");
    }));
    assert!(panic.is_err());
    assert!(book.poisoned && !book.active);

    // Queue fallback preserves the original pending-write/encoder independence.
    let mut original = Book::new(64);
    {
        let mut frame = original.begin(false).unwrap();
        frame.plan(None).unwrap();
        frame.write(12).unwrap();
    }
    assert!(!original.poisoned && !original.active);
    assert_eq!(submit(&mut original, false, None, &[]).0, Selection::Queue(Fallback::Distance));
}

#[test]
fn malformed_phase_overrun_and_epoch_exhaustion_fail_closed() {
    for operation in 0..8 {
        let mut book = Book::new(64);
        {
            let mut frame = book.begin(true).unwrap();
            let error = match operation {
                0 => frame.write(4).unwrap_err(),
                1 => frame.seal().unwrap_err(),
                2 => { frame.plan(Some(12)).unwrap(); frame.submitted().unwrap_err() },
                3 => { frame.plan(Some(12)).unwrap(); frame.plan(Some(12)).unwrap_err() },
                4 => { frame.plan(Some(12)).unwrap(); frame.write(0).unwrap_err() },
                5 => { frame.plan(Some(12)).unwrap(); frame.write(2).unwrap_err() },
                6 => { frame.plan(Some(12)).unwrap(); frame.write(12).unwrap(); frame.write(4).unwrap_err() },
                7 => { frame.plan(Some(12)).unwrap(); frame.seal().unwrap(); frame.write(4).unwrap_err() },
                _ => unreachable!(),
            };
            assert_eq!(error, [Error::Unplanned, Error::Unplanned, Error::WrongPhase,
                Error::AlreadyPlanned, Error::InvalidWrite, Error::InvalidWrite,
                Error::Budget, Error::WrongPhase][operation]);
            assert_eq!(frame.queue_writes, 0, "an admitted overrun must not become Queue fallback");
        }
        assert!(matches!(book.begin(true), Err(Error::Poisoned)));
    }
    let mut book = Book::new(64);
    book.slots[0].epoch = MAX_EPOCH;
    {
        let mut frame = book.begin(true).unwrap();
        assert_eq!(frame.plan(Some(12)), Err(Error::EpochExhausted));
    }
    assert_eq!(book.slots[0].epoch, MAX_EPOCH);
    assert!(matches!(book.begin(true), Err(Error::Poisoned)));
    assert!(MAX_EPOCH.checked_mul(4).and_then(|v| v.checked_add(2)).is_some());

    let mailbox = Mailbox::new();
    assert_eq!(mailbox.slots.len(), SLOTS);
    assert!(mailbox.slots.iter().all(|word| word.load(Ordering::Acquire) == 0));
}
