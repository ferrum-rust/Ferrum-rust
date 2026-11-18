//! Automatic-policy GPU regressions; child of renderer_arena_gpu_tests.
use super::*;

// Policy regressions extend the actual Renderer oracle above. Original is the
// preserved independent cached reference even when ordinary None becomes auto.
fn policy_renderer(choice: Option<Choice>, settings: &Settings, world: &World, player: &Player, sim: &Simulation) -> Renderer {
    policy_renderer_diagnostics(choice, settings, world, player, sim, false)
}
fn policy_renderer_diagnostics(choice: Option<Choice>, settings: &Settings, world: &World, player: &Player, sim: &Simulation, suppressed: bool) -> Renderer {
    let mut renderer = pollster::block_on(Renderer::new_with_arena_diagnostics(None, crate::validation::assets(),
        crate::validation::resources(), settings, None, choice, suppressed)).unwrap();
    renderer.resize(640, 360);
    renderer.torch_flicker = Default::default();
    renderer.visual_random = crate::vanilla::random::JavaRandom::new(189);
    for _ in 0..40 { renderer.tick_lighting(world, player, sim, settings.render_distance); }
    renderer
}
fn small_empty_arena(renderer: &mut Renderer) {
    // Dedicated chunks may remain after a live upgrade; no shared allocation
    // exists yet. This only replaces empty test storage, never old GPU meshes.
    if renderer.near_arena.is_some() {
        renderer.arena_test_limits(Limits { vertex_page: 4*1024*1024, index_page: 64*1024, owners: 64 });
    }
}
fn policy_screenshot(renderer: &mut Renderer, settings: &Settings, world: &World, player: &Player, sim: &Simulation) -> Vec<u8> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    struct OwnedFile(std::path::PathBuf);
    impl Drop for OwnedFile { fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); } }
    let owned = loop {
        let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("ferrum-arena-policy-{}-{sequence}.png", std::process::id()));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => { drop(file); break OwnedFile(path); }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("cannot create private screenshot fixture: {error}"),
        }
    };
    let gui = Gui { screen: Screen::Playing, hide_hud: true, ..Default::default() };
    let ui = Frame::default();
    let input = RenderInput { selected_block: None, assets: crate::validation::assets(), resources: crate::validation::resources(),
        settings, gui: &gui, ui: &ui, world: Some(world), player: Some(player), simulation: Some(sim), alpha: 1., time: 0. };
    // Exercise production screenshot's separate Session/submit/acknowledgement
    // path, not the fixture submit helper. PNG decoder gives canonical RGBA.
    renderer.screenshot(&owned.0, &input).unwrap();
    let image = image::open(&owned.0).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (640, 360)); image.into_raw()
}

