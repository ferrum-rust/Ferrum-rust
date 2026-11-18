//! World types selected by GuiCreateWorld. The existing default provider owns its own cache.
use crate::{
    vanilla::{generator, terrain::Settings},
    world::{Chunk, ChunkPos},
    world_library::Rules,
    Options,
};
use anyhow::{bail, ensure, Context, Result};
use glam::Vec3;
use rayon::prelude::*;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

enum Provider {
    Flat(Arc<Chunk>),
    Debug,
    Landscape(Box<generator::Generator>),
}

enum TerrainType {
    Default,
    Flat,
    Debug,
    Landscape(Settings, bool),
}

fn terrain(rules: &Rules) -> Result<TerrainType> {
    Ok(match rules.world_type.to_ascii_lowercase().as_str() {
        "default" => TerrainType::Default,
        "flat" => TerrainType::Flat,
        "debug_all_block_states" => TerrainType::Debug,
        "largebiomes" => TerrainType::Landscape(
            Settings {
                biome_size: 6,
                ..Settings::default()
            },
            false,
        ),
        "amplified" => TerrainType::Landscape(Settings::default(), true),
        "customized" => {
            let settings: Settings = if rules.generator_options.is_empty() {
                Settings::default()
            } else {
                serde_json::from_str(&rules.generator_options)
                    .context("Invalid customized terrain settings")?
            };
            ensure!(
                settings.biome_size <= 8
                    && settings.river_size <= 8
                    && (0..=255).contains(&settings.sea_level),
                "Invalid customized biome size, river size or sea level"
            );
            TerrainType::Landscape(settings, false)
        }
        other => bail!("Unsupported world type {other}"),
    })
}

pub fn validate(rules: &Rules) -> Result<()> {
    terrain(rules).map(|_| ())
}

impl Provider {
    fn new(seed: u64, rules: &Rules) -> Result<Self> {
        Ok(match terrain(rules)? {
            TerrainType::Default => {
                Self::Landscape(Box::new(generator::Generator::new(seed as i64)))
            }
            TerrainType::Flat => Self::Flat(Arc::new(flat_chunk(&rules.generator_options))),
            TerrainType::Debug => Self::Debug,
            TerrainType::Landscape(settings, amplified) => Self::Landscape(Box::new(
                generator::Generator::configured(seed as i64, settings, amplified),
            )),
        })
    }

    fn chunk(&self, pos: ChunkPos) -> Result<Arc<Chunk>> {
        Ok(match self {
            Self::Flat(chunk) => Arc::clone(chunk),
            Self::Debug => Arc::new(debug_chunk(pos)),
            Self::Landscape(g) => Arc::new(g.chunk(pos)?),
        })
    }
}

struct CachedProvider {
    seed: u64,
    rules: Rules,
    provider: Arc<Provider>,
}

static WORLD_TYPE_CACHE: OnceLock<Mutex<Option<CachedProvider>>> = OnceLock::new();

