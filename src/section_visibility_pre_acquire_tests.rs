//! Child of dense_tests: the expected history uses the retained original AoS
//! traversal and NEVER prepares a failed acquisition. The candidate uses the
//! actual View and the production pre-acquire cancellation function.
use super::*;

#[derive(Clone, Copy)]
struct Pose { eye: Vec3, direction: Vec3, radius: u8 }
impl Pose {
    fn frame(self) -> (Mat4, Frustum) {
        let matrix = Mat4::perspective_rh(1.1, 1.3, 0.05, 160.)
            * Mat4::look_to_rh(self.eye, self.direction, Vec3::Y);
        (matrix, Frustum::new(matrix))
    }
}
fn pose(x: f32, y: f32, z: f32, direction: Vec3, radius: u8) -> Pose {
    Pose { eye: Vec3::new(x, y, z), direction, radius }
}
fn update_actual(view: &mut View, scene: &Scene, p: Pose, revision: u8) -> usize {
    let (matrix, frustum) = p.frame();
    let calls = std::cell::Cell::new(0);
    // Fixed world origin: moving the camera must not silently change graphs.
    view.update(matrix, &frustum, p.eye, p.direction, p.radius, |key| {
        calls.set(calls.get() + 1); scene.graph(key, (0, 0), revision)
    });
    calls.get()
}
fn success(actual: &mut View, expected: &mut OriginalView,
    scene: &Scene, p: Pose, revision: u8) {
    update_actual(actual, scene, p, revision);
    let (matrix, frustum) = p.frame();
    expected.update(matrix, &frustum, p.eye, p.direction, p.radius,
        |key| scene.original_graph(key, (0, 0), revision));
    let block = p.eye.floor().as_ivec3();
    let center = (block.x.div_euclid(16), block.z.div_euclid(16));
    let r = i32::from(p.radius) + 1;
    let mut columns = Vec::new();
    for z in -r..=r { for x in -r..=r { columns.push((center.0+x, center.1+z)); } }
    columns.extend(expected.reached.keys().copied());
    columns.extend([(i32::MIN, i32::MAX), (i32::MAX, i32::MIN)]);
    for column in columns {
        for present in [0, 1, 0x8000, 0x5555, 0xaaaa, u16::MAX] {
            assert_eq!(actual.mask(column, &frustum, present),
                expected.mask(column, &frustum, present),
                "next successful mask differs: {column:?}, revision={revision}");
        }
        for y in 0..16 {
            let section = IVec3::new(column.0, y, column.1);
            assert_eq!(actual.order(section), expected.order(section),
                "next successful boss order differs: {section:?}");
        }
    }
    assert_eq!(actual.frustum_only, expected.frustum_only);
    assert_eq!(actual.render_order, expected.render_order);
    // Inactive camera_exits/pending/seen may retain different historical cache
    // contents after an early return. They are not renderer-visible output;
    // later successes below prove their continuation remains equivalent.
}
fn failed(actual: &mut View, scene: &Scene, p: Pose, revision: u8) {
    update_actual(actual, scene, p, revision);
    crate::renderer::pre_acquire_visibility::cancel_view(actual);
    assert!(actual.key.is_none());
    assert!(actual.camera_exits.is_none());
}

#[test]
fn failed_pre_acquire_views_preserve_original_next_success_and_boss_order_history() {
    let scene = Scene::new();
    let start = pose(7.2, 8.5, 8.5, Vec3::X, 4);
    for revision in 0..=4 {
        let (mut actual, mut expected) = (View::default(), OriginalView::default());
        actual.track_order(true); expected.track_order(true);
        success(&mut actual, &mut expected, &scene, start, revision);
        // Valid old cache, different failed cell/angle, then retry the exact old
        // camera: expected can reuse its old cache; candidate must recompute.
        failed(&mut actual, &scene, pose(16.25, 23.5, -0.25, Vec3::NEG_Z, 4), revision);
        failed(&mut actual, &scene, pose(-0.25, 8.5, 16.25, Vec3::Z, 1), revision);
        success(&mut actual, &mut expected, &scene, start, revision);
        for tracking in [false, true] {
            // This GUI/boss setting changes even on a failed render attempt,
            // so BOTH histories receive it. Only candidate prepares the view.
            actual.track_order(tracking); expected.track_order(tracking);
            failed(&mut actual, &scene, pose(7.8, 8.5, 8.8, Vec3::NEG_X, 4), revision);
            for p in [pose(7.4, 8.5, 8.6, Vec3::NEG_Z, 4),
                pose(8.1, 8.5, 8.6, Vec3::X, 4),
                pose(16.1, 24.1, -0.1, Vec3::Z, 1), start] {
                success(&mut actual, &mut expected, &scene, p, revision);
            }
        }
    }
}

#[test]
fn failed_pre_acquire_then_graph_publication_preserves_missing_solid_and_camera_exit_cases() {
    let scene = Scene::new();
    let start = pose(7.2, 8.5, 8.5, Vec3::X, 4);
    let (mut actual, mut expected) = (View::default(), OriginalView::default());
    actual.track_order(true); expected.track_order(true);
    let mut revision = 0;
    success(&mut actual, &mut expected, &scene, start, revision);
    for next in [4, 1, 2, 3, 0, 4, 0] {
        failed(&mut actual, &scene, pose(8.2, 8.5, 8.5, Vec3::NEG_X, 4), revision);
        // Real mesh publication/removal invalidates both histories, including
        // an unchanged camera cell and a camera graph that was previously absent.
        actual.invalidate(); expected.invalidate(); revision = next;
        success(&mut actual, &mut expected, &scene, start, revision);
        failed(&mut actual, &scene, pose(7.4, 8.5, 8.6, Vec3::Z, 4), revision);
        success(&mut actual, &mut expected, &scene, start, revision);
    }
    for p in [pose(8.5, -0.1, 8.5, Vec3::Z, 1),
        pose(8.5, 256., 8.5, Vec3::NEG_Z, 1),
        pose(-16.01, 255.9, 16., Vec3::X, 4),
        pose(8.5, f32::NAN, 8.5, Vec3::Z, 1), start] {
        failed(&mut actual, &scene, p, revision);
        success(&mut actual, &mut expected, &scene, p, revision);
        success(&mut actual, &mut expected, &scene, start, revision);
    }
}

#[test]
fn cold_failed_pre_acquire_and_repeated_cancellation_recover_without_a_prior_valid_cache() {
    let scene = Scene::new();
    for revision in 0..=4 {
        let (mut actual, mut expected) = (View::default(), OriginalView::default());
        actual.track_order(true); expected.track_order(true);
        failed(&mut actual, &scene, pose(8.5, 8.5, 8.5, Vec3::NEG_Z, 4), revision);
        crate::renderer::pre_acquire_visibility::cancel_view(&mut actual);
        for p in [pose(7.2, 8.5, 8.5, Vec3::X, 4),
            pose(7.3, 8.5, 8.5, Vec3::NEG_X, 4),
            pose(-0.01, 64., -16.01, Vec3::Z, 16)] {
            success(&mut actual, &mut expected, &scene, p, revision);
        }
    }
}
