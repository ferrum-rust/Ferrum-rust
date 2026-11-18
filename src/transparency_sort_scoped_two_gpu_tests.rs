//! Focused real-renderer oracle for the optional two-helper scheduler.
//! Reuses the established glass builder/readback; this is not a timing fixture.
use super::*;

#[test]
#[ignore="requires original block resources and Vulkan pixel/index readback; two actual helpers"]
fn scoped_two_matches_original_pixels_and_gpu_indices_with_two_helpers() {
    let assets=crate::validation::assets();let resources=crate::validation::resources();
    let mut world=glass_world();
    // Nine disconnected layers produce3456 blend quads per slot,110592 total.
    // Full quad counts remain above98304 when section membership is reduced;
    // each of the32 jobs still contains both visible membership bits.
    for x in (0..16).step_by(2) {for z in (0..16).step_by(2) {for y in (50..=60).step_by(2) {
        world.set(IVec3::new(x,y,z),95<<4|((x+z+y)%15) as u16);
    }}}
    let settings=Settings{render_distance:4,clouds:0,entity_shadows:false,..Default::default()};
    let template=mesh::build_with_settings(&world,(0,0),assets,&settings);
    let total_quads=template.quads()*DRAW_SLOTS;
    assert_eq!(DRAW_SLOTS,32);
    assert_eq!(total_quads,110592);
    assert!(total_quads>=98304&&total_quads<=1048576);
    assert!(template.vertices.len()<=u16::MAX as usize);
    assert!(template.vertices.iter().all(|v|v.layer&crate::models::BLEND!=0));
    let mut sim=Simulation::new(GameMode::Creative,189);sim.clock=6000;
    let gui=Gui{screen:Screen::Playing,hide_hud:true,..Default::default()};let ui=UiFrame::default();
    let path=TempImage(std::env::temp_dir().join(format!("ferrum-two-helper-sort-gpu-{}-{}.png",
        std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())));
    let mut reference:Option<Vec<(image::RgbaImage,Indices)>>=None;
    for mode in [Mode::Original,Mode::ScopedTwo] {
        let mut renderer=pollster::block_on(Renderer::new(None,assets,resources,&settings,None)).unwrap();
        renderer.resize(320,180);renderer.configure_transparency_sort(mode);
        renderer.transparency_sort.ensure_helper_for_test();
        assert_eq!(renderer.transparency_sort.helper_threads(),if mode==Mode::ScopedTwo{2}else{0},
            "the candidate must own two real worker threads, not merely select a mode name");
        let mut player=Player::new(Vec3::new(8.,70.,29.));player.set_rotation_degrees(180.,20.);
        for _ in 0..40 {renderer.tick_lighting(&world,&player,&sim,settings.render_distance);}
        upload_scene(&mut renderer,&template,0);
        assert_eq!(renderer.chunks.values().filter(|c|c.gpu.index_format==wgpu::IndexFormat::Uint16).count(),16);
        assert_eq!(renderer.chunks.values().filter(|c|c.gpu.index_format==wgpu::IndexFormat::Uint32).count(),16);
        let mut frames:Vec<(image::RgbaImage,Indices)>=Vec::new();
        for phase in 0..5 {
            match phase {
                0|1=>{}, // first sort, then identical camera with no sort admission
                2=>player.set_pose_f64([11.,70.,29.],180.,20.,false),
                3=>{for chunk in renderer.chunks.values_mut(){chunk.present=1;}},
                4=>{for chunk in renderer.chunks.values_mut(){chunk.present=3;}},
                _=>unreachable!(),
            }
            let before=renderer.transparency_sort.successful_runs();
            let input=RenderInput{selected_block:None,assets,resources,settings:&settings,
                gui:&gui,ui:&ui,world:Some(&world),player:Some(&player),simulation:Some(&sim),alpha:1.,time:0.};
            renderer.screenshot(&path.0,&input).unwrap();
            assert_eq!(renderer.visible_chunks,DRAW_SLOTS,"every threshold-forcing job must actually reach the renderer");
            let admitted=mode==Mode::ScopedTwo&&phase!=1;
            assert_eq!(renderer.transparency_sort.successful_runs()-before,usize::from(admitted),
                "real two-helper admission/unchanged skip differs in {mode:?} phase{phase}");
            assert_eq!(renderer.transparency_sort.helper_threads(),if mode==Mode::ScopedTwo{2}else{0});
            let pixels=image::open(&path.0).unwrap().to_rgba8();
            let indices=read_and_check_gpu_indices(&renderer);
            let expected_quads=if phase==3{total_quads/2}else{total_quads};
            assert_eq!(indices.iter().map(|(_,v)|v.len()/6).sum::<usize>(),expected_quads,
                "mask changes must affect real uploaded u16/u32 prefixes");
            if phase==1 {
                assert_eq!(pixels,frames[0].0,"unchanged eye must preserve pixels");
                assert_eq!(indices,frames[0].1,"unchanged eye must preserve GPU indices");
            }
            if phase==4 {
                assert_eq!(pixels,frames[2].0,"restoring membership must restore the moved-camera pixels");
                assert_eq!(indices,frames[2].1,"restoring membership must restore the exact moved-camera GPU order");
            }
            if let Some(reference)=&reference {
                assert_eq!(pixels,reference[phase].0,"real Vulkan pixels differ from Original in {mode:?} phase{phase}");
                assert_eq!(indices,reference[phase].1,"actual narrow/wide GPU indices differ from Original in {mode:?} phase{phase}");
            }
            frames.push((pixels,indices));
        }
        assert_eq!(renderer.transparency_sort.successful_runs(),if mode==Mode::ScopedTwo{4}else{0});
        assert_ne!(frames[0].0,frames[2].0,"moving camera must change the visible projection");
        assert_ne!(frames[0].1,frames[2].1,"moving camera must change actual index order");
        assert_ne!(frames[2].1,frames[3].1,"partial membership must change actual GPU indices");
        let positions=renderer.chunks.keys().copied().collect::<Vec<_>>();
        for p in positions {renderer.remove(p);}
        let input=RenderInput{selected_block:None,assets,resources,settings:&settings,
            gui:&gui,ui:&ui,world:Some(&world),player:Some(&player),simulation:Some(&sim),alpha:1.,time:0.};
        renderer.screenshot(&path.0,&input).unwrap();
        let empty=image::open(&path.0).unwrap().to_rgba8();
        let visible=frames[4].0.pixels().zip(empty.pixels()).filter(|(a,b)|
            (0..3).any(|c|a[c].abs_diff(b[c])>8)).count();
        assert!(visible>500,"comparison must contain rendered glass, not only sky/fog ({visible} changed pixels)");
        eprintln!("Two-helper Vulkan oracle {mode:?}: {total_quads} quads in{DRAW_SLOTS} jobs,{} real helpers,{visible} visible pixels; exact mixed-width GPU indices",
            renderer.transparency_sort.helper_threads());
        if reference.is_none(){reference=Some(frames);}
    }
}
