<script lang="ts">
  import { goto } from "$app/navigation";
  import { api } from "$lib/api";
  import { session } from "$lib/session.svelte";
  import { askDialog } from "$lib/dialog.svelte";
  import type { SearchItem } from "$lib/types";
  import PosterImg from "$lib/ui/PosterImg.svelte";

  $effect(() => {
    if (!session.features.searchPage) goto("/", { replaceState: true });
  });

  let query = $state("");
  let results = $state<SearchItem[]>([]);
  let searching = $state(false);
  let error = $state("");
  let addedIds = $state<Set<string>>(new Set());
  let timer: ReturnType<typeof setTimeout> | undefined;

  let seq = 0;

  $effect(() => {
    const q = query.trim();
    clearTimeout(timer);
    if (q.length < 2) {
      seq++; // invalidate any in-flight search
      results = [];
      error = "";
      searching = false;
      return;
    }
    timer = setTimeout(async () => {
      const mySeq = ++seq;
      searching = true;
      error = "";
      try {
        const r = await api.searchTmdb(q);
        if (mySeq !== seq) return; // stale response — a newer search superseded us
        results = r;
      } catch (e) {
        if (mySeq !== seq) return;
        error = String(e);
      } finally {
        if (mySeq === seq) searching = false;
      }
    }, 350);
    return () => clearTimeout(timer);
  });

  async function add(item: SearchItem, target: "show" | "movie", myState: string) {
    error = "";
    try {
      await api.titleAdd(item.tmdbId, item.mediaType === "tv" ? "tv" : "movie", target, myState);
      addedIds = new Set(addedIds).add(`${item.mediaType}-${item.tmdbId}`);
    } catch (e) {
      error = String(e);
    }
  }

  const stateLabels: Record<string, string> = {
    watching: "Watching", plan_to_watch: "Plan to watch",
    watched: "Watched", watchlist: "Watchlist",
  };
  // the clicked state carried over to the other section keeps its intent:
  // currently engaged stays engaged, saved for later stays saved for later
  const crossState: Record<string, string> = {
    watching: "watched", plan_to_watch: "watchlist",
    watched: "watching", watchlist: "plan_to_watch",
  };

  async function requestAdd(item: SearchItem, target: "show" | "movie", myState: string) {
    if (!session.askAddType || !session.features.movies) return add(item, target, myState);
    const otherState = crossState[myState];
    const showState = target === "show" ? myState : otherState;
    const movieState = target === "movie" ? myState : otherState;
    const pick = await askDialog({
      title: `Add ${item.title}`,
      message: `TMDB lists this as a ${item.mediaType === "tv" ? "show" : "movie"}. Where should it go?`,
      buttons: [
        { label: `As a show · ${stateLabels[showState]}`, value: "show", kind: "gold" },
        { label: `As a movie · ${stateLabels[movieState]}`, value: "movie", kind: "gold" },
        { label: "Cancel", value: "cancel", kind: "ghost" },
      ],
    });
    if (pick !== "show" && pick !== "movie") return;
    await add(item, pick, pick === "show" ? showState : movieState);
  }
</script>

<div class="head"><h1>Search</h1></div>
<input class="big" placeholder="Search shows and movies on TMDB…" bind:value={query} aria-label="Search TMDB" />
{#if error}<p style="color:var(--red)">{error}</p>{/if}
{#if searching}<p class="dim">Searching…</p>{/if}

<div class="grid-posters results">
  {#each results.filter((r) => session.features.movies || r.mediaType !== "movie") as r, i (`${r.mediaType}-${r.tmdbId}`)}
    <div class="cell rise" style={`animation-delay:${Math.min(i, 14) * 35}ms`}>
      <PosterImg path={r.posterPath} alt={r.title} glow />
      <span class="t" title={r.title}>{r.title}</span>
      <span class="dim y">{r.mediaType === "tv" ? "Show" : "Movie"}{r.year ? ` · ${r.year}` : ""}</span>
      {#if r.inLibrary || addedIds.has(`${r.mediaType}-${r.tmdbId}`)}
        <span class="badge">✓ in library</span>
      {:else if r.mediaType === "tv"}
        <div class="row">
          <button class="btn-mini gold" onclick={() => requestAdd(r, "show", "watching")}>Watching</button>
          <button class="btn-mini" onclick={() => requestAdd(r, "show", "plan_to_watch")}>Plan</button>
        </div>
      {:else}
        <div class="row">
          <button class="btn-mini gold" onclick={() => requestAdd(r, "movie", "watched")}>Watched</button>
          <button class="btn-mini" onclick={() => requestAdd(r, "movie", "watchlist")}>Watchlist</button>
        </div>
      {/if}
    </div>
  {/each}
</div>

<style>
  .head { margin-bottom: 20px; }
  .big { width: 100%; max-width: 720px; font-size: 16px; padding: 13px 16px; }
  .results { margin-top: 24px; }
  .cell { display: flex; flex-direction: column; gap: 4px; }
  .t { font-weight: 600; font-size: 14px; margin-top: 8px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .y { font-size: 12.5px; }
  .row { display: flex; gap: 6px; margin-top: 6px; }
  .btn-mini {
    flex: 1; font-size: 12px; font-weight: 600; padding: 6px 8px;
    border-radius: 999px; background: var(--raise-hi); color: var(--dim);
  }
  .btn-mini:hover { color: var(--text); }
  .btn-mini.gold { background: var(--gold); color: #14100a; }
  .badge { margin-top: 6px; align-self: flex-start; }
  @media (max-width: 760px) {
    .big { max-width: none; font-size: 15px; }
  }
</style>
