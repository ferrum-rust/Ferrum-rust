//! Lossless terrain index storage and bounded allocation slack. Vertex bits,
//! primitive order and texture/light inputs remain unchanged.
enum Storage {
    Narrow(Vec<u16>),
    Wide(Vec<u32>),
}

pub(super) struct Indices {
    storage: Storage,
    count: u32,
    max_local_index:Option<u32>,
}
impl Indices {
    pub(super) fn new(indices: Vec<u32>) -> Self {
        let count = indices.len() as u32;
        // Worker-side existing index-width scan also provides the actual
        // logical vertex reach proof. Padding is appended only afterward.
        let max_local_index=indices.iter().copied().max();
        let storage = if max_local_index.is_none_or(|i|i<=u32::from(u16::MAX)) {
            let mut narrow = Vec::with_capacity(indices.len().next_multiple_of(2));
            narrow.extend(indices.into_iter().map(|i| i as u16));
            // queue.write_buffer requires four-byte aligned copy sizes. The
            // padding is never drawn, including an odd triangle index count.
            if narrow.len() % 2 != 0 { narrow.push(0); }
            Storage::Narrow(narrow)
        } else { Storage::Wide(indices) };
        Self { storage, count, max_local_index }
    }
    pub(super) fn format(&self) -> wgpu::IndexFormat {
        match self.storage { Storage::Narrow(_) => wgpu::IndexFormat::Uint16, Storage::Wide(_) => wgpu::IndexFormat::Uint32 }
    }
    pub(super) fn count(&self) -> u32 { self.count }
    pub(super) fn max_local_index(&self)->Option<u32>{self.max_local_index}
    pub(super) fn bytes(&self) -> &[u8] {
        match &self.storage {
            Storage::Narrow(values) => bytemuck::cast_slice(values),
            Storage::Wide(values) => bytemuck::cast_slice(values),
        }
    }
    #[cfg(test)]
    pub(super) fn wide(&mut self) {
        if let Storage::Narrow(values) = &self.storage {
            self.storage = Storage::Wide(values[..self.count as usize].iter().map(|&i| u32::from(i)).collect());
        }
    }
    #[cfg(test)]
    pub(super) fn values(&self) -> Vec<u32> {
        match &self.storage {
            Storage::Narrow(values) => values[..self.count as usize].iter().map(|&i| u32::from(i)).collect(),
            Storage::Wide(values) => values.clone(),
        }
    }
}

pub(super) fn narrow_into(indices: &[u32], scratch: &mut Vec<u16>) {
    scratch.clear();
    scratch.extend(indices.iter().map(|&i| { debug_assert!(i <= u32::from(u16::MAX)); i as u16 }));
    if scratch.len() % 2 != 0 { scratch.push(0); }
}

