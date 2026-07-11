use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use chrono::NaiveDate;
use rusqlite::Connection;

use crate::error::{AppError, Result};
use crate::export::{ExportMovie, ExportShow, SeasonEp, TvTimeExport};
use crate::models::*;
use crate::queries;
use crate::tmdb::{EpisodeMeta, Tmdb};

/// TV Time state -> ShowTrack my_state.
pub fn map_status(tvtime_status: &str) -> &'static str {
    match tvtime_status {
        "stopped" => "stopped",
        "not_started_yet" => "plan_to_watch",
        _ => "watching", // up_to_date, continuing, unknown
    }
}

/// Which episode tmdb_ids should be marked watched on import:
/// regular seasons only (season > 0), aired on/before the export date,
/// and not listed in the export's unwatched list.
pub fn episodes_to_mark(
    episodes: &[EpisodeMeta],
    unwatched: &[SeasonEp],
    cutoff: NaiveDate,
) -> Vec<i64> {
    let skip: HashSet<(i64, i64)> = unwatched.iter().map(|u| (u.season, u.episode)).collect();
    episodes.iter()
        .filter(|e| e.season_number > 0)
        .filter(|e| e.air_date.map(|d| d <= cutoff).unwrap_or(false))
        .filter(|e| !skip.contains(&(e.season_number, e.episode_number)))
        .map(|e| e.tmdb_id)
        .collect()
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Run a DB closure on a blocking thread (never blocks the async runtime).
pub async fn with_db<R: Send + 'static>(
    db: &Arc<Mutex<Connection>>,
    f: impl FnOnce(&Connection) -> Result<R> + Send + 'static,
) -> Result<R> {
    let db = db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db.lock().map_err(|_| AppError::msg("db lock poisoned"))?;
        f(&conn)
    })
    .await
    .map_err(|e| AppError::msg(format!("join: {e}")))?
}

async fn import_one_show<T: Tmdb + ?Sized>(
    tmdb: &T,
    db: &Arc<Mutex<Connection>>,
    s: &ExportShow,
    cutoff: chrono::NaiveDate,
) -> Result<Option<ImportAnomaly>> {
    let tmdb_id = tmdb.find_show(s.tvdb_id).await?
        .ok_or_else(|| AppError::msg(format!("TVDB id {} not found on TMDB", s.tvdb_id)))?;
    let full = tmdb.show_with_episodes(tmdb_id).await?;
    // watched == 0 means the user never started this show; the unwatched list can't
    // be trusted to exclude everything when TVDB/TMDB numbering drifts (anime), so
    // don't mark anything at all.
    let mark = if s.watched == 0 {
        Vec::new()
    } else {
        episodes_to_mark(&full.episodes, &s.unwatched_regular_eps, cutoff)
    };
    let actual = mark.len() as i64;
    let expected = s.watched;
    let state = map_status(&s.status).to_string();
    let (favorite, tvdb_id, imdb_id) = (s.is_favorite, s.tvdb_id, s.imdb_id.clone());
    let title = s.title.clone();
    with_db(db, move |conn| {
        let now = now_rfc3339();
        let existed = queries::show_exists(conn, full.meta.tmdb_id)?;
        queries::upsert_show_meta(conn, &full.meta, &now)?;
        if !existed {
            queries::set_show_user(conn, full.meta.tmdb_id, &state, favorite, Some(tvdb_id), imdb_id.as_deref())?;
            crate::oplog::log_show_add(conn, full.meta.tmdb_id, &state, &now)?;
        }
        queries::upsert_episodes(conn, full.meta.tmdb_id, &full.episodes)?;
        crate::oplog::import_watches(conn, &mark, &now)?;
        Ok(())
    })
    .await?;
    Ok((actual != expected).then_some(ImportAnomaly { title, tmdb_id, expected, actual }))
}

