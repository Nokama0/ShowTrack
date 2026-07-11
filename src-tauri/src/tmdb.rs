use async_trait::async_trait;
use chrono::NaiveDate;
use serde_json::Value;
use tokio::sync::Semaphore;

use crate::error::{AppError, Result};

const API: &str = "https://api.themoviedb.org/3";

#[derive(Debug, Clone)]
pub struct ShowMeta {
    pub tmdb_id: i64,
    pub name: String,
    pub year: Option<i64>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub overview: Option<String>,
    pub status: Option<String>,
    pub season_numbers: Vec<i64>,
    /// Japanese-language animation per TMDB (original_language ja + genre 16).
    pub is_anime: bool,
}

#[derive(Debug, Clone)]
pub struct EpisodeMeta {
    pub tmdb_id: i64,
    pub season_number: i64,
    pub episode_number: i64,
    pub name: Option<String>,
    pub air_date: Option<NaiveDate>,
    pub runtime: Option<i64>,
    pub still_path: Option<String>,
    pub overview: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MovieMeta {
    pub tmdb_id: i64,
    pub imdb_id: Option<String>,
    pub title: String,
    pub year: Option<i64>,
    pub release_date: Option<String>,
    pub runtime: Option<i64>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub overview: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ShowFull {
    pub meta: ShowMeta,
    pub episodes: Vec<EpisodeMeta>,
}

#[derive(Debug, Clone)]
pub struct SearchResultItem {
    pub tmdb_id: i64,
    pub media_type: String,
    pub title: String,
    pub year: Option<i64>,
    pub poster_path: Option<String>,
    pub overview: Option<String>,
}

#[async_trait]
pub trait Tmdb: Send + Sync {
    async fn find_show(&self, tvdb_id: i64) -> Result<Option<i64>>;
    async fn find_movie(&self, imdb_id: &str) -> Result<Option<i64>>;
    async fn show_with_episodes(&self, tmdb_id: i64) -> Result<ShowFull>;
    async fn movie(&self, tmdb_id: i64) -> Result<MovieMeta>;
    async fn search(&self, query: &str) -> Result<Vec<SearchResultItem>>;
    async fn validate(&self) -> Result<bool>;
}

pub struct TmdbClient {
    http: reqwest::Client,
    key: String,
    sem: Semaphore,
}

impl TmdbClient {
    pub fn new(http: reqwest::Client, key: String) -> Self {
        Self { http, key, sem: Semaphore::new(4) }
    }

    /// Base show record only (no seasons); used by the anime library scan.
    pub async fn tv_base(&self, tmdb_id: i64) -> Result<ShowMeta> {
        parse_tv_base(&self.get_json(&format!("/tv/{tmdb_id}")).await?)
    }

    /// GET an API path (path may already contain a query string). Adds auth,
    /// throttles to 4 in-flight, retries once after 500ms on HTTP 429.
    async fn get_json(&self, path_and_query: &str) -> Result<Value> {
        let _permit = self.sem.acquire().await.map_err(|e| AppError::msg(e.to_string()))?;
        let sep = if path_and_query.contains('?') { '&' } else { '?' };
        // v4 tokens are JWTs (contain '.'), v3 keys are plain hex
        let (url, bearer) = if self.key.contains('.') {
            (format!("{API}{path_and_query}"), true)
        } else {
            (format!("{API}{path_and_query}{sep}api_key={}", self.key), false)
        };
        for attempt in 0..2 {
            let mut req = self.http.get(&url);
            if bearer {
                req = req.bearer_auth(&self.key);
            }
            let resp = req.send().await?;
            if resp.status().as_u16() == 429 && attempt == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }
            if resp.status().as_u16() == 404 {
                return Err(AppError::msg("not found on TMDB (404)"));
            }
            let resp = resp.error_for_status()?;
            return Ok(resp.json().await?);
        }
        unreachable!()
    }
}

fn s(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).map(str::to_string)
}

fn i(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(|x| x.as_i64())
}

fn year_of(date: Option<&str>) -> Option<i64> {
    date.and_then(|d| d.get(..4)).and_then(|y| y.parse().ok())
}

pub fn parse_find(v: &Value, list: &str) -> Option<i64> {
    v.get(list)?.as_array()?.first().and_then(|r| r.get("id")).and_then(|x| x.as_i64())
}

