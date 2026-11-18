use super::*;
use super::super::near_arena::{Arena, Limits, PageFactory, PageKind, Request, Token};
use super::super::near_arena_plan::{Args, PageKey, Plan, Prepared};

struct Fixture { entries: Vec<Info>, reads: Cell<usize> }
impl Fixture { fn new(entries: Vec<Info>) -> Self { Self { entries, reads: Cell::new(0) } } }
impl Rows for Fixture {
    fn len(&self) -> usize { self.entries.len() }
    fn info(&self, index: usize) -> Info {
        self.reads.set(self.reads.get() + 1);
        let row = &self.entries[index];
        Info { backing: row.backing, width: row.width, range: row.range.clone() }
    }
}
struct Forward<'a>(&'a Fixture);
impl Source for Forward<'_> {
    fn len(&self) -> usize { self.0.entries.len() }
    fn row(&self, index: usize) -> Row<'_> {
        let row = &self.0.entries[index];
        Row { backing: row.backing, width: row.width, ranges: std::slice::from_ref(&row.range) }
    }
}
#[derive(Default)]
struct Factory { next: usize }
impl PageFactory for Factory {
    type Handle = usize; type Error = ();
    fn create(&mut self, _: PageKind, _: u64) -> Result<usize, ()> {
        let id = self.next; self.next += 1; Ok(id)
    }
}
fn request(width: IndexWidth) -> Request {
    Request { vertices: 4, indices: 18, vertex_bytes: 128, index_bytes: 18 * width.bytes(),
        vertex_reserved: 128, index_reserved: if width == IndexWidth::U16 { 40 } else { 72 },
        width, max_local_index: Some(3) }
}
fn owners() -> (Arena<usize>, [Token; 8]) {
    let mut arena = Arena::new(near_arena::Mode::ArenaMdi,
        Limits { vertex_page: 512, index_page: 512, owners: 8 }).unwrap();
    let mut factory = Factory::default();
    let tokens = std::array::from_fn(|i| {
        let width = if i < 4 { IndexWidth::U16 } else { IndexWidth::U32 };
        let mut pending = arena.begin_update(None, request(width), &mut factory).unwrap();
        let _ = pending.target(); pending.commit()
    });
    (arena, tokens)
}
fn limits(max: u32) -> DrawLimits { DrawLimits::negotiated(true, true, true, Some(max)) }
fn alpha_rows(tokens: [Token; 8]) -> Fixture {
    let shared = |index: usize, range: Range<u32>| Info { backing: Backing::Arena(tokens[index]),
        width: if index < 4 { IndexWidth::U16 } else { IndexWidth::U32 }, range };
    Fixture::new(vec![shared(0, 6..18), shared(1, 12..18), shared(2, 6..12), shared(3, 6..18),
        Info { backing: Backing::Dedicated, width: IndexWidth::U16, range: 6..18 },
        shared(4, 6..18),
        Info { backing: Backing::Dedicated, width: IndexWidth::U16, range: 6..6 },
        shared(5, 12..18), shared(6, 6..12), shared(7, 6..18)])
}

#[derive(Debug, PartialEq, Eq)]
enum Flat {
    Dedicated { original: usize, width: IndexWidth, range: Range<u32> },
    Shared { key: PageKey, argument: Args },
}
fn original_draw(recording: &Recording<'_, usize>, original: usize, row: &Info) -> Flat {
    match row.backing {
        Backing::Dedicated => Flat::Dedicated { original, width: row.width, range: row.range.clone() },
        Backing::Arena(token) => {
            let b = recording.binding(token).unwrap();
            let stride = if row.width == IndexWidth::U16 { 2 } else { 4 };
            // Independent original allocation-slice addressing. Do not call
            // Binding::draw, reverse_index, fill_ranges or planner validation.
            Flat::Shared { key: PageKey { arena_id: b.arena_id, vertex_page: b.vertex_page,
                index_page: b.index_page, width: row.width },
                argument: Args { index_count: row.range.end - row.range.start, instance_count: 1,
                    first_index: (b.index_range.start / stride + u64::from(row.range.start)) as u32,
                    base_vertex: (b.vertex_range.start / 32) as i32, first_instance: 0 } }
        }
    }
}
fn original_reverse(recording: &Recording<'_, usize>, rows: &Fixture) -> Vec<Flat> {
    rows.entries.iter().enumerate().rev().filter(|(_, row)| !row.range.is_empty())
        .map(|(original, row)| original_draw(recording, original, row)).collect()
}
fn expanded<const N: usize>(ready: &Prepared<'_, '_, '_, usize, ReverseSource<'_, '_, Fixture>, N>,
    rows: &Fixture) -> Vec<Flat>
{
    let mut draws = Vec::new();
    for command in ready.commands() {
        match command {
            Command::Direct { row, range } => {
                // Exercise the production mapping used by DrawPlan::draw.
                let original = reverse_index(rows.entries.len(), *row).unwrap();
                let original_row = &rows.entries[original];
                let info = Info { backing: original_row.backing, width: original_row.width, range: range.clone() };
                draws.push(original_draw(ready.recording(), original, &info));
            }
            Command::Multi { row, binding, first_arg, count } => {
                let b = ready.binding(*row).unwrap().unwrap();
                assert_eq!(*binding, PageKey { arena_id: b.arena_id, vertex_page: b.vertex_page,
                    index_page: b.index_page, width: b.request.width });
                for &argument in &ready.args()[*first_arg as usize..(*first_arg + *count) as usize] {
                    draws.push(Flat::Shared { key: *binding, argument });
                }
            }
        }
    }
    draws
}

