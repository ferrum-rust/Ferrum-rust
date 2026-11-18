use crate::{gameplay::Mode, nbt::Tag, ui::WorldEntry, world, Options};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Rules {
    pub world_type: String,
    pub generator_options: String,
    pub structures: bool,
    pub bonus_chest: bool,
    pub allow_cheats: bool,
    pub hardcore: bool,
}
impl Default for Rules {
    fn default() -> Self {
        Self {
            world_type: "default".into(),
            generator_options: String::new(),
            structures: true,
            bonus_chest: false,
            allow_cheats: false,
            hardcore: false,
        }
    }
}

pub fn folder_name(name: &str, entries: &[WorldEntry]) -> String {
    let mut folder: String = name
        .trim()
        .chars()
        .map(|c| {
            if c < ' ' || c == char::from(127) || c == char::from(96) || "/\\:*?\"<>|.".contains(c)
            {
                '_'
            } else {
                c
            }
        })
        .collect();
    if folder.is_empty() {
        folder.push_str("World");
    }
    let upper = folder.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "CON" | "COM" | "PRN" | "AUX" | "CLOCK$" | "NUL"
    ) || (upper.len() == 4
        && (upper.starts_with("COM") || upper.starts_with("LPT"))
        && matches!(upper.as_bytes()[3], b'1'..=b'9'))
    {
        folder = format!("_{folder}_");
    }
    while entries
        .iter()
        .any(|e| e.folder.to_lowercase() == folder.to_lowercase())
    {
        folder.push('-');
    }
    folder
}

#[derive(Clone)]
pub struct Library {
    pub project: PathBuf,
    originals: PathBuf,
}
impl Default for Library {
    fn default() -> Self {
        Self {
            project: ".".into(),
            originals: crate::minecraft_dir().join("saves"),
        }
    }
}
#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct ImportedEdit {
    name: Option<String>,
    deleted: bool,
    default_mode: Option<u8>,
}

fn millis(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
fn modified(path: &Path) -> i64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .map(millis)
        .unwrap_or(0)
}
fn unique_id() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQUENCE: AtomicU32 = AtomicU32::new(0);
    format!(
        "{}-{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}
fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn nbt_long(data: &Tag, key: &str) -> Option<i64> {
    match data.get(key)? {
        Tag::Long(n) => Some(*n),
        tag => tag.int().map(i64::from),
    }
}
fn try_read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    Ok(serde_json::from_reader(std::io::BufReader::new(
        fs::File::open(path)?,
    ))?)
}

