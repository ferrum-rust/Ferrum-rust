use super::*;
use std::{
    io::{Cursor, Read},
    path::Path,
};

const PATTERN: &str = r#"
struct Config { frame: u32 }
@group(0) @binding(0) var<uniform> config:Config;
@vertex fn vs(@builtin(vertex_index) i:u32)->@builtin(position) vec4<f32>{
    let xy=vec2<f32>(f32((i<<1u)&2u),f32(i&2u));
    return vec4<f32>(xy*2.0-1.0,0.0,1.0);
}
@fragment fn fs(@builtin(position) pos:vec4<f32>)->@location(0) vec4<f32>{
    let x=u32(pos.x);let y=u32(pos.y);let n=config.frame;
    return vec4<f32>(f32(x*4u+n*17u),f32(y*6u+n*13u),f32(((x/5u+y/4u)%2u)*95u+30u+n*11u),255.)/255.;
}"#;

#[test]
#[ignore = "requires local 1.8.9 shaders and Vulkan"]
fn installed_super_secret_graphs_against_original_framebuffers() -> Result<()> {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(
        &include_bytes!("../tests/fixtures/vanilla189-post-effects.rgb.gz")[..],
    )
    .read_to_end(&mut bytes)?;
    let mut reference = Cursor::new(bytes.as_slice());
    let mut wh = [0; 2];
    reference.read_exact(&mut wh)?;
    let size = wh.map(u32::from);
    let mut name_len = [0; 2];
    reference.read_exact(&mut name_len)?;
    let mut renderer = vec![0; usize::from(u16::from_be_bytes(name_len))];
    reference.read_exact(&mut renderer)?;
    let renderer = String::from_utf8(renderer)?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..Default::default()
    });
    let adapters = instance.enumerate_adapters(wgpu::Backends::VULKAN);
    let adapter = adapters
        .iter()
        .find(|a| a.get_info().name.eq_ignore_ascii_case(&renderer))
        .or_else(|| adapters.first())
        .context("Vulkan adapter")?;
    let matched_gpu = adapter.get_info().name.eq_ignore_ascii_case(&renderer);
    eprintln!("Vulkan comparison adapter: {}", adapter.get_info().name);
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default(), None))?;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("test pattern"),
        source: wgpu::ShaderSource::Wgsl(PATTERN.into()),
    });
    let pattern = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("test pattern"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
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
    let frame_cfg = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pattern.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: frame_cfg.as_entire_binding(),
        }],
    });
    let output = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: extent(size),
        format,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = output.create_view(&Default::default());
    let row = (size[0] * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * size[1]),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let assets = Path::new(".local/resources/assets/minecraft");
    let mut failures = Vec::new();
    for (index, name) in PRESETS.iter().enumerate() {
        let mut len = [0; 2];
        reference.read_exact(&mut len)?;
        let mut expected_name = vec![0; usize::from(u16::from_be_bytes(len))];
        reference.read_exact(&mut expected_name)?;
        assert_eq!(&expected_name, name.as_bytes());
        let mut graph = Graph::load(&device, &queue, format, size, index, |path| {
            Ok(std::fs::read(assets.join(path))?)
        })?;
        for frame in 0u32..2 {
            queue.write_buffer(&frame_cfg, 0, bytemuck::bytes_of(&frame));
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: graph.main_view(),
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
                pass.set_pipeline(&pattern);
                pass.set_bind_group(0, &group, &[]);
                pass.draw(0..3, 0..1);
            }
            graph.draw(&queue, &mut encoder, &view, 0.25 + frame as f32 * 0.5);
            encoder.copy_texture_to_buffer(
                output.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row),
                        rows_per_image: Some(size[1]),
                    },
                },
                extent(size),
            );
            queue.submit(Some(encoder.finish()));
            let (tx, rx) = std::sync::mpsc::channel();
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = tx.send(result);
                });
            device.poll(wgpu::Maintain::Wait);
            rx.recv()??;
            let pixels = buffer.slice(..).get_mapped_range();
            let mut sum = 0u64;
            let mut maximum = 0u8;
            let mut outside = 0;
            for rgba in pixels
                .chunks_exact(row as usize)
                .flat_map(|r| r[..size[0] as usize * 4].as_chunks::<4>().0)
            {
                let mut expected = [0; 3];
                reference.read_exact(&mut expected)?;
                for (a, b) in rgba.iter().zip(expected) {
                    let diff = a.abs_diff(b);
                    sum += u64::from(diff);
                    maximum = maximum.max(diff);
                    outside += u32::from(diff > 2);
                }
            }
            drop(pixels);
            buffer.unmap();
            let mean = sum as f64 / f64::from(size[0] * size[1] * 3);
            eprintln!(
                "{name} frame={frame} mean={mean:.4} max={maximum} channels_over_two={outside}"
            );
            if (matched_gpu && maximum > 2) || (!matched_gpu && mean > 5.) {
                failures.push((name, frame, mean, maximum));
            }
        }
    }
    assert_eq!(reference.position(), bytes.len() as u64);
    assert!(
        failures.is_empty(),
        "Original framebuffer differences: {failures:?}"
    );
    Ok(())
}
