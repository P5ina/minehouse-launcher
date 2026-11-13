//! The MineHouse modpack from the site's `manifest.json`. Same files and the
//! same `config/minehouse-pack.json` state as the in-game updater of the
//! MineHouse client mod (minehouse-client.jar, MineHouse Menu before pack
//! 1.14.0), so the two never fight. Files the pack dropped (the four old jars
//! of our mods in 1.14.0) are removed by the same sync. Mods the player added
//! themselves are never touched (see [`crate::user_mods`]).
//!
//! Optional mods: the manifest's `options` may be turned off by the player,
//! each file's `choices` says which options keep it (none: always). The choice
//! is `disabled` in the state file. An optional mod the player deleted (or
//! swapped for their own file) counts as turned off and is not downloaded
//! again; a required file always comes back.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::http::{self, Download, Hash};
use crate::paths::{safe_join, Paths};
use crate::progress::Reporter;

#[derive(Clone, Debug, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub files: Vec<PackFile>,
    /// Game version the pack is built for. Older manifests don't have these.
    #[serde(default)]
    pub minecraft: Option<String>,
    #[serde(default)]
    pub loader_version: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
    /// Modrinth mods on the server. A player who adds one of these gets
    /// exactly this version, or the server won't let them in.
    #[serde(default)]
    pub server_mods: Vec<ServerMod>,
    /// Mods the player may turn off. Older manifests: none, everything is required.
    #[serde(default)]
    pub options: Vec<PackOption>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PackOption {
    /// Modrinth slug, as in the files' `mod`.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub why: Option<String>,
    /// Options it can't run without (Iris needs Sodium).
    #[serde(default)]
    pub requires: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerMod {
    pub project: String,
    pub version: String,
    /// Fabric id of the server's jar: another project can ship the same id.
    #[serde(default)]
    pub mod_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PackFile {
    pub path: String,
    pub url: String,
    pub sha512: String,
    pub size: u64,
    /// Modrinth slug of the mod in this file (not on our own jars).
    #[serde(default, rename = "mod")]
    pub mod_slug: Option<String>,
    /// Installed while any of these options is on; `None`: always.
    #[serde(default)]
    pub choices: Option<Vec<String>>,
}

/// `config/minehouse-pack.json`
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub version: String,
    pub files: Vec<String>,
    /// Options the player turned off.
    #[serde(default)]
    pub disabled: BTreeSet<String>,
    /// Option → its main file as installed, to notice when the player deletes it.
    #[serde(default)]
    pub installed: BTreeMap<String, String>,
}

impl Manifest {
    /// Options that end up on: not turned off, and everything they require is on.
    pub fn enabled(&self, disabled: &BTreeSet<String>) -> HashSet<String> {
        let by_id: HashMap<&str, &PackOption> =
            self.options.iter().map(|o| (o.id.as_str(), o)).collect();
        fn on<'a>(
            id: &'a str,
            by_id: &HashMap<&str, &'a PackOption>,
            disabled: &BTreeSet<String>,
            seen: &mut HashSet<&'a str>,
        ) -> bool {
            let Some(o) = by_id.get(id) else { return false };
            if disabled.contains(id) {
                return false;
            }
            if !seen.insert(id) {
                return true;
            }
            o.requires.iter().all(|r| on(r, by_id, disabled, seen))
        }
        self.options
            .iter()
            .filter(|o| on(&o.id, &by_id, disabled, &mut HashSet::new()))
            .map(|o| o.id.clone())
            .collect()
    }

    /// The files to have installed with this choice.
    pub fn wanted(&self, disabled: &BTreeSet<String>) -> Vec<&PackFile> {
        let on = self.enabled(disabled);
        self.files
            .iter()
            .filter(|f| match &f.choices {
                None => true,
                Some(c) => c.is_empty() || c.iter().any(|c| on.contains(c)),
            })
            .collect()
    }

    pub fn main_file(&self, option: &str) -> Option<&PackFile> {
        self.files
            .iter()
            .find(|f| f.mod_slug.as_deref() == Some(option))
    }

    /// The player's choice: what they turned off, plus optional mods they
    /// deleted from `mods/` since the last install.
    pub fn disabled_after(&self, old: Option<&State>, game: &Path) -> BTreeSet<String> {
        let Some(old) = old else {
            return BTreeSet::new();
        };
        let mut disabled = old.disabled.clone();
        let had: HashSet<&str> = old.files.iter().map(String::as_str).collect();
        let gone = |p: &str| safe_join(game, p).map(|p| !p.exists()).unwrap_or(false);
        for o in &self.options {
            if disabled.contains(&o.id) {
                continue;
            }
            let Some(main) = self.main_file(&o.id) else {
                continue;
            };
            let prev = old.installed.get(&o.id).map(String::as_str).or_else(|| {
                had.contains(main.path.as_str())
                    .then_some(main.path.as_str())
            });
            if let Some(prev) = prev {
                if gone(prev) && gone(&main.path) {
                    disabled.insert(o.id.clone());
                }
            }
        }
        disabled
    }
}