#[test]
fn reverse_adapter_matches_original_indices_and_ranges_without_capacity_growth() {
    assert_eq!(Mode::default(), Mode::Direct);
    assert_eq!(reverse_index(0, 0), None);
    assert_eq!(reverse_index(usize::MAX, usize::MAX), None);
    assert_eq!(reverse_index(usize::MAX, usize::MAX - 1), Some(0));
    let rows = Fixture::new((0..MAX_RECORDS).map(|i| Info { backing: Backing::Dedicated,
        width: if i % 2 == 0 { IndexWidth::U16 } else { IndexWidth::U32 },
        range: (i as u32 * 6)..(i as u32 * 6 + if i % 3 == 0 { 0 } else { 6 }) }).collect());
    let mut ranges = Vec::with_capacity(MAX_RECORDS);
    let allocation = (ranges.as_ptr(), ranges.capacity());
    assert!(fill_ranges(&rows, &mut ranges, MAX_RECORDS));
    let independent_order: Vec<_> = rows.entries.iter().enumerate().rev().collect();
    let source = ReverseSource { rows: &rows, ranges: &ranges };
    assert_eq!(source.len(), MAX_RECORDS);
    for (at, (original, expected)) in independent_order.iter().enumerate() {
        assert_eq!(reverse_index(rows.len(), at), Some(*original));
        let actual = source.row(at);
        assert_eq!(actual.backing, expected.backing); assert_eq!(actual.width, expected.width);
        assert_eq!(actual.ranges, std::slice::from_ref(&expected.range));
    }
    assert_eq!((ranges.as_ptr(), ranges.capacity()), allocation);
    rows.reads.set(0);
    assert!(!fill_ranges(&rows, &mut ranges, MAX_RECORDS - 1));
    assert!(ranges.is_empty()); assert_eq!(rows.reads.get(), 0, "reject before any input traversal");
    assert_eq!((ranges.as_ptr(), ranges.capacity()), allocation);
    let mut tiny = Vec::with_capacity(1);
    assert!(!fill_ranges(&rows, &mut tiny, MAX_RECORDS));
    assert!(tiny.is_empty()); assert_eq!(rows.reads.get(), 0);
    let empty = Fixture::new(Vec::new());
    assert!(fill_ranges(&empty, &mut ranges, MAX_RECORDS)); assert!(ranges.is_empty());
}

#[test]
fn alpha_plan_expands_to_original_reverse_order_across_pages_widths_and_native_splits() {
    let (mut arena, tokens) = owners(); let rows = alpha_rows(tokens);
    let recording = arena.begin_recording().unwrap();
    let expected = original_reverse(&recording, &rows); assert_eq!(expected.len(), 9);
    let mut ranges = Vec::with_capacity(MAX_RECORDS);
    assert!(fill_ranges(&rows, &mut ranges, MAX_RECORDS));
    let source = ReverseSource { rows: &rows, ranges: &ranges };
    let mut plan = BoundedPlan::<MAX_RECORDS>::new().unwrap();
    for native in [1, 2, 3, 4, u32::MAX] {
        let ready = plan.build(near_arena::Mode::ArenaMdi, limits(native), &recording, &source).unwrap();
        assert_eq!(expanded(&ready, &rows), expected, "native limit{native}");
        assert_eq!(ready.summary().records, 9);
        if native == 1 { assert_eq!(ready.summary().multi_calls, 0); assert_eq!(ready.summary().direct_calls, 9); }
        if native == 3 {
            assert_eq!(ready.summary().multi_calls, 2);
            assert_eq!(ready.summary().multi_records, 6);
            assert_eq!(ready.summary().direct_calls, 3);
        }
        if native >= 4 { assert_eq!(ready.summary().multi_calls, 2); assert_eq!(ready.summary().multi_records, 8); }
    }
    recording.discarded();
}

