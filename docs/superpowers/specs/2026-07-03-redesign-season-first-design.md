# ShowTrack — "Marquee/Signal" Redesign + Season-First Model (2026-07-03)

Full UI redesign per approved mockup `design-previews/mockup-d-merged.html` (direction D: the
Marquee skin on the Signal layout), rebuilt around the user's actual viewing model:

> **The user binges whole seasons, starting only once the season's final episode has aired.**
> "Next episode available" is meaningless; the atomic unit of the UI is the **season**.

## Visual system (from mockup D — the mockup file is the visual reference of record)

- **Tokens**: `--bg #07090f`, `--raise #0d1119`, `--raise-hi #131927`, `--line #1a2130`,
  `--text #e9edf4`, `--dim #8b94a7`, `--faint #525b6e`, accent `--gold #ffcf6e`
  (+ `--gold-dim rgba(255,207,110,.14)`); radii 18px (cards) / 12px (controls); film-grain
  overlay + bottom gradient on posters.
- **Type**: Anton (display: page titles, section eyebrows, season markers, stat numbers —
  always uppercase, letter-spaced) + Archivo (everything else). Bundled locally via
  `@fontsource/anton` + `@fontsource-variable/archivo` — no CDN at runtime.
- **Navigation**: 88px icon dock (replaces the sidebar): Shows · Airing · Movies · Search ·
  Settings, active = inset gold outline. Dock foot shows the episodes-watched tally.
- **Posters**: real TMDB art via the existing `poster://` protocol. Signature light-bleed:
  each poster card casts a colored glow (`box-shadow`) derived from the poster's dominant
  color, sampled client-side on a small canvas. Requires the protocol response to add
  `Access-Control-Allow-Origin: *` and `<img crossorigin="anonymous">` (canvas readback across
  the tauri.localhost ↔ poster.localhost origin pair). Fallback when sampling fails or no
  poster: neutral slate glow + Anton uppercase title block (as in the mockup).
- Buttons: gold pill = primary (`✓ Season watched`), ghost pill = secondary. Filter chips,
  stat pills, and badge styles exactly as the mockup.
- **Ultrawide-first layout** (user runs 3440×1440; no left-crammed columns, no dead space):
  content is fluid full-width. At ≥1720px viewport the Shows screen is a two-zone dashboard —
  center (hero + ready grid, fluid) and a **420px sticky right rail** holding Airing now +
  Caught up; below 1720px the rail stacks under the center column. Poster tracks are
  `minmax(184px, 218px)` auto-fill so cards multiply across width instead of ballooning.
  The same principle applies to every screen: Movies/Search grids fill the width, the detail
  page uses a wide two-column arrangement (poster/meta rail + season blocks) at ≥1720px,
  Airing uses multi-column rows. No screen gets a hard max-width below ~2600px.

## Season model (backend)

All derivable from existing tables — **no schema change**. Specials (season 0) are excluded
from every computation below; they remain visible on the detail page only.

Per (show, season>0), with `today` = local date:
- `total`, `aired` (air_date NOT NULL AND ≤ today), `watched`, `unairedKnown` (air_date > today),
  `dateless` (air_date IS NULL), `finale` = MAX(air_date) **only if dateless = 0**, else None;
  `nextAir` = MIN(air_date > today); `runtimeLeftMin` = SUM(runtime) over aired∧unwatched
  (missing runtimes contribute 0; UI prefixes the figure with "~").
- **complete** = aired == total AND total > 0 (implies dateless = 0).
- Season kind for the UI:
  - **ready**: complete AND watched < total. Sub-kind: `start` (zero watches show-wide) /
    `continue` (the show has watches elsewhere, this season has none) / `finish`
    (watched > 0 in this season — e.g. numbering-drift leftovers; badge "N left in Sx").
  - **airing**: not complete AND (aired > 0 OR nextAir exists). Label: "bingeable {finale}"
    when finale known, "starts {nextAir}" when aired == 0, "finale TBA" when dateless > 0.
  - **done**: watched == total.

Show-level derivation for the Shows screen:
- `ready` = the LOWEST-numbered ready season (marking it promotes the next one on refetch).
- `airing` = the current airing season (lowest-numbered non-complete season with activity).
- **Caught up** = no ready season AND no unwatched aired episode anywhere (season>0).

### New IPC surface

