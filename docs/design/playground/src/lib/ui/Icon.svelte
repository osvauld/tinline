<script lang="ts">
  import { ICONS } from './icons';
  let { name, size = 22, label, class: cls = '' }: { name: string; size?: number; label?: string; class?: string } = $props();
</script>

<svg
  class="i {cls}" width={size} height={size} viewBox="0 0 24 24"
  role={label ? 'img' : undefined} aria-label={label} aria-hidden={label ? undefined : 'true'} focusable="false"
>{@html ICONS[name] ?? ''}</svg>

<style>
  .i { fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; flex: none; }
</style>
