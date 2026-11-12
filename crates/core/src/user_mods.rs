//! Mods the player adds on top of the MineHouse pack: from Modrinth or their
//! own `.jar` files.
//!
//! They live in the same `mods/` folder (that is where Fabric looks), but the
//! pack updater only ever touches files listed in the site's manifest, so
//! these survive every pack update. A disabled mod is renamed to
//! `<name>.jar.disabled`, which Fabric skips. What the launcher knows about
//! each file (Modrinth project, version, who pulled it in as a dependency) is
//! kept in `user-mods.json` next to the instance.
//!
//! The server only lets a client in when shared mods match, so before a mod
//! goes in it is checked against the pack and the server:
//! - already in the pack → refused (two copies crash Fabric);
//! - also on the server (`server_mods` in the manifest) → exactly the
//!   server's version is installed (an old client FSit once broke joining);
//! - server-only, or needs a server part MineHouse doesn't have → refused.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::http::{self, Download, Hash};
use crate::modpack::Manifest;
use crate::modrinth::{self, Version};
use crate::paths::Paths;
use crate::progress::Reporter;

const DISABLED: &str = ".disabled";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UserMod {
    /// File name in `mods/` without the `.disabled` suffix.
    pub file: String,
    pub enabled: bool,
    pub title: String,
    #[serde(default)]
    pub version: String,
    /// Modrinth project id; `None` for a jar Modrinth doesn't know.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub version_id: Option<String>,
    /// Minecraft version the installed file was picked for.
    #[serde(default)]
    pub game_version: Option<String>,
    /// Fabric mod id from `fabric.mod.json`.
    #[serde(default)]
    pub mod_id: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    /// Installed only because these projects need it; removed with the last of them.
    #[serde(default)]
    pub required_by: Vec<String>,
    /// Why the launcher turned the mod off on its own.
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    mods: Vec<UserMod>,
}

/// What a Modrinth project may do in this instance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "version", rename_all = "snake_case")]
pub enum Verdict {
    Ok,
    /// On the server too: this exact version.
    Server(String),
    InPack,
    ServerOnly,
    NeedsServer,
}

impl Verdict {
    pub fn allowed(&self) -> bool {
        matches!(self, Self::Ok | Self::Server(_))
    }
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Ok | Self::Server(_) => "",
            Self::InPack => "Уже есть в сборке MineHouse",
            Self::ServerOnly => "Мод только для сервера",
            Self::NeedsServer => "Нужен и на сервере, а на MineHouse его нет",
        }
    }
}

/// The pack and server facts every check needs, from the site's manifest.
#[derive(Clone, Debug, Default)]
pub struct Pack {
    pub minecraft: String,
    /// File names the pack owns in `mods/`.
    pub files: HashSet<String>,
    /// Modrinth projects in the pack.
    pub projects: HashSet<String>,
    /// Modrinth project → version installed on the server.
    pub server: HashMap<String, String>,
    /// Fabric id of a server mod → its Modrinth version.
    pub server_ids: HashMap<String, String>,
    /// Optional pack mods and the player's choice.
    pub options: Vec<PackChoice>,
}

/// An optional pack mod as the «Моды» window shows it.
#[derive(Clone, Debug, Serialize)]
pub struct PackChoice {
    pub id: String,
    pub name: String,
    pub why: Option<String>,
    /// The player wants it (not turned off).
    pub enabled: bool,
    /// Names of turned-off options it can't run without.
    pub missing: Vec<String>,
    /// Modrinth project of its file.
    #[serde(skip)]
    pub project: Option<String>,
}

impl Pack {
    /// Everything in the manifest (as if no optional mod were off).
    pub fn from_manifest(m: &Manifest) -> Self {
        Self::chosen(m, &BTreeSet::new())
    }

    /// The pack with the player's choice: turned-off mods are not part of it,
    /// so the player may install their own version.
    pub fn load(m: &Manifest, paths: &Paths) -> Self {
        Self::chosen(m, &crate::modpack::disabled(paths))
    }

    pub fn chosen(m: &Manifest, disabled: &BTreeSet<String>) -> Self {
        let mut pack = Pack {
            minecraft: m
                .minecraft
                .clone()
                .unwrap_or_else(|| crate::DEFAULT_MINECRAFT.into()),
            ..Default::default()
        };
        let on = m.enabled(disabled);
        let names: HashMap<&str, &str> = m
            .options
            .iter()
            .map(|o| (o.id.as_str(), o.name.as_str()))
            .collect();
        pack.options = m
            .options
            .iter()
            .map(|o| PackChoice {
                id: o.id.clone(),
                name: o.name.clone(),
                why: o.why.clone(),
                enabled: !disabled.contains(&o.id),
                missing: o
                    .requires
                    .iter()
                    .filter(|r| !on.contains(*r))
                    .map(|r| names.get(r.as_str()).copied().unwrap_or(r).to_string())
                    .collect(),
                project: m.main_file(&o.id).and_then(|f| modrinth_project_of(&f.url)),
            })
            .collect();
        for f in m.wanted(disabled) {
            if let Some(name) = f.path.strip_prefix("mods/") {
                pack.files.insert(name.to_string());
            }
            if let Some(id) = modrinth_project_of(&f.url) {
                pack.projects.insert(id);
            }
        }
        for s in &m.server_mods {
            pack.server.insert(s.project.clone(), s.version.clone());
            if let Some(id) = &s.mod_id {
                pack.server_ids.insert(id.clone(), s.version.clone());
            }
        }
        pack
    }

