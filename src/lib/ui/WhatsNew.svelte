<script lang="ts">
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { session } from "$lib/session.svelte";

  const latest = $derived(session.whatsNew[0]?.version ?? "");
  const multi = $derived(session.whatsNew.length > 1);

  function dismiss() {
    session.whatsNew = [];
  }
  function onKey(e: KeyboardEvent) {
    if (session.whatsNew.length > 0 && e.key === "Escape") {
      e.preventDefault();
      dismiss();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

{#if session.whatsNew.length > 0}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="overlay" transition:fade={{ duration: 200 }} onclick={dismiss} role="presentation">
    <div
      class="panel"
      in:fly={{ y: 16, duration: 300, easing: cubicOut }}
      out:fly={{ y: 10, duration: 180, easing: cubicOut }}
      onclick={(e) => e.stopPropagation()}
      role="dialog"
      aria-modal="true"
      aria-label="What's new"
      tabindex="-1"
    >
      <div class="head">
        <div class="eyebrow">
          What's <b>new</b>
          <span class="ver dim">· {multi ? `updated to ${latest}` : latest}</span>
        </div>
        <button class="close" onclick={dismiss} aria-label="Dismiss what's new">✕</button>
      </div>
      <div class="body">
        {#each session.whatsNew as entry (entry.version)}
          <div class="ver-block">
            {#if multi}<div class="vlabel">{entry.version}</div>{/if}
            <ul>
              {#each entry.notes as note}<li>{note}</li>{/each}
            </ul>
          </div>
        {/each}
      </div>
      <button class="btn-gold got" onclick={dismiss}>Got it</button>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed; inset: 0; z-index: 50;
    background: rgba(4, 6, 11, 0.66);
    display: flex; align-items: center; justify-content: center;
    padding: 20px;
  }
  .panel {
    width: min(460px, 100%); max-height: 82vh;
    display: flex; flex-direction: column;
    border-radius: 18px; border: 1px solid rgba(255, 207, 110, 0.35);
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.5);
    background:
      radial-gradient(120% 160% at 90% 0%, rgba(255, 207, 110, 0.12), transparent 55%),
      linear-gradient(105deg, #0c1018 45%, #101623);
    padding: 22px 24px 20px;
  }
  .head { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; margin-bottom: 14px; }
  .eyebrow {
    font-family: "Anton", sans-serif; font-size: 13px; letter-spacing: 0.18em;
    text-transform: uppercase; color: var(--dim);
  }
  .eyebrow b { color: var(--gold); font-weight: 400; }
  .ver { font-family: "Archivo Variable", sans-serif; font-size: 12.5px; letter-spacing: 0; text-transform: none; margin-left: 4px; }
  .close {
    background: none; color: var(--dim); font-size: 13px; padding: 4px 9px; border-radius: 8px;
    transition: color 0.15s ease, background-color 0.15s ease;
  }
  .close:hover { color: var(--text); background: var(--raise-hi); }
  .body { overflow-y: auto; overscroll-behavior: contain; }
  .ver-block + .ver-block { margin-top: 14px; }
  .vlabel {
    font-family: "Anton", sans-serif; font-size: 12px; letter-spacing: 0.05em;
    color: var(--gold); margin-bottom: 5px;
  }
  ul { margin: 0; padding-left: 20px; display: flex; flex-direction: column; gap: 6px; }
  li { font-size: 13.5px; line-height: 1.45; color: var(--text); }
  li::marker { color: var(--gold); }
  .got { margin-top: 18px; align-self: flex-start; padding: 9px 22px; font-size: 13.5px; }
  @media (max-width: 760px) {
    .overlay {
      padding: calc(20px + env(safe-area-inset-top)) 18px calc(20px + env(safe-area-inset-bottom));
    }
    .panel { width: 100%; max-width: none; max-height: 86vh; }
    .got { width: 100%; justify-content: center; align-self: stretch; }
  }
</style>
