//! LAN sync server (desktop). Runs only while the app is open and the
//! Settings toggle is on. Home-LAN threat model: bearer token gates all
//! endpoints except /pair; no TLS by design (see spec).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use rand::RngCore;
use subtle::ConstantTimeEq;

use crate::error::Result;
use crate::import::with_db;
use crate::oplog;
use crate::sync_proto::*;

pub struct PairCode {
    pub code: String,
    pub expires: Instant,
}

pub struct SyncShared {
    pub pair: Mutex<Option<PairCode>>,
}

pub struct ServerCtx {
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub http: reqwest::Client,
    pub shared: Arc<SyncShared>,
    pub backups_dir: PathBuf,
}

pub struct SyncServerHandle {
    pub port: u16,
    pub shutdown: tokio::sync::watch::Sender<bool>,
    pub task: tauri::async_runtime::JoinHandle<()>,
}

pub fn mint_token() -> String {
    let mut b = [0u8; 32];
    rand::rng().fill_bytes(&mut b);
    oplog::hex(&b)
}

pub fn new_pair_code(shared: &Arc<SyncShared>) -> String {
    let code = format!("{:06}", rand::rng().next_u32() % 1_000_000);
    *shared.pair.lock().unwrap() = Some(PairCode {
        code: code.clone(),
        expires: Instant::now() + Duration::from_secs(300),
    });
    code
}

fn err(status: StatusCode, msg: &str) -> Response {
    (status, Json(serde_json::json!({"error": msg}))).into_response()
}

async fn check_auth(ctx: &ServerCtx, headers: &axum::http::HeaderMap) -> std::result::Result<(), Response> {
    let given = headers.get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let stored = with_db(&ctx.db, |c| oplog::sync_get(c, "token")).await
        .ok().flatten().unwrap_or_default();
    if stored.is_empty() || given.as_bytes().ct_eq(stored.as_bytes()).unwrap_u8() == 0 {
        return Err(err(StatusCode::UNAUTHORIZED, "missing or wrong token"));
    }
    Ok(())
}

