<script lang="ts">
  let {
    name,
    size = 22,
  }: {
    name: "shows" | "anime" | "airing" | "movies" | "search" | "settings";
    size?: number;
  } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.8"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#if name === "shows"}
    <path d="M8 5.5v13l11-6.5z" fill="currentColor" stroke="none" />
  {:else if name === "anime"}
    <!-- torii gate -->
    <path d="M3.5 6.5c5.5-1.6 11.5-1.6 17 0M6.5 5.9V19.5M17.5 5.9V19.5M4.5 10.5h15" />
  {:else if name === "airing"}
    <circle cx="12" cy="12" r="8.5" />
    <path d="M12 7.5V12l3 2" />
  {:else if name === "movies"}
    <rect x="4" y="5" width="16" height="14" rx="2.5" />
    <path d="M4 9.5h16M8.5 5v4.5M15.5 5v4.5" />
  {:else if name === "search"}
    <circle cx="11" cy="11" r="6.5" />
    <path d="M15.8 15.8L20 20" />
  {:else}
    <path
      d="M12 3.5l1.8 6.7 6.7 1.8-6.7 1.8-1.8 6.7-1.8-6.7-6.7-1.8 6.7-1.8z"
      fill="currentColor"
      stroke="none"
    />
  {/if}
</svg>

<style>
  svg { display: block; }
</style>
