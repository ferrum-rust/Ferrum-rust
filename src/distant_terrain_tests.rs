use super::*;
use clap::Parser;
use serde_json::Value;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "ferrum-lod-source-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        if let (Ok(target), Ok(base)) = (self.0.canonicalize(), std::env::temp_dir().canonicalize())
        {
            if target.parent() == Some(base.as_path())
                && target
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("ferrum-lod-source-"))
            {
                let _ = fs::remove_dir_all(target);
            }
        }
    }
}
fn options(project: &Path, seed: i64) -> Options {
    let mut options = Options::parse_from(["ferrum189"]);
    options.project_dir = project.to_owned();
    options.seed = seed as u64;
    options
}
fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../tests/fixtures/vanilla189-distant-surfaces.json"
    ))
    .unwrap()
}

#[test]
fn authoritative_surface_skips_decorations_but_retains_leaves_snow_and_liquids() {
    let mut chunk = Chunk::empty();
    chunk.set(2, 63, 2, 2 << 4);
    for id in [6, 31, 37, 38, 50, 51, 55, 59, 65, 66, 83, 106, 111, 175] {
        chunk.set(2, 64, 2, id << 4);
        assert_eq!(
            surface(&chunk, 2, 2),
            Surface {
                height: 64,
                state: 2 << 4,
                biome: 1
            },
            "decoration {id}"
        );
    }
    for id in [8, 9, 10, 11, 18, 78, 79, 161, 174] {
        chunk.set(2, 64, 2, id << 4);
        assert_eq!(
            surface(&chunk, 2, 2),
            Surface {
                height: 65,
                state: id << 4,
                biome: 1
            },
            "surface {id}"
        );
    }
    assert_eq!(
        surface(&chunk, 3, 3),
        Surface {
            biome: 1,
            ..Surface::default()
        }
    );
}

