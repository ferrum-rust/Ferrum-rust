use clap::Parser;

#[test]
fn packed_frustum_is_explicit_streaming_only_and_excludes_early_visibility() {
    let ordinary=crate::Options::try_parse_from(["ferrum"]).unwrap();
    assert!(!ordinary.benchmark_packed_frustum);
    assert!(!ordinary.benchmark_pre_acquire_visibility);
    for args in [vec!["ferrum","--benchmark-packed-frustum"],
        vec!["ferrum","--benchmark","--benchmark-packed-frustum"],
        vec!["ferrum","--benchmark","--benchmark-streaming","--benchmark-packed-frustum","--benchmark-pre-acquire-visibility"],
        vec!["ferrum","--benchmark","--benchmark-streaming","--benchmark-pre-acquire-visibility","--benchmark-packed-frustum"]] {
        assert!(crate::Options::try_parse_from(args).is_err());
    }
    let trial=crate::Options::try_parse_from(["ferrum","--benchmark","--benchmark-streaming","--benchmark-packed-frustum"]).unwrap();
    assert!(trial.benchmark_packed_frustum);
    assert!(!trial.benchmark_pre_acquire_visibility);
    assert!(trial.benchmark_near_arena.is_none());
    assert!(trial.benchmark_transparency_sort.is_none());
    assert!(trial.benchmark_transparency_upload.is_none());
    assert!(trial.benchmark_transparency_draw.is_none());
    assert!(!trial.benchmark_gpu_timestamps);
    assert!(!trial.benchmark_surface_waits);
}
