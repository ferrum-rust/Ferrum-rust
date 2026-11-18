use super::{GpuMesh, Vertex, DEPTH};

struct Pipelines {
    outline: wgpu::RenderPipeline,
    damage: wgpu::RenderPipeline,
}

pub(super) struct Overlays {
    eyes: [Pipelines; 3],
    pub(super) cracks: GpuMesh,
}

impl Overlays {
    pub(super) fn new(
        device: &wgpu::Device,
        layout: &wgpu::PipelineLayout,
        shader: &wgpu::ShaderModule,
        format: wgpu::TextureFormat,
    ) -> Self {
        use wgpu::{BlendComponent, BlendFactor, BlendOperation, BlendState, ColorWrites};
        let damage_blend = BlendState {
            color: BlendComponent {
                src_factor: BlendFactor::Dst,
                dst_factor: BlendFactor::Src,
                operation: BlendOperation::Add,
            },
            alpha: BlendComponent::REPLACE,
        };
        let vertices = [wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Uint32,3=>Unorm8x4,4=>Unorm8x4],
        }];
        let eyes = [
            ColorWrites::ALL,
            ColorWrites::RED | ColorWrites::ALPHA,
            ColorWrites::GREEN | ColorWrites::BLUE | ColorWrites::ALPHA,
        ]
        .map(|write_mask| {
            let make = |outline| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(if outline {
                        "selected block outline"
                    } else {
                        "block damage"
                    }),
                    layout: Some(layout),
                    vertex: wgpu::VertexState {
                        module: shader,
                        entry_point: Some(if outline { "vs_outline" } else { "vs_main" }),
                        buffers: if outline { &[] } else { &vertices },
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: shader,
                        entry_point: Some(if outline {
                            "fs_outline"
                        } else {
                            "fs_block_damage"
                        }),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            write_mask,
                            blend: Some(if outline {
                                BlendState::ALPHA_BLENDING
                            } else {
                                damage_blend
                            }),
                        })],
                        compilation_options: Default::default(),
                    }),
                    primitive: wgpu::PrimitiveState {
                        cull_mode: if outline {
                            None
                        } else {
                            Some(wgpu::Face::Back)
                        },
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH,
                        depth_write_enabled: false,
                        depth_compare: wgpu::CompareFunction::LessEqual,
                        stencil: Default::default(),
                        bias: if outline {
                            Default::default()
                        } else {
                            wgpu::DepthBiasState {
                                constant: -3,
                                slope_scale: -3.,
                                clamp: 0.,
                            }
                        },
                    }),
                    multisample: Default::default(),
                    multiview: None,
                    cache: None,
                })
            };
            Pipelines {
                outline: make(true),
                damage: make(false),
            }
        });
        Self {
            eyes,
            cracks: GpuMesh::new(device),
        }
    }

    pub(super) fn draw<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        bind: &'a wgpu::BindGroup,
        eye: usize,
        outline: bool,
    ) {
        if !outline && self.cracks.count == 0 {
            return;
        }
        pass.set_bind_group(0, bind, &[]);
        if outline {
            pass.set_pipeline(&self.eyes[eye].outline);
            pass.draw(0..12 * 6, 0..1);
        }
        if self.cracks.count > 0 {
            pass.set_pipeline(&self.eyes[eye].damage);
            self.cracks.draw(pass);
        }
    }
}
