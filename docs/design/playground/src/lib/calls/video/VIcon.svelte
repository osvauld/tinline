<script lang="ts">
  /* Video-call glyphs (kept here so the shared icon table is untouched): same 24 grid and 1.8 stroke as ui/Icon. */
  let { name, size = 22, class: cls = '' }: { name: 'video' | 'videooff' | 'flip'; size?: number; class?: string } = $props();
  const P = {
    video: '<rect x="3" y="6.5" width="12.5" height="11" rx="3"/><path d="M15.5 10.6l4.4-2.5a.6.6 0 0 1 .9.5v6.8a.6.6 0 0 1-.9.5l-4.4-2.5"/>',
    videooff: '<path d="M10 6.5H6.5A3.5 3.5 0 0 0 3 10v4a3.5 3.5 0 0 0 3.5 3.5h6A3 3 0 0 0 15.5 14.5v-4M15.5 10.6l4.4-2.5a.6.6 0 0 1 .9.5v6.8a.6.6 0 0 1-.9.5l-.4-.2"/><path d="M4 4l16 16"/>',
    flip: '<path d="M19.5 10.5A8 8 0 0 0 5.2 8"/><path d="M5 4v4.2h4.2"/><path d="M4.5 13.5A8 8 0 0 0 18.8 16"/><path d="M19 20v-4.2h-4.2"/>'
  };
</script>

<svg class="i {cls}" width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" focusable="false">{@html P[name]}</svg>

<style>
  .i { fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; flex: none; }
</style>
