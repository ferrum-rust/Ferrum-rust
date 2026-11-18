//! Actual Renderer alpha submission oracle; child of renderer_arena_gpu_tests.
//! No substitute shader, copied draw loop, or CPU-only image expectation.
use super::*;
use super::super::super::translucent_mdi::Mode as AlphaMode;

fn alpha_panel(id: i32, variant: u8, wide: bool) -> Mesh {
    let mut mesh = panel(id, variant, wide);
    for vertex in &mut mesh.vertices {
        if vertex.layer & models::BLEND != 0 {
            // Distinct per-owner, noncommuting colors at the SAME actual faces.
            // Unlike the parent fixture's repeated colors, chunk order matters.
            vertex.color = if vertex.position[0] < 0. {
                [35 + id as u8 * 20, 210 - id as u8 * 18 + variant * 10, 50 + id as u8 * 16, 60 + id as u8 * 12]
            } else {
                [220 - id as u8 * 20, 40 + id as u8 * 18 + variant * 10, 200 - id as u8 * 12, 80 + id as u8 * 9]
            };
        }
    }
    *mesh.sections.as_mut().unwrap().owners.last_mut().unwrap() = 5;
    mesh
}
fn alpha_upload(renderer: &mut Renderer, id: i32, variant: u8, wide: bool) {
    renderer.upload((id, 0), alpha_panel(id, variant, wide));
    renderer.chunks.get_mut(&(id, 0)).unwrap().graph = None;
}
fn alpha_renderer(mode: AlphaMode, settings: &Settings, world: &World, player: &Player, sim: &Simulation) -> Renderer {
    let mut renderer = setup(None, settings, world, player, sim);
    assert_eq!(renderer.near_arena_report()["effective"], "ArenaMdi");
    assert_eq!(renderer.near_arena_report()["policy"]["automatic"], true);
    assert_direct_storage(&renderer);
    renderer.configure_translucent_mdi(mode).unwrap();
    if mode == AlphaMode::Direct { assert_direct_storage(&renderer); }
    assert_eq!(renderer.transparent_staging_report()["mode"], "queue", "independent Queue index preparation in both arms");
    renderer
}
fn assert_direct_storage(renderer: &Renderer) {
    let report = renderer.translucent_mdi_report();
    assert_eq!(report["enabled"], false); assert_eq!(report["mode"], "direct");
    for key in ["argument_buffer_bytes", "range_capacity", "plan_reserved_bytes"] {
        if let Some(value) = report.get(key) {
            assert_eq!(value.as_u64(), Some(0),
                "ordinary direct alpha has no additional planner/range/argument storage: {key}");
        }
    }
}
fn alpha_count(renderer: &Renderer, group: &str, name: &str) -> u64 {
    renderer.translucent_mdi_report()[group][name].as_u64().unwrap()
}
fn alpha_screenshot(renderer: &mut Renderer, settings: &Settings, world: &World, player: &Player, sim: &Simulation) -> Vec<u8> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    struct Owned(std::path::PathBuf);
    impl Drop for Owned { fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); } }
    let file = loop {
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("ferrum-alpha-mdi-oracle-{}-{id}.png", std::process::id()));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(handle) => { drop(handle); break Owned(path); }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("private screenshot fixture: {error}"),
        }
    };
    let gui = Gui { screen: Screen::Playing, hide_hud: true, ..Default::default() }; let ui = Frame::default();
    let input = RenderInput { selected_block: None, assets: crate::validation::assets(), resources: crate::validation::resources(),
        settings, gui: &gui, ui: &ui, world: Some(world), player: Some(player), simulation: Some(sim), alpha: 1., time: 0. };
    renderer.screenshot(&file.0, &input).unwrap();
    let image = image::open(&file.0).unwrap().to_rgba8();
    let [width, height] = renderer.resolution(); assert_eq!(image.dimensions(), (width, height)); image.into_raw()
}
fn reverse_only_conservative_keys(renderer: &mut Renderer, reversed: bool) {
    assert_eq!(renderer.chunks.len(), 8);
    for (&(id, _), chunk) in &mut renderer.chunks {
        // All real geometry stays at z8/z10/z12. These bounds remain conservative
        // and unique; only the production chunk encounter order is reversed.
        let key = if reversed { 7-id } else { id };
        chunk.gpu.min.z = -20. - key as f32 * 2.;
    }
    renderer.terrain_bundles.invalidate();
}

