//! Optional fixed-capacity stage ledger. No GPU resource, clock or per-frame allocation.
use super::{elapsed_ms, Ticket, MAX_ROWS};
use anyhow::{anyhow, ensure, Result};

pub(super) const QUERY_COUNT: u32 = 20;
pub(super) const STRIDE: u64 = QUERY_COUNT as u64 * 8;
pub(super) const READBACK_BYTES: u64 = MAX_ROWS as u64 * STRIDE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Label {
    WorldBegin(u8), NearBegin(u8), NearEnd(u8), FarEnd(u8),
    SkyDynamicEnd(u8), AlphaEnd(u8), WorldEnd(u8),
    PostBegin, PostEnd, GuiBegin, GuiEnd,
}
impl Label {
    fn name(self) -> &'static str {
        match self {
            Self::WorldBegin(_) => "world_begin", Self::NearBegin(_) => "near_begin",
            Self::NearEnd(_) => "near_end", Self::FarEnd(_) => "far_end",
            Self::SkyDynamicEnd(_) => "sky_dynamic_end", Self::AlphaEnd(_) => "alpha_end",
            Self::WorldEnd(_) => "world_end", Self::PostBegin => "post_begin",
            Self::PostEnd => "post_end", Self::GuiBegin => "gui_begin", Self::GuiEnd => "gui_end",
        }
    }
    fn eye(self) -> Option<u8> {
        match self {
            Self::WorldBegin(e) | Self::NearBegin(e) | Self::NearEnd(e) | Self::FarEnd(e)
            | Self::SkyDynamicEnd(e) | Self::AlphaEnd(e) | Self::WorldEnd(e) => Some(e),
            _ => None,
        }
    }
    fn placement(self) -> Placement {
        match self {
            Self::NearBegin(_) | Self::NearEnd(_) | Self::FarEnd(_)
            | Self::SkyDynamicEnd(_) | Self::AlphaEnd(_) => Placement::Pass,
            _ => Placement::Encoder,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Placement { Encoder, Pass }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Point { WholeBegin, Stage(Label), WholeEnd }
impl Point {
    fn name(self) -> &'static str {
        match self { Self::WholeBegin => "whole_begin", Self::WholeEnd => "whole_end", Self::Stage(s) => s.name() }
    }
    fn eye(self) -> Option<u8> { match self { Self::Stage(s) => s.eye(), _ => None } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Position { Begin, Eye(u8, u8), BetweenEyes, AfterEyes, Post, AfterPost, Gui, AfterGui, End }

#[derive(Clone, Copy, Debug)]
struct Pending {
    ticket: Ticket,
    points: [Point; QUERY_COUNT as usize],
    used: u8,
    position: Position,
    eyes: u8,
    postfx: bool,
    gui: bool,
}
impl Pending {
    fn new(ticket: Ticket) -> Self {
        Self { ticket, points: [Point::WholeBegin; QUERY_COUNT as usize], used: 1,
            position: Position::Begin, eyes: 0, postfx: false, gui: false }
    }
    fn push(&mut self, point: Point, next: Position) -> Result<u32> {
        ensure!((self.used as u32) < QUERY_COUNT, "stage query prefix exceeds fixed capacity");
        let index = self.used as u32;
        self.points[index as usize] = point;
        self.used += 1;
        self.position = next;
        Ok(index)
    }
    fn mark(&mut self, label: Label, placement: Placement) -> Result<u32> {
        use Label::*;
        use Position::*;
        ensure!(label.placement() == placement, "stage marker used on the wrong encoder/pass boundary");
        let next = match (self.position, label) {
            (Begin, WorldBegin(0)) => Eye(0, 0),
            (BetweenEyes, WorldBegin(1)) => Eye(1, 0),
            (Eye(e, 0), NearBegin(n)) if e == n => Eye(e, 1),
            (Eye(e, 1), NearEnd(n)) if e == n => Eye(e, 2),
            (Eye(e, 2), FarEnd(n)) if e == n => Eye(e, 3),
            (Eye(e, 3), SkyDynamicEnd(n)) if e == n => Eye(e, 4),
            (Eye(e, 4), AlphaEnd(n)) if e == n => Eye(e, 5),
            (Eye(e, 5), WorldEnd(n)) if e == n => if e == 0 { BetweenEyes } else { AfterEyes },
            (Begin | BetweenEyes | AfterEyes, PostBegin) => Post,
            (Post, PostEnd) => AfterPost,
            (Begin | BetweenEyes | AfterEyes | AfterPost, GuiBegin) => Gui,
            (Gui, GuiEnd) => AfterGui,
            _ => return Err(anyhow!("stage marker order, eye, or duplicate mismatch")),
        };
        let index = self.push(Point::Stage(label), next)?;
        match label {
            WorldEnd(e) => self.eyes = e + 1,
            PostEnd => self.postfx = true,
            GuiEnd => self.gui = true,
            _ => {}
        }
        Ok(index)
    }
    fn end(&mut self) -> Result<u32> {
        ensure!(matches!(self.position, Position::Begin | Position::BetweenEyes | Position::AfterEyes
            | Position::AfterPost | Position::AfterGui), "stage region was not closed before whole encoder end");
        self.push(Point::WholeEnd, Position::End)
    }
}

#[derive(Clone, Copy)]
struct Row { pending: Pending, raw: Option<[u64; QUERY_COUNT as usize]> }
pub(super) struct Book { rows: Vec<Row>, pending: Option<Pending>, limit: usize }
impl Book {
    pub(super) fn new(limit: usize) -> Result<Self> {
        ensure!(limit <= MAX_ROWS, "stage row capacity exceeds fixed maximum");
        let mut rows = Vec::new();
        rows.try_reserve_exact(limit).map_err(|e| anyhow!("stage CPU row allocation: {e}"))?;
        Ok(Self { rows, pending: None, limit })
    }
    pub(super) fn begin(&mut self, ticket: Ticket) -> Result<()> {
        ensure!(self.pending.is_none(), "previous stage ticket is still pending");
        ensure!(ticket.slot as usize == self.rows.len() && self.rows.len() < self.limit,
            "stage ticket row is out of bounds");
        self.pending = Some(Pending::new(ticket));
        Ok(())
    }
    fn pending(&mut self, ticket: Ticket) -> Result<&mut Pending> {
        let pending = self.pending.as_mut().ok_or_else(|| anyhow!("stage ticket is absent"))?;
        ensure!(pending.ticket == ticket, "stage ticket identity mismatch");
        Ok(pending)
    }
    pub(super) fn mark(&mut self, ticket: Ticket, label: Label, placement: Placement) -> Result<u32> {
        self.pending(ticket)?.mark(label, placement)
    }
    pub(super) fn end(&mut self, ticket: Ticket) -> Result<u32> { self.pending(ticket)?.end() }
    pub(super) fn validate_submission(&self, ticket: Ticket) -> Result<()> {
        let p = self.pending.as_ref().ok_or_else(|| anyhow!("stage submission has no pending ticket"))?;
        ensure!(p.ticket == ticket && p.position == Position::End, "stage submission identity or completion mismatch");
        ensure!(ticket.slot as usize == self.rows.len() && self.rows.len() < self.limit,
            "stage submitted row is out of bounds");
        Ok(())
    }
    pub(super) fn submitted(&mut self, ticket: Ticket) -> Result<()> {
        self.validate_submission(ticket)?;
        self.rows.push(Row { pending: self.pending.take().unwrap(), raw: None });
        Ok(())
    }
    pub(super) fn decode(&mut self, index: usize, bytes: &[u8]) -> [u64; 2] {
        // Only the written prefix is read. Fixed-stride padding has no meaning.
        let row = &mut self.rows[index];
        let count = row.pending.used as usize;
        let mut raw = [0; QUERY_COUNT as usize];
        for (out, value) in raw[..count].iter_mut().zip(bytes[..count*8].chunks_exact(8)) {
            *out = u64::from_ne_bytes(value.try_into().unwrap());
        }
        let pair = [raw[0], raw[count - 1]];
        row.raw = Some(raw);
        pair
    }
    pub(super) fn report(&self, complete: bool, period: f64) -> serde_json::Value {
        let mut valid = 0usize; let mut invalid = 0usize; let mut zeros = 0usize;
        for row in &self.rows {
            if let Some(raw) = row.raw {
                for pair in raw[..row.pending.used as usize].windows(2) {
                    match elapsed_ms([pair[0],pair[1]],period) {
                        Ok(ms) => { valid += 1; zeros += usize::from(ms == 0.); }
                        Err(_) => invalid += 1,
                    }
                }
            }
        }
        let rows = complete.then(|| self.rows.iter().enumerate().map(|(index, row)| {
            let p = &row.pending;
            let raw = row.raw.expect("terminal stage rows must be decoded");
            let points = p.points[..p.used as usize].iter().enumerate().map(|(i, point)|
                serde_json::json!({"query_index":i,"label":point.name(),"eye":point.eye(),"raw_ticks":raw[i]})).collect::<Vec<_>>();
            let intervals = (0..p.used as usize - 1).map(|i| {
                let result = elapsed_ms([raw[i],raw[i+1]],period);
                serde_json::json!({"from":i,"to":i+1,"gpu_ms":result.as_ref().ok().copied(),
                    "invalid_reason":result.err()})
            }).collect::<Vec<_>>();
            serde_json::json!({"row":index,"mode":if p.eyes>0 {"world"} else {"no_world"},
                "eyes":p.eyes,"postfx":p.postfx,"gui":p.gui,"used_query_count":p.used,
                "points":points,"intervals":intervals})
        }).collect::<Vec<_>>());
        serde_json::json!({"enabled":true,"schema_version":1,"capacity_rows":self.limit,
            "query_count":QUERY_COUNT,"readback_stride_bytes":STRIDE,"readback_bytes":READBACK_BYTES,
            "storage_shared_with_whole_frame":true,"cpu_row_size_bytes":std::mem::size_of::<Row>(),
            "cpu_reserved_bytes":self.rows.capacity()*std::mem::size_of::<Row>(),"submitted":self.rows.len(),
            "valid_intervals":valid,"invalid_intervals":invalid,"quantized_zero_intervals":zeros,"rows":rows,
            "scope":"Ordered adjacent GPU marker regions, per eye; near_begin follows pass begin/clear, alpha includes overlays/shadows/translucency/fire. Panorama and other gaps remain unlabeled intervals. A present region may contain no draws; absence is not zero. Markers may reorder on a backend/driver, so invalid subtractions remain null with their raw values. Stage and whole-frame resource fields describe the same readback buffer, not additive allocations."})
    }
}

#[cfg(test)]
#[path = "gpu_timestamp_stages_tests.rs"]
mod tests;
