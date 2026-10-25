<script lang="ts">
  import { api } from "#lib/api.js";
  import { session } from "#lib/session.svelte.js";
  import { airingLabel, fmtDate, fmtRuntime } from "#lib/format.js";
  import type { BingeCard } from "#lib/types.js";
  import PosterImg from "#lib/ui/PosterImg.svelte";
  import Ring from "#lib/ui/Ring.svelte";
  import TileSlider from "#lib/ui/TileSlider.svelte";

  let { animeOnly = false }: { animeOnly?: boolean } = $props();

  let cards = $state<BingeCard[]>([]);
  let loaded = $state(false);
  let loadError = $state("");
  const tabKey = $derived(animeOnly ? "anime" : "shows");
  type Filter = "watching" | "plan_to_watch" | "stopped" | "all";
  // svelte-ignore state_referenced_locally -- the remembered tab seeds the initial value only
  let filter = $state<Filter>(session.tabs[animeOnly ? "anime" : "shows"] as Filter);

  function setFilter(value: Filter) {
    filter = value;
    session.tabs[tabKey] = value;
    api.settingsSet(`tab_${tabKey}`, value).catch(() => {}); // cosmetic preference, losing it is fine
  }
  // showId -> season marked this session (undo affordance until the next refetch)
  let marked = $state<Map<number, { season: number; ids: number[]; priorDone: boolean }>>(new Map());

  $effect(() => {
    void session.refreshTick;
    (async () => {
      try {
        cards = await api.bingeList();
        marked = new Map();
        loadError = "";
      } catch (e) {
        loadError = String(e);
      } finally {
        loaded = true;
      }
    })();
  });

  const byActivity = (a: BingeCard, b: BingeCard) =>
    (b.lastActivity ?? "").localeCompare(a.lastActivity ?? "") || a.name.localeCompare(b.name);

  // anime lives on its own page while that page is enabled
  const pool = $derived(
    cards.filter((c) => (animeOnly ? c.isAnime : !(c.isAnime && session.features.animePage))),
  );

  const readyPool = $derived(
    pool.filter(
      (c) =>
        c.ready &&
        (c.myState === "watching" || c.myState === "plan_to_watch"),
    ),
  );
  const hero = $derived.by(() => {
    const started = readyPool.filter((c) => c.ready!.kind !== "start").toSorted(byActivity);
    if (started.length > 0) return started[0];
    const starts = readyPool
      .filter((c) => c.ready!.kind === "start")
      .toSorted((a, b) => a.name.localeCompare(b.name));
    return starts[0] ?? null;
  });
  // the hero's show deliberately stays in this grid too
  const grid = $derived(
    readyPool.toSorted((a, b) => {
      const k = (x: BingeCard) => (x.ready!.kind === "start" ? 1 : 0);
      return k(a) - k(b) || byActivity(a, b);
    }),
  );
  const airing = $derived(
    pool
      .filter((c) => c.myState === "watching" && c.airing && !c.ready)
      .toSorted(
        (a, b) =>
          (a.airing!.finaleDate ?? "9999").localeCompare(b.airing!.finaleDate ?? "9999") ||
          a.name.localeCompare(b.name),
      ),
  );
  const caught = $derived(
    pool
      .filter((c) => c.myState === "watching" && c.caughtUp && !c.airing && !c.ready && c.seriesAired > 0)
      .toSorted((a, b) => a.name.localeCompare(b.name)),
  );
  const others = $derived(
    pool
      .filter((c) => (filter === "all" ? true : c.myState === filter))
      .toSorted((a, b) => a.name.localeCompare(b.name)),
  );
  // episode-by-episode mode: anything with an unwatched aired episode is
  // watchable now, sorted by recent activity; season-readiness stops gating
  const episodeGrid = $derived(
    pool
      .filter(
        (c) =>
          (c.myState === "watching" || c.myState === "plan_to_watch") &&
          c.seriesAired > c.seriesWatchedAired,
      )
      .toSorted(byActivity),
  );
  const mainGrid = $derived(session.episodeMode ? episodeGrid : grid);

  async function markNext(card: BingeCard) {
    try {
      await api.showMarkNext(card.tmdbId);
      cards = cards.map((x) =>
        x.tmdbId === card.tmdbId ? { ...x, seriesWatchedAired: x.seriesWatchedAired + 1 } : x,
      );
    } catch (e) {
      session.banner = String(e);
    }
  }

  let query = $state("");
  const tileVar = $derived(`--tile:${session.tiles[animeOnly ? "anime" : "shows"]}px`);
  const q = $derived(query.trim().toLowerCase());
  // typing searches the whole pool, whatever chip is active
  const matches = $derived(
    pool.filter((c) => c.name.toLowerCase().includes(q)).toSorted((a, b) => a.name.localeCompare(b.name)),
  );
  const readyCount = $derived(readyPool.filter((c) => c.ready!.kind !== "start").length);
  const startCount = $derived(readyPool.filter((c) => c.ready!.kind === "start").length);

  const baseChips = $derived([
    ["watching", "Watching"],
    ["plan_to_watch", "Plan to watch"],
    ["stopped", "Stopped"],
    ["all", animeOnly ? "All anime" : "All shows"],
  ] as const);
  const chips = $derived(
    session.allFirst ? [baseChips[3], baseChips[0], baseChips[1], baseChips[2]] : [...baseChips],
  );

  async function markSeason(card: BingeCard) {
    const season = card.ready?.seasonNumber;
    if (season == null || marked.has(card.tmdbId)) return;
    try {
      const ids = await api.seasonMark(card.tmdbId, season, true);
      marked = new Map(marked).set(card.tmdbId, { season, ids, priorDone: false });
    } catch (e) {
      session.banner = String(e);
    }
  }

  async function markPrevious(card: BingeCard) {
    const entry = marked.get(card.tmdbId);
    if (!entry || entry.priorDone) return;
    try {
      const ids = await api.seasonsMarkBefore(card.tmdbId, entry.season);
      marked = new Map(marked).set(card.tmdbId, {
        ...entry,
        ids: [...entry.ids, ...ids], // undo reverts previous seasons too
        priorDone: true,
      });
    } catch (e) {
      session.banner = String(e);
    }
  }

  const priorOffer = (c: BingeCard) => {
    const entry = marked.get(c.tmdbId);
    return entry && !entry.priorDone && c.ready!.priorUnwatched > 0
      ? `Also mark S1–S${entry.season - 1} (${c.ready!.priorUnwatched} eps)`
      : null;
  };

  async function undo(card: BingeCard) {
    const entry = marked.get(card.tmdbId);
    if (!entry) return;
    try {
      await api.episodesUnmark(entry.ids);
      const m = new Map(marked);
      m.delete(card.tmdbId);
      marked = m;
    } catch (e) {
      session.banner = String(e);
    }
  }

  const badge = (c: BingeCard) =>
    c.ready!.kind === "start"
      ? "ready to start"
      : c.ready!.kind === "finish"
        ? `${c.ready!.epLeft} left in S${c.ready!.seasonNumber}`
        : "ready to watch";
  const seasonLine = (c: BingeCard) => {
    const r = c.ready!;
    const rt = r.runtimeLeftMin > 0 ? ` · ~${fmtRuntime(r.runtimeLeftMin)}` : "";
    return `S${r.seasonNumber} · ${r.epLeft} eps${rt}`;
  };
  const heroMeta = (c: BingeCard) => {
    const r = c.ready!;
    const rt = r.runtimeLeftMin > 0 ? ` · ~${fmtRuntime(r.runtimeLeftMin)}` : "";
    const finale = c.airing ? "" : " · the whole season is waiting";
    return r.kind === "finish"
      ? `${r.epLeft} of ${r.epCount} episodes left${rt}`
      : `${r.epCount} episodes${rt}${finale}`;
  };
  const seriesPct = (c: BingeCard) =>
    c.seriesAired > 0 ? Math.round((c.seriesWatchedAired / c.seriesAired) * 100) : 0;
