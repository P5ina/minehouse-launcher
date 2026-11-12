use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The launcher's own folders. Nothing is shared with the official launcher
/// or other launchers, so the MineHouse install can't break theirs.
///
/// - `root`: launcher config (settings, account, list of instances, caches);
///   always in the system app-data folder.
/// - `data`: shared game files (Java, Minecraft, libraries, assets) and the
///   `instances/` folder; the player can move it (see [`crate::instances`]).
/// - `game`: `.minecraft` of the selected instance.
#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
    pub data: PathBuf,
    pub game: PathBuf,
}

impl Paths {
    /// `%APPDATA%\MineHouse`, `~/Library/Application Support/MineHouse`,
    /// `~/.local/share/minehouse`. `MINEHOUSE_HOME` overrides it.
    pub fn default_root() -> Result<PathBuf> {
        if let Some(home) = std::env::var_os("MINEHOUSE_HOME") {
            return Ok(home.into());
        }
        let dirs = directories::ProjectDirs::from("pro", "minehouse", "MineHouse")
            .context("no home directory")?;
        Ok(dirs.data_dir().to_path_buf())
    }

    /// The default root with the selected instance and the chosen data folder.
    pub fn default_location() -> Result<Self> {
        Ok(crate::instances::Config::load(&Self::default_root()?).paths())
    }

    /// Everything under one folder, the instance in `root/game` (tests, first run).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            data: root.clone(),
            game: root.join("game"),
            root,
        }
    }

    pub fn runtime(&self, component: &str) -> PathBuf {
        self.runtimes().join(component)
    }
    pub fn runtimes(&self) -> PathBuf {
        self.data.join("runtime")
    }
    pub fn versions(&self) -> PathBuf {
        self.data.join("versions")
    }
    pub fn version_dir(&self, id: &str) -> PathBuf {
        self.versions().join(id)
    }
    pub fn libraries(&self) -> PathBuf {
        self.data.join("libraries")
    }
    pub fn assets(&self) -> PathBuf {
        self.data.join("assets")
    }
    /// `.minecraft` of the selected instance: mods, config, saves, options.
    pub fn game_dir(&self) -> PathBuf {
        self.game.clone()
    }
    /// Launcher bookkeeping of the instance (installed mods and packs).
    pub fn instance_meta(&self) -> PathBuf {
        self.game.join(".minehouse")
    }
    pub fn natives(&self, version: &str) -> PathBuf {
        self.data.join("natives").join(version)
    }
    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }
    pub fn account_file(&self) -> PathBuf {
        self.root.join("account.json")
    }
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }
}

/// Joins a relative path from a manifest under `root`, refusing `..`,
/// absolute paths and drive prefixes so a bad manifest can't write elsewhere.
pub fn safe_join(root: &Path, rel: &str) -> Result<PathBuf> {
    use std::path::Component;
    let rel_path = Path::new(rel);
    anyhow::ensure!(!rel.is_empty(), "empty path");
    for c in rel_path.components() {
        match c {
            Component::Normal(_) => {}
            _ => anyhow::bail!("unsafe path in manifest: {rel}"),
        }
    }
    Ok(root.join(rel_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_rejects_escapes() {
        let root = Path::new("/x");
        assert!(safe_join(root, "mods/a.jar").is_ok());
        assert!(safe_join(root, "../a.jar").is_err());
        assert!(safe_join(root, "/etc/passwd").is_err());
        assert!(safe_join(root, "mods/../../a").is_err());
        assert!(safe_join(root, "").is_err());
    }
}
