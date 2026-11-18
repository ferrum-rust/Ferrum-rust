//! Actual Renderer byte/pixel oracle for the bounded transparent staging trial.
//! Child of renderer_arena_gpu_tests: all draw/submit work is production code.
use super::*;
use super::super::super::transparent_staging::Mode as UploadMode;

fn staging_panel(id: i32, variant: u8, wide: bool) -> Mesh {
    let mut mesh = panel(id, variant, wide);
    // Separate the two transparent memberships. Opaque owners remain 3/4/5.
    // A partial mask must rewrite only six indices; an empty mask writes none.
    *mesh.sections.as_mut().unwrap().owners.last_mut().unwrap() = 5;
    mesh
}
fn staging_upload(renderer: &mut Renderer, id: i32, variant: u8, wide: bool) {
    renderer.upload((id, 0), staging_panel(id, variant, wide));
    renderer.chunks.get_mut(&(id, 0)).unwrap().graph = None;
}
fn staging_renderer(choice: Option<Choice>, mode: UploadMode, settings: &Settings,
    world: &World, player: &Player, sim: &Simulation) -> Renderer {
    let mut renderer = setup(choice, settings, world, player, sim);
    renderer.configure_transparent_staging(mode).unwrap();
    if choice.is_none() && settings.render_distance == 128 {
        assert_eq!(renderer.near_arena_report()["effective"], "ArenaMdi");
        assert_eq!(renderer.near_arena_report()["policy"]["automatic"], true);
    }
    renderer
}
fn count(renderer: &Renderer, name: &str) -> u64 {
    renderer.transparent_staging_report()["counts"][name].as_u64().unwrap()
}
fn submit_menu_frame(renderer: &mut Renderer, settings: &Settings) {
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("staging no-world loading/menu path"), size: wgpu::Extent3d { width: 640, height: 360, depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: renderer.config.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT, view_formats: &[] });
    let gui = Gui::default(); let ui = Frame { size: [640., 360.], ..Default::default() };
    let input = RenderInput { selected_block: None, assets: crate::validation::assets(), resources: crate::validation::resources(),
        settings, gui: &gui, ui: &ui, world: None, player: None, simulation: None, alpha: 1., time: 0. };
    let encoder = renderer.device.create_command_encoder(&Default::default());
    renderer.submit_arena_test_frame(encoder, &texture.create_view(&Default::default()), &input,
        &mut render_profile::Frame::default(), |_, _| {});
}
fn assert_mask_bytes(images: &[Image]) {
    for ((full, partial), empty) in images[4].geometry.iter().zip(&images[5].geometry).zip(&images[6].geometry) {
        assert_eq!(full.position, partial.position); assert_eq!(full.position, empty.position);
        assert_eq!(full.transparent.len(), 12); assert_eq!(partial.transparent.len(), 6);
        assert!(empty.transparent.is_empty());
        let prefix = full.transparent.start as usize * full.width as usize;
        let tail = prefix + 6 * full.width as usize;
        assert!(prefix != 0, "real opaque prefix must precede every transparent update");
        assert_eq!(&full.bytes[..prefix], &partial.bytes[..prefix], "opaque prefix untouched");
        assert_eq!(&full.bytes[tail..], &partial.bytes[tail..], "inactive suffix untouched on shrink");
        assert_eq!(partial.bytes, empty.bytes, "empty translucent result performs no write");
    }
}
fn staging_screenshot(renderer: &mut Renderer, settings: &Settings, world: &World,
    player: &Player, sim: &Simulation) -> Vec<u8> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    struct Owned(std::path::PathBuf);
    impl Drop for Owned { fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); } }
    let file = loop {
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("ferrum-staging-oracle-{}-{id}.png", std::process::id()));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(handle) => { drop(handle); break Owned(path); }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("private screenshot fixture: {error}"),
        }
    };
    let gui = Gui { screen: Screen::Playing, hide_hud: true, ..Default::default() };
    let ui = Frame::default();
    let input = RenderInput { selected_block: None, assets: crate::validation::assets(), resources: crate::validation::resources(),
        settings, gui: &gui, ui: &ui, world: Some(world), player: Some(player), simulation: Some(sim), alpha: 1., time: 0. };
    renderer.screenshot(&file.0, &input).unwrap();
    let image = image::open(&file.0).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (640, 360)); image.into_raw()
}

