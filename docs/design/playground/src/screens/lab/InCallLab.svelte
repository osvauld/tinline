<script lang="ts">
  /* In-call lab (`?lab=incall`): how do I send a message while on a call (Android)? Three phones side by side, each
     with its own choices (lib/calls/incall.svelte.ts); the scene buttons play the same moment on all three, then each
     phone stays fully usable by hand. Design study only (not to port). */
  type Opts = { msg: boolean; back: 'where' | 'chat'; strip: 'top' | 'bottom'; end: boolean; typing: 'keep' | 'slim' | 'hide'; header: boolean };
  const PRESETS: Record<string, { label: string; why: string; o: Opts }> = {
    today: { label: 'Today', why: 'What the playground does now: back to where I was, strip with End, nothing else.',
      o: { msg: false, back: 'where', strip: 'bottom', end: true, typing: 'keep', header: false } },
    keep: { label: 'Message button · strip stays', why: 'Message button on the call screen. While I type, the strip stays as it is, between the composer and the keyboard (I may be writing to someone else). No End on it.',
      o: { msg: true, back: 'where', strip: 'bottom', end: false, typing: 'keep', header: true } },
    slim: { label: 'Message button · strip slims', why: 'Same, but while I type the strip slims to a thin line (name and time) above the keyboard; it grows back when the keyboard goes.',
      o: { msg: true, back: 'where', strip: 'bottom', end: false, typing: 'slim', header: true } },
    message: { label: 'Message button · strip hides', why: 'Same, but the strip hides while I type. In their chat the green header button leads back; in anyone else\'s chat only the notification does.',
      o: { msg: true, back: 'where', strip: 'bottom', end: false, typing: 'hide', header: true } },
    backchat: { label: 'Back goes to the chat', why: 'No new button: ← always lands in their chat, wherever the call started. Same strip and header as the middle phone.',
      o: { msg: false, back: 'chat', strip: 'bottom', end: false, typing: 'hide', header: true } },
  };
  let theme = $state<'light' | 'dark'>('light');
  let phones = $state<Opts[]>([{ ...PRESETS.keep.o }, { ...PRESETS.slim.o }, { ...PRESETS.message.o }]);
  let names = $state<string[]>(['keep', 'slim', 'message']);
  let keys = $state([0, 0, 0]);
  const frames = $state<HTMLIFrameElement[]>([]);

  const src = (o: Opts, k: number) => {
    const ic = [o.msg && 'msg', o.back === 'chat' && 'back-chat', !o.end && 'noend', o.typing === 'hide' && 'kbdhide', o.typing === 'slim' && 'kbdslim', o.header && 'header', 'kbd'].filter(Boolean).join(',');
    return `/?form=android&theme=${theme}&strip=${o.strip}&ic=${ic}&iclab=1&bare=1&k=${k}`;
  };
  const scene = (what: string) => frames.forEach((f) => f?.contentWindow?.postMessage({ t: 'incall', what }, location.origin));
  function preset(i: number, name: string) { names[i] = name; phones[i] = { ...PRESETS[name].o }; keys[i]++; }
  function set<K extends keyof Opts>(i: number, k: K, v: Opts[K]) { phones[i][k] = v; names[i] = 'custom'; keys[i]++; }
</script>

