//! The few Modrinth API v2 calls the mod browser needs.
//! <https://docs.modrinth.com/api/>

use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const API: &str = "https://api.modrinth.com/v2";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SearchHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub client_side: String,
    #[serde(default)]
    pub server_side: String,
}

#[derive(Debug, Deserialize)]
struct SearchResult {
    hits: Vec<SearchHit>,
    total_hits: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Project {
    pub id: String,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub client_side: String,
    #[serde(default)]
    pub server_side: String,
    #[serde(default)]
    pub project_type: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Version {
    pub id: String,
    pub project_id: String,
    pub version_number: String,
    #[serde(default)]
    pub version_type: String,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    pub files: Vec<VersionFile>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Dependency {
    pub version_id: Option<String>,
    pub project_id: Option<String>,
    pub dependency_type: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct VersionFile {
    pub url: String,
    pub filename: String,
    pub size: u64,
    #[serde(default)]
    pub primary: bool,
    pub hashes: HashMap<String, String>,
}

impl Version {
    /// The jar to download: the one marked primary, else the first jar that
    /// isn't sources/javadoc.
    pub fn primary_file(&self) -> Option<&VersionFile> {
        let jar = |f: &&VersionFile| {
            f.filename.ends_with(".jar")
                && !f.filename.ends_with("-sources.jar")
                && !f.filename.ends_with("-javadoc.jar")
        };
        self.files
            .iter()
            .filter(jar)
            .find(|f| f.primary)
            .or_else(|| self.files.iter().find(jar))
    }

    /// The `.zip` of a resource pack or shader pack version.
    pub fn primary_zip(&self) -> Option<&VersionFile> {
        let zip = |f: &&VersionFile| f.filename.to_ascii_lowercase().ends_with(".zip");
        self.files
            .iter()
            .filter(zip)
            .find(|f| f.primary)
            .or_else(|| self.files.iter().find(zip))
    }
}

/// Fabric mods for `mc`, most relevant first (most downloaded for an empty
/// query). `exclude` projects are left out.
pub async fn search(
    client: &reqwest::Client,
    query: &str,
    mc: &str,
    offset: u32,
    exclude: &[String],
) -> Result<(Vec<SearchHit>, u64)> {
    let mut facets = vec![
        vec!["categories:fabric".to_string()],
        vec![format!("versions:{mc}")],
        vec!["project_type:mod".to_string()],
    ];
    facets.extend(exclude.iter().map(|id| vec![format!("project_id!={id}")]));
    search_facets(client, query, offset, &facets).await
}

/// Resource packs (`project_type` "resourcepack") or shader packs ("shader",
/// only the ones made for Iris) for `mc`.
pub async fn search_content(
    client: &reqwest::Client,
    project_type: &str,
    category: Option<&str>,
    query: &str,
    mc: &str,
    offset: u32,
) -> Result<(Vec<SearchHit>, u64)> {
    let mut facets = vec![
        vec![format!("project_type:{project_type}")],
        vec![format!("versions:{mc}")],
    ];
    if let Some(c) = category {
        facets.push(vec![format!("categories:{c}")]);
    }
    search_facets(client, query, offset, &facets).await
}

async fn search_facets(
    client: &reqwest::Client,
    query: &str,
    offset: u32,
    facets: &[Vec<String>],
) -> Result<(Vec<SearchHit>, u64)> {
    let facets = serde_json::to_string(facets)?;
    let index = if query.trim().is_empty() {
        "downloads"
    } else {
        "relevance"
    };
    let res: SearchResult = client
        .get(format!("{API}/search"))
        .query(&[
            ("query", query.trim()),
            ("facets", &facets),
            ("index", index),
            ("limit", "20"),
            ("offset", &offset.to_string()),
        ])
        .send()
        .await
        .context("Modrinth недоступен")?
        .error_for_status()?
        .json()
        .await?;
    Ok((res.hits, res.total_hits))
}

pub async fn projects(client: &reqwest::Client, ids: &[String]) -> Result<Vec<Project>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(client
        .get(format!("{API}/projects"))
        .query(&[("ids", serde_json::to_string(ids)?)])
        .send()
        .await
        .context("Modrinth недоступен")?
        .error_for_status()?
        .json()
        .await?)
}

pub async fn version(client: &reqwest::Client, id: &str) -> Result<Version> {
    Ok(client
        .get(format!("{API}/version/{id}"))
        .send()
        .await
        .context("Modrinth недоступен")?
        .error_for_status()?
        .json()
        .await?)
}

/// The version a file belongs to, by its sha512.
pub async fn version_from_hash(client: &reqwest::Client, sha512: &str) -> Result<Version> {
    Ok(client
        .get(format!("{API}/version_file/{sha512}"))
        .query(&[("algorithm", "sha512")])
        .send()
        .await
        .context("Modrinth недоступен")?
        .error_for_status()?
        .json()
        .await?)
}

/// Newest Fabric version of a project for `mc`; releases win over betas/alphas.
pub async fn latest_version(
    client: &reqwest::Client,
    project: &str,
    mc: &str,
) -> Result<Option<Version>> {
    let versions: Vec<Version> = client
        .get(format!("{API}/project/{project}/version"))
        .query(&[
            ("loaders", r#"["fabric"]"#.to_string()),
            ("game_versions", serde_json::to_string(&[mc])?),
        ])
        .send()
        .await
        .context("Modrinth недоступен")?
        .error_for_status()?
        .json()
        .await?;
    Ok(pick_latest(versions))
}

/// Newest version of a resource pack (`loader` "minecraft") or shader pack
/// ("iris") that lists `mc`; releases win.
pub async fn latest_content_version(
    client: &reqwest::Client,
    project: &str,
    loader: &str,
    mc: &str,
) -> Result<Option<Version>> {
    let versions: Vec<Version> = client
        .get(format!("{API}/project/{project}/version"))
        .query(&[
            ("loaders", serde_json::to_string(&[loader])?),
            ("game_versions", serde_json::to_string(&[mc])?),
        ])
        .send()
        .await
        .context("Modrinth недоступен")?
        .error_for_status()?
        .json()
        .await?;
    let usable: Vec<Version> = versions
        .into_iter()
        .filter(|v| v.primary_zip().is_some())
        .collect();
    let idx = usable
        .iter()
        .position(|v| v.version_type == "release")
        .unwrap_or(0);
    Ok(usable.into_iter().nth(idx))
}

/// Modrinth lists versions newest first.
fn pick_latest(versions: Vec<Version>) -> Option<Version> {
    let usable: Vec<Version> = versions
        .into_iter()
        .filter(|v| v.primary_file().is_some())
        .collect();
    let release = usable.iter().position(|v| v.version_type == "release");
    let idx = release.unwrap_or(0);
    usable.into_iter().nth(idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(id: &str, kind: &str, file: &str) -> Version {
        serde_json::from_value(serde_json::json!({
            "id": id, "project_id": "p", "version_number": id, "version_type": kind,
            "files": [{"url": "https://x", "filename": file, "size": 1, "primary": false, "hashes": {}}]
        }))
        .unwrap()
    }

    #[test]
    fn release_wins_over_newer_beta() {
        let got = pick_latest(vec![v("b", "beta", "a.jar"), v("r", "release", "a.jar")]);
        assert_eq!(got.unwrap().id, "r");
        let got = pick_latest(vec![v("b", "beta", "a.jar"), v("a", "alpha", "a.jar")]);
        assert_eq!(got.unwrap().id, "b");
    }

    #[test]
    fn primary_file_skips_sources() {
        let ver: Version = serde_json::from_value(serde_json::json!({
            "id": "x", "project_id": "p", "version_number": "1",
            "files": [
                {"url": "u1", "filename": "m-sources.jar", "size": 1, "primary": false, "hashes": {}},
                {"url": "u2", "filename": "m.jar", "size": 1, "primary": false, "hashes": {}}
            ]
        }))
        .unwrap();
        assert_eq!(ver.primary_file().unwrap().url, "u2");
    }
}
