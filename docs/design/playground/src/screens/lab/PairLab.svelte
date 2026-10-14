<script lang="ts">
  /* Two-phone lab (`?lab=pair`): Maya (left, a brand-new account) adds Tara (right, someone who already uses Tinline),
     then they message and call. Each side is a whole playground window; lib/sim/pair.svelte.ts links them.
     The buttons below act as "the world": Tara's phone going off, her code being used by someone else, and so on.
     Design simulation only (not to port). */
  let theme = $state<'light' | 'dark'>('light');
  let tara = $state<'android' | 'desktop'>('android');
  let key = $state(0);
  /* Icon set in both windows: the tin study (default) or the current set, to compare in place. */
  let icons = $state<'tin' | 'cur'>('tin');
  let off = $state(false), blocked = $state(false);
  // One channel per lab tab: a second (or stale) lab tab must not answer this tab's phones.
  const room = Math.random().toString(36).slice(2, 8);
  const ch = new BroadcastChannel('tinline-pair-' + room);
  const ctl = (what: string) => ch.postMessage({ t: 'ctl', to: 'tara', what });
  const left = $derived(`/?form=android&theme=${theme}&me=maya&step=done&icons=${icons}&room=${room}&k=${key}`);
  const right = $derived(`/?form=${tara}&theme=${theme}&me=tara&icons=${icons}&room=${room}&k=${key}`);
  // Maya's window may load after Tara's: ask Tara to say her code again once both are up.
  const loaded = () => setTimeout(() => ctl('announce'), 400);
  function reset() { off = false; blocked = false; key++; }
  /* Any code change restarts both windows together: a hot update gives a window a second copy of the edited module while
     the link (pair.svelte.ts) keeps the first, so a half-updated pair misbehaves (requests not arriving, your own code
     accepted as someone else's). Every page gets Vite's update events, so the lab listens itself. */
  let reloadT = 0;
  const later = () => { clearTimeout(reloadT); reloadT = window.setTimeout(reset, 300); };
  if (import.meta.hot) { import.meta.hot.on('vite:beforeUpdate', later); import.meta.hot.on('vite:beforeFullReload', later); }
</script>

<div class="lab" data-theme={theme}>
  <header>
    <b>Adding one contact, both sides</b>
    <span class="hint">Left: Maya signs up, then adds Tara. Right: Tara, already on Tinline. Copy Tara’s code on the right, paste it on the left.</span>
    <div class="ctl">
      <button type="button" onclick={() => (theme = theme === 'light' ? 'dark' : 'light')}>{theme === 'light' ? 'Dark' : 'Light'}</button>
      <button type="button" onclick={() => { tara = tara === 'android' ? 'desktop' : 'android'; key++; }}>Tara on {tara === 'android' ? 'a computer' : 'a phone'}</button>
      <button type="button" onclick={() => { icons = icons === 'tin' ? 'cur' : 'tin'; key++; }}>Icons: {icons === 'tin' ? 'tin' : 'current'}</button>
      <span class="sep"></span>
      <button type="button" aria-pressed={off} onclick={() => { off = !off; ctl(off ? 'offline' : 'online'); }}>Tara’s device {off ? 'is off' : 'is on'}</button>
      <button type="button" onclick={() => ctl('renew')}>Tara made a new code</button>
      <button type="button" onclick={() => ctl('expire')}>Tara’s code expired</button>
      <button type="button" aria-pressed={blocked} onclick={() => { blocked = !blocked; ctl(blocked ? 'block' : 'unblock'); }}>Tara {blocked ? 'blocks' : 'doesn’t block'} Maya</button>
      <span class="sep"></span>
      <button type="button" onclick={reset}>Start over</button>
    </div>
  </header>
  <main class:wide={tara === 'desktop'}>
    {#key key}
      <figure><figcaption>Maya · new account</figcaption><iframe title="Maya" src={left} onload={loaded}></iframe></figure>
      <figure><figcaption>Tara · already on Tinline</figcaption><iframe title="Tara" src={right} onload={loaded}></iframe></figure>
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
  main { display: grid; grid-template-columns: 460px 460px; gap: 16px; justify-content: center; padding: 16px; }
  main.wide { grid-template-columns: 460px minmax(0, 1fr); }
  figure { margin: 0; display: grid; gap: 6px; }
  figcaption { font-weight: 600; opacity: .8; }
  iframe { inline-size: 100%; block-size: 900px; border: 0; border-radius: 16px; background: transparent; }
</style>
