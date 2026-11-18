//! Reuse one unique Chunk only where every intervening block/light callback
//! is proven inert. All exceptional candidates return to the existing adapter.
use super::*;
use crate::world::Chunk;

/// The caller already performed and counted this candidate's predicate read.
/// Its cursor points immediately after this position in the candidate slice.
#[derive(Clone, Copy, Debug)]
pub(super) struct ReadCandidate {
    pub position: IVec3,
    pub old: u16,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum ColumnProgress {
    /// Nothing changed and no additional logical reads occurred. The caller
    /// retains its already-read first candidate and uses its current body.
    Declined,
    /// Handled the first candidate and every candidate before this cursor.
    /// The next candidate, if any, is unread (typically a different column).
    Advanced { cursor: usize },
    /// This candidate's read is already counted and the cursor has advanced.
    /// Drop all column borrows before dispatching the existing Next::Write.
    ReadFallback { candidate: ReadCandidate, cursor: usize },
}

/// Narrow raw-World writer. The caller admits only the existing exact inert
/// whitelist and actual old stone/netherrack, excluding every tile-creating ID.
/// The known-state Chunk setter preserves generic top, tile and index effects.
/// Spawner/banner initialization in World::set_raw is therefore unreachable.
struct InertColumnEdit<'a> {
    chunk: &'a mut Chunk,
    journal: Option<&'a mut crate::world::EditJournal>,
    light_revision: &'a mut u64,
}
impl InertColumnEdit<'_> {
    fn set(&mut self, p: IVec3, old: u16, new: u16) {
        debug_assert_ne!(old, new);
        let index = Chunk::index(p.x.rem_euclid(16), p.y, p.z.rem_euclid(16));
        self.chunk.set_inert_ore_known(index, old, new);
        *self.light_revision = self.light_revision.wrapping_add(1);
        if let Some(journal) = self.journal.as_mut() {
            journal.insert(p.to_array(), new);
        }
    }
}

fn has_target_metadata(
    chunk: &Chunk,
    p: IVec3,
    containers: &BTreeMap<[i32; 3], LiveContainer>,
    spawners: &BTreeMap<[i32; 3], String>,
    drops: &BTreeMap<[i32; 3], super::super::drops::DropTile>,
) -> bool {
    let at = p.to_array();
    let local = [p.x.rem_euclid(16), p.y, p.z.rem_euclid(16)];
    (!containers.is_empty() && containers.contains_key(&at))
        || (!spawners.is_empty() && spawners.contains_key(&at))
        || (!drops.is_empty() && drops.contains_key(&at))
        || (!chunk.tiles.is_empty() && chunk.tiles.contains_key(&local))
}

/// Called inside the current Access::with borrow, after its hooks/whitelist
/// checks and after reading the first changed candidate. No callback may run
/// between that read and this function. It never calls back into Access.
///
/// The lighting predicate first verifies this column's existing section and
/// below-height invariant. Light state cannot mutate during the retained
/// borrow, so later candidates use the same immutable section/height arrays.
pub(super) fn try_write_column(
    provider: &mut Provider,
    b: &mut RuntimeBackend,
    positions: &[IVec3],
    mut cursor: usize,
    first: ReadCandidate,
    ore: u16,
    replace_id: u16,
) -> ColumnProgress {
    #[cfg(test)]
    if b.reference_inert_column { return ColumnProgress::Declined; }
    let p = first.position;
    debug_assert!(cursor != 0 && cursor <= positions.len() && positions[cursor - 1] == p);
    debug_assert!(matches!(replace_id, 1 | 87));
    if !valid(p) || first.old >> 4 != replace_id || first.old == ore {
        return ColumnProgress::Declined;
    }
    if !b.light.opaque_replacement_is_noop(p, first.old, ore) {
        return ColumnProgress::Declined;
    }
    let pos = cp(p);
    let light = &b.light.chunks[&pos];
    // This split keeps journal/revision bookkeeping disjoint from the borrowed
    // Chunk. Other runtime fields remain usable without borrowing all of World.
    let World { chunks, edits, light_revision, .. } = &mut b.world;
    let Some(chunk) = chunks.get_mut(&pos) else { return ColumnProgress::Declined; };
    if chunk.get(p.x.rem_euclid(16), p.y, p.z.rem_euclid(16)) != first.old
        || has_target_metadata(chunk, p, &b.live_containers, &b.live_spawners, &b.drop_tiles)
    {
        return ColumnProgress::Declined;
    }
    // Lazy COW: all no-op/nonmatching runs stay in the existing read scanner.
    // Only this already-proven changed write can acquire a unique Chunk.
    let mut writer = InertColumnEdit {
        chunk: Arc::make_mut(chunk),
        journal: if b.record_overlay_edits { Some(edits) } else { None },
        light_revision,
    };
    #[cfg(test)]
    { b.inert_column_batches += 1; }
    let mut candidate = first;
    let mut dirty_marked = false;
    // No callback can replace this cache during a retained column run. Drop
    // the optional borrow on every return, including an already-read fallback.
    let mut precipitation = b.precipitation.get_mut(&pos).map(Vec::as_mut_slice);
    loop {
        let p = candidate.position;
        if let Some(cache) = precipitation.as_deref_mut() {
            invalidate_precipitation_column(cache, p);
        }
        // The outer scanner (or this function below) counted the predicate.
        b.profile.access_reads += 2;
        b.profile.access_writes += 1;
        writer.set(p, candidate.old, ore);
        // Both original light phases are proven no-ops at this position. Keep
        // each remaining publication at its original per-write boundary.
        if b.record_inert_change_diagnostics { b.output.changed.push(p); }
        if !dirty_marked {
            b.dirty_chunks.insert(pos);
            // No callback/provider mutation can intervene in this retained
            // column run. Later marks would only hit its same-column cache.
            provider.mark_block_modified(pos);
            dirty_marked = true;
        }
        if b.record_inert_change_diagnostics { b.block_effects.changed.push(p); }
        #[cfg(test)]
        {
            b.inert_ore_writes += 1;
            b.inert_column_writes += 1;
        }

        // Never speculate block states across a callback/fallback. The writer
        // and the local dirty cache are dropped on every return below.
        loop {
            let Some(&p) = positions.get(cursor) else { return ColumnProgress::Advanced { cursor }; };
            if !valid(p) || cp(p) != pos { return ColumnProgress::Advanced { cursor }; }
            let old = writer.chunk.get(p.x.rem_euclid(16), p.y, p.z.rem_euclid(16));
            b.profile.access_reads += 1;
            cursor += 1;
            if old >> 4 != replace_id { continue; }
            if old == ore {
                if let Some(cache) = precipitation.as_deref_mut() {
                    invalidate_precipitation_column(cache, p);
                }
                b.profile.access_reads += 1;
                continue;
            }
            candidate = ReadCandidate { position: p, old };
            let column = (p.z.rem_euclid(16) * 16 + p.x.rem_euclid(16)) as usize;
            if !light.sections[p.y as usize / 16] || p.y >= light.height[column]
                || has_target_metadata(writer.chunk, p, &b.live_containers, &b.live_spawners, &b.drop_tiles)
            {
                return ColumnProgress::ReadFallback { candidate, cursor };
            }
            break;
        }
    }
}

#[cfg(test)]
#[path = "runtime_inert_column_tests.rs"]
mod tests;
