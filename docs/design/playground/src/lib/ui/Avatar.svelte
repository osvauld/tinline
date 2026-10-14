<script lang="ts">
  /* The one avatar. Initials by default; with `person` (a person id, or 'me') it also shows that person's profile picture:
     ready -> the image; selected but not cached yet -> initials with a faint breathing ring (no spinner, no percentage);
     removed or broken -> initials. Group avatars never take a person. */
  import Icon from './Icon.svelte';
  import { initials } from '../model/store.svelte';
  import { pictureFor } from '../model/actions/profile.svelte';
  import { avatarArt, strokePath, syntheticDid, SMALL_BELOW_PX } from './fingerprint';
  let { name, hue = 0, size = 48, group = false, person }: { name: string; hue?: number; size?: number; group?: boolean; person?: string } = $props();

  const pic = $derived(!group && person ? pictureFor(person) : { state: 'none' as const });
  /* An image that fails to decode falls back to initials, until a different picture is selected. */
  let badUrl = $state('');
  const url = $derived(pic.state === 'ready' && pic.url !== badUrl ? pic.url : undefined);
  /* No picture: the person's fingerprint mandala (docs/design/FINGERPRINT-AVATARS.md). Playground people get a synthetic DID. */
  const art = $derived(!group && person ? avatarArt(syntheticDid(person), size < SMALL_BELOW_PX) : null);
</script>

<span
  class="t-avatar {group ? 'grp' : art ? 'fp' : 'av' + (hue % 6)}" class:wait={pic.state === 'awaiting'} class:pic={!!url}
  style:width="{size}px" style:height="{size}px" style:font-size="{Math.round(size * 0.34)}px"
  style:--fpbl={art?.light.bg} style:--fpfl={art?.light.fg} style:--fpbd={art?.dark.bg} style:--fpfd={art?.dark.fg}
  data-pic={pic.state === 'none' ? undefined : pic.state}
  aria-hidden="true"
>
  {#if group}<Icon name="users" size={Math.round(size * 0.46)} />{:else if art}
    <svg viewBox="0 0 100 100" width={size} height={size} fill="none" stroke-width={art.strokeWidth} stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      {#each art.strokes as st, i (i)}<path d={strokePath(st)} stroke={st.gold ? 'var(--gold)' : 'var(--fpfg)'} opacity={st.alpha} />{/each}
    </svg>
  {:else}{initials(name)}{/if}
  {#if url}<img src={url} alt="" draggable="false" onerror={() => (badUrl = url)} />{/if}
</span>

<style>
  .t-avatar { position: relative; overflow: hidden; }
  .fp { --fpbg: var(--fpbl); --fpfg: var(--fpfl); background: var(--fpbg); }
  .fp svg { position: absolute; inset: 0; display: block; }
  :global(:root[data-theme='dark']) .fp { --fpbg: var(--fpbd); --fpfg: var(--fpfd); }
  @media (prefers-color-scheme: dark) { :global(:root:not([data-theme='light'])) .fp { --fpbg: var(--fpbd); --fpfg: var(--fpfd); } }
  .grp { background: var(--surface-2); color: var(--accent-ink); }
  img { position: absolute; inset: 0; inline-size: 100%; block-size: 100%; object-fit: cover; border-radius: inherit; animation: arrive var(--dur-crossfade) var(--ease-out); }
  /* A hairline over the picture, so a pale photo doesn't dissolve into a pale surface (and a dark one keeps an edge). */
  .pic::after { content: ''; position: absolute; inset: 0; border-radius: inherit; pointer-events: none; box-shadow: inset 0 0 0 1px rgb(0 0 0 / .1); }
  :global(:root[data-theme='dark']) .pic::after { box-shadow: inset 0 0 0 1px rgb(255 255 255 / .12); }
  @media (prefers-color-scheme: dark) { :global(:root:not([data-theme='light'])) .pic::after { box-shadow: inset 0 0 0 1px rgb(255 255 255 / .12); } }
  /* Awaiting: the ring slowly breathes in and out. Static when motion is reduced. */
  .wait::after { content: ''; position: absolute; inset: 0; border-radius: inherit; pointer-events: none; opacity: .5;
    box-shadow: inset 0 0 0 max(1.5px, 0.03em) color-mix(in srgb, currentColor 45%, transparent); animation: breathe calc(var(--dur-overlayOpen) * 12) ease-in-out infinite; /* ~2.9 s, scaled by slow motion */ }
  :global(:root[data-motion='reduce']) .wait::after { animation: none; }
  @keyframes arrive { from { opacity: 0; } }
  @keyframes breathe { 50% { opacity: .12; } }
</style>