async fn import_one_movie<T: Tmdb + ?Sized>(
    tmdb: &T,
    db: &Arc<Mutex<Connection>>,
    m: &ExportMovie,
) -> Result<()> {
    let imdb = m.imdb_id.as_deref()
        .ok_or_else(|| AppError::msg("movie has no IMDb id in export"))?;
    let tmdb_id = tmdb.find_movie(imdb).await?
        .ok_or_else(|| AppError::msg(format!("IMDb id {imdb} not found on TMDB")))?;
    let meta = tmdb.movie(tmdb_id).await?;
    let state = if m.is_watched { "watched" } else { "watchlist" };
    let (watched_at, rewatch, favorite) = (m.watched_at.clone(), m.rewatch_count, m.is_favorite);
    let (tvdb_id, uuid) = (m.tvdb_id, m.uuid.clone());
    with_db(db, move |conn| {
        let now = now_rfc3339();
        let existed = queries::movie_exists(conn, meta.tmdb_id)?;
        queries::upsert_movie_meta(conn, &meta, &now)?;
        if !existed {
            queries::set_movie_user(conn, meta.tmdb_id, state, watched_at.as_deref(),
                                    rewatch, favorite, Some(tvdb_id), Some(&uuid))?;
            crate::oplog::log_movie_add(conn, meta.tmdb_id, state, watched_at.as_deref(), &now)?;
        }
        Ok(())
    })
    .await
}