#[test]
#[ignore = "requires original resources and Vulkan MDI; automatic128 policy versus preserved Original through live transitions"]
fn automatic_arena_policy_preserves_original_pixels_through_upgrade_latch_failure_clear_and_screenshot() {
    let world = world(); let player = player();
    let mut sim = Simulation::new(GameMode::Creative, 189); sim.clock = 6000;
    let mut expected: Option<(Vec<Image>, Vec<Vec<u8>>)> = None;
    for choice in [Some(Choice::Original), None] {
        let automatic = choice.is_none();
        let mut settings = settings(); settings.render_distance = 12;
        let mut renderer = policy_renderer(choice, &settings, &world, &player, &sim);
        assert!(renderer.near_arena.is_none(), "current lower-distance setting remains on original cached path");
        for id in 0..4 { upload(&mut renderer, id, 0, false); }
        drop(capture(&mut renderer, &settings, &world, &player, &sim));
        drop(capture(&mut renderer, &settings, &world, &player, &sim));
        let mut tickets = Vec::new(); let mut images = Vec::new();
        let mut screenshots = Vec::new(); let mut mixed_token = None;
        for phase in 0..13 {
            match phase {
                1 => {
                    // Upgrade with valid old geometry resident. The newly
                    // published arena owns nothing until future mesh uploads.
                    settings.render_distance = 128; renderer.apply_settings(&settings);
                    assert_eq!(renderer.near_arena.is_some(), automatic);
                    assert!(renderer.chunks.values().all(|c| c.gpu.arena_token.is_none()
                        && c.gpu.vertex_offset == 0 && c.gpu.index_offset == 0));
                    small_empty_arena(&mut renderer);
                }
                2 => {
                    upload(&mut renderer, 0, 1, false); upload(&mut renderer, 1, 1, false);
                    upload(&mut renderer, 2, 1, true);
                    settings.anaglyph = true; renderer.apply_settings(&settings);
                    if automatic {
                        assert!(renderer.chunks[&(0, 0)].gpu.arena_token.is_some());
                        assert!(renderer.chunks[&(3, 0)].gpu.arena_token.is_none());
                        mixed_token = renderer.chunks[&(0, 0)].gpu.arena_token;
                    }
                }
                3 => {
                    settings.render_distance = 12; renderer.apply_settings(&settings);
                    assert_eq!(renderer.near_arena.is_some(), automatic, "live shared owners stay latched until clear");
                    if automatic { assert_eq!(renderer.chunks[&(0, 0)].gpu.arena_token, mixed_token); }
                }
                4 => {
                    renderer.remove((1, 0)); upload(&mut renderer, 1, 2, false);
                    screenshots.push(policy_screenshot(&mut renderer, &settings, &world, &player, &sim));
                }
                5 => {
                    if automatic { assert!(renderer.near_arena_report()["multi_calls_per_eye_not_scaled"].as_u64().unwrap() > 0); }
                    renderer.clear();
                    assert!(renderer.near_arena.is_none(), "clear at lower requested distance releases the latch");
                }
                6 => { for id in 0..4 { upload(&mut renderer, id, 1, false); } }
                7 => {
                    if automatic { renderer.arena_test_fail_automatic_initialization_once(); }
                    settings.render_distance = 128; renderer.apply_settings(&settings);
                    assert!(renderer.near_arena.is_none(), "checked initialization failure preserves the existing dedicated path");
                    assert_eq!(renderer.near_arena_report()["policy"]["initialization_failed"], automatic);
                    assert!(renderer.chunks.values().all(|c| c.gpu.arena_token.is_none()
                        && c.gpu.vertex_offset == 0 && c.gpu.index_offset == 0));
                }
                8 => {
                    // Failed automatic initialization is sticky: changing
                    // settings must not repeatedly allocate or lose old data.
                    settings.render_distance = 12; renderer.apply_settings(&settings);
                    settings.render_distance = 128; renderer.apply_settings(&settings);
                    assert!(renderer.near_arena.is_none());
                    assert_eq!(renderer.near_arena_report()["policy"]["initialization_failed"], automatic);
                }
                9 => {
                    renderer.clear();
                    assert!(renderer.near_arena.is_none(), "clear cannot retry a failed automatic initialization");
                    for id in 0..4 { upload(&mut renderer, id, 1, false); }
                }
                10 => {
                    // Complete readbacks against their original Device before
                    // replacing it. Recreating resources permits a fresh policy
                    // decision, unlike settings/clear on a failed Renderer.
                    images.extend(tickets.drain(..).map(|ticket: Ticket| ticket.finish(&renderer.device)));
                    renderer = policy_renderer(choice, &settings, &world, &player, &sim);
                    assert_eq!(renderer.near_arena.is_some(), automatic);
                    assert_eq!(renderer.near_arena_report()["policy"]["initialization_failed"], false);
                    small_empty_arena(&mut renderer);
                    renderer.clear();
                    assert_eq!(renderer.near_arena.is_some(), automatic, "healthy clear reselects requested128");
                    small_empty_arena(&mut renderer);
                }
                11 => { for id in 0..4 { upload(&mut renderer, id, 0, false); } }
                12 => { settings.anaglyph = false; settings.mipmaps = 0; renderer.apply_settings(&settings);
                    screenshots.push(policy_screenshot(&mut renderer, &settings, &world, &player, &sim)); }
                _ => {}
            }
            tickets.push(capture(&mut renderer, &settings, &world, &player, &sim));
            if phase == 0 { assert!(renderer.terrain_draws_cached(), "both lower-distance arms really retain ordinary cache behavior"); }
            if automatic && phase == 7 { assert_eq!(renderer.visible_chunks, 4, "failed admission must not discard old visible geometry"); }
        }
        images.extend(tickets.into_iter().map(|t| t.finish(&renderer.device)));
        assert!(images[0].color.iter().zip(&images[5].color).filter(|(a,b)| a != b).count() > 1000);
        if automatic {
            let report = renderer.near_arena_report(); assert_eq!(report["effective"], "ArenaMdi");
            assert!(report["multi_calls_per_eye_not_scaled"].as_u64().unwrap() > 0);
            assert!(renderer.arena_test_health().1, "screenshot acknowledges its real submitted Session");
        }
        if let Some((expected_images, expected_pngs)) = &expected {
            assert_eq!(images.len(), expected_images.len());
            for (phase, (actual, original)) in images.iter().zip(expected_images).enumerate() {
                assert_eq!(actual, original, "automatic policy changed original GPU output at phase{phase}");
            }
            assert_eq!(&screenshots, expected_pngs, "real screenshot path must preserve exact decoded RGBA");
        } else { expected = Some((images, screenshots)); }
    }
}

