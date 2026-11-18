//! Actual arena upload capture on/off, with the same Renderer submit oracle.
use super::*;

fn upload_capture_renderer(choice: Option<Choice>, settings: &Settings, world: &World,
    player: &Player, sim: &Simulation, enabled: bool) -> Renderer {
    let mut renderer = setup(choice, settings, world, player, sim);
    assert!(renderer.near_arena.is_some(), "all fixture arms must actually use the arena-owner upload dispatch");
    renderer.arena_test_limits(Limits { vertex_page: 4*1024*1024, index_page: 64*1024, owners: 64 });
    let policy = renderer.near_arena_report()["policy"].clone();
    renderer.configure_arena_upload_capture(enabled, Some(std::time::Instant::now()));
    assert_eq!(renderer.near_arena_report()["policy"], policy, "capture must not suppress or change automatic/explicit selection");
    assert_eq!(renderer.arena_upload_capture_report()["enabled"], enabled);
    renderer
}

#[test]
#[ignore = "requires original resources and Vulkan MDI; captured actual arena uploads preserve queued pixels/depth/index bytes"]
fn arena_upload_capture_matches_uncaptured_automatic_mdi_and_dedicated_lifecycle() {
    let world = world(); let mut sim = Simulation::new(GameMode::Creative, 189); sim.clock = 6000;
    for choice in [None, Some(Choice::Mdi), Some(Choice::Dedicated)] {
        let mut expected: Option<Vec<Image>> = None;
        for enabled in [false, true] {
            let mut settings = settings(); settings.render_distance = 128; let mut player = player();
            let mut renderer = upload_capture_renderer(choice, &settings, &world, &player, &sim, enabled);
            let initial_policy = renderer.near_arena_report()["policy"].clone();
            let mut tickets = Vec::new();
            for phase in 0..8 {
                match phase {
                    0 => { for id in 0..8 { upload(&mut renderer, id, 0, id == 7); } }
                    1 => upload(&mut renderer, 2, 1, false),
                    2 => {
                        upload(&mut renderer, 0, 1, true); upload(&mut renderer, 6, 1, true);
                        if choice != Some(Choice::Dedicated) {
                            assert!(renderer.chunks[&(0, 0)].gpu.arena_token.is_some());
                            assert!(renderer.chunks[&(6, 0)].gpu.arena_token.is_none(), "third >2MiB owner really exhausts two4MiB pages");
                            assert_eq!(renderer.near_arena_report()["fallbacks"], 1);
                        }
                    }
                    3 => {
                        player.set_pose_f64([4., 65., 55.], 180., 0., true);
                        settings.anaglyph = true; renderer.apply_settings(&settings);
                        for chunk in renderer.chunks.values_mut() { chunk.present = (1<<3) | (1<<5); }
                    }
                    4 => { renderer.remove((1, 0)); upload(&mut renderer, 1, 2, false); }
                    5 => renderer.upload((3, 0), models::empty()),
                    6 => renderer.clear(),
                    7 => { for id in 0..6 { upload(&mut renderer, id, 0, false); } }
                    _ => unreachable!(),
                }
                tickets.push(capture(&mut renderer, &settings, &world, &player, &sim));
            }
            // Copies from submitted frames precede every subsequent overwrite,
            // fallback and clear. The fixture adds no host wait inside the loop.
            let images: Vec<_> = tickets.into_iter().map(|t| t.finish(&renderer.device)).collect();
            assert_eq!(renderer.near_arena_report()["policy"], initial_policy);
            assert!(images[0].geometry.iter().any(|g| g.width == 2) && images[0].geometry.iter().any(|g| g.width == 4));
            assert_ne!(images[0].color, images[1].color, "fitting payload update is visible");
            assert!(images[5].color.iter().zip(&images[6].color).filter(|(a,b)| a != b).count() > 1000);
            if choice != Some(Choice::Dedicated) {
                assert!(renderer.near_arena_report()["multi_calls_per_eye_not_scaled"].as_u64().unwrap() > 0);
            }
            let report = renderer.arena_upload_capture_report();
            if enabled {
                assert_eq!(report["observations"], 19, "clear preserves this configured capture window");
                assert_eq!(report["invalid_observations"], 0);
                assert_eq!(report["total"]["calls"], 19);
                assert_eq!(report["phase_names"].as_array().unwrap().len(), 18);
                assert_eq!(report["slowest_limit"], 8);
                let slowest = report["slowest"].as_array().unwrap();
                assert_eq!(slowest.len(), 8); assert!(slowest.iter().all(|o| !o.is_null()));
                assert!(slowest.windows(2).all(|v| v[0]["total_ms"].as_f64().unwrap() >= v[1]["total_ms"].as_f64().unwrap()));
                assert!(report["configuration_application_elapsed_ms"].as_f64().unwrap() >= 0.);
                if choice == Some(Choice::Dedicated) {
                    assert_eq!(report["explicit_dedicated_controls"], 19);
                    assert_eq!(report["shared_uploads"], 0); assert_eq!(report["fallbacks"], 0);
                } else {
                    assert_eq!(report["explicit_dedicated_controls"], 0);
                    assert_eq!(report["shared_uploads"], 18); assert_eq!(report["fallbacks"], 1);
                }
            }
            if let Some(expected) = &expected {
                for (phase, (actual, original)) in images.iter().zip(expected).enumerate() {
                    assert_eq!(actual, original, "capture changed {choice:?} actual GPU output at phase{phase}");
                }
            } else { expected = Some(images); }
        }
    }
}

