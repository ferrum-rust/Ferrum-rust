//! Durable sparse overlays with an optional, owner-local native-sync cursor.
//! The primary HashMap and its iteration order are unchanged. Only a native
//! client that calls take_changes records mutations; other worlds have no
//! unconsumed dirty queue. Acknowledging sync never removes durable overlays.
use std::{
    collections::{hash_map, HashMap, HashSet},
    ops::Deref,
    sync::OnceLock,
};

type Position = [i32; 3];

#[derive(Debug, Default)]
pub(crate) struct EditJournal {
    values: HashMap<Position, u16>,
    tracking: Option<Tracking>,
    // Unordered membership only. This can prove that a publication needs no
    // writes, but differing overlays still use values' original iteration.
    columns: OnceLock<super::ChunkMap<Vec<Position>>>,
}

#[derive(Debug, Default)]
struct Tracking {
    dirty: HashSet<Position>,
    reset: bool,
}

pub(crate) enum EditChanges {
    /// First native use, replacement, or clear: rebuild from current values.
    Reset,
    /// A position may now be absent. Read its latest value at consumption time.
    Positions(HashSet<Position>),
}

impl EditJournal {
    pub(crate) fn insert(&mut self, position: Position, state: u16) -> Option<u16> {
        let old = self.values.insert(position, state);
        if old.is_none() && (0..256).contains(&position[1]) {
            if let Some(columns) = self.columns.get_mut() {
                columns.entry(column(position)).or_default().push(position);
            }
        }
        if old != Some(state) {
            self.changed(position);
        }
        old
    }

    pub(crate) fn remove(&mut self, position: &Position) -> Option<u16> {
        let old = self.values.remove(position);
        if old.is_some() {
            if (0..256).contains(&position[1]) {
                if let Some(columns) = self.columns.get_mut() {
                    let pos = column(*position);
                    let members = columns.get_mut(&pos).expect("Indexed edit column");
                    if let Some(index) = members.iter().position(|p| p == position) {
                        members.swap_remove(index);
                    }
                    if members.is_empty() { columns.remove(&pos); }
                }
            }
            self.changed(*position);
        }
        old
    }

    pub(crate) fn clear(&mut self) {
        self.values.clear();
        if let Some(columns) = self.columns.get_mut() { columns.clear(); }
        if let Some(tracking) = &mut self.tracking {
            tracking.dirty.clear();
            tracking.reset = true;
        }
    }

    fn changed(&mut self, position: Position) {
        if let Some(tracking) = &mut self.tracking {
            if !tracking.reset {
                tracking.dirty.insert(position);
            }
        }
    }

    /// Fast negative filter, including air edits. The second value reports
    /// actual full-map work when initializing this derived index. No primary
    /// entry, hash layout, sync cursor, or overlay order is changed.
    pub(crate) fn intersects_columns(&self, positions: impl IntoIterator<Item = super::ChunkPos>) -> (bool, usize) {
        if self.values.is_empty() { return (false, 0); }
        let rebuilt = self.columns.get().is_none();
        let columns = self.columns.get_or_init(|| {
            let mut columns: super::ChunkMap<Vec<Position>> = super::ChunkMap::default();
            for &position in self.values.keys() {
                if (0..256).contains(&position[1]) {
                    columns.entry(column(position)).or_default().push(position);
                }
            }
            columns
        });
        (positions.into_iter().any(|pos| columns.contains_key(&pos)),
            if rebuilt { self.values.len() } else { 0 })
    }

    /// A conservative no-write proof for publications. Callers first initialize
    /// membership through intersects_columns; absent derived state falls back.
    /// This never applies entries in the index's unordered traversal order.
    pub(crate) fn matches_published_columns<'a>(&self,
        chunks: impl IntoIterator<Item = (super::ChunkPos, &'a super::Chunk)>,
    ) -> (bool, usize) {
        if self.values.is_empty() { return (true, 0); }
        let Some(columns) = self.columns.get() else { return (false, 0); };
        let mut examined = 0;
        for (position, chunk) in chunks {
            if let Some(members) = columns.get(&position) {
                for p in members {
                    examined += 1;
                    let state = self.values[p];
                    if chunk.get(p[0].rem_euclid(16), p[1], p[2].rem_euclid(16)) != state {
                        return (false, examined);
                    }
                }
            }
        }
        (true, examined)
    }

    pub(crate) fn take_changes(&mut self) -> EditChanges {
        let Some(tracking) = &mut self.tracking else {
            self.tracking = Some(Tracking::default());
            return EditChanges::Reset;
        };
        if std::mem::take(&mut tracking.reset) {
            tracking.dirty.clear();
            EditChanges::Reset
        } else {
            EditChanges::Positions(std::mem::take(&mut tracking.dirty))
        }
    }
}

impl Clone for EditJournal {
    fn clone(&self) -> Self {
        // A cloned World owns an independent journal. Its first native consumer
        // must bootstrap even if the old consumer has already drained changes.
        Self { values: self.values.clone(), tracking: None, columns: OnceLock::new() }
    }
}