#[test]
#[ignore = "requires original resources and Vulkan MDI; real opaque+alpha argument separation and queued lifecycle"]
fn translucent_mdi_matches_direct_through_real_renderer_lifecycle_and_screenshot() {
    let world = world(); let mut sim = Simulation::new(GameMode::Creative, 189); sim.clock = 6000;
    let mut expected: Option<(Vec<Image>, Vec<u8>)> = None;
    for mode in [AlphaMode::Direct, AlphaMode::Mdi] {
        let mut settings = settings(); settings.render_distance = 128;
        let mut player = player(); let mut renderer = alpha_renderer(mode, &settings, &world, &player, &sim);
        if mode == AlphaMode::Mdi { renderer.translucent_mdi_test_limits(3, 4096); }
        let mut tickets = Vec::new();
        for phase in 0..14 {
            match phase {
                1 => { for id in 0..8 { alpha_upload(&mut renderer, id, 0, id == 7); }
                    assert!(renderer.chunks.values().all(|c| c.gpu.arena_token.is_some()));
                    assert!(renderer.chunks.values().any(|c| c.gpu.vertex_offset != 0 && c.gpu.index_offset != 0)); }
                3 => alpha_upload(&mut renderer, 2, 1, false),
                4 => player.set_pose_f64([4., 65., 55.], 180., 0., true),
                5 => { player.set_pose_f64([-4., 65., 55.], 180., 0., true); settings.anaglyph = true; renderer.apply_settings(&settings); }
                6 => { for chunk in renderer.chunks.values_mut() { chunk.present = (1 << 3) | (1 << 4); } }
                7 => { for chunk in renderer.chunks.values_mut() { chunk.present = 1 << 4; } }
                8 => { for chunk in renderer.chunks.values_mut() { chunk.present = (1 << 3) | (1 << 4) | (1 << 5); }
                    alpha_upload(&mut renderer, 0, 1, true); alpha_upload(&mut renderer, 6, 1, true);
                    assert!(renderer.chunks[&(0, 0)].gpu.arena_token.is_some());
                    assert!(renderer.chunks[&(6, 0)].gpu.arena_token.is_none(), "real PageLimit fallback remains in-place among shared owners"); }
                9 => { renderer.remove((1, 0)); alpha_upload(&mut renderer, 1, 2, false); }
                10 => { alpha_upload(&mut renderer, 0, 1, false); renderer.upload((3, 0), models::empty()); }
                11 => renderer.clear(),
                12 => { for id in 0..6 { alpha_upload(&mut renderer, id, 1, false); } }
                13 => { settings.anaglyph = false; settings.mipmaps = 0; renderer.apply_settings(&settings); renderer.resize(704, 396); }
                _ => {}
            }
            let before = (mode == AlphaMode::Mdi).then(|| (
                alpha_count(&renderer, "counts", "prepared_frames"),
                alpha_count(&renderer, "counts", "planned_records"),
                alpha_count(&renderer, "issued", "eyes"),
                alpha_count(&renderer, "issued", "logical_records")));
            tickets.push(capture(&mut renderer, &settings, &world, &player, &sim));
            if phase == 1 {
                assert_eq!(renderer.visible_chunks, 8);
                if mode == AlphaMode::Mdi {
                    let report = renderer.translucent_mdi_report();
                    assert_eq!(report["max_records"], 4096); assert_eq!(report["argument_buffer_bytes"], 81920);
                    assert_eq!(report["range_capacity"], 4096);
                    assert!(alpha_count(&renderer, "counts", "planned_multi_calls") >= 2, "native max3 splits the seven compatible U16 records");
                    assert_eq!(alpha_count(&renderer, "counts", "planned_multi_records"), 6);
                    assert_eq!(alpha_count(&renderer, "counts", "planned_direct_calls"), 2, "wide singleton plus final narrow singleton stay direct");
                    assert_eq!(alpha_count(&renderer, "issued", "logical_records"), 8);
                }
            }
            if phase == 5 {
                if let Some((frames, records, eyes, issued)) = before {
                    assert_eq!(alpha_count(&renderer, "counts", "prepared_frames") - frames, 1, "prepare alpha arguments once before both eyes");
                    assert_eq!(alpha_count(&renderer, "counts", "planned_records") - records, 8);
                    assert_eq!(alpha_count(&renderer, "issued", "eyes") - eyes, 2);
                    assert_eq!(alpha_count(&renderer, "issued", "logical_records") - issued, 16);
                }
            }
        }
        // Both opaque and alpha argument uploads precede these actual passes.
        // A shared argument offset would corrupt opaque results, not just alpha.
        // Every readback is submitted BEFORE the next update/reuse/clear. First
        // host Wait occurs only after all14 logical frames have been submitted.
        let images: Vec<_> = tickets.into_iter().map(|ticket| ticket.finish(&renderer.device)).collect();
        assert_eq!(images[1], images[2], "unchanged second frame must retain identical pixels and GPU bytes");
        assert_ne!(images[0].color, images[1].color);
        assert_ne!(images[2].color, images[3].color, "fitting payload/color replacement visibly matters");
        assert!(images[1].depth.iter().any(|d| *d < 1.0_f32.to_bits()), "opaque argument stream really draws depth");
        assert!(images[1].geometry.iter().any(|g| g.width == 2) && images[1].geometry.iter().any(|g| g.width == 4));
        assert!(images[6].geometry.iter().all(|g| g.transparent.len() == 6));
        assert!(images[7].geometry.iter().all(|g| g.transparent.is_empty()));
        assert_ne!(images[4].geometry[0].bytes, images[5].geometry[0].bytes, "actual eye-dependent sorted index payload changes");
        player.set_pose_f64([4., 65., 55.], 180., 0., true);
        let screenshot = alpha_screenshot(&mut renderer, &settings, &world, &player, &sim);
        if mode == AlphaMode::Mdi {
            assert!(alpha_count(&renderer, "issued", "multi_calls") > 0);
            assert!(alpha_count(&renderer, "issued", "multi_records") > 8);
        } else { assert_direct_storage(&renderer); }
        if let Some((expected_images, expected_screenshot)) = &expected {
            assert_eq!(&images, expected_images, "real Direct/Mdi opaque+alpha RGBA/depth/full GPU index bytes");
            assert_eq!(&screenshot, expected_screenshot);
        } else { expected = Some((images, screenshot)); }
    }
}

