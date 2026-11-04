// No console window behind the launcher on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod account;

use std::sync::Arc;

use minehouse_core::auth::{self, DeviceCode};
use minehouse_core::paths::Paths;
use minehouse_core::progress::{Event, Reporter};
use minehouse_core::settings::Settings;
use minehouse_core::user_mods::{self, Pack};
use minehouse_core::{http, install, launch, modpack, modrinth, ping};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

struct App {
    paths: Paths,
    client: http::ReqwestClient,
    busy: Mutex<()>,
    /// The last manifest fetch, for the mod browser.
    manifest: Mutex<Option<modpack::Manifest>>,
}

impl App {
    async fn manifest(&self, refresh: bool) -> Res<modpack::Manifest> {
        let mut cached = self.manifest.lock().await;
        if refresh || cached.is_none() {
            let m = modpack::fetch_manifest(&self.client, &self.paths)
                .await
                .map_err(err)?;
            *cached = Some(m);
        }
        Ok(cached.clone().unwrap())
    }

    /// Pack/server facts with the player's choice of optional mods.
    async fn pack(&self, refresh: bool) -> Res<Pack> {
        Ok(Pack::load(&self.manifest(refresh).await?, &self.paths))
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
    /// False inside Flatpak: Flathub ships the updates there.
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
    let account = account::load(&app.paths).map(|a| AccountInfo {
        name: a.session.name,
        uuid: a.session.uuid,
    });
    Ok(Info {
        version: minehouse_core::LAUNCHER_VERSION,
        login_configured: auth::client_id().is_some(),
        account,
        settings: Settings::load(&app.paths),
        pack_version: minehouse_core::modpack::read_state(&app.paths)
            .await
            .map(|s| s.version),
        game_dir: app.paths.game_dir().to_string_lossy().into_owned(),
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
    account::save(&app.paths, &account).map_err(err)?;
    Ok(AccountInfo {
        name: account.session.name,
        uuid: account.session.uuid,
    })
}

#[tauri::command]
async fn logout(app: State<'_, Arc<App>>) -> Res<()> {
    account::clear(&app.paths);
    Ok(())
}

#[tauri::command]
async fn save_settings(app: State<'_, Arc<App>>, settings: Settings) -> Res<()> {
    let mut s = settings;
    s.memory_mb = s.memory_mb.clamp(1024, 32768);
    // Alts are changed only by the profile_* commands.
    let stored = Settings::load(&app.paths);
    s.offline_profiles = stored.offline_profiles;
    s.active_profile = stored.active_profile;
    s.save(&app.paths).map_err(err)
}

/// Adds an offline profile (alt). Only with a signed-in Microsoft account:
/// playing as an alt is for players who own the game, as in Prism Launcher.
#[tauri::command]
async fn profile_add(app: State<'_, Arc<App>>, name: String) -> Res<()> {
    let name = name.trim().to_string();
    let main = account::load(&app.paths)
        .ok_or("Твинков можно добавить только после входа через Microsoft")?;
    if !auth::valid_nickname(&name) {
        return Err("Ник: от 3 до 16 латинских букв, цифр или _".into());
    }
    let mut s = Settings::load(&app.paths);
    let taken = |n: &str| n.eq_ignore_ascii_case(&name);
    if taken(&main.session.name) || s.offline_profiles.iter().any(|n| taken(n)) {
        return Err("Такой ник уже есть".into());
    }
    s.offline_profiles.push(name.clone());
    s.active_profile = Some(name);
    s.save(&app.paths).map_err(err)
}

#[tauri::command]
async fn profile_remove(app: State<'_, Arc<App>>, name: String) -> Res<()> {
    let mut s = Settings::load(&app.paths);
    s.offline_profiles.retain(|n| n != &name);
    if s.active_profile.as_deref() == Some(name.as_str()) {
        s.active_profile = None;
    }
    s.save(&app.paths).map_err(err)
}

/// `None` = play as the Microsoft account.
#[tauri::command]
async fn profile_select(app: State<'_, Arc<App>>, name: Option<String>) -> Res<()> {
    let mut s = Settings::load(&app.paths);
    if let Some(n) = &name {
        if !s.offline_profiles.contains(n) {
            return Err("Нет такого твинка".into());
        }
    }
    s.active_profile = name;
    s.save(&app.paths).map_err(err)
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
    let dir = app.paths.game_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    handle
        .opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Changing `mods/` while the game runs (or installs) would break it, and
/// Windows keeps the jars locked anyway.
fn not_busy(app: &App) -> Res<tokio::sync::MutexGuard<'_, ()>> {
    app.busy
        .try_lock()
        .map_err(|_| "Закройте игру, чтобы менять моды".to_string())
}

#[tauri::command]
async fn mods_list(app: State<'_, Arc<App>>) -> Res<user_mods::Listing> {
    let pack = app.pack(true).await?;
    user_mods::list(&app.paths, &pack).map_err(err)
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
    let installed: std::collections::HashSet<String> = user_mods::list(&app.paths, &pack)
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
    let _guard = not_busy(&app)?;
    let pack = app.pack(false).await?;
    user_mods::install_modrinth(&app.client, &app.paths, &pack, &project, icon)
        .await
        .map_err(err)
}

/// Adds the player's own jars (from the file picker or dropped on the window).
/// Returns one line per file: what was added, or why not.
#[tauri::command]
async fn mods_add_files(app: State<'_, Arc<App>>, files: Vec<String>) -> Res<Vec<String>> {
    let _guard = not_busy(&app)?;
    let pack = app.pack(false).await?;
    let mut out = Vec::new();
    for f in files {
        let path = std::path::PathBuf::from(&f);
        out.push(
            match user_mods::add_local(&app.client, &app.paths, &pack, &path).await {
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
    let _guard = not_busy(&app)?;
    let pack = app.pack(false).await?;
    user_mods::set_enabled(&app.paths, &pack, &file, enabled).map_err(err)
}

/// Turns an optional mod of the pack on or off (downloads or removes it now).
#[tauri::command]
async fn mods_pack_option(app: State<'_, Arc<App>>, id: String, enabled: bool) -> Res<()> {
    let _guard = not_busy(&app)?;
    let manifest = app.manifest(false).await?;
    user_mods::set_pack_option(
        &app.client,
        &app.paths,
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
    let _guard = not_busy(&app)?;
    let pack = app.pack(false).await?;
    user_mods::remove(&app.paths, &pack, &file).map_err(err)
}

#[tauri::command]
async fn mods_update(app: State<'_, Arc<App>>) -> Res<Vec<String>> {
    let _guard = not_busy(&app)?;
    let pack = app.pack(true).await?;
    user_mods::update_all(&app.client, &app.paths, &pack)
        .await
        .map_err(err)
}

#[tauri::command]
async fn open_mods_dir(app: State<'_, Arc<App>>, handle: AppHandle) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = app.paths.game_dir().join("mods");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    handle
        .opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Sign-in refresh → install/update → launch. Progress goes out as `progress` events.
#[tauri::command]
async fn play(app: State<'_, Arc<App>>, handle: AppHandle) -> Res<()> {
    let app = app.inner().clone();
    let Ok(_guard) = app.busy.try_lock() else {
        return Err("Уже запускается".into());
    };
    let emitter = handle.clone();
    let reporter = Reporter::new(move |e: Event| {
        let _ = emitter.emit("progress", e);
    });

    reporter.stage("Входим в аккаунт");
    let stored = account::load(&app.paths).ok_or("Сначала войдите через Microsoft")?;
    // Refreshing the Microsoft account also proves the game is owned: it fails
    // without a Minecraft profile. Alts are only launched after that.
    let account = auth::refresh(&app.client, &stored).await.map_err(err)?;
    account::save(&app.paths, &account).map_err(err)?;

    let installed = install::install(&app.paths, &reporter).await.map_err(err)?;
    let settings = Settings::load(&app.paths);
    let session = match &settings.active_profile {
        Some(alt) if settings.offline_profiles.contains(alt) => {
            reporter.log(format!("Играем за твинка {alt} (офлайн-профиль)"));
            auth::offline_session(alt)
        }
        _ => account.session.clone(),
    };

    reporter.stage("Игра запущена");
    let _ = handle.emit("game", "started");
    if settings.close_on_launch {
        if let Some(w) = handle.get_webview_window("main") {
            let _ = w.hide();
        }
    }
    let status = launch::launch(&app.paths, &installed, &session, &settings, &reporter)
        .await
        .map_err(err)?;
    if let Some(w) = handle.get_webview_window("main") {
        let _ = w.show();
    }
    let _ = handle.emit("game", "stopped");
    if !status.success() {
        return Err(format!(
            "Игра закрылась с ошибкой ({status}). Подробности в журнале."
        ));
    }
    Ok(())
}

fn main() {
    let paths = Paths::default_location().expect("data directory");
    let app = Arc::new(App {
        paths,
        client: http::client(),
        busy: Mutex::new(()),
        manifest: Mutex::new(None),
    });
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init());
    // The Flatpak build is updated by Flathub; replacing files inside the
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
            play
        ])
        .run(tauri::generate_context!())
        .expect("error while running MineHouse launcher");
}
