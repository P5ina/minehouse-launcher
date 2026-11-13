// No console window behind the launcher on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod account;

use std::path::PathBuf;
use std::sync::Arc;

use minehouse_core::auth::{self, DeviceCode};
use minehouse_core::packs::{self, Kind};
use minehouse_core::paths::Paths;
use minehouse_core::progress::{Event, Reporter};
use minehouse_core::settings::Settings;
use minehouse_core::user_mods::{self, Pack};
use minehouse_core::{http, install, instances, launch, modpack, modrinth, ping};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

struct App {
    /// Launcher config folder; game files go wherever [`instances::Config`] says.
    root: PathBuf,
    client: http::ReqwestClient,
    /// Instances being installed or played, and whether the game folder is moving.
    runs: std::sync::Mutex<Runs>,
    /// Java, Minecraft, libraries and assets are shared: one install at a time.
    shared: Mutex<()>,
    /// The last manifest fetch, for the mod browser.
    manifest: Mutex<Option<modpack::Manifest>>,
}

#[derive(Default)]
struct Runs {
    /// Instance id → nickname it plays as (`None` while only its files are changed).
    running: std::collections::HashMap<String, Option<String>>,
    moving: bool,
}

/// Holds one instance until dropped: nobody else installs, plays or edits it.
struct InstanceLock {
    app: Arc<App>,
    id: String,
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        self.app.runs.lock().unwrap().running.remove(&self.id);
    }
}

impl InstanceLock {
    fn set_nick(&self, nick: &str) {
        if let Some(n) = self.app.runs.lock().unwrap().running.get_mut(&self.id) {
            *n = Some(nick.to_string());
        }
    }
}

/// Locks `id`, or explains why not with `busy` (what the player tried to do).
fn lock_instance(app: &Arc<App>, id: &str, busy: &str) -> Res<InstanceLock> {
    let mut runs = app.runs.lock().unwrap();
    if runs.moving {
        return Err("Подождите, папка игры переносится".into());
    }
    if runs.running.contains_key(id) {
        return Err(busy.to_string());
    }
    runs.running.insert(id.to_string(), None);
    Ok(InstanceLock {
        app: app.clone(),
        id: id.to_string(),
    })
}

/// The selected instance, locked while its mods or packs change.
fn lock_active(app: &Arc<App>, busy: &str) -> Res<InstanceLock> {
    let id = app.config().active().id.clone();
    lock_instance(app, &id, busy)
}

impl App {
    fn config(&self) -> instances::Config {
        instances::Config::load(&self.root)
    }

    /// Paths of the selected instance, read fresh: the player can switch it.
    fn paths(&self) -> Paths {
        self.config().paths()
    }

    async fn manifest(&self, refresh: bool) -> Res<modpack::Manifest> {
        let mut cached = self.manifest.lock().await;
        if refresh || cached.is_none() {
            let m = modpack::fetch_manifest(&self.client, &self.paths())
                .await
                .map_err(err)?;
            *cached = Some(m);
        }
        Ok(cached.clone().unwrap())
    }

    /// Pack/server facts with the player's choice of optional mods.
    async fn pack(&self, refresh: bool) -> Res<Pack> {
        Ok(Pack::load(&self.manifest(refresh).await?, &self.paths()))
    }
}

#[derive(Serialize)]
struct Info {
    version: &'static str,
    login_configured: bool,
    account: Option<AccountInfo>,
    settings: Settings,
    pack_version: Option<String>,
    game_dir: String,
    instances: Vec<instances::Instance>,
    active_instance: String,
    /// Where Java, Minecraft and `instances/` are.
    data_dir: String,
    /// The system app-data folder, for «вернуть как было».
    default_data_dir: String,
    /// False until the first-start onboarding is done.
    setup_done: bool,
    /// False inside Flatpak: the sandbox only writes to its own folder.
    can_move_data: bool,
    /// False inside Flatpak: `flatpak update` ships the updates there.
    self_update: bool,
}

