//! Render-only heightfield tiles. The authoritative world and its full-detail
//! meshes remain independent; the shader clips these coarse surfaces out of
//! each uploaded full-detail chunk, including incomplete streaming rings.
use super::{far_bundles, geometry, Renderer};
use crate::{assets::Assets, distant_terrain::{TerrainTile, TilePos, GRID, STEP, TILE_BLOCKS},
    mesh::Mesh, models};
use glam::{IVec3, Vec3};
use std::collections::HashMap;

pub(crate) const DISTANT: u32 = 1 << 31;

#[derive(Default)]
pub(super) struct Distant {
    pub(super) tiles: HashMap<TilePos, far_bundles::Tile>,
    pub(super) bundles: far_bundles::Groups,
    pub(super) binding_revision: u64,
    #[cfg(test)]
    pub(super) disable_bundles: bool,
    pub(super) mask_origin: Option<(i32,i32)>,
    pub(super) visible: usize,
}

impl Distant {
    pub(super) fn invalidate_bindings(&mut self) {
        self.binding_revision=self.binding_revision.wrapping_add(1);
        self.bundles.invalidate_bindings();
    }
}

impl Renderer {
    pub(crate) fn upload_distant(&mut self, position: TilePos, mut mesh: Mesh) {
        // ON-only provenance uses the actual unmodified payload, not the caller
        // or AABB label. OFF replacement must discard any prior valid proof.
        let coverage_proof=self.far_coverage.prove_upload(position,&mesh);
        let indices = geometry::Indices::new(std::mem::take(&mut mesh.indices));
        let revision=self.distant.bundles.payload_changed(position);
        let tile=self.distant.tiles.entry(position).or_insert_with(||far_bundles::Tile::new(&self.device));
        tile.upload(&self.device,&self.queue,&mesh,&indices,revision);
        tile.coverage_proof=coverage_proof;
    }
    pub(crate) fn remove_distant(&mut self, position: TilePos) {
        self.distant.tiles.remove(&position);
        self.distant.bundles.removed(position,self.distant.tiles.len());
    }
    /// Anchor the 64x64 coverage mask around the target full-detail [min,max).
    /// Actual uploaded chunks, rather than the whole target, hide distant cells.
    pub(crate) fn set_distant_detail_bounds(&mut self, min: (i32,i32), max: (i32,i32)) {
        assert!(max.0-min.0<=64 && max.1-min.1<=64);
        self.distant.mask_origin=Some((min.0-(64-(max.0-min.0))/2,
            min.1-(64-(max.1-min.1))/2));
    }
    pub(crate) fn distant_stats(&self) -> (usize,usize,u64) {
        (self.distant.tiles.len(),self.distant.visible,self.distant.tiles.values()
            .map(|tile|tile.gpu.vertex_capacity+tile.gpu.index_capacity).sum())
    }
    pub(super) fn distant_mask(&self, center:(i32,i32),radius:u32)->([i32;4],[[u32;4];32]) {
        let origin=self.distant.mask_origin.unwrap_or((center.0-32,center.1-32));
        ([origin.0,origin.1,64,64],coverage(origin,center,radius,self.chunks.keys().copied()))
    }
}

fn coverage(origin:(i32,i32),center:(i32,i32),radius:u32,
    chunks:impl IntoIterator<Item=(i32,i32)>)->[[u32;4];32] {
    let mut mask=[[0u32;4];32];
    for (x,z) in chunks {
        // Streaming retains one halo column, and can retain older uploads
        // while a native publication is in flight. Section visibility refuses
        // those outside its radius, so they must not hide distant surfaces.
        if x.abs_diff(center.0)>radius || z.abs_diff(center.1)>radius {continue;}
        let (x,z)=(x-origin.0,z-origin.1);
        if (0..64).contains(&x) && (0..64).contains(&z) {
            let index=(z*64+x) as usize;
            mask[index/128][(index/32)%4]|=1<<(index%32);
        }
    }
    mask
}

