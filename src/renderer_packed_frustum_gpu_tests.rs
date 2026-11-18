//! Actual Renderer scalar/packed oracle. Uses the ordinary post-acquire encode
//! path and production shaders/draws, including shared allocation offsets.
use super::*;

const KEYS: [ChunkPos; 4] = [(-1,0),(0,0),(-1,1),(0,1)];
fn geometry(renderer: &mut Renderer, id: usize, variant: u8, wide: bool) {
    // Keep the actual graph and section-owner metadata: unlike a graphless
    // panel, this must use View::mask and production prepare_ranges.
    renderer.upload(KEYS[id], panel(id as i32, variant, wide));
}
fn camera_graphs(renderer: &mut Renderer, wall: bool) {
    let mut chunk=crate::world::Chunk::empty();
    if wall { for y in 0..256 { for z in 0..16 { chunk.set(8,y,z,16); } } }
    for position in [(-1,3),(0,3)] {
        let mut mesh=models::empty();
        mesh.sections=Some(crate::mesh::visibility::TerrainSections::new(&chunk,true));
        renderer.upload(position,mesh);
    }
}
fn assert_actual_mode(renderer:&Renderer,world:&World,player:&Player,sim:&Simulation,
    settings:&Settings,packed:bool) {
    let hurt=HurtView::for_camera(sim,1.);
    let (_,_,_,frustum)=renderer.world_view(world,player,settings,0,1.,hurt,
        sim.equipped.bow_fov_multiplier(1.),sim.effects.get(9).is_some());
    assert_eq!(frustum.packed_for_test(),packed,"world_view must construct the selected actual Frustum");
    let report=renderer.packed_frustum_report();
    assert_eq!(report["enabled"],packed);
    assert_eq!(report["mode"],if packed {"packed-four-planes"} else {"scalar"});
    assert_eq!(renderer.pre_acquire_visibility_report(),serde_json::json!({"enabled":false,"mode":"after-acquire"}));
}

#[test]
#[ignore="requires original resources and Vulkan; exact actual scalar/packed Renderer pixels, masks and indices"]
fn packed_frustum_matches_scalar_renderer_masks_stereo_publication_and_screenshot() {
    let world=world(); let mut sim=Simulation::new(GameMode::Creative,189); sim.clock=6000;
    for choice in [Some(Choice::Original),Some(Choice::Mdi)] {
        let mut expected:Option<Vec<Image>>=None;
        let mut expected_screenshot=None;
        for packed in [false,true] {
            let mut settings=settings(); settings.render_distance=128;
            let mut player=player(); player.set_pose_f64([-1.2,65.,55.],180.,0.,true);
            let mut renderer=setup(choice,&settings,&world,&player,&sim);
            assert!(!renderer.packed_frustum_report()["enabled"].as_bool().unwrap());
            let policy=renderer.near_arena_report()["policy"].clone();
            renderer.configure_packed_frustum(packed);
            assert_eq!(renderer.near_arena_report()["policy"],policy,"candidate cannot change backing policy");
            assert_actual_mode(&renderer,&world,&player,&sim,&settings,packed);
            camera_graphs(&mut renderer,false);
            for id in 0..4 { geometry(&mut renderer,id,0,id==3); }
            if choice==Some(Choice::Mdi) {
                assert!(renderer.chunks.values().any(|c|c.gpu.arena_token.is_some()&&c.gpu.index_offset!=0));
            }
            let mut tickets=Vec::new();
            tickets.push(capture(&mut renderer,&settings,&world,&player,&sim));
            assert!(KEYS.iter().any(|p|renderer.chunks[p].mask!=0));
            tickets.push(capture(&mut renderer,&settings,&world,&player,&sim));
            player.set_pose_f64([2.,65.,55.],170.,0.,true);
            tickets.push(capture(&mut renderer,&settings,&world,&player,&sim));
            // Real section-owner filtering with an empty and a reduced mask;
            // both records and GPU index bytes remain in the comparison.
            renderer.chunks.get_mut(&KEYS[0]).unwrap().present=0;
            renderer.chunks.get_mut(&KEYS[1]).unwrap().present=1<<3;
            tickets.push(capture(&mut renderer,&settings,&world,&player,&sim));
            // A zero requested mask skips preparation; its cached chunk.mask
            // deliberately remains from the prior visible frame.
            assert_eq!(renderer.chunks[&KEYS[0]].present,0);
            assert_eq!(renderer.chunks[&KEYS[1]].mask & !(1<<3),0);
            for id in 0..2 { geometry(&mut renderer,id,1,id==1); }
            camera_graphs(&mut renderer,true);
            tickets.push(capture(&mut renderer,&settings,&world,&player,&sim));
            settings.anaglyph=true; renderer.apply_settings(&settings);
            tickets.push(capture(&mut renderer,&settings,&world,&player,&sim));
            renderer.resize(642,362);
            tickets.push(capture(&mut renderer,&settings,&world,&player,&sim));
            camera_graphs(&mut renderer,false);
            geometry(&mut renderer,2,2,false);
            player.set_pose_f64([-2.,65.,55.],180.,0.,true);
            tickets.push(capture(&mut renderer,&settings,&world,&player,&sim));
            let before_empty=tickets.len()-1;
            for key in KEYS { renderer.remove(key); }
            tickets.push(capture(&mut renderer,&settings,&world,&player,&sim));
            renderer.clear();
            assert_actual_mode(&renderer,&world,&player,&sim,&settings,packed);
            camera_graphs(&mut renderer,false);
            for id in 0..4 { geometry(&mut renderer,id,0,id==3); }
            tickets.push(capture(&mut renderer,&settings,&world,&player,&sim));
            assert_eq!(tickets.len(),10);
            // A real screenshot also uses world_view and the selected frustum.
            // All ten lifecycle readbacks are queued before its terminal Wait.
            let gui=Gui { screen:Screen::Playing,hide_hud:true,..Default::default() };
            let ui=Frame::default();
            let input=RenderInput { selected_block:None,assets:crate::validation::assets(),
                resources:crate::validation::resources(),settings:&settings,gui:&gui,ui:&ui,
                world:Some(&world),player:Some(&player),simulation:Some(&sim),alpha:1.,time:0. };
            let path=std::env::temp_dir().join(format!("ferrum-packed-frustum-{}-{}-{packed}.png",
                std::process::id(),if choice==Some(Choice::Mdi){"mdi"}else{"original"}));
            renderer.screenshot(&path,&input).unwrap();
            let screenshot=std::fs::read(&path).unwrap(); std::fs::remove_file(path).unwrap();
            if let Some(expected)=&expected_screenshot { assert_eq!(&screenshot,expected); }
            else { expected_screenshot=Some(screenshot); }
            let images=tickets.into_iter().map(|t|t.finish(&renderer.device)).collect::<Vec<_>>();
            assert!(images[before_empty].color.iter().zip(&images[before_empty+1].color)
                .filter(|(a,b)|a!=b).count()>50,"actual graph-filtered geometry must be visible");
            if let Some(expected)=&expected { assert_eq!(&images,expected); }
            else { expected=Some(images); }
            assert_actual_mode(&renderer,&world,&player,&sim,&settings,packed);
        }
    }
}