#[test]
#[ignore = "requires original resources and Vulkan MDI; real Renderer Queue/Staged byte lifecycle without per-frame waits"]
fn transparent_staging_matches_queue_through_masks_stereo_busy_reuse_and_real_screenshot() {
    let world = world(); let mut sim = Simulation::new(GameMode::Creative, 189); sim.clock = 6000;
    for choice in [Some(Choice::Original), None] {
        let mut expected: Option<(Vec<Image>, Image, Vec<u8>)> = None;
        for mode in [UploadMode::Queue, UploadMode::Staged] {
            let mut settings = settings(); settings.render_distance = 128;
            let mut player = player();
            let mut renderer = staging_renderer(choice, mode, &settings, &world, &player, &sim);
            if mode == UploadMode::Staged { renderer.staging_test_hold_recalled(true); }
            submit_menu_frame(&mut renderer, &settings);
            if mode == UploadMode::Staged {
                assert_eq!(count(&renderer, "empty_frames"), 1);
                assert_eq!(count(&renderer, "staged_copies"), 0);
                assert_eq!(count(&renderer, "remap_requests"), 0);
                assert_eq!(renderer.transparent_staging_report()["poisoned"], false);
            }
            let mut tickets = Vec::new();
            for phase in 0..14 {
                match phase {
                    1 => { for id in 0..8 { staging_upload(&mut renderer, id, 0, id == 7); }
                        if choice.is_none() {
                            assert!(renderer.chunks.values().all(|c| c.gpu.arena_token.is_some()));
                            assert!(renderer.chunks.values().any(|c| c.gpu.index_offset != 0 && c.gpu.vertex_offset != 0));
                        }
                    }
                    3 => player.set_pose_f64([-4., 65., 55.], 180., 0., true),
                    4 => { player.set_pose_f64([4., 65., 55.], 180., 0., true);
                        settings.anaglyph = true; renderer.apply_settings(&settings); }
                    5 => { for chunk in renderer.chunks.values_mut() { chunk.present = (1 << 3) | (1 << 4); } }
                    6 => { for chunk in renderer.chunks.values_mut() { chunk.present = 1 << 4; } }
                    7 => { for chunk in renderer.chunks.values_mut() { chunk.present = (1 << 3) | (1 << 4) | (1 << 5); }
                        player.set_pose_f64([-4., 65., 55.], 180., 0., true); }
                    8 => { staging_upload(&mut renderer, 0, 1, true); staging_upload(&mut renderer, 6, 1, true);
                        if choice.is_none() {
                            assert!(renderer.chunks[&(0, 0)].gpu.arena_token.is_some());
                            assert!(renderer.chunks[&(6, 0)].gpu.arena_token.is_none(), "real bounded arena dedicated fallback");
                        }
                    }
                    9 => { renderer.remove((1, 0)); staging_upload(&mut renderer, 1, 2, false); }
                    10 => { staging_upload(&mut renderer, 0, 1, false); renderer.upload((3, 0), models::empty()); }
                    11 => renderer.clear(),
                    12 => { for id in 0..6 { staging_upload(&mut renderer, id, 1, false); } }
                    13 => { settings.anaglyph = false; settings.mipmaps = 0; renderer.apply_settings(&settings); }
                    _ => {}
                }
                // Third identical nonempty submitted frame really reaches the
                // existing ordinary whole-list cache; it does not host-wait.
                if phase == 2 { drop(capture(&mut renderer, &settings, &world, &player, &sim)); }
                let before = (mode == UploadMode::Staged).then(|| count(&renderer, "staged_copies"));
                tickets.push(capture(&mut renderer, &settings, &world, &player, &sim));
                if phase == 2 && choice == Some(Choice::Original) { assert!(renderer.terrain_draws_cached()); }
                if let Some(before) = before {
                    let delta = count(&renderer, "staged_copies") - before;
                    if [1, 3, 4, 5].contains(&phase) { assert_eq!(delta, 8, "phase{phase}: one preparation per chunk, including stereo"); }
                    else { assert_eq!(delta, 0, "unchanged/empty or whole-frame Queue fallback"); }
                    if phase == 5 { assert_eq!(count(&renderer, "staged_frames"), 4); }
                    if phase == 7 { assert!(count(&renderer, "busy_frames") > 0); }
                }
            }
            if mode == UploadMode::Staged {
                let report = renderer.transparent_staging_report();
                assert_eq!(report["slab_count"], 4); assert_eq!(report["slab_bytes"], 8 * 1024 * 1024);
                assert_eq!(report["fixed_source_payload_bytes"], 32 * 1024 * 1024);
                assert_eq!(count(&renderer, "staged_frames"), 4);
                assert_eq!(count(&renderer, "staged_copies"), 32);
                assert!(count(&renderer, "queue_writes") > 0);
                assert_eq!(report["poisoned"], false);
            }
            // ALL snapshots were submitted before later updates/release/clear.
            // The first host Wait is only here after the whole lifecycle.
            let images: Vec<_> = tickets.into_iter().map(|ticket| ticket.finish(&renderer.device)).collect();
            assert_mask_bytes(&images);
            assert!(images[1].geometry.iter().any(|g| g.width == 2) && images[1].geometry.iter().any(|g| g.width == 4));
            assert_ne!(images[0].color, images[1].color, "fixture must draw visible terrain");
            assert_ne!(images[1].geometry[0].bytes, images[3].geometry[0].bytes, "moving eye really changes GPU order");
            if mode == UploadMode::Staged { renderer.staging_test_hold_recalled(false); }
            player.set_pose_f64([4., 65., 55.], 180., 0., true);
            let reuse = capture(&mut renderer, &settings, &world, &player, &sim).finish(&renderer.device);
            if mode == UploadMode::Staged {
                assert!(count(&renderer, "reused_frames") > 0, "reuse a completed remapped slab, never grow the pool");
                assert_eq!(count(&renderer, "staged_copies"), 38);
            }
            player.set_pose_f64([-4., 65., 55.], 180., 0., true);
            let screenshot = staging_screenshot(&mut renderer, &settings, &world, &player, &sim);
            if mode == UploadMode::Staged { assert_eq!(count(&renderer, "staged_copies"), 44, "real screenshot owns staging submit acknowledgement"); }
            if let Some((expected_images, expected_reuse, expected_screenshot)) = &expected {
                assert_eq!(&images, expected_images, "exact Queue/Staged pixels, depth, opaque prefix, active and inactive indices: {choice:?}");
                assert_eq!(&reuse, expected_reuse); assert_eq!(&screenshot, expected_screenshot);
            } else { expected = Some((images, reuse, screenshot)); }
        }
    }
}

