<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { api } from "#lib/api.js";
  import { session, type Features } from "#lib/session.svelte.js";
  import { changelogFor } from "#lib/changelog.js";
  import { isAndroid } from "#lib/platform.js";
  import { getVersion } from "@tauri-apps/api/app";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import type { CacheInfo, ImportProgress, ImportReport, RefreshSummary, SyncServerStatus, SyncStatus, UpdateInfo } from "#lib/types.js";

  let key = $state("");
  let keyStatus = $state("");
  let importing = $state(false);
  let progress = $state<ImportProgress | null>(null);
  let report = $state<ImportReport | null>(null);
  let refreshing = $state(false);
  let refreshResult = $state<RefreshSummary | null>(null);
  let cache = $state<CacheInfo | null>(null);
  let importError = $state("");

  let sync = $state<SyncServerStatus | null>(null);
  let syncPort = $state(47810);
  let pairCode = $state("");
  let syncMsg = $state("");

  let phoneSync = $state<SyncStatus | null>(null);
  let peerInput = $state("");
  let codeInput = $state("");
  let pairing = $state(false);
  let syncing = $state(false);
  let unpairing = $state(false);
  let phoneSyncMsg = $state("");

  let backupBusy = $state(false);
  let backupMsg = $state("");
  let startMax = $state(true);

  let appVersion = $state("");
  let upd = $state<UpdateInfo | null>(null);
  let updMsg = $state("");
  let updChecking = $state(false);
  let updInstalling = $state(false);

  async function checkUpdates() {
    updChecking = true;
    updMsg = "";
    upd = null;
    try {
      upd = await api.updateCheck();
      if (!upd) updMsg = "You are on the latest version.";
    } catch (e) {
      updMsg = String(e);
    } finally {
      updChecking = false;
    }
  }

  async function installUpdate() {
    if (!upd?.installerUrl) return;
    updInstalling = true;
    updMsg = "";
    try {
      await api.updateInstall(upd.installerUrl); // downloads, hands over to the new version, exits
    } catch (e) {
      updMsg = String(e);
      updInstalling = false;
    }
  }

  function openWhatsNew() {
    session.whatsNew = changelogFor(appVersion);
  }

  async function togglePref(key: string, field: "askAddType" | "allFirst" | "episodeMode" | "navReorder" | "navSeparators" | "keepSearch", on: boolean) {
    session[field] = on;
    try {
      await api.settingsSet(key, on ? "1" : "0");
    } catch (e) {
      session.banner = String(e);
    }
  }

  const featureDefs: { key: keyof Features; setting: string; label: string; hint: string }[] = [
    { key: "movies", setting: "feat_movies", label: "Movie tracking", hint: "the Movies page and movie results in Search" },
    { key: "animePage", setting: "feat_anime_page", label: "Anime page", hint: "your anime on its own season-first tab" },
    { key: "airingPage", setting: "feat_airing_page", label: "Airing page", hint: "the finale-countdown page" },
    { key: "searchPage", setting: "feat_search_page", label: "Search page", hint: "adding new shows and movies" },
    { key: "hero", setting: "feat_hero", label: "Up next banner", hint: "the big season suggestion on Shows" },
    { key: "airingRail", setting: "feat_airing_rail", label: "Airing now on Shows", hint: "the collapsible rail beside the grid" },
  ];

  const mb = (b: number) => (b / 1024 / 1024).toFixed(1);

  // ── section index: names plus the live state of each section ──
  let active = $state("tmdb");

  const dataStatus = () => {
    if (isAndroid) return phoneSync?.paired ? "Paired with desktop" : "Not paired";
    if (sync?.pairedDevice) return "Phone paired";
    return sync?.enabled ? "Sync on, no phone" : "Sync off";
  };

  const sections = $derived(
    [
      { id: "tmdb", name: "TMDB key", status: session.hasKey ? "Key saved" : "No key yet" },
      {
        id: "features",
        name: "Features",
        status: `${Object.values(session.features).filter(Boolean).length} of ${featureDefs.length} on`,
      },
      ...(isAndroid
        ? []
        : [{ id: "display", name: "Display", status: `${session.zoom}%${startMax ? " · maximized" : ""}` }]),
      { id: "data", name: "Library data", status: dataStatus() },
      { id: "maintenance", name: "Maintenance", status: cache ? `${mb(cache.bytes)} MB cached` : "" },
    ],
  );

  // one shared gold marker that slides between entries as the active section changes
  let navEl = $state<HTMLElement | null>(null);
  let markerTop = $state(0);
  let markerH = $state(0);

  function placeMarker() {
    const el = navEl?.querySelector<HTMLElement>("a.on");
    if (!el) return;
    markerTop = el.offsetTop + el.offsetHeight * 0.16;
    markerH = el.offsetHeight * 0.68;
  }

  $effect(() => {
    void active;
    void sections; // status text can rewrap an entry, moving everything below it
    placeMarker();
  });

  function goTo(e: MouseEvent, id: string) {
    e.preventDefault();
    active = id; // the marker slides there right away; the spy reconciles after
    document.getElementById(id)?.scrollIntoView({ behavior: "smooth", block: "start" });
  }

  function spy() {
    // at the very bottom the last section wins even if its top never crosses
    // the threshold (short tail pages, low zoom)
    if (window.innerHeight + window.scrollY >= document.body.scrollHeight - 4) {
      active = sections[sections.length - 1]?.id ?? "tmdb";
      return;
    }
    let cur = sections[0]?.id ?? "tmdb";
    for (const s of sections) {
      const el = document.getElementById(s.id);
      if (el && el.getBoundingClientRect().top <= 150) cur = s.id;
    }
    active = cur;
  }

  async function toggleFeature(def: (typeof featureDefs)[number], on: boolean) {
    session.features[def.key] = on;
    try {
      await api.settingsSet(def.setting, on ? "1" : "0");
    } catch (e) {
      session.banner = String(e);
    }
  }

  async function doBackupExport() {
    backupMsg = "";
    try {
      const path = await save({
        title: "Export ShowTrack backup",
        defaultPath: `showtrack-backup-${new Date().toISOString().slice(0, 10)}.json`,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (typeof path !== "string") return;
      backupBusy = true;
      await api.backupExport(path);
      backupMsg = `Exported to ${path}`;
    } catch (e) {
      backupMsg = String(e);
    } finally {
      backupBusy = false;
    }
  }

  async function doBackupRestore() {
    backupMsg = "";
    try {
      const path = await open({
        title: "Pick a ShowTrack backup",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (typeof path !== "string") return;
      backupBusy = true;
      const r = await api.backupImport(path);
      backupMsg = `Restored: ${r.shows} shows and ${r.movies} movies added.`;
      session.refreshTick++;
    } catch (e) {
      backupMsg = String(e);
    } finally {
      backupBusy = false;
    }
  }

  let winTitle = $state("");

  async function applyTitle() {
    const t = winTitle.trim();
    try {
      await getCurrentWindow().setTitle(t || "ShowTrack");
      await api.settingsSet("window_title", t);
    } catch (e) {
      session.banner = String(e);
    }
  }

  async function toggleStartMax(on: boolean) {
    startMax = on;
    try {
      await api.settingsSet("start_maximized", on ? "1" : "0");
    } catch (e) {
      session.banner = String(e);
    }
  }

  onMount(() => {
    getVersion().then((v) => (appVersion = v)).catch(() => {});
    api.importLastReport().then((r) => (report = r));
    api.cacheInfo().then((c) => (cache = c));
    if (!isAndroid) {
      api.syncServerStatus().then((s) => { sync = s; syncPort = s.port; });
      api.settingsAll().then((s) => {
        startMax = s["start_maximized"] !== "0";
        winTitle = s["window_title"] ?? "";
      });
    }
    if (isAndroid) {
      api.syncStatus().then((s) => {
        phoneSync = s;
        if (s.peerAddr) peerInput = s.peerAddr;
      });
    }
    window.addEventListener("scroll", spy, { passive: true });
    window.addEventListener("resize", placeMarker);
    spy();
    const unProgress = listen<ImportProgress>("import:progress", (e) => (progress = e.payload));
    const unDone = listen<ImportReport>("import:done", (e) => {
      report = e.payload;
      importing = false;
      progress = null;
    });
    return () => {
      window.removeEventListener("scroll", spy);
      window.removeEventListener("resize", placeMarker);
      unProgress.then((f) => f());
      unDone.then((f) => f());
    };
  });

  async function saveKey() {
    keyStatus = "checking…";
    try {
      const ok = await api.tmdbValidateKey(key.trim());
      keyStatus = ok ? "✓ valid, saved" : "✗ rejected by TMDB";
      if (ok) session.hasKey = true;
    } catch (e) {
      keyStatus = `✗ ${e}`;
    }
  }

  async function runImport() {
    importError = "";
    try {
      const path = await open({
        title: "Pick your TV Time export",
        filters: [{ name: "HTML", extensions: ["html"] }],
      });
      if (typeof path !== "string") return;
      importing = true;
      await api.importRun(path);
    } catch (e) {
      importing = false;
      importError = String(e);
    }
  }

  async function runRefresh() {
    refreshing = true;
    refreshResult = null;
    try {
      refreshResult = await api.refreshRun();
    } catch (e) {
      refreshResult = { refreshed: 0, failed: [String(e)] };
    } finally {
      refreshing = false;
    }
  }

  async function toggleSync() {
    if (!sync) return;
    syncMsg = "";
    pairCode = "";
    try {
      sync = await api.syncServerSet(!sync.enabled, syncPort);
    } catch (e) {
      syncMsg = String(e);
      try { sync = await api.syncServerStatus(); } catch {}
    }
  }

  async function applyPort() {
    if (!sync) return;
    try {
      sync = await api.syncServerSet(sync.enabled, syncPort);
    } catch (e) {
      syncMsg = String(e);
      try { sync = await api.syncServerStatus(); } catch {}
    }
  }

  async function showCode() {
    try {
      pairCode = await api.syncPairCode();
    } catch (e) {
      syncMsg = String(e);
    }
  }

  async function unpair() {
    try {
      await api.syncUnpair();
      pairCode = "";
      sync = await api.syncServerStatus();
    } catch (e) {
      syncMsg = String(e);
    }
  }

  async function doPair() {
    pairing = true;
    phoneSyncMsg = "";
    try {
      const r = await api.syncPair(peerInput.trim(), codeInput.trim());
      phoneSyncMsg = `Paired. Copied ${r.shows} shows and ${r.movies} movies from the desktop.`;
      phoneSync = await api.syncStatus();
      session.refreshTick++;
    } catch (e) {
      phoneSyncMsg = String(e);
    } finally {
      pairing = false;
    }
  }

  async function doSyncNow() {
    syncing = true;
    phoneSyncMsg = "";
    try {
      const r = await api.syncNow();
      phoneSyncMsg = `Synced: ${r.pulled} pulled, ${r.pushed} pushed.`;
      phoneSync = await api.syncStatus();
      if (r.pulled > 0) session.refreshTick++;
    } catch (e) {
      phoneSyncMsg = String(e);
    } finally {
      syncing = false;
    }
  }

  async function doUnpair() {
    unpairing = true;
    phoneSyncMsg = "";
    try {
      await api.syncUnpair();
      phoneSync = await api.syncStatus();
    } catch (e) {
      phoneSyncMsg = String(e);
    } finally {
      unpairing = false;
    }
  }

  // the slider previews its value while dragging; zoom applies on release
  let zoomDraft = $state(100);
  $effect(() => {
    zoomDraft = session.zoom;
  });

  function applyZoom(next: number) {
    if (next === session.zoom) return;
    session.zoom = next; // layout $effect applies it live
    api.settingsSet("ui_zoom", String(next)).catch((e) => (session.banner = String(e)));
  }
</script>

<div class="head"><h1>Settings</h1></div>

<div class="swrap">
  <nav class="sindex" aria-label="Settings sections" bind:this={navEl}>
    {#if markerH > 0}
      <span class="smarker" style={`top:${markerTop}px;height:${markerH}px`} aria-hidden="true"></span>
    {/if}
    {#each sections as s, i (s.id)}
      <a
        href={`#${s.id}`}
        class:on={active === s.id}
        style={`animation-delay:${80 + i * 55}ms`}
        onclick={(e) => goTo(e, s.id)}
      >
        <b>{s.name}</b>
        {#if s.status}<span class="sstat">{s.status}</span>{/if}
      </a>
    {/each}
  </nav>

  <div class="sbody">
    <section id="tmdb" class="sblock">
      <h2>TMDB <b>key</b><small>metadata and posters come from themoviedb.org</small></h2>
      <div class="panel section">
        <div class="row">
          <input style="flex:1" placeholder="paste a new key to replace the stored one" bind:value={key} />
          <button class="btn-gold" onclick={saveKey} disabled={key.trim() === ""}>Validate & save</button>
        </div>
        {#if keyStatus}<p class="dim">{keyStatus}</p>{/if}
      </div>
    </section>

    <section id="features" class="sblock">
      <h2>Features <b>on or off</b><small>pages disappear from the navigation when off</small></h2>
      <div class="panel section">
        {#each featureDefs as def (def.key)}
          <label class="featrow">
            <input
              type="checkbox"
              checked={session.features[def.key]}
              onchange={(e) => toggleFeature(def, e.currentTarget.checked)}
            />
            <span><b>{def.label}</b> <span class="dim">· {def.hint}</span></span>
          </label>
        {/each}
        <div class="prefdiv"></div>
        <label class="featrow">
          <input type="checkbox" checked={session.askAddType}
                 onchange={(e) => togglePref("search_ask_type", "askAddType", e.currentTarget.checked)} />
          <span><b>Ask where to add</b> <span class="dim">· adding from Search asks whether the title goes with shows, anime or movies</span></span>
        </label>
        <label class="featrow">
          <input type="checkbox" checked={session.allFirst}
                 onchange={(e) => togglePref("all_first", "allFirst", e.currentTarget.checked)} />
          <span><b>All filters first</b> <span class="dim">· the All chip leads on Shows, Anime and Movies</span></span>
        </label>
        <label class="featrow">
          <input type="checkbox" checked={session.episodeMode}
                 onchange={(e) => togglePref("episode_mode", "episodeMode", e.currentTarget.checked)} />
          <span><b>Episode-by-episode</b> <span class="dim">· shows surface as soon as an episode airs, no waiting for the full season</span></span>
        </label>
        <label class="featrow">
          <input type="checkbox" checked={session.navReorder}
                 onchange={(e) => togglePref("nav_reorder", "navReorder", e.currentTarget.checked)} />
          <span><b>Re-order navigation</b> <span class="dim">· drag the sidebar buttons into your own order; Settings stays put</span></span>
        </label>
        <label class="featrow">
          <input type="checkbox" checked={session.keepSearch}
                 onchange={(e) => togglePref("keep_search", "keepSearch", e.currentTarget.checked)} />
          <span><b>Keep search</b> <span class="dim">· the Search page remembers your last search until you clear it</span></span>
        </label>
        <label class="featrow">
          <input type="checkbox" checked={session.navSeparators}
                 onchange={(e) => togglePref("nav_separators", "navSeparators", e.currentTarget.checked)} />
          <span><b>Navigation separators</b> <span class="dim">· a thin line between the sidebar buttons</span></span>
        </label>
      </div>
    </section>

    {#if !isAndroid}
      <section id="display" class="sblock">
        <h2>Display</h2>
        <div class="panel section">
          <div class="row" style="align-items:center">
            <span class="rowlabel">UI scale</span>
            <input
              class="zslider"
              type="range"
              min="80"
              max="150"
              step="10"
              bind:value={zoomDraft}
              onchange={() => applyZoom(zoomDraft)}
              aria-label="UI scale"
            />
            <span class="zoomval">{zoomDraft}%</span>
          </div>
          <div class="row" style="align-items:center">
            <span class="rowlabel">Window title</span>
            <input
              style="flex:1;max-width:320px"
              placeholder="ShowTrack"
              bind:value={winTitle}
              onkeydown={(e) => e.key === "Enter" && applyTitle()}
              aria-label="Window title"
            />
            <button class="btn-ghost" onclick={applyTitle}>Apply</button>
            <span class="dim">leave empty for the default</span>
          </div>
          <label class="featrow">
            <input type="checkbox" checked={startMax} onchange={(e) => toggleStartMax(e.currentTarget.checked)} />
            <span><b>Start maximized</b> <span class="dim">· applied at the next launch</span></span>
          </label>
        </div>
      </section>
    {/if}

    <section id="data" class="sblock">
      <h2>Library <b>data</b><small>everything that moves your library in or out</small></h2>
      <div class="panel section datapanel">
        {#if !isAndroid}
          <div class="dblock">
            <h3 class="subh">Import from TV Time</h3>
            <p class="dim">Re-runnable: it only adds and updates, never deletes. Episodes marked in ShowTrack keep their dates.</p>
            <div class="row">
              <button class="btn-gold" onclick={runImport} disabled={progress !== null}>
                {importing ? "Importing…" : "Pick export file & import"}
              </button>
            </div>
            {#if importError}<p style="color:var(--red)">{importError}</p>{/if}
            {#if progress}
              <p>{progress.stage}: {progress.done + 1}/{progress.total} · {progress.label}</p>
              <div class="bar big"><i style={`width:${((progress.done + 1) / Math.max(progress.total, 1)) * 100}%`}></i></div>
            {/if}
            {#if report}
              <details class="report">
                <summary>
                  <span class="chev" aria-hidden="true">▸</span>
                  Last import {report.finishedAt.slice(0, 10)} · {report.showsOk} shows, {report.moviesOk} movies
                  {#if report.failures.length}<span class="warn">· {report.failures.length} failed</span>{/if}
                  {#if report.anomalies.length}<span class="warn">· {report.anomalies.length} mismatches</span>{/if}
                </summary>
                {#if report.failures.length}
                  <h2 class="ph rh">Failures ({report.failures.length})</h2>
                  <ul>
                    {#each report.failures as f}
                      <li><b>{f.title}</b> ({f.kind}): {f.reason}</li>
                    {/each}
                  </ul>
                  <p class="dim">Add these manually via Search, or re-run the import to retry.</p>
                {/if}
                {#if report.anomalies.length}
                  <h2 class="ph rh">Count mismatches ({report.anomalies.length})</h2>
                  <ul>
                    {#each report.anomalies as a}
                      <li>
                        <a class="alink" href={`/show/${a.tmdbId}`}><b>{a.title}</b></a>
                        · TV Time says {a.expected} watched, ShowTrack marked {a.actual}. Review its episodes.
                      </li>
                    {/each}
                  </ul>
                {/if}
                {#if report.specialsNote.length}
                  <h2 class="ph rh">Watched specials to re-tick manually</h2>
                  <ul>
                    {#each report.specialsNote as s}
                      <li><b>{s.title}</b> · {s.watchedSpecials} special(s) were watched on TV Time</li>
                    {/each}
                  </ul>
                {/if}
              </details>
            {/if}
          </div>

          <div class="dblock">
            <h3 class="subh">Backup</h3>
            <p class="dim">
              Exports the whole library (shows, movies, watch history and the TMDB key) to a JSON file.
              Restoring merges: it only adds what is missing, never deletes.
            </p>
            <div class="row">
              <button class="btn-gold" onclick={doBackupExport} disabled={backupBusy}>Export library</button>
              <button class="btn-ghost" onclick={doBackupRestore} disabled={backupBusy}>Restore from backup</button>
            </div>
            {#if backupMsg}<p class="dim">{backupMsg}</p>{/if}
          </div>

          {#if sync}
            <div class="dblock">
              <h3 class="subh">Phone sync</h3>
              <p class="dim">
                Pairs the Android app over your home network. The server only runs while
                this app is open. Find this PC's address with ipconfig.
              </p>
              <div class="row" style="align-items:center">
                <button class={sync.enabled ? "btn-gold" : "btn-ghost"} onclick={toggleSync}>
                  {sync.enabled ? "Sync is on" : "Turn sync on"}
                </button>
                <input style="width:110px" type="number" bind:value={syncPort} min="1024" max="65535" />
                <button class="btn-ghost" onclick={applyPort} disabled={!sync.enabled}>Apply port</button>
              </div>
              {#if sync.enabled && !sync.running && sync.startError}
                <p style="color:var(--red)">
                  Sync could not start ({sync.startError}). Pick another port and press Apply port.
                </p>
              {/if}
              {#if sync.running}
                <div class="row" style="align-items:center">
                  <button class="btn-ghost" onclick={showCode}>Show pairing code</button>
                  {#if pairCode}<span class="code">{pairCode}</span><span class="dim">valid 5 minutes, one use</span>{/if}
                </div>
                <p class="dim">
                  {sync.pairedDevice
                    ? `Paired phone: ${sync.pairedDevice}${sync.lastPushAt ? ` · last push ${sync.lastPushAt}` : ""}`
                    : "No phone paired yet."}
                  {#if sync.pairedDevice}<button class="unpairbtn" onclick={unpair}>Unpair</button>{/if}
                </p>
              {/if}
              {#if syncMsg}<p style="color:var(--red)">{syncMsg}</p>{/if}
            </div>
          {/if}
        {:else if phoneSync}
          <div class="dblock">
            <h3 class="subh">Sync with desktop</h3>
            {#if !phoneSync.paired}
              <p class="dim">
                On the desktop app: Settings, turn Phone sync on, then Show pairing code.
                Enter this PC's address and the code here.
              </p>
              <input placeholder="192.168.1.20:47810" bind:value={peerInput} />
              <input placeholder="6-digit code" inputmode="numeric" maxlength="6" bind:value={codeInput} />
              <button class="btn-gold" onclick={doPair}
                      disabled={pairing || peerInput.trim() === "" || codeInput.trim().length !== 6}>
                {pairing ? "Pairing…" : "Pair & copy library"}
              </button>
            {:else}
              <p class="dim">
                Paired with {phoneSync.peerAddr}
                {phoneSync.lastSync ? ` · last sync ${phoneSync.lastSync}` : ""}
              </p>
              <div class="row">
                <button class="btn-gold" onclick={doSyncNow} disabled={syncing}>
                  {syncing ? "Syncing…" : "Sync now"}
                </button>
                <button class="btn-ghost" onclick={doUnpair} disabled={unpairing}>Unpair</button>
              </div>
            {/if}
            {#if phoneSyncMsg}<p class="dim">{phoneSyncMsg}</p>{/if}
          </div>
        {/if}
      </div>
    </section>

    <section id="maintenance" class="sblock">
      <h2>Maintenance</h2>
      <div class="panel section">
        <div class="row" style="align-items:center">
          <span class="rowlabel">Airing shows</span>
          <button class="btn-ghost" onclick={runRefresh} disabled={refreshing}>{refreshing ? "Refreshing…" : "Refresh now"}</button>
        </div>
        {#if refreshResult}
          <p class="dim">
            {refreshResult.refreshed} refreshed{refreshResult.failed.length
              ? `, ${refreshResult.failed.length} failed: ${refreshResult.failed.join("; ")}`
              : ""}
          </p>
        {/if}
        {#if !isAndroid}
          <div class="row" style="align-items:center">
            <span class="rowlabel">Updates</span>
            <button class="btn-ghost" onclick={checkUpdates} disabled={updChecking}>
              {updChecking ? "Checking…" : "Check for updates"}
            </button>
            {#if appVersion}<span class="dim">version {appVersion}</span>{/if}
          </div>
          {#if updMsg}<p class="dim">{updMsg}</p>{/if}
          {#if upd}
            <div class="updcard">
              <b>Version {upd.version} is available.</b>
              {#if upd.notes}<p class="dim updnotes">{upd.notes}</p>{/if}
              {#if upd.installerUrl}
                <button class="btn-gold" onclick={installUpdate} disabled={updInstalling}>
                  {updInstalling ? "Downloading…" : upd.kind === "portable" ? "Update and restart" : "Install and restart"}
                </button>
              {:else}
                <p class="dim">Download it from {upd.pageUrl}</p>
              {/if}
            </div>
          {/if}
        {/if}
        <div class="row" style="align-items:center">
          <span class="rowlabel">What's new</span>
          <button class="btn-ghost" onclick={openWhatsNew} disabled={!appVersion}>Show</button>
        </div>
        {#if cache}
          <p class="dim cacheline">Image cache · {cache.path} · {mb(cache.bytes)} MB</p>
        {/if}
      </div>
    </section>
  </div>
</div>

<style>
  .head { margin-bottom: 26px; }

  /* full-width ledger: sticky index plus the content column */
  .swrap {
    display: grid; grid-template-columns: 250px minmax(0, 1fr);
    gap: 56px;
  }
  .sindex {
    position: sticky; top: 28px; align-self: start;
    display: flex; flex-direction: column; gap: 4px;
  }
  /* the shared active marker; sliding top/height carry it between entries */
  .smarker {
    position: absolute; left: 0; width: 2px; border-radius: 2px;
    z-index: 1; /* above the entries, whose backgrounds would otherwise cover it at rest */
    background: var(--gold); pointer-events: none;
    transition: top 0.3s cubic-bezier(0.2, 0.7, 0.3, 1), height 0.3s cubic-bezier(0.2, 0.7, 0.3, 1);
    animation: rise 0.45s cubic-bezier(0.2, 0.7, 0.3, 1) 80ms both;
  }
  .sindex a {
    position: relative;
    display: block; padding: 10px 14px; border-radius: 12px;
    transition: background-color 0.18s ease, transform 0.15s ease;
    animation: rise 0.45s cubic-bezier(0.2, 0.7, 0.3, 1) both;
  }
  /* hover hint: a faint bar grows in, nothing shifts */
  .sindex a::before {
    content: ""; position: absolute; left: 0; top: 50%; translate: 0 -50%;
    width: 2px; border-radius: 2px; background: var(--gold);
    height: 0; opacity: 0;
    transition: height 0.24s cubic-bezier(0.2, 0.7, 0.3, 1), opacity 0.18s ease;
  }
  .sindex a:hover { background: var(--raise); }
  .sindex a:hover::before { height: 38%; opacity: 0.55; }
  .sindex a.on::before { display: none; }
  .sindex a:active { transform: scale(0.97); }
  .sindex a b {
    display: block; font-family: "Anton", sans-serif; font-weight: 400;
    font-size: 14px; letter-spacing: 0.07em; text-transform: uppercase;
    color: var(--dim); transition: color 0.18s ease;
  }
  .sindex a .sstat {
    display: block; font-size: 11.5px; color: var(--faint); margin-top: 2px;
    transition: color 0.18s ease;
  }
  .sindex a:hover b { color: var(--text); }
  .sindex a:hover .sstat { color: var(--dim); }
  .sindex a.on { background: var(--raise); }
  .sindex a.on b { color: var(--gold); }
  .sindex a.on .sstat { color: var(--dim); }

  .sbody { min-width: 0; }
  .sblock { scroll-margin-top: 24px; animation: rise 0.45s cubic-bezier(0.2, 0.7, 0.3, 1) both; }
  .sblock + .sblock { margin-top: 40px; }
  .sblock:nth-child(2) { animation-delay: 60ms; }
  .sblock:nth-child(3) { animation-delay: 120ms; }
  .sblock:nth-child(4) { animation-delay: 180ms; }
  .sblock:nth-child(5) { animation-delay: 240ms; }
  .sblock h2 { margin: 0 0 14px; }
  /* tall enough that the spy can reach the last section at any zoom */
  @media (min-width: 1281px) {
    .sblock:last-child { min-height: calc(100vh - 200px); }
  }

  .section { display: flex; flex-direction: column; gap: 10px; }
  .ph { margin: 0; }
  .row { display: flex; gap: 8px; }
  .rowlabel { min-width: 110px; font-weight: 600; font-size: 13.5px; }
  .zoomval { min-width: 52px; text-align: center; font-weight: 600; }
  .zslider {
    appearance: none; width: 260px; height: 5px; padding: 0;
    background: #232b3d; border: none; border-radius: 999px; cursor: pointer;
  }
  .zslider::-webkit-slider-thumb {
    appearance: none; width: 18px; height: 18px; border-radius: 50%;
    background: var(--gold); border: none;
    box-shadow: 0 0 12px rgba(255, 207, 110, 0.35);
    transition: transform 0.12s ease, box-shadow 0.18s ease;
  }
  .zslider::-webkit-slider-thumb:hover { transform: scale(1.15); }
  .zslider:active::-webkit-slider-thumb {
    transform: scale(1.25); box-shadow: 0 0 20px rgba(255, 207, 110, 0.5);
  }
  .zslider:focus { box-shadow: none; border: none; }
  .featrow { display: flex; align-items: center; gap: 10px; cursor: pointer; padding: 2px 0; }
  .prefdiv { border-top: 1px solid var(--line); margin: 8px 0 6px; }
  .updcard {
    border: 1px solid rgba(255, 207, 110, 0.4); border-radius: var(--r-md);
    padding: 12px 16px; display: flex; flex-direction: column; gap: 8px;
    align-items: flex-start; animation: rise 0.3s cubic-bezier(0.2, 0.7, 0.3, 1) both;
  }
  .updnotes { white-space: pre-line; margin: 0; font-size: 12.5px; }
  .featrow input { flex-shrink: 0; }
  .bar.big { height: 8px; border-radius: 4px; }
  .bar.big i { transition: width 0.2s; }

  /* the three data flows share one panel, separated by hairlines */
  .datapanel { gap: 0; }
  .dblock { display: flex; flex-direction: column; gap: 10px; padding: 6px 0 18px; }
  .dblock + .dblock { border-top: 1px solid var(--line); padding-top: 18px; }
  .dblock:last-child { padding-bottom: 4px; }
  .subh {
    margin: 0; font-family: "Anton", sans-serif; font-weight: 400; font-size: 14px;
    letter-spacing: 0.07em; text-transform: uppercase; color: var(--text);
  }
  .subh + .dim { margin-top: -4px; }

  /* the import report folds away until wanted */
  .report summary {
    cursor: pointer; list-style: none; font-size: 13px; color: var(--dim);
    padding: 8px 0 2px; display: flex; align-items: baseline; gap: 8px; flex-wrap: wrap;
  }
  .report summary::-webkit-details-marker { display: none; }
  .report summary:hover { color: var(--text); }
  .report .chev { transition: transform 0.3s cubic-bezier(0.2, 0.7, 0.3, 1); display: inline-block; }
  .report[open] .chev { transform: rotate(90deg); }
  .report .warn { color: var(--gold); }
  .report .rh { margin: 12px 0 4px; }
  .report ul { margin: 4px 0 8px; padding-left: 20px; }
  .alink { color: var(--gold); }
  .cacheline { border-top: 1px solid var(--line); padding-top: 12px; }

  .code {
    font-family: "Anton", sans-serif; font-size: 30px; letter-spacing: 0.35em;
    color: var(--gold); padding: 2px 10px;
  }
  .unpairbtn { margin-left: 10px; font-size: 12px; padding: 4px 12px; }

  @media (max-width: 1279px) {
    .swrap { grid-template-columns: minmax(0, 1fr); }
    .sindex { display: none; }
  }
  @media (max-width: 760px) {
    .sblock + .sblock { margin-top: 30px; }
    /* narrow screens: action buttons drop to their own full-width row
       instead of squeezing beside the input and wrapping their label */
    .row { flex-wrap: wrap; }
    .row .btn-gold, .row .btn-ghost { flex: 1 0 100%; justify-content: center; text-align: center; }
    .rowlabel { flex-basis: 100%; }
  }
</style>
