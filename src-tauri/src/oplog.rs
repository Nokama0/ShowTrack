//! Change tracking for LAN sync: every local mutation appends an op here.
//! The oplog holds ONLY local-origin ops; applying peer ops writes tables
//! directly (see apply_* below) and never appends.

use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Op {
    pub seq: i64,
    pub ts: String,
    pub entity: String, // "ep" | "show" | "movie"
    pub entity_id: i64,
    pub op: String,
    pub payload: Option<serde_json::Value>,
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------- sync_state ----------

pub fn sync_get(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn.query_row("SELECT value FROM sync_state WHERE key=?1", [key], |r| r.get(0)).optional()?)
}

pub fn sync_set(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_state(key,value) VALUES(?1,?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value])?;
    Ok(())
}

pub fn sync_del(conn: &Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM sync_state WHERE key=?1", [key])?;
    Ok(())
}

// ---------- append / read ----------

pub fn append(conn: &Connection, entity: &str, entity_id: i64, op: &str,
              payload: Option<serde_json::Value>, ts: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO oplog(ts,entity,entity_id,op,payload) VALUES(?1,?2,?3,?4,?5)",
        params![ts, entity, entity_id.to_string(), op, payload.map(|p| p.to_string())])?;
    Ok(())
}

pub fn ops_since(conn: &Connection, since: i64) -> Result<Vec<Op>> {
    let mut st = conn.prepare(
        "SELECT seq, ts, entity, entity_id, op, payload FROM oplog WHERE seq>?1 ORDER BY seq")?;
    let rows = st.query_map([since], |r| {
        Ok(Op {
            seq: r.get(0)?, ts: r.get(1)?, entity: r.get(2)?,
            entity_id: r.get::<_, String>(3)?.parse().unwrap_or(0),
            op: r.get(4)?,
            payload: r.get::<_, Option<String>>(5)?
                .and_then(|s| serde_json::from_str(&s).ok()),
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn latest_seq(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COALESCE(MAX(seq),0) FROM oplog", [], |r| r.get(0))?)
}

/// Convert an existing (pre-sync) library into ops. Android-only, at migration:
/// lets the phone push its pre-pairing data to the desktop. Returns ops written.
pub fn seed_from_library(conn: &Connection) -> Result<usize> {
    let mut n = 0usize;
    let shows: Vec<(i64, String, i64, String)> = conn.prepare(
        "SELECT tmdb_id, my_state, is_favorite, added_at FROM shows")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<std::result::Result<_, _>>()?;
    for (id, state, fav, added) in shows {
        let src = if id < 0 { "movie" } else { "tv" };
        append(conn, "show", id, "add", Some(serde_json::json!({"state": state, "src": src})), &added)?;
        n += 1;
        if fav != 0 {
            append(conn, "show", id, "favorite", Some(serde_json::json!({"value": true})), &added)?;
            n += 1;
        }
    }
    let watches: Vec<(i64, Option<String>, i64)> = conn.prepare(
        "SELECT w.episode_tmdb_id, w.watched_at, e.show_tmdb_id
         FROM episode_watches w JOIN episodes e ON e.tmdb_id=w.episode_tmdb_id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<_, _>>()?;
    for (ep, at, show) in watches {
        let ts = at.clone().unwrap_or_else(|| "1970-01-01T00:00:00Z".into());
        append(conn, "ep", ep, "watch",
               Some(serde_json::json!({"watched_at": at, "show": show})), &ts)?;
        n += 1;
    }
    let movies: Vec<(i64, String, Option<String>, i64, i64, String)> = conn.prepare(
        "SELECT tmdb_id, my_state, watched_at, rewatch_count, is_favorite, added_at FROM movies")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?
        .collect::<std::result::Result<_, _>>()?;
    for (id, state, at, rew, fav, added) in movies {
        let src = if id < 0 { "tv" } else { "movie" };
        append(conn, "movie", id, "add",
               Some(serde_json::json!({"state": state, "watched_at": at, "src": src})), &added)?;
        n += 1;
        if fav != 0 {
            append(conn, "movie", id, "favorite", Some(serde_json::json!({"value": true})), &added)?;
            n += 1;
        }
        if rew > 0 {
            append(conn, "movie", id, "rewatch", Some(serde_json::json!({"count": rew})), &added)?;
            n += 1;
        }
    }
    Ok(n)
}

// ---------- mutation wrappers ----------
// Every local mutation path goes through one of these so it lands in the oplog.

fn show_of_episode(conn: &Connection, ep_id: i64) -> Result<i64> {
    Ok(conn.query_row("SELECT show_tmdb_id FROM episodes WHERE tmdb_id=?1", [ep_id], |r| r.get(0))?)
}

fn is_watched(conn: &Connection, ep_id: i64) -> Result<bool> {
    Ok(conn.query_row("SELECT 1 FROM episode_watches WHERE episode_tmdb_id=?1", [ep_id], |_| Ok(()))
        .optional()?.is_some())
}

fn log_watches(conn: &Connection, ids: &[i64], now: &str) -> Result<()> {
    for id in ids {
        let show = show_of_episode(conn, *id)?;
        append(conn, "ep", *id, "watch", Some(json!({"watched_at": now, "show": show})), now)?;
    }
    Ok(())
}

fn log_unwatches(conn: &Connection, ids: &[i64], now: &str) -> Result<()> {
    for id in ids {
        append(conn, "ep", *id, "unwatch", None, now)?;
    }
    Ok(())
}

pub fn episode_mark(conn: &Connection, ep_id: i64, watched: bool, now: &str) -> Result<()> {
    let was = is_watched(conn, ep_id)?;
    crate::queries::set_episode_watched(conn, ep_id, watched, now)?;
    match (was, watched) {
        (false, true) => log_watches(conn, &[ep_id], now),
        (true, false) => log_unwatches(conn, &[ep_id], now),
        _ => Ok(()), // no state change (INSERT OR IGNORE / DELETE of nothing): no op
    }
}

pub fn episodes_unmark(conn: &Connection, ids: &[i64], now: &str) -> Result<()> {
    crate::queries::unmark_episodes(conn, ids)?;
    log_unwatches(conn, ids, now)
}

pub fn season_mark_collect(conn: &Connection, show_id: i64, season: i64,
                           today: &str, now: &str) -> Result<Vec<i64>> {
    let ids = crate::queries::mark_season_collect(conn, show_id, season, today, now)?;
    log_watches(conn, &ids, now)?;
    Ok(ids)
}

pub fn season_unmark(conn: &Connection, show_id: i64, season: i64,
                     today: &str, now: &str) -> Result<()> {
    let ids: Vec<i64> = conn.prepare(
        "SELECT w.episode_tmdb_id FROM episode_watches w
         JOIN episodes e ON e.tmdb_id=w.episode_tmdb_id
         WHERE e.show_tmdb_id=?1 AND e.season_number=?2")?
        .query_map(rusqlite::params![show_id, season], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    crate::queries::mark_season(conn, show_id, season, false, today, now)?;
    log_unwatches(conn, &ids, now)
}

pub fn seasons_mark_before(conn: &Connection, show_id: i64, season: i64,
                           today: &str, now: &str) -> Result<Vec<i64>> {
    let ids = crate::queries::mark_seasons_before(conn, show_id, season, today, now)?;
    log_watches(conn, &ids, now)?;
    Ok(ids)
}

pub fn episodes_mark_before(conn: &Connection, show_id: i64, season: i64, episode: i64,
                            today: &str, now: &str) -> Result<Vec<i64>> {
    let ids = crate::queries::mark_episodes_before(conn, show_id, season, episode, today, now)?;
    log_watches(conn, &ids, now)?;
    Ok(ids)
}

pub fn mark_all_aired(conn: &Connection, show_id: i64, today: &str, now: &str) -> Result<()> {
    let ids: Vec<i64> = conn.prepare(
        "SELECT tmdb_id FROM episodes e
         WHERE show_tmdb_id=?1 AND air_date IS NOT NULL AND air_date<=?2
           AND NOT EXISTS (SELECT 1 FROM episode_watches w WHERE w.episode_tmdb_id=e.tmdb_id)")?
        .query_map(rusqlite::params![show_id, today], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    crate::queries::mark_all_aired(conn, show_id, today, now)?;
    log_watches(conn, &ids, now)
}

pub fn show_set_state(conn: &Connection, id: i64, state: &str, now: &str) -> Result<()> {
    crate::queries::set_show_state(conn, id, state)?;
    append(conn, "show", id, "state", Some(json!({"state": state})), now)
}

pub fn show_toggle_favorite(conn: &Connection, id: i64, now: &str) -> Result<bool> {
    let v = crate::queries::toggle_show_favorite(conn, id)?;
    append(conn, "show", id, "favorite", Some(json!({"value": v})), now)?;
    Ok(v)
}

/// `src` names the TMDB record backing this entry: "tv" for native shows,
/// "movie" for movies filed under Shows (negative local id) — the receiver
/// needs it to fetch the right endpoint.
pub fn log_show_add(conn: &Connection, id: i64, state: &str, src: &str, now: &str) -> Result<()> {
    append(conn, "show", id, "add", Some(json!({"state": state, "src": src})), now)
}

/// Delete a show entirely (cascade drops its episodes and watches).
pub fn show_remove(conn: &Connection, id: i64, now: &str) -> Result<()> {
    conn.execute("DELETE FROM shows WHERE tmdb_id=?1", [id])?;
    append(conn, "show", id, "remove", None, now)
}

/// Delete a movie entirely.
pub fn movie_remove(conn: &Connection, id: i64, now: &str) -> Result<()> {
    conn.execute("DELETE FROM movies WHERE tmdb_id=?1", [id])?;
    append(conn, "movie", id, "remove", None, now)
}

/// Set the rewatch count to an absolute value (never below zero).
pub fn movie_set_rewatch(conn: &Connection, id: i64, count: i64, now: &str) -> Result<i64> {
    let c = count.max(0);
    conn.execute("UPDATE movies SET rewatch_count=?2 WHERE tmdb_id=?1", params![id, c])?;
    append(conn, "movie", id, "rewatch", Some(json!({"count": c})), now)?;
    Ok(c)
}

pub fn movie_set_state(conn: &Connection, id: i64, state: &str, now: &str) -> Result<()> {
    crate::queries::set_movie_state(conn, id, state, now)?;
    let at = (state == "watched").then_some(now);
    append(conn, "movie", id, "state", Some(json!({"state": state, "watched_at": at})), now)
}

pub fn movie_rewatch(conn: &Connection, id: i64, now: &str) -> Result<i64> {
    let n = crate::queries::movie_rewatch(conn, id)?;
    append(conn, "movie", id, "rewatch", Some(json!({"count": n})), now)?;
    Ok(n)
}

pub fn movie_toggle_favorite(conn: &Connection, id: i64, now: &str) -> Result<bool> {
    let v = crate::queries::toggle_movie_favorite(conn, id)?;
    append(conn, "movie", id, "favorite", Some(json!({"value": v})), now)?;
    Ok(v)
}

pub fn log_movie_add(conn: &Connection, id: i64, state: &str,
                     watched_at: Option<&str>, src: &str, now: &str) -> Result<()> {
    append(conn, "movie", id, "add",
           Some(json!({"state": state, "watched_at": watched_at, "src": src})), now)
}

/// Import watches have NULL watched_at by design; the op ts is the import time.
fn log_import_watches(conn: &Connection, ids: &[i64], now: &str) -> Result<()> {
    for id in ids {
        let show = show_of_episode(conn, *id)?;
        append(conn, "ep", *id, "watch",
               Some(json!({"watched_at": null, "show": show})), now)?;
    }
    Ok(())
}

/// Mark import watches, logging ops ONLY for episodes not already watched:
/// episodes_to_mark is a pure function of the export, so a re-import hands us
/// the same ids again and must not duplicate ops (the DB side is already
/// idempotent via mark_episodes_import's INSERT OR IGNORE).
pub fn import_watches(conn: &Connection, ids: &[i64], now: &str) -> Result<()> {
    let mut new_ids = Vec::new();
    for id in ids {
        if !is_watched(conn, *id)? {
            new_ids.push(*id);
        }
    }
    crate::queries::mark_episodes_import(conn, ids)?;
    log_import_watches(conn, &new_ids, now)
}

// ---------- applying peer ops ----------
// Peer ops are applied idempotently with last-write-wins per (entity, group).
// This side writes tables directly and NEVER appends to the oplog.

/// Ops in the same group compete under last-write-wins.
fn group_ops(entity: &str, op: &str) -> &'static [&'static str] {
    match (entity, op) {
        ("ep", _) => &["watch", "unwatch"],
        (_, "favorite") => &["favorite"],
        (_, "rewatch") => &["rewatch"],
        // remove shares the existence group: a newer remove beats an older
        // add/state, and a newer add re-adds after an older remove
        _ => &["add", "state", "remove"],
    }
}

fn newer_local_op_exists(conn: &Connection, op: &Op) -> Result<bool> {
    let names = group_ops(&op.entity, &op.op);
    let placeholders = names.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT COUNT(*) FROM oplog WHERE entity=?1 AND entity_id=?2 AND ts>?3 AND op IN ({placeholders})");
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![
        Box::new(op.entity.clone()), Box::new(op.entity_id.to_string()), Box::new(op.ts.clone())];
    for n in names { params.push(Box::new(*n)); }
    let n: i64 = conn.query_row(&sql, rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())), |r| r.get(0))?;
    Ok(n > 0)
}

fn pstr(op: &Op, key: &str) -> Option<String> {
    op.payload.as_ref()?.get(key)?.as_str().map(str::to_string)
}

/// True when a local remove tombstone is at least as new as `ts`: the entity
/// was deleted here at or after that moment, so an op from before the removal
/// must not resurrect it (not even via the prefetch path).
fn removed_since(conn: &Connection, entity: &str, entity_id: i64, ts: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM oplog WHERE entity=?1 AND entity_id=?2 AND op='remove' AND ts>=?3",
        params![entity, entity_id.to_string(), ts], |r| r.get(0))?;
    Ok(n > 0)
}

/// Apply one peer op to local tables. Never appends to the oplog.
/// Returns false when skipped (LWW-older, or entity unknown for non-add ops).
pub fn apply_op_local(conn: &Connection, op: &Op) -> Result<bool> {
    if newer_local_op_exists(conn, op)? {
        return Ok(false);
    }
    match (op.entity.as_str(), op.op.as_str()) {
        ("ep", "watch") => {
            let exists: bool = conn.query_row("SELECT 1 FROM episodes WHERE tmdb_id=?1",
                [op.entity_id], |_| Ok(())).optional()?.is_some();
            if !exists { return Ok(false); } // caller pre-fetches; still missing = skip
            conn.execute(
                "INSERT INTO episode_watches(episode_tmdb_id,watched_at,source) VALUES(?1,?2,'app')
                 ON CONFLICT(episode_tmdb_id) DO UPDATE SET watched_at=excluded.watched_at",
                params![op.entity_id, pstr(op, "watched_at")])?;
            Ok(true)
        }
        ("ep", "unwatch") => {
            conn.execute("DELETE FROM episode_watches WHERE episode_tmdb_id=?1", [op.entity_id])?;
            Ok(true)
        }
        ("show", "add") | ("show", "state") => {
            if !crate::queries::show_exists(conn, op.entity_id)? { return Ok(false); }
            if let Some(state) = pstr(op, "state") {
                crate::queries::set_show_state(conn, op.entity_id, &state)?;
            }
            Ok(true)
        }
        ("show", "favorite") => {
            if !crate::queries::show_exists(conn, op.entity_id)? { return Ok(false); }
            let v = op.payload.as_ref().and_then(|p| p["value"].as_bool()).unwrap_or(false);
            conn.execute("UPDATE shows SET is_favorite=?2 WHERE tmdb_id=?1",
                         params![op.entity_id, v as i64])?;
            Ok(true)
        }
        ("movie", "add") | ("movie", "state") => {
            if !crate::queries::movie_exists(conn, op.entity_id)? { return Ok(false); }
            if let Some(state) = pstr(op, "state") {
                let at = pstr(op, "watched_at").unwrap_or_else(|| op.ts.clone());
                crate::queries::set_movie_state(conn, op.entity_id, &state, &at)?;
            }
            Ok(true)
        }
        ("movie", "favorite") => {
            if !crate::queries::movie_exists(conn, op.entity_id)? { return Ok(false); }
            let v = op.payload.as_ref().and_then(|p| p["value"].as_bool()).unwrap_or(false);
            conn.execute("UPDATE movies SET is_favorite=?2 WHERE tmdb_id=?1",
                         params![op.entity_id, v as i64])?;
            Ok(true)
        }
        ("movie", "rewatch") => {
            if !crate::queries::movie_exists(conn, op.entity_id)? { return Ok(false); }
            let n = op.payload.as_ref().and_then(|p| p["count"].as_i64()).unwrap_or(0);
            conn.execute("UPDATE movies SET rewatch_count=?2 WHERE tmdb_id=?1",
                         params![op.entity_id, n])?;
            Ok(true)
        }
        ("show", "remove") => {
            conn.execute("DELETE FROM shows WHERE tmdb_id=?1", [op.entity_id])?;
            Ok(true) // cascade drops episodes and watches; delete-of-nothing is fine
        }
        ("movie", "remove") => {
            conn.execute("DELETE FROM movies WHERE tmdb_id=?1", [op.entity_id])?;
            Ok(true)
        }
        _ => Ok(false), // unknown op kind from a future version: skip, never fail
    }
}

#[derive(Debug, Default, Serialize)]
pub struct ApplyOutcome {
    pub applied: usize,
    pub skipped: usize,
    pub last_seq: Option<i64>,
    pub error: Option<String>,
}

/// Apply a peer batch in seq order with prefix semantics: the first hard error
/// stops the batch; `last_seq` is the cursor the caller may safely advance to.
/// `add` ops (and watch ops for locally-missing episodes) fetch metadata first.
pub async fn apply_ops<T: crate::tmdb::Tmdb + ?Sized>(
    db: &Arc<Mutex<rusqlite::Connection>>,
    tmdb: Option<&T>,
    ops: Vec<Op>,
) -> ApplyOutcome {
    use crate::import::with_db;
    let mut out = ApplyOutcome::default();
    for op in ops {
        // the prefetch below would resurrect a removed entity before the LWW
        // check can reject the op, so tombstones are honoured first
        if op.op != "remove" {
            let tomb: Option<(&str, i64)> = match op.entity.as_str() {
                "ep" => op.payload.as_ref().and_then(|p| p["show"].as_i64()).map(|id| ("show", id)),
                "show" => Some(("show", op.entity_id)),
                "movie" => Some(("movie", op.entity_id)),
                _ => None,
            };
            if let Some((te, tid)) = tomb {
                let ts = op.ts.clone();
                match with_db(db, move |c| removed_since(c, te, tid, &ts)).await {
                    Ok(true) => {
                        out.skipped += 1;
                        out.last_seq = Some(op.seq);
                        continue;
                    }
                    Ok(false) => {}
                    Err(e) => { out.error = Some(e.to_string()); break; }
                }
            }
        }
        // pre-fetch metadata where the op needs an entity we do not have
        let fetch_show: Option<i64> = match (op.entity.as_str(), op.op.as_str()) {
            ("show", "add") => Some(op.entity_id),
            ("ep", "watch") => op.payload.as_ref().and_then(|p| p["show"].as_i64()),
            _ => None,
        };
        if let Some(show_id) = fetch_show {
            let (has_show, has_ep) = {
                let (e, id) = (op.entity.clone(), op.entity_id);
                match with_db(db, move |c| {
                    let hs = crate::queries::show_exists(c, show_id)?;
                    let he = e != "ep" || c.query_row("SELECT 1 FROM episodes WHERE tmdb_id=?1",
                        [id], |_| Ok(())).optional()?.is_some();
                    Ok((hs, he))
                }).await {
                    Ok(v) => v,
                    Err(e) => { out.error = Some(e.to_string()); break; }
                }
            };
            if !has_show || !has_ep {
                let Some(t) = tmdb else {
                    out.error = Some("TMDB key needed to fetch new items".into());
                    break;
                };
                // shows filed from a movie record carry a negative local id
                let src_movie = pstr(&op, "src").as_deref() == Some("movie") || show_id < 0;
                let full = if src_movie {
                    match t.movie(show_id.abs()).await {
                        Ok(m) => crate::tmdb::ShowFull {
                            meta: crate::tmdb::ShowMeta::from_movie(&m),
                            episodes: Vec::new(),
                        },
                        Err(e) => { out.error = Some(format!("fetch show {show_id}: {e}")); break; }
                    }
                } else {
                    match t.show_with_episodes(show_id).await {
                        Ok(f) => f,
                        Err(e) => { out.error = Some(format!("fetch show {show_id}: {e}")); break; }
                    }
                };
                let is_add = op.op == "add";
                let state = pstr(&op, "state").unwrap_or_else(|| "watching".into());
                let ts = op.ts.clone();
                if let Err(e) = with_db(db, move |c| {
                    let existed = crate::queries::show_exists(c, full.meta.tmdb_id)?;
                    crate::queries::upsert_show_meta(c, &full.meta, &ts)?;
                    if !existed && is_add {
                        crate::queries::set_show_user(c, full.meta.tmdb_id, &state, false, None, None)?;
                    }
                    crate::queries::upsert_episodes(c, full.meta.tmdb_id, &full.episodes)
                }).await {
                    out.error = Some(e.to_string());
                    break;
                }
            }
        }
        if op.entity == "movie" && op.op == "add" {
            let id = op.entity_id;
            let known = match with_db(db, move |c| crate::queries::movie_exists(c, id)).await {
                Ok(v) => v,
                Err(e) => { out.error = Some(e.to_string()); break; }
            };
            if !known {
                let Some(t) = tmdb else {
                    out.error = Some("TMDB key needed to fetch new items".into());
                    break;
                };
                // movies filed from a TV record carry a negative local id
                let src_tv = pstr(&op, "src").as_deref() == Some("tv") || id < 0;
                let meta = if src_tv {
                    match t.show_with_episodes(id.abs()).await {
                        Ok(f) => crate::tmdb::MovieMeta::from_show(&f.meta),
                        Err(e) => { out.error = Some(format!("fetch movie {id}: {e}")); break; }
                    }
                } else {
                    match t.movie(id).await {
                        Ok(m) => m,
                        Err(e) => { out.error = Some(format!("fetch movie {id}: {e}")); break; }
                    }
                };
                let state = pstr(&op, "state").unwrap_or_else(|| "watchlist".into());
                let at = pstr(&op, "watched_at");
                let ts = op.ts.clone();
                if let Err(e) = with_db(db, move |c| {
                    let existed = crate::queries::movie_exists(c, meta.tmdb_id)?;
                    crate::queries::upsert_movie_meta(c, &meta, &ts)?;
                    if !existed {
                        crate::queries::set_movie_user(c, meta.tmdb_id, &state,
                            at.as_deref(), 0, false, None, None)?;
                    }
                    Ok(())
                }).await {
                    out.error = Some(e.to_string());
                    break;
                }
            }
        }
        let op2 = op.clone();
        match with_db(db, move |c| apply_op_local(c, &op2)).await {
            Ok(true) => out.applied += 1,
            Ok(false) => out.skipped += 1,
            Err(e) => { out.error = Some(e.to_string()); break; }
        }
        out.last_seq = Some(op.seq);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(conn: &rusqlite::Connection) {
        conn.execute_batch(
            "INSERT INTO shows(tmdb_id,name,my_state,added_at) VALUES(1,'A','watching','2026-06-01T00:00:00Z');
             INSERT INTO episodes(tmdb_id,show_tmdb_id,season_number,episode_number,air_date) VALUES
               (10,1,1,1,'2026-06-01'),(11,1,1,2,'2026-06-08'),(12,1,2,1,'2026-06-15');
             INSERT INTO movies(tmdb_id,title,my_state,added_at) VALUES(50,'M','watchlist','2026-06-01T00:00:00Z');").unwrap();
    }

    const NOW: &str = "2026-07-10T00:00:00Z";
    const TODAY: &str = "2026-07-10";

    #[test]
    fn every_mutation_wrapper_appends_an_op() {
        let conn = crate::db::open_memory().unwrap();
        fixture(&conn);
        episode_mark(&conn, 10, true, NOW).unwrap();          // watch (payload carries show)
        episode_mark(&conn, 10, false, NOW).unwrap();         // unwatch
        let ids = season_mark_collect(&conn, 1, 1, TODAY, NOW).unwrap(); // 2 watch ops
        assert_eq!(ids.len(), 2);
        season_unmark(&conn, 1, 1, TODAY, NOW).unwrap();      // 2 unwatch ops
        show_set_state(&conn, 1, "stopped", NOW).unwrap();
        assert!(show_toggle_favorite(&conn, 1, NOW).unwrap());
        movie_set_state(&conn, 50, "watched", NOW).unwrap();
        assert_eq!(movie_rewatch(&conn, 50, NOW).unwrap(), 1);
        assert!(movie_toggle_favorite(&conn, 50, NOW).unwrap());
        let ops = ops_since(&conn, 0).unwrap();
        assert_eq!(ops.len(), 11);
        let watch = &ops[0];
        assert_eq!((watch.entity.as_str(), watch.op.as_str()), ("ep", "watch"));
        assert_eq!(watch.payload.as_ref().unwrap()["show"], 1);
        let rewatch = ops.iter().find(|o| o.op == "rewatch").unwrap();
        assert_eq!(rewatch.payload.as_ref().unwrap()["count"], 1); // absolute count
    }

    #[test]
    fn episode_mark_logs_only_on_state_change() {
        let conn = crate::db::open_memory().unwrap();
        fixture(&conn);
        episode_mark(&conn, 10, true, NOW).unwrap();
        episode_mark(&conn, 10, true, NOW).unwrap();  // already watched: DB no-op, no op
        episode_mark(&conn, 11, false, NOW).unwrap(); // not watched: DB no-op, no op
        let ops = ops_since(&conn, 0).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!((ops[0].entity.as_str(), ops[0].op.as_str()), ("ep", "watch"));
    }

    #[test]
    fn import_watches_only_logs_newly_marked() {
        let conn = crate::db::open_memory().unwrap();
        fixture(&conn);
        episode_mark(&conn, 10, true, NOW).unwrap(); // pre-existing app watch: 1 op
        import_watches(&conn, &[10, 11], NOW).unwrap(); // only 11 is newly marked
        let ops = ops_since(&conn, 0).unwrap();
        assert_eq!(ops.len(), 2);
        let imp = &ops[1];
        assert_eq!((imp.entity.as_str(), imp.entity_id, imp.op.as_str()), ("ep", 11, "watch"));
        assert!(imp.payload.as_ref().unwrap()["watched_at"].is_null());
        import_watches(&conn, &[10, 11], NOW).unwrap(); // re-import: nothing new, no ops
        assert_eq!(latest_seq(&conn).unwrap(), 2);
    }

    #[test]
    fn append_and_read_roundtrip() {
        let conn = crate::db::open_memory().unwrap();
        append(&conn, "ep", 10, "watch",
               Some(serde_json::json!({"watched_at":"2026-07-10T00:00:00Z","show":1})),
               "2026-07-10T00:00:00Z").unwrap();
        append(&conn, "show", 1, "state",
               Some(serde_json::json!({"state":"stopped"})), "2026-07-10T00:01:00Z").unwrap();
        let ops = ops_since(&conn, 0).unwrap();
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].seq, 1);
        assert_eq!(ops[0].entity, "ep");
        assert_eq!(ops[0].entity_id, 10);
        assert_eq!(ops[0].payload.as_ref().unwrap()["show"], 1);
        assert_eq!(ops_since(&conn, 1).unwrap().len(), 1);
        assert_eq!(latest_seq(&conn).unwrap(), 2);
    }

    #[test]
    fn seeding_converts_library_to_ops() {
        let conn = crate::db::open_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO shows(tmdb_id,name,my_state,is_favorite,added_at)
               VALUES(1,'A','watching',1,'2026-06-01T00:00:00Z');
             INSERT INTO episodes(tmdb_id,show_tmdb_id,season_number,episode_number)
               VALUES(10,1,1,1);
             INSERT INTO episode_watches(episode_tmdb_id,watched_at,source)
               VALUES(10,'2026-06-02T00:00:00Z','app');
             INSERT INTO movies(tmdb_id,title,my_state,watched_at,rewatch_count,is_favorite,added_at)
               VALUES(50,'M','watched','2026-06-03T00:00:00Z',2,0,'2026-06-01T00:00:00Z');").unwrap();
        let n = seed_from_library(&conn).unwrap();
        assert_eq!(n, 5); // show add + show favorite + watch + movie add + movie rewatch
        let ops = ops_since(&conn, 0).unwrap();
        let w = ops.iter().find(|o| o.entity == "ep").unwrap();
        assert_eq!(w.payload.as_ref().unwrap()["show"], 1);
    }

    // ---------- applying peer ops ----------

    #[test]
    fn apply_is_idempotent_and_lww() {
        let conn = crate::db::open_memory().unwrap();
        fixture(&conn);
        let watch = Op { seq: 1, ts: "2026-07-01T00:00:00Z".into(), entity: "ep".into(),
            entity_id: 10, op: "watch".into(),
            payload: Some(json!({"watched_at": "2026-07-01T00:00:00Z", "show": 1})) };
        assert!(apply_op_local(&conn, &watch).unwrap());
        assert!(apply_op_local(&conn, &watch).unwrap()); // re-apply: no error, same state
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM episode_watches", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        // a strictly newer LOCAL op in the same group wins over an older peer op
        episode_mark(&conn, 10, false, "2026-07-02T00:00:00Z").unwrap(); // local unwatch, newer
        assert!(!apply_op_local(&conn, &watch).unwrap()); // peer watch (older) skipped
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM episode_watches", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        // ...but a NEWER peer op applies over an older local one
        let newer = Op { ts: "2026-07-03T00:00:00Z".into(), ..watch.clone() };
        assert!(apply_op_local(&conn, &newer).unwrap());
    }

    #[test]
    fn remove_deletes_and_competes_in_the_existence_group() {
        let conn = crate::db::open_memory().unwrap();
        fixture(&conn);
        show_remove(&conn, 1, "2026-07-05T00:00:00Z").unwrap();
        assert!(!crate::queries::show_exists(&conn, 1).unwrap());
        // cascade dropped the episodes too
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM episodes", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        // an OLDER peer add loses to the newer local remove
        let peer_add = Op { seq: 1, ts: "2026-07-01T00:00:00Z".into(), entity: "show".into(),
            entity_id: 1, op: "add".into(), payload: Some(json!({"state": "watching", "src": "tv"})) };
        assert!(!apply_op_local(&conn, &peer_add).unwrap());
        // a NEWER peer remove op applies idempotently on a missing row
        let peer_rm = Op { seq: 2, ts: "2026-07-06T00:00:00Z".into(), entity: "show".into(),
            entity_id: 1, op: "remove".into(), payload: None };
        assert!(apply_op_local(&conn, &peer_rm).unwrap());
        // movies mirror
        movie_remove(&conn, 50, "2026-07-05T00:00:00Z").unwrap();
        assert!(!crate::queries::movie_exists(&conn, 50).unwrap());
    }

    #[test]
    fn set_rewatch_is_absolute_and_never_negative() {
        let conn = crate::db::open_memory().unwrap();
        fixture(&conn);
        assert_eq!(movie_set_rewatch(&conn, 50, 3, NOW).unwrap(), 3);
        assert_eq!(movie_set_rewatch(&conn, 50, -2, NOW).unwrap(), 0);
        let ops = ops_since(&conn, 0).unwrap();
        let counts: Vec<i64> = ops.iter().filter(|o| o.op == "rewatch")
            .map(|o| o.payload.as_ref().unwrap()["count"].as_i64().unwrap()).collect();
        assert_eq!(counts, vec![3, 0]);
    }

    #[test]
    fn lww_favorite_and_state_do_not_fight() {
        let conn = crate::db::open_memory().unwrap();
        fixture(&conn);
        show_set_state(&conn, 1, "stopped", "2026-07-05T00:00:00Z").unwrap(); // local, newer
        let peer_fav = Op { seq: 1, ts: "2026-07-01T00:00:00Z".into(), entity: "show".into(),
            entity_id: 1, op: "favorite".into(), payload: Some(json!({"value": true})) };
        assert!(apply_op_local(&conn, &peer_fav).unwrap()); // different group: applies
        let peer_state = Op { op: "state".into(), payload: Some(json!({"state": "watching"})), ..peer_fav };
        assert!(!apply_op_local(&conn, &peer_state).unwrap()); // same group, older: skipped
    }

    fn conn_exec(c: &Connection, sql: &str) {
        c.execute_batch(sql).unwrap();
    }

    #[derive(Default)]
    struct MockTmdb {
        movie_calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl crate::tmdb::Tmdb for MockTmdb {
        async fn find_show(&self, _: i64) -> crate::error::Result<Option<i64>> { Ok(None) }
        async fn find_movie(&self, _: &str) -> crate::error::Result<Option<i64>> { Ok(None) }
        async fn show_with_episodes(&self, id: i64) -> crate::error::Result<crate::tmdb::ShowFull> {
            Ok(crate::tmdb::ShowFull {
                meta: crate::tmdb::ShowMeta {
                    tmdb_id: id, name: format!("Show {id}"), year: Some(2020),
                    poster_path: None, backdrop_path: None, overview: None, status: None,
                    season_numbers: vec![1], is_anime: false,
                },
                episodes: vec![crate::tmdb::EpisodeMeta {
                    tmdb_id: if id == 1 { 10 } else { id * 100 + 1 }, season_number: 1, episode_number: 1,
                    name: None, air_date: None, runtime: None, still_path: None, overview: None,
                }],
            })
        }
        async fn movie(&self, id: i64) -> crate::error::Result<crate::tmdb::MovieMeta> {
            self.movie_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(crate::tmdb::MovieMeta {
                tmdb_id: id, imdb_id: None, title: format!("Movie {id}"), year: Some(2020),
                release_date: None, runtime: None, poster_path: None, backdrop_path: None, overview: None,
            })
        }
        async fn search(&self, _: &str) -> crate::error::Result<Vec<crate::tmdb::SearchResultItem>> { Ok(vec![]) }
        async fn validate(&self) -> crate::error::Result<bool> { Ok(true) }
    }

    #[tokio::test]
    async fn two_dbs_converge_through_op_exchange() {
        use std::sync::{Arc, Mutex};
        let a = Arc::new(Mutex::new(crate::db::open_memory().unwrap()));
        let b = Arc::new(Mutex::new(crate::db::open_memory().unwrap()));
        // A adds a show (unknown to B -> B must fetch it via Tmdb) and marks its episode
        {
            let c = a.lock().unwrap();
            fixture(&c);
            log_show_add(&c, 1, "watching", "tv", "2026-07-01T00:00:00Z").unwrap();
            episode_mark(&c, 10, true, "2026-07-01T01:00:00Z").unwrap();
        }
        // B favorites a movie it adds
        {
            let c = b.lock().unwrap();
            conn_exec(&c, "INSERT INTO movies(tmdb_id,title,my_state,added_at)
                           VALUES(50,'M','watchlist','2026-06-01T00:00:00Z')");
            log_movie_add(&c, 50, "watchlist", None, "movie", "2026-07-01T00:00:00Z").unwrap();
            movie_toggle_favorite(&c, 50, "2026-07-01T01:00:00Z").unwrap();
        }
        let ops_a = { let c = a.lock().unwrap(); ops_since(&c, 0).unwrap() };
        let ops_b = { let c = b.lock().unwrap(); ops_since(&c, 0).unwrap() };
        let tmdb = MockTmdb::default();
        let out_b = apply_ops(&b, Some(&tmdb), ops_a.clone()).await;
        let out_a = apply_ops(&a, Some(&tmdb), ops_b.clone()).await;
        assert!(out_a.error.is_none() && out_b.error.is_none());
        // converged: B has the show + watch, A has the movie + favorite
        let b_watches = |c: &Connection| -> i64 {
            c.query_row("SELECT COUNT(*) FROM episode_watches WHERE episode_tmdb_id=10",
                        [], |r| r.get(0)).unwrap()
        };
        let a_movie = |c: &Connection| -> (String, i64) {
            c.query_row("SELECT my_state, is_favorite FROM movies WHERE tmdb_id=50",
                        [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap()
        };
        assert_eq!(b_watches(&b.lock().unwrap()), 1);
        assert_eq!(a_movie(&a.lock().unwrap()), ("watchlist".into(), 1));
        // re-applying the same batches changes nothing (idempotence)
        let out_b2 = apply_ops(&b, Some(&tmdb), ops_a).await;
        let out_a2 = apply_ops(&a, Some(&tmdb), ops_b).await;
        assert!(out_b2.error.is_none() && out_a2.error.is_none());
        assert_eq!(b_watches(&b.lock().unwrap()), 1); // still exactly one watch row
        assert_eq!(a_movie(&a.lock().unwrap()), ("watchlist".into(), 1)); // state + favorite unchanged
    }

    #[tokio::test]
    async fn tombstone_blocks_resurrection_by_older_peer_ops() {
        use std::sync::{Arc, Mutex};
        let db = Arc::new(Mutex::new(crate::db::open_memory().unwrap()));
        {
            let c = db.lock().unwrap();
            fixture(&c);
            show_remove(&c, 1, "2026-07-05T00:00:00Z").unwrap();
            movie_remove(&c, 50, "2026-07-05T00:00:00Z").unwrap();
        }
        let tmdb = MockTmdb::default();
        // a peer that never saw the removal sends its older ops: an ep watch
        // (whose prefetch used to re-add the show), a favorite, and the add
        let stale = vec![
            Op { seq: 1, ts: "2026-07-01T00:00:00Z".into(), entity: "show".into(), entity_id: 1,
                 op: "add".into(), payload: Some(json!({"state": "watching", "src": "tv"})) },
            Op { seq: 2, ts: "2026-07-01T01:00:00Z".into(), entity: "ep".into(), entity_id: 10,
                 op: "watch".into(), payload: Some(json!({"watched_at": "2026-07-01T01:00:00Z", "show": 1})) },
            Op { seq: 3, ts: "2026-07-01T02:00:00Z".into(), entity: "movie".into(), entity_id: 50,
                 op: "favorite".into(), payload: Some(json!({"value": true})) },
        ];
        let out = apply_ops(&db, Some(&tmdb), stale).await;
        assert!(out.error.is_none());
        assert_eq!((out.applied, out.skipped), (0, 3));
        assert_eq!(out.last_seq, Some(3)); // cursor still advances past them
        {
            let c = db.lock().unwrap();
            assert!(!crate::queries::show_exists(&c, 1).unwrap());
            assert!(!crate::queries::movie_exists(&c, 50).unwrap());
        }
        // an add NEWER than the removal is a deliberate re-add and applies
        let readd = Op { seq: 4, ts: "2026-07-06T00:00:00Z".into(), entity: "show".into(),
            entity_id: 1, op: "add".into(), payload: Some(json!({"state": "watching", "src": "tv"})) };
        let out2 = apply_ops(&db, Some(&tmdb), vec![readd]).await;
        assert!(out2.error.is_none());
        assert!(crate::queries::show_exists(&db.lock().unwrap(), 1).unwrap());
    }

    #[tokio::test]
    async fn movie_add_fetches_unknown_movie_metadata() {
        use std::sync::{Arc, Mutex};
        let db = Arc::new(Mutex::new(crate::db::open_memory().unwrap()));
        let add = Op { seq: 1, ts: "2026-07-01T00:00:00Z".into(), entity: "movie".into(),
            entity_id: 60, op: "add".into(),
            payload: Some(json!({"state": "watched", "watched_at": "2026-06-30T00:00:00Z"})) };
        let tmdb = MockTmdb::default();
        let out = apply_ops(&db, Some(&tmdb), vec![add]).await;
        assert!(out.error.is_none());
        assert_eq!((out.applied, out.skipped), (1, 0));
        assert_eq!(out.last_seq, Some(1));
        assert_eq!(tmdb.movie_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let c = db.lock().unwrap();
        let (title, state, at): (String, String, Option<String>) = c.query_row(
            "SELECT title, my_state, watched_at FROM movies WHERE tmdb_id=60", [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!(title, "Movie 60"); // metadata came from the Tmdb fetch
        assert_eq!(state, "watched");  // user state came from the op payload
        assert_eq!(at.as_deref(), Some("2026-06-30T00:00:00Z"));
        // applying peer ops never appends to the local oplog
        assert_eq!(latest_seq(&c).unwrap(), 0);
    }

    #[tokio::test]
    async fn prefix_stops_at_first_fetch_failure_without_advancing_cursor() {
        use std::sync::{Arc, Mutex};
        let db = Arc::new(Mutex::new(crate::db::open_memory().unwrap()));
        { let c = db.lock().unwrap(); fixture(&c); }
        let ops = vec![
            Op { seq: 7, ts: "2026-07-01T00:00:00Z".into(), entity: "show".into(),
                 entity_id: 1, op: "favorite".into(), payload: Some(json!({"value": true})) },
            // movie 60 is unknown -> needs a fetch, but tmdb is None -> hard error
            Op { seq: 8, ts: "2026-07-01T00:01:00Z".into(), entity: "movie".into(),
                 entity_id: 60, op: "add".into(), payload: Some(json!({"state": "watchlist"})) },
            Op { seq: 9, ts: "2026-07-01T00:02:00Z".into(), entity: "show".into(),
                 entity_id: 1, op: "state".into(), payload: Some(json!({"state": "stopped"})) },
        ];
        let out = apply_ops(&db, None::<&MockTmdb>, ops).await;
        assert!(out.error.is_some());
        assert_eq!((out.applied, out.skipped), (1, 0));
        assert_eq!(out.last_seq, Some(7)); // cursor frozen at the last success, not past the failure
        let c = db.lock().unwrap();
        let (state, fav): (String, i64) = c.query_row(
            "SELECT my_state, is_favorite FROM shows WHERE tmdb_id=1", [],
            |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(fav, 1);              // op 7 landed
        assert_eq!(state, "watching");   // op 9 never ran: batch stopped at op 8
        assert!(!crate::queries::movie_exists(&c, 60).unwrap());
    }
}
