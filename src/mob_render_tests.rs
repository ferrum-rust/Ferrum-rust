use super::*;
use crate::{
    gameplay::{MobKind, Mode, Simulation},
    player::Player,
    renderer::{RenderInput, Renderer},
    settings::Settings,
    ui::{Frame, Gui, Screen},
    world::{Chunk, World},
};
use std::sync::Arc;

#[path = "bat_render_tests.rs"]
mod bat;

#[test]
fn warmup_prepares_all_species_and_animation_rigs_once_before_drawing() {
    start_render_model_warmup();
    let first = warm_render_models();
    let again = warm_render_models();
    assert!(std::ptr::eq(first, again));
    assert_eq!(first.species, SPECIES.len());
    assert_eq!(first.animation_sets, POSES.len());
    assert_eq!(first.rigs, SPECIES.len() + 2);
    assert_eq!(first.geometry.item_models, 573);
    assert_eq!(first.geometry.player_models, 6);
    assert_eq!(first.geometry.vehicle_models, 2);
    let repeated = crate::models::warm_render_geometry();
    assert_eq!(repeated.item_models, first.geometry.item_models);
    assert_eq!(repeated.player_models, first.geometry.player_models);
    assert_eq!(repeated.vehicle_models, first.geometry.vehicle_models);
    for (id, species) in SPECIES.iter() {
        assert!(rigid::RIGS.contains_key(&(species.frames[0].as_ptr() as usize)),
            "species {id} must already have its rigid animation cache");
    }
    for id in [54, 55] {
        assert!(rigid::RIGS.contains_key(&(POSES[&id][0].as_ptr() as usize)));
    }
    assert_eq!(render_model_cache_report()["ready"], true);
}

