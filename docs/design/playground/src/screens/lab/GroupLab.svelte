<script lang="ts">
  /* Group-call lab (`?lab=group`): one group, "Weekend hike", seen by three members at once. Arjun's laptop is the
     group's call host (groups are hosted on a computer); Maya and Sam are on phones. Start a call in any window
     (open Weekend hike, tap the call button), join from the others, mute, leave. The "talks" switches make a person
     speak in every window; the host buttons play what happens to Arjun's laptop. lib/sim/group.svelte.ts links them.
     Design simulation only (not to port). */
  import { onMount } from 'svelte';
  let theme = $state<'light' | 'dark'>('light');
  let key = $state(0);
  let icons = $state<'tin' | 'cur'>('tin');
  const room = Math.random().toString(36).slice(2, 8);
  const ch = new BroadcastChannel('tinline-group-' + room);
  const WHO = [
    { id: 'maya', label: 'Maya · phone', form: 'android' },
    { id: 'arjun', label: 'Arjun · laptop, the call host', form: 'desktop' },
    { id: 'sam', label: 'Sam · phone', form: 'android' },
  ] as const;
  let talking = $state<Record<string, boolean>>({});
  const src = (w: (typeof WHO)[number]) => `/?form=${w.form}&theme=${theme}&gme=${w.id}&icons=${icons}&room=${room}&k=${key}`;
  const talk = (id: string) => { talking[id] = !talking[id]; ch.postMessage({ t: 'ctl', what: 'talk', who: id, on: talking[id] }); };
  /* Each person's line to the host: Strong → Good → Fair → Weak → Strong. Maya's also goes relayed below Good. */
  const WORD = { 4: 'strong', 3: 'good', 2: 'fair', 1: 'weak' } as Record<number, string>;
  let lines = $state<Record<string, number>>({});
  const line = (id: string) => { const n = (lines[id] ?? 4) === 1 ? 4 : (lines[id] ?? 4) - 1; lines[id] = n; ch.postMessage({ t: 'ctl', what: 'line', who: id, level: n, relayed: n <= 2 }); };
  const host = (kind: string) => ch.postMessage({ t: 'ctl', what: 'host', kind });
  function reset() { talking = {}; lines = {}; key++; }
  // A code change restarts all three windows together (a hot update would split a window's modules; see PairLab).
  let reloadT = 0;
  const later = () => { clearTimeout(reloadT); reloadT = window.setTimeout(reset, 300); };
  if (import.meta.hot) { import.meta.hot.on('vite:beforeUpdate', later); import.meta.hot.on('vite:beforeFullReload', later); }
  onMount(() => { document.title = 'Group call lab'; });
</script>

<div class="lab" data-theme={theme}>
  <header>
    <b>A group call, three members</b>
    <span class="hint">Open Weekend hike in any window and start a call; join from the others. Arjun’s laptop hosts it.</span>
    <div class="ctl">
      <button type="button" onclick={() => (theme = theme === 'light' ? 'dark' : 'light')}>{theme === 'light' ? 'Dark' : 'Light'}</button>
      <button type="button" onclick={() => { icons = icons === 'tin' ? 'cur' : 'tin'; key++; }}>Icons: {icons === 'tin' ? 'tin' : 'current'}</button>
      <span class="sep"></span>
      {#each WHO as w (w.id)}
        <button type="button" aria-pressed={!!talking[w.id]} onclick={() => talk(w.id)}>{w.id[0].toUpperCase() + w.id.slice(1)} {talking[w.id] ? 'talks' : 'is quiet'}</button>
      {/each}
      <span class="sep"></span>
      {#each WHO.filter((w) => w.id !== 'arjun') as w (w.id)}
        <button type="button" onclick={() => line(w.id)}>{w.id[0].toUpperCase() + w.id.slice(1)}’s line: {WORD[lines[w.id] ?? 4]}</button>
      {/each}
      <span class="sep"></span>
      <button type="button" onclick={() => host('lowBattery')}>Laptop battery low</button>
      <button type="button" onclick={() => host('limited')}>Hosting limited</button>
      <button type="button" onclick={() => host('recover')}>Laptop back to normal</button>
      <button type="button" onclick={() => host('lost')}>Laptop drops off</button>
      <span class="sep"></span>
      <button type="button" onclick={reset}>Start over</button>
    </div>
  </header>
  <main>
    {#key key}
      {#each WHO as w (w.id)}
        <figure class:wide={w.form === 'desktop'}><figcaption>{w.label}</figcaption><iframe title={w.id} src={src(w)}></iframe></figure>
      {/each}
    {/key}
  </main>
</div>

<style>
  .lab { min-block-size: 100vh; background: #e9ece9; color: #17201d; font: 14px/1.4 system-ui, sans-serif; }
  .lab[data-theme='dark'] { background: #0d1210; color: #e6ece9; }
  header { position: sticky; top: 0; z-index: 1; display: grid; gap: 6px; padding: 12px 16px; background: inherit; border-block-end: 1px solid #8884; }
  .hint { opacity: .75; }
  .ctl { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  .ctl button { border: 1px solid #8886; background: transparent; color: inherit; border-radius: 10px; padding: 5px 10px; font: inherit; cursor: pointer; }
  .ctl button[aria-pressed='true'] { background: #c9a2274d; }
  .sep { inline-size: 1px; block-size: 20px; background: #8885; margin-inline: 4px; }
  main { display: grid; grid-template-columns: 460px minmax(760px, 1fr) 460px; gap: 16px; padding: 16px; overflow-x: auto; }
  figure { margin: 0; display: grid; gap: 6px; min-inline-size: 0; }
  figcaption { font-weight: 600; opacity: .8; }
  iframe { inline-size: 100%; block-size: 900px; border: 0; border-radius: 16px; background: transparent; }
</style>
