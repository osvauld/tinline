<script lang="ts">
  /* Icons: Tinline's own "tin" glyphs (the icon study, screens/lab/tinline-icons.ts `OWN`, drawn from the mark's
     maths) wherever one exists, else the current set (icons.ts). Trial switch: `?icons=cur` shows the old set only,
     `?icons=gold` draws the string parts in gold. The tin set is still a study: icons.ts stays the port source until
     the user picks a set. */
  import { ICONS } from './icons';
  import { OWN } from '../../screens/lab/tinline-icons';
  let { name, size = 22, label, class: cls = '' }: { name: string; size?: number; label?: string; class?: string } = $props();
  const mode = new URLSearchParams(location.search).get('icons');
  const body = $derived(mode !== 'cur' && OWN[name]
    ? OWN[name].parts().map(([d, c]) => `<path${c ? ` class="${c}"` : ''} d="${d}"/>`).join('')
    : (ICONS[name] ?? ''));
</script>

<svg
  class="i {cls}" class:gold={mode === 'gold'} width={size} height={size} viewBox="0 0 24 24"
  role={label ? 'img' : undefined} aria-label={label} aria-hidden={label ? undefined : 'true'} focusable="false"
>{@html body}</svg>

<style>
  .i { fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; flex: none; }
  .i :global(.f) { fill: currentColor; stroke: none; }
  .i.gold :global(.str) { stroke: var(--gold); }
</style>
