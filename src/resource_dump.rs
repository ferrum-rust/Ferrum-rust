//! Locates `%APPDATA%\.minecraft\versions\1.8.9\1.8.9.jar` and dumps owned 1.8.9
//! resources (JAR copy, textures/models/lang, indexed sounds/locales, registry, and
//! manifest) into `.local/resources` natively without external Python/Java scripts.
#![allow(dead_code)]

use anyhow::{bail, ensure, Context, Result};
use flate2::read::GzDecoder;
use serde::Deserialize;
use sha1::{Digest, Sha1};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::Duration,
};

pub const MISSING_JAR_NOTICE: &str =
    "1.8.9.jar이 없습니다. 공식런쳐에서 1.8.9 버전을 한번 실행해주세요.";
pub const MISSING_JAR_POPUP: &str =
    "1.8.9.jar이 없습니다.\n공식런쳐에서 1.8.9 버전을 한번 실행해주세요.";

const DEFAULT_18_INDEX_URL: &str =
    "https://launchermeta.mojang.com/mc/assets/1.8/f6ad102bcaa53b1a58358f16e376d548d44933ec/1.8.json";

static REGISTRY_189_GZ: &[u8] = include_bytes!("vanilla/registry189.json.gz");

#[derive(Deserialize)]
struct VersionMeta {
    #[serde(rename = "assetIndex")]
    asset_index: Option<AssetIndexRef>,
}

#[derive(Deserialize)]
struct AssetIndexRef {
    id: String,
    url: Option<String>,
    sha1: Option<String>,
}

#[derive(Deserialize)]
struct AssetIndexFile {
    objects: BTreeMap<String, AssetObject>,
}

#[derive(Deserialize)]
struct AssetObject {
    hash: String,
}

pub fn appdata_minecraft_dir() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    if appdata.is_empty() {
        return None;
    }
    Some(PathBuf::from(appdata).join(".minecraft"))
}

pub fn is_valid_189_jar(path: &Path) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return false;
    };
    let has_splashes = archive.by_name("assets/minecraft/texts/splashes.txt").is_ok();
    let has_lang = archive.by_name("assets/minecraft/lang/en_US.lang").is_ok();
    has_splashes && has_lang
}

pub fn find_189_jar_in(mc_dir: &Path) -> Option<PathBuf> {
    let direct = mc_dir.join("versions").join("1.8.9").join("1.8.9.jar");
    if direct.is_file() && is_valid_189_jar(&direct) {
        return Some(direct);
    }
    let versions_dir = mc_dir.join("versions");
    let Ok(entries) = fs::read_dir(&versions_dir) else {
        return None;
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|name| {
                        name == "1.8.9"
                            || name.starts_with("1.8.9-")
                            || name.starts_with("1.8.9_")
                            || name.starts_with("1.8.9 ")
                    })
        })
        .collect();
    dirs.sort();
    for dir in dirs {
        if let Some(stem) = dir.file_name().and_then(|n| n.to_str()) {
            let named = dir.join(format!("{stem}.jar"));
            if named.is_file() && is_valid_189_jar(&named) {
                return Some(named);
            }
        }
        if let Ok(files) = fs::read_dir(&dir) {
            for f in files.flatten() {
                let p = f.path();
                if p.extension().and_then(|e| e.to_str()) == Some("jar") && is_valid_189_jar(&p) {
                    return Some(p);
                }
            }
        }
    }
    None
}

pub fn find_appdata_189_jar() -> Option<(PathBuf, PathBuf)> {
    let mc_dir = appdata_minecraft_dir()?;
    let jar = find_189_jar_in(&mc_dir)?;
    Some((mc_dir, jar))
}

fn sha1_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(40);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

fn sha1_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("파일 열기 실패: {}", path.display()))?;
    let mut hasher = Sha1::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    let mut out = String::with_capacity(40);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    Ok(out)
}

pub fn is_dump_ready(jar_src: &Path, dest_dir: &Path) -> bool {
    let dumped_jar = dest_dir.join("1.8.9.jar");
    let (Ok(src_meta), Ok(dst_meta)) = (jar_src.metadata(), dumped_jar.metadata()) else {
        return false;
    };
    if !dst_meta.is_file() || src_meta.len() != dst_meta.len() {
        return false;
    }
    if !dest_dir.join("registry.json").is_file()
        || !dest_dir.join("assets").join("pack.mcmeta").is_file()
        || !dest_dir
            .join("assets")
            .join("minecraft")
            .join("sounds.json")
            .is_file()
    {
        return false;
    }
    let Ok(manifest_bytes) = fs::read(dest_dir.join("manifest.json")) else {
        return false;
    };
    let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(&manifest_bytes) else {
        return false;
    };
    if manifest["format"].as_u64() != Some(3) {
        return false;
    }
    let Some(files) = manifest["files"].as_array() else {
        return false;
    };
    if files.is_empty() {
        return false;
    }
    if let Some(saved_sha1) = manifest["jar_sha1"].as_str() {
        let Ok(cur_sha1) = sha1_file(jar_src) else {
            return false;
        };
        return saved_sha1.eq_ignore_ascii_case(&cur_sha1);
    }
    true
}

