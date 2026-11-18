use clap::Parser;

#[test]
fn pre_acquire_trial_is_explicit_streaming_only_without_a_backing_override() {
    assert!(!crate::Options::try_parse_from(["ferrum"]).unwrap().benchmark_pre_acquire_visibility);
    for args in [vec!["ferrum","--benchmark-pre-acquire-visibility"],
        vec!["ferrum","--benchmark","--benchmark-pre-acquire-visibility"]] {
        assert!(crate::Options::try_parse_from(args).is_err());
    }
    let args=crate::Options::try_parse_from(["ferrum","--benchmark","--benchmark-streaming",
        "--benchmark-pre-acquire-visibility"]).unwrap();
    assert!(args.benchmark_pre_acquire_visibility);
    assert!(args.benchmark_near_arena.is_none());assert!(!args.benchmark_gpu_timestamps);
    assert!(!args.benchmark_surface_waits);assert!(!args.benchmark_near_opaque_runs);
    assert!(args.benchmark_transparency_upload.is_none());assert!(args.benchmark_transparency_draw.is_none());
    args.validate_surface_wait_build().unwrap();
}
