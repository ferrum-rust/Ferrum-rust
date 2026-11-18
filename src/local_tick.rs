//! One in-flight local tick with exclusive ownership of gameplay state.
//! The caller keeps a read-only render copy and defers mutations until poll
//! returns the actual owner. No state merge, tick skipping or prediction occurs.
use crate::{gameplay::{self, Outcome, Simulation}, player::Player,
    resources::Resources, settings::Settings, world::World, Options};
use anyhow::{bail, ensure, Context, Result};
use glam::{DVec3, IVec3};
use serde_json::Value;
use std::{panic::{catch_unwind, AssertUnwindSafe},
    sync::{atomic::{AtomicUsize, Ordering}, mpsc, Arc},
    thread::{self, JoinHandle}, time::{Duration, Instant}};

#[derive(Clone)]
pub(crate) struct LocalTickState {
    pub world: World,
    pub player: Player,
    pub sim: Simulation,
}

pub(crate) struct TickInput {
    pub resources: Arc<Resources>,
    pub settings: Settings,
    pub options: Options,
    pub left: bool,
    pub right: bool,
    pub chest: Option<IVec3>,
    pub death_screen: bool,
}

#[derive(Default)]
struct TickOutput {
    outcome: Outcome,
    chat: Vec<Value>,
    lan_chat: Vec<String>,
    world_sounds: Vec<(String, DVec3, f32, f32)>,
    open_command_block: Option<IVec3>,
    died: bool,
}

pub(crate) struct CompletedTick {
    pub id: u64,
    pub state: LocalTickState,
    pub outcome: Outcome,
    pub chat: Vec<Value>,
    pub lan_chat: Vec<String>,
    pub world_sounds: Vec<(String, DVec3, f32, f32)>,
    pub open_command_block: Option<IVec3>,
    pub portal_travel: Option<gameplay::PortalTravel>,
    pub died: bool,
    pub elapsed: Duration,
}

fn step(state: &mut LocalTickState, input: TickInput) -> TickOutput {
    let LocalTickState { world, player, sim } = state;
    sim.server_scene.chest_lids.tick(world, input.chest);
    let mut outcome = sim.tick(world, player, &input.resources, &input.settings, input.left, input.right);
    let mut result = TickOutput::default();
    if let Some(host) = sim.lan.clone() {
        host.update(world, sim, player, &input.settings, &input.resources, &input.options, &mut outcome);
        result.lan_chat = host.drain();
    }
    for (name, pos, volume, pitch) in std::mem::take(&mut outcome.world_sounds) {
        result.world_sounds.push((name.to_owned(), pos, volume, pitch));
    }
    for position in std::mem::take(&mut outcome.command_blocks) {
        let commands = gameplay::command_blocks::execute(sim, player, world,
            &input.resources, &input.options, position);
        outcome.changed.extend(commands.changed);
        result.chat.extend(commands.messages);
        result.world_sounds.extend(commands.sounds);
    }
    result.open_command_block = outcome.open_command_block.take();
    if sim.health <= 0. && !input.death_screen {
        sim.die(player);
        result.died = true;
        outcome.portal_travel = None;
    }
    result.outcome = outcome;
    result
}

pub(crate) struct LocalTickWorker {
    owner: OwnerWorker<LocalTickState, TickInput, TickOutput>,
}
impl LocalTickWorker {
    pub fn new() -> Result<Self> { Ok(Self { owner: OwnerWorker::new("local-tick", step)? }) }
    pub fn with_wake(wake: impl Fn() + Send + 'static) -> Result<Self> {
        Ok(Self { owner: OwnerWorker::with_wake("local-tick", step, wake)? })
    }
    /// The state slot is consumed only after successful submission. Busy or
    /// disconnected submissions leave the authoritative state in the slot.
    pub fn submit(&mut self, state: &mut Option<LocalTickState>, input: TickInput) -> Result<u64> {
        self.owner.submit(state, input)
    }
    pub fn busy(&self) -> bool { self.owner.in_flight.is_some() }
    pub fn poll(&mut self) -> Result<Option<CompletedTick>> { self.owner.poll().map(|v| v.map(completed)) }
    /// Explicit save/exit barrier. Normal frames must use poll instead.
    pub fn drain(&mut self) -> Result<Option<CompletedTick>> { self.owner.drain().map(|v| v.map(completed)) }
    /// Dispose of an obsolete render copy off the frame thread. At most two
    /// retirements may be queued; a rejected value is returned to its caller.
    pub fn retire(&mut self, state: LocalTickState) -> std::result::Result<(), LocalTickState> {
        self.owner.retire(state)
    }
    /// A panicked tick is not safe to resume or save as completed. Preserve its
    /// owned state for diagnostics/recovery instead of silently losing it.
    pub fn take_failed_state(&mut self) -> Option<LocalTickState> { self.owner.failed_state.take() }
}
fn completed(done: OwnerCompletion<LocalTickState, TickOutput>) -> CompletedTick {
    let TickOutput { mut outcome, chat, lan_chat, world_sounds, open_command_block, died } = done.output;
    let portal_travel = outcome.portal_travel.take();
    CompletedTick { id: done.id, state: done.state, outcome, chat, lan_chat, world_sounds,
        open_command_block, portal_travel, died, elapsed: done.elapsed }
}

