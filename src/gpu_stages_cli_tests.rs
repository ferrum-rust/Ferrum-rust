use clap::Parser;

#[test]
fn gpu_stages_requires_legacy_capture_and_keeps_default_backing_and_observer_conflicts() {
    let ordinary=crate::Options::try_parse_from(["ferrum"]).unwrap();
    assert!(!ordinary.benchmark_gpu_timestamps && !ordinary.benchmark_gpu_stages);
    for args in [vec!["ferrum","--benchmark-gpu-stages"],
        vec!["ferrum","--benchmark","--benchmark-streaming","--benchmark-gpu-stages"],
        vec!["ferrum","--benchmark","--benchmark-gpu-timestamps","--benchmark-gpu-stages"]] {
        assert!(crate::Options::try_parse_from(args).is_err());
    }
    let base=["ferrum","--benchmark","--benchmark-streaming","--benchmark-gpu-timestamps"];
    let legacy=crate::Options::try_parse_from(base).unwrap();
    assert!(legacy.benchmark_gpu_timestamps && !legacy.benchmark_gpu_stages);
    let staged=crate::Options::try_parse_from(base.into_iter().chain(["--benchmark-gpu-stages"])).unwrap();
    assert!(staged.benchmark_gpu_timestamps && staged.benchmark_gpu_stages);
    assert!(staged.benchmark_near_arena.is_none());
    assert!(staged.benchmark_transparency_sort.is_none());
    assert!(staged.benchmark_transparency_upload.is_none());
    assert!(staged.benchmark_transparency_draw.is_none());
    for extra in [vec!["--benchmark-surface-waits"],vec!["--benchmark-capture-uploads"],
        vec!["--benchmark-capture-arena-uploads"],vec!["--benchmark-capture-near-opaque"],
        vec!["--benchmark-capture-transparency"],vec!["--benchmark-near-opaque-runs"],
        vec!["--benchmark-transparency-upload","queue"],vec!["--benchmark-transparency-draw","direct"]] {
        assert!(crate::Options::try_parse_from(base.into_iter().chain(["--benchmark-gpu-stages"]).chain(extra)).is_err());
    }
}
