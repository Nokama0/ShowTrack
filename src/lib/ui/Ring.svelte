<script lang="ts">
  let {
    pct,
    label,
    onclick,
  }: { pct: number; label: string; onclick: () => void } = $props();
  const p = $derived(Math.min(100, Math.max(0, pct)));
  const C = 2 * Math.PI * 24; // circumference of the r=24 track
</script>

<button class="ring" aria-label={label} {onclick}>
  <svg viewBox="0 0 54 54" aria-hidden="true">
    <circle class="track" cx="27" cy="27" r="24" />
    {#if p > 0}
      <circle
        class="arc"
        cx="27"
        cy="27"
        r="24"
        stroke-dasharray={C}
        stroke-dashoffset={C * (1 - p / 100)}
      />
    {/if}
  </svg>
  <b>✓</b>
</button>

<style>
  .ring {
    position: absolute; top: -9px; right: -7px; z-index: 2;
    width: 54px; height: 54px; border-radius: 50%; padding: 0;
    background: var(--raise-hi);
    display: flex; align-items: center; justify-content: center;
    box-shadow: 0 8px 20px rgba(0, 0, 0, 0.5);
    transition: transform 0.15s ease;
    animation: fadein 0.4s ease both;
  }
  svg {
    position: absolute; inset: 0; width: 100%; height: 100%;
    transform: rotate(-90deg); /* arc starts at 12 o'clock */
  }
  circle { fill: none; stroke-width: 6; }
  .track { stroke: var(--line); }
  .arc {
    stroke: var(--gold); stroke-linecap: round;
    transition: stroke-dashoffset 0.5s cubic-bezier(0.2, 0.7, 0.3, 1);
  }
  .ring b {
    font-size: 18px; color: var(--gold); font-weight: 700;
    transform: translateY(-1px); position: relative;
  }
  .ring:hover { transform: scale(1.08); filter: none; }
  @media (max-width: 760px) {
    .ring { width: 48px; height: 48px; top: -8px; right: -6px; }
    .ring b { font-size: 16px; }
  }
</style>
