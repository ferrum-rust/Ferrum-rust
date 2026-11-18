use super::*;
use crate::{
    gameplay::{Mode, Simulation},
    models::{NO_MIPMAP, SOLID},
    player::Player,
    renderer::{RenderInput, Renderer},
    settings::Settings,
    ui::{Frame, Gui, Screen},
    world::Chunk,
};
use std::sync::Arc;

#[test]
fn cutout_sampling_matches_original_block_registry() {
    let blocks: Vec<u16> = serde_json::from_str(include_str!(
        "../tests/fixtures/vanilla189-cutout-blocks.json"
    ))
    .unwrap();
    assert_eq!(blocks.len(), 50);
    for state in 0..4096 {
        assert_eq!(
            texture_flags(state) & NO_MIPMAP != 0,
            blocks.contains(&(state >> 4)),
            "original CUTOUT layer, state {state}"
        );
    }
}

#[test]
#[ignore = "uses the installed original plant models and a Vulkan device"]
fn distant_cutout_plants_keep_original_pixels_with_mipmaps_enabled() {
    let assets = crate::validation::assets();
    let resources = crate::validation::resources();
    let mut world = World {
        chunks: Default::default(),
        spawn: Vec3::new(8., 64., 0.),
        label: "Original CUTOUT foliage".into(),
        save_path: Default::default(),
        edits: Default::default(),
        imported: false,
        light_revision: 0,
    };
    for x in -2..=2 {
        for z in 0..=5 {
            let mut chunk = Chunk::empty();
            chunk.biomes.fill(1);
            for lx in 0..16 {
                for lz in 0..16 {
                    chunk.set(lx, 63, lz, 2 << 4);
                }
            }
            world.chunks.insert((x, z), Arc::new(chunk));
        }
    }
    // Near flowers/ferns and dense distant grass exercise levels 0 through 4.
    for (row, z) in [6, 10, 16, 24, 36, 52, 72, 88].into_iter().enumerate() {
        for x in (-28..=44).step_by(2) {
            let state = if row < 3 {
                [31 << 4 | 1, 31 << 4 | 2, 37 << 4, 38 << 4 | 3,
                    175 << 4 | 2, 6 << 4][(x + 28) as usize / 2 % 6]
            } else { 31 << 4 | 1 };
            let p = IVec3::new(x, 64, z);
            world.set(p, state);
            if state >> 4 == 175 { world.set(p + IVec3::Y, 175 << 4 | 8); }
        }
    }
    let mut settings = Settings {
        clouds: 0,
        mipmaps: 0,
        render_distance: 8,
        entity_shadows: false,
        ..Default::default()
    };
    let mut player = Player::new(Vec3::new(8., 64., 0.));
    player.set_rotation_degrees(0., 7.);
    let mut sim = Simulation::new(Mode::Creative, 189);
    sim.clock = 6000;
    let mut renderer =
        pollster::block_on(Renderer::new(None, assets, resources, &settings, None)).unwrap();
    let (width, height) = (768, 432);
    renderer.resize(width, height);
    let gui = Gui { screen: Screen::Playing, hide_hud: true, ..Default::default() };
    let path = std::env::temp_dir().join(format!("ferrum-cutout-{}.png", std::process::id()));
    for _ in 0..40 { renderer.tick_lighting(&world, &player, &sim, 2); }
    let mut frames = Vec::new();
    for (mipmaps, regress) in [(0, false), (4, false), (4, true)] {
        settings.mipmaps = mipmaps;
        renderer.apply_settings(&settings);
        for &position in world.chunks.keys() {
            let mut mesh = build_with_settings(&world, position, assets, &settings);
            for vertex in &mut mesh.vertices {
                if vertex.layer & NO_MIPMAP == 0 {
                    // Flat ground keeps the comparison exclusively about plant sampling;
                    // actual terrain textures legitimately change with the mipmap option.
                    vertex.layer |= SOLID;
                    vertex.color = [105, 145, 75, 255];
                } else if regress {
                    vertex.layer &= !NO_MIPMAP;
                }
            }
            renderer.upload(position, mesh);
        }
        renderer.screenshot(&path, &RenderInput {
            selected_block: None,
            assets, resources, settings: &settings, gui: &gui, ui: &Frame::default(),
            world: Some(&world), player: Some(&player), simulation: Some(&sim),
            alpha: 1., time: 0.,
        }).unwrap();
        frames.push(image::open(&path).unwrap().to_rgb8());
    }
    std::fs::remove_file(path).unwrap();
    let mismatch = frames[0].pixels().zip(frames[1].pixels()).filter(|(a,b)| a != b).count();
    assert_eq!(mismatch, 0, "CUTOUT must keep the original nearest base-level pixels");
    let changed = frames[1].pixels().zip(frames[2].pixels()).filter(|(a,b)| a != b).count();
    let dark = frames[1].pixels().zip(frames[2].pixels())
        .filter(|(a,b)| i16::from(a[1]) - i16::from(b[1]) > 20).count();
    assert!(changed > 200 && dark > 100,
        "the distant fixture must catch the previous dark mip fragments: {changed} changed, {dark} dark");
    eprintln!("CUTOUT mips 0/4 identical; old mip path changes {changed} pixels, darkens {dark}");
    if let Some(path) = std::env::var_os("FERRUM_FOLIAGE_CAPTURE") {
        let mut sheet = image::RgbImage::new(width, height * 3);
        for (row, frame) in frames.iter().enumerate() {
            image::imageops::replace(&mut sheet, frame, 0, row as i64 * i64::from(height));
        }
        sheet.save(path).unwrap();
    }
}
