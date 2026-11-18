//! The original Super Secret Settings pass graphs, evaluated between world rendering and GUI.
use anyhow::{bail, ensure, Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
#[path = "postfx_source.rs"]
mod source;
use source::{ProgramInputs, Values};
#[cfg(test)]
#[path = "postfx_tests.rs"]
mod tests;

pub(super) const PRESETS: [&str; 24] = [
    "notch",
    "fxaa",
    "art",
    "bumpy",
    "blobs2",
    "pencil",
    "color_convolve",
    "deconverge",
    "flip",
    "invert",
    "ntsc",
    "outline",
    "phosphor",
    "scan_pincushion",
    "sobel",
    "bits",
    "desaturate",
    "green",
    "blur",
    "wobble",
    "blobs",
    "antialias",
    "creeper",
    "spider",
];

#[derive(Deserialize)]
struct Chain {
    targets: Vec<String>,
    passes: Vec<PassDef>,
}
#[derive(Deserialize)]
struct PassDef {
    name: String,
    intarget: String,
    outtarget: String,
    #[serde(default)]
    auxtargets: Vec<Aux>,
    #[serde(default)]
    uniforms: Vec<Uniform>,
}
#[derive(Deserialize)]
struct Aux {
    name: String,
    id: String,
    #[serde(default)]
    bilinear: bool,
}
#[derive(Deserialize)]
struct Program {
    vertex: String,
    fragment: String,
    samplers: Vec<Sampler>,
    uniforms: Vec<Uniform>,
    #[serde(default)]
    blend: Blend,
}
#[derive(Deserialize, Default)]
struct Blend {
    #[serde(default)]
    srcrgb: String,
    #[serde(default)]
    dstrgb: String,
}
#[derive(Deserialize)]
struct Uniform {
    name: String,
    values: Vec<f32>,
    #[serde(default)]
    count: usize,
}
#[derive(Deserialize)]
struct Sampler {
    name: String,
}

fn resource_name(s: &str) -> Result<&str> {
    ensure!(
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
        "Invalid shader resource name {s:?}"
    );
    Ok(s)
}
fn target(device: &wgpu::Device, format: wgpu::TextureFormat, size: [u32; 2]) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("post-processing framebuffer"),
            size: extent(size),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}
fn extent([width, height]: [u32; 2]) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    }
}

enum Image {
    Target(usize),
    Resource(wgpu::TextureView),
}
struct Pass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    output: usize,
    uniforms: wgpu::Buffer,
    vars: ProgramInputs,
    images: Vec<(Image, wgpu::Sampler)>,
    group: wgpu::BindGroup,
}
fn bindings(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    images: &[(Image, wgpu::Sampler)],
    targets: &[wgpu::TextureView],
) -> wgpu::BindGroup {
    let mut entries = vec![wgpu::BindGroupEntry {
        binding: 0,
        resource: uniforms.as_entire_binding(),
    }];
    for (i, (img, sampler)) in images.iter().enumerate() {
        let view = match img {
            Image::Target(k) => &targets[*k],
            Image::Resource(v) => v,
        };
        entries.push(wgpu::BindGroupEntry {
            binding: i as u32 * 2 + 1,
            resource: wgpu::BindingResource::TextureView(view),
        });
        entries.push(wgpu::BindGroupEntry {
            binding: i as u32 * 2 + 2,
            resource: wgpu::BindingResource::Sampler(sampler),
        });
    }
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("post-processing inputs"),
        layout,
        entries: &entries,
    })
}

pub(super) struct Graph {
    pub index: usize,
    pub enabled: bool,
    targets: Vec<wgpu::TextureView>,
    passes: Vec<Pass>,
    format: wgpu::TextureFormat,
    size: [u32; 2],
    copy: wgpu::RenderPipeline,
    copy_layout: wgpu::BindGroupLayout,
    copy_group: wgpu::BindGroup,
    previous_partial: f32,
    accumulated: f32,
}

