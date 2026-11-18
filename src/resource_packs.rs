//! Project-local 1.8.9 directory and ZIP resource packs. Never extract pack paths.
use anyhow::{bail, ensure, Context, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

pub const DIRECTORY: &str = "resourcepacks";
pub fn icon_key(name: Option<&str>) -> String {
    format!(
        "pack-icons/{}",
        name.map_or_else(|| "default".into(), |s| format!("custom/{s}"))
    )
}
pub fn icon(pack: &Pack) -> Result<Vec<u8>> {
    let path = Path::new(DIRECTORY).join(&pack.name);
    let mut bytes = Vec::new();
    if path.is_dir() {
        File::open(path.join("pack.png"))?
            .take(4 * 1024 * 1024)
            .read_to_end(&mut bytes)?;
    } else {
        zip::ZipArchive::new(File::open(path)?)?
            .by_name("pack.png")?
            .take(4 * 1024 * 1024)
            .read_to_end(&mut bytes)?;
    }
    Ok(bytes)
}
const PREFIX: &str = "assets/minecraft/";
const ENTRY_LIMIT: u64 = 64 * 1024 * 1024;
const STACK_LIMIT: usize = 512 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Pack {
    pub name: String,
    pub description: String,
    pub format: i64,
    languages: BTreeMap<String, crate::resources::Language>,
}
fn text_component(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(items) => items.iter().map(text_component).collect(),
        serde_json::Value::Object(_) => {
            let mut text = value["text"]
                .as_str()
                .or_else(|| value["translate"].as_str())
                .unwrap_or("")
                .to_owned();
            if let Some(extra) = value["extra"].as_array() {
                text.extend(extra.iter().map(text_component));
            }
            text
        }
        _ => String::new(),
    }
}
fn metadata(path: &Path) -> Result<Pack> {
    let mut data = Vec::new();
    if path.is_dir() {
        File::open(path.join("pack.mcmeta"))?
            .take(1024 * 1024)
            .read_to_end(&mut data)?;
    } else {
        ensure!(
            path.extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("zip")),
            "Expected a .zip pack"
        );
        zip::ZipArchive::new(File::open(path)?)?
            .by_name("pack.mcmeta")?
            .take(1024 * 1024)
            .read_to_end(&mut data)?;
    }
    let meta: serde_json::Value = serde_json::from_slice(&data)?;
    let pack = &meta["pack"];
    Ok(Pack {
        languages: match meta.get("language") {
            Some(value) => serde_json::from_value(value.clone())?,
            None => BTreeMap::new(),
        },
        name: path
            .file_name()
            .context("Missing pack name")?
            .to_string_lossy()
            .into_owned(),
        description: text_component(&pack["description"]),
        format: pack["pack_format"]
            .as_i64()
            .context("Missing pack_format")?,
    })
}
pub fn discover() -> Result<Vec<Pack>> {
    fs::create_dir_all(DIRECTORY)?;
    let mut result = Vec::new();
    for entry in fs::read_dir(DIRECTORY)? {
        let e = entry?;
        if !e.file_type()?.is_symlink() {
            if let Ok(pack) = metadata(&e.path()) {
                result.push(pack);
            }
        }
    }
    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}