#[test]
fn distant_surface_240_samples_match_original_carved_chunk_bytecode() {
    let temporary = Temporary::new();
    let fixture = fixture();
    let mut checked = 0;
    for case in fixture["cases"].as_array().unwrap() {
        let seed = case["seed"].as_str().unwrap().parse::<i64>().unwrap();
        let source = DistantTerrain::new(&options(&temporary.0, seed))
            .unwrap()
            .unwrap();
        let position = (
            case["chunk"][0].as_i64().unwrap() as i32,
            case["chunk"][1].as_i64().unwrap() as i32,
        );
        let chunk = source.generator.chunk(position).unwrap();
        for (index, expected) in case["samples"].as_array().unwrap().iter().enumerate() {
            let sample = surface(
                &chunk,
                (index % 4) as i32 * STEP + 2,
                (index / 4) as i32 * STEP + 2,
            );
            assert_eq!(
                serde_json::json!([sample.height, sample.state, sample.biome]),
                *expected,
                "seed {seed} original chunk {position:?} cell {index}"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 240);
}

#[test]
fn negative_tile_mapping_cache_reopen_and_corruption_regeneration_preserve_surfaces() {
    fn require_send_sync<T: Send + Sync>() {}
    require_send_sync::<DistantTerrain>();
    let temporary = Temporary::new();
    let options = options(&temporary.0, 189);
    let source = DistantTerrain::new(&options).unwrap().unwrap();
    let before = Instant::now();
    let tile = source.tile((-1, 0)).unwrap();
    let cold_ms = before.elapsed().as_secs_f64() * 1000.;
    assert_eq!(tile.heights.len(), 1024);
    let fixture = fixture();
    let original = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["seed"] == "189" && case["chunk"] == serde_json::json!([-1, 2]))
        .unwrap();
    // Negative tile (-1,0) contains chunks X -8..-1, Z 0..7. The
    // original reference chunk (-1,2) begins at cell (28,8).
    for (index, expected) in original["samples"].as_array().unwrap().iter().enumerate() {
        let sample = tile.heights[(8 + index / 4) * GRID + 28 + index % 4];
        assert_eq!(
            serde_json::json!([sample.height, sample.state, sample.biome]),
            *expected
        );
    }
    let cache = source.cache_path((-1, 0));
    assert_eq!(fs::metadata(&cache).unwrap().len(), CACHE_BYTES as u64);
    let reopened = DistantTerrain::new(&options).unwrap().unwrap();
    let before = Instant::now();
    assert_eq!(reopened.tile((-1, 0)).unwrap(), tile);
    let cached_ms = before.elapsed().as_secs_f64() * 1000.;
    let mut corrupt = fs::read(&cache).unwrap();
    corrupt[HEADER_BYTES + 3] ^= 0x80;
    fs::write(&cache, corrupt).unwrap();
    assert!(reopened.read_cache((-1, 0)).is_none());
    assert_eq!(reopened.tile((-1, 0)).unwrap(), tile);
    assert_eq!(reopened.read_cache((-1, 0)).unwrap(), tile);
    eprintln!(
        "distant tile: cold {cold_ms:.3} ms, cached {cached_ms:.3} ms, {} bytes",
        CACHE_BYTES
    );
    assert!(
        !temporary.0.join("saves").exists(),
        "visual cache must never create player saves"
    );
    assert!(source.tile((234375, 0)).is_err());
    assert!(source.tile((i32::MIN, 0)).is_err());
}

#[test]
fn distant_cache_rejects_wrong_identity_truncation_and_invalid_surface_even_with_checksum() {
    let tile = TerrainTile {
        position: (-234375, 234374),
        heights: vec![
            Surface {
                height: 256,
                state: 174 << 4,
                biome: 12
            };
            1024
        ],
    };
    let bytes = encode(&tile, u64::MAX, 123);
    assert_eq!(bytes.len(), CACHE_BYTES);
    assert_eq!(
        decode(&bytes, u64::MAX, 123, tile.position),
        Some(tile.clone())
    );
    for end in 0..bytes.len() {
        assert!(decode(&bytes[..end], u64::MAX, 123, tile.position).is_none());
    }
    assert!(decode(&bytes, 0, 123, tile.position).is_none());
    assert!(decode(&bytes, u64::MAX, 124, tile.position).is_none());
    assert!(decode(&bytes, u64::MAX, 123, (0, 0)).is_none());
    for (offset, value) in [
        (HEADER_BYTES, 1u8),
        (HEADER_BYTES + 1, 2),
        (HEADER_BYTES + 3, 0xff),
        (HEADER_BYTES + 4, 255),
    ] {
        let mut invalid = bytes.clone();
        invalid[offset] = value;
        let sum = checksum(&invalid[..CACHE_BYTES - 8]);
        invalid[CACHE_BYTES - 8..].copy_from_slice(&sum.to_le_bytes());
        assert!(decode(&invalid, u64::MAX, 123, tile.position).is_none());
    }
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(decode(&extra, u64::MAX, 123, tile.position).is_none());
}

#[test]
fn distant_configuration_isolates_seed_settings_and_unsupported_world_sources() {
    let temporary = Temporary::new();
    let mut opts = options(&temporary.0, 189);
    let default = DistantTerrain::new(&opts).unwrap().unwrap();
    opts.seed = 190;
    let other = DistantTerrain::new(&opts).unwrap().unwrap();
    assert_ne!(default.cache_root, other.cache_root);
    for kind in ["largeBiomes", "amplified", "customized"] {
        opts.world_rules.world_type = kind.into();
        let source = DistantTerrain::new(&opts).unwrap().unwrap();
        assert_ne!(other.cache_root, source.cache_root);
    }
    opts.world_rules.generator_options = "{\"fixedBiome\":2,\"seaLevel\":80}".into();
    let custom = DistantTerrain::new(&opts).unwrap().unwrap();
    assert_ne!(custom.cache_root, other.cache_root);
    for kind in ["flat", "debug_all_block_states"] {
        opts.world_rules.world_type = kind.into();
        assert!(DistantTerrain::new(&opts).unwrap().is_none());
    }
    opts.world_rules.world_type = "default".into();
    opts.world = Some(temporary.0.clone());
    assert!(DistantTerrain::new(&opts).unwrap().is_none());
    opts.world = None;
    opts.connect = Some("example.invalid".into());
    assert!(DistantTerrain::new(&opts).unwrap().is_none());
    opts.connect = None;
    opts.generator = 2;
    assert!(DistantTerrain::new(&opts).unwrap().is_none());
}
