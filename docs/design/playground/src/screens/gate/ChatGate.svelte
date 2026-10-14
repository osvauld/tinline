<script lang="ts">
  /* PROPOSED prototype (?gate=chat): the create flow as a short, caring conversation. Same state machine as the steps
     (gate.svelte.ts: history, back, busy, progress); only the view differs. Tinline's lines sit on the left, your answers on
     the right. The dock under the conversation has one fixed height on every step and its controls hang from its bottom
     edge, so the main action never moves; on Android the soft keyboard pushes the whole dock up, like a chat composer. */
  import { tick, untrack } from 'svelte';
  import { nav } from '../../lib/nav/router.svelte';
  import { open } from '../../lib/model/actions/people.svelte';
  import { gate, DEMO_PHRASE, CONFIRM_AT, type Step } from '../../lib/model/actions/gate.svelte';
  import { motion } from '../../lib/fx/reduced.svelte';
  import { timeScale } from '../../lib/fx/motion';
  import Mark from '../../lib/brand/Mark.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { bubbleRadii, type Pos } from '../../lib/geom/shape';
  import { shape } from '../../lib/geom/action';
  import { geo, R, uniform } from '../../lib/geom/geo.svelte';

  type Kind = 'text' | 'phrase' | 'terms' | 'perms' | 'warn';
  type Line = { t: string; kind?: Kind };
  const first = $derived(gate.name.trim().split(/\s+/)[0] ?? '');
  const protect = $derived(gate.isAndroid
    ? 'It encrypts your account on this phone. Your phone remembers it, so calls still ring after a restart.'
    : 'It encrypts your account on this computer. You’d type it each time Tinline starts.');

  function lines(s: Step): Line[] {
    switch (s) {
      case 'welcome': return [
        { t: 'Hi, welcome to Tinline.' },
        { t: 'Let’s set you up. It only takes a few minutes.' },
        { t: 'Tinline isn’t for emergency calls. For those, please use your phone.', kind: 'warn' },
      ];
      case 'name': return [{ t: 'What should people call you?' }];
      case 'pass': return [
        { t: first ? `Lovely to meet you, ${first}.` : 'Lovely to meet you.' },
        { t: 'Would you like a passphrase? ' + protect },
        { t: 'If you forget it, your recovery words can still bring your account back.' },
      ];
      case 'phrase': return [
        { t: 'Your account is ready. Here are your recovery words.' },
        { t: 'If you ever switch devices, they bring your account along. Save a copy somewhere safe. Paper works great.' },
        { t: '', kind: 'phrase' },
      ];
      case 'confirm': return [{ t: `Type words ${CONFIRM_AT.join(', ').replace(/, (\d+)$/, ' and $1')} from your saved copy, so you know it’s right.` }];
      case 'device': return [{ t: `What should I call this ${gate.noun}? Only your own devices see this name.` }];
      case 'terms': return [{ t: 'A few promises before you start.' }, { t: '', kind: 'terms' }];
      case 'perms': return [{ t: 'So calls can reach you, Tinline needs these. Each one is optional, and you can change them later.' }, { t: '', kind: 'perms' }];
      case 'done': return [{ t: first ? `Your line is open, ${first}.` : 'Your line is open.' }, { t: 'Scan their code or share yours, and you can call each other.' },
        { t: 'Two things worth knowing. Link another device when you can, so losing one never locks you out.' },
        { t: gate.isAndroid ? 'And groups are hosted on a computer, so to start one, add Tinline on a computer that can stay on.' : 'And this computer can host your groups. Keep Tinline open and they stay reachable.' }];
      default: return [];
    }
  }

  /* What you "said" at each step you've passed (the label you chose, or your typed answer). */
  let said = $state<Partial<Record<Step, string>>>({});
  function answerOf(s: Step): string {
    if (said[s]) return said[s]!;
    return ({ welcome: 'Create my account', name: gate.name.trim(), pass: gate.skippedPass ? 'Skip for now' : '••••••••', phrase: 'I’ve saved them',
      confirm: 'Done', device: gate.deviceLabel, terms: 'I agree', perms: 'Continue' } as Partial<Record<Step, string>>)[s] ?? '';
  }
  const turns = $derived([...gate.history, gate.step]);

  /* Tinline "types" each new line (dots, then the bubble). Going back, or reduced motion, shows everything at once. */
  let shown = $state(99);
  let timer: ReturnType<typeof setInterval> | undefined;
  const TYPE_MS = 420;
  $effect(() => {
    const s = gate.step, d = gate.dir;
    untrack(() => {
      clearInterval(timer);
      const n = lines(s).length;
      if (d === -1 || motion.reduced) { shown = n; return; }
      shown = 0;
      timer = setInterval(() => { shown += 1; if (shown >= n) clearInterval(timer); }, TYPE_MS * timeScale());
    });
    return () => clearInterval(timer);
  });
  const ready = $derived(shown >= lines(gate.step).length);

  let scroller = $state<HTMLElement>();
  $effect(() => {
    void turns.length; void shown; void gate.busy; void gate.phraseShown;
    tick().then(() => scroller?.scrollTo({ top: scroller.scrollHeight, behavior: motion.reduced ? 'auto' : 'smooth' }));
  });

  /* The keyboard opening (or the window shrinking) keeps the newest line in view, like a chat. */
  $effect(() => {
    const el = scroller; if (!el) return;
    const ro = new ResizeObserver(() => el.scrollTo({ top: el.scrollHeight }));
    ro.observe(el);
    return () => ro.disconnect();
  });

  function say(s: Step, text: string, then: () => void) { said[s] = text; then(); }

  // per-step dock state
  let shownPass = $state(false);
  const strength = $derived.by(() => { const p = gate.pass; if (!p) return 0; const w = p.trim().split(/\s+/).length; return p.length >= 16 || w >= 4 ? 3 : p.length >= 8 ? 2 : 1; });
  const says = ['Any length works. A few random words are easy to remember.', 'Short. A few more words make it stronger.', 'Fair', 'Strong'];
  let answers = $state(['', '', '']);
  let tried = $state(false);
  const confirmBad = $derived(tried && !gate.confirmOk(answers));
  const sugg = $derived(gate.isAndroid ? ['Personal phone', 'Work phone', 'Tablet'] : ['Home computer', 'Work computer', 'Laptop']);
  $effect(() => { if (gate.step === 'device' && !gate.deviceLabel) untrack(() => (gate.deviceLabel = gate.defaultLabel)); });
  type Perm = { k: string; icon: string; title: string; why: string };
  const perms: Perm[] = $derived(gate.isAndroid ? [
    { k: 'mic', icon: 'mic', title: 'Microphone', why: 'So they can hear you, only during a call.' },
    { k: 'notif', icon: 'bell', title: 'Notifications', why: 'So you see calls and messages.' },
    { k: 'lock', icon: 'phone', title: 'Ring over lock screen', why: 'Calls show full screen, like the phone app.' },
    { k: 'battery', icon: 'battery', title: 'Run in background', why: 'Keeps your line open, using a little battery.' },
    { k: 'camera', icon: 'camera', title: 'Camera', why: 'Only for video calls, when you turn it on.' },
  ] : [
    { k: 'notif', icon: 'bell', title: 'Notifications', why: 'So you see calls and messages.' },
    { k: 'mic', icon: 'mic', title: 'Microphone', why: 'So they can hear you, only during a call.' },
    { k: 'camera', icon: 'camera', title: 'Camera', why: 'Only for video calls, when you turn it on.' },
  ]);
  const touched = $derived(perms.some((p) => gate.perms[p.k] !== 'idle'));

  const them = (i: number, n: number): Pos => (n === 1 ? 'solo' : i === 0 ? 'first' : i === n - 1 ? 'last' : 'mid');
  const rThem = (p: Pos) => bubbleRadii(p, 'left', R().bub, R().tight);
  const rMe = bubbleRadii('solo', 'right', R().bub, R().tight);
  const pill = uniform(R().field);
  const autofocus = (n: HTMLElement) => { n.focus({ preventScroll: true }); };