#[test]
#[ignore = "requires original resources and Vulkan; whole-frame oversized/distance fallback and real serial capture"]
fn transparent_staging_fallback_and_capture_preserve_queue_bytes() {
    let world = world(); let mut sim = Simulation::new(GameMode::Creative, 189); sim.clock = 6000;
    for scenario in 0..3 {
        let mut expected = None;
        for mode in [UploadMode::Queue, UploadMode::Staged] {
            let mut settings = settings(); settings.render_distance = if scenario == 1 { 16 } else { 128 };
            let mut player = player();
            let mut renderer = staging_renderer(Some(Choice::Original), mode, &settings, &world, &player, &sim);
            if mode == UploadMode::Staged && scenario == 0 { renderer.staging_test_capacity(8); }
            if scenario == 2 { renderer.configure_transparency_capture(true); renderer.transparency_capture_time(Some(100.)); }
            for id in 0..8 { staging_upload(&mut renderer, id, 0, id == 7); }
            let mut tickets = Vec::new();
            tickets.push(capture(&mut renderer, &settings, &world, &player, &sim));
            player.set_pose_f64([-4., 65., 55.], 180., 0., true);
            tickets.push(capture(&mut renderer, &settings, &world, &player, &sim));
            let images: Vec<_> = tickets.into_iter().map(|ticket| ticket.finish(&renderer.device)).collect();
            if scenario == 2 { assert_eq!(renderer.transparency_capture_report()["cases"], 1, "actual collector begin/finish ticket, not only its enabled branch"); }
            if mode == UploadMode::Staged {
                if scenario == 2 {
                    assert_eq!(count(&renderer, "staged_copies"), 16);
                    assert_eq!(count(&renderer, "queue_writes"), 0);
                } else {
                    assert_eq!(count(&renderer, "staged_copies"), 0);
                    assert_eq!(count(&renderer, "queue_writes"), 16);
                    assert_eq!(count(&renderer, if scenario == 0 { "oversize_frames" } else { "distance_frames" }), 2);
                }
                assert_eq!(renderer.transparent_staging_report()["poisoned"], false);
            }
            if let Some(expected) = &expected { assert_eq!(&images, expected, "fallback/capture scenario{scenario}"); }
            else { expected = Some(images); }
        }
    }
}

