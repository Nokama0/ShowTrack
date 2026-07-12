<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { api } from "$lib/api";
  import { session } from "$lib/session.svelte";
  import type { MovieRow } from "$lib/types";
  import BingeBoard from "$lib/ui/BingeBoard.svelte";
  import PosterImg from "$lib/ui/PosterImg.svelte";
  import MovieSheet from "$lib/ui/MovieSheet.svelte";

  $effect(() => {
    if (!session.features.animePage) goto("/", { replaceState: true });
  });

  // one-time backfill: existing libraries predate the anime flag
  let scan = $state<"checking" | "running" | "failed" | "done">("checking");
  let scanError = $state("");

  async function runScan() {
    scan = "running";
    scanError = "";
    for (let attempt = 0; attempt < 20; attempt++) {
      try {
        await api.animeScan();
        scan = "done";
        session.refreshTick++;
        return;
      } catch (e) {
        // the launch refresh or a sync may hold the busy flag; wait it out
        if (String(e).includes("another job is already running")) {
          await new Promise((r) => setTimeout(r, 3000));
          continue;
        }
        scanError = String(e);
        scan = "failed";
        return;
      }
    }
    scanError = "The app is busy. Try again in a moment.";
    scan = "failed";
  }

  // anime movies: movie rows the user filed with the anime
  let movies = $state<MovieRow[]>([]);
  let selected = $state<MovieRow | null>(null);
  const animeMovies = $derived(movies.filter((m) => m.isAnime));

  function replace(updated: MovieRow) {
    movies = movies.map((m) => (m.tmdbId === updated.tmdbId ? updated : m));
    if (selected?.tmdbId === updated.tmdbId) selected = updated;
  }

  function removed(id: number) {
    movies = movies.filter((x) => x.tmdbId !== id);
    selected = null;
  }

  onMount(async () => {
    api.moviesList().then((m) => (movies = m)).catch(() => {});
    try {
      const s = await api.settingsAll();
      if (s["anime_scan_done"] === "1") {
        scan = "done";
      } else {
        await runScan();
      }
    } catch (e) {
      scanError = String(e);
      scan = "failed";
    }
  });
</script>

{#if scan === "running"}
  <div class="panel scannote rise">
    <b>Finding the anime in your library.</b>
    <span class="dim">Each show gets checked once on TMDB. This runs a single time and can take a minute.</span>
  </div>
{:else if scan === "failed"}
  <div class="panel scannote rise">
    <span style="color:var(--red)">{scanError}</span>
    <button class="btn-ghost" onclick={runScan}>Try again</button>
  </div>
{/if}

<BingeBoard animeOnly />

{#if animeMovies.length > 0 && session.features.movies}
  <section class="amovies">
    <h2>Anime <b>movies</b><small>filed here from Search; they keep their movie controls</small></h2>
    <div class="grid-posters">
      {#each animeMovies as m, i (m.tmdbId)}
        <button class="cell rise" style={`animation-delay:${Math.min(i, 14) * 35}ms`} onclick={() => (selected = m)}>
          <PosterImg path={m.posterPath} alt={m.title} glow />
          <span class="t" title={m.title}>{m.title}</span>
          <span class="dim y">{m.year ?? ""} · {m.myState === "watched" ? "Watched" : "Will watch"}{m.isFavorite ? " ★" : ""}</span>
        </button>
      {/each}
    </div>
  </section>
{/if}

{#if selected}
  <MovieSheet movie={selected} onreplace={replace} onremove={removed} onclose={() => (selected = null)} />
{/if}

<style>
  .scannote {
    display: flex; align-items: center; gap: 12px; flex-wrap: wrap;
    margin-bottom: 22px; padding: 14px 18px;
  }
  .amovies { margin-top: 44px; }
  .amovies h2 { margin: 0 0 18px; }
  .cell { background: none; padding: 0; text-align: left; display: flex; flex-direction: column; gap: 4px; border-radius: var(--r-lg); }
  .cell:hover { filter: none; }
  .t { font-weight: 600; font-size: 14px; margin-top: 8px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .y { font-size: 12.5px; }
</style>