#[test]
#[ignore = "requires Vulkan MDI; exact-distance automatic admission and empty-state diagnostic suppression"]
fn automatic_arena_policy_keeps_unmeasured_distances_and_diagnostic_controls_original() {
    let world = world(); let player = player(); let mut sim = Simulation::new(GameMode::Creative, 189); sim.clock = 6000;
    let mut settings = settings(); settings.render_distance = 12;
    let mut renderer = policy_renderer(None, &settings, &world, &player, &sim);
    for distance in [8, 12, 16, 127] {
        settings.render_distance = distance; renderer.apply_settings(&settings);
        assert!(renderer.near_arena.is_none(), "unmeasured distance{distance} must not silently activate shared storage");
    }
    settings.render_distance = 128; renderer.apply_settings(&settings);
    assert!(renderer.near_arena.is_some()); small_empty_arena(&mut renderer);
    // This is the actual App constructor seam: known diagnostics suppress
    // optional features and State before creation, not after a discarded page.
    let mut suppressed = policy_renderer_diagnostics(None, &settings, &world, &player, &sim, true);
    assert!(suppressed.near_arena.is_none());
    assert!(!suppressed.device.features().contains(wgpu::Features::MULTI_DRAW_INDIRECT),
        "known diagnostics must suppress optional MDI negotiation before Device creation");
    assert_eq!(suppressed.near_arena_report()["policy"]["diagnostic_suppressed"], true);
    assert_eq!(suppressed.near_arena_report()["policy"]["initialization_failed"], false);
    suppressed.apply_settings(&settings); suppressed.clear();
    assert!(suppressed.near_arena.is_none());
    drop(suppressed);
    drop(renderer);
    for diagnostic in 0..4 {
        let mut renderer = policy_renderer(None, &settings, &world, &player, &sim);
        assert!(renderer.near_arena.is_some()); small_empty_arena(&mut renderer);
        match diagnostic {
            0 => renderer.configure_upload_capture(true, None),
            1 => renderer.configure_near_opaque_shadow(true),
            2 => renderer.configure_near_opaque_runs(true),
            3 => renderer.configure_transparency_capture(true),
            _ => unreachable!(),
        }
        assert!(renderer.near_arena.is_none(), "diagnostic explicitly retains its original backing reference");
        assert_eq!(renderer.near_arena_report()["policy"]["diagnostic_suppressed"], true);
        upload(&mut renderer, 0, 0, false);
        assert!(renderer.chunks[&(0, 0)].gpu.arena_token.is_none());
        let ticket = capture(&mut renderer, &settings, &world, &player, &sim);
        assert_eq!(renderer.visible_chunks, 1); drop(ticket.finish(&renderer.device));
        renderer.clear(); assert!(renderer.near_arena.is_none(), "active diagnostic suppresses automatic reselect");
        match diagnostic {
            0 => renderer.configure_upload_capture(false, None),
            1 => renderer.configure_near_opaque_shadow(false),
            2 => renderer.configure_near_opaque_runs(false),
            3 => renderer.configure_transparency_capture(false),
            _ => unreachable!(),
        }
        renderer.apply_settings(&settings); renderer.clear();
        assert!(renderer.near_arena.is_none(), "diagnostic selection remains sticky for this Renderer");
        assert_eq!(renderer.near_arena_report()["policy"]["diagnostic_suppressed"], true);
    }
}

