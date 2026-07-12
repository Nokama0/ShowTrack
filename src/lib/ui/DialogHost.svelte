<script lang="ts">
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { dialog, settleDialog } from "$lib/dialog.svelte";

  function onKey(e: KeyboardEvent) {
    if (dialog.current && e.key === "Escape") {
      e.preventDefault();
      settleDialog(null);
    }
  }
</script>

<svelte:window onkeydown={onKey} />

{#if dialog.current}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="dlg-overlay" transition:fade={{ duration: 160 }} onclick={() => settleDialog(null)} role="presentation">
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div
      class="dlg"
      role="dialog"
      tabindex="-1"
      aria-modal="true"
      aria-label={dialog.current.title}
      in:fly={{ y: 14, duration: 220, easing: cubicOut }}
      out:fly={{ y: 10, duration: 150, easing: cubicOut }}
      onclick={(e) => e.stopPropagation()}
    >
      <h2>{dialog.current.title}</h2>
      <p>{dialog.current.message}</p>
      <div class="dlg-actions">
        {#each dialog.current.buttons as b (b.value)}
          <button class={b.kind} onclick={() => settleDialog(b.value)}>
            {b.label}
            {#if b.hint}<span class="hintdot" title={b.hint}>?</span>{/if}
          </button>
        {/each}
      </div>
    </div>
  </div>
{/if}

<style>
  .dlg-overlay {
    position: fixed; inset: 0; z-index: 60;
    background: rgba(4, 6, 11, 0.66);
    display: flex; align-items: center; justify-content: center;
    padding: 20px;
  }
  .dlg {
    width: min(420px, 100%);
    background: var(--raise);
    border: 1px solid var(--line);
    border-radius: var(--r-lg, 16px);
    padding: 22px 22px 18px;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.5);
  }
  h2 {
    font-family: "Anton", sans-serif;
    font-size: 19px; font-weight: 400; letter-spacing: 0.02em;
    margin: 0 0 8px;
  }
  p { color: var(--dim); font-size: 14px; line-height: 1.5; margin: 0 0 18px; }
  .dlg-actions { display: flex; flex-direction: column; gap: 8px; }
  .dlg-actions button {
    position: relative;
    width: 100%; padding: 10px 34px; border-radius: 12px;
    font-size: 14px; font-weight: 600;
    transition: filter 0.15s ease, background-color 0.15s ease, transform 0.1s ease;
  }
  /* absolute so the label stays centered like hint-less buttons */
  .hintdot {
    position: absolute; right: 10px; top: 50%; translate: 0 -50%;
    width: 18px; height: 18px; border-radius: 50%;
    display: flex; align-items: center; justify-content: center;
    font-size: 11px; font-weight: 700;
    background: rgba(0, 0, 0, 0.22); cursor: help;
  }
  .dlg-actions button:active { transform: scale(0.98); }
  .ghost { background: var(--raise-hi); color: var(--dim); }
  .ghost:hover { color: var(--text); }
  .gold { background: var(--gold); color: #14100a; }
  .gold:hover { filter: brightness(1.08); }
  .danger { background: var(--red); color: #fff; }
  .danger:hover { filter: brightness(1.1); }
</style>