impl Library {
    #[cfg(test)]
    pub fn isolated(project: PathBuf) -> Self {
        Self {
            originals: project.join("java/saves"),
            project,
        }
    }
    fn local(&self) -> PathBuf {
        self.project.join("worlds")
    }
    fn saves(&self) -> PathBuf {
        self.project.join("saves")
    }
    fn native_save(&self, id: &str) -> PathBuf {
        self.saves().join("native-v4").join(id)
    }
    fn import_edit(&self, folder: &str) -> PathBuf {
        self.local()
            .join(".imported")
            .join(format!("{folder}.json"))
    }
    fn import(&self, path: PathBuf, folder: String) -> Result<Option<WorldEntry>> {
        let edit: ImportedEdit = match fs::File::open(self.import_edit(&folder)) {
            Ok(f) => serde_json::from_reader(f)?,
            Err(e) if e.kind() == ErrorKind::NotFound => Default::default(),
            Err(e) => return Err(e.into()),
        };
        if edit.deleted {
            return Ok(None);
        }
        let root = world::level_data(&path)?;
        let data = root.get("Data").context("Missing level data")?;
        if data
            .get("DataVersion")
            .and_then(Tag::int)
            .is_some_and(|v| v > 99)
        {
            return Ok(None);
        }
        let string = |key| data.get(key).and_then(Tag::string);
        let int = |key| data.get(key).and_then(Tag::int);
        let game_type_id = edit.default_mode.unwrap_or_else(||int("GameType").unwrap_or(0).clamp(0, 3) as u8);
        let overlay = self.saves().join(format!(
            "{}.json",
            world::overlay_key(&fs::canonicalize(&path)?.to_string_lossy())
        ));
        let requires_conversion = int("version") == Some(19132);
        Ok(Some(WorldEntry {
            last_played: nbt_long(data, "LastPlayed")
                .unwrap_or_else(|| modified(&path.join("level.dat")))
                .max(modified(&overlay)),
            name: edit
                .name
                .unwrap_or_else(|| string("LevelName").unwrap_or("").to_owned()),
            folder,
            seed: nbt_long(data, "RandomSeed").unwrap_or(0) as u64,
            mode: if game_type_id == 1 {
                Mode::Creative
            } else {
                Mode::Survival
            },
            game_type_id,
            save_id: None,
            path: Some(path),
            descriptor: None,
            generator: 3,
            village: false,
            requires_conversion,
            rules: Rules {
                world_type: string("generatorName").unwrap_or("default").to_owned(),
                generator_options: string("generatorOptions").unwrap_or("").to_owned(),
                structures: int("MapFeatures") != Some(0),
                bonus_chest: false,
                allow_cheats: int("allowCommands").map_or(game_type_id == 1, |v| v != 0),
                hardcore: int("hardcore").is_some_and(|v| v != 0),
            },
        }))
    }
    pub fn list(&self) -> Vec<WorldEntry> {
        let mut out = Vec::new();
        if let Ok(iter) = fs::read_dir(self.local()) {
            for item in iter.flatten() {
                let path = item.path();
                let kind = match item.file_type() {
                    Ok(k) => k,
                    Err(_) => continue,
                };
                let descriptor =
                    if kind.is_dir() && !item.file_name().to_string_lossy().starts_with('.') {
                        path.join("world.json")
                    } else if kind.is_file() && path.extension().is_some_and(|e| e == "json") {
                        path.clone()
                    } else {
                        continue;
                    };
                if !descriptor.is_file() {
                    continue;
                }
                let Ok(mut e) = try_read_json::<WorldEntry>(&descriptor) else {
                    continue;
                };
                let Some(id) = e.save_id.as_deref().filter(|s| safe_id(s)) else {
                    continue;
                };
                if e.path.is_some() {
                    continue;
                }
                if e.folder.is_empty() {
                    e.folder = id.into();
                }
                if e.mode == Mode::Creative {
                    e.game_type_id = 1;
                }
                let last_save = modified(&self.saves().join(format!("world-{id}.json")))
                    .max(modified(&self.native_save(id).join("world.json.gz")));
                e.last_played = e.last_played.max(last_save);
                if e.last_played == 0 {
                    e.last_played = modified(&descriptor);
                }
                e.descriptor = Some(descriptor);
                out.push(e);
            }
        }
        if let Ok(iter) = fs::read_dir(&self.originals) {
            for item in iter.flatten() {
                if let Ok(Some(entry)) =
                    self.import(item.path(), item.file_name().to_string_lossy().into_owned())
                {
                    out.push(entry);
                }
            }
        }
        out.sort_by(|a, b| {
            b.last_played
                .cmp(&a.last_played)
                .then_with(|| a.folder.encode_utf16().cmp(b.folder.encode_utf16()))
        });
        out
    }
    fn lock(&self) -> Result<fs::File> {
        fs::create_dir_all(self.local())?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.local().join(".library.lock"))?;
        lock.lock()?;
        Ok(lock)
    }
    pub fn create(&self, name: &str, seed: u64, mode: Mode, rules: Rules) -> Result<WorldEntry> {
        let _lock = self.lock()?;
        ensure!(!name.is_empty(), "Empty world name");
        let root = self.local();
        fs::create_dir_all(&root)?;
        let mut folder = folder_name(name, &self.list());
        let dir = loop {
            let path = root.join(&folder);
            match fs::create_dir(&path) {
                Ok(()) => break path,
                Err(e) if e.kind() == ErrorKind::AlreadyExists => folder.push('-'),
                Err(e) => return Err(e.into()),
            }
        };
        let descriptor = dir.join("world.json");
        let generator = if rules.world_type.eq_ignore_ascii_case("default") { 4 } else { 3 };
        let entry = WorldEntry {
            name: name.trim().to_owned(),
            seed,
            mode,
            game_type_id: u8::from(mode == Mode::Creative),
            rules,
            save_id: Some(unique_id()),
            folder,
            descriptor: Some(descriptor.clone()),
            path: None,
            generator,
            village: false,
            requires_conversion: false,
            last_played: millis(SystemTime::now()),
        };
        if let Err(e) = world::atomic_json(&descriptor, &entry) {
            let _ = fs::remove_dir(&dir);
            return Err(e);
        }
        Ok(entry)
    }
    fn current(&self, e: &WorldEntry) -> Result<WorldEntry> {
        self.list()
            .into_iter()
            .find(|v| v.path == e.path && v.save_id == e.save_id && v.descriptor == e.descriptor)
            .context("This world is no longer in the list")
    }
    pub(crate) fn default_mode(options:&crate::Options,id:u8)->Result<()> {
        if options.save_id.is_none() && options.world.is_none(){return Ok(())}
        let library=Self{project:options.project_dir.clone(),..Self::default()};
        let _lock=library.lock()?;
        let entry=library.list().into_iter().find(|e|match &options.world{
            Some(path)=>e.path.as_ref()==Some(path),None=>e.save_id==options.save_id,
        });
        if let Some(mut entry)=entry{
            if let Some(path)=entry.descriptor.clone(){
                entry.game_type_id=id;entry.mode=if id==1{Mode::Creative}else{Mode::Survival};
                world::atomic_json(&path,&entry)?;
            }else{
                let path=library.import_edit(&entry.folder);
                let mut edit:ImportedEdit=if path.exists(){try_read_json(&path)?}else{Default::default()};
                edit.default_mode=Some(id);world::atomic_json(&path,&edit)?;
            }
        }
        Ok(())
    }
    pub fn rename(&self, entry: &WorldEntry, name: &str) -> Result<()> {
        let _lock = self.lock()?;
        let mut e = self.current(entry)?;
        ensure!(!name.trim().is_empty(), "Empty world name");
        e.name = name.trim().into();
        if let Some(path) = &e.descriptor {
            world::atomic_json(path, &e)
        } else {
            let path=self.import_edit(&e.folder);
            let mut edit:ImportedEdit=if path.exists(){try_read_json(&path)?}else{Default::default()};
            edit.name=Some(e.name);world::atomic_json(&path,&edit)
        }
    }
    pub fn delete(&self, entry: &WorldEntry) -> Result<()> {
        let _lock = self.lock()?;
        let e = self.current(entry)?;
        let base = if let Some(id) = &e.save_id {
            format!("world-{id}")
        } else {
            world::overlay_key(
                &fs::canonicalize(e.path.as_ref().context("Missing original world")?)?
                    .to_string_lossy(),
            )
        };
        let trash = self.local().join(".trash").join(unique_id());
        fs::create_dir_all(&trash)?;
        let overlay = self.saves().join(format!("{base}.json"));
        let mut files = vec![overlay.with_extension("player.json"), overlay];
        let native = e.save_id.as_deref().map(|id| self.native_save(id))
            .filter(|path| path.exists());
        if let Some(path) = &native {
            // A whole populated checkpoint belongs to this save identity.
            // Validate both resolved targets before moving its directory.
            let project = self.project.canonicalize()?;
            ensure!(path.is_dir() && path.canonicalize()?.starts_with(&project)
                && trash.canonicalize()?.starts_with(&project),
                "Native save/trash directory is outside this project");
            files.push(path.clone());
        }
        if let Some(file) = &e.descriptor {
            files.push(file.clone());
        }
        let mut moved = Vec::new();
        let result = (|| -> Result<()> {
            for file in files {
                let dest = if e.descriptor.as_ref() == Some(&file) {
                    trash.join("descriptor.json")
                } else if native.as_ref() == Some(&file) {
                    trash.join("native-v4")
                } else {
                    trash.join(file.file_name().context("Invalid save file")?)
                };
                match fs::rename(&file, &dest) {
                    Ok(()) => moved.push((file, dest)),
                    Err(err) if err.kind() == ErrorKind::NotFound => {}
                    Err(err) => return Err(err.into()),
                }
            }
            if e.path.is_some() {
                world::atomic_json(
                    &self.import_edit(&e.folder),
                    &ImportedEdit {
                        name: None,
                        deleted: true,
                        ..Default::default()
                    },
                )?;
            }
            Ok(())
        })();
        if let Err(err) = result {
            for (file, dest) in moved.into_iter().rev() {
                fs::rename(dest, file)?;
            }
            return Err(err);
        }
        if let Some(path) = e
            .descriptor
            .filter(|p| p.file_name().is_some_and(|s| s == "world.json"))
        {
            if let Some(dir) = path
                .parent()
                .filter(|dir| dir.parent() == Some(self.local().as_path()))
            {
                let _ = fs::remove_dir(dir);
            }
        }
        Ok(())
    }
}
impl WorldEntry {
    pub fn apply_to(&self, options: &mut Options) {
        options.world.clone_from(&self.path);
        options.save_id.clone_from(&self.save_id);
        options.connect = None;
        options.seed = self.seed;
        options.generator = self.generator;
        options.village = self.village;
        options.creative = self.mode == Mode::Creative;
        options.fresh = false;
        options.world_rules.clone_from(&self.rules);
    }
}