/// Terrain changes infrequently compared with dynamic entities. Four-KiB
/// buckets avoid almost doubling a large buffer at power-of-two boundaries.
/// Reclaim a historical large mesh only after both relative and absolute
/// slack are substantial, so small block edits do not churn allocations.
pub(super) fn terrain_capacity(current: u64, needed: u64) -> u64 {
    let target = needed.max(256).next_multiple_of(4096);
    if current == 0 || needed > current || (current > target.saturating_mul(2) && current-target >= 64*1024) {
        target
    } else { current }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_indices_round_trip_all_u16_values_and_keep_wide_fallback() {
        for values in [vec![],vec![0,1,2],(0..=u32::from(u16::MAX)).rev().collect(),
            vec![0,65535,65536,2,u32::MAX]] {
            let expected = values.clone();
            let mut indices = Indices::new(values);
            assert_eq!(indices.values(),expected);
            assert_eq!(indices.count(),expected.len() as u32);
            assert_eq!(indices.bytes().len()%4,0);
            assert_eq!(indices.format(),if expected.iter().all(|&i|i<=65535) {
                wgpu::IndexFormat::Uint16
            } else { wgpu::IndexFormat::Uint32 });
            indices.wide();
            assert_eq!(indices.values(),expected);
            assert_eq!(indices.bytes().len(),expected.len()*4);
        }
        let original = [65535,0,1,65535,1,2];
        let mut scratch=Vec::new();
        narrow_into(&original,&mut scratch);
        assert_eq!(scratch.iter().map(|&i|u32::from(i)).collect::<Vec<_>>(),original);
    }

    #[test]
    fn terrain_capacity_avoids_power_of_two_slack_and_small_edit_shrink_churn() {
        assert_eq!(terrain_capacity(256,65537),69632);
        assert_eq!(terrain_capacity(69632,65536),69632);
        assert_eq!(terrain_capacity(69632,40960),69632);
        assert_eq!(terrain_capacity(1024*1024,65536),65536);
        assert_eq!(terrain_capacity(65536,1),65536);
        assert_eq!(terrain_capacity(128*1024,0),4096);
        assert_eq!(terrain_capacity(0,0),4096);
        for needed in [1,4095,4096,4097,65535,65537,1024*1024+1] {
            let capacity=terrain_capacity(256,needed);
            assert!(capacity>=needed);
            if needed>256 {assert!(capacity-needed<4096);}
        }
    }

    #[test]
    #[ignore = "requires original block resources and Vulkan readback"]
    fn mixed_index_widths_preserve_textures_translucent_reordering_and_cached_draws() {
        use crate::{gameplay::{Mode,Simulation},mesh,player::Player,
            renderer::{PreparedChunk,RenderInput,Renderer},settings::Settings,
            ui::{Frame,Gui,Screen},world::{Chunk,World}};
        use glam::{IVec3,Vec3};
        use std::sync::Arc;
        let assets=crate::validation::assets();
        let resources=crate::validation::resources();
        let mut world=World { chunks:crate::world::ChunkMap::from([((0,0),Arc::new(Chunk::empty())),
            ((1,0),Arc::new(Chunk::empty()))]),spawn:Vec3::ZERO,label:String::new(),
            save_path:Default::default(),edits:Default::default(),imported:false,light_revision:0 };
        for x in 0..32 {for z in 0..16 {
            world.set(IVec3::new(x,61,z),(if (x+z)%2==0 {1}else{4})<<4);
        }}
        for base in [0,16] {
            for x in 10..14 {for z in 6..10 {world.set(IVec3::new(base+x,62,z),9<<4);}}
            for y in 62..65 {for z in [5,8] {
                world.set(IVec3::new(base+8,y,z),95<<4 | if z==5 {14}else{3});
            }}
            world.set(IVec3::new(base+6,62,6),50<<4|5);
            world.set(IVec3::new(base+5,62,8),31<<4|1);
            world.set(IVec3::new(base+6,62,10),37<<4);
        }
        let settings=Settings {render_distance:4,clouds:0,entity_shadows:false,..Default::default()};
        let mut renderer=pollster::block_on(Renderer::new(None,assets,resources,&settings,None)).unwrap();
        renderer.resize(640,360);
        let mut sim=Simulation::new(Mode::Creative,189);sim.clock=6000;
        let mut player=Player::new(Vec3::new(16.,67.,26.));
        player.set_rotation_degrees(180.,25.);
        let gui=Gui {screen:Screen::Playing,hide_hud:true,..Default::default()};
        for _ in 0..40 {renderer.tick_lighting(&world,&player,&sim,settings.render_distance);}
        let path=std::env::temp_dir().join(format!("ferrum-index-width-{}.png",std::process::id()));
        let mut frames=Vec::new();
        let mut capacities=Vec::new();
        for compact in [false,true] {
            for pos in [(0,0),(1,0)] {
                let mut mesh=mesh::build_with_settings(&world,pos,assets,&settings);
                if pos==(1,0) {
                    // Identical rendered vertices referenced above 65535 prove
                    // that a wide terrain mesh can coexist with a narrow one.
                    let prefix=vec![mesh.vertices[0];65536];
                    mesh.vertices.splice(0..0,prefix);
                    for index in &mut mesh.indices {*index+=65536;}
                }
                let original_bytes=mesh.vertices.len()*32+mesh.indices.len()*4;
                let mut prepared=PreparedChunk::new(mesh);
                if !compact {prepared.force_wide_indices();}
                if compact && pos==(0,0) {assert!(prepared.byte_len()<original_bytes);}
                renderer.upload(pos,prepared);
            }
            assert_eq!(renderer.chunks[&(0,0)].gpu.index_format,if compact {
                wgpu::IndexFormat::Uint16
            }else{wgpu::IndexFormat::Uint32});
            assert_eq!(renderer.chunks[&(1,0)].gpu.index_format,wgpu::IndexFormat::Uint32);
            capacities.push(renderer.chunks.values().map(|c|c.gpu.vertex_capacity+c.gpu.index_capacity).sum::<u64>());
            for (position,yaw) in [([16.,67.,26.],180.),([16.,67.,-10.],0.)] {
                player.set_pose_f64(position,yaw,25.,false);
                assert!(player.direction().dot((Vec3::new(16.,62.,8.)-player.position).normalize())>0.9);
                let input=RenderInput {selected_block:None,assets,resources,settings:&settings,
                    gui:&gui,ui:&Frame::default(),world:Some(&world),player:Some(&player),
                    simulation:Some(&sim),alpha:1.,time:0.};
                renderer.screenshot(&path,&input).unwrap();
                assert_eq!(renderer.visible_chunks,2,"both index-width probes must face visible terrain");
                let direct=image::open(&path).unwrap().to_rgba8();
                for _ in 0..3 {renderer.screenshot(&path,&input).unwrap();}
                assert!(renderer.terrain_draws_cached());
                let cached=image::open(&path).unwrap().to_rgba8();
                assert_eq!(direct,cached,"direct and bundled draws must agree");
                // Payload changes outside the remembered visible order must
                // keep its bundles; changing a visible payload must reset them
                // even when the existing GPU buffer capacity is sufficient.
                let offscreen=(40,40);
                renderer.upload(offscreen,mesh::build_with_settings(&world,(0,0),assets,&settings));
                assert!(renderer.terrain_draws_cached());
                renderer.screenshot(&path,&input).unwrap();
                assert_eq!(image::open(&path).unwrap().to_rgba8(),cached);
                renderer.remove(offscreen);
                assert!(renderer.terrain_draws_cached());
                renderer.screenshot(&path,&input).unwrap();
                assert_eq!(image::open(&path).unwrap().to_rgba8(),cached);
                let mut replacement=PreparedChunk::new(mesh::build_with_settings(&world,(0,0),assets,&settings));
                if !compact {replacement.force_wide_indices();}
                renderer.upload((0,0),replacement);
                assert!(!renderer.terrain_draws_cached());
                renderer.screenshot(&path,&input).unwrap();
                assert_eq!(image::open(&path).unwrap().to_rgba8(),cached);
                for _ in 0..3 {renderer.screenshot(&path,&input).unwrap();}
                assert!(renderer.terrain_draws_cached());
                assert!(renderer.visible_chunks==2 && renderer.triangles>200);
                frames.push(cached);
            }
        }
        // Compare against the same sky/hand with uploaded terrain removed;
        // equality of two invisible/fog-only views must not pass this test.
        for pos in [(0,0),(1,0)] {renderer.remove(pos);}
        renderer.screenshot(&path,&RenderInput {selected_block:None,assets,resources,settings:&settings,
            gui:&gui,ui:&Frame::default(),world:Some(&world),player:Some(&player),
            simulation:Some(&sim),alpha:1.,time:0.}).unwrap();
        let empty=image::open(&path).unwrap().to_rgba8();
        std::fs::remove_file(&path).unwrap();
        let visible=frames[3].pixels().zip(empty.pixels()).filter(|(a,b)|
            (0..3).any(|c|a[c].abs_diff(b[c])>8)).count();
        assert!(visible>10_000,"visible terrain probe has only {visible} pixels");
        if let Some(path)=std::env::var_os("FERRUM_INDEX_CAPTURE").map(std::path::PathBuf::from) {
            if let Some(parent)=path.parent().filter(|p|!p.as_os_str().is_empty()) {std::fs::create_dir_all(parent).unwrap();}
            let mut sheet=image::RgbaImage::new(1280,720);
            for (i,frame) in frames.iter().enumerate() {
                image::imageops::replace(&mut sheet,frame,(i%2) as i64*640,(i/2) as i64*360);
            }
            sheet.save(path).unwrap();
        }
        assert_eq!(frames[0],frames[2],"narrow front-face/translucent order differs from u32");
        assert_eq!(frames[1],frames[3],"narrow reverse translucent order differs from u32");
        eprintln!("Mixed terrain indices: {visible} visible pixels, byte-exact u16/u32 and direct/bundle images; allocated bytes {:?}",capacities);
    }
}
