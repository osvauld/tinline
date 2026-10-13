<script lang="ts">
  /* A stand-in QR: deterministic modules from a seed, three real-looking finder squares. Not scannable, by design. */
  let { seed = 1, dim = false, size = 208, label = 'QR code to link this device' }: { seed?: number; dim?: boolean; size?: number; label?: string } = $props();
  const N = 29;
  const cells = $derived.by(() => {
    let s = (seed * 2654435761) >>> 0;
    const rnd = () => ((s = (s * 1664525 + 1013904223) >>> 0), s / 4294967296);
    const finder = (x: number, y: number) => (x < 8 && y < 8) || (x >= N - 8 && y < 8) || (x < 8 && y >= N - 8);
    const out: string[] = [];
    for (let y = 0; y < N; y++) for (let x = 0; x < N; x++) if (!finder(x, y) && rnd() > 0.52) out.push(`M${x} ${y}h1v1h-1z`);
    return out.join('');
  });
  const fp = (x: number, y: number) => `M${x} ${y}h7v7h-7zM${x + 1} ${y + 1}v5h5v-5zM${x + 2} ${y + 2}h3v3h-3z`;
</script>

<svg viewBox="-2 -2 {N + 4} {N + 4}" width={size} height={size} role="img" aria-label={label} class:dim shape-rendering="crispEdges">
  <rect x="-2" y="-2" width={N + 4} height={N + 4} fill="#fff" />
  <path d={cells} fill="#17201d" />
  <path d="{fp(0, 0)}{fp(N - 7, 0)}{fp(0, N - 7)}" fill="#17201d" fill-rule="evenodd" />
</svg>

<style>
  svg { border-radius: 12px; display: block; max-inline-size: 100%; block-size: auto; }
  .dim { opacity: .18; }
</style>
