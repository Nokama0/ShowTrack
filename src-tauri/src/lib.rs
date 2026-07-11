pub mod binge;
pub mod commands;
pub mod db;
pub mod error;
pub mod export;
pub mod images;
pub mod import;
pub mod models;
pub mod oplog;
pub mod queries;
pub mod refresh;
pub mod sync_client;
pub mod sync_proto;
pub mod sync_server;
pub mod tmdb;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use tauri::Manager;

pub struct AppState {
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub http: reqwest::Client,
    pub busy: Arc<AtomicBool>,
    pub sync_shared: Arc<sync_server::SyncShared>,
    pub sync_server: Arc<Mutex<Option<sync_server::SyncServerHandle>>>,
    pub backups_dir: std::path::PathBuf,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("poster", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn(async move {
                images::serve(app, request, responder).await;
            });
        })
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let conn = db::open(&data_dir.join("showtrack.db"))?;
            app.manage(AppState {
                db: Arc::new(Mutex::new(conn)),
                http: reqwest::Client::new(),
                busy: Arc::new(AtomicBool::new(false)),
                sync_shared: Arc::new(sync_server::SyncShared { pair: Mutex::new(None) }),
                sync_server: Arc::new(Mutex::new(None)),
                backups_dir: data_dir.join("backups"),
            });
            // safety net: show the window even if page load never fires
            if let Some(w) = app.get_webview_window("main") {
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    let _ = w.show();
                });
            }
            #[cfg(not(target_os = "android"))]
            {
                let state = app.state::<AppState>();
                let (enabled, port, maximize) = {
                    let c = state.db.lock().unwrap();
                    let en = oplog::sync_get(&c, "sync_enabled").ok().flatten()
                        .is_some_and(|v| v == "1");
                    let p = oplog::sync_get(&c, "sync_port").ok().flatten()
                        .and_then(|v| v.parse().ok()).unwrap_or(47810u16);
                    // maximized unless the user turned it off
                    let m = queries::setting(&c, "start_maximized").ok().flatten()
                        .map(|v| v != "0").unwrap_or(true);
                    (en, p, m)
                };
                if maximize {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.maximize();
                    }
                }
                if enabled {
                    let ctx = Arc::new(sync_server::ServerCtx {
                        db: state.db.clone(), http: state.http.clone(),
                        shared: state.sync_shared.clone(), backups_dir: state.backups_dir.clone(),
                    });
                    let handle_slot = state.sync_server.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Ok(h) = sync_server::start(ctx, port).await {
                            *handle_slot.lock().unwrap() = Some(h);
                        }
                    });
                }
            }
            Ok(())
        })
        .on_page_load(|webview, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                let _ = webview.window().show();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::settings_all,
            commands::settings_set,
            commands::tmdb_validate_key,
            commands::import_run,
            commands::import_last_report,
            commands::refresh_run,
            commands::shows_list,
            commands::show_detail,
            commands::show_set_state,
            commands::show_toggle_favorite,
            commands::episode_mark,
            commands::season_mark,
            commands::episodes_unmark,
            commands::seasons_mark_before,
            commands::episodes_mark_before,
            commands::show_mark_all,
            commands::show_mark_next,
            commands::upcoming_list,
            commands::movies_list,
            commands::movie_set_state,
            commands::movie_rewatch,
            commands::movie_toggle_favorite,
            commands::search_tmdb,
            commands::show_add,
            commands::movie_add,
            commands::cache_info,
            commands::binge_list,
            commands::airing_list,
            commands::sync_server_status,
            commands::sync_server_set,
            commands::sync_pair_code,
            commands::sync_unpair,
            commands::anime_scan,
            commands::backup_export,
            commands::backup_import,
            commands::sync_pair,
            commands::sync_now,
            commands::sync_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
