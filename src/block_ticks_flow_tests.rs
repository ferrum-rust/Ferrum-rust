//! Retain the old Vec/EnumSet emulation to check both selected directions and
//! every recursive world access, including reads that publish new block state.
use super::*;
use std::cell::{Cell, RefCell};

fn original_flow_directions(world: &impl BlockAccess, p: IVec3, fluid: u16) -> Vec<IVec3> {
    let mut best = 1000;
    let mut directions = Vec::new();
    for d in HORIZONTAL {
        let at = p + d;
        if flow_blocked(world, at) || level(world, at, fluid) == 0 { continue; }
        let cost = if flow_blocked(world, at - IVec3::Y) {
            flow_cost(world, at, fluid, 1, -d)
        } else { 0 };
        if cost < best { directions.clear(); }
        if cost <= best { directions.push(d); best = cost; }
    }
    directions.sort_by_key(|d| match *d {
        IVec3::NEG_Z => 2, IVec3::Z => 3, IVec3::NEG_X => 4, _ => 5,
    });
    directions
}

#[derive(Debug, PartialEq)]
enum Event {
    Accessible([i32; 3], bool),
    Read([i32; 3], u16),
    Publication([i32; 3], u16, u64),
    Nested(Vec<[i32; 3]>),
}
struct TraceWorld {
    case: i32,
    fluid: u16,
    reference: bool,
    reentrant: bool,
    entered: Cell<bool>,
    reads: Cell<usize>,
    blocks: RefCell<BTreeMap<[i32; 3], u16>>,
    random: RefCell<JavaRandom>,
    events: RefCell<Vec<Event>>,
}
impl TraceWorld {
    fn new(case: i32, fluid: u16, reference: bool, reentrant: bool) -> Self {
        Self { case, fluid, reference, reentrant, entered: Cell::new(false), reads: Cell::new(0),
            blocks: RefCell::new(BTreeMap::new()), random: RefCell::new(JavaRandom::new(189)),
            events: RefCell::new(Vec::new()) }
    }
    fn initial(&self, p: IVec3) -> u16 {
        match self.case {
            0 => 0, // Four equal immediate downward exits.
            1 => 16, // Every direction is blocked.
            2 => if p.y <= 1 && p != IVec3::new(0, 1, 1) { 16 } else { 0 },
            _ => {
                let n = (p.x * 13 + p.z * 7 + self.case).rem_euclid(17);
                if p.y <= 1 { return if n < 3 { 0 } else { 16 }; }
                match n {
                    0 => 16,
                    1 => 64 << 4, // Special flow obstacle despite its shape.
                    2 => 90 << 4,
                    3 => self.fluid << 4,
                    4 => (self.fluid << 4) | 3,
                    5 => if self.fluid == 8 { 11 << 4 } else { 9 << 4 },
                    _ => 0,
                }
            }
        }
    }
}
impl BlockAccess for TraceWorld {
    fn get(&self, p: IVec3) -> u16 {
        self.reads.set(self.reads.get() + 1);
        if self.reentrant && self.reads.get() == 23 && !self.entered.replace(true) {
            // Model a synchronous neighbor publication that changes later
            // probes, reseeds a shared RNG, and performs its own flow search.
            let state = self.fluid << 4;
            let next = p + IVec3::X;
            self.blocks.borrow_mut().insert(next.to_array(), state);
            let rng = {
                let mut random = self.random.borrow_mut();
                let seed = random.next_long() ^ self.reads.get() as i64;
                random.set_seed(seed);
                random.state()
            };
            self.events.borrow_mut().push(Event::Publication(next.to_array(), state, rng));
            let at = IVec3::new(8, 2, -8);
            let directions = if self.reference { original_flow_directions(self, at, self.fluid) }
                else { flow_directions(self, at, self.fluid).collect() };
            self.events.borrow_mut().push(Event::Nested(directions.iter().map(|p| p.to_array()).collect()));
        }
        let state = self.blocks.borrow().get(&p.to_array()).copied().unwrap_or_else(|| self.initial(p));
        self.events.borrow_mut().push(Event::Read(p.to_array(), state));
        state
    }
    fn chunk_loaded(&self, _: ChunkPos) -> bool { true }
    fn accessible(&self, p: IVec3) -> bool {
        let available = (0..HEIGHT).contains(&p.y) && (self.case != 5 || p.x >= -1);
        self.events.borrow_mut().push(Event::Accessible(p.to_array(), available));
        available
    }
    fn set_before_added(&mut self, _: IVec3, _: u16) -> bool { panic!("flow search must not write") }
}

#[test]
fn flow_mask_preserves_ties_better_cost_reset_and_eager_enum_order() {
    let at = IVec3::new(0, 2, 0);
    let order = [IVec3::NEG_Z, IVec3::Z, IVec3::NEG_X, IVec3::X];
    for (case, expected) in [(0, order.to_vec()), (1, vec![]), (2, vec![IVec3::Z])] {
        let world = TraceWorld::new(case, 8, false, false);
        assert_eq!(flow_directions(&world, at, 8).collect::<Vec<_>>(), expected);
    }
    let world = TraceWorld::new(0, 8, false, false);
    let directions = flow_directions(&world, at, 8);
    let reads = world.reads.get();
    for d in HORIZONTAL { world.blocks.borrow_mut().insert((at + d).to_array(), 16); }
    assert_eq!(directions.collect::<Vec<_>>(), order);
    assert_eq!(world.reads.get(), reads, "selection must finish before flow writes begin");
}

#[test]
fn flow_mask_preserves_original_recursive_reads_and_reentrant_publications() {
    let at = IVec3::new(0, 2, 0);
    let mut nested = 0;
    for case in 0..8 {
        for fluid in [8, 10] {
            for reentrant in [false, true] {
                let expected = TraceWorld::new(case, fluid, true, reentrant);
                let actual = TraceWorld::new(case, fluid, false, reentrant);
                let reference = original_flow_directions(&expected, at, fluid);
                let directions = flow_directions(&actual, at, fluid).collect::<Vec<_>>();
                assert_eq!(directions, reference, "case {case} fluid {fluid} reentrant {reentrant}");
                assert_eq!(*actual.events.borrow(), *expected.events.borrow(), "ordered world accesses");
                assert_eq!(*actual.blocks.borrow(), *expected.blocks.borrow(), "publication side effects");
                assert_eq!(actual.random.borrow().state(), expected.random.borrow().state(), "shared callback RNG");
                assert_eq!(actual.reads.get(), expected.reads.get());
                nested += usize::from(actual.entered.get());
            }
        }
    }
    assert!(nested > 0, "fixture must exercise recursive flow during a read");
}
