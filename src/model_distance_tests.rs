use super::*;

#[test]
fn basic_early_distance_is_conservative_for_both_ages_at_float_boundaries() {
    let eyes = [Vec3::ZERO, Vec3::new(-2048.25, 64.125, 4096.75)];
    for &kind in MobKind::ALL {
        let (adult_width, adult_height) = kind.size();
        for eye in eyes { for limit in [0f32, 1., 96.] {
            for x in [limit - 0.0001, limit, limit + 0.0001, -limit, 4096.] {
                for y in [-adult_height, 0., adult_height] {
                    let position = eye + Vec3::new(x, y, 0.);
                    let early = basic_distance_culled(position, adult_width, adult_height, eye, limit * limit);
                    let original = |child| {
                        let scale = if child { 0.5 } else { 1. };
                        let (width, height) = (adult_width * scale, adult_height * scale);
                        let min = position - Vec3::new(width / 2., 0., width / 2.);
                        let max = position + Vec3::new(width / 2., height, width / 2.);
                        ((min + max) / 2.).distance_squared(eye) > limit * limit
                    };
                    assert_eq!(early, original(false) && original(true), "{kind:?}, {position:?}, {eye:?}, {limit}");
                }
            }
        } }
    }
    assert!(!basic_distance_culled(Vec3::splat(f32::NAN), 0.6, 1.8, Vec3::ZERO, 9216.));
}

#[test]
fn disabled_model_profile_never_starts_a_clock_and_retains_zero_counts() {
    let p = build_profile::Profile::default();
    assert!(p.timer().is_none());
    assert_eq!(build_profile::elapsed(p.timer()), 0.);
    assert_eq!((p.native_mobs, p.native_early_distance, p.basic_early_distance), (0, 0, 0));
}

#[test]
#[ignore = "loads installed assets; paired original/early-distance CPU meshes, no GPU"]
fn early_distance_matches_original_meshes_and_counts_with_species_variants_and_interpolation() {
    use crate::{gameplay::Mode, vanilla::drops::DropNbt as Tag, world::{Chunk, ChunkMap}};
    use std::sync::Arc;
    let assets = crate::validation::assets();
    let resources = crate::validation::resources();
    let mut ground = Chunk::empty();
    for x in 0..16 { for z in 0..16 { ground.set(x, 63, z, 1 << 4); } }
    let ground = Arc::new(ground);
    let world = World {
        chunks: (-2..=2).flat_map(|x| (-2..=3).map(move |z| (x, z)))
            .map(|p| (p, ground.clone())).collect::<ChunkMap<_>>(),
        spawn: Vec3::new(0.5, 64., -8.), label: "distance projection".into(),
        save_path: Default::default(), edits: Default::default(), imported: false, light_revision: 0,
    };
    let player = Player::new(world.spawn);
    let eye = player.eye(0.5);
    let mut sim = Simulation::new(Mode::Creative, 189);
    sim.clock = 6000;
    for (i, &kind) in MobKind::ALL.iter().enumerate() {
        for far in [false, true] {
            let position = if far { eye + Vec3::new(400. + i as f32, 0., 400.) }
                else { Vec3::new((i % 8) as f32 * 4. - 14., 64., (i / 8) as f32 * 7. + 8.) };
            let id = sim.spawn_registered(kind, position, 0.3);
            let mob = sim.mobs.iter_mut().find(|m| m.id == id).unwrap();
            mob.previous = position - Vec3::X * 0.4;
            mob.hurt = 4;
            mob.age = 57;
            let brain = sim.mob_brains.entry(id).or_default();
            brain.growing_age = if i % 2 == 0 { -1200 } else { 0 };
            brain.gait.previous_amount = 0.2;
            brain.gait.amount = 0.7;
            brain.gait.swing = 2.3;
            brain.fire_ticks = 10;
            brain.fire_flag = true;
            let data = sim.command_entities.mobs.entry(id).or_default();
            data.extra.insert("IsBaby".into(), Tag::Byte((i % 2) as i8));
            data.extra.insert("Size".into(), Tag::Int(3));
            data.extra.insert("SkeletonType".into(), Tag::Int(1));
            data.extra.insert("IsVillager".into(), Tag::Byte(1));
            data.extra.insert("Elder".into(), Tag::Byte(1));
            data.extra.insert("Type".into(), Tag::Int(1));
            data.extra.insert("Variant".into(), Tag::Int(259));
            data.equipment.insert(99, Some(Stack::new(276, 1)));
        }
    }
    // Exercise native interpolation across the radius instead of rejecting on
    // current position alone; at alpha=1 this actor can become visible.
    let id = sim.spawn_registered(MobKind::Bat, eye + Vec3::Z * 8., 0.);
    sim.mobs.iter_mut().find(|m| m.id == id).unwrap().previous = eye + Vec3::Z * 200.;
    // Keep at least one zombie and skeleton on the original basic-model route.
    for kind in [MobKind::Zombie, MobKind::Skeleton] {
        let id = sim.spawn_registered(kind, eye + Vec3::Z * 12., 0.);
        sim.command_entities.mobs.remove(&id);
    }
    let before = serde_json::to_vec(&sim).unwrap();
    let frustum = Frustum::new(Mat4::perspective_rh(70f32.to_radians(), 16. / 9., 0.05, 256.)
        * Mat4::look_to_rh(eye, Vec3::Z, Vec3::Y));
    for entity_culling in [false, true] { for alpha in [0., 0.5, 1.] {
        let settings = Settings { entity_distance: 96, entity_culling, entity_shadows: true, ..Default::default() };
        let context = || ModelContext { sim: &sim, player: &player, world: &world, assets, resources,
            settings: &settings, frustum: &frustum, viewport_height: 1080, eye, alpha,
            perspective: 0, render_hand: false, hitboxes: true };
        let seeds = crate::vanilla::random::independent_seed_calls_for_thread();
        let expected = build_impl::<false>(context(), &mut build_profile::Profile::default());
        let mut profile = build_profile::Profile { enabled: true, ..Default::default() };
        let actual = build_impl::<true>(context(), &mut profile);
        assert_eq!(crate::vanilla::random::independent_seed_calls_for_thread(), seeds);
        assert_eq!((actual.visible, actual.total, actual.culled), (expected.visible, expected.total, expected.culled));
        assert!(actual.visible > 0, "fixture must draw visible actors");
        assert!(profile.native_early_distance > 0 && profile.basic_early_distance > 0);
        assert_eq!(profile.mobs, sim.mobs.len());
        for (a, b) in [(&actual.opaque, &expected.opaque), (&actual.translucent, &expected.translucent)] {
            assert!(bytemuck::cast_slice::<_, u8>(&a.vertices) == bytemuck::cast_slice::<_, u8>(&b.vertices),
                "vertex bytes differ at alpha={alpha}, occlusion={entity_culling}");
            assert_eq!(a.indices, b.indices);
            assert_eq!(a.visible_faces, b.visible_faces);
        }
    } }
    assert_eq!(serde_json::to_vec(&sim).unwrap(), before);
}
