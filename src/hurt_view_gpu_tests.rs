use crate::{
    gameplay::{Mode, Simulation},
    inventory::Stack,
    player::Player,
    renderer::{RenderInput, Renderer},
    settings::Settings,
    ui::{self, Frame, Gui, Screen, Stats},
    world::{Chunk, World},
};
use glam::Vec3;
use image::{GrayImage, Luma, RgbImage};
use std::sync::Arc;

// Sample the baseline mask at inverse-rotated pixel centers. Pixels newly
// exposed from outside the old viewport have no reference value.
fn check_roll(before: &GrayImage, peak: &GrayImage, name: &str) {
    let (width, height) = before.dimensions();
    let (cx, cy) = (width as f32 / 2., height as f32 / 2.);
    // Captured by HurtCameraOracle at hurtTime 9 and partialTicks .375.
    let (sin, cos) = (-13.803636_f32).to_radians().sin_cos();
    let (mut common, mut union, mut moved) = (0, 0, 0);
    for (x, y, pixel) in peak.enumerate_pixels() {
        let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
        let (sx, sy) = (cx + cos * dx - sin * dy, cy + sin * dx + cos * dy);
        if !(0.0..width as f32).contains(&sx) || !(0.0..height as f32).contains(&sy) {
            continue;
        }
        let expected = before.get_pixel(sx as u32, sy as u32)[0] != 0;
        let actual = pixel[0] != 0;
        union += u32::from(expected || actual);
        common += u32::from(expected && actual);
        moved += u32::from(actual != (before.get_pixel(x, y)[0] != 0));
    }
    assert!(moved > 200, "{name} failed to move ({moved} pixels)");
    assert!(
        common * 100 > union * 96,
        "{name} misses original projection ({common}/{union})"
    );
}

#[test]
#[ignore = "needs Vulkan and the installed 1.8.9 resources"]
fn vulkan_hurt_camera_rolls_world_and_hand_but_keeps_hud_fixed() {
    let assets = crate::validation::assets();
    let resources = crate::validation::resources();
    let settings = Settings {
        clouds: 0,
        render_distance: 4,
        gui_scale: 1,
        bobbing: false,
        ..Default::default()
    };
    let mut renderer =
        pollster::block_on(Renderer::new(None, assets, resources, &settings, None)).unwrap();
    let (width, height) = (512, 320);
    renderer.resize(width, height);
    let mut chunk = Chunk::empty();
    for x in 0..16 {
        for z in 0..16 {
            chunk.set(x, 63, z, 2 << 4);
        }
    }
    for x in 2..14 {
        for y in 64..68 {
            chunk.set(x, y, 5, 98 << 4);
        }
    }
    chunk.set(5, 66, 5, (35 << 4) | 14); // Red wool landmark in the stone wall.
    let world = World {
        chunks: [((0, 0), Arc::new(chunk))].into(),
        spawn: Vec3::new(8.5, 64., 12.5),
        label: String::new(),
        edits: Default::default(),
        save_path: Default::default(),
        imported: false,
        light_revision: 0,
    };
    renderer.upload(
        (0, 0),
        crate::mesh::build_with_settings(&world, (0, 0), assets, &settings),
    );
    let mut player = Player::new(world.spawn);
    player.set_rotation_degrees(180., 0.);
    let mut sim = Simulation::new(Mode::Survival, 189);
    sim.clock = 6000;
    let gui = Gui {
        screen: Screen::Playing,
        ..Default::default()
    };
    let hidden = Gui {
        screen: Screen::Playing,
        hide_hud: true,
        ..Default::default()
    };
    let path = std::env::temp_dir().join(format!("ferrum-hurt-camera-{}.png", std::process::id()));
    let empty = Frame::default();
    let mut capture = |state: &Simulation, gui: &Gui, frame: &Frame| {
        renderer
            .screenshot(
                &path,
                &RenderInput {
                    assets,
                    resources,
                    settings: &settings,
                    gui,
                    ui: frame,
                    selected_block: None,
                    world: Some(&world),
                    player: Some(&player),
                    simulation: Some(state),
                    alpha: 0.375,
                    time: 1.,
                },
            )
            .unwrap();
        image::open(&path).unwrap().to_rgb8()
    };
    let red_mask = |img: &RgbImage| {
        GrayImage::from_fn(width, height, |x, y| {
            let [r, g, b] = img.get_pixel(x, y).0.map(u16::from);
            Luma([u8::from(r > 40 && r > 2 * g && r > 2 * b)])
        })
    };
    let difference = |a: &RgbImage, b: &RgbImage| {
        GrayImage::from_fn(width, height, |x, y| {
            Luma([u8::from(a.get_pixel(x, y) != b.get_pixel(x, y))])
        })
    };
    for stack in [None, Some(Stack::new(278, 1))] {
        sim.inventory.slots[0] = stack.clone();
        for _ in 0..8 {
            sim.equipped.tick(stack.clone(), None);
        }
        let frame = ui::build(
            &gui,
            &settings,
            resources,
            assets,
            [width, height],
            Some(&sim),
            &Stats::default(),
        );
        let world_before = capture(&sim, &hidden, &empty);
        let hand_before = capture(&sim, &gui, &empty);
        let before = capture(&sim, &gui, &frame);
        sim.hurt_camera.hurt();
        sim.hurt_camera.tick(false);
        let world_peak = capture(&sim, &hidden, &empty);
        let hand_peak = capture(&sim, &gui, &empty);
        let peak = capture(&sim, &gui, &frame);
        check_roll(&red_mask(&world_before), &red_mask(&world_peak), "red wool");
        check_roll(
            &difference(&world_before, &hand_before),
            &difference(&world_peak, &hand_peak),
            if stack.is_some() {
                "diamond pickaxe"
            } else {
                "empty hand"
            },
        );
        let mut heart_pixels = 0;
        for y in height - 39..height - 30 {
            for x in width / 2 - 91..width / 2 - 10 {
                let pixel = before.get_pixel(x, y);
                if pixel[0] > 100 && pixel[1] < 45 && pixel[2] < 45 {
                    assert_eq!(pixel, peak.get_pixel(x, y), "heart at {x},{y}");
                    heart_pixels += 1;
                }
            }
        }
        assert!(
            heart_pixels > 100,
            "survival heart sprites missing ({heart_pixels} pixels)"
        );
        for _ in 0..9 {
            sim.hurt_camera.tick(false);
        }
        let recovered = capture(&sim, &gui, &frame);
        assert!(
            before == recovered,
            "expired animation changed the fixed-camera framebuffer"
        );
        if let (true, Some(dest)) = (
            stack.is_some(),
            std::env::var_os("FERRUM_HURT_CAMERA_CAPTURE"),
        ) {
            let mut strip = RgbImage::new(width * 3, height);
            for (i, img) in [&before, &peak, &recovered].into_iter().enumerate() {
                image::imageops::replace(&mut strip, img, i as i64 * i64::from(width), 0);
            }
            strip.save(dest).unwrap();
        }
    }
    std::fs::remove_file(path).unwrap();
}
