use anyhow::{Context, Result};
use image::imageops::FilterType;
use std::{collections::HashMap, fs::File, io::Read, path::Path};
#[path = "block_animation.rs"]
pub mod animation;
#[path = "biome_colors.rs"]
pub mod biome_colors;
#[path = "block_models.rs"]
pub(crate) mod block_models;

pub struct Assets {
    /// Identity shared by queued geometry without retaining all decoded pixels.
    pub(crate) identity: std::sync::Arc<()>,
    pub animations: std::sync::Arc<Vec<animation::Clip>>,
    pub flowing_tiles: std::sync::Arc<Vec<animation::Clip>>,
    pub tile_size: u32,
    pub pixels: Vec<u8>,
    pub layers: u32,
    pub materials: HashMap<String, u32>,
    pub fallback: HashMap<u16, [u32; 6]>,
    pub biome_colors: biome_colors::BiomeColors,
    pub block_models: block_models::BlockModels,
}
impl Default for Assets {
    fn default() -> Self {
        Self {
            identity: Default::default(),
            animations: Default::default(),
            flowing_tiles: Default::default(),
            tile_size: 16,
            pixels: Vec::new(),
            layers: 1,
            materials: HashMap::new(),
            fallback: HashMap::new(),
            biome_colors: Default::default(),
            block_models: Default::default(),
        }
    }
}
impl Assets {
    #[cfg(test)]
    pub fn load(jar: &Path) -> Result<Self> {
        Self::load_with_packs(jar, &crate::resource_packs::Stack::default())
    }
    pub fn load_with_packs(jar: &Path, packs: &crate::resource_packs::Stack) -> Result<Self> {
        let file =
            File::open(jar).with_context(|| format!("Open your 1.8.9 jar: {}", jar.display()))?;
        let mut zip = zip::ZipArchive::new(file)?;
        let files: std::collections::BTreeSet<String> = zip.file_names().map(str::to_owned)
            .chain(packs.names().map(|p| format!("assets/minecraft/{p}"))).collect();
        let mut read = |name: &str| -> Result<Vec<u8>> {
            if let Some(bytes) = name.strip_prefix("assets/minecraft/").and_then(|n| packs.get(n)) {
                return Ok(bytes.to_vec());
            }
            let mut bytes = Vec::new();
            zip.by_name(name)?.read_to_end(&mut bytes)?;
            Ok(bytes)
        };
        let mut load_colormap = |name: &str| -> Result<Vec<u32>> {
            let bytes = read(&format!("assets/minecraft/textures/colormap/{name}.png"))?;
            let image = image::load_from_memory(&bytes)?.to_rgb8();
            anyhow::ensure!(
                image.dimensions() == (256, 256),
                "Invalid original {name} colormap dimensions"
            );
            Ok(image
                .pixels()
                .map(|p| (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32)
                .collect())
        };
        let biome_colors = biome_colors::BiomeColors {
            grass: load_colormap("grass")?,
            foliage: load_colormap("foliage")?,
        };
        let mut names = files
            .iter().map(String::as_str)
            .filter(|name| {
                name.starts_with("assets/minecraft/textures/blocks/") && name.ends_with(".png")
            })
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let animated: std::collections::HashSet<String> = files
            .iter()
            .filter_map(|name| {
                let meta = name.strip_prefix("assets/minecraft/")?;
                let texture = meta.strip_suffix(".mcmeta")?;
                (texture.ends_with(".png") && packs.metadata_visible(texture, meta))
                    .then(|| format!("assets/minecraft/{texture}"))
            })
            .collect();
        names.sort_by(|a, b| {
            animated
                .contains(b)
                .cmp(&animated.contains(a))
                .then_with(|| a.cmp(b))
        });
        let mut decoded_tiles = Vec::with_capacity(names.len());
        let mut max_dim = 16u32;
        for name in names {
            let bytes = read(&name)?;
            let decoded = image::load_from_memory(&bytes)?.to_rgba8();
            let key = Path::new(&name)
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let frame_dim = decoded.width().min(decoded.height());
            let base_dim = if matches!(key.as_str(), "water_flow" | "lava_flow") {
                frame_dim / 2
            } else {
                frame_dim
            };
            max_dim = max_dim.max(base_dim);
            decoded_tiles.push((name, key, decoded));
        }
        let tile_size = 1u32 << (31 - max_dim.clamp(16, 256).leading_zeros());
        let layer_bytes = tile_size as usize * tile_size as usize * 4;
        let checker = (tile_size / 4).max(1);
        let mut animations = Vec::new();
        let mut flowing_tiles = [None, None];
        let mut pixels = Vec::with_capacity((decoded_tiles.len() + 1) * layer_bytes);
        let mut materials = HashMap::new();
        // Index zero is an explicit missing-texture tile for unsupported IDs.
        for y in 0..tile_size {
            for x in 0..tile_size {
                let v = if (x / checker + y / checker) % 2 == 0 {
                    [235, 20, 235, 255]
                } else {
                    [25, 25, 25, 255]
                };
                pixels.extend(v);
            }
        }
        for (name, key, decoded) in decoded_tiles {
            let metadata = animated.contains(&name).then(|| read(&format!("{name}.mcmeta"))).transpose()?;
            if let Some(metadata) = &metadata {
                animations.push(
                    animation::Clip::read(&decoded, metadata, tile_size)
                        .with_context(|| format!("Load animation {name}"))?,
                );
            }
            let frame = image::imageops::crop_imm(
                &decoded,
                0,
                0,
                decoded.width(),
                decoded.width().min(decoded.height()),
            )
            .to_image();
            let resized = image::imageops::resize(&frame, tile_size, tile_size, FilterType::Nearest);
            if let Some(slot) = ["water_flow", "lava_flow"].iter().position(|&flow| flow == key) {
                // A side face uses half this sprite's width. Keep the entire
                // 32-texel frame so that one block still displays 16 texels.
                flowing_tiles[slot] = Some(if let Some(metadata) = &metadata {
                    animation::Clip::read(&decoded, metadata, animation::FLOW_SIZE)?
                } else {
                    animation::Clip::still(frame, animation::FLOW_SIZE)
                });
            }
            materials.insert(key, (pixels.len() / layer_bytes) as u32);
            pixels.extend_from_slice(resized.as_raw());
        }
        let mut models = HashMap::new();
        let mut states = HashMap::new();
        let json_names = files
            .iter().map(String::as_str)
            .filter(|n| {
                n.ends_with(".json")
                    && (n.starts_with("assets/minecraft/models/block/")
                        || n.starts_with("assets/minecraft/blockstates/"))
            })
            .map(str::to_owned)
            .collect::<Vec<_>>();
        for name in json_names {
            let relative = name.trim_start_matches("assets/minecraft/");
            let bytes = if let Some(upper)=packs.get(relative).filter(|_|relative.starts_with("blockstates/")) {
                if let Ok(mut entry)=zip.by_name(&name) {
                    let mut lower=Vec::new();
                    entry.read_to_end(&mut lower)?;
                    crate::resource_packs::merge_blockstates(&lower,upper)
                } else { upper.to_vec() }
            } else if let Some(bytes)=packs.get(relative) {bytes.to_vec()} else {
                let mut bytes=Vec::new();zip.by_name(&name)?.read_to_end(&mut bytes)?;bytes
            };
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if let Some(name) = name.strip_prefix("assets/minecraft/models/") {
                    models.insert(name.trim_end_matches(".json").to_owned(), value);
                } else if let Some(name) = name.strip_prefix("assets/minecraft/blockstates/") {
                    states.insert(name.trim_end_matches(".json").to_owned(), value);
                }
            }
        }
        let mut fallback = HashMap::new();
        if let Ok(bytes) = std::fs::read(".local/resources/registry.json") {
            if let Ok(registry) = serde_json::from_slice::<crate::resources::Registry>(&bytes) {
                for (id, definition) in registry.blocks {
                    let Some(variants) = states
                        .get(&definition.name)
                        .and_then(|v| v["variants"].as_object())
                    else {
                        continue;
                    };
                    let Some(variant) = variants.get("normal").or_else(|| variants.values().next())
                    else {
                        continue;
                    };
                    let variant = variant
                        .as_array()
                        .and_then(|v| v.first())
                        .unwrap_or(variant);
                    let Some(name) = variant["model"].as_str() else {
                        continue;
                    };
                    let textures = model_textures(
                        &models,
                        &format!("block/{}", name.trim_start_matches("minecraft:")),
                        0,
                    );
                    let layers = std::array::from_fn(|face| {
                        let preferred = if face == 2 {
                            "bottom"
                        } else if face == 3 {
                            "top"
                        } else {
                            "side"
                        };
                        for key in [
                            preferred, "all", "texture", "particle", "end", "front", "lower",
                        ] {
                            if let Some(layer) = resolve_texture(&textures, key, &materials) {
                                return layer;
                            }
                        }
                        textures
                            .keys()
                            .find_map(|key| resolve_texture(&textures, key, &materials))
                            .unwrap_or(0)
                    });
                    fallback.insert(id, layers);
                }
            }
        }
        let block_models = block_models::BlockModels::load(&models, &states, &materials)?;
        Ok(Self {
            identity: Default::default(),
            animations: std::sync::Arc::new(animations),
            flowing_tiles: std::sync::Arc::new(flowing_tiles.into_iter()
                .collect::<Option<Vec<_>>>().context("Missing flowing water or lava texture")?),
            tile_size,
            layers: (pixels.len() / layer_bytes) as u32,
            pixels,
            materials,
            fallback,
            biome_colors,
            block_models,
        })
    }
    pub fn texture(&self, state: u16, axis: usize, positive: bool) -> u32 {
        let id = state >> 4;
        let meta = state & 15;
        if let Some(face) = self.block_models.cube(state, [4, 0, 2][axis] + usize::from(positive)) {
            return face.layer;
        }
        if id == 171 {
            return self.texture((35 << 4) | meta, axis, positive);
        }
        if matches!(id, 59 | 141 | 142) {
            let (crop, stage) = match id {
                59 => ("wheat", meta.min(7)),
                141 => ("carrots", if meta >= 7 { 3 } else { (meta / 2).min(2) }),
                _ => ("potatoes", if meta >= 7 { 3 } else { (meta / 2).min(2) }),
            };
            return self
                .materials
                .get(&format!("{crop}_stage_{stage}"))
                .copied()
                .unwrap_or(0);
        }
        if matches!(id, 64 | 71 | 193..=197) {
            let wood = match id {
                71 => "iron",
                193 => "spruce",
                194 => "birch",
                195 => "jungle",
                196 => "acacia",
                197 => "dark_oak",
                _ => "wood",
            };
            let half = if meta & 8 == 0 { "lower" } else { "upper" };
            return self
                .materials
                .get(&format!("door_{wood}_{half}"))
                .copied()
                .unwrap_or(0);
        }
        let name = match id {
            1 => match meta {
                1 => "stone_granite",
                2 => "stone_granite_smooth",
                3 => "stone_diorite",
                4 => "stone_diorite_smooth",
                5 => "stone_andesite",
                6 => "stone_andesite_smooth",
                _ => "stone",
            },
            2 => {
                if axis == 1 {
                    if positive {
                        "grass_top"
                    } else {
                        "dirt"
                    }
                } else {
                    "grass_side"
                }
            }
            3 => {
                if meta == 1 {
                    "coarse_dirt"
                } else {
                    "dirt"
                }
            }
            4 => "cobblestone",
            5 => match meta {
                1 => "planks_spruce",
                2 => "planks_birch",
                3 => "planks_jungle",
                4 => "planks_acacia",
                5 => "planks_big_oak",
                _ => "planks_oak",
            },
            6 => match meta & 7 {
                1 => "sapling_spruce",
                2 => "sapling_birch",
                3 => "sapling_jungle",
                4 => "sapling_acacia",
                5 => "sapling_roofed_oak",
                _ => "sapling_oak",
            },
            7 => "bedrock",
            8 | 9 => "water_still",
            10 | 11 => "lava_still",
            27 => {
                if meta & 8 != 0 {
                    "rail_golden_powered"
                } else {
                    "rail_golden"
                }
            }
            28 => {
                if meta & 8 != 0 {
                    "rail_detector_powered"
                } else {
                    "rail_detector"
                }
            }
            66 => {
                if rail_shape(state) >= 6 {
                    "rail_normal_turned"
                } else {
                    "rail_normal"
                }
            }
            157 => {
                if meta & 8 != 0 {
                    "rail_activator_powered"
                } else {
                    "rail_activator"
                }
            }
            12 => {
                if meta == 1 {
                    "red_sand"
                } else {
                    "sand"
                }
            }
            13 => "gravel",
            14 => "gold_ore",
            15 => "iron_ore",
            16 => "coal_ore",
            17 => match (meta & 3, axis == 1) {
                (1, true) => "log_spruce_top",
                (1, false) => "log_spruce",
                (2, true) => "log_birch_top",
                (2, false) => "log_birch",
                (3, true) => "log_jungle_top",
                (3, false) => "log_jungle",
                (_, true) => "log_oak_top",
                _ => "log_oak",
            },
            18 => match meta & 3 {
                1 => "leaves_spruce",
                2 => "leaves_birch",
                3 => "leaves_jungle",
                _ => "leaves_oak",
            },
            19 => "sponge",
            20 => "glass",
            21 => "lapis_ore",
            22 => "lapis_block",
            24 => {
                if axis == 1 {
                    "sandstone_top"
                } else {
                    "sandstone_normal"
                }
            }
            30 => "web",
            31 => match meta {
                0 => "deadbush",
                2 => "fern",
                _ => "tallgrass",
            },
            32 => "deadbush",
            35 => match meta {
                1 => "wool_colored_orange",
                2 => "wool_colored_magenta",
                3 => "wool_colored_light_blue",
                4 => "wool_colored_yellow",
                5 => "wool_colored_lime",
                6 => "wool_colored_pink",
                7 => "wool_colored_gray",
                8 => "wool_colored_silver",
                9 => "wool_colored_cyan",
                10 => "wool_colored_purple",
                11 => "wool_colored_blue",
                12 => "wool_colored_brown",
                13 => "wool_colored_green",
                14 => "wool_colored_red",
                15 => "wool_colored_black",
                _ => "wool_colored_white",
            },
            37 => "flower_dandelion",
            38 => match meta {
                1 => "flower_blue_orchid",
                2 => "flower_allium",
                3 => "flower_houstonia",
                4 => "flower_tulip_red",
                5 => "flower_tulip_orange",
                6 => "flower_tulip_white",
                7 => "flower_tulip_pink",
                8 => "flower_oxeye_daisy",
                _ => "flower_rose",
            },
            39 => "mushroom_brown",
            40 => "mushroom_red",
            41 => "gold_block",
            42 => "iron_block",
            43 | 44 => match meta & 7 {
                1 => {
                    if axis == 1 {
                        "sandstone_top"
                    } else {
                        "sandstone_normal"
                    }
                }
                2 => "planks_oak",
                3 => "cobblestone",
                4 => "brick",
                5 => "stonebrick",
                6 => "nether_brick",
                7 => "quartz_block_side",
                _ => {
                    if axis == 1 {
                        "stone_slab_top"
                    } else {
                        "stone_slab_side"
                    }
                }
            },
            45 => "brick",
            46 => "tnt_side",
            47 => {
                if axis == 1 {
                    "planks_oak"
                } else {
                    "bookshelf"
                }
            }
            48 => "cobblestone_mossy",
            49 => "obsidian",
            50 => "torch_on",
            52 => "mob_spawner",
            53 | 85 | 107 => "planks_oak",
            54 | 146 => "planks_oak",
            130 => "obsidian",
            56 => "diamond_ore",
            57 => "diamond_block",
            58 => {
                if axis == 1 {
                    "crafting_table_top"
                } else {
                    "crafting_table_side"
                }
            }
            60 => {
                if axis == 1 && positive {
                    if meta > 0 {
                        "farmland_wet"
                    } else {
                        "farmland_dry"
                    }
                } else {
                    "dirt"
                }
            }
            61 | 62 => {
                let front = match meta {
                    3 => axis == 2 && positive,
                    4 => axis == 0 && !positive,
                    5 => axis == 0 && positive,
                    _ => axis == 2 && !positive,
                };
                if axis == 1 {
                    "furnace_top"
                } else if front {
                    if id == 62 {
                        "furnace_front_on"
                    } else {
                        "furnace_front_off"
                    }
                } else {
                    "furnace_side"
                }
            }
            67 => "cobblestone",
            96 => "trapdoor",
            108 => "brick",
            109 => "stonebrick",
            113 | 114 => "nether_brick",
            125 | 126 => match meta & 7 {
                1 => "planks_spruce",
                2 => "planks_birch",
                3 => "planks_jungle",
                4 => "planks_acacia",
                5 => "planks_big_oak",
                _ => "planks_oak",
            },
            128 => {
                if axis == 1 {
                    "sandstone_top"
                } else {
                    "sandstone_normal"
                }
            }
            134 | 183 | 188 => "planks_spruce",
            135 | 184 | 189 => "planks_birch",
            136 | 185 | 190 => "planks_jungle",
            139 => {
                if meta == 1 {
                    "cobblestone_mossy"
                } else {
                    "cobblestone"
                }
            }
            156 => {
                if axis == 1 {
                    "quartz_block_top"
                } else {
                    "quartz_block_side"
                }
            }
            163 | 186 | 192 => "planks_acacia",
            164 | 187 | 191 => "planks_big_oak",
            167 => "iron_trapdoor",
            180..=182 => {
                if axis == 1 {
                    "red_sandstone_top"
                } else {
                    "red_sandstone_normal"
                }
            }
            73 | 74 => "redstone_ore",
            78 | 80 => "snow",
            79 => "ice",
            81 => {
                if axis == 1 {
                    "cactus_top"
                } else {
                    "cactus_side"
                }
            }
            82 => "clay",
            86 | 91 => "pumpkin_side",
            87 => "netherrack",
            88 => "soul_sand",
            89 => "glowstone",
            90 => "portal",
            92 => {
                if axis == 1 {
                    if positive {
                        "cake_top"
                    } else {
                        "cake_bottom"
                    }
                } else {
                    "cake_side"
                }
            }
            95 => "glass",
            98 => match meta {
                1 => "stonebrick_mossy",
                2 => "stonebrick_cracked",
                3 => "stonebrick_carved",
                _ => "stonebrick",
            },
            103 => "melon_side",
            110 => {
                if axis == 1 && positive {
                    "mycelium_top"
                } else {
                    "mycelium_side"
                }
            }
            112 => "nether_brick",
            116 => {
                if axis == 1 {
                    if positive {
                        "enchanting_table_top"
                    } else {
                        "enchanting_table_bottom"
                    }
                } else {
                    "enchanting_table_side"
                }
            }
            117 => "brewing_stand",
            118 => {
                if axis == 1 {
                    if positive {
                        "cauldron_top"
                    } else {
                        "cauldron_bottom"
                    }
                } else {
                    "cauldron_side"
                }
            }
            120 => {
                if axis == 1 {
                    if positive {
                        "endframe_top"
                    } else {
                        "end_stone"
                    }
                } else {
                    "endframe_side"
                }
            }
            121 => "end_stone",
            122 => "dragon_egg",
            129 => "emerald_ore",
            132 => "trip_wire",
            133 => "emerald_block",
            138 => "beacon",
            140 => "flower_pot",
            145 => {
                if axis == 1 && positive {
                    match (meta >> 2).min(2) {
                        1 => "anvil_top_damaged_1",
                        2 => "anvil_top_damaged_2",
                        _ => "anvil_top_damaged_0",
                    }
                } else {
                    "anvil_base"
                }
            }
            152 => "redstone_block",
            153 => "quartz_ore",
            155 => "quartz_block_side",
            159 => "hardened_clay_stained_white",
            161 if meta & 3 == 1 => "leaves_big_oak",
            161 => "leaves_acacia",
            162 => {
                if axis == 1 {
                    "log_acacia_top"
                } else {
                    "log_acacia"
                }
            }
            165 => "slime",
            168 => "prismarine_rough",
            169 => "sea_lantern",
            170 => "hay_block_side",
            172 => "hardened_clay",
            173 => "coal_block",
            174 => "ice_packed",
            175 => match meta {
                0 => "double_plant_sunflower_bottom",
                1 => "double_plant_syringa_bottom",
                2 => "double_plant_grass_bottom",
                3 => "double_plant_fern_bottom",
                4 => "double_plant_rose_bottom",
                5 => "double_plant_paeonia_bottom",
                8 => "double_plant_sunflower_top",
                9 => "double_plant_syringa_top",
                10 => "double_plant_grass_top",
                11 => "double_plant_fern_top",
                12 => "double_plant_rose_top",
                13 => "double_plant_paeonia_top",
                _ => "tallgrass",
            },
            179 => "red_sandstone_normal",
            _ => {
                return self
                    .fallback
                    .get(&id)
                    .map(|f| f[axis * 2 + positive as usize])
                    .unwrap_or(0)
            }
        };
        self.materials.get(name).copied().unwrap_or(0)
    }
    pub fn particle_texture(&self, state: u16) -> u32 {
        let id = state >> 4;
        match id {
            54 | 63 | 68 | 146 | 176 | 177 => {
                return *self.materials.get("planks_oak").unwrap_or(&0);
            }
            119 | 130 => return *self.materials.get("obsidian").unwrap_or(&0),
            8 | 9 => return *self.materials.get("water_still").unwrap_or(&0),
            10 | 11 => return *self.materials.get("lava_still").unwrap_or(&0),
            144 => return *self.materials.get("soul_sand").unwrap_or(&0),
            36 => return *self.materials.get("piston_side").unwrap_or(&0),
            _ => {}
        }
        if let Some(layer) = self.block_models.particle(state) {
            if layer != 0 {
                return layer;
            }
        }
        if matches!(id, 2 | 110) || (id == 3 && (state & 15) == 2) {
            return *self.materials.get("dirt").unwrap_or(&0);
        }
        if id == 160 {
            let color = [
                "glass_white",
                "glass_orange",
                "glass_magenta",
                "glass_light_blue",
                "glass_yellow",
                "glass_lime",
                "glass_pink",
                "glass_gray",
                "glass_silver",
                "glass_cyan",
                "glass_purple",
                "glass_blue",
                "glass_brown",
                "glass_green",
                "glass_red",
                "glass_black",
            ][(state & 15) as usize];
            return *self.materials.get(color).unwrap_or(&0);
        }
        self.texture(state, 2, true)
    }
}
fn model_textures(
    models: &HashMap<String, serde_json::Value>,
    name: &str,
    depth: u8,
) -> std::collections::BTreeMap<String, String> {
    if depth > 16 {
        return Default::default();
    }
    let Some(model) = models.get(name.trim_start_matches("minecraft:")) else {
        return Default::default();
    };
    let mut textures = model
        .get("parent")
        .and_then(|p| p.as_str())
        .map(|p| model_textures(models, p, depth + 1))
        .unwrap_or_default();
    if let Some(map) = model["textures"].as_object() {
        for (k, v) in map {
            if let Some(v) = v.as_str() {
                textures.insert(k.clone(), v.into());
            }
        }
    }
    textures
}
fn resolve_texture(
    textures: &std::collections::BTreeMap<String, String>,
    key: &str,
    materials: &HashMap<String, u32>,
) -> Option<u32> {
    let mut value = textures.get(key)?.as_str();
    for _ in 0..16 {
        if let Some(key) = value.strip_prefix('#') {
            value = textures.get(key)?;
        } else {
            return materials
                .get(
                    value
                        .trim_start_matches("minecraft:")
                        .trim_start_matches("blocks/"),
                )
                .copied();
        }
    }
    None
}
#[cfg(test)]
pub const HOTBAR: [u16; 9] = [
    2 << 4,
    1 << 4,
    3 << 4,
    4 << 4,
    5 << 4,
    17 << 4,
    20 << 4,
    45 << 4,
    89 << 4,
];
pub fn solid(state: u16) -> bool {
    !matches!(
        state >> 4,
        0 | 6
            | 8
            | 9
            | 10
            | 11
            | 30
            | 31
            | 32
            | 37
            | 38
            | 39
            | 40
            | 50
            | 51
            | 55
            | 59
            | 63
            | 65
            | 66
            | 68
            | 69
            | 70
            | 72
            | 75
            | 76
            | 77
            | 83
            | 90
            | 104
            | 105
            | 106
            | 115
            | 127
            | 131
            | 132
            | 140
            | 141
            | 142
            | 143
            | 147
            | 148
            | 157
            | 171
            | 175
            | 176
            | 177
    )
}
pub fn see_through(state: u16) -> bool {
    !solid(state)
        || matches!(
            state >> 4,
            18 | 20 | 29 | 33 | 34 | 36 | 79 | 95 | 96 | 101 | 102 | 119 | 138 | 144 | 160 | 161 | 165 | 166 | 167
        )
}
pub fn crossed(state: u16) -> bool {
    matches!(
        state >> 4,
        6 | 31 | 32 | 37 | 38 | 39 | 40 | 59 | 83 | 141 | 142 | 175
    )
}

/// These blocks cannot be merged into the full-cube face mask or occlude an
/// adjacent cube's whole face. Their mesh uses state-dependent model bounds.
pub fn partial_model(state: u16) -> bool {
    block_models::BlockModels::static_model(state >> 4) || matches!(
        state >> 4,
        26 | 27 | 36 | 55
            | 28
            | 44
            | 50
            | 75
            | 76
            | 53
            | 54
            | 60
            | 64
            | 66
            | 67
            | 71
            | 78
            | 85
            | 88
            | 96
            | 101
            | 102
            | 107
            | 108
            | 109
            | 111
            | 113
            | 114
            | 126
            | 128
            | 130
            | 134
            | 135
            | 136
            | 139
            | 146
            | 156
            | 157
            | 160
            | 163
            | 164
            | 167
            | 171
            | 180
            | 182
            | 183..=197
    )
}

/// Normal rail metadata has ten shapes; powered/detector/activator rails keep
/// their powered bit separately and support only straight/ascending shapes.
pub fn rail_shape(state: u16) -> u16 {
    let shape = state & if state >> 4 == 66 { 15 } else { 7 };
    if shape <= if state >> 4 == 66 { 9 } else { 5 } {
        shape
    } else {
        0
    }
}
