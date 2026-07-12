use std::collections::HashMap;
use std::sync::atomic::Ordering;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::import::with_db;
use crate::models::*;
use crate::tmdb::{Tmdb, TmdbClient};
use crate::{binge, export, images, import, oplog, queries, refresh, AppState};

type CmdResult<T> = std::result::Result<T, String>;

fn today_local() -> String {
    chrono::Local::now().date_naive().to_string()
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Build a TMDB client from the stored key, or explain what's missing.
async fn client(state: &State<'_, AppState>) -> CmdResult<TmdbClient> {
    let key = with_db(&state.db, |c| queries::setting(c, "tmdb_key")).await
        .map_err(|e| e.to_string())?
        .ok_or("TMDB API key not set. Add it in Settings.")?;
    Ok(TmdbClient::new(state.http.clone(), key))
}

struct BusyGuard(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl BusyGuard {
    fn acquire(state: &AppState) -> CmdResult<Self> {
        if state.busy.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return Err("another job is already running".into());
        }
        Ok(BusyGuard(state.busy.clone()))
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

// ---------- settings ----------

#[tauri::command]
pub async fn settings_all(state: State<'_, AppState>) -> CmdResult<HashMap<String, String>> {
    with_db(&state.db, |c| {
        let mut st = c.prepare("SELECT key, value FROM settings")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<HashMap<_, _>, _>>()?)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn settings_set(state: State<'_, AppState>, key: String, value: String) -> CmdResult<()> {
    with_db(&state.db, move |c| queries::set_setting(c, &key, &value)).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tmdb_validate_key(state: State<'_, AppState>, key: String) -> CmdResult<bool> {
    let c = TmdbClient::new(state.http.clone(), key.clone());
    let ok = c.validate().await.map_err(|e| e.to_string())?;
    if ok {
        with_db(&state.db, move |c| queries::set_setting(c, "tmdb_key", &key)).await
            .map_err(|e| e.to_string())?;
    }
    Ok(ok)
}

// ---------- import ----------

#[tauri::command]
pub async fn import_run(app: AppHandle, state: State<'_, AppState>, path: String) -> CmdResult<()> {
    let guard = BusyGuard::acquire(&state)?;
    let tmdb = client(&state).await?;
    let html = tokio::fs::read_to_string(&path).await.map_err(|e| e.to_string())?;
    let parsed = export::parse_export(&html).map_err(|e| e.to_string())?;
    let db = state.db.clone();
    tauri::async_runtime::spawn(async move {
        let _guard = guard; // released (and busy cleared) when the job ends, even on panic-free error paths
        let emitter = app.clone();
        let report = import::run_import(&tmdb, db.clone(), parsed, move |p| {
            let _ = emitter.emit("import:progress", &p);
        })
        .await;
        if let Ok(json) = serde_json::to_string(&report) {
            let _ = with_db(&db, move |c| queries::set_setting(c, "last_import_report", &json)).await;
        }
        let _ = app.emit("import:done", &report);
    });
    Ok(())
}

#[tauri::command]
pub async fn import_last_report(state: State<'_, AppState>) -> CmdResult<Option<ImportReport>> {
    let json = with_db(&state.db, |c| queries::setting(c, "last_import_report")).await
        .map_err(|e| e.to_string())?;
    Ok(json.and_then(|j| serde_json::from_str(&j).ok()))
}

// ---------- refresh ----------

#[tauri::command]
pub async fn refresh_run(state: State<'_, AppState>) -> CmdResult<RefreshSummary> {
    let _guard = BusyGuard::acquire(&state)?;
    let tmdb = client(&state).await?;
    Ok(refresh::run_refresh(&tmdb, state.db.clone()).await)
}

// ---------- shows ----------

#[tauri::command]
pub async fn shows_list(state: State<'_, AppState>) -> CmdResult<Vec<ShowCard>> {
    with_db(&state.db, |c| queries::list_show_cards(c, &today_local())).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn show_detail(state: State<'_, AppState>, id: i64) -> CmdResult<ShowDetail> {
    with_db(&state.db, move |c| queries::show_detail(c, id)).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn show_set_state(state: State<'_, AppState>, id: i64, my_state: String) -> CmdResult<()> {
    with_db(&state.db, move |c| oplog::show_set_state(c, id, &my_state, &now_rfc3339())).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn show_toggle_favorite(state: State<'_, AppState>, id: i64) -> CmdResult<bool> {
    with_db(&state.db, move |c| oplog::show_toggle_favorite(c, id, &now_rfc3339())).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn episode_mark(state: State<'_, AppState>, id: i64, watched: bool) -> CmdResult<()> {
    with_db(&state.db, move |c| oplog::episode_mark(c, id, watched, &now_rfc3339())).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn season_mark(state: State<'_, AppState>, show_id: i64, season: i64, watched: bool) -> CmdResult<Vec<i64>> {
    with_db(&state.db, move |c| {
        if watched {
            oplog::season_mark_collect(c, show_id, season, &today_local(), &now_rfc3339())
        } else {
            oplog::season_unmark(c, show_id, season, &today_local(), &now_rfc3339())?;
            Ok(Vec::new())
        }
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn episodes_unmark(state: State<'_, AppState>, ids: Vec<i64>) -> CmdResult<()> {
    with_db(&state.db, move |c| oplog::episodes_unmark(c, &ids, &now_rfc3339())).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn seasons_mark_before(state: State<'_, AppState>, show_id: i64, season: i64) -> CmdResult<Vec<i64>> {
    with_db(&state.db, move |c| {
        oplog::seasons_mark_before(c, show_id, season, &today_local(), &now_rfc3339())
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn episodes_mark_before(state: State<'_, AppState>, show_id: i64, season: i64, episode: i64) -> CmdResult<Vec<i64>> {
    with_db(&state.db, move |c| {
        oplog::episodes_mark_before(c, show_id, season, episode, &today_local(), &now_rfc3339())
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn show_mark_all(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    with_db(&state.db, move |c| oplog::mark_all_aired(c, id, &today_local(), &now_rfc3339())).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn show_mark_next(state: State<'_, AppState>, id: i64) -> CmdResult<ShowCard> {
    with_db(&state.db, move |c| {
        let today = today_local();
        let card = queries::show_card(c, id, &today)?;
        if let Some(next) = card.next {
            oplog::episode_mark(c, next.episode_id, true, &now_rfc3339())?;
        }
        queries::show_card(c, id, &today)
    })
    .await
    .map_err(|e| e.to_string())
}

// ---------- upcoming / movies ----------

#[tauri::command]
pub async fn upcoming_list(state: State<'_, AppState>) -> CmdResult<Vec<UpcomingItem>> {
    with_db(&state.db, |c| queries::list_upcoming(c, &today_local())).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn movies_list(state: State<'_, AppState>) -> CmdResult<Vec<MovieRow>> {
    with_db(&state.db, |c| queries::list_movies(c)).await.map_err(|e| e.to_string())
}

// ---------- binge (redesign) ----------

#[tauri::command]
pub async fn binge_list(state: State<'_, AppState>) -> CmdResult<Vec<BingeCard>> {
    with_db(&state.db, |c| binge::build_binge_cards(c, &today_local())).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn airing_list(state: State<'_, AppState>) -> CmdResult<Vec<AiringRow>> {
    with_db(&state.db, |c| binge::build_airing_rows(c, &today_local())).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn movie_set_state(state: State<'_, AppState>, id: i64, my_state: String) -> CmdResult<()> {
    with_db(&state.db, move |c| oplog::movie_set_state(c, id, &my_state, &now_rfc3339())).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn movie_rewatch(state: State<'_, AppState>, id: i64) -> CmdResult<i64> {
    with_db(&state.db, move |c| oplog::movie_rewatch(c, id, &now_rfc3339())).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn movie_set_rewatch(state: State<'_, AppState>, id: i64, count: i64) -> CmdResult<i64> {
    with_db(&state.db, move |c| oplog::movie_set_rewatch(c, id, count, &now_rfc3339())).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn show_remove(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    with_db(&state.db, move |c| oplog::show_remove(c, id, &now_rfc3339())).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn movie_remove(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    with_db(&state.db, move |c| oplog::movie_remove(c, id, &now_rfc3339())).await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn movie_toggle_favorite(state: State<'_, AppState>, id: i64) -> CmdResult<bool> {
    with_db(&state.db, move |c| oplog::movie_toggle_favorite(c, id, &now_rfc3339())).await
        .map_err(|e| e.to_string())
}

// ---------- search / add ----------

#[tauri::command]
pub async fn search_tmdb(state: State<'_, AppState>, query: String) -> CmdResult<Vec<SearchItem>> {
    let tmdb = client(&state).await?;
    let results = tmdb.search(&query).await.map_err(|e| e.to_string())?;
    let (show_ids, movie_ids) = with_db(&state.db, |c| queries::library_ids(c)).await
        .map_err(|e| e.to_string())?;
    Ok(results.into_iter().map(|r| {
        let lib_state = match r.media_type.as_str() {
            // cross-filed entries live in the other table under a negative id
            "tv" => show_ids.get(&r.tmdb_id).or_else(|| movie_ids.get(&-r.tmdb_id)),
            _ => movie_ids.get(&r.tmdb_id).or_else(|| show_ids.get(&-r.tmdb_id)),
        }.cloned();
        SearchItem {
            tmdb_id: r.tmdb_id, media_type: r.media_type, title: r.title,
            year: r.year, poster_path: r.poster_path, overview: r.overview,
            in_library: lib_state.is_some(), lib_state,
        }
    }).collect())
}

#[tauri::command]
pub async fn show_add(state: State<'_, AppState>, tmdb_id: i64, my_state: String) -> CmdResult<()> {
    let tmdb = client(&state).await?;
    let full = tmdb.show_with_episodes(tmdb_id).await.map_err(|e| e.to_string())?;
    with_db(&state.db, move |c| {
        queries::upsert_show_meta(c, &full.meta, &now_rfc3339())?;
        queries::set_show_user(c, full.meta.tmdb_id, &my_state, false, None, None)?;
        queries::upsert_episodes(c, full.meta.tmdb_id, &full.episodes)?;
        oplog::log_show_add(c, full.meta.tmdb_id, &my_state, "tv", full.meta.is_anime, &now_rfc3339())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn movie_add(state: State<'_, AppState>, tmdb_id: i64, my_state: String) -> CmdResult<()> {
    let tmdb = client(&state).await?;
    let meta = tmdb.movie(tmdb_id).await.map_err(|e| e.to_string())?;
    with_db(&state.db, move |c| {
        let now = now_rfc3339();
        queries::upsert_movie_meta(c, &meta, &now)?;
        let watched_at = (my_state == "watched").then(|| now.clone());
        queries::set_movie_user(c, meta.tmdb_id, &my_state, watched_at.as_deref(), 0, false, None, None)?;
        oplog::log_movie_add(c, meta.tmdb_id, &my_state, watched_at.as_deref(), "movie", &now)?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())
}

// ---------- cross-type add ----------

/// Add a searched title into either section, regardless of which type TMDB
/// filed it under. Cross-filed entries store the other type's record with a
/// NEGATIVE local id (see tmdb::ShowMeta::from_movie / MovieMeta::from_show).
#[tauri::command]
pub async fn title_add(state: State<'_, AppState>, tmdb_id: i64, src: String,
                       target: String, my_state: String) -> CmdResult<()> {
    let tmdb = client(&state).await?;
    match target.as_str() {
        "movie" => {
            let (meta, op_src) = if src == "tv" {
                let show = tmdb.tv_base(tmdb_id).await.map_err(|e| e.to_string())?;
                (crate::tmdb::MovieMeta::from_show(&show), "tv")
            } else {
                (tmdb.movie(tmdb_id).await.map_err(|e| e.to_string())?, "movie")
            };
            with_db(&state.db, move |c| {
                let now = now_rfc3339();
                queries::upsert_movie_meta(c, &meta, &now)?;
                let watched_at = (my_state == "watched").then(|| now.clone());
                queries::set_movie_user(c, meta.tmdb_id, &my_state, watched_at.as_deref(), 0, false, None, None)?;
                oplog::log_movie_add(c, meta.tmdb_id, &my_state, watched_at.as_deref(), op_src, &now)
            })
            .await
            .map_err(|e| e.to_string())
        }
        "show" | "anime" => {
            let as_anime = target == "anime";
            let (full, op_src) = if src == "tv" {
                (tmdb.show_with_episodes(tmdb_id).await.map_err(|e| e.to_string())?, "tv")
            } else {
                let movie = tmdb.movie(tmdb_id).await.map_err(|e| e.to_string())?;
                (crate::tmdb::ShowFull {
                    meta: crate::tmdb::ShowMeta::from_movie(&movie),
                    episodes: Vec::new(),
                }, "movie")
            };
            // "watched_all" is a UI intent, not a state: store watching and
            // mark everything aired, exactly like the show page button
            let mark_all = my_state == "watched_all";
            let store_state = if mark_all { "watching".to_string() } else { my_state };
            with_db(&state.db, move |c| {
                let now = now_rfc3339();
                queries::upsert_show_meta(c, &full.meta, &now)?;
                if as_anime {
                    // the user's section choice outranks TMDB detection
                    c.execute("UPDATE shows SET is_anime=1 WHERE tmdb_id=?1", [full.meta.tmdb_id])?;
                }
                queries::set_show_user(c, full.meta.tmdb_id, &store_state, false, None, None)?;
                queries::upsert_episodes(c, full.meta.tmdb_id, &full.episodes)?;
                oplog::log_show_add(c, full.meta.tmdb_id, &store_state, op_src,
                                    as_anime || full.meta.is_anime, &now)?;
                if mark_all {
                    oplog::mark_all_aired(c, full.meta.tmdb_id, &today_local(), &now)?;
                }
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())
        }
        _ => Err("unknown add target".into()),
    }
}

// ---------- updater ----------

#[tauri::command]
pub async fn update_check(state: State<'_, AppState>) -> CmdResult<Option<crate::updater::UpdateInfo>> {
    crate::updater::check(&state.http).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_install(app: AppHandle, state: State<'_, AppState>, url: String) -> CmdResult<()> {
    #[cfg(target_os = "android")]
    {
        let _ = (&app, &state, &url);
        Err("on Android, install the new APK from the releases page".into())
    }
    #[cfg(not(target_os = "android"))]
    {
        let path = crate::updater::download_installer(&state.http, &url).await
            .map_err(|e| e.to_string())?;
        let launch = if crate::updater::is_portable() {
            // no installer to run: the new exe takes this file's place
            crate::updater::swap_running_exe(&path).map_err(|e| e.to_string())?
        } else {
            path
        };
        std::process::Command::new(&launch).spawn().map_err(|e| e.to_string())?;
        app.exit(0);
        Ok(())
    }
}

// ---------- misc ----------

#[tauri::command]
pub async fn cache_info(app: AppHandle) -> CmdResult<CacheInfo> {
    let root = app.path().app_data_dir().map_err(|e| e.to_string())?.join("images");
    let path = root.display().to_string();
    let bytes = tauri::async_runtime::spawn_blocking(move || images::dir_size(&root)).await
        .map_err(|e| e.to_string())?;
    Ok(CacheInfo { path, bytes })
}

// ---------- sync (desktop server side) ----------

use crate::sync_server;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncServerStatus {
    pub running: bool,
    pub enabled: bool,
    pub port: u16,
    pub paired_device: Option<String>,
    pub last_push_at: Option<String>,
}

async fn server_status(state: &State<'_, AppState>) -> CmdResult<SyncServerStatus> {
    let running = state.sync_server.lock().unwrap().is_some();
    let st = with_db(&state.db, |c| {
        Ok((
            crate::oplog::sync_get(c, "sync_enabled")?.is_some_and(|v| v == "1"),
            crate::oplog::sync_get(c, "sync_port")?.and_then(|v| v.parse().ok()).unwrap_or(47810u16),
            crate::oplog::sync_get(c, "paired_device")?,
            crate::oplog::sync_get(c, "last_push_at")?,
        ))
    }).await.map_err(|e| e.to_string())?;
    Ok(SyncServerStatus {
        running, enabled: st.0, port: st.1, paired_device: st.2, last_push_at: st.3,
    })
}

#[tauri::command]
pub async fn sync_server_status(state: State<'_, AppState>) -> CmdResult<SyncServerStatus> {
    server_status(&state).await
}

#[tauri::command]
pub async fn sync_server_set(state: State<'_, AppState>, enabled: bool, port: u16)
    -> CmdResult<SyncServerStatus> {
    with_db(&state.db, move |c| {
        crate::oplog::sync_set(c, "sync_enabled", if enabled { "1" } else { "0" })?;
        crate::oplog::sync_set(c, "sync_port", &port.to_string())
    }).await.map_err(|e| e.to_string())?;
    // stop whatever is running, and wait for the listener to actually close
    // before rebinding, so a same-port restart never races the old socket.
    let old = state.sync_server.lock().unwrap().take();
    if let Some(h) = old {
        let _ = h.shutdown.send(true);
        let _ = h.task.await;
    }
    if enabled {
        let ctx = std::sync::Arc::new(sync_server::ServerCtx {
            db: state.db.clone(), http: state.http.clone(),
            shared: state.sync_shared.clone(), backups_dir: state.backups_dir.clone(),
        });
        match sync_server::start(ctx, port).await {
            Ok(h) => *state.sync_server.lock().unwrap() = Some(h),
            Err(e) => {
                // Roll back so sync_state does not claim enabled when nothing is running.
                let _ = with_db(&state.db, |c| crate::oplog::sync_set(c, "sync_enabled", "0")).await;
                return Err(e.to_string());
            }
        }
    }
    server_status(&state).await
}

#[tauri::command]
pub async fn sync_pair_code(state: State<'_, AppState>) -> CmdResult<String> {
    if state.sync_server.lock().unwrap().is_none() {
        return Err("turn phone sync on first".into());
    }
    Ok(sync_server::new_pair_code(&state.sync_shared))
}

#[tauri::command]
pub async fn sync_unpair(state: State<'_, AppState>) -> CmdResult<()> {
    with_db(&state.db, |c| {
        for k in ["token", "paired_device", "peer_addr", "peer_cursor",
                  "push_cursor", "last_sync", "last_push_at"] {
            crate::oplog::sync_del(c, k)?;
        }
        Ok(())
    }).await.map_err(|e| e.to_string())
}

// ---------- anime ----------

/// Re-check every show on TMDB and flag the anime. One-time backfill for
/// libraries that predate the is_anime column; new adds are flagged inline.
#[tauri::command]
pub async fn anime_scan(state: State<'_, AppState>) -> CmdResult<i64> {
    let _guard = BusyGuard::acquire(&state)?;
    let tmdb = client(&state).await?;
    let ids: Vec<i64> = with_db(&state.db, |c| {
        // negative ids are cross-filed movie records with no TV entry to check
        let mut st = c.prepare("SELECT tmdb_id FROM shows WHERE tmdb_id > 0 ORDER BY tmdb_id")?;
        let v = st.query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
        Ok(v)
    })
    .await
    .map_err(|e| e.to_string())?;
    let mut found = 0i64;
    for id in ids {
        // shows TMDB no longer knows keep their current flag
        if let Ok(meta) = tmdb.tv_base(id).await {
            if meta.is_anime {
                found += 1;
            }
            // raise-only: a user-forced anime flag survives detection saying no
            if meta.is_anime {
                with_db(&state.db, move |c| queries::set_show_anime(c, id, true))
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    with_db(&state.db, |c| queries::set_setting(c, "anime_scan_done", "1")).await
        .map_err(|e| e.to_string())?;
    Ok(found)
}

// ---------- backup ----------

#[tauri::command]
pub async fn backup_export(state: State<'_, AppState>, path: String) -> CmdResult<()> {
    let _guard = BusyGuard::acquire(&state)?;
    let snap = with_db(&state.db, crate::sync_proto::build_snapshot).await
        .map_err(|e| e.to_string())?;
    let json = serde_json::to_vec(&snap).map_err(|e| e.to_string())?;
    tokio::fs::write(&path, json).await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn backup_import(state: State<'_, AppState>, path: String) -> CmdResult<PairSummary> {
    let _guard = BusyGuard::acquire(&state)?;
    let bytes = tokio::fs::read(&path).await.map_err(|e| e.to_string())?;
    let snap: crate::sync_proto::Snapshot = serde_json::from_slice(&bytes)
        .map_err(|_| "that file is not a ShowTrack backup".to_string())?;
    let (shows, movies) = with_db(&state.db, move |c| crate::sync_proto::apply_library(c, &snap))
        .await
        .map_err(|e| e.to_string())?;
    Ok(PairSummary { shows, movies })
}

// ---------- sync (phone client side) ----------

#[derive(serde::Serialize)]
pub struct PairSummary { pub shows: usize, pub movies: usize }

#[derive(serde::Serialize)]
pub struct SyncSummary { pub pulled: usize, pub pushed: usize }

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub paired: bool,
    pub peer_addr: Option<String>,
    pub last_sync: Option<String>,
}

#[tauri::command]
pub async fn sync_pair(state: State<'_, AppState>, addr: String, code: String) -> CmdResult<PairSummary> {
    let _guard = BusyGuard::acquire(&state)?;
    let (shows, movies) = crate::sync_client::pair(&state.http, &state.db, &addr, &code).await
        .map_err(|e| e.to_string())?;
    Ok(PairSummary { shows, movies })
}

#[tauri::command]
pub async fn sync_now(state: State<'_, AppState>) -> CmdResult<SyncSummary> {
    let _guard = BusyGuard::acquire(&state)?;
    let tmdb = client(&state).await.ok();
    let (pulled, pushed) = crate::sync_client::sync_now(&state.http, &state.db, tmdb.as_ref()).await
        .map_err(|e| e.to_string())?;
    Ok(SyncSummary { pulled, pushed })
}

#[tauri::command]
pub async fn sync_status(state: State<'_, AppState>) -> CmdResult<SyncStatus> {
    with_db(&state.db, |c| {
        Ok(SyncStatus {
            paired: crate::oplog::sync_get(c, "token")?.is_some()
                && crate::oplog::sync_get(c, "peer_addr")?.is_some(),
            peer_addr: crate::oplog::sync_get(c, "peer_addr")?,
            last_sync: crate::oplog::sync_get(c, "last_sync")?,
        })
    }).await.map_err(|e| e.to_string())
}
