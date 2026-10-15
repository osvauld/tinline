<script lang="ts">
  import { onDestroy, tick, untrack } from 'svelte';
  import { fade } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import QrCode from '../people/QrCode.svelte';
  import { db } from '../../lib/model/store.svelte';
  import { pp, DIRECTORY, resolveCode, requestSent, type Found } from '../../lib/model/actions/people.svelte';
  import { DEMO_KEY, DEMO_PHRASE, gateSim } from '../../lib/model/actions/gate.svelte';
  import { dur } from '../../lib/fx/motion';
  import { motion } from '../../lib/fx/reduced.svelte';

  type Page = 'restore' | 'review' | 'protect' | 'done' | 'profile' | 'auth' | 'backup' | 'share' | 'qr' | 'pickImage' | 'contactPreview' | 'requestSent';
  let { flow, rtl = false, sample = 0, denyClipboard = false }: {
    flow: 'restore' | 'profile' | 'share'; rtl?: boolean; sample?: number; denyClipboard?: boolean;
  } = $props();
  let page = $state<Page>(untrack(() => flow));
  let format = $state<'key' | 'words'>('key');
  let draft = $state('');
  let pass = $state('');
  let error = $state('');
  let copied = $state(false);
  let copying = $state(false);
  let sharing = $state(false);
  let candidate = $state<Extract<Found, { ok: true }> | null>(null);
  let shown = $state(false);
  let authenticated = $state(false);
  let heading = $state<HTMLHeadingElement>();
  // A security timeout, not animation timing. Never store recovery material in db/localStorage.
  const REVEAL_SECONDS = 30;
  let hideTimer: ReturnType<typeof setTimeout> | undefined;
  let generation = 0;
  let sampleSeen = 0;
  const titles: Record<Page, string> = {
    restore: 'Welcome back', review: 'Your account', protect: 'Protect this phone',
    done: 'Welcome back', profile: 'My profile', auth: 'Account recovery',
    backup: 'Your recovery backup', share: 'Your contact code', qr: 'Your contact code',
    pickImage: 'Choose image', contactPreview: 'Is this who you expected?', requestSent: 'Request sent',
  };
  const subtitles: Partial<Record<Page, string>> = {
    restore: 'Use your recovery key or 24 words.', review: 'The same identity, on this phone.',
    protect: 'Add a passphrase for this phone. You can do this later.',
    done: 'Your account is back.', auth: 'Check it’s you before opening your backup.',
    backup: 'Keep this private. Anyone with it can act as you.',
    share: 'Send it to someone you’d like to add.', qr: 'Let them scan this with Tinline.',
    pickImage: 'Recent images', contactPreview: 'Check before you connect.', requestSent: 'Your line opens when they accept.',
  };
  const backTo = $derived(({ review: 'restore', protect: 'review', auth: 'profile', backup: 'profile', qr: 'share',
    share: flow === 'profile' ? 'profile' : undefined, pickImage: 'qr', contactPreview: 'qr', requestSent: 'qr' } as Partial<Record<Page, Page>>)[page]);

  function hide() { shown = false; copied = false; copying = false; sharing = false; clearTimeout(hideTimer); generation++; }
  function lock() {
    hide(); pass = ''; draft = ''; error = ''; authenticated = false;
    if (page === 'backup') page = 'auth';
  }
  function go(next: Page) {
    hide(); error = ''; pass = ''; copying = false;
    if (page === 'restore') draft = '';
    if (next !== 'backup') authenticated = false;
    if (next !== 'contactPreview' && next !== 'requestSent') candidate = null;
    page = next;
  }
  function reveal() {
    if (!authenticated) return;
    shown = true; clearTimeout(hideTimer);
    hideTimer = setTimeout(hide, REVEAL_SECONDS * 1000);
  }
  function setFormat(next: 'key' | 'words') { hide(); error = ''; format = next; draft = ''; }
  function restore() {
    const normalized = draft.trim().split(/\s+/).join(' ');
    // Deliberately a fixture-only simulation. Real apps MUST use core checksum validation.
    const valid = format === 'key' ? normalized === DEMO_KEY : normalized.toLowerCase() === DEMO_PHRASE.join(' ');
    if (!valid) { error = `Check your ${format === 'key' ? 'recovery key' : '24 words'} and try again.`; return; }
    go('review');
  }
  function unlock() {
    if (gateSim.hasPassphrase && pass !== gateSim.passphrase) { error = 'That passphrase isn’t right.'; return; }
    go('backup'); authenticated = true;
  }
  async function copy(kind: 'contact' | 'recovery') {
    if (copying || (kind === 'recovery' && (!authenticated || !shown || format !== 'key'))) return;
    const current = ++generation;
    copying = true; copied = false; error = '';
    try {
      if (denyClipboard) throw new Error('Simulated clipboard refusal');
      await navigator.clipboard.writeText(kind === 'contact' ? pp.myCode : DEMO_KEY);
      if (current === generation) copied = true;
    } catch {
      if (current === generation) error = 'Couldn’t copy. Try again or select the text.';
    } finally { if (current === generation) copying = false; }
  }
  async function shareContact() {
    if (sharing || (page !== 'share' && page !== 'qr')) return;
    const current = ++generation;
    sharing = true; copied = false; error = '';
    try {
      if (!navigator.share) {
        error = 'Sharing isn’t available here. Copy your code instead.';
        return;
      }
      // Bare contact code only. Opening a chooser does not establish that anything was sent.
      await navigator.share({ text: pp.myCode });
    } catch (e) {
      if (current === generation && !(typeof e === 'object' && e !== null && 'name' in e && e.name === 'AbortError')) {
        error = 'Couldn’t open sharing. Copy your code instead.';
      }
    } finally { if (current === generation) sharing = false; }
  }
  function chooseImage(kind: 'contact' | 'none' | 'multiple') {
    // Synthetic OS-picker preview only; real decoding is in Android's ContactQrImagePicker.
    go('qr');
    if (kind === 'none') { error = 'No QR found. Try a clearer image.'; return; }
    if (kind === 'multiple') { error = 'More than one QR. Crop to the one you want.'; return; }
    const result = resolveCode(DIRECTORY[0].card);
    if (!result.ok) { error = result.error; return; }
    go('contactPreview'); candidate = result;
  }
  function confirmContact() {
    if (!candidate || candidate.blocked || candidate.already) return;
    requestSent(candidate); go('requestSent');
  }
  $effect(() => {
    if (sample !== sampleSeen) {
      sampleSeen = sample;
      if (page === 'restore') { draft = format === 'key' ? DEMO_KEY : DEMO_PHRASE.join(' '); error = ''; }
    }
  });
  $effect(() => {
    page;
    void tick().then(() => heading?.focus({ preventScroll: true }));
  });
  // Changing the auth method in the simulator cannot bypass an already-open gate.
  $effect(() => { gateSim.hasPassphrase; untrack(lock); });
  onDestroy(() => { clearTimeout(hideTimer); generation++; });
