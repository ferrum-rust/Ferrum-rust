use super::*;
use crate::mesh::Frustum;

#[test]
#[ignore = "requires original bat texture and Vulkan"]
fn bat_wing_textures_remain_visible_from_both_sides_through_every_flap() {
    let assets = crate::validation::assets();
    let resources = crate::validation::resources();
    let settings = Settings { clouds: 0, entity_shadows: false, entity_culling: false,
        render_distance: 4, ..Default::default() };
    let world = World {
        chunks: [((0, 0), Arc::new(Chunk::empty()))].into(),
        spawn: Vec3::ZERO, label: "Bat wing culling fixture".into(),
        save_path: Default::default(), edits: Default::default(), imported: false, light_revision: 0,
    };
    let mut sim = Simulation::new(Mode::Creative, 189);
    sim.clock = 6000;
    let gui = Gui { screen: Screen::Playing, hide_hud: true, ..Default::default() };
    let mut renderer = pollster::block_on(Renderer::new(None, assets, resources, &settings, None)).unwrap();
    let (width, height) = (192, 160);
    renderer.resize(width, height);
    let path = std::env::temp_dir().join(format!("ferrum-bat-wings-{}.png", std::process::id()));
    let mut sheet = image::RgbImage::new(width * 16, height * 4);
    let mut lost = 0;
    for (view, (z, yaw)) in [(6.3, 0.), (9.7, 180.)].into_iter().enumerate() {
        let eye = Vec3::new(8., 64.35, z);
        let mut player = Player::new(eye - Vec3::Y * 1.62);
        player.set_rotation_degrees(yaw, 0.);
        let camera = crate::models::camera(&player, &world, &settings, 0, 1.);
        let frustum = Frustum::new(Mat4::perspective_rh(70_f32.to_radians(), width as f32/height as f32,
            0.05, 64.) * camera.view_at(camera.eye));
        let capture = |renderer: &mut Renderer| {
            renderer.screenshot(&path, &RenderInput {
                selected_block: None, assets, resources, settings: &settings, gui: &gui,
                ui: &Frame::default(), world: Some(&world), player: Some(&player),
                simulation: Some(&sim), alpha: 1., time: 0.,
            }).unwrap();
            image::open(&path).unwrap().to_rgb8()
        };
        renderer.upload((0,0), crate::models::empty());
        let background = capture(&mut renderer);
        for phase in 0..16 {
            let age = phase as f32 / 16. * std::f32::consts::TAU / 1.3;
            let mut counts = Vec::new();
            for regress in [false, true] {
                let mut dynamic = Dynamic { opaque: crate::models::empty(), translucent: crate::models::empty(),
                    nametags: crate::models::empty(), total: 0, visible: 0, culled: 0 };
                let context = ModelContext {
                    sim: &sim, player: &player, world: &world, assets, resources, settings: &settings,
                    frustum: &frustum, viewport_height: height, eye: camera.eye, alpha: 1.,
                    perspective: 0, render_hand: false, hitboxes: false,
                };
                draw_at(&mut dynamic, &context, &Actor {
                    species: 65, previous: Vec3::new(8.,64.,8.), feet: Vec3::new(8.,64.,8.),
                    yaw: 0., gait: Default::default(), appearance: Default::default(), hurt: false,
                    ..Default::default()
                }, false, age);
                assert_eq!(dynamic.visible, 1);
                let mesh = &mut dynamic.opaque;
                // ModelBat has five head/body boxes, followed by its four wing boxes.
                // Keep only wings so body pixels cannot hide a missing wing regression.
                mesh.indices = mesh.indices.chunks_exact(3).enumerate()
                    .filter(|(index, triangle)| triangle[0] >= 5 * 12 * 3 && (!regress || index % 2 == 0))
                    .flat_map(|(_, triangle)| triangle.iter().copied()).collect();
                renderer.upload((0,0), dynamic.opaque);
                let image = capture(&mut renderer);
                let occupied = image.pixels().zip(background.pixels()).filter(|(a,b)| a != b).count();
                counts.push(occupied);
                if !regress {
                    assert!(occupied > 60, "view {view} phase {phase}: both wing surfaces disappeared ({occupied})");
                }
                image::imageops::replace(&mut sheet, &image, i64::from(width)*phase,
                    i64::from(height)*(view as i64*2+i64::from(regress)));
            }
            lost += counts[0].saturating_sub(counts[1]);
        }
    }
    std::fs::remove_file(path).unwrap();
    assert!(lost > 500, "original transparent back UVs must expose the old culling failure: {lost}");
    eprintln!("Bat wing reverse faces restored {lost} pixels over front/back flap sequence");
    if let Some(out) = std::env::var_os("FERRUM_BAT_CAPTURE") { sheet.save(out).unwrap(); }
}
