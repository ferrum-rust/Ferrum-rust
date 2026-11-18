use super::{base, item_tag, Tag};
use crate::{
    inventory::Stack,
    resources::Resources,
    vanilla::{drops::DropNbt, item_nbt},
};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

const PATTERNS: &[(&str, &str)] = &[
    ("b", "base"),
    ("bl", "square_bottom_left"),
    ("br", "square_bottom_right"),
    ("tl", "square_top_left"),
    ("tr", "square_top_right"),
    ("bs", "stripe_bottom"),
    ("ts", "stripe_top"),
    ("ls", "stripe_left"),
    ("rs", "stripe_right"),
    ("cs", "stripe_center"),
    ("ms", "stripe_middle"),
    ("drs", "stripe_downright"),
    ("dls", "stripe_downleft"),
    ("ss", "small_stripes"),
    ("cr", "cross"),
    ("sc", "straight_cross"),
    ("bt", "triangle_bottom"),
    ("tt", "triangle_top"),
    ("bts", "triangles_bottom"),
    ("tts", "triangles_top"),
    ("ld", "diagonal_left"),
    ("rd", "diagonal_up_right"),
    ("lud", "diagonal_up_left"),
    ("rud", "diagonal_right"),
    ("mc", "circle"),
    ("mr", "rhombus"),
    ("vh", "half_vertical"),
    ("hh", "half_horizontal"),
    ("vhr", "half_vertical_right"),
    ("hhb", "half_horizontal_bottom"),
    ("bo", "border"),
    ("cbo", "curly_border"),
    ("cre", "creeper"),
    ("gra", "gradient"),
    ("gru", "gradient_up"),
    ("bri", "bricks"),
    ("sku", "skull"),
    ("flo", "flower"),
    ("moj", "mojang"),
];
#[derive(Clone, Copy)]
pub(crate) struct Region {
    pub layer: u32,
    pub origin: [f32; 2],
}
impl Region {
    fn at(layer: u32, i: usize) -> Self {
        Self {
            layer,
            origin: [(i % 4) as f32 * 0.25, ((i / 4) % 4) as f32 * 0.25],
        }
    }
}
#[derive(PartialEq)]
struct Design {
    color: u8,
    layers: Vec<(usize, u8)>,
}
struct Entry {
    design: Design,
    rgba: Vec<u8>,
    revision: u64,
    last_used: Instant,
}
#[derive(Default)]
struct Slots {
    entries: Vec<Entry>,
    revision: u64,
}
pub(crate) struct Atlas {
    first: u32,
    slots: Mutex<Slots>,
}
fn color(n: i32) -> u8 {
    if (0..16).contains(&n) {
        n as u8
    } else {
        0
    }
}

