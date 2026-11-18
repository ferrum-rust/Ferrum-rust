use super::*;
use clap::Parser;

#[test]
fn automatic_policy_is_exactly_the_measured_distance_and_capability_gate() {
    // Independent acceptance table: low and unmeasured distances remain on
    // the ordinary cached renderer; the tested high setting alone opts in.
    for distance in [0, 2, 12, 16, 24, 127, 128, 129, u32::MAX] {
        for supported in [false, true] {
            let policy = Policy::new(None, distance, supported);
            let expected = if distance == 128 && supported { Some(Choice::Mdi) } else { None };
            assert!(policy.automatic());
            assert_eq!(policy.distance(), distance);
            assert_eq!(policy.desired(), expected, "distance={distance}, supported={supported}");
            assert!(!policy.reason().is_empty());
            for active in [false, true] {
                for empty in [false, true] {
                    // New activation can retain all existing dedicated meshes.
                    // An active owner's lowering is deferred until those meshes
                    // have been dropped; active valid owners are never replaced.
                    let want = match (expected, active, empty) {
                        (Some(choice), false, _) => Selection::Arena(choice),
                        (Some(_), true, _) | (None, true, false) => Selection::Hold,
                        (None, _, _) => Selection::Original,
                    };
                    assert_eq!(policy.select(active, empty), want);
                }
            }
        }
    }
}

#[test]
fn distance_changes_allow_live_dedicated_upgrade_but_never_forget_a_live_owner() {
    let mut policy = Policy::new(None, 12, true);
    assert_eq!(policy.select(false, false), Selection::Original);
    policy.update_distance(128);
    assert_eq!(policy.distance(), 128);
    assert_eq!(policy.select(false, false), Selection::Arena(Choice::Mdi));
    // Renderer publishes a checked State without replacing old dedicated data.
    assert_eq!(policy.select(true, false), Selection::Hold);
    policy.update_distance(12);
    assert_eq!(policy.desired(), None);
    assert_eq!(policy.select(true, false), Selection::Hold);
    // clear() must first discard cached bindings and chunk owners. Only then
    // does the policy permit releasing the State and its persistent pages.
    assert_eq!(policy.select(true, true), Selection::Original);
    assert_eq!(policy.select(false, true), Selection::Original);
    policy.update_distance(128);
    assert_eq!(policy.select(false, true), Selection::Arena(Choice::Mdi));
    // Repeated settings evaluation is idempotent once the State is installed.
    for _ in 0..8 { assert_eq!(policy.select(true, true), Selection::Hold); }
}

#[test]
fn explicit_original_and_experiments_ignore_automatic_eligibility_and_suppression() {
    for choice in [Choice::Original, Choice::Dedicated, Choice::Direct, Choice::Mdi] {
        let expected = if choice == Choice::Original { None } else { Some(choice) };
        for supported in [false, true] {
            let mut policy = Policy::new(Some(choice), 12, supported);
            assert!(!policy.automatic());
            policy.suppress_diagnostic("fixture diagnostic");
            policy.initialization_failed("fixture automatic initialization failure");
            for distance in [12, 128, 16] {
                policy.update_distance(distance);
                assert_eq!(policy.desired(), expected);
                assert_eq!(policy.select(false, false), expected.map_or(Selection::Original, Selection::Arena));
                assert_eq!(policy.select(true, false), Selection::Hold);
            }
        }
    }
}

#[test]
fn automatic_failures_and_diagnostics_fail_closed_before_new_shared_admission() {
    let mut failed = Policy::new(None, 128, true);
    failed.initialization_failed(&"x".repeat(8192));
    assert!(!failed.reason().is_empty());
    assert!(failed.reason().len() < 8192, "failure storage must be bounded");
    assert_eq!(failed.desired(), None);
    assert_eq!(failed.select(false, false), Selection::Original);
    failed.update_distance(12);
    failed.update_distance(128);
    assert_eq!(failed.select(false, true), Selection::Original, "do not retry failed initialization every frame");
    // Even defensive suppression after activation must not release live owners.
    assert_eq!(failed.select(true, false), Selection::Hold);
    assert_eq!(failed.select(true, true), Selection::Original);

    let mut diagnostic = Policy::new(None, 128, true);
    diagnostic.suppress_diagnostic("upload capture requires ordinary backing");
    assert_eq!(diagnostic.desired(), None);
    assert_eq!(diagnostic.select(false, false), Selection::Original);
    diagnostic.update_distance(12);
    diagnostic.update_distance(128);
    assert_eq!(diagnostic.select(false, true), Selection::Original);
    assert_eq!(diagnostic.select(true, false), Selection::Hold);
}

#[test]
fn original_cli_preserves_an_explicit_legacy_cached_control() {
    assert!(crate::Options::try_parse_from(["ferrum"]).unwrap().benchmark_near_arena.is_none());
    for args in [vec!["ferrum", "--benchmark-near-arena", "original"],
        vec!["ferrum", "--benchmark", "--benchmark-near-arena", "original"]] {
        assert!(crate::Options::try_parse_from(args).is_err());
    }
    let parsed = crate::Options::try_parse_from(["ferrum", "--benchmark", "--benchmark-streaming",
        "--benchmark-near-arena", "original"]).unwrap();
    assert_eq!(parsed.benchmark_near_arena, Some(Choice::Original));
    let automatic = crate::Options::try_parse_from(["ferrum", "--benchmark", "--benchmark-streaming"]).unwrap();
    assert!(automatic.benchmark_near_arena.is_none(), "absence now exercises the guarded default");
}
