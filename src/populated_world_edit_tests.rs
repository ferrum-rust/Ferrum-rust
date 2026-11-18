//! Compare the incremental sender with the previous full-journal queue rule.
//! These fixtures never generate terrain or submit their synthetic commands.
use super::*;
use crate::world::Chunk;

fn copy_command(command: &Command) -> Command {
    match command {
        Command::Edits(edits) => Command::Edits(edits.clone()),
        Command::Generate(positions) => Command::Generate(positions.clone()),
        Command::Snapshot { gameplay } => Command::Snapshot { gameplay: gameplay.clone() },
        _ => panic!("unexpected command in edit-queue fixture"),
    }
}

fn same_command(actual: &Command, expected: &Command) {
    match (actual, expected) {
        (Command::Edits(a), Command::Edits(b)) => {
            assert_eq!(a.len(), b.len(), "edit batch length");
            for (i, (a, b)) in a.iter().zip(b).enumerate() {
                assert_eq!((a.position, a.state, a.expected, a.notify_neighbors),
                    (b.position, b.state, b.expected, b.notify_neighbors), "edit index {i}");
            }
        }
        (Command::Generate(a), Command::Generate(b)) => assert_eq!(a, b),
        (Command::Snapshot { gameplay: a }, Command::Snapshot { gameplay: b }) => assert_eq!(a, b),
        _ => panic!("different command kinds: actual {actual:?}, expected {expected:?}"),
    }
}

#[derive(Default)]
struct FullScan {
    values: HashMap<[i32; 3], u16>,
    known: HashSet<ChunkPos>,
    sent: HashMap<[i32; 3], u16>,
    queued: VecDeque<Command>,
}
impl FullScan {
    fn queue(&mut self) {
        let mut edits = self.values.iter().filter(|(p, state)| {
            self.known.contains(&(p[0].div_euclid(16), p[2].div_euclid(16)))
                && self.sent.get(*p) != Some(*state)
        }).map(|(&position, &state)| BlockEdit {
            position, state, expected: None, notify_neighbors: false,
        }).collect::<Vec<_>>();
        edits.sort_unstable_by_key(|edit| edit.position);
        self.queued.extend(edits.chunks(EDIT_BATCH).map(|batch| Command::Edits(batch.to_vec())));
    }
    fn submitted(&mut self, command: &Command) {
        if let Command::Edits(edits) = command {
            for edit in edits { self.sent.insert(edit.position, edit.state); }
        }
    }
}

struct Pair {
    host: PopulatedWorld,
    world: World,
    old: FullScan,
}
impl Pair {
    fn new() -> Self {
        let mut session = NativeSession::in_memory(189, 1234).unwrap();
        assert!(matches!(session.receive().unwrap(), Event::Ready { .. }));
        Self {
            host: PopulatedWorld::new(session, false),
            world: World { chunks: Default::default(), spawn: Vec3::ZERO,
                label: "incremental edit oracle".into(), save_path: Default::default(),
                edits: Default::default(), imported: true, light_revision: 0 },
            old: FullScan::default(),
        }
    }
    fn insert(&mut self, p: [i32; 3], state: u16) {
        assert_eq!(self.world.edits.insert(p, state), self.old.values.insert(p, state));
    }
    fn remove(&mut self, p: [i32; 3]) {
        assert_eq!(self.world.edits.remove(&p), self.old.values.remove(&p));
    }
    fn clear(&mut self) { self.world.edits.clear(); self.old.values.clear(); }
    fn known(&mut self, positions: &[ChunkPos]) {
        self.host.known_chunks.extend(positions.iter().copied());
        self.old.known.extend(positions.iter().copied());
    }
    fn publish(&mut self, positions: &[ChunkPos]) {
        let chunk = Arc::new(Chunk::empty());
        self.host.apply(&mut self.world, Update {
            chunks: positions.iter().map(|&p| (p, chunk.clone())).collect(), ..Default::default()
        });
        self.old.known.extend(positions.iter().copied());
    }
    fn check(&self) {
        let values = self.world.edits.iter().map(|(&p, &s)| (p, s)).collect::<HashMap<_, _>>();
        assert_eq!(values, self.old.values, "journal retains the full save/overlay map");
        assert_eq!(self.host.known_chunks, self.old.known);
        assert_eq!(self.host.sent_edits, self.old.sent, "queueing never acknowledges an unsent value");
        assert_eq!(self.host.queued.len(), self.old.queued.len(), "command count");
        for (actual, expected) in self.host.queued.iter().zip(&self.old.queued) {
            same_command(actual, expected);
        }
    }
    fn queue(&mut self) -> usize {
        let scanned = self.host.queue_edits(&mut self.world);
        self.old.queue();
        self.check();
        scanned
    }
    fn rebuild(&mut self) -> usize {
        let remaining = std::mem::take(&mut self.host.queued);
        let old_remaining = std::mem::take(&mut self.old.queued);
        let scanned = self.host.queue_edits(&mut self.world);
        self.old.queue();
        self.host.queued.extend(remaining.into_iter().filter(|c| !matches!(c, Command::Edits(_))));
        self.old.queued.extend(old_remaining.into_iter().filter(|c| !matches!(c, Command::Edits(_))));
        self.check();
        scanned
    }
    fn cancel(&mut self) { self.host.queued.clear(); self.old.queued.clear(); }
    fn enqueue(&mut self, command: Command) {
        self.host.queued.push_back(copy_command(&command)); self.old.queued.push_back(command);
    }
    fn submit_front(&mut self) -> Command {
        let command = self.host.queued.pop_front().expect("queued command");
        let old = self.old.queued.pop_front().expect("oracle command");
        same_command(&command, &old);
        self.host.record_submitted_edits(&command); self.old.submitted(&old);
        self.check();
        command
    }
}

