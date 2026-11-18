use super::*;
use crate::{
    gameplay::{Mode, Simulation},
    player::Player,
    renderer::{RenderInput, Renderer},
    settings::Settings,
    ui::{Frame, Gui, Screen},
    world::{Chunk, World},
};
use std::sync::Arc;

#[test]
fn fishing_line_keeps_one_physical_pixel_under_perspective() {
    for height in [256, 1080, 2160] {
        let width = height as f32 * 16. / 9.;
        for fov in [30_f32, 70., 110.] {
            for roll in [0_f32, 0.35] {
                let view = Mat4::from_rotation_z(roll)
                    * Mat4::look_at_rh(Vec3::new(3., 7., 2.), Vec3::new(0., 6., 10.), Vec3::Y);
                let matrix = Mat4::perspective_rh(fov.to_radians(), 16. / 9., 0.05, 256.) * view;
                let frustum = Frustum::new(matrix);
                let world = view.inverse();
                let screen = |p: Vec3| {
                    let q = matrix * p.extend(1.);
                    glam::Vec2::new(q.x * width, q.y * height as f32) / (2. * q.w)
                };
                for distance in [1_f32, 10., 20., 90.] {
                    let a = world.transform_point3(Vec3::new(-0.4, 0.1, -1.) * distance);
                    let b = world.transform_point3(Vec3::new(0.3, -0.1, -1.6) * distance);
                    let quad = frustum.pixel_line(a, b, height).unwrap().map(screen);
                    let tangent = (screen(b) - screen(a)).normalize();
                    for edge in [quad[1] - quad[0], quad[2] - quad[3]] {
                        assert!((edge.length() - 1.).abs() < 0.01, "width {edge:?}");
                        assert!(
                            edge.dot(tangent).abs() < 0.01,
                            "offset must be perpendicular"
                        );
                    }
                    assert!((quad[1] - quad[0]).perp_dot(quad[2] - quad[0]) > 0.);
                }
                let behind = world.transform_point3(Vec3::new(-0.1, 0., 0.1));
                let ahead = world.transform_point3(Vec3::new(0.1, 0., -3.));
                let clipped = frustum.pixel_line(behind, ahead, height).unwrap();
                for p in clipped {
                    let clip = matrix * p.extend(1.);
                    assert!(clip.z >= -1e-5 && clip.w > 0., "must clip at near plane");
                }
                assert!(frustum
                    .pixel_line(behind, behind + Vec3::Y * 0.01, height)
                    .is_none());
            }
        }
    }
}

#[test]
fn fireball_billboard_faces_camera_without_backface_culling() {
    let world = World {
        chunks: Default::default(),
        spawn: Vec3::new(0., 64., 0.),
        label: String::new(),
        save_path: Default::default(),
        edits: Default::default(),
        imported: false,
        light_revision: 0,
    };
    let settings = Settings {
        bobbing: false,
        ..Default::default()
    };
    let mut player = Player::new(Vec3::new(0., 64., 0.));
    let proj_mat = Mat4::perspective_rh(70_f32.to_radians(), 16. / 9., 0.05, 256.);
    for perspective in [0_u8, 1, 2] {
        for yaw in [-180_f32, -135., -90., -45., 0., 37.5, 90., 135., 179.9] {
            for pitch in [-89_f32, -60., -25., 0., 25., 60., 89.] {
                player.set_rotation_degrees(yaw, pitch);
                assert_eq!(player.java_rotation(), [yaw, pitch]);
                let view = camera(&player, &world, &settings, perspective, 1.);
                let matrix = proj_mat * view.view_at(view.eye);
                let pos = view.eye + view.direction * 6.;
                let rot = billboard_rotation(player.java_rotation(), perspective);
                let q = [
                    Vec3::new(-0.5, -0.25, 0.),
                    Vec3::new(0.5, -0.25, 0.),
                    Vec3::new(0.5, 0.75, 0.),
                    Vec3::new(-0.5, 0.75, 0.),
                ]
                .map(|v| pos + rot.transform_vector3(v * 2.));
                let normal = (q[1] - q[0]).cross(q[2] - q[0]).normalize();
                assert!(
                    normal.dot(-view.direction) > 0.9999,
                    "perspective={perspective} yaw={yaw} pitch={pitch} dot={}",
                    normal.dot(-view.direction)
                );
                let ndc = q.map(|p| {
                    let c = matrix * p.extend(1.);
                    assert!(c.w > 0.);
                    glam::Vec2::new(c.x / c.w, c.y / c.w)
                });
                let area = (ndc[1] - ndc[0]).perp_dot(ndc[2] - ndc[0]);
                assert!(
                    area > 0.,
                    "fireball quad was backface-culled: perspective={perspective} yaw={yaw} pitch={pitch} area={area}"
                );
                assert!(
                    ndc[2].y > ndc[1].y,
                    "fireball quad upside-down: perspective={perspective} yaw={yaw} pitch={pitch}"
                );
            }
        }
    }
}

