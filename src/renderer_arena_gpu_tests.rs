//! Integration oracle: actual Renderer upload, sort, encode and submit paths.
//! No fixture shader or hand-written draw loop substitutes for production draw.
use super::*;
use crate::{gameplay::Mode as GameMode, ui::Screen};

#[derive(Debug, PartialEq, Eq)]
struct Geometry {
    position: ChunkPos,
    count: u32,
    width: u32,
    bounds: [[u32; 3]; 2],
    present: u16,
    mask: u16,
    solid: Vec<std::ops::Range<u32>>,
    transparent: std::ops::Range<u32>,
    bytes: Vec<u8>,
}
#[derive(Debug, PartialEq, Eq)]
struct Image {
    color: Vec<[u8; 4]>,
    depth: Vec<u32>,
    geometry: Vec<Geometry>,
}
struct Ticket {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    row: u32,
    plane: u64,
    geometry: Vec<(Geometry, u64)>,
}
impl Ticket {
    fn finish(self, device: &wgpu::Device) -> Image {
        let (tx, rx) = std::sync::mpsc::channel();
        self.buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| { let _ = tx.send(r); });
        device.poll(wgpu::Maintain::Wait);
        rx.recv().unwrap().unwrap();
        let mapped = self.buffer.slice(..).get_mapped_range();
        let mut image = Image { color: Vec::new(), depth: Vec::new(), geometry: Vec::new() };
        for y in 0..self.height as usize {
            let start = y * self.row as usize;
            image.color.extend(mapped[start..start + self.width as usize * 4].chunks_exact(4)
                .map(|v| <[u8; 4]>::try_from(v).unwrap()));
            let start = self.plane as usize + start;
            image.depth.extend(mapped[start..start + self.width as usize * 4].chunks_exact(4)
                .map(|v| u32::from_le_bytes(v.try_into().unwrap())));
        }
        for (mut geometry, offset) in self.geometry {
            let n = geometry.count as usize * geometry.width as usize;
            geometry.bytes.extend_from_slice(&mapped[offset as usize..offset as usize + n]);
            image.geometry.push(geometry);
        }
        drop(mapped);
        self.buffer.unmap();
        image
    }
}

