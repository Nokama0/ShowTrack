<script lang="ts">
  import { goto } from "$app/navigation";
  import { api } from "$lib/api";
  import { session } from "$lib/session.svelte";
  import { airingLabel, fmtDate, fmtMonth } from "$lib/format";
  import type { AiringRow } from "$lib/types";
  import PosterImg from "$lib/ui/PosterImg.svelte";

  let rows = $state<AiringRow[]>([]);
  let loaded = $state(false);
  let loadError = $state("");

  $effect(() => {
    if (!session.features.airingPage) goto("/", { replaceState: true });
  });

  $effect(() => {
    void session.refreshTick;
    (async () => {
      try {
        rows = await api.airingList();
        loadError = "";
      } catch (e) {
        loadError = String(e);
      } finally {
        loaded = true;
      }
    })();
  });

  const groups = $derived.by(() => {
    const m = new Map<string, AiringRow[]>();
    for (const r of rows) {
      const key = r.finaleDate ? fmtMonth(r.finaleDate) : "Finale TBA";
      const list = m.get(key) ?? [];
      list.push(r);
      m.set(key, list);
    }
    return [...m.entries()]; // backend sort: finale-known first, ascending
  });
</script>

<div class="head">
  <h1>Airing <span>now</span></h1>
</div>
<p class="dim intro">Seasons still filling up. Each one moves to Ready the day its finale airs.</p>

{#if loadError}
  <p style="color:var(--red)">{loadError}</p>
{:else if !loaded}
  <p class="dim">Loading…</p>
{:else if groups.length === 0}
  <p class="dim">Nothing airing for the shows you follow. New seasons appear here automatically after a refresh.</p>
{:else}
  {#each groups as [month, items] (month)}
    {#if month === "Finale TBA"}
      <h2>Finale <b>TBA</b></h2>
    {:else}
      <h2>Watchable <b>{month}</b></h2>
    {/if}
    <div class="airgrid">
      {#each items as r, i (r.showTmdbId)}
        <a class="air panel rise" style={`animation-delay:${Math.min(i, 10) * 45}ms`} href={`/show/${r.showTmdbId}`}>
          <div class="mini"><PosterImg path={r.posterPath} size="w185" alt={r.showName} /></div>
          <div class="body">
            <div class="top">
              <b>{r.showName}</b>
              <span class="when">{airingLabel(r).toUpperCase()}</span>
            </div>
            <div class="sub dim">Season {r.seasonNumber} · {r.airedCount} of {r.totalCount} episodes aired</div>
            <div class="bar"><i style={`width:${r.totalCount > 0 ? (r.airedCount / r.totalCount) * 100 : 0}%`}></i></div>
            <div class="cap">
              {#if r.nextAirDate}next episode {fmtDate(r.nextAirDate)}{/if}
              {#if r.nextAirDate && r.finaleDate}&nbsp;·&nbsp;{/if}
              {#if r.finaleDate}finale {fmtDate(r.finaleDate)}{/if}
            </div>
          </div>
        </a>
      {/each}
    </div>
  {/each}
{/if}

<style>
  .head { margin-bottom: 8px; }
  .intro { margin: 0 0 10px; }
  .airgrid { display: grid; grid-template-columns: repeat(auto-fill, minmax(360px, 1fr)); gap: 14px; }
  /* min-width overrides the grid item's auto minimum: without it the nowrap
     labels force the card wider than the track and the whole page overflows
     sideways (which unanchors the fixed tab bar on Android) */
  .air { display: flex; gap: 16px; min-width: 0; overflow: hidden; }
  .air .when { flex-shrink: 0; }
  .air:hover { border-color: var(--faint); transform: translateY(-2px); }
  .mini { width: 56px; flex-shrink: 0; }
  .body { flex: 1; min-width: 0; }
  .top { display: flex; justify-content: space-between; align-items: baseline; gap: 10px; }
  .top b { font-weight: 600; font-size: 15px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .when { font-family: "Anton", sans-serif; font-size: 12px; letter-spacing: 0.08em; color: var(--gold); white-space: nowrap; }
  .sub { font-size: 12.5px; margin: 3px 0 12px; }
  .cap { font-size: 11px; color: var(--faint); margin-top: 7px; }
  @media (max-width: 760px) {
    .airgrid { grid-template-columns: 1fr; }
  }
</style>