    pub fn verdict(&self, project: &str, client_side: &str, server_side: &str) -> Verdict {
        if self.projects.contains(project) {
            Verdict::InPack
        } else if let Some(v) = self.server.get(project) {
            Verdict::Server(v.clone())
        } else if client_side == "unsupported" {
            Verdict::ServerOnly
        } else if server_side == "required" {
            Verdict::NeedsServer
        } else {
            Verdict::Ok
        }
    }

    /// [`Self::verdict`] for a mod that is pulled in as a dependency.
    /// Libraries are often tagged "server: required" just because they can
    /// run there; whether a server part is needed is up to the mod that
    /// pulls them in, which is checked on its own.
    pub fn dependency_verdict(
        &self,
        project: &str,
        client_side: &str,
        server_side: &str,
    ) -> Verdict {
        match self.verdict(project, client_side, server_side) {
            Verdict::NeedsServer => Verdict::Ok,
            v => v,
        }
    }

    /// The verdict for a mod the player has, taking into account whether
    /// another mod pulled it in.
    fn verdict_for(&self, m: &UserMod, info: &modrinth::Project) -> Verdict {
        if m.required_by.is_empty() {
            self.verdict(&info.id, &info.client_side, &info.server_side)
        } else {
            self.dependency_verdict(&info.id, &info.client_side, &info.server_side)
        }
    }

    /// A jar with the Fabric id of a server mod but not the server's file
    /// (e.g. the original FSit next to the server's FSit Continued).
    pub fn clashes_with_server(&self, mod_id: Option<&str>, version_id: Option<&str>) -> bool {
        match mod_id.and_then(|id| self.server_ids.get(id)) {
            Some(want) => version_id != Some(want.as_str()),
            None => false,
        }
    }
}

const SERVER_CLASH: &str =
    "на сервере стоит другая версия этого мода, найдите его в поиске Modrinth";

/// `https://cdn.modrinth.com/data/<project>/versions/...` → project id.
fn modrinth_project_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://cdn.modrinth.com/data/")?;
    let id = rest.split('/').next()?;
    (!id.is_empty()).then(|| id.to_string())
}

fn index_path(paths: &Paths) -> PathBuf {
    paths.instance_meta().join("user-mods.json")
}

fn mods_dir(paths: &Paths) -> PathBuf {
    paths.game_dir().join("mods")
}

fn file_path(paths: &Paths, m: &UserMod) -> PathBuf {
    let mut name = m.file.clone();
    if !m.enabled {
        name.push_str(DISABLED);
    }
    mods_dir(paths).join(name)
}

fn load(paths: &Paths) -> Index {
    std::fs::read(index_path(paths))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save(paths: &Paths, index: &Index) -> Result<()> {
    std::fs::create_dir_all(paths.instance_meta())?;
    let tmp = index_path(paths).with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(index)?)?;
    std::fs::rename(tmp, index_path(paths))?;
    Ok(())
}

/// A plain file name: no folders, so a bad name can't point outside `mods/`.
fn check_file_name(name: &str) -> Result<()> {
    anyhow::ensure!(
        !name.is_empty()
            && !name.contains(['/', '\\', ':'])
            && name != "."
            && name != ".."
            && name.ends_with(".jar"),
        "плохое имя файла: {name}"
    );
    Ok(())
}

#[derive(Debug, Default, Clone)]
pub struct JarInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub provides: Vec<String>,
}

/// `fabric.mod.json` of a jar; `None` if it isn't a Fabric mod.
pub fn read_jar(path: &Path) -> Option<JarInfo> {
    #[derive(Deserialize)]
    struct FabricMod {
        id: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        version: Option<String>,
        #[serde(default)]
        provides: Vec<String>,
    }
    let file = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let mut entry = zip.by_name("fabric.mod.json").ok()?;
    let mut text = String::new();
    entry.read_to_string(&mut text).ok()?;
    // Some mods put raw newlines/tabs inside strings, which Fabric tolerates.
    let text = text.replace(['\n', '\r', '\t'], " ");
    let m: FabricMod = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    Some(JarInfo {
        name: m.name.unwrap_or_else(|| m.id.clone()),
        version: m.version.unwrap_or_default(),
        id: m.id,
        provides: m.provides,
    })
}

