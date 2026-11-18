use clap::Parser;

#[test]
fn covered_lod_trial_is_explicit_streaming_only_and_preserves_other_defaults() {
    assert!(!crate::Options::try_parse_from(["ferrum"]).unwrap().benchmark_cull_covered_lod);
    for args in [vec!["ferrum", "--benchmark-cull-covered-lod"],
        vec!["ferrum", "--benchmark", "--benchmark-cull-covered-lod"]] {
        assert!(crate::Options::try_parse_from(args).is_err());
    }
    let base = ["ferrum", "--benchmark", "--benchmark-streaming", "--benchmark-cull-covered-lod"];
    let options = crate::Options::try_parse_from(base).unwrap();
    assert!(options.benchmark_cull_covered_lod);
    assert!(!options.benchmark_packed_frustum && !options.benchmark_pre_acquire_visibility);
    assert!(!options.benchmark_gpu_timestamps && !options.benchmark_gpu_stages);
    assert!(options.benchmark_near_arena.is_none() && options.benchmark_transparency_sort.is_none());
    assert!(options.benchmark_transparency_upload.is_none() && options.benchmark_transparency_draw.is_none());
    for flag in ["--benchmark-packed-frustum", "--benchmark-pre-acquire-visibility"] {
        assert!(crate::Options::try_parse_from(base.into_iter().chain([flag])).is_err());
        let reversed = ["ferrum", "--benchmark", "--benchmark-streaming", flag, "--benchmark-cull-covered-lod"];
        assert!(crate::Options::try_parse_from(reversed).is_err());
    }
    let observed = crate::Options::try_parse_from(base.into_iter()
        .chain(["--benchmark-gpu-timestamps", "--benchmark-gpu-stages"])).unwrap();
    assert!(observed.benchmark_cull_covered_lod && observed.benchmark_gpu_stages);
    assert!(observed.benchmark_near_arena.is_none());
}
