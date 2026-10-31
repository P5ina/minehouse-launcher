use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The launcher's own folder. Nothing is shared with the official launcher
/// or other launchers, so the MineHouse install can't break theirs.
#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    /// `%APPDATA%\MineHouse`, `~/Library/Application Support/MineHouse`,
    /// `~/.local/share/minehouse`. `MINEHOUSE_HOME` overrides it.
    pub fn default_location() -> Result<Self> {
        if let Some(home) = std::env::var_os("MINEHOUSE_HOME") {
            return Ok(Self::new(home));
        }
        let dirs = directories::ProjectDirs::from("pro", "minehouse", "MineHouse")
            .context("no home directory")?;
        Ok(Self::new(dirs.data_dir()))
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn runtime(&self, component: &str) -> PathBuf {
        self.root.join("runtime").join(component)
    }
    pub fn versions(&self) -> PathBuf {
        self.root.join("versions")
    }
    pub fn version_dir(&self, id: &str) -> PathBuf {
        self.versions().join(id)
    }
    pub fn libraries(&self) -> PathBuf {
        self.root.join("libraries")
    }
    pub fn assets(&self) -> PathBuf {
        self.root.join("assets")
    }
    /// `.minecraft` of the MineHouse instance: mods, config, saves, options.
    pub fn game_dir(&self) -> PathBuf {
        self.root.join("game")
    }
    pub fn natives(&self, version: &str) -> PathBuf {
        self.root.join("natives").join(version)
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