// Production sorts centers, not map keys. Every panel has the same conservative
// X/Y extent and a distinct Z center, realized by two unreferenced vertices.
// For these camera positions all centers lie strictly behind the camera. Thus
// equal-distance HashMap encounter order cannot cause false three-device diffs,
// while the common coplanar face still exposes a reordered actual draw stream.
fn panel(id: i32, variant: u8, wide: bool) -> Mesh {
    let mut mesh = models::empty();
    let mut sections = crate::mesh::visibility::TerrainSections::new(&crate::world::Chunk::empty(), true);
    let x = -9. + (id % 4) as f32 * 4.5;
    let y = 59. + (id / 4) as f32 * 6.;
    let texture = crate::validation::assets().texture(1 << 4, 2, true);
    let mut face = |x: f32, y: f32, z: f32, w: f32, h: f32, section: u8, layer: u32, color: [u8; 4]| {
        models::face(&mut mesh, [Vec3::new(x, y, z), Vec3::new(x+w, y, z),
            Vec3::new(x+w, y+h, z), Vec3::new(x, y+h, z)], [0., 0., 3., 1.], layer, color);
        sections.owners.push(section);
    };
    for section in 3..6u8 {
        let textured = section == 4 && id % 2 == 0;
        let layer = if textured { texture | models::UNLIT } else { models::SOLID | models::UNLIT };
        let color = if textured { [255; 4] } else { [40 + id as u8 * 20, 70 + variant * 50, 150, 255] };
        face(x, y + f32::from(section - 3), 8., 4., 0.9, section, layer, color);
    }
    face(-2., 71., 10., 4., 2., 3, models::SOLID | models::UNLIT,
        [30 + id as u8 * 25, 100 + variant * 40, 60, 255]);
    // Left/right transparent distances swap when the camera crosses X=0.
    for (dx, color) in [(-4., [240, 80, 80, 100]), (4., [70, 140, 240, 110])] {
        face(dx - 2., 62., 12., 4., 6., 3, models::SOLID | models::UNLIT | models::BLEND, color);
    }
    drop(face);
    if variant == 2 { mesh.indices.drain(0..6); sections.owners.remove(0); }
    if wide {
        mesh.vertices.splice(0..0, vec![mesh.vertices[0]; 65_536]);
        for index in &mut mesh.indices { *index += 65_536; }
    }
    mesh.min = Vec3::new(-12., 56., -20. - id as f32 * 2.);
    mesh.max = Vec3::new(12., 74., 12.);
    let mut bound = mesh.vertices[0];
    bound.position = mesh.min.to_array(); mesh.vertices.push(bound);
    bound.position = mesh.max.to_array(); mesh.vertices.push(bound);
    mesh.sections = Some(sections);
    mesh
}
fn upload(renderer: &mut Renderer, id: i32, variant: u8, wide: bool) {
    renderer.upload((id, 0), panel(id, variant, wide));
    renderer.chunks.get_mut(&(id, 0)).unwrap().graph = None;
}
fn settings() -> Settings {
    Settings { render_distance: 16, clouds: 0, entity_shadows: false, ..Default::default() }
}
fn world() -> World {
    World { chunks: Default::default(), spawn: Vec3::ZERO, label: String::new(), save_path: Default::default(),
        edits: Default::default(), imported: false, light_revision: 0 }
}
fn player() -> Player {
    let mut player = Player::new(Vec3::new(0., 65., 55.));
    player.set_rotation_degrees(180., 0.); player
}
fn setup(choice: Option<Choice>, settings: &Settings, world: &World, player: &Player, sim: &Simulation) -> Renderer {
    let mut renderer = pollster::block_on(Renderer::new_with_arena(None, crate::validation::assets(),
        crate::validation::resources(), settings, None, choice)).unwrap();
    renderer.resize(640, 360);
    if renderer.near_arena.is_some() { renderer.arena_test_limits(Limits { vertex_page: 4*1024*1024, index_page: 64*1024, owners: 64 }); }
    renderer.torch_flicker = Default::default();
    renderer.visual_random = crate::vanilla::random::JavaRandom::new(189);
    for _ in 0..40 { renderer.tick_lighting(world, player, sim, settings.render_distance); }
    if choice == Some(Choice::Mdi) {
        assert_eq!(renderer.near_arena_report()["effective"], "ArenaMdi", "fixture requires actual native MDI admission");
    }
    renderer
}
fn capture(renderer: &mut Renderer, settings: &Settings, world: &World, player: &Player, sim: &Simulation) -> Ticket {
    let mut distances: Vec<_> = renderer.chunks.values().filter(|chunk| chunk.gpu.count != 0)
        .map(|chunk| (((chunk.gpu.min + chunk.gpu.max)*0.5 - player.eye(1.)).length_squared()).to_bits()).collect();
    distances.sort_unstable();
    assert!(distances.windows(2).all(|pair| pair[0] != pair[1]), "independent maps must have a unique production sort key");
    let [width, height] = renderer.resolution();
    let extent = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
    let texture = |format, label| renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label), size: extent, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format, usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[] });
    let color = texture(renderer.config.format, "actual arena Renderer color");
    let depth = texture(DEPTH, "actual arena Renderer readable depth");
    renderer.depth = depth.create_view(&Default::default());
    let row = (width*4).div_ceil(256)*256;
    let plane = u64::from(row)*u64::from(height);
    let mut keys: Vec<_> = renderer.chunks.keys().copied().collect(); keys.sort_unstable();
    let bytes = keys.iter().map(|key| {
        let gpu = &renderer.chunks[key].gpu;
        (u64::from(gpu.count)*if gpu.index_format == wgpu::IndexFormat::Uint16 { 2 } else { 4 }).div_ceil(4)*4
    }).sum::<u64>();
    let buffer = renderer.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("queued actual arena Renderer readback"), size: plane*2+bytes,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let gui = Gui { screen: Screen::Playing, hide_hud: true, ..Default::default() };
    let ui = Frame::default();
    let input = RenderInput { selected_block: None, assets: crate::validation::assets(), resources: crate::validation::resources(),
        settings, gui: &gui, ui: &ui, world: Some(world), player: Some(player), simulation: Some(sim), alpha: 1., time: 0. };
    let encoder = renderer.device.create_command_encoder(&Default::default());
    let mut profile = render_profile::Frame { enabled: true, ..Default::default() };
    let mut geometry = Vec::new();
    renderer.submit_arena_test_frame(encoder, &color.create_view(&Default::default()), &input, &mut profile, |encoder, renderer| {
        // Session owns the arena mutex here. Only copy buffers and scalar chunk
        // metadata; never call a reporting/health method that re-locks State.
        for (texture, offset, aspect) in [(&color, 0, wgpu::TextureAspect::All), (&depth, plane, wgpu::TextureAspect::DepthOnly)] {
            encoder.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect },
                wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout {
                    offset, bytes_per_row: Some(row), rows_per_image: Some(height) } }, extent);
        }
        let mut offset = plane*2;
        for key in keys {
            let chunk = &renderer.chunks[&key]; let gpu = &chunk.gpu;
            let width = if gpu.index_format == wgpu::IndexFormat::Uint16 { 2 } else { 4 };
            let bytes = (u64::from(gpu.count)*u64::from(width)).div_ceil(4)*4;
            if bytes != 0 { encoder.copy_buffer_to_buffer(&gpu.indices, gpu.index_offset, &buffer, offset, bytes); }
            geometry.push((Geometry { position: key, count: gpu.count, width,
                bounds: [gpu.min.to_array().map(f32::to_bits), gpu.max.to_array().map(f32::to_bits)],
                present: chunk.present, mask: chunk.mask, solid: chunk.solid_ranges.clone(),
                transparent: chunk.translucent_range(), bytes: Vec::new() }, offset));
            offset += bytes;
        }
    });
    if renderer.near_arena.is_some() { assert!(!renderer.terrain_draws_cached(), "all explicit arms bypass the whole-list cache"); }
    Ticket { buffer, width, height, row, plane, geometry }
}

