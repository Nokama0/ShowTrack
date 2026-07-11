<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { api } from "$lib/api";
  import { session } from "$lib/session.svelte";
  import BingeBoard from "$lib/ui/BingeBoard.svelte";

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

  onMount(async () => {
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

<style>
  .scannote {
    display: flex; align-items: center; gap: 12px; flex-wrap: wrap;
    margin-bottom: 22px; padding: 14px 18px;
  }
</style>