</script>

<div class="head">
  {#if animeOnly}
    <h1>Anime</h1>
  {:else}
    <h1>Shows</h1>
  {/if}
  <div class="stats">
    <div class="stat g"><b>{readyCount}</b><span>seasons ready</span></div>
    <div class="stat"><b>{startCount}</b><span>ready to start</span></div>
    <div class="stat"><b>{caught.length}</b><span>caught up</span></div>
  </div>
</div>

<div class="toolbar">
  <div class="chips">
    {#each chips as [value, label] (value)}
      <button class="chip" class:on={filter === value} onclick={() => setFilter(value)}>{label}</button>
    {/each}
  </div>
  <input placeholder="Filter…" bind:value={query} aria-label={animeOnly ? "Filter anime" : "Filter shows"} />
  <TileSlider section={animeOnly ? "anime" : "shows"} />
</div>

{#snippet plainCards(list: BingeCard[])}
  <div class="grid-cards" style={tileVar}>
    {#each list as c (c.tmdbId)}
      <div class="card">
        <a href={`/show/${c.tmdbId}`}><PosterImg path={c.posterPath} alt={c.name} glow /></a>
        <div class="under">
          <a class="title" href={`/show/${c.tmdbId}`} title={c.name}>{c.name}</a>
          <div class="season-line dim">{c.seriesWatchedAired}/{c.seriesAired} watched</div>
          {#if c.ready}<span class="badge">{badge(c)}</span>{/if}
        </div>
      </div>
    {/each}
  </div>
  {#if list.length === 0}<p class="dim">Nothing here.</p>{/if}
{/snippet}

{#if loadError}
  <p style="color:var(--red)">{loadError}</p>
{:else if !loaded}
  <p class="dim">Loading…</p>
{:else if q !== ""}
  {@render plainCards(matches)}
{:else if filter !== "watching"}
  {@render plainCards(others)}
{:else}
  {#if pool.length === 0}
    {#if animeOnly}
      <p class="dim">No anime in your library yet. Add some via Search and they will show up here.</p>
    {:else}
      <p class="dim">No shows yet. Run the TV Time import from Settings, or add shows via Search.</p>
    {/if}
  {:else}
  <div class="cols" class:norail={!session.features.airingRail}>
    <div class="center">
      {#if hero && session.features.hero}
        <section class="hero" aria-label="Up next">
          <div class="eyebrow">Up <b>next</b></div>
          <div class="hrow">
            <div>
              <a class="hshow" href={`/show/${hero.tmdbId}`}>{hero.name}</a>
              <div class="hseason">Season <span>{hero.ready!.seasonNumber}</span></div>
              <div class="hmeta dim">{heroMeta(hero)}</div>
            </div>
            <div class="hactions">
              {#if marked.has(hero.tmdbId)}
                <span class="badge">Season {marked.get(hero.tmdbId)!.season} marked ✓</span>
                {#if priorOffer(hero)}
                  <button class="btn-gold" onclick={() => markPrevious(hero)}>{priorOffer(hero)}</button>
                {/if}
                <button class="btn-ghost" onclick={() => undo(hero)}>Undo</button>
              {:else}
                <a class="btn-ghost" href={`/show/${hero.tmdbId}`}>Episode list</a>
                <button class="btn-gold" onclick={() => markSeason(hero)}>✓&nbsp; Season watched</button>
              {/if}
            </div>
          </div>
        </section>
      {/if}

      {#if session.episodeMode}
        <h2>Ready to <b>watch</b><small>every unwatched aired episode counts</small></h2>
      {:else}
        <h2>Complete seasons <b>ready</b><small>every episode has aired · tap the ring when you finish</small></h2>
      {/if}
      {#if mainGrid.length === 0 && !hero}
        <p class="dim">{session.episodeMode ? "Nothing new to watch right now." : "No complete seasons waiting. Check Airing for what's on the way."}</p>
      {:else}
        <div class="grid-cards" style={tileVar}>
          {#each mainGrid as c, i (c.tmdbId)}
            <div class="card rise" style={`animation-delay:${Math.min(i, 12) * 45}ms`}>
              {#if c.ready && !marked.has(c.tmdbId)}
                <Ring pct={seriesPct(c)} label={`Mark ${c.name} season ${c.ready!.seasonNumber} watched`} onclick={() => markSeason(c)} />
              {/if}
              <a href={`/show/${c.tmdbId}`}><PosterImg path={c.posterPath} alt={c.name} glow /></a>
              <div class="under">
                <a class="title" href={`/show/${c.tmdbId}`} title={c.name}>{c.name}</a>
                {#if marked.has(c.tmdbId)}
                  <div class="season-line">Season {marked.get(c.tmdbId)!.season} marked ✓</div>
                  {#if priorOffer(c)}
                    <button class="undo prior" onclick={() => markPrevious(c)}>{priorOffer(c)}</button>
                  {/if}
                  <button class="undo" onclick={() => undo(c)}>Undo</button>
                {:else if c.ready}
                  <div class="season-line"><b>{seasonLine(c)}</b></div>
                  <span class="badge">{badge(c)}</span>
                {:else}
                  <div class="season-line"><b>{c.seriesAired - c.seriesWatchedAired} to watch</b>{#if c.airing}<span class="dim"> · S{c.airing.seasonNumber} airing</span>{/if}</div>
                  <button class="undo" onclick={() => markNext(c)}>✓ Next episode</button>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>

    {#if session.features.airingRail}
    <aside class="rail">
      <details class="railbox" open>
        <summary>
          <h2 class="railh">Airing <b>now</b><small>they move left when the finale lands</small></h2>
          <span class="chev" aria-hidden="true">▸</span>
        </summary>
        {#if airing.length === 0}
          <p class="dim">Nothing airing for the shows you follow.</p>
        {:else}
          <div class="airlist">
            {#each airing as c, i (c.tmdbId)}
              <a class="air panel rise" style={`animation-delay:${80 + Math.min(i, 10) * 45}ms`} href={`/show/${c.tmdbId}`}>
                <div class="top">
                  <b>{c.name}</b>
                  <span class="when">{airingLabel(c.airing!).toUpperCase()}</span>
                </div>
                <div class="sub dim">
                  Season {c.airing!.seasonNumber} · {c.airing!.airedCount} of {c.airing!.totalCount} episodes aired
                </div>
                <div class="bar"><i style={`width:${c.airing!.totalCount > 0 ? (c.airing!.airedCount / c.airing!.totalCount) * 100 : 0}%`}></i></div>
                {#if c.airing!.nextAirDate}
                  <div class="cap">next episode {fmtDate(c.airing!.nextAirDate)}{c.airing!.finaleDate ? ` · finale ${fmtDate(c.airing!.finaleDate)}` : ""}</div>
                {/if}
              </a>
            {/each}
          </div>
        {/if}
      </details>
    </aside>
    {/if}
  </div>
  {/if}
{/if}

<style>
  .head { display: flex; align-items: flex-end; justify-content: space-between; gap: 18px; flex-wrap: wrap; margin-bottom: 24px; }
  .toolbar { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; margin-bottom: 30px; }
  .toolbar input { margin-left: auto; }
  @media (max-width: 760px) {
    .toolbar input { margin-left: 0; flex: 1; }
  }
  .hero {
    position: relative; border-radius: 22px; overflow: hidden;
    border: 1px solid var(--line);
    animation: rise 0.5s cubic-bezier(0.2, 0.7, 0.3, 1) both;
    background:
      radial-gradient(120% 180% at 88% 0%, rgba(148, 163, 184, 0.14), transparent 55%),
      radial-gradient(110% 160% at 8% 110%, rgba(255, 207, 110, 0.1), transparent 50%),
      linear-gradient(105deg, #0c1018 45%, #101623);
    padding: 34px 38px 30px; margin-bottom: 8px;
  }
  .hero::after {
    content: ""; position: absolute; inset: 0; pointer-events: none;
    background: repeating-linear-gradient(0deg, rgba(255, 255, 255, 0.014) 0 1px, transparent 1px 3px);
  }
  .eyebrow { font-family: "Anton", sans-serif; font-size: 12px; letter-spacing: 0.22em; text-transform: uppercase; color: var(--dim); margin-bottom: 12px; }
  .eyebrow b { color: var(--gold); font-weight: 400; }
  .hrow { display: flex; align-items: flex-end; justify-content: space-between; gap: 30px; flex-wrap: wrap; }
  .hshow { font-size: 16px; font-weight: 600; color: var(--dim); }
  .hshow:hover { color: var(--text); }
  .hseason { font-family: "Anton", sans-serif; font-size: clamp(52px, 6vw, 78px); line-height: 1; }
  .hseason span { color: var(--gold); }
  .hmeta { font-size: 13.5px; margin-top: 8px; }
  .hactions { display: flex; gap: 12px; align-items: center; position: relative; z-index: 1; }
  .card { position: relative; }
  .under { padding: 11px 3px 0; }
  .title { display: block; font-weight: 600; font-size: 14px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .season-line { color: var(--dim); font-size: 12.5px; margin: 2px 0 8px; }
  .season-line b { color: var(--gold); font-weight: 700; }
  .undo { font-size: 12px; padding: 4px 12px; }
  .undo.prior { color: var(--gold); margin-right: 6px; margin-bottom: 4px; }
  .railbox summary {
    list-style: none; cursor: pointer;
    display: flex; align-items: baseline; justify-content: space-between; gap: 10px;
  }
  .railbox summary::-webkit-details-marker { display: none; }
  /* padding, not margin: child margins collapse through ::details-content and
     snap away at the very end of the close animation */
  .railbox p { margin: 0; padding: 10px 0 2px; }
  .railh { margin-top: 6px; }
  .chev {
    color: var(--dim); font-size: 15px;
    transition: transform 0.3s cubic-bezier(0.2, 0.7, 0.3, 1);
  }
  .railbox[open] .chev { transform: rotate(90deg); }
  .airlist { display: flex; flex-direction: column; gap: 12px; padding-top: 2px; }
  .air { display: block; }
  .air:hover { border-color: var(--faint); transform: translateY(-2px); }
  .air .top { display: flex; justify-content: space-between; align-items: baseline; gap: 10px; }
  .air .top b { font-weight: 600; font-size: 15px; }
  .air .when { font-family: "Anton", sans-serif; font-size: 12px; letter-spacing: 0.08em; color: var(--gold); white-space: nowrap; }
  .air .sub { font-size: 12.5px; margin: 3px 0 12px; }
  .air .cap { font-size: 11px; color: var(--faint); margin-top: 7px; }
  @media (max-width: 760px) {
    .head { margin-bottom: 16px; }
    .hero { padding: 22px 20px 20px; border-radius: 18px; }
    .hseason { font-size: clamp(40px, 13vw, 56px); }
    .hrow { flex-direction: column; align-items: stretch; gap: 18px; }
    .hactions { flex-direction: column; align-items: stretch; }
    .hactions .btn-gold, .hactions .btn-ghost {
      justify-content: center; text-align: center; white-space: nowrap;
    }
  }
</style>
