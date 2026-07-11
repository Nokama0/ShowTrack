import { invoke } from "@tauri-apps/api/core";
import type {
  AiringRow, BingeCard, CacheInfo, ImportReport, MovieRow, PairSummary, RefreshSummary,
  SearchItem, ShowCard, ShowDetail, SyncServerStatus, SyncStatus, SyncSummary, UpcomingItem, UpdateInfo,
} from "./types";
import { isAndroid } from "./platform";

export const api = {
  settingsAll: () => invoke<Record<string, string>>("settings_all"),
  settingsSet: (key: string, value: string) => invoke<void>("settings_set", { key, value }),
  tmdbValidateKey: (key: string) => invoke<boolean>("tmdb_validate_key", { key }),
  importRun: (path: string) => invoke<void>("import_run", { path }),
  importLastReport: () => invoke<ImportReport | null>("import_last_report"),
  refreshRun: () => invoke<RefreshSummary>("refresh_run"),
  showsList: () => invoke<ShowCard[]>("shows_list"),
  showDetail: (id: number) => invoke<ShowDetail>("show_detail", { id }),
  showSetState: (id: number, myState: string) => invoke<void>("show_set_state", { id, myState }),
  showToggleFavorite: (id: number) => invoke<boolean>("show_toggle_favorite", { id }),
  episodeMark: (id: number, watched: boolean) => invoke<void>("episode_mark", { id, watched }),
  seasonMark: (showId: number, season: number, watched: boolean) =>
    invoke<number[]>("season_mark", { showId, season, watched }),
  episodesUnmark: (ids: number[]) => invoke<void>("episodes_unmark", { ids }),
  seasonsMarkBefore: (showId: number, season: number) =>
    invoke<number[]>("seasons_mark_before", { showId, season }),
  episodesMarkBefore: (showId: number, season: number, episode: number) =>
    invoke<number[]>("episodes_mark_before", { showId, season, episode }),
  showMarkAll: (id: number) => invoke<void>("show_mark_all", { id }),
  showMarkNext: (id: number) => invoke<ShowCard>("show_mark_next", { id }),
  upcomingList: () => invoke<UpcomingItem[]>("upcoming_list"),
  moviesList: () => invoke<MovieRow[]>("movies_list"),
  movieSetState: (id: number, myState: string) => invoke<void>("movie_set_state", { id, myState }),
  movieRewatch: (id: number) => invoke<number>("movie_rewatch", { id }),
  movieSetRewatch: (id: number, count: number) => invoke<number>("movie_set_rewatch", { id, count }),
  showRemove: (id: number) => invoke<void>("show_remove", { id }),
  movieRemove: (id: number) => invoke<void>("movie_remove", { id }),
  titleAdd: (tmdbId: number, src: string, target: string, myState: string) =>
    invoke<void>("title_add", { tmdbId, src, target, myState }),
  updateCheck: () => invoke<UpdateInfo | null>("update_check"),
  updateInstall: (url: string) => invoke<void>("update_install", { url }),
  movieToggleFavorite: (id: number) => invoke<boolean>("movie_toggle_favorite", { id }),
  searchTmdb: (query: string) => invoke<SearchItem[]>("search_tmdb", { query }),
  showAdd: (tmdbId: number, myState: string) => invoke<void>("show_add", { tmdbId, myState }),
  movieAdd: (tmdbId: number, myState: string) => invoke<void>("movie_add", { tmdbId, myState }),
  cacheInfo: () => invoke<CacheInfo>("cache_info"),
  bingeList: () => invoke<BingeCard[]>("binge_list"),
  airingList: () => invoke<AiringRow[]>("airing_list"),
  syncServerStatus: () => invoke<SyncServerStatus>("sync_server_status"),
  syncServerSet: (enabled: boolean, port: number) =>
    invoke<SyncServerStatus>("sync_server_set", { enabled, port }),
  syncPairCode: () => invoke<string>("sync_pair_code"),
  syncUnpair: () => invoke<void>("sync_unpair"),
  animeScan: () => invoke<number>("anime_scan"),
  backupExport: (path: string) => invoke<void>("backup_export", { path }),
  backupImport: (path: string) => invoke<PairSummary>("backup_import", { path }),
  syncPair: (addr: string, code: string) => invoke<PairSummary>("sync_pair", { addr, code }),
  syncNow: () => invoke<SyncSummary>("sync_now"),
  syncStatus: () => invoke<SyncStatus>("sync_status"),
};

/** Build a poster:// URL (Windows WebView2 exposes custom schemes as http://<scheme>.localhost). */
export function imgUrl(size: "w185" | "w342" | "w780" | "original", path: string | null | undefined): string | null {
  if (!path) return null;
  const rel = `${size}${path}`; // TMDB paths start with '/'
  return navigator.userAgent.includes("Windows") || isAndroid
    ? `http://poster.localhost/${rel}`
    : `poster://localhost/${rel}`;
}