#[test]
fn imported_mob_parts_fit_original_endpoints_and_keep_rigid_surfaces_between_them() {
    for (rest, label) in SPECIES
        .iter()
        .map(|(id, s)| (&s.frames[0], *id))
        .chain([54, 55].map(|id| (&POSES[&id][0], id)))
    {
        let rig = &rigid::RIGS[&(rest.as_ptr() as usize)];
        let frames: Vec<_> = rig.frames.values().collect();
        for frame in &frames {
            for (part, sample) in frame.iter().enumerate() {
                assert!(
                    sample.valid || !rig.parts[part].has_surface(),
                    "species {label}, part {part} must animate instead of falling back to rest (fit error {})", sample.error
                );
                assert!(
                    sample.error <= 0.002,
                    "species {label}, part {part}: fit error {}",
                    sample.error
                );
            }
        }
        for (from, to) in frames
            .iter()
            .zip(frames.iter().cycle().skip(1))
            .take(frames.len())
        {
            for (index, triangles) in rest.chunks(12).enumerate() {
                for fraction in [0.25, 0.5, 0.75] {
                    for amount in [0.2, 0.5, 1.] {
                        let matrix =
                            rig.parts[index].matrix(from[index], to[index], fraction, amount);
                        for tri in triangles {
                            let original = [0, 1, 2].map(|j| rigid::point(tri, j));
                            let shown = original.map(|p| matrix.transform_point3(p));
                            let area = |v: [Vec3; 3]| (v[1] - v[0]).cross(v[2] - v[0]).length();
                            assert!((area(shown) - area(original)).abs() <= area(original) * 0.0002 + 1e-5,
                                "species {label}, part {index} must not collapse at {fraction}/{amount}");
                            for edge in 0..3 {
                                let a = original[edge].distance(original[(edge + 1) % 3]);
                                let b = shown[edge].distance(shown[(edge + 1) % 3]);
                                assert!((a - b).abs() <= a * 0.0002 + 1e-5);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn mob_pose_interpolation_keeps_hinges_connected() {
    use glam::Quat;
    let start = rigid::Transform {
        center: Vec3::X,
        rotation: Quat::IDENTITY,
    };
    let end = rigid::Transform {
        center: Vec3::Y,
        rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
    };
    for t in [0., 0.1, 0.5, 0.9, 1.] {
        let pose = start.interpolate(end, t);
        let expected = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2 * t) * Vec3::X;
        assert!(pose.center.abs_diff_eq(expected, 1e-5));
        assert!((pose.center - pose.rotation * Vec3::X).length() < 1e-5);
        let opposite = start.interpolate(
            rigid::Transform {
                rotation: -end.rotation,
                ..end
            },
            t,
        );
        assert!(opposite.center.abs_diff_eq(pose.center, 1e-5));
    }
}

#[test]
fn original_registry_has_a_model_and_texture_for_every_native_mob() {
    assert_eq!(SPECIES.len(), MobKind::ALL.len());
    for &kind in MobKind::ALL {
        let id = kind.vanilla_id();
        let data = species(id).unwrap();
        assert!(
            data.frames.iter().all(|frame| !frame.is_empty()),
            "{kind:?}"
        );
        assert!(kind.texture().starts_with("textures/entity/"), "{kind:?}");
    }
    for &id in &[56, 60, 61, 63, 64, 65, 67, 68, 94, 101] {
        let frames = &POSES[&id];
        assert_eq!(frames.len(), if id == 61 { 80 } else { 8 });
        assert!(frames
            .iter()
            .all(|frame| frame.len() == SPECIES[&id].frames[0].len()));
        assert!(
            frames.iter().any(|frame| frame != &frames[0]),
            "motion for {id}"
        );
    }
    let villager = &POSES[&54];
    assert_eq!(villager.len(), 9);
    assert!(villager[0].len() > SPECIES[&54].frames[0].len()); // villager's nose
}

#[test]
fn local_variant_nbt_selects_original_skins_and_dimensions() {
    use crate::vanilla::drops::DropNbt as Tag;
    let mut sim = Simulation::new(Mode::Creative, 189);
    for (kind, key, value, texture, scale) in [
        (
            MobKind::Skeleton,
            "SkeletonType",
            1,
            "skeleton/wither_skeleton",
            1.2,
        ),
        (
            MobKind::Zombie,
            "IsVillager",
            1,
            "zombie/zombie_villager",
            1.,
        ),
        (MobKind::Guardian, "Elder", 1, "guardian_elder", 2.35),
        (MobKind::Horse, "Type", 1, "horse/donkey", 1.),
        (MobKind::Rabbit, "RabbitType", 99, "rabbit/caerbannog", 1.),
        (MobKind::Ocelot, "CatType", 2, "cat/red", 1.),
        (MobKind::Slime, "Size", 3, "slime/slime", 4.),
    ] {
        let index = sim.mobs.len();
        let id = sim.spawn_registered(kind, Vec3::ZERO, 0.);
        sim.mob_brains.entry(id).or_default().growing_age = 0;
        sim.command_entities
            .mobs
            .entry(id)
            .or_default()
            .extra
            .insert(key.into(), Tag::Int(value));
        let a = Appearance::local(&sim, &sim.mobs[index]);
        assert_eq!(
            a.texture(kind.vanilla_id()),
            format!("textures/entity/{texture}.png")
        );
        assert_eq!(a.scale(kind.vanilla_id()), scale);
    }
}

#[test]
#[ignore = "requires installed assets and a Vulkan device"]
fn vulkan_renders_each_native_mob_in_the_world() {
    let assets = crate::validation::assets();
    let resources = crate::validation::resources();
    let settings = Settings {
        entity_shadows: false,
        entity_culling: false,
        clouds: 0,
        render_distance: 4,
        ..Default::default()
    };
    let mut renderer =
        pollster::block_on(Renderer::new(None, assets, resources, &settings, None)).unwrap();
    let (w, h) = (256, 192);
    renderer.resize(w, h);
    let mut chunk = Chunk::empty();
    for z in 0..16 {
        for x in 0..16 {
            chunk.set(x, 63, z, 2 << 4);
        }
    }
    let chunk = Arc::new(chunk);
    let world = World {
        chunks: (-1..=1)
            .flat_map(|x| (-1..=1).map(move |z| (x, z)))
            .map(|pos| (pos, chunk.clone()))
            .collect(),
        spawn: Vec3::ZERO,
        label: String::new(),
        edits: Default::default(),
        save_path: Default::default(),
        imported: false,
        light_revision: 0,
    };
    for &pos in world.chunks.keys() {
        renderer.upload(
            pos,
            crate::mesh::build_with_settings(&world, pos, assets, &settings),
        );
    }
    let gui = Gui {
        screen: Screen::Playing,
        hide_hud: true,
        ..Default::default()
    };
    let path = std::env::temp_dir().join(format!("ferrum-native-mobs-{}.png", std::process::id()));
    let out = std::env::var_os("FERRUM_MOB_MODEL_CAPTURE");
    let mut sheet = out.as_ref().map(|_| image::RgbImage::new(w * 8, h * 4));
    let mut kinds = MobKind::ALL.to_vec();
    kinds.sort_by_key(|kind| kind.vanilla_id());
    for (i, kind) in kinds.into_iter().enumerate() {
        let mut sim = Simulation::new(Mode::Creative, 189);
        sim.clock = 6000;
        let above_ground = match kind {
            MobKind::Ghast => 4.,
            MobKind::Squid => 1.4,
            MobKind::EnderDragon => 3.,
            MobKind::Bat => 0.6,
            _ => 0.,
        };
        let feet = Vec3::new(8., 64. + above_ground, 8.);
        let baseline_sim = sim.clone();
        sim.spawn_registered(kind, feet, std::f32::consts::PI);
        let id = kind.vanilla_id();
        let scale = Appearance::local(&sim, &sim.mobs[0]).scale(id);
        let mut low = Vec3::splat(f32::INFINITY);
        let mut high = -low;
        for triangle in &SPECIES[&id].frames[0] {
            for vertex in triangle.chunks_exact(5) {
                let point = Vec3::new(vertex[0] as f32, vertex[1] as f32, vertex[2] as f32)
                    * (scale / 2048.);
                low = low.min(point);
                high = high.max(point);
            }
        }
        let padding = match kind {
            MobKind::Slime | MobKind::MagmaCube => 1.8,
            MobKind::EnderDragon => 0.8,
            _ => 1.35,
        };
        let radius = ((high - low).max_element() * padding).max(0.6);
        let center = (high + low) * 0.5 + Vec3::Y * if id == 94 { -0.7 } else { 0. };
        let mut p = Player::new(Vec3::ZERO);
        let (direction, yaw) = if kind == MobKind::EnderDragon {
            (Vec3::new(0.7660444, 0.5, -0.6427876), 50.)
        } else {
            (Vec3::new(0.5, 0.22, 0.8660254), 150.)
        };
        let eye = feet + center + direction * radius;
        p.set_pose_f64(
            (eye - Vec3::Y * 1.62).as_dvec3().to_array(),
            yaw,
            direction.y.atan().to_degrees(),
            false,
        );
        for _ in 0..40 {
            renderer.tick_lighting(&world, &p, &sim, settings.render_distance);
        }
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
                        player: Some(&p),
                        simulation: Some(sim),
                        alpha: 0.5,
                        time: 0.,
                    },
                )
                .unwrap();
            image::open(&path).unwrap().to_rgb8()
        };
        let baseline = capture(&baseline_sim);
        assert_ne!(
            resources.sprite(kind.texture()).layer,
            0,
            "missing skin: {kind:?}"
        );
        let actual = capture(&sim);
        let changed = actual
            .pixels()
            .zip(baseline.pixels())
            .filter(|(a, b)| a != b)
            .count();
        assert!(changed > 80, "{kind:?} only drew {changed} pixels");
        if let Some(image) = sheet.as_mut() {
            image::imageops::replace(
                image,
                &actual,
                (i as i64 % 8) * i64::from(w),
                (i as i64 / 8) * i64::from(h),
            );
        }
    }
    if let (Some(image), Some(out)) = (sheet, out) {
        image.save(out).unwrap();
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires installed assets and Vulkan temporal framebuffer readback"]
fn native_added_mob_surfaces_stay_stable_across_animation_cycles() {
    let assets = crate::validation::assets();
    let resources = crate::validation::resources();
    let settings = Settings {
        entity_shadows: false,
        entity_culling: true,
        clouds: 0,
        render_distance: 4,
        ..Default::default()
    };
    let mut renderer =
        pollster::block_on(Renderer::new(None, assets, resources, &settings, None)).unwrap();
    let (width, height) = (192, 160);
    renderer.resize(width, height);
    let mut ground = Chunk::empty();
    for x in 0..16 {
        for z in 0..16 {
            ground.set(x, 63, z, 2 << 4);
        }
    }
    let ground = Arc::new(ground);
    let world = World {
        chunks: (-2..=2)
            .flat_map(|x| (-2..=2).map(move |z| (x, z)))
            .map(|pos| (pos, ground.clone()))
            .collect(),
        spawn: Vec3::ZERO,
        label: "Native mob temporal fixture".into(),
        edits: Default::default(),
        save_path: Default::default(),
        imported: false,
        light_revision: 0,
    };
    for &pos in world.chunks.keys() {
        renderer.upload(
            pos,
            crate::mesh::build_with_settings(&world, pos, assets, &settings),
        );
    }
    let gui = Gui {
        screen: Screen::Playing,
        hide_hud: true,
        ..Default::default()
    };
    let path = std::env::temp_dir().join(format!("ferrum-mob-temporal-{}.png", std::process::id()));
    let mut kinds: Vec<_> = MobKind::ALL
        .iter()
        .copied()
        .filter(|kind| {
            !matches!(
                kind,
                MobKind::Cow
                    | MobKind::Pig
                    | MobKind::Sheep
                    | MobKind::Chicken
                    | MobKind::Zombie
                    | MobKind::Skeleton
                    | MobKind::Creeper
                    | MobKind::Spider
                    | MobKind::Villager
            )
        })
        .collect();
    kinds.sort_by_key(|kind| kind.vanilla_id());
    let mut sheet = image::RgbImage::new(width * 16, height * kinds.len() as u32);
    let mut failures = Vec::new();
    for (row, kind) in kinds.into_iter().enumerate() {
        let mut sim = Simulation::new(Mode::Creative, 189);
        sim.clock = 6000;
        sim.mob_spawning = false;
        let feet = Vec3::new(
            8.,
            64. + match kind {
                MobKind::Ghast => 4.,
                MobKind::Squid => 1.4,
                MobKind::EnderDragon => 3.,
                MobKind::Bat => 0.6,
                _ => 0.,
            },
            8.,
        );
        let baseline = sim.clone();
        let entity = sim.spawn_registered(kind, feet, std::f32::consts::PI);
        let id = kind.vanilla_id();
        let scale = Appearance::local(&sim, &sim.mobs[0]).scale(id);
        let mut low = Vec3::splat(f32::INFINITY);
        let mut high = -low;
        for frame in SPECIES[&id]
            .frames
            .iter()
            .chain(POSES.get(&id).into_iter().flatten())
        {
            for triangle in frame {
                for v in triangle.chunks_exact(5) {
                    let p = Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32) * (scale / 2048.);
                    low = low.min(p);
                    high = high.max(p);
                }
            }
        }
        let radius = ((high - low).max_element() * if id == 63 { 0.9 } else { 1.4 }).max(0.6);
        let center = (high + low) * 0.5 + Vec3::Y * if id == 94 { -0.7 } else { 0. };
        let (direction, yaw) = if id == 63 {
            (Vec3::new(0.7660444, 0.5, -0.6427876), 50.)
        } else {
            (Vec3::new(0.5, 0.22, 0.8660254), 150.)
        };
        let eye = feet + center + direction * radius;
        let mut player = Player::new(Vec3::ZERO);
        player.set_pose_f64(
            (eye - Vec3::Y * 1.62).as_dvec3().to_array(),
            yaw,
            direction.y.atan().to_degrees(),
            false,
        );
        for _ in 0..40 {
            renderer.tick_lighting(&world, &player, &sim, settings.render_distance);
        }
        let mut capture = |sim: &Simulation, alpha: f32| {
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
                        alpha,
                        time: 0.,
                    },
                )
                .unwrap();
            image::open(&path).unwrap().to_rgb8()
        };
        let empty = capture(&baseline, 0.);
        let cycle = match id {
            56 => std::f32::consts::TAU / 0.3,
            60 | 67 => std::f32::consts::TAU / 0.9,
            61 => 200.,
            63 => 20.,
            64 => std::f32::consts::TAU / 0.1,
            65 => std::f32::consts::TAU / 1.3,
            68 => std::f32::consts::TAU / 0.125,
            94 => std::f32::consts::TAU / 0.15,
            _ => 20.,
        };
        let mut min_pixels = usize::MAX;
        let mut max_pixels = 0;
        let mut worst_neighbor = 0_f32;
        let mut worst_substantial = 0_f32;
        let mut worst_silhouette = 0_f32;
        for phase in 0..16 {
            let mut pair = Vec::new();
            for offset in [0., 1. / 4096.] {
                let fraction = phase as f32 / 16. + offset;
                let age = fraction * cycle;
                let alpha = age.fract();
                sim.mobs[0].age = age as u32;
                let swing = fraction
                    * match id {
                        101 => 4.,
                        99 => 13.,
                        _ => std::f32::consts::TAU / 0.6662,
                    };
                sim.mob_brains.entry(entity).or_default().gait = crate::entity_visual::Gait {
                    previous_yaw: Some(std::f32::consts::PI),
                    previous_amount: 1.,
                    amount: 1.,
                    swing: swing + 1. - alpha,
                };
                pair.push(capture(&sim, alpha));
            }
            let shown = &pair[0];
            let occupied = shown
                .pixels()
                .zip(empty.pixels())
                .filter(|(a, b)| a != b)
                .count();
            let changed = shown
                .pixels()
                .zip(pair[1].pixels())
                .filter(|(a, b)| a != b)
                .count();
            let substantial = shown
                .pixels()
                .zip(pair[1].pixels())
                .filter(|(a, b)| a.0.iter().zip(b.0).any(|(a, b)| a.abs_diff(b) > 16))
                .count();
            let silhouette = shown
                .pixels()
                .zip(pair[1].pixels())
                .zip(empty.pixels())
                .filter(|((a, b), background)| (*a != *background) != (*b != *background))
                .count();
            min_pixels = min_pixels.min(occupied);
            max_pixels = max_pixels.max(occupied);
            worst_neighbor = worst_neighbor.max(changed as f32 / occupied.max(1) as f32);
            worst_substantial = worst_substantial.max(substantial as f32 / occupied.max(1) as f32);
            worst_silhouette = worst_silhouette.max(silhouette as f32 / occupied.max(1) as f32);
            image::imageops::replace(
                &mut sheet,
                shown,
                i64::from(phase * width),
                row as i64 * i64::from(height),
            );
        }
        eprintln!("{id} {kind:?}: pixels {min_pixels}..{max_pixels}, raw change {worst_neighbor:.3}, substantial {worst_substantial:.3}, silhouette {worst_silhouette:.3}");
        if min_pixels < 80 || worst_substantial > 0.08 || worst_silhouette > 0.08 {
            failures.push(format!(
                "{kind:?}: pixels={min_pixels}, substantial={worst_substantial:.3}, silhouette={worst_silhouette:.3}"
            ));
        }
    }
    if let Some(out) = std::env::var_os("FERRUM_MOB_TEMPORAL_CAPTURE") {
        sheet.save(out).unwrap();
    }
    std::fs::remove_file(path).unwrap();
    assert!(
        failures.is_empty(),
        "unstable mob surfaces: {}",
        failures.join("; ")
    );
}