/// Fabric ids (and `provides`) of the pack's jars as installed.
fn pack_mod_ids(paths: &Paths, pack: &Pack) -> HashSet<String> {
    let dir = mods_dir(paths);
    let mut ids = HashSet::new();
    for name in &pack.files {
        if let Some(info) = read_jar(&dir.join(name)) {
            ids.insert(info.id);
            ids.extend(info.provides);
        }
    }
    ids
}

fn sha512_file(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha512};
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha512::new();
    std::io::copy(&mut f, &mut h)?;
    Ok(hex::encode(h.finalize()))
}

/// The index matched against what is really in `mods/`: entries whose file
/// is gone are dropped, jars the player put there by hand are added.
fn reconcile(paths: &Paths, pack: &Pack) -> Result<Index> {
    let mut index = load(paths);
    let dir = mods_dir(paths);
    std::fs::create_dir_all(&dir)?;

    index.mods.retain(|m| {
        if pack.files.contains(&m.file) {
            return false;
        }
        if file_path(paths, m).exists() {
            return true;
        }
        // Renamed by hand between .jar and .jar.disabled.
        let other = UserMod {
            enabled: !m.enabled,
            ..m.clone()
        };
        file_path(paths, &other).exists()
    });
    for m in &mut index.mods {
        if !file_path(paths, m).exists() {
            m.enabled = !m.enabled;
        }
    }

    let known: HashSet<String> = index.mods.iter().map(|m| m.file.clone()).collect();
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let (file, enabled) = match name.strip_suffix(DISABLED) {
            Some(f) => (f.to_string(), false),
            None => (name.clone(), true),
        };
        if !file.ends_with(".jar") || pack.files.contains(&file) || known.contains(&file) {
            continue;
        }
        let info = read_jar(&entry.path());
        found.push(UserMod {
            title: info
                .as_ref()
                .map(|i| i.name.clone())
                .unwrap_or_else(|| file.clone()),
            version: info.as_ref().map(|i| i.version.clone()).unwrap_or_default(),
            mod_id: info.map(|i| i.id),
            file,
            enabled,
            ..Default::default()
        });
    }
    found.sort_by(|a, b| a.file.cmp(&b.file));
    index.mods.extend(found);
    save(paths, &index)?;
    Ok(index)
}

#[derive(Debug, Serialize)]
pub struct Listing {
    pub minecraft: String,
    pub mods: Vec<UserMod>,
    /// File names of the pack's mods as installed, for the read-only list.
    pub pack: Vec<String>,
    /// Optional pack mods with on/off switches.
    pub options: Vec<PackChoice>,
}

pub fn list(paths: &Paths, pack: &Pack) -> Result<Listing> {
    let index = reconcile(paths, pack)?;
    let mut files: Vec<String> = pack.files.iter().cloned().collect();
    files.sort_by_key(|f| f.to_lowercase());
    Ok(Listing {
        minecraft: pack.minecraft.clone(),
        mods: index.mods,
        pack: files,
        options: pack.options.clone(),
    })
}

/// One Modrinth version to download, and why.
struct Planned {
    version: Version,
    title: String,
    icon: Option<String>,
    required_by: Option<String>,
}

/// The version to use for `project`: the server's if the server has it,
/// otherwise the newest for this Minecraft.
async fn pick_version(
    client: &reqwest::Client,
    pack: &Pack,
    project: &str,
    verdict: &Verdict,
) -> Result<Version> {
    if let Verdict::Server(v) = verdict {
        return modrinth::version(client, v).await;
    }
    modrinth::latest_version(client, project, &pack.minecraft)
        .await?
        .with_context(|| format!("нет версии для Fabric {}", pack.minecraft))
}

/// `project` and every required dependency it still lacks.
async fn plan(
    client: &reqwest::Client,
    pack: &Pack,
    index: &Index,
    project: &str,
) -> Result<Vec<Planned>> {
    let mut out: Vec<Planned> = Vec::new();
    let mut queue: Vec<(String, Option<String>)> = vec![(project.to_string(), None)];
    let mut seen: HashSet<String> = HashSet::new();
    while let Some((id, parent)) = queue.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let info = modrinth::projects(client, std::slice::from_ref(&id))
            .await?
            .into_iter()
            .next()
            .with_context(|| format!("проект {id} не найден на Modrinth"))?;
        let verdict = if parent.is_some() {
            pack.dependency_verdict(&info.id, &info.client_side, &info.server_side)
        } else {
            pack.verdict(&info.id, &info.client_side, &info.server_side)
        };
        if parent.is_some()
            && (verdict == Verdict::InPack
                || index
                    .mods
                    .iter()
                    .any(|m| m.project.as_deref() == Some(&info.id)))
        {
            continue; // the dependency is already here
        }
        if !verdict.allowed() {
            match &parent {
                None => bail!("{}: {}", info.title, verdict.reason()),
                Some(_) => bail!(
                    "нужен мод {}, но его нельзя поставить: {}",
                    info.title,
                    verdict.reason()
                ),
            }
        }
        if info.project_type != "mod" && !info.project_type.is_empty() {
            bail!("{} — это не мод ({})", info.title, info.project_type);
        }
        let version = pick_version(client, pack, &info.id, &verdict).await?;
        for dep in &version.dependencies {
            if dep.dependency_type != "required" {
                continue;
            }
            if let Some(p) = &dep.project_id {
                queue.push((p.clone(), Some(info.id.clone())));
            }
        }
        out.push(Planned {
            version,
            title: info.title,
            icon: None,
            required_by: parent,
        });
    }
    Ok(out)
}

