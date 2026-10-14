<script lang="ts">
  /* Add a contact (decided 2026-10-13): two ways, and only these.
     QR (for in person): my QR to be scanned, and (phone) the camera to scan theirs. The surest way: you can see who it is.
     Code: the bare code, nothing else around it. Copy mine and send it in any app, or paste theirs.
     Everything happens in one fixed-height body, so the sheet never resizes: the found card replaces it in place. */
  import { onDestroy, untrack } from 'svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import QrCode from './QrCode.svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import LineJoin from './LineJoin.svelte';
  import Btn from '../groups/Btn.svelte';
  import { person } from '../../lib/model/store.svelte';
  import { nav } from '../../lib/nav/router.svelte';
  import { prefs } from '../../lib/model/prefs.svelte';
  import { addPerson, answerRequest, closeDialog, codeHooks, codeUntil, pp, requestSent, reach, resolveCode, scanResult, type Found, type Reach } from '../../lib/model/actions/people.svelte';
  import { ensureChat } from '../../lib/model/actions/profile';

  const TABS = [['code', 'Code'], ['person', 'QR']] as const;   // the default first
  type Tab = (typeof TABS)[number][0];
  let tab = $state<Tab>(untrack(() => pp.addTab));
  const pill = new URLSearchParams(location.search).get('acswitch') === 'pill';
  let scanning = $state(false);   // phone, QR tab: the camera replaces my QR in the same box
  let res = $state<(Found & { ok: true; how: 'scan' | 'code' }) | null>(null);
  const isPhone = $derived(prefs.form === 'android');
  const cam = $derived(pp.perms.camera);
  let timer = 0;
  $effect(() => {
    if (!(tab === 'person' && scanning && cam === 'granted' && !res)) return;
    timer = window.setTimeout(() => { res = { ...(scanResult() as Found & { ok: true }), how: 'scan' }; }, 2200);
    return () => clearTimeout(timer);
  });
  function pick(t: Tab) { tab = t; res = null; added = false; error = ''; scanning = false; fail = null; }
  function tabKey(e: KeyboardEvent) {
    const d = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0;
    if (!d) return;
    e.preventDefault(); pick(tab === 'person' ? 'code' : 'person');
    queueMicrotask(() => document.getElementById('ac-tab-' + tab)?.focus());
  }
  let added = $state(false);
  let error = $state('');
  let copied = $state(false);
  let copyTimer = 0;
  onDestroy(() => { clearTimeout(copyTimer); clearTimeout(timer); });

  function copy() {
    navigator.clipboard?.writeText(pp.myCode).catch(() => {});
    copied = true; clearTimeout(copyTimer); copyTimer = window.setTimeout(() => (copied = false), 1600);
  }
  /* Paste reads the clipboard and looks it up at once (any text that contains a code works). */
  async function paste(given?: string) {
    let text = given ?? '';
    if (given == null) { try { text = await navigator.clipboard.readText(); } catch { /* no permission */ } }
    if (!text.trim()) { error = 'Your clipboard is empty right now. When their code arrives, copy it and paste it here.'; return; }
    const f = resolveCode(text);
    if (!f.ok) { error = f.error; return; }
    error = ''; res = { ...f, how: 'code' };
  }
  /* Adding dials them (core add_contact): "Adding…" while it reaches their device. If it can't, the reason sits in
     the same line as the question (muted, nothing moves) and the button offers the next step. */
  let adding = $state(false);
  let fail = $state<Exclude<Reach, 'ok'> | null>(null);
  /* The real dial can take up to 45 s (30 s connect + 15 s answer): after a moment the line says what it needs. */
  let slow = $state(false);
  async function add() {
    if (!res || adding) return;
    const f = { id: res.id, name: res.name, hue: res.hue }, how = res.how;
    adding = true; fail = null; slow = false;
    const st = window.setTimeout(() => (slow = true), 2500);
    const r = await reach({ ...f, code: res.code });
    clearTimeout(st); adding = false; slow = false;
    if (r === 'pending') { requestSent(f); return; }
    if (r !== 'ok') { fail = r; return; }
    addPerson(f, how); added = true;
  }
  /* Sent and waiting: the line keeps its connecting sway while this sheet is open; when they accept, it pulls tight
     right here (or, if I closed the sheet, the moment opens again by itself: people.svelte.ts `accepted`). */
  $effect(() => { pp.facing = res?.id ?? null; });
  onDestroy(() => { pp.facing = null; });
  const requested = $derived(!!res && !added && pp.sent.some((x) => x.id === res!.id));
  $effect(() => { if (res && !added && !res.already && person(res.id)?.contact) added = true; });
  function message(id = res?.id) {
    if (!id) return;
    const c = ensureChat(id); closeDialog();
    if (c) (prefs.form === 'desktop' ? nav.open({ name: 'conversation', chat: c.id }) : nav.push({ name: 'conversation', chat: c.id }));
  }
  function profile() { if (!res) return; const id = res.id; closeDialog(); prefs.form === 'desktop' ? nav.open({ name: 'contact', person: id }) : nav.push({ name: 'contact', person: id }); }
  const inc = $derived(pp.incoming);
  const first = $derived(res ? (person(res.id)?.name ?? res.name).split(' ')[0] : '');
