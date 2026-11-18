//! Actual generated LOD output oracle. Both arms retain the same near geometry,
//! shader, camera mask, settings and ordered far submissions except proven skips.
use super::*;
use crate::{distant_terrain::{Surface,TILE_CHUNKS}, gameplay::{Mode,Simulation},
    player::Player, renderer::{render_profile,RenderInput,NearArenaMode,DEPTH},
    settings::Settings, ui::{Frame,Gui,Screen}, world::World};

#[derive(Debug,PartialEq,Eq)]
struct Pixels { color:Vec<[u8;4]>, depth:Vec<u32> }
struct Shot { buffer:wgpu::Buffer, width:u32, height:u32, row:u32, plane:u64 }
fn read_all(device:&wgpu::Device,shots:Vec<Shot>)->Vec<Pixels> {
    let receivers=shots.iter().map(|shot| {
        let(tx,rx)=std::sync::mpsc::sync_channel(1);
        shot.buffer.slice(..).map_async(wgpu::MapMode::Read,move |result|{let _=tx.send(result);});rx
    }).collect::<Vec<_>>();
    device.poll(wgpu::Maintain::Wait);
    shots.into_iter().zip(receivers).map(|(shot,rx)| {
        rx.recv().unwrap().unwrap();let data=shot.buffer.slice(..).get_mapped_range();
        let mut image=Pixels {color:Vec::new(),depth:Vec::new()};
        for y in 0..shot.height as usize {
            let at=y*shot.row as usize;
            image.color.extend(data[at..at+shot.width as usize*4].chunks_exact(4).map(|v|<[u8;4]>::try_from(v).unwrap()));
            let at=at+shot.plane as usize;
            image.depth.extend(data[at..at+shot.width as usize*4].chunks_exact(4)
                .map(|v|u32::from_le_bytes(v.try_into().unwrap())));
        }
        drop(data);shot.buffer.unmap();image
    }).collect()
}
fn world()->World { World {chunks:Default::default(),spawn:Vec3::ZERO,label:String::new(),
    save_path:Default::default(),edits:Default::default(),imported:false,light_revision:0} }
fn settings()->Settings {Settings {render_distance:128,clouds:0,entity_shadows:false,
    particles:0,..Default::default()} }
