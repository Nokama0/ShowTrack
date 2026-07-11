# ShowTrack Phase 2: Two-Way LAN Sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Two-way sync between the desktop app and the Android app over the home LAN, with pairing-code security, oplog-based change tracking, and a full-library snapshot bootstrap.

**Architecture:** Every local mutation appends to an `oplog` table (migration v2). The desktop runs an axum HTTP server (port 47810, only while the app is open and the Settings toggle is on) exposing pair/snapshot/changes endpoints behind a bearer token minted by a 6-digit pairing code. The phone drives all traffic from the **Rust side** (reqwest commands — the Android release WebView blocks cleartext `fetch()`): pull peer ops → apply (idempotent, last-write-wins) → push own ops → advance cursors.

**Tech Stack:** axum 0.8 (server), reqwest/rustls (client), rusqlite, `rand` (token + device id), `subtle` (constant-time compares), tower + http-body-util (router tests only).

## Global Constraints

- Spec: `docs/superpowers/specs/2026-07-09-android-port-design.md` (Phase 2 section). Deviations must be recorded in the spec's deviations section.
- The phone sync client lives in Rust commands, NEVER frontend `fetch()` (Android release manifest `usesCleartextTraffic=false` blocks WebView cleartext; Rust sockets are exempt).
- Never `INSERT OR REPLACE` on shows/episodes/movies (CASCADE wipes `episode_watches`). Merge with `ON CONFLICT DO UPDATE` / `INSERT OR IGNORE`.
- No em dashes anywhere in UI copy. Animations always on (never gate on prefers-reduced-motion). Gold/dark theme tokens from `app.css` (`--gold`, `--raise`, `--line`, `.panel`, `.btn-gold`, `.btn-ghost`).
- Defaults, verbatim: port **47810**; token **32 random bytes, hex**; pairing code **6 digits**, expires after **one use or 5 minutes**; rolling DB backups **keep 5**, taken before applying a pushed batch; auth via `Authorization: Bearer <token>` with constant-time compare; 401 otherwise. No TLS, no mDNS; manual IP entry.
- All `cargo test` must stay green with **zero warnings**; `pnpm check` 0 errors 0 warnings; `pnpm build` clean.
- Android release builds: FOREGROUND shell with timeout, `pnpm tauri android build --target aarch64` (universal wedges). Env: `JAVA_HOME="C:\Program Files\Android\Android Studio\jbr"`, `ANDROID_HOME=$env:LOCALAPPDATA\Android\Sdk`, `NDK_HOME=$env:ANDROID_HOME\ndk\27.2.12479018`.
- Samsung wireless adb: port changes between sessions (`adb mdns services` to rediscover); service pauses while the phone is locked.

## File Structure

- `src-tauri/src/db.rs` — migration v2 (oplog, sync_state, device id, Android-only seed).
- `src-tauri/src/oplog.rs` — NEW: op model, append + read helpers, mutation wrappers (mutate + log), apply logic with LWW, sync_state accessors, library seeding.
- `src-tauri/src/sync_proto.rs` — NEW: wire types (PairRequest/Response, ChangesResponse, PushRequest/Response, Snapshot + row structs), snapshot build/apply.
- `src-tauri/src/sync_server.rs` — NEW: axum router, auth middleware, pair/snapshot/changes handlers, DB backups, server start/stop.
- `src-tauri/src/sync_client.rs` — NEW: phone-side pair + bootstrap + sync_now over reqwest.
- `src-tauri/src/commands.rs` — mutation commands rewired through oplog wrappers; new sync commands.
- `src-tauri/src/lib.rs` — AppState additions, command registration, server autostart.
- `src-tauri/src/import.rs` — import appends ops.
- `src/lib/api.ts`, `src/lib/types.ts` — sync API + types.
- `src/routes/settings/+page.svelte` — "Phone sync" panel (desktop) + "Sync with desktop" panel (Android).
- `src/routes/+layout.svelte` — auto-sync on launch/foreground (Android).

### Design decisions locked here (record in spec deviations if changed)

