use anyhow::{Context, Result};
use image::RgbaImage;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default)]
pub struct Sprite {
    pub layer: u32,
    pub width: u32,
    pub height: u32,
}
fn icon_hash(bytes: &[u8]) -> u64 {
    use std::hash::{DefaultHasher, Hasher};
    let mut hash = DefaultHasher::new();
    hash.write(bytes);
    hash.finish()
}
#[derive(Clone, Debug, Deserialize)]
pub struct Language {
    #[serde(skip)]
    pub code: String,
    pub name: String,
    pub region: String,
    #[serde(default)]
    pub bidirectional: bool,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Definition {
    pub name: String,
    pub translation: String,
    pub food: u8,
}
#[derive(Deserialize, Default)]
pub struct Registry {
    pub blocks: BTreeMap<u16, Definition>,
    pub items: BTreeMap<u16, Definition>,
}
pub struct Resources {
    pub banners: crate::banner::texture::Atlas,
    pub(crate) skins: crate::player_skins::Atlas,
    pub root: PathBuf,
    pack_icon_hashes: BTreeMap<String, u64>,
    packs: std::sync::Arc<crate::resource_packs::Stack>,
    unicode_locales: std::collections::HashSet<String>,
    pub splashes: Vec<String>,
    pub pixels: Vec<u8>,
    pub sprites: HashMap<String, Sprite>,
    pub languages: Vec<Language>,
    translations: HashMap<String, HashMap<String, String>>,
    pub glyph_sizes: Vec<u8>,
    pub ascii_width: [f32; 256],
    pub registry: Registry,
    pub item_textures: HashMap<u16, String>,
    pub sound_files: usize,
    pub resource_files: usize,
}
pub fn spawn_egg_entity_key(meta: u16) -> Option<&'static str> {
    match meta {
        1 => Some("Item"),
        2 => Some("XPOrb"),
        8 => Some("LeashKnot"),
        9 => Some("Painting"),
        10 => Some("Arrow"),
        11 => Some("Snowball"),
        12 => Some("Fireball"),
        13 => Some("SmallFireball"),
        14 => Some("ThrownEnderpearl"),
        15 => Some("EyeOfEnderSignal"),
        16 => Some("ThrownPotion"),
        17 => Some("ThrownExpBottle"),
        18 => Some("ItemFrame"),
        19 => Some("WitherSkull"),
        20 => Some("PrimedTnt"),
        21 => Some("FallingSand"),
        22 => Some("FireworksRocketEntity"),
        30 => Some("ArmorStand"),
        40 => Some("MinecartCommandBlock"),
        41 => Some("Boat"),
        42 => Some("MinecartRideable"),
        43 => Some("MinecartChest"),
        44 => Some("MinecartFurnace"),
        45 => Some("MinecartTNT"),
        46 => Some("MinecartHopper"),
        47 => Some("MinecartSpawner"),
        48 => Some("Mob"),
        49 => Some("Monster"),
        50 => Some("Creeper"),
        51 => Some("Skeleton"),
        52 => Some("Spider"),
        53 => Some("Giant"),
        54 => Some("Zombie"),
        55 => Some("Slime"),
        56 => Some("Ghast"),
        57 => Some("PigZombie"),
        58 => Some("Enderman"),
        59 => Some("CaveSpider"),
        60 => Some("Silverfish"),
        61 => Some("Blaze"),
        62 => Some("LavaSlime"),
        63 => Some("EnderDragon"),
        64 => Some("WitherBoss"),
        65 => Some("Bat"),
        66 => Some("Witch"),
        67 => Some("Endermite"),
        68 => Some("Guardian"),
        90 => Some("Pig"),
        91 => Some("Sheep"),
        92 => Some("Cow"),
        93 => Some("Chicken"),
        94 => Some("Squid"),
        95 => Some("Wolf"),
        96 => Some("MushroomCow"),
        97 => Some("SnowMan"),
        98 => Some("Ozelot"),
        99 => Some("VillagerGolem"),
        100 => Some("EntityHorse"),
        101 => Some("Rabbit"),
        120 => Some("Villager"),
        200 => Some("EnderCrystal"),
        _ => None,
    }
}

