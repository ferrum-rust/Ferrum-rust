//! CLI/build guards only; public-span collector tests are separate.
use clap::Parser;

#[test]
fn surface_wait_capture_is_explicit_streaming_only_and_keeps_automatic_backing() {
    let ordinary = crate::Options::try_parse_from(["ferrum"]).unwrap();
    assert!(!ordinary.benchmark_surface_waits);
    ordinary.validate_surface_wait_build().unwrap();
    for args in [vec!["ferrum", "--benchmark-surface-waits"],
        vec!["ferrum", "--benchmark", "--benchmark-surface-waits"]] {
        assert!(crate::Options::try_parse_from(args).is_err());
    }
    let base = ["ferrum", "--benchmark", "--benchmark-streaming", "--benchmark-surface-waits"];
    let requested = crate::Options::try_parse_from(base).unwrap();
    assert!(requested.benchmark_surface_waits);
    assert!(requested.benchmark_near_arena.is_none());
    assert!(requested.benchmark_transparency_sort.is_none());
    assert!(requested.benchmark_transparency_upload.is_none());
    assert!(requested.benchmark_transparency_draw.is_none());
    // The flag never forces or suppresses automatic opaque MDI. As with the
    // GPU timing diagnostic, an explicit backing control remains permitted.
    for mode in ["original", "mdi"] {
        let mut args = base.to_vec(); args.extend(["--benchmark-near-arena", mode]);
        assert!(crate::Options::try_parse_from(args).is_ok());
    }
    let mut args = base.to_vec(); args.extend(["--benchmark-slow-frame-ms", "4"]);
    assert!(crate::Options::try_parse_from(args).is_ok());
    if cfg!(feature = "surface-wait-trace") {
        requested.validate_surface_wait_build().unwrap();
    } else {
        let error = requested.validate_surface_wait_build().unwrap_err().to_string();
        assert!(error.contains("--features surface-wait-trace"));
        assert!(error.contains("ordinary installed build"));
    }
}

#[test]
fn surface_wait_capture_rejects_other_observers_and_explicit_transfer_draw_trials() {
    let base = ["ferrum", "--benchmark", "--benchmark-streaming", "--benchmark-surface-waits"];
    for extra in [vec!["--benchmark-gpu-timestamps"], vec!["--benchmark-capture-uploads"],
        vec!["--benchmark-capture-arena-uploads"], vec!["--benchmark-capture-near-opaque"],
        vec!["--benchmark-near-opaque-runs"], vec!["--benchmark-capture-transparency"],
        vec!["--benchmark-transparency-upload", "queue"], vec!["--benchmark-transparency-upload", "staged"],
        vec!["--benchmark-transparency-draw", "direct"], vec!["--benchmark-transparency-draw", "mdi"]] {
        let mut args = base.to_vec(); args.extend(extra.clone());
        assert!(crate::Options::try_parse_from(args).is_err(), "must reject {extra:?}");
        // Clap conflicts must not depend on argument order.
        let mut reverse = vec!["ferrum", "--benchmark", "--benchmark-streaming"];
        reverse.extend(extra); reverse.push("--benchmark-surface-waits");
        assert!(crate::Options::try_parse_from(reverse).is_err());
    }
}