- **Local oplog stores only local-origin ops.** Applying peer ops writes tables directly and never appends. Therefore no `origin` column is needed (the spec's "devices ignore their own ops on pull" is satisfied structurally) and pull can return the whole oplog.
- **Conflict groups for LWW** per (entity, entity_id): `ep:watch/unwatch` → one group; `show/movie add+state` → one group; `favorite` its own; `rewatch` its own. A peer op loses only to a strictly newer local op in the same group.
- **"Per-batch atomic" is relaxed to prefix-apply**: ops apply in seq order; the first hard failure stops the batch and the cursor advances to the last success. Idempotent apply makes retries safe. Rationale: applying an `add` op can require a TMDB fetch, which cannot live inside a DB transaction.
- **Watch ops carry the show id** (`payload.show`) so the receiver can fetch missing episode metadata (e.g. peer refreshed a new season first) instead of wedging forever.
- **Snapshot merge semantics mirror re-import**: metadata upserts always; user state (`my_state`, favorite, watches) only for entities the phone does not already have; watches are `INSERT OR IGNORE`. Phone-local data is never clobbered.
- **Android-only oplog seeding at migration v2**: the phone converts its existing library to ops so pre-pairing phone data reaches the desktop on first push. Desktop does not seed (the snapshot covers desktop→phone; seeding 16k+ import watches would bloat the oplog for nothing).
- Sync modules compile on both platforms (no cfg maze); the UI gates which commands are reachable per platform.

---

### Task 1: Migration v2 + oplog core

**Files:**
- Modify: `src-tauri/src/db.rs`
- Modify: `src-tauri/src/lib.rs` (module decl)
- Modify: `src-tauri/Cargo.toml` (add `rand`)
- Create: `src-tauri/src/oplog.rs`

**Interfaces:**
- Produces: `oplog::Op { seq: i64, ts: String, entity: String, entity_id: i64, op: String, payload: Option<serde_json::Value> }` (Serialize/Deserialize/Clone/PartialEq)
- Produces: `oplog::append(conn, entity: &str, entity_id: i64, op: &str, payload: Option<serde_json::Value>, ts: &str) -> Result<()>`
- Produces: `oplog::ops_since(conn, since: i64) -> Result<Vec<Op>>`, `oplog::latest_seq(conn) -> Result<i64>`
- Produces: `oplog::sync_get(conn, key: &str) -> Result<Option<String>>`, `oplog::sync_set(conn, key, value) -> Result<()>`, `oplog::sync_del(conn, key) -> Result<()>`
- Produces: `oplog::hex(bytes: &[u8]) -> String`, `oplog::seed_from_library(conn) -> Result<usize>`
- sync_state keys used project-wide: `device_id`, `token`, `paired_device`, `peer_addr`, `peer_cursor`, `push_cursor`, `last_sync`, `last_push_at`, `sync_enabled`, `sync_port`

- [ ] **Step 1: Add the `rand` dependency**

In `src-tauri/Cargo.toml` under `[dependencies]`:

```toml
rand = "0.9"
```

- [ ] **Step 2: Write failing migration + oplog tests**

Append to `src-tauri/src/db.rs` tests:

```rust
#[test]
fn migrates_to_v2_with_oplog_and_device_id() {
    let conn = open_memory().unwrap();
    let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(v, 2);
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('oplog','sync_state')",
        [], |r| r.get(0)).unwrap();
    assert_eq!(n, 2);
    let id = crate::oplog::sync_get(&conn, "device_id").unwrap().unwrap();
    assert_eq!(id.len(), 32); // 16 random bytes, hex
}
```

New test module in `src-tauri/src/oplog.rs` (written together with Step 3's skeleton):

```rust
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
```

- [ ] **Step 3: Implement migration + oplog core**

`src-tauri/src/db.rs` — add after `SCHEMA_V1`:

```rust
const SCHEMA_V2: &str = "
CREATE TABLE oplog (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  ts TEXT NOT NULL,
  entity TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  op TEXT NOT NULL,
  payload TEXT
);
CREATE INDEX idx_oplog_entity ON oplog(entity, entity_id);
CREATE TABLE sync_state (key TEXT PRIMARY KEY, value TEXT NOT NULL);
";
```

Extend `init` (after the `v < 1` block):

```rust
    if v < 2 {
        conn.execute_batch(SCHEMA_V2)?;
        let mut id = [0u8; 16];
        rand::RngCore::fill_bytes(&mut rand::rng(), &mut id);
        conn.execute("INSERT INTO sync_state(key,value) VALUES('device_id',?1)",
                     [crate::oplog::hex(&id)])?;
        // The phone converts its pre-sync library into ops so it can push them
        // after pairing. Desktop does not seed: the snapshot is desktop -> phone.
        if cfg!(target_os = "android") {
            crate::oplog::seed_from_library(conn)?;
        }
        conn.pragma_update(None, "user_version", 2)?;
    }
```

Create `src-tauri/src/oplog.rs`:

```rust
//! Change tracking for LAN sync: every local mutation appends an op here.
//! The oplog holds ONLY local-origin ops; applying peer ops writes tables
//! directly (see apply_* below) and never appends.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

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
        append(conn, "show", id, "add", Some(serde_json::json!({"state": state})), &added)?;
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
        append(conn, "movie", id, "add",
               Some(serde_json::json!({"state": state, "watched_at": at})), &added)?;
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
```

Add `pub mod oplog;` to `src-tauri/src/lib.rs` (alphabetical, after `models`).

Add a seed test to `oplog.rs` tests (build a tiny library by hand, call `seed_from_library`, assert op count and that a watch op carries `payload.show`):

```rust
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
```

- [ ] **Step 4: Run and verify**

Run: `cd src-tauri && cargo test`
Expected: all pass (39 existing + 3 new), zero warnings.

- [ ] **Step 5: Commit**

```bash
git add src-tauri
git commit -m "feat(sync): migration v2 with oplog, sync_state, device id, Android seed"
```

---

### Task 2: Log every mutation path

**Files:**
- Modify: `src-tauri/src/oplog.rs` (mutation wrappers)
- Modify: `src-tauri/src/commands.rs` (rewire mutation commands)
- Modify: `src-tauri/src/import.rs` (import appends ops)

**Interfaces:**
- Consumes: `queries::*` mutation functions as they exist today; `oplog::append`.
- Produces (all `&Connection`-level, unit-testable):
  - `oplog::episode_mark(conn, ep_id: i64, watched: bool, now: &str) -> Result<()>`
  - `oplog::episodes_unmark(conn, ids: &[i64], now: &str) -> Result<()>`
  - `oplog::season_mark_collect(conn, show_id, season, today, now) -> Result<Vec<i64>>`
  - `oplog::season_unmark(conn, show_id, season, today, now) -> Result<()>`
  - `oplog::seasons_mark_before(conn, show_id, season, today, now) -> Result<Vec<i64>>`
  - `oplog::episodes_mark_before(conn, show_id, season, episode, today, now) -> Result<Vec<i64>>`
  - `oplog::mark_all_aired(conn, show_id, today, now) -> Result<()>`
  - `oplog::show_set_state(conn, id, state: &str, now) -> Result<()>`
  - `oplog::show_toggle_favorite(conn, id, now) -> Result<bool>`
  - `oplog::log_show_add(conn, id, state: &str, now) -> Result<()>`
  - `oplog::movie_set_state(conn, id, state: &str, now) -> Result<()>`
  - `oplog::movie_rewatch(conn, id, now) -> Result<i64>`
  - `oplog::movie_toggle_favorite(conn, id, now) -> Result<bool>`
  - `oplog::log_movie_add(conn, id, state: &str, watched_at: Option<&str>, now) -> Result<()>`
  - `oplog::log_import_watches(conn, ids: &[i64], now) -> Result<()>`
- Op vocabulary (fixed from here on):
  - `("ep", id, "watch", {"watched_at": <ts|null>, "show": <show_id>})`
  - `("ep", id, "unwatch", null)`
  - `("show", id, "add", {"state": s})` / `("show", id, "state", {"state": s})` / `("show", id, "favorite", {"value": b})`
  - `("movie", id, "add", {"state": s, "watched_at": <ts|null>})` / `"state"` (same payload) / `"favorite"` ({"value": b}) / `"rewatch"` ({"count": n} — absolute)

- [ ] **Step 1: Write failing wrapper tests**

In `oplog.rs` tests (reuse the `seed`-style fixture from `queries.rs` tests by inserting rows directly):

```rust
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
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test oplog`
Expected: FAIL (functions not defined).

- [ ] **Step 3: Implement the wrappers**

Add to `oplog.rs` (uses `crate::queries`):

```rust
use serde_json::json;

fn show_of_episode(conn: &Connection, ep_id: i64) -> Result<i64> {
    Ok(conn.query_row("SELECT show_tmdb_id FROM episodes WHERE tmdb_id=?1", [ep_id], |r| r.get(0))?)
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
    crate::queries::set_episode_watched(conn, ep_id, watched, now)?;
    if watched { log_watches(conn, &[ep_id], now) } else { log_unwatches(conn, &[ep_id], now) }
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

pub fn log_show_add(conn: &Connection, id: i64, state: &str, now: &str) -> Result<()> {
    append(conn, "show", id, "add", Some(json!({"state": state})), now)
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
                     watched_at: Option<&str>, now: &str) -> Result<()> {
    append(conn, "movie", id, "add", Some(json!({"state": state, "watched_at": watched_at})), now)
}

/// Import watches have NULL watched_at by design; the op ts is the import time.
pub fn log_import_watches(conn: &Connection, ids: &[i64], now: &str) -> Result<()> {
    for id in ids {
        let show = show_of_episode(conn, *id)?;
        append(conn, "ep", *id, "watch",
               Some(json!({"watched_at": null, "show": show})), now)?;
    }
    Ok(())
}
```

- [ ] **Step 4: Rewire commands and import**

In `src-tauri/src/commands.rs`, swap each mutation to its wrapper (same signatures, `queries::` → `oplog::`, passing `&now_rfc3339()` where new):

- `episode_mark`: `oplog::episode_mark(c, id, watched, &now_rfc3339())`
- `season_mark`: watched branch → `oplog::season_mark_collect(...)`; unmark branch → `oplog::season_unmark(c, show_id, season, &today_local(), &now_rfc3339())?; Ok(Vec::new())`
- `episodes_unmark` → `oplog::episodes_unmark(c, &ids, &now_rfc3339())`
- `seasons_mark_before` / `episodes_mark_before` → oplog variants
- `show_mark_all` → `oplog::mark_all_aired(...)`
- `show_mark_next`: replace `queries::set_episode_watched(c, next.episode_id, true, ...)` with `oplog::episode_mark(c, next.episode_id, true, &now_rfc3339())`
- `show_set_state` → `oplog::show_set_state`
- `show_toggle_favorite` → `oplog::show_toggle_favorite(c, id, &now_rfc3339())`
- `movie_set_state` → `oplog::movie_set_state`
- `movie_rewatch` → `oplog::movie_rewatch(c, id, &now_rfc3339())`
- `movie_toggle_favorite` → `oplog::movie_toggle_favorite(c, id, &now_rfc3339())`
- `show_add`: inside the existing `with_db` closure, after `upsert_episodes`, add `oplog::log_show_add(c, full.meta.tmdb_id, &my_state, &now_rfc3339())?;`
- `movie_add`: after `set_movie_user`, add `oplog::log_movie_add(c, meta.tmdb_id, &my_state, watched_at.as_deref(), &now)?;`

In `src-tauri/src/import.rs` `import_one_show`'s closure: after `queries::mark_episodes_import(conn, &mark)?;` add

```rust
        if !existed {
            crate::oplog::log_show_add(conn, full.meta.tmdb_id, &state, &now)?;
        }
        crate::oplog::log_import_watches(conn, &mark, &now)?;
```

(move the `if !existed` add-op next to the existing `set_show_user` guard). In `import_one_movie`'s closure, inside its `if !existed` block:

```rust
            crate::oplog::log_movie_add(conn, meta.tmdb_id, state, watched_at.as_deref(), &now)?;
```

- [ ] **Step 5: Run the whole suite**

Run: `cd src-tauri && cargo test`
Expected: all pass, zero warnings. (Existing import tests exercise the new import logging.)

- [ ] **Step 6: Commit**

```bash
git add src-tauri
git commit -m "feat(sync): every mutation path appends an oplog entry"
```

---

### Task 3: Applying peer ops (idempotent, LWW, prefix)

**Files:**
- Modify: `src-tauri/src/oplog.rs`

**Interfaces:**
- Consumes: `Op`, `crate::tmdb::Tmdb` trait (`show_with_episodes`, `movie`), `crate::import::with_db`.
- Produces:
  - `oplog::apply_op_local(conn, op: &Op) -> Result<bool>` (true = applied, false = skipped by LWW/unknown-entity)
  - `oplog::ApplyOutcome { pub applied: usize, pub skipped: usize, pub last_seq: Option<i64>, pub error: Option<String> }`
  - `oplog::apply_ops<T: Tmdb + ?Sized>(db: &Arc<Mutex<Connection>>, tmdb: Option<&T>, ops: Vec<Op>) -> ApplyOutcome` (async; prefix semantics)

- [ ] **Step 1: Write failing tests**

```rust
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
```

And the two-DB convergence test (async, uses a mock Tmdb):

```rust
struct MockTmdb;

#[async_trait::async_trait]
impl crate::tmdb::Tmdb for MockTmdb {
    async fn find_show(&self, _: i64) -> crate::error::Result<Option<i64>> { Ok(None) }
    async fn find_movie(&self, _: &str) -> crate::error::Result<Option<i64>> { Ok(None) }
    async fn show_with_episodes(&self, id: i64) -> crate::error::Result<crate::tmdb::ShowFull> {
        Ok(crate::tmdb::ShowFull {
            meta: crate::tmdb::ShowMeta {
                tmdb_id: id, name: format!("Show {id}"), year: Some(2020),
                poster_path: None, backdrop_path: None, overview: None, status: None,
            },
            episodes: vec![crate::tmdb::EpisodeMeta {
                tmdb_id: id * 100 + 1, season_number: 1, episode_number: 1,
                name: None, air_date: None, runtime: None, still_path: None, overview: None,
            }],
        })
    }
    async fn movie(&self, id: i64) -> crate::error::Result<crate::tmdb::MovieMeta> {
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
        log_show_add(&c, 1, "watching", "2026-07-01T00:00:00Z").unwrap();
        episode_mark(&c, 10, true, "2026-07-01T01:00:00Z").unwrap();
    }
    // B favorites a movie it adds
    {
        let c = b.lock().unwrap();
        conn_exec(&c, "INSERT INTO movies(tmdb_id,title,my_state,added_at)
                       VALUES(50,'M','watchlist','2026-06-01T00:00:00Z')");
        log_movie_add(&c, 50, "watchlist", None, "2026-07-01T00:00:00Z").unwrap();
        movie_toggle_favorite(&c, 50, "2026-07-01T01:00:00Z").unwrap();
    }
    let ops_a = { let c = a.lock().unwrap(); ops_since(&c, 0).unwrap() };
    let ops_b = { let c = b.lock().unwrap(); ops_since(&c, 0).unwrap() };
    let out_b = apply_ops(&b, Some(&MockTmdb), ops_a.clone()).await;
    let out_a = apply_ops(&a, Some(&MockTmdb), ops_b.clone()).await;
    assert!(out_a.error.is_none() && out_b.error.is_none());
    // converged: B has the show + watch, A has the movie + favorite
    {
        let c = b.lock().unwrap();
        let n: i64 = c.query_row("SELECT COUNT(*) FROM episode_watches WHERE episode_tmdb_id=10",
                                 [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }
    {
        let c = a.lock().unwrap();
        let fav: i64 = c.query_row("SELECT is_favorite FROM movies WHERE tmdb_id=50",
                                   [], |r| r.get(0)).unwrap();
        assert_eq!(fav, 1);
    }
    // re-applying the same batches changes nothing (idempotence)
    let out_b2 = apply_ops(&b, Some(&MockTmdb), ops_a).await;
    assert!(out_b2.error.is_none());
}
```

(Define a small `fn conn_exec(c: &Connection, sql: &str)` test helper that unwraps `execute_batch`. NOTE: `fixture` inserts show 1 with episodes directly, so B applying "show add" for id 1 goes through the MockTmdb fetch path; the mock's episode ids differ from A's — the watch op for ep 10 then exercises the payload-`show` fetch path. That is intended: apply must fetch show 1's episodes when ep 10 is missing, and since MockTmdb returns ep 101 only, ep 10 stays missing → the op is SKIPPED, not an error. Assert accordingly: change the B-side assertion to `n == 0` and `out_b.skipped >= 1` if you keep MockTmdb minimal, or extend the mock to return episode 10 for show 1 — prefer extending the mock: `tmdb_id: if id == 1 { 10 } else { id * 100 + 1 }` — then the convergence assertion above holds as written.)

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri && cargo test oplog`
Expected: FAIL (apply functions not defined).

- [ ] **Step 3: Implement apply**

```rust
use std::sync::{Arc, Mutex};

/// Ops in the same group compete under last-write-wins.
fn group_ops(entity: &str, op: &str) -> &'static [&'static str] {
    match (entity, op) {
        ("ep", _) => &["watch", "unwatch"],
        (_, "favorite") => &["favorite"],
        (_, "rewatch") => &["rewatch"],
        _ => &["add", "state"],
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
        _ => Ok(false), // unknown op kind from a future version: skip, never fail
    }
}

#[derive(Debug, Default, serde::Serialize)]
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
                match t.show_with_episodes(show_id).await {
                    Ok(full) => {
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
                    Err(e) => { out.error = Some(format!("fetch show {show_id}: {e}")); break; }
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
                match t.movie(id).await {
                    Ok(meta) => {
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
                    Err(e) => { out.error = Some(format!("fetch movie {id}: {e}")); break; }
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
```

Note: `apply_op_local` needs `use rusqlite::OptionalExtension;` at module level.

- [ ] **Step 4: Run and fix until green**

Run: `cd src-tauri && cargo test`
Expected: all pass, zero warnings.

- [ ] **Step 5: Commit**

```bash
git add src-tauri
git commit -m "feat(sync): idempotent LWW op application with prefix batches"
```

---

### Task 4: Wire types + snapshot build/apply

**Files:**
- Create: `src-tauri/src/sync_proto.rs`
- Modify: `src-tauri/src/lib.rs` (module decl)

**Interfaces:**
- Consumes: `oplog::{Op, latest_seq, sync_get, sync_set}`, `queries::setting`.
- Produces (all `Serialize + Deserialize`):
  - `PairRequest { device_id: String, code: String }`, `PairResponse { token: String, device_id: String }`
  - `ChangesResponse { ops: Vec<Op>, latest_seq: i64 }`
  - `PushRequest { device_id: String, ops: Vec<Op> }`, `PushResponse { applied_through: i64, error: Option<String> }`
  - `Snapshot { device_id, seq, tmdb_key: Option<String>, shows: Vec<SnapShow>, episodes: Vec<SnapEpisode>, watches: Vec<SnapWatch>, movies: Vec<SnapMovie> }` with row structs mirroring every table column
  - `build_snapshot(conn) -> Result<Snapshot>`
  - `apply_snapshot(conn, snap: &Snapshot) -> Result<(usize, usize)>` — returns (shows added, movies added); merge semantics; sets `peer_cursor` = snap.seq and stores `tmdb_key` only if none is set locally

- [ ] **Step 1: Write the failing round-trip test**

```rust
#[test]
fn snapshot_round_trips_into_empty_db() {
    let a = crate::db::open_memory().unwrap();
    a.execute_batch(
        "INSERT INTO shows(tmdb_id,name,year,my_state,is_favorite,added_at)
           VALUES(1,'A',2020,'watching',1,'2026-06-01T00:00:00Z');
         INSERT INTO episodes(tmdb_id,show_tmdb_id,season_number,episode_number,air_date)
           VALUES(10,1,1,1,'2026-06-01');
         INSERT INTO episode_watches(episode_tmdb_id,watched_at,source) VALUES(10,NULL,'import');
         INSERT INTO movies(tmdb_id,title,my_state,rewatch_count,is_favorite,added_at)
           VALUES(50,'M','watched',2,0,'2026-06-01T00:00:00Z');
         INSERT INTO settings(key,value) VALUES('tmdb_key','k123');").unwrap();
    crate::oplog::append(&a, "show", 1, "state", None, "2026-06-02T00:00:00Z").unwrap();

    let snap = build_snapshot(&a).unwrap();
    let json = serde_json::to_string(&snap).unwrap();
    let snap2: Snapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(snap2.seq, 1);

    let b = crate::db::open_memory().unwrap();
    let (s, m) = apply_snapshot(&b, &snap2).unwrap();
    assert_eq!((s, m), (1, 1));
    let src: String = b.query_row(
        "SELECT source FROM episode_watches WHERE episode_tmdb_id=10", [], |r| r.get(0)).unwrap();
    assert_eq!(src, "import"); // provenance preserved
    assert_eq!(crate::oplog::sync_get(&b, "peer_cursor").unwrap().unwrap(), "1");
    assert_eq!(crate::queries::setting(&b, "tmdb_key").unwrap().unwrap(), "k123");
    let fav: i64 = b.query_row("SELECT is_favorite FROM shows WHERE tmdb_id=1", [], |r| r.get(0)).unwrap();
    assert_eq!(fav, 1);
}

#[test]
fn snapshot_never_clobbers_existing_local_state() {
    let a = crate::db::open_memory().unwrap();
    a.execute_batch("INSERT INTO shows(tmdb_id,name,my_state,added_at)
                     VALUES(1,'A','watching','2026-06-01T00:00:00Z')").unwrap();
    let snap = build_snapshot(&a).unwrap();
    let b = crate::db::open_memory().unwrap();
    b.execute_batch(
        "INSERT INTO shows(tmdb_id,name,my_state,added_at) VALUES(1,'A','stopped','2026-05-01T00:00:00Z');
         INSERT INTO settings(key,value) VALUES('tmdb_key','mine');").unwrap();
    apply_snapshot(&b, &snap).unwrap();
    let st: String = b.query_row("SELECT my_state FROM shows WHERE tmdb_id=1", [], |r| r.get(0)).unwrap();
    assert_eq!(st, "stopped"); // phone-local state survives
    assert_eq!(crate::queries::setting(&b, "tmdb_key").unwrap().unwrap(), "mine");
}
```

- [ ] **Step 2: Run to verify failure, then implement**

`src-tauri/src/sync_proto.rs`:

```rust
//! Wire types shared by the desktop sync server and the phone sync client,
//! plus full-library snapshot build/apply (the phone's bootstrap).

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::oplog::{self, Op};

#[derive(Serialize, Deserialize)]
pub struct PairRequest { pub device_id: String, pub code: String }

#[derive(Serialize, Deserialize)]
pub struct PairResponse { pub token: String, pub device_id: String }

#[derive(Serialize, Deserialize)]
pub struct ChangesResponse { pub ops: Vec<Op>, pub latest_seq: i64 }

#[derive(Serialize, Deserialize)]
pub struct PushRequest { pub device_id: String, pub ops: Vec<Op> }

#[derive(Serialize, Deserialize)]
pub struct PushResponse { pub applied_through: i64, pub error: Option<String> }

#[derive(Serialize, Deserialize)]
pub struct SnapShow {
    pub tmdb_id: i64, pub tvdb_id: Option<i64>, pub imdb_id: Option<String>,
    pub name: String, pub year: Option<i64>, pub poster_path: Option<String>,
    pub backdrop_path: Option<String>, pub overview: Option<String>,
    pub tmdb_status: Option<String>, pub my_state: String, pub is_favorite: i64,
    pub added_at: String, pub last_refreshed_at: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct SnapEpisode {
    pub tmdb_id: i64, pub show_tmdb_id: i64, pub season_number: i64, pub episode_number: i64,
    pub name: Option<String>, pub air_date: Option<String>, pub runtime: Option<i64>,
    pub still_path: Option<String>, pub overview: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct SnapWatch {
    pub episode_tmdb_id: i64, pub watched_at: Option<String>, pub source: String,
}

#[derive(Serialize, Deserialize)]
pub struct SnapMovie {
    pub tmdb_id: i64, pub tvdb_id: Option<i64>, pub imdb_id: Option<String>,
    pub tvtime_uuid: Option<String>, pub title: String, pub year: Option<i64>,
    pub release_date: Option<String>, pub runtime: Option<i64>,
    pub poster_path: Option<String>, pub backdrop_path: Option<String>,
    pub overview: Option<String>, pub my_state: String, pub watched_at: Option<String>,
    pub rewatch_count: i64, pub is_favorite: i64, pub added_at: String,
}

#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    pub device_id: String,
    pub seq: i64,
    pub tmdb_key: Option<String>,
    pub shows: Vec<SnapShow>,
    pub episodes: Vec<SnapEpisode>,
    pub watches: Vec<SnapWatch>,
    pub movies: Vec<SnapMovie>,
}

pub fn build_snapshot(conn: &Connection) -> Result<Snapshot> {
    let shows = conn.prepare(
        "SELECT tmdb_id,tvdb_id,imdb_id,name,year,poster_path,backdrop_path,overview,
                tmdb_status,my_state,is_favorite,added_at,last_refreshed_at FROM shows")?
        .query_map([], |r| Ok(SnapShow {
            tmdb_id: r.get(0)?, tvdb_id: r.get(1)?, imdb_id: r.get(2)?, name: r.get(3)?,
            year: r.get(4)?, poster_path: r.get(5)?, backdrop_path: r.get(6)?,
            overview: r.get(7)?, tmdb_status: r.get(8)?, my_state: r.get(9)?,
            is_favorite: r.get(10)?, added_at: r.get(11)?, last_refreshed_at: r.get(12)?,
        }))?.collect::<std::result::Result<_, _>>()?;
    let episodes = conn.prepare(
        "SELECT tmdb_id,show_tmdb_id,season_number,episode_number,name,air_date,runtime,
                still_path,overview FROM episodes")?
        .query_map([], |r| Ok(SnapEpisode {
            tmdb_id: r.get(0)?, show_tmdb_id: r.get(1)?, season_number: r.get(2)?,
            episode_number: r.get(3)?, name: r.get(4)?, air_date: r.get(5)?,
            runtime: r.get(6)?, still_path: r.get(7)?, overview: r.get(8)?,
        }))?.collect::<std::result::Result<_, _>>()?;
    let watches = conn.prepare(
        "SELECT episode_tmdb_id,watched_at,source FROM episode_watches")?
        .query_map([], |r| Ok(SnapWatch {
            episode_tmdb_id: r.get(0)?, watched_at: r.get(1)?, source: r.get(2)?,
        }))?.collect::<std::result::Result<_, _>>()?;
    let movies = conn.prepare(
        "SELECT tmdb_id,tvdb_id,imdb_id,tvtime_uuid,title,year,release_date,runtime,
                poster_path,backdrop_path,overview,my_state,watched_at,rewatch_count,
                is_favorite,added_at FROM movies")?
        .query_map([], |r| Ok(SnapMovie {
            tmdb_id: r.get(0)?, tvdb_id: r.get(1)?, imdb_id: r.get(2)?, tvtime_uuid: r.get(3)?,
            title: r.get(4)?, year: r.get(5)?, release_date: r.get(6)?, runtime: r.get(7)?,
            poster_path: r.get(8)?, backdrop_path: r.get(9)?, overview: r.get(10)?,
            my_state: r.get(11)?, watched_at: r.get(12)?, rewatch_count: r.get(13)?,
            is_favorite: r.get(14)?, added_at: r.get(15)?,
        }))?.collect::<std::result::Result<_, _>>()?;
    Ok(Snapshot {
        device_id: oplog::sync_get(conn, "device_id")?.unwrap_or_default(),
        seq: oplog::latest_seq(conn)?,
        tmdb_key: crate::queries::setting(conn, "tmdb_key")?,
        shows, episodes, watches, movies,
    })
}

/// Merge a snapshot: metadata upserts always; user state and watches only for
/// entities this DB does not already have (mirrors re-import semantics).
pub fn apply_snapshot(conn: &Connection, snap: &Snapshot) -> Result<(usize, usize)> {
    let tx = conn.unchecked_transaction()?;
    let (mut new_shows, mut new_movies) = (0usize, 0usize);
    for s in &snap.shows {
        let existed = crate::queries::show_exists(&tx, s.tmdb_id)?;
        tx.execute(
            "INSERT INTO shows(tmdb_id,tvdb_id,imdb_id,name,year,poster_path,backdrop_path,
                               overview,tmdb_status,my_state,is_favorite,added_at,last_refreshed_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
             ON CONFLICT(tmdb_id) DO UPDATE SET
               tvdb_id=COALESCE(excluded.tvdb_id,tvdb_id), imdb_id=COALESCE(excluded.imdb_id,imdb_id),
               name=excluded.name, year=excluded.year, poster_path=excluded.poster_path,
               backdrop_path=excluded.backdrop_path, overview=excluded.overview,
               tmdb_status=excluded.tmdb_status",
            params![s.tmdb_id, s.tvdb_id, s.imdb_id, s.name, s.year, s.poster_path,
                    s.backdrop_path, s.overview, s.tmdb_status, s.my_state, s.is_favorite,
                    s.added_at, s.last_refreshed_at])?;
        if !existed { new_shows += 1; }
    }
    for e in &snap.episodes {
        tx.execute(
            "INSERT INTO episodes(tmdb_id,show_tmdb_id,season_number,episode_number,name,
                                  air_date,runtime,still_path,overview)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(tmdb_id) DO UPDATE SET
               show_tmdb_id=excluded.show_tmdb_id, season_number=excluded.season_number,
               episode_number=excluded.episode_number, name=excluded.name,
               air_date=excluded.air_date, runtime=excluded.runtime,
               still_path=excluded.still_path, overview=excluded.overview",
            params![e.tmdb_id, e.show_tmdb_id, e.season_number, e.episode_number, e.name,
                    e.air_date, e.runtime, e.still_path, e.overview])?;
    }
    for w in &snap.watches {
        tx.execute(
            "INSERT OR IGNORE INTO episode_watches(episode_tmdb_id,watched_at,source)
             VALUES(?1,?2,?3)",
            params![w.episode_tmdb_id, w.watched_at, w.source])?;
    }
    for m in &snap.movies {
        let existed = crate::queries::movie_exists(&tx, m.tmdb_id)?;
        tx.execute(
            "INSERT INTO movies(tmdb_id,tvdb_id,imdb_id,tvtime_uuid,title,year,release_date,
                                runtime,poster_path,backdrop_path,overview,my_state,watched_at,
                                rewatch_count,is_favorite,added_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
             ON CONFLICT(tmdb_id) DO UPDATE SET
               tvdb_id=COALESCE(excluded.tvdb_id,tvdb_id), imdb_id=COALESCE(excluded.imdb_id,imdb_id),
               tvtime_uuid=COALESCE(excluded.tvtime_uuid,tvtime_uuid), title=excluded.title,
               year=excluded.year, release_date=excluded.release_date, runtime=excluded.runtime,
               poster_path=excluded.poster_path, backdrop_path=excluded.backdrop_path,
               overview=excluded.overview",
            params![m.tmdb_id, m.tvdb_id, m.imdb_id, m.tvtime_uuid, m.title, m.year,
                    m.release_date, m.runtime, m.poster_path, m.backdrop_path, m.overview,
                    m.my_state, m.watched_at, m.rewatch_count, m.is_favorite, m.added_at])?;
        if !existed { new_movies += 1; }
    }
    if crate::queries::setting(&tx, "tmdb_key")?.is_none() {
        if let Some(k) = &snap.tmdb_key {
            crate::queries::set_setting(&tx, "tmdb_key", k)?;
        }
    }
    oplog::sync_set(&tx, "peer_cursor", &snap.seq.to_string())?;
    tx.commit()?;
    Ok((new_shows, new_movies))
}
```

Add `pub mod sync_proto;` to `lib.rs`.

NOTE: the show/movie upsert here intentionally leaves `my_state`/`is_favorite`/`watched_at`/`rewatch_count` untouched on conflict (only metadata columns update) — that is the never-clobber rule the second test locks in.

- [ ] **Step 3: Run and verify green**

Run: `cd src-tauri && cargo test sync_proto`
Expected: PASS. Then full `cargo test`: all green, zero warnings.

- [ ] **Step 4: Commit**

```bash
git add src-tauri
git commit -m "feat(sync): wire types and snapshot build/apply with merge semantics"
```

---

### Task 5: Desktop sync server (axum router)

**Files:**
- Create: `src-tauri/src/sync_server.rs`
- Modify: `src-tauri/src/lib.rs` (module decl)
- Modify: `src-tauri/Cargo.toml`

**Interfaces:**
- Consumes: `sync_proto::*`, `oplog::*`, `import::with_db`, `tmdb::TmdbClient`.
- Produces:
  - `sync_server::SyncShared { pair: std::sync::Mutex<Option<PairCode>> }`, `PairCode { code: String, expires: std::time::Instant }`
  - `sync_server::ServerCtx { db: Arc<Mutex<Connection>>, http: reqwest::Client, shared: Arc<SyncShared>, backups_dir: PathBuf }`
  - `sync_server::router(ctx: Arc<ServerCtx>) -> axum::Router`
  - `sync_server::start(ctx: Arc<ServerCtx>, port: u16) -> Result<SyncServerHandle>` (async), `SyncServerHandle { port: u16, shutdown: tokio::sync::watch::Sender<bool> }`
  - `sync_server::new_pair_code(shared) -> String`, `sync_server::mint_token() -> String`
  - Routes: `POST /sync/v1/pair`, `GET /sync/v1/snapshot`, `GET /sync/v1/changes?since=N`, `POST /sync/v1/changes`

- [ ] **Step 1: Add dependencies**

`src-tauri/Cargo.toml`:

```toml
axum = { version = "0.8", default-features = false, features = ["http1", "json", "query", "tokio"] }
subtle = "2"
```

Extend tokio features: `tokio = { version = "1", features = ["sync", "fs", "macros", "rt", "time", "net"] }`.

```toml
[dev-dependencies]
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
```

- [ ] **Step 2: Write failing router tests**

In `sync_server.rs` tests (router-level via `tower::ServiceExt::oneshot`):

```rust
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
}
```

NOTE on the backup test: an in-memory DB cannot `VACUUM INTO`? It can — SQLite supports `VACUUM INTO 'file'` from a `:memory:` connection. The assertion stands.

- [ ] **Step 3: Implement the server**

```rust
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
    tauri::async_runtime::spawn(async move {
        let _ = axum::serve(listener, router(ctx))
            .with_graceful_shutdown(async move { let _ = rx.changed().await; })
            .await;
    });
    Ok(SyncServerHandle { port, shutdown: tx })
}
```

Add `pub mod sync_server;` to `lib.rs`. Check `error.rs` for the exact `AppError::msg` constructor name and the `From<std::io::Error>` impl before using `?` on `fs` calls; adapt if the error type differs.

- [ ] **Step 4: Run and fix until green**

Run: `cd src-tauri && cargo test sync_server`
Expected: 3 tests pass. Then full `cargo test` green, zero warnings.

- [ ] **Step 5: Commit**

```bash
git add src-tauri
git commit -m "feat(sync): axum LAN sync server with pairing, auth, rolling backups"
```

---

### Task 6: Server lifecycle + desktop sync commands

**Files:**
- Modify: `src-tauri/src/lib.rs` (AppState, autostart, registration)
- Modify: `src-tauri/src/commands.rs` (sync command section)
- Modify: `src/lib/api.ts`, `src/lib/types.ts`

**Interfaces:**
- Consumes: `sync_server::{ServerCtx, SyncShared, SyncServerHandle, start, new_pair_code}`.
- Produces commands (registered in `generate_handler`):
  - `sync_server_status() -> SyncServerStatus { running: bool, enabled: bool, port: u16, pairedDevice: Option<String>, lastPushAt: Option<String> }` (serde camelCase)
  - `sync_server_set(enabled: bool, port: u16) -> SyncServerStatus` (persists `sync_enabled`/`sync_port`, starts/stops the server)
  - `sync_pair_code() -> String` (errors if the server is not running)
  - `sync_unpair() -> ()` (clears `token`, `paired_device`, `peer_addr`, `peer_cursor`, `push_cursor`, `last_sync`, `last_push_at`)
- Produces AppState fields: `sync_shared: Arc<SyncShared>`, `sync_server: Arc<Mutex<Option<SyncServerHandle>>>`, `backups_dir: PathBuf`
- api.ts: `syncServerStatus()`, `syncServerSet(enabled, port)`, `syncPairCode()`, `syncUnpair()`

- [ ] **Step 1: Extend AppState and setup in `lib.rs`**

```rust
pub struct AppState {
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub http: reqwest::Client,
    pub busy: Arc<AtomicBool>,
    pub sync_shared: Arc<sync_server::SyncShared>,
    pub sync_server: Arc<Mutex<Option<sync_server::SyncServerHandle>>>,
    pub backups_dir: std::path::PathBuf,
}
```

In `setup`, after `app.manage(...)` construct with the new fields (`backups_dir: data_dir.join("backups")`, `sync_shared: Arc::new(sync_server::SyncShared { pair: Mutex::new(None) })`, `sync_server: Arc::new(Mutex::new(None))`), then autostart (desktop only):

```rust
            #[cfg(not(target_os = "android"))]
            {
                let state = app.state::<AppState>();
                let (enabled, port) = {
                    let c = state.db.lock().unwrap();
                    let en = oplog::sync_get(&c, "sync_enabled").ok().flatten()
                        .is_some_and(|v| v == "1");
                    let p = oplog::sync_get(&c, "sync_port").ok().flatten()
                        .and_then(|v| v.parse().ok()).unwrap_or(47810u16);
                    (en, p)
                };
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
```

Register the four new commands in `generate_handler` (after `commands::airing_list`): `commands::sync_server_status, commands::sync_server_set, commands::sync_pair_code, commands::sync_unpair, commands::sync_pair, commands::sync_now, commands::sync_status` (the last three arrive in Task 8 — add them to the macro in Task 8, not now).

- [ ] **Step 2: Implement the desktop commands** (new section at the bottom of `commands.rs`)

```rust
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
    // stop whatever is running
    if let Some(h) = state.sync_server.lock().unwrap().take() {
        let _ = h.shutdown.send(true);
    }
    if enabled {
        let ctx = std::sync::Arc::new(sync_server::ServerCtx {
            db: state.db.clone(), http: state.http.clone(),
            shared: state.sync_shared.clone(), backups_dir: state.backups_dir.clone(),
        });
        let h = sync_server::start(ctx, port).await.map_err(|e| e.to_string())?;
        *state.sync_server.lock().unwrap() = Some(h);
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
```

- [ ] **Step 3: api.ts + types.ts**

`src/lib/types.ts`:

```ts
export interface SyncServerStatus {
  running: boolean;
  enabled: boolean;
  port: number;
  pairedDevice: string | null;
  lastPushAt: string | null;
}

export interface SyncSummary {
  pulled: number;
  pushed: number;
}

export interface SyncStatus {
  paired: boolean;
  peerAddr: string | null;
  lastSync: string | null;
}

export interface PairSummary {
  shows: number;
  movies: number;
}
```

`src/lib/api.ts` (import the new types; add before the closing brace):

```ts
  syncServerStatus: () => invoke<SyncServerStatus>("sync_server_status"),
  syncServerSet: (enabled: boolean, port: number) =>
    invoke<SyncServerStatus>("sync_server_set", { enabled, port }),
  syncPairCode: () => invoke<string>("sync_pair_code"),
  syncUnpair: () => invoke<void>("sync_unpair"),
  syncPair: (addr: string, code: string) => invoke<PairSummary>("sync_pair", { addr, code }),
  syncNow: () => invoke<SyncSummary>("sync_now"),
  syncStatus: () => invoke<SyncStatus>("sync_status"),
```

(The last three commands land in Task 8; TypeScript only needs the invoke names to exist at runtime by the time the UI calls them.)

- [ ] **Step 4: Gates**

Run: `cd src-tauri && cargo test` (green, zero warnings) and `pnpm check` (0/0). `cargo build` to prove the desktop target compiles with the new AppState.

- [ ] **Step 5: Commit**

```bash
git add src-tauri src/lib
git commit -m "feat(sync): server lifecycle, autostart and desktop sync commands"
```

---

### Task 7: Desktop Settings panel

**Files:**
- Modify: `src/routes/settings/+page.svelte`

**Interfaces:**
- Consumes: `api.syncServerStatus/syncServerSet/syncPairCode/syncUnpair`, `SyncServerStatus` type, `isAndroid`.

- [ ] **Step 1: Add the panel** (desktop only, after the "Refresh airing shows" panel)

Script additions:

```ts
  import type { SyncServerStatus } from "$lib/types";

  let sync = $state<SyncServerStatus | null>(null);
  let syncPort = $state(47810);
  let pairCode = $state("");
  let syncMsg = $state("");

  // in onMount, alongside the other fetches:
  if (!isAndroid) {
    api.syncServerStatus().then((s) => { sync = s; syncPort = s.port; });
  }

  async function toggleSync() {
    if (!sync) return;
    syncMsg = "";
    pairCode = "";
    try {
      sync = await api.syncServerSet(!sync.enabled, syncPort);
    } catch (e) {
      syncMsg = String(e);
    }
  }

  async function applyPort() {
    if (!sync) return;
    try {
      sync = await api.syncServerSet(sync.enabled, syncPort);
    } catch (e) {
      syncMsg = String(e);
    }
  }

  async function showCode() {
    try {
      pairCode = await api.syncPairCode();
    } catch (e) {
      syncMsg = String(e);
    }
  }

  async function unpair() {
    await api.syncUnpair();
    pairCode = "";
    sync = await api.syncServerStatus();
  }
```

Markup:

```svelte
  {#if !isAndroid && sync}
    <div class="panel section">
      <h2 class="ph">Phone sync</h2>
      <p class="dim">
        Pairs the Android app over your home network. The server only runs while
        this app is open. Find this PC's address with ipconfig.
      </p>
      <div class="row" style="align-items:center">
        <button class={sync.enabled ? "btn-gold" : "btn-ghost"} onclick={toggleSync}>
          {sync.enabled ? "Sync is on" : "Turn sync on"}
        </button>
        <input style="width:110px" type="number" bind:value={syncPort} min="1024" max="65535" />
        <button class="btn-ghost" onclick={applyPort} disabled={!sync.enabled}>Apply port</button>
      </div>
      {#if sync.running}
        <div class="row" style="align-items:center">
          <button class="btn-ghost" onclick={showCode}>Show pairing code</button>
          {#if pairCode}<span class="code">{pairCode}</span><span class="dim">valid 5 minutes, one use</span>{/if}
        </div>
        <p class="dim">
          {sync.pairedDevice
            ? `Paired phone: ${sync.pairedDevice}${sync.lastPushAt ? ` · last push ${sync.lastPushAt}` : ""}`
            : "No phone paired yet."}
          {#if sync.pairedDevice}<button class="unpairbtn" onclick={unpair}>Unpair</button>{/if}
        </p>
      {/if}
      {#if syncMsg}<p style="color:var(--red)">{syncMsg}</p>{/if}
    </div>
  {/if}
```

Style additions:

```css
  .code {
    font-family: "Anton", sans-serif; font-size: 30px; letter-spacing: 0.35em;
    color: var(--gold); padding: 2px 10px;
  }
  .unpairbtn { margin-left: 10px; font-size: 12px; padding: 4px 12px; }
```

- [ ] **Step 2: Gates + visual check**

`pnpm check` (0/0), `pnpm build`. Then run the desktop app with the WebView2 debug port
(`$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"`), open Settings via
chrome-devtools MCP, toggle sync on, click "Show pairing code", screenshot the panel. Verify
`netstat -an | findstr 47810` shows LISTENING while on, gone after toggling off.

- [ ] **Step 3: Commit**

```bash
git add src/routes/settings
git commit -m "feat(sync): desktop Phone sync settings panel"
```

---

### Task 8: Phone sync client

**Files:**
- Create: `src-tauri/src/sync_client.rs`
- Modify: `src-tauri/src/lib.rs` (module decl + register `sync_pair`, `sync_now`, `sync_status`)
- Modify: `src-tauri/src/commands.rs` (the three client commands)

**Interfaces:**
- Consumes: `sync_proto::*`, `oplog::{ops_since, apply_ops, sync_get, sync_set}`, `apply_snapshot`, `TmdbClient`.
- Produces:
  - `sync_client::pair(http, db, addr: &str, code: &str) -> Result<(usize, usize)>` (pairs + snapshot bootstrap; returns (shows, movies) added)
  - `sync_client::sync_now(http, db, tmdb: Option<&TmdbClient>) -> Result<(usize, usize)>` (returns (pulled_applied, pushed))
  - Commands: `sync_pair(addr, code) -> PairSummary { shows, movies }`, `sync_now() -> SyncSummary { pulled, pushed }`, `sync_status() -> SyncStatus { paired, peerAddr, lastSync }` (serde camelCase)

- [ ] **Step 1: Write the failing loopback integration test**

In `sync_client.rs` — spins the real axum router on `127.0.0.1:0` and drives the full loop over HTTP:

```rust
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
```

- [ ] **Step 2: Run to verify failure, then implement**

```rust
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
        return Err(AppError::msg("wrong or expired pairing code"));
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
```

Commands in `commands.rs`:

```rust
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
```

Add `pub mod sync_client;` to `lib.rs` and register `commands::sync_pair, commands::sync_now, commands::sync_status` in `generate_handler`.

- [ ] **Step 3: Run and fix until green**

Run: `cd src-tauri && cargo test`
Expected: all pass including the loopback integration test, zero warnings.

- [ ] **Step 4: Commit**

```bash
git add src-tauri
git commit -m "feat(sync): phone client with pair, snapshot bootstrap and two-way sync"
```

---

### Task 9: Phone Settings panel + auto-sync

**Files:**
- Modify: `src/routes/settings/+page.svelte` (Android panel)
- Modify: `src/routes/+layout.svelte` (auto-sync on launch/foreground)

**Interfaces:**
- Consumes: `api.syncPair/syncNow/syncStatus/syncUnpair`, `SyncStatus/SyncSummary/PairSummary`, `session.banner`, `session.refreshTick`, `isAndroid`.

- [ ] **Step 1: Android Settings panel** (placed right after the TMDB key panel)

Script additions:

```ts
  import type { SyncStatus } from "$lib/types";

  let phoneSync = $state<SyncStatus | null>(null);
  let peerInput = $state("");
  let codeInput = $state("");
  let pairing = $state(false);
  let syncing = $state(false);
  let phoneSyncMsg = $state("");

  // in onMount:
  if (isAndroid) {
    api.syncStatus().then((s) => {
      phoneSync = s;
      if (s.peerAddr) peerInput = s.peerAddr;
    });
  }

  async function doPair() {
    pairing = true;
    phoneSyncMsg = "";
    try {
      const r = await api.syncPair(peerInput.trim(), codeInput.trim());
      phoneSyncMsg = `Paired. Copied ${r.shows} shows and ${r.movies} movies from the desktop.`;
      phoneSync = await api.syncStatus();
      session.refreshTick++;
    } catch (e) {
      phoneSyncMsg = String(e);
    } finally {
      pairing = false;
    }
  }

  async function doSyncNow() {
    syncing = true;
    phoneSyncMsg = "";
    try {
      const r = await api.syncNow();
      phoneSyncMsg = `Synced: ${r.pulled} pulled, ${r.pushed} pushed.`;
      phoneSync = await api.syncStatus();
      if (r.pulled > 0) session.refreshTick++;
    } catch (e) {
      phoneSyncMsg = String(e);
    } finally {
      syncing = false;
    }
  }

  async function doUnpair() {
    await api.syncUnpair();
    phoneSync = await api.syncStatus();
    phoneSyncMsg = "";
  }
```

Markup:

```svelte
  {#if isAndroid && phoneSync}
    <div class="panel section">
      <h2 class="ph">Sync with desktop</h2>
      {#if !phoneSync.paired}
        <p class="dim">
          On the desktop app: Settings, turn Phone sync on, then Show pairing code.
          Enter this PC's address and the code here.
        </p>
        <input placeholder="192.168.1.20:47810" bind:value={peerInput} />
        <input placeholder="6-digit code" inputmode="numeric" maxlength="6" bind:value={codeInput} />
        <button class="btn-gold" onclick={doPair}
                disabled={pairing || peerInput.trim() === "" || codeInput.trim().length !== 6}>
          {pairing ? "Pairing…" : "Pair & copy library"}
        </button>
      {:else}
        <p class="dim">
          Paired with {phoneSync.peerAddr}
          {phoneSync.lastSync ? ` · last sync ${phoneSync.lastSync}` : ""}
        </p>
        <div class="row">
          <button class="btn-gold" onclick={doSyncNow} disabled={syncing}>
            {syncing ? "Syncing…" : "Sync now"}
          </button>
          <button class="btn-ghost" onclick={doUnpair}>Unpair</button>
        </div>
      {/if}
      {#if phoneSyncMsg}<p class="dim">{phoneSyncMsg}</p>{/if}
    </div>
  {/if}
```

- [ ] **Step 2: Auto-sync on launch and foreground** (`src/routes/+layout.svelte`)

In `onMount`, after the refresh block:

```ts
    if (isAndroid) {
      let lastAuto = 0;
      const autoSync = async () => {
        if (Date.now() - lastAuto < 60_000) return;
        lastAuto = Date.now();
        try {
          const st = await api.syncStatus();
          if (!st.paired) return;
          const r = await api.syncNow();
          if (r.pulled > 0) session.refreshTick++;
        } catch {
          // desktop away from home or app closed: quiet note, app fully usable
          session.banner = "Desktop not reachable for sync. Showing local data.";
        }
      };
      autoSync();
      const onVis = () => { if (document.visibilityState === "visible") autoSync(); };
      document.addEventListener("visibilitychange", onVis);
      return () => document.removeEventListener("visibilitychange", onVis);
    }
```

CAUTION: `onMount` already returns a cleanup for the listeners? It does not (the existing one returns nothing). If combining returns gets awkward, register the visibilitychange cleanup separately with its own `onMount(() => { ...; return cleanup; })` block.

- [ ] **Step 3: Gates**

`pnpm check` (0/0), `pnpm build`, `cd src-tauri && cargo test` still green.

- [ ] **Step 4: Commit**

```bash
git add src/routes src/lib
git commit -m "feat(sync): phone pairing UI and auto-sync on launch/foreground"
```

---

### Task 10: On-device verification, builds, docs

**Files:**
- Modify: `README.md`
- Modify: `docs/superpowers/specs/2026-07-09-android-port-design.md` (record any deviations, including the locked decisions listed at the top of this plan)

- [ ] **Step 1: Full gates + desktop build**

`cd src-tauri && cargo test` (all green, zero warnings), `pnpm check`, then
`pnpm tauri build --no-bundle`. If `src-tauri/icons/` changed since the last build remember the
build-script cache gotcha (`touch src-tauri/build.rs`) — not expected here.

- [ ] **Step 2: Android build + install** (FOREGROUND, arm64 only)

```powershell
$env:JAVA_HOME = "C:\Program Files\Android\Android Studio\jbr"
$env:ANDROID_HOME = "$env:LOCALAPPDATA\Android\Sdk"
$env:NDK_HOME = "$env:ANDROID_HOME\ndk\27.2.12479018"
pnpm tauri android build --target aarch64
adb mdns services   # rediscover the wireless port first
adb install -r src-tauri\gen\android\app\build\outputs\apk\universal\release\app-universal-release.apk
```

- [ ] **Step 3: End-to-end pairing on real hardware**

1. Launch the rebuilt desktop exe with the WebView2 debug port; via chrome-devtools MCP open Settings, turn Phone sync on, click Show pairing code, read the code from the DOM.
2. Confirm the desktop firewall allows inbound 47810 (first run may prompt; if pairing times out, add a rule: `netsh advfirewall firewall add rule name="ShowTrack sync" dir=in action=allow protocol=TCP localport=47810`).
3. On the phone via adb: open Settings tab, tap the address field, `adb shell input text "192.168.1.<desktop-ip>:47810"`, enter the code, tap "Pair & copy library". Screenshot the success message (expect roughly 170 shows / 590 movies copied).
4. Couch-mark test: on the phone mark an episode watched, tap Sync now; on the desktop (devtools) reload the show page and verify the mark arrived. Then unmark it on the DESKTOP, on the phone tap Sync now, verify the phone unmarked it (LWW: the desktop unwatch is newer). This leaves the user's real data unchanged.
5. Verify a rolling backup appeared under `%APPDATA%\com.nokama.showtrack\backups\` (or the app_data dir the logs show) after the phone's push.
6. Screenshot: phone paired panel, desktop panel showing paired device + last push.

- [ ] **Step 4: Docs**

README: add a "Phone sync" section (how to pair, port default, backups location, the
"server only runs while the app is open" property). Spec: record the locked design decisions
(oplog stores local ops only / no origin column; prefix-apply; payload.show on watch ops;
snapshot merge semantics; Android-only seeding) under Recorded deviations.

- [ ] **Step 5: Final commit**

```bash
git add README.md docs
git commit -m "docs(sync): phone sync usage, spec deviations for phase 2"
```

---

## Self-Review Notes

- Spec coverage: migration v2 (T1), every-mutation ops (T2), idempotent/LWW apply + origin handling (T3), snapshot (T4), server endpoints + auth + backups + code expiry (T5), Settings server panel + toggle + port + code + unpair (T6/T7), phone pair/bootstrap/auto-sync/manual sync + quiet failure banner (T8/T9), on-device checklist + both-directions couch test (T10). Rust test list from the spec maps: oplog-append-per-mutation (T2), idempotent re-apply + LWW both orders (T3), pair/auth cases (T5), snapshot round-trip (T4), two-DB convergence (T3 + T8 over real HTTP).
- Type consistency: `Op` defined once in T1 and reused; `ServerCtx`/`SyncShared` defined in T5, consumed in T6/T8 tests; command names match api.ts entries added in T6.
- Known judgment calls are listed under "Design decisions locked here" — implementers must not silently change them.