#[test]
#[ignore = "requires original resources and Vulkan native MDI; actual Renderer exact color/depth/index lifecycle"]
fn renderer_arena_matches_ordinary_and_explicit_dedicated_through_real_submit_lifecycle() {
    let world = world(); let mut sim = Simulation::new(GameMode::Creative, 189); sim.clock = 6000;
    let mut expected: Option<Vec<Image>> = None;
    for choice in [Some(Choice::Original), Some(Choice::Dedicated), Some(Choice::Direct), Some(Choice::Mdi)] {
        let mut settings = settings(); let mut player = player();
        let mut renderer = setup(choice, &settings, &world, &player, &sim);
        let mut tickets = Vec::new(); let mut first_token = None;
        for phase in 0..13 {
            match phase {
                1 => { for id in 0..8 { upload(&mut renderer, id, 0, id == 7); }
                    first_token = renderer.chunks[&(0, 0)].gpu.arena_token;
                    if matches!(choice, Some(Choice::Direct | Choice::Mdi)) {
                        assert!(renderer.chunks.values().all(|c| c.gpu.arena_token.is_some()));
                        assert!(renderer.chunks.values().any(|c| c.gpu.vertex_offset != 0 && c.gpu.index_offset != 0));
                    }
                }
                3 => upload(&mut renderer, 2, 1, false),
                4 => player.set_pose_f64([4., 65., 55.], 180., 0., true),
                5 => { player.set_pose_f64([-4., 65., 55.], 180., 0., true);
                    settings.anaglyph = true; renderer.apply_settings(&settings); }
                6 => { upload(&mut renderer, 0, 1, true);
                    if let Some(token) = first_token { assert_ne!(renderer.chunks[&(0, 0)].gpu.arena_token, Some(token)); }
                    // Two >2 MiB owners fill the two 4 MiB vertex pages. A
                    // third one cannot fit either hole and must use the exact
                    // dedicated fallback, without increasing the page cap.
                    upload(&mut renderer, 6, 1, true);
                    if matches!(choice, Some(Choice::Direct | Choice::Mdi)) {
                        assert!(renderer.chunks[&(0, 0)].gpu.arena_token.is_some());
                        assert!(renderer.chunks[&(6, 0)].gpu.arena_token.is_none());
                        assert_eq!(renderer.near_arena_report()["unique_shared_vertex_pages"], 2);
                        assert_eq!(renderer.near_arena_report()["fallbacks"], 1);
                    }
                }
                7 => { for chunk in renderer.chunks.values_mut() { chunk.present = (1<<3) | (1<<5); } }
                8 => { let old = renderer.chunks[&(1, 0)].gpu.arena_token; renderer.remove((1, 0));
                    upload(&mut renderer, 1, 2, false);
                    if let Some(token) = old { assert_ne!(renderer.chunks[&(1, 0)].gpu.arena_token, Some(token)); }
                }
                9 => { upload(&mut renderer, 0, 1, false); renderer.upload((3, 0), models::empty()); }
                10 => { renderer.clear();
                    if renderer.near_arena.is_some() { let report = renderer.near_arena_report();
                        assert_eq!(report["unique_shared_committed_bytes"], 0); assert_eq!(report["shared_chunks"], 0); }
                }
                11 => { for id in 0..6 { upload(&mut renderer, id, 1, false); }
                    if let Some(token) = first_token { assert_ne!(renderer.chunks[&(0, 0)].gpu.arena_token.unwrap().identity()[0], token.identity()[0]); }
                }
                12 => { settings.anaglyph = false; settings.mipmaps = 0; renderer.apply_settings(&settings); }
                _ => {}
            }
            // The existing ordinary cache requires three unchanged nonempty
            // frames. Submit one extra identical frame in EVERY arm without
            // shifting the thirteen compared phase positions or host-waiting.
            if phase == 2 { drop(capture(&mut renderer, &settings, &world, &player, &sim)); }
            tickets.push(capture(&mut renderer, &settings, &world, &player, &sim));
            if phase == 1 { assert_eq!(renderer.visible_chunks, 8); }
            if phase == 2 && choice == Some(Choice::Original) { assert!(renderer.terrain_draws_cached(), "ordinary independent reference actually uses its existing cache"); }
            if phase == 7 { assert!(renderer.chunks.values().all(|c| c.solid_ranges.len() == 2)); }
        }
        // Every frame copy was submitted before the next upload/reuse/clear.
        // There is deliberately no host map or Maintain::Wait inside that loop.
        let actual: Vec<_> = tickets.into_iter().map(|ticket| ticket.finish(&renderer.device)).collect();
        assert!(actual[0].color.iter().zip(&actual[1].color).filter(|(a,b)| a != b).count() > 1000);
        assert_ne!(actual[2].color, actual[3].color, "fitting payload replacement must visibly matter");
        assert_ne!(actual[4].geometry[0].bytes, actual[5].geometry[0].bytes, "moving eye must change actual transparent index writes");
        assert!(actual[1].geometry.iter().any(|g| g.width == 2) && actual[1].geometry.iter().any(|g| g.width == 4));
        if choice == Some(Choice::Mdi) {
            let report = renderer.near_arena_report();
            assert!(report["multi_calls_per_eye_not_scaled"].as_u64().unwrap() > 0);
            assert!(report["multi_records_per_eye_not_scaled"].as_u64().unwrap() > 8);
        }
        if let Some(expected) = &expected {
            assert_eq!(actual.len(), expected.len());
            for (phase, (actual, expected)) in actual.iter().zip(expected).enumerate() {
                assert_eq!(actual, expected, "actual Renderer choice {choice:?}, phase {phase}");
            }
        } else { expected = Some(actual); }
    }
}

