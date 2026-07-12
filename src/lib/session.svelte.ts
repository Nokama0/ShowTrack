import { api } from "./api";

export interface Features {
  movies: boolean;
  animePage: boolean;
  airingPage: boolean;
  searchPage: boolean;
  hero: boolean;
  airingRail: boolean;
}

export const session = $state({
  ready: false,
  hasKey: false,
  banner: "" as string,
  refreshTick: 0,
  zoom: 100,
  askAddType: false,
  allFirst: false,
  episodeMode: false,
  navReorder: false,
  navSeparators: false,
  navOrder: ["/", "/anime", "/airing", "/movies", "/search"] as string[],
  tabs: {
    shows: "watching",
    anime: "watching",
    movies: "watched",
  } as Record<"shows" | "anime" | "movies", string>,
  features: {
    movies: true,
    animePage: true,
    airingPage: true,
    searchPage: true,
    hero: true,
    airingRail: true,
  } as Features,
});

export async function loadSession() {
  try {
    const s = await api.settingsAll();
    session.hasKey = !!s["tmdb_key"];
    const z = parseInt(s["ui_zoom"] ?? "100", 10);
    session.zoom = Number.isFinite(z) ? Math.min(150, Math.max(80, z)) : 100;
    session.askAddType = s["search_ask_type"] === "1";
    session.allFirst = s["all_first"] === "1";
    session.episodeMode = s["episode_mode"] === "1";
    session.navReorder = s["nav_reorder"] === "1";
    session.navSeparators = s["nav_separators"] === "1";
    const canonicalNav = ["/", "/anime", "/airing", "/movies", "/search"];
    const storedNav = (s["nav_order"] ?? "").split(",").filter((h) => canonicalNav.includes(h));
    session.navOrder = [...storedNav, ...canonicalNav.filter((h) => !storedNav.includes(h))];
    const pick = (key: string, allowed: string[], dflt: string) =>
      allowed.includes(s[key]) ? s[key] : dflt;
    session.tabs = {
      shows: pick("tab_shows", ["watching", "plan_to_watch", "stopped", "all"], "watching"),
      anime: pick("tab_anime", ["watching", "plan_to_watch", "stopped", "all"], "watching"),
      movies: pick("tab_movies", ["watched", "watchlist", "all"], "watched"),
    };
    const on = (key: string) => s[key] !== "0"; // everything defaults to on
    session.features = {
      movies: on("feat_movies"),
      animePage: on("feat_anime_page"),
      airingPage: on("feat_airing_page"),
      searchPage: on("feat_search_page"),
      hero: on("feat_hero"),
      airingRail: on("feat_airing_rail"),
    };
  } catch (e) {
    session.banner = `Failed to load settings: ${e}`;
  } finally {
    session.ready = true;
  }
}