#[test]
#[ignore = "requires original resources and Vulkan MDI; bounded alpha record overflow falls back in exact order"]
fn translucent_mdi_record_limit_fallback_and_later_admission_preserve_direct_output() {
    let world = world(); let mut sim = Simulation::new(GameMode::Creative, 189); sim.clock = 6000;
    let mut settings = settings(); settings.render_distance = 128;
    let mut expected = None;
    for mode in [AlphaMode::Direct, AlphaMode::Mdi] {
        let mut player = player(); let mut renderer = alpha_renderer(mode, &settings, &world, &player, &sim);
        if mode == AlphaMode::Mdi { renderer.translucent_mdi_test_limits(3, 4); }
        for id in 0..8 { alpha_upload(&mut renderer, id, 0, id == 7); }
        let mut tickets = Vec::new();
        for phase in 0..6 {
            match phase {
                1 => { for chunk in renderer.chunks.values_mut() { chunk.present = (1 << 3) | (1 << 4); } }
                2 => { for chunk in renderer.chunks.values_mut() { chunk.present = (1 << 3) | (1 << 4) | (1 << 5); }
                    player.set_pose_f64([-4., 65., 55.], 180., 0., true); }
                3 => { for id in 3..8 { renderer.remove((id, 0)); } }
                4 => renderer.upload((1, 0), models::empty()),
                5 => alpha_upload(&mut renderer, 4, 1, false),
                _ => {}
            }
            tickets.push(capture(&mut renderer, &settings, &world, &player, &sim));
            if mode == AlphaMode::Mdi {
                if phase < 3 {
                    assert_eq!(alpha_count(&renderer, "counts", "record_limit_fallback_frames"), phase + 1);
                    assert_eq!(alpha_count(&renderer, "issued", "multi_calls"), 0, "overflow never partially submits a reordered prefix");
                } else {
                    assert_eq!(alpha_count(&renderer, "counts", "record_limit_fallback_frames"), 3);
                    assert!(alpha_count(&renderer, "issued", "multi_calls") > 0, "same bounded planner admits later smaller real source");
                }
            }
        }
        // Replacements/removals and changing indirect counts were all queued
        // before these maps; no per-frame wait hides argument or owner reuse.
        let images: Vec<_> = tickets.into_iter().map(|ticket| ticket.finish(&renderer.device)).collect();
        assert_eq!(images[0].geometry.len(), 8); assert_eq!(images[3].geometry.len(), 3);
        assert!(images[1].geometry.iter().all(|g| g.transparent.len() == 6));
        assert!(images[2].geometry.iter().all(|g| g.transparent.len() == 12));
        assert!(images[4].geometry.iter().any(|g| g.count == 0));
        if let Some(expected) = &expected { assert_eq!(&images, expected, "bounded whole-source fallback and later admission keep Direct RGBA/depth/indices"); }
        else { expected = Some(images); }
    }
}