impl Atlas {
    pub(crate) fn reserve(pixels: &mut Vec<u8>) -> Self {
        let page = 256 * 256 * 4;
        let first = (pixels.len() / page) as u32;
        pixels.resize(pixels.len() + 16 * page, 0);
        Self {
            first,
            slots: Mutex::default(),
        }
    }
    pub(crate) fn stack(&self, r: &Resources, stack: &Stack) -> Region {
        self.region(r, item_tag(stack), stack.meta)
    }
    pub(super) fn region(&self, r: &Resources, tag: Option<&Tag>, fallback: u16) -> Region {
        let dye = color(base(tag, fallback));
        let plain = Region::at(r.sprite("creative/banners").layer, dye as usize);
        let Some(DropNbt::List {
            element: 10,
            values,
        }) = tag.and_then(|v| v.get("Patterns"))
        else {
            return plain;
        };
        let layers = values
            .iter()
            .filter_map(|v| {
                let t = item_nbt::compound(v)?;
                let DropNbt::String(code) = t.get("Pattern")? else {
                    return None;
                };
                let mask = PATTERNS.iter().position(|(key, _)| *key == code)?;
                Some((mask, color(item_nbt::number(t.get("Color")))))
            })
            .take(16)
            .collect::<Vec<_>>();
        if layers.is_empty() {
            return plain;
        }
        let design = Design { color: dye, layers };
        let mut cache = self.slots.lock().unwrap();
        let now = Instant::now();
        let slot = if let Some(i) = cache.entries.iter().position(|e| e.design == design) {
            cache.entries[i].last_used = now;
            i
        } else {
            let i = if cache.entries.len() < 256 {
                cache.entries.len()
            } else if let Some(i) = cache
                .entries
                .iter()
                .position(|e| now.duration_since(e.last_used) > Duration::from_secs(60))
            {
                i
            } else {
                return plain;
            };
            cache.revision += 1;
            let entry = Entry {
                rgba: compose(r, plain, &design),
                design,
                revision: cache.revision,
                last_used: now,
            };
            if i == cache.entries.len() {
                cache.entries.push(entry);
            } else {
                cache.entries[i] = entry;
            }
            i
        };
        Region::at(self.first + (slot / 16) as u32, slot)
    }
    pub(crate) fn upload(&self, queue: &wgpu::Queue, texture: &wgpu::Texture, last: &mut u64) {
        let cache = self.slots.lock().unwrap();
        if *last == cache.revision {
            return;
        }
        for (i, e) in cache
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.revision > *last)
        {
            let origin = wgpu::Origin3d {
                x: ((i % 4) * 64) as u32,
                y: ((i / 4 % 4) * 64) as u32,
                z: self.first + (i / 16) as u32,
            };
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin,
                    aspect: wgpu::TextureAspect::All,
                },
                &e.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(64),
                },
                wgpu::Extent3d {
                    width: 64,
                    height: 64,
                    depth_or_array_layers: 1,
                },
            );
        }
        *last = cache.revision;
    }
}
fn pixel(r: &Resources, layer: u32, x: usize, y: usize) -> &[u8] {
    let i = (((layer as usize * 256 + y) * 256 + x) * 4) as usize;
    &r.pixels[i..i + 4]
}
fn compose(r: &Resources, plain: Region, design: &Design) -> Vec<u8> {
    let raw = r.sprite("textures/entity/banner_base.png");
    let [ox, oy] = plain.origin.map(|n| (n * 256.) as usize);
    let mut out = Vec::with_capacity(64 * 64 * 4);
    for y in 0..64 {
        for x in 0..64 {
            out.extend_from_slice(pixel(r, plain.layer, ox + x, oy + y));
        }
    }
    for &(kind, dye) in &design.layers {
        let mask = r.sprite(&format!("textures/entity/banner/{}.png", PATTERNS[kind].1));
        if mask.width != raw.width || mask.height != raw.height {
            continue;
        }
        let [_, red, green, blue] = crate::ui::creative::dye_colors()[dye as usize].to_be_bytes();
        for y in 0..64 {
            for x in 0..64 {
                let sx = x * raw.width as usize / 64;
                let sy = y * raw.height as usize / 64;
                let m = pixel(r, mask.layer, sx, sy);
                if m[3] == 0 || m[0] == 0 {
                    continue;
                }
                let source = pixel(r, raw.layer, sx, sy);
                let alpha = u32::from(m[0]);
                let out = &mut out[(y * 64 + x) * 4..][..4];
                for (i, d) in [red, green, blue].into_iter().enumerate() {
                    let tint = u32::from(d) * u32::from(source[i]) / 255;
                    // Java2D rounds both premultiplied components before adding them.
                    out[i] = (((tint * alpha + 127) / 255)
                        + ((u32::from(out[i]) * (255 - alpha) + 127) / 255))
                        as u8;
                }
            }
        }
    }
    out
}
#[test]
#[ignore = "requires original resource pack for the pixel comparison"]
fn original_gpu_composited_banner_matches_all_rgba_pixels_for_placed_and_held() {
    let r = crate::validation::resources();
    let stack = super::tests::stack();
    let placed = super::tile(&stack, glam::IVec3::new(8, 64, 8));
    let design = r.banners.stack(r, &stack);
    let same = r.banners.region(r, item_nbt::compound(&placed), 0);
    assert_eq!((design.layer, design.origin), (same.layer, same.origin));
    let cache = r.banners.slots.lock().unwrap();
    let index = ((design.layer - r.banners.first) * 16
        + (design.origin[1] * 16.) as u32
        + (design.origin[0] * 4.) as u32) as usize;
    let actual = &cache.entries[index].rgba;
    let wanted = &super::tests::ORIGINAL.rgba;
    assert_eq!(actual.len(), wanted.len());
    assert!(
        actual == wanted,
        "{} differing RGBA bytes",
        actual.iter().zip(wanted).filter(|(a, b)| a != b).count()
    );
}
