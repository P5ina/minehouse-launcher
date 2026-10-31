//! Mojang version JSON: client jar, libraries, assets, log config.

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::http::{self, Download, Hash};
use crate::paths::Paths;
use crate::rules::{Env, Rule};

pub const VERSION_MANIFEST: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
pub const RESOURCES: &str = "https://resources.download.minecraft.net";

#[derive(Debug, Deserialize)]
pub struct VersionManifest {
    pub versions: Vec<VersionRef>,
}

#[derive(Debug, Deserialize)]
pub struct VersionRef {
    pub id: String,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub id: String,
    #[serde(default)]
    pub inherits_from: Option<String>,
    pub main_class: String,
    #[serde(default)]
    pub arguments: Arguments,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub asset_index: Option<AssetIndexRef>,
    #[serde(default)]
    pub downloads: Option<Downloads>,
    #[serde(default)]
    pub java_version: Option<JavaVersion>,
    #[serde(default)]
    pub logging: Option<Logging>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Argument>,
    #[serde(default)]
    pub jvm: Vec<Argument>,
    /// Mojang's suggested memory/GC flags (since 26.x).
    #[serde(default, rename = "default-user-jvm")]
    pub default_user_jvm: Vec<Argument>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Argument {
    Plain(String),
    Conditional {
        #[serde(default)]
        rules: Vec<Rule>,
        value: OneOrMany,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl Argument {
    /// The argument's strings if its rules allow it here.
    pub fn resolve(&self, env: &Env) -> Vec<String> {
        match self {
            Argument::Plain(s) => vec![s.clone()],
            Argument::Conditional { rules, value } if env.allows(Some(rules)) => match value {
                OneOrMany::One(s) => vec![s.clone()],
                OneOrMany::Many(v) => v.clone(),
            },
            Argument::Conditional { .. } => vec![],
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    /// Maven repository base (Fabric-style entries).
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LibraryDownloads {
    #[serde(default)]
    pub artifact: Option<Artifact>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Artifact {
    pub path: Option<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AssetIndexRef {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Downloads {
    pub client: Artifact,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub component: String,
    pub major_version: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Logging {
    pub client: Option<LoggingClient>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LoggingClient {
    pub argument: String,
    pub file: LoggingFile,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LoggingFile {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct AssetIndex {
    pub objects: std::collections::HashMap<String, AssetObject>,
}

#[derive(Debug, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

/// `group:artifact:version[:classifier][@ext]` → maven path.
pub fn maven_path(name: &str) -> Result<String> {
    let (coords, ext) = name.split_once('@').unwrap_or((name, "jar"));
    let parts: Vec<&str> = coords.split(':').collect();
    anyhow::ensure!(parts.len() >= 3, "bad maven name {name}");
    let (group, artifact, version) = (parts[0], parts[1], parts[2]);
    let classifier = parts.get(3).map(|c| format!("-{c}")).unwrap_or_default();
    Ok(format!(
        "{}/{artifact}/{version}/{artifact}-{version}{classifier}.{ext}",
        group.replace('.', "/")
    ))
}

/// `group:artifact[:classifier]` without the version, to drop duplicates.
pub fn library_key(name: &str) -> String {
    let coords = name.split('@').next().unwrap_or(name);
    let parts: Vec<&str> = coords.split(':').collect();
    match parts.as_slice() {
        [g, a, _v, c, ..] => format!("{g}:{a}:{c}"),
        [g, a, ..] => format!("{g}:{a}"),
        _ => coords.to_owned(),
    }
}

/// Where a library goes and where to get it, or None when this OS doesn't need it.
pub fn library_download(lib: &Library, paths: &Paths, env: &Env) -> Result<Option<Download>> {
    if !env.allows(lib.rules.as_deref()) {
        return Ok(None);
    }
    if let Some(art) = lib.downloads.as_ref().and_then(|d| d.artifact.as_ref()) {
        let rel = match &art.path {
            Some(p) => p.clone(),
            None => maven_path(&lib.name)?,
        };
        return Ok(Some(Download {
            url: art.url.clone(),
            path: crate::paths::safe_join(&paths.libraries(), &rel)?,
            hash: art.sha1.clone().map(Hash::Sha1).unwrap_or(Hash::None),
            size: art.size,
            executable: false,
        }));
    }
    if lib.downloads.is_some() {
        // Only natives classifiers (pre-1.19 format), nothing for the classpath.
        return Ok(None);
    }
    let rel = maven_path(&lib.name)?;
    let base = lib
        .url
        .as_deref()
        .unwrap_or("https://libraries.minecraft.net/");
    let base = base.trim_end_matches('/');
    Ok(Some(Download {
        url: format!("{base}/{rel}"),
        path: crate::paths::safe_join(&paths.libraries(), &rel)?,
        hash: lib.sha1.clone().map(Hash::Sha1).unwrap_or(Hash::None),
        size: lib.size,
        executable: false,
    }))
}

pub async fn fetch_version(
    client: &reqwest::Client,
    paths: &Paths,
    id: &str,
) -> Result<VersionJson> {
    let cache = paths.version_dir(id).join(format!("{id}.json"));
    let manifest: Result<VersionManifest> = http::get_json(client, VERSION_MANIFEST).await;
    match manifest {
        Ok(m) => {
            let v = m
                .versions
                .iter()
                .find(|v| v.id == id)
                .with_context(|| format!("Minecraft {id} not found in Mojang's manifest"))?;
            http::get_json_cached(client, &v.url, &cache).await
        }
        Err(e) => {
            let bytes = tokio::fs::read(&cache).await.map_err(|_| e)?;
            Ok(serde_json::from_slice(&bytes)?)
        }
    }
}

pub fn client_jar(paths: &Paths, v: &VersionJson) -> Result<Download> {
    let art = &v
        .downloads
        .as_ref()
        .context("version has no client download")?
        .client;
    Ok(Download {
        url: art.url.clone(),
        path: paths.version_dir(&v.id).join(format!("{}.jar", v.id)),
        hash: art.sha1.clone().map(Hash::Sha1).unwrap_or(Hash::None),
        size: art.size,
        executable: false,
    })
}

pub async fn asset_downloads(
    client: &reqwest::Client,
    paths: &Paths,
    v: &VersionJson,
) -> Result<Vec<Download>> {
    let idx = v
        .asset_index
        .as_ref()
        .context("version has no asset index")?;
    let index_path = paths
        .assets()
        .join("indexes")
        .join(format!("{}.json", idx.id));
    let index_dl = Download {
        url: idx.url.clone(),
        path: index_path.clone(),
        hash: Hash::Sha1(idx.sha1.clone()),
        size: Some(idx.size),
        executable: false,
    };
    http::fetch_all(
        client,
        vec![index_dl],
        1,
        &crate::progress::Reporter::silent(),
    )
    .await?;
    let index: AssetIndex = serde_json::from_slice(&tokio::fs::read(&index_path).await?)?;
    let mut seen = std::collections::HashSet::new();
    Ok(index
        .objects
        .into_values()
        .filter(|o| seen.insert(o.hash.clone()))
        .map(|o| {
            let prefix = &o.hash[..2];
            Download {
                url: format!("{RESOURCES}/{prefix}/{}", o.hash),
                path: paths.assets().join("objects").join(prefix).join(&o.hash),
                hash: Hash::Sha1(o.hash.clone()),
                size: Some(o.size),
                executable: false,
            }
        })
        .collect())
}

pub fn log_config(paths: &Paths, v: &VersionJson) -> Option<(String, Download)> {
    let c = v.logging.as_ref()?.client.as_ref()?;
    let path = paths.assets().join("log_configs").join(&c.file.id);
    Some((
        c.argument.replace("${path}", &path.to_string_lossy()),
        Download {
            url: c.file.url.clone(),
            path,
            hash: Hash::Sha1(c.file.sha1.clone()),
            size: Some(c.file.size),
            executable: false,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maven() {
        assert_eq!(
            maven_path("net.fabricmc:fabric-loader:0.20.1").unwrap(),
            "net/fabricmc/fabric-loader/0.20.1/fabric-loader-0.20.1.jar"
        );
        assert_eq!(
            maven_path("org.lwjgl:lwjgl:3.4.3:natives-linux").unwrap(),
            "org/lwjgl/lwjgl/3.4.3/lwjgl-3.4.3-natives-linux.jar"
        );
        assert_eq!(library_key("org.ow2.asm:asm:9.10.1"), "org.ow2.asm:asm");
        assert_eq!(
            library_key("org.lwjgl:lwjgl:3.4.3:natives-linux"),
            "org.lwjgl:lwjgl:natives-linux"
        );
    }
}