#[test]
#[ignore = "requires original resources and Vulkan MDI; live original diagnostics stay safe while live shared ownership is rejected"]
fn diagnostic_activation_on_live_original_preserves_backing_and_rejects_live_shared() {
    let world = world(); let player = player();
    let mut sim = Simulation::new(GameMode::Creative, 189); sim.clock = 6000;
    let mut expected: Option<Vec<Image>> = None;
    for choice in [Some(Choice::Original), None] {
        let mut settings = settings(); settings.render_distance = 4;
        let mut renderer = policy_renderer(choice, &settings, &world, &player, &sim);
        for id in 0..4 { upload(&mut renderer, id, 0, false); }
        drop(capture(&mut renderer, &settings, &world, &player, &sim));
        drop(capture(&mut renderer, &settings, &world, &player, &sim));
        let before = capture(&mut renderer, &settings, &world, &player, &sim);
        assert!(renderer.terrain_draws_cached());
        assert!(renderer.near_arena.is_none());
        // This is the existing scoped-sort oracle's phase6 trigger: geometry
        // is resident, but no arena owns any allocation. Enabling this capture
        // must not require discarding those valid dedicated meshes.
        renderer.configure_transparency_capture(true);
        assert!(renderer.near_arena.is_none());
        assert_eq!(renderer.chunks.len(), 4);
        assert!(renderer.chunks.values().all(|c| c.gpu.arena_token.is_none()
            && c.gpu.vertex_offset == 0 && c.gpu.index_offset == 0));
        let same_settings = capture(&mut renderer, &settings, &world, &player, &sim);
        settings.render_distance = 128; renderer.apply_settings(&settings);
        assert!(renderer.near_arena.is_none(), "enabled diagnostic must prevent later automatic shared admission");
        assert_eq!(renderer.near_arena_report()["policy"]["diagnostic_suppressed"], choice.is_none());
        let high = capture(&mut renderer, &settings, &world, &player, &sim);
        let images: Vec<_> = [before, same_settings, high].into_iter().map(|t| t.finish(&renderer.device)).collect();
        assert_eq!(images[0], images[1], "enabling capture cannot change resident original geometry or pixels");
        if let Some(expected) = &expected { assert_eq!(&images, expected); } else { expected = Some(images); }
    }

    let mut settings = settings(); settings.render_distance = 128;
    let mut renderer = policy_renderer(None, &settings, &world, &player, &sim);
    assert!(renderer.near_arena.is_some()); small_empty_arena(&mut renderer);
    for id in 0..4 { upload(&mut renderer, id, 0, false); }
    assert!(renderer.chunks.values().all(|c| c.gpu.arena_token.is_some()));
    let before = capture(&mut renderer, &settings, &world, &player, &sim);
    let tokens: Vec<_> = (0..4).map(|id| renderer.chunks[&(id,0)].gpu.arena_token).collect();
    let policy = renderer.near_arena_report()["policy"].clone();
    let capture_state = renderer.transparency_capture_report();
    let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        renderer.configure_transparency_capture(true);
    }));
    assert!(rejected.is_err(), "an active shared owner may not be dropped to enable a dedicated diagnostic");
    assert_eq!(renderer.near_arena_report()["policy"], policy, "reject before sticky policy mutation");
    assert_eq!(renderer.transparency_capture_report(), capture_state, "reject before collector replacement");
    assert_eq!((0..4).map(|id| renderer.chunks[&(id,0)].gpu.arena_token).collect::<Vec<_>>(), tokens);
    assert!(renderer.arena_test_health().1);
    let after = capture(&mut renderer, &settings, &world, &player, &sim);
    assert_eq!(before.finish(&renderer.device), after.finish(&renderer.device));
}
