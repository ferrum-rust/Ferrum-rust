//! Child of renderer_arena_gpu_tests; capture() uses the real submit-owned
//! helper, which prepares/consumes the same carrier used by Renderer::render.
use super::*;

const KEYS: [ChunkPos; 4] = [(-1, 0), (0, 0), (-1, 1), (0, 1)];
fn geometry(renderer: &mut Renderer, id: usize, variant: u8, wide: bool) {
    renderer.upload(KEYS[id], panel(id as i32, variant, wide));
    renderer.chunks.get_mut(&KEYS[id]).unwrap().graph = None;
}
fn camera_graphs(renderer: &mut Renderer, wall: bool) {
    let mut chunk = crate::world::Chunk::empty();
    if wall { for y in 0..256 { for z in 0..16 { chunk.set(8, y, z, 16); } } }
    for position in [(-1, 3), (0, 3)] {
        let mut mesh = models::empty();
        mesh.sections = Some(crate::mesh::visibility::TerrainSections::new(&chunk, true));
        renderer.upload(position, mesh);
    }
}
fn menu_attempt(renderer: &mut Renderer, settings: &Settings) {
    let [width, height] = renderer.resolution();
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pre-acquire no-world target"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format: renderer.config.format, usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let gui = Gui::default(); let ui = Frame::default();
    let input = RenderInput { selected_block: None, assets: crate::validation::assets(),
        resources: crate::validation::resources(), settings, gui: &gui, ui: &ui,
        world: None, player: None, simulation: None, alpha: 1., time: 0. };
    let encoder = renderer.device.create_command_encoder(&Default::default());
    let mut profile = render_profile::Frame { enabled: true, ..Default::default() };
    renderer.submit_arena_test_frame(encoder, &texture.create_view(&Default::default()),
        &input, &mut profile, |_, _| {});
    assert_eq!(profile.pre_acquire_ms, 0.);
    assert_eq!(profile.stages_ms.pre_acquire_section_visibility, 0.);
}
fn failed_acquisition(renderer: &mut Renderer, settings: &Settings, world: &World,
    player: &Player, sim: &Simulation, enabled: bool, error: wgpu::SurfaceError) {
    let gui = Gui { screen: Screen::Playing, hide_hud: true, ..Default::default() };
    let ui = Frame::default();
    let input = RenderInput { selected_block: None, assets: crate::validation::assets(),
        resources: crate::validation::resources(), settings, gui: &gui, ui: &ui,
        world: Some(world), player: Some(player), simulation: Some(sim), alpha: 1., time: 0. };
    let before_arena = renderer.near_arena_report();
    let before_random = renderer.visual_random.state();
    let before_map = renderer.map_key;
    let before_cloud = renderer.cloud_key;
    let before_lightmap = renderer.lightmap_key;
    let mut profile = render_profile::Frame { enabled: true, ..Default::default() };
    let prepared = renderer.prepare_pre_acquire_visibility(&input, &mut profile);
    assert_eq!(prepared.is_some(), enabled);
    if let Some(prepared) = &prepared {
        let hurt = HurtView::for_camera(sim, 1.);
        let expected = renderer.world_view(world, player, settings, gui.perspective, 1.,
            hurt, sim.equipped.bow_fov_multiplier(1.), sim.effects.get(9).is_some());
        assert_eq!(prepared.view.0.eye.to_array().map(f32::to_bits),
            expected.0.eye.to_array().map(f32::to_bits));
        assert_eq!(prepared.view.0.direction.to_array().map(f32::to_bits),
            expected.0.direction.to_array().map(f32::to_bits));
        assert_eq!(prepared.view.1.to_cols_array().map(f32::to_bits),
            expected.1.to_cols_array().map(f32::to_bits));
        assert_eq!(prepared.view.2.to_cols_array().map(f32::to_bits),
            expected.2.to_cols_array().map(f32::to_bits));
    }
    assert_eq!(profile.counters.tracked_buffer_write_calls, 0);
    assert_eq!(profile.stages_ms.transparency_sort, 0.);
    assert_eq!(profile.stages_ms.section_visibility, 0.);
    let expected_error = format!("{error:?}");
    let result = renderer.finish_pre_acquire_visibility::<()>(prepared.is_some(), Err(error));
    assert_eq!(format!("{:?}", result.unwrap_err()), expected_error);
    drop(prepared);
    assert_eq!(renderer.near_arena_report(), before_arena);
    assert_eq!(renderer.visual_random.state(), before_random);
    assert_eq!(renderer.map_key, before_map);
    assert_eq!(renderer.cloud_key, before_cloud);
    assert!(renderer.lightmap_key == before_lightmap);
    if renderer.near_arena.is_some() { assert_eq!(renderer.arena_test_health(), (false, true)); }
}