fn bulk_position(i: i32) -> [i32; 3] { [i % 16 - 16, 32 + i / 256, (i / 16) % 16 - 16] }
fn batch_lengths(queue: &VecDeque<Command>) -> Vec<usize> {
    queue.iter().filter_map(|command| match command {
        Command::Edits(edits) => Some(edits.len()), _ => None,
    }).collect()
}

#[test]
fn incremental_edits_match_full_scan_batches_mutations_cancellation_and_submission() {
    let mut pair = Pair::new();
    pair.known(&[(-1, -1)]);
    for i in (0..8211).rev() { pair.insert(bulk_position(i), 16 + (i % 7) as u16); }
    pair.queue();
    assert_eq!(batch_lengths(&pair.host.queued), [4096, 4096, 19]);
    assert!(pair.host.sent_edits.is_empty());
    // Canceling a queue has no acknowledgement; rebuilding must recover all
    // edits without rereading the retained historical map.
    pair.cancel();
    assert_eq!(pair.queue(), 0);
    assert_eq!(batch_lengths(&pair.host.queued), [4096, 4096, 19]);
    let Command::Edits(first) = pair.submit_front() else { panic!("first edit batch"); };
    assert_eq!(pair.host.sent_edits.len(), 4096);
    pair.enqueue(Command::Generate(vec![(5, 7), (-5, 2)]));
    pair.rebuild();
    assert_eq!(batch_lengths(&pair.host.queued), [4096, 19]);

    let acknowledged = first[13].clone();
    pair.insert(acknowledged.position, 896);
    pair.insert(acknowledged.position, acknowledged.state); // revert before drain
    let unsent = match pair.host.queued.front().unwrap() {
        Command::Edits(edits) => edits[17].position, _ => unreachable!(),
    };
    pair.insert(unsent, 48);
    pair.insert(unsent, 48); // same-value writes must not create duplicates
    pair.remove(unsent);
    pair.rebuild();
    assert_eq!(batch_lengths(&pair.host.queued), [4096, 18]);
    pair.insert(acknowledged.position, 224);
    pair.rebuild();
    assert_eq!(batch_lengths(&pair.host.queued), [4096, 19]);

    // Queued command payloads remain immutable while gameplay advances. An
    // old submission may acknowledge only that old value, never a newer one.
    let old_command = copy_command(pair.host.queued.front().unwrap());
    let changed = match &old_command { Command::Edits(edits) => edits[0].position, _ => unreachable!() };
    pair.insert(changed, 1168);
    pair.cancel();
    pair.queue();
    pair.host.record_submitted_edits(&old_command);
    pair.old.submitted(&old_command);
    pair.check();
    pair.rebuild();
    assert!(pair.host.queued.iter().any(|command| matches!(command,
        Command::Edits(edits) if edits.iter().any(|e| e.position == changed && e.state == 1168))));
    pair.clear();
    pair.rebuild();
    assert!(batch_lengths(&pair.host.queued).is_empty());
    pair.insert(acknowledged.position, pair.old.sent[&acknowledged.position]);
    pair.rebuild();
    assert!(batch_lengths(&pair.host.queued).is_empty(), "removal does not undo submitted native state");
}

