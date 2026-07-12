use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::Result;
use crate::models::*;
use crate::tmdb::{EpisodeMeta, MovieMeta, ShowMeta};

// ---------- settings ----------

pub fn setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| r.get(0)).optional()?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings(key,value) VALUES(?1,?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value])?;
    Ok(())
}

// ---------- shows ----------

pub fn upsert_show_meta(conn: &Connection, m: &ShowMeta, now: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO shows(tmdb_id,name,year,poster_path,backdrop_path,overview,tmdb_status,is_anime,added_at,last_refreshed_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)
         ON CONFLICT(tmdb_id) DO UPDATE SET
           name=excluded.name, year=excluded.year, poster_path=excluded.poster_path,
           backdrop_path=excluded.backdrop_path, overview=excluded.overview,
           tmdb_status=excluded.tmdb_status, is_anime=MAX(is_anime, excluded.is_anime),
           last_refreshed_at=excluded.last_refreshed_at",
        params![m.tmdb_id, m.name, m.year, m.poster_path, m.backdrop_path, m.overview, m.status,
                m.is_anime as i64, now])?;
    Ok(())
}

pub fn show_exists(conn: &Connection, id: i64) -> Result<bool> {
    Ok(conn.query_row("SELECT 1 FROM shows WHERE tmdb_id=?1", [id], |_| Ok(())).optional()?.is_some())
}

pub fn set_show_user(conn: &Connection, id: i64, state: &str, favorite: bool,
                     tvdb_id: Option<i64>, imdb_id: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE shows SET my_state=?2, is_favorite=?3,
           tvdb_id=COALESCE(?4,tvdb_id), imdb_id=COALESCE(?5,imdb_id) WHERE tmdb_id=?1",
        params![id, state, favorite as i64, tvdb_id, imdb_id])?;
    Ok(())
}

pub fn set_show_state(conn: &Connection, id: i64, state: &str) -> Result<()> {
    conn.execute("UPDATE shows SET my_state=?2 WHERE tmdb_id=?1", params![id, state])?;
    Ok(())
}

pub fn toggle_show_favorite(conn: &Connection, id: i64) -> Result<bool> {
    conn.execute("UPDATE shows SET is_favorite=1-is_favorite WHERE tmdb_id=?1", [id])?;
    Ok(conn.query_row("SELECT is_favorite FROM shows WHERE tmdb_id=?1", [id],
        |r| r.get::<_, i64>(0))? != 0)
}

pub fn upsert_episodes(conn: &Connection, show_id: i64, eps: &[EpisodeMeta]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut st = tx.prepare(
            "INSERT INTO episodes(tmdb_id,show_tmdb_id,season_number,episode_number,name,air_date,runtime,still_path,overview)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(tmdb_id) DO UPDATE SET
               show_tmdb_id=excluded.show_tmdb_id, season_number=excluded.season_number,
               episode_number=excluded.episode_number, name=excluded.name, air_date=excluded.air_date,
               runtime=excluded.runtime, still_path=excluded.still_path, overview=excluded.overview")?;
        for e in eps {
            st.execute(params![
                e.tmdb_id, show_id, e.season_number, e.episode_number, e.name,
                e.air_date.map(|d| d.to_string()), e.runtime, e.still_path, e.overview])?;
        }
    }
    tx.commit()?;
    Ok(())
}

// ---------- watches ----------