</script>

<svelte:document onvisibilitychange={() => { if (document.hidden) lock(); }} />
<svelte:window onblur={lock} />

<section class="phone" dir={rtl ? 'rtl' : 'ltr'} data-phone data-page={page} aria-label="Interactive mobile preview">
  <header class="appbar">
    {#if backTo}<button class="icon back" type="button" aria-label="Back" onclick={() => backTo && go(backTo)}><Icon name="back" size={22} /></button>{:else}<span class="brand">tinline</span>{/if}
    <span class="privacy">{page === 'backup' || page === 'auth' ? 'Only you' : ''}</span>
  </header>
  <main class="scroll">
    {#key page}
      <div class="stage" in:fade={{ duration: motion.reduced ? 0 : dur('crossfade'), easing: cubicOut }}>
        <div class="heading"><h2 bind:this={heading} tabindex="-1">{titles[page]}</h2>{#if subtitles[page]}<p class="lead">{subtitles[page]}</p>{/if}</div>
        {#if page === 'restore'}
          <form id="restore-form" onsubmit={(e) => { e.preventDefault(); restore(); }}>
            <div class="seg" role="group" aria-label="Restore with">
              <button type="button" aria-pressed={format === 'key'} onclick={() => setFormat('key')}>Recovery key</button>
              <button type="button" aria-pressed={format === 'words'} onclick={() => setFormat('words')}>24 words</button>
            </div>
            <label class="field-label" for="recovery-input">{format === 'key' ? 'Recovery key' : 'Recovery words'}</label>
            <textarea id="recovery-input" class:words-input={format === 'words'} dir="ltr" bind:value={draft} oninput={() => error = ''} placeholder={format === 'key' ? 'tlkey-…' : 'Your words, in order'} spellcheck="false" autocapitalize="off" autocomplete="off" aria-invalid={!!error} aria-describedby="form-status"></textarea>
            <p class="quiet">{format === 'key' ? 'One line, saved when you made your account.' : 'Separate each word with a space.'}</p>
            <details><summary>What comes back?</summary><p>Your identity comes back. The key itself doesn’t contain your messages. A linked device or an available backup may help bring those over.</p></details>
          </form>
        {:else if page === 'review' || page === 'done'}
          <div class="identity"><Avatar name={db.account.name} person="me" size={104} /><strong>{db.account.name}</strong><span>{page === 'done' ? 'Ready for the next step' : 'Recovered from your backup'}</span></div>
          {#if page === 'review'}<p class="quiet centered">This key restores your account, not a copy of your old messages.</p>{/if}
        {:else if page === 'protect'}
          <form id="protect-form" onsubmit={(e) => { e.preventDefault(); if (pass) go('done'); }}>
            <label class="field-label" for="new-pass">New passphrase</label>
            <input id="new-pass" type="password" bind:value={pass} autocomplete="new-password" />
            <p class="quiet">Your recovery key still works if you forget it.</p>
          </form>
        {:else if page === 'profile'}
          <div class="identity profile-identity"><Avatar name={db.account.name} person="me" size={104} /><strong>{db.account.name}</strong></div>
          <div class="rows"><button type="button" class="row" data-profile-share onclick={() => go('share')}><Icon name="userplus" size={22} /><span><b>Share my code</b><small>For people you’d like to add</small></span><span class="chevron"><Icon name="chevr" size={20} /></span></button></div>
          <p class="section-label">Only you</p>
          <div class="rows"><button type="button" class="row" data-profile-recovery onclick={() => go('auth')}><Icon name="key" size={22} /><span><b>Account recovery</b><small>Your key and 24 words</small></span><span class="chevron"><Icon name="chevr" size={20} /></span></button></div>
        {:else if page === 'auth'}
          <div class="motif"><Icon name="lock" size={36} /></div>
          <form id="auth-form" onsubmit={(e) => { e.preventDefault(); unlock(); }}>
            {#if gateSim.hasPassphrase}<label class="field-label" for="auth-pass">Your passphrase</label><input id="auth-pass" type="password" bind:value={pass} oninput={() => error = ''} autocomplete="current-password" />
            {:else}<p class="quiet centered">Use your phone’s screen lock to continue.</p>{/if}
          </form>
        {:else if page === 'backup'}
          <div class="seg" role="group" aria-label="Backup format"><button type="button" aria-pressed={format === 'key'} onclick={() => setFormat('key')}>Recovery key</button><button type="button" aria-pressed={format === 'words'} onclick={() => setFormat('words')}>24 words</button></div>
          <div class="secret" data-secret data-shown={shown}>
            {#if !shown}<div class="concealed"><Icon name="lock" size={26} /><span>Your {format === 'key' ? 'key is' : 'words are'} hidden</span></div>
            {:else if format === 'key'}<code dir="ltr" data-recovery-value>{DEMO_KEY}</code>
            {:else}<ol dir="ltr">{#each DEMO_PHRASE as word, i (i)}<li><span>{i + 1}</span>{word}</li>{/each}</ol>{/if}
          </div>
          <div class="reveal-bar"><button type="button" class="text" data-reveal onclick={() => { if (shown) hide(); else reveal(); }}><Icon name={shown ? 'eyeoff' : 'eye'} size={18} />{shown ? 'Hide' : format === 'key' ? 'Show key' : 'Show words'}</button><span class="quiet">Hides after {REVEAL_SECONDS} seconds</span></div>
          <p class="quiet">{format === 'key' ? 'Save it somewhere private, like a password manager.' : 'Write these down in order. Keep them somewhere private.'}</p>
          <details><summary>Key or words?</summary><p>Two ways to save the same backup. Either restores your account. Never send either to a contact or to support.</p></details>
        {:else if page === 'share' || page === 'qr'}
          <div class="identity share-identity"><Avatar name={db.account.name} person="me" size={48} /><strong dir="auto" data-username>{db.account.name}</strong></div>
          <div class="code-space">
            {#if page === 'qr'}<QrCode seed={77} size={232} label="Sample contact QR. This preview code cannot be scanned." />
            {:else}<div class="code-card"><Icon name="userplus" size={28} /><code dir="ltr" title={pp.myCode} data-contact-code>{pp.myCode}</code></div>{/if}
          </div>
          {#if page === 'qr'}<div class="qr-actions"><button class="text" type="button" onclick={() => go('share')}>Use a code instead</button><button class="secondary" data-choose-image type="button" onclick={() => go('pickImage')}>Choose image</button></div>
          {:else}<button class="text alternate" type="button" onclick={() => go('qr')}><Icon name="qr" size={18} />Show QR instead</button>{/if}
        {:else if page === 'pickImage'}
          <div class="gallery" aria-label="Simulated photo picker">
            <button type="button" data-image="contact" onclick={() => chooseImage('contact')}><span class="thumbnail"><QrCode seed={38} size={92} label="Sample screenshot with a contact QR" /></span><span>Contact QR</span></button>
            <button type="button" data-image="none" onclick={() => chooseImage('none')}><span class="thumbnail"><Icon name="image" size={44} /></span><span>A photo</span></button>
            <button type="button" data-image="multiple" onclick={() => chooseImage('multiple')}><span class="thumbnail"><Icon name="qr" size={32} /><Icon name="qr" size={32} /></span><span>Two codes</span></button>
          </div>
        {:else if (page === 'contactPreview' || page === 'requestSent') && candidate}
          <div class="identity"><Avatar name={candidate.name} person={candidate.id} hue={candidate.hue} size={104} /><strong dir="auto" data-found-username>{candidate.name}</strong><span>{page === 'requestSent' ? 'Waiting for them' : 'Read from image'}</span></div>
          {#if candidate.blocked}<p class="quiet centered">You blocked this account. Unblock them in Privacy first.</p>{:else if candidate.already}<p class="quiet centered">Already in your contacts.</p>{/if}
        {/if}
      </div>
    {/key}
  </main>
  <footer>
    <p id="form-status" class="status" role="status" aria-live="polite">{error || (copied ? (page === 'backup' ? 'Copied. Keep it somewhere private.' : 'Copied. Send it in any app.') : '')}</p>
    {#if page === 'restore'}<button class="primary" type="submit" form="restore-form" disabled={!draft.trim()}>Continue</button>
    {:else if page === 'review'}<button class="primary" type="button" onclick={() => go('protect')}>Use this account</button>
    {:else if page === 'protect'}<button class="primary" type="submit" form="protect-form" disabled={!pass}>Continue</button><button class="text" type="button" onclick={() => go('done')}>Not now</button>
    {:else if page === 'done'}<button class="primary" type="button" onclick={() => go('profile')}>Continue</button>
    {:else if page === 'auth'}<button class="primary" type="submit" form="auth-form" disabled={gateSim.hasPassphrase && !pass}>{gateSim.hasPassphrase ? 'Continue' : 'Use screen lock'}</button>
    {:else if page === 'backup'}{#if format === 'key'}<button class="primary" data-copy-key type="button" disabled={!shown || copying} onclick={() => copy('recovery')}><Icon name={copied ? 'check' : 'copy'} size={20} />{copied ? 'Copied' : 'Copy key'}</button>{:else}<button class="primary" type="button" onclick={() => go('profile')}>Done</button>{/if}
    {:else if page === 'share' || page === 'qr'}<div class="share-actions"><button class="primary" data-copy-contact type="button" disabled={copying} onclick={() => copy('contact')}><Icon name={copied ? 'check' : 'copy'} size={20} />{copied ? 'Copied' : 'Copy my code'}</button><button class="secondary" data-share-contact type="button" disabled={sharing} onclick={shareContact}>Share</button></div>
    {:else if page === 'pickImage'}<button class="text" type="button" onclick={() => go('qr')}>Cancel</button>
    {:else if page === 'contactPreview' && candidate}<button class="primary" data-confirm-contact type="button" disabled={candidate.blocked || candidate.already} onclick={confirmContact}>Add {candidate.name}</button>
    {:else if page === 'requestSent'}<button class="primary" type="button" onclick={() => go('qr')}>Done</button>{/if}
  </footer>
</section>

<style>
  .phone { inline-size: 100%; max-inline-size: 390px; block-size: 748px; display: flex; flex-direction: column; border: 1px solid var(--line); border-radius: 28px; background: var(--bg); overflow: hidden; box-shadow: var(--shadow-lift); }
  .appbar { flex: none; block-size: 60px; display: flex; align-items: center; justify-content: space-between; padding-inline: 16px; }
  .brand { font-size: 19px; font-weight: 650; letter-spacing: -.04em; }
  .privacy { font-size: var(--fs-sub); color: var(--muted); }
  button { border: 0; font: inherit; cursor: pointer; min-block-size: 44px; }
  button:disabled { opacity: .42; cursor: default; }
  button:focus-visible, summary:focus-visible { outline: 2px solid var(--accent); outline-offset: 3px; }
  h2:focus { outline: none; }
  .icon { inline-size: 44px; display: grid; place-items: center; background: transparent; border-radius: 50%; color: var(--ink); }
  .scroll { flex: 1; min-block-size: 0; overflow: auto; padding: 20px 24px 0; }
  .stage { min-inline-size: 0; padding-block-end: 12px; }
  h2 { font-size: 28px; line-height: 1.18; font-weight: 600; letter-spacing: -.035em; margin: 0; }
  .lead { color: var(--muted); font-size: 15px; line-height: 1.5; margin: 12px 0 0; max-inline-size: 32ch; text-wrap: pretty; }
  .heading { margin-block-end: 32px; }
  form { display: grid; }
  .seg { display: flex; gap: 4px; padding: 4px; background: var(--surface-2); border-radius: 16px; margin-block-end: 28px; }
  .seg button { flex: 1; padding-inline: 8px; border-radius: 12px; background: transparent; color: var(--muted); font-size: 14px; }
  .seg button[aria-pressed='true'] { background: var(--surface); color: var(--ink); box-shadow: var(--shadow-lift); font-weight: 600; }
  .field-label { font-size: 14px; font-weight: 600; margin-block-end: 10px; }
  textarea, input { inline-size: 100%; border: 1px solid var(--line); border-radius: 16px; background: var(--surface); color: var(--ink); padding: 14px; font: inherit; outline-offset: 3px; }
  textarea { resize: vertical; min-block-size: 112px; font: 15px/24px ui-monospace, monospace; overflow-wrap: anywhere; }
  textarea.words-input { min-block-size: 190px; }
  input { min-block-size: 52px; }
  .quiet { color: var(--muted); font-size: 13px; line-height: 1.55; }
  p.quiet { margin-block: 12px; }
  .centered { text-align: center; }
  details { color: var(--muted); font-size: 13px; line-height: 1.6; margin-block-start: 20px; }
  summary { min-block-size: 44px; cursor: pointer; display: list-item; align-content: center; }
  details p { margin-block: 0 12px; }
  .identity { display: flex; flex-direction: column; align-items: center; gap: 16px; padding-block: 28px; }
  .identity strong { font-size: 22px; font-weight: 600; letter-spacing: -.025em; min-inline-size: 0; max-inline-size: 100%; overflow-wrap: anywhere; }
  .identity > span { font-size: 14px; color: var(--muted); }
  .profile-identity { padding-block: 0 40px; }
  .share-identity { padding-block: 0; flex-direction: row; justify-content: center; gap: 12px; }
  .phone[data-page='share'] .heading, .phone[data-page='qr'] .heading { margin-block-end: 20px; }
  .rows { background: var(--surface); border: 1px solid var(--line); border-radius: 18px; }
  .row { display: flex; gap: 14px; align-items: center; inline-size: 100%; text-align: start; background: transparent; color: var(--ink); padding: 18px 16px; border-radius: inherit; }
  .row > span:not(.chevron) { flex: 1; min-inline-size: 0; }
  .row b { display: block; font-size: 15px; font-weight: 600; }
  .row small { display: block; margin-block-start: 4px; font-size: 13px; color: var(--muted); }
  .section-label { font-size: 13px; color: var(--muted); margin: 30px 4px 10px; }
  .chevron { color: var(--muted); }
  [dir='rtl'] .back, [dir='rtl'] .chevron { transform: scaleX(-1); }
  .motif { display: grid; place-items: center; block-size: 100px; margin-block-end: 24px; color: var(--accent-ink); }
  .secret { border: 1px solid var(--line); border-radius: 18px; background: var(--surface); padding: 20px; min-block-size: 140px; }
  .secret code { display: block; font: 17px/28px ui-monospace, monospace; overflow-wrap: anywhere; user-select: all; }
  .concealed { min-block-size: 98px; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 12px; color: var(--muted); font-size: 14px; }
  ol { display: grid; grid-template-columns: 1fr 1fr; gap: 8px 16px; list-style: none; margin: 0; padding: 0; }
  li { font-size: 14px; display: flex; gap: 10px; }
  li span { color: var(--muted); inline-size: 20px; font-variant-numeric: tabular-nums; font-size: 12px; }
  .reveal-bar { display: flex; align-items: center; justify-content: space-between; gap: 8px; margin-block-start: 10px; }
  .reveal-bar .quiet { font-size: 11px; }
  .text { background: transparent; color: var(--accent-ink); display: inline-flex; gap: 8px; align-items: center; justify-content: center; padding-inline: 4px; }
  .code-space { block-size: 240px; display: grid; place-items: center; margin-block-start: 8px; }
  .code-card { min-inline-size: 0; inline-size: 100%; display: grid; justify-items: center; gap: 16px; text-align: center; border: 1px solid var(--line); border-radius: 20px; padding: 24px; color: var(--muted); background: var(--surface); }
  .code-card code { max-inline-size: 100%; }
  .code-card code { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font: 12px/20px ui-monospace, monospace; }
  .alternate { display: flex; margin-inline: auto; }
  .qr-actions, .share-actions { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 8px; }
  .qr-actions { grid-template-columns: minmax(0, 1fr) auto; }
  .secondary { border: 1px solid var(--line); border-radius: 18px; padding-inline: 12px; background: var(--surface); color: var(--ink); }
  .share-actions .primary { gap: 6px; font-size: 14px; }
  .gallery { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 16px; }
  .gallery button { background: transparent; color: var(--ink); padding: 0; text-align: start; font-size: 13px; display: grid; gap: 8px; }
  .thumbnail { min-block-size: 132px; border: 1px solid var(--line); border-radius: 16px; background: var(--surface); display: flex; align-items: center; justify-content: center; gap: 8px; color: var(--muted); }
  footer { flex: none; padding: 0 24px 24px; display: grid; gap: 6px; background: var(--bg); }
  .status { color: var(--muted); font-size: 13px; line-height: 18px; min-block-size: 36px; margin: 0; padding-block-start: 6px; }
  .primary { inline-size: 100%; min-block-size: 52px; border-radius: 18px; background: var(--accent); color: var(--on-accent, #fff); font-weight: 600; display: flex; align-items: center; justify-content: center; gap: 10px; }
</style>
