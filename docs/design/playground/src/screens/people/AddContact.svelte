<script lang="ts">
  /* Add a contact (simulated, no camera): scan their QR, show mine, or paste an invite code. All three end in the same
     card, which says plainly how sure Tinline is about who this is (a scan is not a verification of the person). */
  import { onDestroy, untrack } from 'svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Btn from '../groups/Btn.svelte';
  import QrCode from './QrCode.svelte';
  import { person } from '../../lib/model/store.svelte';
  import { nav } from '../../lib/nav/router.svelte';
  import { prefs } from '../../lib/model/prefs.svelte';
  import { addPerson, closeDialog, MY_CODE, pp, resolveCode, scanResult, type Found } from '../../lib/model/actions/people.svelte';
  import { ensureChat } from '../../lib/model/actions/profile';

  const TABS = [['scan', 'Scan'], ['mine', 'My code'], ['paste', 'Paste code']] as const;
  type Tab = (typeof TABS)[number][0];
  let tab = $state<Tab>(untrack(() => pp.addTab));
  let res = $state<(Found & { ok: true; how: 'scan' | 'code' }) | null>(null);
  let added = $state(false);
  let text = $state('');
  let error = $state('');
  let copied = $state(false);
  let timer = 0, copyTimer = 0;
  const cam = $derived(pp.perms.camera);

  /* The viewfinder runs while the Scan tab is showing, has permission and holds no result. */
  const looking = $derived(tab === 'scan' && cam === 'granted' && !res);
  $effect(() => {
    if (!looking) return;
    timer = window.setTimeout(() => { const f = scanResult(); res = { ...(f as Found & { ok: true }), how: 'scan' }; }, 2200);
    return () => clearTimeout(timer);
  });
  onDestroy(() => { clearTimeout(timer); clearTimeout(copyTimer); });

  function pick(t: Tab) { tab = t; res = null; added = false; error = ''; }
  function lookup() {
    const f = resolveCode(text);
    if (!f.ok) { error = f.error; return; }
    error = ''; res = { ...f, how: 'code' };
  }
  function add() {
    if (!res) return;
    addPerson({ id: res.id, name: res.name, hue: res.hue }, res.how);
    added = true;
  }
  function message() {
    if (!res) return;
    const c = ensureChat(res.id); closeDialog();
    if (c) (prefs.form === 'desktop' ? nav.open({ name: 'conversation', chat: c.id }) : nav.push({ name: 'conversation', chat: c.id }));
  }
  function profile() { if (!res) return; const id = res.id; closeDialog(); prefs.form === 'desktop' ? nav.open({ name: 'contact', person: id }) : nav.push({ name: 'contact', person: id }); }
  function copy() { copied = true; clearTimeout(copyTimer); copyTimer = window.setTimeout(() => (copied = false), 1600); }
  const first = $derived(res ? (person(res.id)?.name ?? res.name).split(' ')[0] : '');
  /* Tabs by arrow keys, like any tablist. */
  function tabKey(e: KeyboardEvent) {
    const i = TABS.findIndex(([k]) => k === tab), d = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0;
    if (!d) return;
    e.preventDefault(); pick(TABS[(i + d + TABS.length) % TABS.length][0]);
    queueMicrotask(() => document.getElementById('ac-tab-' + tab)?.focus());
  }
</script>