#[test]
fn deferred_edits_promote_from_discarded_halo_and_survive_visible_eviction_and_reset() {
    let mut pair = Pair::new();
    let known = [-1, 70, -17];
    let future = [33, 90, -34];
    let removed = [34, 91, -34];
    pair.known(&[(-1, -2)]);
    pair.insert(known, 224); pair.insert(future, 896); pair.insert(removed, 48);
    pair.queue();
    assert_eq!(batch_lengths(&pair.host.queued), [1]);
    pair.submit_front();
    pair.cancel();
    pair.insert(future, 1168); pair.remove(removed);
    // The publication is entirely outside wanted/the visible cache. It still
    // makes the native owner eligible for the latest deferred edits.
    pair.publish(&[(2, -3)]);
    assert!(pair.world.chunks.is_empty());
    pair.queue();
    assert!(matches!(pair.host.queued.front(), Some(Command::Edits(edits))
        if edits.len() == 1 && edits[0].position == future && edits[0].state == 1168));
    pair.submit_front();
    pair.host.wanted.insert((2, -3));
    pair.publish(&[(2, -3)]);
    assert_eq!(pair.world.get(IVec3::from_array(future)), 1168);
    pair.world.chunks.remove(&(2, -3));
    pair.publish(&[(2, -3)]);
    assert_eq!(pair.world.get(IVec3::from_array(future)), 1168);
    pair.queue();
    assert!(pair.host.queued.is_empty(), "republishing retained native state does not resend it");

    // A copied/replaced journal starts a fresh tracking epoch; historical
    // pending/deferred entries must be rebuilt, including removals.
    pair.insert([200, 70, 200], 2448);
    pair.queue(); pair.cancel();
    let replacement = HashMap::from([(known, 48), (future, 16), ([200, 71, 200], 2064)]);
    pair.world.edits = replacement.clone().into(); pair.old.values = replacement;
    pair.queue();
    assert_eq!(batch_lengths(&pair.host.queued), [2]);
    pair.cancel();
    pair.world.edits = pair.world.edits.clone();
    pair.queue();
    assert_eq!(batch_lengths(&pair.host.queued), [2]);
    pair.cancel();
    pair.publish(&[(12, 12)]);
    pair.queue();
    assert_eq!(batch_lengths(&pair.host.queued), [3]);
    pair.clear(); pair.cancel(); pair.queue();
    assert!(pair.host.queued.is_empty());
    pair.world.edits = Default::default(); pair.old.values.clear();
    pair.queue();
    assert!(pair.host.queued.is_empty());
}

#[test]
fn incremental_edits_keep_captured_save_barrier_and_acknowledge_only_submitted_values() {
    let mut pair = Pair::new();
    pair.known(&[(0, 0)]);
    let p = [1, 80, 1];
    pair.insert(p, 16);
    pair.queue();
    pair.enqueue(Command::Generate(vec![(2, 3)]));
    // Synthetic in-flight ownership prevents all actual worker submissions.
    // Real durable checkpoint/resume coverage remains in populated_world_tests.
    pair.host.in_flight = Some(700);
    pair.host.persistent = true;
    let mut sim = Simulation::default();
    sim.health = 7.;
    let pose = PlayerSave { position: [1.5, 81., 1.5], motion: None };
    let captured = pair.host.capture(&pair.world, pose, &sim).unwrap();
    let old_remaining = std::mem::take(&mut pair.old.queued);
    pair.old.queue();
    pair.old.queued.push_back(Command::Snapshot { gameplay: Some(captured.clone()) });
    pair.old.queued.extend(old_remaining.into_iter().filter(|c| !matches!(c, Command::Edits(_))));
    assert!(pair.host.autosave(&mut pair.world, pose, &sim).unwrap());
    pair.check();
    assert!(pair.host.saving && pair.host.sent_edits.is_empty());
    pair.insert(p, 896);
    pair.insert([2, 80, 1], 48);
    sim.health = 13.;
    assert!(!pair.host.autosave(&mut pair.world, pose, &sim).unwrap());
    pair.host.poll(&mut pair.world, &mut sim).unwrap();
    pair.check();
    assert!(matches!(pair.host.queued.get(1), Some(Command::Snapshot { gameplay: Some(save) }) if save == &captured));
    pair.submit_front(); // only captured 16 is acknowledged
    assert_eq!(pair.host.sent_edits.get(&p), Some(&16));
    assert!(matches!(pair.submit_front(), Command::Snapshot { .. }));
    pair.host.saving = false;
    pair.host.in_flight = None;
    pair.rebuild();
    assert!(matches!(pair.host.queued.front(), Some(Command::Edits(edits))
        if edits.len() == 2 && edits[0].position == p && edits[0].state == 896));
    assert!(matches!(pair.host.queued.back(), Some(Command::Generate(v)) if v == &vec![(2, 3)]));
}

