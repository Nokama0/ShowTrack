use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NextEpisode {
    pub episode_id: i64,
    pub season_number: i64,
    pub episode_number: i64,
    pub name: Option<String>,
    pub air_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowCard {
    pub tmdb_id: i64,
    pub name: String,
    pub poster_path: Option<String>,
    pub my_state: String,
    pub is_favorite: bool,
    pub watched: i64,
    pub total_aired: i64,
    pub next: Option<NextEpisode>,
    pub next_air_date: Option<String>,
    pub last_activity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowRow {
    pub tmdb_id: i64,
    pub tvdb_id: Option<i64>,
    pub imdb_id: Option<String>,
    pub name: String,
    pub year: Option<i64>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub overview: Option<String>,
    pub tmdb_status: Option<String>,
    pub my_state: String,
    pub is_favorite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeRow {
    pub tmdb_id: i64,
    pub season_number: i64,
    pub episode_number: i64,
    pub name: Option<String>,
    pub air_date: Option<String>,
    pub runtime: Option<i64>,
    pub still_path: Option<String>,
    pub overview: Option<String>,
    pub watched: bool,
    pub watched_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeasonDetail {
    pub season_number: i64,
    pub episodes: Vec<EpisodeRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowDetail {
    pub show: ShowRow,
    pub seasons: Vec<SeasonDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovieRow {
    pub tmdb_id: i64,
    pub imdb_id: Option<String>,
    pub title: String,
    pub year: Option<i64>,
    pub release_date: Option<String>,
    pub runtime: Option<i64>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub overview: Option<String>,
    pub my_state: String,
    pub watched_at: Option<String>,
    pub rewatch_count: i64,
    pub is_favorite: bool,
    pub is_anime: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpcomingItem {
    pub episode_id: i64,
    pub show_tmdb_id: i64,
    pub show_name: String,
    pub poster_path: Option<String>,
    pub season_number: i64,
    pub episode_number: i64,
    pub episode_name: Option<String>,
    pub air_date: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSeason {
    pub season_number: i64,
    pub episode_count: i64,
    pub first_air: Option<String>,
    pub last_air: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TitlePreview {
    pub media_type: String, // "tv" | "movie"
    pub title: String,
    pub year: Option<i64>,
    pub poster_path: Option<String>,
    pub overview: Option<String>,
    pub runtime: Option<i64>,   // movies
    pub status: Option<String>, // shows
    pub seasons: Vec<PreviewSeason>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchItem {
    pub tmdb_id: i64,
    pub media_type: String, // "tv" | "movie"
    pub title: String,
    pub year: Option<i64>,
    pub poster_path: Option<String>,
    pub overview: Option<String>,
    pub in_library: bool,
    /// the my_state of the matching library row, for the search badge
    pub lib_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    pub stage: String, // "shows" | "movies"
    pub done: usize,
    pub total: usize,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportFailure {
    pub kind: String, // "show" | "movie"
    pub title: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportAnomaly {
    pub title: String,
    pub tmdb_id: i64,
    pub expected: i64,
    pub actual: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecialsNote {
    pub title: String,
    pub watched_specials: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub shows_ok: i64,
    pub movies_ok: i64,
    pub failures: Vec<ImportFailure>,
    pub anomalies: Vec<ImportAnomaly>,
    pub specials_note: Vec<SpecialsNote>,
    pub finished_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshSummary {
    pub refreshed: i64,
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheInfo {
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadySeason {
    pub season_number: i64,
    pub ep_count: i64,
    pub ep_left: i64,
    pub runtime_left_min: i64,
    pub kind: String, // "start" | "continue" | "finish"
    /// Unwatched aired episodes in seasons BEFORE this one (drives "also mark previous").
    pub prior_unwatched: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiringSeason {
    pub season_number: i64,
    pub aired_count: i64,
    pub total_count: i64,
    pub next_air_date: Option<String>,
    pub finale_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BingeCard {
    pub tmdb_id: i64,
    pub name: String,
    pub poster_path: Option<String>,
    pub my_state: String,
    pub is_favorite: bool,
    pub is_anime: bool,
    pub series_watched_aired: i64,
    pub series_aired: i64,
    pub ready: Option<ReadySeason>,
    pub airing: Option<AiringSeason>,
    pub caught_up: bool,
    pub last_activity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiringRow {
    pub show_tmdb_id: i64,
    pub show_name: String,
    pub poster_path: Option<String>,
    pub season_number: i64,
    pub aired_count: i64,
    pub total_count: i64,
    pub next_air_date: Option<String>,
    pub finale_date: Option<String>,
}