</script>

<div class="ac pf-dlg" data-add-contact-sheet>
  <!-- Someone used my code: the same sheet becomes "New contact" (their side of the handshake). -->
  <h2>{inc ? (inc.phase === 'asking' ? 'Contact request' : 'New contact') : 'Add contact'}</h2>
  <!-- Switch style under trial: chips (default, the same chips as the Chats filter) or ?acswitch=pill (the old segmented pill). -->
  <div class="tabs" class:gone={!!inc} class:chips={!pill} role="tablist" aria-label="How to add" tabindex="-1" onkeydown={tabKey}>
    {#each TABS as [k, l] (k)}
      <button type="button" role="tab" id="ac-tab-{k}" class="press {pill ? 'tab' : 't-chip'}" class:on={!pill && tab === k} aria-selected={tab === k} tabindex={tab === k ? 0 : -1} aria-controls="ac-body" onclick={() => pick(k)}>{l}</button>
    {/each}
  </div>
  <div class="body" id="ac-body" role="tabpanel" aria-labelledby="ac-tab-{tab}">
    {#if inc}
      {@const ifirst = inc.name.split(' ')[0]}
      <div class="found" data-incoming={inc.phase}>
        <LineJoin them={inc} state={inc.phase === 'joined' ? 'taut' : 'ringing'} />
        <b class="nm" dir="auto">{inc.name}</b>
        <span class="state"><Icon name="userplus" size={14} />{inc.kind === 'accepted' ? 'Accepted your request' : 'Has your code'}</span>
        <p class="why" role="status">
          {#if inc.phase === 'asking'}{ifirst} wants to add you as a contact. Accepting lets you message and call each other.
          {:else if inc.kind === 'accepted'}{ifirst} accepted. Your line to {ifirst} is open. Say hello.
          {:else}Your line to {ifirst} is open. Say hello.{/if}
        </p>
      </div>
    {:else if res}
      <div class="found" data-found={res.id}>
        <LineJoin them={res} state={added || res.already ? 'taut' : adding || requested ? 'ringing' : 'slack'} />
        <b class="nm" dir="auto">{res.name}</b>
        {#if res.how === 'scan'}<span class="state"><Icon name="check" size={14} />Scanned in person</span>{/if}
        <p class="why">
          {#if res.blocked}You blocked {first}. Unblock them in Privacy before adding them again.
          {:else if res.already}{first} is already in your contacts.
          {:else if added}Your line to {first} is open. Say hello.
          {:else if requested}Request sent. Your line opens when {first} accepts.
          {:else if adding}{slow ? `Still trying. ${first}’s Tinline needs to be open and online for this.` : `Reaching ${first}’s Tinline…`}
          {:else if fail === 'timeout'}Couldn’t reach {first}’s Tinline. It needs to be open and online, so try again when {first}’s around.
          {:else if fail === 'refused'}{first}’s code isn’t valid any more. They may have made a new one, so ask {first} for their newest code.
          {:else if fail === 'blocked'}You can’t add {first} right now. They’ve chosen not to be in touch here.
          {:else if fail === 'nonet'}You’re offline. Once you’re connected, try again.
          {:else if res.how === 'scan'}You scanned it from their screen, so you know whose code this is.
          {:else}Is this the {first} you expected? If you’re not sure, check with them first.{/if}
        </p>
      </div>
    {:else if tab === 'person'}
      <div class="mine">
        <div class="qrbox">
          {#if scanning}
            {#if cam === 'granted'}
              <div class="finder" role="img" aria-label="Camera view, simulated. Looking for a code." data-viewfinder>
                <i class="c tl"></i><i class="c tr"></i><i class="c bl"></i><i class="c br"></i><i class="sweep"></i></div>
            {:else}
              <div class="denied" data-camera-denied><Icon name="camera" size={28} /><b>The camera is off</b>
                <Btn variant="outline" onclick={() => (pp.perms.camera = 'granted')}>Allow the camera</Btn></div>
            {/if}
          {:else}<QrCode seed={77} size={260} label="Your QR code, a sample that can’t be scanned" />{/if}
        </div>
        <p class="why c">{scanning ? 'Point the camera at the code on their screen.' : 'Let them scan this with Tinline.'}</p>
        {#if isPhone}<Btn variant="outline" data-scan-toggle onclick={() => (scanning = !scanning)}>{scanning ? 'Show my code' : 'Scan their code'}</Btn>{/if}
      </div>
    {:else}
      <section class="part">
        <!-- "New code" sits on the label's line (a text button, same height), so it never moves anything. -->
        <div class="hd"><b>Your code</b><button type="button" class="newcode press" data-new-code onclick={() => codeHooks.renew()}>New code</button></div>
        <p class="why">Send it to anyone you’d like to talk to, in any app. It works until {codeUntil()}.</p>
        <code class="code" title={pp.myCode} data-my-code>{pp.myCode}</code>
        <Btn variant="primary" data-copy-code onclick={copy}><span class="cw">{copied ? 'Copied' : 'Copy my code'}</span></Btn>
      </section>
      <div class="or" aria-hidden="true"><span>or</span></div>
      <section class="part">
        <b>Their code</b>
        <Btn variant="outline" data-paste-now onclick={() => paste()}>Paste their code</Btn>
        <p class="err" role="status" data-code-error>{error}</p>
      </section>
    {/if}
  </div>
  <div class="acts">
    {#if inc}
      {#if inc.phase === 'asking'}
        {@const rid = pp.requests.find((r) => r.who === inc.id)?.id}
        <Btn variant="quiet" onclick={() => rid && answerRequest(rid, false)}>Not now</Btn>
        <Btn variant="primary" data-accept onclick={() => rid && answerRequest(rid, true)}>Accept</Btn>
      {:else}
        <Btn variant="quiet" onclick={closeDialog}>Done</Btn>
        <Btn variant="primary" onclick={() => message(inc.id)}>Message {inc.name.split(' ')[0]}</Btn>
      {/if}
    {:else if res}
      {#if requested}
        <Btn variant="primary" onclick={closeDialog}>Done</Btn>
      {:else if added}
        <Btn variant="quiet" onclick={closeDialog}>Done</Btn><Btn variant="primary" onclick={() => message()}>Message {first}</Btn>
      {:else if res.already}
        <Btn variant="quiet" onclick={closeDialog}>Close</Btn><Btn variant="primary" onclick={profile}>Open profile</Btn>
      {:else if res.blocked}
        <Btn variant="quiet" onclick={closeDialog}>Close</Btn><Btn variant="primary" onclick={() => { closeDialog(); nav.open({ name: 'privacy', section: 'blocked' }); }}>Open blocked people</Btn>
      {:else}
        <Btn variant="quiet" disabled={adding} onclick={() => { res = null; error = ''; fail = null; }}>Back</Btn>
        {#if fail === 'refused'}<Btn variant="primary" data-add-confirm onclick={() => { res = null; fail = null; }}>Paste a new code</Btn>
        {:else if fail === 'blocked'}<Btn variant="primary" data-add-confirm onclick={closeDialog}>Close</Btn>
        {:else}<Btn variant="primary" data-add-confirm disabled={adding} onclick={add}><span class="cw">{adding ? 'Adding…' : fail ? 'Try again' : 'Add ' + first}</span></Btn>{/if}
      {/if}
    {:else}
      <Btn variant="quiet" onclick={closeDialog}>Close</Btn>
    {/if}
  </div>
</div>

<style>
  .ac { display: grid; gap: 12px; }
  .ac h2 { margin: 0; }
  /* 360: fits Your code + the two-line paste hint, and a 260 px QR (2026-10-14: the real code is long and dense, so 176 px
     was too small to scan on a phone). Was 328 / 176. */
  .body { block-size: 360px; display: grid; align-content: center; grid-template-columns: minmax(0, 1fr); gap: 12px; overflow: hidden; }
  .tabs { display: flex; gap: 4px; padding: 3px; border-radius: 20px; background: var(--surface-2); }
  .tabs.chips { padding: 0; background: none; border-radius: 0; gap: 8px; }
  .tab { flex: 1; min-block-size: 36px; border: 0; border-radius: 17px; background: transparent; color: var(--muted); font: inherit; font-weight: 600; font-size: var(--fs-sub); cursor: pointer; }
  .tab[aria-selected='true'] { background: var(--surface); color: var(--ink); box-shadow: var(--shadow-lift), inset 0 0 0 1px var(--hairline); }
  .mine { display: grid; justify-items: center; gap: 10px; }
  .mine :global(.btn) { inline-size: 100%; }
  .why.c { text-align: center; }
  .qrbox { inline-size: 272px; block-size: 272px; display: grid; place-items: center; }
  .finder { position: relative; inline-size: 272px; block-size: 272px; border-radius: 20px; background: #0e1513; overflow: hidden; }
  .c.tl, .c.tr, .c.bl, .c.br { position: absolute; inline-size: 30px; block-size: 30px; border: 3px solid #fff; opacity: .9; }
  .tl { inset: 14px auto auto 14px; border-width: 3px 0 0 3px !important; border-top-left-radius: 10px; }
  .tr { inset: 14px 14px auto auto; border-width: 3px 3px 0 0 !important; border-top-right-radius: 10px; }
  .bl { inset: auto auto 14px 14px; border-width: 0 0 3px 3px !important; border-bottom-left-radius: 10px; }
  .br { inset: auto 14px 14px auto; border-width: 0 3px 3px 0 !important; border-bottom-right-radius: 10px; }
  .sweep { position: absolute; inset-inline: 20px; inset-block-start: 20px; block-size: 2px; border-radius: 1px; background: var(--accent); box-shadow: 0 0 12px var(--accent); animation: sweep 1.6s ease-in-out infinite alternate; }
  :global(:root[data-motion='reduce']) .sweep { animation: none; inset-block-start: 50%; }
  @keyframes sweep { to { inset-block-start: calc(100% - 22px); } }
  .denied { display: grid; justify-items: center; align-content: center; gap: 10px; inline-size: 100%; block-size: 100%; border-radius: 20px; background: var(--surface-2); color: var(--muted); padding: 12px; }
  .denied b { color: var(--ink); font-size: var(--fs-sub); }
  .state { display: inline-flex; align-items: center; gap: 6px; padding: 3px 10px; border-radius: 13px; background: var(--accent-soft); color: var(--accent-ink); font-size: var(--fs-sub); line-height: 20px; font-weight: 600; }
  .part { display: grid; gap: 8px; }
  .part b { font-size: var(--fs-sub); }
  .hd { display: flex; align-items: baseline; justify-content: space-between; }
  .newcode { border: 0; background: none; padding: 0; color: var(--accent-ink); font: inherit; font-size: var(--fs-sub); font-weight: 600; cursor: pointer; }
  .part :global(.btn) { inline-size: 100%; }
  .why { margin: 0; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; text-wrap: pretty; }
  /* The code itself, one quiet line: there to see that it's text you can send, never to read or type. */
  .code { display: block; padding: 8px 12px; border-radius: 12px; background: var(--surface-2); color: var(--muted);
    font: 12px/18px ui-monospace, 'SF Mono', Menlo, Consolas, monospace; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; user-select: all; }
  .or { display: grid; grid-template-columns: 1fr auto 1fr; align-items: center; gap: 10px; color: var(--muted); font-size: var(--fs-sub); }
  .or::before, .or::after { content: ''; block-size: 1px; background: var(--line); }
  .err { margin: 0; min-block-size: 38px; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; }
  .tabs.gone { visibility: hidden; }   /* keeps its height: the sheet doesn't jump when it becomes New contact */
  .found { display: grid; justify-items: center; gap: 8px; text-align: center; }
  .found .why { max-inline-size: 34ch; text-wrap: balance; min-block-size: 57px; }   /* three lines reserved: the reason can change without moving the face */
  .nm { font-size: 20px; line-height: 26px; font-weight: 650; }
  .acts { display: flex; gap: 8px; justify-content: flex-end; margin-block-start: 2px; }
  .cw { display: inline-block; min-inline-size: 96px; text-align: center; }
</style>
