use chrono::NaiveDate;
use serde::Deserialize;

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Deserialize)]
pub struct SeasonEp {
    pub season: i64,
    pub episode: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportShow {
    pub title: String,
    pub tvdb_id: i64,
    #[serde(default)]
    pub imdb_id: Option<String>,
    pub status: String,
    pub is_favorite: bool,
    pub watched: i64,
    pub total: i64,
    #[serde(default)]
    pub total_specials: i64,
    #[serde(default)]
    pub watched_specials: i64,
    #[serde(default)]
    pub unwatched_regular_eps: Vec<SeasonEp>,
    #[serde(default)]
    pub watched_no_date: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportMovie {
    pub title: String,
    pub tvdb_id: i64,
    #[serde(default)]
    pub imdb_id: Option<String>,
    pub uuid: String,
    #[serde(default)]
    pub year: Option<i64>,
    pub is_watched: bool,
    pub is_favorite: bool,
    #[serde(default)]
    pub watched_at: Option<String>,
    #[serde(default)]
    pub rewatch_count: i64,
}

#[derive(Debug, Clone)]
pub struct TvTimeExport {
    pub export_date: NaiveDate,
    pub shows: Vec<ExportShow>,
    pub movies: Vec<ExportMovie>,
}

/// Grab the `[...]` JSON array from a `var NAME = [...];` line.
fn extract_array<'a>(line: &'a str) -> Option<&'a str> {
    let start = line.find('[')?;
    let end = line.rfind(']')?;
    if end <= start {
        return None;
    }
    Some(&line[start..=end])
}

pub fn parse_export(html: &str) -> Result<TvTimeExport> {
    let mut shows_json: Option<&str> = None;
    let mut movies_json: Option<&str> = None;
    for line in html.lines() {
        let t = line.trim_start();
        if t.starts_with("var SHOWS") {
            shows_json = extract_array(line);
        } else if t.starts_with("var MOVIES") {
            movies_json = extract_array(line);
        }
    }
    let shows_json = shows_json.ok_or_else(|| AppError::msg("no `var SHOWS` array found — is this a TV Time Out export?"))?;
    let movies_json = movies_json.ok_or_else(|| AppError::msg("no `var MOVIES` array found — is this a TV Time Out export?"))?;

    let marker = "Export Summary ";
    let pos = html.find(marker).ok_or_else(|| AppError::msg("no export date found"))?;
    let date_str = html.get(pos + marker.len()..pos + marker.len() + 10)
        .ok_or_else(|| AppError::msg("truncated export date"))?;
    let export_date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
        .map_err(|e| AppError::msg(format!("bad export date {date_str:?}: {e}")))?;

    Ok(TvTimeExport {
        export_date,
        shows: serde_json::from_str(shows_json)?,
        movies: serde_json::from_str(movies_json)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/sample_export.html");

    #[test]
    fn parses_fixture() {
        let ex = parse_export(FIXTURE).unwrap();
        assert_eq!(ex.export_date, chrono::NaiveDate::from_ymd_opt(2026, 7, 1).unwrap());
        assert_eq!(ex.shows.len(), 2);
        assert_eq!(ex.movies.len(), 2);

        let s = &ex.shows[1];
        assert_eq!(s.title, "Partially Watched");
        assert_eq!(s.tvdb_id, 253463);
        assert_eq!(s.status, "stopped");
        assert!(s.is_favorite);
        assert_eq!(s.watched, 26);
        assert_eq!(s.total, 32);
        assert_eq!(s.watched_specials, 2);
        assert_eq!(s.unwatched_regular_eps.len(), 2);
        assert_eq!(s.unwatched_regular_eps[0].season, 7);
        assert_eq!(s.unwatched_regular_eps[0].episode, 1);

        let m = &ex.movies[0];
        assert_eq!(m.title, "Watched Movie");
        assert_eq!(m.imdb_id.as_deref(), Some("tt5294550"));
        assert!(m.is_watched);
        assert_eq!(m.watched_at.as_deref(), Some("2024-06-09T15:05:22Z"));
        assert_eq!(m.rewatch_count, 2);
        assert!(!ex.movies[1].is_watched);
        assert_eq!(ex.movies[1].year, None);
    }

    #[test]
    fn rejects_html_without_data() {
        assert!(parse_export("<html><body>nope</body></html>").is_err());
    }

    /// Integration check against a real export. Run manually:
    /// TVTIME_EXPORT=path\to\export.html cargo test real_export -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_export() {
        let path = std::env::var("TVTIME_EXPORT").expect("set TVTIME_EXPORT to the export html path");
        let html = std::fs::read_to_string(path).unwrap();
        let ex = parse_export(&html).unwrap();
        assert_eq!(ex.shows.len(), 171);
        assert_eq!(ex.movies.len(), 594);
        assert!(ex.shows.iter().all(|s| s.tvdb_id > 0));
        assert!(ex.movies.iter().all(|m| m.imdb_id.is_some()));
    }
}