impl PartialEq for EditJournal {
    fn eq(&self, other: &Self) -> bool { self.values == other.values }
}
impl Eq for EditJournal {}

impl Deref for EditJournal {
    type Target = HashMap<Position, u16>;
    fn deref(&self) -> &Self::Target { &self.values }
}
// Deliberately no DerefMut: mutations must pass through the tracked operations.
impl<'a> IntoIterator for &'a EditJournal {
    type Item = (&'a Position, &'a u16);
    type IntoIter = hash_map::Iter<'a, Position, u16>;
    fn into_iter(self) -> Self::IntoIter { self.values.iter() }
}
impl From<HashMap<Position, u16>> for EditJournal {
    fn from(values: HashMap<Position, u16>) -> Self { Self { values, tracking: None, columns: OnceLock::new() } }
}
impl FromIterator<(Position, u16)> for EditJournal {
    fn from_iter<T: IntoIterator<Item = (Position, u16)>>(iter: T) -> Self {
        Self::from(iter.into_iter().collect::<HashMap<_, _>>())
    }
}
impl Extend<(Position, u16)> for EditJournal {
    fn extend<T: IntoIterator<Item = (Position, u16)>>(&mut self, iter: T) {
        // Clear before consuming the iterator, including partial extensions
        // that unwind. Preserve HashMap::extend's reserve/layout behavior.
        self.columns.take();
        match &mut self.tracking {
            Some(tracking) if !tracking.reset => {
                // Preserve the original HashMap::extend reserve/iteration path
                // and iterator size_hint; repeated insert would change layout.
                self.values.extend(iter.into_iter().inspect(|(position, _)| {
                    tracking.dirty.insert(*position);
                }));
            }
            _ => self.values.extend(iter),
        }
    }
}

fn column(position: Position) -> super::ChunkPos {
    (position[0].div_euclid(16), position[2].div_euclid(16))
}

#[cfg(test)]
#[path = "edit_journal_column_tests.rs"]
mod column_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn order(values: &HashMap<Position, u16>) -> Vec<(Position, u16)> {
        values.iter().map(|(p, s)| (*p, *s)).collect()
    }

    #[test]
    fn journal_preserves_hashmap_iteration_and_extend_layout() {
        let initial = (0..257).map(|i| ([i, 10, -i], i as u16)).collect::<HashMap<_, _>>();
        let mut reference = initial.clone();
        let mut actual = EditJournal::from(initial);
        assert!(matches!(actual.take_changes(), EditChanges::Reset));
        for i in 0..512 {
            let p = [i, 10, -i];
            assert_eq!(actual.insert(p, i as u16 + 3), reference.insert(p, i as u16 + 3));
        }
        for i in (0..512).step_by(3) {
            let p = [i, 10, -i];
            assert_eq!(actual.remove(&p), reference.remove(&p));
        }
        assert_eq!(order(&actual), order(&reference));
        let batch = (0..4097).map(|i| ([i, 99, -i], i as u16)).collect::<Vec<_>>();
        reference.extend(batch.iter().copied());
        actual.extend(batch.iter().copied());
        assert_eq!(actual.capacity(), reference.capacity());
        assert_eq!(order(&actual), order(&reference));
        // A filtered iterator supplies a different lower size_hint.
        reference.extend(batch.iter().copied().filter(|(p, _)| p[0] % 3 == 0));
        actual.extend(batch.iter().copied().filter(|(p, _)| p[0] % 3 == 0));
        assert_eq!(order(&actual), order(&reference));
        actual.clear(); reference.clear();
        actual.extend(batch.clone()); reference.extend(batch);
        assert_eq!(order(&actual), order(&reference));
    }

    #[test]
    fn journal_tracking_is_opt_in_bounded_by_positions_and_never_shared() {
        fn require_send_sync<T: Send + Sync>() {}
        require_send_sync::<EditJournal>();
        let mut owner = EditJournal::default();
        for i in 0..1000 { owner.insert([1, 2, 3], i); }
        owner.remove(&[1, 2, 3]);
        assert!(owner.tracking.is_none(), "unconsumed worlds cannot accumulate tombstones");
        owner.insert([9, 8, 7], 5);
        assert!(matches!(owner.take_changes(), EditChanges::Reset));
        for i in 0..1000 { owner.insert([1, 2, 3], i); }
        owner.remove(&[1, 2, 3]);
        let EditChanges::Positions(dirty) = owner.take_changes() else { panic!("expected delta"); };
        assert_eq!(dirty, HashSet::from([[1, 2, 3]]));
        let mut copied = owner.clone();
        assert_eq!(copied, owner);
        assert!(matches!(copied.take_changes(), EditChanges::Reset));
        copied.insert([4, 5, 6], 7);
        let EditChanges::Positions(dirty) = owner.take_changes() else { panic!("expected delta"); };
        assert!(dirty.is_empty(), "a cloned journal must not share its mutation cursor");
        owner.clear();
        owner.insert([7, 8, 9], 1);
        assert!(matches!(owner.take_changes(), EditChanges::Reset));
        assert_eq!(owner.len(), 1);
    }
}
