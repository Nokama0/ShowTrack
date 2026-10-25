<script lang="ts">
  import { imgUrl } from "#lib/api.js";
  let {
    path,
    size = "w342",
    alt = "",
    glow = false,
  }: {
    path: string | null | undefined;
    size?: "w185" | "w342" | "w780" | "original";
    alt?: string;
    glow?: boolean;
  } = $props();

  const url = $derived(imgUrl(size, path));
  const NEUTRAL = "0 16px 44px -12px rgba(148,163,184,.30)";
  let shadow = $state(NEUTRAL);

  /** Average color pushed to a vivid glow (sat >= .55, lightness .55). */
  function boost(r: number, g: number, b: number): [number, number, number] {
    const max = Math.max(r, g, b);
    const min = Math.min(r, g, b);
    const d = max - min;
    let h = 0;
    if (d !== 0) {
      if (max === r) h = ((g - b) / d) % 6;
      else if (max === g) h = (b - r) / d + 2;
      else h = (r - g) / d + 4;
      h *= 60;
      if (h < 0) h += 360;
    }
    let s = max === 0 ? 0 : d / max;
    s = Math.max(s, 0.55);
    const l = 0.55;
    const c = (1 - Math.abs(2 * l - 1)) * s;
    const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
    const m = l - c / 2;
    const [rr, gg, bb] =
      h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x]
      : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
    return [Math.round((rr + m) * 255), Math.round((gg + m) * 255), Math.round((bb + m) * 255)];
  }

  function sample(e: Event) {
    if (!glow) return;
    try {
      const img = e.currentTarget as HTMLImageElement;
      const canvas = document.createElement("canvas");
      canvas.width = 12;
      canvas.height = 18;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      ctx.drawImage(img, 0, 0, 12, 18);
      const data = ctx.getImageData(0, 0, 12, 18).data;
      let r = 0, g = 0, b = 0;
      const n = data.length / 4;
      for (let i = 0; i < data.length; i += 4) {
        r += data[i]; g += data[i + 1]; b += data[i + 2];
      }
      const ar = r / n, ag = g / n, ab = b / n;
      if (Math.max(ar, ag, ab) - Math.min(ar, ag, ab) < 12) return; // achromatic poster — keep the neutral glow
      const [br, bg, bb] = boost(ar, ag, ab);
      shadow = `0 16px 44px -12px rgba(${br},${bg},${bb},.38)`;
    } catch {
      // tainted canvas / decode failure — keep the neutral glow
    }
  }
</script>

{#if url}
  <div class="wrap" style={glow ? `box-shadow:${shadow}` : ""}>
    <img src={url} {alt} loading="lazy" crossorigin="anonymous" onload={sample} />
    <span class="fx"></span>
  </div>
{:else}
  <div class="wrap fallback" style={glow ? `box-shadow:${NEUTRAL}` : ""}>
    <b>{alt}</b>
    <span class="fx"></span>
  </div>
{/if}

<style>
  .wrap {
    position: relative; aspect-ratio: 2 / 3; border-radius: var(--r-lg);
    overflow: hidden; background: var(--raise-hi);
    transition: box-shadow 0.5s ease;
  }
  img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .fx {
    position: absolute; inset: 0; pointer-events: none;
    background:
      repeating-linear-gradient(115deg, rgba(255, 255, 255, 0.03) 0 1px, transparent 1px 4px),
      linear-gradient(180deg, transparent 45%, rgba(0, 0, 0, 0.5));
  }
  .fallback {
    display: flex; align-items: flex-end; padding: 13px;
    background: linear-gradient(155deg, #1c2027, #08090c 65%);
  }
  .fallback b {
    font-family: "Anton", sans-serif; font-weight: 400; font-size: 19px;
    line-height: 1.05; letter-spacing: 0.02em; text-transform: uppercase;
    color: rgba(255, 255, 255, 0.92); position: relative; z-index: 1;
  }
</style>
