//! Read-only distant surface data. This source never owns or requests native
//! world chunks: its terrain, Random instances and rebuildable cache are purely
//! visual, independent of population, player edits and saved simulation state.
use crate::{
    vanilla::{generator::Generator, terrain::Settings},
    world::Chunk,
    Options,
};
use anyhow::{ensure, Context, Result};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

pub const TILE_CHUNKS: i32 = 8;
pub const TILE_BLOCKS: i32 = 128;
pub const GRID: usize = 32;
pub const STEP: i32 = 4;
pub type TilePos = (i32, i32);
const MAGIC: &[u8; 8] = b"FERLOD01";
const VERSION: u32 = 1;
const HEADER_BYTES: usize = 44;
const CACHE_BYTES: usize = HEADER_BYTES + GRID * GRID * 5 + 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Surface {
    /// Y coordinate of the top face, 0 for an empty column, at most 256.
    pub height: u16,
    pub state: u16,
    pub biome: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainTile {
    pub position: TilePos,
    /// Z-major/X-inner; (x,z) samples the center of its 4×4 block cell.
    pub heights: Vec<Surface>,
}

#[derive(Clone)]
pub struct DistantTerrain {
    generator: Arc<Generator>,
    seed: u64,
    settings_hash: u64,
    cache_root: PathBuf,
}

impl DistantTerrain {
    /// Unsupported/imported worlds must not be filled with unrelated terrain.
    /// The caller owns its bounded low-priority worker pool and visible tiles.
    pub fn new(options: &Options) -> Result<Option<Self>> {
        if options.world.is_some()
            || options.connect.is_some()
            || options.structure_preview.is_some()
            || options.generator < 3
        {
            return Ok(None);
        }
        let rules = &options.world_rules;
        let kind = rules.world_type.to_ascii_lowercase();
        let generator = match kind.as_str() {
            "default" => Generator::new(options.seed as i64),
            "largebiomes" => Generator::configured(
                options.seed as i64,
                Settings {
                    biome_size: 6,
                    ..Settings::default()
                },
                false,
            ),
            "amplified" => Generator::configured(options.seed as i64, Settings::default(), true),
            "customized" => {
                // Use the same validation and settings interpretation as the
                // playable world's configured pre-population terrain provider.
                crate::world_types::validate(rules)?;
                let settings = if rules.generator_options.is_empty() {
                    Settings::default()
                } else {
                    serde_json::from_str(&rules.generator_options)
                        .context("Read distant customized terrain settings")?
                };
                Generator::configured(options.seed as i64, settings, false)
            }
            _ => return Ok(None),
        };
        let settings_hash = checksum(
            format!(
                "original189-terrain-v1\0{kind}\0{}",
                rules.generator_options
            )
            .as_bytes(),
        );
        let cache_root = options
            .project_dir
            .join(".cache")
            .join("lod189")
            .join(format!("{:016x}-{settings_hash:016x}", options.seed));
        Ok(Some(Self {
            generator: Arc::new(generator),
            seed: options.seed,
            settings_hash,
            cache_root,
        }))
    }

    /// One tile at a time: at most one full generated chunk is retained while
    /// extracting the 1024 samples. No nested Rayon/global pool is used here.
    pub fn tile(&self, position: TilePos) -> Result<TerrainTile> {
        validate_position(position)?;
        if let Some(tile) = self.read_cache(position) {
            return Ok(tile);
        }
        let mut tile = TerrainTile {
            position,
            heights: vec![Surface::default(); GRID * GRID],
        };
        for cz in 0..TILE_CHUNKS {
            for cx in 0..TILE_CHUNKS {
                let chunk = self
                    .generator
                    .chunk((position.0 * TILE_CHUNKS + cx, position.1 * TILE_CHUNKS + cz))?;
                for z in 0..4 {
                    for x in 0..4 {
                        let gx = cx * 4 + x;
                        let gz = cz * 4 + z;
                        tile.heights[gz as usize * GRID + gx as usize] =
                            surface(&chunk, x * STEP + STEP / 2, z * STEP + STEP / 2);
                    }
                }
            }
        }
        // Cache failure must not turn otherwise valid visual terrain into a
        // world-load error. No player save is read or written on this path.
        let _ = self.write_cache(&tile);
        Ok(tile)
    }

    fn cache_path(&self, position: TilePos) -> PathBuf {
        self.cache_root
            .join(format!("{}_{}.lod", position.0, position.1))
    }

    fn read_cache(&self, position: TilePos) -> Option<TerrainTile> {
        let mut bytes = Vec::with_capacity(CACHE_BYTES);
        // A corrupt file cannot force an unbounded allocation or read.
        File::open(self.cache_path(position))
            .ok()?
            .take((CACHE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .ok()?;
        decode(&bytes, self.seed, self.settings_hash, position)
    }

    fn write_cache(&self, tile: &TerrainTile) -> Result<()> {
        fs::create_dir_all(&self.cache_root)?;
        let bytes = encode(tile, self.seed, self.settings_hash);
        atomic_cache(&self.cache_path(tile.position), &bytes)
    }
}

/// The renderer may replace pure-terrain cells using an already authoritative
/// nearby chunk. Water/ice/lava, leaves and snow remain surfaces. Non-solid
/// decorations must not become opaque 4×4 plates in the distant height mesh.
pub fn surface(chunk: &Chunk, x: i32, z: i32) -> Surface {
    let biome = chunk.biomes[(z * 16 + x) as usize];
    for y in (0..256).rev() {
        let state = chunk.get(x, y, z);
        if (crate::vanilla::material::is_solid(state) && !matches!(state >> 4, 96 | 140 | 145 | 167))
            || crate::vanilla::material::is_liquid(state)
            || crate::vanilla::material::is_leaves(state)
            || state >> 4 == 78
        {
            return Surface {
                height: (y + 1) as u16,
                state,
                biome,
            };
        }
    }
    Surface {
        biome,
        ..Surface::default()
    }
}

fn validate_position((x, z): TilePos) -> Result<()> {
    ensure!(
        (-234_375..234_375).contains(&x) && (-234_375..234_375).contains(&z),
        "Distant tile exceeds original world coordinate limits"
    );
    Ok(())
}

fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}

fn encode(tile: &TerrainTile, seed: u64, settings_hash: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(CACHE_BYTES);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend_from_slice(&seed.to_le_bytes());
    bytes.extend_from_slice(&settings_hash.to_le_bytes());
    bytes.extend_from_slice(&tile.position.0.to_le_bytes());
    bytes.extend_from_slice(&tile.position.1.to_le_bytes());
    bytes.extend_from_slice(&(GRID as u16).to_le_bytes());
    bytes.extend_from_slice(&(STEP as u16).to_le_bytes());
    bytes.extend_from_slice(&(tile.heights.len() as u32).to_le_bytes());
    for sample in &tile.heights {
        bytes.extend_from_slice(&sample.height.to_le_bytes());
        bytes.extend_from_slice(&sample.state.to_le_bytes());
        bytes.push(sample.biome);
    }
    let hash = checksum(&bytes);
    bytes.extend_from_slice(&hash.to_le_bytes());
    bytes
}

fn decode(bytes: &[u8], seed: u64, settings_hash: u64, position: TilePos) -> Option<TerrainTile> {
    if bytes.len() != CACHE_BYTES || &bytes[..8] != MAGIC {
        return None;
    }
    let u16_at = |i| u16::from_le_bytes(bytes[i..i + 2].try_into().unwrap());
    let u32_at = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    let u64_at = |i| u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap());
    if u32_at(8) != VERSION
        || u64_at(12) != seed
        || u64_at(20) != settings_hash
        || u32_at(28) as i32 != position.0
        || u32_at(32) as i32 != position.1
        || u16_at(36) != GRID as u16
        || u16_at(38) != STEP as u16
        || u32_at(40) != (GRID * GRID) as u32
        || u64_at(CACHE_BYTES - 8) != checksum(&bytes[..CACHE_BYTES - 8])
    {
        return None;
    }
    let mut heights = Vec::with_capacity(GRID * GRID);
    for sample in bytes[HEADER_BYTES..CACHE_BYTES - 8].chunks_exact(5) {
        let height = u16::from_le_bytes([sample[0], sample[1]]);
        let state = u16::from_le_bytes([sample[2], sample[3]]);
        let biome = sample[4];
        if height > 256
            || (height == 0) != (state == 0)
            || (state != 0 && state >> 4 == 0)
            || state >> 4 > 197
            || crate::vanilla::biome::get_biome(i32::from(biome)).is_none()
        {
            return None;
        }
        heights.push(Surface {
            height,
            state,
            biome,
        });
    }
    Some(TerrainTile { position, heights })
}

fn atomic_cache(path: &Path, bytes: &[u8]) -> Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temporary = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.flush()?;
        drop(file);
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
#[path = "distant_terrain_tests.rs"]
mod tests;