- `binge_list() -> Vec<BingeCard>`; `BingeCard { tmdbId, name, posterPath, myState, isFavorite,
  seriesWatched, seriesAired, ready: Option<ReadySeason>, airing: Option<AiringSeason>,
  lastActivity }` with `ReadySeason { seasonNumber, epCount, epLeft, runtimeLeftMin, kind }`
  and `AiringSeason { seasonNumber, airedCount, totalCount, nextAirDate, finaleDate }`
  (display labels — "bingeable {date}" / "starts {date}" / "finale TBA" — are computed in the
  UI from these fields, in one shared helper).
- `airing_list() -> Vec<AiringRow>` — airing seasons of `watching` shows only:
  `{ showTmdbId, showName, posterPath, seasonNumber, airedCount, totalCount, nextAirDate,
  finaleDate }`, sorted by finale date (known first, ascending), then name.
- Existing commands are kept unchanged (detail page computes its season blocks client-side
  from `show_detail`'s episode rows).

## Screens

- **Shows** (default): header "READY TO BINGE" + three stat pills (seasons ready / ready to
  start / caught up); filter chips (Watching · Plan to watch · Stopped · All). Chip scoping:
  the default **Watching** view is the full binge page (hero, ready grid — which deliberately
  mixes `watching` shows and plan-to-watch `start` cards — airing rows, caught-up chips);
  the other chips show a plain poster grid of that state's shows (ready badges still shown). **Hero =
  tonight's binge**: the ready `continue`/`finish` card with the most recent `lastActivity`,
  else the alphabetically-first ready `start`; giant Anton "Season N", "{eps} episodes ·
  ~{h m} · finale aired {date}", actions "Episode list" (→ detail) + gold "✓ Season watched".
  Grid of remaining ready cards: corner **ring** (series progress = seriesWatched/seriesAired,
  gold arc) whose tap marks that season; badges "ready to binge" / "ready to start" /
  "N left in Sx". Then **Airing now** rows (fill-up bar, "BINGEABLE {date}" / "starts {date}" /
  "finale TBA"), then **Caught up** gold-dot chips. Empty states per section as in mockup copy.
- **Ring tap / hero ✓**: calls existing `season_mark(show, season, true)`, optimistic; the
  card/hero swaps to a transient "Season N marked — Undo" state (Undo = `season_mark(false)`),
  cleared on next data refresh. No confirm dialog.
- **Airing** (replaces "Upcoming"): the `airing_list` rows grouped under month headers,
  same row style as the Shows screen's airing section, plus each row's next-episode date as
  small caption. Empty state: "Nothing airing for the shows you follow."
- **Show detail**: same data, new skin; seasons render as **season blocks** — header row per
  season: Anton "SEASON N", counts, runtime-left, state badge, and a prominent per-season
  "✓ Season watched" / "Unmark" (aired-aware, as today); episode checklist collapses under it
  (collapsed by default for `done` and `ready-start` seasons, open for the active one).
- **Movies / Search / Settings / Onboarding**: functionality unchanged, reskinned to the
  token system (dock, chips, pills, panels, type). Search result cards adopt the poster
  treatment; movie modal becomes a raised panel with gold primary actions.
- **UI zoom** control and all Settings features carry over unchanged.

## Out of scope (unchanged from v1)

Stats page, ratings/notes, notifications, episode rewatches, drag/reorder, per-episode
"watch next" surfaces (removed by design).

## Error handling

Unchanged patterns: tick-driven refetch (`session.refreshTick`), `loadError` per screen,
`safely()`-style resync on mutation failure, offline banner. Dominant-color sampling must
never block or break a card (async, fallback glow).

## Testing

- Rust: unit tests for the season classifier (ready/airing/done, dateless, drift-finish,
  multi-season promotion, specials exclusion) and both new queries/commands against seeded
  in-memory DBs; existing suite stays green.
- Frontend gates: `pnpm check` 0/0, `pnpm build` clean. Manual: mockup-parity pass on each
  screen with the real library, ring-mark + Undo round-trip, color sampling on real posters.

## Recorded deviations (accepted)

- Dock-foot episode tally omitted (Shows header stats cover it).
- Hero meta line shows episode count + runtime + "the whole season is waiting" — no
  "finale aired {date}" (ReadySeason carries no finale date); revisit only if missed in use.
- Ring-tap Undo removes exactly the watches the tap created (episodes_unmark); the detail
  page's "Unmark season" keeps whole-season semantics by design.
- Plan-to-watch shows stay on the binge dashboard after their first season is marked
  (readyPool admits plan_to_watch regardless of ready-kind).