#[test]
#[ignore = "requires original resources and Vulkan; guard before CPU sort and permanent abandoned-encoder poison"]
fn transparent_staging_rejects_raw_encode_and_poisoned_identical_eye_retry() {
    let world = world(); let mut sim = Simulation::new(GameMode::Creative, 189); sim.clock = 6000;
    let mut settings = settings(); settings.render_distance = 128;
    let player = player();
    let mut renderer = staging_renderer(Some(Choice::Original), UploadMode::Staged, &settings, &world, &player, &sim);
    staging_upload(&mut renderer, 0, 0, false);
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("staging abandoned encoder guard"), size: wgpu::Extent3d { width: 640, height: 360, depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: renderer.config.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT, view_formats: &[] });
    let view = texture.create_view(&Default::default());
    let gui = Gui { screen: Screen::Playing, hide_hud: true, ..Default::default() }; let ui = Frame::default();
    let input = RenderInput { selected_block: None, assets: crate::validation::assets(), resources: crate::validation::resources(),
        settings: &settings, gui: &gui, ui: &ui, world: Some(&world), player: Some(&player), simulation: Some(&sim), alpha: 1., time: 0. };
    let mut encoder = renderer.device.create_command_encoder(&Default::default());
    assert!(renderer.chunks[&(0, 0)].translucent_range().is_empty());
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        renderer.encode(&mut encoder, &view, &input, &mut render_profile::Frame::default());
    })).is_err(), "raw encode cannot abandon selected staging work");
    drop(encoder);
    assert!(renderer.chunks[&(0, 0)].translucent_range().is_empty(), "raw guard precedes every Order.sort");
    assert_eq!(renderer.transparent_staging_report()["poisoned"], false);
    let encoder = renderer.device.create_command_encoder(&Default::default());
    renderer.staging_test_abandon_frame(encoder, &view, &input, &mut render_profile::Frame::default());
    assert_eq!(renderer.chunks[&(0, 0)].translucent_range().len(), 12, "abandon actual completed CPU sorting and encoded copies");
    assert_eq!(renderer.transparent_staging_report()["poisoned"], true);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drop(capture(&mut renderer, &settings, &world, &player, &sim));
    })).is_err(), "same eye/mask cannot silently skip the lost staged copy");
    renderer.clear();
    assert!(renderer.configure_transparent_staging(UploadMode::Queue).is_err(), "mode toggle cannot revive poisoned owner");
    assert!(renderer.configure_transparent_staging(UploadMode::Staged).is_err(), "clear cannot revive poisoned owner");
}

