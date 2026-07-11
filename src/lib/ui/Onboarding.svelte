<script lang="ts">
  import { api } from "$lib/api";
  import { session } from "$lib/session.svelte";

  let key = $state("");
  let checking = $state(false);
  let error = $state("");

  async function validate() {
    checking = true;
    error = "";
    try {
      if (await api.tmdbValidateKey(key.trim())) {
        session.hasKey = true;
      } else {
        error = "TMDB rejected that key (401). Double-check it and try again.";
      }
    } catch (e) {
      error = `Could not reach TMDB: ${e}`;
    } finally {
      checking = false;
    }
  }
</script>

<div class="wrap">
  <div class="panel box">
    <div class="wordmark">Show<span>Track</span></div>
    <p class="dim">
      ShowTrack needs a free TMDB API key for posters, episode data and search.
      Create an account at
      <a class="link" href="https://www.themoviedb.org/settings/api" target="_blank" rel="noopener noreferrer">themoviedb.org/settings/api</a>
      and paste your key (v3) or Read Access Token (v4) below.
    </p>
    <input placeholder="TMDB API key or token" bind:value={key} />
    {#if error}<p class="err">{error}</p>{/if}
    <button class="btn-gold" onclick={validate} disabled={checking || key.trim() === ""}>
      {checking ? "Checking…" : "Validate & save"}
    </button>
    <p class="dim">Next step: import your TV Time export from the Settings page.</p>
  </div>
</div>

<style>
  .wrap { display: flex; align-items: center; justify-content: center; min-height: 80vh; }
  .box {
    max-width: 480px; display: flex; flex-direction: column; gap: 14px; padding: 28px;
    animation: rise 0.45s cubic-bezier(0.2, 0.7, 0.3, 1) both;
  }
  .wordmark {
    font-family: "Anton", sans-serif; font-size: 26px; letter-spacing: 0.04em; text-transform: uppercase;
  }
  .wordmark span { color: var(--gold); }
  .link { color: var(--gold); }
  .err { color: var(--red); margin: 0; }
</style>
