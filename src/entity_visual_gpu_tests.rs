use crate::{
    gameplay::{Mob, MobKind, Mode, Simulation},
    player::Player,
    renderer::{RenderInput, Renderer},
    settings::Settings,
    ui::{Frame, Gui, Screen},
    world::{Chunk, World},
};
use glam::Vec3;
use image::RgbImage;
use std::sync::Arc;

#[test]
#[ignore = "uses installed 1.8.9 resources and a Vulkan adapter"]
fn vulkan_sheep_stride_hurt_skin_and_fire_billboards() {
    let assets = crate::validation::assets();
    let resources = crate::validation::resources();
    let settings = Settings {
        render_distance: 4,
        clouds: 0,
        entity_culling: false,
        ..Default::default()
    };
    let mut renderer =
        pollster::block_on(Renderer::new(None, assets, resources, &settings, None)).unwrap();
    let (w, h) = (448, 320);
    renderer.resize(w, h);
    let mut chunk = Chunk::empty();
    for x in 0..16 {
        for z in 0..16 {
            chunk.set(x, 63, z, 2 << 4);
        }
    }
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
    player.set_rotation_degrees(180., 9.);
    let mut sim = Simulation::new(Mode::Survival, 189);
    sim.clock = 6000;
    let feet = Vec3::new(8.5, 64., 9.2);
    sim.mobs.push(Mob {
        id: 17,
        kind: MobKind::Sheep,
        position: feet,
        previous: feet,
        velocity: Vec3::ZERO,
        yaw: std::f32::consts::FRAC_PI_2,
        health: 8.,
        age: 20,
        turn_ticks: 0,
        hurt: 0,
        fuse: 0,
        swing_ticks: 0,
        death_ticks: 0,
        eat_ticks: 0,
        attack_ticks: 0,
        rose_ticks: 0,
        prev_fuse: 0,
    });
    sim.mob_brains.entry(17).or_default().gait = super::Gait {
        previous_yaw: None,
        previous_amount: 1.,
        amount: 1.,
        swing: 3.,
    };
    let path =
        std::env::temp_dir().join(format!("ferrum-entity-visual-{}.png", std::process::id()));
    let mut capture = |sim: &Simulation, perspective: u8, partial: f32| {
        let gui = Gui {
            hide_hud: true,
            screen: Screen::Playing,
            perspective,
            ..Default::default()
        };
        renderer
            .screenshot(
                &path,
                &RenderInput {
                    assets,
                    resources,
                    settings: &settings,
                    gui: &gui,
                    ui: &Frame::default(),
                    selected_block: None,
                    world: Some(&world),
                    player: Some(&player),
                    simulation: Some(sim),
                    alpha: partial,
                    time: 1.,
                },
            )
            .unwrap();
        image::open(&path).unwrap().to_rgb8()
    };
    let changed =
        |a: &RgbImage, b: &RgbImage| a.pixels().zip(b.pixels()).filter(|(p, q)| p != q).count();
    let redder = |a: &RgbImage, b: &RgbImage| {
        a.pixels()
            .zip(b.pixels())
            .filter(|(p, q)| {
                q[0] >= p[0]
                    && i16::from(q[1]) + 8 < i16::from(p[1])
                    && i16::from(q[2]) + 8 < i16::from(p[2])
            })
            .count()
    };
    let start = capture(&sim, 0, 0.);
    let middle = capture(&sim, 0, 0.5);
    let end = capture(&sim, 0, 1.);
    assert!(
        changed(&start, &middle) > 40 && changed(&middle, &end) > 40,
        "sheep legs require separate sub-tick poses"
    );
    sim.mobs[0].hurt = 10;
    let hurt = capture(&sim, 0, 0.5);
    assert!(
        redder(&middle, &hurt) > 250,
        "visible wool and skin must flash red"
    );
    sim.mobs[0].hurt = 0;
    sim.mob_brains.get_mut(&17).unwrap().fire_flag = true;
    let burning_sheep = capture(&sim, 0, 0.5);
    let orange = |img: &RgbImage| {
        img.pixels()
            .filter(|p| p[0] > 170 && p[1] > 45 && p[2] < p[1] / 2)
            .count()
    };
    assert!(
        orange(&burning_sheep) > orange(&middle) + 150,
        "world-facing fire layers must be visible"
    );
    sim.mob_brains.get_mut(&17).unwrap().fire_flag = false;
    sim.fire_flag = true;
    let first_person = capture(&sim, 0, 0.5);
    assert!(
        orange(&first_person) > orange(&middle) + 1000,
        "fire overlay must also be visible with hidden HUD"
    );
    sim.fire_flag = false;
    sim.mobs.clear();
    let steve = capture(&sim, 2, 0.);
    sim.hurt_camera.hurt();
    let steve_hurt = capture(&sim, 2, 0.);
    assert!(
        redder(&steve, &steve_hurt) > 250,
        "local F5 skin must flash red"
    );
    if let Some(dest) = std::env::var_os("FERRUM_ENTITY_VISUAL_CAPTURE") {
        let mut strip = RgbImage::new(w * 3, h * 2);
        for (i, img) in [
            &middle,
            &hurt,
            &burning_sheep,
            &steve,
            &steve_hurt,
            &first_person,
        ]
        .into_iter()
        .enumerate()
        {
            image::imageops::replace(
                &mut strip,
                img,
                (i % 3) as i64 * i64::from(w),
                (i / 3) as i64 * i64::from(h),
            );
        }
        strip.save(dest).unwrap();
    }
    std::fs::remove_file(path).unwrap();
}