/// ModelBlockDefinition collects variants from every resource layer, low to high.
pub(crate) fn merge_blockstates(lower:&[u8],higher:&[u8])->Vec<u8>{
    use serde_json::Value;
    let (Ok(mut low),Ok(high))=(serde_json::from_slice::<Value>(lower),serde_json::from_slice::<Value>(higher))
        else {return higher.to_vec()};
    let (Some(a),Some(b))=(low["variants"].as_object_mut(),high["variants"].as_object())
        else {return higher.to_vec()};
    a.extend(b.clone());
    serde_json::to_vec(&low).unwrap()
}
#[derive(Default)]
pub struct Stack {
    files: BTreeMap<String, Vec<u8>>,
    sound_manifests: Vec<Vec<u8>>,
    pub languages: BTreeMap<String, crate::resources::Language>,
    texture_layers: BTreeMap<String, usize>,
    layer: usize,
    size: usize,
}
impl Stack {
    pub fn load(names: &[String]) -> Result<Arc<Self>> {
        Self::load_from(Path::new(DIRECTORY), names).map(Arc::new)
    }
    pub fn with_server(names:&[String],server:&Path)->Result<Arc<Self>>{
        let mut out=Self::load_from(Path::new(DIRECTORY),names)?;
        let name=server.file_name().and_then(|s|s.to_str()).context("Invalid cached pack name")?.to_owned();
        out.add_from(server.parent().context("Missing cache directory")?,&[name])?;
        Ok(Arc::new(out))
    }
    fn load_from(dir: &Path, names: &[String]) -> Result<Self> {
        let mut out = Self::default();
        out.add_from(dir,names)?;
        Ok(out)
    }
    fn add_from(&mut self,dir:&Path,names:&[String])->Result<()>{
        let out=self;
        let mut seen = BTreeSet::new();
        for name in names.iter().rev() {
            out.layer += 1;
            ensure!(
                Path::new(name).file_name().and_then(|s| s.to_str()) == Some(name.as_str())
                    && !name.contains(['/', '\\', ':']),
                "Invalid pack name"
            );
            ensure!(seen.insert(name), "Duplicate resource pack: {name}");
            let path = dir.join(name);
            ensure!(
                !fs::symlink_metadata(&path)?.file_type().is_symlink(),
                "Symbolic link resource pack"
            );
            let pack = metadata(&path).with_context(|| format!("Read {name}/pack.mcmeta"))?;
            for (code, language) in pack.languages {
                out.languages.entry(code).or_insert(language);
            }
            if path.is_dir() {
                let assets = path.join("assets/minecraft");
                if assets.is_dir() {
                    ensure!(
                        assets.canonicalize()?.starts_with(path.canonicalize()?),
                        "Resource directory outside pack"
                    );
                    out.read_directory(&assets, &assets)?;
                }
            } else {
                let mut zip = zip::ZipArchive::new(File::open(&path)?)?;
                for index in 0..zip.len() {
                    let file = zip.by_index(index)?;
                    if file.is_dir() {
                        continue;
                    }
                    let Some(relative) = file.name().strip_prefix(PREFIX).map(str::to_owned) else {
                        continue;
                    };
                    out.put(&relative, file.size(), file)?;
                }
            }
        }
        Ok(())
    }
    fn read_directory(&mut self, base: &Path, directory: &Path) -> Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() && !kind.is_symlink() {
                self.read_directory(base, &entry.path())?;
            } else if kind.is_file() {
                let path = entry.path();
                let name = path
                    .strip_prefix(base)?
                    .to_string_lossy()
                    .replace('\\', "/");
                self.put(&name, entry.metadata()?.len(), File::open(path)?)?;
            }
        }
        Ok(())
    }
    fn put(&mut self, name: &str, len: u64, reader: impl Read) -> Result<()> {
        if !valid_resource_name(name) {
            bail!("Invalid resource name {name}");
        }
        ensure!(
            len <= ENTRY_LIMIT && self.size.saturating_add(len as usize) <= STACK_LIMIT,
            "Resource pack exceeds memory limit"
        );
        let mut bytes = Vec::new();
        reader.take(len + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() as u64 == len, "Invalid resource length");
        if name == "sounds.json" {
            self.size += bytes.len();
            self.sound_manifests.push(bytes);
        } else if name.starts_with("lang/") && name.ends_with(".lang") {
            // Locale loads all matching language files from low to high priority.
            let entry = self.files.entry(name.into()).or_default();
            entry.push(b'\n');
            entry.extend_from_slice(&bytes);
            self.size += bytes.len() + 1;
        } else {
            if name.starts_with("blockstates/") && name.ends_with(".json") {
                if let Some(lower)=self.files.get(name) { bytes=merge_blockstates(lower,&bytes); }
            }
            if name.ends_with(".png") || name.ends_with(".png.mcmeta") {
                self.texture_layers.insert(name.to_owned(), self.layer);
            }
            self.size += bytes.len();
            if let Some(old) = self.files.insert(name.to_string(), bytes) {
                self.size -= old.len();
            }
        }
        Ok(())
    }
    pub fn get(&self, relative: &str) -> Option<&[u8]> {
        if relative == "sounds.json" {
            return self.sound_manifests.last().map(Vec::as_slice);
        }
        self.files.get(relative).map(Vec::as_slice)
    }
    pub fn sound_manifests(&self) -> impl Iterator<Item = &[u8]> {
        self.sound_manifests.iter().map(Vec::as_slice)
    }
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }
    // FallbackResourceManager reads metadata only from the selected texture's
    // own layer or a higher layer. Layer zero is the base game.
    pub fn metadata_visible(&self, texture: &str, metadata: &str) -> bool {
        self.texture_layers.get(metadata).unwrap_or(&0)
            >= self.texture_layers.get(texture).unwrap_or(&0)
    }
    pub fn bytes(&self, root: &Path, relative: &str) -> Result<Vec<u8>> {
        ensure!(
            valid_resource_name(relative),
            "Invalid resource name {relative}"
        );
        if let Some(bytes) = self.get(relative) {
            return Ok(bytes.to_vec());
        }
        let path: PathBuf = root.join(relative);
        fs::read(&path).with_context(|| format!("Read {}", path.display()))
    }
}
fn valid_resource_name(name: &str) -> bool {
    !name.contains(['\\', ':']) && !name.split('/').any(|part| matches!(part, "" | "." | ".."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    #[ignore = "uses the installed local 1.8.9 jar"]
    fn partial_blockstate_packs_override_single_variants_and_keep_parent_layers() {
        let mut stack=Stack::default();
        for (name,blob) in [
            ("blockstates/lever.json",r#"{"variants":{"facing=up_z,powered=false":{"model":"stone"}}}"#),
            ("blockstates/lever.json",r#"{"variants":{"facing=up_z,powered=true":{"model":"gold_block"}}}"#),
            ("blockstates/stone_monster_egg.json",r#"{"variants":{"normal":{"model":"lever_off"}}}"#),
        ] {
            stack.put(name,blob.len() as u64,blob.as_bytes()).unwrap();
        }
        let a=crate::assets::Assets::load_with_packs(&crate::minecraft_dir().join("versions/1.8.9/1.8.9.jar"),&stack).unwrap();
        let model = |meta: u16| a.block_models.get((69u16 << 4) | meta).unwrap();
        assert!(model(5).iter().all(|q|q.layer==a.materials["stone"]));
        assert!(model(13).iter().all(|q|q.layer==a.materials["gold_block"]));
        assert_eq!(model(1).len(),12);
        assert!(a.block_models.custom_shape(97<<4));
        assert_eq!(a.block_models.carried(97<<4).unwrap().len(),12);
        assert_eq!(a.texture((97<<4)|1,0,true),a.materials["cobblestone"]);
    }
    #[test]
    fn directory_and_zip_resolve_with_priority_and_merge_locales() {
        let root = std::env::temp_dir().join(format!(
            "ferrum-pack-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let folder = root.join("Low");
        let data = folder.join("assets/minecraft");
        fs::create_dir_all(data.join("lang")).unwrap();
        fs::create_dir_all(data.join("sounds")).unwrap();
        let metadata = br#"{"pack":{"pack_format":1,"description":{"text":"test"}}}"#;
        fs::write(folder.join("pack.mcmeta"), metadata).unwrap();
        fs::write(data.join("lang/en_US.lang"), "test.low=low\ntest.both=low").unwrap();
        fs::write(data.join("sounds/test.ogg"), b"low").unwrap();
        fs::write(data.join("sounds.json"), b"low events").unwrap();
        fs::write(data.join("test.png.mcmeta"), b"low metadata").unwrap();
        let mut zip = zip::ZipWriter::new(File::create(root.join("High.zip")).unwrap());
        let opt = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in [
            ("pack.mcmeta", metadata.as_slice()),
            (
                "assets/minecraft/lang/en_US.lang",
                b"test.both=high".as_slice(),
            ),
            ("assets/minecraft/sounds/test.ogg", b"high".as_slice()),
            ("assets/minecraft/sounds.json", b"high events".as_slice()),
            ("assets/minecraft/test.png", b"high texture".as_slice()),
        ] {
            zip.start_file(name, opt).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        let stack = Stack::load_from(&root, &["High.zip".into(), "Low".into()]).unwrap();
        assert_eq!(stack.get("sounds/test.ogg"), Some(b"high".as_slice()));
        assert_eq!(stack.sound_manifests().collect::<Vec<_>>(), [b"low events".as_slice(), b"high events"]);
        assert_eq!(stack.get("sounds.json"), Some(b"high events".as_slice()));
        assert!(!stack.metadata_visible("test.png", "test.png.mcmeta"));
        assert!(stack.bytes(&root, "../test.png").is_err());
        let lang = std::str::from_utf8(stack.get("lang/en_US.lang").unwrap()).unwrap();
        assert!(lang.find("test.low=low").unwrap() < lang.find("test.both=high").unwrap());
        let other = Stack::load_from(&root, &["Low".into(), "High.zip".into()]).unwrap();
        assert_eq!(other.get("sounds/test.ogg"), Some(b"low".as_slice()));
        assert!(other.metadata_visible("test.png", "test.png.mcmeta"));
        assert!(Stack::load_from(&root, &["../Low".into()]).is_err());
        assert_eq!(fs::read(data.join("sounds/test.ogg")).unwrap(), b"low");
        fs::remove_dir_all(root).unwrap();
    }
}
