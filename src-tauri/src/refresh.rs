use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::import::with_db;
use crate::models::RefreshSummary;
use crate::queries;
use crate::tmdb::Tmdb;

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Re-fetch metadata + episodes for every watching, still-airing show.
pub async fn run_refresh<T: Tmdb + ?Sized>(
    tmdb: &T,
    db: Arc<Mutex<Connection>>,
) -> RefreshSummary {
    let ids = match with_db(&db, |conn| queries::shows_to_refresh(conn)).await {
        Ok(ids) => ids,
        Err(e) => return RefreshSummary { refreshed: 0, failed: vec![e.to_string()] },
    };
    let mut summary = RefreshSummary { refreshed: 0, failed: Vec::new() };
    for id in ids {
        match tmdb.show_with_episodes(id).await {
            Ok(full) => {
                let res = with_db(&db, move |conn| {
                    let now = now_rfc3339();
                    queries::upsert_show_meta(conn, &full.meta, &now)?;
                    queries::upsert_episodes(conn, full.meta.tmdb_id, &full.episodes)?;
                    Ok(())
                })
                .await;
                match res {
                    Ok(()) => summary.refreshed += 1,
                    Err(e) => summary.failed.push(format!("show {id}: {e}")),
                }
            }
            Err(e) => summary.failed.push(format!("show {id}: {e}")),
        }
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;

    use crate::db;
    use crate::error::{AppError, Result as AResult};
    use crate::tmdb::{EpisodeMeta, MovieMeta, SearchResultItem, ShowFull, ShowMeta};

    struct OneShow(ShowFull);

    #[async_trait]
    impl Tmdb for OneShow {
        async fn find_show(&self, _t: i64) -> AResult<Option<i64>> { Ok(None) }
        async fn find_movie(&self, _i: &str) -> AResult<Option<i64>> { Ok(None) }
        async fn show_with_episodes(&self, tmdb_id: i64) -> AResult<ShowFull> {
            if tmdb_id == self.0.meta.tmdb_id { Ok(self.0.clone()) }
            else { Err(AppError::msg("boom")) }
        }
        async fn movie(&self, _t: i64) -> AResult<MovieMeta> { Err(AppError::msg("no")) }
        async fn search(&self, _q: &str) -> AResult<Vec<SearchResultItem>> { Ok(Vec::new()) }
        async fn validate(&self) -> AResult<bool> { Ok(true) }
    }

    fn meta(id: i64, status: &str) -> ShowMeta {
        ShowMeta {
            tmdb_id: id, name: format!("S{id}"), year: None, poster_path: None,
            backdrop_path: None, overview: None, status: Some(status.into()),
            season_numbers: vec![1], is_anime: false,
        }
    }

    fn ep(id: i64, num: i64, air: &str) -> EpisodeMeta {
        EpisodeMeta {
            tmdb_id: id, season_number: 1, episode_number: num, name: None,
            air_date: chrono::NaiveDate::parse_from_str(air, "%Y-%m-%d").ok(),
            runtime: None, still_path: None, overview: None,
        }
    }

    #[tokio::test]
    async fn refreshes_airing_adds_new_episode_and_reports_failures() {
        let db = Arc::new(Mutex::new(db::open_memory().unwrap()));
        {
            let conn = db.lock().unwrap();
            // airing show 1 with one episode, watching
            queries::upsert_show_meta(&conn, &meta(1, "Returning Series"), "t").unwrap();
            queries::set_show_user(&conn, 1, "watching", false, None, None).unwrap();
            queries::upsert_episodes(&conn, 1, &[ep(10, 1, "2026-06-01")]).unwrap();
            // airing show 2, watching, but the fake API errors on it
            queries::upsert_show_meta(&conn, &meta(2, "Returning Series"), "t").unwrap();
            queries::set_show_user(&conn, 2, "watching", false, None, None).unwrap();
            // ended show 3 must not be touched
            queries::upsert_show_meta(&conn, &meta(3, "Ended"), "t").unwrap();
            queries::set_show_user(&conn, 3, "watching", false, None, None).unwrap();
        }
        // fake returns show 1 with a NEW episode 11
        let tmdb = OneShow(ShowFull {
            meta: meta(1, "Returning Series"),
            episodes: vec![ep(10, 1, "2026-06-01"), ep(11, 2, "2026-07-20")],
        });
        let summary = run_refresh(&tmdb, db.clone()).await;
        assert_eq!(summary.refreshed, 1);
        assert_eq!(summary.failed.len(), 1);
        assert!(summary.failed[0].starts_with("show 2"));
        let conn = db.lock().unwrap();
        let up = queries::list_upcoming(&conn, "2026-07-02").unwrap();
        assert_eq!(up.len(), 1);
        assert_eq!(up[0].episode_id, 11);
    }
}
