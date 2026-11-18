//! Original sprite sheet timing and mip selection. The nine animated block
//! layers are adjacent, so each changed mip uses a single GPU upload.
use anyhow::{ensure, Result};
use image::RgbaImage;
use serde::Deserialize;
use std::sync::{Arc, OnceLock};

pub const LEVELS: usize = 5;
pub const FLOW_SIZE: u32 = 32;
type Frame = [Vec<u8>; LEVELS];
#[derive(Deserialize)]
struct Section {
    #[serde(default = "one")]
    frametime: u32,
    #[serde(default)]
    interpolate: bool,
    #[serde(default)]
    frames: Vec<Entry>,
}
fn one() -> u32 {
    1
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Entry {
    Index(usize),
    Timed { index: usize, time: Option<u32> },
}
pub struct Clip {
    order: Vec<(usize, u32)>,
    frames: Vec<Frame>,
    interpolate: bool,
}
impl Clip {
    pub fn read(image: &RgbaImage, metadata: &[u8], tile_size: u32) -> Result<Self> {
        #[derive(Deserialize)]
        struct File {
            animation: Section,
        }
        let config = serde_json::from_slice::<File>(metadata)?.animation;
        let w = image.width();
        ensure!(
            w >= 16 && w.is_power_of_two() && image.height().is_multiple_of(w),
            "Invalid animated block sheet"
        );
        let count = (image.height() / w) as usize;
        let order: Vec<_> = if config.frames.is_empty() {
            (0..count).map(|i| (i, config.frametime)).collect()
        } else {
            config
                .frames
                .iter()
                .map(|entry| match entry {
                    Entry::Index(i) => (*i, config.frametime),
                    Entry::Timed { index, time } => (*index, time.unwrap_or(config.frametime)),
                })
                .collect()
        };
        ensure!(
            !order.is_empty() && order.iter().all(|&(i, t)| i < count && t > 0),
            "Invalid animation frame or duration"
        );
        let frames = (0..count)
            .map(|i| frame_mips(image::imageops::crop_imm(image, 0, i as u32 * w, w, w).to_image(), tile_size))
            .collect();
        Ok(Self {
            order,
            frames,
            interpolate: config.interpolate,
        })
    }
    pub fn still(frame: RgbaImage, tile_size: u32) -> Self {
        Self { order: vec![(0, 1)], frames: vec![frame_mips(frame, tile_size)], interpolate: false }
    }
}
fn frame_mips(frame: RgbaImage, tile_size: u32) -> Frame {
    static GAMMA: OnceLock<[f32; 256]> = OnceLock::new();
    let gamma =
        GAMMA.get_or_init(|| std::array::from_fn(|i| f64::from(i as f32 / 255.).powf(2.2) as f32));
    let mut width = frame.width() as usize;
    let mut data = frame.into_raw();
    // TextureUtil tests the first pixels corresponding to the input mip count.
    let transparent = data
        .as_chunks::<4>()
        .0
        .iter()
        .take(LEVELS)
        .any(|c| c[3] == 0);
    std::array::from_fn(|level| {
        let size = (tile_size >> level) as usize;
        let mut output = vec![0; size * size * 4];
        for y in 0..size {
            for x in 0..size {
                let sx = (2 * x + 1) * width / (size * 2);
                let sy = (2 * y + 1) * width / (size * 2);
                let index = (sy * width + sx) * 4;
                output[(y * size + x) * 4..(y * size + x + 1) * 4]
                    .copy_from_slice(&data[index..index + 4]);
            }
        }
        if level + 1 < LEVELS && width > 1 {
            let next = width / 2;
            let mut scaled = vec![0; next * next * 4];
            for y in 0..next {
                for x in 0..next {
                    let i = (2 * y * width + 2 * x) * 4;
                    for c in 0..4 {
                        let mut sum = 0f32;
                        for p in [i, i + 4, i + width * 4, i + width * 4 + 4] {
                            if !transparent || data[p + 3] != 0 {
                                sum += gamma[data[p + c] as usize];
                            }
                        }
                        let power = f64::from(sum * 0.25).powf(1. / 2.2);
                        let value = ((if transparent {
                            power
                        } else {
                            f64::from(power as f32)
                        }) * 255.) as u8;
                        scaled[(y * next + x) * 4 + c] = if transparent && c == 3 && value < 96 {
                            0
                        } else {
                            value
                        };
                    }
                }
            }
            data = scaled;
            width = next;
        }
        output
    })
}
#[derive(Default, Clone, Copy)]
struct Cursor {
    entry: usize,
    tick: u32,
}
pub struct Animator {
    clips: Arc<Vec<Clip>>,
    cursors: Vec<Cursor>,
    pixels: Frame,
}
impl Animator {
    pub fn new(clips: Arc<Vec<Clip>>) -> Self {
        let cursors = vec![Cursor::default(); clips.len()];
        let pixels = std::array::from_fn(|level| {
            clips
                .iter()
                .flat_map(|s| s.frames[0][level].iter().copied())
                .collect()
        });
        Self {
            clips,
            cursors,
            pixels,
        }
    }
    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        for (layer, (clip, cursor)) in self.clips.iter().zip(&mut self.cursors).enumerate() {
            let (from, time) = clip.order[cursor.entry];
            let next_entry = (cursor.entry + 1) % clip.order.len();
            let next = clip.order[next_entry].0;
            cursor.tick += 1;
            let advance = cursor.tick >= time;
            if advance {
                cursor.entry = next_entry;
                cursor.tick = 0;
            }
            if from == next || !advance && !clip.interpolate {
                continue;
            }
            changed = true;
            let weight = 1. - f64::from(cursor.tick) / f64::from(time);
            for level in 0..LEVELS {
                let first = &clip.frames[from][level];
                let second = &clip.frames[next][level];
                let out = &mut self.pixels[level][layer * first.len()..(layer + 1) * first.len()];
                if advance {
                    out.copy_from_slice(second);
                } else {
                    for (i, v) in out.iter_mut().enumerate() {
                        *v = if i % 4 == 3 {
                            first[i]
                        } else {
                            (f64::from(first[i]) * weight + f64::from(second[i]) * (1. - weight))
                                as u8
                        };
                    }
                }
            }
        }
        changed
    }
    pub fn base_pixels(&self) -> &[u8] {
        &self.pixels[0]
    }
    pub fn upload(&self, queue: &wgpu::Queue, texture: &wgpu::Texture, first_layer: u32) {
        if self.clips.is_empty() {
            return;
        }
        let mips = (texture.mip_level_count() as usize).min(self.pixels.len());
        for (level, data) in self.pixels[..mips].iter().enumerate() {
            let size = (texture.width() >> level).max(1);
            if data.len() != (size * size * 4) as usize * self.clips.len() {
                break;
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d { x: 0, y: 0, z: first_layer },
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size * 4),
                    rows_per_image: Some(size),
                },
                wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: self.clips.len() as u32,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[test]
    fn hd_tile_size_animations_preserve_all_five_mip_levels() {
        for tile_size in [16u32, 32, 64, 128, 256] {
            let mut sheet = RgbaImage::new(tile_size, tile_size * 2);
            for y in 0..tile_size {
                for x in 0..tile_size {
                    sheet.put_pixel(x, y, image::Rgba([200, 40, 20, 255]));
                    sheet.put_pixel(x, y + tile_size, image::Rgba([20, 120, 220, 255]));
                }
            }
            let meta = br#"{"animation":{"frametime":2,"interpolate":true}}"#;
            let clip = Clip::read(&sheet, meta, tile_size).unwrap();
            let mut animator = Animator::new(Arc::new(vec![clip]));
            assert_eq!(animator.base_pixels().len(), (tile_size * tile_size * 4) as usize);
            assert!(animator.tick());
            for level in 0..LEVELS {
                let s = (tile_size >> level) as usize;
                assert_eq!(animator.pixels[level].len(), s * s * 4);
            }
        }
    }
    #[test]
    #[ignore = "requires the user's local 1.8.9 JAR"]
    fn original_animated_block_gpu_uploads_match_through_each_mip_and_frame_wrap() {
        type Samples = Vec<(u32, [String; LEVELS])>;
        let expected: BTreeMap<String, Samples> = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-block-animation.json"
        ))
        .unwrap();
        let jar = crate::minecraft_dir().join("versions/1.8.9/1.8.9.jar");
        let assets = crate::assets::Assets::load(&jar).unwrap();
        assert_eq!(assets.tile_size, 16);
        assert_eq!(assets.animations.len(), 9);
        let mut atlas = Animator::new(assets.animations.clone());
        let mut flow = Animator::new(assets.flowing_tiles.clone());
        let mut rows: Vec<_> = expected
            .iter()
            .flat_map(|(name, samples)| samples.iter().map(move |(tick, hash)| (*tick, name, hash)))
            .collect();
        rows.sort_by_key(|r| r.0);
        let mut time = 0;
        for (tick, name, hashes) in rows {
            while time < tick {
                atlas.tick();
                flow.tick();
                time += 1;
            }
            let (pixels, size, layer) = match name.as_str() {
                "water_flow" => (&flow.pixels, FLOW_SIZE as usize, 0),
                "lava_flow" => (&flow.pixels, FLOW_SIZE as usize, 1),
                _ => (
                    &atlas.pixels,
                    assets.tile_size as usize,
                    (assets.materials[name] - 1) as usize,
                ),
            };
            for (level, expected) in hashes.iter().enumerate() {
                let bytes = (size >> level) * (size >> level) * 4;
                let slice = &pixels[level][layer * bytes..(layer + 1) * bytes];
                let hash = slice.iter().fold(0xcbf29ce484222325u64, |h, v| {
                    (h ^ u64::from(*v)).wrapping_mul(0x100000001b3)
                });
                assert_eq!(
                    hash.to_string(),
                    *expected,
                    "{name} tick {tick} mip {level}"
                );
            }
        }
        assert_eq!(time, 6600);
    }
}
