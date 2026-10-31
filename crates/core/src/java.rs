//! Java from Mojang's own runtime manifest (the same builds the official
//! launcher uses), so players never install Java themselves.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::http::{self, Download, Hash};
use crate::paths::Paths;
use crate::progress::Reporter;

pub const RUNTIMES: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

type AllRuntimes = HashMap<String, HashMap<String, Vec<RuntimeEntry>>>;

#[derive(Debug, Deserialize)]
struct RuntimeEntry {
    manifest: ManifestRef,
    version: RuntimeVersion,
}

#[derive(Debug, Deserialize)]
struct ManifestRef {
    url: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeVersion {
    name: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeManifest {
    files: HashMap<String, RuntimeFile>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum RuntimeFile {
    Directory,
    File {
        #[serde(default)]
        executable: bool,
        downloads: FileDownloads,
    },
    Link {
        target: String,
    },
}

#[derive(Debug, Deserialize)]
struct FileDownloads {
    raw: Raw,
}

#[derive(Debug, Deserialize)]
struct Raw {
    sha1: String,
    size: u64,
    url: String,
}

/// Mojang's platform key for this machine.
pub fn platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "aarch64") => "windows-arm64",
        ("windows", _) => "windows-x64",
        ("macos", "aarch64") => "mac-os-arm64",
        ("macos", _) => "mac-os",
        (_, "x86") => "linux-i386",
        _ => "linux",
    }
}

/// Path of the `java` binary inside an installed runtime.
pub fn java_binary(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("jre.bundle/Contents/Home/bin/java")
    } else if cfg!(windows) {
        // javaw: no console window behind the game.
        home.join("bin").join("javaw.exe")
    } else {
        home.join("bin/java")
    }
}

/// Installs (or repairs) runtime `component`, returns the java binary.
pub async fn install(
    client: &reqwest::Client,
    paths: &Paths,
    component: &str,
    reporter: &Reporter,
) -> Result<PathBuf> {
    let home = paths.runtime(component);
    let all_cache = paths.root.join("runtime").join("all.json");
    let all: AllRuntimes = http::get_json_cached(client, RUNTIMES, &all_cache).await?;
    let entry = all
        .get(platform())
        .and_then(|p| p.get(component))
        .and_then(|v| v.first())
        .with_context(|| format!("Mojang has no Java '{component}' for {}", platform()))?;
    reporter.log(format!(
        "Java {} ({component}, {})",
        entry.version.name,
        platform()
    ));
    let manifest: RuntimeManifest =
        http::get_json_cached(client, &entry.manifest.url, &home.join("manifest.json")).await?;

    let mut downloads = Vec::new();
    let mut links = Vec::new();
    for (rel, f) in &manifest.files {
        let path = crate::paths::safe_join(&home, rel)?;
        match f {
            RuntimeFile::Directory => tokio::fs::create_dir_all(&path).await?,
            RuntimeFile::File {
                executable,
                downloads: d,
            } => downloads.push(Download {
                url: d.raw.url.clone(),
                path,
                hash: Hash::Sha1(d.raw.sha1.clone()),
                size: Some(d.raw.size),
                executable: *executable,
            }),
            RuntimeFile::Link { target } => links.push((path, target.clone())),
        }
    }
    http::fetch_all(client, downloads, 16, reporter).await?;

    #[cfg(unix)]
    for (path, target) in links {
        if tokio::fs::symlink_metadata(&path).await.is_ok() {
            continue;
        }
        if let Some(dir) = path.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        tokio::fs::symlink(&target, &path).await?;
    }
    #[cfg(not(unix))]
    drop(links);

    let java = java_binary(&home);
    anyhow::ensure!(
        java.exists(),
        "Java installed but {} is missing",
        java.display()
    );
    Ok(java)
}
