//! Fixed-function celestial draws from the Overworld RenderGlobal sky pass.
use crate::{
    resources::Resources,
    vanilla::{lightmap, math, random::JavaRandom},
};
use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use std::ops::Range;
use wgpu::util::DeviceExt;

pub struct Frame {
    pub projection_view: Mat4,
    pub time: i64,
    pub partial: f32,
    pub rain: f32,
    pub sunrise: Option<[f32; 4]>,
    /// 0 = ordinary, 1 = red eye, 2 = cyan eye.
    pub eye: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    uv: [f32; 2],
    kind: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniform {
    projection_view: [[f32; 4]; 4],
    orbit: [[f32; 4]; 4],
    center: [f32; 4],
    rim: [f32; 4],
    // Original unquantized sunrise alpha controls fan geometry.
    state: [f32; 4],
    sprites: [u32; 4],
    extent: [f32; 4],
}

fn quantized(color: [f32; 4]) -> [f32; 4] {
    color.map(|v| (v * 255.) as u8 as f32 / 255.)
}

impl Frame {
    fn uniforms(&self, textures: [u32; 2], extent: [f32; 4]) -> Uniform {
        let angle = lightmap::celestial_angle(self.time, self.partial, 0);
        let radians = angle * std::f32::consts::PI * 2.;
        let brightness = (1. - (math::cos(radians) * 2. + 0.25)).clamp(0., 1.);
        let opacity = 1. - self.rain;
        let sun = self.sunrise.unwrap_or([0.; 4]);
        let [r, g, b, a] = sun;
        let center = if self.eye == 0 {
            sun
        } else {
            [
                (r * 30. + g * 59. + b * 11.) / 100.,
                (r * 30. + g * 70.) / 100.,
                (r * 30. + b * 70.) / 100.,
                a,
            ]
        };
        let phase = (self.time / 24000).rem_euclid(8) as u32;
        Uniform {
            projection_view: self.projection_view.to_cols_array_2d(),
            orbit: (Mat4::from_rotation_y(-std::f32::consts::FRAC_PI_2)
                * Mat4::from_rotation_x((angle * 360.).to_radians()))
            .to_cols_array_2d(),
            center: quantized(center),
            rim: quantized([r, g, b, 0.]),
            state: [
                a,
                if math::sin(radians) < 0. { -1. } else { 1. },
                opacity,
                brightness * brightness * 0.5 * opacity,
            ],
            sprites: [textures[0], textures[1], phase % 4, phase / 4],
            extent,
        }
    }
}

struct Geometry {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    fan: Range<u32>,
    bodies: Range<u32>,
    stars: Range<u32>,
}
impl Geometry {
    fn quad(&mut self, kind: u32, points: [[f32; 3]; 4], uvs: [[f32; 2]; 4]) {
        let first = self.vertices.len() as u32;
        self.vertices
            .extend(points.into_iter().zip(uvs).map(|(position, uv)| Vertex {
                position,
                uv,
                kind,
            }));
        self.indices.extend([0, 1, 2, 0, 2, 3].map(|i| first + i));
    }
    fn new() -> Self {
        let mut out = Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            fan: 0..0,
            bodies: 0..0,
            stars: 0..0,
        };
        out.vertices.push(Vertex {
            position: [0., 100., 0.],
            uv: [0.; 2],
            kind: 0,
        });
        for i in 0..=16 {
            let a = i as f32 * std::f32::consts::PI * 2. / 16.;
            out.vertices.push(Vertex {
                position: [
                    math::sin(a) * 120.,
                    math::cos(a) * 120.,
                    -math::cos(a) * 40.,
                ],
                uv: [0.; 2],
                kind: 1,
            });
            if i > 0 {
                out.indices.extend([0, i, i + 1]);
            }
        }
        out.fan = 0..out.indices.len() as u32;
        out.quad(
            2,
            [
                [-30., 100., -30.],
                [30., 100., -30.],
                [30., 100., 30.],
                [-30., 100., 30.],
            ],
            [[0., 0.], [1., 0.], [1., 1.], [0., 1.]],
        );
        out.quad(
            3,
            [
                [-20., -100., 20.],
                [20., -100., 20.],
                [20., -100., -20.],
                [-20., -100., -20.],
            ],
            [[1., 1.], [0., 1.], [0., 0.], [1., 0.]],
        );
        out.bodies = out.fan.end..out.indices.len() as u32;
        let mut random = JavaRandom::new(10842);
        for _ in 0..1500 {
            let [mut x, mut y, mut z] =
                std::array::from_fn(|_| f64::from(random.next_float() * 2. - 1.));
            let size = f64::from(0.15 + random.next_float() * 0.1);
            let len = x * x + y * y + z * z;
            if !(len < 1. && len > 0.01) {
                continue;
            }
            let scale = 1. / len.sqrt();
            x *= scale;
            y *= scale;
            z *= scale;
            let (az_s, az_c) = x.atan2(z).sin_cos();
            let (el_s, el_c) = (x * x + z * z).sqrt().atan2(y).sin_cos();
            let (roll_s, roll_c) = (random.next_double() * std::f64::consts::PI * 2.).sin_cos();
            let points = std::array::from_fn(|j| {
                let u = (((j & 2) as i32) - 1) as f64 * size;
                let v = ((((j + 1) & 2) as i32) - 1) as f64 * size;
                let a = u * roll_c - v * roll_s;
                let b = v * roll_c + u * roll_s;
                let h = -a * el_c;
                [
                    (x * 100. + (h * az_s - b * az_c)) as f32,
                    (y * 100. + a * el_s) as f32,
                    (z * 100. + (b * az_s + h * az_c)) as f32,
                ]
            });
            out.quad(4, points, [[0.; 2]; 4]);
        }
        out.stars = out.bodies.end..out.indices.len() as u32;
        out
    }
}
pub struct Celestial {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    fan: Range<u32>,
    bodies: Range<u32>,
    stars: Range<u32>,
    textures: [u32; 2],
    extent: [f32; 4],
    uniforms: [wgpu::Buffer; 3],
    groups: [wgpu::BindGroup; 3],
    fan_pipelines: [wgpu::RenderPipeline; 3],
    body_pipelines: [wgpu::RenderPipeline; 3],
}

