//! Resource packs and shader packs the player adds to the MineHouse instance:
//! from Modrinth or their own `.zip` files.
//!
//! They go where the game looks for them: `resourcepacks/` and
//! `shaderpacks/` in the game folder. The pack updater never touches these
//! folders. What is turned on is stored exactly where the game keeps it, so
//! the launcher and the in-game screens always agree:
//! - resource packs: `resourcePacks` in `options.txt` (`file/<name>`; the last
//!   one wins over the others);
//! - shaders: `shaderPack` + `enableShaders` in `config/iris.properties`. Only
//!   one shader can be on, and it needs Iris (an optional mod of the pack).
//!
//! The Modrinth project, version and icon of each file are kept in
//! `content-packs.json` next to the instance.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::http::{self, Download, Hash};
use crate::modrinth;
use crate::paths::Paths;
use crate::progress::Reporter;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Resource,
    Shader,
}

impl Kind {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "resource" | "resourcepack" | "resourcepacks" => Ok(Self::Resource),
            "shader" | "shaders" | "shaderpacks" => Ok(Self::Shader),
            _ => bail!("неизвестный тип: {s}"),
        }
    }
    pub fn dir(self, paths: &Paths) -> PathBuf {
        paths.game_dir().join(match self {
            Self::Resource => "resourcepacks",
            Self::Shader => "shaderpacks",
        })
    }
    /// Modrinth `project_type`.
    pub fn project_type(self) -> &'static str {
        match self {
            Self::Resource => "resourcepack",
            Self::Shader => "shader",
        }
    }
    /// Modrinth loader of the versions we can use.
    pub fn loader(self) -> &'static str {
        match self {
            Self::Resource => "minecraft",
            Self::Shader => "iris",
        }
    }
    /// Extra search category: only shaders that run on Iris.
    pub fn category(self) -> Option<&'static str> {
        match self {
            Self::Resource => None,
            Self::Shader => Some("iris"),
        }
    }
    fn noun(self) -> &'static str {
        match self {
            Self::Resource => "Ресурспак",
            Self::Shader => "Шейдер",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Entry {
    file: String,
    title: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    version_id: Option<String>,
    #[serde(default)]
    icon: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    #[serde(default)]
    resource: Vec<Entry>,
    #[serde(default)]
    shader: Vec<Entry>,
}

impl Index {
    fn list(&mut self, kind: Kind) -> &mut Vec<Entry> {
        match kind {
            Kind::Resource => &mut self.resource,
            Kind::Shader => &mut self.shader,
        }
    }
}

/// A pack as the window shows it.
#[derive(Clone, Debug, Serialize)]
pub struct ContentPack {
    /// File (or folder) name in `resourcepacks/` / `shaderpacks/`.
    pub file: String,
    pub title: String,
    pub version: String,
    pub project: Option<String>,
    pub icon: Option<String>,
    pub enabled: bool,
    /// Place among the enabled resource packs: higher wins over lower.
    pub order: Option<usize>,
}

fn index_path(paths: &Paths) -> PathBuf {
    paths.root.join("content-packs.json")
}

fn load_index(paths: &Paths) -> Index {
    std::fs::read(index_path(paths))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_index(paths: &Paths, index: &Index) -> Result<()> {
    std::fs::create_dir_all(&paths.root)?;
    std::fs::write(index_path(paths), serde_json::to_vec_pretty(index)?)?;
    Ok(())
}

fn check_name(name: &str) -> Result<()> {
    anyhow::ensure!(
        !name.is_empty()
            && !name.contains(['/', '\\'])
            && name != "."
            && name != ".."
            && !name.starts_with('.'),
        "плохое имя файла: {name}"
    );
    Ok(())
}

fn is_pack(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    !name.starts_with('.') && (path.is_dir() || name.to_ascii_lowercase().ends_with(".zip"))
}

/// Packs in the folder, newest index data merged in. Files the player put in
/// the folder by hand are listed too.
pub fn list(paths: &Paths, kind: Kind) -> Result<Vec<ContentPack>> {
    let dir = kind.dir(paths);
    let mut index = load_index(paths);
    let mut names: Vec<String> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter(|e| is_pack(&e.path()))
            .filter_map(|e| e.file_name().into_string().ok())
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort_by_key(|n| n.to_lowercase());
    // Forget files that are gone.
    let before = index.list(kind).len();
    index.list(kind).retain(|e| names.contains(&e.file));
    if index.list(kind).len() != before {
        save_index(paths, &index)?;
    }
    let on = enabled_set(paths, kind);
    Ok(names
        .into_iter()
        .map(|file| {
            let e = index.list(kind).iter().find(|e| e.file == file).cloned();
            ContentPack {
                enabled: on.contains(&file),
                order: on.iter().position(|f| *f == file),
                title: e
                    .as_ref()
                    .map(|e| e.title.clone())
                    .unwrap_or_else(|| title_of(&file)),
                version: e.as_ref().map(|e| e.version.clone()).unwrap_or_default(),
                project: e.as_ref().and_then(|e| e.project.clone()),
                icon: e.and_then(|e| e.icon),
                file,
            }
        })
        .collect())
}

fn title_of(file: &str) -> String {
    let stem = file
        .strip_suffix(".zip")
        .or_else(|| file.strip_suffix(".ZIP"))
        .unwrap_or(file);
    stem.replace('_', " ")
}

/// Modrinth projects of the installed packs of this kind.
pub fn installed_projects(paths: &Paths, kind: Kind) -> Vec<String> {
    list(paths, kind)
        .map(|l| l.into_iter().filter_map(|p| p.project).collect())
        .unwrap_or_default()
}

fn enabled_set(paths: &Paths, kind: Kind) -> Vec<String> {
    match kind {
        Kind::Resource => options::resource_packs(paths)
            .into_iter()
            .filter_map(|p| p.strip_prefix("file/").map(str::to_string))
            .collect(),
        Kind::Shader => iris::active(paths).into_iter().collect(),
    }
}

/// Installs the newest version of a Modrinth resource/shader pack for `mc`
/// and turns it on (a shader replaces the one that was on). Returns its title.
pub async fn install_modrinth(
    client: &reqwest::Client,
    paths: &Paths,
    kind: Kind,
    project: &str,
    icon: Option<String>,
    mc: &str,
) -> Result<String> {
    let info = modrinth::projects(client, &[project.to_string()])
        .await?
        .into_iter()
        .next()
        .context("Проект не найден на Modrinth")?;
    anyhow::ensure!(
        info.project_type == kind.project_type(),
        "{} — это не {}",
        info.title,
        kind.noun().to_lowercase()
    );
    let version = modrinth::latest_content_version(client, &info.id, kind.loader(), mc)
        .await?
        .with_context(|| format!("У «{}» нет версии для Minecraft {mc}", info.title))?;
    let file = version.primary_zip().context("в версии нет .zip")?.clone();
    check_name(&file.filename)?;

    let dir = kind.dir(paths);
    std::fs::create_dir_all(&dir)?;
    http::fetch_all(
        client,
        vec![Download {
            url: file.url.clone(),
            path: dir.join(&file.filename),
            hash: match file.hashes.get("sha512") {
                Some(h) => Hash::Sha512(h.clone()),
                None => Hash::None,
            },
            size: Some(file.size),
            executable: false,
        }],
        1,
        &Reporter::silent(),
    )
    .await?;

    let mut index = load_index(paths);
    // An older file of the same project goes away, keeping its place if it was on.
    let old: Vec<String> = index
        .list(kind)
        .iter()
        .filter(|e| e.project.as_deref() == Some(info.id.as_str()) && e.file != file.filename)
        .map(|e| e.file.clone())
        .collect();
    for o in &old {
        if kind == Kind::Resource {
            options::rename_resource_pack(paths, o, &file.filename)?;
        }
        remove_path(&dir.join(o))?;
    }
    index
        .list(kind)
        .retain(|e| !old.contains(&e.file) && e.file != file.filename);
    index.list(kind).push(Entry {
        file: file.filename.clone(),
        title: info.title.clone(),
        version: version.version_number.clone(),
        project: Some(info.id.clone()),
        version_id: Some(version.id.clone()),
        icon,
    });
    save_index(paths, &index)?;
    set_enabled(paths, kind, &file.filename, true)?;
    Ok(info.title)
}

/// What a `.zip` holds: shaders have a `shaders/` folder, resource packs a
/// `pack.mcmeta` at the top.
pub fn detect(path: &Path) -> Option<Kind> {
    if path.is_dir() {
        if path.join("shaders").is_dir() {
            return Some(Kind::Shader);
        }
        return path.join("pack.mcmeta").is_file().then_some(Kind::Resource);
    }
    let file = std::fs::File::open(path).ok()?;
    let zip = zip::ZipArchive::new(file).ok()?;
    let mut resource = false;
    for name in zip.file_names() {
        if name.starts_with("shaders/") {
            return Some(Kind::Shader);
        }
        if name == "pack.mcmeta" {
            resource = true;
        }
    }
    resource.then_some(Kind::Resource)
}

/// `pack.mcmeta` description of a resource pack zip, for the title of a file
/// Modrinth doesn't know. Only plain-text descriptions are used.
fn zip_description(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let mut text = String::new();
    zip.by_name("pack.mcmeta")
        .ok()?
        .read_to_string(&mut text)
        .ok()?;
    let v: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    v["pack"]["description"].as_str().map(str::to_string)
}

/// Adds the player's own `.zip` (picked or dropped on the window), finds out
/// whether it is a shader or a resource pack, turns it on. Returns what it is
/// and its title.
pub async fn add_local(
    client: &reqwest::Client,
    paths: &Paths,
    src: &Path,
) -> Result<(Kind, String)> {
    let name = src
        .file_name()
        .and_then(|n| n.to_str())
        .context("плохое имя файла")?
        .to_string();
    check_name(&name)?;
    let kind = detect(src).with_context(|| format!("{name}: это не ресурспак и не шейдер"))?;
    let dir = kind.dir(paths);
    std::fs::create_dir_all(&dir)?;
    let dest = dir.join(&name);
    if src.is_dir() {
        bail!("{name}: перетащите .zip, а не папку");
    }
    if dest != src {
        std::fs::copy(src, &dest).with_context(|| format!("не удалось скопировать {name}"))?;
    }

    // Modrinth knows most packs by their hash: then it gets a real title and icon.
    let sha = {
        use sha2::{Digest, Sha512};
        let bytes = std::fs::read(&dest)?;
        hex::encode(Sha512::digest(&bytes))
    };
    let mut entry = Entry {
        file: name.clone(),
        title: title_of(&name),
        ..Default::default()
    };
    if let Ok(v) = modrinth::version_from_hash(client, &sha).await {
        if let Ok(Some(p)) = modrinth::projects(client, std::slice::from_ref(&v.project_id))
            .await
            .map(|p| p.into_iter().next())
        {
            entry.title = p.title;
        }
        entry.version = v.version_number;
        entry.project = Some(v.project_id);
        entry.version_id = Some(v.id);
    } else if kind == Kind::Resource {
        if let Some(d) = zip_description(&dest).filter(|d| !d.trim().is_empty() && d.len() < 60) {
            entry.version = d;
        }
    }
    let title = entry.title.clone();
    let mut index = load_index(paths);
    index.list(kind).retain(|e| e.file != name);
    index.list(kind).push(entry);
    save_index(paths, &index)?;
    set_enabled(paths, kind, &name, true)?;
    Ok((kind, title))
}

pub fn set_enabled(paths: &Paths, kind: Kind, file: &str, enabled: bool) -> Result<()> {
    check_name(file)?;
    anyhow::ensure!(kind.dir(paths).join(file).exists(), "нет файла {file}");
    match kind {
        Kind::Resource => options::set_resource_pack(paths, file, enabled),
        Kind::Shader => iris::set(paths, file, enabled),
    }
}

/// Moves a resource pack one step up (`up`, wins over more packs) or down
/// in the order of the enabled ones.
pub fn move_resource_pack(paths: &Paths, file: &str, up: bool) -> Result<()> {
    check_name(file)?;
    options::move_resource_pack(paths, file, up)
}

pub fn remove(paths: &Paths, kind: Kind, file: &str) -> Result<()> {
    check_name(file)?;
    let path = kind.dir(paths).join(file);
    if path.exists() {
        set_enabled(paths, kind, file, false)?;
        remove_path(&path)?;
    }
    let mut index = load_index(paths);
    index.list(kind).retain(|e| e.file != file);
    save_index(paths, &index)
}

fn remove_path(path: &Path) -> Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Newer Modrinth versions for every pack of this kind. Returns the titles
/// of the updated ones.
pub async fn update_all(
    client: &reqwest::Client,
    paths: &Paths,
    kind: Kind,
    mc: &str,
) -> Result<Vec<String>> {
    let index = load_index(paths);
    let entries = match kind {
        Kind::Resource => index.resource,
        Kind::Shader => index.shader,
    };
    let mut updated = Vec::new();
    for e in entries {
        let Some(project) = e.project.clone() else {
            continue;
        };
        let Ok(Some(v)) =
            modrinth::latest_content_version(client, &project, kind.loader(), mc).await
        else {
            continue;
        };
        if Some(&v.id) == e.version_id.as_ref() {
            continue;
        }
        let was_on = enabled_set(paths, kind).contains(&e.file);
        let title = install_modrinth(client, paths, kind, &project, e.icon.clone(), mc).await?;
        if !was_on {
            if let Some(f) = v.primary_zip() {
                set_enabled(paths, kind, &f.filename, false)?;
            }
        }
        updated.push(title);
    }
    Ok(updated)
}

/// `options.txt`: one `key:value` per line; `resourcePacks` is a JSON list
/// in load order (the last one is on top).
mod options {
    use super::*;

    const PACKS: &str = "resourcePacks";
    /// Packs the game would otherwise drop because their format is older or
    /// newer than the game's (the same list the «use anyway» button fills).
    const INCOMPATIBLE: &str = "incompatibleResourcePacks";

    fn path(paths: &Paths) -> PathBuf {
        paths.game_dir().join("options.txt")
    }

    fn read(paths: &Paths) -> Vec<String> {
        std::fs::read_to_string(path(paths))
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    fn get(lines: &[String], key: &str) -> Option<Vec<String>> {
        let prefix = format!("{key}:");
        lines
            .iter()
            .find_map(|l| l.strip_prefix(&prefix))
            .and_then(|v| serde_json::from_str(v).ok())
    }

    fn put(lines: &mut Vec<String>, key: &str, list: &[String]) {
        let prefix = format!("{key}:");
        let line = format!("{prefix}{}", serde_json::to_string(list).unwrap());
        match lines.iter_mut().find(|l| l.starts_with(&prefix)) {
            Some(l) => *l = line,
            None => lines.push(line),
        }
    }

    fn write(paths: &Paths, lines: &[String]) -> Result<()> {
        std::fs::create_dir_all(paths.game_dir())?;
        let mut text = lines.join("\n");
        text.push('\n');
        std::fs::write(path(paths), text).context("не удалось записать options.txt")
    }

    pub fn resource_packs(paths: &Paths) -> Vec<String> {
        get(&read(paths), PACKS).unwrap_or_default()
    }

    fn edit(paths: &Paths, f: impl FnOnce(&mut Vec<String>, &mut Vec<String>)) -> Result<()> {
        let mut lines = read(paths);
        let mut packs = get(&lines, PACKS).unwrap_or_else(|| vec!["vanilla".into()]);
        let mut incompatible = get(&lines, INCOMPATIBLE).unwrap_or_default();
        f(&mut packs, &mut incompatible);
        put(&mut lines, PACKS, &packs);
        put(&mut lines, INCOMPATIBLE, &incompatible);
        write(paths, &lines)
    }

    pub fn set_resource_pack(paths: &Paths, file: &str, enabled: bool) -> Result<()> {
        let id = format!("file/{file}");
        edit(paths, |packs, incompatible| {
            packs.retain(|p| p != &id);
            incompatible.retain(|p| p != &id);
            if enabled {
                // On top of everything turned on before.
                packs.push(id.clone());
                incompatible.push(id);
            }
        })
    }

    pub fn rename_resource_pack(paths: &Paths, from: &str, to: &str) -> Result<()> {
        let (from, to) = (format!("file/{from}"), format!("file/{to}"));
        if !resource_packs(paths).contains(&from) {
            return Ok(());
        }
        edit(paths, |packs, incompatible| {
            for p in packs.iter_mut().chain(incompatible.iter_mut()) {
                if *p == from {
                    *p = to.clone();
                }
            }
        })
    }

    pub fn move_resource_pack(paths: &Paths, file: &str, up: bool) -> Result<()> {
        let id = format!("file/{file}");
        edit(paths, |packs, _| {
            // Only the player's packs trade places; built-in ones stay put.
            let mine: Vec<usize> = packs
                .iter()
                .enumerate()
                .filter(|(_, p)| p.starts_with("file/"))
                .map(|(i, _)| i)
                .collect();
            let Some(pos) = mine.iter().position(|&i| packs[i] == id) else {
                return;
            };
            let other = if up {
                mine.get(pos + 1)
            } else {
                pos.checked_sub(1).and_then(|p| mine.get(p))
            };
            if let Some(&o) = other {
                packs.swap(mine[pos], o);
            }
        })
    }
}

/// `config/iris.properties` (Java properties).
mod iris {
    use super::*;

    fn path(paths: &Paths) -> PathBuf {
        paths.game_dir().join("config").join("iris.properties")
    }

    fn unescape(v: &str) -> String {
        let mut out = String::new();
        let mut chars = v.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    fn escape(v: &str) -> String {
        let mut out = String::new();
        for (i, c) in v.chars().enumerate() {
            if matches!(c, '\\' | '=' | ':' | '#' | '!') || (i == 0 && c == ' ') {
                out.push('\\');
            }
            out.push(c);
        }
        out
    }

    fn read(paths: &Paths) -> Vec<String> {
        std::fs::read_to_string(path(paths))
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    fn get(lines: &[String], key: &str) -> Option<String> {
        lines.iter().find_map(|l| {
            let l = l.trim_start();
            let rest = l.strip_prefix(key)?;
            let rest = rest.trim_start();
            let rest = rest.strip_prefix('=').or_else(|| rest.strip_prefix(':'))?;
            Some(unescape(rest.trim_start()))
        })
    }

    fn put(lines: &mut Vec<String>, key: &str, value: &str) {
        let line = format!("{key}={}", escape(value));
        let found = lines.iter_mut().find(|l| {
            let l = l.trim_start();
            l.strip_prefix(key)
                .map(|r| r.trim_start().starts_with(['=', ':']))
                .unwrap_or(false)
        });
        match found {
            Some(l) => *l = line,
            None => lines.push(line),
        }
    }

    /// The shader that is on, if any.
    pub fn active(paths: &Paths) -> Option<String> {
        let lines = read(paths);
        let on = get(&lines, "enableShaders")
            .map(|v| v == "true")
            .unwrap_or(false);
        get(&lines, "shaderPack").filter(|p| on && !p.is_empty())
    }

    pub fn set(paths: &Paths, file: &str, enabled: bool) -> Result<()> {
        let mut lines = read(paths);
        if enabled {
            put(&mut lines, "shaderPack", file);
            put(&mut lines, "enableShaders", "true");
        } else if get(&lines, "shaderPack").as_deref() == Some(file) {
            put(&mut lines, "enableShaders", "false");
        } else {
            return Ok(());
        }
        let p = path(paths);
        std::fs::create_dir_all(p.parent().unwrap())?;
        let mut text = lines.join("\n");
        text.push('\n');
        std::fs::write(p, text).context("не удалось записать настройки Iris")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> Paths {
        let dir =
            std::env::temp_dir().join(format!("mh-packs-{}-{}", std::process::id(), rand_suffix()));
        let _ = std::fs::remove_dir_all(&dir);
        Paths::new(dir)
    }

    fn rand_suffix() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::Relaxed)
    }

    fn touch(paths: &Paths, kind: Kind, name: &str) {
        let dir = kind.dir(paths);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), b"x").unwrap();
    }

    #[test]
    fn resource_packs_go_to_options_txt() {
        let p = temp();
        std::fs::create_dir_all(p.game_dir()).unwrap();
        std::fs::write(
            p.game_dir().join("options.txt"),
            "version:4000\nresourcePacks:[\"vanilla\",\"fabric\"]\nlang:ru_ru\n",
        )
        .unwrap();
        touch(&p, Kind::Resource, "A.zip");
        touch(&p, Kind::Resource, "B.zip");
        set_enabled(&p, Kind::Resource, "A.zip", true).unwrap();
        set_enabled(&p, Kind::Resource, "B.zip", true).unwrap();
        assert_eq!(
            options::resource_packs(&p),
            ["vanilla", "fabric", "file/A.zip", "file/B.zip"]
        );
        move_resource_pack(&p, "A.zip", true).unwrap();
        assert_eq!(
            options::resource_packs(&p),
            ["vanilla", "fabric", "file/B.zip", "file/A.zip"]
        );
        set_enabled(&p, Kind::Resource, "B.zip", false).unwrap();
        let text = std::fs::read_to_string(p.game_dir().join("options.txt")).unwrap();
        assert!(text.starts_with("version:4000\n"));
        assert!(text.contains("lang:ru_ru"));
        assert!(text.contains("incompatibleResourcePacks:[\"file/A.zip\"]"));
        let l = list(&p, Kind::Resource).unwrap();
        assert_eq!(l.len(), 2);
        assert!(l[0].enabled && !l[1].enabled);
        assert_eq!(l[0].order, Some(0));
    }

    #[test]
    fn options_txt_is_created_with_vanilla() {
        let p = temp();
        touch(&p, Kind::Resource, "A.zip");
        set_enabled(&p, Kind::Resource, "A.zip", true).unwrap();
        assert_eq!(options::resource_packs(&p), ["vanilla", "file/A.zip"]);
    }

    #[test]
    fn one_shader_at_a_time() {
        let p = temp();
        touch(&p, Kind::Shader, "BSL v8.zip");
        touch(&p, Kind::Shader, "Comp:lementary.zip");
        let cfg = p.game_dir().join("config");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(
            cfg.join("iris.properties"),
            "#Iris\nmaxShadowRenderDistance=32\nenableShaders=false\n",
        )
        .unwrap();
        set_enabled(&p, Kind::Shader, "BSL v8.zip", true).unwrap();
        assert_eq!(iris::active(&p).as_deref(), Some("BSL v8.zip"));
        set_enabled(&p, Kind::Shader, "Comp:lementary.zip", true).unwrap();
        assert_eq!(iris::active(&p).as_deref(), Some("Comp:lementary.zip"));
        let text = std::fs::read_to_string(cfg.join("iris.properties")).unwrap();
        assert!(text.contains("shaderPack=Comp\\:lementary.zip"));
        assert!(text.contains("maxShadowRenderDistance=32"));
        // Turning off a shader that isn't on changes nothing.
        set_enabled(&p, Kind::Shader, "BSL v8.zip", false).unwrap();
        assert!(iris::active(&p).is_some());
        set_enabled(&p, Kind::Shader, "Comp:lementary.zip", false).unwrap();
        assert_eq!(iris::active(&p), None);
        let l = list(&p, Kind::Shader).unwrap();
        assert!(l.iter().all(|s| !s.enabled));
    }

    #[test]
    fn detects_zip_kind() {
        let p = temp();
        std::fs::create_dir_all(&p.root).unwrap();
        let mk = |name: &str, entries: &[&str]| {
            let path = p.root.join(name);
            let mut w = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            for e in entries {
                w.start_file(*e, zip::write::SimpleFileOptions::default())
                    .unwrap();
                std::io::Write::write_all(&mut w, b"{}").unwrap();
            }
            w.finish().unwrap();
            path
        };
        assert_eq!(
            detect(&mk("s.zip", &["shaders/final.fsh"])),
            Some(Kind::Shader)
        );
        assert_eq!(
            detect(&mk("r.zip", &["pack.mcmeta", "assets/x"])),
            Some(Kind::Resource)
        );
        assert_eq!(detect(&mk("n.zip", &["readme.txt"])), None);
    }

    #[test]
    fn remove_turns_off_and_deletes() {
        let p = temp();
        touch(&p, Kind::Resource, "A.zip");
        set_enabled(&p, Kind::Resource, "A.zip", true).unwrap();
        remove(&p, Kind::Resource, "A.zip").unwrap();
        assert!(!Kind::Resource.dir(&p).join("A.zip").exists());
        assert_eq!(options::resource_packs(&p), ["vanilla"]);
        assert!(remove(&p, Kind::Resource, "../x").is_err());
    }
}
