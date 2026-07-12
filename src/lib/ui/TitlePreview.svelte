<script lang="ts">
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { api } from "$lib/api";
  import type { SearchItem, TitlePreview } from "$lib/types";
  import PosterImg from "./PosterImg.svelte";

  let { item, added, onadd, onclose }: {
    item: SearchItem;
    added: boolean;
    onadd: () => void;
    onclose: () => void;
  } = $props();

  let data = $state<TitlePreview | null>(null);
  let error = $state("");

  $effect(() => {
    data = null;
    error = "";
    api.titlePreview(item.tmdbId, item.mediaType)
      .then((d) => (data = d))
      .catch((e) => (error = String(e)));
  });

  $effect(() => {
    document.documentElement.style.overflowY = "hidden";
    return () => {
      document.documentElement.style.overflowY = "";
    };
  });

  const isSheet = () => window.matchMedia("(max-width: 760px)").matches;
  const modalY = () => (isSheet() ? 48 : 12);

  const fmtSpan = (s: { firstAir: string | null; lastAir: string | null }) => {
    if (!s.firstAir) return "dates TBA";
    const y = (d: string) => d.slice(0, 4);
    return s.lastAir && s.lastAir !== s.firstAir
      ? `${y(s.firstAir)} to ${y(s.lastAir)}`
      : y(s.firstAir);
  };
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="overlay" transition:fade={{ duration: 200 }} onclick={onclose} role="presentation">
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div
    class="panel modal"
    in:fly={{ y: modalY(), duration: 300, easing: cubicOut }}
    out:fly={{ y: modalY(), duration: 220, easing: cubicOut }}
    onclick={(e) => e.stopPropagation()}
    role="presentation"
  >
    <div class="mposter"><PosterImg path={item.posterPath} alt={item.title} /></div>
    <div class="mbody">
      <h1 class="mtitle">{item.title}{#if item.year}<span class="dim my"> ({item.year})</span>{/if}</h1>
      {#if error}
        <p style="color:var(--red)">{error}</p>
      {:else if !data}
        <p class="dim">Loading…</p>
      {:else}
        {#if data.mediaType === "movie"}
          {#if data.runtime}<p class="dim">{data.runtime} min</p>{/if}
        {:else if data.status}
          <p class="dim">{data.status}</p>
        {/if}
        {#if data.overview}<p class="dim ov">{data.overview}</p>{/if}
        {#if data.seasons.length > 0}
          <div class="seasons">
            {#each data.seasons as s (s.seasonNumber)}
              <div class="srow">
                <b>{s.seasonNumber === 0 ? "Specials" : `Season ${s.seasonNumber}`}</b>
                <span class="dim">{s.episodeCount} episodes · {fmtSpan(s)}</span>
              </div>
            {/each}
          </div>
        {/if}
      {/if}
      <div class="mactions">
        {#if added}
          <span class="badge">✓ in library</span>
        {:else}
          <button class="btn-gold" onclick={onadd}>+ Add to library</button>
        {/if}
        <button class="btn-ghost" onclick={onclose}>Close</button>
      </div>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed; inset: 0; background: rgba(3, 4, 8, 0.72);
    display: flex; align-items: center; justify-content: center;
    z-index: 40;
    touch-action: none;
  }
  .modal {
    display: flex; gap: 20px; max-width: 660px; width: 92%; padding: 24px;
    max-height: 82vh; overflow-y: auto;
    overscroll-behavior: contain;
  }
  .mposter { width: 190px; flex-shrink: 0; }
  .mbody { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 8px; }
  .mtitle { font-size: 22px; }
  .my { font-family: "Archivo Variable", sans-serif; font-size: 14px; text-transform: none; letter-spacing: 0; margin-left: 8px; }
  .ov { font-size: 13px; }
  .seasons { display: flex; flex-direction: column; gap: 6px; margin: 4px 0; }
  .srow {
    display: flex; justify-content: space-between; gap: 12px; align-items: baseline;
    padding: 7px 12px; border: 1px solid var(--line); border-radius: 10px;
  }
  .srow b { font-size: 13.5px; }
  .srow span { font-size: 12.5px; white-space: nowrap; }
  .mactions { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; margin-top: 4px; }
  @media (max-width: 760px) {
    .overlay { align-items: flex-end; }
    .modal {
      flex-direction: column; width: 100%; max-width: none;
      border-radius: 22px 22px 0 0; max-height: 86vh;
      padding-bottom: calc(24px + env(safe-area-inset-bottom));
    }
    .mposter { width: 140px; align-self: center; }
  }
</style>
