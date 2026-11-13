//! Several instances («сборки») of the MineHouse pack. Each one has its own
//! `.minecraft`: mods, resource packs, shaders, options and worlds. Java,
//! Minecraft, libraries and assets are downloaded once and shared.
//!
//! The list lives in `<root>/instances.json`. The player can move the data
//! folder (shared files + `instances/`) to another disk, at first start or
//! later in the settings; the launcher config itself stays in `root`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::paths::Paths;

/// The instance every launcher had before there were several.
pub const DEFAULT_ID: &str = "minehouse";
const DEFAULT_NAME: &str = "MineHouse";
const MAX_NAME: usize = 40;
/// Folders in the data folder shared by all instances.
const SHARED: &[&str] = &["runtime", "versions", "libraries", "assets", "natives"];
/// Launcher indexes that lived in `root` before instances had their own.
const LEGACY_INDEXES: &[&str] = &["user-mods.json", "content-packs.json"];

/// What an instance runs.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Java Minecraft with Fabric and the MineHouse modpack.
    #[default]
    Fabric,
    /// Horizon, Minecraft rewritten in Rust, with MineHouse's native mods
    /// (see [`crate::horizon`]): lighter, but no Fabric mods or shaders.
    Horizon,
}

impl Kind {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "fabric" => Ok(Self::Fabric),
            "horizon" => Ok(Self::Horizon),
            _ => anyhow::bail!("Неизвестный тип сборки {s}"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Instance {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: Kind,
    /// Its `.minecraft`.
    pub dir: PathBuf,
    /// Unix seconds.
    #[serde(default)]
    pub created_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    #[serde(skip)]
    root: PathBuf,
    /// Where the game files go; `None` = `root`.
    pub data_dir: Option<PathBuf>,
    /// The player has seen the first-start question «where to install».
    pub setup_done: bool,
    pub active: String,
    pub instances: Vec<Instance>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            root: PathBuf::new(),
            data_dir: None,
            setup_done: false,
            active: DEFAULT_ID.into(),
            instances: Vec::new(),
        }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Config {
    fn file(root: &Path) -> PathBuf {
        root.join("instances.json")
    }

    /// Reads the list; without one, the single instance in `root/game` that
    /// earlier versions used. Never fails: a broken file means defaults.
    pub fn load(root: &Path) -> Self {
        let read: Option<Self> = std::fs::read(Self::file(root))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        let fresh = read.is_none();
        let mut c = read.unwrap_or_default();
        c.root = root.to_path_buf();
        if fresh {
            // Someone who already played keeps everything where it is.
            c.setup_done = ["game", "settings.json", "account.json"]
                .iter()
                .any(|f| root.join(f).exists());
        }
        if c.instances.is_empty() {
            c.instances.push(Instance {
                id: DEFAULT_ID.into(),
                name: DEFAULT_NAME.into(),
                dir: root.join("game"),
                kind: Kind::Fabric,
                created_at: now(),
            });
        }
        if !c.instances.iter().any(|i| i.id == c.active) {
            c.active = c.instances[0].id.clone();
        }
        c.migrate_indexes();
        c
    }

    /// Moves `root/user-mods.json` & co. into the instance they describe.
    fn migrate_indexes(&self) {
        let legacy = self.root.join("game");
        let Some(inst) = self.instances.iter().find(|i| i.dir == legacy) else {
            return;
        };
        let meta = inst.dir.join(".minehouse");
        for name in LEGACY_INDEXES {
            let from = self.root.join(name);
            let to = meta.join(name);
            if from.exists() && !to.exists() {
                let _ = std::fs::create_dir_all(&meta);
                let _ = std::fs::rename(&from, &to);
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(&self.root)?;
        let file = Self::file(&self.root);
        let tmp = file.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, file)?;
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn data_dir(&self) -> PathBuf {
        self.data_dir.clone().unwrap_or_else(|| self.root.clone())
    }

    pub fn instances_dir(&self) -> PathBuf {
        self.data_dir().join("instances")
    }

    pub fn active(&self) -> &Instance {
        self.get(&self.active).unwrap_or(&self.instances[0])
    }

    pub fn get(&self, id: &str) -> Option<&Instance> {
        self.instances.iter().find(|i| i.id == id)
    }

    fn get_mut(&mut self, id: &str) -> Result<&mut Instance> {
        self.instances
            .iter_mut()
            .find(|i| i.id == id)
            .context("Нет такой сборки")
    }

    /// Paths of the selected instance.
    pub fn paths(&self) -> Paths {
        self.paths_for(self.active())
    }

    pub fn paths_for(&self, inst: &Instance) -> Paths {
        Paths {
            root: self.root.clone(),
            data: self.data_dir(),
            game: inst.dir.clone(),
        }
    }

    /// Folders the launcher made itself: only these are deleted or moved.
    fn managed(&self, inst: &Instance) -> bool {
        inst.dir == self.root.join("game")
            || inst.dir.parent() == Some(self.instances_dir().as_path())
    }

    fn check_name(&self, name: &str, except: Option<&str>) -> Result<String> {
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), "Введите название сборки");
        anyhow::ensure!(
            name.chars().count() <= MAX_NAME,
            "Название длиннее {MAX_NAME} символов"
        );
        let taken = self
            .instances
            .iter()
            .any(|i| Some(i.id.as_str()) != except && i.name.to_lowercase() == name.to_lowercase());
        anyhow::ensure!(!taken, "Сборка «{name}» уже есть");
        Ok(name.to_string())
    }

    /// An id that is also a free folder name in `instances/`.
    fn free_id(&self, name: &str) -> String {
        let base = slug(name);
        let dir = self.instances_dir();
        let free = |id: &str| self.get(id).is_none() && !dir.join(id).exists();
        if free(&base) {
            return base;
        }
        (2..)
            .map(|n| format!("{base}-{n}"))
            .find(|id| free(id))
            .unwrap()
    }

    /// Adds an empty instance and selects it; the next «Играть» installs it.
    pub fn create(&mut self, name: &str, kind: Kind) -> Result<Instance> {
        let name = self.check_name(name, None)?;
        let id = self.free_id(&name);
        let dir = self.instances_dir().join(&id);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("Не удалось создать папку {}", dir.display()))?;
        let inst = Instance {
            id: id.clone(),
            name,
            dir,
            kind,
            created_at: now(),
        };
        self.instances.push(inst.clone());
        self.active = id;
        self.save()?;
        Ok(inst)
    }

    /// A copy of an instance with its mods, packs, options and worlds; selected.
    pub fn duplicate(&mut self, from: &str, name: &str) -> Result<Instance> {
        let from = self.get(from).context("Нет такой сборки")?;
        let (src, kind) = (from.dir.clone(), from.kind);
        let name = self.check_name(name, None)?;
        let id = self.free_id(&name);
        let dir = self.instances_dir().join(&id);
        if src.exists() {
            if let Err(e) = copy_tree(&src, &dir, &|p| p.file_name() == Some("logs".as_ref())) {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(e).context("Не удалось скопировать сборку");
            }
        } else {
            std::fs::create_dir_all(&dir)?;
        }
        let inst = Instance {
            id: id.clone(),
            name,
            dir,
            kind,
            created_at: now(),
        };
        self.instances.push(inst.clone());
        self.active = id;
        self.save()?;
        Ok(inst)
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<()> {
        let name = self.check_name(name, Some(id))?;
        self.get_mut(id)?.name = name;
        self.save()
    }

    pub fn select(&mut self, id: &str) -> Result<()> {
        anyhow::ensure!(self.get(id).is_some(), "Нет такой сборки");
        self.active = id.to_string();
        self.save()
    }

    /// Forgets the instance and deletes its folder (worlds included).
    pub fn remove(&mut self, id: &str) -> Result<()> {
        anyhow::ensure!(
            self.instances.len() > 1,
            "Это последняя сборка, её нельзя удалить"
        );
        let pos = self
            .instances
            .iter()
            .position(|i| i.id == id)
            .context("Нет такой сборки")?;
        let inst = self.instances[pos].clone();
        if self.managed(&inst) {
            match std::fs::remove_dir_all(&inst.dir) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(e).with_context(|| {
                        format!("Не удалось удалить папку {}", inst.dir.display())
                    })
                }
            }
        }
        self.instances.remove(pos);
        if self.active == id {
            self.active = self.instances[0].id.clone();
        }
        self.save()
    }