// The bonus chest belongs to the new save. Use the spawn area's loaded surface,
// then the generator's four placement attempts and the original ten loot rolls.
pub fn place_bonus_chest(
    world: &mut world::World,
    sim: &mut crate::gameplay::Simulation,
    seed: u64,
) {
    use crate::vanilla::{
        loot::{fill_loot, LootEntry},
        material,
        random::JavaRandom,
        spawning::solid_top,
    };
    use glam::IVec3;
    let mut random = JavaRandom::new(seed as i64);
    let spawn = world.spawn.floor().as_ivec3();
    for _ in 0..10 {
        let x = spawn.x + random.next_int(6) - random.next_int(6);
        let z = spawn.z + random.next_int(6) - random.next_int(6);
        let Some(y) = (1..256).rev().find(|&y| {
            let state = world.get(IVec3::new(x, y, z));
            state != 0 && !material::is_leaves(state)
        }) else {
            continue;
        };
        for _ in 0..4 {
            let p = IVec3::new(
                x + random.next_int(4) - random.next_int(4),
                y + 1 + random.next_int(3) - random.next_int(3),
                z + random.next_int(4) - random.next_int(4),
            );
            if !(1..256).contains(&p.y) || world.get(p) != 0 || !solid_top(world.get(p - IVec3::Y))
            {
                continue;
            }
            if !world.set(p, 54 << 4 | 2) {
                continue;
            }
            let entries = [
                (280, 1, 3, 10),
                (5, 1, 3, 10),
                (17, 1, 3, 10),
                (275, 1, 1, 3),
                (271, 1, 1, 5),
                (274, 1, 1, 3),
                (270, 1, 1, 5),
                (260, 2, 3, 5),
                (297, 2, 3, 3),
                (162, 1, 3, 10),
            ]
            .map(|(id, min, max, weight)| LootEntry::new(id, 0, min, max, weight));
            let mut chest = crate::container::Container::new(crate::container::Kind::Chest);
            chest.slots = fill_loot(&mut random, &entries, 10, 27)
                .into_iter()
                .map(|slot| slot.map(|s| crate::inventory::Stack::new(s.id, s.count)))
                .collect();
            sim.containers
                .insert(crate::gameplay::container_key(p), chest);
            for d in [-IVec3::X, IVec3::X, -IVec3::Z, IVec3::Z] {
                let torch = p + d;
                if world.get(torch) == 0 && solid_top(world.get(torch - IVec3::Y)) {
                    world.set(torch, 50 << 4 | 5);
                }
            }
            return;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn library() -> Library {
        let project = std::env::temp_dir().join(format!("ferrum-world-menu-{}", unique_id()));
        Library {
            originals: project.join("java/saves"),
            project,
        }
    }
    #[test]
    fn rename_recreate_and_delete_own_and_imported_worlds_separately() {
        let l = library();
        let original = l.originals.join("Survival 189");
        fs::create_dir_all(&original).unwrap();
        let data = include_bytes!("../tests/fixtures/world-menu-survival.dat");
        fs::write(original.join("level.dat"), data).unwrap();
        let imported = l.list().pop().unwrap();
        assert_eq!(imported.seed, 189);
        assert_eq!(imported.game_type_id, 0);
        assert_eq!(imported.last_played, 1700000000000);
        assert_eq!(imported.name, "서바이벌 월드");
        l.rename(&imported, "Imports").unwrap();
        assert_eq!(l.list()[0].name, "Imports");
        let first = l
            .create(
                "Survival 189",
                u64::MAX,
                Mode::Creative,
                Rules {
                    world_type: "flat".into(),
                    allow_cheats: true,
                    ..Default::default()
                },
            )
            .unwrap();
        let second = l
            .create("SURVIVAL 189", 189, Mode::Survival, Default::default())
            .unwrap();
        assert_eq!(first.folder, "Survival 189-");
        assert_eq!(second.folder, "SURVIVAL 189--");
        assert_ne!(first.save_id, second.save_id);
        assert_eq!(folder_name("CON", &[]), "_CON_");
        assert_eq!(folder_name("  .A?/B  ", &[]), "_A__B");
        assert_eq!(folder_name(" ", &[]), "World");
        let saved = l
            .saves()
            .join(format!("world-{}.json", first.save_id.as_deref().unwrap()));
        fs::create_dir_all(l.saves()).unwrap();
        fs::write(&saved, b"test overlay").unwrap();
        fs::write(saved.with_extension("player.json"), b"older player").unwrap();
        l.rename(&first, "Display Only").unwrap();
        let renamed = l
            .list()
            .into_iter()
            .find(|e| e.name == "Display Only")
            .unwrap();
        assert_eq!(renamed.save_id, first.save_id);
        assert_eq!(renamed.seed, u64::MAX);
        assert!(renamed.rules.allow_cheats);
        l.delete(&renamed).unwrap();
        assert!(!saved.exists() && !saved.with_extension("player.json").exists());
        assert!(l.delete(&renamed).is_err());
        l.delete(&imported).unwrap();
        assert_eq!(l.list().len(), 1);
        assert_eq!(l.list()[0].save_id, second.save_id);
        assert_eq!(fs::read(original.join("level.dat")).unwrap(), data);
        fs::remove_dir_all(l.project).unwrap();
    }
    #[test]
    fn native_checkpoint_last_played_recreate_and_delete_follow_save_identity() {
        let l = library();
        let mut first = l.create("Populated", 189, Mode::Creative, Rules::default()).unwrap();
        assert_eq!(first.generator, 4);
        first.last_played = 1;
        world::atomic_json(first.descriptor.as_ref().unwrap(), &first).unwrap();
        let first_native = l.native_save(first.save_id.as_deref().unwrap());
        fs::create_dir_all(&first_native).unwrap();
        let checkpoint = first_native.join("world.json.gz");
        fs::write(&checkpoint, b"original populated checkpoint").unwrap();
        fs::write(first_native.join("session.lock"), b"native lock marker").unwrap();
        let listed = l.list().pop().unwrap();
        assert_eq!(listed.last_played, modified(&checkpoint));
        assert!(listed.last_played > 1);

        // Re-create copies creation settings into a new identity, never its
        // old population/player checkpoint. The source remains untouched.
        let recreated = l.create("Copy of Populated", first.seed, first.mode, first.rules.clone()).unwrap();
        assert_ne!(recreated.save_id, first.save_id);
        let recreated_native = l.native_save(recreated.save_id.as_deref().unwrap());
        assert!(!recreated_native.exists());
        assert_eq!(fs::read(&checkpoint).unwrap(), b"original populated checkpoint");
        fs::create_dir_all(&recreated_native).unwrap();
        fs::write(recreated_native.join("world.json.gz"), b"separate new world").unwrap();
        let old_overlay = l.saves().join(format!("world-{}.json", first.save_id.as_deref().unwrap()));
        fs::write(&old_overlay, b"retained pre-upgrade overlay").unwrap();
        l.rename(&first, "Renamed Populated").unwrap();
        assert_eq!(fs::read(&checkpoint).unwrap(), b"original populated checkpoint");
        l.delete(&first).unwrap();
        assert!(!first_native.exists());
        assert!(!old_overlay.exists());
        assert_eq!(fs::read(recreated_native.join("world.json.gz")).unwrap(), b"separate new world");
        let remaining = l.list();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].save_id, recreated.save_id);
        let trash = fs::read_dir(l.local().join(".trash")).unwrap().next().unwrap().unwrap().path();
        assert_eq!(fs::read(trash.join("native-v4/world.json.gz")).unwrap(), b"original populated checkpoint");
        assert_eq!(fs::read(trash.join("native-v4/session.lock")).unwrap(), b"native lock marker");
        assert_eq!(fs::read(trash.join(old_overlay.file_name().unwrap())).unwrap(), b"retained pre-upgrade overlay");

        let project = l.project.canonicalize().unwrap();
        let temp = std::env::temp_dir().canonicalize().unwrap();
        assert_eq!(project.parent(), Some(temp.as_path()));
        assert!(project.file_name().unwrap().to_string_lossy().starts_with("ferrum-world-menu-"));
        fs::remove_dir_all(project).unwrap();
    }
}