pub async fn fetch_manifest(client: &reqwest::Client, paths: &Paths) -> Result<Manifest> {
    let cache = paths.root.join("modpack-manifest.json");
    // MINEHOUSE_MANIFEST_URL: try a pack build before it is on the site.
    let url =
        std::env::var("MINEHOUSE_MANIFEST_URL").unwrap_or_else(|_| crate::MANIFEST_URL.into());
    http::get_json_cached(client, &url, &cache).await
}

fn state_path(paths: &Paths) -> std::path::PathBuf {
    paths.game_dir().join("config").join("minehouse-pack.json")
}

pub async fn read_state(paths: &Paths) -> Option<State> {
    let bytes = tokio::fs::read(state_path(paths)).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn read_state_sync(paths: &Paths) -> Option<State> {
    let bytes = std::fs::read(state_path(paths)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Options the player turned off (as of the last install or choice).
pub fn disabled(paths: &Paths) -> BTreeSet<String> {
    read_state_sync(paths)
        .map(|s| s.disabled)
        .unwrap_or_default()
}

/// Saves the player's choice; [`install`] applies it.
pub fn set_disabled(paths: &Paths, disabled: BTreeSet<String>) -> Result<()> {
    let mut state = read_state_sync(paths).unwrap_or_default();
    state.disabled = disabled;
    let path = state_path(paths);
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, serde_json::to_vec_pretty(&state)?)?;
    Ok(())
}

/// Downloads new/changed pack files, removes files the pack dropped,
/// writes the state file and first-run defaults.
pub async fn install(
    client: &reqwest::Client,
    paths: &Paths,
    manifest: &Manifest,
    reporter: &Reporter,
) -> Result<()> {
    let game = paths.game_dir();
    tokio::fs::create_dir_all(game.join("mods")).await?;
    tokio::fs::create_dir_all(game.join("config")).await?;

    let old = read_state(paths).await;
    let disabled = manifest.disabled_after(old.as_ref(), &game);
    if let Some(o) = &old {
        for id in disabled.difference(&o.disabled) {
            reporter.log(format!(
                "Мод сборки «{id}» удалён из папки mods, больше не ставим"
            ));
        }
    }
    let wanted_files = manifest.wanted(&disabled);

    let mut downloads = Vec::new();
    for f in &wanted_files {
        downloads.push(Download {
            url: f.url.clone(),
            path: safe_join(&game, &f.path)?,
            hash: Hash::Sha512(f.sha512.clone()),
            size: Some(f.size),
            executable: false,
        });
    }
    let fetched = http::fetch_all(client, downloads, 8, reporter).await?;
    if fetched > 0 {
        reporter.log(format!(
            "Сборка {}: обновлено файлов: {fetched}",
            manifest.version
        ));
    }

    let wanted: HashSet<&str> = wanted_files.iter().map(|f| f.path.as_str()).collect();
    if let Some(old) = &old {
        for p in old.files.iter().filter(|p| !wanted.contains(p.as_str())) {
            if let Ok(path) = safe_join(&game, p) {
                if tokio::fs::remove_file(&path).await.is_ok() {
                    reporter.log(format!("Удалён файл сборки: {p}"));
                }
            }
        }
    }
    let on = manifest.enabled(&disabled);
    let state = State {
        version: manifest.version.clone(),
        files: wanted_files.iter().map(|f| f.path.clone()).collect(),
        installed: wanted_files
            .iter()
            .filter_map(|f| {
                let m = f.mod_slug.as_ref()?;
                on.contains(m).then(|| (m.clone(), f.path.clone()))
            })
            .collect(),
        disabled,
    };
    tokio::fs::write(state_path(paths), serde_json::to_vec_pretty(&state)?).await?;

    // First-run defaults from the .mrpack overrides; the player's own edits win later.
    let menu_cfg = game.join("config").join("minehouse-menu.properties");
    if !menu_cfg.exists() {
        let url = format!("{}/modpack/minehouse-menu.properties", crate::SITE);
        match client
            .get(&url)
            .send()
            .await
            .and_then(|r| r.error_for_status())
        {
            Ok(res) => tokio::fs::write(&menu_cfg, res.bytes().await?).await?,
            Err(e) => tracing::warn!("{url}: {e}"),
        }
    }
    let servers = game.join("servers.dat");
    if !servers.exists() {
        let server = manifest.server.as_deref().unwrap_or(crate::SERVER);
        tokio::fs::write(&servers, servers_dat(&[("MineHouse", server)])).await?;
    }
    Ok(())
}

fn nbt_string(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u16).to_be_bytes());
    out.extend_from_slice(s.as_bytes());
}