impl Graph {
    pub fn main_view(&self) -> &wgpu::TextureView {
        &self.targets[0]
    }
    pub fn load(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        size: [u32; 2],
        index: usize,
        read: impl Fn(&str) -> Result<Vec<u8>>,
    ) -> Result<Self> {
        let preset = PRESETS[index];
        let chain: Chain = serde_json::from_slice(&read(&format!("shaders/post/{preset}.json"))?)?;
        ensure!(
            chain.targets.len() <= 32 && chain.passes.len() <= 64,
            "Shader graph exceeds limits"
        );
        let mut ids = HashMap::from([("minecraft:main".to_owned(), 0usize)]);
        let mut targets = vec![target(device, format, size)];
        for name in chain.targets {
            ensure!(
                ids.insert(name, targets.len()).is_none(),
                "Duplicate shader target"
            );
            targets.push(target(device, format, size));
        }
        let mut passes = Vec::new();
        for pass in chain.passes {
            let input = *ids
                .get(&pass.intarget)
                .context("Unknown post-processing input")?;
            let output = *ids
                .get(&pass.outtarget)
                .context("Unknown post-processing output")?;
            let name = resource_name(&pass.name)?;
            let program: Program =
                serde_json::from_slice(&read(&format!("shaders/program/{name}.json"))?)?;
            let mut vars = ProgramInputs::new();
            let mut varyings = BTreeMap::new();
            let mut names = Vec::new();
            let mut images = Vec::new();
            for sampler in &program.samplers {
                resource_name(&sampler.name)?;
                let (img, repeat, linear) = if sampler.name == "DiffuseSampler" {
                    (Image::Target(input), false, false)
                } else {
                    let aux = pass
                        .auxtargets
                        .iter()
                        .find(|a| a.name == sampler.name)
                        .context("Missing shader sampler input")?;
                    if let Some(k) = ids.get(&aux.id) {
                        (Image::Target(*k), false, false)
                    } else {
                        let id = resource_name(&aux.id)?;
                        let img =
                            image::load_from_memory(&read(&format!("textures/effect/{id}.png"))?)?
                                .to_rgba8();
                        let size = extent([img.width(), img.height()]);
                        let tex = device.create_texture(&wgpu::TextureDescriptor {
                            label: Some("post effect texture"),
                            size,
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            usage: wgpu::TextureUsages::TEXTURE_BINDING
                                | wgpu::TextureUsages::COPY_DST,
                            view_formats: &[],
                        });
                        queue.write_texture(
                            tex.as_image_copy(),
                            img.as_raw(),
                            wgpu::TexelCopyBufferLayout {
                                offset: 0,
                                bytes_per_row: Some(size.width * 4),
                                rows_per_image: Some(size.height),
                            },
                            size,
                        );
                        (
                            Image::Resource(tex.create_view(&Default::default())),
                            true,
                            aux.bilinear,
                        )
                    }
                };
                ensure!(
                    !matches!(img, Image::Target(k) if k==output),
                    "Shader pass reads its output"
                );
                names.push((sampler.name.clone(), !repeat));
                let address = if repeat {
                    wgpu::AddressMode::Repeat
                } else {
                    wgpu::AddressMode::ClampToEdge
                };
                let filter = if linear {
                    wgpu::FilterMode::Linear
                } else {
                    wgpu::FilterMode::Nearest
                };
                images.push((
                    img,
                    device.create_sampler(&wgpu::SamplerDescriptor {
                        address_mode_u: address,
                        address_mode_v: address,
                        min_filter: filter,
                        mag_filter: filter,
                        ..Default::default()
                    }),
                ));
            }
            let mut shader = |file: &str, vert: bool| -> Result<wgpu::ShaderModule> {
                let ext = if vert { "vsh" } else { "fsh" };
                let bytes = read(&format!("shaders/program/{}.{ext}", resource_name(file)?))?;
                let src = source::stage(
                    std::str::from_utf8(&bytes)?,
                    vert,
                    &mut vars,
                    &mut varyings,
                    &names,
                )?;
                let stage = if vert {
                    wgpu::naga::ShaderStage::Vertex
                } else {
                    wgpu::naga::ShaderStage::Fragment
                };
                Ok(device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(file),
                    source: wgpu::ShaderSource::Glsl {
                        shader: src.into(),
                        stage,
                        defines: Default::default(),
                    },
                }))
            };
            let vertex = shader(&program.vertex, true)?;
            let fragment = shader(&program.fragment, false)?;
            for uniform in program.uniforms.iter().chain(pass.uniforms.iter()) {
                if let Some(slot) = vars.slots.get(&uniform.name) {
                    if uniform.count > 1 && uniform.values.len() == 1 {
                        vars.values[*slot] = [uniform.values[0]; 4];
                    } else {
                        for (out, v) in vars.values[*slot].iter_mut().zip(&uniform.values) {
                            *out = *v;
                        }
                    }
                }
            }
            let mut entries = vec![wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }];
            for i in 0..images.len() as u32 {
                entries.push(wgpu::BindGroupLayoutEntry {
                    binding: i * 2 + 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                });
                entries.push(wgpu::BindGroupLayoutEntry {
                    binding: i * 2 + 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                });
            }
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(name),
                entries: &entries,
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(name),
                bind_group_layouts: &[&layout],
                push_constant_ranges: &[],
            });
            let blend = match (program.blend.srcrgb.as_str(), program.blend.dstrgb.as_str()) {
                ("" | "one", "" | "zero") => None,
                ("srcalpha", "1-srcalpha") => Some(wgpu::BlendState::ALPHA_BLENDING),
                pair => bail!("Unsupported post-processing blend {pair:?}"),
            };
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(name),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &vertex,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &fragment,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                multiview: None,
                cache: None,
            });
            let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("post processing parameters"),
                size: std::mem::size_of::<Values>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = bindings(device, &layout, &uniforms, &images, &targets);
            passes.push(Pass {
                pipeline,
                layout,
                output,
                uniforms,
                vars,
                images,
                group,
            });
        }
        let copy_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("present processed scene"),
            source: wgpu::ShaderSource::Wgsl(COPY.into()),
        });
        let copy = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("present processed scene"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &copy_module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &copy_module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let copy_layout = copy.get_bind_group_layout(0);
        let copy_group = copy_group(device, &copy_layout, &targets[0]);
        Ok(Self {
            index,
            enabled: true,
            targets,
            passes,
            format,
            size,
            copy,
            copy_layout,
            copy_group,
            previous_partial: 0.,
            accumulated: 0.,
        })
    }
    pub fn resize(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        if self.size == size {
            return;
        }
        self.size = size;
        for view in &mut self.targets {
            *view = target(device, self.format, size);
        }
        self.copy_group = copy_group(device, &self.copy_layout, &self.targets[0]);
        for p in &mut self.passes {
            p.group = bindings(device, &p.layout, &p.uniforms, &p.images, &self.targets);
        }
    }
    pub fn draw(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        out: &wgpu::TextureView,
        partial: f32,
    ) {
        let delta = partial - self.previous_partial;
        self.accumulated += if delta < 0. { delta + 1. } else { delta };
        if self.accumulated > 20. {
            self.accumulated -= 20.;
        }
        self.previous_partial = partial;
        let [w, h] = self.size.map(|n| n as f32);
        for p in &mut self.passes {
            p.vars.values[0] = [w, h, 0., 0.];
            for name in ["ScreenSize", "OutSize", "InSize"] {
                if let Some(&i) = p.vars.slots.get(name) {
                    p.vars.values[i] = [w, h, 0., 0.];
                }
            }
            if let Some(&i) = p.vars.slots.get("Time") {
                p.vars.values[i][0] = self.accumulated / 20.;
            }
            queue.write_buffer(&p.uniforms, 0, bytemuck::cast_slice(&p.vars.values));
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("original shader pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.targets[p.output],
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&p.pipeline);
            pass.set_bind_group(0, &p.group, &[]);
            pass.draw(0..4, 0..1);
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("post effects before GUI"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: out,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.copy);
        pass.set_bind_group(0, &self.copy_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
fn copy_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("post effect output"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(view),
        }],
    })
}
const COPY: &str = r#"
@group(0) @binding(0) var scene:texture_2d<f32>;
@vertex fn vs(@builtin(vertex_index) i:u32)->@builtin(position) vec4<f32>{
    let xy=vec2<f32>(f32((i<<1u)&2u),f32(i&2u));
    return vec4<f32>(xy*2.0-1.0,0.0,1.0);
}
@fragment fn fs(@builtin(position) p:vec4<f32>)->@location(0) vec4<f32>{
    return vec4<f32>(textureLoad(scene,vec2<i32>(p.xy),0).rgb,1.0);
}"#;
