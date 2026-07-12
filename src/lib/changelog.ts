export interface ChangeEntry {
  version: string;
  notes: string[];
}

// Newest first. Add a new entry at the top for each release; the notes are the
// user-facing highlights shown in the "What's new" card after an update.
export const CHANGELOG: ChangeEntry[] = [
  {
    version: "0.3.0",
    notes: [
      "A what's-new pop-up now appears after each update, summarizing what changed.",
    ],
  },
  {
    version: "0.2.8",
    notes: [
      "Episodes that have not aired yet can no longer be checked off.",
      "The progress ring on show cards scales with the tile-size slider.",
    ],
  },
  {
    version: "0.2.7",
    notes: ["Poster grids fill the row edge to edge at every tile size."],
  },
  {
    version: "0.2.6",
    notes: ["The tile-size slider goes smaller, fitting even more per row."],
  },
  {
    version: "0.2.5",
    notes: [
      "Tile-size sliders on Shows, Anime and Movies.",
      "Keep search: the Search page can remember your last search.",
    ],
  },
  {
    version: "0.2.4",
    notes: [
      "Episode-by-episode mode, for watching without waiting for the full season.",
      "Preview any search result before adding it.",
      "Anime movies, a filter box on Shows and Anime, and a customizable sidebar.",
    ],
  },
  {
    version: "0.2.3",
    notes: ["One Add to library button with a guided flow, plus anime as an add option."],
  },
  {
    version: "0.2.2",
    notes: ["Each section remembers its filter; search results show their library state."],
  },
  {
    version: "0.2.1",
    notes: ["Portable builds update themselves, and you can set a custom window title."],
  },
  {
    version: "0.2.0",
    notes: ["Built-in updater, title removal, an All movies filter, and rewatch editing."],
  },
];

/** Compare X.Y.Z versions: -1, 0, 1. */
function cmp(a: string, b: string): number {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (d !== 0) return d > 0 ? 1 : -1;
  }
  return 0;
}

/** Entries strictly newer than `since`, up to and including `current` (newest first). */
export function changesSince(since: string, current: string): ChangeEntry[] {
  return CHANGELOG.filter((e) => cmp(e.version, since) > 0 && cmp(e.version, current) <= 0);
}

/** The notes for a specific version, for re-opening the pop-up on demand;
 *  falls back to the newest entry if that version has none. */
export function changelogFor(version: string): ChangeEntry[] {
  const m = CHANGELOG.filter((e) => e.version === version);
  return m.length ? m : CHANGELOG.slice(0, 1);
}