#[test]
fn stale_save_submission_rechecks_a_latest_value_equal_to_the_previous_acknowledgment() {
    let mut pair = Pair::new();
    pair.known(&[(0, 0)]);
    let p = [1, 80, 1];
    let original = 16;
    let captured = 896;
    pair.insert(p, original);
    pair.queue();
    pair.submit_front();

    // An in-flight generation delays an autosave's captured edit. Its remaining
    // generation must resume only after the captured snapshot has completed.
    pair.host.in_flight = Some(700);
    pair.host.persistent = true;
    pair.enqueue(Command::Generate(vec![(2, 3)]));
    pair.insert(p, captured);
    let sim = Simulation::default();
    let pose = PlayerSave { position: [1.5, 81., 1.5], motion: None };
    let save = pair.host.capture(&pair.world, pose, &sim).unwrap();
    let remaining = std::mem::take(&mut pair.old.queued);
    pair.old.queue();
    pair.old.queued.push_back(Command::Snapshot { gameplay: Some(save.clone()) });
    pair.old.queued.extend(remaining.into_iter().filter(|c| !matches!(c, Command::Edits(_))));
    assert!(pair.host.autosave(&mut pair.world, pose, &sim).unwrap());
    pair.check();

    // This is the normal (non-initial-view) manual save's queue_edits boundary:
    // latest == sent, so the latest dirty value is consumed without queuing it.
    // The older autosave still owns its immutable edit and snapshot commands.
    pair.insert(p, original);
    pair.queue();
    assert!(pair.host.pending_edits.is_empty());
    assert_eq!(pair.host.sent_edits.get(&p), Some(&original));
    pair.submit_front();
    assert_eq!(pair.host.sent_edits.get(&p), Some(&captured));
    assert!(matches!(pair.submit_front(), Command::Snapshot { gameplay: Some(value) } if value == save));
    pair.host.saving = false;
    pair.host.in_flight = None;

    // No further journal mutation occurs. A submission changed the comparison
    // baseline, so the old full scan still requires the reverted value before
    // the pending generation. Incremental reconciliation must do the same.
    pair.rebuild();
    assert!(matches!(pair.host.queued.front(), Some(Command::Edits(edits))
        if edits.len() == 1 && edits[0].position == p && edits[0].state == original));
    assert!(matches!(pair.host.queued.back(), Some(Command::Generate(v)) if v == &vec![(2, 3)]));
    pair.submit_front();
    pair.rebuild();
    assert!(batch_lengths(&pair.host.queued).is_empty(), "the repaired value is acknowledged once");
}

#[test]
fn submitted_rechecks_respect_removed_overlays_and_journal_resets() {
    let mut pair = Pair::new();
    pair.known(&[(0, 0)]);
    let p = [1, 80, 1];
    pair.insert(p, 16);
    pair.queue();
    pair.submit_front();
    pair.insert(p, 896);
    pair.queue();
    let stale = copy_command(pair.host.queued.front().unwrap());
    pair.cancel();
    pair.remove(p);
    pair.queue();
    assert!(pair.host.pending_edits.is_empty());

    // A previously captured command can still submit after the removal was
    // consumed. Rechecking that key must read absence, not resurrect its payload.
    pair.host.record_submitted_edits(&stale);
    pair.old.submitted(&stale);
    pair.check();
    assert!(pair.host.submitted_rechecks.contains(&p));
    pair.rebuild();
    assert!(pair.host.queued.is_empty());
    assert!(pair.host.submitted_rechecks.is_empty());

    // Replacing the journal requires a full current-map rebuild. Old submission
    // keys must be discarded along with obsolete pending and deferred entries.
    pair.host.record_submitted_edits(&stale);
    pair.old.submitted(&stale);
    let q = [2, 80, 1];
    let replacement = HashMap::from([(q, 48)]);
    pair.world.edits = replacement.clone().into();
    pair.old.values = replacement;
    pair.queue();
    assert!(pair.host.submitted_rechecks.is_empty());
    assert_eq!(pair.host.pending_edits.keys().copied().collect::<Vec<_>>(), [q]);
    pair.submit_front();
    assert!(pair.host.submitted_rechecks.contains(&q));
    pair.clear();
    pair.rebuild();
    assert!(pair.host.queued.is_empty());
    assert!(pair.host.submitted_rechecks.is_empty());
}