#[test]
#[ignore = "requires Vulkan MDI; successful-only capture must preserve checked failure and post-target poison semantics"]
fn arena_upload_capture_preserves_pre_target_failures_fallback_and_post_target_poison() {
    let world = world(); let player = player(); let mut settings = settings(); settings.render_distance = 128;
    let mut sim = Simulation::new(GameMode::Creative, 189); sim.clock = 6000;
    for choice in [None, Some(Choice::Mdi)] {
        let mut expected: Option<Vec<Image>> = None;
        for enabled in [false, true] {
            let mut renderer = upload_capture_renderer(choice, &settings, &world, &player, &sim, enabled);
            upload(&mut renderer, 0, 0, false);
            let old = renderer.chunks[&(0, 0)].gpu.arena_token.unwrap();
            let before = capture(&mut renderer, &settings, &world, &player, &sim);
            renderer.arena_test_faults(true, true, false);
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| renderer.upload((0, 0), forced_wide_panel()))).is_err());
            assert_eq!(renderer.arena_test_health(), (true, true));
            assert_eq!(renderer.chunks[&(0, 0)].gpu.arena_token, Some(old));
            if enabled { assert_eq!(renderer.arena_upload_capture_report()["observations"], 1, "panic exits are not reported as completed uploads"); }
            renderer.arena_test_clear_mutex_poison();
            let unchanged = capture(&mut renderer, &settings, &world, &player, &sim);
            renderer.arena_test_faults(true, false, false);
            renderer.upload((0, 0), forced_wide_panel()); renderer.chunks.get_mut(&(0, 0)).unwrap().graph = None;
            assert!(renderer.chunks[&(0, 0)].gpu.arena_token.is_none());
            let fallback = capture(&mut renderer, &settings, &world, &player, &sim);
            let images: Vec<_> = [before, unchanged, fallback].into_iter().map(|t| t.finish(&renderer.device)).collect();
            assert_eq!(images[0], images[1]);
            assert_eq!(images[0].color, images[2].color); assert_eq!(images[0].depth, images[2].depth);
            assert_eq!(logical_indices(&images[0]), logical_indices(&images[2]));
            upload(&mut renderer, 0, 0, false);
            assert!(renderer.chunks[&(0, 0)].gpu.arena_token.is_some());
            renderer.arena_test_faults(false, false, true);
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| upload(&mut renderer, 0, 1, false))).is_err());
            assert_eq!(renderer.arena_test_health(), (true, false));
            if enabled {
                let report = renderer.arena_upload_capture_report();
                assert_eq!(report["observations"], 3); assert_eq!(report["invalid_observations"], 0);
                assert_eq!(report["shared_uploads"], 2); assert_eq!(report["fallbacks"], 1);
            }
            if let Some(expected) = &expected { assert_eq!(&images, expected, "capturing failures changed {choice:?} GPU output"); }
            else { expected = Some(images); }
        }
    }
}
