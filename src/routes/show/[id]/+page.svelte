<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { appConfirm } from "#lib/dialog.svelte.js";
  import { api, imgUrl } from "#lib/api.js";
  import { fmtRuntime, relDays } from "#lib/format.js";
  import type { EpisodeRow, ShowDetail } from "#lib/types.js";
  import PosterImg from "#lib/ui/PosterImg.svelte";
  import Select from "#lib/ui/Select.svelte";

  let detail = $state<ShowDetail | null>(null);
  let error = $state("");
  let actionError = $state("");

  const id = $derived(Number(page.params.id));
  const today = new Date().toLocaleDateString("sv-SE"); // YYYY-MM-DD, local

  async function load() {
    try {
      detail = await api.showDetail(id);
    } catch (e) {
      error = String(e);
    }
  }
  onMount(load);

  async function safely(fn: () => Promise<void>) {
    try {
      actionError = "";
      await fn();
    } catch (e) {
      actionError = String(e);
      await load(); // resync UI with DB after a failed/partial mutation
    }
  }

  const airedIn = (eps: EpisodeRow[]) => eps.filter((e) => e.airDate && e.airDate <= today).length;
  const watchedIn = (eps: EpisodeRow[]) => eps.filter((e) => e.watched).length;
  const runtimeLeft = (eps: EpisodeRow[]) =>
    eps.filter((e) => !e.watched && e.airDate && e.airDate <= today)
      .reduce((s, e) => s + (e.runtime ?? 0), 0);

  type SeasonState = "done" | "ready" | "airing" | "tba";
  function seasonState(eps: EpisodeRow[]): SeasonState {
    const total = eps.length;
    const aired = airedIn(eps);
    if (total > 0 && watchedIn(eps) >= total) return "done";
    if (total > 0 && aired === total) return "ready";
    if (aired > 0 || eps.some((e) => e.airDate && e.airDate > today)) return "airing";
    return "tba";
  }
  function seasonBadge(eps: EpisodeRow[]): string {
    const st = seasonState(eps);
    if (st === "done") return "watched";
    if (st === "airing") return `airing · ${airedIn(eps)}/${eps.length} aired`;
    if (st === "tba") return "dates TBA";
    const left = eps.length - watchedIn(eps);
    return left === eps.length ? "ready to watch" : `${left} left`;
  }
  const activeSeason = $derived.by(() => {
    if (!detail) return null;
    const s = detail.seasons.find(
      (x) =>
        x.seasonNumber > 0 &&
        watchedIn(x.episodes) < airedIn(x.episodes),
    );
    return s?.seasonNumber ?? null;
  });

  async function toggleEpisode(epId: number, watched: boolean) {
    if (!detail) return;
    let at: { season: number; episode: number } | null = null;
    for (const s of detail.seasons)
      for (const e of s.episodes)
        if (e.tmdbId === epId) {
          e.watched = watched;
          e.watchedAt = watched ? new Date().toISOString() : null;
          at = { season: e.seasonNumber, episode: e.episodeNumber };
        }
    await safely(() => api.episodeMark(epId, watched));
    // the backend starts a planned show on its first watch; this toggle skips the reload
    if (!actionError && watched && detail.show.myState === "plan_to_watch") detail.show.myState = "watching";
    epPrompt = null;
    if (!actionError && watched && at && at.season > 0) {
      const count = unwatchedAiredBefore(at.season, at.episode);
      if (count > 0) epPrompt = { ...at, count };
    }
  }

  let epPrompt = $state<{ season: number; episode: number; count: number } | null>(null);

  const unwatchedAiredBefore = (season: number, episode: number) => {
    const s = detail?.seasons.find((x) => x.seasonNumber === season);
    return s
      ? s.episodes.filter((e) => e.episodeNumber < episode && !e.watched && e.airDate && e.airDate <= today).length
      : 0;
  };

  async function markPreviousEpisodes() {
    const p = epPrompt;
    if (!p) return;
    epPrompt = null;
    await safely(async () => {
      await api.episodesMarkBefore(id, p.season, p.episode);
      await load();
    });
  }

  let priorPrompt = $state<{ season: number; count: number } | null>(null);

  const priorUnwatchedBefore = (season: number) => {
    if (!detail) return 0;
    return detail.seasons
      .filter((s) => s.seasonNumber > 0 && s.seasonNumber < season)
      .flatMap((s) => s.episodes)
      .filter((e) => !e.watched && e.airDate && e.airDate <= today).length;
  };

  async function markSeason(season: number, watched: boolean) {
    await safely(async () => {
      await api.seasonMark(id, season, watched);
      await load();
    });
    epPrompt = null;
    priorPrompt = null;
    if (watched) {
      const count = priorUnwatchedBefore(season);
      if (count > 0) priorPrompt = { season, count };
    }
  }

  async function markPrevious() {
    const p = priorPrompt;
    if (!p) return;
    priorPrompt = null;
    await safely(async () => {
      await api.seasonsMarkBefore(id, p.season);
      await load();
    });
  }

  async function removeShow() {
    if (!detail) return;
    const ok = await appConfirm({
      title: "Remove show",
      message: `Remove ${detail.show.name} from your library? Its watch history goes with it.`,
      confirmLabel: "Remove",
      danger: true,
    });
    if (!ok) return;
    try {
      await api.showRemove(id);
      // land back wherever the user came from (Anime, Search, Shows...)
      if (history.length > 1) history.back();
      else goto("/", { replace: true });
    } catch (e) {
      actionError = String(e);
    }
  }

  async function markAll() {
    epPrompt = null;
    await safely(async () => {
      await api.showMarkAll(id);
      await load();
    });
  }

  async function setState(myState: string) {
    if (!detail) return;
    detail.show.myState = myState as ShowDetail["show"]["myState"];
    await safely(() => api.showSetState(id, myState));
  }

  async function toggleFav() {
    await safely(async () => {
      if (!detail) return;
      detail.show.isFavorite = await api.showToggleFavorite(id);
    });
  }

  const se = (s: number, e: number) => `S${String(s).padStart(2, "0")}E${String(e).padStart(2, "0")}`;
  const seasonName = (n: number) => (n === 0 ? "Specials" : `Season ${n}`);
