//! Opt-in finite-run GPU encoder timing. No mapping/polling occurs while recording.
//! Every ticket's start/end/resolve/copy must be submitted once, in order, on one Queue.
use anyhow::{anyhow, bail, ensure, Result};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_ROWS: usize = 131_072;
const BYTES_PER_ROW: u64 = 16;
const READBACK_BYTES: u64 = MAX_ROWS as u64 * BYTES_PER_ROW;
const RESOLVE_BYTES: u64 = 256;
static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

#[path = "gpu_timestamp_stages.rs"]
mod stages;
pub(crate) use stages::Label;

/// Additional capability is requested only for the explicit stage diagnostic.
pub(crate) fn required_features_stages(adapter: &wgpu::Adapter) -> Result<wgpu::Features> {
    supported_stages(adapter.features(), adapter.get_downlevel_capabilities().flags)
}
fn supported_stages(available: wgpu::Features, downlevel: wgpu::DownlevelFlags) -> Result<wgpu::Features> {
    let base = supported(available, downlevel)?;
    ensure!(available.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES),
        "GPU stage timestamps require TIMESTAMP_QUERY_INSIDE_PASSES");
    Ok(base | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES)
}

fn features() -> wgpu::Features {
    wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS
}

fn supported(available: wgpu::Features, downlevel: wgpu::DownlevelFlags) -> Result<wgpu::Features> {
    ensure!(available.contains(features()), "GPU timestamp diagnostic requires TIMESTAMP_QUERY and TIMESTAMP_QUERY_INSIDE_ENCODERS");
    ensure!(downlevel.contains(wgpu::DownlevelFlags::NONBLOCKING_QUERY_RESOLVE),
        "GPU timestamp diagnostic requires nonblocking queue-timeline query resolve");
    Ok(features())
}

