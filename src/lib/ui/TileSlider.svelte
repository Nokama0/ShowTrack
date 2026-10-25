<script lang="ts">
  import { api } from "#lib/api.js";
  import { session } from "#lib/session.svelte.js";

  let { section }: { section: "shows" | "anime" | "movies" } = $props();
</script>

<div class="tileslider" title="Tile size">
  <span class="dim lbl">Tiles</span>
  <input
    type="range"
    min="80"
    max="260"
    step="10"
    bind:value={session.tiles[section]}
    onchange={() => api.settingsSet(`tile_${section}`, String(session.tiles[section])).catch(() => {})}
    aria-label="Tile size"
  />
</div>

<style>
  .tileslider { display: flex; align-items: center; gap: 8px; }
  .lbl { font-size: 12px; }
  input {
    appearance: none; width: 96px; height: 4px; border-radius: 999px;
    background: var(--raise-hi); outline: none; cursor: pointer; padding: 0;
  }
  input::-webkit-slider-thumb {
    appearance: none; width: 13px; height: 13px; border-radius: 50%;
    background: var(--gold); border: none;
    transition: transform 0.15s ease;
  }
  input::-webkit-slider-thumb:hover { transform: scale(1.2); }
  /* the mobile grid override pins its own column width; hide a dead control */
  @media (max-width: 760px) {
    .tileslider { display: none; }
  }
</style>
