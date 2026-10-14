<script lang="ts">
  /* Login lab (`?lab=accounts`): switching identity on the unlock screen, on the phone and on desktop. Before
     unlock nothing is open, so switching is only "which identity do I open"; nothing gets locked or paused.
     Today the surfaces disagree: desktop lists "Other accounts" under the field; Android has a "Switch account"
     text button to a confirm page; the playground has nothing. Three treatments of the same screen:
     A. Name in the title: "Welcome back, Maya ⌄"; tap the name for the identities (sheet on phone, menu on desktop).
        Picked 2026-10-14 for up to 3 identities. Identities have nicknames (no first/last name), so the title says
        the whole nickname and nothing repeats it.
     B. Avatar row: every identity's face in a row at the top; the selected one is large; tap another.
     C. Text button: "Switch account" under Unlock opens the same list (Android today).
     A remembered identity (phone key store / desktop keyring) opens as soon as it is picked.
     Pictures: before unlock the core reads only account.json (name, DID); a small clear picture would sit beside it
     (user OK, 2026-10-14). Messages, contacts and keys stay encrypted. */
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { dur } from '../../lib/fx/motion';

  type Acc = { name: string; hue: number; remembered: boolean };
  const ALL: Acc[] = [
    { name: 'Maya', hue: 2, remembered: false },
    { name: 'Studio', hue: 4, remembered: false },
    { name: 'Home', hue: 1, remembered: true },
  ];
  type Variant = 'chip' | 'row' | 'text';
  const VARIANTS: { v: Variant; label: string; how: string }[] = [
    { v: 'chip', label: 'A · Name in the title (picked)', how: 'Tap your name' },
    { v: 'row', label: 'B · Avatar row', how: 'Tap another face' },
    { v: 'text', label: 'C · Switch account button', how: 'Like Android today' },
  ];

  let theme = $state<'light' | 'dark'>('light');
  let form = $state<'phone' | 'desktop'>('phone');
  let count = $state<2 | 3>(2);
  $effect(() => { document.documentElement.dataset.theme = theme; });
  const accs = $derived(ALL.slice(0, count));
  const st = $state(VARIANTS.map(() => ({ cur: 0, list: false, pass: '', opened: '', fade: false })));
  $effect(() => { if (count < 3) st.forEach((s) => { if (s.cur >= count) s.cur = 0; }); });

  function pick(i: number, k: number) {
    const s = st[i];
    s.list = false;
    if (k === s.cur) return;
    s.fade = true;
    setTimeout(() => {
      s.cur = k; s.pass = ''; s.fade = false;
      if (accs[k].remembered) s.opened = accs[k].name;
    }, dur('crossfade'));
  }
  function unlock(i: number) { const s = st[i]; if (s.pass) { s.opened = accs[s.cur].name; s.pass = ''; } }
</script>

