<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { api } from "$lib/api";
  import { session } from "$lib/session.svelte";
  import type { MovieRow } from "$lib/types";
  import PosterImg from "$lib/ui/PosterImg.svelte";
  import Select from "$lib/ui/Select.svelte";
  import MovieSheet from "$lib/ui/MovieSheet.svelte";
  import TileSlider from "$lib/ui/TileSlider.svelte";

  $effect(() => {
    if (!session.features.movies) goto("/", { replaceState: true });
  });

  let movies = $state<MovieRow[]>([]);
  let loaded = $state(false);
  let loadError = $state("");
  type Tab = "watched" | "watchlist" | "all";
  let tab = $state<Tab>(session.tabs.movies as Tab);

  function setTab(value: Tab) {
    tab = value;
    session.tabs.movies = value;
    api.settingsSet("tab_movies", value).catch(() => {}); // cosmetic preference, losing it is fine
  }
  const tabDefs = [
    ["watched", "Watched"],
    ["watchlist", "Watchlist"],
    ["all", "All movies"],
  ] as const;
  const tabs = $derived(
    session.allFirst ? [tabDefs[2], tabDefs[0], tabDefs[1]] : [...tabDefs],
  );
  let sort = $state<"watchedAt" | "title" | "year">("watchedAt");
  let query = $state("");
  let selected = $state<MovieRow | null>(null);

  onMount(async () => {
    try {
      movies = await api.moviesList();
      loadError = "";
    } catch (e) {
      loadError = String(e);
    } finally {
      loaded = true;
    }
  });

  // anime movies live on the Anime page while that tab is on
  const pool = $derived(movies.filter((m) => !(m.isAnime && session.features.animePage)));

  const visible = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const list = pool.filter(
      (m) => (tab === "all" || m.myState === tab) && (q === "" || m.title.toLowerCase().includes(q)),
    );
    const cmp: Record<string, (a: MovieRow, b: MovieRow) => number> = {
      watchedAt: (a, b) => (b.watchedAt ?? "").localeCompare(a.watchedAt ?? ""),
      title: (a, b) => a.title.localeCompare(b.title),
      year: (a, b) => (b.year ?? 0) - (a.year ?? 0),
    };
    return list.slice().sort(cmp[sort]);
  });

  // render in chunks so a 550-movie library does not shrink the scrollbar
  // to a sliver; more load as the sentinel scrolls near the viewport
  const pageSize = () => (window.matchMedia("(max-width: 760px)").matches ? 30 : 50);
  let limit = $state(pageSize());
  let sentinel = $state<HTMLDivElement | null>(null);

  $effect(() => {
    void tab; void query; void sort;
    limit = pageSize(); // any filter change restarts from the first chunk
  });

  $effect(() => {
    if (!sentinel) return;
    const ob = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) limit += pageSize();
      },
      { rootMargin: "600px" },
    );
    ob.observe(sentinel);
    return () => ob.disconnect();
  });

  function replace(updated: MovieRow) {
    movies = movies.map((m) => (m.tmdbId === updated.tmdbId ? updated : m));
    if (selected?.tmdbId === updated.tmdbId) selected = updated;
  }

  function removed(id: number) {
    movies = movies.filter((x) => x.tmdbId !== id);
    selected = null;
  }
</script>

<div class="head">
  <h1>Movies</h1>
  <div class="stats">
    <div class="stat g"><b>{pool.filter((m) => m.myState === "watched").length}</b><span>watched</span></div>
    <div class="stat"><b>{pool.filter((m) => m.myState === "watchlist").length}</b><span>watchlist</span></div>
  </div>
</div>

<div class="toolbar">
  <div class="chips">
    {#each tabs as [value, label] (value)}
      <button class="chip" class:on={tab === value} onclick={() => setTab(value)}>{label}</button>
    {/each}
  </div>
  <Select
    value={sort}
    label="Sort movies"
    options={[
      { value: "watchedAt", label: "By date watched" },
      { value: "title", label: "By title" },
      { value: "year", label: "By year" },
    ]}
    onchange={(v) => (sort = v as "watchedAt" | "title" | "year")}
  />
  <input placeholder="Filter…" bind:value={query} />
  <TileSlider section="movies" />
</div>

{#if loadError}
  <p style="color:var(--red)">{loadError}</p>
{:else if !loaded}
  <p class="dim">Loading…</p>
{:else if visible.length === 0}
  <p class="dim">Nothing here yet. Add movies via Search.</p>
{:else}
  <div class="grid-posters" style={`--tile:${session.tiles.movies}px`}>
    {#each visible.slice(0, limit) as m, i (m.tmdbId)}
      <button class="cell rise" style={`animation-delay:${Math.min(i, 14) * 35}ms`} onclick={() => (selected = m)}>
        <PosterImg path={m.posterPath} alt={m.title} glow />
        <span class="t" title={m.title}>{m.title}</span>
        <span class="dim y">{m.year ?? ""}{m.isFavorite ? " ★" : ""}</span>
      </button>
    {/each}
  </div>
  {#if visible.length > limit}
    <div class="sentinel" bind:this={sentinel} aria-hidden="true"></div>
  {/if}
{/if}

{#if selected}
  <MovieSheet movie={selected} onreplace={replace} onremove={removed} onclose={() => (selected = null)} />
{/if}

<style>
  .head { display: flex; align-items: flex-end; justify-content: space-between; gap: 18px; flex-wrap: wrap; margin-bottom: 20px; }
  .toolbar { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; margin-bottom: 26px; }
  .toolbar input { margin-left: auto; }
  .cell { background: none; padding: 0; text-align: left; display: flex; flex-direction: column; gap: 4px; border-radius: var(--r-lg); }
  .sentinel { height: 1px; }
  .cell:hover { filter: none; }
  .t { font-weight: 600; font-size: 14px; margin-top: 8px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .y { font-size: 12.5px; }
  @media (max-width: 760px) {
    .toolbar input { margin-left: 0; flex: 1; }
  }
</style>
