//! The MineHouse modpack from the site's `manifest.json`. Same files and the
//! same `config/minehouse-pack.json` state as the in-game updater of the
//! MineHouse Menu mod, so the two never fight. Mods the player added
//! themselves are never touched.

use std::collections::HashSet;

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
}

#[derive(Clone, Debug, Deserialize)]
pub struct PackFile {
    pub path: String,
    pub url: String,
    pub sha512: String,
    pub size: u64,
}

/// `config/minehouse-pack.json`
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State {
    pub version: String,
    pub files: Vec<String>,
}

pub async fn fetch_manifest(client: &reqwest::Client, paths: &Paths) -> Result<Manifest> {
    let cache = paths.root.join("modpack-manifest.json");
    http::get_json_cached(client, crate::MANIFEST_URL, &cache).await
}

fn state_path(paths: &Paths) -> std::path::PathBuf {
    paths.game_dir().join("config").join("minehouse-pack.json")
}

pub async fn read_state(paths: &Paths) -> Option<State> {
    let bytes = tokio::fs::read(state_path(paths)).await.ok()?;
    serde_json::from_slice(&bytes).ok()
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

    let mut downloads = Vec::new();
    for f in &manifest.files {
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

    let wanted: HashSet<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
    if let Some(old) = read_state(paths).await {
        for p in old.files.iter().filter(|p| !wanted.contains(p.as_str())) {
            if let Ok(path) = safe_join(&game, p) {
                if tokio::fs::remove_file(&path).await.is_ok() {
                    reporter.log(format!("Удалён старый файл сборки: {p}"));
                }
            }
        }
    }
    let state = State {
        version: manifest.version.clone(),
        files: manifest.files.iter().map(|f| f.path.clone()).collect(),
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

    #[test]
    fn servers_dat_layout() {
        let b = servers_dat(&[("MineHouse", "play.minehouse.pro")]);
        assert_eq!(&b[..3], &[0x0a, 0, 0]);
        assert_eq!(*b.last().unwrap(), 0);
        assert!(b.windows(18).any(|w| w == b"play.minehouse.pro"));
    }
}