/// Uncompressed NBT `{servers: [{name, ip, acceptTextures: 1b}]}`.
pub fn servers_dat(servers: &[(&str, &str)]) -> Vec<u8> {
    let mut o = vec![0x0a];
    nbt_string(&mut o, "");
    o.push(0x09);
    nbt_string(&mut o, "servers");
    o.push(0x0a);
    o.extend_from_slice(&(servers.len() as i32).to_be_bytes());
    for (name, ip) in servers {
        o.push(0x08);
        nbt_string(&mut o, "name");
        nbt_string(&mut o, name);
        o.push(0x08);
        nbt_string(&mut o, "ip");
        nbt_string(&mut o, ip);
        o.push(0x01);
        nbt_string(&mut o, "acceptTextures");
        o.push(1);
        o.push(0);
    }
    o.push(0);
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_without_game_fields_parses() {
        let m: Manifest = serde_json::from_str(
            r#"{"version":"1.0.0","files":[{"path":"mods/a.jar","url":"https://x/a.jar","sha512":"ab","size":1}]}"#,
        )
        .unwrap();
        assert_eq!(m.minecraft, None);
        assert_eq!(m.files[0].path, "mods/a.jar");
    }

    fn optional_manifest() -> Manifest {
        serde_json::from_str(
            r#"{"version":"2","options":[
                {"id":"sodium","name":"Sodium"},
                {"id":"iris","name":"Iris","requires":["sodium"]},
                {"id":"litematica","name":"Litematica"}],
              "files":[
                {"path":"mods/fabric-api.jar","url":"u","sha512":"a","size":1,"mod":"fabric-api"},
                {"path":"mods/sodium.jar","url":"u","sha512":"a","size":1,"mod":"sodium","choices":["sodium"]},
                {"path":"mods/iris.jar","url":"u","sha512":"a","size":1,"mod":"iris","choices":["iris"]},
                {"path":"mods/litematica.jar","url":"u","sha512":"a","size":1,"mod":"litematica","choices":["litematica"]},
                {"path":"mods/malilib.jar","url":"u","sha512":"a","size":1,"mod":"malilib","choices":["litematica"]}]}"#,
        )
        .unwrap()
    }

    fn paths_of(files: Vec<&PackFile>) -> Vec<&str> {
        files.into_iter().map(|f| f.path.as_str()).collect()
    }

    #[test]
    fn choice_drops_mod_its_libraries_and_dependents() {
        let m = optional_manifest();
        assert_eq!(m.wanted(&BTreeSet::new()).len(), 5);
        let off: BTreeSet<String> = ["sodium".to_string(), "litematica".to_string()].into();
        assert_eq!(paths_of(m.wanted(&off)), vec!["mods/fabric-api.jar"]);
    }

    #[test]
    fn deleted_optional_mod_stays_off_required_comes_back() {
        let m = optional_manifest();
        let dir = std::env::temp_dir().join(format!("mh-pack-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("mods")).unwrap();
        // The player deleted Litematica and fabric-api; legacy state without `installed`.
        for f in ["sodium.jar", "iris.jar", "malilib.jar"] {
            std::fs::write(dir.join("mods").join(f), b"x").unwrap();
        }
        let old = State {
            version: "1".into(),
            files: m.files.iter().map(|f| f.path.clone()).collect(),
            ..Default::default()
        };
        let disabled = m.disabled_after(Some(&old), &dir);
        assert_eq!(disabled, BTreeSet::from(["litematica".to_string()]));
        assert_eq!(
            paths_of(m.wanted(&disabled)),
            vec!["mods/fabric-api.jar", "mods/sodium.jar", "mods/iris.jar"]
        );
        // A fresh install has nothing to compare with.
        assert!(m.disabled_after(None, &dir).is_empty());
        // Updated mod with a new file name: the old one, still there, counts as installed.
        let old = State {
            version: "1".into(),
            files: vec![],
            installed: BTreeMap::from([("sodium".into(), "mods/iris.jar".into())]),
            ..Default::default()
        };
        assert!(m.disabled_after(Some(&old), &dir).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn servers_dat_layout() {
        let b = servers_dat(&[("MineHouse", "play.minehouse.pro")]);
        assert_eq!(&b[..3], &[0x0a, 0, 0]);
        assert_eq!(*b.last().unwrap(), 0);
        assert!(b.windows(18).any(|w| w == b"play.minehouse.pro"));
    }
}
