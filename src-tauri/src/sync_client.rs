//! Phone-side sync client. Lives in Rust because the Android release WebView
//! blocks cleartext fetch(); reqwest sockets are exempt from that policy.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::error::{AppError, Result};
use crate::import::with_db;
use crate::oplog;
use crate::sync_proto::*;

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn base(addr: &str) -> String {
    format!("http://{addr}/sync/v1")
}

/// Pair with the desktop using the 6-digit code, then bootstrap from its snapshot.
pub async fn pair(http: &reqwest::Client, db: &Arc<Mutex<rusqlite::Connection>>,
                  addr: &str, code: &str) -> Result<(usize, usize)> {
    let device_id = with_db(db, |c| oplog::sync_get(c, "device_id")).await?
        .ok_or_else(|| AppError::msg("device id missing"))?;
    let resp = http.post(format!("{}/pair", base(addr)))
        .timeout(Duration::from_secs(10))
        .json(&PairRequest { device_id, code: code.to_string() })
        .send().await.map_err(|e| AppError::msg(format!("desktop unreachable: {e}")))?;
    if resp.status().as_u16() == 403 {
        return Err(AppError::msg(
            "wrong or expired pairing code (5 wrong tries cancel it: show a new code on the desktop)"));
    }
    let resp = resp.error_for_status().map_err(|e| AppError::msg(e.to_string()))?;
    let pr: PairResponse = resp.json().await.map_err(|e| AppError::msg(e.to_string()))?;

    let (token, peer, a) = (pr.token.clone(), pr.device_id.clone(), addr.to_string());
    with_db(db, move |c| {
        oplog::sync_set(c, "token", &token)?;
        oplog::sync_set(c, "paired_device", &peer)?;
        oplog::sync_set(c, "peer_addr", &a)
    }).await?;

    // snapshot bootstrap
    let snap: Snapshot = http.get(format!("{}/snapshot", base(addr)))
        .timeout(Duration::from_secs(120))
        .bearer_auth(&pr.token)
        .send().await.map_err(|e| AppError::msg(format!("snapshot: {e}")))?
        .error_for_status().map_err(|e| AppError::msg(e.to_string()))?
        .json().await.map_err(|e| AppError::msg(format!("snapshot decode: {e}")))?;
    let added = with_db(db, move |c| {
        let added = apply_snapshot(c, &snap)?;
        oplog::sync_set(c, "last_sync", &now_rfc3339())?;
        Ok(added)
    }).await?;
    Ok(added)
}

