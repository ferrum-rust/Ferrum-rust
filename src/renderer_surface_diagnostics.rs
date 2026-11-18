//! Metadata only. Snapshot the existing capabilities query; no GPU queries here.
const MODES:[wgpu::PresentMode;6]=[wgpu::PresentMode::AutoVsync,wgpu::PresentMode::AutoNoVsync,
    wgpu::PresentMode::Fifo,wgpu::PresentMode::FifoRelaxed,wgpu::PresentMode::Immediate,wgpu::PresentMode::Mailbox];
#[derive(Clone,Copy)]
pub(super) struct Support { bits:u8 }
impl Support {
    pub(super) fn new(modes:&[wgpu::PresentMode])->Self {
        Self{bits:modes.iter().fold(0,|bits,mode|bits | (1u8 << (*mode as u8)))}
    }
    fn contains(self,mode:wgpu::PresentMode)->bool {self.bits & (1u8 << (mode as u8)) != 0}
    fn derived(self,requested:wgpu::PresentMode)->Option<wgpu::PresentMode> {
        use wgpu::PresentMode::*;
        if self.contains(requested){return Some(requested);}
        let fallback:&[wgpu::PresentMode]=match requested {
            AutoVsync=>&[FifoRelaxed,Fifo],AutoNoVsync=>&[Immediate,Mailbox,Fifo],_=>&[],
        };
        fallback.iter().copied().find(|&mode|self.contains(mode))
    }
}
pub(super) fn report(config:&wgpu::SurfaceConfiguration,support:Option<Support>)->serde_json::Value {
    let available=support.map(|support|MODES.into_iter().filter(|&mode|support.contains(mode))
        .map(|mode|format!("{mode:?}")).collect::<Vec<_>>());
    let derived=support.and_then(|support|support.derived(config.present_mode)).map(|mode|format!("{mode:?}"));
    serde_json::json!({"has_surface":support.is_some(),"requested_present_mode":format!("{:?}",config.present_mode),
        "requested_maximum_frame_latency":config.desired_maximum_frame_latency,
        "current_extent_pixels":[config.width,config.height],"requested_format":format!("{:?}",config.format),
        "requested_alpha_mode":format!("{:?}",config.alpha_mode),
        "available_mode_set_at_constructor":available,"derived_mode_from_initial_capabilities":derived,
        "observed_native_present_mode":null,"effective_maximum_frame_latency":null,"actual_swapchain_image_count":null,
        "derivation":"Pinned wgpu-core24.0.5: supported explicit request unchanged; AutoNoVsync Immediate->Mailbox->Fifo; AutoVsync FifoRelaxed->Fifo.",
        "note":"Metadata only: no extra GPU query/configure or presentation change. Capabilities are a fixed snapshot from the existing constructor query; derived mode uses current requested config and that initial set, NOT an observed active native swapchain mode. Resize/settings update requested config; monitor/surface capability changes may make the original snapshot stale. Resource-pack reload constructs a new snapshot. Headless has_surface=false, available/derived=null. Latency is a clamped backend hint, not measured queue depth or image count. Vulkan24 acquire includes a prior-submission fence wait before vkAcquireNextImageKHR, plus other wrapper work; surface_acquire wall time cannot distinguish those causes. Existing present_mode remains the requested policy."})
}
#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::PresentMode::*;
    #[test]
    fn pinned_present_choice_uses_fixed_preference_not_capability_order() {
        let cases:&[(&[wgpu::PresentMode],Option<wgpu::PresentMode>,Option<wgpu::PresentMode>)]=&[
            (&[],None,None),(&[Fifo],Some(Fifo),Some(Fifo)),
            (&[Mailbox],Some(Mailbox),None),(&[Immediate],Some(Immediate),None),
            (&[Fifo,Mailbox],Some(Mailbox),Some(Fifo)),
            (&[Fifo,Immediate],Some(Immediate),Some(Fifo)),
            (&[Mailbox,Immediate],Some(Immediate),None),
            (&[Fifo,Mailbox,Immediate],Some(Immediate),Some(Fifo)),
            (&[FifoRelaxed],None,Some(FifoRelaxed)),
            (&[Fifo,FifoRelaxed,Mailbox],Some(Mailbox),Some(FifoRelaxed)),
        ];
        for &(available,no_vsync,vsync) in cases {
            for reverse in [false,true] {
                let mut modes=available.to_vec();if reverse{modes.reverse();}
                let support=Support::new(&modes);
                assert_eq!(support.derived(AutoNoVsync),no_vsync);
                assert_eq!(support.derived(AutoVsync),vsync);
                for explicit in [Fifo,FifoRelaxed,Immediate,Mailbox] {
                    assert_eq!(support.derived(explicit),available.contains(&explicit).then_some(explicit));
                }
            }
        }
        // Preserve core's already-supported early return, including synthetic
        // automatic entries that real HAL capabilities never advertise.
        assert_eq!(Support::new(&[AutoNoVsync,Fifo]).derived(AutoNoVsync),Some(AutoNoVsync));
    }
    #[test]
    fn report_keeps_requested_current_config_separate_from_initial_and_unknown_native_state() {
        let mut config=wgpu::SurfaceConfiguration{usage:wgpu::TextureUsages::RENDER_ATTACHMENT,
            format:wgpu::TextureFormat::Rgba8Unorm,width:640,height:360,present_mode:AutoNoVsync,
            desired_maximum_frame_latency:2,alpha_mode:wgpu::CompositeAlphaMode::Auto,view_formats:vec![]};
        let initial=Support::new(&[Fifo,Immediate]);
        let first=report(&config,Some(initial));assert_eq!(first["derived_mode_from_initial_capabilities"],"Immediate");
        config.present_mode=AutoVsync;config.width=1920;config.height=1001;
        let changed=report(&config,Some(initial));assert_eq!(changed["derived_mode_from_initial_capabilities"],"Fifo");
        assert_eq!(changed["requested_present_mode"],"AutoVsync");assert_eq!(changed["current_extent_pixels"],serde_json::json!([1920,1001]));
        assert_eq!(changed["available_mode_set_at_constructor"],first["available_mode_set_at_constructor"]);
        assert_eq!(changed["requested_maximum_frame_latency"],2);
        for field in ["observed_native_present_mode","effective_maximum_frame_latency","actual_swapchain_image_count"] {assert!(changed[field].is_null());}
        let headless=report(&config,None);assert_eq!(headless["has_surface"],false);
        assert!(headless["available_mode_set_at_constructor"].is_null());assert!(headless["derived_mode_from_initial_capabilities"].is_null());
    }
}
