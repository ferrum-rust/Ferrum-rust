//! GuiMainMenu's 256-pixel cube capture, rotated blur copies and final viewport crop.
use crate::resources::Resources;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniform {
    crop: [f32; 4],
    rotation: [f32; 4],
    a: [u32; 4],
    b: [u32; 4],
}
pub(super) struct TitlePanorama {
    cube: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    blit: wgpu::RenderPipeline,
    groups: [wgpu::BindGroup; 2],
    targets: [wgpu::TextureView; 2],
    uniform: wgpu::Buffer,
    layers: [u32; 6],
}
impl TitlePanorama {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        atlas: &wgpu::TextureView,
        resources: &Resources,
    ) -> Self {
        let targets = std::array::from_fn(|_| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("title 256px framebuffer"),
                    size: wgpu::Extent3d {
                        width: 256,
                        height: 256,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        });
        let mut image = super::tex_entry(3);
        image.ty = wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("title panorama bindings"),
            entries: &[
                super::uniform_entry(0),
                super::tex_entry(1),
                super::sampler_entry(2),
                image,
            ],
        });
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("title camera"),
            contents: bytemuck::bytes_of(&Uniform::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            min_filter: wgpu::FilterMode::Linear,
            mag_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            ..Default::default()
        });
        let groups = std::array::from_fn(|index| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("title framebuffer inputs"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(atlas),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&targets[index]),
                    },
                ],
            })
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("title framebuffer pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("title panorama"),
            source: wgpu::ShaderSource::Wgsl(include_str!("panorama.wgsl").into()),
        });
        let pipeline = |entry, format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vertex"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    targets: &[Some(format.into())],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        Self {
            cube: pipeline("cube", wgpu::TextureFormat::Rgba8Unorm),
            blur: pipeline("blur", wgpu::TextureFormat::Rgba8Unorm),
            blit: pipeline("blit", format),
            uniform,
            targets,
            groups,
            layers: std::array::from_fn(|i| {
                resources
                    .sprite(&format!("textures/gui/title/background/panorama_{i}.png"))
                    .layer
            }),
        }
    }
    fn pass<'a>(
        encoder: &'a mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) -> wgpu::RenderPass<'a> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("title panorama"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        })
    }
    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        queue: &wgpu::Queue,
        output: &wgpu::TextureView,
        size: [f32; 2],
        time: Option<f32>,
    ) {
        let Some(seconds) = time else {
            drop(Self::pass(encoder, output));
            return;
        };
        let ticks = seconds * 20.;
        let (sp, cp) = (crate::vanilla::math::sin(ticks / 400.) * 25. + 20.)
            .to_radians()
            .sin_cos();
        let (sy, cy) = (-ticks * 0.1).to_radians().sin_cos();
        let [w, h] = size;
        let crop = 120. / (256. * w.max(h));
        let [a, b, c, d, e, f] = self.layers;
        let uniform = Uniform {
            crop: [h * crop, w * crop, 0., 0.],
            rotation: [sp, cp, sy, cy],
            a: [a, b, c, d],
            b: [e, f, 0, 0],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        {
            let mut pass = Self::pass(encoder, &self.targets[0]);
            pass.set_pipeline(&self.cube);
            pass.set_bind_group(0, &self.groups[1], &[]);
            pass.draw(0..3, 0..1);
        }
        // The seventh glCopy captures the sixth blurred result; the last draw to the old framebuffer is discarded.
        for i in 0..6 {
            let mut pass = Self::pass(encoder, &self.targets[(i + 1) % 2]);
            pass.set_pipeline(&self.blur);
            pass.set_bind_group(0, &self.groups[i % 2], &[]);
            pass.draw(0..3, 0..1);
        }
        let mut pass = Self::pass(encoder, output);
        pass.set_pipeline(&self.blit);
        pass.set_bind_group(0, &self.groups[0], &[]);
        pass.draw(0..3, 0..1);
    }
}