pub fn clear_cache() {
    if let Some(cache) = WORLD_TYPE_CACHE.get() {
        *cache.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

fn shared(options: &Options) -> Result<Arc<Provider>> {
    let mut current = WORLD_TYPE_CACHE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(cached) = current
        .as_ref()
        .filter(|c| c.seed == options.seed && c.rules == options.world_rules)
    {
        return Ok(Arc::clone(&cached.provider));
    }
    let provider = Arc::new(Provider::new(options.seed, &options.world_rules)?);
    *current = Some(CachedProvider {
        seed: options.seed,
        rules: options.world_rules.clone(),
        provider: Arc::clone(&provider),
    });
    Ok(provider)
}

pub fn read_chunks(
    options: &Options,
    positions: &[ChunkPos],
) -> Result<Vec<(ChunkPos, Arc<Chunk>)>> {
    if options
        .world_rules
        .world_type
        .eq_ignore_ascii_case("default")
    {
        return generator::read_chunks(options.seed, positions);
    }
    let provider = shared(options)?;
    positions
        .par_iter()
        .map(|&pos| Ok((pos, provider.chunk(pos)?)))
        .collect()
}

pub fn spawn(options: &Options) -> Result<Vec3> {
    if options
        .world_rules
        .world_type
        .eq_ignore_ascii_case("default")
    {
        return generator::preview_spawn(options.seed);
    }
    // Bounded preview even for custom presets without any grass (e.g. water world).
    let chunk = shared(options)?.chunk((0, 0))?;
    let top = (0..256)
        .rev()
        .find(|&y| chunk.get(0, y, 0) != 0)
        .map_or(64, |y| y + 1);
    Ok(Vec3::new(0.5, top as f32 + 0.01, 0.5))
}

pub(crate) type Blocks = HashMap<String, (u16, [i8; 16])>;
const CLASSIC: &str = "3;minecraft:bedrock,2*minecraft:dirt,minecraft:grass;1;village";

struct FlatLayer {
    count: usize,
    state: u16,
    name: &'static str,
}

struct Flat<'a> {
    layers: Vec<FlatLayer>,
    biome: i32,
    features: &'a str,
}

pub(crate) fn blocks() -> &'static Blocks {
    static REGISTRY: OnceLock<Blocks> = OnceLock::new();
    REGISTRY.get_or_init(|| serde_json::from_str(include_str!("vanilla/world_flat_blocks.json")).expect("original block names"))
}
fn parse_flat(options: &str) -> Option<Flat<'_>> {
    let registry = blocks();
    let mut parts = options.split(';');
    let first = parts.next()?;
    let (version, layer_text) = match parts.next() {
        Some(layers) => (first.parse::<i32>().unwrap_or(0), layers),
        None => (0, first),
    };
    if !(0..=3).contains(&version) {
        return None;
    }
    let mut layers = Vec::new();
    let mut remaining = 256;
    for layer in layer_text.trim_end_matches(',').split(',') {
        let (count, block) = match layer.split_once(if version < 3 { 'x' } else { '*' }) {
            Some((n, b)) => (n.parse::<i32>().ok()?.max(0) as usize, b),
            None => (1, layer),
        };
        let block = block.strip_prefix("minecraft:").unwrap_or(block);
        let (key, meta) = match block.split_once(':') {
            Some((n, m)) => (n, m.parse::<i32>().ok()?),
            None => (block, 0),
        };
        let (name, (id, metadata)) = if let Ok(id) = key.parse::<i32>() {
            registry
                .iter()
                .find(|(_, (i, _))| i32::from(*i) == id)
                .or_else(|| registry.get_key_value("air"))
        } else if version == 3 {
            registry.get_key_value(key)
        } else {
            None
        }?;
        let meta = if (0..16).contains(&meta) {
            meta as usize
        } else {
            0
        };
        let canonical = u16::try_from(metadata[meta]).ok()?;
        let count = count.min(remaining);
        layers.push(FlatLayer {
            count,
            state: id << 4 | canonical,
            name,
        });
        remaining -= count;
    }
    let biome = if version > 0 {
        parts.next().and_then(|s| s.parse().ok()).unwrap_or(1)
    } else {
        1
    };
    let features = if version > 0 {
        parts.next().unwrap_or("village")
    } else {
        "village"
    };
    Some(Flat {
        layers,
        biome,
        features,
    })
}