/// Downloads planned versions into `mods/` and records them.
async fn apply(
    client: &reqwest::Client,
    paths: &Paths,
    pack: &Pack,
    index: &mut Index,
    planned: Vec<Planned>,
    reporter: &Reporter,
) -> Result<Vec<String>> {
    let dir = mods_dir(paths);
    let mut downloads = Vec::new();
    let mut entries = Vec::new();
    for p in planned {
        let f = p.version.primary_file().context("у версии нет jar")?;
        check_file_name(&f.filename)?;
        if pack.files.contains(&f.filename) {
            bail!("{} совпадает с файлом сборки", f.filename);
        }
        downloads.push(Download {
            url: f.url.clone(),
            path: dir.join(&f.filename),
            hash: match f.hashes.get("sha512") {
                Some(h) => Hash::Sha512(h.clone()),
                None => Hash::None,
            },
            size: Some(f.size),
            executable: false,
        });
        entries.push(UserMod {
            file: f.filename.clone(),
            enabled: true,
            title: p.title,
            version: p.version.version_number.clone(),
            project: Some(p.version.project_id.clone()),
            version_id: Some(p.version.id.clone()),
            game_version: Some(pack.minecraft.clone()),
            mod_id: None,
            icon: p.icon,
            required_by: p.required_by.into_iter().collect(),
            note: None,
        });
    }
    http::fetch_all(client, downloads, 4, reporter).await?;

    for e in &mut entries {
        e.mod_id = read_jar(&dir.join(&e.file)).map(|i| i.id);
    }
    if let Some(bad) = entries
        .iter()
        .find(|e| pack.clashes_with_server(e.mod_id.as_deref(), e.version_id.as_deref()))
    {
        let msg = format!("{}: {SERVER_CLASH}", bad.title);
        for e in &entries {
            if !index.mods.iter().any(|m| m.file == e.file) {
                let _ = std::fs::remove_file(dir.join(&e.file));
            }
        }
        bail!(msg);
    }
    let mut titles = Vec::new();
    for mut e in entries {
        titles.push(e.title.clone());
        // Replace an older file of the same project.
        if let Some(old) = index
            .mods
            .iter()
            .position(|m| m.project.is_some() && m.project == e.project)
        {
            let old = index.mods.remove(old);
            e.required_by = old.required_by.clone();
            if old.file != e.file || !old.enabled {
                let _ = std::fs::remove_file(file_path(paths, &old));
            }
            // An update keeps the mod off if the player turned it off.
            if !old.enabled {
                let from = file_path(paths, &e);
                e.enabled = false;
                if std::fs::rename(&from, file_path(paths, &e)).is_err() {
                    e.enabled = true;
                }
            }
        }
        index.mods.retain(|m| m.file != e.file);
        index.mods.push(e);
    }
    Ok(titles)
}

/// Installs a Modrinth project and its required dependencies. Returns the titles installed.
pub async fn install_modrinth(
    client: &reqwest::Client,
    paths: &Paths,
    pack: &Pack,
    project: &str,
    icon: Option<String>,
) -> Result<Vec<String>> {
    let mut index = reconcile(paths, pack)?;
    let mut planned = plan(client, pack, &index, project).await?;
    if let Some(first) = planned.iter_mut().find(|p| p.required_by.is_none()) {
        first.icon = icon;
    }
    let titles = apply(
        client,
        paths,
        pack,
        &mut index,
        planned,
        &Reporter::silent(),
    )
    .await?;
    save(paths, &index)?;
    Ok(titles)
}