fn color(assets:&Assets,state:u16,biome:u8,p:IVec3,axis:usize)->[u8;4] {
    let rgb = match state>>4 {
        2 if axis==1 => assets.biome_colors.color(biome,p,0),
        8|9 => assets.biome_colors.color(biome,p,2),
        18 if state&3==1 => 0x619961,
        18 if state&3==2 => 8431445,
        18|161 => assets.biome_colors.color(biome,p,1),
        _ => 0xffffff,
    };
    let shade = match axis { 0=>0.6, 2=>0.8, _=>1. };
    [(((rgb>>16)&255) as f32*shade) as u8,(((rgb>>8)&255) as f32*shade) as u8,
        ((rgb&255) as f32*shade) as u8,255]
}

pub(crate) fn build(tile:&TerrainTile,assets:&Assets)->Mesh {
    build_with_stride(tile,assets,STEP as u8)
}

/// Sampling stays on the cached four-block surface grid. Only distant mesh
/// density changes; cells never cross a chunk edge, so the existing near mask
/// and full-depth boundary skirts remain exact at every supported density.
pub(crate) fn build_with_stride(tile:&TerrainTile,assets:&Assets,stride:u8)->Mesh {
    assert_eq!(tile.heights.len(),GRID*GRID);
    assert!(matches!(stride,4|8|16));
    let step=i32::from(stride);
    let sample_stride=stride as usize/STEP as usize;
    let grid=GRID/sample_stride;
    let surface_at=|x:usize,z:usize| tile.heights[
        (z*sample_stride+sample_stride/2)*GRID+x*sample_stride+sample_stride/2];
    let mut mesh=models::empty();
    mesh.min=Vec3::new((tile.position.0*TILE_BLOCKS) as f32,0.,(tile.position.1*TILE_BLOCKS) as f32);
    mesh.max=mesh.min+Vec3::new(TILE_BLOCKS as f32,256.,TILE_BLOCKS as f32);
    mesh.vertices.reserve(grid*grid*8);
    mesh.indices.reserve(grid*grid*12);
    for z in 0..grid {for x in 0..grid {
        let surface=surface_at(x,z);
        if surface.state==0 || surface.height==0 {continue;}
        let p=IVec3::new(tile.position.0*TILE_BLOCKS+x as i32*step,
            i32::from(surface.height),tile.position.1*TILE_BLOCKS+z as i32*step);
        let first_vertex=mesh.vertices.len();
        let a=p.as_vec3();
        let y=a.y-if matches!(surface.state>>4,8..=11) {1./9.} else {0.};
        let (x0,x1,z0,z1)=(a.x,a.x+step as f32,a.z,a.z+step as f32);
        let layer=assets.texture(surface.state,1,true)|DISTANT|models::OPAQUE_LEAF;
        models::face(&mut mesh,[Vec3::new(x0,y,z0),Vec3::new(x0,y,z1),
            Vec3::new(x1,y,z1),Vec3::new(x1,y,z0)], [0.,0.,step as f32,step as f32],
            layer,color(assets,surface.state,surface.biome,p,1));
        for (dx,dz,axis,positive) in [(-1,0,0,false),(1,0,0,true),(0,-1,2,false),(0,1,2,true)] {
            let (nx,nz)=(x as i32+dx,z as i32+dz);
            // Any chunk can be replaced independently by full-detail terrain.
            // Its actual boundary height need not equal the 4-block-center
            // sample, so a skirt stopped at that sample can expose a crack.
            // Keep complete side walls at chunk boundaries; neighboring LOD
            // tops occlude them until the neighboring chunk is replaced.
            let coordinate=if axis==0 {x} else {z};
            let chunk_edge=(coordinate+usize::from(positive))%(16/stride as usize)==0;
            let bottom=if !chunk_edge && nx>=0 && nz>=0 && nx<grid as i32 && nz<grid as i32 {
                let other=surface_at(nx as usize,nz as usize);
                f32::from(other.height)-if matches!(other.state>>4,8..=11) {1./9.} else {0.}
            } else {0.};
            if bottom>=y {continue;}
            let side=|mesh:&mut Mesh,low:f32,high:f32,state:u16| {
                if high<=low {return;}
                let vertices=match (axis,positive) {
                    (0,false)=>[[x0,low,z0],[x0,low,z1],[x0,high,z1],[x0,high,z0]],
                    (0,true)=>[[x1,low,z1],[x1,low,z0],[x1,high,z0],[x1,high,z1]],
                    (2,false)=>[[x1,low,z0],[x0,low,z0],[x0,high,z0],[x1,high,z0]],
                    _=>[[x0,low,z1],[x1,low,z1],[x1,high,z1],[x0,high,z1]],
                };
                models::face(mesh,vertices.map(Vec3::from_array),[0.,0.,step as f32,high-low],
                    assets.texture(state,axis,positive)|DISTANT|models::OPAQUE_LEAF,
                    color(assets,state,surface.biome,p,axis));
            };
            if surface.state>>4==2 {
                side(&mut mesh,bottom,(y-1.).max(bottom),3<<4);
                side(&mut mesh,bottom.max(y-1.),y,surface.state);
            } else {side(&mut mesh,bottom,y,surface.state);}
        }
        // Spare light bytes encode the owning cell center as an exact offset.
        // Flat owner-chunk clipping also removes vertical skirts sitting on a
        // near chunk edge; flooring the fragment position alone picks the wrong
        // side of a boundary for half of those faces.
        for vertex in &mut mesh.vertices[first_vertex..] {
            vertex.light[2]=(p.x+step/2-vertex.position[0] as i32+128) as u8;
            vertex.light[3]=(p.z+step/2-vertex.position[2] as i32+128) as u8;
        }
    }}
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distant_terrain::Surface;

    #[test]
    fn sparse_negative_chunk_mask_and_uniform_alignment_match_shader() {
        let origin=(-32,-32);
        let positions=[(-32,-32),(-1,-1),(0,0),(31,31),(-33,0),(32,0)];
        let mask=coverage(origin,(0,0),64,positions);
        for z in -32..32 {for x in -32..32 {
            let index=((z+32)*64+x+32) as usize;
            let present=mask[index/128][(index/32)%4]&(1<<(index%32))!=0;
            assert_eq!(present,positions.contains(&(x,z)),"({x},{z})");
        }}
        assert_eq!(mask.iter().flatten().map(|word|word.count_ones()).sum::<u32>(),4);
        assert_eq!(std::mem::offset_of!(crate::renderer::Camera,distant_origin),208);
        assert_eq!(std::mem::offset_of!(crate::renderer::Camera,distant_mask),224);
        assert_eq!(std::mem::offset_of!(crate::renderer::Camera,distant_far),736);
        assert_eq!(std::mem::size_of::<crate::renderer::Camera>(),752);
    }

    #[test]
    fn uploaded_halo_outside_camera_detail_radius_does_not_hide_distant_terrain() {
        let origin=(-24,-24);
        let positions=[(-24,0),(24,0),(25,0),(0,25),(0,-24),(-1,24)];
        for center in [(0,0),(1,0)] {
            let mask=coverage(origin,center,24,positions);
            for (x,z) in positions {
                let index=((z-origin.1)*64+x-origin.0) as usize;
                let hidden=mask[index/128][(index/32)%4]&(1<<(index%32))!=0;
                assert_eq!(hidden,x.abs_diff(center.0)<=24 && z.abs_diff(center.1)<=24,
                    "camera {center:?}, retained upload ({x},{z})");
            }
        }
    }

    #[test]
    fn heightfield_keeps_materials_cell_owners_and_outward_top_faces() {
        let assets=crate::validation::assets();
        let mut tile=TerrainTile {position:(-1,1),heights:vec![Surface {height:64,state:2<<4,biome:1};GRID*GRID]};
        tile.heights[0]=Surface {height:70,state:12<<4,biome:2};
        tile.heights[1]=Surface {height:63,state:9<<4,biome:6};
        let mesh=build(&tile,assets);
        assert!(mesh.quads()>=GRID*GRID);
        for quad in mesh.vertices.chunks_exact(4) {
            assert!(quad.iter().all(|v|v.layer&DISTANT!=0 && v.layer&models::BLEND==0));
            let centers=quad.iter().map(|v|[
                v.position[0] as i32+i32::from(v.light[2])-128,
                v.position[2] as i32+i32::from(v.light[3])-128]).collect::<Vec<_>>();
            assert!(centers.iter().all(|c|*c==centers[0]));
            assert_eq!(centers[0][0].rem_euclid(STEP),STEP/2);
            assert_eq!(centers[0][1].rem_euclid(STEP),STEP/2);
            let a=Vec3::from_array(quad[0].position);
            let b=Vec3::from_array(quad[1].position);
            let c=Vec3::from_array(quad[2].position);
            let normal=(b-a).cross(c-a);
            assert!(normal.length_squared()>0.);
            if quad.iter().all(|v|v.position[1]==a.y) {assert!(normal.y>0.);}
        }
        assert_eq!(mesh.vertices[0].layer&65535,assets.texture(12<<4,1,true));
        assert!(mesh.vertices.iter().any(|v|v.layer&65535==assets.texture(9<<4,1,true)));
        assert!(mesh.vertices.iter().any(|v|v.color==color(assets,2<<4,1,IVec3::new(-122,64,130),1)));
    }

    #[test]
    fn chunk_boundary_skirts_close_lower_full_detail_neighbors_without_internal_walls() {
        let assets=crate::validation::assets();
        let tile=TerrainTile {position:(0,0),heights:vec![Surface {
            height:68,state:1<<4,biome:1};GRID*GRID]};
        let mesh=build(&tile,assets);
        // Equal coarse samples previously emitted no wall at x16. Replacing
        // the left chunk with its actual lower (e.g.y64) surface then exposed
        // the interval y64..68. The right chunk must own a full outward wall.
        let walls=mesh.vertices.chunks_exact(4).filter(|quad|
            quad.iter().all(|v|v.position[0]==16.)
            && quad.iter().any(|v|v.position[2]==4.)
            && quad.iter().any(|v|v.position[2]==8.)).collect::<Vec<_>>();
        let wall=walls.into_iter().find(|quad| {
            let a=Vec3::from_array(quad[0].position);
            let b=Vec3::from_array(quad[1].position);
            let c=Vec3::from_array(quad[2].position);
            (b-a).cross(c-a).x<0.
        }).expect("right coarse chunk must face the replaced left chunk");
        assert_eq!(wall.iter().map(|v|v.position[1]).fold(f32::INFINITY,f32::min),0.);
        assert_eq!(wall.iter().map(|v|v.position[1]).fold(0.,f32::max),68.);
        assert!(wall.iter().all(|v|(v.position[0] as i32+i32::from(v.light[2])-128).div_euclid(16)==1));
        assert!(!mesh.vertices.chunks_exact(4).any(|quad|
            quad.iter().all(|v|v.position[0]==4.) && quad.iter().any(|v|v.position[1]<68.)),
            "equal cells inside a single chunk need no extra wall");
        assert_eq!(mesh.quads(),2048); //1024 tops +64 chunk perimeters of16 faces
    }

    #[test]
    fn coarse_density_reduces_geometry_without_crossing_mask_or_skirt_boundaries() {
        let assets=crate::validation::assets();
        let tile=TerrainTile {position:(-1,1),heights:vec![Surface {
            height:68,state:1<<4,biome:1};GRID*GRID]};
        for (stride,expected_quads) in [(4,2048),(8,768),(16,320)] {
            let mesh=build_with_stride(&tile,assets,stride);
            assert_eq!(mesh.quads(),expected_quads);
            let mut tops=0;
            let mut owners=std::collections::HashSet::new();
            for quad in mesh.vertices.chunks_exact(4) {
                let owner: [[i32;2];4]=std::array::from_fn(|i| {
                    let v=&quad[i];
                    [v.position[0] as i32+i32::from(v.light[2])-128,
                        v.position[2] as i32+i32::from(v.light[3])-128]
                });
                assert!(owner.iter().all(|p|*p==owner[0]));
                let chunk=(owner[0][0].div_euclid(16),owner[0][1].div_euclid(16));
                owners.insert(chunk);
                for v in quad {
                    assert!((chunk.0*16..=(chunk.0+1)*16).contains(&(v.position[0] as i32)));
                    assert!((chunk.1*16..=(chunk.1+1)*16).contains(&(v.position[2] as i32)));
                }
                if quad.iter().all(|v|v.position[1]==68.) { tops+=1; }
                else {
                    assert_eq!(quad.iter().map(|v|v.position[1]).fold(f32::INFINITY,f32::min),0.);
                    assert!(quad.iter().all(|v|v.position[0] as i32%16==0)
                        || quad.iter().all(|v|v.position[2] as i32%16==0),
                        "flat surfaces need walls only at chunk/tile seams");
                }
            }
            assert_eq!(tops,(TILE_BLOCKS/i32::from(stride)).pow(2));
            assert_eq!(owners.len(),64);
        }
        // Existing callers retain exactly the old four-block mesh layout.
        let old=build(&tile,assets);
        let fine=build_with_stride(&tile,assets,4);
        assert_eq!(old.indices,fine.indices);
        assert_eq!(bytemuck::cast_slice::<_,u8>(&old.vertices),bytemuck::cast_slice::<_,u8>(&fine.vertices));
    }

    #[test]
    #[ignore="requires original assets, native terrain generation and Vulkan readback"]
    fn distant_128_draws_original_far_terrain_and_masks_only_uploaded_near_chunks() {
        use crate::{gameplay::{Mode,Simulation},player::Player,renderer::RenderInput,
            settings::Settings,ui::{Frame,Gui,Screen},world::{Chunk,World}};
        use std::sync::Arc;
        use clap::Parser;
        let assets=crate::validation::assets();let resources=crate::validation::resources();
        let settings=Settings {render_distance:128,clouds:0,entity_shadows:false,..Default::default()};
        let mut renderer=pollster::block_on(Renderer::new(None,assets,resources,&settings,None)).unwrap();
        renderer.resize(960,540);
        let mut world=World {chunks:crate::world::ChunkMap::new(),spawn:Vec3::ZERO,label:String::new(),
            save_path:Default::default(),edits:Default::default(),imported:false,light_revision:0};
        for x in [0,2] {
            let mut chunk=Chunk::empty();
            for bx in 0..16 {for bz in 0..16 {chunk.set(bx,80,bz,1<<4);}}
            world.chunks.insert((x,0),Arc::new(chunk));
        }
        for &p in world.chunks.keys() {renderer.upload(p,crate::mesh::build_with_settings(&world,p,assets,&settings));}
        renderer.set_distant_detail_bounds((-24,-24),(25,25));
        let mut player=Player::new(Vec3::new(24.,108.,72.));player.set_rotation_degrees(180.,35.);
        assert!(player.direction().dot((Vec3::new(24.,81.,8.)-player.position).normalize())>0.9);
        let mut sim=Simulation::new(Mode::Creative,189);sim.clock=6000;
        let gui=Gui {screen:Screen::Playing,hide_hud:true,..Default::default()};
        for _ in 0..40 {renderer.tick_lighting(&world,&player,&sim,24);}
        let temporary=std::env::temp_dir().join(format!("ferrum-lod-render-{}-{}",std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&temporary).unwrap();
        let capture_path=temporary.join("probe.png");
        let capture=|renderer:&mut Renderer,player:&Player| {
            renderer.screenshot(&capture_path,&RenderInput {selected_block:None,assets,resources,
                settings:&settings,gui:&gui,ui:&Frame::default(),world:Some(&world),player:Some(player),
                simulation:Some(&sim),alpha:1.,time:0.}).unwrap();
            image::open(&capture_path).unwrap().to_rgba8()
        };
        let mut near=TerrainTile {position:(0,0),heights:vec![Surface {height:64,state:2<<4,biome:1};GRID*GRID]};
        let mut masked=None;
        for stride in [4,8,16] {
            near.heights.iter_mut().for_each(|surface|surface.height=64);
            renderer.upload_distant(near.position,build_with_stride(&near,assets,stride));
            let original=capture(&mut renderer,&player);
            for z in 0..4 {for x in (0..4).chain(8..12) {near.heights[z*GRID+x].height=120;}}
            renderer.upload_distant(near.position,build_with_stride(&near,assets,stride));
            let frame=capture(&mut renderer,&player);
            assert_eq!(original,frame,"stride {stride}: near chunks must clip every coarse top and edge skirt");
            masked=Some(frame);
        }
        let masked=masked.unwrap();
        let stats=renderer.distant_stats();assert_eq!(stats.0,1);assert_eq!(stats.1,1);assert!(stats.2<1024*1024);
        renderer.remove((0,0));renderer.remove((2,0));
        let unmasked=capture(&mut renderer,&player);
        let changed=|a:&image::RgbaImage,b:&image::RgbaImage|a.pixels().zip(b.pixels())
            .filter(|(a,b)|(0..3).any(|c|a[c].abs_diff(b[c])>8)).count();
        assert!(changed(&masked,&unmasked)>1000,"near mask probe must visibly exercise its coverage");
        renderer.remove_distant(near.position);
        player.set_pose_f64([64.,100.,32.],180.,1.,false);
        assert!(player.direction().dot((Vec3::new(64.,70.,-1472.)-player.position).normalize())>0.9);
        let sky=capture(&mut renderer,&player);
        let mut options=crate::Options::parse_from(["ferrum189","--seed","8924706420471469774"]);
        options.project_dir=temporary.clone();
        let source=crate::distant_terrain::DistantTerrain::new(&options).unwrap().unwrap();
        let tile=source.tile((0,-12)).unwrap(); // original surface, 88..96 chunks away
        renderer.upload_distant(tile.position,build_with_stride(&tile,assets,8));
        let distant=capture(&mut renderer,&player);
        let far_pixels=changed(&sky,&distant);
        assert!(far_pixels>100,"128-distance view must render far terrain, found {far_pixels} pixels");
        assert_eq!(renderer.distant_stats().1,1,"near section traversal must not cull the far tile");
        if let Some(path)=std::env::var_os("FERRUM_DISTANT_CAPTURE") {
            let path=std::path::PathBuf::from(path);
            if let Some(parent)=path.parent().filter(|p|!p.as_os_str().is_empty()) {std::fs::create_dir_all(parent).unwrap();}
            let mut sheet=image::RgbaImage::new(1920,1080);
            for (i,frame) in [&masked,&unmasked,&sky,&distant].into_iter().enumerate() {
                image::imageops::replace(&mut sheet,frame,(i%2) as i64*960,(i/2) as i64*540);
            }
            sheet.save(path).unwrap();
        }
        eprintln!("Distant128: {far_pixels} original terrain pixels at88..96 chunks; near mask removes {} overlapping pixels; resident {:?}",changed(&masked,&unmasked),renderer.distant_stats());
        assert!(temporary.file_name().unwrap().to_string_lossy().starts_with("ferrum-lod-render-"));
        std::fs::remove_dir_all(temporary).unwrap();
    }
}

#[cfg(test)]
#[path="far_coverage_gpu_tests.rs"]
mod covered_gpu_tests;