fn sanitize_rel_path(base: &Path, relative: &str) -> Result<PathBuf> {
    ensure!(
        !relative.is_empty() && !relative.contains('\\'),
        "유효하지 않은 리소스 상대 경로: {relative}"
    );
    let rel_path = Path::new(relative);
    let mut out = base.to_path_buf();
    let mut pushed = false;
    for comp in rel_path.components() {
        match comp {
            Component::Normal(part) => {
                out.push(part);
                pushed = true;
            }
            _ => bail!("허용되지 않는 리소스 경로 구성요소: {relative}"),
        }
    }
    ensure!(pushed, "빈 리소스 경로입니다");
    Ok(out)
}

fn ensure_parent(path: &Path, created_dirs: &mut HashSet<PathBuf>) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !created_dirs.contains(parent) {
            fs::create_dir_all(parent)?;
            created_dirs.insert(parent.to_path_buf());
        }
    }
    Ok(())
}

pub fn decode_embedded_registry() -> Result<Vec<u8>> {
    let mut decoder = GzDecoder::new(REGISTRY_189_GZ);
    let mut out = Vec::with_capacity(64 * 1024);
    decoder
        .read_to_end(&mut out)
        .context("내장 1.8.9 레지스트리 압축 해제 실패")?;
    Ok(out)
}

fn http_get_bytes(url: &str) -> Result<Vec<u8>> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .new_agent();
    let mut resp = agent
        .get(url)
        .call()
        .with_context(|| format!("원격 에셋 요청 실패: {url}"))?;
    let mut bytes = Vec::new();
    resp.body_mut()
        .as_reader()
        .read_to_end(&mut bytes)
        .with_context(|| format!("원격 에셋 읽기 실패: {url}"))?;
    Ok(bytes)
}

fn load_asset_index(mc_dir: &Path, jar_src: &Path) -> Result<BTreeMap<String, AssetObject>> {
    let mut index_id = String::from("1.8");
    let mut index_url = String::from(DEFAULT_18_INDEX_URL);
    let mut expected_sha1: Option<String> = None;

    let version_json_path = jar_src.with_extension("json");
    if let Ok(raw) = fs::read(&version_json_path) {
        if let Ok(meta) = serde_json::from_slice::<VersionMeta>(&raw) {
            if let Some(idx) = meta.asset_index {
                if !idx.id.is_empty() {
                    index_id = idx.id;
                }
                if let Some(u) = idx.url.filter(|s| !s.is_empty()) {
                    index_url = u;
                }
                expected_sha1 = idx.sha1.filter(|s| s.len() == 40);
            }
        }
    }

    let indexes_dir = mc_dir.join("assets").join("indexes");
    let primary_index = indexes_dir.join(format!("{index_id}.json"));
    let fallback_index = indexes_dir.join("1.8.json");

    let index_bytes = if primary_index.is_file() {
        fs::read(&primary_index)?
    } else if fallback_index.is_file() {
        fs::read(&fallback_index)?
    } else {
        let fetched = http_get_bytes(&index_url)?;
        if let Some(exp) = &expected_sha1 {
            ensure!(
                sha1_bytes(&fetched).eq_ignore_ascii_case(exp),
                "에셋 인덱스 SHA-1 검증 실패"
            );
        }
        fetched
    };

    let parsed: AssetIndexFile =
        serde_json::from_slice(&index_bytes).context("1.8 에셋 인덱스 JSON 파싱 실패")?;
    Ok(parsed.objects)
}

