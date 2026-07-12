use std::path::Path;

use rusqlite::Connection;

use crate::error::Result;

const SCHEMA_V1: &str = "
CREATE TABLE shows (
  tmdb_id INTEGER PRIMARY KEY,
  tvdb_id INTEGER,
  imdb_id TEXT,
  name TEXT NOT NULL,
  year INTEGER,
  poster_path TEXT,
  backdrop_path TEXT,
  overview TEXT,
  tmdb_status TEXT,
  my_state TEXT NOT NULL DEFAULT 'watching'
    CHECK (my_state IN ('watching','stopped','plan_to_watch')),
  is_favorite INTEGER NOT NULL DEFAULT 0,
  added_at TEXT NOT NULL,
  last_refreshed_at TEXT
);
CREATE TABLE episodes (
  tmdb_id INTEGER PRIMARY KEY,
  show_tmdb_id INTEGER NOT NULL REFERENCES shows(tmdb_id) ON DELETE CASCADE,
  season_number INTEGER NOT NULL,
  episode_number INTEGER NOT NULL,
  name TEXT,
  air_date TEXT,
  runtime INTEGER,
  still_path TEXT,
  overview TEXT
);
CREATE INDEX idx_episodes_show ON episodes(show_tmdb_id, season_number, episode_number);
CREATE INDEX idx_episodes_air ON episodes(air_date);
CREATE TABLE episode_watches (
  episode_tmdb_id INTEGER PRIMARY KEY REFERENCES episodes(tmdb_id) ON DELETE CASCADE,
  watched_at TEXT,
  source TEXT NOT NULL DEFAULT 'app' CHECK (source IN ('app','import'))
);
CREATE TABLE movies (
  tmdb_id INTEGER PRIMARY KEY,
  tvdb_id INTEGER,
  imdb_id TEXT,
  tvtime_uuid TEXT,
  title TEXT NOT NULL,
  year INTEGER,
  release_date TEXT,
  runtime INTEGER,
  poster_path TEXT,
  backdrop_path TEXT,
  overview TEXT,
  my_state TEXT NOT NULL DEFAULT 'watchlist' CHECK (my_state IN ('watched','watchlist')),
  watched_at TEXT,
  rewatch_count INTEGER NOT NULL DEFAULT 0,
  is_favorite INTEGER NOT NULL DEFAULT 0,
  added_at TEXT NOT NULL
);
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

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

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(path)?;
    let _mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    init(&conn)?;
    Ok(conn)
}

pub fn open_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    init(&conn)?;
    Ok(conn)
}

fn init(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if v < 1 {
        conn.execute_batch(SCHEMA_V1)?;
        conn.pragma_update(None, "user_version", 1)?;
    }
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
    if v < 3 {
        conn.execute_batch("ALTER TABLE shows ADD COLUMN is_anime INTEGER NOT NULL DEFAULT 0;")?;
        conn.pragma_update(None, "user_version", 3)?;
    }
    if v < 4 {
        // anime movies: user-chosen section, never auto-detected
        conn.execute_batch("ALTER TABLE movies ADD COLUMN is_anime INTEGER NOT NULL DEFAULT 0;")?;
        conn.pragma_update(None, "user_version", 4)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_to_v1_with_foreign_keys() {
        let conn = open_memory().unwrap();
        // Migrations cascade in one init() pass, so a fresh db lands on the
        // latest version (now 2), not exactly 1. Assert the v1 step ran.
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert!(v >= 1);
        let fk: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(fk, 1);
        // all five tables exist
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN
             ('shows','episodes','episode_watches','movies','settings')", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 5);
    }

    #[test]
    fn migrates_to_v2_with_oplog_and_device_id() {
        let conn = open_memory().unwrap();
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 4); // cascades through v4 (movie is_anime) in one pass
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('oplog','sync_state')",
            [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
        let id = crate::oplog::sync_get(&conn, "device_id").unwrap().unwrap();
        assert_eq!(id.len(), 32); // 16 random bytes, hex
    }

    #[test]
    fn cascade_deletes_watches_with_show() {
        let conn = open_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO shows(tmdb_id,name,my_state,added_at) VALUES(1,'X','watching','2026-01-01T00:00:00Z');
             INSERT INTO episodes(tmdb_id,show_tmdb_id,season_number,episode_number) VALUES(10,1,1,1);
             INSERT INTO episode_watches(episode_tmdb_id,watched_at,source) VALUES(10,NULL,'import');
             DELETE FROM shows WHERE tmdb_id=1;").unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM episode_watches", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }
}