<div class="lab" data-theme={theme}>
  <header>
    <b>Messaging during a call · Android</b>
    <span class="hint">Play a moment on all three phones, then try each by hand. Typing opens a stand-in keyboard. Changing a switch reloads that phone.</span>
    <div class="ctl">
      <span class="k">Start</span>
      <button type="button" onclick={() => scene('from-calls')}>Call from the Calls tab</button>
      <button type="button" onclick={() => scene('from-chat')}>Call from their chat</button>
      <button type="button" onclick={() => scene('incoming')}>They call me (I'm in Contacts)</button>
      <span class="sep"></span>
      <span class="k">Then</span>
      <button type="button" onclick={() => scene('back')}>← Back</button>
      <button type="button" onclick={() => scene('message')}>Message button</button>
      <button type="button" onclick={() => scene('other')}>Open someone else's chat</button>
      <button type="button" onclick={() => scene('type')}>Start typing</button>
      <button type="button" onclick={() => scene('done')}>Close keyboard</button>
      <button type="button" onclick={() => scene('return')}>Return to the call</button>
      <span class="sep"></span>
      <button type="button" onclick={() => scene('reset')}>Reset</button>
      <button type="button" onclick={() => { theme = theme === 'light' ? 'dark' : 'light'; keys = keys.map((k) => k + 1); }}>{theme === 'light' ? 'Dark' : 'Light'}</button>
    </div>
  </header>
  <main>
    {#each phones as o, i (i)}
      <figure>
        <figcaption>
          <select value={names[i]} onchange={(e) => preset(i, e.currentTarget.value)} aria-label="Phone {i + 1} preset">
            {#each Object.entries(PRESETS) as [k, p] (k)}<option value={k}>{p.label}</option>{/each}
            {#if names[i] === 'custom'}<option value="custom">Custom</option>{/if}
          </select>
          <p class="why">{names[i] === 'custom' ? 'Your own mix.' : PRESETS[names[i]].why}</p>
          <div class="sw">
            <label><input type="checkbox" checked={o.msg} onchange={(e) => set(i, 'msg', e.currentTarget.checked)} /> Message button</label>
            <label><input type="checkbox" checked={o.back === 'chat'} onchange={(e) => set(i, 'back', e.currentTarget.checked ? 'chat' : 'where')} /> ← goes to their chat</label>
            <label><input type="checkbox" checked={o.header} onchange={(e) => set(i, 'header', e.currentTarget.checked)} /> Green "Return to call" in their chat</label>
            <label><input type="checkbox" checked={o.end} onchange={(e) => set(i, 'end', e.currentTarget.checked)} /> End on the strip</label>
            <label>While typing <select value={o.typing} onchange={(e) => set(i, 'typing', e.currentTarget.value as Opts['typing'])}><option value="keep">strip stays</option><option value="slim">strip slims</option><option value="hide">strip hides</option></select></label>
            <label><input type="checkbox" checked={o.strip === 'bottom'} onchange={(e) => set(i, 'strip', e.currentTarget.checked ? 'bottom' : 'top')} /> Strip at the bottom</label>
          </div>
        </figcaption>
        {#key keys[i]}<iframe title="Phone {i + 1}" src={src(o, keys[i])} bind:this={frames[i]}></iframe>{/key}
      </figure>
    {/each}
  </main>
</div>

<style>
  .lab { min-block-size: 100vh; background: #e9ece9; color: #17201d; font: 14px/1.4 system-ui, sans-serif; }
  .lab[data-theme='dark'] { background: #0d1210; color: #e6ece9; }
  header { position: sticky; top: 0; z-index: 1; display: grid; gap: 6px; padding: 12px 16px; background: inherit; border-block-end: 1px solid #8884; }
  .hint { opacity: .75; }
  .ctl { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  .ctl .k { font-size: 12px; opacity: .6; text-transform: uppercase; letter-spacing: .04em; }
  .ctl button, select { border: 1px solid #8886; background: transparent; color: inherit; border-radius: 10px; padding: 5px 10px; font: inherit; cursor: pointer; }
  select { font-weight: 650; }
  .sep { inline-size: 1px; block-size: 20px; background: #8885; margin-inline: 4px; }
  main { display: grid; grid-template-columns: repeat(3, minmax(400px, 1fr)); align-items: start; gap: 12px; padding: 12px 16px; }
  figure { margin: 0; display: grid; gap: 8px; align-content: start; }
  figcaption { display: grid; gap: 6px; min-block-size: 150px; align-content: start; }
  .why { margin: 0; font-size: 13px; opacity: .8; }
  .sw { display: grid; grid-template-columns: 1fr 1fr; gap: 2px 10px; font-size: 13px; }
  label { display: flex; gap: 6px; align-items: center; cursor: pointer; }
  iframe { inline-size: 392px; block-size: 844px; justify-self: center; border: 0; border-radius: 16px; background: transparent; }
</style>
