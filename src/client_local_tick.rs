//! Single-owner local ticks. While a tick is in flight, Game contains only a
//! presentation mirror: inputs and native/light publications wait for ownership
//! to return. In particular, no block edits are merged into an old tick result.
use super::*;
use crate::local_tick::{CompletedTick, LocalTickState, LocalTickWorker, TickInput};

pub(super) enum DeferredInput {
    Window(WindowId, WindowEvent),
    Look(f64, f64),
    Open(Screen),
}

impl App {
    pub(super) fn simulation_active(&self) -> bool {
        self.local_tick_failure.is_none() && (matches!(self.gui.screen,
            Screen::Playing | Screen::Inventory | Screen::Crafting | Screen::Creative
                | Screen::Chest | Screen::Furnace | Screen::Trading | Screen::Sleeping | Screen::Chat)
            || self.game.as_ref().is_some_and(|g| g.remote.is_some() || g.sim.lan.is_some()))
    }

    /// The local 20 Hz clock is independent of rendering and its FPS cap.
    /// Completion events return ownership and may start a catch-up tick, without
    /// drawing an extra frame or waiting for slow AI on the window thread.
    pub(super) fn service_local_ticks(&mut self, event_loop: &ActiveEventLoop, now: Instant,
        origin: interval_profile::Origin) -> Result<()> {
        let started = self.interval_profile.timer();
        let result = self.service_local_ticks_inner(event_loop, now);
        self.interval_profile.service(origin, started);
        result
    }