pub fn parse_tv_base(v: &Value) -> Result<ShowMeta> {
    let animation = v.get("genres").and_then(|g| g.as_array())
        .map(|arr| arr.iter().any(|g| i(g, "id") == Some(16)))
        .unwrap_or(false);
    Ok(ShowMeta {
        tmdb_id: i(v, "id").ok_or_else(|| AppError::msg("tv: missing id"))?,
        name: s(v, "name").ok_or_else(|| AppError::msg("tv: missing name"))?,
        year: year_of(s(v, "first_air_date").as_deref()),
        poster_path: s(v, "poster_path"),
        backdrop_path: s(v, "backdrop_path"),
        overview: s(v, "overview"),
        status: s(v, "status"),
        season_numbers: v.get("seasons").and_then(|x| x.as_array()).map(|arr| {
            arr.iter().filter_map(|sn| i(sn, "season_number")).collect()
        }).unwrap_or_default(),
        is_anime: animation && s(v, "original_language").as_deref() == Some("ja"),
    })
}

/// Parse the `episodes` array of one season object.
pub fn parse_season_episodes(v: &Value) -> Vec<EpisodeMeta> {
    let Some(arr) = v.get("episodes").and_then(|x| x.as_array()) else {
        return Vec::new();
    };
    arr.iter().filter_map(|e| {
        Some(EpisodeMeta {
            tmdb_id: i(e, "id")?,
            season_number: i(e, "season_number")?,
            episode_number: i(e, "episode_number")?,
            name: s(e, "name"),
            air_date: s(e, "air_date").and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()),
            runtime: i(e, "runtime"),
            still_path: s(e, "still_path"),
            overview: s(e, "overview"),
        })
    }).collect()
}

pub fn parse_movie(v: &Value) -> Result<MovieMeta> {
    let release_date = s(v, "release_date");
    Ok(MovieMeta {
        tmdb_id: i(v, "id").ok_or_else(|| AppError::msg("movie: missing id"))?,
        imdb_id: s(v, "imdb_id"),
        title: s(v, "title").ok_or_else(|| AppError::msg("movie: missing title"))?,
        year: year_of(release_date.as_deref()),
        release_date,
        runtime: i(v, "runtime"),
        poster_path: s(v, "poster_path"),
        backdrop_path: s(v, "backdrop_path"),
        overview: s(v, "overview"),
    })
}

pub fn parse_search(v: &Value) -> Vec<SearchResultItem> {
    let Some(arr) = v.get("results").and_then(|x| x.as_array()) else {
        return Vec::new();
    };
    arr.iter().filter_map(|r| {
        let media_type = s(r, "media_type")?;
        let (title, date) = match media_type.as_str() {
            "tv" => (s(r, "name")?, s(r, "first_air_date")),
            "movie" => (s(r, "title")?, s(r, "release_date")),
            _ => return None,
        };
        Some(SearchResultItem {
            tmdb_id: i(r, "id")?,
            media_type,
            title,
            year: year_of(date.as_deref()),
            poster_path: s(r, "poster_path"),
            overview: s(r, "overview"),
        })
    }).collect()
}

#[async_trait]
impl Tmdb for TmdbClient {
    async fn find_show(&self, tvdb_id: i64) -> Result<Option<i64>> {
        let v = self.get_json(&format!("/find/{tvdb_id}?external_source=tvdb_id")).await?;
        Ok(parse_find(&v, "tv_results"))
    }

    async fn find_movie(&self, imdb_id: &str) -> Result<Option<i64>> {
        let v = self.get_json(&format!("/find/{imdb_id}?external_source=imdb_id")).await?;
        Ok(parse_find(&v, "movie_results"))
    }

    async fn show_with_episodes(&self, tmdb_id: i64) -> Result<ShowFull> {
        let base = self.get_json(&format!("/tv/{tmdb_id}")).await?;
        let meta = parse_tv_base(&base)?;
        let mut episodes = Vec::new();
        // append_to_response accepts at most 20 sub-requests per call
        for chunk in meta.season_numbers.chunks(20) {
            let append = chunk.iter().map(|n| format!("season/{n}"))
                .collect::<Vec<_>>().join(",");
            let v = self.get_json(&format!("/tv/{tmdb_id}?append_to_response={append}")).await?;
            for n in chunk {
                if let Some(season) = v.get(format!("season/{n}")) {
                    episodes.extend(parse_season_episodes(season));
                }
            }
        }
        Ok(ShowFull { meta, episodes })
    }

    async fn movie(&self, tmdb_id: i64) -> Result<MovieMeta> {
        let v = self.get_json(&format!("/movie/{tmdb_id}")).await?;
        parse_movie(&v)
    }

    async fn search(&self, query: &str) -> Result<Vec<SearchResultItem>> {
        let q: String = url_encode(query);
        let v = self.get_json(&format!("/search/multi?include_adult=false&query={q}")).await?;
        Ok(parse_search(&v))
    }