async fn pair(State(ctx): State<Arc<ServerCtx>>, Json(req): Json<PairRequest>) -> Response {
    {
        let mut slot = ctx.shared.pair.lock().unwrap();
        let ok = match slot.as_ref() {
            Some(p) => p.expires > Instant::now()
                && req.code.as_bytes().ct_eq(p.code.as_bytes()).unwrap_u8() == 1,
            None => false,
        };
        if !ok {
            return err(StatusCode::FORBIDDEN, "wrong or expired pairing code");
        }
        *slot = None; // one use
    }
    let token = mint_token();
    let t = token.clone();
    let peer = req.device_id.clone();
    let stored = with_db(&ctx.db, move |c| {
        oplog::sync_set(c, "token", &t)?;
        oplog::sync_set(c, "paired_device", &peer)?;
        oplog::sync_get(c, "device_id")
    }).await;
    match stored {
        Ok(Some(device_id)) => Json(PairResponse { token, device_id }).into_response(),
        Ok(None) => err(StatusCode::INTERNAL_SERVER_ERROR, "device id missing"),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn snapshot(State(ctx): State<Arc<ServerCtx>>, headers: axum::http::HeaderMap) -> Response {
    if let Err(r) = check_auth(&ctx, &headers).await { return r; }
    match with_db(&ctx.db, crate::sync_proto::build_snapshot).await {
        Ok(s) => Json(s).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

#[derive(serde::Deserialize)]
struct SinceQ { since: i64 }

async fn get_changes(State(ctx): State<Arc<ServerCtx>>, headers: axum::http::HeaderMap,
                     Query(q): Query<SinceQ>) -> Response {
    if let Err(r) = check_auth(&ctx, &headers).await { return r; }
    let since = q.since;
    match with_db(&ctx.db, move |c| {
        Ok(ChangesResponse { ops: oplog::ops_since(c, since)?, latest_seq: oplog::latest_seq(c)? })
    }).await {
        Ok(r) => Json(r).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// Rolling pre-apply backup, keep the newest 5.
pub async fn backup_db(ctx: &ServerCtx) -> Result<()> {
    std::fs::create_dir_all(&ctx.backups_dir)?;
    let name = format!("showtrack-{}.db", chrono::Local::now().format("%Y%m%d-%H%M%S%.3f"));
    let target = ctx.backups_dir.join(name);
    let path = target.to_string_lossy().to_string();
    with_db(&ctx.db, move |c| {
        c.execute("VACUUM INTO ?1", [path])?;
        Ok(())
    }).await?;
    let mut entries: Vec<_> = std::fs::read_dir(&ctx.backups_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("showtrack-"))
        .collect();
    entries.sort_by_key(|e| e.file_name());
    while entries.len() > 5 {
        let old = entries.remove(0);
        let _ = std::fs::remove_file(old.path());
    }
    Ok(())
}

async fn post_changes(State(ctx): State<Arc<ServerCtx>>, headers: axum::http::HeaderMap,
                      Json(req): Json<PushRequest>) -> Response {
    if let Err(r) = check_auth(&ctx, &headers).await { return r; }
    if let Err(e) = backup_db(&ctx).await {
        return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("backup failed: {e}"));
    }
    let key = with_db(&ctx.db, |c| crate::queries::setting(c, "tmdb_key")).await.ok().flatten();
    let tmdb = key.map(|k| crate::tmdb::TmdbClient::new(ctx.http.clone(), k));
    let out = oplog::apply_ops(&ctx.db, tmdb.as_ref().map(|t| t as &dyn crate::tmdb::Tmdb), req.ops).await;
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let _ = with_db(&ctx.db, move |c| oplog::sync_set(c, "last_push_at", &now)).await;
    Json(PushResponse { applied_through: out.last_seq.unwrap_or(0), error: out.error }).into_response()
}

pub fn router(ctx: Arc<ServerCtx>) -> Router {
    Router::new()
        .route("/sync/v1/pair", post(pair))
        .route("/sync/v1/snapshot", get(snapshot))
        .route("/sync/v1/changes", get(get_changes).post(post_changes))
        .with_state(ctx)
}

pub async fn start(ctx: Arc<ServerCtx>, port: u16) -> Result<SyncServerHandle> {
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await
        .map_err(|e| crate::error::AppError::msg(format!("port {port}: {e}")))?;
    let (tx, mut rx) = tokio::sync::watch::channel(false);
    let task = tauri::async_runtime::spawn(async move {
        let _ = axum::serve(listener, router(ctx))
            .with_graceful_shutdown(async move { let _ = rx.changed().await; })
            .await;
    });
    Ok(SyncServerHandle { port, shutdown: tx, task })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::util::ServiceExt;

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let b = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&b).unwrap()
    }

    fn test_ctx() -> Arc<ServerCtx> {
        let conn = crate::db::open_memory().unwrap();
        Arc::new(ServerCtx {
            db: Arc::new(std::sync::Mutex::new(conn)),
            http: reqwest::Client::new(),
            shared: Arc::new(SyncShared { pair: std::sync::Mutex::new(None) }),
            backups_dir: std::env::temp_dir().join(format!("st-backups-{}", std::process::id())),
        })
    }

    #[tokio::test]
    async fn pair_rejects_wrong_expired_and_missing_codes() {
        let ctx = test_ctx();
        let app = router(ctx.clone());
        let req = |code: &str| Request::post("/sync/v1/pair")
            .header("content-type", "application/json")
            .body(Body::from(format!(r#"{{"device_id":"phone1","code":"{code}"}}"#))).unwrap();
        // no code generated yet
        assert_eq!(app.clone().oneshot(req("123456")).await.unwrap().status(), StatusCode::FORBIDDEN);
        // wrong code
        let good = new_pair_code(&ctx.shared);
        assert_eq!(app.clone().oneshot(req("000000")).await.unwrap().status(), StatusCode::FORBIDDEN);
        // right code works once and mints a 64-char hex token
        let resp = app.clone().oneshot(req(&good)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["token"].as_str().unwrap().len(), 64);
        // one use only
        assert_eq!(app.clone().oneshot(req(&good)).await.unwrap().status(), StatusCode::FORBIDDEN);
        // expired
        let code = new_pair_code(&ctx.shared);
        ctx.shared.pair.lock().unwrap().as_mut().unwrap().expires =
            std::time::Instant::now() - std::time::Duration::from_secs(1);
        assert_eq!(app.clone().oneshot(req(&code)).await.unwrap().status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn endpoints_require_the_bearer_token() {
        let ctx = test_ctx();
        {
            let c = ctx.db.lock().unwrap();
            crate::oplog::sync_set(&c, "token", "aaaa").unwrap();
        }
        let app = router(ctx);
        let no_auth = Request::get("/sync/v1/changes?since=0").body(Body::empty()).unwrap();
        assert_eq!(app.clone().oneshot(no_auth).await.unwrap().status(), StatusCode::UNAUTHORIZED);
        let bad = Request::get("/sync/v1/changes?since=0")
            .header("authorization", "Bearer bbbb").body(Body::empty()).unwrap();
        assert_eq!(app.clone().oneshot(bad).await.unwrap().status(), StatusCode::UNAUTHORIZED);
        let good = Request::get("/sync/v1/changes?since=0")
            .header("authorization", "Bearer aaaa").body(Body::empty()).unwrap();
        let resp = app.clone().oneshot(good).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["latest_seq"], 0);
    }

    #[tokio::test]
    async fn push_takes_a_backup_and_applies_ops() {
        let ctx = test_ctx();
        {
            let c = ctx.db.lock().unwrap();
            crate::oplog::sync_set(&c, "token", "aaaa").unwrap();
            c.execute_batch(
                "INSERT INTO shows(tmdb_id,name,my_state,added_at) VALUES(1,'A','watching','2026-06-01T00:00:00Z');
                 INSERT INTO episodes(tmdb_id,show_tmdb_id,season_number,episode_number,air_date)
                   VALUES(10,1,1,1,'2026-06-01');").unwrap();
        }
        let app = router(ctx.clone());
        let body = serde_json::json!({
            "device_id": "phone1",
            "ops": [{"seq": 7, "ts": "2026-07-10T00:00:00Z", "entity": "ep", "entity_id": 10,
                     "op": "watch", "payload": {"watched_at": "2026-07-10T00:00:00Z", "show": 1}}]
        });
        let req = Request::post("/sync/v1/changes")
            .header("authorization", "Bearer aaaa")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v = body_json(resp).await;
        assert_eq!(v["applied_through"], 7);
        let c = ctx.db.lock().unwrap();
        let n: i64 = c.query_row("SELECT COUNT(*) FROM episode_watches WHERE episode_tmdb_id=10",
                                 [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        let backups = std::fs::read_dir(&ctx.backups_dir).unwrap().count();
        assert_eq!(backups, 1);
    }

    #[tokio::test]
    async fn restart_on_same_port_succeeds_after_awaited_shutdown() {
        let ctx = test_ctx();
        let port = 47899; // fixed test port, distinct from the real default
        let h1 = start(ctx.clone(), port).await.unwrap();
        let _ = h1.shutdown.send(true);
        let _ = h1.task.await;
        // The listener is provably closed once the task above has finished,
        // so rebinding the same port immediately must not race AddrInUse.
        let h2 = start(ctx.clone(), port).await;
        assert!(h2.is_ok());
        let h2 = h2.unwrap();
        let _ = h2.shutdown.send(true);
        let _ = h2.task.await;
    }
}