    /// The folder the files would really go to if the player picks `chosen`:
    /// a non-empty folder that isn't ours gets a `MineHouse` subfolder, so
    /// picking «D:\Games» doesn't spill `libraries/`, `assets/`… into it.
    pub fn resolve_data_dir(&self, chosen: &Path) -> Result<PathBuf> {
        anyhow::ensure!(chosen.is_absolute(), "Нужен полный путь к папке");
        if chosen == self.root || chosen == self.data_dir() {
            return Ok(chosen.to_path_buf());
        }
        let ours =
            chosen.join("instances").is_dir() || SHARED.iter().any(|s| chosen.join(s).is_dir());
        let empty = std::fs::read_dir(chosen)
            .map(|mut d| d.next().is_none())
            .unwrap_or(true);
        Ok(if ours || empty {
            chosen.to_path_buf()
        } else {
            chosen.join("MineHouse")
        })
    }

    /// Moves the game files and the launcher's instance folders to `chosen`.
    /// Same disk: instant renames; another disk: copy, then delete the old
    /// copy. Saved after each step, so a failure halfway leaves every
    /// instance pointing at a complete folder. Returns the folder used.
    pub fn set_data_dir(&mut self, chosen: &Path, step: &dyn Fn(String)) -> Result<PathBuf> {
        let target = self.resolve_data_dir(chosen)?;
        let old = self.data_dir();
        if target == old {
            self.setup_done = true;
            self.save()?;
            return Ok(target);
        }
        let nested = |a: &Path, b: &Path| a != self.root && a.starts_with(b);
        anyhow::ensure!(
            !nested(&target, &old) && !nested(&old, &target),
            "Выберите папку не внутри нынешней папки игры"
        );
        for inst in &self.instances {
            anyhow::ensure!(
                !target.starts_with(&inst.dir),
                "Эта папка внутри сборки «{}»",
                inst.name
            );
        }
        std::fs::create_dir_all(&target)
            .with_context(|| format!("Не удалось создать папку {}", target.display()))?;
        let probe = target.join(".minehouse-write-test");
        std::fs::write(&probe, b"ok")
            .with_context(|| format!("Нет доступа на запись в {}", target.display()))?;
        let _ = std::fs::remove_file(&probe);

        let new_instances = target.join("instances");
        for i in 0..self.instances.len() {
            let inst = self.instances[i].clone();
            if !self.managed(&inst) {
                continue;
            }
            let mut dst = new_instances.join(&inst.id);
            let mut n = 2;
            while dst.exists() {
                dst = new_instances.join(format!("{}-{n}", inst.id));
                n += 1;
            }
            step(format!("Переносим сборку «{}»", inst.name));
            move_tree(&inst.dir, &dst)
                .with_context(|| format!("Не удалось перенести сборку «{}»", inst.name))?;
            self.instances[i].dir = dst;
            self.save()?;
        }

        step("Переносим Java и Minecraft".into());
        for name in SHARED {
            // Not fatal: anything left behind is downloaded again.
            let _ = move_tree(&old.join(name), &target.join(name));
        }
        let _ = std::fs::remove_dir(old.join("instances"));
        if old != self.root {
            let _ = std::fs::remove_dir(&old);
        }

        self.data_dir = (target != self.root).then(|| target.clone());
        self.setup_done = true;
        self.save()?;
        Ok(target)
    }
}