/// Flatpak sets `FLATPAK_ID` and mounts `/.flatpak-info` in every sandbox.
fn in_flatpak() -> bool {
    std::env::var_os("FLATPAK_ID").is_some() || std::path::Path::new("/.flatpak-info").exists()
}

#[derive(Serialize)]
struct AccountInfo {
    name: String,
    uuid: String,
}

type Res<T> = Result<T, String>;

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

#[tauri::command]
async fn info(app: State<'_, Arc<App>>) -> Res<Info> {
    let config = app.config();
    let paths = config.paths();
    let account = account::load(&paths).map(|a| AccountInfo {
        name: a.session.name,
        uuid: a.session.uuid,
    });
    Ok(Info {
        version: minehouse_core::LAUNCHER_VERSION,
        login_configured: auth::client_id().is_some(),
        account,
        settings: Settings::load(&paths),
        pack_version: minehouse_core::modpack::read_state(&paths)
            .await
            .map(|s| s.version),
        game_dir: paths.game_dir().to_string_lossy().into_owned(),
        active_instance: config.active().id.clone(),
        data_dir: config.data_dir().to_string_lossy().into_owned(),
        default_data_dir: config.root().to_string_lossy().into_owned(),
        setup_done: config.setup_done,
        can_move_data: !in_flatpak(),
        instances: config.instances,
        self_update: !in_flatpak(),
    })
}

#[tauri::command]
async fn server_status() -> Res<ping::Status> {
    ping::ping(minehouse_core::SERVER_HOST, minehouse_core::SERVER_PORT)
        .await
        .map_err(err)
}

#[tauri::command]
async fn login_start(app: State<'_, Arc<App>>) -> Res<DeviceCode> {
    auth::start_device_code(&app.client).await.map_err(err)
}

#[tauri::command]
async fn login_finish(app: State<'_, Arc<App>>, code: DeviceCode) -> Res<AccountInfo> {
    let account = auth::finish_device_code(&app.client, &code)
        .await
        .map_err(err)?;
    account::save(&app.paths(), &account).map_err(err)?;
    Ok(AccountInfo {
        name: account.session.name,
        uuid: account.session.uuid,
    })
}

#[tauri::command]
async fn logout(app: State<'_, Arc<App>>) -> Res<()> {
    account::clear(&app.paths());
    Ok(())
}

#[tauri::command]
async fn save_settings(app: State<'_, Arc<App>>, settings: Settings) -> Res<()> {
    let mut s = settings;
    s.memory_mb = s.memory_mb.clamp(1024, 32768);
    // Alts are changed only by the profile_* commands.
    let stored = Settings::load(&app.paths());
    s.offline_profiles = stored.offline_profiles;
    s.active_profile = stored.active_profile;
    s.save(&app.paths()).map_err(err)
}

/// Adds an offline profile (alt). Only with a signed-in Microsoft account:
/// playing as an alt is for players who own the game, as in Prism Launcher.
#[tauri::command]
async fn profile_add(app: State<'_, Arc<App>>, name: String) -> Res<()> {
    let name = name.trim().to_string();
    let main = account::load(&app.paths())
        .ok_or("Твинков можно добавить только после входа через Microsoft")?;
    if !auth::valid_nickname(&name) {
        return Err("Ник: от 3 до 16 латинских букв, цифр или _".into());
    }
    let mut s = Settings::load(&app.paths());
    let taken = |n: &str| n.eq_ignore_ascii_case(&name);
    if taken(&main.session.name) || s.offline_profiles.iter().any(|n| taken(n)) {
        return Err("Такой ник уже есть".into());
    }
    s.offline_profiles.push(name.clone());
    s.active_profile = Some(name);
    s.save(&app.paths()).map_err(err)
}

