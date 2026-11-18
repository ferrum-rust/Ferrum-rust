//! Benchmark-only model-stage clocks and counts. No ordinary-frame clock reads.
use serde::Serialize;
use std::time::Instant;
#[derive(Clone, Default, Serialize)]
pub(crate) struct Profile {
    #[serde(skip)]
    pub enabled: bool,
    pub block_entities_ms: f64,
    pub mobs_ms: f64,
    pub drops_orbs_ms: f64,
    pub other_entities_ms: f64,
    pub server_ms: f64,
    pub player_ms: f64,
    pub particles_ms: f64,
    pub weather_ms: f64,
    pub world_columns: usize,
    pub mobs: usize,
    pub native_mobs: usize,
    pub native_early_distance: usize,
    pub basic_early_distance: usize,
    pub drops: usize,
    pub orbs: usize,
    pub particles: usize,
    pub arrows: usize,
    pub mob_shots: usize,
    pub falling: usize,
    pub pickup_effects: usize,
}
impl Profile {
    #[inline]
    pub fn timer(&self) -> Option<Instant> { self.enabled.then(Instant::now) }
}
#[inline]
pub(super) fn elapsed(start: Option<Instant>) -> f64 {
    start.map_or(0., |at| at.elapsed().as_secs_f64() * 1000.)
}
