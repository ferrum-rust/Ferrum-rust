//! Explicit CPU-overlap trial. No surface, Queue, encoder or native calls here.
use super::*;

pub(super) type WorldView = (models::Viewpoint, Mat4, Mat4, Frustum);
pub(super) struct PreparedView {
    pub(super) hurt: HurtView,
    pub(super) view: WorldView,
}

#[derive(Default, serde::Serialize)]
struct Counts {
    world_prepared: u64,
    world_acquired: u64,
    world_aborted: u64,
    empty_attempts: u64,
    empty_acquired: u64,
    empty_aborted: u64,
}

/// The disabled ordinary renderer owns no trial allocation and reads no clock.
#[derive(Default)]
pub(super) struct Trial(Option<Box<Counts>>);
impl Trial {
    fn enabled(&self) -> bool { self.0.is_some() }
    fn prepared(&mut self, world: bool) {
        if let Some(counts) = self.0.as_mut() {
            if world { counts.world_prepared += 1; } else { counts.empty_attempts += 1; }
        }
    }
    fn acquired(&mut self, world: bool, success: bool) {
        if let Some(counts) = self.0.as_mut() {
            match (world, success) {
                (true, true) => counts.world_acquired += 1,
                (true, false) => counts.world_aborted += 1,
                (false, true) => counts.empty_acquired += 1,
                (false, false) => counts.empty_aborted += 1,
            }
        }
    }
}

/// Cancel only derived traversal cache validity. The next consumer MUST call
/// View::update before querying masks/order, as Renderer::boss/encode already do.
/// This does not claim byte-for-byte equality of inactive scratch/cache fields.
pub(crate) fn cancel_view(view: &mut crate::mesh::visibility::View) {
    view.invalidate();
}

impl Renderer {
    pub(crate) fn configure_pre_acquire_visibility(&mut self, enabled: bool) {
        self.pre_acquire_visibility = Trial(enabled.then(|| Box::new(Counts::default())));
    }
    pub(crate) fn pre_acquire_visibility_report(&self) -> serde_json::Value {
        let Some(counts) = self.pre_acquire_visibility.0.as_deref() else {
            return serde_json::json!({"enabled":false,"mode":"after-acquire"});
        };
        serde_json::json!({
            "enabled":true,"mode":"before-acquire-visibility","performance_trial":true,
            "counts":counts,
            "scope":"Counts cover render attempts including warmup and failed acquisition; screenshot/raw encode do not use this trial. Only the existing world camera/frustum and section traversal are prepared before acquisition. Models, sort history, Queue writes, GPU tickets, arena/staging sessions and passes remain after success.",
            "timing":"Successful measured render rows report pre_acquire_ms inclusive of camera calculation and traversal; stages_ms.pre_acquire_section_visibility is nested there, not in encode_ms. Post-acquire stages_ms.section_visibility is zero when the carrier is reused. Compare total render/present and pre_acquire_ms+encode_ms, never a shorter encode label alone.",
            "failure":"Original acquisition errors are forwarded; prepared visibility is invalidated, and the next boss/encode consumer updates before querying. No speculative Order or GPU state exists to roll back.",
        })
    }
    pub(super) fn prepare_pre_acquire_visibility(&mut self, input: &RenderInput<'_>,
        profile: &mut render_profile::Frame) -> Option<PreparedView> {
        if !self.pre_acquire_visibility.enabled() { return None; }
        let (Some(world), Some(player), Some(sim)) = (input.world, input.player, input.simulation) else {
            self.pre_acquire_visibility.prepared(false);
            return None;
        };
        let started = profile.timer();
        let hurt = HurtView::for_camera(sim, input.alpha);
        let view = self.world_view(world, player, input.settings, input.gui.perspective,
            input.alpha, hurt, sim.equipped.bow_fov_multiplier(input.alpha), sim.effects.get(9).is_some());
        let section_started = profile.timer();
        let (viewpoint, _, matrix, frustum) = &view;
        let radius = input.settings.full_detail_distance() as u8;
        let columns = graph_lookup::Columns::new(viewpoint.eye, radius);
        self.section_view.update(*matrix, frustum, viewpoint.eye, viewpoint.direction, radius,
            |pos| columns.get(pos, || self.chunks.get(&pos)?.graph.as_deref()));
        profile.stages_ms.pre_acquire_section_visibility += render_profile::elapsed(section_started);
        profile.pre_acquire_ms += render_profile::elapsed(started);
        self.pre_acquire_visibility.prepared(true);
        Some(PreparedView { hurt, view })
    }
    pub(super) fn finish_pre_acquire_visibility<T>(&mut self, prepared: bool,
        result: std::result::Result<T, wgpu::SurfaceError>) -> std::result::Result<T, wgpu::SurfaceError> {
        if self.pre_acquire_visibility.enabled() {
            self.pre_acquire_visibility.acquired(prepared, result.is_ok());
            if prepared && result.is_err() { cancel_view(&mut self.section_view); }
        }
        result
    }
}