/// Folder-safe ASCII id: «Тест шейдеров» → `test-sheyderov`.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        let t: &str = match c {
            'a'..='z' | '0'..='9' => {
                out.push(c);
                continue;
            }
            'а' => "a",
            'б' => "b",
            'в' => "v",
            'г' => "g",
            'д' => "d",
            'е' | 'ё' | 'э' => "e",
            'ж' => "zh",
            'з' => "z",
            'и' => "i",
            'й' | 'ы' => "y",
            'к' => "k",
            'л' => "l",
            'м' => "m",
            'н' => "n",
            'о' => "o",
            'п' => "p",
            'р' => "r",
            'с' => "s",
            'т' => "t",
            'у' => "u",
            'ф' => "f",
            'х' => "h",
            'ц' => "ts",
            'ч' => "ch",
            'ш' => "sh",
            'щ' => "sch",
            'ю' => "yu",
            'я' => "ya",
            'ъ' | 'ь' => "",
            _ => "-",
        };
        out.push_str(t);
    }
    let parts: Vec<&str> = out.split('-').filter(|p| !p.is_empty()).collect();
    let mut s = parts.join("-");
    s.truncate(32);
    let s = s.trim_end_matches('-').to_string();
    if s.is_empty() {
        "instance".into()
    } else {
        s
    }
}