pub fn ensure_dumped_resources(
    mc_dir: &Path,
    jar_src: &Path,
    dest_dir: &Path,
    mut on_progress: impl FnMut(usize, usize, &str),
) -> Result<PathBuf> {
    let dumped_jar = dest_dir.join("1.8.9.jar");
    if is_dump_ready(jar_src, dest_dir) {
        return Ok(dumped_jar);
    }

    fs::create_dir_all(dest_dir)
        .with_context(|| format!("리소스 폴더 생성 실패: {}", dest_dir.display()))?;

    on_progress(0, 100, "1.8.9.jar 무결성 확인 및 복사 중...");
    let jar_sha1 = sha1_file(jar_src)?;
    let tmp_jar = dest_dir.join("1.8.9.jar.tmp");
    fs::copy(jar_src, &tmp_jar)
        .with_context(|| format!("1.8.9.jar 복사 실패: {}", jar_src.display()))?;
    fs::rename(&tmp_jar, &dumped_jar)
        .with_context(|| format!("1.8.9.jar 배치 실패: {}", dumped_jar.display()))?;

    let objects = load_asset_index(mc_dir, jar_src)?;
    let jar_file = File::open(&dumped_jar)?;
    let mut archive = zip::ZipArchive::new(jar_file).context("1.8.9.jar 압축 열기 실패")?;

    let mut jar_indices = Vec::new();
    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index_raw(i) else {
            continue;
        };
        let name = entry.name();
        if !name.ends_with('/') && (name.starts_with("assets/") || name == "pack.png") {
            jar_indices.push(i);
        }
    }

    let total_steps = (jar_indices.len() + objects.len() + 2).max(1);
    let mut done_steps = 0usize;
    let mut created_dirs = HashSet::new();
    created_dirs.insert(dest_dir.to_path_buf());
    let mut all_files = BTreeSet::new();
    let mut buf = Vec::with_capacity(64 * 1024);

    on_progress(0, total_steps, "1.8.9.jar 내부 텍스처·모델 덤프 중...");
    let mut jar_count = 0usize;
    for (k, idx) in jar_indices.into_iter().enumerate() {
        let mut entry = archive.by_index(idx)?;
        let name = entry.name().to_owned();
        let target = sanitize_rel_path(dest_dir, &name)?;
        ensure_parent(&target, &mut created_dirs)?;
        buf.clear();
        entry.read_to_end(&mut buf)?;
        let need_write = target
            .metadata()
            .map(|m| !m.is_file() || m.len() != buf.len() as u64)
            .unwrap_or(true);
        if need_write {
            fs::write(&target, &buf)?;
        }
        all_files.insert(name);
        jar_count += 1;
        done_steps += 1;
        if k % 64 == 0 {
            on_progress(done_steps, total_steps, "1.8.9.jar 내부 텍스처·모델 덤프 중...");
        }
    }

    let objects_root = mc_dir.join("assets").join("objects");
    let mut external_count = 0usize;
    for (k, (name, record)) in objects.into_iter().enumerate() {
        let hash = record.hash.trim().to_ascii_lowercase();
        ensure!(
            hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
            "잘못된 에셋 해시: {name}"
        );
        let relative = if name.starts_with("assets/") {
            name
        } else {
            format!("assets/{name}")
        };
        let target = sanitize_rel_path(dest_dir, &relative)?;
        ensure_parent(&target, &mut created_dirs)?;

        let already_valid = target.is_file()
            && sha1_file(&target)
                .map(|actual| actual.eq_ignore_ascii_case(&hash))
                .unwrap_or(false);
        if !already_valid {
            let local_obj = objects_root.join(&hash[0..2]).join(&hash);
            let data = if local_obj.is_file() {
                fs::read(&local_obj)?
            } else {
                let url = format!(
                    "https://resources.download.minecraft.net/{}/{hash}",
                    &hash[0..2]
                );
                http_get_bytes(&url)?
            };
            ensure!(
                sha1_bytes(&data).eq_ignore_ascii_case(&hash),
                "에셋 체크섬 불일치: {relative}"
            );
            fs::write(&target, &data)?;
        }

        all_files.insert(relative);
        external_count += 1;
        done_steps += 1;
        if k % 24 == 0 {
            on_progress(done_steps, total_steps, "사운드 및 다국어 에셋 덤프 중...");
        }
    }

    on_progress(done_steps, total_steps, "레지스트리 및 매니페스트 기록 중...");
    let reg_bytes = decode_embedded_registry()?;
    let reg_val: serde_json::Value = serde_json::from_slice(&reg_bytes)?;
    let block_count = reg_val["blocks"].as_object().map_or(0, |m| m.len());
    let item_count = reg_val["items"].as_object().map_or(0, |m| m.len());
    fs::write(dest_dir.join("registry.json"), &reg_bytes)?;
    done_steps += 1;

    let languages = all_files.iter().filter(|n| n.contains("/lang/")).count();
    let sounds = all_files.iter().filter(|n| n.ends_with(".ogg")).count();
    let files_vec: Vec<String> = all_files.into_iter().collect();
    let manifest = serde_json::json!({
        "format": 3,
        "jar_sha1": jar_sha1,
        "jar_files": jar_count,
        "external_files": external_count,
        "languages": languages,
        "sounds": sounds,
        "block_registry": block_count,
        "item_registry": item_count,
        "files": files_vec,
    });
    let manifest_tmp = dest_dir.join("manifest.json.tmp");
    {
        let mut f = File::create(&manifest_tmp)?;
        f.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
        f.flush()?;
    }
    fs::rename(&manifest_tmp, dest_dir.join("manifest.json"))?;

    on_progress(total_steps, total_steps, "1.8.9 리소스 덤프 완료!");
    Ok(dumped_jar)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_minecraft_dir_returns_none_and_embedded_registry_is_valid() {
        let empty_dir = std::env::temp_dir().join(format!(
            "ferrum-empty-mc-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(find_189_jar_in(&empty_dir).is_none());

        let reg = decode_embedded_registry().expect("embedded registry189.json.gz must decode");
        let parsed: serde_json::Value = serde_json::from_slice(&reg).unwrap();
        assert_eq!(parsed["blocks"].as_object().unwrap().len(), 197);
        assert_eq!(parsed["items"].as_object().unwrap().len(), 187);
    }

    #[test]
    fn synthetic_189_jar_and_assets_dump_cleanly_and_pass_ready_check() {
        let base = std::env::temp_dir().join(format!(
            "ferrum-dump-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let ver_dir = base.join("mc").join("versions").join("1.8.9");
        let idx_dir = base.join("mc").join("assets").join("indexes");
        let obj_dir = base.join("mc").join("assets").join("objects");
        fs::create_dir_all(&ver_dir).unwrap();
        fs::create_dir_all(&idx_dir).unwrap();

        // Build minimal synthetic 1.8.9.jar
        let jar_path = ver_dir.join("1.8.9.jar");
        {
            let f = File::create(&jar_path).unwrap();
            let mut zip = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("assets/minecraft/texts/splashes.txt", opts)
                .unwrap();
            zip.write_all(b"Hello Ferrum!\n").unwrap();
            zip.start_file("assets/minecraft/lang/en_US.lang", opts)
                .unwrap();
            zip.write_all(b"menu.singleplayer=Singleplayer\n").unwrap();
            zip.start_file("pack.png", opts).unwrap();
            zip.write_all(b"png").unwrap();
            zip.finish().unwrap();
        }

        // Build minimal synthetic indexed objects (pack.mcmeta + sounds.json)
        let mcmeta = br#"{"pack":{"pack_format":1,"description":"test"},"language":{"en_US":{"name":"English","region":"US","bidirectional":false}}}"#;
        let sounds = b"{}";
        let h_mcmeta = sha1_bytes(mcmeta);
        let h_sounds = sha1_bytes(sounds);
        fs::create_dir_all(obj_dir.join(&h_mcmeta[0..2])).unwrap();
        fs::write(obj_dir.join(&h_mcmeta[0..2]).join(&h_mcmeta), mcmeta).unwrap();
        fs::create_dir_all(obj_dir.join(&h_sounds[0..2])).unwrap();
        fs::write(obj_dir.join(&h_sounds[0..2]).join(&h_sounds), sounds).unwrap();

        fs::write(
            ver_dir.join("1.8.9.json"),
            r#"{"assetIndex":{"id":"1.8"}}"#,
        )
        .unwrap();
        fs::write(
            idx_dir.join("1.8.json"),
            serde_json::to_vec(&serde_json::json!({
                "objects": {
                    "pack.mcmeta": { "hash": h_mcmeta },
                    "minecraft/sounds.json": { "hash": h_sounds }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let found = find_189_jar_in(&base.join("mc")).expect("must find 1.8.9.jar");
        assert_eq!(found, jar_path);

        let dest = base.join("out_resources");
        assert!(!is_dump_ready(&found, &dest));
        let mut progress_calls = 0usize;
        let dumped = ensure_dumped_resources(&base.join("mc"), &found, &dest, |_, _, _| {
            progress_calls += 1;
        })
        .expect("dump must succeed");
        assert!(progress_calls > 0);
        assert_eq!(dumped, dest.join("1.8.9.jar"));
        assert!(is_dump_ready(&found, &dest));
        assert!(dest.join("registry.json").is_file());
        assert!(dest.join("manifest.json").is_file());
        assert!(dest.join("assets/pack.mcmeta").is_file());
        assert!(dest.join("assets/minecraft/sounds.json").is_file());

        let _ = fs::remove_dir_all(&base);
    }
}