/// Copies the player's own jar into `mods/`. If Modrinth knows the file,
/// it is checked like a Modrinth install and can be updated later.
pub async fn add_local(
    client: &reqwest::Client,
    paths: &Paths,
    pack: &Pack,
    src: &Path,
) -> Result<String> {
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .context("нет имени файла")?;
    anyhow::ensure!(name.ends_with(".jar"), "{name}: нужен файл .jar");
    check_file_name(&name)?;
    let info = read_jar(src).with_context(|| format!("{name}: это не мод Fabric"))?;
    if pack.files.contains(&name) {
        bail!("{name}: этот файл уже есть в сборке");
    }
    if pack_mod_ids(paths, pack).contains(&info.id) {
        bail!("{}: этот мод уже есть в сборке MineHouse", info.name);
    }

    let mut entry = UserMod {
        file: name.clone(),
        enabled: true,
        title: info.name.clone(),
        version: info.version.clone(),
        mod_id: Some(info.id.clone()),
        ..Default::default()
    };
    // Best effort: a jar Modrinth knows gets the same server checks.
    if let Ok(hash) = sha512_file(src) {
        if let Ok(v) = modrinth::version_from_hash(client, &hash).await {
            if let Some(p) = modrinth::projects(client, std::slice::from_ref(&v.project_id))
                .await
                .ok()
                .and_then(|p| p.into_iter().next())
            {
                let verdict = pack.verdict(&p.id, &p.client_side, &p.server_side);
                match &verdict {
                    Verdict::Server(want) if *want != v.id => bail!(
                        "{}: на сервере другая версия этого мода, поставьте его через поиск Modrinth",
                        p.title
                    ),
                    v2 if !v2.allowed() => bail!("{}: {}", p.title, v2.reason()),
                    _ => {}
                }
                entry.title = p.title;
                entry.project = Some(p.id);
                entry.version_id = Some(v.id);
                entry.version = v.version_number;
                entry.game_version = v
                    .game_versions
                    .contains(&pack.minecraft)
                    .then(|| pack.minecraft.clone());
            }
        }
    }

    if pack.clashes_with_server(Some(&info.id), entry.version_id.as_deref()) {
        bail!("{}: {SERVER_CLASH}", entry.title);
    }
    let mut index = reconcile(paths, pack)?;
    if index.mods.iter().any(|m| m.file == name) {
        bail!("{name}: такой файл уже добавлен");
    }
    if let Some(p) = &entry.project {
        if index.mods.iter().any(|m| m.project.as_ref() == Some(p)) {
            bail!("{}: этот мод уже установлен", entry.title);
        }
    }
    let dest = mods_dir(paths).join(&name);
    std::fs::copy(src, &dest).with_context(|| format!("копирование {name}"))?;
    let title = entry.title.clone();
    index.mods.push(entry);
    save(paths, &index)?;
    Ok(title)
}

pub fn set_enabled(paths: &Paths, pack: &Pack, file: &str, enabled: bool) -> Result<()> {
    let mut index = reconcile(paths, pack)?;
    let m = index
        .mods
        .iter_mut()
        .find(|m| m.file == file)
        .context("мод не найден")?;
    if m.enabled == enabled {
        return Ok(());
    }
    if enabled {
        if let Some(id) = &m.mod_id {
            if pack_mod_ids(paths, pack).contains(id) {
                bail!("{}: этот мод уже есть в сборке MineHouse", m.title);
            }
        }
        if pack.clashes_with_server(m.mod_id.as_deref(), m.version_id.as_deref()) {
            bail!("{}: {SERVER_CLASH}", m.title);
        }
    }
    let from = file_path(paths, m);
    m.enabled = enabled;
    m.note = None;
    std::fs::rename(&from, file_path(paths, m))?;
    save(paths, &index)
}

/// Deletes a mod, and the dependencies nothing else needs any more.
pub fn remove(paths: &Paths, pack: &Pack, file: &str) -> Result<()> {
    let mut index = reconcile(paths, pack)?;
    let pos = index
        .mods
        .iter()
        .position(|m| m.file == file)
        .context("мод не найден")?;
    let m = index.mods.remove(pos);
    std::fs::remove_file(file_path(paths, &m)).with_context(|| format!("удаление {file}"))?;
    let mut gone: Vec<String> = m.project.into_iter().collect();
    while let Some(project) = gone.pop() {
        let mut i = 0;
        while i < index.mods.len() {
            let d = &mut index.mods[i];
            let was_dep = !d.required_by.is_empty();
            d.required_by.retain(|p| *p != project);
            if was_dep && d.required_by.is_empty() {
                let d = index.mods.remove(i);
                let _ = std::fs::remove_file(file_path(paths, &d));
                gone.extend(d.project);
            } else {
                i += 1;
            }
        }
    }
    save(paths, &index)
}

/// Moves every Modrinth mod to the version it should have now (the server's,
/// or the newest for this Minecraft). Returns the titles updated.
pub async fn update_all(
    client: &reqwest::Client,
    paths: &Paths,
    pack: &Pack,
) -> Result<Vec<String>> {
    let mut index = reconcile(paths, pack)?;
    let projects: Vec<String> = index
        .mods
        .iter()
        .filter_map(|m| m.project.clone())
        .collect();
    let infos = modrinth::projects(client, &projects).await?;
    let mut planned = Vec::new();
    for info in infos {
        let Some(m) = index
            .mods
            .iter()
            .find(|m| m.project.as_deref() == Some(&info.id))
        else {
            continue;
        };
        let verdict = pack.verdict_for(m, &info);
        if !verdict.allowed() {
            continue;
        }
        let Ok(v) = pick_version(client, pack, &info.id, &verdict).await else {
            continue;
        };
        if m.version_id.as_deref() != Some(&v.id) {
            planned.push(Planned {
                version: v,
                title: m.title.clone(),
                icon: m.icon.clone(),
                required_by: None,
            });
        }
    }
    if planned.is_empty() {
        return Ok(Vec::new());
    }
    let titles = apply(
        client,
        paths,
        pack,
        &mut index,
        planned,
        &Reporter::silent(),
    )
    .await;
    save(paths, &index)?;
    titles
}

