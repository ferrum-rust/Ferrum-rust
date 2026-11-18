use crate::{
    assets::Assets,
    gameplay::Simulation,
    mesh::{Frustum, Mesh, Vertex},
    models,
    player::Player,
    resources::Resources,
    settings::Settings,
    ui::{Frame, Gui, UiVertex},
    world::{ChunkPos, World},
};
use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use std::{collections::HashMap, path::Path, sync::Arc};
#[path = "postfx.rs"]
mod postfx;
use winit::window::Window;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
#[path = "chunk_transparency.rs"]
mod chunk_transparency;
use chunk_transparency::ChunkMesh;
pub(crate) use chunk_transparency::PreparedChunk;
pub(crate) use chunk_transparency::sort_batch::Mode as TransparencySortMode;
#[path = "gpu_geometry.rs"]
mod geometry;
#[path="near_arena.rs"]
mod near_arena;
#[path="near_arena_plan.rs"]
mod near_arena_plan;
#[path="renderer_arena.rs"]
mod renderer_arena;
#[path="renderer_arena_policy.rs"]
mod renderer_arena_policy;
pub(crate) use renderer_arena::Choice as NearArenaMode;
#[path = "distant_render.rs"]
pub(crate) mod distant_render;
#[path = "panorama.rs"]
mod panorama;
#[path = "terrain_bundles.rs"]
mod terrain_bundles;
#[path = "near_opaque_shadow.rs"]
mod near_opaque_shadow;
#[path="upload_capture.rs"]
pub(crate) mod upload_capture;
#[path="renderer_surface_diagnostics.rs"]
mod surface_diagnostics;
#[path="arena_upload_capture.rs"]
mod arena_upload_capture;
#[path = "gpu_timestamps.rs"]
mod gpu_timestamps;
#[path = "far_coverage.rs"]
mod far_coverage;
#[path = "surface_wait_trace.rs"]
mod surface_wait_trace;
#[path = "pre_acquire_visibility.rs"]
pub(crate) mod pre_acquire_visibility;
#[path="transparent_staging.rs"]
pub(crate) mod transparent_staging;
#[path="translucent_mdi.rs"]
pub(crate) mod translucent_mdi;
#[path="near_opaque_runs.rs"]
mod near_opaque_runs;
#[path = "renderer_graph_lookup.rs"]
mod graph_lookup;
#[path = "far_group_bundles.rs"]
mod far_bundles;
#[path = "render_profile.rs"]
mod render_profile;
#[path = "transparency_capture.rs"]
mod transparency_capture;
#[path = "block_overlays.rs"]
mod block_overlays;
#[path = "hurt_view.rs"]
mod hurt_view;
use hurt_view::HurtView;
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Camera {
    matrix: [[f32; 4]; 4],
    eye_fog: [f32; 4],
    outline_min: [f32; 4],
    outline_max: [f32; 4],
    lighting: [f32; 4],
    effects: [f32; 4],
    sky_color: [f32; 4],
    fog_ranges: [f32; 4],
    sky_origin: [f32; 4],
    hand_view: [f32; 4],
    distant_origin: [i32; 4],
    distant_mask: [[u32; 4]; 32],
    distant_far: [f32; 4],
}
pub(crate) struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    pub(crate) count: u32,
    min: Vec3,
    max: Vec3,
    vertex_capacity: u64,
    index_capacity: u64,
    index_format: wgpu::IndexFormat,
    vertex_offset:u64,index_offset:u64,arena_token:Option<near_arena::Token>,
}
fn geometry_readback_usage()->wgpu::BufferUsages {
    #[cfg(test)] {wgpu::BufferUsages::COPY_SRC}
    #[cfg(not(test))] {wgpu::BufferUsages::empty()}
}
impl GpuMesh {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            vertices: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("dynamic vertices"),
                size: 256,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST | geometry_readback_usage(),
                mapped_at_creation: false,
            }),
            indices: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("dynamic indices"),
                size: 256,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST | geometry_readback_usage(),
                mapped_at_creation: false,
            }),
            count: 0,
            min: Vec3::ZERO,
            max: Vec3::ZERO,
            vertex_capacity: 256,
            index_capacity: 256,
            index_format: wgpu::IndexFormat::Uint32,
            vertex_offset:0,index_offset:0,arena_token:None,
        }
    }
    fn update(&mut self, d: &wgpu::Device, q: &wgpu::Queue, m: &Mesh) {
        self.update_geometry(d,q,m,bytemuck::cast_slice(&m.indices),m.indices.len() as u32,
            wgpu::IndexFormat::Uint32,false);
    }
    fn update_profiled(&mut self, d: &wgpu::Device, q: &wgpu::Queue, m: &Mesh,
        profile: &mut render_profile::Frame) {
        if profile.enabled {
            let vertices = m.vertices.len() * std::mem::size_of::<Vertex>();
            let indices = m.indices.len() * std::mem::size_of::<u32>();
            profile.buffer_write(vertices, vertices as u64 > self.vertex_capacity);
            profile.buffer_write(indices, indices as u64 > self.index_capacity);
        }
        self.update(d, q, m);
    }
    fn update_terrain(&mut self, d: &wgpu::Device, q: &wgpu::Queue, m: &Mesh, indices: &geometry::Indices) {
        self.update_geometry(d,q,m,indices.bytes(),indices.count(),indices.format(),true);
    }
    fn update_geometry(&mut self, d: &wgpu::Device, q: &wgpu::Queue, m: &Mesh,
        ib: &[u8], count: u32, index_format: wgpu::IndexFormat, terrain: bool) {
        debug_assert!(self.arena_token.is_none()&&self.vertex_offset==0&&self.index_offset==0);
        let vb = bytemuck::cast_slice(&m.vertices);
        for (buffer, cap, bytes, usage) in [
            (
                &mut self.vertices,
                &mut self.vertex_capacity,
                vb,
                wgpu::BufferUsages::VERTEX,
            ),
            (
                &mut self.indices,
                &mut self.index_capacity,
                ib,
                wgpu::BufferUsages::INDEX,
            ),
        ] {
            let next_capacity = if terrain { geometry::terrain_capacity(*cap,bytes.len() as u64) }
                else if bytes.len() as u64 > *cap { (bytes.len() as u64).next_power_of_two() }
                else { *cap };
            if next_capacity != *cap {
                *cap = next_capacity;
                *buffer = d.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("resized geometry"),
                    size: *cap,
                    usage: usage | wgpu::BufferUsages::COPY_DST | geometry_readback_usage(),
                    mapped_at_creation: false,
                });
            }
            if !bytes.is_empty() {
                q.write_buffer(buffer, 0, bytes);
            }
        }
        self.count = count;
        self.index_format = index_format;
        self.min = m.min;
        self.max = m.max;
    }
    fn update_terrain_captured(&mut self,d:&wgpu::Device,q:&wgpu::Queue,m:&Mesh,
        indices:&geometry::Indices,trace:&mut upload_capture::Trace) {
        let ib=indices.bytes();let count=indices.count();let index_format=indices.format();
        debug_assert!(self.arena_token.is_none()&&self.vertex_offset==0&&self.index_offset==0);
        let vb = bytemuck::cast_slice(&m.vertices);
        for (kind,(buffer, cap, bytes, usage)) in [
            (
                &mut self.vertices,
                &mut self.vertex_capacity,
                vb,
                wgpu::BufferUsages::VERTEX,
            ),
            (
                &mut self.indices,
                &mut self.index_capacity,
                ib,
                wgpu::BufferUsages::INDEX,
            ),
        ].into_iter().enumerate() {
            trace.observation.buffers[kind].old_capacity=*cap;
            trace.observation.buffers[kind].submitted_bytes=bytes.len() as u64;
            let next_capacity=geometry::terrain_capacity(*cap,bytes.len() as u64);
            if next_capacity != *cap {
                let replacing=std::time::Instant::now();
                *cap = next_capacity;
                *buffer = {
                    let creating=std::time::Instant::now();
                    let created=d.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("resized geometry"),
                    size: *cap,
                    usage: usage | wgpu::BufferUsages::COPY_DST | geometry_readback_usage(),
                    mapped_at_creation: false,
                    });
                    trace.created(kind,creating);created
                };
                trace.replaced(kind,replacing);
            }
            trace.observation.buffers[kind].new_capacity=*cap;
            if !bytes.is_empty() {
                let writing=std::time::Instant::now();
                q.write_buffer(buffer, 0, bytes);
                trace.written(kind,writing);
            }
        }
        self.count = count;
        self.index_format = index_format;
        self.min = m.min;
        self.max = m.max;
        trace.observation.new_index_width=if index_format==wgpu::IndexFormat::Uint16{2}else{4};
    }
    fn vertex_slice(&self)->wgpu::BufferSlice<'_>{self.vertices.slice(self.vertex_offset..self.vertex_offset+self.vertex_capacity)}
    fn index_slice(&self)->wgpu::BufferSlice<'_>{self.indices.slice(self.index_offset..self.index_offset+self.index_capacity)}
    fn draw<'a>(&'a self, p: &mut wgpu::RenderPass<'a>) {
        self.draw_range(p, 0..self.count);
    }
    fn draw_range<'a>(&'a self, p: &mut wgpu::RenderPass<'a>, range: std::ops::Range<u32>) {
        if !range.is_empty() {
            p.set_vertex_buffer(0, self.vertex_slice());
            p.set_index_buffer(self.index_slice(), self.index_format);
            p.draw_indexed(range, 0, 0..1);
        }
    }
}
pub struct RenderInput<'a> {
    pub selected_block: Option<glam::IVec3>,
    pub assets: &'a Assets,
    pub resources: &'a Resources,
    pub settings: &'a Settings,
    pub gui: &'a Gui,
    pub ui: &'a Frame,
    pub world: Option<&'a World>,
    pub player: Option<&'a Player>,
    pub simulation: Option<&'a Simulation>,
    pub alpha: f32,
    pub time: f32,
}
pub struct Renderer {
    postfx: Option<postfx::Graph>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: Option<wgpu::Surface<'static>>,
    config: wgpu::SurfaceConfiguration,
    surface_support_at_constructor:Option<surface_diagnostics::Support>,
    depth: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    red: wgpu::RenderPipeline,
    cyan: wgpu::RenderPipeline,
    transparent: wgpu::RenderPipeline,
    transparent_red: wgpu::RenderPipeline,
    transparent_cyan: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    binds: [wgpu::BindGroup; 2],
    uniforms: [wgpu::Buffer; 2],
    lightmap_texture: wgpu::Texture,
    lightmap_view: wgpu::TextureView,
    lightmap_sampler: wgpu::Sampler,
    lightmap_key: Option<crate::vanilla::lightmap::Input>,
    torch_flicker: crate::vanilla::lightmap::TorchFlicker,
    fog_brightness: crate::vanilla::atmosphere::FogBrightness,
    visual_random: crate::vanilla::random::JavaRandom,
    animated_tiles: crate::assets::animation::Animator,
    animated_flow: crate::assets::animation::Animator,
    flow_texture: wgpu::Texture,
    flow_view: wgpu::TextureView,
    block_texture: wgpu::Texture,
    block_view: wgpu::TextureView,
    atlas_view: wgpu::TextureView,
    atlas_texture:wgpu::Texture,
    banner_revision: u64,
    skin_revision: u64,
    map_key:Option<(i32,u64)>,
    atlas_sampler: wgpu::Sampler,
    mipmaps: u8,
    chunks: HashMap<ChunkPos, ChunkMesh>,
    distant: distant_render::Distant,
    terrain_bundles: terrain_bundles::TerrainBundles,
    near_opaque_shadow: near_opaque_shadow::Shadow,
    upload_capture: upload_capture::Capture,
    arena_upload_capture:arena_upload_capture::Capture,
    gpu_timestamps: Option<gpu_timestamps::Capture>,
    surface_wait_trace: Option<surface_wait_trace::Capture>,
    pre_acquire_visibility: pre_acquire_visibility::Trial,
    packed_frustum: bool,
    far_coverage: far_coverage::Trial,
    transparent_staging:Option<transparent_staging::Shared>,
    translucent_mdi:Option<translucent_mdi::State>,
    near_arena:Option<renderer_arena::Shared>,
    arena_capabilities:Option<renderer_arena::Capabilities>,
    arena_policy:renderer_arena_policy::Policy,
    #[cfg(test)] fail_automatic_arena_once:bool,
    near_opaque_runs: near_opaque_runs::Runs,
    section_view: crate::mesh::visibility::View,
    dynamic: GpuMesh,
    shadows: GpuMesh,
    pub(crate) nametags: GpuMesh,
    first_person_fire: GpuMesh,
    sky: GpuMesh,
    end_sky: GpuMesh,
    celestial: crate::celestial::Celestial,
    cloud: GpuMesh,
    block_overlays: block_overlays::Overlays,
    cloud_key: Option<(i32, i32, u8, u32)>,
    ui_pipeline: wgpu::RenderPipeline,
    ui_bind: wgpu::BindGroup,
    ui_buffer: wgpu::Buffer,
    ui_capacity: u64,
    panorama: panorama::TitlePanorama,
    pub adapter_name: String,
    pub present_mode: String,
    /// CPU wall time for acquisition, encoding, submission, and presentation.
    /// Submission/acquisition can include driver waits; these are not GPU timestamps.
    pub render_cpu_ms: [f64; 4],
    render_profile: render_profile::Recorder,
    transparency_capture: transparency_capture::Collector,
    transparency_sort: chunk_transparency::sort_batch::Batch,
    pub visible_chunks: usize,
    pub triangles: usize,
    pub visible_entities: usize,
    pub total_entities: usize,
    pub culled_entities: usize,
    update_count: u32,
}
fn array_texture(
    d: &wgpu::Device,
    q: &wgpu::Queue,
    pixels: &[u8],
    size: u32,
    layers: u32,
    mips: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = d.create_texture(&wgpu::TextureDescriptor {
        label: Some("Minecraft resource array"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: layers,
        },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // Vanilla 1.8.9 multiplies texture/lightmap values in its legacy
        // non-sRGB framebuffer. Automatic transfer functions brighten caves.
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut data = pixels.to_vec();
    let mut width = size;
    for level in 0..mips {
        q.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(width),
            },
            wgpu::Extent3d {
                width,
                height: width,
                depth_or_array_layers: layers,
            },
        );
        if level + 1 < mips {
            let next = width / 2;
            let mut smaller = vec![0; (next * next * layers * 4) as usize];
            for l in 0..layers {
                for y in 0..next {
                    for x in 0..next {
                        let mut alpha_sum = 0u32;
                        let mut rgb_sum = [0u32; 3];
                        for dy in 0..2 {
                            for dx in 0..2 {
                                let idx = ((l * width * width
                                    + (y * 2 + dy) * width
                                    + x * 2
                                    + dx)
                                    * 4) as usize;
                                let a = data[idx + 3] as u32;
                                alpha_sum += a;
                                for c in 0..3 {
                                    rgb_sum[c] += data[idx + c] as u32 * a;
                                }
                            }
                        }
                        let dst = ((l * next * next + y * next + x) * 4) as usize;
                        for c in 0..3 {
                            smaller[dst + c] = if alpha_sum > 0 {
                                ((rgb_sum[c] + alpha_sum / 2) / alpha_sum) as u8
                            } else {
                                0
                            };
                        }
                        smaller[dst + 3] = ((alpha_sum + 2) / 4) as u8;
                    }
                }
            }
            data = smaller;
            width = next;
        }
    }
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    (texture, view)
}
fn tex_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2Array,
            multisampled: false,
        },
        count: None,
    }
}
fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}
fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}
fn terrain_bind(
    d: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    block: (&wgpu::TextureView, &wgpu::Sampler),
    atlas: (&wgpu::TextureView, &wgpu::Sampler),
    light: (&wgpu::TextureView, &wgpu::Sampler),
    flow: &wgpu::TextureView,
) -> wgpu::BindGroup {
    d.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("terrain resources"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(block.0),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(block.1),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(atlas.0),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Sampler(atlas.1),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(light.0),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::Sampler(light.1),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::TextureView(flow),
            },
        ],
    })
}
fn tile_sampler(d: &wgpu::Device, mips: u8) -> wgpu::Sampler {
    d.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("nearest blocks / selectable mipmaps"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Linear,
        lod_max_clamp: mips as f32,
        ..Default::default()
    })
}
fn depth(d: &wgpu::Device, w: u32, h: u32) -> wgpu::TextureView {
    d.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
    .create_view(&Default::default())
}
impl Renderer {
    pub(crate) fn menu_device_stats(&self) -> std::collections::BTreeMap<String,String> {
        let limits = self.device.limits();
        [
            ("gpu_info", self.adapter_name.clone()), ("graphics_backend", "Vulkan".into()),
            ("framebuffer_width",self.config.width.to_string()), ("framebuffer_height",self.config.height.to_string()),
            ("max_texture_dimension_2d",limits.max_texture_dimension_2d.to_string()),
            ("max_texture_array_layers",limits.max_texture_array_layers.to_string()),
            ("max_buffer_size",limits.max_buffer_size.to_string()),
        ].into_iter().map(|(k,v)|(k.to_owned(),v)).collect()
    }
    fn world_view(&self,world:&World,player:&Player,s:&Settings,perspective:u8,alpha:f32,hurt:HurtView,bow_fov:f32,confusion_active:bool)->(models::Viewpoint,Mat4,Mat4,Frustum){
        let view = models::camera(player,world,s,perspective,alpha);
        let f2 = player.prev_time_in_portal + (player.time_in_portal - player.prev_time_in_portal) * alpha;
        let swirl = if f2 > 0.0 {
            let speed = if confusion_active { 7.0 } else { 20.0 };
            let mut f4 = 5.0 / (f2 * f2 + 5.0) - f2 * 0.04;
            f4 *= f4;
            let axis = Vec3::new(0.0, std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);
            let angle = ((self.update_count as f32 + alpha) * speed).to_radians();
            Mat4::from_axis_angle(axis, angle)
                * Mat4::from_scale(Vec3::new(1.0 / f4, 1.0, 1.0))
                * Mat4::from_axis_angle(axis, -angle)
        } else {
            Mat4::IDENTITY
        };
        let projection=Mat4::perspective_rh(hurt.fov(s.fov * bow_fov).to_radians(),self.config.width as f32/self.config.height as f32,0.05,
            s.render_distance as f32*16.*std::f32::consts::SQRT_2) * hurt.rotation * swirl;
        let matrix = projection * view.view_at(view.eye);
        (view,projection,matrix,Frustum::new_with_packed(matrix,self.packed_frustum))
    }
    pub(crate) fn configure_covered_lod_culling(&mut self, enabled: bool) {
        // Enabling after an upload does not invent a proof or force a rebuild.
        // Unproven resident meshes remain on the original rendering path.
        self.far_coverage = far_coverage::Trial::new(enabled);
    }
    pub(crate) fn covered_lod_culling_report(&self) -> serde_json::Value {
        self.far_coverage.report()
    }
    pub(crate) fn configure_packed_frustum(&mut self, enabled: bool) {
        if self.packed_frustum != enabled {
            self.packed_frustum = enabled;
            self.section_view.invalidate();
        }
    }
    pub(crate) fn packed_frustum_report(&self) -> serde_json::Value {
        serde_json::json!({
            "enabled":self.packed_frustum,
            "mode":if self.packed_frustum {"packed-four-planes"} else {"scalar"},
            "explicit_trial":self.packed_frustum,
            "frustum_storage_bytes":std::mem::size_of::<Frustum>(),
            "storage":"The optional packed planes are inline with the original six planes, with no heap allocation. This increases Frustum type size even in scalar mode; the current same-binary scalar arm is the performance control.",
            "scope":"Only world_view Frustum construction/evaluation selection changes. Original matrix math, section traversal/order, surface acquisition order, automatic arena policy, GPU work and graphics settings remain unchanged. Ordinary default is scalar after-acquire.",
        })
    }
    pub fn boss(&mut self,world:&World,player:&Player,sim:&Simulation,s:&Settings,perspective:u8,alpha:f32)->Option<models::boss_bar::Boss>{
        let scene=&sim.server_scene;
        let native: Vec<_> = sim.mobs.iter().filter_map(|mob| models::boss_bar::Candidate::local(mob, sim)).collect();
        self.section_view.track_order(!scene.bosses.is_empty() || !native.is_empty());
        if scene.bosses.is_empty() && native.is_empty(){return None}
        let (view,_,mat,f)=self.world_view(world,player,s,perspective,alpha,HurtView::for_camera(sim, alpha),sim.equipped.bow_fov_multiplier(alpha),sim.effects.get(9).is_some());
        let eye = view.eye;
        let columns=graph_lookup::Columns::new(eye,s.full_detail_distance() as u8);
        self.section_view.update(mat,&f,eye,view.direction,s.full_detail_distance() as u8,
            |pos|columns.get(pos,||self.chunks.get(&pos)?.graph.as_deref()));
        let camera=player.previous.lerp(player.position,alpha).as_dvec3();
        let start=(eye/16.).floor().as_ivec3();
        scene.bosses.iter().chain(&native).filter(|b|{
            let p=b.section;let chunk=(p.x,p.z);let bit=1<<p.y;
            !(scene.camera==Some(b.id) && perspective==0 && !player.sleeping)
                && (!(0.0..256.0).contains(&b.at.y) || world.chunks.contains_key(&chunk))
                && (b.boss.kind == 63 || self.section_view.mask(chunk,&f,bit)!=0)
                && b.visible(camera,&f)
        }).max_by_key(|b|{
            let d=b.section-start;
            // Published visibility graphs retain the exact visitation order. When a mesh is pending,
            // FIFO traversal over DOWN,UP,NORTH,SOUTH,WEST,EAST gives this open-section ordering.
            (self.section_view.order(b.section).unwrap_or(0),d.abs().element_sum(),[
                d.y.min(0),-d.y.max(0),d.z.min(0),-d.z.max(0),d.x.min(0),-d.x.max(0)],b.order)
        }).map(|b|b.boss.clone())
    }
    pub fn resolution(&self) -> [u32; 2] {
        [self.config.width, self.config.height]
    }
    pub async fn new(
        window: Option<Arc<Window>>,
        assets: &Assets,
        r: &Resources,
        s: &Settings,
        benchmark_adapter: Option<&str>,
    ) -> Result<Self> {
        Self::new_with_arena(window,assets,r,s,benchmark_adapter,None).await
    }
    pub(crate) async fn new_with_arena(
        window:Option<Arc<Window>>,assets:&Assets,r:&Resources,s:&Settings,
        benchmark_adapter:Option<&str>,benchmark_near_arena:Option<NearArenaMode>,
    )->Result<Self>{
        Self::new_with_arena_diagnostics(window,assets,r,s,benchmark_adapter,benchmark_near_arena,false).await
    }
    pub(crate) async fn new_with_arena_diagnostics(
        window:Option<Arc<Window>>,assets:&Assets,r:&Resources,s:&Settings,
        benchmark_adapter:Option<&str>,benchmark_near_arena:Option<NearArenaMode>,
        suppress_automatic_for_diagnostics:bool,
    )->Result<Self>{
        Self::new_with_benchmark_diagnostics(window, assets, r, s, benchmark_adapter,
            benchmark_near_arena, suppress_automatic_for_diagnostics, false).await
    }
    pub(crate) async fn new_with_benchmark_diagnostics(
        window:Option<Arc<Window>>,assets:&Assets,r:&Resources,s:&Settings,
        benchmark_adapter:Option<&str>,benchmark_near_arena:Option<NearArenaMode>,
        suppress_automatic_for_diagnostics:bool, capture_gpu_timestamps:bool,
    )->Result<Self>{
        Self::new_with_gpu_stage_diagnostics(window,assets,r,s,benchmark_adapter,
            benchmark_near_arena,suppress_automatic_for_diagnostics,capture_gpu_timestamps,false).await
    }
    pub(crate) async fn new_with_gpu_stage_diagnostics(
        window:Option<Arc<Window>>,assets:&Assets,r:&Resources,s:&Settings,
        benchmark_adapter:Option<&str>,benchmark_near_arena:Option<NearArenaMode>,
        suppress_automatic_for_diagnostics:bool,capture_gpu_timestamps:bool,capture_gpu_stages:bool,
    )->Result<Self>{
        anyhow::ensure!(!capture_gpu_stages||capture_gpu_timestamps,
            "GPU stages require the existing GPU timestamp capture");
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..Default::default()
        });
        let surface = window
            .as_ref()
            .map(|w| instance.create_surface(w.clone()))
            .transpose()?;
        let adapter = if let Some(name) = benchmark_adapter {
            let adapters = instance.enumerate_adapters(wgpu::Backends::VULKAN);
            let available = adapters
                .iter()
                .map(|a| a.get_info().name)
                .collect::<Vec<_>>();
            adapters
                .into_iter()
                .find(|a| {
                    a.get_info().name.eq_ignore_ascii_case(name)
                        && surface.as_ref().is_none_or(|s| a.is_surface_supported(s))
                })
                .with_context(|| {
                    format!("No presentable Vulkan adapter named {name}. Available: {available:?}")
                })?
        } else {
            instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: surface.as_ref(),
                    force_fallback_adapter: false,
                })
                .await
                .context("No Vulkan adapter available")?
        };
        let info = adapter.get_info();
        anyhow::ensure!(info.backend == wgpu::Backend::Vulkan, "Vulkan required");
        let automatic=benchmark_near_arena.is_none();
        let mut arena_capabilities=match benchmark_near_arena {
            Some(NearArenaMode::Original)=>None,
            Some(mode)=>Some(renderer_arena::Capabilities::inspect(&adapter,mode)),
            None if suppress_automatic_for_diagnostics=>None,
            None=>Some(renderer_arena::Capabilities::inspect(&adapter,NearArenaMode::Mdi)),
        };
        // Negotiate once so a later live settings upgrade can be admitted
        // without replacing the Device or migrating existing dedicated meshes.
        let mut descriptor=wgpu::DeviceDescriptor {
            label:Some("Ferrum Vulkan"),
            required_features:arena_capabilities.as_ref().map_or(wgpu::Features::empty(),renderer_arena::Capabilities::features),
            required_limits:wgpu::Limits {max_texture_array_layers:r.layers().max(assets.layers).max(256),..Default::default()},
            memory_hints:wgpu::MemoryHints::Performance,
        };
        if capture_gpu_timestamps {
            descriptor.required_features |= if capture_gpu_stages {
                gpu_timestamps::required_features_stages(&adapter)?
            } else { gpu_timestamps::required_features(&adapter)? };
        }
        let mut optional_device_failure=None;
        let (device,queue)=match adapter.request_device(&descriptor,None).await {
            Ok(pair)=>pair,
            Err(error) if automatic&&!capture_gpu_timestamps&&!descriptor.required_features.is_empty()=>{
                optional_device_failure=Some(format!("automatic optional Device feature request failed: {error}"));
                descriptor.required_features=wgpu::Features::empty();
                arena_capabilities=None;
                adapter.request_device(&descriptor,None).await?
            },
            Err(error)=>return Err(error.into()),
        };
        let gpu_timestamps = if capture_gpu_timestamps {
            Some(if capture_gpu_stages { gpu_timestamps::Capture::new_stages(&device,&queue)? }
                else { gpu_timestamps::Capture::new(&device, &queue)? })
        } else { None };
        let mut arena_policy=renderer_arena_policy::Policy::new(benchmark_near_arena,s.render_distance,
            arena_capabilities.is_some_and(|caps|caps.supports_automatic()));
        if suppress_automatic_for_diagnostics {arena_policy.suppress_diagnostic("requested diagnostic requires original cached backing");}
        if let Some(reason)=optional_device_failure {arena_policy.initialization_failed(&reason);}
        let size = window
            .as_ref()
            .map(|w| w.inner_size())
            .unwrap_or(winit::dpi::PhysicalSize::new(1600, 900));
        let (format,surface_support_at_constructor) = surface
            .as_ref()
            .map(|v| {
                let caps = v.get_capabilities(&adapter);
                let format=caps.formats
                    .iter()
                    .copied()
                    .find(|f| !f.is_srgb())
                    .unwrap_or(caps.formats[0]);
                (format,Some(surface_diagnostics::Support::new(&caps.present_modes)))
            })
            .unwrap_or((wgpu::TextureFormat::Rgba8Unorm,None));
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: if s.vsync {
                wgpu::PresentMode::AutoVsync
            } else {
                wgpu::PresentMode::AutoNoVsync
            },
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        if let Some(surface) = &surface {
            surface.configure(&device, &config);
        }
        let block_mips = (assets.tile_size.trailing_zeros() + 1).min(5);
        let (block_texture, block_view) = array_texture(
            &device,
            &queue,
            &assets.pixels,
            assets.tile_size,
            assets.layers,
            block_mips,
        );
        let animated_tiles = crate::assets::animation::Animator::new(assets.animations.clone());
        animated_tiles.upload(&queue, &block_texture, 1);
        let animated_flow = crate::assets::animation::Animator::new(assets.flowing_tiles.clone());
        let (flow_texture, flow_view) = array_texture(&device, &queue, animated_flow.base_pixels(),
            crate::assets::animation::FLOW_SIZE, 2, crate::assets::animation::LEVELS as u32);
        animated_flow.upload(&queue, &flow_texture, 0);
        let (atlas_texture, atlas_view) = array_texture(&device, &queue, &r.pixels, 256, r.layers(), 1);
        let block_sampler = tile_sampler(&device, s.mipmaps);
        let (lightmap_texture, lightmap_view) =
            array_texture(&device, &queue, &[0; 16 * 16 * 4], 16, 1, 1);
        let lightmap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("1.8.9 linear lightmap"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pixel font and sprites"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("terrain bindings"),
            entries: &[
                uniform_entry(0),
                tex_entry(1),
                sampler_entry(2),
                tex_entry(3),
                sampler_entry(4),
                tex_entry(5),
                sampler_entry(6),
                tex_entry(7),
            ],
        });
        let uniforms = std::array::from_fn(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("camera"),
                size: std::mem::size_of::<Camera>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let binds = std::array::from_fn(|i| {
            terrain_bind(
                &device,
                &layout,
                &uniforms[i],
                (&block_view, &block_sampler),
                (&atlas_view, &atlas_sampler),
                (&lightmap_view, &lightmap_sampler),
                &flow_view,
            )
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("terrain"),
            source: wgpu::ShaderSource::Wgsl(include_str!("terrain.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let make_pipeline = |mask, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor{label:Some("terrain pipeline"),layout:Some(&pipeline_layout),vertex:wgpu::VertexState{module:&shader,entry_point:Some("vs_main"),compilation_options:Default::default(),buffers:&[wgpu::VertexBufferLayout{array_stride:std::mem::size_of::<Vertex>()as u64,step_mode:wgpu::VertexStepMode::Vertex,attributes:&wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Uint32,3=>Unorm8x4,4=>Unorm8x4]}]},fragment:Some(wgpu::FragmentState{module:&shader,entry_point:Some("fs_main"),compilation_options:Default::default(),targets:&[Some(wgpu::ColorTargetState{format,blend:if blend{Some(wgpu::BlendState::ALPHA_BLENDING)}else{None},write_mask:mask})]}),primitive:wgpu::PrimitiveState{cull_mode:Some(wgpu::Face::Back),..Default::default()},depth_stencil:Some(wgpu::DepthStencilState{format:DEPTH,depth_write_enabled:!blend,depth_compare:wgpu::CompareFunction::LessEqual,stencil:Default::default(),bias:Default::default()}),multisample:Default::default(),multiview:None,cache:None})
        };
        let pipeline = make_pipeline(wgpu::ColorWrites::ALL, false);
        let red = make_pipeline(wgpu::ColorWrites::RED | wgpu::ColorWrites::ALPHA, false);
        let cyan = make_pipeline(wgpu::ColorWrites::GREEN | wgpu::ColorWrites::BLUE, false);
        let transparent = make_pipeline(wgpu::ColorWrites::ALL, true);
        let transparent_red =
            make_pipeline(wgpu::ColorWrites::RED | wgpu::ColorWrites::ALPHA, true);
        let transparent_cyan =
            make_pipeline(wgpu::ColorWrites::GREEN | wgpu::ColorWrites::BLUE, true);
        let ui_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("GUI bindings"),
            entries: &[
                tex_entry(0),
                sampler_entry(1),
                tex_entry(2),
                sampler_entry(3),
            ],
        });
        let ui_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &ui_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&atlas_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&block_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&block_sampler),
                },
            ],
        });
        let ui_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("GUI"),
            source: wgpu::ShaderSource::Wgsl(include_str!("hud.wgsl").into()),
        });
        let ul = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&ui_layout],
            push_constant_ranges: &[],
        });
        let ui_pipeline=device.create_render_pipeline(&wgpu::RenderPipelineDescriptor{label:Some("original GUI"),layout:Some(&ul),vertex:wgpu::VertexState{module:&ui_shader,entry_point:Some("vs_main"),compilation_options:Default::default(),buffers:&[wgpu::VertexBufferLayout{array_stride:std::mem::size_of::<UiVertex>()as u64,step_mode:wgpu::VertexStepMode::Vertex,attributes:&wgpu::vertex_attr_array![0=>Float32x2,1=>Float32x2,2=>Float32x4,3=>Uint32,4=>Float32x4]}]},fragment:Some(wgpu::FragmentState{module:&ui_shader,entry_point:Some("fs_main"),compilation_options:Default::default(),targets:&[Some(wgpu::ColorTargetState{format,blend:Some(wgpu::BlendState::ALPHA_BLENDING),write_mask:wgpu::ColorWrites::ALL})]}),primitive:Default::default(),depth_stencil:None,multisample:Default::default(),multiview:None,cache:None});
        let panorama = panorama::TitlePanorama::new(&device, format, &atlas_view, r);
        let ui_capacity = 65536;
        let ui_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: ui_capacity,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let dynamic = GpuMesh::new(&device);
        let shadows = GpuMesh::new(&device);
        let nametags = GpuMesh::new(&device);
        let mut first_person_fire = GpuMesh::new(&device);
        first_person_fire.update(&device, &queue, &crate::entity_visual::first_person_fire(assets));
        let mut sky = GpuMesh::new(&device);
        sky.update(&device, &queue, &models::sky_plane());
        let mut end_sky = GpuMesh::new(&device);
        end_sky.update(&device, &queue, &models::end_sky(r));
        let celestial = crate::celestial::Celestial::new(
            &device,
            format,
            DEPTH,
            &atlas_view,
            &atlas_sampler,
            r,
        );
        let cloud = GpuMesh::new(&device);
        let block_overlays = block_overlays::Overlays::new(&device, &pipeline_layout, &shader, format);
        let depth = depth(&device, config.width, config.height);
        let present_mode = format!("{:?}", config.present_mode);
        // Off-screen screenshots have a repeatable visual sequence. Windowed
        // play has its own client RNG, independent of world and entity RNGs.
        let visual_seed = if surface.is_some() {
            use std::hash::BuildHasher;
            std::collections::hash_map::RandomState::new().hash_one(()) as i64
        } else {
            189
        };
        let near_arena=if arena_policy.desired().is_some() {
            match renderer_arena::State::new(&device,arena_capabilities.expect("selected mode has capabilities")) {
                Ok(state)=>Some(state),
                Err(error) if automatic=>{arena_policy.initialization_failed(&format!("automatic arena initialization failed: {error}"));None},
                Err(error)=>return Err(error),
            }
        }else{None};
        println!("Vulkan adapter: {}", info.name);
        Ok(Self {
            device,
            queue,
            surface,
            config,
            surface_support_at_constructor,
            depth,
            pipeline,
            red,
            cyan,
            transparent,
            transparent_red,
            transparent_cyan,
            layout,
            binds,
            uniforms,
            lightmap_texture,
            lightmap_view,
            lightmap_sampler,
            lightmap_key: None,
            torch_flicker: Default::default(),
            fog_brightness: Default::default(),
            visual_random: crate::vanilla::random::JavaRandom::new(visual_seed),
            animated_tiles,
            animated_flow,
            flow_texture,
            flow_view,
            block_texture,
            block_view,
            atlas_view,
            atlas_texture,map_key:None, banner_revision: 0, skin_revision: 0,
            atlas_sampler,
            mipmaps: s.mipmaps,
            chunks: HashMap::new(),
            distant: Default::default(),
            terrain_bundles: Default::default(),
            near_opaque_shadow: Default::default(),
            upload_capture: Default::default(),
            arena_upload_capture:Default::default(),
            gpu_timestamps,
            surface_wait_trace: None,
            pre_acquire_visibility: Default::default(),
            packed_frustum: false,
            far_coverage: Default::default(),
            transparent_staging:None,
            translucent_mdi:None,
            near_arena,arena_capabilities,arena_policy,
            #[cfg(test)] fail_automatic_arena_once:false,
            near_opaque_runs: Default::default(),
            section_view: Default::default(),
            dynamic,
            shadows,
            nametags,
            first_person_fire,
            sky,
            end_sky,
            celestial,
            cloud,
            block_overlays,
            cloud_key: None,
            ui_pipeline,
            ui_bind,
            ui_buffer,
            ui_capacity,
            panorama,
            postfx: None,
            adapter_name: info.name,
            present_mode,
            render_cpu_ms: [0.; 4],
            render_profile: Default::default(),
            transparency_capture: Default::default(),
            transparency_sort: Default::default(),
            visible_chunks: 0,
            triangles: 0,
            visible_entities: 0,
            total_entities: 0,
            culled_entities: 0,
            update_count: 0,
        })
    }
    pub fn tick_lighting(&mut self, world: &World, player: &Player, sim: &Simulation, radius: u32) {
        self.update_count = self.update_count.wrapping_add(1);
        self.torch_flicker.tick(&mut self.visual_random);
        let [block, sky, ..] = world.visual_light_uv(player.position.floor().as_ivec3(), 0);
        let (rain, thunder) = sim.weather.strengths(1.);
        let sky_darkening =
            crate::vanilla::atmosphere::skylight_subtracted(sim.clock as i64, rain, rain * thunder);
        let effective_sky = if sim.dimension == 0 {
            (sky / 16).saturating_sub(sky_darkening)
        } else {
            0
        };
        self.fog_brightness.tick_dimension(
            (block / 16).max(effective_sky),
            radius,
            sim.dimension,
        );
        if self.animated_tiles.tick() {
            self.animated_tiles.upload(&self.queue, &self.block_texture, 1);
        }
        if self.animated_flow.tick() {
            self.animated_flow.upload(&self.queue, &self.flow_texture, 0);
        }
    }
    pub(crate) fn surface_configuration_report(&self)->serde_json::Value {
        surface_diagnostics::report(&self.config,self.surface_support_at_constructor)
    }
    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        if let Some(effect) = self.postfx.as_mut() {
            effect.resize(&self.device, [w,h]);
        }
        self.depth = depth(&self.device, w, h);
        if let Some(s) = &self.surface {
            s.configure(&self.device, &self.config);
        }
    }
    pub fn apply_settings(&mut self, s: &Settings) {
        self.arena_policy.update_distance(s.render_distance);
        self.reconcile_arena_policy();
        let mode = if s.vsync {
            wgpu::PresentMode::AutoVsync
        } else {
            wgpu::PresentMode::AutoNoVsync
        };
        if mode != self.config.present_mode {
            self.config.present_mode = mode;
            self.present_mode = format!("{mode:?}");
            if let Some(surface) = &self.surface {
                surface.configure(&self.device, &self.config);
            }
        }
        if self.mipmaps != s.mipmaps {
            self.near_opaque_shadow.bindings_changed();
            self.near_opaque_runs.bindings_changed();
            self.terrain_bundles.invalidate();
            self.distant.invalidate_bindings();
            self.mipmaps = s.mipmaps;
            let sampler = tile_sampler(&self.device, s.mipmaps);
            self.binds = std::array::from_fn(|i| {
                terrain_bind(
                    &self.device,
                    &self.layout,
                    &self.uniforms[i],
                    (&self.block_view, &sampler),
                    (&self.atlas_view, &self.atlas_sampler),
                    (&self.lightmap_view, &self.lightmap_sampler),
                    &self.flow_view,
                )
            });
        }
    }
    #[cfg(test)]
    pub(crate) fn terrain_draws_cached(&self) -> bool {
        !self.terrain_bundles.eyes.is_empty()
    }
    pub fn upload(&mut self, p: ChunkPos, m: impl Into<PreparedChunk>) {
        self.upload_prepared(p, m.into());
    }
    pub(crate) fn upload_prepared(&mut self, p: ChunkPos, prepared: PreparedChunk) {
        if self.near_arena.is_some(){self.upload_arena(p,prepared);return;}
        if let Some(trace)=self.upload_capture.start(p) {
            self.upload_prepared_captured(p,prepared,trace);return;
        }
        self.near_opaque_runs.payload_changed(p);
        let arena_upload = if self.near_opaque_shadow.enabled() {
            let (order, _, matching, cached) = self.terrain_bundles.diagnostic_key();
            self.near_opaque_shadow.payload_changed(p, order, matching != 0 || cached);
            Some(prepared.arena_upload_metadata())
        }else{None};
        self.terrain_bundles.invalidate_position(p);
        self.section_view.invalidate();
        let gpu = self
            .chunks
            .entry(p)
            .or_insert_with(|| ChunkMesh::new(&self.device));
        gpu.update_prepared(&self.device, &self.queue, prepared);
        if let Some(mut upload)=arena_upload {
            upload.vertex_reserved=gpu.gpu.vertex_capacity;
            upload.index_reserved=gpu.gpu.index_capacity;
            self.near_opaque_shadow.geometry_uploaded(p,upload);
        }
    }
    fn upload_prepared_captured(&mut self, p: ChunkPos, prepared: PreparedChunk, mut trace: upload_capture::Trace) {
        trace.observation.map_len_before=self.chunks.len();
        trace.observation.map_capacity_before=self.chunks.capacity();
        let hooks=std::time::Instant::now();
        self.near_opaque_runs.payload_changed(p);
        let arena_upload = if self.near_opaque_shadow.enabled() {
            let (order, _, matching, cached) = self.terrain_bundles.diagnostic_key();
            self.near_opaque_shadow.payload_changed(p, order, matching != 0 || cached);
            Some(prepared.arena_upload_metadata())
        }else{None};
        trace.phase(upload_capture::Phase::Hooks,hooks);
        self.terrain_bundles.invalidate_position_captured(p,&mut trace);
        self.section_view.invalidate();
        let entry=std::time::Instant::now();
        let gpu = self
            .chunks
            .entry(p)
            .or_insert_with(|| {
                trace.observation.vacant=true;
                let initial=std::time::Instant::now();
                let chunk=ChunkMesh::new(&self.device);
                trace.phase(upload_capture::Phase::Initial,initial);chunk
            });
        trace.phase(upload_capture::Phase::Entry,entry);
        gpu.update_prepared_captured(&self.device,&self.queue,prepared,&mut trace);
        trace.returned();
        if let Some(mut upload)=arena_upload {
            let hooks=std::time::Instant::now();
            upload.vertex_reserved=gpu.gpu.vertex_capacity;
            upload.index_reserved=gpu.gpu.index_capacity;
            self.near_opaque_shadow.geometry_uploaded(p,upload);
            trace.phase(upload_capture::Phase::Hooks,hooks);
        }
        trace.observation.map_len_after=self.chunks.len();
        trace.observation.map_capacity_after=self.chunks.capacity();
        self.upload_capture.record(trace.finish());
    }
    pub fn remove(&mut self, p: ChunkPos) {
        if self.near_arena.is_some(){self.remove_arena(p);return;}
        if self.chunks.remove(&p).is_some() {
            self.near_opaque_runs.removed(p);
            if self.near_opaque_shadow.enabled() {
                let (order, _, matching, cached) = self.terrain_bundles.diagnostic_key();
                self.near_opaque_shadow.removed(p, order, matching != 0 || cached);
            }
            self.terrain_bundles.invalidate_position(p);
            self.section_view.invalidate();
        }
    }
    pub fn clear(&mut self) {
        self.postfx = None;
        self.fog_brightness = Default::default();
        self.near_opaque_shadow.cleared();
        self.near_opaque_runs.cleared();
        self.terrain_bundles.invalidate();
        self.section_view.invalidate();
        self.chunks.clear();
        if self.near_arena.is_some(){self.clear_arena();}
        self.reconcile_arena_policy();
        self.distant=Default::default();
        self.cloud_key = None;
        self.nametags.count = 0;
    }
    pub fn cycle_shader(&mut self, resources: &Resources, active_world: bool) -> Result<()> {
        if !active_world { return Ok(()); }
        let next = self.postfx.take().map_or(0, |p| p.index + 1);
        if next == postfx::PRESETS.len() { return Ok(()); }
        self.load_shader(resources, next)
    }
    pub fn reload_shader_from(&mut self, previous: &Self, resources: &Resources) -> Result<()> {
        if let Some(effect) = &previous.postfx {
            self.load_shader(resources, effect.index)?;
        }
        Ok(())
    }
    fn load_shader(&mut self, resources: &Resources, next: usize) -> Result<()> {
        self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let graph = postfx::Graph::load(&self.device, &self.queue, self.config.format, self.resolution(), next,
            |path| resources.bytes(path));
        if let Some(error) = pollster::block_on(self.device.pop_error_scope()) {
            anyhow::bail!("Shader {}: {error}", postfx::PRESETS[next]);
        }
        self.postfx = Some(graph?);
        Ok(())
    }
    pub fn toggle_shader(&mut self) {
        if let Some(effect) = &mut self.postfx {
            effect.enabled = !effect.enabled;
        }
    }
    fn encode(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        output: &wgpu::TextureView,
        input: &RenderInput<'_>,
        profile: &mut render_profile::Frame,
    ) {
        assert!(self.near_arena.is_none(),"shared arena encoding requires a submit-owned Session");
        assert!(self.transparent_staging.is_none(),"staging encoding requires a submit-owned Session");
        self.encode_inner(encoder,output,input,profile,None,None,None);
    }
    fn encode_inner(&mut self,encoder:&mut wgpu::CommandEncoder,output:&wgpu::TextureView,
        input:&RenderInput<'_>,profile:&mut render_profile::Frame,mut arena_session:Option<&mut renderer_arena::Session<'_>>,
        mut staging_session:Option<&mut transparent_staging::Session<'_>>,
        prepared_view:Option<pre_acquire_visibility::PreparedView>) {
        let s = input.settings;
        let post_view = self.postfx.as_ref().filter(|p| p.enabled && input.world.is_some()).map(|p| p.main_view().clone());
        let view = post_view.as_ref().unwrap_or(output);
        let far = s.render_distance as f32 * 16.0;
        self.visible_chunks = 0;
        self.distant.visible=0;
        self.triangles = 0;
        if let (Some(world), Some(player), Some(sim)) =
            (input.world, input.player, input.simulation)
        {
            let visibility_prepared = prepared_view.is_some();
            let (hurt,(viewpoint,projection,matrix,frustum)) = match prepared_view {
                Some(prepared) => (prepared.hurt,prepared.view),
                None => {
                    let hurt = HurtView::for_camera(sim,input.alpha);
                    (hurt,self.world_view(world,player,s,input.gui.perspective,input.alpha,hurt,sim.equipped.bow_fov_multiplier(input.alpha),sim.effects.get(9).is_some()))
                },
            };
            let models::Viewpoint { eye, direction, right } = viewpoint;
            if let Some(m)=&sim.server_scene.map{
                if self.map_key!=Some((m.id,m.revision)){
                    self.queue.write_texture(wgpu::TexelCopyTextureInfo{texture:&self.atlas_texture,mip_level:0,origin:wgpu::Origin3d{x:0,y:0,z:input.resources.layers()-1},aspect:wgpu::TextureAspect::All},&m.rgba,
                    wgpu::TexelCopyBufferLayout{offset:0,bytes_per_row:Some(512),rows_per_image:Some(128)},wgpu::Extent3d{width:128,height:128,depth_or_array_layers:1});
                    self.map_key=Some((m.id,m.revision));
                }
            }else{self.map_key=None;}
            let model_started = profile.timer();
            let mut dynamic = models::build_profiled(models::ModelContext {
                sim,
                player,
                world,
                assets: input.assets,
                resources: input.resources,
                settings: s,
                frustum: &frustum,
                viewport_height: self.config.height,
                eye,
                alpha: input.alpha,
                perspective: input.gui.perspective,
                render_hand: !input.gui.hide_hud && !input.gui.spectator.spectator && !player.sleeping,
                hitboxes: input.gui.hitboxes,
            }, &mut profile.models);
            profile.stages_ms.model_cpu += render_profile::elapsed(model_started);
            let camera_chunk = (eye / 16.).floor().as_ivec3();
            let (distant_origin,distant_mask)=self.distant_mask((camera_chunk.x,camera_chunk.z),s.full_detail_distance());
            let distant_far=[(camera_chunk.x-s.render_distance as i32) as f32*16.,
                (camera_chunk.z-s.render_distance as i32) as f32*16.,
                (camera_chunk.x+s.render_distance as i32+1) as f32*16.,
                (camera_chunk.z+s.render_distance as i32+1) as f32*16.];
            let chest_started = profile.timer();
            let nearby_chests = self.chunks.iter()
                .filter(|((x, z), chunk)| !chunk.chests.is_empty()
                    && (x - camera_chunk.x).abs() <= 4 && (z - camera_chunk.z).abs() <= 4)
                .flat_map(|(_, chunk)| chunk.chests.iter().copied());
            crate::chest_render::visible(&mut dynamic.opaque, world, &sim.server_scene.chest_lids,
                nearby_chests, input.resources, eye, &frustum, input.alpha);
            profile.stages_ms.chest_cpu += render_profile::elapsed(chest_started);
            self.visible_entities = dynamic.visible;
            self.total_entities = dynamic.total;
            self.culled_entities = dynamic.culled;
            if profile.enabled {
                profile.camera = eye.to_array();
                profile.counters.loaded_chunks = world.chunks.len();
                profile.counters.dynamic_vertices = dynamic.opaque.vertices.len()
                    + dynamic.translucent.vertices.len()
                    + dynamic.nametags.vertices.len();
                profile.counters.dynamic_indices = dynamic.opaque.indices.len()
                    + dynamic.translucent.indices.len()
                    + dynamic.nametags.indices.len();
            }
            let dynamic_started = profile.timer();
            self.dynamic.update_profiled(&self.device, &self.queue, &dynamic.opaque, profile);
            self.shadows.update_profiled(&self.device, &self.queue, &dynamic.translucent, profile);
            self.nametags.update_profiled(&self.device, &self.queue, &dynamic.nametags, profile);
            profile.stages_ms.dynamic_gpu_writes += render_profile::elapsed(dynamic_started);
            let overworld = sim.dimension == 0;
            let show_sky = overworld && s.render_distance >= 4;
            let show_end_sky = sim.dimension == 1;
            let clouds = if overworld { s.clouds } else { 0 };
            let key = (
                (eye.x / 48.0).floor() as i32,
                (eye.z / 48.0).floor() as i32,
                clouds,
                s.full_detail_distance(),
            );
            if self.cloud_key != Some(key) {
                let cloud_started = profile.timer();
                self.cloud.update_profiled(
                    &self.device,
                    &self.queue,
                    &models::clouds(eye, clouds, input.resources, s.full_detail_distance() as f32*16.),
                    profile,
                );
                self.cloud_key = Some(key);
                profile.stages_ms.clouds += render_profile::elapsed(cloud_started);
            }
            if !visibility_prepared {
                let section_started = profile.timer();
                let columns=graph_lookup::Columns::new(eye,s.full_detail_distance() as u8);
                self.section_view.update(
                    matrix,
                    &frustum,
                    eye,
                    direction,
                    s.full_detail_distance() as u8,
                    |pos| columns.get(pos,||self.chunks.get(&pos)?.graph.as_deref()),
                );
                profile.stages_ms.section_visibility += render_profile::elapsed(section_started);
            }
            let visible_started = profile.timer();
            let capture=&mut self.transparency_capture;
            let batch_requested=self.transparency_sort.wants_batch(capture.enabled());
            let mut visible = if let Some(staging)=staging_session.as_mut() {
                let mut visible = self
                .chunks
                .iter_mut()
                .filter_map(|(&pos, chunk)| {
                    if chunk.gpu.count == 0 || !frustum.visible(chunk.gpu.min, chunk.gpu.max) {
                        return None;
                    }
                    let mask = if chunk.graph.is_some() {
                        self.section_view.mask(pos, &frustum, chunk.present)
                    } else {
                        chunk.present
                    };
                    if mask == 0 {
                        return None;
                    }
                    chunk.prepare_ranges(mask);
                    Some((pos, chunk))
                })
                .collect::<Vec<_>>();
            // Preserve encounter order. Select one sink for the WHOLE frame
            // before any Order mutation; no mid-frame Queue/staging mixture.
            let bound=visible.iter().fold(Some(0),|sum,(_,chunk)|
                transparent_staging::add_bound(sum,chunk.staging_bound(eye,chunk.mask)));
            staging.plan(bound).expect("transparent staging preflight failed closed");
            if batch_requested {
                if self.transparency_sort.run(&mut visible,eye,profile) {
                    self.transparency_sort.complete_staged(&mut visible,&self.queue,profile,staging,encoder);
                }else{
                    for (pos,chunk) in &mut visible {
                        chunk.prepare_staged(&self.queue,eye,chunk.mask,profile,*pos,capture,staging,encoder);
                    }
                }
            }else{
                for (pos,chunk) in &mut visible {
                    chunk.prepare_staged(&self.queue,eye,chunk.mask,profile,*pos,capture,staging,encoder);
                }
            }
            visible
            }else{
                // Ordinary Queue path retains its original collect/prepare/
                // batch completion order and performs no staging preflight.
                let mut visible = self
                .chunks
                .iter_mut()
                .filter_map(|(&pos, chunk)| {
                    if chunk.gpu.count == 0 || !frustum.visible(chunk.gpu.min, chunk.gpu.max) {
                        return None;
                    }
                    let mask = if chunk.graph.is_some() {
                        self.section_view.mask(pos, &frustum, chunk.present)
                    } else {
                        chunk.present
                    };
                    if mask == 0 {
                        return None;
                    }
                    if batch_requested {chunk.prepare_ranges(mask);}
                    else {chunk.prepare(&self.queue, eye, mask, profile, pos, capture);}
                    Some((pos, chunk))
                })
                .collect::<Vec<_>>();
            if batch_requested {
                if self.transparency_sort.run(&mut visible,eye,profile) {
                    self.transparency_sort.complete(&mut visible,&self.queue,profile);
                }else{
                    for (pos,chunk) in &mut visible {
                        chunk.prepare(&self.queue,eye,chunk.mask,profile,*pos,capture);
                    }
                }
            }
            visible
            };
            profile.stages_ms.visible_chunks_inclusive += render_profile::elapsed(visible_started);
            let order_started = profile.timer();
            visible.sort_unstable_by(|(pa, a), (pb, b)| {
                chunk_transparency::cmp_column_front_to_back(
                    (*pa, a.gpu.min, a.gpu.max),
                    (*pb, b.gpu.min, b.gpu.max),
                    eye,
                )
            });
            self.visible_chunks = visible.len();
            let distant_bindings=self.distant.binding_revision;
            let distant_resident=self.distant.tiles.len();
            let distant_bundles_enabled={
                #[cfg(test)] { !self.distant.disable_bundles }
                #[cfg(not(test))] { true }
            };
            // Enumerate ALL residents before visibility filtering. Each group
            // retains the same visible subsequence as the original values().
            self.far_coverage.begin_frame(profile.enabled);
            let distant_visible=self.distant.tiles.iter().enumerate().filter_map(|(index,(&position,tile))| {
                let mesh=&tile.gpu;
                if !(mesh.count!=0 && mesh.max.x>distant_far[0] && mesh.max.z>distant_far[1]
                    && mesh.min.x<distant_far[2] && mesh.min.z<distant_far[3]
                    && frustum.visible(mesh.min,mesh.max)) {return None;}
                // Use exactly the origin/mask later copied into each Camera.
                // The OFF branch does no new mask lookup or counter work.
                if !self.far_coverage.keep_visible(tile.coverage_proof,distant_origin,&distant_mask,
                    mesh.count,profile.enabled) {return None;}
                Some(far_bundles::Visible {
                    group:far_bundles::slot(index),position,revision:tile.revision,gpu:mesh})
            }).collect::<Vec<_>>();
            self.distant.visible=distant_visible.len();
            self.triangles = visible.iter().map(|(_, c)| c.triangles()).sum::<usize>()
                + (self.dynamic.count + self.cloud.count) as usize / 3
                + distant_visible.iter().map(|tile|tile.gpu.count as usize/3).sum::<usize>();
            profile.stages_ms.visible_order += render_profile::elapsed(order_started);
            let outline = input.selected_block.and_then(|p| {
                let state = world.get(p);
                (state != 0).then(|| crate::vanilla::ray_trace::outline_bounds(world, p, state)
                    .expand(glam::DVec3::splat(f64::from(0.002_f32))))
            });
            let overlay_started = profile.timer();
            let mut cracks = models::empty();
            let local_damage = sim.break_target.and_then(|p| {
                let stage = (sim.breaking * 10.) as i32 - 1;
                (0..10).contains(&stage).then_some((p, stage as u8))
            });
            for (p, stage) in local_damage.into_iter().chain(sim.server_scene.cracks.iter().copied()) {
                if p.as_vec3().distance_squared(eye) <= 1024. {
                    let angle = sim.server_scene.chest_lids.angle(world, p, input.alpha);
                    crate::mesh::block_damage(&mut cracks, world, input.assets, s, p, stage, angle);
                }
            }
            self.block_overlays.cracks.update_profiled(&self.device, &self.queue, &cracks, profile);
            profile.stages_ms.block_overlay += render_profile::elapsed(overlay_started);
            let (rain_now, thunder_now) = sim.weather.strengths(1.);
            let daylight = crate::vanilla::lightmap::sun_brightness(
                sim.clock as i64,
                1.0,
                sim.dimension,
                rain_now,
                thunder_now,
            );
            let lightmap_key = crate::vanilla::lightmap::Input {
                dimension: sim.dimension,
                sun: daylight,
                boss:input.gui.boss_bar.tint,previous_boss:input.gui.boss_bar.previous_tint,
                gamma: s.gamma,
                torch_flicker: self.torch_flicker.value,
                night_vision: sim.effects.get(16).map(|effect| effect.duration),
                ..Default::default()
            };
            if self.lightmap_key != Some(lightmap_key) {
                let colors = crate::vanilla::lightmap::colors(&crate::vanilla::lightmap::Input {
                    partial: input.alpha,
                    ..lightmap_key
                });
                self.queue.write_texture(
                    self.lightmap_texture.as_image_copy(),
                    bytemuck::cast_slice(&colors),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(64),
                        rows_per_image: Some(16),
                    },
                    self.lightmap_texture.size(),
                );
                self.lightmap_key = Some(lightmap_key);
            }
            let feet = player.previous.lerp(player.position, input.alpha);
            let block_pos = player.position.floor().as_ivec3();
            let near = eye + direction * 0.05;
            let eye_block = near.floor().as_ivec3();
            let mut medium_state = world.get(eye_block);
            if matches!(medium_state >> 4, 8..=11) {
                let level = (medium_state & 15) as u8;
                let depth = f32::from(if level >= 8 { 1 } else { level + 1 }) / 9. - 0.11111111;
                if near.y >= (eye_block.y + 1) as f32 - depth {
                    medium_state = world.get(eye_block + glam::IVec3::Y);
                }
            }
            let medium = match medium_state >> 4 {
                8 | 9 => crate::vanilla::atmosphere::Medium::Water,
                10 | 11 => crate::vanilla::atmosphere::Medium::Lava,
                _ => crate::vanilla::atmosphere::Medium::Air,
            };
            let (rain, thunder) = sim.weather.strengths(input.alpha);
            let atmosphere =
                crate::vanilla::atmosphere::sample(crate::vanilla::atmosphere::Input {
                    dimension: sim.dimension,
                    time: sim.clock as i64,
                    partial: input.alpha,
                    feet_y: f64::from(feet.y),
                    temperature: crate::assets::biome_colors::temperature(
                        crate::assets::biome_colors::biome_at(world, block_pos),
                        block_pos,
                    ),
                    look_x: player.direction().x,
                    radius: s.render_distance,
                    brightness: self.fog_brightness.sample(input.alpha),
                    medium,
                    blindness: sim.effects.get(15).map(|e| e.duration),
                    night_vision: sim.effects.get(16).map(|e| e.duration),
                    respiration: sim.inventory.armor_enchantment(5),
                    water_breathing: sim.effects.get(13).is_some(),
                    flat: sim.flat_world,
                    rain,
                    thunder: rain * thunder,
                    anaglyph: s.anaglyph,
                    ..Default::default()
                });
            let [r, g, b] = input.gui.boss_bar.fog(atmosphere.fog,input.alpha).map(|c| if c.is_finite() { c } else { 0. });
            let count = if s.anaglyph { 2 } else { 1 };
            let eyes = [
                terrain_bundles::Eye {
                    opaque: if count == 1 {
                        &self.pipeline
                    } else {
                        &self.red
                    },
                    translucent: if count == 1 {
                        &self.transparent
                    } else {
                        &self.transparent_red
                    },
                    bind: &self.binds[0],
                },
                terrain_bundles::Eye {
                    opaque: &self.cyan,
                    translucent: &self.transparent_cyan,
                    bind: &self.binds[1],
                },
            ];
            // Diagnostic-only consecutive-frame CPU shadow, outside the
            // existing bundle timer. Its cost remains inside total encode.
            let shadow_sample = self.near_opaque_shadow.begin_frame(profile.enabled);
            if shadow_sample {
                let (order, old_eyes, matching, cached) = self.terrain_bundles.diagnostic_key();
                self.near_opaque_shadow.sample(profile.frame_id, count, self.config.format,
                    order, old_eyes, matching, cached, &visible);
            }
            let bundles_started = profile.timer();
            let (rebuilt,bundle_draws)=if arena_session.is_some(){
                self.terrain_bundles.invalidate();(false,0)
            }else{self.terrain_bundles.prepare(&self.device,self.config.format,&eyes[..count],&visible)};
            profile.stages_ms.terrain_bundles += render_profile::elapsed(bundles_started);
            if shadow_sample {
                self.near_opaque_shadow.completed(rebuilt, !self.terrain_bundles.eyes.is_empty());
            }
            if profile.enabled {
                profile.counters.terrain_bundle_rebuilds += usize::from(rebuilt);
                profile.counters.terrain_bundle_draws += bundle_draws;
            }
            self.near_opaque_runs.prepare(&self.device,self.config.format,&eyes[..count],
                &visible,!self.terrain_bundles.eyes.is_empty(),profile);
            self.distant.bundles.prepare(&self.device,self.config.format,&eyes[..count],
                distant_bindings,distant_resident,&distant_visible,distant_bundles_enabled,profile);
            // Build and upload the immutable opaque arguments once, before
            // either eye. No per-eye queue write can overwrite prior arguments.
            let alpha_limits=if self.translucent_mdi.is_some() {
                arena_session.as_ref().and_then(|session|session.translucent_limits())
            }else{None};
            let arena_source=renderer_arena::Visible::new(&visible);
            let arena_plan=arena_session.as_mut().and_then(|session|session.prepare(&arena_source,&self.queue));
            // Explicit-only reverse adapter and separate immutable arguments.
            // Both plans borrow one Recording and both uploads precede BOTH eyes.
            let alpha_rows=self.translucent_mdi.as_ref().map(|_|translucent_mdi::VisibleRows::new(&visible));
            let mut alpha_frame=self.translucent_mdi.as_mut().map(|state|
                state.frame(alpha_rows.as_ref().unwrap(),s.render_distance,alpha_limits,arena_plan.is_some()));
            let alpha_issued=alpha_frame.as_ref().map(|frame|frame.issued);
            let alpha_plan=match (&mut alpha_frame,&arena_plan) {
                (Some(frame),Some(opaque))=>frame.builder.as_mut().and_then(|builder|
                    builder.build(opaque.recording(),&frame.source,&self.queue)),
                _=>None,
            };
            for (i, eye_pass) in eyes[..count].iter().enumerate() {
                let offset = if count == 2 {
                    right * if i == 0 { -0.032 } else { 0.032 }
                } else {
                    Vec3::ZERO
                };
                let camera = Camera {
                    matrix: (projection * viewpoint.view_at(eye + offset))
                        .to_cols_array_2d(),
                    eye_fog: [eye.x, eye.y, eye.z, far],
                    outline_min: outline.map_or(Vec3::ZERO, |b| b.min.as_vec3())
                        .extend(2. / self.config.width as f32).to_array(),
                    outline_max: outline.map_or(Vec3::ZERO, |b| b.max.as_vec3())
                        .extend(2. / self.config.height as f32).to_array(),
                    lighting: [daylight, s.gamma, r, g],
                    effects: [
                        input.resources.sprite("textures/misc/enchanted_item_glint.png").layer as f32,
                        input.time,
                        b,
                        self.config.height as f32 / self.config.width as f32,
                    ],
                    sky_color: [
                        atmosphere.sky[0],
                        atmosphere.sky[1],
                        atmosphere.sky[2],
                        atmosphere.density.unwrap_or(0.),
                    ],
                    fog_ranges: [
                        atmosphere.terrain_range[0],
                        atmosphere.terrain_range[1],
                        atmosphere.sky_range[0],
                        atmosphere.sky_range[1],
                    ],
                    sky_origin: feet.extend(0.).to_array(),
                    hand_view: hurt.hand_view(player.eyes_in_water),
                    distant_origin,
                    distant_mask,
                    distant_far,
                };
                let uniform_started = profile.timer();
                self.queue
                    .write_buffer(&self.uniforms[i], 0, bytemuck::bytes_of(&camera));
                profile.stages_ms.camera_uniform_writes += render_profile::elapsed(uniform_started);
                profile.buffer_write(std::mem::size_of::<Camera>(), false);
                let draw_started = profile.timer();
                if let Some(capture)=&mut self.gpu_timestamps {
                    capture.mark_encoder(encoder,gpu_timestamps::Label::WorldBegin(i as u8)).expect("GPU stage marker rejected");
                }
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("world"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: if i == 0 {
                                wgpu::LoadOp::Clear(wgpu::Color {
                                    r: f64::from(r),
                                    g: f64::from(g),
                                    b: f64::from(b),
                                    a: 1.0,
                                })
                            } else {
                                wgpu::LoadOp::Load
                            },
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                if let Some(capture)=&mut self.gpu_timestamps {
                    capture.mark_pass(&mut pass,gpu_timestamps::Label::NearBegin(i as u8)).expect("GPU stage marker rejected");
                }
                let near_started = profile.timer();
                let cached = self.terrain_bundles.eyes.get(i);
                if profile.enabled { profile.counters.near_bundled_eyes += usize::from(cached.is_some()); }
                if let Some(batches) = cached {
                    pass.execute_bundles([&batches.opaque]);
                } else if let Some(plan)=&arena_plan {
                    pass.set_pipeline(eye_pass.opaque);pass.set_bind_group(0,eye_pass.bind,&[]);
                    plan.draw(&mut pass,&visible,profile);
                } else if !self.near_opaque_runs.draw(&mut pass,&visible,eye_pass,i,profile) {
                    pass.set_pipeline(eye_pass.opaque);
                    pass.set_bind_group(0, eye_pass.bind, &[]);
                    for (_, chunk) in &visible {
                        for range in &chunk.solid_ranges {
                            if profile.enabled { profile.counters.near_direct_draws += usize::from(!range.is_empty()); }
                            chunk.gpu.draw_range(&mut pass, range.clone());
                        }
                    }
                }
                if let Some(capture)=&mut self.gpu_timestamps {
                    capture.mark_pass(&mut pass,gpu_timestamps::Label::NearEnd(i as u8)).expect("GPU stage marker rejected");
                }
                profile.stages_ms.world_near_opaque += render_profile::elapsed(near_started);
                let far_started = profile.timer();
                // Distant tiles have their own culling path: the near section
                // graph must never suppress terrain beyond the resident ring.
                if !distant_visible.is_empty() {
                    self.distant.bundles.draw(&mut pass,&distant_visible,eye_pass,i,
                        distant_bundles_enabled,profile);
                }
                if let Some(capture)=&mut self.gpu_timestamps {
                    capture.mark_pass(&mut pass,gpu_timestamps::Label::FarEnd(i as u8)).expect("GPU stage marker rejected");
                }
                profile.stages_ms.world_far_opaque += render_profile::elapsed(far_started);
                let dynamic_started = profile.timer();
                if self.dynamic.count != 0 || self.cloud.count != 0 || show_sky || show_end_sky {
                    // execute_bundles resets all render pass state.
                    if cached.is_some() {
                        pass.set_pipeline(eye_pass.opaque);
                        pass.set_bind_group(0, eye_pass.bind, &[]);
                    }
                    if show_sky {
                        // RenderGlobal disables depth writes for the sky. The
                        // sky shader returns alpha=1, so this preserves its
                        // opaque color while preventing far-plane depth from
                        // intermittently occluding the sun, moon and stars.
                        pass.set_pipeline(eye_pass.translucent);
                        self.sky.draw(&mut pass);
                        self.celestial.draw(
                            &mut pass,
                            &self.queue,
                            crate::celestial::Frame {
                                projection_view: projection
                                    * viewpoint.view_at(eye + offset - feet),
                                time: sim.clock as i64,
                                partial: input.alpha,
                                rain,
                                sunrise: atmosphere.sunrise,
                                eye: if count == 1 { 0 } else { i + 1 },
                            },
                        );
                        pass.set_pipeline(eye_pass.opaque);
                        pass.set_bind_group(0, eye_pass.bind, &[]);
                    } else if show_end_sky {
                        pass.set_pipeline(eye_pass.translucent);
                        pass.set_bind_group(0, eye_pass.bind, &[]);
                        self.end_sky.draw(&mut pass);
                        pass.set_pipeline(eye_pass.opaque);
                        pass.set_bind_group(0, eye_pass.bind, &[]);
                    }
                    self.dynamic.draw(&mut pass);
                    self.cloud.draw(&mut pass);
                }
                if let Some(capture)=&mut self.gpu_timestamps {
                    capture.mark_pass(&mut pass,gpu_timestamps::Label::SkyDynamicEnd(i as u8)).expect("GPU stage marker rejected");
                }
                profile.stages_ms.world_sky_dynamic += render_profile::elapsed(dynamic_started);
                let translucent_started = profile.timer();
                self.block_overlays.draw(&mut pass, eye_pass.bind, if count == 1 { 0 } else { i + 1 }, outline.is_some());
                if self.shadows.count != 0 || cached.is_none() {
                    pass.set_pipeline(eye_pass.translucent);
                    pass.set_bind_group(0, eye_pass.bind, &[]);
                    self.shadows.draw(&mut pass);
                }
                if let Some(batches) = cached {
                    pass.execute_bundles([&batches.translucent]);
                    if let Some(issued)=alpha_issued{translucent_mdi::cached_eye(issued);}
                } else if let Some(plan)=&alpha_plan {
                    plan.draw(&mut pass,&visible,profile);
                } else if let Some(issued)=alpha_issued {
                    let mut calls=0u64;
                    for (_,chunk) in visible.iter().rev(){
                        let range=chunk.translucent_range();calls+=u64::from(!range.is_empty());
                        if profile.enabled{profile.counters.near_translucent_direct_draws+=usize::from(!range.is_empty());}
                        chunk.gpu.draw_range(&mut pass,range);
                    }
                    translucent_mdi::direct_eye(issued,calls);
                } else {
                    for (_, chunk) in visible.iter().rev() {
                        if profile.enabled { profile.counters.near_translucent_direct_draws += usize::from(!chunk.translucent_range().is_empty()); }
                        chunk.gpu.draw_range(&mut pass, chunk.translucent_range());
                    }
                }
                if self.nametags.count != 0 {
                    pass.set_pipeline(eye_pass.translucent);
                    pass.set_bind_group(0, eye_pass.bind, &[]);
                    self.nametags.draw(&mut pass);
                }
                if input.gui.perspective == 0 && !player.sleeping && !player.is_spectator()
                    && !input.gui.spectator.spectator && sim.player_burning() {
                    pass.set_pipeline(eye_pass.translucent);
                    pass.set_bind_group(0, eye_pass.bind, &[]);
                    self.first_person_fire.draw(&mut pass);
                }
                if let Some(capture)=&mut self.gpu_timestamps {
                    capture.mark_pass(&mut pass,gpu_timestamps::Label::AlphaEnd(i as u8)).expect("GPU stage marker rejected");
                }
                profile.stages_ms.world_translucent += render_profile::elapsed(translucent_started);
                let pass_started = profile.timer();
                drop(pass);
                if let Some(capture)=&mut self.gpu_timestamps {
                    capture.mark_encoder(encoder,gpu_timestamps::Label::WorldEnd(i as u8)).expect("GPU stage marker rejected");
                }
                profile.stages_ms.world_pass_finish += render_profile::elapsed(pass_started);
                profile.stages_ms.world_draw_record += render_profile::elapsed(draw_started);
            }
        } else {
            self.visible_entities = 0;
            self.total_entities = 0;
            self.culled_entities = 0;
            self.panorama.render(encoder, &self.queue, view, input.ui.size,
                input.ui.panorama.then_some(input.ui.panorama_time));
        }
        if post_view.is_some() {
            let postfx_started = profile.timer();
            if let Some(capture)=&mut self.gpu_timestamps {
                capture.mark_encoder(encoder,gpu_timestamps::Label::PostBegin).expect("GPU stage marker rejected");
            }

            self.postfx.as_mut().unwrap().draw(&self.queue, encoder, output, input.alpha);
            if let Some(capture)=&mut self.gpu_timestamps {
                capture.mark_encoder(encoder,gpu_timestamps::Label::PostEnd).expect("GPU stage marker rejected");
            }
            profile.stages_ms.postfx += render_profile::elapsed(postfx_started);
        }
        let view = output;
        let banners_started = profile.timer();
        input.resources.banners.upload(&self.queue, &self.atlas_texture, &mut self.banner_revision);
        input.resources.skins.upload(&self.queue, &self.atlas_texture, &mut self.skin_revision);
        profile.stages_ms.banner_upload += render_profile::elapsed(banners_started);
        let ui_started = profile.timer();
        let bytes = bytemuck::cast_slice(&input.ui.vertices);
        profile.buffer_write(bytes.len(), bytes.len() as u64 > self.ui_capacity);
        if bytes.len() as u64 > self.ui_capacity {
            self.ui_capacity = (bytes.len() as u64).next_power_of_two();
            self.ui_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: self.ui_capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !bytes.is_empty() {
            self.queue.write_buffer(&self.ui_buffer, 0, bytes);
            profile.stages_ms.ui_gpu_writes += render_profile::elapsed(ui_started);
            let ui_draw_started = profile.timer();
            if let Some(capture)=&mut self.gpu_timestamps {
                capture.mark_encoder(encoder,gpu_timestamps::Label::GuiBegin).expect("GPU stage marker rejected");
            }

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("GUI"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.ui_pipeline);
            pass.set_bind_group(0, &self.ui_bind, &[]);
            pass.set_vertex_buffer(0, self.ui_buffer.slice(..));
            pass.draw(0..input.ui.vertices.len() as u32, 0..1);
            drop(pass);
            if let Some(capture)=&mut self.gpu_timestamps {
                capture.mark_encoder(encoder,gpu_timestamps::Label::GuiEnd).expect("GPU stage marker rejected");
            }
            profile.stages_ms.ui_draw_record += render_profile::elapsed(ui_draw_started);
        } else {
            profile.stages_ms.ui_gpu_writes += render_profile::elapsed(ui_started);
        }
    }
    pub(crate) fn configure_upload_capture(&mut self,enabled:bool,application_origin:Option<std::time::Instant>) {
        if enabled {self.suppress_automatic_arena("upload capture requires original cached backing");}
        assert!(!enabled||self.near_arena.is_none(),"upload capture and arena prototype are separate experiments");
        self.upload_capture=upload_capture::Capture::new(enabled);
        self.upload_capture.application_origin(application_origin);
    }
    pub(crate) fn take_upload_observation(&mut self)->Option<upload_capture::Observation> {self.upload_capture.take_last()}
    pub(crate) fn upload_capture_report(&self)->serde_json::Value {self.upload_capture.report()}
    pub(crate) fn configure_arena_upload_capture(&mut self,enabled:bool,origin:Option<std::time::Instant>) {
        // This diagnostic NEVER changes policy, owner, features or backing.
        self.arena_upload_capture=arena_upload_capture::Capture::new(enabled,origin);
    }
    pub(crate) fn arena_upload_capture_report(&self)->serde_json::Value {self.arena_upload_capture.report()}
    pub(crate) fn configure_near_opaque_runs(&mut self, enabled: bool) {
        if enabled {self.suppress_automatic_arena("opaque-run experiment requires original dedicated backing");}
        assert!(!enabled||self.near_arena.is_none(),"arena prototype bypasses optional near bundles");
        self.near_opaque_runs=near_opaque_runs::Runs::new(enabled);
    }
    pub(crate) fn near_opaque_runs_report(&self)->serde_json::Value {
        self.near_opaque_runs.report()
    }
    pub(crate) fn configure_near_opaque_shadow(&mut self, enabled: bool) {
        if enabled {self.suppress_automatic_arena("opaque shadow requires original cached backing");}
        assert!(!enabled||self.near_arena.is_none(),"arena prototype and hypothetical arena census must not mix");
        self.near_opaque_shadow = near_opaque_shadow::Shadow::new(enabled);
    }
    pub(crate) fn near_opaque_shadow_report(&self) -> serde_json::Value {
        self.near_opaque_shadow.report()
    }
    pub(crate) fn configure_transparency_sort(&mut self,mode:TransparencySortMode) {
        self.transparency_sort.configure(mode);
    }
    pub(crate) fn transparency_sort_report(&self)->serde_json::Value {self.transparency_sort.report()}
    pub(crate) fn configure_transparency_capture(&mut self,enabled:bool) {
        if enabled {self.suppress_automatic_arena("transparency capture requires original cached backing");}
        self.transparency_capture=transparency_capture::Collector::new(enabled);
    }
    pub(crate) fn transparency_capture_time(&mut self,time:Option<f64>) {
        self.transparency_capture.measured_time(time);
    }
    pub(crate) fn transparency_capture_report(&self)->serde_json::Value {self.transparency_capture.report()}
    pub(crate) fn write_transparency_capture(&self,path:&Path)->Result<()> {self.transparency_capture.write_once(path)}
    pub(crate) fn configure_slow_frame_threshold(&mut self, threshold: crate::benchmark_profile::SlowFrameMs) {
        self.render_profile.configure_threshold(threshold);
    }
    pub(crate) fn set_frame_profiling(&mut self, enabled: bool) {
        self.render_profile.enabled = enabled;
    }
    /// Report-time census of owned terrain buffers, never a per-frame scan.
    /// Requested capacities exclude driver allocations and retired submissions.
    pub(crate) fn terrain_allocation_report(&self) -> serde_json::Value {
        let (mut vertices, mut indices, mut index_count) = (0u64, 0u64, 0u64);
        let (mut uint16, mut uint32, mut nonempty) = (0usize, 0usize, 0usize);
        let mut dedicated=0usize;
        for chunk in self.chunks.values() {
            let gpu = &chunk.gpu;
            vertices += gpu.vertex_capacity;
            indices += gpu.index_capacity;
            index_count += u64::from(gpu.count);
            dedicated+=usize::from(gpu.arena_token.is_none());
            nonempty += usize::from(gpu.count != 0);
            match gpu.index_format {
                wgpu::IndexFormat::Uint16 => uint16 += 1,
                wgpu::IndexFormat::Uint32 => uint32 += 1,
            }
        }
        let owned=if self.near_arena.is_some(){self.arena_unique_pages().map(|pages|[pages[0]+dedicated,pages[1]+dedicated])}
            else{Some([self.chunks.len(),self.chunks.len()])};
        serde_json::json!({
            "scope": "One census at final benchmark report; current requested buffer capacities, excluding driver bookkeeping, in-flight retired buffers and non-terrain resources",
            "near": {
                "resident_chunks": self.chunks.len(),
                "nonempty_meshes": nonempty,
                "owned_vertex_buffers": owned.map(|v|v[0]),
                "vertex_handle_references":self.chunks.len(),
                "owned_index_buffers": owned.map(|v|v[1]),
                "index_handle_references":self.chunks.len(),
                "vertex_capacity_bytes": vertices,
                "index_capacity_bytes": indices,
                "total_capacity_bytes": vertices + indices,
                "capacity_scope":if self.near_arena.is_some(){"sum of chunk reservations plus dedicated capacities; NOT unique GPU page bytes; use near_arena.unique_shared_committed_bytes"}else{"unique dedicated resident buffers"},
                "index_count": index_count,
                "uint16_meshes": uint16,
                "uint32_meshes": uint32,
            },
            "near_bundles": {
                "cached_eye_pairs": self.terrain_bundles.eyes.len(),
                "cached_bundles": self.terrain_bundles.eyes.len() * 2,
            },
            "far_bundles": self.distant.bundles.allocation_report(),
        })
    }
    pub(crate) fn frame_profile_report(&self) -> serde_json::Value {
        self.render_profile.report()
    }
    pub(crate) fn configure_surface_wait_trace(&mut self, enabled: bool) -> Result<()> {
        // No device feature negotiation, graphics changes or global subscriber.
        // A new Renderer owns a distinct collector identity (not a native
        // Device ID). Pack reload is
        // rejected while this explicit diagnostic is requested, rather than
        // silently replacing a live collector and losing its measured rows.
        anyhow::ensure!(self.surface_wait_trace.is_none(), "Surface-wait capture is already configured");
        if enabled {
            anyhow::ensure!(self.gpu_timestamps.is_none(), "Surface-wait and GPU timestamp captures are separate diagnostics");
            self.surface_wait_trace = Some(surface_wait_trace::Capture::new()?);
        }
        Ok(())
    }
    pub(crate) fn surface_wait_trace_presented(&mut self, sample_index: Option<usize>,
        begin_application_ms: Option<f64>, end_application_ms: f64) -> Result<()> {
        if let Some(capture) = &mut self.surface_wait_trace {
            capture.presented(sample_index, begin_application_ms, end_application_ms)?;
        }
        Ok(())
    }
    pub(crate) fn surface_wait_trace_report(&self) -> serde_json::Value {
        self.surface_wait_trace.as_ref().map_or_else(surface_wait_trace::disabled_report,
            surface_wait_trace::Capture::report)
    }
    pub(crate) fn gpu_timestamps_presented(&mut self, sample_index: Option<usize>,
        begin_application_ms: Option<f64>, end_application_ms: f64) -> Result<()> {
        if let Some(capture) = &mut self.gpu_timestamps {
            capture.presented(sample_index, begin_application_ms, end_application_ms)?;
        }
        Ok(())
    }
    pub(crate) fn finish_gpu_timestamps(&mut self) -> Result<()> {
        if let Some(capture) = &mut self.gpu_timestamps { capture.finish(&self.device)?; }
        Ok(())
    }
    pub(crate) fn gpu_timestamps_report(&self) -> serde_json::Value {
        self.gpu_timestamps.as_ref().map_or_else(|| serde_json::json!({"enabled": false}),
            gpu_timestamps::Capture::report)
    }
    fn gpu_timestamp_render_error(error: anyhow::Error) -> wgpu::SurfaceError {
        // An explicitly requested diagnostic must abort on invalid ownership or
        // recording, rather than publish a partial capture as a successful run.
        eprintln!("GPU timestamp diagnostic failed: {error:#}");
        wgpu::SurfaceError::Other
    }
    pub fn render(&mut self, input: &RenderInput<'_>) -> Result<(), wgpu::SurfaceError> {
        if let Some(capture) = &mut self.gpu_timestamps { capture.clear_pending_present(); }
        if let Some(capture) = &mut self.surface_wait_trace { capture.clear_pending_present(); }
        let mut profile = self.render_profile.begin(input.time);
        self.transparency_capture.frame(profile.frame_id,input.time);
        let profile_started = profile.timer();
        let thread_started = profile.thread_cpu();
        self.render_cpu_ms = [0.; 4];
        // Explicit trial only: derived camera/visibility, with no GPU or
        // history-sensitive sort state touched before successful acquire.
        let prepared_view = self.prepare_pre_acquire_visibility(input,&mut profile);
        let started = std::time::Instant::now();
        let frame = if profile.enabled {
            if let Some(capture) = &mut self.surface_wait_trace {
                // input.time remains f32-derived context only. Exact successful
                // present anchors are attached by App after render returns.
                capture.acquire(profile.frame_id, f64::from(input.time),
                    || self.surface.as_ref().unwrap().get_current_texture())
            } else { self.surface.as_ref().unwrap().get_current_texture() }
        } else if let Some(capture) = &mut self.surface_wait_trace {
            // Exercise the same scoped dispatcher on real warmup acquisitions;
            // only separate warmup counts change, with no callback clock reads.
            capture.warmup_acquire(profile.frame_id, f64::from(input.time),
                || self.surface.as_ref().unwrap().get_current_texture())
        } else { self.surface.as_ref().unwrap().get_current_texture() };
        self.render_cpu_ms[0] = started.elapsed().as_secs_f64() * 1000.;
        if profile.enabled {
            if let Some(capture) = &mut self.surface_wait_trace {
                // Reuse the original public acquire measurement; no new outer
                // clock or attribution of its residual to OS/GPU/driver waits.
                let validation = capture.public_elapsed(self.render_cpu_ms[0]);
                // Keep a native Lost/Outdated/Timeout error authoritative. A
                // malformed diagnostic after native success aborts explicitly;
                // the collector retains its invalid/lifecycle counters.
                if validation.is_err() && frame.is_ok() {
                    return self.finish_pre_acquire_visibility(prepared_view.is_some(),Err(wgpu::SurfaceError::Other));
                }
            }
        }
        let frame = self.finish_pre_acquire_visibility(prepared_view.is_some(),frame)?;
        let gpu_ticket = if profile.enabled {
            self.gpu_timestamps.as_mut().and_then(|capture| capture.reserve(profile.frame_id, input.time))
        } else { None };
        let started = std::time::Instant::now();
        let encode_cpu_started = profile.thread_cpu();
        let encoder_started = profile.timer();
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        profile.stages_ms.encoder_create = render_profile::elapsed(encoder_started);
        let arena_owner=self.near_arena.as_ref().map(Arc::clone);
        let mut arena_state=arena_owner.as_ref().map(|state|state.lock().expect("near arena owner poisoned"));
        let mut arena_session=arena_state.as_mut().map(|state|state.frame());
        let staging_owner=self.transparent_staging.as_ref().map(Arc::clone);
        let mut staging_state=staging_owner.as_ref().map(|state|state.lock().expect("staging owner poisoned"));
        let mut staging_session=staging_state.as_mut().map(|state|state.frame(input.settings.render_distance==128)
            .expect("transparent staging frame rejected"));
        if let (Some(capture), Some(ticket)) = (&mut self.gpu_timestamps, gpu_ticket.as_ref()) {
            capture.encode_start(&mut encoder, ticket).map_err(Self::gpu_timestamp_render_error)?;
        }
        self.encode_inner(&mut encoder,&view,input,&mut profile,arena_session.as_mut(),staging_session.as_mut(),prepared_view);
        if let Some(session)=staging_session.as_mut(){session.finish().expect("staging finish failed closed");}
        if let (Some(capture), Some(ticket)) = (&mut self.gpu_timestamps, gpu_ticket.as_ref()) {
            capture.encode_end(&mut encoder, ticket).map_err(Self::gpu_timestamp_render_error)?;
        }
        self.render_cpu_ms[1] = started.elapsed().as_secs_f64() * 1000.;
        profile.stages_ms.finish(self.render_cpu_ms[1]);
        profile.encode_thread_cpu_ms = render_profile::cpu_elapsed(encode_cpu_started, profile.thread_cpu());
        let started = std::time::Instant::now();
        self.queue.submit(Some(encoder.finish()));
        if let Some(session)=arena_session.take(){session.submitted();}
        if let Some(session)=staging_session.take(){session.submitted().expect("staging submit acknowledgement failed");}
        self.render_cpu_ms[2] = started.elapsed().as_secs_f64() * 1000.;
        let started = std::time::Instant::now();
        frame.present();
        self.render_cpu_ms[3] = started.elapsed().as_secs_f64() * 1000.;
        profile.render_wall_ms = render_profile::elapsed(profile_started);
        profile.render_thread_cpu_ms = render_profile::cpu_elapsed(thread_started, profile.thread_cpu());
        [profile.surface_acquire_ms, profile.encode_ms, profile.finish_and_submit_ms, profile.present_ms] = self.render_cpu_ms;
        profile.counters.visible_chunks = self.visible_chunks;
        profile.counters.distant_visible_tiles = self.distant.visible;
        profile.counters.visible_entities = self.visible_entities;
        profile.counters.total_entities = self.total_entities;
        if let (Some(capture), Some(ticket)) = (&mut self.gpu_timestamps, gpu_ticket) {
            capture.submitted(ticket, gpu_timestamps::CpuTimes {
                acquire_ms: self.render_cpu_ms[0], encode_ms: self.render_cpu_ms[1],
                submit_ms: self.render_cpu_ms[2], present_ms: self.render_cpu_ms[3],
            }).map_err(Self::gpu_timestamp_render_error)?;
        }
        self.render_profile.finish(profile);
        Ok(())
    }
    pub fn screenshot(&mut self, path: &Path, input: &RenderInput<'_>) -> Result<()> {
        let [w, h] = self.resolution();
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("screenshot"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let row = (w * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: row as u64 * h as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let arena_owner=self.near_arena.as_ref().map(Arc::clone);
        let mut arena_state=arena_owner.as_ref().map(|state|state.lock().expect("near arena owner poisoned"));
        let mut arena_session=arena_state.as_mut().map(|state|state.frame());
        let staging_owner=self.transparent_staging.as_ref().map(Arc::clone);
        let mut staging_state=staging_owner.as_ref().map(|state|state.lock().expect("staging owner poisoned"));
        let mut staging_session=staging_state.as_mut().map(|state|state.frame(input.settings.render_distance==128)
            .expect("transparent staging frame rejected"));
        self.encode_inner(&mut encoder,&texture.create_view(&Default::default()),input,
            &mut render_profile::Frame::default(),arena_session.as_mut(),staging_session.as_mut(),None);
        if let Some(session)=staging_session.as_mut(){session.finish().expect("staging screenshot finish failed closed");}
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        if let Some(session)=arena_session.take(){session.submitted();}
        if let Some(session)=staging_session.take(){session.submitted().expect("staging submit acknowledgement failed");}
        let (tx, rx) = std::sync::mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv()??;
        let mapped = buffer.slice(..).get_mapped_range();
        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        for line in mapped.chunks_exact(row as usize) {
            pixels.extend_from_slice(&line[..(w * 4) as usize]);
        }
        if matches!(
            self.config.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        image::save_buffer(path, &pixels, w, h, image::ColorType::Rgba8)?;
        drop(mapped);
        buffer.unmap();
        println!("Screenshot: {}", path.display());
        Ok(())
    }
}

#[cfg(test)]
#[path="sky_gpu_tests.rs"]
mod sky_gpu_tests;