fn setup(enabled:bool,s:&Settings,choice:Option<NearArenaMode>)->Renderer {
    let mut renderer=pollster::block_on(Renderer::new_with_arena(None,crate::validation::assets(),
        crate::validation::resources(),s,None,choice)).unwrap();
    renderer.resize(320,180);renderer.configure_covered_lod_culling(enabled);
    if choice.is_none() {
        assert_eq!(renderer.near_arena_report()["effective"],"ArenaMdi");
        renderer.arena_test_limits(crate::renderer::near_arena::Limits {
            vertex_page:4*1024*1024,index_page:64*1024,owners:128});
    }
    assert_eq!(renderer.covered_lod_culling_report()["enabled"],enabled);
    renderer
}
fn tile(position:TilePos,height:u16)->TerrainTile {
    TerrainTile {position,heights:(0..GRID*GRID).map(|i|Surface {
        height:height+((i/GRID/4+i%GRID/4)%3) as u16,
        state:if i%GRID/4%2==0 {2<<4}else{1<<4},biome:1,
    }).collect()}
}
fn camera(position:TilePos,halo:bool)->Player {
    let base=Vec3::new((position.0*TILE_BLOCKS) as f32,0.,(position.1*TILE_BLOCKS) as f32);
    let feet=base+Vec3::new(if halo {-280.}else{64.},140.,200.);
    let mut player=Player::new(feet);
    let direction=(base+Vec3::new(64.,65.,64.)-player.eye(1.)).normalize();
    player.set_rotation_degrees((-direction.x).atan2(direction.z).to_degrees(),-direction.y.asin().to_degrees());
    assert!(player.direction().dot(direction)>0.999);
    player
}
fn chunk_min(position:TilePos)->(i32,i32) {(position.0*TILE_CHUNKS,position.1*TILE_CHUNKS)}
fn hole(position:TilePos)->(i32,i32) {let p=chunk_min(position);(p.0+3,p.1+4)}
fn anchor_mask(renderer:&mut Renderer,player:&Player) {
    let p=(player.eye(1.)/16.).floor().as_ivec3();
    renderer.set_distant_detail_bounds((p.x-24,p.z-24),(p.x+25,p.z+25));
}
fn fill_near(renderer:&mut Renderer,position:TilePos) {
    let p=chunk_min(position);
    for z in 0..8 {for x in 0..8 {renderer.upload((p.0+x,p.1+z),models::empty());}}
    // A real, visible close opaque surface remains after all far fragments are
    // hidden. Its position belongs to its uploaded owner; hole() is elsewhere.
    let a=Vec3::new((position.0*TILE_BLOCKS) as f32,96.,(position.1*TILE_BLOCKS) as f32);
    let mut mesh=models::empty();
    models::face(&mut mesh,[a,a+Vec3::Z*16.,a+Vec3::new(16.,0.,16.),a+Vec3::X*16.],
        [0.;4],models::SOLID|models::UNLIT,[45,205,70,255]);
    mesh.min=a;mesh.max=a+Vec3::new(16.,0.,16.);
    renderer.upload(p,mesh);
}
fn prepare_case(renderer:&mut Renderer,s:&Settings,world:&World,sim:&Simulation,position:TilePos,cached:bool)->Player {
    renderer.clear();renderer.apply_settings(s);renderer.distant.disable_bundles=!cached;
    renderer.torch_flicker=Default::default();renderer.visual_random=crate::vanilla::random::JavaRandom::new(189);
    let player=camera(position,false);anchor_mask(renderer,&player);fill_near(renderer,position);
    for _ in 0..40 {renderer.tick_lighting(world,&player,sim,s.render_distance);}
    player
}
fn upload_generated(renderer:&mut Renderer,position:TilePos,stride:u8,height:u16,proof:bool) {
    renderer.upload_distant(position,build_with_stride(&tile(position,height),crate::validation::assets(),stride));
    assert_eq!(renderer.distant.tiles[&position].coverage_proof.is_some(),proof);
}
// The expected visibility is declared independently by each fixture phase. It
// is checked against both actual issued calls and the candidate census.
fn capture(renderer:&mut Renderer,s:&Settings,world:&World,sim:&Simulation,player:&Player,
    frame:usize,enabled:bool,has_tile:bool,culled:bool,unproven:bool)->Shot {
    let before=renderer.covered_lod_culling_report();
    let omitted=if culled {renderer.distant.tiles.values().map(|t|u64::from(t.gpu.count)).sum()}else{0};
    let [width,height]=renderer.resolution();let extent=wgpu::Extent3d {width,height,depth_or_array_layers:1};
    let make_texture=|format|renderer.device.create_texture(&wgpu::TextureDescriptor {label:None,
        size:extent,mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format,
        usage:wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::COPY_SRC,view_formats:&[]});
    let color=make_texture(renderer.config.format);let depth=make_texture(DEPTH);
    renderer.depth=depth.create_view(&Default::default());
    let row=(width*4).div_ceil(256)*256;let plane=u64::from(row)*u64::from(height);
    let buffer=renderer.device.create_buffer(&wgpu::BufferDescriptor {label:Some("covered LOD queued RGBA/depth"),
        size:plane*2,usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
    let gui=Gui {screen:Screen::Playing,hide_hud:true,..Default::default()};
    let ui=Frame {size:[width as f32,height as f32],..Default::default()};
    let input=RenderInput {selected_block:None,assets:crate::validation::assets(),resources:crate::validation::resources(),
        settings:s,gui:&gui,ui:&ui,world:Some(world),player:Some(player),simulation:Some(sim),alpha:1.,time:0.};
    let mut profile=render_profile::Frame {enabled:true,frame_id:frame as u64,..Default::default()};
    let encoder=renderer.device.create_command_encoder(&Default::default());
    renderer.submit_arena_test_frame(encoder,&color.create_view(&Default::default()),&input,&mut profile,|encoder,_| {
        for (texture,offset,aspect) in [(&color,0,wgpu::TextureAspect::All),(&depth,plane,wgpu::TextureAspect::DepthOnly)] {
            encoder.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo {texture,mip_level:0,origin:wgpu::Origin3d::ZERO,aspect},
                wgpu::TexelCopyBufferInfo {buffer:&buffer,layout:wgpu::TexelCopyBufferLayout {
                    offset,bytes_per_row:Some(row),rows_per_image:Some(height)}},extent);
        }
    });
    let visible=usize::from(has_tile&&!culled);let eyes=if s.anaglyph {2}else{1};
    assert_eq!(renderer.distant.visible,visible,"frame{frame}: actual ordered visible list");
    assert_eq!(profile.counters.far_direct_draws+profile.counters.far_bundle_draws,visible*eyes,
        "frame{frame}: covered tiles must issue zero GPU draws, including both eyes");
    if !renderer.distant.disable_bundles {
        assert_eq!(profile.counters.far_bundle_draws,visible*eyes,"single visible group must use real cached bundles");
    }else{assert_eq!(profile.counters.far_bundle_draws,0);}
    if enabled {
        let after=renderer.covered_lod_culling_report();
        for (name,delta) in [("measured_frames",1),("visible_candidates_before",u64::from(has_tile)),
            ("visible_culled",u64::from(culled)),("indices_omitted",omitted),("unproven_kept",u64::from(unproven&&has_tile))] {
            assert_eq!(after["counts"][name].as_u64().unwrap()-before["counts"][name].as_u64().unwrap(),delta,
                "frame{frame}: {name}");
        }
    }
    Shot {buffer,width,height,row,plane}
}
fn difference(a:&Pixels,b:&Pixels)->usize {
    a.color.iter().zip(&b.color).zip(a.depth.iter().zip(&b.depth))
        .filter(|((a,b),(da,db))|a!=b||da!=db).count()
}

#[test]
#[ignore="requires original assets and Vulkan; exact generated LOD coverage/color/depth and cached/direct paths"]
fn covered_lod_tiles_match_original_generated_strides_holes_revisions_and_stereo() {
    let world=world();let mut sim=Simulation::new(Mode::Creative,189);sim.clock=6000;
    for choice in [Some(NearArenaMode::Original),None] {
    let mut expected=None;
    for enabled in [false,true] {
        let mut s=settings();let mut renderer=setup(enabled,&s,choice);let mut shots=Vec::new();let mut witnesses=Vec::new();
        for position in [(0,0),(-2,-3)] {for stride in [4,8,16] {for cached in [false,true] {
            s.anaglyph=stride==8;
            let player=prepare_case(&mut renderer,&s,&world,&sim,position,cached);
            if choice.is_none() {
                assert_eq!(renderer.near_arena_report()["effective"],"ArenaMdi");
                let close=&renderer.chunks[&chunk_min(position)].gpu;
                assert!(close.count>0&&close.arena_token.is_some(),"real close geometry must use shared arena backing");
            }
            upload_generated(&mut renderer,position,stride,64,enabled);
            let covered=shots.len();shots.push(capture(&mut renderer,&s,&world,&sim,&player,covered,enabled,true,enabled,false));
            renderer.remove(hole(position));
            let partial=shots.len();shots.push(capture(&mut renderer,&s,&world,&sim,&player,partial,enabled,true,false,false));
            renderer.upload(hole(position),models::empty());
            let old_revision=renderer.distant.tiles[&position].revision;
            upload_generated(&mut renderer,position,stride,80,enabled);
            assert_ne!(renderer.distant.tiles[&position].revision,old_revision);
            let replaced=shots.len();shots.push(capture(&mut renderer,&s,&world,&sim,&player,replaced,enabled,true,enabled,false));
            renderer.remove(hole(position));renderer.remove_distant(position);
            let empty=shots.len();shots.push(capture(&mut renderer,&s,&world,&sim,&player,empty,enabled,false,false,false));
            witnesses.push((covered,partial,replaced,empty));
        }}}
        // All changing submissions and output copies precede this first wait.
        let images=read_all(&renderer.device,shots);assert_eq!(images.len(),48);
        for (covered,partial,replaced,empty) in witnesses {
            assert_eq!(images[covered],images[replaced],"covered replacement cannot change pixels or depth");
            assert_eq!(images[covered],images[empty],"covered far is already a complete shader no-op");
            assert!(difference(&images[partial],&images[empty])>16,"the one-chunk hole must expose real far geometry");
            assert!(images[covered].depth.iter().filter(|&&v|f32::from_bits(v)<1.).count()>8,"close geometry remains visible");
        }
        if let Some(original)=&expected {assert_eq!(&images,original);}else{expected=Some(images);}
    }
    }
}

#[test]
#[ignore="requires original assets and Vulkan; conservative unknown/outside/halo/world-limit fallback"]
fn covered_lod_unknown_outside_halo_world_edges_and_clear_preserve_original_output() {
    let world=world();let mut sim=Simulation::new(Mode::Creative,189);sim.clock=6000;
    let mut expected=None;
    for enabled in [false,true] {
        let s=settings();let mut renderer=setup(enabled,&s,Some(NearArenaMode::Original));let mut shots=Vec::new();let mut visible_witnesses=Vec::new();
        for cached in [false,true] {
            let position=(0,0);let player=prepare_case(&mut renderer,&s,&world,&sim,position,cached);
            upload_generated(&mut renderer,position,8,64,enabled);
            let covered=shots.len();shots.push(capture(&mut renderer,&s,&world,&sim,&player,covered,enabled,true,enabled,false));
            renderer.set_distant_detail_bounds((100,100),(149,149));
            let outside=shots.len();shots.push(capture(&mut renderer,&s,&world,&sim,&player,outside,enabled,true,false,false));
            assert!(renderer.distant_mask((4,12),24).1.iter().flatten().all(|&v|v==0));
            let halo=camera(position,true);anchor_mask(&mut renderer,&halo);
            let halo_frame=shots.len();shots.push(capture(&mut renderer,&s,&world,&sim,&halo,halo_frame,enabled,true,false,false));
            let halo_mask=renderer.distant_mask((-18,12),24).1;
            assert_eq!(halo_mask.iter().flatten().map(|v|v.count_ones()).sum::<u32>(),56,
                "uploaded column7 lies exactly one beyond the actual full-detail radius");
            anchor_mask(&mut renderer,&player);
            // Same key/capacity can replace a proven payload with arbitrary
            // geometry. It ignores the near-mask shader branch and MUST stay.
            let mut unflagged=build_with_stride(&tile(position,64),crate::validation::assets(),8);
            for v in &mut unflagged.vertices {v.layer&=!DISTANT;}
            renderer.upload_distant(position,unflagged);
            assert!(renderer.distant.tiles[&position].coverage_proof.is_none());
            let unknown=shots.len();shots.push(capture(&mut renderer,&s,&world,&sim,&player,unknown,enabled,true,false,enabled));
            // All flags are otherwise real, but the flat owner of the last
            // chunk column lies outside this nominal tile. Key-only proof fails.
            let mut foreign=build_with_stride(&tile(position,64),crate::validation::assets(),8);
            for v in &mut foreign.vertices {v.light[2]=v.light[2].checked_add(16).unwrap();}
            renderer.upload_distant(position,foreign);
            assert!(renderer.distant.tiles[&position].coverage_proof.is_none());
            let foreign_frame=shots.len();shots.push(capture(&mut renderer,&s,&world,&sim,&player,foreign_frame,enabled,true,false,enabled));
            upload_generated(&mut renderer,position,8,64,enabled);
            let restored=shots.len();shots.push(capture(&mut renderer,&s,&world,&sim,&player,restored,enabled,true,enabled,false));
            assert_ne!(unknown,restored);visible_witnesses.push((outside,covered));visible_witnesses.push((unknown,covered));
            visible_witnesses.push((foreign_frame,covered));
            // clear drops every old proof/payload and cache. The configured mode
            // remains active; a fresh ordinary upload obtains a fresh proof.
            let previous=renderer.covered_lod_culling_report();
            let again=prepare_case(&mut renderer,&s,&world,&sim,position,cached);
            assert!(renderer.distant.tiles.is_empty());
            if enabled {assert_eq!(renderer.covered_lod_culling_report()["counts"],previous["counts"]);}
            upload_generated(&mut renderer,position,16,64,enabled);
            let index=shots.len();shots.push(capture(&mut renderer,&s,&world,&sim,&again,index,enabled,true,enabled,false));
        }
        // The exact-arithmetic provenance guard intentionally rejects these
        // original-world-edge coordinates. They remain drawn even if all64
        // shader owners are covered; this is conservative, not lost LOD.
        for (position,stride,cached) in [((-234375,0),4,true),((234374,0),16,false)] {
            let player=prepare_case(&mut renderer,&s,&world,&sim,position,cached);
            upload_generated(&mut renderer,position,stride,64,false);
            let full=shots.len();shots.push(capture(&mut renderer,&s,&world,&sim,&player,full,enabled,true,false,enabled));
            renderer.remove(hole(position));
            let partial=shots.len();shots.push(capture(&mut renderer,&s,&world,&sim,&player,partial,enabled,true,false,enabled));
            renderer.remove_distant(position);
            let empty=shots.len();shots.push(capture(&mut renderer,&s,&world,&sim,&player,empty,enabled,false,false,false));
            visible_witnesses.push((partial,empty));
        }
        let images=read_all(&renderer.device,shots);assert_eq!(images.len(),20);
        for (visible,hidden) in visible_witnesses {assert!(difference(&images[visible],&images[hidden])>16,"fallback needs a real output witness");}
        if let Some(original)=&expected {assert_eq!(&images,original);}else{expected=Some(images);}
    }
}