/// Modrinth projects of the mods that are on.
fn enabled_projects(index: &Index) -> HashSet<String> {
    index
        .mods
        .iter()
        .filter(|m| m.enabled)
        .filter_map(|m| m.project.clone())
        .collect()
}

/// A dependency of a mod that is on. The launcher never turns these off by
/// itself: the mod that pulled it in decides whether it is needed.
fn needed(m: &UserMod, enabled: &HashSet<String>) -> bool {
    m.required_by.iter().any(|p| enabled.contains(p))
}

/// Turns back on the dependencies the launcher turned off by itself while
/// the mod that needs them is on (older launchers did that). Ones the
/// player turned off by hand (no note) stay off, and so do ones the pack
/// already has: a second copy would crash the game, and the pack's copy
/// serves the mod that needs it.
fn restore_needed(
    paths: &Paths,
    index: &mut Index,
    pack_ids: &HashSet<String>,
    reporter: &Reporter,
) {
    loop {
        let enabled = enabled_projects(index);
        let mut changed = false;
        for m in index.mods.iter_mut() {
            if m.enabled
                || m.note.is_none()
                || !needed(m, &enabled)
                || m.mod_id.as_ref().is_some_and(|id| pack_ids.contains(id))
            {
                continue;
            }
            let from = file_path(paths, m);
            m.enabled = true;
            if std::fs::rename(&from, file_path(paths, m)).is_err() {
                m.enabled = false;
                continue;
            }
            m.note = None;
            reporter.log(format!(
                "Мод «{}» снова включён: он нужен другому моду",
                m.title
            ));
            changed = true;
        }
        if !changed {
            return;
        }
    }
}

fn disable(paths: &Paths, m: &mut UserMod, note: String, reporter: &Reporter) {
    let from = file_path(paths, m);
    m.enabled = false;
    if std::fs::rename(&from, file_path(paths, m)).is_err() {
        m.enabled = true;
        return;
    }
    reporter.log(format!("Мод «{}» выключен: {note}", m.title));
    m.note = Some(note);
}

/// Turns an optional pack mod on or off and applies it right away: downloads
/// what is now needed, removes what is not (the game must not be running).
pub async fn set_pack_option(
    client: &reqwest::Client,
    paths: &Paths,
    manifest: &Manifest,
    id: &str,
    enabled: bool,
    reporter: &Reporter,
) -> Result<()> {
    let Some(option) = manifest.options.iter().find(|o| o.id == id) else {
        bail!("в сборке нет необязательного мода {id}");
    };
    if enabled {
        let pack = Pack::load(manifest, paths);
        let project = pack
            .options
            .iter()
            .find(|o| o.id == id)
            .and_then(|o| o.project.clone());
        let index = reconcile(paths, &pack)?;
        if let Some(own) = index
            .mods
            .iter()
            .find(|m| m.enabled && project.is_some() && m.project == project)
        {
            bail!(
                "Сначала выключите или удалите свою версию «{}», иначе будет два одинаковых мода",
                own.title
            );
        }
    }
    let mut disabled = crate::modpack::disabled(paths);
    if enabled {
        disabled.remove(id);
    } else {
        disabled.insert(id.to_string());
    }
    crate::modpack::set_disabled(paths, disabled)?;
    crate::modpack::install(client, paths, manifest, reporter).await?;
    reporter.log(format!(
        "{} «{}»",
        if enabled {
            "Включён мод сборки"
        } else {
            "Выключен мод сборки"
        },
        option.name
    ));
    Ok(())
}

/// Runs before every launch, after the pack is installed: turns off player
/// mods that would now clash with the pack or the server and moves Modrinth
/// mods to the server's version or to a new Minecraft. Never fails the launch.
pub async fn sync(
    client: &reqwest::Client,
    paths: &Paths,
    manifest: &Manifest,
    reporter: &Reporter,
) {
    if let Err(e) = sync_inner(client, paths, manifest, reporter).await {
        reporter.log(format!("Свои моды не проверены: {e:#}"));
    }
}