<div class="ac pf-dlg" data-add-contact-sheet>
  <h2>Add contact</h2>
  <div class="tabs" role="tablist" aria-label="How to add" tabindex="-1" onkeydown={tabKey}>
    {#each TABS as [k, l] (k)}
      <button type="button" role="tab" id="ac-tab-{k}" class="tab press" aria-selected={tab === k} tabindex={tab === k ? 0 : -1} aria-controls="ac-body" onclick={() => pick(k)}>{l}</button>
    {/each}
  </div>

  <div class="body" id="ac-body" role="tabpanel" aria-labelledby="ac-tab-{tab}">
    {#if res}
      <div class="found" data-found={res.id}>
        <Avatar name={res.name} hue={res.hue} person={res.id} size={72} />
        <b class="nm" dir="auto">{res.name}</b>
        <span class="state" class:weak={res.how === 'code'} data-verify={res.how}>
          <Icon name={res.how === 'scan' ? 'check' : 'info'} size={14} />{res.how === 'scan' ? 'Scanned from their screen' : 'Not confirmed in person'}</span>
        <p class="why">
          {#if res.blocked}You blocked {first}. Unblock them in Privacy before adding them again.
          {:else if res.already}{first} is already in your contacts.
          {:else if added}{first} is in your contacts. {pp.newContactsCall ? 'They can call you.' : 'They can message you; calls stay off until you allow them.'}
          {:else if res.how === 'scan'}Each person chooses their own name, so add {first} only if you expected this person.
          {:else}This code didn’t come from their screen, so Tinline can’t tell who sent it to you. Check with {first} another way before sharing anything private.{/if}
        </p>
      </div>
    {:else if tab === 'scan'}
      {#if cam === 'granted'}
        <div class="finder" role="img" aria-label="Camera view, simulated. Looking for a code." data-viewfinder>
          <i class="c tl"></i><i class="c tr"></i><i class="c bl"></i><i class="c br"></i><i class="sweep"></i>
        </div>
        <p class="why">Point the camera at the QR code on their screen.</p>
      {:else}
        <div class="denied" data-camera-denied>
          <Icon name="camera" size={28} />
          <b>Camera access is off</b>
          <p>Tinline can’t scan without the camera. You can allow it in system settings, or add someone with their code instead.</p>
          <div class="row2"><Btn variant="outline" onclick={() => (pp.perms.camera = 'granted')}>Open settings</Btn><Btn variant="primary" onclick={() => pick('paste')}>Paste a code</Btn></div>
        </div>
      {/if}
    {:else if tab === 'mine'}
      <div class="mine">
        <QrCode seed={77} size={176} label="Your QR code, a sample that can’t be scanned" />
        <code class="code" data-my-code>{MY_CODE}</code>
        <p class="why">Show this to someone next to you, or send them the code. They can ask to message you, and you answer each request.</p>
      </div>
    {:else}
      <form class="paste" onsubmit={(e) => { e.preventDefault(); lookup(); }}>
        <label class="fld"><span>Invite code</span>
          <input data-autofocus type="text" bind:value={text} placeholder="TIN-7K2M-Q4RX" autocomplete="off" autocapitalize="characters" spellcheck="false"
            aria-invalid={error ? 'true' : undefined} aria-describedby="ac-err" oninput={() => (error = '')} /></label>
        <p class="err" id="ac-err" role="alert" data-code-error>{error}</p>
      </form>
    {/if}
  </div>

  <div class="acts">
    {#if res}
      {#if added}
        <Btn variant="quiet" onclick={closeDialog}>Done</Btn><Btn variant="primary" onclick={message}>Message {first}</Btn>
      {:else if res.already}
        <Btn variant="quiet" onclick={closeDialog}>Close</Btn><Btn variant="primary" onclick={profile}>Open profile</Btn>
      {:else if res.blocked}
        <Btn variant="quiet" onclick={closeDialog}>Close</Btn><Btn variant="primary" onclick={() => { closeDialog(); nav.open({ name: 'privacy', section: 'blocked' }); }}>Open blocked people</Btn>
      {:else}
        <Btn variant="quiet" onclick={() => { res = null; error = ''; }}>{res.how === 'scan' ? 'Scan again' : 'Back'}</Btn><Btn variant="primary" data-add-confirm onclick={add}>Add {first}</Btn>
      {/if}
    {:else if tab === 'mine'}
      <Btn variant="quiet" onclick={closeDialog}>Close</Btn><Btn variant="outline" data-copy-code onclick={copy}><span class="cw">{copied ? 'Copied' : 'Copy code'}</span></Btn>
    {:else if tab === 'paste'}
      <Btn variant="quiet" onclick={closeDialog}>Cancel</Btn><Btn variant="primary" onclick={lookup} disabled={!text.trim()}>Look up</Btn>
    {:else}
      <Btn variant="quiet" onclick={closeDialog}>Cancel</Btn>
    {/if}
  </div>
</div>

<style>
  .ac { display: grid; gap: 12px; }
  .ac h2 { margin: 0; }
  .tabs { display: flex; gap: 4px; padding: 3px; border-radius: 20px; background: var(--surface-2); }
  .tab { flex: 1; min-block-size: 36px; border: 0; border-radius: 17px; background: transparent; color: var(--muted); font: inherit; font-weight: 600; font-size: var(--fs-sub); cursor: pointer; }
  .tab[aria-selected='true'] { background: var(--surface); color: var(--ink); box-shadow: var(--shadow-lift), inset 0 0 0 1px var(--hairline); }
  .body { block-size: 316px; display: grid; place-content: center; justify-items: center; text-align: center; gap: 10px; overflow: hidden; }
  .why { margin: 0; max-inline-size: 34ch; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; text-wrap: balance; }
  .finder { position: relative; inline-size: 220px; block-size: 220px; border-radius: 20px; background: #0e1513; overflow: hidden; }
  .c { position: absolute; inline-size: 34px; block-size: 34px; border: 3px solid #fff; opacity: .9; }
  .tl { inset: 16px auto auto 16px; border-width: 3px 0 0 3px; border-top-left-radius: 10px; }
  .tr { inset: 16px 16px auto auto; border-width: 3px 3px 0 0; border-top-right-radius: 10px; }
  .bl { inset: auto auto 16px 16px; border-width: 0 0 3px 3px; border-bottom-left-radius: 10px; }
  .br { inset: auto 16px 16px auto; border-width: 0 3px 3px 0; border-bottom-right-radius: 10px; }
  .sweep { position: absolute; inset-inline: 22px; inset-block-start: 22px; block-size: 2px; border-radius: 1px; background: var(--accent); box-shadow: 0 0 12px var(--accent); animation: sweep 1.6s ease-in-out infinite alternate; }
  :global(:root[data-motion='reduce']) .sweep { animation: none; inset-block-start: 50%; }
  @keyframes sweep { to { inset-block-start: calc(100% - 24px); } }
  .denied { display: grid; justify-items: center; gap: 6px; color: var(--muted); }
  .denied b { color: var(--ink); font-size: var(--fs-body); }
  .denied p { margin: 0; max-inline-size: 32ch; font-size: var(--fs-sub); line-height: 19px; }
  .row2 { display: flex; gap: 8px; margin-block-start: 8px; }
  .mine { display: grid; justify-items: center; gap: 10px; }
  .code { font: 600 17px/22px ui-monospace, 'SF Mono', Menlo, Consolas, monospace; letter-spacing: .06em; padding: 4px 12px; border-radius: 10px; background: var(--surface-2); user-select: all; }
  .paste { inline-size: 100%; display: grid; gap: 8px; align-content: center; text-align: start; }
  .fld { display: grid; gap: 6px; font-weight: 600; font-size: var(--fs-sub); }
  .fld input { min-block-size: var(--btn); padding: 0 14px; border-radius: var(--r-ctl); border: 1px solid var(--line); background: var(--surface); font: 500 var(--fs-body) ui-monospace, Menlo, Consolas, monospace; letter-spacing: .04em; color: var(--ink); }
  .fld input[aria-invalid='true'] { border-color: var(--danger); }
  .err { margin: 0; min-block-size: 38px; color: var(--danger); font-size: var(--fs-sub); line-height: 19px; font-weight: 400; }
  .found { display: grid; justify-items: center; gap: 8px; }
  .nm { font-size: 20px; line-height: 26px; font-weight: 650; }
  .state { display: inline-flex; align-items: center; gap: 6px; padding: 3px 10px; border-radius: 13px; background: var(--accent-soft); color: var(--accent-ink); font-size: var(--fs-sub); line-height: 20px; font-weight: 600; }
  .state.weak { background: var(--warn-bg); color: var(--warn-ink); }
  .acts { display: flex; gap: 8px; justify-content: flex-end; margin-block-start: 2px; }
  .cw { display: inline-block; min-inline-size: 76px; text-align: center; }
</style>