#[tauri::command]
async fn profile_remove(app: State<'_, Arc<App>>, name: String) -> Res<()> {
    let mut s = Settings::load(&app.paths());
    s.offline_profiles.retain(|n| n != &name);
    if s.active_profile.as_deref() == Some(name.as_str()) {
        s.active_profile = None;
    }
    s.save(&app.paths()).map_err(err)
}

/// `None` = play as the Microsoft account.
#[tauri::command]
async fn profile_select(app: State<'_, Arc<App>>, name: Option<String>) -> Res<()> {
    let mut s = Settings::load(&app.paths());
    if let Some(n) = &name {
        if !s.offline_profiles.contains(n) {
            return Err("Нет такого твинка".into());
        }
    }
    s.active_profile = name;
    s.save(&app.paths()).map_err(err)
}

/// Latest posts from www.minehouse.pro/news for the home screen.
#[tauri::command]
async fn news(app: State<'_, Arc<App>>) -> Res<serde_json::Value> {
    let url = format!("{}/api/news?limit=6", minehouse_core::SITE);
    let res = app
        .client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let res = res.error_for_status().map_err(|e| e.to_string())?;
    res.json().await.map_err(|e| e.to_string())
}

/// Opens an https link in the default browser. Done on the Rust side so the
/// webview needs no opener URL scope (the bare allow-open-url permission had none).
#[tauri::command]
async fn open_url(handle: AppHandle, url: String) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    if !url.starts_with("https://") {
        return Err("Можно открывать только https-ссылки".into());
    }
    handle
        .opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn open_game_dir(app: State<'_, Arc<App>>, handle: AppHandle) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = app.paths().game_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    handle
        .opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Changing `mods/` while the game runs (or installs) would break it, and
/// Windows keeps the jars locked anyway.
fn not_busy(app: &Arc<App>) -> Res<InstanceLock> {
    lock_active(app, "Закройте игру этой сборки, чтобы менять моды")
}

#[tauri::command]
async fn mods_list(app: State<'_, Arc<App>>) -> Res<user_mods::Listing> {
    let pack = app.pack(true).await?;
    user_mods::list(&app.paths(), &pack).map_err(err)
}

#[derive(Serialize)]
struct SearchHit {
    #[serde(flatten)]
    hit: modrinth::SearchHit,
    verdict: user_mods::Verdict,
    reason: &'static str,
    installed: bool,
}

#[derive(Serialize)]
struct Search {
    hits: Vec<SearchHit>,
    total: u64,
}

#[tauri::command]
async fn mods_search(app: State<'_, Arc<App>>, query: String, offset: u32) -> Res<Search> {
    let pack = app.pack(false).await?;
    // Browsing without a query: don't fill the first page with mods the pack already has.
    let exclude: Vec<String> = if query.trim().is_empty() {
        pack.projects.iter().cloned().collect()
    } else {
        Vec::new()
    };
    let (hits, total) = modrinth::search(&app.client, &query, &pack.minecraft, offset, &exclude)
        .await
        .map_err(err)?;
    let installed: std::collections::HashSet<String> = user_mods::list(&app.paths(), &pack)
        .map_err(err)?
        .mods
        .into_iter()
        .filter_map(|m| m.project)
        .collect();
    Ok(Search {
        total,
        hits: hits
            .into_iter()
            .map(|hit| {
                let verdict = pack.verdict(&hit.project_id, &hit.client_side, &hit.server_side);
                SearchHit {
                    installed: installed.contains(&hit.project_id),
                    reason: verdict.reason(),
                    verdict,
                    hit,
                }
            })
            .collect(),
    })
}

#[tauri::command]
async fn mods_install(
    app: State<'_, Arc<App>>,
    project: String,
    icon: Option<String>,
) -> Res<Vec<String>> {
    let _guard = not_busy(app.inner())?;
    let pack = app.pack(false).await?;
    user_mods::install_modrinth(&app.client, &app.paths(), &pack, &project, icon)
        .await
        .map_err(err)
}

/// Adds the player's own jars (from the file picker or dropped on the window).
/// Returns one line per file: what was added, or why not.
#[tauri::command]
async fn mods_add_files(app: State<'_, Arc<App>>, files: Vec<String>) -> Res<Vec<String>> {
    let _guard = not_busy(app.inner())?;
    let pack = app.pack(false).await?;
    let mut out = Vec::new();
    for f in files {
        let path = std::path::PathBuf::from(&f);
        out.push(
            match user_mods::add_local(&app.client, &app.paths(), &pack, &path).await {
                Ok(title) => format!("Добавлен: {title}"),
                Err(e) => format!("{e:#}"),
            },
        );
    }
    Ok(out)
}

#[tauri::command]
async fn mods_pick_files(app: State<'_, Arc<App>>, handle: AppHandle) -> Res<Vec<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    handle
        .dialog()
        .file()
        .set_title("Выберите моды")
        .add_filter("Мод Fabric", &["jar"])
        .pick_files(move |files| {
            let _ = tx.send(files);
        });
    let Some(files) = rx.await.ok().flatten() else {
        return Ok(Vec::new());
    };
    let files = files
        .into_iter()
        .filter_map(|f| f.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    mods_add_files(app, files).await
}

#[tauri::command]
async fn mods_toggle(app: State<'_, Arc<App>>, file: String, enabled: bool) -> Res<()> {
    let _guard = not_busy(app.inner())?;
    let pack = app.pack(false).await?;
    user_mods::set_enabled(&app.paths(), &pack, &file, enabled).map_err(err)
}

/// Turns an optional mod of the pack on or off (downloads or removes it now).
#[tauri::command]
async fn mods_pack_option(app: State<'_, Arc<App>>, id: String, enabled: bool) -> Res<()> {
    let _guard = not_busy(app.inner())?;
    let manifest = app.manifest(false).await?;
    user_mods::set_pack_option(
        &app.client,
        &app.paths(),
        &manifest,
        &id,
        enabled,
        &Reporter::silent(),
    )
    .await
    .map_err(err)
}

#[tauri::command]
async fn mods_remove(app: State<'_, Arc<App>>, file: String) -> Res<()> {
    let _guard = not_busy(app.inner())?;
    let pack = app.pack(false).await?;
    user_mods::remove(&app.paths(), &pack, &file).map_err(err)
}

#[tauri::command]
async fn mods_update(app: State<'_, Arc<App>>) -> Res<Vec<String>> {
    let _guard = not_busy(app.inner())?;
    let pack = app.pack(true).await?;
    user_mods::update_all(&app.client, &app.paths(), &pack)
        .await
        .map_err(err)
}

#[tauri::command]
async fn open_mods_dir(app: State<'_, Arc<App>>, handle: AppHandle) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = app.paths().game_dir().join("mods");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    handle
        .opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

// ---- Resource packs and shaders -------------------------------------------

/// Iris (and Sodium under it) are optional mods of the pack; shaders need them.
#[derive(Serialize)]
struct IrisState {
    /// Iris is on in the pack (or always part of it).
    ready: bool,
    /// What else has to be turned on for it, by name.
    missing: Vec<String>,
}

fn iris_state(pack: &Pack) -> IrisState {
    match pack.options.iter().find(|o| o.id == "iris") {
        Some(o) => {
            let mut missing = o.missing.clone();
            if !o.enabled {
                missing.push(o.name.clone());
            }
            IrisState {
                ready: missing.is_empty(),
                missing,
            }
        }
        None => {
            let ready = pack.files.iter().any(|f| f.starts_with("iris"));
            IrisState {
                ready,
                missing: if ready { vec![] } else { vec!["Iris".into()] },
            }
        }
    }
}

#[derive(Serialize)]
struct PacksListing {
    packs: Vec<packs::ContentPack>,
    minecraft: String,
    iris: IrisState,
}

fn kind(s: &str) -> Res<Kind> {
    Kind::parse(s).map_err(err)
}

#[tauri::command]
async fn packs_list(app: State<'_, Arc<App>>, kind: String) -> Res<PacksListing> {
    let kind = self::kind(&kind)?;
    let pack = app.pack(false).await?;
    Ok(PacksListing {
        packs: packs::list(&app.paths(), kind).map_err(err)?,
        minecraft: pack.minecraft.clone(),
        iris: iris_state(&pack),
    })
}

#[derive(Serialize)]
struct PackHit {
    #[serde(flatten)]
    hit: modrinth::SearchHit,
    installed: bool,
}

#[derive(Serialize)]
struct PackSearch {
    hits: Vec<PackHit>,
    total: u64,
}

#[tauri::command]
async fn packs_search(
    app: State<'_, Arc<App>>,
    kind: String,
    query: String,
    offset: u32,
) -> Res<PackSearch> {
    let kind = self::kind(&kind)?;
    let pack = app.pack(false).await?;
    let (hits, total) = modrinth::search_content(
        &app.client,
        kind.project_type(),
        kind.category(),
        &query,
        &pack.minecraft,
        offset,
    )
    .await
    .map_err(err)?;
    let installed = packs::installed_projects(&app.paths(), kind);
    Ok(PackSearch {
        total,
        hits: hits
            .into_iter()
            .map(|hit| PackHit {
                installed: installed.contains(&hit.project_id),
                hit,
            })
            .collect(),
    })
}

/// The game reads these files only at start; while it runs, its own screens own them.
fn not_playing(app: &Arc<App>) -> Res<InstanceLock> {
    lock_active(
        app,
        "Закройте игру этой сборки: пока она запущена, паки меняются в её настройках",
    )
}

#[tauri::command]
async fn packs_install(
    app: State<'_, Arc<App>>,
    kind: String,
    project: String,
    icon: Option<String>,
) -> Res<String> {
    let kind = self::kind(&kind)?;
    let _guard = not_playing(app.inner())?;
    let pack = app.pack(false).await?;
    packs::install_modrinth(
        &app.client,
        &app.paths(),
        kind,
        &project,
        icon,
        &pack.minecraft,
    )
    .await
    .map_err(err)
}

#[derive(Serialize)]
struct Added {
    lines: Vec<String>,
    /// What the last added file was, for the tab to show.
    kind: Option<Kind>,
}

#[tauri::command]
async fn packs_add_files(app: State<'_, Arc<App>>, files: Vec<String>) -> Res<Added> {
    let _guard = not_playing(app.inner())?;
    let mut out = Added {
        lines: Vec::new(),
        kind: None,
    };
    for f in files {
        let path = std::path::PathBuf::from(&f);
        match packs::add_local(&app.client, &app.paths(), &path).await {
            Ok((kind, title)) => {
                out.kind = Some(kind);
                out.lines.push(format!(
                    "Добавлен {}: {title}",
                    match kind {
                        Kind::Resource => "ресурспак",
                        Kind::Shader => "шейдер",
                    }
                ));
            }
            Err(e) => out.lines.push(format!("{e:#}")),
        }
    }
    Ok(out)
}

#[tauri::command]
async fn packs_pick_files(app: State<'_, Arc<App>>, handle: AppHandle) -> Res<Added> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    handle
        .dialog()
        .file()
        .set_title("Выберите ресурспаки или шейдеры")
        .add_filter("Ресурспак или шейдер", &["zip"])
        .pick_files(move |files| {
            let _ = tx.send(files);
        });
    let Some(files) = rx.await.ok().flatten() else {
        return Ok(Added {
            lines: Vec::new(),
            kind: None,
        });
    };
    let files = files
        .into_iter()
        .filter_map(|f| f.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    packs_add_files(app, files).await
}

#[tauri::command]
async fn packs_toggle(
    app: State<'_, Arc<App>>,
    kind: String,
    file: String,
    enabled: bool,
) -> Res<()> {
    let kind = self::kind(&kind)?;
    let _guard = not_playing(app.inner())?;
    packs::set_enabled(&app.paths(), kind, &file, enabled).map_err(err)
}

#[tauri::command]
async fn packs_move(app: State<'_, Arc<App>>, file: String, up: bool) -> Res<()> {
    let _guard = not_playing(app.inner())?;
    packs::move_resource_pack(&app.paths(), &file, up).map_err(err)
}

#[tauri::command]
async fn packs_remove(app: State<'_, Arc<App>>, kind: String, file: String) -> Res<()> {
    let kind = self::kind(&kind)?;
    let _guard = not_playing(app.inner())?;
    packs::remove(&app.paths(), kind, &file).map_err(err)
}

#[tauri::command]
async fn packs_update(app: State<'_, Arc<App>>, kind: String) -> Res<Vec<String>> {
    let kind = self::kind(&kind)?;
    let _guard = not_playing(app.inner())?;
    let pack = app.pack(false).await?;
    packs::update_all(&app.client, &app.paths(), kind, &pack.minecraft)
        .await
        .map_err(err)
}

/// Turns on Iris and the optional mods it needs (Sodium), downloading them now.
#[tauri::command]
async fn packs_enable_iris(app: State<'_, Arc<App>>) -> Res<()> {
    let _guard = not_busy(app.inner())?;
    let manifest = app.manifest(false).await?;
    let Some(iris) = manifest.options.iter().find(|o| o.id == "iris") else {
        return Err("В сборке нет Iris".into());
    };
    let mut ids = iris.requires.clone();
    ids.push("iris".into());
    for id in ids {
        user_mods::set_pack_option(
            &app.client,
            &app.paths(),
            &manifest,
            &id,
            true,
            &Reporter::silent(),
        )
        .await
        .map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
async fn open_packs_dir(app: State<'_, Arc<App>>, handle: AppHandle, kind: String) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = self::kind(&kind)?.dir(&app.paths());
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    handle
        .opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

// ---- Instances («сборки») and the game folder --------------------------------

/// Moving the game folder while any game runs would pull files from under it.
struct MoveLock(Arc<App>);

impl Drop for MoveLock {
    fn drop(&mut self) {
        self.0.runs.lock().unwrap().moving = false;
    }
}

fn lock_all(app: &Arc<App>) -> Res<MoveLock> {
    let mut runs = app.runs.lock().unwrap();
    if !runs.running.is_empty() || runs.moving {
        return Err("Закройте все запущенные сборки, чтобы перенести папку".into());
    }
    runs.moving = true;
    Ok(MoveLock(app.clone()))
}

#[tauri::command]
async fn instance_create(app: State<'_, Arc<App>>, name: String) -> Res<instances::Instance> {
    app.config().create(&name).map_err(err)
}

/// Copies mods, packs, options and worlds; can take a while with big worlds.
#[tauri::command]
async fn instance_duplicate(
    app: State<'_, Arc<App>>,
    id: String,
    name: String,
) -> Res<instances::Instance> {
    let _guard = lock_instance(
        app.inner(),
        &id,
        "Закройте игру этой сборки, чтобы сделать копию",
    )?;
    let mut config = app.config();
    tokio::task::spawn_blocking(move || config.duplicate(&id, &name))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)
}

#[tauri::command]
async fn instance_rename(app: State<'_, Arc<App>>, id: String, name: String) -> Res<()> {
    app.config().rename(&id, &name).map_err(err)
}

#[tauri::command]
async fn instance_select(app: State<'_, Arc<App>>, id: String) -> Res<()> {
    app.config().select(&id).map_err(err)
}

#[tauri::command]
async fn instance_remove(app: State<'_, Arc<App>>, id: String) -> Res<()> {
    let _guard = lock_instance(
        app.inner(),
        &id,
        "Закройте игру этой сборки, чтобы её удалить",
    )?;
    let mut config = app.config();
    tokio::task::spawn_blocking(move || config.remove(&id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)
}

#[tauri::command]
async fn open_instance_dir(app: State<'_, Arc<App>>, handle: AppHandle, id: String) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    let config = app.config();
    let dir = config.get(&id).ok_or("Нет такой сборки")?.dir.clone();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    handle
        .opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Folder picker for the game folder. Returns where files would really go
/// (a busy folder gets a `MineHouse` subfolder), or `None` if cancelled.
#[tauri::command]
async fn data_dir_pick(app: State<'_, Arc<App>>, handle: AppHandle) -> Res<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    handle
        .dialog()
        .file()
        .set_title("Куда ставить игру и сборки")
        .pick_folder(move |dir| {
            let _ = tx.send(dir);
        });
    let Some(dir) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let dir = dir.into_path().map_err(|e| e.to_string())?;
    let resolved = app.config().resolve_data_dir(&dir).map_err(err)?;
    Ok(Some(resolved.to_string_lossy().into_owned()))
}

/// Moves Java, Minecraft and the instances to `path`, reporting each step as
/// a `progress` stage. Also ends the first-start setup.
#[tauri::command]
async fn data_dir_set(app: State<'_, Arc<App>>, handle: AppHandle, path: String) -> Res<String> {
    if in_flatpak() {
        return Err("Во Flatpak папку игры менять нельзя".into());
    }
    let _guard = lock_all(app.inner())?;
    let mut config = app.config();
    let used = tokio::task::spawn_blocking(move || {
        let step = move |name: String| {
            let _ = handle.emit("progress", Event::Stage { name });
        };
        config.set_data_dir(std::path::Path::new(&path), &step)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(err)?;
    Ok(used.to_string_lossy().into_owned())
}

/// The first-start onboarding is done (or skipped).
#[tauri::command]
async fn setup_finish(app: State<'_, Arc<App>>, done: bool) -> Res<()> {
    let mut config = app.config();
    config.setup_done = done;
    config.save().map_err(err)
}

/// A `progress` / `game` event of one instance: several can run at once.
#[derive(Serialize, Clone)]
struct Tagged<T: Serialize> {
    instance: String,
    #[serde(flatten)]
    event: T,
}

#[derive(Serialize, Clone)]
struct GameState {
    state: &'static str,
}

fn running_count(app: &App) -> usize {
    app.runs
        .lock()
        .unwrap()
        .running
        .values()
        .filter(|n| n.is_some())
        .count()
}

/// Sign-in refresh → install/update → launch of the selected instance. Other
/// instances can be started meanwhile, each as its own nickname. Progress goes
/// out as `progress` events tagged with the instance.
#[tauri::command]
async fn play(app: State<'_, Arc<App>>, handle: AppHandle) -> Res<()> {
    let app = app.inner().clone();
    // One instance for the whole run, even if the player selects another meanwhile.
    let config = app.config();
    let id = config.active().id.clone();
    let lock = lock_instance(&app, &id, "Эта сборка уже запущена")?;
    let paths = config.paths();
    let emitter = handle.clone();
    let tag = id.clone();
    let reporter = Reporter::new(move |e: Event| {
        let _ = emitter.emit(
            "progress",
            Tagged {
                instance: tag.clone(),
                event: e,
            },
        );
    });
    let game_state = |state: &'static str| {
        let _ = handle.emit(
            "game",
            Tagged {
                instance: id.clone(),
                event: GameState { state },
            },
        );
    };
    reporter.log(format!("Сборка «{}»", config.active().name));

    let settings = Settings::load(&paths);
    let alt = settings
        .active_profile
        .clone()
        .filter(|a| settings.offline_profiles.contains(a));
    // The server lets a nickname in once: a second window needs another one.
    let nick_taken = |nick: &str| {
        let runs = app.runs.lock().unwrap();
        let other = runs
            .running
            .iter()
            .find(|(other, n)| {
                **other != id && n.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(nick))
            })
            .map(|(other, _)| other.clone());
        other.map(|o| {
            let name = config.get(&o).map(|i| i.name.clone()).unwrap_or(o);
            format!(
                "Под ником {nick} уже запущена сборка «{name}». Для второго окна выберите другой ник или твинка"
            )
        })
    };
    if let Some(alt) = &alt {
        if let Some(e) = nick_taken(alt) {
            return Err(e);
        }
        lock.set_nick(alt);
    }

    reporter.stage("Входим в аккаунт");
    let stored = account::load(&paths).ok_or("Сначала войдите через Microsoft")?;
    // Refreshing the Microsoft account also proves the game is owned: it fails
    // without a Minecraft profile. Alts are only launched after that.
    let account = auth::refresh(&app.client, &stored).await.map_err(err)?;
    account::save(&paths, &account).map_err(err)?;
    let session = match &alt {
        Some(alt) => {
            reporter.log(format!("Играем за твинка {alt} (офлайн-профиль)"));
            auth::offline_session(alt)
        }
        None => {
            if let Some(e) = nick_taken(&account.session.name) {
                return Err(e);
            }
            lock.set_nick(&account.session.name);
            account.session.clone()
        }
    };

    let installed = {
        let _shared = app.shared.lock().await;
        install::install(&paths, &reporter).await.map_err(err)?
    };

    reporter.stage("Игра запущена");
    game_state("started");
    if settings.close_on_launch {
        if let Some(w) = handle.get_webview_window("main") {
            let _ = w.hide();
        }
    }
    let status = launch::launch(&paths, &installed, &session, &settings, &reporter).await;
    drop(lock);
    // Back when the last game closes.
    if running_count(&app) == 0 {
        if let Some(w) = handle.get_webview_window("main") {
            let _ = w.show();
        }
    }
    game_state("stopped");
    let status = status.map_err(err)?;
    if !status.success() {
        return Err(format!(
            "Игра закрылась с ошибкой ({status}). Подробности в журнале."
        ));
    }
    Ok(())
}

/// Instances being installed or played right now.
#[tauri::command]
async fn running(app: State<'_, Arc<App>>) -> Res<Vec<String>> {
    Ok(app.runs.lock().unwrap().running.keys().cloned().collect())
}

fn main() {
    let root = Paths::default_root().expect("data directory");
    let app = Arc::new(App {
        root,
        client: http::client(),
        runs: std::sync::Mutex::new(Runs::default()),
        shared: Mutex::new(()),
        manifest: Mutex::new(None),
    });
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init());
    // The Flatpak build is updated by `flatpak update`; replacing files inside the
    // sandbox would not work anyway.
    if !in_flatpak() {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }
    builder
        .manage(app)
        .invoke_handler(tauri::generate_handler![
            info,
            server_status,
            login_start,
            login_finish,
            logout,
            save_settings,
            profile_add,
            profile_remove,
            profile_select,
            news,
            open_url,
            open_game_dir,
            open_mods_dir,
            mods_list,
            mods_search,
            mods_install,
            mods_add_files,
            mods_pick_files,
            mods_toggle,
            mods_pack_option,
            mods_remove,
            mods_update,
            packs_list,
            packs_search,
            packs_install,
            packs_add_files,
            packs_pick_files,
            packs_toggle,
            packs_move,
            packs_remove,
            packs_update,
            packs_enable_iris,
            open_packs_dir,
            instance_create,
            instance_duplicate,
            instance_rename,
            instance_select,
            instance_remove,
            open_instance_dir,
            data_dir_pick,
            data_dir_set,
            setup_finish,
            running,
            play
        ])
        .run(tauri::generate_context!())
        .expect("error while running MineHouse launcher");
}
