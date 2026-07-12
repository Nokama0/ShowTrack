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
    #[serde(default)] // absent in pre-anime backups
    pub is_anime: i64,
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
    #[serde(default)] // absent in pre-anime-movies backups
    pub is_anime: i64,
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
                tmdb_status,my_state,is_favorite,is_anime,added_at,last_refreshed_at FROM shows")?
        .query_map([], |r| Ok(SnapShow {
            tmdb_id: r.get(0)?, tvdb_id: r.get(1)?, imdb_id: r.get(2)?, name: r.get(3)?,
            year: r.get(4)?, poster_path: r.get(5)?, backdrop_path: r.get(6)?,
            overview: r.get(7)?, tmdb_status: r.get(8)?, my_state: r.get(9)?,
            is_favorite: r.get(10)?, is_anime: r.get(11)?, added_at: r.get(12)?,
            last_refreshed_at: r.get(13)?,
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
                is_favorite,added_at,is_anime FROM movies")?
        .query_map([], |r| Ok(SnapMovie {
            tmdb_id: r.get(0)?, tvdb_id: r.get(1)?, imdb_id: r.get(2)?, tvtime_uuid: r.get(3)?,
            title: r.get(4)?, year: r.get(5)?, release_date: r.get(6)?, runtime: r.get(7)?,
            poster_path: r.get(8)?, backdrop_path: r.get(9)?, overview: r.get(10)?,
            my_state: r.get(11)?, watched_at: r.get(12)?, rewatch_count: r.get(13)?,
            is_favorite: r.get(14)?, added_at: r.get(15)?, is_anime: r.get(16)?,
        }))?.collect::<std::result::Result<_, _>>()?;
    Ok(Snapshot {
        device_id: oplog::sync_get(conn, "device_id")?.unwrap_or_default(),
        seq: oplog::latest_seq(conn)?,
        tmdb_key: crate::queries::setting(conn, "tmdb_key")?,
        shows, episodes, watches, movies,
    })
}

/// Merge a snapshot's library with no sync bookkeeping: metadata upserts
/// always; user state and watches only for entities this DB does not already
/// have (mirrors re-import semantics). Also the backup-restore path.
pub fn apply_library(conn: &Connection, snap: &Snapshot) -> Result<(usize, usize)> {
    let tx = conn.unchecked_transaction()?;
    let (mut new_shows, mut new_movies) = (0usize, 0usize);
    for s in &snap.shows {
        let existed = crate::queries::show_exists(&tx, s.tmdb_id)?;
        tx.execute(
            "INSERT INTO shows(tmdb_id,tvdb_id,imdb_id,name,year,poster_path,backdrop_path,
                               overview,tmdb_status,my_state,is_favorite,is_anime,added_at,last_refreshed_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
             ON CONFLICT(tmdb_id) DO UPDATE SET
               tvdb_id=COALESCE(excluded.tvdb_id,tvdb_id), imdb_id=COALESCE(excluded.imdb_id,imdb_id),
               name=excluded.name, year=excluded.year, poster_path=excluded.poster_path,
               backdrop_path=excluded.backdrop_path, overview=excluded.overview,
               tmdb_status=excluded.tmdb_status, is_anime=excluded.is_anime",
            params![s.tmdb_id, s.tvdb_id, s.imdb_id, s.name, s.year, s.poster_path,
                    s.backdrop_path, s.overview, s.tmdb_status, s.my_state, s.is_favorite,
                    s.is_anime, s.added_at, s.last_refreshed_at])?;
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
                                rewatch_count,is_favorite,added_at,is_anime)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
             ON CONFLICT(tmdb_id) DO UPDATE SET
               tvdb_id=COALESCE(excluded.tvdb_id,tvdb_id), imdb_id=COALESCE(excluded.imdb_id,imdb_id),
               tvtime_uuid=COALESCE(excluded.tvtime_uuid,tvtime_uuid), title=excluded.title,
               year=excluded.year, release_date=excluded.release_date, runtime=excluded.runtime,
               poster_path=excluded.poster_path, backdrop_path=excluded.backdrop_path,
               overview=excluded.overview, is_anime=MAX(is_anime, excluded.is_anime)",
            params![m.tmdb_id, m.tvdb_id, m.imdb_id, m.tvtime_uuid, m.title, m.year,
                    m.release_date, m.runtime, m.poster_path, m.backdrop_path, m.overview,
                    m.my_state, m.watched_at, m.rewatch_count, m.is_favorite, m.added_at,
                    m.is_anime])?;
        if !existed { new_movies += 1; }
    }
    if crate::queries::setting(&tx, "tmdb_key")?.is_none() {
        if let Some(k) = &snap.tmdb_key {
            crate::queries::set_setting(&tx, "tmdb_key", k)?;
        }
    }
    tx.commit()?;
    Ok((new_shows, new_movies))
}

/// Sync bootstrap: the library merge plus the peer cursor.
pub fn apply_snapshot(conn: &Connection, snap: &Snapshot) -> Result<(usize, usize)> {
    let added = apply_library(conn, snap)?;
    oplog::sync_set(conn, "peer_cursor", &snap.seq.to_string())?;
    Ok(added)
}

#[cfg(test)]
mod tests {
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

        let snap = super::build_snapshot(&a).unwrap();
        let json = serde_json::to_string(&snap).unwrap();
        let snap2: super::Snapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap2.seq, 1);

        let b = crate::db::open_memory().unwrap();
        let (s, m) = super::apply_snapshot(&b, &snap2).unwrap();
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
    fn backup_restore_leaves_sync_state_alone_and_is_idempotent() {
        let a = crate::db::open_memory().unwrap();
        a.execute_batch(
            "INSERT INTO shows(tmdb_id,name,my_state,added_at) VALUES(1,'A','watching','2026-06-01T00:00:00Z');
             INSERT INTO movies(tmdb_id,title,my_state,added_at) VALUES(50,'M','watchlist','2026-06-01T00:00:00Z');").unwrap();
        let snap = super::build_snapshot(&a).unwrap();
        let b = crate::db::open_memory().unwrap();
        let (s, m) = super::apply_library(&b, &snap).unwrap();
        assert_eq!((s, m), (1, 1));
        // restore must not fabricate a sync relationship
        assert_eq!(crate::oplog::sync_get(&b, "peer_cursor").unwrap(), None);
        // restoring the same backup again adds nothing
        let (s2, m2) = super::apply_library(&b, &snap).unwrap();
        assert_eq!((s2, m2), (0, 0));
    }

    #[test]
    fn snapshot_never_clobbers_existing_local_state() {
        let a = crate::db::open_memory().unwrap();
        a.execute_batch("INSERT INTO shows(tmdb_id,name,my_state,added_at)
                         VALUES(1,'A','watching','2026-06-01T00:00:00Z')").unwrap();
        let snap = super::build_snapshot(&a).unwrap();
        let b = crate::db::open_memory().unwrap();
        b.execute_batch(
            "INSERT INTO shows(tmdb_id,name,my_state,added_at) VALUES(1,'A','stopped','2026-05-01T00:00:00Z');
             INSERT INTO settings(key,value) VALUES('tmdb_key','mine');").unwrap();
        super::apply_snapshot(&b, &snap).unwrap();
        let st: String = b.query_row("SELECT my_state FROM shows WHERE tmdb_id=1", [], |r| r.get(0)).unwrap();
        assert_eq!(st, "stopped"); // phone-local state survives
        assert_eq!(crate::queries::setting(&b, "tmdb_key").unwrap().unwrap(), "mine");
    }
}
