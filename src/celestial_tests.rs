use super::*;
use glam::Vec3;
use serde_json::Value;
use std::io::Read;

#[test]
fn original_render_global_star_buffer_and_168_celestial_draw_streams_match() {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(
        include_bytes!("../tests/fixtures/vanilla189-celestial.json.gz").as_slice(),
    )
    .read_to_end(&mut bytes)
    .unwrap();
    let data: Value = serde_json::from_slice(&bytes).unwrap();
    let geometry = Geometry::new();
    let star_vertices = &geometry.vertices[26..];
    let hash = star_vertices
        .iter()
        .flat_map(|v| v.position)
        .flat_map(f32::to_le_bytes)
        .fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        });
    assert_eq!(star_vertices.len(), data[0].as_u64().unwrap() as usize);
    assert_eq!(hash.to_string(), data[1].as_str().unwrap());
    let rows = data[2].as_array().unwrap();
    assert_eq!(rows.len(), 168);
    let f = |v: &Value| v.as_f64().unwrap() as f32;
    for row in rows {
        let time = row[0].as_i64().unwrap();
        let partial = f(&row[1]);
        let rain = f(&row[3]);
        let eye = if row[2] == 2 {
            0
        } else {
            row[2].as_u64().unwrap() as usize + 1
        };
        let sunrise = row[7].as_array().map(|a| std::array::from_fn(|j| f(&a[j])));
        let frame = Frame {
            time,
            partial,
            rain,
            sunrise,
            eye,
            projection_view: Mat4::IDENTITY,
        };
        let uniforms = frame.uniforms([1, 2], [1.; 4]);
        assert_eq!(
            uniforms.sprites[2] + 4 * uniforms.sprites[3],
            row[4].as_u64().unwrap() as u32
        );
        assert_eq!(uniforms.state[3], f(&row[5]) * (1. - rain));
        assert_eq!(lightmap::celestial_angle(time, partial, 0), f(&row[6]));
        for call in row[8].as_array().unwrap() {
            let fan = call[0] == 6;
            let matrix = Mat4::from_cols_array(&std::array::from_fn(|j| f(&call[3][j])));
            assert_eq!(
                [
                    call[5].as_u64().unwrap(),
                    call[6].as_u64().unwrap(),
                    call[7].as_u64().unwrap(),
                    call[8].as_u64().unwrap()
                ],
                [770, if fan { 771 } else { 1 }, 1, 0]
            );
            let start = if fan {
                0
            } else if call[1] == "textures/environment/sun.png" {
                18
            } else {
                22
            };
            let actual = &geometry.vertices[start..];
            if !fan {
                assert_eq!(f(&call[2][3]), uniforms.state[2]);
            }
            for (j, v) in call[4].as_array().unwrap().iter().enumerate() {
                let vp = Vec3::from_array(actual[j].position);
                let original_pos = Vec3::new(f(&v[0]), f(&v[1]), f(&v[2]));
                let expected = matrix.transform_point3(original_pos);
                let world = if fan {
                    let c = if j == 0 {
                        uniforms.center
                    } else {
                        uniforms.rim
                    };
                    assert_eq!(
                        c.map(|v| (255. * v).round() as u8),
                        std::array::from_fn(|n| v[3 + n].as_u64().unwrap() as u8)
                    );
                    Vec3::new(
                        -uniforms.state[1] * vp.y,
                        -vp.z * uniforms.state[0],
                        uniforms.state[1] * vp.x,
                    )
                } else {
                    let [mut u, mut y] = actual[j].uv;
                    if start == 22 {
                        u = (u + uniforms.sprites[2] as f32) * 0.25;
                        y = (y + uniforms.sprites[3] as f32) * 0.5;
                    }
                    assert_eq!([u, y], [f(&v[3]), f(&v[4])]);
                    Mat4::from_cols_array_2d(&uniforms.orbit).transform_point3(vp)
                };
                assert!(
                    world.abs_diff_eq(expected, 0.00004),
                    "{time} {j}: {world} / {expected}"
                );
            }
        }
    }
}
#[test]
#[ignore = "requires Vulkan and original local sun and moon textures"]
fn original_sun_moon_phases_stars_and_sunrise_framebuffer_pixels_match() {
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(
        include_bytes!("../tests/fixtures/vanilla189-celestial.json.gz").as_slice(),
    )
    .read_to_end(&mut bytes)
    .unwrap();
    let data: Value = serde_json::from_slice(&bytes).unwrap();
    let mut resources = Resources::load().unwrap();
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        pollster::block_on(adapter.request_device(&Default::default(), None)).unwrap();
    let make_texture = |size, format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let atlas = make_texture(
        wgpu::Extent3d {
            width: 256,
            height: 256,
            depth_or_array_layers: 2,
        },
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    );
    for (i, name) in [
        "textures/environment/sun.png",
        "textures/environment/moon_phases.png",
    ]
    .into_iter()
    .enumerate()
    {
        let sprite = resources.sprites.get_mut(name).unwrap();
        let offset = sprite.layer as usize * 256 * 256 * 4;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &atlas,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: i as u32,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &resources.pixels[offset..offset + 256 * 256 * 4],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(1024),
                rows_per_image: Some(256),
            },
            wgpu::Extent3d {
                width: 256,
                height: 256,
                depth_or_array_layers: 1,
            },
        );
        sprite.layer = i as u32;
    }
    let view = atlas.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sampler = device.create_sampler(&Default::default());
    let celestial = Celestial::new(
        &device,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Depth32Float,
        &view,
        &sampler,
        &resources,
    );
    drop(resources);
    let size = wgpu::Extent3d {
        width: 64,
        height: 64,
        depth_or_array_layers: 1,
    };
    let color = make_texture(
        size,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let depth = make_texture(
        size,
        wgpu::TextureFormat::Depth32Float,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
    );
    let color_view = color.create_view(&Default::default());
    let depth_view = depth.create_view(&Default::default());
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16384,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let f = |v: &Value| v.as_f64().unwrap() as f32;
    let mut samples = 0;
    for row in data[3].as_array().unwrap() {
        let matrix = |ix: usize| Mat4::from_cols_array(&std::array::from_fn(|n| f(&row[ix][n])));
        let mut commands = device.create_command_encoder(&Default::default());
        {
            let mut pass = commands.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 24. / 255.,
                            g: 48. / 255.,
                            b: 96. / 255.,
                            a: 1.,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            celestial.draw(
                &mut pass,
                &queue,
                Frame {
                    projection_view: matrix(2) * matrix(3),
                    time: row[0].as_i64().unwrap(),
                    partial: 0.,
                    rain: f(&row[1]),
                    sunrise: row[4].as_array().map(|a| std::array::from_fn(|k| f(&a[k]))),
                    eye: 0,
                },
            );
        }
        commands.copy_texture_to_buffer(
            color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(64),
                },
            },
            size,
        );
        queue.submit([commands.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        read.slice(..)
            .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
        device.poll(wgpu::Maintain::Wait);
        rx.recv().unwrap().unwrap();
        let pixels = read.slice(..).get_mapped_range();
        for pixel in row[5].as_array().unwrap() {
            let x = pixel[0].as_u64().unwrap() as usize;
            let y = pixel[1].as_u64().unwrap() as usize;
            for channel in 0..3 {
                let orig = pixel[2 + channel].as_i64().unwrap();
                let val = i64::from(pixels[(y * 64 + x) * 4 + channel]);
                assert!(
                    (val - orig).abs() <= 2,
                    "time{} x{x} y{y} channel{channel}: {val} vs {orig}",
                    row[0]
                );
            }
            samples += 1;
        }
        drop(pixels);
        read.unmap();
    }
    assert_eq!(samples, 1430);
}