<main data-lab-form={form}>
  <header>
    <h1>Switching identity at login</h1>
    <p>The unlock screen with more than one identity on this {form === 'phone' ? 'phone' : 'computer'}. Pick another identity, then unlock it.</p>
    <div class="bar">
      <div class="seg" role="group" aria-label="Form factor">
        <button type="button" aria-pressed={form === 'phone'} onclick={() => (form = 'phone')}>Phone</button>
        <button type="button" aria-pressed={form === 'desktop'} onclick={() => (form = 'desktop')}>Desktop</button>
      </div>
      <div class="seg" role="group" aria-label="Identities">
        <button type="button" aria-pressed={count === 2} onclick={() => (count = 2)}>2 identities</button>
        <button type="button" aria-pressed={count === 3} onclick={() => (count = 3)}>3 identities</button>
      </div>
      <div class="seg" role="group" aria-label="Theme">
        <button type="button" aria-pressed={theme === 'light'} onclick={() => (theme = 'light')}>Light</button>
        <button type="button" aria-pressed={theme === 'dark'} onclick={() => (theme = 'dark')}>Dark</button>
      </div>
    </div>
    {#if count === 3}<p class="sub">Home is remembered here, so it opens right away.</p>{/if}
  </header>

  <div class="devices">
    {#each VARIANTS as vr, i (vr.v)}
      {@const s = st[i]}
      {@const me = accs[s.cur]}
      <figure>
        <figcaption><b>{vr.label}</b><small>{vr.how}</small></figcaption>
        <div class="dev" class:desk={form === 'desktop'}>
          {#if s.opened}
            <div class="opened">
              <Avatar name={s.opened} hue={ALL.find((a) => a.name === s.opened)?.hue ?? 0} size={56} />
              <b>{s.opened} is open</b>
              <small>(Its chats would be here.)</small>
              <button type="button" class="txt" onclick={() => (s.opened = '')}>Back to the login screen</button>
            </div>
          {:else}
            <form class="unlock" class:fade={s.fade} onsubmit={(e) => { e.preventDefault(); unlock(i); }}>
              {#if vr.v === 'row'}
                <div class="faces" role="radiogroup" aria-label="Identity">
                  {#each accs as a, k (a.name)}
                    <button type="button" role="radio" aria-checked={k === s.cur} aria-label={a.name} title={a.name} class="face press" class:sel={k === s.cur} onclick={() => pick(i, k)}>
                      <Avatar name={a.name} hue={a.hue} size={k === s.cur ? 72 : 44} />
                    </button>
                  {/each}
                </div>
              {:else}
                <Avatar name={me.name} hue={me.hue} size={72} />
              {/if}
              {#if vr.v === 'chip'}
                <span class="chipwrap">
                  <b class="wb">Welcome back, <button type="button" class="who-btn press" aria-label={`${me.name}, switch identity`} aria-haspopup={form === 'phone' ? 'dialog' : 'menu'} aria-expanded={s.list} onclick={() => (s.list = !s.list)}>{me.name}<Icon name="chev" size={20} /></button></b>
                  {#if s.list && form === 'desktop'}
                    <div class="menu" role="menu" aria-label="Open as">
                      {#each accs as a, k (a.name)}
                        <button type="button" role="menuitemradio" aria-checked={k === s.cur} class="row press" class:cur={k === s.cur} onclick={() => pick(i, k)}>
                          <Avatar name={a.name} hue={a.hue} size={32} /><span><b>{a.name}</b><small>{a.remembered ? 'Opens right away' : 'Asks for your passphrase'}</small></span>
                          {#if k === s.cur}<Icon name="check" size={16} />{/if}
                        </button>
                      {/each}
                      <div class="row"><Icon name="plus" size={18} /><span><b class="plain">Add another name</b></span></div>
                    </div>
                  {/if}
                </span>
              {:else}
                <b class="wb">Welcome back, {me.name}</b>
              {/if}
              <label class="fld"><span>Passphrase</span><input type="password" bind:value={s.pass} autocomplete="off" /></label>
              <div class="acts">
                <button type="submit" class="go" disabled={!s.pass}>Unlock</button>
                <button type="button" class="txt">I forgot it</button>
              </div>
              {#if vr.v === 'text'}<button type="button" class="txt" onclick={() => (s.list = true)}>Switch account</button>{/if}
            </form>
          {/if}

          {#if s.list && (form === 'phone' || vr.v === 'text')}
            <button type="button" class="scrim" aria-label="Close" onclick={() => (s.list = false)}></button>
            <div class="sheet" class:dialog={form === 'desktop'} role="dialog" aria-label="Identities">
              {#if form === 'phone'}<i class="grab" aria-hidden="true"></i>{/if}
              <p class="sh">Open as</p>
              {#each accs as a, k (a.name)}
                <button type="button" class="row press" class:cur={k === s.cur} onclick={() => pick(i, k)}>
                  <Avatar name={a.name} hue={a.hue} size={44} />
                  <span><b>{a.name}</b><small>{a.remembered ? 'Opens right away' : 'Asks for your passphrase'}</small></span>
                  {#if k === s.cur}<Icon name="check" size={18} />{/if}
                </button>
              {/each}
              <div class="row"><Icon name="plus" size={20} /><span><b class="plain">Add another name</b></span></div>
            </div>
          {/if}
        </div>
      </figure>
    {/each}
  </div>
</main>

<style>
  main { min-block-size: 100dvh; background: var(--canvas, var(--bg)); color: var(--ink); padding: 24px 16px 48px; display: grid; gap: 20px; justify-items: center; }
  header { text-align: center; max-inline-size: 760px; display: grid; gap: 10px; justify-items: center; }
  h1 { font-size: 20px; margin: 0; }
  header p { margin: 0; color: var(--muted); font-size: 14px; }
  header .sub { font-size: 13px; }
  .bar { display: flex; gap: 12px; flex-wrap: wrap; justify-content: center; }
  .seg { display: flex; gap: 4px; padding: 3px; border-radius: 12px; background: var(--surface-2); }
  .seg button { border: 0; padding: 7px 12px; min-block-size: 36px; border-radius: 9px; font: inherit; font-size: 13px; background: transparent; color: var(--ink); cursor: pointer; }
  .seg button[aria-pressed='true'] { background: var(--surface-raised, var(--bg)); box-shadow: var(--shadow-lift); }
  .devices { display: flex; flex-wrap: wrap; gap: 28px; justify-content: center; max-inline-size: 100%; }
  figure { margin: 0; display: grid; gap: 10px; justify-items: center; max-inline-size: 100%; }
  figcaption { display: grid; justify-items: center; font-size: 13px; gap: 2px; }
  figcaption small { color: var(--muted); }
  .dev { position: relative; inline-size: min(340px, calc(100vw - 32px)); block-size: 640px; border-radius: 28px; overflow: hidden; background: var(--bg); box-shadow: 0 0 0 1px var(--hairline), var(--shadow-lift); }
  /* Desktop: a window; the gate card keeps its phone-like column, centred, wider padding (StepShell desktop). */
  .dev.desk { inline-size: min(560px, calc(100vw - 32px)); block-size: 520px; border-radius: 12px; }
  /* Anchored from a fixed top (the app's .center.anchor), so a chip or a row never re-centres the rest. */
  .unlock { display: grid; justify-items: center; align-content: start; gap: 14px; padding: 96px 24px 24px; block-size: 100%; box-sizing: border-box; transition: opacity var(--dur-crossfade) var(--ease-out); }
  .desk .unlock { padding: 56px 40px 24px; inline-size: min(420px, 100%); margin-inline: auto; }
  .unlock.fade { opacity: .3; }
  .wb { font-size: 24px; }
  .desk .wb { font-size: 28px; }
  .faces { display: flex; align-items: center; gap: 10px; block-size: 76px; }
  .face { border: 0; padding: 0; background: transparent; border-radius: 50%; cursor: pointer; display: grid; place-items: center; min-inline-size: 44px; min-block-size: 44px; }
  .face:not(.sel) { opacity: .75; }
  .face:not(.sel):hover { opacity: 1; }
  .face.sel { cursor: default; }
  .chipwrap { position: relative; display: grid; justify-items: center; }
  /* The name is the switcher: same type as the title, a quiet chevron, a 44 px target. */
  .who-btn { display: inline-flex; align-items: center; gap: 2px; min-block-size: 44px; padding: 0 4px; margin: -6px -4px; border: 0; border-radius: 10px; background: transparent; color: inherit; font: inherit; cursor: pointer; }
  .who-btn :global(svg) { color: var(--muted); }
  .who-btn:hover { background: var(--surface); }
  .menu { position: absolute; inset-block-start: calc(100% + 6px); inset-inline-start: 50%; translate: -50% 0; z-index: 2; inline-size: 300px; padding: 6px; display: grid; gap: 2px; border-radius: 14px; background: var(--surface-raised, var(--bg)); box-shadow: var(--shadow-pop); animation: rise var(--dur-enter) var(--ease-out); }
  .menu .row { min-block-size: 44px; padding: 6px 10px; }
  .fld { display: grid; gap: 6px; inline-size: 100%; font-size: 13px; font-weight: 600; margin-block-start: 6px; }
  .fld input { block-size: 48px; border-radius: var(--r-field, 12px); border: 1.5px solid var(--line); background: var(--bg); color: var(--ink); padding: 0 12px; font: inherit; }
  .acts { display: grid; gap: 4px; inline-size: 100%; justify-items: center; }
  .desk .acts { display: flex; flex-direction: row-reverse; justify-content: flex-start; gap: 12px; }
  .go { inline-size: 100%; block-size: 48px; border: 0; border-radius: var(--r-ctl, 14px); background: var(--btn, var(--accent)); color: var(--on-accent); font: inherit; font-weight: 600; cursor: pointer; }
  .desk .go { inline-size: auto; min-inline-size: 160px; block-size: 44px; }
  .go:disabled { background: var(--surface-2); color: var(--muted); cursor: default; }
  .txt { border: 0; background: transparent; color: var(--accent-ink, var(--accent)); font: inherit; font-weight: 600; cursor: pointer; padding: 8px; min-block-size: 44px; }
  .opened { display: grid; justify-items: center; align-content: center; gap: 10px; block-size: 100%; padding: 24px; text-align: center; }
  .opened small { color: var(--muted); }
  .scrim { position: absolute; inset: 0; border: 0; background: rgb(0 0 0 / .28); animation: fade var(--dur-enter) var(--ease-out); }
  .sheet { position: absolute; inset-inline: 0; inset-block-end: 0; background: var(--surface-raised, var(--bg)); border-radius: var(--r-sheet, 24px) var(--r-sheet, 24px) 0 0; padding: 8px 10px 18px; display: grid; gap: 2px; box-shadow: var(--shadow-sheet); animation: rise var(--dur-enter) var(--ease-out); }
  .sheet.dialog { inset: auto; inset-block-start: 50%; inset-inline-start: 50%; translate: -50% -50%; inline-size: 340px; border-radius: 16px; padding: 10px; }
  .grab { justify-self: center; inline-size: 36px; block-size: 4px; border-radius: 2px; background: var(--hairline); margin-block-end: 6px; }
  .sh { margin: 4px 12px 6px; font-size: 13px; color: var(--muted); }
  .row { display: flex; align-items: center; gap: 12px; padding: 10px 12px; min-block-size: 56px; border: 0; background: transparent; color: var(--ink); font: inherit; text-align: start; border-radius: 12px; inline-size: 100%; box-sizing: border-box; }
  button.row { cursor: pointer; }
  button.row:hover { background: var(--surface); }
  .row.cur { background: var(--surface); }
  .row span { display: grid; flex: 1; min-inline-size: 0; }
  .row small { color: var(--muted); font-size: 13px; }
  .plain { font-weight: 400; }
  @keyframes rise { from { transform: translateY(12px); opacity: 0; } }
  @keyframes fade { from { opacity: 0; } }
  :global(:root[data-motion='reduce']) .unlock, :global(:root[data-motion='reduce']) .sheet, :global(:root[data-motion='reduce']) .menu, :global(:root[data-motion='reduce']) .scrim { animation: none; transition: none; }
</style>
