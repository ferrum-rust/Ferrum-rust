use super::*;

#[test]
fn resident_slot_groups_preserve_the_original_visible_subsequence() {
    let order=(0..129).map(|i|((i*43)%137,100-i)).collect::<Vec<_>>();
    for mask in 0u16..256 {
        let visible=order.iter().enumerate().filter(|(i,_)|mask&(1<<(i%8))!=0)
            .map(|(i,p)|(slot(i),*p)).collect::<Vec<_>>();
        let expected=order.iter().enumerate().filter(|(i,_)|mask&(1<<(i%8))!=0)
            .map(|(_,p)|*p).collect::<Vec<_>>();
        assert!(visible.windows(2).all(|p|p[0].0<=p[1].0));
        let mut grouped=Vec::new();
        for group in 0..order.len().div_ceil(GROUP_SIZE) {
            let values=visible.iter().filter(|(g,_)|*g==group).map(|(_,p)|*p).collect::<Vec<_>>();
            assert!(values.len()<=GROUP_SIZE); grouped.extend(values);
        }
        assert_eq!(grouped,expected);
    }
}

#[test]
#[ignore = "requires installed assets and Vulkan readback; exact far bundle/direct images"]
fn grouped_far_cache_is_bounded_and_invalidates_payload_binding_eye_and_renderer_changes() {
    use crate::{gameplay::{Mode,Simulation}, models, player::Player, renderer::{Renderer,RenderInput},
        settings::Settings, ui::{Frame,Gui,Screen}, world::{Chunk,World}};
    use glam::Vec3;
    use std::sync::Arc;
    let assets = crate::validation::assets();
    let resources = crate::validation::resources();
    let mut settings = Settings { render_distance:128, clouds:0, entity_shadows:false, ..Default::default() };
    let mut renderer = pollster::block_on(Renderer::new_with_arena(None,assets,resources,&settings,None,Some(crate::renderer::NearArenaMode::Original))).unwrap();
    renderer.resize(480,270);
    let mut world = World { chunks:Default::default(), spawn:Vec3::ZERO, label:String::new(),
        save_path:Default::default(), edits:Default::default(), imported:false, light_revision:0 };
    let mut player = Player::new(Vec3::new(0.,83.,90.));
    player.set_rotation_degrees(180.,20.);
    let mut sim = Simulation::new(Mode::Creative,189);
    sim.clock = 6000;
    // A visible hand and sky exercise binding restoration when no near bundle
    // exists, which was the dangerous case for independent far commands.
    let gui = Gui { screen:Screen::Playing, ..Default::default() };
    for _ in 0..40 { renderer.tick_lighting(&world,&player,&sim,24); }
    let path = std::env::temp_dir().join(format!("ferrum-far-bundle-{}-{}.png",std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let capture = |renderer:&mut Renderer, settings:&Settings, world:&World, camera:&Player| {
        renderer.screenshot(&path,&RenderInput { selected_block:None,assets,resources,settings,
            gui:&gui,ui:&Frame::default(),world:Some(world),player:Some(camera),simulation:Some(&sim),
            alpha:1.,time:0. }).unwrap();
        image::open(&path).unwrap().to_rgba8()
    };
    let plane = |i: i32, variant: u8| {
        let mut mesh = models::empty();
        let (column,row) = (i % 9,i / 9);
        let x = (column - 4) as f32 * 8.;
        let z = 16. + row as f32 * (32. / 9.);
        mesh.min = Vec3::new(x,64.,z);
        mesh.max = Vec3::new(x+7.,64.,z+3.5);
        for z in [z,z+1.75] {
            models::face(&mut mesh,[Vec3::new(x,64.,z),Vec3::new(x,64.,z+1.75),
                Vec3::new(x+7.,64.,z+1.75),Vec3::new(x+7.,64.,z)], [0.;4],
                models::SOLID | crate::renderer::distant_render::DISTANT,
                [40 + column as u8 * 20, 180 - variant * 30, 80 + row as u8 * 10 + variant * 20, 255]);
        }
        mesh
    };
    const COUNT:i32=81;
    let cached_position=|renderer:&Renderer,p| renderer.distant.bundles.slots.iter().flatten()
        .any(|c|c.key.tiles.iter().any(|(at,_)|*at==p));
    for i in 0..COUNT { renderer.upload_distant((i,0),plane(i,0)); }
    renderer.distant.disable_bundles = true;
    let direct = capture(&mut renderer,&settings,&world,&player);
    assert_eq!(renderer.distant.visible,COUNT as usize,"all capped-build probes must be visible");
    assert!(renderer.distant.bundles.slots.iter().all(Option::is_none));
    renderer.distant.disable_bundles = false;
    let mixed = capture(&mut renderer,&settings,&world,&player);
    assert_eq!(mixed,direct,"first frame mixes direct and cached tiles in original order");
    assert_eq!(renderer.distant.bundles.slots.iter().filter(|c|c.is_some()).count(),BUILDS_PER_FRAME);
    for _ in 0..3 { assert_eq!(capture(&mut renderer,&settings,&world,&player),direct); }
    assert!(renderer.distant.bundles.slots.iter().all(Option::is_some));

    // The referenced payload can change without either GPU capacity changing.
    let old_capacity = { let t=&renderer.distant.tiles[&(0,0)]; (t.gpu.vertex_capacity,t.gpu.index_capacity) };
    let old_revision = renderer.distant.tiles[&(0,0)].revision;
    let mut fewer = plane(0,1); fewer.indices.truncate(6);
    renderer.upload_distant((0,0),fewer);
    let tile=&renderer.distant.tiles[&(0,0)];
    assert!(!cached_position(&renderer,(0,0)));
    assert_ne!(tile.revision,old_revision);
    assert_eq!((tile.gpu.vertex_capacity,tile.gpu.index_capacity),old_capacity);
    let paired = |renderer:&mut Renderer,settings:&Settings,world:&World| {
        renderer.distant.disable_bundles = true;
        let expected = capture(renderer,settings,world,&player);
        renderer.distant.disable_bundles = false;
        assert_eq!(capture(renderer,settings,world,&player),expected);
        expected
    };
    let changed = paired(&mut renderer,&settings,&world);
    assert_ne!(changed,direct,"changed count/color payload must alter visible pixels");

    // Buffer growth and the u16 -> u32 index transition retain exact geometry.
    let mut wide = plane(1,2);
    let prefix = vec![wide.vertices[0];65536];
    wide.vertices.splice(0..0,prefix);
    for i in &mut wide.indices { *i += 65536; }
    renderer.upload_distant((1,0),wide);
    assert!(!cached_position(&renderer,(1,0)));
    assert_eq!(renderer.distant.tiles[&(1,0)].gpu.index_format,wgpu::IndexFormat::Uint32);
    paired(&mut renderer,&settings,&world);
    let removed_revision=renderer.distant.tiles[&(2,0)].revision;
    renderer.remove_distant((2,0));
    assert!(!renderer.distant.tiles.contains_key(&(2,0)));
    renderer.upload_distant((2,0),plane(2,1));
    assert_ne!(renderer.distant.tiles[&(2,0)].revision,removed_revision);
    assert!(!cached_position(&renderer,(2,0)));
    paired(&mut renderer,&settings,&world);

    // Near uploads/removals must leave otherwise valid far cache entries alone.
    let keys=renderer.distant.bundles.slots.iter().map(|c|c.as_ref().map(|c|c.key.clone())).collect::<Vec<_>>();
    let mut chunk=Chunk::empty(); chunk.set(0,63,0,16);
    world.chunks.insert((0,0),Arc::new(chunk));
    renderer.upload((0,0),crate::mesh::build_with_settings(&world,(0,0),assets,&settings));
    assert_eq!(renderer.distant.bundles.slots.iter().map(|c|c.as_ref().map(|c|c.key.clone())).collect::<Vec<_>>(),keys);
    paired(&mut renderer,&settings,&world);
    renderer.remove((0,0));
    paired(&mut renderer,&settings,&world);

    for (anaglyph,mipmaps) in [(true,0),(false,4),(true,4),(false,0)] {
        let old_epoch=renderer.distant.binding_revision;
        let old_mipmaps=settings.mipmaps;
        settings.anaglyph=anaglyph; settings.mipmaps=mipmaps;
        renderer.apply_settings(&settings);
        if old_mipmaps!=mipmaps {
            assert_ne!(renderer.distant.binding_revision,old_epoch);
            assert!(renderer.distant.bundles.slots.iter().all(Option::is_none));
        }
        paired(&mut renderer,&settings,&world);
        for _ in 0..3 { capture(&mut renderer,&settings,&world,&player); }
        assert!(renderer.distant.bundles.slots.iter().all(|c|c.as_ref().unwrap().eyes.len()==if anaglyph {2}else{1}));
    }
    renderer.resize(512,288);
    paired(&mut renderer,&settings,&world);

    // Resource-pack replacement constructs a new Renderer with new bindings;
    // none of the previous renderer's cached device resources may transfer.
    let mut replacement=pollster::block_on(Renderer::new_with_arena(None,assets,resources,&settings,None,Some(crate::renderer::NearArenaMode::Original))).unwrap();
    replacement.resize(480,270);
    assert!(replacement.distant.tiles.is_empty());
    replacement.upload_distant((0,0),plane(4,0));
    assert!(!cached_position(&replacement,(0,0)));
    paired(&mut replacement,&settings,&world);

    // Frustum changes produce exact ordered subsets within each stable group.
    let mut other_camera=Player::new(Vec3::new(30.,83.,90.));
    other_camera.set_rotation_degrees(200.,20.);
    renderer.distant.disable_bundles=true;
    let expected=capture(&mut renderer,&settings,&world,&other_camera);
    assert!((1..COUNT as usize).contains(&renderer.distant.visible),
        "moved camera must exercise a nonempty strict visible subset");
    renderer.distant.disable_bundles=false;
    assert_eq!(capture(&mut renderer,&settings,&world,&other_camera),expected);
    paired(&mut renderer,&settings,&world);
    // Invisible residents still participate in group slots. Force table growth
    // and index shifts, then compare immediately while rebuilds are capped.
    for i in 0..128 {renderer.upload_distant((1000+i,0),models::empty());}
    paired(&mut renderer,&settings,&world);
    assert!(renderer.distant.bundles.slots.len()<=renderer.distant.tiles.len().div_ceil(GROUP_SIZE));

    // Rule out comparing two invisible frames: remove all far geometry only.
    renderer.distant.disable_bundles=true;
    let with_far=capture(&mut renderer,&settings,&world,&player);
    for i in 0..COUNT { renderer.remove_distant((i,0)); }
    let without_far=capture(&mut renderer,&settings,&world,&player);
    let pixels=with_far.pixels().zip(without_far.pixels()).filter(|(a,b)|a!=b).count();
    assert!(pixels>1000,"far probe must render visible terrain: {pixels}");
    std::fs::remove_file(path).unwrap();
}
