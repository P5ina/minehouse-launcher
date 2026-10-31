// No console window behind the launcher on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod account;

use std::sync::Arc;

use minehouse_core::auth::{self, DeviceCode};
use minehouse_core::paths::Paths;
use minehouse_core::progress::{Event, Reporter};
use minehouse_core::settings::Settings;
use minehouse_core::{http, install, launch, ping};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

struct App {
    paths: Paths,
    client: http::ReqwestClient,
    busy: Mutex<()>,
}

#[derive(Serialize)]
struct Info {
    version: &'static str,
    login_configured: bool,
    account: Option<AccountInfo>,
    settings: Settings,
    pack_version: Option<String>,
    game_dir: String,
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
    s.save(&app.paths).map_err(err)
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
    let account = auth::refresh(&app.client, &stored).await.map_err(err)?;
    account::save(&app.paths, &account).map_err(err)?;

    let installed = install::install(&app.paths, &reporter).await.map_err(err)?;
    let settings = Settings::load(&app.paths);

    reporter.stage("Игра запущена");
    let _ = handle.emit("game", "started");
    if settings.close_on_launch {
        if let Some(w) = handle.get_webview_window("main") {
            let _ = w.hide();
        }
    }
    let status = launch::launch(
        &app.paths,
        &installed,
        &account.session,
        &settings,
        &reporter,
    )
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
    });
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(app)
        .invoke_handler(tauri::generate_handler![
            info,
            server_status,
            login_start,
            login_finish,
            logout,
            save_settings,
            open_url,
            open_game_dir,
            play
        ])
        .run(tauri::generate_context!())
        .expect("error while running MineHouse launcher");
}