impl Celestial {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        depth: wgpu::TextureFormat,
        atlas: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        resources: &Resources,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Overworld sun, moon and stars"),
            source: wgpu::ShaderSource::Wgsl(include_str!("celestial.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("celestial bindings"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("celestial layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipes = |dst| {
            std::array::from_fn(|eye| {
                let mask = [
                    wgpu::ColorWrites::ALL,
                    wgpu::ColorWrites::RED,
                    wgpu::ColorWrites::GREEN | wgpu::ColorWrites::BLUE,
                ][eye];
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {label:Some("celestial composition"),layout:Some(&pipeline_layout),
                vertex:wgpu::VertexState{module:&shader,entry_point:Some("vs"),compilation_options:Default::default(),buffers:&[wgpu::VertexBufferLayout{array_stride:std::mem::size_of::<Vertex>() as u64,step_mode:wgpu::VertexStepMode::Vertex,attributes:&wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Uint32]}]},
                fragment:Some(wgpu::FragmentState{module:&shader,entry_point:Some("fs"),compilation_options:Default::default(),targets:&[Some(wgpu::ColorTargetState{format,write_mask:mask,blend:Some(wgpu::BlendState{color:wgpu::BlendComponent{src_factor:wgpu::BlendFactor::SrcAlpha,dst_factor:dst,operation:wgpu::BlendOperation::Add},alpha:wgpu::BlendComponent::REPLACE})})]}),
                primitive:Default::default(),depth_stencil:Some(wgpu::DepthStencilState{format:depth,depth_write_enabled:false,depth_compare:wgpu::CompareFunction::LessEqual,stencil:Default::default(),bias:Default::default()}),multisample:Default::default(),multiview:None,cache:None})
            })
        };
        let fan_pipelines = pipes(wgpu::BlendFactor::OneMinusSrcAlpha);
        let body_pipelines = pipes(wgpu::BlendFactor::One);
        let uniforms = std::array::from_fn(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("celestial eye"),
                size: std::mem::size_of::<Uniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let groups = std::array::from_fn(|i| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("celestial eye textures"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniforms[i].as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(atlas),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            })
        });
        let g = Geometry::new();
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("fixed sky geometry"),
            contents: bytemuck::cast_slice(&g.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("fixed sky triangles"),
            contents: bytemuck::cast_slice(&g.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let [sun, moon] = [
            "textures/environment/sun.png",
            "textures/environment/moon_phases.png",
        ]
        .map(|name| resources.sprite(name));
        Self {
            vertices,
            indices,
            fan: g.fan,
            bodies: g.bodies,
            stars: g.stars,
            textures: [sun.layer, moon.layer],
            extent: [
                sun.width as f32 / 256.,
                sun.height as f32 / 256.,
                moon.width as f32 / 256.,
                moon.height as f32 / 256.,
            ],
            uniforms,
            groups,
            fan_pipelines,
            body_pipelines,
        }
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, queue: &wgpu::Queue, frame: Frame) {
        let eye = frame.eye;
        let data = frame.uniforms(self.textures, self.extent);
        queue.write_buffer(&self.uniforms[eye], 0, bytemuck::bytes_of(&data));
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.set_bind_group(0, &self.groups[eye], &[]);
        if frame.sunrise.is_some() {
            pass.set_pipeline(&self.fan_pipelines[eye]);
            pass.draw_indexed(self.fan.clone(), 0, 0..1);
        }
        pass.set_pipeline(&self.body_pipelines[eye]);
        let end = if data.state[3] > 0. {
            self.stars.end
        } else {
            self.bodies.end
        };
        pass.draw_indexed(self.bodies.start..end, 0, 0..1);
    }
}
#[cfg(test)]
#[path = "celestial_tests.rs"]
mod tests;

