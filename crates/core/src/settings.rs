use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::paths::Paths;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// -Xmx in megabytes.
    pub memory_mb: u32,
    /// Extra JVM flags, split on spaces.
    pub extra_jvm_args: String,
    /// Close the launcher window once the game window is up.
    pub close_on_launch: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            memory_mb: 4096,
            extra_jvm_args: String::new(),
            close_on_launch: false,
        }
    }
}

impl Settings {
    pub fn load(paths: &Paths) -> Self {
        std::fs::read(paths.settings_file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        std::fs::create_dir_all(&paths.root)?;
        std::fs::write(paths.settings_file(), serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}