#[test]
#[ignore = "requires original resources and Vulkan; actual ScopedWide admission through staged batch completion"]
fn transparent_staging_batch_completion_matches_queue_with_98304_real_sorted_quads() {
    let world = world(); let mut sim = Simulation::new(GameMode::Creative, 189); sim.clock = 6000;
    let mut settings = settings(); settings.render_distance = 128;
    let mut expected = None;
    for mode in [UploadMode::Queue, UploadMode::Staged] {
        let mut player = player();
        let mut renderer = staging_renderer(Some(Choice::Original), mode, &settings, &world, &player, &sim);
        renderer.configure_transparency_sort(super::super::super::TransparencySortMode::ScopedWide);
        renderer.transparency_sort.ensure_helper_for_test();
        if mode == UploadMode::Staged { renderer.staging_test_hold_recalled(true); }
        for id in 0..32 {
            let mut mesh = staging_panel(id % 8, 0, id == 31);
            let n = mesh.indices.len();
            let left: [u32; 6] = mesh.indices[n-12..n-6].try_into().unwrap();
            let right: [u32; 6] = mesh.indices[n-6..].try_into().unwrap();
            // The full lifecycle above proves visible translucent blending.
            // This focused branch oracle keeps repeated faces subpixel-sized,
            // avoiding a gratuitous high-overdraw GPU stress test.
            for indices in [left, right] {
                let first = *indices.iter().min().unwrap() as usize;
                let center = mesh.vertices[first..first+4].iter().fold(Vec3::ZERO,
                    |sum, v| sum + Vec3::from_array(v.position)) * 0.25;
                for vertex in &mut mesh.vertices[first..first+4] {
                    let mut p = Vec3::from_array(vertex.position);
                    p.x = center.x + (p.x-center.x) * 0.0025;
                    p.y = center.y + (p.y-center.y) * 0.0025;
                    vertex.position = p.to_array();
                }
            }
            mesh.indices.truncate(n-12);
            let sections = mesh.sections.as_mut().unwrap();
            sections.owners.truncate(sections.owners.len()-2);
            for _ in 0..1536 {
                mesh.indices.extend_from_slice(&left); mesh.indices.extend_from_slice(&right);
                sections.owners.extend_from_slice(&[3, 5]);
            }
            assert_eq!(mesh.quads(), 4 + 3072);
            mesh.min.z = -20. - id as f32 * 2.;
            let bound = mesh.vertices.len()-2; mesh.vertices[bound].position = mesh.min.to_array();
            renderer.upload((id, 0), mesh); renderer.chunks.get_mut(&(id, 0)).unwrap().graph = None;
        }
        let mut tickets = Vec::new();
        for phase in 0..3 {
            if phase == 1 { player.set_pose_f64([-4., 65., 55.], 180., 0., true); }
            let runs = renderer.transparency_sort.successful_runs();
            tickets.push(capture(&mut renderer, &settings, &world, &player, &sim));
            assert_eq!(renderer.visible_chunks, 32);
            assert_eq!(renderer.transparency_sort.successful_runs()-runs, usize::from(phase < 2),
                "exact32-job/98304-quad admission must execute batch completion");
        }
        assert_eq!(renderer.transparency_sort.successful_runs(), 2);
        if mode == UploadMode::Staged {
            assert_eq!(count(&renderer, "staged_frames"), 2);
            assert_eq!(count(&renderer, "staged_copies"), 64);
            assert_eq!(count(&renderer, "queue_writes"), 0);
            assert_eq!(count(&renderer, "oversize_frames"), 0);
        }
        let images: Vec<_> = tickets.into_iter().map(|ticket| ticket.finish(&renderer.device)).collect();
        assert_ne!(images[0].geometry[0].bytes, images[1].geometry[0].bytes);
        assert_eq!(images[1], images[2], "unchanged eye preserves actual GPU bytes and pixels");
        assert!(images[0].geometry.iter().any(|g| g.width == 2) && images[0].geometry.iter().any(|g| g.width == 4));
        if let Some(expected) = &expected { assert_eq!(&images, expected, "actual batched staged completion equals Queue"); }
        else { expected = Some(images); }
    }
}