#[test]
#[ignore = "uses installed resources and Vulkan framebuffer readback"]
fn native_throwables_bow_and_fishing_draw_original_textures_and_visible_pixels() {
    let assets = crate::validation::assets();
    let resources = crate::validation::resources();
    let mut ground = Chunk::empty();
    for x in 0..16 {
        for z in 0..16 {
            ground.set(x, 63, z, 2 << 4);
        }
    }
    let ground = Arc::new(ground);
    let mut world = World {
        chunks: (-1..=2)
            .flat_map(|x| (-1..=2).map(move |z| (x, z)))
            .map(|p| (p, ground.clone()))
            .collect(),
        spawn: Vec3::new(8., 64., 4.),
        label: "Native projectile render fixture".into(),
        save_path: Default::default(),
        edits: Default::default(),
        imported: false,
        light_revision: 0,
    };
    let settings = Settings {
        clouds: 0,
        entity_shadows: false,
        render_distance: 3,
        ..Default::default()
    };
    let mut renderer =
        pollster::block_on(Renderer::new(None, assets, resources, &settings, None)).unwrap();
    let (width, height) = (384, 256);
    renderer.resize(width, height);
    for &pos in world.chunks.keys() {
        renderer.upload(
            pos,
            crate::mesh::build_with_settings(&world, pos, assets, &settings),
        );
    }
    let path = std::env::temp_dir().join(format!(
        "ferrum-native-projectile-{}.png",
        std::process::id()
    ));
    let mut sheet = image::RgbImage::new(width * 3, height * 3);
    let mut gui = Gui {
        screen: Screen::Playing,
        hide_hud: true,
        ..Default::default()
    };
    for (index, (item, texture)) in [
        (261, "textures/entity/arrow.png"),
        (332, "textures/items/snowball.png"),
        (344, "textures/items/egg.png"),
        (368, "textures/items/ender_pearl.png"),
        (381, "textures/items/ender_eye.png"),
        (346, "textures/particle/particles.png"),
        (384, "textures/items/experience_bottle.png"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut sim = Simulation::new(Mode::Survival, 189);
        sim.mob_spawning = false;
        sim.clock = 6000;
        sim.inventory.slots[0] = Some(Stack::new(item, 16));
        sim.inventory.slots[1] = Some(Stack::new(262, 16));
        let mut player = Player::new(world.spawn);
        player.set_rotation_degrees(0., -10.);
        let hold = if item == 261 { 20 } else { 1 };
        for _ in 0..hold {
            sim.tick(&mut world, &mut player, resources, &settings, false, true);
        }
        sim.tick(&mut world, &mut player, resources, &settings, false, false);
        if matches!(item, 381 | 346) {
            for _ in 0..10 {
                sim.tick(&mut world, &mut player, resources, &settings, false, false);
            }
        }
        let position = match item {
            381 => sim
                .fishing
                .eyes
                .first()
                .expect("normal use must launch an Eye")
                .position
                .as_vec3(),
            346 => sim
                .fishing
                .hook
                .as_ref()
                .expect("normal use must cast the bobber")
                .position
                .as_vec3(),
            _ => sim
                .projectiles
                .first()
                .expect("normal use must launch a projectile")
                .position
                .as_vec3(),
        };
        if item == 346 {
            assert!(
                position.distance(player.position) > 10.,
                "line must remain visible beyond 10 blocks"
            );
            player.set_rotation_degrees(0., 0.);
            // Keep the caster's viewpoint to include the original rod and its line.
            for _ in 0..8 {
                sim.equipped.tick(sim.inventory.held(), None);
            }
            gui.hide_hud = false;
        } else {
            let eye = position + Vec3::new(1., 0.35, -1.3);
            let look = position - eye;
            let yaw = (-look.x).atan2(look.z).to_degrees();
            let pitch = (-look.y).atan2(look.x.hypot(look.z)).to_degrees();
            player.set_pose_f64(
                (eye - Vec3::Y * 1.62).as_dvec3().to_array(),
                yaw,
                pitch,
                false,
            );
        }
        for _ in 0..40 {
            renderer.tick_lighting(&world, &player, &sim, 3);
        }
        assert_ne!(
            resources.sprite(texture).layer,
            0,
            "missing original {texture}"
        );
        let mut baseline = sim.clone();
        baseline.projectiles.clear();
        baseline.fishing.eyes.clear();
        baseline.fishing.hook = None;
        let mut capture = |sim: &Simulation| {
            renderer
                .screenshot(
                    &path,
                    &RenderInput {
                        selected_block: None,
                        assets,
                        resources,
                        settings: &settings,
                        gui: &gui,
                        ui: &Frame::default(),
                        world: Some(&world),
                        player: Some(&player),
                        simulation: Some(sim),
                        alpha: 1.,
                        time: 0.,
                    },
                )
                .unwrap();
            image::open(&path).unwrap().to_rgb8()
        };
        let empty = capture(&baseline);
        let shown = capture(&sim);
        image::imageops::replace(
            &mut sheet,
            &shown,
            (index as i64 % 3) * i64::from(width),
            (index as i64 / 3) * i64::from(height),
        );
        if let Some(out) = std::env::var_os("FERRUM_PROJECTILE_CAPTURE") {
            sheet.save(out).unwrap();
        }
        let changed = shown
            .pixels()
            .zip(empty.pixels())
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            changed > 40,
            "item {item} drew only {changed} changed pixels"
        );
        if item == 346 {
            let line_pixels = shown
                .enumerate_pixels()
                .filter(|(x, y, pixel)| {
                    *x < width * 3 / 4
                        && *y < height * 3 / 4
                        && pixel.0 == [0; 3]
                        && empty.get_pixel(*x, *y).0 != [0; 3]
                })
                .count();
            assert!(
                line_pixels > 8,
                "fishing line drew only {line_pixels} black pixels"
            );
            let view = camera(&player, &world, &settings, 0, 1.);
            let matrix = Mat4::perspective_rh(
                settings.fov.to_radians(),
                width as f32 / height as f32,
                0.05,
                128.,
            ) * view.view_at(view.eye);
            let anchor = matrix * (position + Vec3::Y * 0.25).extend(1.);
            let start = (((anchor.x / anchor.w + 1.) * width as f32 * 0.5).ceil() as u32) + 2;
            let missing_columns = (start..width * 3 / 4)
                .filter(|x| {
                    !(0..height * 3 / 4).any(|y| {
                        shown.get_pixel(*x, y).0 == [0; 3] && empty.get_pixel(*x, y).0 != [0; 3]
                    })
                })
                .count();
            assert!(
                missing_columns <= 1,
                "fishing line has {missing_columns} empty columns"
            );
            let bobber_pixels = shown
                .enumerate_pixels()
                .filter(|(x, y, pixel)| {
                    *x < width * 3 / 4
                        && *y < height * 3 / 4
                        && pixel.0 != [0; 3]
                        && **pixel != *empty.get_pixel(*x, *y)
                })
                .count();
            assert!(
                bobber_pixels > 4,
                "bobber texture drew only {bobber_pixels} colored pixels"
            );
            // Pixel differences in the hand's corner prove the cast rod variant reached the renderer.
            let held_pixels = shown
                .enumerate_pixels()
                .filter(|(x, y, pixel)| {
                    *x > width * 2 / 3 && *y > height / 2 && **pixel != *empty.get_pixel(*x, *y)
                })
                .count();
            assert!(
                held_pixels > 20,
                "cast rod changed only {held_pixels} held pixels"
            );
            assert_ne!(
                resources
                    .sprite("textures/items/fishing_rod_cast.png")
                    .layer,
                0
            );
        }
    }
    std::fs::remove_file(path).unwrap();
}
