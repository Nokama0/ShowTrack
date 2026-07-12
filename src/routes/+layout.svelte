<script lang="ts">
  import "@fontsource/anton";
  import "@fontsource-variable/archivo";
  import "../app.css";
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { loadSession, session } from "$lib/session.svelte";
  import { api } from "$lib/api";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { isAndroid } from "$lib/platform";
  import Onboarding from "$lib/ui/Onboarding.svelte";
  import NavIcon from "$lib/ui/NavIcon.svelte";
  import DialogHost from "$lib/ui/DialogHost.svelte";

  let { children } = $props();

  const allLinks = [
    { href: "/", label: "Shows", icon: "shows" },
    { href: "/anime", label: "Anime", icon: "anime" },
    { href: "/airing", label: "Airing", icon: "airing" },
    { href: "/movies", label: "Movies", icon: "movies" },
    { href: "/search", label: "Search", icon: "search" },
    { href: "/settings", label: "Settings", icon: "settings" },
  ] as const;
  const links = $derived(
    allLinks.filter(
      (l) =>
        (l.href !== "/anime" || session.features.animePage) &&
        (l.href !== "/airing" || session.features.airingPage) &&
        (l.href !== "/movies" || session.features.movies) &&
        (l.href !== "/search" || session.features.searchPage),
    ),
  );

  let lastAuto = 0;
  const autoSync = async () => {
    if (!isAndroid) return;
    if (Date.now() - lastAuto < 60_000) return;
    lastAuto = Date.now();
    try {
      const st = await api.syncStatus();
      if (!st.paired) return;
      const r = await api.syncNow();
      if (r.pulled > 0) session.refreshTick++;
    } catch (e) {
      // the launch refresh may still hold the busy flag; retry on next foreground
      if (String(e).includes("another job is already running")) {
        lastAuto = 0;
        return;
      }
      // desktop away from home or app closed: quiet note, app fully usable
      session.banner = "Desktop not reachable for sync. Showing local data.";
    }
  };

  onMount(async () => {
    await loadSession();
    if (session.hasKey) {
      try {
        const s = await api.refreshRun();
        if (s.failed.length > 0) {
          session.banner = `Refresh: ${s.refreshed} updated, ${s.failed.length} failed. Showing cached data where needed.`;
        }
        session.refreshTick++;
      } catch {
        // busy or offline; cached data is fine, note it quietly
        session.banner = "Could not refresh airing shows (offline?). Showing cached data.";
      }
    }
    // after the refresh so the two never fight over the busy flag
    autoSync();
    if (!isAndroid) {
      // quiet launch check; no releases yet or offline is not worth a banner
      api.updateCheck().then((u) => {
        if (u) session.banner = `ShowTrack ${u.version} is available. Install it from Settings.`;
      }).catch(() => {});
    }
  });

  onMount(() => {
    const onVis = () => { if (document.visibilityState === "visible") autoSync(); };
    document.addEventListener("visibilitychange", onVis);
    return () => document.removeEventListener("visibilitychange", onVis);
  });

  $effect(() => {
    if (isAndroid) return;
    // cosmetic: ignore failures, app stays at current zoom
    getCurrentWebview().setZoom(session.zoom / 100).catch(() => {});
  });
</script>

<div class="shell">
  <nav class="dock" aria-label="Sections">
    <div class="mark"><img src="/appicon.png" alt="ShowTrack" /></div>
    {#each links as l (l.href)}
      <a href={l.href} class:on={page.url.pathname === l.href}>
        <span class="ico"><NavIcon name={l.icon} size={19} /></span>{l.label}
      </a>
    {/each}
  </nav>
  <nav class="tabbar" class:solid={isAndroid} aria-label="Sections, mobile">
    {#each links as l (l.href)}
      <a href={l.href} class:on={page.url.pathname === l.href}>
        <span class="ico"><NavIcon name={l.icon} size={22} /></span>{l.label}
      </a>
    {/each}
  </nav>
  <main>
    {#if session.banner}
      <div class="banner">
        <span>{session.banner}</span>
        <button onclick={() => (session.banner = "")}>✕</button>
      </div>
    {/if}
    {#if session.ready}
      {#if session.hasKey}
        {#key page.url.pathname}
          <div class="page-enter">
            {@render children()}
          </div>
        {/key}
      {:else}
        <Onboarding />
      {/if}
    {/if}
  </main>
</div>
<DialogHost />

<style>
  .shell { display: flex; min-height: 100vh; }
  .dock {
    width: 88px; flex-shrink: 0; padding: 22px 12px;
    display: flex; flex-direction: column; align-items: center; gap: 8px;
    border-right: 1px solid var(--line); position: sticky; top: 0; height: 100vh;
  }
  .mark {
    width: 44px; height: 44px; border-radius: 13px; margin-bottom: 18px;
    border: 1px solid var(--line); overflow: hidden;
  }
  .mark img { width: 100%; height: 100%; display: block; }
  .dock a {
    width: 60px; padding: 9px 0 7px; border-radius: 12px; text-align: center;
    color: var(--dim); font-size: 10.5px; font-weight: 600; letter-spacing: 0.02em;
    transition: background-color 0.18s ease, color 0.18s ease, box-shadow 0.18s ease, transform 0.12s ease;
  }
  .dock a .ico { display: flex; justify-content: center; margin-bottom: 4px; opacity: 0.9; }
  .dock a:hover { background: var(--raise); color: var(--text); }
  .dock a:active { transform: scale(0.94); }
  .dock a.on { background: var(--raise-hi); color: var(--text); box-shadow: inset 0 0 0 1px var(--gold); }
  main { flex: 1; min-width: 0; padding: var(--main-pad-y) var(--main-pad-x) 64px; }
  .tabbar {
    display: none; position: fixed; left: 0; right: 0; bottom: 0; z-index: 30;
    background: var(--raise);
    background: color-mix(in srgb, var(--raise) 92%, transparent);
    backdrop-filter: blur(14px);
    border-top: 1px solid var(--line);
    padding: 6px 4px calc(6px + env(safe-area-inset-bottom));
  }
  /* Android WebView: backdrop-filter on a fixed bar composites unreliably over
     animating content (bar vanishes or paints without its children); go solid */
  .tabbar.solid { background: var(--raise); backdrop-filter: none; }
  .tabbar a {
    flex: 1; text-align: center; padding: 6px 0 4px; border-radius: 10px;
    color: var(--dim); font-size: 10px; font-weight: 600;
    transition: color 0.18s ease, background-color 0.18s ease, transform 0.12s ease;
  }
  .tabbar a .ico { display: flex; justify-content: center; margin-bottom: 3px; }
  .tabbar a:active { transform: scale(0.92); }
  .tabbar a.on { color: var(--gold); }
  @media (max-width: 760px) {
    .dock { display: none; }
    .tabbar { display: flex; }
    main {
      --main-pad-y: 18px; --main-pad-x: 14px;
      padding-bottom: calc(84px + env(safe-area-inset-bottom));
    }
  }
</style>