struct Job<S, I> { id: u64, state: S, input: I }
enum Request<S, I> { Tick(Job<S, I>), Retire(S) }
struct Reply<S, O> { id: u64, state: S, output: std::result::Result<O, String>, elapsed: Duration }
struct OwnerCompletion<S, O> { id: u64, state: S, output: O, elapsed: Duration }
struct OwnerWorker<S, I, O> {
    sender: Option<mpsc::Sender<Request<S, I>>>,
    receiver: mpsc::Receiver<Reply<S, O>>,
    thread: Option<JoinHandle<()>>,
    in_flight: Option<u64>,
    next_id: u64,
    retired: Arc<AtomicUsize>,
    failed: Option<String>,
    failed_state: Option<S>,
}
impl<S: Send + 'static, I: Send + 'static, O: Send + 'static> OwnerWorker<S, I, O> {
    fn new(name: &str, mut execute: impl FnMut(&mut S, I) -> O + Send + 'static) -> Result<Self> {
        Self::with_wake(name, move |state, input| execute(state, input), || {})
    }
    fn with_wake(name: &str, mut execute: impl FnMut(&mut S, I) -> O + Send + 'static,
        wake: impl Fn() + Send + 'static) -> Result<Self> {
        let (sender, jobs) = mpsc::channel::<Request<S, I>>();
        let (reply, receiver) = mpsc::channel();
        let retired = Arc::new(AtomicUsize::new(0));
        let pending_retired = retired.clone();
        let thread = thread::Builder::new().name(name.into()).spawn(move || {
            while let Ok(request) = jobs.recv() {
                match request {
                    Request::Retire(state) => {
                        drop(state);
                        pending_retired.fetch_sub(1, Ordering::Relaxed);
                    }
                    Request::Tick(Job { id, mut state, input }) => {
                        let start = Instant::now();
                        let output = catch_unwind(AssertUnwindSafe(|| execute(&mut state, input)))
                            .map_err(|panic| panic.downcast_ref::<String>().cloned()
                                .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                                .unwrap_or_else(|| "non-string panic".into()));
                        let failed = output.is_err();
                        if reply.send(Reply { id, state, output, elapsed: start.elapsed() }).is_err() { break; }
                        // Publish ownership before waking the event loop: its
                        // first nonblocking poll must already see the reply.
                        wake();
                        if failed { break; }
                    }
                }
            }
        }).context("Start local gameplay worker")?;
        Ok(Self { sender: Some(sender), receiver, thread: Some(thread), in_flight: None,
            next_id: 1, retired, failed: None, failed_state: None })
    }
    fn submit(&mut self, state: &mut Option<S>, input: I) -> Result<u64> {
        ensure!(self.in_flight.is_none(), "Local tick is already in flight");
        ensure!(self.failed.is_none(), "Local tick worker failed: {}", self.failed.as_deref().unwrap_or(""));
        let sender = self.sender.as_ref().context("Local tick worker is closed")?;
        let state_value = state.take().context("Missing authoritative local tick state")?;
        let id = self.next_id;
        let request = Request::Tick(Job { id, state: state_value, input });
        if let Err(error) = sender.send(request) {
            if let Request::Tick(job) = error.0 { *state = Some(job.state); }
            self.failed = Some("disconnected".into());
            bail!("Local tick worker disconnected before submission");
        }
        self.in_flight = Some(id);
        self.next_id = self.next_id.wrapping_add(1);
        Ok(id)
    }
    fn accept(&mut self, reply: Reply<S, O>) -> Result<OwnerCompletion<S, O>> {
        if self.in_flight != Some(reply.id) {
            self.failed_state = Some(reply.state);
            self.failed = Some("unexpected completion sequence".into());
            self.in_flight = None;
            bail!("Local tick completion sequence does not match its owner");
        }
        self.in_flight = None;
        match reply.output {
            Ok(output) => Ok(OwnerCompletion { id: reply.id, state: reply.state, output, elapsed: reply.elapsed }),
            Err(message) => {
                self.failed_state = Some(reply.state);
                self.failed = Some(message.clone());
                bail!("Local tick {} failed: {message}", reply.id)
            }
        }
    }
    fn poll(&mut self) -> Result<Option<OwnerCompletion<S, O>>> {
        if self.in_flight.is_none() { return Ok(None); }
        match self.receiver.try_recv() {
            Ok(reply) => self.accept(reply).map(Some),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.in_flight = None;
                self.failed = Some("disconnected during tick".into());
                bail!("Local tick worker disconnected during a tick")
            }
        }
    }
    fn drain(&mut self) -> Result<Option<OwnerCompletion<S, O>>> {
        if self.in_flight.is_none() { return Ok(None); }
        let reply = match self.receiver.recv() {
            Ok(reply) => reply,
            Err(_) => {
                self.in_flight = None;
                self.failed = Some("disconnected during barrier".into());
                bail!("Local tick worker disconnected during barrier");
            }
        };
        self.accept(reply).map(Some)
    }
    fn retire(&mut self, state: S) -> std::result::Result<(), S> {
        let Some(sender) = &self.sender else { return Err(state) };
        if self.failed.is_some() || self.retired.try_update(Ordering::Relaxed, Ordering::Relaxed,
            |n| (n < 2).then_some(n + 1)).is_err() { return Err(state); }
        if let Err(error) = sender.send(Request::Retire(state)) {
            self.retired.fetch_sub(1, Ordering::Relaxed);
            if let Request::Retire(state) = error.0 { return Err(state); }
            unreachable!();
        }
        Ok(())
    }
}
impl<S, I, O> Drop for OwnerWorker<S, I, O> {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() { let _ = thread.join(); }
    }
}

#[cfg(test)]
#[path = "local_tick_tests.rs"]
mod tests;