impl Resources {
    #[cfg(test)]
    pub fn load() -> Result<Self> {
        Self::load_with_packs(Default::default())
    }
    pub fn load_with_packs(packs: std::sync::Arc<crate::resource_packs::Stack>) -> Result<Self> {
        let root = PathBuf::from(".local/resources/assets/minecraft");
        let manifest: serde_json::Value = serde_json::from_slice(
            &std::fs::read(".local/resources/manifest.json")
                .context("Run resources.bat to import your local Minecraft resources")?,
        )?;
        let files = manifest["files"]
            .as_array()
            .context("Resource manifest files")?;
        let mut sprites = HashMap::new();
        let mut pixels = vec![255; 256 * 256 * 4];
        let mut translations = HashMap::new();
        let metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(".local/resources/assets/pack.mcmeta")?)?;
        let mut locale_meta: BTreeMap<String, Language> =
            serde_json::from_value(metadata["language"].clone())?;
        // LanguageManager keeps the first declaration of each language.
        for (code, language) in &packs.languages {
            locale_meta
                .entry(code.clone())
                .or_insert_with(|| language.clone());
        }
        let mut languages: Vec<_> = locale_meta
            .into_iter()
            .map(|(code, mut language)| {
                language.code = code.to_lowercase();
                language
            })
            .collect();
        let names: std::collections::BTreeSet<&str> = files
            .iter()
            .filter_map(|f| f.as_str()?.strip_prefix("assets/minecraft/"))
            .chain(packs.names())
            .collect();
        for relative in names {
            if relative.starts_with("textures/")
                && relative.ends_with(".png")
                && !relative.starts_with("textures/blocks/")
            {
                let original = image::load_from_memory(&packs.bytes(&root, relative)?)?.to_rgba8();
                let original = if matches!(relative,"textures/items/compass.png"|"textures/items/clock.png") && original.height()>=original.width(){
                    // Dynamic item sprites are frame strips. Retain every frame in an atlas
                    // and expose the first frame normally for renderers that request a lone sprite.
                    let size=original.width();let count=(original.height()/size).min(256);
                    let mut atlas=RgbaImage::new(256,256);
                    for i in 0..count{
                        let src=image::imageops::crop_imm(&original,0,i*size,size,size).to_image();
                        let frame=image::imageops::resize(&src,16,16,image::imageops::FilterType::Nearest);
                        image::imageops::replace(&mut atlas,&frame,i64::from((i%16)*16),i64::from((i/16)*16));
                    }
                    sprites.insert(format!("{relative}#frames"),Sprite{layer:(pixels.len()/(256*256*4)) as u32,width:16,height:count});
                    pixels.extend_from_slice(atlas.as_raw());
                    image::imageops::crop_imm(&original,0,0,size,size).to_image()
                }else{original};
                let mut page = RgbaImage::new(256, 256);
                let (sprite_w, sprite_h) = if relative == "textures/misc/enchanted_item_glint.png"
                    && original.width() > 0
                    && original.height() > 0
                {
                    let (src_w, src_h) = (original.width(), original.height());
                    let src = original.as_raw();
                    let dst = page.as_mut();
                    for dy in 0..256u32 {
                        let sy = ((dy as f32 + 0.5) / 256.0) * src_h as f32 - 0.5;
                        let sy_floor = sy.floor();
                        let fy = sy - sy_floor;
                        let y0 = (sy_floor as i32).rem_euclid(src_h as i32) as u32;
                        let y1 = (y0 + 1) % src_h;
                        for dx in 0..256u32 {
                            let sx = ((dx as f32 + 0.5) / 256.0) * src_w as f32 - 0.5;
                            let sx_floor = sx.floor();
                            let fx = sx - sx_floor;
                            let x0 = (sx_floor as i32).rem_euclid(src_w as i32) as u32;
                            let x1 = (x0 + 1) % src_w;
                            let p00 = ((y0 * src_w + x0) * 4) as usize;
                            let p10 = ((y0 * src_w + x1) * 4) as usize;
                            let p01 = ((y1 * src_w + x0) * 4) as usize;
                            let p11 = ((y1 * src_w + x1) * 4) as usize;
                            let out = ((dy * 256 + dx) * 4) as usize;
                            for ch in 0..4 {
                                let top = f32::from(src[p00 + ch]) * (1.0 - fx)
                                    + f32::from(src[p10 + ch]) * fx;
                                let bot = f32::from(src[p01 + ch]) * (1.0 - fx)
                                    + f32::from(src[p11 + ch]) * fx;
                                dst[out + ch] = (top * (1.0 - fy) + bot * fy)
                                    .round()
                                    .clamp(0.0, 255.0) as u8;
                            }
                        }
                    }
                    (256, 256)
                } else {
                    let decoded = if original.width() > 256 || original.height() > 256 {
                        image::imageops::thumbnail(&original, 256, 256)
                    } else {
                        original
                    };
                    let dims = (decoded.width(), decoded.height());
                    image::imageops::replace(&mut page, &decoded, 0, 0);
                    dims
                };
                let sprite = Sprite {
                    layer: (pixels.len() / (256 * 256 * 4)) as u32,
                    width: sprite_w,
                    height: sprite_h,
                };
                sprites.insert(relative.to_owned(), sprite);
                pixels.extend_from_slice(page.as_raw());
            }
            if relative.starts_with("lang/") && relative.ends_with(".lang") {
                let mut content = std::fs::read_to_string(root.join(relative)).unwrap_or_default();
                if let Some(overlay) = packs.get(relative) {
                    content.push('\n');
                    content.push_str(std::str::from_utf8(overlay)?);
                }
                let mut table = HashMap::new();
                for line in content.lines() {
                    if line.starts_with('#') {
                        continue;
                    }
                    if let Some((key, value)) = line.split_once('=') {
                        table.insert(key.trim().to_owned(), value.replace("%d", "%s"));
                    }
                }
                let code = Path::new(relative)
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .to_lowercase();
                translations.insert(code, table);
            }
        }
        let base=packs.bytes(&root,"textures/entity/banner_base.png")?;
        let mask=packs.bytes(&root,"textures/entity/banner/base.png")?;
        let base=image::load_from_memory(&base)?.resize_exact(64,64,image::imageops::FilterType::Nearest).to_rgba8();
        let mask=image::load_from_memory(&mask)?.resize_exact(64,64,image::imageops::FilterType::Nearest).to_rgba8();
        let mut page=RgbaImage::new(256,256);
        for (i,&color) in crate::ui::creative::dye_colors().iter().enumerate(){
            let [_,r,g,b]=color.to_be_bytes();let mut banner=base.clone();
            for ((px,src),mask) in banner.pixels_mut().zip(base.pixels()).zip(mask.pixels()){
                let alpha=if mask[3]==0{0}else{u32::from(mask[0])};
                for (channel,dye) in [r,g,b].into_iter().enumerate(){
                    let src=u32::from(src[channel]);let tinted=src*u32::from(dye)/255;
                    px[channel]=(((tinted*alpha+127)/255)+((src*(255-alpha)+127)/255)) as u8;
                }
            }
            image::imageops::replace(&mut page,&banner,(i%4*64) as i64,(i/4*64) as i64);
        }
        sprites.insert("creative/banners".into(),Sprite{layer:(pixels.len()/(256*256*4)) as u32,width:256,height:256});
        pixels.extend_from_slice(page.as_raw());
        {
            let sodium_assets: [(&str, &[u8], Option<(u32, u32)>); 3] = [
                (
                    "sodium/config_icon",
                    include_bytes!("../sodium-dev/common/src/main/resources/config-icon.png"),
                    Some((38, 38)),
                ),
                (
                    "sodium/reset_button",
                    include_bytes!("../sodium-dev/common/src/main/resources/assets/sodium/textures/gui/reset_button.png"),
                    None,
                ),
                (
                    "sodium/tooltip_arrows",
                    include_bytes!("../sodium-dev/common/src/main/resources/assets/sodium/textures/gui/tooltip_arrows.png"),
                    None,
                ),
            ];
            for (key, raw, resize) in sodium_assets {
                let decoded = image::load_from_memory(raw)?;
                let img = match resize {
                    Some((w, h)) => decoded
                        .resize_exact(w, h, image::imageops::FilterType::Lanczos3)
                        .to_rgba8(),
                    None => decoded.to_rgba8(),
                };
                let mut page = RgbaImage::new(256, 256);
                image::imageops::replace(&mut page, &img, 0, 0);
                sprites.insert(
                    key.into(),
                    Sprite {
                        layer: (pixels.len() / (256 * 256 * 4)) as u32,
                        width: 256,
                        height: 256,
                    },
                );
                pixels.extend_from_slice(page.as_raw());
            }
        }
        let mut icons = Vec::new();
        let mut pack_icon_hashes = BTreeMap::new();
        if let Ok(bytes) = std::fs::read(".local/resources/pack.png") {
            icons.push((crate::resource_packs::icon_key(None), bytes));
        }
        if Path::new(crate::resource_packs::DIRECTORY).is_dir() {
            for pack in crate::resource_packs::discover().unwrap_or_default() {
                if let Ok(bytes) = crate::resource_packs::icon(&pack) {
                    pack_icon_hashes.insert(pack.name.clone(), icon_hash(&bytes));
                    icons.push((crate::resource_packs::icon_key(Some(&pack.name)), bytes));
                }
            }
        }
        for (key, bytes) in icons {
            if let Ok(decoded) = image::load_from_memory(&bytes) {
                let image = decoded.thumbnail(256, 256).to_rgba8();
                let mut page = RgbaImage::new(256, 256);
                image::imageops::replace(&mut page, &image, 0, 0);
                sprites.insert(
                    key,
                    Sprite {
                        layer: (pixels.len() / (256 * 256 * 4)) as u32,
                        width: image.width(),
                        height: image.height(),
                    },
                );
                pixels.extend_from_slice(page.as_raw());
            }
        }
        languages.sort_by(|a, b| a.code.cmp(&b.code));
        languages.dedup_by(|a, b| a.code == b.code);
        let glyph_sizes = packs.bytes(&root, "font/glyph_sizes.bin")?;
        let ascii =
            image::load_from_memory(&packs.bytes(&root, "textures/font/ascii.png")?)?.to_rgba8();
        let unicode_locales = translations
            .iter()
            .filter_map(|(code, values)| {
                let fallback = translations
                    .get("en_us")
                    .into_iter()
                    .flat_map(|v| v.iter())
                    .filter(|(k, _)| !values.contains_key(*k))
                    .map(|(_, v)| v);
                let (mut wide, mut total) = (0usize, 0usize);
                for unit in values
                    .values()
                    .chain(fallback)
                    .flat_map(|s| s.encode_utf16())
                {
                    wide += usize::from(unit >= 256);
                    total += 1;
                }
                (total > 0 && wide as f32 / total as f32 > 0.1).then(|| code.clone())
            })
            .collect();
        let mut ascii_width = [6.0; 256];
        anyhow::ensure!(
            ascii.width() >= 16 && ascii.width() == ascii.height(),
            "Expected a square 16-by-16 ASCII glyph grid"
        );
        let cell = ascii.width() / 16;
        for c in 0..256u32 {
            let mut occupied = 0;
            for y in 0..cell {
                for x in 0..cell {
                    if ascii.get_pixel(c % 16 * cell + x, c / 16 * cell + y).0[3] > 0 {
                        occupied = occupied.max(x + 1);
                    }
                }
            }
            ascii_width[c as usize] = (occupied as f32 * 8.0 / cell as f32).round() + 1.;
        }
        ascii_width[32] = 4.0;
        let registry: Registry =
            serde_json::from_slice(&std::fs::read(".local/resources/registry.json")?)?;
        let mut item_textures = HashMap::new();
        for (&id, def) in &registry.items {
            let path = format!("models/item/{}.json", def.name);
            let texture = packs
                .bytes(&root, &path)
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                .and_then(|v| v["textures"]["layer0"].as_str().map(str::to_owned))
                .map(|p| format!("textures/{}.png", p.trim_start_matches("minecraft:")))
                .unwrap_or_else(|| format!("textures/items/{}.png", def.name));
            item_textures.insert(id, texture);
        }
        println!(
            "Resources: {} files, {} languages, {} sounds, {} GUI/entity/font texture pages",
            files.len(),
            languages.len(),
            manifest["sounds"],
            pixels.len() / (256 * 256 * 4)
        );
        let splashes = String::from_utf8(packs.bytes(&root, "texts/splashes.txt")?)?
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .filter(|s| {
                s.encode_utf16()
                    .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c)))
                    != 125780783
            })
            .map(str::to_owned)
            .collect();
        let banner_atlas = crate::banner::texture::Atlas::reserve(&mut pixels);
        let skin_atlas = crate::player_skins::Atlas::reserve(&mut pixels);
        // Last array layer is an upload target for the currently held server map.
        pixels.resize(pixels.len()+256*256*4,0);
        Ok(Self {
            banners: banner_atlas,
            skins: skin_atlas,
            pack_icon_hashes,
            splashes,
            root,
            packs,
            unicode_locales,
            pixels,
            sprites,
            languages,
            translations,
            glyph_sizes,
            ascii_width,
            registry,
            item_textures,
            sound_files: manifest["sounds"].as_u64().unwrap_or(0) as usize,
            resource_files: files.len(),
        })
    }
    pub fn bytes(&self, relative: &str) -> Result<Vec<u8>> {
        self.packs.bytes(&self.root, relative)
    }
    pub fn pack_icons_changed(&self, available: &[crate::resource_packs::Pack]) -> bool {
        available.iter().any(|pack| {
            let current = crate::resource_packs::icon(pack).ok().map(|bytes| icon_hash(&bytes));
            self.pack_icon_hashes.get(&pack.name).copied() != current
        })
    }
    pub fn sound_manifests(&self) -> Result<impl Iterator<Item = std::borrow::Cow<'_, [u8]>>> {
        use std::borrow::Cow;
        let base = std::fs::read(self.root.join("sounds.json"))?;
        Ok(std::iter::once(Cow::Owned(base)).chain(self.packs.sound_manifests().map(Cow::Borrowed)))
    }
    pub fn has_sound(&self, name: &str) -> bool {
        let path = format!("sounds/{name}.ogg");
        self.packs.get(&path).is_some() || self.root.join(path).is_file()
    }
    pub fn unicode(&self, code: &str) -> bool {
        self.unicode_locales.contains(code)
    }
    pub fn layers(&self) -> u32 {
        (self.pixels.len() / (256 * 256 * 4)) as u32
    }
    pub fn sprite(&self, name: &str) -> Sprite {
        self.sprites.get(name).copied().unwrap_or(Sprite {
            layer: 0,
            width: 16,
            height: 16,
        })
    }
    pub fn tr(&self, code: &str, key: &str) -> String {
        self.translations
            .get(code)
            .and_then(|t| t.get(key))
            .or_else(|| self.translations.get("en_us").and_then(|t| t.get(key)))
            .cloned()
            .unwrap_or_else(|| key.to_owned())
    }
    pub fn item_name(&self, language: &str, id: u16, meta: u16) -> String {
        if id == 383 {
            let base = self.tr(language, "item.monsterPlacer.name").trim().to_owned();
            if let Some(ekey) = spawn_egg_entity_key(meta) {
                let entity_name = self.tr(language, &format!("entity.{ekey}.name"));
                return format!("{base} {entity_name}");
            }
            return base;
        }
        let special = match (id, meta) {
            (1, 0) => Some("tile.stone.stone.name"),
            (3, 0) => Some("tile.dirt.default.name"),
            (5, 0) => Some("tile.wood.oak.name"),
            (17, 0) => Some("tile.log.oak.name"),
            (35, 0) => Some("tile.cloth.white.name"),
            _ => None,
        };
        if let Some(key) = special {
            return self.tr(language, key);
        }
        let def = if id < 256 {
            self.registry.blocks.get(&id)
        } else {
            self.registry.items.get(&id)
        };
        def.map(|d| {
            let t = self.tr(language, &d.translation);
            if t == d.translation {
                d.name.replace('_', " ")
            } else {
                t
            }
        })
        .unwrap_or_else(|| format!("Item {id}:{meta}"))
    }
    pub fn item_sprite(&self, id: u16, meta: u16) -> Sprite {
        let special = match (id, meta) {
            (263, 1) => Some("textures/items/charcoal.png"),
            (373, _) => Some(if meta & 0x4000 != 0 {
                "textures/items/potion_bottle_splash.png"
            } else {
                "textures/items/potion_bottle_drinkable.png"
            }),
            (351, _) => Some(match meta {
                0 => "textures/items/dye_powder_black.png",
                1 => "textures/items/dye_powder_red.png",
                4 => "textures/items/dye_powder_blue.png",
                15 => "textures/items/dye_powder_white.png",
                _ => "textures/items/dye_powder_green.png",
            }),
            _ => None,
        };
        self.sprite(
            special
                .or_else(|| self.item_textures.get(&id).map(String::as_str))
                .unwrap_or("textures/items/barrier.png"),
        )
    }
}