    fn service_local_ticks_inner(&mut self, event_loop: &ActiveEventLoop, now: Instant) -> Result<()> {
        if event_loop.exiting() { return Ok(()); }
        anyhow::ensure!(self.local_tick_failure.is_none(), "Local gameplay stopped after a failed tick: {}",
            self.local_tick_failure.as_deref().unwrap_or(""));
        let completed = self.poll_local_tick(event_loop)?;
        if event_loop.exiting() { return Ok(()); }
        let dt = elapsed_since_pump(&mut self.last_tick_pump, now);
        if self.game.as_ref().is_some_and(|g| g.remote.is_none()) {
            let active = self.simulation_active();
            if catchup_publication_due(completed, active, self.accumulator, dt) {
                // A continuously slow worker must still publish generation,
                // lighting and meshes before the next loan, and allow autosave.
                // Otherwise every render frame could see only a busy mirror.
                let start = Instant::now();
                let autosave = self.gui.screen == Screen::Playing
                    && self.last_save.elapsed() > Duration::from_secs(30);
                let g = self.game.as_mut().unwrap();
                let result = g.update_chunks(self.renderer.as_mut().unwrap(), &self.assets, &self.settings)
                    .and_then(|()| if autosave { g.autosave() } else { Ok(false) });
                let elapsed = start.elapsed().as_secs_f64() * 1000.;
                record(&mut g.chunk_timings, "local_tick_catchup_publication", elapsed);
                self.interval_profile.stage_ms(interval_profile::Stage::CatchupPublication, elapsed);
                if result? { self.last_save = Instant::now(); }
            }
            self.advance_ticks(active, dt)?;
        } else if self.game.as_ref().is_some_and(|g| g.remote.is_some()) {
            let poll_err = {
                let g = self.game.as_mut().unwrap();
                let remote = g.remote.as_mut().unwrap();
                remote.visuals.particle_setting = self.settings.particles;
                remote.fancy_graphics = self.settings.fancy;
                remote
                    .poll_world(
                        &mut g.world,
                        &mut g.player,
                        &mut g.sim,
                        &mut g.dirty,
                        self.renderer.as_mut(),
                    )
                    .err()
            };
            if let Some(error) = poll_err {
                if self.options.connect.is_some() && self.options.screenshot.is_some() {
                    anyhow::bail!("Remote screenshot: {error}");
                }
                let msg = format!("{error:#}");
                self.save_quit()?;
                self.open(Screen::Multiplayer);
                self.gui.multiplayer.connection_error(msg);
                return Ok(());
            }
            let active = self.simulation_active();
            let died = self.advance_ticks(active, dt)?;
            if died {
                if self.gui.screen != Screen::Death {
                    self.open(Screen::Death);
                }
                if let Some(g) = &mut self.game {
                    if g.autosave()? {
                        self.last_save = Instant::now();
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn local_tick_deadline(&self, now: Instant) -> Option<Instant> {
        if self.local_tick_failure.is_some() || self.game.is_none() {
            return None;
        }
        if self.game.as_ref().is_some_and(|g| g.remote.is_some()) {
            return Some(next_tick_deadline(self.last_tick_pump, self.accumulator));
        }
        if self.tick_busy() {
            // Real windows always install a completion proxy. Headless test
            // callers without one may still service the owner with a fallback.
            return self.tick_proxy.is_none().then_some(now + Duration::from_millis(1));
        }
        (self.simulation_active() || self.gui.screen == Screen::Death)
            .then(|| next_tick_deadline(self.last_tick_pump, self.accumulator))
    }

    pub(super) fn advance_ticks(&mut self, active: bool, dt: f32) -> Result<bool> {
        if (!active && self.gui.screen != Screen::Death) || self.game.is_none() { return Ok(false); }
        if self.game.as_ref().is_some_and(|g| g.remote.is_some()) {
            accumulate_remote_ticks(&mut self.accumulator, dt);
        } else {
            accumulate_ticks(&mut self.accumulator, dt);
        }
        while self.accumulator >= 0.05 && !self.tick_busy() {
            let g = self.game.as_mut().unwrap();
            if !active {
                g.sim.hurt_camera.tick(g.sim.health <= 0.);
                self.accumulator -= 0.05;
                continue;
            }
            self.gui.boss_bar.tick();
            self.smooth_camera.tick(self.settings.sensitivity);
            let lighting_started = self.interval_profile.timer();
            if let Some(renderer) = self.renderer.as_mut() {
                renderer.tick_lighting(&g.world, &g.player, &g.sim, self.settings.full_detail_distance());
            }
            self.interval_profile.stage(interval_profile::Stage::TickLighting, lighting_started);
            if let Some(remote) = &mut g.remote {
                g.sim.server_scene.chest_lids.tick(&g.world, None);
                let out = remote.tick(&mut g.world, &mut g.player, &mut g.sim,
                    &self.resources, self.left, self.right)?;
                for p in out.changed { g.mark(p); }
                for (name, volume) in out.sounds { self.audio.play(&self.resources, name, volume); }
                for (name, volume, pitch) in out.pitched_sounds {
                    self.audio.play_pitched(&self.resources, name, volume, pitch);
                }
                self.audio.listener(g.player.precise_eye(1.0), g.player.yaw);
                for (name, pos, volume, pitch) in out.world_sounds {
                    self.audio.play_world(&self.resources, name, volume, pitch, pos);
                }
                let close_portal_gui = std::mem::take(&mut g.player.close_portal_gui);
                let dead = g.sim.health <= 0.;
                self.accumulator -= 0.05;
                if dead && self.gui.screen != Screen::Death { return Ok(true); }
                if !dead && close_portal_gui && should_close_portal_gui(self.gui.screen) {
                    self.open(Screen::Playing);
                }
            } else {
                let submit_started = self.interval_profile.timer();
                let result = self.submit_local_tick();
                self.interval_profile.stage(interval_profile::Stage::Submit, submit_started);
                result?;
                self.accumulator -= 0.05;
                // Exactly one authority is in flight. Any catch-up tick starts
                // after completion and ordered input replay on its wake event.
                break;
            }
        }
        Ok(false)
    }

    pub(super) fn tick_busy(&self) -> bool {
        self.local_tick.as_ref().is_some_and(LocalTickWorker::busy)
    }

    /// A visual mouse update is immediate; the same ordered deltas are applied
    /// to the returned authority before another tick can be submitted.
    pub(super) fn look_local(&mut self, dx: f64, dy: f64) {
        if self.tick_busy() { self.deferred_input.push_back(DeferredInput::Look(dx, dy)); }
        if let Some(g) = &mut self.game { g.player.look(dx, dy); }
    }

    pub(super) fn submit_local_tick(&mut self) -> Result<()> {
        anyhow::ensure!(self.local_tick_failure.is_none(), "A failed local tick cannot be resumed");
        if self.local_tick.is_none() {
            self.local_tick = Some(if let Some(proxy) = self.tick_proxy.clone() {
                LocalTickWorker::with_wake(move || { let _ = proxy.send_event(AppEvent::LocalTick); })?
            } else { LocalTickWorker::new()? });
        }
        let g = self.game.as_mut().expect("Local tick needs a world");
        debug_assert!(g.remote.is_none());
        let mut tick_settings = self.settings.clone();
        tick_settings.render_distance = self.settings.full_detail_distance();
        let queued = if self.local_right_clicks > 0 {
            self.local_right_clicks -= 1;
            true
        } else {
            false
        };
        let chest = (self.gui.screen == Screen::Chest).then_some(self.gui.container).flatten();
        if let Some((anchor, _)) = chest.and_then(|pos| sp_chest_anchor_and_center(&g.world, pos)) {
            self.sp_chest_lids.insert(anchor);
        }
        let input = TickInput {
            resources: self.resources.clone(), settings: tick_settings,
            options: g.options.clone(), left: self.left, right: self.right || queued,
            chest,
            death_screen: self.gui.screen == Screen::Death,
        };
        let copy_started = Instant::now();
        let mut copy_phase = g.options.benchmark_streaming.then(Instant::now);
        // Chunk payloads are immutable Arcs. The journal belongs only to the
        // owner; neither drawing nor picking needs to copy it every 50 ms.
        let mirror_world = World { chunks: g.world.chunks.clone(), spawn: g.world.spawn,
            label: g.world.label.clone(), save_path: g.world.save_path.clone(),
            edits: Default::default(), imported: g.world.imported, light_revision: g.world.light_revision };
        if let Some(phase) = &mut copy_phase { g.record_chunk_phase("tick_presentation_world_copy", phase); }
        let mirror_player = g.player.clone();
        if let Some(phase) = &mut copy_phase { g.record_chunk_phase("tick_presentation_player_copy", phase); }
        let (mirror_sim, sim_copy_profile) = if g.options.benchmark_streaming {
            let (copy, profile) = g.sim.presentation_clone_profiled_reusing(&mut g.spare_presentation_brains);
            (copy, Some(profile))
        } else { (g.sim.presentation_clone_reusing(&mut g.spare_presentation_brains), None) };
        if let Some(phase) = &mut copy_phase { g.record_chunk_phase("tick_presentation_sim_copy", phase); }
        let mut state = Some(LocalTickState {
            world: std::mem::replace(&mut g.world, mirror_world),
            player: std::mem::replace(&mut g.player, mirror_player),
            sim: std::mem::replace(&mut g.sim, mirror_sim),
        });
        let elapsed = copy_started.elapsed().as_secs_f64() * 1000.;
        record(&mut g.chunk_timings, "tick_presentation_copy", elapsed);
        self.interval_profile.stage_ms(interval_profile::Stage::Copy, elapsed);
        // Aggregate after the measured copy: no report-map allocation enters a
        // field's clone time. Normal gameplay never requests these profiles.
        if let Some(profile) = sim_copy_profile {
            for (name, ms) in profile.timings() { record(&mut g.chunk_timings, name, ms); }
            for (name, count) in profile.counts() {
                let stats = g.presentation_copy_counts.entry(name).or_default();
                stats[0] += 1; stats[1] += count as u64; stats[2] = stats[2].max(count as u64);
            }
        }
        if let Err(error) = self.local_tick.as_mut().unwrap().submit(&mut state, input) {
            if let Some(state) = state {
                g.world = state.world; g.player = state.player; g.sim = state.sim;
            }
            return Err(error);
        }
        Ok(())
    }

    fn accept_local_tick(&mut self, completed: CompletedTick) -> Result<()> {
        let g = self.game.as_mut().expect("In-flight tick retains its world");
        let mut retired = LocalTickState {
            world: std::mem::replace(&mut g.world, completed.state.world),
            player: std::mem::replace(&mut g.player, completed.state.player),
            sim: std::mem::replace(&mut g.sim, completed.state.sim),
        };
        // Capture this retired projection. An unused older spare moves to the
        // existing retirement queue instead of being dropped on the window thread.
        std::mem::swap(&mut retired.sim.mob_brains, &mut g.spare_presentation_brains);
        // Dropping large AI/container snapshots is also worker work. A bounded
        // retirement queue prevents render copies accumulating during a stall.
        let retired_on_window = self.local_tick.as_mut().unwrap().retire(retired).is_err();
        self.interval_profile.retired_on_window(retired_on_window);
        record(&mut g.chunk_timings, "local_tick_worker", completed.elapsed.as_secs_f64() * 1000.);
        for p in completed.outcome.changed { g.mark(p); }
        for (name, volume) in completed.outcome.sounds { self.audio.play(&self.resources, name, volume); }
        for (name, volume, pitch) in completed.outcome.pitched_sounds {
            self.audio.play_pitched(&self.resources, name, volume, pitch);
        }
        for line in completed.lan_chat { self.gui.chat.push(line, false); }
        for line in completed.chat {
            self.gui.chat.receive(&line.to_string(), 1, &self.resources, &self.settings.language);
        }
        self.audio.listener(g.player.precise_eye(1.0), g.player.yaw);
        for (name, position, volume, pitch) in completed.world_sounds {
            self.audio.play_world(&self.resources, &name, volume, pitch, position);
        }
        if g.remote.is_none() && !self.sp_chest_lids.is_empty() {
            let mut chest_sounds = Vec::new();
            self.sp_chest_lids.retain(|&anchor| {
                let Some((_, chest_center)) = sp_chest_anchor_and_center(&g.world, anchor) else {
                    return false;
                };
                let prev_angle = g.sim.server_scene.chest_lids.angle(&g.world, anchor, 0.0);
                let curr_angle = g.sim.server_scene.chest_lids.angle(&g.world, anchor, 1.0);
                let prev_linear = 1.0 - (1.0 - prev_angle).clamp(0.0, 1.0).cbrt();
                let curr_linear = 1.0 - (1.0 - curr_angle).clamp(0.0, 1.0).cbrt();
                if prev_linear == 0.0 && curr_linear > 0.0 {
                    chest_sounds.push(("random.chestopen", chest_center));
                } else if prev_linear >= 0.5 - 1e-4 && curr_linear < 0.5 - 1e-4 {
                    chest_sounds.push(("random.chestclosed", chest_center));
                }
                curr_angle > 0.0
            });
            for (sound, chest_center) in chest_sounds {
                self.audio
                    .play_world(&self.resources, sound, 0.5, 0.95, chest_center);
            }
        }
        let close_portal_gui = std::mem::take(&mut g.player.close_portal_gui);
        if completed.died {
            self.open(Screen::Death);
            if let Some(g) = &mut self.game {
                if g.autosave()? {
                    self.last_save = Instant::now();
                }
            }
        } else if let Some(travel) = completed.portal_travel {
            if self.gui.screen != Screen::Playing {
                self.open(Screen::Playing);
            }
            self.perform_portal_travel(travel)?;
        } else if let Some(position) = completed.open_command_block {
            self.gui.command_block = ui::command_block::Editor::from_world(
                &g.world, position, &self.resources, &self.settings);
            self.open(Screen::CommandBlock);
        } else if close_portal_gui && should_close_portal_gui(self.gui.screen) {
            self.open(Screen::Playing);
        }
        Ok(())
    }

    pub(super) fn poll_local_tick(&mut self, event_loop: &ActiveEventLoop) -> Result<bool> {
        let result = self.local_tick.as_mut().map(LocalTickWorker::poll).transpose();
        let completed = match result {
            Ok(completed) => completed.flatten(),
            Err(error) => {
                self.local_tick_failure = Some(format!("{error:#}"));
                self.deferred_input.clear();
                if let Some(state) = self.local_tick.as_mut().and_then(LocalTickWorker::take_failed_state) {
                    if let Some(g) = &mut self.game {
                        g.world = state.world; g.player = state.player; g.sim = state.sim;
                    }
                }
                return Err(error);
            }
        };
        let returned_owner = completed.is_some();
        if let Some(completed) = completed {
            let started = self.interval_profile.timer();
            let result = self.accept_local_tick(completed);
            self.interval_profile.stage(interval_profile::Stage::Accept, started);
            result?;
        }
        // Edges are never coalesced: Q, attack presses, slot changes, GUI edits,
        // focus loss and close/save retain the order delivered by winit.
        let replay_started = (!self.deferred_input.is_empty()).then(|| self.interval_profile.timer()).flatten();
        while !self.tick_busy() && !event_loop.exiting() {
            let Some(input) = self.deferred_input.pop_front() else { break; };
            match input {
                DeferredInput::Window(id, event) => self.window_event(event_loop, id, event),
                DeferredInput::Look(dx, dy) => {
                    if self.gui.screen == Screen::Playing { self.look_local(dx, dy); }
                }
                DeferredInput::Open(screen) => self.open(screen),
            }
        }
        self.interval_profile.stage(interval_profile::Stage::InputReplay, replay_started);
        Ok(returned_owner)
    }
}

fn should_close_portal_gui(screen: Screen) -> bool {
    !matches!(
        screen,
        Screen::Playing
            | Screen::Pause
            | Screen::Options
            | Screen::OptionPage(_)
            | Screen::Video
            | Screen::Controls
            | Screen::Languages
            | Screen::Statistics
            | Screen::Achievements
            | Screen::ShareLan
            | Screen::Multiplayer
            | Screen::Realms
            | Screen::Worlds
            | Screen::RenameWorld
            | Screen::DeleteWorld
            | Screen::Create
            | Screen::Main
            | Screen::Death
            | Screen::Loading
    )
}

fn next_tick_deadline(last_pump: Instant, accumulator: f32) -> Instant {
    last_pump + Duration::from_secs_f32((0.05 - accumulator).max(0.))
}

fn catchup_publication_due(completed: bool, active: bool, accumulator: f32, dt: f32) -> bool {
    completed && active && accumulator + dt >= 0.05
}

fn elapsed_since_pump(last: &mut Instant, now: Instant) -> f32 {
    let dt = now.saturating_duration_since(*last).as_secs_f32();
    *last = now.max(*last);
    dt
}

fn accumulate_ticks(accumulator: &mut f32, dt: f32) {
    *accumulator = (*accumulator + dt).min(0.25);
}

fn accumulate_remote_ticks(accumulator: &mut f32, dt: f32) {
    *accumulator = (*accumulator + dt.clamp(0., 0.5)).min(0.5);
}

fn record(timings: &mut std::collections::BTreeMap<&'static str, [f64; 3]>, phase: &'static str, ms: f64) {
    let value = timings.entry(phase).or_default();
    value[0] += 1.; value[1] += ms; value[2] = value[2].max(ms);
}

fn sp_chest_anchor_and_center(world: &World, pos: IVec3) -> Option<(IVec3, glam::DVec3)> {
    let state = world.get(pos);
    if !crate::chest_render::is_chest(state) {
        return None;
    }
    let id = state >> 4;
    let other = (id != 130)
        .then(|| {
            [IVec3::NEG_Z, IVec3::NEG_X, IVec3::Z, IVec3::X]
                .into_iter()
                .map(|step| pos + step)
                .find(|&p| world.get(p) >> 4 == id)
        })
        .flatten();
    let anchor = other.map_or(pos, |p| pos.min(p));
    let center = match other {
        Some(q) => glam::DVec3::new(
            f64::from(pos.x + q.x) * 0.5 + 0.5,
            f64::from(anchor.y) + 0.5,
            f64::from(pos.z + q.z) * 0.5 + 0.5,
        ),
        None => anchor.as_dvec3() + glam::DVec3::splat(0.5),
    };
    Some((anchor, center))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_tick_deadlines_run_twenty_ticks_at_ten_rendered_frames_per_second() {
        let start = Instant::now();
        let end = start + Duration::from_millis(1001);
        let mut last_pump = start;
        let mut accumulator = 0.;
        let mut next_frame = start + Duration::from_millis(100);
        let mut completion = None;
        let (mut frames, mut ticks) = (0, 0);
        loop {
            let wake = completion.unwrap_or_else(|| next_tick_deadline(last_pump, accumulator));
            let now = next_frame.min(wake);
            if now > end { break; }
            if completion == Some(now) { completion = None; }
            accumulate_ticks(&mut accumulator, elapsed_since_pump(&mut last_pump, now));
            if completion.is_none() && accumulator >= 0.05 {
                ticks += 1;
                accumulator -= 0.05;
                completion = Some(now + Duration::from_millis(7));
            }
            if now == next_frame {
                frames += 1;
                next_frame += Duration::from_millis(100);
            }
            assert!(ticks <= 20 && frames <= 10, "deadlines must not spin or add render frames");
        }
        assert_eq!((ticks, frames), (20, 10));
    }

    #[test]
    fn local_tick_clock_preserves_waiting_tick_debt_and_does_not_double_count_a_frame() {
        let start = Instant::now();
        let mut last = start;
        let mut accumulator = 0.;
        let same_frame = start + Duration::from_millis(60);
        accumulate_ticks(&mut accumulator, elapsed_since_pump(&mut last, same_frame));
        let accumulated = accumulator;
        accumulate_ticks(&mut accumulator, elapsed_since_pump(&mut last, same_frame));
        assert_eq!(accumulator, accumulated, "frame plus about_to_wait count elapsed time once");
        accumulator -= 0.05; // submit one owner, then wait through an expensive tick
        let completed = start + Duration::from_millis(180);
        accumulate_ticks(&mut accumulator, elapsed_since_pump(&mut last, completed));
        assert!(next_tick_deadline(last, accumulator) <= completed);
        let mut catchup = 0;
        while accumulator >= 0.05 { accumulator -= 0.05; catchup += 1; }
        assert_eq!(catchup, 2, "all complete 50 ms periods survive an in-flight tick");
        let after_capture = last + Duration::from_millis(2);
        last = after_capture;
        assert_eq!(elapsed_since_pump(&mut last, completed), 0.);
        assert_eq!(last, after_capture, "opening a GUI may reset the clock during completion polling");
    }

    #[test]
    fn local_tick_slow_completions_publish_once_before_each_catchup_loan() {
        let start = Instant::now();
        let mut last = start + Duration::from_millis(50); // first tick submitted
        let mut accumulator = 0.;
        let mut published = Vec::new();
        for tick in 1..=4 {
            // A render wake occurs while the 80ms owner is still running.
            let before = start + Duration::from_millis(50 + 80 * tick - 30);
            let elapsed = elapsed_since_pump(&mut last, before);
            assert!(!catchup_publication_due(false, true, accumulator, elapsed));
            accumulate_ticks(&mut accumulator, elapsed);
            let completion = start + Duration::from_millis(50 + 80 * tick);
            let elapsed = elapsed_since_pump(&mut last, completion);
            assert!(catchup_publication_due(true, true, accumulator, elapsed));
            published.push(tick); // publication sees the returned authority
            accumulate_ticks(&mut accumulator, elapsed);
            accumulator -= 0.05; // next loan happens only after publication
            assert!(!catchup_publication_due(false, true, accumulator,
                elapsed_since_pump(&mut last, completion)), "a second pump must not publish twice");
        }
        assert_eq!(published, vec![1, 2, 3, 4]);
        assert!(!catchup_publication_due(true, false, 0.2, 0.), "pausing prevents catch-up loans");
        assert!(!catchup_publication_due(true, true, 0.01, 0.007), "fast ticks leave normal frame publication unchanged");
    }

    #[test]
    fn progressive_initial_meshes_cover_entry_and_retain_every_distant_column() {
        let empty = Arc::new(Chunk::empty());
        let world = World { chunks: (-17..=17).flat_map(|x| (-17..=17).map(move |z|
            (x, z))).map(|p| (p, empty.clone())).collect(), spawn: Vec3::ZERO,
            label: String::new(), save_path: PathBuf::new(), edits: Default::default(),
            imported: false, light_revision: 0 };
        let near = initial_mesh_positions(&world, (0, 0), true);
        assert_eq!(near.len(), 9);
        assert!(mesh_neighbors((0, 0)).iter().all(|p| near.contains(p)));
        let all = initial_mesh_positions(&world, (0, 0), false);
        assert_eq!(all.len(), 1225);
        let initial = near.iter().copied().collect::<HashSet<_>>();
        let rest = world.chunks.keys().filter(|p| !initial.contains(p)).copied().collect::<HashSet<_>>();
        assert_eq!(initial.union(&rest).copied().collect::<HashSet<_>>(), world.chunks.keys().copied().collect());
        assert!(initial.is_disjoint(&rest));
    }

    #[test]
    fn native_entry_requires_all_nine_lit_columns_and_completed_uploads() {
        let mut column = Chunk::empty();
        column.light = Some(Arc::new(crate::vanilla::lighting::ChunkLight::from_chunk(&Default::default(), false)));
        let column = Arc::new(column);
        let mut world = World { chunks: mesh_neighbors((-2, 3)).into_iter()
                .map(|p| (p, column.clone())).collect(), spawn: Vec3::ZERO,
            label: String::new(), save_path: PathBuf::new(), edits: Default::default(),
            imported: false, light_revision: 0 };
        let uploaded = HashSet::new();
        assert!(entry_meshes_ready(&world, (-2, 3), &uploaded, true));
        assert!(!entry_meshes_ready(&world, (-2, 3), &HashSet::from([(-2, 3)]), true));
        world.chunks.remove(&(-3, 2));
        assert!(!entry_meshes_ready(&world, (-2, 3), &uploaded, true), "absent columns cannot satisfy entry");
        assert!(entry_meshes_ready(&world, (-2, 3), &uploaded, false), "finite imported worlds may omit neighbors");
        world.chunks.insert((-3, 2), Arc::new(Chunk::empty()));
        assert!(!entry_meshes_ready(&world, (-2, 3), &uploaded, true), "unlit terrain is not playable entry");
    }

    #[test]
    fn only_interactive_and_real_streaming_benchmarks_defer_world_entry() {
        use clap::Parser;
        for args in [vec!["ferrum189"], vec!["ferrum189", "--benchmark", "--benchmark-streaming"]] {
            assert!(Options::parse_from(args).progressive_world_entry());
        }
        for args in [vec!["ferrum189", "--check"], vec!["ferrum189", "--screenshot", "test.png"],
            vec!["ferrum189", "--benchmark"]] {
            assert!(!Options::parse_from(args).progressive_world_entry());
        }
    }

    #[test]
    #[ignore = "uses installed resources and a disposable local game"]
    fn local_tick_ownership_replays_look_and_pause_without_overwriting_edits_or_saving_a_mirror() {
        let (mut app, dir) = ui_tests::isolated_app();
        let mut options = app.options.clone(); options.project_dir = dir.clone();
        let mut game = Game::load(options, &app.assets, &app.settings).unwrap();
        game.sim.mob_spawning = false;
        game.sim.mobs.clear();
        let edit = IVec3::new(0, 100, 0);
        game.world.set(edit, 89 << 4);
        game.sim.block_ticks.schedule_block_update(edit, 89, 100_000, 0);
        assert!(!serde_json::to_value(&game.sim.block_ticks).unwrap()["queue"]
            .as_object().unwrap().is_empty());
        let original_yaw = game.player.yaw;
        let mut expected_look = game.player.clone();
        expected_look.look(3.25, -2.5);
        expected_look.look(-1.25, 0.5);
        app.game = Some(game);
        app.open(Screen::Playing);
        app.submit_local_tick().unwrap();
        assert!(app.tick_busy());
        assert!(app.game.as_ref().unwrap().world.edits.is_empty());
        assert!(serde_json::to_value(&app.game.as_ref().unwrap().sim.block_ticks)
            .unwrap()["queue"].as_object().unwrap().is_empty(),
            "busy presentation never copies the owner's pending scheduler");
        assert!(app.save().is_err(), "a presentation mirror cannot be saved");
        app.look_local(3.25, -2.5);
        app.look_local(-1.25, 0.5);
        assert_ne!(app.game.as_ref().unwrap().player.yaw, original_yaw);
        app.open(Screen::Pause);
        assert_eq!(app.gui.screen, Screen::Playing);
        let completed = app.local_tick.as_mut().unwrap().drain().unwrap().unwrap();
        app.accept_local_tick(completed).unwrap();
        while let Some(event) = app.deferred_input.pop_front() {
            match event {
                DeferredInput::Look(x,y) => app.look_local(x,y),
                DeferredInput::Open(screen) => app.open(screen),
                DeferredInput::Window(..) => unreachable!(),
            }
        }
        let g = app.game.as_ref().unwrap();
        assert_eq!(g.player.java_rotation(), expected_look.java_rotation());
        assert!(!serde_json::to_value(&g.sim.block_ticks).unwrap()["queue"]
            .as_object().unwrap().is_empty(), "returning authority retains pending work");
        assert_eq!(g.world.get(edit), 89 << 4);
        assert_eq!(g.world.edits.get(&edit.to_array()), Some(&(89 << 4)));
        assert_eq!(app.gui.screen, Screen::Pause);
        assert_eq!(app.accumulator, 0.);
        app.local_tick_failure = Some("fixture failed tick".into());
        assert!(app.save().is_err(), "partial tick recovery is never persisted");
        assert!(app.submit_local_tick().is_err(), "partial ticks are never rerun");
        assert_eq!(app.game.as_ref().unwrap().world.edits.get(&edit.to_array()), Some(&(89 << 4)));
        let root = dir.canonicalize().unwrap();
        assert_eq!(root.parent(), Some(std::env::temp_dir().canonicalize().unwrap().as_path()));
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("ferrum-controls-"));
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}