pub fn mark_episodes_import(conn: &Connection, ids: &[i64]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut st = tx.prepare(
            "INSERT OR IGNORE INTO episode_watches(episode_tmdb_id,watched_at,source)
             VALUES(?1,NULL,'import')")?;
        for id in ids {
            st.execute([id])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn set_episode_watched(conn: &Connection, ep_id: i64, watched: bool, now: &str) -> Result<()> {
    if watched {
        conn.execute(
            "INSERT OR IGNORE INTO episode_watches(episode_tmdb_id,watched_at,source) VALUES(?1,?2,'app')",
            params![ep_id, now])?;
    } else {
        conn.execute("DELETE FROM episode_watches WHERE episode_tmdb_id=?1", [ep_id])?;
    }
    Ok(())
}

pub fn mark_season(conn: &Connection, show_id: i64, season: i64, watched: bool,
                   today: &str, now: &str) -> Result<()> {
    if watched {
        conn.execute(
            "INSERT OR IGNORE INTO episode_watches(episode_tmdb_id,watched_at,source)
             SELECT tmdb_id, ?4, 'app' FROM episodes
             WHERE show_tmdb_id=?1 AND season_number=?2 AND air_date IS NOT NULL AND air_date<=?3",
            params![show_id, season, today, now])?;
    } else {
        conn.execute(
            "DELETE FROM episode_watches WHERE episode_tmdb_id IN
             (SELECT tmdb_id FROM episodes WHERE show_tmdb_id=?1 AND season_number=?2)",
            params![show_id, season])?;
    }
    Ok(())
}

/// Mark a season's aired episodes watched, returning ONLY the ids this call inserted
/// (already-watched episodes are untouched and not returned).
pub fn mark_season_collect(conn: &Connection, show_id: i64, season: i64,
                           today: &str, now: &str) -> Result<Vec<i64>> {
    let mut st = conn.prepare(
        "SELECT tmdb_id FROM episodes e
         WHERE show_tmdb_id=?1 AND season_number=?2 AND air_date IS NOT NULL AND air_date<=?3
           AND NOT EXISTS (SELECT 1 FROM episode_watches w WHERE w.episode_tmdb_id=e.tmdb_id)")?;
    let ids: Vec<i64> = st.query_map(params![show_id, season, today], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    let tx = conn.unchecked_transaction()?;
    {
        let mut ins = tx.prepare(
            "INSERT OR IGNORE INTO episode_watches(episode_tmdb_id,watched_at,source) VALUES(?1,?2,'app')")?;
        for id in &ids {
            ins.execute(params![id, now])?;
        }
    }
    tx.commit()?;
    Ok(ids)
}

/// Delete exactly the given watch rows (Undo for mark_season_collect).
pub fn unmark_episodes(conn: &Connection, ids: &[i64]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut st = tx.prepare("DELETE FROM episode_watches WHERE episode_tmdb_id=?1")?;
        for id in ids {
            st.execute([id])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Mark every unwatched aired regular episode in seasons BEFORE `before_season`,
/// returning ONLY the ids this call inserted (undo-compatible with unmark_episodes).
pub fn mark_seasons_before(conn: &Connection, show_id: i64, before_season: i64,
                           today: &str, now: &str) -> Result<Vec<i64>> {
    let mut st = conn.prepare(
        "SELECT tmdb_id FROM episodes e
         WHERE show_tmdb_id=?1 AND season_number>0 AND season_number<?2
           AND air_date IS NOT NULL AND air_date<=?3
           AND NOT EXISTS (SELECT 1 FROM episode_watches w WHERE w.episode_tmdb_id=e.tmdb_id)")?;
    let ids: Vec<i64> = st.query_map(params![show_id, before_season, today], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    let tx = conn.unchecked_transaction()?;
    {
        let mut ins = tx.prepare(
            "INSERT OR IGNORE INTO episode_watches(episode_tmdb_id,watched_at,source) VALUES(?1,?2,'app')")?;
        for id in &ids {
            ins.execute(params![id, now])?;
        }
    }
    tx.commit()?;
    Ok(ids)
}

/// Mark unwatched aired episodes of one season BEFORE `before_episode`,
/// returning ONLY the ids this call inserted (undo-compatible with unmark_episodes).
pub fn mark_episodes_before(conn: &Connection, show_id: i64, season: i64, before_episode: i64,
                            today: &str, now: &str) -> Result<Vec<i64>> {
    let mut st = conn.prepare(
        "SELECT tmdb_id FROM episodes e
         WHERE show_tmdb_id=?1 AND season_number=?2 AND episode_number<?3
           AND air_date IS NOT NULL AND air_date<=?4
           AND NOT EXISTS (SELECT 1 FROM episode_watches w WHERE w.episode_tmdb_id=e.tmdb_id)")?;
    let ids: Vec<i64> = st.query_map(params![show_id, season, before_episode, today], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    let tx = conn.unchecked_transaction()?;
    {
        let mut ins = tx.prepare(
            "INSERT OR IGNORE INTO episode_watches(episode_tmdb_id,watched_at,source) VALUES(?1,?2,'app')")?;
        for id in &ids {
            ins.execute(params![id, now])?;
        }
    }
    tx.commit()?;
    Ok(ids)
}

pub fn mark_all_aired(conn: &Connection, show_id: i64, today: &str, now: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO episode_watches(episode_tmdb_id,watched_at,source)
         SELECT tmdb_id, ?3, 'app' FROM episodes
         WHERE show_tmdb_id=?1 AND air_date IS NOT NULL AND air_date<=?2",
        params![show_id, today, now])?;
    Ok(())
}

pub fn watched_regular_count(conn: &Connection, show_id: i64) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM episode_watches w
         JOIN episodes e ON e.tmdb_id=w.episode_tmdb_id
         WHERE e.show_tmdb_id=?1 AND e.season_number>0",
        [show_id], |r| r.get(0))?)
}

// ---------- cards / detail / upcoming ----------

pub fn show_card(conn: &Connection, id: i64, today: &str) -> Result<ShowCard> {
    let (name, poster_path, my_state, is_favorite) = conn.query_row(
        "SELECT name, poster_path, my_state, is_favorite FROM shows WHERE tmdb_id=?1",
        [id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?,
                      r.get::<_, String>(2)?, r.get::<_, i64>(3)?)))?;
    let watched = watched_regular_count(conn, id)?;
    let total_aired: i64 = conn.query_row(
        "SELECT COUNT(*) FROM episodes
         WHERE show_tmdb_id=?1 AND season_number>0 AND air_date IS NOT NULL AND air_date<=?2",
        params![id, today], |r| r.get(0))?;
    let next = conn.query_row(
        "SELECT tmdb_id, season_number, episode_number, name, air_date FROM episodes e
         WHERE show_tmdb_id=?1 AND season_number>0 AND air_date IS NOT NULL AND air_date<=?2
           AND NOT EXISTS (SELECT 1 FROM episode_watches w WHERE w.episode_tmdb_id=e.tmdb_id)
         ORDER BY season_number, episode_number LIMIT 1",
        params![id, today],
        |r| Ok(NextEpisode {
            episode_id: r.get(0)?, season_number: r.get(1)?, episode_number: r.get(2)?,
            name: r.get(3)?, air_date: r.get(4)?,
        })).optional()?;
    let next_air_date: Option<String> = conn.query_row(
        "SELECT MIN(air_date) FROM episodes WHERE show_tmdb_id=?1 AND season_number>0 AND air_date>?2",
        params![id, today], |r| r.get(0))?;
    let last_activity: Option<String> = conn.query_row(
        "SELECT MAX(w.watched_at) FROM episode_watches w
         JOIN episodes e ON e.tmdb_id=w.episode_tmdb_id WHERE e.show_tmdb_id=?1",
        [id], |r| r.get(0))?;
    Ok(ShowCard {
        tmdb_id: id, name, poster_path, my_state,
        is_favorite: is_favorite != 0,
        watched, total_aired, next, next_air_date, last_activity,
    })
}

pub fn list_show_cards(conn: &Connection, today: &str) -> Result<Vec<ShowCard>> {
    let ids: Vec<i64> = conn.prepare("SELECT tmdb_id FROM shows ORDER BY name")?
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    ids.into_iter().map(|id| show_card(conn, id, today)).collect()
}

pub fn show_detail(conn: &Connection, id: i64) -> Result<ShowDetail> {
    let show = conn.query_row(
        "SELECT tmdb_id, tvdb_id, imdb_id, name, year, poster_path, backdrop_path,
                overview, tmdb_status, my_state, is_favorite
         FROM shows WHERE tmdb_id=?1",
        [id],
        |r| Ok(ShowRow {
            tmdb_id: r.get(0)?, tvdb_id: r.get(1)?, imdb_id: r.get(2)?, name: r.get(3)?,
            year: r.get(4)?, poster_path: r.get(5)?, backdrop_path: r.get(6)?,
            overview: r.get(7)?, tmdb_status: r.get(8)?, my_state: r.get(9)?,
            is_favorite: r.get::<_, i64>(10)? != 0,
        }))?;
    let mut st = conn.prepare(
        "SELECT e.tmdb_id, e.season_number, e.episode_number, e.name, e.air_date,
                e.runtime, e.still_path, e.overview, w.watched_at,
                CASE WHEN w.episode_tmdb_id IS NULL THEN 0 ELSE 1 END
         FROM episodes e
         LEFT JOIN episode_watches w ON w.episode_tmdb_id=e.tmdb_id
         WHERE e.show_tmdb_id=?1
         ORDER BY e.season_number, e.episode_number")?;
    let rows = st.query_map([id], |r| {
        Ok(EpisodeRow {
            tmdb_id: r.get(0)?, season_number: r.get(1)?, episode_number: r.get(2)?,
            name: r.get(3)?, air_date: r.get(4)?, runtime: r.get(5)?,
            still_path: r.get(6)?, overview: r.get(7)?,
            watched_at: r.get(8)?, watched: r.get::<_, i64>(9)? != 0,
        })
    })?;
    let mut seasons: Vec<SeasonDetail> = Vec::new();
    for row in rows {
        let e = row?;
        match seasons.last_mut() {
            Some(s) if s.season_number == e.season_number => s.episodes.push(e),
            _ => seasons.push(SeasonDetail { season_number: e.season_number, episodes: vec![e] }),
        }
    }
    Ok(ShowDetail { show, seasons })
}

pub fn list_upcoming(conn: &Connection, today: &str) -> Result<Vec<UpcomingItem>> {
    let mut st = conn.prepare(
        "SELECT e.tmdb_id, e.show_tmdb_id, s.name, s.poster_path,
                e.season_number, e.episode_number, e.name, e.air_date
         FROM episodes e JOIN shows s ON s.tmdb_id=e.show_tmdb_id
         WHERE s.my_state='watching' AND e.air_date IS NOT NULL AND e.air_date>?1
         ORDER BY e.air_date, s.name, e.season_number, e.episode_number
         LIMIT 300")?;
    let rows = st.query_map([today], |r| {
        Ok(UpcomingItem {
            episode_id: r.get(0)?, show_tmdb_id: r.get(1)?, show_name: r.get(2)?,
            poster_path: r.get(3)?, season_number: r.get(4)?, episode_number: r.get(5)?,
            episode_name: r.get(6)?, air_date: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

// ---------- movies ----------

pub fn upsert_movie_meta(conn: &Connection, m: &MovieMeta, now: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO movies(tmdb_id,imdb_id,title,year,release_date,runtime,poster_path,backdrop_path,overview,added_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
         ON CONFLICT(tmdb_id) DO UPDATE SET
           imdb_id=excluded.imdb_id, title=excluded.title, year=excluded.year,
           release_date=excluded.release_date, runtime=excluded.runtime,
           poster_path=excluded.poster_path, backdrop_path=excluded.backdrop_path,
           overview=excluded.overview",
        params![m.tmdb_id, m.imdb_id, m.title, m.year, m.release_date, m.runtime,
                m.poster_path, m.backdrop_path, m.overview, now])?;
    Ok(())
}

pub fn movie_exists(conn: &Connection, id: i64) -> Result<bool> {
    Ok(conn.query_row("SELECT 1 FROM movies WHERE tmdb_id=?1", [id], |_| Ok(())).optional()?.is_some())
}

pub fn set_movie_user(conn: &Connection, id: i64, state: &str, watched_at: Option<&str>,
                      rewatch: i64, favorite: bool, tvdb_id: Option<i64>, uuid: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE movies SET my_state=?2, watched_at=?3, rewatch_count=?4, is_favorite=?5,
           tvdb_id=COALESCE(?6,tvdb_id), tvtime_uuid=COALESCE(?7,tvtime_uuid) WHERE tmdb_id=?1",
        params![id, state, watched_at, rewatch, favorite as i64, tvdb_id, uuid])?;
    Ok(())
}

pub fn set_movie_state(conn: &Connection, id: i64, state: &str, now: &str) -> Result<()> {
    if state == "watched" {
        conn.execute(
            "UPDATE movies SET my_state='watched', watched_at=COALESCE(watched_at,?2) WHERE tmdb_id=?1",
            params![id, now])?;
    } else {
        conn.execute(
            "UPDATE movies SET my_state='watchlist', watched_at=NULL, rewatch_count=0 WHERE tmdb_id=?1",
            [id])?;
    }
    Ok(())
}

pub fn movie_rewatch(conn: &Connection, id: i64) -> Result<i64> {
    conn.execute("UPDATE movies SET rewatch_count=rewatch_count+1 WHERE tmdb_id=?1", [id])?;
    Ok(conn.query_row("SELECT rewatch_count FROM movies WHERE tmdb_id=?1", [id], |r| r.get(0))?)
}

pub fn toggle_movie_favorite(conn: &Connection, id: i64) -> Result<bool> {
    conn.execute("UPDATE movies SET is_favorite=1-is_favorite WHERE tmdb_id=?1", [id])?;
    Ok(conn.query_row("SELECT is_favorite FROM movies WHERE tmdb_id=?1", [id],
        |r| r.get::<_, i64>(0))? != 0)
}

pub fn list_movies(conn: &Connection) -> Result<Vec<MovieRow>> {
    let mut st = conn.prepare(
        "SELECT tmdb_id, imdb_id, title, year, release_date, runtime, poster_path,
                backdrop_path, overview, my_state, watched_at, rewatch_count, is_favorite
         FROM movies ORDER BY title")?;
    let rows = st.query_map([], |r| {
        Ok(MovieRow {
            tmdb_id: r.get(0)?, imdb_id: r.get(1)?, title: r.get(2)?, year: r.get(3)?,
            release_date: r.get(4)?, runtime: r.get(5)?, poster_path: r.get(6)?,
            backdrop_path: r.get(7)?, overview: r.get(8)?, my_state: r.get(9)?,
            watched_at: r.get(10)?, rewatch_count: r.get(11)?,
            is_favorite: r.get::<_, i64>(12)? != 0,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

// ---------- refresh / search support ----------

pub fn shows_to_refresh(conn: &Connection) -> Result<Vec<i64>> {
    let mut st = conn.prepare(
        "SELECT tmdb_id FROM shows
         WHERE my_state='watching' AND tmdb_id > 0
           AND (tmdb_status IS NULL OR tmdb_status NOT IN ('Ended','Canceled'))
         ORDER BY tmdb_id")?;
    let rows = st.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub type LibraryStates = (HashMap<i64, String>, HashMap<i64, String>);

/// Every library id with its user state, shows then movies.
pub fn library_ids(conn: &Connection) -> Result<LibraryStates> {
    let mut shows = HashMap::new();
    let mut st = conn.prepare("SELECT tmdb_id, my_state FROM shows")?;
    for row in st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
        let (id, state) = row?;
        shows.insert(id, state);
    }
    let mut movies = HashMap::new();
    let mut st = conn.prepare("SELECT tmdb_id, my_state FROM movies")?;
    for row in st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
        let (id, state) = row?;
        movies.insert(id, state);
    }
    Ok((shows, movies))
}

// ---------- season aggregates (redesign) ----------

#[derive(Debug, Clone)]
pub struct SeasonAgg {
    pub show_tmdb_id: i64,
    pub season_number: i64,
    pub total: i64,
    pub aired: i64,
    pub watched: i64,
    pub watched_aired: i64,
    pub dateless: i64,
    pub max_air: Option<String>,
    pub next_air: Option<String>,
    pub runtime_left_min: i64,
}

pub fn season_aggregates(conn: &Connection, today: &str) -> Result<Vec<SeasonAgg>> {
    let mut st = conn.prepare(
        "SELECT e.show_tmdb_id, e.season_number,
                COUNT(*),
                SUM(CASE WHEN e.air_date IS NOT NULL AND e.air_date<=?1 THEN 1 ELSE 0 END),
                SUM(CASE WHEN w.episode_tmdb_id IS NOT NULL THEN 1 ELSE 0 END),
                SUM(CASE WHEN w.episode_tmdb_id IS NOT NULL AND e.air_date IS NOT NULL AND e.air_date<=?1 THEN 1 ELSE 0 END),
                SUM(CASE WHEN e.air_date IS NULL THEN 1 ELSE 0 END),
                MAX(e.air_date),
                MIN(CASE WHEN e.air_date>?1 THEN e.air_date END),
                SUM(CASE WHEN e.air_date IS NOT NULL AND e.air_date<=?1 AND w.episode_tmdb_id IS NULL THEN COALESCE(e.runtime,0) ELSE 0 END)
         FROM episodes e
         LEFT JOIN episode_watches w ON w.episode_tmdb_id=e.tmdb_id
         WHERE e.season_number>0
         GROUP BY e.show_tmdb_id, e.season_number
         ORDER BY e.show_tmdb_id, e.season_number")?;
    let rows = st.query_map([today], |r| {
        Ok(SeasonAgg {
            show_tmdb_id: r.get(0)?, season_number: r.get(1)?, total: r.get(2)?,
            aired: r.get(3)?, watched: r.get(4)?, watched_aired: r.get(5)?,
            dateless: r.get(6)?, max_air: r.get(7)?, next_air: r.get(8)?,
            runtime_left_min: r.get(9)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

#[derive(Debug, Clone)]
pub struct ShowBasics {
    pub tmdb_id: i64,
    pub name: String,
    pub poster_path: Option<String>,
    pub my_state: String,
    pub is_favorite: bool,
    pub is_anime: bool,
}

pub fn show_basics(conn: &Connection) -> Result<Vec<ShowBasics>> {
    let mut st = conn.prepare(
        "SELECT tmdb_id, name, poster_path, my_state, is_favorite, is_anime FROM shows ORDER BY name")?;
    let rows = st.query_map([], |r| {
        Ok(ShowBasics {
            tmdb_id: r.get(0)?, name: r.get(1)?, poster_path: r.get(2)?,
            my_state: r.get(3)?, is_favorite: r.get::<_, i64>(4)? != 0,
            is_anime: r.get::<_, i64>(5)? != 0,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn set_show_anime(conn: &Connection, id: i64, is_anime: bool) -> Result<()> {
    conn.execute("UPDATE shows SET is_anime=?2 WHERE tmdb_id=?1", params![id, is_anime as i64])?;
    Ok(())
}

pub fn last_activity_map(conn: &Connection) -> Result<HashMap<i64, String>> {
    let mut st = conn.prepare(
        "SELECT e.show_tmdb_id, MAX(w.watched_at) FROM episode_watches w
         JOIN episodes e ON e.tmdb_id=w.episode_tmdb_id
         WHERE w.watched_at IS NOT NULL
         GROUP BY e.show_tmdb_id")?;
    let mut map = HashMap::new();
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?)))?;
    for row in rows {
        let (id, at) = row?;
        if let Some(at) = at {
            map.insert(id, at);
        }
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::tmdb::{EpisodeMeta, MovieMeta, ShowMeta};
    use chrono::NaiveDate;

    const NOW: &str = "2026-07-02T10:00:00Z";
    const TODAY: &str = "2026-07-02";

    fn show_meta(id: i64, name: &str) -> ShowMeta {
        ShowMeta {
            tmdb_id: id, name: name.into(), year: Some(2020),
            poster_path: Some("/p.jpg".into()), backdrop_path: None,
            overview: Some("o".into()), status: Some("Returning Series".into()),
            season_numbers: vec![1], is_anime: false,
        }
    }

    fn ep(id: i64, season: i64, num: i64, air: &str) -> EpisodeMeta {
        EpisodeMeta {
            tmdb_id: id, season_number: season, episode_number: num,
            name: Some(format!("E{num}")),
            air_date: NaiveDate::parse_from_str(air, "%Y-%m-%d").ok(),
            runtime: Some(42), still_path: None, overview: None,
        }
    }

    fn movie_meta(id: i64, title: &str) -> MovieMeta {
        MovieMeta {
            tmdb_id: id, imdb_id: Some("tt1".into()), title: title.into(),
            year: Some(2017), release_date: Some("2017-05-01".into()), runtime: Some(120),
            poster_path: None, backdrop_path: None, overview: None,
        }
    }

    fn seed(conn: &rusqlite::Connection) {
        upsert_show_meta(conn, &show_meta(1, "Alpha"), NOW).unwrap();
        set_show_user(conn, 1, "watching", false, Some(999), None).unwrap();
        upsert_episodes(conn, 1, &[
            ep(10, 1, 1, "2026-06-01"),
            ep(11, 1, 2, "2026-06-08"),
            ep(12, 1, 3, "2026-07-10"), // future
            ep(13, 0, 1, "2026-06-01"), // special
            ep(14, 0, 2, "2026-07-05"), // future special
        ]).unwrap();
    }

    #[test]
    fn settings_roundtrip() {
        let conn = db::open_memory().unwrap();
        assert_eq!(setting(&conn, "k").unwrap(), None);
        set_setting(&conn, "k", "v1").unwrap();
        set_setting(&conn, "k", "v2").unwrap();
        assert_eq!(setting(&conn, "k").unwrap(), Some("v2".into()));
    }

    #[test]
    fn upsert_show_preserves_user_state_and_watches() {
        let conn = db::open_memory().unwrap();
        seed(&conn);
        set_show_state(&conn, 1, "stopped").unwrap();
        set_episode_watched(&conn, 10, true, NOW).unwrap();
        // metadata refresh must not clobber state or cascade-delete watches
        upsert_show_meta(&conn, &show_meta(1, "Alpha Renamed"), NOW).unwrap();
        upsert_episodes(&conn, 1, &[ep(10, 1, 1, "2026-06-01")]).unwrap();
        let card = show_card(&conn, 1, TODAY).unwrap();
        assert_eq!(card.name, "Alpha Renamed");
        assert_eq!(card.my_state, "stopped");
        assert_eq!(card.watched, 1);
    }

    #[test]
    fn card_progress_next_and_upcoming() {
        let conn = db::open_memory().unwrap();
        seed(&conn);
        set_episode_watched(&conn, 10, true, NOW).unwrap();
        let card = show_card(&conn, 1, TODAY).unwrap();
        assert_eq!(card.watched, 1);
        assert_eq!(card.total_aired, 2); // E3 future, special excluded
        let next = card.next.unwrap();
        assert_eq!(next.episode_id, 11);
        assert_eq!(card.next_air_date.as_deref(), Some("2026-07-10"));
        assert_eq!(card.last_activity.as_deref(), Some(NOW));

        let up = list_upcoming(&conn, TODAY).unwrap();
        assert_eq!(up.len(), 2);
        assert_eq!(up[0].episode_id, 14); // special airs 2026-07-05, first
        assert_eq!(up[1].episode_id, 12); // regular airs 2026-07-10
        assert_eq!(up[0].show_name, "Alpha");

        // caught up: mark E2 too -> no next
        set_episode_watched(&conn, 11, true, NOW).unwrap();
        let card = show_card(&conn, 1, TODAY).unwrap();
        assert!(card.next.is_none());
    }

    #[test]
    fn import_marks_are_ignored_if_already_watched() {
        let conn = db::open_memory().unwrap();
        seed(&conn);
        set_episode_watched(&conn, 10, true, NOW).unwrap();
        mark_episodes_import(&conn, &[10, 11]).unwrap();
        // 10 keeps its app watch date; 11 is import (NULL date)
        let d = show_detail(&conn, 1).unwrap();
        let s1 = &d.seasons.iter().find(|s| s.season_number == 1).unwrap().episodes;
        assert_eq!(s1[0].watched_at.as_deref(), Some(NOW));
        assert!(s1[1].watched && s1[1].watched_at.is_none());
    }

    #[test]
    fn season_and_all_marks_only_aired() {
        let conn = db::open_memory().unwrap();
        seed(&conn);
        mark_season(&conn, 1, 1, true, TODAY, NOW).unwrap();
        assert_eq!(watched_regular_count(&conn, 1).unwrap(), 2); // not the future E3
        mark_season(&conn, 1, 1, false, TODAY, NOW).unwrap();
        assert_eq!(watched_regular_count(&conn, 1).unwrap(), 0);
        mark_all_aired(&conn, 1, TODAY, NOW).unwrap();
        // regular aired (2) + aired special (1)
        let d = show_detail(&conn, 1).unwrap();
        let watched: usize = d.seasons.iter().flat_map(|s| &s.episodes).filter(|e| e.watched).count();
        assert_eq!(watched, 3);
    }

    #[test]
    fn mark_seasons_before_targets_only_prior_unwatched_aired() {
        let conn = db::open_memory().unwrap();
        seed(&conn); // show 1: S1 eps 10,11 aired / 12 future / specials 13,14
        // add an aired S2 episode so "before season 2" targets only S1
        upsert_episodes(&conn, 1, &[ep(20, 2, 1, "2026-06-15")]).unwrap();
        set_episode_watched(&conn, 10, true, NOW).unwrap(); // S1E1 already watched
        let ids = mark_seasons_before(&conn, 1, 2, TODAY, NOW).unwrap();
        assert_eq!(ids, vec![11]); // only unwatched aired S1 ep; not future 12, specials, or S2
        assert_eq!(watched_regular_count(&conn, 1).unwrap(), 2);
        unmark_episodes(&conn, &ids).unwrap();
        assert_eq!(watched_regular_count(&conn, 1).unwrap(), 1); // prior watch survives
    }

    #[test]
    fn mark_episodes_before_targets_only_prior_unwatched_aired_in_season() {
        let conn = db::open_memory().unwrap();
        seed(&conn); // S1: E1 aired, E2 aired, E3 future; specials E1 aired
        upsert_episodes(&conn, 1, &[ep(15, 1, 4, "2026-06-20")]).unwrap(); // aired E4
        set_episode_watched(&conn, 11, true, NOW).unwrap(); // E2 already watched
        let ids = mark_episodes_before(&conn, 1, 1, 4, TODAY, NOW).unwrap();
        assert_eq!(ids, vec![10]); // E1 only: E2 watched, E3 future, E4 not before, specials other season
        assert_eq!(watched_regular_count(&conn, 1).unwrap(), 2);
        unmark_episodes(&conn, &ids).unwrap();
        assert_eq!(watched_regular_count(&conn, 1).unwrap(), 1); // E2's prior watch survives
    }

    #[test]
    fn season_collect_marks_only_new_and_undo_spares_prior_watches() {
        let conn = db::open_memory().unwrap();
        seed(&conn);
        // pre-existing watch (e.g. from import): ep 10
        set_episode_watched(&conn, 10, true, "2026-06-20T00:00:00Z").unwrap();
        let ids = mark_season_collect(&conn, 1, 1, TODAY, NOW).unwrap();
        assert_eq!(ids, vec![11]); // only the newly-marked aired ep; 10 already watched, 12 future
        assert_eq!(watched_regular_count(&conn, 1).unwrap(), 2);
        unmark_episodes(&conn, &ids).unwrap();
        assert_eq!(watched_regular_count(&conn, 1).unwrap(), 1); // ep 10's prior watch survives
    }

    #[test]
    fn show_detail_groups_seasons() {
        let conn = db::open_memory().unwrap();
        seed(&conn);
        let d = show_detail(&conn, 1).unwrap();
        assert_eq!(d.show.name, "Alpha");
        assert_eq!(d.seasons.len(), 2); // season 0 and 1
        assert_eq!(d.seasons[0].season_number, 0);
        assert_eq!(d.seasons[1].episodes.len(), 3);
    }

    #[test]
    fn movie_lifecycle() {
        let conn = db::open_memory().unwrap();
        upsert_movie_meta(&conn, &movie_meta(50, "Film"), NOW).unwrap();
        set_movie_user(&conn, 50, "watched", Some("2024-06-09T15:05:22Z"), 2, true, Some(7), Some("uuid-1")).unwrap();
        let rows = list_movies(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].my_state, "watched");
        assert_eq!(rows[0].rewatch_count, 2);
        assert!(rows[0].is_favorite);

        assert_eq!(movie_rewatch(&conn, 50).unwrap(), 3);
        set_movie_state(&conn, 50, "watchlist", NOW).unwrap();
        let rows = list_movies(&conn).unwrap();
        assert_eq!(rows[0].my_state, "watchlist");
        assert!(rows[0].watched_at.is_none());
        // back to watched stamps now
        set_movie_state(&conn, 50, "watched", NOW).unwrap();
        assert_eq!(list_movies(&conn).unwrap()[0].watched_at.as_deref(), Some(NOW));
        assert!(!toggle_movie_favorite(&conn, 50).unwrap());
    }

    #[test]
    fn refresh_targets_and_library_ids() {
        let conn = db::open_memory().unwrap();
        seed(&conn); // status Returning Series, watching
        upsert_show_meta(&conn, &ShowMeta { status: Some("Ended".into()), ..show_meta(2, "Done") }, NOW).unwrap();
        set_show_user(&conn, 2, "watching", false, None, None).unwrap();
        upsert_show_meta(&conn, &show_meta(3, "Dropped"), NOW).unwrap();
        set_show_user(&conn, 3, "stopped", false, None, None).unwrap();
        assert_eq!(shows_to_refresh(&conn).unwrap(), vec![1]);

        upsert_movie_meta(&conn, &movie_meta(50, "Film"), NOW).unwrap();
        let (sids, mids) = library_ids(&conn).unwrap();
        assert!(sids.contains_key(&1) && sids.contains_key(&2) && sids.contains_key(&3));
        assert_eq!(mids.get(&50).map(String::as_str), Some("watchlist"));
    }
}
