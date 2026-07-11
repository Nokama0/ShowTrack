use std::collections::HashMap;

use rusqlite::Connection;

use crate::error::Result;
use crate::models::{AiringRow, AiringSeason, BingeCard, ReadySeason};
use crate::queries::{self, SeasonAgg};

pub struct ShowClass {
    pub ready: Option<ReadySeason>,
    pub airing: Option<AiringSeason>,
    pub series_watched_aired: i64,
    pub series_aired: i64,
    pub caught_up: bool,
}

fn is_complete(a: &SeasonAgg) -> bool {
    a.total > 0 && a.aired == a.total
}

/// Fold one show's season aggregates (season>0, ascending) into its binge state.
pub fn classify_show(aggs: &[SeasonAgg]) -> ShowClass {
    let show_watched: i64 = aggs.iter().map(|a| a.watched).sum();
    let series_aired: i64 = aggs.iter().map(|a| a.aired).sum();
    let series_watched_aired: i64 = aggs.iter().map(|a| a.watched_aired).sum();
    let ready = aggs.iter()
        .find(|a| is_complete(a) && a.watched < a.total)
        .map(|a| ReadySeason {
            season_number: a.season_number,
            ep_count: a.total,
            ep_left: a.total - a.watched,
            runtime_left_min: a.runtime_left_min,
            kind: if show_watched == 0 { "start" } else if a.watched > 0 { "finish" } else { "continue" }
                .to_string(),
            prior_unwatched: aggs.iter()
                .filter(|p| p.season_number < a.season_number)
                .map(|p| p.aired - p.watched_aired)
                .sum(),
        });
    let airing = aggs.iter()
        .find(|a| !is_complete(a) && (a.aired > 0 || a.next_air.is_some()))
        .map(|a| AiringSeason {
            season_number: a.season_number,
            aired_count: a.aired,
            total_count: a.total,
            next_air_date: a.next_air.clone(),
            finale_date: if a.dateless == 0 { a.max_air.clone() } else { None },
        });
    let caught_up = ready.is_none() && series_watched_aired >= series_aired;
    ShowClass { ready, airing, series_watched_aired, series_aired, caught_up }
}

pub fn build_binge_cards(conn: &Connection, today: &str) -> Result<Vec<BingeCard>> {
    let mut by_show: HashMap<i64, Vec<SeasonAgg>> = HashMap::new();
    for a in queries::season_aggregates(conn, today)? {
        by_show.entry(a.show_tmdb_id).or_default().push(a);
    }
    let activity = queries::last_activity_map(conn)?;
    let mut cards = Vec::new();
    for b in queries::show_basics(conn)? {
        let class = match by_show.get(&b.tmdb_id) {
            Some(aggs) => classify_show(aggs),
            None => ShowClass {
                ready: None, airing: None,
                series_watched_aired: 0, series_aired: 0, caught_up: true,
            },
        };
        cards.push(BingeCard {
            tmdb_id: b.tmdb_id,
            name: b.name,
            poster_path: b.poster_path,
            my_state: b.my_state,
            is_favorite: b.is_favorite,
            is_anime: b.is_anime,
            series_watched_aired: class.series_watched_aired,
            series_aired: class.series_aired,
            ready: class.ready,
            airing: class.airing,
            caught_up: class.caught_up,
            last_activity: activity.get(&b.tmdb_id).cloned(),
        });
    }
    Ok(cards)
}