</script>

{#snippet bubble(l: Line, pos: Pos)}
  <div class="b them {l.kind ?? ''}" class:wide={l.kind === 'phrase' || l.kind === 'terms' || l.kind === 'perms'}
    use:shape={{ radii: rThem(pos), smoothing: geo.smooth, stroke: { width: 1, color: 'var(--bub-in-line)' } }}>
    {#if l.kind === 'phrase'}
      <div class="words">
        {#each DEMO_PHRASE as w, i (i)}<span class="w"><i>{i + 1}</i><code>{w}</code></span>{/each}
      </div>
    {:else if l.kind === 'terms'}
      <ul class="pts">
        <li><Icon name="shield" size={18} /><span><b>Free of ads and tracking.</b> Tinline doesn’t collect your contacts, calls or location.</span></li>
        <li><Icon name="lock" size={18} /><span><b>Calls are end-to-end encrypted.</b> If a direct line isn’t possible, an encrypted relay passes the call along. It can’t hear it.</span></li>
        <li><Icon name="key" size={18} /><span><b>Your account is yours.</b> It lives on your devices, not on a server. Your recovery words are the way back in, so keep them safe.</span></li>
      </ul>
      <p class="fine">By agreeing you accept the <button type="button" class="lk">Terms of Use</button> and have read the <button type="button" class="lk">Privacy Policy</button>.</p>
    {:else if l.kind === 'perms'}
      <ul class="perms">
        {#each perms as p (p.k)}
          {@const st = gate.perms[p.k]}
          <li>
            <span class="ic"><Icon name={p.icon} size={18} /></span>
            <span class="tx"><b>{p.title}</b><small class:den={st === 'denied'}>{st === 'denied' ? 'Off for now. You can turn it on in system settings.' : p.why}</small></span>
            <span class="act">{#if st === 'granted'}<span class="ok" role="img" aria-label="{p.title} allowed"><Icon name="check" size={20} /></span>{:else}<button type="button" class="allow press" aria-label="Allow {p.title.toLowerCase()}" onclick={() => gate.allow(p.k)}>{st === 'denied' ? 'Retry' : 'Allow'}</button>{/if}</span>
          </li>
        {/each}
      </ul>
    {:else}{l.t}{/if}
  </div>
{/snippet}

<div class="chat" data-chat-gate>
  <div class="scroll" bind:this={scroller}>
    <div class="log" aria-live="polite">
      <!-- The conversation opens under the mark, like a chat's intro; it scrolls away as the talk goes on. -->
      <div class="intro"><Mark size={88} live label="Tinline" /><b>Tinline</b></div>
      {#each turns as s, ti (s + ti)}
        {@const ls = lines(s)}
        {@const n = s === gate.step ? Math.min(shown, ls.length) : ls.length}
        {#if n > 0 || (s === gate.step && !ready)}
          <div class="run">
            <span class="av" aria-hidden="true"><Mark size={28} /></span>
            <div class="col">
              {#each ls.slice(0, n) as l, i (i)}<div class="in">{@render bubble(l, them(i, s === gate.step && !ready ? n + 1 : n))}</div>{/each}
              {#if s === gate.step && (!ready || gate.busy)}
                <div class="in"><div class="b them typing" use:shape={{ radii: rThem(n ? 'last' : 'solo'), smoothing: geo.smooth, stroke: { width: 1, color: 'var(--bub-in-line)' } }} aria-label={gate.busy ? 'Encrypting your account' : 'Tinline is typing'}>
                  {#if gate.busy}<span class="busy">Encrypting your account…</span>{:else}<i></i><i></i><i></i>{/if}
                </div></div>
              {/if}
            </div>
          </div>
        {/if}
        {#if s !== gate.step && answerOf(s)}
          <div class="mine"><div class="b me" use:shape={{ radii: rMe, smoothing: geo.smooth }}>{answerOf(s)}</div></div>
        {/if}
      {/each}
    </div>
  </div>

  <div class="dock" class:wait={!ready} inert={!ready || undefined}>
    {#key gate.step}
    <div class="dk">
      {#if gate.step === 'welcome'}
        <div class="replies">
          <button type="button" class="rp press" onclick={() => say('welcome', 'Restore my account', () => gate.startRestore())}>Restore my account</button>
          <button type="button" class="rp press" onclick={() => say('welcome', 'Link my other device', () => gate.startLink())}>Link my other device</button>
          <button type="button" class="rp main press" onclick={() => say('welcome', 'Create my account', () => gate.startCreate())}>Create my account</button>
        </div>
      {:else if gate.step === 'name'}
        <p class="hint">Shown to people you add. You can change it any time.</p>
        <form class="cmp" onsubmit={(e) => { e.preventDefault(); if (gate.name.trim()) say('name', gate.name.trim(), () => gate.go('pass')); }}>
          <span class="fw"><span class="skin" aria-hidden="true" use:shape={{ radii: pill, smoothing: geo.smooth, stroke: { width: 1, color: 'var(--line)' } }}></span><input class="fld" aria-label="Your nickname" placeholder="Your nickname" autocomplete="name" bind:value={gate.name} use:autofocus /></span>
          <button class="send press" aria-label="Send" disabled={!gate.name.trim()}><Icon name="send" size={20} /></button>
        </form>
      {:else if gate.step === 'pass'}
        <div class="meter" aria-live="polite">
          <span class="bars" aria-hidden="true">{#each [1, 2, 3] as i (i)}<i class:on={strength >= i} data-s={strength}></i>{/each}</span>
          <span class="says">{#each says as t, i (i)}<small class:cur={i === (gate.pass ? strength : 0)}>{t}</small>{/each}</span>
        </div>
        <form class="cmp" onsubmit={(e) => { e.preventDefault(); if (gate.pass && !gate.busy) { said.pass = '•'.repeat(Math.min(12, Math.max(8, gate.pass.length))); gate.submitPass(false); } }}>
          <span class="fw">
            <span class="skin" aria-hidden="true" use:shape={{ radii: pill, smoothing: geo.smooth, stroke: { width: 1, color: 'var(--line)' } }}></span><input class="fld" type={shownPass ? 'text' : 'password'} aria-label="Passphrase" placeholder="Passphrase" autocomplete="new-password" disabled={gate.busy} bind:value={gate.pass} use:autofocus />
            <button type="button" class="eye press" aria-label={shownPass ? 'Hide passphrase' : 'Show passphrase'} aria-pressed={shownPass} onclick={() => (shownPass = !shownPass)}><Icon name={shownPass ? 'eyeoff' : 'eye'} size={20} /></button>
          </span>
          <button class="send press" aria-label="Add passphrase" disabled={!gate.pass || gate.busy}><Icon name="send" size={20} /></button>
        </form>
        <div class="replies row"><button type="button" class="rp press" disabled={gate.busy} onclick={() => say('pass', 'Skip for now', () => gate.submitPass(true))}>Skip for now</button></div>
      {:else if gate.step === 'phrase'}
        <div class="replies">
          {#if gate.phraseShown}
            <button type="button" class="rp press" onclick={() => say('phrase', 'Check my copy', () => gate.go('confirm'))}>Check my copy</button>
            <button type="button" class="rp main press" onclick={() => say('phrase', 'I’ve saved them', () => gate.go('device'))}>I’ve saved them</button>
          {:else}
            <button type="button" class="rp press" onclick={() => say('phrase', 'I wrote them down', () => gate.go('device'))}>I wrote them down</button>
            <button type="button" class="rp main press" onclick={() => { navigator.clipboard?.writeText(DEMO_PHRASE.join(' ')).catch(() => {}); gate.phraseShown = true; }}>Copy my words</button>
          {/if}
        </div>
      {:else if gate.step === 'confirm'}
        <p class="hint" class:bad={confirmBad} role={confirmBad ? 'alert' : undefined}>{confirmBad ? 'One of those doesn’t match. Check your saved copy.' : 'Type each word as it is in your copy.'}</p>
        <form class="cmp" onsubmit={(e) => { e.preventDefault(); tried = true; if (gate.confirmOk(answers)) { tried = false; answers = ['', '', '']; gate.back(); } }}>
          {#each CONFIRM_AT as p, i (p)}<span class="fw"><span class="skin" aria-hidden="true" use:shape={{ radii: pill, smoothing: geo.smooth, stroke: { width: 1, color: 'var(--line)' } }}></span><input class="fld mono" aria-label="Word {p}" placeholder="Word {p}" bind:value={answers[i]} /></span>{/each}
          <button class="send press" aria-label="Done" disabled={answers.some((a) => !a.trim())}><Icon name="check" size={20} /></button>
        </form>
      {:else if gate.step === 'device'}
        <div class="replies row">{#each sugg as s (s)}<button type="button" class="rp chip press" aria-pressed={gate.deviceLabel === s} onclick={() => (gate.deviceLabel = s)}>{s}</button>{/each}</div>
        <form class="cmp" onsubmit={(e) => { e.preventDefault(); const v = gate.deviceLabel.trim(); if (v) { gate.deviceLabel = v; say('device', v, () => gate.nextAfterDevice()); } }}>
          <span class="fw"><span class="skin" aria-hidden="true" use:shape={{ radii: pill, smoothing: geo.smooth, stroke: { width: 1, color: 'var(--line)' } }}></span><input class="fld" aria-label="Device name" placeholder="Device name" bind:value={gate.deviceLabel} /></span>
          <button class="send press" aria-label="Send" disabled={!gate.deviceLabel.trim()}><Icon name="send" size={20} /></button>
        </form>
      {:else if gate.step === 'terms'}
        <div class="replies"><button type="button" class="rp main press" onclick={() => { gate.agreed = true; say('terms', 'I agree', () => gate.go('perms')); }}>I agree</button></div>
      {:else if gate.step === 'perms'}
        <div class="replies"><button type="button" class="rp press" class:main={touched} onclick={() => say('perms', touched ? 'Continue' : 'Not now', () => gate.go('done'))}>{touched ? 'Continue' : 'Not now'}</button></div>
      {:else if gate.step === 'done'}
        <div class="replies"><button type="button" class="rp press" onclick={() => gate.finish()}>Not now</button><button type="button" class="rp main press" onclick={() => { gate.finish(); nav.setTab('contacts'); open('addContact'); }}>Add my first contact</button></div>
      {/if}
    </div>
    {/key}
  </div>
</div>

<style>
  .chat { flex: 1; min-block-size: 0; display: flex; flex-direction: column; }
  .scroll { flex: 1; min-block-size: 0; overflow: auto; scrollbar-width: thin; mask-image: linear-gradient(to bottom, transparent 0, #000 20px); }
  /* Bottom-anchored log: short conversations sit just above the dock, like a chat. */
  .log { min-block-size: 100%; display: flex; flex-direction: column; justify-content: flex-end; gap: 10px; padding: 8px 20px 12px; }
  .intro { display: grid; justify-items: center; gap: 0; padding-block: 8px 12px; margin-block-end: auto; color: var(--ink); }
  .intro b { font-size: 20px; line-height: 26px; font-weight: 700; margin-block-start: -10px; }
  .run { display: flex; align-items: flex-end; gap: 8px; }
  .av { flex: none; inline-size: 28px; block-size: 28px; display: grid; place-items: center; color: var(--ink); }
  .col { display: flex; flex-direction: column; align-items: flex-start; gap: 2px; min-inline-size: 0; flex: 1; }
  .mine { display: flex; justify-content: flex-end; }
  .in { display: contents; }
  .b { position: relative; max-inline-size: min(82%, 440px); padding: 9px 14px; font-size: var(--fs-body); line-height: var(--lh-body); border: 0;
    animation: bubIn var(--dur-bubbleIn) var(--ease-out) both; }
  .them { background: var(--bub-in); color: var(--bub-in-ink); }
  .me { background: var(--bub-out); color: var(--bub-out-ink); }
  .them.warn { background: var(--warn-bg); color: var(--warn-ink); }
  .b.wide { inline-size: min(100%, 460px); max-inline-size: 100%; padding: 11px 13px; }
  @keyframes bubIn { from { opacity: 0; transform: translateY(6px); } }
  :global([data-motion='reduce']) .b { animation: none; }

  .typing { display: inline-flex; gap: 4px; align-items: center; min-block-size: 38px; }
  .typing i { inline-size: 6px; block-size: 6px; border-radius: 50%; background: var(--muted); opacity: .4; animation: dot 1s var(--ease-inOut) infinite; }
  .typing i:nth-child(2) { animation-delay: .15s; } .typing i:nth-child(3) { animation-delay: .3s; }
  @keyframes dot { 50% { opacity: 1; transform: translateY(-2px); } }
  .busy { color: var(--muted); }

  .words { position: relative; display: grid; grid-template-columns: repeat(3, 1fr); gap: 4px 6px; }
  :global([data-form='android']) .words { grid-template-columns: repeat(2, 1fr); }
  .w { display: flex; align-items: center; gap: 8px; min-block-size: 30px; padding-inline: 8px; border-radius: 8px; background: var(--surface-2); }
  .w i { inline-size: 18px; text-align: end; font-style: normal; font-size: var(--fs-meta); color: var(--muted); font-variant-numeric: tabular-nums; }
  .w code { font: 14px/18px ui-monospace, 'SF Mono', Menlo, Consolas, monospace; }

  .pts, .perms { list-style: none; margin: 0; padding: 0; display: grid; gap: 10px; }
  .pts li { display: flex; gap: 10px; align-items: flex-start; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; }
  .pts li :global(svg) { flex: none; color: var(--accent-ink); margin-block-start: 1px; }
  .pts b { color: var(--ink); }
  .fine { margin: 10px 0 0; font-size: var(--fs-sub); color: var(--muted); }
  .lk { border: 0; background: none; padding: 0; color: var(--accent-ink); text-decoration: underline; font: inherit; cursor: pointer; }
  .perms { gap: 6px; }
  .perms li { display: flex; gap: 10px; align-items: center; }
  .ic { display: grid; place-items: center; inline-size: 34px; block-size: 34px; border-radius: 50%; background: var(--accent-soft); color: var(--accent-ink); flex: none; }
  .tx { display: grid; flex: 1; min-inline-size: 0; font-size: var(--fs-sub); line-height: 18px; }
  .tx small { color: var(--muted); font-size: var(--fs-sub); }
  .tx .den { color: var(--muted); }
  .act { inline-size: 64px; flex: none; display: grid; place-items: center; }
  .allow { inline-size: 64px; min-block-size: 36px; border-radius: 18px; border: 1px solid var(--line); background: transparent; color: var(--accent-ink); font-weight: 650; cursor: pointer; }
  .ok { color: var(--accent-ink); display: grid; place-items: center; }

  /* The dock: one height on every step (the tallest, welcome's three replies); content hangs from the bottom edge. */
  .dock { flex: none; block-size: 172px; padding: 8px 20px 20px; display: grid; }
  .dk { display: flex; flex-direction: column; justify-content: flex-end; gap: 8px; min-block-size: 0; animation: dockIn var(--dur-enter) var(--ease-out) both; }
  .dock.wait .dk { opacity: 0; animation: none; }
  @keyframes dockIn { from { opacity: 0; } }
  :global([data-motion='reduce']) .dk { animation: none; }
  .replies { display: flex; flex-direction: column; align-items: flex-end; gap: 8px; }
  .replies.row { flex-direction: row; flex-wrap: wrap; justify-content: flex-end; }
  /* Replies look like the bubble you are about to send: outlined until chosen, the main one in your own mint. */
  .rp { min-block-size: 44px; padding-inline: 18px; border-radius: 22px; border: 1px solid var(--accent-line, var(--line)); background: var(--surface); color: var(--accent-ink);
    font-weight: 650; font-size: var(--fs-body); cursor: pointer; }
  .rp:hover:not(:disabled) { background: var(--surface-2); }
  .rp.main { background: var(--bub-out); color: var(--bub-out-ink); border-color: transparent; }
  .rp.main:hover:not(:disabled) { background: var(--bub-out); filter: brightness(.97); }
  .rp:disabled { opacity: .45; cursor: default; }
  .rp.chip { min-block-size: 36px; padding-inline: 14px; font-size: var(--fs-sub); }
  .rp.chip[aria-pressed='true'] { background: var(--accent-soft); border-color: transparent; }
  .cmp { display: flex; gap: 8px; align-items: center; }
  .fw { position: relative; flex: 1; min-inline-size: 0; display: flex; isolation: isolate; }
  .fld { position: relative; flex: 1; min-inline-size: 0; inline-size: 100%; min-block-size: 46px; padding-inline: 18px; border: 0; border-radius: 22px; background: transparent; color: var(--ink); font-size: var(--fs-body); }
  .skin { position: absolute; inset: 0; pointer-events: none; background: var(--surface); border-radius: 22px; box-shadow: inset 0 0 0 1px var(--line); }
  .skin:global([data-shaped]) { box-shadow: none; }
  .fld.mono { font-family: ui-monospace, 'SF Mono', Menlo, Consolas, monospace; padding-inline: 14px; }
  .fld:focus-visible { outline: 2px solid var(--accent-ink); outline-offset: 1px; }
  .fw .fld { padding-inline-end: 48px; }
  .eye { position: absolute; inset-inline-end: 2px; inset-block-start: 1px; inline-size: 44px; block-size: 44px; display: grid; place-items: center; border: 0; background: transparent; color: var(--muted); border-radius: 50%; cursor: pointer; }
  .send { flex: none; inline-size: 46px; block-size: 46px; border-radius: 50%; border: 0; display: grid; place-items: center; background: var(--accent); color: var(--on-accent); cursor: pointer; }
  .send:disabled { background: var(--surface-2); color: var(--muted); cursor: default; }
  .hint { margin: 0; padding-inline: 18px; font-size: var(--fs-sub); line-height: 18px; color: var(--muted); }
  .hint.bad { color: var(--danger); font-weight: 600; }
  .meter { display: flex; align-items: center; gap: 10px; padding-inline: 18px; }
  .bars { display: inline-flex; gap: 4px; }
  .bars i { inline-size: 24px; block-size: 5px; border-radius: 3px; background: var(--line); transition: background-color var(--dur-switch) var(--ease-out); }
  .bars i.on[data-s='1'] { background: var(--danger); } .bars i.on[data-s='2'] { background: var(--gold); } .bars i.on[data-s='3'] { background: var(--accent); }
  .says { display: grid; flex: 1; min-inline-size: 0; }
  .says > small { grid-area: 1 / 1; visibility: hidden; color: var(--muted); font-size: var(--fs-sub); line-height: 18px; }
  .says > small.cur { visibility: visible; }
</style>