/// Serial import; per-item failures land in the report, never abort the run.
pub async fn run_import<T: Tmdb + ?Sized>(
    tmdb: &T,
    db: Arc<Mutex<Connection>>,
    export: TvTimeExport,
    mut progress: impl FnMut(ImportProgress),
) -> ImportReport {
    let mut report = ImportReport::default();
    let total = export.shows.len();
    for (i, s) in export.shows.iter().enumerate() {
        progress(ImportProgress {
            stage: "shows".into(), done: i, total, label: s.title.clone(),
        });
        match import_one_show(tmdb, &db, s, export.export_date).await {
            Ok(anomaly) => {
                report.shows_ok += 1;
                report.anomalies.extend(anomaly);
                if s.watched_specials > 0 {
                    report.specials_note.push(SpecialsNote {
                        title: s.title.clone(),
                        watched_specials: s.watched_specials,
                    });
                }
            }
            Err(e) => report.failures.push(ImportFailure {
                kind: "show".into(), title: s.title.clone(), reason: e.to_string(),
            }),
        }
    }
    let total = export.movies.len();
    for (i, m) in export.movies.iter().enumerate() {
        progress(ImportProgress {
            stage: "movies".into(), done: i, total, label: m.title.clone(),
        });
        match import_one_movie(tmdb, &db, m).await {
            Ok(()) => report.movies_ok += 1,
            Err(e) => report.failures.push(ImportFailure {
                kind: "movie".into(), title: m.title.clone(), reason: e.to_string(),
            }),
        }
    }
    report.finished_at = now_rfc3339();
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(id: i64, season: i64, num: i64, air: Option<&str>) -> EpisodeMeta {
        EpisodeMeta {
            tmdb_id: id,
            season_number: season,
            episode_number: num,
            name: None,
            air_date: air.map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap()),
            runtime: None,
            still_path: None,
            overview: None,
        }
    }

    fn cutoff() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()
    }

    #[test]
    fn marks_aired_regular_episodes() {
        let eps = vec![ep(1, 1, 1, Some("2020-01-01")), ep(2, 1, 2, Some("2020-01-08"))];
        assert_eq!(episodes_to_mark(&eps, &[], cutoff()), vec![1, 2]);
    }

    #[test]
    fn skips_specials_unaired_future_and_unwatched() {
        let eps = vec![
            ep(1, 0, 1, Some("2020-01-01")),  // special -> skip
            ep(2, 1, 1, Some("2020-01-01")),  // keep
            ep(3, 1, 2, None),                // no air date -> skip
            ep(4, 1, 3, Some("2026-07-02")),  // after cutoff -> skip
            ep(5, 1, 4, Some("2026-07-01")),  // on cutoff -> keep
            ep(6, 7, 1, Some("2025-04-10")),  // in unwatched list -> skip
        ];
        let unwatched = vec![SeasonEp { season: 7, episode: 1 }];
        assert_eq!(episodes_to_mark(&eps, &unwatched, cutoff()), vec![2, 5]);
    }

    #[test]
    fn maps_tvtime_status() {
        assert_eq!(map_status("up_to_date"), "watching");
        assert_eq!(map_status("continuing"), "watching");
        assert_eq!(map_status("stopped"), "stopped");
        assert_eq!(map_status("not_started_yet"), "plan_to_watch");
        assert_eq!(map_status("unknown"), "watching");
    }

    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    use crate::db;
    use crate::error::{AppError, Result as AResult};
    use crate::export::{ExportMovie, ExportShow, TvTimeExport};
    use crate::queries;
    use crate::tmdb::{MovieMeta, SearchResultItem, ShowFull, ShowMeta, Tmdb};

    struct FakeTmdb {
        tv_by_tvdb: HashMap<i64, i64>,
        movie_by_imdb: HashMap<String, i64>,
        shows: HashMap<i64, ShowFull>,
        movies: HashMap<i64, MovieMeta>,
    }

    #[async_trait]
    impl Tmdb for FakeTmdb {
        async fn find_show(&self, tvdb_id: i64) -> AResult<Option<i64>> {
            Ok(self.tv_by_tvdb.get(&tvdb_id).copied())
        }
        async fn find_movie(&self, imdb_id: &str) -> AResult<Option<i64>> {
            Ok(self.movie_by_imdb.get(imdb_id).copied())
        }
        async fn show_with_episodes(&self, tmdb_id: i64) -> AResult<ShowFull> {
            self.shows.get(&tmdb_id).cloned().ok_or_else(|| AppError::msg("fake: no show"))
        }
        async fn movie(&self, tmdb_id: i64) -> AResult<MovieMeta> {
            self.movies.get(&tmdb_id).cloned().ok_or_else(|| AppError::msg("fake: no movie"))
        }
        async fn search(&self, _q: &str) -> AResult<Vec<SearchResultItem>> {
            Ok(Vec::new())
        }
        async fn validate(&self) -> AResult<bool> {
            Ok(true)
        }
    }

    fn fake() -> FakeTmdb {
        let meta = |id: i64, name: &str| ShowMeta {
            tmdb_id: id, name: name.into(), year: Some(2020), poster_path: None,
            backdrop_path: None, overview: None, status: Some("Ended".into()),
            season_numbers: vec![1], is_anime: false,
        };
        let mut shows = HashMap::new();
        // show 100: 3 aired episodes; export says watched=2 with S1E3 unwatched -> exact match
        shows.insert(100, ShowFull {
            meta: meta(100, "Exact"),
            episodes: vec![
                ep(1000, 1, 1, Some("2020-01-01")),
                ep(1001, 1, 2, Some("2020-01-08")),
                ep(1002, 1, 3, Some("2020-01-15")),
            ],
        });
        // show 200: 2 aired episodes; export claims watched=5 -> anomaly
        shows.insert(200, ShowFull {
            meta: meta(200, "Drifted"),
            episodes: vec![ep(2000, 1, 1, Some("2020-01-01")), ep(2001, 1, 2, Some("2020-01-08"))],
        });
        let movie = |id: i64, title: &str| MovieMeta {
            tmdb_id: id, imdb_id: None, title: title.into(), year: Some(2017),
            release_date: Some("2017-05-01".into()), runtime: Some(120),
            poster_path: None, backdrop_path: None, overview: None,
        };
        FakeTmdb {
            tv_by_tvdb: HashMap::from([(81189, 100), (99999, 200)]),
            movie_by_imdb: HashMap::from([("tt1".into(), 500), ("tt2".into(), 501)]),
            shows,
            movies: HashMap::from([(500, movie(500, "Seen")), (501, movie(501, "Unseen"))]),
        }
    }

    fn export_show(title: &str, tvdb: i64, watched: i64, total: i64,
                   unwatched: Vec<SeasonEp>, specials: i64) -> ExportShow {
        ExportShow {
            title: title.into(), tvdb_id: tvdb, imdb_id: None, status: "up_to_date".into(),
            is_favorite: false, watched, total, total_specials: 0,
            watched_specials: specials, unwatched_regular_eps: unwatched, watched_no_date: 0,
        }
    }

    fn export_movie(title: &str, imdb: &str, watched: bool) -> ExportMovie {
        ExportMovie {
            title: title.into(), tvdb_id: 1, imdb_id: Some(imdb.into()),
            uuid: "u".into(), year: Some(2017), is_watched: watched, is_favorite: watched,
            watched_at: watched.then(|| "2024-06-09T15:05:22Z".to_string()),
            rewatch_count: if watched { 1 } else { 0 },
        }
    }

    fn test_export() -> TvTimeExport {
        TvTimeExport {
            export_date: cutoff(),
            shows: vec![
                export_show("Exact", 81189, 2, 3, vec![SeasonEp { season: 1, episode: 3 }], 2),
                export_show("Drifted", 99999, 5, 5, vec![], 0),
                export_show("Missing", 11111, 1, 1, vec![], 0), // no TMDB match -> failure
            ],
            movies: vec![
                export_movie("Seen", "tt1", true),
                export_movie("Unseen", "tt2", false),
                export_movie("Ghost", "tt404", true), // no TMDB match -> failure
            ],
        }
    }

    /// Re:ZERO regression: a never-watched show (watched=0) whose TVDB episode
    /// numbering doesn't match TMDB's must import with ZERO watches, even though
    /// the aired episodes don't appear in the (differently numbered) unwatched list.
    #[tokio::test]
    async fn never_watched_show_marks_nothing_despite_numbering_drift() {
        let db = Arc::new(Mutex::new(db::open_memory().unwrap()));
        let mut tmdb = fake();
        tmdb.tv_by_tvdb.insert(55555, 300);
        tmdb.shows.insert(300, ShowFull {
            meta: ShowMeta {
                tmdb_id: 300, name: "NeverStarted".into(), year: Some(2016), poster_path: None,
                backdrop_path: None, overview: None, status: Some("Ended".into()),
                season_numbers: vec![1], is_anime: false,
            },
            episodes: vec![ep(3000, 1, 1, Some("2016-04-04")), ep(3001, 1, 2, Some("2016-04-11"))],
        });
        let export = TvTimeExport {
            export_date: cutoff(),
            shows: vec![ExportShow {
                status: "not_started_yet".into(),
                // TVDB numbers the same episodes as season 90 -> no (season, episode) overlap
                ..export_show("NeverStarted", 55555, 0, 2,
                    vec![SeasonEp { season: 90, episode: 1 }, SeasonEp { season: 90, episode: 2 }], 0)
            }],
            movies: vec![],
        };
        let report = run_import(&tmdb, db.clone(), export, |_| {}).await;
        assert_eq!(report.shows_ok, 1);
        assert!(report.anomalies.is_empty(), "0 expected == 0 marked, no anomaly");
        let conn = db.lock().unwrap();
        assert_eq!(queries::watched_regular_count(&conn, 300).unwrap(), 0);
        let d = queries::show_detail(&conn, 300).unwrap();
        assert_eq!(d.show.my_state, "plan_to_watch");
    }

    #[tokio::test]
    async fn import_end_to_end() {
        let db = Arc::new(Mutex::new(db::open_memory().unwrap()));
        let tmdb = fake();
        let mut events = 0usize;
        let report = run_import(&tmdb, db.clone(), test_export(), |_p| events += 1).await;

        assert_eq!(report.shows_ok, 2);
        assert_eq!(report.movies_ok, 2);
        assert_eq!(report.failures.len(), 2);
        assert!(report.failures.iter().any(|f| f.kind == "show" && f.title == "Missing"));
        assert!(report.failures.iter().any(|f| f.kind == "movie" && f.title == "Ghost"));
        assert_eq!(report.anomalies.len(), 1);
        assert_eq!(report.anomalies[0].title, "Drifted");
        assert_eq!(report.anomalies[0].expected, 5);
        assert_eq!(report.anomalies[0].actual, 2);
        assert_eq!(report.specials_note.len(), 1);
        assert_eq!(report.specials_note[0].title, "Exact");
        assert!(events >= 6); // one per item minimum

        let conn = db.lock().unwrap();
        // Exact: S1E1+S1E2 watched (import, no date), S1E3 not
        assert_eq!(queries::watched_regular_count(&conn, 100).unwrap(), 2);
        let d = queries::show_detail(&conn, 100).unwrap();
        assert!(d.seasons[0].episodes[0].watched && d.seasons[0].episodes[0].watched_at.is_none());
        assert_eq!(d.show.my_state, "watching");
        // movies
        let movies = queries::list_movies(&conn).unwrap();
        assert_eq!(movies.len(), 2);
        let seen = movies.iter().find(|m| m.title == "Seen").unwrap();
        assert_eq!(seen.my_state, "watched");
        assert_eq!(seen.watched_at.as_deref(), Some("2024-06-09T15:05:22Z"));
        assert_eq!(seen.rewatch_count, 1);
        assert!(seen.is_favorite);
        assert_eq!(movies.iter().find(|m| m.title == "Unseen").unwrap().my_state, "watchlist");
    }

    #[tokio::test]
    async fn import_is_idempotent_and_preserves_app_watches() {
        let db = Arc::new(Mutex::new(db::open_memory().unwrap()));
        let tmdb = fake();
        let _ = run_import(&tmdb, db.clone(), test_export(), |_| {}).await;
        {
            let conn = db.lock().unwrap();
            // user marks S1E3 in-app after first import
            queries::set_episode_watched(&conn, 1002, true, "2026-07-02T10:00:00Z").unwrap();
        }
        let report2 = run_import(&tmdb, db.clone(), test_export(), |_| {}).await;
        assert_eq!(report2.shows_ok, 2);
        let conn = db.lock().unwrap();
        assert_eq!(queries::watched_regular_count(&conn, 100).unwrap(), 3);
        let d = queries::show_detail(&conn, 100).unwrap();
        // the app-sourced watch kept its date
        assert_eq!(d.seasons[0].episodes[2].watched_at.as_deref(), Some("2026-07-02T10:00:00Z"));
        // no duplicate movies
        assert_eq!(queries::list_movies(&conn).unwrap().len(), 2);
    }

    #[tokio::test]
    async fn reimport_appends_no_duplicate_ops() {
        let db = Arc::new(Mutex::new(db::open_memory().unwrap()));
        let tmdb = fake();
        let _ = run_import(&tmdb, db.clone(), test_export(), |_| {}).await;
        let after_first = {
            let conn = db.lock().unwrap();
            crate::oplog::latest_seq(&conn).unwrap()
        };
        assert!(after_first > 0);
        let _ = run_import(&tmdb, db.clone(), test_export(), |_| {}).await;
        let conn = db.lock().unwrap();
        assert_eq!(crate::oplog::latest_seq(&conn).unwrap(), after_first);
    }

    #[tokio::test]
    async fn reimport_preserves_in_app_user_state() {
        let db = Arc::new(Mutex::new(db::open_memory().unwrap()));
        let tmdb = fake();
        let _ = run_import(&tmdb, db.clone(), test_export(), |_| {}).await;
        {
            let conn = db.lock().unwrap();
            // user activity after first import:
            queries::set_movie_state(&conn, 501, "watched", "2026-07-03T09:00:00Z").unwrap(); // watchlist -> watched
            queries::set_show_state(&conn, 100, "stopped").unwrap();
            queries::toggle_show_favorite(&conn, 100).unwrap();
        }
        let report = run_import(&tmdb, db.clone(), test_export(), |_| {}).await;
        assert_eq!(report.shows_ok, 2);
        let conn = db.lock().unwrap();
        let movies = queries::list_movies(&conn).unwrap();
        let unseen = movies.iter().find(|m| m.title == "Unseen").unwrap();
        assert_eq!(unseen.my_state, "watched"); // export says watchlist — must NOT revert
        assert_eq!(unseen.watched_at.as_deref(), Some("2026-07-03T09:00:00Z"));
        let d = queries::show_detail(&conn, 100).unwrap();
        assert_eq!(d.show.my_state, "stopped"); // export says up_to_date -> watching — must NOT revert
        assert!(d.show.is_favorite);
    }
}