/// Airing seasons of shows the user is watching, finale-known first (ascending), then name.
pub fn build_airing_rows(conn: &Connection, today: &str) -> Result<Vec<AiringRow>> {
    let mut rows: Vec<AiringRow> = build_binge_cards(conn, today)?
        .into_iter()
        .filter(|c| c.my_state == "watching")
        .filter_map(|c| {
            c.airing.map(|a| AiringRow {
                show_tmdb_id: c.tmdb_id,
                show_name: c.name,
                poster_path: c.poster_path,
                season_number: a.season_number,
                aired_count: a.aired_count,
                total_count: a.total_count,
                next_air_date: a.next_air_date,
                finale_date: a.finale_date,
            })
        })
        .collect();
    rows.sort_by(|x, y| match (&x.finale_date, &y.finale_date) {
        (Some(a), Some(b)) => a.cmp(b).then_with(|| x.show_name.cmp(&y.show_name)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => x.show_name.cmp(&y.show_name),
    });
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::queries::{self, SeasonAgg};
    use crate::tmdb::{EpisodeMeta, ShowMeta};

    const TODAY: &str = "2026-07-03";
    const NOW: &str = "2026-07-03T10:00:00Z";

    fn agg(season: i64, total: i64, aired: i64, watched: i64, watched_aired: i64,
           dateless: i64, max_air: Option<&str>, next_air: Option<&str>, runtime_left: i64) -> SeasonAgg {
        SeasonAgg {
            show_tmdb_id: 1, season_number: season, total, aired, watched, watched_aired,
            dateless, max_air: max_air.map(str::to_string), next_air: next_air.map(str::to_string),
            runtime_left_min: runtime_left,
        }
    }

    #[test]
    fn ready_continue_picks_lowest_complete_unwatched_season() {
        // S1 fully watched, S2 and S3 complete+unwatched -> ready = S2, kind continue
        let aggs = vec![
            agg(1, 8, 8, 8, 8, 0, Some("2020-05-01"), None, 0),
            agg(2, 10, 10, 0, 0, 0, Some("2021-05-01"), None, 420),
            agg(3, 10, 10, 0, 0, 0, Some("2022-05-01"), None, 430),
        ];
        let c = classify_show(&aggs);
        let r = c.ready.unwrap();
        assert_eq!(r.season_number, 2);
        assert_eq!(r.kind, "continue");
        assert_eq!(r.ep_count, 10);
        assert_eq!(r.ep_left, 10);
        assert_eq!(r.runtime_left_min, 420);
        assert!(c.airing.is_none());
        assert!(!c.caught_up);
        assert_eq!(c.series_aired, 28);
        assert_eq!(c.series_watched_aired, 8);
        assert_eq!(r.prior_unwatched, 0); // S1 fully watched
    }

    #[test]
    fn prior_unwatched_counts_gaps_in_earlier_seasons() {
        // S1: 8 aired, only 5 watched (drift gaps); S2 complete+unwatched -> ready S2
        // wait: S1 has unwatched aired eps AND is complete -> S1 itself is ready ("finish").
        // Use an INCOMPLETE S1 (airing) so S2 is the ready one:
        // S1: 10 total, 8 aired, 5 watched (2 unaired) -> not complete -> airing
        // S2: complete unwatched -> ready, prior_unwatched = 8 - 5 = 3
        let aggs = vec![
            agg(1, 10, 8, 5, 5, 0, Some("2026-08-01"), Some("2026-07-20"), 120),
            agg(2, 6, 6, 0, 0, 0, Some("2021-05-01"), None, 260),
        ];
        let c = classify_show(&aggs);
        let r = c.ready.unwrap();
        assert_eq!(r.season_number, 2);
        assert_eq!(r.prior_unwatched, 3);
    }

    #[test]
    fn ready_start_when_zero_watches_showwide() {
        let aggs = vec![agg(1, 25, 25, 0, 0, 0, Some("2016-09-19"), None, 600)];
        let r = classify_show(&aggs).ready.unwrap();
        assert_eq!(r.kind, "start");
    }

    #[test]
    fn ready_finish_when_season_partially_watched() {
        // drift leftovers: 3 of 5 watched in a fully-aired season
        let aggs = vec![agg(1, 5, 5, 3, 3, 0, Some("2019-03-01"), None, 90)];
        let r = classify_show(&aggs).ready.unwrap();
        assert_eq!(r.kind, "finish");
        assert_eq!(r.ep_left, 2);
    }

    #[test]
    fn airing_with_all_dates_known_has_finale() {
        // 5 of 8 aired, finale date known and in the future
        let aggs = vec![agg(3, 8, 5, 5, 5, 0, Some("2026-08-24"), Some("2026-07-06"), 0)];
        let c = classify_show(&aggs);
        assert!(c.ready.is_none());
        let a = c.airing.unwrap();
        assert_eq!(a.aired_count, 5);
        assert_eq!(a.finale_date.as_deref(), Some("2026-08-24"));
        assert_eq!(a.next_air_date.as_deref(), Some("2026-07-06"));
        assert!(c.caught_up, "all aired episodes watched -> caught up while airing");
    }

    #[test]
    fn airing_with_dateless_episodes_has_tba_finale() {
        let aggs = vec![agg(27, 10, 5, 0, 0, 3, Some("2026-07-30"), Some("2026-07-09"), 220)];
        let a = classify_show(&aggs).airing.unwrap();
        assert_eq!(a.finale_date, None);
    }

    #[test]
    fn premiere_counts_as_airing() {
        // season fully scheduled, nothing aired yet
        let aggs = vec![agg(2, 8, 0, 0, 0, 0, Some("2026-09-02"), Some("2026-07-15"), 0)];
        let a = classify_show(&aggs).airing.unwrap();
        assert_eq!(a.aired_count, 0);
        assert_eq!(a.next_air_date.as_deref(), Some("2026-07-15"));
        assert_eq!(a.finale_date.as_deref(), Some("2026-09-02"));
    }

    #[test]
    fn fully_watched_show_is_caught_up() {
        let aggs = vec![agg(1, 8, 8, 8, 8, 0, Some("2020-05-01"), None, 0)];
        let c = classify_show(&aggs);
        assert!(c.ready.is_none() && c.airing.is_none() && c.caught_up);
    }

    // ── integration: real SQL through the seeded schema ──

    fn seed_show(conn: &rusqlite::Connection, id: i64, name: &str, state: &str) {
        let meta = ShowMeta {
            tmdb_id: id, name: name.into(), year: Some(2020), poster_path: Some("/p.jpg".into()),
            backdrop_path: None, overview: None, status: Some("Returning Series".into()),
            season_numbers: vec![0, 1], is_anime: false,
        };
        queries::upsert_show_meta(conn, &meta, NOW).unwrap();
        queries::set_show_user(conn, id, state, false, None, None).unwrap();
    }

    fn ep(id: i64, season: i64, num: i64, air: Option<&str>, runtime: Option<i64>) -> EpisodeMeta {
        EpisodeMeta {
            tmdb_id: id, season_number: season, episode_number: num, name: Some(format!("E{num}")),
            air_date: air.map(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap()),
            runtime, still_path: None, overview: None,
        }
    }

    #[test]
    fn build_cards_and_airing_rows_from_seeded_db() {
        let conn = db::open_memory().unwrap();

        // show 1 "Binge Me": watching; S1 watched (2 eps), S2 complete unwatched (2 eps, 50m each),
        // and a special that must be ignored
        seed_show(&conn, 1, "Binge Me", "watching");
        queries::upsert_episodes(&conn, 1, &[
            ep(10, 1, 1, Some("2020-01-01"), Some(40)), ep(11, 1, 2, Some("2020-01-08"), Some(40)),
            ep(12, 2, 1, Some("2021-01-01"), Some(50)), ep(13, 2, 2, Some("2021-01-08"), Some(50)),
            ep(14, 0, 1, Some("2020-06-01"), Some(30)), // special: excluded everywhere
        ]).unwrap();
        queries::set_episode_watched(&conn, 10, true, NOW).unwrap();
        queries::set_episode_watched(&conn, 11, true, NOW).unwrap();

        // show 2 "On Air": watching; S1 airing 1/3 aired, all dates known
        seed_show(&conn, 2, "On Air", "watching");
        queries::upsert_episodes(&conn, 2, &[
            ep(20, 1, 1, Some("2026-07-01"), Some(30)),
            ep(21, 1, 2, Some("2026-07-08"), Some(30)),
            ep(22, 1, 3, Some("2026-07-15"), Some(30)),
        ]).unwrap();

        // show 3 "Planned": plan_to_watch, S1 complete -> ready(start), NOT in airing rows
        seed_show(&conn, 3, "Planned", "plan_to_watch");
        queries::upsert_episodes(&conn, 3, &[ep(30, 1, 1, Some("2019-01-01"), Some(45))]).unwrap();

        let cards = crate::binge::build_binge_cards(&conn, TODAY).unwrap();
        assert_eq!(cards.len(), 3);
        let byname = |n: &str| cards.iter().find(|c| c.name == n).unwrap();

        let b = byname("Binge Me");
        let r = b.ready.as_ref().unwrap();
        assert_eq!((r.season_number, r.ep_count, r.ep_left, r.runtime_left_min), (2, 2, 2, 100));
        assert_eq!(r.kind, "continue");
        assert_eq!((b.series_aired, b.series_watched_aired), (4, 2)); // special not counted
        assert_eq!(b.last_activity.as_deref(), Some(NOW));
        assert!(!b.caught_up);

        let a = byname("On Air");
        let ai = a.airing.as_ref().unwrap();
        assert_eq!((ai.aired_count, ai.total_count), (1, 3));
        assert_eq!(ai.next_air_date.as_deref(), Some("2026-07-08"));
        assert_eq!(ai.finale_date.as_deref(), Some("2026-07-15"));
        assert!(a.ready.is_none());
        assert!(!a.caught_up); // 1 aired, unwatched

        let p = byname("Planned");
        assert_eq!(p.ready.as_ref().unwrap().kind, "start");

        let rows = crate::binge::build_airing_rows(&conn, TODAY).unwrap();
        assert_eq!(rows.len(), 1, "plan_to_watch shows excluded from airing rows");
        assert_eq!(rows[0].show_name, "On Air");
        assert_eq!(rows[0].finale_date.as_deref(), Some("2026-07-15"));
    }
}
