//! Optional public tracing scopes only; no native handles or synchronization changes.
#[cfg(feature="surface-wait-trace")]
#[path="surface_wait_trace_enabled.rs"]
mod enabled;
#[cfg(feature="surface-wait-trace")]
pub(crate) use enabled::Capture;

pub(crate) fn disabled_report()->serde_json::Value {
    serde_json::json!({"enabled":false,"feature_enabled":cfg!(feature="surface-wait-trace"),
        "diagnostic_not_baseline":false})
}

#[cfg(not(feature="surface-wait-trace"))]
pub(crate) struct Capture;
#[cfg(not(feature="surface-wait-trace"))]
impl Capture {
    pub(crate) fn new()->anyhow::Result<Self> {
        anyhow::bail!("surface wait capture requires the surface-wait-trace build feature")
    }
    pub(crate) fn clear_pending_present(&mut self) {}
    pub(crate) fn acquire<T,E>(&mut self,_frame_id:u64,_application_seconds:f64,
        acquire:impl FnOnce()->Result<T,E>)->Result<T,E> { acquire() }
    pub(crate) fn warmup_acquire<T,E>(&mut self,_frame_id:u64,_application_seconds:f64,
        acquire:impl FnOnce()->Result<T,E>)->Result<T,E> { acquire() }
    pub(crate) fn public_elapsed(&mut self,_milliseconds:f64)->anyhow::Result<()> {Ok(())}
    pub(crate) fn presented(&mut self,_sample_index:Option<usize>,_begin_application_ms:Option<f64>,
        _end_application_ms:f64)->anyhow::Result<()> {Ok(())}
    pub(crate) fn report(&self)->serde_json::Value {disabled_report()}
}

#[cfg(all(test,not(feature="surface-wait-trace")))]
mod disabled_tests {
    #[test]
    fn feature_off_capture_is_zero_sized_and_rejected_before_use() {
        assert_eq!(std::mem::size_of::<super::Capture>(),0);
        assert!(super::Capture::new().is_err());
        assert_eq!(super::disabled_report()["feature_enabled"],false);
    }
}