#[test]
#[ignore = "requires original resources and Vulkan; real pre-acquire carrier, failure recovery and pixels"]
fn pre_acquire_visibility_matches_original_carrier_output_and_failed_acquire_history() {
    let world = world();
    let mut sim = Simulation::new(GameMode::Creative, 189); sim.clock = 6000;
    for choice in [Some(Choice::Original), Some(Choice::Mdi)] {
        let mut expected: Option<Vec<Image>> = None;
        let mut expected_screenshot = None;
        for enabled in [false, true] {
            let mut settings = settings(); settings.render_distance = 128;
            let mut player = player(); player.set_pose_f64([-1.2, 65., 55.], 180., 0., true);
            let mut renderer = setup(choice, &settings, &world, &player, &sim);
            renderer.configure_pre_acquire_visibility(enabled);
            menu_attempt(&mut renderer, &settings);
            camera_graphs(&mut renderer, false);
            for id in 0..4 { geometry(&mut renderer, id, 0, id == 3); }
            if choice == Some(Choice::Mdi) {
                assert!(renderer.chunks.values().any(|chunk|
                    chunk.gpu.arena_token.is_some() && chunk.gpu.index_offset != 0));
            }
            let mut tickets = Vec::new();
            tickets.push(capture(&mut renderer, &settings, &world, &player, &sim));
            tickets.push(capture(&mut renderer, &settings, &world, &player, &sim));
            for error in [wgpu::SurfaceError::Timeout, wgpu::SurfaceError::Lost,
                wgpu::SurfaceError::Outdated] {
                // If failure incorrectly advances Order, its +0.6 eye could
                // make the next -0.1 eye skip a needed sort (distance<1), unlike
                // the prior -1.2 successful eye (distance>1). The active left/
                // right transparent face order changes across X=0.
                player.set_pose_f64([-1.2, 65., 55.], 180., 0., true);
                tickets.push(capture(&mut renderer, &settings, &world, &player, &sim));
                player.set_pose_f64([0.6, 65., 55.], 180., 0., true);
                failed_acquisition(&mut renderer, &settings, &world, &player, &sim, enabled, error);
                player.set_pose_f64([-0.1, 65., 55.], 180., 0., true);
                tickets.push(capture(&mut renderer, &settings, &world, &player, &sim));
            }
            // Publication invalidates the camera graph without changing the
            // camera cell; both histories must recompute equivalent visibility.
            camera_graphs(&mut renderer, true);
            player.set_pose_f64([-0.1, 65., 55.], 170., 0., true);
            tickets.push(capture(&mut renderer, &settings, &world, &player, &sim));
            settings.anaglyph = true; renderer.apply_settings(&settings);
            player.set_pose_f64([2., 65., 55.], 180., 0., true);
            tickets.push(capture(&mut renderer, &settings, &world, &player, &sim));
            renderer.resize(642, 362);
            tickets.push(capture(&mut renderer, &settings, &world, &player, &sim));
            camera_graphs(&mut renderer, false);
            geometry(&mut renderer, 1, 1, true);
            tickets.push(capture(&mut renderer, &settings, &world, &player, &sim));
            let before_empty = tickets.len() - 1;
            for key in KEYS { renderer.remove(key); }
            tickets.push(capture(&mut renderer, &settings, &world, &player, &sim));
            let report = renderer.pre_acquire_visibility_report();
            if enabled {
                assert_eq!(report["counts"]["world_prepared"], tickets.len() as u64 + 3);
                assert_eq!(report["counts"]["world_acquired"], tickets.len() as u64);
                assert_eq!(report["counts"]["world_aborted"], 3);
                assert_eq!(report["counts"]["empty_attempts"], 1);
                assert_eq!(report["counts"]["empty_acquired"], 1);
                assert_eq!(report["counts"]["empty_aborted"], 0);
            } else {
                assert_eq!(report, serde_json::json!({"enabled":false,"mode":"after-acquire"}));
            }
            // All changing frames and failure/recovery sequences are already
            // queued. Screenshot remains its ordinary no-surface path and must
            // not count a second speculative/acquired visibility attempt.
            for id in 0..4 { geometry(&mut renderer, id, 0, id == 3); }
            let gui = Gui { screen: Screen::Playing, hide_hud: true, ..Default::default() };
            let ui = Frame::default();
            let input = RenderInput { selected_block: None, assets: crate::validation::assets(),
                resources: crate::validation::resources(), settings: &settings, gui: &gui, ui: &ui,
                world: Some(&world), player: Some(&player), simulation: Some(&sim), alpha: 1., time: 0. };
            let path = std::env::temp_dir().join(format!("ferrum-pre-acquire-{}-{}-{enabled}.png",
                std::process::id(), if choice == Some(Choice::Mdi) { "mdi" } else { "original" }));
            renderer.screenshot(&path, &input).unwrap();
            let screenshot = std::fs::read(&path).unwrap();
            std::fs::remove_file(path).unwrap();
            assert_eq!(renderer.pre_acquire_visibility_report(), report);
            if let Some(expected) = &expected_screenshot { assert_eq!(&screenshot, expected); }
            else { expected_screenshot = Some(screenshot); }
            // The screenshot's terminal Wait services these older submissions;
            // their distinct readback buffers preserve every prior snapshot.
            let images = tickets.into_iter().map(|ticket| ticket.finish(&renderer.device)).collect::<Vec<_>>();
            assert!(images[before_empty].color.iter().zip(&images.last().unwrap().color)
                .filter(|(a,b)| a != b).count() > 50, "actual world geometry must be visible");
            if let Some(expected) = &expected { assert_eq!(&images, expected); }
            else { expected = Some(images); }
        }
    }
}
