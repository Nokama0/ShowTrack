export interface NextEpisode {
  episodeId: number;
  seasonNumber: number;
  episodeNumber: number;
  name: string | null;
  airDate: string | null;
}

export interface ShowCard {
  tmdbId: number;
  name: string;
  posterPath: string | null;
  myState: "watching" | "stopped" | "plan_to_watch";
  isFavorite: boolean;
  watched: number;
  totalAired: number;
  next: NextEpisode | null;
  nextAirDate: string | null;
  lastActivity: string | null;
}

export interface ShowRow {
  tmdbId: number;
  tvdbId: number | null;
  imdbId: string | null;
  name: string;
  year: number | null;
  posterPath: string | null;
  backdropPath: string | null;
  overview: string | null;
  tmdbStatus: string | null;
  myState: "watching" | "stopped" | "plan_to_watch";
  isFavorite: boolean;
}

export interface EpisodeRow {
  tmdbId: number;
  seasonNumber: number;
  episodeNumber: number;
  name: string | null;
  airDate: string | null;
  runtime: number | null;
  stillPath: string | null;
  overview: string | null;
  watched: boolean;
  watchedAt: string | null;
}

export interface SeasonDetail {
  seasonNumber: number;
  episodes: EpisodeRow[];
}

export interface ShowDetail {
  show: ShowRow;
  seasons: SeasonDetail[];
}

export interface MovieRow {
  tmdbId: number;
  imdbId: string | null;
  title: string;
  year: number | null;
  releaseDate: string | null;
  runtime: number | null;
  posterPath: string | null;
  backdropPath: string | null;
  overview: string | null;
  myState: "watched" | "watchlist";
  watchedAt: string | null;
  rewatchCount: number;
  isFavorite: boolean;
  isAnime: boolean;
}

export interface UpcomingItem {
  episodeId: number;
  showTmdbId: number;
  showName: string;
  posterPath: string | null;
  seasonNumber: number;
  episodeNumber: number;
  episodeName: string | null;
  airDate: string;
}

export interface SearchItem {
  tmdbId: number;
  mediaType: "tv" | "movie";
  title: string;
  year: number | null;
  posterPath: string | null;
  overview: string | null;
  inLibrary: boolean;
  libState: string | null;
}

export interface ImportProgress {
  stage: "shows" | "movies";
  done: number;
  total: number;
  label: string;
}

export interface ImportFailure {
  kind: "show" | "movie";
  title: string;
  reason: string;
}

export interface ImportAnomaly {
  title: string;
  tmdbId: number;
  expected: number;
  actual: number;
}

export interface SpecialsNote {
  title: string;
  watchedSpecials: number;
}

export interface ImportReport {
  showsOk: number;
  moviesOk: number;
  failures: ImportFailure[];
  anomalies: ImportAnomaly[];
  specialsNote: SpecialsNote[];
  finishedAt: string;
}

export interface RefreshSummary {
  refreshed: number;
  failed: string[];
}

export interface CacheInfo {
  path: string;
  bytes: number;
}

export interface ReadySeason {
  seasonNumber: number;
  epCount: number;
  epLeft: number;
  runtimeLeftMin: number;
  kind: "start" | "continue" | "finish";
  /** Unwatched aired episodes in seasons before this one. */
  priorUnwatched: number;
}

export interface AiringSeason {
  seasonNumber: number;
  airedCount: number;
  totalCount: number;
  nextAirDate: string | null;
  finaleDate: string | null;
}

export interface BingeCard {
  tmdbId: number;
  name: string;
  posterPath: string | null;
  myState: "watching" | "stopped" | "plan_to_watch";
  isFavorite: boolean;
  isAnime: boolean;
  seriesWatchedAired: number;
  seriesAired: number;
  ready: ReadySeason | null;
  airing: AiringSeason | null;
  caughtUp: boolean;
  lastActivity: string | null;
}

export interface AiringRow {
  showTmdbId: number;
  showName: string;
  posterPath: string | null;
  seasonNumber: number;
  airedCount: number;
  totalCount: number;
  nextAirDate: string | null;
  finaleDate: string | null;
}

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

export interface UpdateInfo {
  version: string;
  notes: string;
  installerUrl: string | null;
  pageUrl: string;
  kind: "setup" | "portable";
}
