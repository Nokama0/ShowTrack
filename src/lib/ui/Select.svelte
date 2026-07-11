<script lang="ts">
  let {
    value,
    options,
    onchange,
    label = "Select",
  }: {
    value: string;
    options: { value: string; label: string }[];
    onchange: (v: string) => void;
    label?: string;
  } = $props();

  let open = $state(false);
  const current = $derived(options.find((o) => o.value === value)?.label ?? value);

  function pick(v: string) {
    open = false;
    if (v !== value) onchange(v);
  }
</script>

<svelte:window
  onclick={() => (open = false)}
  onkeydown={(e) => {
    if (e.key === "Escape") open = false;
  }}
/>

<div class="sel">
  <button
    class="selbtn"
    aria-label={label}
    aria-expanded={open}
    onclick={(e) => {
      e.stopPropagation();
      open = !open;
    }}
  >
    <span>{current}</span>
    <span class="chev" class:up={open} aria-hidden="true">▾</span>
  </button>
  {#if open}
    <div class="pop" role="listbox" aria-label={label}>
      {#each options as o (o.value)}
        <button
          class="opt"
          class:sel={o.value === value}
          role="option"
          aria-selected={o.value === value}
          onclick={(e) => {
            e.stopPropagation();
            pick(o.value);
          }}
        >
          {o.label}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .sel { position: relative; min-width: 170px; }
  .selbtn {
    width: 100%; display: flex; justify-content: space-between; align-items: center; gap: 10px;
    background: var(--raise); border: 1px solid var(--line); border-radius: var(--r-md);
    padding: 9px 12px; color: var(--text); font-weight: 500; text-align: left;
  }
  .selbtn:hover { border-color: var(--faint); filter: none; }
  .chev {
    color: var(--dim); font-size: 11px;
    transition: transform 0.25s cubic-bezier(0.2, 0.7, 0.3, 1);
  }
  .chev.up { transform: rotate(180deg); }
  .pop {
    position: absolute; top: calc(100% + 6px); left: 0; right: 0; z-index: 20;
    background: var(--raise-hi); border: 1px solid var(--line); border-radius: var(--r-md);
    padding: 5px; display: flex; flex-direction: column; gap: 2px;
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.5);
    animation: rise 0.22s cubic-bezier(0.2, 0.7, 0.3, 1) both;
  }
  .opt {
    text-align: left; background: none; padding: 8px 10px; border-radius: 8px;
    color: var(--dim); font-weight: 500;
  }
  .opt:hover { background: var(--raise); color: var(--text); filter: none; }
  .opt.sel { color: var(--gold); background: var(--gold-dim); }
</style>