/// One full sync: pull peer ops since our cursor, apply, push our ops, advance cursors.
pub async fn sync_now(http: &reqwest::Client, db: &Arc<Mutex<rusqlite::Connection>>,
                      tmdb: Option<&crate::tmdb::TmdbClient>) -> Result<(usize, usize)> {
    let (addr, token, peer_cursor, push_cursor) = with_db(db, |c| {
        Ok((
            oplog::sync_get(c, "peer_addr")?,
            oplog::sync_get(c, "token")?,
            oplog::sync_get(c, "peer_cursor")?.and_then(|v| v.parse::<i64>().ok()).unwrap_or(0),
            oplog::sync_get(c, "push_cursor")?.and_then(|v| v.parse::<i64>().ok()).unwrap_or(0),
        ))
    }).await?;
    let addr = addr.ok_or_else(|| AppError::msg("not paired"))?;
    let token = token.ok_or_else(|| AppError::msg("not paired"))?;

    // pull
    let ch: ChangesResponse = http.get(format!("{}/changes?since={peer_cursor}", base(&addr)))
        .timeout(Duration::from_secs(30))
        .bearer_auth(&token)
        .send().await.map_err(|e| AppError::msg(format!("desktop unreachable: {e}")))?
        .error_for_status().map_err(|e| AppError::msg(e.to_string()))?
        .json().await.map_err(|e| AppError::msg(e.to_string()))?;
    let latest = ch.latest_seq;
    let out = oplog::apply_ops(db, tmdb.map(|t| t as &dyn crate::tmdb::Tmdb), ch.ops).await;
    let pulled = out.applied;
    let new_cursor = if out.error.is_none() { latest } else { out.last_seq.unwrap_or(peer_cursor) };
    with_db(db, move |c| oplog::sync_set(c, "peer_cursor", &new_cursor.to_string())).await?;
    if let Some(e) = out.error {
        return Err(AppError::msg(format!("pull applied partially: {e}")));
    }

    // push
    let ops = with_db(db, move |c| oplog::ops_since(c, push_cursor)).await?;
    let mut pushed = 0usize;
    if !ops.is_empty() {
        pushed = ops.len();
        let device_id = with_db(db, |c| oplog::sync_get(c, "device_id")).await?.unwrap_or_default();
        let pr: PushResponse = http.post(format!("{}/changes", base(&addr)))
            .timeout(Duration::from_secs(60))
            .bearer_auth(&token)
            .json(&PushRequest { device_id, ops })
            .send().await.map_err(|e| AppError::msg(format!("push: {e}")))?
            .error_for_status().map_err(|e| AppError::msg(e.to_string()))?
            .json().await.map_err(|e| AppError::msg(e.to_string()))?;
        if pr.applied_through > 0 {
            let at = pr.applied_through;
            with_db(db, move |c| oplog::sync_set(c, "push_cursor", &at.to_string())).await?;
        }
        if let Some(e) = pr.error {
            return Err(AppError::msg(format!("push applied partially: {e}")));
        }
    }
    with_db(db, move |c| oplog::sync_set(c, "last_sync", &now_rfc3339())).await?;
    Ok((pulled, pushed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    async fn serve(ctx: Arc<crate::sync_server::ServerCtx>) -> String {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, crate::sync_server::router(ctx)).await.unwrap();
        });
        format!("127.0.0.1:{}", addr.port())
    }

    #[tokio::test]
    async fn pair_bootstrap_and_two_way_sync_over_http() {
        // desktop DB with a show, one watch and a movie
        let desktop = Arc::new(Mutex::new(crate::db::open_memory().unwrap()));
        {
            let c = desktop.lock().unwrap();
            c.execute_batch(
                "INSERT INTO shows(tmdb_id,name,my_state,added_at) VALUES(1,'A','watching','2026-06-01T00:00:00Z');
                 INSERT INTO episodes(tmdb_id,show_tmdb_id,season_number,episode_number,air_date) VALUES
                   (10,1,1,1,'2026-06-01'),(11,1,1,2,'2026-06-08');
                 INSERT INTO movies(tmdb_id,title,my_state,added_at) VALUES(50,'M','watchlist','2026-06-01T00:00:00Z');
                 INSERT INTO settings(key,value) VALUES('tmdb_key','k');").unwrap();
            crate::oplog::episode_mark(&c, 10, true, "2026-07-01T00:00:00Z").unwrap();
        }
        let shared = Arc::new(crate::sync_server::SyncShared { pair: Mutex::new(None) });
        let ctx = Arc::new(crate::sync_server::ServerCtx {
            db: desktop.clone(), http: reqwest::Client::new(), shared: shared.clone(),
            backups_dir: std::env::temp_dir().join(format!("st-sync-it-{}", std::process::id())),
        });
        let addr = serve(ctx).await;
        let code = crate::sync_server::new_pair_code(&shared);

        // phone: fresh DB, pair + bootstrap
        let phone = Arc::new(Mutex::new(crate::db::open_memory().unwrap()));
        let http = reqwest::Client::new();
        let (shows, movies) = pair(&http, &phone, &addr, &code).await.unwrap();
        assert_eq!((shows, movies), (1, 1));
        {
            let c = phone.lock().unwrap();
            let n: i64 = c.query_row("SELECT COUNT(*) FROM episode_watches", [], |r| r.get(0)).unwrap();
            assert_eq!(n, 1); // bootstrap carried the watch
        }

        // couch-mark on the phone -> desktop sees it after sync
        {
            let c = phone.lock().unwrap();
            crate::oplog::episode_mark(&c, 11, true, "2026-07-02T00:00:00Z").unwrap();
        }
        let (pulled, pushed) = sync_now(&http, &phone, None).await.unwrap();
        assert_eq!(pushed, 1);
        {
            let c = desktop.lock().unwrap();
            let n: i64 = c.query_row("SELECT COUNT(*) FROM episode_watches WHERE episode_tmdb_id=11",
                                     [], |r| r.get(0)).unwrap();
            assert_eq!(n, 1);
        }

        // desktop-mark -> phone sees it after the next sync
        {
            let c = desktop.lock().unwrap();
            crate::oplog::movie_set_state(&c, 50, "watched", "2026-07-03T00:00:00Z").unwrap();
        }
        let (pulled2, _) = sync_now(&http, &phone, None).await.unwrap();
        assert!(pulled2 >= 1);
        {
            let c = phone.lock().unwrap();
            let st: String = c.query_row("SELECT my_state FROM movies WHERE tmdb_id=50", [], |r| r.get(0)).unwrap();
            assert_eq!(st, "watched");
        }
        // second sync is a no-op (cursors advanced)
        let (p3, s3) = sync_now(&http, &phone, None).await.unwrap();
        assert_eq!((p3, s3), (0, 0));
        let _ = pulled;
    }
}
