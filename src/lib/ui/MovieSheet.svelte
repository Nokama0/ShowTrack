<script lang="ts">
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { appConfirm } from "#lib/dialog.svelte.js";
  import { api } from "#lib/api.js";
  import { session } from "#lib/session.svelte.js";
  import type { MovieRow } from "#lib/types.js";
  import PosterImg from "./PosterImg.svelte";

  let { movie, onreplace, onremove, onclose }: {
    movie: MovieRow;
    onreplace: (m: MovieRow) => void;
    onremove: (id: number) => void;
    onclose: () => void;
  } = $props();

  // bottom sheet slides further on mobile than the centered desktop modal
  const isSheet = () => window.matchMedia("(max-width: 760px)").matches;
  const modalY = () => (isSheet() ? 48 : 12);

  // the page behind must not scroll while the sheet is open
  $effect(() => {
    document.documentElement.style.overflowY = "hidden";
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
    if (dragY > 90) onclose();
    dragY = 0;
  }

  async function setState(myState: "watched" | "watchlist") {
    const watchedAt = myState === "watched" ? (movie.watchedAt ?? new Date().toISOString()) : null;
    onreplace({ ...movie, myState, watchedAt, rewatchCount: myState === "watched" ? movie.rewatchCount : 0 });
    await api.movieSetState(movie.tmdbId, myState);
  }

  async function setRewatches(count: number) {
    try {
      onreplace({ ...movie, rewatchCount: await api.movieSetRewatch(movie.tmdbId, count) });
    } catch (e) {
      session.banner = String(e);
    }
  }

  async function resetRewatches() {
    const ok = await appConfirm({
      title: "Reset rewatches",
      message: `Reset the rewatch count for ${movie.title}?`,
      confirmLabel: "Reset",
    });
    if (!ok) return;
    await setRewatches(0);
  }

  async function removeMovie() {
    const ok = await appConfirm({
      title: "Remove movie",
      message: `Remove ${movie.title} from your library? Its watch history goes with it.`,
      confirmLabel: "Remove",
      danger: true,
    });
    if (!ok) return;
    try {
      await api.movieRemove(movie.tmdbId);
      onremove(movie.tmdbId);
    } catch (e) {
      session.banner = String(e);
    }
  }

  async function toggleFav() {
    onreplace({ ...movie, isFavorite: await api.movieToggleFavorite(movie.tmdbId) });
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="overlay" transition:fade={{ duration: 200 }} onclick={onclose} role="presentation">
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
    <div class="mposter"><PosterImg path={movie.posterPath} alt={movie.title} /></div>
    <div class="mbody">
      <h1 class="mtitle">{movie.title}{#if movie.year}<span class="dim my"> ({movie.year})</span>{/if}</h1>
      {#if movie.runtime}<p class="dim">{movie.runtime} min</p>{/if}
      {#if movie.overview}<p class="dim ov">{movie.overview}</p>{/if}
      {#if movie.myState === "watched"}
        <p>Watched {movie.watchedAt ? movie.watchedAt.slice(0, 10) : ""}</p>
        <div class="mactions rewatches">
          <span class="dim">Rewatches</span>
          <button class="rwbtn" aria-label="One rewatch fewer"
                  onclick={() => setRewatches(movie.rewatchCount - 1)}
                  disabled={movie.rewatchCount <= 0}>−</button>
          <b class="rwcount">{movie.rewatchCount}</b>
          <button class="rwbtn" aria-label="One more rewatch"
                  onclick={() => setRewatches(movie.rewatchCount + 1)}>+</button>
          {#if movie.rewatchCount > 0}
            <button class="btn-ghost rwreset" onclick={resetRewatches}>Reset</button>
          {/if}
        </div>
        <div class="mactions">
          <button class="btn-ghost" onclick={() => setState("watchlist")}>Move to watchlist</button>
        </div>
      {:else}
        <div class="mactions">
          <button class="btn-gold" onclick={() => setState("watched")}>✓ Watched now</button>
        </div>
      {/if}
      <div class="mactions">
        <button class="btn-ghost" class:favon={movie.isFavorite} onclick={toggleFav}>
          {movie.isFavorite ? "★ Favorite" : "☆ Favorite"}
        </button>
        <button class="btn-ghost removebtn" onclick={removeMovie}>Remove</button>
        <button class="btn-ghost" onclick={onclose}>Close</button>
      </div>
    </div>
  </div>
</div>

<style>
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
    .overlay { align-items: flex-end; }
    .modal {
      flex-direction: column; width: 100%; max-width: none;
      border-radius: 22px 22px 0 0; max-height: 86vh; overflow-y: auto;
      padding-bottom: calc(24px + env(safe-area-inset-bottom));
    }
    .mposter { width: 140px; align-self: center; }
  }
</style>