#[test]
fn opaque_and_reverse_alpha_plans_keep_distinct_arguments_under_one_recording() {
    let (mut arena, tokens) = owners(); let rows = alpha_rows(tokens);
    let opaque_rows = Fixture::new(rows.entries.iter().map(|row| Info { backing: row.backing,
        width: row.width, range: 0..6 }).collect());
    let source_opaque = Forward(&opaque_rows);
    let mut ranges = Vec::with_capacity(MAX_RECORDS);
    assert!(fill_ranges(&rows, &mut ranges, MAX_RECORDS));
    let source_alpha = ReverseSource { rows: &rows, ranges: &ranges };
    let mut opaque_plan = Plan::new().unwrap();
    let mut alpha_plan = BoundedPlan::<MAX_RECORDS>::new().unwrap();
    assert!(opaque_plan.reserved_storage_bytes() > alpha_plan.reserved_storage_bytes());
    let recording = arena.begin_recording().unwrap();
    {
        let opaque = opaque_plan.build(near_arena::Mode::ArenaMdi, limits(3), &recording, &source_opaque).unwrap();
        let opaque_args = opaque.argument_bytes().to_vec();
        let opaque_commands = opaque.commands().to_vec();
        let opaque_pointer = opaque.args().as_ptr();
        let expected = original_reverse(&recording, &rows);
        let alpha = alpha_plan.build(near_arena::Mode::ArenaMdi, limits(3), opaque.recording(), &source_alpha).unwrap();
        assert_eq!(expanded(&alpha, &rows), expected);
        assert!(!opaque_args.is_empty() && !alpha.argument_bytes().is_empty());
        assert_ne!(opaque_pointer, alpha.args().as_ptr(), "separate plans cannot share argument storage");
        assert_ne!(opaque_args.as_slice(), alpha.argument_bytes(), "different prefixes/order must produce different streams");
        assert_eq!(opaque.argument_bytes(), opaque_args);
        assert_eq!(opaque.commands(), opaque_commands);
        assert_eq!(opaque.args().as_ptr(), opaque_pointer);
    }
    recording.discarded();
}

#[test]
fn small_alpha_capacity_fails_whole_plan_and_reuses_storage_after_bounded_fallback() {
    let (mut arena, tokens) = owners();
    let mut rows = Fixture::new((0..4).map(|i| Info { backing: Backing::Arena(tokens[i]),
        width: IndexWidth::U16, range: 6..18 }).collect());
    let mut ranges = Vec::with_capacity(4); let mut plan = BoundedPlan::<4>::new().unwrap();
    let bytes = plan.reserved_storage_bytes(); let recording = arena.begin_recording().unwrap();
    assert!(fill_ranges(&rows, &mut ranges, 4));
    let pointers;
    {
        let source = ReverseSource { rows: &rows, ranges: &ranges };
        let ready = plan.build(near_arena::Mode::ArenaMdi, limits(3), &recording, &source).unwrap();
        assert_eq!(expanded(&ready, &rows), original_reverse(&recording, &rows));
        assert_eq!(ready.summary().multi_calls, 1); assert_eq!(ready.summary().direct_calls, 1);
        pointers = (ready.commands().as_ptr(), ready.args().as_ptr());
    }
    rows.entries.push(Info { backing: Backing::Dedicated, width: IndexWidth::U16, range: 0..0 });
    assert!(!fill_ranges(&rows, &mut ranges, 4)); assert!(ranges.is_empty());
    // Even a surplus empty row is rejected: a limited traversal cannot emit
    // a prefix then replay the complete original list and duplicate draws.
    let deliberately_overfull: Vec<_> = rows.entries.iter().rev().map(|row| row.range.clone()).collect();
    let source = ReverseSource { rows: &rows, ranges: &deliberately_overfull };
    assert!(matches!(plan.build(near_arena::Mode::ArenaMdi, limits(3), &recording, &source),
        Err(near_arena_plan::Error::TooManyRecords)));
    rows.entries.pop(); assert!(fill_ranges(&rows, &mut ranges, 4));
    {
        let source = ReverseSource { rows: &rows, ranges: &ranges };
        let ready = plan.build(near_arena::Mode::ArenaMdi, limits(3), &recording, &source).unwrap();
        assert_eq!(expanded(&ready, &rows), original_reverse(&recording, &rows));
        assert_eq!((ready.commands().as_ptr(), ready.args().as_ptr()), pointers);
    }
    assert_eq!(plan.reserved_storage_bytes(), bytes);
    recording.discarded();
}