    async fn validate(&self) -> Result<bool> {
        match self.get_json("/configuration").await {
            Ok(_) => Ok(true),
            Err(AppError::Http(e)) if e.status().map(|s| s.as_u16() == 401).unwrap_or(false) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

/// Minimal percent-encoding for a query value (no extra crate needed).
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_find_results() {
        let v = json!({"tv_results":[{"id":1396}],"movie_results":[]});
        assert_eq!(parse_find(&v, "tv_results"), Some(1396));
        assert_eq!(parse_find(&v, "movie_results"), None);
    }

    #[test]
    fn parses_tv_base() {
        let v = json!({
            "id": 1396, "name": "Breaking Bad", "first_air_date": "2008-01-20",
            "poster_path": "/p.jpg", "backdrop_path": "/b.jpg", "overview": "Chemistry.",
            "status": "Ended",
            "seasons": [{"season_number": 0}, {"season_number": 1}, {"season_number": 2}]
        });
        let m = parse_tv_base(&v).unwrap();
        assert_eq!(m.tmdb_id, 1396);
        assert_eq!(m.name, "Breaking Bad");
        assert_eq!(m.year, Some(2008));
        assert_eq!(m.status.as_deref(), Some("Ended"));
        assert_eq!(m.season_numbers, vec![0, 1, 2]);
    }

    #[test]
    fn detects_anime_from_language_and_genre() {
        let anime = json!({"id": 1, "name": "A", "original_language": "ja",
                           "genres": [{"id": 16, "name": "Animation"}], "seasons": []});
        assert!(parse_tv_base(&anime).unwrap().is_anime);
        let western_cartoon = json!({"id": 2, "name": "B", "original_language": "en",
                                     "genres": [{"id": 16, "name": "Animation"}], "seasons": []});
        assert!(!parse_tv_base(&western_cartoon).unwrap().is_anime);
        let jp_drama = json!({"id": 3, "name": "C", "original_language": "ja",
                              "genres": [{"id": 18, "name": "Drama"}], "seasons": []});
        assert!(!parse_tv_base(&jp_drama).unwrap().is_anime);
    }

    #[test]
    fn parses_season_episodes_with_empty_air_date() {
        let v = json!({"episodes":[
            {"id": 62085, "season_number": 1, "episode_number": 1, "name": "Pilot",
             "air_date": "2008-01-20", "runtime": 58, "still_path": "/s.jpg", "overview": "x"},
            {"id": 62086, "season_number": 1, "episode_number": 2, "name": "TBA",
             "air_date": "", "runtime": null, "still_path": null, "overview": ""}
        ]});
        let eps = parse_season_episodes(&v);
        assert_eq!(eps.len(), 2);
        assert_eq!(eps[0].air_date, chrono::NaiveDate::from_ymd_opt(2008, 1, 20));
        assert_eq!(eps[1].air_date, None);
        assert_eq!(eps[1].runtime, None);
    }

    #[test]
    fn parses_movie() {
        let v = json!({"id": 27205, "imdb_id": "tt1375666", "title": "Inception",
            "release_date": "2010-07-15", "runtime": 148,
            "poster_path": "/i.jpg", "backdrop_path": null, "overview": "Dreams."});
        let m = parse_movie(&v).unwrap();
        assert_eq!(m.tmdb_id, 27205);
        assert_eq!(m.year, Some(2010));
        assert_eq!(m.release_date.as_deref(), Some("2010-07-15"));
    }

    #[test]
    fn parses_search_skipping_people() {
        let v = json!({"results":[
            {"media_type":"tv","id":1,"name":"Show","first_air_date":"2020-05-05","poster_path":null,"overview":"o"},
            {"media_type":"movie","id":2,"title":"Film","release_date":"1999-01-01","poster_path":"/f.jpg","overview":"o"},
            {"media_type":"person","id":3,"name":"Actor"}
        ]});
        let r = parse_search(&v);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].media_type, "tv");
        assert_eq!(r[0].title, "Show");
        assert_eq!(r[0].year, Some(2020));
        assert_eq!(r[1].media_type, "movie");
        assert_eq!(r[1].title, "Film");
    }

    /// Live smoke test — needs a real key. Run manually:
    /// TMDB_KEY=... cargo test tmdb_live -- --ignored --nocapture   (PowerShell: $env:TMDB_KEY="...")
    #[tokio::test]
    #[ignore]
    async fn tmdb_live() {
        let key = std::env::var("TMDB_KEY").expect("set TMDB_KEY");
        let c = TmdbClient::new(reqwest::Client::new(), key);
        assert!(c.validate().await.unwrap());
        // Breaking Bad: TVDB 81189 -> TMDB 1396
        assert_eq!(c.find_show(81189).await.unwrap(), Some(1396));
        let full = c.show_with_episodes(1396).await.unwrap();
        assert_eq!(full.meta.name, "Breaking Bad");
        assert!(full.episodes.len() >= 62);
        let m = c.movie(27205).await.unwrap();
        assert_eq!(m.title, "Inception");
    }
}