fn logical_indices(image: &Image) -> Vec<Vec<u32>> {
    image.geometry.iter().map(|g| if g.width == 2 {
        g.bytes.chunks_exact(2).map(|b| u32::from(u16::from_le_bytes(b.try_into().unwrap()))).collect()
    } else { g.bytes.chunks_exact(4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).collect() }).collect()
}
fn forced_wide_panel() -> PreparedChunk {
    let mut prepared = PreparedChunk::new(panel(0, 0, false)); prepared.force_wide_indices(); prepared
}
#[test]
#[ignore = "requires original resources and Vulkan; injected checked failures never request real OOM"]
fn renderer_arena_checked_failures_keep_old_geometry_or_fail_closed_after_target() {
    let settings = settings(); let world = world(); let player = player();
    let mut sim = Simulation::new(GameMode::Creative, 189); sim.clock = 6000;
    for choice in [Choice::Direct, Choice::Mdi] {
        let mut renderer = setup(Some(choice), &settings, &world, &player, &sim);
        upload(&mut renderer, 0, 0, false);
        let old = renderer.chunks[&(0, 0)].gpu.arena_token.unwrap();
        let before = capture(&mut renderer, &settings, &world, &player, &sim);
        // A first U32 index page must be created; inject validation errors in
        // both that factory and the checked dedicated fallback, before target.
        renderer.arena_test_faults(true, true, false);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| renderer.upload((0, 0), forced_wide_panel()))).is_err());
        assert_eq!(renderer.arena_test_health(), (true, true));
        assert_eq!(renderer.chunks[&(0, 0)].gpu.arena_token, Some(old));
        renderer.arena_test_clear_mutex_poison();
        let unchanged = capture(&mut renderer, &settings, &world, &player, &sim);
        // Repeat only the page error. Checked dedicated replacement succeeds,
        // and only then may the old shared token be retired.
        renderer.arena_test_faults(true, false, false);
        renderer.upload((0, 0), forced_wide_panel()); renderer.chunks.get_mut(&(0, 0)).unwrap().graph = None;
        assert!(renderer.chunks[&(0, 0)].gpu.arena_token.is_none());
        assert_eq!(renderer.chunks[&(0, 0)].gpu.index_format, wgpu::IndexFormat::Uint32);
        assert!(renderer.near_arena.as_ref().unwrap().lock().unwrap().arena.binding(old).is_err());
        let fallback = capture(&mut renderer, &settings, &world, &player, &sim);
        let before = before.finish(&renderer.device); let unchanged = unchanged.finish(&renderer.device);
        let fallback = fallback.finish(&renderer.device);
        assert_eq!(before, unchanged, "two checked allocation errors must preserve all old output and active bytes");
        assert_eq!(before.color, fallback.color); assert_eq!(before.depth, fallback.depth);
        assert_eq!(logical_indices(&before), logical_indices(&fallback));
        let report = renderer.near_arena_report();
        assert_eq!(report["fallbacks"], 2); assert_eq!(report["page_creation_failures"], 2);
        // Return to a shared owner, then fail AFTER obtaining its write target.
        // This is not a rollback: the arena must become permanently unusable.
        upload(&mut renderer, 0, 0, false);
        assert!(renderer.chunks[&(0, 0)].gpu.arena_token.is_some());
        renderer.arena_test_faults(false, false, true);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| upload(&mut renderer, 0, 1, false))).is_err());
        assert_eq!(renderer.arena_test_health(), (true, false));
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| renderer.arena_test_clear_mutex_poison())).is_err());
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| capture(&mut renderer, &settings, &world, &player, &sim))).is_err());
    }
}

#[path="renderer_arena_policy_gpu_tests.rs"]
mod policy_gpu_tests;

#[cfg(test)]
#[path="renderer_arena_upload_gpu_tests.rs"]
mod arena_upload_capture_tests;

#[path="renderer_index_staging_gpu_tests.rs"]
mod transparent_staging_tests;

#[path="renderer_translucent_mdi_gpu_tests.rs"]
mod translucent_mdi_gpu_tests;

#[path="renderer_pre_acquire_visibility_gpu_tests.rs"]
mod pre_acquire_visibility_gpu_tests;

#[path="renderer_packed_frustum_gpu_tests.rs"]
mod packed_frustum_gpu_tests;

#[path="renderer_gpu_stages_gpu_tests.rs"]
mod gpu_stages_tests;