async fn sync_inner(
    client: &reqwest::Client,
    paths: &Paths,
    manifest: &Manifest,
    reporter: &Reporter,
) -> Result<()> {
    let pack = Pack::load(manifest, paths);
    let mut index = reconcile(paths, &pack)?;
    let ids = pack_mod_ids(paths, &pack);
    restore_needed(paths, &mut index, &ids, reporter);
    if !index.mods.iter().any(|m| m.enabled) {
        return Ok(());
    }
    let enabled = enabled_projects(&index);

    // Modrinth mods: pack/server rules and the Minecraft version.
    let projects: Vec<String> = index
        .mods
        .iter()
        .filter(|m| m.enabled)
        .filter_map(|m| m.project.clone())
        .collect();
    let infos = match modrinth::projects(client, &projects).await {
        Ok(i) => i,
        Err(e) => {
            reporter.log(format!(
                "Modrinth недоступен, свои моды не обновлены: {e:#}"
            ));
            Vec::new()
        }
    };
    let mut planned = Vec::new();
    for info in infos {
        let Some(m) = index
            .mods
            .iter_mut()
            .find(|m| m.enabled && m.project.as_deref() == Some(&info.id))
        else {
            continue;
        };
        let verdict = pack.verdict_for(m, &info);
        let is_dep = needed(m, &enabled);
        if !verdict.allowed() {
            if !is_dep {
                disable(paths, m, verdict.reason().to_string(), reporter);
            }
            continue;
        }
        let stale = match &verdict {
            Verdict::Server(v) => m.version_id.as_deref() != Some(v),
            _ => m.game_version.as_deref() != Some(&pack.minecraft),
        };
        if !stale {
            continue;
        }
        match pick_version(client, &pack, &info.id, &verdict).await {
            Ok(v) => planned.push(Planned {
                version: v,
                title: m.title.clone(),
                icon: m.icon.clone(),
                required_by: None,
            }),
            Err(_) if !is_dep && !matches!(verdict, Verdict::Server(_)) => disable(
                paths,
                m,
                format!("нет версии для Minecraft {}", pack.minecraft),
                reporter,
            ),
            Err(e) => reporter.log(format!("{}: {e:#}", m.title)),
        }
    }
    if !planned.is_empty() {
        reporter.stage("Обновляем свои моды");
        match apply(client, paths, &pack, &mut index, planned, reporter).await {
            Ok(t) => reporter.log(format!("Обновлены свои моды: {}", t.join(", "))),
            Err(e) => reporter.log(format!("Свои моды не обновлены: {e:#}")),
        }
    }

    // Any enabled jar whose Fabric id the pack already has would crash the
    // game. This one applies to dependencies too: the pack's copy serves them.
    for m in index.mods.iter_mut().filter(|m| m.enabled) {
        if m.mod_id.is_none() {
            m.mod_id = read_jar(&file_path(paths, m)).map(|i| i.id);
        }
        if m.mod_id.as_ref().is_some_and(|id| ids.contains(id)) {
            disable(paths, m, "уже есть в сборке MineHouse".into(), reporter);
        } else if !needed(m, &enabled)
            && pack.clashes_with_server(m.mod_id.as_deref(), m.version_id.as_deref())
        {
            disable(paths, m, SERVER_CLASH.into(), reporter);
        }
    }
    save(paths, &index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modpack::{PackFile, ServerMod};

    fn manifest() -> Manifest {
        Manifest {
            version: "1".into(),
            files: vec![PackFile {
                path: "mods/fabric-api.jar".into(),
                url: "https://cdn.modrinth.com/data/P7dR8mSH/versions/x/fabric-api.jar".into(),
                sha512: String::new(),
                size: 0,
                mod_slug: Some("fabric-api".into()),
                choices: None,
            }],
            options: vec![],
            minecraft: Some("26.3".into()),
            loader_version: None,
            server: None,
            server_mods: vec![ServerMod {
                project: "dF99Qv0b".into(),
                version: "PrTshZAh".into(),
                mod_id: Some("fsit".into()),
            }],
        }
    }

    fn write_jar(path: &Path, fabric_json: &str) {
        let f = std::fs::File::create(path).unwrap();
        let mut z = zip::ZipWriter::new(f);
        z.start_file::<_, ()>("fabric.mod.json", Default::default())
            .unwrap();
        std::io::Write::write_all(&mut z, fabric_json.as_bytes()).unwrap();
        z.finish().unwrap();
    }

    #[test]
    fn verdicts() {
        let pack = Pack::from_manifest(&manifest());
        assert!(pack.files.contains("fabric-api.jar"));
        assert_eq!(
            pack.verdict("P7dR8mSH", "optional", "optional"),
            Verdict::InPack
        );
        assert_eq!(
            pack.verdict("dF99Qv0b", "optional", "required"),
            Verdict::Server("PrTshZAh".into())
        );
        assert_eq!(
            pack.verdict("x", "unsupported", "required"),
            Verdict::ServerOnly
        );
        assert_eq!(
            pack.verdict("x", "optional", "required"),
            Verdict::NeedsServer
        );
        // A library another mod pulled in (Fzzy Config for Subtle Effects).
        assert_eq!(
            pack.dependency_verdict("x", "required", "required"),
            Verdict::Ok
        );
        assert_eq!(
            pack.dependency_verdict("x", "unsupported", "required"),
            Verdict::ServerOnly
        );
        assert_eq!(pack.verdict("x", "required", "optional"), Verdict::Ok);
        assert_eq!(pack.verdict("x", "required", "unsupported"), Verdict::Ok);
        // Original FSit (other project, same Fabric id) vs the server's FSit Continued.
        assert!(pack.clashes_with_server(Some("fsit"), Some("old")));
        assert!(pack.clashes_with_server(Some("fsit"), None));
        assert!(!pack.clashes_with_server(Some("fsit"), Some("PrTshZAh")));
        assert!(!pack.clashes_with_server(Some("other"), None));
    }

    #[test]
    fn hand_added_jars_are_listed_and_toggled() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path());
        let pack = Pack::from_manifest(&manifest());
        let mods = paths.game_dir().join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        write_jar(&mods.join("fabric-api.jar"), r#"{"id":"fabric-api"}"#);
        write_jar(
            &mods.join("mine.jar"),
            "{\"id\":\"mine\",\"name\":\"My\nMod\",\"version\":\"1.0\"}",
        );

        let l = list(&paths, &pack).unwrap();
        assert_eq!(l.mods.len(), 1, "pack files are not player mods");
        assert_eq!(l.mods[0].mod_id.as_deref(), Some("mine"));
        assert_eq!(l.mods[0].title, "My Mod");

        set_enabled(&paths, &pack, "mine.jar", false).unwrap();
        assert!(mods.join("mine.jar.disabled").exists());
        assert!(!mods.join("mine.jar").exists());
        let l = list(&paths, &pack).unwrap();
        assert!(!l.mods[0].enabled);

        set_enabled(&paths, &pack, "mine.jar", true).unwrap();
        remove(&paths, &pack, "mine.jar").unwrap();
        assert!(!mods.join("mine.jar").exists());
        assert!(mods.join("fabric-api.jar").exists());
        assert!(list(&paths, &pack).unwrap().mods.is_empty());
    }

    #[test]
    fn duplicate_of_pack_mod_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().join("inst"));
        let pack = Pack::from_manifest(&manifest());
        let mods = paths.game_dir().join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        write_jar(&mods.join("fabric-api.jar"), r#"{"id":"fabric-api"}"#);
        let src = dir.path().join("fabric-api-old.jar");
        write_jar(&src, r#"{"id":"fabric-api"}"#);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        // No network needed: the pack check comes before the Modrinth lookup.
        let err = rt
            .block_on(add_local(&reqwest::Client::new(), &paths, &pack, &src))
            .unwrap_err();
        assert!(format!("{err}").contains("уже есть в сборке"));
        assert!(!mods.join("fabric-api-old.jar").exists());
    }

    #[test]
    fn dependency_turned_off_by_launcher_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path());
        let mods = paths.game_dir().join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        let m = |file: &str, project: &str, by: &[&str], enabled: bool, note: bool| UserMod {
            file: file.into(),
            enabled,
            title: file.into(),
            project: Some(project.into()),
            required_by: by.iter().map(|s| s.to_string()).collect(),
            note: note.then(|| "Нужен и на сервере".to_string()),
            ..Default::default()
        };
        let mut index = Index {
            mods: vec![
                m("a.jar", "A", &[], true, false),
                m("lib.jar", "L", &["A"], false, true),
                m("lib2.jar", "L2", &["L"], false, true),
                m("hand.jar", "H", &["A"], false, false),
                m("orphan.jar", "O", &["X"], false, true),
            ],
        };
        for u in &index.mods {
            std::fs::write(file_path(&paths, u), b"").unwrap();
        }
        restore_needed(&paths, &mut index, &HashSet::new(), &Reporter::silent());
        let on: Vec<bool> = index.mods.iter().map(|u| u.enabled).collect();
        assert_eq!(on, [true, true, true, false, false]);
        assert!(mods.join("lib.jar").exists());
        assert!(mods.join("lib2.jar").exists());
        assert!(index.mods[1].note.is_none());
    }

    #[test]
    fn removing_a_mod_removes_its_orphan_dependencies() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path());
        let pack = Pack::from_manifest(&manifest());
        let mods = paths.game_dir().join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        for f in ["a.jar", "b.jar", "lib.jar"] {
            write_jar(&mods.join(f), &format!(r#"{{"id":"{f}"}}"#));
        }
        let m = |file: &str, project: &str, by: &[&str]| UserMod {
            file: file.into(),
            enabled: true,
            title: file.into(),
            project: Some(project.into()),
            required_by: by.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        save(
            &paths,
            &Index {
                mods: vec![
                    m("a.jar", "A", &[]),
                    m("b.jar", "B", &[]),
                    m("lib.jar", "L", &["A", "B"]),
                ],
            },
        )
        .unwrap();
        remove(&paths, &pack, "a.jar").unwrap();
        assert!(mods.join("lib.jar").exists(), "B still needs it");
        remove(&paths, &pack, "b.jar").unwrap();
        assert!(!mods.join("lib.jar").exists());
    }
}
