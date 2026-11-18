//! Ordered ore writes whose complete vanilla callback chain is inert. Missing
//! columns, custom hooks and unusual target metadata keep the eventful path.
use super::*;

fn inert_replacement(ore: u16, replace_id: u16) -> bool {
    matches!(replace_id, 1 | 87)
        && crate::world::inert_ore_replacement(ore)
}

impl Access<'_> {
    /// Returns false without world accesses when the caller must use the
    /// ordinary ore adapter. An accepted row can still fall back cell by cell.
    pub(super) fn replace_inert_ore_line(
        &mut self,
        positions: &[IVec3],
        ore: u16,
        replace_id: u16,
    ) -> bool {
        if !inert_replacement(ore, replace_id) { return false; }
        let enabled = self.with(|_, b| {
            #[cfg(test)]
            if b.reference_inert_ore || b.reference_ore_batch { return false; }
            b.hooks.supports_inert_ore_writes()
        });
        if !enabled { return false; }

        enum Next { Read(IVec3), Write(IVec3), Remainder, Done }
        let mut cursor = 0;
        while cursor < positions.len() {
            let next = self.with(|provider, b| {
                // A fallback can reenter a host or publish another chunk. Do
                // not retain the capability or any chunk borrow across it.
                if !b.hooks.supports_inert_ore_writes() { return Next::Remainder; }
                while cursor < positions.len() {
                    let p = positions[cursor];
                    if !valid(p) { return Next::Read(p); }
                    let pos = cp(p);
                    let Some(chunk) = b.world.chunks.get(&pos) else { return Next::Read(p); };
                    let mut changed = None;
                    {
                        // This cache reference is local to the read/no-op scan;
                        // release it before the writer or any fallback can reenter.
                        let mut precipitation = b.precipitation.get_mut(&pos).map(Vec::as_mut_slice);
                        // The first candidate is already valid and in this column.
                        // Resolve Dense/Cold once for this read-only run; no write,
                        // callback or maintenance can intervene before it ends.
                        let states = chunk.blocks.as_slice();
                        // Keep one loaded-column lookup for runs of non-matches.
                        // Stop at each changed write so its lighting completes
                        // before reading any later candidate's block state.
                        while cursor < positions.len() {
                            let p = positions[cursor];
                            if !valid(p) || cp(p) != pos { break; }
                            let old = states[crate::world::Chunk::index(
                                p.x.rem_euclid(16), p.y, p.z.rem_euclid(16))];
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
                            let at = p.to_array();
                            let local = [p.x.rem_euclid(16), p.y, p.z.rem_euclid(16)];
                            if (!b.live_containers.is_empty() && b.live_containers.contains_key(&at))
                                || (!b.live_spawners.is_empty() && b.live_spawners.contains_key(&at))
                                || (!b.drop_tiles.is_empty() && b.drop_tiles.contains_key(&at))
                                || (!chunk.tiles.is_empty() && chunk.tiles.contains_key(&local))
                            {
                                return Next::Write(p);
                            }
                            changed = Some((p, old));
                            break;
                        }
                    }
                    let Some((p, old)) = changed else { continue; };
                    match super::inert_column::try_write_column(
                        provider, b, positions, cursor,
                        super::inert_column::ReadCandidate { position: p, old }, ore, replace_id,
                    ) {
                        super::inert_column::ColumnProgress::Declined => {},
                        super::inert_column::ColumnProgress::Advanced { cursor: next } => {
                            cursor = next;
                            continue;
                        }
                        super::inert_column::ColumnProgress::ReadFallback { candidate, cursor: next } => {
                            cursor = next;
                            return Next::Write(candidate.position);
                        }
                    }
                    // before_set_attempt runs even for same-state attempts
                    // above. No callback or RNG draw can intervene here.
                    invalidate_precipitation(&mut b.precipitation, pos, p);
                    // Preserve the two logical reads in set_block_flags and
                    // BlockTicks::write, in addition to the ore predicate read.
                    b.profile.access_reads += 2;
                    b.profile.access_writes += 1;
                    let written = if b.record_overlay_edits { b.world.set(p, ore) }
                        else { b.world.set_raw(p, ore) };
                    assert!(written, "Published chunk missing during inert ore write");
                    // World::set_raw keeps Chunk::set, COW snapshots, tick
                    // indexes, top and the per-block lighting revision intact.
                    // The guarded tile maps and every vanilla block callback
                    // are unchanged/no-op for these exact old/new block IDs.
                    b.light.opaque_block_replaced(&b.world, p, old, ore);
                    if b.record_inert_change_diagnostics { b.output.changed.push(p); }
                    b.dirty_chunks.insert(pos);
                    provider.mark_block_modified(pos);
                    // Flags=2 requests no neighbor notification. Neither the
                    // scheduler nor drops has any event for these inert IDs.
                    b.light.after_block_added_state(&b.world, p, old, ore);
                    if b.record_inert_change_diagnostics { b.block_effects.changed.push(p); }
                    #[cfg(test)]
                    { b.inert_ore_writes += 1; }
                }
                Next::Done
            });
            match next {
                Next::Read(p) => {
                    cursor += 1;
                    if self.get_block(p).expect("Native feature block read failed") >> 4 == replace_id {
                        self.set_block(p, ore).expect("Native feature block write failed");
                    }
                }
                Next::Write(p) => { self.set_block(p, ore).expect("Native feature block write failed"); }
                Next::Remainder => {
                    for &p in &positions[cursor..] {
                        if self.get_block(p).expect("Native feature block read failed") >> 4 == replace_id {
                            self.set_block(p, ore).expect("Native feature block write failed");
                        }
                    }
                    break;
                }
                Next::Done => break,
            }
        }
        true
    }
}

#[cfg(test)]
#[path = "runtime_inert_ore_tests.rs"]
mod tests;
