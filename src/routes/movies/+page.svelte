<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { confirm } from "@tauri-apps/plugin-dialog";
  import { api } from "$lib/api";
  import { session } from "$lib/session.svelte";
  import type { MovieRow } from "$lib/types";
  import PosterImg from "$lib/ui/PosterImg.svelte";
  import Select from "$lib/ui/Select.svelte";

  $effect(() => {
    if (!session.features.movies) goto("/", { replaceState: true });
  });

  // bottom sheet slides further on mobile than the centered desktop modal
  const isSheet = () => window.matchMedia("(max-width: 760px)").matches;
  const modalY = () => (isSheet() ? 48 : 12);

  // the page behind must not scroll while the sheet is open
  $effect(() => {
    document.documentElement.style.overflowY = selected ? "hidden" : "";
    return () => {
      document.documentElement.style.overflowY = "";
    };
  });

  // pull the sheet down past the threshold to dismiss it
  let sheetEl = $state<HTMLDivElement | null>(null);
  let dragY = $state(0);
  let armed = false; // only a gesture that STARTS with the sheet at its top can dismiss
  let startY = 0;

  function sheetTouchStart(e: TouchEvent) {
    if (!isSheet() || !sheetEl) return;
    armed = sheetEl.scrollTop <= 0;
    startY = e.touches[0].clientY;
  }

  function sheetTouchMove(e: TouchEvent) {
    if (!armed || !sheetEl) return;
    const dy = e.touches[0].clientY - startY;
    if (sheetEl.scrollTop > 0 || dy <= 0) {
      dragY = 0;
      return;
    }
    e.preventDefault(); // the pull is ours, not the scroller's
    dragY = dy;
  }

  function sheetTouchEnd() {
    armed = false;
    if (dragY > 90) selected = null;
    dragY = 0;
  }

  let movies = $state<MovieRow[]>([]);
  let loaded = $state(false);
  let loadError = $state("");
  let tab = $state<"watched" | "watchlist" | "all">("watched");
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

  const visible = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const list = movies.filter(
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

  async function setState(m: MovieRow, myState: "watched" | "watchlist") {
    const watchedAt = myState === "watched" ? (m.watchedAt ?? new Date().toISOString()) : null;
    replace({ ...m, myState, watchedAt, rewatchCount: myState === "watched" ? m.rewatchCount : 0 });
    await api.movieSetState(m.tmdbId, myState);
  }

  async function setRewatches(m: MovieRow, count: number) {
    try {
      replace({ ...m, rewatchCount: await api.movieSetRewatch(m.tmdbId, count) });
    } catch (e) {
      session.banner = String(e);
    }
  }

  async function resetRewatches(m: MovieRow) {
    if (!(await confirm(`Reset the rewatch count for ${m.title}?`, { title: "ShowTrack" }))) return;
    await setRewatches(m, 0);
  }

  async function removeMovie(m: MovieRow) {
    const ok = await confirm(
      `Remove ${m.title} from your library? Its watch history goes with it.`,
      { title: "ShowTrack", kind: "warning" },
    );
    if (!ok) return;
    try {
      await api.movieRemove(m.tmdbId);
      movies = movies.filter((x) => x.tmdbId !== m.tmdbId);
      selected = null;
    } catch (e) {
      session.banner = String(e);
    }
  }

  async function toggleFav(m: MovieRow) {
    replace({ ...m, isFavorite: await api.movieToggleFavorite(m.tmdbId) });
  }
</script>

<div class="head">
  <h1>Movies</h1>
  <div class="stats">
    <div class="stat g"><b>{movies.filter((m) => m.myState === "watched").length}</b><span>watched</span></div>
    <div class="stat"><b>{movies.filter((m) => m.myState === "watchlist").length}</b><span>watchlist</span></div>
  </div>
</div>

<div class="toolbar">
  <div class="chips">
    {#each tabs as [value, label] (value)}
      <button class="chip" class:on={tab === value} onclick={() => (tab = value)}>{label}</button>
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
</div>

{#if loadError}
  <p style="color:var(--red)">{loadError}</p>
{:else if !loaded}
  <p class="dim">Loading…</p>
{:else if visible.length === 0}
  <p class="dim">Nothing here yet. Add movies via Search.</p>
{:else}
  <div class="grid-posters">
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
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="overlay" transition:fade={{ duration: 200 }} onclick={() => (selected = null)} role="presentation">
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div
      class="panel modal"
      bind:this={sheetEl}
      in:fly={{ y: modalY(), duration: 300, easing: cubicOut }}
      out:fly={{ y: modalY(), duration: 220, easing: cubicOut }}
      style={dragY > 0 ? `transform: translateY(${Math.round(dragY * 0.55)}px); transition: none;` : ""}
      ontouchstart={sheetTouchStart}
      ontouchmove={sheetTouchMove}
      ontouchend={sheetTouchEnd}
      onclick={(e) => e.stopPropagation()}
      role="presentation"
    >
      <div class="mposter"><PosterImg path={selected.posterPath} alt={selected.title} /></div>
      <div class="mbody">
        <h1 class="mtitle">{selected.title}{#if selected.year}<span class="dim my"> ({selected.year})</span>{/if}</h1>
        {#if selected.runtime}<p class="dim">{selected.runtime} min</p>{/if}
        {#if selected.overview}<p class="dim ov">{selected.overview}</p>{/if}
        {#if selected.myState === "watched"}
          <p>Watched {selected.watchedAt ? selected.watchedAt.slice(0, 10) : ""}</p>
          <div class="mactions rewatches">
            <span class="dim">Rewatches</span>
            <button class="rwbtn" aria-label="One rewatch fewer"
                    onclick={() => setRewatches(selected!, selected!.rewatchCount - 1)}
                    disabled={selected.rewatchCount <= 0}>−</button>
            <b class="rwcount">{selected.rewatchCount}</b>
            <button class="rwbtn" aria-label="One more rewatch"
                    onclick={() => setRewatches(selected!, selected!.rewatchCount + 1)}>+</button>
            {#if selected.rewatchCount > 0}
              <button class="btn-ghost rwreset" onclick={() => resetRewatches(selected!)}>Reset</button>
            {/if}
          </div>
          <div class="mactions">
            <button class="btn-ghost" onclick={() => setState(selected!, "watchlist")}>Move to watchlist</button>
          </div>
        {:else}
          <div class="mactions">
            <button class="btn-gold" onclick={() => setState(selected!, "watched")}>✓ Watched now</button>
          </div>
        {/if}
        <div class="mactions">
          <button class="btn-ghost" class:favon={selected.isFavorite} onclick={() => toggleFav(selected!)}>
            {selected.isFavorite ? "★ Favorite" : "☆ Favorite"}
          </button>
          <button class="btn-ghost removebtn" onclick={() => removeMovie(selected!)}>Remove</button>
          <button class="btn-ghost" onclick={() => (selected = null)}>Close</button>
        </div>
      </div>
    </div>
  </div>
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
  .overlay {
    position: fixed; inset: 0; background: rgba(3, 4, 8, 0.72);
    display: flex; align-items: center; justify-content: center;
    z-index: 40; /* above the mobile tab bar (30) */
    touch-action: none; /* dragging the backdrop must not scroll the page */
  }
  .modal {
    display: flex; gap: 20px; max-width: 660px; width: 92%; padding: 24px;
    overscroll-behavior: contain; /* sheet scroll never chains to the page */
    touch-action: pan-y;
    transition: transform 0.25s cubic-bezier(0.2, 0.7, 0.3, 1); /* snap back after a short pull */
  }
  .mposter { width: 190px; flex-shrink: 0; }
  .mbody { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 8px; }
  .mtitle { font-size: 22px; }
  .my { font-family: "Archivo Variable", sans-serif; font-size: 14px; text-transform: none; letter-spacing: 0; margin-left: 8px; }
  .ov { font-size: 13px; }
  .mactions { display: flex; gap: 8px; flex-wrap: wrap; }
  .favon { color: var(--gold); border-color: var(--gold); }
  .rewatches { align-items: center; }
  .rwbtn { padding: 4px 13px; font-size: 15px; border-radius: 999px; }
  .rwcount { min-width: 22px; text-align: center; }
  .rwreset { padding: 6px 14px; font-size: 12.5px; }
  .removebtn { color: var(--red); border-color: rgba(255, 84, 112, 0.35); }
  .removebtn:hover { border-color: var(--red); color: var(--red); }
  @media (max-width: 760px) {
    .toolbar input { margin-left: 0; flex: 1; }
    .overlay { align-items: flex-end; }
    .modal {
      flex-direction: column; width: 100%; max-width: none;
      border-radius: 22px 22px 0 0; max-height: 86vh; overflow-y: auto;
      padding-bottom: calc(24px + env(safe-area-inset-bottom));
    }
    .mposter { width: 140px; align-self: center; }
  }
</style>