fn normalize_features(text: &str) -> String {
    use crate::vanilla::java_map::JavaMap;
    let lower = text.to_lowercase();
    let mut features = JavaMap::default();
    for entry in lower.split(',') {
        let (name, options) = entry.split_once('(').unwrap_or((entry, ""));
        if name.is_empty() {
            continue;
        }
        let mut parameters = JavaMap::default();
        for (key, value) in options
            .strip_suffix(')')
            .unwrap_or("")
            .split(' ')
            .filter_map(|s| s.split_once('='))
        {
            parameters.insert(key.to_owned(), value);
        }
        let params = parameters
            .entries()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>();
        let suffix = if params.is_empty() {
            String::new()
        } else {
            format!("({})", params.join(" "))
        };
        features.insert(name.to_owned(), suffix);
    }
    features
        .entries()
        .map(|(k, v)| format!("{k}{v}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Use the same bytecode-derived state/metadata map for the flat editor and chunk generation.
/// Upgrade old numeric layer codes to named v3 codes so selecting a legacy preset can round-trip.
pub fn normalize_flat(options: &str) -> String {
    let Some(flat) = parse_flat(options) else {
        return CLASSIC.to_owned();
    };
    let layers = flat
        .layers
        .iter()
        .map(|layer| {
            let count = if layer.count == 1 {
                String::new()
            } else {
                format!("{}*", layer.count)
            };
            let meta = layer.state & 15;
            let data = if meta == 0 {
                String::new()
            } else {
                format!(":{meta}")
            };
            format!("{count}minecraft:{}{data}", layer.name)
        })
        .collect::<Vec<_>>();
    format!(
        "3;{};{};{}",
        layers.join(","),
        flat.biome,
        normalize_features(flat.features)
    )
}

fn flat_chunk(options: &str) -> Chunk {
    let flat = parse_flat(options).unwrap_or_else(|| parse_flat(CLASSIC).unwrap());
    let mut chunk = Chunk::empty();
    chunk
        .biomes
        .fill(crate::vanilla::biome::get_biome(flat.biome).map_or(0, |b| b.id));
    let mut start = 0;
    for layer in flat.layers {
        let end = start + layer.count;
        if layer.state != 0 && start < end {
            chunk.blocks[start * 256..end * 256].fill(layer.state);
            chunk.top = end as i32;
        }
        start = end;
    }
    chunk
}

fn debug_chunk((cx, cz): ChunkPos) -> Chunk {
    const STATES: &[u8] = include_bytes!("vanilla/world_debug_states.bin");
    let count = STATES.len() / 2;
    let width = (count as f64).sqrt().ceil() as usize;
    let height = count.div_ceil(width);
    let mut chunk = Chunk::empty();
    chunk.blocks[60 * 256..61 * 256].fill(166 << 4);
    chunk.top = 61;
    for z in 0..16 {
        for x in 0..16 {
            let gx = cx.wrapping_mul(16).wrapping_add(x);
            let gz = cz.wrapping_mul(16).wrapping_add(z);
            if gx <= 0 || gz <= 0 || gx % 2 == 0 || gz % 2 == 0 {
                continue;
            }
            let (row, col) = (gx as usize / 2, gz as usize / 2);
            if row <= width && col <= height {
                let index = (row * width + col) * 2;
                if let Some(pair) = STATES.get(index..index + 2) {
                    chunk.set(x, 70, z, u16::from_le_bytes([pair[0], pair[1]]));
                }
            }
        }
    }
    chunk
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Sample {
        #[serde(rename = "type")]
        kind: String,
        options: String,
        pos: ChunkPos,
        blocks: String,
        biomes: String,
    }

    fn hash(bytes: impl IntoIterator<Item = u8>) -> String {
        format!(
            "{:x}",
            bytes
                .into_iter()
                .fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(b))
                    .wrapping_mul(0x100000001b3))
        )
    }

    #[test]
    fn original_world_type_chunks() {
        let samples: Vec<Sample> = serde_json::from_str(include_str!(
            "../tests/fixtures/vanilla189-world-terrain.json"
        ))
        .unwrap();
        let mut differences = Vec::new();
        for (i, sample) in samples.into_iter().enumerate() {
            let rules = Rules {
                world_type: sample.kind,
                generator_options: sample.options,
                ..Rules::default()
            };
            let chunk = Provider::new(189, &rules)
                .unwrap()
                .chunk(sample.pos)
                .unwrap();
            let blocks = hash(chunk.blocks.iter().flat_map(|s| s.to_le_bytes()));
            let biomes = hash(chunk.biomes);
            if (blocks.as_str(), biomes.as_str()) != (&sample.blocks, &sample.biomes) {
                differences.push(format!(
                    "{i} {} {:?} blocks {blocks}/{} biomes {biomes}/{}",
                    rules.world_type, sample.pos, sample.blocks, sample.biomes
                ));
            }
        }
        assert!(differences.is_empty(), "{}", differences.join("\n"));
    }
}