/// Copies `src` into `dst`, merging into existing folders; files already in
/// `dst` are kept. Symlinks (Mojang's macOS Java has them) stay symlinks.
pub fn copy_tree(src: &Path, dst: &Path, skip: &dyn Fn(&Path) -> bool) -> Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if meta.is_dir() {
        std::fs::create_dir_all(dst)?;
        for e in std::fs::read_dir(src)? {
            let e = e?;
            let path = e.path();
            if skip(&path) {
                continue;
            }
            copy_tree(&path, &dst.join(e.file_name()), skip)?;
        }
    } else if std::fs::symlink_metadata(dst).is_ok() {
        // Keep what is there.
    } else if meta.file_type().is_symlink() {
        let target = std::fs::read_link(src)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, dst)?;
        #[cfg(not(unix))]
        std::fs::copy(src.parent().unwrap_or(src).join(target), dst)?;
    } else {
        std::fs::copy(src, dst)
            .with_context(|| format!("{} → {}", src.display(), dst.display()))?;
    }
    Ok(())
}

/// Renames when it can, otherwise copies everything and only then deletes
/// the source, so a failed copy never loses files. A missing `src` is fine.
pub fn move_tree(src: &Path, dst: &Path) -> Result<()> {
    if std::fs::symlink_metadata(src).is_err() {
        return Ok(());
    }
    if std::fs::symlink_metadata(dst).is_err() {
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p)?;
        }
        if std::fs::rename(src, dst).is_ok() {
            return Ok(());
        }
    }
    copy_tree(src, dst, &|_| false)?;
    if std::fs::symlink_metadata(src)?.is_dir() {
        std::fs::remove_dir_all(src)?;
    } else {
        std::fs::remove_file(src)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slug("Тест шейдеров"), "test-sheyderov");
        assert_eq!(slug("  My Pack!! 2 "), "my-pack-2");
        assert_eq!(slug("★★★"), "instance");
    }

    #[test]
    fn fresh_config_keeps_old_instance() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("game/mods")).unwrap();
        std::fs::write(root.join("user-mods.json"), "{}").unwrap();
        let c = Config::load(root);
        assert!(c.setup_done);
        assert_eq!(c.active().dir, root.join("game"));
        assert_eq!(c.paths().data, root);
        assert!(root.join("game/.minehouse/user-mods.json").exists());
        assert!(!root.join("user-mods.json").exists());

        let empty = tempfile::tempdir().unwrap();
        assert!(!Config::load(empty.path()).setup_done);
    }

    #[test]
    fn create_rename_remove() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = Config::load(dir.path());
        let a = c.create("Шейдеры", Kind::Fabric).unwrap();
        assert_eq!(a.id, "sheydery");
        assert_eq!(c.active, "sheydery");
        assert!(a.dir.is_dir());
        assert!(c.create("шейдеры", Kind::Fabric).is_err());
        let b = c.duplicate("sheydery", "Шейдеры 2").unwrap();
        assert_eq!(b.id, "sheydery-2");
        c.rename("sheydery-2", "Лёгкая").unwrap();
        let h = c.create("Horizon", Kind::Horizon).unwrap();
        assert_eq!(h.kind, Kind::Horizon);

        let c2 = Config::load(dir.path());
        assert_eq!(c2.instances.len(), 4);
        assert_eq!(c2.get("horizon").unwrap().kind, Kind::Horizon);
        // The instance every launcher had is Fabric.
        assert_eq!(c2.get(DEFAULT_ID).unwrap().kind, Kind::Fabric);
        assert_eq!(c2.get("sheydery-2").unwrap().name, "Лёгкая");

        c.remove("sheydery").unwrap();
        assert!(!a.dir.exists());
        c.remove("sheydery-2").unwrap();
        c.remove("horizon").unwrap();
        assert!(c.remove(DEFAULT_ID).is_err());
    }

    #[test]
    fn move_data_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        std::fs::create_dir_all(root.join("game/saves/w")).unwrap();
        std::fs::write(root.join("game/saves/w/level.dat"), "x").unwrap();
        std::fs::create_dir_all(root.join("libraries/a")).unwrap();
        std::fs::write(root.join("libraries/a/b.jar"), "jar").unwrap();
        let mut c = Config::load(&root);
        c.create("Вторая", Kind::Fabric).unwrap();
        std::fs::write(c.get("vtoraya").unwrap().dir.join("options.txt"), "o").unwrap();

        // A folder with other things in it gets a MineHouse subfolder.
        let games = dir.path().join("Games");
        std::fs::create_dir_all(games.join("Other")).unwrap();
        let used = c.set_data_dir(&games, &|_| {}).unwrap();
        assert_eq!(used, games.join("MineHouse"));

        let c = Config::load(&root);
        let p = c.paths_for(c.get(DEFAULT_ID).unwrap());
        assert_eq!(p.data, used);
        assert_eq!(p.game, used.join("instances/minehouse"));
        assert!(p.game.join("saves/w/level.dat").exists());
        assert!(used.join("libraries/a/b.jar").exists());
        assert!(used.join("instances/vtoraya/options.txt").exists());
        assert!(!root.join("game").exists());
        assert!(!root.join("libraries").exists());
        assert!(!root.join("instances").exists());

        // And back to the default.
        let mut c = c;
        c.set_data_dir(&root, &|_| {}).unwrap();
        let c = Config::load(&root);
        assert_eq!(c.data_dir, None);
        assert!(root.join("instances/minehouse/saves/w/level.dat").exists());
        assert!(!used.exists());
    }

    #[test]
    fn copy_then_delete_across_folders() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("a");
        std::fs::create_dir_all(src.join("x")).unwrap();
        std::fs::write(src.join("x/f"), "1").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("f", src.join("x/link")).unwrap();
        let dst = dir.path().join("b");
        std::fs::create_dir_all(dst.join("x")).unwrap();
        std::fs::write(dst.join("x/f"), "keep").unwrap();
        move_tree(&src, &dst).unwrap();
        assert!(!src.exists());
        assert_eq!(std::fs::read_to_string(dst.join("x/f")).unwrap(), "keep");
        #[cfg(unix)]
        assert_eq!(
            std::fs::read_link(dst.join("x/link")).unwrap(),
            Path::new("f")
        );
    }
}