/// Call before requesting the Device. Union these bits with its existing features;
/// never silently retry by removing arena features when this diagnostic is requested.
pub(crate) fn required_features(adapter: &wgpu::Adapter) -> Result<wgpu::Features> {
    supported(adapter.features(), adapter.get_downlevel_capabilities().flags)
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CpuTimes {
    pub acquire_ms: f64,
    pub encode_ms: f64,
    pub submit_ms: f64,
    pub present_ms: f64,
}

impl CpuTimes {
    fn valid(self) -> bool {
        [self.acquire_ms, self.encode_ms, self.submit_ms, self.present_ms]
            .into_iter().all(|v| v.is_finite() && v >= 0.)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Ticket { owner: u64, serial: u64, slot: u32 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase { Reserved, Started, Ended }

#[derive(Clone, Copy, Debug)]
struct Pending { ticket: Ticket, phase: Phase, renderer_frame_id: u64, application_seconds: f32 }

#[derive(Clone, Copy, Debug)]
struct Stamp { sample_index: Option<usize>, begin_ms: Option<f64>, end_ms: f64 }

#[derive(Clone, Copy, Debug)]
struct Row {
    renderer_frame_id: u64,
    application_seconds: f32,
    cpu: CpuTimes,
    stamp: Option<Stamp>,
    raw: Option<[u64; 2]>,
    gpu_ms: Option<f64>,
    invalid: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Terminal { Open, Finishing, Complete, Failed }

struct Book {
    owner: u64,
    serial: u64,
    limit: usize,
    rows: Vec<Row>,
    pending: Option<Pending>,
    pending_stamp: Option<usize>,
    terminal: Terminal,
    failure: Option<String>,
    attempted: u64,
    overflow: u64,
    canceled_reserved: u64,
    late_reserves: u64,
}

impl Book {
    fn new(limit: usize) -> Result<Self> {
        ensure!(limit <= MAX_ROWS, "timestamp row capacity exceeds fixed maximum");
        let owner = NEXT_OWNER.fetch_update(Ordering::Relaxed, Ordering::Relaxed,
            |n| n.checked_add(1)).map_err(|_| anyhow!("timestamp capture identity exhausted"))?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(limit).map_err(|e| anyhow!("timestamp CPU row allocation: {e}"))?;
        Ok(Self { owner, serial: 0, limit, rows, pending: None, pending_stamp: None,
            terminal: Terminal::Open, failure: None, attempted: 0, overflow: 0,
            canceled_reserved: 0, late_reserves: 0 })
    }

    fn fail(&mut self, reason: impl Into<String>) {
        // A completed immutable readback is not reopened or invalidated by a
        // mistakenly repeated late caller; that operation still returns Err.
        if self.terminal == Terminal::Complete { return; }
        if self.failure.is_none() { self.failure = Some(reason.into()); }
        self.terminal = Terminal::Failed;
        self.pending_stamp = None;
    }

    fn open(&self) -> Result<()> {
        ensure!(self.terminal == Terminal::Open, "timestamp diagnostic is not recording: {}",
            self.failure.as_deref().unwrap_or("terminal capture"));
        Ok(())
    }

    // Called before acquisition, including failed/disabled render attempts.
    fn clear_pending_present(&mut self) {
        self.pending_stamp = None;
        if self.terminal != Terminal::Open { return; }
        if let Some(pending) = self.pending.take() {
            if pending.phase == Phase::Reserved {
                self.canceled_reserved = self.canceled_reserved.saturating_add(1);
            } else {
                self.pending = Some(pending);
                self.fail("a recorded timestamp ticket was abandoned or its submission is uncertain");
            }
        }
    }

    fn reserve(&mut self, renderer_frame_id: u64, application_seconds: f32) -> Option<Ticket> {
        self.clear_pending_present();
        if self.terminal != Terminal::Open {
            self.late_reserves = self.late_reserves.saturating_add(1);
            return None;
        }
        self.attempted = self.attempted.saturating_add(1);
        if !application_seconds.is_finite() {
            self.fail("nonfinite renderer context time");
            return None;
        }
        if self.rows.len() == self.limit {
            self.overflow = self.overflow.saturating_add(1);
            return None;
        }
        let Some(serial) = self.serial.checked_add(1) else {
            self.fail("timestamp ticket serial exhausted"); return None;
        };
        self.serial = serial;
        let ticket = Ticket { owner: self.owner, serial, slot: self.rows.len() as u32 };
        self.pending = Some(Pending { ticket, phase: Phase::Reserved, renderer_frame_id, application_seconds });
        Some(ticket)
    }

    fn validate(&self, ticket: Ticket, phase: Phase) -> Result<Pending> {
        self.open()?;
        let pending = self.pending.ok_or_else(|| anyhow!("no pending timestamp ticket"))?;
        ensure!(pending.ticket == ticket && pending.phase == phase,
            "timestamp ticket identity or phase mismatch");
        ensure!(ticket.slot as usize == self.rows.len() && self.rows.len() < self.limit,
            "timestamp ticket offset is out of bounds");
        Ok(pending)
    }

    fn started(&mut self, ticket: Ticket) -> Result<()> {
        self.validate(ticket, Phase::Reserved)?;
        self.pending.as_mut().unwrap().phase = Phase::Started;
        Ok(())
    }

    fn ended(&mut self, ticket: Ticket) -> Result<()> {
        self.validate(ticket, Phase::Started)?;
        self.pending.as_mut().unwrap().phase = Phase::Ended;
        Ok(())
    }

    fn submitted(&mut self, ticket: Ticket, cpu: CpuTimes) -> Result<()> {
        let pending = self.validate(ticket, Phase::Ended)?;
        ensure!(cpu.valid(), "invalid CPU timestamp context");
        // Capacity was acquired before measurement and admission forbids growth.
        self.rows.push(Row { renderer_frame_id: pending.renderer_frame_id,
            application_seconds: pending.application_seconds, cpu, stamp: None,
            raw: None, gpu_ms: None, invalid: None });
        self.pending = None;
        self.pending_stamp = Some(ticket.slot as usize);
        Ok(())
    }

    fn presented(&mut self, sample_index: Option<usize>, begin_ms: Option<f64>, end_ms: f64) -> Result<()> {
        self.open()?;
        let Some(index) = self.pending_stamp.take() else { return Ok(()); };
        ensure!(end_ms.is_finite() && end_ms >= 0., "invalid App present endpoint");
        ensure!(sample_index.is_some() == begin_ms.is_some(), "incomplete App interval identity");
        if let Some(begin) = begin_ms {
            ensure!(begin.is_finite() && begin >= 0. && begin <= end_ms, "invalid App present interval");
        }
        self.rows[index].stamp = Some(Stamp { sample_index, begin_ms, end_ms });
        Ok(())
    }

    fn start_finish(&mut self) -> Result<bool> {
        match self.terminal {
            Terminal::Complete => return Ok(false),
            Terminal::Failed => bail!("timestamp diagnostic failed: {}", self.failure.as_deref().unwrap_or("unknown")),
            Terminal::Finishing => bail!("timestamp finalization is already in progress"),
            Terminal::Open => {}
        }
        if let Some(pending) = self.pending {
            if pending.phase != Phase::Reserved {
                self.fail("cannot finalize an encoded ticket without a confirmed submission");
                bail!("{}", self.failure.as_deref().unwrap());
            }
            self.canceled_reserved = self.canceled_reserved.saturating_add(1);
            self.pending = None;
        }
        self.pending_stamp = None;
        self.terminal = Terminal::Finishing;
        Ok(true)
    }
}

fn elapsed_ms(pair: [u64; 2], period_ns: f64) -> std::result::Result<f64, &'static str> {
    if !period_ns.is_finite() || period_ns <= 0. { return Err("invalid_timestamp_period"); }
    let delta = pair[1].checked_sub(pair[0]).ok_or("reversed_or_unknown_wrap")?;
    let ms = delta as f64 * period_ns / 1_000_000.;
    if !ms.is_finite() { return Err("nonfinite_gpu_elapsed"); }
    Ok(ms)
}

/// Owns only diagnostic resources/scalars. Same Device and serial Queue for its whole life.
/// Dropping an unfinalized capture loses its report; it does not wait for the GPU.
pub(crate) struct Capture {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    period_ns: f64,
    book: Book,
    stages: Option<stages::Book>,
}

impl Capture {
    /// Construct before measurement, after required_features checked the actual Adapter.
    /// Error scopes here are initialization-only; no per-frame scope/poll is introduced.
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Self> {
        Self::new_inner(device, queue, false)
    }
    pub(crate) fn new_stages(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Self> {
        Self::new_inner(device, queue, true)
    }
    fn new_inner(device: &wgpu::Device, queue: &wgpu::Queue, stage_mode: bool) -> Result<Self> {
        ensure!(!stage_mode || device.features().contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES),
            "GPU stage timestamp feature was not enabled on Device");
        ensure!(device.features().contains(features()), "timestamp features were not enabled on Device");
        let period_ns = f64::from(queue.get_timestamp_period());
        ensure!(period_ns.is_finite() && period_ns > 0., "invalid Queue timestamp period");
        let book = Book::new(MAX_ROWS)?;
        // Optional CPU storage is reserved before measurement and before error scopes.
        let stages = if stage_mode { Some(stages::Book::new(MAX_ROWS)?) } else { None };
        device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("benchmark encoder timestamps"), ty: wgpu::QueryType::Timestamp,
            count: if stage_mode { stages::QUERY_COUNT } else { 2 },
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("benchmark timestamp resolve"), size: RESOLVE_BYTES,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("benchmark timestamp append-only readback"),
            size: if stage_mode { stages::READBACK_BYTES } else { READBACK_BYTES },
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let validation = pollster::block_on(device.pop_error_scope());
        let allocation = pollster::block_on(device.pop_error_scope());
        if let Some(error) = validation.or(allocation) { bail!("timestamp resource creation: {error}"); }
        Ok(Self { queries, resolve, readback, period_ns, book, stages })
    }

    pub(crate) fn clear_pending_present(&mut self) { self.book.clear_pending_present(); }
    pub(crate) fn reserve(&mut self, frame_id: u64, application_seconds: f32) -> Option<Ticket> {
        self.book.reserve(frame_id, application_seconds)
    }

    pub(crate) fn encode_start(&mut self, encoder: &mut wgpu::CommandEncoder, ticket: &Ticket) -> Result<()> {
        // Mark first: panic/abort while recording cannot make the slot reusable.
        self.book.started(*ticket)?;
        if let Some(stages) = &mut self.stages {
            if let Err(error) = stages.begin(*ticket) { self.book.fail(error.to_string()); return Err(error); }
        }
        encoder.write_timestamp(&self.queries, 0);
        Ok(())
    }

    pub(crate) fn encode_end(&mut self, encoder: &mut wgpu::CommandEncoder, ticket: &Ticket) -> Result<()> {
        self.book.validate(*ticket, Phase::Started)?;
        if let Some(stages) = &mut self.stages {
            let last = match stages.end(*ticket) {
                Ok(index) => index,
                Err(error) => { self.book.fail(error.to_string()); return Err(error); }
            };
            encoder.write_timestamp(&self.queries, last);
            encoder.resolve_query_set(&self.queries, 0..last+1, &self.resolve, 0);
            encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.readback,
                u64::from(ticket.slot) * stages::STRIDE, u64::from(last+1)*8);
        } else {
            encoder.write_timestamp(&self.queries, 1);
            encoder.resolve_query_set(&self.queries, 0..2, &self.resolve, 0);
            encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.readback,
                u64::from(ticket.slot) * BYTES_PER_ROW, BYTES_PER_ROW);
        }
        self.book.ended(*ticket)
    }

    /// Optional marker hooks. Screenshots/raw encoding without a pending ticket
    /// do not join the measured capture. A malformed live ticket fails closed.
    pub(crate) fn mark_encoder(&mut self, encoder: &mut wgpu::CommandEncoder, label: Label) -> Result<()> {
        if self.stages.is_none() { return Ok(()); }
        let Some(pending) = self.book.pending else { return Ok(()); };
        let result: Result<()> = (|| {
            self.book.validate(pending.ticket, Phase::Started)?;
            let index = self.stages.as_mut().unwrap().mark(pending.ticket, label, stages::Placement::Encoder)?;
            encoder.write_timestamp(&self.queries, index);
            Ok(())
        })();
        if let Err(error) = &result { self.book.fail(format!("{error:#}")); }
        result
    }
    pub(crate) fn mark_pass(&mut self, pass: &mut wgpu::RenderPass<'_>, label: Label) -> Result<()> {
        if self.stages.is_none() { return Ok(()); }
        let Some(pending) = self.book.pending else { return Ok(()); };
        let result: Result<()> = (|| {
            self.book.validate(pending.ticket, Phase::Started)?;
            let index = self.stages.as_mut().unwrap().mark(pending.ticket, label, stages::Placement::Pass)?;
            pass.write_timestamp(&self.queries, index);
            Ok(())
        })();
        if let Err(error) = &result { self.book.fail(format!("{error:#}")); }
        result
    }

    /// Only after this ticket's encoder was submitted once and frame.present returned.
    pub(crate) fn submitted(&mut self, ticket: Ticket, cpu: CpuTimes) -> Result<()> {
        let result: Result<()> = (|| {
            if let Some(stages) = &self.stages { stages.validate_submission(ticket)?; }
            self.book.submitted(ticket, cpu)?;
            if let Some(stages) = &mut self.stages { stages.submitted(ticket)?; }
            Ok(())
        })();
        if let Err(error) = &result { self.book.fail(error.to_string()); }
        result
    }

    /// Uses App's already-read successful-present Instant; no clock read here.
    pub(crate) fn presented(&mut self, sample_index: Option<usize>, begin_application_ms: Option<f64>, end_application_ms: f64) -> Result<()> {
        let result = self.book.presented(sample_index, begin_application_ms, end_application_ms);
        if let Err(error) = &result { self.book.fail(error.to_string()); }
        result
    }

    /// TERMINAL. Call only after measurement and its final screenshot are finished.
    /// Maps once, waits once outside frame_seconds, and never permits future capture.
    pub(crate) fn finish(&mut self, device: &wgpu::Device) -> Result<()> {
        if !self.book.start_finish()? { return Ok(()); }
        if self.book.rows.is_empty() {
            self.book.terminal = Terminal::Complete;
            return Ok(());
        }
        let stride = if self.stages.is_some() { stages::STRIDE } else { BYTES_PER_ROW };
        let used = self.book.rows.len() as u64 * stride;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.readback.slice(..used).map_async(wgpu::MapMode::Read, move |result| { let _ = tx.send(result); });
        device.poll(wgpu::Maintain::Wait);
        let mapped = match rx.try_recv() {
            Ok(Ok(())) => true,
            Ok(Err(error)) => { self.book.fail(format!("timestamp terminal map failed: {error}")); false }
            Err(error) => { self.book.fail(format!("timestamp terminal map callback unavailable: {error}")); false }
        };
        if !mapped {
            self.readback.unmap();
            bail!("{}", self.book.failure.as_deref().unwrap());
        }
        {
            let bytes = self.readback.slice(..used).get_mapped_range();
            for (index, (row, pair)) in self.book.rows.iter_mut().zip(bytes.chunks_exact(stride as usize)).enumerate() {
                let raw = if let Some(stages) = &mut self.stages {
                    stages.decode(index, pair)
                } else {
                    [u64::from_ne_bytes(pair[..8].try_into().unwrap()),
                     u64::from_ne_bytes(pair[8..16].try_into().unwrap())]
                };
                row.raw = Some(raw);
                match elapsed_ms(raw, self.period_ns) {
                    Ok(ms) => row.gpu_ms = Some(ms),
                    Err(reason) => row.invalid = Some(reason),
                }
            }
        }
        self.readback.unmap();
        self.book.terminal = Terminal::Complete;
        Ok(())
    }

    pub(crate) fn report(&self) -> serde_json::Value {
        let complete = self.book.terminal == Terminal::Complete;
        let mut sum = 0.;
        let mut maximum = 0f64;
        let mut valid = 0usize;
        let mut zero = 0usize;
        for row in &self.book.rows {
            if let Some(ms) = row.gpu_ms {
                sum += ms; maximum = maximum.max(ms); valid += 1; zero += usize::from(ms == 0.);
            }
        }
        let rows = complete.then(|| self.book.rows.iter().enumerate().map(|(index,row)| {
            serde_json::json!({ "row": index, "renderer_frame_id": row.renderer_frame_id,
                "application_seconds": row.application_seconds,
                "sample_index": row.stamp.and_then(|s| s.sample_index),
                "begin_application_ms": row.stamp.and_then(|s| s.begin_ms),
                "end_application_ms": row.stamp.map(|s| s.end_ms),
                "cpu_ms": {"surface_acquire":row.cpu.acquire_ms,"encode":row.cpu.encode_ms,
                    "finish_and_submit":row.cpu.submit_ms,"present":row.cpu.present_ms},
                "raw_ticks": row.raw, "gpu_encoder_ms": row.gpu_ms, "invalid_reason": row.invalid })
        }).collect::<Vec<_>>());
        serde_json::json!({ "enabled": true, "diagnostic_not_baseline": true,
            "state": match self.book.terminal {Terminal::Open=>"recording",Terminal::Finishing=>"finishing",Terminal::Complete=>"complete",Terminal::Failed=>"failed"},
            "failure": self.book.failure, "timestamp_period_ns": self.period_ns,
            "capacity_rows": self.book.limit, "query_count": if self.stages.is_some() { stages::QUERY_COUNT } else { 2 }, "resolve_bytes": RESOLVE_BYTES,
            "readback_bytes": if self.stages.is_some() { stages::READBACK_BYTES } else { READBACK_BYTES },
            "stages": self.stages.as_ref().map_or_else(|| serde_json::json!({"enabled":false}), |s| s.report(complete,self.period_ns)), "cpu_row_size_bytes": std::mem::size_of::<Row>(),
            "cpu_reserved_bytes": self.book.rows.capacity() * std::mem::size_of::<Row>(),
            "attempted": self.book.attempted, "submitted": self.book.rows.len(),
            "overflow_skipped": self.book.overflow, "canceled_reserved": self.book.canceled_reserved,
            "late_reserves": self.book.late_reserves,
            "unanchored_rows": self.book.rows.iter().filter(|r|r.stamp.is_none()).count(),
            "valid_gpu_rows": valid, "invalid_gpu_rows": self.book.rows.iter().filter(|r|r.invalid.is_some()).count(),
            "quantized_zero_rows": zero, "gpu_encoder_total_ms": complete.then_some(sum),
            "gpu_encoder_max_ms": (valid>0).then_some(maximum), "rows": rows,
            "scope": "Full frame encoder execution span between two GPU markers; includes both eyes, postfx and UI. Excludes pending Queue::write_buffer transfers executed before the user encoder, queue waiting before the first marker, and query resolve/copy after the end marker. GPU stalls within the markers are included; backend/driver timestamp reordering limits exact boundaries.",
            "clock_note": "Raw GPU ticks have no App clock calibration. CPU present endpoints use App.started; application_seconds is only the earlier render context. sample_index is the existing present interval index (null for the first anchor). Acquire of frame N precedes its GPU work and may wait earlier submissions; compare prior row identities separately, without claiming the exact waited image/submission or display/input latency.",
            "overhead_note": if self.stages.is_some() { "Append-only stage diagnostic: at most20 timestamp writes/resets, one prefix resolve and one used-prefix copy into a fixed160-byte row. Whole-frame and stage reporting share one20MiB readback; auxiliary CPU rows are preallocated. No per-frame application map/poll/wait or row allocation. Backend allocations/barriers and timestamp perturbation remain possible; one terminal map/wait after measurement. Missing stages are absent, reordered intervals remain invalid, and fixed-stride padding is never interpreted." } else { "Append-only: two timestamp writes/resets, one resolve and one 16-byte copy per captured frame; no application map/poll/wait or per-frame row allocation. wgpu may allocate command/tracker data and emit whole-buffer barriers. One terminal map/wait occurs after measurement. Capacity overflow truncates the late window; no live slot reuse or busy-slot selection. Fixed resource figures exclude backend allocations and terminal JSON serialization." } })
    }
}

#[cfg(test)]
#[path = "gpu_timestamps_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "gpu_timestamps_gpu_tests.rs"]
mod gpu_tests;