</script>

{#if error}
  <p style="color:var(--red)">{error}</p>
{:else if !detail}
  <p class="dim">Loading…</p>
{:else}
  {#if detail.show.backdropPath}
    <div class="backdrop" style={`background-image:url(${imgUrl("w780", detail.show.backdropPath)})`}></div>
  {/if}
  <div class="dcols">
    <aside class="drail">
      <div class="posterbox"><PosterImg path={detail.show.posterPath} alt={detail.show.name} glow /></div>
      <h1 class="name">
        {detail.show.name}
        {#if detail.show.year}<span class="dim yr">({detail.show.year})</span>{/if}
      </h1>
      <div class="dim sub">
        {detail.show.tmdbStatus ?? ""}
        <button class="fav" class:favon={detail.show.isFavorite} onclick={toggleFav} title="Favorite">★</button>
      </div>
      <div class="controls">
        <Select
          value={detail.show.myState}
          label="Show status"
          options={[
            { value: "watching", label: "Watching" },
            { value: "plan_to_watch", label: "Plan to watch" },
            { value: "stopped", label: "Stopped" },
          ]}
          onchange={(v) => setState(v)}
        />
        <button class="btn-ghost" onclick={markAll}>Mark all aired watched</button>
        <button class="btn-ghost removebtn" onclick={removeShow}>Remove from library</button>
      </div>
      {#if actionError}<p class="acterr">{actionError}</p>{/if}
      {#if priorPrompt}
        <button class="btn-ghost priorbtn" onclick={markPrevious}>
          Also mark seasons before S{priorPrompt.season} ({priorPrompt.count} episodes)
        </button>
      {/if}
      {#if detail.show.overview}<p class="dim ov">{detail.show.overview}</p>{/if}
    </aside>

    <div class="seasons">
      {#each detail.seasons as season, i (season.seasonNumber)}
        <details class="panel season rise" style={`animation-delay:${Math.min(i, 8) * 50}ms`} open={season.seasonNumber === activeSeason}>
          <summary>
            <span class="sname">{seasonName(season.seasonNumber)}</span>
            <span class="badge" class:dim={seasonState(season.episodes) === "done" || season.seasonNumber === 0}>
              {seasonBadge(season.episodes)}
            </span>
            <span class="dim counts">
              {watchedIn(season.episodes)}/{season.episodes.length}
              {#if runtimeLeft(season.episodes) > 0}&nbsp;·&nbsp;~{fmtRuntime(runtimeLeft(season.episodes))} left{/if}
            </span>
            {#if airedIn(season.episodes) > 0}
              <button
                class="sbtn"
                onclick={(e) => {
                  e.preventDefault();
                  markSeason(season.seasonNumber, watchedIn(season.episodes) < airedIn(season.episodes));
                }}
              >
                {watchedIn(season.episodes) < airedIn(season.episodes) ? "✓ Season watched" : "Unmark season"}
              </button>
            {/if}
          </summary>
          {#each season.episodes as ep (ep.tmdbId)}
            {@const rel = relDays(ep.airDate)}
            {@const aired = !!ep.airDate && ep.airDate <= today}
            <label class="ep" class:future={!aired}>
              <input
                type="checkbox"
                checked={ep.watched}
                disabled={!aired && !ep.watched}
                onchange={(e) => toggleEpisode(ep.tmdbId, e.currentTarget.checked)}
              />
              <span class="dim senum">{se(ep.seasonNumber, ep.episodeNumber)}</span>
              <span class="ename">{ep.name ?? "TBA"}</span>
              <span class="dim edate">{ep.airDate ?? ""}{#if rel}<span class="rel">· {rel}</span>{/if}</span>
            </label>
            {#if epPrompt && epPrompt.season === ep.seasonNumber && epPrompt.episode === ep.episodeNumber}
              <button class="btn-ghost eprior" onclick={markPreviousEpisodes}>
                Also mark episodes before E{epPrompt.episode} ({epPrompt.count} {epPrompt.count === 1 ? "episode" : "episodes"})
              </button>
            {/if}
          {/each}
        </details>
      {/each}
    </div>
  </div>
{/if}

<style>
  .backdrop {
    /* full-bleed: escape main's padding so the art spans the whole content area */
    margin: calc(var(--main-pad-y) * -1) calc(var(--main-pad-x) * -1) -110px;
    height: 340px; background-size: cover;
    background-position: center 20%; opacity: 0.38;
    mask-image: linear-gradient(to bottom, black 30%, transparent 96%);
    animation: fadein 0.7s ease both;
  }
  .dcols { display: grid; grid-template-columns: 1fr; gap: 36px; position: relative; }
  @media (min-width: 1720px) {
    .dcols { grid-template-columns: 340px minmax(0, 1fr); }
    .drail { position: sticky; top: 28px; align-self: start; }
  }
  .posterbox { max-width: 260px; margin-bottom: 16px; }
  .name { font-size: 26px; }
  .yr { font-family: "Archivo Variable", sans-serif; font-size: 15px; text-transform: none; letter-spacing: 0; }
  .sub { margin: 6px 0 14px; display: flex; align-items: center; gap: 8px; }
  .fav { background: none; font-size: 20px; color: var(--dim); padding: 0 4px; }
  .fav.favon { color: var(--gold); }
  .controls { display: flex; flex-direction: column; gap: 10px; max-width: 260px; }
  .acterr { color: var(--red); margin: 10px 0 0; }
  .removebtn { color: var(--red); border-color: rgba(255, 84, 112, 0.35); }
  .removebtn:hover { border-color: var(--red); color: var(--red); }
  .priorbtn { color: var(--gold); border-color: rgba(255, 207, 110, 0.4); margin-top: 10px; animation: rise 0.3s cubic-bezier(0.2, 0.7, 0.3, 1) both; }
  .eprior {
    color: var(--gold); border-color: rgba(255, 207, 110, 0.4);
    margin: 8px 0 8px 26px; /* indented to the episode text, past the checkbox */
    padding: 7px 14px; font-size: 12.5px;
    animation: rise 0.3s cubic-bezier(0.2, 0.7, 0.3, 1) both;
  }
  .ov { max-width: 300px; margin-top: 16px; font-size: 13px; }
  .seasons { display: flex; flex-direction: column; gap: 12px; min-width: 0; }
  .season { padding: 14px 18px; }
  summary {
    cursor: pointer; display: flex; align-items: center; gap: 18px; flex-wrap: wrap;
    padding: 2px 0;
    /* keep the gap under the header in step with the season collapse */
    transition: margin-bottom 0.35s cubic-bezier(0.2, 0.7, 0.3, 1);
  }
  summary:hover .sname { color: var(--gold); }
  .season[open] summary { margin-bottom: 12px; }
  .sname {
    font-family: "Anton", sans-serif; font-size: 15px; letter-spacing: 0.06em;
    text-transform: uppercase; transition: color 0.18s ease;
  }
  .counts { font-size: 12px; }
  .sbtn { margin-left: auto; font-size: 12px; }
  .ep {
    display: flex; align-items: center; gap: 10px; padding: 7px 6px;
    border-top: 1px solid var(--line); cursor: pointer; border-radius: 6px;
    transition: background-color 0.15s ease;
  }
  .ep:hover { background: rgba(255, 255, 255, 0.03); }
  /* an episode that has not aired yet cannot be watched */
  .ep.future { cursor: default; }
  .ep.future:hover { background: none; }
  .ep.future .senum, .ep.future .ename { opacity: 0.5; }
  .senum { font-family: monospace; }
  .ename { flex: 1; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .edate { font-size: 12px; }
  .rel { color: var(--gold); margin-left: 6px; }
  .ep input { width: 16px; height: 16px; }
  @media (max-width: 760px) {
    .backdrop { height: 210px; margin-bottom: -120px; }
    .posterbox { max-width: 150px; }
    .name { font-size: 21px; }
    .ov { font-size: 12.5px; max-width: none; }
    .controls { max-width: none; }
    summary { gap: 10px; }
    .sbtn { flex-basis: 100%; margin-left: 0; order: 9; }
    .ep { min-height: 44px; padding: 8px 6px; }
    .edate { font-size: 11px; }
  }
</style>
