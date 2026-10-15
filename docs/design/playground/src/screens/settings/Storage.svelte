<script lang="ts">
  /* Settings > Storage (2026-10-15). What Tinline keeps on this device, one bar, then the two things I can do about it:
     when pictures download, and clearing the cache. Chats and files are only removed from their chat (not here). */
  import Icon from '../../lib/ui/Icon.svelte';
  import { device, fmtBytes, type AutoDl } from '../../lib/model/device.svelte';
  import { gate } from '../../lib/model/actions/gate.svelte';

  const KINDS = [
    { k: 'pictures', label: 'Pictures', c: 'var(--accent)' },
    { k: 'files', label: 'Files', c: 'var(--gold)' },
    { k: 'chats', label: 'Messages', c: 'var(--muted)' },
    { k: 'cache', label: 'Cache', c: 'var(--line)' },
  ] as const;
  const total = $derived(KINDS.reduce((n, x) => n + device.used[x.k], 0));
  const DL: { k: AutoDl; label: string; sub: string }[] = [
    { k: 'always', label: 'Always', sub: 'On Wi-Fi and mobile data' },
    { k: 'wifi', label: 'On Wi-Fi', sub: 'On mobile data they wait for a tap' },
    { k: 'never', label: 'When I tap them', sub: 'Each shows a soft preview until then' },
  ];
  let cleared = $state(false);
</script>

<div class="pf-scroll" data-storage>
<div class="pf-col">
  <section class="pf-sec first" aria-labelledby="st-h">
    <h2 id="st-h">On this {gate.noun}</h2>
    <div class="pf-card use">
      <p class="tot"><b>{fmtBytes(total)}</b></p>
      <div class="bar" aria-hidden="true">{#each KINDS as x (x.k)}{#if device.used[x.k]}<span style:flex-grow={device.used[x.k]} style:background={x.c}></span>{/if}{/each}</div>
      <ul class="legend">
        {#each KINDS as x (x.k)}<li><span class="sw" style:background={x.c}></span>{x.label}<span class="n">{fmtBytes(device.used[x.k])}</span></li>{/each}
      </ul>
    </div>
  </section>

  <section class="pf-sec" aria-labelledby="dl-h">
    <h2 id="dl-h">Download pictures</h2>
    <div class="pf-card" role="radiogroup" aria-label="Download pictures">
      {#each DL as d (d.k)}
        <div class="pf-act opt">
          <button type="button" role="radio" class="pick press" aria-checked={device.autoDl === d.k} onclick={() => (device.autoDl = d.k)}>
            <span class="radio" aria-hidden="true"></span><span class="t"><b>{d.label}</b><small>{d.sub}</small></span>
          </button>
        </div>
      {/each}
    </div>
  </section>

  <section class="pf-sec">
    <div class="pf-card">
      <button type="button" class="pf-act press" disabled={!device.used.cache} onclick={() => { device.clearCache(); cleared = true; }}>
        <Icon name="trash" size={20} /><span class="t"><b>Clear cache</b><small>{device.used.cache ? `${fmtBytes(device.used.cache)} of previews and pictures you’ve seen. They download again when you open them.` : cleared ? 'Cleared' : 'Nothing to clear'}</small></span>
      </button>
    </div>
    <p class="pf-note">To free more space, remove pictures or files from their chat.</p>
  </section>
</div>
</div>

<style>
  .first { margin-block-start: 16px; }
  .use { padding: 16px; display: grid; gap: 12px; }
  .tot { margin: 0; font-size: var(--fs-title); font-variant-numeric: tabular-nums; }
  .bar { display: flex; gap: 2px; block-size: 10px; border-radius: 5px; overflow: hidden; background: var(--surface-2); }
  .bar span { flex-basis: 0; min-inline-size: 3px; transition: flex-grow var(--dur-switch) var(--ease-out); }
  .legend { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; font-size: var(--fs-sub); }
  .legend li { display: flex; align-items: center; gap: 8px; }
  .legend .sw { inline-size: 10px; block-size: 10px; border-radius: 3px; }
  .legend .n { margin-inline-start: auto; color: var(--muted); font-variant-numeric: tabular-nums; }
  .opt { padding: 0; }
  .pick { display: flex; align-items: center; gap: 14px; inline-size: 100%; min-block-size: 56px; padding: 8px 16px; border: 0; background: none; color: inherit; font: inherit; text-align: start; cursor: pointer; }
  .pick:hover { background: var(--surface-2); }
  .radio { flex: none; inline-size: 22px; block-size: 22px; border-radius: 50%; box-shadow: inset 0 0 0 2px var(--line); display: grid; place-items: center; transition: box-shadow var(--dur-switch) var(--ease-out); }
  .radio::after { content: ''; inline-size: 12px; block-size: 12px; border-radius: 50%; background: var(--accent); transform: scale(0); transition: transform var(--dur-switch) var(--ease-out); }
  [aria-checked='true'] .radio { box-shadow: inset 0 0 0 2px var(--accent); }
  [aria-checked='true'] .radio::after { transform: none; }
</style>