#[test]
#[ignore = "requires original resources and Vulkan MDI; noncommutative reverse-order sensitivity and queued return"]
fn translucent_mdi_preserves_noncommutative_alpha_order_and_rejects_forward_order_control() {
    let world = world(); let mut sim = Simulation::new(GameMode::Creative, 189); sim.clock = 6000;
    let mut settings = settings(); settings.render_distance = 128; let player = player();
    let mut expected = None;
    for mode in [AlphaMode::Direct, AlphaMode::Mdi] {
        let mut renderer = alpha_renderer(mode, &settings, &world, &player, &sim);
        if mode == AlphaMode::Mdi { renderer.translucent_mdi_test_limits(3, 4096); }
        for id in 0..8 { alpha_upload(&mut renderer, id, 0, id == 7); }
        let correct = capture(&mut renderer, &settings, &world, &player, &sim);
        reverse_only_conservative_keys(&mut renderer, true);
        let deliberately_wrong_order = capture(&mut renderer, &settings, &world, &player, &sim);
        reverse_only_conservative_keys(&mut renderer, false);
        let restored = capture(&mut renderer, &settings, &world, &player, &sim);
        if mode == AlphaMode::Mdi {
            assert!(alpha_count(&renderer, "issued", "multi_calls") >= 6,
                "all three order-control frames actually exercise native alpha multi-draw");
        }
        let images = [correct, deliberately_wrong_order, restored].map(|ticket| ticket.finish(&renderer.device));
        assert_eq!(images[0], images[2], "restoring only encounter keys restores exact output");
        assert_eq!(logical_indices(&images[0]), logical_indices(&images[1]), "negative control never changes actual index bytes");
        let clear = 1.0_f32.to_bits();
        let different_alpha_only = images[0].color.iter().zip(&images[1].color)
            .zip(images[0].depth.iter().zip(&images[1].depth))
            .filter(|((a, b), (za, zb))| a != b && **za == clear && **zb == clear).count();
        assert!(different_alpha_only > 100, "distinct per-owner alpha order must visibly matter where neither image writes opaque depth; got{different_alpha_only}");
        if let Some(expected) = &expected { assert_eq!(&images, expected, "candidate must match both independent order controls"); }
        else { expected = Some(images); }
    }
}
